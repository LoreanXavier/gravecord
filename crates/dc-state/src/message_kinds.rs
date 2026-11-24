//! What each message type draws. Port of the message type enum (module 448761), the system message table `ls` in
//! module 117096 and the thread starter component `n$` in module 891522. Each line is the catalog message its handler
//! formats (`intl.format(key, values)`), in the interface locale.
//!
//! The official client sends every type that is not a user message (`USER_MESSAGE` in 448761) to the system message
//! table. A type whose table entry is `void 0`, or that is missing from it, draws nothing at all, and a handler
//! that returns `null` draws nothing either. Types in `USER_MESSAGE` use the ordinary message layout. Type 21 is
//! the thread starter: the message it references is drawn in its place.

use crate::{Id, Message, User, id, text};
use chrono::{DateTime, Utc};
use serde_json::Value;

pub const DEFAULT: u64 = 0;
pub const RECIPIENT_ADD: u64 = 1;
pub const RECIPIENT_REMOVE: u64 = 2;
pub const CALL: u64 = 3;
pub const CHANNEL_NAME_CHANGE: u64 = 4;
pub const CHANNEL_ICON_CHANGE: u64 = 5;
pub const CHANNEL_PINNED_MESSAGE: u64 = 6;
pub const USER_JOIN: u64 = 7;
pub const GUILD_BOOST: u64 = 8;
pub const GUILD_BOOST_TIER_1: u64 = 9;
pub const GUILD_BOOST_TIER_2: u64 = 10;
pub const GUILD_BOOST_TIER_3: u64 = 11;
pub const CHANNEL_FOLLOW_ADD: u64 = 12;
pub const GUILD_STREAM: u64 = 13;
pub const GUILD_DISCOVERY_DISQUALIFIED: u64 = 14;
pub const GUILD_DISCOVERY_REQUALIFIED: u64 = 15;
pub const GUILD_DISCOVERY_GRACE_PERIOD_INITIAL_WARNING: u64 = 16;
pub const GUILD_DISCOVERY_GRACE_PERIOD_FINAL_WARNING: u64 = 17;
pub const THREAD_CREATED: u64 = 18;
pub const REPLY: u64 = 19;
pub const CHAT_INPUT_COMMAND: u64 = 20;
pub const THREAD_STARTER_MESSAGE: u64 = 21;
pub const GUILD_INVITE_REMINDER: u64 = 22;
pub const CONTEXT_MENU_COMMAND: u64 = 23;
pub const AUTO_MODERATION_ACTION: u64 = 24;
pub const ROLE_SUBSCRIPTION_PURCHASE: u64 = 25;
pub const INTERACTION_PREMIUM_UPSELL: u64 = 26;
pub const STAGE_START: u64 = 27;
pub const STAGE_END: u64 = 28;
pub const STAGE_SPEAKER: u64 = 29;
pub const STAGE_RAISE_HAND: u64 = 30;
pub const STAGE_TOPIC: u64 = 31;
pub const GUILD_APPLICATION_PREMIUM_SUBSCRIPTION: u64 = 32;
pub const PRIVATE_CHANNEL_INTEGRATION_ADDED: u64 = 33;
pub const PRIVATE_CHANNEL_INTEGRATION_REMOVED: u64 = 34;
pub const PREMIUM_REFERRAL: u64 = 35;
pub const GUILD_INCIDENT_ALERT_MODE_ENABLED: u64 = 36;
pub const GUILD_INCIDENT_ALERT_MODE_DISABLED: u64 = 37;
pub const GUILD_INCIDENT_REPORT_RAID: u64 = 38;
pub const GUILD_INCIDENT_REPORT_FALSE_ALARM: u64 = 39;
pub const GUILD_DEADCHAT_REVIVE_PROMPT: u64 = 40;
pub const CUSTOM_GIFT: u64 = 41;
pub const GUILD_GAMING_STATS_PROMPT: u64 = 42;
pub const PURCHASE_NOTIFICATION: u64 = 44;
pub const VOICE_HANGOUT_INVITE: u64 = 45;
pub const POLL_RESULT: u64 = 46;
pub const CHANGELOG: u64 = 47;
pub const NITRO_NOTIFICATION: u64 = 48;
pub const CHANNEL_LINKED_TO_LOBBY: u64 = 49;
pub const GIFTING_PROMPT: u64 = 50;
pub const IN_GAME_MESSAGE_NUX: u64 = 51;
pub const GUILD_JOIN_REQUEST_ACCEPT_NOTIFICATION: u64 = 52;
pub const GUILD_JOIN_REQUEST_REJECT_NOTIFICATION: u64 = 53;
pub const GUILD_JOIN_REQUEST_WITHDRAWN_NOTIFICATION: u64 = 54;
pub const HD_STREAMING_UPGRADED: u64 = 55;
pub const CHAT_WALLPAPER_SET: u64 = 56;
pub const CHAT_WALLPAPER_REMOVED: u64 = 57;
pub const REPORT_TO_MOD_DELETED_MESSAGE: u64 = 58;
pub const REPORT_TO_MOD_TIMEOUT_USER: u64 = 59;
pub const REPORT_TO_MOD_KICK_USER: u64 = 60;
pub const REPORT_TO_MOD_BAN_USER: u64 = 61;
pub const REPORT_TO_MOD_CLOSED_REPORT: u64 = 62;
pub const EMOJI_ADDED: u64 = 63;
pub const PREMIUM_GROUP_INVITE: u64 = 64;
pub const VOICE_SESSION: u64 = 65;
pub const FRIEND_REQUEST_ACCEPTED: u64 = 67;
pub const MEDIA_MENTION_MESSAGE: u64 = 68;
pub const GUILD_SPACE_MESSAGE: u64 = 69;

