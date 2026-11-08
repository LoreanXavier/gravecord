//! Go Live on the main gateway: stream keys, the payloads of ops 18 to 22, the four dispatches, and the rendezvous that
//! turns "created" plus "server update" into the details of the second voice connection.
//!
//! Every shape is the pinned build's: keys from module 652896, op payloads from the gateway socket in module 868, the
//! dispatch fields from module 225123, delete reasons from module 652215, the connection ids from `RTCConnection`
//! (`trueServerId`, `trueChannelId` in module 921747).
use serde_json::{Value, json};
use std::{fmt, time::Duration};
use tokio::time::Instant;

/// Both halves must arrive within this long, or the stream fails (the task's bound).
pub const RENDEZVOUS_TIMEOUT: Duration = Duration::from_secs(30);

/// `guild:<guild>:<channel>:<owner>` in a server, `call:<channel>:<owner>` in a DM or group call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamKey {
    Guild {
        guild: u64,
        channel: u64,
        owner: u64,
    },
    Call {
        channel: u64,
        owner: u64,
    },
}

impl StreamKey {
    pub fn parse(key: &str) -> Option<Self> {
        let id = |part: &str| part.parse::<u64>().ok().filter(|v| *v != 0);
        let mut parts = key.split(':');
        match (
            parts.next()?,
            parts.next()?,
            parts.next()?,
            parts.next(),
            parts.next(),
        ) {
            ("guild", guild, channel, Some(owner), None) => Some(Self::Guild {
                guild: id(guild)?,
                channel: id(channel)?,
                owner: id(owner)?,
            }),
            ("call", channel, owner, None, None) => Some(Self::Call {
                channel: id(channel)?,
                owner: id(owner)?,
            }),
            _ => None,
        }
    }

    pub fn owner(self) -> u64 {
        match self {
            Self::Guild { owner, .. } | Self::Call { owner, .. } => owner,
        }
    }

    pub fn channel(self) -> u64 {
        match self {
            Self::Guild { channel, .. } | Self::Call { channel, .. } => channel,
        }
    }

    pub fn guild(self) -> Option<u64> {
        match self {
            Self::Guild { guild, .. } => Some(guild),
            Self::Call { .. } => None,
        }
    }
}

impl fmt::Display for StreamKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Guild {
                guild,
                channel,
                owner,
            } => write!(f, "guild:{guild}:{channel}:{owner}"),
            Self::Call { channel, owner } => write!(f, "call:{channel}:{owner}"),
        }
    }
}

/// Op 18 `STREAM_CREATE`: `{type, guild_id, channel_id, preferred_region}` (`streamCreate`). `guild` is `None` in a
/// call, where the type is "call" and the guild id is null.
pub fn create(guild: Option<u64>, channel: u64, preferred_region: Option<&str>) -> Value {
    json!({
        "type": if guild.is_some() { "guild" } else { "call" },
        "guild_id": guild.map(|g| g.to_string()),
        "channel_id": channel.to_string(),
        "preferred_region": preferred_region,
    })
}

/// Op 19 `STREAM_DELETE`.
pub fn delete(key: &StreamKey) -> Value {
    json!({"stream_key": key.to_string()})
}

/// Op 20 `STREAM_WATCH`.
pub fn watch(key: &StreamKey) -> Value {
    json!({"stream_key": key.to_string()})
}

/// Op 21 `STREAM_PING`.
pub fn ping(key: &StreamKey) -> Value {
    json!({"stream_key": key.to_string()})
}

/// Op 22 `STREAM_SET_PAUSED`.
pub fn set_paused(key: &StreamKey, paused: bool) -> Value {
    json!({"stream_key": key.to_string(), "paused": paused})
}

/// Why a stream ended, from `STREAM_DELETE` (`reason`, `unavailable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// The owner (or we) ended it.
    UserRequested,
    /// The stream has the most viewers it allows.
    StreamFull,
    /// We may not watch or stream here.
    Unauthorized,
    /// The server rate limited streams.
    RateLimited,
    /// The stream is unavailable for now and may come back (`unavailable`).
    Unavailable,
    /// No reason given, or one this build does not know.
    Other,
    /// Our own connection to it failed or timed out.
    Failed,
}

/// The four stream dispatches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dispatch {
    /// `STREAM_CREATE`: the stream exists and names its voice server.
    Create {
        key: StreamKey,
        rtc_server_id: u64,
        rtc_channel_id: Option<u64>,
        region: Option<String>,
        viewers: Vec<u64>,
        paused: bool,
    },
    /// `STREAM_SERVER_UPDATE`: where to connect, with the token.
    ServerUpdate {
        key: StreamKey,
        endpoint: String,
        token: String,
    },
    /// `STREAM_UPDATE`: viewers and pause state.
    Update {
        key: StreamKey,
        viewers: Option<Vec<u64>>,
        paused: bool,
    },
    Delete {
        key: StreamKey,
        reason: EndReason,
    },
}

