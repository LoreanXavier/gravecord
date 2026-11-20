//! The Browse page (module 821816, `pageWrapper_4Qm5zV`): the sort select and the filter toggle, the filter panel at the
//! right, and below them either the catalog's collections (a banner and the cards of each) or the answer of a search.
//! A collection page (a category's own page) reuses the grid under that category's banner.

use super::data::{self, Category, Filters, Sort};
use super::{Root, color, icon, text};
use crate::parity::Parity;
use gpui::{AnyElement, Context, IntoElement, SharedString, Window, div, img, prelude::*, px};

/// How many collections one page of the catalog read asks for (module 821816's page size).
const PAGE: u32 = 5;
const COLUMN_GAP: f32 = 24.;

const ITEM_TYPES: [(&str, &str); 5] = [
    ("Avatar Decorations", "AVATAR_DECORATION"),
    ("Nameplates", "NAMEPLATE"),
    ("Profile Effects", "PROFILE_EFFECT"),
    ("Profile Frames", "PROFILE_FRAME"),
    ("Bundles", "BUNDLE"),
];
const COLORS: [(&str, &str); 10] = [
    ("#9B59B6", "COLLECTIBLES_COLOR_PURPLE"),
    ("#3498DB", "COLLECTIBLES_COLOR_BLUE"),
    ("#2ECC71", "COLLECTIBLES_COLOR_GREEN"),
    ("#A0522D", "COLLECTIBLES_COLOR_BROWN"),
    ("#F1C40F", "COLLECTIBLES_COLOR_YELLOW"),
    ("#E67E22", "COLLECTIBLES_COLOR_ORANGE"),
    ("#E74C3C", "COLLECTIBLES_COLOR_RED"),
    ("#EC407A", "COLLECTIBLES_COLOR_PINK"),
    ("#FFFFFF", "COLLECTIBLES_COLOR_WHITE"),
    ("#262626", "COLLECTIBLES_COLOR_BLACK"),
];
const THEMES: [(&str, &str, &str); 10] = [
    ("Anime", "COLLECTIBLES_THEME_ANIME", "shop_theme_anime"),
    ("Gaming", "COLLECTIBLES_THEME_GAMING", "shop_theme_gaming"),
    (
        "Cute & Cozy",
        "COLLECTIBLES_THEME_CUTE_COZY",
        "shop_theme_cute_cozy",
    ),
    ("Sci-Fi", "COLLECTIBLES_THEME_SCI_FI", "shop_theme_sci_fi"),
    (
        "Food & Drinks",
        "COLLECTIBLES_THEME_FOOD_DRINKS",
        "shop_theme_food_drinks",
    ),
    (
        "Fantasy",
        "COLLECTIBLES_THEME_FANTASY",
        "shop_theme_fantasy",
    ),
    (
        "Animals & Pets",
        "COLLECTIBLES_THEME_ANIMALS_PETS",
        "shop_theme_animals_pets",
    ),
    ("Nature", "COLLECTIBLES_THEME_NATURE", "shop_theme_nature"),
    (
        "Movies & TV",
        "COLLECTIBLES_THEME_MOVIES_TV_SHOWS",
        "shop_theme_movies_tv",
    ),
    (
        "Dark & Moody",
        "COLLECTIBLES_THEME_DARK_MOODY",
        "shop_theme_dark_moody",
    ),
];

impl Root {
    /// Starts the reads the Browse page needs: the first catalog page, or the search for the chosen filters.
    pub(super) fn shop_browse_enter(&mut self) {
        if self.shop.filters.is_empty() {
            self.shop_catalog_page(0);
        } else {
            self.shop_run_search();
        }
    }

    fn shop_catalog_page(&mut self, page: u32) {
        let key = format!("shop:collections:{page}");
        self.shop_request(&key, data::collections_route(page * PAGE, PAGE));
    }

    /// The search read for the current filters and sort; its key names them so a change asks again.
    fn shop_run_search(&mut self) {
        let route = data::search_route(&self.shop.filters, self.shop.sort, 0, 24);
        let key = format!("shop:search:{}", route.len() as u64 * 31 + hash(&route));
        self.shop.results = self.fetched.get(&key).map(data::parse_search);
        self.shop.results_key = key.clone();
        self.shop_request(&key, route);
    }

