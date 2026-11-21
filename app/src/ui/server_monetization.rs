//! Server Subscriptions monetization eligibility checklist (`useMonetizationEligibilityChecklist`,
//! module 715395 `x`, `g`).
//!
//! Renders the 8-requirement eligibility checklist (Community Guidelines violations, server owner
//! age, minimum member count, server age in weeks/days, weekly communicators, member retention,
//! non-NSFW focus, and 2FA requirement) with live Contact Support and Enable 2FA actions.

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, FocusHandle, FontWeight, IntoElement, div, prelude::*, px, rgba};
use serde_json::{Value, json};

/// REST fetch key prefix for guild creator monetization eligibility (`GET /guilds/{id}/creator-monetization/eligibility`).
pub(super) const ELIGIBILITY_KEY_PREFIX: &str = "server-monetization-eligibility:";
/// `MVz.PUBLIC_GUILD_GUILDLINES` Help Center URL (module 652215 / 975571).
const COMMUNITY_GUIDELINES_URL: &str = "https://discord.com/guidelines";
/// `getSubmitRequestURL()` (module 975571).
const SUBMIT_REQUEST_URL: &str = "https://support.discord.com/hc/en-us/requests/new";

/// Server monetization eligibility payload (`715395`).
#[derive(Clone, Debug, Default)]
pub(crate) struct MonetizationEligibility {
    pub no_recent_violations: bool,
    pub minimum_owner_age_in_years: Option<u64>,
    pub meets_owner_age_requirement: Option<bool>,
    pub minimum_size: Option<u64>,
    pub has_sufficient_members: Option<bool>,
    pub minimum_age_in_days: Option<u64>,
    pub meets_server_age_requirement: Option<bool>,
    pub weekly_communicators: Option<bool>,
    pub has_member_retention: Option<bool>,
    pub not_nsfw: bool,
    pub has_enabled_2fa: Option<bool>,
}

impl MonetizationEligibility {
    pub(crate) fn from_value(v: &Value) -> Self {
        Self {
            no_recent_violations: v["no_recent_violations"].as_bool().unwrap_or(true),
            minimum_owner_age_in_years: v["minimum_owner_age_in_years"].as_u64(),
            meets_owner_age_requirement: v["meets_owner_age_requirement"].as_bool(),
            minimum_size: v["minimum_size"].as_u64(),
            has_sufficient_members: v["has_sufficient_members"].as_bool(),
            minimum_age_in_days: v["minimum_age_in_days"].as_u64(),
            meets_server_age_requirement: v["meets_server_age_requirement"].as_bool(),
            weekly_communicators: v["weekly_communicators"].as_bool(),
            has_member_retention: v["has_member_retention"].as_bool(),
            not_nsfw: v["not_nsfw"].as_bool().unwrap_or(true),
            has_enabled_2fa: v["has_enabled_2fa"].as_bool(),
        }
    }
}

/// One checklist row produced by `build_checklist` (module 715395 `x`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EligibilityItem {
    pub key: &'static str,
    pub label: String,
    pub description: String,
    pub checked: bool,
    pub action: Option<EligibilityAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EligibilityAction {
    ContactSupport,
    Enable2fa,
}

impl EligibilityAction {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ContactSupport => dc_intl::t("xU2fl+"),
            Self::Enable2fa => dc_intl::t("BU4Diu"),
        }
    }
}

