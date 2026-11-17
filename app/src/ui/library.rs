//! The Library (`/library`, module 813662 `eJ`): the header (`K`: the inventory icon, "Library", My Games and
//! Settings as top pills, the Filter field on My Games), My Games (`ep`) and its empty states (`renderImportHelp`: "No
//! games found" over the wumpus picture, with "Hm, seems you don't have any games…" or, with a filter, "After looking
//! far and wide…"), and Settings (`ez`): the two shortcut switches of the settings proto (`gameLibrary`
//! `installShortcutDesktop`, off unless set, and `installShortcutStartMenu`, on unless set) and the install locations
//! (`eG`/`eV`, `InstallationManagerStore`): each with its disk use ring (`eD`), its label (`Main` for the default
//! `%LOCALAPPDATA%\DiscordGames`), the Default tag and the path, which rolls to the free space on hover; Edit opens the
//! row as a card with the name, the read-only location, "Make this my default location", Remove Location (asks
//! first), Cancel and Save; Add Install Location asks for a folder. The locations live in the local store under
//! `InstallationManagerStore`, as the official persisted store does.
//!
//! Measured in `design/dom/library/{empty,filter-focus,settings,settings-metadata,settings-edit}-darker-cozy.json`.
//! Not here yet: the games table (`tS`: rows with art, platform, last played, Play/Install and the overflow menu, sort,
//! hidden games): the captured account owns no games, so no row was ever captured.
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, PathPromptOptions,
    SharedString, Subscription, Window, div, img, prelude::*, px,
};
use serde_json::{Value, json};

/// `persistKey` of module 775228.
const STORE_KEY: &str = "InstallationManagerStore";
const LIBRARY_KEY: &str = "library:applications";
/// The empty-state picture (`emptyWumpus_-JvGQh` in a dark theme), 404 by 202.
const WUMPUS: &str = "https://discord.com/assets/b584f76d3ff86700.svg";
/// `Math.ceil(2 * Math.PI * 20)`: the ring's dash length.
const RING: f32 = 126.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Games,
    Settings,
}

/// `eV` while editing: the label typed so far and the default box.
pub(super) struct Editing {
    path: String,
    label: Entity<Editor>,
    default: bool,
    _events: Subscription,
}

#[derive(Default)]
pub(super) struct Library {
    pub open: bool,
    pub tab: Tab,
    filter: Option<(Entity<Editor>, Subscription)>,
    query: String,
    editing: Option<Editing>,
    /// The location whose "Remove Location" question is up.
    removing: Option<String>,
}

/// `u`: the default location.
fn default_path() -> String {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
    format!("{base}\\DiscordGames")
}

/// `getLabelFromPath`: "Main" for the default location, else the folder's name.
fn label_from_path(path: &str) -> String {
    if path == default_path() {
        return dc_intl::tr("Main").to_string();
    }
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("?")
        .to_owned()
}

/// The store's state: the locations in order, their labels and the default one (`initialize`).
#[derive(Clone, Debug, PartialEq)]
struct Locations {
    paths: Vec<String>,
    labels: serde_json::Map<String, Value>,
    default: String,
}

