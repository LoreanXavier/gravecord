//! What blocking someone changes, measured in `design/dom/modals/block-confirm-darker-cozy.json` (the DM right
//! after Block): their messages fold into "N blocked messages — Show" (`+FcYM/`), the composer becomes
//! `9T6N5/` "You cannot send messages to a user you have blocked." with Unblock, the DM row carries the blocked
//! icon, and the profile panel asks "Show Profile?" (`b33pLD`, `8F+WNz`, `UJKH/l`) before it shows them.

use super::chat_notices::{Alert, Then};
use super::{Root, avatar_source, color, icon};
use crate::parity::Parity;
use dc_state::{Channel, Id, Message, permissions};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

pub(super) const RELATIONSHIP_BLOCKED: u64 = 2;

/// `tL` of module 559868: the account whose DM carries the changelog.
pub(super) const CHANGELOG_USER: Id = 1_232_523_165_893_132_288;
/// `Do` of module 559868, where Open Blog goes.
pub(super) const CHANGELOG_BLOG: &str = "https://discord.com/category/product";
/// `getArticleURL(SYSTEM_DMS)` (`360036118732`, module 652215).
const SYSTEM_DMS_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/360036118732";
/// Module 388668, the system DM notice's picture.
const SYSTEM_DM_ART: &str = "https://discord.com/assets/f13bb5d4ef6ec865.svg";

/// The notice in place of the composer (module 544990).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ChatBlocker {
    /// A DM with a system account; `changelog` when it is the changelog account's.
    System { changelog: bool },
    /// The DM's other person is blocked.
    Blocked(Id),
}

/// Why a run of messages is folded (`MESSAGE_GROUP_BLOCKED`, `_IGNORED`, `_SPAMMER` of module 891522 `eA`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fold {
    Blocked,
    Ignored,
    Spammer,
    /// Not in the pinned client: MessageLogger's `MESSAGE_GROUP_DELETED` (`collapseDeleted`,
    /// `plugin_hooks/message_logger.rs`).
    Deleted,
}

/// Not in the pinned client: how many kinds of fold there are, for BetterBlockedUsers' rows by kind
/// (`plugin_hooks/better_blocked_users.rs`).
pub(super) const FOLDS: usize = 4;

/// `2021-12_inferno_spam_redaction` (module 891522 `en`): treatment 1 collapses guild channel messages from likely
/// spammers; the default (no assignment) leaves them.
pub(super) const SPAM_REDACTION: &str = "2021-12_inferno_spam_redaction";

/// `MessageTypes.AUTO_MODERATION_ACTION` (module 233444 `ER`).
const AUTO_MODERATION_ACTION: u64 = 24;

/// `xfkfTK`: "1 message hidden from likely spammer.", "N messages hidden from likely spammer." (no `=0` case).
pub(super) fn spammer_count(count: usize) -> String {
    if count == 1 {
        "1 message hidden from likely spammer.".to_owned()
    } else {
        format!("{count} messages hidden from likely spammer.")
    }
}

/// `+FcYM/`: "1 blocked message", "N blocked messages".
pub(super) fn blocked_count(count: usize) -> String {
    match count {
        0 => "No blocked messages".to_owned(),
        1 => "1 blocked message".to_owned(),
        _ => format!("{count} blocked messages"),
    }
}

/// `VFWjc+`: "1 ignored message", "N ignored messages".
pub(super) fn ignored_count(count: usize) -> String {
    match count {
        0 => "No ignored messages".to_owned(),
        1 => "1 ignored message".to_owned(),
        _ => format!("{count} ignored messages"),
    }
}

/// Not in the pinned client: MessageLogger's `DELETED_MESSAGE_COUNT` (`plugin_hooks/message_logger.rs`).
pub(super) fn deleted_count(count: usize) -> String {
    match count {
        0 => "No deleted messages".to_owned(),
        1 => "1 deleted message".to_owned(),
        _ => format!("{count} deleted messages"),
    }
}

