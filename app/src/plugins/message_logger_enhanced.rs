//! MessageLoggerEnhanced (Equicord `src/equicordplugins/messageLoggerEnhanced`, GPL-3.0-or-later): what its log
//! keeps and the original's rules, apart from the app. A record is a deleted, edited or ghost-pinging message as
//! JSON with its status (`DBMessageRecord`); `should_ignore` is `shouldIgnore`, `re_add` the `reAddDeletedMessages`
//! of a loaded page, `query` and `does_match` the log view's filter (`parseQuery`), `good_to_cache` the saved
//! attachments' limits (`isAttachmentGoodToCache` and the native side's own checks).

use dc_state::Id;
use serde_json::Value;

/// `DBMessageStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Status {
    Deleted,
    Edited,
    GhostPinged,
}

impl Status {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Status::Deleted => "DELETED",
            Status::Edited => "EDITED",
            Status::GhostPinged => "GHOST_PINGED",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "DELETED" => Some(Status::Deleted),
            "EDITED" => Some(Status::Edited),
            "GHOST_PINGED" => Some(Status::GhostPinged),
            _ => None,
        }
    }
}

/// `DBMessageRecord`: `message_id`, `channel_id`, `status` and the message.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Record {
    pub id: Id,
    pub channel: Id,
    pub status: Status,
    pub message: Value,
}

impl Record {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        let message = value.get("message")?.clone();
        let id = id_of(&value["message_id"]).or_else(|| id_of(&message["id"]))?;
        let channel = id_of(&value["channel_id"]).or_else(|| id_of(&message["channel_id"]))?;
        let status = value["status"]
            .as_str()
            .and_then(Status::parse)
            .unwrap_or_else(|| status_of(&message, None));
        Some(Self {
            id,
            channel,
            status,
            message,
        })
    }

    pub(crate) fn to_value(&self) -> Value {
        serde_json::json!({
            "message_id": self.id.to_string(),
            "channel_id": self.channel.to_string(),
            "status": self.status.as_str(),
            "message": self.message,
        })
    }

    /// The message's time (`message.timestamp`, ISO 8601, which sorts as text).
    pub(crate) fn timestamp(&self) -> &str {
        self.message["timestamp"].as_str().unwrap_or_default()
    }
}

