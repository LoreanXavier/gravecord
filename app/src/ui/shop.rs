//! The Shop page behind the Shop row of the Home sidebar (the router's `/shop`). The header with its hover menus, the
//! search field, the wishlist button and the Orbs balance live here, together with the answers the pages read; the cards
//! are in `cards`, the Featured and Orbs Shop pages in `featured`, the Browse page in `browse` and the product modal in
//! `modal`. Official modules followed: 971458 (the header and its menus), 685491 (the search field), 253782 (the tab
//! page), 821816 (Browse), 987421 (the card), 110676 (the blocks).
mod browse;
mod cards;
mod data;
mod featured;
mod modal;

use super::discover::{gutter_scrollbar, text};
use super::{Root, color, editor_style, icon};
use crate::{
    editor::{Editor, EditorEvent},
    parity::Parity,
};
use data::{Category, Filters, Home, Product, Sort};
use gpui::{
    AnyElement, Context, Entity, Focusable, IntoElement, ScrollHandle, SharedString, Subscription,
    Window, deferred, div, img, point, prelude::*, px,
};
use std::collections::{HashMap, HashSet};

pub(super) const FETCH_PREFIX: &str = "shop:";
/// The width of the scroller's gutter and the page's left edge in the 1280 window.
const GUTTER: f32 = 8.;
const HEADER: f32 = 49.;

/// The tabs of the header (`G2` of module 758836) the page can show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Featured,
    Browse,
    Orbs,
}

/// The hover menus of the header and the balance popout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Menu {
    Browse,
    GameShops,
    More,
    Balance,
}

#[derive(Default)]
pub(super) struct State {
    pub open: bool,
    pub tab: Tab,
    /// Which header entry the pointer is on, and which menu it is on; a menu stays open while either holds.
    pub hover_item: Option<Menu>,
    pub hover_menu: Option<Menu>,
    pub scroll: ScrollHandle,
    pub search: Option<Entity<Editor>>,
    pub events: Option<Subscription>,
    pub home: Home,
    pub orbs: Home,
    pub collections: Vec<Category>,
    pub collections_total: u64,
    pub products: HashMap<String, Product>,
    pub balance: Option<i64>,
    /// Skus the account wishlisted in this session (the wishlist read itself is not fetched).
    pub wishlist: HashSet<String>,
    pub filters: Filters,
    pub sort: Sort,
    pub filters_open: bool,
    pub sort_open: bool,
    /// The search answer of the Browse page: sku ids and whether more pages exist.
    pub results: Option<(Vec<String>, bool)>,
    pub feed: Vec<String>,
    pub modal: Option<String>,
    pub shelves: HashMap<String, ScrollHandle>,
    pub notice: Option<String>,
    pub checkout: Option<modal::Checkout>,
    /// The sort of the "Find your style" feed, its select being open, and how many times Shuffle was pressed.
    /// The feed's chosen sort; `None` is the block's default ("For You" when it has recommendations).
    pub feed_sort: Option<Sort>,
    pub feed_menu: bool,
    pub shuffle: u32,
    /// The collection page being shown on the Browse tab (a category's sku id).
    pub collection: Option<String>,
    /// The id of the account's wishlist, learned from the answer to an add.
    pub wishlist_id: Option<String>,
    /// The id of the order an Orbs claim created.
    pub order: Option<String>,
    /// The card the pointer is over (an effect preview plays while it is).
    pub hover_card: Option<String>,
    /// The scroll of the product modal's body.
    pub modal_scroll: ScrollHandle,
    pub catalog_pages: u32,
    pub results_key: String,
}

impl Root {
    /// The Shop is the Home content while its flag is set and nothing else has taken the content area.
    pub(super) fn shop_active(&self) -> bool {
        self.shop.open
            && self.guild.is_none()
            && self.channel.is_none()
            && !self.nitro_open
            && !self.discover.open
            && !self.quests_active()
    }

    pub(super) fn open_shop(&mut self, cx: &mut Context<Self>) {
        if self.shop_active() {
            return;
        }
        let from = self.route();
        self.history.visit(
            from,
            super::history::Route {
                guild: None,
                channel: None,
                nitro: false,
                guild_guide: false,
                channel_browser: false,
                discover: None,
                quests: false,
                shop: true,
            },
        );
        self.set_shop_open(cx);
    }