impl Root {
    pub(super) fn is_blocked(&self, user: Id) -> bool {
        self.stores.relationships.get(&user) == Some(&RELATIONSHIP_BLOCKED)
    }

    /// Module 694318 `s`: the author carries the SPAMMER flag (`1 << 20`).
    fn is_likely_spammer(&self, user: &dc_state::User) -> bool {
        self.stores
            .users
            .get(&user.id)
            .unwrap_or(user)
            .has_flag(dc_state::USER_FLAG_SPAMMER)
    }

    /// Module 694318 `kf`: the message is a likely spammer's, and not an AutoMod action.
    pub(super) fn likely_spammer_message(&self, message: &Message) -> bool {
        message.kind != AUTO_MODERATION_ACTION && self.is_likely_spammer(&message.author)
    }

    /// `treatSpam` of module 891522: the experiment's treatment, unless the account is itself flagged.
    pub(super) fn treat_spam(&self) -> bool {
        self.stores.experiments.legacy_bucket(SPAM_REDACTION) == Some(1)
            && !self
                .stores
                .current_user
                .as_ref()
                .is_some_and(|me| me.has_flag(dc_state::USER_FLAG_SPAMMER))
    }

    /// `isBlockedForMessage` (module 994500): the author, or the person who ran the command, is blocked.
    fn blocked_for_message(&self, message: &Message) -> bool {
        self.is_blocked(message.author.id)
            || message
                .extra
                .interaction
                .as_ref()
                .is_some_and(|(_, user)| self.is_blocked(user.id))
    }

    /// Module 81437 before a jump to `message` (the reply preview, search, the inbox): true when the jump may go
    /// on. A blocked or ignored author shows its "Okay" alert and stops it; a likely spammer's message asks first
    /// where the account may see it anyway (a private channel, or Moderate Members there), else it stops with
    /// "This person is likely a spammer".
    pub(super) fn jump_allowed(&mut self, message: &Message) -> bool {
        let name = message.author.username.clone();
        let okay = || Some(dc_intl::t("BddRzS").to_owned());
        let alert = if self.blocked_for_message(message) {
            Alert::show(
                "jump-blocked",
                dc_intl::t("j7eA/g"),
                Some(dc_intl::format("dTNNgr", &[("name", name.as_str().into())])),
                None,
                okay(),
                Then::Close,
            )
        } else if self.stores.is_ignored(message.author.id) {
            Alert::show(
                "jump-ignored",
                dc_intl::t("XyWoKV"),
                Some(dc_intl::format("8t8doK", &[("name", name.as_str().into())])),
                None,
                okay(),
                Then::Close,
            )
        } else if self.likely_spammer_message(message) {
            let channel = message.channel_id;
            // `isPrivate()`: a DM or a group DM.
            let private = self
                .stores
                .private_channels
                .iter()
                .any(|known| known.id == channel);
            // `can(MODERATE_MEMBERS, channel)`: a thread asks its parent channel.
            let moderator = self.guild_of_channel(channel).is_some_and(|guild| {
                let known = self.stores.guild(guild);
                let parent = known.and_then(|known| {
                    known
                        .threads
                        .iter()
                        .find(|thread| thread.id == channel)
                        .map_or(Some(channel), |thread| Some(thread.parent_id))
                });
                let channel: Option<Channel> = known.zip(parent).and_then(|(known, parent)| {
                    known
                        .channels
                        .iter()
                        .find(|item| item.id == parent)
                        .cloned()
                });
                self.my_permission(guild, channel.as_ref(), permissions::MODERATE_MEMBERS)
            });
            if private || moderator {
                Alert::show(
                    "jump-spammer-confirm",
                    dc_intl::t("cZcG+P"),
                    Some(dc_intl::format("1YTWty", &[("name", name.as_str().into())])),
                    Some(dc_intl::t("ETE/oC").to_owned()),
                    Some(dc_intl::t("+TSRGD").to_owned()),
                    Then::JumpToMessage {
                        channel,
                        message: message.id,
                    },
                )
            } else {
                Alert::show(
                    "jump-spammer",
                    dc_intl::t("6vJKFk"),
                    Some(dc_intl::format("zKNgPF", &[("name", name.as_str().into())])),
                    None,
                    okay(),
                    Then::Close,
                )
            }
        } else {
            return true;
        };
        // An alert already open closes as it would (its own close step), and this one takes its place.
        let open = self.chat_extra.alert.take();
        self.alert_closed(open);
        self.chat_extra.alert = Some(alert);
        false
    }