pub(crate) fn id_of(value: &Value) -> Option<Id> {
    match value {
        Value::String(text) => text.parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

/// `hasPingged`: it mentions everyone or you.
pub(crate) fn has_pinged(message: &Value, me: Option<Id>) -> bool {
    message["mention_everyone"] == Value::Bool(true)
        || me.is_some_and(|me| {
            message["mentions"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|mention| id_of(&mention["id"]).or_else(|| id_of(mention)) == Some(me))
        })
}

/// `isGhostPinged`: marked so, or deleted after pinging you.
pub(crate) fn is_ghost_pinged(message: &Value, me: Option<Id>) -> bool {
    message["ghostPinged"] == Value::Bool(true)
        || (message["deleted"] == Value::Bool(true) && has_pinged(message, me))
}

/// `getMessageStatus`.
pub(crate) fn status_of(message: &Value, me: Option<Id>) -> Status {
    if is_ghost_pinged(message, me) {
        Status::GhostPinged
    } else if message["deleted"] == Value::Bool(true) {
        Status::Deleted
    } else {
        Status::Edited
    }
}

/// The settings `shouldIgnore` reads, MessageLogger's three lists among them.
#[derive(Clone, Debug, Default)]
pub(crate) struct IgnoreSettings {
    pub ignore_bots: bool,
    pub ignore_self: bool,
    pub ignore_webhooks: bool,
    pub always_log_dms: bool,
    pub always_log_current: bool,
    pub cache_from_servers: bool,
    pub ignore_muted_guilds: bool,
    pub ignore_muted_categories: bool,
    pub ignore_muted_channels: bool,
    pub whitelisted: String,
    pub blacklisted: String,
    /// MessageLogger's `ignoreUsers`, `ignoreChannels` and `ignoreGuilds`.
    pub base_lists: [String; 3],
}

/// What `shouldIgnore` is told of a message and reads of the stores.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Seen {
    pub channel: Id,
    pub author: Option<Id>,
    pub guild: Option<Id>,
    pub flags: u64,
    pub bot: bool,
    pub ghost_pinged: bool,
    pub cached_by_us: bool,
    pub webhook: bool,
    pub me: Option<Id>,
    pub is_dm: bool,
    pub current_channel: Option<Id>,
    pub guild_muted: bool,
    pub category_muted: bool,
    pub channel_muted: bool,
}

/// `shouldIgnore`: user over channel over server, a whitelisting first; true means the message is not logged.
pub(crate) fn should_ignore(settings: &IgnoreSettings, seen: &Seen) -> bool {
    const EPHEMERAL: u64 = 64;
    if seen.flags & EPHEMERAL == EPHEMERAL {
        return true;
    }
    if settings.ignore_self && seen.author.is_some() && seen.author == seen.me {
        return true;
    }
    if settings.always_log_dms && seen.is_dm {
        return false;
    }
    let log_current = settings.always_log_current && seen.current_channel == Some(seen.channel);
    let listed = |list: &str, id: Option<Id>| {
        id.is_some_and(|id| {
            let id = id.to_string();
            list.split(',').any(|entry| entry == id)
        })
    };
    let ids = [seen.author, Some(seen.channel), seen.guild];
    let white = &settings.whitelisted;
    let black: String = std::iter::once(settings.blacklisted.as_str())
        .chain(settings.base_lists.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(",");
    let whitelisted = ids.iter().any(|id| listed(white, *id));
    let author_white = listed(white, seen.author);
    let channel_white = listed(white, Some(seen.channel));
    let guild_white = listed(white, seen.guild);
    let blacklisted = ids.iter().any(|id| listed(&black, *id));
    let author_black = listed(&black, seen.author);
    let channel_black = listed(&black, Some(seen.channel));
    if settings.ignore_bots && seen.bot && !author_white {
        return true;
    }
    if settings.ignore_webhooks && seen.webhook && !author_white {
        return true;
    }
    if seen.ghost_pinged || author_white {
        return false;
    }
    if author_black {
        return true;
    }
    if channel_white {
        return false;
    }
    if channel_black {
        return true;
    }
    if log_current || whitelisted {
        return false;
    }
    if seen.cached_by_us && !settings.cache_from_servers && seen.guild.is_some() && !guild_white {
        return true;
    }
    if blacklisted && (!author_white || !channel_white) {
        return true;
    }
    (seen.guild.is_some() && settings.ignore_muted_guilds && seen.guild_muted)
        || (settings.ignore_muted_categories && seen.category_muted)
        || (settings.ignore_muted_channels && seen.channel_muted)
}

/// Whether a server message from the gateway is kept in the cache (`messageCreateHandler`): every server's with
/// `cacheMessagesFromServers`, else only when its channel, author or server is whitelisted.
pub(crate) fn caches_server_message(
    cache_from_servers: bool,
    whitelisted: &str,
    ids: [Option<Id>; 3],
) -> bool {
    cache_from_servers
        || ids
            .iter()
            .flatten()
            .any(|id| whitelisted.split(',').any(|entry| entry == id.to_string()))
}

/// Discord's epoch in Unix milliseconds, for a snowflake's time.
const DISCORD_EPOCH: u64 = 1_420_070_400_000;

fn snowflake_ms(id: Id) -> u64 {
    (id >> 22) + DISCORD_EPOCH
}

/// `reAddDeletedMessages`: the kept deleted messages of the page's stretch join it (newest first). `channel_start`:
/// the page reaches the newest message, so later ones join too; `channel_end`: it reaches the first, so earlier
/// ones join too.
pub(crate) fn re_add(
    page: &mut Vec<Value>,
    deleted: &[Value],
    channel_start: bool,
    channel_end: bool,
) {
    let times: Vec<u64> = page
        .iter()
        .filter_map(|message| id_of(&message["id"]))
        .map(snowflake_ms)
        .collect();
    let (Some(lowest), Some(highest)) = (times.iter().min().copied(), times.iter().max().copied())
    else {
        return;
    };
    let joining: Vec<Value> = deleted
        .iter()
        .filter(|message| {
            let Some(id) = id_of(&message["id"]) else {
                return false;
            };
            let time = snowflake_ms(id);
            (channel_end || time > lowest)
                && (channel_start || time < highest)
                && !page.iter().any(|known| id_of(&known["id"]) == Some(id))
        })
        .cloned()
        .collect();
    if joining.is_empty() {
        return;
    }
    page.extend(joining);
    page.sort_by_key(|message| std::cmp::Reverse(id_of(&message["id"]).unwrap_or(0)));
}

/// `validIdSearchTypes`.
const QUERY_KEYS: &[&str] = &[
    "server", "guild", "channel", "in", "user", "from", "message", "has", "before", "after",
    "around", "near", "during",
];

/// One `key:value` filter of the query (`parseQuery`), `!` in front negating it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Filter {
    pub key: &'static str,
    pub value: String,
    pub negate: bool,
}

/// `tokenizeQuery`: the filters, and the other words (matched in the content, ignoring case).
pub(crate) fn query(text: &str) -> (Vec<Filter>, Vec<String>) {
    let mut filters = Vec::new();
    let mut rest = Vec::new();
    for word in text.split(' ') {
        let trimmed = word.trim();
        let (negate, body) = match trimmed.strip_prefix('!') {
            Some(body) => (true, body),
            None => (false, trimmed),
        };
        let parsed = body.split_once(':').and_then(|(key, value)| {
            let key = QUERY_KEYS.iter().find(|known| **known == key)?;
            (!value.is_empty()).then(|| Filter {
                key,
                value: value.split(':').next().unwrap_or(value).to_owned(),
                negate,
            })
        });
        match parsed {
            Some(filter) if !trimmed.is_empty() => filters.push(filter),
            _ => rest.push(word.to_owned()),
        }
    }
    (filters, rest)
}

/// What `doesMatch` reads of the stores about a record: its channel's and server's names and server id.
#[derive(Clone, Debug, Default)]
pub(crate) struct Place {
    pub channel_name: Option<String>,
    pub guild: Option<Id>,
    pub guild_name: Option<String>,
}

fn has_link(text: &str) -> bool {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r"[-a-zA-Z0-9@:%._+~#=]{1,256}\.[a-zA-Z0-9()]{1,6}\b([-a-zA-Z0-9()@:%_+.~#?&/=]*)",
        )
        .expect("link regex")
    })
    .is_match(text)
}

