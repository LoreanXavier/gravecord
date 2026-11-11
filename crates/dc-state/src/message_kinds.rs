//! What each message type draws. Port of the message type enum (module 448761), the system message table `ls` in
//! module 117096 and the thread starter component `n$` in module 891522, with the strings from
//! `design/snapshot/dumps/STRINGS/en-US.json`.
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

/// A system message's line.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub icon: LineIcon,
    pub parts: Vec<Part>,
    /// `T.A.Action`: " — Join the call".
    pub action: Option<String>,
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

/// `moment.duration(...).humanize()`.
pub fn humanize(milliseconds: i64) -> String {
    let seconds = (milliseconds.max(0) as f64 / 1000.).round() as i64;
    let minutes = (seconds as f64 / 60.).round() as i64;
    let hours = (minutes as f64 / 60.).round() as i64;
    let days = (hours as f64 / 24.).round() as i64;
    let months = (days as f64 / 30.).round() as i64;
    let plural =
        |count: i64, unit: &str| format!("{count} {unit}{}", if count == 1 { "" } else { "s" });
    if seconds < 45 {
        "a few seconds".to_owned()
    } else if seconds < 90 {
        "a minute".to_owned()
    } else if minutes < 45 {
        plural(minutes, "minute")
    } else if minutes < 90 {
        "an hour".to_owned()
    } else if hours < 22 {
        plural(hours, "hour")
    } else if hours < 36 {
        "a day".to_owned()
    } else if days < 26 {
        plural(days, "day")
    } else if days < 46 {
        "a month".to_owned()
    } else if days < 320 {
        plural(months, "month")
    } else if days < 548 {
        "a year".to_owned()
    } else {
        plural((days as f64 / 365.).round() as i64, "year")
    }
}