    /// A jump from the reply preview, search or the inbox, through module 81437's check.
    pub(super) fn jump_to_message_checked(&mut self, message: &Message, cx: &mut Context<Self>) {
        if self.jump_allowed(message) {
            self.jump_to_message(message, cx);
        } else {
            cx.notify();
        }
    }

    /// `blockedSystemMessage_cKojhe`: the 16px blocked icon in the gutter, the count in `--text-default`, an em dash,
    /// and Show/Hide in `--text-muted`. 26px tall (2px padding around a 22px line).
    pub(super) fn blocked_row(
        &self,
        first: Id,
        count: usize,
        kind: Fold,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = self.shown_blocked.contains(&first);
        // A list row's own margin is not measured, so the gap above is padding on a wrapper: the line sits 19px down
        // and the next group starts 2px under it, as measured in the capture.
        div()
            .h(px(47.))
            .pt(px(19.))
            .child(
                div()
                    .h(px(26.))
                    .py(px(2.))
                    .flex()
                    .items_center()
                    .child(
                        // `blockedIcon_cKojhe`: 28px in, the text at the 72px gutter.
                        // Module 891522 `n3`: EyeSlash for ignored, Deny for blocked, XSmall otherwise.
                        div().w(px(72.)).pl(px(28.)).child(icon(
                            match kind {
                                Fold::Ignored => "eye_slash",
                                Fold::Blocked => "blocked",
                                // Not in the pinned client: MessageLogger's fold is neither kind, so `n3` gives it XSmall.
                                Fold::Spammer | Fold::Deleted => "x_small",
                            },
                            16.,
                            "--text-muted",
                        )),
                    )
                    .child(
                        div()
                            .parity("blocked-row")
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .text_color(color("--text-default"))
                            .child(format!(
                                "{} \u{2014} ",
                                match kind {
                                    Fold::Ignored => ignored_count(count),
                                    Fold::Blocked => blocked_count(count),
                                    Fold::Spammer => spammer_count(count),
                                    // Not in the pinned client: MessageLogger's `DELETED_MESSAGE_COUNT`.
                                    Fold::Deleted => deleted_count(count),
                                }
                            )),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("blocked-toggle-{first}")))
                            .cursor_pointer()
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .text_color(color("--text-muted"))
                            .hover(|style| style.underline())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.shown_blocked.remove(&first) {
                                    this.shown_blocked.insert(first);
                                }
                                this.rebuild_rows();
                                cx.notify();
                            }))
                            .child(if shown { "Hide" } else { "Show" }),
                    ),
            )
            .into_any_element()
    }

    /// `wrapper_ST_Y69`: in place of the composer while the DM's other person is blocked.
    pub(super) fn blocked_composer(&self, user: Id, cx: &mut Context<Self>) -> AnyElement {
        div()
            .parity_with_border("blocked-composer", 1.)
            .h(px(58.))
            .py(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-low"))
            .flex()
            .items_center()
            .child(
                div()
                    .ml(px(16.))
                    .flex_1()
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("You cannot send messages to a user you have blocked."),
            )
            .child(
                div()
                    .id("blocked-unblock")
                    .ml(px(12.))
                    .mr(px(12.))
                    .h(px(32.))
                    .px(px(11.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--control-secondary-border-default"))
                    .bg(color("--control-secondary-background-default"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--control-secondary-text-default"))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.set_relationship(user, None, cx)),
                    )
                    .child("Unblock"),
            )
            .into_any_element()
    }

    /// What takes the composer's place in the open DM (module 544990): a system DM's notice before the blocked one,
    /// which a group DM never shows.
    pub(super) fn chat_blocker(&self) -> Option<ChatBlocker> {
        let channel = self.current_dm()?;
        if channel.kind != 1 {
            return None;
        }
        let recipient = channel
            .recipients
            .first()
            .and_then(|user| self.stores.users.get(user))?;
        if recipient.system {
            // `(0,g.A)(channel.id)`, module 343328: the DM with the changelog account.
            return Some(ChatBlocker::System {
                changelog: recipient.id == CHANGELOG_USER,
            });
        }
        (channel.recipients.len() == 1 && self.is_blocked(recipient.id))
            .then_some(ChatBlocker::Blocked(recipient.id))
    }

    /// `wrapper_ST_Y69` (module 786051) with module 544990's system DM content: the picture (40px, 8px into the
    /// wrapper, 16px before the text), "This chat is reserved for official Discord notifications." in heading-sm
    /// semibold over "Discord will never ask you for your password or account token." in body-xs muted, and Learn More
    /// (Open Blog with the launch icon in the changelog DM). No capture shows this variant: the wrapper is the blocked
    /// one's measurement and the rest comes from the `_ST_Y69` rules.
    pub(super) fn system_dm_composer(&self, changelog: bool, cx: &mut Context<Self>) -> AnyElement {
        let picture = self.images.get(SYSTEM_DM_ART).cloned();
        if picture.is_none() {
            self.want_image(SYSTEM_DM_ART.to_owned(), SYSTEM_DM_ART.to_owned(), cx);
        }
        div()
            .parity_with_border("system-dm-composer", 1.)
            .h(px(58.))
            .py(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-low"))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .ml(px(16.))
                    .h(px(20.))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .ml(px(-8.))
                            .mr(px(16.))
                            .size(px(40.))
                            .flex_none()
                            .rounded(px(8.))
                            .overflow_hidden()
                            .children(picture.map(|image| img(image).size(px(40.)))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .max_h(px(40.))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .w_full()
                                    .flex_none()
                                    .max_h(px(40.))
                                    .overflow_hidden()
                                    .child(dc_intl::t("Bt2N7D")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("n/Vzkw")),
                            ),
                    ),
            )
            .child(
                div().mr(px(12.)).flex().child(
                    div()
                        .id("system-dm-button")
                        .ml(px(12.))
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color("--control-secondary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .when(changelog, |this| {
                            this.child(icon(
                                "open_external",
                                16.,
                                "--control-secondary-text-default",
                            ))
                        })
                        .on_click(move |_, _, cx| {
                            cx.open_url(if changelog {
                                CHANGELOG_BLOG
                            } else {
                                SYSTEM_DMS_ARTICLE
                            })
                        })
                        .child(dc_intl::t(if changelog { "+KSnWX" } else { "hvVgAZ" })),
                ),
            )
            .into_any_element()
    }

    /// `container_7ne32W`: the blocked person's profile waits behind "Show Profile?". gpui has no backdrop blur, so
    /// the panel's own surface stands in for the blurred card.
    /// The ignored person's profile asks the same (`/cZp5s`), with "Don't show this again" (`QbcRCJ`, module 254828)
    /// turning the question off for good (`safetySettings.ignoreProfileSpeedbumpDisabled`).
    pub(super) fn blocked_profile_gate(
        &self,
        user: &dc_state::User,
        ignored: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (avatar_key, avatar_url) = avatar_source(user);
        let avatar = self.images.get(&avatar_key).cloned();
        if avatar.is_none() {
            self.want_image(avatar_key, avatar_url, cx);
        }
        let loading_avatar = avatar.is_none();
        let id = user.id;
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(240.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .relative()
                                    .size(px(56.))
                                    .when(loading_avatar, |this| {
                                        this.rounded_full().bg(color("--background-mod-subtle"))
                                    })
                                    .children(avatar.map(|image| img(image).size(px(56.)).rounded_full()))
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(32.))
                                            .top(px(32.))
                                            .size(px(32.))
                                            .rounded_full()
                                            .bg(color("--background-base-lowest"))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon(
                                                if ignored { "eye_slash" } else { "blocked" },
                                                24.,
                                                "--text-default",
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child("Show Profile?"),
                            )
                            .child(
                                div()
                                    .text_center()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(if ignored {
                                        format!(
                                            "You ignored {}, so just making sure. They won't be notified, and will still be ignored",
                                            user.display_name()
                                        )
                                    } else {
                                        format!(
                                            "You blocked {}, so just making sure. They won't be notified, and will still be blocked",
                                            user.display_name()
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("blocked-show-profile")
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(gpui::rgba(0xffffff14))
                            // The profile theme's button grey, `rgb(85, 85, 85)` in the capture.
                            .bg(gpui::rgb(0x555555))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.shown_blocked_profiles.insert(id);
                                cx.notify();
                            }))
                            .child("Yes, show profile"),
                    )
                    .when(ignored, |this| {
                        this.child(
                            div()
                                .id("ignored-dont-show")
                                .mt(px(-8.))
                                .cursor_pointer()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-brand"))
                                .hover(|style| style.underline())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.disable_ignore_speedbump(id);
                                    cx.notify();
                                }))
                                .child("Don't show this again"),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// Module 698405 (`LurkerJoinPopout`): CHAT (`0`) vs REACTIONS (`1`) CTA popout kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum LurkerPopoutKind {
    Chat,
    Reactions,
}

/// Module 698405: state for the Lurker Join-Server CTA popout (`Xiwf1Q`/`GXvlU9`, `d7b1p6`,
/// `9VLmlZ`, `2m+Sqk`, `PUT /guilds/{guildId}/members/@me?lurker=false`).
#[derive(Clone, Debug)]
pub(crate) struct LurkerPopout {
    pub guild_id: Id,
    pub kind: LurkerPopoutKind,
    pub submitting: bool,
}

pub(crate) fn lurker_popout_title(kind: LurkerPopoutKind) -> &'static str {
    match kind {
        LurkerPopoutKind::Chat => dc_intl::t("Xiwf1Q"),
        LurkerPopoutKind::Reactions => dc_intl::t("GXvlU9"),
    }
}

pub(crate) fn join_lurked_guild_request(guild_id: Id) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("lurker:join:{guild_id}")),
        method: reqwest::Method::PUT,
        route: format!("/guilds/{guild_id}/members/@me?lurker=false"),
        body: Some(serde_json::json!({})),
    }
}

