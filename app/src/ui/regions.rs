//! Region views: the main window's big areas, each its own view, so an animation inside one of them redraws that
//! area and not the whole window.
//!
//! A region holds nothing but a handle to the main view and the area it draws. Its render calls the main view's own
//! build function for the area, so every listener stays bound to the main view. The main view places the region
//! as a cached view of a given size: gpui reuses the area's previous frame (layout result, hit boxes, paint output)
//! until the region is notified. Every notify of the main view notifies all regions (they observe it), so a change of
//! state redraws everything exactly as before; only the animations that name the view they are drawn in
//! (`note_drawn`, `wake`, the region driver of `motion`) leave the other regions alone.

use super::{Root, motion};
use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, EntityId, IntoElement,
    ParentElement, Render, StyleRefinement, Styled, Subscription, WeakEntity, Window, div, list,
    px,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::parity::Parity;

/// The regions the main view is cut into: the areas of fixed size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Titlebar,
    Rail,
    Sidebar,
    Header,
    Messages,
    Members,
    Composer,
}

pub(crate) const KINDS: usize = 7;
const ALL: [Kind; KINDS] = [
    Kind::Titlebar,
    Kind::Rail,
    Kind::Sidebar,
    Kind::Header,
    Kind::Messages,
    Kind::Members,
    Kind::Composer,
];
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub(crate) const NAMES: [&str; KINDS] = [
    "titlebar", "rail", "sidebar", "header", "messages", "members", "composer",
];

static COUNT: [AtomicU64; KINDS] = [const { AtomicU64::new(0) }; KINDS];
static NANOS: [AtomicU64; KINDS] = [const { AtomicU64::new(0) }; KINDS];

/// Renders and nanoseconds spent building each region so far.
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub(crate) fn counters() -> [(u64, u64); KINDS] {
    let mut out = [(0, 0); KINDS];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = (
            COUNT[index].load(Ordering::Relaxed),
            NANOS[index].load(Ordering::Relaxed),
        );
    }
    out
}

/// Counts one render of a region and the time until it is dropped.
struct RenderTimer {
    kind: Kind,
    started: std::time::Instant,
}