fn date_ms(text: &str) -> Option<i64> {
    if let Ok(at) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(at.timestamp_millis());
    }
    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .map(|at| at.and_utc().timestamp_millis())
}

/// `doesMatch`: whether a record's message passes one filter.
pub(crate) fn does_match(filter: &Filter, message: &Value, place: &Place) -> bool {
    let value = filter.value.as_str();
    let lower = value.to_lowercase();
    let text = |key: &str| message[key].as_str().unwrap_or_default();
    match filter.key {
        "in" | "channel" => match &place.channel_name {
            Some(name) => text("channel_id") == value || name.to_lowercase().contains(&lower),
            None => text("channel_id") == value,
        },
        "message" => text("id") == value,
        "from" | "user" => {
            let author = &message["author"];
            author["id"].as_str() == Some(value)
                || author["username"]
                    .as_str()
                    .is_some_and(|name| name.to_lowercase().contains(&lower))
                || author["global_name"]
                    .as_str()
                    .or(author["globalName"].as_str())
                    .is_some_and(|name| name.to_lowercase().contains(&lower))
        }
        "guild" | "server" => {
            let Some(guild) = place.guild else {
                return false;
            };
            match &place.guild_name {
                Some(name) => guild.to_string() == value || name.to_lowercase().contains(&lower),
                None => guild.to_string() == value,
            }
        }
        "before" | "after" | "around" | "near" | "during" => {
            let (Some(at), Some(given)) = (date_ms(text("timestamp")), date_ms(value)) else {
                return false;
            };
            match filter.key {
                "before" => at < given,
                "after" => at > given,
                _ => (at - given).abs() < 24 * 60 * 60 * 1000,
            }
        }
        "has" => {
            let attachments = message["attachments"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let embeds = message["embeds"].as_array().cloned().unwrap_or_default();
            let typed = |kind: &str| {
                attachments.iter().any(|attachment| {
                    attachment["content_type"]
                        .as_str()
                        .is_some_and(|content| content.starts_with(kind))
                })
            };
            match value {
                "attachment" => !attachments.is_empty(),
                "image" => {
                    typed("image")
                        || embeds
                            .iter()
                            .any(|embed| !embed["image"].is_null() || !embed["thumbnail"].is_null())
                }
                "video" => typed("video") || embeds.iter().any(|embed| !embed["video"].is_null()),
                "embed" => !embeds.is_empty(),
                "link" => has_link(text("content")),
                _ => false,
            }
        }
        _ => false,
    }
}

/// The log view's filter on a record: every filter (negated ones inverted) and every other word in the content.
pub(crate) fn passes_query(
    filters: &[Filter],
    words: &[String],
    message: &Value,
    place: &Place,
) -> bool {
    let content = message["content"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    filters
        .iter()
        .all(|filter| does_match(filter, message, place) != filter.negate)
        && words
            .iter()
            .all(|word| content.contains(&word.to_lowercase()))
}

/// `isGroupStart` of the log view: a new author, or five minutes apart.
pub(crate) fn is_group_start(current: &Value, previous: Option<&Value>) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    if current["id"] == previous["id"] || current["author"]["id"] != previous["author"]["id"] {
        return true;
    }
    let at = |message: &Value| date_ms(message["timestamp"].as_str().unwrap_or_default());
    match (at(current), at(previous)) {
        (Some(a), Some(b)) => (a - b).abs() >= 5 * 60 * 1000,
        _ => true,
    }
}

/// `blockedExts` of `list.ts`: file types never saved, whatever the setting lists.
const BLOCKED_EXTENSIONS: &[&str] = &[
    "com",
    "exe",
    "dll",
    "ocx",
    "ps1",
    "ps1xml",
    "ps2",
    "ps2xml",
    "psc1",
    "psc2",
    "msh",
    "msh1",
    "msh2",
    "mshxml",
    "msh1xml",
    "msh2xml",
    "js",
    "jse",
    "vbs",
    "vb",
    "vbe",
    "cmd",
    "bat",
    "hta",
    "inf",
    "reg",
    "pif",
    "scr",
    "cpl",
    "scf",
    "msc",
    "pol",
    "hlp",
    "chm",
    "ws",
    "wsf",
    "wsc",
    "wsh",
    "jar",
    "z",
    "bz2",
    "cab",
    "tar",
    "ace",
    "msi",
    "msp",
    "mst",
    "msu",
    "ppkg",
    "bak",
    "tmp",
    "ost",
    "pst",
    "pkg",
    "iso",
    "img",
    "vhd",
    "vhdx",
    "application",
    "lock",
    "lck",
    "sln",
    "cs",
    "csproj",
    "resx",
    "config",
    "resources",
    "pdb",
    "manifest",
    "doc",
    "dot",
    "wbk",
    "xls",
    "xlt",
    "xlm",
    "xla",
    "ppt",
    "pot",
    "pps",
    "ade",
    "adp",
    "mdb",
    "cdb",
    "mda",
    "mdn",
    "mdt",
    "mdf",
    "mde",
    "ldb",
    "wps",
    "xlsb",
    "xlam",
    "xll",
    "xlw",
    "ppam",
];

/// `attachmentFileExtensions`' `onChange`: lower case, trimmed, the blocked types taken out.
pub(crate) fn allowed_extensions(setting: &str) -> Vec<String> {
    setting
        .split(',')
        .map(|ext| ext.trim().to_lowercase())
        .filter(|ext| !ext.is_empty() && !BLOCKED_EXTENSIONS.contains(&ext.as_str()))
        .collect()
}

/// `getFileExtension` of the file name (else the address), else the content type's subtype, else `.png`; with
/// its dot.
pub(crate) fn file_extension(attachment: &Value) -> String {
    let name = attachment["filename"]
        .as_str()
        .or(attachment["url"].as_str())
        .unwrap_or_default();
    let path = name.split('?').next().unwrap_or_default();
    if let Some((_, ext)) = path.rsplit_once('.')
        && !ext.is_empty()
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return format!(".{ext}");
    }
    attachment["content_type"]
        .as_str()
        .and_then(|content| content.split('/').nth(1))
        .map(|sub| format!(".{}", sub.split(';').next().unwrap_or(sub)))
        .unwrap_or_else(|| ".png".to_owned())
}

