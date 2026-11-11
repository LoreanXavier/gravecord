//! The Discover page behind the compass at the bottom of the server rail. The sidebar (Apps, Servers, Quests), the header bar
//! with its tabs and search field, the hero and the scroll frame live here; the Servers page is in `servers`, the Apps pages in
//! `apps`, and the response parsers in `data`.
//!
//! Official modules followed: 300290 (Servers page), 586193 (Apps pages), 521974 and 770472 (the collapsing search bar),
//! 965660, 808097, 449054 and 324580 (directory requests).
mod app_data;
mod apps;
mod data;
mod hubs;
mod profile;
mod servers;

use super::{Root, color, editor_style, icon};
use crate::ui::motion::Transitions as _;
use crate::{
    editor::{Editor, EditorEvent},
    net::Request,
    parity::Parity,
};
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, ScrollHandle, SharedString,
    Subscription, Window, canvas, div, fill, img, point, prelude::*, px, size,
};
use std::sync::Arc;

/// What the Discover page shows (the router's `/discovery/servers`, `/discovery/applications` and an application's page).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Servers,
    Apps,
    App(Id),
}

/// A directory request in flight, an answer, or an error (the pages' loading, content and error states).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed,
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(value) => Some(value),
            _ => None,
        }
    }
}

/// The three states of the header's search bar (module 521974): a magnifier at the right, a field floating over the tabs,
/// and a field in the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum SearchMode {
    #[default]
    Collapsed,
    Floating,
    Inline,
}

pub(super) const FETCH_PREFIX: &str = "discover:";
/// The page area starts 375px from the left: the rail and the 303px sidebar.
const PAGE_LEFT: f32 = 375.;
/// `scroller_jkJlMe` keeps an 8px gutter for its scrollbar.
const GUTTER: f32 = 8.;
const HEADER: f32 = 49.;

pub(super) struct State {
    pub open: bool,
    pub place: Place,
    pub scroll: ScrollHandle,
    pub search: Option<Entity<Editor>>,
    events: Option<Subscription>,
    /// The field has been seen focused while floating; it folds back once focus leaves an empty field.
    floating_focused: bool,
    /// A server was lurked and its tile has not arrived yet: the guild to open once it does.
    pub awaiting_guild: Option<Id>,
    pub notice: Option<String>,
    pub servers: servers::Servers,
    pub apps: apps::Apps,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            place: Place::Servers,
            scroll: ScrollHandle::new(),
            search: None,
            events: None,
            floating_focused: false,
            awaiting_guild: None,
            notice: None,
            servers: servers::Servers::default(),
            apps: apps::Apps::default(),
        }
    }
}

/// A text run with the page's usual metrics.
pub(super) fn text(size: f32, line: f32, weight: f32, tint: &str) -> gpui::Div {
    div()
        .text_size(px(size))
        .line_height(px(line))
        .font_weight(FontWeight(weight))
        .text_color(color(tint))
}

impl Root {
    pub(super) fn discover_place(&self) -> Option<Place> {
        (self.discover.open && self.guild.is_none() && self.channel.is_none())
            .then_some(self.discover.place)
    }

    pub(super) fn discover_active(&self) -> bool {
        self.discover_place().is_some()
    }

