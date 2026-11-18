//! Offline replay of Discord for the capture harness (`cargo xtask perf --replay <folder>`).
//!
//! Two switches, both set only by a capture build (`app/src/main.rs`); a normal build never sets either, and then
//! everything here is a no-op:
//!
//! - **Playback** (`GRAVECORD_REPLAY=<url of the local server>`): [`rewrite`] turns every Discord URL the app asks for
//!   (`https://discord.com/api/...`, the CDN, `wss://gateway.discord.gg/...`) into one on the local server
//!   (`http://127.0.0.1:<port>/<host>/<path>`), which answers from a recording (`xtask/src/replay.rs`).
//! - **Recording** (`GRAVECORD_RECORD=<folder>`): what the real gateway and REST answered is written to the folder as it
//!   arrives, one record at a time and flushed, so a run killed at its end keeps everything. Request headers are never
//!   kept; every value under a key that names a token, a session id or a secret is replaced ([`sanitize`]), and so are
//!   such query parameters of the URLs ([`normalize_url`]).
//!
//! The folder holds `http.jsonl` (one line per answer: when it was asked, method, URL, status, content type, the time
//! it took, the body's file under `http/`) and `gateway.jsonl` (one line per frame: the time since the recorder started, the direction,
//! and the payload, already decoded).

use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// What a redacted value is replaced with.
pub const REDACTED: &str = "replay-redacted";

static ENDPOINT: OnceLock<String> = OnceLock::new();
static RECORDER: OnceLock<Recorder> = OnceLock::new();
static STAGE: OnceLock<fn(&'static str, Duration)> = OnceLock::new();

/// Points the app at a local replay server (`http://127.0.0.1:<port>`). Once per process.
pub fn set_endpoint(url: &str) {
    let _ = ENDPOINT.set(url.trim_end_matches('/').to_owned());
}

/// The local replay server, when this run plays one back.
pub fn endpoint() -> Option<&'static str> {
    ENDPOINT.get().map(String::as_str)
}

/// `url` as this run must ask for it: unchanged, or on the replay server with its host as the first path segment.
pub fn rewrite(url: &str) -> std::borrow::Cow<'_, str> {
    let Some(base) = endpoint() else {
        return url.into();
    };
    rewrite_onto(base, url).map_or(url.into(), Into::into)
}

fn rewrite_onto(base: &str, url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let local = match scheme {
        "https" | "http" => base.to_owned(),
        "wss" | "ws" => base.replacen("http", "ws", 1),
        _ => return None,
    };
    if url.starts_with(&local) {
        return None;
    }
    Some(format!("{local}/{rest}"))
}

/// The URL a request of this run stands for on Discord: the inverse of [`rewrite`] (a path of the replay server
/// becomes `https://<host>/<path>`), with token-like query values redacted as the recorder keeps them. The key the
/// server looks answers up by.
pub fn normalize_url(url: &str) -> String {
    let url = match url.strip_prefix('/') {
        Some(rest) => format!("https://{rest}"),
        None => url.to_owned(),
    };
    let Some((path, query)) = url.split_once('?') else {
        return url;
    };
    let pairs: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) if is_secret_key(name) => format!("{name}={REDACTED}"),
            _ => pair.to_owned(),
        })
        .collect();
    format!("{path}?{}", pairs.join("&"))
}

/// A key whose value is never kept: tokens of any kind, session ids, secrets.
pub fn is_secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("token") || key.contains("session_id") || key.contains("secret")
}

/// Replaces every value under a secret key (at any depth), and the gateway's resume URL, so a recording holds no
/// credential and no live session.
pub fn sanitize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                if is_secret_key(key) {
                    *item = match item {
                        Value::Null => Value::Null,
                        Value::String(_) => Value::String(REDACTED.to_owned()),
                        _ => Value::Null,
                    };
                } else if key == "resume_gateway_url" {
                    *item = Value::String("wss://replay-gateway.invalid".to_owned());
                } else {
                    sanitize(item);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(sanitize),
        _ => {}
    }
}

/// Lets the app time stages that happen inside the network crates (the gateway's READY frame read, inflated,
/// unpacked): `hook(stage, how long ago it happened)`.
pub fn set_stage_hook(hook: fn(&'static str, Duration)) {
    let _ = STAGE.set(hook);
}

/// Marks a stage of the start that happened `ago` before now (no-op without a hook).
pub fn stage(name: &'static str, ago: Duration) {
    if let Some(hook) = STAGE.get() {
        hook(name, ago);
    }
}

enum Entry {
    Http {
        /// When the request was sent, since the recorder started.
        at_ms: u64,
        method: String,
        url: String,
        status: u16,
        content_type: Option<String>,
        elapsed_ms: u64,
        body: Vec<u8>,
    },
    Gateway {
        at_ms: u64,
        direction: &'static str,
        payload: Value,
    },
}

struct Recorder {
    started: Instant,
    sender: std::sync::Mutex<mpsc::Sender<Entry>>,
}

/// Starts recording into `dir` (created if needed). Once per process.
pub fn start_recording(dir: PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("http"))?;
    let open = |name: &str| {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(name))
    };
    let mut http = open("http.jsonl")?;
    let mut gateway = open("gateway.jsonl")?;
    // Several runs may record into one folder: bodies are numbered after what is there.
    let mut next = std::fs::read_dir(dir.join("http"))?.count();
    let (sender, receiver) = mpsc::channel::<Entry>();
    std::thread::Builder::new()
        .name("replay-recorder".to_owned())
        .spawn(move || {
            for entry in receiver {
                let _ = write_entry(&dir, &mut http, &mut gateway, &mut next, entry);
            }
        })?;
    let _ = RECORDER.set(Recorder {
        started: Instant::now(),
        sender: std::sync::Mutex::new(sender),
    });
    Ok(())
}

