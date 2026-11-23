//! The Featured and Orbs Shop pages (the tab page `tK`/`tW` of module 253782): each block in its `blockContainer`
//! (30px above and below, 32px at the sides, none above and below the first), its content in the 1080px centred
//! section; the hero (module 353448), the featured tiles (module 196231), shelves that scroll sideways (module
//! 151869), the wide banner and the "Find your style" feed. Before the tab answers, the loading page of `tK` stands in:
//! the hero's empty header with four skeleton cards, two skeleton tiles and twelve skeleton cards in the feed.

use super::data::{self, Block, CARD, Filters, GAP, Hero, Sort};
use super::{Root, color, icon, text};
use crate::parity::Parity;
use gpui::{
    AnyElement, BoxShadow, Context, FontWeight, IntoElement, ObjectFit, ScrollHandle, SharedString,
    Window, deferred, div, hsla, img, linear_color_stop, linear_gradient, point, prelude::*, px,
};

/// `blockContainer_XBXDcd`: `padding: 30px 32px`.
const BLOCK_Y: f32 = 30.;
/// `featuredBlock`: 236 tall.
const TILE: f32 = 236.;
/// The edge fade of a sideways row (`--custom-edge-fade-width: var(--space-xl)`).
const EDGE_FADE: f32 = 24.;
/// How long an arrow's scroll glides (the scroller's `behavior: "smooth"`; Chromium's own curve, not in the bundle).
const GLIDE_MS: f64 = 300.;

impl Root {
    /// The width of the shop's scroller content: the window less what lies left of the scroller and its gutter.
    pub(super) fn shop_content_width(&self, window: &Window) -> f32 {
        let viewport = f32::from(window.viewport_size().width);
        let bounds = self.shop.scroll.bounds();
        // Left of the scroller: the server rail and the DM sidebar (375 in the 1280 capture).
        let left = if bounds.size.width > px(0.) {
            f32::from(bounds.left())
        } else {
            375.
        };
        (viewport - left - super::GUTTER).max(0.)
    }

    pub(super) fn shop_featured(
        &mut self,
        orbs: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let home = if orbs {
            self.shop.orbs.clone()
        } else {
            self.shop.home.clone()
        };
        let content = self.shop_content_width(window);
        let section = data::section_width(content);
        let key = if orbs { "shop:orbs" } else { "shop:home" };
        let mut page = div()
            .parity(if orbs { "shop-orbs" } else { "shop-featured" })
            .w_full()
            .flex()
            .flex_col();
        // `isFetchingShopHome || shopBlocks.length === 0`: the loading page.
        if !self.fetched.contains_key(key) || home.blocks.is_empty() {
            return page
                .child(self.shop_loading_page(orbs, section, window, cx))
                .into_any_element();
        }
        // Every product the page draws that no answer brought yet, asked for in one batch (`requestProducts` of
        // module 295586) before any card is laid out.
        let mut wanted: Vec<String> = Vec::new();
        for block in &home.blocks {
            match block {
                Block::Hero(hero) => wanted.extend(hero.skus.iter().cloned()),
                Block::Shelf { skus, .. } => wanted.extend(skus.iter().cloned()),
                _ => {}
            }
        }
        self.shop_need_products(&wanted);
        for (index, block) in home.blocks.iter().enumerate() {
            let element = match block {
                Block::Hero(hero) => self.shop_hero(orbs, hero, content, section, window, cx),
                Block::Featured(subs) => self.shop_marketing(subs, section, cx),
                Block::Shelf {
                    name,
                    category,
                    skus,
                    button,
                    background,
                    title_color,
                } => self.shop_shelf(
                    &Shelf {
                        name,
                        category,
                        skus,
                        button: button.as_deref(),
                        background: background.as_deref(),
                        title_color: title_color.as_deref(),
                    },
                    section,
                    window,
                    cx,
                ),
                Block::Feed {
                    popular,
                    recommended,
                } => Some(self.shop_feed(orbs, popular, recommended, section, window, cx)),
                Block::WideBanner {
                    title,
                    body,
                    banner,
                    logo,
                } => Some(self.shop_wide_banner(
                    title,
                    body,
                    banner.as_deref(),
                    logo.as_deref(),
                    section,
                )),
                Block::SocialLayerPromoBanner {
                    application_id,
                    header_text,
                    subheader_text,
                    gradient_colors,
                    gradient_angle,
                    sku_ids,
                    end_time,
                    cta_type,
                    logo_url,
                    terms_url,
                    background_image_url,
                } => self.shop_social_layer_promo_banner(
                    application_id,
                    header_text,
                    subheader_text,
                    gradient_colors,
                    *gradient_angle,
                    sku_ids,
                    end_time.as_deref(),
                    *cta_type,
                    logo_url.as_deref(),
                    terms_url.as_deref(),
                    background_image_url.as_deref(),
                    section,
                    cx,
                ),
            };
            if let Some(element) = element {
                page = page.child(block_container(index == 0, element));
            }
        }
        if self.shop.feed_visible >= super::FEED_MAX {
            page = page.child(self.shop_end_of_feed(cx));
        }
        self.shop_grow_feed(window);
        page.into_any_element()
    }

    /// `numVisibleItems`: twelve more feed cards each time the page is scrolled to within 200px of its end, up to 36.
    fn shop_grow_feed(&mut self, window: &mut Window) {
        let handle = &self.shop.scroll;
        let max = handle.max_offset().y;
        if max <= px(0.) || self.shop.feed_visible >= super::FEED_MAX {
            return;
        }
        if max + handle.offset().y <= px(200.) {
            let next = (self.shop.feed_visible + super::FEED_FIRST).min(super::FEED_MAX);
            if next != self.shop.feed_visible {
                self.shop.feed_visible = next;
                if let Some(sort @ (Sort::Recent | Sort::PriceLow)) = self.shop.feed_sort {
                    let limit = (self.shop.feed_visible + 8) as u32;
                    let route = data::search_route(&Filters::default(), sort, 0, limit);
                    let key = format!("shop:feed:{sort:?}:{limit}");
                    self.shop_request(&key, route);
                    if let Some(body) = self.fetched.get(&key).cloned() {
                        let (ids, _) = data::parse_search(&body);
                        self.shop.feed = ids;
                    }
                }
                window.request_animation_frame();
            }
        }
    }

