//! Frames a provider draws itself inside the chat, as the official list mounts their iframes: today the Spotify embed
//! (module 265705 `K` picks it for a `link` embed of provider "Spotify" on open.spotify.com; module 628137 draws
//! `open.spotify.com/embed/<type>/<id>` 300 to 400 wide, 80 tall for a track, 232 for an episode or show, 352 for an
//! album, artist or playlist).
//!
//! Each frame is a system web view (`dc_webhost::WebHost::player`, transparent, so the provider's rounded corners show
//! the chat behind them) laid over a box of the frame's size. In the official client the iframe is part of the page:
//! it scrolls with the chat, is cut where the chat's scroller ends, moves with the window and sits under the app's
//! menus and dialogs. The web view is a window of its own, so the box tells it where it is from the paint that drew
//! it ([`Painted`]): the box's bounds and the visible part of them (the content mask the chat list paints its rows
//! under), in device pixels, sent the moment they change and never otherwise. The surface's window covers only that
//! visible part and follows the app window by itself (`dc_webhost`). A box that was not drawn in a frame (scrolled out
//! of the list, another channel opened) hides its view at the end of that frame's paint and closes it a few frames
//! later, as the official list unmounts the iframe.
//!
//! A native window cannot be drawn under gpui's layers, so a view steps aside (hidden, its page kept running and never
//! reloaded) while anything of the app's own is drawn over its box: the known full-window layers at once
//! ([`Root::web_surfaces_covered`]), and any other layer from the frame it was drawn in ([`covered_by_layers`]). Each box
//! leaves a hitbox where it is painted; after the frame, the box is covered where a mouse-blocking hitbox drawn above
//! it (every menu, popout, picker and dialog scrim, plugin dialogs included) or a tooltip lies over it.
//!
//! The provider players (`provider_player.rs`) use the same boxes and the same paint-time placement.
use super::Root;
use dc_webhost::{Area, Placement, Placer, WebHost};
use gpui::{
    AnyElement, Bounds, Context, EntityId, Hitbox, HitboxBehavior, IntoElement, Pixels, Point,
    Window, WindowId, canvas, div, point, prelude::*, px,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The spacing of the points a box is tested at for layers over it ([`sample_points`]): its corners, its edges and a
/// grid inside. A layer that overlaps a box's edge for this length or more, holds one of its corners, or lies inside it
/// with both sides this long or more is found; every mouse-blocking layer of the app (menus from 144 wide, popouts,
/// scrims) is.
const SAMPLE_STEP: f32 = 24.;

thread_local! {
    /// Tooltips drawn this frame (they take no pointer, so no hitbox says they are there): their window and box.
    static TIP_COVERS: RefCell<Vec<(WindowId, Bounds<Pixels>)>> = const { RefCell::new(Vec::new()) };
}

/// A tooltip is drawn at `bounds` in `window`: a web view under it steps aside ([`covered_by_layers`]).
pub(super) fn cover(window: WindowId, bounds: Bounds<Pixels>) {
    TIP_COVERS.with(|covers| covers.borrow_mut().push((window, bounds)));
}

/// The points a box's visible part `area` is tested at: its corners, its edges every `step` and a grid of the same
/// spacing inside, each half a pixel in from the edge so it falls inside the box's hitbox.
pub(super) fn sample_points(area: Bounds<Pixels>, step: f32) -> Vec<Point<Pixels>> {
    let (left, top) = (f32::from(area.origin.x), f32::from(area.origin.y));
    let (width, height) = (f32::from(area.size.width), f32::from(area.size.height));
    if width < 1. || height < 1. {
        return Vec::new();
    }
    let line = |start: f32, length: f32| {
        let (first, last) = (start + 0.5, start + length - 0.5);
        let mut at = vec![first];
        let mut next = first + step;
        while next < last {
            at.push(next);
            next += step;
        }
        if last > first {
            at.push(last);
        }
        at
    };
    let xs = line(left, width);
    let ys = line(top, height);
    let mut points = Vec::with_capacity(xs.len() * ys.len());
    for y in &ys {
        for x in &xs {
            points.push(point(px(*x), px(*y)));
        }
    }
    points
}

/// Whether anything of the app's own lies over a box's visible part `area`: a tooltip box that meets it, or a point
/// of it where `reachable` (the box's own hitbox, tested against the drawn frame) is false, so a mouse-blocking layer
/// was drawn above it there.
pub(super) fn covered_by_layers(
    area: Bounds<Pixels>,
    tips: &[Bounds<Pixels>],
    reachable: impl Fn(Point<Pixels>) -> bool,
) -> bool {
    tips.iter().any(|tip| tip.intersects(&area))
        || sample_points(area, SAMPLE_STEP)
            .into_iter()
            .any(|at| !reachable(at))
}

/// The pictures the views took of their pages, drawn in a box while its view is away for a layer over it.
#[derive(Default)]
pub(super) struct Stills {
    /// The picture each box draws while away (BGRA, as gpui keeps them).
    pictures: HashMap<String, std::sync::Arc<gpui::RenderImage>>,
    /// The boxes whose loaded view is away.
    aside: HashSet<String>,
    /// Pictures no box draws any more, whose textures go at the next [`Stills::release`].
    retired: Vec<std::sync::Arc<gpui::RenderImage>>,
}

impl Stills {
    /// The view over `anchor` is `visible` or not. Going away, the box takes the newest picture the view took while
    /// it showed (the view takes none while hidden, so the picture holds still). Returns whether the box's drawing
    /// changed.
    pub(super) fn keep(&mut self, anchor: &str, web: &WebHost, visible: bool) -> bool {
        if visible {
            return self.aside.remove(anchor);
        }
        let newly = self.aside.insert(anchor.to_owned());
        let mut replaced = false;
        if let Some(picture) = web.take_still().and_then(|png| decode_still(&png)) {
            if let Some(old) = self.pictures.insert(anchor.to_owned(), picture) {
                self.retired.push(old);
            }
            replaced = true;
        }
        newly || replaced
    }

    /// What the box `anchor` draws in place of its view, if it is away and has a picture.
    pub(super) fn shown(&self, anchor: &str) -> Option<std::sync::Arc<gpui::RenderImage>> {
        self.aside
            .contains(anchor)
            .then(|| self.pictures.get(anchor).cloned())
            .flatten()
    }

    /// The view over `anchor` closed: its picture goes.
    pub(super) fn forget(&mut self, anchor: &str) {
        self.aside.remove(anchor);
        if let Some(old) = self.pictures.remove(anchor) {
            self.retired.push(old);
        }
    }

    /// Keeps the pictures of the boxes `keep` says.
    fn retain(&mut self, keep: impl Fn(&String) -> bool) {
        self.aside.retain(|anchor| keep(anchor));
        let gone: Vec<String> = self
            .pictures
            .keys()
            .filter(|anchor| !keep(anchor))
            .cloned()
            .collect();
        for anchor in gone {
            self.forget(&anchor);
        }
    }

    /// Frees the textures of the pictures no box draws any more.
    pub(super) fn release(&mut self, cx: &mut gpui::App) {
        for old in self.retired.drain(..) {
            cx.drop_image(old, None);
        }
    }

    #[cfg(feature = "capture")]
    fn counts(&self) -> (usize, usize) {
        (self.pictures.len(), self.aside.len())
    }
}

/// A view's PNG as a picture gpui draws (BGRA).
fn decode_still(png: &[u8]) -> Option<std::sync::Arc<gpui::RenderImage>> {
    let mut pixels = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(std::sync::Arc::new(gpui::RenderImage::new(vec![
        image::Frame::new(pixels),
    ])))
}

/// A view shows when neither a full-window layer of the app (`covered`) nor a layer drawn over its own box (`layered`)
/// is there.
pub(super) fn view_shown(covered: bool, layered: bool) -> bool {
    !covered && !layered
}

/// Views kept alive per channel at once: scrolling up and down within the same channel/DM keeps loaded
/// embeds parked (hidden, WebView2 rendering suspended) instead of reloading them; switching channels
/// tears all of them down immediately. If a channel has more distinct embeds than this, the
/// least-recently-seen hidden frame is evicted to make room for a newly visible one.
const MAX_FRAMES: usize = 16;
/// The page the official iframe sits in.
const REFERER: &str = "https://discord.com/";

struct Frame {
    url: String,
    web: Option<WebHost>,
    /// Shown as far as the app goes (no overlay of its own over it).
    visible: bool,
    /// Monotonic frame counter of the last paint that showed this frame, for LRU eviction when full.
    last_seen: u64,
}

/// What the paint of a frame says about the boxes web views sit over. Shared with the boxes' paint callbacks.
#[derive(Default)]
pub(super) struct Painted {
    /// Boxes built this frame: anchor to the page its frame shows (`None`: a box whose view another owner opens).
    built: HashMap<String, Option<String>>,
    /// Where each box was painted this frame.
    placed: HashMap<String, Placement>,
    /// The open views: anchor to its placer and the placement last sent to it.
    links: HashMap<String, (Placer, Option<Placement>)>,
    /// The last finished frame's boxes and places, read by the next render.
    last_built: HashMap<String, Option<String>>,
    last_placed: HashMap<String, Placement>,
    /// Each box's hitbox this frame with the visible part it is tested over, then the last finished frame's.
    hits: HashMap<String, (Hitbox, Bounds<Pixels>)>,
    last_hits: HashMap<String, (Hitbox, Bounds<Pixels>)>,
    /// The boxes another layer of the app lay over in the last frame tested ([`Painted::test_layers`]).
    pub(super) layered: HashSet<String>,
}

impl Painted {
    /// A box was painted at `placement`: its view moves there now, if it is not there already.
    fn place(&mut self, anchor: &str, placement: Placement) {
        match self.placed.get_mut(anchor) {
            Some(slot) => *slot = placement,
            None => {
                self.placed.insert(anchor.to_owned(), placement);
            }
        }
        if let Some((placer, last)) = self.links.get_mut(anchor)
            && *last != Some(placement)
        {
            placer.place(placement);
            *last = Some(placement);
        }
    }

    /// The frame's paint is over: the views whose boxes were not painted hide now, not at the next frame (which may
    /// not come soon), and the frame's record becomes the last one.
    fn finish(&mut self) {
        for (anchor, (placer, last)) in &mut self.links {
            if self.placed.contains_key(anchor) {
                continue;
            }
            if let Some(placement) = last.filter(|placement| placement.shown.is_some()) {
                let hidden = Placement::hidden(placement.frame);
                placer.place(hidden);
                *last = Some(hidden);
            }
        }
        std::mem::swap(&mut self.built, &mut self.last_built);
        std::mem::swap(&mut self.placed, &mut self.last_placed);
        std::mem::swap(&mut self.hits, &mut self.last_hits);
        self.built.clear();
        self.placed.clear();
        self.hits.clear();
    }

    /// After a frame is drawn (its hitboxes are the window's last rendered frame): which of its boxes another layer
    /// lies over. Returns whether that changed.
    fn test_layers(&mut self, window: &Window) -> bool {
        let id = window.window_handle().window_id();
        let tips: Vec<Bounds<Pixels>> = TIP_COVERS.with(|covers| {
            covers
                .borrow()
                .iter()
                .filter(|(at, _)| *at == id)
                .map(|(_, bounds)| *bounds)
                .collect()
        });
        let layered: HashSet<String> = self
            .last_hits
            .iter()
            .filter(|(_, (hitbox, area))| {
                covered_by_layers(*area, &tips, |at| hitbox.is_hovered_at(at, window))
            })
            .map(|(anchor, _)| anchor.clone())
            .collect();
        let changed = layered != self.layered;
        self.layered = layered;
        changed
    }

    /// Where the box `anchor` was in the last finished frame, if it was built and painted there.
    pub(super) fn last_place(&self, anchor: &str) -> Option<Placement> {
        self.last_built
            .contains_key(anchor)
            .then(|| self.last_placed.get(anchor).copied())
            .flatten()
    }

    /// The view of `anchor` opened: it follows the box from now on, starting where the box was last painted.
    pub(super) fn link(&mut self, anchor: &str, placer: Placer, placement: Placement) {
        self.links
            .insert(anchor.to_owned(), (placer, Some(placement)));
    }

    pub(super) fn unlink(&mut self, anchor: &str) {
        self.links.remove(anchor);
    }
}

/// The placement of a box painted at `bounds` under the content mask `clip` (logical pixels), at `scale` device pixels
/// each (the window's scale factor, which includes the app's zoom).
pub(super) fn placement_of(bounds: Bounds<Pixels>, clip: Bounds<Pixels>, scale: f32) -> Placement {
    let rect = |bounds: Bounds<Pixels>| {
        (
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        )
    };
    Placement {
        frame: Area::logical(rect(bounds), scale),
        shown: Some(Area::logical(rect(clip), scale)),
    }
}

#[derive(Default)]
pub(super) struct WebFrames {
    pub(super) painted: Rc<RefCell<Painted>>,
    /// The pictures the views took of their pages, drawn while a view is away ([`Root::web_frame_still`]).
    pub(super) stills: Stills,
    frames: HashMap<String, Frame>,
    /// The channel/DM whose frames are currently cached; changing it closes all open frames immediately.
    channel: Option<dc_state::Id>,
    /// Monotonic frame counter used to pick the least-recently-seen hidden frame if `MAX_FRAMES` is reached.
    tick: u64,
}

/// Pure channel + LRU slot reconciliation so unit tests exercise the exact same eviction rules as
/// [`Root::web_frames_frame`]. Returns the anchors that were closed and removed.
fn reconcile_slots(
    frames: &mut HashMap<String, Frame>,
    cached_channel: &mut Option<dc_state::Id>,
    current_channel: Option<dc_state::Id>,
    tick: &mut u64,
    last_built: &HashMap<String, Option<String>>,
) -> Vec<String> {
    let mut closed = Vec::new();
    if *cached_channel != current_channel {
        *cached_channel = current_channel;
        closed.extend(frames.drain().map(|(anchor, _)| anchor));
    }
    *tick = tick.wrapping_add(1);
    let now = *tick;
    for (anchor, url) in last_built {
        let Some(url) = url else { continue };
        if let Some(frame) = frames.get_mut(anchor) {
            frame.last_seen = now;
            if &frame.url != url {
                frame.url = url.clone();
                frame.web = None;
                closed.push(anchor.clone());
            }
            continue;
        }
        if frames.len() >= MAX_FRAMES {
            let victim = frames
                .iter()
                .filter(|(k, _)| !last_built.contains_key(*k))
                .min_by_key(|(_, f)| f.last_seen)
                .map(|(k, _)| k.clone());
            if let Some(victim) = victim {
                frames.remove(&victim);
                closed.push(victim);
            } else {
                continue;
            }
        }
        frames.insert(
            anchor.clone(),
            Frame {
                url: url.clone(),
                web: None,
                visible: false,
                last_seen: now,
            },
        );
    }
    closed
}

impl WebFrames {
    /// Records the box `anchor` as built this frame and returns the element that reports where it is painted. Laid
    /// over the box (absolute, its full size).
    pub(super) fn probe(&self, anchor: String, page: Option<String>) -> impl IntoElement {
        self.painted.borrow_mut().built.insert(anchor.clone(), page);
        let painted = self.painted.clone();
        let hit_painted = self.painted.clone();
        let hit_anchor = anchor.clone();
        canvas(
            move |bounds, window, _| {
                // A hitbox that takes nothing from the pointer: after the frame, a point of the box where it is not
                // among the hovered ones has a mouse-blocking layer over it.
                let area = bounds.intersect(&window.content_mask().bounds);
                if area.size.width > px(0.) && area.size.height > px(0.) {
                    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                    hit_painted
                        .borrow_mut()
                        .hits
                        .insert(hit_anchor.clone(), (hitbox, area));
                }
            },
            move |bounds, _, window, _| {
                let placement =
                    placement_of(bounds, window.content_mask().bounds, window.scale_factor());
                painted.borrow_mut().place(&anchor, placement);
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Painted after everything else in the window's main tree: ends the frame's record ([`Painted::finish`]) and,
    /// while a box is drawn, tests the finished frame for layers over the boxes before the next frame, notifying
    /// `root` when that changed so its render shows or hides their views.
    pub(super) fn sweep(&self, root: EntityId) -> impl IntoElement {
        let painted = self.painted.clone();
        canvas(
            // The deferred layers (tooltips among them) are laid out after the main tree.
            |_, _, _| TIP_COVERS.with(|covers| covers.borrow_mut().clear()),
            move |_, _, window, _| {
                let mut record = painted.borrow_mut();
                record.finish();
                if record.last_hits.is_empty() && record.layered.is_empty() {
                    return;
                }
                drop(record);
                let painted = painted.clone();
                window.on_next_frame(move |window, cx| {
                    if painted.borrow_mut().test_layers(window) {
                        cx.notify(root);
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_0()
    }
}

/// `utm_source=discord&utm_medium=desktop` (module 272984 `A`).
const UTM: &str = "?utm_source=discord&utm_medium=desktop";

/// The Spotify player for an embed, with its height, when the official client would draw one (265705 `K` and the
/// test before it): a `link` embed whose provider is "Spotify" at open.spotify.com, `/intl-xx/` left out of the path,
/// `<type>/<id>` or `user/<name>/playlist/<id>`.
pub(super) fn spotify_frame(embed: &dc_state::Embed) -> Option<(String, f32)> {
    if embed.kind != "link" || embed.provider.as_deref() != Some("Spotify") {
        return None;
    }
    let url = embed.url.as_deref()?;
    let parsed = reqwest::Url::parse(url).ok()?;
    if parsed.host_str() != Some("open.spotify.com") {
        return None;
    }
    let parts: Vec<&str> = parsed
        .path()
        .split('/')
        .filter(|part| !part.is_empty() && !part.starts_with("intl-"))
        .collect();
    let (kind, id) = match parts.as_slice() {
        [user, _, playlist, id, ..] if user.eq_ignore_ascii_case("user") => {
            if !playlist.eq_ignore_ascii_case("playlist") {
                return None;
            }
            ("playlist", *id)
        }
        [kind, id, ..] => (*kind, *id),
        _ => return None,
    };
    let kind = kind.to_ascii_lowercase();
    // 628137 `e`: the frame's height by resource type (`M0` of module 272984).
    let height = match kind.as_str() {
        "track" => 80.,
        "episode" | "show" => 232.,
        "album" | "artist" | "playlist" => 352.,
        _ => return None,
    };
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    Some((
        format!("https://open.spotify.com/embed/{kind}/{id}{UTM}"),
        height,
    ))
}

impl Root {
    /// The boxes of the last frame and those a layer lay over, with each loaded view's state (the `webframes` step).
    #[cfg(feature = "capture")]
    pub(crate) fn script_web_layers(&self) -> String {
        let painted = self.web_frames.painted.borrow();
        let mut boxes: Vec<String> = painted
            .last_hits
            .iter()
            .map(|(anchor, (_, area))| {
                format!(
                    "{anchor}@{:.0},{:.0},{:.0}x{:.0}{}",
                    f32::from(area.origin.x),
                    f32::from(area.origin.y),
                    f32::from(area.size.width),
                    f32::from(area.size.height),
                    if painted.layered.contains(anchor) {
                        " layered"
                    } else {
                        " clear"
                    }
                )
            })
            .collect();
        boxes.sort();
        let mut views: Vec<String> = self
            .web_frames
            .frames
            .iter()
            .map(|(anchor, frame)| {
                // The surface's own window as Win32 last left it: the same handle before and after a layer means the
                // page was hidden and shown again, never reloaded.
                let surface = frame.web.as_ref().map(WebHost::shown).unwrap_or_default();
                format!(
                    "{anchor} open={} shown={} window={:#x} on_screen={}",
                    frame.web.is_some(),
                    frame.visible,
                    surface.window,
                    surface.visible
                )
            })
            .collect();
        views.sort();
        format!(
            "web layers: covered={} boxes=[{}] views=[{}] stills={} aside={}
",
            self.web_surfaces_covered(),
            boxes.join("; "),
            views.join("; "),
            self.web_frames.stills.counts().0,
            self.web_frames.stills.counts().1
        )
    }

    /// The chat list's viewport, in the window's logical pixels (the `webframes` harness step).
    #[cfg(feature = "capture")]
    pub(crate) fn script_chat_viewport(&self) -> gpui::Bounds<gpui::Pixels> {
        self.list.viewport_bounds()
    }

    /// The box a frame is drawn over: `maxWidth 400, minWidth 300, width 100%` and the frame's height.
    pub(super) fn web_frame_box(&self, key: String, url: String, height: f32) -> AnyElement {
        let width = self.content_width.clamp(300., 400.);
        let anchor = format!("webframe:{key}");
        div()
            .relative()
            .w(px(width))
            .h(px(height))
            .children(self.web_frame_still(&anchor))
            .child(self.web_frames.probe(anchor, Some(url)))
            .into_any_element()
    }

    /// What a box draws while its view is away for a layer over it: the page as the view last showed it, when it gave
    /// a picture of it. The box keeps its size either way, so nothing in the chat moves.
    pub(super) fn web_frame_still(&self, anchor: &str) -> Option<AnyElement> {
        let still = self.web_frames.stills.shown(anchor)?;
        Some(
            gpui::img(still)
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .into_any_element(),
        )
    }

    /// Something of the app's own is over the chat (a menu, a popout, a picker, a dialog, settings, a full-window
    /// layer): the web views, which are windows above the app's own drawing, step aside, as the official iframe goes
    /// under the layer. The same test serves every web view laid over the chat.
    /// Whether the view over the box `anchor` shows: nothing of the app's own over the chat (`covered`, from
    /// [`Self::web_surfaces_covered`]) nor over the box itself ([`covered_by_layers`]).
    pub(super) fn web_surface_shown(&self, anchor: &str, covered: bool) -> bool {
        view_shown(
            covered,
            self.web_frames.painted.borrow().layered.contains(anchor),
        )
    }

    pub(super) fn web_surfaces_covered(&self) -> bool {
        self.modal_open() > 0
            || self.any_layer_open()
            || self.lightbox.is_some()
            || self.game_profile.is_some()
            || self.settings.is_some()
            || self.profile.is_some()
            || self.full_profile.is_some()
            || self.server_settings.is_some()
            || self.channel_settings.is_some()
            || crate::intl::load_error().is_some()
    }

    /// Called every frame: opens, shows, hides and closes the views from where their boxes were drawn in the last
    /// frame. Where a view sits is not set here: the box's paint sends it ([`Painted::place`]).
    ///
    /// Frames stay alive (hidden and suspended by WebView2) while scrolling within the same channel/DM so
    /// scrolling back never reloads Spotify embeds; switching channels tears them all down immediately.
    pub(super) fn web_frames_frame(&mut self, window: &Window, cx: &mut Context<Self>) {
        let current_channel = self.channel;
        // A chat without frames costs a frame nothing here.
        if self.web_frames.frames.is_empty()
            && !self
                .web_frames
                .painted
                .borrow()
                .last_built
                .values()
                .any(Option::is_some)
        {
            self.web_frames.channel = current_channel;
            self.web_frames
                .stills
                .retain(|anchor| !anchor.starts_with("webframe:"));
            self.web_frames.stills.release(cx);
            return;
        }
        let covered = self.web_surfaces_covered();
        let painted = self.web_frames.painted.clone();
        let mut painted = painted.borrow_mut();
        let evicted = reconcile_slots(
            &mut self.web_frames.frames,
            &mut self.web_frames.channel,
            current_channel,
            &mut self.web_frames.tick,
            &painted.last_built,
        );
        for anchor in evicted {
            painted.unlink(&anchor);
        }
        let mut failed = Vec::new();
        for (anchor, frame) in self.web_frames.frames.iter_mut() {
            let visible = view_shown(covered, painted.layered.contains(anchor));
            let Some(placement) = painted.last_place(anchor) else {
                // Scrolled out of the viewport within the same channel: Painted::finish already hid
                // the popup and suspended WebView2 rendering; keep the frame parked until the user
                // scrolls back or switches channels.
                if !visible
                    && frame.visible
                    && let Some(web) = frame.web.as_ref()
                {
                    web.set_visible(false);
                    frame.visible = false;
                }
                continue;
            };
            match frame.web.as_ref() {
                None => {
                    let Some(dir) = crate::storage::data_dir().map(|dir| dir.join("webview"))
                    else {
                        continue;
                    };
                    match WebHost::player(window, dir, placement, &frame.url, Some(REFERER)) {
                        Ok(web) => {
                            web.set_visible(visible);
                            painted.link(anchor, web.placer(), placement);
                            frame.web = Some(web);
                            frame.visible = visible;
                            tracing::debug!("web frame opened");
                        }
                        Err(error) => {
                            tracing::warn!("web frame could not open: {error}");
                            failed.push(anchor.clone());
                        }
                    }
                }
                Some(web) => {
                    if visible != frame.visible {
                        web.set_visible(visible);
                        frame.visible = visible;
                    }
                }
            }
        }
        for anchor in failed {
            painted.unlink(&anchor);
            self.web_frames.frames.remove(&anchor);
        }
        drop(painted);
        // A box whose view is away draws the picture the view last took.
        let WebFrames { frames, stills, .. } = &mut self.web_frames;
        let mut redraw = false;
        for (anchor, frame) in frames.iter() {
            if let Some(web) = frame.web.as_ref() {
                redraw |= stills.keep(anchor, web, frame.visible);
            }
        }
        stills.retain(|anchor| !anchor.starts_with("webframe:") || frames.contains_key(anchor));
        stills.release(cx);
        if redraw {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    fn spotify(kind: &str, provider: &str, url: &str) -> dc_state::Embed {
        dc_state::Embed {
            kind: kind.to_owned(),
            provider: Some(provider.to_owned()),
            url: Some(url.to_owned()),
            ..Default::default()
        }
    }

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(width), px(height)),
        }
    }

    /// A mouse-blocking layer at `layer`: the box's hitbox is not reached at the points under it.
    fn blocked_by(layers: Vec<Bounds<Pixels>>) -> impl Fn(Point<Pixels>) -> bool {
        move |at| !layers.iter().any(|layer| layer.contains(&at))
    }

    #[test]
    fn a_view_steps_aside_for_any_layer_over_its_box_and_only_then() {
        // A Spotify album box, 400 by 352, in the chat.
        let player = bounds(400., 200., 400., 352.);
        let shown = |layers: Vec<Bounds<Pixels>>, tips: &[Bounds<Pixels>]| {
            view_shown(false, covered_by_layers(player, tips, blocked_by(layers)))
        };
        // Nothing over it: shown.
        assert!(shown(vec![], &[]));
        // A dialog's scrim (the VoiceMessages dialog of 31.png): the whole window.
        assert!(!shown(vec![bounds(0., 0., 1600., 900.)], &[]));
        // A context menu (144 wide) dropped over the box's middle, or a popout over its right half.
        assert!(!shown(vec![bounds(500., 300., 144., 60.)], &[]));
        assert!(!shown(vec![bounds(700., 100., 340., 500.)], &[]));
        // A menu that only grazes a corner, or an edge for a few pixels.
        assert!(!shown(vec![bounds(790., 540., 200., 200.)], &[]));
        assert!(!shown(vec![bounds(450., 548., 188., 100.)], &[]));
        // A menu beside the box, touching nothing of it: the player stays (the official iframe shows round it).
        assert!(shown(vec![bounds(810., 200., 188., 300.)], &[]));
        // A tooltip, which takes no pointer, over its top edge.
        assert!(!shown(vec![], &[bounds(560., 180., 80., 30.)]));
        assert!(shown(vec![], &[bounds(560., 150., 80., 30.)]));
        // A full-window layer the app knows of hides it whatever the hitboxes say.
        assert!(!view_shown(true, false));
    }

    #[test]
    fn the_test_points_cover_the_box_s_corners_edges_and_inside() {
        let area = bounds(10., 20., 100., 50.);
        let points = sample_points(area, 24.);
        for corner in [
            point(px(10.5), px(20.5)),
            point(px(109.5), px(20.5)),
            point(px(10.5), px(69.5)),
            point(px(109.5), px(69.5)),
        ] {
            assert!(points.contains(&corner), "{corner:?}");
        }
        assert!(points.iter().all(|at| area.contains(at)));
        // No two neighbours further apart than the step.
        let mut xs: Vec<f32> = points.iter().map(|at| f32::from(at.x)).collect();
        xs.sort_by(f32::total_cmp);
        xs.dedup();
        assert!(xs.windows(2).all(|pair| pair[1] - pair[0] <= 24.));
        // A box scrolled out of sight has nothing to test.
        assert!(sample_points(bounds(0., 0., 0., 50.), 24.).is_empty());
    }

    #[test]
    fn a_box_scrolled_under_the_header_shows_only_below_the_chat_s_top() {
        // 63.png: the chat list starts at 98 (titlebar 32, header 49 and its border); a track box 80 tall starts 30
        // above it at 125% scale. The surface covers only the 50 logical pixels under the list's top.
        let chat = bounds(312., 98., 900., 600.);
        let placement = placement_of(bounds(384., 68., 400., 80.), chat, 1.25);
        let laid = dc_webhost::layout(placement, (2000, 1200)).expect("partly visible");
        assert_eq!(laid.window.top, 123);
        assert_eq!(laid.window.bottom, 185);
        assert_eq!(laid.view.top, -38);
        assert_eq!(laid.view.height(), 100);
        // Scrolled all the way under the header: nothing shows.
        let gone = placement_of(bounds(384., 10., 400., 80.), chat, 1.25);
        assert_eq!(dc_webhost::layout(gone, (2000, 1200)), None);
    }

    #[test]
    fn a_box_moves_its_view_once_per_change_and_hides_when_not_painted() {
        let mut painted = Painted::default();
        let chat = bounds(0., 0., 1000., 800.);
        let at = |y: f32| placement_of(bounds(100., y, 400., 352.), chat, 1.);
        let frame = |painted: &mut Painted, y: Option<f32>| {
            if let Some(y) = y {
                painted.built.insert("webframe:a".into(), Some("u".into()));
                painted.place("webframe:a", at(y));
            }
            painted.finish();
        };
        // Built and painted, no view yet: only recorded for the next render, which opens one there.
        frame(&mut painted, Some(100.));
        assert_eq!(painted.last_place("webframe:a"), Some(at(100.)));
        let (placer, sent) = dc_webhost::Placer::recording();
        painted.link("webframe:a", placer, at(100.));
        // Painted where it was: nothing is sent; an idle chat costs the view nothing.
        frame(&mut painted, Some(100.));
        assert!(sent.take().is_empty());
        // Scrolled: sent once, in the paint that moved it (not at the next render).
        frame(&mut painted, Some(60.));
        assert_eq!(sent.take(), vec![at(60.)]);
        // Not built (scrolled out of the list or another channel): hidden at the end of that paint, once.
        frame(&mut painted, None);
        assert_eq!(sent.take(), vec![Placement::hidden(at(60.).frame)]);
        assert_eq!(painted.last_place("webframe:a"), None);
        frame(&mut painted, None);
        assert!(sent.take().is_empty());
        // Back in view: placed again.
        frame(&mut painted, Some(200.));
        assert_eq!(sent.take(), vec![at(200.)]);
    }

    #[test]
    fn spotify_links_get_the_official_player_and_height() {
        let frame = |url: &str| spotify_frame(&spotify("link", "Spotify", url));
        assert_eq!(
            frame("https://open.spotify.com/album/5WrpCQmbTUZuzNxGCPUFSV"),
            Some((
                "https://open.spotify.com/embed/album/5WrpCQmbTUZuzNxGCPUFSV?utm_source=discord&utm_medium=desktop"
                    .to_owned(),
                352.
            ))
        );
        assert_eq!(
            frame("https://open.spotify.com/track/abc?si=1").map(|f| f.1),
            Some(80.)
        );
        assert_eq!(
            frame("https://open.spotify.com/intl-de/track/abc").map(|f| f.1),
            Some(80.)
        );
        assert_eq!(
            frame("https://open.spotify.com/episode/abc").map(|f| f.1),
            Some(232.)
        );
        assert_eq!(
            frame("https://open.spotify.com/user/me/playlist/abc").map(|f| f.0),
            Some(
                "https://open.spotify.com/embed/playlist/abc?utm_source=discord&utm_medium=desktop"
                    .to_owned()
            )
        );
        // Not a player: other hosts, providers, embed types, unknown kinds, odd ids.
        assert!(frame("https://example.com/album/abc").is_none());
        assert!(frame("https://open.spotify.com/genre/abc").is_none());
        assert!(frame("https://open.spotify.com/user/me").is_none());
        assert!(frame("https://open.spotify.com/album/a%22b").is_none());
        assert!(
            spotify_frame(&spotify(
                "article",
                "Spotify",
                "https://open.spotify.com/album/abc"
            ))
            .is_none()
        );
        assert!(
            spotify_frame(&spotify(
                "link",
                "Other",
                "https://open.spotify.com/album/abc"
            ))
            .is_none()
        );
    }

    #[test]
    fn same_channel_scroll_keeps_frames_alive_and_channel_switch_clears_them() {
        let mut frames = HashMap::new();
        let mut cached_channel = None;
        let mut tick = 0;

        let mut built = HashMap::new();
        built.insert(
            "webframe:m1:0".to_owned(),
            Some("https://open.spotify.com/embed/track/1".to_owned()),
        );
        let evicted = reconcile_slots(
            &mut frames,
            &mut cached_channel,
            Some(100),
            &mut tick,
            &built,
        );
        assert!(evicted.is_empty());
        assert!(frames.contains_key("webframe:m1:0"));

        // Scroll far away within the same channel for 120 frames: the frame stays cached instead of reloading.
        let empty = HashMap::new();
        for _ in 0..120 {
            let evicted = reconcile_slots(
                &mut frames,
                &mut cached_channel,
                Some(100),
                &mut tick,
                &empty,
            );
            assert!(evicted.is_empty());
            assert!(frames.contains_key("webframe:m1:0"));
        }

        // Switching to another DM/channel clears all frames from the previous channel immediately.
        let evicted = reconcile_slots(
            &mut frames,
            &mut cached_channel,
            Some(200),
            &mut tick,
            &empty,
        );
        assert_eq!(evicted, vec!["webframe:m1:0".to_owned()]);
        assert!(frames.is_empty());
        assert_eq!(cached_channel, Some(200));
    }

    #[test]
    fn exceeding_max_frames_in_one_channel_evicts_least_recently_seen_hidden_frame() {
        let mut frames = HashMap::new();
        let mut cached_channel = Some(42);
        let mut tick = 0;

        for i in 0..MAX_FRAMES {
            let mut built = HashMap::new();
            built.insert(
                format!("webframe:m{i}:0"),
                Some(format!("https://open.spotify.com/embed/track/{i}")),
            );
            let evicted = reconcile_slots(
                &mut frames,
                &mut cached_channel,
                Some(42),
                &mut tick,
                &built,
            );
            assert!(evicted.is_empty());
        }
        assert_eq!(frames.len(), MAX_FRAMES);

        // One more distinct embed scrolls into view: the oldest hidden embed (m0) is evicted.
        let mut built = HashMap::new();
        built.insert(
            "webframe:m_new:0".to_owned(),
            Some("https://open.spotify.com/embed/track/new".to_owned()),
        );
        let evicted = reconcile_slots(
            &mut frames,
            &mut cached_channel,
            Some(42),
            &mut tick,
            &built,
        );
        assert_eq!(evicted, vec!["webframe:m0:0".to_owned()]);
        assert!(!frames.contains_key("webframe:m0:0"));
        assert!(frames.contains_key("webframe:m_new:0"));
        assert_eq!(frames.len(), MAX_FRAMES);
    }
}