/// Builds the ordered eligibility items exactly as module 715395 `x` does.
pub(crate) fn build_checklist(
    e: &MonetizationEligibility,
    user_mfa_enabled: bool,
    guild_mfa_elevated: bool,
    sorted_by_ineligible: bool,
) -> Vec<EligibilityItem> {
    let mut items = Vec::new();
    let has_violations = !e.no_recent_violations;
    items.push(EligibilityItem {
        key: "no_violations_requirement",
        label: if e.no_recent_violations {
            dc_intl::t("1lGNPZ").to_owned()
        } else {
            dc_intl::t("D+gTJt").to_owned()
        },
        description: dc_intl::format(
            "HFY0m6",
            &[("communityGuidelinesUrl", COMMUNITY_GUIDELINES_URL.into())],
        ),
        checked: e.no_recent_violations,
        action: has_violations.then_some(EligibilityAction::ContactSupport),
    });

    if let (Some(min_years), Some(meets)) =
        (e.minimum_owner_age_in_years, e.meets_owner_age_requirement)
    {
        items.push(EligibilityItem {
            key: "owner_age_requirement",
            label: if meets {
                dc_intl::t("+F8haD").to_owned()
            } else {
                dc_intl::t("5BwC/O").to_owned()
            },
            description: dc_intl::format(
                "DW1Vae",
                &[("minimumOwnerAgeInYears", (min_years as i64).into())],
            ),
            checked: meets,
            action: None,
        });
    }

    if let (Some(min_size), Some(sufficient)) = (e.minimum_size, e.has_sufficient_members) {
        items.push(EligibilityItem {
            key: "member_count_requirement",
            label: if sufficient {
                dc_intl::t("j7wXWo").to_owned()
            } else {
                dc_intl::t("W0suNz").to_owned()
            },
            description: dc_intl::format("up53zR", &[("minimumSize", (min_size as i64).into())]),
            checked: sufficient,
            action: None,
        });
    }

    if let (Some(days), Some(meets)) = (e.minimum_age_in_days, e.meets_server_age_requirement) {
        let min_age = if days > 0 && days.is_multiple_of(7) {
            dc_intl::format("EmoBD2", &[("weeks", ((days / 7) as i64).into())])
        } else {
            dc_intl::format("k2UNz+", &[("days", (days as i64).into())])
        };
        items.push(EligibilityItem {
            key: "server_age_requirement",
            label: if meets {
                dc_intl::t("mjbvWw").to_owned()
            } else {
                dc_intl::t("9BV6L6").to_owned()
            },
            description: dc_intl::format("Zwv84O", &[("minimumAge", min_age.as_str().into())]),
            checked: meets,
            action: None,
        });
    }

    if let Some(weekly) = e.weekly_communicators {
        items.push(EligibilityItem {
            key: "weekly_communicator_count_requirement",
            label: if weekly {
                dc_intl::t("Qw7qv4").to_owned()
            } else {
                dc_intl::t("b45kGG").to_owned()
            },
            description: dc_intl::t("NbtjEC").to_owned(),
            checked: weekly,
            action: None,
        });
    }

    if let Some(retention) = e.has_member_retention {
        items.push(EligibilityItem {
            key: "member_retention_requirement",
            label: if retention {
                dc_intl::t("Qvq39M").to_owned()
            } else {
                dc_intl::t("azHboI").to_owned()
            },
            description: dc_intl::t("u4rCYO").to_owned(),
            checked: retention,
            action: None,
        });
    }

    items.push(EligibilityItem {
        key: "nsfw_requirement",
        label: if e.not_nsfw {
            dc_intl::t("bymfTb").to_owned()
        } else {
            dc_intl::t("718pRA").to_owned()
        },
        description: dc_intl::t("5ZqX+j").to_owned(),
        checked: e.not_nsfw,
        action: None,
    });

    if let Some(has_2fa) = e.has_enabled_2fa {
        let needs_action = !has_2fa && (!user_mfa_enabled || !guild_mfa_elevated);
        items.push(EligibilityItem {
            key: "2fa_requirement",
            label: if has_2fa {
                dc_intl::t("NqVyFk").to_owned()
            } else {
                dc_intl::t("VcDNIV").to_owned()
            },
            description: dc_intl::format("7NzkfV", &[("enableMFAHook", "".into())]),
            checked: has_2fa,
            action: needs_action.then_some(EligibilityAction::Enable2fa),
        });
    }

    if sorted_by_ineligible {
        items.sort_by_key(|item| item.checked);
    }
    items
}

pub(crate) struct MonetizationModal {
    pub guild_id: Id,
    pub eligibility: MonetizationEligibility,
    pub loading: bool,
    focus: FocusHandle,
}

impl Root {
    pub(super) fn monetization_modal_open(&self) -> bool {
        self.monetization_modal.is_some()
    }

    /// Opens the Server Subscriptions monetization eligibility checklist (`715395`) for `guild_id`.
    #[allow(dead_code)]
    pub(super) fn open_monetization_eligibility(&mut self, guild_id: Id, cx: &mut Context<Self>) {
        self.monetization_modal = Some(MonetizationModal {
            guild_id,
            eligibility: MonetizationEligibility::default(),
            loading: true,
            focus: cx.focus_handle(),
        });
        let _ = self.requests.send(Request::Fetch {
            key: format!("{ELIGIBILITY_KEY_PREFIX}{guild_id}"),
            route: format!("/guilds/{guild_id}/creator-monetization/eligibility"),
        });
        cx.notify();
    }

    pub(super) fn close_monetization_eligibility(&mut self, cx: &mut Context<Self>) {
        self.monetization_modal = None;
        cx.notify();
    }

    pub(super) fn monetization_modal_key(
        &mut self,
        key: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.monetization_modal.is_none() {
            return false;
        }
        if key.key == "escape" {
            self.close_monetization_eligibility(cx);
            return true;
        }
        false
    }

