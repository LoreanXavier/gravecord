//! Suspended User Safety Hub page (module 187686, `SuspendedUserSafetyHubPage`) and embedded Safety Hub standing &
//! violations body (module 37380).
//!
//! Displays the top danger notice bar, all nine `AgeCheckStatus` (`239093.qN`) banners, account standing summary, and
//! collapsible Active / Expired violation accordions paginated in groups of three. Selecting any violation opens the
//! Violation Detail modal (`violation_detail.rs`, module 163002), whose appeal CTA opens `violation_appeal.rs`
//! (module 894683).

use super::violation_detail::{ViolationAppealState, ViolationDetailModal, ViolationOrigin};
use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, InteractiveText, IntoElement, SharedString,
    StyledText, deferred, div, prelude::*, px,
};
use std::borrow::Cow;
use std::ops::Range;

const PAGE_STEP: usize = 3;

/// Official `239093.qN` age-check states of `SuspendedUserSafetyHubPage` (module 187686).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AgeCheckStatus {
    None,
    Loading,
    Success,
    Verified,
    VerifiedOtherViolationsRemain,
    Error,
    Failure,
    Underage,
    UnderageManualReview,
}

/// One violation entry in the Safety Hub body (module 37380).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct SafetyHubViolation {
    pub id: String,
    pub description: String,
    pub guild_name: Option<String>,
    pub origin: ViolationOrigin,
    pub actions: Vec<String>,
    pub expiration_date: Option<String>,
    pub explainer_url: String,
    pub is_new: bool,
    pub expired: bool,
    pub appealed: bool,
    pub is_coppa: bool,
}

/// Open state of `SuspendedUserSafetyHubPage` (module 187686 + 37380).
pub(crate) struct SuspendedSafetyHub {
    pub suspended_token: String,
    pub age_check: AgeCheckStatus,
    pub expressive_modal_v2: bool,
    pub manual_review_methods_loading: bool,
    pub manual_review_methods_available: usize,
    pub loading: bool,
    pub load_error: bool,
    pub active_open: bool,
    pub expired_open: bool,
    pub active_shown: usize,
    pub expired_shown: usize,
    pub violations: Vec<SafetyHubViolation>,
    /// Module 793163 (`AgeVerificationModalLayout`): `Some(step)` (`0` = start, `1` = method step).
    pub age_verify_step: Option<u8>,
}

impl SuspendedSafetyHub {
    #[allow(dead_code)]
    pub(crate) fn new(suspended_token: String) -> Self {
        Self {
            suspended_token,
            age_check: AgeCheckStatus::None,
            expressive_modal_v2: false,
            manual_review_methods_loading: false,
            manual_review_methods_available: 0,
            loading: false,
            load_error: false,
            active_open: true,
            expired_open: false,
            active_shown: PAGE_STEP,
            expired_shown: PAGE_STEP,
            violations: Vec::new(),
            age_verify_step: None,
        }
    }

    pub(crate) fn violation_title(v: &SafetyHubViolation) -> String {
        match v.origin {
            ViolationOrigin::OwnedGuild => dc_intl::format(
                "Lb0HVv",
                &[
                    (
                        "guildName",
                        Arg::Str(Cow::Borrowed(v.guild_name.as_deref().unwrap_or("Server"))),
                    ),
                    ("description", Arg::Str(Cow::Borrowed(&v.description))),
                ],
            ),
            ViolationOrigin::JoinedGuild => dc_intl::format(
                "rmpEPD",
                &[
                    (
                        "guildName",
                        Arg::Str(Cow::Borrowed(v.guild_name.as_deref().unwrap_or("Server"))),
                    ),
                    (
                        "classification_type",
                        Arg::Str(Cow::Borrowed(&v.description)),
                    ),
                ],
            ),
            ViolationOrigin::User => dc_intl::format(
                "QY4g5t",
                &[("description", Arg::Str(Cow::Borrowed(&v.description)))],
            ),
        }
    }
}

enum BannerKind {
    Info,
    Success,
    Critical,
}

type BannerAction = (
    &'static str,
    String,
    Box<dyn Fn(&mut Root, &mut Context<Root>)>,
);

