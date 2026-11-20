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
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, MouseButton,
    PathPromptOptions, Pixels, Point, SharedString, Subscription, Window, deferred, div, img,
    prelude::*, px,
};
use serde_json::{Value, json};

/// `LibraryApplicationFlags` (`hM6` of module 652215, used by module 873079).
pub(super) const LIB_FLAG_HIDDEN: u64 = 1;
pub(super) const LIB_FLAG_PRIVATE: u64 = 2;
pub(super) const LIB_FLAG_OVERLAY_DISABLED: u64 = 4;

/// Context menu state for a library game row (`game-context`, module 873079).
#[derive(Clone, Debug)]
pub(super) struct LibraryGameMenu {
    pub app_id: String,
    pub branch_id: String,
    pub sku_id: Option<String>,
    pub name: String,
    pub position: Point<Pixels>,
    pub installed: bool,
    pub running: bool,
    pub up_to_date: bool,
    pub has_manifest: bool,
    pub has_voice_context: bool,
    pub flags: u64,
    pub has_library_app: bool,
    pub copy_submenu_open: bool,
}

/// First-time-in-session hide confirmation modal state (module 873079 `oB7isi`/`HXfjKt`/`OWjIiV`).
#[derive(Clone, Debug)]
pub(super) struct LibraryHideConfirm {
    pub app_id: String,
    pub branch_id: String,
    pub name: String,
    pub next_flags: u64,
}

/// Official `LIBRARY_APPLICATION_BRANCH` flags update (`59636` `V`, `652215`).
pub(super) fn library_flags_request(app_id: &str, branch_id: &str, flags: u64) -> Request {
    Request::Rest {
        key: Some(format!("library:flags:{app_id}:{branch_id}")),
        method: reqwest::Method::PATCH,
        route: format!("/users/@me/library/{app_id}/{branch_id}"),
        body: Some(json!({ "flags": flags })),
    }
}

/// `persistKey` of module 775228.
const STORE_KEY: &str = "InstallationManagerStore";
const LIBRARY_KEY: &str = "library:applications";
#[cfg(feature = "capture")]
static CAPTURED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
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

/// `ButtonStates` (`w` of module 979604): action state for a game row in Library > My Games.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AppButtonState {
    Play,
    Install,
    Update,
    Paused,
    Locate,
    Uninstalling,
    Queued,
    Downloading,
    UnsupportedOs,
    AddToLibrary,
    PreorderWait,
}

/// All 10 playful uninstalling verbs from module 979604 (`getText` `"uninstalling"`).
const UNINSTALL_VERBS: &[&str] = &[
    "r9wmKn", // Uninstalling
    "6CpimS", // Obliterating
    "ysbNDc", // Vaporizing
    "TLnXx/", // Destroying
    "Qi8mne", // Terminating
    "yvMu3S", // Annihilating
    "PnNUZ/", // Dismantling
    "hU2TEJ", // Deleting
    "YyY51/", // Razing
    "PbHJb3", // Expunging
];

/// Resolves the button state for a library entry (`getButtonState` of module 979604).
fn app_button_state(entry: &Value) -> AppButtonState {
    let flags = entry["flags"].as_u64().unwrap_or(0);
    // `hM6.HIDDEN = 1 << 0`
    if (flags & 1) != 0 {
        return AppButtonState::AddToLibrary;
    }
    if entry["sku"]["preorder_release_at"].is_string()
        || entry["is_preorder"].as_bool().unwrap_or(false)
    {
        return AppButtonState::PreorderWait;
    }
    match entry["action_state"].as_str().unwrap_or_default() {
        "play" => AppButtonState::Play,
        "install" => AppButtonState::Install,
        "update" => AppButtonState::Update,
        "paused" => AppButtonState::Paused,
        "locate" => AppButtonState::Locate,
        "uninstalling" => AppButtonState::Uninstalling,
        "queued" => AppButtonState::Queued,
        "downloading" => AppButtonState::Downloading,
        "unsupported_os" => AppButtonState::UnsupportedOs,
        _ => {
            if cfg!(target_os = "windows") {
                AppButtonState::Install
            } else {
                AppButtonState::UnsupportedOs
            }
        }
    }
}

