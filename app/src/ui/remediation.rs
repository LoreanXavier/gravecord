//! Ignore and Block confirmations (modules 143547 and 796788, both on ConfirmModal 640238): the user's 56px avatar
//! with the action's badge, a heading and line, three cells saying what changes, the other remedy offered under "Not
//! enough?" / "Too much?", the feature guide link, and Cancel beside the confirm button. Measured in
//! `design/dom/modals/ignore-user-darker-cozy.json` and `block-user-darker-cozy.json`.
//!
//! Ignore is `PUT /users/@me/relationships/{id}/ignore` (undone by DELETE); Block puts relationship type 2. Both end
//! in a success toast ("{name} ignored", "{name} blocked").

use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, img, prelude::*, px,
    rgba,
};

/// `RELATIONSHIP_TYPES.BLOCKED`.
const BLOCKED: u64 = 2;
/// `MVz.STEALTH_REMEDIATION_FEATURE_GUIDE` through `getArticleURL` on support.discord.com in the en-US locale.
const FEATURE_GUIDE: &str = "https://support.discord.com/hc/en-us/articles/28084948873623";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Remedy {
    Ignore,
    Block,
}

impl Root {
    /// Ignore (`ignoreUser`): the PUT, the store flag, and `showIgnoreSuccessToast`.
    pub(super) fn ignore_user(&mut self, user: Id, cx: &mut Context<Self>) {
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PUT,
            route: format!("/users/@me/relationships/{user}/ignore"),
            body: None,
        });
        self.stores.set_ignored(user, true);
        let name = self.user_name(user);
        self.show_success_toast(format!("{name} ignored"), cx);
    }

    /// Unignore (`unignoreUser`): the DELETE and `showUnignoreSuccessToast`.
    pub(super) fn unignore_user(&mut self, user: Id, cx: &mut Context<Self>) {
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::DELETE,
            route: format!("/users/@me/relationships/{user}/ignore"),
            body: None,
        });
        self.stores.set_ignored(user, false);
        let name = self.user_name(user);
        self.show_success_toast(format!("{name} unignored"), cx);
    }

    /// Block (`blockUser`) and `showBlockSuccessToast`.
    pub(super) fn block_user(&mut self, user: Id, cx: &mut Context<Self>) {
        self.set_relationship(user, Some(BLOCKED), cx);
        let name = self.user_name(user);
        self.show_success_toast(format!("{name} blocked"), cx);
    }

    /// Unblock from a menu: the DELETE and `showUnblockSuccessToast`.
    pub(super) fn unblock_user(&mut self, user: Id, cx: &mut Context<Self>) {
        self.set_relationship(user, None, cx);
        let name = self.user_name(user);
        self.show_success_toast(format!("{name} unblocked"), cx);
    }

    /// `getName`: the display name, else the username.
    fn user_name(&self, user: Id) -> String {
        self.stores
            .users
            .get(&user)
            .map(|known| known.display_name().to_owned())
            .unwrap_or_default()
    }

    pub(super) fn remediation_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (user, remedy) = self.remediation?;
        let known = self.stores.users.get(&user)?.clone();
        let name = known.display_name().to_owned();
        let avatar = self.images.get(&avatar_source(&known).0).cloned();
        let (badge, heading, line, cells, other_title, other_name, other_line, confirm) =
            match remedy {
                Remedy::Ignore => (
                    "eye_slash",
                    format!("Ignore {name}?"),
                    "Get space without letting them know",
                    [
                        (
                            "circle_check",
                            "Hide their profile and messages",
                            "You can unhide them at any time",
                        ),
                        (
                            "circle_check",
                            "Mute their notifications and activity",
                            "You'll see less of them on Discord",
                        ),
                        (
                            "circle_info",
                            "They can still message you and see your activity",
                            "They won't know you ignored them",
                        ),
                    ],
                    "Not enough?",
                    "Block instead",
                    "Stop direct contact and limit what they view",
                    "Ignore",
                ),
                Remedy::Block => (
                    "deny_circle",
                    format!("Block {name}?"),
                    "Stop direct contact and limit what they view",
                    [
                        (
                            "circle_check",
                            "Stop direct messages",
                            "You can unhide past messages if you want",
                        ),
                        (
                            "circle_check",
                            "Limit access to your profile and activity",
                            "They won't see your mutual friends, bio, or status",
                        ),
                        (
                            "warning_triangle",
                            "They can interact with you in GDMs and servers",
                            "If you join a voice channel they're active in, we'll let you know",
                        ),
                    ],
                    "Too much?",
                    "Ignore instead",
                    "Get space from them without letting them know",
                    "Block",
                ),
            };
        // `confirmationHeader_RETnwM`: the avatar with its badge (32px, 4px of modal colour around a 24px icon).
        let header = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .relative()
                    .size(px(56.))
                    .children(avatar.map(|picture| img(picture).size(px(56.)).rounded_full()))
                    .child(
                        div()
                            .absolute()
                            .left(px(32.))
                            .top(px(32.))
                            .size(px(32.))
                            .p(px(4.))
                            .rounded_full()
                            .bg(color("--background-surface-high"))
                            .child(icon(badge, 24., "--icon-subtle")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(24.))
                            .line_height(px(32.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(heading),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(line),
                    ),
            );
        // `cell_NhnBvM`: 16px above and below, the icon centred beside two lines, `--border-muted` under all but the
        // last.
        let cells = div()
            .flex()
            .flex_col()
            .children(
                cells
                    .into_iter()
                    .enumerate()
                    .map(|(index, (glyph, title, detail))| {
                        div()
                            .when(index < 2, |this| {
                                this.border_b_1().border_color(color("--border-muted"))
                            })
                            .py(px(16.))
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .child(icon(glyph, 24., "--icon-subtle"))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
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
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-subtle"))
                                            .child(detail),
                                    ),
                            )
                    }),
            );
        let other = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(other_title),
            )
            .child(
                // `content_zgsiD2`: the row on the base-lower colour, 16px around, the button at the end.
                div()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(color("--background-base-lower"))
                    .px(px(16.))
                    .py(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(other_name),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-subtle"))
                                    .child(other_line),
                            ),
                    )
                    .child(
                        div()
                            .id("remediation-switch")
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|style| style.bg(color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child("View details")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                // The other confirmation replaces this one.
                                this.remediation = Some((
                                    user,
                                    match remedy {
                                        Remedy::Ignore => Remedy::Block,
                                        Remedy::Block => Remedy::Ignore,
                                    },
                                ));
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().flex().justify_center().child(
                    div()
                        .id("remediation-guide")
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-link"))
                        .child("Explore feature guide")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.follow_link(FEATURE_GUIDE, "Explore feature guide", cx);
                        })),
                ),
            );
        let footer_button =
            |id: &'static str, label: &'static str, background: &str, border: &str, text: &str| {
                div()
                    .id(id)
                    .w(px(100.))
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color(border))
                    .bg(color(background))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color(text))
                    .child(label)
            };
        let (confirm_background, confirm_border) = match remedy {
            Remedy::Ignore => (
                "--control-primary-background-default",
                "--control-primary-border-default",
            ),
            Remedy::Block => (
                "--control-critical-primary-background-default",
                "--control-critical-primary-border-default",
            ),
        };
        let modal = div()
            .parity_with_border("remediation-modal", 1.)
            .id("remediation-modal")
            .occlude()
            .w(px(444.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-normal"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.16),
                offset: gpui::point(px(0.), px(4.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div().pt(px(8.)).px(px(24.)).pb(px(20.)).child(
                    div()
                        .pt(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(header)
                        .child(cells)
                        .child(other),
                ),
            )
            .child(
                div()
                    .px(px(24.))
                    .pt(px(16.))
                    .pb(px(16.))
                    .flex()
                    .justify_end()
                    .gap(px(12.))
                    .child(
                        footer_button(
                            "remediation-cancel",
                            "Cancel",
                            "--control-secondary-background-default",
                            "--control-secondary-border-default",
                            "--control-secondary-text-default",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.remediation = None;
                            cx.notify();
                        })),
                    )
                    .child(
                        footer_button(
                            "remediation-confirm",
                            confirm,
                            confirm_background,
                            confirm_border,
                            "--white",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remediation = None;
                            match remedy {
                                Remedy::Ignore => this.ignore_user(user, cx),
                                Remedy::Block => this.block_user(user, cx),
                            }
                            cx.notify();
                        })),
                    ),
            );
        let pose = crate::ui::motion::modal_pose("remediation");
        Some(
            deferred(
                div()
                    .id("remediation-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.remediation = None;
                        cx.notify();
                    }))
                    .child(modal.posed(pose)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

/// Labels for the profile menu's Ignore and Block entries in the current state.
pub(super) fn ignore_label(ignored: bool) -> SharedString {
    if ignored { "Unignore" } else { "Ignore" }.into()
}
