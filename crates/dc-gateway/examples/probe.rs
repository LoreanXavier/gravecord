//! Connects with `secrets.toml`, waits for READY and prints counts only. No window, no message content, no ids.

use anyhow::{Context, Result, bail};
use dc_gateway::{Command, Config, Event, spawn};
use dc_proto::identity::{IdentifyState, Identity};
use dc_proto::secrets;
use std::path::Path;
use std::time::Duration;

fn length(value: &serde_json::Value, key: &str) -> usize {
    value[key].as_array().map_or(0, Vec::len)
}

#[tokio::main]
async fn main() -> Result<()> {
    let token = secrets::load(Path::new("secrets.toml")).context("loading secrets.toml")?;
    let (commands, mut events) = spawn(Config {
        token,
        identity: Identity::new(),
        state: IdentifyState::default(),
    });
    let run = async {
        let mut ready = false;
        while let Some(event) = events.recv().await {
            match event {
                Event::Connected => println!("connected"),
                Event::Dispatch { name, data } if name == "READY" => {
                    ready = true;
                    println!(
                        "READY: {} keys, {} guilds, {} private channels, {} relationships, resume url {}",
                        data.as_object().map_or(0, serde_json::Map::len),
                        length(&data, "guilds"),
                        length(&data, "private_channels"),
                        length(&data, "relationships"),
                        data["resume_gateway_url"].is_string(),
                    );
                }
                Event::Dispatch { name, data } if name == "READY_SUPPLEMENTAL" => {
                    println!("READY_SUPPLEMENTAL: {} guilds", length(&data, "guilds"));
                    return Ok(());
                }
                Event::Dispatch { name, .. } => println!("dispatch {name}"),
                Event::Closed { code, .. } => println!("closed {code}"),
                Event::Reconnecting { delay } => println!("reconnecting in {delay:?}"),
                Event::AuthenticationFailed => bail!("authentication failed"),
                Event::Reset => println!("session reset"),
            }
        }
        bail!("stream ended, ready: {ready}")
    };
    let result = tokio::time::timeout(Duration::from_secs(30), run).await;
    let _ = commands.send(Command::Shutdown);
    tokio::time::sleep(Duration::from_millis(300)).await;
    result.context("timed out")?
}