/// `USER_MESSAGE` in module 448761: the types drawn with the ordinary message layout.
pub const USER_MESSAGE: [u64; 9] = [
    DEFAULT,
    REPLY,
    CHAT_INPUT_COMMAND,
    CONTEXT_MENU_COMMAND,
    INTERACTION_PREMIUM_UPSELL,
    CUSTOM_GIFT,
    VOICE_HANGOUT_INVITE,
    CHANGELOG,
    MEDIA_MENTION_MESSAGE,
];

/// Module 143413: a message is a system message when its type is not a user message.
pub fn is_system(kind: u64) -> bool {
    !USER_MESSAGE.contains(&kind)
}

/// What a message turns into on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    /// The ordinary layout: author, time, content, files, embeds.
    Normal,
    /// THREAD_STARTER_MESSAGE: the referenced message in the ordinary layout, or the "could not load" line.
    ThreadStarter,
    /// One line with an icon in the gutter.
    System,
    /// Nothing is drawn: the official table has no entry, or its entry returns `null`.
    Nothing,
}

/// The system table's entry for a type: whether it draws anything for this message.
pub fn presentation(message: &Message) -> Presentation {
    let kind = message.kind;
    if USER_MESSAGE.contains(&kind) {
        return Presentation::Normal;
    }
    match kind {
        THREAD_STARTER_MESSAGE => Presentation::ThreadStarter,
        // Table entries that are `void 0`, and types the table does not list.
        GUILD_INVITE_REMINDER
        | GUILD_DEADCHAT_REVIVE_PROMPT
        | GUILD_GAMING_STATS_PROMPT
        | HD_STREAMING_UPGRADED
        | CHAT_WALLPAPER_SET
        | CHAT_WALLPAPER_REMOVED
        | EMOJI_ADDED => Presentation::Nothing,
        // `tG` draws the report-to-mod notices only for the moderators who can see them; the app has no such view.
        REPORT_TO_MOD_DELETED_MESSAGE
        | REPORT_TO_MOD_TIMEOUT_USER
        | REPORT_TO_MOD_KICK_USER
        | REPORT_TO_MOD_BAN_USER
        | REPORT_TO_MOD_CLOSED_REPORT => Presentation::Nothing,
        // `I.ER`: AutoMod's own card needs its notification embed; without one the handler returns `null`.
        AUTO_MODERATION_ACTION => {
            if message
                .embeds
                .iter()
                .any(|embed| embed.kind == "auto_moderation_notification")
            {
                Presentation::Normal
            } else {
                Presentation::Nothing
            }
        }
        // `eB` needs the result fields of the first embed.
        POLL_RESULT => {
            if message.embeds.first().is_some_and(|embed| {
                embed
                    .fields
                    .iter()
                    .any(|field| field.name == "poll_question_text")
            }) {
                Presentation::System
            } else {
                Presentation::Nothing
            }
        }
        // Handlers that need data the model does not carry (gifting prompt, guild space data, referral trial offer,
        // lobby application) return `null` without it.
        GIFTING_PROMPT | GUILD_SPACE_MESSAGE | PREMIUM_REFERRAL | NITRO_NOTIFICATION
        | PREMIUM_GROUP_INVITE => Presentation::Nothing,
        RECIPIENT_ADD
        | RECIPIENT_REMOVE
        | CALL
        | CHANNEL_NAME_CHANGE
        | CHANNEL_ICON_CHANGE
        | CHANNEL_PINNED_MESSAGE
        | USER_JOIN
        | GUILD_BOOST..=GUILD_BOOST_TIER_3
        | CHANNEL_FOLLOW_ADD
        | GUILD_STREAM
        | GUILD_DISCOVERY_DISQUALIFIED..=GUILD_DISCOVERY_GRACE_PERIOD_FINAL_WARNING
        | THREAD_CREATED
        | ROLE_SUBSCRIPTION_PURCHASE
        | STAGE_START..=STAGE_TOPIC
        | GUILD_APPLICATION_PREMIUM_SUBSCRIPTION
        | PRIVATE_CHANNEL_INTEGRATION_ADDED
        | PRIVATE_CHANNEL_INTEGRATION_REMOVED
        | GUILD_INCIDENT_ALERT_MODE_ENABLED..=GUILD_INCIDENT_REPORT_FALSE_ALARM
        | PURCHASE_NOTIFICATION
        | CHANNEL_LINKED_TO_LOBBY
        | IN_GAME_MESSAGE_NUX
        | GUILD_JOIN_REQUEST_ACCEPT_NOTIFICATION..=GUILD_JOIN_REQUEST_WITHDRAWN_NOTIFICATION
        | VOICE_SESSION
        | FRIEND_REQUEST_ACCEPTED => {
            // Four types have their own line in the app; the others draw when their handler has what it needs.
            let own = matches!(
                kind,
                CHANNEL_PINNED_MESSAGE | USER_JOIN | THREAD_CREATED | FRIEND_REQUEST_ACCEPTED
            );
            if own || system_line(message, &LineContext::default()).is_some() {
                Presentation::System
            } else {
                Presentation::Nothing
            }
        }
        // `unknown message type`: the table has no entry.
        _ => Presentation::Nothing,
    }
}

