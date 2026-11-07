//! Voice control socket messages: builders for what we send, validated parsers for what the
//! server sends. Op numbers and field names follow `dc_proto::voice_op` and
//! `tools/voice-notes.md` (pinned build 667580).
//!
//! Parsing rules adapted from Serein `crates/discord-voice/src/transport.rs` (`endpoint`,
//! `number`, `id`, `transition`, `acknowledges`), MIT OR Apache-2.0.
use crate::rtp::Mode;
use dc_proto::voice_op::VoiceOp;
use serde_json::{Value, json};
use std::net::IpAddr;

/// Upper bound for one signaling message or binary frame (Serein's `MAX_SIGNAL`).
pub(crate) const MAX_SIGNAL: usize = 64 * 1024;
/// DAVE protocol version this client implements and requires.
pub(crate) const DAVE_PROTOCOL_VERSION: u64 = 1;
/// Discord's documented "disconnected" close code (kicked, moved, channel deleted). Not in
/// the pinned bundle's list in `voice_op::close`; taken from Serein, which stops on it.
pub(crate) const CLOSE_DISCONNECTED: u16 = 4014;

/// `wss://<endpoint>/?v=9`. Like Serein, only `*.discord.media` hosts receive the token.
pub(crate) fn endpoint_url(raw: &str) -> Result<String, &'static str> {
    if raw.is_empty()
        || raw.len() > 256
        || raw.contains('/')
        || raw.contains('@')
        || raw.contains('?')
        || raw.contains('#')
    {
        return Err("Invalid Discord voice endpoint");
    }
    let url = url::Url::parse(&format!("wss://{raw}/?v=9"))
        .map_err(|_| "Invalid Discord voice endpoint")?;
    let host = url.host_str().ok_or("Missing Discord voice host")?;
    if !host.ends_with(".discord.media") || !url.username().is_empty() || url.password().is_some() {
        return Err("Voice endpoint is outside Discord media");
    }
    Ok(url.into())
}

fn op(op: VoiceOp, d: Value) -> String {
    json!({"op": op as u8, "d": d}).to_string()
}

pub(crate) struct Ids<'a> {
    pub server_id: u64,
    pub channel_id: u64,
    pub user_id: u64,
    pub session_id: &'a str,
    pub token: &'a str,
}

/// Op 0. Audio only: no video, no simulcast streams.
pub(crate) fn identify(ids: &Ids) -> String {
    op(
        VoiceOp::Identify,
        json!({
            "server_id": ids.server_id.to_string(),
            "channel_id": ids.channel_id.to_string(),
            "user_id": ids.user_id.to_string(),
            "session_id": ids.session_id,
            "token": ids.token,
            "max_dave_protocol_version": DAVE_PROTOCOL_VERSION,
            "video": false,
            "streams": [],
        }),
    )
}

/// Op 7, after a dropped socket.
pub(crate) fn resume(ids: &Ids, seq_ack: i64) -> String {
    op(
        VoiceOp::Resume,
        json!({
            "server_id": ids.server_id.to_string(),
            "channel_id": ids.channel_id.to_string(),
            "session_id": ids.session_id,
            "token": ids.token,
            "seq_ack": seq_ack,
        }),
    )
}

/// Op 3: `{t, seq_ack}` from server version 8; a bare value before that.
pub(crate) fn heartbeat(nonce: u64, seq_ack: i64, server_version: u64) -> String {
    if server_version >= 8 {
        op(VoiceOp::Heartbeat, json!({"t": nonce, "seq_ack": seq_ack}))
    } else {
        op(VoiceOp::Heartbeat, json!(nonce))
    }
}

/// Op 1 with our discovered external address and the single Opus codec.
pub(crate) fn select_protocol(address: IpAddr, port: u16, mode: Mode) -> String {
    op(
        VoiceOp::SelectProtocol,
        json!({
            "protocol": "udp",
            "data": {"address": address.to_string(), "port": port, "mode": mode.as_str()},
            "codecs": [{"name": "opus", "type": "audio", "priority": 1000, "payload_type": crate::rtp::OPUS_PAYLOAD_TYPE}],
        }),
    )
}