    /// Collectibles Shop Game Promotion Banner (`type == 9`, module 504262 `"In Game Items"`):
    /// fetches the application's storefront (`GET /partner-sdk/applications/{app}/application-storefront`),
    /// renders the promo banner with gradient/background, 32px logo, header/subheader, Game Shop (`apFNLU`)
    /// or Get Nitro (`pj0XBN`) CTA, optional countdown pill, `"Terms Apply."` (`mYS7uY`), 1/2/3+ SKU cards
    /// (`aria_label("kocF+6")`), and `"© 2026 MARVEL"` legal footer for Marvel Rivals (`1346069614634864772`).
    #[allow(clippy::too_many_arguments)]
    fn shop_social_layer_promo_banner(
        &mut self,
        application_id: &str,
        header_text: &str,
        subheader_text: &str,
        gradient_colors: &[String],
        gradient_angle: Option<f32>,
        sku_ids: &[String],
        end_time: Option<&str>,
        cta_type: i64,
        logo_url: Option<&str>,
        terms_url: Option<&str>,
        background_image_url: Option<&str>,
        section: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !application_id.is_empty() {
            let key = super::game_shop::storefront_key(application_id);
            let route = super::game_shop::storefront_route(application_id);
            self.shop_request(&key, route);
        }
        let app_id = application_id.to_owned();
        let storefront = self.shop.storefronts.get(application_id).cloned();
        let bg_img = background_image_url.and_then(|u| self.shop_img(u));
        let logo_img = logo_url.and_then(|u| self.shop_img(u));
        let parsed_grad = (gradient_colors.len() >= 2)
            .then(|| {
                let parse_hex = |s: &str| -> Option<gpui::Rgba> {
                    let h = s.trim().trim_start_matches('#');
                    let v = u32::from_str_radix(h, 16).ok()?;
                    Some(if h.len() == 6 {
                        gpui::rgba((v << 8) | 0xff)
                    } else {
                        gpui::rgba(v)
                    })
                };
                let first = parse_hex(&gradient_colors[0])?;
                let last = parse_hex(gradient_colors.last()?)?;
                Some(linear_gradient(
                    gradient_angle.unwrap_or(135.),
                    linear_color_stop(first, 0.),
                    linear_color_stop(last, 1.),
                ))
            })
            .flatten();
        let countdown = end_time.and_then(|ts| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs() as i64;
            let days = data::days_left(ts, now)?;
            Some(if days > 1 {
                dc_intl::t!("Io7ozn", days = days).to_string()
            } else {
                dc_intl::tr("Last day to buy").to_owned()
            })
        });
        let cta_label = if cta_type == 1 {
            dc_intl::t("pj0XBN")
        } else {
            dc_intl::t("apFNLU")
        };
        let terms_href = terms_url.map(str::to_owned);
        let cards: Vec<AnyElement> = sku_ids
            .iter()
            .take(4)
            .enumerate()
            .map(|(i, sku)| {
                let listing = storefront.as_ref().and_then(|sf| sf.listings.get(sku));
                let title = listing
                    .map(|l| l.name.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| dc_intl::t("kocF+6").to_owned());
                let price = listing.and_then(|l| l.price.clone());
                let art = listing
                    .and_then(|l| l.card_image.as_deref())
                    .map(|asset| super::game_shop::asset_url(application_id, asset, 512))
                    .and_then(|u| self.shop_img(&u));
                let open_app = app_id.clone();
                div()
                    .id(SharedString::from(format!("shop-sl-promo-sku-{i}-{sku}")))
                    .flex_1()
                    .min_w_0()
                    .h(px(164.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .hover(|s| s.border_color(color("--border-normal")))
                    .bg(color("--background-base-lower"))
                    .overflow_hidden()
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !open_app.is_empty() {
                            this.shop_open_game_shop(&open_app, 0, cx);
                        }
                    }))
                    .child(
                        div()
                            .flex_1()
                            .w_full()
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .child(match art {
                                Some(image) => img(image)
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .into_any_element(),
                                None => {
                                    icon("storefront_store", 28., "--icon-muted").into_any_element()
                                }
                            }),
                    )
                    .child(
                        div()
                            .px(px(10.))
                            .py(px(8.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(6.))
                            .child(
                                text(13., 18., 600., "--text-strong")
                                    .truncate()
                                    .child(title),
                            )
                            .when_some(price, |row, p| {
                                row.child(text(12., 16., 600., "--text-muted").child(p))
                            }),
                    )
                    .into_any_element()
            })
            .collect();

