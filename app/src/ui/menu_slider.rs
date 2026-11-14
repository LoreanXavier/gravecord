//! Discord's shared menu volume/range slider control (modules `310592`, `964355`, `792251`).
//!
//! Renders a `58px` menu row with a top label (no static `%` suffix in the header), a `24px` interactive slider
//! track with `5px` horizontal track inset so the `20px` circular grabber never clips at `0%` or `100%`/`200%`, a
//! `4px -> 6px` bar and `16px -> 20px` grabber transition over `100ms` on hover or active drag, and a live `%`
//! `tooltip::Active` bubble centered above the grabber thumb.

use super::motion::{self, Ease, css};
use super::tooltip::{Active, Body, Side, Tip};
use super::{Root, color};
use gpui::{
    AnyElement, Bounds, BoxShadow, Context, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Rgba, SharedString, Window, WindowId, div, hsla, point,
    prelude::*, px, relative, rgba, size,
};
use std::rc::Rc;

/// Menu slider row height (`34px` label header + `24px` slider track).
pub(super) const ROW_HEIGHT: f32 = 58.;
/// Outer interactive slider track row height (`--slider-height` / `.slider_OnqacX`).
pub(super) const SLIDER_HEIGHT: f32 = 24.;
/// Horizontal track inset inside the slider row (`--custom-track-inset: 5px`).
pub(super) const TRACK_INSET: f32 = 5.;

const BAR_IDLE: f32 = 4.;
const BAR_ACTIVE: f32 = 6.;
const GRABBER_IDLE: f32 = 16.;
const GRABBER_ACTIVE: f32 = 20.;
const TRANSITION_MS: f64 = 100.;

/// Color resolution palette for menus rendered in standard, Midnight (in-call), or Onyx (overlay) themes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ThemePalette {
    #[default]
    Default,
    Midnight,
    Onyx,
}

impl ThemePalette {
    pub(super) fn resolve(self, name: &str) -> Rgba {
        match self {
            Self::Default => color(name),
            Self::Midnight => rgba(dc_tokens::Theme::Midnight.color(name)),
            Self::Onyx => super::overlay::onyx(name),
        }
    }
}

/// Callback invoked with the updated slider step during pointer drag or click.
pub(crate) type SliderChange = Rc<dyn Fn(&mut Root, u32, &mut Context<Root>)>;

/// Active window-level menu slider drag state tracked on [`Root`].
#[derive(Clone)]
pub(crate) struct ActiveSliderDrag {
    pub id: SharedString,
    pub tip_key: SharedString,
    pub window: WindowId,
    pub track: Bounds<Pixels>,
    pub hit_bounds: Bounds<Pixels>,
    pub min: u32,
    pub max: u32,
    pub format: Rc<dyn Fn(u32) -> String>,
    pub on_change: SliderChange,
}

/// Configuration for [`Root::menu_slider_row`].
pub(super) struct MenuSliderConfig {
    pub id: SharedString,
    pub label: String,
    pub min: u32,
    pub max: u32,
    pub value: u32,
    pub palette: ThemePalette,
    pub format: Rc<dyn Fn(u32) -> String>,
    pub on_change: SliderChange,
}

/// Default formatter displaying integer percentages (e.g. `"100%"`, `964355` `${e.toFixed(0)}%`).
pub(super) fn percent_format() -> Rc<dyn Fn(u32) -> String> {
    Rc::new(|pct| format!("{pct}%"))
}

/// Raw `792251.M`: converts linear amplitude (`1.0` = unity) to perceptual slider percentage (`100.0` = unity).
pub(super) fn amplitude_to_perceptual_raw(amplitude: f32) -> f32 {
    if !amplitude.is_finite() || amplitude <= 0. {
        return 0.;
    }
    let position = if amplitude < 1. {
        amplitude.powf(1. / 2.8)
    } else {
        20. * amplitude.log10() / 6. + 1.
    };
    position * 100.
}

/// Raw `792251.w`: converts perceptual slider percentage (`0..=200`) to linear amplitude (`1.0` = unity).
pub(super) fn perceptual_to_amplitude_raw(perceptual_pct: f32) -> f32 {
    if !perceptual_pct.is_finite() || perceptual_pct <= 0. {
        return 0.;
    }
    let ratio = perceptual_pct / 100.;
    if ratio < 1. {
        ratio.powf(2.8)
    } else {
        10f32.powf((ratio - 1.) * 6. / 20.)
    }
}

