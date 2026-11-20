//! Small chat-side dialogs and the thread requests that go one after the other.
//!
//! - The alert of module 579872 `show`: a small modal with a title, the body as its subtitle and one "Okay" (or the
//!   given confirm text), with an optional secondary button. Pin this post? (module 41402) is one.
//! - Thread updates of module 7909 that take two requests in a row (`lockThread` of an archived thread unarchives
//!   it first; replacing the forum's pinned post unpins the old one before the new one is pinned): each waits for the
//!   one before it to succeed.

use super::Root;
use super::forward::Destination;
use super::modal::{self, Tone};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Focusable, FontWeight, IntoElement, ParentElement, SharedString, Styled,
    Window, div, prelude::*, px,
};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};

/// The key of a step's answer: `thread-step:{chain}`.
pub(super) const KEY_PREFIX: &str = "thread-step:";

/// `--radius-sm` of the design system (module 952007): 8px.
const RADIUS_SM: f32 = 8.;

/// `OPT_IN_CHANNELS_ON` of a server's notification settings flags, `OPT_IN_ENABLED` of a channel override's (module
/// 395504 `WW`, `vv`).
const OPT_IN_CHANNELS_ON: u64 = 1 << 14;
const OPT_IN_ENABLED: u64 = 1 << 12;

/// Module 228098 (without its resource-channel exemption): the server browses picked channels only and neither the
/// channel nor its category is picked.
pub(super) fn opt_in_notice_shown(opt_in_on: bool, channel: bool, parent: bool) -> bool {
    opt_in_on && !channel && !parent
}

/// `ChannelFlags.PINNED` (module 746080 `lx`).
pub(super) const PINNED: u64 = 1 << 1;

/// What the alert's buttons do besides closing it.
#[derive(Clone, Debug)]
pub(super) enum Then {
    Close,
    /// Switch to Push to Talk of "Push to Talk Required" (module 596339 `wI`): `setMode(PUSH_TO_TALK)`.
    SwitchToPushToTalk,
    /// "Jump to Message" of module 81437's likely-spammer confirm: the jump goes on.
    JumpToMessage {
        channel: Id,
        message: Id,
    },
    /// Module 41402's `onConfirm`: `await unpin(old)`, then `pin(post)`.
    ReplacePin {
        post: Id,
        old: Id,
    },
    /// Retry of Forward Failed (module 181622): the forward modal again, with the failed destinations chosen.
    RetryForward {
        message: Box<dc_state::Message>,
        destinations: Vec<Destination>,
    },
    /// "Yes, turn them off" (module 228855): the server joins `restricted_guilds`.
    DmsOff {
        guild: Id,
    },
    /// Upload Logs of module 72514 after a survey: the RTC logs go up as Voice & Video's button sends them.
    UploadLogs,
    /// Save of the Soundboard Keybind modal (module 29681 `f`).
    SaveSoundboardKeybind,
    /// Confirm of "Are you sure you know this person?" (module 987176): the message request is accepted.
    AcceptMessageRequest {
        channel: Id,
    },
    /// "Manage Messages" of the scheduled limit alert (module 859403 `vh`): the inbox on its Scheduled tab.
    ScheduledInbox,
    /// Submit of Report a Raid (module 935074): `B` of module 859706.
    ReportRaid {
        guild: Id,
    },
    /// Submit of "Why was this server locked?" (module 336496): only the `GUILD_RAID_LOCKDOWN_FEEDBACK` analytics
    /// event, which is not sent here; the dialog closes.
    LockdownFeedback,
    /// Clear of "Clear message?" (module 990474 `C`): the mentioned command takes the composer.
    UseCommand {
        name: String,
        id: u64,
    },
    /// Confirm of Withdraw Application (module 678655 `handleCancelApplication`): `removeGuildJoinRequest`.
    WithdrawApplication {
        guild: Id,
    },
    /// Reapply of module 267.
    Reapply {
        guild: Id,
    },
    /// The button of "Mute Notifications" (module 259763): the app's DM takes `muted`.
    MuteAppDm {
        channel: Id,
        muted: bool,
    },
    /// Deauthorize of an app's DM (module 891522 `s6`).
    DeauthorizeApp {
        application: String,
        token: String,
    },
    /// Okay of "Delete Permission Settings" (module 495603's `handleDeletePermission` in module 336541):
    /// `clearPermissionOverwrite`, `DELETE /channels/{channel}/permissions/{target}`.
    ClearOverwrite {
        channel: Id,
        target: Id,
    },
    /// Mark as resolved of "What happened?" (module 580812): `A` of module 859706 with `W_` of the reasons chosen.
    ResolveRaid {
        guild: Id,
        message: Id,
    },
    /// Leave Server of "Server Unavailable" (module 537560): `leaveGuild`.
    LeaveGuild {
        guild: Id,
    },
    /// A help article opened while the dialog stays (Learn More of module 784578: `window.open`).
    OpenArticle {
        url: &'static str,
    },
    /// Back to Login of module 784578 (and its close): `AGE_GATE_MODAL_CLOSE`, then the login.
    BackToLogin,
    /// Accept of "Accept friend request?" (module 691464): the accept again with `confirm_stranger_request`.
    AcceptStranger {
        user: Id,
    },
    /// The confirm of modules 32167 and 413201 (`gF` of module 365258, after 413201 sets the activity default).
    SyncActivity {
        restricting: bool,
        guilds: Vec<Id>,
        activity: Option<u64>,
    },
}

/// Module 987176 as module 190857 opens it before a message request is accepted: the title, its subtitle, Cancel and
/// Confirm.
pub(super) fn accept_request_alert(channel: Id) -> Alert {
    Alert {
        id: "message-request-confirm",
        title: dc_intl::t("66tnno").to_owned(),
        subtitle: Some(vec![(dc_intl::t("c/k4SW").to_owned(), false)]),
        content: None,
        buttons: vec![
            AlertButton {
                id: "message-request-confirm-cancel",
                label: dc_intl::t("ETE/oC").to_owned(),
                tone: Tone::Secondary,
                then: Then::Close,
            },
            AlertButton {
                id: "message-request-confirm-accept",
                label: dc_intl::t("cY+Oob").to_owned(),
                tone: Tone::Primary,
                then: Then::AcceptMessageRequest { channel },
            },
        ],
    }
}

/// "Clear message?" (module 990474 `C`, the confirm of 314116): `+awCIy`, Cancel and Clear.
pub(super) fn clear_for_command_alert(name: String, id: u64) -> Alert {
    Alert::show(
        "clear-for-command",
        dc_intl::t("pe26Cj"),
        Some(dc_intl::t("+awCIy").to_owned()),
        Some(dc_intl::t("ETE/oC").to_owned()),
        Some(dc_intl::t("VkKicb").to_owned()),
        Then::UseCommand { name, id },
    )
}

/// The behaviours of Report a Raid (`M` of module 935074) with their strings.
const RAID_KINDS: [(&str, &str); 5] = [
    ("MESSAGE_SPAM", "fwloj2"),
    ("DM_SPAM", "9CYNmS"),
    ("MENTION_SPAM", "hR/IdO"),
    ("SUSPICIOUS_USERS", "lKXu+n"),
    ("SETTINGS_SPAM", "ETFVFw"),
];

/// The reasons of "Why was this server locked?" (`S` of module 336496, values of module 615550 `n7`) with their
/// strings; the last, Other, opens the text box.
pub(super) const LOCKDOWN_REASONS: [(&str, &str); 6] = [
    ("DM_SPAM", "//3pvi"),
    ("MENTION_SPAM", "SdVsip"),
    ("CHANNEL_SPAM", "uTiSVL"),
    ("SUS_NEW_MEMBERS", "GQczU8"),
    ("CHANGING_SETTINGS", "AAgqy3"),
    ("OTHER", "ryPKb7"),
];
const LOCKDOWN_OTHER: usize = 5;

/// Module 336496, opened by Security Actions (module 671576) when a save lifts every pause that was on: the six reasons
/// as a checkbox group, "Tell us what happened" under Other, Cancel and Submit (disabled until a reason is chosen).
pub(super) fn lockdown_feedback_alert() -> Alert {
    Alert {
        id: "lockdown-feedback",
        title: dc_intl::t("f5hd9P").to_owned(),
        subtitle: None,
        content: Some(Content::LockdownFeedback { chosen: Vec::new() }),
        buttons: vec![
            AlertButton {
                id: "lockdown-feedback-cancel",
                label: dc_intl::t("ETE/oC").to_owned(),
                tone: Tone::Secondary,
                then: Then::Close,
            },
            AlertButton {
                id: "lockdown-feedback-submit",
                label: dc_intl::t("nAt0rE").to_owned(),
                tone: Tone::Primary,
                then: Then::LockdownFeedback,
            },
        ],
    }
}

