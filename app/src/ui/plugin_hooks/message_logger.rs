//! MessageLogger (Vencord `src/plugins/messageLogger`, GPL-3.0-or-later): a deleted message stays where it was
//! (`handleDelete`, through `Stores::keep_deleted`), drawn in red (`deleteStyle` text: the row's text, muted, strong
//! and link colours become the danger reds; overlay: a 15% red background), its pictures grey until hovered, and its
//! hover bar down to "Clear history". An edit keeps the text it replaced (`editHistory`, with the new edit's time):
//! with `inlineEdits` each earlier version is drawn above the content, dimmed, with its own " (edited)" mark; the
//! message's own "(edited)" opens "Message Edit History" (tabs by time, Show Diff). With `logDeletedAttachments` an
//! edit that drops attachments keeps them, grey. `showEditDiffs` colours what each version added (green) and removed
//! (red) against the next one (the original's LCS over characters, emoji and mentions as one); `separatedDiffs` shows
//! the original with its removals and the content with its additions instead. The message menu of a message with
//! history drops the quick reactions and the rows a deleted message cannot use, and ends with Toggle Deleted
//! Highlight, Enable/Disable Diff View and Clear Message History; channel, thread, DM and group DM menus get Clear
//! Message Log. `collapseDeleted` folds deleted messages like blocked ones ("N deleted messages"). Messages by bots,
//! yourself and the listed users, channels (or their category) and servers are let go as they were (`shouldIgnore`),
//! and so is an ephemeral message. The up arrow's last editable message skips deleted ones. Not part of the pinned
//! client.
//!
//! The original's `normalizeNonce` guards a message replaced in place by a new one with the same nonce; the store here
//! never replaces a message by its nonce, so there is nothing to guard. Its `+:emoji:` target fix waits for the base's
//! own `+:emoji:` rule (BetterPlusReacts' gap).
//!
//! GAP: emoji inside a diff get no green or red film over them (the original's `::after`), and a deleted message's
//! embeds keep their own title colour (`--embed-title`, which the app's embeds do not read).

use super::super::Root;
use chrono::{DateTime, Utc};
use dc_state::{Id, Message};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

const ID: &str = "MessageLogger";
/// `MessageFlags.EPHEMERAL`.
const EPHEMERAL: u64 = 64;
/// `VENBOT_USER_ID` and `VC_SUPPORT_CATEGORY_ID`, `EQUIBOT_USER_ID` and `SUPPORT_CHANNEL_ID`.
const VENBOT: Id = 1_017_176_847_865_352_332;
const VC_SUPPORT_CATEGORY: Id = 1_108_135_649_699_180_705;
const EQUIBOT: Id = 1_243_063_117_852_835_941;
const SUPPORT_CHANNEL: Id = 1_297_590_739_911_573_585;

/// One logged edit: when the next version was made, and the text it replaced.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Edit {
    pub at: DateTime<Utc>,
    pub content: String,
}

#[derive(Default)]
struct Logged {
    deleted: HashSet<Id>,
    history: HashMap<Id, Vec<Edit>>,
    /// `firstEditTimestamp`: the message's edit time (else its time) when it was first seen.
    first_edit: HashMap<Id, DateTime<Utc>>,
    deleted_attachments: HashSet<Id>,
    /// The attachments of the deleted messages, by the message they belong to.
    attachment_owner: HashMap<Id, Id>,
    /// `disabledDiffMessages`.
    diff_off: HashSet<Id>,
    /// Toggle Deleted Highlight took the class off.
    highlight_off: HashSet<Id>,
    /// The ids the delete being dispatched keeps (decided before the store sees it).
    keep: HashSet<Id>,
    /// The attachments of the message an update is about, before it.
    before_update: Option<(Id, Vec<dc_state::Attachment>)>,
    /// The open history dialog: the message, its tab and Show Diff.
    dialog: Option<(Id, usize, bool)>,
}

thread_local! {
    static LOGGED: RefCell<Logged> = RefCell::new(Logged::default());
}

/// A deleted message's colours are being drawn (`.messagelogger-deleted` with the text style).
static SCOPED: AtomicBool = AtomicBool::new(false);

/// The plugin's settings.
#[derive(Clone, Debug, Default)]
struct Settings {
    log_deletes: bool,
    log_edits: bool,
    log_deleted_attachments: bool,
    inline_edits: bool,
    ignore_bots: bool,
    ignore_self: bool,
    ignore_self_edits: bool,
    ignore_users: String,
    ignore_channels: String,
    ignore_guilds: String,
    show_edit_diffs: bool,
    separated_diffs: bool,
}

fn settings() -> Option<Settings> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(|state| {
        if !state.is_on(ID) {
            return None;
        }
        let flag = |name: &str| state.value(ID, name) == Value::Bool(true);
        let text = |value: Value| value.as_str().unwrap_or_default().to_owned();
        let show_edit_diffs = flag("showEditDiffs");
        let ignore_self = flag("ignoreSelf");
        Some(Settings {
            log_deletes: flag("logDeletes"),
            log_edits: flag("logEdits"),
            log_deleted_attachments: flag("logDeletedAttachments"),
            inline_edits: flag("inlineEdits"),
            ignore_bots: flag("ignoreBots"),
            ignore_self,
            // `disabled()` while ignoreSelf is on.
            ignore_self_edits: !ignore_self && flag("ignoreSelfEdits"),
            ignore_users: text(state.value(ID, "ignoreUsers")),
            ignore_channels: text(state.value(ID, "ignoreChannels")),
            ignore_guilds: text(state.value(ID, "ignoreGuilds")),
            show_edit_diffs,
            // `disabled()` while showEditDiffs is off (and turned off with it).
            separated_diffs: show_edit_diffs && flag("separatedDiffs"),
        })
    })
}

