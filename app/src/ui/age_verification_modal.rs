//! Primary Age Verification onboarding modal (`AgeVerificationGetStartedModal`, module 929180).
//!
//! Displays the 3-step numbered verification checklist (`HphYKp`/`GCZC+9`, `nkO4L3`/`rHZFsH`,
//! `aVwLfn`/`n5vd1E`), action-bar `"Learn More"` external Help Center link (`aA6q/z`), and
//! `"Get Started"` CTA (`SJMnkX`) posting `POST /age-verification/verify`.

use super::{Root, color, icon};
use crate::net::Request;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use serde_json::json;

/// Help Center URLs referenced by module 929180 (`AGE_VERIFICATION_LEARN_MORE`, `SYSTEM_DMS`).
pub(crate) const AGE_VERIFY_LEARN_MORE_URL: &str =
    "https://support.discord.com/hc/articles/30326565624343";
pub(crate) const SYSTEM_DMS_HELP_URL: &str = "https://support.discord.com/hc/articles/360036118732";

/// Module 112954 (`IncodeAgeVerificationModal`): selectable verification method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IncodeMethod {
    FacialAgeEstimation = 1,
    IdVerification = 2,
}

/// Module 112954 capture reducer state machine (`"capturing"` | `"waiting_for_result"` |
/// `"fallback_available"` | `"bootstrapping_fallback"` | `"error"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum IncodeCaptureState {
    Capturing,
    WaitingForResult,
    FallbackAvailable,
    BootstrappingFallback,
    Error,
}

/// Module 112954: active Incode verification session state.
#[derive(Clone, Debug)]
pub(crate) struct IncodeAgeVerificationState {
    pub selected_method: Option<IncodeMethod>,
    pub capture_state: IncodeCaptureState,
    pub interview_id: Option<String>,
    pub hide_footer: bool,
}

/// Module 112954 (`295972.D0`): `POST /age-verification/verify` (`{ method, vendor: "INCODE" }`).
pub(crate) fn incode_start_verification_request(method: IncodeMethod) -> Request {
    Request::Rest {
        key: Some(format!("incode:verify:{}", method as u8)),
        method: reqwest::Method::POST,
        route: "/age-verification/verify".to_owned(),
        body: Some(json!({
            "method": method as u8,
            "vendor": "INCODE",
        })),
    }
}

/// Module 112954 (`295972.Zo`): `POST /age-verification/incode/interview` (`{ interview_id }`).
pub(crate) fn incode_register_interview_request(interview_id: &str) -> Request {
    Request::Rest {
        key: Some(format!("incode:interview:{interview_id}")),
        method: reqwest::Method::POST,
        route: "/age-verification/incode/interview".to_owned(),
        body: Some(json!({ "interview_id": interview_id })),
    }
}

/// Module 112954 (`295972.eb`): `POST /age-verification/incode/session` (`{ previous_interview_id }`).
pub(crate) fn incode_fallback_session_request(previous_interview_id: &str) -> Request {
    Request::Rest {
        key: Some(format!("incode:fallback:{previous_interview_id}")),
        method: reqwest::Method::POST,
        route: "/age-verification/incode/session".to_owned(),
        body: Some(json!({ "previous_interview_id": previous_interview_id })),
    }
}

/// Module 112954: true if a selfie attempt should trigger fallback to government ID scan.
#[allow(dead_code)]
pub(crate) fn incode_should_fallback_to_id(
    method: IncodeMethod,
    screen_event: Option<&str>,
    finish_action: Option<&str>,
    score_status: Option<&str>,
) -> bool {
    if method != IncodeMethod::FacialAgeEstimation {
        return false;
    }
    screen_event.is_some_and(|e| e.ends_with("Exhausted"))
        || finish_action == Some("rejected")
        || matches!(score_status, Some("FAIL" | "MANUAL_FAIL"))
}

fn av_copy(key: &str, fallback: &str) -> String {
    if dc_intl::has_key(key) {
        let s = dc_intl::t(key);
        if !s.trim().is_empty() {
            return s.to_owned();
        }
    }
    fallback.to_owned()
}

/// Official entry points from module `36149` (`ST` / `mK`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AgeVerificationEntryPoint {
    #[default]
    Default,
    StageChannelPrompt,
    StartStagePrompt,
    StageRaiseHand,
    SensitiveMedia,
    FamilyCenterConnection,
}

