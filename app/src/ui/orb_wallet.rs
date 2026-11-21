//! Orb Wallet Popout & Active Challenges Panel (module 150849, secondary bundle 312653).
//!
//! Opened from the Orbs balance pill on Quest Home (`quests.rs`) and Shop (`shop.rs`).
//! Fetches:
//! - `GET /users/@me/virtual-currency/total-redeemed`
//! - `GET /users/@me/achievements/list?reward_type=orbs`
//! - `POST /users/@me/achievements/{id}/claim`
//! - `POST /users/@me/achievements/unread-ack`
//!
//! Renders the 340px wallet card (`XKj5L9`), balance header (`cKwv4k`/`zPaLL9`), next Nitro Orbs
//! tag (`04j3XV`), primary CTA card (Shop Orbs `v8QrJf`/`HACucK`/`7raRgL` or Nitro Upsell
//! `ZqCAos`/`Ba/7wO`/`U9UQJE`/`FkfrRH`), Active Challenges (`H6Ny8N`, multiplier pills
//! `OHLdjq`/`WOMrJf`, rows with `B8Fxns`/`h2qWpS`/`WmfVjs`/`k6h2J3`, cleared state `xOP5OP`/
//! `XW2CuY`, error retry `ggLzeg`, claim error `FYb5rH`), unclaimed account notice
//! (`qxk9zo`/`gP/vPe`), and footer (`JALI2K`, `pWG4ze`, `B5tZXw`, `5qZv9E`).

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use dc_state::premium::PremiumType;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, deferred, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};
use serde_json::{Value, json};

pub(crate) const TOTAL_REDEEMED_KEY: &str = "orbs:total-redeemed";
pub(crate) const ACHIEVEMENTS_LIST_KEY: &str = "orbs:achievements-list";
pub(crate) const ACHIEVEMENTS_CLAIM_PREFIX: &str = "orbs:achievement-claim:";
pub(crate) const ACHIEVEMENTS_UNREAD_ACK_KEY: &str = "orbs:achievements-unread-ack";

const ORBS_HELP_URL: &str = "https://support.discord.com/hc/en-us/articles/30593690165783";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChallengeStatus {
    InProgress,
    Claimable,
    Claimed,
}