/// Whether the plugin is on (no setting read: the rows ask this each frame).
pub(in crate::ui) fn message_logger_on() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("MessageLogger")
}

/// `deleteStyle` is the overlay.
fn overlay_style() -> bool {
    crate::plugins::with(|state| state.value(ID, "deleteStyle") == "overlay")
}

/// `Stores::keep_deleted`: what the delete being dispatched keeps.
fn keep(message: &Message) -> bool {
    LOGGED.with(|logged| {
        let mut logged = logged.borrow_mut();
        let kept = logged.keep.contains(&message.id);
        if kept {
            logged.deleted.insert(message.id);
            for attachment in &message.attachments {
                logged.attachment_owner.insert(attachment.id, message.id);
            }
        }
        kept
    })
}

/// A deleted message the plugin keeps.
pub(in crate::ui) fn message_logger_deleted(message: Id) -> bool {
    message_logger_on() && LOGGED.with(|logged| logged.borrow().deleted.contains(&message))
}

/// A deleted message whose highlight is on, and whether it is drawn with the overlay.
pub(in crate::ui) fn message_logger_highlight(message: Id) -> Option<bool> {
    if !message_logger_on() {
        return None;
    }
    let lit = LOGGED.with(|logged| {
        let logged = logged.borrow();
        logged.deleted.contains(&message) && !logged.highlight_off.contains(&message)
    });
    lit.then(overlay_style)
}

/// A picture drawn grey: an attachment an edit took away, or one of a deleted message in its highlight.
pub(in crate::ui) fn message_logger_attachment_gray(attachment: Id) -> bool {
    if !message_logger_on() {
        return false;
    }
    LOGGED.with(|logged| {
        let logged = logged.borrow();
        logged.deleted_attachments.contains(&attachment)
            || logged
                .attachment_owner
                .get(&attachment)
                .is_some_and(|message| {
                    logged.deleted.contains(message) && !logged.highlight_off.contains(message)
                })
    })
}

/// `doesMessageHaveHistory`.
fn has_history(logged: &Logged, message: &Message) -> bool {
    logged.deleted.contains(&message.id)
        || logged
            .history
            .get(&message.id)
            .is_some_and(|edits| !edits.is_empty())
        || message
            .attachments
            .iter()
            .any(|attachment| logged.deleted_attachments.contains(&attachment.id))
}

/// Whether a message has history while the plugin is on (the hover bar's Clear history, the menus' rows).
pub(in crate::ui) fn message_logger_history(message: &Message) -> bool {
    message_logger_on() && LOGGED.with(|logged| has_history(&logged.borrow(), message))
}

/// The logged edits of a message.
pub(in crate::ui) fn message_logger_edits(message: Id) -> Vec<Edit> {
    if !message_logger_on() {
        return Vec::new();
    }
    LOGGED.with(|logged| {
        logged
            .borrow()
            .history
            .get(&message)
            .cloned()
            .unwrap_or_default()
    })
}

/// Whether diffs are drawn for this message (`showEditDiffs` and not turned off for it).
fn diffs_for(settings: &Settings, message: Id) -> bool {
    settings.show_edit_diffs && LOGGED.with(|logged| !logged.borrow().diff_off.contains(&message))
}

/// The message menu's state: deleted, has history, and the diff toggle (whether it is off) when diffs are on.
pub(in crate::ui) fn message_logger_menu(message: &Message) -> Option<(bool, Option<bool>)> {
    if !message_logger_history(message) {
        return None;
    }
    let settings = settings()?;
    LOGGED.with(|logged| {
        let logged = logged.borrow();
        Some((
            logged.deleted.contains(&message.id),
            settings
                .show_edit_diffs
                .then(|| logged.diff_off.contains(&message.id)),
        ))
    })
}

/// `--status-danger` and `--red-460` over the tokens `.messagelogger-deleted` sets (`deleteStyleText.css`).
pub(in crate::ui) fn scoped_token(name: &str) -> Option<&'static str> {
    if !SCOPED.load(Ordering::Relaxed) {
        return None;
    }
    match name {
        "--text-default" | "--interactive-icon-default" | "--text-muted" => Some("--status-danger"),
        "--embed-title" | "--text-link" | "--text-strong" => Some("--red-460"),
        _ => None,
    }
}

/// While alive, the deleted message's text colours are drawn (the row of a deleted message in the text style).
pub(in crate::ui) struct Scope(bool);

impl Drop for Scope {
    fn drop(&mut self) {
        if self.0 {
            SCOPED.store(false, Ordering::Relaxed);
        }
    }
}

/// The colour scope of a message's row.
pub(in crate::ui) fn message_logger_scope(message: Id) -> Scope {
    let red = message_logger_highlight(message) == Some(false);
    if red {
        SCOPED.store(true, Ordering::Relaxed);
    }
    Scope(red)
}

/// `createWordDiff`'s parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Part {
    Added,
    Removed,
    Unchanged,
}

