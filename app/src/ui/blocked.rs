//! What blocking someone changes, measured in `design/dom/modals/block-confirm-darker-cozy.json` (the DM right
//! after Block): their messages fold into "N blocked messages — Show" (`+FcYM/`), the composer becomes
//! `9T6N5/` "You cannot send messages to a user you have blocked." with Unblock, the DM row carries the blocked
//! icon, and the profile panel asks "Show Profile?" (`b33pLD`, `8F+WNz`, `UJKH/l`) before it shows them.

use super::{Root, avatar_source, color, icon};
use crate::parity::Parity;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

pub(super) const RELATIONSHIP_BLOCKED: u64 = 2;

/// `tL` of module 559868: the account whose DM carries the changelog.
const CHANGELOG_USER: Id = 1_232_523_165_893_132_288;
/// `Do` of module 559868, where Open Blog goes.
const CHANGELOG_BLOG: &str = "https://discord.com/category/product";
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

impl Root {
    pub(super) fn is_blocked(&self, user: Id) -> bool {
        self.stores.relationships.get(&user) == Some(&RELATIONSHIP_BLOCKED)
    }

    /// `blockedSystemMessage_cKojhe`: the 16px blocked icon in the gutter, the count in `--text-default`, an em dash,
    /// and Show/Hide in `--text-muted`. 26px tall (2px padding around a 22px line).
    pub(super) fn blocked_row(
        &self,
        first: Id,
        count: usize,
        ignored: bool,
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
                        div().w(px(72.)).pl(px(28.)).child(icon(
                            if ignored { "eye_slash" } else { "blocked" },
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
                                if ignored {
                                    ignored_count(count)
                                } else {
                                    blocked_count(count)
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

impl Root {
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
}