impl Locations {
    fn from_value(value: Option<Value>) -> Self {
        let value = value.unwrap_or(Value::Null);
        let default = value["defaultInstallationPath"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(default_path);
        let mut paths: Vec<String> = value["installationPaths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|path| path.as_str().map(str::to_owned))
            .collect();
        if paths.is_empty() {
            paths.push(default.clone());
        }
        Self {
            paths,
            labels: value["pathLabels"].as_object().cloned().unwrap_or_default(),
            default,
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "installationPaths": self.paths,
            "pathLabels": self.labels,
            "defaultInstallationPath": self.default,
        })
    }

    fn stored_label(&self, path: &str) -> Option<String> {
        self.labels
            .get(path)
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    fn label(&self, path: &str) -> String {
        self.labels
            .get(path)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| label_from_path(path))
    }

    /// `INSTALLATION_LOCATION_ADD`.
    fn add(&mut self, path: String) {
        if !self.paths.contains(&path) {
            self.paths.push(path);
        }
    }

    /// `INSTALLATION_LOCATION_REMOVE`: never the default one.
    fn remove(&mut self, path: &str) {
        if path == self.default {
            return;
        }
        self.paths.retain(|known| known != path);
        self.labels.remove(path);
    }

    /// `INSTALLATION_LOCATION_UPDATE`: a non-empty new label, and the default when asked.
    fn update(&mut self, path: &str, label: &str, default: bool) {
        if !self.paths.iter().any(|known| known == path) {
            return;
        }
        if !label.is_empty() && self.labels.get(path).and_then(Value::as_str) != Some(label) {
            self.labels
                .insert(path.to_owned(), Value::String(label.to_owned()));
        }
        if default {
            self.default = path.to_owned();
        }
    }
}

/// `Xq` with `useKibibytes` alone: whole MB up to a GB, then whole GB, with no space ("87GB").
fn size_text(kb: u64) -> String {
    let mb = kb.div_ceil(1024);
    if mb < 1024 {
        format!("{mb}MB")
    } else {
        format!("{}GB", (mb as f64 / 1024.).round() as u64)
    }
}

/// The free and total kilobytes of the drive holding `path`.
#[cfg(windows)]
fn disk_space(path: &str) -> Option<(u64, u64)> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::HSTRING;
    // The location may not exist yet: ask about its drive.
    let root: String = path.chars().take(3).collect();
    let (mut free, mut total) = (0u64, 0u64);
    unsafe {
        GetDiskFreeSpaceExW(
            &HSTRING::from(root),
            Some(&mut free),
            Some(&mut total),
            None,
        )
    }
    .ok()?;
    Some((free / 1024, total / 1024))
}

#[cfg(not(windows))]
fn disk_space(_path: &str) -> Option<(u64, u64)> {
    None
}

/// `eD`: the used share, its percentage text and the arc's colour.
fn usage(free_kb: u64, total_kb: u64) -> (f32, String, gpui::Rgba) {
    let used = total_kb.saturating_sub(free_kb) as f32;
    let share = if total_kb == 0 {
        0.
    } else {
        (used / total_kb as f32).clamp(0., 1.)
    };
    let channel = |slope: f32, base: f32| (slope * share + base).round() / 255.;
    let tint = gpui::Rgba {
        r: channel(-85., 199.),
        g: channel(-71., 208.),
        b: channel(-22., 240.),
        a: 1.,
    };
    (share, format!("{}%", (share * 100.).floor() as u32), tint)
}

fn paint_ring(
    bounds: gpui::Bounds<gpui::Pixels>,
    share: f32,
    tint: gpui::Rgba,
    track: gpui::Hsla,
    window: &mut Window,
) {
    let cx = bounds.origin.x + bounds.size.width * 0.5;
    let cy = bounds.origin.y + bounds.size.height * 0.5;
    // `svg_bFFeGy` turns the circle by -86deg; SVG circles start at three o'clock and run clockwise.
    let start = (-86f32).to_radians();
    let arc = |from: f32, sweep: f32, tint: gpui::Hsla, window: &mut Window| {
        let segments = ((sweep / std::f32::consts::TAU * 96.).ceil() as usize).max(2);
        let mut path = gpui::PathBuilder::stroke(px(4.));
        for step in 0..=segments {
            let angle = from + sweep * (step as f32 / segments as f32);
            let (sin, cos) = angle.sin_cos();
            let at = gpui::point(cx + px(20. * cos), cy + px(20. * sin));
            if step == 0 {
                path.move_to(at);
            } else {
                path.line_to(at);
            }
        }
        if let Ok(built) = path.build() {
            window.paint_path(built, tint);
        }
    };
    arc(start, std::f32::consts::TAU, track, window);
    // `strokeDasharray: floor(126 * share), 126` over the 2πr circumference.
    let dash = (RING * share).floor() / (std::f32::consts::TAU * 20.);
    if dash > 0. {
        arc(
            start,
            std::f32::consts::TAU * dash.min(1.),
            tint.into(),
            window,
        );
    }
}

