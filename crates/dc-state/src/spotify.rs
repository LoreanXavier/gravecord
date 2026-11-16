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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub account_id: String,
    pub track_id: String,
    pub start_time: i64,
    pub repeat: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncingWith {
    pub user_id: Id,
    pub party_id: String,
    pub track_id: String,
    pub start_time: i64,
}

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
        // A device's zero-position stopped snapshot during handoff does not erase listen-along state.
        if !data["device"].is_null() && self.syncing.is_some() && position == 0 && !playing {
            return;
        }
        let Some(track_id) = data["track"]["id"].as_str().filter(|_| playing) else {
            if let Some(index) = previous {
                self.players.remove(index);
            }
            return;
        };
        let proposed = now_ms.saturating_sub(position);
        let start_time = previous
            .map(|index| self.players[index].start_time)
            .filter(|old| proposed.abs_diff(*old) <= 1_500)
            .unwrap_or(proposed);
        let player = Player {
            account_id: account.into(),
            track_id: track_id.into(),
            start_time,
            repeat: data["repeat"].as_bool().unwrap_or(false),
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
}
