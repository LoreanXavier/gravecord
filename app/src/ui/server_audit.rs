//! Server Settings > Audit Log (module 989433 `cA`, `cx`, requests `cn`, `ci`, `cl`; store module 523599; record module
//! 411153; formatter module 898472, generated into `audit_spec.rs`; icons and details module 438407).
//!
//! `GET /guilds/{id}/audit-logs` lists 50 entries at a time, by user and by action when filtered. Close entries of one
//! kind merge into one row (`P`), and a row with changes to show expands to a numbered list. Not drawn: onboarding and
//! home settings' own change lists, avatars' and names' context menus, and the labels of AutoMod's enums.
//! Measured in `design/dom/settings/guild-audit-darker-cozy.json`, `-actions-`, `-users-`, `-expanded-`.

use super::audit_spec::{
    ACTION_FILTERS, ADDED_FLAGS, CHANGE_RULES, HIDDEN_CHANGES, LOCALES, MESSAGES, PERMISSIONS,
    REASON, REMOVED_FLAGS, Rule, TITLES, action,
};
use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use dc_state::{ChannelKind, Id, User};
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, SharedString, StyledText,
    deferred, div, img, prelude::*, px,
};
use serde_json::Value;
use std::collections::HashMap;

pub(super) const AUDIT_PREFIX: &str = "guild-audit:";
const NEXT_PREFIX: &str = "guild-audit-next:";
const PAGE: usize = 50;
/// `P`: entries merge within this long and this many times.
const MERGE_MINUTES: i64 = 30;
const MERGE_MAX: usize = 50;
/// The `icon_LajYMK` overlays, by what the action does.
const OVERLAY_CREATE: &str = "https://discord.com/assets/9dabbd95e9e8a278.svg";
const OVERLAY_DELETE: &str = "https://discord.com/assets/273a82a28d0e1160.svg";
const OVERLAY_UPDATE: &str = "https://discord.com/assets/f50ede2a8b3701be.svg";

/// The bits that make someone a moderator for the user filter (`g`).
const MODERATOR_BITS: u64 = (1 << 1)
    | (1 << 2)
    | (1 << 3)
    | (1 << 4)
    | (1 << 5)
    | (1 << 13)
    | (1 << 27)
    | (1 << 28)
    | (1 << 29)
    | (1 << 30)
    | (1 << 24)
    | (1 << 22)
    | (1 << 23);

/// The dark theme's picture for an action, from `.theme-dark .icon_LajYMK.target*`.
fn icon_url(action_id: u64, target: &str) -> &'static str {
    let hash = if action_id == action::MESSAGE_DELETE {
        "14bedd367511ce89"
    } else if matches!(
        action_id,
        action::AUTO_MODERATION_BLOCK_MESSAGE
            | action::AUTO_MODERATION_FLAG_TO_CHANNEL
            | action::AUTO_MODERATION_USER_COMMUNICATION_DISABLED
            | action::AUTO_MODERATION_QUARANTINE_USER
    ) {
        "91bd7f1643fe02ff"
    } else {
        match target {
            "ALL" => "df1be54a7d6a8857",
            "GUILD" | "GUILD_PROFILE" => "ac84ae8206a6f5e1",
            "CHANNEL" | "CHANNEL_OVERWRITE" | "VOICE_CHANNEL_STATUS" => "b120d96b118bf2c1",
            "USER" | "GUILD_MEMBER_VERIFICATION" => "69a57bf1542db6d0",
            "ROLE" => "eea18d5e3a85428c",
            "ONBOARDING_PROMPT" | "GUILD_ONBOARDING" | "HOME_SETTINGS" => "e35fcd676e720a43",
            "INTEGRATION" => "0f213e7334ec406b",
            "INVITE" => "83713b6ddb3aaba9",
            "WEBHOOK" | "AUTO_MODERATION_RULE" => "91bd7f1643fe02ff",
            "EMOJI" => "e8e77b2ca603a44a",
            "STICKER" => "6d6c4298fbda978b",
            "STAGE_INSTANCE" => "b81e4773a96fb05d",
            "GUILD_SCHEDULED_EVENT" | "GUILD_SCHEDULED_EVENT_EXCEPTION" => "06e0c61b48f72f6d",
            "THREAD" => "f31aad3177a6c7c8",
            "APPLICATION_COMMAND" => "3934256e60927112",
            "GUILD_HOME" => "606a039f64780e30",
            "GUILD_SOUNDBOARD" => "547b254784ef1abb",
            _ => "ac84ae8206a6f5e1",
        }
    };
    ICON_URLS
        .iter()
        .find(|url| url.contains(hash))
        .copied()
        .unwrap_or(ICON_URLS[0])
}

const ICON_URLS: &[&str] = &[
    "https://discord.com/assets/ac84ae8206a6f5e1.svg",
    "https://discord.com/assets/14bedd367511ce89.svg",
    "https://discord.com/assets/91bd7f1643fe02ff.svg",
    "https://discord.com/assets/df1be54a7d6a8857.svg",
    "https://discord.com/assets/b120d96b118bf2c1.svg",
    "https://discord.com/assets/69a57bf1542db6d0.svg",
    "https://discord.com/assets/eea18d5e3a85428c.svg",
    "https://discord.com/assets/e35fcd676e720a43.svg",
    "https://discord.com/assets/0f213e7334ec406b.svg",
    "https://discord.com/assets/83713b6ddb3aaba9.svg",
    "https://discord.com/assets/e8e77b2ca603a44a.svg",
    "https://discord.com/assets/6d6c4298fbda978b.svg",
    "https://discord.com/assets/b81e4773a96fb05d.svg",
    "https://discord.com/assets/06e0c61b48f72f6d.svg",
    "https://discord.com/assets/f31aad3177a6c7c8.svg",
    "https://discord.com/assets/3934256e60927112.svg",
    "https://discord.com/assets/606a039f64780e30.svg",
    "https://discord.com/assets/547b254784ef1abb.svg",
];

/// A message as the string table parses it.
fn message(id: &str) -> Option<Value> {
    let at = MESSAGES
        .binary_search_by(|(known, _)| (*known).cmp(id))
        .ok()?;
    serde_json::from_str(MESSAGES[at].1).ok()
}

/// A piece of a formatted message.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Span {
    Plain(String),
    Strong(String),
    /// A small round swatch (`colorHook_LajYMK`).
    Swatch(String),
}

/// `intl.format`: fills a parsed message from its arguments. `count` selects plurals.
fn format_nodes(
    nodes: &Value,
    args: &HashMap<&str, String>,
    count: Option<i64>,
    strong: bool,
    out: &mut Vec<Span>,
) {
    let Some(nodes) = nodes.as_array() else {
        return;
    };
    let push = |text: String, strong: bool, out: &mut Vec<Span>| {
        if !text.is_empty() {
            out.push(if strong {
                Span::Strong(text)
            } else {
                Span::Plain(text)
            });
        }
    };
    for node in nodes {
        if let Some(text) = node.as_str() {
            push(text.to_owned(), strong, out);
            continue;
        }
        match node[0].as_i64() {
            // An argument, a number.
            Some(1 | 2) => {
                let name = node[1].as_str().unwrap_or_default();
                push(args.get(name).cloned().unwrap_or_default(), strong, out);
            }
            // A plural: the exact form, then `one`, then `other`.
            Some(6) => {
                let name = node[1].as_str().unwrap_or_default();
                let number = args
                    .get(name)
                    .and_then(|value| value.parse::<i64>().ok())
                    .or(count)
                    .unwrap_or(0);
                let forms = &node[2];
                let exact = format!("={number}");
                let form = if forms.get(&exact).is_some() {
                    &forms[&exact]
                } else if number == 1 && forms.get("one").is_some() {
                    &forms["one"]
                } else {
                    &forms["other"]
                };
                format_nodes(form, args, Some(number), strong, out);
            }
            // The plural's number.
            Some(7) => push(count.unwrap_or(0).to_string(), strong, out),
            Some(8) => {
                let tag = node[1].as_str().unwrap_or_default();
                match tag {
                    "$b" => format_nodes(&node[2], args, count, true, out),
                    // `ch`: a person's name is in the row's normal weight, whatever the message marks.
                    "userHook" | "memberHook" => {
                        let mut inner = Vec::new();
                        format_nodes(&node[2], args, count, false, &mut inner);
                        out.extend(inner.into_iter().map(|span| match span {
                            Span::Strong(text) => Span::Plain(text),
                            other => other,
                        }));
                    }
                    "newColorHook" => {
                        if let Some(value) = args.get("newValue") {
                            out.push(Span::Swatch(value.clone()));
                        }
                    }
                    "oldColorHook" => {
                        if let Some(value) = args.get("oldValue") {
                            out.push(Span::Swatch(value.clone()));
                        }
                    }
                    "newColorsHook" => {
                        if let Some(value) = args.get("newColors") {
                            for (at, colour) in value.split(',').enumerate() {
                                if at > 0 {
                                    push(", ".to_owned(), true, out);
                                }
                                push(colour.to_owned(), true, out);
                                out.push(Span::Swatch(colour.to_owned()));
                            }
                        }
                    }
                    _ => format_nodes(&node[2], args, count, strong, out),
                }
            }
            _ => {}
        }
    }
}