/// `isAttachmentGoodToCache` with the native checks: under the size limit, a listed type, an id and extension
/// that cannot leave the folder, and a Discord CDN address.
pub(crate) fn good_to_cache(attachment: &Value, size_limit_mb: f64, extensions: &str) -> bool {
    let size = attachment["size"].as_f64().unwrap_or(0.);
    if size > size_limit_mb * 1024. * 1024. {
        return false;
    }
    let ext = file_extension(attachment);
    let bare = ext.trim_start_matches('.').to_lowercase();
    let id = attachment["id"].as_str().unwrap_or_default();
    let cdn = |key: &str| {
        attachment[key].as_str().is_some_and(|url| {
            url.starts_with("https://cdn.discordapp.com/")
                || url.starts_with("https://media.discordapp.net/")
        })
    };
    !id.is_empty()
        && id.chars().all(|c| c.is_ascii_digit())
        && !bare.contains(['/', '\\'])
        && allowed_extensions(extensions).contains(&bare)
        && cdn("url")
}

/// The address of a saved picture in a re-added message: the app asks for it as for any picture, and the network
/// side reads the file instead (`saved_image_bytes`), as the original hands its saved files out as `blob:` addresses.
pub(crate) const SAVED_SCHEME: &str = "gravecord-saved-image:";