/// Op 5. Flag 1 is microphone speech.
pub(crate) fn speaking(speaking: bool, ssrc: u32) -> String {
    op(
        VoiceOp::Speaking,
        json!({"speaking": u8::from(speaking), "delay": 0, "ssrc": ssrc}),
    )
}

/// Op 16, sent after READY to servers of version 6 and up.
pub(crate) fn backend_version() -> String {
    op(VoiceOp::VoiceBackendVersion, json!({}))
}

/// Op 23.
pub(crate) fn transition_ready(transition_id: u16) -> String {
    op(
        VoiceOp::DaveTransitionReady,
        json!({"transition_id": transition_id}),
    )
}

/// Op 31.
pub(crate) fn invalid_commit_welcome(transition_id: u16) -> String {
    op(
        VoiceOp::MlsInvalidCommitWelcome,
        json!({"transition_id": transition_id}),
    )
}

/// A parsed text frame: `{op, seq, d}`.
pub(crate) struct Text {
    pub op: Option<VoiceOp>,
    pub raw_op: u64,
    pub seq: Option<i64>,
    pub d: Value,
}
pub(crate) fn parse_text(text: &str) -> Result<Text, &'static str> {
    if text.len() > MAX_SIGNAL {
        return Err("Voice signaling message too large");
    }
    let mut event: Value = serde_json::from_str(text).map_err(|_| "Invalid voice JSON")?;
    let raw_op = event["op"].as_u64().ok_or("Malformed voice signaling op")?;
    Ok(Text {
        op: VoiceOp::from_u64(raw_op),
        raw_op,
        seq: event["seq"].as_i64(),
        d: event["d"].take(),
    })
}

/// A server binary frame: big-endian sequence number, op, payload.
pub(crate) struct Binary<'a> {
    pub seq: u16,
    pub op: u8,
    pub payload: &'a [u8],
}
pub(crate) fn parse_binary(bytes: &[u8]) -> Result<Binary<'_>, &'static str> {
    if bytes.len() < 3 {
        return Err("Truncated DAVE signaling");
    }
    if bytes.len() > MAX_SIGNAL {
        return Err("DAVE signaling frame too large");
    }
    Ok(Binary {
        seq: u16::from_be_bytes([bytes[0], bytes[1]]),
        op: bytes[2],
        payload: &bytes[3..],
    })
}

pub(crate) fn number(data: &Value, key: &str) -> Result<u64, &'static str> {
    data[key].as_u64().ok_or("Malformed voice signaling field")
}
/// Snowflakes travel as strings; zero is never a user.
pub(crate) fn id(data: &Value, key: &str) -> Result<u64, &'static str> {
    parse_id(&data[key])
}
fn parse_id(value: &Value) -> Result<u64, &'static str> {
    value
        .as_str()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v != 0)
        .ok_or("Malformed voice participant")
}
pub(crate) fn transition(data: &Value) -> Result<u16, &'static str> {
    u16::try_from(number(data, "transition_id")?).map_err(|_| "Invalid voice transition")
}

/// Op 8.
pub(crate) struct Hello {
    pub interval_ms: u64,
    pub version: u64,
}
pub(crate) fn hello(data: &Value) -> Result<Hello, &'static str> {
    let interval = data["heartbeat_interval"]
        .as_f64()
        .filter(|v| v.is_finite() && (100.0..=120_000.0).contains(v))
        .ok_or("Invalid voice heartbeat interval")?;
    Ok(Hello {
        // Serein caps the interval at 5 s so a stalled socket is noticed quickly.
        interval_ms: (interval as u64).min(5000),
        version: data["v"].as_u64().unwrap_or(0),
    })
}

