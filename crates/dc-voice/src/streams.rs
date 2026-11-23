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
    /// The stream is in a Stage channel, which may run at DAVE protocol version 0 (see [`crate::Join::stage`]). The
    /// tracker does not know channel types: the app sets it.
    pub stage: bool,
}

impl fmt::Debug for StreamJoin {
    /// Secrets and ids never reach logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamJoin")
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

/// Waits for `STREAM_CREATE` and `STREAM_SERVER_UPDATE` of one stream, in either order. Once the stream exists, every
/// later `STREAM_SERVER_UPDATE` names the server to connect to again, as the official `STREAM_SERVER_UPDATE` handler
/// of module 753181 calls `connect(endpoint, token)` on the stream's existing connection each time.
pub struct Rendezvous {
    key: StreamKey,
    role: Role,
    user_id: u64,
    session_id: String,
    started: Instant,
    create: Option<(u64, u64)>,
    server: Option<(String, String)>,
    /// The server of the last join, which a lost connection goes back to.
    last: Option<(String, String)>,
    /// A join was handed out: the thirty second bound no longer applies.
    joined: bool,
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
            last: None,
            joined: false,
        }
    }

    /// Takes a dispatch of this stream; returns the join once both halves are in, and again for each later
    /// `STREAM_SERVER_UPDATE` (the stream moved to another server, or its token was renewed). A repeated
    /// `STREAM_CREATE` of a stream that exists joins nothing (`null == o` in the official handler).
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
                if self.create.is_none() {
                    let channel = rtc_channel_id.unwrap_or(rtc_server_id.saturating_sub(1));
                    self.create = Some((*rtc_server_id, channel));
                }
            }
            Dispatch::ServerUpdate {
                endpoint, token, ..
            } => self.server = Some((endpoint.clone(), token.clone())),
            _ => return None,
        }
        self.create?;
        self.last = Some(self.server.take()?);
        self.joined = true;
        self.rejoin()
    }

    /// The join of the server last named, for a connection that was lost (`None` before the first join).
    pub fn rejoin(&self) -> Option<StreamJoin> {
        let ((server_id, channel_id), (endpoint, token)) = (self.create?, self.last.clone()?);
        Some(StreamJoin {
            key: self.key,
            role: self.role,
            endpoint,
            token,
            session_id: self.session_id.clone(),
            server_id,
            channel_id,
            user_id: self.user_id,
            stage: false,
        })
    }

    /// The first join did not come within thirty seconds.
    pub fn expired(&self, now: Instant) -> bool {
        !self.joined && now.saturating_duration_since(self.started) >= RENDEZVOUS_TIMEOUT
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
    /// Who watches (`viewer_ids` of the stream's dispatches), for the viewer pile and its list.
    pub viewer_ids: Vec<u64>,
}