#[derive(Clone, Debug)]
pub(crate) struct OrbChallenge {
    pub id: String,
    pub title: String,
    pub orb_amount: u64,
    pub status: ChallengeStatus,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct OrbWalletState {
    pub open: bool,
    pub loading: bool,
    pub load_error: bool,
    pub claim_error: bool,
    pub balance: Option<u64>,
    pub total_redeemed: u64,
    pub next_nitro_orbs_days: Option<(u64, u32)>,
    pub multiplier: u32,
    pub primary_card_dismissed: bool,
    pub unclaimed_account: bool,
    pub challenges: Vec<OrbChallenge>,
}

/// `GET /users/@me/virtual-currency/total-redeemed` (module 150849 / 786953).
pub(crate) fn total_redeemed_request() -> Request {
    Request::Rest {
        key: Some(TOTAL_REDEEMED_KEY.to_owned()),
        method: reqwest::Method::GET,
        route: "/users/@me/virtual-currency/total-redeemed".to_owned(),
        body: None,
    }
}

/// `GET /users/@me/achievements/list?reward_type=orbs` (module 150849 / 12510).
pub(crate) fn achievements_list_request() -> Request {
    Request::Rest {
        key: Some(ACHIEVEMENTS_LIST_KEY.to_owned()),
        method: reqwest::Method::GET,
        route: "/users/@me/achievements/list?reward_type=orbs".to_owned(),
        body: None,
    }
}

/// `POST /users/@me/achievements/{id}/claim` (module 150849 / 12510).
pub(crate) fn claim_achievement_request(id: &str) -> Request {
    Request::Rest {
        key: Some(format!("{ACHIEVEMENTS_CLAIM_PREFIX}{id}")),
        method: reqwest::Method::POST,
        route: format!("/users/@me/achievements/{id}/claim"),
        body: Some(json!({})),
    }
}

/// `POST /users/@me/achievements/unread-ack` (module 150849 / 12510).
pub(crate) fn unread_ack_request() -> Request {
    Request::Rest {
        key: Some(ACHIEVEMENTS_UNREAD_ACK_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: "/users/@me/achievements/unread-ack".to_owned(),
        body: Some(json!({})),
    }
}

impl Root {
    /// Toggles the Orb Wallet Popout (`150849`) from Quest Home or Shop balance pill.
    pub(super) fn toggle_orb_wallet(&mut self, cx: &mut Context<Self>) {
        self.orb_wallet.open = !self.orb_wallet.open;
        if self.orb_wallet.open {
            let live_balance = (self.quests.currency.shown() > 0)
                .then(|| self.quests.currency.shown())
                .or_else(|| self.shop.balance.map(|b| b as u64));
            if self.orb_wallet.balance.is_none() {
                self.orb_wallet.balance = live_balance;
            }
            if self.orb_wallet.multiplier == 0 {
                self.orb_wallet.multiplier = 2;
            }
            self.orb_wallet.unclaimed_account = self.stores.account.email.is_none();
            self.orb_wallet.loading = self.orb_wallet.challenges.is_empty();
            self.orb_wallet.load_error = false;
            self.orb_wallet.claim_error = false;
            let _ = self.requests.send(total_redeemed_request());
            let _ = self.requests.send(achievements_list_request());
            let _ = self.requests.send(unread_ack_request());
        }
        cx.notify();
    }

    /// Handles Orb Wallet REST answers (`total-redeemed`, `achievements/list`, `claim`).
    pub(super) fn handle_orb_wallet_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let failed = body["error"].is_string() || body["status"].as_u64().is_some_and(|s| s >= 400);
        if key == TOTAL_REDEEMED_KEY {
            if let Some(total) = body["body"]["total_redeemed"].as_u64() {
                self.orb_wallet.total_redeemed = total;
            }
            cx.notify();
        } else if key == ACHIEVEMENTS_LIST_KEY {
            self.orb_wallet.loading = false;
            if failed {
                self.orb_wallet.load_error = true;
            } else {
                self.orb_wallet.load_error = false;
                if let Some(arr) = body["body"]["achievements"].as_array() {
                    self.orb_wallet.challenges = arr
                        .iter()
                        .filter_map(|a| {
                            let id = a["id"].as_str()?.to_owned();
                            let title = a["title"].as_str().unwrap_or("Challenge").to_owned();
                            let orb_amount = a["reward_amount"].as_u64().unwrap_or(100);
                            let status = if a["claimed"].as_bool().unwrap_or(false) {
                                ChallengeStatus::Claimed
                            } else if a["completed"].as_bool().unwrap_or(false) {
                                ChallengeStatus::Claimable
                            } else {
                                ChallengeStatus::InProgress
                            };
                            Some(OrbChallenge {
                                id,
                                title,
                                orb_amount,
                                status,
                            })
                        })
                        .collect();
                }
            }
            cx.notify();
        } else if let Some(id) = key.strip_prefix(ACHIEVEMENTS_CLAIM_PREFIX) {
            if failed {
                self.orb_wallet.claim_error = true;
            } else {
                self.orb_wallet.claim_error = false;
                if let Some(ch) = self.orb_wallet.challenges.iter_mut().find(|c| c.id == id) {
                    ch.status = ChallengeStatus::Claimed;
                    if let Some(bal) = self.orb_wallet.balance.as_mut() {
                        *bal += ch.orb_amount;
                    }
                }
            }
            cx.notify();
        }
    }

    /// Modal/popout layer for the 340px Orb Wallet & Active Challenges panel (`150849`).
    pub(super) fn orb_wallet_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.orb_wallet.open {
            return None;
        }
        let wallet = &self.orb_wallet;
        let is_nitro = self.premium() == PremiumType::Nitro;
        let balance_val = wallet.balance.unwrap_or(0);
        let multiplier = wallet.multiplier.max(2);

        let balance_aria = if wallet.loading && wallet.balance.is_none() {
            dc_intl::t("cKwv4k").to_owned()
        } else {
            dc_intl::format("zPaLL9", &[("balance", Arg::Num(balance_val as f64))])
        };

        // Primary CTA card (`eP`): Nitro + balance >= 1400 -> Shop Orbs; non-Nitro -> Nitro Upsell.
        let primary_card = (!wallet.primary_card_dismissed).then(|| {
            if is_nitro && balance_val >= 1400 {
                div()
                    .mx(px(16.))
                    .p(px(14.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(rgba(0x5865f233), 0.),
                        linear_color_stop(rgba(0x2b2d31ff), 1.),
                    ))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("v8QrJf")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("HACucK")),
                                    ),
                            )
                            .child(
                                div()
                                    .id("orb-wallet-dismiss-primary")
                                    .flex_none()
                                    .cursor_pointer()
                                    .size(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 14., "--icon-subtle"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.orb_wallet.primary_card_dismissed = true;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("orb-wallet-redeem-cta")
                            .cursor_pointer()
                            .w_full()
                            .h(px(32.))
                            .rounded(px(6.))
                            .bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("7raRgL"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.orb_wallet.open = false;
                                this.open_shop(cx);
                            })),
                    )
            } else {
                let upsell_copy =
                    dc_intl::format("Ba/7wO", &[("multiplier", Arg::Num(f64::from(multiplier)))]);
                div()
                    .mx(px(16.))
                    .p(px(14.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(rgba(0xeb459e66))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(rgba(0xeb459e33), 0.),
                        linear_color_stop(rgba(0x5865f233), 1.),
                    ))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(icon("nitro_wheel", 14., "--white"))
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("FkfrRH").to_uppercase()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("ZqCAos")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-default"))
                                            .child(upsell_copy),
                                    ),
                            )
                            .child(
                                div()
                                    .id("orb-wallet-dismiss-upsell")
                                    .flex_none()
                                    .cursor_pointer()
                                    .size(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 14., "--icon-subtle"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.orb_wallet.primary_card_dismissed = true;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("orb-wallet-get-nitro")
                            .cursor_pointer()
                            .w_full()
                            .h(px(32.))
                            .rounded(px(6.))
                            .bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("U9UQJE"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.orb_wallet.open = false;
                                this.open_nitro(cx);
                            })),
                    )
            }
        });

        let mult_pill_label = if is_nitro {
            dc_intl::format("OHLdjq", &[("multiplier", Arg::Num(f64::from(multiplier)))])
        } else {
            dc_intl::format("WOMrJf", &[("multiplier", Arg::Num(f64::from(multiplier)))])
        };

        let challenges_body: AnyElement = if wallet.load_error {
            div()
                .id("orb-challenges-retry")
                .cursor_pointer()
                .px(px(12.))
                .py(px(16.))
                .text_center()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(color("--text-muted"))
                .child(dc_intl::format("ggLzeg", &[]))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.orb_wallet.load_error = false;
                    this.orb_wallet.loading = true;
                    let _ = this.requests.send(achievements_list_request());
                    cx.notify();
                }))
                .into_any_element()
        } else if wallet.loading {
            div()
                .px(px(8.))
                .py(px(6.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .children((0..3).map(|_| {
                    div()
                        .h(px(44.))
                        .rounded(px(8.))
                        .bg(color("--background-base-lowest"))
                }))
                .into_any_element()
        } else if wallet.challenges.is_empty()
            || wallet
                .challenges
                .iter()
                .all(|c| c.status == ChallengeStatus::Claimed)
        {
            div()
                .px(px(12.))
                .py(px(18.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .child(icon("circle_check", 24., "--status-positive"))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("xOP5OP")),
                )
                .child(
                    div()
                        .text_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("XW2CuY")),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .children(wallet.challenges.iter().enumerate().map(|(idx, ch)| {
                    let ch_id = ch.id.clone();
                    let status = ch.status;
                    let sub_text = match status {
                        ChallengeStatus::Claimable => dc_intl::format_runs(
                            "h2qWpS",
                            &[("orbAmount", Arg::Num(ch.orb_amount as f64))],
                        )
                        .into_iter()
                        .filter(|(_, hook)| hook.as_deref() != Some("orbIconHook"))
                        .map(|(s, _)| s)
                        .collect::<String>(),
                        ChallengeStatus::Claimed => dc_intl::t("k6h2J3").to_owned(),
                        ChallengeStatus::InProgress => dc_intl::format_runs(
                            "B8Fxns",
                            &[("orbAmount", Arg::Num(ch.orb_amount as f64))],
                        )
                        .into_iter()
                        .filter(|(_, hook)| hook.as_deref() != Some("orbIconHook"))
                        .map(|(s, _)| s)
                        .collect::<String>(),
                    };
                    div()
                        .id(format!("orb-challenge-row-{idx}"))
                        .px(px(8.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(color("--background-base-lower"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .flex_none()
                                        .size(px(36.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(if status == ChallengeStatus::InProgress {
                                            color("--border-subtle")
                                        } else {
                                            color("--status-positive")
                                        })
                                        .bg(color("--background-base-lowest"))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("shop_orb_18", 18., "--text-strong")),
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
                                                .text_size(px(13.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .truncate()
                                                .child(ch.title.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(color("--text-muted"))
                                                .truncate()
                                                .child(sub_text),
                                        ),
                                ),
                        )
                        .child(match status {
                            ChallengeStatus::Claimable => div()
                                .id(format!("orb-challenge-claim-{idx}"))
                                .flex_none()
                                .cursor_pointer()
                                .px(px(12.))
                                .h(px(28.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(12.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("WmfVjs"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let _ = this.requests.send(claim_achievement_request(&ch_id));
                                    if let Some(c) = this
                                        .orb_wallet
                                        .challenges
                                        .iter_mut()
                                        .find(|c| c.id == ch_id)
                                    {
                                        c.status = ChallengeStatus::Claimed;
                                        if let Some(b) = this.orb_wallet.balance.as_mut() {
                                            *b += c.orb_amount;
                                        }
                                    }
                                    cx.notify();
                                }))
                                .into_any_element(),
                            ChallengeStatus::Claimed => {
                                icon("circle_check", 16., "--status-positive").into_any_element()
                            }
                            ChallengeStatus::InProgress => {
                                icon("shop_arrow_right", 14., "--icon-subtle").into_any_element()
                            }
                        })
                }))
                .into_any_element()
        };

        let redeemed_label = dc_intl::format(
            "B5tZXw",
            &[("orbAmount", Arg::Num(wallet.total_redeemed as f64))],
        );

        Some(
            deferred(
                div()
                    .id("orb-wallet-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x00000080))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.orb_wallet.open = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("orb-wallet-popout")
                            .aria_label(balance_aria)
                            .w(px(340.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .py(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            // Header: Orb icon + balance + title + close
                            .child(
                                div()
                                    .px(px(16.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .gap(px(12.))
                                            .child(
                                                div()
                                                    .flex_none()
                                                    .size(px(44.))
                                                    .rounded_full()
                                                    .bg(rgba(0x5865f22a))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon(
                                                        "shop_orb_18",
                                                        26.,
                                                        "--text-strong",
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .child(
                                                        div()
                                                            .text_size(px(11.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--text-muted"))
                                                            .child(
                                                                dc_intl::t("XKj5L9").to_uppercase(),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(24.))
                                                            .line_height(px(28.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--text-strong"))
                                                            .child(balance_val.to_string()),
                                                    )
                                                    .children(wallet.next_nitro_orbs_days.map(
                                                        |(orbs, days)| {
                                                            div()
                                                                .text_size(px(11.))
                                                                .text_color(color("--text-muted"))
                                                                .child(dc_intl::format(
                                                                    "04j3XV",
                                                                    &[
                                                                        (
                                                                            "orbAmount",
                                                                            Arg::Num(orbs as f64),
                                                                        ),
                                                                        (
                                                                            "days",
                                                                            Arg::Num(f64::from(
                                                                                days,
                                                                            )),
                                                                        ),
                                                                    ],
                                                                ))
                                                        },
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("orb-wallet-close")
                                            .aria_label(dc_intl::t("cpT0Cq"))
                                            .flex_none()
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 16., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.orb_wallet.open = false;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .when(wallet.unclaimed_account, |col| {
                                col.child(
                                    div()
                                        .mx(px(16.))
                                        .p(px(10.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--status-warning"))
                                        .bg(rgba(0xf0b2321a))
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(dc_intl::t("qxk9zo")),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("gP/vPe")),
                                        ),
                                )
                            })
                            .children(primary_card)
                            // Active Challenges section
                            .child(
                                div()
                                    .border_t_1()
                                    .border_b_1()
                                    .border_color(color("--border-subtle"))
                                    .px(px(12.))
                                    .py(px(12.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .px(px(4.))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("H6Ny8N").to_uppercase()),
                                            )
                                            .child(
                                                div()
                                                    .px(px(8.))
                                                    .py(px(2.))
                                                    .rounded_full()
                                                    .bg(linear_gradient(
                                                        90.,
                                                        linear_color_stop(rgba(0xeb459eff), 0.),
                                                        linear_color_stop(rgba(0x5865f2ff), 1.),
                                                    ))
                                                    .text_size(px(11.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--white"))
                                                    .child(mult_pill_label),
                                            ),
                                    )
                                    .when(wallet.claim_error, |col| {
                                        col.child(
                                            div()
                                                .rounded(px(6.))
                                                .border_1()
                                                .border_color(color("--status-danger"))
                                                .bg(rgba(0xda373c1a))
                                                .px(px(10.))
                                                .py(px(6.))
                                                .text_size(px(12.))
                                                .text_color(color("--text-feedback-critical"))
                                                .child(dc_intl::t("FYb5rH")),
                                        )
                                    })
                                    .child(challenges_body),
                            )
                            // Footer: Quests & Shop buttons + redeemed count + Need help?
                            .child(
                                div()
                                    .px(px(16.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .id("orb-wallet-open-quests")
                                                    .flex_1()
                                                    .cursor_pointer()
                                                    .h(px(34.))
                                                    .rounded(px(6.))
                                                    .bg(color(
                                                        "--control-secondary-background-default",
                                                    ))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color(
                                                        "--control-secondary-text-default",
                                                    ))
                                                    .child(dc_intl::t("JALI2K"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.orb_wallet.open = false;
                                                        this.open_quests(cx);
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .id("orb-wallet-open-shop")
                                                    .flex_1()
                                                    .cursor_pointer()
                                                    .h(px(34.))
                                                    .rounded(px(6.))
                                                    .bg(color(
                                                        "--control-secondary-background-default",
                                                    ))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color(
                                                        "--control-secondary-text-default",
                                                    ))
                                                    .child(dc_intl::t("pWG4ze"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.orb_wallet.open = false;
                                                        this.open_shop(cx);
                                                    })),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .gap(px(6.))
                                            .text_size(px(11.))
                                            .text_color(color("--text-muted"))
                                            .child(redeemed_label)
                                            .child("•")
                                            .child(
                                                div()
                                                    .id("orb-wallet-help")
                                                    .cursor_pointer()
                                                    .text_color(color("--text-link"))
                                                    .child(dc_intl::t("5qZv9E"))
                                                    .on_click(|_, _, cx| {
                                                        cx.open_url(ORBS_HELP_URL);
                                                    }),
                                            ),
                                    ),
                            ),
                    ),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `orb-wallet [active|cleared|error|unclaimed|close]` (module 150849).
    #[cfg(feature = "capture")]
    pub(crate) fn script_orb_wallet(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.orb_wallet.open = false;
                cx.notify();
                "orb-wallet: closed".to_owned()
            }
            "cleared" => {
                self.orb_wallet = OrbWalletState {
                    open: true,
                    loading: false,
                    load_error: false,
                    claim_error: false,
                    balance: Some(1850),
                    total_redeemed: 4200,
                    next_nitro_orbs_days: Some((300, 5)),
                    multiplier: 2,
                    primary_card_dismissed: false,
                    unclaimed_account: false,
                    challenges: Vec::new(),
                };
                cx.notify();
                "orb-wallet: cleared".to_owned()
            }
            "error" => {
                self.orb_wallet = OrbWalletState {
                    open: true,
                    loading: false,
                    load_error: true,
                    claim_error: false,
                    balance: Some(600),
                    total_redeemed: 1200,
                    next_nitro_orbs_days: None,
                    multiplier: 2,
                    primary_card_dismissed: false,
                    unclaimed_account: false,
                    challenges: Vec::new(),
                };
                cx.notify();
                "orb-wallet: error".to_owned()
            }
            "unclaimed" => {
                self.orb_wallet = OrbWalletState {
                    open: true,
                    loading: false,
                    load_error: false,
                    claim_error: false,
                    balance: Some(250),
                    total_redeemed: 0,
                    next_nitro_orbs_days: None,
                    multiplier: 2,
                    primary_card_dismissed: true,
                    unclaimed_account: true,
                    challenges: vec![OrbChallenge {
                        id: "ach_1".to_owned(),
                        title: "Complete your first Quest".to_owned(),
                        orb_amount: 300,
                        status: ChallengeStatus::InProgress,
                    }],
                };
                cx.notify();
                "orb-wallet: unclaimed".to_owned()
            }
            _ => {
                self.orb_wallet = OrbWalletState {
                    open: true,
                    loading: false,
                    load_error: false,
                    claim_error: false,
                    balance: Some(1650),
                    total_redeemed: 2400,
                    next_nitro_orbs_days: Some((300, 4)),
                    multiplier: 2,
                    primary_card_dismissed: false,
                    unclaimed_account: false,
                    challenges: vec![
                        OrbChallenge {
                            id: "ach_1".to_owned(),
                            title: "Complete a Video Quest".to_owned(),
                            orb_amount: 500,
                            status: ChallengeStatus::Claimable,
                        },
                        OrbChallenge {
                            id: "ach_2".to_owned(),
                            title: "Play 15 min of a Quest game".to_owned(),
                            orb_amount: 700,
                            status: ChallengeStatus::InProgress,
                        },
                    ],
                };
                cx.notify();
                "orb-wallet: active".to_owned()
            }
        }
    }
}
