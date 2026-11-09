//! Server Settings > AutoMod (module 989433 `iC`, cards `iI`, `ij`, `ip`, `iE`, `ih`, `im`, `ix`, editors `n$`, `nY`, `nW`,
//! `nx`, `nZ`, `nQ`, actions `nh`, exemptions `tz`, the bar `tR`; rule table module 850216; checks module 627794; store
//! module 364806; requests module 179832; dialogs modules 148210, 158956, 16039, 146502, 49065; menu module 345585).
//!
//! `GET /guilds/{id}/auto-moderation/rules` lists the rules, at most once in 20 seconds. Save Changes `POST`s a new rule
//! and `PATCH`es a saved one; the card's switch `PATCH`es at once; Delete Rule `DELETE`s; a change to the regex patterns
//! asks `.../rules/validate` for the errors to mark.
//! Measured in `design/dom/settings/guild-automod*-darker-cozy.json`, `modals/automod-*`, `popouts/automod-more-*`.
//!
//! Not drawn or not done:
//! - App Moderation rules (`tQ`, trigger 8) and Enforce Server Rules (trigger 7): their experiments keep them off here, so
//!   rules of those kinds are kept but not listed.
//! - The springs that open a card and slide the bar in, the Save button's spinner (it only dims), the list's loading
//!   spinner `tS` (nothing stands in the cards' place until the rules arrive), and the page's scrollbars.
//! - A text area grows with its text instead of scrolling inside its height, and dragging its corner cannot make it
//!   smaller than its text.
//! - A regex tag's error tooltip shows on hover only, not for its first five seconds or while it is edited; a tag being
//!   edited is a fixed 500 wide rather than as wide as its text. The live check of a word list runs on every edit
//!   rather than through the 300 ms debounce. The pointer-only `isSelecting` look and blink of a tag are not drawn.
//! - Copy AutoMod Rule ID: developer mode is not in the settings store yet.
//! - `AUTOMOD_MENTION_SPAM`'s deep link that opens the mention rule directly. Clear Mention Raid and the alert actions
//!   belong to the alert message, not this page.
//! - Announcement channels use the text glyph, forum and media channels the forum glyph, in the exemption list.
//! - `localeCompare` when sorting word lists is approximated by case-folded comparison.
//! - A failed save puts the rule back, where the official optimistic update (`tR`) only does so if the request throws,
//!   which `saveRule` never lets it do.
//!
//! Not run: Community servers' parts (the Members section with `nQ`, the mention raid checkbox, Cannot delete rule): the
//! fixture is not a Community server, and the sidebar hides this page for one.

use super::tooltip::{Body, Side, Tip};
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent, EditorStyle};
use crate::net::Request;
use dc_state::{Channel, ChannelKind, Id, permissions};
use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, FontWeight, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, Pixels, Point, SharedString, Stateful, Subscription, Window,
    deferred, div, prelude::*, px,
};
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant};

pub(super) const AUTOMOD_PREFIX: &str = "guild-automod:";
const LIST_PREFIX: &str = "guild-automod:list:";
const ROLES_PREFIX: &str = "guild-automod:roles:";
const SAVED_PREFIX: &str = "guild-automod:saved:";
const DELETED_PREFIX: &str = "guild-automod:deleted:";
const VALIDATE_PREFIX: &str = "guild-automod:validate:";
/// `syncRules`: how seldom the list is read again.
const SYNC_EVERY: Duration = Duration::from_millis(20_000);
/// `nU`: how long a typed pattern waits before it is checked.
const VALUE_CHECK_AFTER: Duration = Duration::from_millis(1000);
/// Module 652215's `GUILD_AUTOMOD_BLOCKED_MESSAGE` and `GUILD_AUTOMOD_REGEX` articles.
const LEARN_MORE: &str = "https://support.discord.com/hc/en-us/articles/4421269296535";
const REGEX_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/10069840290711";
/// Module 904813: the rule Discord itself keeps in every server.
const DEFAULT_RULE: &str = "1030554520465440818";
/// Module 890634 `xv`, as one line in the input.
const REGEX_PLACEHOLDER: &str = "^b(a|@)d$w(o|0)rd(s|$)";
/// Module 181658's `aTVNes`, for a failure with no message of its own.
const GENERIC_ERROR: &str = "Oops! You've caught an ultra rare error. This is probably our fault, so please try again or check our status page.";
const REGEX_ERRORS: &str = "Please change any regexes that have errors and try again.";
const KEYWORD_ERRORS: &str = "Please change any keywords that have errors and try again.";
const ALLOW_HINT: &str = "Add words or phrases members are allowed to use, separated by a comma (cat, happy puppy, *cookie*)";
const KEYWORD_HINT: &str = "Separate words or phrases with a comma (dog, cat, tiger) or new line. For each word, use * at the beginning, end, or both for partial matching.";
const EXEMPT_HINT: &str = "Pssst \u{2014} members with Admin and Manage Server permissions are always excluded from filter rules.";
const REGEX_LIMIT: usize = 10;
const REGEX_MAX_LENGTH: usize = 260;
const KEYWORD_LIMIT: usize = 1000;
const KEYWORD_MAX_LENGTH: usize = 60;
const NAME_LIMIT: usize = 100;
const MESSAGE_LIMIT: usize = 300;
const MENTION_MIN: i64 = 1;
const MENTION_MAX: i64 = 50;

/// Module 890634 `uh`.
const KEYWORD: u64 = 1;
const ML_SPAM: u64 = 3;
const DEFAULT_KEYWORD_LIST: u64 = 4;
const MENTION_SPAM: u64 = 5;
const USER_PROFILE: u64 = 6;
/// Module 890634 `AH`.
const BLOCK_MESSAGE: u64 = 1;
const FLAG_TO_CHANNEL: u64 = 2;
const TIMEOUT: u64 = 3;
const QUARANTINE_USER: u64 = 4;
/// Module 890634 `Mc`.
const MESSAGE_SEND: u64 = 1;
const MEMBER_JOIN_OR_UPDATE: u64 = 2;
/// Module 200700's durations, in its order.
const DURATIONS: [(u64, &str); 6] = [
    (60, "60 secs"),
    (300, "5 mins"),
    (600, "10 mins"),
    (3600, "1 hour"),
    (86400, "1 day"),
    (604800, "1 week"),
];

/// The line under an action's description: its words, the bold part, the link, and whether the link is drawn.
type ActionLine = (String, Option<(usize, usize)>, &'static str, bool);

/// A word list's field (`t5`) as its two callers set it up.
struct WordsField {
    field: Field,
    editor: Entity<Editor>,
    count: usize,
    limit: usize,
    lowest: bool,
    hint: &'static str,
    error: Option<String>,
}

/// One line of module 850216's `i$`.
struct Trigger {
    name: &'static str,
    event: u64,
    per_guild: usize,
    actions: &'static [u64],
    defaults: &'static [u64],
}

fn trigger(kind: u64) -> Trigger {
    match kind {
        KEYWORD => Trigger {
            name: "Block Custom Words",
            event: MESSAGE_SEND,
            per_guild: 6,
            actions: &[BLOCK_MESSAGE, FLAG_TO_CHANNEL, TIMEOUT],
            defaults: &[BLOCK_MESSAGE],
        },
        ML_SPAM => Trigger {
            name: "Block Suspected Spam Content",
            event: MESSAGE_SEND,
            per_guild: 1,
            actions: &[BLOCK_MESSAGE, FLAG_TO_CHANNEL],
            defaults: &[BLOCK_MESSAGE],
        },
        DEFAULT_KEYWORD_LIST => Trigger {
            name: "Block Commonly Flagged Words",
            event: MESSAGE_SEND,
            per_guild: 1,
            actions: &[BLOCK_MESSAGE, FLAG_TO_CHANNEL],
            defaults: &[BLOCK_MESSAGE],
        },
        MENTION_SPAM => Trigger {
            name: "Block Mention Spam",
            event: MESSAGE_SEND,
            per_guild: 1,
            actions: &[BLOCK_MESSAGE, FLAG_TO_CHANNEL, TIMEOUT],
            defaults: &[BLOCK_MESSAGE],
        },
        USER_PROFILE => Trigger {
            name: "Block Words in Member Profile Names",
            event: MEMBER_JOIN_OR_UPDATE,
            per_guild: 1,
            actions: &[QUARANTINE_USER, FLAG_TO_CHANNEL],
            defaults: &[QUARANTINE_USER],
        },
        _ => Trigger {
            name: "Unknown",
            event: 0,
            per_guild: 0,
            actions: &[],
            defaults: &[],
        },
    }
}

/// `ws`: the sections, in order, with the triggers each shows.
fn sections(community: bool) -> Vec<(&'static str, Vec<u64>)> {
    let mut list = Vec::new();
    if community {
        list.push(("Members", vec![USER_PROFILE]));
    }
    list.push((
        "Content",
        vec![MENTION_SPAM, ML_SPAM, DEFAULT_KEYWORD_LIST, KEYWORD],
    ));
    list
}

/// `u9`'s count over the sections' triggers.
fn rule_counts(rules: &[AutoRule], community: bool) -> (usize, usize) {
    let (mut enabled, mut total) = (0, 0);
    for trigger in sections(community)
        .into_iter()
        .flat_map(|(_, triggers)| triggers)
    {
        let mut any = false;
        for rule in rules.iter().filter(|rule| rule.trigger_type == trigger) {
            any = true;
            total += 1;
            enabled += usize::from(rule.enabled);
        }
        total += usize::from(!any);
    }
    (enabled, total)
}

/// `XO`: an action's starting value. The metadata's `undefined`s are left out, as `JSON.stringify` leaves them.
fn default_action(kind: u64) -> Value {
    match kind {
        TIMEOUT => json!({"type": TIMEOUT, "metadata": {"duration_seconds": 60}}),
        _ => json!({"type": kind, "metadata": {}}),
    }
}

fn action_kind(action: &Value) -> u64 {
    action["type"].as_u64().unwrap_or(0)
}

/// `kT`.
fn default_metadata(kind: u64, community: bool) -> Value {
    match kind {
        DEFAULT_KEYWORD_LIST => json!({"allow_list": [], "presets": []}),
        USER_PROFILE | KEYWORD => {
            json!({"keyword_filter": [], "regex_patterns": [], "allow_list": []})
        }
        MENTION_SPAM => {
            json!({"mention_total_limit": 20, "mention_raid_protection_enabled": community})
        }
        _ => Value::Null,
    }
}

/// A rule as the store holds it; its metadata and actions stay in their wire shape, so unknown fields survive a save.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AutoRule {
    id: String,
    name: String,
    guild_id: String,
    event_type: u64,
    trigger_type: u64,
    trigger_metadata: Value,
    actions: Vec<Value>,
    enabled: bool,
    creator_id: String,
    position: i64,
    exempt_channels: Vec<String>,
    exempt_roles: Vec<String>,
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.clone()),
                    Value::Number(number) => Some(number.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

impl AutoRule {
    /// `N`.
    fn from_wire(value: &Value) -> Self {
        let mut metadata = value["trigger_metadata"].clone();
        if let Some(fields) = metadata.as_object_mut() {
            fields.remove("keyword_lists");
        }
        Self {
            id: value["id"].as_str().unwrap_or_default().to_owned(),
            name: value["name"].as_str().unwrap_or_default().to_owned(),
            guild_id: value["guild_id"].as_str().unwrap_or_default().to_owned(),
            event_type: value["event_type"].as_u64().unwrap_or(0),
            trigger_type: value["trigger_type"].as_u64().unwrap_or(0),
            trigger_metadata: metadata,
            actions: value["actions"].as_array().cloned().unwrap_or_default(),
            enabled: value["enabled"].as_bool().unwrap_or(false),
            creator_id: value["creator_id"].as_str().unwrap_or_default().to_owned(),
            position: value["position"].as_i64().unwrap_or(0),
            exempt_channels: strings(&value["exempt_channels"]),
            exempt_roles: strings(&value["exempt_roles"]),
        }
    }

    /// `_I`, with the number the rule gets when the server has some of its kind.
    fn new(guild: &str, kind: u64, creator: &str, existing: usize, community: bool) -> Self {
        let spec = trigger(kind);
        let name = if existing > 0 {
            format!("{} {}", spec.name, existing + 1)
        } else {
            spec.name.to_owned()
        };
        Self {
            id: format!("{guild}-{kind}-new-rule"),
            name,
            guild_id: guild.to_owned(),
            event_type: spec.event,
            trigger_type: kind,
            trigger_metadata: default_metadata(kind, community),
            actions: spec
                .defaults
                .iter()
                .map(|kind| default_action(*kind))
                .collect(),
            enabled: true,
            creator_id: creator.to_owned(),
            position: 0,
            exempt_channels: Vec::new(),
            exempt_roles: Vec::new(),
        }
    }

    /// `wC`: whether the rule has a server id (`Ut1`).
    fn is_saved(&self) -> bool {
        let id = self.id.trim();
        (17..=19).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_digit())
    }

    /// `R`.
    fn is_default(&self) -> bool {
        self.id == DEFAULT_RULE
    }

    fn list(&self, key: &str) -> Vec<String> {
        strings(&self.trigger_metadata[key])
    }

    fn set_list(&mut self, key: &str, list: Vec<String>) {
        self.set_meta(key, json!(list));
    }

    fn set_meta(&mut self, key: &str, value: Value) {
        if !self.trigger_metadata.is_object() {
            self.trigger_metadata = Value::Object(Map::new());
        }
        self.trigger_metadata[key] = value;
    }

    fn action(&self, kind: u64) -> Option<&Value> {
        self.actions
            .iter()
            .find(|action| action_kind(action) == kind)
    }

    /// `b_`: without that action, then with the new one last.
    fn with_action(&mut self, kind: u64, action: Option<Value>) {
        self.actions.retain(|known| action_kind(known) != kind);
        self.actions.extend(action);
    }

    /// `zR`: the actions it has, in the table's order.
    fn shown_actions(&self) -> Vec<&Value> {
        trigger(self.trigger_type)
            .actions
            .iter()
            .filter_map(|kind| self.action(*kind))
            .collect()
    }

    /// `M`: what goes on the wire. Roles that no longer exist are dropped.
    fn body(&self, role_exists: &dyn Fn(&str) -> bool) -> Value {
        let mut body = json!({
            "id": self.id,
            "name": self.name,
            "guild_id": self.guild_id,
            "event_type": self.event_type,
            "trigger_type": self.trigger_type,
            "actions": self.actions,
            "enabled": self.enabled,
            "creator_id": self.creator_id,
            "position": self.position,
            "exempt_channels": self.exempt_channels,
            "exempt_roles": self.exempt_roles
                .iter()
                .filter(|role| role_exists(role))
                .collect::<Vec<_>>(),
        });
        if !self.trigger_metadata.is_null() {
            body["trigger_metadata"] = self.trigger_metadata.clone();
        }
        body
    }

    /// `S`: a new rule's body has no id.
    fn create_body(&self, role_exists: &dyn Fn(&str) -> bool) -> Value {
        let mut body = self.body(role_exists);
        if let Some(fields) = body.as_object_mut() {
            fields.remove("id");
        }
        body
    }

    /// `saveRule`'s first lines: the word lists without repeats, in order.
    fn tidy(&mut self) {
        if self.trigger_type == KEYWORD {
            let keywords = sorted_unique(self.list("keyword_filter"));
            self.set_list("keyword_filter", keywords);
            let allowed = sorted_unique(self.list("allow_list"));
            self.set_list("allow_list", allowed);
        }
        if self.trigger_type == DEFAULT_KEYWORD_LIST {
            let allowed = sorted_unique(self.list("allow_list"));
            self.set_list("allow_list", allowed);
        }
    }
}

/// `HR(Uq(list))`. `localeCompare` is approximated by comparing case-folded text, then lower case before upper.
fn sorted_unique(list: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<String> = list
        .into_iter()
        .filter(|item| seen.insert(item.clone()))
        .collect();
    let bare = |text: &str| -> String { text.replace(['*', '"', '\''], "") };
    unique.sort_by(|left, right| {
        let (left_bare, right_bare) = (bare(left), bare(right));
        left_bare
            .to_lowercase()
            .cmp(&right_bare.to_lowercase())
            .then_with(|| right_bare.cmp(&left_bare))
    });
    unique
}