    pub(super) fn monetization_eligibility_fetched(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(gid_str) = key.strip_prefix(ELIGIBILITY_KEY_PREFIX) else {
            return false;
        };
        let Ok(gid) = gid_str.parse::<Id>() else {
            return false;
        };
        let body = self.fetched.get(key).cloned().unwrap_or(Value::Null);
        if let Some(m) = self.monetization_modal.as_mut()
            && m.guild_id == gid
        {
            m.eligibility = MonetizationEligibility::from_value(&body);
            m.loading = false;
        }
        cx.notify();
        true
    }

    /// Capture helper: `monetization-eligibility` opens the checklist with mixed passing/failing items.
    #[cfg(feature = "capture")]
    pub(crate) fn script_monetization_eligibility(&mut self, cx: &mut Context<Self>) -> String {
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        self.monetization_modal = Some(MonetizationModal {
            guild_id,
            eligibility: MonetizationEligibility {
                no_recent_violations: true,
                minimum_owner_age_in_years: Some(18),
                meets_owner_age_requirement: Some(true),
                minimum_size: Some(500),
                has_sufficient_members: Some(false),
                minimum_age_in_days: Some(28),
                meets_server_age_requirement: Some(true),
                weekly_communicators: Some(true),
                has_member_retention: Some(false),
                not_nsfw: true,
                has_enabled_2fa: Some(false),
            },
            loading: false,
            focus: cx.focus_handle(),
        });
        cx.notify();
        "monetization-eligibility: opened".to_owned()
    }

    /// Renders the Monetization Eligibility Checklist modal (`715395`).
    pub(super) fn monetization_eligibility_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.monetization_modal.as_ref()?;
        let guild_id = modal.guild_id;
        let user_mfa = self.stores.mfa_enabled;
        let guild_mfa = false;
        let items = build_checklist(&modal.eligibility, user_mfa, guild_mfa, true);

