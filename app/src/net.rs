//! The network side. Runs on tokio and talks to the UI thread over channels, so gpui never blocks on I/O.

use dc_gateway::{Command, Config, Event, spawn};
use dc_http::Client;
use dc_proto::identity::{IdentifyState, Identity};
use dc_proto::op::Op;
use dc_proto::secrets::Token;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::warn;

pub enum Update {
    Dispatch { name: String, data: Value },
    Messages { channel: u64, page: Value },
    Image { key: String, bytes: Vec<u8> },
}

pub enum Request {
    LoadMessages { channel: u64 },
    Send { channel: u64, content: String },
    Subscribe { guild: u64, channel: u64 },
    Image { key: String, url: String },
}

const MESSAGE_PAGE: u32 = 50;

pub async fn run(
    token: Token,
    updates: mpsc::UnboundedSender<Update>,
    mut requests: mpsc::UnboundedReceiver<Request>,
) {
    let identity = Identity::new();
    let client = match Client::new(token.clone(), identity.clone()) {
        Ok(client) => Arc::new(client),
        Err(error) => {
            warn!("http client failed: {error}");
            return;
        }
    };
    let (commands, mut events) = spawn(Config {
        token,
        identity,
        state: IdentifyState::default(),
    });

    let forward = updates.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                Event::Dispatch { name, data } => {
                    let _ = forward.send(Update::Dispatch { name, data });
                }
                Event::AuthenticationFailed => warn!("gateway rejected the token"),
                Event::Reconnecting { delay } => warn!("gateway reconnecting in {delay:?}"),
                Event::Connected | Event::Closed { .. } => {}
            }
        }
    });

    while let Some(request) = requests.recv().await {
        let client = client.clone();
        let updates = updates.clone();
        match request {
            Request::LoadMessages { channel } => {
                tokio::spawn(async move {
                    let route = format!("/channels/{channel}/messages?limit={MESSAGE_PAGE}");
                    match client.get(&route).await {
                        Ok(page) => {
                            let _ = updates.send(Update::Messages { channel, page });
                        }
                        Err(error) => warn!("loading messages failed: {error}"),
                    }
                });
            }
            Request::Send { channel, content } => {
                tokio::spawn(async move {
                    let body = json!({"content": content, "tts": false, "flags": 0});
                    if let Err(error) = client
                        .post(&format!("/channels/{channel}/messages"), &body)
                        .await
                    {
                        warn!("sending failed: {error}");
                    }
                });
            }
            Request::Subscribe { guild, channel } => {
                let subscription = json!({"typing": true, "activities": true, "threads": true, "channels": {channel.to_string(): [[0, 99]]}});
                let data = json!({"subscriptions": {guild.to_string(): subscription}});
                let _ = commands.send(Command::Send {
                    op: Op::GuildSubscriptionsBulk,
                    data,
                });
            }
            Request::Image { key, url } => {
                tokio::spawn(async move {
                    match client.fetch_public(&url).await {
                        Ok(bytes) => {
                            let _ = updates.send(Update::Image { key, bytes });
                        }
                        Err(error) => warn!("image failed: {error}"),
                    }
                });
            }
        }
    }
}
