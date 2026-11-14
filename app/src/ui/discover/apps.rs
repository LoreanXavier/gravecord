//! The Apps pages (module 586193): the directory home with its collections, the category tabs, the search results with
//! their category counts, and an application's own page.
use super::app_data::{self, App, AppPage, Collection};
use super::data::{self, Category};
use super::{Load, Place, Root, SearchMode, text};
use crate::ui::{color, icon};
use crate::{net::Request, parity::Parity};
use dc_state::Id;
use gpui::{
    AnyElement, ClipboardItem, Context, IntoElement, ObjectFit, SharedString, div, img, point,
    prelude::*, px,
};
use serde_json::{Value, json};
use std::collections::HashMap;

/// `GZoV1O`'s help article (`APPS_LEARN_MORE`, module 652215).
const LEARN_MORE: &str =
    "https://support.discord.com/hc/en-us/articles/26592957841303-How-to-Discover-and-Add-Apps";
const GAP: f32 = 16.;

#[derive(Clone, Debug, Default)]
pub(in crate::ui) struct AppSearch {
    pub query: String,
    pub category: Option<i64>,
    pub page: u32,
    pub result: Load<AppPage>,
    /// The counts of the unfiltered answer, which the category column keeps while a category is chosen.
    pub counts: Vec<(i64, u64)>,
    pub all_total: u64,
    pub seq: u64,
}

pub(in crate::ui) struct Apps {
    pub mode: SearchMode,
    /// 0 is the Featured tab, otherwise a category id.
    pub tab: i64,
    pub collections: Load<Vec<Collection>>,
    pub categories: Vec<Category>,
    pub listing: HashMap<(i64, u32), Load<AppPage>>,
    pub page: u32,
    pub search: Option<AppSearch>,
    pub details: HashMap<Id, Load<Value>>,
    pub similar: HashMap<Id, Vec<App>>,
    pub covers: HashMap<Id, Option<String>>,
    pub carousel: usize,
    pub overview_open: bool,
    /// The two colors of a banner-less application's page header, made once from its icon.
    pub banner_colors: HashMap<Id, ([u8; 3], [u8; 3])>,
    pub seq: u64,
}

impl Default for Apps {
    fn default() -> Self {
        Self {
            mode: SearchMode::Inline,
            tab: 0,
            collections: Load::Idle,
            categories: Vec::new(),
            listing: HashMap::new(),
            page: 1,
            search: None,
            details: HashMap::new(),
            similar: HashMap::new(),
            covers: HashMap::new(),
            carousel: 0,
            overview_open: false,
            banner_colors: HashMap::new(),
            seq: 0,
        }
    }
}

/// The hero text of a category page (`eE`): the description by category id.
fn category_description(id: i64) -> &'static str {
    match id {
        4 => "Watch videos, listen to music, and share moments together.",
        6 => "Jump into solo puzzles, party games, competitive showdowns, and more.",
        8 => "Build, manage, and grow a thriving community.",
        9 => "Gather, spark conversations, and build connections.",
        10 => "Add convenience, organization, and productivity to your hangouts.",
        _ => "",
    }
}

/// The columns of `content_ZWbFoF` by window width: 1 below 800px, 2 from 800px, 4 from 1200px.
fn columns_for(viewport: f32) -> usize {
    if viewport >= 1200. {
        4
    } else if viewport >= 800. {
        2
    } else {
        1
    }
}

impl Root {
    pub(super) fn apps_enter(&mut self, _cx: &mut Context<Self>) {
        self.discover_request(
            "discover:apps:categories".into(),
            app_data::categories_route(),
        );
        self.apps_load_tab();
    }

    fn apps_load_tab(&mut self) {
        let tab = self.discover.apps.tab;
        if tab == 0 {
            if self.discover.apps.collections == Load::Idle {
                self.discover.apps.collections = Load::Loading;
            }
            self.discover_request(
                "discover:apps:collections".into(),
                app_data::collections_route(),
            );
        } else {
            let page = self.discover.apps.page;
            let entry = self.discover.apps.listing.entry((tab, page)).or_default();
            if *entry == Load::Idle {
                *entry = Load::Loading;
            }
            self.discover_request(
                format!("discover:apps:list:{tab}:{page}"),
                app_data::search_route("", Some(tab), page),
            );
        }
    }

    pub(super) fn app_enter(&mut self, id: u64, _cx: &mut Context<Self>) {
        self.discover.apps.carousel = 0;
        self.discover.apps.overview_open = false;
        self.app_request(id);
    }

    fn app_request(&mut self, id: u64) {
        self.discover.apps.overview_open = false;
        self.discover.apps.details.entry(id).or_default();
        if self.discover.apps.details[&id] == Load::Idle {
            self.discover.apps.details.insert(id, Load::Loading);
        }
        self.discover_request(format!("discover:app:{id}"), app_data::app_route(id));
        self.discover_request(
            format!("discover:app:similar:{id}"),
            app_data::similar_route(id),
        );
    }

