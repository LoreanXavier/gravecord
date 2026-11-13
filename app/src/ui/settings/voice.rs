//! Voice & Video's microphone controls as module 158935 builds them over `MediaEngineStore` (754712): the stored
//! choices, the input profile presets merged over them at read time (`t3()`), and the rows drawn for them (device
//! selects, volume sliders, Mic Test, input sensitivity, push to talk with its keybind and release delay). Measured in
//! `design/dom/settings/voice-video*-darker-cozy.json`; strings from `STRINGS/en-US.json` by the hashes the spec names.

pub(super) mod extra;

use super::model::{Choice, Node, Runs};
use super::{Page, Root, color, mask_icon};
use crate::storage;
use crate::ui::audio_menu::{self, Side, slider_to_volume, volume_to_slider};
use crate::ui::tooltip;
use gpui::{
    AnyElement, Bounds, Context, FontWeight, Image, ImageFormat, IntoElement, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Rgba, SharedString, Task,
    Window, canvas, deferred, div, img, linear_color_stop, linear_gradient, point, prelude::*, px,
    size,
};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) const PROFILE: &str = "voice_input_profile_setting";
pub(super) const SENSITIVITY: &str = "voice_input_sensitivity_field_set";
pub(super) const NOISE: &str = "voice_noise_suppression_setting";
pub(super) const ECHO: &str = "voice_echo_cancellation_setting";
/// `getArticleURL(VOICE_VIDEO_TROUBLESHOOTING)` with the page's own query (`oo` in module 158935).
const TROUBLESHOOTING_GUIDE: &str = "https://support.discord.com/hc/en-us/articles/360045138471?utm_source=discord&utm_medium=blog&utm_campaign=2020-06_help-voice-video&utm_content=--t%3Apm";
/// `getArticleURL(SOUNDBOARD)`: Soundboard Volume's and Entrance Sounds' links.
pub(super) const SOUNDBOARD_GUIDE: &str =
    "https://support.discord.com/hc/en-us/articles/12612888127767";
/// `getArticleURL(NO_INPUT_DETECTED)`.
const NO_INPUT_HELP: &str = "https://support.discord.com/hc/en-us/articles/214925018";

pub(super) const PUSH_TO_TALK: &str = "voice_push_to_talk_setting";
pub(super) const KEYBIND: &str = "voice_push_to_talk_keybind_setting";
pub(super) const RELEASE_DELAY: &str = "voice_push_to_talk_release_delay_setting";
pub(super) const AGC: &str = "voice_automatic_gain_control_setting";
pub(super) const ADVANCED: &str = "advanced_voice_activity_processing_setting";
const BYPASS: &str = "voice_bypass_system_input_processing_setting";
const INPUT_SELECT: &str = "voice_microphone_input_select";
const OUTPUT_SELECT: &str = "voice_speakers_output_select";
pub(super) const INPUT_VOLUME: &str = "voice_input_volume_setting";
pub(super) const OUTPUT_VOLUME: &str = "voice_output_volume_setting";
/// Soundboard Volume (`aU` in 158935): `soundboardSettings.volume`, 0 to 100, default 100.
pub(super) const SOUNDBOARD_VOLUME: &str = "soundboard_volume_setting";
const MIC_TEST: &str = "voice_microphone_test_setting";
pub(super) const ADVANCED_ACCORDION: &str = "input_profile_voice_advanced_accordion";

/// Where the choices are kept in storage.json, beside the user panel's devices and volumes (`audio.input_device`...).
const NOISE_STORED: &str = "audio.noise_suppression";
const ECHO_STORED: &str = "audio.echo_cancellation";
const ROOM_ECHO_STORED: &str = "audio.room_echo";
/// Shared with the user panel's Input Profile submenu, which writes the same label.
pub(in crate::ui) const PROFILE_STORED: &str = "audio.input_profile";
const PUSH_TO_TALK_STORED: &str = "audio.push_to_talk";
const THRESHOLD_STORED: &str = "audio.threshold";
const AUTO_THRESHOLD_STORED: &str = "audio.auto_threshold";
const ADVANCED_STORED: &str = "audio.advanced_voice_activity";
const DELAY_STORED: &str = "audio.push_to_talk_delay";
const SHORTCUT_STORED: &str = "audio.push_to_talk_shortcut";
const AGC_STORED: &str = "audio.automatic_gain_control";
const SOUNDBOARD_VOLUME_STORED: &str = "audio.soundboard_volume";

/// A stored soundboard volume as the setting reads it (`H()`: `volume ?? 100`), kept inside 0 to 100.
fn soundboard_volume_from(stored: Option<&serde_json::Value>) -> f32 {
    stored
        .and_then(serde_json::Value::as_f64)
        .map_or(100., |volume| (volume as f32).clamp(0., 100.))
}

/// The soundboard volume, 0 to 100 (default 100), as Soundboard Volume in Voice & Video keeps it. Sounds played from
/// the soundboard scale by `volume / 100`.
pub fn soundboard_volume() -> f32 {
    soundboard_volume_from(storage::get(SOUNDBOARD_VOLUME_STORED).as_ref())
}

/// `USER_SOUNDBOARD_SET_VOLUME`: keeps a volume (0 to 100) as the setting; the picker's own slider writes it too.
pub fn set_soundboard_volume(volume: f32) {
    storage::set(
        SOUNDBOARD_VOLUME_STORED,
        json!(volume.clamp(0., 100.).round()),
    );
}

/// What the slider writes (`w` of 792251 over its 0 to 100 position): whole-percent steps on the volume curve.
fn soundboard_volume_at(fraction: f32) -> f32 {
    slider_to_volume((fraction * 100.).round())
        .round()
        .clamp(0., 100.)
}

fn store_soundboard_volume(fraction: f32) {
    storage::set(
        SOUNDBOARD_VOLUME_STORED,
        json!(soundboard_volume_at(fraction)),
    );
}

/// The rows module 158935 adds under Push to Talk while it is on (`ae`, `at`); the generated page has neither.
static KEYBIND_ROW: Node = Node::Custom {
    key: KEYBIND,
    height: 76.,
};
static DELAY_ROW: Node = Node::Custom {
    key: RELEASE_DELAY,
    height: 68.,
};

/// `r5.m` (621380), in the radio's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Profile {
    VoiceIsolation,
    Studio,
    Custom,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::VoiceIsolation, Profile::Studio, Profile::Custom];

    /// `cjPbpT`, `VZPR0R`, `N/PQjv`.
    pub fn label(self) -> &'static str {
        match self {
            Profile::VoiceIsolation => dc_intl::tr("Voice Isolation"),
            Profile::Studio => dc_intl::tr("Studio"),
            Profile::Custom => dc_intl::tr("Custom"),
        }
    }

    pub fn index(self) -> usize {
        match self {
            Profile::VoiceIsolation => 0,
            Profile::Studio => 1,
            Profile::Custom => 2,
        }
    }

    /// `null` (nothing stored) is Custom, as `getActiveInputProfile() ?? CUSTOM` reads it.
    pub fn from_label(label: &str) -> Self {
        Profile::ALL
            .into_iter()
            .find(|profile| profile.label() == label)
            .unwrap_or(Profile::Custom)
    }
}

/// What `MediaEngineStore` keeps for the default context (754712 `e5()`), as far as this page shows it. Noise
/// suppression is the select's index: Krisp (DeepFilterNet here), Standard, None.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Stored {
    pub profile: Profile,
    /// `mode === PUSH_TO_TALK`.
    pub push_to_talk: bool,
    /// `modeOptions.threshold`, dB.
    pub threshold: f32,
    pub auto_threshold: bool,
    /// `modeOptions.vadUseKrisp`.
    pub advanced_voice_activity: bool,
    /// `modeOptions.delay`, ms.
    pub delay: f32,
    /// `modeOptions.shortcut`: `[deviceType, code, platform]` each.
    pub shortcut: Vec<[u32; 3]>,
    pub echo: bool,
    pub suppression: usize,
    /// gravecord's Reduce Room Echo.
    pub room_echo: bool,
    pub agc: bool,
}

impl Default for Stored {
    /// `e5()` on desktop: voice activity, -60 dB, automatic, Krisp's VAD, 20 ms, no keybind, echo cancellation,
    /// Krisp, automatic gain control, no profile.
    fn default() -> Self {
        Self {
            profile: Profile::Custom,
            push_to_talk: false,
            threshold: -60.,
            auto_threshold: true,
            advanced_voice_activity: true,
            delay: 20.,
            shortcut: Vec::new(),
            echo: true,
            suppression: 0,
            room_echo: false,
            agc: true,
        }
    }
}

/// The values every getter reads: the stored ones with the profile's preset over them (`t3()` and `e7`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Effective {
    pub push_to_talk: bool,
    pub threshold: f32,
    pub auto_threshold: bool,
    pub advanced_voice_activity: bool,
    pub delay: f32,
    pub echo: bool,
    pub suppression: usize,
    pub room_echo: bool,
    pub agc: bool,
}

impl Stored {
    /// `e7[activeInputProfile ?? CUSTOM]` over the stored values. Voice Isolation forces automatic sensitivity with
    /// Krisp's VAD, echo cancellation, Krisp and AGC; Studio forces voice activity at -84 dB with everything off.
    /// Reduce Room Echo is gravecord's own and off under both presets, which choose all processing themselves.
    pub fn effective(&self) -> Effective {
        let mut effective = Effective {
            push_to_talk: self.push_to_talk,
            threshold: self.threshold,
            auto_threshold: self.auto_threshold,
            advanced_voice_activity: self.advanced_voice_activity,
            delay: self.delay,
            echo: self.echo,
            suppression: self.suppression,
            room_echo: self.room_echo,
            agc: self.agc,
        };
        match self.profile {
            Profile::Custom => {}
            Profile::VoiceIsolation => {
                effective.auto_threshold = true;
                effective.advanced_voice_activity = true;
                effective.echo = true;
                effective.suppression = 0;
                effective.agc = true;
                effective.room_echo = false;
            }
            Profile::Studio => {
                effective.push_to_talk = false;
                effective.threshold = -84.;
                effective.auto_threshold = false;
                effective.advanced_voice_activity = false;
                effective.echo = false;
                effective.suppression = 2;
                effective.agc = false;
                effective.room_echo = false;
            }
        }
        effective
    }

    /// `lE.setMode(mode, changes)`: the options written are the effective ones with the changes over them, so a mode
    /// edit under a preset saves the preset's sensitivity into the custom values, as the official store does.
    pub fn set_mode(&mut self, push_to_talk: bool, change: impl FnOnce(&mut Stored)) {
        let effective = self.effective();
        self.threshold = effective.threshold;
        self.auto_threshold = effective.auto_threshold;
        self.advanced_voice_activity = effective.advanced_voice_activity;
        self.push_to_talk = push_to_talk;
        change(self);
    }