impl Dispatch {
    pub fn key(&self) -> StreamKey {
        match self {
            Self::Create { key, .. }
            | Self::ServerUpdate { key, .. }
            | Self::Update { key, .. }
            | Self::Delete { key, .. } => *key,
        }
    }
}

fn ids(value: &Value) -> Option<Vec<u64>> {
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|id| id.as_str()?.parse().ok())
            .collect(),
    )
}

/// Parses a gateway dispatch by its `t`. `None` for other events and for malformed ones.
pub fn parse_dispatch(name: &str, data: &Value) -> Option<Dispatch> {
    let key = StreamKey::parse(data["stream_key"].as_str()?)?;
    let id = |field: &str| {
        data[field]
            .as_str()?
            .parse::<u64>()
            .ok()
            .filter(|v| *v != 0)
    };
    match name {
        "STREAM_CREATE" => Some(Dispatch::Create {
            key,
            rtc_server_id: id("rtc_server_id")?,
            rtc_channel_id: id("rtc_channel_id"),
            region: data["region"].as_str().map(str::to_owned),
            viewers: ids(&data["viewer_ids"]).unwrap_or_default(),
            paused: data["paused"].as_bool().unwrap_or(false),
        }),
        "STREAM_SERVER_UPDATE" => Some(Dispatch::ServerUpdate {
            key,
            endpoint: data["endpoint"].as_str()?.to_owned(),
            token: data["token"].as_str()?.to_owned(),
        }),
        "STREAM_UPDATE" => Some(Dispatch::Update {
            key,
            viewers: ids(&data["viewer_ids"]),
            paused: data["paused"].as_bool().unwrap_or(false),
        }),
        "STREAM_DELETE" => Some(Dispatch::Delete {
            key,
            reason: if data["unavailable"].as_bool() == Some(true) {
                EndReason::Unavailable
            } else {
                match data["reason"].as_str() {
                    Some("user_requested") => EndReason::UserRequested,
                    Some("stream_full") => EndReason::StreamFull,
                    Some("unauthorized") => EndReason::Unauthorized,
                    Some("safety_guild_rate_limited") => EndReason::RateLimited,
                    _ => EndReason::Other,
                }
            },
        }),
        _ => None,
    }
}

/// Whether we send the stream or watch it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Send,
    Watch,
}

/// What the second voice connection of a stream needs. Like the official `RTCConnection` for a stream: the server id is
/// the stream's `rtc_server_id`; the channel (and so the DAVE group) is `rtc_channel_id`, else one before the server id.
pub struct StreamJoin {
    pub key: StreamKey,
    pub role: Role,
    pub endpoint: String,
    pub token: String,
    pub session_id: String,
    pub server_id: u64,
    pub channel_id: u64,
    pub user_id: u64,
}

impl fmt::Debug for StreamJoin {
    /// Secrets and ids never reach logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamJoin")
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

/// Waits for `STREAM_CREATE` and `STREAM_SERVER_UPDATE` of one stream, in either order.
pub struct Rendezvous {
    key: StreamKey,
    role: Role,
    user_id: u64,
    session_id: String,
    started: Instant,
    create: Option<(u64, u64)>,
    server: Option<(String, String)>,
}

impl Rendezvous {
    pub fn new(key: StreamKey, role: Role, user_id: u64, session_id: String, now: Instant) -> Self {
        Self {
            key,
            role,
            user_id,
            session_id,
            started: now,
            create: None,
            server: None,
        }
    }

    /// Takes a dispatch of this stream; returns the join once both halves are in. It returns it once.
    pub fn feed(&mut self, dispatch: &Dispatch) -> Option<StreamJoin> {
        if dispatch.key() != self.key {
            return None;
        }
        match dispatch {
            Dispatch::Create {
                rtc_server_id,
                rtc_channel_id,
                ..
            } => {
                let channel = rtc_channel_id.unwrap_or(rtc_server_id.saturating_sub(1));
                self.create = Some((*rtc_server_id, channel));
            }
            Dispatch::ServerUpdate {
                endpoint, token, ..
            } => self.server = Some((endpoint.clone(), token.clone())),
            _ => return None,
        }
        let ((server_id, channel_id), (endpoint, token)) = (
            self.create?,
            self.server.take_if(|_| self.create.is_some())?,
        );
        self.create = None;
        Some(StreamJoin {
            key: self.key,
            role: self.role,
            endpoint,
            token,
            session_id: self.session_id.clone(),
            server_id,
            channel_id,
            user_id: self.user_id,
        })
    }

