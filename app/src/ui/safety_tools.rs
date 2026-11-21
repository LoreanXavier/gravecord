//! DM Safety Tools modal (`SafetyToolsModal`, module 556127 `tt`, `H`, `F`, `$`, `X`, `Z`, `q`;
//! tips data module 327337 `RU`/`x7`/`jR`/`CL`; tips card module 889229; false-positive report
//! module 544231 `XR`; CSS modules 524785, 367699, 514646, 102097, 903919).
//!
//! Three slides inside a 440px small modal:
//! - `Actions` ("Safety toolkit"): Ignore/Unignore, Block/Unblock, Report; Vibe with Wumpus,
//!   Review safety tips, Find support (locale-aware helpline: Crisis Text Line / ThroughlineCare /
//!   NoFiltr); "Why am I seeing this?"
//! - `SafetyTips` ("Safety tips"): Brand gradient header ("Remember, you're always in control")
//!   and the three numbered tips of module 327337 `RU`.
//! - `AboutSafetyAlerts` ("About safety alerts"): Teen safety explanation with Help Center link,
//!   "Go to settings" button, and "Did we make a mistake? Let us know" false-positive report
//!   (`POST /channels/{channel_id}/safety-warning/report-false-positive`).

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, IntoElement, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};
use serde_json::Value;

/// REST fetch key prefix for false-positive feedback (`544231 XR`).
pub(super) const FALSE_POSITIVE_PREFIX: &str = "safety-tools-false-positive:";
/// `MVz.SAFETY_ALERTS` Help Center article URL (module 652215 / 975571).
const SAFETY_ALERTS_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/18210977897239";
/// Helpline URLs from module 327337 (`x7`, `jR`, `CL`).
const CRISIS_TEXT_LINE_URL: &str = "https://www.crisistextline.org/";
const NO_FILTR_URL: &str = "https://nofiltr.org/resources-hub/";
const THROUGHLINE_URL: &str = "https://discord.findahelpline.com/";
/// Lofi beats video for "Vibe with Wumpus" (module 674272).
const VIBE_WITH_WUMPUS_URL: &str = "https://discord.com/safety";

/// Slides of `SafetyToolsModal` (`G` of module 556127).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slide {
    Actions,
    SafetyTips,
    AboutSafetyAlerts,
}

/// Regional support helpline variant (`426190 N`/`z` in module 556127).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SupportProvider {
    CrisisTextLine,
    ThroughlineCare,
    NoFiltr,
}

impl SupportProvider {
    pub(crate) fn from_locale(locale: &str) -> Self {
        match locale {
            "en-US" => Self::CrisisTextLine,
            "en-GB" | "fr" | "de" | "es-ES" | "pt-BR" | "nl" | "pl" | "it" => Self::ThroughlineCare,
            _ => Self::NoFiltr,
        }
    }

    pub(crate) fn subtitle_key(self) -> &'static str {
        match self {
            Self::CrisisTextLine => "iNcsrW",
            Self::ThroughlineCare => "S9O1ZZ",
            Self::NoFiltr => "g5uwC5",
        }
    }

    pub(crate) fn url(self) -> &'static str {
        match self {
            Self::CrisisTextLine => CRISIS_TEXT_LINE_URL,
            Self::ThroughlineCare => THROUGHLINE_URL,
            Self::NoFiltr => NO_FILTR_URL,
        }
    }
}

/// The three tips of module 327337 `RU`.
pub(crate) const SAFETY_TIP_KEYS: [&str; 3] = ["bCWw8l", "mzSucz", "ZkmC7s"];

/// Feedback vote for Safety Tips ("Was this helpful?", module 198916: `L84yVm`, `2GrOCN`, `COp9BO`, `gd/Yqs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SafetyFeedback {
    Upvote,
    Downvote,
}

pub(crate) struct SafetyToolsModal {
    pub channel_id: Id,
    pub other_user_id: Id,
    pub slide: Slide,
    pub submitting_false_positive: bool,
    /// Optional thumbs-up/down feedback vote on `Slide::SafetyTips` (module 198916).
    pub feedback: Option<SafetyFeedback>,
    focus: FocusHandle,
}

