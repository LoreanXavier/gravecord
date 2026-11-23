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

/// REST keys for Server Monetization Team Onboarding (`75834`).
pub(crate) const TEAMS_FETCH_KEY: &str = "server-monetization:teams";
pub(crate) const TEAM_ONBOARDING_SUBMIT_KEY: &str = "server-monetization:team-onboarding";
const CREATOR_POLICY_URL: &str = "https://support.discord.com/hc/en-us/articles/10575066024983";
const DEVELOPER_PORTAL_TEAMS_URL: &str = "https://discord.com/developers/teams";

#[derive(Clone, Debug)]
pub(crate) struct PayoutTeamOption {
    pub id: String,
    pub name: String,
}

/// Open Server Monetization Team Onboarding modal (module 75834: `QXf93A`, `GfObDE`, `geKm7t`,
/// `inJKQv`, `Jyy4pV`, `U1Vz24`, `JddVgE`, `+ALa7+`).
#[derive(Clone, Debug)]
pub(crate) struct MonetizationTeamOnboardingModal {
    pub guild_id: Id,
    pub require_team_setup: bool,
    pub teams: Vec<PayoutTeamOption>,
    pub selected_team_id: Option<String>,
    pub agreed: bool,
    pub dropdown_open: bool,
}

/// `GET /teams?include_payout_account_status=true` (module 75834).
pub(crate) fn fetch_payout_teams_request() -> Request {
    Request::Rest {
        key: Some(TEAMS_FETCH_KEY.to_owned()),
        method: reqwest::Method::GET,
        route: "/teams?include_payout_account_status=true".to_owned(),
        body: None,
    }
}

