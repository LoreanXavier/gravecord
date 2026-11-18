//! `cargo xtask replay serve <folder> [--port N] [--ready-delay-ms N] [--no-latency]`, and the server behind
//! `cargo xtask perf --replay <folder>`: a local Discord that answers from a recording, so perf runs and captures never
//! sign the fixture account in (Discord flagged it on 2026-11-18 after hundreds of logins a day).
//!
//! The recording (`dc_proto::replay`, made by `cargo xtask perf --record <folder>`) holds every REST and CDN answer and
//! every gateway frame of one signed-in run. One port serves both:
//!
//! - **HTTP**: `GET /<host>/<path>` (the app's `dc_proto::replay::rewrite`) answers what `https://<host>/<path>` answered
//!   when recorded, after the time it took then. The i-th request of a URL gets its i-th recorded answer, the last one
//!   after that; a URL never recorded gets an immediate 404 and a line in the miss log, never a hang.
//! - **WebSocket** (the gateway, ETF over `zstd-stream` as `dc-gateway` speaks it): HELLO after the recorded connect
//!   time; on IDENTIFY, READY after the recorded identify-to-READY time (a fixed server time, `--ready-delay-ms` to set
//!   another), then READY_SUPPLEMENTAL and what the server pushed unasked in the first seconds, at their recorded
//!   offsets. Answers to the client's own ops (member lists after subscriptions, member chunks) are sent when the
//!   client sends that op again. Heartbeats are acknowledged; the rest of the live traffic is left out, so a replayed
//!   session idles.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::protocol::{Role, WebSocket};
use tungstenite::{Message, handshake::derive_accept_key};

/// Unasked dispatches later than this after READY are live chatter, not part of the start.
const START_BURST: Duration = Duration::from_secs(5);
/// A dispatch this long after a client op is not that op's answer.
const ANSWER_WINDOW_MS: u64 = 3000;
const OP_DISPATCH: u64 = 0;
const OP_HEARTBEAT: u64 = 1;
const OP_IDENTIFY: u64 = 2;
const OP_RESUME: u64 = 6;
const OP_HELLO: u64 = 10;
const OP_HEARTBEAT_ACK: u64 = 11;
const OP_QOS_HEARTBEAT: u64 = 40;

#[derive(Clone)]
pub struct Options {
    /// Overrides the recorded identify-to-READY time.
    pub ready_delay_ms: Option<u64>,
    /// Answers every request at once instead of after its recorded time.
    pub no_latency: bool,
    /// Where the server's own lines go (IDENTIFY and READY times, misses).
    pub log: Option<PathBuf>,
}

struct Answer {
    status: u16,
    content_type: Option<String>,
    elapsed_ms: u64,
    file: PathBuf,
}

/// Dispatches the gateway sends in answer to one client op, each with its delay after the op.
struct Reply {
    op: u64,
    request: Value,
    items: Vec<(u64, Value)>,
}

pub struct Recording {
    /// By method and URL, in recorded order.
    http: HashMap<(String, String), Vec<Answer>>,
    hello: Value,
    hello_delay_ms: u64,
    ready_delay_ms: u64,
    /// READY at offset 0, then the start's other dispatches, by offset from READY.
    start: Vec<(u64, Value)>,
    replies: Vec<Reply>,
    /// The recorded account, so the replayed login looks like its token (`GRAVECORD_REPLAY_ACCOUNT`).
    pub account: Option<String>,
}

fn read_lines(path: &Path) -> Result<Vec<Value>> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// `value` with every object's keys sorted, so two ops that differ only in key order match.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonical(&map[key])))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// The servers an op is about: `guild_id` (one or a list) and the keys of `subscriptions`.
fn guilds_of(data: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match &data["guild_id"] {
        Value::String(id) => out.push(id.clone()),
        Value::Array(ids) => out.extend(ids.iter().filter_map(|id| id.as_str().map(str::to_owned))),
        _ => {}
    }
    if let Some(map) = data["subscriptions"].as_object() {
        out.extend(map.keys().cloned());
    }
    out
}

