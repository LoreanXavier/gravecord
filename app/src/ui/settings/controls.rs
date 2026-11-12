//! The sliders of the generated pages and the values they set. The official ones are `sN` sliders (module 158935:
//! `stickToMarkers`, one value per marker); their values live in `AccessibilityStore` and `TTSStore`, which are
//! device settings: they stay on this machine (`storage.json`) and never go to the account.

use super::color;
use super::model::Choice;
use crate::ui::Root;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};

/// One slider: the key of its row, the range its track covers, the values it can take (its markers) and the value
/// it starts at.
pub(super) struct SliderSpec {
    pub key: &'static str,
    pub min: f32,
    pub max: f32,
    pub markers: &'static [f32],
    pub default: f32,
    /// What the bubble over the grabber says (`onValueRender`).
    pub format: fn(f32) -> String,
}

const fn steps<const N: usize>(start: f32, step: f32) -> [f32; N] {
    let mut out = [0.; N];
    let mut index = 0;
    while index < N {
        out[index] = start + step * index as f32;
        index += 1;
    }
    out
}

/// `FONT_SIZES` (module 652215 `iA`).
const FONT_SIZES: [f32; 7] = [12., 14., 15., 16., 18., 20., 24.];
/// The marks of "Space Between Message Groups" (the settings page lists 0, 4, 8, 16 and 24px).
const SPACINGS: [f32; 5] = [0., 4., 8., 16., 24.];
/// `ZOOM_SCALES`.
const ZOOMS: [f32; 11] = [50., 67., 75., 80., 90., 100., 110., 125., 150., 175., 200.];
/// `SATURATION_INCREMENTS`: 0 to 1 in steps of .05.
const SATURATIONS: [f32; 21] = steps(0., 0.05);
/// `TOAST_DURATION_SECOND_INCREMENTS`: 3 to 20 seconds.
const TOASTS: [f32; 18] = steps(3., 1.);
/// Module 75804: the speech rate goes from 0 to 10 in steps of .25.
const RATES: [f32; 41] = steps(0., 0.25);

pub(super) const FONT_SIZE: &str = "appearance_font_scaling";
pub(super) const GROUP_SPACING: &str = "appearance_message_group_spacing";
pub(super) const ZOOM: &str = "appearance_zoom";
pub(super) const SATURATION: &str = "saturation";
pub(super) const TOAST_DURATION: &str = "toast_duration";
/// `PKaNJL` of the strings: the line the Preview button reads aloud.
const TTS_SAMPLE: &str = "This is what text-to-speech sounds like at the current speed.";
pub(super) const SPEECH_RATE: &str = "tts_playback_rate";

fn pixels(value: f32) -> String {
    format!("{value:.0}px")
}

fn percent(value: f32) -> String {
    format!("{value:.0}%")
}

fn saturation_text(value: f32) -> String {
    format!("{:.0}%", value * 100.)
}

fn seconds(value: f32) -> String {
    format!("{value:.0}s")
}

/// `x${e.toFixed(2)}`, module 158935 `formatValue`.
fn rate(value: f32) -> String {
    format!("x{value:.2}")
}

pub(super) static SLIDERS: [SliderSpec; 6] = [
    SliderSpec {
        key: FONT_SIZE,
        min: 12.,
        max: 24.,
        markers: &FONT_SIZES,
        default: 16.,
        format: pixels,
    },
    SliderSpec {
        key: GROUP_SPACING,
        min: 0.,
        max: 24.,
        markers: &SPACINGS,
        default: 16.,
        format: pixels,
    },
    SliderSpec {
        key: ZOOM,
        min: 50.,
        max: 200.,
        markers: &ZOOMS,
        default: 100.,
        format: percent,
    },
    SliderSpec {
        key: SATURATION,
        min: 0.,
        max: 1.,
        markers: &SATURATIONS,
        default: 1.,
        format: saturation_text,
    },
    SliderSpec {
        key: TOAST_DURATION,
        min: 3.,
        max: 20.,
        markers: &TOASTS,
        default: 3.,
        format: seconds,
    },
    SliderSpec {
        key: SPEECH_RATE,
        min: 0.,
        max: 10.,
        markers: &RATES,
        default: 1.,
        format: rate,
    },
];

pub(super) fn spec(key: &str) -> Option<&'static SliderSpec> {
    SLIDERS.iter().find(|spec| spec.key == key)
}

impl SliderSpec {
    /// Where a value sits along the track, 0 to 1.
    pub fn fraction_of(&self, value: f32) -> f32 {
        ((value - self.min) / (self.max - self.min)).clamp(0., 1.)
    }

    /// The marker nearest to a position on the track (`stickToMarkers`).
    pub fn value_at(&self, fraction: f32) -> f32 {
        let wanted = self.min + fraction.clamp(0., 1.) * (self.max - self.min);
        self.markers
            .iter()
            .copied()
            .min_by(|a, b| (a - wanted).abs().total_cmp(&(b - wanted).abs()))
            .unwrap_or(self.default)
    }

