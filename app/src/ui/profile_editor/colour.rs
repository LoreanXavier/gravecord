//! The colour picker popout (module 508274, `T`): a saturation and brightness box, a hue slider, a hex field with
//! its "#" and the eyedropper, and the suggested swatches. Used by the banner colour and by the colour swatches of
//! the display name style editor.

use super::text;
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use crate::ui::{Root, color};
use gpui::{
    AnyElement, Bounds, Context, Entity, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    Pixels, Subscription, canvas, div, hsla, linear_color_stop, linear_gradient, prelude::*, px,
    rgba, svg,
};
use std::cell::Cell;
use std::rc::Rc;

/// What the colour being picked is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Banner,
    /// The nth colour of the name style being edited.
    Style(usize),
}

/// The five suggestions of the popout, in the order of the capture (`suggestedColors` of module 927071 are taken
/// from the avatar; the default avatar's are fixed).
pub(super) const SUGGESTED: [u32; 5] = [0x5c64f4, 0xd2d5fc, 0x5cf4a0, 0xf4ec5c, 0xf45cb0];

pub(super) struct State {
    pub target: Target,
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
    pub hex: Entity<Editor>,
    boxed: Rc<Cell<Bounds<Pixels>>>,
    slider: Rc<Cell<Bounds<Pixels>>>,
    _events: Subscription,
}

pub(super) fn to_hsv(rgb: u32) -> (f32, f32, f32) {
    let r = ((rgb >> 16) & 0xff) as f32 / 255.;
    let g = ((rgb >> 8) & 0xff) as f32 / 255.;
    let b = (rgb & 0xff) as f32 / 255.;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0. {
        0.
    } else if max == r {
        60. * (((g - b) / delta).rem_euclid(6.))
    } else if max == g {
        60. * ((b - r) / delta + 2.)
    } else {
        60. * ((r - g) / delta + 4.)
    };
    (hue, if max == 0. { 0. } else { delta / max }, max)
}

pub(super) fn from_hsv(hue: f32, saturation: f32, value: f32) -> u32 {
    let chroma = value * saturation;
    let x = chroma * (1. - ((hue / 60.) % 2. - 1.).abs());
    let m = value - chroma;
    let (r, g, b) = match (hue / 60.).floor() as i32 {
        0 => (chroma, x, 0.),
        1 => (x, chroma, 0.),
        2 => (0., chroma, x),
        3 => (0., x, chroma),
        4 => (x, 0., chroma),
        _ => (chroma, 0., x),
    };
    let channel = |value: f32| ((value + m) * 255.).round().clamp(0., 255.) as u32;
    (channel(r) << 16) | (channel(g) << 8) | channel(b)
}

impl State {
    pub(super) fn new(target: Target, initial: u32, cx: &mut Context<Root>) -> Self {
        let hex = cx.new(|cx| {
            let mut editor = Editor::new(crate::ui::editor_style(), cx);
            editor.set_single_line(true);
            editor.set_max_utf16(6);
            editor.set_text(&format!("{initial:06x}"));
            editor
        });
        let events = cx.subscribe(&hex, |this, editor, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited) {
                let text = editor.read(cx).text().trim_start_matches('#').to_owned();
                if text.len() == 6
                    && let Ok(value) = u32::from_str_radix(&text, 16)
                {
                    this.profile_editor_set_colour(value, false, cx);
                }
            }
        });
        let (hue, saturation, value) = to_hsv(initial);
        Self {
            target,
            hue,
            saturation,
            value,
            hex,
            boxed: Rc::default(),
            slider: Rc::default(),
            _events: events,
        }
    }

    pub(super) fn rgb(&self) -> u32 {
        from_hsv(self.hue, self.saturation, self.value)
    }
}