/// `MVz.GUILD_RAID` of module 652215 for "Learn more about raids".
const RAID_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/10989121220631";

/// Report a Raid (module 935074, on the dialog of 189213): "Which behavior best describes how this raid is currently
/// disrupting your server?" with Learn more about raids, the five behaviours as a checkbox group, Cancel and Submit.
pub(super) fn raid_report_alert(guild: Id) -> Alert {
    Alert {
        id: "report-raid",
        title: dc_intl::t("uYPGsS").to_owned(),
        subtitle: None,
        content: Some(Content::RaidReport { chosen: Vec::new() }),
        buttons: vec![
            AlertButton {
                id: "report-raid-cancel",
                label: dc_intl::t("ETE/oC").to_owned(),
                tone: Tone::Secondary,
                then: Then::Close,
            },
            AlertButton {
                id: "report-raid-submit",
                label: dc_intl::t("geKm7t").to_owned(),
                tone: Tone::Primary,
                then: Then::ReportRaid { guild },
            },
        ],
    }
}

/// `B` of module 859706: `POST /guilds/{id}/auto-moderation/report-raid` with no body, only when the server has a
/// safety alerts or public updates channel.
pub(super) fn raid_report_request(guild: Id, alerts_channel: Option<Id>) -> Option<Request> {
    alerts_channel?;
    Some(Request::Rest {
        key: None,
        method: reqwest::Method::POST,
        route: format!("/guilds/{guild}/auto-moderation/report-raid"),
        body: None,
    })
}

/// Report App of module 630891 (`useReportAppItemModal`, on the dialog of 189213 with no actions): "What do you
/// want to report?" over two clickable rows of module 632738.
pub(super) fn report_app_alert(user: Id, app: Id) -> Alert {
    Alert {
        id: "report-app",
        title: dc_intl::t("Bd10bR").to_owned(),
        subtitle: None,
        content: Some(Content::ReportApp { user, app }),
        buttons: Vec::new(),
    }
}

/// "Invite permission required" (module 132610): the title, its subtitle and Got It.
pub(super) fn invite_permission_alert() -> Alert {
    Alert {
        id: "invite-permission-required",
        title: dc_intl::t("xSV3nU").to_owned(),
        subtitle: Some(vec![(dc_intl::t("jPMGbB").to_owned(), false)]),
        content: None,
        buttons: vec![AlertButton {
            id: "invite-permission-got-it",
            label: dc_intl::t("NX+WJN").to_owned(),
            tone: Tone::Primary,
            then: Then::Close,
        }],
    }
}

#[derive(Clone, Debug)]
pub(super) struct AlertButton {
    pub id: &'static str,
    pub label: String,
    pub tone: Tone,
    pub then: Then,
}

/// What a dialog shows under its title.
#[derive(Clone, Debug)]
pub(super) enum Content {
    /// `body-md/normal` in `text-default` (module 130577's notice).
    Text(String),
    /// Module 181622's `failedDestinations_PiyhYP` box of rows.
    FailedDestinations(Vec<Destination>),
    /// Module 228855: "This change affects" over the server's row.
    AffectedGuild(Id),
    /// Module 929120: the card of the servers an activity change touches; `clickable` (module 413201) opens
    /// Activity Privacy.
    AffectedGuilds {
        guilds: Vec<Id>,
        restricting: bool,
        clickable: bool,
    },
    /// Module 940372: the files of a bulk emoji upload that failed, a row each (`errors_qy2FFG`, 8px apart).
    EmojiUploadErrors(Vec<(super::server_emoji::UploadError, String)>),
    /// Module 29681: the Open Soundboard (Hold) keybind being edited; `original` is its shortcut when the modal
    /// opened (`None`: there was none), put back unless Save is pressed.
    SoundboardKeybind { original: Option<Vec<[u32; 3]>> },
    /// Module 935074's checkbox group, with the indices of `RAID_KINDS` chosen.
    RaidReport { chosen: Vec<usize> },
    /// Module 267: "Reason:" (4px from it) and the moderators' reason, `body-md/medium` in `text-default`.
    RejectionReason(String),
    /// Module 336496's checkbox group, with the indices of `LOCKDOWN_REASONS` chosen.
    LockdownFeedback { chosen: Vec<usize> },
    /// Module 580812's checkbox group, with the indices of `automod_alert::RESOLUTIONS` chosen.
    RaidResolve { chosen: Vec<usize> },
    /// Report App of the bot overflow menu (module 630891): the app's profile or its behaviour.
    ReportApp { user: Id, app: Id },
    /// Module 181094: the whole topic of a channel (`parseTopic`, `body-md/normal`, selectable markup).
    ChannelTopic(Id),
}

/// Channel Topic (module 181094 on the dialog of 189213, `md` with its child): the title `X8jMDh`, the channel's
/// name after its channel list icon as the subtitle (`leadingIcon`, `sm` in `TEXT_SUBTLE`, `--space-4` from the text),
/// the topic, and no actions. Opened by a click on the header's topic (565265 `handleOpenTopic`, not on a link,
/// mention or emoji in it) and by See More of the spoiler gate (873614).
pub(super) fn channel_topic_alert(channel: Id) -> Alert {
    Alert {
        id: "channel-topic",
        title: dc_intl::t("X8jMDh").to_owned(),
        subtitle: None,
        content: Some(Content::ChannelTopic(channel)),
        buttons: Vec::new(),
    }
}

impl Content {
    /// Submit of module 336496 is disabled while no reason is chosen (`disabled: 0 === b.length`).
    pub(super) fn blocks_submit(&self) -> bool {
        matches!(self, Content::LockdownFeedback { chosen } if chosen.is_empty())
    }
}

/// The Soundboard Keybind modal (module 29681): "Soundboard Keybind", Cancel and Save.
pub(super) fn soundboard_keybind_alert(original: Option<Vec<[u32; 3]>>) -> Alert {
    Alert {
        id: "soundboard-keybind",
        title: dc_intl::t("0Osu3x").to_owned(),
        subtitle: None,
        content: Some(Content::SoundboardKeybind { original }),
        buttons: vec![
            AlertButton {
                id: "soundboard-keybind-cancel",
                label: dc_intl::t("ETE/oC").to_owned(),
                tone: Tone::Secondary,
                then: Then::Close,
            },
            AlertButton {
                id: "soundboard-keybind-save",
                label: dc_intl::t("R3BPH+").to_owned(),
                tone: Tone::Primary,
                then: Then::SaveSoundboardKeybind,
            },
        ],
    }
}

/// "Failed to upload files" (module 940372): the failed files of a bulk emoji upload and Got It.
pub(super) fn upload_errors_alert(
    errors: Vec<(super::server_emoji::UploadError, String)>,
) -> Alert {
    Alert {
        id: "emoji-upload-errors",
        title: dc_intl::t("AUIsQU").to_owned(),
        subtitle: None,
        content: Some(Content::EmojiUploadErrors(errors)),
        buttons: vec![AlertButton {
            id: "emoji-upload-errors-ok",
            label: dc_intl::t("NX+WJN").to_owned(),
            tone: Tone::Secondary,
            then: Then::Close,
        }],
    }
}

/// One open dialog: the alert of module 579872 `show` (small, the body as its subtitle) or a dialog of module 189213
/// with content (`md` once it has children).
#[derive(Clone, Debug)]
pub(super) struct Alert {
    pub id: &'static str,
    pub title: String,
    /// The subtitle as runs of text, the bold ones `$b` of the message.
    pub subtitle: Option<Vec<(String, bool)>>,
    pub content: Option<Content>,
    pub buttons: Vec<AlertButton>,
}

impl Alert {
    /// `show({title, body, confirmText?, cancelText?, onConfirm})`: the cancel button (when given) first, then the
    /// confirm button, "Okay" unless named.
    pub(super) fn show(
        id: &'static str,
        title: impl Into<String>,
        body: Option<String>,
        cancel: Option<String>,
        confirm: Option<String>,
        then: Then,
    ) -> Self {
        let mut buttons = Vec::new();
        if let Some(cancel) = cancel.filter(|text| !text.is_empty()) {
            buttons.push(AlertButton {
                id: "alert-cancel",
                label: cancel,
                tone: Tone::Secondary,
                then: Then::Close,
            });
        }
        buttons.push(AlertButton {
            id: "alert-confirm",
            label: confirm.unwrap_or_else(|| dc_intl::tr("Okay").to_owned()),
            tone: Tone::Primary,
            then,
        });
        Self {
            id,
            title: title.into(),
            subtitle: body.map(|body| vec![(body, false)]),
            content: None,
            buttons,
        }
    }

