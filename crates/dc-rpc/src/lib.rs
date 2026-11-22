//! The local RPC server games talk to: the named pipe `\\.\pipe\discord-ipc-N` (the first free `N` of 0 to 9),
//! the frame protocol, the handshake, `SET_ACTIVITY` and the removal of a client's activity when its pipe closes.
//! Follows the official client's RPC server (modules 998921, 223769, 596339, 90924, 913659) and the desktop side
//! (`discord_rpc` `RPCIPC.js`). No UI: the host reads `Event`s and answers the REST questions through `Directory`.

mod activity;
mod commands;
mod directory;
mod frame;
#[cfg(windows)]
mod pipe;
mod session;
mod validate;

pub use activity::{Outcome, activity_flags, activity_join_dispatch_frame, flags};
pub use directory::{BoxFuture, Cache, Directory};
pub use frame::{Decoder, FrameError, MAX_BUFFER, MAX_MESSAGE, Opcode, encode};
pub use session::{HANDSHAKE_TIMEOUT, serve};
pub use validate::set_activity as validate_set_activity;

use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

/// The RPC error codes (`Lw`/`Lw6` in module 818348) the server answers with.
pub mod codes {
    pub const INVALID_PAYLOAD: i64 = 4000;
    pub const INVALID_COMMAND: i64 = 4002;
    pub const INVALID_PERMISSIONS: i64 = 4006;
    pub const INVALID_ACTIVITY_SECRET: i64 = 5005;
}

/// The socket close codes (`YI`/`YI$`).
pub mod close_codes {
    pub const NORMAL: i64 = 1000;
    pub const UNSUPPORTED: i64 = 1003;
    pub const ABNORMAL: i64 = 1006;
    pub const INVALID_CLIENTID: i64 = 4000;
    pub const INVALID_VERSION: i64 = 4004;
}

/// An error the server answers a request with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// What the server tells the host: the dispatches the official server makes on the Flux dispatcher.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `RPC_APP_CONNECTED`: a client passed the handshake. `application` is its `/rpc` description.
    Connected { socket: u64, application: Value },
    /// `LOCAL_ACTIVITY_UPDATE`: the activity of a socket was set, or cleared (`activity: None`).
    Activity {
        socket: u64,
        pid: Option<i64>,
        application_id: Option<String>,
        activity: Option<Value>,
        party_privacy: Option<i64>,
    },
    /// `RPC_APP_DISCONNECTED`: the socket is gone, whatever its state.
    Disconnected { socket: u64 },
}

/// Limits of the server; `Config::default` is the official client's.
#[derive(Clone)]
pub struct Config {
    /// Pipe names to try in order (the first free one is served).
    pub pipe_names: Vec<String>,
    /// `MAX_CONNECTIONS`.
    pub max_connections: usize,
    /// How long a client may stay silent before its handshake.
    pub handshake_timeout: Duration,
    /// Handshakes per application per `throttle_window` (`new RateLimiter(2, 60 s)`).
    pub throttle_capacity: u32,
    pub throttle_window: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pipe_names: (0..10)
                .map(|index| format!(r"\\.\pipe\discord-ipc-{index}"))
                .collect(),
            max_connections: 100,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            throttle_capacity: 2,
            throttle_window: Duration::from_secs(60),
        }
    }
}

/// Module 898467: a token bucket per client id. A handshake waits for a token instead of failing.
pub struct Throttle {
    capacity: u32,
    interval: Duration,
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl Throttle {
    pub fn new(capacity: u32, window: Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            interval: window / capacity.max(1),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub async fn acquire(&self, key: &str) {
        loop {
            let wait = {
                let mut buckets = self.buckets.lock().expect("throttle");
                let now = Instant::now();
                let capacity = f64::from(self.capacity);
                let bucket = buckets.entry(key.to_owned()).or_insert((capacity, now));
                let refill =
                    now.duration_since(bucket.1).as_secs_f64() / self.interval.as_secs_f64();
                bucket.0 = (bucket.0 + refill).min(capacity);
                bucket.1 = now;
                if bucket.0 >= 1.0 {
                    bucket.0 -= 1.0;
                    return;
                }
                self.interval.mul_f64(1.0 - bucket.0)
            };
            tokio::time::sleep(wait).await;
        }
    }
}

/// State shared by every connection of a server.
pub struct Shared {
    pub(crate) dir: Arc<dyn Directory>,
    pub(crate) cache: Cache,
    pub(crate) throttle: Throttle,
    pub(crate) events: mpsc::UnboundedSender<Event>,
    pub(crate) config: Config,
    pub(crate) next_socket: AtomicU64,
    pub(crate) active: AtomicUsize,
    pub(crate) clock: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl Shared {
    pub fn new(
        dir: Arc<dyn Directory>,
        events: mpsc::UnboundedSender<Event>,
        config: Config,
    ) -> Self {
        Self {
            dir,
            cache: Cache::default(),
            throttle: Throttle::new(config.throttle_capacity, config.throttle_window),
            events,
            config,
            // `_.uniqueId()` counts up from 1.
            next_socket: AtomicU64::new(1),
            active: AtomicUsize::new(0),
            clock: Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_millis() as u64)
            }),
        }
    }

    pub(crate) fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    /// Replaces the clock (tests).
    pub fn with_clock(mut self, clock: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }
}

/// A running pipe server.
pub struct Server {
    name: String,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    /// Binds the first free pipe of `config.pipe_names` and serves it on the current Tokio runtime. A pipe another
    /// process serves is never opened or probed: it is skipped. Fails on a platform without named pipes.
    pub fn start(
        dir: Arc<dyn Directory>,
        events: mpsc::UnboundedSender<Event>,
        config: Config,
    ) -> std::io::Result<Self> {
        #[cfg(windows)]
        {
            let shared = Arc::new(Shared::new(dir, events, config));
            let (stop, watcher) = watch::channel(false);
            let (name, task) = pipe::bind(shared, watcher)?;
            Ok(Self { name, stop, task })
        }
        #[cfg(not(windows))]
        {
            let _ = (dir, events, config);
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "the RPC pipe is only served on Windows",
            ))
        }
    }

    /// The pipe being served.
    pub fn pipe_name(&self) -> &str {
        &self.name
    }

    /// Closes every client (`User logged out`) and releases the pipe.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}