        let mut list = div().flex().flex_col().gap(px(10.));
        for item in items {
            let icon_el = if item.checked {
                div()
                    .size(px(24.))
                    .flex_none()
                    .rounded_full()
                    .bg(color("--status-positive"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("checkmark_large", 14., "--white"))
            } else {
                div()
                    .size(px(24.))
                    .flex_none()
                    .rounded_full()
                    .bg(color("--status-danger"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("automod_cross", 12., "--white"))
            };
            let action_btn = item.action.map(|act| {
                let id = gpui::SharedString::from(format!("monetization-action-{}", item.key));
                div()
                    .id(id)
                    .flex_none()
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(6.))
                    .bg(color("--brand-500"))
                    .hover(|s| s.bg(color("--brand-560")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child(act.label())
                    .on_click(cx.listener(move |this, _, _, cx| match act {
                        EligibilityAction::ContactSupport => cx.open_url(SUBMIT_REQUEST_URL),
                        EligibilityAction::Enable2fa => {
                            if !this.stores.mfa_enabled {
                                this.close_monetization_eligibility(cx);
                                this.open_settings_section("Account", cx);
                            } else {
                                let _ = this.requests.send(Request::Rest {
                                    key: None,
                                    method: reqwest::Method::POST,
                                    route: format!("/guilds/{guild_id}/mfa"),
                                    body: Some(json!({ "level": 1 })),
                                });
                            }
                        }
                    }))
            });

            list = list.child(
                div()
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(icon_el)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(item.label),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(item.description),
                            ),
                    )
                    .children(action_btn),
            );
        }

        Some(
            div()
                .id("monetization-backdrop")
                .track_focus(&modal.focus)
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| this.close_monetization_eligibility(cx)))
                .child(
                    div()
                        .id("monetization-modal")
                        .w(px(540.))
                        .max_h(px(640.))
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
                                        .line_height(px(22.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child("Server Subscriptions Eligibility"),
                                )
                                .child(
                                    div()
                                        .id("monetization-close")
                                        .size(px(28.))
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(
                                            "modal_close",
                                            18.,
                                            "--interactive-text-default",
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_monetization_eligibility(cx);
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .id("monetization-body")
                                .flex_1()
                                .min_h(px(0.))
                                .overflow_y_scroll()
                                .p(px(16.))
                                .child(list),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// REST fetch key for accepting updated Creator Monetization terms (`710113`).
pub(crate) const ACCEPT_TERMS_KEY: &str = "server-monetization:accept-terms";
const MONETIZATION_TERMS_URL: &str = "https://support.discord.com/hc/en-us/articles/5330075836311";

/// Open Creator Monetization Terms Update Accept modal (module 710113).
#[derive(Clone, Debug)]
pub(crate) struct MonetizationTermsModal {
    pub guild_id: Id,
    pub demonetized: bool,
    pub busy: bool,
    pub error: Option<String>,
}

/// `POST /guilds/{id}/creator-monetization/accept-new-terms` or `-demonetized` (module 710113).
pub(crate) fn accept_monetization_terms_request(guild_id: Id, demonetized: bool) -> Request {
    let suffix = if demonetized {
        "accept-new-terms-demonetized"
    } else {
        "accept-new-terms"
    };
    Request::Rest {
        key: Some(ACCEPT_TERMS_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/guilds/{guild_id}/creator-monetization/{suffix}"),
        body: Some(json!({})),
    }
}

fn monetization_terms_styled(key: &str) -> gpui::StyledText {
    let runs = dc_intl::format_runs(
        key,
        &[(
            "termsURL",
            dc_intl::Arg::Str(std::borrow::Cow::Borrowed(MONETIZATION_TERMS_URL)),
        )],
    );
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (chunk, hook) in runs {
        let start = text.len();
        text.push_str(&chunk);
        if hook.as_deref() == Some("$link") && start < text.len() {
            highlights.push((
                start..text.len(),
                gpui::HighlightStyle {
                    color: Some(color("--text-link").into()),
                    ..gpui::HighlightStyle::default()
                },
            ));
        }
    }
    gpui::StyledText::new(text).with_highlights(highlights)
}

impl Root {
    /// Opens the Creator Monetization Terms Update modal (`710113`).
    #[allow(dead_code)]
    pub(super) fn open_monetization_terms_modal(
        &mut self,
        guild_id: Id,
        demonetized: bool,
        cx: &mut Context<Self>,
    ) {
        self.monetization_terms_modal = Some(MonetizationTermsModal {
            guild_id,
            demonetized,
            busy: false,
            error: None,
        });
        cx.notify();
    }

    /// Handles the REST answer for `accept-new-terms` / `accept-new-terms-demonetized`.
    pub(super) fn handle_monetization_terms_response(
        &mut self,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.monetization_terms_modal.as_mut() else {
            return;
        };
        modal.busy = false;
        let failed = body["error"].is_string() || body["status"].as_u64().is_some_and(|s| s >= 400);
        if failed {
            modal.error = Some(
                body["body"]["message"]
                    .as_str()
                    .unwrap_or_else(|| dc_intl::t("5mlOCW"))
                    .to_owned(),
            );
        } else {
            self.monetization_terms_modal = None;
        }
        cx.notify();
    }

    /// Modal layer for Creator Monetization Terms Update (`710113`).
    pub(super) fn monetization_terms_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.monetization_terms_modal.as_ref()?;
        let guild_id = modal.guild_id;
        let demonetized = modal.demonetized;
        let busy = modal.busy;

        Some(
            gpui::deferred(
                div()
                    .id("monetization-terms-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.monetization_terms_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("monetization-terms-modal")
                            .w(px(520.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .px(px(24.))
                                    .pt(px(20.))
                                    .pb(px(12.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(20.))
                                            .line_height(px(26.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("TP4GSt")),
                                    )
                                    .child(
                                        div()
                                            .id("monetization-terms-close")
                                            .flex_none()
                                            .size(px(32.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.monetization_terms_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(24.))
                                    .pb(px(20.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(14.))
                                    .child(
                                        div()
                                            .id("monetization-terms-intro")
                                            .cursor_pointer()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(monetization_terms_styled("c0IsON"))
                                            .on_click(|_, _, cx| {
                                                cx.open_url(MONETIZATION_TERMS_URL);
                                            }),
                                    )
                                    .child(
                                        div()
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-base-lower"))
                                            .p(px(14.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(12.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .text_size(px(14.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--text-strong"))
                                                            .child(dc_intl::t("UzDaWD")),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .line_height(px(18.))
                                                            .text_color(color("--text-default"))
                                                            .child(dc_intl::t("IDk8Fz")),
                                                    ),
                                            )
                                            .child(div().h(px(1.)).bg(color("--border-subtle")))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .text_size(px(14.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--text-strong"))
                                                            .child(dc_intl::t("36/RiT")),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .line_height(px(18.))
                                                            .text_color(color("--text-default"))
                                                            .child(dc_intl::t("CBzNtr")),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("monetization-terms-footer-note")
                                            .cursor_pointer()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-muted"))
                                            .child(monetization_terms_styled("+oNl6x"))
                                            .on_click(|_, _, cx| {
                                                cx.open_url(MONETIZATION_TERMS_URL);
                                            }),
                                    )
                                    .children(modal.error.as_ref().map(|err| {
                                        div()
                                            .rounded(px(6.))
                                            .border_1()
                                            .border_color(color("--status-danger"))
                                            .bg(rgba(0xda373c1a))
                                            .px(px(12.))
                                            .py(px(8.))
                                            .text_size(px(13.))
                                            .text_color(color("--text-feedback-critical"))
                                            .child(err.clone())
                                    })),
                            )
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(14.))
                                    .bg(color("--background-base-lower"))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .id("monetization-terms-later")
                                            .cursor_pointer()
                                            .px(px(14.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("Jj8nUC"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.monetization_terms_modal = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("monetization-terms-accept")
                                            .when(!busy, |d| d.cursor_pointer())
                                            .when(busy, |d| d.opacity(0.6))
                                            .px(px(20.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .bg(color("--control-primary-background-default"))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("pC/ozD"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if busy {
                                                    return;
                                                }
                                                if let Some(m) =
                                                    this.monetization_terms_modal.as_mut()
                                                {
                                                    m.busy = true;
                                                    m.error = None;
                                                }
                                                let _ = this.requests.send(
                                                    accept_monetization_terms_request(
                                                        guild_id,
                                                        demonetized,
                                                    ),
                                                );
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `monetization-terms [open|demonetized|error|close]` (module 710113).
    #[cfg(feature = "capture")]
    pub(crate) fn script_monetization_terms(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        match arg.trim() {
            "" | "open" => {
                self.monetization_terms_modal = Some(MonetizationTermsModal {
                    guild_id,
                    demonetized: false,
                    busy: false,
                    error: None,
                });
                cx.notify();
                "monetization-terms: open".to_owned()
            }
            "demonetized" => {
                self.monetization_terms_modal = Some(MonetizationTermsModal {
                    guild_id,
                    demonetized: true,
                    busy: false,
                    error: None,
                });
                cx.notify();
                "monetization-terms: demonetized".to_owned()
            }
            "error" => {
                self.monetization_terms_modal = Some(MonetizationTermsModal {
                    guild_id,
                    demonetized: false,
                    busy: false,
                    error: Some(dc_intl::t("5mlOCW").to_owned()),
                });
                cx.notify();
                "monetization-terms: error".to_owned()
            }
            "close" => {
                self.monetization_terms_modal = None;
                cx.notify();
                "monetization-terms: closed".to_owned()
            }
            other => format!("monetization-terms: unknown {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_715395_monetization_eligibility_strings_resolve() {
        for key in [
            "1lGNPZ", "D+gTJt", "HFY0m6", "xU2fl+", "+F8haD", "5BwC/O", "DW1Vae", "j7wXWo",
            "W0suNz", "up53zR", "mjbvWw", "9BV6L6", "Zwv84O", "EmoBD2", "k2UNz+", "Qw7qv4",
            "b45kGG", "NbtjEC", "Qvq39M", "azHboI", "u4rCYO", "bymfTb", "718pRA", "5ZqX+j",
            "NqVyFk", "VcDNIV", "7NzkfV", "BU4Diu",
        ] {
            assert!(
                dc_intl::has_key(key),
                "missing en-US key {key} in server_monetization 715395"
            );
        }
    }

    #[test]
    fn checklist_formats_weeks_days_and_sorts_ineligible_first() {
        let e = MonetizationEligibility {
            no_recent_violations: false,
            minimum_owner_age_in_years: Some(18),
            meets_owner_age_requirement: Some(true),
            minimum_size: Some(500),
            has_sufficient_members: Some(false),
            minimum_age_in_days: Some(21),
            meets_server_age_requirement: Some(true),
            weekly_communicators: Some(true),
            has_member_retention: Some(true),
            not_nsfw: true,
            has_enabled_2fa: Some(false),
        };
        let items = build_checklist(&e, false, false, true);
        assert_eq!(items.len(), 8);
        assert!(!items[0].checked);
        assert!(items.last().unwrap().checked);
        let age = items
            .iter()
            .find(|i| i.key == "server_age_requirement")
            .unwrap();
        assert!(age.description.contains("3"));
        let violations = items
            .iter()
            .find(|i| i.key == "no_violations_requirement")
            .unwrap();
        assert_eq!(violations.action, Some(EligibilityAction::ContactSupport));
        let mfa = items.iter().find(|i| i.key == "2fa_requirement").unwrap();
        assert_eq!(mfa.action, Some(EligibilityAction::Enable2fa));
    }
}