    pub(super) fn shop_browse(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = window;
        if let Some(collection) = self.shop.collection.clone() {
            return self.shop_collection_page(&collection, window, cx);
        }
        let filters_open = self.shop.filters_open;
        let sort = self.shop.sort;
        let label = match sort {
            Sort::Recent => "Recently Added",
            Sort::PriceLow => "Price: Low to High",
            Sort::PriceHigh => "Price: High to Low",
            Sort::Popular | Sort::Recommended => "Popular",
            Sort::Relevance => "Relevance",
        };
        let mut options = vec![
            ("Recently Added", Sort::Recent),
            ("Price: Low to High", Sort::PriceLow),
            ("Price: High to Low", Sort::PriceHigh),
            ("Popular", Sort::Popular),
        ];
        if !self.shop.filters.search.is_empty() {
            options.push(("Relevance", Sort::Relevance));
        }
        let select = self.shop_select(
            "shop-browse-sort",
            label,
            200.,
            self.shop.sort_open,
            |this, open, cx| {
                this.shop.sort_open = open;
                cx.notify();
            },
            &options,
            |this, sort, cx| {
                this.shop.sort = sort;
                this.shop.sort_chosen = true;
                this.shop.sort_open = false;
                this.shop_browse_enter();
                cx.notify();
            },
            cx,
        );
        let toggle = div()
            .id("shop-filters-toggle")
            .parity("shop-filters-toggle")
            .h(px(40.))
            .px(px(14.))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .hover(|button| button.bg(color("--background-mod-strong")))
            .on_click(cx.listener(|this, _, _, cx| {
                this.shop.filters_open = !this.shop.filters_open;
                cx.notify();
            }))
            .child(
                text(16., 20., 500., "--text-strong").child(if filters_open {
                    "Hide Filters"
                } else {
                    "Show Filters"
                }),
            )
            .child(icon("shop_filters", 20., "--text-strong"));
        let controls = div()
            .parity("shop-controls")
            .w_full()
            .pt(px(20.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(self.shop_results_title())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(text(16., 20., 600., "--text-default").child("Sort by"))
                            .child(select),
                    )
                    .child(toggle),
            );
        let content = if self.shop.filters.is_empty() {
            self.shop_catalog(filters_open, window, cx)
        } else {
            self.shop_results_grid(filters_open, window, cx)
        };
        let panel = filters_open.then(|| self.shop_filter_panel(cx));
        div()
            .parity("shop-browse")
            .relative()
            .w_full()
            .pl(px(20.))
            .pr(px(12.))
            .pb(px(40.))
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(controls)
            .child(content)
            .children(panel)
            .into_any_element()
    }

    /// The count line at the left of the controls ("12 results for ..."), empty without filters.
    fn shop_results_title(&self) -> gpui::Div {
        let filters = &self.shop.filters;
        let count = self.shop.results.as_ref().map_or(0, |(skus, _)| skus.len());
        let title = if filters.is_empty() {
            String::new()
        } else if !filters.search.is_empty() {
            format!("{count} results for \"{}\"", filters.search)
        } else {
            format!("{count} results")
        };
        div().child(text(20., 24., 600., "--text-strong").child(title))
    }

    /// The catalog: each collection's banner over a three column grid of its cards; more pages follow the scroll.
    fn shop_catalog(
        &mut self,
        filters_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let categories = self.shop.collections.clone();
        let loaded_pages = self.shop.catalog_pages.max(1);
        let near_end = {
            let max = self.shop.scroll.max_offset().y;
            max <= px(0.) || -self.shop.scroll.offset().y + px(600.) >= max
        };
        let total = self.shop.collections_total as usize;
        if near_end && categories.len() < total {
            let next = (categories.len() as u32).div_ceil(PAGE);
            self.shop_catalog_page(next);
        }
        let _ = loaded_pages;
        let mut list = div().flex().flex_col().gap(px(24.));
        for category in &categories {
            list = list.child(self.shop_category(category, filters_open, window, cx));
        }
        list.into_any_element()
    }