/// Converts an integer perceptual step (`0..=max_pct`, `max_pct` = `100` or `200`) to linear amplitude (`792251.w`).
pub(super) fn perceptual_to_amplitude(pct: u32, max_pct: u32) -> f32 {
    perceptual_to_amplitude_raw(pct.min(max_pct) as f32)
}

/// Converts a linear amplitude (`0.0..=2.0`) to a quantized `1%` integer step in `0..=max_pct` (`792251.M`).
pub(super) fn amplitude_to_perceptual(amplitude: f32, max_pct: u32) -> u32 {
    amplitude_to_perceptual_raw(amplitude)
        .round()
        .clamp(0., max_pct as f32) as u32
}

/// Snaps a normalized pointer fraction (`0.0..=1.0`) to an integer step in `min..=max`.
pub(super) fn snap_step(fraction: f32, min: u32, max: u32) -> u32 {
    if max <= min || !fraction.is_finite() {
        return min;
    }
    let span = (max - min) as f32;
    (min as f32 + fraction.clamp(0., 1.) * span).round() as u32
}

/// Maps an integer step in `min..=max` to `[0.0, 1.0]`.
pub(super) fn step_fraction(value: u32, min: u32, max: u32) -> f32 {
    if max <= min {
        return 0.;
    }
    let clamped = value.clamp(min, max);
    (clamped - min) as f32 / (max - min) as f32
}

fn fraction_at(track: Bounds<Pixels>, x: Pixels) -> f32 {
    let width = f32::from(track.size.width);
    if width <= 0. {
        return 0.;
    }
    ((f32::from(x) - f32::from(track.left())) / width).clamp(0., 1.)
}

fn expanded_hit_bounds(track: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        point(track.origin.x - px(TRACK_INSET), track.origin.y),
        size(
            track.size.width + px(TRACK_INSET * 2.),
            track.size.height.max(px(SLIDER_HEIGHT)),
        ),
    )
}

fn thumb_anchor(track: Bounds<Pixels>, fraction: f32, grabber_size: f32) -> Bounds<Pixels> {
    let thumb_x = track.origin.x + track.size.width * fraction.clamp(0., 1.);
    Bounds::new(
        point(
            thumb_x - px(grabber_size / 2.),
            track.origin.y + px(SLIDER_HEIGHT / 2. - grabber_size / 2.),
        ),
        size(px(grabber_size), px(grabber_size)),
    )
}

fn show_slider_tip(
    this: &mut Root,
    tip_key: SharedString,
    track: Bounds<Pixels>,
    fraction: f32,
    text: String,
    window: &Window,
) {
    let anchor = thumb_anchor(track, fraction, GRABBER_ACTIVE);
    let tip = Tip::body(Body::Text(text.into()), Side::Top);
    this.tip = Some(Active::new(tip_key, tip, anchor).in_window(window.window_handle().window_id()));
}

/// Shared `58px` menu slider row container with top label ONLY (no static `%` suffix in the header).
pub(super) fn row_frame(
    id: impl Into<SharedString>,
    label: String,
    palette: ThemePalette,
    slider: AnyElement,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .h(px(ROW_HEIGHT))
        .flex_none()
        .flex()
        .flex_col()
        .child(
            div()
                .p(px(8.))
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(palette.resolve("--text-strong"))
                .whitespace_nowrap()
                .child(label),
        )
        .child(slider)
}