/// Op 2.
#[derive(Debug, PartialEq)]
pub(crate) struct Ready {
    pub ssrc: u32,
    pub ip: IpAddr,
    pub port: u16,
    pub mode: Mode,
}
pub(crate) fn ready(data: &Value) -> Result<Ready, &'static str> {
    let ssrc = u32::try_from(number(data, "ssrc")?).map_err(|_| "Invalid voice SSRC")?;
    let ip = data["ip"]
        .as_str()
        .ok_or("Missing voice server address")?
        .parse()
        .map_err(|_| "Invalid voice server address")?;
    let port = u16::try_from(number(data, "port")?)
        .ok()
        .filter(|p| *p > 0)
        .ok_or("Invalid voice server port")?;
    let modes = data["modes"]
        .as_array()
        .ok_or("Missing voice encryption modes")?;
    let mode = Mode::choose(modes.iter().filter_map(Value::as_str))
        .ok_or("Required voice transport encryption is unavailable")?;
    Ok(Ready {
        ssrc,
        ip,
        port,
        mode,
    })
}

/// Op 4: the transport key, checked against the mode we selected and DAVE version 1.
pub(crate) fn session_description(
    data: &mut Value,
    selected: Mode,
) -> Result<zeroize::Zeroizing<[u8; 32]>, &'static str> {
    if data["mode"].as_str() != Some(selected.as_str()) {
        return Err("Discord negotiated another voice encryption mode");
    }
    if data["dave_protocol_version"].as_u64() != Some(DAVE_PROTOCOL_VERSION) {
        return Err("Required DAVE version 1 encryption was not negotiated");
    }
    let values = data["secret_key"].take();
    let values = values.as_array().ok_or("Missing voice transport key")?;
    if values.len() != 32 {
        return Err("Invalid voice transport key");
    }
    let mut key = zeroize::Zeroizing::new([0; 32]);
    for (out, v) in key.iter_mut().zip(values) {
        *out = v
            .as_u64()
            .and_then(|v| u8::try_from(v).ok())
            .ok_or("Invalid voice transport key")?;
    }
    Ok(key)
}

/// Op 11: `user_ids`, bounded to the 64-participant limit.
pub(crate) fn client_connect(data: &Value) -> Result<Vec<u64>, &'static str> {
    let ids = data["user_ids"]
        .as_array()
        .ok_or("Missing voice participants")?;
    if ids.len() > crate::dave::MAX_PARTICIPANTS {
        return Err("Voice channel exceeds the 64 participant limit");
    }
    ids.iter().map(parse_id).collect()
}

