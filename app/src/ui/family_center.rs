//! The Family Center row of the Home sidebar (`ID` of module 596339), its Disable Family Center
//! dialog (module 371158), and the teen Guardian QR Share Code widget (module 509914 / 207049).
//!
//! The row shows while the account's `familyCenterEnabledV2` privacy setting is on and the account may see NSFW
//! content (`shouldShowFamilyCenterRow`: module 253932 `dm` and module 945276, `nsfwAllowed`). Its close button (`Ib`,
//! `XSmallIcon`, "Close") opens the dialog: "Disable Family Center", the sentence whose link opens User Settings >
//! Family Center, Cancel and the critical Disable Family Center, which turns the setting off and goes to Friends.

use super::modal::{self, Tone};
use super::{Root, color, icon};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::time::{Duration, Instant};

/// The privacy setting `familyCenterEnabledV2` (the Family Center panel's "Enable Family Center tab").
const ENABLED: &str = "family_center_enabled";
/// `GET /family-center/@me/link-code` (module 509914 / 899847).
pub(super) const LINK_CODE_ROUTE: &str = "/family-center/@me/link-code";
pub(super) const LINK_CODE_KEY: &str = "family_center:link_code";
/// Link code refresh cadence: 5 minutes (`300s`, module 509914).
pub(super) const LINK_CODE_TTL_SECS: u64 = 300;

#[derive(Default)]
pub(super) struct State {
    /// The Disable Family Center dialog is open.
    pub disable: bool,
    /// Module 207049 / 509914: Guardian QR Share Code modal is open.
    pub share_modal: bool,
    /// Cached link code (override or fetched from `GET /family-center/@me/link-code`).
    pub link_code: Option<String>,
    /// Timestamp when the current link code was acquired (for 5-minute `M:SS` countdown).
    pub link_code_fetched_at: Option<Instant>,
    /// Shows `"+5kSoW"` (`"Link Copied!"`) until this instant after clicking Copy Link.
    pub copied_until: Option<Instant>,
    /// Guard so only one 1-second countdown refresh timer is running at a time.
    pub ticker_active: bool,
}

/// Official Family Center guardian invite URL (`https://discord.com/feature/family-center/my-family/{userId}/{linkCode}`).
pub(super) fn family_center_share_url(user_id: dc_state::Id, link_code: &str) -> String {
    format!("https://discord.com/feature/family-center/my-family/{user_id}/{link_code}")
}