    pub(super) fn apps_fetched(&mut self, key: &str) {
        let body = self.fetched.get(key).cloned().unwrap_or_default();
        let failed = crate::net::is_fetch_failure(&body);
        if key == "discover:apps:collections" {
            if failed {
                self.discover.apps.collections = Load::Failed;
                self.discover_forget(key);
            } else {
                let collections = app_data::parse_collections(&body);
                for collection in &collections {
                    self.app_images(&collection.apps);
                }
                self.discover.apps.collections = Load::Ready(collections);
            }
        } else if key == "discover:apps:categories" {
            if failed {
                self.discover_forget(key);
            } else {
                self.discover.apps.categories = data::parse_categories(&body);
            }
        } else if let Some(rest) = key.strip_prefix("discover:apps:list:") {
            let mut parts = rest.split(':');
            let (Some(Ok(tab)), Some(Ok(page))) = (
                parts.next().map(str::parse::<i64>),
                parts.next().map(str::parse::<u32>),
            ) else {
                return;
            };
            if failed {
                self.discover.apps.listing.insert((tab, page), Load::Failed);
                self.discover_forget(key);
            } else {
                let result = app_data::parse_search(&body);
                self.app_images(&result.apps);
                self.discover
                    .apps
                    .listing
                    .insert((tab, page), Load::Ready(result));
            }
        } else if let Some(seq) = key.strip_prefix("discover:apps:search:") {
            let Ok(seq) = seq.parse::<u64>() else { return };
            let mut images = Vec::new();
            if let Some(search) = self.discover.apps.search.as_mut()
                && search.seq == seq
            {
                if failed {
                    search.result = Load::Failed;
                } else {
                    let result = app_data::parse_search(&body);
                    if search.category.is_none() {
                        search.counts = result.counts.clone();
                        search.all_total = result.total;
                    }
                    images = result.apps.clone();
                    search.result = Load::Ready(result);
                }
            }
            self.app_images(&images);
        } else if let Some(id) = key.strip_prefix("discover:apps:assets:") {
            let Ok(id) = id.parse::<u64>() else { return };
            let url = if failed {
                None
            } else {
                app_data::cover_url(id, &body)
            };
            if let Some(url) = &url {
                self.discover_image(url);
            }
            self.discover.apps.covers.insert(id, url);
        } else if let Some(id) = key.strip_prefix("discover:app:similar:") {
            let Ok(id) = id.parse::<u64>() else { return };
            if !failed {
                let apps: Vec<App> = body["applications"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(app_data::parse_app)
                    .collect();
                self.app_images(&apps);
                self.discover.apps.similar.insert(id, apps);
            }
        } else if let Some(id) = key.strip_prefix("discover:app:") {
            let Ok(id) = id.parse::<u64>() else { return };
            if failed {
                self.discover.apps.details.insert(id, Load::Failed);
                self.discover_forget(key);
            } else {
                if let Some(app) = app_data::parse_app(&body) {
                    self.app_images(&[app]);
                }
                if let Some(items) = body["directory_entry"]["carousel_items"].as_array() {
                    for item in items {
                        if let Some(url) = carousel_image(item) {
                            self.discover_image(&url);
                        }
                    }
                }
                self.discover.apps.details.insert(id, Load::Ready(body));
            }
        }
    }

    /// Asks for what a card draws: the icon, and the cover (the `embedded_cover` asset of an embedded app, else the bot
    /// banner).
    fn app_images(&mut self, apps: &[App]) {
        for app in apps {
            if let Some(url) = app.icon_url(96) {
                self.discover_image(&url);
            }
            if app.embedded {
                self.discover_request(
                    format!("discover:apps:assets:{}", app.id),
                    app_data::assets_route(app.id),
                );
            }
            if let Some(url) = app.banner_url() {
                self.discover_image(&url);
            }
        }
    }

    // ---- search ----

    pub(super) fn apps_search(&mut self, text: &str, cx: &mut Context<Self>) {
        self.discover.apps.search = Some(AppSearch {
            query: text.to_owned(),
            page: 1,
            ..AppSearch::default()
        });
        self.discover.scroll.set_offset(point(px(0.), px(0.)));
        self.apps_search_request();
        cx.notify();
    }

    fn apps_search_request(&mut self) {
        self.discover.apps.seq += 1;
        let seq = self.discover.apps.seq;
        let Some(search) = self.discover.apps.search.as_mut() else {
            return;
        };
        search.seq = seq;
        search.result = Load::Loading;
        let route = app_data::search_route(&search.query, search.category, search.page);
        let _ = self.requests.send(Request::Fetch {
            key: format!("discover:apps:search:{seq}"),
            route,
        });
    }

    pub(super) fn apps_exit_search(&mut self) {
        self.discover.apps.search = None;
        self.discover.apps.seq += 1;
    }

    pub(super) fn apps_retry(&mut self) {
        match self.discover.place {
            Place::App(id) => {
                self.discover.apps.details.remove(&id);
                self.app_request(id);
            }
            _ => {
                self.discover.apps.collections = Load::Idle;
                self.discover
                    .apps
                    .listing
                    .retain(|_, load| *load != Load::Failed);
                self.apps_load_tab();
            }
        }
    }

    pub(super) fn apps_results_title(&self) -> String {
        let query = self
            .discover
            .apps
            .search
            .as_ref()
            .map(|search| search.query.clone())
            .unwrap_or_default();
        format!("Search results for \"{query}\"")
    }

    pub(super) fn apps_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.discover.apps.tab;
        let mut tabs: Vec<(i64, String)> = vec![(0, "Featured".to_owned())];
        tabs.extend(
            self.discover
                .apps
                .categories
                .iter()
                .map(|category| (category.id, category.name.clone())),
        );
        let mut row = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .pt(px(2.))
            .flex()
            .items_center()
            .overflow_hidden();
        for (index, (id, label)) in tabs.into_iter().enumerate() {
            row = row.child(
                self.discover_tab(
                    SharedString::from(format!("discover-apps-tab-{id}")),
                    &label,
                    id == current,
                    index == 0,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.discover.apps.tab = id;
                    this.discover.apps.page = 1;
                    this.discover.apps.search = None;
                    this.discover.scroll.set_offset(point(px(0.), px(0.)));
                    this.apps_load_tab();
                    cx.notify();
                })),
            );
        }
        row.into_any_element()
    }

    // ---- cards ----

    /// `ef`: a card with the cover at 16:9, the 56px icon overlapping it, the name, the category and the description.
    fn app_card(
        &self,
        app: &App,
        card_width: f32,
        show_category: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = app.id;
        let cover = if app.embedded {
            self.discover
                .apps
                .covers
                .get(&id)
                .cloned()
                .flatten()
                .and_then(|url| self.images.get(&format!("discover:img:{url}")).cloned())
        } else {
            None
        }
        .or_else(|| {
            app.banner_url()
                .and_then(|url| self.images.get(&format!("discover:img:{url}")).cloned())
        });
        let icon_image = app
            .icon_url(96)
            .and_then(|url| self.images.get(&format!("discover:img:{url}")).cloned());
        let banner_height = (card_width - 2.) * 9. / 16.;
        // Home, category and search cards all carry the category line, so the description keeps two lines.
        let _ = show_category;
        let sub = true;
        div()
            .h(px(banner_height + 28. + 106. + 2. - 2.))
            .child(
                div()
                    .id(SharedString::from(format!("discover-app-{id}")))
                    .w_full()
                    .h(px(banner_height + 28. + 106. + 2.))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-low"))
                    .cursor_pointer()
                    .hover(|style| {
                        style
                            .bg(color("--background-surface-high"))
                            .border_color(color("--border-strong"))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_discover(Place::App(id), cx);
                    }))
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .w_full()
                            .h(px(banner_height + 28.))
                            .child(
                                div()
                                    .w_full()
                                    .h(px(banner_height))
                                    .overflow_hidden()
                                    .children(cover.map(|image| {
                                        img(image).size_full().object_fit(ObjectFit::Cover)
                                    })),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(px(12.))
                                    .top(px(banner_height - 28.))
                                    .size(px(56.))
                                    .rounded(px(12.))
                                    .border(px(4.))
                                    .border_color(color("--background-base-low"))
                                    .bg(color("--background-base-lowest"))
                                    .overflow_hidden()
                                    .children(icon_image.map(|image| {
                                        img(image).size_full().object_fit(ObjectFit::Cover)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .h(px(106.))
                            .pl(px(16.))
                            .pr(px(16.))
                            .pt(px(4.))
                            .flex()
                            .flex_col()
                            .child(
                                text(16., 20., 600., "--text-strong")
                                    .flex_none()
                                    .truncate()
                                    .child(app.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .mt(px(4.))
                                    .h(px(16.))
                                    .flex()
                                    .gap(px(4.))
                                    .when_some(app.category.clone(), |row, name| {
                                        row.child(
                                            text(12., 16., 500., "--text-default").child(name),
                                        )
                                    })
                                    .when(app.promoted && app.category.is_some(), |row| {
                                        row.child(
                                            text(12., 16., 500., "--text-muted").child("\u{2022}"),
                                        )
                                    })
                                    .when(app.promoted, |row| {
                                        row.child(
                                            text(12., 16., 500., "--text-muted").child("Promoted"),
                                        )
                                    }),
                            )
                            .child(
                                text(14., 18., 500., "--text-default")
                                    .mt(px(8.))
                                    .max_h(px(if sub { 36. } else { 54. }))
                                    .overflow_hidden()
                                    .line_clamp(if sub { 2 } else { 3 })
                                    .text_ellipsis()
                                    .child(app.description.clone()),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn app_grid(
        &self,
        apps: &[App],
        width: f32,
        show_category: bool,
        cx: &mut Context<Self>,
        viewport: f32,
    ) -> AnyElement {
        let columns = columns_for(viewport);
        let inner = width - 64.;
        let card_width = (inner - GAP * (columns as f32 - 1.)) / columns as f32;
        div()
            .parity("discover-app-grid")
            .w_full()
            .px(px(32.))
            .py(px(16.))
            .grid()
            .grid_cols(columns as u16)
            .gap(px(GAP))
            .children(
                apps.iter()
                    .map(|app| self.app_card(app, card_width, show_category, cx)),
            )
            .into_any_element()
    }

    fn app_heading(&self, title: &str, first: bool) -> AnyElement {
        text(20., 24., 600., "--text-strong")
            .flex_none()
            .pl(px(32.))
            .pt(px(if first { 24. } else { 16. }))
            .h(px(if first { 48. } else { 40. }))
            .child(title.to_owned())
            .into_any_element()
    }

    /// `Have Questions?`: the help-centre card closing the home page.
    fn app_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .pl(px(32.))
            .pr(px(32.))
            .pb(px(32.))
            .child(
                div()
                    .w_full()
                    .h(px(72.))
                    .rounded(px(16.))
                    .bg(gpui::rgba(0x97979f14))
                    .pl(px(16.))
                    .pr(px(16.))
                    .pt(px(16.))
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(text(20., 24., 600., "--text-strong").child("Have Questions?"))
                            .child(
                                text(12., 16., 400., "--text-default").child(
                                    "Visit our help center to learn more about using and discovering apps on Discord.",
                                ),
                            ),
                    )
                    .child(self.app_button("discover-learn-more", "Learn More", Some(40.), true, cx).on_click(
                        cx.listener(|_, _, _, cx| cx.open_url(LEARN_MORE)),
                    )),
            )
            .into_any_element()
    }

    /// `button_F7qvhY`: a filled brand button or the subtle grey one, 40 or 32px tall.
    fn app_button(
        &self,
        id: &'static str,
        label: &str,
        height: Option<f32>,
        primary: bool,
        _cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let height = height.unwrap_or(32.);
        let large = height >= 40.;
        div()
            .id(id)
            .flex_none()
            .h(px(height))
            .px(px(if large { 16. } else { 12. }))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(if primary {
                color("--brand-500")
            } else {
                color("--background-mod-subtle")
            })
            .hover(|style| {
                style.bg(if primary {
                    color("--brand-530")
                } else {
                    color("--background-mod-normal")
                })
            })
            .child(
                text(
                    if large { 16. } else { 14. },
                    if large { 20. } else { 18. },
                    500.,
                    if primary { "--white" } else { "--text-strong" },
                )
                .child(label.to_owned()),
            )
    }

    fn app_icon_button(
        &self,
        id: &'static str,
        glyph: &'static str,
        size: f32,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .flex_none()
            .size(px(size))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .hover(|style| style.bg(color("--background-mod-normal")))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(icon(
                glyph,
                if size >= 40. { 20. } else { 16. },
                "--text-strong",
            ))
    }

    // ---- the pages ----

    pub(super) fn apps_body(
        &mut self,
        width: f32,
        _offset: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let viewport = width + 375. + 8.;
        if self.discover.apps.search.is_some() {
            return self.apps_results(width, viewport, cx);
        }
        let tab = self.discover.apps.tab;
        let mut page = div().w_full().flex().flex_col();
        if tab == 0 {
            page = page.child(self.discover_hero_split(
                ["Find things to", "do together"],
                "Bring the fun to your hangouts with apps for games, music, creative tools, and more.",
                width + 8.,
            ));
            match self.discover.apps.collections.clone() {
                Load::Ready(collections) => {
                    for (index, collection) in collections.iter().enumerate() {
                        page = page
                            .child(self.app_heading(&collection.title, index == 0))
                            .child(self.app_grid(&collection.apps, width, false, cx, viewport));
                    }
                    page = page.child(div().h(px(16.))).child(self.app_footer(cx));
                }
                Load::Failed => {
                    page = page.child(self.discover_message(
                        "Uh oh, something went wrong",
                        "Try again later",
                        Some("discover-apps-retry"),
                        cx,
                    ));
                }
                _ => {}
            }
        } else {
            let category = self
                .discover
                .apps
                .categories
                .iter()
                .find(|category| category.id == tab)
                .map(|category| category.name.clone())
                .unwrap_or_default();
            page = page.child(self.discover_hero(&category, category_description(tab), width + 8.));
            let current = self.discover.apps.page;
            match self
                .discover
                .apps
                .listing
                .get(&(tab, current))
                .cloned()
                .unwrap_or_default()
            {
                Load::Ready(result) => {
                    page = page
                        .child(self.app_grid(&result.apps, width, true, cx, viewport))
                        .child(self.app_pages(result.pages, current, cx));
                }
                Load::Failed => {
                    page = page.child(self.discover_message(
                        "Uh oh, something went wrong",
                        "Try again later",
                        Some("discover-apps-retry"),
                        cx,
                    ));
                }
                _ => {}
            }
        }
        page.into_any_element()
    }

    /// The Apps hero: two title lines, each its own heading.
    fn discover_hero_split(&self, lines: [&str; 2], description: &str, width: f32) -> AnyElement {
        self.discover_hero(&lines.join("\n"), description, width)
    }

    /// Page buttons under a paginated grid: previous, up to seven numbers around the current page, next.
    fn app_pages(&self, pages: u32, current: u32, cx: &mut Context<Self>) -> AnyElement {
        if pages <= 1 {
            return div().into_any_element();
        }
        let first = current.saturating_sub(3).max(1);
        let last = (first + 6).min(pages);
        let mut row = div()
            .w_full()
            .pb(px(32.))
            .flex()
            .justify_center()
            .gap(px(8.));
        let button = |id: SharedString, label: String, target: u32, active: bool| {
            text(
                14.,
                18.,
                600.,
                if active {
                    "--text-strong"
                } else {
                    "--text-muted"
                },
            )
            .id(id)
            .min_w(px(32.))
            .h(px(32.))
            .px(px(8.))
            .rounded(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(active, |this| this.bg(color("--background-mod-normal")))
            .hover(|style| style.bg(color("--background-mod-subtle")))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.apps_goto_page(target);
                cx.notify();
            }))
        };
        if current > 1 {
            row = row.child(button(
                "discover-page-prev".into(),
                "Previous".into(),
                current - 1,
                false,
            ));
        }
        for number in first..=last {
            row = row.child(button(
                SharedString::from(format!("discover-page-{number}")),
                number.to_string(),
                number,
                number == current,
            ));
        }
        if current < pages {
            row = row.child(button(
                "discover-page-next".into(),
                "Next".into(),
                current + 1,
                false,
            ));
        }
        row.into_any_element()
    }

    fn apps_goto_page(&mut self, page: u32) {
        self.discover.scroll.set_offset(point(px(0.), px(0.)));
        if let Some(search) = self.discover.apps.search.as_mut() {
            search.page = page;
            self.apps_search_request();
        } else {
            self.discover.apps.page = page;
            self.apps_load_tab();
        }
    }

    /// The search results (`nz`): the category counts at the left, the grid and the page buttons.
    fn apps_results(&mut self, width: f32, viewport: f32, cx: &mut Context<Self>) -> AnyElement {
        let Some(search) = self.discover.apps.search.clone() else {
            return div().into_any_element();
        };
        let mut rows: Vec<(Option<i64>, String, u64)> =
            vec![(None, "All".to_owned(), search.all_total)];
        for (id, count) in &search.counts {
            let name = self
                .discover
                .apps
                .categories
                .iter()
                .find(|category| category.id == *id)
                .map(|category| category.name.clone())
                .unwrap_or_default();
            rows.push((Some(*id), name, *count));
        }
        let column = div()
            .flex_none()
            .w(px(200.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(rows.into_iter().map(|(id, name, count)| {
                let selected = id == search.category;
                div()
                    .id(SharedString::from(format!(
                        "discover-apps-cat-{}",
                        id.unwrap_or(0)
                    )))
                    .h(px(40.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .when(selected, |this| this.bg(color("--background-mod-normal")))
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .child(
                        text(
                            14.,
                            18.,
                            500.,
                            if selected {
                                "--text-strong"
                            } else {
                                "--text-muted"
                            },
                        )
                        .child(name),
                    )
                    .child(text(12., 16., 500., "--text-muted").child(data::group_digits(count)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(search) = this.discover.apps.search.as_mut() {
                            search.category = id;
                            search.page = 1;
                        }
                        this.discover.scroll.set_offset(point(px(0.), px(0.)));
                        this.apps_search_request();
                        cx.notify();
                    }))
            }));
        let body: AnyElement = match &search.result {
            Load::Ready(result) if result.apps.is_empty() => {
                let heading = match search.category.and_then(|id| {
                    self.discover
                        .apps
                        .categories
                        .iter()
                        .find(|category| category.id == id)
                }) {
                    Some(category) => format!("No results found in {}", category.name),
                    None => "No results found".to_owned(),
                };
                self.discover_message(&heading, "Try searching for something else.", None, cx)
            }
            Load::Ready(result) => div()
                .w_full()
                .child(self.app_grid(&result.apps, width - 232., true, cx, viewport - 232.))
                .child(self.app_pages(result.pages, search.page, cx))
                .into_any_element(),
            Load::Failed => {
                self.discover_message("Uh oh, something went wrong", "Try again later", None, cx)
            }
            _ => div().into_any_element(),
        };
        div()
            .w_full()
            .pt(px(24.))
            .pl(px(32.))
            .flex()
            .child(column)
            .child(div().flex_1().min_w(px(0.)).child(body))
            .into_any_element()
    }

    // ---- an application's page ----

    pub(super) fn app_detail_header(
        &self,
        row: gpui::Div,
        offset: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let id = match self.discover.place {
            Place::App(id) => id,
            _ => 0,
        };
        let detail = self.discover.apps.details.get(&id).and_then(Load::ready);
        let name = detail
            .and_then(|body| body["name"].as_str())
            .unwrap_or_default()
            .to_owned();
        // The compact buttons replace the page's own once those scroll out of view.
        let show = detail.is_some() && offset > 311.;
        let row = row.bg(color("--background-base-low"));
        row.child(
            div().flex_1().flex().child(
                div()
                    .id("discover-back")
                    .flex_none()
                    .size(px(32.))
                    .rounded(px(4.))
                    .bg(color("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_discover(Place::Apps, cx);
                    }))
                    .child(icon("lb_arrow_left", 18., "--text-strong")),
            ),
        )
        .child(
            text(20., 24., 600., "--text-strong")
                .flex_none()
                .max_w(px(300.))
                .truncate()
                .child(name),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .justify_end()
                .gap(px(8.))
                .when(show, |this| {
                    this.children(
                        detail
                            .and_then(app_data::parse_app)
                            .filter(|app| app.embedded)
                            .map(|_| {
                                self.app_button("discover-head-launch", "Launch", None, true, cx)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.launch_activity(&id.to_string(), cx)
                                    }))
                            }),
                    )
                    .child(
                        self.app_button("discover-head-add", "Add App", None, false, cx)
                            .on_click(cx.listener(move |this, _, _, cx| this.app_add(id, cx))),
                    )
                    .child(
                        self.app_icon_button("discover-head-link", "discover_link", 32.)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(app_link(id)));
                            })),
                    )
                    .child(
                        self.app_icon_button("discover-head-more", "discover_more", 32.)
                            .relative()
                            .child(self.anchor_probe("discover-head-more"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.app_more_menu("discover-head-more", id, cx)
                            })),
                    )
                }),
        )
    }

    /// The "..." button: the menu opens to the left of the button with its top edge on the button's (module 586193
    /// `l4`: `position: left, align: top`, 8px apart).
    fn app_more_menu(&mut self, key: &'static str, id: u64, cx: &mut Context<Self>) {
        let bounds = self.tip_anchors.borrow().get(key).copied();
        if let Some(bounds) = bounds {
            self.open_context_menu(
                super::super::context_menu::Target::AppDirectory(id),
                gpui::point(bounds.left() - px(8.), bounds.top()),
                cx,
            );
        }
    }

    /// The Add App button: starts the application's install through the OAuth consent flow (`oauth_begin`).
    fn app_add(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(Load::Ready(body)) = self.discover.apps.details.get(&id) else {
            return;
        };
        let scopes = body["install_params"]["scopes"].clone();
        let permissions = body["install_params"]["permissions"].clone();
        let args = json!({
            "client_id": id.to_string(),
            "scopes": if scopes.is_array() { scopes } else { json!(["applications.commands"]) },
            "permissions": permissions,
        });
        let nonce = format!("discover-add-{id}");
        self.oauth_begin(nonce, &args, cx);
    }

    pub(super) fn app_detail_body(
        &mut self,
        id: u64,
        width: f32,
        _offset: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let load = self
            .discover
            .apps
            .details
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let body = match load {
            Load::Ready(body) => body,
            Load::Failed => {
                return self.discover_message(
                    "Uh oh, something went wrong",
                    "Try again later",
                    Some("discover-app-retry"),
                    cx,
                );
            }
            _ => return div().into_any_element(),
        };
        let Some(app) = app_data::parse_app(&body) else {
            return div().into_any_element();
        };
        let icon_image = app
            .icon_url(96)
            .and_then(|url| self.images.get(&format!("discover:img:{url}")).cloned());
        let banner = app
            .banner_url()
            .and_then(|url| self.images.get(&format!("discover:img:{url}")).cloned());
        let detail_w = width.min(1248.);
        let main_w = (detail_w - 352.).max(320.);
        let banner_height = detail_w * 72. / 325.;
        // `eC` (module 586193): without a banner picture the header is `linear-gradient(45deg, first, second)`, the
        // icon's main color and the first of its complements.
        let gradient = if banner.is_none() && icon_image.is_some() {
            let colors = self.discover.apps.banner_colors.get(&id).copied();
            colors.or_else(|| {
                let picture = icon_image.as_ref()?;
                let decoded = image::load_from_memory(picture.bytes()).ok()?;
                let first = crate::palette::avatar_palette(decoded.to_rgba8().as_raw())
                    .first()
                    .copied()?;
                let pair = banner_gradient_colors(first);
                self.discover.apps.banner_colors.insert(id, pair);
                Some(pair)
            })
        } else {
            None
        };
        let entry = &body["directory_entry"];
        let items: Vec<String> = entry["carousel_items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(carousel_image)
            .collect();
        let current = self
            .discover
            .apps
            .carousel
            .min(items.len().saturating_sub(1));
        let carousel_image_handle = items
            .get(current)
            .and_then(|url| self.images.get(&format!("discover:img:{url}")).cloned());
        let total = items.len();
        let overview = body["directory_entry"]["detailed_description"]
            .as_str()
            .filter(|text| !text.is_empty())
            .or_else(|| body["description"].as_str())
            .unwrap_or_default()
            .to_owned();
        let long = overview.chars().count() > 600;
        let open = self.discover.apps.overview_open;
        let paragraphs = self.app_markdown(&overview, cx);
        let commands = app_data::popular_commands(&body);
        let permissions = app_data::install_permissions(&body);
        let data_intents = app_data::intents(&body);
        let privacy_url = body["privacy_policy_url"]
            .as_str()
            .filter(|url| !url.is_empty())
            .map(str::to_owned);
        let languages: Vec<String> = entry["supported_locales"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|code| code.as_str())
            .map(|code| {
                if code == "en-US" {
                    "English, US".to_owned()
                } else {
                    data::LANGUAGES
                        .iter()
                        .find(|(known, _)| *known == code)
                        .map_or_else(|| code.to_owned(), |(_, name)| (*name).to_owned())
                }
            })
            .collect();
        let section = |title: &str| text(14., 18., 600., "--text-strong").child(title.to_owned());
        let sidebar =
            div()
                .flex_none()
                .w(px(240.))
                .flex()
                .flex_col()
                .gap(px(32.))
                .when_some(entry["guild_count"].as_u64(), |this, count| {
                    this.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(section("Server Count"))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(icon("discover_servers_count", 16., "--text-muted"))
                                    .child(text(14., 18., 400., "--text-default").child(format!(
                                        "in {} servers",
                                        app_data::short_count(count)
                                    ))),
                            ),
                    )
                })
                .when(
                    body["categories"]
                        .as_array()
                        .is_some_and(|rows| !rows.is_empty()),
                    |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .child(section("Categories"))
                                .child(div().flex().gap(px(8.)).children(
                                    body["categories"].as_array().into_iter().flatten().map(
                                        |row| {
                                            text(14., 18., 600., "--text-default")
                                                .h(px(32.))
                                                .px(px(12.))
                                                .rounded(px(4.))
                                                .border_1()
                                                .border_color(color("--border-strong"))
                                                .flex()
                                                .items_center()
                                                .child(
                                                    row["name"]
                                                        .as_str()
                                                        .unwrap_or_default()
                                                        .to_owned(),
                                                )
                                        },
                                    ),
                                )),
                        )
                    },
                )
                .when(!languages.is_empty(), |this| {
                    this.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(section("Supported Languages"))
                            .child(
                                text(14., 18., 600., "--text-default").child(languages.join(", ")),
                            ),
                    )
                })
                .child({
                    let links: Vec<(&'static str, String)> = [
                        ("Terms of Service", body["terms_of_service_url"].as_str()),
                        ("Privacy Policy", body["privacy_policy_url"].as_str()),
                    ]
                    .into_iter()
                    .filter_map(|(label, url)| {
                        Some((label, url.filter(|url| !url.is_empty())?.to_owned()))
                    })
                    .collect();
                    div().when(!links.is_empty(), |this| {
                        this.flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(section("Links"))
                            .children(links.into_iter().map(|(label, url)| {
                                div()
                                    .id(label)
                                    .flex()
                                    .gap(px(8.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
                                    .child(icon("discover_doc", 20., "--text-muted"))
                                    .child(text(14., 18., 500., "--text-default").child(label))
                            }))
                    })
                });
        let carousel = div().w_full().flex().flex_col().when(total > 0, |this| {
            this.child(
                div()
                    .w_full()
                    .h(px(main_w * 9. / 16.))
                    .rounded(px(5.))
                    .overflow_hidden()
                    .bg(color("--background-base-lowest"))
                    .children(
                        carousel_image_handle
                            .map(|image| img(image).size_full().object_fit(ObjectFit::Contain)),
                    ),
            )
            .child(
                div()
                    .h(px(64.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().px(px(12.)).child(
                        carousel_arrow("discover-carousel-prev", "discover_arrow_left").on_click(
                            cx.listener(move |this, _, _, cx| {
                                let apps = &mut this.discover.apps;
                                apps.carousel = (apps.carousel + total - 1) % total;
                                cx.notify();
                            }),
                        ),
                    ))
                    .child(div().flex().gap(px(8.)).children((0..total).map(|index| {
                        div()
                            .id(("discover-carousel-dot", index))
                            .size(px(8.))
                            .rounded_full()
                            .bg(color("--white"))
                            .cursor_pointer()
                            .opacity(if index == current { 1. } else { 0.6 })
                            .when(index != current, |dot| {
                                dot.hover(|style| style.opacity(0.8))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.discover.apps.carousel = index;
                                cx.notify();
                            }))
                    })))
                    .child(div().px(px(12.)).child(
                        carousel_arrow("discover-carousel-next", "discover_arrow_right").on_click(
                            cx.listener(move |this, _, _, cx| {
                                let apps = &mut this.discover.apps;
                                apps.carousel = (apps.carousel + 1) % total;
                                cx.notify();
                            }),
                        ),
                    )),
            )
        });
        let similar = self
            .discover
            .apps
            .similar
            .get(&id)
            .cloned()
            .unwrap_or_default();
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .h(px(banner_height))
                    .flex_none()
                    .overflow_hidden()
                    .bg(color("--background-mod-normal"))
                    .when_some(gradient, |this, (from, to)| {
                        let stop = |rgb: [u8; 3]| {
                            gpui::rgb(
                                (u32::from(rgb[0]) << 16)
                                    | (u32::from(rgb[1]) << 8)
                                    | u32::from(rgb[2]),
                            )
                        };
                        this.bg(gpui::linear_gradient(
                            45.,
                            gpui::linear_color_stop(stop(from), 0.),
                            gpui::linear_color_stop(stop(to), 1.),
                        ))
                    })
                    .children(
                        banner.map(|image| img(image).size_full().object_fit(ObjectFit::Cover)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(1248.))
                            .flex()
                            .flex_col()
                            .child(
                                div().h(px(52.)).flex_none().pl(px(28.)).child(
                                    div()
                                        .mt(px(-52.))
                                        .size(px(104.))
                                        .rounded(px(24.))
                                        .border(px(4.))
                                        .border_color(color("--background-base-low"))
                                        .bg(color("--background-base-low"))
                                        .overflow_hidden()
                                        .children(icon_image.map(|image| {
                                            img(image).size_full().object_fit(ObjectFit::Cover)
                                        })),
                                ),
                            )
                            .child(
                                div()
                                    .pl(px(32.))
                                    .pr(px(32.))
                                    .pt(px(20.))
                                    .h(px(80.))
                                    .flex_none()
                                    .flex()
                                    .justify_between()
                                    .child(
                                        text(32., 40., 600., "--text-strong")
                                            .flex_1()
                                            .min_w(px(0.))
                                            .mr(px(16.))
                                            .truncate()
                                            .child(app.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .flex()
                                            .gap(px(8.))
                                            .when(app.embedded, |this| {
                                                this.child(
                                                    self.app_button(
                                                        "discover-launch",
                                                        "Launch",
                                                        Some(40.),
                                                        true,
                                                        cx,
                                                    )
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.launch_activity(
                                                                &id.to_string(),
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                                )
                                            })
                                            .child(
                                                self.app_button(
                                                    "discover-add",
                                                    "Add App",
                                                    Some(40.),
                                                    false,
                                                    cx,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.app_add(id, cx)
                                                })),
                                            )
                                            .child(
                                                self.app_icon_button(
                                                    "discover-link",
                                                    "discover_link",
                                                    40.,
                                                )
                                                .on_click(cx.listener(move |_, _, _, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(app_link(id)),
                                                    );
                                                })),
                                            )
                                            .child(
                                                self.app_icon_button(
                                                    "discover-more",
                                                    "discover_more",
                                                    40.,
                                                )
                                                .relative()
                                                .child(self.anchor_probe("discover-more"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.app_more_menu("discover-more", id, cx)
                                                })),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .pl(px(32.))
                                    .pr(px(32.))
                                    .pb(px(32.))
                                    .flex()
                                    .gap(px(48.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(32.))
                                            .child(carousel)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(text(20., 24., 600., "--text-strong").child("Overview"))
                                    .child(
                                        div()
                                            .id("discover-overview")
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.))
                                            .when(long && !open, |this| {
                                                this.max_h(px(272.)).overflow_hidden()
                                            })
                                            .child(paragraphs),
                                    )
                                    .when(long, |this| {
                                        this.child(
                                            div()
                                                .id("discover-show-more")
                                                .w_full()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .gap(px(8.))
                                                .cursor_pointer()
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .h(px(1.))
                                                        .bg(gpui::rgba(0x97979f70)),
                                                )
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.discover.apps.overview_open =
                                                        !this.discover.apps.overview_open;
                                                    cx.notify();
                                                }))
                                                .child(text(16., 20., 600., "--text-muted").child(
                                                    if open { "Show less" } else { "Show more" },
                                                ))
                                                .child(icon(
                                                    "discover_show_more",
                                                    18.,
                                                    "--text-default",
                                                ))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .h(px(1.))
                                                        .bg(gpui::rgba(0x97979f70)),
                                                ),
                                        )
                                    }),
                            )
                            .when(!commands.is_empty(), |this| {
                                this.child(app_section(
                                    "Popular Slash Commands",
                                    div().flex().flex_col().gap(px(16.)).children(
                                        commands.iter().map(|(name, description)| {
                                            div()
                                                .flex()
                                                .items_baseline()
                                                .gap(px(16.))
                                                .child(
                                                    text(14., 18.667, 400., "--text-strong")
                                                        .flex_none()
                                                        .px(px(4.))
                                                        .py(px(2.))
                                                        .rounded(px(4.))
                                                        .border_1()
                                                        .border_color(color("--border-subtle"))
                                                        .bg(color("--background-base-lowest"))
                                                        .font_family(crate::ui::MONO)
                                                        .child(format!("/{name}")),
                                                )
                                                .child(
                                                    text(16., 20., 400., "--text-default")
                                                        .min_w(px(0.))
                                                        .child(description.clone()),
                                                )
                                        }),
                                    ),
                                ))
                            })
                            .when(!permissions.is_empty(), |this| {
                                this.child(app_section(
                                    "Permissions",
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(12.))
                                        .child(
                                            text(12., 16., 600., "--text-strong")
                                                .child("Required Permissions"),
                                        )
                                        .child(div().grid().grid_cols(3).gap(px(10.)).children(
                                            permissions.iter().map(|label| {
                                                div()
                                                    .flex()
                                                    .child(icon(
                                                        "discover_permission_check",
                                                        16.,
                                                        "--status-positive",
                                                    ))
                                                    .child(
                                                        text(14., 18., 400., "--text-default")
                                                            .child(*label),
                                                    )
                                            }),
                                        )),
                                ))
                            })
                            .child(self.app_data_access(data_intents, privacy_url, cx))
                            .when(!similar.is_empty(), |this| {
                                this.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(16.))
                                        .child(
                                            text(14., 18., 600., "--text-strong")
                                                .child("You Might Also Like"),
                                        )
                                        .child(
                                            div().grid().grid_cols(2).gap(px(16.)).children(
                                                similar
                                                    .iter()
                                                    .take(4)
                                                    .map(|app| self.similar_card(app, cx)),
                                            ),
                                        ),
                                )
                            }),
                    )
                    .child(sidebar),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// Markdown at the body size of the page (16/20): paragraphs, headings, list rows, bold, links followed.
    fn app_markdown(&self, source: &str, cx: &mut Context<Self>) -> gpui::Div {
        let blocks = dc_markdown::parse(source);
        let tint: gpui::Hsla = color("--text-default").into();
        div().flex().flex_col().gap(px(8.)).children(
            blocks
                .iter()
                .filter(|block| !crate::ui::block_text(block).trim().is_empty())
                .map(|block| {
                    let spans = crate::ui::block_spans(block).to_vec();
                    let weight = if matches!(block, dc_markdown::Block::Heading { .. }) {
                        600.
                    } else {
                        400.
                    };
                    let pieces = spans
                        .iter()
                        .map(|span| self.piece(span, tint, 16., weight, 20.))
                        .collect();
                    let links = crate::ui::link_runs(&spans);
                    let root = cx.weak_entity();
                    let line = crate::inline::inline(pieces, px(20.)).on_piece_click(
                        move |piece, _, cx| {
                            if let Some(Some((url, label))) = links.get(piece).cloned() {
                                let _ =
                                    root.update(cx, |this, cx| this.follow_link(&url, &label, cx));
                            }
                        },
                    );
                    match block {
                        dc_markdown::Block::ListItem { depth, number, .. } => div()
                            .flex()
                            .pl(px(26. + f32::from(*depth) * 18.))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(18.))
                                    .child(match number {
                                        Some(number) => format!("{number}."),
                                        None => "\u{2022}".to_owned(),
                                    })
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default")),
                            )
                            .child(div().flex_1().min_w(px(0.)).child(line))
                            .into_any_element(),
                        _ => div().child(line).into_any_element(),
                    }
                }),
        )
    }

    /// Data Access (module 586193 `tD`): the privileged intents the application holds, then the baseline note and the
    /// developer's privacy policy.
    fn app_data_access(
        &self,
        intents: app_data::Intents,
        privacy: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = |glyph: &'static str, heading: &'static str, body: &'static str| {
            div()
                .flex()
                .gap(px(16.))
                .text_color(color("--text-default"))
                .child(icon(glyph, 24., "--text-default"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(text(14., 18., 600., "--text-default").child(heading))
                        .child(text(16., 20., 400., "--text-default").child(body)),
                )
        };
        let mut content = div().flex().flex_col();
        if intents.any() {
            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .pb(px(16.))
                    .mb(px(16.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .child(
                        text(14., 18., 600., "--text-default")
                            .child("This app can access the following data in the server:"),
                    )
                    .when(intents.message_content, |this| {
                        this.child(row(
                            "discover_intent_chat",
                            "Read Messages",
                            "Can access messages in the server and read their contents.",
                        ))
                    })
                    .when(intents.presences, |this| {
                        this.child(row(
                            "discover_intent_presence",
                            "Presence",
                            "Can see online status of server members and what they are playing, watching, or listening to.",
                        ))
                    })
                    .when(intents.members, |this| {
                        this.child(row(
                            "discover_intent_members",
                            "Members",
                            "Can see who's in the server and their roles, nicknames, and permission changes.",
                        ))
                    }),
            );
        }
        content = content.child(self.app_markdown(
            &format!(
                "All apps can access a set of baseline data. Please visit the [Help Center]({DATA_ACCESS_HELP}) to learn more."
            ),
            cx,
        ));
        if let Some(url) = privacy {
            content = content.child(div().mt(px(8.)).child(self.app_markdown(
                &format!(
                    "For more information about this app's privacy practices, see the developer's [privacy policy]({url})."
                ),
                cx,
            )));
        }
        app_section("Data Access", content)
    }

    /// `appContainer_T6XJeD`: a related application, 48px icon, name, category and description.
    fn similar_card(&self, app: &App, cx: &mut Context<Self>) -> AnyElement {
        let id = app.id;
        let icon_image = app
            .icon_url(96)
            .and_then(|url| self.images.get(&format!("discover:img:{url}")).cloned());
        div()
            .id(SharedString::from(format!("discover-similar-{id}")))
            .p(px(16.))
            .rounded(px(8.))
            .bg(color("--background-base-lower"))
            .flex()
            .flex_col()
            .gap(px(16.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.open_discover(Place::App(id), cx)))
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(
                        div()
                            .size(px(48.))
                            .flex_none()
                            .rounded(px(12.))
                            .overflow_hidden()
                            .children(
                                icon_image.map(|image| {
                                    img(image).size_full().object_fit(ObjectFit::Cover)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                text(16., 20., 600., "--text-strong")
                                    .truncate()
                                    .child(app.name.clone()),
                            )
                            .children(
                                app.category
                                    .clone()
                                    .map(|name| text(12., 16., 500., "--text-muted").child(name)),
                            ),
                    ),
            )
            .child(
                text(14., 18., 500., "--text-default")
                    .max_h(px(36.))
                    .overflow_hidden()
                    .line_clamp(2)
                    .text_ellipsis()
                    .child(app.description.clone()),
            )
            .into_any_element()
    }
}

/// `https://discord.com/discovery/applications/{id}`: what the copy-link button puts on the clipboard.
pub(super) fn app_link(id: u64) -> String {
    format!("https://discord.com/discovery/applications/{id}")
}

/// `getArticleURL(BOT_DATA_ACCESS)` (modules 652215, 975571).
const DATA_ACCESS_HELP: &str = "https://support.discord.com/hc/en-us/articles/7933951485975";

/// A page section with its 20/24 heading over the framed box (`sectionContainer` / `contentContainer`).
fn app_section(title: &'static str, content: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(text(20., 24., 600., "--text-strong").child(title))
        .child(
            div()
                .p(px(16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lower"))
                .child(content),
        )
        .into_any_element()
}

/// The two ends of the header gradient: the icon's main color and the first complement of it.
fn banner_gradient_colors(first: [u8; 3]) -> ([u8; 3], [u8; 3]) {
    (first, crate::palette::complementary_colors(first)[0])
}

/// `button_F7qvhY` icon-only in the carousel's pagination: 40px, no fill, the icon in the icon-only grey.
fn carousel_arrow(id: &'static str, glyph: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(40.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(color("--control-icon-only-background-hover")))
        .group(id)
        .child(
            gpui::svg()
                .path(format!("icons/{glyph}.svg"))
                .size(px(20.))
                .flex_none()
                .text_color(color("--control-icon-only-icon-default"))
                .group_hover(id, |style| {
                    style.text_color(color("--control-icon-only-icon-hover"))
                }),
        )
}

/// A carousel item that is a picture (`type` image); videos and embeds have no still here.
fn carousel_image(item: &Value) -> Option<String> {
    let url = item["url"].as_str()?;
    if url.contains("youtube.com") || url.contains("youtu.be") || item["type"].as_i64() == Some(2) {
        return None;
    }
    Some(url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_follow_the_viewport() {
        assert_eq!(columns_for(1280.), 4);
        assert_eq!(columns_for(1000.), 2);
        assert_eq!(columns_for(700.), 1);
    }

    #[test]
    fn carousel_skips_videos() {
        assert!(
            carousel_image(&json!({"type": 2, "url": "https://www.youtube.com/embed/x"})).is_none()
        );
        assert_eq!(
            carousel_image(&json!({"type": 1, "url": "https://x/y.png"})).as_deref(),
            Some("https://x/y.png")
        );
    }

    #[test]
    fn copied_link_names_the_application() {
        assert_eq!(app_link(7), "https://discord.com/discovery/applications/7");
    }
}