/// Button text for a library action state (`getText` of module 979604).
fn app_button_label(state: AppButtonState, uninstall_idx: usize) -> &'static str {
    match state {
        AppButtonState::Play => dc_intl::tr("Play"),
        AppButtonState::Install | AppButtonState::UnsupportedOs => dc_intl::t("nL0WvC"),
        AppButtonState::Update => dc_intl::t("tkhwp9"),
        AppButtonState::Paused => dc_intl::t("6EKrh4"),
        AppButtonState::Locate => dc_intl::t("nIj+a0"),
        AppButtonState::Uninstalling => {
            dc_intl::t(UNINSTALL_VERBS[uninstall_idx % UNINSTALL_VERBS.len()])
        }
        AppButtonState::Queued => dc_intl::t("TiJg5l"),
        AppButtonState::Downloading => dc_intl::t("Tdhepf"),
        AppButtonState::PreorderWait => dc_intl::t("xiXyPc"),
        AppButtonState::AddToLibrary => dc_intl::t("r3fwuQ"),
    }
}

/// Optional disabled-state helper text (`render` of module 979604: `Aqe2ZC`, `LBm0A4`/`0/xHFO`, `QUSQIA`).
fn app_button_disabled_reason(entry: &Value, state: AppButtonState) -> Option<String> {
    match state {
        AppButtonState::PreorderWait => entry["sku"]["preorder_release_at"].as_str().map(|date| {
            dc_intl::format(
                "Aqe2ZC",
                &[("date", dc_intl::Arg::Str(std::borrow::Cow::Borrowed(date)))],
            )
        }),
        AppButtonState::UnsupportedOs => {
            let os = dc_intl::t("0/xHFO");
            Some(dc_intl::format(
                "LBm0A4",
                &[(
                    "operatingSystem",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(os)),
                )],
            ))
        }
        AppButtonState::Install if entry["has_no_build"].as_bool().unwrap_or(false) => {
            Some(dc_intl::t("QUSQIA").to_owned())
        }
        _ => None,
    }
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
    /// Local override of an entry's install state for interactive action feedback (module 979604).
    action_overrides: std::collections::BTreeMap<String, AppButtonState>,
    /// Local override of an entry's `LibraryApplicationFlags` after toggling in `game-context` (module 873079).
    flag_overrides: std::collections::BTreeMap<String, u64>,
    /// Open `game-context` menu (module 873079).
    pub(super) game_menu: Option<LibraryGameMenu>,
    /// First-time hide confirmation modal (module 873079).
    pub(super) hide_confirm: Option<LibraryHideConfirm>,
    /// Tracks `hasRemovedLibraryApplicationThisSession` (module 873079).
    removed_this_session: bool,
}

