//! The Servers page (module 300290): hero, category tabs, the card grid, the search results page with its Show filter and
//! language select, and the lurk request a card click sends (`joinGuild` with `lurker: true`, module 778858).
use super::data::{self, Category, GuildCard, SearchPage, ServersTab};
use super::{Load, Place, Root, SearchMode, text};
use crate::ui::motion::Transitions as _;
use crate::ui::{color, icon};
use crate::{net::Request, parity::Parity};
use gpui::{
    AnyElement, Context, IntoElement, ObjectFit, SharedString, div, img, point, prelude::*, px,
    rgba,
};
use std::collections::HashMap;

const DEFAULT_ART: &str = "https://discord.com/assets/b00e3f0a7090b61f.svg";
/// The grid gap and the page padding of `content_vA5YGD`.
const GAP: f32 = 16.;
const CARD_HEIGHT: f32 = 320.;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(in crate::ui) enum Menu {
    #[default]
    None,
    Show,
    Language,
}

/// A search results page: the query, the filters and what has arrived.
#[derive(Clone, Debug, Default)]
pub(in crate::ui) struct Results {
    pub query: String,
    pub category: i64,
    pub language: Option<String>,
    pub guilds: Vec<GuildCard>,
    pub total: u64,
    /// The All tab's total, kept from the unfiltered answer.
    pub all_total: u64,
    pub counts: Vec<(i64, u64)>,
    pub loading: bool,
    pub failed: bool,
    pub seq: u64,
}

#[derive(Default)]
pub(in crate::ui) struct Servers {
    pub tab: Option<ServersTab>,
    pub featured: Load<Vec<GuildCard>>,
    pub by_category: HashMap<i64, Load<Vec<GuildCard>>>,
    pub categories: Vec<Category>,
    pub results: Option<Results>,
    pub mode: SearchMode,
    pub menu: Menu,
    /// The language list's scroll, for its thumb.
    pub language_scroll: gpui::ScrollHandle,
    pub seq: u64,
    pub pending: Vec<u64>,
    /// Servers this run opened as a preview and has not left yet.
    pub lurked: Vec<u64>,
    /// The profile popout in view (the button's bounds and the server).
    pub popout: Option<super::profile::Popout>,
    /// What each profile request answered, and when it did.
    pub profiles: HashMap<u64, super::profile::Fetch>,
    pub profile_times: HashMap<u64, std::time::Instant>,
    /// The Student Hub dialog.
    pub hub: Option<super::hubs::HubDialog>,
}

impl Root {
    fn servers_tab(&self) -> ServersTab {
        self.discover.servers.tab.unwrap_or(ServersTab::Home)
    }

    pub(super) fn servers_enter(&mut self, _cx: &mut Context<Self>) {
        self.discover_image_default();
        self.servers_load_tab();
        self.discover_request(
            "discover:servers:categories".into(),
            data::categories_route(),
        );
    }

    fn discover_image_default(&mut self) {
        self.discover_image(DEFAULT_ART);
    }

    fn servers_load_tab(&mut self) {
        match self.servers_tab().category() {
            None => {
                if self.discover.servers.featured == Load::Idle {
                    self.discover.servers.featured = Load::Loading;
                }
                self.discover_request("discover:servers:featured".into(), data::featured_route());
            }
            Some(category) => {
                let entry = self
                    .discover
                    .servers
                    .by_category
                    .entry(category)
                    .or_default();
                if *entry == Load::Idle {
                    *entry = Load::Loading;
                }
                self.discover_request(
                    format!("discover:servers:cat:{category}"),
                    data::category_route(category),
                );
            }
        }
    }