/// Formats remaining seconds until link code rotation as `M:SS` (e.g. `4:59`).
pub(super) fn format_link_code_countdown(remaining_secs: u64) -> String {
    format!("{}:{:02}", remaining_secs / 60, remaining_secs % 60)
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

    /// Request a fresh link code (`GET /family-center/@me/link-code`) and start the 1s countdown ticker.
    pub(super) fn request_family_center_link_code(&mut self, cx: &mut Context<Self>) {
        self.family_center.link_code_fetched_at = Some(Instant::now());
        let _ = self.requests.send(crate::net::Request::Rest {
            key: Some(LINK_CODE_KEY.to_owned()),
            method: reqwest::Method::GET,
            route: LINK_CODE_ROUTE.to_owned(),
            body: None,
        });
        if !self.family_center.ticker_active {
            self.family_center.ticker_active = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    let keep_going = this
                        .update(cx, |root, cx| {
                            let active = root.family_center.share_modal
                                || root.settings_panel_is_family_center();
                            if !active {
                                root.family_center.ticker_active = false;
                                return false;
                            }
                            if let Some(fetched_at) = root.family_center.link_code_fetched_at
                                && fetched_at.elapsed().as_secs() >= LINK_CODE_TTL_SECS
                            {
                                root.family_center.link_code_fetched_at = Some(Instant::now());
                                let _ = root.requests.send(crate::net::Request::Rest {
                                    key: Some(LINK_CODE_KEY.to_owned()),
                                    method: reqwest::Method::GET,
                                    route: LINK_CODE_ROUTE.to_owned(),
                                    body: None,
                                });
                            }
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if !keep_going {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    fn settings_panel_is_family_center(&self) -> bool {
        self.settings.is_some()
    }

    fn active_family_link_code(&self) -> Option<String> {
        self.family_center.link_code.clone().or_else(|| {
            self.fetched
                .get(LINK_CODE_KEY)
                .and_then(|v| v["link_code"].as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
    }

    /// Module 509914 `r`: Teen Guardian QR Share Code card (`card_PK0nhg`):
    /// - `"Share code with guardian"` (`pojgfk`)
    /// - `"New code in:"` (`RfkLDs`) + `M:SS` live countdown
    /// - 200x200 QR code via `login::qr_code_card` with `.aria_label("Mi60fm")`
    /// - `"or share link"` (`TGr8Ws`) divider + read-only URL box + Copy Link (`WqhZss` -> `+5kSoW`)
    pub(super) fn family_center_share_code_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let user_id = self.stores.current_user.as_ref().map(|u| u.id).unwrap_or(0);
        let code = self
            .active_family_link_code()
            .unwrap_or_else(|| "LINK-CODE-LOADING".to_owned());
        let url = family_center_share_url(user_id, &code);
        let elapsed = self
            .family_center
            .link_code_fetched_at
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        let remaining = LINK_CODE_TTL_SECS.saturating_sub(elapsed);
        let countdown = format_link_code_countdown(remaining);
        let copied = self
            .family_center
            .copied_until
            .is_some_and(|until| Instant::now() < until);
        let copy_url = url.clone();
        let qr = super::login::qr_code_card(&url, 200., 8., 4.);

        div()
            .id("family-center-share-code-card")
            .p(px(24.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("pojgfk")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("RfkLDs"))
                            .child(
                                div()
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(countdown),
                            ),
                    ),
            )
            .child(
                div()
                    .id("family-center-qr-wrap")
                    .aria_label(dc_intl::t("Mi60fm"))
                    .p(px(8.))
                    .rounded(px(12.))
                    .bg(rgba(0xffffffff))
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(qr),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().flex_1().h(px(1.)).bg(color("--border-subtle")))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("TGr8Ws")),
                    )
                    .child(div().flex_1().h(px(1.)).bg(color("--border-subtle"))),
            )
            .child(
                div()
                    .w_full()
                    .h(px(44.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .text_color(color("--text-default"))
                            .truncate()
                            .child(url),
                    )
                    .child(
                        div()
                            .id("family-center-copy-link")
                            .flex_none()
                            .h(px(30.))
                            .px(px(12.))
                            .rounded(px(6.))
                            .bg(if copied {
                                color("--status-positive")
                            } else {
                                color("--control-primary-background-default")
                            })
                            .hover(|s| {
                                s.bg(if copied {
                                    color("--status-positive")
                                } else {
                                    color("--control-primary-background-hover")
                                })
                            })
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(12.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(if copied {
                                dc_intl::t("+5kSoW")
                            } else {
                                dc_intl::t("WqhZss")
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    copy_url.clone(),
                                ));
                                this.family_center.copied_until =
                                    Some(Instant::now() + Duration::from_secs(1));
                                this.put_app_toast(dc_intl::t("+5kSoW").to_owned(), true, cx);
                                cx.spawn(async move |weak, cx| {
                                    cx.background_executor().timer(Duration::from_secs(1)).await;
                                    let _ = weak.update(cx, |root, cx| {
                                        root.family_center.copied_until = None;
                                        cx.notify();
                                    });
                                })
                                .detach();
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// Module 207049 modal shell wrapping module 509914 Guardian QR Share Code (`ITlV6p` / `F4GT2S`).
    pub(super) fn family_center_share_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.family_center.share_modal {
            return None;
        }
        let body = self.family_center_share_code_card(cx);
        let dialog = div()
            .id("family-center-share-modal")
            .occlude()
            .w(px(460.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--modal-background"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(20.))
                    .pt(px(18.))
                    .pb(px(12.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("ITlV6p")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("F4GT2S")),
                            ),
                    )
                    .child(
                        div()
                            .id("family-center-share-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.family_center.share_modal = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().px(px(20.)).pb(px(20.)).child(body));
        Some(
            deferred(
                div()
                    .id("family-center-share-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.family_center.share_modal = false;
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(94)
            .into_any_element(),
        )
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
            "teen" => {
                self.stores.account.nsfw_allowed = false;
                self.family_center.link_code = Some("GUARDIAN-QR-2026".to_owned());
                self.family_center.link_code_fetched_at = Some(Instant::now());
            }
            "share" => {
                self.stores.account.nsfw_allowed = false;
                self.family_center.link_code = Some("GUARDIAN-QR-2026".to_owned());
                self.family_center.link_code_fetched_at = Some(Instant::now());
                self.family_center.share_modal = true;
            }
            "share-close" => self.family_center.share_modal = false,
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

    #[test]
    fn guardian_share_url_countdown_and_strings_match_509914() {
        assert_eq!(
            family_center_share_url(42, "CODE123"),
            "https://discord.com/feature/family-center/my-family/42/CODE123"
        );
        assert_eq!(format_link_code_countdown(300), "5:00");
        assert_eq!(format_link_code_countdown(65), "1:05");
        assert_eq!(format_link_code_countdown(9), "0:09");
        for key in [
            "pojgfk", "RfkLDs", "Mi60fm", "TGr8Ws", "WqhZss", "+5kSoW", "ITlV6p", "F4GT2S",
        ] {
            assert!(dc_intl::has_key(key), "missing key {key} in 509914/207049");
        }
    }
}