/// `tokenizeMessage`: custom emoji and mentions as one token, anything else a character.
fn tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        // `text.slice(i + 1, i + 3) === ":"` never holds (two characters against one): only `<a:` opens an emoji.
        let opens = rest.starts_with("<a:") || rest.starts_with("<@") || rest.starts_with("<#");
        if opens && let Some(end) = rest.find('>') {
            out.push(&rest[..=end]);
            at += end + 1;
            continue;
        }
        let width = rest.chars().next().map_or(1, char::len_utf8);
        out.push(&rest[..width]);
        at += width;
    }
    out
}

/// `createWordDiff(old, new)`.
pub(in crate::ui) fn diff(old: &str, new: &str) -> Vec<(Part, String)> {
    // The lengths are UTF-16 lengths, as in the original.
    let units = |text: &str| text.encode_utf16().count();
    if units(old) < units(new) && new.starts_with(old) {
        let mut parts = Vec::new();
        if !old.is_empty() {
            parts.push((Part::Unchanged, old.to_owned()));
        }
        parts.push((Part::Added, new[old.len()..].to_owned()));
        return parts;
    }
    if units(old) < units(new) && new.ends_with(old) {
        let mut parts = vec![(Part::Added, new[..new.len() - old.len()].to_owned())];
        if !old.is_empty() {
            parts.push((Part::Unchanged, old.to_owned()));
        }
        return parts;
    }
    if units(new) < units(old) && old.starts_with(new) {
        let mut parts = Vec::new();
        if !new.is_empty() {
            parts.push((Part::Unchanged, new.to_owned()));
        }
        parts.push((Part::Removed, old[new.len()..].to_owned()));
        return parts;
    }
    if units(new) < units(old) && old.ends_with(new) {
        let mut parts = vec![(Part::Removed, old[..old.len() - new.len()].to_owned())];
        if !new.is_empty() {
            parts.push((Part::Unchanged, new.to_owned()));
        }
        return parts;
    }
    let a = tokens(old);
    let b = tokens(new);
    let mut table = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            table[i][j] = if a[i - 1] == b[j - 1] {
                table[i - 1][j - 1] + 1
            } else {
                table[i - 1][j].max(table[i][j - 1])
            };
        }
    }
    let (mut i, mut j) = (a.len(), b.len());
    let mut parts: Vec<(Part, &str)> = Vec::new();
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
            parts.push((Part::Unchanged, a[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || table[i][j - 1] > table[i - 1][j]) {
            parts.push((Part::Added, b[j - 1]));
            j -= 1;
        } else if i > 0 {
            parts.push((Part::Removed, a[i - 1]));
            i -= 1;
        }
    }
    parts.reverse();
    let mut grouped: Vec<(Part, String)> = Vec::new();
    for (part, text) in parts {
        match grouped.last_mut() {
            Some((last, joined)) if *last == part => joined.push_str(text),
            _ => grouped.push((part, text.to_owned())),
        }
    }
    grouped
}

/// `buildViewSegments`: the original leaves out what was added, the update what was removed.
fn view(parts: &[(Part, String)], original: bool) -> Vec<(Part, String)> {
    parts
        .iter()
        .filter(|(part, _)| *part != if original { Part::Added } else { Part::Removed })
        .cloned()
        .collect()
}

/// `shouldIgnore` with what it reads of the stores.
fn ignored(
    settings: &Settings,
    message: &Message,
    me: Option<Id>,
    parent: Option<Id>,
    guild: Option<Id>,
    edit: bool,
) -> bool {
    let author = message.author.id;
    // `ignoreUsers.includes(id)`: the comma-separated text holds it anywhere.
    let holds = |list: &str, id: Option<Id>| id.is_some_and(|id| list.contains(&id.to_string()));
    (settings.ignore_bots && message.author.bot)
        || (settings.ignore_self && Some(author) == me)
        || (settings.ignore_self_edits && edit && Some(author) == me)
        || holds(&settings.ignore_users, Some(author))
        || holds(&settings.ignore_channels, Some(message.channel_id))
        || holds(&settings.ignore_channels, parent)
        || if edit {
            !settings.log_edits
        } else {
            !settings.log_deletes
        }
        || holds(&settings.ignore_guilds, guild)
        || (author == VENBOT && parent == Some(VC_SUPPORT_CATEGORY))
        || (author == EQUIBOT && message.channel_id == SUPPORT_CHANNEL)
}

/// `handleUpdateAttachments`: the old attachments, each the new one or the old marked deleted, then the new ones.
fn merged_attachments(
    old: &[dc_state::Attachment],
    new: &[dc_state::Attachment],
) -> (Vec<dc_state::Attachment>, Vec<Id>) {
    let mut gone = Vec::new();
    let mut out: Vec<dc_state::Attachment> = old
        .iter()
        .map(
            |before| match new.iter().find(|after| after.id == before.id) {
                Some(after) => after.clone(),
                None => {
                    gone.push(before.id);
                    before.clone()
                }
            },
        )
        .collect();
    out.extend(
        new.iter()
            .filter(|after| !old.iter().any(|before| before.id == after.id))
            .cloned(),
    );
    (out, gone)
}

impl Root {
    /// The channel's category and server, for `shouldIgnore`.
    fn message_logger_place(&self, channel: Id) -> (Option<Id>, Option<Id>) {
        let guild = self.guild_of_channel(channel);
        let parent = guild.and_then(|guild| {
            let known = self.stores.guild(guild)?;
            match known.channels.iter().find(|known| known.id == channel) {
                Some(found) => found.parent_id,
                None => known
                    .threads
                    .iter()
                    .find(|known| known.id == channel)
                    .map(|thread| thread.parent_id),
            }
        });
        (parent, guild)
    }

