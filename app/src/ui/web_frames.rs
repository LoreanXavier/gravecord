//! Frames a provider draws itself inside the chat, as the official list mounts their iframes: today the Spotify embed
//! (module 265705 `K` picks it for a `link` embed of provider "Spotify" on open.spotify.com; module 628137 draws
//! `open.spotify.com/embed/<type>/<id>` 300 to 400 wide, 80 tall for a track, 232 for an episode or show, 352 for an
//! album, artist or playlist).
//!
//! Each frame is a system web view (`dc_webhost::WebHost::player`, transparent, so the provider's rounded corners show
//! the chat behind them) laid over a box of the frame's size. The box records its bounds every frame; the view follows
//! it as the chat scrolls, steps aside while a menu or dialog is over the chat, and closes once its box has gone undrawn
//! for a few frames (scrolled out of the list or another channel opened), as the official list unmounts the iframe.
use super::Root;
use dc_webhost::WebHost;
use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*, px};
use std::cell::RefCell;
use std::collections::HashMap;

/// Frames the box may go undrawn before its view closes (a frame or two can pass while a list re-lays out).
const GRACE_FRAMES: u32 = 6;
/// Views open at once at most: a channel full of Spotify links opens the ones on screen, not one per link.
const MAX_FRAMES: usize = 8;
/// The page the official iframe sits in.
const REFERER: &str = "https://discord.com/";

struct Frame {
    url: String,
    web: Option<WebHost>,
    bounds: (f32, f32, f32, f32),
    visible: bool,
    unseen: u32,
}

#[derive(Default)]
pub(super) struct WebFrames {
    /// Boxes drawn by the last render: anchor key to address.
    drawn: RefCell<HashMap<String, String>>,
    frames: HashMap<String, Frame>,
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
    pub(crate) fn script_chat_viewport(&self) -> gpui::Bounds<gpui::Pixels> {
        self.list.viewport_bounds()
    }

    /// The box a frame is drawn over: `maxWidth 400, minWidth 300, width 100%` and the frame's height.
    pub(super) fn web_frame_box(&self, key: String, url: String, height: f32) -> AnyElement {
        let anchor = format!("webframe:{key}");
        self.web_frames
            .drawn
            .borrow_mut()
            .insert(anchor.clone(), url);
        let width = self.content_width.clamp(300., 400.);
        div()
            .relative()
            .w(px(width))
            .h(px(height))
            .child(self.anchor_probe(anchor))
            .into_any_element()
    }

    /// Something of the app's own is over the chat: the views, which paint above the window, step aside.
    fn web_frames_covered(&self) -> bool {
        self.context.open.is_some()
            || self.menu.is_some()
            || self.quick_switcher.is_some()
            || self.lightbox.is_some()
            || self.game_profile.is_some()
            || self.settings.is_some()
            || self.profile.is_some()
            || self.full_profile.is_some()
    }

    /// Called every frame: opens, places, hides and closes the views from where their boxes were drawn last.
    pub(super) fn web_frames_frame(&mut self, window: &Window, _cx: &mut Context<Self>) {
        let drawn = std::mem::take(&mut *self.web_frames.drawn.borrow_mut());
        let covered = self.web_frames_covered();
        for (anchor, url) in &drawn {
            let open = self.web_frames.frames.len();
            if let Some(frame) = self.web_frames.frames.get_mut(anchor) {
                if &frame.url != url {
                    frame.url = url.clone();
                    frame.web = None;
                }
            } else if open < MAX_FRAMES {
                self.web_frames.frames.insert(
                    anchor.clone(),
                    Frame {
                        url: url.clone(),
                        web: None,
                        bounds: (0., 0., 0., 0.),
                        visible: false,
                        unseen: 0,
                    },
                );
            }
        }
        let mut closed = Vec::new();
        for (anchor, frame) in &mut self.web_frames.frames {
            let seen = self
                .tip_anchors
                .borrow_mut()
                .remove(anchor.as_str())
                .map(|bounds| {
                    (
                        f32::from(bounds.origin.x),
                        f32::from(bounds.origin.y),
                        f32::from(bounds.size.width),
                        f32::from(bounds.size.height),
                    )
                });
            let Some(bounds) = seen.filter(|_| drawn.contains_key(anchor)) else {
                frame.unseen += 1;
                if frame.unseen > GRACE_FRAMES {
                    closed.push(anchor.clone());
                } else if let Some(web) = frame.web.as_ref().filter(|_| frame.visible) {
                    web.set_visible(false);
                    frame.visible = false;
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
                    match WebHost::player(window, dir, bounds, &frame.url, Some(REFERER)) {
                        Ok(web) => {
                            web.set_visible(visible);
                            frame.web = Some(web);
                            frame.bounds = bounds;
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
                    if bounds != frame.bounds {
                        web.set_bounds(bounds);
                        frame.bounds = bounds;
                    }
                    if visible != frame.visible {
                        web.set_visible(visible);
                        frame.visible = visible;
                    }
                }
            }
        }
        for anchor in closed {
            self.web_frames.frames.remove(&anchor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spotify(kind: &str, provider: &str, url: &str) -> dc_state::Embed {
        dc_state::Embed {
            kind: kind.to_owned(),
            provider: Some(provider.to_owned()),
            url: Some(url.to_owned()),
            ..Default::default()
        }
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