        let is_marvel = application_id == "1346069614634864772";
        let cta_app = app_id.clone();
        let banner = div()
            .id(SharedString::from(format!(
                "shop-sl-promo-{application_id}"
            )))
            .relative()
            .w(px(section))
            .rounded(px(16.))
            .border_2()
            .border_color(color("--border-normal"))
            .bg(color("--background-base-lowest"))
            .overflow_hidden()
            .when_some(parsed_grad, |d, grad| d.bg(grad))
            .when_some(bg_img, |d, image| {
                d.child(
                    img(image)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .opacity(0.35),
                )
            })
            .child(
                div()
                    .relative()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(16.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .when_some(logo_img, |col, image| {
                                        col.child(
                                            img(image).h(px(32.)).object_fit(ObjectFit::Contain),
                                        )
                                    })
                                    .child(
                                        text(24., 30., 700., "--text-strong")
                                            .child(header_text.to_owned()),
                                    )
                                    .when(!subheader_text.is_empty(), |col| {
                                        col.child(
                                            text(14., 20., 400., "--text-default")
                                                .child(subheader_text.to_owned()),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .when_some(countdown, |row, pill| {
                                        row.child(
                                            div()
                                                .px(px(10.))
                                                .py(px(5.))
                                                .rounded(px(999.))
                                                .bg(color("--background-surface-high"))
                                                .border_1()
                                                .border_color(color("--border-subtle"))
                                                .flex()
                                                .items_center()
                                                .gap(px(5.))
                                                .child(icon("webhook_clock", 13., "--text-strong"))
                                                .child(
                                                    text(12., 16., 600., "--text-strong")
                                                        .child(pill),
                                                ),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "shop-sl-promo-cta-{application_id}"
                                            )))
                                            .h(px(38.))
                                            .px(px(16.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-primary-background-hover"))
                                            })
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(text(14., 18., 600., "--white").child(cta_label))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if cta_type == 1 {
                                                    this.open_purchase(false, cx);
                                                } else if !cta_app.is_empty() {
                                                    this.shop_open_game_shop(&cta_app, 0, cx);
                                                }
                                            })),
                                    ),
                            ),
                    )
                    .when(!cards.is_empty(), |col| {
                        col.child(
                            div()
                                .id(SharedString::from(format!(
                                    "shop-sl-promo-grid-{application_id}"
                                )))
                                .aria_label(dc_intl::t("kocF+6"))
                                .flex()
                                .gap(px(12.))
                                .children(cards),
                        )
                    })
                    .when(terms_href.is_some() || is_marvel, |col| {
                        col.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(div().children(terms_href.map(|href| {
                                    div()
                                        .id(SharedString::from(format!(
                                            "shop-sl-promo-terms-{application_id}"
                                        )))
                                        .cursor_pointer()
                                        .text_size(px(12.))
                                        .text_color(color("--text-link"))
                                        .hover(|s| s.underline())
                                        .child(dc_intl::t("mYS7uY"))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.open_url(&href);
                                        }))
                                })))
                                .when(is_marvel, |row| {
                                    row.child(
                                        text(11., 14., 500., "--text-muted")
                                            .child("\u{00a9} 2026 MARVEL"),
                                    )
                                }),
                        )
                    }),
            );
        Some(centered(banner.into_any_element()))
    }

    /// The loading page of `tK`: the hero with no art and four skeleton cards, the two skeleton tiles of the featured
    /// block, and the feed's header over twelve skeleton cards.
    fn shop_loading_page(
        &mut self,
        orbs: bool,
        section: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let vw = f32::from(window.viewport_size().width);
        let header = (0.18 * vw).max(130.);
        let cards: Vec<AnyElement> = (0..4)
            .map(|index| skeleton_card(index, Some(CARD), window))
            .collect();
        let hero = div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .w(px(section))
                    .pb(px(24.))
                    .child(div().h(px(header + 3.)))
                    .child(
                        div()
                            .w_full()
                            .h(px(CARD))
                            .overflow_hidden()
                            .flex()
                            .gap(px(GAP))
                            .children(cards),
                    ),
            );
        let tile_width = (section - GAP) / 2.;
        let tiles = div()
            .w(px(section))
            .flex()
            .gap(px(GAP))
            .child(skeleton_tile(tile_width, 0, window))
            .child(skeleton_tile(tile_width, 1, window));
        let feed = self.shop_feed(orbs, &[], &[], section, window, cx);
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(block_container(true, hero.into_any_element()))
            .child(block_container(false, centered(tiles.into_any_element())))
            .child(block_container(false, feed))
            .into_any_element()
    }

    /// The hero block (`heroBlock_XBXDcd`): the banner behind the whole block, the header with the logo (or the
    /// title) and the button, then the hero's cards: a sideways row on Featured, the feed grid on the Orbs Shop.
    fn shop_hero(
        &mut self,
        orbs: bool,
        hero: &Hero,
        content: f32,
        section: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let vw = f32::from(window.viewport_size().width);
        // `banner_XBXDcd` is 350 tall, or 24vw (200 at least) when responsive; `heroHeaderContainer_XBXDcd` 255, or
        // 18vw (130 at least), below a 3px padding.
        let (banner_height, header_height) = if hero.responsive {
            ((0.24 * vw).max(200.), (0.18 * vw).max(130.))
        } else {
            (350., 255.)
        };
        // The banner spans the block container, its 32px side padding included.
        let banner_width = content;
        // The Orbs page shows its gradient alone: the banner there is an animation with no still in the capture.
        let art = if orbs {
            None
        } else {
            self.shop_cover_at(
                hero.banner.as_deref(),
                banner_width,
                banner_height,
                (0.5, 0.),
            )
        };
        let logo_url = hero.logo.clone().or_else(|| {
            self.shop
                .home
                .categories
                .iter()
                .find(|entry| entry.sku_id == hero.category)
                .and_then(|entry| entry.hero_logo_url.clone())
        });
        // `heroHeaderLogo_XBXDcd`: at most 90 tall (or the configured height) and 540 wide, its own proportions.
        let logo = logo_url.and_then(|url| {
            let (aspect, image) = self.shop_img_aspect(&url)?;
            let max_height = hero.logo_max_height.unwrap_or(90.);
            let height = max_height.min(540. / aspect);
            Some(
                img(image)
                    .w(px(height * aspect))
                    .h(px(height))
                    .mb(px(12.))
                    .object_fit(ObjectFit::Contain),
            )
        });
        let gradient = hero.background_style.as_deref().and_then(parse_gradient);
        let page_bg = color("--background-base-lower");
        // `mask-image: linear-gradient(180deg, #000 40%, rgba(0,0,0,.8) 60%, rgba(0,0,0,.4) 80%, transparent)`: drawn
        // as the page colour laid over the art, in the mask's three steps.
        let fade = |from: f32, to: f32, start: f32, end: f32| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(banner_height * start))
                .h(px(banner_height * (end - start)))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(gpui::Rgba { a: from, ..page_bg }, 0.),
                    linear_color_stop(gpui::Rgba { a: to, ..page_bg }, 1.),
                ))
        };
        let banner_box = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(banner_height))
            .overflow_hidden()
            .children(gradient.map(|(top, bottom)| {
                div().absolute().inset_0().bg(linear_gradient(
                    180.,
                    linear_color_stop(top, 0.),
                    linear_color_stop(bottom, 1.),
                ))
            }))
            .children(art)
            .child(fade(0., 0.2, 0.4, 0.6))
            .child(fade(0.2, 0.6, 0.6, 0.8))
            .child(fade(0.6, 1., 0.8, 1.));
        let category_id = hero.category.clone();
        let button_label = if orbs {
            dc_intl::tr("Go to Quest Home")
        } else {
            dc_intl::tr("Shop the Collection")
        };
        let button = overlay_button("shop-hero-button", button_label).on_click(cx.listener(
            move |this, _, _, cx| {
                if orbs {
                    this.open_quests(cx);
                } else {
                    this.shop_open_collection(&category_id, cx);
                }
            },
        ));
        let now = now_secs();
        let limited = hero
            .unpublished_at
            .as_deref()
            .and_then(|at| data::days_left(at, now))
            .is_some();
        let mut left = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(16.));
        if limited {
            left = left.child(pill(dc_intl::tr("Limited time")));
        }
        let mut names = div().flex().flex_col().items_start().children(logo);
        if let Some(title) = &hero.title {
            names = names.child(if orbs {
                text(36., 52., 600., "--text-strong")
                    .mb(px(10.))
                    .child(title.clone())
            } else {
                text(40., 48., 600., "--text-strong").child(title.clone())
            });
        }
        if !hero.summary.is_empty() {
            names = names.child(
                text(
                    16.,
                    20.,
                    400.,
                    if orbs { "--text-subtle" } else { "--white" },
                )
                .when_some(
                    hero.banner_text_color.as_deref().and_then(parse_hex),
                    |summary, tint| summary.text_color(tint),
                )
                .child(hero.summary.clone()),
            );
        }
        left = left.child(names);
        let header = div()
            .parity("shop-hero-header")
            .w_full()
            .h(px(header_height + 3.))
            .pt(px(3.))
            .flex()
            .items_center()
            .justify_between()
            .child(left)
            .child(div().flex().justify_end().child(button));
        // Module 60140 (`HP8LNG` "Featured products"): filter out SKUs without a category / absent,
        // showing 4 skeleton cards while resolving.
        let _featured_label = dc_intl::t("HP8LNG");
        let valid_skus: Vec<String> = hero
            .skus
            .iter()
            .filter(|sku| {
                !self.shop.absent.contains(*sku)
                    && self
                        .shop_product(sku)
                        .is_none_or(|p| !p.category_sku_id.is_empty())
            })
            .cloned()
            .collect();
        let cards: AnyElement = if valid_skus.is_empty() && !hero.skus.is_empty() {
            let skeletons: Vec<AnyElement> = (0..4)
                .map(|index| skeleton_card(index, Some(CARD), window))
                .collect();
            div()
                .w_full()
                .h(px(CARD))
                .overflow_hidden()
                .flex()
                .gap(px(GAP))
                .children(skeletons)
                .into_any_element()
        } else if orbs {
            self.shop_feed_grid(&valid_skus, section, false, window, cx)
        } else {
            self.shop_shelf_row("hero", &valid_skus, section, false, window, cx)
        };
        Some(
            div()
                .parity("shop-hero")
                .relative()
                // As wide as the block container, its 32px padding included: the banner spans it.
                .w(px(content))
                .flex()
                .flex_col()
                .items_center()
                .child(banner_box)
                .child(
                    div()
                        .relative()
                        .w(px(section))
                        .pb(px(24.))
                        .child(header)
                        .child(cards),
                )
                .into_any_element(),
        )
    }

    /// A shelf block (`shelfBlock_dg2ugo`): the heading with its secondary "Shop All" button, 24px above a sideways
    /// row; with a background picture, the whole shelf sits in a rounded box with 24px padding.
    fn shop_shelf(
        &mut self,
        shelf: &Shelf,
        section: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Shelf {
            name,
            category,
            skus,
            button,
            background,
            title_color,
        } = *shelf;
        // `h.filter(e => getCategoryForProduct(e.skuId) != null)`, and nothing at all for an empty shelf.
        let shown: Vec<String> = skus
            .iter()
            .filter(|sku| !self.shop.absent.contains(*sku))
            .cloned()
            .collect();
        if shown.is_empty() {
            return None;
        }
        let with_background = background.is_some();
        let row_width = if with_background {
            section - 48.
        } else {
            section
        };
        let category = category.to_owned();
        let label = button
            .map(str::to_owned)
            .unwrap_or_else(|| dc_intl::t!("bc9RBE", category_name = name));
        let shop_all = button.is_some().then(|| {
            let target = category.clone();
            let base = if with_background {
                overlay_button(
                    SharedString::from(format!("shop-shelf-button-{category}")),
                    &label,
                )
            } else {
                secondary_button(
                    SharedString::from(format!("shop-shelf-button-{category}")),
                    &label,
                )
            };
            base.on_click(cx.listener(move |this, _, _, cx| this.shop_open_collection(&target, cx)))
        });
        let title = text(20., 24., 600., "--text-strong")
            .truncate()
            .when(with_background, |title| {
                title.text_color(
                    title_color
                        .and_then(parse_hex)
                        .unwrap_or(gpui::rgb(0xffffff)),
                )
            })
            .child(name.to_owned());
        let row = self.shop_shelf_row(name, &shown, row_width, with_background, window, cx);
        let art = background.and_then(|url| {
            self.shop_cover_at(Some(url), section, 24. + 40. + 24. + CARD + 24., (0.5, 0.5))
        });
        Some(
            div()
                .parity("shop-shelf")
                .w(px(section))
                .relative()
                .flex()
                .flex_col()
                .when(with_background, |shelf| {
                    shelf
                        .rounded(px(16.))
                        .overflow_hidden()
                        .bg(color("--background-base-lowest"))
                        .p(px(24.))
                        .gap(px(24.))
                })
                .children(art.map(|art| div().absolute().inset_0().child(art)))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_h(px(40.))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(px(if with_background { 16. } else { 12. }))
                        .when(!with_background, |header| header.mb(px(24.)))
                        .child(title)
                        .children(shop_all),
                )
                .child(div().relative().child(row))
                .into_any_element(),
        )
    }

    /// A sideways row of 246px cards (`container_s64Ydp`): the arrows show once the cards overflow, each fading out
    /// where there is nothing more to scroll to, and the row fades at the edges it can scroll past.
    pub(super) fn shop_shelf_row(
        &mut self,
        key: &str,
        skus: &[String],
        width: f32,
        with_background: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let skus: Vec<String> = skus
            .iter()
            .filter(|sku| !self.shop.absent.contains(*sku))
            .cloned()
            .collect();
        let handle = self.shop.shelves.entry(key.to_owned()).or_default().clone();
        self.shop_glide_step(key, &handle, window);
        // The row is as wide as its cards: the scroller measures its overflow from its direct child, so a row that only
        // filled the viewport would leave the arrows and the sideways wheel nothing to move.
        let count = skus.len();
        let row_width = count as f32 * CARD + count.saturating_sub(1) as f32 * GAP;
        let mut row = div().flex().gap(px(GAP)).flex_none().w(px(row_width));
        for sku in &skus {
            row = row.child(self.shop_card(sku, Some(CARD), window, cx));
        }
        let max = f32::from(handle.max_offset().x);
        let offset = -f32::from(handle.offset().x);
        // `showActions`: the content is wider than the row.
        let show_actions = row_width > width + 0.5;
        let can_left = offset > 0.5;
        let can_right = if max > 0. {
            offset < max - 1.
        } else {
            show_actions
        };
        let draw_fade =
            !with_background && super::super::settings::client_theme::active().is_none();
        let page_bg = super::super::settings::client_theme::solid("--background-base-lower");
        let fade = |at_left: bool| {
            let (from, to) = if at_left {
                (page_bg, gpui::Rgba { a: 0., ..page_bg })
            } else {
                (gpui::Rgba { a: 0., ..page_bg }, page_bg)
            };
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .w(px(EDGE_FADE))
                .when(at_left, |fade| fade.left_0())
                .when(!at_left, |fade| fade.right_0())
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(from, 0.),
                    linear_color_stop(to, 1.),
                ))
        };
        let arrow = |at_left: bool, enabled: bool, cx: &mut Context<Self>| {
            let key = key.to_owned();
            let handle = handle.clone();
            div()
                .id(SharedString::from(format!(
                    "shop-shelf-{}-{key}",
                    if at_left { "left" } else { "right" }
                )))
                .absolute()
                .top(px((CARD - 40.) / 2.))
                .when(at_left, |arrow| arrow.left(px(8.)))
                .when(!at_left, |arrow| arrow.right(px(8.)))
                .size(px(40.))
                .rounded(px(8.))
                .bg(gpui::rgba(0x0000008f))
                .flex()
                .items_center()
                .justify_center()
                .when(!enabled, |arrow| arrow.opacity(0.))
                .when(enabled, |arrow| {
                    arrow
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let scroll = -f32::from(handle.offset().x);
                            let max = f32::from(handle.max_offset().x);
                            if let Some(to) =
                                data::shelf_page_target(!at_left, scroll, width, count, max)
                            {
                                this.shop.shelf_glide.insert(
                                    key.clone(),
                                    (scroll, to, super::super::motion::now(), None),
                                );
                            }
                            cx.notify();
                        }))
                })
                .child(icon(
                    if at_left {
                        "shop_arrow_left"
                    } else {
                        "shop_arrow_right"
                    },
                    24.,
                    "--white",
                ))
        };
        let id = SharedString::from(format!("shop-shelf-{key}"));
        div()
            .relative()
            .w(px(width))
            .h(px(CARD))
            .child(
                div()
                    .id(id)
                    .size_full()
                    .overflow_x_scroll()
                    // A vertical wheel scrolls the page, not the row (only a sideways wheel or trackpad moves it, as
                    // in Chromium).
                    .restrict_scroll_to_axis()
                    .track_scroll(&handle)
                    .child(row),
            )
            .when(draw_fade && can_left, |this| this.child(fade(true)))
            .when(draw_fade && can_right && show_actions, |this| {
                this.child(fade(false))
            })
            .when(show_actions, |this| {
                this.child(arrow(true, can_left, cx))
                    .child(arrow(false, can_right, cx))
            })
            .into_any_element()
    }

    /// Moves a shelf along its glide for this frame and asks for the next one until it arrives.
    fn shop_glide_step(&mut self, key: &str, handle: &ScrollHandle, window: &mut Window) {
        let Some((from, to, start, written)) = self.shop.shelf_glide.get(key).copied() else {
            return;
        };
        if let Some(last) = written
            && (-f32::from(handle.offset().x) - last).abs() > 1.
        {
            self.shop.shelf_glide.remove(key);
            return;
        }
        let now = super::super::motion::now();
        let progress = if super::super::motion::reduced() {
            1.
        } else {
            ((now - start) / GLIDE_MS).clamp(0., 1.) as f32
        };
        // Ease in and out.
        let eased = if progress < 0.5 {
            2. * progress * progress
        } else {
            1. - (-2. * progress + 2.).powi(2) / 2.
        };
        let at = from + (to - from) * eased;
        handle.set_offset(point(px(-at), handle.offset().y));
        if progress >= 1. {
            self.shop.shelf_glide.remove(key);
        } else {
            if let Some(entry) = self.shop.shelf_glide.get_mut(key) {
                entry.3 = Some(at);
            }
            window.request_animation_frame();
        }
    }

    /// The featured block (`col2_XBXDcd`): two tiles side by side, each 236 tall with its art covering it from the
    /// left (`background-position: 0px 50%`), the limited-time pill and the "Take me there" button. Tiles of any
    /// type but `CATEGORY` are not drawn.
    fn shop_marketing(
        &mut self,
        subs: &[data::SubBlock],
        section: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tiles: Vec<&data::SubBlock> = subs.iter().filter(|sub| sub.kind == 0).collect();
        if tiles.is_empty() {
            return None;
        }
        let width = (section - GAP) / 2.;
        let now = now_secs();
        let mut grid = div()
            .parity("shop-marketing")
            .w(px(section))
            .flex()
            .gap(px(GAP));
        for (index, sub) in tiles.into_iter().enumerate() {
            let art = self.shop_cover_at(sub.asset.as_deref(), width, TILE, (0., 0.5));
            let category = sub.category.clone();
            let button_category = sub.category.clone();
            let limited = sub
                .unpublished_at
                .as_deref()
                .and_then(|at| data::days_left(at, now))
                .is_some();
            let body = sub.body_text.clone().map(|body| {
                text(16., 20., 500., "--white")
                    .max_w(px(300.))
                    .when_some(
                        sub.text_color.as_deref().and_then(parse_hex),
                        |body, tint| body.text_color(tint),
                    )
                    .child(body)
            });
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("shop-marketing-{index}")))
                    .relative()
                    .w(px(width))
                    .h(px(TILE))
                    .rounded(px(16.))
                    .overflow_hidden()
                    .cursor_pointer()
                    .hover(|tile| {
                        tile.shadow(vec![BoxShadow {
                            color: hsla(0., 0., 0., 0.14),
                            offset: point(px(0.), px(8.)),
                            blur_radius: px(10.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shop_open_collection(&category, cx);
                    }))
                    .children(art)
                    .when(limited, |tile| {
                        tile.child(
                            div()
                                .absolute()
                                .top(px(32.))
                                .left(px(24.))
                                .child(pill(dc_intl::tr("Limited time"))),
                        )
                    })
                    .children(body.map(|body| {
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(24.))
                            .pt(px(54.))
                            .pb(px(72.))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .child(body)
                    }))
                    .child(
                        div().absolute().bottom(px(24.)).left(px(24.)).child(
                            overlay_button(
                                SharedString::from(format!("shop-marketing-button-{index}")),
                                dc_intl::tr("Take me there"),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.shop_open_collection(&button_category, cx);
                                },
                            )),
                        ),
                    ),
            );
        }
        Some(grid.into_any_element())
    }

    /// The wide banner (`wideBannerBackgroundImg_XBXDcd`): the art covering it from the left, the title and body over
    /// it.
    fn shop_wide_banner(
        &mut self,
        title: &str,
        body: &str,
        banner: Option<&str>,
        logo: Option<&str>,
        section: f32,
    ) -> AnyElement {
        let art = self.shop_cover_at(banner, section, 141., (0., 0.5));
        let logo = logo.and_then(|url| self.shop_img(url));
        div()
            .parity("shop-wide-banner")
            .relative()
            .w(px(section))
            .h(px(141.))
            .rounded(px(16.))
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            .children(art)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .px(px(32.))
                    .flex()
                    .items_center()
                    .gap(px(24.))
                    .children(
                        logo.map(|logo| img(logo).size(px(96.)).object_fit(ObjectFit::Contain)),
                    )
                    .child(
                        div()
                            .max_w(px(400.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(text(24., 32., 600., "--text-strong").child(title.to_owned()))
                            .child(text(14., 18., 500., "--text-muted").child(body.to_owned())),
                    ),
            )
            .into_any_element()
    }

    /// Cards in the feed's grid (`feed_XBXDcd`): as many 246px-or-wider columns as the section holds.
    fn shop_feed_grid(
        &mut self,
        skus: &[String],
        section: f32,
        hide_bundle_backgrounds: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (columns, column) = data::feed_columns(section);
        let mut grid = div()
            .w(px(section))
            .grid()
            .grid_cols(columns as u16)
            .gap(px(GAP));
        let shown: Vec<String> = skus
            .iter()
            .filter(|sku| *sku != data::NITRO_1_DAY && !self.shop.absent.contains(*sku))
            .cloned()
            .collect();
        for sku in &shown {
            grid = grid.child(self.shop_card_with(
                sku,
                Some(column),
                hide_bundle_backgrounds,
                window,
                cx,
            ));
        }
        grid.into_any_element()
    }

    /// "Find your style" (`popularPicksSection_XBXDcd`): the sort select, Shuffle and the feed grid.
    fn shop_feed(
        &mut self,
        orbs: bool,
        popular: &[String],
        recommended: &[String],
        section: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The block opens on "For You" when it carries recommendations (`showRecommendationOption` of module 253782).
        let has_recommended = !recommended.is_empty();
        let sorted = self.shop.feed_sort.unwrap_or(if has_recommended {
            Sort::Recommended
        } else {
            Sort::Popular
        });
        let count = self.shop.feed_visible.max(super::FEED_FIRST);
        let ordered: Vec<String> = match sorted {
            Sort::Recommended => recommended.to_vec(),
            Sort::Popular => popular.to_vec(),
            _ => self.shop.feed.clone(),
        };
        // `skuIds.slice(0, numVisibleItems)` after the cards that cannot show are left out.
        let mut seen = std::collections::HashSet::new();
        let skus: Vec<String> = ordered
            .into_iter()
            .filter(|sku| sku != data::NITRO_1_DAY && !self.shop.absent.contains(sku))
            .filter(|sku| seen.insert(sku.clone()))
            .take(count)
            .collect();
        self.shop_need_products(&skus);
        let waiting = popular.is_empty() && recommended.is_empty()
            || (!matches!(sorted, Sort::Popular | Sort::Recommended) && self.shop.feed.is_empty());
        let label = match sorted {
            Sort::Recommended => dc_intl::tr("For You"),
            Sort::Recent => dc_intl::tr("Recently Added"),
            Sort::PriceLow => dc_intl::tr("Price: Low to High"),
            _ => dc_intl::tr("Popular"),
        };
        let mut options = vec![
            ("Popular", Sort::Popular),
            ("Recently Added", Sort::Recent),
            ("Price: Low to High", Sort::PriceLow),
        ];
        if has_recommended {
            options.insert(0, ("For You", Sort::Recommended));
        }
        let select = self.shop_select(
            "shop-feed-sort",
            label,
            280.,
            self.shop.feed_menu,
            |this, open, cx| {
                this.shop.feed_menu = open;
                cx.notify();
            },
            &options,
            |this, sort, cx| {
                this.shop.feed_sort = Some(sort);
                this.shop.feed_menu = false;
                if !matches!(sort, Sort::Popular | Sort::Recommended) {
                    // `limit: numVisibleItems + 8` (module 253782).
                    let limit = (this.shop.feed_visible + 8) as u32;
                    let route = data::search_route(&Filters::default(), sort, 0, limit);
                    let key = format!("shop:feed:{sort:?}:{limit}");
                    this.shop.feed.clear();
                    this.shop_request(&key, route);
                    if let Some(body) = this.fetched.get(&key).cloned() {
                        let (ids, _) = data::parse_search(&body);
                        this.shop.feed = ids;
                    }
                }
                cx.notify();
            },
            cx,
        );
        let shuffle = secondary_button("shop-shuffle", dc_intl::tr("Shuffle!"))
            .px(px(16.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.shop.shuffle += 1;
                // `randomize: true`, `limit: numVisibleItems + 32`.
                let limit = this.shop.feed_visible + 32;
                let key = format!("shop:feed:shuffle{}", this.shop.shuffle);
                this.shop.feed_sort = Some(Sort::Relevance);
                this.shop.feed.clear();
                this.shop_request(
                    &key,
                    format!(
                        "/shop/search?randomize=true&offset=0&limit={limit}&published_collections_only=true"
                    ),
                );
                cx.notify();
            }));
        let title = if orbs {
            // `dFgeuZ` (module 253782); the capture's heading is 237px wide, as this string is.
            dc_intl::tr("Orbs-Worthy Popular Picks")
        } else {
            dc_intl::tr("Find your style")
        };
        let grid = if waiting {
            let (columns, column) = data::feed_columns(section);
            let mut grid = div()
                .w(px(section))
                .grid()
                .grid_cols(columns as u16)
                .gap(px(GAP));
            for index in 0..super::FEED_FIRST {
                grid = grid.child(skeleton_card(index, Some(column), window));
            }
            grid.into_any_element()
        } else {
            self.shop_feed_grid(&skus, section, true, window, cx)
        };
        div()
            .parity("shop-popular")
            .w(px(section))
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .mb(px(24.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(text(20., 24., 600., "--text-strong").child(title))
                            .when(has_recommended, |row| {
                                row.child(icon("shop_info", 18., "--interactive-text-default"))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        text(16., 20., 500., "--text-default")
                                            .child(dc_intl::tr("Sort by")),
                                    )
                                    .child(select),
                            )
                            .child(shuffle),
                    ),
            )
            .child(grid)
            .into_any_element()
    }

    /// `endOfFeed_XBXDcd`: "There's so much more" and the Shop All button, once the feed stopped growing.
    fn shop_end_of_feed(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .m(px(32.))
                    .min_w(px(320.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(24.))
                    .child(
                        text(16., 20., 600., "--text-strong")
                            .child(dc_intl::tr("There's so much more")),
                    )
                    .child(
                        div()
                            .id("shop-end-of-feed")
                            .w_full()
                            .h(px(40.))
                            .rounded(px(8.))
                            .bg(color("--background-brand"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|button| button.bg(color("--brand-560")))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.shop_set_tab(super::Tab::Browse, cx);
                            }))
                            .child(text(16., 20., 500., "--white").child(dc_intl::tr("Shop All"))),
                    ),
            )
            .into_any_element()
    }

    /// The select field of module 637330 style (`selectField_vWC4bk`): 40 tall, with the options in a popout under it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn shop_select(
        &mut self,
        id: &'static str,
        label: &str,
        width: f32,
        open: bool,
        toggle: fn(&mut Root, bool, &mut Context<Root>),
        options: &[(&'static str, Sort)],
        choose: fn(&mut Root, Sort, &mut Context<Root>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let trigger_bounds = self.tip_anchors.borrow().get(id).copied();
        let flip_up = trigger_bounds.is_some_and(|b| {
            let est_h = (options.len() as f32) * 36. + 20.;
            let below = self.viewport.1 - f32::from(b.bottom()) - 12.;
            below < est_h && f32::from(b.top()) - 12. > below
        });
        let menu = open.then(|| {
            let rows: Vec<AnyElement> = options
                .iter()
                .map(|(name, sort)| {
                    let sort = *sort;
                    div()
                        .id(SharedString::from(format!("{id}-{name}")))
                        .h(px(36.))
                        .px(px(8.))
                        .rounded(px(2.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .hover(|row| row.bg(color("--background-mod-subtle")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            choose(this, sort, cx);
                        }))
                        .child(text(14., 18., 500., "--text-strong").child(dc_intl::tr(name)))
                        .into_any_element()
                })
                .collect();
            deferred(
                div()
                    .id(SharedString::from(format!("{id}-menu")))
                    .absolute()
                    .when(!flip_up, |d| d.top(px(44.)))
                    .when(flip_up, |d| d.bottom(px(44.)))
                    .left_0()
                    .w(px(width))
                    .occlude()
                    .p(px(8.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-high"))
                    .border(px(1.))
                    .border_color(color("--border-subtle"))
                    .shadow(vec![super::super::shadow_high()])
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(
                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                            if trigger_bounds.is_none_or(|b| !b.contains(&event.position)) {
                                toggle(this, false, cx);
                            }
                        },
                    ))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .with_priority(2)
        });
        div()
            .id(id)
            .parity("shop-select")
            .relative()
            .child(self.anchor_probe(id))
            .flex_none()
            .w(px(width))
            .h(px(40.))
            .rounded(px(8.))
            .border(px(1.))
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .pl(px(11.))
            .pr(px(7.))
            .flex()
            .items_center()
            .justify_between()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| toggle(this, !open, cx)))
            .child(
                text(16., 20., 400., "--text-default")
                    .truncate()
                    .font_weight(FontWeight(400.))
                    .child(label.to_owned()),
            )
            .child(icon("shop_select_chevron", 24., "--text-default"))
            .children(menu)
            .into_any_element()
    }

    /// Opens a collection (a category's own page): its banner and every product.
    pub(super) fn shop_open_collection(&mut self, category: &str, cx: &mut Context<Self>) {
        self.shop.collection = Some(category.to_owned());
        self.shop_set_tab(super::Tab::Browse, cx);
    }
}

/// A shelf block's record.
struct Shelf<'a> {
    name: &'a str,
    category: &'a str,
    skus: &'a [String],
    button: Option<&'a str>,
    background: Option<&'a str>,
    title_color: Option<&'a str>,
}

