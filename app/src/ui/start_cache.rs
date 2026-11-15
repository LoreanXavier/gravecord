//! The start cache: what the first screen needs, kept from the last run, so the app draws it before the gateway's
//! READY arrives and shows the same screen again, without a jump, when it does.
//!
//! The official client does the same with its lazy cache (`READY_SUPPLEMENTAL` and the stores in `STORES.md`): the
//! guild list with icons, the channel list of the server in view, the last channel's recent messages and the user.
//! Two files in the app's data folder: `start-ready.json.zst` (READY, compressed & cut down) and
//! `start-messages.json` (one page of the last channel). Never a token: READY is cut to a list of the keys the
//! stores read, and the session and the account's private fields are not among them. Both files are versioned and
//! bounded; one that is unreadable, from another version, or too large is ignored, and the app starts as it does
//! without a cache.
//!
//! A harness or inert run neither reads nor writes them unless it sets `GRAVECORD_CACHE=1`, so every capture starts
//! the same; normal desktop runs use the cache by default unless `GRAVECORD_CACHE=0`.

use super::Root;
use gpui::Context;
use serde_json::{Map, Value, json};
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Bumped when the stores read other fields of READY, or the files change shape.
const VERSION: u64 = 2;
/// The most an uncompressed READY cache payload may hold, in bytes.
const MAX_READY_BYTES: usize = 8 * 1024 * 1024;
const MAX_MESSAGES_BYTES: usize = 512 * 1024;
/// Messages kept of the last channel.
const MAX_MESSAGES: usize = 50;

const READY_FILE: &str = "start-ready.json.zst";
const LEGACY_READY_FILE: &str = "start-ready.json";
const MESSAGES_FILE: &str = "start-messages.json";
const WISHLISTS_FILE: &str = "start-wishlists.json.zst";
const MAX_WISHLISTS_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_WISHLIST_USERS: usize = 64;

/// The keys of READY the stores read to draw the first screen. Everything else is left out, which keeps the session
/// (`session_id`, `sessions`, `analytics_token`, `auth_session_id_hash`) and the geo list out of the file.
const READY_KEYS: [&str; 13] = [
    "user",
    "users",
    "relationships",
    "private_channels",
    "guilds",
    "merged_members",
    "notification_settings",
    "user_settings_proto",
    "user_guild_settings",
    "read_state",
    "consents",
    "apex_experiments",
    "guild_folders",
];

/// The fields of the account's own user that are not private: no email, phone, or two-factor state.
const USER_KEYS: [&str; 15] = [
    "id",
    "username",
    "global_name",
    "avatar",
    "discriminator",
    "banner",
    "accent_color",
    "avatar_decoration_data",
    "collectibles",
    "primary_guild",
    "premium_type",
    "flags",
    "public_flags",
    "bot",
    "display_name_styles",
];

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct CachedWishlists {
    pub(crate) wishlist_ids: std::collections::HashMap<u64, String>,
    pub(crate) wishlists: std::collections::HashMap<u64, Value>,
    pub(crate) products: Vec<super::shop::data::Product>,
}

/// What a run starts from.
pub(crate) struct Cached {
    pub(crate) ready: Value,
    pub(crate) messages: Option<(u64, Value)>,
    pub(crate) wishlists: Option<CachedWishlists>,
}

/// Whether this run uses the cache: enabled by default on normal desktop launches unless
/// `GRAVECORD_CACHE=0`, and disabled in tests, capture runs, or inert runs unless `GRAVECORD_CACHE=1`.
pub(crate) fn enabled() -> bool {
    match std::env::var("GRAVECORD_CACHE").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => !(cfg!(test) || crate::storage::is_capture_run() || crate::inert::on()),
    }
}

fn file(name: &str) -> Option<PathBuf> {
    Some(crate::storage::data_dir()?.join(name))
}