/// A call's participants and length (`message.call`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallInfo {
    pub participants: Vec<Id>,
    pub ended: Option<DateTime<Utc>>,
}

/// `role_subscription_data`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoleSubscription {
    pub tier_name: String,
    pub total_months: u64,
    pub renewal: bool,
}

/// The parts of a message that only some types carry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MessageExtra {
    /// `mentions`: the users named in the message (the target of a group add or remove).
    pub mention_users: Vec<User>,
    pub application: Option<(Id, String)>,
    pub call: Option<CallInfo>,
    pub purchase_product: Option<String>,
    pub role_subscription: Option<RoleSubscription>,
    /// `interaction`: the command a reply answers, as (name, who ran it).
    pub interaction: Option<(String, User)>,
    /// `interaction_metadata.ephemerality_reason`: why an app's answer reached only this account (module 376767).
    pub ephemerality_reason: Option<u64>,
    /// `poll`: the question text and the answers, as (text, votes).
    pub poll: Option<Poll>,
    /// `activity.type`: an activity invite carried by the message.
    pub activity: Option<u64>,
    /// Whether `components` is not empty.
    pub has_components: bool,
    /// `components`: the controls and layout parts under the message (`components` module).
    pub components: Vec<super::components::Component>,
    /// `referenced_message` was sent as `null`: the message this one points at was deleted.
    pub reference_deleted: bool,
}

/// `poll`: what a poll message shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Poll {
    pub question: String,
    pub answers: Vec<String>,
    pub expiry: Option<DateTime<Utc>>,
    pub multiselect: bool,
    pub finalized: bool,
    /// Votes per answer, when the results are already in.
    pub counts: Vec<u64>,
    /// `answers[].answer_id`, in the order of `answers`.
    pub answer_ids: Vec<u64>,
    /// `answers[].poll_media.emoji`.
    pub emoji: Vec<Option<PollEmoji>>,
    /// `answers[].poll_media.attachment_ids`: the pictures of an image-only answer.
    pub attachments: Vec<Vec<Id>>,
    /// `results.answer_counts[].me_voted`: whether this account picked the answer.
    pub me_voted: Vec<bool>,
    /// `layout_type`: 1 default, 2 image-only answers (module 744593).
    pub layout: u64,
    /// The people known to have picked each answer, by answer id: what a vote event or a loaded voters list told
    /// this session (the VOTE reactions of `ReactionStore`, which the vote-count tooltip reads).
    pub voters: std::collections::HashMap<u64, Vec<Id>>,
}

/// `poll_media.emoji`: a custom emoji by id, or a unicode one by its characters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PollEmoji {
    pub id: Option<Id>,
    pub name: String,
    pub animated: bool,
}

impl MessageExtra {
    pub fn from_value(value: &Value) -> Self {
        let parse_time = |time: &Value| {
            time.as_str()
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc))
        };
        Self {
            mention_users: value["mentions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(User::from_value)
                .collect(),
            application: id(&value["application"]["id"]).zip(text(&value["application"]["name"])),
            call: value["call"].is_object().then(|| CallInfo {
                participants: value["call"]["participants"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(id)
                    .collect(),
                ended: parse_time(&value["call"]["ended_timestamp"]),
            }),
            purchase_product: text(
                &value["purchase_notification"]["guild_product_purchase"]["product_name"],
            ),
            role_subscription: value["role_subscription_data"].is_object().then(|| {
                RoleSubscription {
                    tier_name: text(&value["role_subscription_data"]["tier_name"])
                        .unwrap_or_default(),
                    total_months: value["role_subscription_data"]["total_months_subscribed"]
                        .as_u64()
                        .unwrap_or(0),
                    renewal: value["role_subscription_data"]["is_renewal"]
                        .as_bool()
                        .unwrap_or(false),
                }
            }),
            interaction: text(&value["interaction"]["name"])
                .zip(User::from_value(&value["interaction"]["user"])),
            ephemerality_reason: value["interaction_metadata"]["ephemerality_reason"].as_u64(),
            poll: value["poll"]
                .is_object()
                .then(|| Poll::from_value(&value["poll"])),
            activity: value["activity"]["type"].as_u64(),
            reference_deleted: value.get("referenced_message").is_some_and(Value::is_null),
            has_components: value["components"]
                .as_array()
                .is_some_and(|components| !components.is_empty()),
            components: super::components::parse(&value["components"]),
        }
    }
}