/// The strings table's entry for a type, with the message's own data filled in. `None` when the official handler
/// would return `null` for this message.
pub fn system_line(message: &Message, context: &LineContext) -> Option<Line> {
    let actor = || Part::User(Box::new(message.author.clone()));
    let text = |value: &str| Part::Text(value.to_owned());
    let line = |icon, parts: Vec<Part>| Line {
        icon,
        parts,
        action: None,
    };
    let guild = context.guild_name.clone().unwrap_or_default();
    let first = message.extra.mention_users.first();
    Some(match message.kind {
        RECIPIENT_ADD => {
            let target = first?;
            if context.in_thread {
                // `tusv2h`
                line(
                    LineIcon::ArrowIn,
                    vec![
                        actor(),
                        text(" added "),
                        Part::User(Box::new(target.clone())),
                        text(" to the thread."),
                    ],
                )
            } else {
                // `MMN2Jq`
                line(
                    LineIcon::ArrowIn,
                    vec![
                        actor(),
                        text(" added "),
                        Part::User(Box::new(target.clone())),
                        text(" to the group."),
                    ],
                )
            }
        }
        RECIPIENT_REMOVE => {
            let target = first?;
            let left = target.id == message.author.id;
            match (context.in_thread, left) {
                // `32QI5/`
                (true, _) => line(
                    LineIcon::ArrowOut,
                    vec![
                        actor(),
                        text(" removed "),
                        Part::User(Box::new(target.clone())),
                        text(" from the thread."),
                    ],
                ),
                // `5v2xa8`
                (false, true) => line(LineIcon::ArrowOut, vec![actor(), text(" left the group.")]),
                // `L2FyVq`
                (false, false) => line(
                    LineIcon::ArrowOut,
                    vec![
                        actor(),
                        text(" removed "),
                        Part::User(Box::new(target.clone())),
                        text(" from the group."),
                    ],
                ),
            }
        }
        CALL => {
            let call = message.extra.call.as_ref()?;
            // Module 117096: missed is `!isCallActive && !participants.includes(me)`; a call with no end is active.
            let missed = call.ended.is_some() && !call.participants.contains(&context.me);
            let length = call
                .ended
                .map(|ended| humanize((ended - message.timestamp).num_milliseconds()));
            if missed {
                // `AcqBmO`, `43phHx`: "You missed a call from {username}."
                let mut parts = vec![text("You missed a call from "), actor()];
                match length {
                    Some(length) => parts.push(Part::Text(format!(" that lasted {length}."))),
                    None => parts.push(text(".")),
                }
                line(LineIcon::CallMissed, parts)
            } else {
                // `7TeC1P`, `LuB5RD`
                let mut parts = vec![actor()];
                match length {
                    Some(length) => {
                        parts.push(Part::Text(format!(" started a call that lasted {length}.")))
                    }
                    None => parts.push(text(" started a call.")),
                }
                let mut shown = line(LineIcon::Call, parts);
                if call.ended.is_none() {
                    shown.action = Some("Join the call".to_owned());
                }
                shown
            }
        }
        CHANNEL_NAME_CHANGE => {
            if context.in_group_dm {
                // `AI1ZZr`, `geP3rU`
                let mut shown = if message.content.is_empty() {
                    line(
                        LineIcon::Pencil,
                        vec![actor(), text(" removed the custom group name.")],
                    )
                } else {
                    line(
                        LineIcon::Pencil,
                        vec![
                            actor(),
                            text(" changed the group name: "),
                            Part::Bold(message.content.clone()),
                            text("."),
                        ],
                    )
                };
                shown.action = Some("Edit Group".to_owned());
                shown
            } else {
                // `SOQ4hJ` for a forum post, `oItgEw` otherwise.
                let what = if context.in_forum_post {
                    " changed the post title: "
                } else {
                    " changed the channel name: "
                };
                line(
                    LineIcon::Pencil,
                    vec![actor(), text(what), Part::Bold(message.content.clone())],
                )
            }
        }
        CHANNEL_ICON_CHANGE => {
            // `4wLp25`
            let mut shown = line(
                LineIcon::Pencil,
                vec![actor(), text(" changed the group icon.")],
            );
            shown.action = Some("Edit Group".to_owned());
            shown
        }
        GUILD_BOOST..=GUILD_BOOST_TIER_3 => {
            let tier = match message.kind {
                GUILD_BOOST_TIER_1 => Some("Level 1"),
                GUILD_BOOST_TIER_2 => Some("Level 2"),
                GUILD_BOOST_TIER_3 => Some("Level 3"),
                _ => None,
            };
            // `57St/7`, `cUfTTE`
            let mut parts = vec![actor(), text(" just boosted the server!")];
            if let (Some(tier), false) = (tier, guild.is_empty()) {
                parts.push(Part::Text(format!(" {guild} has achieved ")));
                parts.push(Part::Bold(format!("{tier}!")));
            }
            line(LineIcon::Boost, parts)
        }
        CHANNEL_FOLLOW_ADD => {
            // `47CZc2`: the webhook's name is the content.
            line(
                LineIcon::ArrowIn,
                vec![
                    actor(),
                    text(" has added "),
                    Part::Bold(message.content.clone()),
                    text(" to this channel. Its most important updates will show up here."),
                ],
            )
        }
        GUILD_DISCOVERY_DISQUALIFIED => line(
            LineIcon::Warning,
            vec![
                Part::Link(guild.clone()),
                text(
                    " has been removed from Server Discovery because it no longer passes all the requirements. Check ",
                ),
                Part::Link("Server Settings".to_owned()),
                text(" for more details."),
            ],
        ),
        GUILD_DISCOVERY_REQUALIFIED => line(
            LineIcon::Discovery,
            vec![text(
                "This server is eligible for Server Discovery again and has been automatically relisted!",
            )],
        ),
        GUILD_DISCOVERY_GRACE_PERIOD_INITIAL_WARNING => line(
            LineIcon::Warning,
            vec![
                Part::Link(guild.clone()),
                text(
                    " has failed Discovery activity requirements for 1 week. If it fails for 4 weeks in a row, it will be automatically removed from Discovery.",
                ),
            ],
        ),
        GUILD_DISCOVERY_GRACE_PERIOD_FINAL_WARNING => line(
            LineIcon::Warning,
            vec![
                Part::Link(guild.clone()),
                text(
                    " has failed Discovery activity requirements for 3 weeks in a row. If it fails for 1 more week, it will be removed from Discovery.",
                ),
            ],
        ),
        STAGE_START => line(
            LineIcon::StageStart,
            vec![
                actor(),
                text(" started "),
                Part::Bold(message.content.clone()),
            ],
        ),
        STAGE_END => line(
            LineIcon::Stage,
            vec![
                actor(),
                text(" ended "),
                Part::Bold(message.content.clone()),
            ],
        ),
        STAGE_SPEAKER => line(
            LineIcon::SpeakerArrow,
            vec![actor(), text(" is now a speaker.")],
        ),
        STAGE_RAISE_HAND => line(LineIcon::Hand, vec![actor(), text(" requested to speak.")]),
        STAGE_TOPIC => line(
            LineIcon::Stage,
            vec![
                actor(),
                text(" changed the Stage topic: "),
                Part::Bold(message.content.clone()),
            ],
        ),
        GUILD_APPLICATION_PREMIUM_SUBSCRIPTION => {
            let (_, name) = message.extra.application.as_ref()?;
            // `dXdgno`
            line(
                LineIcon::Ticket,
                vec![
                    actor(),
                    Part::Text(format!(
                        " upgraded {name} to premium for this server! \u{1f389}"
                    )),
                ],
            )
        }
        PRIVATE_CHANNEL_INTEGRATION_ADDED | PRIVATE_CHANNEL_INTEGRATION_REMOVED => {
            let name = message
                .extra
                .application
                .as_ref()
                .map(|(_, name)| name.clone());
            let added = message.kind == PRIVATE_CHANNEL_INTEGRATION_ADDED;
            let icon = if added {
                LineIcon::ArrowIn
            } else {
                LineIcon::ArrowOut
            };
            match name {
                // `/qIck6`
                Some(name) if added => line(
                    icon,
                    vec![
                        actor(),
                        text(" has added "),
                        Part::Name(name),
                        text(" to the server."),
                    ],
                ),
                Some(name) => line(
                    icon,
                    vec![
                        actor(),
                        text(" removed "),
                        Part::Name(name),
                        text(" from the server."),
                    ],
                ),
                None if added => line(icon, vec![text("An app was added to the server.")]),
                None => line(icon, vec![text("An app was removed from the server.")]),
            }
        }
        GUILD_INCIDENT_ALERT_MODE_ENABLED => line(
            LineIcon::Warning,
            vec![
                actor(),
                Part::Text(format!(" enabled security actions for {guild}")),
                Part::Text(if message.content.is_empty() {
                    String::new()
                } else {
                    format!(" until {}", message.content)
                }),
            ],
        ),
        GUILD_INCIDENT_ALERT_MODE_DISABLED => line(
            LineIcon::Warning,
            vec![
                actor(),
                Part::Text(format!(" disabled security actions for {guild}")),
            ],
        ),
        GUILD_INCIDENT_REPORT_RAID => line(
            LineIcon::Warning,
            vec![actor(), Part::Text(format!(" reported a raid in {guild}."))],
        ),
        GUILD_INCIDENT_REPORT_FALSE_ALARM => line(
            LineIcon::Warning,
            vec![actor(), text(" resolved an Activity Alert.")],
        ),
        PURCHASE_NOTIFICATION => {
            let product = message.extra.purchase_product.clone()?;
            // `TQs67g`
            line(
                LineIcon::ArrowIn,
                vec![
                    actor(),
                    text(" has purchased "),
                    Part::Link(product),
                    text("!"),
                ],
            )
        }
        CHANNEL_LINKED_TO_LOBBY => {
            let (_, name) = message.extra.application.as_ref()?;
            // `mAtJTC`
            line(
                LineIcon::Refresh,
                vec![
                    actor(),
                    text(" has linked this channel to "),
                    Part::Bold(name.clone()),
                    text(". Messages are now syncing. "),
                    Part::Link("Learn More".to_owned()),
                ],
            )
        }
        GUILD_JOIN_REQUEST_ACCEPT_NOTIFICATION => line(
            LineIcon::ArrowIn,
            vec![text("This application was approved! Welcome!")],
        ),
        GUILD_JOIN_REQUEST_REJECT_NOTIFICATION => line(
            LineIcon::ArrowOut,
            vec![text("This application was rejected.")],
        ),
        GUILD_JOIN_REQUEST_WITHDRAWN_NOTIFICATION => line(
            LineIcon::ArrowOut,
            vec![text("This application has been withdrawn.")],
        ),
        VOICE_SESSION => line(
            LineIcon::Voice,
            vec![
                actor(),
                text(" started "),
                Part::Link("a voice hangout".to_owned()),
            ],
        ),
        ROLE_SUBSCRIPTION_PURCHASE => {
            let role = message.extra.role_subscription.as_ref()?;
            let months = role.total_months;
            let how = if role.renewal {
                " renewed "
            } else {
                " joined "
            };
            let mut parts = vec![actor(), text(how), Part::Bold(role.tier_name.clone())];
            // `OQ0OUy`, `+N9bxq`, `OxP1NC`, `6Z1E+7`
            if months > 0 {
                parts.push(Part::Text(" and has been a subscriber of ".to_owned()));
                parts.push(Part::Link(guild));
                parts.push(Part::Text(format!(
                    " for {months} month{}!",
                    if months == 1 { "" } else { "s" }
                )));
            } else if role.renewal {
                parts.push(Part::Text(" in their ".to_owned()));
                parts.push(Part::Link(guild));
                parts.push(Part::Text(" subscription!".to_owned()));
            } else {
                parts.push(Part::Text(" as a subscriber of ".to_owned()));
                parts.push(Part::Link(guild));
                parts.push(Part::Text("!".to_owned()));
            }
            line(LineIcon::Ticket, parts)
        }
        IN_GAME_MESSAGE_NUX => {
            let (_, name) = message.extra.application.as_ref()?;
            // `m7Lwas`
            line(
                LineIcon::GameController,
                vec![
                    actor(),
                    text(" messaged you from "),
                    Part::Name(name.clone()),
                    text(
                        ". In-game chat may not include rich messaging features such as images, polls, or apps. ",
                    ),
                    Part::Link("Learn More".to_owned()),
                ],
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
            // `VJcK41`: the title is cut to 50 characters by `EJ`.
            let title: String = if question.chars().count() > 50 {
                question.chars().take(50).collect::<String>() + "…"
            } else {
                question
            };
            line(
                LineIcon::Polls,
                vec![
                    actor(),
                    text("'s poll "),
                    Part::Link(title),
                    text(" has closed."),
                ],
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
        assert_eq!(
            line.parts[1],
            Part::Text(" just boosted the server!".into())
        );
        assert_eq!(line.parts[2], Part::Text(" Fixture has achieved ".into()));
        assert_eq!(line.parts[3], Part::Bold("Level 2!".into()));
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
}