/// Whether a heartbeat acknowledgement names the latest outstanding nonce or one of the
/// `missed` heartbeats sent before it; a late acknowledgement still proves liveness.
pub(crate) fn acknowledges(awaiting: Option<u64>, missed: u8, nonce: Option<u64>) -> bool {
    awaiting
        .zip(nonce)
        .is_some_and(|(latest, nonce)| latest.wrapping_sub(nonce) <= u64::from(missed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Ids<'static> {
        Ids {
            server_id: 1,
            channel_id: 2,
            user_id: 3,
            session_id: "session",
            token: "token",
        }
    }
    fn parsed(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn endpoint_urls_carry_version_9_and_stay_on_discord_media() {
        // The default wss port is implied.
        assert_eq!(
            endpoint_url("c-ams01-abcd.discord.media:443").unwrap(),
            "wss://c-ams01-abcd.discord.media/?v=9"
        );
        assert_eq!(
            endpoint_url("c-ams01-abcd.discord.media:2096").unwrap(),
            "wss://c-ams01-abcd.discord.media:2096/?v=9"
        );
        assert_eq!(
            endpoint_url("rotterdam1234.discord.media").unwrap(),
            "wss://rotterdam1234.discord.media/?v=9"
        );
        for bad in [
            "",
            "evil.example:443",
            "discord.media.evil.example",
            "x.discord.media/../",
            "user@x.discord.media",
            "x.discord.media?v=1",
            "wss://x.discord.media",
        ] {
            assert!(endpoint_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn identify_and_resume_fields() {
        let identify = parsed(&identify(&ids()));
        assert_eq!(identify["op"], 0);
        let d = &identify["d"];
        assert_eq!(d["server_id"], "1");
        assert_eq!(d["channel_id"], "2");
        assert_eq!(d["user_id"], "3");
        assert_eq!(d["session_id"], "session");
        assert_eq!(d["token"], "token");
        assert_eq!(d["max_dave_protocol_version"], 1);
        assert_eq!(d["video"], false);
        assert_eq!(d["streams"], json!([]));

        let resume = parsed(&resume(&ids(), 41));
        assert_eq!(resume["op"], 7);
        assert_eq!(resume["d"]["seq_ack"], 41);
        assert_eq!(resume["d"]["channel_id"], "2");
    }

    #[test]
    fn heartbeat_shape_follows_server_version() {
        let modern = parsed(&heartbeat(5, 12, 8));
        assert_eq!(modern, json!({"op": 3, "d": {"t": 5, "seq_ack": 12}}));
        let legacy = parsed(&heartbeat(5, 12, 7));
        assert_eq!(legacy, json!({"op": 3, "d": 5}));
    }

    #[test]
    fn select_protocol_and_speaking() {
        let select = parsed(&select_protocol(
            "203.0.113.9".parse().unwrap(),
            50_000,
            Mode::Aes256GcmRtpSize,
        ));
        assert_eq!(select["op"], 1);
        assert_eq!(select["d"]["protocol"], "udp");
        assert_eq!(
            select["d"]["data"],
            json!({"address": "203.0.113.9", "port": 50000, "mode": "aead_aes256_gcm_rtpsize"})
        );
        assert_eq!(select["d"]["codecs"][0]["payload_type"], 120);
        assert_eq!(
            parsed(&speaking(true, 9)),
            json!({"op": 5, "d": {"speaking": 1, "delay": 0, "ssrc": 9}})
        );
        assert_eq!(
            parsed(&transition_ready(4)),
            json!({"op": 23, "d": {"transition_id": 4}})
        );
        assert_eq!(parsed(&invalid_commit_welcome(4))["op"], 31);
        assert_eq!(parsed(&backend_version()), json!({"op": 16, "d": {}}));
    }

    #[test]
    fn text_and_binary_frames() {
        let text =
            parse_text(r#"{"op":8,"seq":3,"d":{"v":8,"heartbeat_interval":13750.25}}"#).unwrap();
        assert_eq!(text.op, Some(VoiceOp::Hello));
        assert_eq!(text.seq, Some(3));
        let hello = hello(&text.d).unwrap();
        assert_eq!((hello.interval_ms, hello.version), (5000, 8));
        assert!(super::hello(&json!({"heartbeat_interval": 10})).is_err());
        assert!(super::hello(&json!({"heartbeat_interval": "x"})).is_err());
        let unknown = parse_text(r#"{"op":99,"d":null}"#).unwrap();
        assert_eq!((unknown.op, unknown.raw_op), (None, 99));
        assert!(parse_text(r#"{"d":{}}"#).is_err());
        assert!(parse_text("not json").is_err());

        let frame = parse_binary(&[0x01, 0x02, 25, 9, 8]).unwrap();
        assert_eq!(
            (frame.seq, frame.op, frame.payload),
            (0x0102, 25, &[9u8, 8][..])
        );
        assert!(parse_binary(&[0, 1]).is_err());
        assert!(parse_binary(&vec![0; MAX_SIGNAL + 1]).is_err());
    }

    #[test]
    fn ready_and_session_description() {
        let data = json!({"ssrc": 42, "ip": "203.0.113.9", "port": 50001,
            "modes": ["aead_xchacha20_poly1305_rtpsize", "aead_aes256_gcm_rtpsize", "xsalsa20_poly1305"]});
        let ready = ready(&data).unwrap();
        assert_eq!(ready.ssrc, 42);
        assert_eq!(ready.port, 50001);
        assert_eq!(ready.mode, Mode::Aes256GcmRtpSize);
        let only_chacha = json!({"ssrc": 1, "ip": "203.0.113.9", "port": 1, "modes": ["aead_xchacha20_poly1305_rtpsize"]});
        assert_eq!(
            super::ready(&only_chacha).unwrap().mode,
            Mode::XChaCha20Poly1305RtpSize
        );
        for bad in [
            json!({"ssrc": 1, "ip": "203.0.113.9", "port": 1, "modes": ["xsalsa20_poly1305"]}),
            json!({"ssrc": 1, "ip": "203.0.113.9", "port": 0, "modes": ["aead_aes256_gcm_rtpsize"]}),
            json!({"ssrc": 1u64 << 33, "ip": "203.0.113.9", "port": 1, "modes": ["aead_aes256_gcm_rtpsize"]}),
            json!({"ssrc": 1, "ip": "nowhere", "port": 1, "modes": ["aead_aes256_gcm_rtpsize"]}),
        ] {
            assert!(super::ready(&bad).is_err(), "{bad}");
        }

        let key: Vec<u8> = (0..32).collect();
        let mut description = json!({"mode": "aead_aes256_gcm_rtpsize", "secret_key": key, "dave_protocol_version": 1});
        let parsed = session_description(&mut description, Mode::Aes256GcmRtpSize).unwrap();
        assert_eq!(parsed[31], 31);
        assert!(
            description["secret_key"].is_null(),
            "key moved out of the JSON"
        );
        for (mut bad, mode) in [
            (
                json!({"mode": "aead_aes256_gcm_rtpsize", "secret_key": key, "dave_protocol_version": 1}),
                Mode::XChaCha20Poly1305RtpSize,
            ),
            (
                json!({"mode": "aead_aes256_gcm_rtpsize", "secret_key": key, "dave_protocol_version": 0}),
                Mode::Aes256GcmRtpSize,
            ),
            (
                json!({"mode": "aead_aes256_gcm_rtpsize", "secret_key": [1, 2], "dave_protocol_version": 1}),
                Mode::Aes256GcmRtpSize,
            ),
            (
                json!({"mode": "aead_aes256_gcm_rtpsize", "secret_key": vec![256; 32], "dave_protocol_version": 1}),
                Mode::Aes256GcmRtpSize,
            ),
        ] {
            assert!(session_description(&mut bad, mode).is_err());
        }
    }

    #[test]
    fn participants_and_acknowledgements() {
        assert_eq!(
            client_connect(&json!({"user_ids": ["5", "6"]})).unwrap(),
            [5, 6]
        );
        assert!(client_connect(&json!({"user_ids": ["0"]})).is_err());
        assert!(
            client_connect(&json!({"user_ids": [5]})).is_err(),
            "snowflakes are strings"
        );
        let many: Vec<String> = (1..=65).map(|i| i.to_string()).collect();
        assert!(client_connect(&json!({ "user_ids": many })).is_err());
        assert_eq!(id(&json!({"user_id": "77"}), "user_id"), Ok(77));
        assert_eq!(
            transition(&json!({"transition_id": 70000})),
            Err("Invalid voice transition")
        );

        assert!(acknowledges(Some(5), 0, Some(5)));
        assert!(
            acknowledges(Some(5), 1, Some(4)),
            "late ack of the missed one"
        );
        assert!(!acknowledges(Some(5), 0, Some(4)), "stale duplicate");
        assert!(!acknowledges(None, 0, Some(5)));
        assert!(!acknowledges(Some(5), 0, None));
    }
}