    /// Module 189213 picks `md` when the dialog has children, else `sm`.
    pub(super) fn medium(&self) -> bool {
        // "Push to Talk Required" names its size (`size: "md"`, module 596339 `wI`).
        self.content.is_some() || self.id == super::vad_permission::WARNING_ID
    }
}

/// The forwards of one Send of the forward modal, by nonce, until each has its answer.
pub(super) struct ForwardBatch {
    pub message: dc_state::Message,
    pub pending: HashMap<String, Destination>,
    pub failed: Vec<Destination>,
}

/// Forward Failed (module 181622): "The message could not be forwarded to the following N location(s):" over the
/// failed destinations; Cancel and Retry.
pub(super) fn forward_failed_alert(
    message: dc_state::Message,
    destinations: Vec<Destination>,
) -> Alert {
    Alert {
        id: "forward-failed",
        title: dc_intl::tr("Forward Failed").to_owned(),
        subtitle: Some(vec![(
            dc_intl::t!("cn9vFb", count = destinations.len()).to_string(),
            false,
        )]),
        content: Some(Content::FailedDestinations(destinations.clone())),
        buttons: vec![
            AlertButton {
                id: "forward-failed-cancel",
                label: dc_intl::tr("Cancel").to_owned(),
                tone: Tone::Secondary,
                then: Then::Close,
            },
            AlertButton {
                id: "forward-failed-retry",
                label: dc_intl::tr("Retry").to_owned(),
                tone: Tone::Primary,
                then: Then::RetryForward {
                    message: Box::new(message),
                    destinations,
                },
            },
        ],
    }
}

#[derive(Default)]
pub(super) struct State {
    pub alert: Option<Alert>,
    /// The chat's loading placeholders for the channel being loaded (`chat_skeleton.rs`).
    pub skeleton: Option<super::chat_skeleton::Held>,
    /// The "Other" text box of "Why was this server locked?" while that dialog is open.
    pub lockdown_other: Option<gpui::Entity<crate::editor::Editor>>,
    pub forward_batch: Option<ForwardBatch>,
    /// A right press on a `#channel` mention: where, and the channel (read by the message menu's opener).
    pub mention_hit: Option<(gpui::Point<gpui::Pixels>, Id)>,
    /// A right press on an attachment chip: where, its address and its file name (module 740024's menu).
    pub attachment_hit: Option<(gpui::Point<gpui::Pixels>, String, String)>,
    /// A dialog on the graphic modal (`ui/graphic_dialogs.rs`).
    pub graphic: Option<super::graphic_dialogs::GraphicDialog>,
    /// `_` of module 578818: the Urgent Message dialog was shown and the flag is not cleared yet.
    pub urgent_shown: bool,
    /// The requests still to go of each chain, by chain number.
    chains: HashMap<u64, VecDeque<(Id, Value)>>,
    next_chain: u64,
    /// `isStranger` of `RelationshipStore` (module 994500): per person, whether they are a stranger and until when
    /// (ms since the epoch) the answer holds (`friend_accept.rs`).
    pub strangers: HashMap<Id, (bool, i64)>,
    /// The underage dialog (module 784578) was closed without a button: the next frame goes back to the login.
    pub back_to_login: bool,
    /// Requests accepted at once whose answer has not come yet (put back if the server asks for the confirmation).
    pub accepting: std::collections::HashSet<Id>,
}

/// A thread action of the context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThreadStep {
    Close,
    Open,
    Lock,
    Unlock,
    Pin,
    Unpin,
}

/// The `PATCH /channels/{thread}` bodies module 7909 sends for an action, in order.
///
/// - `archiveThread(e, false)`: `{archived: true}`; `unarchiveThread(e, false)`: `{archived: false}`.
/// - `lockThread`: an archived thread is first unarchived, then `{locked: true, archived: <was archived>}`.
/// - `unlockThread`: an archived thread is first `unarchiveThread(e, true)` (`{archived: false, locked: false}`),
///   then `{locked: false, archived: <was archived>}`.
/// - `pin`: `updateFlags(flags | PINNED, isArchived)`: `{flags}`, with `archived: false` for an archived post;
///   `unpin`: `{flags: flags & ~PINNED}`.
pub(super) fn thread_bodies(step: ThreadStep, archived: bool, flags: u64) -> Vec<Value> {
    match step {
        ThreadStep::Close => vec![json!({"archived": true})],
        ThreadStep::Open => vec![json!({"archived": false})],
        ThreadStep::Lock => {
            let mut out = Vec::new();
            if archived {
                out.push(json!({"archived": false}));
            }
            out.push(json!({"locked": true, "archived": archived}));
            out
        }
        ThreadStep::Unlock => {
            let mut out = Vec::new();
            if archived {
                out.push(json!({"archived": false, "locked": false}));
            }
            out.push(json!({"locked": false, "archived": archived}));
            out
        }
        ThreadStep::Pin => {
            let mut body = json!({"flags": flags | PINNED});
            if archived {
                body["archived"] = json!(false);
            }
            vec![body]
        }
        ThreadStep::Unpin => vec![json!({"flags": flags & !PINNED})],
    }
}

impl Root {
    /// The thread as the store has it: (archived, flags).
    fn thread_state(&self, thread: Id) -> Option<(bool, u64)> {
        self.stores
            .guilds
            .iter()
            .find_map(|guild| guild.threads.iter().find(|known| known.id == thread))
            .map(|known| (known.archived, known.flags))
    }

