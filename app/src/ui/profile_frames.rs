//! Profile frames (collectible type 3): the frame a profile carries comes from its `collectibles` (module 851767,
//! the server profile's own first when that server is in view: `t?.profileFrame ?? e.profileFrame` of module 527212),
//! its layers from the frame item of `GET /collectibles-products/<sku>` (module 892118 reads `inner_width` and the
//! three overflows). Module 744808 draws them in `profileFrameContainer_v8co33`, an absolute box the size of the
//! profile card it is the last child of (`outer_P1YEjQ` of the popout and the DM sidebar, `main.profile_s3DB6Y` of the
//! modal, never the whole modal):
//!
//! * the frame box (`profileFrame_v8co33`) is the card's height and its width plus the scaled horizontal overflow on
//!   each side, the scale being the card's width over `inner_width` (`--custom-profile-frame-layer-scale`);
//! * a `staple` is one picture across the box, hung above the top (`overflow_top` scaled) or below the bottom;
//! * a `rail` is a picture the box's height with the picture's width the box's (`background-size: 100% auto`) placed
//!   at the top, centre or bottom, and left out when `responsive` and the card is wider than 300:480;
//! * a `border` is the picture repeated down the box (`ceil(height / picture height) + 1` copies);
//! * `front` layers sit over the card's content (`z-index: 4`), `back` ones under the card itself (`z-index: -1`
//!   in a card that makes no stacking context), so the card's background hides them;
//! * the card makes room for what hangs out of it (`.custom-profile-frame` margins of the scaled overflows): all
//!   around in the DM sidebar (`themeContainer_S_xUx6`, scale 304 over `inner_width`, none in a window 510px tall or
//!   shorter), below only in the popout (`themeContainer_NOlkhE`, scale 300, 304 when themed), none in the modal,
//!   whose card also drops its bottom staples (`u5` of module 927071);
//! * a framed popout or sidebar card loses its `--shadow-border` ring (`box-shadow: unset`).
//!
//! The modal hides frames below a 785px tall window, the popout below 480px and the DM sidebar below 510px
//! (`@media (max-height)` rules of the same stylesheet).

use super::Root;
use dc_state::Id;
use gpui::{AnyElement, IntoElement, ObjectFit, div, img, prelude::*, px};
use serde_json::Value;

