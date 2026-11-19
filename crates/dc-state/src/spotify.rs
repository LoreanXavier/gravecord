//! Token-free player/device state and activity controls from modules 655116, 286617 and 107750.
//! The network worker owns credentials; this store only receives account and playback facts.

use crate::{Id, id};
use serde_json::{Value, json};

/// Provider player snapshots become the same token-free `SPOTIFY_PLAYER_STATE` payload as 655116 `ec`.
pub fn player_event(account: &str, state: &Value) -> Value {
    let item = &state["item"];
    let episode = item["type"] == "episode";
    let track = if matches!(item["type"].as_str(), Some("track" | "episode")) {
        let album = if episode {
            &item["show"]
        } else {
            &item["album"]
        };
        let artists: Vec<Value> = if episode {
            Vec::new()
        } else {
            item["artists"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|artist| {
                    let id = artist["id"].as_str()?;
                    let name = artist["name"].as_str()?;
                    Some(json!({"id": id, "name": name}))
                })
                .collect()
        };
        let image = album["images"].as_array().and_then(|images| images.first()).map(|image| {
            json!({"url": image["url"], "width": image["width"], "height": image["height"]})
        });
        json!({
            "id": if episode { item["id"].as_str() } else { item["linked_from"]["id"].as_str().or_else(|| item["id"].as_str()) },
            "name": item["name"].as_str().unwrap_or_default(),
            "duration": item["duration_ms"].as_i64().unwrap_or_default(),
            "type": if episode { "episode" } else { "track" },
            "album": {"id": album["id"].as_str().unwrap_or_default(),
                "name": album["name"].as_str().unwrap_or_default(), "image": image,
                "type": if episode { "show" } else { album["type"].as_str().unwrap_or("album") }},
            "artists": artists,
            "isLocal": !episode && item["is_local"].as_bool().unwrap_or(false),
        })
    } else {
        Value::Null
    };
    let device = if state["device"].is_object() {
        let d = &state["device"];
        json!({"id": d["id"], "name": d["name"], "type": d["type"], "is_active": true,
            "is_restricted": d["is_restricted"].as_bool().unwrap_or(false),
            "volume_percent": d["volume_percent"]})
    } else {
        Value::Null
    };
    json!({"accountId": account, "track": track, "device": device,
        "volumePercent": state["device"]["volume_percent"].as_i64().unwrap_or_default(),
        "isPlaying": state["is_playing"].as_bool().unwrap_or(false),
        "repeat": !state.is_null() && state["repeat_state"] != "off", "position": state["progress_ms"].as_i64().unwrap_or_default(),
        "context": null})
}

/// Where the `context` of a player state comes from (`ec` of module 655116).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextSource {
    /// Not an album or a playlist: no context.
    None,
    /// The album itself, or the context the account's player already holds for the same uri.
    Ready(Value),
    /// A playlist not seen yet: fetched by its id (`GET` of the context's `href`), then kept only if public.
    Playlist(String),
}

/// `ec`: an album or playlist context of a provider player snapshot. `previous` is the context the account's current
/// player state holds (`getPlayerState(e).context`).
pub fn context_source(state: &Value, previous: Option<&Value>) -> ContextSource {
    let context = &state["context"];
    let kind = context["type"].as_str();
    if !matches!(kind, Some("playlist" | "album")) {
        return ContextSource::None;
    }
    if let Some(previous) = previous
        && previous["uri"].is_string()
        && previous["uri"] == context["uri"]
    {
        return ContextSource::Ready(previous.clone());
    }
    if kind == Some("album") {
        return ContextSource::Ready(context.clone());
    }
    // The fetch goes to the playback API with the account's credential: only a base-62 playlist id is taken.
    let id = context["href"]
        .as_str()
        .and_then(|href| href.strip_prefix("https://api.spotify.com/v1/playlists/"))
        .or_else(|| context["uri"].as_str()?.strip_prefix("spotify:playlist:"))
        .map(|id| id.split(['?', '/']).next().unwrap_or_default())
        .filter(|id| {
            !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric())
        });
    match id {
        Some(id) => ContextSource::Playlist(id.to_owned()),
        None => ContextSource::None,
    }
}

/// The fetched context as the player state keeps it: a private playlist is none (`t.type === PLAYLIST && !t.public`).
pub fn public_context(context: Value) -> Value {
    if context["type"] == "playlist" && context["public"] != true {
        Value::Null
    } else {
        context
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub show_activity: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub active: bool,
    pub restricted: bool,
    pub computer: bool,
}

impl Device {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: value["id"].as_str()?.to_owned(),
            active: value["is_active"].as_bool().unwrap_or(false),
            restricted: value["is_restricted"].as_bool().unwrap_or(false),
            computer: value["type"] == "Computer",
        })
    }
}

#[derive(Clone, Debug, Default)]
struct Socket {
    account_id: String,
    connected: bool,
    premium: Option<bool>,
    devices: Vec<Device>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Track,
    Artist,
    Album,
    Playlist,
    Episode,
    Show,
}

impl Resource {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "track" => Self::Track,
            "artist" => Self::Artist,
            "album" => Self::Album,
            "playlist" => Self::Playlist,
            "episode" => Self::Episode,
            "show" => Self::Show,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Playlist => "playlist",
            Self::Episode => "episode",
            Self::Show => "show",
        }
    }
}

/// The track of a `SPOTIFY_PLAYER_STATE` (`ec` of module 655116): a track, or an episode with its show as the album.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub duration: i64,
    /// `track` or `episode`.
    pub kind: String,
    pub album_id: String,
    pub album_name: String,
    pub album_image_url: Option<String>,
    /// `album`, `single`, `compilation` or `show`.
    pub album_type: String,
    /// `(id, name)`, in the provider's order.
    pub artists: Vec<(String, String)>,
    pub is_local: bool,
}