impl Root {
    pub(super) fn safety_tools_open(&self) -> bool {
        self.safety_tools.is_some()
    }

    /// Opens the Safety Tools modal (`556127`) for a DM channel and peer user.
    pub(super) fn open_safety_tools(
        &mut self,
        channel_id: Id,
        other_user_id: Id,
        cx: &mut Context<Self>,
    ) {
        self.safety_tools = Some(SafetyToolsModal {
            channel_id,
            other_user_id,
            slide: Slide::Actions,
            submitting_false_positive: false,
            feedback: None,
            focus: cx.focus_handle(),
        });
        cx.notify();
    }

    pub(super) fn close_safety_tools(&mut self, cx: &mut Context<Self>) {
        self.safety_tools = None;
        cx.notify();
    }

    pub(super) fn safety_tools_key(
        &mut self,
        key: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.safety_tools.is_none() {
            return false;
        }
        if key.key == "escape" {
            self.close_safety_tools(cx);
            return true;
        }
        false
    }

    /// Handles the response from `POST /channels/{id}/safety-warning/report-false-positive`.
    pub(super) fn safety_tools_fetched(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if !key.starts_with(FALSE_POSITIVE_PREFIX) {
            return false;
        }
        let ok = self
            .fetched
            .get(key)
            .and_then(|v| v["ok"].as_bool())
            .unwrap_or(false);
        if ok {
            self.close_safety_tools(cx);
            self.show_success_toast(dc_intl::t("FhgVWi").to_owned(), cx);
        } else {
            if let Some(m) = self.safety_tools.as_mut() {
                m.submitting_false_positive = false;
            }
            self.show_app_toast(dc_intl::t("R0RpRX"), cx);
        }
        cx.notify();
        true
    }

    /// Capture helper: `safety-tools [actions|tips|about]` opens the modal on any slide.
    #[cfg(feature = "capture")]
    pub(crate) fn script_safety_tools(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let (channel_id, other_user_id) = self
            .stores
            .private_channels
            .first()
            .and_then(|ch| self.stores.dm_recipient(ch).map(|u| (ch.id, u.id)))
            .unwrap_or((1, 2));
        let slide = if arg.contains("tips") {
            Slide::SafetyTips
        } else if arg.contains("about") {
            Slide::AboutSafetyAlerts
        } else {
            Slide::Actions
        };
        self.open_safety_tools(channel_id, other_user_id, cx);
        if let Some(m) = self.safety_tools.as_mut() {
            m.slide = slide;
        }
        format!("safety-tools: opened {slide:?}")
    }