    fn load() -> Self {
        let mut stored = Stored::default();
        let bool_of = |key: &str| storage::get(key).and_then(|value| value.as_bool());
        let float_of = |key: &str| storage::get(key).and_then(|value| value.as_f64());
        if let Some(label) =
            storage::get(PROFILE_STORED).and_then(|value| value.as_str().map(str::to_owned))
        {
            stored.profile = Profile::from_label(&label);
        }
        if let Some(on) = bool_of(PUSH_TO_TALK_STORED) {
            stored.push_to_talk = on;
        }
        if let Some(db) = float_of(THRESHOLD_STORED) {
            stored.threshold = (db as f32).clamp(-100., 0.);
        }
        if let Some(on) = bool_of(AUTO_THRESHOLD_STORED) {
            stored.auto_threshold = on;
        }
        if let Some(on) = bool_of(ADVANCED_STORED) {
            stored.advanced_voice_activity = on;
        }
        if let Some(ms) = float_of(DELAY_STORED) {
            stored.delay = (ms as f32).clamp(0., 2000.);
        }
        if let Some(list) =
            storage::get(SHORTCUT_STORED).and_then(|value| value.as_array().cloned())
        {
            stored.shortcut = list
                .iter()
                .filter_map(|entry| {
                    let parts = entry.as_array()?;
                    let part = |index: usize| parts.get(index)?.as_u64().map(|value| value as u32);
                    Some([part(0)?, part(1)?, part(2)?])
                })
                .collect();
        }
        if let Some(on) = bool_of(ECHO_STORED) {
            stored.echo = on;
        }
        if let Some(index) = storage::get(NOISE_STORED).and_then(|value| value.as_u64()) {
            stored.suppression = (index as usize).min(2);
        }
        if let Some(on) = bool_of(ROOM_ECHO_STORED) {
            stored.room_echo = on;
        }
        if let Some(on) = bool_of(AGC_STORED) {
            stored.agc = on;
        }
        stored
    }

    fn save(&self) {
        storage::set(PROFILE_STORED, json!(self.profile.label()));
        storage::set(PUSH_TO_TALK_STORED, json!(self.push_to_talk));
        storage::set(THRESHOLD_STORED, json!(self.threshold));
        storage::set(AUTO_THRESHOLD_STORED, json!(self.auto_threshold));
        storage::set(ADVANCED_STORED, json!(self.advanced_voice_activity));
        storage::set(DELAY_STORED, json!(self.delay));
        storage::set(SHORTCUT_STORED, json!(self.shortcut));
        storage::set(ECHO_STORED, json!(self.echo));
        storage::set(NOISE_STORED, json!(self.suppression));
        storage::set(ROOM_ECHO_STORED, json!(self.room_echo));
        storage::set(AGC_STORED, json!(self.agc));
    }
}

/// The keys `Stored` holds (`voice_setting_changed`).
const STORED_KEYS: [&str; 8] = [
    PROFILE,
    SENSITIVITY,
    ADVANCED,
    PUSH_TO_TALK,
    NOISE,
    ECHO,
    AGC,
    super::render::ROOM_ECHO_KEY,
];

/// Gives the processors the effective values (`t6`, `ns`). Every mode push lets go of push to talk first, as
/// `t6` does through 140175.
fn push(effective: &Effective) {
    let processing = &dc_voice::prefs().processing;
    processing.set_suppression(super::render::suppression(effective.suppression));
    processing.set_echo_cancellation(effective.echo);
    processing.set_room_echo(effective.room_echo);
    processing.set_automatic_gain_control(effective.agc);
    processing.set_push_to_talk(effective.push_to_talk);
    processing.set_push_to_talk_delay_ms(effective.delay.round() as u32);
    processing.set_threshold_db(effective.threshold);
    processing.set_auto_threshold(effective.auto_threshold);
    processing.set_advanced_voice_activity(effective.advanced_voice_activity);
    processing.set_push_to_talk_held(false);
}

/// `Number.prototype.toFixed`: the sign kept apart and halves rounded away from zero.
fn to_fixed(value: f32, digits: usize) -> String {
    let scale = 10f64.powi(digits as i32);
    let magnitude = ((value.abs() as f64) * scale + 0.5).floor() / scale;
    let text = format!("{magnitude:.digits$}");
    if value < 0. && magnitude != 0. {
        format!("-{text}")
    } else {
        text
    }
}

/// The manual sensitivity slider's bubble: `${(-((100-e)*1)).toFixed(0)}dB` for slider position `e`.
pub(super) fn sensitivity_bubble(position: f32) -> String {
    format!("{}dB", to_fixed(-(100. - position), 0))
}

/// The release delay's bubble: `1.50s` from a second up, else `20 ms` with a no-break space.
pub(super) fn delay_bubble(ms: f32) -> String {
    if ms >= 1000. {
        format!("{}s", to_fixed(ms / 1000., 2))
    } else {
        format!("{}\u{a0}ms", to_fixed(ms, 0))
    }
}

/// `tI` in module 158935: the first titles (three under Custom, two otherwise), joined as the strings `3H9tCW`,
/// `MWryo6`, `a00b5G` say, or with "and more" (`O8vNbS`, `acXG/W`, `5+ldWc`) when more follow.
pub(super) fn collapsed_subtitle(titles: &[&str], limit: usize) -> String {
    let shown = limit.min(3).min(titles.len());
    let first = &titles[..shown];
    if titles.len() <= shown {
        match first {
            [a] => (*a).to_owned(),
            [a, b] => format!("{a} and {b}"),
            [a, b, c] => format!("{a}, {b}, and {c}"),
            _ => String::new(),
        }
    } else {
        match first {
            [a] => format!("{a} and more"),
            [a, b] => format!("{a}, {b}, and more"),
            [a, b, c] => format!("{a}, {b}, {c}, and more"),
            _ => String::new(),
        }
    }
}

/// The title a node shows (`useTitle`), for the accordion's collapsed subtitle.
fn node_title(node: &Node) -> Option<&'static str> {
    match node {
        Node::Switch { label, .. }
        | Node::Slider { label, .. }
        | Node::Radio { label, .. }
        | Node::Select { label, .. }
        | Node::Control { label, .. }
        | Node::Accordion { label, .. } => Some(label),
        Node::Category { title, legend, .. } => title.or(*legend),
        _ => None,
    }
}

/// `u.Nu` in module 650583: Windows virtual-key codes by key name, in the bundle's order (layout extras last).
const WINDOWS_KEYS: &[(&str, u32)] = &[
    ("a", 65),
    ("s", 83),
    ("d", 68),
    ("f", 70),
    ("h", 72),
    ("g", 71),
    ("z", 90),
    ("x", 88),
    ("c", 67),
    ("v", 86),
    ("b", 66),
    ("q", 81),
    ("w", 87),
    ("e", 69),
    ("r", 82),
    ("y", 89),
    ("t", 84),
    ("1", 49),
    ("2", 50),
    ("3", 51),
    ("4", 52),
    ("6", 54),
    ("5", 53),
    ("=", 187),
    ("9", 57),
    ("7", 55),
    ("-", 189),
    ("8", 56),
    ("0", 48),
    ("]", 221),
    ("o", 79),
    ("u", 85),
    ("[", 219),
    ("i", 73),
    ("p", 80),
    ("l", 76),
    ("j", 74),
    ("k", 75),
    (";", 186),
    (",", 188),
    ("/", 191),
    ("n", 78),
    ("m", 77),
    (".", 190),
    ("numpad .", 110),
    ("numpad *", 106),
    ("numpad +", 107),
    ("numpad clear", 144),
    ("numpad /", 111),
    ("numpad -", 109),
    ("numpad =", 226),
    ("numpad 0", 96),
    ("numpad 1", 97),
    ("numpad 2", 98),
    ("numpad 3", 99),
    ("numpad 4", 100),
    ("numpad 5", 101),
    ("numpad 6", 102),
    ("numpad 7", 103),
    ("numpad 8", 104),
    ("numpad 9", 105),
    ("enter", 13),
    ("tab", 9),
    ("space", 32),
    ("backspace", 8),
    ("esc", 27),
    ("meta", 91),
    ("shift", 160),
    ("caps lock", 20),
    ("alt", 164),
    ("ctrl", 162),
    ("right shift", 161),
    ("right alt", 165),
    ("right ctrl", 163),
    ("right meta", 93),
    ("f1", 112),
    ("f2", 113),
    ("f3", 114),
    ("f4", 115),
    ("f5", 116),
    ("f6", 117),
    ("f7", 118),
    ("f8", 119),
    ("f9", 120),
    ("f10", 121),
    ("f11", 122),
    ("f12", 123),
    ("f13", 124),
    ("f14", 125),
    ("f15", 126),
    ("f16", 127),
    ("f17", 128),
    ("f18", 129),
    ("f19", 130),
    ("f20", 131),
    ("f21", 132),
    ("f22", 133),
    ("f23", 134),
    ("f24", 135),
    ("home", 36),
    ("page up", 33),
    ("del", 46),
    ("end", 35),
    ("page down", 34),
    ("left", 37),
    ("right", 39),
    ("down", 40),
    ("up", 38),
    ("insert", 45),
    ("break", 19),
    ("scroll lock", 145),
    ("print screen", 44),
    ("rewind", 177),
    ("play", 179),
    ("fast forward", 176),
    ("^", 220),
    ("\u{a7}", 220),
    ("\u{bd}", 220),
    ("plus", 192),
    ("\u{17e}", 192),
    ("@", 192),
    ("|", 220),
    ("#", 222),
    ("$", 222),
    ("process", 229),
    ("\u{b7}", 229),
    ("\u{630}", 192),
    ("`", 192),
    ("\u{ac}", 223),
    ("\\", 220),
    ("'", 222),
];

/// `KEYBOARD_KEY`, `MOUSE_BUTTON`, `KEYBOARD_MODIFIER_KEY`, `GAMEPAD_BUTTON` (650583 `zY`).
pub(super) const KEYBOARD_KEY: u32 = 0;
pub(super) const MOUSE_BUTTON: u32 = 1;
const KEYBOARD_MODIFIER_KEY: u32 = 2;
const GAMEPAD_BUTTON: u32 = 3;
/// `g$.WINDOWS`.
pub(super) const WINDOWS: u32 = 1;

/// 350535's `f = invert(u.Nu)` with `f["223"] = "`"`: lodash keeps the last name a code appears under.
fn windows_key_name(code: u32) -> Option<&'static str> {
    if code == 223 {
        return Some("`");
    }
    WINDOWS_KEYS
        .iter()
        .rev()
        .find(|(_, value)| *value == code)
        .map(|(name, _)| *name)
}

/// 350535 `dI(shortcut, true)`: each key's name, `mouseN`, `gamepadN` or `UNKn`, joined with " + " and upper-cased.
pub(super) fn shortcut_text(shortcut: &[[u32; 3]]) -> String {
    shortcut
        .iter()
        .map(|[kind, code, platform]| match *kind {
            KEYBOARD_KEY | KEYBOARD_MODIFIER_KEY => (*platform == WINDOWS)
                .then(|| windows_key_name(*code))
                .flatten()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("UNK{code}")),
            MOUSE_BUTTON => format!("mouse{code}"),
            GAMEPAD_BUTTON => format!("gamepad{code}"),
            other => format!("dev{other},{code}"),
        })
        .collect::<Vec<_>>()
        .join(" + ")
        .to_uppercase()
}

/// gpui's key name for a key that went down, as the Windows code the official recorder stores. gpui names a few
/// keys differently, and turns shift with a symbol key into the shifted symbol; those are taken back to the key
/// that makes them on a US layout.
pub(super) fn windows_code(key: &str) -> Option<u32> {
    let name = match key {
        "escape" => "esc",
        "pageup" => "page up",
        "pagedown" => "page down",
        "delete" => "del",
        "menu" => "right meta",
        "!" => "1",
        "@" => "2",
        "#" => "3",
        "$" => "4",
        "%" => "5",
        "^" => "6",
        "&" => "7",
        "*" => "8",
        "(" => "9",
        ")" => "0",
        "_" => "-",
        "+" => "=",
        "{" => "[",
        "}" => "]",
        ":" => ";",
        "\"" => "'",
        "<" => ",",
        ">" => ".",
        "?" => "/",
        "~" => "`",
        "|" => "\\",
        other => other,
    };
    WINDOWS_KEYS
        .iter()
        .find(|(entry, _)| *entry == name)
        .map(|(_, code)| *code)
}