impl Track {
    fn from_value(value: &Value) -> Option<Self> {
        let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
        let album = &value["album"];
        Some(Self {
            id: value["id"].as_str()?.to_owned(),
            name: text(&value["name"]),
            duration: value["duration"].as_i64().unwrap_or_default(),
            kind: value["type"].as_str().unwrap_or("track").to_owned(),
            album_id: text(&album["id"]),
            album_name: text(&album["name"]),
            album_image_url: album["image"]["url"].as_str().map(str::to_owned),
            album_type: album["type"].as_str().unwrap_or("album").to_owned(),
            artists: value["artists"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|artist| {
                    Some((
                        artist["id"].as_str()?.to_owned(),
                        artist["name"].as_str()?.to_owned(),
                    ))
                })
                .collect(),
            is_local: value["isLocal"].as_bool().unwrap_or(false),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub account_id: String,
    pub track_id: String,
    pub start_time: i64,
    pub repeat: bool,
    pub track: Track,
    /// The uri of the album or public playlist the track plays from (`context` of the player state).
    pub context_uri: Option<String>,
}

/// `HS` of module 272984: the party id prefix of a Spotify activity.
pub const PARTY_PREFIX: &str = "spotify:";
/// `p` of module 913659: Spotify album art is sent as `spotify:` and the part of its URL after this.
const IMAGE_PREFIX: &str = "https://i.scdn.co/image/";
/// `jUm.PLAY | jUm.SYNC` (module 652215): the activity can be played and listened along to.
pub const ACTIVITY_FLAGS: u64 = 32 | 16;
/// `G` of module 655116: the player state is cleared this long after the track should have ended.
pub const TRACK_END_GRACE_MS: i64 = 5_000;
/// `G` of module 655116: a listen-along whose own playback stopped or moved to another track ends this long after.
pub const LISTEN_ALONG_STOP_MS: i64 = 5_000;
/// `x` of module 655116: a new start within this of the last one keeps the last one (player clock jitter).
const START_JITTER_MS: i64 = 1_500;

/// `I.length > 128 ? I.substring(0, 125) + "..." : I`, counted in UTF-16 units like the JS string.
fn truncate_details(name: &str) -> String {
    if name.encode_utf16().count() <= 128 {
        return name.to_owned();
    }
    let mut units = 0;
    let mut cut = String::new();
    for c in name.chars() {
        units += c.len_utf16();
        if units > 125 {
            break;
        }
        cut.push(c);
    }
    cut.push_str("...");
    cut
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncingWith {
    pub user_id: Id,
    pub party_id: String,
    pub track_id: String,
    pub start_time: i64,
}

/// What the host's presence asks of a listen-along (`syncWith(PresenceStore)` of module 655116).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncStep {
    /// Not the host, or nothing changed.
    Nothing,
    /// The host's track or its start moved: play it at the host's position (`ea(…, false)`, "presence change").
    Replay(Play),
    /// The host has no Spotify activity: the sync ends unless it comes back within the grace (`w`, five minutes).
    HostGone,
    /// The host's activity is there again: the grace timer stops.
    HostBack,
}

/// `w` of module 655116: how long a listen-along waits for the host's activity to come back.
pub const LISTEN_ALONG_GRACE_MS: u64 = 5 * 60 * 1000;
/// `U` of module 655116: speaking this long in a voice channel while Spotify plays pauses it.
pub const AUTO_PAUSE_MS: u64 = 30 * 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Play {
    pub account_id: String,
    pub device_id: String,
    pub track_id: String,
    pub resource: Resource,
    pub position_ms: i64,
    pub context_uri: Option<String>,
    /// A separate repeat request follows successful playback; `None` leaves it unchanged.
    pub repeat: Option<bool>,
}

impl Play {
    /// Module 272984 PLAYER_OPEN omits attribution parameters for the playback API.
    pub fn body(&self) -> Value {
        let uri = format!(
            "spotify:{}:{}",
            self.resource.as_str(),
            component(&self.track_id)
        );
        let mut body = json!({"position_ms": self.position_ms});
        if let Some(context) = &self.context_uri {
            body["context_uri"] = json!(context);
            body["offset"] = json!({"uri": uri});
        } else {
            body["uris"] = json!([uri]);
        }
        body
    }
}

fn component(value: &str) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            result.push(char::from(byte));
        } else {
            let _ = write!(result, "%{byte:02X}");
        }
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Controls {
    pub has_account: bool,
    pub not_playable: bool,
    pub own: bool,
    pub same_track: bool,
    pub same_party: bool,
    pub same_user: bool,
}

impl Controls {
    pub fn play_disabled(self) -> bool {
        self.own || self.not_playable || self.same_track
    }

    /// 353411 includes `notPlayable` in the sync button, in addition to 286617's sync state.
    pub fn sync_disabled(self) -> bool {
        self.own || self.not_playable || self.same_party || self.same_user
    }
}

#[derive(Default)]
pub struct SpotifyStore {
    accounts: Vec<Account>,
    sockets: Vec<Socket>,
    players: Vec<Player>,
    player_order: Vec<String>,
    last_played_track: Option<String>,
    syncing: Option<SyncingWith>,
    protocol_registered: bool,
    /// `W` of module 655116: the account whose player state is cleared at this time (the track's end plus `G`).
    track_end: Option<(String, i64)>,
    /// `Y` of module 655116: when a listen-along whose own playback stopped ends, and the account to clear then.
    listen_along_stop: Option<(String, i64)>,
}

impl SpotifyStore {
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    pub fn has_connected_account(&self) -> bool {
        !self.sockets.is_empty()
    }

    pub fn syncing_with(&self) -> Option<&SyncingWith> {
        self.syncing.as_ref()
    }

    pub fn player(&self) -> Option<&Player> {
        self.players.first()
    }

    pub fn current_track_id(&self) -> Option<&str> {
        self.player()
            .map(|player| player.track_id.as_str())
            .or_else(|| self.syncing.as_ref().map(|sync| sync.track_id.as_str()))
            .or(self.last_played_track.as_deref())
    }

    pub fn active_device(&self) -> Option<(&str, &Device)> {
        self.sockets
            .iter()
            .filter(|socket| socket.connected)
            .find_map(|socket| {
                socket
                    .devices
                    .iter()
                    .find(|device| device.active)
                    .map(|device| (socket.account_id.as_str(), device))
            })
    }

    pub fn playable_computers(&self) -> Vec<(&str, &Device)> {
        self.sockets
            .iter()
            .filter(|socket| socket.connected)
            .filter_map(|socket| {
                socket
                    .devices
                    .iter()
                    .find(|device| device.computer && !device.restricted)
                    .map(|device| (socket.account_id.as_str(), device))
            })
            .collect()
    }

    pub fn active_premium(&self) -> Option<bool> {
        let (account, _) = self.active_device()?;
        self.sockets
            .iter()
            .find(|socket| socket.account_id == account)?
            .premium
    }

    /// A socket is playable while connecting or open (655116's `ee`), never once closed.
    pub fn set_socket_connected(&mut self, account: &str, connected: bool) {
        if let Some(socket) = self
            .sockets
            .iter_mut()
            .find(|socket| socket.account_id == account)
        {
            socket.connected = connected;
        }
    }

    pub fn controls(&self, me: Id, user: Id, activity: &Value) -> Controls {
        let track = self.current_track_id();
        let current_party = self
            .syncing
            .as_ref()
            .map(|sync| sync.party_id.clone())
            .or_else(|| self.player().map(|_| format!("spotify:{me}")));
        Controls {
            has_account: self.has_connected_account(),
            not_playable: self.has_connected_account()
                && self.active_device().is_none()
                && !self.protocol_registered,
            own: me == user,
            same_track: track.is_some() && track == activity["sync_id"].as_str(),
            same_party: current_party.is_some()
                && current_party.as_deref() == activity["party"]["id"].as_str(),
            same_user: self
                .syncing
                .as_ref()
                .is_some_and(|sync| sync.user_id == user),
        }
    }

    fn replace_accounts(&mut self, values: &Value) {
        self.accounts = values
            .as_array()
            .into_iter()
            .flatten()
            .filter(|value| value["type"] == "spotify")
            .filter_map(|value| {
                Some(Account {
                    id: value["id"].as_str()?.to_owned(),
                    show_activity: value["show_activity"].as_bool().unwrap_or(false),
                })
            })
            .collect();
        self.sockets
            .retain(|socket| self.accounts.iter().any(|a| a.id == socket.account_id));
        self.players
            .retain(|player| self.accounts.iter().any(|a| a.id == player.account_id));
        self.player_order
            .retain(|account| self.accounts.iter().any(|a| a.id == *account));
    }

    pub fn dispatch(&mut self, name: &str, data: &Value, now_ms: i64) {
        match name {
            "READY" => self.replace_accounts(&data["connected_accounts"]),
            "CONNECTION_OPEN" => self.replace_accounts(&data["connectedAccounts"]),
            "USER_CONNECTIONS_UPDATE" if data["local"] == true && data["accounts"].is_array() => {
                self.replace_accounts(&data["accounts"]);
            }
            "USER_CONNECTION_UPDATE" if data["platformType"] == "spotify" => {
                if let Some(account) = self.accounts.iter_mut().find(|a| data["id"] == a.id)
                    && let Some(show) = data["showActivity"].as_bool()
                {
                    account.show_activity = show;
                }
            }
            "SPOTIFY_SET_PROTOCOL_REGISTERED" => {
                self.protocol_registered = data["isRegistered"].as_bool().unwrap_or(false);
            }
            "SPOTIFY_ACCOUNT_ACCESS_TOKEN" => {
                // Only the fact of an accepted credential crosses into the UI store.
                if let Some(account) = data["accountId"].as_str()
                    && self.accounts.iter().any(|a| a.id == account)
                    && !self
                        .sockets
                        .iter()
                        .any(|socket| socket.account_id == account)
                {
                    self.sockets.push(Socket {
                        account_id: account.into(),
                        connected: true,
                        ..Socket::default()
                    });
                }
            }
            "SPOTIFY_ACCOUNT_ACCESS_TOKEN_REVOKE" => {
                if let Some(account) = data["accountId"].as_str() {
                    self.sockets.retain(|socket| socket.account_id != account);
                    self.players.retain(|player| player.account_id != account);
                    self.player_order.retain(|known| known != account);
                }
            }
            "SPOTIFY_PROFILE_UPDATE" => {
                if let Some(socket) = self
                    .sockets
                    .iter_mut()
                    .find(|s| data["accountId"] == s.account_id)
                {
                    socket.premium = data["isPremium"].as_bool();
                }
            }
            "SPOTIFY_SET_DEVICES" => {
                if let Some(socket) = self
                    .sockets
                    .iter_mut()
                    .find(|s| data["accountId"] == s.account_id)
                {
                    socket.devices = data["devices"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Device::from_value)
                        .collect();
                }
            }
            "SPOTIFY_SET_ACTIVE_DEVICE" => {
                if let Some(socket) = self
                    .sockets
                    .iter_mut()
                    .find(|s| data["accountId"] == s.account_id)
                {
                    for device in &mut socket.devices {
                        device.active = data["deviceId"] == device.id;
                    }
                }
            }
            "SPOTIFY_PLAYER_PLAY" => {
                self.last_played_track = data["id"].as_str().map(str::to_owned)
            }
            "SPOTIFY_PLAYER_STATE" => self.player_state(data, now_ms),
            _ => {}
        }
    }

    /// `shouldShowActivity` of module 655116: a track plays, its account has "Display Spotify as your status" on,
    /// and the account is not idle (`IdleStore.isIdle`, ten minutes without input).
    pub fn should_show_activity(&self, idle: bool) -> bool {
        self.player().is_some_and(|player| {
            self.accounts
                .iter()
                .any(|account| account.id == player.account_id && account.show_activity)
        }) && !idle
    }

    /// `getActivity` of module 655116 as `LocalActivityStore` lists it (`{type: LISTENING, ...}`, module 491009 `B`):
    /// the playing track, the first five artists, the album art and name, the track's start and end, the party
    /// (`spotify:<own id>`, or the host's party while listening along) and, unless the track is a local file, the
    /// sync id, the play and sync flags and the metadata a listener's client plays it from.
    ///
    /// Without a player of its own the official getter returns the listened-to host's activity, but
    /// `shouldShowActivity` is then false and `SelfPresenceStore` drops it (module 894768 `V`): nothing of it is ever
    /// sent or shown, so this returns `None` there.
    pub fn activity(&self, me: Id) -> Option<Value> {
        let player = self.player()?;
        let track = &player.track;
        let artists = &track.artists[..track.artists.len().min(5)];
        let mut assets = serde_json::Map::new();
        if let Some(image) = track
            .album_image_url
            .as_deref()
            .and_then(|url| url.split(IMAGE_PREFIX).nth(1))
            .filter(|key| !key.is_empty())
        {
            assets.insert("large_image".into(), json!(format!("spotify:{image}")));
        }
        if track.album_type != "single" {
            assets.insert("large_text".into(), json!(track.album_name));
        }
        let party = self
            .syncing
            .as_ref()
            .map(|sync| sync.party_id.clone())
            .unwrap_or_else(|| format!("{PARTY_PREFIX}{me}"));
        let mut activity = json!({
            "type": 2,
            "name": "Spotify",
            "assets": assets,
            "details": truncate_details(&track.name),
        });
        if !artists.is_empty() {
            let state: Vec<String> = artists
                .iter()
                .map(|(_, name)| name.replace(';', ""))
                .collect();
            activity["state"] = json!(state.join("; "));
        }
        activity["timestamps"] = json!({
            "start": player.start_time,
            "end": player.start_time + track.duration,
        });
        activity["party"] = json!({ "id": party });
        if !track.is_local {
            activity["sync_id"] = json!(track.id);
            activity["flags"] = json!(ACTIVITY_FLAGS);
            let mut metadata = serde_json::Map::new();
            if let Some(context) = &player.context_uri {
                metadata.insert("context_uri".into(), json!(context));
            }
            metadata.insert("album_id".into(), json!(track.album_id));
            metadata.insert(
                "artist_ids".into(),
                json!(artists.iter().map(|(id, _)| id).collect::<Vec<_>>()),
            );
            metadata.insert("type".into(), json!(track.kind));
            metadata.insert("button_urls".into(), json!([]));
            activity["metadata"] = Value::Object(metadata);
        }
        Some(activity)
    }

    /// When the player state is next due to be cleared (`W`, the track's end plus five seconds), if a track plays.
    pub fn track_end_ms(&self) -> Option<i64> {
        self.track_end.as_ref().map(|(_, at)| *at)
    }

    /// When a listen-along whose own playback stopped ends (`Y`), if one is due.
    pub fn listen_along_stop_ms(&self) -> Option<i64> {
        self.listen_along_stop.as_ref().map(|(_, at)| *at)
    }

    /// `W` fired: the track should have ended and no new state came, so the player is cleared (`Z`). True when it
    /// was due.
    pub fn expire_track(&mut self, now_ms: i64) -> bool {
        let Some((account, at)) = self.track_end.clone() else {
            return false;
        };
        if now_ms < at {
            return false;
        }
        self.track_end = None;
        self.player_state(&player_event(&account, &Value::Null), now_ms);
        true
    }

    /// `Y` fired: the listen-along ends (`stopSync`, then `es`) and the account's player is cleared (`Z`). Returns
    /// whether it was due, and the account to pause when the active player still plays the shared track.
    pub fn expire_listen_along(&mut self, now_ms: i64) -> Option<Option<String>> {
        let (account, at) = self.listen_along_stop.clone()?;
        if now_ms < at {
            return None;
        }
        self.listen_along_stop = None;
        let pause = self.stop_sync();
        self.player_state(&player_event(&account, &Value::Null), now_ms);
        Some(pause)
    }

    fn player_state(&mut self, data: &Value, now_ms: i64) {
        let Some(account) = data["accountId"].as_str() else {
            return;
        };
        if !self.accounts.iter().any(|a| a.id == account) {
            return;
        }
        if let Some(device) = Device::from_value(&data["device"])
            && let Some(socket) = self.sockets.iter_mut().find(|s| s.account_id == account)
        {
            let selected = device.id.clone();
            if let Some(existing) = socket.devices.iter_mut().find(|d| d.id == selected) {
                *existing = device;
            } else {
                socket.devices.push(device);
            }
            for device in &mut socket.devices {
                device.active = device.id == selected;
            }
        }
        let previous = self
            .players
            .iter()
            .position(|player| player.account_id == account);
        let position = data["position"].as_i64().unwrap_or(0);
        let playing = data["isPlaying"] == true;
        // A paused player has no track (`o ? … : u = null`): the activity goes at once, there is no pause grace.
        let track = Track::from_value(&data["track"]).filter(|_| playing);
        // A device's zero-position stopped snapshot during handoff does not erase listen-along state (`N`).
        let handoff =
            !data["device"].is_null() && self.syncing.is_some() && position == 0 && !playing;
        // `W`: restarted for every playing track, stopped otherwise.
        self.track_end = match &track {
            Some(track) if !handoff => Some((
                account.to_owned(),
                now_ms + track.duration - position + TRACK_END_GRACE_MS,
            )),
            _ => None,
        };
        // `Y`: a listen-along whose own playback paused, lost its device or moved to another track ends in five
        // seconds unless the shared track plays again first.
        let off_track = track
            .as_ref()
            .zip(self.syncing.as_ref())
            .is_some_and(|(track, sync)| track.id != sync.track_id);
        self.listen_along_stop = if self.syncing.is_some()
            && ((!playing && position > 0) || data["device"].is_null() || off_track)
        {
            Some((account.to_owned(), now_ms + LISTEN_ALONG_STOP_MS))
        } else {
            None
        };
        if handoff {
            return;
        }
        let Some(track) = track else {
            if let Some(index) = previous {
                self.players.remove(index);
            }
            return;
        };
        let proposed = now_ms.saturating_sub(position);
        let start_time = previous
            .map(|index| self.players[index].start_time)
            .filter(|old| proposed.abs_diff(*old) as i64 <= START_JITTER_MS)
            .unwrap_or(proposed);
        let player = Player {
            account_id: account.into(),
            track_id: track.id.clone(),
            start_time,
            repeat: data["repeat"].as_bool().unwrap_or(false),
            track,
            context_uri: data["context"]["uri"].as_str().map(str::to_owned),
        };
        if let Some(index) = previous {
            self.players[index] = player;
        } else {
            if !self.player_order.iter().any(|known| known == account) {
                self.player_order.push(account.into());
            }
            self.players.push(player);
            self.players.sort_by_key(|player| {
                self.player_order
                    .iter()
                    .position(|a| a == &player.account_id)
            });
        }
    }

    /// `ACTIVITY_PLAY` needs metadata; opening a track URL is a separate operation.
    pub fn plan_play(&self, activity: &Value, metadata: &Value) -> Option<Play> {
        if !metadata.is_object() {
            return None;
        }
        let mut play = self.plan(activity, metadata, 0)?;
        play.context_uri = metadata["context_uri"].as_str().map(str::to_owned);
        Some(play)
    }

    fn plan(&self, activity: &Value, metadata: &Value, position_ms: i64) -> Option<Play> {
        let (account, device) = self.active_device()?;
        let party = activity["party"]["id"].as_str()?;
        if !party.starts_with("spotify:") {
            return None;
        }
        let resource = match metadata.get("type").filter(|value| !value.is_null()) {
            Some(value) => Resource::parse(value.as_str()?)?,
            None => Resource::Track,
        };
        Some(Play {
            account_id: account.into(),
            device_id: device.id.clone(),
            track_id: activity["sync_id"].as_str()?.into(),
            resource,
            position_ms,
            context_uri: None,
            repeat: None,
        })
    }

    pub fn start_sync(&mut self, user: Id, activity: &Value, now_ms: i64) -> Option<Play> {
        let start = &activity["timestamps"]["start"];
        let start_time = start
            .as_i64()
            .or_else(|| start.as_str()?.parse().ok())
            .unwrap_or(now_ms);
        let mut play = self.plan(
            activity,
            &activity["metadata"],
            now_ms.saturating_sub(start_time).max(0),
        )?;
        if !self
            .players
            .iter()
            .any(|player| player.account_id == play.account_id && !player.repeat)
        {
            play.repeat = Some(false);
        }
        self.syncing = Some(SyncingWith {
            user_id: user,
            party_id: activity["party"]["id"].as_str()?.into(),
            track_id: play.track_id.clone(),
            start_time,
        });
        Some(play)
    }

    /// Only pause if the active player still has the track we started together (655116 `es`).
    pub fn stop_sync(&mut self) -> Option<String> {
        let sync = self.syncing.take()?;
        let (account, _) = self.active_device()?;
        self.players
            .iter()
            .any(|p| p.account_id == account && p.track_id == sync.track_id)
            .then(|| account.to_owned())
    }

    /// A `PRESENCE_UPDATE` while listening along.
    pub fn presence_step(&mut self, event: &Value, now_ms: i64) -> SyncStep {
        let Some(sync) = self.syncing.clone() else {
            return SyncStep::Nothing;
        };
        if id(&event["user"]["id"]) != Some(sync.user_id) {
            return SyncStep::Nothing;
        }
        let Some((user, activity)) = self.syncing_user_activity(event) else {
            return SyncStep::HostGone;
        };
        let activity = activity.clone();
        let start = &activity["timestamps"]["start"];
        let start = start.as_i64().or_else(|| start.as_str()?.parse().ok());
        let track_moved = activity["sync_id"]
            .as_str()
            .is_some_and(|track| track != sync.track_id);
        let start_moved = start.is_some_and(|start| start != sync.start_time);
        if track_moved || start_moved {
            return match self.start_sync(user, &activity, now_ms) {
                Some(play) => SyncStep::Replay(play),
                None => SyncStep::HostBack,
            };
        }
        if let (Some(party), Some(sync)) = (activity["party"]["id"].as_str(), self.syncing.as_mut())
            && party != sync.party_id
        {
            sync.party_id = party.to_owned();
        }
        SyncStep::HostBack
    }

    pub fn syncing_user_activity<'a>(&self, event: &'a Value) -> Option<(Id, &'a Value)> {
        let sync = self.syncing.as_ref()?;
        let user = id(&event["user"]["id"])?;
        if user != sync.user_id {
            return None;
        }
        event["activities"]
            .as_array()?
            .iter()
            .find(|a| {
                a["party"]["id"]
                    .as_str()
                    .is_some_and(|p| p.starts_with("spotify:"))
            })
            .map(|a| (user, a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_snapshots_keep_track_episode_and_empty_player_shapes() {
        let raw = json!({"is_playing":true,"repeat_state":"off","progress_ms":2500,
            "private":"discard", "device":{"id":"pc","is_active":false,"extra":"discard"},
            "item":{"type":"track","id":"regional","linked_from":{"id":"original"},"name":"song","duration_ms":12345,
                "album":{"id":"album","name":"album name","images":[{"url":"https://i.scdn.co/image/test"}]},
                "artists":[{"id":"artist","name":"artist name","extra":"discard"},{"name":"invalid"}]}});
        let event = player_event("a", &raw);
        assert_eq!(event["track"]["id"], "original");
        assert_eq!(event["track"]["artists"].as_array().unwrap().len(), 1);
        assert_eq!(event["device"]["is_active"], true);
        assert_eq!(event["repeat"], false);
        assert!(!event.to_string().contains("discard"));
        let episode = player_event(
            "a",
            &json!({"item":{"type":"episode","id":"episode","linked_from":{"id":"wrong"},
            "show":{"id":"show","name":"series","images":[]}}}),
        );
        assert_eq!(episode["track"]["id"], "episode");
        assert_eq!(episode["track"]["album"]["type"], "show");
        assert_eq!(episode["track"]["artists"], json!([]));
        let empty = player_event("a", &Value::Null);
        assert_eq!(empty["repeat"], false);
        assert_eq!(empty["isPlaying"], false);
        assert!(empty["track"].is_null());
    }

    fn linked() -> SpotifyStore {
        let mut store = SpotifyStore::default();
        store.dispatch("READY", &json!({"connected_accounts":[{"id":"a","type":"spotify","show_activity":true},{"id":"other","type":"twitch"}]}), 0);
        store.dispatch("SPOTIFY_ACCOUNT_ACCESS_TOKEN", &json!({"accountId":"a"}), 0);
        store
    }

    fn playable() -> SpotifyStore {
        let mut store = linked();
        store.dispatch(
            "SPOTIFY_SET_DEVICES",
            &json!({"accountId":"a","devices":[{"id":"pc","type":"Computer","is_active":true}]}),
            0,
        );
        store
    }

    fn activity() -> Value {
        json!({"type":2,"name":"Spotify","sync_id":"track","party":{"id":"spotify:2"},"timestamps":{"start":10000}})
    }

    fn player(store: &mut SpotifyStore, track: &str, position: i64, now: i64) {
        store.dispatch("SPOTIFY_PLAYER_STATE", &json!({"accountId":"a","isPlaying":true,"track":{"id":track},"position":position,"repeat":false}), now);
    }

    #[test]
    fn controls_follow_connection_device_protocol_track_and_party() {
        let a = activity();
        let empty = SpotifyStore::default().controls(1, 2, &a);
        assert!(!empty.has_account && !empty.play_disabled() && !empty.sync_disabled());
        let mut store = linked();
        assert!(store.controls(1, 2, &a).not_playable);
        store.dispatch(
            "SPOTIFY_SET_PROTOCOL_REGISTERED",
            &json!({"isRegistered":true}),
            0,
        );
        assert!(!store.controls(1, 2, &a).play_disabled());
        assert!(store.controls(1, 1, &a).play_disabled());
        let mut store = playable();
        player(&mut store, "track", 0, 10000);
        assert!(store.controls(1, 2, &a).play_disabled());
        assert!(!store.controls(1, 2, &a).sync_disabled());
        store.start_sync(2, &a, 10000).unwrap();
        assert!(store.controls(1, 2, &a).same_user);
        assert!(store.controls(1, 3, &a).same_party);
        assert!(store.controls(1, 3, &a).sync_disabled());
    }

    #[test]
    fn connection_refresh_prunes_devices_players_and_revoked_socket() {
        let mut store = playable();
        assert_eq!(store.accounts().len(), 1);
        player(&mut store, "track", 0, 10000);
        store.dispatch("USER_CONNECTIONS_UPDATE", &json!({}), 0);
        assert!(store.has_connected_account());
        store.dispatch(
            "USER_CONNECTION_UPDATE",
            &json!({"platformType":"spotify","id":"a","showActivity":false}),
            0,
        );
        assert!(!store.accounts()[0].show_activity);
        store.dispatch(
            "SPOTIFY_ACCOUNT_ACCESS_TOKEN_REVOKE",
            &json!({"accountId":"a"}),
            0,
        );
        assert!(!store.has_connected_account());
        assert!(store.player().is_none());
        store.dispatch("SPOTIFY_ACCOUNT_ACCESS_TOKEN", &json!({"accountId":"a"}), 0);
        store.dispatch(
            "USER_CONNECTIONS_UPDATE",
            &json!({"local":true,"accounts":[]}),
            0,
        );
        assert!(store.accounts().is_empty() && !store.has_connected_account());
        assert!(store.active_device().is_none());
    }

    #[test]
    fn active_devices_and_computer_candidates_are_distinct() {
        let mut store = linked();
        store.dispatch(
            "SPOTIFY_SET_DEVICES",
            &json!({"accountId":"a","devices":[
            {"id":"restricted","type":"Computer","is_restricted":true},
            {"id":"phone","type":"Smartphone","is_active":true},
            {"id":"pc","type":"Computer"}]}),
            0,
        );
        assert_eq!(store.active_device().unwrap().1.id, "phone");
        assert_eq!(store.playable_computers()[0].1.id, "pc");
        store.dispatch(
            "SPOTIFY_PROFILE_UPDATE",
            &json!({"accountId":"a","isPremium":true}),
            0,
        );
        assert_eq!(store.active_premium(), Some(true));
        store.set_socket_connected("a", false);
        assert!(store.active_device().is_none() && store.playable_computers().is_empty());
        assert!(store.has_connected_account());
    }

    #[test]
    fn playback_bodies_use_context_offset_or_resource_uri() {
        let store = playable();
        let mut a = activity();
        a["sync_id"] = json!("episode /é");
        let play = store.plan_play(&a, &json!({"type":"episode"})).unwrap();
        assert_eq!(
            play.body(),
            json!({"uris":["spotify:episode:episode%20%2F%C3%A9"],"position_ms":0})
        );
        let context = store
            .plan_play(&a, &json!({"context_uri":"spotify:playlist:list"}))
            .unwrap();
        assert_eq!(
            context.body(),
            json!({"context_uri":"spotify:playlist:list","offset":{"uri":"spotify:track:episode%20%2F%C3%A9"},"position_ms":0})
        );
        assert!(store.plan_play(&a, &Value::Null).is_none());
        assert!(store.plan_play(&a, &json!({"type":"invalid"})).is_none());
        assert!(store.plan_play(&a, &json!({"type":42})).is_none());
        a["party"]["id"] = json!("other");
        assert!(store.plan_play(&a, &json!({})).is_none());
    }

    #[test]
    fn listen_along_seeks_and_does_not_pause_an_unrelated_track() {
        let mut store = playable();
        let a = activity();
        let play = store.start_sync(2, &a, 14000).unwrap();
        assert_eq!(play.position_ms, 4000);
        assert_eq!(play.repeat, Some(false));
        let mut string_clock = a.clone();
        string_clock["timestamps"]["start"] = json!("10000");
        assert_eq!(
            store
                .start_sync(2, &string_clock, 14000)
                .unwrap()
                .position_ms,
            4000
        );
        player(&mut store, "track", 4000, 14000);
        assert_eq!(store.start_sync(2, &a, 9000).unwrap().position_ms, 0);
        assert_eq!(store.start_sync(2, &a, 14000).unwrap().repeat, None);
        assert_eq!(store.stop_sync().as_deref(), Some("a"));
        store.start_sync(2, &a, 14000).unwrap();
        player(&mut store, "different", 0, 16000);
        assert_eq!(store.stop_sync(), None);
        assert!(store.syncing_with().is_none());
    }

    #[test]
    fn the_host_presence_moves_the_listen_along() {
        let mut store = playable();
        store.start_sync(2, &activity(), 14000).unwrap();
        let presence = |activities: Value| json!({"user": {"id": "2"}, "activities": activities});
        assert_eq!(
            store.presence_step(&presence(json!([activity()])), 15000),
            SyncStep::HostBack
        );
        let mut next = activity();
        next["sync_id"] = json!("next");
        next["timestamps"]["start"] = json!(20000);
        match store.presence_step(&presence(json!([next])), 21000) {
            SyncStep::Replay(play) => {
                assert_eq!(play.track_id, "next");
                assert_eq!(play.position_ms, 1000);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            store.presence_step(&presence(json!([])), 22000),
            SyncStep::HostGone
        );
        let other = json!({"user": {"id": "3"}, "activities": []});
        assert_eq!(store.presence_step(&other, 22000), SyncStep::Nothing);
    }

    #[test]
    fn player_clock_absorbs_small_jitter_and_retains_device_handoff() {
        let mut store = playable();
        player(&mut store, "track", 4000, 14000);
        player(&mut store, "track", 4500, 15000);
        assert_eq!(store.player().unwrap().start_time, 10000);
        player(&mut store, "track", 1000, 15000);
        assert_eq!(store.player().unwrap().start_time, 14000);
        store.start_sync(2, &activity(), 15000).unwrap();
        store.dispatch("SPOTIFY_PLAYER_STATE", &json!({"accountId":"a","device":{"id":"new","type":"Computer"},"isPlaying":false,"position":0}), 15000);
        assert_eq!(store.player().unwrap().track_id, "track");
        assert_eq!(store.active_device().unwrap().1.id, "new");
        store.dispatch(
            "SPOTIFY_PLAYER_STATE",
            &json!({"accountId":"a","isPlaying":false,"position":500}),
            15000,
        );
        assert!(store.player().is_none());
    }

    #[test]
    fn paused_accounts_keep_their_player_priority_when_they_resume() {
        let mut store = playable();
        store.dispatch(
            "USER_CONNECTIONS_UPDATE",
            &json!({"local":true,"accounts":[
            {"id":"a","type":"spotify"},{"id":"b","type":"spotify"}]}),
            0,
        );
        player(&mut store, "first", 0, 10000);
        store.dispatch(
            "SPOTIFY_PLAYER_STATE",
            &json!({"accountId":"b","isPlaying":true,"track":{"id":"second"},"position":0}),
            10000,
        );
        store.dispatch(
            "SPOTIFY_PLAYER_STATE",
            &json!({"accountId":"a","isPlaying":false}),
            10000,
        );
        assert_eq!(store.player().unwrap().account_id, "b");
        player(&mut store, "resumed", 0, 11000);
        assert_eq!(store.player().unwrap().account_id, "a");
    }

    #[test]
    fn stores_ready_wires_only_spotify_account_metadata() {
        let mut stores = crate::Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user":{"id":"1","username":"fixture"},
            "connected_accounts":[{"id":"a","type":"spotify","show_activity":true}]}),
        );
        assert_eq!(
            stores.spotify.accounts(),
            &[Account {
                id: "a".into(),
                show_activity: true
            }]
        );
        assert!(!stores.spotify.has_connected_account());
        stores.dispatch("SPOTIFY_ACCOUNT_ACCESS_TOKEN", &json!({"accountId":"a"}));
        assert!(stores.spotify.has_connected_account());
        let mut store = playable();
        store.start_sync(2, &activity(), 10000).unwrap();
        assert!(store.controls(1, 2, &activity()).same_track);
        let event = json!({"user":{"id":"2"},"activities":[activity()]});
        assert_eq!(store.syncing_user_activity(&event).unwrap().0, 2);
        assert!(
            store
                .syncing_user_activity(&json!({"user":{"id":"3"},"activities":[activity()]}))
                .is_none()
        );
    }
    /// A `/me/player` (or dealer `PLAYER_STATE_CHANGED` `event.state`) snapshot of a playing track.
    fn snapshot(name: &str, progress: i64, playing: bool) -> Value {
        json!({"is_playing": playing, "repeat_state": "off", "progress_ms": progress,
            "device": {"id": "pc", "name": "PC", "type": "Computer", "is_active": true, "volume_percent": 50},
            "context": {"type": "album", "uri": "spotify:album:alb", "href": "https://api.spotify.com/v1/albums/alb"},
            "item": {"type": "track", "id": "trk", "name": name, "duration_ms": 200_000, "is_local": false,
                "album": {"id": "alb", "name": "The Album", "type": "album",
                    "images": [{"url": "https://i.scdn.co/image/ab67616d0000b273abc", "width": 640, "height": 640}]},
                "artists": [{"id": "r1", "name": "One; Two"}, {"id": "r2", "name": "Three"}]}})
    }

    fn state(store: &mut SpotifyStore, snapshot: &Value, now: i64) {
        let mut event = player_event("a", snapshot);
        event["context"] = public_context(match context_source(snapshot, None) {
            ContextSource::Ready(context) => context,
            _ => Value::Null,
        });
        store.dispatch("SPOTIFY_PLAYER_STATE", &event, now);
    }

    #[test]
    fn a_playing_track_is_the_listening_activity_655116_builds() {
        let mut store = playable();
        assert_eq!(store.activity(7), None);
        state(&mut store, &snapshot("Song", 30_000, true), 100_000);
        assert!(store.should_show_activity(false));
        assert!(!store.should_show_activity(true));
        assert_eq!(
            store.activity(7).unwrap(),
            json!({"type": 2, "name": "Spotify",
                "assets": {"large_image": "spotify:ab67616d0000b273abc", "large_text": "The Album"},
                "details": "Song", "state": "One Two; Three",
                "timestamps": {"start": 70_000, "end": 270_000},
                "party": {"id": "spotify:7"}, "sync_id": "trk", "flags": 48,
                "metadata": {"context_uri": "spotify:album:alb", "album_id": "alb", "artist_ids": ["r1", "r2"],
                    "type": "track", "button_urls": []}})
        );
        // The player clock's jitter keeps the start: the same activity, no new presence update.
        state(&mut store, &snapshot("Song", 31_400, true), 101_000);
        assert_eq!(store.activity(7).unwrap()["timestamps"]["start"], 70_000);
        // A seek moves it.
        state(&mut store, &snapshot("Song", 90_000, true), 102_000);
        assert_eq!(store.activity(7).unwrap()["timestamps"]["start"], 12_000);
        // Pausing clears the activity at once; there is no pause grace.
        state(&mut store, &snapshot("Song", 90_000, false), 103_000);
        assert_eq!(store.activity(7), None);
        assert!(!store.should_show_activity(false));
        assert_eq!(store.track_end_ms(), None);
    }

    #[test]
    fn display_spotify_as_your_status_off_hides_the_activity() {
        let mut store = playable();
        state(&mut store, &snapshot("Song", 0, true), 1_000);
        store.dispatch(
            "USER_CONNECTION_UPDATE",
            &json!({"platformType": "spotify", "id": "a", "showActivity": false}),
            2_000,
        );
        assert!(!store.should_show_activity(false));
        store.dispatch(
            "USER_CONNECTION_UPDATE",
            &json!({"platformType": "spotify", "id": "a", "showActivity": true}),
            3_000,
        );
        assert!(store.should_show_activity(false));
        // Unlinking the account takes the player with it.
        store.dispatch(
            "USER_CONNECTIONS_UPDATE",
            &json!({"local": true, "accounts": []}),
            4_000,
        );
        assert!(!store.should_show_activity(false));
        assert_eq!(store.activity(7), None);
    }

    #[test]
    fn local_files_singles_episodes_and_long_names_follow_get_activity() {
        let mut store = playable();
        let mut local = snapshot("Song", 0, true);
        local["item"]["is_local"] = json!(true);
        local["item"]["album"]["type"] = json!("single");
        local["item"]["album"]["images"] = json!([]);
        local["item"]["artists"] = json!([]);
        local["context"] = Value::Null;
        state(&mut store, &local, 1_000);
        assert_eq!(
            store.activity(7).unwrap(),
            json!({"type": 2, "name": "Spotify", "assets": {}, "details": "Song",
                "timestamps": {"start": 1_000, "end": 201_000}, "party": {"id": "spotify:7"}})
        );
        let mut long = snapshot(&"é".repeat(130), 0, true);
        long["item"]["artists"] = json!(
            (0..7)
                .map(|i| json!({"id": format!("r{i}"), "name": format!("A{i}")}))
                .collect::<Vec<_>>()
        );
        long["item"]["album"]["images"] = json!([{"url": "https://example.com/elsewhere.jpg"}]);
        state(&mut store, &long, 2_000);
        let activity = store.activity(7).unwrap();
        assert_eq!(activity["details"], format!("{}...", "é".repeat(125)));
        assert_eq!(activity["state"], "A0; A1; A2; A3; A4");
        assert_eq!(
            activity["metadata"]["artist_ids"],
            json!(["r0", "r1", "r2", "r3", "r4"])
        );
        assert_eq!(activity["assets"], json!({"large_text": "The Album"}));
        let episode = json!({"is_playing": true, "progress_ms": 0, "device": {"id": "pc"},
            "item": {"type": "episode", "id": "ep", "name": "Episode", "duration_ms": 1000,
                "show": {"id": "show", "name": "Series", "images": []}}});
        state(&mut store, &episode, 3_000);
        let activity = store.activity(7).unwrap();
        assert_eq!(activity["metadata"]["type"], "episode");
        assert_eq!(activity["metadata"]["album_id"], "show");
        assert_eq!(activity["assets"]["large_text"], "Series");
        assert!(activity.get("state").is_none());
    }

    #[test]
    fn the_track_end_timer_clears_a_player_nobody_updated() {
        let mut store = playable();
        state(&mut store, &snapshot("Song", 150_000, true), 1_000_000);
        // `W`: duration - position + five seconds.
        assert_eq!(store.track_end_ms(), Some(1_055_000));
        assert!(!store.expire_track(1_054_999));
        assert!(store.player().is_some());
        assert!(store.expire_track(1_055_000));
        assert_eq!(store.activity(7), None);
        assert_eq!(store.track_end_ms(), None);
    }

    #[test]
    fn listening_along_carries_the_host_party_and_stops_when_own_playback_does() {
        let mut store = playable();
        let host = json!({"type": 2, "name": "Spotify", "sync_id": "trk", "party": {"id": "spotify:2"},
            "timestamps": {"start": 10_000}});
        store.start_sync(2, &host, 20_000).unwrap();
        state(&mut store, &snapshot("Song", 10_000, true), 20_000);
        assert_eq!(store.activity(7).unwrap()["party"]["id"], "spotify:2");
        assert_eq!(store.listen_along_stop_ms(), None);
        // A zero-position stopped snapshot of a device handoff changes nothing.
        state(&mut store, &snapshot("Song", 0, false), 21_000);
        assert!(store.player().is_some());
        // Own playback paused mid-track: the listen-along ends in five seconds.
        state(&mut store, &snapshot("Song", 12_000, false), 22_000);
        assert_eq!(store.listen_along_stop_ms(), Some(27_000));
        // Playing the shared track again first cancels it.
        state(&mut store, &snapshot("Song", 12_000, true), 23_000);
        assert_eq!(store.listen_along_stop_ms(), None);
        let mut other = snapshot("Other", 0, true);
        other["item"]["id"] = json!("other");
        state(&mut store, &other, 24_000);
        assert_eq!(store.expire_listen_along(28_999), None);
        assert_eq!(store.expire_listen_along(29_000), Some(None));
        assert!(store.syncing_with().is_none());
        assert_eq!(store.activity(7), None);
    }

    #[test]
    fn contexts_are_albums_known_uris_or_fetched_public_playlists() {
        let album = snapshot("Song", 0, true);
        assert_eq!(
            context_source(&album, None),
            ContextSource::Ready(album["context"].clone())
        );
        let mut playlist = album.clone();
        playlist["context"] = json!({"type": "playlist", "uri": "spotify:playlist:37i9dQ",
            "href": "https://api.spotify.com/v1/playlists/37i9dQ"});
        assert_eq!(
            context_source(&playlist, None),
            ContextSource::Playlist("37i9dQ".into())
        );
        let held = json!({"type": "playlist", "uri": "spotify:playlist:37i9dQ", "public": true});
        assert_eq!(
            context_source(&playlist, Some(&held)),
            ContextSource::Ready(held.clone())
        );
        playlist["context"]["href"] = json!("https://evil.example/v1/playlists/x");
        playlist["context"]["uri"] = json!("spotify:playlist:../../me");
        assert_eq!(context_source(&playlist, None), ContextSource::None);
        let mut artist = album.clone();
        artist["context"] = json!({"type": "artist", "uri": "spotify:artist:r1"});
        assert_eq!(context_source(&artist, None), ContextSource::None);
        assert_eq!(public_context(held.clone()), held);
        assert_eq!(
            public_context(json!({"type": "playlist", "uri": "u", "public": false})),
            Value::Null
        );
        assert_eq!(public_context(album["context"].clone()), album["context"]);
    }
}