    /// Module 626977's bars over a thread's chat (`chatHeaderBar_By_-nl`: `--background-mod-normal`, 8px 16px inside,
    /// the text `body-sm/normal` in `--text-default` 16px from a `sm` secondary button): a closed and locked thread
    /// (`O`) offers Open to whoever may open it again (`lK` of 406704: Manage Threads for a locked one), a locked one
    /// (`G`) offers Unlock with Manage Threads (`H_`). A post says "post"; a media post shows nothing.
    pub(super) fn thread_lock_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (guild, channel) = (self.guild?, self.channel?);
        let thread = self
            .stores
            .guild(guild)?
            .threads
            .iter()
            .find(|known| known.id == channel)?
            .clone();
        if !thread.locked {
            return None;
        }
        let parent = self
            .stores
            .guild(guild)?
            .channels
            .iter()
            .find(|known| known.id == thread.parent_id)
            .cloned();
        if parent.as_ref().is_some_and(dc_state::Channel::is_media) {
            return None;
        }
        let post = parent.as_ref().is_some_and(dc_state::Channel::is_forum);
        let manage = parent.as_ref().is_some_and(|parent| {
            self.my_permission(guild, Some(parent), dc_state::permissions::MANAGE_THREADS)
        });
        let (text, label, step) = match (thread.archived, post) {
            (true, true) => (
                "This post has been closed and locked. Only moderators can reopen it.",
                "Open",
                ThreadStep::Open,
            ),
            (true, false) => (
                "This thread has been closed and locked. Only moderators can reopen it.",
                "Open",
                ThreadStep::Open,
            ),
            (false, true) => (
                "This post has been locked. Only moderators can send messages.",
                "Unlock",
                ThreadStep::Unlock,
            ),
            (false, false) => (
                "This thread has been locked. Only moderators can send messages.",
                "Unlock",
                ThreadStep::Unlock,
            ),
        };
        let id = thread.id;
        Some(
            div()
                .flex_none()
                .px(px(16.))
                .py(px(8.))
                .bg(super::color("--background-mod-normal"))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(super::color("--text-default"))
                        .child(dc_intl::tr(text).to_owned()),
                )
                .children(manage.then(|| {
                    super::dm::small_button(
                        "thread-lock-notice-action",
                        dc_intl::tr(label),
                        super::dm::Tone::Secondary,
                        true,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.run_thread_step(id, step);
                        cx.notify();
                    }))
                }))
                .into_any_element(),
        )
    }

    /// "This channel is not on your channel list." (module 157259, shown by module 626977 when `228098` holds): the
    /// account browses only the channels it picked (`OPT_IN_CHANNELS_ON` of the server's settings) and neither the
    /// channel nor its category is picked (`OPT_IN_ENABLED` of their overrides); never for a thread. Module 859937's
    /// `chatHeaderBar` (`--background-surface-high`, a `--border-muted` rule under it, 8px 16px inside with 28px at the
    /// start), the line in `body-sm/normal` and a `sm` primary "Add to Channel List" with the plus icon.
    pub(super) fn opt_in_channel_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (guild, channel) = (self.guild?, self.channel?);
        let server = self.stores.guild(guild)?;
        let parent_id = server
            .channels
            .iter()
            .find(|known| known.id == channel)?
            .parent_id;
        // `jN` of module 395504: opt-in counts only in a Community server, or for staff.
        let community = server.features.iter().any(|feature| feature == "COMMUNITY")
            || self
                .stores
                .current_user
                .as_ref()
                .is_some_and(|user| user.has_flag(dc_state::USER_FLAG_STAFF));
        let settings = self.stores.guild_settings.guilds.get(&guild)?;
        let picked = |id: Option<Id>| {
            id.and_then(|id| settings.channels.get(&id))
                .is_some_and(|own| own.flags & OPT_IN_ENABLED != 0)
        };
        // GAP: the Server Guide resource-channel exemption (`IS_GUILD_RESOURCE_CHANNEL` behind `473529 d`) is not read.
        if !opt_in_notice_shown(
            community && settings.flags & OPT_IN_CHANNELS_ON != 0,
            picked(Some(channel)),
            picked(parent_id),
        ) {
            return None;
        }
        Some(
            div()
                .flex_none()
                .relative()
                .py(px(8.))
                .pr(px(16.))
                .pl(px(28.))
                .bg(super::color("--background-surface-high"))
                .border_b_1()
                .border_color(super::color("--border-muted"))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex_1()
                        .mr(px(16.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(super::color("--text-default"))
                        .child(dc_intl::t("iOWmmB")),
                )
                .child(
                    super::dm::small_button(
                        "opt-in-add-channel",
                        dc_intl::t("ZtIlQo"),
                        super::dm::Tone::Primary,
                        true,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.add_to_channel_list(guild, channel);
                        cx.notify();
                    })),
                )
                .into_any_element(),
        )
    }

    /// `AC(guild, channel, true)` of module 816662: the channel's override gains `OPT_IN_ENABLED`, at once and through
    /// `PATCH /users/@me/guilds/settings`.
    fn add_to_channel_list(&mut self, guild: Id, channel: Id) {
        let Some(settings) = self.stores.guild_settings.guilds.get_mut(&guild) else {
            return;
        };
        // A channel without an override inherits its notifications (`INHERIT`), as `from_value` reads one.
        let own = settings.channels.entry(channel).or_insert_with(|| {
            dc_state::guild_settings::Override {
                message_notifications: dc_state::guild_settings::INHERIT,
                ..Default::default()
            }
        });
        own.flags |= OPT_IN_ENABLED;
        let flags = own.flags;
        let _ = self.requests.send(Request::GuildSettings {
            body: json!({"guilds": {guild.to_string(): {"channel_overrides": {channel.to_string(): {"flags": flags}}}}}),
        });
    }

    /// Runs a context menu thread action; the bodies go one after the other.
    pub(super) fn run_thread_step(&mut self, thread: Id, step: ThreadStep) {
        let (archived, flags) = self.thread_state(thread).unwrap_or_default();
        let steps = thread_bodies(step, archived, flags)
            .into_iter()
            .map(|body| (thread, body))
            .collect();
        if matches!(step, ThreadStep::Pin | ThreadStep::Unpin) {
            // `updateFlags` dispatches the merged THREAD_UPDATE before its request.
            self.set_thread_flags(
                thread,
                if step == ThreadStep::Pin {
                    flags | PINNED
                } else {
                    flags & !PINNED
                },
            );
        }
        self.send_thread_chain(steps);
    }

    fn set_thread_flags(&mut self, thread: Id, flags: u64) {
        for guild in &mut self.stores.guilds {
            if let Some(known) = guild.threads.iter_mut().find(|known| known.id == thread) {
                known.flags = flags;
            }
        }
    }

    /// The forum's pinned post other than `post` (`Eh` of module 435470: the first thread of the parent with the
    /// PINNED flag).
    pub(super) fn pinned_post_of_parent(&self, post: Id) -> Option<Id> {
        let guild = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.threads.iter().any(|known| known.id == post))?;
        let parent = guild
            .threads
            .iter()
            .find(|known| known.id == post)?
            .parent_id;
        guild
            .threads
            .iter()
            .find(|known| known.parent_id == parent && known.id != post && known.pinned())
            .map(|known| known.id)
    }

    /// Pin Post (module 41402): with another post pinned already, "Pin this post?" asks first.
    pub(super) fn pin_post(&mut self, post: Id) {
        match self.pinned_post_of_parent(post) {
            Some(old) => {
                self.chat_extra.alert = Some(Alert::show(
                    "pin-post-alert",
                    dc_intl::tr("Pin this post?"),
                    Some(dc_intl::t!("mi5+Vl").to_owned()),
                    None,
                    None,
                    Then::ReplacePin { post, old },
                ));
            }
            None => self.run_thread_step(post, ThreadStep::Pin),
        }
    }

    fn send_thread_chain(&mut self, steps: VecDeque<(Id, Value)>) {
        let chain = self.chat_extra.next_chain;
        self.chat_extra.next_chain += 1;
        self.chat_extra.chains.insert(chain, steps);
        self.send_next_step(chain);
    }

    fn send_next_step(&mut self, chain: u64) {
        let Some(steps) = self.chat_extra.chains.get_mut(&chain) else {
            return;
        };
        let Some((thread, body)) = steps.pop_front() else {
            self.chat_extra.chains.remove(&chain);
            return;
        };
        let last = steps.is_empty();
        if last {
            self.chat_extra.chains.remove(&chain);
        }
        let _ = self.requests.send(Request::Rest {
            key: (!last).then(|| format!("{KEY_PREFIX}{chain}")),
            method: reqwest::Method::PATCH,
            route: format!("/channels/{thread}"),
            body: Some(body),
        });
    }

    /// A step's answer: the next one goes when it succeeded, else the chain stops.
    pub(super) fn thread_step_fetched(&mut self, key: &str, body: &Value) {
        let Some(chain) = key
            .strip_prefix(KEY_PREFIX)
            .and_then(|chain| chain.parse().ok())
        else {
            return;
        };
        if body["ok"].as_bool() == Some(true) {
            self.send_next_step(chain);
        } else {
            self.chat_extra.chains.remove(&chain);
        }
    }

    fn alert_then(&mut self, then: Then, window: &mut Window, cx: &mut Context<Self>) {
        let closed = self.chat_extra.alert.take();
        match then {
            Then::Close => self.alert_closed(closed),
            Then::SwitchToPushToTalk => {
                self.set_push_to_talk(true);
                self.clear_vad_warning();
                cx.notify();
            }
            Then::JumpToMessage { channel, message } => {
                self.jump_to_message_id(channel, message, true, None);
                cx.notify();
            }
            Then::ReplacePin { post, old } => {
                let (old_archived, old_flags) = self.thread_state(old).unwrap_or_default();
                let (archived, flags) = self.thread_state(post).unwrap_or_default();
                self.set_thread_flags(old, old_flags & !PINNED);
                self.set_thread_flags(post, flags | PINNED);
                let mut steps: VecDeque<(Id, Value)> =
                    thread_bodies(ThreadStep::Unpin, old_archived, old_flags)
                        .into_iter()
                        .map(|body| (old, body))
                        .collect();
                steps.extend(
                    thread_bodies(ThreadStep::Pin, archived, flags)
                        .into_iter()
                        .map(|body| (post, body)),
                );
                self.send_thread_chain(steps);
            }
            Then::RetryForward {
                message,
                destinations,
            } => self.retry_forward(*message, destinations, window, cx),
            Then::DmsOff { guild } => self.turn_off_guild_dms(guild, cx),
            Then::SyncActivity {
                restricting,
                guilds,
                activity,
            } => self.sync_activity(restricting, &guilds, activity, cx),
            Then::UploadLogs => self.voice_upload_logs(cx),
            Then::SaveSoundboardKeybind => self.soundboard_keybind_save(),
            Then::AcceptMessageRequest { channel } => self.mr_accept_now(channel, cx),
            Then::AcceptStranger { user } => self.accept_stranger(user, cx),
            Then::OpenArticle { url } => {
                self.chat_extra.alert = closed;
                self.follow_link(url, url, cx);
            }
            Then::BackToLogin => self.back_to_login(cx),
            Then::ScheduledInbox => self.open_inbox_scheduled(cx),
            Then::ReportRaid { guild } => {
                let channel = self
                    .stores
                    .guild(guild)
                    .and_then(|known| known.alerts_channel);
                if let Some(request) = raid_report_request(guild, channel) {
                    let _ = self.requests.send(request);
                }
                // GAP: "Raid report submitted" goes to the screen reader announcer, which is not built.
                tracing::debug!("raid report submitted");
            }
            Then::LockdownFeedback => {
                self.chat_extra.lockdown_other = None;
                tracing::debug!("lockdown feedback submitted");
            }
            Then::UseCommand { name, id } => self.use_command_mention(&name, id, window, cx),
            Then::WithdrawApplication { guild } => self.remove_join_request(guild),
            Then::Reapply { guild } => self.reapply(guild),
            Then::MuteAppDm { channel, muted } => self.mute_app_dm(channel, muted),
            Then::ClearOverwrite { channel, target } => {
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::DELETE,
                    route: format!("/channels/{channel}/permissions/{target}"),
                    body: None,
                });
                if let Some(settings) = self.channel_settings.as_mut() {
                    settings.perm_edits.remove(&target);
                    if settings.perm_target == Some(target) {
                        settings.perm_target = None;
                    }
                }
            }
            Then::ResolveRaid { guild, message } => {
                self.chat_extra.lockdown_other = None;
                let chosen = match closed.and_then(|alert| alert.content) {
                    Some(Content::RaidResolve { chosen }) => chosen,
                    _ => Vec::new(),
                };
                let reason = super::automod_alert::resolution_reason(&chosen);
                let _ = self
                    .requests
                    .send(super::automod_alert::false_alarm_request(
                        guild, message, reason,
                    ));
            }
            Then::LeaveGuild { guild } => {
                let _ = self.requests.send(super::guild_menu::leave_request(guild));
            }
            Then::DeauthorizeApp { application, token } => {
                self.deauthorize_app(&application, &token, cx);
            }
        }
    }

    /// A dialog closed without its confirming button: the Soundboard Keybind modal puts the keybind back (its
    /// `useState` copy is simply dropped in the official client).
    pub(super) fn alert_closed(&mut self, closed: Option<Alert>) {
        // Don't switch of "Push to Talk Required" (and Escape): `clearVADWarning`.
        if closed
            .as_ref()
            .is_some_and(|alert| alert.id == super::vad_permission::WARNING_ID)
        {
            self.clear_vad_warning();
        }
        if closed
            .as_ref()
            .is_some_and(|alert| alert.id == super::birthday::UNDERAGE_ID)
        {
            self.back_to_login_later();
        }
        match closed.and_then(|alert| alert.content) {
            Some(Content::SoundboardKeybind { original }) => {
                self.soundboard_keybind_revert(original);
            }
            Some(Content::LockdownFeedback { .. } | Content::RaidResolve { .. }) => {
                self.chat_extra.lockdown_other = None
            }
            _ => {}
        }
    }

    /// Report Raid of the server header menu (module 808144): the dialog of module 935074.
    pub(super) fn open_report_raid(&mut self, guild: Id, cx: &mut Context<Self>) {
        self.chat_extra.alert = Some(raid_report_alert(guild));
        cx.notify();
    }

    /// Module 336496 after a Security Actions save that lifted the pauses, with its "Other" text box (module 103557,
    /// `placeholder` "Tell us what happened", `autoFocus` once it shows).
    pub(super) fn open_lockdown_feedback(&mut self, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut input = crate::editor::Editor::new(super::editor_style(), cx);
            input.set_keeps_text(true);
            input.set_placeholder(dc_intl::t("PAM+JR").to_owned());
            input
        });
        self.chat_extra.lockdown_other = Some(input);
        self.chat_extra.alert = Some(lockdown_feedback_alert());
        cx.notify();
    }

    /// Opens Channel Topic (module 181094) for a server channel that has a topic.
    pub(super) fn open_channel_topic(&mut self, channel: Id, cx: &mut Context<Self>) {
        self.chat_extra.alert = Some(channel_topic_alert(channel));
        cx.notify();
    }

    fn topic_channel(&self, channel: Id) -> Option<(&dc_state::Channel, bool)> {
        let guild = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == channel))?;
        let known = guild.channels.iter().find(|known| known.id == channel)?;
        Some((known, guild.rules_channel_id == Some(channel)))
    }

    /// The subtitle of module 181094: `{text: channel name, leadingIcon: gU(channel, guild)}` (module 20742 `O`).
    fn channel_topic_subtitle(&self, channel: Id) -> Option<AnyElement> {
        let (known, rules) = self.topic_channel(channel)?;
        let name = super::channel_icon::name(&super::channel_icon::Facts {
            locked: false,
            ..self.channel_icon_facts(known, rules)
        });
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .flex_none()
                        .child(super::icon(name, 16., "--text-subtle")),
                )
                .child(div().min_w(px(0.)).child(known.name.clone()))
                .into_any_element(),
        )
    }

    /// The topic of module 181094: `parseTopic` (lines kept) in `body-md/normal` (16/20) `text-default`.
    fn channel_topic_body(&self, channel: Id, cx: &mut Context<Self>) -> AnyElement {
        let topic = self
            .topic_channel(channel)
            .and_then(|(known, _)| known.topic.clone())
            .unwrap_or_default();
        div()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(super::color("--text-default"))
            .child(self.topic_text_full(&topic, super::color("--text-default").into(), cx))
            .into_any_element()
    }

    /// Escape closes the open alert; true when there was one.
    pub(super) fn chat_alert_escape(&mut self) -> bool {
        let closed = self.chat_extra.alert.take();
        let was = closed.is_some();
        self.alert_closed(closed);
        was
    }

    pub(super) fn chat_alert_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let alert = self.chat_extra.alert.clone()?;
        let blocked = alert.content.as_ref().is_some_and(Content::blocks_submit);
        let buttons = alert
            .buttons
            .iter()
            .cloned()
            .map(|button| {
                let then = button.then.clone();
                let tone = if blocked && matches!(button.tone, Tone::Primary) {
                    Tone::PrimaryDisabled
                } else {
                    button.tone
                };
                // A disabled button's click does nothing (modal.rs), so the dialog stays open.
                modal::button(button.id, button.label, tone, move |this, window, cx| {
                    this.alert_then(then.clone(), window, cx)
                })
            })
            .collect();
        let medium = alert.medium();
        let alert_id = alert.id;
        let subtitle = if matches!(alert.content, Some(Content::RaidReport { .. })) {
            Some(raid_subtitle(cx))
        } else if let Some(Content::ChannelTopic(channel)) = alert.content {
            self.channel_topic_subtitle(channel)
        } else {
            alert.subtitle.map(rich_text)
        };
        let spec = modal::Spec {
            id: alert.id,
            title: alert.title.into(),
            subtitle,
            body: alert.content.map(|content| self.alert_content(content, cx)),
            buttons,
            close: Box::new(|this, _, _| {
                let closed = this.chat_extra.alert.take();
                this.alert_closed(closed);
            }),
        };
        Some(if alert_id == "report-app" {
            // Opened from the bot profile modal's menu: above the modal that stays open under it.
            modal::render_over_layers(spec, cx)
        } else if medium {
            modal::render(spec, cx)
        } else {
            modal::render_small(spec, cx)
        })
    }

    fn alert_content(&self, content: Content, cx: &mut Context<Self>) -> AnyElement {
        let raid_resolve = matches!(content, Content::RaidResolve { .. });
        match content {
            Content::Text(text) => body_text(text),
            Content::ChannelTopic(channel) => self.channel_topic_body(channel, cx),
            Content::SoundboardKeybind { .. } => self.soundboard_keybind_body(cx),
            Content::FailedDestinations(destinations) => {
                // `failedDestinations_PiyhYP`: a `--border-subtle` edge, 8px corners, 16px above, 8px 16px inside;
                // `failedRow_PiyhYP` 48px rows with the 32px picture 12px from the name (`body-md/medium`,
                // `--text-strong`, one line).
                div()
                    .mt(px(16.))
                    .py(px(8.))
                    .px(px(16.))
                    .border_1()
                    .border_color(super::color("--border-subtle"))
                    .rounded(px(8.))
                    .children(destinations.into_iter().map(|destination| {
                        let (icon, name, _) = self.destination_parts(destination, cx);
                        div()
                            .h(px(48.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(icon)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(super::color("--text-strong"))
                                    .child(name),
                            )
                    }))
                    .into_any_element()
            }
            Content::AffectedGuild(guild) => self.affected_guild(guild),
            Content::AffectedGuilds {
                guilds,
                restricting,
                clickable,
            } => self.affected_guilds_card(&guilds, restricting, clickable, cx),
            Content::RejectionReason(reason) => div()
                .flex()
                .flex_wrap()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(super::color("--text-default"))
                .child(div().mr(px(4.)).child(dc_intl::t("cf1psW")))
                .child(reason)
                .into_any_element(),
            Content::EmojiUploadErrors(errors) => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(errors.iter().enumerate().map(|(index, (kind, filename))| {
                    self.emoji_upload_error_row(index, *kind, filename, cx)
                }))
                .into_any_element(),
            // `group_Si_H15`: a column 16px apart with 8px above and 4px below; each checkbox of 568385 has its label
            // (`body-md/normal`, `text-strong`) 12px after the box.
            Content::RaidReport { chosen, .. } => div()
                .pt(px(8.))
                .pb(px(4.))
                .flex()
                .flex_col()
                .gap(px(16.))
                .children(RAID_KINDS.iter().enumerate().map(|(index, (kind, label))| {
                    let checked = chosen.contains(&index);
                    div()
                        .id(SharedString::from(format!("report-raid-{kind}")))
                        .flex()
                        .items_start()
                        .gap(px(12.))
                        .cursor_pointer()
                        .child(super::server_onboarding::checkbox(checked, false))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(super::color("--text-strong"))
                                .child(dc_intl::t(label)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(Alert {
                                content: Some(Content::RaidReport { chosen, .. }),
                                ..
                            }) = this.chat_extra.alert.as_mut()
                            {
                                toggle_choice(chosen, index);
                            }
                            cx.notify();
                        }))
                }))
                .into_any_element(),
            // Module 336496: a stack 8px apart (`B` gap 8) of the checkbox group (module 167417, drawn as Report a
            // Raid's) and, while Other is chosen, `textboxContainer_p32tDk` (8px above, 32px in from the start) with
            // the text area of module 103557: three rows (`rows: 3`, 20px lines) inside `padding: 12px 10px`.
            // `B` (8px apart) of two `clickable_zgsiD2` rows: `--background-mod-subtle` (`--background-mod-strong`
            // hovered), 16px inside, `--radius-sm` corners, the title (`body-md/semibold`, `text-strong`) over the
            // description (`body-md/medium`, `text-subtle`) 4px apart, the small chevron at the end. The first reports
            // the app's profile (928658 `NW`), the second the app (928658 `r3`, entrypoint `user_profile`).
            Content::ReportApp { user, app } => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(
                    [("eyEkG1", "ptItsj", false), ("atP0yX", "UGg603", true)]
                        .into_iter()
                        .map(|(title, description, behaviour)| {
                            div()
                                .id(SharedString::from(format!("report-app-{title}")))
                                .p(px(16.))
                                .rounded(px(8.))
                                .bg(super::color("--background-mod-subtle"))
                                .hover(|style| style.bg(super::color("--background-mod-strong")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(16.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(
                                            div()
                                                .text_size(px(16.))
                                                .line_height(px(20.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(super::color("--text-strong"))
                                                .child(dc_intl::t(title)),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(16.))
                                                .line_height(px(20.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(super::color("--text-subtle"))
                                                .child(dc_intl::t(description)),
                                        ),
                                )
                                .child(
                                    gpui::svg()
                                        .path("icons/bot_chevron_small_right.svg")
                                        .size(px(24.))
                                        .flex_none()
                                        .text_color(super::color("--icon-default")),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    // GAP: the official keeps the chooser under the report until it is sent.
                                    this.chat_extra.alert = None;
                                    if behaviour {
                                        this.open_report_app(app, "user_profile", cx);
                                    } else {
                                        this.open_report_user(user, window, cx);
                                    }
                                    cx.notify();
                                }))
                        }),
                )
                .into_any_element(),
            Content::LockdownFeedback { chosen } | Content::RaidResolve { chosen } => {
                // Module 580812 draws the same group (module 167417) and text box (module 103557) as module 336496.
                let (reasons, other_index, prefix): (&[(&str, &str)], usize, &str) = if raid_resolve
                {
                    (
                        &super::automod_alert::RESOLUTIONS,
                        super::automod_alert::RESOLUTION_OTHER,
                        "raid-resolve",
                    )
                } else {
                    (&LOCKDOWN_REASONS, LOCKDOWN_OTHER, "lockdown-feedback")
                };
                let other = chosen.contains(&other_index);
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .pt(px(8.))
                            .pb(px(4.))
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .children(reasons.iter().enumerate().map(|(index, (kind, label))| {
                                let checked = chosen.contains(&index);
                                div()
                                    .id(SharedString::from(format!("{prefix}-{kind}")))
                                    .flex()
                                    .items_start()
                                    .gap(px(12.))
                                    .cursor_pointer()
                                    .child(super::server_onboarding::checkbox(checked, false))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .text_color(super::color("--text-strong"))
                                            .child(dc_intl::t(label)),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(Alert {
                                            content:
                                                Some(
                                                    Content::LockdownFeedback { chosen }
                                                    | Content::RaidResolve { chosen },
                                                ),
                                            ..
                                        }) = this.chat_extra.alert.as_mut()
                                        {
                                            toggle_choice(chosen, index);
                                            if index == other_index
                                                && chosen.contains(&other_index)
                                                && let Some(input) =
                                                    this.chat_extra.lockdown_other.as_ref()
                                            {
                                                let focus = input.read(cx).focus_handle(cx);
                                                window.focus(&focus, cx);
                                            }
                                        }
                                        cx.notify();
                                    }))
                            })),
                    )
                    .when(other, |stack| {
                        stack.children(self.chat_extra.lockdown_other.clone().map(|input| {
                            div().mt(px(8.)).pl(px(32.)).w_full().child(
                                div()
                                    .id(SharedString::from(format!("{prefix}-other")))
                                    .w_full()
                                    .px(px(10.))
                                    .py(px(12.))
                                    .min_h(px(3. * 20. + 24.))
                                    .rounded(px(RADIUS_SM))
                                    .border_1()
                                    .border_color(super::color("--input-border-default"))
                                    .bg(super::color("--input-background-default"))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .child(input),
                            )
                        }))
                    })
                    .into_any_element()
            }
        }
    }
}