impl AgeVerificationEntryPoint {
    pub(crate) fn is_stage(self) -> bool {
        matches!(
            self,
            Self::StageChannelPrompt | Self::StartStagePrompt | Self::StageRaiseHand
        )
    }

    pub(crate) fn title(self) -> &'static str {
        if self.is_stage() {
            dc_intl::t("lSWVTM")
        } else {
            dc_intl::t("xYXsr6")
        }
    }

    pub(crate) fn subtitle(self) -> &'static str {
        if self.is_stage() {
            dc_intl::t("S/xS/w")
        } else {
            dc_intl::t("HxS3oQ")
        }
    }
}

/// Active state for module 929180 (`AgeVerificationGetStartedModal`).
#[derive(Clone, Debug, Default)]
pub(crate) struct AgeVerificationModalState {
    pub entry_point: AgeVerificationEntryPoint,
    pub loading: bool,
    pub error: Option<String>,
}

/// Module 929180 / 750481 (`POST /age-verification/verify`): initiates age verification and returns
/// `{ verification_webview_url, verification_request_id, verification_vendor_name }`.
pub(crate) fn initiate_age_verification_request(method: Option<&str>) -> Request {
    let mut body = json!({});
    if let Some(m) = method {
        body["method"] = json!(m);
    }
    Request::Rest {
        key: Some("age-verification:verify".to_owned()),
        method: reqwest::Method::POST,
        route: "/age-verification/verify".to_owned(),
        body: Some(body),
    }
}

/// Module 929180: split `n5vd1E` (`"You'll know it's us from the official tag. Learn more"`)
/// into plain body text + clickable `"Learn more"` link without duplicating the link copy.
pub(crate) fn step3_description_parts() -> (String, String) {
    let raw = dc_intl::t("n5vd1E");
    if let Some(idx) = raw.find('[') {
        let prefix = raw[..idx].trim_end().to_owned();
        let rest = &raw[idx..];
        let link_text = rest
            .split_once(':')
            .map(|(_, r)| r.trim_end_matches(']').trim())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| dc_intl::t("aA6q/z"))
            .to_owned();
        return (prefix, link_text);
    }
    let learn_lower = dc_intl::t("aA6q/z").to_ascii_lowercase();
    let raw_lower = raw.to_ascii_lowercase();
    if !learn_lower.is_empty()
        && let Some(idx) = raw_lower.rfind(&learn_lower)
    {
        let prefix = raw[..idx].trim_end().to_owned();
        let link = raw[idx..].trim().to_owned();
        return (prefix, link);
    }
    (raw.to_owned(), dc_intl::t("aA6q/z").to_owned())
}

/// Auxiliary age-verification dialogs sharing one slot on `Root`:
/// - Module `174453`: Manual review in progress (`CNm4w6` / `14Fje3` / `NX+WJN`).
/// - Module `253463`: Remove your assigned age group? reset confirmation (`g7/14g` / `FbTAmI` / `V822Mp` / `ETE/oC`).
/// - Module `297102`: Update age group retry modal (`JSdbBe` / `JNK1ue` / `El4aXl` / `FYkioq` / `xMfbRz` / `L+FgkZ`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgeVerifyAuxDialog {
    ManualReviewPending,
    ResetConfirm { submitting: bool },
    RetryUpdateAgeGroup,
}

/// Module 297102 (`700129.A`): header SVG graphic for Update Age Group modal.
pub(crate) const AGE_VERIFY_RETRY_GRAPHIC: &str = "https://cdn.discordapp.com/assets/content/9d1441338ce9090a7c1e9fab8fe34e58daa2ab7df4ac84fa280c308869d8f27b.svg";

/// Module 253463 (`96358.js`): `POST /users/@me/age-verification/reset`.
pub(crate) fn reset_age_verification_request() -> Request {
    Request::Rest {
        key: Some("age-verification:reset".to_owned()),
        method: reqwest::Method::POST,
        route: "/users/@me/age-verification/reset".to_owned(),
        body: None,
    }
}

impl Root {
    #[allow(dead_code)]
    pub(crate) fn open_age_verification_modal(
        &mut self,
        entry_point: AgeVerificationEntryPoint,
        cx: &mut Context<Self>,
    ) {
        self.age_verification_modal = Some(AgeVerificationModalState {
            entry_point,
            loading: false,
            error: None,
        });
        cx.notify();
    }

    pub(crate) fn close_age_verification_modal(&mut self) -> bool {
        self.age_verification_modal.take().is_some()
    }