    fn message_logger_ignored(&self, settings: &Settings, message: &Message, edit: bool) -> bool {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let (parent, guild) = self.message_logger_place(message.channel_id);
        ignored(settings, message, me, parent, guild, edit)
    }

    /// A message of the stores by its channel and id, in the list or the window's caches.
    fn message_logger_find(&self, channel: Id, message: Id) -> Option<&Message> {
        let window = self.stores.windows.get(&channel);
        self.stores
            .messages
            .get(&channel)
            .into_iter()
            .flatten()
            .chain(window.into_iter().flat_map(|window| {
                window
                    .before_cache()
                    .messages()
                    .iter()
                    .chain(window.after_cache().messages().iter())
            }))
            .find(|known| known.id == message)
    }

    /// Before the stores see a dispatch: what a delete keeps, what an edit replaces.
    pub(in crate::ui) fn message_logger_before(&mut self, name: &str, data: &Value) {
        let settings = settings();
        self.stores.keep_deleted = settings.as_ref().map(|_| keep as fn(&Message) -> bool);
        let Some(settings) = settings else {
            return;
        };
        let channel = data["channel_id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok());
        match name {
            "MESSAGE_DELETE" | "MESSAGE_DELETE_BULK" => {
                let Some(channel) = channel else {
                    return;
                };
                let ids: Vec<Id> = if name == "MESSAGE_DELETE" {
                    data["id"]
                        .as_str()
                        .and_then(|id| id.parse().ok())
                        .into_iter()
                        .collect()
                } else {
                    data["ids"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|id| id.as_str()?.parse().ok())
                        .collect()
                };
                // `mlDeleted`: the plugin's own Clear removes it for good.
                let cleared = data["mlDeleted"] == Value::Bool(true);
                let keep: HashSet<Id> = ids
                    .into_iter()
                    .filter(|id| {
                        !cleared
                            && self
                                .message_logger_find(channel, *id)
                                .is_some_and(|message| {
                                    message.flags & EPHEMERAL != EPHEMERAL
                                        && !self.message_logger_ignored(&settings, message, false)
                                })
                    })
                    .collect();
                LOGGED.with(|logged| logged.borrow_mut().keep = keep);
            }
            "MESSAGE_UPDATE" => {
                let (Some(channel), Some(id)) = (
                    channel,
                    data["id"].as_str().and_then(|id| id.parse::<Id>().ok()),
                ) else {
                    return;
                };
                let Some(known) = self.message_logger_find(channel, id) else {
                    return;
                };
                let first_seen = known.edited_at.unwrap_or(known.timestamp);
                let skip = data["flags"].as_u64().unwrap_or(0) & EPHEMERAL == EPHEMERAL
                    || self.message_logger_ignored(&settings, known, true);
                let old_content = known.content.clone();
                let old_attachments = known.attachments.clone();
                let edited = data["edited_timestamp"]
                    .as_str()
                    .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                    .map(|at| at.with_timezone(&Utc));
                LOGGED.with(|logged| {
                    let mut logged = logged.borrow_mut();
                    logged.first_edit.entry(id).or_insert(first_seen);
                    // `edited_timestamp && content !== m.content`.
                    if !skip
                        && let Some(at) = edited
                        && data["content"].as_str() != Some(old_content.as_str())
                    {
                        logged.history.entry(id).or_default().push(Edit {
                            at,
                            content: old_content,
                        });
                    }
                    logged.before_update = (!skip && settings.log_deleted_attachments)
                        .then_some((id, old_attachments));
                });
            }
            _ => {}
        }
    }

    /// After the stores saw a dispatch: an edit's dropped attachments come back, marked deleted.
    pub(in crate::ui) fn message_logger_after(&mut self, name: &str, data: &Value) {
        if name != "MESSAGE_UPDATE" {
            if name == "MESSAGE_DELETE" || name == "MESSAGE_DELETE_BULK" {
                LOGGED.with(|logged| logged.borrow_mut().keep.clear());
            }
            return;
        }
        let Some((id, old)) = LOGGED.with(|logged| logged.borrow_mut().before_update.take()) else {
            return;
        };
        // An update without the attachments leaves them as they are (`attachments ?? []` is only read with them).
        let Some(listed) = data["attachments"].as_array() else {
            return;
        };
        let new: Vec<dc_state::Attachment> = listed
            .iter()
            .filter_map(dc_state::Attachment::from_value)
            .collect();
        let (merged, gone) = if new.is_empty() {
            (
                old.clone(),
                old.iter().map(|attachment| attachment.id).collect(),
            )
        } else {
            merged_attachments(&old, &new)
        };
        let Some(channel) = data["channel_id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok())
        else {
            return;
        };
        if let Some(list) = self.stores.messages.get_mut(&channel) {
            let window = self.stores.windows.entry(channel).or_default();
            window.update(list, id, |message| message.attachments = merged.clone());
        }
        LOGGED.with(|logged| logged.borrow_mut().deleted_attachments.extend(gone));
    }

    /// `clearMessageHistory`: a deleted message goes for good, another loses its edits and deleted attachments.
    pub(in crate::ui) fn message_logger_clear(&mut self, channel: Id, message: Id) {
        let deleted = LOGGED.with(|logged| logged.borrow_mut().deleted.remove(&message));
        if deleted {
            let data = serde_json::json!({
                "id": message.to_string(),
                "channel_id": channel.to_string(),
                "mlDeleted": true,
            });
            self.message_logger_before("MESSAGE_DELETE", &data);
            self.stores.dispatch("MESSAGE_DELETE", &data);
            self.message_logger_after("MESSAGE_DELETE", &data);
            LOGGED.with(|logged| {
                let mut logged = logged.borrow_mut();
                logged.history.remove(&message);
                logged.highlight_off.remove(&message);
            });
        } else {
            let gone = LOGGED.with(|logged| {
                let mut logged = logged.borrow_mut();
                logged.history.remove(&message);
                logged.deleted_attachments.clone()
            });
            if let Some(list) = self.stores.messages.get_mut(&channel) {
                let window = self.stores.windows.entry(channel).or_default();
                window.update(list, message, |known| {
                    known
                        .attachments
                        .retain(|attachment| !gone.contains(&attachment.id));
                });
            }
        }
        self.rebuild_rows();
    }

    /// "Clear Message Log": every message of the channel with history.
    pub(in crate::ui) fn message_logger_clear_channel(&mut self, channel: Id) {
        let with_history: Vec<Id> = self
            .stores
            .messages
            .get(&channel)
            .into_iter()
            .flatten()
            .filter(|message| message_logger_history(message))
            .map(|message| message.id)
            .collect();
        for message in with_history {
            self.message_logger_clear(channel, message);
        }
    }

    /// Whether a channel's menu offers Clear Message Log (a message of it has history).
    pub(in crate::ui) fn message_logger_channel_has_history(&self, channel: Id) -> bool {
        message_logger_on()
            && self
                .stores
                .messages
                .get(&channel)
                .is_some_and(|messages| messages.iter().any(message_logger_history))
    }

    /// The channel a menu's Clear Message Log is about: the channel, thread or DM, or for a person's menu the
    /// channel it was opened in (`channel-context`, `thread-context`, `gdm-context`, `user-context`).
    pub(in crate::ui) fn message_logger_menu_channel(
        &self,
        target: super::super::context_menu::Target,
    ) -> Option<Id> {
        use super::super::context_menu::Target;
        match target {
            Target::Channel(channel) | Target::Thread(channel) | Target::Dm(channel) => {
                Some(channel)
            }
            Target::User { .. } => self.channel,
            _ => None,
        }
    }

    /// The menus' Clear Message Log, in the "mark-channel-read" group when there is one, else at the end.
    pub(in crate::ui) fn with_message_logger_row(
        &self,
        mut items: Vec<super::super::context_menu::Entry>,
        target: super::super::context_menu::Target,
    ) -> Vec<super::super::context_menu::Entry> {
        use super::super::context_menu::{Act, Entry, Row};
        let Some(channel) = self.message_logger_menu_channel(target) else {
            return items;
        };
        if !self.message_logger_channel_has_history(channel) {
            return items;
        }
        let mut row = Row::new(Act::PluginClearMessageLog, "Clear Message Log");
        row.danger = true;
        let row = Entry::Row(row);
        match items
            .iter()
            .position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::MarkRead))
        {
            Some(at) => {
                let end = items[at..]
                    .iter()
                    .position(|entry| matches!(entry, Entry::Separator))
                    .map_or(items.len(), |end| at + end);
                items.insert(end, row);
            }
            None => {
                if !items.is_empty() {
                    items.push(Entry::Separator);
                }
                items.push(row);
            }
        }
        items
    }

