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
    AnyElement, Context, FontWeight, IntoElement, ParentElement, Styled, Window, div, prelude::*,
    px,
};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};

/// The key of a step's answer: `thread-step:{chain}`.
pub(super) const KEY_PREFIX: &str = "thread-step:";

/// `--radius-sm` of the design system (module 952007): 8px.
const RADIUS_SM: f32 = 8.;

/// `ChannelFlags.PINNED` (module 746080 `lx`).
pub(super) const PINNED: u64 = 1 << 1;

/// What the alert's buttons do besides closing it.
#[derive(Clone, Debug)]
pub(super) enum Then {
    Close,
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
}

#[derive(Clone, Debug)]
pub(super) struct AlertButton {
    pub id: &'static str,
    pub label: String,
    pub secondary: bool,
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
    /// Module 940372: the files of a bulk emoji upload that failed, a row each (`errors_qy2FFG`, 8px apart).
    EmojiUploadErrors(Vec<(super::server_emoji::UploadError, String)>),
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
            secondary: true,
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
                secondary: true,
                then: Then::Close,
            });
        }
        buttons.push(AlertButton {
            id: "alert-confirm",
            label: confirm.unwrap_or_else(|| dc_intl::tr("Okay").to_owned()),
            secondary: false,
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
        self.content.is_some()
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
                secondary: true,
                then: Then::Close,
            },
            AlertButton {
                id: "forward-failed-retry",
                label: dc_intl::tr("Retry").to_owned(),
                secondary: false,
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
    pub forward_batch: Option<ForwardBatch>,
    /// A right press on a `#channel` mention: where, and the channel (read by the message menu's opener).
    pub mention_hit: Option<(gpui::Point<gpui::Pixels>, Id)>,
    /// The requests still to go of each chain, by chain number.
    chains: HashMap<u64, VecDeque<(Id, Value)>>,
    next_chain: u64,
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
        self.chat_extra.alert = None;
        match then {
            Then::Close => {}
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
        }
    }

    /// Escape closes the open alert; true when there was one.
    pub(super) fn chat_alert_escape(&mut self) -> bool {
        self.chat_extra.alert.take().is_some()
    }

    pub(super) fn chat_alert_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let alert = self.chat_extra.alert.clone()?;
        let buttons = alert
            .buttons
            .iter()
            .cloned()
            .map(|button| {
                let then = button.then.clone();
                modal::button(
                    button.id,
                    button.label,
                    if button.secondary {
                        Tone::Secondary
                    } else {
                        Tone::Primary
                    },
                    move |this, window, cx| this.alert_then(then.clone(), window, cx),
                )
            })
            .collect();
        let medium = alert.medium();
        let spec = modal::Spec {
            id: alert.id,
            title: alert.title.into(),
            subtitle: alert.subtitle.map(rich_text),
            body: alert.content.map(|content| self.alert_content(content, cx)),
            buttons,
            close: Box::new(|this, _, _| this.chat_extra.alert = None),
        };
        Some(if medium {
            modal::render(spec, cx)
        } else {
            modal::render_small(spec, cx)
        })
    }

    fn alert_content(&self, content: Content, cx: &mut Context<Self>) -> AnyElement {
        match content {
            Content::Text(text) => body_text(text),
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
            Content::EmojiUploadErrors(errors) => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(errors.iter().enumerate().map(|(index, (kind, filename))| {
                    self.emoji_upload_error_row(index, *kind, filename, cx)
                }))
                .into_any_element(),
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
                        secondary: false,
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
                    secondary: true,
                    then: Then::Close,
                },
                AlertButton {
                    id: "dm-upsell-off",
                    label: dc_intl::tr("Yes, turn them off").to_owned(),
                    secondary: false,
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
        assert!(alert.buttons[0].secondary);
        assert_eq!(alert.buttons[1].label, "Go");
    }
}