    #[allow(clippy::too_many_arguments)]
    fn safety_action_row(
        &self,
        id: &'static str,
        title: &'static str,
        subtitle: &'static str,
        button_text: &'static str,
        primary: bool,
        disabled: bool,
        border_top: bool,
        on_press: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .py(px(16.))
            .when(border_top, |d| {
                d.border_t_1().border_color(color("--border-subtle"))
            })
            .flex()
            .items_center()
            .justify_between()
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
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(subtitle),
                    ),
            )
            .child(
                div()
                    .id(id)
                    .flex_none()
                    .h(px(32.))
                    .px(px(14.))
                    .rounded(px(8.))
                    .bg(if primary {
                        color("--brand-500")
                    } else {
                        color("--control-secondary-background-default")
                    })
                    .when(!primary, |d| {
                        d.border_1()
                            .border_color(color("--control-secondary-border-default"))
                    })
                    .when(disabled, |d| d.opacity(0.5))
                    .when(!disabled, |d| {
                        d.cursor_pointer()
                            .hover(|s| {
                                s.bg(if primary {
                                    color("--brand-560")
                                } else {
                                    color("--background-mod-subtle")
                                })
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_press(this, window, cx);
                            }))
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(if primary {
                        color("--white")
                    } else {
                        color("--control-secondary-text-default")
                    })
                    .child(button_text),
            )
            .into_any_element()
    }

    fn safety_nav_row(
        &self,
        id: &'static str,
        title: &'static str,
        subtitle: Option<&'static str>,
        border_top: bool,
        target: Slide,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .py(px(16.))
            .when(border_top, |d| {
                d.border_t_1().border_color(color("--border-subtle"))
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(m) = this.safety_tools.as_mut() {
                    m.slide = target;
                }
                cx.notify();
            }))
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
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .children(subtitle.map(|sub| {
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(sub)
                    })),
            )
            .child(icon(
                "bot_chevron_small_right",
                20.,
                "--interactive-text-default",
            ))
            .into_any_element()
    }

    /// Renders `SafetyToolsModal` (module 556127).
    pub(super) fn safety_tools_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.safety_tools.as_ref()?;
        let channel_id = modal.channel_id;
        let other_user_id = modal.other_user_id;
        let slide = modal.slide;
        let submitting = modal.submitting_false_positive;

        let _aria = dc_intl::t("eXlt01");
        let heading = match slide {
            Slide::Actions => dc_intl::t("MAhAp6"),
            Slide::SafetyTips => dc_intl::t("EtNxi6"),
            Slide::AboutSafetyAlerts => dc_intl::t("qI14KM"),
        };

        let body: AnyElement = match slide {
            Slide::Actions => {
                let is_ignored = self.stores.ignored.contains(&other_user_id);
                let is_blocked = self.stores.relationships.get(&other_user_id).copied() == Some(2);
                let provider = SupportProvider::from_locale(dc_intl::locale());

                let group_1 = div()
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .child(self.safety_action_row(
                        "safety-tools-ignore",
                        dc_intl::t("B7ZT06"),
                        dc_intl::t("fCfp49"),
                        if is_ignored {
                            dc_intl::t("9e0wLn")
                        } else {
                            dc_intl::t("B7ZT06")
                        },
                        false,
                        is_blocked,
                        false,
                        move |this, _, cx| {
                            if is_ignored {
                                this.unignore_user(other_user_id, cx);
                            } else {
                                this.ignore_user(other_user_id, cx);
                            }
                        },
                        cx,
                    ))
                    .child(self.safety_action_row(
                        "safety-tools-block",
                        dc_intl::t("oDxaKy"),
                        dc_intl::t("Lj37az"),
                        if is_blocked {
                            dc_intl::t("Hro40y")
                        } else {
                            dc_intl::t("oDxaKy")
                        },
                        true,
                        false,
                        true,
                        move |this, _, cx| {
                            if is_blocked {
                                this.unblock_user(other_user_id, cx);
                            } else {
                                this.block_user(other_user_id, cx);
                            }
                            this.close_safety_tools(cx);
                        },
                        cx,
                    ))
                    .child(self.safety_action_row(
                        "safety-tools-report",
                        dc_intl::t("X27yhD"),
                        dc_intl::t("0tydOa"),
                        dc_intl::t("X27yhD"),
                        false,
                        false,
                        true,
                        move |this, window, cx| {
                            this.close_safety_tools(cx);
                            this.open_report_user(other_user_id, window, cx);
                        },
                        cx,
                    ));

                let group_2 = div()
                    .mt(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .child(self.safety_action_row(
                        "safety-tools-vibe",
                        dc_intl::t("syuaPI"),
                        dc_intl::t("LLBnNk"),
                        dc_intl::t("rzvzyZ"),
                        false,
                        false,
                        false,
                        |this, _, cx| {
                            cx.open_url(VIBE_WITH_WUMPUS_URL);
                            this.close_safety_tools(cx);
                        },
                        cx,
                    ))
                    .child(self.safety_nav_row(
                        "safety-tools-tips",
                        dc_intl::t("7LgVmt"),
                        Some(dc_intl::t("pwoRjc")),
                        true,
                        Slide::SafetyTips,
                        cx,
                    ))
                    .child(self.safety_action_row(
                        "safety-tools-support",
                        dc_intl::t("ZSbbMJ"),
                        dc_intl::t(provider.subtitle_key()),
                        dc_intl::t("rK4Gqv"),
                        false,
                        false,
                        true,
                        move |_, _, cx| {
                            cx.open_url(provider.url());
                        },
                        cx,
                    ));

                let group_3 = div()
                    .mt(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .child(self.safety_nav_row(
                        "safety-tools-about",
                        dc_intl::t("otdt24"),
                        None,
                        false,
                        Slide::AboutSafetyAlerts,
                        cx,
                    ));

                div()
                    .flex()
                    .flex_col()
                    .child(group_1)
                    .child(group_2)
                    .child(group_3)
                    .into_any_element()
            }
            Slide::SafetyTips => {
                // `tipsSection_zaXzS4` (module 889229 + 903919).
                let mut list = div().px(px(16.)).pb(px(16.)).flex().flex_col();
                for (index, key) in SAFETY_TIP_KEYS.iter().enumerate() {
                    list = list.child(
                        div()
                            .mt(px(16.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .size(px(32.))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(color("--background-mod-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-brand"))
                                    .child(format!("{}", index + 1)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t(key)),
                            ),
                    );
                }

                let voted = modal.feedback;
                let _tips_title = dc_intl::t("lyt43P");
                let _up_aria = dc_intl::t("2GrOCN");
                let _down_aria = dc_intl::t("COp9BO");
                let vote_btn = |id: &'static str, kind: SafetyFeedback, label: &'static str| {
                    let active = voted == Some(kind);
                    div()
                        .id(id)
                        .h(px(30.))
                        .px(px(12.))
                        .rounded(px(6.))
                        .bg(if active {
                            color("--control-primary-background-default")
                        } else {
                            color("--control-secondary-background-default")
                        })
                        .hover(|s| {
                            s.bg(if active {
                                color("--control-primary-background-hover")
                            } else {
                                color("--control-secondary-background-hover")
                            })
                        })
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(if active {
                            color("--white")
                        } else {
                            color("--text-strong")
                        })
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) = this.safety_tools.as_mut()
                                && m.feedback != Some(kind)
                            {
                                m.feedback = Some(kind);
                                this.put_app_toast(dc_intl::t("gd/Yqs").to_owned(), true, cx);
                            }
                            cx.notify();
                        }))
                };
                // Module 198916: Safety Tips + "More you can do" (`K5FKtc`) + "Was this helpful?" (`L84yVm`).
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(color("--background-base-lower"))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .p(px(16.))
                                    .bg(linear_gradient(
                                        270.,
                                        linear_color_stop(rgba(0x5865f2aa), 0.),
                                        linear_color_stop(color("--brand-500"), 0.71),
                                    ))
                                    .child(
                                        div()
                                            .max_w(px(280.))
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("0QSL1C")),
                                    ),
                            )
                            .child(list),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("K5FKtc")),
                            )
                            .child(
                                div()
                                    .id("safety-tips-more-actions")
                                    .p(px(12.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("MAhAp6"))
                                    .child(icon("shop_arrow_right", 14., "--text-muted"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.safety_tools.as_mut() {
                                            m.slide = Slide::Actions;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("L84yVm")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(vote_btn(
                                        "safety-tips-vote-up",
                                        SafetyFeedback::Upvote,
                                        "👍",
                                    ))
                                    .child(vote_btn(
                                        "safety-tips-vote-down",
                                        SafetyFeedback::Downvote,
                                        "👎",
                                    )),
                            ),
                    )
                    .into_any_element()
            }
            Slide::AboutSafetyAlerts => {
                // `F` of module 556127 (`wrapperStyle_vC6mwh`).
                let about_text =
                    dc_intl::format("njJ/Cg", &[("learnMoreLink", SAFETY_ALERTS_ARTICLE.into())]);
                let false_positive_text =
                    dc_intl::format("2uYViD", &[("reportFalsePositiveHook", "".into())]);
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(24.))
                    .child(
                        div()
                            .id("safety-tools-learn-more")
                            .cursor_pointer()
                            .text_center()
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .text_color(color("--text-default"))
                            .child(about_text)
                            .on_click(|_, _, cx| {
                                cx.open_url(SAFETY_ALERTS_ARTICLE);
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("safety-tools-go-settings")
                                    .w_full()
                                    .h(px(38.))
                                    .rounded(px(8.))
                                    .bg(color("--brand-500"))
                                    .hover(|s| s.bg(color("--brand-560")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("qpWnyC"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_safety_tools(cx);
                                        this.open_settings_section("Content & Social", cx);
                                    })),
                            )
                            .child(
                                div()
                                    .id("safety-tools-false-positive")
                                    .pt(px(8.))
                                    .when(submitting, |d| d.opacity(0.5))
                                    .when(!submitting, |d| d.cursor_pointer())
                                    .text_center()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-link"))
                                    .child(false_positive_text)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.safety_tools.as_mut() {
                                            if m.submitting_false_positive {
                                                return;
                                            }
                                            m.submitting_false_positive = true;
                                        }
                                        let _ = this.requests.send(Request::Rest {
                                            key: Some(format!(
                                                "{FALSE_POSITIVE_PREFIX}{channel_id}"
                                            )),
                                            method: reqwest::Method::POST,
                                            route: format!(
                                                "/channels/{channel_id}/safety-warning/report-false-positive"
                                            ),
                                            body: Some(Value::Null),
                                        });
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
        };

        let show_back = matches!(slide, Slide::SafetyTips | Slide::AboutSafetyAlerts);
        let footer = div()
            .px(px(16.))
            .py(px(14.))
            .bg(color("--background-surface-higher"))
            .border_t_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .id("safety-tools-close")
                    .h(px(32.))
                    .px(px(14.))
                    .rounded(px(8.))
                    .bg(color("--control-secondary-background-default"))
                    .border_1()
                    .border_color(color("--control-secondary-border-default"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .text_color(color("--control-secondary-text-default"))
                    .child(dc_intl::t("cpT0Cq"))
                    .on_click(cx.listener(|this, _, _, cx| this.close_safety_tools(cx))),
            )
            .when(show_back, |d| {
                d.child(
                    div()
                        .id("safety-tools-back")
                        .h(px(32.))
                        .px(px(14.))
                        .rounded(px(8.))
                        .bg(color("--control-secondary-background-default"))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .text_color(color("--control-secondary-text-default"))
                        .child(dc_intl::t("13/7kX"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(m) = this.safety_tools.as_mut() {
                                m.slide = Slide::Actions;
                            }
                            cx.notify();
                        })),
                )
            });

        Some(
            div()
                .id("safety-tools-backdrop")
                .track_focus(&modal.focus)
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| this.close_safety_tools(cx)))
                .child(
                    div()
                        .id("safety-tools-modal")
                        .w(px(440.))
                        .max_h(px(660.))
                        .rounded(px(12.))
                        .overflow_hidden()
                        .bg(color("--background-surface-high"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .shadow_xl()
                        .flex()
                        .flex_col()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .pt(px(20.))
                                .pb(px(14.))
                                .px(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_size(px(22.))
                                        .line_height(px(28.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(heading),
                                ),
                        )
                        .child(
                            div()
                                .id("safety-tools-body")
                                .flex_1()
                                .min_h(px(0.))
                                .overflow_y_scroll()
                                .px(px(16.))
                                .pb(px(16.))
                                .child(body),
                        )
                        .child(footer),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_safety_tools_and_tips_strings_resolve() {
        for key in [
            "FhgVWi", "R0RpRX", "njJ/Cg", "qpWnyC", "2uYViD", "B7ZT06", "fCfp49", "9e0wLn",
            "oDxaKy", "Lj37az", "Hro40y", "X27yhD", "0tydOa", "ZSbbMJ", "iNcsrW", "rK4Gqv",
            "g5uwC5", "S9O1ZZ", "syuaPI", "LLBnNk", "rzvzyZ", "7LgVmt", "pwoRjc", "otdt24",
            "0QSL1C", "13/7kX", "EtNxi6", "qI14KM", "MAhAp6", "eXlt01", "cpT0Cq",
            // 327337 RU tips
            "bCWw8l", "mzSucz", "ZkmC7s",
        ] {
            assert!(
                dc_intl::has_key(key),
                "missing en-US key {key} in safety_tools"
            );
        }
        assert_eq!(
            SupportProvider::from_locale("en-US"),
            SupportProvider::CrisisTextLine
        );
        assert_eq!(
            SupportProvider::from_locale("en-GB"),
            SupportProvider::ThroughlineCare
        );
        assert_eq!(SupportProvider::from_locale("tr"), SupportProvider::NoFiltr);
    }
}