/// What a dispatch led to.
#[derive(Debug, Default)]
pub struct Step {
    /// Both halves are in, or the stream named a server again: (re)start the voice connection with this.
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
    viewer_ids: Vec<u64>,
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
            viewer_ids: Vec::new(),
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
            viewer_ids: self.viewer_ids.clone(),
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
                self.viewer_ids = viewers.clone();
                self.paused = *paused;
                // A repeated create of a stream that is up changes nothing of its connection.
                if self.status == StreamStatus::Starting {
                    step.update = self.set(StreamStatus::Connecting);
                }
            }
            Dispatch::Update {
                viewers, paused, ..
            } => {
                let before = (self.viewer_ids.clone(), self.paused);
                if let Some(viewers) = viewers {
                    self.viewers = viewers.len();
                    self.viewer_ids = viewers.clone();
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
                if step.update.is_none() && self.viewer_ids != before.0 {
                    step.update = Some(self.current());
                }
            }
            // `STREAM_DELETE` with `unavailable` (the stream's server went away): the official store keeps the stream
            // as RECONNECTING (module 616356) and its next `STREAM_CREATE` and `STREAM_SERVER_UPDATE` join again. Ended
            // here, the watcher was left with the Watch Stream card.
            Dispatch::Delete {
                reason: EndReason::Unavailable,
                ..
            } => step.update = self.set(StreamStatus::Connecting),
            Dispatch::Delete { reason, .. } => step.update = self.ended(*reason),
            Dispatch::ServerUpdate { .. } => {}
        }
        if let Some(rendezvous) = &mut self.rendezvous
            && let Some(join) = rendezvous.feed(dispatch)
        {
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

    /// The stream's voice connection was lost without being asked to end (module 921747 `_handleDisconnect`: anything
    /// but a "Force Close" reconnects; the stream itself ends only with `STREAM_DELETE` or our own stop). Returns the
    /// join to connect with again, and the status change to Connecting when there is one. `None` when the stream has
    /// ended or never had a server.
    pub fn lost(&mut self) -> Option<(StreamJoin, Option<StreamUpdate>)> {
        if matches!(self.status, StreamStatus::Ended(_)) {
            return None;
        }
        let join = self.rendezvous.as_ref()?.rejoin()?;
        Some((join, self.set(StreamStatus::Connecting)))
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
        assert_eq!(
            viewers.viewer_ids,
            vec![7, 8],
            "the pile needs who watches, not how many"
        );
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

    /// Module 753181: `STREAM_SERVER_UPDATE` connects the stream's existing connection again each time; a repeated
    /// `STREAM_CREATE` does not. A watch that was joined is never failed by the thirty second bound.
    #[test]
    fn a_later_server_update_moves_the_stream_connection() {
        let now = Instant::now();
        let key = StreamKey::parse("call:2:3").unwrap();
        let mut tracker = Tracker::new(key, Role::Watch, 9, "s".into(), now);
        let create = |server: u64| Dispatch::Create {
            key,
            rtc_server_id: server,
            rtc_channel_id: Some(77),
            region: None,
            viewers: vec![9],
            paused: false,
        };
        let server = |endpoint: &str, token: &str| Dispatch::ServerUpdate {
            key,
            endpoint: endpoint.into(),
            token: token.into(),
        };
        assert!(tracker.dispatch(&create(900), now).join.is_none());
        let first = tracker
            .dispatch(&server("a.discord.media", "t1"), now)
            .join
            .unwrap();
        assert_eq!(
            (first.server_id, first.channel_id, first.endpoint.as_str()),
            (900, 77, "a.discord.media")
        );
        tracker.connection(&crate::StreamEvent::Connected);
        assert!(
            tracker.dispatch(&create(901), now).join.is_none(),
            "the stream exists: a second create joins nothing and keeps its server"
        );
        let moved = tracker
            .dispatch(&server("b.discord.media", "t2"), now)
            .join
            .unwrap();
        assert_eq!(
            (
                moved.server_id,
                moved.endpoint.as_str(),
                moved.token.as_str()
            ),
            (900, "b.discord.media", "t2")
        );
        assert_eq!(moved.session_id, "s");
        assert_eq!(
            tracker.status(),
            StreamStatus::Live,
            "a move is not a new stream"
        );
        assert!(tracker.tick(now + Duration::from_secs(120)).is_none());
        let other = Dispatch::ServerUpdate {
            key: StreamKey::parse("call:2:4").unwrap(),
            endpoint: "c".into(),
            token: "t".into(),
        };
        assert!(tracker.dispatch(&other, now).join.is_none());
        tracker.stopped();
        assert!(
            tracker
                .dispatch(&server("c.discord.media", "t3"), now)
                .join
                .is_none(),
            "an ended stream stays ended"
        );
    }

    /// A lost connection goes back to the server last named with its token; nothing to rejoin before the first
    /// join or after the stream ended.
    #[test]
    fn a_lost_connection_rejoins_the_last_server_until_the_stream_ends() {
        let now = Instant::now();
        let key = StreamKey::parse("guild:1:2:3").unwrap();
        let mut tracker = Tracker::new(key, Role::Watch, 9, "s".into(), now);
        assert!(tracker.lost().is_none(), "no server yet");
        let create = Dispatch::Create {
            key,
            rtc_server_id: 900,
            rtc_channel_id: None,
            region: None,
            viewers: vec![],
            paused: false,
        };
        tracker.dispatch(&create, now);
        assert!(tracker.lost().is_none(), "half a rendezvous");
        let server = |endpoint: &str, token: &str| Dispatch::ServerUpdate {
            key,
            endpoint: endpoint.into(),
            token: token.into(),
        };
        tracker.dispatch(&server("a", "t1"), now);
        tracker.connection(&crate::StreamEvent::Connected);
        let (join, update) = tracker.lost().unwrap();
        assert_eq!(
            (
                join.endpoint.as_str(),
                join.token.as_str(),
                join.server_id,
                join.channel_id
            ),
            ("a", "t1", 900, 899)
        );
        assert_eq!(update.unwrap().status, StreamStatus::Connecting);
        let (_, update) = tracker.lost().unwrap();
        assert!(
            update.is_none(),
            "still connecting: no second status change"
        );
        tracker.dispatch(&server("b", "t2"), now);
        assert_eq!(
            tracker.lost().unwrap().0.endpoint,
            "b",
            "the newest server is the one to go back to"
        );
        tracker.dispatch(
            &Dispatch::Delete {
                key,
                reason: EndReason::UserRequested,
            },
            now,
        );
        assert!(
            tracker.lost().is_none(),
            "an ended stream is not reconnected"
        );
    }

    /// `STREAM_DELETE` with `unavailable`: the stream waits to come back (RECONNECTING, module 616356) and its next
    /// server joins again, where any other reason ends it.
    #[test]
    fn an_unavailable_stream_waits_and_rejoins_its_next_server() {
        let now = Instant::now();
        let key = StreamKey::parse("call:2:3").unwrap();
        let mut tracker = Tracker::new(key, Role::Watch, 9, "s".into(), now);
        let create = Dispatch::Create {
            key,
            rtc_server_id: 900,
            rtc_channel_id: None,
            region: None,
            viewers: vec![],
            paused: false,
        };
        let server = |endpoint: &str| Dispatch::ServerUpdate {
            key,
            endpoint: endpoint.into(),
            token: "t".into(),
        };
        tracker.dispatch(&create, now);
        assert!(tracker.dispatch(&server("a"), now).join.is_some());
        let update = tracker
            .dispatch(
                &Dispatch::Delete {
                    key,
                    reason: EndReason::Unavailable,
                },
                now,
            )
            .update;
        assert_eq!(tracker.status(), StreamStatus::Connecting);
        assert!(update.is_none() || update.is_some_and(|u| u.status == StreamStatus::Connecting));
        tracker.dispatch(&create, now);
        assert!(
            tracker.dispatch(&server("b"), now).join.is_some(),
            "the stream's next server joins again"
        );
        tracker.dispatch(
            &Dispatch::Delete {
                key,
                reason: EndReason::Other,
            },
            now,
        );
        assert_eq!(tracker.status(), StreamStatus::Ended(EndReason::Other));
    }

    #[test]
    fn only_socket_losses_reconnect() {
        use crate::DisconnectReason;
        assert!(!crate::ended_locally(&DisconnectReason::Closed(4009)));
        assert!(!crate::ended_locally(&DisconnectReason::Kicked));
        assert!(!crate::ended_locally(&DisconnectReason::Failed(
            "Stream voice Hello timed out"
        )));
        assert!(crate::ended_locally(&DisconnectReason::Requested));
        assert!(crate::ended_locally(&DisconnectReason::Failed(
            "Screen capture ended"
        )));
        assert!(crate::ended_locally(&DisconnectReason::Failed(
            "Screen capture could not start"
        )));
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
