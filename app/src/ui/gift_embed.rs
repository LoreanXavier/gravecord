//! Gift links in messages become gift cards (`renderGiftCodes` and `o2`/`o0` of module 679636).
//!
//! A message's codes (`giftCodes`, `e7`/`pF` of module 45938, in `dc_state::gift_codes`) are resolved the way
//! GiftCodeStore does it for loaded messages (module 30793 `S`): `GIFT_CODE_RESOLVE`, then
//! `GET /entitlements/gift-codes/{code}?with_application=false&with_subscription_plan=true`, then
//! `GIFT_CODE_RESOLVE_SUCCESS` or `_FAILURE` with the API error.
//!
//! Each code draws one card in a full-width container (`giftCodeContainer_YUQhib`), sized by that container's width
//! (`withSize`, module 456412) as the official measures it, so the first frame lays out at width 0:
//!
//! - a promotion code: the SKU tile titled "You got a promotion code!" with Claim;
//! - no gift (unknown or revoked): the skeleton while it resolves, then "You sent/received a gift, but..." (`oQ`);
//! - a gift: the SKU tile (`oz`, module 679636) with the gift's title, line, Accept/Open Gift/Claimed and art. The
//!   tile is the skeleton until the SKU and its listing are known (the resolve answer carries both).
//!
//! The tile (`tile_lS7BT7` with `embedHorizontal_mVuBLJ`/`embedVertical_mVuBLJ`, module 622413 and 863574) is
//! content-box: 660 by 173 inside a 4px padding and a 1px border when wider than 660, else 240 by 300; the art takes
//! 305px on the right (`mediaHorizontal_lS7BT7`) or the top. A styled gift's art is its box animation (module
//! 446263; the Standard Box's idle file, its loop file once redeemed) at its first frame; an unstyled one shows the
//! listing's thumbnail (`splash_1TNd4r`, cover).
use super::settings::gift_redeem::{Gift, Modal, message_resolve_route};
use super::{Root, color};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::Message;
use dc_state::gift_codes::{self, GiftCode, INVALID_GIFT_REDEMPTION_CLIENT_UPDATE_REQUIRED};
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, ObjectFit, SharedString,
    StyledImage, div, img, prelude::*, px,
};
use serde_json::{Value, json};

/// The key of a gift's resolve: `gift-code-resolve:{code}`.
const RESOLVE_KEY: &str = "gift-code-resolve:";
/// The key of a gift box picture: `gift-box:{style}:{idle|loop}` (drawn from its lottie file in `net.rs`).
pub(crate) const GIFT_BOX_KEY: &str = "gift-box:";
/// The key of a listing's thumbnail: `gift-thumb:{sku}`.
const THUMB_KEY: &str = "gift-thumb:";

/// `STANDARD_BOX` of `o2` (module 682901) and its two files (modules 567091 and 598683, chosen by module 446263).
const STANDARD_BOX: u64 = 4;
const STANDARD_BOX_IDLE: &str = "https://cdn.discordapp.com/assets/content/c1363a4c21c7694c37c11fd80a6b2a78caa1632059dbbadb88c08e1fc5501fbc.lottiejson";
const STANDARD_BOX_LOOP: &str = "https://cdn.discordapp.com/assets/content/13050a4f904230e1617429a7f6f42ab959e81a99c099d4542eb43af9beb612d2.lottiejson";
/// `SOCIAL_LAYER_GAME_ITEM` of the product lines (`EZt`, module 652215): `bF` of module 871123.
const SOCIAL_LAYER_GAME_ITEM: u64 = 14;
/// The width the tile turns horizontal past (`oz`: `width > 660`; `o0` and `oQ`: `width >= 660`).
const HORIZONTAL: f32 = 660.;

/// The lottie files are 900 by 700 (`w`, `h`).
const BOX_ASPECT: f32 = 700. / 900.;