/// READY cut down to what the first screen reads. `last_guild` is the server the app opens on: it keeps its
/// channels, roles and emoji; the others keep what the rail and the unread dots need.
pub(crate) fn trim_ready(ready: &Value, last_guild: Option<&str>) -> Option<Value> {
    let source = ready.as_object()?;
    let mut out = Map::new();
    for key in READY_KEYS {
        if let Some(value) = source.get(key) {
            out.insert(key.to_owned(), value.clone());
        }
    }
    if let Some(user) = source.get("user").and_then(Value::as_object) {
        let kept: Map<String, Value> = USER_KEYS
            .iter()
            .filter_map(|key| Some((key.to_string(), user.get(*key)?.clone())))
            .collect();
        out.insert("user".to_owned(), Value::Object(kept));
    }
    let guilds: Vec<Value> = source
        .get("guilds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|guild| trim_guild(guild, last_guild))
        .collect();
    out.insert("guilds".to_owned(), Value::Array(guilds));
    let value = Value::Object(out);
    (serde_json::to_vec(&value).ok()?.len() <= MAX_READY_BYTES).then_some(value)
}

/// One guild of READY without what is large and not needed to draw it: its members and presences, its threads,
/// its voice states, and (for a server that is not open) its emoji and stickers.
fn trim_guild(guild: &Value, last_guild: Option<&str>) -> Value {
    let Some(object) = guild.as_object() else {
        return guild.clone();
    };
    let open = last_guild.is_some_and(|id| object.get("id").and_then(Value::as_str) == Some(id));
    let always_dropped = [
        "members",
        "presences",
        "voice_states",
        "stage_instances",
        "guild_scheduled_events",
        "activity_instances",
        "threads",
    ];
    let dropped_when_closed = ["emojis", "stickers"];
    let kept: Map<String, Value> = object
        .iter()
        .filter(|(key, _)| !always_dropped.contains(&key.as_str()))
        .filter(|(key, _)| open || !dropped_when_closed.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Value::Object(kept)
}

/// Writes `bytes` to a sibling temp file and renames it over `path`, so a crash mid-write never leaves a half file.
fn write_atomic_bytes(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let temporary = path.with_extension("tmp");
    if std::fs::write(&temporary, bytes).is_ok() {
        let _ = std::fs::rename(&temporary, path);
    }
}

/// Writes `value` to `name` beside and renames it over the file, so a crash mid-write never leaves half a file.
fn write_atomic(name: &str, value: &Value) {
    let Some(path) = file(name) else {
        return;
    };
    let Ok(bytes) = serde_json::to_vec(value) else {
        return;
    };
    write_atomic_bytes(&path, &bytes);
}

fn write_zstd_json(path: &Path, value: &Value) {
    let Ok(json) = serde_json::to_vec(value) else {
        return;
    };
    let Ok(compressed) = zstd::encode_all(json.as_slice(), 1) else {
        return;
    };
    write_atomic_bytes(path, &compressed);
}

fn load_zstd_json(path: &Path, limit: usize) -> Option<Value> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = zstd::Decoder::new(file).ok()?;
    let mut buf = Vec::new();
    decoder
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut buf)
        .ok()?;
    if buf.len() > limit {
        return None;
    }
    let value: Value = serde_json::from_slice(&buf).ok()?;
    (value["version"].as_u64() == Some(VERSION)).then_some(value)
}

fn save_trimmed_ready_to(path: &Path, trimmed: &Value) {
    if serde_json::to_vec(trimmed).is_ok_and(|bytes| bytes.len() <= MAX_READY_BYTES) {
        write_zstd_json(path, &json!({"version": VERSION, "ready": trimmed}));
    }
}

#[cfg(test)]
fn save_ready_to(path: &Path, ready: &Value, last_guild: Option<&str>) {
    let Some(trimmed) = trim_ready(ready, last_guild) else {
        return;
    };
    save_trimmed_ready_to(path, &trimmed);
}