/// Shared bar + grabber visuals for menu sliders (`4->6px` bar, `16->20px` white circular grabber with `--border-normal`).
pub(super) fn track_layers(
    anim_id: &str,
    fraction: f32,
    active: bool,
    palette: ThemePalette,
) -> (AnyElement, AnyElement) {
    let fraction = fraction.clamp(0., 1.);
    let progress = motion::value(
        motion::key(anim_id),
        if active { 1. } else { 0. },
        css(TRANSITION_MS, Ease::Ease),
    );
    let bar_h = BAR_IDLE + (BAR_ACTIVE - BAR_IDLE) * progress;
    let grabber_size = GRABBER_IDLE + (GRABBER_ACTIVE - GRABBER_IDLE) * progress;

    // `--slider-track-background` matches `.bar_OnqacX`; `--brand-500` fills the active left segment.
    let bar = div()
        .absolute()
        .left_0()
        .right_0()
        .top(px((SLIDER_HEIGHT - bar_h) / 2.))
        .h(px(bar_h))
        .rounded(px(4.))
        .overflow_hidden()
        .bg(palette.resolve("--slider-track-background"))
        .child(
            div()
                .h_full()
                .w(relative(fraction))
                .bg(palette.resolve("--brand-500")),
        )
        .into_any_element();

    let grabber = div()
        .absolute()
        .left(relative(fraction))
        .ml(px(-grabber_size / 2.))
        .top(px((SLIDER_HEIGHT - grabber_size) / 2.))
        .size(px(grabber_size))
        .rounded_full()
        .bg(gpui::white())
        .border_1()
        .border_color(palette.resolve("--border-normal"))
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.14),
            offset: point(px(0.), px(1.)),
            blur_radius: px(4.),
            spread_radius: px(0.),
            inset: false,
        }])
        .into_any_element();

    (bar, grabber)
}

