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
/// The open server's member list as the window last showed it (its top rows), apart from the page so a long list never
/// pushes the page over its bound.
const MEMBERS_FILE: &str = "start-members.json";
const MAX_MEMBER_LIST_BYTES: usize = 256 * 1024;
/// The most rows of the member list kept: more than a window shows.
pub(crate) const MAX_MEMBER_ROWS: usize = 60;
/// The most bytes read for the member list's avatars at a start, on top of the page's own budget.
const MAX_MEMBER_PICTURE_BYTES: usize = 2 * 1024 * 1024;
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
    /// The pictures of the first screen (image key and the file's bytes) that the picture cache holds, read with the
    /// rest so they are in the very first frame instead of arriving one by one after it.
    pub(crate) pictures: Vec<(String, Vec<u8>)>,
    /// The kept page's authors as server members (`KeptPage::members`).
    pub(crate) members: Vec<Value>,
    /// The open server's member list (its server and a `GUILD_MEMBER_LIST_UPDATE` with one SYNC of the top rows).
    pub(crate) member_list: Option<(u64, Value)>,
}

/// Pictures of the first screen kept by name with the messages, and the most bytes read for them at a start.
const MAX_PICTURES: usize = 64;
const MAX_PICTURE_BYTES: usize = 4 * 1024 * 1024;

/// The most bytes one picture kept only for the start may have on disk.
const MAX_KEPT_PICTURE_BYTES: usize = 1024 * 1024;

/// The folder of the pictures kept only for the start, next to the other start files.
const START_PICTURES_DIR: &str = "start-pictures";

/// Pictures of the first screen that the picture requests do not keep on disk: attachments', embeds' and components'
/// pictures (each named by its message), server emojis and the composer's emoji button (cut from a sheet with the
/// theme's tint). Only the screen in view's are kept, in their own folder by image key (`start-pictures`): each save
/// replaces the set, logging out removes it, and what is scrolled through is never written. A copy of another length
/// than the picture in memory is written again (an edited embed, the button in another tint).
pub(crate) fn kept_for_the_start(key: &str) -> bool {
    key.starts_with("attachment:")
        || key.starts_with("embed:")
        || key.starts_with("comp:")
        || key.starts_with("custom:")
        || key.starts_with("custom-animated:")
        || key == EMOJI_BUTTON
}

const EMOJI_BUTTON: &str = "emoji_button";

/// The file of a picture kept for the start, named by its image key.
fn start_picture_name(key: &str) -> String {
    let name: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{name}.bin")
}

