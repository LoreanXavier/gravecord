//! The app's side of the local RPC server (`dc-rpc`): the REST questions it asks (`Directory`), the account it
//! speaks for, and the events it hands to the UI. The server starts with the network task, so it lives as long as the
//! login. A harness or capture run never starts it (`GRAVECORD_ALLOW_PRESENCE=1` allows it, as it allows a presence
//! update), so no game can reach a capture run's account.

use crate::net::Update;
use dc_http::Client;
use dc_rpc::{BoxFuture, Config, Directory, Server};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// The current user as `READY` carries it (module 639621), set by the UI once the account is known.
static USER: Mutex<Option<Value>> = Mutex::new(None);
/// `allowActivityPartyPrivacyFriends` and `allowActivityPartyPrivacyVoiceChannel`.
static PARTY_FRIENDS: AtomicBool = AtomicBool::new(true);
static PARTY_VOICE: AtomicBool = AtomicBool::new(true);

/// The user object of module 639621 for the account.
pub fn user_json(user: &dc_state::User) -> Value {
    json!({
        "id": user.id.to_string(),
        "username": user.username,
        "discriminator": user.discriminator,
        "global_name": user.global_name,
        "avatar": user.avatar,
        "avatar_decoration_data": null,
        "bot": user.bot,
        "flags": user.public_flags,
        "public_flags": user.public_flags,
        "premium_type": 0,
    })
}

pub fn set_user(user: Option<Value>) {
    *USER.lock().expect("rpc user") = user;
}

pub fn set_party_privacy(friends: bool, voice: bool) {
    PARTY_FRIENDS.store(friends, Ordering::Relaxed);
    PARTY_VOICE.store(voice, Ordering::Relaxed);
}

struct Rest(Arc<Client>);

impl Directory for Rest {
    fn application(&self, id: &str) -> BoxFuture<'_, Option<Value>> {
        let route = format!("/oauth2/applications/{id}/rpc");
        Box::pin(async move { self.0.get(&route).await.ok() })
    }

    fn assets(&self, id: &str) -> BoxFuture<'_, Option<Vec<Value>>> {
        let route = format!("/oauth2/applications/{id}/assets");
        Box::pin(async move {
            match self.0.get(&route).await.ok()? {
                Value::Array(assets) => Some(assets),
                _ => None,
            }
        })
    }

    fn external_assets(&self, id: &str, urls: Vec<String>) -> BoxFuture<'_, Option<Vec<Value>>> {
        let route = format!("/applications/{id}/external-assets");
        Box::pin(async move {
            match self.0.post(&route, &json!({ "urls": urls })).await.ok()? {
                Value::Array(found) => Some(found),
                _ => None,
            }
        })
    }

    fn current_user(&self) -> Option<Value> {
        USER.lock().expect("rpc user").clone()
    }

    fn party_privacy(&self) -> (bool, bool) {
        (
            PARTY_FRIENDS.load(Ordering::Relaxed),
            PARTY_VOICE.load(Ordering::Relaxed),
        )
    }
}

/// Starts the pipe server unless this is a harness run. The returned handle keeps it alive: dropping it (logging
/// out) closes every client and frees the pipe.
pub fn start(client: Arc<Client>, updates: mpsc::UnboundedSender<Update>) -> Option<Server> {
    if crate::storage::is_capture_run() && !crate::games::live_presence_allowed() {
        return None;
    }
    let (events, mut received) = mpsc::unbounded_channel();
    let server = match Server::start(Arc::new(Rest(client)), events, Config::default()) {
        Ok(server) => server,
        Err(error) => {
            tracing::warn!(%error, "RPC pipe unavailable");
            return None;
        }
    };
    tracing::info!(pipe = server.pipe_name(), "RPC server started");
    tokio::spawn(async move {
        while let Some(event) = received.recv().await {
            if updates.send(Update::Rpc(event)).is_err() {
                break;
            }
        }
    });
    Some(server)
}