fn format_message(id: &str, args: &HashMap<&str, String>, count: Option<i64>) -> Vec<Span> {
    let mut out = Vec::new();
    if let Some(nodes) = message(id) {
        format_nodes(&nodes, args, count, false, &mut out);
    }
    out
}

/// One text message without arguments.
fn plain_message(id: &str) -> String {
    format_message(id, &HashMap::new(), None)
        .into_iter()
        .map(|span| match span {
            Span::Plain(text) | Span::Strong(text) | Span::Swatch(text) => text,
        })
        .collect()
}

/// `QO`: one change of an entry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Change {
    key: String,
    old: Value,
    new: Value,
    subtarget: Option<String>,
}

impl Change {
    fn new(key: &str, old: Value, new: Value) -> Self {
        Self {
            key: key.to_owned(),
            old,
            new,
            subtarget: None,
        }
    }
}

/// `c` in module 411153, after `ea`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    id: String,
    action: u64,
    target_id: Option<String>,
    user_id: Option<String>,
    options: Value,
    changes: Vec<Change>,
    start_ms: i64,
    end_ms: i64,
}

/// `u`: what kind of thing an action is about.
fn target_type(id: u64) -> &'static str {
    use action::*;
    if id <= GUILD_UPDATE {
        "GUILD"
    } else if id <= CHANNEL_DELETE || id == MESSAGE_BULK_DELETE || id == CHANNEL_POSITION_UPDATE {
        "CHANNEL"
    } else if id <= CHANNEL_OVERWRITE_DELETE {
        "CHANNEL_OVERWRITE"
    } else if id <= BOT_ADD || id == MESSAGE_DELETE || id == MESSAGE_PIN || id == MESSAGE_UNPIN {
        "USER"
    } else if id <= ROLE_DELETE || id == ROLE_POSITION_UPDATE {
        "ROLE"
    } else if id <= INVITE_DELETE {
        "INVITE"
    } else if id <= WEBHOOK_DELETE {
        "WEBHOOK"
    } else if id <= EMOJI_DELETE {
        "EMOJI"
    } else if id <= INTEGRATION_DELETE {
        "INTEGRATION"
    } else if id <= STAGE_INSTANCE_DELETE {
        "STAGE_INSTANCE"
    } else if id <= STICKER_DELETE {
        "STICKER"
    } else if id <= GUILD_SCHEDULED_EVENT_DELETE {
        "GUILD_SCHEDULED_EVENT"
    } else if id <= THREAD_DELETE {
        "THREAD"
    } else if id == APPLICATION_COMMAND_PERMISSION_UPDATE {
        "APPLICATION_COMMAND"
    } else if id <= SOUNDBOARD_SOUND_DELETE {
        "GUILD_SOUNDBOARD"
    } else if id < AUTO_MODERATION_BLOCK_MESSAGE {
        "AUTO_MODERATION_RULE"
    } else if matches!(
        id,
        AUTO_MODERATION_BLOCK_MESSAGE
            | AUTO_MODERATION_FLAG_TO_CHANNEL
            | AUTO_MODERATION_USER_COMMUNICATION_DISABLED
            | AUTO_MODERATION_QUARANTINE_USER
    ) {
        "USER"
    } else if id <= CREATOR_MONETIZATION_TERMS_ACCEPTED {
        "GUILD"
    } else if id <= ONBOARDING_PROMPT_DELETE {
        "ONBOARDING_PROMPT"
    } else if id <= ONBOARDING_UPDATE {
        "GUILD_ONBOARDING"
    } else if id <= GUILD_HOME_REMOVE_ITEM {
        "GUILD_HOME"
    } else if id <= HARMFUL_LINKS_BLOCKED_MESSAGE {
        "GUILD"
    } else if id <= HOME_SETTINGS_UPDATE {
        "HOME_SETTINGS"
    } else if id <= VOICE_CHANNEL_STATUS_DELETE {
        "VOICE_CHANNEL_STATUS"
    } else if id <= GUILD_SCHEDULED_EVENT_EXCEPTION_DELETE {
        "GUILD_SCHEDULED_EVENT_EXCEPTION"
    } else if id <= GUILD_MEMBER_VERIFICATION_UPDATE {
        "GUILD_MEMBER_VERIFICATION"
    } else if id <= GUILD_PROFILE_UPDATE {
        "GUILD_PROFILE"
    } else if id <= GUILD_MIGRATE_BYPASS_SLOWMODE_PERMISSION {
        "GUILD"
    } else {
        "UNKNOWN"
    }
}

/// `o`: what the action does to its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Create,
    Update,
    Delete,
    All,
}

fn action_kind(id: u64) -> Kind {
    use action::*;
    match id {
        CHANNEL_CREATE
        | CHANNEL_OVERWRITE_CREATE
        | MEMBER_BAN_REMOVE
        | ROLE_CREATE
        | INVITE_CREATE
        | WEBHOOK_CREATE
        | EMOJI_CREATE
        | STICKER_CREATE
        | MESSAGE_PIN
        | BOT_ADD
        | INTEGRATION_CREATE
        | STAGE_INSTANCE_CREATE
        | GUILD_SCHEDULED_EVENT_CREATE
        | THREAD_CREATE
        | AUTO_MODERATION_RULE_CREATE
        | CREATOR_MONETIZATION_REQUEST_CREATED
        | ONBOARDING_PROMPT_CREATE
        | ONBOARDING_CREATE
        | GUILD_HOME_FEATURE_ITEM
        | SOUNDBOARD_SOUND_CREATE
        | HOME_SETTINGS_CREATE
        | VOICE_CHANNEL_STATUS_CREATE => Kind::Create,
        CHANNEL_DELETE
        | CHANNEL_OVERWRITE_DELETE
        | MEMBER_KICK
        | MEMBER_PRUNE
        | MEMBER_BAN_ADD
        | ROLE_DELETE
        | INVITE_DELETE
        | WEBHOOK_DELETE
        | EMOJI_DELETE
        | STICKER_DELETE
        | MESSAGE_DELETE
        | MESSAGE_BULK_DELETE
        | MESSAGE_UNPIN
        | INTEGRATION_DELETE
        | MEMBER_DISCONNECT
        | STAGE_INSTANCE_DELETE
        | GUILD_SCHEDULED_EVENT_DELETE
        | GUILD_SCHEDULED_EVENT_EXCEPTION_DELETE
        | THREAD_DELETE
        | AUTO_MODERATION_BLOCK_MESSAGE
        | AUTO_MODERATION_RULE_DELETE
        | ONBOARDING_PROMPT_DELETE
        | GUILD_HOME_REMOVE_ITEM
        | SOUNDBOARD_SOUND_DELETE
        | VOICE_CHANNEL_STATUS_DELETE => Kind::Delete,
        GUILD_UPDATE
        | CHANNEL_UPDATE
        | CHANNEL_OVERWRITE_UPDATE
        | CHANNEL_POSITION_UPDATE
        | MEMBER_UPDATE
        | MEMBER_ROLE_UPDATE
        | ROLE_UPDATE
        | ROLE_POSITION_UPDATE
        | INVITE_UPDATE
        | WEBHOOK_UPDATE
        | EMOJI_UPDATE
        | STICKER_UPDATE
        | INTEGRATION_UPDATE
        | MEMBER_MOVE
        | STAGE_INSTANCE_UPDATE
        | GUILD_SCHEDULED_EVENT_UPDATE
        | GUILD_SCHEDULED_EVENT_EXCEPTION_CREATE
        | GUILD_SCHEDULED_EVENT_EXCEPTION_UPDATE
        | THREAD_UPDATE
        | APPLICATION_COMMAND_PERMISSION_UPDATE
        | CREATOR_MONETIZATION_TERMS_ACCEPTED
        | AUTO_MODERATION_RULE_UPDATE
        | AUTO_MODERATION_FLAG_TO_CHANNEL
        | AUTO_MODERATION_USER_COMMUNICATION_DISABLED
        | AUTO_MODERATION_QUARANTINE_USER
        | ONBOARDING_PROMPT_UPDATE
        | ONBOARDING_UPDATE
        | SOUNDBOARD_SOUND_UPDATE
        | HOME_SETTINGS_UPDATE
        | GUILD_MEMBER_VERIFICATION_UPDATE
        | GUILD_PROFILE_UPDATE
        | GUILD_MIGRATE_PIN_PERMISSION
        | GUILD_MIGRATE_BYPASS_SLOWMODE_PERMISSION => Kind::Update,
        _ => Kind::All,
    }
}

/// Discord's epoch, for the time inside an id.
fn id_ms(id: &str) -> i64 {
    id.parse::<u64>()
        .map_or(0, |id| ((id >> 22) + 1_420_070_400_000) as i64)
}

/// Whether two entries become one row (the test inside `P`).
fn merges(last: &Entry, next: &Entry, merged: usize) -> bool {
    use action::*;
    last.action == next.action
        && last.target_id == next.target_id
        && last.user_id == next.user_id
        && last.options == next.options
        && (next.start_ms - last.start_ms) / 60_000 < MERGE_MINUTES
        && merged < MERGE_MAX
        && target_type(next.action) != "INVITE"
        && !matches!(
            next.action,
            MESSAGE_DELETE
                | MESSAGE_BULK_DELETE
                | MESSAGE_PIN
                | MESSAGE_UNPIN
                | MEMBER_MOVE
                | MEMBER_DISCONNECT
                | CHANNEL_POSITION_UPDATE
                | ROLE_POSITION_UPDATE
                | BOT_ADD
                | APPLICATION_COMMAND_PERMISSION_UPDATE
                | MEMBER_PRUNE
        )
}

