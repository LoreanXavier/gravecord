//! Zoom of a focused stream (modules 40056 and 555148 `M`): the wheel zooms between 1 and 5 about the pointer, by
//! 1.1 to the power of the wheel's hundredths held to one step; while zoomed, a drag pans, held so the picture
//! never leaves the tile, and a press that moved does not count as a click on the tile. The zoom belongs to the
//! focused tile and starts over when another one is focused.
//!
//! The tile's zoom controls (module 117029, `*_PkSv2u` in `688508.*.css`): a Zoom in button; while zoomed, or for
//! two seconds after a zoom (`isZooming`), a minimap of the whole picture with the part on screen outlined, and
//! under it Zoom out, the zoom level slider (module 310592) and Zoom in. A button steps 0.25 about the centre; a
//! press or drag on the minimap moves the outlined part there.
//!
//! A zoom step eases for 0.15s (`--custom-zoom-transition` of 555148 `M`), unless the tile is being dragged, the
//! wheel turned in the last 100ms or the slider moved in the last 100ms; the outline eases for 0.1s
//! (`--custom-zoom-indicator-transition` of 117029), also not while the minimap is dragged. Both are `.full-motion`
//! rules. Not built: the 40ms gathering of wheel events (a wheel event zooms at once) and the freeze of the ease
//! while the tile is resized.