/// `dmSettingsUpsellLastShown` of module 959702: the modal shows at most once a week.
const DM_UPSELL_KEY: &str = "dmSettingsUpsellLastShown";
const DM_UPSELL_COOLDOWN_MS: i64 = 604_800_000;

/// `openDmSettingsUpsellModal`: shown when it never was, or more than a week ago.
pub(super) fn dm_upsell_due(last_shown_ms: Option<i64>, now_ms: i64) -> bool {
    last_shown_ms.is_none_or(|last| now_ms - last > DM_UPSELL_COOLDOWN_MS)
}

/// The action handler of module 8619 for `AUTO_MODERATION_CONTENT_DELETED`: the dialog of module 130577, or the toast
/// when the message stays in its loaded channel with the notice.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum AutomodDeleted {
    Dialog,
    /// "Your message was deleted in #channel", for a channel other than the open one.
    Toast,
    /// The open channel shows the message with its notice.
    Nothing,
}

pub(super) fn automod_deleted(
    has_thread: bool,
    has_message: bool,
    has_notice: bool,
    in_open_channel: bool,
) -> AutomodDeleted {
    if has_thread || !has_message || !has_notice {
        AutomodDeleted::Dialog
    } else if in_open_channel {
        AutomodDeleted::Nothing
    } else {
        AutomodDeleted::Toast
    }
}

