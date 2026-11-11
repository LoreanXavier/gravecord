//! The Custom Theme editor a free account gets from "Try it out" (module 500779 and its panel, measured on
//! `custom-theme-editor-nonnitro-darker-cozy@1.png` and its DOM dump): settings close, the app narrows to the left
//! and takes the preview theme, a 290 px panel on the right holds the Appearance pair, the colour picker, the
//! intensity slider, "Surprise Me!" and "Reset". Nothing is saved: a non-Nitro account only previews.
//!
//! Defaults and rules read in the bundle: colours `[#5865F2]` (`a0`), angle 0, intensity 74 (`chassisMixAmount`),
//! the hex field takes six digits, "Add Color" adds the last colour with its hue 15 degrees on (at most five), "Surprise
//! Me!" is module 935444 `mf`.

use super::client_theme::{self, Custom, tinted};
use super::{Root, color, mask_icon};
use crate::editor::{Editor, EditorEvent};
use crate::ui::editor_style;
use gpui::{
    AnyElement, Bounds, Context, Entity, FontWeight, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Subscription, canvas, div, linear_color_stop, linear_gradient,
    prelude::*, px, rgba,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The panel: `container_K7aN4Z`, 290 wide at the right edge of the 1280 window.
pub(super) const PANEL_WIDTH: f32 = 290.;
const DEFAULT_COLOUR: u32 = 0x5865f2;
const DEFAULT_MIX: f32 = 74.;
const MAX_COLOURS: usize = 5;
/// `a` of module 935444: the colours "Surprise Me!" draws from.
const SURPRISE_COLOURS: [u32; 14] = [
    0x94e0cf, 0x9af0b1, 0x9a90ff, 0x9a53ff, 0xfda6e4, 0xffe6c0, 0xefb4aa, 0x56b69f, 0x29c566,
    0x5348ca, 0x6d24d4, 0xca48c8, 0xf0ae29, 0xdf4232,
];
const SURPRISE_ANGLES: [f32; 8] = [0., 45., 90., 135., 180., 225., 270., 315.];
const SURPRISE_INTENSITIES: [f32; 4] = [20., 40., 60., 80.];

struct State {
    custom: Custom,
    selected: usize,
    hue: f32,
    saturation: f32,
    value: f32,
    hex: Entity<Editor>,
    intensity: Entity<Editor>,
    square: Rc<Cell<Bounds<Pixels>>>,
    hue_bar: Rc<Cell<Bounds<Pixels>>>,
    track: Rc<Cell<Bounds<Pixels>>>,
    _events: [Subscription; 2],
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn default_custom() -> Custom {
    Custom {
        colors: vec![DEFAULT_COLOUR],
        angle: 0.,
        base_mix: DEFAULT_MIX,
        light: false,
    }
}

fn hex_text(rgb: u32) -> String {
    format!("#{rgb:06X}")
}

fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.trim().trim_start_matches('#');
    (digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
}

fn to_hsl(rgb: u32) -> (f32, f32, f32) {
    let (r, g, b) = (
        ((rgb >> 16) & 0xff) as f32 / 255.,
        ((rgb >> 8) & 0xff) as f32 / 255.,
        (rgb & 0xff) as f32 / 255.,
    );
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.;
    let delta = max - min;
    if delta == 0. {
        return (0., 0., l);
    }
    let s = delta / (1. - (2. * l - 1.).abs());
    let h = if max == r {
        60. * ((g - b) / delta).rem_euclid(6.)
    } else if max == g {
        60. * ((b - r) / delta + 2.)
    } else {
        60. * ((r - g) / delta + 4.)
    };
    (h, s, l)
}

fn from_hsl(hue: f32, s: f32, l: f32) -> u32 {
    let hue = hue.rem_euclid(360.);
    let chroma = (1. - (2. * l - 1.).abs()) * s;
    let x = chroma * (1. - ((hue / 60.) % 2. - 1.).abs());
    let m = l - chroma / 2.;
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

fn mix_rgb(a: u32, b: u32) -> u32 {
    let channel = |shift: u32| ((((a >> shift) & 0xff) + ((b >> shift) & 0xff)) / 2) << shift;
    channel(16) | channel(8) | channel(0)
}

fn to_hsv(rgb: u32) -> (f32, f32, f32) {
    let (r, g, b) = (
        ((rgb >> 16) & 0xff) as f32 / 255.,
        ((rgb >> 8) & 0xff) as f32 / 255.,
        (rgb & 0xff) as f32 / 255.,
    );
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let delta = max - min;
    let hue = if delta == 0. {
        0.
    } else if max == r {
        60. * ((g - b) / delta).rem_euclid(6.)
    } else if max == g {
        60. * ((b - r) / delta + 2.)
    } else {
        60. * ((r - g) / delta + 4.)
    };
    (hue, if max == 0. { 0. } else { delta / max }, max)
}

fn from_hsv(hue: f32, saturation: f32, value: f32) -> u32 {
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

/// "Surprise Me!" (`mf`): 20% of the time two different colours of the list, otherwise one colour with a harmony of
/// it (analogous, complementary, split-complementary or triadic); an angle of the eight and an intensity of four.
/// `random` yields numbers in `0..1`.
pub(super) fn surprise(mut random: impl FnMut() -> f32) -> Custom {
    let mut pick = |len: usize| ((random() * len as f32) as usize).min(len - 1);
    let intensity = SURPRISE_INTENSITIES[pick(SURPRISE_INTENSITIES.len())];
    let angle = SURPRISE_ANGLES[pick(SURPRISE_ANGLES.len())];
    let two_colour = pick(5) == 0;
    let colors = if two_colour {
        let first = pick(SURPRISE_COLOURS.len());
        let mut second = pick(SURPRISE_COLOURS.len());
        while second == first {
            second = pick(SURPRISE_COLOURS.len());
        }
        vec![SURPRISE_COLOURS[first], SURPRISE_COLOURS[second]]
    } else {
        let base = SURPRISE_COLOURS[pick(SURPRISE_COLOURS.len())];
        let (h, s, l) = to_hsl(base);
        match pick(4) {
            0 => vec![from_hsl(h - 30., s, l), base, from_hsl(h + 30., s, l)],
            1 => {
                let opposite = from_hsl((h + 180.) % 360., s, l);
                vec![base, mix_rgb(base, opposite), opposite]
            }
            2 => vec![
                base,
                from_hsl((h + 150.) % 360., s, l),
                from_hsl((h + 210.) % 360., s, l),
            ],
            _ => vec![
                base,
                from_hsl((h + 120.) % 360., s, l),
                from_hsl((h + 240.) % 360., s, l),
            ],
        }
    };
    Custom {
        colors,
        angle,
        base_mix: intensity,
        light: false,
    }
}

/// The colour "Add Color" adds: the last colour with its hue 15 degrees on.
pub(super) fn next_colour(last: u32) -> u32 {
    let (h, s, l) = to_hsl(last);
    from_hsl(h + 15., s, l)
}

fn with_state<R>(run: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|state| state.borrow_mut().as_mut().map(run))
}

fn random_unit() -> f32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    thread_local! {
        static SEED: Cell<u64> = Cell::new(
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |time| time.as_nanos() as u64) | 1
        );
    }
    SEED.with(|seed| {
        let mut x = seed.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        seed.set(x);
        (x >> 40) as f32 / (1u64 << 24) as f32
    })
}

impl Root {
    /// Opens the editor from "Try it out": the whole app takes the theme, the panel goes on the right.
    pub(super) fn open_custom_theme_editor(&mut self, cx: &mut Context<Self>) {
        let custom = default_custom();
        let hex = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_max_utf16(7);
            editor.set_text(&hex_text(DEFAULT_COLOUR));
            editor
        });
        let intensity = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_max_utf16(4);
            editor.set_text(&format!("{}%", DEFAULT_MIX as u32));
            editor
        });
        let hex_events = cx.subscribe(&hex, |this, editor, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited)
                && let Some(rgb) = parse_hex(editor.read(cx).text())
            {
                this.custom_theme_set_colour(rgb, false, cx);
            }
        });
        let intensity_events = cx.subscribe(&intensity, |this, editor, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited) {
                let text = editor.read(cx).text();
                let digits: String = text.chars().filter(char::is_ascii_digit).collect();
                if let Ok(number) = digits.parse::<f32>() {
                    this.custom_theme_set_intensity(number.min(100.), false, cx);
                }
            }
        });
        let (hue, saturation, value) = to_hsv(DEFAULT_COLOUR);
        STATE.with(|state| {
            *state.borrow_mut() = Some(State {
                custom: custom.clone(),
                selected: 0,
                hue,
                saturation,
                value,
                hex,
                intensity,
                square: Rc::default(),
                hue_bar: Rc::default(),
                track: Rc::default(),
                _events: [hex_events, intensity_events],
            });
        });
        self.enter_custom_theme_editor(&custom, cx);
    }

    fn custom_theme_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(custom) = with_state(|state| state.custom.clone()) {
            client_theme::show_custom(&custom);
        }
        cx.refresh_windows();
        cx.notify();
    }

    /// A new colour for the selected slot, from the picker (`from_field` false: the hex field is rewritten) or the
    /// hex field (true: left alone so the caret stays).
    fn custom_theme_set_colour(&mut self, rgb: u32, from_picker: bool, cx: &mut Context<Self>) {
        let hex = with_state(|state| {
            if state.custom.colors.get(state.selected) == Some(&rgb) && !from_picker {
                return None;
            }
            if let Some(slot) = state.custom.colors.get_mut(state.selected) {
                *slot = rgb;
            }
            if !from_picker {
                (state.hue, state.saturation, state.value) = to_hsv(rgb);
            }
            Some(state.hex.clone())
        })
        .flatten();
        let Some(hex) = hex else {
            return;
        };
        if from_picker {
            hex.update(cx, |editor, cx| {
                editor.set_text(&hex_text(rgb));
                cx.notify();
            });
        }
        self.custom_theme_changed(cx);
    }

    fn custom_theme_pick(&mut self, hue: f32, saturation: f32, value: f32, cx: &mut Context<Self>) {
        with_state(|state| {
            state.hue = hue;
            state.saturation = saturation;
            state.value = value;
        });
        self.custom_theme_set_colour(from_hsv(hue, saturation, value), true, cx);
    }

    fn custom_theme_set_intensity(&mut self, mix: f32, write: bool, cx: &mut Context<Self>) {
        let editor = with_state(|state| {
            state.custom.base_mix = mix.clamp(0., 100.);
            state.intensity.clone()
        });
        if write && let Some(editor) = editor {
            editor.update(cx, |editor, cx| {
                editor.set_text(&format!("{}%", mix.round() as u32));
                cx.notify();
            });
        }
        self.custom_theme_changed(cx);
    }

    fn custom_theme_set_light(&mut self, light: bool, cx: &mut Context<Self>) {
        with_state(|state| state.custom.light = light);
        self.custom_theme_changed(cx);
    }

    fn custom_theme_add_colour(&mut self, cx: &mut Context<Self>) {
        let added = with_state(|state| {
            if state.custom.colors.len() >= MAX_COLOURS {
                return None;
            }
            let next = next_colour(*state.custom.colors.last().unwrap_or(&DEFAULT_COLOUR));
            state.custom.colors.push(next);
            state.selected = state.custom.colors.len() - 1;
            (state.hue, state.saturation, state.value) = to_hsv(next);
            Some((next, state.hex.clone()))
        })
        .flatten();
        if let Some((rgb, hex)) = added {
            hex.update(cx, |editor, cx| {
                editor.set_text(&hex_text(rgb));
                cx.notify();
            });
            self.custom_theme_changed(cx);
        }
    }

    fn custom_theme_surprise(&mut self, cx: &mut Context<Self>) {
        let drawn = surprise(random_unit);
        let editors = with_state(|state| {
            let light = state.custom.light;
            state.custom = Custom { light, ..drawn };
            state.selected = 0;
            let first = state.custom.colors[0];
            (state.hue, state.saturation, state.value) = to_hsv(first);
            (
                first,
                state.custom.base_mix,
                state.hex.clone(),
                state.intensity.clone(),
            )
        });
        if let Some((first, mix, hex, intensity)) = editors {
            hex.update(cx, |editor, cx| {
                editor.set_text(&hex_text(first));
                cx.notify();
            });
            intensity.update(cx, |editor, cx| {
                editor.set_text(&format!("{}%", mix as u32));
                cx.notify();
            });
        }
        self.custom_theme_changed(cx);
    }

    fn custom_theme_reset(&mut self, cx: &mut Context<Self>) {
        let editors = with_state(|state| {
            state.custom = default_custom();
            state.selected = 0;
            (state.hue, state.saturation, state.value) = to_hsv(DEFAULT_COLOUR);
            (state.hex.clone(), state.intensity.clone())
        });
        if let Some((hex, intensity)) = editors {
            hex.update(cx, |editor, cx| {
                editor.set_text(&hex_text(DEFAULT_COLOUR));
                cx.notify();
            });
            intensity.update(cx, |editor, cx| {
                editor.set_text(&format!("{}%", DEFAULT_MIX as u32));
                cx.notify();
            });
        }
        self.custom_theme_changed(cx);
    }

    /// The editor panel, or `None` while it is closed.
    pub(in crate::ui) fn custom_theme_panel(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !client_theme::editing() {
            return None;
        }
        let (custom, selected, hue, saturation, value, hex, intensity, square, hue_bar, track) =
            with_state(|state| {
                (
                    state.custom.clone(),
                    state.selected,
                    state.hue,
                    state.saturation,
                    state.value,
                    state.hex.clone(),
                    state.intensity.clone(),
                    state.square.clone(),
                    state.hue_bar.clone(),
                    state.track.clone(),
                )
            })?;
        let rgb = custom
            .colors
            .get(selected)
            .copied()
            .unwrap_or(DEFAULT_COLOUR);
        let text = |size: f32, weight: f32, tint: &'static str| {
            div()
                .text_size(px(size))
                .line_height(px(size + 4.))
                .font_weight(FontWeight(weight))
                .text_color(color(tint))
        };
        let label = |name: &'static str| text(14., 600., "--text-default").h(px(18.)).child(name);
        let section = || div().flex().flex_col().gap(px(12.));
        let pointer = |left: f32, top: f32, fill: Option<u32>| {
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .size(px(18.))
                .rounded_full()
                .border(px(3.))
                .border_color(rgba(0xffffffff))
                .when_some(fill, |pointer, fill| pointer.bg(rgba((fill << 8) | 0xff)))
        };
        let pill = |id: &'static str, icon: &'static str, light: bool, on: bool| {
            div()
                .id(id)
                .flex_1()
                .h(px(32.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(on, |pill| pill.bg(tinted("--background-base-low")))
                .child(mask_icon(
                    icon,
                    16.,
                    if on {
                        "--text-strong"
                    } else {
                        "--text-default"
                    },
                ))
                .on_click(cx.listener(move |this, _, _, cx| this.custom_theme_set_light(light, cx)))
        };
        let button = |id: &'static str, icon: &'static str, label_text: &'static str, dim: bool| {
            div()
                .id(id)
                .h(px(40.))
                .rounded(px(8.))
                .bg(color("--control-secondary-background-default"))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .when(dim, |button| button.opacity(0.5))
                .when(!dim, |button| button.cursor_pointer())
                .when(!icon.is_empty(), |button| {
                    button.child(mask_icon(icon, 20., "--control-secondary-text-default"))
                })
                .child(text(16., 500., "--control-secondary-text-default").child(label_text))
        };

        // Saturation square (`saturation_qOZw1R`, 264 by 152) and hue bar (`hue_qOZw1R`, 264 by 20, 2px inset).
        let pick_square = {
            let area = square.clone();
            move |position: gpui::Point<Pixels>, this: &mut Root, cx: &mut Context<Root>| {
                let rect = area.get();
                if rect.size.width <= px(0.) || rect.size.height <= px(0.) {
                    return;
                }
                let s = ((position.x - rect.origin.x) / rect.size.width).clamp(0., 1.);
                let v = 1. - ((position.y - rect.origin.y) / rect.size.height).clamp(0., 1.);
                let hue = with_state(|state| state.hue).unwrap_or(0.);
                this.custom_theme_pick(hue, s, v, cx);
            }
        };
        let pick_square_move = pick_square.clone();
        let pick_hue = {
            let area = hue_bar.clone();
            move |position: gpui::Point<Pixels>, this: &mut Root, cx: &mut Context<Root>| {
                let rect = area.get();
                if rect.size.width <= px(0.) {
                    return;
                }
                let ratio = ((position.x - rect.origin.x) / rect.size.width).clamp(0., 1.);
                let (s, v) =
                    with_state(|state| (state.saturation, state.value)).unwrap_or((1., 1.));
                this.custom_theme_pick(ratio * 360., s, v, cx);
            }
        };
        let pick_hue_move = pick_hue.clone();
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
        let saturation_box = div()
            .id("custom-theme-square")
            .relative()
            .flex_none()
            .w(px(264.))
            .h(px(152.))
            .overflow_hidden()
            .cursor_crosshair()
            .bg(rgba((from_hsv(hue, 1., 1.) << 8) | 0xff))
            .child(
                canvas(move |bounds, _, _| square.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(div().absolute().size_full().bg(linear_gradient(
                90.,
                linear_color_stop(rgba(0xffffffff), 0.),
                linear_color_stop(rgba(0xffffff00), 1.),
            )))
            .child(div().absolute().size_full().bg(linear_gradient(
                180.,
                linear_color_stop(rgba(0x00000000), 0.),
                linear_color_stop(rgba(0x000000ff), 1.),
            )))
            .child(pointer(
                saturation * 264. - 9.,
                (1. - value) * 152. - 9.,
                None,
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    pick_square(event.position, this, cx)
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    pick_square_move(event.position, this, cx);
                }
            }));
        let hue_slider = div()
            .id("custom-theme-hue")
            .relative()
            .flex_none()
            .w(px(264.))
            .h(px(20.))
            .px(px(2.))
            .cursor_crosshair()
            .child(
                div()
                    .relative()
                    .size_full()
                    .rounded(px(10.))
                    .overflow_hidden()
                    .flex()
                    .child(
                        canvas(move |bounds, _, _| hue_bar.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .children(segments),
            )
            .child(pointer(
                2. + hue / 360. * 260. - 9.,
                1.,
                Some(from_hsv(hue, 1., 1.)),
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    pick_hue(event.position, this, cx)
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    pick_hue_move(event.position, this, cx);
                }
            }));

        // Intensity slider: bar 4 high, fill and a 16 white grabber over a 254 wide track.
        let ratio = custom.base_mix / 100.;
        let set_track = {
            let area = track.clone();
            move |position: gpui::Point<Pixels>, this: &mut Root, cx: &mut Context<Root>| {
                let rect = area.get();
                if rect.size.width <= px(0.) {
                    return;
                }
                let ratio = ((position.x - rect.origin.x) / rect.size.width).clamp(0., 1.);
                this.custom_theme_set_intensity((ratio * 100.).round(), true, cx);
            }
        };
        let set_track_move = set_track.clone();
        let slider = div()
            .id("custom-theme-intensity")
            .relative()
            .flex_none()
            .w(px(264.))
            .h(px(24.))
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .top(px(10.))
                    .left_0()
                    .w(px(264.))
                    .h(px(4.))
                    .rounded(px(2.))
                    .bg(tinted("--background-base-low")),
            )
            .child(
                div()
                    .absolute()
                    .top(px(9.))
                    .left_0()
                    .w(px(5. + ratio * 254.))
                    .h(px(6.))
                    .rounded(px(3.))
                    .bg(color("--background-brand")),
            )
            .child(
                div()
                    .absolute()
                    .left(px(5.))
                    .top_0()
                    .w(px(254.))
                    .h_full()
                    .child(
                        canvas(move |bounds, _, _| track.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(ratio * 254. - 8.))
                            .top(px(4.))
                            .size(px(16.))
                            .rounded_full()
                            .border_1()
                            .border_color(color("--border-strong"))
                            .bg(rgba(0xffffffff)),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    set_track(event.position, this, cx)
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    set_track_move(event.position, this, cx);
                }
            }));

        let at_default = custom == default_custom();
        let on_unlock = "--black";
        Some(
            div()
                .id("custom-theme-panel")
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .w(px(PANEL_WIDTH))
                .border_1()
                .border_color(color("--border-faint"))
                .bg(tinted("--background-base-lower"))
                .occlude()
                .child(
                    div()
                        .id("custom-theme-scroll")
                        .size_full()
                        .overflow_y_scroll()
                        .pl(px(12.))
                        .pr(px(12.))
                        .pt(px(16.))
                        .pb(px(140.))
                        .flex()
                        .flex_col()
                        .gap(px(32.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .h(px(32.))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            text(20., 600., "--text-strong")
                                                .child("New! Custom Themes"),
                                        )
                                        .child(
                                            div()
                                                .id("custom-theme-close")
                                                .size(px(32.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.exit_theme_preview(cx)
                                                }))
                                                .child(mask_icon(
                                                    "theme_editor_close",
                                                    16.,
                                                    "--interactive-text-default",
                                                )),
                                        ),
                                )
                                .child(text(14., 500., "--text-default").child(
                                    "Match your mood with endless color combinations. Try it now!",
                                )),
                        )
                        .child(
                            section().child(label("Appearance")).child(
                                div()
                                    .h(px(42.))
                                    .p(px(4.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .gap(px(4.))
                                    .child(pill(
                                        "custom-theme-dark",
                                        "theme_editor_moon",
                                        false,
                                        !custom.light,
                                    ))
                                    .child(pill(
                                        "custom-theme-light",
                                        "theme_editor_sun",
                                        true,
                                        custom.light,
                                    )),
                            ),
                        )
                        .child(
                            section().child(label("Colors")).child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(saturation_box)
                                    .child(hue_slider)
                                    .child(
                                        div()
                                            .h(px(42.))
                                            .pl(px(8.))
                                            .pr(px(4.))
                                            .rounded(px(8.))
                                            .bg(color("--control-secondary-background-default"))
                                            .flex()
                                            .items_center()
                                            .child(
                                                div()
                                                    .size(px(26.))
                                                    .flex_none()
                                                    .mr(px(8.))
                                                    .rounded(px(4.))
                                                    .border_1()
                                                    .border_color(color("--border-strong"))
                                                    .bg(rgba((rgb << 8) | 0xff)),
                                            )
                                            .child(div().flex_1().child(hex))
                                            .child(
                                                div()
                                                    .size(px(32.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(mask_icon(
                                                        "theme_editor_eyedropper",
                                                        16.,
                                                        "--interactive-text-default",
                                                    )),
                                            ),
                                    )
                                    .child(
                                        button(
                                            "custom-theme-add",
                                            "theme_editor_plus",
                                            "Add Color",
                                            false,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.custom_theme_add_colour(cx)
                                            }),
                                        ),
                                    ),
                            ),
                        )
                        .child(
                            section().child(label("Controls")).child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .h(px(22.))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                text(16., 500., "--text-strong")
                                                    .child("Color Intensity"),
                                            )
                                            .child(div().w(px(54.)).child(intensity)),
                                    )
                                    .child(slider),
                            ),
                        )
                        .child(
                            div()
                                .mt(px(19. - 32. + 32.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(
                                    button(
                                        "custom-theme-surprise",
                                        "theme_editor_dice",
                                        "Surprise Me!",
                                        false,
                                    )
                                    .on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.custom_theme_surprise(cx)
                                        }),
                                    ),
                                )
                                .child(button("custom-theme-reset", "", "Reset", at_default).when(
                                    !at_default,
                                    |button| {
                                        button.on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.custom_theme_reset(cx)
                                            }),
                                        )
                                    },
                                )),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .bottom_0()
                        .w_full()
                        .h(px(125.))
                        .px(px(20.))
                        .py(px(16.))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .bg(tinted("--background-base-lower"))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .size(px(40.))
                                        .flex_none()
                                        .rounded(px(8.))
                                        .bg(rgba(0x0000008f))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(mask_icon("golive_heart", 20., "--icon-default")),
                                )
                                .child(
                                    div()
                                        .id("custom-theme-nitro")
                                        .flex_1()
                                        .h(px(40.))
                                        .rounded(px(8.))
                                        .bg(color("--white"))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(4.))
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                this.exit_theme_preview(cx);
                                                this.open_purchase(false, cx);
                                            }),
                                        )
                                        .child(mask_icon("nitro_wheel", 20., on_unlock))
                                        .child(text(16., 500., on_unlock).child("Get Nitro")),
                                ),
                        )
                        .child(
                            div()
                                .id("custom-theme-back")
                                .h(px(40.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.exit_theme_preview(cx);
                                    this.open_settings_page("appearance", cx);
                                }))
                                .child(
                                    text(16., 500., "--control-secondary-text-default")
                                        .child("Back to Settings"),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Drops the editor's state: the preview is over.
    pub(super) fn custom_theme_closed(&mut self) {
        STATE.with(|state| *state.borrow_mut() = None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_field_takes_six_digits_with_or_without_the_hash() {
        assert_eq!(parse_hex("#5865F2"), Some(0x5865f2));
        assert_eq!(parse_hex("5865f2"), Some(0x5865f2));
        assert_eq!(parse_hex("#5865F"), None);
        assert_eq!(parse_hex("#58GGF2"), None);
    }

    #[test]
    fn add_color_moves_the_hue_fifteen_degrees() {
        let (h, _, _) = to_hsl(0x5865f2);
        let (next, _, _) = to_hsl(next_colour(0x5865f2));
        assert!((next - (h + 15.)).abs() < 2., "{h} -> {next}");
    }

    #[test]
    fn surprise_gives_known_angle_intensity_and_two_or_three_colours() {
        let mut seed = 7u32;
        let mut random = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        for _ in 0..200 {
            let custom = surprise(&mut random);
            assert!(SURPRISE_ANGLES.contains(&custom.angle));
            assert!(SURPRISE_INTENSITIES.contains(&custom.base_mix));
            assert!(
                (2..=3).contains(&custom.colors.len()),
                "{:?}",
                custom.colors
            );
        }
    }

    #[test]
    fn hsv_round_trips() {
        for rgb in [0x5865f2, 0x000000, 0xffffff, 0x12ab90] {
            let (h, s, v) = to_hsv(rgb);
            assert_eq!(from_hsv(h, s, v), rgb);
        }
    }
}