/// Whether a dispatch answers the op `(op, data)` the client sent just before it.
fn answers(op: u64, data: &Value, dispatch: &Value) -> bool {
    let d = &dispatch["d"];
    if d["nonce"].is_string() && data["nonce"].is_string() {
        return d["nonce"] == data["nonce"];
    }
    d["guild_id"]
        .as_str()
        .is_some_and(|guild| op != OP_HEARTBEAT && guilds_of(data).iter().any(|id| id == guild))
}

impl Recording {
    pub fn load(dir: &Path) -> Result<Self> {
        let mut http: HashMap<(String, String), Vec<Answer>> = HashMap::new();
        for line in read_lines(&dir.join("http.jsonl"))? {
            let (Some(method), Some(url), Some(file)) = (
                line["method"].as_str(),
                line["url"].as_str(),
                line["file"].as_str(),
            ) else {
                continue;
            };
            http.entry((method.to_owned(), url.to_owned()))
                .or_default()
                .push(Answer {
                    status: line["status"].as_u64().unwrap_or(200) as u16,
                    content_type: line["content_type"].as_str().map(str::to_owned),
                    elapsed_ms: line["elapsed_ms"].as_u64().unwrap_or(0),
                    file: dir.join(file),
                });
        }
        let frames = read_lines(&dir.join("gateway.jsonl"))?;
        // The first connection: from the first `connect` to the next.
        let mut connect_at = None;
        let mut hello = None;
        let mut identify_at = None;
        let mut ready_at = None;
        let mut start = Vec::new();
        let mut replies: Vec<Reply> = Vec::new();
        let mut last_op: Option<(u64, Value, u64)> = None;
        let mut account = None;
        for frame in &frames {
            let at = frame["at_ms"].as_u64().unwrap_or(0);
            let payload = &frame["payload"];
            match frame["dir"].as_str() {
                Some("connect") => {
                    if connect_at.is_some() {
                        break;
                    }
                    connect_at = Some(at);
                }
                Some("send") => {
                    let op = payload["op"].as_u64().unwrap_or(u64::MAX);
                    if op == OP_IDENTIFY && identify_at.is_none() {
                        identify_at = Some(at);
                    }
                    if ready_at.is_some()
                        && ![OP_HEARTBEAT, OP_QOS_HEARTBEAT, OP_IDENTIFY].contains(&op)
                    {
                        replies.push(Reply {
                            op,
                            request: canonical(&payload["d"]),
                            items: Vec::new(),
                        });
                        last_op = Some((op, payload["d"].clone(), at));
                    }
                }
                Some("recv") => {
                    let op = payload["op"].as_u64().unwrap_or(u64::MAX);
                    if op == OP_HELLO && hello.is_none() {
                        hello = Some((payload.clone(), at));
                        continue;
                    }
                    if op != OP_DISPATCH {
                        continue;
                    }
                    let name = payload["t"].as_str().unwrap_or_default();
                    if name == "READY" && ready_at.is_none() {
                        ready_at = Some(at);
                        account = payload["d"]["user"]["id"].as_str().map(str::to_owned);
                        start.push((0, payload.clone()));
                        continue;
                    }
                    let Some(ready) = ready_at else {
                        continue;
                    };
                    if let Some((op, data, sent)) = &last_op
                        && at.saturating_sub(*sent) <= ANSWER_WINDOW_MS
                        && answers(*op, data, payload)
                        && let Some(reply) = replies.last_mut()
                    {
                        reply.items.push((at - sent, payload.clone()));
                        continue;
                    }
                    let offset = at.saturating_sub(ready);
                    if name == "READY_SUPPLEMENTAL" || offset <= START_BURST.as_millis() as u64 {
                        start.push((offset, payload.clone()));
                    }
                }
                _ => {}
            }
        }
        let (Some((hello, hello_at)), Some(identify_at), Some(ready_at)) =
            (hello, identify_at, ready_at)
        else {
            bail!(
                "{} holds no HELLO, IDENTIFY and READY: not a recording",
                dir.display()
            );
        };
        replies.retain(|reply| !reply.items.is_empty());
        Ok(Self {
            http,
            hello,
            hello_delay_ms: hello_at.saturating_sub(connect_at.unwrap_or(hello_at)),
            ready_delay_ms: ready_at.saturating_sub(identify_at),
            start,
            replies,
            account,
        })
    }