    /// The marker one step on from `value`: `direction` is -1 or 1.
    pub fn step(&self, value: f32, direction: i32) -> f32 {
        let current = self
            .markers
            .iter()
            .position(|marker| (marker - value).abs() < 1e-4)
            .unwrap_or_else(|| {
                self.markers
                    .iter()
                    .position(|marker| *marker >= value)
                    .unwrap_or(self.markers.len() - 1)
            });
        let next = (current as i64 + i64::from(direction)).clamp(0, self.markers.len() as i64 - 1);
        self.markers[next as usize]
    }
}

impl Root {
    /// A numeric device setting: the user's value here, else the default.
    pub(in crate::ui) fn pref_number(&self, key: &'static str, default: f32) -> f32 {
        match self.setting_choices.get(key) {
            Some(Choice::Number(value)) => *value,
            _ => default,
        }
    }

    /// The slider's own value (the default until it is moved).
    pub(super) fn slider_value(&self, spec: &SliderSpec) -> f32 {
        if spec.key == GROUP_SPACING {
            return self.message_group_spacing();
        }
        self.pref_number(spec.key, spec.default)
    }

    /// `AccessibilityStore.fontSize`: the size of the chat's text, 16px by default.
    pub(in crate::ui) fn chat_font_size(&self) -> f32 {
        self.pref_number(FONT_SIZE, 16.)
    }

    /// "Zoom level": the whole interface scaled like the official's page zoom, 50 to 200 percent (`ZOOMS`).
    pub(in crate::ui) fn zoom_factor(&self) -> f32 {
        (self.pref_number(ZOOM, 100.) / 100.).clamp(0.5, 2.0)
    }

    /// `ZOOM_IN`/`ZOOM_OUT`/`ZOOM_RESET` (module 406975): Ctrl plus, minus and 0 step through `ZOOMS`
    /// (`ZOOM_SCALES`) or return to 100, and the level is saved like the slider's.
    pub(in crate::ui) fn zoom_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let current = self.pref_number(ZOOM, 100.);
        let next = match key {
            "+" | "=" => step_zoom(current, 1),
            "-" => step_zoom(current, -1),
            "0" => 100.,
            _ => return false,
        };
        self.slider_committed(ZOOM, next);
        cx.notify();
        true
    }

    /// `AccessibilityStore.messageGroupSpacing`: 16px between groups, none in compact mode, until it is set.
    pub(in crate::ui) fn message_group_spacing(&self) -> f32 {
        let default = if self.compact_messages() { 0. } else { 16. };
        self.pref_number(GROUP_SPACING, default)
    }

    /// "Chat Message Display": compact lines instead of the cozy groups (`useCompactMode`).
    pub(in crate::ui) fn compact_messages(&self) -> bool {
        self.pref_index("appearance_message_display_mode", 0) == 1
    }

    pub(in crate::ui) fn saturation_factor(&self) -> f32 {
        self.pref_number(SATURATION, 1.)
    }

    /// The "Preview" button under the Text-to-Speech rate (module 158935 `es`): speaks a sample line at the chosen
    /// rate and shows Pause while it does, a second press stops it. 92 by 32 in the golden: 12px padding, a 16px icon,
    /// 4px, the 14/18 label.
    pub(super) fn tts_preview_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let speaking = self.speech.is_speaking();
        div()
            .id("tts-preview")
            .mt(px(16.))
            .self_start()
            .h(px(32.))
            .px(px(12.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap(px(4.))
            .bg(color("--control-primary-background-default"))
            .hover(|style| style.bg(color("--control-primary-background-hover")))
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(gpui::FontWeight(500.))
            .text_color(color("--control-primary-text-default"))
            .on_click(cx.listener(|this, _, _, cx| {
                // `PKaNJL` is the sample line; pressing again while it plays stops it.
                if this.speech.is_speaking() {
                    this.speech.stop();
                } else {
                    this.speech.speak(TTS_SAMPLE.to_owned(), this.speech_rate());
                }
                cx.notify();
            }))
            .child(if speaking {
                // The pause glyph: two bars.
                div()
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(3.))
                    .child(
                        div()
                            .w(px(3.))
                            .h(px(10.))
                            .rounded(px(1.))
                            .bg(color("--control-primary-text-default")),
                    )
                    .child(
                        div()
                            .w(px(3.))
                            .h(px(10.))
                            .rounded(px(1.))
                            .bg(color("--control-primary-text-default")),
                    )
                    .into_any_element()
            } else {
                super::mask_icon("sb_play", 16., "--control-primary-text-default")
                    .into_any_element()
            })
            .child(dc_intl::tr("Preview"))
            .into_any_element()
    }

    /// Minimum notification duration in milliseconds (`minToastDurationMs`).
    pub(in crate::ui) fn toast_duration_ms(&self) -> u64 {
        (self.pref_number(TOAST_DURATION, 3.) * 1000.) as u64
    }

    /// The text-to-speech rate (`TTSStore.speechRate`).
    pub(in crate::ui) fn speech_rate(&self) -> f32 {
        self.pref_number(SPEECH_RATE, 1.)
    }
}