static IMAGES_DIR: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);

/// Where the saved pictures are (`imageCacheDir`), for the network side.
pub(crate) fn set_images_dir(dir: std::path::PathBuf) {
    if let Ok(mut known) = IMAGES_DIR.write() {
        *known = Some(dir);
    }
}

/// A saved picture's file name from its address: `<attachment id>.<ext>`, nothing that leaves the folder.
pub(crate) fn saved_name(url: &str) -> Option<&str> {
    let name = url.strip_prefix(SAVED_SCHEME)?.split(['?', '#']).next()?;
    let (id, ext) = name.split_once('.')?;
    (!id.is_empty()
        && id.chars().all(|c| c.is_ascii_digit())
        && !ext.is_empty()
        && ext.chars().all(|c| c.is_ascii_alphanumeric()))
    .then_some(name)
}

/// The bytes of a saved picture asked for by its address (`getImageNative`); `None` for any other address.
pub(crate) fn saved_image_bytes(url: &str) -> Option<Vec<u8>> {
    let name = saved_name(url)?;
    let dir = IMAGES_DIR.read().ok()?.clone()?;
    std::fs::read(dir.join(name)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings() -> IgnoreSettings {
        IgnoreSettings {
            always_log_dms: true,
            always_log_current: true,
            ..IgnoreSettings::default()
        }
    }

    #[test]
    fn should_ignore_follows_its_priorities() {
        let base = Seen {
            channel: 10,
            author: Some(1),
            guild: Some(20),
            me: Some(9),
            ..Seen::default()
        };
        let s = settings();
        assert!(!should_ignore(&s, &base));
        assert!(should_ignore(&s, &Seen { flags: 64, ..base }));
        // A server message only our cache saw is let go without cacheMessagesFromServers.
        let cached = Seen {
            cached_by_us: true,
            ..base
        };
        assert!(should_ignore(&s, &cached));
        assert!(!should_ignore(
            &IgnoreSettings {
                cache_from_servers: true,
                ..settings()
            },
            &cached
        ));
        // The open channel is logged anyway, and so is a whitelisted server.
        assert!(!should_ignore(
            &s,
            &Seen {
                current_channel: Some(10),
                ..cached
            }
        ));
        let mut listed = settings();
        listed.whitelisted = "20".to_owned();
        assert!(!should_ignore(&listed, &cached));
        // A blacklisted author is ignored even in a whitelisted channel; a whitelisted author wins over a
        // blacklisted channel.
        let mut both = settings();
        both.whitelisted = "10".to_owned();
        both.blacklisted = "1".to_owned();
        assert!(should_ignore(&both, &base));
        both.whitelisted = "1".to_owned();
        both.blacklisted = "10".to_owned();
        assert!(!should_ignore(&both, &base));
        // MessageLogger's own lists blacklist too.
        let mut base_lists = settings();
        base_lists.base_lists[1] = "10".to_owned();
        assert!(should_ignore(&base_lists, &base));
        // Bots, webhooks and yourself by their switches; a ghost ping is kept over the lists.
        let bots = IgnoreSettings {
            ignore_bots: true,
            ..settings()
        };
        assert!(should_ignore(&bots, &Seen { bot: true, ..base }));
        let mut ghost = settings();
        ghost.blacklisted = "1".to_owned();
        assert!(!should_ignore(
            &ghost,
            &Seen {
                ghost_pinged: true,
                ..base
            }
        ));
        let me = IgnoreSettings {
            ignore_self: true,
            ..settings()
        };
        assert!(should_ignore(
            &me,
            &Seen {
                author: Some(9),
                is_dm: true,
                ..base
            }
        ));
        // DMs are always logged; muted places by their switches.
        let mut dm = settings();
        dm.blacklisted = "1".to_owned();
        assert!(!should_ignore(
            &dm,
            &Seen {
                is_dm: true,
                ..base
            }
        ));
        let muted = IgnoreSettings {
            ignore_muted_guilds: true,
            ..settings()
        };
        assert!(should_ignore(
            &muted,
            &Seen {
                guild_muted: true,
                ..base
            }
        ));
    }

    #[test]
    fn deleted_messages_rejoin_their_stretch() {
        let id = |n: u64| json!({"id": (n << 22).to_string()});
        let mut page = vec![id(30), id(20)];
        let kept = [id(10), id(25), id(40)];
        re_add(&mut page, &kept, false, false);
        let ids: Vec<u64> = page
            .iter()
            .map(|m| id_of(&m["id"]).unwrap() >> 22)
            .collect();
        assert_eq!(ids, [30, 25, 20]);
        re_add(&mut page, &kept, true, true);
        let ids: Vec<u64> = page
            .iter()
            .map(|m| id_of(&m["id"]).unwrap() >> 22)
            .collect();
        assert_eq!(ids, [40, 30, 25, 20, 10]);
    }

    #[test]
    fn the_query_reads_filters_and_words() {
        let (filters, words) = query("from:ada !has:image hello");
        assert_eq!(filters.len(), 2);
        assert_eq!(filters[0].key, "from");
        assert!(filters[1].negate);
        assert_eq!(words, ["hello"]);
        let message = json!({"id": "5", "content": "Hello world", "author": {"id": "1", "username": "ada"},
            "attachments": [], "embeds": [], "timestamp": "2026-11-18T01:00:00+00:00"});
        let place = Place::default();
        assert!(passes_query(&filters, &words, &message, &place));
        let (filters, words) = query("has:link");
        assert!(!passes_query(&filters, &words, &message, &place));
        let (filters, words) = query("before:2026-11-19 after:2026-11-17");
        assert!(passes_query(&filters, &words, &message, &place));
        let (filters, words) = query("nope:x");
        assert!(filters.is_empty());
        assert!(!passes_query(&filters, &words, &message, &place));
    }

    #[test]
    fn attachments_are_saved_by_size_and_type() {
        let attachment = json!({"id": "123", "filename": "a.PNG", "size": 1000,
            "url": "https://cdn.discordapp.com/attachments/1/123/a.PNG"});
        let exts = "png,jpg";
        assert_eq!(file_extension(&attachment), ".PNG");
        assert!(good_to_cache(&attachment, 12., exts));
        assert!(!good_to_cache(&attachment, 0.0001, exts));
        assert!(!good_to_cache(&attachment, 12., "jpg"));
        assert_eq!(allowed_extensions(" PNG ,exe,"), ["png"]);
        let elsewhere = json!({"id": "123", "filename": "a.png", "size": 1, "url": "https://example.com/a.png"});
        assert!(!good_to_cache(&elsewhere, 12., exts));
    }

    #[test]
    fn saved_addresses_stay_in_the_folder() {
        assert_eq!(
            saved_name("gravecord-saved-image:123.png?width=4"),
            Some("123.png")
        );
        assert_eq!(saved_name("gravecord-saved-image:../x.png"), None);
        assert_eq!(saved_name("gravecord-saved-image:12.p/g"), None);
        assert_eq!(saved_name("https://cdn.discordapp.com/123.png"), None);
    }

    #[test]
    fn ghost_pings_and_statuses() {
        let message = json!({"deleted": true, "mentions": [{"id": "9"}]});
        assert!(is_ghost_pinged(&message, Some(9)));
        assert_eq!(status_of(&message, Some(9)), Status::GhostPinged);
        assert_eq!(status_of(&message, Some(8)), Status::Deleted);
        assert_eq!(
            status_of(&json!({"editHistory": [1]}), None),
            Status::Edited
        );
        let record = Record {
            id: 5,
            channel: 6,
            status: Status::Edited,
            message: json!({"id": "5"}),
        };
        assert_eq!(Record::from_value(&record.to_value()), Some(record));
    }
}