    pub(in crate::ui) fn age_verification_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.age_verification_modal.as_ref()?;
        let (step3_prefix, step3_link) = step3_description_parts();
        let loading = state.loading;

        let step_row = |num: &'static str, title: &'static str, body: AnyElement| {
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(28.))
                        .flex_none()
                        .rounded_full()
                        .bg(color("--background-mod-subtle"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(num),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(body),
                )
        };

        Some(
            deferred(
                div()
                    .id("age-verification-modal-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.close_age_verification_modal();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("age-verification-modal")
                            .w(px(460.))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            // Header
                            .child(
                                div()
                                    .px(px(24.))
                                    .pt(px(24.))
                                    .pb(px(16.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_start()
                                            .justify_between()
                                            .gap(px(12.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(px(20.))
                                                    .line_height(px(26.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(state.entry_point.title()),
                                            )
                                            .child(
                                                div()
                                                    .id("age-verification-modal-close")
                                                    .size(px(28.))
                                                    .rounded(px(6.))
                                                    .cursor_pointer()
                                                    .hover(|s| {
                                                        s.bg(color("--background-mod-subtle"))
                                                    })
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .aria_label(SharedString::from(dc_intl::t(
                                                        "cpT0Cq",
                                                    )))
                                                    .child(icon(
                                                        "modal_close",
                                                        18.,
                                                        "--icon-subtle",
                                                    ))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.close_age_verification_modal();
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(state.entry_point.subtitle()),
                                    ),
                            )
                            // 3 numbered steps (`632738.PQ` listType="numbered")
                            .child(
                                div()
                                    .px(px(24.))
                                    .pb(px(20.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(16.))
                                    .child(step_row(
                                        "1",
                                        dc_intl::t("HphYKp"),
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("GCZC+9"))
                                            .into_any_element(),
                                    ))
                                    .child(step_row(
                                        "2",
                                        dc_intl::t("nkO4L3"),
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("rHZFsH"))
                                            .into_any_element(),
                                    ))
                                    .child(step_row(
                                        "3",
                                        dc_intl::t("aVwLfn"),
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .items_center()
                                            .gap(px(4.))
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-muted"))
                                            .child(step3_prefix)
                                            .child(
                                                div()
                                                    .id("age-verify-system-dms-link")
                                                    .cursor_pointer()
                                                    .text_color(color("--text-link"))
                                                    .hover(|s| s.underline())
                                                    .child(step3_link)
                                                    .on_click(cx.listener(|_, _, _, cx| {
                                                        cx.open_url(SYSTEM_DMS_HELP_URL);
                                                    })),
                                            )
                                            .into_any_element(),
                                    ))
                                    .children(state.error.clone().map(|err| {
                                        div()
                                            .p(px(10.))
                                            .rounded(px(8.))
                                            .bg(color("--input-background-error-default"))
                                            .border_1()
                                            .border_color(color("--input-border-error-default"))
                                            .text_size(px(13.))
                                            .text_color(color("--text-feedback-critical"))
                                            .child(err)
                                    })),
                            )
                            // Footer action bar: Learn More link (left) + Get Started CTA (right)
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(16.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        // `.learnMoreButton_asFdry` (`gap: 4px`)
                                        div()
                                            .id("age-verify-learn-more")
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-link"))
                                            .hover(|s| s.underline())
                                            .child(dc_intl::t("aA6q/z"))
                                            .child(icon("link_external_small", 14., "--text-link"))
                                            .on_click(cx.listener(|_, _, _, cx| {
                                                cx.open_url(AGE_VERIFY_LEARN_MORE_URL);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("age-verify-get-started")
                                            .h(px(38.))
                                            .px(px(16.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .when(loading, |d| d.opacity(0.6))
                                            .when(!loading, |d| d.cursor_pointer())
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--white"))
                                                    .child(dc_intl::t("SJMnkX")),
                                            )
                                            .child(icon("link_external_small", 14., "--white"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(m) =
                                                    this.age_verification_modal.as_mut()
                                                {
                                                    if m.loading {
                                                        return;
                                                    }
                                                    m.loading = true;
                                                    m.error = None;
                                                }
                                                let _ = this
                                                    .requests
                                                    .send(initiate_age_verification_request(None));
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 112954 (`IncodeAgeVerificationModal`, CSS `603494`): renders either the method picker
    /// (`eZvwAe`/`5yWXmT`: "Take a selfie" `rgXXcW` vs "Scan your ID" `NeVlw/`) or the active
    /// capture / fallback (`Ae/g7X` + `eVjNWp`) / waiting (`wJVyYR`) / error (`c6kn6F` + `/nicWo`) shell.
    pub(super) fn incode_age_verification_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.incode_age_verify.as_ref()?;
        let body: AnyElement = match state.selected_method {
            None => {
                // Method picker (`ExpressiveModal` blue gradient header + 2 option cards)
                div()
                    .w(px(480.))
                    .max_w(px(480.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px(px(24.))
                            .pt(px(24.))
                            .pb(px(16.))
                            .bg(gpui::linear_gradient(
                                135.,
                                gpui::linear_color_stop(rgba(0x5865f244), 0.),
                                gpui::linear_color_stop(rgba(0x23242800), 1.),
                            ))
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .line_height(px(26.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(av_copy(
                                                "eZvwAe",
                                                "How would you like to verify your age?",
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-muted"))
                                            .child(av_copy(
                                                "5yWXmT",
                                                "Choose the option that works best for you.",
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id("incode-picker-close")
                                    .size(px(28.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.incode_age_verify = None;
                                        cx.notify();
                                    }))
                                    .child(icon("modal_close", 16., "--icon-subtle")),
                            ),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .pb(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("incode-pick-selfie")
                                    .p(px(14.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-mod-subtle"))
                                    .hover(|s| s.bg(color("--background-mod-normal")))
                                    .cursor_pointer()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(st) = this.incode_age_verify.as_mut() {
                                            st.selected_method =
                                                Some(IncodeMethod::FacialAgeEstimation);
                                            st.capture_state = IncodeCaptureState::Capturing;
                                        }
                                        let _ = this.requests.send(
                                            incode_start_verification_request(
                                                IncodeMethod::FacialAgeEstimation,
                                            ),
                                        );
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(av_copy("rgXXcW", "Take a selfie")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-muted"))
                                            .child(av_copy(
                                                "fm7qBC",
                                                "Fastest option. Your biometric data is processed locally and never leaves your device.",
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id("incode-pick-id")
                                    .p(px(14.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-mod-subtle"))
                                    .hover(|s| s.bg(color("--background-mod-normal")))
                                    .cursor_pointer()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(st) = this.incode_age_verify.as_mut() {
                                            st.selected_method = Some(IncodeMethod::IdVerification);
                                            st.capture_state = IncodeCaptureState::Capturing;
                                        }
                                        let _ = this.requests.send(
                                            incode_start_verification_request(
                                                IncodeMethod::IdVerification,
                                            ),
                                        );
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(av_copy("NeVlw/", "Scan your ID")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-muted"))
                                            .child(av_copy(
                                                "ARmJ0M",
                                                "Use a driver's license, passport, or other government ID.",
                                            )),
                                    ),
                            ),
                    )
                    .when(!state.hide_footer, |card| {
                        card.child(
                            div()
                                .px(px(24.))
                                .py(px(14.))
                                .bg(color("--background-base-lower"))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .id("incode-back-to-start")
                                        .cursor_pointer()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-link"))
                                        .hover(|s| s.underline())
                                        .child(dc_intl::t("fjKFae"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.incode_age_verify = None;
                                            this.open_age_verification_modal(
                                                AgeVerificationEntryPoint::Default,
                                                cx,
                                            );
                                        })),
                                )
                                .child(
                                    div()
                                        .id("incode-picker-cancel")
                                        .h(px(34.))
                                        .px(px(14.))
                                        .rounded(px(8.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::t("4gTnU0"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.incode_age_verify = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                    })
                    .into_any_element()
            }
            Some(method) => {
                let interview_id = state
                    .interview_id
                    .clone()
                    .unwrap_or_else(|| "demo-interview".to_owned());
                let prev_id = interview_id.clone();
                let status_body: AnyElement = match state.capture_state {
                    IncodeCaptureState::Error => div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(16.))
                        .p(px(24.))
                        .child(icon("shop_circle_error", 36., "--status-danger"))
                        .child(
                            div()
                                .max_w(px(320.))
                                .text_center()
                                .text_size(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("c6kn6F")),
                        )
                        .child(
                            div()
                                .id("incode-retry")
                                .h(px(38.))
                                .px(px(18.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("/nicWo"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(st) = this.incode_age_verify.as_mut() {
                                        st.capture_state = IncodeCaptureState::Capturing;
                                    }
                                    let _ = this
                                        .requests
                                        .send(incode_start_verification_request(method));
                                    cx.notify();
                                })),
                        )
                        .into_any_element(),
                    IncodeCaptureState::FallbackAvailable
                    | IncodeCaptureState::BootstrappingFallback => {
                        let loading =
                            state.capture_state == IncodeCaptureState::BootstrappingFallback;
                        let msg = if loading {
                            av_copy("s+qGhK", "Preparing ID scan...")
                        } else {
                            av_copy("Ae/g7X", "We couldn't confirm your age with a selfie.")
                        };
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(24.))
                            .p(px(24.))
                            .child(
                                div()
                                    .max_w(px(320.))
                                    .text_center()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(msg),
                            )
                            .child(
                                div()
                                    .id("incode-continue-id-scan")
                                    .h(px(40.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .when(loading, |d| d.opacity(0.6))
                                    .when(!loading, |d| d.cursor_pointer())
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(av_copy("eVjNWp", "Continue with ID scan"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(st) = this.incode_age_verify.as_mut() {
                                            st.capture_state =
                                                IncodeCaptureState::BootstrappingFallback;
                                            st.selected_method = Some(IncodeMethod::IdVerification);
                                        }
                                        let _ = this
                                            .requests
                                            .send(incode_fallback_session_request(&prev_id));
                                        cx.notify();
                                    })),
                            )
                            .into_any_element()
                    }
                    IncodeCaptureState::WaitingForResult => div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(16.))
                        .p(px(24.))
                        .child(super::voice_view::spinner())
                        .child(
                            div()
                                .max_w(px(320.))
                                .text_center()
                                .text_size(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("wJVyYR")),
                        )
                        .into_any_element(),
                    IncodeCaptureState::Capturing => div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(16.))
                        .p(px(24.))
                        .child(super::voice_view::spinner())
                        .child(
                            div()
                                .max_w(px(320.))
                                .text_center()
                                .text_size(px(15.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(av_copy("S+mgl0", "Processing")),
                        )
                        .child(
                            div()
                                .id("incode-register-complete")
                                .h(px(36.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(dc_intl::t("wJVyYR"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(st) = this.incode_age_verify.as_mut() {
                                        st.capture_state = IncodeCaptureState::WaitingForResult;
                                    }
                                    let _ = this
                                        .requests
                                        .send(incode_register_interview_request(&interview_id));
                                    cx.notify();
                                })),
                        )
                        .into_any_element(),
                };

                div()
                    .id("incode-capture-shell")
                    .aria_label(av_copy("eKo6mw", "Age verification"))
                    .w(px(520.))
                    .max_w(px(700.))
                    .min_h(px(380.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .justify_between()
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
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(av_copy("eKo6mw", "Age verification")),
                            )
                            .child(
                                div()
                                    .id("incode-shell-close")
                                    .size(px(28.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.incode_age_verify = None;
                                        cx.notify();
                                    }))
                                    .child(icon("modal_close", 16., "--icon-subtle")),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(status_body),
                    )
                    .into_any_element()
            }
        };

        Some(
            deferred(
                div()
                    .id("incode-age-verify-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.incode_age_verify = None;
                            cx.notify();
                        }),
                    )
                    .child(body),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 174453 (`871309.A.showManualReviewPendingModal`): opens the "Manual review in progress" dialog.
    #[allow(dead_code)]
    pub(crate) fn open_age_verification_manual_review_pending(&mut self, cx: &mut Context<Self>) {
        self.age_verification_modal = None;
        self.incode_age_verify = None;
        self.age_verify_aux = Some(AgeVerifyAuxDialog::ManualReviewPending);
        cx.notify();
    }

    /// Module 253463 (`USER_AGE_VERIFICATION_RESET_CONFIRM`): opens the "Remove your assigned age group?" confirm dialog.
    #[allow(dead_code)]
    pub(crate) fn open_age_verification_reset_confirm(&mut self, cx: &mut Context<Self>) {
        self.age_verification_modal = None;
        self.incode_age_verify = None;
        self.age_verify_aux = Some(AgeVerifyAuxDialog::ResetConfirm { submitting: false });
        cx.notify();
    }

    /// Module 297102 (`Update age group`): opens the retry selfie/ID verification expressive modal (`blue` gradient).
    #[allow(dead_code)]
    pub(crate) fn open_age_verification_retry_modal(&mut self, cx: &mut Context<Self>) {
        self.age_verification_modal = None;
        self.incode_age_verify = None;
        self.age_verify_aux = Some(AgeVerifyAuxDialog::RetryUpdateAgeGroup);
        self.want_image(
            AGE_VERIFY_RETRY_GRAPHIC.to_owned(),
            AGE_VERIFY_RETRY_GRAPHIC.to_owned(),
            cx,
        );
        cx.notify();
    }

    /// Renders modules `174453`, `253463`, and `297102` auxiliary age-verification dialogs.
    pub(super) fn age_verify_aux_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.age_verify_aux?;
        match dialog {
            AgeVerifyAuxDialog::ManualReviewPending => Some(
                deferred(
                    div()
                        .id("age-verify-manual-review-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b8))
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(24.))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.age_verify_aux = None;
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .id("age-verify-manual-review-modal")
                                .occlude()
                                .w(px(400.))
                                .p(px(24.))
                                .rounded(px(12.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-high"))
                                .shadow_xl()
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .child(
                                    div()
                                        .flex()
                                        .items_start()
                                        .justify_between()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .text_size(px(20.))
                                                .line_height(px(24.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("CNm4w6")),
                                        )
                                        .child(
                                            div()
                                                .id("age-verify-manual-review-close")
                                                .size(px(28.))
                                                .rounded(px(6.))
                                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon(
                                                    "modal_close",
                                                    16.,
                                                    "--interactive-text-default",
                                                ))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.age_verify_aux = None;
                                                    cx.notify();
                                                })),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-subtle"))
                                        .child(dc_intl::t("14Fje3")),
                                )
                                .child(
                                    div().pt(px(8.)).w_full().child(
                                        div()
                                            .id("age-verify-manual-review-ok")
                                            .w_full()
                                            .h(px(38.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-primary-background-hover"))
                                            })
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("NX+WJN"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.age_verify_aux = None;
                                                cx.notify();
                                            })),
                                    ),
                                ),
                        ),
                )
                .with_priority(3)
                .into_any_element(),
            ),
            AgeVerifyAuxDialog::ResetConfirm { submitting } => Some(
                deferred(
                    div()
                        .id("age-verify-reset-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b8))
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(24.))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                if !submitting {
                                    this.age_verify_aux = None;
                                    cx.notify();
                                }
                            }),
                        )
                        .child(
                            div()
                                .id("age-verify-reset-modal")
                                .occlude()
                                .w(px(400.))
                                .p(px(24.))
                                .rounded(px(12.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-high"))
                                .shadow_xl()
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .child(
                                    div()
                                        .flex()
                                        .items_start()
                                        .justify_between()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .text_size(px(20.))
                                                .line_height(px(24.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("g7/14g")),
                                        )
                                        .child(
                                            div()
                                                .id("age-verify-reset-close")
                                                .size(px(28.))
                                                .rounded(px(6.))
                                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                                .when(!submitting, |d| d.cursor_pointer())
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon(
                                                    "modal_close",
                                                    16.,
                                                    "--interactive-text-default",
                                                ))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if !submitting {
                                                        this.age_verify_aux = None;
                                                        cx.notify();
                                                    }
                                                })),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-subtle"))
                                        .child(dc_intl::t("FbTAmI")),
                                )
                                .child(
                                    div()
                                        .pt(px(8.))
                                        .w_full()
                                        .flex()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .id("age-verify-reset-cancel")
                                                .flex_1()
                                                .h(px(38.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color("--border-muted"))
                                                .bg(color("--control-secondary-background-default"))
                                                .hover(|s| {
                                                    s.bg(color(
                                                        "--control-secondary-background-hover",
                                                    ))
                                                })
                                                .when(!submitting, |d| d.cursor_pointer())
                                                .when(submitting, |d| d.opacity(0.5))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color(
                                                    "--control-secondary-text-default",
                                                ))
                                                .child(dc_intl::t("ETE/oC"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if !submitting {
                                                        this.age_verify_aux = None;
                                                        cx.notify();
                                                    }
                                                })),
                                        )
                                        .child(
                                            div()
                                                .id("age-verify-reset-confirm")
                                                .flex_1()
                                                .h(px(38.))
                                                .rounded(px(8.))
                                                .bg(color(
                                                    "--control-critical-primary-background-default",
                                                ))
                                                .hover(|s| {
                                                    s.bg(color(
                                                        "--control-critical-primary-background-hover",
                                                    ))
                                                })
                                                .when(!submitting, |d| d.cursor_pointer())
                                                .when(submitting, |d| d.opacity(0.5))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--white"))
                                                .child(dc_intl::t("V822Mp"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if submitting {
                                                        return;
                                                    }
                                                    let _ = this.requests.send(
                                                        reset_age_verification_request(),
                                                    );
                                                    this.age_verify_aux = None;
                                                    cx.notify();
                                                })),
                                        ),
                                ),
                        ),
                )
                .with_priority(3)
                .into_any_element(),
            ),
            AgeVerifyAuxDialog::RetryUpdateAgeGroup => {
                let look = super::graphic_modal::Look {
                    width: 480.,
                    gradient: (
                        "--expressive-gradient-blue-start",
                        "--expressive-gradient-blue-end",
                    ),
                    aspect: 16. / 9.,
                };
                let learn_runs = dc_intl::format_runs("L+FgkZ", &[]);
                let learn_line = div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-muted"))
                    .children(learn_runs.into_iter().enumerate().map(|(idx, (seg, hook))| {
                        if hook.is_some() {
                            div()
                                .id(("age-verify-retry-learn", idx))
                                .text_color(color("--text-link"))
                                .cursor_pointer()
                                .hover(|s| s.underline())
                                .child(seg)
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.open_url(AGE_VERIFY_LEARN_MORE_URL);
                                }))
                                .into_any_element()
                        } else {
                            div().child(seg).into_any_element()
                        }
                    }));
                let body = div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    // Info notice (`El4aXl`, `circle_info`)
                    .child(
                        div()
                            .p(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-muted"))
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .child(icon("circle_info", 16., "--text-link"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("El4aXl")),
                            ),
                    )
                    // Clickable card (`FYkioq` / `xMfbRz` + `bot_chevron_small_right`)
                    .child(
                        div()
                            .id("age-verify-retry-start-card")
                            .p(px(16.))
                            .rounded(px(8.))
                            .bg(color("--background-mod-subtle"))
                            .hover(|s| s.bg(color("--background-mod-strong")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(16.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let _ = this.requests.send(initiate_age_verification_request(None));
                                this.age_verify_aux = None;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("FYkioq")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-subtle"))
                                            .child(dc_intl::t("xMfbRz")),
                                    ),
                            )
                            .child(icon(
                                "bot_chevron_small_right",
                                18.,
                                "--interactive-icon-default",
                            )),
                    )
                    .child(learn_line)
                    .into_any_element();
                let spec = super::graphic_modal::Spec {
                    id: "age-verify-retry-modal",
                    graphic: Some(super::graphic_modal::Graphic {
                        url: AGE_VERIFY_RETRY_GRAPHIC,
                        width: 288.,
                        height: 162.,
                    }),
                    title: dc_intl::t("JSdbBe").into(),
                    subtitle: Some(div().child(dc_intl::t("JNK1ue")).into_any_element()),
                    body: Some(body),
                    buttons: Vec::new(),
                    close: Box::new(|this, _, cx| {
                        this.age_verify_aux = None;
                        cx.notify();
                    }),
                };
                Some(self.graphic_modal_with(spec, look, cx))
            }
        }
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_age_verify_modal(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.close_age_verification_modal();
                self.incode_age_verify = None;
                self.age_verify_aux = None;
                cx.notify();
                "age verification modal closed".to_owned()
            }
            "manual-review-pending" => {
                self.open_age_verification_manual_review_pending(cx);
                "age verification manual review pending opened".to_owned()
            }
            "reset-confirm" => {
                self.open_age_verification_reset_confirm(cx);
                "age verification reset confirm opened".to_owned()
            }
            "retry" => {
                self.open_age_verification_retry_modal(cx);
                "age verification retry modal opened".to_owned()
            }
            "stage" => {
                self.open_age_verification_modal(AgeVerificationEntryPoint::StageChannelPrompt, cx);
                "age verification modal opened (stage)".to_owned()
            }
            "incode-picker" => {
                self.age_verification_modal = None;
                self.incode_age_verify = Some(IncodeAgeVerificationState {
                    selected_method: None,
                    capture_state: IncodeCaptureState::Capturing,
                    interview_id: None,
                    hide_footer: false,
                });
                cx.notify();
                "incode age verify opened (picker)".to_owned()
            }
            "incode-fallback" => {
                self.age_verification_modal = None;
                self.incode_age_verify = Some(IncodeAgeVerificationState {
                    selected_method: Some(IncodeMethod::FacialAgeEstimation),
                    capture_state: IncodeCaptureState::FallbackAvailable,
                    interview_id: Some("int-42".to_owned()),
                    hide_footer: false,
                });
                cx.notify();
                "incode age verify opened (fallback)".to_owned()
            }
            "incode-error" => {
                self.age_verification_modal = None;
                self.incode_age_verify = Some(IncodeAgeVerificationState {
                    selected_method: Some(IncodeMethod::FacialAgeEstimation),
                    capture_state: IncodeCaptureState::Error,
                    interview_id: None,
                    hide_footer: false,
                });
                cx.notify();
                "incode age verify opened (error)".to_owned()
            }
            _ => {
                self.open_age_verification_modal(AgeVerificationEntryPoint::Default, cx);
                "age verification modal opened (default)".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_verification_modal_929180_intl_and_request() {
        for key in [
            "SJMnkX", "aA6q/z", "HphYKp", "GCZC+9", "nkO4L3", "rHZFsH", "aVwLfn", "n5vd1E",
            "lSWVTM", "xYXsr6", "S/xS/w", "HxS3oQ",
        ] {
            assert!(dc_intl::has_key(key), "missing 929180 key {key}");
        }
        assert_eq!(
            AgeVerificationEntryPoint::StageChannelPrompt.title(),
            dc_intl::t("lSWVTM")
        );
        assert_eq!(
            AgeVerificationEntryPoint::Default.title(),
            dc_intl::t("xYXsr6")
        );
        let (prefix, link) = step3_description_parts();
        assert!(!prefix.is_empty());
        assert!(!link.is_empty());
        let req = initiate_age_verification_request(Some("video_selfie"));
        assert!(matches!(
            req,
            Request::Rest { route, .. } if route == "/age-verification/verify"
        ));
    }

    #[test]
    fn incode_age_verification_112954_requests_fallback_and_intl() {
        for key in [
            "/nicWo", "4gTnU0", "fjKFae", "c6kn6F", "wJVyYR", "S+mgl0", "eZvwAe", "5yWXmT",
            "rgXXcW", "fm7qBC", "NeVlw/", "ARmJ0M", "s+qGhK", "Ae/g7X", "eVjNWp", "eKo6mw",
        ] {
            assert!(dc_intl::has_key(key), "missing 112954 key {key}");
        }
        assert!(incode_should_fallback_to_id(
            IncodeMethod::FacialAgeEstimation,
            Some("selfieExhausted"),
            None,
            None,
        ));
        assert!(incode_should_fallback_to_id(
            IncodeMethod::FacialAgeEstimation,
            None,
            Some("rejected"),
            None,
        ));
        assert!(incode_should_fallback_to_id(
            IncodeMethod::FacialAgeEstimation,
            None,
            None,
            Some("MANUAL_FAIL"),
        ));
        assert!(!incode_should_fallback_to_id(
            IncodeMethod::IdVerification,
            Some("selfieExhausted"),
            Some("rejected"),
            Some("FAIL"),
        ));
        assert!(matches!(
            incode_start_verification_request(IncodeMethod::FacialAgeEstimation),
            Request::Rest { route, body: Some(ref b), .. }
                if route == "/age-verification/verify" && b["vendor"] == "INCODE" && b["method"] == 1
        ));
        assert!(matches!(
            incode_register_interview_request("int-1"),
            Request::Rest { route, body: Some(ref b), .. }
                if route == "/age-verification/incode/interview" && b["interview_id"] == "int-1"
        ));
        assert!(matches!(
            incode_fallback_session_request("int-1"),
            Request::Rest { route, body: Some(ref b), .. }
                if route == "/age-verification/incode/session" && b["previous_interview_id"] == "int-1"
        ));
    }

    #[test]
    fn age_verification_manual_review_pending_174453_intl() {
        for key in ["CNm4w6", "14Fje3", "NX+WJN"] {
            assert!(dc_intl::has_key(key), "missing 174453 intl key {key}");
        }
        assert!(dc_intl::t("14Fje3").contains("48-72"));
    }

    #[test]
    fn age_verification_reset_253463_and_retry_297102() {
        for key in [
            "g7/14g", "FbTAmI", "V822Mp", "ETE/oC", "JSdbBe", "JNK1ue", "El4aXl", "FYkioq",
            "xMfbRz", "mFvt9M", "L+FgkZ",
        ] {
            assert!(dc_intl::has_key(key), "missing 253463/297102 key {key}");
        }
        assert!(matches!(
            reset_age_verification_request(),
            Request::Rest { route, .. } if route == "/users/@me/age-verification/reset"
        ));
    }
}