    /// Toggle Deleted Highlight.
    pub(in crate::ui) fn message_logger_toggle_highlight(&mut self, message: Id) {
        LOGGED.with(|logged| {
            let mut logged = logged.borrow_mut();
            if !logged.highlight_off.remove(&message) {
                logged.highlight_off.insert(message);
            }
        });
    }

    /// Enable / Disable Diff View.
    pub(in crate::ui) fn message_logger_toggle_diff(&mut self, message: Id) {
        LOGGED.with(|logged| {
            let mut logged = logged.borrow_mut();
            if !logged.diff_off.remove(&message) {
                logged.diff_off.insert(message);
            }
        });
    }

    /// Whether deleted messages fold into "N deleted messages" (`collapseDeleted`).
    pub(in crate::ui) fn message_logger_collapses(&self) -> bool {
        message_logger_on()
            && crate::plugins::with(|state| state.value(ID, "collapseDeleted") == Value::Bool(true))
    }

    /// The edit marker's click: "Message Edit History" on the current version.
    pub(in crate::ui) fn message_logger_open_history(&mut self, message: Id) {
        if settings().is_none() {
            return;
        }
        let show = settings().is_some_and(|settings| settings.show_edit_diffs);
        let tab = LOGGED.with(|logged| logged.borrow().history.get(&message).map_or(0, Vec::len));
        LOGGED.with(|logged| logged.borrow_mut().dialog = Some((message, tab, show)));
    }