/// The icon in front of a system line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineIcon {
    /// `PencilIcon` xs.
    Pencil,
    /// `PinIcon` sm.
    Pin,
    /// `ThreadIcon`.
    Thread,
    /// The green arrow image (module 617184): somebody came in.
    ArrowIn,
    /// The red arrow image (module 884797): somebody went out.
    ArrowOut,
    /// `StageIcon` md, green when a stage starts.
    StageStart,
    Stage,
    /// `HandRequestSpeakIcon`.
    Hand,
    /// `MicrophoneArrowRightIcon`.
    SpeakerArrow,
    /// `TicketIcon`.
    Ticket,
    /// `RefreshIcon` sm in the positive colour.
    Refresh,
    /// `VoiceNormalIcon` md in the positive colour.
    Voice,
    /// Module 175476: a call that was joined.
    Call,
    /// Module 130158: a call that was missed.
    CallMissed,
    /// Module 290388: a warning for discovery.
    Warning,
    /// Module 222548.
    Discovery,
    /// The server boost icon of `sY`: the boost badge.
    Boost,
    /// `GameControllerIcon` sm.
    GameController,
    /// `PollsIcon` xs.
    Polls,
}

/// A piece of a system line.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    /// A user's name: a link to their profile.
    User(Box<User>),
    /// A name with no profile behind it (an application, a webhook).
    Name(String),
    /// Medium weight, strong colour.
    Bold(String),
    /// `Anchor`: a link in the content colour.
    Link(String),
}

/// The trailing control of a line, drawn by the view: `T.A.Action` "Join the call" (`oa9mvZ`) after an active call's
/// line, or the "Edit Group" link (`5Q9+/L`, the `editGroupButton` of the group name and icon lines).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineAction {
    JoinCall,
    EditGroup,
}

/// A system message's line.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub icon: LineIcon,
    pub parts: Vec<Part>,
    pub action: Option<LineAction>,
}

/// What the line needs to know about where the message sits.
#[derive(Clone, Debug, Default)]
pub struct LineContext {
    pub guild_name: Option<String>,
    pub in_thread: bool,
    pub in_group_dm: bool,
    pub in_forum_post: bool,
    pub me: Id,
}

/// `moment.duration(...).humanize()` in the interface locale.
pub fn humanize(milliseconds: i64) -> String {
    dc_intl::humanize(milliseconds, false)
}

/// What a hook of a message stands for in the line.
#[derive(Clone, Copy)]
enum Hook<'a> {
    /// A user's name, a link to their profile (`usernameHook` and the like).
    User(&'a User),
    /// A name with no profile behind it.
    Name,
    /// Medium weight, strong colour.
    Bold,
    /// A link.
    Link,
}

/// A catalog message as the parts of a line: its plain runs as text, `$b` as bold, `$link` as a link, and each hook
/// as `hooks` says. The hooks are given no value, so their runs carry their children (the name they wrap).
fn keyed(key: &str, values: &[(&str, dc_intl::Arg<'_>)], hooks: &[(&str, Hook<'_>)]) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for (text, tag) in dc_intl::format_runs(key, values) {
        let hook = tag.as_deref().and_then(|tag| {
            hooks
                .iter()
                .find(|(name, _)| *name == tag)
                .map(|(_, hook)| *hook)
        });
        let part = match (tag.as_deref(), hook) {
            (_, Some(Hook::User(user))) => Part::User(Box::new(user.clone())),
            (_, Some(Hook::Name)) => Part::Name(text),
            (Some("$b"), None) | (_, Some(Hook::Bold)) => Part::Bold(text),
            (Some("$link"), None) | (_, Some(Hook::Link)) => Part::Link(text),
            _ => Part::Text(text),
        };
        parts.push(part);
    }
    parts
}

/// The last text of a line without its trailing space, for a line the view ends with a control of its own.
fn trim_end(mut parts: Vec<Part>) -> Vec<Part> {
    if let Some(Part::Text(text)) = parts.last_mut() {
        let kept = text.trim_end().len();
        text.truncate(kept);
        if text.is_empty() {
            parts.pop();
        }
    }
    parts
}