impl Root {
    /// Renders a Discord `MenuSliderControl` row wired with live hover/drag `%` tooltip and window-level drag.
    pub(super) fn menu_slider_row(
        &self,
        cfg: MenuSliderConfig,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let MenuSliderConfig {
            id,
            label,
            min,
            max,
            value,
            palette,
            format,
            on_change,
        } = cfg;
        let row_id: SharedString = format!("{id}-row").into();
        let track_key: SharedString = format!("{id}-track").into();
        let tip_key: SharedString = format!("{id}-tip").into();
        let fraction = step_fraction(value, min, max);

        let dragging = self
            .active_menu_slider_drag
            .as_ref()
            .is_some_and(|drag| drag.id == id);
        let hovering = self.tip.as_ref().is_some_and(|tip| tip.key() == tip_key.as_ref());
        let active = dragging || hovering;
        let (bar, grabber) = track_layers(&format!("menu-slider-{id}"), fraction, active, palette);

        let down_id = id.clone();
        let down_track = track_key.clone();
        let down_tip = tip_key.clone();
        let down_format = Rc::clone(&format);
        let down_change = Rc::clone(&on_change);

        let hover_id = id.clone();
        let hover_track = track_key.clone();
        let hover_tip = tip_key.clone();
        let hover_format = Rc::clone(&format);

        let slider = div()
            .id(id)
            .relative()
            .mx(px(8.))
            .h(px(SLIDER_HEIGHT))
            .cursor_ew_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let Some(track) = this.tip_anchors.borrow().get(&down_track).copied() else {
                        return;
                    };
                    let hit_bounds = expanded_hit_bounds(track);
                    let next_fraction = fraction_at(track, event.position.x);
                    let next_value = snap_step(next_fraction, min, max);
                    let snapped_fraction = step_fraction(next_value, min, max);

                    this.active_menu_slider_drag = Some(ActiveSliderDrag {
                        id: down_id.clone(),
                        tip_key: down_tip.clone(),
                        window: window.window_handle().window_id(),
                        track,
                        hit_bounds,
                        min,
                        max,
                        format: Rc::clone(&down_format),
                        on_change: Rc::clone(&down_change),
                    });
                    (down_change)(this, next_value, cx);
                    show_slider_tip(
                        this,
                        down_tip.clone(),
                        track,
                        snapped_fraction,
                        (down_format)(next_value),
                        window,
                    );
                    cx.notify();
                }),
            )
            .on_hover(cx.listener(move |this, inside: &bool, window, cx| {
                let is_dragging_this = this
                    .active_menu_slider_drag
                    .as_ref()
                    .is_some_and(|drag| drag.id == hover_id);
                if *inside {
                    let track = this.tip_anchors.borrow().get(&hover_track).copied();
                    if let Some(track) = track {
                        show_slider_tip(
                            this,
                            hover_tip.clone(),
                            track,
                            fraction,
                            (hover_format)(value),
                            window,
                        );
                        cx.notify();
                    }
                } else if !is_dragging_this
                    && this
                        .tip
                        .as_ref()
                        .is_some_and(|tip| tip.key() == hover_tip.as_ref())
                {
                    this.tip = None;
                    cx.notify();
                }
            }))
            .child(bar)
            .child(
                div()
                    .absolute()
                    .left(px(TRACK_INSET))
                    .right(px(TRACK_INSET))
                    .top_0()
                    .bottom_0()
                    .child(self.anchor_probe(track_key))
                    .child(grabber),
            )
            .into_any_element();

        row_frame(row_id, label, palette, slider).into_any_element()
    }

    /// Window-level `DispatchPhase::Capture` pointer-move handler for active menu slider drags.
    pub(super) fn menu_slider_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.active_menu_slider_drag.clone() else {
            return;
        };
        if window.window_handle().window_id() != drag.window {
            return;
        }
        if !event.dragging() {
            self.active_menu_slider_drag = None;
            if self
                .tip
                .as_ref()
                .is_some_and(|tip| tip.key() == drag.tip_key.as_ref())
            {
                self.tip = None;
            }
            cx.notify();
            return;
        }
        cx.stop_propagation();
        let fraction = fraction_at(drag.track, event.position.x);
        let value = snap_step(fraction, drag.min, drag.max);
        let snapped_fraction = step_fraction(value, drag.min, drag.max);
        (drag.on_change)(self, value, cx);
        show_slider_tip(
            self,
            drag.tip_key.clone(),
            drag.track,
            snapped_fraction,
            (drag.format)(value),
            window,
        );
        cx.notify();
    }

    /// Window-level `DispatchPhase::Capture` mouse-up handler releasing active menu slider drags.
    pub(super) fn menu_slider_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.active_menu_slider_drag.as_ref() else {
            return;
        };
        if event.button != MouseButton::Left || window.window_handle().window_id() != drag.window {
            return;
        }
        let drag = self.active_menu_slider_drag.take().expect("checked above");
        if !drag.hit_bounds.contains(&event.position)
            && self
                .tip
                .as_ref()
                .is_some_and(|tip| tip.key() == drag.tip_key.as_ref())
        {
            self.tip = None;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perceptual_curve_matches_792251_reference_points() {
        assert_eq!(perceptual_to_amplitude(0, 100), 0.);
        assert_eq!(perceptual_to_amplitude(0, 200), 0.);
        assert!((perceptual_to_amplitude(50, 100) - 0.143_587).abs() < 1e-4);
        assert_eq!(perceptual_to_amplitude(100, 100), 1.);
        assert_eq!(perceptual_to_amplitude(100, 200), 1.);
        assert!((perceptual_to_amplitude(200, 200) - 1.995_262).abs() < 1e-4);

        assert_eq!(amplitude_to_perceptual(0., 200), 0);
        assert_eq!(amplitude_to_perceptual(0.143_587, 200), 50);
        assert_eq!(amplitude_to_perceptual(1., 200), 100);
        assert_eq!(amplitude_to_perceptual(1.995_262, 200), 200);
        assert_eq!(amplitude_to_perceptual(2.0, 200), 200);
    }

    #[test]
    fn perceptual_curve_round_trips_every_integer_percent_step() {
        for max_pct in [100, 200] {
            for pct in 0..=max_pct {
                let amp = perceptual_to_amplitude(pct, max_pct);
                assert_eq!(
                    amplitude_to_perceptual(amp, max_pct),
                    pct,
                    "round-trip failed for pct={pct} max_pct={max_pct}"
                );
            }
        }
    }

    #[test]
    fn snap_step_quantizes_to_integer_steps_and_clamps() {
        assert_eq!(snap_step(-0.25, 0, 200), 0);
        assert_eq!(snap_step(0., 0, 200), 0);
        assert_eq!(snap_step(0.248, 0, 200), 50);
        assert_eq!(snap_step(0.5, 0, 200), 100);
        assert_eq!(snap_step(1.25, 0, 200), 200);
        assert_eq!(snap_step(f32::NAN, 1, 100), 1);
        assert_eq!(snap_step(0.504, 0, 100), 50);
        assert_eq!(snap_step(0.506, 0, 100), 51);
    }
}