impl Root {
    /// `AUTO_MODERATION_CONTENT_DELETED {guild_id, channel_id, message, thread, notice}` after the stores took it.
    pub(super) fn automod_content_deleted(&mut self, data: &Value, cx: &mut Context<Self>) {
        let message = dc_state::Message::from_value(&data["message"]);
        let has_notice = message
            .as_ref()
            .is_some_and(|message| self.stores.automod_removal_notice(message.id).is_some());
        let channel = message.as_ref().map(|message| message.channel_id);
        match automod_deleted(
            !data["thread"].is_null(),
            message.is_some(),
            has_notice,
            channel.is_some() && channel == self.channel,
        ) {
            AutomodDeleted::Nothing => {}
            AutomodDeleted::Toast => {
                let name = channel.and_then(|channel| {
                    self.stores
                        .guilds
                        .iter()
                        .flat_map(|guild| guild.channels.iter())
                        .find(|known| known.id == channel)
                        .map(|known| known.name.clone())
                });
                // No name, no toast (`null != name`).
                if let Some(name) = name {
                    let text = dc_intl::t!("9U3Wb3", channel = format!("#{name}"));
                    self.put_icon_toast(
                        text,
                        ("icons/guild_menu_privacy.svg", "--icon-feedback-critical"),
                        cx,
                    );
                }
            }
            AutomodDeleted::Dialog => {
                // Module 130577: "Your post was removed" with the post's name under it for a thread, else "Your
                // message was removed"; the notice under the title.
                let thread = &data["thread"];
                let notice = data["notice"].as_str().unwrap_or_default().to_owned();
                self.chat_extra.alert = Some(Alert {
                    id: "automod-removed",
                    title: if thread.is_null() {
                        dc_intl::tr("Your message was removed").to_owned()
                    } else {
                        dc_intl::tr("Your post was removed").to_owned()
                    },
                    subtitle: thread["name"]
                        .as_str()
                        .map(|name| vec![(name.to_owned(), false)]),
                    content: Some(Content::Text(
                        message
                            .as_ref()
                            .map_or(notice, |message| message.content.clone()),
                    )),
                    buttons: vec![AlertButton {
                        id: "automod-removed-okay",
                        label: dc_intl::tr("Okay").to_owned(),
                        tone: Tone::Primary,
                        then: Then::Close,
                    }],
                });
                cx.notify();
            }
        }
    }

