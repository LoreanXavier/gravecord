//! ImageZoom (Vencord `src/plugins/imageZoom`, GPL-3.0-or-later): in the picture viewer, holding the left button on a
//! picture shows a lens (`vc-imgzoom-lens`: `size` px, round or `square`, a 2px grey border with a grey inner glow)
//! that magnifies the picture `zoom` times under the pointer; the press no longer zooms the viewer. While held, the
//! wheel changes the zoom (`zoomSpeed` per notch, 1 at least) and Shift with the wheel the lens size (50 px at least),
//! `invertScroll` turning the direction; with `saveZoomValues` both are kept.
//!
//! GAP: `nearestNeighbour` (the renderer samples pictures smoothly, with no pixelated mode), an animated picture's lens
//! (it shows the still frame), and the image menu's rows (Square Lens, Nearest Neighbour and the three sliders), which
//! the plugin's settings hold. Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Bounds, Context, IntoElement, ParentElement, Pixels, Point, Styled, div, img, px,
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::sync::Arc;

const ID: &str = "ImageZoom";

/// The settings the lens reads.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Lens {
    zoom: f32,
    size: f32,
    speed: f32,
    invert: bool,
    save: bool,
    square: bool,
}

fn lens_settings(state: &State) -> Option<Lens> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    let number = |value: Value, fallback: f64| value.as_f64().unwrap_or(fallback) as f32;
    let flag = |key: &str| state.value(ID, key) == Value::Bool(true);
    Some(Lens {
        zoom: number(key("zoom"), 2.),
        size: number(key("size"), 100.),
        speed: number(key("zoomSpeed"), 0.5),
        invert: flag("invertScroll"),
        save: flag("saveZoomValues"),
        square: flag("square"),
    })
}

fn current() -> Option<Lens> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(lens_settings)
}

/// The held lens: where the pointer is, and the zoom and size it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    at: Point<Pixels>,
    zoom: f32,
    size: f32,
}

thread_local! {
    static HELD: Cell<Option<Held>> = const { Cell::new(None) };
    /// The picture's place on screen, drawn last frame.
    static PICTURE: RefCell<Option<Bounds<Pixels>>> = const { RefCell::new(None) };
}

/// `onWheel`: the zoom after a wheel step (`deltaY / 100 * speed`, never under 1).
fn zoomed(zoom: f32, delta_y: f32, lens: Lens) -> f32 {
    let sign = if lens.invert { -1. } else { 1. };
    (zoom + delta_y / 100. * sign * lens.speed).max(1.)
}

/// The size after a Shift wheel step (`deltaY * speed`, never under 50).
fn sized(size: f32, delta_y: f32, lens: Lens) -> f32 {
    let sign = if lens.invert { -1. } else { 1. };
    (size + delta_y * sign * lens.speed).max(50.)
}

impl Root {
    /// The left press on a picture: true when the lens took it (the viewer does not drag or zoom).
    pub(in crate::ui) fn image_zoom_press(
        &mut self,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(lens) = current() else {
            return false;
        };
        HELD.with(|held| {
            held.set(Some(Held {
                at,
                zoom: lens.zoom,
                size: lens.size,
            }))
        });
        cx.notify();
        true
    }

    /// The pointer moved: true while the lens is held.
    pub(in crate::ui) fn image_zoom_move(
        &mut self,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(mut held) = HELD.with(Cell::get) else {
            return false;
        };
        held.at = at;
        HELD.with(|cell| cell.set(Some(held)));
        cx.notify();
        true
    }

    /// The left button rose: true when it ends the lens (the viewer does not toggle its zoom).
    pub(in crate::ui) fn image_zoom_release(&mut self, cx: &mut Context<Self>) -> bool {
        if HELD.with(|held| held.take()).is_none() {
            return false;
        }
        cx.notify();
        true
    }

    /// A wheel step while the lens is held: zoom, or with Shift the lens size.
    pub(in crate::ui) fn image_zoom_scroll(
        &mut self,
        delta_y: f32,
        shift: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let (Some(lens), Some(mut held)) = (current(), HELD.with(Cell::get)) else {
            return false;
        };
        if shift {
            held.size = sized(held.size, delta_y, lens);
        } else {
            held.zoom = zoomed(held.zoom, delta_y, lens);
        }
        HELD.with(|cell| cell.set(Some(held)));
        if lens.save {
            crate::plugins::update(|state| {
                state.set_value(
                    ID,
                    if shift { "size" } else { "zoom" },
                    &json!(if shift { held.size } else { held.zoom }),
                );
            });
        }
        cx.notify();
        true
    }

    /// The lens over a picture drawn `width` by `height`, while it is held; and the probe that keeps the picture's
    /// place. `None` with the plugin off.
    pub(in crate::ui) fn image_zoom_lens(
        &self,
        picture: Arc<gpui::Image>,
        width: f32,
        height: f32,
    ) -> Option<Vec<AnyElement>> {
        let lens = current()?;
        let probe = gpui::canvas(
            |bounds, _, _| PICTURE.with(|cell| *cell.borrow_mut() = Some(bounds)),
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .into_any_element();
        let (Some(held), Some(bounds)) =
            (HELD.with(Cell::get), PICTURE.with(|cell| *cell.borrow()))
        else {
            return Some(vec![probe]);
        };
        let local_x = f32::from(held.at.x - bounds.origin.x);
        let local_y = f32::from(held.at.y - bounds.origin.y);
        let half = held.size / 2.;
        let grey = gpui::rgb(0x808080);
        let glass = div()
            .absolute()
            .left(px(local_x - half - 2.))
            .top(px(local_y - half - 2.))
            .size(px(held.size))
            .border_2()
            .border_color(grey)
            .when(!lens.square, |this| this.rounded_full())
            .overflow_hidden()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0.5, 1.),
                offset: gpui::point(px(0.), px(0.)),
                blur_radius: px(2.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                img(picture)
                    .absolute()
                    .left(px(-(local_x * held.zoom - half)))
                    .top(px(-(local_y * held.zoom - half)))
                    .w(px(width * held.zoom))
                    .h(px(height * held.zoom)),
            )
            .into_any_element();
        Some(vec![probe, glass])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_follows_the_original() {
        let mut state = State::default();
        assert_eq!(lens_settings(&state), None);
        state.set_enabled(ID, true);
        let lens = lens_settings(&state).unwrap();
        assert_eq!((lens.zoom, lens.size, lens.speed), (2., 100., 0.5));
        assert!(lens.invert && lens.save && !lens.square);
        // Inverted: scrolling down (positive delta) zooms out, never below 1.
        assert!((zoomed(2., 100., lens) - 1.5).abs() < 1e-6);
        assert!((zoomed(1.2, 100., lens) - 1.).abs() < 1e-6);
        assert!((sized(100., -100., lens) - 150.).abs() < 1e-6);
        assert!((sized(60., 100., lens) - 50.).abs() < 1e-6);
    }
}
