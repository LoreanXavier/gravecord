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
    MouseMoveEvent, Pixels, ScrollHandle, Subscription, canvas, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
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
    angle: Entity<Editor>,
    angle_track: Rc<Cell<Bounds<Pixels>>>,
    square: Rc<Cell<Bounds<Pixels>>>,
    hue_bar: Rc<Cell<Bounds<Pixels>>>,
    track: Rc<Cell<Bounds<Pixels>>>,
    scroll: ScrollHandle,
    _events: [Subscription; 3],
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

/// `chroma.valid("#" + text)` for what the field can hold (7 characters): three, four or six hex digits; a fourth
/// digit is an alpha the editor drops.
fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.trim().trim_start_matches('#');
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let expand = |digit: u8| {
        let value = (digit as char).to_digit(16).unwrap_or(0);
        value * 16 + value
    };
    match digits.len() {
        6 => u32::from_str_radix(digits, 16).ok(),
        3 | 4 => {
            let bytes = digits.as_bytes();
            Some((expand(bytes[0]) << 16) | (expand(bytes[1]) << 8) | expand(bytes[2]))
        }
        _ => None,
    }
}

/// What the hex field shows (`g`): the text trimmed, a `#` in front, in capitals (`value.toUpperCase()`).
fn hex_field_text(text: &str) -> String {
    let trimmed = text.trim();
    let hashed = if trimmed.starts_with('#') || trimmed.is_empty() {
        trimmed.to_owned()
    } else {
        format!("#{trimmed}")
    };
    hashed.to_uppercase()
}

/// A number field of the panel (`controlLabelInput`: `Math.round(value)` plus its suffix): the text without the suffix
/// read like `parseInt` and clamped. Text that is not a number keeps the field's last value, and an empty field shows
/// nothing without changing the value.
enum Typed {
    Empty,
    Number(f32),
    Rejected,
}