fn status_banner(
    id: &'static str,
    kind: BannerKind,
    body: AnyElement,
    action: Option<BannerAction>,
    cx: &mut Context<Root>,
) -> AnyElement {
    let (bg, border, icon_name, icon_color) = match kind {
        BannerKind::Info => (
            color("--background-base-lower"),
            color("--border-subtle"),
            "circle_info",
            "--text-link",
        ),
        BannerKind::Success => (
            color("--background-base-lower"),
            color("--text-feedback-positive"),
            "circle_check",
            "--text-feedback-positive",
        ),
        BannerKind::Critical => (
            color("--background-base-lower"),
            color("--text-feedback-critical"),
            "standing_circle_error",
            "--text-feedback-critical",
        ),
    };
    div()
        .id(id)
        .w_full()
        .px(px(16.))
        .py(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .flex()
        .items_center()
        .gap(px(12.))
        .child(icon(icon_name, 20., icon_color))
        .child(div().flex_1().min_w(px(0.)).child(body))
        .children(action.map(|(btn_id, label, handler)| {
            div()
                .id(btn_id)
                .flex_shrink_0()
                .px(px(14.))
                .h(px(32.))
                .rounded(px(6.))
                .bg(color("--control-primary-background-default"))
                .hover(|s| s.bg(color("--control-primary-background-hover")))
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(13.))
                .font_weight(FontWeight(500.))
                .text_color(color("--white"))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    handler(this, cx);
                }))
        }))
        .into_any_element()
}

/// Module 793163 (`AgeVerificationModalLayout`): whether the footer distributes space-between
/// (when `"Back to start"` `fjKFae` is shown alongside the right slot / `"Cancel"` `4gTnU0`) vs `end`.
pub(super) fn age_verification_footer_space_between(show_back: bool, hide_footer: bool) -> bool {
    !hide_footer && show_back
}

