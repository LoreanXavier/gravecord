//! MessageLoggerEnhanced (Equicord `src/equicordplugins/messageLoggerEnhanced`, GPL-3.0-or-later): a log of deleted,
//! edited and ghost-pinging messages kept on disk across restarts (the original's IndexedDB `MessageLoggerIDB`),
//! beside MessageLogger, which it builds on (`plugin_hooks/message_logger.rs`, the original's dependency).
//!
//! - What it sees: every message of a loaded page and, from the gateway, every DM message and every server message
//!   whose channel is open here (the original's MessageStore), plus other servers' messages with
//!   `cacheMessagesFromServers` or when their channel, author or server is whitelisted (`cacheSentMessages`, at most
//!   `cacheLimit`).
//! - A delete of a seen message is logged as Deleted, or Ghost Pinged when it mentioned you or everyone; an edit as
//!   Edited with its earlier texts (`editHistory`), unless `shouldIgnore` says otherwise (ephemeral; yourself with
//!   `ignoreSelf`; DMs always kept with `alwaysLogDirectMessages`; bots and webhooks by their switches unless the
//!   author is whitelisted; ghost pings kept; then the author's, the channel's lists, the open channel
//!   (`alwaysLogCurrentChannel`), any whitelisting, a server message only this cache saw, any blacklisting
//!   (`blacklistedIds` and MessageLogger's three lists), muted servers, categories and channels by their switches).
//!   A delete it ignores also leaves the chat, as the original's `mlDeleted` delete does. `saveMessages` off keeps
//!   nothing new.
//! - `saveImages`: a deleted message's attachments within `attachmentSizeLimitInMegabytes` and of the listed
//!   `attachmentFileExtensions` (never the blocked types) are fetched from Discord's CDN and saved as
//!   `<attachment id>.<ext>` in the image folder (`imageCacheDir`), and the re-added message shows the saved file.
//! - With MessageLogger on, a loaded page gets the logged deleted messages of its stretch back
//!   (`reAddDeletedMessages`), drawn deleted by MessageLogger, and its messages their logged edits, so both survive a
//!   restart. With MessageLogger off the log is kept and shown in the log view only.
//! - Limits: `messageLimit` drops the oldest records past it, `timeBasedCleanupMinutes` server messages older than
//!   that (at most once a minute, the open channel spared with `preserveCurrentChannel`), `clearLogsOnRestart`
//!   empties the log at start. `permanentlyRemoveLogByDefault` makes MessageLogger's Clear Message History also
//!   remove the record.
//! - The log view (`LogsModal`): Deleted, Edited and Ghost Pinged tabs, the filter (`from:`, `in:`, `server:`,
//!   `has:`, `before:`, `after:`, `around:`, `message:` and words, `!` negating), the messages grouped as the chat
//!   groups them with "From …" under each with `ShowWhereMessageIsFrom`, `messagesToDisplayAtOnceInLogs` at a time
//!   with Load More, Sort Oldest/Newest First (stored as `sortNewest`), Clear Visible Logs and Clear All Logs with
//!   their questions, "No results in …" and the empty log's Import Logs, and a right-click menu per message (Jump To
//!   Message, Open user profile, Copy Content, Copy User ID, Copy Message ID, Copy Channel ID, Copy Server ID, Delete
//!   Log). It opens from the title bar's Open Logs button (`ShowLogsButton`), the card's Open Logs, and the "Message
//!   Logger" submenu of the message, channel, person, server and group DM menus, which also opens the logs for that
//!   user, channel or server, puts it on or takes it off the whitelist or blacklist, and for a logged message removes
//!   its record for good.
//! - The card's own section: Open Logs, Clear Logs, Import Logs and Export Logs (the original's `{"messages": [...]}`
//!   file), the image and logs folders with Browse, and Open Image Cache Folder.
//!
//! Not part of the pinned client.

use super::super::modal::{self, Tone};
use super::super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::plugins::message_logger_enhanced::{
    self as rules, IgnoreSettings, Place, Record, SAVED_SCHEME, Seen, Status,
};
use chrono::{DateTime, Utc};
use dc_state::{Id, Message};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, ClipboardItem, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, px,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const ID: &str = "MessageLoggerEnhanced";
/// The key of a saved attachment's fetch.
pub(in crate::ui) const IMAGE_PREFIX: &str = "mle-save:";
/// `LOGS_DATA_FILENAME` is the export's; the log itself lives beside it.
const LOG_FILE: &str = "messages.json";
const EXPORT_NAME: &str = "message-logger-logs-idb.json";
/// `CLEANUP_COOLDOWN`.
const CLEANUP_COOLDOWN: Duration = Duration::from_secs(60);

/// The settings the plugin acts on.
#[derive(Clone, Debug)]
struct Settings {
    save_messages: bool,
    save_images: bool,
    ignore: IgnoreSettings,
    permanently_remove: bool,
    show_where: bool,
    at_once: usize,
    message_limit: usize,
    size_limit: f64,
    extensions: String,
    cache_limit: usize,
    cleanup_minutes: f64,
    preserve_current: bool,
    clear_on_restart: bool,
    images_dir: String,
    logs_dir: String,
}

fn current() -> Option<Settings> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(|state| {
        if !state.is_on(ID) {
            return None;
        }
        let flag = |key: &str| state.value(ID, key) == Value::Bool(true);
        let text = |key: &str| state.value(ID, key).as_str().unwrap_or_default().to_owned();
        let key = |name: &str| state.value(ID, name).as_f64().unwrap_or(0.);
        let base = |key: &str| {
            state
                .value("MessageLogger", key)
                .as_str()
                .unwrap_or_default()
                .to_owned()
        };
        Some(Settings {
            save_messages: flag("saveMessages"),
            save_images: flag("saveImages"),
            ignore: IgnoreSettings {
                ignore_bots: flag("ignoreBots"),
                ignore_self: flag("ignoreSelf"),
                ignore_webhooks: flag("ignoreWebhooks"),
                always_log_dms: flag("alwaysLogDirectMessages"),
                always_log_current: flag("alwaysLogCurrentChannel"),
                cache_from_servers: flag("cacheMessagesFromServers"),
                ignore_muted_guilds: flag("ignoreMutedGuilds"),
                ignore_muted_categories: flag("ignoreMutedCategories"),
                ignore_muted_channels: flag("ignoreMutedChannels"),
                whitelisted: text("whitelistedIds"),
                blacklisted: text("blacklistedIds"),
                base_lists: [
                    base("ignoreUsers"),
                    base("ignoreChannels"),
                    base("ignoreGuilds"),
                ],
            },
            permanently_remove: flag("permanentlyRemoveLogByDefault"),
            show_where: flag("ShowWhereMessageIsFrom"),
            at_once: key("messagesToDisplayAtOnceInLogs").max(1.) as usize,
            message_limit: key("messageLimit") as usize,
            size_limit: key("attachmentSizeLimitInMegabytes"),
            extensions: text("attachmentFileExtensions"),
            cache_limit: key("cacheLimit") as usize,
            cleanup_minutes: key("timeBasedCleanupMinutes"),
            preserve_current: flag("preserveCurrentChannel"),
            clear_on_restart: flag("clearLogsOnRestart"),
            images_dir: text("imageCacheDir"),
            logs_dir: text("logsDir"),
        })
    })
}

/// Whether the plugin is on (no setting read: the title bar asks each frame).
fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("MessageLoggerEnhanced")
}

fn show_logs_button() -> bool {
    on() && crate::plugins::with(|state| state.value(ID, "ShowLogsButton") == Value::Bool(true))
}

/// The plugin's data folder (`MessageLoggerData`).
fn data_dir() -> Option<PathBuf> {
    crate::storage::data_dir().map(|dir| dir.join("MessageLoggerData"))
}

/// The image folder: `imageCacheDir`, else `MessageLoggerData/savedImages`.
fn images_dir(settings: &Settings) -> Option<PathBuf> {
    if settings.images_dir.is_empty() {
        data_dir().map(|dir| dir.join("savedImages"))
    } else {
        Some(PathBuf::from(&settings.images_dir))
    }
}