    pub fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) >= RENDEZVOUS_TIMEOUT
    }

    pub fn key(&self) -> StreamKey {
        self.key
    }
}

/// Where a stream stands, in the official client's words (`CONNECTING`, `ACTIVE`, `PAUSED`, `ENDED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamStatus {
    /// Asked for (op 18 or 20); the gateway has not answered yet.
    Starting,
    /// The stream exists; its voice connection is being set up.
    Connecting,
    /// Media flows (sender) or the stream is on its way (watcher).
    Live,
    Paused,
    Ended(EndReason),
}

/// One state change for the app: the stream, whether we send or watch it, its status and its viewer count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamUpdate {
    pub key: StreamKey,
    pub role: Role,
    pub status: StreamStatus,
    pub viewers: usize,
}

/// What a dispatch led to.
#[derive(Debug, Default)]
pub struct Step {
    /// Both halves are in: start the voice connection with this.
    pub join: Option<StreamJoin>,
    pub update: Option<StreamUpdate>,
}

/// Follows one stream from the request to its end. The app feeds it the stream dispatches and the voice connection
/// events, and acts on what it returns; it holds no sockets, so it is testable by itself.
pub struct Tracker {
    key: StreamKey,
    role: Role,
    rendezvous: Option<Rendezvous>,
    status: StreamStatus,
    viewers: usize,
    paused: bool,
}

impl Tracker {
    pub fn new(key: StreamKey, role: Role, user_id: u64, session_id: String, now: Instant) -> Self {
        Self {
            key,
            role,
            rendezvous: Some(Rendezvous::new(key, role, user_id, session_id, now)),
            status: StreamStatus::Starting,
            viewers: 0,
            paused: false,
        }
    }

    pub fn key(&self) -> StreamKey {
        self.key
    }

    pub fn status(&self) -> StreamStatus {
        self.status
    }

    pub fn current(&self) -> StreamUpdate {
        StreamUpdate {
            key: self.key,
            role: self.role,
            status: self.status,
            viewers: self.viewers,
        }
    }

    fn set(&mut self, status: StreamStatus) -> Option<StreamUpdate> {
        if matches!(self.status, StreamStatus::Ended(_)) || self.status == status {
            return None;
        }
        self.status = status;
        Some(self.current())
    }

    fn ended(&mut self, reason: EndReason) -> Option<StreamUpdate> {
        self.rendezvous = None;
        self.set(StreamStatus::Ended(reason))
    }

    pub fn dispatch(&mut self, dispatch: &Dispatch, _now: Instant) -> Step {
        if dispatch.key() != self.key {
            return Step::default();
        }
        let mut step = Step::default();
        match dispatch {
            Dispatch::Create {
                viewers, paused, ..
            } => {
                self.viewers = viewers.len();
                self.paused = *paused;
                step.update = self.set(StreamStatus::Connecting);
            }
            Dispatch::Update {
                viewers, paused, ..
            } => {
                let before = (self.viewers, self.paused);
                if let Some(viewers) = viewers {
                    self.viewers = viewers.len();
                }
                self.paused = *paused;
                if self.paused != before.1 {
                    let status = match (self.paused, self.status) {
                        (true, StreamStatus::Live) => Some(StreamStatus::Paused),
                        (false, StreamStatus::Paused) => Some(StreamStatus::Live),
                        _ => None,
                    };
                    if let Some(status) = status {
                        step.update = self.set(status);
                    }
                }
                if step.update.is_none() && self.viewers != before.0 {
                    step.update = Some(self.current());
                }
            }
            Dispatch::Delete { reason, .. } => step.update = self.ended(*reason),
            Dispatch::ServerUpdate { .. } => {}
        }
        if let Some(rendezvous) = &mut self.rendezvous
            && let Some(join) = rendezvous.feed(dispatch)
        {
            self.rendezvous = None;
            step.join = Some(join);
        }
        step
    }

    /// An event of the stream voice connection.
    pub fn connection(&mut self, event: &crate::StreamEvent) -> Option<StreamUpdate> {
        use crate::{DisconnectReason, StreamEvent};
        match event {
            StreamEvent::Live { .. } => self.set(if self.paused {
                StreamStatus::Paused
            } else {
                StreamStatus::Live
            }),
            // A watcher is live as soon as the connection is up; its pictures follow.
            StreamEvent::Connected if self.role == Role::Watch => self.set(StreamStatus::Live),
            StreamEvent::Ended { reason } => self.ended(match reason {
                DisconnectReason::Requested => EndReason::UserRequested,
                _ => EndReason::Failed,
            }),
            _ => None,
        }
    }