/// What the card shows, from the store (`o2`'s connected props).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Card {
    /// `a1.W`: an empty tile.
    Loading { horizontal: bool },
    /// `oQ`: the gift link is no longer active.
    Invalid {
        horizontal: bool,
        is_self: bool,
        client_update: bool,
    },
    /// The SKU tile.
    Tile(Box<Tile>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Tile {
    pub horizontal: bool,
    pub title: Option<String>,
    /// Runs of the line: text and its rich-text tag (`$b`, `$link`).
    pub tagline: Vec<(String, Option<Box<str>>)>,
    pub button: Option<Button>,
    /// "x of y copies left" and "Expires in n hours", stacked on the right.
    pub metadata: Vec<String>,
    pub art: Art,
    /// A Shop gift's card takes no pointer but its button (`collectiblesEmbedWrapper_c9v7J5`).
    pub collectibles: bool,
    /// A promotion's Claim, the rest's Accept.
    pub promotion: bool,
    pub subscription: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Button {
    pub text: String,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Art {
    /// `renderCustomGiftBox`: the style and whether it is redeemed (loop) or not (idle).
    Box {
        style: u64,
        redeemed: bool,
    },
    /// The listing's thumbnail.
    Thumbnail {
        key: String,
    },
    None,
}

/// What `o0` reads besides the gift: who is looking and who sent it.
pub(super) struct Viewer<'a> {
    pub name: &'a str,
    pub verified: bool,
    pub premium: bool,
    /// `isSelfGift`: the gift's sender is the viewer (or, without a gift, the message's author is).
    pub is_self: bool,
    pub custom_gift_message: bool,
    pub content: &'a str,
}

fn runs(key: &str, values: &dc_intl::Args<'_>) -> Vec<(String, Option<Box<str>>)> {
    dc_intl::format_runs(key, values)
}

fn text(value: &str) -> Vec<(String, Option<Box<str>>)> {
    vec![(value.to_owned(), None)]
}

/// `renderTitle` of `o0`.
pub(super) fn title(gift: &GiftCode, viewer: &Viewer) -> Option<String> {
    if viewer.custom_gift_message && !viewer.is_self {
        return Some(dc_intl::format(
            "t1SOId",
            &[("recipientDisplayName", viewer.name.into())],
        ));
    }
    Some(
        match (gift.is_subscription(), viewer.is_self) {
            (true, true) => dc_intl::t("2PJ1NP"),
            (true, false) => dc_intl::t("hrnGng"),
            (false, true) => dc_intl::t("QLEMld"),
            (false, false) => dc_intl::t("W4DBcy"),
        }
        .to_owned(),
    )
}

/// `renderBody` of `o0`. The library application is never known (the game library is not modelled), so its branches
/// are not reached.
pub(super) fn body(
    gift: &GiftCode,
    sku: &gift_codes::Sku,
    plan: Option<&gift_codes::SubscriptionPlan>,
    gifter: Option<&gift_codes::Gifter>,
    viewer: &Viewer,
) -> Vec<(String, Option<Box<str>>)> {
    let game_item = sku.product_line == Some(SOCIAL_LAYER_GAME_ITEM);
    if !viewer.is_self && viewer.custom_gift_message && !gift.redeemed {
        return if game_item {
            text(viewer.content)
        } else {
            Vec::new()
        };
    }
    let name = if sku.preorder {
        dc_intl::format("evinTd", &[("name", sku.name.as_str().into())])
    } else {
        sku.name.clone()
    };
    let sku_name = || ("skuName", dc_intl::Arg::from(name.as_str()));
    if gift.redeemed {
        return if gift.is_subscription() || gift.is_collectibles() || game_item {
            text(dc_intl::t("mVC3Cv"))
        } else {
            runs("ss/L+/", &[sku_name()])
        };
    }
    if gift.is_claimed() {
        return text(dc_intl::t("ARWFQX"));
    }
    if !viewer.verified {
        return runs("GQxl7v", &[]);
    }
    if viewer.is_self {
        return if gift.is_self_redeemable() {
            text(dc_intl::t("lQI+cB"))
        } else {
            text(&dc_intl::format("A+etHx", &[sku_name()]))
        };
    }
    if gift.is_existing_premium_subscription_disallowed() {
        return text(dc_intl::t("UCIU9y"));
    }
    let max = dc_intl::Arg::Num(gift.max_uses as f64);
    if gift.has_multiple_copies() {
        return match (gifter, gift.is_subscription()) {
            (Some(gifter), true) => runs(
                "l3VxgG",
                &[
                    ("username", gifter.tag().into()),
                    ("maxUses", max),
                    sku_name(),
                ],
            ),
            (Some(gifter), false) => runs(
                "9cYrw5",
                &[
                    ("username", gifter.tag().into()),
                    ("totalCopies", max),
                    sku_name(),
                ],
            ),
            (None, true) => text(&dc_intl::format("svrO3W", &[("maxUses", max), sku_name()])),
            (None, false) => text(&dc_intl::format(
                "3AgAn3",
                &[("totalCopies", max), sku_name()],
            )),
        };
    }
    if gift.is_subscription() {
        let Some(plan) = plan else {
            return text(dc_intl::t("ZTNur7"));
        };
        let count = (
            "intervalCount",
            dc_intl::Arg::Num(plan.interval_count as f64),
        );
        return match gifter {
            Some(gifter) => runs(
                if plan.is_month() { "/RDIEA" } else { "3CX6Ev" },
                &[("username", gifter.tag().into()), sku_name(), count],
            ),
            None => runs(
                if plan.is_month() { "2O4lo5" } else { "+XjmsR" },
                &[sku_name(), count],
            ),
        };
    }
    match gifter {
        Some(gifter) => runs("3HsdQ/", &[("username", gifter.tag().into())]),
        None => text(dc_intl::t("Jdnjjj")),
    }
}

/// `renderButton` of `o0`: Claimed once redeemed, Open Gift for a styled gift, Accept otherwise; disabled when it is
/// redeemed, claimed, the viewer's own gift that may not go to its sender, a gift an existing subscription may not
/// take, or the account is not verified.
pub(super) fn button(gift: &GiftCode, viewer: &Viewer) -> Button {
    let refused = (!gift.is_self_redeemable() && viewer.is_self)
        || (gift.is_existing_premium_subscription_disallowed() && viewer.premium);
    Button {
        text: if gift.redeemed {
            dc_intl::t("BTihou")
        } else if gift.gift_style.is_some() {
            dc_intl::t("TiZFqX")
        } else {
            dc_intl::t("bUvv1f")
        }
        .to_owned(),
        disabled: gift.redeemed || refused || gift.is_claimed() || !viewer.verified,
    }
}

/// The right column of `renderActions`: the copies left, then the hours left of an unclaimed gift that expires.
pub(super) fn metadata(gift: &GiftCode, now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    let mut lines = Vec::new();
    if gift.has_multiple_copies() {
        lines.push(dc_intl::format(
            if gift.is_subscription() {
                "4iHwKT"
            } else {
                "YeLq88"
            },
            &[
                ("remaining", dc_intl::Arg::Num(gift.remaining_uses() as f64)),
                ("total", dc_intl::Arg::Num(gift.max_uses as f64)),
            ],
        ));
    }
    if !gift.redeemed
        && !gift.is_claimed()
        && let Some(at) = gift.expires_at
    {
        // moment's `diff(now, "h")` truncates toward zero.
        let hours = (at - now).num_hours();
        lines.push(dc_intl::format(
            "nZBvUR",
            &[("hours", dc_intl::Arg::Num(hours as f64))],
        ));
    }
    lines
}

/// `o0.render` and `oz.render` for one code.
pub(super) fn card(stores: &dc_state::Stores, code: &str, width: f32, viewer: &Viewer) -> Card {
    let gift = stores.gift_code(code);
    let error = stores
        .gift_code_error(code)
        .and_then(|error| error["code"].as_i64());
    let gift = match gift {
        Some(gift) if !gift.revoked || gift.promotion => gift,
        _ => {
            return if stores.gift_code_is_resolved(code) {
                Card::Invalid {
                    horizontal: width >= HORIZONTAL,
                    is_self: viewer.is_self,
                    client_update: error == Some(INVALID_GIFT_REDEMPTION_CLIENT_UPDATE_REQUIRED),
                }
            } else {
                Card::Loading {
                    horizontal: width >= HORIZONTAL,
                }
            };
        }
    };
    // `oz`: the tile waits for the SKU and its listing.
    let (Some(sku), Some(listing)) = (
        stores.gift_sku(&gift.sku_id),
        stores.gift_store_listing(&gift.sku_id),
    ) else {
        return Card::Loading {
            horizontal: width > HORIZONTAL,
        };
    };
    let horizontal = width > HORIZONTAL;
    let thumbnail = || {
        listing
            .thumbnail
            .as_ref()
            .map_or(Art::None, |_| Art::Thumbnail {
                key: format!("{THUMB_KEY}{}", sku.id),
            })
    };
    if gift.promotion {
        return Card::Tile(Box::new(Tile {
            horizontal,
            title: Some(dc_intl::t("X4p5uH").to_owned()),
            tagline: text(dc_intl::t("VIuwD7")),
            button: Some(Button {
                text: dc_intl::t("71nuwc").to_owned(),
                disabled: false,
            }),
            metadata: Vec::new(),
            art: thumbnail(),
            collectibles: false,
            promotion: true,
            subscription: false,
        }));
    }
    let plan = gift.subscription_plan.as_ref().or_else(|| {
        gift.subscription_plan_id
            .as_deref()
            .and_then(|plan| stores.gift_subscription_plan(plan))
    });
    let gifter = gift
        .user_id
        .as_deref()
        .and_then(|user| stores.gift_gifter(user));
    let game_item = sku.product_line == Some(SOCIAL_LAYER_GAME_ITEM);
    Card::Tile(Box::new(Tile {
        horizontal,
        title: title(gift, viewer),
        tagline: body(gift, sku, plan, gifter, viewer),
        button: Some(button(gift, viewer)),
        metadata: metadata(gift, chrono::Utc::now()),
        art: match gift.gift_style {
            Some(style) if !game_item => Art::Box {
                style,
                redeemed: gift.redeemed,
            },
            _ => thumbnail(),
        },
        collectibles: gift.is_collectibles(),
        promotion: false,
        subscription: gift.is_subscription(),
    }))
}

/// The tile's outer box (`tile_lS7BT7` + `embedHorizontal_mVuBLJ`/`embedVertical_mVuBLJ`): content-box 660 by 173 or
/// 240 by 300 inside `--space-4` (4px) of padding and a 1px `--border-subtle` border, `--radius-sm` (8px) corners,
/// `--background-surface-high` (`--background-surface-higher` under the pointer).
fn tile_box(id: SharedString, horizontal: bool, hover: bool) -> gpui::Stateful<gpui::Div> {
    let (width, height) = if horizontal {
        (660., 173.)
    } else {
        (240., 300.)
    };
    div()
        .id(id)
        .flex_none()
        .w(px(width + 10.))
        .h(px(height + 10.))
        .p(px(4.))
        .border_1()
        .border_color(color("--border-subtle"))
        .rounded(px(8.))
        .bg(color("--background-surface-high"))
        .when(hover, |this| {
            this.hover(|style| style.bg(color("--background-surface-higher")))
        })
        .overflow_hidden()
        .flex()
        .when(horizontal, |this| this.flex_row_reverse())
        .when(!horizontal, |this| this.flex_col())
}

impl Root {
    /// GiftCodeStore's `S` for the open channel's messages: each code not asked about yet is resolved.
    pub(super) fn request_gift_codes(&mut self) {
        let codes: Vec<String> = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .into_iter()
            .flatten()
            .flat_map(|message| {
                gift_codes::message_gift_codes(message.kind, &message.content, &message.embeds)
            })
            .collect();
        for code in codes {
            if self.stores.gift_code_is_resolving(&code) || self.stores.gift_code_is_resolved(&code)
            {
                self.request_gift_art(&code);
                continue;
            }
            self.stores
                .dispatch("GIFT_CODE_RESOLVE", &json!({"code": code.clone()}));
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{RESOLVE_KEY}{code}")),
                method: reqwest::Method::GET,
                route: message_resolve_route(&code),
                body: None,
            });
        }
    }

    /// A card's pictures: the box file of a styled gift, else the listing's thumbnail at 600 (`YE` of module 371794:
    /// `app-assets/{application}/store/{thumbnail}.webp?size=600`).
    fn request_gift_art(&mut self, code: &str) {
        let Some(gift) = self.stores.gift_code(code).cloned() else {
            return;
        };
        if let Some(style) = gift.gift_style {
            if style == STANDARD_BOX {
                let (state, url) = if gift.redeemed {
                    ("loop", STANDARD_BOX_LOOP)
                } else {
                    ("idle", STANDARD_BOX_IDLE)
                };
                self.request_image(format!("{GIFT_BOX_KEY}{style}:{state}"), url.to_owned());
            }
            return;
        }
        let Some(sku) = self.stores.gift_sku(&gift.sku_id).cloned() else {
            return;
        };
        if let Some((thumbnail, mime)) = self
            .stores
            .gift_store_listing(&sku.id)
            .and_then(|listing| listing.thumbnail.clone())
        {
            let extension = match mime.as_str() {
                "video/quicktime" | "video/mp4" => "mp4",
                "image/gif" => "gif",
                _ => "webp",
            };
            self.request_image(
                format!("{THUMB_KEY}{}", sku.id),
                format!(
                    "https://cdn.discordapp.com/app-assets/{}/store/{thumbnail}.{extension}?size=600",
                    sku.application_id
                ),
            );
        }
    }

    /// The answer of a gift's resolve. True when the key was one.
    pub(super) fn gift_code_answered(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(code) = key.strip_prefix(RESOLVE_KEY) else {
            return false;
        };
        if body["ok"].as_bool() == Some(true) {
            self.stores.dispatch(
                "GIFT_CODE_RESOLVE_SUCCESS",
                &json!({"giftCode": body["body"].clone()}),
            );
            self.request_gift_art(code);
        } else {
            self.stores.dispatch(
                "GIFT_CODE_RESOLVE_FAILURE",
                &json!({"code": code, "error": body["body"].clone()}),
            );
        }
        tracing::info!(
            "gift code resolved: {} {}",
            body["status"].as_u64().unwrap_or(0),
            if self.stores.gift_code(code).is_some() {
                "found"
            } else {
                "none"
            }
        );
        cx.notify();
        true
    }

    fn gift_viewer_name(&self) -> String {
        self.stores
            .current_user
            .as_ref()
            .map(|user| {
                user.global_name
                    .clone()
                    .unwrap_or_else(|| user.username.clone())
            })
            .unwrap_or_default()
    }

    /// `renderGiftCodes`: one card per code, each in a full-width container.
    pub(super) fn gift_cards(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let codes = gift_codes::message_gift_codes(message.kind, &message.content, &message.embeds);
        let me = self.stores.current_user.as_ref()?;
        if codes.is_empty() {
            return None;
        }
        let width = self.gift_embed_width.get();
        let name = self.gift_viewer_name();
        let custom = gift_codes::is_custom_gift(message.kind, &message.embeds)
            || message.kind == dc_state::message_kinds::CUSTOM_GIFT;
        let cards: Vec<AnyElement> = codes
            .iter()
            .map(|code| {
                let is_self = match self.stores.gift_code(code) {
                    Some(gift) => gift.user_id.as_deref() == Some(me.id.to_string().as_str()),
                    None => message.author.id == me.id,
                };
                let viewer = Viewer {
                    name: &name,
                    verified: self.stores.account.verified,
                    premium: self
                        .stores
                        .account
                        .premium_type
                        .is_some_and(|tier| tier > 0),
                    is_self,
                    custom_gift_message: custom,
                    content: &message.content,
                };
                let card = card(&self.stores, code, width, &viewer);
                self.gift_card_element(code, message, card, cx)
            })
            .collect();
        let measured = self.gift_embed_width.clone();
        Some(
            div()
                .w_full()
                .relative()
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(cards)
                .child(
                    gpui::canvas(
                        move |bounds, window, _| {
                            let width = f32::from(bounds.size.width);
                            if (measured.get() - width).abs() > 0.5 {
                                measured.set(width);
                                window.refresh();
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .into_any_element(),
        )
    }

    fn gift_card_element(
        &self,
        code: &str,
        message: &Message,
        card: Card,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = SharedString::from(format!("gift-embed-{}-{code}", message.id));
        match card {
            Card::Loading { horizontal } => tile_box(id, horizontal, true)
                .parity_with_border("gift-embed", 1.)
                .child(description(None, Vec::new(), None, Vec::new(), cx))
                .into_any_element(),
            Card::Invalid {
                horizontal,
                is_self,
                client_update,
            } => {
                let title = if is_self {
                    dc_intl::t("mT9B49")
                } else {
                    dc_intl::t("Vo5yHw")
                };
                let tagline = if client_update {
                    dc_intl::t("QXgO5w")
                } else if is_self {
                    dc_intl::t("eEM3dq")
                } else {
                    dc_intl::t("tB8S6u")
                };
                let button = if is_self {
                    // Gift Inventory: settings' gift page (`GIFT_PANEL`).
                    self.gift_card_button(
                        format!("gift-inventory-{code}"),
                        dc_intl::t("jcSP+g").to_owned(),
                        false,
                        |this, cx| this.open_settings_section("Gift Inventory", cx),
                        cx,
                    )
                } else {
                    self.gift_card_button(
                        format!("gift-accept-{code}"),
                        dc_intl::t("bUvv1f").to_owned(),
                        true,
                        |_, _| {},
                        cx,
                    )
                };
                tile_box(id, horizontal, true)
                    .parity_with_border("gift-embed", 1.)
                    .child(description(
                        Some(title.to_owned()),
                        text(tagline),
                        Some(button),
                        Vec::new(),
                        cx,
                    ))
                    .into_any_element()
            }
            Card::Tile(tile) => {
                let code_owned = code.to_owned();
                let channel = message.channel_id;
                let button = tile.button.clone().map(|button| {
                    let promotion = tile.promotion;
                    let code = code_owned.clone();
                    self.gift_card_button(
                        format!("gift-accept-{code_owned}"),
                        button.text,
                        button.disabled,
                        move |this, cx| {
                            if promotion {
                                // `handleClaimPromotion`: the promotion's page.
                                this.follow_link(
                                    &format!("https://discord.com/billing/promotions/{code}"),
                                    "",
                                    cx,
                                );
                            } else {
                                this.open_gift_accept(&code, channel, cx);
                            }
                        },
                        cx,
                    )
                });
                let tagline = self.gift_tagline(code, &tile.tagline, cx);
                let media = self.gift_media(&tile);
                let subscription = tile.subscription;
                tile_box(id, tile.horizontal, !tile.collectibles)
                    .parity_with_border("gift-embed", 1.)
                    // `handleEmbedClick`: a subscription gift's card opens the Nitro page.
                    .when(subscription && !tile.collectibles, |this| {
                        this.cursor_pointer().on_click(
                            cx.listener(|this, _, _, cx| this.open_settings_section("Nitro", cx)),
                        )
                    })
                    .child(media)
                    .child(description_runs(
                        tile.title.clone(),
                        tagline,
                        button,
                        tile.metadata.clone(),
                    ))
                    .into_any_element()
            }
        }
    }

    /// The art column: `mediaHorizontal_lS7BT7` (305px, centred) or `media_lS7BT7` on top.
    fn gift_media(&self, tile: &Tile) -> AnyElement {
        let horizontal = tile.horizontal;
        let media = div().relative().flex_none().when(horizontal, |this| {
            this.w(px(305.)).h_full().flex().items_center()
        });
        match &tile.art {
            Art::Box { style, redeemed } => {
                let key = format!(
                    "{GIFT_BOX_KEY}{style}:{}",
                    if *redeemed { "loop" } else { "idle" }
                );
                // `customGiftEmbedWrapper_c9v7J5` (128px high, or the full column) and `customGiftEmbedBox`: 234px
                // wide 36px from the left and 24px off the bottom, or 174px, 33px and 18px.
                let (width, left, bottom) = if horizontal {
                    (234., 36., 24.)
                } else {
                    (174., 33., 18.)
                };
                let height = width * BOX_ASPECT;
                let wrapper = div()
                    .relative()
                    .overflow_hidden()
                    .when(horizontal, |this| this.size_full())
                    .when(!horizontal, |this| this.w_full().h(px(128.)));
                let picture = self.images.get(&key).map(|image| {
                    img(image.clone())
                        .absolute()
                        .left(px(left))
                        .bottom(px(bottom))
                        .w(px(width))
                        .h(px(height))
                        .into_any_element()
                });
                media
                    .when(!horizontal, |this| this.w_full())
                    .child(wrapper.children(picture))
                    .into_any_element()
            }
            Art::Thumbnail { key } => {
                // `splashContainer_QpDdCx`: the column (or 56% of the width high), the picture covering it.
                let figure = div()
                    .relative()
                    .overflow_hidden()
                    .when(horizontal, |this| this.size_full())
                    .when(!horizontal, |this| this.w_full().h(px(240. * 0.56)));
                let picture = self.images.get(key).map(|image| {
                    img(image.clone())
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                });
                media
                    .when(!horizontal, |this| this.w_full())
                    .child(figure.children(picture))
                    .into_any_element()
            }
            Art::None => media.into_any_element(),
        }
    }

    /// The line, its `$b` runs bold and its `$link` runs links (`GQxl7v`'s opens the account page).
    fn gift_tagline(
        &self,
        code: &str,
        runs: &[(String, Option<Box<str>>)],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut text = String::new();
        let mut highlights = Vec::new();
        let mut links = Vec::new();
        for (part, tag) in runs {
            let start = text.len();
            text.push_str(part);
            match tag.as_deref() {
                Some("$b") => highlights.push((
                    start..text.len(),
                    HighlightStyle {
                        font_weight: Some(FontWeight::BOLD),
                        ..Default::default()
                    },
                )),
                Some("$link") => {
                    highlights.push((
                        start..text.len(),
                        HighlightStyle {
                            color: Some(color("--text-link").into()),
                            ..Default::default()
                        },
                    ));
                    links.push(start..text.len());
                }
                _ => {}
            }
        }
        let verify = runs == dc_intl::format_runs("GQxl7v", &[]).as_slice();
        let open = cx.listener(move |this, _: &usize, _, cx| {
            // `handleVerificationClick` opens the account page; the library links have nothing to open here.
            if verify {
                this.open_settings(cx);
            }
        });
        gpui::InteractiveText::new(
            SharedString::from(format!("gift-tagline-{code}")),
            gpui::StyledText::new(text).with_highlights(highlights),
        )
        .on_click(links, move |index, window, cx| open(&index, window, cx))
        .into_any_element()
    }

    /// An `eg.$` button, `variant: "primary"`, `size: "sm"` (the design system button of module 193433, as
    /// `modal::button` draws it): 32px high, 11px each side inside a 1px `--control-primary-border-default` border,
    /// 14/18 medium white on `--control-primary-background-default`. Disabled, the button is at `opacity: .5` as one
    /// layer: its fill and border at half alpha, its label blended half over the card's surface.
    fn gift_card_button(
        &self,
        id: String,
        label: String,
        disabled: bool,
        press: impl Fn(&mut Root, &mut Context<Root>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let background = color("--control-primary-background-default");
        let border = color("--control-primary-border-default");
        let text = color("--white");
        let (background, border, text) = if disabled {
            (
                super::modal::half(background),
                super::modal::half(border),
                super::modal::half_over(text, color("--background-surface-high")),
            )
        } else {
            (background, border, text)
        };
        div()
            .id(SharedString::from(id))
            .flex_none()
            .h(px(32.))
            .px(px(11.))
            .min_w(px(60.))
            .rounded(px(8.))
            .border_1()
            .border_color(border)
            .bg(background)
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(text)
            .when(disabled, |this| this.cursor_not_allowed())
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .hover(|style| {
                        style
                            .bg(color("--control-primary-background-hover"))
                            .border_color(color("--control-primary-border-hover"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        press(this, cx);
                        cx.notify();
                    }))
            })
            .child(label)
            .into_any_element()
    }

    /// `handleAccept`: the accept dialog (module 499454 opening module 935263), with the card's channel.
    pub(super) fn open_gift_accept(
        &mut self,
        code: &str,
        channel: dc_state::Id,
        cx: &mut Context<Self>,
    ) {
        let Some(gift) = self.stores.gift_code(code) else {
            return;
        };
        let sku_name = self
            .stores
            .gift_sku(&gift.sku_id)
            .map(|sku| sku.name.clone())
            .unwrap_or_default();
        let sender = gift
            .user_id
            .as_deref()
            .and_then(|user| {
                user.parse::<dc_state::Id>()
                    .ok()
                    .and_then(|id| self.stores.users.get(&id))
                    .map(|user| user.display_name().to_owned())
                    .or_else(|| self.stores.gift_gifter(user).map(|g| g.username.clone()))
            })
            .unwrap_or_default();
        let gift = Gift::from_store(gift, &sku_name, &sender);
        self.settings_extra.gift.modal = Some(Modal::new(gift, Some(channel)));
        cx.notify();
    }
}

/// `description_lS7BT7`: 12px of padding, `--text-strong`; the title (18px semibold, 1.2), the line (15px, 1.3,
/// `--text-default`, 8px below, taking the rest), the actions 12px below (`tileActions_03TL0y`): the button on the
/// left, the metadata (12px `--primary-400`) stacked at the bottom right.
fn description_runs(
    title: Option<String>,
    tagline: AnyElement,
    button: Option<AnyElement>,
    metadata: Vec<String>,
) -> AnyElement {
    div()
        .relative()
        .flex_1()
        .min_w(px(0.))
        .p(px(12.))
        .flex()
        .flex_col()
        .text_color(color("--text-strong"))
        .child(
            div()
                .flex_none()
                .text_size(px(18.))
                .line_height(px(18. * 1.2))
                .font_weight(FontWeight(600.))
                .children(title),
        )
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .mt(px(8.))
                .overflow_hidden()
                .text_size(px(15.))
                .line_height(px(15. * 1.3))
                .text_color(color("--text-default"))
                .child(tagline),
        )
        .child(
            div()
                .flex_none()
                .mt(px(12.))
                .flex()
                .items_center()
                .font_weight(FontWeight(500.))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .justify_between()
                        .child(div().flex().children(button))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_end()
                                .justify_end()
                                .text_size(px(12.))
                                .text_color(color("--primary-400"))
                                .children(metadata.into_iter().map(|line| div().child(line))),
                        ),
                ),
        )
        .into_any_element()
}

fn description(
    title: Option<String>,
    tagline: Vec<(String, Option<Box<str>>)>,
    button: Option<AnyElement>,
    metadata: Vec<String>,
    _cx: &mut Context<Root>,
) -> AnyElement {
    let line: String = tagline.into_iter().map(|(text, _)| text).collect();
    description_runs(
        title,
        div().child(line).into_any_element(),
        button,
        metadata,
    )
}

#[cfg(feature = "capture")]
impl Root {
    /// `gift-embed <width>`: the measured width the cards lay out at (a capture of the narrow layout);
    /// `gift-embed state <code>`: what the store holds for a code.
    pub(crate) fn script_gift_embed(&mut self, args: &[&str], cx: &mut Context<Self>) -> String {
        match args {
            ["state", code] => format!(
                "gift-embed: {code} resolving={} resolved={} accepting={} error={:?} gift={:?} width={}",
                self.stores.gift_code_is_resolving(code),
                self.stores.gift_code_is_resolved(code),
                self.stores.gift_code_is_accepting(code),
                self.stores.gift_code_error(code).map(|e| e["code"].clone()),
                self.stores.gift_code(code).map(|gift| (
                    gift.uses,
                    gift.max_uses,
                    gift.redeemed,
                    gift.revoked,
                    gift.gift_style
                )),
                self.gift_embed_width.get()
            ),
            ["accept", code] => {
                let channel = self.channel.unwrap_or_default();
                self.open_gift_accept(code, channel, cx);
                format!(
                    "gift-embed: accept {code}, modal {:?}",
                    self.settings_extra.gift.modal.as_ref().map(|m| m.step())
                )
            }
            ["dispatch", name, json @ ..] => {
                let text = json.join(" ");
                match serde_json::from_str::<Value>(&text) {
                    Ok(data) => {
                        self.stores.dispatch(name, &data);
                        if let Some(code) = data["giftCode"]["code"].as_str() {
                            self.request_gift_art(code);
                        }
                        cx.notify();
                        format!("gift-embed: {name} dispatched")
                    }
                    Err(error) => format!("gift-embed: not json: {error}"),
                }
            }
            _ => "gift-embed: state <code> | accept <code> | dispatch <ACTION> <json>".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stores_with(gift: Value) -> dc_state::Stores {
        let mut stores = dc_state::Stores::default();
        stores.dispatch("GIFT_CODE_RESOLVE_SUCCESS", &json!({"giftCode": gift}));
        stores
    }

    /// The gift of the owner's capture (30.png): a Shop gift with the Standard Box, sent by the viewer, claimed.
    fn shop_gift() -> Value {
        json!({
            "code": "AbCdEfGhJkMnPqRs",
            "sku_id": "1144058522808614923",
            "application_id": gift_codes::COLLECTIBLES_APPLICATION_ID,
            "uses": 1,
            "max_uses": 1,
            "redeemed": false,
            "gift_style": 4,
            "user": {"id": "10", "username": "me", "discriminator": "0"},
            "store_listing": {"id": "2", "sku": {"id": "1144058522808614923", "name": "Gift", "type": 3, "product_line": 7, "application_id": gift_codes::COLLECTIBLES_APPLICATION_ID}},
        })
    }

    fn viewer(is_self: bool) -> Viewer<'static> {
        Viewer {
            name: "Me",
            verified: true,
            premium: false,
            is_self,
            custom_gift_message: false,
            content: "",
        }
    }

    fn tile(card: Card) -> Tile {
        match card {
            Card::Tile(tile) => *tile,
            other => panic!("not a tile: {other:?}"),
        }
    }

    fn line(tile: &Tile) -> String {
        tile.tagline.iter().map(|(text, _)| text.as_str()).collect()
    }

    #[test]
    fn the_owner_capture_is_a_sent_claimed_styled_gift() {
        let stores = stores_with(shop_gift());
        let tile = tile(card(&stores, "AbCdEfGhJkMnPqRs", 700., &viewer(true)));
        assert!(tile.horizontal);
        assert_eq!(tile.title.as_deref(), Some("You sent a gift!"));
        assert_eq!(
            line(&tile),
            "Hmm, it seems someone already claimed this gift."
        );
        assert_eq!(
            tile.button,
            Some(Button {
                text: "Open Gift".to_owned(),
                disabled: true
            })
        );
        assert_eq!(
            tile.art,
            Art::Box {
                style: 4,
                redeemed: false
            }
        );
        assert!(tile.collectibles);
    }

    #[test]
    fn the_layout_turns_at_660() {
        let stores = stores_with(shop_gift());
        assert!(!tile(card(&stores, "AbCdEfGhJkMnPqRs", 660., &viewer(true))).horizontal);
        assert!(tile(card(&stores, "AbCdEfGhJkMnPqRs", 661., &viewer(true))).horizontal);
        // The invalid card and the resolving skeleton turn at 660 itself.
        let mut stores = dc_state::Stores::default();
        stores.dispatch("GIFT_CODE_RESOLVE", &json!({"code": "x"}));
        assert_eq!(
            card(&stores, "x", 660., &viewer(true)),
            Card::Loading { horizontal: true }
        );
        stores.dispatch(
            "GIFT_CODE_RESOLVE_FAILURE",
            &json!({"code": "x", "error": {"code": 10038}}),
        );
        assert_eq!(
            card(&stores, "x", 660., &viewer(false)),
            Card::Invalid {
                horizontal: true,
                is_self: false,
                client_update: false
            }
        );
    }

    #[test]
    fn titles_follow_subscription_and_sender() {
        let mut gift = shop_gift();
        gift["uses"] = json!(0);
        let stores = stores_with(gift.clone());
        let received = tile(card(&stores, "AbCdEfGhJkMnPqRs", 700., &viewer(false)));
        assert_eq!(received.title.as_deref(), Some("A wild gift appears!"));
        assert_eq!(
            received.button,
            Some(Button {
                text: "Open Gift".to_owned(),
                disabled: false
            })
        );
        // The sender sees the self-redeem line.
        let sent = tile(card(&stores, "AbCdEfGhJkMnPqRs", 700., &viewer(true)));
        assert_eq!(
            line(&sent),
            "If you want to claim this gift for yourself, go right ahead. We won't judge :)"
        );
        gift["subscription_plan"] = json!({"id": "511651880837840896", "interval": 1, "interval_count": 1, "sku_id": "521847234246082599"});
        gift["gift_style"] = Value::Null;
        gift["store_listing"]["sku"]["name"] = json!("Nitro");
        let stores = stores_with(gift);
        let nitro = tile(card(&stores, "AbCdEfGhJkMnPqRs", 700., &viewer(false)));
        assert_eq!(
            nitro.title.as_deref(),
            Some("You've been gifted a subscription!")
        );
        assert_eq!(line(&nitro), "me has gifted you Nitro for 1 month!");
        assert!(
            nitro
                .tagline
                .iter()
                .any(|(text, tag)| text == "me" && tag.as_deref() == Some("$b"))
        );
        assert_eq!(nitro.button.unwrap().text, "Accept");
        let sent = tile(card(&stores, "AbCdEfGhJkMnPqRs", 700., &viewer(true)));
        assert_eq!(sent.title.as_deref(), Some("You gifted a subscription!"));
    }

    #[test]
    fn redeemed_unverified_and_refused_states() {
        let mut gift = shop_gift();
        gift["uses"] = json!(0);
        let mut stores = stores_with(gift.clone());
        stores.dispatch(
            "GIFT_CODE_REDEEM_SUCCESS",
            &json!({"code": "AbCdEfGhJkMnPqRs"}),
        );
        let redeemed = tile(card(&stores, "AbCdEfGhJkMnPqRs", 700., &viewer(false)));
        assert_eq!(line(&redeemed), "Looks like you already claimed this gift.");
        assert_eq!(
            redeemed.button,
            Some(Button {
                text: "Claimed".to_owned(),
                disabled: true
            })
        );
        assert_eq!(
            redeemed.art,
            Art::Box {
                style: 4,
                redeemed: true
            }
        );
        let stores = stores_with(gift.clone());
        let unverified = Viewer {
            verified: false,
            ..viewer(false)
        };
        let tile_unverified = tile(card(&stores, "AbCdEfGhJkMnPqRs", 700., &unverified));
        assert_eq!(
            line(&tile_unverified),
            "Verify your account before claiming this gift."
        );
        assert!(tile_unverified.button.unwrap().disabled);
        // Not self-redeemable: the sender's button is off and the line thanks them.
        gift["flags"] = json!(4);
        let stores = stores_with(gift);
        let refused = tile(card(&stores, "AbCdEfGhJkMnPqRs", 700., &viewer(true)));
        assert!(refused.button.as_ref().unwrap().disabled);
        assert_eq!(line(&refused), "Thanks for giving the gift of Gift.");
    }

    #[test]
    fn copies_and_expiry_go_to_the_metadata() {
        let mut gift = shop_gift();
        gift["uses"] = json!(1);
        gift["max_uses"] = json!(3);
        gift["expires_at"] = json!("2026-11-28T12:00:00+00:00");
        let stores = stores_with(gift);
        let gift = stores
            .gift_code_at(
                "AbCdEfGhJkMnPqRs",
                chrono::DateTime::parse_from_rfc3339("2026-11-27T00:00:00+00:00")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            )
            .unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-27T01:30:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            metadata(gift, now),
            vec![
                "2 of 3 copies left".to_owned(),
                "Expires in 34 hours".to_owned()
            ]
        );
    }

    #[test]
    fn promotion_and_custom_gift_messages() {
        let mut gift = shop_gift();
        gift["promotion"] = json!({"id": "1"});
        let stores = stores_with(gift);
        let promo = tile(card(&stores, "AbCdEfGhJkMnPqRs", 700., &viewer(false)));
        assert!(promo.promotion);
        assert_eq!(promo.title.as_deref(), Some("You got a promotion code!"));
        assert_eq!(promo.button.unwrap().text, "Claim");
        // A custom gift message names its recipient and leaves the line empty for a Shop gift.
        let mut gift = shop_gift();
        gift["uses"] = json!(0);
        let stores = stores_with(gift);
        let custom = Viewer {
            custom_gift_message: true,
            content: "enjoy",
            ..viewer(false)
        };
        let tile_custom = tile(card(&stores, "AbCdEfGhJkMnPqRs", 700., &custom));
        assert_eq!(tile_custom.title.as_deref(), Some("Me, you have a gift!"));
        assert!(tile_custom.tagline.is_empty());
    }

    #[test]
    fn an_unknown_sku_keeps_the_skeleton_and_a_client_update_error_says_so() {
        let mut gift = shop_gift();
        gift["store_listing"] = Value::Null;
        let stores = stores_with(gift);
        assert_eq!(
            card(&stores, "AbCdEfGhJkMnPqRs", 700., &viewer(true)),
            Card::Loading { horizontal: true }
        );
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "GIFT_CODE_RESOLVE_FAILURE",
            &json!({"code": "y", "error": {"code": 50283}}),
        );
        assert!(matches!(
            card(&stores, "y", 300., &viewer(true)),
            Card::Invalid {
                horizontal: false,
                client_update: true,
                ..
            }
        ));
    }
}
