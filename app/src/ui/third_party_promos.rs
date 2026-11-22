//! Third-party partner promotions modal (`452417` `ThirdPartyPromotionsModal`: locked preview `ei`,
//! unlocked modal `er`, recurring expander `et`) and partner perk cards (`116011`: perk card `wx`,
//! monthly countdown bar `vw`, styles `341973` `_5cFAmt`).
//!
//! REST endpoints (`412260` / `158476` / `264779`):
//! - `GET /storefront/promotions?locale={locale}&platform=0`
//! - `GET /users/@me/outbound-promotions/codes?locale={locale}`
//! - `POST /outbound-promotions/{promotionId}/claim`

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::premium::PremiumType;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use serde_json::Value;

pub(super) const PROMOS_PREFIX: &str = "third-party-promos:";
const PROMOS_LIST_KEY: &str = "third-party-promos:list";
const PROMOS_CODES_KEY: &str = "third-party-promos:codes";
const PROMOS_CLAIM_PREFIX: &str = "third-party-promos:claim:";

/// Promotion type enum from module `852218`.
pub(crate) const PROMO_TYPE_THIRD_PARTY_OUTBOUND: u64 = 4;
pub(crate) const PROMO_TYPE_THIRD_PARTY_OUTBOUND_RECURRING: u64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PartnerId {
    SteelSeries,
    Logitech,
    YouTube,
    BestBuy,
    CallOfDuty,
    Xbox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PartnerConfig {
    pub id: PartnerId,
    pub title_key: &'static str,
    pub body_key: &'static str,
    pub claimed_body_key: Option<&'static str>,
    pub claim_label_key: &'static str,
    pub redeem_label_key: &'static str,
    pub discount_percentage: Option<u32>,
    pub landing_url: &'static str,
    pub asset_url: &'static str,
    pub show_section_headers: bool,
}

pub(crate) fn partner_config(partner: PartnerId) -> PartnerConfig {
    match partner {
        PartnerId::SteelSeries => PartnerConfig {
            id: PartnerId::SteelSeries,
            title_key: "p7BkHh",
            body_key: "nmvvaN",
            claimed_body_key: Some("w8CXUl"),
            claim_label_key: "vwASIl",
            redeem_label_key: "DF68t7",
            discount_percentage: Some(20),
            landing_url: "https://steelseries.com/",
            asset_url: "https://cdn.discordapp.com/assets/content/6bbac5a155872455d86969ce5309725d9aa0317ba3b104b3027716e70040e11f.webp",
            show_section_headers: true,
        },
        PartnerId::Logitech => PartnerConfig {
            id: PartnerId::Logitech,
            title_key: "PF1aT5",
            body_key: "KoKwMn",
            claimed_body_key: None,
            claim_label_key: "vwASIl",
            redeem_label_key: "DF68t7",
            discount_percentage: Some(30),
            landing_url: "https://www.logitech.com/",
            asset_url: "https://cdn.discordapp.com/assets/content/c93472f5033e3079579ad496c9a54a95faa23623f5b1d11590d536c861f52e7c.svg",
            show_section_headers: true,
        },
        PartnerId::YouTube => PartnerConfig {
            id: PartnerId::YouTube,
            title_key: "CwMGMb",
            body_key: "TgHy6p",
            claimed_body_key: None,
            claim_label_key: "vwASIl",
            redeem_label_key: "KfOPbQ",
            discount_percentage: None,
            landing_url: "https://www.youtube.com/redeem",
            asset_url: "https://cdn.discordapp.com/assets/content/9fc3dd18f3074d23a9bf78be5287bf7a727597c962ea220a0ebda311fd7997ea.webp",
            show_section_headers: false,
        },
        PartnerId::BestBuy => PartnerConfig {
            id: PartnerId::BestBuy,
            title_key: "ecPCdG",
            body_key: "evF0PO",
            claimed_body_key: None,
            claim_label_key: "vwASIl",
            redeem_label_key: "DF68t7",
            discount_percentage: None,
            landing_url: "https://www.bestbuy.com/",
            asset_url: "https://cdn.discordapp.com/assets/content/a95525ff6082a74538cbdc2d99c11fb59fe43718b018db01cfc02114785ad8fe.png",
            show_section_headers: false,
        },
        PartnerId::CallOfDuty => PartnerConfig {
            id: PartnerId::CallOfDuty,
            title_key: "xDMrVa",
            body_key: "LUOF3v",
            claimed_body_key: None,
            claim_label_key: "lbyFG+",
            redeem_label_key: "6rwUm2",
            discount_percentage: None,
            landing_url: "https://www.callofduty.com/redeem",
            asset_url: "https://cdn.discordapp.com/assets/content/6bbac5a155872455d86969ce5309725d9aa0317ba3b104b3027716e70040e11f.webp",
            show_section_headers: false,
        },
        PartnerId::Xbox => PartnerConfig {
            id: PartnerId::Xbox,
            title_key: "oDfh3O",
            body_key: "nDEuO1",
            claimed_body_key: None,
            claim_label_key: "vwASIl",
            redeem_label_key: "KfOPbQ",
            discount_percentage: None,
            landing_url: "https://www.xbox.com/",
            asset_url: "https://cdn.discordapp.com/assets/content/7ce3849519c8d8cd4657b08bd2c689ea934bb60f53b959a04eb3b1db5d2f002a.png",
            show_section_headers: false,
        },
    }
}

pub(crate) fn format_partner_title(cfg: &PartnerConfig) -> String {
    match cfg.discount_percentage {
        Some(pct) => dc_intl::format(
            cfg.title_key,
            &[("discountPercentage", dc_intl::Arg::Num(pct as f64))],
        ),
        None => dc_intl::t(cfg.title_key).to_owned(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OutboundPromotion {
    pub id: String,
    pub title: String,
    pub body: String,
    pub promotion_type: u64,
    pub partner: PartnerId,
    pub landing_url: String,
    pub next_drop_days: Option<u32>,
    pub unavailable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClaimedPromoCode {
    pub promotion_id: String,
    pub code: String,
    pub claimed_at: String,
}

/// Parses `GET /storefront/promotions` keeping third-party outbound (`4`) and recurring (`7`) promos.
pub(crate) fn parse_outbound_promotions(value: &Value) -> Vec<OutboundPromotion> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let promotion_type = item["promotion_type"].as_u64().unwrap_or(0);
            if promotion_type != PROMO_TYPE_THIRD_PARTY_OUTBOUND
                && promotion_type != PROMO_TYPE_THIRD_PARTY_OUTBOUND_RECURRING
            {
                return None;
            }
            let id = item["id"].as_str()?.to_owned();
            let title = item["outbound_title"]
                .as_str()
                .or_else(|| item["title"].as_str())
                .unwrap_or_default()
                .to_owned();
            let body = item["outbound_redemption_page_link_text"]
                .as_str()
                .or_else(|| item["body"].as_str())
                .unwrap_or_default()
                .to_owned();
            let landing_url = item["outbound_redemption_page_link"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let lower = title.to_lowercase();
            let partner = if lower.contains("logitech") {
                PartnerId::Logitech
            } else if lower.contains("youtube") {
                PartnerId::YouTube
            } else if lower.contains("best buy") {
                PartnerId::BestBuy
            } else if lower.contains("call of duty") || lower.contains("blurple") {
                PartnerId::CallOfDuty
            } else if lower.contains("xbox") {
                PartnerId::Xbox
            } else {
                PartnerId::SteelSeries
            };
            Some(OutboundPromotion {
                id,
                title,
                body,
                promotion_type,
                partner,
                landing_url,
                next_drop_days: item["next_drop_days"].as_u64().map(|n| n as u32),
                unavailable: item["unavailable"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

/// Parses `GET /users/@me/outbound-promotions/codes`.
pub(crate) fn parse_claimed_codes(value: &Value) -> Vec<ClaimedPromoCode> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let promotion_id = item["promotion_id"]
                .as_str()
                .or_else(|| item["promotion"]["id"].as_str())?
                .to_owned();
            let code = item["code"].as_str()?.to_owned();
            let claimed_at = item["claimed_at"].as_str().unwrap_or_default().to_owned();
            Some(ClaimedPromoCode {
                promotion_id,
                code,
                claimed_at,
            })
        })
        .collect()
}

/// Sorts claimed codes newest-first and splits into `(primary, extra_count)` for `452417.et` (`+1 Code`).
pub(crate) fn sort_recurring_codes(mut codes: Vec<ClaimedPromoCode>) -> Vec<ClaimedPromoCode> {
    codes.sort_by(|a, b| b.claimed_at.cmp(&a.claimed_at));
    codes
}

#[derive(Clone, Debug)]
pub(super) struct ThirdPartyPromoModal {
    pub partner: PartnerId,
    pub locked: bool,
    pub expanded_recurring: bool,
    pub copied_code: Option<String>,
    pub claim_error: bool,
    pub claiming_id: Option<String>,
}

#[derive(Default)]
pub(super) struct State {
    pub modal: Option<ThirdPartyPromoModal>,
    pub promotions: Vec<OutboundPromotion>,
    pub claimed_codes: Vec<ClaimedPromoCode>,
    pub requested: bool,
}

impl Root {
    /// Opens `ThirdPartyPromotionsModal` (`452417`) for `partner`, fetching promotions & claimed codes.
    pub(super) fn open_third_party_promo_modal(
        &mut self,
        partner: PartnerId,
        cx: &mut Context<Self>,
    ) {
        let locked = self.premium() != PremiumType::Nitro;
        self.third_party_promos.modal = Some(ThirdPartyPromoModal {
            partner,
            locked,
            expanded_recurring: false,
            copied_code: None,
            claim_error: false,
            claiming_id: None,
        });
        let cfg = partner_config(partner);
        self.request_image(
            format!("third-party-promo:{:?}", partner),
            cfg.asset_url.to_owned(),
        );
        self.fetch_third_party_promos();
        cx.notify();
    }

    pub(super) fn fetch_third_party_promos(&mut self) {
        if self.third_party_promos.requested {
            return;
        }
        self.third_party_promos.requested = true;
        let locale = "en-US";
        let _ = self.requests.send(Request::Rest {
            key: Some(PROMOS_LIST_KEY.to_owned()),
            method: reqwest::Method::GET,
            route: format!("/storefront/promotions?locale={locale}&platform=0"),
            body: None,
        });
        let _ = self.requests.send(Request::Rest {
            key: Some(PROMOS_CODES_KEY.to_owned()),
            method: reqwest::Method::GET,
            route: format!("/users/@me/outbound-promotions/codes?locale={locale}"),
            body: None,
        });
    }

    pub(super) fn claim_third_party_promo(&mut self, promo_id: String, cx: &mut Context<Self>) {
        if let Some(m) = self.third_party_promos.modal.as_mut() {
            m.claiming_id = Some(promo_id.clone());
            m.claim_error = false;
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{PROMOS_CLAIM_PREFIX}{promo_id}")),
            method: reqwest::Method::POST,
            route: format!("/outbound-promotions/{promo_id}/claim"),
            body: Some(serde_json::json!({})),
        });
        cx.notify();
    }

    pub(super) fn third_party_promos_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        if key == PROMOS_LIST_KEY {
            self.third_party_promos.promotions = parse_outbound_promotions(body);
        } else if key == PROMOS_CODES_KEY {
            self.third_party_promos.claimed_codes = sort_recurring_codes(parse_claimed_codes(body));
        } else if let Some(promo_id) = key.strip_prefix(PROMOS_CLAIM_PREFIX) {
            if let Some(m) = self.third_party_promos.modal.as_mut() {
                m.claiming_id = None;
            }
            if let Some(code) = body["code"].as_str().filter(|s| !s.is_empty()) {
                let entry = ClaimedPromoCode {
                    promotion_id: promo_id.to_owned(),
                    code: code.to_owned(),
                    claimed_at: body["claimed_at"].as_str().unwrap_or_default().to_owned(),
                };
                self.third_party_promos
                    .claimed_codes
                    .retain(|c| c.promotion_id != promo_id || c.code != entry.code);
                self.third_party_promos.claimed_codes.insert(0, entry);
            } else if let Some(m) = self.third_party_promos.modal.as_mut() {
                m.claim_error = true;
            }
        }
        cx.notify();
    }

    /// Renders a `116011.wx` partner perk card (`.promoCardContainer_5cFAmt`).
    #[allow(clippy::too_many_arguments)]
    fn render_promo_card(
        &self,
        cfg: &PartnerConfig,
        promo: &OutboundPromotion,
        claimed: Option<&ClaimedPromoCode>,
        locked: bool,
        copied: bool,
        claiming: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = if promo.title.is_empty() {
            format_partner_title(cfg)
        } else {
            promo.title.clone()
        };
        let body = if !promo.body.is_empty() {
            promo.body.clone()
        } else if claimed.is_some()
            && let Some(key) = cfg.claimed_body_key
        {
            dc_intl::t(key).to_owned()
        } else {
            dc_intl::t(cfg.body_key).to_owned()
        };
        let landing = if promo.landing_url.is_empty() {
            cfg.landing_url.to_owned()
        } else {
            promo.landing_url.clone()
        };
        let pic = self
            .images
            .get(&format!("third-party-promo:{:?}", cfg.id))
            .cloned();
        let promo_id = promo.id.clone();

        let footer: Option<AnyElement> = if locked {
            None
        } else if let Some(code_entry) = claimed {
            let code_copy = code_entry.code.clone();
            let store_url = landing.clone();
            let redeem_label = dc_intl::t(cfg.redeem_label_key);
            Some(
                div()
                    .border_t_1()
                    .border_color(color("--border-muted"))
                    .pt(px(16.))
                    .child(
                        div()
                            .mx(px(16.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(code_entry.code.clone()),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "promo-copy-{}",
                                        code_entry.code
                                    )))
                                    .size(px(32.))
                                    .rounded(px(8.))
                                    .bg(if copied {
                                        color("--control-connected-background-default")
                                    } else {
                                        color("--control-secondary-background-default")
                                    })
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(
                                        if copied {
                                            "checkmark_large"
                                        } else {
                                            "clips_copy"
                                        },
                                        16.,
                                        "--white",
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            code_copy.clone(),
                                        ));
                                        if let Some(m) = this.third_party_promos.modal.as_mut() {
                                            m.copied_code = Some(code_copy.clone());
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("promo-store-{promo_id}")))
                                    .h(px(32.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--control-secondary-border-default"))
                                    .bg(color("--control-secondary-background-default"))
                                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .child(redeem_label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.follow_link(&store_url, "", cx);
                                    })),
                            ),
                    )
                    .into_any_element(),
            )
        } else {
            let unavailable = promo.unavailable || claiming;
            let btn_label = if promo.unavailable {
                dc_intl::t("8LKchl")
            } else {
                dc_intl::t(cfg.claim_label_key)
            };
            Some(
                div()
                    .px(px(16.))
                    .child(
                        div()
                            .id(SharedString::from(format!("promo-claim-{promo_id}")))
                            .h(px(36.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .when(unavailable, |b| b.opacity(0.5).cursor_not_allowed())
                            .when(!unavailable, |b| {
                                b.cursor_pointer()
                                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.claim_third_party_promo(promo_id.clone(), cx);
                                    }))
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(btn_label),
                    )
                    .into_any_element(),
            )
        };

        let countdown = (!locked)
            .then_some(promo.next_drop_days)
            .flatten()
            .filter(|d| *d > 0)
            .map(|days| {
                // `116011.vw` (`.progressBarSlim_5cFAmt`)
                let fill = (1.0 - (days as f32 / 30.0)).clamp(0.1, 1.0);
                div()
                    .px(px(16.))
                    .my(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::format(
                                "65EEvD",
                                &[("days", dc_intl::Arg::Num(days as f64))],
                            )),
                    )
                    .child(
                        div()
                            .h(px(6.))
                            .w_full()
                            .rounded_full()
                            .bg(color("--background-mod-subtle"))
                            .overflow_hidden()
                            .child(div().h_full().w(gpui::relative(fill)).bg(
                                gpui::linear_gradient(
                                    90.,
                                    gpui::linear_color_stop(gpui::rgb(0x5865f2), 0.),
                                    gpui::linear_color_stop(gpui::rgb(0x9ea8ff), 1.),
                                ),
                            )),
                    )
            });

        div()
            .w_full()
            .py(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-muted"))
            .bg(color("--background-surface-highest"))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .when(locked, |col| {
                                col.child(
                                    div()
                                        .size(px(32.))
                                        .rounded(px(8.))
                                        .bg(color("--background-mod-subtle"))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("access_lock", 16., "--text-muted")),
                                )
                            })
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
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(body),
                            ),
                    )
                    .child(
                        div()
                            .w(px(116.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(100.))
                                    .rounded(px(if locked { 10. } else { 16. }))
                                    .overflow_hidden()
                                    .bg(color("--background-mod-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(match pic {
                                        Some(img_pic) => gpui::img(img_pic)
                                            .size(px(100.))
                                            .object_fit(gpui::ObjectFit::Cover)
                                            .into_any_element(),
                                        None => icon("nitro_wheel", 36., "--text-muted")
                                            .into_any_element(),
                                    }),
                            ),
                    ),
            )
            .children(countdown)
            .children(footer)
            .into_any_element()
    }

    /// Renders `ThirdPartyPromotionsModal` (`452417`: locked `ei`, unlocked `er`, recurring `et`).
    pub(super) fn third_party_promo_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.third_party_promos.modal.as_ref()?;
        let cfg = partner_config(modal.partner);
        let title = format_partner_title(&cfg);

        let partner_promos: Vec<OutboundPromotion> = self
            .third_party_promos
            .promotions
            .iter()
            .filter(|p| p.partner == modal.partner)
            .cloned()
            .collect();
        let fallback_promo = OutboundPromotion {
            id: format!("preview-{:?}", modal.partner),
            title: title.clone(),
            body: dc_intl::t(cfg.body_key).to_owned(),
            promotion_type: PROMO_TYPE_THIRD_PARTY_OUTBOUND_RECURRING,
            partner: modal.partner,
            landing_url: cfg.landing_url.to_owned(),
            next_drop_days: (!modal.locked).then_some(14),
            unavailable: false,
        };
        let active_promos = if partner_promos.is_empty() {
            vec![fallback_promo]
        } else {
            partner_promos
        };

        let recurring: Vec<&OutboundPromotion> = active_promos
            .iter()
            .filter(|p| p.promotion_type == PROMO_TYPE_THIRD_PARTY_OUTBOUND_RECURRING)
            .collect();
        let one_time: Vec<&OutboundPromotion> = active_promos
            .iter()
            .filter(|p| p.promotion_type == PROMO_TYPE_THIRD_PARTY_OUTBOUND)
            .collect();

        let mut content = div().flex().flex_col().gap(px(14.));
        if modal.claim_error {
            content = content.child(
                div()
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-feedback-mod-critical"))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(dc_intl::t("i2EuFO")),
            );
        }
        if !recurring.is_empty() {
            if cfg.show_section_headers {
                content = content.child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("9Y2p6p")),
                );
            }
            for promo in recurring {
                let matching_codes: Vec<&ClaimedPromoCode> = self
                    .third_party_promos
                    .claimed_codes
                    .iter()
                    .filter(|c| c.promotion_id == promo.id)
                    .collect();
                let primary = matching_codes.first().copied();
                let copied =
                    primary.is_some_and(|c| modal.copied_code.as_deref() == Some(c.code.as_str()));
                let claiming = modal.claiming_id.as_deref() == Some(promo.id.as_str());
                let card = self.render_promo_card(
                    &cfg,
                    promo,
                    primary,
                    modal.locked,
                    copied,
                    claiming,
                    cx,
                );
                let extra_count = matching_codes.len().saturating_sub(1);
                content = content.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(card)
                        .when(
                            !modal.locked && extra_count > 0 && !modal.expanded_recurring,
                            |col| {
                                // `452417.et` (`.extraCodeButton_5cFAmt`, `rjjZxV` "+1 Code")
                                col.child(
                                    div()
                                        .id("promo-extra-codes-toggle")
                                        .w(gpui::relative(0.8))
                                        .mx_auto()
                                        .py(px(4.))
                                        .rounded_b(px(12.))
                                        .border_x_1()
                                        .border_b_1()
                                        .border_color(color("--border-muted"))
                                        .bg(color("--background-surface-highest"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t("rjjZxV"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(m) = this.third_party_promos.modal.as_mut()
                                            {
                                                m.expanded_recurring = true;
                                            }
                                            cx.notify();
                                        })),
                                )
                            },
                        )
                        .when(
                            !modal.locked && modal.expanded_recurring && extra_count > 0,
                            |col| {
                                let mut list = div().mt(px(8.)).flex().flex_col().gap(px(8.));
                                for extra in matching_codes.iter().skip(1) {
                                    let copied_extra =
                                        modal.copied_code.as_deref() == Some(extra.code.as_str());
                                    list = list.child(self.render_promo_card(
                                        &cfg,
                                        promo,
                                        Some(extra),
                                        false,
                                        copied_extra,
                                        false,
                                        cx,
                                    ));
                                }
                                col.child(list)
                            },
                        ),
                );
            }
        }
        if !one_time.is_empty() {
            if cfg.show_section_headers {
                content = content.child(
                    div()
                        .mt(px(10.))
                        .text_size(px(12.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("Wm58LR")),
                );
            }
            for promo in one_time {
                let claimed = self
                    .third_party_promos
                    .claimed_codes
                    .iter()
                    .find(|c| c.promotion_id == promo.id);
                let copied =
                    claimed.is_some_and(|c| modal.copied_code.as_deref() == Some(c.code.as_str()));
                let claiming = modal.claiming_id.as_deref() == Some(promo.id.as_str());
                content = content.child(self.render_promo_card(
                    &cfg,
                    promo,
                    claimed,
                    modal.locked,
                    copied,
                    claiming,
                    cx,
                ));
            }
        }

        let dialog = div()
            .id("third-party-promo-modal")
            .occlude()
            .w(px(520.))
            .max_h(px(640.))
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
                    .flex_none()
                    .px(px(20.))
                    .py(px(16.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("nitro_wheel", 20., "--text-strong"))
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .line_height(px(22.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .id("third-party-promo-close")
                            .size(px(32.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 18., "--text-muted"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.third_party_promos.modal = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("third-party-promo-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(20.))
                    .child(content),
            );

        Some(
            deferred(
                div()
                    .id("third-party-promo-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.third_party_promos.modal = None;
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Capture harness: `third-party-promo [locked|unlocked|claimed|error|close]` (`452417` / `116011`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_third_party_promo(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.third_party_promos.modal = None;
                cx.notify();
                "third-party-promo: closed".to_owned()
            }
            mode => {
                let locked = mode == "locked";
                let claimed = mode == "claimed";
                self.third_party_promos.promotions = vec![OutboundPromotion {
                    id: "promo_ss_1".to_owned(),
                    title: format_partner_title(&partner_config(PartnerId::SteelSeries)),
                    body: dc_intl::t("nmvvaN").to_owned(),
                    promotion_type: PROMO_TYPE_THIRD_PARTY_OUTBOUND_RECURRING,
                    partner: PartnerId::SteelSeries,
                    landing_url: "https://steelseries.com/".to_owned(),
                    next_drop_days: Some(12),
                    unavailable: false,
                }];
                self.third_party_promos.claimed_codes = if claimed {
                    vec![
                        ClaimedPromoCode {
                            promotion_id: "promo_ss_1".to_owned(),
                            code: "SS-NITRO-2026-ABCD".to_owned(),
                            claimed_at: "2026-11-20T00:00:00Z".to_owned(),
                        },
                        ClaimedPromoCode {
                            promotion_id: "promo_ss_1".to_owned(),
                            code: "SS-NITRO-2026-PREV".to_owned(),
                            claimed_at: "2026-10-20T00:00:00Z".to_owned(),
                        },
                    ]
                } else {
                    Vec::new()
                };
                self.third_party_promos.modal = Some(ThirdPartyPromoModal {
                    partner: PartnerId::SteelSeries,
                    locked,
                    expanded_recurring: false,
                    copied_code: None,
                    claim_error: mode == "error",
                    claiming_id: None,
                });
                cx.notify();
                format!("third-party-promo: {mode}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn partner_configs_and_outbound_promo_parsing_cover_module_452417() {
        for partner in [
            PartnerId::SteelSeries,
            PartnerId::Logitech,
            PartnerId::YouTube,
            PartnerId::BestBuy,
            PartnerId::CallOfDuty,
            PartnerId::Xbox,
        ] {
            let cfg = partner_config(partner);
            assert!(!format_partner_title(&cfg).is_empty());
            assert!(!dc_intl::t(cfg.body_key).is_empty());
        }
        let raw = json!([
            {
                "id": "p1",
                "promotion_type": 4,
                "outbound_title": "20% Off SteelSeries",
                "outbound_redemption_page_link": "https://steelseries.com/"
            },
            {
                "id": "p2",
                "promotion_type": 7,
                "outbound_title": "Logitech G Monthly Reward",
                "next_drop_days": 14
            },
            {
                "id": "p_skip",
                "promotion_type": 1,
                "outbound_title": "Ignored"
            }
        ]);
        let promos = parse_outbound_promotions(&raw);
        assert_eq!(promos.len(), 2);
        assert_eq!(promos[0].partner, PartnerId::SteelSeries);
        assert_eq!(promos[1].partner, PartnerId::Logitech);
        assert_eq!(promos[1].next_drop_days, Some(14));

        let codes = sort_recurring_codes(parse_claimed_codes(&json!([
            {"promotion_id": "p2", "code": "OLD", "claimed_at": "2026-09-01T00:00:00Z"},
            {"promotion_id": "p2", "code": "NEW", "claimed_at": "2026-10-01T00:00:00Z"}
        ])));
        assert_eq!(codes[0].code, "NEW");
        assert_eq!(codes[1].code, "OLD");
        assert!(!dc_intl::t("rjjZxV").is_empty());
        assert!(!dc_intl::t("i2EuFO").is_empty());
    }
}
