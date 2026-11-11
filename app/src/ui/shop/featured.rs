//! The Featured and Orbs Shop pages (the tab page of module 253782): the hero block with its shelf of cards, the marketing
//! blocks, shelves that scroll sideways, the wide banner and the "Find your style" feed.

use super::data::{self, Block, Filters, Sort};
use super::{Root, color, icon, text};
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, ScrollHandle, SharedString, Window,
    deferred, div, img, linear_color_stop, linear_gradient, point, prelude::*, px,
};

/// The blocks sit 60px apart; the hero block is the banner (307) plus the 233px the cards hang from.
const BLOCK_GAP: f32 = 60.;
const BANNER: f32 = 307.;
const CARD: f32 = 246.;

impl Root {
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
        let mut page = div()
            .parity(if orbs { "shop-orbs" } else { "shop-featured" })
            .w_full()
            .px(px(32.))
            .pb(px(60.))
            .flex()
            .flex_col()
            .gap(px(BLOCK_GAP));
        let mut first = true;
        for block in &home.blocks {
            let element = match block {
                Block::Hero {
                    category,
                    name,
                    title,
                    summary,
                    skus,
                    banner,
                    background_style,
                } => self.shop_hero(
                    orbs,
                    category,
                    name,
                    title.as_deref(),
                    summary,
                    skus,
                    banner.as_deref(),
                    background_style.as_deref(),
                    window,
                    cx,
                ),
                Block::Featured(subs) => self.shop_marketing(subs, cx),
                Block::Shelf {
                    name,
                    category,
                    skus,
                    button,
                } => self.shop_shelf(name, category, skus, button.as_deref(), window, cx),
                Block::Feed {
                    popular,
                    recommended,
                } => self.shop_feed(orbs, popular, recommended, window, cx),
                Block::WideBanner {
                    title,
                    body,
                    banner,
                    logo,
                } => self.shop_wide_banner(title, body, banner.as_deref(), logo.as_deref()),
            };
            // The hero is bled to the page's edges: pull it out of the 32px padding and up under the header rule.
            page = page.child(if first {
                div().mx(px(-32.)).child(element).into_any_element()
            } else {
                element
            });
            first = false;
        }
        page.into_any_element()
    }

    /// The hero block (`heroBlock_XBXDcd`): the banner with the logo or title, its button, and the cards below.
    #[allow(clippy::too_many_arguments)]
    fn shop_hero(
        &mut self,
        orbs: bool,
        category: &str,
        name: &str,
        title: Option<&str>,
        summary: &str,
        skus: &[String],
        banner: Option<&str>,
        background_style: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The Orbs page shows its gradient alone: the banner there is an animation with no still in the capture.
        let art = if orbs { None } else { self.shop_cover(banner) };
        let logo_url = self
            .shop
            .home
            .categories
            .iter()
            .find(|entry| entry.sku_id == category)
            .and_then(|entry| entry.hero_logo_url.clone());
        let logo = logo_url.and_then(|url| self.shop_img(&url));
        let gradient = background_style.and_then(parse_gradient);
        let page_bg = color("--background-base-lower");
        let banner_box = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(BANNER))
            .overflow_hidden()
            .children(gradient.map(|(top, bottom)| {
                div().absolute().inset_0().bg(linear_gradient(
                    180.,
                    linear_color_stop(top, 0.),
                    linear_color_stop(bottom, 1.),
                ))
            }))
            .children(art.map(|art| div().absolute().inset_0().child(art)))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(150.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(gpui::Rgba { a: 0., ..page_bg }, 0.),
                        linear_color_stop(page_bg, 1.),
                    )),
            );
        let category_id = category.to_owned();
        let button_label = if orbs {
            "Go to Quest Home"
        } else {
            "Shop the Collection"
        };
        let button = div()
            .id("shop-hero-button")
            .parity("shop-hero-button")
            .absolute()
            .top(px(98.))
            .right(px(32.))
            .h(px(40.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(color("--white"))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if orbs {
                    this.open_quests(cx);
                } else {
                    this.shop_open_collection(&category_id, cx);
                }
            }))
            .child(text(16., 20., 500., "--black").child(button_label));
        let heading = if orbs {
            div()
                .absolute()
                .top(px(78.))
                .left(px(32.))
                .flex()
                .flex_col()
                .child(
                    text(36., 52., 600., "--text-strong").child(title.unwrap_or(name).to_owned()),
                )
                .child(
                    text(16., 20., 400., "--interactive-text-default")
                        .mt(px(8.))
                        .child(summary.to_owned()),
                )
                .into_any_element()
        } else {
            div()
                .absolute()
                .top(px(52.))
                .left(px(32.))
                .w(px(227.))
                .h(px(120.))
                .children(logo.map(|logo| img(logo).size_full().object_fit(ObjectFit::Contain)))
                .into_any_element()
        };
        let cards: AnyElement = if orbs {
            let mut grid = div().w_full().grid().grid_cols(3).gap(px(24.));
            for sku in skus.iter().filter(|sku| *sku != data::NITRO_1_DAY) {
                grid = grid.child(self.shop_card(sku, None, window, cx));
            }
            grid.into_any_element()
        } else {
            self.shop_shelf_row("hero", skus, window, cx)
        };
        div()
            .parity("shop-hero")
            .relative()
            .w_full()
            .child(banner_box)
            .child(heading)
            .child(button)
            .child(
                div()
                    .relative()
                    .pt(px(233.))
                    .px(px(32.))
                    .pb(px(if orbs { 0. } else { 4. }))
                    .child(cards),
            )
            .into_any_element()
    }

    /// A shelf block: heading, the secondary "Shop All" button and a sideways scrolling row of cards.
    fn shop_shelf(
        &mut self,
        name: &str,
        category: &str,
        skus: &[String],
        button: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let category = category.to_owned();
        let button = button.map(|label| {
            div()
                .id(SharedString::from(format!("shop-shelf-button-{category}")))
                .h(px(40.))
                .px(px(16.))
                .rounded(px(8.))
                .bg(color("--background-mod-subtle"))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|button| button.bg(color("--background-mod-strong")))
                .on_click(
                    cx.listener(move |this, _, _, cx| this.shop_open_collection(&category, cx)),
                )
                .child(text(16., 20., 500., "--text-strong").child(label.to_owned()))
        });
        div()
            .parity("shop-shelf")
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        text(20., 24., 600., "--text-strong")
                            .truncate()
                            .child(name.to_owned()),
                    )
                    .children(button),
            )
            .child(
                div()
                    .mt(px(24.))
                    .child(self.shop_shelf_row(name, skus, window, cx)),
            )
            .into_any_element()
    }

    /// The row of 246px cards with the 24px gap and the overlay arrows (`actions_s64Ydp`).
    pub(super) fn shop_shelf_row(
        &mut self,
        key: &str,
        skus: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let handle = self.shop.shelves.entry(key.to_owned()).or_default().clone();
        // The row is as wide as its cards: the scroller measures its overflow from its direct child, so a row that only
        // filled the viewport would leave the arrows and the sideways wheel nothing to move.
        let row_width = skus.len() as f32 * CARD + skus.len().saturating_sub(1) as f32 * 24.;
        let mut row = div().flex().gap(px(24.)).flex_none().w(px(row_width));
        for sku in skus {
            row = row.child(self.shop_card(sku, Some(CARD), window, cx));
        }
        let max = handle.max_offset().x;
        let offset = -handle.offset().x;
        let (left, right) = (offset > px(1.), offset + px(1.) < max || max <= px(0.));
        let arrow = |id: String, name: &'static str, at_left: bool, handle: ScrollHandle| {
            div()
                .id(SharedString::from(id))
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
                .cursor_pointer()
                .on_click(cx.listener(move |_, _, _, cx| {
                    let step = px(CARD + 24.) * 2.;
                    let current = handle.offset();
                    let next = if at_left {
                        (current.x + step).min(px(0.))
                    } else {
                        (current.x - step).max(-handle.max_offset().x)
                    };
                    handle.set_offset(point(next, current.y));
                    cx.notify();
                }))
                .child(icon(name, 24., "--white"))
        };
        let key = key.to_owned();
        div()
            .relative()
            .w_full()
            .h(px(CARD))
            .child(
                div()
                    .id(SharedString::from(format!("shop-shelf-{key}")))
                    .size_full()
                    .overflow_x_scroll()
                    .track_scroll(&handle)
                    .child(row),
            )
            .when(left, |this| {
                this.child(arrow(
                    format!("shop-shelf-left-{key}"),
                    "shop_arrow_left",
                    true,
                    handle.clone(),
                ))
            })
            .when(right, |this| {
                this.child(arrow(
                    format!("shop-shelf-right-{key}"),
                    "shop_arrow_right",
                    false,
                    handle.clone(),
                ))
            })
            .into_any_element()
    }

    /// The marketing blocks (`marketingBlock_nQ1zr7`): two to a row, 236 tall, art filling the rounded box.
    fn shop_marketing(&mut self, subs: &[data::SubBlock], cx: &mut Context<Self>) -> AnyElement {
        let mut grid = div()
            .parity("shop-marketing")
            .w_full()
            .flex()
            .flex_wrap()
            .gap(px(24.));
        for (index, sub) in subs.iter().enumerate() {
            let art = self.shop_cover_box(sub.asset.as_deref(), (833. - 24.) / 2., 236.);
            let category = sub.category.clone();
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("shop-marketing-{index}")))
                    .relative()
                    .w(px((833. - 24.) / 2.))
                    .h(px(236.))
                    .rounded(px(16.))
                    .overflow_hidden()
                    .bg(color("--background-base-low"))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shop_open_collection(&category, cx);
                    }))
                    .children(art),
            );
        }
        grid.into_any_element()
    }

    /// The wide banner of the Orbs page: the art with its title and body.
    fn shop_wide_banner(
        &mut self,
        title: &str,
        body: &str,
        banner: Option<&str>,
        logo: Option<&str>,
    ) -> AnyElement {
        let art = self.shop_cover(banner);
        let logo = logo.and_then(|url| self.shop_img(url));
        div()
            .parity("shop-wide-banner")
            .relative()
            .w_full()
            .h(px(141.))
            .rounded(px(16.))
            .overflow_hidden()
            .bg(color("--background-base-low"))
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
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(text(24., 32., 600., "--text-strong").child(title.to_owned()))
                            .child(text(16., 20., 400., "--text-default").child(body.to_owned())),
                    ),
            )
            .into_any_element()
    }

    /// "Find your style" (`popularPicksSection_XBXDcd`): the sort select, Shuffle and a three column feed.
    fn shop_feed(
        &mut self,
        orbs: bool,
        popular: &[String],
        recommended: &[String],
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
        let count = if orbs { 12 } else { 24 };
        let skus: Vec<String> = match sorted {
            Sort::Recommended => recommended.iter().take(count).cloned().collect(),
            Sort::Popular => popular.iter().take(count).cloned().collect(),
            _ if self.shop.feed.is_empty() => popular.iter().take(count).cloned().collect(),
            _ => self.shop.feed.clone(),
        };
        let label = match sorted {
            Sort::Recommended => "For You",
            Sort::Recent => "Recently Added",
            Sort::PriceLow => "Price: Low to High",
            _ => "Popular",
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
                    let route = data::search_route(&Filters::default(), sort, 0, 24);
                    let key = format!("shop:feed:{sort:?}");
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
        let shuffle = div()
            .id("shop-shuffle")
            .h(px(40.))
            .px(px(24.))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .cursor_pointer()
            .hover(|button| button.bg(color("--background-mod-strong")))
            .on_click(cx.listener(|this, _, _, cx| {
                this.shop.shuffle += 1;
                let key = format!("shop:feed:shuffle{}", this.shop.shuffle);
                this.shop.feed_sort = Some(Sort::Relevance);
                this.shop_request(
                    &key,
                    "/shop/search?randomize=true&offset=0&limit=32&published_collections_only=true"
                        .to_owned(),
                );
                cx.notify();
            }))
            .child(text(16., 20., 500., "--text-strong").child("Shuffle!"));
        let mut grid = div().parity("shop-feed").grid().grid_cols(3).gap(px(24.));
        for sku in skus.iter().filter(|sku| *sku != data::NITRO_1_DAY) {
            grid = grid.child(self.shop_card(sku, None, window, cx));
        }
        div()
            .parity("shop-popular")
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(text(20., 24., 600., "--text-strong").child(if orbs {
                                "Orbs Exclusives"
                            } else {
                                "Find your style"
                            }))
                            .child(icon("shop_info", 18., "--interactive-text-default")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(text(16., 20., 600., "--text-default").child("Sort by"))
                            .child(select)
                            .child(shuffle),
                    ),
            )
            .child(div().mt(px(24.)).child(grid))
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
                        .on_click(cx.listener(move |this, _, _, cx| choose(this, sort, cx)))
                        .child(text(14., 18., 500., "--text-strong").child(*name))
                        .into_any_element()
                })
                .collect();
            deferred(
                div()
                    .absolute()
                    .top(px(44.))
                    .left_0()
                    .w(px(width))
                    .occlude()
                    .p(px(8.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-high"))
                    .border(px(1.))
                    .border_color(color("--border-subtle"))
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
    use super::parse_gradient;

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
}