    /// Spans of `text` read as markdown, with `\n` between blocks, each in `tint` (a diff part's colour, if any).
    fn message_logger_pieces(
        &self,
        text: &str,
        tint: gpui::Hsla,
        size: f32,
        emoji: f32,
    ) -> (Vec<crate::inline::Piece>, Vec<Option<(String, String)>>) {
        let mut spans: Vec<dc_markdown::Span> = Vec::new();
        for (index, block) in dc_markdown::parse(text).iter().enumerate() {
            if index > 0 {
                spans.push(dc_markdown::Span {
                    text: "\n".to_owned(),
                    style: dc_markdown::Style::default(),
                    kind: dc_markdown::Kind::Text,
                });
            }
            match block {
                dc_markdown::Block::Code { text, .. } => spans.push(dc_markdown::Span {
                    text: text.clone(),
                    style: dc_markdown::Style {
                        code: true,
                        ..dc_markdown::Style::default()
                    },
                    kind: dc_markdown::Kind::Text,
                }),
                other => spans.extend(super::super::block_spans(other).iter().cloned()),
            }
        }
        let links = super::super::link_runs(&spans);
        let pieces = spans
            .iter()
            .map(|span| self.piece(span, tint, size, 400., emoji))
            .collect();
        (pieces, links)
    }

