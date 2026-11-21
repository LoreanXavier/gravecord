//! The application storefront modal (module 719847), opened by the Store button of a bot's popout and profile modal
//! (module 571077, for an application with `storefront_available`).
//!
//! - The modal: the design system dialog at size `xxl` (`size-xxl_dvN-MP`: at most 1280 wide, 80% of the window once
//!   the dialog area is 1080 or wider), the store icon of module 575926 before "{appName} Store", Copy Link (the
//!   app's `/discovery/applications/{id}/store` page, "Link Copied!") and the close button; the body; the footer with
//!   the developer's Terms of Service and Privacy Policy links (module 816709) or "This developer has not set up...".
//! - The body (`Mp` of module 55709): the header (`wrapper_qZsksx`, the app's icon and name over the blurred banner,
//!   the banner to the right), then the subscriptions under "Personal Subscriptions" or "Server Subscriptions"
//!   (module 311321) and the one-time items under "Items", in grids of 287px cards.
//! - A subscription card (module 885996 `O`): summary, thumbnail (or the badge icon), "Subscribe for {rate}" (module
//!   376374 `rc`), description and the tier's benefits ("Exclusive to this tier", five at most, then "+ N more
//!   benefits"). An item card (module 55709 `h`): header image (or the ticket icon), name, description, and the footer
//!   with "1 / 1 Available" for a durable item and "Buy for {price}" (`$P`), "Acquired" once owned.
//! - The data (`C` of module 123791): `GET /applications/{id}/store-layout` (subscriptions and one-time listings,
//!   ApplicationStoreDirectoryStore of module 791878), `GET /store/published-listings/skus?application_id={id}` (the
//!   SKUs with their prices), the application's entitlements, and each subscription's plans
//!   (`/store/published-listings/skus/{sku}/subscription-plans`). The application itself is
//!   `GET /applications/{id}/public` (`fetchApplication`).
//!
//! GAP: the purchase and subscribe buttons start the checkout (`useStorefrontCheckout`, module 638643) or, outside the
//! global discovery experiment, open the app directory's SKU page; neither is built here (no purchases), so they do
//! nothing. The cards' details modals (168393, 963179), the subscribed states of module 185438 (the account's
//! application subscriptions are not read: every plan reads as available) and the animated banner on hover are not
//! built.

use super::modal::{self, Tone};
use super::{Root, color, icon};
use crate::net::Request;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, SharedString, Window, deferred, div,
    img, prelude::*, px,
};
use serde_json::Value;

/// The key prefix of this modal's reads: `storefront:{what}:{app}`.
pub(super) const KEY_PREFIX: &str = "storefront:";

/// `APPLICATION_USER_SUBSCRIPTION` and `APPLICATION_GUILD_SUBSCRIPTION` of `SKUFlags` (module 652215 `d68`).
#[cfg(test)]
const USER_SUBSCRIPTION: u64 = 1 << 8;
const GUILD_SUBSCRIPTION: u64 = 1 << 7;
/// `SKUTypes.DURABLE` (`Puh`).
const DURABLE: u64 = 2;
/// `SKUTypes.SUBSCRIPTION`.
const SUBSCRIPTION: u64 = 5;
/// `maxBenefits` of a subscription card without a subtitle.
const MAX_BENEFITS: usize = 5;
/// The card width of `productSection_OnNQXM`'s grid columns.
const CARD: f32 = 287.;

/// Active card detail overlay inside the App Storefront (`168393` SubscriptionDetailsModal /
/// `963179` ItemDetailsModal, opened by clicking a card in module 55709).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum StorefrontDetail {
    Subscription(Listing),
    Item(Sku),
}

/// Subscription card availability & ownership state (`185438` + `376374.rc` + `55709`).
#[derive(Clone, Debug, PartialEq, Eq)]
enum SubCardState {
    Available,
    UpcomingPlan {
        starts_at: String,
    },
    Subscribed {
        until: Option<String>,
        is_guild: bool,
    },
    Unavailable,
}

/// The open modal: the application and the server it was opened in.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Open {
    pub app: String,
    pub guild: Option<u64>,
    pub detail: Option<StorefrontDetail>,
}

/// A benefit of a subscription listing (`{id, name, description, icon}`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Benefit {
    pub name: String,
    /// `icon.store_asset_id` of a `STORE_ASSET` icon.
    pub asset: Option<String>,
    /// `icon.emoji` of an `EMOJI` icon.
    pub emoji: Option<String>,
}

/// A store listing as `ApplicationStoreDirectoryStore` keeps it (module 790436 `createFromServer`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Listing {
    pub id: String,
    pub sku: String,
    pub sku_flags: u64,
    pub summary: String,
    pub description: Option<String>,
    /// `thumbnail.id`, a store asset.
    pub thumbnail: Option<String>,
    pub benefits: Vec<Benefit>,
}

/// A SKU of the published listings: its type, name and price.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Sku {
    pub id: String,
    pub kind: u64,
    pub name: String,
    /// `(amount, currency)` in the currency's minor unit.
    pub price: Option<(i64, String)>,
    /// The published listing's `description` and `header_background` (StoreListingStore `getForSKU`).
    pub description: Option<String>,
    pub header: Option<String>,
}

/// A subscription plan of a SKU: price and interval.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Plan {
    pub price: i64,
    pub currency: String,
    /// `SubscriptionIntervalTypes`: MONTH 1, YEAR 2.
    pub interval: u64,
    pub interval_count: u64,
}

fn text_of(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .filter(|text| !text.is_empty())
}

/// `convertStoreListing`: a localized field (`{default, localizations}`) or a plain string.
fn localized(value: &Value) -> Option<String> {
    text_of(&value["default"]).or_else(|| text_of(value))
}

pub(super) fn parse_listing(value: &Value) -> Option<Listing> {
    Some(Listing {
        id: text_of(&value["id"])?,
        sku: text_of(&value["sku"]["id"])?,
        sku_flags: value["sku"]["flags"].as_u64().unwrap_or(0),
        summary: localized(&value["summary"]).unwrap_or_default(),
        description: localized(&value["description"]),
        thumbnail: text_of(&value["thumbnail"]["id"]),
        benefits: value["benefits"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|benefit| Benefit {
                name: benefit["name"].as_str().unwrap_or_default().to_owned(),
                asset: text_of(&benefit["icon"]["store_asset_id"]),
                emoji: text_of(&benefit["icon"]["emoji"]),
            })
            .collect(),
    })
}