/// Whether Windows is set to its light app theme (`AppsUseLightTheme`), read from the registry every few seconds by
/// a thread that starts with the first call. `GRAVECORD_DEVICE_THEME=light|dark` stands in for it in a harness run.
pub(super) fn device_light() -> Option<bool> {
    use std::sync::atomic::{AtomicU8, Ordering};
    static STATE: AtomicU8 = AtomicU8::new(2);
    static STARTED: std::sync::Once = std::sync::Once::new();
    if let Ok(forced) = std::env::var("GRAVECORD_DEVICE_THEME") {
        return Some(forced == "light");
    }
    #[cfg(windows)]
    STARTED.call_once(|| {
        std::thread::spawn(|| {
            loop {
                if let Some(light) = read_apps_use_light_theme() {
                    STATE.store(u8::from(light), Ordering::Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        });
    });
    #[cfg(not(windows))]
    let _ = &STARTED;
    match STATE.load(Ordering::Relaxed) {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

#[cfg(windows)]
fn read_apps_use_light_theme() -> Option<bool> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let value = text.split_whitespace().last()?;
    Some(value != "0x0")
}

impl Root {
    /// A setting took its new value: what has to happen right away besides the pages reading it.
    pub(in crate::ui) fn setting_applied(&mut self, key: &'static str) {
        self.theme_switch_changed(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_sit_where_the_page_draws_its_marks() {
        let font = spec(FONT_SIZE).unwrap();
        // The 14px mark is a sixth along the track, 16px a third (`pages.rs`).
        assert!((font.fraction_of(14.) - 0.16666).abs() < 1e-3);
        assert!((font.fraction_of(16.) - 0.33333).abs() < 1e-3);
        let zoom = spec(ZOOM).unwrap();
        assert!((zoom.fraction_of(67.) - 0.11332).abs() < 1e-3);
        let rate = spec(SPEECH_RATE).unwrap();
        assert!((rate.fraction_of(1.) - 0.1).abs() < 1e-3);
        let toast = spec(TOAST_DURATION).unwrap();
        assert!((toast.fraction_of(5.) - 0.11764).abs() < 1e-3);
    }

    #[test]
    fn a_drag_sticks_to_the_nearest_marker() {
        let font = spec(FONT_SIZE).unwrap();
        assert_eq!(font.value_at(0.), 12.);
        assert_eq!(font.value_at(1.), 24.);
        // Between 15px (.25) and 16px (.333) the middle goes to the nearer one.
        assert_eq!(font.value_at(0.27), 15.);
        assert_eq!(font.value_at(0.31), 16.);
        let saturation = spec(SATURATION).unwrap();
        assert!((saturation.value_at(0.52) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn arrow_keys_move_one_marker() {
        let font = spec(FONT_SIZE).unwrap();
        assert_eq!(font.step(16., 1), 18.);
        assert_eq!(font.step(16., -1), 15.);
        assert_eq!(font.step(24., 1), 24.);
        assert_eq!(font.step(12., -1), 12.);
    }

    #[test]
    fn bubbles_use_the_official_formats() {
        assert_eq!((spec(FONT_SIZE).unwrap().format)(16.), "16px");
        assert_eq!((spec(SPEECH_RATE).unwrap().format)(1.), "x1.00");
        assert_eq!((spec(TOAST_DURATION).unwrap().format)(3.), "3s");
        assert_eq!((spec(SATURATION).unwrap().format)(0.5), "50%");
    }

    #[test]
    fn accessibility_navigator_rows_open_their_pages() {
        use super::super::actions::destination_of;
        assert_eq!(
            destination_of("accessibility_to_display_navigator"),
            Some(("Appearance", None))
        );
        assert_eq!(
            destination_of("appearance_theme_accessibility_navigator"),
            Some(("Accessibility", None))
        );
        assert_eq!(
            destination_of("appearance_chat_accessibility_navigator"),
            Some(("Accessibility", None))
        );
    }
}

/// One step along `ZOOMS` from `current`; a value off the list counts as the first level (`indexOf` is -1).
fn step_zoom(current: f32, by: i32) -> f32 {
    let at = ZOOMS
        .iter()
        .position(|&z| z == current)
        .map_or(-1, |i| i as i32);
    ZOOMS[(at + by).clamp(0, ZOOMS.len() as i32 - 1) as usize]
}

#[cfg(test)]
mod zoom_key_tests {
    use super::*;

    #[test]
    fn zoom_steps_follow_the_official_scales() {
        assert_eq!(step_zoom(100., 1), 110.);
        assert_eq!(step_zoom(100., -1), 90.);
        assert_eq!(step_zoom(200., 1), 200.);
        assert_eq!(step_zoom(50., -1), 50.);
        assert_eq!(step_zoom(137., 1), 50.);
    }
}
