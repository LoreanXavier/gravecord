//! The Windows named pipe: the first free `discord-ipc-N` is created (never opened: a pipe somebody else serves is
//! skipped without a probe), and each client gets its own instance of it. Plumbing after Serein's
//! `crates/platform/src/game_activity.rs` (Tokio named pipes, one instance kept ready, remote clients refused).

use crate::{Shared, session};
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};

fn create(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    ServerOptions::new()
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .in_buffer_size(16 * 1024)
        .out_buffer_size(16 * 1024)
        .create(name)
}

/// Creates the first free pipe and starts its accept loop.
pub fn bind(
    shared: Arc<Shared>,
    stop: watch::Receiver<bool>,
) -> io::Result<(String, JoinHandle<()>)> {
    let mut last = None;
    for name in &shared.config.pipe_names {
        match create(name, true) {
            Ok(listener) => {
                let name = name.clone();
                let task = tokio::spawn(accept_loop(name.clone(), listener, shared, stop));
                return Ok((name, task));
            }
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("no pipe name to try")))
}

async fn accept_loop(
    name: String,
    mut listener: NamedPipeServer,
    shared: Arc<Shared>,
    mut stop: watch::Receiver<bool>,
) {
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            connected = listener.connect() => {
                if connected.is_err() {
                    break;
                }
                // The next instance is created before the connected one is handed over, so the name is never free.
                let Ok(next) = create(&name, false) else { break };
                let client = std::mem::replace(&mut listener, next);
                let shared = shared.clone();
                let stop = stop.clone();
                clients.spawn(async move { session::serve(client, &shared, stop).await });
            }
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
            }
        }
    }
    drop(listener);
    // Each client sees the stop flag, says goodbye and ends.
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        while clients.join_next().await.is_some() {}
    })
    .await;
    clients.shutdown().await;
}