/// Whether this run records.
pub fn recording() -> bool {
    RECORDER.get().is_some()
}

fn send(entry: Entry) {
    if let Some(recorder) = RECORDER.get()
        && let Ok(sender) = recorder.sender.lock()
    {
        let _ = sender.send(entry);
    }
}

/// One answer of a REST or CDN request, as it arrived.
pub fn record_http(
    method: &str,
    url: &str,
    status: u16,
    content_type: Option<&str>,
    elapsed: Duration,
    body: &[u8],
) {
    let Some(recorder) = RECORDER.get() else {
        return;
    };
    let at_ms = recorder
        .started
        .elapsed()
        .saturating_sub(elapsed)
        .as_millis() as u64;
    send(Entry::Http {
        at_ms,
        method: method.to_owned(),
        url: url.to_owned(),
        status,
        content_type: content_type.map(str::to_owned),
        elapsed_ms: elapsed.as_millis() as u64,
        body: body.to_vec(),
    });
}

/// A gateway frame the server sent, decoded; `read` is when its bytes came off the socket.
pub fn record_gateway_recv(read: Instant, payload: &Value) {
    let Some(recorder) = RECORDER.get() else {
        return;
    };
    let at_ms = read.saturating_duration_since(recorder.started).as_millis() as u64;
    send(Entry::Gateway {
        at_ms,
        direction: "recv",
        payload: payload.clone(),
    });
}

/// A gateway frame this client sent (`op`, `data`), or a connection event (`connect`, with `Value::Null`).
pub fn record_gateway_send(direction: &'static str, op: Option<u64>, data: &Value) {
    let Some(recorder) = RECORDER.get() else {
        return;
    };
    send(Entry::Gateway {
        at_ms: recorder.started.elapsed().as_millis() as u64,
        direction,
        payload: json!({"op": op, "d": data}),
    });
}

fn write_entry(
    dir: &std::path::Path,
    http: &mut File,
    gateway: &mut File,
    next: &mut usize,
    entry: Entry,
) -> std::io::Result<()> {
    match entry {
        Entry::Http {
            at_ms,
            method,
            url,
            status,
            content_type,
            elapsed_ms,
            body,
        } => {
            let json_body = content_type
                .as_deref()
                .is_some_and(|kind| kind.contains("json"));
            let body = if json_body {
                match serde_json::from_slice::<Value>(&body) {
                    Ok(mut value) => {
                        sanitize(&mut value);
                        serde_json::to_vec(&value).unwrap_or_default()
                    }
                    Err(_) => body,
                }
            } else {
                body
            };
            let file = format!("http/{next:06}.bin");
            *next += 1;
            // The body first: an index line never points at a file that is not there.
            std::fs::write(dir.join(&file), &body)?;
            let line = json!({
                "at_ms": at_ms,
                "method": method,
                "url": normalize_url(&url),
                "status": status,
                "content_type": content_type,
                "elapsed_ms": elapsed_ms,
                "file": file,
            });
            writeln!(http, "{line}")?;
            http.flush()
        }
        Entry::Gateway {
            at_ms,
            direction,
            mut payload,
        } => {
            sanitize(&mut payload);
            let line = json!({"at_ms": at_ms, "dir": direction, "payload": payload});
            writeln!(gateway, "{line}")?;
            gateway.flush()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_move_onto_the_local_server_and_back() {
        let base = "http://127.0.0.1:4100";
        assert_eq!(
            rewrite_onto(base, "https://discord.com/api/v9/users/@me?x=1").unwrap(),
            "http://127.0.0.1:4100/discord.com/api/v9/users/@me?x=1"
        );
        assert_eq!(
            rewrite_onto(
                base,
                "wss://gateway.discord.gg/?encoding=etf&v=9&compress=zstd-stream"
            )
            .unwrap(),
            "ws://127.0.0.1:4100/gateway.discord.gg/?encoding=etf&v=9&compress=zstd-stream"
        );
        assert!(rewrite_onto(base, "http://127.0.0.1:4100/a").is_none());
        assert!(rewrite_onto(base, "data:image/png;base64,AA").is_none());
        assert_eq!(
            normalize_url("/cdn.discordapp.com/avatars/1/a.webp?size=80"),
            "https://cdn.discordapp.com/avatars/1/a.webp?size=80"
        );
    }

    #[test]
    fn secrets_are_redacted_at_any_depth_and_in_queries() {
        let mut value = json!({
            "session_id": "abc",
            "analytics_token": "t",
            "user": {"id": "1", "access_token": "x"},
            "sessions": [{"session_id": "s", "status": "online"}],
            "resume_gateway_url": "wss://gateway-us-east1-b.discord.gg",
            "count": 2,
            "auth_session_id_hash": "h",
            "client_secret": 5
        });
        sanitize(&mut value);
        assert_eq!(value["session_id"], REDACTED);
        assert_eq!(value["analytics_token"], REDACTED);
        assert_eq!(value["user"]["access_token"], REDACTED);
        assert_eq!(value["user"]["id"], "1");
        assert_eq!(value["sessions"][0]["session_id"], REDACTED);
        assert_eq!(value["sessions"][0]["status"], "online");
        assert_eq!(value["auth_session_id_hash"], REDACTED);
        assert!(value["client_secret"].is_null());
        assert_eq!(value["count"], 2);
        assert_eq!(value["resume_gateway_url"], "wss://replay-gateway.invalid");
        assert_eq!(
            normalize_url("https://discord.com/api/v9/x?feature=a&refresh_token=abc&n=1"),
            format!("https://discord.com/api/v9/x?feature=a&refresh_token={REDACTED}&n=1")
        );
    }
}