    /// `DM_SETTINGS_UPSELL_SHOW {guild_id}` (module 959702), after the account turned down message requests from the
    /// server: once a week, the "Turn off direct messages from this server?" dialog (module 228855), which tells the
    /// service it was seen (`POST /users/@me/guilds/{guild}/member/ack-dm-upsell-settings`).
    pub(super) fn dm_settings_upsell(&mut self, data: &Value, cx: &mut Context<Self>) {
        let Some(guild) = data["guild_id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok())
        else {
            return;
        };
        let now = chrono::Utc::now().timestamp_millis();
        let last = crate::storage::get(DM_UPSELL_KEY).and_then(|value| value.as_i64());
        if !dm_upsell_due(last, now) {
            return;
        }
        let Some(name) = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .map(|known| known.name.clone())
        else {
            return;
        };
        crate::storage::set(DM_UPSELL_KEY, json!(now));
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: format!("/users/@me/guilds/{guild}/member/ack-dm-upsell-settings"),
            body: None,
        });
        let subtitle = dc_intl::format_runs("Depjkv", &[("guild_name", name.into())])
            .into_iter()
            .map(|(text, tag)| (text, tag.as_deref() == Some("$b")))
            .collect();
        self.chat_extra.alert = Some(Alert {
            id: "dm-settings-upsell",
            title: dc_intl::tr("Turn off direct messages from this server?").to_owned(),
            subtitle: Some(subtitle),
            content: Some(Content::AffectedGuild(guild)),
            buttons: vec![
                AlertButton {
                    id: "dm-upsell-keep",
                    label: dc_intl::tr("No, keep them on").to_owned(),
                    tone: Tone::Secondary,
                    then: Then::Close,
                },
                AlertButton {
                    id: "dm-upsell-off",
                    label: dc_intl::tr("Yes, turn them off").to_owned(),
                    tone: Tone::Primary,
                    then: Then::DmsOff { guild },
                },
            ],
        });
        cx.notify();
    }

    /// "Yes, turn them off": the server joins the account's `restricted_guilds`, then "Settings changed".
    fn turn_off_guild_dms(&mut self, guild: Id, cx: &mut Context<Self>) {
        let mut ids = self
            .stores
            .proto_ids(&dc_proto::settings::DM_RESTRICTED_GUILDS);
        if !ids.contains(&guild) {
            ids.push(guild);
        }
        self.set_ids(&dc_proto::settings::DM_RESTRICTED_GUILDS, &ids);
        self.show_success_toast(dc_intl::tr("Settings changed").to_owned(), cx);
    }

    /// The server header's invite button (module 182179): `K` of module 855687 (Create Invite in the server, or a
    /// custom invite link) opens Invite to Server; anyone else sees "Invite permission required" (module 132610),
    /// its subtitle and Got It.
    pub(super) fn header_invite(&mut self, cx: &mut Context<Self>) {
        let Some(guild) = self.guild else { return };
        let vanity = self
            .stores
            .guild(guild)
            .is_some_and(|known| known.vanity_url_code.is_some());
        if vanity || self.my_permission(guild, None, dc_state::permissions::CREATE_INSTANT_INVITE) {
            self.open_invite(cx);
            return;
        }
        self.script_invite_permission(cx);
    }

    /// The header invite's refusal; the harness shows it for an account that may invite.
    pub(crate) fn script_invite_permission(&mut self, cx: &mut Context<Self>) {
        self.chat_extra.alert = Some(invite_permission_alert());
        cx.notify();
    }

    /// Limited access (module 218642, opened by module 700241 from the request interceptor of module 596339 for a
    /// 4xx answer whose code is 340000 to 349999): title, the explanation, Okay.
    pub(super) fn limited_access_alert(&mut self, cx: &mut Context<Self>) {
        self.chat_extra.alert = Some(Alert::show(
            "limited-access",
            dc_intl::tr("Your account has limited access"),
            Some(dc_intl::t!("zNPBMA").to_owned()),
            None,
            None,
            Then::Close,
        ));
        cx.notify();
    }

    /// Module 228855's children: "This change affects" (`body-md/medium`, `text-muted`), 8px above the server's box
    /// (`guildContainer_wWIOj0`: `--background-mod-subtle`, a `--border-subtle` edge, `--radius-sm` corners, 16px
    /// inside) with its 30px icon (`SMALL`) 12px from its name (`body-md/semibold`).
    /// Module 929120 (`card_k0Unn4`): on `--background-mod-subtle` with `--radius-md`, 16px in, the count of servers
    /// (body-md/semibold) over "Sharing" (`text-feedback-positive`) or "Not Sharing" and "• {first} and others" in
    /// body-sm/medium, the icon stack of module 926418 at the end (with a chevron when it opens Activity Privacy).
    fn affected_guilds_card(
        &self,
        guilds: &[Id],
        restricting: bool,
        clickable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // GAP: `AB` of module 365258 orders the servers by frecency, which is not kept; they stay newest joined first.
        let first = guilds
            .first()
            .and_then(|guild| self.stores.guild(*guild))
            .map(|guild| guild.name.clone())
            .unwrap_or_default();
        let summary = dc_intl::format(
            if guilds.len() > 1 { "8ZLbvR" } else { "+NoTYm" },
            &[("guildName", first.into())],
        );
        // The stack lies on the card's `--background-mod-subtle` over the dialog's surface.
        let under = super::modal::over(
            super::color("--background-mod-subtle"),
            super::color("--background-surface-high"),
        );
        let stack = self.guild_stack(guilds, under, cx);
        div()
            .id("activity-sync-card")
            .flex()
            .items_center()
            .gap(px(12.))
            .p(px(16.))
            .rounded(px(12.))
            .bg(super::color("--background-mod-subtle"))
            .when(clickable, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        // `L7` of module 481045: Activity Privacy.
                        this.chat_extra.alert = None;
                        if let Some(open) = this.settings.as_mut() {
                            *open =
                                super::settings::Open::new(super::settings::Page::ActivityPrivacy);
                        }
                        cx.notify();
                    }))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(super::color("--text-strong"))
                            .child(dc_intl::format(
                                "0fkj8J",
                                &[("count", (guilds.len() as i64).into())],
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(super::color(if restricting {
                                        "--text-feedback-positive"
                                    } else {
                                        "--text-muted"
                                    }))
                                    .child(dc_intl::t(if restricting {
                                        "e6Kpa7"
                                    } else {
                                        "cy4G4y"
                                    })),
                            )
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_color(super::color("--text-muted"))
                                    .child(summary),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .children(stack)
                    .when(clickable, |this| {
                        this.child(super::icon("settings_chevron", 12., "--text-subtle"))
                    }),
            )
            .into_any_element()
    }

    fn affected_guild(&self, guild: Id) -> AnyElement {
        let known = self.stores.guilds.iter().find(|known| known.id == guild);
        let name = known.map(|known| known.name.clone()).unwrap_or_default();
        let acronym = known.map(dc_state::Guild::acronym).unwrap_or_default();
        let picture = self.images.get(&format!("guild:{guild}")).cloned();
        // `R[SMALL]` of module 682163: the acronym's size by its length.
        let sizes = [14., 14., 12., 12., 10., 8., 6.];
        let acronym_size = sizes[acronym.chars().count().saturating_sub(1).min(6)];
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(super::color("--text-muted"))
                    .child(dc_intl::tr("This change affects").to_owned()),
            )
            .child(
                div()
                    .p(px(16.))
                    .rounded(px(RADIUS_SM))
                    .border_1()
                    .border_color(super::color("--border-subtle"))
                    .bg(super::color("--background-mod-subtle"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .size(px(30.))
                                    .flex_none()
                                    .rounded_full()
                                    .overflow_hidden()
                                    .bg(super::color("--background-base-lower"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(acronym_size))
                                    .text_color(super::color("--text-default"))
                                    .when(picture.is_none(), |this| this.child(acronym))
                                    .when_some(picture, |this, picture| {
                                        this.child(gpui::img(picture).size(px(30.)))
                                    }),
                            )
                            .child(
                                div()
                                    .ml(px(12.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(super::color("--text-default"))
                                    .child(name),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// Runs of text, the bold ones at weight 700 (`$b`, a `strong`).
/// A checkbox of a group: chosen when it was not, else no longer.
fn toggle_choice(chosen: &mut Vec<usize>, index: usize) {
    match chosen.iter().position(|known| *known == index) {
        Some(at) => {
            chosen.remove(at);
        }
        None => chosen.push(index),
    }
}

/// `Hg8Ee7` with its `$link` "Learn more about raids" in `--text-link`, which opens the raids article.
fn raid_subtitle(cx: &mut Context<Root>) -> AnyElement {
    let mut text = String::new();
    let mut links = Vec::new();
    let mut highlights = Vec::new();
    for (run, tag) in dc_intl::format_runs("Hg8Ee7", &[]) {
        let start = text.len();
        text.push_str(&run);
        if tag.as_deref() == Some("$link") {
            links.push(start..text.len());
            highlights.push((
                start..text.len(),
                gpui::HighlightStyle {
                    color: Some(super::color("--text-link").into()),
                    ..Default::default()
                },
            ));
        }
    }
    let open = cx.listener(|this, _: &usize, _, cx| this.follow_link(RAID_ARTICLE, "", cx));
    div()
        .child(
            gpui::InteractiveText::new(
                "report-raid-learn-more",
                gpui::StyledText::new(text).with_highlights(highlights),
            )
            .on_click(links, move |index, window, cx| open(&index, window, cx)),
        )
        .into_any_element()
}

fn rich_text(runs: Vec<(String, bool)>) -> AnyElement {
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (run, bold) in runs {
        let start = text.len();
        text.push_str(&run);
        if bold {
            highlights.push((
                start..text.len(),
                gpui::HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
            ));
        }
    }
    div()
        .child(gpui::StyledText::new(text).with_highlights(highlights))
        .into_any_element()
}

/// The address Copy Link of a channel mention copies (module 147036 `af` with no thread: `O(guild, channel, message)`,
/// the `CHANNEL` route on the app's host; a DM is `@me`).
pub(super) fn channel_link(guild: Option<Id>, channel: Id, message: Option<Id>) -> String {
    let guild = guild.map_or_else(|| "@me".to_owned(), |guild| guild.to_string());
    match message {
        Some(message) => format!("https://discord.com/channels/{guild}/{channel}/{message}"),
        None => format!("https://discord.com/channels/{guild}/{channel}"),
    }
}

/// The body text of a dialog (`body-md/normal`, `text-default`).
pub(super) fn body_text(text: impl Into<gpui::SharedString>) -> AnyElement {
    div()
        .text_size(px(16.))
        .line_height(px(20.))
        .text_color(super::color("--text-default"))
        .child(text.into())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Module 336496: six reasons, Submit held back until one is chosen.
    #[test]
    fn the_lockdown_feedback_waits_for_a_reason() {
        let alert = lockdown_feedback_alert();
        assert_eq!(alert.title, "Why was this server locked?");
        let labels: Vec<_> = LOCKDOWN_REASONS
            .iter()
            .map(|(_, key)| dc_intl::t(key))
            .collect();
        assert_eq!(labels[0], "DM Spam");
        assert_eq!(labels[5], "Other");
        assert_eq!(dc_intl::t("PAM+JR"), "Tell us what happened");
        assert!(alert.content.as_ref().unwrap().blocks_submit());
        assert!(!Content::LockdownFeedback { chosen: vec![3] }.blocks_submit());
        assert!(!Content::RaidReport { chosen: vec![] }.blocks_submit());
        assert_eq!(alert.buttons[1].label, "Submit");
    }

    /// Module 935074 and `B` of 859706: five behaviours to tick, and the report goes only to a server with an alerts
    /// channel.
    #[test]
    fn a_raid_report_ticks_behaviours_and_posts_only_with_an_alerts_channel() {
        let alert = raid_report_alert(7);
        assert_eq!(alert.title, "Report a Raid");
        assert!(alert.medium());
        assert_eq!(alert.buttons[1].label, "Submit");
        let mut chosen = Vec::new();
        toggle_choice(&mut chosen, 1);
        toggle_choice(&mut chosen, 3);
        toggle_choice(&mut chosen, 1);
        assert_eq!(chosen, [3]);
        assert_eq!(
            dc_intl::t(RAID_KINDS[3].1),
            "Nothing yet, but suspicious new members"
        );
        assert!(raid_report_request(7, None).is_none());
        match raid_report_request(7, Some(9)) {
            Some(Request::Rest {
                method,
                route,
                body,
                ..
            }) => {
                assert_eq!(method, reqwest::Method::POST);
                assert_eq!(route, "/guilds/7/auto-moderation/report-raid");
                assert!(body.is_none());
            }
            _ => panic!("no report request"),
        }
    }

    #[test]
    fn the_channel_list_bar_shows_for_an_unpicked_channel_of_an_opt_in_server() {
        assert!(opt_in_notice_shown(true, false, false));
        assert!(!opt_in_notice_shown(true, true, false));
        assert!(!opt_in_notice_shown(true, false, true));
        assert!(!opt_in_notice_shown(false, false, false));
        let alert = invite_permission_alert();
        assert_eq!(alert.title, "Invite permission required");
        assert_eq!(alert.buttons[0].label, "Got It");
    }

    #[test]
    fn locking_an_archived_thread_unarchives_it_first() {
        assert_eq!(
            thread_bodies(ThreadStep::Lock, true, 0),
            vec![
                json!({"archived": false}),
                json!({"locked": true, "archived": true})
            ]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Lock, false, 0),
            vec![json!({"locked": true, "archived": false})]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Unlock, true, 0),
            vec![
                json!({"archived": false, "locked": false}),
                json!({"locked": false, "archived": true})
            ]
        );
    }

    #[test]
    fn pinning_sets_the_flag_and_opens_an_archived_post() {
        assert_eq!(
            thread_bodies(ThreadStep::Pin, false, 1 << 4),
            vec![json!({"flags": (1 << 4) | 2})]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Pin, true, 0),
            vec![json!({"flags": 2, "archived": false})]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Unpin, true, 2 | 16),
            vec![json!({"flags": 16})]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Close, false, 0),
            vec![json!({"archived": true})]
        );
        assert_eq!(
            thread_bodies(ThreadStep::Open, true, 0),
            vec![json!({"archived": false})]
        );
    }

    #[test]
    fn a_channel_mention_link_names_its_server_or_me() {
        assert_eq!(
            channel_link(Some(1), 2, None),
            "https://discord.com/channels/1/2"
        );
        assert_eq!(
            channel_link(None, 2, Some(3)),
            "https://discord.com/channels/@me/2/3"
        );
    }

    #[test]
    fn the_dm_upsell_waits_a_week_between_showings() {
        assert!(dm_upsell_due(None, 0));
        assert!(!dm_upsell_due(Some(1_000), 1_000 + 604_800_000));
        assert!(dm_upsell_due(Some(1_000), 1_001 + 604_800_000));
    }

    #[test]
    fn automod_deletes_show_the_dialog_unless_the_message_keeps_its_notice() {
        use AutomodDeleted::*;
        // A thread, a missing message or one with no notice (its channel was not loaded): the dialog.
        assert_eq!(automod_deleted(true, true, true, false), Dialog);
        assert_eq!(automod_deleted(false, false, false, false), Dialog);
        assert_eq!(automod_deleted(false, true, false, true), Dialog);
        // The message stays with its notice: a toast unless its channel is the open one.
        assert_eq!(automod_deleted(false, true, true, false), Toast);
        assert_eq!(automod_deleted(false, true, true, true), Nothing);
    }

    #[test]
    fn forward_failed_counts_its_locations() {
        let message =
            dc_state::Message::from_value(&json!({"id": "5", "channel_id": "6", "content": "x",
            "timestamp": "2026-11-17T21:00:00+00:00", "author": {"id": "9", "username": "a"}}))
            .unwrap();
        let one = forward_failed_alert(message.clone(), vec![Destination::Channel(1)]);
        assert_eq!(one.title, "Forward Failed");
        assert_eq!(
            one.subtitle.unwrap()[0].0,
            "The message could not be forwarded to the following location:"
        );
        let two = forward_failed_alert(message, vec![Destination::Channel(1), Destination::Dm(2)]);
        assert_eq!(
            two.subtitle.clone().unwrap()[0].0,
            "The message could not be forwarded to the following locations:"
        );
        assert!(two.medium());
        assert_eq!(two.buttons[1].label, "Retry");
    }

    #[test]
    fn the_alert_has_okay_unless_named_and_cancel_first() {
        let alert = Alert::show("a", "T", None, None, None, Then::Close);
        assert_eq!(alert.buttons.len(), 1);
        assert_eq!(alert.buttons[0].label, "Okay");
        let alert = Alert::show(
            "a",
            "T",
            None,
            Some("Cancel".into()),
            Some("Go".into()),
            Then::Close,
        );
        assert_eq!(alert.buttons[0].tone, Tone::Secondary);
        assert_eq!(alert.buttons[1].label, "Go");
    }
}
