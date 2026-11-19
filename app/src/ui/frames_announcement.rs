//! "New in the Shop: Profile Frames" (module 627781), opened once by `CollectiblesMarketingManager` (module 596339
//! `Nu.maybeOpenAnnouncementModal`) after the connection opens, for a verified account that has not dismissed
//! `COLLECTIBLES_PROFILE_FRAMES_ANNOUNCEMENT` (745). Any close dismisses it; Browse Frames opens the Shop.
//!
//! The dialog (size `lg`, 680 wide) is filled by `content_2LEGNU` (`--background-base-lower`, 24px padding, 16px
//! gap): the artwork across the top fading out by half its height, the overlay close button 20px in, the
//! `display-md` title over its line, two cards (`card_2LEGNU`: picture 120 high, `heading-sm/semibold` and
//! `body-sm/medium`) and Browse Frames at 60% of the width.
//!
//! GAP: the artwork's `mask-image` fade and the blurred backdrop under the title are drawn as a gradient over it.

use super::{Root, color, icon};
use crate::net::Request;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, deferred, div, img, linear_color_stop,
    linear_gradient, prelude::*, px,
};
use serde_json::json;

/// `DismissibleContent.COLLECTIBLES_PROFILE_FRAMES_ANNOUNCEMENT` (module 554146).
pub(super) const CONTENT: u16 = 745;

const ART: &str = "https://cdn.discordapp.com/assets/content/1a3e6059912682527c3fdd25719dbee27c89e23c2c3aba2acc9348e951e5acad.jpg";
const CARD_LOOK: &str = "https://cdn.discordapp.com/assets/content/2c67362cb5834dffecbfce5e2b725245d0120a869b0d5245f2d8d71f6a4f5be9.png";
const CARD_GIFT: &str = "https://cdn.discordapp.com/assets/content/ac2f61afc0a4de0533067bef49b716c95e9b05bba9ec10ffa634c5ed9662a533.png";

/// `maybeOpenAnnouncementModal`: a verified account whose settings arrived without the content dismissed.
pub(super) fn announcement_due(loaded: bool, verified: bool, dismissed: bool) -> bool {
    loaded && verified && !dismissed
}

impl Root {
    /// `POST_CONNECTION_OPEN` of `CollectiblesMarketingManager`: the dialog, once per session.
    pub(super) fn maybe_open_frames_announcement(&mut self) {
        if self.frames_announcement_seen {
            return;
        }
        let verified = self.stores.account.verified;
        if !announcement_due(
            self.stores.settings_loaded(),
            verified,
            self.stores.content_dismissed(CONTENT),
        ) {
            return;
        }
        self.frames_announcement_seen = true;
        for url in [ART, CARD_LOOK, CARD_GIFT] {
            self.request_image(url.to_owned(), url.to_owned());
        }
        self.frames_announcement = true;
    }

    /// `Dr(COLLECTIBLES_PROFILE_FRAMES_ANNOUNCEMENT)` with any dismiss action, and the dialog closes.
    pub(super) fn close_frames_announcement(&mut self) {
        self.frames_announcement = false;
        if self.stores.content_dismissed(CONTENT) {
            return;
        }
        let patch = self.stores.dismiss_content(CONTENT);
        if !patch.is_empty() && !crate::storage::settings_writes_blocked() {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/1".into(),
                body: Some(json!({ "settings": patch })),
            });
        }
    }

    pub(super) fn frames_announcement_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.frames_announcement {
            return None;
        }
        let pose = super::motion::modal_pose("frames-announcement");
        let art = self.images.get(ART).cloned();
        let card = |picture: &'static str, title: &'static str, body: &'static str| {
            div()
                .flex_1()
                .min_w(px(0.))
                .p(px(20.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(16.))
                .text_center()
                .child(
                    div().w_full().h(px(120.)).children(
                        self.images
                            .get(picture)
                            .cloned()
                            .map(|art| img(art).size_full().object_fit(ObjectFit::Contain)),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t(title)),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t(body)),
                        ),
                )
        };
        let base = color("--background-base-lower");
        let clear = gpui::Rgba { a: 0., ..base };
        let title_lines: Vec<String> = dc_intl::t("W3MtdV")
            .split('\n')
            .map(str::to_uppercase)
            .collect();
        let dialog = div()
            .id("frames-announcement")
            .occlude()
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .relative()
            .w(px(680.))
            .p(px(24.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(base)
            .overflow_hidden()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(16.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .children(art.map(|art| {
                // `background_2LEGNU`: the artwork at the full width from the top, fading out by half the height.
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    // `background-size: 100% auto`: the picture's own ratio sets its height.
                    .child(img(art).w_full())
                    .child(div().absolute().inset_0().bg(linear_gradient(
                        180.,
                        linear_color_stop(clear, 0.3),
                        linear_color_stop(base, 1.),
                    )))
            }))
            .child(
                div()
                    .id("frames-announcement-close")
                    .absolute()
                    .top(px(20.))
                    .right(px(20.))
                    .size(px(32.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .bg(color("--control-overlay-secondary-background-default"))
                    .hover(|style| style.bg(color("--control-overlay-secondary-background-hover")))
                    .child(icon(
                        "modal_close",
                        16.,
                        "--control-overlay-secondary-text-default",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_frames_announcement();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        // `titleContainer_2LEGNU`: 200px down, 8px between the title and its line.
                        div()
                            .pt(px(200.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.))
                            .text_center()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .font_family(super::nitro::NITRO_FONT)
                                    .text_size(px(32.))
                                    .line_height(px(34.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .children(title_lines),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("v72/pr")),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .w_full()
                            .flex()
                            .gap(px(16.))
                            .child(card(CARD_LOOK, "zOtryQ", "azn85l"))
                            .child(card(CARD_GIFT, "Ky01Mq", "VgVDbe")),
                    )
                    .child(div().mt(px(8.)).w(px((680. - 48.) * 0.6)).flex().child(
                        super::modal::footer_button(
                            super::modal::button(
                                "frames-announcement-browse",
                                dc_intl::t("c7U9yf"),
                                super::modal::Tone::Primary,
                                |this, _, cx| {
                                    // `TAKE_ACTION`, then the Shop (`Cz` of module 110676).
                                    this.close_frames_announcement();
                                    this.open_shop(cx);
                                },
                            ),
                            cx,
                        ),
                    )),
            );
        Some(
            deferred(
                div()
                    .id("frames-announcement-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(color("--background-scrim")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_frames_announcement();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(feature = "capture")]
impl Root {
    /// Capture only: the dialog, as `maybeOpenAnnouncementModal` would open it.
    pub fn script_frames_announcement(&mut self, cx: &mut Context<Self>) {
        for url in [ART, CARD_LOOK, CARD_GIFT] {
            self.request_image(url.to_owned(), url.to_owned());
        }
        self.frames_announcement = true;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_announcement_waits_for_a_verified_account_that_has_not_seen_it() {
        assert!(super::announcement_due(true, true, false));
        assert!(!super::announcement_due(false, true, false));
        assert!(!super::announcement_due(true, false, false));
        assert!(!super::announcement_due(true, true, true));
        assert_eq!(dc_intl::t("W3MtdV"), "New in the Shop:\nProfile Frames");
    }
}