/// The run may write its files: never in tests, and a harness run only when it keeps its settings
/// (`GRAVECORD_STORAGE_WRITE=1`), as `storage.json`.
fn writes() -> bool {
    !cfg!(test)
        && (!crate::storage::is_capture_run()
            || std::env::var("GRAVECORD_STORAGE_WRITE").as_deref() == Ok("1"))
}

/// Writes a file beside and renames it over, off the UI thread; the newest content wins.
fn write_file(path: PathBuf, bytes: Vec<u8>) {
    static PENDING: Mutex<Option<(PathBuf, Vec<u8>)>> = Mutex::new(None);
    static WRITER: Mutex<()> = Mutex::new(());
    if !writes() {
        return;
    }
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some((path, bytes));
    }
    std::thread::spawn(|| {
        let Ok(_writing) = WRITER.lock() else {
            return;
        };
        let Some((path, bytes)) = PENDING.lock().ok().and_then(|mut pending| pending.take()) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let temporary = path.with_extension("json.tmp");
        if std::fs::write(&temporary, bytes).is_ok() {
            let _ = std::fs::rename(&temporary, &path);
        }
    });
}

/// A message seen, as JSON (`cacheSentMessages`), and whether only this cache saw it (`ourCache`).
#[derive(Default)]
struct Cache {
    order: VecDeque<Id>,
    messages: HashMap<Id, (Value, bool)>,
}

impl Cache {
    /// `LimitedMap.set`: past `cacheLimit` the oldest entry goes.
    fn set(&mut self, id: Id, message: Value, ours: bool, limit: usize) {
        if !self.messages.contains_key(&id) {
            if limit > 0
                && self.messages.len() >= limit
                && let Some(oldest) = self.order.pop_front()
            {
                self.messages.remove(&oldest);
            }
            self.order.push_back(id);
        }
        self.messages.insert(id, (message, ours));
    }
}

/// The open log view.
struct View {
    tab: Status,
    filter: Entity<Editor>,
    _typing: Subscription,
    sort_newest: bool,
    shown: usize,
    /// Clear Visible Logs (with the count) or Clear All Logs asks first.
    confirm: Option<Option<usize>>,
    /// The right-click menu: the record and where it opened, in window coordinates.
    menu: Option<(Id, gpui::Point<gpui::Pixels>)>,
}

#[derive(Default)]
struct Kept {
    /// The log once read (`MessageLoggerIDB`), by message id.
    records: Option<HashMap<Id, Record>>,
    cache: Cache,
    /// The deletes `shouldIgnore` let go, for MessageLogger's keep decision of the same dispatch.
    ignored: HashSet<Id>,
    last_cleanup: Option<Instant>,
    /// Attachments being fetched to be saved: the message and the file name.
    saving: HashMap<Id, (Id, String, bool)>,
    view: Option<View>,
    /// The card's Clear Logs question.
    clear_question: bool,
}

thread_local! {
    static KEPT: RefCell<Kept> = RefCell::new(Kept::default());
}

/// `initIDB` and the start: the log read from its file once (emptied with `clearLogsOnRestart`).
fn with_records<R>(run: impl FnOnce(&mut HashMap<Id, Record>) -> R) -> R {
    KEPT.with(|kept| {
        let mut kept = kept.borrow_mut();
        let records = kept.records.get_or_insert_with(|| {
            let clear = current().is_some_and(|settings| settings.clear_on_restart);
            if cfg!(test) || clear {
                if clear {
                    write_records(&HashMap::new());
                }
                return HashMap::new();
            }
            data_dir()
                .and_then(|dir| std::fs::read(dir.join(LOG_FILE)).ok())
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .map(|value| read_records(&value))
                .unwrap_or_default()
        });
        run(records)
    })
}

/// Records from a log file or an export (`{"messages": [...]}`, or a list).
fn read_records(value: &Value) -> HashMap<Id, Record> {
    value
        .get("messages")
        .unwrap_or(value)
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Record::from_value)
        .map(|record| (record.id, record))
        .collect()
}

fn records_json(records: &HashMap<Id, Record>) -> Vec<u8> {
    let mut list: Vec<&Record> = records.values().collect();
    list.sort_by_key(|record| record.id);
    let messages: Vec<Value> = list.iter().map(|record| record.to_value()).collect();
    serde_json::to_vec_pretty(&json!({ "messages": messages })).unwrap_or_default()
}

fn write_records(records: &HashMap<Id, Record>) {
    if let Some(dir) = data_dir() {
        write_file(dir.join(LOG_FILE), records_json(records));
    }
}

fn save() {
    with_records(|records| write_records(records));
}

/// A deleted message the plugin ignored in this dispatch: MessageLogger lets it go too.
pub(in crate::ui) fn message_logger_enhanced_ignored(message: Id) -> bool {
    KEPT.with(|kept| kept.borrow().ignored.contains(&message))
}

/// After a dispatch: the ignored deletes are forgotten.
pub(in crate::ui) fn message_logger_enhanced_after() {
    KEPT.with(|kept| kept.borrow_mut().ignored.clear());
}

/// The edits of a record as MessageLogger keeps them (`editHistory`).
fn edits_of(message: &Value) -> Vec<super::message_logger::Edit> {
    message["editHistory"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|edit| {
            Some(super::message_logger::Edit {
                at: DateTime::parse_from_rfc3339(edit["timestamp"].as_str()?)
                    .ok()?
                    .with_timezone(&Utc),
                content: edit["content"].as_str()?.to_owned(),
            })
        })
        .collect()
}

/// A record's message as the chat gets it back: its saved attachments drawn from their files (the picture's
/// address), their links still the ones Discord gave.
fn readded(record: &Record) -> Value {
    let mut message = record.message.clone();
    if let Some(attachments) = message["attachments"].as_array_mut() {
        for attachment in attachments {
            let picture = attachment["content_type"]
                .as_str()
                .is_some_and(|content| content.starts_with("image/"));
            if picture && let Some(name) = attachment["path"].as_str().map(str::to_owned) {
                attachment["proxy_url"] = Value::from(format!("{SAVED_SCHEME}{name}"));
            }
        }
    }
    message
}

/// A message the stores hold, as the JSON the log keeps, for one the cache did not see (the original falls back on
/// MessageStore): what the log view and a re-added message draw.
fn message_json(message: &Message) -> Value {
    let author = &message.author;
    json!({
        "id": message.id.to_string(),
        "channel_id": message.channel_id.to_string(),
        "type": message.kind,
        "content": message.content,
        "timestamp": message.timestamp.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "edited_timestamp": message.edited_at.map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        "flags": message.flags,
        "webhook_id": message.webhook_id.map(|id| id.to_string()),
        "mention_everyone": message.mention_everyone,
        "mentions": message.mention_user_ids.iter().map(|id| json!({"id": id.to_string()})).collect::<Vec<_>>(),
        "author": {
            "id": author.id.to_string(),
            "username": author.username,
            "global_name": author.global_name,
            "avatar": author.avatar,
            "discriminator": author.discriminator,
            "bot": author.bot,
        },
        "attachments": message.attachments.iter().map(|attachment| json!({
            "id": attachment.id.to_string(),
            "filename": attachment.filename,
            "url": attachment.url,
            "proxy_url": attachment.proxy_url,
            "content_type": attachment.content_type,
            "width": attachment.width,
            "height": attachment.height,
            "size": attachment.size,
        })).collect::<Vec<_>>(),
        "embeds": [],
    })
}