/// Writes pictures kept for the start off the UI thread. With `wanted` (the screen's whole set), files of other
/// pictures are removed, so the folder holds one screen.
fn write_start_pictures(
    pictures: Vec<(String, std::sync::Arc<gpui::Image>)>,
    wanted: Option<Vec<String>>,
) {
    let pictures: Vec<_> = pictures
        .into_iter()
        .filter(|(_, image)| !image.bytes.is_empty() && image.bytes.len() <= MAX_KEPT_PICTURE_BYTES)
        .collect();
    if pictures.is_empty() && wanted.is_none() {
        return;
    }
    let Some(dir) = file(START_PICTURES_DIR) else {
        return;
    };
    std::thread::spawn(move || {
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        for (key, image) in pictures {
            let path = dir.join(start_picture_name(&key));
            // A copy of the same length is the same picture (the key names it); another length is a newer one.
            if std::fs::metadata(&path).is_ok_and(|meta| meta.len() == image.bytes.len() as u64) {
                continue;
            }
            let part = path.with_extension("part");
            if std::fs::write(&part, &image.bytes).is_ok() {
                let _ = std::fs::rename(&part, &path);
            }
        }
        if let Some(wanted) = wanted {
            let keep: std::collections::HashSet<String> =
                wanted.iter().map(|key| start_picture_name(key)).collect();
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".bin") && !keep.contains(&name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    });
}

/// A picture kept for the start, if the folder has it.
fn read_start_picture(key: &str) -> Option<Vec<u8>> {
    let bytes = std::fs::read(file(START_PICTURES_DIR)?.join(start_picture_name(key))).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

impl Cached {
    /// The account the cache was kept for.
    pub(crate) fn account(&self) -> Option<u64> {
        self.ready["user"]["id"].as_str()?.parse().ok()
    }
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
    Some(crate::storage::cache_dir()?.join(name))
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
    ];
    // The open server's active threads are rows of its channel list: without them the list grew by a row under the
    // first frame when READY came.
    let dropped_when_closed = ["emojis", "stickers", "threads"];
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
pub(crate) fn save_messages(
    account: Option<u64>,
    channel: u64,
    page: &Value,
    pictures: &[(String, String)],
    members: Vec<Value>,
) {
    let (true, Some(account)) = (enabled(), account) else {
        return;
    };
    let Some(list) = page.as_array() else {
        return;
    };
    // The server answers newest first; the newest are the ones worth keeping.
    let kept: Vec<Value> = list.iter().take(MAX_MESSAGES).cloned().collect();
    let pictures: Vec<Value> = pictures
        .iter()
        .take(MAX_PICTURES)
        .map(|(key, url)| json!([key, url]))
        .collect();
    let members: Vec<Value> = members.into_iter().take(MAX_MESSAGES).collect();
    let value = json!({"version": VERSION, "account": account.to_string(), "channel": channel.to_string(), "page": kept, "pictures": pictures, "members": members});
    if serde_json::to_vec(&value).is_ok_and(|bytes| bytes.len() <= MAX_MESSAGES_BYTES) {
        std::thread::spawn(move || write_atomic(MESSAGES_FILE, &value));
    }
}

/// Keeps the open server's member list for the next start: `update` is a `GUILD_MEMBER_LIST_UPDATE` with one SYNC of
/// its top rows, `pictures` the rows' avatars (key and URL; the picture cache holds their bytes).
pub(crate) fn save_member_list(
    account: Option<u64>,
    guild: u64,
    update: &Value,
    pictures: &[(String, String)],
) {
    let (true, Some(account)) = (enabled(), account) else {
        return;
    };
    let pictures: Vec<Value> = pictures
        .iter()
        .filter(|(_, url)| url.starts_with("https://"))
        .take(MAX_MEMBER_ROWS)
        .map(|(key, url)| json!([key, url]))
        .collect();
    let value = json!({"version": VERSION, "account": account.to_string(), "guild": guild.to_string(), "update": update, "pictures": pictures});
    if serde_json::to_vec(&value).is_ok_and(|bytes| bytes.len() <= MAX_MEMBER_LIST_BYTES) {
        std::thread::spawn(move || write_atomic(MEMBERS_FILE, &value));
    }
}

/// What the members file keeps.
struct KeptMemberList {
    guild: u64,
    update: Value,
    pictures: Vec<(String, String)>,
}

/// The kept member list, when it was kept for `account` and is one SYNC of at most `MAX_MEMBER_ROWS` rows.
fn member_list_for(mut value: Value, account: Option<u64>) -> Option<KeptMemberList> {
    let kept_for = value["account"].as_str()?.parse::<u64>().ok()?;
    if Some(kept_for) != account {
        return None;
    }
    let guild: u64 = value["guild"].as_str()?.parse().ok()?;
    let update = value["update"].take();
    let ops = update["ops"].as_array()?;
    let [op] = ops.as_slice() else {
        return None;
    };
    let rows = op["items"].as_array()?.len();
    if op["op"] != "SYNC" || rows == 0 || rows > MAX_MEMBER_ROWS {
        return None;
    }
    if update["guild_id"].as_str() != Some(guild.to_string().as_str()) {
        return None;
    }
    let pictures = value["pictures"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_MEMBER_ROWS)
        .filter_map(|pair| {
            Some((
                pair.get(0)?.as_str()?.to_owned(),
                pair.get(1)?.as_str()?.to_owned(),
            ))
        })
        .filter(|(_, url)| url.starts_with("https://"))
        .collect();
    Some(KeptMemberList {
        guild,
        update,
        pictures,
    })
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

/// The account a login token belongs to: its first part is the account's id in base64. `None` for a token of another
/// shape, which then starts without the cache. The id is never logged.
pub(crate) fn token_account(token: &str) -> Option<u64> {
    use base64::Engine as _;
    let first = token.split('.').next()?.trim_end_matches('=');
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(first)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(first))
        .ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Whether a cached READY is the account's: the cache of another account (one signed in before a dead login was
/// replaced by another, say) is never drawn.
fn is_for(ready: &Value, account: Option<u64>) -> bool {
    account.is_some_and(|account| {
        ready["user"]["id"]
            .as_str()
            .and_then(|id| id.parse::<u64>().ok())
            == Some(account)
    })
}

/// What the messages file keeps of the screen in view.
struct KeptPage {
    channel: u64,
    page: Value,
    /// Image key and URL of the screen's pictures.
    pictures: Vec<(String, String)>,
    /// The server member objects of the page's authors (`guild_id` added), so their names and colors are right in the
    /// first frame.
    members: Vec<Value>,
}

/// The kept page of the messages file, when it was kept for `account`.
fn messages_for(mut value: Value, account: Option<u64>) -> Option<KeptPage> {
    let kept_for = value["account"].as_str()?.parse::<u64>().ok()?;
    if Some(kept_for) != account {
        return None;
    }
    let channel = value["channel"].as_str()?.parse().ok()?;
    let members = match value["members"].take() {
        Value::Array(members) => members.into_iter().take(MAX_MESSAGES).collect(),
        _ => Vec::new(),
    };
    let pictures = value["pictures"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_PICTURES)
        .filter_map(|pair| {
            Some((
                pair.get(0)?.as_str()?.to_owned(),
                pair.get(1)?.as_str()?.to_owned(),
            ))
        })
        // The pictures kept for the start are read by key from their own folder; the others by URL from the
        // picture cache, so only https ones.
        .filter(|(key, url)| kept_for_the_start(key) || url.starts_with("https://"))
        .collect();
    Some(KeptPage {
        channel,
        page: value["page"].take(),
        pictures,
        members,
    })
}

/// The bytes the caches hold for `pictures`, at most `MAX_PICTURE_BYTES` in all: one that does not fit is skipped and
/// the smaller ones after it still come (the list is the most wanted first). A copy past its freshness is still the
/// picture of its URL (the URLs name the picture's hash), and the usual request refreshes it.
fn read_pictures(pictures: &[(String, String)]) -> Vec<(String, Vec<u8>)> {
    read_pictures_within(pictures, MAX_PICTURE_BYTES)
}

fn read_pictures_within(pictures: &[(String, String)], budget: usize) -> Vec<(String, Vec<u8>)> {
    let mut total = 0;
    let mut out = Vec::new();
    for (key, url) in pictures {
        let bytes = if kept_for_the_start(key) {
            read_start_picture(key)
        } else {
            super::shop::art_cache::read_raw(url).map(|cached| cached.bytes)
        };
        let Some(bytes) = bytes else {
            continue;
        };
        if total + bytes.len() > budget {
            continue;
        }
        total += bytes.len();
        out.push((key.clone(), bytes));
    }
    out
}

/// What the last run left for `account`, if this run uses the cache and the files are good and are that account's.
pub(crate) fn load(account: Option<u64>) -> Option<Cached> {
    if !enabled() || account.is_none() {
        return None;
    }
    let ready = load_ready_from(&file(READY_FILE)?, MAX_READY_BYTES + 1024)?["ready"].clone();
    if !is_for(&ready, account) {
        return None;
    }
    let (messages, pictures, members) = match read(MESSAGES_FILE, MAX_MESSAGES_BYTES)
        .and_then(|value| messages_for(value, account))
    {
        Some(kept) => (
            Some((kept.channel, kept.page)),
            read_pictures(&kept.pictures),
            kept.members,
        ),
        None => (None, Vec::new(), Vec::new()),
    };
    let mut pictures = pictures;
    let member_list = read(MEMBERS_FILE, MAX_MEMBER_LIST_BYTES)
        .and_then(|value| member_list_for(value, account))
        .map(|kept| {
            let have: std::collections::HashSet<String> =
                pictures.iter().map(|(key, _)| key.clone()).collect();
            let wanted: Vec<(String, String)> = kept
                .pictures
                .into_iter()
                .filter(|(key, _)| !have.contains(key))
                .collect();
            pictures.extend(read_pictures_within(&wanted, MAX_MEMBER_PICTURE_BYTES));
            (kept.guild, kept.update)
        });
    let wishlists = file(WISHLISTS_FILE)
        .and_then(|path| load_wishlists_from(&path, MAX_WISHLISTS_BYTES + 1024));
    crate::start_trace::mark("cache_read");
    Some(Cached {
        ready,
        messages,
        wishlists,
        pictures,
        members,
        member_list,
    })
}

/// The channel of the place the app was last on (`SelectedChannelStore`), when it was a server's: the page worth
/// asking for before anything else. A harness run keeps no place, so it has none. Read on the UI thread.
pub(crate) fn saved_channel() -> Option<u64> {
    let guild = super::last_channel::last()??;
    super::last_channel::channel_of(guild)
}

/// Logging out removes what the account left, so the next login never starts from it.
pub(crate) fn forget() {
    for name in [
        READY_FILE,
        LEGACY_READY_FILE,
        MESSAGES_FILE,
        WISHLISTS_FILE,
        MEMBERS_FILE,
    ] {
        if let Some(path) = file(name) {
            let _ = std::fs::remove_file(path);
        }
    }
    if let Some(dir) = file(START_PICTURES_DIR) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// A member list to keep: its server, its `GUILD_MEMBER_LIST_UPDATE` and its rows' avatars (key and URL).
type KeptList = (u64, Value, Vec<(String, String)>);

impl Root {
    /// Applies the cache before the gateway answers: the same stores, filled the way READY fills them, and the
    /// server and channel the app opens on. The real READY that follows replaces them as a reconnect's does.
    pub(crate) fn apply_cache(&mut self, cached: Cached, cx: &mut Context<Self>) {
        if self.stores.current_user.is_some() {
            // The gateway was first: nothing to draw from the past.
            return;
        }
        let applying = std::time::Instant::now();
        self.add_start_pictures(cached.pictures);
        self.cached_ready = Some(cached.ready.clone());
        let dispatching = std::time::Instant::now();
        self.stores.dispatch("READY", &cached.ready);
        crate::start_trace::took("dur_cache_dispatch", dispatching);
        for member in &cached.members {
            let Some(guild) = member["guild_id"].as_str().and_then(|id| id.parse().ok()) else {
                continue;
            };
            if self.stores.guild(guild).is_none() {
                continue;
            }
            self.stores.members.apply_member(guild, member, None);
            if let Some(user) = member["user"]["id"].as_str().and_then(|id| id.parse().ok()) {
                self.start_members.push((guild, user));
            }
        }
        self.hydrated_from_cache = true;
        self.reset_theme_sync();
        let opening = std::time::Instant::now();
        self.after_ready();
        crate::start_trace::took("dur_cache_after_ready", opening);
        self.request_group_images();
        let wishing = std::time::Instant::now();
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
                self.fetched
                    .insert(format!("profile-wishlist:{user}"), list);
            }
        }
        crate::start_trace::took("dur_cache_wishlists", wishing);
        // The member list as the last run showed it, when the app opens on that server.
        if let Some((guild, update)) = cached.member_list
            && Some(guild) == self.guild
        {
            self.stores.dispatch("GUILD_MEMBER_LIST_UPDATE", &update);
            self.start_member_list = Some((guild, update));
            crate::start_trace::mark("cache_members");
        }
        if let Some((channel, page)) = cached.messages
            && Some(channel) == self.channel
        {
            let loading = std::time::Instant::now();
            self.stores.load_messages_since(channel, &page, 0);
            self.rebuild_rows();
            crate::start_trace::took("dur_cache_rows", loading);
            let asking = std::time::Instant::now();
            self.request_images();
            crate::start_trace::took("dur_cache_images", asking);
            self.ready = true;
            crate::start_trace::mark("cache_messages");
        }
        crate::start_trace::took("dur_cache_apply", applying);
        crate::start_trace::mark("cache_applied");
        cx.notify();
    }

    /// The first screen's pictures the cache read (Cached::pictures), put in before anything asks for them; the
    /// usual requests still run and bring the same bytes (or a newer copy) behind them. A picture already in is kept.
    pub(crate) fn add_start_pictures(&mut self, pictures: Vec<(String, Vec<u8>)>) {
        let kept = pictures
            .iter()
            .filter(|(key, _)| kept_for_the_start(key))
            .count();
        tracing::debug!(target: "gravecord::start", pictures = pictures.len(), kept, "start pictures read");
        for (key, bytes) in pictures {
            let format = super::image_format(&key, &bytes);
            self.images
                .entry(key)
                .or_insert_with(|| std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)));
        }
    }

    /// The start's timeline as the frames show it (`start_trace`): the first frame, the first with messages, with the
    /// live page, with the DM list and the member list, and the first in which the rail's icons and the avatars of the
    /// open channel's authors are all decoded. Nothing is done once those are reached or the start is long past.
    pub(super) fn trace_start_frame(&mut self, window: &mut gpui::Window, cx: &mut gpui::App) {
        use crate::start_trace::{mark, now_ms, reached};
        if now_ms() > 20_000
            || (reached("frame_pictures") && reached("frame_dm_list") && reached("frame_members"))
        {
            return;
        }
        mark("first_render");
        if self.stores.current_user.is_some() {
            mark("frame_user");
        }
        let messages = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel));
        let has_messages = messages.is_some_and(|list| list.iter().next().is_some());
        if has_messages {
            mark("frame_messages");
            if reached("msgs_applied") {
                mark("frame_live_messages");
            }
        }
        if !self.stores.private_channels.is_empty() {
            mark("frame_dm_list");
        }
        if reached("members_applied") {
            mark("frame_members");
        } else if self.start_member_list.is_some() {
            mark("frame_cached_members");
        }
        #[cfg(feature = "capture")]
        if has_messages && !reached("frame_pictures") {
            self.trace_start_pictures(window, cx);
        }
        #[cfg(not(feature = "capture"))]
        let _ = (window, cx);
    }

    /// Capture runs only: whether the rail's icons and the open channel's avatars are all decoded. It only looks: a
    /// picture whose decode has not started is counted missing, and none is started here.
    #[cfg(feature = "capture")]
    fn trace_start_pictures(&mut self, window: &mut gpui::Window, cx: &mut gpui::App) {
        use crate::start_trace::{mark, reached};
        let missing = self
            .start_picture_states(window, cx)
            .iter()
            // A picture with its bytes in that no frame has painted (an author scrolled out of view) is `queued`: it is
            // not waited for.
            .filter(|(_, state)| matches!(*state, "absent" | "decoding"))
            .count();
        if missing == 0 {
            mark(if reached("msgs_applied") {
                "frame_pictures"
            } else {
                "frame_cached_pictures"
            });
        }
    }

    /// The pictures the start waits for and where each is: `absent` (no bytes yet), `queued` (bytes, no decode
    /// started), `decoding`, `decoded`. For `perf pictures`, which names only the kind of each key.
    #[cfg(feature = "capture")]
    pub(crate) fn start_picture_states(
        &self,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> Vec<(String, &'static str)> {
        let messages = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel));
        let mut keys: Vec<String> = messages
            .into_iter()
            .flatten()
            .map(|message| self.guild_avatar_source(&message.author).0)
            .collect();
        keys.extend(
            self.stores
                .guilds
                .iter()
                .filter(|guild| guild.icon.is_some())
                .take(12)
                .map(|guild| format!("guild:{}", guild.id)),
        );
        // The attachments' previews and server emojis of the newest messages, which the start keeps too.
        let messages: Vec<_> = messages.into_iter().flatten().rev().take(25).collect();
        keys.extend(
            messages
                .iter()
                .flat_map(|message| super::media_image::wanted(message, false, false))
                .map(|(key, _)| key)
                .filter(|key| kept_for_the_start(key) && key != EMOJI_BUTTON),
        );
        keys.sort();
        keys.dedup();
        keys.into_iter()
            .map(|key| {
                let state = match self.images.get(&key) {
                    None => "absent",
                    Some(image) if !image.is_asset_cached(cx) => "queued",
                    Some(image) if image.clone().get_render_image(window, cx).is_none() => {
                        "decoding"
                    }
                    Some(_) => "decoded",
                };
                (key, state)
            })
            .collect()
    }

    /// The pictures of the screen in view, as image key and URL, the most wanted first: the account's own avatar, the
    /// avatars of the open channel's authors (newest messages first), the rail's server icons, the server notice's
    /// illustration, the composer's emoji button, on Home the DM list's avatars, and last the newest messages' own
    /// pictures. Kept with the messages, they are read with the cache at the next start.
    pub(super) fn start_pictures(&self) -> Vec<(String, String)> {
        fn push(out: &mut Vec<(String, String)>, pair: (String, String)) {
            if out.len() < MAX_PICTURES && !out.iter().any(|(key, _)| *key == pair.0) {
                out.push(pair);
            }
        }
        let mut out = Vec::new();
        if let Some(me) = &self.stores.current_user {
            push(&mut out, super::avatar_source(me));
        }
        let messages = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel));
        for message in messages.into_iter().flatten().rev() {
            push(&mut out, self.guild_avatar_source(&message.author));
        }
        for guild in &self.stores.guilds {
            if let Some(icon) = &guild.icon {
                push(
                    &mut out,
                    (
                        format!("guild:{}", guild.id),
                        format!(
                            "https://cdn.discordapp.com/icons/{}/{icon}.webp?size=96",
                            guild.id
                        ),
                    ),
                );
            }
        }
        // The open server's notice illustration ("An adventure begins."), a client asset kept on disk.
        if let Some(picture) = self.channel_notice_picture() {
            push(&mut out, (picture.to_owned(), picture.to_owned()));
        }
        if self.channel.is_some() {
            push(&mut out, (EMOJI_BUTTON.to_owned(), String::new()));
        }
        if self.guild.is_none() {
            for dm in &self.stores.private_channels {
                if let Some(user) = self.stores.dm_recipient(dm) {
                    push(&mut out, super::avatar_source(user));
                }
            }
        }
        if let Some(messages) = messages {
            // The newest messages' own pictures: the ones the picture cache keeps (file kind icons, stickers, client
            // assets) and the attachments' previews and server emojis kept for the start.
            let gifs = self.pref_on("animate_gifs", true) && !super::motion::reduced();
            let emojis = self.pref_on("animate_emojis", true);
            let newest: Vec<_> = messages.iter().rev().take(25).collect();
            let wanted = newest
                .iter()
                .flat_map(|message| super::media_image::wanted(message, gifs, emojis))
                .chain(super::message_kinds::wanted(messages));
            for (key, url) in wanted {
                if crate::net::disk_cacheable_raw_image(&key, &url)
                    || (enabled() && kept_for_the_start(&key) && !url.starts_with("data:"))
                {
                    push(&mut out, (key, url));
                }
            }
        }
        out
    }

    /// Writes the screen's pictures that only the start keeps (`kept_for_the_start`): the ones in now at once, the
    /// others as they come in (`start_picture_arrived`). The screen's set replaces the last one's on disk.
    pub(super) fn keep_start_pictures(&mut self, pictures: &[(String, String)]) {
        self.start_keep.clear();
        if !enabled() {
            return;
        }
        let wanted: Vec<String> = pictures
            .iter()
            .map(|(key, _)| key)
            .filter(|key| kept_for_the_start(key))
            .cloned()
            .collect();
        let mut have = Vec::new();
        for key in &wanted {
            match self.images.get(key) {
                Some(image) => have.push((key.clone(), image.clone())),
                None => {
                    self.start_keep.insert(key.clone());
                }
            }
        }
        write_start_pictures(have, Some(wanted));
    }

    /// A picture came in: written when the screen's start pictures wait for it. The emoji button is written each time
    /// it is cut, so a new tint replaces the old one.
    pub(super) fn start_picture_arrived(&mut self, key: &str, image: &std::sync::Arc<gpui::Image>) {
        if self.start_keep.remove(key) || (key == EMOJI_BUTTON && enabled()) {
            write_start_pictures(vec![(key.to_owned(), image.clone())], None);
        }
    }
    /// The open server channel's authors as server members (`guild_id` added), for the next start's first frame.
    pub(super) fn start_members_to_keep(&self) -> Vec<Value> {
        let (Some(guild), Some(channel)) = (self.guild, self.channel) else {
            return Vec::new();
        };
        let mut seen = std::collections::HashSet::new();
        self.stores
            .messages
            .get(&channel)
            .into_iter()
            .flatten()
            .rev()
            .filter(|message| seen.insert(message.author.id))
            .filter_map(|message| self.stores.members.get(guild, message.author.id))
            .take(MAX_MESSAGES)
            .map(|member| {
                let mut value = member.to_cache_value();
                value["guild_id"] = Value::String(guild.to_string());
                value
            })
            .collect()
    }

    /// The members the cache brought are asked for again by id once the gateway is up: the answer replaces them, and
    /// one who is no longer a member is dropped (`not_found`). Until then they keep the names and colors of the first
    /// frame from changing under it.
    pub(super) fn refresh_start_members(&mut self) {
        let members = std::mem::take(&mut self.start_members);
        let mut by_guild: std::collections::BTreeMap<u64, Vec<u64>> = Default::default();
        for (guild, user) in members {
            by_guild.entry(guild).or_default().push(user);
        }
        for (guild, user_ids) in by_guild {
            let _ = self.requests.send(crate::net::Request::GuildMembers {
                guilds: vec![guild],
                query: String::new(),
                limit: 0,
                user_ids,
            });
        }
    }

    /// Starts decoding the pictures the cache brought (`Cached::pictures`) on the background threads, before the first
    /// frame asks for them. gpui keeps each decode by the picture's bytes, so the frame's own `img` finds it done.
    pub(crate) fn decode_start_pictures(&mut self, window: &mut gpui::Window, cx: &mut gpui::App) {
        let pictures: Vec<std::sync::Arc<gpui::Image>> = self.images.values().cloned().collect();
        for picture in pictures {
            let _ = picture.get_render_image(window, cx);
        }
        crate::start_trace::mark("pictures_decoding");
    }

    /// The open server's member list as the window shows it, for the next start: a `GUILD_MEMBER_LIST_UPDATE` with one
    /// SYNC of the rows that fit in the window (each member with the presence that sets its status and activity line,
    /// so a row is as tall as it will be live) and the rows' avatars. `None` before the list is in.
    pub(super) fn member_list_to_keep(&self) -> Option<KeptList> {
        use dc_state::member_list::Row;
        let guild = self.guild?;
        let list = self.stores.member_lists.get(&guild)?;
        let fits = ((self.viewport.1 / 42.).ceil() as usize + 2).min(MAX_MEMBER_ROWS);
        let mut items = Vec::new();
        let mut pictures = Vec::new();
        for row in list.rows.iter().take(fits) {
            match row {
                Row::Group { id, count } => {
                    items.push(json!({"group": {"id": id, "count": count}}))
                }
                Row::Member {
                    user,
                    nick,
                    roles,
                    status,
                } => {
                    let mut member = self
                        .stores
                        .members
                        .get(guild, user.id)
                        .map(dc_state::members::Member::to_cache_value)
                        .unwrap_or_else(|| json!({}));
                    member["user"] = user.to_cache_value();
                    member["nick"] = json!(nick);
                    member["roles"] = roles.iter().map(|role| json!(role.to_string())).collect();
                    let presence = self.stores.presence.get(user.id, Some(guild));
                    member["presence"] = json!({
                        "user": {"id": user.id.to_string()},
                        "status": status,
                        "activities": presence.map(|presence| presence.activities.clone()).unwrap_or_default(),
                        "client_status": presence.map_or(Value::Null, |presence| presence.clients.clone()),
                    });
                    items.push(json!({ "member": member }));
                    pictures.push(self.guild_avatar_source(user));
                }
                Row::Unknown => break,
            }
        }
        if items.is_empty() {
            return None;
        }
        let groups: Vec<Value> = list
            .groups
            .iter()
            .map(|(id, count)| json!({"id": id, "count": count}))
            .collect();
        let last = items.len() - 1;
        let update = json!({
            "guild_id": guild.to_string(),
            "id": "everyone",
            "member_count": self.stores.member_counts.get_member_count(guild),
            "online_count": self.stores.member_counts.get_online_count(guild),
            "groups": groups,
            "ops": [{"op": "SYNC", "range": [0, last], "items": items}],
        });
        Some((guild, update, pictures))
    }

    /// A live `GUILD_MEMBER_LIST_UPDATE` is about to be applied. The first SYNC of the server whose list came from the
    /// start cache replaces that list whole (a SYNC only writes its own range, and the cached rows past it would stay).
    pub(super) fn member_list_arriving(&mut self, data: &Value) {
        let Some((guild, _)) = &self.start_member_list else {
            return;
        };
        let same = data["guild_id"].as_str() == Some(guild.to_string().as_str());
        let syncs = data["ops"]
            .as_array()
            .is_some_and(|ops| ops.iter().any(|op| op["op"] == "SYNC"));
        if same && syncs {
            self.stores.member_lists.remove(guild);
            self.start_member_list = None;
        }
    }

    /// The live list is in: it is kept for the next start the first time, then at most every 30 s.
    pub(super) fn member_list_applied(&mut self, data: &Value) {
        let Some(guild) = self.guild else {
            return;
        };
        if data["guild_id"].as_str() != Some(guild.to_string().as_str()) || !enabled() {
            return;
        }
        if self.member_list_saved.is_some_and(|(saved, at)| {
            saved == guild && at.elapsed() < std::time::Duration::from_secs(30)
        }) {
            return;
        }
        let Some((guild, update, pictures)) = self.member_list_to_keep() else {
            return;
        };
        self.member_list_saved = Some((guild, std::time::Instant::now()));
        save_member_list(
            self.stores.current_user.as_ref().map(|me| me.id),
            guild,
            &update,
            &pictures,
        );
    }

    /// The live READY empties the presence store: the cached rows keep their presences (and so their heights and
    /// status dots) until the live list replaces them.
    pub(super) fn keep_start_member_presences(&mut self) {
        if let Some((guild, update)) = &self.start_member_list
            && Some(*guild) == self.guild
        {
            self.stores
                .presence
                .dispatch("GUILD_MEMBER_LIST_UPDATE", update);
        }
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
        assert!(guilds[0].get("members").is_none() && guilds[0]["threads"].is_array());
        assert!(guilds[1]["roles"].is_array() && guilds[1].get("emojis").is_none());
        assert!(guilds[1].get("threads").is_none());
        assert!(guilds[1]["channels"].is_array());
        assert_eq!(guilds[1]["properties"]["name"], "closed");
    }

    #[test]
    fn zstd_ready_roundtrips_and_honors_decompression_cap() {
        let dir =
            std::env::temp_dir().join(format!("gravecord-start-cache-test-{}", std::process::id()));
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
    fn a_token_names_its_account_and_odd_tokens_name_none() {
        use base64::Engine as _;
        let id = "1554855651514122281";
        let head = base64::engine::general_purpose::STANDARD_NO_PAD.encode(id);
        assert_eq!(
            token_account(&format!("{head}.GabcDE.signature-part")),
            Some(1554855651514122281)
        );
        // Padded and URL-safe heads read the same.
        let padded = base64::engine::general_purpose::STANDARD.encode(id);
        assert_eq!(
            token_account(&format!("{padded}.x.y")),
            Some(1554855651514122281)
        );
        assert_eq!(token_account("mfa.something-else"), None);
        assert_eq!(token_account(""), None);
        let words = base64::engine::general_purpose::STANDARD_NO_PAD.encode("not-an-id");
        assert_eq!(token_account(&format!("{words}.x.y")), None);
    }

    #[test]
    fn only_the_screens_attachments_and_emojis_are_kept_for_the_start() {
        assert!(kept_for_the_start("attachment:2"));
        assert!(kept_for_the_start("embed:7:0:image") && kept_for_the_start("comp:7:media:0"));
        assert!(kept_for_the_start("custom:5") && kept_for_the_start("custom-animated:5"));
        assert!(kept_for_the_start(EMOJI_BUTTON));
        assert!(
            !kept_for_the_start("avatar:1"),
            "avatars go through the usual cache"
        );
        // Named by key, one file per picture, nothing that leaves the folder.
        assert_eq!(start_picture_name("attachment:12"), "attachment_12.bin");
        assert_eq!(start_picture_name("../x"), "___x.bin");
        // A kept page's start pictures are read by key whatever their URL; others only over https.
        let page = json!({"version": VERSION, "account": "1", "channel": "100", "page": [],
            "pictures": [["attachment:9", "https://media.discordapp.net/attachments/1/9/a.png?ex=1"],
                ["emoji_button", ""], ["avatar:3", "http://insecure"]]});
        let kept = messages_for(page, Some(1)).unwrap();
        let keys: Vec<&str> = kept.pictures.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["attachment:9", "emoji_button"]);
    }

    #[test]
    fn a_cache_is_used_only_by_its_own_account() {
        let ready = trim_ready(&ready(), Some("10")).unwrap();
        assert!(is_for(&ready, Some(1)));
        assert!(
            !is_for(&ready, Some(2)),
            "another account's cache is never drawn"
        );
        assert!(
            !is_for(&ready, None),
            "a token without an account starts cold"
        );
        let cached = Cached {
            ready,
            messages: None,
            wishlists: None,
            pictures: Vec::new(),
            members: Vec::new(),
            member_list: None,
        };
        assert_eq!(cached.account(), Some(1));
        let page = json!({"version": VERSION, "account": "1", "channel": "100", "page": [{"id": "5"}],
            "pictures": [["avatar:2", "https://cdn.discordapp.com/avatars/2/h.webp?size=80"], ["x", "file:///etc"], [1]],
            "members": [{"guild_id": "10", "user": {"id": "2", "username": "other"}, "roles": ["1"]}]});
        let kept = messages_for(page.clone(), Some(1)).unwrap();
        assert_eq!(kept.channel, 100);
        assert_eq!(kept.members.len(), 1);
        assert_eq!(
            kept.pictures,
            vec![(
                "avatar:2".to_owned(),
                "https://cdn.discordapp.com/avatars/2/h.webp?size=80".to_owned()
            )],
            "only well-formed https entries are read"
        );
        assert!(messages_for(page, Some(2)).is_none());
        let unkeyed = json!({"version": VERSION, "channel": "100", "page": []});
        assert!(
            messages_for(unkeyed, Some(1)).is_none(),
            "a page kept before pages named their account is not drawn"
        );
    }

    #[test]
    fn a_kept_member_list_is_one_sync_of_its_own_account_and_server() {
        let item = |user: &str| {
            json!({"member": {"user": {"id": user, "username": "u"}, "roles": [],
            "presence": {"user": {"id": user}, "status": "online", "activities": [{"type": 4, "state": "hi"}]}}})
        };
        let update = json!({"guild_id": "10", "id": "everyone", "groups": [{"id": "online", "count": 2}],
            "ops": [{"op": "SYNC", "range": [0, 2], "items": [{"group": {"id": "online", "count": 2}}, item("2"), item("3")]}]});
        let file = json!({"version": VERSION, "account": "1", "guild": "10", "update": update,
            "pictures": [["avatar:2", "https://cdn.discordapp.com/avatars/2/a.webp?size=80"], ["x", "file:///etc"]]});
        let kept = member_list_for(file.clone(), Some(1)).unwrap();
        assert_eq!(kept.guild, 10);
        assert_eq!(kept.pictures.len(), 1, "only https pictures are read");
        assert!(
            member_list_for(file.clone(), Some(2)).is_none(),
            "another account's list is never drawn"
        );
        let mut other = file.clone();
        other["guild"] = json!("11");
        assert!(
            member_list_for(other, Some(1)).is_none(),
            "the list names its own server"
        );
        let mut two_ops = file;
        two_ops["update"]["ops"] = json!([{"op": "SYNC", "range": [0, 0], "items": [item("2")]},
            {"op": "DELETE", "index": 0}]);
        assert!(member_list_for(two_ops, Some(1)).is_none());
        // Replayed as the gateway's own update, the rows, their presences and the group come back.
        let mut stores = dc_state::Stores::default();
        stores.dispatch("GUILD_MEMBER_LIST_UPDATE", &kept.update);
        let list = stores.member_lists.get(&10).unwrap();
        assert_eq!(list.rows.len(), 3);
        assert_eq!(list.groups, vec![("online".to_owned(), 2)]);
        let presence = stores.presence.get(2, Some(10)).unwrap();
        assert_eq!(presence.status, "online");
        assert_eq!(presence.activities.len(), 1);
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
        assert_eq!(
            loaded.wishlist_ids.get(&2).map(String::as_str),
            Some("wl_2")
        );
        assert_eq!(loaded.products.len(), 1);
        assert_eq!(loaded.products[0].sku_id, "77");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