/// The strings table's entry for a type, with the message's own data filled in. `None` when the official handler
/// would return `null` for this message.
pub fn system_line(message: &Message, context: &LineContext) -> Option<Line> {
    use dc_intl::Arg;
    let author = &message.author;
    let name = author.display_name().to_owned();
    let line = |icon, parts: Vec<Part>| Line {
        icon,
        parts,
        action: None,
    };
    // `{username}` and its `usernameHook`: the author, the shape most entries take.
    let by_author = |key: &str, extra: Vec<(&str, Arg<'_>)>, hooks: &[(&str, Hook<'_>)]| {
        let mut values = vec![("username", Arg::from(name.clone()))];
        values.extend(extra);
        let mut all = vec![("usernameHook", Hook::User(author))];
        all.extend_from_slice(hooks);
        keyed(key, &values, &all)
    };
    let guild = context.guild_name.clone().unwrap_or_default();
    let first = message.extra.mention_users.first();
    Some(match message.kind {
        RECIPIENT_ADD => {
            let target = first?;
            let parts = if context.in_thread {
                // `ss`
                keyed(
                    "tusv2h",
                    &[
                        ("actorName", Arg::from(name.clone())),
                        ("targetName", Arg::from(target.display_name())),
                    ],
                    &[
                        ("actorHook", Hook::User(author)),
                        ("targetHook", Hook::User(target)),
                    ],
                )
            } else {
                // `i1`
                by_author(
                    "MMN2Jq",
                    vec![("otherUsername", Arg::from(target.display_name()))],
                    &[("otherUsernameHook", Hook::User(target))],
                )
            };
            line(LineIcon::ArrowIn, parts)
        }
        RECIPIENT_REMOVE => {
            let target = first?;
            let left = target.id == author.id;
            let parts = match (context.in_thread, left) {
                // `sl`
                (true, _) => keyed(
                    "32QI5/",
                    &[
                        ("actorName", Arg::from(name.clone())),
                        ("targetName", Arg::from(target.display_name())),
                    ],
                    &[
                        ("actorHook", Hook::User(author)),
                        ("targetHook", Hook::User(target)),
                    ],
                ),
                // `i2` without another user.
                (false, true) => by_author("5v2xa8", Vec::new(), &[]),
                (false, false) => by_author(
                    "L2FyVq",
                    vec![("otherUsername", Arg::from(target.display_name()))],
                    &[("otherUsernameHook", Hook::User(target))],
                ),
            };
            line(LineIcon::ArrowOut, parts)
        }
        CALL => {
            let call = message.extra.call.as_ref()?;
            // Module 117096: missed is `!isCallActive && !participants.includes(me)`; a call with no end is active.
            let missed = call.ended.is_some() && !call.participants.contains(&context.me);
            // Module 480012: `call.duration.humanize()`, none while the call goes on.
            let length = call
                .ended
                .map(|ended| humanize((ended - message.timestamp).num_milliseconds()));
            let duration = |length: &String| vec![("callDuration", Arg::from(length.clone()))];
            if missed {
                let parts = match &length {
                    Some(length) => by_author("AcqBmO", duration(length), &[]),
                    None => by_author("43phHx", Vec::new(), &[]),
                };
                line(LineIcon::CallMissed, parts)
            } else {
                let parts = match &length {
                    Some(length) => by_author("7TeC1P", duration(length), &[]),
                    None => by_author("LuB5RD", Vec::new(), &[]),
                };
                let mut shown = line(LineIcon::Call, parts);
                if call.ended.is_none() {
                    shown.action = Some(LineAction::JoinCall);
                }
                shown
            }
        }
        CHANNEL_NAME_CHANGE => {
            // `iD`
            if context.in_group_dm {
                let key = if message.content.is_empty() {
                    "AI1ZZr"
                } else {
                    "geP3rU"
                };
                let parts = by_author(
                    key,
                    vec![
                        ("channelName", Arg::from(message.content.clone())),
                        ("editGroupButton", Arg::from("")),
                    ],
                    &[],
                );
                Line {
                    icon: LineIcon::Pencil,
                    parts: trim_end(parts),
                    action: Some(LineAction::EditGroup),
                }
            } else {
                let key = if context.in_forum_post {
                    "SOQ4hJ"
                } else {
                    "oItgEw"
                };
                line(
                    LineIcon::Pencil,
                    by_author(
                        key,
                        vec![("channelName", Arg::from(message.content.clone()))],
                        &[],
                    ),
                )
            }
        }
        CHANNEL_ICON_CHANGE => {
            // `iy`
            let parts = by_author("4wLp25", vec![("editGroupButton", Arg::from(""))], &[]);
            Line {
                icon: LineIcon::Pencil,
                parts: trim_end(parts),
                action: Some(LineAction::EditGroup),
            }
        }
        GUILD_BOOST..=GUILD_BOOST_TIER_3 => {
            // `sY`; the tier names of module 473145 `gb`.
            let tier = match message.kind {
                GUILD_BOOST_TIER_1 => Some("nzXtaS"),
                GUILD_BOOST_TIER_2 => Some("h33/uW"),
                GUILD_BOOST_TIER_3 => Some("BfF6ED"),
                _ => None,
            };
            // `sp`: the content is the number of boosts, 1 when it is not a positive number.
            let count = message
                .content
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|count| *count != 0.0 && !count.is_nan())
                .unwrap_or(1.0);
            let times = vec![("numSubscriptions", Arg::Num(count))];
            let parts = match tier.filter(|_| !guild.is_empty()) {
                Some(tier) => {
                    let mut values = vec![
                        ("guildName", Arg::from(guild.clone())),
                        ("newTierName", Arg::from(dc_intl::t(tier))),
                    ];
                    if count > 1.0 {
                        values.extend(times);
                        by_author("PO9uJD", values, &[])
                    } else {
                        by_author("cUfTTE", values, &[])
                    }
                }
                None if count > 1.0 => by_author("yfC9ds", times, &[]),
                None => by_author("57St/7", Vec::new(), &[]),
            };
            line(LineIcon::Boost, parts)
        }
        CHANNEL_FOLLOW_ADD => {
            // `iS`: the webhook's name is the content.
            line(
                LineIcon::ArrowIn,
                by_author(
                    "47CZc2",
                    vec![("webhookName", Arg::from(message.content.clone()))],
                    &[("webhookNameHook", Hook::Bold)],
                ),
            )
        }
        // `iB`, `iJ`, `iz`: the guild's name, else the literal "this server" (`iV`, not a catalog message).
        GUILD_DISCOVERY_DISQUALIFIED
        | GUILD_DISCOVERY_GRACE_PERIOD_INITIAL_WARNING
        | GUILD_DISCOVERY_GRACE_PERIOD_FINAL_WARNING => {
            let key = match message.kind {
                GUILD_DISCOVERY_DISQUALIFIED => "bYdG3f",
                GUILD_DISCOVERY_GRACE_PERIOD_INITIAL_WARNING => "vzfXql",
                _ => "/TfvyR",
            };
            let shown = if guild.is_empty() {
                "this server".to_owned()
            } else {
                guild.clone()
            };
            line(
                LineIcon::Warning,
                keyed(key, &[("guildName", Arg::from(shown))], &[]),
            )
        }
        GUILD_DISCOVERY_REQUALIFIED => line(
            LineIcon::Discovery,
            vec![Part::Text(dc_intl::t("tu6tOR").to_owned())],
        ),
        // `i4`, `i6`, `se`: the content is the topic.
        STAGE_START | STAGE_END | STAGE_TOPIC => {
            let (key, icon) = match message.kind {
                STAGE_START => ("Rv+TSM", LineIcon::StageStart),
                STAGE_END => ("zla/ux", LineIcon::Stage),
                _ => ("Yy6vLs", LineIcon::Stage),
            };
            line(
                icon,
                by_author(
                    key,
                    vec![("topic", Arg::from(message.content.clone()))],
                    &[],
                ),
            )
        }
        // `i8`
        STAGE_SPEAKER => line(LineIcon::SpeakerArrow, by_author("dKW5C+", Vec::new(), &[])),
        // `i9`
        STAGE_RAISE_HAND => line(LineIcon::Hand, by_author("tv2DNy", Vec::new(), &[])),
        GUILD_APPLICATION_PREMIUM_SUBSCRIPTION => {
            let parts = match &message.extra.application {
                Some((_, application)) => by_author(
                    "dXdgno",
                    vec![("applicationName", Arg::from(application.clone()))],
                    &[],
                ),
                None => by_author("O829Uu", Vec::new(), &[]),
            };
            line(LineIcon::Ticket, parts)
        }
        PRIVATE_CHANNEL_INTEGRATION_ADDED | PRIVATE_CHANNEL_INTEGRATION_REMOVED => {
            // Module 552691 `eu` / `A0`.
            let added = message.kind == PRIVATE_CHANNEL_INTEGRATION_ADDED;
            let icon = if added {
                LineIcon::ArrowIn
            } else {
                LineIcon::ArrowOut
            };
            let parts = match &message.extra.application {
                Some((_, application)) => by_author(
                    if added { "J8SaGy" } else { "eGCDak" },
                    vec![("otherUsername", Arg::from(application.clone()))],
                    &[("otherUsernameHook", Hook::Name)],
                ),
                None => by_author(if added { "+6V2sd" } else { "sAX6rs" }, Vec::new(), &[]),
            };
            line(icon, parts)
        }
        // `iw`: the content is the time the actions end, written by `toLocaleString` (not ported: the content is
        // shown as sent).
        GUILD_INCIDENT_ALERT_MODE_ENABLED => line(
            LineIcon::Warning,
            by_author(
                "+m8eD7",
                vec![("time", Arg::from(message.content.clone()))],
                &[],
            ),
        ),
        GUILD_INCIDENT_ALERT_MODE_DISABLED => {
            line(LineIcon::Warning, by_author("BHeke+", Vec::new(), &[]))
        }
        // `iK`
        GUILD_INCIDENT_REPORT_RAID => line(
            LineIcon::Warning,
            by_author("W0UBI3", vec![("guildName", Arg::from(guild.clone()))], &[]),
        ),
        // `iZ`
        GUILD_INCIDENT_REPORT_FALSE_ALARM => {
            line(LineIcon::Warning, by_author("qntXNQ", Vec::new(), &[]))
        }
        PURCHASE_NOTIFICATION => {
            let product = message.extra.purchase_product.clone()?;
            // `V`
            line(
                LineIcon::ArrowIn,
                by_author("TQs67g", vec![("productName", Arg::from(product))], &[]),
            )
        }
        CHANNEL_LINKED_TO_LOBBY => {
            let (_, application) = message.extra.application.as_ref()?;
            // `ik`
            line(
                LineIcon::Refresh,
                by_author(
                    "mAtJTC",
                    vec![("applicationName", Arg::from(application.clone()))],
                    &[("applicationNameHook", Hook::Bold)],
                ),
            )
        }
        // Without the join request's user and guild the handlers write the short strings.
        GUILD_JOIN_REQUEST_ACCEPT_NOTIFICATION => line(
            LineIcon::ArrowIn,
            vec![Part::Text(dc_intl::t("2VLV0d").to_owned())],
        ),
        GUILD_JOIN_REQUEST_REJECT_NOTIFICATION => line(
            LineIcon::ArrowOut,
            vec![Part::Text(dc_intl::t("FVF6qU").to_owned())],
        ),
        GUILD_JOIN_REQUEST_WITHDRAWN_NOTIFICATION => line(
            LineIcon::ArrowOut,
            vec![Part::Text(dc_intl::t("BMlbE7").to_owned())],
        ),
        // `s9` without the session's length.
        VOICE_SESSION => line(
            LineIcon::Voice,
            by_author("eX6e/3", Vec::new(), &[("channelHook", Hook::Link)]),
        ),
        ROLE_SUBSCRIPTION_PURCHASE => {
            let role = message.extra.role_subscription.as_ref()?;
            // Module 942075 `gw`: with the months when there are any.
            let key = match (role.total_months > 0, role.renewal) {
                (true, true) => "Iy66M7",
                (true, false) => "eCgb2W",
                (false, true) => "mPTTdv",
                (false, false) => "mYjFFx",
            };
            line(
                LineIcon::Ticket,
                by_author(
                    key,
                    vec![
                        ("guildName", Arg::from(guild.clone())),
                        ("tierName", Arg::from(role.tier_name.clone())),
                        ("months", Arg::from(role.total_months)),
                    ],
                    &[],
                ),
            )
        }
        IN_GAME_MESSAGE_NUX => {
            let (_, application) = message.extra.application.as_ref()?;
            // `i0`
            line(
                LineIcon::GameController,
                by_author(
                    "m7Lwas",
                    vec![("gameName", Arg::from(application.clone()))],
                    &[("gameIconHook", Hook::Name)],
                ),
            )
        }
        POLL_RESULT => {
            let question = message
                .embeds
                .first()?
                .fields
                .iter()
                .find(|field| field.name == "poll_question_text")?
                .value
                .clone();
            // The title is cut to 50 characters by `EJ`.
            let title: String = if question.chars().count() > 50 {
                question.chars().take(50).collect::<String>() + "…"
            } else {
                question
            };
            line(
                LineIcon::Polls,
                by_author("VJcK41", vec![("title", Arg::from(title))], &[]),
            )
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(kind: u64) -> Message {
        let mut message = Message::from_value(&serde_json::json!({
            "id": "1", "channel_id": "2", "type": kind, "content": "",
            "timestamp": "2026-11-11T10:00:00.000000+00:00",
            "author": {"id": "3", "username": "ada"},
        }))
        .unwrap();
        message.kind = kind;
        message
    }

    #[test]
    fn user_messages_use_the_ordinary_layout() {
        for kind in [0, 19, 20, 23, 26, 41, 45, 47, 68] {
            assert_eq!(presentation(&message(kind)), Presentation::Normal, "{kind}");
            assert!(!is_system(kind));
        }
    }

    #[test]
    fn the_thread_starter_is_not_an_ordinary_or_system_line() {
        assert_eq!(
            presentation(&message(THREAD_STARTER_MESSAGE)),
            Presentation::ThreadStarter
        );
    }

    #[test]
    fn table_entries_without_a_handler_draw_nothing() {
        for kind in [22, 40, 42, 43, 55, 56, 57, 63, 66, 70, 99] {
            assert_eq!(
                presentation(&message(kind)),
                Presentation::Nothing,
                "{kind}"
            );
        }
    }

    #[test]
    fn every_system_type_with_a_line_has_one() {
        let mut shown = message(0);
        shown.extra.mention_users = vec![User {
            id: 4,
            username: "bob".into(),
            ..User::default()
        }];
        shown.extra.application = Some((9, "Helper".into()));
        shown.extra.call = Some(CallInfo::default());
        shown.extra.purchase_product = Some("Pack".into());
        shown.extra.role_subscription = Some(RoleSubscription::default());
        shown.extra.poll = Some(Poll::default());
        for kind in 0..=70 {
            shown.kind = kind;
            if presentation(&shown) == Presentation::System
                && ![
                    CHANNEL_PINNED_MESSAGE,
                    USER_JOIN,
                    THREAD_CREATED,
                    FRIEND_REQUEST_ACCEPTED,
                ]
                .contains(&kind)
            {
                assert!(
                    system_line(&shown, &LineContext::default()).is_some(),
                    "type {kind} says System but has no line"
                );
            }
        }
    }

    #[test]
    fn the_channel_name_line_follows_the_official_string() {
        let mut changed = message(CHANNEL_NAME_CHANGE);
        changed.content = "general-2".into();
        let line = system_line(&changed, &LineContext::default()).unwrap();
        assert_eq!(line.icon, LineIcon::Pencil);
        assert_eq!(
            line.parts[1..],
            [
                Part::Text(" changed the channel name: ".into()),
                Part::Bold("general-2".into())
            ]
        );
        let post = system_line(
            &changed,
            &LineContext {
                in_forum_post: true,
                ..LineContext::default()
            },
        )
        .unwrap();
        assert_eq!(
            post.parts[1],
            Part::Text(" changed the post title: ".into())
        );
    }

    #[test]
    fn boosts_name_the_tier_reached() {
        let line = system_line(
            &message(GUILD_BOOST_TIER_2),
            &LineContext {
                guild_name: Some("Fixture".into()),
                ..LineContext::default()
            },
        )
        .unwrap();
        // `cUfTTE`: "{username} just boosted the server! {guildName} has achieved **{newTierName}!**".
        assert_eq!(
            line.parts[1],
            Part::Text(" just boosted the server! Fixture has achieved ".into())
        );
        assert_eq!(line.parts[2], Part::Bold("Level 2!".into()));
        // `yfC9ds`: a content of several boosts names their number.
        let mut many = message(GUILD_BOOST);
        many.content = "3".into();
        let line = system_line(&many, &LineContext::default()).unwrap();
        assert_eq!(
            line.parts[1..],
            [
                Part::Text(" just boosted the server ".into()),
                Part::Bold("3".into()),
                Part::Text(" times!".into())
            ]
        );
    }

    /// The group name line ends before its "Edit Group" link, which the view draws as the line's action.
    #[test]
    fn the_group_name_line_leaves_its_link_to_the_view() {
        let mut renamed = message(CHANNEL_NAME_CHANGE);
        renamed.content = "pals".into();
        let line = system_line(
            &renamed,
            &LineContext {
                in_group_dm: true,
                ..LineContext::default()
            },
        )
        .unwrap();
        assert_eq!(line.action, Some(LineAction::EditGroup));
        assert_eq!(
            line.parts[1..],
            [
                Part::Text(" changed the group name: ".into()),
                Part::Bold("pals".into()),
                Part::Text(".".into())
            ]
        );
    }

    /// Module 552691 `eu`: the app's name, then the help center link.
    #[test]
    fn an_added_app_names_it_and_links_the_help_center() {
        let mut added = message(PRIVATE_CHANNEL_INTEGRATION_ADDED);
        added.extra.application = Some((5, "Helper".into()));
        let line = system_line(&added, &LineContext::default()).unwrap();
        assert_eq!(
            line.parts[1..],
            [
                Part::Text(" added the ".into()),
                Part::Name("Helper".into()),
                Part::Text(" app. See our ".into()),
                Part::Link("help center".into()),
                Part::Text(" for more info.".into())
            ]
        );
    }

    /// Module 942075: months in the plural form of the message, the membership wording without them.
    #[test]
    fn role_subscriptions_pick_their_message_by_months_and_renewal() {
        let mut joined = message(ROLE_SUBSCRIPTION_PURCHASE);
        joined.extra.role_subscription = Some(RoleSubscription {
            tier_name: "Gold".into(),
            total_months: 1,
            renewal: false,
        });
        let context = LineContext {
            guild_name: Some("Fixture".into()),
            ..LineContext::default()
        };
        let line = system_line(&joined, &context).unwrap();
        assert_eq!(line.parts.last(), Some(&Part::Text(" for 1 month!".into())));
        joined.extra.role_subscription = Some(RoleSubscription {
            tier_name: "Gold".into(),
            total_months: 0,
            renewal: true,
        });
        let line = system_line(&joined, &context).unwrap();
        assert_eq!(line.parts.last(), Some(&Part::Text(" membership!".into())));
    }

    #[test]
    fn a_missed_call_reads_from_the_viewer() {
        let mut call = message(CALL);
        call.extra.call = Some(CallInfo {
            participants: vec![3],
            ended: Some(call.timestamp + chrono::Duration::seconds(200)),
        });
        let mine = system_line(
            &call,
            &LineContext {
                me: 3,
                ..LineContext::default()
            },
        )
        .unwrap();
        assert_eq!(mine.icon, LineIcon::Call);
        assert_eq!(
            mine.parts[1],
            Part::Text(" started a call that lasted 3 minutes.".into())
        );
        let theirs = system_line(
            &call,
            &LineContext {
                me: 8,
                ..LineContext::default()
            },
        )
        .unwrap();
        assert_eq!(theirs.icon, LineIcon::CallMissed);
        assert_eq!(
            theirs.parts[0],
            Part::Text("You missed a call from ".into())
        );
    }

    #[test]
    fn humanize_matches_the_duration_rounding() {
        assert_eq!(humanize(10_000), "a few seconds");
        assert_eq!(humanize(60_000), "a minute");
        assert_eq!(humanize(200_000), "3 minutes");
        assert_eq!(humanize(3_600_000), "an hour");
        assert_eq!(humanize(3 * 3_600_000), "3 hours");
        assert_eq!(humanize(2 * 86_400_000), "2 days");
    }

    #[test]
    fn automod_needs_its_notification_embed() {
        let mut alert = message(AUTO_MODERATION_ACTION);
        assert_eq!(presentation(&alert), Presentation::Nothing);
        alert.embeds.push(crate::Embed {
            kind: "auto_moderation_notification".into(),
            ..crate::Embed::default()
        });
        assert_eq!(presentation(&alert), Presentation::Normal);
    }

    /// The lines' words come from the catalog: the line builder's string literals are catalog keys and the names of
    /// values and hooks, never interface text (the guard against a sentence written back in English).
    #[test]
    fn the_line_builder_writes_no_interface_text() {
        let source = include_str!("message_kinds.rs");
        let start = source.find("fn keyed(").expect("the builder");
        let end = source.find("#[cfg(test)]").expect("the tests");
        let body = &source[start..end];
        let allowed = ["this server", "\u{2026}", "poll_question_text"];
        let mut found = Vec::new();
        let mut rest = body;
        while let Some(open) = rest.find('"') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('"') else {
                break;
            };
            let text = &after[..close];
            rest = &after[close + 1..];
            let name = text
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '$');
            let english = dc_intl::keys_of(text).next().is_some() && !dc_intl::has_key(text);
            if !allowed.contains(&text) && (english || !(dc_intl::has_key(text) || name)) {
                found.push(text.to_owned());
            }
        }
        assert!(
            found.is_empty(),
            "literal text in the line builder: {found:?}"
        );
    }
}