/// `c_`: the words of a text area.
fn parse_words(text: &str) -> Vec<String> {
    text.split(['\t', '\n', ','])
        .map(|word| {
            let mut collapsed = String::new();
            let mut gap = String::new();
            for character in word.chars() {
                if character.is_whitespace() {
                    gap.push(character);
                } else {
                    // `\s{2,}` becomes one space; a single whitespace character stays as it is.
                    if gap.chars().count() >= 2 {
                        collapsed.push(' ');
                    } else {
                        collapsed.push_str(&gap);
                    }
                    gap.clear();
                    collapsed.push(character);
                }
            }
            collapsed.trim().to_owned()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// `Mv`.
fn join_words(words: &[String]) -> String {
    words.join(", ")
}

/// `ir` then `Mv(c_(...))`: what a paste with separators turns the field into.
fn normalized_paste(text: &str) -> Option<String> {
    (!text.is_empty() && (text.contains('\n') || text.contains(',')))
        .then(|| join_words(&parse_words(text)))
}

fn utf16_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `h` (`wk`): what is wrong with a word list.
fn keywords_problem(words: &[String], limit: usize) -> Option<String> {
    if words.len() > limit {
        return Some(format!("Please enter no more than {limit} keywords."));
    }
    words
        .iter()
        .find(|word| {
            let length = utf16_length(word);
            !(1..=KEYWORD_MAX_LENGTH).contains(&length)
        })
        .map(|word| {
            format!(
                "{word} has an invalid length. Keywords must be between 1 and {KEYWORD_MAX_LENGTH} characters."
            )
        })
}

/// `xG` then `uV`, with `saveRule`'s words for its two kinds of error. `others` are the server's saved rules.
fn check_rule(rule: &AutoRule, others: &[AutoRule]) -> Result<(), String> {
    let spec = trigger(rule.trigger_type);
    let same_kind = others
        .iter()
        .filter(|other| other.id != rule.id && other.trigger_type == rule.trigger_type)
        .count();
    if same_kind > spec.per_guild {
        return Err(format!(
            "You have exceeded the maximum number of rules of type {}",
            rule.trigger_type
        ));
    }
    if rule
        .actions
        .iter()
        .any(|action| !spec.actions.contains(&action_kind(action)))
    {
        return Err(
            "You have provided an action that is not available for this trigger type".into(),
        );
    }
    if rule.event_type != spec.event {
        return Err(
            "You have provided an event type that is not available for this trigger type".into(),
        );
    }
    if rule.trigger_type == MENTION_SPAM {
        let limit = rule.trigger_metadata["mention_total_limit"].as_i64();
        if !limit.is_some_and(|limit| (MENTION_MIN..=MENTION_MAX).contains(&limit)) {
            return Err(format!(
                "Enter a number between {MENTION_MIN} and {MENTION_MAX}."
            ));
        }
    }
    if rule.trigger_type == KEYWORD {
        let keywords = rule.list("keyword_filter");
        let patterns = rule.list("regex_patterns");
        if keywords.is_empty() && patterns.is_empty() {
            return Err("Please enter at least one keyword or one regex pattern.".into());
        }
        if keywords.len() > KEYWORD_LIMIT {
            return Err(format!(
                "Please enter no more than {KEYWORD_LIMIT} keywords."
            ));
        }
        if keywords_problem(&keywords, KEYWORD_LIMIT).is_some() {
            return Err(KEYWORD_ERRORS.into());
        }
        if patterns.len() > REGEX_LIMIT {
            return Err(format!(
                "Please enter no more than {REGEX_LIMIT} regex patterns."
            ));
        }
        if patterns.iter().any(|pattern| {
            let length = utf16_length(pattern);
            !(1..=REGEX_MAX_LENGTH).contains(&length)
        }) {
            return Err(REGEX_ERRORS.into());
        }
    }
    if rule.actions.is_empty() {
        return Err("Please add at least one action to this rule.".into());
    }
    Ok(())
}

/// `LG.getAnyErrorMessage`: the first field error down the tree, or the response's message.
fn any_error_message(body: &Value) -> String {
    let message = body["message"].as_str();
    if message.is_none() && body.is_object() {
        // A body of `{field: ["words"]}` (the constructor's first branch).
        let first = body
            .as_object()
            .and_then(|fields| fields.values().next())
            .and_then(|value| value.get(0))
            .and_then(Value::as_str);
        if let Some(first) = first {
            return first.to_owned();
        }
    }
    let mut errors = &body["errors"];
    while errors.is_object() {
        if let Some(message) = errors["_errors"][0]["message"].as_str() {
            return message.to_owned();
        }
        let Some(next) = errors.as_object().and_then(|fields| fields.values().next()) else {
            break;
        };
        errors = next;
    }
    message.unwrap_or(GENERIC_ERROR).to_owned()
}

/// `saveRule`'s catch.
fn save_error(body: &Value) -> String {
    if body["code"] == 50035 && !body["errors"]["trigger_metadata"]["regex_patterns"].is_null() {
        REGEX_ERRORS.to_owned()
    } else {
        any_error_message(body)
    }
}

/// One entry of `nP`'s list.
#[derive(Clone, Debug, PartialEq)]
struct RegexError {
    /// The pattern, or `_errors` for one about the whole list.
    pattern: String,
    message: String,
    /// The part of the pattern to mark, in characters.
    offset: Option<usize>,
    length: Option<usize>,
}

/// `nP`: what the validate route said about the patterns; `None` when its error is about something else.
fn regex_errors(body: &Value) -> Option<Vec<RegexError>> {
    let patterns = body["errors"]["trigger_metadata"]["regex_patterns"].as_object()?;
    let mut list: Vec<RegexError> = patterns
        .iter()
        .filter(|(pattern, _)| *pattern != "_errors")
        .map(|(pattern, entry)| RegexError {
            pattern: pattern.clone(),
            message: entry["description"]
                .as_str()
                .or_else(|| entry["_errors"][0]["message"].as_str())
                .unwrap_or(REGEX_ERRORS)
                .to_owned(),
            offset: entry["erroring_character_offset"]
                .as_u64()
                .map(|value| value as usize),
            length: entry["erroring_character_length"]
                .as_u64()
                .map(|value| value as usize),
        })
        .collect();
    if let Some(whole) = patterns.get("_errors").and_then(Value::as_array)
        && !whole.is_empty()
    {
        list.push(RegexError {
            pattern: "_errors".to_owned(),
            message: whole[0]["message"]
                .as_str()
                .unwrap_or(REGEX_ERRORS)
                .to_owned(),
            offset: None,
            length: None,
        });
    }
    Some(list)
}

/// `nI`: a paste becomes several patterns when it has several lines.
fn split_patterns(text: &str) -> Vec<String> {
    text.split(['\n', '\t', '\r', '\u{c}', '\u{b}', '\u{10}'])
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect()
}

/// The stepper's `parseInt` (`nX.l`): the leading number, or nothing.
fn parse_int(text: &str) -> Option<i64> {
    let text = text.trim_start();
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<i64>().ok().map(|number| number * sign)
}

/// What the stepper's input does with text: what it shows and what the rule gets; `None` leaves both alone.
fn stepper_input(text: &str) -> Option<(String, i64)> {
    if text.is_empty() || text == "-" {
        return Some((text.to_owned(), MENTION_MIN));
    }
    let number = parse_int(text)?.clamp(MENTION_MIN, MENTION_MAX);
    Some((number.to_string(), number))
}

/// `AH`'s name in a pill (`nl.x`), with its helper in brackets.
fn action_title(kind: u64) -> &'static str {
    match kind {
        BLOCK_MESSAGE => "Block message",
        FLAG_TO_CHANNEL => "Send alert",
        TIMEOUT => "Timeout member",
        QUARANTINE_USER => "Block member interactions",
        _ => "",
    }
}

fn action_description(kind: u64, trigger_type: u64) -> &'static str {
    match (kind, trigger_type) {
        (BLOCK_MESSAGE, MENTION_SPAM) => "Block messages with an excessive # of unique mentions.",
        (BLOCK_MESSAGE, ML_SPAM) => "Block messages with spam-like content from being posted.",
        (BLOCK_MESSAGE, _) => "Block messages containing these words from being posted.",
        (FLAG_TO_CHANNEL, _) => {
            "Sends an alert containing the flagged message to a chosen channel."
        }
        (TIMEOUT, _) => {
            "Temporarily disables a member's ability to send messages or join voice channels."
        }
        (QUARANTINE_USER, _) => "Prevents a member from using text, voice, or other interactions.",
        _ => "",
    }
}

fn action_glyph(kind: u64) -> &'static str {
    match kind {
        FLAG_TO_CHANNEL => "automod_alert",
        TIMEOUT => "automod_timeout",
        QUARANTINE_USER => "automod_block",
        _ => "automod_block",
    }
}

fn duration_label(seconds: u64) -> Option<&'static str> {
    DURATIONS
        .iter()
        .find(|(value, _)| *value == seconds)
        .map(|(_, label)| *label)
}

/// `it`'s words and glyph for a rule card.
fn trigger_description(kind: u64) -> &'static str {
    match kind {
        KEYWORD => "Create your own filter to block specific language from your server.",
        ML_SPAM => {
            "Monitor messages, Forum posts, and threads for potentially spammy content or activity. (Support for English only)"
        }
        DEFAULT_KEYWORD_LIST => {
            "Flag messages that contain profanity and more. (Support for English only)"
        }
        MENTION_SPAM => "Block messages with an excessive # of role and user mentions",
        USER_PROFILE => "Block specific language in your member's usernames.",
        _ => "",
    }
}

fn trigger_glyph(kind: u64) -> &'static str {
    match kind {
        MENTION_SPAM => "automod_at",
        KEYWORD => "automod_custom",
        ML_SPAM | USER_PROFILE => "automod_spam",
        _ => "automod_flagged",
    }
}

/// `xZUvxR` and `dJN7Lk`: how many words a saved custom rule has.
fn rule_subtext(rule: &AutoRule) -> String {
    if !rule.is_saved() || rule.trigger_type != KEYWORD {
        return String::new();
    }
    let plural = |count: usize, one: &str| {
        if count == 1 {
            format!("{count} {one}")
        } else if one == "regex" {
            format!("{count} regexes")
        } else {
            format!("{count} {one}s")
        }
    };
    let words = rule.list("keyword_filter").len();
    let patterns = rule.list("regex_patterns").len();
    if patterns > 0 {
        format!("({}, {})", plural(words, "word"), plural(patterns, "regex"))
    } else {
        format!("({})", plural(words, "word"))
    }
}

/// `fuzzysearch`: the query's letters in order in the text.
fn fuzzy_match(query: &str, text: &str) -> bool {
    let mut letters = text.chars();
    query
        .chars()
        .all(|wanted| letters.by_ref().any(|letter| letter == wanted))
}

/// `P`: how a role is written.
fn role_display(name: &str) -> String {
    if name.contains('@') {
        name.to_owned()
    } else {
        format!("@{name}")
    }
}

/// A row of the exemption list (`d` of module 508595).
#[derive(Clone, Debug, PartialEq)]
struct ChannelRow {
    id: Id,
    name: String,
    kind: ChannelKind,
    nested: bool,
}

/// `d` of module 508595: what is left to pick, loose channels first, each category with its channels under it.
fn channel_rows(channels: &[Channel], selected: &[String]) -> Vec<ChannelRow> {
    let taken = |channel: &Channel| selected.contains(&channel.id.to_string());
    let usable = |channel: &&Channel| {
        matches!(
            channel.kind,
            ChannelKind::Text
                | ChannelKind::Voice
                | ChannelKind::Other(5)
                | ChannelKind::Other(15)
                | ChannelKind::Other(16)
        )
    };
    let mut ordered: Vec<&Channel> = channels.iter().collect();
    ordered.sort_by_key(|channel| (channel.position, channel.id));
    let vocal = |channel: &Channel| channel.kind == ChannelKind::Voice;
    // `r.I` and `VOCAL`: the store's two lists, one after the other.
    let members: Vec<&Channel> = ordered
        .iter()
        .copied()
        .filter(usable)
        .filter(|channel| !vocal(channel))
        .chain(
            ordered
                .iter()
                .copied()
                .filter(usable)
                .filter(|channel| vocal(channel)),
        )
        .collect();
    let mut top: Vec<&Channel> = ordered
        .iter()
        .copied()
        .filter(|channel| channel.kind == ChannelKind::Category)
        .collect();
    top.extend(
        members
            .iter()
            .copied()
            .filter(|channel| channel.parent_id.is_none() && !taken(channel)),
    );
    // Categories last; otherwise the higher position first (`l - n`), stable for equal ones.
    top.sort_by(|left, right| {
        let category = |channel: &Channel| channel.kind == ChannelKind::Category;
        category(left)
            .cmp(&category(right))
            .then_with(|| right.position.cmp(&left.position))
    });
    let mut rows = Vec::new();
    for channel in top {
        if taken(channel) {
            continue;
        }
        rows.push(ChannelRow {
            id: channel.id,
            name: channel.name.clone(),
            kind: channel.kind,
            nested: channel.parent_id.is_some(),
        });
        if channel.kind == ChannelKind::Category {
            for member in members
                .iter()
                .filter(|member| member.parent_id == Some(channel.id) && !taken(member))
            {
                rows.push(ChannelRow {
                    id: member.id,
                    name: member.name.clone(),
                    kind: member.kind,
                    nested: true,
                });
            }
        }
    }
    rows
}

fn channel_glyph(kind: ChannelKind) -> &'static str {
    match kind {
        ChannelKind::Category => "automod_category",
        ChannelKind::Voice => "automod_channel_voice",
        ChannelKind::Other(15 | 16) => "channel_type_forum",
        _ => "automod_channel_text",
    }
}

/// A role as the exemption list shows it.
#[derive(Clone, Debug, PartialEq)]
struct PickRole {
    id: String,
    name: String,
    color: u32,
}

fn field_style(size: f32, line_height: f32) -> EditorStyle {
    let mut style = super::editor_style();
    style.size = size;
    style.line_height = line_height;
    style.placeholder = super::hsla("--input-placeholder-text-default");
    style
}

fn code_style() -> EditorStyle {
    let mut style = field_style(12., 16.);
    style.font = gpui::font(super::MONO);
    style
}

/// A text area's corner or the regex box's edge in the hand.
struct ResizeDrag(Resized);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Resized {
    Keywords,
    Allow,
    Regex,
}

/// What follows the cursor while resizing: nothing, the box itself moves.
struct ResizeGhost;

impl gpui::Render for ResizeGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The height a drag leaves a box at: `textarea`'s `min-height: 40` or `nf.A`'s `minDimension: 100`.
fn resized_height(which: Resized, from_height: f32, moved: f32) -> f32 {
    let lowest = if which == Resized::Regex { 100. } else { 40. };
    (from_height + moved).max(lowest)
}

/// Which field an editor belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Field {
    Name,
    Keywords,
    Allow,
    Mention,
    RegexInput,
    TagEdit,
    Query,
    Message,
}

/// What the open card's fields hold besides the rule (the components' local state).
pub(crate) struct Fields {
    name: Entity<Editor>,
    keywords: Option<Entity<Editor>>,
    allow: Option<Entity<Editor>>,
    mention: Option<Entity<Editor>>,
    regex_input: Option<Entity<Editor>>,
    query: Entity<Editor>,
    /// The regex tag being edited in place.
    tag_edit: Option<(usize, Entity<Editor>)>,
    allow_open: bool,
    regex_open: bool,
    keywords_height: f32,
    allow_height: f32,
    regex_height: f32,
    /// Where a resize started: the pointer's height and the box's.
    resize_from: (f32, f32),
    keywords_error: Option<String>,
    allow_error: Option<String>,
    /// The next edit of the word lists comes from Ctrl+V.
    pasting: bool,
    /// Selected regex tags, by position.
    selected: Vec<usize>,
    regex_errors: Vec<RegexError>,
    value_error: Option<RegexError>,
    /// `nL`'s undo list: the tags and the typed text at each step.
    history: Vec<(Vec<String>, String)>,
    history_at: usize,
    /// A typed pattern is waiting for its check.
    value_check_waiting: bool,
    _events: Vec<Subscription>,
}

enum Dialog {
    /// Module 148210.
    Message {
        editor: Entity<Editor>,
        _events: Subscription,
    },
    /// Module 158956.
    Alert {
        is_edit: bool,
        channel: Option<String>,
        error: bool,
        open: bool,
    },
    /// Module 16039.
    Timeout {
        is_edit: bool,
        seconds: u64,
        open: bool,
    },
}

enum Confirm {
    /// Module 146502, then editing `next`.
    Unsaved { name: String, next: AutoRule },
    /// Module 49065.
    Remove { rule: AutoRule },
    /// The alert `c` opens for a Community server's mention rule.
    CannotDelete,
}

struct Menu {
    at: Point<Pixels>,
    rule: AutoRule,
}

/// What a request in flight puts back if it fails.
struct Pending {
    key: String,
    restore: Option<AutoRule>,
}

/// `GuildSettings`' AutoMod: the rule store (module 928348) and the editing store (module 364806).
pub(crate) struct AutoModState {
    guild: Id,
    rules: Vec<AutoRule>,
    loaded: bool,
    synced: Option<Instant>,
    roles: Vec<PickRole>,
    editing: Option<AutoRule>,
    has_changes: bool,
    saving: bool,
    error: Option<String>,
    fields: Option<Fields>,
    dialog: Option<Dialog>,
    confirm: Option<Confirm>,
    menu: Option<Menu>,
    picker_open: bool,
    pending: Option<Pending>,
    deleting: bool,
    sequence: u64,
    validate_key: Option<String>,
    value_key: Option<String>,
}

impl AutoModState {
    fn new(guild: Id) -> Self {
        Self {
            guild,
            rules: Vec::new(),
            loaded: false,
            synced: None,
            roles: Vec::new(),
            editing: None,
            has_changes: false,
            saving: false,
            error: None,
            fields: None,
            dialog: None,
            confirm: None,
            menu: None,
            picker_open: false,
            pending: None,
            deleting: false,
            sequence: 0,
            validate_key: None,
            value_key: None,
        }
    }

    fn rules_of(&self, kind: u64) -> Vec<&AutoRule> {
        self.rules
            .iter()
            .filter(|rule| rule.trigger_type == kind)
            .collect()
    }

    /// `updateRule`: a rule put in its place, or added.
    fn update_rule(&mut self, rule: AutoRule) {
        if let Some(known) = self.rules.iter_mut().find(|known| known.id == rule.id) {
            *known = rule;
        } else {
            self.rules.push(rule);
        }
    }

    fn next_key(&mut self, prefix: &str) -> String {
        self.sequence += 1;
        format!("{prefix}{}:{}", self.guild, self.sequence)
    }

    /// The fields open now, for typing in tests.
    #[cfg(feature = "capture")]
    pub(super) fn editors(&self) -> Vec<Entity<Editor>> {
        let mut list = Vec::new();
        if let Some(fields) = self.fields.as_ref() {
            list.push(fields.name.clone());
            list.extend(fields.keywords.clone());
            list.extend(fields.allow.clone());
            list.extend(fields.mention.clone());
            list.extend(fields.regex_input.clone());
            list.push(fields.query.clone());
            list.extend(fields.tag_edit.as_ref().map(|(_, editor)| editor.clone()));
        }
        if let Some(Dialog::Message { editor, .. }) = self.dialog.as_ref() {
            list.push(editor.clone());
        }
        list
    }
}

impl Root {
    fn automod(&self) -> Option<&AutoModState> {
        self.server_settings.as_ref()?.automod.as_ref()
    }

    fn automod_mut(&mut self) -> Option<&mut AutoModState> {
        self.server_settings.as_mut()?.automod.as_mut()
    }

    fn automod_community(&self, guild: Id) -> bool {
        self.stores
            .guild(guild)
            .is_some_and(|known| known.features.iter().any(|feature| feature == "COMMUNITY"))
    }

