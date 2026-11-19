//! The overlay's Video and Go Live widgets (module 415173 `iS` and `e2`; the rules are `dc_overlay::video`).
//!
//! - Video (`videoList_BKJg6Y`: a centred row, or a column when vertical, 8 px apart, at the widget's `opacity`): each
//!   camera of the voice channel in a tile with the base tile radius (8 px, `tile_BKJg6Y`), the picture covering it
//!   (`fit: COVER`), else the person's avatar on the avatar's colour. Unlocked with no camera, the empty state (the
//!   video icon at 24%, "Cameras in this voice channel are shown here." as its label).
//! - Go Live (`goLiveGridContainer_meNfas` at the widget's `opacity`, `gridContainer_3BuwBT`): the streams of the
//!   channel, each at 16:9 across the widget, `padding` (8 px) apart. A stream we watch shows its picture
//!   (`fit: CONTAIN`); unlocked, one we do not watch shows its preview (`em.A` with `noText`). Unlocked with no
//!   stream, the empty state (the screen-arrow icon, "Go Live streams are shown in a list here.").
//!
//! GAP: the tiles' own chrome of module 175203 in overlay mode (name tag, hover actions, the per-tile context menu
//! 588860) and the Go Live tile's hover row (`e_`: Watch and pin) are not drawn; the Go Live widget does not resize
//! itself to its tile count (`eK`), it keeps the size the layout gives it.

use super::Root;
use super::overlay::onyx;
use dc_overlay::layout::Widget;
use dc_overlay::video;
use dc_state::Id;
use dc_voice::VideoSource;
use gpui::{AnyElement, Context, IntoElement, div, img, prelude::*, px, svg};

/// `--custom-base-tile-border-radius`.
const TILE_RADIUS: f32 = 8.;

impl Root {
    /// The content box of a widget (its size less padding and border), as the layout gives it.
    fn overlay_content_size(&self, widget: &Widget, window: (f32, f32)) -> (f32, f32) {
        let (width, height) = dc_overlay::drag::requested_size(
            widget.size.pixels(window.0, window.1),
            widget.min_size,
        );
        let inset = 2. * (dc_overlay::drag::PADDING + dc_overlay::drag::BORDER);
        (
            (width.unwrap_or(widget.min_size.0) - inset).max(0.),
            (height.unwrap_or(widget.min_size.1) - inset).max(0.),
        )
    }