    pub(super) fn servers_fetched(&mut self, key: &str) {
        let body = self.fetched.get(key).cloned().unwrap_or_default();
        let failed = crate::net::is_fetch_failure(&body);
        if key == "discover:servers:featured" {
            if failed {
                self.discover.servers.featured = Load::Failed;
                self.discover_forget(key);
            } else {
                let cards = data::parse_guilds(&body);
                self.request_card_images(&cards);
                self.discover.servers.featured = Load::Ready(cards);
            }
        } else if key == "discover:servers:categories" {
            if failed {
                self.discover_forget(key);
            } else {
                self.discover.servers.categories = data::parse_categories(&body);
            }
        } else if let Some(id) = key.strip_prefix("discover:servers:cat:") {
            let Ok(id) = id.parse::<i64>() else { return };
            if failed {
                self.discover.servers.by_category.insert(id, Load::Failed);
                self.discover_forget(key);
            } else {
                let cards = data::parse_guilds(&body);
                self.request_card_images(&cards);
                self.discover
                    .servers
                    .by_category
                    .insert(id, Load::Ready(cards));
            }
        } else if let Some(seq) = key.strip_prefix("discover:servers:search:") {
            let Ok(seq) = seq.parse::<u64>() else { return };
            self.servers_search_answer(seq, &body, failed);
        }
    }

    fn request_card_images(&mut self, cards: &[GuildCard]) {
        for card in cards {
            if let Some(url) = card.splash_url() {
                self.discover_image(&url);
            }
            if let Some(url) = card.icon_url() {
                self.discover_image(&url);
            }
        }
    }

    // ---- search ----

    pub(super) fn servers_search(&mut self, query: &str, cx: &mut Context<Self>) {
        self.discover.servers.menu = Menu::None;
        self.discover.servers.results = Some(Results {
            query: query.to_owned(),
            category: -1,
            ..Results::default()
        });
        self.discover.scroll.set_offset(point(px(0.), px(0.)));
        self.servers_search_page(0);
        cx.notify();
    }

    /// Sends the search for the page starting at `offset` (the answers are told apart by a counter, so an old answer
    /// after a filter change is dropped).
    fn servers_search_page(&mut self, offset: u32) {
        let Some(results) = self.discover.servers.results.as_mut() else {
            return;
        };
        results.loading = true;
        results.failed = false;
        let route = data::search_route(
            &results.query,
            results.category,
            results.language.as_deref(),
            offset,
            true,
        );
        self.discover.servers.seq += 1;
        let seq = self.discover.servers.seq;
        if let Some(results) = self.discover.servers.results.as_mut() {
            results.seq = seq;
        }
        let key = format!("discover:servers:search:{seq}");
        let _ = self.requests.send(Request::Fetch { key, route });
    }

    fn servers_search_answer(&mut self, seq: u64, body: &serde_json::Value, failed: bool) {
        let Some(results) = self.discover.servers.results.as_mut() else {
            return;
        };
        if results.seq != seq {
            return;
        }
        results.loading = false;
        if failed {
            results.failed = true;
            return;
        }
        let page: SearchPage = data::parse_search(body);
        let first = results.guilds.is_empty();
        results.total = page.total;
        if results.category == -1 {
            results.all_total = page.total;
        }
        if first && results.category == -1 && !page.counts.is_empty() || results.counts.is_empty() {
            results.counts = page.counts.clone();
        }
        let cards = page.guilds;
        results.guilds.extend(cards.iter().cloned());
        self.request_card_images(&cards);
    }

    pub(super) fn servers_exit_search(&mut self) {
        self.discover.servers.results = None;
        self.discover.servers.menu = Menu::None;
        self.discover.servers.mode = SearchMode::Collapsed;
        self.discover.servers.seq += 1;
    }

    /// Module 300290 `Z`: the Show filter changes the category, the language select the language; both start over.
    fn servers_refilter(&mut self, category: Option<i64>, language: Option<Option<String>>) {
        let Some(results) = self.discover.servers.results.as_mut() else {
            return;
        };
        if let Some(category) = category {
            results.category = category;
        }
        if let Some(language) = language {
            results.language = language;
        }
        results.guilds.clear();
        self.discover.servers.menu = Menu::None;
        self.discover.scroll.set_offset(point(px(0.), px(0.)));
        self.servers_search_page(0);
    }