/// Every key of a keybind is down. Mouse and gamepad parts cannot be held in the window, so they never are.
pub(super) fn shortcut_down(shortcut: &[[u32; 3]], held: &[u32]) -> bool {
    !shortcut.is_empty()
        && shortcut.iter().all(|[kind, code, _]| {
            matches!(*kind, KEYBOARD_KEY | KEYBOARD_MODIFIER_KEY) && held.contains(code)
        })
}

/// The modifier keys down, as the codes of `ctrl`, `shift`, `alt` and `meta`.
pub(super) fn modifier_codes(modifiers: &Modifiers) -> Vec<u32> {
    [
        (modifiers.control, 162),
        (modifiers.shift, 160),
        (modifiers.alt, 164),
        (modifiers.platform, 91),
    ]
    .into_iter()
    .filter_map(|(down, code)| down.then_some(code))
    .collect()
}

/// A slider being dragged: its key, its bounds in the window, and where it is (0 to 1).
struct Drag {
    key: &'static str,
    bounds: Bounds<Pixels>,
    fraction: f32,
}

struct MicTest {
    started: Instant,
    /// `didDeafenUser`: we deafened the call and undeafen it after.
    deafened: bool,
    /// When the level last sat at the floor, for the 2000 ms before "no input" shows.
    silent_since: Option<Instant>,
    detecting: bool,
}

/// The system's devices, read when the page opens and when a select opens.
#[derive(Default)]
struct Devices {
    inputs: Vec<String>,
    outputs: Vec<String>,
    default_input: Option<String>,
    default_output: Option<String>,
}

impl Devices {
    fn read() -> Self {
        Self {
            inputs: dc_voice::input_devices(),
            outputs: dc_voice::output_devices(),
            default_input: dc_voice::default_input_device(),
            default_output: dc_voice::default_output_device(),
        }
    }
}

/// The voice settings' state on `Root`.
pub(in crate::ui) struct VoiceState {
    stored: Stored,
    /// The microphone outside a call, while the page shows (and for Mic Test in a call).
    preview: Option<dc_voice::Preview>,
    preview_failed: Option<Instant>,
    repaint: Option<Task<()>>,
    mic_test: Option<MicTest>,
    /// The keybind recorder is listening (`RECORDING`), and the keys held since it started.
    recording: bool,
    /// `[deviceType, code]` of what is down while recording.
    recording_held: Vec<[u32; 2]>,
    /// Keys held now, for push to talk.
    held: Vec<u32>,
    modifiers: Modifiers,
    drag: Option<Drag>,
    /// A focus handle per slider, so the arrow keys reach the one that was clicked.
    slider_focus: std::cell::RefCell<std::collections::HashMap<&'static str, gpui::FocusHandle>>,
    hovered_slider: Option<&'static str>,
    device_open: Option<Side>,
    devices: Devices,
    /// The Camera section: its select and Test Video.
    pub(super) camera: super::voice_camera::CameraPage,
    page_shown: bool,
    /// The switches, slider and buttons that act on a call or the machine (`extra.rs`).
    extra: extra::Extra,
}

impl VoiceState {
    /// At start: the stored choices, given to the processors.
    pub fn restore() -> Self {
        let stored = Stored::load();
        push(&stored.effective());
        Self {
            stored,
            preview: None,
            preview_failed: None,
            repaint: None,
            mic_test: None,
            recording: false,
            recording_held: Vec::new(),
            held: Vec::new(),
            modifiers: Modifiers::default(),
            drag: None,
            slider_focus: Default::default(),
            hovered_slider: None,
            device_open: None,
            devices: Devices::default(),
            camera: Default::default(),
            page_shown: false,
            extra: extra::Extra::restore(),
        }
    }

    /// Reset all Voice & Video settings: the stored choices are the defaults again, given to the processors.
    fn reset_stored(&mut self) {
        self.stored = Stored::default();
        push(&self.stored.effective());
        self.held.clear();
    }

    /// The voice settings as the upload's `mediaEngineState` carries them.
    fn stored_json(&self) -> serde_json::Value {
        let stored = &self.stored;
        json!({
            "inputProfile": stored.profile.label(),
            "pushToTalk": stored.push_to_talk,
            "threshold": stored.threshold,
            "autoThreshold": stored.auto_threshold,
            "advancedVoiceActivity": stored.advanced_voice_activity,
            "delay": stored.delay,
            "echoCancellation": stored.echo,
            "noiseSuppression": stored.suppression,
            "automaticGainControl": stored.agc,
            "inputVolume": dc_voice::prefs().input_gain(),
            "outputVolume": dc_voice::prefs().output_gain(),
        })
    }

    /// The page's choices for the stored values: what the generated switches, radio and select read.
    pub fn choices(&self) -> std::collections::HashMap<&'static str, Choice> {
        let stored = &self.stored;
        [
            (PROFILE, Choice::Index(stored.profile.index())),
            (SENSITIVITY, Choice::On(stored.auto_threshold)),
            (NOISE, Choice::Index(stored.suppression)),
            (ECHO, Choice::On(stored.echo)),
            (super::render::ROOM_ECHO_KEY, Choice::On(stored.room_echo)),
            (PUSH_TO_TALK, Choice::On(stored.push_to_talk)),
            (AGC, Choice::On(stored.agc)),
            (ADVANCED, Choice::On(stored.advanced_voice_activity)),
        ]
        .into_iter()
        .chain(self.extra.choices())
        .collect()
    }

    /// One line for the harness's `note`.
    #[cfg(feature = "capture")]
    pub fn describe(&self) -> String {
        let stored = &self.stored;
        let (level, voice) = dc_voice::prefs().processing.meter();
        format!(
            "profile={} ptt={} auto={} threshold={:.1} advanced={} delay={} shortcut={:?} echo={} suppression={} agc={} recording={} mic_test={:?} preview={} level={level:.1} voice={voice} held={}",
            stored.profile.label(),
            stored.push_to_talk,
            stored.auto_threshold,
            stored.threshold,
            stored.advanced_voice_activity,
            stored.delay,
            shortcut_text(&stored.shortcut),
            stored.echo,
            stored.suppression,
            stored.agc,
            self.recording,
            self.mic_test.as_ref().map(|test| if test.detecting {
                "detecting"
            } else {
                "no-input"
            }),
            self.preview.is_some(),
            dc_voice::prefs().processing.push_to_talk_held(),
        )
    }
}

fn label_text(text: &'static str) -> gpui::Div {
    div()
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-strong"))
        .child(text)
}

fn with_alpha(mut value: Rgba, alpha: f32) -> Rgba {
    value.a = alpha;
    value
}

fn hex(value: Rgba) -> String {
    let byte = |channel: f32| (channel * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        byte(value.r),
        byte(value.g),
        byte(value.b)
    )
}

/// The meter's notches (782600 `M`): 8px cells, each filled but for a 4px pill, in the bundle's path.
fn notch_svg(width: u32, height: u32, fill: &str) -> String {
    let inner = height.saturating_sub(8);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><defs><pattern id="pill" width="8" height="{height}" patternUnits="userSpaceOnUse"><path d="m0 0h8v{height}h-8zm4 2c-1.1045695 0-2 .8954305-2 2v{inner}c0 1.1045695.8954305 2 2 2s2-.8954305 2-2v-{inner}c0-1.1045695-.8954305-2-2-2z" fill-rule="evenodd" fill="{fill}"/></pattern></defs><rect width="100%" height="100%" fill="url(#pill)"/></svg>"#
    )
}

/// `v(e) = 8 * Math.round(e / 8)`.
fn to_eight(value: f32) -> f32 {
    8. * (value / 8. + 0.5).floor()
}

/// A `sm` button (`sm_F7qvhY`): 32 tall, 11px in, `body-sm/medium`.
fn small_button(text: &'static str, background: Rgba, border: Rgba, foreground: Rgba) -> gpui::Div {
    div()
        .flex_none()
        .h(px(32.))
        .px(px(11.))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(background)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(foreground)
        .whitespace_nowrap()
        .child(text)
}

/// The open flag of the page's selects (device lists, camera list) in the layer system: Escape closes the list
/// before the settings window (`keyboard/layers.rs`: every `/open` choice is a select layer).
const SELECT_OPEN: &str = "voice_select/open";

impl Root {
    /// Mirrors the lists' open state into the layer system.
    pub(in crate::ui) fn voice_sync_select_flag(&mut self) {
        let open = self.voice.device_open.is_some() || self.voice.camera.open;
        self.setting_choices.insert(SELECT_OPEN, Choice::On(open));
    }

    /// Escape or an outside press closed the select layer: the lists follow.
    pub(in crate::ui) fn voice_selects_closed_by_layer(&mut self) {
        self.voice.device_open = None;
        self.voice.camera.open = false;
    }

    fn voice_custom_profile(&self) -> bool {
        self.voice.stored.profile == Profile::Custom
    }

    /// Each row's `usePredicate`: the processing controls only under Custom (`isInputProfileCustom()`), Push to Talk
    /// except under Studio, its keybind and delay only while the mode is push to talk.
    pub(super) fn voice_row_shown(&self, key: &str) -> bool {
        match key {
            SENSITIVITY | NOISE | ECHO | AGC | ADVANCED | BYPASS => self.voice_custom_profile(),
            key if key == super::render::ROOM_ECHO_KEY => self.voice_custom_profile(),
            PUSH_TO_TALK => self.voice.stored.profile != Profile::Studio,
            KEYBIND | RELEASE_DELAY => self.voice.stored.effective().push_to_talk,
            extra::ATTENUATION_SET => extra::attenuation_shown(),
            // `usePredicate`: the strength only while Stream Attenuation is on.
            extra::STREAM_ATTEN_STRENGTH => self.voice.extra.stream_attenuation,
            // `usePredicate` of the Video Background row (module 463951): the engine has the video filter (here: the
            // segmentation model is present, see voice_camera/background.rs) and there is a camera.
            "camera_background_setting" => self.video_background_shown(),
            _ => true,
        }
    }