impl Drop for RenderTimer {
    fn drop(&mut self) {
        let index = self.kind as usize;
        COUNT[index].fetch_add(1, Ordering::Relaxed);
        NANOS[index].fetch_add(self.started.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}

thread_local! {
    /// The regions' view ids, and the main view's, for the wakes that name no view.
    static IDS: RefCell<(Option<EntityId>, Vec<EntityId>)> = const { RefCell::new((None, Vec::new())) };
    /// The views animated things were drawn in since the last wake (`None`: drawn where no view was known).
    static DRAWN: RefCell<HashSet<Option<EntityId>>> = RefCell::new(HashSet::new());
}

/// Notifies every region (the main view's observers do the same on its own notify).
pub(crate) fn notify_all(cx: &mut App) {
    let ids = IDS.with(|ids| ids.borrow().1.clone());
    for id in ids {
        cx.notify(id);
    }
}

/// Redraws the main view, and with it every region, on the next frame: for a measure taken while painting that
/// changes what the regions build.
pub(crate) fn notify_root_next_frame(window: &mut Window) {
    if let Some(root) = IDS.with(|ids| ids.borrow().0) {
        window.on_next_frame(move |_, cx| cx.notify(root));
    }
}

/// Called where something animated is built (a playing sticker, a GIF tile): the view being built now is where it
/// is drawn, and the next wake of its timer redraws that view.
pub(crate) fn note_drawn() {
    let view = motion::current_view();
    DRAWN.with(|drawn| drawn.borrow_mut().insert(view));
}

/// A frame timer of an animated thing fired: redraws the views it was drawn in and nothing else. When it was drawn
/// where no view was known, or nowhere, the whole window is redrawn.
pub(crate) fn wake(cx: &mut App) {
    let drawn: Vec<Option<EntityId>> = DRAWN.with(|drawn| drawn.borrow_mut().drain().collect());
    let root = IDS.with(|ids| ids.borrow().0);
    if drawn.is_empty() || drawn.contains(&None) {
        if let Some(root) = root {
            cx.notify(root);
        }
        return;
    }
    for view in drawn.into_iter().flatten() {
        cx.notify(view);
    }
}

/// One area of the main view.
pub(crate) struct Region {
    root: WeakEntity<Root>,
    kind: Kind,
    _observing: Subscription,
}

impl Region {
    fn new(root: &Entity<Root>, kind: Kind, cx: &mut Context<Self>) -> Self {
        Self {
            root: root.downgrade(),
            kind,
            // Whatever the main view notifies, every region redraws.
            _observing: cx.observe(root, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for Region {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _timer = RenderTimer {
            kind: self.kind,
            started: std::time::Instant::now(),
        };
        let cached = enabled(self.kind);
        if cached {
            motion::begin_view(cx.entity_id());
        } else {
            motion::leave_view();
        }
        let kind = self.kind;
        let content = match self.root.upgrade() {
            Some(root) => root.update(cx, |root, cx| root.render_region(kind, window, cx)),
            None => div().into_any_element(),
        };
        motion::leave_view();
        if !cached {
            return content;
        }
        // The wrapper gives the area's own element the cached size to fill.
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(content)
            .child(motion::region_driver())
            .into_any_element()
    }
}

/// The region views of one main view.
#[derive(Default)]
pub(crate) struct Regions {
    views: Vec<Entity<Region>>,
    composer: std::rc::Rc<ComposerSize>,
}

/// The composer form's height: what it measured last paint and what the main view laid its region out at.
#[derive(Default)]
pub(crate) struct ComposerSize {
    measured: std::cell::Cell<f32>,
    used: std::cell::Cell<f32>,
}

/// The form's height before it has been measured: its 58px box and the 8px under it.
const COMPOSER_GUESS: f32 = 58. + 8.;

impl Regions {
    /// Creates the views at the first render.
    pub(crate) fn ensure(&mut self, root: &Entity<Root>, cx: &mut Context<Root>) {
        if !self.views.is_empty() {
            return;
        }
        self.views = ALL
            .iter()
            .map(|kind| cx.new(|cx| Region::new(root, *kind, cx)))
            .collect();
        let ids = self.views.iter().map(Entity::entity_id).collect();
        IDS.with(|cell| *cell.borrow_mut() = (Some(root.entity_id()), ids));
    }

    /// The area as a cached view laid out at `style`; an empty element before `ensure`.
    pub(crate) fn place(&self, kind: Kind, style: StyleRefinement) -> AnyElement {
        match self.views.get(kind as usize) {
            Some(view) if !enabled(kind) => AnyView::from(view.clone()).into_any_element(),
            Some(view) => AnyView::from(view.clone()).cached(style).into_any_element(),
            None => div().into_any_element(),
        }
    }
}

/// The sizes the main view lays the regions out at (each is the size of the area's own root element).
pub(crate) fn style_of(kind: Kind) -> StyleRefinement {
    let style = StyleRefinement::default();
    match kind {
        // `title_LnjVAX`: the bar across the window.
        Kind::Titlebar => style.flex_none().w_full().h(px(32.)),
        // `wrapper_5dqHTG` of the server rail.
        Kind::Rail => style.flex_none().w(px(72.)).h_full(),
        // The channel list and the direct message list, default 303 wide.
        Kind::Sidebar => style_of_sidebar(303.),
        // `.title_f75fb0`: 48 plus its 1px bottom border.
        Kind::Header => style.flex_none().w_full().h(px(49.)),
        Kind::Messages => style.flex_1().min_h(px(0.)).w_full(),
        // The member list, 264 wide.
        Kind::Members => style.flex_none().w(px(264.)).h_full(),
        // The height is measured, see `Regions::place_composer`.
        Kind::Composer => style.flex_none().w_full(),
    }
}

/// Width-aware layout style for the resizable channel / DM sidebar region.
pub(crate) fn style_of_sidebar(sidebar_width: f32) -> StyleRefinement {
    StyleRefinement::default()
        .flex_none()
        .w(px(sidebar_width))
        .h_full()
}

impl Root {
    /// Builds the area `kind` with the main view's own function for it.
    pub(super) fn render_region(
        &mut self,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if enabled(kind) {
            // What this area laid out for the keyboard last time is laid out again below.
            self.keys.stops.begin_region(motion::current_view());
        }
        if matches!(kind, Kind::Members | Kind::Messages) {
            // The member list (beside a channel, or inside a forum) asks for the rows in view.
            self.member_list_frame(cx);
        }
        match kind {
            Kind::Titlebar => self.titlebar(cx).into_any_element(),
            Kind::Rail => self.rail(cx).into_any_element(),
            Kind::Sidebar if self.in_favorites() => self.favorites_sidebar(cx).into_any_element(),
            Kind::Sidebar if self.guild.is_some() => self.sidebar(cx).into_any_element(),
            Kind::Sidebar => self.home_sidebar(cx).into_any_element(),
            Kind::Header => self.header(cx).into_any_element(),
            Kind::Members => self.member_panel(cx).into_any_element(),
            Kind::Composer => self.composer_bar(cx).into_any_element(),
            Kind::Messages if hidden("messages") => div().into_any_element(),
            Kind::Messages => {
                let rows = list(
                    self.list.clone(),
                    cx.processor(|this, index, window, cx| {
                        #[cfg(feature = "capture")]
                        let _row = crate::perf::Timer::row();
                        // The rows are built while the list is prepared, after the region's own render ended.
                        if enabled(Kind::Messages) {
                            motion::enter_view(window.current_view());
                        }
                        let row = this.row(index, cx);
                        motion::leave_view();
                        row
                    }),
                )
                .size_full();
                // Module 891522 `lQ` while the first page loads (`chat_skeleton.rs`).
                let skeleton = self.chat_skeleton(f32::from(window.viewport_size().height));
                div()
                    .parity("messages")
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.messages_entry(cx))
                    .child(rows)
                    .child(self.chat_thumb())
                    .children(skeleton)
                    .children(self.new_messages_bar(cx))
                    .children(self.jump_to_present_bar(cx))
                    .into_any_element()
            }
        }
    }
}

/// Which regions are cached views. All are off by default: each still needs its own run of the use checks (typing,
/// hover, switching, popouts, dialogs, settings, incoming messages, scrolling, resize). The capture build can turn them
/// on (`GRAVECORD_REGIONS=all`, or a list such as `rail,sidebar`; `0` turns all off) to measure, and to compare a run
/// against the same areas drawn as part of the main view.
const CACHED: [bool; KINDS] = [false; KINDS];

fn enabled(kind: Kind) -> bool {
    #[cfg(feature = "capture")]
    {
        static ON: std::sync::OnceLock<[bool; KINDS]> = std::sync::OnceLock::new();
        ON.get_or_init(|| match std::env::var("GRAVECORD_REGIONS").as_deref() {
            Ok("1" | "all") => [true; KINDS],
            Ok("0") => [false; KINDS],
            Ok(list) => {
                let mut on = [false; KINDS];
                for name in list.split(',') {
                    if let Some(index) = NAMES.iter().position(|known| *known == name) {
                        on[index] = true;
                    }
                }
                on
            }
            Err(_) => CACHED,
        })[kind as usize]
    }
    #[cfg(not(feature = "capture"))]
    CACHED[kind as usize]
}

/// Whether the capture build was told to leave `part` out (`GRAVECORD_HIDE=composer,user_panel,messages`), to measure
/// what the rest of the window costs a frame without it.
pub(crate) fn hidden(part: &str) -> bool {
    #[cfg(feature = "capture")]
    {
        static LIST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        LIST.get_or_init(|| std::env::var("GRAVECORD_HIDE").unwrap_or_default())
            .split(',')
            .any(|item| item == part)
    }
    #[cfg(not(feature = "capture"))]
    {
        let _ = part;
        false
    }
}

impl Regions {
    /// The composer as a region laid out at its measured height (the form overlaps the list by 16px, which the
    /// region's box does not include), or `None` when the composer is drawn inline.
    pub(crate) fn place_composer(&self) -> Option<AnyElement> {
        let view = self.views.get(Kind::Composer as usize)?;
        if !enabled(Kind::Composer) {
            return None;
        }
        let measured = self.composer.measured.get();
        let height = if measured > 0. {
            measured
        } else {
            COMPOSER_GUESS
        };
        self.composer.used.set(height);
        let style = style_of(Kind::Composer).h(px(height - 16.));
        Some(AnyView::from(view.clone()).cached(style).into_any_element())
    }

    /// Measures the composer form while it paints. A height that differs from the one the region was laid out at
    /// redraws the main view, which lays the region out again at the new height.
    pub(crate) fn composer_probe(&self) -> impl IntoElement {
        let size = self.composer.clone();
        gpui::canvas(
            move |bounds, window, _| {
                let height = f32::from(bounds.size.height);
                size.measured.set(height);
                if (height - size.used.get()).abs() > 0.01
                    && size.used.get() > 0.
                    && let Some(root) = IDS.with(|ids| ids.borrow().0)
                {
                    window.on_next_frame(move |_, cx| cx.notify(root));
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

/// Redraws the main view and every region: for a change of how things look that no state of the main view carries
/// (a global such as the theme, the saturation, the zoom or a reduced-motion switch). The regions reuse their last
/// frame until they are notified, so whatever changes a global the regions read calls this.
pub(crate) fn invalidate_all(cx: &mut App) {
    if let Some(root) = IDS.with(|ids| ids.borrow().0) {
        cx.notify(root);
    }
    notify_all(cx);
}

thread_local! {
    /// What the globals the regions read looked like at the last render of the main view.
    static LOOK: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Called each render of the main view with a key of the globals the regions read (theme, saturation, zoom, motion
/// preference, window activity, high contrast, the client theme). A key that differs from the last redraws every
/// region on the next frame, so a global that changed without a notify of the main view is never left stale.
pub(crate) fn sync_look(key: u64, window: &mut Window) {
    let before = LOOK.with(|look| look.replace(Some(key)));
    if before.is_some_and(|before| before != key) {
        window.on_next_frame(|_, cx| invalidate_all(cx));
    }
}

/// Whether a harness draw leaves the regions' cached frames alone (`GRAVECORD_NATURAL_DRAW=1`), as the window's own
/// frames do, instead of forcing every view to render again.
#[cfg(feature = "capture")]
pub(crate) fn natural_draw() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("GRAVECORD_NATURAL_DRAW").as_deref() == Ok("1"))
}

/// What the harness does before it draws a frame by hand. By default every view renders again. A natural draw
/// redraws only the views the window itself would: the dirty ones, and the ones that asked for another frame (the
/// next-frame callbacks a window runs between two draws are not run by a hand draw, so they are notified here).
#[cfg(feature = "capture")]
pub(crate) fn prepare_hand_draw(window: &mut Window, cx: &mut App) {
    if !natural_draw() {
        window.refresh();
        return;
    }
    let (views, everywhere) = motion::animating_views();
    for view in views {
        cx.notify(view);
    }
    if everywhere {
        invalidate_all(cx);
    }
}