/// `blockContainer_XBXDcd`: 30px above and below (none for the first block), 32px at the sides, relative so the
/// hero's banner can span it.
fn block_container(first: bool, child: AnyElement) -> AnyElement {
    div()
        .relative()
        .w_full()
        .px(px(data::BLOCK_SIDE))
        .when(!first, |block| block.py(px(BLOCK_Y)))
        .flex()
        .flex_col()
        .items_center()
        .child(child)
        .into_any_element()
}

fn centered(child: AnyElement) -> AnyElement {
    div()
        .w_full()
        .flex()
        .justify_center()
        .child(child)
        .into_any_element()
}

/// The `overlay-primary` button: white with black text, 40 tall.
fn overlay_button(id: impl Into<gpui::ElementId>, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(40.))
        .px(px(16.))
        .rounded(px(8.))
        .bg(color("--white"))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .child(text(16., 20., 500., "--black").child(label.to_owned()))
}

/// The `secondary` button: the subtle fill, 40 tall.
fn secondary_button(id: impl Into<gpui::ElementId>, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(40.))
        .px(px(16.))
        .rounded(px(8.))
        .bg(color("--background-mod-subtle"))
        .flex()
        .items_center()
        .cursor_pointer()
        .hover(|button| button.bg(color("--background-mod-strong")))
        .child(text(16., 20., 500., "--text-strong").child(label.to_owned()))
}