impl Root {
    /// Module 698405 (`.container_Kou47m`): 64x64 illustration + content column (`max-w: 320px`)
    /// with heading (`Xiwf1Q` / `GXvlU9`), body (`d7b1p6`), primary `"Join this server"` (`9VLmlZ`)
    /// and blank `"I'm not ready yet"` (`2m+Sqk`) buttons.
    pub(super) fn lurker_popout_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pop = self.lurker_popout.as_ref()?;
        let guild_id = pop.guild_id;
        let submitting = pop.submitting;
        let title = lurker_popout_title(pop.kind);
        Some(
            gpui::deferred(
                div()
                    .id("lurker-popout-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x00000066))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.lurker_popout = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("lurker-popout-card")
                            .occlude()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .p(px(16.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .shadow_lg()
                            .flex()
                            .items_start()
                            .gap(px(24.))
                            .child(
                                div()
                                    .size(px(64.))
                                    .flex_none()
                                    .rounded(px(16.))
                                    .bg(color("--background-brand"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("guild_menu_invite_people", 32., "--white")),
                            )
                            .child(
                                div()
                                    .max_w(px(320.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("d7b1p6")),
                                    )
                                    .child(
                                        div()
                                            .mt(px(10.))
                                            .flex()
                                            .items_center()
                                            .gap(px(24.))
                                            .child(
                                                div()
                                                    .id("lurker-popout-join")
                                                    .h(px(36.))
                                                    .px(px(16.))
                                                    .rounded(px(8.))
                                                    .bg(color("--control-primary-background-default"))
                                                    .when(!submitting, |b| {
                                                        b.hover(|s| {
                                                            s.bg(color(
                                                                "--control-primary-background-hover",
                                                            ))
                                                        })
                                                        .cursor_pointer()
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if let Some(p) =
                                                                    this.lurker_popout.as_mut()
                                                                {
                                                                    p.submitting = true;
                                                                }
                                                                let _ = this.requests.send(
                                                                    join_lurked_guild_request(
                                                                        guild_id,
                                                                    ),
                                                                );
                                                                this.lurker_popout = None;
                                                                cx.notify();
                                                            },
                                                        ))
                                                    })
                                                    .when(submitting, |b| b.opacity(0.6))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--white"))
                                                    .child(dc_intl::t("9VLmlZ")),
                                            )
                                            .child(
                                                div()
                                                    .id("lurker-popout-cancel")
                                                    .cursor_pointer()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--interactive-text-default"))
                                                    .hover(|s| {
                                                        s.text_color(color(
                                                            "--interactive-text-hover",
                                                        ))
                                                        .underline()
                                                    })
                                                    .child(dc_intl::t("2m+Sqk"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.lurker_popout = None;
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            ),
                    ),
            )
            .with_priority(4)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_lurker_popout(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.eq_ignore_ascii_case("close") {
            self.lurker_popout = None;
            cx.notify();
            return "lurker-popout: closed".to_owned();
        }
        let kind = if arg.eq_ignore_ascii_case("reactions") {
            LurkerPopoutKind::Reactions
        } else {
            LurkerPopoutKind::Chat
        };
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        self.lurker_popout = Some(LurkerPopout {
            guild_id,
            kind,
            submitting: false,
        });
        cx.notify();
        format!("lurker-popout: open {:?}", kind)
    }

    /// `j0.updateSetting(true)`: the settings-proto PATCH, kept locally at once, and this profile shown.
    fn disable_ignore_speedbump(&mut self, user: Id) {
        // Built from the account's whole safety message; nothing is sent before the settings have arrived.
        let patch = self
            .stores
            .set_pref(&dc_proto::settings::IGNORE_SPEEDBUMP_DISABLED, 1);
        if !patch.is_empty() {
            let _ = self.requests.send(crate::net::Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/1".to_owned(),
                body: Some(serde_json::json!({"settings": patch})),
            });
        }
        self.stores.ignore_speedbump_disabled = true;
        self.shown_blocked_profiles.insert(user);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_like_the_string() {
        assert_eq!(blocked_count(1), "1 blocked message");
        assert_eq!(blocked_count(3), "3 blocked messages");
    }

    #[test]
    fn lurker_popout_698405_copy_and_join_request() {
        assert_eq!(
            lurker_popout_title(LurkerPopoutKind::Chat),
            "You can't talk here yet!"
        );
        assert_eq!(
            lurker_popout_title(LurkerPopoutKind::Reactions),
            "You can't react until you join!"
        );
        assert_eq!(
            dc_intl::t("d7b1p6"),
            "You have to join the community first before you can interact."
        );
        assert_eq!(dc_intl::t("9VLmlZ"), "Join this server");
        assert_eq!(dc_intl::t("2m+Sqk"), "I'm not ready yet");
        match join_lurked_guild_request(42) {
            crate::net::Request::Rest { method, route, .. } => {
                assert_eq!(method, reqwest::Method::PUT);
                assert_eq!(route, "/guilds/42/members/@me?lurker=false");
            }
            _ => panic!("expected Rest request"),
        }
    }
}