    /// `parseEditContent`: a version's text, with its diff against `next` when diffs are drawn.
    fn message_logger_version(
        &self,
        message: &Message,
        content: &str,
        next: Option<&str>,
        diffs: bool,
        dim: bool,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        use gpui::prelude::FluentBuilder as _;
        use gpui::{IntoElement, ParentElement, Styled, div, px};
        let size = self.chat_font_size();
        let line = (size * 1.375).round();
        let emoji = (22. * size / 16.).round();
        let light = super::super::theme() == dc_tokens::Theme::Light;
        // `.messagelogger-edited`: `brightness(80%)` on dark themes, half opacity on the light one.
        let shade = |tint: gpui::Hsla| {
            if dim && !light {
                gpui::Hsla {
                    l: tint.l * 0.8,
                    ..tint
                }
            } else {
                tint
            }
        };
        let base = shade(super::super::color("--text-default").into());
        let green = shade(gpui::rgb(0x85e89d).into());
        let red = shade(gpui::rgb(0xf47068).into());
        let lines: Vec<Vec<(Part, String)>> = match next {
            Some(next) if diffs && next != content => {
                let parts = diff(content, next);
                if settings().is_some_and(|settings| settings.separated_diffs) {
                    let original = view(&parts, true);
                    let updated = view(&parts, false);
                    let current = next == message.content;
                    let mut out = Vec::new();
                    if !original.is_empty() {
                        out.push(original);
                    }
                    if !current && !updated.is_empty() {
                        out.push(updated);
                    }
                    out
                } else {
                    vec![parts]
                }
            }
            _ => vec![vec![(Part::Unchanged, content.to_owned())]],
        };
        let root = cx.weak_entity();
        div()
            .when(dim && light, |this| this.opacity(0.5))
            .flex()
            .flex_col()
            .gap(px(if lines.len() > 1 { 4. } else { 0. }))
            .children(lines.into_iter().map(|parts| {
                let mut pieces = Vec::new();
                let mut links = Vec::new();
                for (part, text) in parts {
                    let tint = match part {
                        Part::Added => green,
                        Part::Removed => red,
                        Part::Unchanged => base,
                    };
                    let (more, more_links) = self.message_logger_pieces(&text, tint, size, emoji);
                    pieces.extend(more);
                    links.extend(more_links);
                }
                let root = root.clone();
                crate::inline::inline(pieces, px(line))
                    .on_piece_click(move |piece, _, cx| {
                        if let Some(Some((url, label))) = links.get(piece).cloned() {
                            let _ = root.update(cx, |this, cx| this.follow_link(&url, &label, cx));
                        }
                    })
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// `renderEdits`: the earlier versions above the content, each with its " (edited)" mark.
    pub(in crate::ui) fn message_logger_edits_above(
        &self,
        message: &Message,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        use gpui::{IntoElement, ParentElement, Styled, div};
        let history = message_logger_edits(message.id);
        if history.is_empty() {
            return None;
        }
        let settings = settings()?;
        let diffs = diffs_for(&settings, message.id);
        if settings.inline_edits && diffs && settings.separated_diffs {
            // The aggregated view: the original with what it lost, and its time.
            let original = &history[0];
            let parts = diff(&original.content, &message.content);
            let shown = view(&parts, true);
            let body = if shown.is_empty() {
                self.message_logger_version(message, &original.content, None, false, true, cx)
            } else {
                self.message_logger_version(
                    message,
                    &original.content,
                    Some(&message.content),
                    true,
                    true,
                    cx,
                )
            };
            let first = LOGGED.with(|logged| {
                logged
                    .borrow()
                    .first_edit
                    .get(&message.id)
                    .copied()
                    .unwrap_or(message.timestamp)
            });
            return Some(self.message_logger_marked(message.id, 0, body, first, cx));
        }
        if !settings.inline_edits {
            return None;
        }
        let rows = history.iter().enumerate().map(|(index, edit)| {
            let next = history
                .get(index + 1)
                .map_or(message.content.as_str(), |next| next.content.as_str());
            let body =
                self.message_logger_version(message, &edit.content, Some(next), diffs, true, cx);
            self.message_logger_marked(message.id, index, body, edit.at, cx)
        });
        Some(div().flex().flex_col().children(rows).into_any_element())
    }

    /// A version with its " (edited)" mark (`Timestamp isEdited`: the full time in its tooltip).
    fn message_logger_marked(
        &self,
        message: Id,
        index: usize,
        body: gpui::AnyElement,
        at: DateTime<Utc>,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        use gpui::{InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, px};
        let mark = div()
            .id(SharedString::from(format!("ml-edited-{message}-{index}")))
            .text_size(px(10.))
            .line_height(px(22.))
            .text_color(super::super::color("--chat-text-muted"))
            // `" (" + MESSAGE_EDITED + ")"`, as the row's own mark reads.
            .child(" (edited)");
        let tip = super::super::message_tips::edited_tip(at);
        div()
            .flex()
            .flex_wrap()
            .items_end()
            .child(body)
            .child(match tip {
                Some(tip) => self
                    .with_tip(format!("ml-edited-tip-{message}-{index}"), tip, mark, cx)
                    .into_any_element(),
                None => mark.into_any_element(),
            })
            .into_any_element()
    }

    /// The content in the aggregated diff view: the current text with what it added (`customRenderedContent`).
    pub(in crate::ui) fn message_logger_content(
        &self,
        message: &Message,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let history = message_logger_edits(message.id);
        let original = history.first()?;
        let settings = settings()?;
        if !(settings.inline_edits && settings.separated_diffs && diffs_for(&settings, message.id))
        {
            return None;
        }
        let parts = view(&diff(&original.content, &message.content), false);
        let size = self.chat_font_size();
        let line = (size * 1.375).round();
        let emoji = (22. * size / 16.).round();
        let base: gpui::Hsla = super::super::color("--text-default").into();
        let green: gpui::Hsla = gpui::rgb(0x85e89d).into();
        let mut pieces = Vec::new();
        for (part, text) in parts {
            let tint = if part == Part::Added { green } else { base };
            pieces.extend(self.message_logger_pieces(&text, tint, size, emoji).0);
        }
        let _ = cx;
        Some(gpui::IntoElement::into_any_element(crate::inline::inline(
            pieces,
            gpui::px(line),
        )))
    }

    /// "Message Edit History" (size lg): Show Diff, the versions' tabs by time (the first, not logged, disabled when
    /// the message was edited before it was seen), and the chosen version.
    pub(in crate::ui) fn message_logger_dialog(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        use super::super::{color, modal};
        use gpui::prelude::FluentBuilder as _;
        use gpui::{
            InteractiveElement, IntoElement, ParentElement, SharedString,
            StatefulInteractiveElement, Styled, div, px,
        };
        let (id, tab, show) = LOGGED.with(|logged| logged.borrow().dialog)?;
        settings()?;
        let channel = self.channel?;
        let message = self.message_logger_find(channel, id)?.clone();
        let history = message_logger_edits(id);
        let first = LOGGED.with(|logged| {
            logged
                .borrow()
                .first_edit
                .get(&id)
                .copied()
                .unwrap_or_else(|| message.edited_at.unwrap_or(message.timestamp))
        });
        let mut times = vec![first];
        times.extend(history.iter().map(|edit| edit.at));
        let mut contents: Vec<String> = history.iter().map(|edit| edit.content.clone()).collect();
        contents.push(message.content.clone());
        let now = chrono::Local::now();
        let stamp =
            |at: DateTime<Utc>| super::super::header_time(at.with_timezone(&chrono::Local), now);
        // `vc-settings-tab-bar` items, as ServerInfo's tabs draw them.
        let tab_item = |key: String, label: String, selected: bool, disabled: bool| {
            div()
                .id(SharedString::from(key))
                .pb(px(14.))
                .border_b_2()
                .border_color(if selected {
                    color("--interactive-icon-active")
                } else {
                    gpui::rgba(0)
                })
                .when(!disabled, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.border_color(color("--interactive-icon-active")))
                })
                .text_size(px(16.))
                .line_height(px(14.))
                .font_weight(gpui::FontWeight(500.))
                .text_color(if selected {
                    color("--interactive-text-active")
                } else {
                    color("--interactive-icon-default")
                })
                .when(disabled, |this| this.opacity(0.5))
                .child(label)
        };
        let mut tabs: Vec<gpui::AnyElement> = Vec::new();
        if first != message.timestamp {
            let item = tab_item(
                "ml-tab-unlogged".to_owned(),
                stamp(message.timestamp),
                false,
                true,
            );
            tabs.push(
                self.with_tip(
                    "ml-tab-unlogged-tip",
                    super::super::tooltip::Tip::text(
                        "This edit state was not logged so it can't be displayed.",
                        super::super::tooltip::Side::Top,
                    ),
                    item,
                    cx,
                )
                .into_any_element(),
            );
        }
        for (index, at) in times.iter().enumerate() {
            tabs.push(
                tab_item(format!("ml-tab-{index}"), stamp(*at), index == tab, false)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        LOGGED.with(|logged| {
                            if let Some(dialog) = logged.borrow_mut().dialog.as_mut() {
                                dialog.1 = index;
                            }
                        });
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        let last = contents.len() - 1;
        let content = contents.get(tab).cloned().unwrap_or_default();
        let next = (show && tab != last).then(|| contents[last].clone());
        let shown =
            self.message_logger_version(&message, &content, next.as_deref(), show, false, cx);
        let switch = div()
            .id("ml-show-diff")
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .cursor_pointer()
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(gpui::FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Show Diff"),
            )
            .child(super::super::create_channel::switch(show))
            .on_click(cx.listener(|_, _, _, cx| {
                LOGGED.with(|logged| {
                    if let Some(dialog) = logged.borrow_mut().dialog.as_mut() {
                        dialog.2 = !dialog.2;
                    }
                });
                cx.notify();
            }));
        let body = div()
            .pb(px(24.))
            .flex()
            .flex_col()
            .child(switch)
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .flex_wrap()
                    .gap(px(16.))
                    .children(tabs),
            )
            .child(div().mt(px(20.)).child(shown))
            .into_any_element();
        let spec = modal::Spec {
            id: "ml-history-dialog",
            title: "Message Edit History".into(),
            subtitle: None,
            body: Some(body),
            buttons: Vec::new(),
            close: Box::new(|_: &mut Root, _, cx| {
                LOGGED.with(|logged| logged.borrow_mut().dialog = None);
                cx.notify();
            }),
        };
        Some(modal::render_large(spec, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(author: Id, bot: bool, channel: Id) -> Message {
        let mut message = Message::from_value(&serde_json::json!({
            "id": "10", "channel_id": channel.to_string(), "content": "hi", "type": 0,
            "timestamp": "2026-11-19T00:00:00+00:00",
            "author": {"id": author.to_string(), "username": "someone", "bot": bot}
        }))
        .unwrap();
        message.author.bot = bot;
        message
    }

    #[test]
    fn diffs_follow_create_word_diff() {
        assert_eq!(
            diff("watch", "watch this"),
            [
                (Part::Unchanged, "watch".into()),
                (Part::Added, " this".into())
            ]
        );
        assert_eq!(
            diff("prefix watch", "watch"),
            [
                (Part::Removed, "prefix ".into()),
                (Part::Unchanged, "watch".into())
            ]
        );
        assert_eq!(
            diff("cat", "cut"),
            // The original walks back from the end and takes a removal before an addition on a tie, so the
            // addition comes first once the parts are read forwards.
            [
                (Part::Unchanged, "c".into()),
                (Part::Added, "u".into()),
                (Part::Removed, "a".into()),
                (Part::Unchanged, "t".into())
            ]
        );
        assert_eq!(
            tokens("a<@12>b<:x:1>"),
            ["a", "<@12>", "b", "<", ":", "x", ":", "1", ">"]
        );
        assert_eq!(tokens("<a:x:1>"), ["<a:x:1>"]);
        let parts = diff("cat", "cut");
        assert_eq!(view(&parts, true).len(), 3);
        assert_eq!(view(&parts, false).len(), 3);
    }

    #[test]
    fn ignore_rules_follow_should_ignore() {
        let mut settings = Settings {
            log_deletes: true,
            log_edits: true,
            ..Settings::default()
        };
        let other = message(5, false, 7);
        assert!(!ignored(&settings, &other, Some(1), None, None, false));
        settings.ignore_bots = true;
        assert!(ignored(
            &settings,
            &message(5, true, 7),
            Some(1),
            None,
            None,
            false
        ));
        settings.ignore_users = "4,5".into();
        assert!(ignored(&settings, &other, Some(1), None, None, false));
        settings.ignore_users.clear();
        settings.ignore_channels = "99".into();
        assert!(ignored(&settings, &other, Some(1), Some(99), None, false));
        settings.ignore_channels.clear();
        settings.log_edits = false;
        assert!(ignored(&settings, &other, Some(1), None, None, true));
        assert!(!ignored(&settings, &other, Some(1), None, None, false));
        let venbot = message(VENBOT, false, 7);
        assert!(ignored(
            &Settings {
                log_deletes: true,
                ..Settings::default()
            },
            &venbot,
            None,
            Some(VC_SUPPORT_CATEGORY),
            None,
            false
        ));
    }

    #[test]
    fn a_kept_delete_leaves_the_message_in_its_channel() {
        fn keep_all(_: &Message) -> bool {
            true
        }
        let mut stores = dc_state::Stores::default();
        let create = |id: &str| {
            serde_json::json!({
                "id": id, "channel_id": "7", "content": "hi", "type": 0,
                "timestamp": "2026-11-19T00:00:00+00:00",
                "author": {"id": "5", "username": "someone"}
            })
        };
        stores.dispatch("MESSAGE_CREATE", &create("10"));
        stores.dispatch("MESSAGE_CREATE", &create("11"));
        stores.keep_deleted = Some(keep_all);
        stores.dispatch(
            "MESSAGE_DELETE",
            &serde_json::json!({"id": "10", "channel_id": "7"}),
        );
        assert_eq!(stores.messages[&7].len(), 2);
        stores.keep_deleted = None;
        stores.dispatch(
            "MESSAGE_DELETE",
            &serde_json::json!({"id": "10", "channel_id": "7"}),
        );
        assert_eq!(
            stores.messages[&7].iter().map(|m| m.id).collect::<Vec<_>>(),
            [11]
        );
    }

    #[test]
    fn dropped_attachments_stay_in_place() {
        let attachment = |id: &str| {
            dc_state::Attachment::from_value(&serde_json::json!({
                "id": id, "filename": "a.png", "url": "https://cdn.discordapp.com/a.png",
                "proxy_url": "https://media.discordapp.net/a.png", "size": 1
            }))
            .unwrap()
        };
        let (merged, gone) = merged_attachments(
            &[attachment("1"), attachment("2")],
            &[attachment("2"), attachment("3")],
        );
        assert_eq!(merged.iter().map(|a| a.id).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(gone, [1]);
    }
}
