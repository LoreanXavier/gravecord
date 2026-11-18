//! "Trying to reorder channels?" (module 354643), opened when a channel of a server with opt-in channels is dragged
//! (`canDrag` of module 616740, reason `opt-in-channels` of module 79843: a Community server, or a staff account,
//! with `OPT_IN_CHANNELS_ON` in the account's settings of it). It is the alert of module 732159 (small, `primary`):
//! the title, "Turn on **Show All Channels** in your server menu. You can reorder channels when this view is enabled."
//! (`0eydyj`), Cancel and "Take Me There", which opens the server menu (`Z(true)` of module 72152), and the action
//! bar's "Don't show again" box (module 696208 `e`). A ticked box dismisses the content for a week when the dialog
//! closes (`f` of module 770376: `CHANNEL_REORDER_MODAL_DISMISS_SETTING`); while it is dismissed (`A`, cooldown one
//! week) the drag does nothing and nothing opens.

use super::{Root, color, modal};
use crate::net::Request;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px};
use serde_json::json;

/// `DismissibleContent.CHANNEL_REORDER_MODAL_DISMISS_SETTING` (module 554146).
pub(super) const REORDER_CONTENT: u64 = 547;
/// `cooldownDurationMs` of module 770376: `Millis.WEEK`.
pub(super) const REORDER_COOLDOWN_MS: u64 = 7 * 24 * 60 * 60 * 1000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The subtitle as runs, the bold one `$b`.
pub(super) fn subtitle_runs() -> Vec<(String, bool)> {
    dc_intl::format_runs("0eydyj", &[])
        .into_iter()
        .map(|(text, tag)| (text, tag.as_deref() == Some("$b")))
        .collect()
}

impl Root {
    /// `A` of module 770376: the dialog may open (not dismissed in the last week).
    pub(super) fn reorder_hint_allowed(&self) -> bool {
        !self
            .stores
            .recurring_dismissed(REORDER_CONTENT, REORDER_COOLDOWN_MS, now_ms())
    }

    /// The drag of a channel in an opt-in server: the dialog, unless it was dismissed.
    pub(super) fn open_reorder_hint(&mut self, cx: &mut Context<Self>) {
        if self.reorder_hint_allowed() {
            self.reorder_hint = Some(false);
            cx.notify();
        }
    }

    /// `onCloseCallback`: a ticked box dismisses the content (`f`).
    pub(super) fn close_reorder_hint(&mut self) {
        if self.reorder_hint.take() != Some(true) {
            return;
        }
        let patch = self.stores.dismiss_recurring(REORDER_CONTENT, now_ms());
        if !patch.is_empty() && !crate::storage::settings_writes_blocked() {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/1".into(),
                body: Some(json!({ "settings": patch })),
            });
        }
    }

    pub(super) fn reorder_hint_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let checked = self.reorder_hint?;
        let runs = subtitle_runs();
        let mut text = String::new();
        let mut bold = Vec::new();
        for (run, strong) in runs {
            if strong {
                bold.push((
                    text.len()..text.len() + run.len(),
                    gpui::HighlightStyle {
                        font_weight: Some(FontWeight(600.)),
                        ..Default::default()
                    },
                ));
            }
            text.push_str(&run);
        }
        let subtitle = gpui::StyledText::new(text).with_highlights(bold);
        // The checkbox of module 568385 with `labelType: "secondary"` (`body-md/normal`, `text-subtle`, 12px after
        // the box), as the call confirmation's.
        // `actionBarCheckbox_dvN-MP`: 8px free at its end; the label wraps inside the leading part, which shrinks
        // (`actionBarLeading`: `flex: 1 1`, `min-width: 0`).
        let checkbox = div()
            .id("reorder-hint-dont-show")
            .w_full()
            .pr(px(8.))
            .flex()
            .items_start()
            .gap(px(12.))
            .cursor_pointer()
            .child(super::server_onboarding::checkbox(checked, false))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-subtle"))
                    .child(dc_intl::t("m3Vfcs")),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(checked) = this.reorder_hint.as_mut() {
                    *checked = !*checked;
                }
                cx.notify();
            }))
            .into_any_element();
        let spec = modal::Spec {
            id: "reorder-hint-modal",
            title: dc_intl::t("pjYu0/").into(),
            subtitle: Some(subtitle.into_any_element()),
            body: None,
            buttons: vec![
                modal::button(
                    "reorder-hint-cancel",
                    dc_intl::t("ETE/oC"),
                    modal::Tone::Secondary,
                    |this, _, _| this.close_reorder_hint(),
                ),
                modal::button(
                    "reorder-hint-confirm",
                    dc_intl::t("zVZXRU"),
                    modal::Tone::Primary,
                    |this, _, cx| {
                        this.close_reorder_hint();
                        // `Z(true)`: the server menu opens, where Show All Channels is.
                        this.guild_menu = true;
                        cx.notify();
                    },
                ),
            ],
            close: Box::new(|this, _, _| this.close_reorder_hint()),
        };
        Some(modal::render_small_with_input(spec, checkbox, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hint_names_show_all_channels_in_bold() {
        assert_eq!(dc_intl::t("pjYu0/"), "Trying to reorder channels?");
        assert_eq!(dc_intl::t("zVZXRU"), "Take Me There");
        assert_eq!(dc_intl::t("m3Vfcs"), "Don't show again");
        let runs = subtitle_runs();
        assert_eq!(
            runs,
            vec![
                ("Turn on ".to_owned(), false),
                ("Show All Channels".to_owned(), true),
                (
                    " in your server menu. You can reorder channels when this view is enabled."
                        .to_owned(),
                    false
                ),
            ]
        );
    }
}