fn hue_bar(cx: &mut Context<Root>, slider: Rc<Cell<Bounds<Pixels>>>, hue: f32) -> AnyElement {
    let stops: [u32; 7] = [
        0xff0000, 0xffff00, 0x00ff00, 0x00ffff, 0x0000ff, 0xff00ff, 0xff0000,
    ];
    let segments = stops.windows(2).map(|pair| {
        div().flex_1().h_full().bg(linear_gradient(
            90.,
            linear_color_stop(rgba((pair[0] << 8) | 0xff), 0.),
            linear_color_stop(rgba((pair[1] << 8) | 0xff), 1.),
        ))
    });
    let bounds = slider.clone();
    let set = move |position: gpui::Point<Pixels>, this: &mut Root, cx: &mut Context<Root>| {
        let area = bounds.get();
        if area.size.width <= px(0.) {
            return;
        }
        let ratio = ((position.x - area.origin.x) / area.size.width).clamp(0., 1.);
        this.profile_editor_set_hue(ratio * 360., cx);
    };
    let set_move = set.clone();
    div()
        .id("colour-hue")
        .relative()
        .w(px(220.))
        .h(px(8.))
        .rounded(px(4.))
        .flex()
        .child(
            canvas(move |area, _, _| slider.set(area), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .children(segments)
        .child(
            div()
                .absolute()
                .top(px(-3.))
                .left(px(hue / 360. * 220. - 7.))
                .size(px(14.))
                .rounded_full()
                .border_2()
                .border_color(rgba(0xffffffff))
                .bg(rgba((from_hsv(hue, 1., 1.) << 8) | 0xff)),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| set(event.position, this, cx)),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                set_move(event.position, this, cx);
            }
        }))
        .into_any_element()
}