fn typed_number(text: &str, suffix: char, max: f32) -> Typed {
    let text: String = text.chars().filter(|c| *c != suffix).collect();
    let text = text.trim();
    if text.is_empty() {
        return Typed::Empty;
    }
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1., rest),
        None => (1., text.strip_prefix('+').unwrap_or(text)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    match digits.parse::<f32>() {
        Ok(number) => Typed::Number((sign * number).clamp(0., max)),
        Err(_) => Typed::Rejected,
    }
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

/// The direction slider snaps to 90, 180 and 270 within 10 degrees of them (module 500779 `eG`).
pub(super) fn snap_angle(angle: f32) -> f32 {
    [90., 180., 270.]
        .into_iter()
        .find(|stop| (angle - stop).abs() <= 10.)
        .unwrap_or(angle)
}

/// The colour "Add Color" adds (module 500779): the last colour with its hue 15 degrees on, by chroma.js's `hsl`
/// conversions (`set("hsl.h", (h + 15) % 360).hex()`), in double precision with halves rounded up.
pub(super) fn next_colour(last: u32) -> u32 {
    let unit = |shift: u32| f64::from((last >> shift) & 0xff) / 255.;
    let (r, g, b) = (unit(16), unit(8), unit(0));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let delta = max - min;
    let l = (max + min) / 2.;
    let s = if delta == 0. {
        0.
    } else if l < 0.5 {
        delta / (max + min)
    } else {
        delta / (2. - max - min)
    };
    let mut h = if delta == 0. {
        0.
    } else if max == r {
        (g - b) / delta
    } else if max == g {
        2. + (b - r) / delta
    } else {
        4. + (r - g) / delta
    } * 60.;
    if h < 0. {
        h += 360.;
    }
    let h = (h + 15.) % 360.;
    let channel = |value: f64| (value * 255. + 0.5).floor().clamp(0., 255.) as u32;
    if s == 0. {
        let grey = channel(l);
        return (grey << 16) | (grey << 8) | grey;
    }
    let t2 = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
    let t1 = 2. * l - t2;
    let along = |offset: f64| {
        let mut t = h / 360. + offset;
        if t < 0. {
            t += 1.;
        }
        if t > 1. {
            t -= 1.;
        }
        let value = if 6. * t < 1. {
            t1 + (t2 - t1) * 6. * t
        } else if 2. * t < 1. {
            t2
        } else if 3. * t < 2. {
            t1 + (t2 - t1) * (2. / 3. - t) * 6.
        } else {
            t1
        };
        channel(value)
    };
    (along(1. / 3.) << 16) | (along(0.) << 8) | along(-1. / 3.)
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

/// One black box shadow of the panel's controls (`0 <dy> <blur> 0 rgba(0 0 0 / <alpha>)`, optionally inset).
fn shadow(alpha: f32, dy: f32, blur: f32, inset: bool) -> gpui::BoxShadow {
    gpui::BoxShadow {
        color: gpui::hsla(0., 0., 0., alpha),
        offset: gpui::point(px(0.), px(dy)),
        blur_radius: px(blur),
        spread_radius: px(0.),
        inset,
    }
}

/// A slider of the panel (`slider_p3RiOL`): a 4 px bar over 264, the fill and a 16 px white grabber on a 254 px track
/// inset 5; `dots` adds the direction slider's five step dots (the middle one larger).
fn custom_slider(
    id: &'static str,
    ratio: f32,
    track: Rc<Cell<Bounds<Pixels>>>,
    dots: bool,
    cx: &mut Context<Root>,
    set: impl Fn(f32, &mut Root, &mut Context<Root>) + Clone + 'static,
) -> gpui::Stateful<gpui::Div> {
    let ratio = ratio.clamp(0., 1.);
    let on_ratio = {
        let area = track.clone();
        move |position: gpui::Point<Pixels>, this: &mut Root, cx: &mut Context<Root>| {
            let rect = area.get();
            if rect.size.width <= px(0.) {
                return;
            }
            let ratio = ((position.x - rect.origin.x) / rect.size.width).clamp(0., 1.);
            set(ratio, this, cx);
        }
    };
    let on_move = on_ratio.clone();
    div()
        .id(id)
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
                .bg(color("--slider-track-background")),
        )
        .when(dots, |slider| {
            // `angleIndicatorOverlay`: 263 wide, .5 inset, the five dots spread evenly (measured left edges).
            const LEFTS: [f32; 5] = [0.5, 63.75, 127., 192.25, 255.5];
            LEFTS
                .iter()
                .enumerate()
                .fold(slider, |slider, (index, left)| {
                    let size = if index == 2 { 10. } else { 8. };
                    slider.child(
                        div()
                            .absolute()
                            .left(px(*left))
                            .top(px(11.5 - size / 2.))
                            .size(px(size))
                            .rounded_full()
                            .bg(color("--background-mod-muted")),
                    )
                })
        })
        .when(ratio > 0., |slider| {
            // `barFill`: 100% of the 4 px bar plus its 1px `hsla(0 0% 100% .1)` border, clipped by the bar; it is
            // `ratio * 264 + 2` wide (measured: 197.36 at 74%).
            slider.child(
                div()
                    .absolute()
                    .top(px(10.))
                    .left_0()
                    .w(px((ratio * 264. + 2.).min(264.)))
                    .h(px(4.))
                    .rounded_l(px(2.))
                    .bg(color("--background-brand"))
                    .border_t_1()
                    .border_color(rgba(0xffffff1a)),
            )
        })
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
                        .border_color(color("--border-normal"))
                        .shadow(vec![shadow(0.14, 1., 4., false)])
                        .bg(rgba(0xffffffff)),
                ),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                on_ratio(event.position, this, cx)
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                on_move(event.position, this, cx);
            }
        }))
}