/// Shallow `{...cached, ...update}`: the update's fields over the known message.
fn merged(known: &Value, update: &Value) -> Value {
    let mut out = known.clone();
    if let (Some(out), Some(update)) = (out.as_object_mut(), update.as_object()) {
        for (key, value) in update {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl Root {
    /// What `shouldIgnore` reads of the stores about a message.
    fn mle_seen(&self, message: &Value, channel: Id, ours: bool, me: Option<Id>) -> Seen {
        let guild = rules::id_of(&message["guild_id"])
            .or_else(|| rules::id_of(&message["guildId"]))
            .or_else(|| self.guild_of_channel(channel));
        let category = guild.and_then(|guild| {
            self.stores
                .guild(guild)?
                .channels
                .iter()
                .find(|known| known.id == channel)?
                .parent_id
        });
        let now = Utc::now();
        let settings = &self.stores.guild_settings;
        Seen {
            channel,
            author: rules::id_of(&message["author"]["id"]),
            guild,
            flags: message["flags"].as_u64().unwrap_or(0),
            bot: message["author"]["bot"] == Value::Bool(true),
            ghost_pinged: rules::is_ghost_pinged(message, me),
            cached_by_us: ours,
            webhook: !message["webhook_id"].is_null() && message.get("webhook_id").is_some(),
            me,
            is_dm: self
                .stores
                .private_channels
                .iter()
                .any(|known| known.id == channel && known.kind == 1),
            current_channel: self.channel,
            guild_muted: guild.is_some_and(|guild| {
                settings.guilds.get(&guild).is_some_and(|own| {
                    dc_state::guild_settings::mute_active(own.muted, own.mute_end, now)
                })
            }),
            category_muted: guild
                .zip(category)
                .is_some_and(|(guild, category)| settings.channel_muted(guild, category, now)),
            channel_muted: guild.is_some_and(|guild| settings.channel_muted(guild, channel, now)),
        }
    }

    /// Before the stores see a dispatch (from MessageLogger's own look, `message_logger_before`): the cache, and the
    /// records of deletes and edits.
    pub(in crate::ui) fn message_logger_enhanced_before(&mut self, name: &str, data: &Value) {
        if !matches!(
            name,
            "MESSAGE_CREATE" | "MESSAGE_UPDATE" | "MESSAGE_DELETE" | "MESSAGE_DELETE_BULK"
        ) {
            return;
        }
        let Some(settings) = current() else {
            return;
        };
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let channel = rules::id_of(&data["channel_id"]);
        match name {
            "MESSAGE_CREATE" => {
                let (Some(channel), Some(id)) = (channel, rules::id_of(&data["id"])) else {
                    return;
                };
                // The original's MessageStore holds the open channels' messages; its own cache the rest.
                let open = self.stores.messages.contains_key(&channel);
                let guild = rules::id_of(&data["guild_id"]);
                let author = rules::id_of(&data["author"]["id"]);
                if !open
                    && guild.is_some()
                    && !rules::caches_server_message(
                        settings.ignore.cache_from_servers,
                        &settings.ignore.whitelisted,
                        [Some(channel), author, guild],
                    )
                {
                    return;
                }
                KEPT.with(|kept| {
                    kept.borrow_mut()
                        .cache
                        .set(id, data.clone(), !open, settings.cache_limit)
                });
            }
            "MESSAGE_UPDATE" => {
                let (Some(channel), Some(id)) = (channel, rules::id_of(&data["id"])) else {
                    return;
                };
                let known = KEPT.with(|kept| kept.borrow().cache.messages.get(&id).cloned());
                let Some((known, ours)) = known.or_else(|| {
                    self.message_logger_find(channel, id)
                        .map(|found| (message_json(found), false))
                }) else {
                    return;
                };
                let mut message = merged(&known, data);
                // Embeds arriving send an update without content: no edit.
                if let (Some(old), Some(new)) =
                    (known["content"].as_str(), data["content"].as_str())
                    && old != new
                {
                    let mut history = known["editHistory"].as_array().cloned().unwrap_or_default();
                    history.push(json!({"content": old, "timestamp": now_iso()}));
                    message["editHistory"] = Value::Array(history);
                }
                KEPT.with(|kept| {
                    kept.borrow_mut()
                        .cache
                        .set(id, message.clone(), ours, settings.cache_limit)
                });
                let seen = self.mle_seen(&message, channel, ours, me);
                if rules::should_ignore(&settings.ignore, &seen)
                    || message["editHistory"]
                        .as_array()
                        .is_none_or(|history| history.is_empty())
                {
                    return;
                }
                self.mle_add(&settings, message, channel, Status::Edited);
            }
            _ => {
                let Some(channel) = channel else {
                    return;
                };
                let ids: Vec<Id> = if name == "MESSAGE_DELETE" {
                    rules::id_of(&data["id"]).into_iter().collect()
                } else {
                    data["ids"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(rules::id_of)
                        .collect()
                };
                // MessageLogger's own Clear (`mlDeleted`) is not a delete to log.
                if data["mlDeleted"] == Value::Bool(true) {
                    return;
                }
                for id in ids {
                    let known = KEPT.with(|kept| kept.borrow().cache.messages.get(&id).cloned());
                    let Some((mut message, _)) = known.or_else(|| {
                        self.message_logger_find(channel, id)
                            .map(|found| (message_json(found), false))
                    }) else {
                        continue;
                    };
                    message["deleted"] = Value::Bool(true);
                    let ours = !self
                        .stores
                        .messages
                        .get(&channel)
                        .is_some_and(|list| list.iter().any(|known| known.id == id));
                    let seen = self.mle_seen(&message, channel, ours, me);
                    if rules::should_ignore(&settings.ignore, &seen) {
                        KEPT.with(|kept| kept.borrow_mut().ignored.insert(id));
                        continue;
                    }
                    let status = if seen.ghost_pinged {
                        Status::GhostPinged
                    } else {
                        Status::Deleted
                    };
                    message["ghostPinged"] = Value::Bool(seen.ghost_pinged);
                    message["deletedTimestamp"] = Value::from(now_iso());
                    if settings.save_images && status == Status::Deleted {
                        self.mle_save_images(&settings, &message);
                    }
                    self.mle_add(&settings, message, channel, status);
                }
            }
        }
    }

    /// `addMessage`: the record is put (replacing an earlier one), then the time and count limits.
    fn mle_add(&mut self, settings: &Settings, mut message: Value, channel: Id, status: Status) {
        if !settings.save_messages {
            return;
        }
        let Some(id) = rules::id_of(&message["id"]) else {
            return;
        };
        if message["guildId"].is_null() {
            let guild =
                rules::id_of(&message["guild_id"]).or_else(|| self.guild_of_channel(channel));
            if let Some(guild) = guild {
                message["guildId"] = Value::from(guild.to_string());
            }
        }
        if message["channel_id"].is_null() {
            message["channel_id"] = Value::from(channel.to_string());
        }
        let current = self.channel;
        let guild_channels: HashSet<Id> = self
            .stores
            .guilds
            .iter()
            .flat_map(|guild| guild.channels.iter().map(|known| known.id))
            .collect();
        let cleanup = settings.cleanup_minutes > 0.
            && KEPT.with(|kept| {
                let mut kept = kept.borrow_mut();
                let due = kept
                    .last_cleanup
                    .is_none_or(|at| at.elapsed() > CLEANUP_COOLDOWN);
                if due {
                    kept.last_cleanup = Some(Instant::now());
                }
                due
            });
        let limit = settings.message_limit;
        let preserve = settings.preserve_current;
        let minutes = settings.cleanup_minutes;
        with_records(|records| {
            records.insert(
                id,
                Record {
                    id,
                    channel,
                    status,
                    message,
                },
            );
            if cleanup {
                let cutoff = (Utc::now() - chrono::Duration::seconds((minutes * 60.) as i64))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                records.retain(|_, record| {
                    !(record.timestamp() <= cutoff.as_str()
                        && guild_channels.contains(&record.channel)
                        && !(preserve && Some(record.channel) == current))
                });
            }
            if limit > 0 && records.len() > limit {
                let over = records.len() - limit;
                if over < limit {
                    let mut oldest: Vec<(String, Id)> = records
                        .values()
                        .map(|record| (record.timestamp().to_owned(), record.id))
                        .collect();
                    oldest.sort();
                    for (_, id) in oldest.into_iter().take(over) {
                        records.remove(&id);
                    }
                }
            }
        });
        save();
    }

    /// `cacheMessageImages`: each attachment fit to save is fetched (the proxy address first, as it lasts longer).
    fn mle_save_images(&mut self, settings: &Settings, message: &Value) {
        let Some(id) = rules::id_of(&message["id"]) else {
            return;
        };
        if let Some(dir) = images_dir(settings) {
            rules::set_images_dir(dir);
        }
        for attachment in message["attachments"].as_array().into_iter().flatten() {
            if !rules::good_to_cache(attachment, settings.size_limit, &settings.extensions) {
                continue;
            }
            let Some(attachment_id) = rules::id_of(&attachment["id"]) else {
                continue;
            };
            let name = format!("{attachment_id}{}", rules::file_extension(attachment));
            let charset = attachment["content_type"]
                .as_str()
                .is_some_and(|content| content.contains(';'));
            let url = if charset {
                attachment["url"].as_str()
            } else {
                attachment["proxy_url"]
                    .as_str()
                    .or(attachment["url"].as_str())
            };
            let Some(url) = url else {
                continue;
            };
            KEPT.with(|kept| {
                kept.borrow_mut()
                    .saving
                    .insert(attachment_id, (id, name, charset))
            });
            let _ = self.requests.send(crate::net::Request::Image {
                key: format!("{IMAGE_PREFIX}{attachment_id}"),
                url: url.to_owned(),
            });
        }
    }

    /// A saved attachment's bytes arrived: the file is written and the record points at it.
    pub(in crate::ui) fn message_logger_enhanced_image(
        &mut self,
        key: &str,
        bytes: Option<Vec<u8>>,
    ) {
        let Some(attachment) = key
            .strip_prefix(IMAGE_PREFIX)
            .and_then(|id| id.parse::<Id>().ok())
        else {
            return;
        };
        let Some((message, name, tried_old)) =
            KEPT.with(|kept| kept.borrow_mut().saving.remove(&attachment))
        else {
            return;
        };
        let Some(bytes) = bytes else {
            // `useOldUrl`: the address the message had, once.
            if !tried_old
                && let Some(url) = with_records(|records| {
                    records.get(&message).and_then(|record| {
                        record.message["attachments"]
                            .as_array()?
                            .iter()
                            .find(|known| rules::id_of(&known["id"]) == Some(attachment))?["url"]
                            .as_str()
                            .map(str::to_owned)
                    })
                })
            {
                KEPT.with(|kept| {
                    kept.borrow_mut()
                        .saving
                        .insert(attachment, (message, name, true))
                });
                let _ = self.requests.send(crate::net::Request::Image {
                    key: key.to_owned(),
                    url,
                });
            }
            return;
        };
        let Some(dir) = current().as_ref().and_then(images_dir) else {
            return;
        };
        let path = dir.join(&name);
        if writes() {
            std::thread::spawn(move || {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(path, bytes);
            });
        }
        let changed = with_records(|records| {
            let Some(record) = records.get_mut(&message) else {
                return false;
            };
            let Some(list) = record.message["attachments"].as_array_mut() else {
                return false;
            };
            for known in list {
                if rules::id_of(&known["id"]) == Some(attachment) {
                    known["oldUrl"] = known["url"].clone();
                    known["oldProxyUrl"] = known["proxy_url"].clone();
                    known["fileExtension"] =
                        Value::from(name.rsplit_once('.').map_or("", |(_, ext)| ext));
                    known["path"] = Value::from(name.clone());
                    return true;
                }
            }
            false
        });
        if changed {
            save();
        }
    }

    /// A loaded page (`LOAD_MESSAGES_SUCCESS`): its messages join the cache; with MessageLogger on, the logged
    /// deleted messages of its stretch come back and its messages get their logged edits (`processMessageFetch`,
    /// `coolReAddDeletedMessages`). `start`: the page reaches the newest message; `end`: the first.
    pub(in crate::ui) fn message_logger_enhanced_page(
        &self,
        channel: Id,
        start: bool,
        end: bool,
        mut page: Value,
    ) -> Value {
        let Some(settings) = current() else {
            return page;
        };
        let Some(list) = page.as_array_mut() else {
            return page;
        };
        KEPT.with(|kept| {
            let mut kept = kept.borrow_mut();
            for message in list.iter() {
                if let Some(id) = rules::id_of(&message["id"]) {
                    kept.cache
                        .set(id, message.clone(), false, settings.cache_limit);
                }
            }
        });
        if !super::message_logger_on() {
            return page;
        }
        if let Some(dir) = images_dir(&settings) {
            rules::set_images_dir(dir);
        }
        let (deleted, edited): (Vec<Record>, Vec<Record>) = with_records(|records| {
            records
                .values()
                .filter(|record| record.channel == channel)
                .cloned()
                .partition(|record| record.status != Status::Edited)
        });
        let back: Vec<Value> = deleted.iter().map(readded).collect();
        let before: HashSet<Id> = list.iter().filter_map(|m| rules::id_of(&m["id"])).collect();
        rules::re_add(list, &back, start, end);
        for record in &deleted {
            if !before.contains(&record.id)
                && list
                    .iter()
                    .any(|m| rules::id_of(&m["id"]) == Some(record.id))
            {
                let attachments: Vec<Id> = record.message["attachments"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|attachment| rules::id_of(&attachment["id"]))
                    .collect();
                super::message_logger::message_logger_restore(
                    record.id,
                    channel,
                    true,
                    &attachments,
                    edits_of(&record.message),
                );
            }
        }
        for record in &edited {
            if before.contains(&record.id) {
                super::message_logger::message_logger_restore(
                    record.id,
                    channel,
                    false,
                    &[],
                    edits_of(&record.message),
                );
            }
        }
        page
    }

    /// MessageLogger's Clear Message History with `permanentlyRemoveLogByDefault`, and the menu's Remove Message:
    /// the record goes for good.
    pub(in crate::ui) fn message_logger_enhanced_clear(&mut self, message: Id, always: bool) {
        if !always && !current().is_some_and(|settings| settings.permanently_remove) {
            return;
        }
        if with_records(|records| records.remove(&message).is_some()) {
            save();
        }
    }

    /// The title bar's Open Logs button (`headerBarButton`, `ShowLogsButton`).
    pub(in crate::ui) fn message_logger_enhanced_button(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !show_logs_button() {
            return None;
        }
        let open = KEPT.with(|kept| kept.borrow().view.is_some());
        let button = div()
            .id("mle-open-logs")
            .size(px(24.))
            .mr(px(12.))
            .rounded(px(6.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .hover(|style| style.bg(color("--background-mod-normal")))
            .child(icon(
                "plugin_history",
                18.,
                if open {
                    "--interactive-icon-active"
                } else {
                    "--icon-muted"
                },
            ))
            .on_click(cx.listener(|this, _, _, cx| this.message_logger_enhanced_open(None, cx)));
        Some(
            self.with_tip(
                "mle-open-logs-tip",
                super::super::tooltip::Tip::text("Open Logs", super::super::tooltip::Side::Bottom),
                button,
                cx,
            )
            .into_any_element(),
        )
    }

    /// `openLogModal(initialQuery)`.
    pub(in crate::ui) fn message_logger_enhanced_open(
        &mut self,
        query: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(settings) = current() else {
            return;
        };
        let filter = cx.new(|cx| {
            let mut editor = Editor::new(super::super::editor_style(), cx);
            editor.set_single_line(true);
            editor.set_text(query.as_deref().unwrap_or(""));
            editor
        });
        let typing = cx.subscribe(&filter, |_, _, _: &EditorEvent, cx| {
            KEPT.with(|kept| {
                if let Some(view) = kept.borrow_mut().view.as_mut() {
                    view.shown = current().map_or(100, |settings| settings.at_once);
                }
            });
            cx.notify();
        });
        let sort_newest =
            crate::plugins::with(|state| state.value(ID, "sortNewest") == Value::Bool(true));
        KEPT.with(|kept| {
            kept.borrow_mut().view = Some(View {
                tab: Status::Deleted,
                filter,
                _typing: typing,
                sort_newest,
                shown: settings.at_once,
                confirm: None,
                menu: None,
            })
        });
        self.menu = None;
        self.context.open = None;
        // The logged messages' pictures (saved files where there are any), as the chat asks for its own.
        let wanted: Vec<(String, String)> = with_records(|records| {
            records
                .values()
                .filter_map(|record| Message::from_value(&readded(record)))
                .flat_map(|message| super::super::media_image::wanted(&message, false, false))
                .collect()
        });
        for (key, url) in wanted {
            self.request_image(key, url);
        }
        cx.notify();
    }

    fn mle_place(&self, message: &Value) -> Place {
        let channel = rules::id_of(&message["channel_id"]);
        let guild = rules::id_of(&message["guildId"])
            .or_else(|| rules::id_of(&message["guild_id"]))
            .or_else(|| channel.and_then(|channel| self.guild_of_channel(channel)));
        let known = guild.and_then(|guild| self.stores.guild(guild));
        Place {
            channel_name: known.and_then(|known| {
                known
                    .channels
                    .iter()
                    .find(|known| Some(known.id) == channel)
                    .map(|found| found.name.clone())
            }),
            guild,
            guild_name: known.map(|known| known.name.clone()),
        }
    }

    /// The records the view shows: the tab's, filtered, sorted, at most `shown` without a filter.
    fn mle_visible(&self, view: &Shown, query: &str) -> (Vec<Record>, usize) {
        let (filters, words) = rules::query(query);
        let mut list: Vec<Record> = with_records(|records| {
            records
                .values()
                .filter(|record| record.status == view.tab)
                .cloned()
                .collect()
        });
        list.sort_by(|a, b| {
            let order = a.timestamp().cmp(b.timestamp()).then(a.id.cmp(&b.id));
            if view.sort_newest {
                order.reverse()
            } else {
                order
            }
        });
        let total = list.len();
        if query.is_empty() {
            list.truncate(view.shown);
            (list, total)
        } else {
            list.retain(|record| {
                rules::passes_query(
                    &filters,
                    &words,
                    &record.message,
                    &self.mle_place(&record.message),
                )
            });
            // `statusTotal: Number.MAX_SAFE_INTEGER` while filtering: no Load More.
            let shown = list.len();
            (list, shown)
        }
    }

    /// "From …" (`ShowWhereMessageIsFrom`).
    fn mle_from(&self, message: &Value) -> Option<String> {
        let channel = rules::id_of(&message["channel_id"])?;
        if let Some(private) = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
        {
            return if private.kind == 3 {
                private
                    .name
                    .clone()
                    .map(|name| format!("From {name} Group DM"))
            } else {
                message["author"]["username"]
                    .as_str()
                    .map(|name| format!("From {name}'s DMs"))
            };
        }
        let place = self.mle_place(message);
        Some(format!(
            "From {} in {}",
            place.channel_name?, place.guild_name?
        ))
    }

    /// One logged message as the log draws it (`LMessage`, `MessagePreview`): the head on a group's first one, the
    /// earlier versions of an edited one dimmed above, a deleted one in MessageLogger's red.
    fn mle_message(
        &self,
        record: &Record,
        head: bool,
        show_where: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(message) = Message::from_value(&readded(record)) else {
            return div().into_any_element();
        };
        let id = record.id;
        let deleted = record.status != Status::Edited;
        let edits = edits_of(&record.message);
        let body = {
            let _scope = deleted.then(super::message_logger::message_logger_red_scope);
            div()
                .flex()
                .flex_col()
                .children(edits.iter().map(|edit| {
                    div()
                        .opacity(0.5)
                        .text_color(color("--text-default"))
                        .child(format!("{} (edited)", edit.content))
                }))
                .child(self.content(&message))
                .children(self.attachments(&message, cx))
                .into_any_element()
        };
        let row = if head {
            self.inbox_message_with(&message, body, cx)
        } else {
            div()
                .pl(px(72.))
                .pr(px(16.))
                .py(px(2.))
                .child(body)
                .into_any_element()
        };
        let from = show_where.then(|| self.mle_from(&record.message)).flatten();
        div()
            .id(SharedString::from(format!("mle-log-{id}")))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |_, event: &gpui::MouseDownEvent, _, cx| {
                    KEPT.with(|kept| {
                        if let Some(view) = kept.borrow_mut().view.as_mut() {
                            view.menu = Some((id, event.position));
                        }
                    });
                    cx.notify();
                }),
            )
            .child(row)
            .children(from.map(|from| {
                div()
                    .ml(px(84.))
                    .text_size(px(14.))
                    .text_color(if deleted {
                        color("--status-danger")
                    } else {
                        color("--text-muted")
                    })
                    .child(from)
            }))
            .into_any_element()
    }

    /// The log view (`LogsModal`, size lg).
    pub(in crate::ui) fn message_logger_enhanced_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if !KEPT.with(|kept| kept.borrow().view.is_some()) {
            return Vec::new();
        }
        let Some(settings) = current() else {
            return Vec::new();
        };
        let Some((shown, filter, confirm, menu)) = KEPT.with(|kept| {
            let kept = kept.borrow();
            let view = kept.view.as_ref()?;
            Some((
                Shown {
                    tab: view.tab,
                    sort_newest: view.sort_newest,
                    shown: view.shown,
                },
                view.filter.clone(),
                view.confirm,
                view.menu,
            ))
        }) else {
            return Vec::new();
        };
        let (tab, sort_newest) = (shown.tab, shown.sort_newest);
        let query = filter.read(cx).text().to_owned();
        let (visible, status_total) = self.mle_visible(&shown, &query);
        let total = with_records(|records| records.len());
        let tab_item = |status: Status, label: &'static str, cx: &mut Context<Self>| {
            let selected = status == tab;
            div()
                .id(SharedString::from(format!("mle-tab-{}", status.as_str())))
                .flex_1()
                .pb(px(16.))
                .flex()
                .justify_center()
                .cursor_pointer()
                .border_b_2()
                .border_color(if selected {
                    color("--control-brand-foreground")
                } else {
                    gpui::rgba(0)
                })
                .text_size(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(if selected {
                    color("--interactive-text-active")
                } else {
                    color("--interactive-icon-default")
                })
                .child(label)
                .on_click(cx.listener(move |_, _, _, cx| {
                    KEPT.with(|kept| {
                        if let Some(view) = kept.borrow_mut().view.as_mut() {
                            view.tab = status;
                            view.shown = current().map_or(100, |settings| settings.at_once);
                            view.menu = None;
                        }
                    });
                    cx.notify();
                }))
        };
        let header = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .child(tab_item(Status::Deleted, "Deleted", cx))
                    .child(tab_item(Status::Edited, "Edited", cx))
                    .child(tab_item(Status::GhostPinged, "Ghost Pinged", cx)),
            )
            .child(
                div()
                    .relative()
                    .h(px(40.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .text_size(px(16.))
                    .text_color(color("--text-default"))
                    .child(div().flex_1().child(filter.clone()))
                    .when(query.is_empty(), |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(px(12.))
                                .text_color(color("--input-placeholder-text-default"))
                                .child("Filter Messages"),
                        )
                    }),
            );
        let tab_name = match tab {
            Status::Deleted => "Deleted",
            Status::Edited => "Edited",
            Status::GhostPinged => "Ghost Pinged",
        };
        let text = |line: String| {
            div()
                .text_size(px(20.))
                .line_height(px(24.))
                .text_color(color("--text-default"))
                .child(line)
        };
        let mut content = div().flex().flex_col().py(px(16.));
        if total == 0 {
            // `EmptyLogs`.
            content = content
                .items_center()
                .gap(px(12.))
                .child(text("Empty eh".to_owned()));
            if query.is_empty() {
                content = content.child(
                    div()
                        .text_size(px(14.))
                        .text_color(color("--text-muted"))
                        .child("Logs are kept on this computer. Import Logs reads a log file; importing replaces the current log."),
                ).child(small_button("mle-empty-import", "Import Logs", Tone::Primary, cx, |this, _, cx| {
                    this.message_logger_enhanced_import(cx);
                }));
            }
        } else if visible.is_empty() {
            // `NoResults`.
            let (next, last) = match tab {
                Status::Deleted => ("Edited", "Ghost Pinged"),
                Status::Edited => ("Ghost Pinged", "Deleted"),
                Status::GhostPinged => ("Deleted", "Edited"),
            };
            content = content
                .items_center()
                .child(text(format!("No results in {tab_name}.")))
                .child(text(format!("Maybe try {next} or {last}")));
        } else {
            for (index, record) in visible.iter().enumerate() {
                let head = rules::is_group_start(
                    &record.message,
                    index.checked_sub(1).map(|before| &visible[before].message),
                );
                content = content.child(self.mle_message(record, head, settings.show_where, cx));
            }
            if visible.len() < status_total && visible.len() >= settings.at_once {
                content = content.child(div().mt(px(16.)).child(small_button(
                    "mle-load-more",
                    "Load More",
                    Tone::Primary,
                    cx,
                    |_, _, cx| {
                        KEPT.with(|kept| {
                            if let Some(view) = kept.borrow_mut().view.as_mut() {
                                view.shown += current().map_or(100, |settings| settings.at_once);
                            }
                        });
                        cx.notify();
                    },
                )));
            }
        }
        let visible_ids: Vec<Id> = visible.iter().map(|record| record.id).collect();
        let count = visible_ids.len();
        let body = div()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("mle-log-list")
                    .h(px(420.))
                    .overflow_y_scroll()
                    .child(content),
            )
            .into_any_element();
        let clear_visible = modal::button(
            "mle-clear-visible",
            "Clear Visible Logs",
            Tone::CriticalSecondary,
            move |_, _, cx| {
                if count > 0 {
                    KEPT.with(|kept| {
                        if let Some(view) = kept.borrow_mut().view.as_mut() {
                            view.confirm = Some(Some(count));
                        }
                    });
                }
                cx.notify();
            },
        );
        let spec = modal::Spec {
            id: "mle-logs",
            title: "Message Logger".into(),
            subtitle: None,
            body: Some(body),
            buttons: vec![
                modal::button(
                    "mle-sort",
                    if sort_newest {
                        "Sort Oldest First"
                    } else {
                        "Sort Newest First"
                    },
                    Tone::Secondary,
                    |_, _, cx| {
                        let newest = KEPT.with(|kept| {
                            let mut kept = kept.borrow_mut();
                            let view = kept.view.as_mut()?;
                            view.sort_newest = !view.sort_newest;
                            Some(view.sort_newest)
                        });
                        if let Some(newest) = newest {
                            crate::plugins::update(|state| {
                                state.set_value(ID, "sortNewest", &Value::Bool(newest));
                            });
                        }
                        cx.notify();
                    },
                ),
                clear_visible,
                modal::button(
                    "mle-clear-all",
                    "Clear All Logs",
                    Tone::Critical,
                    |_, _, cx| {
                        KEPT.with(|kept| {
                            if let Some(view) = kept.borrow_mut().view.as_mut() {
                                view.confirm = Some(None);
                            }
                        });
                        cx.notify();
                    },
                ),
            ],
            close: Box::new(|_: &mut Root, _, cx| {
                KEPT.with(|kept| kept.borrow_mut().view = None);
                cx.notify();
            }),
        };
        let mut layers = vec![modal::render_xl(spec, cx)];
        if let Some((record, at)) = menu {
            layers.push(self.mle_row_menu(record, at, cx));
        }
        if let Some(which) = confirm {
            let ids = visible_ids.clone();
            let spec = modal::Spec {
                id: "mle-clear-confirm",
                title: "Clear Logs".into(),
                subtitle: None,
                body: Some(
                    div()
                        .text_size(px(16.))
                        .text_color(color("--text-default"))
                        .child(match which {
                            Some(count) => format!("Are you sure you want to clear {count} logs"),
                            None => "Are you sure you want to clear all the logs".to_owned(),
                        })
                        .into_any_element(),
                ),
                buttons: vec![
                    modal::button("mle-clear-cancel", "Cancel", Tone::Secondary, |_, _, cx| {
                        KEPT.with(|kept| {
                            if let Some(view) = kept.borrow_mut().view.as_mut() {
                                view.confirm = None;
                            }
                        });
                        cx.notify();
                    }),
                    modal::button(
                        "mle-clear-yes",
                        "Clear",
                        Tone::Critical,
                        move |this, _, cx| {
                            match which {
                                Some(_) => {
                                    with_records(|records| {
                                        for id in &ids {
                                            records.remove(id);
                                        }
                                    });
                                    save();
                                }
                                None => this.message_logger_enhanced_clear_all(true, cx),
                            }
                            KEPT.with(|kept| {
                                if let Some(view) = kept.borrow_mut().view.as_mut() {
                                    view.confirm = None;
                                }
                            });
                            cx.notify();
                        },
                    ),
                ],
                close: Box::new(|_: &mut Root, _, cx| {
                    KEPT.with(|kept| {
                        if let Some(view) = kept.borrow_mut().view.as_mut() {
                            view.confirm = None;
                        }
                    });
                    cx.notify();
                }),
            };
            layers.push(modal::render_small_over_popouts(spec, cx));
        }
        layers
    }

    /// A logged message's right-click menu in the view.
    fn mle_row_menu(
        &self,
        record: Id,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let message =
            with_records(|records| records.get(&record).map(|record| record.message.clone()))
                .unwrap_or_default();
        let text = |key: &str| message[key].as_str().unwrap_or_default().to_owned();
        let channel = text("channel_id");
        let author = message["author"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let guild = message["guildId"].as_str().map(str::to_owned);
        let mut rows: Vec<(&'static str, &'static str, MenuRun)> = vec![
            ("mle-menu-jump", "Jump To Message", MenuRun::Jump),
            ("mle-menu-profile", "Open user profile", MenuRun::Profile),
            (
                "mle-menu-content",
                "Copy Content",
                MenuRun::Copy(text("content")),
            ),
            ("mle-menu-user", "Copy User ID", MenuRun::Copy(author)),
            (
                "mle-menu-message",
                "Copy Message ID",
                MenuRun::Copy(text("id")),
            ),
            (
                "mle-menu-channel",
                "Copy Channel ID",
                MenuRun::Copy(channel),
            ),
        ];
        if let Some(guild) = guild {
            rows.push(("mle-menu-guild", "Copy Server ID", MenuRun::Copy(guild)));
        }
        rows.push(("mle-menu-delete", "Delete Log", MenuRun::Delete));
        let mut menu = div()
            .id("mle-row-menu")
            .occlude()
            .w(px(220.))
            .p(px(6.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .shadow(vec![super::super::shadow_high()])
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|_, _, _, cx| {
                KEPT.with(|kept| {
                    if let Some(view) = kept.borrow_mut().view.as_mut() {
                        view.menu = None;
                    }
                });
                cx.notify();
            }));
        for (key, label, run) in rows {
            let danger = matches!(run, MenuRun::Delete);
            let message = message.clone();
            menu = menu.child(
                div()
                    .id(key)
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(if danger {
                        color("--text-feedback-critical")
                    } else {
                        color("--interactive-text-default")
                    })
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        KEPT.with(|kept| {
                            if let Some(view) = kept.borrow_mut().view.as_mut() {
                                view.menu = None;
                            }
                        });
                        match &run {
                            MenuRun::Copy(text) => {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                this.show_success_toast("Copied to clipboard!".to_owned(), cx);
                            }
                            MenuRun::Delete => {
                                if with_records(|records| records.remove(&record).is_some()) {
                                    save();
                                }
                            }
                            MenuRun::Jump | MenuRun::Profile => {
                                // `closeAllModals`.
                                KEPT.with(|kept| kept.borrow_mut().view = None);
                                if let Some(found) = Message::from_value(&message) {
                                    if matches!(run, MenuRun::Jump) {
                                        this.jump_to_message_checked(&found, cx);
                                    } else {
                                        this.open_full_profile(found.author.id, window, cx);
                                    }
                                }
                            }
                        }
                        cx.notify();
                    })),
            );
        }
        gpui::deferred(div().absolute().left(at.x).top(at.y).child(menu))
            .with_priority(5)
            .into_any_element()
    }

    /// `clearLogs`: the log and the cache, with the toast.
    fn message_logger_enhanced_clear_all(&mut self, toast: bool, cx: &mut Context<Self>) {
        with_records(HashMap::clear);
        KEPT.with(|kept| kept.borrow_mut().cache = Cache::default());
        save();
        if toast {
            self.show_success_toast("Cleared message log database and cache.".to_owned(), cx);
        }
    }

    /// `importLogs`: a log file chosen, read off the UI thread, replacing the log.
    fn message_logger_enhanced_import(&mut self, cx: &mut Context<Self>) {
        let Some(settings) = current() else {
            return;
        };
        let _ = &settings.logs_dir;
        let options = gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import Logs".into()),
        };
        let picked = super::super::native_dialog::open_paths(
            options,
            super::super::native_dialog::JSON_FILTERS,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = cx
                .background_executor()
                .spawn(async move {
                    std::fs::read(path)
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                        .map(|value| read_records(&value))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match read {
                    Some(records) if !records.is_empty() => {
                        let count = records.len();
                        with_records(|known| *known = records);
                        save();
                        this.show_success_toast(format!("Successfully imported {count} logs"), cx);
                    }
                    _ => this.put_app_toast("No messages found in log file".to_owned(), false, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// `exportLogs`: every record into `message-logger-logs-idb.json` where the save dialog says.
    fn message_logger_enhanced_export(&mut self, cx: &mut Context<Self>) {
        let bytes = with_records(|records| records_json(records));
        let directory = current()
            .filter(|settings| !settings.logs_dir.is_empty())
            .map(|settings| PathBuf::from(settings.logs_dir))
            .or_else(data_dir)
            .unwrap_or_default();
        let picked = super::super::native_dialog::save_path(
            &directory,
            Some(EXPORT_NAME),
            super::super::native_dialog::JSON_FILTERS,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let written = cx
                .background_executor()
                .spawn(async move { std::fs::write(path, bytes).is_ok() })
                .await;
            let _ = this.update(cx, |this, cx| {
                if written {
                    this.show_success_toast("Successfully exported logs".to_owned(), cx);
                } else {
                    this.put_app_toast(
                        "Error exporting logs. Check the console for more information".to_owned(),
                        false,
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    /// `chooseDir`: Browse for the image or logs folder.
    fn message_logger_enhanced_choose_dir(&mut self, key: &'static str, cx: &mut Context<Self>) {
        let options = gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select".into()),
        };
        let picked = super::super::native_dialog::open_paths(
            options,
            super::super::native_dialog::ALL_FILTERS,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                crate::plugins::update(|state| {
                    state.set_value(ID, key, &Value::from(dir.to_string_lossy().into_owned()));
                });
                let message = if key == "imageCacheDir" {
                    "Successfully updated Image Cache Dir"
                } else {
                    "Successfully updated Logs Dir"
                };
                this.show_success_toast(message.to_owned(), cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The card's own section: Open Logs, Clear Logs, Import Logs, Export Logs, the two folders and Open Image Cache
    /// Folder.
    pub(in crate::ui) fn message_logger_enhanced_settings(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(settings) = current() else {
            return div()
                .text_size(px(14.))
                .text_color(color("--text-muted"))
                .child("Turn the plugin on to open, clear, import or export its logs.")
                .into_any_element();
        };
        let folder = |key: &'static str,
                      title: &'static str,
                      path: Option<PathBuf>,
                      cx: &mut Context<Self>| {
            let shown = path
                .map(|path| {
                    let parts: Vec<String> = path
                        .components()
                        .map(|part| part.as_os_str().to_string_lossy().into_owned())
                        .collect();
                    parts[parts.len().saturating_sub(2)..].join("\\")
                })
                .unwrap_or_else(|| "Choose Folder".to_owned());
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(title),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .child(
                            div()
                                .px(px(10.))
                                .py(px(10.))
                                .text_color(color("--text-default"))
                                .child(shown),
                        )
                        .child(div().m(px(6.)).child(small_button(
                            if key == "imageCacheDir" {
                                "mle-browse-images"
                            } else {
                                "mle-browse-logs"
                            },
                            "Browse",
                            Tone::Primary,
                            cx,
                            move |this, _, cx| this.message_logger_enhanced_choose_dir(key, cx),
                        ))),
                )
        };
        let images = images_dir(&settings);
        let logs = if settings.logs_dir.is_empty() {
            data_dir()
        } else {
            Some(PathBuf::from(&settings.logs_dir))
        };
        let open_folder = images.clone();
        let question = KEPT.with(|kept| kept.borrow().clear_question);
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(folder(
                "imageCacheDir",
                "Select saved images directory",
                images,
                cx,
            ))
            .child(folder("logsDir", "Select logs directory", logs, cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(small_button(
                        "mle-open-logs-card",
                        "Open Logs",
                        Tone::Primary,
                        cx,
                        |this, _, cx| {
                            this.message_logger_enhanced_open(None, cx);
                        },
                    ))
                    .child(small_button(
                        "mle-import",
                        "Import Logs",
                        Tone::Primary,
                        cx,
                        |this, _, cx| {
                            this.message_logger_enhanced_import(cx);
                        },
                    ))
                    .child(small_button(
                        "mle-export",
                        "Export Logs",
                        Tone::Primary,
                        cx,
                        |this, _, cx| {
                            this.message_logger_enhanced_export(cx);
                        },
                    ))
                    .child(small_button(
                        "mle-open-folder",
                        "Open Image Cache Folder",
                        Tone::Primary,
                        cx,
                        move |_, _, cx| {
                            if let Some(dir) = &open_folder {
                                let _ = std::fs::create_dir_all(dir);
                                cx.open_url(&format!(
                                    "file:///{}",
                                    dir.to_string_lossy().replace('\\', "/")
                                ));
                            }
                        },
                    ))
                    .child(small_button(
                        "mle-clear-logs",
                        "Clear Logs",
                        Tone::Critical,
                        cx,
                        |_, _, cx| {
                            KEPT.with(|kept| kept.borrow_mut().clear_question = true);
                            cx.notify();
                        },
                    )),
            )
            .when(question, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .text_color(color("--text-default"))
                                .child("Are you sure you want to clear all logs?"),
                        )
                        .child(small_button(
                            "mle-clear-logs-cancel",
                            "Cancel",
                            Tone::Secondary,
                            cx,
                            |_, _, cx| {
                                KEPT.with(|kept| kept.borrow_mut().clear_question = false);
                                cx.notify();
                            },
                        ))
                        .child(small_button(
                            "mle-clear-logs-yes",
                            "Clear",
                            Tone::Critical,
                            cx,
                            |this, _, cx| {
                                KEPT.with(|kept| kept.borrow_mut().clear_question = false);
                                this.message_logger_enhanced_clear_all(false, cx);
                                this.show_success_toast("Cleared Logs".to_owned(), cx);
                            },
                        )),
                )
            })
            .into_any_element()
    }

    /// The ids a menu's Message Logger submenu is about (`idFunctions`): the folder's servers, the server, the user,
    /// the channel.
    fn mle_menu_ids(
        &self,
        user: Option<Id>,
        channel: Option<Id>,
        guild: Option<Id>,
    ) -> Vec<(MenuKind, Vec<Id>)> {
        let mut out = Vec::new();
        if let Some(guild) = guild {
            out.push((MenuKind::Server, vec![guild]));
        }
        if let Some(user) = user {
            out.push((MenuKind::User, vec![user]));
        }
        if let Some(channel) = channel {
            out.push((MenuKind::Channel, vec![channel]));
        }
        out
    }

    /// The rows of the Message Logger submenu: Open Logs, Open Logs For each kind, then each kind's list rows, then
    /// Remove Message (Permanent) for a logged message.
    pub(in crate::ui) fn message_logger_enhanced_rows(
        &self,
        user: Option<Id>,
        channel: Option<Id>,
        guild: Option<Id>,
        logged: Option<Id>,
    ) -> Vec<(String, SubAct)> {
        let Some(settings) = current() else {
            return Vec::new();
        };
        let mut rows = vec![("Open Logs".to_owned(), SubAct::Open(None))];
        let ids = self.mle_menu_ids(user, channel, guild);
        for (kind, list) in &ids {
            if let [one] = list.as_slice() {
                rows.push((
                    format!("Open Logs For {}", kind.name()),
                    SubAct::Open(Some(format!("{}:{one}", kind.name().to_lowercase()))),
                ));
            }
        }
        let listed = |list: &str, id: Id| list.split(',').any(|entry| entry == id.to_string());
        for (kind, list) in &ids {
            for black in [true, false] {
                let (own, other) = if black {
                    (&settings.ignore.blacklisted, &settings.ignore.whitelisted)
                } else {
                    (&settings.ignore.whitelisted, &settings.ignore.blacklisted)
                };
                let name = if black { "Blacklist" } else { "Whitelist" };
                let blocked = list.iter().all(|id| listed(own, *id));
                let opposite = list.iter().any(|id| listed(other, *id));
                let label = if opposite {
                    format!("Move {} to {name}", kind.name())
                } else if blocked {
                    format!("Remove {} From {name}", kind.name())
                } else {
                    format!("{name} {}", kind.name())
                };
                rows.push((label, SubAct::List(black, blocked, list.clone())));
            }
        }
        if let Some(message) = logged {
            let deleted = super::message_logger_deleted(message);
            let has_history = with_records(|records| records.contains_key(&message))
                || !super::message_logger::message_logger_edits(message).is_empty();
            if deleted || has_history {
                rows.push((
                    if deleted {
                        "Remove Message (Permanent)".to_owned()
                    } else {
                        "Remove Message History (Permanent)".to_owned()
                    },
                    SubAct::Remove(message),
                ));
            }
        }
        rows
    }

    /// A row of the Message Logger submenu ran.
    pub(in crate::ui) fn message_logger_enhanced_run(
        &mut self,
        act: SubAct,
        channel: Option<Id>,
        cx: &mut Context<Self>,
    ) {
        match act {
            SubAct::Open(query) => self.message_logger_enhanced_open(query, cx),
            SubAct::List(black, remove, ids) => {
                let (own, other) = if black {
                    ("blacklistedIds", "whitelistedIds")
                } else {
                    ("whitelistedIds", "blacklistedIds")
                };
                crate::plugins::update(|state| {
                    for id in &ids {
                        let id = id.to_string();
                        let edit = |state: &mut crate::plugins::State, key: &str, add: bool| {
                            let text = state.value(ID, key).as_str().unwrap_or_default().to_owned();
                            let mut items: Vec<String> = if text.is_empty() {
                                Vec::new()
                            } else {
                                text.split(',').map(str::to_owned).collect()
                            };
                            items.retain(|item| *item != id);
                            if add {
                                items.push(id.clone());
                            }
                            state.set_value(ID, key, &Value::from(items.join(",")));
                        };
                        if remove {
                            edit(state, own, false);
                        } else {
                            // `addToXAndRemoveFromOpposite`.
                            edit(state, other, false);
                            edit(state, own, true);
                        }
                    }
                });
            }
            SubAct::Remove(message) => {
                // `removeMessageAction`: the record, then the chat's own history of it.
                self.message_logger_enhanced_clear(message, true);
                if let Some(channel) = channel {
                    self.message_logger_clear(channel, message);
                }
            }
        }
        cx.notify();
    }
}

impl Root {
    /// The ids a target menu's submenu is about: the user, the channel and the server.
    fn mle_target(
        &self,
        target: super::super::context_menu::Target,
    ) -> Option<(Option<Id>, Option<Id>, Option<Id>)> {
        use super::super::context_menu::Target;
        match target {
            Target::Guild(guild) => Some((None, None, Some(guild))),
            Target::Channel(channel) | Target::Thread(channel) => {
                Some((None, Some(channel), self.guild_of_channel(channel)))
            }
            Target::Dm(channel) => {
                let user = self
                    .stores
                    .private_channels
                    .iter()
                    .find(|known| known.id == channel && known.kind == 1)
                    .and_then(|known| known.recipients.first().copied());
                Some((user, Some(channel), None))
            }
            Target::User { user, guild } => Some((Some(user), self.channel, guild)),
            _ => None,
        }
    }

    /// The channel, person, server and group DM menus end with "Message Logger" (`contextMenuPath`).
    pub(in crate::ui) fn with_message_logger_enhanced_row(
        &self,
        mut items: Vec<super::super::context_menu::Entry>,
        target: super::super::context_menu::Target,
    ) -> Vec<super::super::context_menu::Entry> {
        use super::super::context_menu::{Act, Entry, Row};
        if !on() || self.mle_target(target).is_none() {
            return items;
        }
        let mut row = Row::new(Act::PluginMessageLogger, "Message Logger");
        row.caret = true;
        if !items.is_empty() {
            items.push(Entry::Separator);
        }
        items.push(Entry::Row(row));
        items
    }

    fn mle_target_rows(&self, target: super::super::context_menu::Target) -> Vec<(String, SubAct)> {
        self.mle_target(target)
            .map(|(user, channel, guild)| {
                self.message_logger_enhanced_rows(user, channel, guild, None)
            })
            .unwrap_or_default()
    }

    /// The rows under a target menu's "Message Logger".
    pub(in crate::ui) fn message_logger_enhanced_sub_rows(
        &self,
        target: super::super::context_menu::Target,
    ) -> Vec<super::super::context_menu::SubRow> {
        self.mle_target_rows(target)
            .into_iter()
            .map(|(label, _)| super::super::context_menu::SubRow {
                label,
                ..Default::default()
            })
            .collect()
    }

    /// A row under a target menu's "Message Logger" ran.
    pub(in crate::ui) fn message_logger_enhanced_run_sub(
        &mut self,
        target: super::super::context_menu::Target,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let channel = self.mle_target(target).and_then(|(_, channel, _)| channel);
        if let Some((_, act)) = self.mle_target_rows(target).into_iter().nth(index) {
            self.close_all_context_menus();
            self.message_logger_enhanced_run(act, channel, cx);
        }
    }

    /// The rows of the message menu's "Message Logger" submenu.
    pub(in crate::ui) fn message_logger_enhanced_message_rows(
        &self,
        channel: Id,
        message: Id,
        cx: &mut Context<Self>,
    ) -> Vec<(AnyElement, f32)> {
        let author = self
            .stores
            .messages
            .get(&channel)
            .and_then(|list| list.iter().find(|known| known.id == message))
            .map(|known| known.author.id);
        let guild = self.guild_of_channel(channel);
        self.message_logger_enhanced_rows(author, Some(channel), guild, Some(message))
            .into_iter()
            .enumerate()
            .map(|(index, (label, act))| {
                (
                    self.submenu_item(
                        SharedString::from(format!("mle-sub-{index}")),
                        None,
                        label,
                        36.,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.submenu = None;
                        this.menu = None;
                        this.message_logger_enhanced_run(act.clone(), Some(channel), cx);
                    }))
                    .into_any_element(),
                    36.,
                )
            })
            .collect()
    }
}

/// The message menu ends with "Message Logger" while the plugin is on.
pub(in crate::ui) fn message_logger_enhanced_on() -> bool {
    on()
}

/// The log view is open (a modal for the key layer).
pub(in crate::ui) fn message_logger_enhanced_view_open() -> bool {
    KEPT.with(|kept| kept.borrow().view.is_some()) && on()
}

impl Root {
    /// Escape closes what the log view has open on top first: the row menu, the question, then the view.
    pub(in crate::ui) fn message_logger_enhanced_escape(&mut self, cx: &mut Context<Self>) -> bool {
        let closed = KEPT.with(|kept| {
            let mut kept = kept.borrow_mut();
            let Some(view) = kept.view.as_mut() else {
                return false;
            };
            if view.menu.take().is_none() && view.confirm.take().is_none() {
                kept.view = None;
            }
            true
        });
        if closed {
            cx.notify();
        }
        closed
    }
}

/// What the view's list is drawn from.
struct Shown {
    tab: Status,
    sort_newest: bool,
    shown: usize,
}

/// What a row of the view's menu does.
enum MenuRun {
    Jump,
    Profile,
    Copy(String),
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuKind {
    Server,
    User,
    Channel,
}

impl MenuKind {
    fn name(self) -> &'static str {
        match self {
            MenuKind::Server => "Server",
            MenuKind::User => "User",
            MenuKind::Channel => "Channel",
        }
    }
}

/// What a row of the Message Logger submenu does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum SubAct {
    Open(Option<String>),
    /// The blacklist (else the whitelist), whether the ids leave it, and the ids.
    List(bool, bool, Vec<Id>),
    Remove(Id),
}

fn small_button(
    id: &'static str,
    text: &'static str,
    tone: Tone,
    cx: &mut Context<Root>,
    run: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
) -> AnyElement {
    let (background, hover, foreground) = match tone {
        Tone::Critical => (
            "--control-critical-primary-background-default",
            "--control-critical-primary-background-hover",
            "--white",
        ),
        Tone::Secondary => (
            "--control-secondary-background-default",
            "--control-secondary-background-hover",
            "--control-secondary-text-default",
        ),
        _ => (
            "--control-primary-background-default",
            "--control-primary-background-hover",
            "--white",
        ),
    };
    div()
        .id(id)
        .flex_none()
        .h(px(32.))
        .px(px(12.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(color(background))
        .hover(move |style| style.bg(color(hover)))
        .text_size(px(14.))
        .font_weight(FontWeight(500.))
        .text_color(color(foreground))
        .child(text)
        .on_click(cx.listener(move |this, _, window, cx| {
            run(this, window, cx);
            cx.notify();
        }))
        .into_any_element()
}