    /// Opening the page: the rules (at most every 20 seconds, `syncRules`), the roles, and the filter level `th` shows.
    pub(super) fn request_automod(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        // `useEffect` on the guild: leaving a guild's rules drops the rule being edited.
        if open
            .automod
            .as_ref()
            .is_none_or(|state| state.guild != guild)
        {
            open.automod = Some(AutoModState::new(guild));
        }
        let due = open
            .automod
            .as_ref()
            .and_then(|state| state.synced)
            .is_none_or(|synced| synced.elapsed() > SYNC_EVERY);
        if due {
            if let Some(state) = open.automod.as_mut() {
                state.synced = Some(Instant::now());
            }
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{LIST_PREFIX}{guild}")),
                method: reqwest::Method::GET,
                route: format!("/guilds/{guild}/auto-moderation/rules"),
                body: None,
            });
        }
        let _ = self.requests.send(Request::Fetch {
            key: format!("{ROLES_PREFIX}{guild}"),
            route: format!("/guilds/{guild}/roles"),
        });
        self.request_safety(guild);
        cx.notify();
    }

    pub(super) fn automod_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        if key.starts_with(LIST_PREFIX) {
            // A failed read leaves the empty store (`syncRules`' catch), and every card offers to set up.
            let rules: Vec<AutoRule> = report["body"]
                .as_array()
                .map(|list| list.iter().map(AutoRule::from_wire).collect())
                .unwrap_or_default();
            if let Some(state) = self.automod_mut() {
                state.rules = rules;
                state.loaded = true;
            }
        } else if key.starts_with(ROLES_PREFIX) {
            let mut roles: Vec<Value> = report.as_array().cloned().unwrap_or_default();
            // `getSortedRoles`: highest first.
            roles.sort_by_key(|role| std::cmp::Reverse(role["position"].as_i64().unwrap_or(0)));
            if let Some(state) = self.automod_mut() {
                let guild = state.guild.to_string();
                state.roles = roles
                    .iter()
                    .filter(|role| role["id"].as_str() != Some(guild.as_str()))
                    .map(|role| PickRole {
                        id: role["id"].as_str().unwrap_or_default().to_owned(),
                        name: role["name"].as_str().unwrap_or_default().to_owned(),
                        color: role["color"].as_u64().unwrap_or(0) as u32,
                    })
                    .collect();
            }
        } else if key.starts_with(SAVED_PREFIX) {
            self.automod_saved(key, &report);
        } else if key.starts_with(DELETED_PREFIX) {
            self.automod_deleted(key, &report, cx);
        } else if key.starts_with(VALIDATE_PREFIX) {
            self.automod_validated(key, &report);
        }
        cx.notify();
    }

    /// The end of `saveRule`.
    fn automod_saved(&mut self, key: &str, report: &Value) {
        let Some(state) = self.automod_mut() else {
            return;
        };
        let pending = state.pending.take_if(|pending| pending.key == key);
        let Some(pending) = pending else {
            return;
        };
        state.saving = false;
        if report["ok"] == true {
            // `M`, then `d(e)`.
            state.editing = None;
            state.has_changes = false;
            state.error = None;
            state.fields = None;
            state.update_rule(AutoRule::from_wire(&report["body"]));
        } else {
            state.error = Some(save_error(&report["body"]));
            if let Some(restore) = pending.restore {
                state.update_rule(restore);
            }
        }
    }

    fn automod_deleted(&mut self, key: &str, report: &Value, cx: &mut Context<Self>) {
        let rule = key
            .strip_prefix(DELETED_PREFIX)
            .unwrap_or_default()
            .to_owned();
        let ok = report["ok"] == true;
        if let Some(state) = self.automod_mut() {
            state.deleting = false;
            if ok {
                state.editing = None;
                state.has_changes = false;
                state.error = None;
                state.fields = None;
                state.rules.retain(|known| known.id != rule);
            }
        }
        if !ok {
            let body = &report["body"];
            let message = if body["message"].is_string() || body["errors"].is_object() {
                any_error_message(body)
            } else {
                "An error occurred, please try again.".to_owned()
            };
            self.put_app_toast(message, false, cx);
        }
    }

    /// `nP`'s two callers: the whole list, or the typed pattern alone.
    fn automod_validated(&mut self, key: &str, report: &Value) {
        let Some(state) = self.automod_mut() else {
            return;
        };
        let whole = state.validate_key.as_deref() == Some(key);
        let typed = state.value_key.as_deref() == Some(key);
        if !whole && !typed {
            return;
        }
        let Some(fields) = state.fields.as_mut() else {
            return;
        };
        let errors = if report["ok"] == true {
            Some(Vec::new())
        } else {
            regex_errors(&report["body"])
        };
        // `n?.trigger_metadata?.regex_patterns == null`: an error about something else changes nothing.
        let Some(errors) = errors else {
            return;
        };
        if whole {
            if errors.is_empty() {
                fields.value_error = None;
            }
            fields.regex_errors = errors;
        } else {
            fields.value_error = errors.into_iter().next();
        }
    }

    fn automod_role_exists(&self, guild: Id) -> impl Fn(&str) -> bool + use<> {
        let known: Vec<String> = self
            .stores
            .guild(guild)
            .map(|known| known.roles.keys().map(ToString::to_string).collect())
            .unwrap_or_default();
        move |role: &str| known.iter().any(|id| id == role)
    }

    /// `R`: asks what the server thinks of the patterns.
    fn automod_send_validate(&mut self, guild: Id, patterns: Vec<String>, whole: bool) {
        let exists = self.automod_role_exists(guild);
        let Some(state) = self.automod_mut() else {
            return;
        };
        let Some(mut rule) = state.editing.clone() else {
            return;
        };
        rule.set_list("regex_patterns", patterns);
        let key = state.next_key(VALIDATE_PREFIX);
        if whole {
            state.validate_key = Some(key.clone());
            // `nU.cancel()`: a waiting check of the typed text is moot now.
            state.value_key = None;
        } else {
            state.value_key = Some(key.clone());
        }
        let body = rule.body(&exists);
        let _ = self.requests.send(Request::Rest {
            key: Some(key),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/auto-moderation/rules/validate"),
            body: Some(body),
        });
    }

    /// `setEditingRule` from a field: the change counts unless a save is running (`x`).
    fn automod_change(&mut self, change: impl FnOnce(&mut AutoRule), cx: &mut Context<Self>) {
        if let Some(state) = self.automod_mut()
            && !state.saving
            && let Some(rule) = state.editing.as_mut()
        {
            change(rule);
            state.has_changes = true;
            state.error = None;
        }
        cx.notify();
    }

    fn automod_editor(
        &mut self,
        field: Field,
        placeholder: &str,
        text: &str,
        style: EditorStyle,
        cx: &mut Context<Self>,
    ) -> (Entity<Editor>, Subscription) {
        let placeholder = placeholder.to_owned();
        let multiline = matches!(field, Field::Keywords | Field::Allow | Field::Message);
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder(placeholder);
            editor.set_text(text);
            editor.set_reports_all_edits(true);
            editor.set_enter_inserts_newline(multiline);
            editor
        });
        let events = cx.subscribe(&editor, move |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => this.automod_field_edited(field, &editor, cx),
                // A one-line field keeps its text: Enter does nothing in the official inputs.
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
                EditorEvent::Cancel => {}
            }
            cx.notify();
        });
        (editor, events)
    }

    fn automod_field_edited(
        &mut self,
        field: Field,
        editor: &Entity<Editor>,
        cx: &mut Context<Self>,
    ) {
        let text = editor.read(cx).text().to_owned();
        match field {
            Field::Name => {
                let cut: String = text.chars().take(NAME_LIMIT).collect();
                if cut != text {
                    editor.update(cx, |editor, _| editor.set_text(&cut));
                }
                self.automod_change(|rule| rule.name = cut, cx);
            }
            Field::Message => {
                let cut: String = text.chars().take(MESSAGE_LIMIT).collect();
                if cut != text {
                    editor.update(cx, |editor, _| editor.set_text(&cut));
                }
            }
            Field::Keywords | Field::Allow => {
                let pasted = self
                    .automod_mut()
                    .and_then(|state| state.fields.as_mut())
                    .is_some_and(|fields| std::mem::take(&mut fields.pasting));
                let shown = if pasted {
                    normalized_paste(&text)
                } else {
                    None
                };
                if let Some(shown) = shown.as_ref() {
                    editor.update(cx, |editor, _| editor.set_text(shown));
                }
                let words = parse_words(shown.as_deref().unwrap_or(&text));
                let keywords = field == Field::Keywords;
                let limit = self.automod_allow_limit();
                let problem =
                    keywords_problem(&words, if keywords { KEYWORD_LIMIT } else { limit });
                if let Some(fields) = self.automod_mut().and_then(|state| state.fields.as_mut()) {
                    if keywords {
                        fields.keywords_error = problem;
                    } else {
                        fields.allow_error = problem;
                    }
                }
                self.automod_change(
                    |rule| {
                        rule.set_list(
                            if keywords {
                                "keyword_filter"
                            } else {
                                "allow_list"
                            },
                            words,
                        );
                    },
                    cx,
                );
            }
            Field::Mention => match stepper_input(&text) {
                Some((shown, value)) => {
                    if shown != text {
                        editor.update(cx, |editor, _| editor.set_text(&shown));
                    }
                    self.automod_change(
                        |rule| rule.set_meta("mention_total_limit", json!(value)),
                        cx,
                    );
                }
                None => {
                    let current = self
                        .automod()
                        .and_then(|state| state.editing.as_ref())
                        .and_then(|rule| rule.trigger_metadata["mention_total_limit"].as_i64())
                        .unwrap_or(MENTION_MIN);
                    editor.update(cx, |editor, _| editor.set_text(&current.to_string()));
                }
            },
            Field::RegexInput => self.automod_regex_typed(text, cx),
            Field::TagEdit => {
                let index = self
                    .automod()
                    .and_then(|state| state.fields.as_ref())
                    .and_then(|fields| fields.tag_edit.as_ref())
                    .map(|(index, _)| *index);
                if let Some(index) = index {
                    let mut tags = self.automod_tags();
                    if index < tags.len() {
                        tags[index] = text;
                        self.automod_set_tags(tags, cx);
                    }
                }
            }
            Field::Query => {
                if let Some(state) = self.automod_mut() {
                    state.picker_open = true;
                }
                cx.notify();
            }
        }
    }

    /// `nx` allows 1000 words where `nY` and `nQ` allow 100.
    fn automod_allow_limit(&self) -> usize {
        match self
            .automod()
            .and_then(|state| state.editing.as_ref())
            .map(|rule| rule.trigger_type)
        {
            Some(DEFAULT_KEYWORD_LIST) => 1000,
            _ => 100,
        }
    }

    fn automod_tags(&self) -> Vec<String> {
        self.automod()
            .and_then(|state| state.editing.as_ref())
            .map(|rule| rule.list("regex_patterns"))
            .unwrap_or_default()
    }

    /// `validatePatternsChanged`: new tags go to the rule and straight to the server for checking.
    fn automod_set_tags(&mut self, tags: Vec<String>, cx: &mut Context<Self>) {
        let old = self.automod_tags();
        if old == tags {
            return;
        }
        let value = self.automod_regex_value(cx);
        if let Some(fields) = self.automod_mut().and_then(|state| state.fields.as_mut()) {
            fields.history.truncate(fields.history_at + 1);
            fields.history.push((tags.clone(), value));
            fields.history_at = fields.history.len() - 1;
            fields.selected.clear();
        }
        self.automod_tags_apply(tags, cx);
    }

    /// The tags as they are now, with their check; the undo list has already moved.
    fn automod_tags_apply(&mut self, tags: Vec<String>, cx: &mut Context<Self>) {
        let list = tags.clone();
        self.automod_change(|rule| rule.set_list("regex_patterns", list), cx);
        let Some(state) = self.automod_mut() else {
            return;
        };
        let guild = state.guild;
        if tags.is_empty() {
            // `nP`: nothing to check is no errors.
            state.validate_key = None;
            if let Some(fields) = state.fields.as_mut() {
                fields.regex_errors.clear();
                fields.value_error = None;
            }
        } else {
            self.automod_send_validate(guild, tags, true);
        }
    }

    fn automod_regex_value(&self, cx: &Context<Self>) -> String {
        self.automod()
            .and_then(|state| state.fields.as_ref())
            .and_then(|fields| fields.regex_input.as_ref())
            .map(|editor| editor.read(cx).text().to_owned())
            .unwrap_or_default()
    }

    /// `onChangeNewTagValue` then `validateEditingValueChanged`: three characters or more are checked, a second later.
    fn automod_regex_typed(&mut self, text: String, cx: &mut Context<Self>) {
        let tags = self.automod_tags();
        let Some(fields) = self.automod_mut().and_then(|state| state.fields.as_mut()) else {
            return;
        };
        fields.history.truncate(fields.history_at + 1);
        fields.history.push((tags, text.clone()));
        fields.history_at = fields.history.len() - 1;
        fields.selected.clear();
        if utf16_length(&text) < 3 || fields.value_check_waiting {
            return;
        }
        fields.value_check_waiting = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(VALUE_CHECK_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                let value = this.automod_regex_value(cx);
                let Some(state) = this.automod_mut() else {
                    return;
                };
                let guild = state.guild;
                if let Some(fields) = state.fields.as_mut() {
                    fields.value_check_waiting = false;
                }
                if utf16_length(&value) >= 3 {
                    this.automod_send_validate(guild, vec![value], false);
                }
            });
        })
        .detach();
    }

    /// `f`: the typed text becomes a tag.
    fn automod_commit_value(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(input) = self
            .automod()
            .and_then(|state| state.fields.as_ref())
            .and_then(|fields| fields.regex_input.clone())
        else {
            return false;
        };
        let value = input.read(cx).text().to_owned();
        if value.trim().is_empty() {
            return false;
        }
        input.update(cx, |editor, _| editor.set_text(""));
        let mut tags = self.automod_tags();
        tags.push(value);
        self.automod_set_tags(tags, cx);
        true
    }

    /// `S`: leaving a tag; an empty one goes away.
    fn automod_finish_tag(&mut self, refocus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fields) = self.automod_mut().and_then(|state| state.fields.as_mut()) else {
            return;
        };
        let Some((index, editor)) = fields.tag_edit.take() else {
            return;
        };
        let input = fields.regex_input.clone();
        if editor.read(cx).text().trim().is_empty() {
            let mut tags = self.automod_tags();
            if index < tags.len() {
                tags.remove(index);
                self.automod_set_tags(tags, cx);
            }
        }
        if refocus && let Some(input) = input {
            let handle = input.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        cx.notify();
    }

    fn automod_start_tag_edit(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.automod_finish_tag(false, window, cx);
        let text = self.automod_tags().get(index).cloned().unwrap_or_default();
        let (editor, events) = self.automod_editor(Field::TagEdit, "", &text, code_style(), cx);
        let handle = editor.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        if let Some(fields) = self.automod_mut().and_then(|state| state.fields.as_mut()) {
            fields.selected.clear();
            fields.tag_edit = Some((index, editor));
            fields._events.push(events);
        }
        cx.notify();
    }

    /// `handleKeyDown` of the regex box, ahead of the field.
    fn automod_regex_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stroke = &event.keystroke;
        let control = stroke.modifiers.control || stroke.modifiers.platform;
        let editing_tag = self
            .automod()
            .and_then(|state| state.fields.as_ref())
            .and_then(|fields| fields.tag_edit.as_ref())
            .map(|(_, editor)| editor.read(cx).text().trim().is_empty());
        if let Some(empty) = editing_tag {
            // `_` of the tag's own input.
            match stroke.key.as_str() {
                "enter" | "tab" => {
                    self.automod_finish_tag(true, window, cx);
                    cx.stop_propagation();
                }
                "backspace" if empty => {
                    self.automod_finish_tag(true, window, cx);
                    cx.stop_propagation();
                }
                _ => {}
            }
            return;
        }
        let value = self.automod_regex_value(cx);
        let blank = value.trim().is_empty();
        match stroke.key.as_str() {
            "enter" | "tab" if !blank => {
                self.automod_commit_value(cx);
                cx.stop_propagation();
            }
            "enter" if blank => cx.stop_propagation(),
            "backspace" => {
                let selected = self
                    .automod()
                    .and_then(|state| state.fields.as_ref())
                    .map(|fields| fields.selected.clone())
                    .unwrap_or_default();
                let tags = self.automod_tags();
                if !selected.is_empty() {
                    let kept: Vec<String> = tags
                        .iter()
                        .enumerate()
                        .filter(|(at, _)| !selected.contains(at))
                        .map(|(_, tag)| tag.clone())
                        .collect();
                    self.automod_set_tags(kept, cx);
                    cx.stop_propagation();
                } else if blank {
                    // The last tag comes back to be typed over.
                    let mut tags = tags;
                    let last = tags.pop().unwrap_or_default();
                    if let Some(input) = self
                        .automod()
                        .and_then(|state| state.fields.as_ref())
                        .and_then(|fields| fields.regex_input.clone())
                    {
                        input.update(cx, |editor, _| editor.set_text(&last));
                    }
                    self.automod_set_tags(tags, cx);
                    cx.stop_propagation();
                }
            }
            "a" if control && blank => {
                let count = self.automod_tags().len();
                if count > 0 {
                    if let Some(fields) = self.automod_mut().and_then(|state| state.fields.as_mut())
                    {
                        fields.selected = (0..count).collect();
                    }
                    cx.stop_propagation();
                }
            }
            "z" | "y" if control => {
                self.automod_regex_history(stroke.key == "z", cx);
                cx.stop_propagation();
            }
            "v" if control => {
                let parts = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .map(|text| split_patterns(&text))
                    .unwrap_or_default();
                // One line pastes into the field as usual.
                if parts.len() > 1 {
                    let mut tags = self.automod_tags();
                    tags.extend(parts);
                    self.automod_set_tags(tags, cx);
                    cx.stop_propagation();
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// `goBack` and `goForward`.
    fn automod_regex_history(&mut self, back: bool, cx: &mut Context<Self>) {
        let Some(fields) = self.automod_mut().and_then(|state| state.fields.as_mut()) else {
            return;
        };
        let at = if back {
            fields.history_at.saturating_sub(1)
        } else {
            (fields.history_at + 1).min(fields.history.len().saturating_sub(1))
        };
        fields.history_at = at;
        let Some((tags, value)) = fields.history.get(at).cloned() else {
            return;
        };
        let input = fields.regex_input.clone();
        if let Some(input) = input {
            input.update(cx, |editor, _| editor.set_text(&value));
        }
        self.automod_tags_apply(tags, cx);
    }

    /// The local state of the components a card mounts when it opens.
    fn automod_make_fields(&mut self, rule: &AutoRule, cx: &mut Context<Self>) -> Fields {
        let mut events = Vec::new();
        let (name, subscription) = self.automod_editor(
            Field::Name,
            "Enter a rule name",
            &rule.name,
            field_style(16., 20.),
            cx,
        );
        events.push(subscription);
        let mut words_field =
            |this: &mut Self, field: Field, words: Vec<String>, cx: &mut Context<Self>| {
                let (editor, subscription) = this.automod_editor(
                    field,
                    "Enter 1 or more words or phrases",
                    &join_words(&words),
                    field_style(16., 20.),
                    cx,
                );
                events.push(subscription);
                editor
            };
        let keyword_like = matches!(rule.trigger_type, KEYWORD | USER_PROFILE);
        let keywords = keyword_like
            .then(|| words_field(self, Field::Keywords, rule.list("keyword_filter"), cx));
        let allow = (keyword_like || rule.trigger_type == DEFAULT_KEYWORD_LIST)
            .then(|| words_field(self, Field::Allow, rule.list("allow_list"), cx));
        let mention = (rule.trigger_type == MENTION_SPAM).then(|| {
            let value = rule.trigger_metadata["mention_total_limit"]
                .as_i64()
                .unwrap_or(20);
            let (editor, subscription) = self.automod_editor(
                Field::Mention,
                "",
                &value.to_string(),
                field_style(16., 20.),
                cx,
            );
            events.push(subscription);
            editor
        });
        let regex_input = keyword_like.then(|| {
            let (editor, subscription) =
                self.automod_editor(Field::RegexInput, REGEX_PLACEHOLDER, "", code_style(), cx);
            events.push(subscription);
            editor
        });
        let placeholder = if rule.trigger_type == USER_PROFILE {
            "Enter roles..."
        } else {
            "Search channels or roles"
        };
        let (query, subscription) =
            self.automod_editor(Field::Query, placeholder, "", field_style(16., 20.), cx);
        events.push(subscription);
        Fields {
            name,
            keywords,
            allow,
            mention,
            regex_input,
            query,
            tag_edit: None,
            allow_open: !rule.list("allow_list").is_empty(),
            regex_open: !rule.list("regex_patterns").is_empty(),
            keywords_height: 78.,
            allow_height: 78.,
            regex_height: 100.,
            resize_from: (0., 0.),
            keywords_error: None,
            allow_error: None,
            pasting: false,
            selected: Vec::new(),
            regex_errors: Vec::new(),
            value_error: None,
            history: vec![(rule.list("regex_patterns"), String::new())],
            history_at: 0,
            value_check_waiting: false,
            _events: events,
        }
    }

    /// `setEditingRule(rule)`, or `null` to close.
    fn automod_set_editing(&mut self, rule: Option<AutoRule>, cx: &mut Context<Self>) {
        let fields = rule.as_ref().map(|rule| self.automod_make_fields(rule, cx));
        if let Some(state) = self.automod_mut() {
            state.has_changes = false;
            state.error = None;
            state.fields = fields;
            state.picker_open = false;
            state.editing = rule;
        }
        cx.notify();
    }

    /// `_I` for this guild.
    fn automod_new_rule(&self, guild: Id, kind: u64) -> AutoRule {
        let existing = self.automod().map_or(0, |state| state.rules_of(kind).len());
        let creator = self
            .stores
            .me()
            .map_or(String::new(), |me| me.id.to_string());
        AutoRule::new(
            &guild.to_string(),
            kind,
            &creator,
            existing,
            self.automod_community(guild),
        )
    }

    /// `E`: a click on a card's header.
    fn automod_card_clicked(&mut self, rule: AutoRule, cx: &mut Context<Self>) {
        let Some(state) = self.automod() else {
            return;
        };
        let current = state
            .editing
            .as_ref()
            .is_some_and(|known| known.id == rule.id);
        let changed = state.has_changes;
        if current {
            if !changed {
                self.automod_set_editing(None, cx);
            }
        } else if changed {
            if let Some(state) = self.automod_mut() {
                state.confirm = Some(Confirm::Unsaved {
                    name: rule.name.clone(),
                    next: rule,
                });
            }
            cx.notify();
        } else {
            self.automod_set_editing(Some(rule), cx);
        }
    }

    /// `onSave` of the bar.
    fn automod_save(&mut self, guild: Id, cx: &mut Context<Self>) {
        // The click is the blur `I` waits for: typed text becomes a tag first.
        self.automod_commit_value(cx);
        let Some(state) = self.automod() else {
            return;
        };
        let Some(rule) = state.editing.clone() else {
            return;
        };
        let is_new = !rule.is_saved();
        if !state.has_changes && !is_new {
            self.automod_cancel(cx);
            return;
        }
        let restore = (!is_new)
            .then(|| {
                state
                    .rules
                    .iter()
                    .find(|known| known.id == rule.id)
                    .cloned()
            })
            .flatten();
        let others = state.rules.clone();
        if !is_new && let Some(state) = self.automod_mut() {
            state.update_rule(rule.clone());
        }
        self.automod_save_rule(guild, rule, others, restore, true, cx);
    }

    /// `saveRule`.
    fn automod_save_rule(
        &mut self,
        guild: Id,
        mut rule: AutoRule,
        others: Vec<AutoRule>,
        restore: Option<AutoRule>,
        editing: bool,
        cx: &mut Context<Self>,
    ) {
        rule.tidy();
        if editing && let Some(state) = self.automod_mut() {
            state.editing = Some(rule.clone());
        }
        if let Err(message) = check_rule(&rule, &others) {
            if let Some(state) = self.automod_mut() {
                state.error = Some(message);
                state.saving = false;
                if let Some(restore) = restore {
                    state.update_rule(restore);
                }
            }
            cx.notify();
            return;
        }
        let exists = self.automod_role_exists(guild);
        let Some(state) = self.automod_mut() else {
            return;
        };
        state.saving = true;
        let key = state.next_key(SAVED_PREFIX);
        state.pending = Some(Pending {
            key: key.clone(),
            restore,
        });
        let (method, route, body) = if rule.is_saved() && !rule.is_default() {
            (
                reqwest::Method::PATCH,
                format!("/guilds/{guild}/auto-moderation/rules/{}", rule.id),
                rule.body(&exists),
            )
        } else {
            (
                reqwest::Method::POST,
                format!("/guilds/{guild}/auto-moderation/rules"),
                rule.create_body(&exists),
            )
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(key),
            method,
            route,
            body: Some(body),
        });
        cx.notify();
    }

    /// `cancelEditingRule`.
    fn automod_cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.automod_mut() {
            state.editing = None;
            state.has_changes = false;
            state.saving = false;
            state.error = None;
            state.fields = None;
            state.picker_open = false;
        }
        cx.notify();
    }

    /// `m` of a saved card: its switch saves at once.
    fn automod_toggle(&mut self, guild: Id, rule: &AutoRule, cx: &mut Context<Self>) {
        if self.automod().is_some_and(|state| state.saving) {
            return;
        }
        let restore = rule.clone();
        let mut next = rule.clone();
        next.enabled = !next.enabled;
        if let Some(state) = self.automod_mut() {
            state.update_rule(next.clone());
        }
        self.automod_save_rule(guild, next, Vec::new(), Some(restore), false, cx);
    }

    /// `onToggleAction`.
    fn automod_toggle_action(
        &mut self,
        kind: u64,
        edit_link: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rule) = self.automod().and_then(|state| state.editing.clone()) else {
            return;
        };
        let current = rule.action(kind).cloned();
        let exists = current.is_some();
        if exists && !edit_link {
            self.automod_change(|rule| rule.with_action(kind, None), cx);
            return;
        }
        let action = if edit_link {
            current.unwrap_or_else(|| default_action(kind))
        } else {
            default_action(kind)
        };
        let dialog = match kind {
            BLOCK_MESSAGE => {
                let text = action["metadata"]["custom_message"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let (editor, events) = self.automod_editor(
                    Field::Message,
                    "Enter your custom message",
                    &text,
                    field_style(16., 20.),
                    cx,
                );
                let handle = editor.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
                Dialog::Message {
                    editor,
                    _events: events,
                }
            }
            FLAG_TO_CHANNEL => Dialog::Alert {
                is_edit: exists,
                channel: action["metadata"]["channel_id"].as_str().map(str::to_owned),
                error: false,
                open: false,
            },
            TIMEOUT => Dialog::Timeout {
                is_edit: exists,
                seconds: action["metadata"]["duration_seconds"]
                    .as_u64()
                    .filter(|seconds| *seconds > 0)
                    .unwrap_or(60),
                open: false,
            },
            _ => {
                // Block member interactions has no dialog (`null != n`).
                self.automod_change(|rule| rule.with_action(kind, Some(action)), cx);
                return;
            }
        };
        if let Some(state) = self.automod_mut() {
            state.dialog = Some(dialog);
        }
        cx.notify();
    }

    fn automod_close_dialog(&mut self) {
        if let Some(state) = self.automod_mut() {
            state.dialog = None;
        }
    }

    /// `onSubmit`, `onEditChannel`, `onUpdateDuration`.
    fn automod_submit_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.automod_mut() else {
            return;
        };
        match state.dialog.as_mut() {
            Some(Dialog::Message { editor, .. }) => {
                let text = editor.read(cx).text().to_owned();
                self.automod_close_dialog();
                self.automod_change(
                    |rule| {
                        let mut action = rule
                            .action(BLOCK_MESSAGE)
                            .cloned()
                            .unwrap_or_else(|| default_action(BLOCK_MESSAGE));
                        action["metadata"]["custom_message"] = json!(text);
                        rule.with_action(BLOCK_MESSAGE, Some(action));
                    },
                    cx,
                );
            }
            Some(Dialog::Alert { channel, error, .. }) => {
                let Some(channel) = channel.clone() else {
                    *error = true;
                    cx.notify();
                    return;
                };
                self.automod_close_dialog();
                self.automod_change(
                    |rule| {
                        let action =
                            json!({"type": FLAG_TO_CHANNEL, "metadata": {"channel_id": channel}});
                        rule.with_action(FLAG_TO_CHANNEL, Some(action));
                        // `t.exemptChannels.add(e)`: the alert channel itself is exempt.
                        if !rule.exempt_channels.contains(&channel) {
                            rule.exempt_channels.push(channel);
                        }
                    },
                    cx,
                );
            }
            Some(Dialog::Timeout { seconds, .. }) => {
                let seconds = *seconds;
                self.automod_close_dialog();
                self.automod_change(
                    |rule| {
                        rule.with_action(
                            TIMEOUT,
                            Some(
                                json!({"type": TIMEOUT, "metadata": {"duration_seconds": seconds}}),
                            ),
                        );
                    },
                    cx,
                );
            }
            None => {}
        }
    }

    /// The Delete Rule confirmation's yes.
    fn automod_delete(&mut self, guild: Id, rule: &AutoRule, cx: &mut Context<Self>) {
        let Some(state) = self.automod_mut() else {
            return;
        };
        if state.deleting {
            return;
        }
        state.deleting = true;
        state.confirm = None;
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{DELETED_PREFIX}{}", rule.id)),
            method: reqwest::Method::DELETE,
            route: format!("/guilds/{guild}/auto-moderation/rules/{}", rule.id),
            body: None,
        });
        cx.notify();
    }

    /// Escape: the open dialog, list or menu first.
    /// The page itself, or the Community Safety step that shows it (`uW.C.AUTOMOD`).
    fn automod_on_screen(open: &super::server_settings::ServerSettings) -> bool {
        open.page == super::server_settings::Page::AutoMod
            || (open.page == super::server_settings::Page::SafetySetup
                && open.safety.step == super::server_safety_community::SafetyStep::AutoMod)
    }

    /// `u9`: enabled rules and all rules, a trigger with none counting as one that is off.
    pub(super) fn automod_counts(&self) -> (usize, usize) {
        let Some(open) = self.server_settings.as_ref() else {
            return (0, 0);
        };
        let community = self
            .stores
            .guild(open.guild)
            .is_some_and(|known| known.features.iter().any(|feature| feature == "COMMUNITY"));
        let rules = open.automod.as_ref().map_or(&[][..], |state| &state.rules);
        rule_counts(rules, community)
    }

    pub(super) fn close_automod_overlay(&mut self) -> bool {
        let Some(state) = self.automod_mut() else {
            return false;
        };
        match state.dialog.as_mut() {
            Some(Dialog::Alert { open, .. } | Dialog::Timeout { open, .. }) if *open => {
                *open = false;
                return true;
            }
            Some(_) => {
                state.dialog = None;
                return true;
            }
            None => {}
        }
        if state.confirm.take().is_some() || state.menu.take().is_some() {
            return true;
        }
        if state.picker_open {
            state.picker_open = false;
            return true;
        }
        if let Some(fields) = state.fields.as_mut()
            && !fields.selected.is_empty()
        {
            fields.selected.clear();
            return true;
        }
        false
    }

    /// The page (`iC`).
    pub(super) fn server_automod_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.automod() else {
            return div().into_any_element();
        };
        let community = self.automod_community(guild);
        let mut page = div()
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("AutoMod"),
            )
            .child(
                div()
                    .pt(px(8.))
                    .pb(px(16.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child("Give your mods a break while keeping your server safe! Set up filters to moderate content and automate a custom response when they\u{2019}re found, and AutoMod will make it happen."),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(self.text_with_link(
                                "automod-learn-more",
                                "",
                                "Learn More",
                                "",
                                cx,
                                |_, cx| cx.open_url(LEARN_MORE),
                            )),
                    ),
            );
        if state.loaded {
            for (title, kinds) in sections(community) {
                let mut section = div().pb(px(24.)).child(
                    div()
                        .pb(px(12.))
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .text_color(color("--text-strong"))
                        .child(title),
                );
                let count = kinds.len();
                for (at, kind) in kinds.into_iter().enumerate() {
                    section = section.child(self.automod_kind(guild, kind, at + 1 == count, cx));
                }
                page = page.child(section);
            }
        }
        // `th`.
        let content = self.safety_content();
        let learn_more = self.text_with_link(
            "automod-content-link",
            "Choose if server members can share image-based media detected by Discord\u{2019}s sensitive content filters. This setting will apply to channels that are not age-restricted. ",
            "Learn more",
            "",
            cx,
            |_, cx| cx.open_url(super::server_safety::CONTENT_ARTICLE),
        );
        page = page.child(
            div()
                .mt(px(16.))
                .mb(px(24.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child("Sensitive content filters"),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(learn_more),
                        ),
                )
                .child(self.safety_picker(
                    guild,
                    1,
                    "Change Explicit Content Filter Level",
                    content,
                    super::server_safety::content_options(community),
                    cx,
                )),
        );
        if state.editing.is_some() {
            page = page.child(div().pb(px(200.)));
        }
        page.into_any_element()
    }

    /// `iI`: one trigger's rules, the card to set one up, and the row to add another.
    fn automod_kind(&self, guild: Id, kind: u64, last: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.automod() else {
            return div().into_any_element();
        };
        let rules = state.rules_of(kind);
        let spec = trigger(kind);
        let editing = state.editing.as_ref();
        let mut list = div();
        if rules.is_empty() {
            let fresh = self.automod_new_rule(guild, kind);
            list = list.child(self.automod_card(guild, fresh, last, cx));
        }
        let count = rules.len();
        let fresh_id = format!("{guild}-{kind}-new-rule");
        let row_shown =
            spec.per_guild > count && count > 0 && editing.is_none_or(|known| known.id == fresh_id);
        for (at, rule) in rules.iter().enumerate() {
            let is_last = last && at + 1 == count && !row_shown;
            list = list.child(self.automod_card(guild, (*rule).clone(), is_last, cx));
        }
        if spec.per_guild > count && count > 0 {
            // `ip`.
            let fresh = self.automod_new_rule(guild, kind);
            let expanded = editing.is_some_and(|known| known.id == fresh.id);
            if expanded {
                list = list.child(
                    div()
                        .mb(px(16.))
                        .child(self.automod_card(guild, fresh, true, cx)),
                );
            } else if editing.is_none() {
                list = list.child(
                    div().mb(px(16.)).child(
                        div()
                            .id(SharedString::from(format!("automod-create-{kind}")))
                            .h(px(60.))
                            .rounded(px(5.))
                            .border_2()
                            .border_dashed()
                            .border_color(color("--border-subtle"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.))
                            .child(icon("access_circle_plus", 20., "--blue-345"))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-link"))
                                    .child(format!("Create {}", spec.name)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let fresh = this.automod_new_rule(guild, kind);
                                this.automod_set_editing(Some(fresh), cx);
                            })),
                    ),
                );
            }
        }
        list.into_any_element()
    }

    /// `ij` and `ti`: one rule's card.
    fn automod_card(
        &self,
        guild: Id,
        rule: AutoRule,
        last: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.automod() else {
            return div().into_any_element();
        };
        let editing = state
            .editing
            .as_ref()
            .filter(|known| known.id == rule.id)
            .cloned();
        let is_open = editing.is_some();
        let stuck = is_open && state.has_changes;
        let shown = editing.unwrap_or_else(|| rule.clone());
        let setup = !rule.is_saved() && !is_open && !rule.is_default();
        let header = if setup {
            self.automod_setup_header(guild, rule.trigger_type, cx)
        } else if is_open {
            self.automod_edit_header(&shown, cx)
        } else {
            self.automod_rule_header(guild, &shown, cx)
        };
        let clicked = rule.clone();
        let menu_rule = shown.clone();
        let header = div()
            .id(SharedString::from(format!("automod-header-{}", rule.id)))
            .when(!stuck && !is_open, |this| this.cursor_pointer())
            .when(!stuck, |this| {
                this.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.automod_card_clicked(clicked.clone(), cx);
                }))
            })
            .child(header);
        div()
            .id(SharedString::from(format!("automod-card-{}", rule.id)))
            .when(!last, |this| this.mb(px(8.)))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    if let Some(state) = this.automod_mut() {
                        state.menu = Some(Menu {
                            at: event.position,
                            rule: menu_rule.clone(),
                        });
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(header)
            .when(is_open, |this| {
                this.child(self.automod_card_body(guild, &shown, cx))
            })
            .into_any_element()
    }

    fn automod_rule_icon(glyph: &'static str) -> gpui::Div {
        div()
            .flex_none()
            .size(px(40.))
            .mr(px(16.))
            .rounded_full()
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .justify_center()
            .child(icon(glyph, 24., "--text-default"))
    }

    /// `il`: an action in a pill.
    fn automod_pill(&self, kind: u64, action: Option<&Value>, guild: Id) -> gpui::Div {
        let helper = action.and_then(|action| match kind {
            FLAG_TO_CHANNEL => {
                let channel: Id = action["metadata"]["channel_id"].as_str()?.parse().ok()?;
                self.stores
                    .guild(guild)?
                    .channels
                    .iter()
                    .find(|known| known.id == channel)
                    .map(|known| format!("(#{})", known.name))
            }
            TIMEOUT => duration_label(action["metadata"]["duration_seconds"].as_u64().unwrap_or(0))
                .map(|label| format!("({label})")),
            _ => None,
        });
        div()
            .h(px(24.))
            .pl(px(6.))
            .pr(px(8.))
            .rounded(px(80.))
            .bg(color("--background-mod-normal"))
            .flex()
            .items_center()
            .child(
                div()
                    .size(px(16.))
                    .mr(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(action_glyph(kind), 14., "--text-muted")),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--interactive-text-default"))
                    .child(action_title(kind).to_lowercase()),
            )
            .children(helper.map(|helper| {
                div()
                    .ml(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-muted"))
                    .child(helper.to_lowercase())
            }))
    }

    fn automod_row() -> gpui::Div {
        div().py(px(12.)).px(px(16.)).flex().items_center()
    }

    fn automod_text_column() -> gpui::Div {
        div().flex_1().min_w(px(0.)).mr(px(8.)).overflow_hidden()
    }

    fn automod_title(text: impl Into<SharedString>) -> gpui::Div {
        div()
            .max_w(px(475.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"))
            .child(text.into())
    }

    fn automod_description(text: impl Into<SharedString>) -> gpui::Div {
        div()
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .child(text.into())
    }

    /// `ix`.
    fn automod_setup_header(&self, guild: Id, kind: u64, cx: &mut Context<Self>) -> AnyElement {
        let spec = trigger(kind);
        let mut pills = div().mt(px(4.)).flex().flex_wrap().gap(px(4.));
        for action in spec.actions {
            pills = pills.child(self.automod_pill(*action, None, guild));
        }
        Self::automod_row()
            .child(Self::automod_rule_icon(trigger_glyph(kind)))
            .child(
                Self::automod_text_column()
                    .child(Self::automod_title(spec.name))
                    .child(Self::automod_description(trigger_description(kind)))
                    .child(pills),
            )
            .child(
                div()
                    .id(SharedString::from(format!("automod-setup-{kind}")))
                    .flex_none()
                    .h(px(32.))
                    .px(px(11.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--control-primary-border-default"))
                    .bg(color("--control-primary-background-default"))
                    .hover(|style| style.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(gpui::white())
                    .child(if kind == KEYWORD { "Create" } else { "Set Up" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        // `createNewEditingRule`: no question even with changes in another rule.
                        let fresh = this.automod_new_rule(guild, kind);
                        this.automod_set_editing(Some(fresh), cx);
                    })),
            )
            .into_any_element()
    }

    fn automod_more(&self, rule: &AutoRule, cx: &mut Context<Self>) -> AnyElement {
        let menu_rule = rule.clone();
        let more = div()
            .id(SharedString::from(format!("automod-more-{}", rule.id)))
            .size(px(24.))
            .cursor_pointer()
            .child(icon("automod_more", 24., "--interactive-text-default"))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if let Some(state) = this.automod_mut() {
                    state.menu = Some(Menu {
                        at: event.position(),
                        rule: menu_rule.clone(),
                    });
                }
                cx.notify();
            }));
        div()
            .flex_none()
            .ml(px(8.))
            .child(self.with_tip(
                format!("automod-more-tip-{}", rule.id),
                Tip::text("More", Side::Top),
                more,
                cx,
            ))
            .into_any_element()
    }

    /// `ih`: a saved rule, closed.
    fn automod_rule_header(
        &self,
        guild: Id,
        rule: &AutoRule,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut pills = div().mt(px(4.)).flex().flex_wrap().gap(px(4.));
        for action in rule.shown_actions() {
            pills = pills.child(self.automod_pill(action_kind(action), Some(action), guild));
        }
        let subtext = rule_subtext(rule);
        let toggled = rule.clone();
        Self::automod_row()
            .child(Self::automod_rule_icon(trigger_glyph(rule.trigger_type)))
            .child(
                Self::automod_text_column()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(Self::automod_title(rule.name.clone()))
                            .when(!subtext.is_empty(), |this| {
                                this.child(
                                    div()
                                        .ml(px(4.))
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--interactive-text-default"))
                                        .child(subtext),
                                )
                            }),
                    )
                    .when(!rule.is_saved(), |this| {
                        this.child(Self::automod_description(trigger_description(
                            rule.trigger_type,
                        )))
                    })
                    .child(pills),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id(SharedString::from(format!("automod-switch-{}", rule.id)))
                            .cursor_pointer()
                            .child(super::create_channel::switch(rule.enabled))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.automod_toggle(guild, &toggled, cx);
                            })),
                    )
                    .when(!rule.is_default(), |this| {
                        this.child(self.automod_more(rule, cx))
                    }),
            )
            .into_any_element()
    }

    /// `im`: the open card's name and switch.
    fn automod_edit_header(&self, rule: &AutoRule, cx: &mut Context<Self>) -> AnyElement {
        let Some(fields) = self.automod().and_then(|state| state.fields.as_ref()) else {
            return div().into_any_element();
        };
        let name = fields.name.clone();
        let focus = name.clone();
        let on = rule.enabled;
        Self::automod_row()
            .child(
                Self::automod_text_column()
                    .child(
                        div()
                            .mb(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Rule Name"),
                    )
                    .child(
                        div()
                            .id("automod-name")
                            .h(px(40.))
                            .pt(px(2.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .cursor_text()
                            .flex()
                            .items_center()
                            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                                cx.stop_propagation();
                                let handle = focus.read(cx).focus_handle(cx);
                                window.focus(&handle, cx);
                            }))
                            .child(div().w_full().child(name)),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(104.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(
                        div()
                            .id("automod-enabled")
                            .cursor_pointer()
                            .child(super::create_channel::switch(on))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.automod_change(|rule| rule.enabled = !on, cx);
                            })),
                    )
                    .child(self.automod_more(rule, cx)),
            )
            .into_any_element()
    }

    /// `n$`: the steps for the rule's kind.
    fn automod_card_body(&self, guild: Id, rule: &AutoRule, cx: &mut Context<Self>) -> AnyElement {
        let (first, second): (AnyElement, AnyElement);
        let exempt_title;
        match rule.trigger_type {
            ML_SPAM => {
                first = Self::automod_step(1, "If message is suspected of being spam...", None);
                exempt_title = "Allow certain roles or channels (optional)";
            }
            MENTION_SPAM => {
                first = self.automod_mention_step(guild, rule, cx);
                exempt_title = "Allow certain roles or channels (optional)";
            }
            DEFAULT_KEYWORD_LIST => {
                first = self.automod_flagged_step(rule, cx);
                exempt_title = "Allow certain roles or channels (optional)";
            }
            USER_PROFILE => {
                first = self.automod_words_step(rule, cx);
                exempt_title = "Allow certain roles (optional)";
            }
            _ => {
                first = self.automod_words_step(rule, cx);
                exempt_title = "Allow certain roles or channels (optional)";
            }
        }
        second = Self::automod_step(
            2,
            "Choose a response",
            Some(self.automod_actions(guild, rule, cx)),
        );
        let exempt = self.automod_exempt(guild, rule, cx);
        div()
            .mx(px(16.))
            .pb(px(16.))
            .child(first)
            .child(Self::automod_connector("automod_arrow"))
            .child(second)
            .child(Self::automod_connector("automod_cross"))
            .child(Self::automod_step(3, exempt_title, Some(exempt)))
            .into_any_element()
    }

    /// `tJ`: 24 high, the glyph starting at the column's middle.
    fn automod_connector(glyph: &'static str) -> gpui::Div {
        div()
            .h(px(24.))
            .pl(px(313.))
            .child(icon(glyph, 24., "--border-subtle"))
    }

    /// `tW`.
    fn automod_step(step: usize, header: &'static str, body: Option<AnyElement>) -> AnyElement {
        div()
            .flex()
            .child(
                div().flex_none().w(px(32.)).mr(px(8.)).mt(px(6.)).child(
                    div()
                        .size(px(32.))
                        .rounded_full()
                        .bg(color("--background-mod-normal"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(step.to_string()),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(header),
                    )
                    .children(body.map(|body| div().pt(px(8.)).child(body))),
            )
            .into_any_element()
    }

    /// `ni.P`.
    fn automod_checkbox(checked: bool, disabled: bool) -> gpui::Div {
        div()
            .relative()
            .flex_none()
            .size(px(20.))
            .rounded(px(4.))
            .border_1()
            .border_color(if checked {
                color("--checkbox-border-selected-default")
            } else {
                color("--checkbox-border-default")
            })
            .bg(if checked {
                color("--checkbox-background-selected-default")
            } else {
                color("--checkbox-background-default")
            })
            .when(disabled, |this| this.opacity(0.5))
            .when(checked, |this| {
                this.child(div().absolute().left(px(0.)).top(px(0.)).child(icon(
                    "automod_check",
                    18.,
                    "--checkbox-icon-active",
                )))
            })
    }

    /// `nZ`'s first step.
    fn automod_mention_step(
        &self,
        guild: Id,
        rule: &AutoRule,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(editor) = self
            .automod()
            .and_then(|state| state.fields.as_ref())
            .and_then(|fields| fields.mention.clone())
        else {
            return div().into_any_element();
        };
        let shown = editor.read(cx).text().to_owned();
        let parsed = shown.parse::<i64>().ok();
        let can_lower = parsed.is_some_and(|value| value > MENTION_MIN);
        let can_raise = parsed.is_some_and(|value| value < MENTION_MAX);
        let step_button = |id: &'static str,
                           glyph: &'static str,
                           enabled: bool,
                           by: i64,
                           cx: &mut Context<Self>| {
            let target = editor.clone();
            div()
                .id(id)
                .size(px(32.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .when(!enabled, |this| this.opacity(0.5))
                .when(enabled, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(color("--control-icon-only-background-hover")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let value =
                                target.read(cx).text().parse::<i64>().unwrap_or(MENTION_MIN) + by;
                            target.update(cx, |editor, _| editor.set_text(&value.to_string()));
                            this.automod_change(
                                |rule| rule.set_meta("mention_total_limit", json!(value)),
                                cx,
                            );
                        }))
                })
                .child(icon(glyph, 16., "--control-icon-only-icon-default"))
        };
        let minus = step_button("automod-mention-minus", "automod_minus", can_lower, -1, cx);
        let plus = step_button("automod-mention-plus", "automod_plus", can_raise, 1, cx);
        let focus = editor.clone();
        let limit_row =
            Self::automod_setting_row("automod_at", "Unique mentions (role + user) per message")
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(Self::automod_setting_title(
                            "Unique mentions (role + user) per message",
                        ))
                        .child(
                            Self::automod_setting_text(
                                "Limit the number of unique mentions in each message.",
                            )
                            .mt(px(2.)),
                        ),
                )
                .child(
                    div().flex_none().p(px(4.)).child(
                        div()
                            .h(px(40.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(minus)
                            .child(
                                div()
                                    .id("automod-mention-value")
                                    .w(px(48.))
                                    .h(px(40.))
                                    .pt(px(2.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .cursor_text()
                                    .flex()
                                    .items_center()
                                    .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                                        let handle = focus.read(cx).focus_handle(cx);
                                        window.focus(&handle, cx);
                                    }))
                                    .child(div().w_full().child(editor.clone())),
                            )
                            .child(plus),
                    ),
                );
        let mut body = div().child(limit_row);
        if self.automod_community(guild) {
            let on = rule.trigger_metadata["mention_raid_protection_enabled"] == true;
            body = body.child(
                Self::automod_setting_row("automod_at", "")
                    .mt(px(4.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(Self::automod_setting_title("Enable mention raid detection"))
                            .child(
                                Self::automod_setting_text(
                                    "Allow automatic detection of mention raids in the server.",
                                )
                                .mt(px(2.)),
                            ),
                    )
                    .child(
                        div()
                            .id("automod-mention-raid")
                            .cursor_pointer()
                            .child(Self::automod_checkbox(on, false))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.automod_change(
                                    |rule| {
                                        rule.set_meta(
                                            "mention_raid_protection_enabled",
                                            json!(!on),
                                        );
                                    },
                                    cx,
                                );
                            })),
                    ),
            );
        }
        Self::automod_step(1, "Configure limits", Some(body.into_any_element()))
    }

    fn automod_setting_row(glyph: &'static str, _title: &'static str) -> gpui::Div {
        div()
            .py(px(8.))
            .px(px(12.))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_none()
                    .size(px(24.))
                    .mr(px(12.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(glyph, 24., "--text-muted")),
            )
    }

    fn automod_setting_title(text: &'static str) -> gpui::Div {
        div()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"))
            .child(text)
    }

    fn automod_setting_text(text: &'static str) -> gpui::Div {
        div()
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--interactive-text-default"))
            .child(text)
    }

    /// `nh`: the actions the kind offers.
    fn automod_actions(&self, guild: Id, rule: &AutoRule, cx: &mut Context<Self>) -> AnyElement {
        let kinds = trigger(rule.trigger_type).actions;
        let count = kinds.len();
        let mut list = div();
        for (at, kind) in kinds.iter().enumerate() {
            list = list.child(
                div()
                    .when(at + 1 != count, |this| this.mb(px(4.)))
                    .child(self.automod_action_row(guild, rule, *kind, cx)),
            );
        }
        list.into_any_element()
    }

    /// `nr`, `nc`, `nu`, `nm`.
    fn automod_action_row(
        &self,
        guild: Id,
        rule: &AutoRule,
        kind: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let action = rule.action(kind);
        let toggled = action.is_some();
        let known = self.stores.guild(guild);
        let can_timeout = known.zip(self.stores.me()).is_some_and(|(known, me)| {
            permissions::can_in_guild(known, me, permissions::MODERATE_MEMBERS)
        });
        let editable = match kind {
            QUARANTINE_USER => false,
            TIMEOUT => can_timeout,
            _ => true,
        };
        // The line under the description, with what it names in bold and the link at its end.
        let extra: Option<ActionLine> = if !toggled {
            None
        } else {
            match kind {
                BLOCK_MESSAGE => Some((
                    "A custom error message can be shown to members.".to_owned(),
                    None,
                    "Edit Custom Message",
                    true,
                )),
                FLAG_TO_CHANNEL => {
                    let channel = action
                        .and_then(|action| action["metadata"]["channel_id"].as_str())
                        .and_then(|channel| channel.parse::<Id>().ok())
                        .and_then(|channel| {
                            known?.channels.iter().find(|known| known.id == channel)
                        })
                        .map(|channel| channel.name.clone());
                    Some(match channel {
                        Some(name) => {
                            let start = "Alert message will be sent to ".len();
                            let bold = format!("(#{name})");
                            (
                                format!("Alert message will be sent to {bold}."),
                                Some((start, start + bold.len())),
                                "Edit Channel",
                                true,
                            )
                        }
                        None => (String::new(), None, "Edit Channel", true),
                    })
                }
                TIMEOUT => {
                    let label = action
                        .and_then(|action| action["metadata"]["duration_seconds"].as_u64())
                        .and_then(duration_label);
                    let lead = if rule.trigger_type == MENTION_SPAM {
                        "Members who exceed your set mention limit will be timed out for: "
                    } else {
                        "Members who use blocked words will be timed out for: "
                    };
                    let text = label.map_or(String::new(), |label| format!("{lead}{label}"));
                    let bold = label.map(|label| (lead.len(), lead.len() + label.len()));
                    Some((text, bold, "Edit Duration", can_timeout))
                }
                _ => None,
            }
        };
        let line = extra.map(|(text, bold, link, linked)| {
            let tone = if kind == TIMEOUT {
                "--interactive-text-default"
            } else {
                "--text-muted"
            };
            let highlights: Vec<_> = bold
                .map(|(from, to)| {
                    (
                        from..to,
                        gpui::HighlightStyle {
                            font_weight: Some(FontWeight(600.)),
                            ..Default::default()
                        },
                    )
                })
                .into_iter()
                .collect();
            div()
                .flex()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color(tone))
                .when(!text.is_empty(), |this| {
                    this.child(
                        div()
                            .min_w(px(0.))
                            .child(gpui::StyledText::new(text).with_highlights(highlights)),
                    )
                })
                .when(linked, |this| {
                    this.child(
                        div()
                            .id(SharedString::from(format!("automod-action-link-{kind}")))
                            .flex_none()
                            .pl(px(4.))
                            .cursor_pointer()
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-link"))
                            .child(link)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.automod_toggle_action(kind, true, window, cx);
                            })),
                    )
                })
        });
        let check = div()
            .id(SharedString::from(format!("automod-action-{kind}")))
            .when(editable, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.automod_toggle_action(kind, false, window, cx);
                    }))
            })
            .child(Self::automod_checkbox(toggled, !editable));
        let check: AnyElement = if kind == TIMEOUT && !can_timeout {
            self.with_tip(
                "automod-timeout-tip",
                Tip::body(
                    Body::Text(
                        "Timeout Members permission is required to configure this action.".into(),
                    ),
                    Side::Top,
                ),
                check,
                cx,
            )
            .into_any_element()
        } else {
            check.into_any_element()
        };
        Self::automod_setting_row(action_glyph(kind), "")
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(Self::automod_setting_title(action_title(kind)))
                    .child(Self::automod_setting_text(action_description(
                        kind,
                        rule.trigger_type,
                    )))
                    .children(line),
            )
            .child(check)
            .into_any_element()
    }

    /// `resize: vertical` on a text area and `nR` on the regex box. The text area has the browser's two strokes in its
    /// corner, measured in the capture; the regex box's handle is its invisible lower edge.
    fn automod_resizer(
        &self,
        which: Resized,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        const STROKES: [(f32, f32); 8] = [
            (12., 7.),
            (11., 8.),
            (10., 9.),
            (9., 10.),
            (8., 11.),
            (7., 12.),
            (12., 11.),
            (11., 12.),
        ];
        let dot = |x: f32, y: f32, value: u32| {
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .size(px(1.))
                .bg(gpui::rgb(value))
        };
        let regex = which == Resized::Regex;
        div()
            .id(match which {
                Resized::Keywords => "automod-keywords-resize",
                Resized::Allow => "automod-allow-resize",
                Resized::Regex => "automod-regex-resize",
            })
            .absolute()
            .right(px(0.))
            .bottom(px(0.))
            .when(regex, |this| this.left(px(0.)).h(px(8.)))
            .when(!regex, |this| this.size(px(15.)))
            .cursor_ns_resize()
            .when(!regex, |this| {
                this.children(STROKES.into_iter().map(|(x, y)| dot(x, y - 1., 0x070707)))
                    .children(STROKES.into_iter().map(|(x, y)| dot(x, y, 0xa0a0a0)))
            })
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, _| {
                    if let Some(fields) = this.automod_mut().and_then(|state| state.fields.as_mut())
                    {
                        fields.resize_from = (f32::from(event.position.y), height);
                    }
                }),
            )
            .on_drag(ResizeDrag(which), |_, _, _, cx| cx.new(|_| ResizeGhost))
            .on_drag_move::<ResizeDrag>(cx.listener(
                move |this, event: &gpui::DragMoveEvent<ResizeDrag>, _, cx| {
                    if event.drag(cx).0 != which {
                        return;
                    }
                    if let Some(fields) = this.automod_mut().and_then(|state| state.fields.as_mut())
                    {
                        let (from_y, from_height) = fields.resize_from;
                        let moved = f32::from(event.event.position.y) - from_y;
                        let height = resized_height(which, from_height, moved);
                        match which {
                            Resized::Keywords => fields.keywords_height = height,
                            Resized::Allow => fields.allow_height = height,
                            Resized::Regex => fields.regex_height = height,
                        }
                    }
                    cx.notify();
                },
            ))
    }

    /// `t5`: a word list with its count and the words under it.
    fn automod_words_field(&self, spec: WordsField, cx: &mut Context<Self>) -> AnyElement {
        let WordsField {
            field,
            editor,
            count,
            limit,
            lowest,
            hint,
            error,
        } = spec;
        let focus = editor.clone();
        let id = if field == Field::Keywords {
            "automod-keywords"
        } else {
            "automod-allow"
        };
        let (which, height) = {
            let fields = self.automod().and_then(|state| state.fields.as_ref());
            if field == Field::Keywords {
                (
                    Resized::Keywords,
                    fields.map_or(78., |fields| fields.keywords_height),
                )
            } else {
                (
                    Resized::Allow,
                    fields.map_or(78., |fields| fields.allow_height),
                )
            }
        };
        div()
            .child(
                div()
                    .relative()
                    .pb(px(3.))
                    .child(
                        div()
                            .id(id)
                            .relative()
                            .min_h(px(height))
                            .pt(px(12.))
                            .px(px(12.))
                            .pb(px(24.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(if lowest {
                                color("--background-base-lowest")
                            } else {
                                color("--input-background-default")
                            })
                            .cursor_text()
                            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                                let handle = focus.read(cx).focus_handle(cx);
                                window.focus(&handle, cx);
                            }))
                            .capture_key_down(cx.listener(
                                move |this, event: &KeyDownEvent, _, cx| {
                                    if event.keystroke.key == "v"
                                        && (event.keystroke.modifiers.control
                                            || event.keystroke.modifiers.platform)
                                        && cx
                                            .read_from_clipboard()
                                            .and_then(|item| item.text())
                                            .is_some_and(|text| !text.is_empty())
                                        && let Some(fields) = this
                                            .automod_mut()
                                            .and_then(|state| state.fields.as_mut())
                                    {
                                        fields.pasting = true;
                                    }
                                },
                            ))
                            .child(div().w_full().child(editor))
                            .child(self.automod_resizer(which, height, cx)),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(12.))
                            .bottom(px(12.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(format!("{count} / {limit}")),
                    ),
            )
            .child(match error {
                Some(error) => div()
                    .pt(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error),
                None => div()
                    .pt(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-default"))
                    .child(hint),
            })
            .into_any_element()
    }

    /// `eV.N`: a section that opens under its title.
    fn automod_collapse(
        id: &'static str,
        title: &'static str,
        open: bool,
        cx: &mut Context<Self>,
        toggle: impl Fn(&mut Fields) + 'static,
    ) -> Stateful<gpui::Div> {
        div()
            .id(id)
            .mt(px(16.))
            .py(px(8.))
            .px(px(12.))
            .rounded(px(8.))
            .bg(color("--background-base-low"))
            .child(
                div()
                    .id(SharedString::from(format!("{id}-header")))
                    .h(px(24.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(icon(
                        if open {
                            "automod_chevron_up"
                        } else {
                            "automod_chevron_down"
                        },
                        24.,
                        "--interactive-text-default",
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(fields) =
                            this.automod_mut().and_then(|state| state.fields.as_mut())
                        {
                            toggle(fields);
                        }
                        cx.notify();
                    })),
            )
    }

    /// `t4`.
    fn automod_allow_section(&self, rule: &AutoRule, cx: &mut Context<Self>) -> AnyElement {
        let Some(fields) = self.automod().and_then(|state| state.fields.as_ref()) else {
            return div().into_any_element();
        };
        let Some(editor) = fields.allow.clone() else {
            return div().into_any_element();
        };
        let open = fields.allow_open;
        let error = fields.allow_error.clone();
        let limit = if rule.trigger_type == DEFAULT_KEYWORD_LIST {
            1000
        } else {
            100
        };
        let count = rule.list("allow_list").len();
        let body = open.then(|| {
            div().pt(px(8.)).child(self.automod_words_field(
                WordsField {
                    field: Field::Allow,
                    editor,
                    count,
                    limit,
                    lowest: true,
                    hint: ALLOW_HINT,
                    error,
                },
                cx,
            ))
        });
        Self::automod_collapse(
            "automod-allow-section",
            "Allow words and phrases (optional)",
            open,
            cx,
            |fields| fields.allow_open = !fields.allow_open,
        )
        .children(body)
        .into_any_element()
    }

    /// `nz` and `nH`.
    fn automod_regex_section(&self, rule: &AutoRule, cx: &mut Context<Self>) -> AnyElement {
        let Some(fields) = self.automod().and_then(|state| state.fields.as_ref()) else {
            return div().into_any_element();
        };
        let open = fields.regex_open;
        let body = open.then(|| self.automod_regex_body(rule, cx));
        Self::automod_collapse(
            "automod-regex-section",
            "Use regex patterns for advanced matching (optional)",
            open,
            cx,
            |fields| fields.regex_open = !fields.regex_open,
        )
        .children(body)
        .into_any_element()
    }

    /// `nL`: the patterns as tags, the input after them, and the count.
    fn automod_regex_body(&self, rule: &AutoRule, cx: &mut Context<Self>) -> AnyElement {
        let Some(fields) = self.automod().and_then(|state| state.fields.as_ref()) else {
            return div().into_any_element();
        };
        let Some(input) = fields.regex_input.clone() else {
            return div().into_any_element();
        };
        let tags = rule.list("regex_patterns");
        let mut scroller = div().p(px(8.)).flex().flex_col().gap(px(4.));
        for (index, tag) in tags.iter().enumerate() {
            let editing = fields
                .tag_edit
                .as_ref()
                .filter(|(at, _)| *at == index)
                .map(|(_, editor)| editor.clone());
            let error = fields
                .regex_errors
                .iter()
                .find(|error| error.pattern == *tag);
            let selected = fields.selected.contains(&index);
            scroller =
                scroller.child(self.automod_regex_tag(index, tag, editing, error, selected, cx));
        }
        let main = input.clone();
        scroller = scroller.child(
            div()
                .min_h(px(26.))
                .min_w(px(100.))
                .p(px(5.))
                .when(fields.tag_edit.is_some(), |this| this.opacity(0.))
                .child(div().w_full().child(main)),
        );
        let focus = input.clone();
        let whole_error = fields
            .regex_errors
            .iter()
            .find(|error| error.pattern == "_errors")
            .map(|error| error.message.clone())
            .or_else(|| {
                fields
                    .value_error
                    .as_ref()
                    .map(|error| error.message.clone())
            });
        let has_errors = !fields.regex_errors.is_empty() || fields.value_error.is_some();
        let subtext: AnyElement = if has_errors {
            match whole_error {
                Some(message) => div()
                    .pt(px(6.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-feedback-critical"))
                    .child(message)
                    .into_any_element(),
                None => div()
                    .pt(px(6.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-feedback-critical"))
                    .child(self.text_with_link(
                        "automod-regex-help-error",
                        "There are issues with your regex input. Please correct the highlighted errors above. ",
                        "Need Help?",
                        "",
                        cx,
                        |_, cx| cx.open_url(REGEX_ARTICLE),
                    ))
                    .into_any_element(),
            }
        } else {
            div()
                .pt(px(6.))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-muted"))
                .child(self.text_with_link(
                    "automod-regex-help",
                    "Use a new line for each regex pattern. Case-insensitive and global flags are applied by default (and other flags are not supported). Visit our ",
                    "Help Center",
                    " for more info.",
                    cx,
                    |_, cx| cx.open_url(REGEX_ARTICLE),
                ))
                .into_any_element()
        };
        div()
            .pt(px(8.))
            .child(
                div()
                    .id("automod-regex-box")
                    .relative()
                    .min_h(px(fields.regex_height))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(color("--background-surface-high"))
                    .bg(color("--background-base-lowest"))
                    .cursor_text()
                    .capture_key_down(cx.listener(Self::automod_regex_key))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        // `G`.
                        this.automod_finish_tag(false, window, cx);
                        if let Some(fields) =
                            this.automod_mut().and_then(|state| state.fields.as_mut())
                        {
                            fields.selected.clear();
                        }
                        let handle = focus.read(cx).focus_handle(cx);
                        window.focus(&handle, cx);
                        cx.notify();
                    }))
                    .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                        // `I` and `S(false)`: leaving the box.
                        this.automod_finish_tag(false, window, cx);
                        this.automod_commit_value(cx);
                    }))
                    .child(scroller)
                    .child(
                        div()
                            .absolute()
                            .right(px(8.))
                            .bottom(px(8.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(format!("{}/{REGEX_LIMIT}", tags.len())),
                    )
                    .child(self.automod_resizer(Resized::Regex, fields.regex_height, cx)),
            )
            .child(subtext)
            .into_any_element()
    }

    /// `nS` and `nC`: one pattern.
    fn automod_regex_tag(
        &self,
        index: usize,
        tag: &str,
        editing: Option<Entity<Editor>>,
        error: Option<&RegexError>,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_editing = editing.is_some();
        let has_error = error.is_some() && !is_editing;
        let value: AnyElement = match editing {
            Some(editor) => div().w(px(500.)).child(editor).into_any_element(),
            None => {
                let mut text = div()
                    .font_family(super::MONO)
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(if selected || has_error {
                        color("--interactive-text-active")
                    } else {
                        color("--text-default")
                    });
                if let Some(error) = error {
                    let chars: Vec<char> = tag.chars().collect();
                    let offset = error.offset.unwrap_or(0).min(chars.len());
                    let length = error
                        .length
                        .unwrap_or(chars.len())
                        .min(chars.len() - offset);
                    let from: usize = chars[..offset].iter().map(|c| c.len_utf8()).sum();
                    let to: usize = from
                        + chars[offset..offset + length]
                            .iter()
                            .map(|c| c.len_utf8())
                            .sum::<usize>();
                    let styled = gpui::StyledText::new(tag.to_owned()).with_highlights([(
                        from..to,
                        gpui::HighlightStyle {
                            background_color: Some(
                                color("--background-feedback-mod-critical").into(),
                            ),
                            ..Default::default()
                        },
                    )]);
                    text = text.child(styled);
                } else {
                    text = text.child(tag.to_owned());
                }
                text.into_any_element()
            }
        };
        let inner = div()
            .id(SharedString::from(format!("automod-regex-tag-{index}")))
            .flex_1()
            .min_w(px(0.))
            .p(px(4.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                let modifiers = event.modifiers();
                if modifiers.control || modifiers.platform {
                    // `O(e)(true)`: Ctrl-click toggles the tag's selection.
                    if let Some(fields) = this.automod_mut().and_then(|state| state.fields.as_mut())
                    {
                        if let Some(at) = fields.selected.iter().position(|known| *known == index) {
                            fields.selected.remove(at);
                        } else {
                            fields.selected.push(index);
                        }
                    }
                    cx.notify();
                } else {
                    this.automod_start_tag_edit(index, window, cx);
                }
            }))
            .child(value);
        let inner: AnyElement = match error.filter(|_| !is_editing) {
            Some(error) => self
                .with_tip(
                    format!("automod-regex-tip-{index}"),
                    Tip::body(Body::Text(error.message.clone().into()), Side::Top),
                    inner,
                    cx,
                )
                .into_any_element(),
            None => inner.into_any_element(),
        };
        div()
            .flex()
            .child(
                div()
                    .max_w(px(520.))
                    .rounded(px(8.))
                    .border_2()
                    .border_color(if has_error {
                        if selected {
                            color("--text-feedback-critical")
                        } else {
                            color("--background-feedback-mod-critical")
                        }
                    } else {
                        gpui::rgba(0)
                    })
                    .bg(if selected {
                        color("--interactive-background-selected")
                    } else if is_editing {
                        gpui::rgba(0)
                    } else if has_error {
                        color("--background-surface-high")
                    } else {
                        color("--background-mod-subtle")
                    })
                    .flex()
                    .items_center()
                    .child(inner)
                    .when(!is_editing, |this| {
                        this.child(
                            div()
                                .id(SharedString::from(format!("automod-regex-remove-{index}")))
                                .px(px(3.))
                                .cursor_pointer()
                                .child(icon("automod_cross", 12., "--text-default"))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    let mut tags = this.automod_tags();
                                    if index < tags.len() {
                                        tags.remove(index);
                                        this.automod_set_tags(tags, cx);
                                    }
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    /// `nY` and `nQ`'s first step.
    fn automod_words_step(&self, rule: &AutoRule, cx: &mut Context<Self>) -> AnyElement {
        let Some(fields) = self.automod().and_then(|state| state.fields.as_ref()) else {
            return div().into_any_element();
        };
        let Some(editor) = fields.keywords.clone() else {
            return div().into_any_element();
        };
        let words = self.automod_words_field(
            WordsField {
                field: Field::Keywords,
                editor,
                count: rule.list("keyword_filter").len(),
                limit: KEYWORD_LIMIT,
                lowest: false,
                hint: KEYWORD_HINT,
                error: fields.keywords_error.clone(),
            },
            cx,
        );
        let body = div()
            .child(words)
            .child(self.automod_regex_section(rule, cx))
            .child(self.automod_allow_section(rule, cx));
        Self::automod_step(1, "Choose your words", Some(body.into_any_element()))
    }

    /// `nx`'s first step: the lists and the allow list.
    fn automod_flagged_step(&self, rule: &AutoRule, cx: &mut Context<Self>) -> AnyElement {
        let chosen = rule.list("presets");
        // `t8`, with their words.
        let presets: [(&str, &'static str, &'static str); 3] = [
            (
                "1",
                "Severe Profanity",
                "Extreme forms of swearing or cursing",
            ),
            (
                "3",
                "Insults & Slurs",
                "Personally insulting terms and terms that may be considered hate speech",
            ),
            (
                "2",
                "Sexual Content",
                "Terms that refer to sexually explicit behavior or activity",
            ),
        ];
        let mut group = div().pt(px(8.)).pb(px(4.)).flex().flex_col().gap(px(16.));
        for (value, title, description) in presets {
            let on = chosen.iter().any(|known| known == value);
            group = group.child(
                div()
                    .id(SharedString::from(format!("automod-preset-{value}")))
                    .cursor_pointer()
                    .flex()
                    .gap(px(12.))
                    .child(Self::automod_checkbox(on, false))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(description),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.automod_change(
                            |rule| {
                                let mut list = rule.list("presets");
                                if let Some(at) = list.iter().position(|known| known == value) {
                                    list.remove(at);
                                } else {
                                    list.push(value.to_owned());
                                }
                                rule.set_list("presets", list);
                            },
                            cx,
                        );
                    })),
            );
        }
        let body = div()
            .child(group)
            .child(self.automod_allow_section(rule, cx));
        Self::automod_step(
            1,
            "Select the type of language",
            Some(body.into_any_element()),
        )
    }

    /// One picked channel or role in the field (`tag`, `richTag`).
    fn automod_tag(
        id: SharedString,
        label: AnyElement,
        cx: &mut Context<Self>,
        remove: impl Fn(&mut AutoRule) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .h(px(26.))
            .mt(px(3.))
            .mb(px(3.))
            .ml(px(3.))
            .pl(px(8.))
            .pr(px(8.))
            .rounded(px(5.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-mod-normal"))
            .hover(|style| style.bg(color("--background-mod-strong")))
            .cursor_pointer()
            .flex()
            .items_center()
            .child(label)
            .child(
                div()
                    .ml(px(4.))
                    .child(icon("automod_cross", 12., "--text-default")),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.automod_change(&remove, cx);
            }))
            .into_any_element()
    }

    /// `tz`: the search field, what is picked, and the hint.
    fn automod_exempt(&self, guild: Id, rule: &AutoRule, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.automod() else {
            return div().into_any_element();
        };
        let Some(fields) = state.fields.as_ref() else {
            return div().into_any_element();
        };
        let query = fields.query.clone();
        let known = self.stores.guild(guild);
        let mut field = div()
            .id("automod-exempt-field")
            .relative()
            .min_h(px(36.))
            .max_h(px(98.))
            .overflow_y_scroll()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .cursor_text()
            .child({
                let anchors = self.tip_anchors.clone();
                gpui::canvas(
                    move |bounds, window, _| {
                        let mut anchors = anchors.borrow_mut();
                        if anchors.get("automod-exempt") != Some(&bounds) {
                            anchors.insert("automod-exempt".into(), bounds);
                            window.request_animation_frame();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full()
            });
        let mut inner = div().w_full().p(px(1.)).flex().flex_wrap();
        for channel_id in &rule.exempt_channels {
            let channel = known.and_then(|known| {
                known
                    .channels
                    .iter()
                    .find(|channel| channel.id.to_string() == *channel_id)
            });
            let Some(channel) = channel else { continue };
            let removed = channel_id.clone();
            inner = inner.child(Self::automod_tag(
                SharedString::from(format!("automod-exempt-channel-{channel_id}")),
                div()
                    .p(px(4.))
                    .flex()
                    .child(div().mr(px(4.)).child(icon(
                        channel_glyph(channel.kind),
                        16.,
                        "--text-muted",
                    )))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(channel.name.clone()),
                    )
                    .into_any_element(),
                cx,
                move |rule| rule.exempt_channels.retain(|known| *known != removed),
            ));
        }
        for role_id in &rule.exempt_roles {
            let Some(role) = state.roles.iter().find(|role| role.id == *role_id) else {
                continue;
            };
            let removed = role_id.clone();
            inner = inner.child(Self::automod_tag(
                SharedString::from(format!("automod-exempt-role-{role_id}")),
                Self::automod_role_label(role).p(px(4.)).into_any_element(),
                cx,
                move |rule| rule.exempt_roles.retain(|known| *known != removed),
            ));
        }
        let focus = query.clone();
        let rich = !rule.exempt_channels.is_empty() || !rule.exempt_roles.is_empty();
        inner = inner.child(
            div()
                .flex_1()
                .min_w(px(48.))
                .when(rich, |this| this.h(px(26.)).my(px(3.)).ml(px(3.)))
                .when(!rich, |this| this.h(px(30.)).m(px(1.)))
                .pt(px(2.))
                .px(px(8.))
                .flex()
                .items_center()
                .child(div().w_full().child(query)),
        );
        field = field
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                let handle = focus.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
                if let Some(state) = this.automod_mut() {
                    state.picker_open = true;
                }
                cx.notify();
            }))
            .child(inner);
        div()
            .child(field)
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-default"))
                    .child(EXEMPT_HINT),
            )
            .into_any_element()
    }

    /// `w`: a role's dot, the `@` when its name has none, and its name.
    fn automod_role_label(role: &PickRole) -> gpui::Div {
        let dot = if role.color == 0 {
            color("--primary-300")
        } else {
            gpui::rgb(role.color)
        };
        div()
            .flex()
            .items_center()
            // `m.W`: a 6 radius circle in a 20 box.
            .child(
                div()
                    .flex_none()
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().size(px(12.)).rounded_full().bg(dot)),
            )
            .when(!role.name.contains('@'), |this| {
                this.child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child("@"),
                )
            })
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(role.name.clone()),
            )
    }

    /// `f.Ei`: what can still be picked, under the field.
    pub(super) fn automod_picker_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if !Self::automod_on_screen(open) {
            return None;
        }
        let state = open.automod.as_ref()?;
        if !state.picker_open {
            return None;
        }
        let rule = state.editing.as_ref()?;
        let fields = state.fields.as_ref()?;
        let anchor = self
            .tip_anchors
            .borrow()
            .get("automod-exempt")
            .copied()?
            .dilate(px(1.));
        let known = self.stores.guild(open.guild)?;
        let query = fields.query.read(cx).text().trim().to_lowercase();
        let only_roles = rule.trigger_type == USER_PROFILE;
        let me = self.stores.me();
        let visible: Vec<Channel> = known
            .channels
            .iter()
            .filter(|channel| {
                channel.kind == ChannelKind::Category
                    || me.is_some_and(|me| {
                        permissions::can_in_channel(known, channel, me, permissions::VIEW_CHANNEL)
                    })
            })
            .cloned()
            .collect();
        let channel_list: Vec<ChannelRow> = if only_roles {
            Vec::new()
        } else {
            channel_rows(&visible, &rule.exempt_channels)
                .into_iter()
                .filter(|row| query.is_empty() || fuzzy_match(&query, &row.name.to_lowercase()))
                .collect()
        };
        let role_list: Vec<PickRole> = state
            .roles
            .iter()
            .filter(|role| !rule.exempt_roles.contains(&role.id))
            .filter(|role| {
                query.is_empty() || fuzzy_match(&query, &role_display(&role.name).to_lowercase())
            })
            .cloned()
            .collect();
        let section = |title: &'static str| {
            div()
                .flex_none()
                .h(px(16.))
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(title)
        };
        let row_label = |content: gpui::Div, nested: bool| {
            div()
                .when(nested, |this| this.ml(px(12.)))
                .p(px(4.))
                .rounded(px(4.))
                .bg(color("--interactive-background-hover"))
                .flex()
                .child(content)
        };
        let mut list = div()
            // `content_tqapBM`: the list stops short of its scrollbar's gutter.
            .mr(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(!channel_list.is_empty() || !role_list.is_empty(), |this| {
                this.pt(px(8.))
            });
        // A section with no rows has no title either, and the line stands only between two.
        if !channel_list.is_empty() {
            list = list.child(section("Channels"));
            for row in &channel_list {
                let id = row.id.to_string();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("automod-pick-channel-{id}")))
                        .flex_none()
                        .h(px(26.))
                        .mr(px(8.))
                        .cursor_pointer()
                        .flex()
                        .child(row_label(
                            div()
                                .flex()
                                .child(div().mr(px(4.)).child(icon(
                                    channel_glyph(row.kind),
                                    16.,
                                    "--text-muted",
                                )))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        .child(row.name.clone()),
                                ),
                            row.nested,
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let picked = id.clone();
                            this.automod_pick(|rule| rule.exempt_channels.push(picked), cx);
                        })),
                );
            }
            if !role_list.is_empty() {
                // `sectionFooter`.
                list = list.child(
                    div()
                        .flex_none()
                        .h(px(24.))
                        .pt(px(11.))
                        .child(div().h(px(1.)).bg(color("--border-subtle"))),
                );
            }
        }
        if !role_list.is_empty() {
            list = list.child(section("Roles"));
        }
        for role in &role_list {
            let id = role.id.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("automod-pick-role-{id}")))
                    .flex_none()
                    .h(px(26.))
                    .mr(px(8.))
                    .cursor_pointer()
                    .flex()
                    .child(row_label(Self::automod_role_label(role), false))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let picked = id.clone();
                        this.automod_pick(|rule| rule.exempt_roles.push(picked), cx);
                    })),
            );
        }
        Some(
            deferred(
                div()
                    .id("automod-picker")
                    .occlude()
                    .absolute()
                    .left(anchor.left())
                    .top(anchor.bottom())
                    .w(anchor.size.width)
                    .max_h(px(260.))
                    .pt(px(8.))
                    .pb(px(16.))
                    .px(px(12.))
                    .rounded_b(px(5.))
                    .border_1()
                    .border_color(color("--background-base-lowest"))
                    .bg(color("--background-surface-high"))
                    .shadow(vec![
                        gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 1., 0.08),
                            offset: gpui::point(px(0.), px(0.)),
                            blur_radius: px(0.),
                            spread_radius: px(1.),
                            inset: false,
                        },
                        gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.24),
                            offset: gpui::point(px(0.), px(12.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        },
                    ])
                    .overflow_y_scroll()
                    .on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        // A press in the field itself keeps the list open.
                        if !anchor.contains(&event.position)
                            && let Some(state) = this.automod_mut()
                        {
                            state.picker_open = false;
                        }
                        cx.notify();
                    }))
                    .child(list),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `$`: the row becomes a tag and the search starts over.
    fn automod_pick(&mut self, change: impl FnOnce(&mut AutoRule), cx: &mut Context<Self>) {
        self.automod_change(change, cx);
        let query = self
            .automod()
            .and_then(|state| state.fields.as_ref())
            .map(|fields| fields.query.clone());
        if let Some(query) = query {
            query.update(cx, |editor, _| editor.set_text(""));
        }
        if let Some(state) = self.automod_mut() {
            state.picker_open = false;
        }
        cx.notify();
    }

    /// `tR`: the bar under the open rule. It is `position: fixed`, 20 above the window's edge.
    pub(super) fn automod_bar_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if !Self::automod_on_screen(open) {
            return None;
        }
        let state = open.automod.as_ref()?;
        let rule = state.editing.as_ref()?;
        let guild = open.guild;
        let saving = state.saving;
        let message: gpui::Div = match state.error.clone() {
            Some(error) => div()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-feedback-critical"))
                .child(error),
            None => div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--white"))
                .child(if state.has_changes {
                    "Careful \u{2014} you have unsaved changes!".to_owned()
                } else {
                    format!("Currently Editing \"{}\" Rule", rule.name)
                }),
        };
        Some(
            deferred(
                div()
                    .id("automod-bar")
                    .occlude()
                    .absolute()
                    .left(px(412.))
                    .bottom(px(20.))
                    .w(px(660.))
                    .h(px(54.))
                    .pl(px(16.))
                    .pr(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-highest"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.2),
                        offset: gpui::point(px(0.), px(2.)),
                        blur_radius: px(10.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(message),
                    )
                    .child(
                        div()
                            .id("automod-cancel")
                            .flex_none()
                            .w(px(60.))
                            .h(px(32.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-brand"))
                            .child("Cancel")
                            .when(!saving, |this| {
                                this.on_click(cx.listener(|this, _, _, cx| this.automod_cancel(cx)))
                            }),
                    )
                    .child(
                        div()
                            .id("automod-save")
                            .flex_none()
                            .ml(px(10.))
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(color("--control-connected-background-default"))
                            .when(saving, |this| this.opacity(0.5))
                            .when(!saving, |this| this.cursor_pointer())
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(gpui::white())
                            .child("Save Changes")
                            .when(!saving, |this| {
                                this.on_click(cx.listener(move |this, _, _, cx| {
                                    this.automod_save(guild, cx);
                                }))
                            }),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The menu (`345585`) at the click.
    pub(super) fn automod_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if !Self::automod_on_screen(open) {
            return None;
        }
        let state = open.automod.as_ref()?;
        let menu = state.menu.as_ref()?;
        let rule = menu.rule.clone();
        let guild = open.guild;
        let editing = state.editing.is_some();
        let community = self.automod_community(guild);
        let item = |id: &'static str, label: &'static str, disabled: bool| {
            div()
                .id(id)
                .h(px(34.))
                .p(px(8.))
                .rounded(px(2.))
                .when(disabled, |this| this.opacity(0.5))
                .when(!disabled, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(color("--interactive-background-hover")))
                })
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(label)
        };
        let edited = rule.clone();
        let removed = rule.clone();
        let saved = rule.is_saved();
        Some(
            deferred(
                div()
                    .id("automod-menu")
                    .occlude()
                    .absolute()
                    .left(menu.at.x)
                    .top(menu.at.y)
                    .min_w(px(144.))
                    .max_w(px(320.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.24),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.automod_mut() {
                            state.menu = None;
                        }
                        cx.notify();
                    }))
                    .child(
                        item("automod-menu-edit", "Edit Rule", editing).when(!editing, |this| {
                            this.on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(state) = this.automod_mut() {
                                    state.menu = None;
                                }
                                this.automod_set_editing(Some(edited.clone()), cx);
                            }))
                        }),
                    )
                    .when(saved, |this| {
                        this.child(item("automod-menu-delete", "Delete Rule", false).on_click(
                            cx.listener(move |this, _, _, cx| {
                                let confirm = if community && removed.trigger_type == MENTION_SPAM {
                                    Confirm::CannotDelete
                                } else {
                                    Confirm::Remove {
                                        rule: removed.clone(),
                                    }
                                };
                                if let Some(state) = this.automod_mut() {
                                    state.menu = None;
                                    state.confirm = Some(confirm);
                                }
                                cx.notify();
                            }),
                        ))
                    }),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The small confirmations: unsaved changes, removing a rule, and a rule that cannot go.
    pub(super) fn automod_confirm_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let state = open.automod.as_ref()?;
        let confirm = state.confirm.as_ref()?;
        let guild = open.guild;
        let close = |this: &mut Root| {
            if let Some(state) = this.automod_mut() {
                state.confirm = None;
            }
        };
        let spec = match confirm {
            Confirm::Unsaved { name, next } => {
                let next = next.clone();
                super::modal::Spec {
                    id: "automod-unsaved",
                    title: "Unsaved Changes".into(),
                    subtitle: Some(
                        format!(
                            "You have unsaved changes to the \"{name}\" AutoMod rule. Are you sure you want to stop editing without saving?"
                        )
                        .into_any_element(),
                    ),
                    body: None,
                    buttons: vec![
                        super::modal::button(
                            "automod-unsaved-cancel",
                            "Cancel",
                            super::modal::Tone::Secondary,
                            move |this, _, _| close(this),
                        ),
                        super::modal::button(
                            "automod-unsaved-confirm",
                            "Confirm",
                            super::modal::Tone::Critical,
                            move |this, _, cx| {
                                close(this);
                                this.automod_set_editing(Some(next.clone()), cx);
                            },
                        ),
                    ],
                    close: Box::new(move |this, _, _| close(this)),
                }
            }
            Confirm::Remove { rule } => {
                let rule = rule.clone();
                let name = rule.name.clone();
                super::modal::Spec {
                    id: "automod-remove",
                    title: "Remove Rule".into(),
                    subtitle: Some(
                        format!("Are you sure you want to remove the \"{name}\" AutoMod rule?")
                            .into_any_element(),
                    ),
                    body: None,
                    buttons: vec![
                        super::modal::button(
                            "automod-remove-cancel",
                            "Cancel",
                            super::modal::Tone::Secondary,
                            move |this, _, _| close(this),
                        ),
                        super::modal::button(
                            "automod-remove-confirm",
                            "Confirm",
                            super::modal::Tone::Critical,
                            move |this, _, cx| this.automod_delete(guild, &rule, cx),
                        ),
                    ],
                    close: Box::new(move |this, _, _| close(this)),
                }
            }
            Confirm::CannotDelete => super::modal::Spec {
                id: "automod-cannot-delete",
                title: "Cannot delete rule".into(),
                subtitle: Some(
                    "The Mention Spam AutoMod rule cannot be deleted from community servers. Please disable the rule instead."
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![super::modal::button(
                    "automod-cannot-delete-ok",
                    "Okay",
                    super::modal::Tone::Primary,
                    move |this, _, _| close(this),
                )],
                close: Box::new(move |this, _, _| close(this)),
            },
        };
        Some(super::modal::render_small(spec, cx))
    }

    /// A dialog's closed select (`890497`).
    fn automod_select(
        &self,
        shown: Option<String>,
        placeholder: &'static str,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("automod-dialog-select")
            .relative()
            .child(self.anchor_probe("automod-dialog-select"))
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
                    .text_color(if shown.is_some() {
                        color("--text-default")
                    } else {
                        color("--input-placeholder-text-default")
                    })
                    .child(shown.unwrap_or_else(|| placeholder.to_owned())),
            )
            .child(
                gpui::svg()
                    .path("icons/select_chevron.svg")
                    .size(px(24.))
                    .text_color(color("--interactive-text-default"))
                    .with_transformation(gpui::Transformation::rotate(gpui::percentage(if open {
                        0.5
                    } else {
                        0.
                    }))),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(Dialog::Alert { open, .. } | Dialog::Timeout { open, .. }) =
                    this.automod_mut().and_then(|state| state.dialog.as_mut())
                {
                    *open = !*open;
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// `C` of module 158956: the channels an alert can go to.
    fn automod_alert_channels(&self, guild: Id) -> Vec<(String, String)> {
        let Some(known) = self.stores.guild(guild) else {
            return Vec::new();
        };
        let me = self.stores.me();
        let mut list: Vec<&Channel> = known
            .channels
            .iter()
            .filter(|channel| matches!(channel.kind, ChannelKind::Text | ChannelKind::Other(5)))
            .filter(|channel| {
                me.is_some_and(|me| {
                    permissions::can_in_channel(known, channel, me, permissions::VIEW_CHANNEL)
                })
            })
            .collect();
        list.sort_by_key(|channel| (channel.position, channel.id));
        list.into_iter()
            .map(|channel| (channel.id.to_string(), format!("#{}", channel.name)))
            .collect()
    }

    /// The custom message, alert channel and timeout dialogs.
    pub(super) fn automod_dialog_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let state = open.automod.as_ref()?;
        let dialog = state.dialog.as_ref()?;
        let guild = open.guild;
        let close = |this: &mut Root| this.automod_close_dialog();
        let (id, title, subtitle, body, confirm): (
            &'static str,
            &'static str,
            &'static str,
            AnyElement,
            &'static str,
        ) = match dialog {
            Dialog::Message { editor, .. } => {
                let focus = editor.clone();
                (
                    "automod-message",
                    "Edit Custom Message",
                    "Your custom message will be shown when AutoMod blocks a member's message. This is a chance to help members understand your server's guidelines!",
                    div()
                        .id("automod-message-field")
                        .min_h(px(88.))
                        .pt(px(12.))
                        .px(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .cursor_text()
                        .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                            let handle = focus.read(cx).focus_handle(cx);
                            window.focus(&handle, cx);
                        }))
                        .child(div().w_full().child(editor.clone()))
                        .into_any_element(),
                    "Edit",
                )
            }
            Dialog::Alert {
                is_edit,
                channel,
                error,
                open,
            } => {
                let shown = channel.as_ref().and_then(|chosen| {
                    self.automod_alert_channels(guild)
                        .into_iter()
                        .find(|(id, _)| id == chosen)
                        .map(|(_, name)| name)
                });
                let note = if *error {
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-feedback-critical"))
                        .child("Please select a channel.")
                } else {
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child("Anyone with access to this text channel will see the alert.")
                };
                (
                    "automod-alert",
                    "Send alert",
                    "Choose a channel to post the alert message to. The alert will contain the flagged message contents, author, and channel it was originally posted to.",
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(self.automod_select(shown, "Select a text channel", *open, cx))
                        .child(note)
                        .into_any_element(),
                    if *is_edit { "Edit" } else { "Save" },
                )
            }
            Dialog::Timeout {
                is_edit,
                seconds,
                open,
            } => (
                "automod-timeout",
                "Timeout member",
                "Choose how long you want to timeout a member if they break this rule. Members in timeout are temporarily not allowed to chat, react in text channels, or connect to voice or Stage channels.",
                self.automod_select(
                    duration_label(*seconds).map(str::to_owned),
                    "Choose duration",
                    *open,
                    cx,
                ),
                if *is_edit { "Edit" } else { "Save" },
            ),
        };
        Some(super::modal::render(
            super::modal::Spec {
                id,
                title: title.into(),
                subtitle: Some(subtitle.into_any_element()),
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "automod-dialog-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "automod-dialog-confirm",
                        confirm,
                        super::modal::Tone::Primary,
                        |this, _, cx| this.automod_submit_dialog(cx),
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }

    /// The open list of a dialog's select (`maxOptionsVisible: 5`).
    pub(super) fn automod_select_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let state = open.automod.as_ref()?;
        let guild = open.guild;
        let (options, current): (Vec<(String, String)>, Option<String>) =
            match state.dialog.as_ref()? {
                Dialog::Alert {
                    open: true,
                    channel,
                    ..
                } => (self.automod_alert_channels(guild), channel.clone()),
                Dialog::Timeout {
                    open: true,
                    seconds,
                    ..
                } => (
                    DURATIONS
                        .iter()
                        .map(|(value, label)| (value.to_string(), (*label).to_owned()))
                        .collect(),
                    Some(seconds.to_string()),
                ),
                _ => return None,
            };
        let field = self
            .tip_anchors
            .borrow()
            .get("automod-dialog-select")
            .copied()?
            .dilate(px(1.));
        let rows: Vec<AnyElement> = options
            .into_iter()
            .map(|(value, label)| {
                let selected = current.as_deref() == Some(value.as_str());
                div()
                    .id(SharedString::from(format!("automod-option-{value}")))
                    .flex_none()
                    .h(px(40.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(label),
                    )
                    .when(selected, |this| {
                        this.child(icon("select_check", 20., "--text-brand"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match this.automod_mut().and_then(|state| state.dialog.as_mut()) {
                            Some(Dialog::Alert {
                                channel,
                                open,
                                error,
                                ..
                            }) => {
                                *channel = Some(value.clone());
                                *open = false;
                                *error = false;
                            }
                            Some(Dialog::Timeout { seconds, open, .. }) => {
                                *seconds = value.parse().unwrap_or(60);
                                *open = false;
                            }
                            _ => {}
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        Some(
            deferred(
                div()
                    .id("automod-select-layer")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(field.bottom() + px(4.))
                    .w(field.size.width)
                    .max_h(px(210.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .overflow_y_scroll()
                    .on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        if !field.contains(&event.position)
                            && let Some(Dialog::Alert { open, .. } | Dialog::Timeout { open, .. }) =
                                this.automod_mut().and_then(|state| state.dialog.as_mut())
                        {
                            *open = false;
                        }
                        cx.notify();
                    }))
                    .child(div().flex().flex_col().children(rows)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(kind: u64) -> AutoRule {
        AutoRule::from_wire(&json!({
            "id": "1568692564637524999",
            "name": "x",
            "guild_id": "1568692564637524109",
            "event_type": trigger(kind).event,
            "trigger_type": kind,
            "trigger_metadata": default_metadata(kind, false),
            "actions": [{"type": 1, "metadata": {}}],
            "enabled": true,
            "creator_id": "1",
            "position": 0,
            "exempt_channels": [],
            "exempt_roles": [],
        }))
    }

    #[test]
    fn a_new_rule_takes_the_tables_defaults_and_counts_its_kind() {
        let rule = AutoRule::new("7", KEYWORD, "9", 0, false);
        assert_eq!(rule.id, "7-1-new-rule");
        assert_eq!(rule.name, "Block Custom Words");
        assert_eq!(rule.event_type, MESSAGE_SEND);
        assert_eq!(rule.actions, vec![json!({"type": 1, "metadata": {}})]);
        assert!(rule.enabled && !rule.is_saved());
        assert_eq!(
            AutoRule::new("7", KEYWORD, "9", 2, false).name,
            "Block Custom Words 3"
        );
        let mention = AutoRule::new("7", MENTION_SPAM, "9", 0, true);
        assert_eq!(mention.trigger_metadata["mention_total_limit"], 20);
        assert_eq!(
            mention.trigger_metadata["mention_raid_protection_enabled"],
            true
        );
        assert!(
            AutoRule::new("7", ML_SPAM, "9", 0, false)
                .trigger_metadata
                .is_null()
        );
        let profile = AutoRule::new("7", USER_PROFILE, "9", 0, true);
        assert_eq!(profile.event_type, MEMBER_JOIN_OR_UPDATE);
        assert_eq!(action_kind(&profile.actions[0]), QUARANTINE_USER);
    }

    #[test]
    fn the_body_matches_m_and_a_new_rule_has_no_id() {
        let mut rule = AutoRule::new("7", ML_SPAM, "9", 0, false);
        rule.exempt_roles = vec!["1".into(), "2".into()];
        let body = rule.create_body(&|role| role == "1");
        assert!(body.get("id").is_none());
        assert!(body.get("trigger_metadata").is_none());
        assert_eq!(body["exempt_roles"], json!(["1"]));
        assert_eq!(body["trigger_type"], ML_SPAM);
        assert_eq!(body["event_type"], MESSAGE_SEND);
        assert_eq!(body["actions"], json!([{"type": 1, "metadata": {}}]));
        let update = saved(KEYWORD).body(&|_| true);
        assert_eq!(update["id"], "1568692564637524999");
        assert_eq!(update["trigger_metadata"]["keyword_filter"], json!([]));
    }

    #[test]
    fn reading_drops_keyword_lists_and_keeps_unknown_metadata() {
        let rule = AutoRule::from_wire(&json!({
            "id": "1568692564637524999",
            "trigger_type": 1,
            "trigger_metadata": {"keyword_lists": [1], "keyword_filter": ["a"], "other": 5},
            "exempt_channels": ["3"],
        }));
        assert!(rule.is_saved());
        assert!(rule.trigger_metadata.get("keyword_lists").is_none());
        assert_eq!(rule.trigger_metadata["other"], 5);
        assert_eq!(rule.exempt_channels, ["3"]);
        assert_eq!(rule.list("keyword_filter"), ["a"]);
    }

    #[test]
    fn only_a_seventeen_to_nineteen_digit_id_counts_as_saved() {
        let mut rule = saved(KEYWORD);
        assert!(rule.is_saved());
        rule.id = "7-1-new-rule".into();
        assert!(!rule.is_saved());
        rule.id = "1234".into();
        assert!(!rule.is_saved());
    }

    #[test]
    fn words_split_like_c_and_join_like_mv() {
        assert_eq!(
            parse_words(" dog, cat  fish\n\ntiger\t,"),
            ["dog", "cat fish", "tiger"]
        );
        assert_eq!(join_words(&["a".into(), "b".into()]), "a, b");
        assert_eq!(normalized_paste("a,b\nc"), Some("a, b, c".to_owned()));
        assert_eq!(normalized_paste("plain"), None);
        assert_eq!(normalized_paste(""), None);
    }

    #[test]
    fn saving_tidies_word_lists_without_touching_patterns() {
        let mut rule = saved(KEYWORD);
        rule.set_list("keyword_filter", vec!["b".into(), "*a".into(), "b".into()]);
        rule.set_list("allow_list", vec!["z".into(), "Y".into()]);
        rule.set_list("regex_patterns", vec!["z".into(), "a".into()]);
        rule.tidy();
        assert_eq!(rule.list("keyword_filter"), ["*a", "b"]);
        assert_eq!(rule.list("allow_list"), ["Y", "z"]);
        assert_eq!(rule.list("regex_patterns"), ["z", "a"]);
    }

    #[test]
    fn validation_says_what_uv_says() {
        let mut keyword = saved(KEYWORD);
        assert_eq!(
            check_rule(&keyword, &[]),
            Err("Please enter at least one keyword or one regex pattern.".into())
        );
        keyword.set_list("keyword_filter", vec!["x".repeat(61)]);
        assert_eq!(check_rule(&keyword, &[]), Err(KEYWORD_ERRORS.into()));
        keyword.set_list("keyword_filter", vec!["ok".into(); 1001]);
        assert_eq!(
            check_rule(&keyword, &[]),
            Err("Please enter no more than 1000 keywords.".into())
        );
        keyword.set_list("keyword_filter", vec![]);
        keyword.set_list("regex_patterns", vec!["a".into(); 11]);
        assert_eq!(
            check_rule(&keyword, &[]),
            Err("Please enter no more than 10 regex patterns.".into())
        );
        keyword.set_list("regex_patterns", vec!["x".repeat(261)]);
        assert_eq!(check_rule(&keyword, &[]), Err(REGEX_ERRORS.into()));
        keyword.set_list("regex_patterns", vec!["ok".into()]);
        assert_eq!(check_rule(&keyword, &[]), Ok(()));
        keyword.actions.clear();
        assert_eq!(
            check_rule(&keyword, &[]),
            Err("Please add at least one action to this rule.".into())
        );

        let mut mention = saved(MENTION_SPAM);
        assert_eq!(check_rule(&mention, &[]), Ok(()));
        mention.set_meta("mention_total_limit", json!(51));
        assert_eq!(
            check_rule(&mention, &[]),
            Err("Enter a number between 1 and 50.".into())
        );

        let spam = saved(ML_SPAM);
        assert_eq!(check_rule(&spam, &[]), Ok(()));
        let mut wrong = saved(ML_SPAM);
        wrong.actions = vec![default_action(TIMEOUT)];
        assert!(check_rule(&wrong, &[]).is_err());
    }

    #[test]
    fn the_rule_count_limit_ignores_the_rule_itself() {
        let rule = saved(MENTION_SPAM);
        let mut other = saved(MENTION_SPAM);
        other.id = "1568692564637524111".into();
        let mut another = other.clone();
        another.id = "1568692564637524112".into();
        assert_eq!(check_rule(&rule, std::slice::from_ref(&other)), Ok(()));
        assert!(check_rule(&rule, &[other, another]).is_err());
    }

    #[test]
    fn keyword_problems_read_like_wk() {
        assert_eq!(keywords_problem(&["a".into()], 1000), None);
        assert_eq!(
            keywords_problem(&["a".into(), "b".into()], 1),
            Some("Please enter no more than 1 keywords.".to_owned())
        );
        let long = "x".repeat(61);
        assert_eq!(
            keywords_problem(std::slice::from_ref(&long), 100),
            Some(format!(
                "{long} has an invalid length. Keywords must be between 1 and 60 characters."
            ))
        );
    }

    #[test]
    fn errors_come_out_of_the_tree_like_get_any_error_message() {
        let body = json!({
            "code": 50035,
            "message": "Invalid Form Body",
            "errors": {"name": {"_errors": [{"code": "X", "message": "Too long"}]}},
        });
        assert_eq!(any_error_message(&body), "Too long");
        assert_eq!(save_error(&body), "Too long");
        assert_eq!(
            any_error_message(&json!({"code": 30001, "message": "Limit"})),
            "Limit"
        );
        assert_eq!(any_error_message(&json!({"name": ["Bad"]})), "Bad");
        assert_eq!(any_error_message(&Value::Null), GENERIC_ERROR);
        let regex = json!({
            "code": 50035,
            "errors": {"trigger_metadata": {"regex_patterns": {"_errors": [{"message": "m"}]}}},
        });
        assert_eq!(save_error(&regex), REGEX_ERRORS);
    }

    #[test]
    fn regex_errors_read_each_pattern_and_the_whole_list() {
        let body = json!({"errors": {"trigger_metadata": {"regex_patterns": {
            "a(": {
                "_errors": [{"code": "C", "message": "Bad"}],
                "description": "Unclosed",
                "erroring_character_offset": 1,
                "erroring_character_length": 1,
            },
            "b": {"_errors": [{"code": "C", "message": "Worse"}]},
            "_errors": [{"message": "Too many"}],
        }}}});
        let list = regex_errors(&body).expect("patterns");
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].pattern, "a(");
        assert_eq!(list[0].message, "Unclosed");
        assert_eq!((list[0].offset, list[0].length), (Some(1), Some(1)));
        assert_eq!(list[1].message, "Worse");
        assert_eq!(list[2].pattern, "_errors");
        assert_eq!(regex_errors(&json!({"errors": {"name": {}}})), None);
    }

    #[test]
    fn a_paste_of_lines_becomes_patterns() {
        assert_eq!(split_patterns(" a \n\nb\tc"), ["a", "b", "c"]);
        assert_eq!(split_patterns("one"), ["one"]);
    }

    #[test]
    fn the_stepper_clamps_and_keeps_an_empty_field() {
        assert_eq!(stepper_input(""), Some((String::new(), 1)));
        assert_eq!(stepper_input("-"), Some(("-".to_owned(), 1)));
        assert_eq!(stepper_input("7"), Some(("7".to_owned(), 7)));
        assert_eq!(stepper_input("7a"), Some(("7".to_owned(), 7)));
        assert_eq!(stepper_input("99"), Some(("50".to_owned(), 50)));
        assert_eq!(stepper_input("-4"), Some(("1".to_owned(), 1)));
        assert_eq!(stepper_input("x"), None);
    }

    #[test]
    fn actions_keep_the_table_order_when_shown() {
        let mut rule = saved(KEYWORD);
        rule.with_action(TIMEOUT, Some(default_action(TIMEOUT)));
        rule.with_action(
            FLAG_TO_CHANNEL,
            Some(json!({"type": 2, "metadata": {"channel_id": "5"}})),
        );
        let kinds: Vec<u64> = rule
            .shown_actions()
            .iter()
            .map(|a| action_kind(a))
            .collect();
        assert_eq!(kinds, [BLOCK_MESSAGE, FLAG_TO_CHANNEL, TIMEOUT]);
        rule.with_action(TIMEOUT, None);
        assert!(rule.action(TIMEOUT).is_none());
        assert_eq!(rule.actions.len(), 2);
    }

    #[test]
    fn sections_follow_community() {
        assert_eq!(sections(false).len(), 1);
        assert_eq!(
            sections(false)[0].1,
            [MENTION_SPAM, ML_SPAM, DEFAULT_KEYWORD_LIST, KEYWORD]
        );
        assert_eq!(sections(true)[0], ("Members", vec![USER_PROFILE]));
    }

    #[test]
    fn subtext_counts_words_and_regexes_in_saved_custom_rules() {
        let mut rule = saved(KEYWORD);
        rule.set_list("keyword_filter", vec!["a".into()]);
        assert_eq!(rule_subtext(&rule), "(1 word)");
        rule.set_list("keyword_filter", vec!["a".into(), "b".into()]);
        rule.set_list("regex_patterns", vec!["a".into()]);
        assert_eq!(rule_subtext(&rule), "(2 words, 1 regex)");
        rule.set_list("regex_patterns", vec!["a".into(), "b".into()]);
        assert_eq!(rule_subtext(&rule), "(2 words, 2 regexes)");
        assert_eq!(rule_subtext(&saved(ML_SPAM)), "");
    }

    fn channel(
        id: Id,
        name: &str,
        kind: ChannelKind,
        position: i64,
        parent: Option<Id>,
    ) -> Channel {
        Channel {
            id,
            name: name.to_owned(),
            kind,
            position,
            parent_id: parent,
            last_message_id: None,
            overwrites: Vec::new(),
            topic: None,
            rate_limit_per_user: 0,
            flags: 0,
            default_auto_archive_duration: None,
            available_tags: Vec::new(),
            default_sort_order: None,
            default_forum_layout: 0,
        }
    }

    #[test]
    fn channel_rows_nest_under_categories_and_hide_what_is_picked() {
        let list = vec![
            channel(1, "Text Channels", ChannelKind::Category, 0, None),
            channel(2, "Voice Channels", ChannelKind::Category, 0, None),
            channel(3, "general", ChannelKind::Text, 0, Some(1)),
            channel(4, "General", ChannelKind::Voice, 0, Some(2)),
            channel(5, "loose", ChannelKind::Text, 3, None),
            channel(6, "stage", ChannelKind::Other(13), 0, None),
        ];
        let names = |rows: Vec<ChannelRow>| -> Vec<String> {
            rows.into_iter().map(|row| row.name).collect()
        };
        assert_eq!(
            names(channel_rows(&list, &[])),
            [
                "loose",
                "Text Channels",
                "general",
                "Voice Channels",
                "General"
            ]
        );
        assert_eq!(
            names(channel_rows(&list, &["3".to_owned()])),
            ["loose", "Text Channels", "Voice Channels", "General"]
        );
        assert_eq!(
            names(channel_rows(&list, &["1".to_owned()])),
            ["loose", "Voice Channels", "General"]
        );
        assert!(channel_rows(&list, &[])[2].nested);
    }

    #[test]
    fn roles_and_search_read_like_the_list() {
        assert_eq!(role_display("mods"), "@mods");
        assert_eq!(role_display("@here"), "@here");
        assert!(fuzzy_match("gnl", "general"));
        assert!(!fuzzy_match("lg", "general"));
    }

    #[test]
    fn resizing_stops_at_each_boxs_lowest_height() {
        assert_eq!(resized_height(Resized::Regex, 100., -50.), 100.);
        assert_eq!(resized_height(Resized::Regex, 100., 30.), 130.);
        assert_eq!(resized_height(Resized::Keywords, 78., -60.), 40.);
        assert_eq!(resized_height(Resized::Allow, 78., 2.), 80.);
    }

    #[test]
    fn durations_have_their_labels() {
        assert_eq!(duration_label(60), Some("60 secs"));
        assert_eq!(duration_label(604800), Some("1 week"));
        assert_eq!(duration_label(61), None);
    }
}