/// The inverted badge (`variant: "inverted"`): white with black capitals.
pub(super) fn pill(label: &str) -> AnyElement {
    div()
        .h(px(16.))
        .px(px(4.))
        .rounded(px(8.))
        .bg(color("--white"))
        .flex()
        .items_center()
        .child(text(12., 16., 600., "--black").child(label.to_uppercase()))
        .into_any_element()
}

/// The background of the skeletons' pulse (`placeholderPulse` / `pulse_kIZjPZ`): base-lower, base-low from 30% to
/// 70%, back to base-lower, over 2 s, alternating, each card of five 0.4 s later than the one before.
fn pulse(index: usize, window: &mut Window) -> gpui::Rgba {
    let lower = color("--background-base-lower");
    let low = color("--background-base-low");
    if super::super::motion::reduced() {
        return lower;
    }
    window.request_animation_frame();
    let now = super::super::motion::now();
    let delay = (index % 5) as f64 * 400.;
    let t = data::pulse_phase(now, delay);
    let mix = |a: f32, b: f32| a + (b - a) * t;
    gpui::Rgba {
        r: mix(lower.r, low.r),
        g: mix(lower.g, low.g),
        b: mix(lower.b, low.b),
        a: mix(lower.a, low.a),
    }
}

/// A skeleton product card (`productSkeletonCardContainer_kIZjPZ`, module 484469): the card's frame pulsing, with a
/// title bar of 60% and a line of 40% at its foot.
pub(super) fn skeleton_card(index: usize, width: Option<f32>, window: &mut Window) -> AnyElement {
    let bar = |share: f32| {
        div()
            .w(gpui::relative(share))
            .h(px(24.))
            .rounded(px(4.))
            .bg(color("--icon-muted"))
            .opacity(0.1)
    };
    div()
        .parity("shop-card-skeleton")
        .flex_none()
        .when_some(width, |card, width| card.w(px(width)))
        .when(width.is_none(), |card| card.w_full())
        .h(px(CARD))
        .rounded(px(8.))
        .border(px(2.))
        .border_color(color("--border-muted"))
        .bg(pulse(index, window))
        .p(px(16.))
        .flex()
        .flex_col()
        .justify_end()
        .gap(px(8.))
        .child(bar(0.6))
        .child(bar(0.4))
        .into_any_element()
}