    /// Rows drawn after `key` that the generated page lacks.
    pub(super) fn voice_rows_after(&self, key: &str) -> Vec<&'static Node> {
        match key {
            ECHO => vec![&super::render::ROOM_ECHO],
            PUSH_TO_TALK => vec![&KEYBIND_ROW, &DELAY_ROW],
            _ => Vec::new(),
        }
    }

    /// `useDisabled`: Advanced Voice Activity only works with automatic sensitivity in voice activity, and the
    /// sensitivity settings not at all in push to talk.
    pub(super) fn voice_disabled(&self, key: &str) -> bool {
        let effective = self.voice.stored.effective();
        match key {
            ADVANCED => effective.push_to_talk || !effective.auto_threshold,
            SENSITIVITY => effective.push_to_talk,
            _ => false,
        }
    }

    /// The advanced accordion's collapsed subtitle, from the rows it shows now.
    pub(super) fn voice_accordion_subtitle(&self, children: &'static [Node]) -> String {
        let titles: Vec<&str> = children
            .iter()
            .filter(|child| self.voice_row_shown(child.key()))
            .filter_map(node_title)
            .collect();
        let limit = if self.voice_custom_profile() { 3 } else { 2 };
        collapsed_subtitle(&titles, limit)
    }

    /// A page choice changed (the generated switch, radio or select already wrote it into `setting_choices`): the
    /// store takes it as the official action does, saves, and the processors follow.
    pub(in crate::ui) fn voice_setting_changed(&mut self, key: &'static str) {
        if self.overlay_setting_changed(key) {
            return;
        }
        if self.clips_setting_changed(key) {
            return;
        }
        let on = matches!(self.setting_choices.get(key), Some(Choice::On(true)));
        let index = match self.setting_choices.get(key) {
            Some(Choice::Index(index)) => *index,
            _ => 0,
        };
        if self.notification_page_changed(key, on, index) {
            return;
        }
        // The Sounds lists (Voice & Video, Notifications) are the notification store's switches.
        if crate::sounds::set_switch(key, on) {
            return;
        }
        if crate::notifications::set_switch(
            key,
            on,
            self.stores.current_user.as_ref().map(|user| user.id),
        ) {
            return;
        }
        if key == "streaming_show_stream_previews" {
            // Read by the engine: the stream we send uploads previews only while it is on.
            crate::storage::set("audio.stream_previews", json!(on));
            self.stream_apply_previews(on);
            return;
        }
        if self.extra_setting_changed(key, on) {
            self.apply_stream_attenuation();
            return;
        }
        if self.action_switch_changed(key, on, index) {
            return;
        }
        self.system_setting_changed(key, on);
        // The voice store owns these keys; `pref_changed` would otherwise keep them as generic rows that only flip.
        if !STORED_KEYS.contains(&key) && self.pref_changed(key, on, index) {
            self.setting_applied(key);
            return;
        }
        let effective_mode = self.voice.stored.effective().push_to_talk;
        let stored = &mut self.voice.stored;
        match key {
            PROFILE => stored.profile = Profile::ALL[index.min(2)],
            SENSITIVITY => stored.set_mode(effective_mode, |stored| stored.auto_threshold = on),
            ADVANCED => stored.set_mode(effective_mode, |stored| {
                stored.advanced_voice_activity = on;
            }),
            PUSH_TO_TALK => stored.set_mode(on, |_| {}),
            NOISE => stored.suppression = index.min(2),
            ECHO => stored.echo = on,
            AGC => stored.agc = on,
            key if key == super::render::ROOM_ECHO_KEY => stored.room_echo = on,
            _ => return,
        }
        self.voice_commit();
    }

    /// The user panel's Input Profile submenu (`setActiveInputProfile`).
    pub(in crate::ui) fn set_input_profile(&mut self, profile: Profile) {
        self.voice.stored.profile = profile;
        self.voice_commit();
    }

    /// Saves, tells the processors, and shows the stored values on the page.
    fn voice_commit(&mut self) {
        self.voice.stored.save();
        push(&self.voice.stored.effective());
        self.voice.held.clear();
        self.sync_ptt_hook();
        let choices = self.voice.choices();
        self.setting_choices.extend(choices);
    }

    fn voice_page_visible(&self) -> bool {
        self.noise_popout_open()
            || self
                .settings
                .as_ref()
                .is_some_and(|open| open.page == Page::VoiceVideo)
    }

    /// `getNoiseCancellation()`: DeepFilterNet is what suppresses noise, under the input profile's choice.
    pub(in crate::ui) fn noise_cancellation(&self) -> bool {
        self.voice.stored.effective().suppression == 0
    }

    /// The input profile decides, so the voice panel's switch is locked (`r` in the official popout).
    pub(in crate::ui) fn noise_cancellation_locked(&self) -> bool {
        self.voice.stored.profile != Profile::Custom
    }

    pub(in crate::ui) fn voice_profile_label(&self) -> Option<&'static str> {
        (self.voice.stored.profile != Profile::Custom).then(|| self.voice.stored.profile.label())
    }

    /// `setNoiseCancellation`: off falls back to the standard suppressor, as the store's default does.
    pub(in crate::ui) fn set_noise_cancellation(&mut self, on: bool) {
        self.voice.stored.suppression = if on { 0 } else { 1 };
        self.voice_commit();
    }

    pub(in crate::ui) fn voice_mic_testing(&self) -> bool {
        self.voice.mic_test.is_some()
    }

    /// The meter's fill while Mic Test runs, 0 to 100.
    pub(in crate::ui) fn voice_mic_progress(&self) -> f32 {
        if self.voice.mic_test.is_none() {
            return 0.;
        }
        let (level, _) = dc_voice::prefs().processing.meter();
        (level + 100.).clamp(0., 100.)
    }

    pub(in crate::ui) fn toggle_mic_test(&mut self, cx: &mut Context<Self>) {
        if self.voice.mic_test.is_some() {
            self.stop_mic_test(cx);
        } else {
            self.start_mic_test(cx);
        }
    }

    /// The meter's notches on the voice panel popout's colour.
    pub(in crate::ui) fn small_notches(&self, width: u32, height: u32) -> Arc<Image> {
        self.notches_on(width, height, color("--background-surface-higher"))
    }

    /// The same notches over any surface colour (the call view's menu is midnight whatever the theme).
    pub(in crate::ui) fn notches_on(&self, width: u32, height: u32, surface: Rgba) -> Arc<Image> {
        let fill = hex(surface);
        let key = format!("voice-notches:{width}:{height}:{fill}");
        if let Some(found) = self.generated_images.borrow().get(&key) {
            return found.clone();
        }
        let made = Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            notch_svg(width, height, &fill).into_bytes(),
        ));
        self.generated_images.borrow_mut().insert(key, made.clone());
        made
    }

    /// Called every frame: while the page shows, the microphone preview runs (outside a call, or for Mic Test) and
    /// the page repaints about every 20 ms, the official meters' event rate. Both stop when it goes.
    pub(in crate::ui) fn sync_voice_page(&mut self, cx: &mut Context<Self>) {
        let visible = self.voice_page_visible();
        if visible && !self.voice.page_shown {
            self.voice.devices = Devices::read();
            self.video_background_page_shown();
        }
        if !visible && self.voice.page_shown {
            // `camera_background_setting` unmounts: what it picked is saved (`_C`, module 352430).
            self.video_background_page_left(cx);
        }
        self.voice.page_shown = visible;
        if !visible {
            if self.voice.mic_test.is_some() {
                self.stop_mic_test(cx);
            }
            self.voice.recording = false;
            self.voice.device_open = None;
            self.voice.camera.open = false;
            self.voice_sync_select_flag();
            // The incoming-call card's camera preview runs without this page.
            if self.voice.camera.test.is_some() && self.dmc.incoming_preview.is_none() {
                self.camera_test_stop(cx);
            }
            // A drag of the marked sliders of the other pages (text size, spacing, zoom...) is not the voice page's.
            if self
                .voice
                .drag
                .as_ref()
                .is_some_and(|drag| super::controls::spec(drag.key).is_none())
            {
                self.voice.drag = None;
            }
        }
        let wanted = visible && (self.call.is_none() || self.voice.mic_test.is_some());
        if !wanted {
            self.voice.preview = None;
        } else if self.voice.preview.is_none()
            && self
                .voice
                .preview_failed
                .is_none_or(|at| at.elapsed() > Duration::from_secs(2))
        {
            let opened = if crate::inert::skip("microphone preview") {
                Err("inert".to_owned())
            } else {
                dc_voice::preview(self.voice.mic_test.is_some()).map_err(|error| error.to_string())
            };
            match opened {
                Ok(preview) => {
                    self.voice.preview = Some(preview);
                    self.voice.preview_failed = None;
                }
                Err(error) => {
                    tracing::warn!("microphone preview: {error}");
                    self.voice.preview_failed = Some(Instant::now());
                }
            }
        }
        if visible && self.voice.repaint.is_none() {
            self.voice.repaint = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(20))
                        .await;
                    let going = this
                        .update(cx, |this, cx| this.voice_tick(cx))
                        .unwrap_or(false);
                    if !going {
                        break;
                    }
                }
            }));
        }
    }

    /// One repaint of the page: Mic Test's silence watch and auto stop, then a new frame for the meters.
    fn voice_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.voice_page_visible() {
            self.voice.repaint = None;
            return false;
        }
        let connected = self.call.as_ref().is_some_and(|call| call.connected);
        // `S && e && !d && _micTestStop()`: testing in a call that is no longer deafened.
        if self.voice.mic_test.is_some() && connected && !self.self_deaf {
            self.stop_mic_test(cx);
        }
        if let Some(test) = self.voice.mic_test.as_mut() {
            let (level, _) = dc_voice::prefs().processing.meter();
            if level <= dc_voice::FLOOR_DB {
                let since = *test.silent_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(2000) {
                    test.detecting = false;
                }
            } else {
                test.silent_since = None;
                test.detecting = true;
            }
        }
        cx.notify();
        true
    }

    fn start_mic_test(&mut self, cx: &mut Context<Self>) {
        let connected = self.call.as_ref().is_some_and(|call| call.connected);
        let deafened = connected && !self.self_deaf;
        if deafened {
            self.toggle_self_deaf(cx);
        }
        self.voice.mic_test = Some(MicTest {
            started: Instant::now(),
            deafened,
            silent_since: None,
            detecting: true,
        });
        match &self.voice.preview {
            Some(preview) => preview.set_loopback(true),
            None => self.sync_voice_page(cx),
        }
        cx.notify();
    }

    fn stop_mic_test(&mut self, cx: &mut Context<Self>) {
        let Some(test) = self.voice.mic_test.take() else {
            return;
        };
        tracing::debug!(
            seconds = test.started.elapsed().as_secs(),
            "mic test stopped"
        );
        if let Some(preview) = &self.voice.preview {
            preview.set_loopback(false);
        }
        if self.call.is_some() {
            // The call's own worker feeds the meters again.
            self.voice.preview = None;
        }
        if test.deafened && self.call.is_some() && self.self_deaf {
            self.toggle_self_deaf(cx);
        }
        cx.notify();
    }

    /// Key down anywhere in the window. The keybind recorder takes it while recording (true: nobody else does);
    /// otherwise it counts toward push to talk.
    pub(in crate::ui) fn voice_key_down(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.clips_key_down(keystroke, cx) || self.kb_key_down(keystroke, cx) {
            return true;
        }
        // The modifiers held with the key count as pressed before it, in case their own change never came.
        if keystroke.modifiers != self.voice.modifiers {
            self.voice_modifiers(keystroke.modifiers, cx);
        }
        let key = keystroke.key.as_str();
        let code = windows_code(key);
        if self.voice.recording {
            // `esc` in a combo ends recording without saving it.
            if key == "escape" {
                self.voice.recording = false;
                self.voice.recording_held.clear();
            } else if let Some(code) = code {
                self.record_part(KEYBOARD_KEY, code);
            }
            cx.notify();
            return true;
        }
        if let Some(code) = code
            && !self.voice.held.contains(&code)
        {
            self.voice.held.push(code);
            self.update_push_to_talk();
        }
        self.kb_window_keys(cx);
        false
    }

    pub(in crate::ui) fn voice_key_up(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) {
        self.clips_key_up(keystroke, cx);
        self.kb_key_up(keystroke);
        if let Some(code) = windows_code(&keystroke.key) {
            self.release_recorded(KEYBOARD_KEY, code);
            self.voice.held.retain(|held| *held != code);
        }
        // Modifiers let go with the key (their own change may never come).
        if keystroke.modifiers != self.voice.modifiers {
            self.voice_modifiers(keystroke.modifiers, cx);
        }
        self.update_push_to_talk();
        self.kb_window_keys(cx);
        cx.notify();
    }

    /// The desktop recorder takes one combo (`handleNativeChange` then `recordEnd`): it ends when the last of its
    /// keys is let go.
    fn release_recorded(&mut self, kind: u32, code: u32) {
        let before = self.voice.recording_held.len();
        self.voice
            .recording_held
            .retain(|held| *held != [kind, code]);
        if self.voice.recording && before > 0 && self.voice.recording_held.is_empty() {
            self.voice.recording = false;
        }
    }

    /// gpui reports modifier keys only as a change of the set held.
    pub(in crate::ui) fn voice_modifiers(&mut self, modifiers: Modifiers, cx: &mut Context<Self>) {
        self.clips_modifiers(modifiers, cx);
        let previous = self.voice.modifiers;
        self.kb_modifiers(&previous, &modifiers, cx);
        let before = modifier_codes(&self.voice.modifiers);
        let after = modifier_codes(&modifiers);
        self.voice.modifiers = modifiers;
        for code in &before {
            if !after.contains(code) {
                self.release_recorded(KEYBOARD_KEY, *code);
                self.voice.held.retain(|held| held != code);
            }
        }
        for code in after {
            if !before.contains(&code) {
                if self.voice.recording {
                    self.record_part(KEYBOARD_KEY, code);
                    cx.notify();
                } else if !self.voice.held.contains(&code) {
                    self.voice.held.push(code);
                }
            }
        }
        self.update_push_to_talk();
        self.kb_window_keys(cx);
    }

    /// Push to talk is the voice mode now.
    pub(in crate::ui) fn kb_push_to_talk(&self) -> bool {
        self.voice.stored.effective().push_to_talk
    }

    /// The modifier keys down as the window last saw them.
    pub(in crate::ui) fn voice_modifiers_now(&self) -> Modifiers {
        self.voice.modifiers
    }

    /// The codes of the keys down in the window.
    pub(in crate::ui) fn kb_held(&self) -> Vec<u32> {
        self.voice.held.clone()
    }

    /// Another keybind recorder took over: this one stops.
    pub(in crate::ui) fn voice_stop_recording(&mut self) {
        self.voice.recording = false;
        self.voice.recording_held.clear();
    }

    /// `TOGGLE_VOICE_MODE`: push to talk and voice activity swap (`setMode` with the other mode).
    pub(in crate::ui) fn kb_toggle_voice_mode(&mut self) {
        let push_to_talk = self.voice.stored.effective().push_to_talk;
        self.voice.stored.set_mode(!push_to_talk, |_| {});
        self.voice_commit();
    }

    /// A key went down while recording: the combo grows and is saved at once, as each `onChange` calls
    /// `setMode(PUSH_TO_TALK, {shortcut})`.
    fn record_part(&mut self, kind: u32, code: u32) {
        if self.voice.recording_held.contains(&[kind, code]) {
            return;
        }
        self.voice.recording_held.push([kind, code]);
        let combo: Vec<[u32; 3]> = self
            .voice
            .recording_held
            .iter()
            .map(|[kind, code]| [*kind, *code, WINDOWS])
            .collect();
        self.voice
            .stored
            .set_mode(true, |stored| stored.shortcut = combo);
        self.voice_commit();
    }

    /// Push to talk is held while every key of the keybind is down (in this window; the system-wide hook is
    /// another task).
    fn update_push_to_talk(&mut self) {
        // While the system-wide hook runs it reports the keybind itself, with the window's keys included.
        if self.ptt_hook.is_some() {
            self.sync_ptt_hook();
            return;
        }
        let stored = &self.voice.stored;
        let held =
            stored.effective().push_to_talk && shortcut_down(&stored.shortcut, &self.voice.held);
        set_push_to_talk_held(held);
        self.ptt_held_changed(held);
        self.sync_ptt_hook();
    }

    /// The official native input numbers mouse buttons from 1 (the primary button is never a keybind part): right 1,
    /// middle 2, back 3, forward 4. The numbers beyond the primary one are not in the dumps; they follow the order the
    /// recorder shows (`mouse4` is the fourth).
    pub(in crate::ui) fn mouse_code(button: MouseButton) -> Option<u32> {
        use gpui::NavigationDirection::{Back, Forward};
        match button {
            MouseButton::Left => None,
            MouseButton::Right => Some(1),
            MouseButton::Middle => Some(2),
            MouseButton::Navigate(Back) => Some(3),
            MouseButton::Navigate(Forward) => Some(4),
        }
    }

    /// A mouse button pressed or released anywhere in the window. While recording it becomes part of the keybind (true:
    /// nobody else takes it).
    pub(in crate::ui) fn voice_mouse(
        &mut self,
        button: MouseButton,
        down: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.clips_mouse(button, down, cx) || self.kb_mouse(button, down, cx) {
            return true;
        }
        let Some(code) = Self::mouse_code(button) else {
            return false;
        };
        if down && self.voice.recording {
            self.record_part(MOUSE_BUTTON, code);
            cx.notify();
            return true;
        }
        if !down {
            self.release_recorded(MOUSE_BUTTON, code);
            self.sync_ptt_hook();
            cx.notify();
        }
        false
    }

    /// The push to talk state changed (from the window's keys or the hook): its sound, inside a call.
    pub(in crate::ui) fn ptt_held_changed(&mut self, held: bool) {
        if std::mem::replace(&mut self.ptt_down, held) == held || self.call.is_none() {
            return;
        }
        let push_to_talk = self.voice.stored.effective().push_to_talk;
        if let Some(sound) =
            crate::ui::voice_sounds::ptt_sound(held, push_to_talk, self.self_mute || self.self_deaf)
        {
            crate::sounds::play_event(sound);
        }
    }

    /// The system-wide hook runs only while connected to voice in push to talk mode with a keybind, and not while the
    /// keybind is being recorded. It never runs in harness runs (nothing there may read the real keyboard).
    pub(in crate::ui) fn sync_ptt_hook(&mut self) {
        let stored = &self.voice.stored;
        let wanted = !storage::is_capture_run()
            && !self.voice.recording
            && self.call.as_ref().is_some_and(|call| call.connected)
            && stored.effective().push_to_talk
            && !stored.shortcut.is_empty();
        if !wanted {
            // Stopping lets go of push to talk (`on_change(false)`).
            if let Some(hook) = self.ptt_hook.take() {
                hook.stop();
                self.ptt_held_changed(false);
            }
            return;
        }
        let shortcut = stored.shortcut.clone();
        #[cfg(windows)]
        match &self.ptt_hook {
            Some(hook) => hook.set_shortcut(shortcut),
            None => {
                let updates = self.updates.clone();
                match dc_hotkey::start(shortcut, move |held| {
                    set_push_to_talk_held(held);
                    let _ = updates.send(crate::net::Update::PushToTalk(held));
                }) {
                    Ok(hook) => self.ptt_hook = Some(hook),
                    Err(error) => tracing::warn!("push to talk hook not started: {error}"),
                }
            }
        }
        #[cfg(not(windows))]
        let _ = shortcut;
    }

    /// The window lost the focus with no hook to see the keys: nothing may stay held.
    pub(in crate::ui) fn release_push_to_talk_on_blur(&mut self) {
        if self.ptt_hook.is_some() {
            return;
        }
        self.voice.held.clear();
        set_push_to_talk_held(false);
        self.ptt_held_changed(false);
    }

    // ----- rows -----

    /// The rows this module draws for a `Node::Custom`, or None for others.
    pub(super) fn voice_custom(
        &self,
        key: &'static str,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        Some(match key {
            INPUT_SELECT => self.device_select(Side::Input, width, cx),
            OUTPUT_SELECT => self.device_select(Side::Output, width, cx),
            MIC_TEST => self.mic_test_row(width, window, cx),
            KEYBIND => self.keybind_row(width, cx),
            "camera_video_preview" => self.camera_preview_row(cx),
            "camera_selection_setting" => self.camera_select_row(false, cx),
            "camera_background_setting" => self.video_background_row(width, cx),
            RELEASE_DELAY => self.slider_row(
                RELEASE_DELAY,
                dc_intl::tr("Push to Talk Release Delay"),
                &[],
                width,
                cx,
            ),
            _ => return None,
        })
    }

    /// Microphone Volume, Speaker Volume, Soundboard Volume and the release delay: the generic slider (528689 `z`).
    pub(super) fn slider_row(
        &self,
        key: &'static str,
        label: &'static str,
        description: Runs,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fraction = self.slider_fraction(key);
        // The label block and the slider 8px under it (`labelContainer_VhFsFg`'s margin).
        let head = if key == SOUNDBOARD_VOLUME {
            // Its description ends in a link to the Soundboard article.
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(label_text(label))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(self.text_with_link(
                            key,
                            "Control how loud sounds are for you personally. ",
                            "Learn more about Soundboard volume.",
                            "",
                            cx,
                            |_, cx| cx.open_url(SOUNDBOARD_GUIDE),
                        )),
                )
                .into_any_element()
        } else if description.is_empty() {
            label_text(label).into_any_element()
        } else {
            super::render::label_block(label, false, description).into_any_element()
        };
        div()
            .min_h(px(48.))
            .py(px(8.))
            .flex()
            .flex_col()
            .child(div().mb(px(8.)).child(head))
            .child(self.slider(
                key,
                fraction,
                width,
                color("--slider-track-background"),
                color("--background-brand"),
                None,
                false,
                cx,
            ))
            .into_any_element()
    }

    /// A slider of the generated pages with its marks (`sN`): the label, the marks above the bar, and a bar that
    /// drags, snaps to the markers and answers the arrow keys. The bar's area is its own probe, so the drag maths of
    /// the voice sliders (5px of grabber inset each side) holds here too.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn marked_slider_row(
        &self,
        key: &'static str,
        label: &'static str,
        badge: bool,
        description: Runs,
        marks: &'static [(&'static str, f32)],
        _default_mark: Option<usize>,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fraction = self.slider_fraction(key);
        // The marker the grabber rests on is the one drawn green (`markValue` of the selected value).
        let current = marks
            .iter()
            .position(|(_, at)| (at - fraction).abs() < 0.002);
        let dragging = self.voice.drag.as_ref().is_some_and(|drag| drag.key == key);
        let focus = self
            .voice
            .slider_focus
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let bar = div()
            .id(SharedString::from(format!("voice-slider-{key}")))
            .absolute()
            .top(px(24.))
            .left_0()
            .w_full()
            .h(px(24.))
            .cursor_pointer()
            .track_focus(&focus)
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                let direction = match event.keystroke.key.as_str() {
                    "left" | "down" => -1,
                    "right" | "up" => 1,
                    "home" => -1_000_000,
                    "end" => 1_000_000,
                    _ => return,
                };
                this.nudge_slider(key, direction, cx);
                cx.stop_propagation();
            }))
            .child(self.anchor_probe(format!("voice-slider-{key}")))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.show_slider_bubble(key);
                } else if this.voice.drag.is_none() {
                    this.hide_tip(&format!("voice-bubble-{key}"));
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if let Some(handle) = this.voice.slider_focus.borrow().get(key) {
                        window.focus(handle, cx);
                    }
                    this.start_slider_drag(key, event.position.x, cx);
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.dragging() && this.voice.drag.as_ref().is_some_and(|drag| drag.key == key)
                {
                    this.move_slider_drag(event.position.x, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_slider_drag(cx)),
            )
            .when(dragging, |this| this.child(self.drag_listener()));
        div()
            .min_h(px(48.))
            .py(px(8.))
            .flex()
            .flex_col()
            .child(super::render::label_block(label, badge, description).mb(px(8.)))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(48.))
                    .child(super::render::slider(marks, current, fraction, width))
                    .child(bar),
            )
            .into_any_element()
    }

    /// Where a slider is, 0 to 1: its drag while dragged, else the stored value.
    fn slider_fraction(&self, key: &'static str) -> f32 {
        if let Some(drag) = self.voice.drag.as_ref().filter(|drag| drag.key == key) {
            return drag.fraction;
        }
        let prefs = dc_voice::prefs();
        match key {
            INPUT_VOLUME => volume_to_slider(prefs.input_gain() * 100.) / 100.,
            OUTPUT_VOLUME => volume_to_slider(prefs.output_gain() * 100.) / 200.,
            RELEASE_DELAY => self.voice.stored.delay / 2000.,
            SOUNDBOARD_VOLUME => volume_to_slider(soundboard_volume()) / 100.,
            SENSITIVITY => (self.voice.stored.threshold + 100.) / 100.,
            extra::ATTENUATION => self.attenuation_fraction(),
            extra::STREAM_ATTEN_STRENGTH => self.stream_attenuation_fraction(),
            _ => super::controls::spec(key)
                .map_or(0., |spec| spec.fraction_of(self.slider_value(spec))),
        }
        .clamp(0., 1.)
    }

    /// The bubble over a slider's grabber.
    fn slider_bubble(&self, key: &'static str, fraction: f32) -> String {
        match key {
            INPUT_VOLUME => format!("{}%", to_fixed(fraction * 100., 0)),
            OUTPUT_VOLUME => format!("{}%", to_fixed((fraction * 200.).round(), 0)),
            RELEASE_DELAY => delay_bubble((fraction * 2000.).round()),
            SOUNDBOARD_VOLUME => format!("{}%", to_fixed((fraction * 100.).round(), 0)),
            SENSITIVITY => sensitivity_bubble(fraction * 100.),
            // The default `formatValue`: the position as a percentage.
            extra::ATTENUATION => format!("{}%", to_fixed(fraction * 100., 0)),
            extra::STREAM_ATTEN_STRENGTH => format!("{}%", to_fixed(fraction * 99. + 1., 0)),
            _ => super::controls::spec(key)
                .map(|spec| (spec.format)(spec.value_at(fraction)))
                .unwrap_or_default(),
        }
    }

    /// A slider: the bar with its fill to the grabber, the grabber, and (for sensitivity) the live level darkening
    /// the bar from the left. Hovered or dragged, the bar is 6px and the grabber 20px (205513 `M`).
    #[allow(clippy::too_many_arguments)]
    fn slider(
        &self,
        key: &'static str,
        fraction: f32,
        width: f32,
        bar: Rgba,
        fill: Rgba,
        level: Option<f32>,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dragging = self.voice.drag.as_ref().is_some_and(|drag| drag.key == key);
        let active = !disabled && (dragging || self.voice.hovered_slider == Some(key));
        let bar_height = if active { 6. } else { 4. };
        let grabber = if active { 20. } else { 16. };
        let track = width - 10.;
        let bar_top = 12. - bar_height / 2.;
        let strip = || {
            div()
                .absolute()
                .top(px(bar_top))
                .left_0()
                .w_full()
                .h(px(bar_height))
                .rounded(px(4.))
                .overflow_hidden()
        };
        let focus = self
            .voice
            .slider_focus
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        div()
            .id(SharedString::from(format!("voice-slider-{key}")))
            .relative()
            .h(px(24.))
            .w_full()
            .track_focus(&focus)
            .when(!disabled, |this| {
                this.on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                    // One step per press: left/down toward the start, right/up toward the end.
                    let direction = match event.keystroke.key.as_str() {
                        "left" | "down" => -1,
                        "right" | "up" => 1,
                        // Home and End go to the first and last stop; `nudge_slider` clamps.
                        "home" => -1_000_000,
                        "end" => 1_000_000,
                        _ => return,
                    };
                    this.nudge_slider(key, direction, cx);
                    cx.stop_propagation();
                }))
            })
            .when(disabled, |this| this.opacity(0.6))
            .child(self.anchor_probe(format!("voice-slider-{key}")))
            .child(
                strip().bg(bar).child(
                    div()
                        .h_full()
                        .w(px(width * fraction))
                        .border_1()
                        .border_color(gpui::hsla(0., 0., 1., 0.1))
                        .bg(fill),
                ),
            )
            .children(level.map(|level| {
                strip().child(
                    div()
                        .h_full()
                        .w(px(width * level))
                        .bg(color("--opacity-black-24")),
                )
            }))
            .child(
                div()
                    .id(SharedString::from(format!("voice-grabber-{key}")))
                    .absolute()
                    .top(px(12. - grabber / 2.))
                    .left(px(5. + track * fraction - grabber / 2.))
                    .size(px(grabber))
                    .rounded_full()
                    .border_1()
                    .border_color(color("--border-normal"))
                    .bg(gpui::white())
                    .when(!disabled, |this| {
                        this.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.show_slider_bubble(key);
                            } else if this.voice.drag.is_none() {
                                this.hide_tip(&format!("voice-bubble-{key}"));
                            }
                            cx.notify();
                        }))
                    }),
            )
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.voice.hovered_slider = Some(key);
                        } else if this.voice.hovered_slider == Some(key) {
                            this.voice.hovered_slider = None;
                        }
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            if let Some(handle) = this.voice.slider_focus.borrow().get(key) {
                                window.focus(handle, cx);
                            }
                            this.start_slider_drag(key, event.position.x, cx);
                        }),
                    )
                    // Moves and the release over the slider itself, before a frame has put the window-wide
                    // listener in place.
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        if event.dragging()
                            && this.voice.drag.as_ref().is_some_and(|drag| drag.key == key)
                        {
                            this.move_slider_drag(event.position.x, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.end_slider_drag(cx)),
                    )
            })
            .when(dragging, |this| this.child(self.drag_listener()))
            .into_any_element()
    }

    /// While a slider is dragged, moves and the release anywhere in the window reach it.
    fn drag_listener(&self) -> impl IntoElement {
        let weak = self.weak.clone();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moved = weak.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Bubble && event.dragging() {
                        let _ = moved.update(cx, |this, cx| {
                            this.move_slider_drag(event.position.x, cx);
                        });
                    }
                });
                let released = weak.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Bubble && event.button == MouseButton::Left {
                        let _ = released.update(cx, |this, cx| this.end_slider_drag(cx));
                    }
                });
            },
        )
        .absolute()
        .w(px(0.))
        .h(px(0.))
    }

    fn slider_grabber_bounds(&self, key: &'static str) -> Option<Bounds<Pixels>> {
        let bounds = self
            .tip_anchors
            .borrow()
            .get(&SharedString::from(format!("voice-slider-{key}")))
            .copied()?;
        let fraction = self.slider_fraction(key);
        let track = f32::from(bounds.size.width) - 10.;
        let center = point(
            bounds.left() + px(5. + track * fraction),
            bounds.top() + px(12.),
        );
        Some(Bounds::centered_at(center, size(px(20.), px(20.))))
    }

    fn show_slider_bubble(&mut self, key: &'static str) {
        let Some(anchor) = self.slider_grabber_bounds(key) else {
            return;
        };
        let text = self.slider_bubble(key, self.slider_fraction(key));
        self.show_tip(
            format!("voice-bubble-{key}"),
            tooltip::Tip::body(tooltip::Body::Text(text.into()), tooltip::Side::Top),
            anchor,
        );
    }

    fn start_slider_drag(&mut self, key: &'static str, x: Pixels, cx: &mut Context<Self>) {
        let Some(bounds) = self
            .tip_anchors
            .borrow()
            .get(&SharedString::from(format!("voice-slider-{key}")))
            .copied()
        else {
            return;
        };
        self.voice.drag = Some(Drag {
            key,
            bounds,
            fraction: 0.,
        });
        self.move_slider_drag(x, cx);
    }

    fn move_slider_drag(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(drag) = self.voice.drag.as_mut() else {
            return;
        };
        let track = f32::from(drag.bounds.size.width) - 10.;
        let mut fraction = ((f32::from(x - drag.bounds.left()) - 5.) / track).clamp(0., 1.);
        // The new slider moves in steps of 1 (310592 `step`); the sensitivity slider drags smoothly.
        let marked = super::controls::spec(drag.key);
        if let Some(spec) = marked {
            // `stickToMarkers`: the grabber rests on a marker.
            fraction = spec.fraction_of(spec.value_at(fraction));
        } else if drag.key != SENSITIVITY {
            let steps = slider_steps(drag.key);
            fraction = (fraction * steps).round() / steps;
        }
        drag.fraction = fraction;
        let key = drag.key;
        if let Some(spec) = marked {
            // `asValueChanges`: the text size, spacing, zoom, saturation and the toast time follow the drag.
            self.setting_choices
                .insert(key, Choice::Number(spec.value_at(fraction)));
        }
        // `asValueChanges`: the volumes follow the drag.
        store_slider_volume(key, fraction);
        self.show_slider_bubble(key);
        cx.notify();
    }

    /// An arrow key on a focused slider: one step (the slider's `step`, 310592) toward the end the arrow points to.
    pub(in crate::ui) fn nudge_slider(
        &mut self,
        key: &'static str,
        direction: i32,
        cx: &mut Context<Self>,
    ) {
        if let Some(spec) = super::controls::spec(key) {
            let value = spec.step(self.slider_value(spec), direction);
            self.slider_committed(key, value);
            self.setting_applied(key);
            cx.notify();
            return;
        }
        let steps = slider_steps(key);
        let current = (self.slider_fraction(key) * steps).round();
        let fraction = ((current + direction as f32).clamp(0., steps)) / steps;
        store_slider_volume(key, fraction);
        self.commit_slider(key, fraction);
        cx.notify();
    }

    /// The value a slider writes when it is let go (or moved by a key): the delay and the sensitivity.
    fn commit_slider(&mut self, key: &'static str, fraction: f32) {
        match key {
            RELEASE_DELAY => {
                let delay = (fraction * 2000.).round();
                self.voice
                    .stored
                    .set_mode(true, |stored| stored.delay = delay);
                self.voice_commit();
            }
            SENSITIVITY => {
                let threshold = fraction * 100. - 100.;
                let mode = self.voice.stored.effective().push_to_talk;
                self.voice
                    .stored
                    .set_mode(mode, |stored| stored.threshold = threshold);
                self.voice_commit();
            }
            extra::ATTENUATION => self.commit_attenuation(fraction),
            extra::STREAM_ATTEN_STRENGTH => {
                self.commit_stream_attenuation(fraction);
                self.apply_stream_attenuation();
            }
            _ => {
                if let Some(spec) = super::controls::spec(key) {
                    self.slider_committed(key, spec.value_at(fraction));
                    self.setting_applied(key);
                }
            }
        }
    }

    /// `onChangeEnd` / `onValueChange`: the delay and the sensitivity are written once, on release.
    fn end_slider_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.voice.drag.take() else {
            return;
        };
        self.commit_slider(drag.key, drag.fraction);
        self.hide_tip(&format!("voice-bubble-{}", drag.key));
        cx.notify();
    }

    /// `voice_input_sensitivity_field_set`: the switch, then the automatic meter or the manual slider (`rK`, `rZ`).
    pub(super) fn sensitivity_row(
        &self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let effective = self.voice.stored.effective();
        let disabled = effective.push_to_talk;
        let automatic = self.voice.stored.auto_threshold;
        let description: Runs = if automatic {
            &[(
                "Controls how much sound Discord transmits from your mic. Speak to test your mic \u{2014} if the meter is filled, Discord is transmitting your beautiful voice.",
                false,
            )]
        } else {
            &[(
                "Controls how much sound Discord transmits from your mic.",
                false,
            )]
        };
        let switch = self.switch_row(
            SENSITIVITY,
            dc_intl::tr("Automatically Adjust Input Sensitivity"),
            false,
            description,
            automatic,
            disabled,
            width,
            window,
            cx,
        );
        let (level, voice) = dc_voice::prefs().processing.meter();
        let lower = if automatic {
            // `.inputSensitivityBar`: 8px, primary-400, green-230 while the engine sends voice.
            div()
                .h(px(40.))
                .child(
                    div()
                        .h(px(8.))
                        .rounded(px(4.))
                        .bg(color(if voice && !disabled {
                            "--green-230"
                        } else {
                            "--primary-400"
                        }))
                        .when(disabled, |this| this.opacity(0.6)),
                )
                .into_any_element()
        } else {
            let shown = if disabled {
                0.
            } else {
                ((level + 100.) / 100.).clamp(0., 1.)
            };
            self.slider(
                SENSITIVITY,
                self.slider_fraction(SENSITIVITY),
                width,
                color("--green-360"),
                color("--yellow-300"),
                Some(shown),
                disabled,
                cx,
            )
        };
        div()
            .py(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(switch)
            .child(lower)
            .into_any_element()
    }

    /// `voice_microphone_input_select` / `voice_speakers_output_select` (625841 with 74848's options).
    fn device_select(&self, side: Side, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let devices = &self.voice.devices;
        let prefs = dc_voice::prefs();
        let (label, list, default, chosen, icon) = match side {
            Side::Input => (
                dc_intl::tr("Microphone"),
                &devices.inputs,
                &devices.default_input,
                prefs.input_device(),
                "voice_select_mic",
            ),
            Side::Output => (
                dc_intl::tr("Speaker"),
                &devices.outputs,
                &devices.default_output,
                prefs.output_device(),
                "voice_select_headphones",
            ),
        };
        // "default" first (named "Windows Default", the default device's name under it), then each device.
        let mut options: Vec<(String, Option<String>, Option<String>)> = vec![(
            dc_intl::tr("Windows Default").to_owned(),
            default
                .as_ref()
                .map(|name| audio_menu::split_device(name).0),
            None,
        )];
        for device in list {
            let (name, kind) = audio_menu::split_device(device);
            options.push((name, kind, Some(device.clone())));
        }
        let selected = options
            .iter()
            .position(|(_, _, device)| *device == chosen && chosen.is_some())
            .unwrap_or(0);
        let (name, sub, _) = options[selected].clone();
        let open = self.voice.device_open == Some(side);
        let field = div()
            .id(SharedString::from(format!("voice-device-{side:?}")))
            .h(px(40.))
            .w_full()
            .pl(px(12.))
            .pr(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(8.))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.voice.device_open == Some(side) {
                    this.voice.device_open = None;
                } else {
                    this.voice.devices = Devices::read();
                    this.voice.device_open = Some(side);
                }
                this.voice_sync_select_flag();
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .whitespace_nowrap()
                    .child(mask_icon(icon, 20., "--text-default"))
                    .child(
                        div()
                            .ml(px(8.))
                            .text_color(color("--text-default"))
                            .child(name),
                    )
                    .children(sub.map(|sub| {
                        div()
                            .ml(px(4.))
                            .text_color(color("--text-muted"))
                            .child(format!("({sub})"))
                    })),
            )
            .child(
                mask_icon("chevron_down", 18., "--text-default").when(open, |this| {
                    this.with_transformation(gpui::Transformation::rotate(gpui::percentage(0.5)))
                }),
            );
        let rows: Vec<AnyElement> = if open {
            options
                .into_iter()
                .enumerate()
                .map(|(index, (name, sub, device))| {
                    let picked = index == selected;
                    let tint = if picked {
                        "--interactive-text-active"
                    } else {
                        "--text-subtle"
                    };
                    div()
                        .id(SharedString::from(format!("voice-device-{side:?}-{index}")))
                        .h(px(64.))
                        .p(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .when(picked, |this| {
                            this.bg(color("--interactive-background-selected"))
                        })
                        .hover(|style| style.bg(color("--interactive-background-hover")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_device(side, device.clone(), cx);
                            this.voice.device_open = None;
                            this.voice_sync_select_flag();
                            cx.notify();
                        }))
                        .child(mask_icon(icon, 20., tint))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .font_weight(FontWeight(500.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-subtle"))
                                        .child(name),
                                )
                                .children(sub.map(|sub| {
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(sub)
                                })),
                        )
                        .when(picked, |this| {
                            this.child(
                                div()
                                    .relative()
                                    .size(px(20.))
                                    .child(
                                        gpui::svg()
                                            .path("icons/theme_check_disc.svg")
                                            .absolute()
                                            .size(px(20.))
                                            .text_color(gpui::white()),
                                    )
                                    .child(
                                        mask_icon("circle_check", 20., "--brand-500").absolute(),
                                    ),
                            )
                        })
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };
        let _ = width;
        div()
            .py(px(8.))
            .flex()
            .flex_col()
            .child(label_text(label).mb(px(8.)))
            .child(div().relative().child(field).when(open, |this| {
                this.child(
                    deferred(
                        super::super::keyboard::outside(
                            div().id(SharedString::from(format!("voice-device-list-{side:?}"))),
                            super::super::keyboard::Layer::SettingsSelect,
                            cx,
                        )
                        .occlude()
                        .absolute()
                        .top(px(48.))
                        .left_0()
                        .right_0()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .overflow_hidden()
                        .child(div().flex().flex_col().children(rows)),
                    )
                    .with_priority(2),
                )
            }))
            .into_any_element()
    }

    /// `voice_microphone_test_setting` (782600 `U`, size `md`): the button, the notched meter, the caption under it
    /// while testing, and the help line.
    fn mic_test_row(&self, width: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let testing = self.voice.mic_test.is_some();
        let detecting = self
            .voice
            .mic_test
            .as_ref()
            .is_none_or(|test| test.detecting);
        // The hidden `buttonSizer` sizes the button to the longer string.
        let button_width = f32::from(
            super::render::measure_text(window, dc_intl::tr("Stop Testing"), 16., 500.).max(
                super::render::measure_text(window, dc_intl::tr("Mic Test"), 16., 500.),
            ),
        )
        .ceil()
            + 32.;
        let column = width - button_width - 16.;
        let meter_width = to_eight(column);
        let (level, _) = dc_voice::prefs().processing.meter();
        let progress = if testing {
            (level + 100.).clamp(0., 100.)
        } else {
            0.
        };
        let shift = (to_eight(meter_width * (100. - progress) / 100.) - meter_width).abs();
        let notches = self.notch_image(meter_width as u32, 28);
        let button = div()
            .id("voice-mic-test")
            .w(px(button_width))
            .on_click(cx.listener(|this, _, _, cx| {
                if this.voice.mic_test.is_some() {
                    this.stop_mic_test(cx);
                } else {
                    this.start_mic_test(cx);
                }
            }))
            .child(super::button(
                "voice-mic-test-button",
                if testing {
                    dc_intl::tr("Stop Testing")
                } else {
                    dc_intl::tr("Mic Test")
                },
                super::model::ButtonKind::Primary,
            ));
        let connected = self.call.as_ref().is_some_and(|call| call.connected);
        let button: AnyElement = if connected && !testing {
            self.with_tip(
                "voice-mic-test-tip",
                tooltip::Tip::text(
                    dc_intl::tr(
                        "You'll be muted and deafened in your voice session during Mic Testing.",
                    ),
                    tooltip::Side::Top,
                ),
                button,
                cx,
            )
            .into_any_element()
        } else {
            button.into_any_element()
        };
        let meter = div()
            .relative()
            .w(px(meter_width))
            .h(px(28.))
            .overflow_hidden()
            .when(progress > 0., |this| {
                this.bg(linear_gradient(
                    90.,
                    linear_color_stop(color("--yellow-260"), 0.),
                    linear_color_stop(color("--green-360"), 1.),
                ))
            })
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(shift))
                    .w(px(meter_width))
                    .h_full()
                    .bg(color("--gradient-progress-pill-background")),
            )
            .child(
                img(notches)
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(meter_width))
                    .h(px(28.)),
            );
        let caption = if testing && detecting {
            div()
                .child(dc_intl::tr("Playing back your beautiful voice"))
                .into_any_element()
        } else if testing {
            let text =
                dc_intl::tr("Please check that you\u{2019}ve selected the right input device.");
            div()
                .child(
                    div()
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::tr(
                            "Discord isn\u{2019}t detecting any input from your mic.",
                        )),
                )
                .child(
                    // `micTestHelpLink`: the link 4px after the sentence, about a space at this size.
                    // The sentence ends in the link, `Anchor` to the article `NO_INPUT_DETECTED`.
                    self.text_with_link(
                        "mic-test-help",
                        &format!("{text} "),
                        dc_intl::tr("Help!"),
                        "",
                        cx,
                        |_, cx| cx.open_url(NO_INPUT_HELP),
                    ),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };
        div()
            .py(px(8.))
            .flex()
            .flex_col()
            .child(
                div()
                    .my(px(4.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .child(button)
                            .child(div().w(px(column)).child(meter)),
                    )
                    .child(
                        div()
                            .ml(px(button_width + 16. + 2.))
                            .min_h(px(32.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-subtle"))
                            .child(caption),
                    ),
            )
            .child(
                div()
                    .when(testing && !detecting, |this| this.invisible())
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(self.text_with_link(
                                "mic-test-guide",
                                "Need help? Check out our ",
                                "troubleshooting guide",
                                ".",
                                cx,
                                |_, cx| cx.open_url(TROUBLESHOOTING_GUIDE),
                            )),
                    ),
            )
            .into_any_element()
    }

    fn notch_image(&self, width: u32, height: u32) -> Arc<Image> {
        let fill = hex(color("--background-base-low"));
        let key = format!("voice-notches:{width}:{height}:{fill}");
        if let Some(found) = self.generated_images.borrow().get(&key) {
            return found.clone();
        }
        let made = Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            notch_svg(width, height, &fill).into_bytes(),
        ));
        self.generated_images.borrow_mut().insert(key, made.clone());
        made
    }

    /// `voice_push_to_talk_keybind_setting`: the label with its note, and the key recorder (368484).
    fn keybind_row(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        const RECORDER: f32 = 300.;
        let recording = self.voice.recording;
        let shortcut = &self.voice.stored.shortcut;
        let red = color("--red-400");
        let critical = color("--text-feedback-critical");
        // The placeholder takes `::placeholder`'s normal weight; a keybind is semibold.
        let (text, text_color, weight) = if recording {
            (String::new(), critical, 600.)
        } else if shortcut.is_empty() {
            (
                dc_intl::tr("No Keybind Set").to_owned(),
                color("--input-placeholder-text-default"),
                400.,
            )
        } else {
            (shortcut_text(shortcut), color("--text-strong"), 600.)
        };
        let button = if recording {
            small_button(
                dc_intl::tr("Stop Recording"),
                with_alpha(red, 0.1),
                color("--control-critical-secondary-border-default"),
                critical,
            )
        } else {
            small_button(
                if shortcut.is_empty() {
                    dc_intl::tr("Record Keybind")
                } else {
                    dc_intl::tr("Edit Keybind")
                },
                color("--control-secondary-background-default"),
                color("--control-secondary-border-default"),
                color("--control-secondary-text-default"),
            )
        };
        let recorder = div()
            .id("voice-keybind-recorder")
            .flex_none()
            .w(px(RECORDER))
            .h(px(44.))
            .rounded(px(8.))
            .border_1()
            .bg(color("--input-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            // `.recording`: a red border. Its pulsing 6px glow is left out: gpui paints a shadow under the whole box,
            // and the field's background is translucent, so it would show inside it too.
            .when(recording, |this| this.border_color(with_alpha(red, 0.6)))
            .when(!recording, |this| {
                this.border_color(color("--input-border-default"))
                    .hover(|style| style.border_color(color("--border-strong")))
            })
            .on_click(cx.listener(|this, _, _, cx| {
                this.voice.recording = !this.voice.recording;
                this.voice.recording_held.clear();
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .pl(px(10.))
                    .text_size(px(14.))
                    .line_height(px(22.))
                    .font_weight(FontWeight(weight))
                    .text_color(text_color)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(text),
            )
            .child(div().flex_none().p(px(4.)).child(button));
        div()
            .py(px(8.))
            .flex()
            .gap(px(24.))
            .child(
                div()
                    .w(px(width - 24. - RECORDER))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(label_text(dc_intl::tr("Push to Talk Keybind")))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(self.text_with_link(
                                "ptt-keybind-settings",
                                "You can add multiple Push to Talk binds under ",
                                "Keybind Settings",
                                ".",
                                cx,
                                // `openUserSettings(SYSTEM_CUSTOM_KEYBINDS_CATEGORY)`.
                                |this, cx| {
                                    this.open_settings_category(
                                        dc_intl::tr("System"),
                                        dc_intl::tr("Custom Keybinds"),
                                        cx,
                                    )
                                },
                            )),
                    ),
            )
            .child(recorder)
            .into_any_element()
    }
}

/// How many steps a slider has end to end: the volumes in whole percents, the delay in milliseconds, the sensitivity in
/// decibels (a drag on it is smooth, a key moves one).
fn slider_steps(key: &str) -> f32 {
    match key {
        OUTPUT_VOLUME => 200.,
        RELEASE_DELAY => 2000.,
        _ => 100.,
    }
}

/// The volume sliders write as they move; the others on release.
fn store_slider_volume(key: &str, fraction: f32) {
    match key {
        INPUT_VOLUME => {
            audio_menu::store_gain(Side::Input, slider_to_volume(fraction * 100.) / 100.)
        }
        OUTPUT_VOLUME => {
            audio_menu::store_gain(Side::Output, slider_to_volume(fraction * 200.) / 100.)
        }
        SOUNDBOARD_VOLUME => store_soundboard_volume(fraction),
        _ => {}
    }
}

/// The app-level push to talk switch: held while the keybind is down (`PUSH_TO_TALK_STATE_CHANGE`). Called from key
/// down and up in the window; a system-wide hook can call it too.
pub(in crate::ui) fn set_push_to_talk_held(held: bool) {
    dc_voice::prefs().processing.set_push_to_talk_held(held);
}

impl Root {
    /// A drag's or hover's bubble, through the tooltip layer.
    fn show_tip(&mut self, key: String, tip: tooltip::Tip, anchor: Bounds<Pixels>) {
        self.tip = Some(tooltip::Active::new(key.into(), tip, anchor));
    }

    fn hide_tip(&mut self, key: &str) {
        if self.tip.as_ref().is_some_and(|tip| tip.key() == key) {
            self.tip = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 754712 `e7` over the stored values: Custom shows them, the presets force theirs and keep the rest.
    #[test]
    fn the_soundboard_volume_reads_and_maps_like_the_setting() {
        use serde_json::json;
        // `H()`: the stored volume, 100 when none; kept in range.
        assert_eq!(soundboard_volume_from(None), 100.);
        assert_eq!(soundboard_volume_from(Some(&json!(37.))), 37.);
        assert_eq!(soundboard_volume_from(Some(&json!(250))), 100.);
        assert_eq!(soundboard_volume_from(Some(&json!(-4))), 0.);
        assert_eq!(soundboard_volume_from(Some(&json!("loud"))), 100.);
        // The slider's ends and the curve: position 50 is a volume of 2.8-th power of one half.
        assert_eq!(soundboard_volume_at(0.), 0.);
        assert_eq!(soundboard_volume_at(1.), 100.);
        assert_eq!(soundboard_volume_at(0.5), (0.5f32.powf(2.8) * 100.).round());
        // Position and volume round trip within a step.
        for position in [40., 75., 100.] {
            let volume = soundboard_volume_at(position / 100.);
            assert!(
                (volume_to_slider(volume) - position).abs() < 2.5,
                "{position}"
            );
        }
    }

    /// What the slider stores reads back as the same volume (the file is JSON; the test leaves the owner's storage alone).
    #[test]
    fn the_soundboard_volume_survives_a_json_round_trip() {
        for fraction in [0., 0.25, 0.5, 0.9, 1.] {
            let written = json!(soundboard_volume_at(fraction));
            let file: serde_json::Value = serde_json::from_str(&written.to_string()).unwrap();
            assert_eq!(
                soundboard_volume_from(Some(&file)),
                soundboard_volume_at(fraction)
            );
        }
    }

    #[test]
    fn presets_merge_over_the_stored_values() {
        let stored = Stored {
            push_to_talk: true,
            threshold: -42.,
            auto_threshold: false,
            advanced_voice_activity: false,
            delay: 300.,
            echo: false,
            suppression: 1,
            room_echo: true,
            agc: false,
            ..Stored::default()
        };
        let custom = stored.effective();
        assert!(custom.push_to_talk && !custom.auto_threshold && custom.threshold == -42.);
        assert_eq!(custom.suppression, 1);

        let isolation = Stored {
            profile: Profile::VoiceIsolation,
            ..stored.clone()
        }
        .effective();
        assert!(isolation.push_to_talk, "the mode is kept");
        assert!(isolation.auto_threshold && isolation.advanced_voice_activity);
        assert!(isolation.echo && isolation.agc && !isolation.room_echo);
        assert_eq!(isolation.suppression, 0, "Krisp");
        assert_eq!(isolation.threshold, -42.);
        assert_eq!(isolation.delay, 300.);

        let studio = Stored {
            profile: Profile::Studio,
            ..stored.clone()
        }
        .effective();
        assert!(!studio.push_to_talk, "voice activity is forced");
        assert_eq!(studio.threshold, -84.);
        assert!(!studio.auto_threshold && !studio.advanced_voice_activity);
        assert!(!studio.echo && !studio.agc && !studio.room_echo);
        assert_eq!(studio.suppression, 2, "None");
    }

    /// Switching profiles writes only the profile: back on Custom, the user's values come back.
    #[test]
    fn switching_profiles_keeps_the_custom_values() {
        let mut stored = Stored {
            auto_threshold: false,
            threshold: -30.,
            ..Stored::default()
        };
        stored.profile = Profile::Studio;
        stored.profile = Profile::Custom;
        assert!(!stored.effective().auto_threshold);
        assert_eq!(stored.effective().threshold, -30.);
    }

    /// `setMode` under a preset saves the effective sensitivity into the custom values (the official leak).
    #[test]
    fn a_mode_change_under_a_preset_saves_its_sensitivity() {
        let mut stored = Stored {
            profile: Profile::VoiceIsolation,
            auto_threshold: false,
            advanced_voice_activity: false,
            ..Stored::default()
        };
        stored.set_mode(true, |_| {});
        assert!(stored.push_to_talk);
        assert!(stored.auto_threshold && stored.advanced_voice_activity);
        stored.profile = Profile::Custom;
        assert!(stored.effective().auto_threshold);
    }

    /// 792251's curve: slider 50 is 14.36, 100 is unity, 200 is +6 dB (199.53), and back again.
    #[test]
    fn the_volume_curve_matches_the_bundle() {
        assert!((slider_to_volume(50.) - 14.358).abs() < 0.01);
        assert_eq!(slider_to_volume(100.), 100.);
        assert!((slider_to_volume(200.) - 199.526).abs() < 0.01);
        assert_eq!(slider_to_volume(0.), 0.);
        for position in [1., 25., 50., 99., 100., 150., 200.] {
            let back = volume_to_slider(slider_to_volume(position));
            assert!(
                (back - position).abs() < 0.01,
                "{position} came back as {back}"
            );
        }
        assert_eq!(
            volume_to_slider(100.) / 200.,
            0.5,
            "the speaker grabber sits in the middle"
        );
    }

    /// The sensitivity slider is -100..0 dB on positions 0..100; bubbles as the renderers format them.
    #[test]
    fn bubbles_read_as_the_official_ones() {
        assert_eq!(sensitivity_bubble(40.), "-60dB");
        assert_eq!(sensitivity_bubble(100.), "0dB");
        assert_eq!(sensitivity_bubble(39.5), "-61dB");
        assert_eq!(delay_bubble(20.), "20\u{a0}ms");
        assert_eq!(delay_bubble(1500.), "1.50s");
        assert_eq!(delay_bubble(2000.), "2.00s");
        assert_eq!(to_fixed(-0.2, 0), "0");
    }

    /// `dI(shortcut, true)` with Windows codes, and gpui's key names to those codes.
    #[test]
    fn shortcuts_display_as_the_recorder_shows_them() {
        assert_eq!(shortcut_text(&[[0, 162, 1], [0, 120, 1]]), "CTRL + F9");
        assert_eq!(shortcut_text(&[[0, 192, 1]]), "`");
        assert_eq!(shortcut_text(&[[0, 220, 1], [0, 222, 1]]), "\\ + '");
        assert_eq!(shortcut_text(&[[1, 4, 1]]), "MOUSE4");
        assert_eq!(shortcut_text(&[[0, 9999, 1]]), "UNK9999");
        assert_eq!(shortcut_text(&[]), "");
        assert_eq!(windows_code("f9"), Some(120));
        assert_eq!(windows_code("a"), Some(65));
        assert_eq!(windows_code("escape"), Some(27));
        assert_eq!(windows_code("pageup"), Some(33));
        assert_eq!(windows_code("!"), Some(49));
        assert_eq!(windows_code("`"), Some(192));
        assert_eq!(
            modifier_codes(&Modifiers {
                control: true,
                shift: true,
                ..Modifiers::default()
            }),
            vec![162, 160]
        );
    }

    /// Push to talk holds while all of the keybind is down, in any order, and not for an empty keybind.
    #[test]
    fn push_to_talk_holds_while_the_whole_keybind_is_down() {
        let ctrl_f9 = [[0, 162, 1], [0, 120, 1]];
        assert!(shortcut_down(&ctrl_f9, &[120, 162]));
        assert!(shortcut_down(&ctrl_f9, &[162, 120, 65]));
        assert!(!shortcut_down(&ctrl_f9, &[162]));
        assert!(!shortcut_down(&[], &[162]));
        assert!(
            !shortcut_down(&[[1, 4, 1]], &[4]),
            "a mouse button is not a key"
        );
    }

    /// The advanced accordion's subtitle: three titles under Custom, two under a preset, "and more" past them.
    #[test]
    fn the_collapsed_subtitle_lists_what_shows() {
        let custom = [
            "Automatic Gain Control",
            "Advanced Voice Activity",
            "Bypass System Audio Input Processing",
            "Audio Device Suggestions",
        ];
        assert_eq!(
            collapsed_subtitle(&custom, 3),
            "Automatic Gain Control, Advanced Voice Activity, Bypass System Audio Input Processing, and more"
        );
        assert_eq!(
            collapsed_subtitle(
                &[
                    "Audio Device Suggestions",
                    "No Audio Detected Warning",
                    "Quality Of Service"
                ],
                2
            ),
            "Audio Device Suggestions, No Audio Detected Warning, and more"
        );
        assert_eq!(collapsed_subtitle(&["A", "B"], 3), "A and B");
        assert_eq!(collapsed_subtitle(&["A", "B", "C"], 3), "A, B, and C");
        assert_eq!(collapsed_subtitle(&["A"], 2), "A");
    }

    /// The meter is a whole number of 8px cells and uncovers the gradient in 8px steps.
    #[test]
    fn the_meter_snaps_to_its_cells() {
        assert_eq!(to_eight(563.), 560.);
        let width = 560.;
        let uncovered = |progress: f32| (to_eight(width * (100. - progress) / 100.) - width).abs();
        assert_eq!(uncovered(0.), 0.);
        assert_eq!(uncovered(100.), 560.);
        assert_eq!(uncovered(50.), 280.);
        assert_eq!(uncovered(1.), 8.);
    }

    #[test]
    fn profiles_read_back_from_their_labels() {
        for profile in Profile::ALL {
            assert_eq!(Profile::from_label(profile.label()), profile);
        }
        assert_eq!(Profile::from_label("something else"), Profile::Custom);
    }
}