/// A library entry's 64px icon (`getIconURL`).
fn library_icon(entry: &Value) -> Option<String> {
    let app = &entry["application"];
    let id = app["id"].as_str()?;
    let icon = app["icon"].as_str()?;
    Some(format!(
        "https://cdn.discordapp.com/app-icons/{id}/{icon}.webp?size=64"
    ))
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

    /// The library answer: its games' icons are asked for.
    pub(super) fn library_fetched(&mut self, key: &str, body: &Value) -> bool {
        if key != LIBRARY_KEY {
            return false;
        }
        // Capture only: a run's own list stays over the account's answer.
        #[cfg(feature = "capture")]
        {
            if body["capture"] == true {
                CAPTURED.store(true, std::sync::atomic::Ordering::Relaxed);
            } else if CAPTURED.load(std::sync::atomic::Ordering::Relaxed) {
                return true;
            }
        }
        self.fetched.insert(key.to_owned(), body.clone());
        for url in self.library_entries().iter().filter_map(library_icon) {
            if !self.images.contains_key(&url) {
                self.request_image(url.clone(), url);
            }
        }
        true
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

    /// `tS` with the columns of `tT`: Name (the game's icon and name), Platform (`gG4` of the distributor, "N/A"
    /// without one), Last Played ("never played": this client keeps no play statistics yet). UNCONFIRMED: the captured
    /// account owns no games, so the table was never compared with a frame; the sizes are the CSS's (`header_-JvGQh`,
    /// `row_-JvGQh` at `--custom-game-list-row-min-height` 62px, the 30/20/20/30 columns) and the icon size is
    /// assumed. Not drawn: the actions column (privacy, overlay and cloud-sync marks, Play/Install with its dropdown),
    /// sorting, the hover state and the context menu.
    fn library_table(&self, entries: &[Value], cx: &mut Context<Self>) -> AnyElement {
        let header_cell = |label: &'static str, share: f32, first: bool| {
            div()
                .w(gpui::relative(share))
                .when(!first, |this| {
                    this.px(px(12.))
                        .border_l_1()
                        .border_color(color("--border-subtle"))
                })
                .child(dc_intl::tr(label))
        };
        let rows: Vec<AnyElement> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let app = &entry["application"];
                let name = app["name"].as_str().unwrap_or_default().to_owned();
                let picture = library_icon(entry).and_then(|url| self.images.get(&url).cloned());
                let platform = match entry["sku"]["distributor"]
                    .as_str()
                    .or_else(|| entry["distributor"].as_str())
                {
                    Some("steam") => "Steam".to_owned(),
                    Some("battlenet") => "Battle.net".to_owned(),
                    Some("epic") => "Epic".to_owned(),
                    Some("gog") => "GOG".to_owned(),
                    Some("origin") => "Origin".to_owned(),
                    Some("uplay") => "Uplay".to_owned(),
                    Some("twitch") => "Twitch".to_owned(),
                    Some("discord") => "Discord".to_owned(),
                    None if entry["sku"].is_object() => "Discord".to_owned(),
                    _ => dc_intl::tr("N/A").to_string(),
                };
                let cell = |share: f32| {
                    div()
                        .w(gpui::relative(share))
                        .py(px(8.))
                        .pl(px(15.))
                        .pr(px(8.))
                        .text_color(color("--text-muted"))
                };
                let entry_for_menu = entry.clone();
                let row_id = SharedString::from(format!("library-row-{index}"));
                div()
                    .id(row_id)
                    .mx(px(18.))
                    .rounded(px(8.))
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                            this.open_library_game_menu(&entry_for_menu, ev.position, cx);
                        }),
                    )
                    .child(
                        div()
                            .mx(px(8.))
                            .min_h(px(62.))
                            .flex()
                            .items_center()
                            .text_color(color("--text-default"))
                            .when(index > 0, |this| {
                                this.border_t_1().border_color(color("--border-subtle"))
                            })
                            .child(
                                div()
                                    .w(gpui::relative(0.3))
                                    .pl(px(8.))
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_none()
                                            .size(px(32.))
                                            .mr(px(20.))
                                            .rounded(px(8.))
                                            .overflow_hidden()
                                            .bg(color("--background-mod-muted"))
                                            .children(
                                                picture.map(|picture| img(picture).size_full()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(name),
                                    ),
                            )
                            .child(cell(0.2).child(platform))
                            .child(cell(0.2).child(dc_intl::tr("never played")))
                            .child(self.library_action_cell(entry, cx)),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .id("library-games")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .text_size(px(14.))
            .line_height(px(18.))
            .child(
                div()
                    .pt(px(20.))
                    .px(px(24.))
                    .pb(px(8.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-muted"))
                    .child(header_cell("Name", 0.3, true))
                    .child(header_cell("Platform", 0.2, false))
                    .child(header_cell("Last Played", 0.2, false))
                    .child(div().w(gpui::relative(0.3))),
            )
            .children(rows)
            .into_any_element()
    }

    /// `LibraryApplicationButton` cell (module 979604): Play / Install / Update / Paused / Downloading / Queued /
    /// Uninstalling / Locate / Coming Soon / Add to library + progress bar & disabled reason tooltip.
    fn library_action_cell(&self, entry: &Value, cx: &mut Context<Self>) -> AnyElement {
        let app_id = entry["application"]["id"]
            .as_str()
            .unwrap_or("0")
            .to_owned();
        let state = self
            .library
            .action_overrides
            .get(&app_id)
            .copied()
            .unwrap_or_else(|| app_button_state(entry));
        let uninstall_idx = app_id
            .bytes()
            .fold(0usize, |a, b| a.wrapping_add(b as usize));
        let label = app_button_label(state, uninstall_idx);
        let disabled_reason = app_button_disabled_reason(entry, state);
        let progress = entry["progress_percent"]
            .as_f64()
            .map(|p| p.clamp(0., 100.) as f32);
        let is_actionable = matches!(
            state,
            AppButtonState::Install
                | AppButtonState::Update
                | AppButtonState::AddToLibrary
                | AppButtonState::Play
        ) && disabled_reason.is_none();
        let style = match state {
            AppButtonState::Install | AppButtonState::Update | AppButtonState::AddToLibrary
                if is_actionable =>
            {
                "primary"
            }
            AppButtonState::Play => "active",
            _ => "secondary",
        };
        let next_state = match state {
            AppButtonState::Install => AppButtonState::Downloading,
            AppButtonState::Update => AppButtonState::Downloading,
            AppButtonState::AddToLibrary => AppButtonState::Install,
            _ => state,
        };
        let id_click = app_id.clone();
        let entry_overflow = entry.clone();

        div()
            .w(gpui::relative(0.3))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(10.))
            .when_some(disabled_reason, |row, reason| {
                row.child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-muted"))
                        .child(reason),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(4.))
                    .child(Self::library_button(
                        SharedString::from(format!("library-app-cta-{app_id}")),
                        label,
                        style,
                        32.,
                        is_actionable,
                        move |this, _, cx| {
                            if next_state != state {
                                this.library
                                    .action_overrides
                                    .insert(id_click.clone(), next_state);
                                cx.notify();
                            }
                        },
                        cx,
                    ))
                    .when(
                        matches!(
                            state,
                            AppButtonState::Downloading
                                | AppButtonState::Paused
                                | AppButtonState::Uninstalling
                        ),
                        |col| {
                            let pct =
                                progress.unwrap_or(if state == AppButtonState::Uninstalling {
                                    45.
                                } else {
                                    35.
                                });
                            let fill = if state == AppButtonState::Paused {
                                color("--background-mod-strong")
                            } else {
                                color("--status-positive")
                            };
                            col.child(
                                div()
                                    .w(px(100.))
                                    .h(px(4.))
                                    .rounded_full()
                                    .bg(color("--background-mod-subtle"))
                                    .overflow_hidden()
                                    .child(div().h_full().w(px(pct)).bg(fill)),
                            )
                        },
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("library-app-more-{app_id}")))
                    .size(px(32.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("more_horizontal", 18., "--interactive-icon-default"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_library_game_menu(&entry_overflow, ev.position, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    /// Opens the official `game-context` menu for a library row (module 873079).
    pub(super) fn open_library_game_menu(
        &mut self,
        entry: &Value,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let app = &entry["application"];
        let app_id = app["id"].as_str().unwrap_or("0").to_owned();
        let branch_id = entry["branch_id"].as_str().unwrap_or(&app_id).to_owned();
        let sku_id = entry["sku"]["id"]
            .as_str()
            .or_else(|| app["primary_sku_id"].as_str())
            .map(str::to_owned);
        let name = app["name"].as_str().unwrap_or("Game").to_owned();
        let flags = self
            .library
            .flag_overrides
            .get(&app_id)
            .copied()
            .unwrap_or_else(|| entry["flags"].as_u64().unwrap_or(0));
        let state = self
            .library
            .action_overrides
            .get(&app_id)
            .copied()
            .unwrap_or_else(|| app_button_state(entry));
        let installed = entry["installed"].as_bool().unwrap_or(matches!(
            state,
            AppButtonState::Play | AppButtonState::Update
        ));
        let running = entry["running"].as_bool().unwrap_or(false);
        let up_to_date = entry["up_to_date"].as_bool().unwrap_or(installed);
        let has_manifest = entry["has_manifest"].as_bool().unwrap_or(true);
        let has_voice_context = entry["has_voice_context"].as_bool().unwrap_or(false);
        let has_library_app = entry["has_library_app"].as_bool().unwrap_or(true);
        self.library.game_menu = Some(LibraryGameMenu {
            app_id,
            branch_id,
            sku_id,
            name,
            position,
            installed,
            running,
            up_to_date,
            has_manifest,
            has_voice_context,
            flags,
            has_library_app,
            copy_submenu_open: false,
        });
        cx.notify();
    }

    pub(super) fn close_library_game_menu(&mut self) -> bool {
        let mut closed = self.library.game_menu.take().is_some();
        if self.library.hide_confirm.take().is_some() {
            closed = true;
        }
        closed
    }

    fn library_apply_flags(
        &mut self,
        app_id: String,
        branch_id: String,
        next_flags: u64,
        cx: &mut Context<Self>,
    ) {
        self.library
            .flag_overrides
            .insert(app_id.clone(), next_flags);
        if (next_flags & LIB_FLAG_HIDDEN) != 0 {
            self.library.removed_this_session = true;
            self.library
                .action_overrides
                .insert(app_id.clone(), AppButtonState::AddToLibrary);
        } else if self.library.action_overrides.get(&app_id) == Some(&AppButtonState::AddToLibrary)
        {
            self.library
                .action_overrides
                .insert(app_id.clone(), AppButtonState::Install);
        }
        let _ = self
            .requests
            .send(library_flags_request(&app_id, &branch_id, next_flags));
        cx.notify();
    }

    /// Renders the official `game-context` menu (module 873079).
    pub(in crate::ui) fn library_game_menu_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.library.game_menu.clone()?;
        let dev = self.developer_mode();
        let x: f32 = menu.position.x.into();
        let y: f32 = menu.position.y.into();
        let left = (x - 210.).max(12.);
        let top = y.max(48.);

        type MenuAction = Box<dyn Fn(&mut Root, &mut Window, &mut Context<Root>)>;
        let row = |id: SharedString,
                   label: String,
                   checked: Option<bool>,
                   has_chevron: bool,
                   on_click: MenuAction| {
            div()
                .id(id)
                .h(px(32.))
                .px(px(8.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|s| s.bg(color("--brand-500")).text_color(color("--white")))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--interactive-text-default"))
                .child(label)
                .when_some(checked, |r, is_checked| {
                    r.child(
                        div()
                            .size(px(18.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(if is_checked {
                                color("--control-primary-background-default")
                            } else {
                                color("--border-strong")
                            })
                            .bg(if is_checked {
                                color("--control-primary-background-default")
                            } else {
                                gpui::hsla(0., 0., 0., 0.).into()
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(is_checked, |b| {
                                b.child(icon("checkbox_check", 12., "--white"))
                            }),
                    )
                })
                .when(has_chevron, |r| {
                    r.child(icon("chevron_right_20", 14., "--interactive-icon-default"))
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    on_click(this, window, cx);
                }))
                .into_any_element()
        };

        let mut items: Vec<AnyElement> = Vec::new();
        if !menu.has_library_app {
            let id_copy = menu.app_id.clone();
            items.push(row(
                "lib-ctx-copy-app-fallback".into(),
                dc_intl::t("FfCL+6").to_owned(),
                None,
                false,
                Box::new(move |this, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(id_copy.clone()));
                    this.library.game_menu = None;
                    cx.notify();
                }),
            ));
        } else {
            // `K`: Play Game (`XKUw8m`)
            if !menu.running && menu.installed {
                let app_id = menu.app_id.clone();
                items.push(row(
                    "lib-ctx-launch".into(),
                    dc_intl::t("XKUw8m").to_owned(),
                    None,
                    false,
                    Box::new(move |this, _, cx| {
                        this.library
                            .action_overrides
                            .insert(app_id.clone(), AppButtonState::Play);
                        this.library.game_menu = None;
                        cx.notify();
                    }),
                ));
            }
            // `F`: Repair (`CKCcdz`)
            if menu.installed && menu.up_to_date && !menu.running {
                let app_id = menu.app_id.clone();
                items.push(row(
                    "lib-ctx-repair".into(),
                    dc_intl::t("CKCcdz").to_owned(),
                    None,
                    false,
                    Box::new(move |this, _, cx| {
                        this.library
                            .action_overrides
                            .insert(app_id.clone(), AppButtonState::Downloading);
                        this.library.game_menu = None;
                        cx.notify();
                    }),
                ));
            }
            // `Z`: Uninstall (`DwgfOM`) or Install (`N+XVnt`)
            if menu.installed {
                if !menu.running {
                    let app_id = menu.app_id.clone();
                    items.push(row(
                        "lib-ctx-uninstall".into(),
                        dc_intl::t("DwgfOM").to_owned(),
                        None,
                        false,
                        Box::new(move |this, _, cx| {
                            this.library
                                .action_overrides
                                .insert(app_id.clone(), AppButtonState::Uninstalling);
                            this.library.game_menu = None;
                            cx.notify();
                        }),
                    ));
                }
            } else if menu.has_manifest {
                let app_id = menu.app_id.clone();
                items.push(row(
                    "lib-ctx-install".into(),
                    dc_intl::t("N+XVnt").to_owned(),
                    None,
                    false,
                    Box::new(move |this, _, cx| {
                        this.library
                            .action_overrides
                            .insert(app_id.clone(), AppButtonState::Downloading);
                        this.library.game_menu = None;
                        cx.notify();
                    }),
                ));
            }

            // `B`: Hide Playing Status (`fHDNJv`)
            let private_on = (menu.flags & LIB_FLAG_PRIVATE) != 0;
            {
                let app_id = menu.app_id.clone();
                let branch_id = menu.branch_id.clone();
                let next_flags = menu.flags ^ LIB_FLAG_PRIVATE;
                items.push(row(
                    "lib-ctx-private".into(),
                    dc_intl::t("fHDNJv").to_owned(),
                    Some(private_on),
                    false,
                    Box::new(move |this, _, cx| {
                        this.library_apply_flags(app_id.clone(), branch_id.clone(), next_flags, cx);
                        if let Some(m) = this.library.game_menu.as_mut() {
                            m.flags = next_flags;
                        }
                    }),
                ));
            }

            // `Q`: Disable Overlay (`rgZKg2`)
            let overlay_off = (menu.flags & LIB_FLAG_OVERLAY_DISABLED) != 0;
            {
                let app_id = menu.app_id.clone();
                let branch_id = menu.branch_id.clone();
                let next_flags = menu.flags ^ LIB_FLAG_OVERLAY_DISABLED;
                items.push(row(
                    "lib-ctx-overlay".into(),
                    dc_intl::t("rgZKg2").to_owned(),
                    Some(overlay_off),
                    false,
                    Box::new(move |this, _, cx| {
                        this.library_apply_flags(app_id.clone(), branch_id.clone(), next_flags, cx);
                        if let Some(m) = this.library.game_menu.as_mut() {
                            m.flags = next_flags;
                        }
                    }),
                ));
            }

            // `Y`: Create Desktop Shortcut (`T4tecH`) when installed on Windows
            if menu.installed && cfg!(target_os = "windows") {
                items.push(row(
                    "lib-ctx-shortcut".into(),
                    dc_intl::t("T4tecH").to_owned(),
                    None,
                    false,
                    Box::new(|this, _, cx| {
                        this.library.game_menu = None;
                        cx.notify();
                    }),
                ));
            }

            // `q`: Hide Game from Library (`TuJXLx`) / Show Game in Library (`0dnEUJ`)
            let is_hidden = (menu.flags & LIB_FLAG_HIDDEN) != 0;
            if !menu.installed || is_hidden {
                let app_id = menu.app_id.clone();
                let branch_id = menu.branch_id.clone();
                let name = menu.name.clone();
                let next_flags = menu.flags ^ LIB_FLAG_HIDDEN;
                items.push(row(
                    "lib-ctx-hidden".into(),
                    if is_hidden {
                        dc_intl::t("0dnEUJ").to_owned()
                    } else {
                        dc_intl::t("TuJXLx").to_owned()
                    },
                    None,
                    false,
                    Box::new(move |this, _, cx| {
                        this.library.game_menu = None;
                        if is_hidden || this.library.removed_this_session {
                            this.library_apply_flags(
                                app_id.clone(),
                                branch_id.clone(),
                                next_flags,
                                cx,
                            );
                        } else {
                            this.library.hide_confirm = Some(LibraryHideConfirm {
                                app_id: app_id.clone(),
                                branch_id: branch_id.clone(),
                                name: name.clone(),
                                next_flags,
                            });
                            cx.notify();
                        }
                    }),
                ));
            }

            // `z`: Developer Mode Copy ID submenu (`OpuAlK`: `qSHshq`, `g7oyyo`, `hReUdL`)
            if dev {
                items.push(
                    div()
                        .my(px(4.))
                        .h(px(1.))
                        .w_full()
                        .bg(color("--border-subtle"))
                        .into_any_element(),
                );
                items.push(row(
                    "lib-ctx-dev-copy".into(),
                    dc_intl::t("OpuAlK").to_owned(),
                    None,
                    true,
                    Box::new(|this, _, cx| {
                        if let Some(m) = this.library.game_menu.as_mut() {
                            m.copy_submenu_open = !m.copy_submenu_open;
                        }
                        cx.notify();
                    }),
                ));
            }

            // `J`: In-Game Voice Settings (`pOul5/`)
            if menu.has_voice_context {
                items.push(
                    div()
                        .my(px(4.))
                        .h(px(1.))
                        .w_full()
                        .bg(color("--border-subtle"))
                        .into_any_element(),
                );
                items.push(row(
                    "lib-ctx-ingame-voice".into(),
                    dc_intl::t("pOul5/").to_owned(),
                    None,
                    false,
                    Box::new(|this, _, cx| {
                        this.library.game_menu = None;
                        this.open_settings_section("Voice & Video", cx);
                        cx.notify();
                    }),
                ));
            }
        }

        let copy_submenu = (dev && menu.has_library_app && menu.copy_submenu_open).then(|| {
            let app_id = menu.app_id.clone();
            let branch_id = menu.branch_id.clone();
            let sku_id = menu.sku_id.clone();
            let mut sub_items: Vec<AnyElement> = vec![
                row(
                    "lib-ctx-copy-app-id".into(),
                    dc_intl::t("qSHshq").to_owned(),
                    None,
                    false,
                    Box::new(move |this, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(app_id.clone()));
                        this.library.game_menu = None;
                        cx.notify();
                    }),
                ),
                row(
                    "lib-ctx-copy-branch-id".into(),
                    dc_intl::t("g7oyyo").to_owned(),
                    None,
                    false,
                    Box::new(move |this, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(branch_id.clone()));
                        this.library.game_menu = None;
                        cx.notify();
                    }),
                ),
            ];
            if let Some(sku) = sku_id {
                sub_items.push(row(
                    "lib-ctx-copy-sku-id".into(),
                    dc_intl::t("hReUdL").to_owned(),
                    None,
                    false,
                    Box::new(move |this, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(sku.clone()));
                        this.library.game_menu = None;
                        cx.notify();
                    }),
                ));
            }
            div()
                .id("lib-ctx-copy-submenu")
                .occlude()
                .w(px(188.))
                .p(px(6.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(8.)),
                    blur_radius: px(16.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .flex()
                .flex_col()
                .children(sub_items)
        });

        let panel = div()
            .id("library-game-context-menu")
            .occlude()
            .absolute()
            .left(px(left))
            .top(px(top))
            .flex()
            .items_end()
            .gap(px(4.))
            .child(
                div()
                    .w(px(210.))
                    .p(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.28),
                        offset: gpui::point(px(0.), px(8.)),
                        blur_radius: px(16.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .flex()
                    .flex_col()
                    .children(items),
            )
            .children(copy_submenu);

        Some(
            deferred(
                div()
                    .id("library-game-menu-backdrop")
                    .absolute()
                    .inset_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.library.game_menu = None;
                            cx.notify();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, _, _, cx| {
                            this.library.game_menu = None;
                            cx.notify();
                        }),
                    )
                    .child(panel),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// First-time hide game confirmation modal (module 873079: `oB7isi`, `HXfjKt`, `OWjIiV`).
    pub(super) fn library_hide_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let confirm = self.library.hide_confirm.clone()?;
        let subtitle = dc_intl::format(
            "HXfjKt",
            &[(
                "name",
                dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&confirm.name)),
            )],
        );
        let close = |this: &mut Root, _: &mut Window, _: &mut Context<Root>| {
            this.library.hide_confirm = None;
        };
        let app_id = confirm.app_id;
        let branch_id = confirm.branch_id;
        let next_flags = confirm.next_flags;
        let spec = super::modal::Spec {
            id: "library-hide-game",
            title: dc_intl::t("oB7isi").into(),
            subtitle: Some(div().child(subtitle).into_any_element()),
            body: None,
            buttons: vec![
                super::modal::button(
                    "library-hide-confirm",
                    dc_intl::t("OWjIiV"),
                    super::modal::Tone::Primary,
                    move |this, _, cx| {
                        this.library.hide_confirm = None;
                        this.library_apply_flags(app_id.clone(), branch_id.clone(), next_flags, cx);
                    },
                ),
                super::modal::button(
                    "library-hide-cancel",
                    dc_intl::tr("Cancel"),
                    super::modal::Tone::Secondary,
                    close,
                ),
            ],
            close: Box::new(close),
        };
        Some(super::modal::render_small(spec, cx))
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

    #[cfg(feature = "capture")]
    pub(crate) fn script_library_game_menu(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg == "hide" {
            self.library.game_menu = None;
            self.library.hide_confirm = Some(LibraryHideConfirm {
                app_id: "1001".to_owned(),
                branch_id: "2001".to_owned(),
                name: "Hades II".to_owned(),
                next_flags: LIB_FLAG_HIDDEN,
            });
            cx.notify();
            return "library-game-menu: hide".to_owned();
        }
        self.library.hide_confirm = None;
        self.library.game_menu = Some(LibraryGameMenu {
            app_id: "1001".to_owned(),
            branch_id: "2001".to_owned(),
            sku_id: Some("3001".to_owned()),
            name: "Hades II".to_owned(),
            position: gpui::point(px(1080.), px(160.)),
            installed: arg != "uninstalled",
            running: false,
            up_to_date: true,
            has_manifest: true,
            has_voice_context: true,
            flags: LIB_FLAG_PRIVATE,
            has_library_app: arg != "fallback",
            copy_submenu_open: true,
        });
        cx.notify();
        format!("library-game-menu: {arg}")
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
                let shown: Vec<Value> = entries
                    .into_iter()
                    .filter(|entry| {
                        query.is_empty()
                            || entry["application"]["name"]
                                .as_str()
                                .is_some_and(|name| name.to_lowercase().contains(&query))
                    })
                    .collect();
                if shown.is_empty() {
                    self.library_empty()
                } else {
                    self.library_table(&shown, cx)
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

    #[test]
    fn library_game_context_menu_flags_and_labels_match_873079() {
        let Request::Rest {
            method,
            route,
            body,
            ..
        } = library_flags_request("1001", "2002", LIB_FLAG_PRIVATE | LIB_FLAG_OVERLAY_DISABLED)
        else {
            panic!("expected rest");
        };
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/users/@me/library/1001/2002");
        assert_eq!(body, Some(json!({ "flags": 6 })));
        assert_eq!(dc_intl::t("tKobzb"), "Application Actions");
        assert_eq!(dc_intl::t("XKUw8m"), "Play Game");
        assert_eq!(dc_intl::t("CKCcdz"), "Repair");
        assert_eq!(dc_intl::t("DwgfOM"), "Uninstall");
        assert_eq!(dc_intl::t("N+XVnt"), "Install");
        assert_eq!(dc_intl::t("fHDNJv"), "Hide Playing Status");
        assert_eq!(dc_intl::t("rgZKg2"), "Disable Overlay");
        assert_eq!(dc_intl::t("T4tecH"), "Create Desktop Shortcut");
        assert_eq!(dc_intl::t("TuJXLx"), "Hide Game from Library");
        assert_eq!(dc_intl::t("0dnEUJ"), "Show Game in Library");
    }
}