use super::motion::{self, Ease, css};
use super::tooltip::{Active, Body, Side, Tip};
use super::{Root, icon};
use dc_tokens::Theme;
use gpui::{
    AnyElement, Bounds, BoxShadow, Context, MouseButton, Pixels, RenderImage, Rgba, SharedString,
    Window, canvas, div, hsla, img, point, prelude::*, px, rgba, size,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

pub(super) const MIN: f32 = 1.;
pub(super) const MAX: f32 = 5.;
/// `w(y-.25, ...)` and `w(y+.25, ...)` of the Zoom out and Zoom in buttons (117029).
pub(super) const STEP: f32 = 0.25;
/// The slider's `step: .01` (117029).
const SLIDER_STEP: f32 = 0.01;
/// `isZooming` holds this long after a zoom (`y.current.start(2e3, ...)`, 40056).
pub(super) const ZOOMING_MS: f64 = 2000.;
/// `isWheeling` holds this long after a wheel event (`$.current.start(100, ...)`, 555148).
const WHEELING_MS: f64 = 100.;
/// `isSlidering` holds this long after the slider moved (`ei.current.start(100, ...)`, 117029).
const SLIDERING_MS: f64 = 100.;
/// `--custom-zoom-minimap-height` (117029).
pub(super) const MINIMAP_HEIGHT: f32 = 120.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Zoom {
    pub level: f32,
    /// How far the picture's centre is from the tile's, in pixels.
    pub pan: (f32, f32),
}

impl Default for Zoom {
    fn default() -> Self {
        Self {
            level: MIN,
            pan: (0., 0.),
        }
    }
}

/// `clampPanOffset`: half of what the zoom adds on each axis.
pub(super) fn clamp_pan(pan: (f32, f32), level: f32, size: (f32, f32)) -> (f32, f32) {
    let x = size.0 * (level - 1.) / 2.;
    let y = size.1 * (level - 1.) / 2.;
    (pan.0.clamp(-x, x), pan.1.clamp(-y, y))
}

/// `doZoom`: the level held to its range, and the point `at` (from the tile's centre) kept under the pointer.
pub(super) fn zoom_to(zoom: Zoom, wanted: f32, at: (f32, f32), size: (f32, f32)) -> Zoom {
    let level = wanted.clamp(MIN, MAX);
    if level == zoom.level {
        return zoom;
    }
    let by = level / zoom.level;
    let pan = (
        (zoom.pan.0 - at.0) * by + at.0,
        (zoom.pan.1 - at.1) * by + at.1,
    );
    Zoom {
        level,
        pan: clamp_pan(pan, level, size),
    }
}

/// The level a wheel movement asks for; `delta_y` as a browser reports it (down is positive, in pixels).
pub(super) fn wheel_level(level: f32, delta_y: f32) -> f32 {
    level * 1.1f32.powf((-delta_y / 100.).clamp(-1., 1.))
}

/// `--custom-zoom-minimap-width`: 120 times the picture's shape, at most 32:9.
pub(super) fn minimap_width(aspect: f32) -> f32 {
    MINIMAP_HEIGHT * aspect.min(32. / 9.)
}

/// The outlined part of the minimap (`ee` of 117029): left, top, width and height, as fractions of the minimap.
pub(super) fn indicator(zoom: Zoom, size: (f32, f32)) -> [f32; 4] {
    let (width, height) = (size.0.max(1.), size.1.max(1.));
    let span = 1. / zoom.level;
    let left = 0.5 - zoom.pan.0 / (width * zoom.level);
    let top = 0.5 - zoom.pan.1 / (height * zoom.level);
    [
        (left - span / 2.).clamp(0., 1. - span),
        (top - span / 2.).clamp(0., 1. - span),
        span,
        span,
    ]
}

/// `Q` of 117029: the pan that centres the tile on the minimap's point at `at` (fractions of the minimap).
pub(super) fn minimap_pan(at: (f32, f32), level: f32, size: (f32, f32)) -> (f32, f32) {
    clamp_pan(
        ((0.5 - at.0) * size.0 * level, (0.5 - at.1) * size.1 * level),
        level,
        size,
    )
}

/// Where on the slider's track a level sits, from 0 to 1 (`Z` of 310592).
pub(super) fn slider_fraction(level: f32) -> f32 {
    ((level - MIN) / (MAX - MIN)).clamp(0., 1.)
}

/// The level at a fraction of the track, snapped to the slider's step (react-aria's `setThumbPercent`).
pub(super) fn slider_level(fraction: f32) -> f32 {
    let raw = MIN + fraction.clamp(0., 1.) * (MAX - MIN);
    let snapped = MIN + ((raw - MIN) / SLIDER_STEP).round() * SLIDER_STEP;
    snapped.clamp(MIN, MAX)
}

/// The slider's value text (`formatValue: e => `${Math.round(100*e)}%``).
pub(super) fn percent(level: f32) -> String {
    format!("{}%", (level * 100.).round() as i32)
}

#[derive(Default)]
pub(super) struct State {
    /// Whose stream the zoom is of.
    owner: Option<u64>,
    pub zoom: Zoom,
    /// A pan in progress: where the pointer went down and the pan then.
    pub drag: Option<((f32, f32), (f32, f32))>,
    /// The press moved the picture: the click that ends it is not the tile's.
    moved: bool,
    /// Where the zoomed tile was painted.
    pub bounds: Rc<Cell<Bounds<Pixels>>>,
    /// When the level last changed, on the motion clock (`isZooming`).
    zoomed_at: Option<f64>,
    /// When the wheel last turned (`isWheeling`).
    wheeled_at: Option<f64>,
    /// When the slider last moved (`isSlidering`).
    slid_at: Option<f64>,
    /// The minimap is being pressed (`U` of 117029).
    pub minimap_drag: bool,
    /// Where the minimap was painted.
    pub minimap: Rc<Cell<Bounds<Pixels>>>,
    /// A slider drag: where the pointer went down and the track fraction it started from.
    pub slider_drag: Option<(f32, f32)>,
    /// Where the slider's track (`track_OnqacX`, 5px in from each end) was painted.
    pub track: Rc<Cell<Bounds<Pixels>>>,
    /// The pointer is on the slider (its `onMouseEnter`).
    pub slider_hover: bool,
    /// The pointer is on the control box: the call view does not go idle (`onPreventIdle("interact")`).
    pub box_hover: bool,
}

impl State {
    /// The zoom follows the focused stream and starts over with another one.
    pub fn follow(&mut self, owner: Option<u64>) {
        if self.owner != owner {
            self.owner = owner;
            self.zoom = Zoom::default();
            self.drag = None;
            self.moved = false;
            self.zoomed_at = None;
            self.minimap_drag = false;
            self.slider_drag = None;
            self.slider_hover = false;
            self.box_hover = false;
        }
    }

    pub fn of(&self, owner: u64) -> Zoom {
        if self.owner == Some(owner) {
            self.zoom
        } else {
            Zoom::default()
        }
    }

    pub fn size(&self) -> (f32, f32) {
        let bounds = self.bounds.get();
        (f32::from(bounds.size.width), f32::from(bounds.size.height))
    }

    /// The pointer from the tile's centre.
    fn off_centre(&self, x: Pixels, y: Pixels) -> (f32, f32) {
        let bounds = self.bounds.get();
        let centre = bounds.center();
        (f32::from(x - centre.x), f32::from(y - centre.y))
    }

    /// `doZoom`: a change of level starts the two seconds of `isZooming`.
    fn do_zoom(&mut self, wanted: f32, at: (f32, f32), now: f64) {
        let zoom = zoom_to(self.zoom, wanted, at, self.size());
        if zoom.level != self.zoom.level {
            self.zoomed_at = Some(now);
        }
        self.zoom = zoom;
    }

    pub fn wheel(&mut self, x: Pixels, y: Pixels, delta_y: f32, now: f64) {
        let at = self.off_centre(x, y);
        let wanted = wheel_level(self.zoom.level, delta_y);
        self.wheeled_at = Some(now);
        self.do_zoom(wanted, at, now);
    }

    /// Zoom out or Zoom in: a step about the tile's centre.
    pub fn step(&mut self, by: f32, now: f64) {
        self.do_zoom(self.zoom.level + by, (0., 0.), now);
    }

    pub fn is_zooming(&self, now: f64) -> bool {
        self.zoomed_at.is_some_and(|at| now - at < ZOOMING_MS)
    }

    /// The minimap and the slider row show (`O || $`).
    pub fn shows_more(&self, now: f64) -> bool {
        self.is_zooming(now) || self.zoom.level > MIN
    }

    fn wheeling(&self, now: f64) -> bool {
        self.wheeled_at.is_some_and(|at| now - at < WHEELING_MS)
    }

    fn slidering(&self, now: f64) -> bool {
        self.slider_drag.is_some() || self.slid_at.is_some_and(|at| now - at < SLIDERING_MS)
    }

    /// `--custom-zoom-transition` is `transform 0.15s ease-out`, not `none`.
    pub fn picture_eases(&self, now: f64) -> bool {
        self.drag.is_none() && !self.wheeling(now) && !self.slidering(now)
    }

    /// `--custom-zoom-indicator-transition` is not `none`.
    pub fn indicator_eases(&self, now: f64) -> bool {
        self.picture_eases(now) && !self.minimap_drag
    }

    pub fn press(&mut self, x: Pixels, y: Pixels) {
        self.moved = false;
        if self.zoom.level > MIN {
            self.drag = Some(((f32::from(x), f32::from(y)), self.zoom.pan));
        }
    }

    pub fn drag_to(&mut self, x: Pixels, y: Pixels) {
        let Some((from, pan)) = self.drag else {
            return;
        };
        let moved = (f32::from(x) - from.0, f32::from(y) - from.1);
        if moved.0.abs() > 0.01 || moved.1.abs() > 0.01 {
            self.moved = true;
        }
        self.zoom.pan = clamp_pan(
            (pan.0 + moved.0, pan.1 + moved.1),
            self.zoom.level,
            self.size(),
        );
    }

    /// The minimap is pressed at a point: the tile centres there, and a drag keeps following.
    pub fn minimap_press(&mut self, x: Pixels, y: Pixels) {
        self.minimap_drag = true;
        self.minimap_to(x, y);
    }

    pub fn minimap_to(&mut self, x: Pixels, y: Pixels) {
        if !self.minimap_drag {
            return;
        }
        let map = self.minimap.get();
        let (width, height) = (f32::from(map.size.width), f32::from(map.size.height));
        if width <= 0. || height <= 0. {
            return;
        }
        let at = (
            f32::from(x - map.origin.x) / width,
            f32::from(y - map.origin.y) / height,
        );
        self.zoom.pan = minimap_pan(at, self.zoom.level, self.size());
    }

    fn track_fraction(&self, x: Pixels) -> f32 {
        let track = self.track.get();
        let width = f32::from(track.size.width);
        if width <= 0. {
            return 0.;
        }
        f32::from(x - track.origin.x) / width
    }

    /// A press on the slider: on the grabber it drags from where the value is; elsewhere the value jumps to the
    /// pointer first (react-aria's `useSliderThumb` and `useSlider`).
    pub fn slider_press(&mut self, x: Pixels, on_grabber: bool, now: f64) {
        let from = if on_grabber {
            slider_fraction(self.zoom.level)
        } else {
            self.track_fraction(x)
        };
        self.slider_drag = Some((f32::from(x), from));
        self.slide(slider_level(from), now);
    }

    pub fn slider_to(&mut self, x: Pixels, now: f64) {
        let Some((start, from)) = self.slider_drag else {
            return;
        };
        let width = f32::from(self.track.get().size.width);
        if width <= 0. {
            return;
        }
        let fraction = from + (f32::from(x) - start) / width;
        self.slide(slider_level(fraction), now);
    }

    /// The slider's `onChange`: `isSlidering`, then a zoom about the centre.
    fn slide(&mut self, level: f32, now: f64) {
        self.slid_at = Some(now);
        self.do_zoom(level, (0., 0.), now);
    }

    pub fn release(&mut self) {
        self.drag = None;
        self.minimap_drag = false;
        // The 100ms of `isSlidering` run on from the slider's last change.
        self.slider_drag = None;
    }

    /// Whether any press of the zoom is held.
    pub fn held(&self) -> bool {
        self.drag.is_some() || self.minimap_drag || self.slider_drag.is_some()
    }

    /// Whether the press that just ended panned; asked once by the click.
    pub fn take_moved(&mut self) -> bool {
        std::mem::take(&mut self.moved)
    }
}

/// The call container carries `theme-midnight`.
fn mid(name: &str) -> Rgba {
    rgba(Theme::Midnight.color(name))
}

/// The minimap's width for the stream's picture; `videoAspectRatio` is 16:9 until a picture is known (40056).
fn picture_width(image: Option<&Arc<RenderImage>>) -> f32 {
    let aspect = image.map_or(16. / 9., |image| {
        let shape = image.size(0);
        shape.width.0 as f32 / (shape.height.0 as f32).max(1.)
    });
    minimap_width(aspect)
}

/// The key of the slider grabber's tooltip.
const GRABBER_TIP: &str = "vstream-zoom-grabber";
/// `--grabber-size` and `--bar-size` of 310592, at rest and while the slider is hovered or dragged.
const GRABBER: (f32, f32) = (16., 20.);
const BAR: (f32, f32) = (4., 6.);
/// The two ends' buttons and the row's two gaps, beside the slider (`controlsRow_PkSv2u`, `gap: 8px`).
const ROW_BESIDE_SLIDER: f32 = 32. + 8. + 8. + 32.;

impl Root {
    /// A zoom just changed: a redraw once `isZooming` is over, so the minimap goes.
    fn zoom_changed(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(ZOOMING_MS as u64 + 16))
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        self.voice_view_moved(cx);
        cx.notify();
    }

    /// The wheel over the focused stream (555148 `el`).
    pub(super) fn stream_zoom_wheel(
        &mut self,
        x: Pixels,
        y: Pixels,
        delta_y: f32,
        cx: &mut Context<Self>,
    ) {
        self.stream_zoom.wheel(x, y, delta_y, motion::now());
        self.zoom_changed(cx);
    }

    /// The slider's value over its grabber, held open while it is dragged (`forceOpen: es`), in the window it is in.
    fn zoom_value_tip(&mut self, window: &Window) {
        let track = self.stream_zoom.track.get();
        let side = GRABBER.1;
        let x = track.origin.x + track.size.width * slider_fraction(self.stream_zoom.zoom.level);
        let anchor = Bounds::new(
            point(x - px(side / 2.), track.origin.y + px(12. - side / 2.)),
            size(px(side), px(side)),
        );
        let tip = Tip::body(
            Body::Text(percent(self.stream_zoom.zoom.level).into()),
            Side::Top,
        );
        self.tip = Some(
            Active::new(GRABBER_TIP.into(), tip, anchor)
                .in_window(window.window_handle().window_id()),
        );
    }

    /// The focused stream's zoom controls, in the tile's action row (module 117029). `image` is the stream's picture.
    pub(super) fn stream_zoom_controls(
        &self,
        image: Option<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = motion::now();
        let zoom = self.stream_zoom.zoom;
        let more = self.stream_zoom.shows_more(now);
        // `overlayButtonContainer_7NOr2f`: `opacity` 0 while the call view is idle, `.2s ease-in-out`.
        let opacity = motion::value(
            motion::key("vstream-zoom-shown"),
            if self.controls_shown() { 1. } else { 0. },
            css(200., Ease::EaseInOut),
        );
        let held = self.stream_zoom.minimap_drag || self.stream_zoom.slider_drag.is_some();
        let weak = cx.entity().downgrade();
        let drags = canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                if !held {
                    return;
                }
                let moved = weak.clone();
                window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                    if phase != gpui::DispatchPhase::Capture {
                        return;
                    }
                    let _ = moved.update(cx, |this, cx| {
                        let now = motion::now();
                        if this.stream_zoom.minimap_drag {
                            this.stream_zoom
                                .minimap_to(event.position.x, event.position.y);
                        } else {
                            this.stream_zoom.slider_to(event.position.x, now);
                            this.zoom_value_tip(window);
                        }
                        this.zoom_changed(cx);
                    });
                });
                let released = weak.clone();
                window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, _, cx| {
                    if phase != gpui::DispatchPhase::Capture {
                        return;
                    }
                    let _ = released.update(cx, |this, cx| {
                        // `forceOpen` ends: the value stays only while the pointer is on the grabber.
                        let off_grabber = this.tip.as_ref().is_some_and(|tip| {
                            tip.key() == GRABBER_TIP && !tip.anchor().contains(&event.position)
                        });
                        if this.stream_zoom.slider_drag.is_some() && off_grabber {
                            this.tip = None;
                        }
                        this.stream_zoom.release();
                        cx.notify();
                    });
                });
            },
        )
        .absolute()
        .size_0();
        let row_width = picture_width(image.as_ref());
        let minimap = more.then(|| self.zoom_minimap(image, row_width, zoom, now, cx));
        let row = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .when(more, |this| {
                this.w(px(row_width))
                    .child(self.zoom_button(false, zoom.level <= MIN, cx))
                    .child(self.zoom_slider(row_width - ROW_BESIDE_SLIDER, zoom.level, cx))
            })
            .child(self.zoom_button(true, zoom.level >= MAX, cx));
        div()
            .id("vstream-zoom")
            .flex()
            .flex_col()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .rounded(px(8.))
            .cursor_default()
            .opacity(opacity)
            .when(more, |this| {
                // `controlsWithChildren_PkSv2u`.
                this.bg(mid("--background-scrim"))
                    .rounded(px(12.))
                    .m(px(-4.))
                    .p(px(4.))
            })
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                this.stream_zoom.box_hover = *inside;
                if !*inside {
                    // `onAllowIdle("interact")`: the idle time starts again.
                    this.voice_view_moved(cx);
                }
                cx.notify();
            }))
            // `onClick: I` and the presses of its children stay off the tile.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .children(minimap)
            .child(row)
            .child(drags)
            .into_any_element()
    }

    /// `minimap_PkSv2u`: the whole picture, `contain`, with the part on screen outlined (`minimapIndicator_PkSv2u`).
    fn zoom_minimap(
        &self,
        image: Option<Arc<RenderImage>>,
        width: f32,
        zoom: Zoom,
        now: f64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let wanted = indicator(zoom, self.stream_zoom.size());
        let ease = if self.stream_zoom.indicator_eases(now) {
            css(100., Ease::EaseOut).full_motion()
        } else {
            css(0., Ease::EaseOut)
        };
        let shown = motion::animate(
            motion::key("vstream-zoom-indicator"),
            wanted.map(f64::from),
            ease,
        );
        let [left, top, across, down] = shown.map(|value| value as f32);
        let bounds = self.stream_zoom.minimap.clone();
        div()
            .id("vstream-zoom-minimap")
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(MINIMAP_HEIGHT))
            .rounded(px(8.))
            .overflow_hidden()
            .map(|this| {
                if self.stream_zoom.minimap_drag {
                    this.cursor_grabbing()
                } else {
                    this.cursor_grab()
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.stream_zoom
                        .minimap_press(event.position.x, event.position.y);
                    this.zoom_changed(cx);
                }),
            )
            .children(image.map(|image| {
                // The minimap's `overflow: hidden` rounds the picture's corners too; gpui clips to a rectangle.
                img(image)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(8.))
                    .object_fit(gpui::ObjectFit::Contain)
            }))
            .child(
                div()
                    .absolute()
                    .left(px(width * left))
                    .top(px(MINIMAP_HEIGHT * top))
                    .w(px(width * across))
                    .h(px(MINIMAP_HEIGHT * down))
                    .rounded(px(8.))
                    .border_2()
                    .border_color(mid("--opacity-blurple-64"))
                    .bg(mid("--opacity-blurple-16")),
            )
            .child(
                canvas(move |painted, _, _| bounds.set(painted), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .into_any_element()
    }

    /// Zoom in or Zoom out: an `overlay-secondary` `sm` icon button (`button_F7qvhY`), disabled at its end.
    fn zoom_button(&self, zoom_in: bool, disabled: bool, cx: &mut Context<Self>) -> AnyElement {
        let (id, glyph, text, by) = if zoom_in {
            ("vstream-zoom-in", "lb_zoom_in", "Zoom in", STEP)
        } else {
            ("vstream-zoom-out", "lb_zoom_out", "Zoom out", -STEP)
        };
        let button = div()
            .id(id)
            .size(px(32.))
            .flex_none()
            .rounded(px(8.))
            .border_1()
            .border_color(mid("--control-overlay-secondary-border-default"))
            .bg(mid("--control-overlay-secondary-background-default"))
            .flex()
            .items_center()
            .justify_center()
            .child(icon(glyph, 16., "--white"));
        if disabled {
            // `:disabled`: half opacity and no pointer events, so no hover and no tooltip.
            return div()
                .flex_none()
                .child(button.opacity(0.5))
                .into_any_element();
        }
        let button = button
            .cursor_pointer()
            .hover(|style| {
                style
                    .bg(mid("--control-overlay-secondary-background-hover"))
                    .border_color(mid("--control-overlay-secondary-border-hover"))
            })
            .active(|style| {
                style
                    .bg(mid("--control-overlay-secondary-background-active"))
                    .border_color(mid("--control-overlay-secondary-border-active"))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.stream_zoom.step(by, motion::now());
                this.zoom_changed(cx);
            }));
        self.with_tip(id, Tip::text(text, Side::Top), button, cx)
            .into_any_element()
    }

    /// The zoom level slider (module 310592, `slider_OnqacX` without marks): a bar through the middle of 24px, filled
    /// to the value, and a round grabber on the track 5px in from each end. Hovered or dragged, the grabber grows
    /// from 16 to 20 and the bar from 4 to 6 (a stiff spring held to `duration: 100`, linear).
    fn zoom_slider(&self, width: f32, level: f32, cx: &mut Context<Self>) -> AnyElement {
        let hot = self.stream_zoom.slider_hover || self.stream_zoom.slider_drag.is_some();
        let grow = css(100., Ease::Linear);
        let grabber = motion::value(
            motion::key("vstream-zoom-grabber-size"),
            if hot { GRABBER.1 } else { GRABBER.0 },
            grow,
        );
        let bar = motion::value(
            motion::key("vstream-zoom-bar-size"),
            if hot { BAR.1 } else { BAR.0 },
            grow,
        );
        let fraction = slider_fraction(level);
        let track_width = (width - 10.).max(0.);
        let track = self.stream_zoom.track.clone();
        let knob = div()
            .size(px(grabber))
            .rounded_full()
            .border_1()
            .border_color(mid("--border-normal"))
            .bg(mid("--white"))
            .shadow(vec![BoxShadow {
                // `--shadow-low` of the midnight theme.
                color: hsla(0., 0., 0., 0.14),
                offset: point(px(0.), px(1.)),
                blur_radius: px(4.),
                spread_radius: px(0.),
                inset: false,
            }])
            .cursor_ew_resize();
        let knob = self
            .with_tip(
                GRABBER_TIP,
                Tip::body(Body::Text(SharedString::from(percent(level))), Side::Top),
                knob,
                cx,
            )
            .absolute()
            .left(px(track_width * fraction - grabber / 2.))
            .top(px(12. - grabber / 2.));
        div()
            .id("vstream-zoom-slider")
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(24.))
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                this.stream_zoom.slider_hover = *inside;
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let grabber = this.tip_anchors.borrow().get(GRABBER_TIP).copied();
                    let on_grabber = grabber.is_some_and(|bounds| bounds.contains(&event.position));
                    this.stream_zoom
                        .slider_press(event.position.x, on_grabber, motion::now());
                    this.zoom_value_tip(window);
                    this.zoom_changed(cx);
                }),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px((24. - bar) / 2.))
                    .w_full()
                    .h(px(bar))
                    .rounded(px(4.))
                    .overflow_hidden()
                    .bg(mid("--slider-track-background"))
                    .child(
                        div()
                            .h_full()
                            .w(px(width * fraction))
                            .border_1()
                            .border_color(hsla(0., 0., 1., 0.1))
                            .bg(mid("--background-brand")),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(5.))
                    .right(px(5.))
                    .child(
                        canvas(move |painted, _, _| track.set(painted), |_, _, _, _| {})
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                    )
                    .child(knob),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f32, f32) = (800., 450.);

    fn state() -> State {
        let state = State::default();
        state.bounds.set(Bounds::new(
            point(px(0.), px(0.)),
            size(px(SIZE.0), px(SIZE.1)),
        ));
        state
    }

    #[test]
    fn a_wheel_step_is_a_tenth_and_no_more() {
        assert!((wheel_level(1., -100.) - 1.1).abs() < 1e-5);
        assert!((wheel_level(1., -900.) - 1.1).abs() < 1e-5);
        assert!((wheel_level(2.2, 100.) - 2.).abs() < 1e-5);
    }

    #[test]
    fn the_level_stays_between_one_and_five() {
        let zoom = zoom_to(Zoom::default(), 9., (0., 0.), SIZE);
        assert_eq!(zoom.level, 5.);
        assert_eq!(zoom_to(zoom, 0.2, (0., 0.), SIZE).level, 1.);
    }

    #[test]
    fn the_point_under_the_pointer_stays_there() {
        // A point 100px right of the centre: after doubling, the picture's centre is 100px to the left.
        let zoom = zoom_to(Zoom::default(), 2., (100., 0.), SIZE);
        assert_eq!(zoom.pan, (-100., 0.));
    }

    #[test]
    fn the_picture_never_leaves_the_tile() {
        assert_eq!(clamp_pan((900., -900.), 2., SIZE), (400., -225.));
        assert_eq!(clamp_pan((30., 30.), 1., SIZE), (0., 0.));
    }

    #[test]
    fn another_stream_starts_at_one() {
        let mut state = State::default();
        state.follow(Some(1));
        state.zoom.level = 3.;
        assert_eq!(state.of(1).level, 3.);
        assert_eq!(state.of(2).level, 1.);
        state.follow(Some(2));
        assert_eq!(state.zoom, Zoom::default());
    }

    #[test]
    fn the_buttons_step_a_quarter_about_the_centre_and_stop_at_the_ends() {
        let mut state = state();
        state.step(STEP, 0.);
        assert_eq!(
            state.zoom,
            Zoom {
                level: 1.25,
                pan: (0., 0.)
            }
        );
        state.zoom.level = 4.9;
        state.step(STEP, 0.);
        assert_eq!(state.zoom.level, MAX);
        state.step(-STEP, 0.);
        assert_eq!(state.zoom.level, 4.75);
    }

    #[test]
    fn the_more_controls_stay_two_seconds_after_the_last_zoom() {
        let mut state = state();
        assert!(!state.shows_more(0.));
        state.step(STEP, 1000.);
        state.step(-STEP, 1500.);
        assert_eq!(state.zoom.level, MIN);
        assert!(state.shows_more(3499.));
        assert!(!state.shows_more(3500.));
        // A step that changes nothing does not count as a zoom.
        state.step(-STEP, 4000.);
        assert!(!state.shows_more(4000.));
    }

    #[test]
    fn the_ease_is_off_while_the_wheel_or_the_slider_moves() {
        let mut state = state();
        assert!(state.picture_eases(0.));
        state.wheel(px(400.), px(225.), -100., 0.);
        assert!(!state.picture_eases(99.));
        assert!(state.picture_eases(100.));
        state.minimap_drag = true;
        assert!(state.picture_eases(100.));
        assert!(!state.indicator_eases(100.));
    }

    #[test]
    fn the_minimap_minds_the_picture_shape() {
        assert!((minimap_width(16. / 9.) - 213.333_33).abs() < 1e-3);
        assert_eq!(minimap_width(5.), 120. * 32. / 9.);
    }

    #[test]
    fn the_indicator_outlines_the_part_on_screen() {
        assert_eq!(indicator(Zoom::default(), SIZE), [0., 0., 1., 1.]);
        // At 2x panned all the way right, the left half of the picture is on screen.
        let zoom = Zoom {
            level: 2.,
            pan: (400., 0.),
        };
        assert_eq!(indicator(zoom, SIZE), [0., 0.25, 0.5, 0.5]);
    }

    #[test]
    fn a_press_on_the_minimap_centres_the_tile_there() {
        let mut state = state();
        state.zoom.level = 2.;
        state.minimap.set(Bounds::new(
            point(px(100.), px(100.)),
            size(px(200.), px(120.)),
        ));
        state.minimap_press(px(150.), px(160.));
        // A quarter in from the left: the picture moves right by a quarter of its 1600px width.
        assert_eq!(state.zoom.pan, (400., 0.));
        assert_eq!(indicator(state.zoom, SIZE)[0], 0.);
    }

    #[test]
    fn the_slider_runs_from_one_to_five_in_hundredths() {
        assert_eq!(slider_level(0.), 1.);
        assert_eq!(slider_level(1.), 5.);
        assert!((slider_level(0.3333) - 2.33).abs() < 1e-5);
        assert_eq!(slider_fraction(3.), 0.5);
        assert_eq!(percent(1.25), "125%");
        assert_eq!(percent(2.334), "233%");
    }

    #[test]
    fn a_grabber_drag_moves_by_the_pointer_and_a_track_press_jumps() {
        let mut state = state();
        state
            .track
            .set(Bounds::new(point(px(10.), px(0.)), size(px(100.), px(24.))));
        state.slider_press(px(60.), false, 0.);
        assert_eq!(state.zoom.level, 3.);
        state.release();
        // Pressed 3px right of the grabber's centre, the value does not jump.
        state.slider_press(px(63.), true, 0.);
        assert_eq!(state.zoom.level, 3.);
        state.slider_to(px(88.), 10.);
        assert_eq!(state.zoom.level, 4.);
        assert!(!state.picture_eases(500.));
        state.release();
        assert!(state.picture_eases(500.));
    }
}