/// `P`: raw entries, newest first, to rows; a merged row keeps the newest entry's id. Deleted things' names are kept for
/// the rows that name them later.
fn build_entries(raw: &[Value], deleted: &mut HashMap<String, String>) -> Vec<Entry> {
    let mut rows: Vec<Entry> = Vec::new();
    let mut merged = 0;
    for item in raw.iter().rev() {
        let action_id = item["action_type"].as_u64().unwrap_or(0);
        let mut changes = Vec::new();
        let (mut name, mut title, mut kind) = (None, None, None);
        if let Some(reason) = item["reason"].as_str() {
            changes.push(Change::new("reason", Value::Null, Value::from(reason)));
        }
        for change in item["changes"].as_array().into_iter().flatten() {
            let built = Change::new(
                change["key"].as_str().unwrap_or_default(),
                change["old_value"].clone(),
                change["new_value"].clone(),
            );
            match built.key.as_str() {
                "name" => name = Some(built.clone()),
                "type" => kind = Some(built.clone()),
                "title" => title = Some(built.clone()),
                _ => {}
            }
            changes.push(built);
        }
        if action_id == action::MEMBER_PRUNE {
            let days = item["options"]["delete_member_days"]
                .as_str()
                .and_then(|days| days.parse::<u64>().ok())
                .or(item["options"]["delete_member_days"].as_u64())
                .unwrap_or(1);
            changes.push(Change::new(
                "prune_delete_days",
                Value::Null,
                Value::from(days),
            ));
        }
        if action_id == action::AUTO_MODERATION_BLOCK_MESSAGE
            && !item["options"]["auto_moderation_rule_name"].is_null()
        {
            changes.push(Change::new(
                "auto_moderation_triggered_rule_name",
                Value::Null,
                item["options"]["auto_moderation_rule_name"].clone(),
            ));
        }
        if action_id == action::VOICE_CHANNEL_STATUS_CREATE && !item["options"]["status"].is_null()
        {
            changes.push(Change::new(
                "status",
                Value::Null,
                item["options"]["status"].clone(),
            ));
        }
        let id = item["id"].as_str().unwrap_or_default().to_owned();
        let at = id_ms(&id);
        let entry = Entry {
            id,
            action: action_id,
            target_id: item["target_id"].as_str().map(str::to_owned),
            user_id: item["user_id"].as_str().map(str::to_owned),
            options: if item["options"].is_null() {
                Value::Object(Default::default())
            } else {
                item["options"].clone()
            },
            changes,
            start_ms: at,
            end_ms: at,
        };
        if let Some(last) = rows.first_mut()
            && merges(last, &entry, merged)
        {
            last.changes.extend(entry.changes);
            last.end_ms = entry.start_ms;
            merged += 1;
            continue;
        }
        if action_kind(entry.action) == Kind::Delete && (name.is_some() || title.is_some()) {
            let mut old = name
                .or(title)
                .and_then(|change| change.old.as_str().map(str::to_owned));
            let target = target_type(entry.action);
            // `tr`: the channel types named with a hash.
            let textual = kind
                .and_then(|change| change.old.as_u64())
                .is_some_and(|kind| matches!(kind, 0 | 5 | 6 | 10 | 11 | 12 | 14 | 15 | 16 | 17));
            if matches!(target, "CHANNEL" | "CHANNEL_OVERWRITE") && textual {
                old = old.map(|old| format!("#{old}"));
            }
            if let (Some(old), Some(target_id)) = (old, entry.target_id.as_ref()) {
                deleted.insert(format!("{target}:{target_id}"), old);
            }
        }
        merged = 0;
        rows.insert(0, entry);
    }
    rows
}

/// Calendar words (`moment().calendar()`).
fn calendar(ms: i64, now: chrono::DateTime<chrono::Local>) -> String {
    use chrono::TimeZone;
    let Some(local) = chrono::Local.timestamp_millis_opt(ms).single() else {
        return String::new();
    };
    let days = (local.date_naive() - now.date_naive()).num_days();
    let time = local.format("%-I:%M %p");
    match days {
        0 => format!("Today at {time}"),
        -1 => format!("Yesterday at {time}"),
        1 => format!("Tomorrow at {time}"),
        -6..=-2 => format!("Last {} at {time}", local.format("%A")),
        2..=6 => format!("{} at {time}", local.format("%A")),
        _ => local.format("%m/%d/%Y").to_string(),
    }
}

/// `w`: a change a row does not list.
fn hidden_change(entry: &Entry, change: &Change) -> bool {
    use action::*;
    if action_kind(entry.action) == Kind::Delete
        && !matches!(entry.action, MEMBER_BAN_ADD | MEMBER_KICK | MEMBER_PRUNE)
    {
        return change.key != "reason";
    }
    let target = target_type(entry.action);
    HIDDEN_CHANGES
        .iter()
        .any(|(known, key)| *known == target && *key == change.key)
}

/// `W`: whether a row can open.
fn has_details(entry: &Entry) -> bool {
    entry
        .changes
        .iter()
        .any(|change| !hidden_change(entry, change))
}

/// The value as `intl.format` prints it.
fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

/// `eT`'s key for a new value.
fn map_key(value: &Value) -> String {
    value_text(value)
}

/// `V` and the rule's function: which message a change uses.
fn change_message(entry: &Entry, change: &Change) -> Option<&'static str> {
    let target = target_type(entry.action);
    if target == "APPLICATION_COMMAND" {
        if change.key == "reason" {
            return Some(REASON);
        }
        return Some(if !change.new.is_null() {
            if change
                .new
                .get("permission")
                .is_some_and(|flag| flag == true)
            {
                "JH+89C"
            } else {
                "HUrFDu"
            }
        } else {
            "vynxnV"
        });
    }
    let rule = CHANGE_RULES
        .iter()
        .find(|(known, key, _)| *known == target && *key == change.key)
        .map(|(_, _, rule)| *rule);
    let rule = match rule {
        Some(rule) => rule,
        // `default: return B()`.
        None if change.key == "reason" => Rule::Fixed(REASON),
        None => return None,
    };
    let truthy = |value: &Value| match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.),
        Value::String(text) => !text.is_empty(),
        _ => true,
    };
    Some(match rule {
        Rule::Fixed(id) => id,
        Rule::OldNull(first, second) => {
            if change.old.is_null() {
                first
            } else {
                second
            }
        }
        Rule::NewNull(first, second) => {
            if change.new.is_null() {
                first
            } else {
                second
            }
        }
        Rule::Truthy(first, second) => {
            if truthy(&change.new) {
                first
            } else {
                second
            }
        }
        Rule::Three(both, new, old) => match (change.new.is_null(), change.old.is_null()) {
            (false, false) => both,
            (false, true) => new,
            (true, false) => old,
            _ => return None,
        },
        Rule::Map(table) => {
            let key = map_key(&change.new);
            table.iter().find(|(value, _)| *value == key)?.1
        }
        Rule::MapOr(table, fallback) => {
            let key = map_key(&change.new);
            table
                .iter()
                .find(|(value, _)| *value == key)
                .map_or(fallback, |(_, id)| *id)
        }
        Rule::EitherTrue(first, second) => {
            let value = if change.new.is_null() {
                &change.old
            } else {
                &change.new
            };
            if *value == true { first } else { second }
        }
        Rule::NewTrue(first, second) => {
            if change.new == true {
                first
            } else {
                second
            }
        }
        Rule::Gradient(one, gradient) => {
            if change.new["secondary_color"].is_null() {
                one
            } else {
                gradient
            }
        }
        Rule::Canceled(first, second) => {
            if change.old.is_null() && change.new != true {
                return None;
            }
            if !truthy(&change.old) && truthy(&change.new) {
                first
            } else if truthy(&change.old) && !truthy(&change.new) {
                second
            } else {
                return None;
            }
        }
    })
}