    /// What the recording holds, for the run's log.
    pub fn summary(&self) -> String {
        format!(
            "{} URLs ({} answers), HELLO after {} ms, READY {} ms after IDENTIFY, {} start dispatches, {} op answers",
            self.http.len(),
            self.http.values().map(Vec::len).sum::<usize>(),
            self.hello_delay_ms,
            self.ready_delay_ms,
            self.start.len(),
            self.replies.len()
        )
    }
}

struct State {
    recording: Recording,
    options: Options,
    /// How many times each URL was asked for.
    asked: Mutex<HashMap<(String, String), usize>>,
    /// How many times each reply was used, by its index.
    replied: Mutex<HashMap<usize, usize>>,
    log: Mutex<Option<fs::File>>,
    started: Instant,
    port: u16,
}

impl State {
    fn note(&self, line: &str) {
        let line = format!("[{:>7} ms] {line}", self.started.elapsed().as_millis());
        if let Ok(mut log) = self.log.lock()
            && let Some(file) = log.as_mut()
        {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// A running server: its address, for `GRAVECORD_REPLAY`.
pub struct Server {
    pub url: String,
    pub account: Option<String>,
}

/// Binds a port (0 for any) and serves `dir` on threads of its own for as long as the process lives.
pub fn start(dir: &Path, port: u16, options: Options) -> Result<Server> {
    let recording = Recording::load(dir)?;
    println!("replay: {}", recording.summary());
    let listener = TcpListener::bind(("127.0.0.1", port)).context("binding the replay port")?;
    let port = listener.local_addr()?.port();
    let log = match &options.log {
        Some(path) => Some(fs::File::create(path)?),
        None => None,
    };
    let account = recording.account.clone();
    let state = Arc::new(State {
        recording,
        options,
        asked: Mutex::new(HashMap::new()),
        replied: Mutex::new(HashMap::new()),
        log: Mutex::new(log),
        started: Instant::now(),
        port,
    });
    state.note(&state.recording.summary());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = state.clone();
            std::thread::spawn(move || {
                let _ = stream.set_nodelay(true);
                if let Err(error) = serve(stream, &state) {
                    state.note(&format!("connection ended: {error}"));
                }
            });
        }
    });
    Ok(Server {
        url: format!("http://127.0.0.1:{port}"),
        account,
    })
}

struct Head {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn read_head(reader: &mut BufReader<TcpStream>) -> Result<Option<Head>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        bail!("bad request line");
    };
    let (method, target) = (method.to_owned(), target.to_owned());
    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            bail!("connection closed inside a request head");
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            headers.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    Ok(Some(Head {
        method,
        target,
        headers,
    }))
}

fn serve(stream: TcpStream, state: &State) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    while let Some(head) = read_head(&mut reader)? {
        if head
            .header("upgrade")
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        {
            // A WebSocket client sends nothing before the 101, so nothing past the head is buffered.
            let key = head.header("sec-websocket-key").context("no key")?;
            let accept = derive_accept_key(key.as_bytes());
            write!(
                writer,
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
            )?;
            writer.flush()?;
            return gateway(writer, state);
        }
        let length: usize = head
            .header("content-length")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0; length];
        reader.read_exact(&mut body)?;
        answer(&mut writer, &head, state)?;
        if head
            .header("connection")
            .is_some_and(|value| value.eq_ignore_ascii_case("close"))
        {
            return Ok(());
        }
    }
    Ok(())
}