/// Syncs `private_channels` (from `READY_SUPPLEMENTAL.lazy_private_channels` and live DM events) and their
/// recipient/relationship `User` records into a trimmed READY snapshot so instant cold starts show the DM list immediately.
pub(crate) fn sync_private_channels(ready: &mut Value, stores: &dc_state::Stores) {
    let Some(object) = ready.as_object_mut() else {
        return;
    };
    let channels: Vec<Value> = stores
        .private_channels
        .iter()
        .map(dc_state::PrivateChannel::to_cache_value)
        .collect();
    object.insert("private_channels".to_owned(), Value::Array(channels));

    if !object.get("users").is_some_and(Value::is_array) {
        object.insert("users".to_owned(), Value::Array(Vec::new()));
    }
    let Some(users) = object.get_mut("users").and_then(Value::as_array_mut) else {
        return;
    };
    let mut index_by_id: std::collections::HashMap<u64, usize> = users
        .iter()
        .enumerate()
        .filter_map(|(idx, user)| {
            let id = user["id"].as_str()?.parse::<u64>().ok()?;
            Some((id, idx))
        })
        .collect();
    let wanted_ids = stores
        .private_channels
        .iter()
        .flat_map(|dm| dm.recipients.iter().copied())
        .chain(stores.relationships.keys().copied());
    for uid in wanted_ids {
        let Some(user) = stores.users.get(&uid) else {
            continue;
        };
        let encoded = user.to_cache_value();
        if let Some(&idx) = index_by_id.get(&uid) {
            users[idx] = encoded;
        } else {
            index_by_id.insert(uid, users.len());
            users.push(encoded);
        }
    }
}

/// Writes an already-trimmed READY snapshot in a background thread.
pub(crate) fn save_trimmed_ready(trimmed: Value) {
    if !enabled() {
        return;
    }
    std::thread::spawn(move || {
        if let Some(path) = file(READY_FILE) {
            save_trimmed_ready_to(&path, &trimmed);
        }
        if let Some(legacy) = file(LEGACY_READY_FILE) {
            let _ = std::fs::remove_file(legacy);
        }
    });
}

fn save_wishlists_to(path: &Path, data: &CachedWishlists) {
    let Ok(payload) = serde_json::to_value(data) else {
        return;
    };
    let wrapped = json!({"version": VERSION, "data": payload});
    if serde_json::to_vec(&wrapped).is_ok_and(|bytes| bytes.len() <= MAX_WISHLISTS_BYTES) {
        write_zstd_json(path, &wrapped);
    }
}

fn load_wishlists_from(path: &Path, limit: usize) -> Option<CachedWishlists> {
    let wrapped = load_zstd_json(path, limit)?;
    serde_json::from_value(wrapped["data"].clone()).ok()
}

pub(crate) fn save_wishlists(mut data: CachedWishlists) {
    if !enabled() {
        return;
    }
    if data.wishlists.len() > MAX_WISHLIST_USERS {
        let keys: Vec<u64> = data
            .wishlists
            .keys()
            .copied()
            .take(data.wishlists.len() - MAX_WISHLIST_USERS)
            .collect();
        for key in keys {
            data.wishlists.remove(&key);
        }
    }
    std::thread::spawn(move || {
        if let Some(path) = file(WISHLISTS_FILE) {
            save_wishlists_to(&path, &data);
        }
    });
}

/// Keeps the newest messages of the channel in view for the next start.
pub(crate) fn save_messages(channel: u64, page: &Value) {
    if !enabled() {
        return;
    }
    let Some(list) = page.as_array() else {
        return;
    };
    // The server answers newest first; the newest are the ones worth keeping.
    let kept: Vec<Value> = list.iter().take(MAX_MESSAGES).cloned().collect();
    let value = json!({"version": VERSION, "channel": channel.to_string(), "page": kept});
    if serde_json::to_vec(&value).is_ok_and(|bytes| bytes.len() <= MAX_MESSAGES_BYTES) {
        std::thread::spawn(move || write_atomic(MESSAGES_FILE, &value));
    }
}