/// `createFromServer` of module 791878: the layout's subscriptions and one-time listings.
pub(super) fn parse_layout(body: &Value) -> (Vec<Listing>, Vec<Listing>) {
    let list = |key: &str| -> Vec<Listing> {
        body[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(parse_listing)
            .collect()
    };
    (list("subscriptions"), list("otps"))
}

/// The SKUs of `/store/published-listings/skus` (one published listing each, the SKU inside).
pub(super) fn parse_skus(body: &Value) -> Vec<Sku> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|listing| {
            let sku = &listing["sku"];
            Some(Sku {
                id: text_of(&sku["id"])?,
                kind: sku["type"].as_u64().unwrap_or(0),
                name: sku["name"].as_str().unwrap_or_default().to_owned(),
                price: sku["price"]["amount"].as_i64().map(|amount| {
                    (
                        amount,
                        sku["price"]["currency"]
                            .as_str()
                            .unwrap_or("usd")
                            .to_owned(),
                    )
                }),
                description: localized(&listing["description"]),
                header: text_of(&listing["header_background"]["id"]),
            })
        })
        .collect()
}

pub(super) fn parse_plans(body: &Value) -> Vec<Plan> {
    body.as_array()
        .into_iter()
        .flatten()
        .map(|plan| Plan {
            price: plan["price"].as_i64().unwrap_or(0),
            currency: plan["currency"].as_str().unwrap_or("usd").to_owned(),
            interval: plan["interval"].as_u64().unwrap_or(1),
            interval_count: plan["interval_count"].as_u64().unwrap_or(1),
        })
        .collect()
}

/// `$g` of module 580630 for the en-US locale.
fn money(amount: i64, currency: &str) -> String {
    super::shop::game_shop::format_price(amount, currency, 2)
}

/// `_J` of module 722178 (`CE`): "{price} / Month", "{price} / Year" or "{price} every {n} months".
pub(super) fn plan_rate(plan: &Plan) -> String {
    let price = money(plan.price, &plan.currency);
    match (plan.interval, plan.interval_count) {
        (2, _) => dc_intl::format("rS8FA+", &[("price", price.as_str().into())]),
        (_, 1) => dc_intl::format("AbOLNu", &[("price", price.as_str().into())]),
        (_, count) => dc_intl::format(
            "Qc+9ww",
            &[
                ("price", price.as_str().into()),
                ("intervalCount", (count as i64).into()),
            ],
        ),
    }
}

/// `oC` of module 163437: subscriptions by the price of their SKU (the first plan of a subscription SKU).
pub(super) fn sort_subscriptions(listings: &mut [Listing], price_of: impl Fn(&str) -> i64) {
    listings.sort_by_key(|listing| price_of(&listing.sku));
}

/// Module 311321: a server subscription among them makes the heading "Server Subscriptions".
pub(super) fn subscriptions_heading(listings: &[Listing]) -> (&'static str, &'static str) {
    if listings
        .iter()
        .any(|listing| listing.sku_flags & GUILD_SUBSCRIPTION != 0)
    {
        ("discover_nav_servers", "XIhSoj")
    } else {
        ("role_person", "dxfZph")
    }
}

/// `STOREFRONT_MORE_BENEFITS` of the legacy catalog (module 855522; en-US).
pub(super) fn more_benefits(count: usize) -> String {
    if count == 1 {
        "+ 1 more benefit".to_owned()
    } else {
        format!("+ {count} more benefits")
    }
}

/// The width of the `xxl` dialog in a window `window` wide: the dialog area is the window less 24px each side.
pub(super) fn modal_width(window: f32) -> f32 {
    let area = window - 48.;
    let width = if area >= 1080. { window * 0.8 } else { area };
    width.min(1280.)
}

/// `productSection_OnNQXM`'s columns at a window this wide.
pub(super) fn grid_columns(window: f32) -> usize {
    match window {
        width if width >= 1600. => 4,
        width if width >= 1220. => 3,
        width if width >= 760. => 2,
        _ => 1,
    }
}

/// The store asset address of module 371794 `YE`.
fn asset(app: &str, id: &str, size: u32) -> String {
    super::shop::game_shop::asset_url(app, id, size)
}

fn key(what: &str, app: &str) -> String {
    format!("{KEY_PREFIX}{what}:{app}")
}

/// `UserIcon`/`ServerIcon` beside a heading, `heading-lg/semibold`.
fn heading(glyph: Option<&'static str>, label: &str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .text_size(px(20.))
        .line_height(px(24.))
        .font_weight(FontWeight(600.))
        .text_color(color("--text-default"))
        .children(glyph.map(|glyph| icon(glyph, 20., "--interactive-text-default")))
        .child(label.to_owned())
}

impl Root {
    /// Store of module 571077: the modal, with its reads.
    pub(crate) fn open_app_storefront(
        &mut self,
        app: String,
        guild: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        for (what, route) in [
            (
                "app",
                format!("/applications/{app}/public?with_guild=false"),
            ),
            ("layout", format!("/applications/{app}/store-layout")),
            (
                "skus",
                format!("/store/published-listings/skus?application_id={app}"),
            ),
            (
                "entitlements",
                format!("/users/@me/applications/{app}/entitlements?exclude_consumed=false"),
            ),
        ] {
            let key = key(what, &app);
            if !self.fetched.contains_key(&key) {
                let _ = self.requests.send(Request::Fetch { key, route });
            }
        }
        self.profile = None;
        self.full_profile = None;
        self.app_storefront = Some(Open {
            app,
            guild,
            detail: None,
        });
        cx.notify();
    }

    /// The answers of the modal's reads; a layout asks for its subscriptions' plans.
    pub(super) fn app_storefront_fetched(
        &mut self,
        key: String,
        body: Value,
        cx: &mut Context<Self>,
    ) {
        if let Some(app) = key.strip_prefix(&format!("{KEY_PREFIX}layout:")) {
            let (subscriptions, _) = parse_layout(&body);
            for listing in subscriptions {
                let plans = key_plans(app, &listing.sku);
                if !self.fetched.contains_key(&plans) {
                    let _ = self.requests.send(Request::Fetch {
                        key: plans,
                        route: format!(
                            "/store/published-listings/skus/{}/subscription-plans",
                            listing.sku
                        ),
                    });
                }
            }
        }
        if let Some(app) = key.strip_prefix(&format!("{KEY_PREFIX}app:")) {
            let banner = body["bot"]["banner"]
                .as_str()
                .zip(body["bot"]["id"].as_str())
                .map(|(banner, bot)| {
                    format!("https://cdn.discordapp.com/banners/{bot}/{banner}.png?size=1024")
                });
            let icon = body["icon"].as_str().map(|icon| {
                format!("https://cdn.discordapp.com/app-icons/{app}/{icon}.png?size=100")
            });
            for url in banner.into_iter().chain(icon) {
                self.request_image(url.clone(), url);
            }
        }
        self.fetched.insert(key, body);
        cx.notify();
    }

