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

    #[cfg(feature = "capture")]
    pub(crate) fn script_age_verify_modal(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.close_age_verification_modal();
                cx.notify();
                "age verification modal closed".to_owned()
            }
            "stage" => {
                self.open_age_verification_modal(AgeVerificationEntryPoint::StageChannelPrompt, cx);
                "age verification modal opened (stage)".to_owned()
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
}
