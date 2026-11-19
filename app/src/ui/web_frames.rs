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
//! later, as the official list unmounts the iframe; while one of the app's own menus, popouts or dialogs is open the
//! views step aside ([`Root::web_surfaces_covered`]), since a native window cannot be drawn under gpui's layers.
//!
//! The provider players (`provider_player.rs`) use the same boxes and the same paint-time placement.
use super::Root;
use dc_webhost::{Area, Placement, Placer, WebHost};
use gpui::{AnyElement, Bounds, Context, IntoElement, Pixels, Window, canvas, div, prelude::*, px};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Frames the box may go undrawn before its view closes (a frame or two can pass while a list re-lays out).
const GRACE_FRAMES: u32 = 6;
/// Views open at once at most: a channel full of Spotify links opens the ones on screen, not one per link.
const MAX_FRAMES: usize = 8;
/// The page the official iframe sits in.
const REFERER: &str = "https://discord.com/";

struct Frame {
    url: String,
    web: Option<WebHost>,
    /// Shown as far as the app goes (no overlay of its own over it).
    visible: bool,
    unseen: u32,
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
        self.built.clear();
        self.placed.clear();
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
    frames: HashMap<String, Frame>,
}

impl WebFrames {
    /// Records the box `anchor` as built this frame and returns the element that reports where it is painted. Laid
    /// over the box (absolute, its full size).
    pub(super) fn probe(&self, anchor: String, page: Option<String>) -> impl IntoElement {
        self.painted.borrow_mut().built.insert(anchor.clone(), page);
        let painted = self.painted.clone();
        canvas(
            move |bounds, window, _| {
                let placement =
                    placement_of(bounds, window.content_mask().bounds, window.scale_factor());
                painted.borrow_mut().place(&anchor, placement);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Painted after everything else in the window: ends the frame's record ([`Painted::finish`]).
    pub(super) fn sweep(&self) -> impl IntoElement {
        let painted = self.painted.clone();
        canvas(
            |_, _, _| {},
            move |_, _, _, _| painted.borrow_mut().finish(),
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
    /// The chat list's viewport, in the window's logical pixels (the `webframes` harness step).
    #[cfg(feature = "capture")]
    pub(crate) fn script_chat_viewport(&self) -> gpui::Bounds<gpui::Pixels> {
        self.list.viewport_bounds()
    }

    /// The box a frame is drawn over: `maxWidth 400, minWidth 300, width 100%` and the frame's height.
    pub(super) fn web_frame_box(&self, key: String, url: String, height: f32) -> AnyElement {
        let width = self.content_width.clamp(300., 400.);
        div()
            .relative()
            .w(px(width))
            .h(px(height))
            .child(self.web_frames.probe(format!("webframe:{key}"), Some(url)))
            .into_any_element()
    }

    /// Something of the app's own is over the chat (a menu, a popout, a picker, a dialog, settings, a full-window
    /// layer): the web views, which are windows above the app's own drawing, step aside, as the official iframe goes
    /// under the layer. The same test serves every web view laid over the chat.
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
    pub(super) fn web_frames_frame(&mut self, window: &Window, _cx: &mut Context<Self>) {
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
            return;
        }
        let covered = self.web_surfaces_covered();
        let painted = self.web_frames.painted.clone();
        let mut painted = painted.borrow_mut();
        let frames = &mut self.web_frames.frames;
        for (anchor, url) in &painted.last_built {
            let Some(url) = url else { continue };
            let open = frames.len();
            if let Some(frame) = frames.get_mut(anchor) {
                if &frame.url != url {
                    frame.url = url.clone();
                    frame.web = None;
                }
            } else if open < MAX_FRAMES {
                frames.insert(
                    anchor.clone(),
                    Frame {
                        url: url.clone(),
                        web: None,
                        visible: false,
                        unseen: 0,
                    },
                );
            }
        }
        let mut closed = Vec::new();
        for (anchor, frame) in frames.iter_mut() {
            let Some(placement) = painted.last_place(anchor) else {
                frame.unseen += 1;
                if frame.unseen > GRACE_FRAMES {
                    closed.push(anchor.clone());
                }
                continue;
            };
            frame.unseen = 0;
            let visible = !covered;
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
                            closed.push(anchor.clone());
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
        for anchor in closed {
            painted.unlink(&anchor);
            frames.remove(&anchor);
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
}