fn answer(writer: &mut TcpStream, head: &Head, state: &State) -> Result<()> {
    let url = dc_proto::replay::normalize_url(&head.target);
    let key = (head.method.clone(), url.clone());
    let answers = state.recording.http.get(&key).or_else(|| {
        // A recorded answer of the same path whose query differed (a cursor, a nonce).
        let path = url.split('?').next().unwrap_or(&url);
        state
            .recording
            .http
            .iter()
            .find(|((method, other), _)| {
                *method == head.method && other.split('?').next() == Some(path)
            })
            .map(|(_, answers)| answers)
    });
    let Some(answers) = answers.filter(|answers| !answers.is_empty()) else {
        state.note(&format!("miss {} {}", head.method, log_url(&url)));
        let body = br#"{"message": "404: Not Found", "code": 0}"#;
        write!(
            writer,
            "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )?;
        writer.write_all(body)?;
        return Ok(writer.flush()?);
    };
    let index = {
        let mut asked = state.asked.lock().unwrap_or_else(|e| e.into_inner());
        let count = asked.entry(key).or_insert(0);
        *count += 1;
        (*count - 1).min(answers.len() - 1)
    };
    let recorded = &answers[index];
    let body = fs::read(&recorded.file).unwrap_or_default();
    if !state.options.no_latency {
        std::thread::sleep(Duration::from_millis(recorded.elapsed_ms));
    }
    let mut response = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\n",
        recorded.status,
        if recorded.status < 400 { "OK" } else { "Error" },
        if recorded.status == 304 {
            0
        } else {
            body.len()
        }
    );
    if let Some(kind) = &recorded.content_type {
        response.push_str(&format!("Content-Type: {kind}\r\n"));
    }
    response.push_str("\r\n");
    writer.write_all(response.as_bytes())?;
    if recorded.status != 304 {
        writer.write_all(&body)?;
    }
    Ok(writer.flush()?)
}