    pub(super) fn servers_results_title(&self) -> String {
        let query = self
            .discover
            .servers
            .results
            .as_ref()
            .map(|results| results.query.clone())
            .unwrap_or_default();
        format!("Search results for \"{query}\"")
    }

    // ---- lurking ----

    /// A card was clicked: `PUT /guilds/{id}/members/@me?lurker=true`. The answer arrives under `discover:lurk:{id}`.
    pub(in crate::ui) fn servers_join(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.discover.servers.pending.contains(&id) {
            return;
        }
        if self.stores.guilds.iter().any(|guild| guild.id == id) {
            // Already a member: the card opens the server.
            self.discover.open = false;
            self.select(id, None);
            cx.notify();
            return;
        }
        self.discover.servers.pending.push(id);
        let _ = self
            .requests
            .send(lurk_request(id, self.stores.session_id.as_deref()));
        cx.notify();
    }

    /// Module 854492 `j`: a server that was only previewed is left once the view is somewhere else. Only previews
    /// this run started count, so a server the account belongs to can never be left from here.
    pub(in crate::ui) fn stop_lurking_elsewhere(&mut self) {
        let current = self.guild;
        let stores = &self.stores;
        let (leave, keep) = lurks_to_stop(&self.discover.servers.lurked, current, |guild| {
            stores.guilds.iter().any(|known| known.id == guild)
        });
        self.discover.servers.lurked = keep;
        for guild in leave {
            let _ = self.requests.send(stop_lurking_request(guild));
        }
    }

    pub(super) fn servers_lurked(&mut self, id: &str, key: &str) {
        let Ok(id) = id.parse::<u64>() else { return };
        self.discover
            .servers
            .pending
            .retain(|pending| *pending != id);
        let body = self.fetched.get(key).cloned().unwrap_or_default();
        self.fetched.remove(key);
        if body["ok"] == true {
            self.discover.awaiting_guild = Some(id);
            if !self.discover.servers.lurked.contains(&id) {
                self.discover.servers.lurked.push(id);
            }
            return;
        }
        // Module 778858's error handling: paused invites and bans have their own wording.
        let code = body["body"]["code"].as_u64();
        self.discover.notice = Some(
            match code {
                Some(180_000) => "Invites to this server are currently paused.",
                Some(40_007) => "Uh-oh. Looks like you've been banned.",
                _ => "Something went wrong. Try again.",
            }
            .to_owned(),
        );
    }

    // ---- views ----