/// `POST /guilds/{guildId}/creator-monetization/ownership-transfer/onboarding` (module 75834).
pub(crate) fn submit_team_onboarding_request(guild_id: Id, team_id: Option<&str>) -> Request {
    Request::Rest {
        key: Some(TEAM_ONBOARDING_SUBMIT_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/guilds/{guild_id}/creator-monetization/ownership-transfer/onboarding"),
        body: Some(json!({ "team_id": team_id })),
    }
}

fn team_terms_agreement_styled() -> gpui::StyledText {
    let runs = dc_intl::format_runs(
        "+ALa7+",
        &[
            (
                "fullTermsUrl",
                dc_intl::Arg::Str(std::borrow::Cow::Borrowed(MONETIZATION_TERMS_URL)),
            ),
            (
                "creatorRevenuePolicyUrl",
                dc_intl::Arg::Str(std::borrow::Cow::Borrowed(CREATOR_POLICY_URL)),
            ),
        ],
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
    /// Opens the Server Monetization Team Onboarding modal (`75834`).
    #[allow(dead_code)]
    pub(super) fn open_monetization_team_onboarding(
        &mut self,
        guild_id: Id,
        require_team_setup: bool,
        cx: &mut Context<Self>,
    ) {
        if require_team_setup {
            let _ = self.requests.send(fetch_payout_teams_request());
        }
        self.monetization_team_onboarding_modal = Some(MonetizationTeamOnboardingModal {
            guild_id,
            require_team_setup,
            teams: Vec::new(),
            selected_team_id: None,
            agreed: false,
            dropdown_open: false,
        });
        cx.notify();
    }

    /// Modal layer for Server Monetization Team Onboarding (`75834`).
    pub(super) fn monetization_team_onboarding_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.monetization_team_onboarding_modal.as_ref()?;
        let needs_new_team = modal.require_team_setup && modal.teams.is_empty();
        let title = if modal.require_team_setup {
            dc_intl::t("inJKQv")
        } else {
            dc_intl::t("GfObDE")
        };
        let subtitle: Option<String> = if modal.require_team_setup {
            if modal.teams.is_empty() {
                let paras: Vec<String> = dc_intl::format_runs("Jyy4pV", &[])
                    .into_iter()
                    .map(|(chunk, _)| chunk.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                Some(paras.join("\n\n"))
            } else {
                Some(dc_intl::t("U1Vz24").to_owned())
            }
        } else {
            None
        };
        let selected_label = modal
            .selected_team_id
            .as_deref()
            .and_then(|sid| modal.teams.iter().find(|t| t.id == sid))
            .map(|t| t.name.clone())
            .unwrap_or_else(|| dc_intl::t("QXf93A").to_owned());
        let submit_enabled = if needs_new_team {
            true
        } else if modal.require_team_setup {
            modal.agreed && modal.selected_team_id.is_some()
        } else {
            modal.agreed
        };
        let guild_id = modal.guild_id;
        let selected_team_id = modal.selected_team_id.clone();

        Some(
            gpui::deferred(
                div()
                    .id("monetization-team-onboarding-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.monetization_team_onboarding_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("monetization-team-onboarding-modal")
                            .w(px(480.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            // Header
                            .child(
                                div()
                                    .px(px(24.))
                                    .pt(px(20.))
                                    .pb(px(12.))
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(20.))
                                            .line_height(px(25.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .id("monetization-team-onboarding-close")
                                            .flex_none()
                                            .size(px(30.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.monetization_team_onboarding_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // Body
                            .child(
                                div()
                                    .px(px(24.))
                                    .pb(px(20.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(16.))
                                    .children(subtitle.map(|sub| {
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-default"))
                                            .child(sub)
                                    }))
                                    .when(modal.require_team_setup && !modal.teams.is_empty(), |col| {
                                        col.child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(6.))
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .font_weight(FontWeight(700.))
                                                        .text_color(color("--text-muted"))
                                                        .child(dc_intl::t("QXf93A").to_uppercase()),
                                                )
                                                .child(
                                                    div()
                                                        .id("monetization-team-select")
                                                        .relative()
                                                        .cursor_pointer()
                                                        .h(px(40.))
                                                        .px(px(12.))
                                                        .rounded(px(8.))
                                                        .border_1()
                                                        .border_color(color("--border-subtle"))
                                                        .bg(color("--background-base-lowest"))
                                                        .flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .child(
                                                            div()
                                                                .text_size(px(14.))
                                                                .text_color(
                                                                    if modal.selected_team_id.is_some()
                                                                    {
                                                                        color("--text-strong")
                                                                    } else {
                                                                        color("--text-muted")
                                                                    },
                                                                )
                                                                .child(selected_label),
                                                        )
                                                        .child(icon(
                                                            "shop_select_chevron",
                                                            16.,
                                                            "--icon-subtle",
                                                        ))
                                                        .child(self.anchor_probe(
                                                            "monetization-team-select",
                                                        ))
                                                        .when(modal.dropdown_open, |sel| {
sel.child(gpui::deferred(div()
                                                            .id("monetization-team-list")
.occlude()
.absolute()
.left_0()
.right_0()
.top(px(44.))
                                                            .rounded(px(8.))
                                                            .border_1()
                                                            .border_color(color("--border-subtle"))
                                                            .bg(color("--background-base-lower"))
.shadow(vec![crate::ui::shadow_high()])
                                                            .p(px(4.))
                                                            .flex()
                                                            .flex_col()
                                                            .gap(px(2.))
                                                            // `maxOptionsVisible` 5 (module 691885): five 38px rows, 2px gaps, 4px padding.
                                                            .max_h(px(206.))
                                                            .overflow_y_scroll()
                                                            .on_click(|_, _, cx| cx.stop_propagation())
                                                            .on_mouse_down_out(cx.listener(
                                                                |this,
                                                                 event: &gpui::MouseDownEvent,
                                                                 _,
                                                                 cx| {
                                                                    let on_trigger = this
                                                                        .tip_anchors
                                                                        .borrow()
                                                                        .get("monetization-team-select")
                                                                        .is_some_and(|field| {
                                                                            field.contains(&event.position)
                                                                        });
                                                                    if !on_trigger
                                                                        && let Some(m) = this
                                                                            .monetization_team_onboarding_modal
                                                                            .as_mut()
                                                                        && m.dropdown_open
                                                                    {
                                                                        m.dropdown_open = false;
                                                                        cx.notify();
                                                                    }
                                                                },
                                                            ))
                                                            .children(modal.teams.iter().enumerate().map(
                                                                |(idx, team)| {
                                                                    let tid = team.id.clone();
                                                                    let active = modal
                                                                        .selected_team_id
                                                                        .as_deref()
                                                                        == Some(team.id.as_str());
                                                                    div()
                                                                        .id((
                                                                            "monetization-team-opt",
                                                                            idx,
                                                                        ))
                                                                        .flex_none()
                                                                        .cursor_pointer()
                                                                        .px(px(10.))
                                                                        .py(px(8.))
                                                                        .rounded(px(6.))
                                                                        .bg(if active {
                                                                            color("--background-mod-subtle")
                                                                        } else {
                                                                            rgba(0x00000000)
                                                                        })
                                                                        .hover(|s| {
                                                                            s.bg(color(
                                                                                "--background-mod-subtle",
                                                                            ))
                                                                        })
                                                                        .text_size(px(14.))
                                                                        .text_color(color(
                                                                            "--text-strong",
                                                                        ))
                                                                        .child(team.name.clone())
                                                                        .on_click(cx.listener(
                                                                            move |this, _, _, cx| {
                                                                                if let Some(m) = this
                                                                                    .monetization_team_onboarding_modal
                                                                                    .as_mut()
                                                                                {
                                                                                    m.selected_team_id =
                                                                                        Some(tid.clone());
                                                                                    m.dropdown_open = false;
                                                                                }
                                                                                cx.notify();
                                                                            },
                                                                        ))
                                                                },
                                                            ))).with_priority(99))
})
.on_click(cx.listener(|this, _, _, cx| {
                                                            if let Some(m) = this
                                                                .monetization_team_onboarding_modal
                                                                .as_mut()
                                                            {
                                                                m.dropdown_open = !m.dropdown_open;
                                                            }
                                                            cx.notify();
                                                        })),
                                                )
                                                ,
                                        )
                                    })
                                    .when(!needs_new_team, |col| {
                                        col.child(
                                            div()
                                                .id("monetization-team-agree")
                                                .cursor_pointer()
                                                .flex()
                                                .items_start()
                                                .gap(px(10.))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(m) = this
                                                        .monetization_team_onboarding_modal
                                                        .as_mut()
                                                    {
                                                        m.agreed = !m.agreed;
                                                    }
                                                    cx.notify();
                                                }))
                                                .child(super::server_games::checkbox(modal.agreed))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_size(px(13.))
                                                        .line_height(px(18.))
                                                        .text_color(color("--text-default"))
                                                        .child(team_terms_agreement_styled()),
                                                ),
                                        )
                                    }),
                            )
                            // Footer
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(14.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("monetization-team-onboarding-submit")
                                            .when(submit_enabled, |b| b.cursor_pointer())
                                            .when(!submit_enabled, |b| b.opacity(0.5))
                                            .px(px(18.))
                                            .h(px(38.))
                                            .rounded(px(6.))
                                            .bg(color("--control-primary-background-default"))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(if needs_new_team {
                                                dc_intl::t("JddVgE")
                                            } else {
                                                dc_intl::t("geKm7t")
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !submit_enabled {
                                                    return;
                                                }
                                                if needs_new_team {
                                                    cx.open_url(DEVELOPER_PORTAL_TEAMS_URL);
                                                } else {
                                                    let _ = this.requests.send(
                                                        submit_team_onboarding_request(
                                                            guild_id,
                                                            selected_team_id.as_deref(),
                                                        ),
                                                    );
                                                    this.monetization_team_onboarding_modal = None;
                                                    this.put_app_toast(
                                                        dc_intl::t("geKm7t").to_owned(),
                                                        true,
                                                        cx,
                                                    );
                                                }
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

    /// Capture harness: `monetization-team [teams|empty|terms|close]` (module 75834).
    #[cfg(feature = "capture")]
    pub(crate) fn script_monetization_team(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let gid = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        match arg.trim() {
            "close" => {
                self.monetization_team_onboarding_modal = None;
                self.monetization_onboarding_modal = None;
                cx.notify();
                "monetization-team: closed".to_owned()
            }
            "earn" | "earn-ineligible" | "earn-appeal" | "earn-pending" => {
                self.monetization_team_onboarding_modal = None;
                self.monetization_onboarding_modal = Some(MonetizationOnboardingModal {
                    guild_id: gid,
                    eligible: arg.trim() != "earn-ineligible",
                    terms_checked: arg.trim() == "earn",
                    can_submit_owner: true,
                    v1_rejected: matches!(arg.trim(), "earn-appeal" | "earn-pending"),
                    app_pending: arg.trim() == "earn-pending",
                    appeal_notice: None,
                    error: None,
                });
                cx.notify();
                format!("monetization-team: {}", arg.trim())
            }
            "empty" => {
                self.monetization_team_onboarding_modal = Some(MonetizationTeamOnboardingModal {
                    guild_id: gid,
                    require_team_setup: true,
                    teams: Vec::new(),
                    selected_team_id: None,
                    agreed: false,
                    dropdown_open: false,
                });
                cx.notify();
                "monetization-team: empty".to_owned()
            }
            "terms" => {
                self.monetization_team_onboarding_modal = Some(MonetizationTeamOnboardingModal {
                    guild_id: gid,
                    require_team_setup: false,
                    teams: Vec::new(),
                    selected_team_id: None,
                    agreed: true,
                    dropdown_open: false,
                });
                cx.notify();
                "monetization-team: terms".to_owned()
            }
            _ => {
                self.monetization_team_onboarding_modal = Some(MonetizationTeamOnboardingModal {
                    guild_id: gid,
                    require_team_setup: true,
                    teams: vec![
                        PayoutTeamOption {
                            id: "team_1".to_owned(),
                            name: "Gravecord Creators LLC".to_owned(),
                        },
                        PayoutTeamOption {
                            id: "team_2".to_owned(),
                            name: "Indie Guild Studio".to_owned(),
                        },
                    ],
                    selected_team_id: Some("team_1".to_owned()),
                    agreed: true,
                    dropdown_open: false,
                });
                cx.notify();
                "monetization-team: teams".to_owned()
            }
        }
    }
}

/// Creator Monetization "Earn money on Discord / Start Earning" onboarding card modal (module 890290:
/// `PqYfh/`, `41wkMc`, `Af4klP`, `NL5ZNS`, `OrkTBn`, `YKw/NQ`; `POST /guilds/{id}/creator-monetization/accept-terms`,
/// `POST /guilds/{id}/creator-monetization/enable-requests`).
#[derive(Clone, Debug)]
pub(crate) struct MonetizationOnboardingModal {
    pub guild_id: Id,
    pub eligible: bool,
    pub terms_checked: bool,
    pub can_submit_owner: bool,
    pub v1_rejected: bool,
    pub app_pending: bool,
    pub appeal_notice: Option<String>,
    pub error: Option<String>,
}

/// Renders the Creator Monetization "Earn money on Discord" onboarding modal (module 890290).
impl Root {
    pub(in crate::ui) fn monetization_onboarding_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.monetization_onboarding_modal.as_ref()?;
        let gid = modal.guild_id;
        let _art_alt = dc_intl::t("Af4klP");
        const COINS_ART: &str = "https://cdn.discordapp.com/assets/content/2fb67de5cada50399335a03ac11b8e83271559ab2335135f2727d6a40c51e4a6.svg";
        let _ = COINS_ART;

        let cta_section: AnyElement = if !modal.eligible {
            // State A: ineligible — Start Earning opens Eligibility Checklist (267912 / 715395).
            div()
                .id("monetization-earn-ineligible")
                .self_start()
                .h(px(38.))
                .px(px(16.))
                .rounded(px(8.))
                .bg(color("--control-primary-background-default"))
                .hover(|s| s.bg(color("--control-primary-background-hover")))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(14.))
                .font_weight(FontWeight(600.))
                .text_color(color("--white"))
                .child(icon("circle_info", 16., "--white"))
                .child(dc_intl::t("NL5ZNS"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.monetization_onboarding_modal = None;
                    this.open_monetization_eligibility(gid, cx);
                }))
                .into_any_element()
        } else if !modal.v1_rejected {
            // State B: eligible — Terms checkbox + Start Earning (`POST /accept-terms`).
            let can_go = modal.terms_checked && modal.can_submit_owner;
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(
                    div()
                        .id("monetization-earn-terms-check")
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(m) = this.monetization_onboarding_modal.as_mut() {
                                m.terms_checked = !m.terms_checked;
                            }
                            cx.notify();
                        }))
                        .child(super::server_games::checkbox(modal.terms_checked))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(
                                    "I accept the Server Monetization Terms and Creator Policy.",
                                ),
                        ),
                )
                .child(
                    div()
                        .id("monetization-earn-accept")
                        .self_start()
                        .h(px(38.))
                        .px(px(18.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .when(!can_go, |b| b.opacity(0.5))
                        .when(can_go, |b| {
                            b.cursor_pointer()
                                .hover(|s| s.bg(color("--control-primary-background-hover")))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let _ = this.requests.send(crate::net::Request::Rest {
                                        key: Some(format!("monetization:accept-terms:{gid}")),
                                        method: reqwest::Method::POST,
                                        route: format!(
                                            "/guilds/{gid}/creator-monetization/accept-terms"
                                        ),
                                        body: None,
                                    });
                                    this.monetization_onboarding_modal = None;
                                    cx.notify();
                                }))
                        })
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("NL5ZNS")),
                )
                .when_some(modal.error.clone(), |col, err| {
                    col.child(
                        div()
                            .p(px(10.))
                            .rounded(px(6.))
                            .bg(rgba(0xf23f421a))
                            .border_1()
                            .border_color(color("--status-danger"))
                            .text_size(px(13.))
                            .text_color(color("--text-feedback-critical"))
                            .child(err),
                    )
                })
                .into_any_element()
        } else if modal.app_pending {
            // State C1: appeal pending review (`OrkTBn`).
            div()
                .p(px(12.))
                .rounded(px(8.))
                .bg(rgba(0xf0b2321a))
                .border_1()
                .border_color(color("--status-warning"))
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(dc_intl::t("OrkTBn"))
                .into_any_element()
        } else {
            // State C2: rejected in V1 with appeal CTA (`YKw/NQ` -> `POST /enable-requests`).
            let notice = modal.appeal_notice.clone().unwrap_or_else(|| {
                "Your previous request was not approved. You can submit an appeal for review."
                    .to_owned()
            });
            div()
                .p(px(12.))
                .rounded(px(8.))
                .bg(rgba(0xf0b2321a))
                .border_1()
                .border_color(color("--status-warning"))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(notice),
                )
                .child(
                    div()
                        .id("monetization-earn-appeal")
                        .self_start()
                        .h(px(34.))
                        .px(px(14.))
                        .rounded(px(6.))
                        .bg(color("--control-secondary-background-default"))
                        .hover(|s| s.bg(color("--control-secondary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("YKw/NQ"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let _ = this.requests.send(crate::net::Request::Rest {
                                key: Some(format!("monetization:enable-request:{gid}")),
                                method: reqwest::Method::POST,
                                route: format!(
                                    "/guilds/{gid}/creator-monetization/enable-requests"
                                ),
                                body: None,
                            });
                            if let Some(m) = this.monetization_onboarding_modal.as_mut() {
                                m.app_pending = true;
                            }
                            cx.notify();
                        })),
                )
                .into_any_element()
        };

        let card = div()
            .id("monetization-onboarding-modal")
            .occlude()
            .w(px(640.))
            .p(px(24.))
            .rounded(px(12.))
            .bg(color("--modal-background"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .gap(px(24.))
            .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
            // Left coins illustration box
            .child(
                div()
                    .flex_none()
                    .w(px(200.))
                    .h(px(160.))
                    .rounded(px(10.))
                    .bg(gpui::linear_gradient(
                        135.,
                        gpui::linear_color_stop(rgba(0x5865f233), 0.),
                        gpui::linear_color_stop(rgba(0xf0b23233), 1.),
                    ))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(
                        "storefront_store",
                        52.,
                        "--control-primary-background-default",
                    )),
            )
            // Right content (`PqYfh/`, `41wkMc`, CTA)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .line_height(px(28.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("PqYfh/")),
                            )
                            .child(
                                div()
                                    .id("monetization-earn-close")
                                    .size(px(28.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 16., "--interactive-icon-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.monetization_onboarding_modal = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t("41wkMc")),
                    )
                    .child(div().mt(px(4.)).child(cta_section)),
            );

        Some(
            gpui::deferred(
                div()
                    .id("monetization-onboarding-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.monetization_onboarding_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(96)
            .into_any_element(),
        )
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