/// A URL for the log: the path only, as the app's own logs keep it.
fn log_url(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// One flushed block of the connection's zstd stream.
fn compress(encoder: &mut zstd::stream::raw::Encoder<'static>, data: &[u8]) -> Result<Vec<u8>> {
    use zstd::stream::raw::{InBuffer, Operation, OutBuffer};
    let mut out = vec![0; data.len() + (1 << 17)];
    let mut input = InBuffer::around(data);
    let mut output = OutBuffer::around(out.as_mut_slice());
    while input.pos() < data.len() {
        encoder.run(&mut input, &mut output)?;
    }
    while encoder.flush(&mut output)? != 0 {}
    let written = output.pos();
    out.truncate(written);
    Ok(out)
}

/// A frame waiting for its moment.
struct Pending {
    at: Instant,
    payload: Value,
}

fn gateway(stream: TcpStream, state: &State) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(2)))?;
    let mut socket = WebSocket::from_raw_socket(stream, Role::Server, None);
    let mut encoder = zstd::stream::raw::Encoder::new(3)?;
    let mut seq = 0u64;
    let opened = Instant::now();
    let recording = &state.recording;
    let mut queue: Vec<Pending> = vec![Pending {
        at: opened + Duration::from_millis(recording.hello_delay_ms),
        payload: recording.hello.clone(),
    }];
    state.note("gateway connected");
    loop {
        queue.sort_by_key(|pending| pending.at);
        while queue
            .first()
            .is_some_and(|first| first.at <= Instant::now())
        {
            let mut pending = queue.remove(0);
            if pending.payload["op"] == OP_DISPATCH {
                seq += 1;
                pending.payload["s"] = json!(seq);
            }
            let name = pending.payload["t"].as_str().map(str::to_owned);
            let bytes = compress(&mut encoder, &dc_proto::etf::pack(&pending.payload))?;
            socket.send(Message::Binary(bytes.into()))?;
            if name.as_deref() == Some("READY") {
                state.note("READY written");
            }
        }
        let message = match socket.read() {
            Ok(message) => message,
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                state.note("gateway closed");
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        let Message::Binary(bytes) = message else {
            if let Message::Close(_) = message {
                state.note("gateway closed by the client");
                return Ok(());
            }
            continue;
        };
        let Ok(payload) = dc_proto::etf::unpack(&bytes) else {
            continue;
        };
        let op = payload["op"].as_u64().unwrap_or(u64::MAX);
        let data = &payload["d"];
        match op {
            OP_HEARTBEAT | OP_QOS_HEARTBEAT => {
                let bytes = compress(
                    &mut encoder,
                    &dc_proto::etf::pack(&json!({"op": OP_HEARTBEAT_ACK, "d": null})),
                )?;
                socket.send(Message::Binary(bytes.into()))?;
            }
            OP_IDENTIFY | OP_RESUME => {
                state.note(&format!(
                    "{} received",
                    if op == OP_IDENTIFY {
                        "IDENTIFY"
                    } else {
                        "RESUME"
                    }
                ));
                let delay = state
                    .options
                    .ready_delay_ms
                    .unwrap_or(recording.ready_delay_ms);
                // Packed now, written at its moment: the delay is exact whatever packing costs.
                let ready_at = Instant::now() + Duration::from_millis(delay);
                for (offset, frame) in &recording.start {
                    let mut frame = frame.clone();
                    if frame["t"] == "READY" {
                        frame["d"]["resume_gateway_url"] =
                            json!(format!("ws://127.0.0.1:{}", state.port));
                    }
                    queue.push(Pending {
                        at: ready_at + Duration::from_millis(*offset),
                        payload: frame,
                    });
                }
            }
            _ => {
                let request = canonical(data);
                let exact = recording
                    .replies
                    .iter()
                    .position(|reply| reply.op == op && reply.request == request);
                let guilds = guilds_of(data);
                let found = exact.or_else(|| {
                    recording.replies.iter().position(|reply| {
                        reply.op == op && !guilds.is_empty() && guilds_of(&reply.request) == guilds
                    })
                });
                let Some(found) = found else {
                    continue;
                };
                // The same op asked again takes the next recorded answer of that kind, if there is one.
                let index = {
                    let mut used = state.replied.lock().unwrap_or_else(|e| e.into_inner());
                    let count = used.entry(found).or_insert(0);
                    *count += 1;
                    let same: Vec<usize> = recording
                        .replies
                        .iter()
                        .enumerate()
                        .filter(|(_, reply)| {
                            reply.op == op && reply.request == recording.replies[found].request
                        })
                        .map(|(index, _)| index)
                        .collect();
                    same[(*count - 1).min(same.len() - 1)]
                };
                let now = Instant::now();
                for (delay, frame) in &recording.replies[index].items {
                    let mut frame = frame.clone();
                    if data["nonce"].is_string() && frame["d"]["nonce"].is_string() {
                        frame["d"]["nonce"] = data["nonce"].clone();
                    }
                    queue.push(Pending {
                        at: now + Duration::from_millis(*delay),
                        payload: frame,
                    });
                }
            }
        }
    }
}