impl Root {
    /// `/library`: the page opens on My Games and asks for the account's library.
    pub(super) fn open_library(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let from = self.route();
        self.history.visit(
            from,
            super::history::Route {
                library: true,
                ..Default::default()
            },
        );
        self.set_library_open(tab, cx);
    }

    /// The part of `open_library` that history replays also run.
    pub(super) fn set_library_open(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.favorites_view = false;
        self.discover.open = false;
        self.shop.open = false;
        self.quests.open = false;
        self.message_requests.open = false;
        self.nitro_open = false;
        self.guild_guide_open = false;
        self.channel_browser_open = false;
        self.guild = None;
        self.channel = None;
        self.unread_from = None;
        self.stop_lurking_elsewhere();
        self.library.open = true;
        self.library.tab = tab;
        self.library.editing = None;
        if self.requested.insert(LIBRARY_KEY.to_owned()) {
            let _ = self.requests.send(Request::Rest {
                key: Some(LIBRARY_KEY.to_owned()),
                method: reqwest::Method::GET,
                route: "/users/@me/library".to_owned(),
                body: None,
            });
        }
        if !self.images.contains_key(WUMPUS) {
            self.request_image(WUMPUS.to_owned(), WUMPUS.to_owned());
        }
        self.rebuild_rows();
        cx.notify();
    }

    pub(super) fn library_active(&self) -> bool {
        self.library.open
            && self.guild.is_none()
            && self.channel.is_none()
            && !self.nitro_open
            && !self.discover.open
            && !self.shop.open
            && !self.quests.open
            && !self.message_requests.open
    }

    /// The account's library entries (`GET /users/@me/library`), empty until the answer is in.
    fn library_entries(&self) -> Vec<Value> {
        self.fetched
            .get(LIBRARY_KEY)
            .filter(|answer| answer["ok"] == true)
            .and_then(|answer| answer["body"].as_array().cloned())
            .unwrap_or_default()
    }

    fn library_locations(&self) -> Locations {
        Locations::from_value(crate::storage::get(STORE_KEY))
    }

    fn save_library_locations(locations: &Locations) {
        crate::storage::set(STORE_KEY, locations.to_value());
    }