impl Root {
    /// The popout, placed at `left`/`top` of the editor.
    pub(super) fn profile_editor_colour_popout(
        &mut self,
        left: f32,
        top: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self
            .profile_editor
            .as_ref()?
            .banner_color
            .as_ref()
            .or_else(|| {
                self.profile_editor
                    .as_ref()?
                    .styles
                    .as_ref()?
                    .picking
                    .as_ref()
            })?;
        let (hue, saturation, value) = (state.hue, state.saturation, state.value);
        let hex = state.hex.clone();
        let boxed = state.boxed.clone();
        let slider = state.slider.clone();
        let rgb = state.rgb();
        let area = boxed.clone();
        let pick = move |position: gpui::Point<Pixels>, this: &mut Root, cx: &mut Context<Root>| {
            let rect = area.get();
            if rect.size.width <= px(0.) || rect.size.height <= px(0.) {
                return;
            }
            let s = ((position.x - rect.origin.x) / rect.size.width).clamp(0., 1.);
            let v = 1. - ((position.y - rect.origin.y) / rect.size.height).clamp(0., 1.);
            this.profile_editor_set_saturation(s, v, cx);
        };
        let pick_move = pick.clone();
        let saturation_box = div()
            .id("colour-box")
            .relative()
            .w(px(220.))
            .h(px(150.))
            .rounded(px(4.))
            .overflow_hidden()
            .bg(linear_gradient(
                90.,
                linear_color_stop(rgba(0xffffffff), 0.),
                linear_color_stop(rgba((from_hsv(hue, 1., 1.) << 8) | 0xff), 1.),
            ))
            .child(
                canvas(move |bounds, _, _| boxed.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(div().absolute().size_full().bg(linear_gradient(
                180.,
                linear_color_stop(rgba(0x00000000), 0.),
                linear_color_stop(rgba(0x000000ff), 1.),
            )))
            .child(
                div()
                    .absolute()
                    .left(px(saturation * 220. - 4.))
                    .top(px((1. - value) * 150. - 4.))
                    .size(px(8.))
                    .rounded_full()
                    .border_1()
                    .border_color(rgba(0xffffffff)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    pick(event.position, this, cx)
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    pick_move(event.position, this, cx);
                }
            }));
        let mut swatches = Vec::new();
        for (index, value) in SUGGESTED.iter().copied().enumerate() {
            swatches.push(
                div()
                    .id(gpui::SharedString::from(format!(
                        "colour-suggested-{index}"
                    )))
                    .size(px(36.))
                    .rounded(px(4.))
                    .bg(rgba((value << 8) | 0xff))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.profile_editor_set_colour(value, true, cx);
                    })),
            );
        }
        let _ = rgb;
        Some(
            div()
                .parity("profile-editor-colour-popout")
                .id("profile-editor-colour-popout")
                .occlude()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(256.))
                .p(px(16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(saturation_box)
                .child(div().mt(px(-4.)).child(hue_bar(cx, slider, hue)))
                .child(
                    div()
                        .mt(px(12.))
                        .h(px(40.))
                        .px(px(8.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--input-border-active"))
                        .bg(color("--input-background-default"))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(text("#", 16., 20., 400., "--text-muted"))
                        .child(div().flex_1().child(hex))
                        .child(
                            svg()
                                .path("icons/profile_editor_eyedropper.svg")
                                .size(px(16.))
                                .text_color(color("--icon-default")),
                        ),
                )
                .child(div().mt(px(4.)).flex().gap(px(8.)).children(swatches))
                .into_any_element(),
        )
    }

    pub(super) fn profile_editor_set_hue(&mut self, hue: f32, cx: &mut Context<Self>) {
        let Some(state) = self.profile_editor_colour_state() else {
            return;
        };
        state.hue = hue;
        let rgb = state.rgb();
        self.profile_editor_colour_applied(rgb, cx);
    }

    pub(super) fn profile_editor_set_saturation(&mut self, s: f32, v: f32, cx: &mut Context<Self>) {
        let Some(state) = self.profile_editor_colour_state() else {
            return;
        };
        state.saturation = s;
        state.value = v;
        let rgb = state.rgb();
        self.profile_editor_colour_applied(rgb, cx);
    }

    /// A colour chosen by hex or a swatch. `sync` writes it back into the hex field (a typed value is already
    /// there).
    pub(super) fn profile_editor_set_colour(
        &mut self,
        rgb: u32,
        sync: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.profile_editor_colour_state() else {
            return;
        };
        let (h, s, v) = to_hsv(rgb);
        // A grey keeps the hue the slider has.
        if s > 0. {
            state.hue = h;
        }
        state.saturation = s;
        state.value = v;
        if sync {
            let hex = state.hex.clone();
            hex.update(cx, |editor, _| editor.set_text(&format!("{rgb:06x}")));
        }
        self.profile_editor_colour_applied(rgb, cx);
    }

    fn profile_editor_colour_state(&mut self) -> Option<&mut State> {
        let open = self.profile_editor.as_mut()?;
        if open.banner_color.is_some() {
            open.banner_color.as_mut()
        } else {
            open.styles.as_mut()?.picking.as_mut()
        }
    }

    /// The colour changed: the banner colour is a pending change, a style colour edits the style being built.
    fn profile_editor_colour_applied(&mut self, rgb: u32, cx: &mut Context<Self>) {
        let saved_hex = self
            .profile_editor
            .as_ref()
            .map(|open| open.saved.accent_color);
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        if let Some(state) = open.banner_color.as_ref() {
            let hex = state.hex.clone();
            let shown = hex.read(cx).text().to_owned();
            if shown.len() != 6 || u32::from_str_radix(&shown, 16).ok() != Some(rgb) {
                hex.update(cx, |editor, _| editor.set_text(&format!("{rgb:06x}")));
            }
            open.pending.accent_color = (saved_hex.flatten() != Some(rgb)).then_some(Some(rgb));
        } else if let Some(styles) = open.styles.as_mut()
            && let Some(picking) = styles.picking.as_ref()
        {
            let hex = picking.hex.clone();
            let target = picking.target;
            let shown = hex.read(cx).text().to_owned();
            if shown.len() != 6 || u32::from_str_radix(&shown, 16).ok() != Some(rgb) {
                hex.update(cx, |editor, _| editor.set_text(&format!("{rgb:06x}")));
            }
            if let Target::Style(index) = target {
                styles.set_color(index, rgb);
            }
        }
        let _ = hsla(0., 0., 0., 0.);
        cx.notify();
    }
}
