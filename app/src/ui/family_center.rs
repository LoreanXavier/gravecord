//! The Family Center row of the Home sidebar (`ID` of module 596339) and its Disable Family Center dialog (module
//! 371158).
//!
//! The row shows while the account's `familyCenterEnabledV2` privacy setting is on and the account may see NSFW
//! content (`shouldShowFamilyCenterRow`: module 253932 `dm` and module 945276, `nsfwAllowed`). Its close button (`Ib`,
//! `XSmallIcon`, "Close") opens the dialog: "Disable Family Center", the sentence whose link opens User Settings >
//! Family Center, Cancel and the critical Disable Family Center, which turns the setting off and goes to Friends.

use super::modal::{self, Tone};
use super::{Root, color};
use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, px};

/// The privacy setting `familyCenterEnabledV2` (the Family Center panel's "Enable Family Center tab").
const ENABLED: &str = "family_center_enabled";

#[derive(Default)]
pub(super) struct State {
    /// The Disable Family Center dialog is open.
    pub disable: bool,
}

/// `shouldShowFamilyCenterRow` of module 596339: the setting is on and the account may see NSFW content.
pub(super) fn row_shown(enabled: bool, nsfw_allowed: bool) -> bool {
    enabled && nsfw_allowed
}

impl Root {
    pub(super) fn family_center_row_shown(&self) -> bool {
        row_shown(
            self.pref_on(ENABLED, false),
            self.stores.account.nsfw_allowed,
        )
    }

    /// The close button of the row (`Ib`): the dialog of module 371158.
    pub(super) fn open_disable_family_center(&mut self, cx: &mut Context<Self>) {
        self.family_center.disable = true;
        cx.notify();
    }

    /// Disable Family Center: `dm.updateSetting(false)`, then Friends (`pX(FRIENDS)`) and the dialog closes.
    fn disable_family_center(&mut self, cx: &mut Context<Self>) {
        self.family_center.disable = false;
        self.set_family_center_enabled(false);
        self.select_home(None);
        cx.notify();
    }

    /// Module 371158 on the dialog of module 189213 (`sm`: its only child is the error line, empty until a failure).
    pub(super) fn disable_family_center_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.family_center.disable {
            return None;
        }
        // `CrZZ+p` with its `openSettingsHook` drawn as an anchor.
        let runs = dc_intl::format_runs("CrZZ+p", &[]);
        let mut text = String::new();
        let mut link = 0..0;
        for (run, hook) in runs {
            let start = text.len();
            text.push_str(&run);
            if hook.as_deref() == Some("openSettingsHook") {
                link = start..text.len();
            }
        }
        let styled = gpui::StyledText::new(text).with_highlights([(
            link.clone(),
            gpui::HighlightStyle {
                color: Some(color("--text-link").into()),
                ..Default::default()
            },
        )]);
        let subtitle = div()
            .child(
                gpui::InteractiveText::new(
                    SharedString::from("disable-family-center-subtitle"),
                    styled,
                )
                .on_click(vec![link], {
                    let open = cx.listener(|this, _: &usize, _, cx| {
                        this.family_center.disable = false;
                        this.open_family_center_panel(cx);
                    });
                    move |index, window, cx| open(&index, window, cx)
                }),
            )
            .into_any_element();
        let spec = modal::Spec {
            id: "disable-family-center",
            title: dc_intl::t("VGB30H").into(),
            subtitle: Some(subtitle),
            body: None,
            buttons: vec![
                modal::button(
                    "disable-family-center-cancel",
                    dc_intl::t("ETE/oC"),
                    Tone::Secondary,
                    |this, _, _| this.family_center.disable = false,
                ),
                modal::button(
                    "disable-family-center-confirm",
                    dc_intl::t("cSb7U0"),
                    Tone::Critical,
                    |this, _, cx| this.disable_family_center(cx),
                ),
            ],
            close: Box::new(|this, _, _| this.family_center.disable = false),
        };
        Some(modal::render_small(spec, cx))
    }

    /// The row's close button (`closeButton_zln_tS`, the private channel row's): 18px, 8px from the end, at 0.7 while
    /// the row is hovered.
    pub(super) fn family_center_close_button(
        &self,
        group: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("family-center-close")
            .aria_label(dc_intl::t("cpT0Cq"))
            .mr(px(8.))
            .size(px(18.))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .opacity(0.)
            .group_hover(group, |style| style.opacity(0.7))
            .hover(|style| style.opacity(1.))
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.open_disable_family_center(cx);
            }))
            .child(super::icon("friend_ignore", 16., "--text-muted"))
    }
}

#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_family_center(&mut self, which: &str, cx: &mut Context<Self>) -> String {
        match which {
            "on" => self.set_family_center_enabled(true),
            // The fixture account may not see NSFW content; a stand-in so the row can be framed.
            "adult" => self.stores.account.nsfw_allowed = true,
            "home" => self.select_home(None),
            "disable" => self.family_center.disable = true,
            _ => return format!("family-center: unknown step {which}"),
        }
        cx.notify();
        format!(
            "family-center: row {} dialog {}",
            self.family_center_row_shown(),
            self.family_center.disable
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_needs_the_setting_and_nsfw_access() {
        assert!(row_shown(true, true));
        assert!(!row_shown(true, false));
        assert!(!row_shown(false, true));
    }

    #[test]
    fn the_dialog_strings_are_the_official_ones() {
        assert_eq!(dc_intl::t("VGB30H"), "Disable Family Center");
        let runs = dc_intl::format_runs("CrZZ+p", &[]);
        assert!(
            runs.iter()
                .any(|(text, hook)| hook.as_deref() == Some("openSettingsHook")
                    && text == "User Settings > Family Center")
        );
    }
}