/// A skeleton tile of the featured block (`featuredBlockSkeleton_XBXDcd`): the logo bar at half the width.
fn skeleton_tile(width: f32, index: usize, window: &mut Window) -> AnyElement {
    div()
        .w(px(width))
        .h(px(TILE))
        .rounded(px(16.))
        .border(px(2.))
        .border_color(color("--border-muted"))
        .bg(pulse(index, window))
        .p(px(16.))
        .flex()
        .items_center()
        .child(
            div()
                .w(gpui::relative(0.5))
                .h(px(48.))
                .rounded(px(4.))
                .bg(color("--icon-muted"))
                .opacity(0.1),
        )
        .into_any_element()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

/// `#rrggbb` as a colour.
fn parse_hex(text: &str) -> Option<gpui::Rgba> {
    let hex = text.trim().strip_prefix('#')?;
    let value = u32::from_str_radix(hex.get(0..6)?, 16).ok()?;
    Some(gpui::rgb(value))
}

/// `linear-gradient(180deg, #110101 0%, #BA2025 100%)` or `linear-gradient(180deg, rgba(39, 30, 173, 0.3), transparent)`
/// as its first and last colour.
fn parse_gradient(text: &str) -> Option<(gpui::Rgba, gpui::Rgba)> {
    let body = text.split_once('(')?.1;
    let mut colors = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let hash = rest.find('#');
        let func = rest.find("rgb");
        let clear = rest.find("transparent");
        let first = [hash, func, clear].into_iter().flatten().min();
        let Some(at) = first else { break };
        let tail = &rest[at..];
        if Some(at) == clear {
            colors.push(gpui::Rgba {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 0.,
            });
            rest = &tail["transparent".len()..];
        } else if Some(at) == hash {
            let value = u32::from_str_radix(tail.get(1..7)?, 16).ok()?;
            colors.push(gpui::rgba(value << 8 | 0xff));
            rest = &tail[7..];
        } else {
            let inner = tail.split_once('(')?.1;
            let (args, after) = inner.split_once(')')?;
            let numbers: Vec<f32> = args
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            let alpha = numbers.get(3).copied().unwrap_or(1.);
            colors.push(gpui::Rgba {
                r: numbers.first()? / 255.,
                g: numbers.get(1)? / 255.,
                b: numbers.get(2)? / 255.,
                a: alpha,
            });
            rest = after;
        }
    }
    Some((*colors.first()?, *colors.last()?))
}

#[cfg(test)]
mod tests {
    use super::{parse_gradient, parse_hex};

    #[test]
    fn gradients_give_their_two_stops() {
        let (top, bottom) =
            parse_gradient("linear-gradient(180deg, #110101 0%, #BA2025 100%)").unwrap();
        assert_eq!((top.r * 255.).round() as u32, 0x11);
        assert_eq!((bottom.r * 255.).round() as u32, 0xba);
        let (top, bottom) =
            parse_gradient("linear-gradient(180deg, rgba(39, 30, 173, 0.3), transparent)").unwrap();
        assert!((top.a - 0.3).abs() < 1e-6 && bottom.a == 0.);
        assert!(parse_gradient("none").is_none());
    }

    #[test]
    fn text_colours_read_as_hex() {
        assert_eq!((parse_hex("#ff0000").unwrap().r * 255.).round(), 255.);
        assert!(parse_hex("white").is_none());
    }
}