/// `cargo xtask replay serve <folder> [--port N] [--ready-delay-ms N] [--no-latency]`: serves until stopped.
pub fn run(args: &[&str]) -> Result<()> {
    let ["serve", folder, rest @ ..] = args else {
        bail!(
            "usage: cargo xtask replay serve <folder> [--port N] [--ready-delay-ms N] [--no-latency]"
        );
    };
    let mut port = 0;
    let mut options = Options {
        ready_delay_ms: None,
        no_latency: false,
        log: None,
    };
    let mut rest = rest.iter();
    while let Some(flag) = rest.next() {
        match *flag {
            "--no-latency" => options.no_latency = true,
            "--port" => port = rest.next().context("--port N")?.parse()?,
            "--ready-delay-ms" => {
                options.ready_delay_ms = Some(rest.next().context("--ready-delay-ms N")?.parse()?)
            }
            "--log" => options.log = Some(PathBuf::from(rest.next().context("--log PATH")?)),
            other => bail!("unknown flag {other}"),
        }
    }
    let server = start(Path::new(folder), port, options)?;
    println!("serving on {}", server.url);
    println!(
        "set GRAVECORD_REPLAY={} GRAVECORD_REPLAY_ACCOUNT={} for the app (capture build)",
        server.url,
        server.account.as_deref().unwrap_or("0")
    );
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_sorts_keys_at_every_depth() {
        let a = json!({"b": 1, "a": {"y": [{"q": 1, "p": 2}], "x": 0}});
        let b = json!({"a": {"x": 0, "y": [{"p": 2, "q": 1}]}, "b": 1});
        assert_eq!(canonical(&a), canonical(&b));
        assert_eq!(canonical(&a).to_string(), canonical(&b).to_string());
    }

    #[test]
    fn a_recording_yields_the_server_times_and_the_answers_to_ops() {
        let dir = std::env::temp_dir().join(format!("replay-load-{}", std::process::id()));
        fs::create_dir_all(dir.join("http")).unwrap();
        let frames = [
            json!({"at_ms": 10, "dir": "connect", "payload": {"op": null, "d": null}}),
            json!({"at_ms": 60, "dir": "recv", "payload": {"op": 10, "d": {"heartbeat_interval": 41250}}}),
            json!({"at_ms": 61, "dir": "send", "payload": {"op": 2, "d": {"token": "replay-redacted"}}}),
            json!({"at_ms": 461, "dir": "recv", "payload": {"op": 0, "t": "READY", "d": {"user": {"id": "7"}}}}),
            json!({"at_ms": 462, "dir": "recv", "payload": {"op": 0, "t": "READY_SUPPLEMENTAL", "d": {}}}),
            json!({"at_ms": 500, "dir": "send", "payload": {"op": 37, "d": {"subscriptions": {"5": {}}}}}),
            json!({"at_ms": 540, "dir": "recv", "payload": {"op": 0, "t": "GUILD_MEMBER_LIST_UPDATE", "d": {"guild_id": "5"}}}),
            json!({"at_ms": 9000, "dir": "recv", "payload": {"op": 0, "t": "TYPING_START", "d": {"channel_id": "1"}}}),
        ];
        let lines: Vec<String> = frames.iter().map(Value::to_string).collect();
        fs::write(dir.join("gateway.jsonl"), lines.join("\n")).unwrap();
        fs::write(dir.join("http/000000.bin"), b"{}").unwrap();
        fs::write(
            dir.join("http.jsonl"),
            json!({"method": "GET", "url": "https://discord.com/api/v9/x", "status": 200, "elapsed_ms": 9, "file": "http/000000.bin"}).to_string(),
        )
        .unwrap();
        let recording = Recording::load(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(recording.hello_delay_ms, 50);
        assert_eq!(recording.ready_delay_ms, 400);
        assert_eq!(recording.account.as_deref(), Some("7"));
        // READY and READY_SUPPLEMENTAL start the session; the typing nine seconds later is live chatter, left out.
        assert_eq!(recording.start.len(), 2);
        assert_eq!(recording.replies.len(), 1);
        assert_eq!(recording.replies[0].items[0].0, 40);
        assert_eq!(recording.http.len(), 1);
    }

    #[test]
    fn a_member_list_answers_the_subscription_of_its_server() {
        let sub = json!({"subscriptions": {"5": {"typing": true}}});
        let list = json!({"op": 0, "t": "GUILD_MEMBER_LIST_UPDATE", "d": {"guild_id": "5"}});
        let other = json!({"op": 0, "t": "PRESENCE_UPDATE", "d": {"guild_id": "6"}});
        assert!(answers(37, &sub, &list));
        assert!(!answers(37, &sub, &other));
        let chunk_ask = json!({"guild_id": ["5"], "nonce": "n1"});
        let chunk =
            json!({"op": 0, "t": "GUILD_MEMBERS_CHUNK", "d": {"guild_id": "5", "nonce": "n1"}});
        let stale =
            json!({"op": 0, "t": "GUILD_MEMBERS_CHUNK", "d": {"guild_id": "5", "nonce": "n0"}});
        assert!(answers(8, &chunk_ask, &chunk));
        assert!(!answers(8, &chunk_ask, &stale));
    }
}