    /// The empty state of a widget: its icon at 24% in the middle (`q.g` with `absolute`).
    fn overlay_widget_empty(icon: &'static str) -> AnyElement {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                svg()
                    .path(format!("icons/{icon}.svg"))
                    .size(px(24.))
                    .text_color(onyx("--white"))
                    .opacity(0.24),
            )
            .into_any_element()
    }

    /// One camera or stream picture, else the person's avatar on their colour.
    fn overlay_video_tile(
        &self,
        user: Id,
        source: VideoSource,
        size: (f32, f32),
        cover: bool,
        preview: Option<std::sync::Arc<gpui::Image>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let person = self.user_of(user);
        let picture = self.video.images.image(&(user, source)).cloned();
        let fit = if cover {
            gpui::ObjectFit::Cover
        } else {
            gpui::ObjectFit::Contain
        };
        let content = match (picture, preview) {
            // gpui does not clip a picture to its parent's corners: the picture is rounded itself.
            (Some(picture), _) => img(picture)
                .size_full()
                .rounded(px(TILE_RADIUS))
                .object_fit(fit)
                .into_any_element(),
            (None, Some(preview)) => img(preview)
                .size_full()
                .rounded(px(TILE_RADIUS))
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element(),
            (None, None) => {
                let (key, url) = super::avatar_source(&person);
                let avatar = self.images.get(&key).cloned();
                if avatar.is_none() {
                    self.want_image(key, url, cx);
                }
                let side = (size.0.min(size.1) * 0.5).clamp(24., 80.);
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(TILE_RADIUS))
                    .bg(self.tile_color(&person, cx))
                    .children(avatar.map(|avatar| img(avatar).size(px(side)).rounded_full()))
                    .into_any_element()
            }
        };
        div()
            .flex_none()
            .w(px(size.0))
            .h(px(size.1))
            .rounded(px(TILE_RADIUS))
            .overflow_hidden()
            .child(content)
            .into_any_element()
    }

    /// The Video widget (`iS`): `None` draws nothing.
    pub(super) fn overlay_video_content(
        &mut self,
        widget: &Widget,
        locked: bool,
        show_empty: bool,
        window: (f32, f32),
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let call = self.call.as_ref().map(|call| (call.guild, call.channel));
        // `getVideoParticipants`, less the ones turned off locally (`ip`).
        let cameras: Vec<Id> = call
            .map(|(guild, channel)| {
                self.stores
                    .voice_members(guild, channel)
                    .into_iter()
                    .filter(|state| state.self_video)
                    .map(|state| state.user_id)
                    .collect()
            })
            .unwrap_or_default();
        let own_on = super::voice_view::call_setting(super::voice_view::SHOW_OWN_CAMERA);
        let mine = me.filter(|me| cameras.contains(me) && own_on);
        let mut others: Vec<Id> = cameras
            .iter()
            .copied()
            .filter(|user| Some(*user) != me && !super::call_menus::video_disabled(*user))
            .collect();
        // The store's order: whoever speaks first, then by name.
        others.sort_by_key(|user| {
            (
                !self.speaking.contains(user),
                self.user_of(*user).display_name().to_lowercase(),
            )
        });
        let horizontal = widget.horizontal();
        let (content_w, content_h) = self.overlay_content_size(widget, window);
        let along = if horizontal { content_w } else { content_h };
        let (shown, length) = video::video_tiles(along, &others, mine);
        if shown.is_empty() {
            // No voice channel: nothing at all (`null == o`).
            call?;
            return (!locked && show_empty).then(|| {
                div()
                    .relative()
                    .size_full()
                    .child(Self::overlay_widget_empty("overlay_video"))
                    .into_any_element()
            });
        }
        // `g`: shown while unlocked, or pinned while locked; otherwise `hidden_BKJg6Y`.
        let display = !locked || widget.pinned;
        if !display {
            return None;
        }
        let size = if horizontal {
            (length, content_h)
        } else {
            (content_w, length)
        };
        let tiles: Vec<AnyElement> = shown
            .into_iter()
            .map(|user| self.overlay_video_tile(user, VideoSource::Camera, size, true, None, cx))
            .collect();
        Some(
            div()
                .flex()
                .flex_1()
                .when(!horizontal, |this| this.flex_col())
                .gap(px(8.))
                .size_full()
                .justify_center()
                .opacity(widget.opacity)
                .children(tiles)
                .into_any_element(),
        )
    }

    /// The Go Live widget (`e2`): `None` draws nothing.
    pub(super) fn overlay_go_live_content(
        &mut self,
        widget: &Widget,
        locked: bool,
        show_empty: bool,
        window: (f32, f32),
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let me = self.stores.current_user.as_ref().map_or(0, |me| me.id);
        let call = self.call.as_ref().map(|call| (call.guild, call.channel));
        let watched: Vec<Id> = self
            .watched_streams()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let streams: Vec<video::Stream> = call
            .map(|(guild, channel)| {
                self.stores
                    .voice_members(guild, channel)
                    .into_iter()
                    .filter(|state| state.self_stream)
                    .map(|state| video::Stream {
                        owner: state.user_id,
                        username: self.user_of(state.user_id).username.clone(),
                        watched: watched.contains(&state.user_id),
                        gone: false,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let horizontal = widget.horizontal();
        let top = widget.anchor.top.is_some();
        let left = widget.anchor.left.is_some();
        // `t` and `n` of `e2`: watched first towards the anchored edge.
        let first = (top && !horizontal) || (left && horizontal);
        let last = (!top && !horizontal) || (!left && horizontal);
        let listed =
            video::go_live_streams(&streams, me, locked, widget.show_all_streams(), first, last);
        let empty = video::go_live_empty(call.is_some(), watched.len(), listed.len(), locked);
        if (empty || !widget.pinned) && locked {
            return None;
        }
        if empty {
            return (show_empty && !locked).then(|| {
                div()
                    .relative()
                    .size_full()
                    .child(Self::overlay_widget_empty("overlay_screen_arrow"))
                    .into_any_element()
            });
        }
        let content = self.overlay_content_size(widget, window);
        let size = video::go_live_tile(content, horizontal);
        let (guild, channel) = call.unwrap_or_default();
        let mut tiles = Vec::new();
        for stream in listed {
            // `eN`: locked, an unwatched stream is not drawn.
            if !stream.watched && locked {
                continue;
            }
            let preview = if stream.watched {
                None
            } else {
                self.stream_preview_want(super::stream_previews::key_of(
                    guild,
                    channel,
                    stream.owner,
                ));
                self.stream_preview_image(guild, channel, stream.owner)
            };
            tiles.push(self.overlay_video_tile(
                stream.owner,
                VideoSource::Stream(stream.owner),
                size,
                false,
                preview,
                cx,
            ));
        }
        Some(
            div()
                .relative()
                .size_full()
                .min_w(px(128.))
                .opacity(widget.opacity)
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .size_full()
                        .when(horizontal, |this| this.flex_row())
                        .when(!horizontal, |this| this.flex_col())
                        .items_start()
                        .gap(px(dc_overlay::drag::PADDING))
                        .children(tiles),
                )
                .into_any_element(),
        )
    }
}
