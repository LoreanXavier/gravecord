//! Server Subscriptions Storefront Page (module 602844, past-due banner module 456540,
//! pending tier change banner module 815332).
//!
//! Fetches `GET /guilds/{id}/role-subscriptions/group-listings` and
//! `GET /guilds/{id}/role-subscriptions/settings`, displays the guild's Server Subscriptions
//! storefront (`KzCF/6`, `rm+ojA`) with tier cards (`role_subscription_card.rs`), past-due
//! alert (`eSuJE2` + `RXsxqb` "Manage Subscription"), pending tier change notice (`chw89X` +
//! `De4wI8` "Undo Change"), and the empty/unavailable states: Mobile payment unavailable
//! (`atZQVZ` / `teWe5G`), Not a member (`gAdWBl` / `X/yMrK`), and Going Away (`t18lFj` /
//! `TvGJgk` / `MvLOqp`).

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, StyledText, deferred, div,
    prelude::*, px, rgba,
};
use serde_json::Value;
use std::borrow::Cow;

pub(crate) const GROUP_LISTINGS_KEY: &str = "role-subs:group-listings";
pub(crate) const SUBS_SETTINGS_KEY: &str = "role-subs:settings";

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct StorefrontTier {
    pub id: String,
    pub name: String,
    pub price_label: String,
    pub description: String,
    pub perks: Vec<String>,
    pub subscribed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingTierChange {
    pub current_listing: String,
    pub next_listing: String,
    pub change_date: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum StorefrontUnavailable {
    MobilePaymentUnavailable,
    NotMember,
    GoingAway,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct ServerSubscriptionsPage {
    pub guild_id: Id,
    pub server_name: String,
    pub description: String,
    pub past_due: bool,
    pub pending_change: Option<PendingTierChange>,
    pub unavailable: Option<StorefrontUnavailable>,
    pub tiers: Vec<StorefrontTier>,
}

/// `GET /guilds/{id}/role-subscriptions/group-listings` (module 602844).
pub(crate) fn group_listings_request(guild_id: Id) -> Request {
    Request::Rest {
        key: Some(GROUP_LISTINGS_KEY.to_owned()),
        method: reqwest::Method::GET,
        route: format!("/guilds/{guild_id}/role-subscriptions/group-listings"),
        body: None,
    }
}

/// `GET /guilds/{id}/role-subscriptions/settings` (module 602844).
pub(crate) fn settings_request(guild_id: Id) -> Request {
    Request::Rest {
        key: Some(SUBS_SETTINGS_KEY.to_owned()),
        method: reqwest::Method::GET,
        route: format!("/guilds/{guild_id}/role-subscriptions/settings"),
        body: None,
    }
}

fn bold_styled(runs: Vec<(String, Option<Box<str>>)>) -> StyledText {
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (chunk, hook) in runs {
        let start = text.len();
        text.push_str(&chunk);
        if hook.as_deref() == Some("$b") && start < text.len() {
            highlights.push((
                start..text.len(),
                HighlightStyle {
                    font_weight: Some(FontWeight(700.)),
                    color: Some(color("--text-strong").into()),
                    ..HighlightStyle::default()
                },
            ));
        }
    }
    StyledText::new(text).with_highlights(highlights)
}

impl Root {
    /// Opens the Server Subscriptions Storefront Page (`602844`) for `guild_id`.
    #[allow(dead_code)]
    pub(super) fn open_server_subscriptions_page(&mut self, guild_id: Id, cx: &mut Context<Self>) {
        let server_name = self
            .stores
            .guild(guild_id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "Server".to_owned());
        self.server_subscriptions_page = Some(ServerSubscriptionsPage {
            guild_id,
            server_name,
            description: String::new(),
            past_due: false,
            pending_change: None,
            unavailable: None,
            tiers: Vec::new(),
        });
        let _ = self.requests.send(group_listings_request(guild_id));
        let _ = self.requests.send(settings_request(guild_id));
        cx.notify();
    }

    /// Handles REST responses for `group-listings` and `settings`.
    pub(super) fn handle_server_subscriptions_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = self.server_subscriptions_page.as_mut() else {
            return;
        };
        if key == GROUP_LISTINGS_KEY {
            let root = &body["body"];
            if let Some(desc) = root["description"].as_str() {
                page.description = desc.to_owned();
            }
            if let Some(arr) = root["subscription_listings"].as_array() {
                page.tiers = arr
                    .iter()
                    .filter_map(|item| {
                        let id = item["id"].as_str()?.to_owned();
                        let name = item["name"].as_str().unwrap_or("Tier").to_owned();
                        let description = item["description"].as_str().unwrap_or("").to_owned();
                        let cents = item["price_tier"].as_u64().unwrap_or(499);
                        let price_label = format!("${:.2}/mo", cents as f64 / 100.);
                        let perks = item["role_benefits"]["benefits"]
                            .as_array()
                            .map(|b| {
                                b.iter()
                                    .filter_map(|x| x["name"].as_str().map(str::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default();
                        Some(StorefrontTier {
                            id,
                            name,
                            price_label,
                            description,
                            perks,
                            subscribed: false,
                        })
                    })
                    .collect();
            }
            cx.notify();
        } else if key == SUBS_SETTINGS_KEY {
            if let Some(desc) = body["body"]["description"].as_str()
                && !desc.is_empty()
            {
                page.description = desc.to_owned();
            }
            cx.notify();
        }
    }

    /// Modal/page layer for Server Subscriptions Storefront (`602844` + `456540`).
    pub(super) fn server_subscriptions_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let page = self.server_subscriptions_page.as_ref()?;
        let subtitle_runs = dc_intl::format_runs(
            "rm+ojA",
            &[("serverName", Arg::Str(Cow::Borrowed(&page.server_name)))],
        );

        let body: AnyElement = if let Some(reason) = page.unavailable {
            let (title, subtitle, aria) = match reason {
                StorefrontUnavailable::MobilePaymentUnavailable => (
                    dc_intl::t("atZQVZ"),
                    dc_intl::t("teWe5G"),
                    dc_intl::t("MvLOqp"),
                ),
                StorefrontUnavailable::NotMember => (
                    dc_intl::t("gAdWBl"),
                    dc_intl::t("X/yMrK"),
                    dc_intl::t("MvLOqp"),
                ),
                StorefrontUnavailable::GoingAway => (
                    dc_intl::t("t18lFj"),
                    dc_intl::t("TvGJgk"),
                    dc_intl::t("MvLOqp"),
                ),
            };
            div()
                .p(px(36.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .id("server-subs-unavailable-art")
                        .aria_label(aria)
                        .size(px(64.))
                        .rounded_full()
                        .bg(rgba(0x5865f226))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("storefront_store", 32., "--white")),
                )
                .child(
                    div()
                        .text_size(px(18.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .text_center()
                        .child(title),
                )
                .child(
                    div()
                        .max_w(px(460.))
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .text_center()
                        .child(subtitle),
                )
                .into_any_element()
        } else {
            let past_due_banner = page.past_due.then(|| {
                // Module 456540: past-due subscription notice (`eSuJE2`, `RXsxqb`)
                div()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--status-danger"))
                    .bg(rgba(0xda373c1f))
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(icon("circle_info", 18., "--status-danger"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("eSuJE2")),
                            ),
                    )
                    .child(
                        div()
                            .id("server-subs-manage-past-due")
                            .flex_none()
                            .cursor_pointer()
                            .px(px(14.))
                            .h(px(32.))
                            .rounded(px(6.))
                            .bg(color("--control-critical-primary-background-default"))
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("RXsxqb"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.server_subscriptions_page = None;
                                this.open_settings_section("Subscriptions", cx);
                            })),
                    )
            });

            let pending_banner = page.pending_change.as_ref().map(|pc| {
                // Module 815332: pending tier change notice (`chw89X`, `De4wI8`)
                let runs = dc_intl::format_runs(
                    "chw89X",
                    &[
                        (
                            "currentListing",
                            Arg::Str(Cow::Borrowed(&pc.current_listing)),
                        ),
                        ("nextListing", Arg::Str(Cow::Borrowed(&pc.next_listing))),
                        ("changeDate", Arg::Str(Cow::Borrowed(&pc.change_date))),
                    ],
                );
                div()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--status-warning"))
                    .bg(rgba(0xf0b2321f))
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(bold_styled(runs)),
                    )
                    .child(
                        div()
                            .id("server-subs-undo-change")
                            .flex_none()
                            .cursor_pointer()
                            .px(px(14.))
                            .h(px(32.))
                            .rounded(px(6.))
                            .bg(color("--control-secondary-background-default"))
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(dc_intl::t("De4wI8"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(p) = this.server_subscriptions_page.as_mut() {
                                    p.pending_change = None;
                                }
                                cx.notify();
                            })),
                    )
            });

            let tier_cards = page.tiers.iter().enumerate().map(|(idx, tier)| {
                let tier_name = tier.name.clone();
                let subscribed = tier.subscribed;
                div()
                    .id(format!("server-subs-tier-{idx}"))
                    .flex_1()
                    .min_w(px(200.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(if subscribed {
                        color("--control-primary-background-default")
                    } else {
                        color("--border-subtle")
                    })
                    .bg(color("--background-base-lower"))
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(16.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .truncate()
                                            .child(tier.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-link"))
                                            .child(tier.price_label.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(tier.description.clone()),
                            )
                            .child(div().pt(px(4.)).flex().flex_col().gap(px(4.)).children(
                                tier.perks.iter().map(|perk| {
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(icon("circle_check", 14., "--status-positive"))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(px(12.))
                                                .text_color(color("--text-default"))
                                                .child(perk.clone()),
                                        )
                                }),
                            )),
                    )
                    .child(
                        div()
                            .id(format!("server-subs-tier-cta-{idx}"))
                            .cursor_pointer()
                            .w_full()
                            .h(px(34.))
                            .rounded(px(6.))
                            .bg(if subscribed {
                                color("--control-secondary-background-default")
                            } else {
                                color("--control-primary-background-default")
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(if subscribed {
                                color("--control-secondary-text-default")
                            } else {
                                color("--white")
                            })
                            .child(if subscribed {
                                dc_intl::t("RXsxqb").to_owned()
                            } else {
                                format!("Subscribe to {tier_name}")
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if subscribed {
                                    this.server_subscriptions_page = None;
                                    this.open_settings_section("Subscriptions", cx);
                                } else {
                                    this.put_app_toast(
                                        format!("Selected {tier_name} subscription tier."),
                                        true,
                                        cx,
                                    );
                                }
                            })),
                    )
            });

            div()
                .p(px(24.))
                .flex()
                .flex_col()
                .gap(px(16.))
                .children(past_due_banner)
                .children(pending_banner)
                .when(!page.description.is_empty(), |col| {
                    col.child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(page.description.clone()),
                    )
                })
                .child(div().flex().flex_row().gap(px(14.)).children(tier_cards))
                .into_any_element()
        };

        Some(
            deferred(
                div()
                    .id("server-subscriptions-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.server_subscriptions_page = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("server-subscriptions-modal")
                            .w(px(760.))
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
                                    .py(px(18.))
                                    .border_b_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
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
                                                    .child(dc_intl::t("KzCF/6")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .line_height(px(18.))
                                                    .text_color(color("--text-muted"))
                                                    .child(bold_styled(subtitle_runs)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("server-subscriptions-close")
                                            .flex_none()
                                            .size(px(32.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.server_subscriptions_page = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(body),
                    ),
            )
            .with_priority(95)
            .into_any_element(),
        )
    }

    /// Capture harness: `server-subs [storefront|past-due|mobile|not-member|going-away|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_server_subscriptions(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        let server_name = self
            .stores
            .guild(guild_id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "Gravecord Creators".to_owned());
        let sample_tiers = vec![
            StorefrontTier {
                id: "tier_1".to_owned(),
                name: "Supporter".to_owned(),
                price_label: "$4.99/mo".to_owned(),
                description: "Support the community and unlock subscriber emojis.".to_owned(),
                perks: vec![
                    "Exclusive #supporters channel".to_owned(),
                    "Custom role color & badge".to_owned(),
                ],
                subscribed: true,
            },
            StorefrontTier {
                id: "tier_2".to_owned(),
                name: "VIP Creator".to_owned(),
                price_label: "$9.99/mo".to_owned(),
                description: "All Supporter perks plus backstage voice and monthly Q&A.".to_owned(),
                perks: vec![
                    "All Supporter benefits".to_owned(),
                    "Backstage Stage access".to_owned(),
                    "Priority feature polls".to_owned(),
                ],
                subscribed: false,
            },
        ];
        let mut page = ServerSubscriptionsPage {
            guild_id,
            server_name,
            description: "Unlock exclusive channels, badges, and behind-the-scenes updates."
                .to_owned(),
            past_due: false,
            pending_change: None,
            unavailable: None,
            tiers: sample_tiers,
        };
        match arg.trim() {
            "" | "storefront" => {
                page.pending_change = Some(PendingTierChange {
                    current_listing: "Supporter".to_owned(),
                    next_listing: "VIP Creator".to_owned(),
                    change_date: "Dec 1, 2026".to_owned(),
                });
                self.server_subscriptions_page = Some(page);
                cx.notify();
                "server-subs: storefront".to_owned()
            }
            "past-due" => {
                page.past_due = true;
                self.server_subscriptions_page = Some(page);
                cx.notify();
                "server-subs: past-due".to_owned()
            }
            "mobile" => {
                page.unavailable = Some(StorefrontUnavailable::MobilePaymentUnavailable);
                self.server_subscriptions_page = Some(page);
                cx.notify();
                "server-subs: mobile".to_owned()
            }
            "not-member" => {
                page.unavailable = Some(StorefrontUnavailable::NotMember);
                self.server_subscriptions_page = Some(page);
                cx.notify();
                "server-subs: not-member".to_owned()
            }
            "going-away" => {
                page.unavailable = Some(StorefrontUnavailable::GoingAway);
                self.server_subscriptions_page = Some(page);
                cx.notify();
                "server-subs: going-away".to_owned()
            }
            "close" => {
                self.server_subscriptions_page = None;
                cx.notify();
                "server-subs: closed".to_owned()
            }
            other => format!("server-subs: unknown {other}"),
        }
    }
}