impl Root {
    /// Opens the editor from "Try it out": the whole app takes the theme, the panel goes on the right.
    pub(super) fn open_custom_theme_editor(&mut self, cx: &mut Context<Self>) {
        let custom = default_custom();
        let field = |cx: &mut Context<Self>, limit: usize, end: bool, text: &str| {
            let text = text.to_owned();
            cx.new(|cx| {
                let mut editor = Editor::new(editor_style(), cx);
                editor.set_single_line(true);
                editor.set_max_utf16(limit);
                // Deleting edits the value too, and Enter leaves the text where it is.
                editor.set_reports_all_edits(true);
                editor.set_keeps_text(true);
                editor.set_align_end(end);
                editor.set_text(&text);
                editor
            })
        };
        let hex = field(cx, 7, false, &hex_text(DEFAULT_COLOUR));
        let intensity = field(cx, 4, true, &format!("{}%", DEFAULT_MIX as u32));
        let angle = field(cx, 4, true, "0\u{b0}");
        let angle_events = cx.subscribe(&angle, |this, editor, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited | EditorEvent::Submit(_)) {
                this.custom_theme_typed_number(&editor, '\u{b0}', 360., true, cx);
            }
        });
        let hex_events = cx.subscribe(&hex, |this, editor, event: &EditorEvent, cx| {
            let text = editor.read(cx).text().to_owned();
            let shown = hex_field_text(&text);
            if shown != text {
                editor.update(cx, |editor, cx| {
                    editor.set_text(&shown);
                    cx.notify();
                });
            }
            match event {
                EditorEvent::Edited => {
                    if let Some(rgb) = parse_hex(&shown) {
                        this.custom_theme_set_colour(rgb, false, cx);
                    }
                }
                // Enter on text that is not a colour puts the selected colour back.
                EditorEvent::Submit(_) if parse_hex(&shown).is_none() => {
                    this.custom_theme_write_fields(cx)
                }
                _ => {}
            }
        });
        let intensity_events = cx.subscribe(&intensity, |this, editor, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited | EditorEvent::Submit(_)) {
                this.custom_theme_typed_number(&editor, '%', 100., false, cx);
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
                angle,
                angle_track: Rc::default(),
                square: Rc::default(),
                hue_bar: Rc::default(),
                track: Rc::default(),
                scroll: ScrollHandle::new(),
                _events: [hex_events, intensity_events, angle_events],
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
        let editors = with_state(|state| {
            state.custom.light = light;
            [
                state.hex.clone(),
                state.intensity.clone(),
                state.angle.clone(),
            ]
        });
        self.custom_theme_changed(cx);
        // The palette changed with the theme: the fields take its text colour.
        for editor in editors.into_iter().flatten() {
            editor.update(cx, |editor, cx| {
                editor.set_style(editor_style());
                cx.notify();
            });
        }
    }

    /// Writes the state into the three fields (the colour of the selected slot, the direction and the intensity).
    fn custom_theme_write_fields(&mut self, cx: &mut Context<Self>) {
        let fields = with_state(|state| {
            let rgb = state
                .custom
                .colors
                .get(state.selected)
                .copied()
                .unwrap_or(DEFAULT_COLOUR);
            (state.hue, state.saturation, state.value) = to_hsv(rgb);
            (
                hex_text(rgb),
                format!("{}%", state.custom.base_mix.round() as u32),
                format!("{}°", state.custom.angle.round() as u32),
                [
                    state.hex.clone(),
                    state.intensity.clone(),
                    state.angle.clone(),
                ],
            )
        });
        if let Some((hex, mix, angle, editors)) = fields {
            for (editor, text) in editors.into_iter().zip([hex, mix, angle]) {
                editor.update(cx, |editor, cx| {
                    editor.set_text(&text);
                    cx.notify();
                });
            }
        }
    }

    /// A number typed into the direction (`angle` true) or intensity field: clamped, written back with its suffix and
    /// applied at once; text that is not a number puts the last value back; a cleared field stays empty.
    fn custom_theme_typed_number(
        &mut self,
        editor: &Entity<Editor>,
        suffix: char,
        max: f32,
        angle: bool,
        cx: &mut Context<Self>,
    ) {
        let text = editor.read(cx).text().to_owned();
        let shown = match typed_number(&text, suffix, max) {
            Typed::Empty => {
                if !text.is_empty() {
                    editor.update(cx, |editor, cx| {
                        editor.set_text("");
                        cx.notify();
                    });
                }
                return;
            }
            Typed::Number(number) => {
                let number = number as u32;
                if angle {
                    self.custom_theme_set_angle(number as f32, false, false, cx);
                } else {
                    self.custom_theme_set_intensity(number as f32, false, cx);
                }
                format!("{number}{suffix}")
            }
            Typed::Rejected => {
                let value = with_state(|state| {
                    if angle {
                        state.custom.angle
                    } else {
                        state.custom.base_mix
                    }
                })
                .unwrap_or(0.);
                format!("{}{suffix}", value.round() as u32)
            }
        };
        if shown != text {
            editor.update(cx, |editor, cx| {
                editor.set_text(&shown);
                cx.notify();
            });
        }
    }

    fn custom_theme_set_angle(
        &mut self,
        angle: f32,
        write: bool,
        snap: bool,
        cx: &mut Context<Self>,
    ) {
        let editor = with_state(|state| {
            let angle = angle.clamp(0., 360.);
            state.custom.angle = if snap { snap_angle(angle) } else { angle };
            state.angle.clone()
        });
        if write && let Some(editor) = editor {
            let shown = with_state(|state| state.custom.angle).unwrap_or(0.);
            editor.update(cx, |editor, cx| {
                editor.set_text(&format!("{}°", shown.round() as u32));
                cx.notify();
            });
        }
        self.custom_theme_changed(cx);
    }

    fn custom_theme_select(&mut self, index: usize, cx: &mut Context<Self>) {
        with_state(|state| {
            if index < state.custom.colors.len() {
                state.selected = index;
            }
        });
        self.custom_theme_write_fields(cx);
        cx.notify();
    }

    /// The "remove colour" button next to the eyedropper: drops the selected colour (one always stays).
    fn custom_theme_remove_colour(&mut self, cx: &mut Context<Self>) {
        with_state(|state| {
            if state.custom.colors.len() > 1 {
                state.custom.colors.remove(state.selected);
                state.selected = state.selected.min(state.custom.colors.len() - 1);
            }
        });
        self.custom_theme_write_fields(cx);
        self.custom_theme_changed(cx);
    }

    fn custom_theme_add_colour(&mut self, cx: &mut Context<Self>) {
        let added = with_state(|state| {
            if state.custom.colors.len() >= MAX_COLOURS {
                return false;
            }
            let next = next_colour(*state.custom.colors.last().unwrap_or(&DEFAULT_COLOUR));
            state.custom.colors.push(next);
            state.selected = state.custom.colors.len() - 1;
            true
        });
        if added == Some(true) {
            self.custom_theme_write_fields(cx);
            self.custom_theme_changed(cx);
        }
    }

    fn custom_theme_surprise(&mut self, cx: &mut Context<Self>) {
        let drawn = surprise(random_unit);
        with_state(|state| {
            let light = state.custom.light;
            state.custom = Custom { light, ..drawn };
            state.selected = 0;
        });
        self.custom_theme_write_fields(cx);
        self.custom_theme_changed(cx);
    }

    fn custom_theme_reset(&mut self, cx: &mut Context<Self>) {
        with_state(|state| {
            state.custom = default_custom();
            state.selected = 0;
        });
        self.custom_theme_write_fields(cx);
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
        let (custom, selected, hue, saturation, value, fields, bounds, scroll) =
            with_state(|state| {
                (
                    state.custom.clone(),
                    state.selected,
                    state.hue,
                    state.saturation,
                    state.value,
                    (
                        state.hex.clone(),
                        state.intensity.clone(),
                        state.angle.clone(),
                    ),
                    (
                        state.square.clone(),
                        state.hue_bar.clone(),
                        state.track.clone(),
                        state.angle_track.clone(),
                    ),
                    state.scroll.clone(),
                )
            })?;
        let (hex, intensity, angle) = fields;
        let (square, hue_bar, track, angle_track) = bounds;
        let slider = custom_slider(
            "custom-theme-intensity",
            custom.base_mix / 100.,
            track,
            false,
            cx,
            |ratio, this, cx| this.custom_theme_set_intensity((ratio * 100.).round(), true, cx),
        );
        let angle_slider = custom_slider(
            "custom-theme-angle",
            custom.angle / 360.,
            angle_track,
            true,
            cx,
            |ratio, this, cx| this.custom_theme_set_angle((ratio * 360.).round(), true, true, cx),
        );
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
        let label = |name: &'static str| text(14., 600., "--text-subtle").h(px(18.)).child(name);
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
                .shadow(vec![
                    shadow(0.15, 3., 13.2, false),
                    shadow(0.05, 1., 2., false),
                ])
                .when_some(fill, |pointer, fill| pointer.bg(rgba((fill << 8) | 0xff)))
        };
        // `pillItem` (module of the tab list pill): 32 high, the selected one 34 with a 1px `--border-subtle` border on
        // `--background-mod-subtle` and the strong text, the other the `--text-subtle` colour.
        let pill = |id: &'static str, icon: &'static str, light: bool, on: bool| {
            div()
                .id(id)
                .flex_1()
                .h(px(if on { 34. } else { 32. }))
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(on, |pill| {
                    pill.border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-mod-muted"))
                })
                .child(mask_icon(
                    icon,
                    16.,
                    if on { "--text-strong" } else { "--text-subtle" },
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
        // `hue-horizontal`: red, yellow 17%, lime 33%, cyan 50%, blue 67%, magenta 83%, red, 8 px corners.
        let stops: [(u32, f32); 7] = [
            (0xff0000, 0.),
            (0xffff00, 17.),
            (0x00ff00, 33.),
            (0x00ffff, 50.),
            (0x0000ff, 67.),
            (0xff00ff, 83.),
            (0xff0000, 100.),
        ];
        let last_segment = stops.len() - 2;
        let segments = stops.windows(2).enumerate().map(|(index, pair)| {
            div()
                .flex_none()
                .w(px(264. * (pair[1].1 - pair[0].1) / 100.))
                .h_full()
                .when(index == 0, |segment| segment.rounded_l(px(8.)))
                .when(index == last_segment, |segment| segment.rounded_r(px(8.)))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(rgba((pair[0].0 << 8) | 0xff), 0.),
                    linear_color_stop(rgba((pair[1].0 << 8) | 0xff), 1.),
                ))
        });
        let saturation_box = div()
            .id("custom-theme-square")
            .relative()
            .flex_none()
            .w(px(264.))
            .h(px(152.))
            .rounded(px(8.))
            .cursor_crosshair()
            .bg(rgba((from_hsv(hue, 1., 1.) << 8) | 0xff))
            .child(
                canvas(move |bounds, _, _| square.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(8.))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(rgba(0xffffffff), 0.),
                        linear_color_stop(rgba(0xffffff00), 1.),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(8.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(rgba(0x00000000), 0.),
                        linear_color_stop(rgba(0x000000ff), 1.),
                    )),
            )
            // The pointer sits 1 px right and below the exact spot (measured: 1172, 315.7 for s .636, v .949).
            .child(pointer(
                saturation * 264. - 8.,
                (1. - value) * 152. - 8.,
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
            .cursor_crosshair()
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .child(
                        canvas(move |bounds, _, _| hue_bar.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .children(segments),
            )
            // `huePointer`: transparent inside, centred 1 px right of `hue / 360` of the 264 px bar.
            .child(pointer(1. + hue / 360. * 264. - 9., 1., None))
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

        // Sliders: bar 4 high, fill and a 16 white grabber over a 254 wide track; the direction one has five dots.
        // `controlLabelInput`: 54 wide (40 plus 12 padding and the 1px border), 22 high, text at the end.
        let number_field = |editor: Entity<Editor>| {
            div()
                .w(px(54.))
                .h(px(22.))
                .flex_none()
                .pl(px(12.))
                .border_1()
                .border_color(rgba(0))
                .rounded(px(4.))
                .flex()
                .items_center()
                .child(div().w(px(40.)).child(editor))
        };
        let several = custom.colors.len() > 1;
        // The colour strip above the square (`gradientBar_YWrQKs`): the colours as the gradient from 10% to 90%, a
        // square on each at its stop, the selected one larger with a notch under it.
        let strip = several.then(|| {
            let count = custom.colors.len();
            let flat = |colour: u32| rgba((colour << 8) | 0xff);
            let stop = |index: usize| 10. + 80. * index as f32 / (count - 1) as f32;
            let squares: Vec<AnyElement> = custom
                .colors
                .iter()
                .enumerate()
                .map(|(index, colour)| {
                    let chosen = index == selected;
                    let size = if chosen { 28. } else { 24. };
                    let centre = 264. * stop(index) / 100.;
                    div()
                        .id(gpui::SharedString::from(format!(
                            "custom-theme-colour-{index}"
                        )))
                        .absolute()
                        .left(px(centre - size / 2.))
                        .top(px(if chosen { 8. } else { 10. }))
                        .size(px(size))
                        .rounded(px(4.))
                        .bg(flat(*colour))
                        .border(px(if chosen { 4. } else { 2. }))
                        .border_color(rgba(0xffffffff))
                        .shadow(vec![
                            shadow(0.15, 3., 13.2, false),
                            shadow(0.05, 1., 2., false),
                            shadow(0.15, 1., 6., true),
                        ])
                        .cursor_pointer()
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.custom_theme_select(index, cx)),
                        )
                        .into_any_element()
                })
                .collect();
            let mut bar = div()
                .relative()
                .flex_none()
                .w(px(264.))
                .h(px(44.))
                .rounded(px(8.))
                .flex()
                .child(
                    div()
                        .h_full()
                        .w(px(264. * stop(0) / 100.))
                        .rounded_l(px(8.))
                        .bg(flat(custom.colors[0])),
                );
            for index in 0..count - 1 {
                bar = bar.child(
                    div()
                        .h_full()
                        .w(px(264. * (stop(index + 1) - stop(index)) / 100.))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(flat(custom.colors[index]), 0.),
                            linear_color_stop(flat(custom.colors[index + 1]), 1.),
                        )),
                );
            }
            bar = bar.child(
                div()
                    .h_full()
                    .flex_1()
                    .rounded_r(px(8.))
                    .bg(flat(custom.colors[count - 1])),
            );
            let notch_left = 264. * stop(selected) / 100. - 4.;
            bar.children(squares).child(
                div()
                    .absolute()
                    .left(px(notch_left))
                    .top(px(35.))
                    .w(px(8.))
                    .h(px(5.))
                    .bg(rgba(0xffffffff)),
            )
        });
        let remove = several.then(|| {
            div()
                .id("custom-theme-remove")
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.custom_theme_remove_colour(cx)))
                .child(mask_icon(
                    "theme_editor_remove",
                    16.,
                    "--interactive-text-default",
                ))
        });
        let direction = several.then(|| {
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
                            text(16., 500., "--text-default")
                                .line_height(px(22.))
                                .relative()
                                .top(px(-1.))
                                .child("Gradient Direction"),
                        )
                        .child(number_field(angle)),
                )
                .child(angle_slider)
        });

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
                .border_color(color("--border-muted"))
                .bg(tinted("--background-base-low"))
                .occlude()
                .child(
                    div()
                        .id("custom-theme-scroll")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&scroll)
                        .pl(px(12.))
                        .pr(px(12.))
                        .pt(px(16.))
                        .pb(px(125.))
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
                                .child(text(14., 500., "--text-muted").child(
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
                                    .bg(tinted("--background-base-lowest"))
                                    .flex()
                                    .items_start()
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
                                    .children(strip)
                                    .child(saturation_box)
                                    .child(hue_slider)
                                    .child(
                                        div()
                                            .h(px(42.))
                                            .pl(px(8.))
                                            .pr(px(4.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-muted"))
                                            .bg(color("--background-mod-subtle"))
                                            .flex()
                                            .items_center()
                                            .child(
                                                div()
                                                    .size(px(26.))
                                                    .flex_none()
                                                    .mr(px(8.))
                                                    .rounded(px(4.))
                                                    .border_1()
                                                    .border_color(color("--border-subtle"))
                                                    .bg(rgba((rgb << 8) | 0xff)),
                                            )
                                            .child(div().flex_1().pt(px(1.)).child(hex))
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
                                            )
                                            .children(remove),
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
                                    .gap(px(16.))
                                    .children(direction)
                                    .child(
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
                                                        text(16., 500., "--text-default")
                                                            .line_height(px(22.))
                                                            .relative()
                                                            .top(px(-1.))
                                                            .child("Color Intensity"),
                                                    )
                                                    .child(number_field(intensity)),
                                            )
                                            .child(slider),
                                    ),
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
                // The thin scrollbar of `scrollbarGutterStable thin` over the 593 px above the footer.
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h(px(593.))
                        .child(crate::ui::menu::menu_scrollbar(scroll)),
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
                        .bg(tinted("--background-base-low"))
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

    /// An unknown token panics inside `color()` while a frame is drawn, so every `"--name"` literal of the app has to
    /// be in all four palettes. Strings that are not tokens (CSS variables of other stylesheets) are listed here.
    #[test]
    fn every_token_literal_of_the_app_resolves() {
        const NOT_TOKENS: &[&str] = &[];
        fn walk(dir: &std::path::Path, found: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, found);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    // Only the code before the file's test module: tests may name unknown tokens on purpose.
                    let code = text.split("#[cfg(test)]").next().unwrap_or("");
                    for line in code.lines() {
                        let line = line.trim_start();
                        if line.starts_with("//") {
                            continue;
                        }
                        let mut rest = line;
                        while let Some(at) = rest.find("\"--") {
                            let tail = &rest[at + 1..];
                            let end = tail
                                .find(|c: char| {
                                    !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                                })
                                .unwrap_or(tail.len());
                            if tail[end..].starts_with('"') {
                                found.push((tail[..end].to_owned(), path.display().to_string()));
                            }
                            rest = &rest[at + 1 + end..];
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        assert!(found.len() > 500, "the scan found {} literals", found.len());
        // Command line flags ("--capture") are literals of the same shape: only names that start like a token family
        // of the palette are checked.
        let families: std::collections::HashSet<String> = [
            "background",
            "text",
            "border",
            "icon",
            "interactive",
            "control",
            "input",
            "channel",
            "chat",
            "button",
            "status",
            "brand",
            "red",
            "green",
            "yellow",
            "orange",
            "primary",
            "white",
            "black",
            "bg",
            "header",
            "mention",
            "scrollbar",
            "link",
            "focus",
            "card",
            "modal",
            "guild",
            "redesign",
            "shadow",
            "spoiler",
            "opacity",
            "radius",
            "space",
            "font",
            "elevation",
            "custom",
            "message",
            "voice",
            "interactive",
            "menu",
            "popout",
            "tooltip",
            "badge",
            "banner",
            "profile",
            "polls",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let mut unknown = Vec::new();
        for (name, file) in &found {
            let family = name.trim_start_matches('-').split('-').next().unwrap_or("");
            if NOT_TOKENS.contains(&name.as_str()) || !families.contains(family) {
                continue;
            }
            for theme in [
                dc_tokens::Theme::Dark,
                dc_tokens::Theme::Light,
                dc_tokens::Theme::Darker,
                dc_tokens::Theme::Midnight,
            ] {
                if !theme.has(name) {
                    unknown.push(format!("{name} ({theme:?}) in {file}"));
                }
            }
        }
        unknown.sort();
        unknown.dedup();
        assert!(
            unknown.is_empty(),
            "unknown design tokens:
{}",
            unknown.join(
                "
"
            )
        );
    }

    #[test]
    fn hex_field_takes_six_digits_with_or_without_the_hash() {
        assert_eq!(parse_hex("#5865F2"), Some(0x5865f2));
        assert_eq!(parse_hex("5865f2"), Some(0x5865f2));
        assert_eq!(parse_hex("#5865F"), None);
        assert_eq!(parse_hex("#58GGF2"), None);
    }

    #[test]
    fn add_color_matches_the_official_pair() {
        // `custom-theme-editor-two-colors-darker-cozy@1.png`: Add Color on the default colour.
        assert_eq!(next_colour(0x5865f2), 0x7258f2);
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