    fn library_filter_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.library.filter.is_some() {
            return;
        }
        let field = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder(dc_intl::tr("Filter").to_string());
            editor
        });
        let events = cx.subscribe(&field, |this, editor, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited) {
                this.library.query = editor.read(cx).text().to_owned();
                cx.notify();
            }
        });
        window.focus(&field.read(cx).focus_handle(cx), cx);
        self.library.filter = Some((field, events));
    }

    /// `K`: the header bar, like the Friends one.
    fn library_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.library.tab;
        let tab = |id: &'static str, label: &'static str, which: Tab| {
            let active = current == which;
            div()
                .id(id)
                .mx(px(8.))
                .h(px(32.))
                .px(px(12.))
                .py(px(4.))
                .rounded(px(8.))
                .cursor_pointer()
                .flex()
                .items_center()
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .when(active, |this| {
                    this.bg(color("--background-mod-strong"))
                        .text_color(color("--text-strong"))
                })
                .when(!active, |this| {
                    this.text_color(color("--interactive-text-default"))
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.library.tab != which {
                        this.open_library(which, cx);
                    }
                }))
                .child(dc_intl::tr(label))
        };
        // `libraryFilter_JpdZpJ`: a 144 by 40 search field with the magnifier in a 30px box.
        let filter = (current == Tab::Games).then(|| {
            let field = self.library.filter.as_ref().map(|(field, _)| field.clone());
            div()
                .id("library-filter")
                .w(px(144.))
                .h(px(40.))
                .mr(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .flex()
                .items_center()
                .gap(px(4.))
                .pl(px(4.))
                .pr(px(10.))
                .cursor_text()
                .on_click(cx.listener(|this, _, window, cx| {
                    this.library_filter_field(window, cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(30.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("search", 16., "--icon-muted")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .overflow_hidden()
                        .map(|this| match field {
                            Some(field) => this.child(field),
                            None => this
                                .text_color(color("--input-placeholder-text-default"))
                                .child(dc_intl::tr("Filter")),
                        }),
                )
        });
        div()
            .flex_none()
            .h(px(49.))
            .border_b_1()
            .border_color(color("--background-mod-subtle"))
            .pl(px(22.))
            .pr(px(8.))
            .flex()
            .items_center()
            .bg(color("--background-base-lower"))
            .child(icon("inventory", 20., "--channels-default"))
            .child(
                div()
                    .ml(px(6.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr("Library")),
            )
            .child(
                div()
                    .ml(px(12.))
                    .size(px(4.))
                    .rounded_full()
                    .bg(color("--background-mod-strong")),
            )
            .child(div().flex_none().w(px(4.)))
            .child(tab("library-games", "My Games", Tab::Games))
            .child(tab("library-settings", "Settings", Tab::Settings))
            .child(div().flex_1())
            .children(filter)
            .into_any_element()
    }

    /// `renderImportHelp`: the wumpus, "No games found" and the line under it.
    fn library_empty(&self) -> AnyElement {
        let picture = self.images.get(WUMPUS).cloned();
        let description = if self.library.query.is_empty() {
            div()
                .child(dc_intl::tr(
                    "Hm, seems you don't have any games. Purchase games and they'll show up here!",
                ))
                .into_any_element()
        } else {
            // `4Q1TAr`: the query in `$b` (strong).
            let lead = "After looking far and wide, we couldn't find any games matching ";
            let line = format!("{lead}{}...", self.library.query);
            let bold = lead.len()..lead.len() + self.library.query.len();
            div()
                .child(gpui::StyledText::new(line).with_highlights([(
                    bold,
                    gpui::HighlightStyle {
                        font_weight: Some(FontWeight(700.)),
                        ..Default::default()
                    },
                )]))
                .into_any_element()
        };
        div()
            .flex_1()
            .min_h(px(0.))
            .pb(px(20.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex_none()
                    .w(px(404.))
                    .h(px(202.))
                    .children(picture.map(|picture| img(picture).w(px(404.)).h(px(202.)))),
            )
            .child(
                div()
                    .mt(px(20.))
                    .mx(px(20.))
                    .text_center()
                    .child(
                        div()
                            .h(px(24.))
                            .mb(px(8.))
                            .text_size(px(24.))
                            .line_height(px(24.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::tr("No games found")),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(description),
                    ),
            )
            .into_any_element()
    }

    /// `eD`: the 50px ring with the used share in its middle.
    fn library_ring(path: &str) -> Option<AnyElement> {
        let (free, total) = disk_space(path)?;
        let (share, text, tint) = usage(free, total);
        let track: gpui::Hsla = color("--text-muted").into();
        Some(
            div()
                .relative()
                .flex_none()
                .size(px(50.))
                .child(
                    gpui::canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| paint_ring(bounds, share, tint, track, window),
                    )
                    .size_full(),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .top(px(19.))
                        .w(px(30.))
                        .text_center()
                        .text_size(px(12.))
                        .line_height(px(12.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(text),
                )
                .into_any_element(),
        )
    }

    fn library_button(
        id: SharedString,
        label: &'static str,
        style: &'static str,
        height: f32,
        enabled: bool,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let (background, border, text) = match style {
            "primary" => (
                "--control-primary-background-default",
                "--control-primary-border-default",
                "--control-primary-text-default",
            ),
            "critical-secondary" => (
                "--control-critical-secondary-background-default",
                "--control-critical-secondary-border-default",
                "--control-critical-secondary-text-default",
            ),
            "active" => (
                "--control-connected-background-default",
                "--control-connected-border-default",
                "--control-connected-text-default",
            ),
            _ => (
                "--control-secondary-background-default",
                "--control-secondary-border-default",
                "--control-secondary-text-default",
            ),
        };
        div()
            .id(id)
            .flex_none()
            .h(px(height))
            .px(px(if height > 32. { 16. } else { 12. }))
            .rounded(px(8.))
            .border_1()
            .border_color(color(border))
            .bg(color(background))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color(text))
            .when(!enabled, |this| this.opacity(0.5))
            .when(enabled, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
            })
            .child(dc_intl::tr(label))
    }

    fn library_start_editing(
        &mut self,
        path: String,
        locations: &Locations,
        cx: &mut Context<Self>,
    ) {
        let label = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder(label_from_path(&path));
            // `value: label`: the stored label only; "Main" or the folder name is the placeholder.
            if let Some(label) = locations.stored_label(&path) {
                editor.set_text(&label);
            }
            editor
        });
        let events = cx.subscribe(&label, |_, _, _: &EditorEvent, cx| cx.notify());
        self.library.editing = Some(Editing {
            default: locations.default == path,
            path,
            label,
            _events: events,
        });
        cx.notify();
    }

    /// `eV.renderViewing`.
    fn library_location_row(
        &self,
        path: &str,
        locations: &Locations,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editing_any = self.library.editing.is_some();
        let path_owned = path.to_owned();
        let hover = disk_space(path)
            .map(|(free, total)| format!("{} available of {}", size_text(free), size_text(total)));
        let roll = SharedString::from(format!("library-path-{path}"));
        div()
            .py(px(20.))
            .border_b_1()
            .border_color(color("--primary-500"))
            .flex()
            .items_center()
            .children(Self::library_ring(path).map(|ring| div().mr(px(10.)).child(ring)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(locations.label(path)),
                            )
                            .when(locations.default == path, |this| {
                                this.child(
                                    div()
                                        .ml(px(8.))
                                        .h(px(14.))
                                        .px(px(4.))
                                        .rounded(px(3.))
                                        .bg(color("--primary-500"))
                                        .flex()
                                        .items_center()
                                        .text_size(px(11.))
                                        .line_height(px(14.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--white"))
                                        .child(dc_intl::tr("Default").to_uppercase()),
                                )
                            }),
                    )
                    // `hoverRoll_Dq3ch1`: the path, and on hover the free space in its place.
                    .child(
                        div()
                            .id(roll)
                            .group("library-path")
                            .mt(px(4.))
                            .h(px(16.))
                            .overflow_hidden()
                            .relative()
                            .text_size(px(14.))
                            .line_height(px(16.))
                            .text_color(color("--primary-400"))
                            .child(
                                div()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .overflow_hidden()
                                    .when(hover.is_some(), |this| {
                                        this.group_hover("library-path", |style| style.opacity(0.))
                                    })
                                    .child(path.to_owned()),
                            )
                            .children(hover.map(|text| {
                                div()
                                    .absolute()
                                    .inset_0()
                                    .opacity(0.)
                                    .whitespace_nowrap()
                                    .group_hover("library-path", |style| style.opacity(1.))
                                    .child(text)
                            })),
                    ),
            )
            .child(Self::library_button(
                SharedString::from(format!("library-edit-{path}")),
                "Edit",
                "secondary",
                32.,
                !editing_any,
                move |this, _, cx| {
                    let locations = this.library_locations();
                    this.library_start_editing(path_owned.clone(), &locations, cx);
                },
                cx,
            ))
            .into_any_element()
    }

    /// `eV.renderEditing`: the row as a card.
    fn library_location_card(
        &self,
        editing: &Editing,
        locations: &Locations,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = editing.path.clone();
        let is_default = locations.default == path;
        let typed = editing.label.read(cx).text().to_owned();
        // Save is off while nothing differs from the stored label and default (`n === props.label`).
        let changed = typed != locations.stored_label(&path).unwrap_or_default()
            || editing.default != is_default;
        let field = |content: AnyElement| {
            div()
                .h(px(40.))
                .px(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .flex()
                .items_center()
                .text_size(px(16.))
                .line_height(px(20.))
                .child(content)
        };
        let label = |text: &'static str| {
            div()
                .h(px(20.))
                .mb(px(8.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(dc_intl::tr(text))
        };
        let remove_path = path.clone();
        div()
            .mx(px(-20.))
            .p(px(19.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--card-background-default"))
            .flex()
            .children(Self::library_ring(&path).map(|ring| div().mr(px(10.)).child(ring)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(label("Install Location Name"))
                    .child(field(editing.label.clone().into_any_element()))
                    .child(div().mt(px(20.)).child(label("Location")))
                    .child(field(
                        div()
                            .text_color(color("--text-muted"))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(path.clone())
                            .into_any_element(),
                    ))
                    .child(
                        div()
                            .id("library-default-location")
                            .mt(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .when(is_default, |this| this.opacity(0.5))
                            .when(!is_default, |this| {
                                this.cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(editing) = this.library.editing.as_mut() {
                                            editing.default = !editing.default;
                                        }
                                        cx.notify();
                                    }))
                            })
                            .child(super::server_games::checkbox(editing.default))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::tr("Make this my default location")),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(20.))
                            .mb(px(20.))
                            .h(px(2.))
                            .bg(color("--border-subtle")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(Self::library_button(
                                "library-remove-location".into(),
                                "Remove Location",
                                "critical-secondary",
                                32.,
                                !is_default,
                                move |this, _, cx| {
                                    this.library.removing = Some(remove_path.clone());
                                    cx.notify();
                                },
                                cx,
                            ))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(
                                        Self::library_button(
                                            "library-cancel".into(),
                                            "Cancel",
                                            "secondary",
                                            40.,
                                            true,
                                            |this, _, cx| {
                                                this.library.editing = None;
                                                cx.notify();
                                            },
                                            cx,
                                        )
                                        .w(px(100.)),
                                    )
                                    .child(
                                        Self::library_button(
                                            "library-save".into(),
                                            "Save",
                                            "active",
                                            40.,
                                            changed,
                                            |this, _, cx| this.library_save_location(cx),
                                            cx,
                                        )
                                        .w(px(100.)),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// `handleSaveChanges`.
    fn library_save_location(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.library.editing.take() else {
            return;
        };
        let label = editing.label.read(cx).text().trim().to_owned();
        let mut locations = self.library_locations();
        locations.update(&editing.path, &label, editing.default);
        Self::save_library_locations(&locations);
        cx.notify();
    }

    /// `handleAddInstallationLocation`: a folder, added at the end.
    fn library_add_location(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths
                .first()
                .map(|path| path.to_string_lossy().into_owned())
            else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let mut locations = this.library_locations();
                locations.add(path);
                Self::save_library_locations(&locations);
                cx.notify();
            });
        })
        .detach();
    }

    /// `ez`: the two shortcut switches and the install locations, 660px wide in the middle.
    fn library_settings(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let locations = self.library_locations();
        let desktop = self.pref_on("library_install_shortcut_desktop", false);
        let start_menu = self.pref_on("library_install_shortcut_start_menu", true);
        let divider = || div().h(px(2.)).bg(color("--border-subtle"));
        let rows: Vec<AnyElement> = locations
            .paths
            .iter()
            .map(|path| match &self.library.editing {
                Some(editing) if &editing.path == path => {
                    self.library_location_card(editing, &locations, cx)
                }
                _ => self.library_location_row(path, &locations, cx),
            })
            .collect();
        div()
            .id("library-settings")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .child(
                div()
                    .w(px(660.))
                    .mx_auto()
                    // The scroller keeps a 16px gutter for its bar: the column centres in what is left.
                    .relative()
                    .left(px(-8.))
                    .pt(px(40.))
                    .pb(px(40.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(self.switch_row_for(
                        "library_install_shortcut_desktop",
                        "Create Desktop Shortcut",
                        "When installing games from the Discord Store.",
                        desktop,
                        window,
                        cx,
                    ))
                    .child(divider())
                    .child(self.switch_row_for(
                        "library_install_shortcut_start_menu",
                        "Create Start Menu Shortcut",
                        "When installing games from the Discord Store. Also makes your games discoverable via windows search.",
                        start_menu,
                        window,
                        cx,
                    ))
                    .child(divider())
                    .child(
                        div()
                            .mt(px(60.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(20.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::tr("Install locations")),
                            )
                            // `control_VhFsFg`: 8px under the label, its children 4px apart.
                            .child(div().mt(px(8.)).flex().flex_col().gap(px(4.)).children(rows))
                            .child(
                                // `buttonRowWrapper_LAvacu`: 20px, after the 4px gap of the control.
                                div().mt(px(24.)).flex().justify_end().child(Self::library_button(
                                    "library-add-location".into(),
                                    "Add Install Location",
                                    "primary",
                                    32.,
                                    true,
                                    |this, _, cx| this.library_add_location(cx),
                                    cx,
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The Remove Location question (`eC.A`).
    pub(super) fn library_remove_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let path = self.library.removing.clone()?;
        let close =
            |this: &mut Root, _: &mut Window, _: &mut Context<Root>| this.library.removing = None;
        let spec = super::modal::Spec {
            id: "library-remove-location",
            title: dc_intl::tr("Remove Location").into(),
            subtitle: Some(
                div()
                    .child(dc_intl::tr(
                        "Are you sure you want to remove this installation location?",
                    ))
                    .into_any_element(),
            ),
            body: None,
            buttons: vec![
                super::modal::button(
                    "library-remove-confirm",
                    dc_intl::tr("Remove Location"),
                    super::modal::Tone::Critical,
                    move |this, _, cx| {
                        this.library.removing = None;
                        this.library.editing = None;
                        let mut locations = this.library_locations();
                        locations.remove(&path);
                        Self::save_library_locations(&locations);
                        cx.notify();
                    },
                ),
                super::modal::button(
                    "library-remove-cancel",
                    dc_intl::tr("Cancel"),
                    super::modal::Tone::Secondary,
                    close,
                ),
            ],
            close: Box::new(close),
        };
        Some(super::modal::render_small(spec, cx))
    }

    /// The page in the main area.
    pub(super) fn library_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let body = match self.library.tab {
            Tab::Settings => self.library_settings(window, cx),
            Tab::Games => {
                let entries = self.library_entries();
                let query = self.library.query.to_lowercase();
                let shown = entries
                    .iter()
                    .filter(|entry| {
                        query.is_empty()
                            || entry["application"]["name"]
                                .as_str()
                                .is_some_and(|name| name.to_lowercase().contains(&query))
                    })
                    .count();
                if shown == 0 {
                    self.library_empty()
                } else {
                    // The games table is not built yet (see the module notes): say how many there are rather than
                    // drawing rows nobody has captured.
                    div()
                        .flex_1()
                        .p(px(24.))
                        .text_color(color("--text-muted"))
                        .child(format!("{shown} games"))
                        .into_any_element()
                }
            }
        };
        div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .bg(color("--background-base-lower"))
            .child(self.library_header(cx))
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_start_with_the_default_and_keep_it() {
        let mut locations = Locations::from_value(None);
        assert_eq!(locations.paths, vec![default_path()]);
        assert_eq!(locations.label(&default_path()), "Main");
        locations.add("D:\\Games".to_owned());
        assert_eq!(locations.label("D:\\Games\\"), "Games");
        locations.update("D:\\Games", "Big disk", true);
        assert_eq!(locations.default, "D:\\Games");
        assert_eq!(locations.label("D:\\Games"), "Big disk");
        locations.remove("D:\\Games");
        assert_eq!(locations.paths.len(), 2, "the default location stays");
        let restored = Locations::from_value(Some(locations.to_value()));
        assert_eq!(restored, locations);
    }

    #[test]
    fn sizes_and_usage_read_as_the_official_formatters() {
        assert_eq!(size_text(91_226_112), "87GB");
        assert_eq!(size_text(975_175_680), "930GB");
        assert_eq!(size_text(500), "1MB");
        let (share, text, tint) = usage(10, 100);
        assert!((share - 0.9).abs() < 1e-6);
        assert_eq!(text, "90%");
        assert!((tint.r - (-85. * 0.9_f32 + 199.).round() / 255.).abs() < 1e-6);
    }
}