    pub(super) fn close_app_storefront(&mut self) {
        self.app_storefront = None;
    }

    fn storefront_plans(&self, app: &str, sku: &str) -> Vec<Plan> {
        self.fetched
            .get(&key_plans(app, sku))
            .map(parse_plans)
            .unwrap_or_default()
    }

    fn storefront_skus(&self, app: &str) -> Vec<Sku> {
        self.fetched
            .get(&key("skus", app))
            .map(parse_skus)
            .unwrap_or_default()
    }

    /// `185438.A`: determines whether the viewer has an active, upcoming, or guild entitlement for a subscription SKU.
    fn storefront_sub_state(&self, app: &str, sku: &str, has_plan: bool) -> SubCardState {
        let ent = self
            .fetched
            .get(&key("entitlements", app))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|e| e["sku_id"].as_str() == Some(sku));
        if let Some(ent) = ent {
            if let Some(starts) = ent["upcoming_starts_at"].as_str()
                && !starts.is_empty()
            {
                return SubCardState::UpcomingPlan {
                    starts_at: starts.to_owned(),
                };
            }
            let is_guild = ent["guild_id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|gid| {
                    self.guild == Some(gid)
                        || self.app_storefront.as_ref().and_then(|o| o.guild) == Some(gid)
                });
            let until = ent["ends_at"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            return SubCardState::Subscribed { until, is_guild };
        }
        if !has_plan {
            SubCardState::Unavailable
        } else {
            SubCardState::Available
        }
    }

    /// `G.L` of module 56754: the account holds an entitlement to the SKU.
    fn storefront_owned(&self, app: &str, sku: &str) -> bool {
        self.fetched
            .get(&key("entitlements", app))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|entitlement| entitlement["sku_id"].as_str() == Some(sku))
    }

    fn storefront_image(&mut self, url: String) -> Option<std::sync::Arc<gpui::Image>> {
        self.request_image(url.clone(), url.clone());
        self.images.get(&url).cloned()
    }

    /// A design system button of the cards (`md`, as wide as its text).
    fn storefront_button(
        id: &'static str,
        label: String,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tone = if enabled {
            Tone::Primary
        } else {
            Tone::PrimaryDisabled
        };
        modal::content_button(
            modal::button(id, label, tone, |_, _, _| {
                // GAP: the checkout of module 638643 (no purchases here).
                tracing::debug!("storefront purchase not started");
            }),
            cx,
        )
    }

    /// The modal (module 719847).
    pub(super) fn app_storefront_layer(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.app_storefront.clone()?;
        if let Some(detail) = open.detail.clone() {
            return Some(self.storefront_detail_modal(&open.app, &detail, cx));
        }
        let app_id = open.app.clone();
        let application = self.fetched.get(&key("app", &app_id)).cloned();
        // `null == k` (the application not here yet): nothing is drawn.
        let application = application.filter(|app| app["name"].is_string())?;
        let name = application["name"].as_str().unwrap_or_default().to_owned();
        let title = dc_intl::format("XDRjs5", &[("appName", name.as_str().into())]);
        let viewport = window.viewport_size();
        let (window_width, window_height) = (f32::from(viewport.width), f32::from(viewport.height));
        let width = modal_width(window_width);
        let body = self.storefront_body(&app_id, &application, &name, window_width, cx);
        let terms = application["terms_of_service_url"]
            .as_str()
            .map(str::to_owned);
        let privacy = application["privacy_policy_url"]
            .as_str()
            .map(str::to_owned);
        let footer = if terms.is_none() && privacy.is_none() {
            div().child(dc_intl::t("3ZY+0D")).into_any_element()
        } else {
            // Module 816709 (`text_dMLfOY`: a row, 4px apart).
            let link = |id: &'static str,
                        label: &'static str,
                        url: String,
                        cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .cursor_pointer()
                    .text_color(color("--text-link"))
                    .hover(|style| style.underline())
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.follow_link(&url, &url, cx)))
            };
            div()
                .flex()
                .gap(px(4.))
                .children(terms.map(|url| link("storefront-terms", dc_intl::t("8S0kcM"), url, cx)))
                .when(
                    application["terms_of_service_url"].is_string()
                        && application["privacy_policy_url"].is_string(),
                    |row| row.child("\u{2022}"),
                )
                .children(
                    privacy.map(|url| link("storefront-privacy", dc_intl::t("ZpyeCm"), url, cx)),
                )
                .into_any_element()
        };
        let pose = super::motion::modal_pose("app-storefront-modal");
        let copy_url = format!("https://discord.com/discovery/applications/{app_id}/store");
        let copy = div()
            .id("storefront-copy-link")
            .aria_label(dc_intl::t("WqhZss"))
            .size(px(40.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(color("--control-icon-only-background-hover")))
            .child(icon(
                "discover_link",
                16.,
                "--control-icon-only-icon-default",
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_url.clone()));
                this.show_success_toast(dc_intl::t("L/PwZf").to_owned(), cx);
            }));
        let close = div()
            .id("storefront-close")
            .size(px(40.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(color("--control-icon-only-background-hover")))
            .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_app_storefront();
                cx.notify();
            }));
        let header = div()
            .flex_none()
            .pt(px(16.))
            .px(px(24.))
            .pb(px(8.))
            .flex()
            .items_start()
            // `headerLeading`: 16px into the padding and up, 12px before the title; `icon_kZsfZm` 16px down and 8px in.
            .child(
                div()
                    .flex_none()
                    .ml(px(-16.))
                    .mt(px(-16.))
                    .pr(px(12.))
                    .child(div().mt(px(16.)).ml(px(8.)).child(icon(
                        "storefront_store",
                        24.,
                        "--text-strong",
                    ))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(title.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .mr(px(-16.))
                    .mt(px(-16.))
                    .pl(px(12.))
                    .flex()
                    .gap(px(8.))
                    .child(copy)
                    .child(close),
            );
        let card = div()
            .id("app-storefront-modal")
            .aria_label(title)
            .occlude()
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .w(px(width))
            .max_h(px(window_height - 32. - 48.))
            .pt(px(8.))
            .pb(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(header)
            .child(
                div()
                    .id("app-storefront-body")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px(px(24.))
                    .child(body),
            )
            .child(
                div()
                    .flex_none()
                    .pt(px(16.))
                    .px(px(24.))
                    .pb(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(footer),
            );
        Some(
            deferred(
                div()
                    .id("app-storefront-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .pt(px(32. + 24.))
                    .pb(px(24.))
                    .bg(pose.dim(color("--background-scrim")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_app_storefront();
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `Mp` of module 55709: the header, then the subscriptions and the items.
    fn storefront_body(
        &mut self,
        app: &str,
        application: &Value,
        name: &str,
        window_width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let banner = application["bot"]["banner"]
            .as_str()
            .zip(application["bot"]["id"].as_str())
            .map(|(banner, bot)| {
                format!("https://cdn.discordapp.com/banners/{bot}/{banner}.png?size=1024")
            })
            .and_then(|url| self.images.get(&url).cloned());
        let app_icon = application["icon"]
            .as_str()
            .map(|icon| format!("https://cdn.discordapp.com/app-icons/{app}/{icon}.png?size=100"))
            .and_then(|url| self.images.get(&url).cloned());
        // `background_qZsksx`: the banner (else the icon) under `hsl(primary-730 / 0.8)`, blurred 4px.
        let backdrop = banner.clone().or_else(|| app_icon.clone());
        let quoted_subtitle = application["storefront_subtitle"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| format!("\u{201C}{s}\u{201D}"));
        let header = div()
            .relative()
            .w_full()
            .h(px(200.))
            .px(px(24.))
            .py(px(16.))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            .flex()
            .items_center()
            .gap(px(24.))
            .children(backdrop.map(|art| {
                div()
                    .absolute()
                    .inset_0()
                    .child(img(art).size_full().object_fit(ObjectFit::Cover))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .backdrop_blur(px(4.))
                            .bg(gpui::hsla(228. / 360., 0.067, 0.143, 0.8)),
                    )
            }))
            .child(
                div()
                    .relative()
                    .min_w(px(260.))
                    .max_w(px(304.))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .children(app_icon.map(|art| {
                        div()
                            .flex_none()
                            .size(px(64.))
                            .rounded(px(12.))
                            .overflow_hidden()
                            .child(img(art).size(px(64.)))
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .line_height(px(32.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(name.to_owned()),
                            )
                            .children(quoted_subtitle.map(|sub| {
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(sub)
                            })),
                    ),
            )
            .children(banner.map(|art| {
                div()
                    .relative()
                    .flex_1()
                    .h_full()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .child(img(art).size_full().object_fit(ObjectFit::Cover))
            }));
        let (mut subscriptions, otps) = self
            .fetched
            .get(&key("layout", app))
            .map(parse_layout)
            .unwrap_or_default();
        let skus = self.storefront_skus(app);
        sort_subscriptions(&mut subscriptions, |sku| {
            let found = skus.iter().find(|known| known.id == sku);
            match found {
                Some(known) if known.kind == SUBSCRIPTION => self
                    .storefront_plans(app, sku)
                    .first()
                    .map_or(0, |plan| plan.price),
                Some(known) => known.price.as_ref().map_or(0, |(amount, _)| *amount),
                None => 0,
            }
        });
        let columns = grid_columns(window_width);
        let grid = |cards: Vec<AnyElement>| {
            // `productSection_OnNQXM`: 287px columns 16px apart, 16px under the heading.
            let mut rows = Vec::new();
            let mut cards = cards.into_iter().peekable();
            while cards.peek().is_some() {
                let row: Vec<AnyElement> = cards.by_ref().take(columns).collect();
                rows.push(div().flex().gap(px(16.)).children(row));
            }
            div()
                .pt(px(16.))
                .flex()
                .flex_col()
                .gap(px(16.))
                .children(rows)
        };
        let mut sections: Vec<AnyElement> = Vec::new();
        if !subscriptions.is_empty() {
            let (glyph, label) = subscriptions_heading(&subscriptions);
            let cards: Vec<AnyElement> = subscriptions
                .iter()
                .enumerate()
                .map(|(index, listing)| self.subscription_card(app, index, listing, cx))
                .collect();
            sections.push(
                div()
                    .child(heading(Some(glyph), dc_intl::t(label)))
                    .child(grid(cards))
                    .into_any_element(),
            );
        }
        let items: Vec<AnyElement> = otps
            .iter()
            .enumerate()
            .filter_map(|(index, listing)| {
                let sku = skus.iter().find(|known| known.id == listing.sku)?.clone();
                self.item_card(app, index, &sku, cx)
            })
            .collect();
        if !otps.is_empty() {
            sections.push(
                div()
                    .child(heading(None, dc_intl::t("yUGTs8")))
                    .child(grid(items))
                    .into_any_element(),
            );
        }
        // `wrapper_OnNQXM`: 16px above and below, 40px between the header and the products and between sections.
        div()
            .py(px(16.))
            .flex()
            .flex_col()
            .gap(px(40.))
            .child(header)
            .child(div().flex().flex_col().gap(px(40.)).children(sections))
            .into_any_element()
    }

    /// Module 885996 `O` for a subscription listing (`J` of module 55709).
    fn subscription_card(
        &mut self,
        app: &str,
        index: usize,
        listing: &Listing,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let thumbnail = listing
            .thumbnail
            .as_ref()
            .and_then(|id| self.storefront_image(asset(app, id, 256)));
        let plan = self.storefront_plans(app, &listing.sku).first().cloned();
        let state = self.storefront_sub_state(app, &listing.sku, plan.is_some());
        let rate_str = plan.as_ref().map(plan_rate);
        let (label, enabled) = match (&state, rate_str.as_deref()) {
            (SubCardState::Subscribed { .. }, Some(rate)) => {
                (dc_intl::format("Hs3Y+I", &[("rate", rate.into())]), false)
            }
            (SubCardState::Subscribed { .. }, None) => (dc_intl::t("6cfuDj").to_owned(), false),
            (SubCardState::UpcomingPlan { .. }, Some(rate)) => {
                (dc_intl::format("k1ew5N", &[("rate", rate.into())]), false)
            }
            (SubCardState::UpcomingPlan { .. }, None) => (dc_intl::t("k1ew5N").to_owned(), false),
            (SubCardState::Unavailable, _) => (dc_intl::t("DLAKbi").to_owned(), false),
            (SubCardState::Available, Some(rate)) => {
                (dc_intl::format("i4T8vz", &[("rate", rate.into())]), true)
            }
            (SubCardState::Available, None) => (dc_intl::t("uuzaAK").to_owned(), true),
        };
        let sub_subtitle: Option<(bool, String)> = match &state {
            SubCardState::Subscribed {
                until: Some(date), ..
            } => Some((
                false,
                dc_intl::format("EV2lZz", &[("date", date.as_str().into())]),
            )),
            SubCardState::Subscribed { is_guild: true, .. } => {
                Some((true, dc_intl::t("l8ufqL").to_owned()))
            }
            SubCardState::UpcomingPlan { starts_at } => Some((
                false,
                dc_intl::format("2w6+NN", &[("date", starts_at.as_str().into())]),
            )),
            _ => None,
        };
        // Module 55709: cards with a subscription status subtitle show at most 4 benefits instead of 5.
        let max_benefits = if sub_subtitle.is_some() {
            MAX_BENEFITS.saturating_sub(1)
        } else {
            MAX_BENEFITS
        };
        let benefits: Vec<AnyElement> = listing
            .benefits
            .iter()
            .take(max_benefits)
            .map(|benefit| {
                let art = benefit
                    .asset
                    .as_ref()
                    .and_then(|id| self.storefront_image(asset(app, id, 64)));
                // `cardBenefit_7LZP3_`: the 24px icon 16px before the text.
                div()
                    .w_full()
                    .flex()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex_none()
                            .size(px(24.))
                            .text_size(px(24.))
                            .line_height(px(24.))
                            .children(
                                art.map(|art| img(art).size(px(24.)).object_fit(ObjectFit::Cover)),
                            )
                            .children(benefit.emoji.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(benefit.name.clone()),
                    )
                    .into_any_element()
            })
            .collect();
        let more = listing.benefits.len().saturating_sub(max_benefits);
        let detail_listing = listing.clone();
        div()
            .id(SharedString::from(format!(
                "storefront-subscription-{index}"
            )))
            .w(px(CARD))
            .flex_none()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--card-background-default"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(open) = this.app_storefront.as_mut() {
                    open.detail = Some(StorefrontDetail::Subscription(detail_listing.clone()));
                }
                cx.notify();
            }))
            .child(
                div()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .self_stretch()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(16.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-default"))
                                    .child(listing.summary.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(80.))
                                    .rounded(px(12.))
                                    .overflow_hidden()
                                    .bg(color("--brand-500"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(match thumbnail {
                                        Some(art) => img(art).size(px(80.)).into_any_element(),
                                        None => icon("storefront_badge", 48., "--icon-strong")
                                            .into_any_element(),
                                    }),
                            )
                            .child({
                                let is_guild_sub = (listing.sku_flags & GUILD_SUBSCRIPTION) != 0;
                                let sku_id = listing.sku.clone();
                                let app_id_click = app.to_owned();
                                let tone = if enabled {
                                    Tone::Primary
                                } else {
                                    Tone::PrimaryDisabled
                                };
                                modal::content_button(
                                    modal::button(
                                        "storefront-subscribe",
                                        label,
                                        tone,
                                        move |this, _, cx| {
                                            if !enabled {
                                                return;
                                            }
                                            if is_guild_sub {
                                                this.open_guild_sub_selection(
                                                    &app_id_click,
                                                    &sku_id,
                                                    cx,
                                                );
                                            } else {
                                                tracing::debug!("storefront purchase not started");
                                            }
                                        },
                                    ),
                                    cx,
                                )
                            })
                            .children(sub_subtitle.map(|(with_guild, text)| {
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(4.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .when(with_guild, |row| {
                                        row.child(icon("storefront_store", 14., "--text-muted"))
                                    })
                                    .child(text)
                            }))
                            .children(listing.description.clone().map(|description| {
                                div()
                                    .w_full()
                                    .max_h(px(36.))
                                    .overflow_hidden()
                                    .text_center()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .line_clamp(2)
                                    .child(description)
                            })),
                    ),
            )
            .when(!benefits.is_empty(), |card| {
                card.child(
                    div()
                        .flex_grow(1.)
                        .p(px(16.))
                        .bg(color("--background-base-lower"))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_start()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        // `STOREFRONT_BENEFITS_TITLE` of the legacy catalog (en-US).
                                        .child("Exclusive to this tier"),
                                )
                                .children(benefits)
                                .when(more > 0, |list| {
                                    list.child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-subtle"))
                                            .child(more_benefits(more)),
                                    )
                                }),
                        ),
                )
            })
            .into_any_element()
    }

    /// `X` of module 55709: a one-time item (none without a price).
    fn item_card(
        &mut self,
        app: &str,
        index: usize,
        sku: &Sku,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (amount, currency) = sku.price.clone()?;
        let header = sku
            .header
            .as_ref()
            .and_then(|id| self.storefront_image(asset(app, id, 256)));
        let durable = sku.kind == DURABLE;
        let owned = durable && self.storefront_owned(app, &sku.id);
        let availability = durable.then(|| {
            if owned {
                dc_intl::t("bm82mm")
            } else {
                dc_intl::t("6gprwf")
            }
        });
        let button = if owned {
            Self::storefront_button(
                "storefront-acquired",
                dc_intl::t("6cfuDj").to_owned(),
                false,
                cx,
            )
        } else {
            Self::storefront_button(
                "storefront-buy",
                dc_intl::format(
                    "Xp5WTn",
                    &[("price", money(amount, &currency).as_str().into())],
                ),
                true,
                cx,
            )
        };
        let detail_sku = sku.clone();
        Some(
            div()
                .id(SharedString::from(format!("storefront-item-{index}")))
                .w(px(CARD))
                .flex_none()
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--card-background-default"))
                .overflow_hidden()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(open) = this.app_storefront.as_mut() {
                        open.detail = Some(StorefrontDetail::Item(detail_sku.clone()));
                    }
                    cx.notify();
                }))
                .child(
                    // `cardHeaderImg_aMqvA2`: 187px on `--brand-400`, the picture covering it.
                    div()
                        .w_full()
                        .h(px(187.))
                        .bg(color("--brand-400"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(match header {
                            Some(art) => img(art)
                                .size_full()
                                .object_fit(ObjectFit::Cover)
                                .into_any_element(),
                            None => icon("system_ticket", 80., "--white").into_any_element(),
                        }),
                )
                .child(
                    div()
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap(px(10.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(sku.name.clone()),
                        )
                        .children(sku.description.clone().map(|description| {
                            div()
                                .w_full()
                                .h(px(36.))
                                .overflow_hidden()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-muted"))
                                .line_clamp(2)
                                .child(description)
                        })),
                )
                .child(
                    div()
                        .p(px(16.))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--interactive-text-default"))
                                .children(availability),
                        )
                        .child(div().flex().items_center().gap(px(8.)).child(button)),
                )
                .into_any_element(),
        )
    }

    /// Card details modal (modules 168393 `SubscriptionDetailsModal` & 963179 `ItemDetailsModal`,
    /// opened by clicking any card in module 55709).
    fn storefront_detail_modal(
        &mut self,
        app: &str,
        detail: &StorefrontDetail,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (title, badge, description, extra_body, cta): (
            String,
            Option<String>,
            Option<String>,
            Option<AnyElement>,
            AnyElement,
        ) = match detail {
            StorefrontDetail::Subscription(listing) => {
                let is_guild = (listing.sku_flags & GUILD_SUBSCRIPTION) != 0;
                let tag = if is_guild {
                    dc_intl::t("XIhSoj").to_owned()
                } else {
                    dc_intl::t("dxfZph").to_owned()
                };
                let plan = self.storefront_plans(app, &listing.sku).first().cloned();
                let state = self.storefront_sub_state(app, &listing.sku, plan.is_some());
                let rate_str = plan.as_ref().map(plan_rate);
                let (label, enabled) = match (&state, rate_str.as_deref()) {
                    (SubCardState::Subscribed { .. }, Some(rate)) => {
                        (dc_intl::format("Hs3Y+I", &[("rate", rate.into())]), false)
                    }
                    (SubCardState::UpcomingPlan { .. }, Some(rate)) => {
                        (dc_intl::format("k1ew5N", &[("rate", rate.into())]), false)
                    }
                    (SubCardState::Unavailable, _) => (dc_intl::t("DLAKbi").to_owned(), false),
                    (SubCardState::Available, Some(rate)) => {
                        (dc_intl::format("i4T8vz", &[("rate", rate.into())]), true)
                    }
                    _ => (dc_intl::t("uuzaAK").to_owned(), true),
                };
                let benefits: Vec<AnyElement> = listing
                    .benefits
                    .iter()
                    .map(|b| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(14.))
                            .text_color(color("--text-default"))
                            .child(icon(
                                "circle_check",
                                16.,
                                "--control-primary-background-default",
                            ))
                            .child(b.name.clone())
                            .into_any_element()
                    })
                    .collect();
                let body = (!benefits.is_empty()).then(|| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .children(benefits)
                        .into_any_element()
                });
                let sku_id = listing.sku.clone();
                let app_click = app.to_owned();
                let cta = modal::content_button(
                    modal::button(
                        "storefront-detail-sub-cta",
                        label,
                        if enabled {
                            Tone::Primary
                        } else {
                            Tone::PrimaryDisabled
                        },
                        move |this, _, cx| {
                            if !enabled {
                                return;
                            }
                            if let Some(open) = this.app_storefront.as_mut() {
                                open.detail = None;
                            }
                            if is_guild {
                                this.open_guild_sub_selection(&app_click, &sku_id, cx);
                            }
                            cx.notify();
                        },
                    ),
                    cx,
                );
                (
                    listing.summary.clone(),
                    Some(tag),
                    listing.description.clone(),
                    body,
                    cta,
                )
            }
            StorefrontDetail::Item(sku) => {
                let durable = sku.kind == DURABLE;
                let owned = durable && self.storefront_owned(app, &sku.id);
                let avail = durable.then(|| {
                    if owned {
                        dc_intl::t("bm82mm").to_owned()
                    } else {
                        dc_intl::t("6gprwf").to_owned()
                    }
                });
                let cta = if owned {
                    Self::storefront_button(
                        "storefront-detail-acquired",
                        dc_intl::t("6cfuDj").to_owned(),
                        false,
                        cx,
                    )
                } else if let Some((amount, currency)) = &sku.price {
                    Self::storefront_button(
                        "storefront-detail-buy",
                        dc_intl::format(
                            "Xp5WTn",
                            &[("price", money(*amount, currency).as_str().into())],
                        ),
                        true,
                        cx,
                    )
                } else {
                    Self::storefront_button(
                        "storefront-detail-unavail",
                        dc_intl::t("DLAKbi").to_owned(),
                        false,
                        cx,
                    )
                };
                (sku.name.clone(), avail, sku.description.clone(), None, cta)
            }
        };
        let dialog = div()
            .id("storefront-detail-modal")
            .occlude()
            .w(px(480.))
            .rounded(px(12.))
            .bg(color("--modal-background"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
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
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .children(badge.map(|b| {
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-muted"))
                                    .child(b)
                            })),
                    )
                    .child(
                        div()
                            .id("storefront-detail-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.app_storefront.as_mut() {
                                    open.detail = None;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .px(px(20.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .children(description.map(|d| {
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(d)
                    }))
                    .children(extra_body),
            )
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .bg(color("--modal-footer-background"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(cta),
            );
        deferred(
            div()
                .id("storefront-detail-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(gpui::rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(open) = this.app_storefront.as_mut() {
                        open.detail = None;
                    }
                    cx.notify();
                }))
                .child(dialog),
        )
        .with_priority(3)
        .into_any_element()
    }
}

fn key_plans(app: &str, sku: &str) -> String {
    format!("{KEY_PREFIX}plans:{app}:{sku}")
}

/// Whether a bot's profile application offers the Store button (`G` of module 658575: `storefront_available`).
pub(super) fn storefront_available(application: &Value) -> bool {
    application["storefront_available"].as_bool() == Some(true)
}

/// REST key for eligible app-subscription guilds (`399143`).
pub(crate) const ELIGIBLE_SUB_GUILDS_KEY: &str = "app-storefront:eligible-sub-guilds";

#[derive(Clone, Debug)]
pub(crate) struct EligibleSubGuild {
    pub id: dc_state::Id,
    pub name: String,
    pub is_current: bool,
}

/// Server Selection modal for an application guild-subscription checkout (module 399143:
/// `xgtI/K`, `rAXXxN`, `cpT0Cq`, `cY+Oob`, `5qyruI`, `RjtuAI`, `M2TbbK`).
#[derive(Clone, Debug)]
pub(crate) struct GuildSubscriptionSelectionModal {
    pub app_id: String,
    pub sku_id: String,
    pub loading: bool,
    pub guilds: Vec<EligibleSubGuild>,
    pub selected_guild_id: Option<dc_state::Id>,
    pub dropdown_open: bool,
}

/// `GET /users/@me/billing/eligible-application-subscription-guilds` (module 399143).
pub(crate) fn eligible_sub_guilds_request(app_id: &str, sku_id: &str) -> Request {
    Request::Rest {
        key: Some(ELIGIBLE_SUB_GUILDS_KEY.to_owned()),
        method: reqwest::Method::GET,
        route: format!(
            "/users/@me/billing/eligible-application-subscription-guilds?application_id={app_id}&sku_id={sku_id}"
        ),
        body: None,
    }
}

impl Root {
    /// Opens the Server Selection modal (`399143`) for a guild-scoped application subscription.
    pub(super) fn open_guild_sub_selection(
        &mut self,
        app_id: &str,
        sku_id: &str,
        cx: &mut Context<Self>,
    ) {
        let current = self.guild;
        let _ = self
            .requests
            .send(eligible_sub_guilds_request(app_id, sku_id));
        let mut guilds: Vec<EligibleSubGuild> = self
            .stores
            .guilds
            .iter()
            .map(|g| EligibleSubGuild {
                id: g.id,
                name: g.name.clone(),
                is_current: current == Some(g.id),
            })
            .collect();
        guilds.sort_by_key(|g| !g.is_current);
        let selected_guild_id = guilds.iter().find(|g| g.is_current).map(|g| g.id);
        self.guild_sub_selection_modal = Some(GuildSubscriptionSelectionModal {
            app_id: app_id.to_owned(),
            sku_id: sku_id.to_owned(),
            loading: false,
            guilds,
            selected_guild_id,
            dropdown_open: false,
        });
        cx.notify();
    }

    /// Modal layer for Application Guild Subscription Server Selection (`399143`).
    pub(super) fn guild_sub_selection_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.guild_sub_selection_modal.as_ref()?;
        let can_confirm = modal.selected_guild_id.is_some();
        let selected_guild = modal
            .selected_guild_id
            .and_then(|sid| modal.guilds.iter().find(|g| g.id == sid));
        let app_id = modal.app_id.clone();
        let sku_id = modal.sku_id.clone();

        Some(
            deferred(
                div()
                    .id("app-guild-sub-select-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.guild_sub_selection_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("app-guild-sub-select-modal")
                            .w(px(440.))
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
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(20.))
                                                    .line_height(px(24.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("xgtI/K")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .line_height(px(18.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("rAXXxN")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("app-guild-sub-select-close")
                                            .flex_none()
                                            .size(px(30.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.guild_sub_selection_modal = None;
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
                                    .gap(px(12.))
                                    .child(if modal.loading {
                                        div()
                                            .h(px(44.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(13.))
                                            .text_color(color("--text-muted"))
                                            .child("...")
                                            .into_any_element()
                                    } else if modal.guilds.is_empty() {
                                        div()
                                            .p(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--status-warning"))
                                            .bg(gpui::rgba(0xf0b2321f))
                                            .flex()
                                            .items_center()
                                            .gap(px(10.))
                                            .child(icon("circle_info", 18., "--status-warning"))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_size(px(13.))
                                                    .line_height(px(18.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("M2TbbK")),
                                            )
                                            .into_any_element()
                                    } else {
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("5qyruI").to_uppercase()),
                                            )
                                            .child(
                                                div()
                                                    .id("app-guild-sub-select-trigger")
                                                    .cursor_pointer()
                                                    .min_h(px(44.))
                                                    .px(px(12.))
                                                    .py(px(8.))
                                                    .rounded(px(8.))
                                                    .border_1()
                                                    .border_color(color("--border-subtle"))
                                                    .bg(color("--background-base-lowest"))
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap(px(10.))
                                                    .child(if let Some(g) = selected_guild {
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(10.))
                                                            .child(
                                                                div()
                                                                    .size(px(24.))
                                                                    .rounded(px(6.))
                                                                    .bg(color("--brand-500"))
                                                                    .flex()
                                                                    .items_center()
                                                                    .justify_center()
                                                                    .text_size(px(11.))
                                                                    .font_weight(FontWeight(700.))
                                                                    .text_color(color("--white"))
                                                                    .child(
                                                                        g.name
                                                                            .chars()
                                                                            .next()
                                                                            .unwrap_or('S')
                                                                            .to_uppercase()
                                                                            .to_string(),
                                                                    ),
                                                            )
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .flex_col()
                                                                    .child(
                                                                        div()
                                                                            .text_size(px(14.))
                                                                            .text_color(color(
                                                                                "--text-strong",
                                                                            ))
                                                                            .child(g.name.clone()),
                                                                    )
                                                                    .when(g.is_current, |col| {
                                                                        col.child(
                                                                            div()
                                                                                .text_size(px(11.))
                                                                                .text_color(color(
                                                                                    "--text-muted",
                                                                                ))
                                                                                .child(dc_intl::t(
                                                                                    "RjtuAI",
                                                                                )),
                                                                        )
                                                                    }),
                                                            )
                                                            .into_any_element()
                                                    } else {
                                                        div()
                                                            .text_size(px(14.))
                                                            .text_color(color("--text-muted"))
                                                            .child(dc_intl::t("5qyruI"))
                                                            .into_any_element()
                                                    })
                                                    .child(icon(
                                                        "shop_select_chevron",
                                                        16.,
                                                        "--icon-subtle",
                                                    ))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(m) = this
                                                            .guild_sub_selection_modal
                                                            .as_mut()
                                                        {
                                                            m.dropdown_open = !m.dropdown_open;
                                                        }
                                                        cx.notify();
                                                    })),
                                            )
                                            .when(modal.dropdown_open, |col| {
                                                col.child(
                                                    div()
                                                        .rounded(px(8.))
                                                        .border_1()
                                                        .border_color(color("--border-subtle"))
                                                        .bg(color("--background-base-lower"))
                                                        .p(px(4.))
                                                        .flex()
                                                        .flex_col()
                                                        .gap(px(2.))
                                                        .children(modal.guilds.iter().map(|g| {
                                                            let gid = g.id;
                                                            let active =
                                                                modal.selected_guild_id == Some(gid);
                                                            div()
                                                                .id(("app-guild-sub-opt", gid))
                                                                .cursor_pointer()
                                                                .px(px(10.))
                                                                .py(px(8.))
                                                                .rounded(px(6.))
                                                                .bg(if active {
                                                                    color("--background-mod-subtle")
                                                                } else {
                                                                    gpui::rgba(0x00000000)
                                                                })
                                                                .hover(|s| {
                                                                    s.bg(color(
                                                                        "--background-mod-subtle",
                                                                    ))
                                                                })
                                                                .flex()
                                                                .flex_col()
                                                                .child(
                                                                    div()
                                                                        .text_size(px(14.))
                                                                        .text_color(color(
                                                                            "--text-strong",
                                                                        ))
                                                                        .child(g.name.clone()),
                                                                )
                                                                .when(g.is_current, |r| {
                                                                    r.child(
                                                                        div()
                                                                            .text_size(px(11.))
                                                                            .text_color(color(
                                                                                "--text-muted",
                                                                            ))
                                                                            .child(dc_intl::t(
                                                                                "RjtuAI",
                                                                            )),
                                                                    )
                                                                })
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        if let Some(m) = this
                                                                            .guild_sub_selection_modal
                                                                            .as_mut()
                                                                        {
                                                                            m.selected_guild_id =
                                                                                Some(gid);
                                                                            m.dropdown_open = false;
                                                                        }
                                                                        cx.notify();
                                                                    },
                                                                ))
                                                        })),
                                                )
                                            })
                                            .into_any_element()
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
                                            .id("app-guild-sub-select-cancel")
                                            .cursor_pointer()
                                            .px(px(16.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .bg(color("--control-secondary-background-default"))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("cpT0Cq"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.guild_sub_selection_modal = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("app-guild-sub-select-confirm")
                                            .when(can_confirm, |b| b.cursor_pointer())
                                            .when(!can_confirm, |b| b.opacity(0.5))
                                            .px(px(18.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .bg(color("--control-primary-background-default"))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("cY+Oob"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !can_confirm {
                                                    return;
                                                }
                                                tracing::debug!(
                                                    %app_id,
                                                    %sku_id,
                                                    "guild subscription server selected"
                                                );
                                                this.guild_sub_selection_modal = None;
                                                this.put_app_toast(
                                                    dc_intl::t("cY+Oob").to_owned(),
                                                    true,
                                                    cx,
                                                );
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(97)
            .into_any_element(),
        )
    }

    /// Capture harness: `app-guild-sub [open|empty|close]` (module 399143).
    #[cfg(feature = "capture")]
    pub(crate) fn script_app_guild_sub(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.guild_sub_selection_modal = None;
                cx.notify();
                "app-guild-sub: closed".to_owned()
            }
            "empty" => {
                self.guild_sub_selection_modal = Some(GuildSubscriptionSelectionModal {
                    app_id: "12345".to_owned(),
                    sku_id: "67890".to_owned(),
                    loading: false,
                    guilds: Vec::new(),
                    selected_guild_id: None,
                    dropdown_open: false,
                });
                cx.notify();
                "app-guild-sub: empty".to_owned()
            }
            _ => {
                let current = self
                    .guild
                    .or_else(|| self.stores.guilds.first().map(|g| g.id));
                let mut guilds: Vec<EligibleSubGuild> = self
                    .stores
                    .guilds
                    .iter()
                    .map(|g| EligibleSubGuild {
                        id: g.id,
                        name: g.name.clone(),
                        is_current: current == Some(g.id),
                    })
                    .collect();
                if guilds.is_empty() {
                    guilds.push(EligibleSubGuild {
                        id: 1,
                        name: "gravecord fixture".to_owned(),
                        is_current: true,
                    });
                }
                let selected_guild_id = guilds.first().map(|g| g.id);
                self.guild_sub_selection_modal = Some(GuildSubscriptionSelectionModal {
                    app_id: "12345".to_owned(),
                    sku_id: "67890".to_owned(),
                    loading: false,
                    guilds,
                    selected_guild_id,
                    dropdown_open: false,
                });
                cx.notify();
                "app-guild-sub: open".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_layout_gives_its_subscriptions_and_items() {
        let body = json!({
            "subscriptions": [{"id": "1", "sku": {"id": "10", "application_id": "5", "flags": USER_SUBSCRIPTION},
                "summary": {"default": "Gold"}, "description": {"default": "The best"}, "thumbnail": {"id": "77"},
                "benefits": [{"id": "b", "name": "Perk", "icon": {"type": 1, "store_asset_id": "88"}}]}],
            "otps": [{"id": "2", "sku": {"id": "20", "application_id": "5", "flags": 0}, "summary": {"default": "Coins"}}],
        });
        let (subscriptions, otps) = parse_layout(&body);
        assert_eq!(subscriptions.len(), 1);
        assert_eq!(subscriptions[0].summary, "Gold");
        assert_eq!(subscriptions[0].description.as_deref(), Some("The best"));
        assert_eq!(subscriptions[0].thumbnail.as_deref(), Some("77"));
        assert_eq!(subscriptions[0].benefits[0].asset.as_deref(), Some("88"));
        assert_eq!(otps[0].sku, "20");
        assert_eq!(subscriptions_heading(&subscriptions).1, "dxfZph");
    }

    #[test]
    fn skus_and_plans_carry_prices() {
        let skus = parse_skus(&json!([{"sku": {"id": "20", "type": 2, "name": "Sword",
            "price": {"amount": 499, "currency": "usd"}}, "description": {"default": "Sharp"},
            "header_background": {"id": "9"}}]));
        assert_eq!(skus[0].price, Some((499, "usd".to_owned())));
        assert_eq!(skus[0].header.as_deref(), Some("9"));
        let plans = parse_plans(
            &json!([{"price": 299, "currency": "usd", "interval": 1, "interval_count": 1}]),
        );
        assert_eq!(plan_rate(&plans[0]), "$2.99 / Month");
        assert_eq!(
            plan_rate(&Plan {
                price: 2999,
                currency: "usd".into(),
                interval: 2,
                interval_count: 1
            }),
            "$29.99 / Year"
        );
        assert_eq!(
            dc_intl::format("Xp5WTn", &[("price", money(499, "usd").as_str().into())]),
            "Buy for $4.99"
        );
    }

    #[test]
    fn subscriptions_go_cheapest_first() {
        let mut listings = vec![
            Listing {
                sku: "a".into(),
                ..Default::default()
            },
            Listing {
                sku: "b".into(),
                ..Default::default()
            },
        ];
        sort_subscriptions(&mut listings, |sku| if sku == "a" { 500 } else { 100 });
        assert_eq!(listings[0].sku, "b");
    }

    #[test]
    fn the_dialog_and_grid_follow_the_window() {
        assert_eq!(modal_width(1000.), 952.);
        assert_eq!(modal_width(1400.), 1120.);
        assert_eq!(modal_width(2000.), 1280.);
        assert_eq!(grid_columns(700.), 1);
        assert_eq!(grid_columns(1280.), 3);
        assert_eq!(more_benefits(1), "+ 1 more benefit");
        assert_eq!(more_benefits(3), "+ 3 more benefits");
        assert!(storefront_available(&json!({"storefront_available": true})));
        assert!(!storefront_available(&json!({})));
    }
}