    fn shop_category(
        &mut self,
        category: &Category,
        filters_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let banner = self.shop_cover_left(
            category
                .catalog_banner_url
                .as_deref()
                .or(category.hero_banner_url.as_deref()),
            865. / 180.,
        );
        let logo = category
            .hero_logo_url
            .as_deref()
            .and_then(|url| self.shop_img(url));
        let id = category.sku_id.clone();
        let skus: Vec<String> = category.products.iter().map(|p| p.sku_id.clone()).collect();
        let grid = self.shop_grid(&skus, filters_open, window, cx);
        div()
            .parity("shop-category")
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .id(SharedString::from(format!("shop-category-{id}")))
                    .relative()
                    .w_full()
                    .h(px(180.))
                    .rounded(px(32.))
                    .overflow_hidden()
                    .bg(color("--background-base-low"))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.shop_open_collection(&id, cx)))
                    .children(banner)
                    .children(logo.map(|logo| {
                        div()
                            .absolute()
                            .left(px(52.))
                            .top_0()
                            .bottom_0()
                            .w(px(175.))
                            .flex()
                            .items_center()
                            .child(
                                img(logo)
                                    .w(px(175.))
                                    .h(px(92.))
                                    .object_fit(gpui::ObjectFit::Contain),
                            )
                    })),
            )
            .child(grid)
            .into_any_element()
    }

    /// Fallback wishlist view when opened without a current user session.
    pub(super) fn shop_wishlist_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let skus = self.shop.wishlist_order.clone();
        let body = if skus.is_empty() {
            div()
                .h(px(200.))
                .flex()
                .items_center()
                .child(text(16., 20., 400., "--text-muted").child(dc_intl::t!("/X1ny6")))
                .into_any_element()
        } else {
            self.shop_grid(&skus, false, window, cx)
        };
        div()
            .parity("shop-wishlist-page")
            .flex()
            .flex_col()
            .gap(px(24.))
            .px(px(32.))
            .pt(px(32.))
            .pb(px(32.))
            .child(text(32., 40., 700., "--text-strong").child(dc_intl::t!("7lZ31J")))
            .child(body)
            .into_any_element()
    }

    /// The cards (`cardsContainer_Mqg79R`): as many columns of 246px or more as the page holds, 24px apart (three
    /// columns of 272.33 in the 865px page of the 1280 capture: `repeat(auto-fill, minmax(246px, 1fr))`).
    fn shop_grid(
        &mut self,
        skus: &[String],
        filters_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = filters_open;
        // The page's 20px left and 12px right padding.
        let width = (self.shop_content_width(window) - 32.).max(0.);
        let (columns, column) = data::feed_columns(width);
        let mut grid = div()
            .w_full()
            .grid()
            .grid_cols(columns as u16)
            .gap(px(COLUMN_GAP));
        let shown: Vec<String> = skus
            .iter()
            .filter(|sku| !self.shop.absent.contains(*sku))
            .cloned()
            .collect();
        for sku in &shown {
            grid = grid.child(self.shop_card(sku, Some(column), window, cx));
        }
        grid.into_any_element()
    }

    fn shop_results_grid(
        &mut self,
        filters_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((skus, _)) = self.shop.results.clone() else {
            return div().h(px(400.)).into_any_element();
        };
        self.shop_grid(&skus, filters_open, window, cx)
    }

    /// A category's own page: its banner and every product.
    fn shop_collection_page(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let category = self
            .shop
            .home
            .categories
            .iter()
            .chain(self.shop.orbs.categories.iter())
            .chain(self.shop.collections.iter())
            .find(|category| category.sku_id == id)
            .cloned();
        let Some(category) = category else {
            return div().h(px(400.)).into_any_element();
        };
        let banner = self.shop_cover(
            category
                .catalog_banner_url
                .as_deref()
                .or(category.hero_banner_url.as_deref()),
        );
        let skus: Vec<String> = category.products.iter().map(|p| p.sku_id.clone()).collect();
        let grid = self.shop_grid(&skus, false, window, cx);
        let breadcrumb = self.shop_collection_breadcrumb(&category.name, cx);
        let related = self.shop_vault_and_related_row(cx);
        let footer = self.shop_collection_footer(cx);
        div()
            .parity("shop-collection")
            .w_full()
            .px(px(20.))
            .pt(px(20.))
            .pb(px(40.))
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(180.))
                    .rounded(px(16.))
                    .overflow_hidden()
                    .bg(color("--background-base-low"))
                    .children(banner)
                    .child(
                        div()
                            .absolute()
                            .top(px(10.))
                            .left(px(16.))
                            .child(breadcrumb),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(24.))
                            .bottom(px(16.))
                            .child(text(24., 32., 600., "--text-strong").child(category.name)),
                    ),
            )
            .child(grid)
            .child(related)
            .child(footer)
            .into_any_element()
    }

    /// `filterOverlay_4Qm5zV`: 262 by 562 over the right of the page, 16px padded, 24px between the sections.
    fn shop_filter_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let filters = self.shop.filters.clone();
        let heading = |label: &'static str| text(16., 20., 600., "--text-strong").child(label);
        let checkbox = |id: String, label: &'static str, checked: bool| {
            div()
                .id(SharedString::from(id))
                .h(px(36.))
                .flex()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .child(
                    div()
                        .size(px(20.))
                        .flex_none()
                        .rounded(px(4.))
                        .border(px(2.))
                        .border_color(color(if checked {
                            "--background-brand"
                        } else {
                            "--interactive-text-default"
                        }))
                        .when(checked, |box_| box_.bg(color("--background-brand")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(checked, |box_| {
                            box_.child(icon("checkbox_check", 14., "--white"))
                        }),
                )
                .child(text(16., 20., 500., "--text-default").child(label))
        };
        let mut types = div().flex().flex_col();
        for (label, kind) in ITEM_TYPES {
            let on = filters.item_types.iter().any(|entry| entry == kind);
            types = types.child(
                checkbox(format!("shop-filter-type-{kind}"), label, on).on_click(cx.listener(
                    move |this, _, _, cx| {
                        Filters::toggled(&mut this.shop.filters.item_types, kind);
                        this.shop_filters_changed(cx);
                    },
                )),
            );
        }
        let only = div()
            .flex()
            .flex_col()
            .child(
                checkbox(
                    "shop-filter-orbs".into(),
                    "Orbs Eligible",
                    filters.orbs_eligible,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.shop.filters.orbs_eligible = !this.shop.filters.orbs_eligible;
                    this.shop_filters_changed(cx);
                })),
            )
            .child(
                checkbox("shop-filter-collabs".into(), "Collabs", filters.collabs).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.shop.filters.collabs = !this.shop.filters.collabs;
                        this.shop_filters_changed(cx);
                    }),
                ),
            );
        let mut swatches = div().flex().flex_wrap().gap(px(8.)).w(px(236.));
        for (hex, name) in COLORS {
            let on = filters.colors.iter().any(|entry| entry == name);
            let value = u32::from_str_radix(&hex[1..], 16).unwrap_or(0);
            swatches = swatches.child(
                div()
                    .id(SharedString::from(format!("shop-filter-color-{name}")))
                    .size(px(36.))
                    .flex_none()
                    .rounded_full()
                    .bg(gpui::rgb(value))
                    .border(px(2.))
                    .border_color(if on {
                        color("--text-strong")
                    } else {
                        gpui::Rgba {
                            a: 0.,
                            ..color("--text-strong")
                        }
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        Filters::toggled(&mut this.shop.filters.colors, name);
                        this.shop_filters_changed(cx);
                    })),
            );
        }
        let mut themes = div().flex().flex_wrap().gap(px(8.));
        for (label, name, art) in THEMES {
            let on = filters.themes.iter().any(|entry| entry == name);
            themes = themes.child(
                div()
                    .id(SharedString::from(format!("shop-filter-theme-{name}")))
                    .h(px(40.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border(px(1.))
                    .border_color(color(if on {
                        "--text-strong"
                    } else {
                        "--border-subtle"
                    }))
                    .bg(color("--background-base-low"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        Filters::toggled(&mut this.shop.filters.themes, name);
                        this.shop_filters_changed(cx);
                    }))
                    .child(icon(art, 16., "--text-strong"))
                    .child(text(14., 18., 500., "--text-strong").child(label)),
            );
        }
        let reset = div()
            .id("shop-filters-reset")
            .h(px(32.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                let search = std::mem::take(&mut this.shop.filters.search);
                this.shop.filters = Filters {
                    search,
                    ..Filters::default()
                };
                this.shop_filters_changed(cx);
            }))
            .child(text(14., 18., 500., "--text-strong").child("Reset"));
        div()
            .id("shop-filter-panel")
            .parity("shop-filters")
            .occlude()
            .absolute()
            .top(px(84.))
            .right(px(12.))
            .w(px(262.))
            .h(px(562.))
            .rounded(px(8.))
            .border(px(1.))
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-low"))
            .overflow_y_scroll()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(heading("Item Type"))
                    .child(types),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(heading("Show Only"))
                    .child(only),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(heading("Color"))
                    .child(swatches),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(heading("Theme"))
                    .child(themes),
            )
            .child(reset)
            .into_any_element()
    }

    fn shop_filters_changed(&mut self, cx: &mut Context<Self>) {
        self.shop.collection = None;
        self.shop_default_sort();
        self.shop_browse_enter();
        cx.notify();
    }
}

fn hash(text: &str) -> u64 {
    text.bytes().fold(1469598103934665603u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(1099511628211)
    })
}