/// Module 793163 (`AgeVerificationModalLayout`): medium (`480px`) modal shell titled `"Age verification"`
/// (`eKo6mw`), optional fullbleed body, and footer `ButtonGroup` (`fjKFae` `"Back to start"`, right slot
/// or `4gTnU0` `"Cancel"`).
#[allow(clippy::too_many_arguments)]
pub(super) fn age_verification_modal_layout(
    body: AnyElement,
    fullbleed: bool,
    show_back_to_start: bool,
    hide_footer: bool,
    footer_right_slot: Option<AnyElement>,
    cx: &mut Context<Root>,
) -> AnyElement {
    let space_between = age_verification_footer_space_between(show_back_to_start, hide_footer);
    let right_slot = footer_right_slot.unwrap_or_else(|| {
        div()
            .id("age-verify-cancel")
            .h(px(38.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .hover(|s| s.bg(color("--background-mod-strong")))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(dc_intl::t("4gTnU0"))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(hub) = this.suspended_safety_hub.as_mut() {
                    hub.age_verify_step = None;
                }
                cx.notify();
            }))
            .into_any_element()
    });
    div()
        .id("age-verification-modal-backdrop")
        .occlude()
        .absolute()
        .inset_0()
        .bg(gpui::rgba(0x000000b3))
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if let Some(hub) = this.suspended_safety_hub.as_mut() {
                    hub.age_verify_step = None;
                }
                cx.notify();
            }),
        )
        .child(
            div()
                .id("age-verification-modal")
                .w(px(480.))
                .rounded(px(12.))
                .overflow_hidden()
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .flex()
                .flex_col()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(20.))
                        .py(px(16.))
                        .border_b_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(18.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("eKo6mw")),
                        )
                        .child(
                            div()
                                .id("age-verify-close")
                                .size(px(28.))
                                .rounded(px(6.))
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("modal_close", 16., "--interactive-icon-default"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(hub) = this.suspended_safety_hub.as_mut() {
                                        hub.age_verify_step = None;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .child(div().when(!fullbleed, |b| b.p(px(20.))).child(body))
                .when(!hide_footer, |card| {
                    card.child(
                        div()
                            .px(px(20.))
                            .py(px(14.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .when(space_between, |f| f.justify_between())
                            .when(!space_between, |f| f.justify_end())
                            .when(show_back_to_start, |f| {
                                f.child(
                                    div()
                                        .id("age-verify-back-to-start")
                                        .h(px(38.))
                                        .px(px(14.))
                                        .rounded(px(8.))
                                        .bg(color("--background-mod-subtle"))
                                        .hover(|s| s.bg(color("--background-mod-strong")))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("fjKFae"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(hub) = this.suspended_safety_hub.as_mut() {
                                                hub.age_verify_step = Some(0);
                                            }
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(right_slot),
                    )
                }),
        )
        .into_any_element()
}

impl Root {
    pub(crate) fn close_suspended_safety_hub(&mut self) -> bool {
        self.suspended_safety_hub.take().is_some()
    }

    pub(crate) fn fetch_suspended_safety_hub(&mut self, cx: &mut Context<Self>) {
        let Some(hub) = self.suspended_safety_hub.as_mut() else {
            return;
        };
        hub.loading = true;
        hub.load_error = false;
        let token = hub.suspended_token.clone();
        if token.is_empty() {
            let _ = self.requests.send(Request::Rest {
                key: Some("safety-hub-me".to_owned()),
                method: reqwest::Method::GET,
                route: "/safety-hub/@me".to_owned(),
                body: None,
            });
        } else {
            let _ = self.requests.send(Request::Rest {
                key: Some("safety-hub-suspended-me".to_owned()),
                method: reqwest::Method::POST,
                route: "/safety-hub/suspended/@me".to_owned(),
                body: Some(serde_json::json!({ "token": token })),
            });
        }
        cx.notify();
    }

    pub(crate) fn fetch_suspended_age_methods(&mut self, cx: &mut Context<Self>) {
        let Some(hub) = self.suspended_safety_hub.as_mut() else {
            return;
        };
        hub.manual_review_methods_loading = true;
        hub.age_verify_step = Some(0);
        let token = hub.suspended_token.clone();
        let _ = self.requests.send(Request::Rest {
            key: Some("safety-hub-age-methods".to_owned()),
            method: reqwest::Method::POST,
            route: "/age-verification/suspended/methods/v2".to_owned(),
            body: Some(serde_json::json!({ "token": token })),
        });
        cx.notify();
    }

    fn open_violation_from_hub(&mut self, violation: &SafetyHubViolation, cx: &mut Context<Self>) {
        let appeal = if violation.is_coppa {
            ViolationAppealState::CoppaAgeVerify
        } else if violation.appealed {
            ViolationAppealState::Appealed
        } else if violation.expired {
            ViolationAppealState::Expired
        } else {
            ViolationAppealState::Eligible
        };
        self.violation_detail_modal = Some(ViolationDetailModal {
            classification: violation.description.clone(),
            guild_name: violation.guild_name.clone(),
            origin: violation.origin,
            actions: violation.actions.clone(),
            expiration_date: violation.expiration_date.clone(),
            explainer_url: violation.explainer_url.clone(),
            appeal,
        });
        cx.notify();
    }

    pub(in crate::ui) fn suspended_safety_hub_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let hub = self.suspended_safety_hub.as_ref()?;
        let age_check = hub.age_check;
        let expressive_v2 = hub.expressive_modal_v2;
        let show_top_danger = age_check != AgeCheckStatus::Verified;

        // Top danger notice (`_` in module 187686): MG+Bzb + Learn More (9JceHN).
        let top_notice = show_top_danger.then(|| {
            div()
                .w_full()
                .px(px(20.))
                .py(px(10.))
                .bg(color("--status-danger"))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("MG+Bzb")),
                )
                .child(
                    div()
                        .id("suspended-hub-learn-more")
                        .px(px(10.))
                        .h(px(26.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(color("--white"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("9JceHN"))
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.open_url("https://support.discord.com/hc/articles/18210965981847");
                        })),
                )
        });

        // Age-check status banner (`G` in module 187686).
        let age_banner: Option<AnyElement> = match age_check {
            AgeCheckStatus::None => None,
            AgeCheckStatus::Loading => Some(status_banner(
                "suspended-age-loading",
                BannerKind::Info,
                div()
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t(if expressive_v2 { "PU8nMu" } else { "nhhy/R" }))
                    .into_any_element(),
                None,
                cx,
            )),
            AgeCheckStatus::Success => {
                let runs =
                    dc_intl::format_runs("maZDL6", &[("login", Arg::Str(Cow::Borrowed("/login")))]);
                let mut full = String::new();
                let mut ranges: Vec<Range<usize>> = Vec::new();
                let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
                let link_style = HighlightStyle {
                    color: Some(color("--text-link").into()),
                    ..Default::default()
                };
                for (chunk, hook) in runs {
                    let start = full.len();
                    full.push_str(&chunk);
                    let end = full.len();
                    if hook.as_deref() == Some("$link") && end > start {
                        ranges.push(start..end);
                        highlights.push((start..end, link_style));
                    }
                }
                let root = cx.entity().downgrade();
                let styled = StyledText::new(full).with_highlights(highlights);
                let text = InteractiveText::new("suspended-age-success-link", styled).on_click(
                    ranges,
                    move |_, _, cx| {
                        let _ = root.update(cx, |this, cx| {
                            this.close_suspended_safety_hub();
                            cx.notify();
                        });
                    },
                );
                Some(status_banner(
                    "suspended-age-success",
                    BannerKind::Success,
                    div()
                        .text_size(px(14.))
                        .text_color(color("--text-default"))
                        .child(text)
                        .into_any_element(),
                    None,
                    cx,
                ))
            }
            AgeCheckStatus::Verified => Some(status_banner(
                "suspended-age-verified",
                BannerKind::Success,
                div()
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("2Qe65J"))
                    .into_any_element(),
                Some((
                    "suspended-age-login",
                    dc_intl::t("2jvQ6K").to_owned(),
                    Box::new(|this, cx| {
                        this.close_suspended_safety_hub();
                        cx.notify();
                    }),
                )),
                cx,
            )),
            AgeCheckStatus::VerifiedOtherViolationsRemain => Some(status_banner(
                "suspended-age-other-violations",
                BannerKind::Success,
                div()
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("Ie7p1Q"))
                    .into_any_element(),
                None,
                cx,
            )),
            AgeCheckStatus::Error => Some(status_banner(
                "suspended-age-error",
                BannerKind::Critical,
                div()
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("4sILBU"))
                    .into_any_element(),
                None,
                cx,
            )),
            AgeCheckStatus::Failure => Some(status_banner(
                "suspended-age-failure",
                BannerKind::Critical,
                div()
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("40R63o"))
                    .into_any_element(),
                None,
                cx,
            )),
            AgeCheckStatus::Underage => Some(status_banner(
                "suspended-age-underage",
                BannerKind::Critical,
                div()
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("dqbMbn"))
                    .into_any_element(),
                Some((
                    "suspended-age-retry",
                    dc_intl::t("IcA9iD").to_owned(),
                    Box::new(|this, cx| {
                        if let Some(hub) = this.suspended_safety_hub.as_mut() {
                            hub.age_check = AgeCheckStatus::Loading;
                        }
                        cx.notify();
                    }),
                )),
                cx,
            )),
            AgeCheckStatus::UnderageManualReview => {
                if hub.manual_review_methods_loading {
                    Some(status_banner(
                        "suspended-age-manual-loading",
                        BannerKind::Info,
                        div()
                            .text_size(px(14.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t(if expressive_v2 { "PU8nMu" } else { "nhhy/R" }))
                            .into_any_element(),
                        None,
                        cx,
                    ))
                } else if hub.manual_review_methods_available == 0 {
                    Some(status_banner(
                        "suspended-age-manual-review",
                        BannerKind::Critical,
                        div()
                            .text_size(px(14.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t("VTgFYh"))
                            .into_any_element(),
                        Some((
                            "suspended-age-request-manual",
                            dc_intl::t("NkTGsC").to_owned(),
                            Box::new(|this, cx| {
                                this.open_violation_appeal(
                                    "coppa-manual-review".to_owned(),
                                    "minimum age requirement".to_owned(),
                                    "https://support.discord.com/hc/articles/18210965981847"
                                        .to_owned(),
                                    true,
                                    this.suspended_safety_hub
                                        .as_ref()
                                        .map(|h| h.suspended_token.clone()),
                                    cx,
                                );
                            }),
                        )),
                        cx,
                    ))
                } else {
                    Some(status_banner(
                        "suspended-age-underage-retry",
                        BannerKind::Critical,
                        div()
                            .text_size(px(14.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t("dqbMbn"))
                            .into_any_element(),
                        Some((
                            "suspended-age-retry-methods",
                            dc_intl::t("IcA9iD").to_owned(),
                            Box::new(|this, cx| {
                                this.fetch_suspended_age_methods(cx);
                            }),
                        )),
                        cx,
                    ))
                }
            }
        };

        // Embedded Safety Hub body (module 37380).
        let active: Vec<&SafetyHubViolation> =
            hub.violations.iter().filter(|v| !v.expired).collect();
        let expired: Vec<&SafetyHubViolation> =
            hub.violations.iter().filter(|v| v.expired).collect();

        let render_violation_row = |idx: usize, v: &SafetyHubViolation, cx: &mut Context<Self>| {
            let owned = v.clone();
            let title = SuspendedSafetyHub::violation_title(v);
            div()
                .id(SharedString::from(format!("safety-hub-viol-{idx}")))
                .w_full()
                .px(px(16.))
                .py(px(14.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .hover(|s| s.bg(color("--background-mod-subtle")))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .when(v.is_new, |d| {
                                    d.child(
                                        div()
                                            .px(px(6.))
                                            .py(px(2.))
                                            .rounded(px(4.))
                                            .bg(color("--status-danger"))
                                            .text_size(px(11.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("QKMRC4")),
                                    )
                                })
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(title),
                                ),
                        )
                        .children(v.expiration_date.clone().map(|exp| {
                            div()
                                .text_size(px(12.))
                                .text_color(color("--text-muted"))
                                .child(exp)
                        })),
                )
                .child(icon(
                    "bot_chevron_small_right",
                    16.,
                    "--interactive-icon-default",
                ))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_violation_from_hub(&owned, cx);
                }))
        };

        let active_header = dc_intl::format("IeV2oY", &[("count", Arg::Num(active.len() as f64))]);
        let expired_header =
            dc_intl::format("fZAHBT", &[("count", Arg::Num(expired.len() as f64))]);

        let active_section = {
            let shown = hub.active_shown.min(active.len());
            let remaining = active.len().saturating_sub(shown);
            let next_page = remaining.min(PAGE_STEP);
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .id("safety-hub-active-toggle")
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(active_header),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t("XJ2YVR")),
                                ),
                        )
                        .child(icon(
                            if hub.active_open {
                                "chevron_down"
                            } else {
                                "bot_chevron_small_right"
                            },
                            18.,
                            "--interactive-icon-default",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(hub) = this.suspended_safety_hub.as_mut() {
                                hub.active_open = !hub.active_open;
                            }
                            cx.notify();
                        })),
                )
                .when(hub.active_open, |d| {
                    if active.is_empty() {
                        d.child(
                            div()
                                .p(px(16.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-base-lower"))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("reLFaV")),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t("ERdH1o")),
                                ),
                        )
                    } else {
                        d.children(
                            active
                                .iter()
                                .take(shown)
                                .enumerate()
                                .map(|(i, v)| render_violation_row(i, v, cx)),
                        )
                        .when(next_page > 0, |d| {
                            let label = dc_intl::format(
                                "9Ml56H",
                                &[("nextPageSize", Arg::Num(next_page as f64))],
                            );
                            d.child(
                                div()
                                    .id("safety-hub-active-more")
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-link"))
                                    .hover(|s| s.underline())
                                    .cursor_pointer()
                                    .child(label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(hub) = this.suspended_safety_hub.as_mut() {
                                            hub.active_shown += PAGE_STEP;
                                        }
                                        cx.notify();
                                    })),
                            )
                        })
                    }
                })
        };

        let expired_section = {
            let shown = hub.expired_shown.min(expired.len());
            let remaining = expired.len().saturating_sub(shown);
            let next_page = remaining.min(PAGE_STEP);
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .id("safety-hub-expired-toggle")
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(expired_header),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t("SzGV0g")),
                                ),
                        )
                        .child(icon(
                            if hub.expired_open {
                                "chevron_down"
                            } else {
                                "bot_chevron_small_right"
                            },
                            18.,
                            "--interactive-icon-default",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(hub) = this.suspended_safety_hub.as_mut() {
                                hub.expired_open = !hub.expired_open;
                            }
                            cx.notify();
                        })),
                )
                .when(hub.expired_open, |d| {
                    if expired.is_empty() {
                        d.child(
                            div()
                                .p(px(16.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-base-lower"))
                                .text_size(px(14.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("RV3AXf")),
                        )
                    } else {
                        d.children(
                            expired
                                .iter()
                                .take(shown)
                                .enumerate()
                                .map(|(i, v)| render_violation_row(1000 + i, v, cx)),
                        )
                        .when(next_page > 0, |d| {
                            let label = dc_intl::format(
                                "9Ml56H",
                                &[("nextPageSize", Arg::Num(next_page as f64))],
                            );
                            d.child(
                                div()
                                    .id("safety-hub-expired-more")
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-link"))
                                    .hover(|s| s.underline())
                                    .cursor_pointer()
                                    .child(label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(hub) = this.suspended_safety_hub.as_mut() {
                                            hub.expired_shown += PAGE_STEP;
                                        }
                                        cx.notify();
                                    })),
                            )
                        })
                    }
                })
        };

        let body_content: AnyElement = if hub.load_error {
            status_banner(
                "safety-hub-load-error",
                BannerKind::Critical,
                div()
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("TDRvqs"))
                    .into_any_element(),
                Some((
                    "safety-hub-load-retry",
                    dc_intl::t("R1AN4F").to_owned(),
                    Box::new(|this, cx| {
                        this.fetch_suspended_safety_hub(cx);
                    }),
                )),
                cx,
            )
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(20.))
                .child(active_section)
                .child(expired_section)
                .into_any_element()
        };

        let age_verify_overlay = hub.age_verify_step.map(|step| {
            let show_back = step > 0;
            let body = div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(if step == 0 {
                            "Choose how you'd like to confirm your age to restore access to your account."
                        } else {
                            "Follow the instructions in the verification prompt or go back to choose another method."
                        }),
                )
                .when(step == 0, |col| {
                    col.child(
                        div()
                            .id("age-verify-method-next")
                            .px(px(14.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("IcA9iD")),
                            )
                            .child(icon("chevron_right", 16., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(hub) = this.suspended_safety_hub.as_mut() {
                                    hub.age_verify_step = Some(1);
                                }
                                cx.notify();
                            })),
                    )
                })
                .into_any_element();
            age_verification_modal_layout(body, false, show_back, false, None, cx)
        });

        Some(
            deferred(
                div()
                    .id("suspended-safety-hub-page")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .flex_col()
                    .children(top_notice)
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .flex()
                            .justify_center()
                            .p(px(24.))
                            .child(
                                div()
                                    .w(px(740.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(20.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(px(24.))
                                                    .line_height(px(30.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child("Account Standing"),
                                            )
                                            .child(
                                                div()
                                                    .id("suspended-hub-close")
                                                    .size(px(32.))
                                                    .rounded(px(8.))
                                                    .cursor_pointer()
                                                    .hover(|s| {
                                                        s.bg(color("--background-mod-subtle"))
                                                    })
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon(
                                                        "modal_close",
                                                        18.,
                                                        "--interactive-icon-default",
                                                    ))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.close_suspended_safety_hub();
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                    .children(age_banner)
                                    .child(body_content),
                            ),
                    )
                    .children(age_verify_overlay),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_suspended_safety_hub(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let arg = arg.trim();
        if arg == "close" {
            self.close_suspended_safety_hub();
            cx.notify();
            return "suspended safety hub closed".to_owned();
        }
        let mut hub = SuspendedSafetyHub::new("suspended-demo-token".to_owned());
        hub.violations = vec![
            SafetyHubViolation {
                id: "v-1".to_owned(),
                description: "spam and platform manipulation".to_owned(),
                guild_name: None,
                origin: ViolationOrigin::User,
                actions: vec![
                    "Temporarily restricted from sending messages".to_owned(),
                    "Account suspended until review completes".to_owned(),
                ],
                expiration_date: Some("Expires May 12, 2027".to_owned()),
                explainer_url: "https://discord.com/guidelines".to_owned(),
                is_new: true,
                expired: false,
                appealed: false,
                is_coppa: false,
            },
            SafetyHubViolation {
                id: "v-2".to_owned(),
                description: "harassment".to_owned(),
                guild_name: Some("gravecord fixture".to_owned()),
                origin: ViolationOrigin::OwnedGuild,
                actions: vec!["Server invite links paused for 7 days".to_owned()],
                expiration_date: Some("Expires June 1, 2027".to_owned()),
                explainer_url: "https://discord.com/guidelines".to_owned(),
                is_new: false,
                expired: false,
                appealed: false,
                is_coppa: false,
            },
            SafetyHubViolation {
                id: "v-3".to_owned(),
                description: "inappropriate content".to_owned(),
                guild_name: None,
                origin: ViolationOrigin::User,
                actions: vec!["Warning issued".to_owned()],
                expiration_date: Some("Expired January 4, 2026".to_owned()),
                explainer_url: "https://discord.com/guidelines".to_owned(),
                is_new: false,
                expired: true,
                appealed: false,
                is_coppa: false,
            },
        ];
        hub.age_check = match arg {
            "loading" => AgeCheckStatus::Loading,
            "success" => AgeCheckStatus::Success,
            "verified" => AgeCheckStatus::Verified,
            "other-violations" => AgeCheckStatus::VerifiedOtherViolationsRemain,
            "error" => AgeCheckStatus::Error,
            "failure" => AgeCheckStatus::Failure,
            "manual-review" => AgeCheckStatus::UnderageManualReview,
            "none" => AgeCheckStatus::None,
            _ => AgeCheckStatus::Underage,
        };
        if arg == "age-modal" {
            hub.age_verify_step = Some(1);
        }
        hub.expired_open = true;
        self.suspended_safety_hub = Some(hub);
        cx.notify();
        format!("suspended safety hub opened ({arg})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_keys_of_suspended_safety_hub_187686_and_37380_resolve() {
        let keys = [
            "MG+Bzb", "9JceHN", "dqbMbn", "IcA9iD", "PU8nMu", "nhhy/R", "VTgFYh", "NkTGsC",
            "maZDL6", "2Qe65J", "2jvQ6K", "Ie7p1Q", "4sILBU", "40R63o", "IeV2oY", "fZAHBT",
            "XJ2YVR", "SzGV0g", "reLFaV", "ERdH1o", "RV3AXf", "QKMRC4", "Lb0HVv", "rmpEPD",
            "QY4g5t", "9Ml56H", "TDRvqs", "R1AN4F",
        ];
        for key in keys {
            let value = dc_intl::t(key);
            assert!(!value.is_empty(), "key {key} must resolve");
            assert_ne!(value, key, "key {key} fell back to literal");
        }
    }

    #[test]
    fn violation_title_formats_user_owned_and_joined_guilds() {
        let user_v = SafetyHubViolation {
            id: "1".into(),
            description: "spam".into(),
            guild_name: None,
            origin: ViolationOrigin::User,
            actions: vec![],
            expiration_date: None,
            explainer_url: String::new(),
            is_new: false,
            expired: false,
            appealed: false,
            is_coppa: false,
        };
        assert!(SuspendedSafetyHub::violation_title(&user_v).contains("spam"));
        let mut guild_v = user_v.clone();
        guild_v.origin = ViolationOrigin::OwnedGuild;
        guild_v.guild_name = Some("Test Guild".into());
        let title = SuspendedSafetyHub::violation_title(&guild_v);
        assert!(title.contains("Test Guild") && title.contains("spam"));
    }

    #[test]
    fn age_verification_modal_layout_793163_keys_and_footer_justify() {
        assert_eq!(dc_intl::t("eKo6mw"), "Age verification");
        assert_eq!(dc_intl::t("fjKFae"), "Back to start");
        assert_eq!(dc_intl::t("4gTnU0"), "Cancel");
        assert!(age_verification_footer_space_between(true, false));
        assert!(!age_verification_footer_space_between(false, false));
        assert!(!age_verification_footer_space_between(true, true));
    }
}