    /// Ends the stream as failed when the two halves did not arrive in time.
    pub fn tick(&mut self, now: Instant) -> Option<StreamUpdate> {
        if self.rendezvous.as_ref().is_some_and(|r| r.expired(now)) {
            return self.ended(EndReason::Failed);
        }
        None
    }

    /// The caller ended the stream (op 19 sent, or the watch left).
    pub fn stopped(&mut self) -> Option<StreamUpdate> {
        self.ended(EndReason::UserRequested)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_in_both_shapes() {
        let guild = StreamKey::parse("guild:11:22:33").unwrap();
        assert_eq!(
            guild,
            StreamKey::Guild {
                guild: 11,
                channel: 22,
                owner: 33
            }
        );
        assert_eq!(guild.to_string(), "guild:11:22:33");
        let call = StreamKey::parse("call:22:33").unwrap();
        assert_eq!(call.to_string(), "call:22:33");
        assert_eq!((call.owner(), call.channel(), call.guild()), (33, 22, None));
        for bad in [
            "",
            "guild:1:2",
            "call:1:2:3",
            "stage:1:2:3",
            "guild:a:2:3",
            "call:0:3",
            "guild:1:2:3:4",
        ] {
            assert!(StreamKey::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn op_payloads_match_the_bundle() {
        assert_eq!(
            create(Some(1), 2, None),
            json!({"type": "guild", "guild_id": "1", "channel_id": "2", "preferred_region": null})
        );
        assert_eq!(
            create(None, 2, Some("rotterdam")),
            json!({"type": "call", "guild_id": null, "channel_id": "2", "preferred_region": "rotterdam"})
        );
        let key = StreamKey::parse("call:2:3").unwrap();
        assert_eq!(delete(&key), json!({"stream_key": "call:2:3"}));
        assert_eq!(watch(&key), json!({"stream_key": "call:2:3"}));
        assert_eq!(ping(&key), json!({"stream_key": "call:2:3"}));
        assert_eq!(
            set_paused(&key, true),
            json!({"stream_key": "call:2:3", "paused": true})
        );
    }

    #[test]
    fn dispatches_parse_and_malformed_ones_do_not() {
        let created = parse_dispatch(
            "STREAM_CREATE",
            &json!({"stream_key": "guild:1:2:3", "rtc_server_id": "900", "rtc_channel_id": "899",
                "region": "us-east", "viewer_ids": ["5", "6"], "paused": false}),
        )
        .unwrap();
        assert_eq!(
            created,
            Dispatch::Create {
                key: StreamKey::parse("guild:1:2:3").unwrap(),
                rtc_server_id: 900,
                rtc_channel_id: Some(899),
                region: Some("us-east".into()),
                viewers: vec![5, 6],
                paused: false
            }
        );
        let update = parse_dispatch(
            "STREAM_UPDATE",
            &json!({"stream_key": "call:2:3", "viewer_ids": ["7"], "paused": true}),
        )
        .unwrap();
        assert!(
            matches!(update, Dispatch::Update { viewers: Some(ref v), paused: true, .. } if v == &[7])
        );
        let ended = |data: Value| match parse_dispatch("STREAM_DELETE", &data).unwrap() {
            Dispatch::Delete { reason, .. } => reason,
            _ => unreachable!(),
        };
        assert_eq!(
            ended(json!({"stream_key": "call:2:3", "reason": "stream_full"})),
            EndReason::StreamFull
        );
        assert_eq!(
            ended(json!({"stream_key": "call:2:3", "reason": "unauthorized"})),
            EndReason::Unauthorized
        );
        assert_eq!(
            ended(json!({"stream_key": "call:2:3", "unavailable": true})),
            EndReason::Unavailable
        );
        assert_eq!(ended(json!({"stream_key": "call:2:3"})), EndReason::Other);
        assert!(parse_dispatch("STREAM_CREATE", &json!({"stream_key": "call:2:3"})).is_none());
        assert!(parse_dispatch("STREAM_SERVER_UPDATE", &json!({"stream_key": "x"})).is_none());
        assert!(parse_dispatch("VOICE_STATE_UPDATE", &json!({"stream_key": "call:2:3"})).is_none());
    }

    #[test]
    fn the_rendezvous_waits_for_both_halves_in_either_order() {
        let now = Instant::now();
        let key = StreamKey::parse("guild:1:2:3").unwrap();
        let create = Dispatch::Create {
            key,
            rtc_server_id: 900,
            rtc_channel_id: None,
            region: None,
            viewers: vec![],
            paused: false,
        };
        let server = Dispatch::ServerUpdate {
            key,
            endpoint: "x.discord.media:443".into(),
            token: "t".into(),
        };
        for order in [[&create, &server], [&server, &create]] {
            let mut rendezvous = Rendezvous::new(key, Role::Send, 3, "session".into(), now);
            assert!(rendezvous.feed(order[0]).is_none());
            let join = rendezvous.feed(order[1]).unwrap();
            assert_eq!(
                (join.server_id, join.channel_id),
                (900, 899),
                "group id is one before the server id"
            );
            assert_eq!(join.endpoint, "x.discord.media:443");
            assert!(rendezvous.feed(&create).is_none(), "only once");
        }
        let other = Dispatch::ServerUpdate {
            key: StreamKey::parse("call:2:3").unwrap(),
            endpoint: "e".into(),
            token: "t".into(),
        };
        let mut rendezvous = Rendezvous::new(key, Role::Watch, 3, "s".into(), now);
        rendezvous.feed(&create);
        assert!(rendezvous.feed(&other).is_none());
        assert!(!rendezvous.expired(now + Duration::from_secs(29)));
        assert!(rendezvous.expired(now + RENDEZVOUS_TIMEOUT));
        let shown = format!("{:?}", rendezvous.feed(&server).unwrap());
        assert!(!shown.contains("discord.media") && !shown.contains("session"));
    }

    #[test]
    fn the_tracker_follows_a_stream_from_request_to_end() {
        let now = Instant::now();
        let key = StreamKey::parse("guild:1:2:3").unwrap();
        let mut tracker = Tracker::new(key, Role::Send, 3, "s".into(), now);
        assert_eq!(tracker.status(), StreamStatus::Starting);
        let create = Dispatch::Create {
            key,
            rtc_server_id: 900,
            rtc_channel_id: None,
            region: None,
            viewers: vec![],
            paused: false,
        };
        let step = tracker.dispatch(&create, now);
        assert_eq!(step.update.unwrap().status, StreamStatus::Connecting);
        assert!(step.join.is_none());
        let step = tracker.dispatch(
            &Dispatch::ServerUpdate {
                key,
                endpoint: "x.discord.media".into(),
                token: "t".into(),
            },
            now,
        );
        assert!(step.join.is_some() && step.update.is_none());
        let live = tracker
            .connection(&crate::StreamEvent::Live { privacy_code: None })
            .unwrap();
        assert_eq!((live.status, live.viewers), (StreamStatus::Live, 0));
        let update = |viewers: Option<Vec<u64>>, paused: bool| Dispatch::Update {
            key,
            viewers,
            paused,
        };
        let viewers = tracker
            .dispatch(&update(Some(vec![7, 8]), false), now)
            .update
            .unwrap();
        assert_eq!(viewers.viewers, 2);
        let paused = tracker.dispatch(&update(None, true), now).update.unwrap();
        assert_eq!(paused.status, StreamStatus::Paused);
        let delete = Dispatch::Delete {
            key,
            reason: EndReason::StreamFull,
        };
        let ended = tracker.dispatch(&delete, now).update.unwrap();
        assert_eq!(ended.status, StreamStatus::Ended(EndReason::StreamFull));
        assert!(tracker.stopped().is_none(), "an ended stream stays ended");
        assert!(tracker.tick(now + Duration::from_secs(60)).is_none());
    }

    #[test]
    fn a_stream_that_never_gets_its_server_fails_after_thirty_seconds() {
        let now = Instant::now();
        let key = StreamKey::parse("call:2:3").unwrap();
        let mut tracker = Tracker::new(key, Role::Watch, 9, "s".into(), now);
        assert!(tracker.tick(now + Duration::from_secs(29)).is_none());
        let failed = tracker.tick(now + RENDEZVOUS_TIMEOUT).unwrap();
        assert_eq!(failed.status, StreamStatus::Ended(EndReason::Failed));
        // A watcher is live once connected; an ended connection ends the stream.
        let mut tracker = Tracker::new(key, Role::Watch, 9, "s".into(), now);
        let live = tracker.connection(&crate::StreamEvent::Connected).unwrap();
        assert_eq!(live.status, StreamStatus::Live);
        let gone = crate::StreamEvent::Ended {
            reason: crate::DisconnectReason::Failed("x"),
        };
        let ended = tracker.connection(&gone).unwrap();
        assert_eq!(ended.status, StreamStatus::Ended(EndReason::Failed));
    }
}