fn read(name: &str, limit: usize) -> Option<Value> {
    let path = file(name)?;
    if std::fs::metadata(&path).ok()?.len() as usize > limit + 1024 {
        return None;
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (value["version"].as_u64() == Some(VERSION)).then_some(value)
}

fn load_ready_from(path: &Path, limit: usize) -> Option<Value> {
    load_zstd_json(path, limit)
}

/// What the last run left, if this run uses the cache and the files are good.
pub(crate) fn load() -> Option<Cached> {
    if !enabled() {
        return None;
    }
    let ready = load_ready_from(&file(READY_FILE)?, MAX_READY_BYTES + 1024)?["ready"].clone();
    ready["user"]["id"].as_str()?;
    let messages = read(MESSAGES_FILE, MAX_MESSAGES_BYTES).and_then(|value| {
        let channel = value["channel"].as_str()?.parse().ok()?;
        Some((channel, value["page"].clone()))
    });
    let wishlists = file(WISHLISTS_FILE)
        .and_then(|path| load_wishlists_from(&path, MAX_WISHLISTS_BYTES + 1024));
    Some(Cached {
        ready,
        messages,
        wishlists,
    })
}

/// Logging out removes what the account left, so the next login never starts from it.
pub(crate) fn forget() {
    for name in [READY_FILE, LEGACY_READY_FILE, MESSAGES_FILE, WISHLISTS_FILE] {
        if let Some(path) = file(name) {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Root {
    /// Applies the cache before the gateway answers: the same stores, filled the way READY fills them, and the
    /// server and channel the app opens on. The real READY that follows replaces them as a reconnect's does.
    pub(crate) fn apply_cache(&mut self, cached: Cached, cx: &mut Context<Self>) {
        if self.stores.current_user.is_some() {
            // The gateway was first: nothing to draw from the past.
            return;
        }
        self.cached_ready = Some(cached.ready.clone());
        self.stores.dispatch("READY", &cached.ready);
        self.hydrated_from_cache = true;
        self.reset_theme_sync();
        self.after_ready();
        self.request_group_images();
        if let Some(wishlists) = cached.wishlists {
            for product in wishlists.products {
                self.shop_prefetch_product_art(&product, 78.);
                self.shop
                    .products
                    .entry(product.sku_id.clone())
                    .or_insert(product);
            }
            self.wishlist_ids_cache.extend(wishlists.wishlist_ids);
            let me = self.stores.current_user.as_ref().map(|user| user.id);
            for (user, list) in wishlists.wishlists {
                self.shop_seed_effects(&list);
                let skus: Vec<String> = list["wishlist_items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|item| {
                        item["sku"]["id"]
                            .as_str()
                            .or_else(|| item["sku_id"].as_str())
                            .map(str::to_owned)
                    })
                    .collect();
                self.wishlist_prefetch_skus.extend(skus.iter().cloned());
                if Some(user) == me && !skus.is_empty() {
                    if let Some(wid) = list["id"].as_str() {
                        self.shop.wishlist_id = Some(wid.to_owned());
                    }
                    self.shop.wishlist = skus.iter().cloned().collect();
                    self.shop.wishlist_order = skus;
                }
                self.fetched.insert(format!("profile-wishlist:{user}"), list);
            }
        }
        if let Some((channel, page)) = cached.messages
            && Some(channel) == self.channel
        {
            self.stores.load_messages_since(channel, &page, 0);
            self.rebuild_rows();
            self.request_images();
            self.ready = true;
        }
        cx.notify();
    }

    /// The server the app opens on, as READY's cache needs to know it.
    pub(super) fn cache_guild(&self) -> Option<String> {
        self.guild.map(|guild| guild.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Value {
        json!({
            "session_id": "secret-session",
            "sessions": [{"session_id": "x", "client_info": {"os": "windows"}}],
            "analytics_token": "tok",
            "auth_session_id_hash": "hash",
            "geo_ordered_rtc_regions": ["a"],
            "user": {"id": "1", "username": "me", "email": "me@example.com", "phone": "+1", "mfa_enabled": true, "avatar": "h"},
            "users": [{"id": "2", "username": "other"}],
            "user_settings_proto": "CgA=",
            "guilds": [
                {"id": "10", "properties": {"name": "open", "icon": "i"}, "channels": [{"id": "100"}], "roles": [{"id": "1"}],
                 "emojis": [{"id": "5"}], "members": [{"user": {"id": "2"}}], "presences": [{}], "threads": [{}]},
                {"id": "11", "properties": {"name": "closed"}, "channels": [{"id": "110"}], "roles": [{"id": "2"}],
                 "emojis": [{"id": "6"}], "voice_states": [{}]}
            ]
        })
    }

    #[test]
    fn no_session_or_private_field_reaches_the_file() {
        let text = serde_json::to_string(&trim_ready(&ready(), Some("10")).unwrap()).unwrap();
        for secret in [
            "secret-session",
            "analytics_token",
            "auth_session_id_hash",
            "me@example.com",
            "+1",
            "mfa_enabled",
            "client_info",
            "geo_ordered",
        ] {
            assert!(!text.contains(secret), "{secret} was kept");
        }
        assert!(text.contains("\"username\":\"me\""));
    }

    #[test]
    fn the_open_server_keeps_all_fields_and_closed_servers_keep_roles() {
        let trimmed = trim_ready(&ready(), Some("10")).unwrap();
        let guilds = trimmed["guilds"].as_array().unwrap();
        assert!(guilds[0]["roles"].is_array() && guilds[0]["emojis"].is_array());
        assert!(guilds[0].get("members").is_none() && guilds[0].get("threads").is_none());
        assert!(guilds[1]["roles"].is_array() && guilds[1].get("emojis").is_none());
        assert!(guilds[1]["channels"].is_array());
        assert_eq!(guilds[1]["properties"]["name"], "closed");
    }

    #[test]
    fn zstd_ready_roundtrips_and_honors_decompression_cap() {
        let dir = std::env::temp_dir().join(format!(
            "gravecord-start-cache-test-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(READY_FILE);
        save_ready_to(&path, &ready(), Some("10"));
        let loaded = load_ready_from(&path, MAX_READY_BYTES + 1024).expect("loads valid cache");
        assert_eq!(loaded["version"].as_u64(), Some(VERSION));
        assert_eq!(loaded["ready"]["user"]["username"], "me");
        assert!(
            load_ready_from(&path, 32).is_none(),
            "capped reader must reject payloads exceeding decompression limit"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_ready_too_large_to_keep_is_left_out() {
        let mut big = ready();
        big["users"] = Value::Array(
            (0..200_000)
                .map(|n| json!({"id": n.to_string(), "username": "padding-padding-padding"}))
                .collect(),
        );
        assert!(trim_ready(&big, None).is_none());
    }

    #[test]
    fn no_run_uses_the_cache_unless_it_asks() {
        if std::env::var("GRAVECORD_CACHE").is_err() {
            assert!(!enabled());
        }
    }

    #[test]
    fn sync_private_channels_and_wishlists_roundtrip() {
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user": {"id": "1", "username": "me"}, "private_channels": []}),
        );
        stores.dispatch(
            "READY_SUPPLEMENTAL",
            &json!({
                "lazy_private_channels": [
                    {
                        "id": "555",
                        "type": 1,
                        "last_message_id": "888",
                        "recipients": [
                            {"id": "2", "username": "bob", "global_name": "Bob", "avatar": "av2"}
                        ]
                    }
                ]
            }),
        );
        let mut trimmed = trim_ready(&ready(), Some("10")).unwrap();
        sync_private_channels(&mut trimmed, &stores);
        let mut fresh = dc_state::Stores::default();
        fresh.dispatch("READY", &trimmed);
        assert_eq!(fresh.private_channels.len(), 1);
        assert_eq!(fresh.private_channels[0].id, 555);
        assert_eq!(fresh.users[&2].avatar.as_deref(), Some("av2"));

        let dir = std::env::temp_dir().join(format!(
            "gravecord-wishlist-cache-test-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(WISHLISTS_FILE);
        let mut wl = CachedWishlists::default();
        wl.wishlist_ids.insert(2, "wl_2".to_owned());
        wl.wishlists.insert(
            2,
            json!({"id": "wl_2", "wishlist_items": [{"sku_id": "77"}]}),
        );
        wl.products.push(super::super::shop::data::Product {
            sku_id: "77".to_owned(),
            name: "Cool Item".to_owned(),
            kind: super::super::shop::data::KIND_AVATAR_DECORATION,
            ..Default::default()
        });
        save_wishlists_to(&path, &wl);
        let loaded =
            load_wishlists_from(&path, MAX_WISHLISTS_BYTES + 1024).expect("wishlists roundtrip");
        assert_eq!(loaded.wishlist_ids.get(&2).map(String::as_str), Some("wl_2"));
        assert_eq!(loaded.products.len(), 1);
        assert_eq!(loaded.products[0].sku_id, "77");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