    /// The part of `open_shop` a history replay also runs.
    pub(super) fn set_shop_open(&mut self, cx: &mut Context<Self>) {
        self.shop.open = true;
        self.guild = None;
        self.channel = None;
        self.nitro_open = false;
        self.guild_guide_open = false;
        self.channel_browser_open = false;
        self.discover.open = false;
        self.quests.open = false;
        self.shop_enter(cx);
    }

    pub(super) fn shop_enter(&mut self, cx: &mut Context<Self>) {
        self.shop_search_editor(cx);
        self.shop.scroll.set_offset(point(px(0.), px(0.)));
        self.shop_request("shop:home", data::shop_home_route("home"));
        self.shop_request("shop:orbs", data::shop_home_route("orbs"));
        self.shop_request("shop:balance", data::BALANCE_ROUTE.to_owned());
        cx.notify();
    }

    pub(super) fn shop_request(&mut self, key: &str, route: String) {
        if self.requested.insert(key.to_owned()) {
            let _ = self.requests.send(crate::net::Request::Fetch {
                key: key.to_owned(),
                route,
            });
        }
    }

    pub(super) fn shop_set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.shop.tab = tab;
        self.shop.hover_item = None;
        self.shop.hover_menu = None;
        self.shop.modal = None;
        self.shop.scroll.set_offset(point(px(0.), px(0.)));
        if tab == Tab::Browse {
            self.shop_browse_enter();
        }
        cx.notify();
    }

    /// The bytes behind a shop image, asking for them on the first draw.
    pub(super) fn shop_img(&mut self, url: &str) -> Option<std::sync::Arc<gpui::Image>> {
        let key = format!("shop:img:{url}");
        if !self.requested.contains(&key) {
            self.requested.insert(key.clone());
            let _ = self.requests.send(crate::net::Request::Image {
                key: key.clone(),
                url: url.to_owned(),
            });
        }
        self.images.get(&key).cloned()
    }

    /// Asks for the products that no answer has delivered yet (`requestProducts` of module 295586: batches of 100).
    pub(super) fn shop_need_products(&mut self, skus: &[String]) {
        let missing: Vec<String> = skus
            .iter()
            .filter(|sku| !self.shop.products.contains_key(*sku))
            .cloned()
            .collect();
        for chunk in missing.chunks(100) {
            let key = format!("shop:skus:{}:{}", chunk[0], chunk.len());
            self.shop_request(&key, data::products_route(chunk));
        }
    }

    pub(super) fn shop_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(body) = self.fetched.get(key).cloned() else {
            return;
        };
        if key.starts_with("shop:order:") {
            self.shop_order_fetched(key, &body, cx);
            return;
        }
        if body["fetch_failed"] == true {
            self.shop_forget(key);
            cx.notify();
            return;
        }
        let remember = |this: &mut Root, products: &[Product]| {
            for product in products {
                this.shop
                    .products
                    .insert(product.sku_id.clone(), product.clone());
            }
        };
        match key {
            "shop:wishlist" => {
                // `{body: {id, wishlist_items: [{sku: {id}}]}}` (module 808247's answer to an add or a remove).
                let list = &body["body"];
                self.shop.wishlist_id = list["id"].as_str().map(str::to_owned);
                if let Some(items) = list["wishlist_items"].as_array() {
                    self.shop.wishlist = items
                        .iter()
                        .filter_map(|item| item["sku"]["id"].as_str().map(str::to_owned))
                        .collect();
                }
            }
            "shop:home" | "shop:orbs" => {
                let home = data::parse_home(&body);
                for category in &home.categories {
                    remember(self, &category.products);
                }
                let mut wanted = Vec::new();
                for block in &home.blocks {
                    match block {
                        data::Block::Feed {
                            popular,
                            recommended,
                        } if key == "shop:home" => {
                            wanted.extend(popular.iter().take(24).cloned());
                            wanted.extend(recommended.iter().take(24).cloned());
                        }
                        data::Block::Feed { popular, .. } => {
                            wanted.extend(popular.iter().take(12).cloned());
                        }
                        data::Block::Shelf { skus, .. } | data::Block::Hero { skus, .. } => {
                            wanted.extend(skus.iter().cloned());
                        }
                        _ => {}
                    }
                }
                if key == "shop:home" {
                    self.shop.home = home;
                } else {
                    self.shop.orbs = home;
                }
                self.shop_need_products(&wanted);
            }
            "shop:balance" => {
                self.shop.balance = body["balance"].as_i64();
            }
            _ if key.starts_with("shop:collections:") => {
                let (categories, total) = data::parse_collections(&body);
                for category in &categories {
                    remember(self, &category.products);
                }
                self.shop.collections.extend(categories);
                self.shop.collections_total = total;
            }
            _ if key.starts_with("shop:search:") => {
                let (skus, more) = data::parse_search(&body);
                self.shop_need_products(&skus);
                if key == self.shop.results_key {
                    self.shop.results = Some((skus, more));
                }
            }
            _ if key.starts_with("shop:skus:") => {
                remember(self, &data::parse_products(&body));
            }
            _ if key.starts_with("shop:feed:") => {
                let (skus, _) = data::parse_search(&body);
                self.shop_need_products(&skus);
                self.shop.feed = skus;
            }
            _ if key.starts_with("shop:product:") => {
                if let Some(product) = data::parse_product(&body) {
                    remember(self, &[product]);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// Asks for a read again after an error.
    fn shop_forget(&mut self, key: &str) {
        self.requested.remove(key);
        self.fetched.remove(key);
    }

    /// Writes the raw answers the page parses to `design/parity/shop-dump-<name>.json` (a harness step); the shapes are
    /// noted beside each parser in `data`.
    #[cfg(feature = "capture")]
    pub fn shop_dump(&self) -> String {
        let mut written = Vec::new();
        for (key, body) in &self.fetched {
            if let Some(name) = key.strip_prefix("shop:") {
                let name = name.replace(':', "-");
                let path = format!("design/parity/shop-dump-{name}.json");
                let text = serde_json::to_string_pretty(body).unwrap_or_default();
                if std::fs::write(&path, text).is_ok() {
                    written.push(name);
                }
            }
        }
        written.sort();
        format!("shop dump: {}", written.join(", "))
    }

    #[cfg(feature = "capture")]
    pub(super) fn shop_search_editors(&self) -> Vec<Entity<Editor>> {
        if self.shop_active() {
            self.shop.search.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }

    // ---- the search field ----

    fn shop_search_editor(&mut self, cx: &mut Context<Self>) {
        if self.shop.search.is_some() {
            return;
        }
        let editor = cx.new(|cx| {
            let mut style = editor_style();
            style.size = 14.;
            style.line_height = 18.;
            style.text = super::hsla("--input-text-default");
            style.caret = super::hsla("--input-text-default");
            style.placeholder = super::hsla("--input-placeholder-text-default");
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder("Search the Shop".into());
            editor.set_reports_all_edits(true);
            editor
        });
        let events = cx.subscribe(&editor, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {}
                EditorEvent::Cancel => {
                    editor.update(cx, |editor, _| editor.set_text(""));
                    this.shop_search("", cx);
                }
                // Enter sends the text to the catalog (`ee` of module 685491).
                EditorEvent::Submit(text) => this.shop_search(text.trim(), cx),
            }
            cx.notify();
        });
        self.shop.search = Some(editor);
        self.shop.events = Some(events);
    }

    /// Runs a search on the Browse page, or clears it.
    pub(super) fn shop_search(&mut self, text: &str, cx: &mut Context<Self>) {
        self.shop.filters.search = text.chars().take(256).collect();
        if self.shop.tab != Tab::Browse {
            self.shop_set_tab(Tab::Browse, cx);
        } else {
            self.shop_browse_enter();
        }
        cx.notify();
    }

    // ---- the page ----

    pub(super) fn shop_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let header = self.shop_header(window, cx);
        let scroll = self.shop.scroll.clone();
        let body = match self.shop.tab {
            Tab::Featured => self.shop_featured(false, window, cx),
            Tab::Orbs => self.shop_featured(true, window, cx),
            Tab::Browse => self.shop_browse(window, cx),
        };
        let scroller = div()
            .id("shop-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .pr(px(GUTTER))
            .child(body);
        let notice = self.shop.notice.clone().map(|message| {
            div()
                .absolute()
                .bottom(px(16.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    text(14., 18., 500., "--text-strong")
                        .px(px(16.))
                        .py(px(10.))
                        .rounded(px(8.))
                        .bg(color("--background-surface-higher"))
                        .child(message),
                )
        });
        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .pt(px(1.))
            .child(
                div()
                    .parity("shop-page")
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .bg(color("--background-base-lower"))
                    .child(header)
                    .child(
                        div()
                            .parity("shop-view")
                            .relative()
                            .flex_1()
                            .min_h(px(0.))
                            .child(scroller)
                            .child(gutter_scrollbar(scroll)),
                    )
                    .children(notice),
            )
            .into_any_element()
    }

    // ---- the header ----

    /// `headerBar_ViLURN`: 49px tall (the 1px rule included), the shop mark, the tabs, and the search, wishlist and Orbs
    /// balance at the right.
    fn shop_header(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.shop.tab;
        let open = self.shop.hover_item.or(self.shop.hover_menu);
        let nav = div()
            .parity("shop-nav")
            .h(px(48.))
            .ml(px(44.))
            .flex()
            .items_center()
            .gap(px(32.))
            .child(self.shop_nav_item("featured", "Featured", tab == Tab::Featured, None, cx))
            .child(self.shop_nav_item(
                "browse",
                "Browse",
                tab == Tab::Browse,
                Some(Menu::Browse),
                cx,
            ))
            .child(self.shop_nav_item("orbs", "Orbs Shop", tab == Tab::Orbs, None, cx))
            .child(self.shop_nav_item("game-shops", "Game Shops", false, Some(Menu::GameShops), cx))
            .child(self.shop_nav_item("more", "More", false, Some(Menu::More), cx));
        let search = self.shop_search_field(window, cx);
        let wishlist = self.shop_wishlist_button(cx);
        let balance = self.shop_balance_pill(cx);
        let menu = open.and_then(|menu| self.shop_menu(menu, cx));
        div()
            .parity("shop-header")
            .relative()
            .flex_none()
            .w_full()
            .h(px(HEADER))
            .pl(px(16.))
            .pr(px(12.))
            .flex()
            .items_center()
            .child(
                div()
                    .size(px(24.))
                    .flex_none()
                    .child(icon("shop_header", 24., "--text-strong")),
            )
            .child(nav)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(search)
                    .child(wishlist)
                    .child(balance),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(1.))
                    .bg(color("--background-mod-subtle")),
            )
            .children(menu)
            .into_any_element()
    }

    fn shop_nav_item(
        &mut self,
        id: &'static str,
        label: &'static str,
        active: bool,
        menu: Option<Menu>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let open = menu.is_some() && (self.shop.hover_item == menu || self.shop.hover_menu == menu);
        let tab = match id {
            "featured" => Some(Tab::Featured),
            "browse" => Some(Tab::Browse),
            "orbs" => Some(Tab::Orbs),
            _ => None,
        };
        div()
            .id(SharedString::from(format!("shop-nav-{id}")))
            .group(SharedString::from(format!("shop-nav-{id}")))
            .relative()
            .flex_none()
            .h_full()
            .flex()
            .items_center()
            .cursor_pointer()
            .child(
                text(16., 20., 500., "--text-default")
                    .flex()
                    .items_center()
                    .child(label),
            )
            .when(menu.is_some(), |item| {
                item.child(
                    div()
                        .w(px(20.))
                        .flex()
                        .justify_end()
                        .items_center()
                        .child(icon(
                            if open { "shop_caret_up" } else { "shop_caret" },
                            12.,
                            "--text-default",
                        )),
                )
            })
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(2.))
                    .when(active, |bar| bar.bg(color("--text-strong")))
                    .when(!active, |bar| {
                        bar.group_hover(SharedString::from(format!("shop-nav-{id}")), |bar| {
                            bar.bg(color("--background-brand"))
                        })
                    }),
            )
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if menu.is_some() {
                    if *hovered {
                        this.shop.hover_item = menu;
                    } else if this.shop.hover_item == menu {
                        this.shop.hover_item = None;
                    }
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| match (tab, menu) {
                (Some(tab), _) => this.shop_set_tab(tab, cx),
                (None, Some(Menu::GameShops | Menu::More)) => {}
                _ => {}
            }))
    }

    /// `searchBar`: 182 by 32 with the 12px magnifier in a 22px box.
    fn shop_search_field(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = self.shop.search.clone() else {
            return div().into_any_element();
        };
        let focused = editor.read(cx).focus_handle(cx).is_focused(window);
        let filled = !editor.read(cx).text().is_empty();
        let border = if focused { 2. } else { 1. };
        div()
            .parity_with_border("shop-search", border)
            .flex_none()
            .w(px(182.))
            .h(px(32.))
            .border(px(border))
            .border_color(color(if focused {
                "--input-border-active"
            } else {
                "--input-border-default"
            }))
            .rounded(px(8.))
            .bg(color("--input-background-default"))
            .pl(px(4. - border))
            .pr(px(5. - border))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("shop_search", 12., "--interactive-text-default")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(18.))
                    .child(editor.clone()),
            )
            .when(filled, |row| {
                row.child(
                    div()
                        .id("shop-search-clear")
                        .size(px(22.))
                        .flex_none()
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(editor) = this.shop.search.clone() {
                                editor.update(cx, |editor, _| editor.set_text(""));
                            }
                            this.shop_search("", cx);
                        }))
                        .child(icon("discover_clear", 12., "--interactive-text-default")),
                )
            })
            .into_any_element()
    }

    /// The 32px wishlist button: it opens the profile's Wishlist tab.
    fn shop_wishlist_button(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let button = div()
            .id("shop-wishlist")
            .parity("shop-wishlist")
            .size(px(32.))
            .flex_none()
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(color("--background-mod-strong")))
            .on_click(cx.listener(|this, _, _, cx| this.shop_open_wishlist(cx)))
            .child(icon("shop_heart", 16., "--text-default"));
        self.with_tip(
            "shop-wishlist",
            super::tooltip::Tip::text("Wishlist", super::tooltip::Side::Bottom),
            button,
            cx,
        )
        .into_any_element()
    }

    /// The balance pill: the Orbs mark and the amount, with the popout of module 986630 on hover.
    fn shop_balance_pill(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let amount = self.shop.balance.unwrap_or(0);
        div()
            .id("shop-balance")
            .parity("shop-balance")
            .flex_none()
            .h(px(32.))
            .px(px(12.))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .gap(px(2.))
            .cursor_pointer()
            .hover(|pill| pill.bg(color("--background-mod-strong")))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.shop.hover_item = Some(Menu::Balance);
                } else if this.shop.hover_item == Some(Menu::Balance) {
                    this.shop.hover_item = None;
                }
                cx.notify();
            }))
            .child(icon("shop_orb_18", 20., "--text-default"))
            .child(text(16., 20., 600., "--text-default").child(amount.to_string()))
            .into_any_element()
    }

    /// The popout under a header entry: 4px below it, left-aligned with it.
    fn shop_menu(&mut self, menu: Menu, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (left, entries): (f32, Vec<(&'static str, Option<Tab>)>) = match menu {
            Menu::Browse => (
                179.,
                vec![
                    ("Avatar Decorations", Some(Tab::Browse)),
                    ("Nameplates", Some(Tab::Browse)),
                    ("Profile Effects", Some(Tab::Browse)),
                    ("Profile Frames", Some(Tab::Browse)),
                    ("Bundles", Some(Tab::Browse)),
                    ("Collabs", Some(Tab::Browse)),
                    ("Shop All", Some(Tab::Browse)),
                ],
            ),
            Menu::GameShops => (
                337.,
                vec![
                    ("Marvel Rivals", None),
                    ("Rust", None),
                    ("Bloons TD 6", None),
                ],
            ),
            Menu::More => (
                538.,
                vec![("Game Servers", None), ("Collection Index", None)],
            ),
            Menu::Balance => return Some(self.shop_balance_popout(cx)),
        };
        let rows: Vec<AnyElement> = entries
            .into_iter()
            .map(|(label, tab)| {
                div()
                    .id(SharedString::from(format!("shop-menu-{label}")))
                    .h(px(34.))
                    .px(px(8.))
                    .rounded(px(2.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(|row| row.bg(color("--background-mod-subtle")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shop_menu_choose(menu, label, tab, cx);
                    }))
                    .child(text(14., 18., 600., "--text-strong").child(label))
                    .into_any_element()
            })
            .collect();
        Some(
            deferred(
                div()
                    .id("shop-menu")
                    .absolute()
                    .top(px(HEADER - 2.))
                    .left(px(left))
                    .pt(px(4.))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.shop.hover_menu = Some(menu);
                        } else if this.shop.hover_menu == Some(menu) {
                            this.shop.hover_menu = None;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .parity("shop-menu")
                            .occlude()
                            .min_w(px(150.))
                            .p(px(8.))
                            .rounded(px(8.))
                            .bg(color("--background-surface-high"))
                            .border(px(1.))
                            .border_color(color("--border-subtle"))
                            .flex()
                            .flex_col()
                            .children(rows),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    fn shop_menu_choose(
        &mut self,
        menu: Menu,
        label: &str,
        tab: Option<Tab>,
        cx: &mut Context<Self>,
    ) {
        self.shop.hover_item = None;
        self.shop.hover_menu = None;
        if menu == Menu::Browse {
            let kind = match label {
                "Avatar Decorations" => Some("AVATAR_DECORATION"),
                "Nameplates" => Some("NAMEPLATE"),
                "Profile Effects" => Some("PROFILE_EFFECT"),
                "Profile Frames" => Some("PROFILE_FRAME"),
                "Bundles" => Some("BUNDLE"),
                _ => None,
            };
            self.shop.filters = Filters::default();
            if let Some(kind) = kind {
                self.shop.filters.item_types.push(kind.to_owned());
            }
            self.shop.filters.collabs = label == "Collabs";
        }
        if let Some(tab) = tab {
            self.shop_set_tab(tab, cx);
        }
        cx.notify();
    }

    /// The balance popout: what Orbs are, and the button to Quest Home.
    fn shop_balance_popout(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let amount = self.shop.balance.unwrap_or(0);
        deferred(
            div()
                .absolute()
                .top(px(HEADER - 6.))
                .right(px(12.))
                .pt(px(4.))
                .child(
                    div()
                        .parity("shop-balance-popout")
                        .occlude()
                        .w(px(280.))
                        .p(px(16.))
                        .rounded(px(8.))
                        .bg(color("--background-surface-high"))
                        .border(px(1.))
                        .border_color(color("--border-subtle"))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(icon("shop_orb_18", 20., "--text-strong"))
                                .child(
                                    text(16., 20., 600., "--text-strong")
                                        .child(format!("{amount} Orbs")),
                                ),
                        )
                        .child(
                            text(14., 18., 400., "--text-default").child(
                                "Complete Quests to earn Orbs, then spend them in the Shop.",
                            ),
                        )
                        .child(
                            div()
                                .id("shop-balance-quests")
                                .h(px(32.))
                                .rounded(px(8.))
                                .bg(color("--background-brand"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.shop.hover_item = None;
                                    this.open_quests(cx);
                                }))
                                .child(text(14., 18., 500., "--white").child("Go to Quest Home")),
                        ),
                ),
        )
        .with_priority(2)
        .into_any_element()
    }

    /// The heart button opens the profile modal on its Wishlist tab; the wishlist read is not part of the page.
    fn shop_open_wishlist(&mut self, cx: &mut Context<Self>) {
        self.shop.notice = Some("Your wishlist opens from your profile.".to_owned());
        cx.notify();
    }

    // ---- shared drawing helpers ----

    /// A remote image filling its box, or the box alone while it loads.
    pub(super) fn shop_cover(&mut self, url: Option<&str>) -> Option<AnyElement> {
        let image = self.shop_img(url?)?;
        Some(
            img(image)
                .size_full()
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element(),
        )
    }
}