/// `ei`: which permission bits an entry's change adds and removes, in the table's order.
fn permission_diff(old: &Value, new: &Value) -> (Vec<u64>, Vec<u64>) {
    let bits = |value: &Value| {
        value
            .as_str()
            .and_then(|text| text.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let (old, new) = (bits(old), bits(new));
    let added = new & !old;
    let removed = old & !new;
    let pick = |mask: u64| -> Vec<u64> {
        PERMISSIONS
            .iter()
            .map(|(bit, _, _)| *bit)
            .filter(|bit| mask & bit == *bit)
            .collect()
    };
    (pick(added), pick(removed))
}

/// `el`.
fn permission_name(bit: u64, target: &str) -> String {
    PERMISSIONS
        .iter()
        .find(|(known, _, _)| *known == bit)
        .map(|(_, channel, other)| {
            if matches!(target, "CHANNEL" | "CHANNEL_OVERWRITE")
                && let Some(channel) = channel
            {
                plain_message(channel)
            } else {
                plain_message(other)
            }
        })
        .unwrap_or_default()
}

/// `Hl`: a colour number as `#RRGGBB`.
fn hex_colour(value: u64) -> String {
    format!("#{value:06X}")
}

/// The whole page's state.
#[derive(Default)]
pub(crate) struct AuditState {
    raw_users: HashMap<String, User>,
    entries: Vec<Entry>,
    /// `k`: names of deleted things, by target type and id.
    deleted: HashMap<String, String>,
    integrations: Vec<Value>,
    webhooks: Vec<Value>,
    events: Vec<Value>,
    rules: Vec<Value>,
    threads: Vec<Value>,
    commands: Vec<Value>,
    loading: bool,
    loading_next: bool,
    loaded: bool,
    error: bool,
    has_older: bool,
    grouped: usize,
    expanded: Option<String>,
    action_filter: Option<u64>,
    user_filter: Option<String>,
    open_select: Option<Select>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Select {
    User,
    Action,
}

/// `cn`'s query.
fn query(before: Option<&str>, user: Option<&str>, action_id: Option<u64>) -> String {
    let mut parts = vec![format!("limit={PAGE}")];
    if let Some(before) = before {
        parts.push(format!("before={before}"));
    }
    if let Some(user) = user {
        parts.push(format!("user_id={user}"));
    }
    if let Some(action_id) = action_id {
        parts.push(format!("action_type={action_id}"));
    }
    parts.join("&")
}

impl Root {
    fn audit_mut(&mut self) -> Option<&mut AuditState> {
        self.server_settings.as_mut().map(|open| &mut open.audit)
    }

    /// Escape: the Filter by User / Filter by Action list closes before the page does.
    pub(super) fn close_audit_layer(&mut self) -> bool {
        self.audit_mut()
            .is_some_and(|state| state.open_select.take().is_some())
    }

    /// `ci`.
    pub(super) fn request_audit(&mut self, guild: Id) {
        let Some(state) = self.audit_mut() else {
            return;
        };
        if state.loading || state.loading_next {
            return;
        }
        state.loading = true;
        let route = format!(
            "/guilds/{guild}/audit-logs?{}",
            query(None, state.user_filter.as_deref(), state.action_filter)
        );
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{AUDIT_PREFIX}{guild}")),
            method: reqwest::Method::GET,
            route,
            body: None,
        });
    }

    /// `cl`.
    fn request_audit_next(&mut self, guild: Id, grouped: bool) {
        let Some(state) = self.audit_mut() else {
            return;
        };
        if !state.has_older || state.loading || state.loading_next {
            return;
        }
        let before = state.entries.last().map(|entry| entry.id.clone());
        state.loading_next = true;
        if grouped {
            state.grouped += 1;
        }
        let route = format!(
            "/guilds/{guild}/audit-logs?{}",
            query(
                before.as_deref(),
                state.user_filter.as_deref(),
                state.action_filter
            )
        );
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{NEXT_PREFIX}{guild}")),
            method: reqwest::Method::GET,
            route,
            body: None,
        });
    }

    pub(super) fn audit_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        let next = key.starts_with(NEXT_PREFIX);
        let body = report["body"].clone();
        let logs: Vec<Value> = body["audit_log_entries"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let users: Vec<User> = body["users"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(User::from_value)
            .collect();
        for user in &users {
            let (key, url) = avatar_source(user);
            self.request_image(key, url);
        }
        for action_id in logs
            .iter()
            .filter_map(|log| log["action_type"].as_u64())
            .collect::<Vec<u64>>()
        {
            let url = icon_url(action_id, target_type(action_id));
            self.request_image(url.to_owned(), url.to_owned());
        }
        for url in [OVERLAY_CREATE, OVERLAY_DELETE, OVERLAY_UPDATE] {
            self.request_image(url.to_owned(), url.to_owned());
        }
        for url in ICON_URLS {
            self.request_image((*url).to_owned(), (*url).to_owned());
        }
        let Some(state) = self.audit_mut() else {
            return;
        };
        for user in users {
            state.raw_users.insert(user.id.to_string(), user);
        }
        let list = |name: &str| body[name].as_array().cloned().unwrap_or_default();
        state.integrations = list("integrations");
        state.webhooks = list("webhooks");
        state.events = list("guild_scheduled_events");
        state.rules = list("auto_moderation_rules");
        state.threads = list("threads");
        state.commands = list("application_commands");
        state.loaded = true;
        if next {
            state.loading_next = false;
            if logs.len() < PAGE {
                state.has_older = false;
            }
            if !logs.is_empty() {
                let mut deleted = std::mem::take(&mut state.deleted);
                let rows = build_entries(&logs, &mut deleted);
                state.deleted = deleted;
                state.entries.extend(rows);
            }
        } else {
            state.loading = false;
            state.grouped = 0;
            if report["ok"] == true {
                state.error = false;
                state.has_older = logs.len() >= PAGE;
                let mut deleted = std::mem::take(&mut state.deleted);
                state.entries = build_entries(&logs, &mut deleted);
                state.deleted = deleted;
            } else {
                state.error = true;
                state.entries.clear();
            }
        }
        cx.notify();
    }

    fn audit_user(&self, id: &str) -> Option<User> {
        let state = &self.server_settings.as_ref()?.audit;
        if let Some(user) = state.raw_users.get(id) {
            return Some(user.clone());
        }
        let parsed = id.parse::<Id>().ok()?;
        if self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == parsed)
        {
            return self.stores.current_user.clone();
        }
        self.stores
            .friends()
            .into_iter()
            .find(|user| user.id == parsed)
            .cloned()
    }

    /// The channel an entry's menu is about (module 989433): its target for a channel or overwrite entry
    /// (`onTargetContextMenu`), else the channel of its options (`onChannelContextMenu`); only a channel the
    /// server still has (module 194195 takes a channel record).
    fn audit_menu_channel(&self, guild: Id, entry: &Entry) -> Option<Id> {
        let known = self.stores.guild(guild)?;
        let exists = |id: Option<&str>| {
            let id = id?.parse::<Id>().ok()?;
            known.channels.iter().any(|c| c.id == id).then_some(id)
        };
        if matches!(target_type(entry.action), "CHANNEL" | "CHANNEL_OVERWRITE")
            && let Some(id) = exists(entry.target_id.as_deref())
        {
            return Some(id);
        }
        exists(entry.options["channel_id"].as_str())
    }

    /// `m1` with its hash, for a channel by id.
    fn audit_channel(&self, guild: Id, id: &str) -> Option<String> {
        let known = self.stores.guild(guild)?;
        let parsed = id.parse::<Id>().ok()?;
        if let Some(channel) = known.channels.iter().find(|channel| channel.id == parsed) {
            return Some(match channel.kind {
                ChannelKind::Text | ChannelKind::Other(5 | 15 | 16) => format!("#{}", channel.name),
                _ => channel.name.clone(),
            });
        }
        known
            .threads
            .iter()
            .find(|thread| thread.id == parsed)
            .map(|thread| format!("\"{}\"", thread.name))
    }

    /// `ec`: the live thing's name, a deleted thing's name, the entry's own value, or the id.
    fn audit_named(&self, entry: &Entry, change_key: &str, live: Option<String>) -> String {
        let target = target_type(entry.action);
        let id = entry.target_id.clone().unwrap_or_default();
        if let Some(live) = live {
            return live;
        }
        let state = self.server_settings.as_ref().map(|open| &open.audit);
        if let Some(saved) = state.and_then(|state| state.deleted.get(&format!("{target}:{id}"))) {
            return saved.clone();
        }
        if let Some(change) = entry.changes.iter().find(|change| change.key == change_key) {
            let value = if change.new.is_null() || change.new == "" {
                &change.old
            } else {
                &change.new
            };
            if !value.is_null() {
                return value_text(value);
            }
        }
        id
    }

    /// `ea`'s first half: the row's target, in words.
    fn audit_target(&self, guild: Id, entry: &Entry) -> String {
        let id = entry.target_id.as_deref().unwrap_or_default();
        let known = self.stores.guild(guild);
        let state = self.server_settings.as_ref().map(|open| &open.audit);
        let from = |list: Option<&Vec<Value>>, key: &str| -> Option<String> {
            list?
                .iter()
                .find(|item| item["id"].as_str() == Some(id))
                .and_then(|item| item[key].as_str().map(str::to_owned))
        };
        match target_type(entry.action) {
            "GUILD"
            | "GUILD_HOME"
            | "GUILD_PROFILE"
            | "GUILD_ONBOARDING"
            | "GUILD_MEMBER_VERIFICATION" => {
                known.map(|known| known.name.clone()).unwrap_or_default()
            }
            "CHANNEL" | "CHANNEL_OVERWRITE" => {
                self.audit_named(entry, "name", self.audit_channel(guild, id))
            }
            "VOICE_CHANNEL_STATUS" => {
                self.audit_named(entry, "status", self.audit_channel(guild, id))
            }
            "USER" => {
                let user = self.audit_user(id);
                self.audit_named(entry, "nick", user.map(|user| user.username))
            }
            "ROLE" => {
                let live = id
                    .parse::<Id>()
                    .ok()
                    .and_then(|role| known.and_then(|known| known.roles.get(&role).cloned()));
                self.audit_named(entry, "name", live)
            }
            "ONBOARDING_PROMPT" => {
                let named = self.audit_named(entry, "id", None);
                if named.is_empty() {
                    plain_message("ZNQyiR")
                } else {
                    named
                }
            }
            "INVITE" => self.audit_named(entry, "code", None),
            "INTEGRATION" => {
                let live = from(state.map(|state| &state.integrations), "name");
                self.audit_named(entry, "type", live)
            }
            "WEBHOOK" => {
                let live = from(state.map(|state| &state.webhooks), "name");
                self.audit_named(entry, "name", live)
            }
            "EMOJI" => {
                let live = id.parse::<Id>().ok().and_then(|emoji| {
                    known.and_then(|known| {
                        known
                            .emojis
                            .iter()
                            .find(|known| known.id == emoji)
                            .map(|emoji| emoji.name.clone())
                    })
                });
                self.audit_named(entry, "name", live)
            }
            "STICKER" => self.audit_named(entry, "name", None),
            "STAGE_INSTANCE" => self.audit_named(entry, "topic", None),
            "GUILD_SCHEDULED_EVENT" | "GUILD_SCHEDULED_EVENT_EXCEPTION" => {
                let live = from(state.map(|state| &state.events), "name");
                self.audit_named(entry, "name", live)
            }
            "THREAD" => {
                let live = from(state.map(|state| &state.threads), "name");
                self.audit_named(entry, "name", live)
            }
            "APPLICATION_COMMAND" => {
                if entry.options["application_id"].as_str() == Some(id) {
                    let integration = state.and_then(|state| {
                        state
                            .integrations
                            .iter()
                            .find(|item| item["application"]["id"].as_str() == Some(id))
                            .and_then(|item| item["name"].as_str().map(str::to_owned))
                    });
                    integration.unwrap_or_else(|| id.to_owned())
                } else {
                    let live = state.and_then(|state| {
                        state
                            .commands
                            .iter()
                            .find(|item| item["id"].as_str() == Some(id))
                            .map(|item| {
                                let name = item["name_localized"]
                                    .as_str()
                                    .filter(|name| !name.is_empty())
                                    .or(item["name"].as_str())
                                    .unwrap_or_default();
                                if item["type"] == 1 {
                                    format!("/\u{2060}{name}")
                                } else {
                                    name.to_owned()
                                }
                            })
                    });
                    self.audit_named(entry, "name", live)
                }
            }
            "AUTO_MODERATION_RULE" => {
                let live = from(state.map(|state| &state.rules), "name");
                self.audit_named(entry, "name", live)
            }
            "GUILD_SOUNDBOARD" => self.audit_named(entry, "name", None),
            "HOME_SETTINGS" => plain_message("VbpLyU"),
            _ => id.to_owned(),
        }
    }

    /// `ee`.
    fn audit_title_id(entry: &Entry) -> Option<&'static str> {
        use action::*;
        let change_of = |key: &str| entry.changes.iter().find(|change| change.key == key);
        let moved = |legacy: &'static str,
                     named: &'static str,
                     users: &'static str,
                     overflow: &'static str| {
            let count = entry.options["unique_count"]
                .as_str()
                .and_then(|count| count.parse::<i64>().ok())
                .or(entry.options["unique_count"].as_i64());
            match count {
                None => legacy,
                Some(count) if count < 1 => legacy,
                _ if entry.options["unique_overflow"] == "1" => overflow,
                Some(1) if !entry.options["member_id"].is_null() => named,
                _ => users,
            }
        };
        match entry.action {
            CHANNEL_CREATE => Some(
                match change_of("type").and_then(|change| change.new.as_u64()) {
                    Some(13) => "OKp4+o",
                    Some(2) => "NPOy4G",
                    Some(4) => "T3KIjz",
                    Some(15) => "VvNgHX",
                    Some(16) => "4NWSxa",
                    Some(5) => "eYP6UV",
                    _ => "wrYNG2",
                },
            ),
            MEMBER_MOVE => Some(moved("Yt6NkU", "VW2XVd", "hEqea3", "qXazAb")),
            MEMBER_DISCONNECT => Some(moved("K4eCZw", "tTwGD8", "lODaeu", "xuGjhY")),
            STAGE_INSTANCE_DELETE => Some(if entry.user_id.is_some() {
                "Oi/in9"
            } else {
                "7ZIFm9"
            }),
            THREAD_CREATE => Some(
                match change_of("type").and_then(|change| change.new.as_u64()) {
                    Some(12) => "Br0y5w",
                    Some(10) => "6uaMmO",
                    _ => "2cxQ7G",
                },
            ),
            AUTO_MODERATION_FLAG_TO_CHANNEL => Some("SD0PwJ"),
            GUILD_HOME_FEATURE_ITEM => Some(
                match change_of("entity_type").and_then(|change| change.new.as_str()) {
                    Some("message") => "PyEa+J",
                    Some("forum_post") => "hCuAb1",
                    _ => "UZ+U3A",
                },
            ),
            other => TITLES
                .iter()
                .find(|(known, _)| *known == other)
                .map(|(_, id)| *id),
        }
    }

    /// What `getChangeTitle` is formatted with.
    fn audit_title(&self, guild: Id, entry: &Entry) -> Vec<Span> {
        let Some(id) = Self::audit_title_id(entry) else {
            return Vec::new();
        };
        let user = entry
            .user_id
            .as_deref()
            .and_then(|user| self.audit_user(user));
        let mut args: HashMap<&str, String> = HashMap::new();
        args.insert(
            "user",
            user.map(|user| {
                if user.discriminator == "0" {
                    user.username
                } else {
                    format!("{}#{}", user.username, user.discriminator)
                }
            })
            .unwrap_or_else(|| plain_message("30mdIx")),
        );
        args.insert("target", self.audit_target(guild, entry));
        let options = &entry.options;
        let count = options["members_removed"]
            .as_str()
            .and_then(|count| count.parse::<i64>().ok())
            .or(options["members_removed"].as_i64())
            .or(options["count"]
                .as_str()
                .and_then(|count| count.parse().ok()))
            .or(options["count"].as_i64());
        if let Some(count) = count {
            args.insert("count", count.to_string());
        }
        if let Some(unique) = options["unique_count"].as_str() {
            args.insert("uniqueCount", unique.to_owned());
        }
        if let Some(member) = options["member_id"].as_str() {
            args.insert(
                "member",
                self.audit_user(member)
                    .map(|user| user.username)
                    .unwrap_or_else(|| plain_message("30mdIx")),
            );
        }
        if let Some(channel) = options["channel_id"].as_str() {
            args.insert(
                "channel",
                self.audit_channel(guild, channel)
                    .unwrap_or_else(|| channel.to_owned()),
            );
        }
        let subtarget = match options["type"].as_str() {
            Some("1") => options["id"].as_str().map(|user| {
                self.audit_user(user)
                    .map(|user| user.username)
                    .unwrap_or_else(|| user.to_owned())
            }),
            Some("0") => options["role_name"].as_str().map(str::to_owned),
            _ => None,
        };
        if let Some(subtarget) = subtarget {
            args.insert("subtarget", subtarget);
        }
        format_message(id, &args, count)
    }

    /// `ea`'s change half: a raw change as the row shows it, sometimes several.
    fn audit_shape_change(&self, guild: Id, entry: &Entry, change: &Change) -> Vec<Change> {
        let map_both = |run: &dyn Fn(&Value) -> Option<String>| {
            let mut shaped = change.clone();
            if let Some(text) = run(&change.new) {
                shaped.new = Value::from(text);
            }
            if let Some(text) = run(&change.old) {
                shaped.old = Value::from(text);
            }
            vec![shaped]
        };
        let is_rule = target_type(entry.action) == "AUTO_MODERATION_RULE";
        match change.key.as_str() {
            "owner_id" => {
                map_both(&|value| self.audit_user(value.as_str()?).map(|user| user.username))
            }
            "channel_id"
            | "afk_channel_id"
            | "system_channel_id"
            | "rules_channel_id"
            | "public_updates_channel_id" => {
                map_both(&|value| self.audit_channel(guild, value.as_str()?))
            }
            "afk_timeout" => map_both(&|value| Some((value.as_u64()? / 60).to_string())),
            "bitrate" => map_both(&|value| Some((value.as_u64()? / 1000).to_string())),
            "color" => map_both(&|value| Some(hex_colour(value.as_u64()?))),
            "preferred_locale" => map_both(&|value| {
                let wanted = value.as_str()?;
                LOCALES
                    .iter()
                    .find(|(locale, _)| *locale == wanted)
                    .map(|(_, name)| (*name).to_owned())
            }),
            "video_quality_mode" => map_both(&|value| {
                Some(if value.as_u64()? == 2 {
                    plain_message("7jOoJE")
                } else {
                    plain_message("jjKYpu")
                })
            }),
            "permissions" => {
                let (added, removed) = permission_diff(&change.old, &change.new);
                let mut out = Vec::new();
                if !added.is_empty() {
                    out.push(Change::new(
                        "permissions_granted",
                        Value::Null,
                        Value::from(added),
                    ));
                }
                if !removed.is_empty() {
                    out.push(Change::new(
                        "permissions_denied",
                        Value::Null,
                        Value::from(removed),
                    ));
                }
                out
            }
            "allow" | "deny" | "permissions_granted" | "permissions_denied" => {
                let (added, removed) = permission_diff(&change.old, &change.new);
                let key = if change.key == "allow" {
                    "permissions_granted"
                } else if change.key == "deny" {
                    "permissions_denied"
                } else {
                    change.key.as_str()
                };
                let mut out = Vec::new();
                if !added.is_empty() {
                    out.push(Change::new(key, Value::Null, Value::from(added)));
                }
                if !removed.is_empty() {
                    out.push(Change::new(
                        "permissions_reset",
                        Value::from(removed.clone()),
                        Value::from(removed),
                    ));
                }
                out
            }
            "flags" => {
                let bits = |value: &Value| value.as_u64().unwrap_or(0);
                let (old, new) = (bits(&change.old), bits(&change.new));
                let mut out = Vec::new();
                let added = new & !old;
                let removed = old & !new;
                let of = |mask: u64| -> Vec<u64> {
                    ADDED_FLAGS
                        .iter()
                        .map(|(flag, _)| *flag)
                        .filter(|flag| mask & flag == *flag)
                        .collect()
                };
                if !of(added).is_empty() {
                    out.push(Change::new("flags", Value::Null, Value::from(of(added))));
                }
                if !of(removed).is_empty() {
                    out.push(Change::new("flags", Value::from(of(removed)), Value::Null));
                }
                if out.is_empty() {
                    vec![change.clone()]
                } else {
                    out
                }
            }
            "system_channel_flags" => {
                const FLAGS: [(u64, &str); 4] = [
                    (1 << 0, "system_channel_flag_join_notifications"),
                    (1 << 1, "system_channel_flag_premium_subscriptions"),
                    (1 << 2, "system_channel_flag_reminder_notifications"),
                    (1 << 3, "system_channel_flag_join_notification_replies"),
                ];
                let bits = |value: &Value| value.as_u64().unwrap_or(0);
                let (old, new) = (bits(&change.old), bits(&change.new));
                FLAGS
                    .iter()
                    .filter(|(flag, _)| old & flag != new & flag)
                    .map(|(flag, key)| {
                        Change::new(
                            key,
                            Value::from(old & flag == 0),
                            Value::from(new & flag == 0),
                        )
                    })
                    .collect()
            }
            "auto_moderation_add_keywords"
            | "auto_moderation_remove_keywords"
            | "auto_moderation_add_regex_patterns"
            | "auto_moderation_remove_regex_patterns"
            | "auto_moderation_add_allow_list"
            | "auto_moderation_remove_allow_list"
                if is_rule =>
            {
                map_both(&|value| {
                    Some(
                        value
                            .as_array()?
                            .iter()
                            .map(|item| format!("'{}'", value_text(item)))
                            .collect::<Vec<_>>()
                            .join(", "),
                    )
                })
            }
            "auto_moderation_exempt_channels" if is_rule => map_both(&|value| {
                Some(
                    value
                        .as_array()?
                        .iter()
                        .filter_map(|channel| self.audit_channel(guild, channel.as_str()?))
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            }),
            "auto_moderation_exempt_roles" if is_rule => map_both(&|value| {
                let known = self.stores.guild(guild)?;
                Some(
                    value
                        .as_array()?
                        .iter()
                        .filter_map(|role| {
                            known
                                .roles
                                .get(&role.as_str()?.parse::<Id>().ok()?)
                                .cloned()
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            }),
            "scheduled_start_time" | "scheduled_end_time" => map_both(&|value| {
                let time = chrono::DateTime::parse_from_rfc3339(value.as_str()?).ok()?;
                Some(
                    time.with_timezone(&chrono::Local)
                        .format("%A, %B %-d, %Y %-I:%M %p")
                        .to_string(),
                )
            }),
            _ => {
                let mut kept = change.clone();
                if entry.action == action::APPLICATION_COMMAND_PERMISSION_UPDATE {
                    let value = if change.new.is_null() {
                        &change.old
                    } else {
                        &change.new
                    };
                    let id = value["id"].as_str().unwrap_or_default();
                    kept.subtarget = match value["type"].as_u64() {
                        Some(1) => id
                            .parse::<Id>()
                            .ok()
                            .and_then(|role| self.stores.guild(guild)?.roles.get(&role).cloned()),
                        Some(2) => self.audit_user(id).map(|user| user.username),
                        Some(3) => self.audit_channel(guild, id),
                        _ => None,
                    };
                }
                vec![kept]
            }
        }
    }

    /// The expanded list (`r`): numbered, and sub-lists under roles, permissions and flags.
    fn audit_details(&self, guild: Id, entry: &Entry) -> AnyElement {
        let tint = match action_kind(entry.action) {
            Kind::Create => "--text-feedback-positive",
            Kind::Delete => "--text-feedback-critical",
            _ => "--text-feedback-warning",
        };
        let target = target_type(entry.action);
        let mut column = div().px(px(8.)).pb(px(10.)).flex().flex_col();
        let mut number = 0;
        let shaped: Vec<Change> = entry
            .changes
            .iter()
            .flat_map(|change| self.audit_shape_change(guild, entry, change))
            .collect();
        for change in &shaped {
            if hidden_change(entry, change) {
                continue;
            }
            number += 1;
            let Some(id) = change_message(entry, change) else {
                continue;
            };
            let mut args: HashMap<&str, String> = HashMap::new();
            args.insert("oldValue", value_text(&change.old));
            args.insert("newValue", value_text(&change.new));
            if let Some(subtarget) = change
                .subtarget
                .clone()
                .or(entry.options["subtarget"].as_str().map(str::to_owned))
            {
                args.insert("subtarget", subtarget);
            }
            if change.key == "colors" {
                let colours: Vec<String> = ["primary_color", "secondary_color", "tertiary_color"]
                    .iter()
                    .filter_map(|key| change.new[*key].as_u64().map(hex_colour))
                    .collect();
                args.insert("newColors", colours.join(","));
            }
            let count = change.new.as_array().map_or(0, |list| list.len() as i64);
            let spans = format_message(id, &args, Some(count));
            let list: Vec<String> = if entry.action == action::MEMBER_ROLE_UPDATE
                || (entry.action == action::INVITE_CREATE && change.key == "role_ids")
            {
                change
                    .new
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter_map(|role| role["name"].as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default()
            } else if target == "ROLE"
                || matches!(
                    entry.action,
                    action::CHANNEL_OVERWRITE_CREATE | action::CHANNEL_OVERWRITE_UPDATE
                )
            {
                change
                    .new
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter_map(Value::as_u64)
                            .map(|bit| permission_name(bit, target))
                            .collect()
                    })
                    .unwrap_or_default()
            } else if matches!(entry.action, action::CHANNEL_UPDATE | action::THREAD_UPDATE)
                && change.key == "flags"
            {
                let of = |value: &Value, table: &[(u64, &str)]| -> Vec<String> {
                    value
                        .as_array()
                        .map(|list| {
                            list.iter()
                                .filter_map(Value::as_u64)
                                .filter_map(|flag| table.iter().find(|(known, _)| *known == flag))
                                .map(|(_, id)| plain_message(id))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let mut all = of(&change.old, REMOVED_FLAGS);
                all.extend(of(&change.new, ADDED_FLAGS));
                all
            } else {
                Vec::new()
            };
            column = column.child(
                div()
                    .mt(px(8.))
                    .ml(px(40.))
                    .flex()
                    .items_baseline()
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(700.))
                            .font_family(super::MONO)
                            .text_color(color(tint))
                            .flex()
                            .child(format!("{number:02}"))
                            .child(div().mx(px(10.)).child("\u{2014}")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w(px(0.))
                            .child(Self::audit_spans(spans, 16.))
                            .line_height(px(16.))
                            .children(list.into_iter().map(|line| {
                                div()
                                    .mt(px(4.))
                                    .text_size(px(16.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-default"))
                                    .child(line)
                            })),
                    ),
            );
        }
        column.into_any_element()
    }

    /// A formatted message as one line of text with its strong parts.
    fn audit_spans(spans: Vec<Span>, size: f32) -> AnyElement {
        let mut text = String::new();
        let mut styles: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
        for span in spans {
            let start = text.len();
            match span {
                Span::Plain(part) => text.push_str(&part),
                Span::Strong(part) => {
                    text.push_str(&part);
                    styles.push((
                        start..text.len(),
                        HighlightStyle {
                            color: Some(color("--text-strong").into()),
                            font_weight: Some(FontWeight(500.)),
                            ..Default::default()
                        },
                    ));
                }
                Span::Swatch(colour) => {
                    text.push_str(" \u{25CF}");
                    let parsed = u32::from_str_radix(colour.trim_start_matches('#'), 16).ok();
                    if let Some(parsed) = parsed {
                        styles.push((
                            start..text.len(),
                            HighlightStyle {
                                color: Some(gpui::rgb(parsed).into()),
                                ..Default::default()
                            },
                        ));
                    }
                }
            }
        }
        div()
            .text_size(px(size))
            .text_color(color("--text-default"))
            .child(StyledText::new(text).with_highlights(styles))
            .into_any_element()
    }

    /// `x` in module 438407: the kind's picture with its create, update or delete mark.
    fn audit_icon(&self, action_id: u64, target: &str, kind: Kind) -> AnyElement {
        let picture = self.images.get(icon_url(action_id, target)).cloned();
        let overlay = match kind {
            Kind::Create => Some(OVERLAY_CREATE),
            Kind::Update => Some(OVERLAY_UPDATE),
            Kind::Delete => Some(OVERLAY_DELETE),
            Kind::All => None,
        }
        .and_then(|url| self.images.get(url).cloned());
        div()
            .relative()
            .size(px(24.))
            .flex_none()
            .children(picture.map(|picture| img(picture).absolute().inset_0().size(px(24.))))
            .children(overlay.map(|overlay| img(overlay).absolute().inset_0().size(px(24.))))
            .into_any_element()
    }

    /// One row (`cx`).
    fn audit_row(
        &self,
        guild: Id,
        entry: &Entry,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let can_open = has_details(entry);
        let now = chrono::Local::now();
        let (start, end) = (calendar(entry.start_ms, now), calendar(entry.end_ms, now));
        let time = if start == end {
            start
        } else {
            format!("{start}\u{2014}{end}")
        };
        let user = entry
            .user_id
            .as_deref()
            .and_then(|user| self.audit_user(user));
        let avatar = user
            .as_ref()
            .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
        let target = target_type(entry.action);
        let id = entry.id.clone();
        let menu_channel = self.audit_menu_channel(guild, entry);
        let header = div()
            .id(SharedString::from(format!("audit-row-{}", entry.id)))
            .h(px(60.))
            .pl(px(10.))
            .pr(px(20.))
            .flex()
            .items_center()
            .when(expanded, |this| {
                this.rounded(px(4.)).bg(color("--card-secondary-bg"))
            })
            .when(can_open, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(state) = this.audit_mut() {
                            state.expanded = if state.expanded.as_deref() == Some(id.as_str()) {
                                None
                            } else {
                                Some(id.clone())
                            };
                        }
                        cx.notify();
                    }))
            })
            .child(self.audit_icon(entry.action, target, action_kind(entry.action)))
            .child(
                div()
                    .ml(px(10.))
                    .size(px(40.))
                    .flex_none()
                    .rounded_full()
                    .children(avatar.map(|avatar| img(avatar).size(px(40.)).rounded_full())),
            )
            .child(
                div()
                    .mx(px(10.))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .line_height(px(20.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            // GAP: the official title opens the channel menu from the channel's own words only;
                            // here a right press anywhere on the title line opens it.
                            .when_some(menu_channel, |this, channel| {
                                this.on_mouse_down(
                                    gpui::MouseButton::Right,
                                    cx.listener(
                                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.open_context_menu(
                                                super::context_menu::Target::AuditChannel(channel),
                                                event.position,
                                                cx,
                                            );
                                        },
                                    ),
                                )
                            })
                            .child(Self::audit_spans(self.audit_title(guild, entry), 16.)),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(time),
                    ),
            )
            .when(can_open, |this| {
                // `ro.A`: the arrow turns down when open.
                this.child(
                    gpui::svg()
                        .path("icons/audit_expand.svg")
                        .size(px(24.))
                        .flex_none()
                        .text_color(color("--interactive-text-default"))
                        .with_transformation(gpui::Transformation::rotate(gpui::percentage(
                            if expanded { 0.25 } else { 0. },
                        ))),
                )
            });
        div()
            .mb(px(8.))
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--card-background-default"))
            .child(header)
            .when(expanded, |this| {
                this.child(self.audit_details(guild, entry))
            })
            .into_any_element()
    }

    /// The moderators the user filter lists (`G`): members with a role that moderates; the list knows only those loaded.
    fn audit_moderators(&self, guild: Id) -> Vec<User> {
        let Some(known) = self.stores.guild(guild) else {
            return Vec::new();
        };
        let Some(list) = self.stores.member_lists.get(&guild) else {
            return Vec::new();
        };
        list.rows
            .iter()
            .filter_map(|row| match row {
                dc_state::member_list::Row::Member { user, roles, .. } => Some((user, roles)),
                _ => None,
            })
            .filter(|(user, roles)| {
                roles.iter().any(|role| {
                    Some(user.id) == known.owner_id
                        || known
                            .role_permissions
                            .get(role)
                            .is_some_and(|bits| bits & MODERATOR_BITS != 0)
                })
            })
            .map(|(user, _)| user.clone())
            .collect()
    }

    /// A closed filter (`e2.Z`): its label over the box.
    fn audit_select(
        &self,
        which: Select,
        label: String,
        value: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self
            .server_settings
            .as_ref()
            .is_some_and(|open| open.audit.open_select == Some(which));
        let key = match which {
            Select::User => "audit-select-user",
            Select::Action => "audit-select-action",
        };
        div()
            .w(px(222.))
            .flex_none()
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label),
            )
            .child(
                div()
                    .id(key)
                    .relative()
                    .child(self.anchor_probe(key))
                    .h(px(40.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if open {
                        color("--text-brand")
                    } else {
                        color("--input-border-default")
                    })
                    .bg(color("--input-background-default"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(value),
                    )
                    .child(
                        gpui::svg()
                            .path("icons/select_chevron.svg")
                            .size(px(24.))
                            .text_color(color("--interactive-text-default"))
                            .with_transformation(gpui::Transformation::rotate(gpui::percentage(
                                if open { 0.5 } else { 0. },
                            ))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(state) = this.audit_mut() {
                            state.open_select = if state.open_select == Some(which) {
                                None
                            } else {
                                Some(which)
                            };
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// The open filter's list, 210 at most.
    pub(super) fn audit_select_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if open.page != super::server_settings::Page::AuditLog {
            return None;
        }
        let state = &open.audit;
        let which = state.open_select?;
        let guild = open.guild;
        let key = match which {
            Select::User => "audit-select-user",
            Select::Action => "audit-select-action",
        };
        let field = self.tip_anchors.borrow().get(key).copied()?.dilate(px(1.));
        let selected_mark =
            |selected: bool| selected.then(|| icon("select_check", 20., "--text-brand"));
        let row = |id: String, leading: AnyElement, label: String, selected: bool| {
            div()
                .id(SharedString::from(format!("audit-option-{id}")))
                .flex_none()
                .h(px(40.))
                .px(px(8.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|style| style.bg(color("--background-mod-normal")))
                .flex()
                .items_center()
                .gap(px(4.))
                .child(leading)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
                .children(selected_mark(selected))
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        match which {
            Select::Action => {
                rows.push(
                    row(
                        "all-actions".to_owned(),
                        self.audit_icon(0, "ALL", Kind::All),
                        plain_message("QxEVcv"),
                        // All's value is null, which no selection matches.
                        false,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_audit_filter(guild, None, None, true, cx);
                    }))
                    .into_any_element(),
                );
                for (action_id, label) in ACTION_FILTERS {
                    let action_id = *action_id;
                    rows.push(
                        row(
                            action_id.to_string(),
                            self.audit_icon(
                                action_id,
                                target_type(action_id),
                                action_kind(action_id),
                            ),
                            plain_message(label),
                            state.action_filter == Some(action_id),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_audit_filter(guild, Some(action_id), None, true, cx);
                        }))
                        .into_any_element(),
                    );
                }
            }
            Select::User => {
                rows.push(
                    row(
                        "all-users".to_owned(),
                        icon("audit_group", 30., "--interactive-text-default").into_any_element(),
                        plain_message("ZRFdsL"),
                        false,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_audit_filter(guild, None, None, false, cx);
                    }))
                    .into_any_element(),
                );
                for user in self.audit_moderators(guild) {
                    let id = user.id.to_string();
                    let chosen = id.clone();
                    let picture = self.images.get(&avatar_source(&user).0).cloned();
                    rows.push(
                        row(
                            id.clone(),
                            div()
                                .size(px(24.))
                                .flex_none()
                                .rounded_full()
                                .children(
                                    picture
                                        .map(|picture| img(picture).size(px(24.)).rounded_full()),
                                )
                                .into_any_element(),
                            user.username.clone(),
                            state.user_filter.as_deref() == Some(id.as_str()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_audit_filter(guild, None, Some(chosen.clone()), false, cx);
                        }))
                        .into_any_element(),
                    );
                }
            }
        }
        let menu_h = ((rows.len() as f32) * 40. + 10.).min(210.);
        let space_below = (self.viewport.1 - f32::from(field.bottom()) - 12.).max(0.);
        let space_above = (f32::from(field.top()) - 12.).max(0.);
        let top = if space_below < menu_h && space_above > space_below {
            (field.top() - px(4.) - px(menu_h)).max(px(8.))
        } else {
            field.bottom() + px(4.)
        };
        Some(
            deferred(
                div()
                    .id("audit-select-layer")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(top)
                    .w(field.size.width)
                    .max_h(px(210.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![super::shadow_high()])
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(
                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                            if !field.contains(&event.position)
                                && let Some(state) = this.audit_mut()
                            {
                                state.open_select = None;
                                cx.notify();
                            }
                        },
                    ))
                    .child(div().flex().flex_col().children(rows)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// `handleFilterActionChange`, `handleFilterUserChange`.
    fn set_audit_filter(
        &mut self,
        guild: Id,
        action_id: Option<u64>,
        user: Option<String>,
        by_action: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.audit_mut() {
            if state.loading || state.loading_next {
                return;
            }
            if by_action {
                state.action_filter = action_id;
            } else {
                state.user_filter = user;
            }
            state.open_select = None;
            state.expanded = None;
        }
        self.request_audit(guild);
        cx.notify();
    }

    /// The page (`cA`).
    pub(super) fn server_audit_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.server_settings.as_ref() else {
            return div().into_any_element();
        };
        let state = &open.audit;
        let user_value = state
            .user_filter
            .as_deref()
            .and_then(|user| self.audit_user(user))
            .map_or_else(|| plain_message("ZRFdsL"), |user| user.username);
        let action_value = state
            .action_filter
            .and_then(|action_id| ACTION_FILTERS.iter().find(|(known, _)| *known == action_id))
            .map_or_else(
                || plain_message("QxEVcv"),
                |(_, label)| plain_message(label),
            );
        let header = div()
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(plain_message("SPWLyT")),
            )
            .child(self.audit_select(Select::User, plain_message("hxnY/q"), user_value, cx))
            .child(div().w(px(16.)))
            .child(self.audit_select(Select::Action, plain_message("rautds"), action_value, cx));
        let entries = state.entries.clone();
        let expanded = state.expanded.clone();
        let (loaded, loading, has_older, grouped, error, loading_next) = (
            state.loaded,
            state.loading,
            state.has_older,
            state.grouped,
            state.error,
            state.loading_next,
        );
        let body: AnyElement = if !loaded || loading {
            div().into_any_element()
        } else if entries.is_empty() {
            div()
                .mt(px(60.))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-muted"))
                        .child(plain_message(if error { "Ww5Tjy" } else { "RHhk+P" })),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .max_w(px(300.))
                        .text_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child(plain_message(if error { "tzkaD7" } else { "lNuYhh" })),
                )
                .into_any_element()
        } else {
            let rows: Vec<AnyElement> = entries
                .iter()
                .map(|entry| {
                    self.audit_row(
                        guild,
                        entry,
                        expanded.as_deref() == Some(entry.id.as_str()),
                        cx,
                    )
                })
                .collect();
            div().flex().flex_col().children(rows).into_any_element()
        };
        // `componentDidUpdate`: while the list's end is in view, it loads on, three times, then asks.
        let tail_key = "audit-tail";
        let tail_seen = self
            .tip_anchors
            .borrow()
            .get(tail_key)
            .is_some_and(|tail| f32::from(tail.top()) < 720.);
        if tail_seen && loaded && has_older && !loading && !loading_next && grouped <= 2 {
            cx.spawn(async move |this, cx| {
                let _ = this.update(cx, |this, _| this.request_audit_next(guild, true));
            })
            .detach();
        }
        div()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .mt(px(20.))
                    .mb(px(20.))
                    .h(px(1.))
                    .bg(color("--border-subtle")),
            )
            .child(body)
            .when(has_older && grouped > 2, |this| {
                this.child(
                    div().mt(px(16.)).flex().justify_center().child(
                        div()
                            .id("audit-load-more")
                            .h(px(40.))
                            .px(px(15.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(plain_message("Q/LSXp"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request_audit_next(guild, false);
                                cx.notify();
                            })),
                    ),
                )
            })
            .child(
                div()
                    .h(px(1.))
                    .relative()
                    .child(self.anchor_probe(tail_key)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn messages_are_sorted_for_their_lookup() {
        assert!(MESSAGES.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn titles_format_with_their_user_and_target() {
        let mut args = HashMap::new();
        args.insert("user", "someone".to_owned());
        args.insert("target", "#general".to_owned());
        let spans = format_message("l5Cu1a", &args, None);
        assert_eq!(
            spans,
            vec![
                Span::Plain("someone".to_owned()),
                Span::Plain(" created channel overrides for ".to_owned()),
                Span::Strong("#general".to_owned()),
            ]
        );
    }

    #[test]
    fn plurals_pick_their_form() {
        let mut args = HashMap::new();
        args.insert("user", "x".to_owned());
        args.insert("count", "3".to_owned());
        let spans = format_message("qKOZTP", &args, Some(3));
        assert_eq!(spans.last(), Some(&Span::Strong(" members".to_owned())));
        args.insert("count", "1".to_owned());
        let spans = format_message("qKOZTP", &args, Some(1));
        assert_eq!(spans.last(), Some(&Span::Strong("a member".to_owned())));
    }

    fn raw(id: u64, action_id: u64, name: &str) -> Value {
        json!({
            "id": (id << 22).to_string(),
            "action_type": action_id,
            "target_id": "5",
            "user_id": "7",
            "changes": [{"key": "name", "old_value": "old", "new_value": name}],
        })
    }

    #[test]
    fn close_entries_of_one_kind_merge_with_their_changes() {
        let mut deleted = HashMap::new();
        // Newest first, one minute apart.
        let rows = build_entries(
            &[
                raw(120_000, action::ROLE_UPDATE, "c"),
                raw(60_000, action::ROLE_UPDATE, "b"),
                raw(0, action::CHANNEL_UPDATE, "a"),
            ],
            &mut deleted,
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].action, action::ROLE_UPDATE);
        assert_eq!(rows[0].changes.len(), 2);
        assert!(rows[0].end_ms > rows[0].start_ms);
    }

    #[test]
    fn entries_far_apart_stay_apart() {
        let mut deleted = HashMap::new();
        let minutes = 31 * 60_000;
        let rows = build_entries(
            &[
                raw(minutes, action::ROLE_UPDATE, "b"),
                raw(0, action::ROLE_UPDATE, "a"),
            ],
            &mut deleted,
        );
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn a_deleted_channel_keeps_its_name_with_a_hash() {
        let mut deleted = HashMap::new();
        let item = json!({
            "id": "1", "action_type": action::CHANNEL_DELETE, "target_id": "5",
            "changes": [
                {"key": "name", "old_value": "general"},
                {"key": "type", "old_value": 0},
            ],
        });
        build_entries(&[item], &mut deleted);
        assert_eq!(deleted["CHANNEL:5"], "#general");
    }

    #[test]
    fn deleted_things_list_only_their_reason() {
        let entry = Entry {
            id: "1".to_owned(),
            action: action::ROLE_DELETE,
            target_id: None,
            user_id: None,
            options: json!({}),
            changes: vec![
                Change::new("name", json!("a"), Value::Null),
                Change::new("reason", Value::Null, json!("spam")),
            ],
            start_ms: 0,
            end_ms: 0,
        };
        assert!(hidden_change(&entry, &entry.changes[0]));
        assert!(!hidden_change(&entry, &entry.changes[1]));
        assert!(has_details(&entry));
    }

    #[test]
    fn change_rules_follow_the_helpers() {
        let entry = |action_id| Entry {
            id: "1".to_owned(),
            action: action_id,
            target_id: None,
            user_id: None,
            options: json!({}),
            changes: Vec::new(),
            start_ms: 0,
            end_ms: 0,
        };
        let channel = entry(action::CHANNEL_UPDATE);
        assert_eq!(
            change_message(&channel, &Change::new("name", Value::Null, json!("a"))),
            Some("f8Rh0U")
        );
        assert_eq!(
            change_message(&channel, &Change::new("name", json!("a"), json!("b"))),
            Some("ebD4Qp")
        );
        // The module's rule for verification: `Enabled join settings` only for a new value of true.
        let verification = entry(action::GUILD_MEMBER_VERIFICATION_UPDATE);
        let key = CHANGE_RULES
            .iter()
            .find(|(target, _, rule)| {
                *target == "GUILD_MEMBER_VERIFICATION" && matches!(rule, Rule::NewTrue("fnkzDY", _))
            })
            .map(|(_, key, _)| *key)
            .unwrap();
        assert_eq!(
            change_message(&verification, &Change::new(key, Value::Null, json!(true))),
            Some("fnkzDY")
        );
    }

    #[test]
    fn permissions_diff_in_the_tables_order() {
        let (added, removed) = permission_diff(&json!("2"), &json!("4"));
        assert_eq!(added, vec![4]);
        assert_eq!(removed, vec![2]);
        assert_eq!(permission_name(8, "ROLE"), "Administrator");
    }

    #[test]
    fn the_query_names_only_its_filters() {
        assert_eq!(query(None, None, None), "limit=50");
        assert_eq!(
            query(Some("9"), Some("7"), Some(10)),
            "limit=50&before=9&user_id=7&action_type=10"
        );
    }

    #[test]
    fn calendar_words_follow_moment() {
        use chrono::TimeZone;
        let now = chrono::Local
            .with_ymd_and_hms(2026, 11, 8, 12, 0, 0)
            .unwrap();
        let at = |day, hour| {
            chrono::Local
                .with_ymd_and_hms(2026, 11, day, hour, 5, 0)
                .unwrap()
                .timestamp_millis()
        };
        assert_eq!(calendar(at(8, 9), now), "Today at 9:05 AM");
        assert_eq!(calendar(at(7, 9), now), "Yesterday at 9:05 AM");
        assert_eq!(calendar(at(4, 9), now), "Last Wednesday at 9:05 AM");
        assert_eq!(calendar(at(1, 9), now), "11/01/2026");
    }
}
