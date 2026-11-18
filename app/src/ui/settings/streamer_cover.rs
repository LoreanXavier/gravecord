//! The Streamer Mode notice that stands in for a settings panel while Streamer Mode hides personal information
//! (module 369143: `useObscuredNotice` of the Account, Logged-in Devices, Nitro, Billing and Connected Apps panels of
//! module 158935). `streamerModeEnabled_yz0Tww` centres the picture (288 by 162, its natural size) over the
//! "Streamer Mode Enabled" button (24px above it, `streamerModeEnabledBtn_yz0Tww`), which opens Streamer Mode
//! (`openUserSettings(STREAMER_MODE_CATEGORY)`); `userSettingsStreamerModeEnabled_yz0Tww` makes it the panel's height.

use super::{Page, Root, account_panels};
use crate::ui::color;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};

/// The notice's picture (module 369143).
pub(super) const COVER_ART: &str = "https://cdn.discordapp.com/assets/content/5324ca0a8264aa1122411939df44225b50d340cec24eb1bde824e850cb32e379.svg";

/// The panels module 158935 builds with `useObscuredNotice` (369143 `L`), as our pages and nested panels.
pub(super) fn obscured(page: Page, panel: Option<account_panels::Panel>) -> bool {
    match page {
        Page::Account => matches!(panel, None | Some(account_panels::Panel::Sessions)),
        Page::Nitro | Page::Billing | Page::ConnectedApps => true,
        _ => false,
    }
}

impl Root {
    /// The notice, or `None` when the open panel shows itself.
    pub(super) fn streamer_cover(&self, page: Page, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !obscured(page, self.settings_extra.panel) || !self.streamer_hides_personal() {
            return None;
        }
        let art = self.images.get(COVER_ART).cloned();
        if art.is_none() {
            self.want_image(COVER_ART.to_owned(), COVER_ART.to_owned(), cx);
        }
        Some(
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(288.))
                        .h(px(162.))
                        .children(art.map(|art| img(art).w(px(288.)).h(px(162.)))),
                )
                // The design system's primary `md` button (40 high, 16px each side).
                .child(
                    div()
                        .id("streamer-mode-enabled")
                        .mt(px(24.))
                        .h(px(40.))
                        .px(px(16.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-primary-border-default"))
                        .bg(color("--control-primary-background-default"))
                        .hover(|style| style.bg(color("--control-primary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("Br1ls3"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_settings_category("Appearance", "Streamer Mode", cx)
                        })),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_covers_the_panels_of_module_158935() {
        assert!(obscured(Page::Account, None));
        assert!(obscured(
            Page::Account,
            Some(account_panels::Panel::Sessions)
        ));
        assert!(!obscured(Page::Account, Some(account_panels::Panel::Mfa)));
        assert!(obscured(Page::Billing, None));
        assert!(obscured(Page::Nitro, None));
        assert!(obscured(Page::ConnectedApps, None));
        assert!(!obscured(Page::Appearance, None));
    }
}