/// The fetch key of a frame's product (`/collectibles-products/<sku>`).
pub(super) const PRODUCT_PREFIX: &str = "frame-product:";
/// `R.PROFILE_FRAME` of module 575593.
const COLLECTIBLE_PROFILE_FRAME: i64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LayerKind {
    Staple,
    Rail,
    Border,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Anchor {
    Top,
    Bottom,
    Center,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Layer {
    pub id: String,
    pub kind: LayerKind,
    /// `order: "front"`: over the card's content; `"back"`: under it.
    pub front: bool,
    pub anchor: Anchor,
    pub responsive: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Frame {
    pub sku: String,
    pub layers: Vec<Layer>,
    pub inner_width: f32,
    pub overflow_top: f32,
    pub overflow_bottom: f32,
    pub overflow_horizontal: f32,
}

/// Where a frame is drawn, for its `max-height` rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrameSurface {
    Popout,
    Modal,
    Sidebar,
    /// The Change Profile Frame dialog's preview card (`container_-JS7Po` of module 641318, `isPreview`).
    Preview,
}

/// The `@media (max-height)` rules of `profileFrameContainer_v8co33`: no frame in a window this short or shorter. A
/// preview (`.preview_v8co33`) is exempt.
pub(super) fn frame_shown(surface: FrameSurface, window_height: f32) -> bool {
    window_height
        > match surface {
            FrameSurface::Popout => 480.,
            FrameSurface::Modal => 785.,
            FrameSurface::Sidebar => 510.,
            FrameSurface::Preview => return true,
        }
}

/// The frame sku of one `collectibles` list: its last type-3 entry, unless it has lapsed.
fn frame_of(list: &Value, now_ms: i64) -> Option<String> {
    let entry = list
        .as_array()?
        .iter()
        .rev()
        .find(|entry| entry["type"].as_i64() == Some(COLLECTIBLE_PROFILE_FRAME))?;
    let lapsed = entry["expires_at"]
        .as_str()
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .is_some_and(|end| end.timestamp_millis() <= now_ms);
    let sku = entry["sku_id"].as_str().map(str::to_owned)?;
    (!lapsed).then_some(sku)
}

/// The frame a profile answer shows: the server profile's when the viewer looks at that server, else the account's.
pub(super) fn equipped_frame(body: &Value, guild: Option<Id>, now_ms: i64) -> Option<String> {
    let member = &body["guild_member_profile"];
    let scoped = guild
        .filter(|guild| member["guild_id"].as_str() == Some(guild.to_string().as_str()))
        .and_then(|_| frame_of(&member["collectibles"], now_ms));
    scoped.or_else(|| frame_of(&body["user_profile"]["collectibles"], now_ms))
}

/// The frame item of a product answer (`fromServer` of module 892118).
pub(super) fn parse_frame(product: &Value) -> Option<Frame> {
    let item = product["items"]
        .as_array()?
        .iter()
        .find(|item| item["type"].as_i64() == Some(COLLECTIBLE_PROFILE_FRAME))?;
    let number = |key: &str| item[key].as_f64().unwrap_or(0.) as f32;
    let layers = item["layers"]
        .as_array()?
        .iter()
        .filter_map(|layer| {
            let kind = match layer["type"].as_str()? {
                "staple" => LayerKind::Staple,
                "rail" => LayerKind::Rail,
                "border" => LayerKind::Border,
                // `default: return null` of module 744808.
                _ => return None,
            };
            Some(Layer {
                id: layer["id"].as_str()?.to_owned(),
                kind,
                front: layer["order"].as_str() == Some("front"),
                anchor: match layer["anchor"].as_str() {
                    Some("top") => Anchor::Top,
                    Some("bottom") => Anchor::Bottom,
                    _ => Anchor::Center,
                },
                responsive: layer["responsive"] == true,
            })
        })
        .collect();
    Some(Frame {
        sku: item["sku_id"].as_str()?.to_owned(),
        layers,
        inner_width: number("inner_width"),
        overflow_top: number("overflow_top"),
        overflow_bottom: number("overflow_bottom"),
        overflow_horizontal: number("overflow_horizontal"),
    })
}

/// `getCollectiblesItemAssetUrl({skuId, assetFormat: "static", assetId})`: a layer's picture.
pub(super) fn layer_url(sku: &str, layer: &str) -> String {
    format!("https://cdn.discordapp.com/media/v1/collectibles-shop/{sku}/{layer}/static")
}

fn layer_key(sku: &str, layer: &str) -> String {
    format!("frame-layer:{sku}:{layer}")
}

impl Frame {
    /// `--custom-profile-frame-layer-scale`: the card's width over the frame's own.
    pub(super) fn scale(&self, width: f32) -> f32 {
        if self.inner_width > 0. {
            width / self.inner_width
        } else {
            1.
        }
    }

    /// `profileFrame_v8co33`: the box's left edge and width over a card `width` wide.
    pub(super) fn frame_box(&self, width: f32) -> (f32, f32) {
        let side = self.overflow_horizontal * self.scale(width);
        (-side, width + 2. * side)
    }

    /// The room a framed card keeps around itself on `surface` (top, each side, bottom): the `.custom-profile-frame`
    /// margins of `themeContainer_S_xUx6` (sidebar) and `themeContainer_NOlkhE` (popout), the overflows scaled by
    /// `--custom-profile-frame-layer-scale`. `themed` is `custom-user-profile-theme` (the popout scales by 304 then).
    /// The dialog's preview card (`:where(.container_-JS7Po).custom-profile-frame`) takes all three, scaled by 300
    /// (304 when themed) like the popout.
    pub(super) fn margins(&self, surface: FrameSurface, themed: bool) -> FrameMargins {
        let scale = |width: f32| self.scale(width);
        let around = |s: f32| FrameMargins {
            top: self.overflow_top * s,
            side: self.overflow_horizontal * s,
            bottom: self.overflow_bottom * s,
        };
        match surface {
            FrameSurface::Sidebar => around(scale(304.)),
            FrameSurface::Preview => around(scale(popout_scale_width(themed))),
            FrameSurface::Popout => FrameMargins {
                bottom: self.overflow_bottom * scale(popout_scale_width(themed)),
                ..FrameMargins::default()
            },
            FrameSurface::Modal => FrameMargins::default(),
        }
    }
}

/// The width `themeContainer_NOlkhE` scales a popout frame's overflows by: 300, 304 on a themed profile.
fn popout_scale_width(themed: bool) -> f32 {
    if themed { 304. } else { 300. }
}

impl Frame {
    /// What a framed popout gives up of its `max-height: calc(100vh - 20px)`: its scaled top and bottom overflows.
    pub(super) fn popout_height_cut(&self, themed: bool) -> f32 {
        (self.overflow_top + self.overflow_bottom) * self.scale(popout_scale_width(themed))
    }
}

/// `--custom-profile-frame-drop-shadow-opacity` of the client theme (`629972.css`): the black of a framed popout's
/// `20px 20px 30px` shadow.
pub(super) fn drop_shadow_opacity(theme: dc_tokens::Theme) -> f32 {
    match theme {
        dc_tokens::Theme::Light => 0.13,
        dc_tokens::Theme::Dark => 0.2,
        dc_tokens::Theme::Darker => 0.4,
        dc_tokens::Theme::Midnight => 0.6,
    }
}

/// The margins of a framed card ([`Frame::margins`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct FrameMargins {
    pub top: f32,
    pub side: f32,
    pub bottom: f32,
}

/// The layers `surface` draws: the modal's card leaves out bottom staples (`filterLayer: u5` of module 927071).
pub(super) fn layer_drawn(surface: FrameSurface, layer: &Layer) -> bool {
    surface != FrameSurface::Modal
        || !(layer.kind == LayerKind::Staple && layer.anchor == Anchor::Bottom)
}

/// The copies of a border picture (`natural` width x height) down a `width` x `height` box (module 744808).
pub(super) fn border_copies(width: f32, height: f32, natural: (f32, f32)) -> usize {
    if natural.0 <= 0. || natural.1 <= 0. {
        return 5;
    }
    let tile = width * (natural.1 / natural.0);
    if tile <= 0. {
        5
    } else {
        (height / tile).ceil() as usize + 1
    }
}

/// `@container (min-aspect-ratio: 300 / 480) { .responsive { display: none } }` of a rail.
pub(super) fn rail_hidden(layer: &Layer, width: f32, height: f32) -> bool {
    layer.kind == LayerKind::Rail && layer.responsive && width * 480. >= height * 300.
}

/// A picture's own size, read from its header.
fn natural_size(image: &gpui::Image) -> Option<(f32, f32)> {
    let reader = image::ImageReader::new(std::io::Cursor::new(image.bytes()))
        .with_guessed_format()
        .ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    Some((width as f32, height as f32))
}

impl Root {
    /// The frame `user`'s fetched profile carries, once its product was read.
    pub(super) fn profile_frame(&self, user: Id) -> Option<Frame> {
        let body = self.fetched.get(&format!("profile:{user}"))?;
        let sku = equipped_frame(body, self.guild, chrono::Utc::now().timestamp_millis())?;
        parse_frame(self.fetched.get(&format!("{PRODUCT_PREFIX}{sku}"))?)
    }

    /// Reads the product of the frame a fetched profile carries.
    pub(super) fn request_profile_frame(&mut self, user: Id) {
        let Some(body) = self.fetched.get(&format!("profile:{user}")) else {
            return;
        };
        let Some(sku) = equipped_frame(body, self.guild, chrono::Utc::now().timestamp_millis())
        else {
            return;
        };
        self.request_frame_product(&sku);
    }

    /// The frame of product `sku` once it was read (the editor's pending or saved choice).
    pub(super) fn frame_of_sku(&self, sku: &str) -> Option<Frame> {
        parse_frame(self.fetched.get(&format!("{PRODUCT_PREFIX}{sku}"))?)
    }

    /// Reads the product of frame `sku`, and its layers once it is in.
    pub(super) fn request_frame_product(&mut self, sku: &str) {
        let key = format!("{PRODUCT_PREFIX}{sku}");
        if let Some(product) = self.fetched.get(&key).cloned() {
            self.request_frame_layers(&product);
        } else if let Ok(sku_id) = sku.parse::<Id>()
            && self.requested.insert(key.clone())
        {
            let _ = self.requests.send(crate::net::Request::Fetch {
                key,
                route: dc_state::profile_effects::product_route(sku_id),
            });
        }
    }

    fn request_frame_layers(&mut self, product: &Value) {
        if let Some(frame) = parse_frame(product) {
            for layer in &frame.layers {
                self.request_image(
                    layer_key(&frame.sku, &layer.id),
                    layer_url(&frame.sku, &layer.id),
                );
            }
        }
    }

    /// The margins `user`'s card keeps on `surface` while its frame is drawn ([`Frame::margins`]); none without one,
    /// while it loads, or in a window too short for `surface` (the sidebar's `@media (max-height: 510px)` sets them
    /// back to 0 with the frame).
    pub(super) fn profile_frame_margins(
        &self,
        user: Id,
        surface: FrameSurface,
        themed: bool,
        window_height: f32,
    ) -> Option<FrameMargins> {
        if !frame_shown(surface, window_height) {
            return None;
        }
        let frame = self.profile_frame(user)?;
        Some(frame.margins(surface, themed))
    }

    /// A frame product arrived: keep it and ask for its layers.
    pub(super) fn profile_frame_fetched(
        &mut self,
        key: String,
        body: Value,
        cx: &mut gpui::Context<Self>,
    ) {
        if body["fetch_failed"] != true {
            self.request_frame_layers(&body);
        }
        self.fetched.insert(key, body);
        cx.notify();
    }

    /// The layers of `user`'s frame over a card `width` wide (and `height` tall when known): the ones under the card
    /// and the ones over it, each an absolute box for a `relative` box exactly the card's (the back ones before the
    /// card, so its background covers them; the front ones after it), outside any clipping. Nothing while the
    /// product or a picture is still loading, and nothing in a window too short for `surface`.
    pub(super) fn profile_frame_layers(
        &self,
        user: Id,
        width: f32,
        height: Option<f32>,
        surface: FrameSurface,
        window_height: f32,
    ) -> (Vec<AnyElement>, Vec<AnyElement>) {
        match self.profile_frame(user) {
            Some(frame) => self.frame_layers(&frame, width, height, surface, window_height),
            None => (Vec::new(), Vec::new()),
        }
    }

    /// [`Root::profile_frame_layers`] for a frame already in hand (the editor's choice).
    pub(super) fn frame_layers(
        &self,
        frame: &Frame,
        width: f32,
        height: Option<f32>,
        surface: FrameSurface,
        window_height: f32,
    ) -> (Vec<AnyElement>, Vec<AnyElement>) {
        let (mut back, mut front) = (Vec::new(), Vec::new());
        if !frame_shown(surface, window_height) {
            return (back, front);
        }
        let scale = frame.scale(width);
        let (left, box_width) = frame.frame_box(width);
        for layer in &frame.layers {
            if !layer_drawn(surface, layer)
                || height.is_some_and(|height| rail_hidden(layer, width, height))
            {
                continue;
            }
            let Some(picture) = self.images.get(&layer_key(&frame.sku, &layer.id)).cloned() else {
                continue;
            };
            let natural = natural_size(&picture).unwrap_or((box_width, box_width));
            let picture_height = box_width * natural.1 / natural.0.max(1.);
            let base = div().absolute().left(px(left)).w(px(box_width));
            let element = match layer.kind {
                LayerKind::Staple => {
                    let placed = match layer.anchor {
                        Anchor::Bottom => base.bottom(px(-frame.overflow_bottom * scale)),
                        _ => base.top(px(-frame.overflow_top * scale)),
                    };
                    placed
                        .h(px(picture_height))
                        .child(img(picture).w(px(box_width)).h(px(picture_height)))
                }
                // `background-size: 100% auto`, no repeat, at the anchor; the rest of the box stays clear.
                LayerKind::Rail => base
                    .top_0()
                    .bottom_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .map(|rail| match layer.anchor {
                        Anchor::Top => rail.justify_start(),
                        Anchor::Bottom => rail.justify_end(),
                        Anchor::Center => rail.justify_center(),
                    })
                    .child(
                        img(picture)
                            .flex_none()
                            .w(px(box_width))
                            .h(px(picture_height))
                            .object_fit(ObjectFit::Fill),
                    ),
                LayerKind::Border => {
                    let copies = border_copies(box_width, height.unwrap_or(window_height), natural);
                    base.top_0()
                        .bottom_0()
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .children((0..copies).map(|_| {
                            img(picture.clone())
                                .flex_none()
                                .w(px(box_width))
                                .h(px(picture_height))
                        }))
                }
            };
            if layer.front {
                front.push(element.into_any_element());
            } else {
                back.push(element.into_any_element());
            }
        }
        (back, front)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn product() -> Value {
        json!({"sku_id": "10", "items": [{
            "type": 3, "sku_id": "10",
            "layers": [
                {"id": "a", "type": "staple", "order": "front", "anchor": "top", "responsive": false},
                {"id": "b", "type": "rail", "order": "back", "anchor": "center", "responsive": true},
                {"id": "c", "type": "border", "order": "front", "anchor": "center", "responsive": false},
                {"id": "d", "type": "sparkle", "order": "front", "anchor": "top"}
            ],
            "inner_width": 1200, "overflow_top": 304, "overflow_bottom": 148, "overflow_horizontal": 56
        }]})
    }

    #[test]
    fn a_frame_item_reads_its_layers_and_overflows() {
        let frame = parse_frame(&product()).unwrap();
        assert_eq!(frame.sku, "10");
        let kinds: Vec<LayerKind> = frame.layers.iter().map(|layer| layer.kind).collect();
        assert_eq!(
            kinds,
            [LayerKind::Staple, LayerKind::Rail, LayerKind::Border]
        );
        assert!(frame.layers[0].front && !frame.layers[1].front);
        assert_eq!(frame.layers[0].anchor, Anchor::Top);
        // A 300px card: a quarter of the 1200px frame, 14px of overflow on each side.
        assert_eq!(frame.scale(300.), 0.25);
        assert_eq!(frame.frame_box(300.), (-14., 328.));
        assert!(parse_frame(&json!({"items": [{"type": 1}]})).is_none());
    }

    #[test]
    fn the_shown_frame_is_the_servers_then_the_accounts_unless_lapsed() {
        let body = json!({
            "user_profile": {"collectibles": [
                {"type": 1, "sku_id": "1"},
                {"type": 3, "sku_id": "2", "expires_at": null}
            ]},
            "guild_member_profile": {"guild_id": "9", "collectibles": [{"type": 3, "sku_id": "3"}]}
        });
        assert_eq!(equipped_frame(&body, Some(9), 0).as_deref(), Some("3"));
        assert_eq!(equipped_frame(&body, Some(8), 0).as_deref(), Some("2"));
        assert_eq!(equipped_frame(&body, None, 0).as_deref(), Some("2"));
        let lapsed = json!({"user_profile": {"collectibles": [
            {"type": 3, "sku_id": "2", "expires_at": "2020-01-01T00:00:00+00:00"}
        ]}});
        assert_eq!(equipped_frame(&lapsed, None, 1_700_000_000_000), None);
    }

    #[test]
    fn borders_repeat_down_the_box_and_rails_give_way_on_wide_cards() {
        // A 300x100 picture across 300px is 100px tall: a 450px card takes ceil(4.5) + 1 copies.
        assert_eq!(border_copies(300., 450., (300., 100.)), 6);
        assert_eq!(border_copies(300., 450., (0., 0.)), 5);
        let frame = parse_frame(&product()).unwrap();
        let rail = &frame.layers[1];
        assert!(rail_hidden(rail, 600., 700.));
        assert!(!rail_hidden(rail, 300., 600.));
        assert!(!rail_hidden(&frame.layers[0], 600., 700.));
    }

    #[test]
    fn the_card_keeps_room_for_its_frame_per_surface() {
        let frame = parse_frame(&product()).unwrap();
        // Sidebar: 304 over 1200 on every side (304, 56 and 148 of the fixture).
        let sidebar = frame.margins(FrameSurface::Sidebar, false);
        let s = 304. / 1200.;
        assert_eq!(
            sidebar,
            FrameMargins {
                top: 304. * s,
                side: 56. * s,
                bottom: 148. * s
            }
        );
        // Popout: below only, 300 unthemed and 304 themed.
        let popout = frame.margins(FrameSurface::Popout, false);
        assert_eq!((popout.top, popout.side), (0., 0.));
        assert_eq!(popout.bottom, 148. * 300. / 1200.);
        assert_eq!(frame.margins(FrameSurface::Popout, true).bottom, 148. * s);
        assert_eq!(
            frame.margins(FrameSurface::Modal, true),
            FrameMargins::default()
        );
        // `max-height: calc(100vh - 20px - top - bottom)` of a framed popout.
        assert_eq!(frame.popout_height_cut(false), (304. + 148.) * 0.25);
        // The Change Profile Frame preview: all around at 300 over 1200, and never hidden by the window's height.
        assert_eq!(
            frame.margins(FrameSurface::Preview, false),
            FrameMargins {
                top: 76.,
                side: 14.,
                bottom: 37.
            }
        );
        assert!(frame_shown(FrameSurface::Preview, 300.));
        assert!(layer_drawn(FrameSurface::Preview, &frame.layers[0]));
        assert_eq!(drop_shadow_opacity(dc_tokens::Theme::Darker), 0.4);
    }

    #[test]
    fn the_modal_card_drops_bottom_staples_only() {
        let staple = |anchor| Layer {
            id: "s".into(),
            kind: LayerKind::Staple,
            front: true,
            anchor,
            responsive: false,
        };
        assert!(!layer_drawn(FrameSurface::Modal, &staple(Anchor::Bottom)));
        assert!(layer_drawn(FrameSurface::Modal, &staple(Anchor::Top)));
        assert!(layer_drawn(FrameSurface::Sidebar, &staple(Anchor::Bottom)));
        assert!(layer_drawn(FrameSurface::Popout, &staple(Anchor::Bottom)));
        let border = Layer {
            kind: LayerKind::Border,
            anchor: Anchor::Bottom,
            ..staple(Anchor::Bottom)
        };
        assert!(layer_drawn(FrameSurface::Modal, &border));
    }

    #[test]
    fn frames_leave_short_windows() {
        assert!(!frame_shown(FrameSurface::Modal, 720.));
        assert!(frame_shown(FrameSurface::Modal, 900.));
        assert!(frame_shown(FrameSurface::Popout, 720.));
        assert!(!frame_shown(FrameSurface::Sidebar, 500.));
    }
}