    pub(super) fn servers_tabs(&self, mode: SearchMode, cx: &mut Context<Self>) -> AnyElement {
        let current = self.servers_tab();
        let mut row = div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .pt(px(2.))
            .flex()
            .items_center()
            .overflow_hidden();
        if mode == SearchMode::Floating {
            row = row.mr(px(240.));
        }
        for (index, tab) in ServersTab::ALL.into_iter().enumerate() {
            row = row.child(
                self.discover_tab(
                    SharedString::from(format!("discover-tab-{index}")),
                    tab.label(),
                    tab == current,
                    index == 0,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.discover.servers.tab = Some(tab);
                    this.discover.scroll.set_offset(point(px(0.), px(0.)));
                    this.servers_load_tab();
                    cx.notify();
                })),
            );
        }
        row.into_any_element()
    }

    /// Columns of `auto-fill, minmax(280px, 1fr)` with a 16px gap in `width`.
    pub(super) fn grid_columns(width: f32) -> usize {
        (((width + GAP) / (280. + GAP)).floor() as usize).max(1)
    }

    pub(super) fn servers_body(
        &mut self,
        width: f32,
        _offset: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.discover.servers.results.is_some() {
            return self.servers_results(width, cx);
        }
        let tab = self.servers_tab();
        let hero = self.discover_hero(tab.hero_title(), tab.hero_description(), width + 8.);
        let load = match tab.category() {
            None => self.discover.servers.featured.clone(),
            Some(category) => self
                .discover
                .servers
                .by_category
                .get(&category)
                .cloned()
                .unwrap_or_default(),
        };
        let mut page = div().w_full().flex().flex_col().child(hero);
        if tab == ServersTab::Hubs {
            return page.child(self.hubs_section(width, cx)).into_any_element();
        }
        if let Some(title) = tab.section_title() {
            page = page.child(
                text(20., 24., 600., "--text-strong")
                    .flex_none()
                    .pl(px(32.))
                    .pt(px(24.))
                    .h(px(48.))
                    .child(title),
            );
        }
        match load {
            Load::Ready(cards) => {
                let columns = Self::grid_columns(width - 40.);
                page = page.child(
                    div()
                        .parity("discover-grid")
                        .w_full()
                        .pl(px(20.))
                        .pr(px(20.))
                        .pt(px(16.))
                        .pb(px(15.))
                        .grid()
                        .grid_cols(columns as u16)
                        .gap(px(GAP))
                        .children(cards.iter().map(|card| self.server_card(card, cx))),
                );
            }
            Load::Failed => {
                page = page.child(self.discover_message(
                    "Something went wrong",
                    "We couldn't load servers. Try again.",
                    Some("discover-servers-retry"),
                    cx,
                ));
            }
            _ => {}
        }
        page.into_any_element()
    }

    /// The empty and error cards: a heading and a line below it, centred.
    pub(super) fn discover_message(
        &self,
        heading: &str,
        body: &str,
        retry: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .w_full()
            .py(px(48.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .child(text(20., 24., 600., "--text-strong").child(heading.to_owned()))
            .child(text(14., 18., 400., "--text-muted").child(body.to_owned()))
            .when_some(retry, |this, id| {
                this.child(
                    text(14., 18., 500., "--white")
                        .id(id)
                        .mt(px(8.))
                        .px(px(16.))
                        .h(px(32.))
                        .rounded(px(8.))
                        .bg(color("--brand-500"))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.discover_retry();
                            cx.notify();
                        }))
                        .child("Try again"),
                )
            })
            .into_any_element()
    }

    pub(super) fn discover_retry(&mut self) {
        match self.discover.place {
            Place::Servers => {
                self.discover.servers.featured = Load::Idle;
                self.discover
                    .servers
                    .by_category
                    .retain(|_, load| *load != Load::Failed);
                self.servers_load_tab();
            }
            _ => self.apps_retry(),
        }
    }

    /// The results page below the header: the Show filter and language select, then the four-column grid.
    fn servers_results(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let Some(results) = self.discover.servers.results.clone() else {
            return div().into_any_element();
        };
        let menu = self.discover.servers.menu;
        let inner = width - 56.;
        let columns = {
            let mut left = inner - 280.;
            let mut count = 1usize;
            while left > 0. {
                left -= 264.;
                count += 1;
            }
            count
        };
        let category_name = |id: i64| -> String {
            if id == -1 {
                return "All".to_owned();
            }
            self.discover
                .servers
                .categories
                .iter()
                .find(|category| category.id == id)
                .map(|category| category.name.clone())
                .unwrap_or_default()
        };
        let selected_name = category_name(results.category);
        let filter = div()
            .relative()
            .flex_none()
            .h(px(26.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(text(14., 18., 500., "--text-muted").child("Show"))
            .child(
                div()
                    .id("discover-show")
                    .h(px(26.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .rounded(px(4.))
                    .bg(color("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.discover.servers.menu = Menu::Show;
                        cx.notify();
                    }))
                    .child(text(14., 18., 500., "--text-muted").child(selected_name))
                    .child(icon(
                        if menu == Menu::Show {
                            "discover_filter_chevron_up"
                        } else {
                            "discover_filter_chevron"
                        },
                        16.,
                        "--text-muted",
                    )),
            )
            .when(menu == Menu::Show, |this| {
                let mut items: Vec<(i64, String, u64)> =
                    vec![(-1, "All".to_owned(), results.all_total)];
                for (id, count) in &results.counts {
                    items.push((*id, category_name(*id), *count));
                }
                this.child(gpui::deferred(
                    div()
                        .id("discover-show-menu")
                        .occlude()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.discover.servers.menu = Menu::None;
                            cx.notify();
                        }))
                        .absolute()
                        .top(px(34.))
                        .left(px(41.))
                        .w(px(144.))
                        .max_h(px(300.))
                        .overflow_y_scroll()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::rgba(0x0000003d).into(),
                            offset: point(px(0.), px(12.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .p(px(8.))
                        .flex()
                        .flex_col()
                        .children(items.into_iter().map(|(id, name, count)| {
                            let selected = id == results.category;
                            div()
                                .id(SharedString::from(format!("discover-show-{id}")))
                                .flex_none()
                                .h(px(52.))
                                .p(px(8.))
                                .rounded(px(4.))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .hover(|style| style.bg(color("--background-mod-subtle")))
                                .when(selected, |row| {
                                    row.child(div().mr(px(8.)).child(icon(
                                        "discover_check_circle",
                                        20.,
                                        "--text-strong",
                                    )))
                                })
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(text(14., 18., 500., "--text-strong").child(name))
                                        .child(
                                            text(12., 16., 400., "--text-muted")
                                                .mt(px(2.))
                                                .child(data::group_digits(count)),
                                        ),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.servers_refilter(Some(id), None);
                                    cx.notify();
                                }))
                        })),
                ))
            });
        let language = results
            .language
            .clone()
            .unwrap_or_else(|| data::default_language().to_owned());
        let select = div()
            .parity("discover-language")
            .relative()
            .flex_none()
            .w(px(180.))
            .h(px(40.))
            .child(
                div()
                    .id("discover-language")
                    .size_full()
                    .rounded(px(8.))
                    .bg(color("--input-background-default"))
                    .border_1()
                    .border_color(color(if menu == Menu::Language {
                        "--input-border-active"
                    } else {
                        "--input-border-default"
                    }))
                    .rule(crate::ui::motion::Rule::TextField)
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.discover.servers.menu = Menu::Language;
                        cx.notify();
                    }))
                    .child(
                        text(16., 20., 500., "--text-strong")
                            .flex_1()
                            .min_w(px(0.))
                            .pl(px(12.))
                            .truncate()
                            .child(data::language_name(&language)),
                    )
                    .child(
                        div()
                            .w(px(36.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(
                                if menu == Menu::Language {
                                    "discover_select_chevron_up"
                                } else {
                                    "discover_select_chevron"
                                },
                                20.,
                                "--text-strong",
                            )),
                    ),
            )
            .when(menu == Menu::Language, |this| {
                // `selectDropdown_vWC4bk`: 180 by 210, a 1px border, and 4px of padding around the `auto` scroller (a
                // 16px gutter, so the rows are 154 wide and the thumb sits at the right edge of the list).
                let list = self.discover.servers.language_scroll.clone();
                let thumb = super::super::settings::auto_scrollbar(list.clone());
                this.child(gpui::deferred(
                    div()
                        .id("discover-language-menu")
                        .occlude()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.discover.servers.menu = Menu::None;
                            cx.notify();
                        }))
                        .absolute()
                        .top(px(44.))
                        .left_0()
                        .w(px(180.))
                        .h(px(210.))
                        .rounded(px(8.))
                        .bg(color("--background-surface-higher"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::rgba(0x00000029).into(),
                            offset: point(px(0.), px(2.)),
                            blur_radius: px(8.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .p(px(4.))
                        .child(
                            div()
                                .relative()
                                .size_full()
                                .child(
                                    div()
                                        .id("discover-language-scroll")
                                        .size_full()
                                        .overflow_y_scroll()
                                        .track_scroll(&list)
                                        .pr(px(16.))
                                        .flex()
                                        .flex_col()
                                        .children(data::LANGUAGES.iter().map(|(code, name)| {
                                            let code = *code;
                                            let selected = code == language;
                                            div()
                                                .id(SharedString::from(format!(
                                                    "discover-lang-{code}"
                                                )))
                                                .flex_none()
                                                .h(px(40.))
                                                .pl(px(8.))
                                                .pr(px(4.))
                                                .rounded(px(4.))
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .hover(|style| {
                                                    style.bg(color("--background-mod-subtle"))
                                                })
                                                .child(
                                                    text(16., 20., 500., "--text-strong")
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .truncate()
                                                        .child(*name),
                                                )
                                                .when(selected, |row| {
                                                    row.child(icon(
                                                        "discover_check",
                                                        20.,
                                                        "--text-strong",
                                                    ))
                                                })
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.servers_refilter(
                                                        None,
                                                        Some(Some(code.to_owned())),
                                                    );
                                                    cx.notify();
                                                }))
                                        })),
                                )
                                .child(thumb),
                        ),
                ))
            });
        let mut page = div().relative().w_full().flex().flex_col().child(
            div()
                .parity("discover-results-heading")
                .mt(px(24.))
                .ml(px(0.))
                .pl(px(32.))
                .mr(px(24.))
                .h(px(40.))
                .flex_none()
                .flex()
                .items_start()
                .justify_between()
                .child(div().pt(px(7.)).child(filter))
                .child(select),
        );
        if results.guilds.is_empty() && !results.loading && !results.failed {
            let named = (results.category != -1).then(|| category_name(results.category));
            let (heading, body) = match named {
                Some(name) => (
                    format!("No results found in {name}"),
                    "Try searching for something else or see results from all categories.",
                ),
                None => (
                    "No results found".to_owned(),
                    "Try searching for something else.",
                ),
            };
            page = page.child(self.discover_message(&heading, body, None, cx));
        } else if results.failed && results.guilds.is_empty() {
            page = page.child(self.discover_message(
                "Something went wrong",
                "We couldn't load servers. Try again.",
                Some("discover-results-retry"),
                cx,
            ));
        } else {
            page = page.child(
                div()
                    .parity("discover-grid")
                    .mt(px(20.))
                    .pl(px(32.))
                    .pr(px(24.))
                    .pb(px(24.))
                    .grid()
                    .grid_cols(columns as u16)
                    .gap(px(GAP))
                    .children(results.guilds.iter().map(|card| self.server_card(card, cx))),
            );
        }
        // Infinite loading: the next page is asked for once the bottom is within 240px.
        let scroll = &self.discover.scroll;
        let near = scroll.max_offset().y.as_f32() + scroll.offset().y.as_f32() < 240.;
        if near
            && !results.loading
            && !results.failed
            && (results.guilds.len() as u64) < results.total
            && !results.guilds.is_empty()
        {
            let loaded = results.guilds.len() as u32;
            self.servers_search_page(loaded);
        }
        page.into_any_element()
    }

    /// `card_1U9V53`: a 322px card with the banner, the icon, the title, three lines of description and the counts.
    fn server_card(&self, card: &GuildCard, cx: &mut Context<Self>) -> AnyElement {
        let id = card.id;
        let group = SharedString::from(format!("discover-card-{id}"));
        let banner = card
            .splash_url()
            .and_then(|url| self.images.get(&format!("discover:img:{url}")).cloned())
            .or_else(|| {
                self.images
                    .get(&format!("discover:img:{DEFAULT_ART}"))
                    .cloned()
            });
        let avatar = card
            .icon_url()
            .and_then(|url| self.images.get(&format!("discover:img:{url}")).cloned());
        let pending = self.discover.servers.pending.contains(&id);
        let badge = if card.has("PARTNERED") {
            Some(true)
        } else if card.has("VERIFIED") {
            Some(false)
        } else {
            None
        };
        let counts = |dot: &'static str, label: String| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .min_w(px(0.))
                .child(div().flex_none().size(px(8.)).rounded_full().bg(color(dot)))
                .child(
                    text(12., 16., 400., "--text-default")
                        .min_w(px(0.))
                        .truncate()
                        .child(label),
                )
        };
        div()
            .h(px(CARD_HEIGHT))
            .child(
                div()
                    .id(SharedString::from(format!("discover-card-{id}")))
                    .group(group.clone())
                    .relative()
                    .w_full()
                    .h(px(CARD_HEIGHT + 2.))
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
                    .on_click(cx.listener(move |this, _, _, cx| this.servers_join(id, cx)))
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .w_full()
                            .h(px(146.))
                            .overflow_hidden()
                            .children(
                                banner.map(|image| {
                                    img(image).size_full().object_fit(ObjectFit::Cover)
                                }),
                            )
                            // `maximizeIcon_BTjcfU` (`eA` of module 300290): 8px in from the banner's top right, it
                            // opens the server's profile popout without joining.
                            .child(
                                div()
                                    .id(SharedString::from(format!("discover-card-more-{id}")))
                                    .absolute()
                                    .top(px(8.))
                                    .right(px(8.))
                                    .size(px(32.))
                                    .rounded(px(4.))
                                    .bg(rgba(0x000000b8))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .child(self.anchor_probe(format!("discover-card-more-{id}")))
                                    .child(icon("discover_maximize", 16., "--text-subtle"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.servers_open_profile(id, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .flex()
                            .flex_col()
                            .pl(px(16.))
                            .pr(px(16.))
                            .pt(px(28.))
                            .pb(px(16.))
                            .child(
                                div()
                                    .flex_none()
                                    .h(px(20.))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .when_some(badge, |row, partner| row.child(flower(partner)))
                                    .child(
                                        text(16., 20., 600., "--text-strong")
                                            .min_w(px(0.))
                                            .truncate()
                                            .child(card.name.clone()),
                                    ),
                            )
                            .child(
                                text(14., 18., 400., "--text-default")
                                    .mt(px(4.))
                                    .max_h(px(54.))
                                    .overflow_hidden()
                                    .line_clamp(3)
                                    .text_ellipsis()
                                    .child(card.description.clone()),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .gap(px(GAP))
                                    .min_w(px(0.))
                                    .children(card.online.map(|count| {
                                        counts(
                                            "--green-360",
                                            format!("{} Online", data::group_digits(count)),
                                        )
                                    }))
                                    .children(card.members.map(|count| {
                                        counts(
                                            "--primary-300",
                                            format!("{} Members", data::group_digits(count)),
                                        )
                                    })),
                            ),
                    )
                    // The icon block overlaps the banner: 56px, the 48px picture inside a 4px ring of the card colour.
                    .child(
                        div()
                            .absolute()
                            .left(px(12.))
                            .top(px(113.))
                            .size(px(56.))
                            .rounded(px(17.))
                            .p(px(4.))
                            .bg(color("--background-base-low"))
                            .group_hover(group, |style| {
                                style.bg(color("--background-surface-high"))
                            })
                            .child(
                                div()
                                    .size(px(48.))
                                    .rounded(px(14.))
                                    .overflow_hidden()
                                    .bg(color("--background-mod-subtle"))
                                    .children(avatar.map(|image| {
                                        img(image).size_full().object_fit(ObjectFit::Cover)
                                    })),
                            ),
                    )
                    .when(pending, |card| {
                        card.child(
                            div()
                                .absolute()
                                .inset_0()
                                .bg(gpui::rgba(0x00000066))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(text(14., 18., 600., "--text-strong").child("Loading...")),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// `flowerStarContainer_d3vyhc`: the 16px flower in green (verified) or brand blue (partnered) with a 10px mark inside.
fn flower(partner: bool) -> impl IntoElement {
    div()
        .relative()
        .flex_none()
        .size(px(16.))
        .ml(px(-2.))
        .child(icon(
            "discover_flower",
            16.,
            if partner {
                "--brand-500"
            } else {
                "--green-360"
            },
        ))
        .child(div().absolute().top(px(3.)).left(px(3.)).child(icon(
            if partner {
                "discover_flower_partner"
            } else {
                "discover_flower_check"
            },
            10.,
            "--white",
        )))
}

/// `PUT /guilds/{id}/members/@me?lurker=true`, the request `joinGuild` sends for a lurked directory server. The answer
/// is read under `discover:lurk:{id}`.
pub(super) fn lurk_request(id: u64, session: Option<&str>) -> Request {
    Request::Rest {
        key: Some(format!("discover:lurk:{id}")),
        method: reqwest::Method::PUT,
        route: data::lurk_route(id, session),
        body: Some(serde_json::json!({})),
    }
}

/// `DELETE /users/@me/guilds/{id}` with `{lurking: true}`.
fn stop_lurking_request(id: u64) -> Request {
    Request::Rest {
        key: None,
        method: reqwest::Method::DELETE,
        route: data::stop_lurking_route(id),
        body: Some(serde_json::json!({"lurking": true})),
    }
}

/// Of the previews this run started: those to end now, and those to keep track of. The one in view stays; one whose
/// server has not arrived yet is kept for later; the rest are ended and forgotten.
fn lurks_to_stop(
    lurked: &[u64],
    current: Option<u64>,
    present: impl Fn(u64) -> bool,
) -> (Vec<u64>, Vec<u64>) {
    let mut leave = Vec::new();
    let mut keep = Vec::new();
    for guild in lurked {
        if Some(*guild) == current || !present(*guild) {
            keep.push(*guild);
        } else {
            leave.push(*guild);
        }
    }
    (leave, keep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lurk_request_is_the_lurker_put() {
        let Request::Rest {
            key,
            method,
            route,
            body,
        } = lurk_request(123, Some("abc"))
        else {
            panic!("a rest request");
        };
        assert_eq!(key.as_deref(), Some("discover:lurk:123"));
        assert_eq!(method, reqwest::Method::PUT);
        assert_eq!(route, "/guilds/123/members/@me?lurker=true&session_id=abc");
        assert_eq!(body, Some(serde_json::json!({})));
    }

    #[test]
    fn a_preview_ends_with_the_lurking_leave_and_only_previews_of_this_run_end() {
        let Request::Rest {
            key,
            method,
            route,
            body,
        } = stop_lurking_request(123)
        else {
            panic!("a rest request");
        };
        assert_eq!(key, None);
        assert_eq!(method, reqwest::Method::DELETE);
        assert_eq!(route, "/users/@me/guilds/123");
        assert_eq!(body, Some(serde_json::json!({"lurking": true})));
        // 1 is in view, 2 has arrived and is elsewhere, 3 has not arrived yet.
        let (leave, keep) = lurks_to_stop(&[1, 2, 3], Some(1), |guild| guild != 3);
        assert_eq!(leave, vec![2]);
        assert_eq!(keep, vec![1, 3]);
        // Nothing was previewed: nothing is ever left, whatever servers the account is in.
        assert_eq!(lurks_to_stop(&[], Some(9), |_| true), (vec![], vec![]));
    }

    #[test]
    fn grid_columns_follow_the_auto_fill_rule() {
        assert_eq!(Root::grid_columns(857.), 2);
        assert_eq!(Root::grid_columns(280.), 1);
        assert_eq!(Root::grid_columns(888.), 3);
    }
}