    /// Opens the page (rail compass, sidebar rows, history). Records the visit like `select_home` does.
    pub(super) fn open_discover(&mut self, place: Place, cx: &mut Context<Self>) {
        if self.discover_place() == Some(place) {
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
                discover: Some(place),
                quests: false,
                shop: false,
                message_requests: false,
            },
        );
        self.set_discover_place(place, cx);
    }

    /// The part of `open_discover` that history replays also run.
    pub(super) fn set_discover_place(&mut self, place: Place, cx: &mut Context<Self>) {
        self.guild = None;
        self.channel = None;
        self.nitro_open = false;
        self.guild_guide_open = false;
        self.channel_browser_open = false;
        self.discover.open = true;
        self.stop_lurking_elsewhere();
        self.discover.place = place;
        self.discover.scroll.set_offset(point(px(0.), px(0.)));
        self.discover.notice = None;
        self.discover.servers.popout = None;
        self.discover.servers.hub = None;
        self.discover_search_editor(cx);
        match place {
            Place::Servers => self.servers_enter(cx),
            Place::Apps => self.apps_enter(cx),
            Place::App(id) => self.app_enter(id, cx),
        }
        cx.notify();
    }

    pub(super) fn discover_request(&mut self, key: String, route: String) {
        if self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch { key, route });
        }
    }

    /// Asks for a request again after an error.
    pub(super) fn discover_forget(&mut self, key: &str) {
        self.requested.remove(key);
        self.fetched.remove(key);
    }

    pub(super) fn discover_image(&mut self, url: &str) -> String {
        let key = format!("discover:img:{url}");
        self.request_image(key.clone(), url.to_owned());
        key
    }

    pub(super) fn discover_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        if key.starts_with("discover:servers:") {
            self.servers_fetched(key);
        } else if key.starts_with("discover:apps:") || key.starts_with("discover:app:") {
            self.apps_fetched(key);
        } else if let Some(id) = key.strip_prefix("discover:lurk:") {
            self.servers_lurked(id, key);
        } else if let Some(id) = key.strip_prefix("discover:profile:") {
            self.servers_profile_fetched(id, key);
        } else if key == hubs::LOOKUP_KEY {
            self.hub_fetched(key);
        }
        cx.notify();
    }

    /// Writes the raw answers the pages parse to `design/parity/discover-<name>.json` (a harness step); the shapes are
    /// noted beside each parser.
    #[cfg(feature = "capture")]
    pub fn discover_dump(&self) -> String {
        let mut written = Vec::new();
        for (key, body) in &self.fetched {
            if let Some(name) = key.strip_prefix("discover:") {
                let name = name.replace(':', "-");
                let path = format!("design/parity/discover-dump-{name}.json");
                let text = serde_json::to_string_pretty(body).unwrap_or_default();
                if std::fs::write(&path, text).is_ok() {
                    written.push(name);
                }
            }
        }
        written.sort();
        format!("discover dump: {}", written.join(", "))
    }

    // ---- the search editor shared by both pages ----

    fn discover_search_editor(&mut self, cx: &mut Context<Self>) {
        if self.discover.search.is_some() {
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
            editor.set_placeholder("Search".into());
            editor.set_reports_all_edits(true);
            editor
        });
        let events = cx.subscribe(&editor, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {}
                EditorEvent::Cancel => {
                    editor.update(cx, |editor, _| editor.set_text(""));
                    this.discover_exit_search(cx);
                }
                EditorEvent::Submit(text) => {
                    let text = text.trim().to_owned();
                    this.discover_submit_search(&text, cx);
                }
            }
            cx.notify();
        });
        self.discover.search = Some(editor);
        self.discover.events = Some(events);
    }

    #[cfg(feature = "capture")]
    pub(super) fn search_editors(&self) -> Vec<Entity<Editor>> {
        if self.discover_active() {
            self.discover.search.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }

    fn discover_query(&self, cx: &gpui::App) -> String {
        self.discover
            .search
            .as_ref()
            .map(|editor| editor.read(cx).text().to_owned())
            .unwrap_or_default()
    }

    fn discover_submit_search(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        match self.discover.place {
            Place::Servers => self.servers_search(text, cx),
            Place::Apps | Place::App(_) => self.apps_search(text, cx),
        }
    }

    /// The field's clear button and Escape: leave the results and fold the field back.
    fn discover_exit_search(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.discover.search.clone() {
            editor.update(cx, |editor, _| editor.set_text(""));
        }
        match self.discover.place {
            Place::Servers => self.servers_exit_search(),
            _ => self.apps_exit_search(),
        }
        self.discover.scroll.set_offset(point(px(0.), px(0.)));
    }

    fn discover_focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.discover.search.clone() {
            window.focus(&editor.read(cx).focus_handle(cx), cx);
        }
    }

    // ---- rail and sidebar ----

    /// The compass in the rail (`ListItem` with the discover icon): the selected tile is the brand colour with a white icon.
    pub(super) fn discover_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.discover_active();
        let group = SharedString::from("discover-row");
        div()
            .relative()
            .w(px(72.))
            .h(px(40.))
            .flex_none()
            .flex()
            .justify_center()
            .group(group.clone())
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(if selected { 0. } else { 10. }))
                    .w(px(4.))
                    .h(px(if selected { 40. } else { 20. }))
                    .rounded_r(px(4.))
                    .bg(color("--text-strong"))
                    .when(!selected, |this| {
                        this.opacity(0.)
                            .group_hover(group.clone(), |style| style.opacity(1.))
                    }),
            )
            .child(
                self.with_tip(
                    "discover",
                    super::tooltip::Tip::rail(super::tooltip::Body::Heading("Discover".into())),
                    div()
                        .flex_none()
                        .size(px(40.))
                        .rounded(px(12.))
                        .bg(color(if selected {
                            "--background-brand"
                        } else {
                            "--background-mod-subtle"
                        }))
                        .when(!selected, |this| {
                            this.group_hover(group, |style| style.bg(color("--background-brand")))
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(
                            "discover",
                            20.,
                            if selected {
                                "--white"
                            } else {
                                "--text-default"
                            },
                        )),
                    cx,
                )
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    let place = if this.discover.open {
                        this.discover.place
                    } else {
                        Place::Servers
                    };
                    let place = match place {
                        Place::App(_) => Place::Apps,
                        other => other,
                    };
                    this.open_discover(place, cx);
                })),
            )
            .into_any_element()
    }

    /// `sidebar_pog62O` of the Discover route: the 303px column with the "Discover" header and its three rows.
    pub(super) fn discover_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let place = self.discover.place;
        let row = |id: &'static str, glyph: &'static str, label: &'static str, selected: bool| {
            div()
                .id(id)
                .flex_none()
                .w(px(286.))
                .h(px(48.))
                .px(px(8.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .when(selected, |this| this.bg(color("--background-mod-normal")))
                .when(!selected, |this| {
                    this.hover(|style| style.bg(color("--background-mod-subtle")))
                })
                .child(icon(
                    glyph,
                    24.,
                    if selected {
                        "--text-strong"
                    } else {
                        "--text-subtle"
                    },
                ))
                .child(
                    text(
                        16.,
                        20.,
                        500.,
                        if selected {
                            "--text-strong"
                        } else {
                            "--text-subtle"
                        },
                    )
                    .child(label),
                )
        };
        div()
            .parity("sidebar")
            .relative()
            .flex_none()
            .w(px(303.))
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_l_1()
                    .border_t_1()
                    .rounded_tl(px(12.))
                    .border_color(color("--background-mod-subtle")),
            )
            .child(div().flex_none().h(px(1.)))
            .child(
                div()
                    .flex_none()
                    .h(px(49.))
                    .border_b_1()
                    .border_color(color("--background-mod-subtle"))
                    .pl(px(17.))
                    .flex()
                    .items_center()
                    .child(text(20., 24., 600., "--text-strong").child("Discover")),
            )
            .child(
                div()
                    .flex_none()
                    .p(px(8.))
                    .pl(px(9.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        row(
                            "discover-apps",
                            "discover_nav_apps",
                            "Apps",
                            matches!(place, Place::Apps | Place::App(_)),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_discover(Place::Apps, cx);
                        })),
                    )
                    .child(
                        row(
                            "discover-servers",
                            "discover_nav_servers",
                            "Servers",
                            place == Place::Servers,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_discover(Place::Servers, cx);
                        })),
                    )
                    .child(
                        row("discover-quests", "discover_nav_quests", "Quests", false)
                            .on_click(cx.listener(|this, _, _, cx| this.open_quests(cx))),
                    ),
            )
    }

    // ---- the page ----

    pub(super) fn discover_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(id) = self.discover.awaiting_guild
            && self.stores.guilds.iter().any(|guild| guild.id == id)
        {
            self.discover.awaiting_guild = None;
            self.discover.open = false;
            self.select(id, None);
        }
        let place = self.discover.place;
        let width = (f32::from(window.viewport_size().width) - PAGE_LEFT).max(0.);
        let scroll = self.discover.scroll.clone();
        let offset = (-f32::from(scroll.offset().y)).max(0.);
        let results = match place {
            Place::Servers => self.discover.servers.results.is_some(),
            Place::Apps => self.discover.apps.search.is_some(),
            Place::App(_) => true,
        };
        self.discover_search_mode(window, cx);
        let content_width = width - GUTTER;
        let body = match place {
            Place::Servers => self.servers_body(content_width, offset, cx),
            Place::Apps => self.apps_body(content_width, offset, cx),
            Place::App(id) => self.app_detail_body(id, content_width, offset, cx),
        };
        let mut header = Some(self.discover_header(window, offset, results, cx));
        let solid_background = results;
        let page_bg = if matches!(place, Place::Servers) && results
            || matches!(place, Place::Apps) && results
        {
            "--background-base-low"
        } else {
            "--background-base-lower"
        };
        let scroller = div()
            .id("discover-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .pr(px(GUTTER))
            .child(body);
        let notice = self.discover.notice.clone().map(|message| {
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
                    .parity("discover-page")
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .bg(color(if solid_background {
                        page_bg
                    } else {
                        "--background-base-lower"
                    }))
                    .when(solid_background, |this| this.children(header.take()))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h(px(0.))
                            .child(scroller)
                            .child(gutter_scrollbar(scroll)),
                    )
                    .when(!solid_background, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(HEADER))
                                .children(header.take()),
                        )
                    })
                    .children(notice),
            )
            .into_any_element()
    }

    /// Module 770472: the bar folds to a magnifier on Servers and stays in the row on Apps.
    fn discover_search_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self
            .discover
            .search
            .as_ref()
            .is_some_and(|editor| editor.read(cx).focus_handle(cx).is_focused(window));
        let empty = self.discover_query(cx).is_empty();
        let mode = match self.discover.place {
            Place::Servers => &mut self.discover.servers.mode,
            _ => &mut self.discover.apps.mode,
        };
        if *mode == SearchMode::Floating {
            if focused {
                self.discover.floating_focused = true;
            } else if self.discover.floating_focused && empty {
                *mode = SearchMode::Collapsed;
                self.discover.floating_focused = false;
            }
        }
    }

    fn discover_mode(&self) -> SearchMode {
        match self.discover.place {
            Place::Servers => self.discover.servers.mode,
            _ => self.discover.apps.mode,
        }
    }

    /// `headerBar_Y0eJLL`: 49px tall, padded 16px, 20px between the icon, the tabs and the search bar.
    fn discover_header(
        &mut self,
        window: &mut Window,
        offset: f32,
        results: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let place = self.discover.place;
        let mode = self.discover_mode();
        let backdrop = (offset / 100.).clamp(0., 1.);
        let mut row = div()
            .parity("discover-header")
            .relative()
            .flex_none()
            .w_full()
            .h(px(HEADER))
            .pl(px(16.))
            .pr(px(16.))
            .flex()
            .items_center()
            .gap(px(20.));
        if results {
            row = row.bg(color("--background-base-low"));
        } else {
            row = row.child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(color("--background-base-lower"))
                    .opacity(backdrop),
            );
        }
        // The 1px ::after rule under the bar.
        row = row.child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(1.))
                .bg(color("--background-mod-subtle")),
        );
        match place {
            Place::Servers if !results => {
                row = row.child(div().pt(px(2.)).child(icon(
                    "discover_header_servers",
                    24.,
                    "--text-strong",
                )));
                row = row.child(self.servers_tabs(mode, cx));
            }
            Place::Apps if !results => {
                row = row.child(div().pt(px(2.)).child(icon(
                    "discover_header_apps",
                    24.,
                    "--text-strong",
                )));
                row = row.child(self.apps_tabs(cx));
            }
            Place::App(_) => {
                row = self.app_detail_header(row, offset, cx);
                return row.into_any_element();
            }
            _ => {
                let title = match place {
                    Place::Servers => self.servers_results_title(),
                    _ => self.apps_results_title(),
                };
                row = row
                    .child(
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
                                this.discover_exit_search(cx);
                                cx.notify();
                            }))
                            .child(icon("lb_arrow_left", 18., "--text-strong")),
                    )
                    .child(
                        text(20., 24., 600., "--text-strong")
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .child(title),
                    );
            }
        }
        match mode {
            SearchMode::Collapsed if !results => {
                row = row.child(
                    div()
                        .id("discover-search-icon")
                        .flex_none()
                        .w(px(24.))
                        .h(px(18.))
                        .flex()
                        .justify_end()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, window, cx| {
                            match this.discover.place {
                                Place::Servers => this.discover.servers.mode = SearchMode::Floating,
                                _ => this.discover.apps.mode = SearchMode::Floating,
                            }
                            this.discover.floating_focused = false;
                            this.discover_focus_search(window, cx);
                            cx.notify();
                        }))
                        .child(icon("search", 18., "--text-strong")),
                );
            }
            SearchMode::Floating if !results => {
                row = row.child(
                    div()
                        .absolute()
                        .top(px(8.5))
                        .right(px(16.))
                        .child(self.discover_search_field(window, cx)),
                );
            }
            _ => {
                row = row.child(self.discover_search_field(window, cx));
            }
        }
        row.into_any_element()
    }

    /// `searchBar_B1ffsk`: 240 by 32, the 12px magnifier in a 22px box, the input and the clear button.
    pub(super) fn discover_search_field(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(editor) = self.discover.search.clone() else {
            return div().into_any_element();
        };
        let focused = editor.read(cx).focus_handle(cx).is_focused(window);
        let filled = !editor.read(cx).text().is_empty();
        // The focus ring is 2px; the unfocused border is 1px.
        let border = if focused { 2. } else { 1. };
        div()
            .parity_with_border("discover-search", border)
            .flex_none()
            .w(px(240.))
            .h(px(32.))
            .border(px(border))
            .border_color(color(if focused {
                "--input-border-active"
            } else {
                "--input-border-default"
            }))
            .rule(crate::ui::motion::Rule::TextField)
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
                    .child(icon("search", 12., "--interactive-text-default")),
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
                        .id("discover-search-clear")
                        .size(px(22.))
                        .flex_none()
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.discover_exit_search(cx);
                            cx.notify();
                        }))
                        .child(icon("discover_clear", 12., "--interactive-text-default")),
                )
            })
            .into_any_element()
    }

    /// One header tab: 14px semibold, `--interactive-text-default`, the active one strong with a 2px underline.
    pub(super) fn discover_tab(
        &self,
        id: SharedString,
        label: &str,
        active: bool,
        first: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .relative()
            .flex_none()
            .h_full()
            .flex()
            .items_center()
            .cursor_pointer()
            .when(!first, |this| this.ml(px(24.)))
            .child(
                text(
                    14.,
                    18.,
                    600.,
                    if active {
                        "--text-default"
                    } else {
                        "--text-subtle"
                    },
                )
                .child(label.to_owned()),
            )
            .when(!active, |this| {
                this.hover(|style| style.text_color(color("--interactive-text-hover")))
            })
            .when(active, |this| {
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(0.))
                        .h(px(2.))
                        .rounded(px(2.))
                        .bg(color("--text-default")),
                )
            })
    }

    /// The hero of the Home and category pages (`container_-6JdM0`): 348px tall, the gradient behind the title and the text.
    pub(super) fn discover_hero(&self, title: &str, description: &str, _width: f32) -> AnyElement {
        let gradient = self.hero_gradient();
        div()
            .parity("discover-hero")
            .relative()
            .w_full()
            .h(px(348.))
            .flex_none()
            .bg(color("--background-base-lower"))
            .child(
                img(gradient)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .object_fit(gpui::ObjectFit::Fill),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(60.))
                    .h(px(288.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .pl(px(32.))
                    .child(
                        // The display face sits 4px lower in a 44px line box than in the official client.
                        text(44., 44., 700., "--text-strong")
                            .relative()
                            .top(px(-4.))
                            .max_w(px(740.))
                            .font_family(super::nitro::NITRO_FONT)
                            .child(title.to_uppercase()),
                    )
                    .child(
                        text(16., 20., 500., "--text-muted")
                            .mt(px(16.))
                            .max_w(px(600.))
                            .child(description.to_owned()),
                    ),
            )
            .into_any_element()
    }

    /// `gradient_-6JdM0`: the radial gradient at 70% opacity over black. GPUI has no radial gradient, so the picture is
    /// computed once at a small size and stretched.
    fn hero_gradient(&self) -> Arc<gpui::Image> {
        let (w, h) = (226u32, 87u32);
        let key = "discover-hero".to_owned();
        if let Some(found) = self.generated_images.borrow().get(&key) {
            return found.clone();
        }
        // radial-gradient(130.66% 324.98% at -3.95% 0, #2f72c1 0, rgba(37,34,154,.8) 35.73%, rgba(25,23,92,0) 100%)
        let stops: [(f32, [f32; 4]); 3] = [
            (0., [47., 114., 193., 1.]),
            (0.3573, [37., 34., 154., 0.8]),
            (1., [25., 23., 92., 0.]),
        ];
        let page = super::theme().color("--background-base-lower");
        let base = [
            (page >> 24) as f32,
            ((page >> 16) & 0xff) as f32,
            ((page >> 8) & 0xff) as f32,
        ];
        let mut pixels = image::RgbaImage::new(w, h);
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            let dx = ((x as f32 + 0.5) / w as f32 + 0.0395) / 1.3066;
            let dy = ((y as f32 + 0.5) / h as f32) / 3.2498;
            let t = (dx * dx + dy * dy).sqrt().min(1.);
            let upper = stops
                .iter()
                .position(|(at, _)| *at >= t)
                .unwrap_or(2)
                .max(1);
            let (from, to) = (stops[upper - 1], stops[upper]);
            let along = ((t - from.0) / (to.0 - from.0)).clamp(0., 1.);
            // Premultiplied interpolation, as CSS does it.
            let mut out = [0u8; 3];
            for i in 0..3 {
                let a = from.1[i] * from.1[3] + (to.1[i] * to.1[3] - from.1[i] * from.1[3]) * along;
                // The layer is 70% opaque over the page colour.
                out[i] = (a * 0.7 + base[i] * 0.3).round().clamp(0., 255.) as u8;
            }
            *pixel = image::Rgba([out[0], out[1], out[2], 255]);
        }
        let mut bytes = Vec::new();
        let _ = image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(&mut bytes),
            pixels.as_raw(),
            w,
            h,
            image::ExtendedColorType::Rgba8,
        );
        let made = Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes));
        self.generated_images.borrow_mut().insert(key, made.clone());
        made
    }
}

/// The scroller's 8px gutter with a 4px thumb (`--scrollbar-auto-thumb`).
pub(super) fn gutter_scrollbar(handle: ScrollHandle) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let max = handle.max_offset().y;
            if max <= px(0.) {
                return;
            }
            let viewport = bounds.size.height;
            let content = viewport + max;
            let thumb = (viewport * (viewport / content)).max(px(40.));
            let scrolled = -handle.offset().y;
            let top = bounds.top() + (viewport - thumb) * (scrolled / max);
            let area = gpui::Bounds::new(point(bounds.right() - px(6.), top), size(px(4.), thumb));
            window.paint_quad(fill(area, color("--scrollbar-auto-thumb")).corner_radii(px(2.)));
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .w(px(GUTTER))
    .h_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_reports_only_a_ready_value() {
        assert_eq!(Load::Ready(3).ready(), Some(&3));
        assert_eq!(Load::<u8>::Loading.ready(), None);
        assert_eq!(Load::<u8>::Failed.ready(), None);
    }
}
