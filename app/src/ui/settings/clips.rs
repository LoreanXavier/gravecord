//! Clips, the User Settings page under Games & Apps. Ported from the official `ClipsStore` (module 419490) and the
//! page module 158935 builds over it (`CLIPS_PANEL`, `CLIPS_GENERAL_CATEGORY`, `CLIPS_CAPTURE_SETTINGS_CATEGORY`,
//! `CLIPS_STORAGE_CATEGORY`); the entry is listed by `sw` of module 734066. Measured in
//! `design/dom/settings/clips-fake-premium*-darker-cozy.json`; strings from the page's message modules.
//!
//! The settings are device-local (`ClipsStore` is a `DeviceSettingsStore`), so they live in `storage.json` under the
//! store's own persist key, `ClipsStore`, in the shape the official store persists (`clipsSettings`,
//! `clipsDefaultSettingsState`). The Clipping keybind is the `SAVE_CLIP` entry of the `KeybindsStore` (default
//! `alt+c`, module 532624), kept under its own key. Capture runs start from the defaults, so a frame never depends on
//! what an earlier run saved.

pub(in crate::ui) mod engine;

use super::model::{ButtonKind, Choice, Runs};
use super::render::{self, PANEL};
use super::voice::{KEYBOARD_KEY, MOUSE_BUTTON, shortcut_text, windows_code};
use super::{Page, Root, button, color, divider, mask_icon};
use crate::parity::Parity;
use crate::storage;
use crate::ui::tooltip;
use dc_state::premium::PremiumType;
use gpui::{
    AnyElement, Bounds, Context, FocusHandle, FontWeight, IntoElement, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathPromptOptions, Pixels, Rgba, SharedString,
    Window, canvas, deferred, div, point, prelude::*, px, relative, size,
};
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// The store's persist key (`ClipsStore.persistKey`).
pub(in crate::ui) const STORED: &str = "ClipsStore";
/// Where the `SAVE_CLIP` entry of the `KeybindsStore` is kept.
const KEYBIND_STORED: &str = "KeybindsStore.SAVE_CLIP";

pub(super) const ENABLE: &str = "clips_enable";
pub(super) const REMINDERS: &str = "clips_enable_reminders";
const GENERAL_CATEGORY: &str = "clips_general_category";
const CAPTURE_CATEGORY: &str = "clips_capture_settings_category";
const STORAGE_CATEGORY: &str = "clips_storage_category";
/// The scroll-spy's keys, in the subnav's order.
pub(super) const CATEGORIES: [&str; 3] = [GENERAL_CATEGORY, CAPTURE_CATEGORY, STORAGE_CATEGORY];
/// `getArticleURL(CLIPS)`.
const LEARN_MORE: &str = "https://support.discord.com/hc/en-us/articles/16861982215703";

/// `I` in the store: the stored location before the official client resolves it to the Videos folder.
const DEFAULT_LOCATION: &str = "default";
/// `p`: the folder the default location adds under Videos.
const DEFAULT_FOLDER: &str = "Discord Clips";

/// The sub-entries of the Clips entry, in the sidebar's order (`CLIPS_*_CATEGORY` titles).
pub(super) const ANCHORS: &[(&str, bool)] = &[
    ("Clipping", false),
    ("Capture Settings", false),
    ("Storage", false),
];

/// `u.LX`: a clip's length in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClipLength {
    Seconds30,
    Minutes1,
    Minutes2,
}

impl ClipLength {
    pub(super) const ALL: [ClipLength; 3] = [
        ClipLength::Seconds30,
        ClipLength::Minutes1,
        ClipLength::Minutes2,
    ];

    pub(super) fn millis(self) -> u64 {
        match self {
            ClipLength::Seconds30 => 30_000,
            ClipLength::Minutes1 => 60_000,
            ClipLength::Minutes2 => 120_000,
        }
    }

    fn from_millis(millis: u64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|length| length.millis() == millis)
    }

    /// `bTFv/3` and `ICo9Nk`: "{count} Second(s)" and "{count} Minute(s)".
    pub(super) fn label(self) -> &'static str {
        match self {
            ClipLength::Seconds30 => "30 Seconds",
            ClipLength::Minutes1 => "1 Minute",
            ClipLength::Minutes2 => "2 Minutes",
        }
    }
}

/// `h.on`: the resolution a clip is recorded in, by its height; `RESOLUTION_SOURCE` is 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Resolution {
    P480,
    P720,
    P1080,
    P1440,
    Source,
}

impl Resolution {
    pub(super) const ALL: [Resolution; 5] = [
        Resolution::P480,
        Resolution::P720,
        Resolution::P1080,
        Resolution::P1440,
        Resolution::Source,
    ];

    pub(super) fn height(self) -> u64 {
        match self {
            Resolution::P480 => 480,
            Resolution::P720 => 720,
            Resolution::P1080 => 1080,
            Resolution::P1440 => 1440,
            Resolution::Source => 0,
        }
    }

    fn from_height(height: u64) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.height() == height)
    }

    /// `TEOC0I` ("{resolution}p") and `XjXqzh` ("Source").
    pub(super) fn label(self) -> &'static str {
        match self {
            Resolution::P480 => "480p",
            Resolution::P720 => "720p",
            Resolution::P1080 => "1080p",
            Resolution::P1440 => "1440p",
            Resolution::Source => "Source",
        }
    }
}

/// `h.kn`: the frame rate a clip is recorded at. The page lists 30 and 60; the store also knows 5 and 15.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrameRate {
    Fps30,
    Fps60,
}

impl FrameRate {
    pub(super) const ALL: [FrameRate; 2] = [FrameRate::Fps30, FrameRate::Fps60];

    pub(super) fn fps(self) -> u64 {
        match self {
            FrameRate::Fps30 => 30,
            FrameRate::Fps60 => 60,
        }
    }

    /// Migration 19 turns the removed 15 into 30; anything else the page does not list is dropped.
    fn from_fps(fps: u64) -> Option<Self> {
        match fps {
            15 | 30 => Some(FrameRate::Fps30),
            60 => Some(FrameRate::Fps60),
            _ => None,
        }
    }

    /// `Qb44XH`: "{fps}FPS".
    pub(super) fn label(self) -> &'static str {
        match self {
            FrameRate::Fps30 => "30FPS",
            FrameRate::Fps60 => "60FPS",
        }
    }
}

/// `clipsQuality`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClipsQuality {
    pub resolution: Resolution,
    pub frame_rate: FrameRate,
    /// 10 to 100.
    pub bitrate_percent: u8,
}

/// `clipSignals`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClipSignals {
    pub enable_distributed_signals: bool,
    pub enable_game_signals: bool,
}

/// `clipsSettings`, with the defaults of `w` in the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ClipsSettings {
    pub clips_enabled: bool,
    /// A folder, or `"default"` until it is resolved to the Videos folder.
    pub storage_location: String,
    pub clips_quality: ClipsQuality,
    pub clips_length: ClipLength,
    pub reminders_enabled: bool,
    pub decoupled_clips_enabled: bool,
    pub max_auto_clips: u64,
    pub clip_signals: ClipSignals,
    pub debug_tooltips_enabled: bool,
    pub enable_autoclipping: Option<bool>,
    pub show_pov_clips_in_gallery: bool,
}

impl Default for ClipsSettings {
    fn default() -> Self {
        Self {
            clips_enabled: false,
            storage_location: DEFAULT_LOCATION.to_owned(),
            clips_quality: ClipsQuality {
                resolution: Resolution::P1080,
                frame_rate: FrameRate::Fps60,
                bitrate_percent: 75,
            },
            clips_length: ClipLength::Seconds30,
            reminders_enabled: true,
            decoupled_clips_enabled: false,
            max_auto_clips: 20,
            clip_signals: ClipSignals {
                enable_distributed_signals: true,
                enable_game_signals: true,
            },
            debug_tooltips_enabled: false,
            enable_autoclipping: None,
            show_pov_clips_in_gallery: false,
        }
    }
}

/// `clipsDefaultSettingsState.clipsEnabledSource` (`$D` in module 696016).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EnabledSource {
    Unset,
    Default,
    User,
}

impl EnabledSource {
    fn name(self) -> &'static str {
        match self {
            EnabledSource::Unset => "unset",
            EnabledSource::Default => "default",
            EnabledSource::User => "user",
        }
    }

    fn from_name(name: &str) -> Self {
        match name {
            "default" => EnabledSource::Default,
            "user" => EnabledSource::User,
            _ => EnabledSource::Unset,
        }
    }
}

/// The persisted part of `ClipsStore`: the settings, and how Enable Clipping came to be what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ClipsStore {
    pub settings: ClipsSettings,
    pub enabled_source: EnabledSource,
}

impl Default for ClipsStore {
    fn default() -> Self {
        Self {
            settings: ClipsSettings::default(),
            enabled_source: EnabledSource::Unset,
        }
    }
}

impl ClipsStore {
    /// What `ClipsStore` persists. A missing or malformed field reads as its default, as the store's migrations
    /// fill them in (a quality that is a bare number or null becomes the default one, a 15 FPS rate becomes 30).
    pub(super) fn from_value(saved: &Value) -> Self {
        let defaults = ClipsSettings::default();
        let settings = &saved["clipsSettings"];
        let quality = &settings["clipsQuality"];
        let flag = |value: &Value, fallback: bool| value.as_bool().unwrap_or(fallback);
        let number = |value: &Value| value.as_u64();
        let clips_quality = if quality.is_object() {
            ClipsQuality {
                resolution: number(&quality["resolution"])
                    .and_then(Resolution::from_height)
                    .unwrap_or(defaults.clips_quality.resolution),
                frame_rate: number(&quality["frameRate"])
                    .and_then(FrameRate::from_fps)
                    .unwrap_or(defaults.clips_quality.frame_rate),
                bitrate_percent: number(&quality["bitratePercent"])
                    .map(|percent| percent.clamp(10, 100) as u8)
                    .unwrap_or(defaults.clips_quality.bitrate_percent),
            }
        } else {
            defaults.clips_quality
        };
        let signals = &settings["clipSignals"];
        let settings = ClipsSettings {
            clips_enabled: flag(&settings["clipsEnabled"], defaults.clips_enabled),
            storage_location: settings["storageLocation"]
                .as_str()
                .filter(|location| !location.is_empty())
                .map_or(defaults.storage_location.clone(), str::to_owned),
            clips_quality,
            clips_length: number(&settings["clipsLength"])
                .and_then(ClipLength::from_millis)
                .unwrap_or(defaults.clips_length),
            reminders_enabled: flag(&settings["remindersEnabled"], defaults.reminders_enabled),
            decoupled_clips_enabled: flag(
                &settings["decoupledClipsEnabled"],
                defaults.decoupled_clips_enabled,
            ),
            max_auto_clips: number(&settings["maxAutoClips"]).unwrap_or(defaults.max_auto_clips),
            clip_signals: ClipSignals {
                enable_distributed_signals: flag(
                    &signals["enableDistributedSignals"],
                    defaults.clip_signals.enable_distributed_signals,
                ),
                enable_game_signals: flag(
                    &signals["enableGameSignals"],
                    defaults.clip_signals.enable_game_signals,
                ),
            },
            debug_tooltips_enabled: flag(
                &settings["debugTooltipsEnabled"],
                defaults.debug_tooltips_enabled,
            ),
            enable_autoclipping: settings["enableAutoclipping"].as_bool(),
            show_pov_clips_in_gallery: flag(
                &settings["showPovClipsInGallery"],
                defaults.show_pov_clips_in_gallery,
            ),
        };
        Self {
            settings,
            enabled_source: EnabledSource::from_name(
                saved["clipsDefaultSettingsState"]["clipsEnabledSource"]
                    .as_str()
                    .unwrap_or(""),
            ),
        }
    }

    pub(super) fn to_value(&self) -> Value {
        let settings = &self.settings;
        let quality = settings.clips_quality;
        let mut fields = Map::new();
        fields.insert("clipsEnabled".into(), json!(settings.clips_enabled));
        fields.insert("storageLocation".into(), json!(settings.storage_location));
        fields.insert(
            "clipsQuality".into(),
            json!({
                "resolution": quality.resolution.height(),
                "frameRate": quality.frame_rate.fps(),
                "bitratePercent": quality.bitrate_percent,
            }),
        );
        fields.insert("clipsLength".into(), json!(settings.clips_length.millis()));
        fields.insert("remindersEnabled".into(), json!(settings.reminders_enabled));
        fields.insert(
            "decoupledClipsEnabled".into(),
            json!(settings.decoupled_clips_enabled),
        );
        fields.insert("maxAutoClips".into(), json!(settings.max_auto_clips));
        fields.insert(
            "clipSignals".into(),
            json!({
                "enableDistributedSignals": settings.clip_signals.enable_distributed_signals,
                "enableGameSignals": settings.clip_signals.enable_game_signals,
            }),
        );
        fields.insert(
            "debugTooltipsEnabled".into(),
            json!(settings.debug_tooltips_enabled),
        );
        fields.insert(
            "enableAutoclipping".into(),
            json!(settings.enable_autoclipping),
        );
        fields.insert(
            "showPovClipsInGallery".into(),
            json!(settings.show_pov_clips_in_gallery),
        );
        json!({
            "clipsSettings": Value::Object(fields),
            "clipsDefaultSettingsState": {
                "clipsEnabledSource": self.enabled_source.name(),
                "version": 0,
                "enableAutoclipping": Value::Null,
            },
        })
    }

    /// `CLIPS_SETTINGS_UPDATE` with `clipsEnabled`: the choice is the user's from now on.
    pub(super) fn set_enabled(&mut self, enabled: bool) {
        self.settings.clips_enabled = enabled;
        self.enabled_source = EnabledSource::User;
    }
}

/// `OH("alt+c")` on Windows: Alt, then C, both keyboard keys of the Windows table.
pub(super) fn default_keybind() -> Vec<[u32; 3]> {
    ["alt", "c"]
        .into_iter()
        .filter_map(windows_code)
        .map(|code| [KEYBOARD_KEY, code, super::voice::WINDOWS])
        .collect()
}

fn keybind_from_value(saved: &Value) -> Option<Vec<[u32; 3]>> {
    saved
        .as_array()?
        .iter()
        .map(|part| {
            let part = part.as_array()?;
            let at = |index: usize| u32::try_from(part.get(index)?.as_u64()?).ok();
            Some([at(0)?, at(1)?, at(2)?])
        })
        .collect()
}

/// The Videos known folder: `app.getPath("videos")`.
#[cfg(windows)]
fn videos_folder() -> Option<PathBuf> {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_Videos)
}

#[cfg(not(windows))]
fn videos_folder() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| Path::new(&home).join("Videos"))
}

/// `app.getPath("documents")`.
#[cfg(windows)]
fn documents_folder() -> Option<PathBuf> {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_Documents)
}

#[cfg(not(windows))]
fn documents_folder() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| Path::new(&home).join("Documents"))
}

#[cfg(windows)]
fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    // SAFETY: the shell returns a null-terminated wide string it allocated; it is copied and freed at once.
    unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let text = std::ffi::OsString::from_wide(path.as_wide());
        CoTaskMemFree(Some(path.0 as *const std::ffi::c_void));
        Some(text.into())
    }
}

/// The default storage location the store resolves at start: `path.join(videos, "Discord Clips")`; when the Videos
/// folder cannot be read, the Documents folder itself.
fn resolve_default_location(videos: Option<PathBuf>, documents: Option<PathBuf>) -> Option<String> {
    let folder = match videos {
        Some(videos) => videos.join(DEFAULT_FOLDER),
        None => documents?,
    };
    Some(folder.to_string_lossy().into_owned())
}

/// Which select's list is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClipSelect {
    Length,
    FrameRate,
    Resolution,
}

impl ClipSelect {
    fn key(self) -> &'static str {
        match self {
            ClipSelect::Length => "clips_length",
            ClipSelect::FrameRate => "clips_frame_rate",
            ClipSelect::Resolution => "clips_resolution",
        }
    }

    fn labels(self) -> Vec<&'static str> {
        match self {
            ClipSelect::Length => ClipLength::ALL.iter().map(|value| value.label()).collect(),
            ClipSelect::FrameRate => FrameRate::ALL.iter().map(|value| value.label()).collect(),
            ClipSelect::Resolution => Resolution::ALL.iter().map(|value| value.label()).collect(),
        }
    }
}

/// `getKeyboardPageStep`: a tenth of the range, (100 - 10) / 10.
const PAGE_STEP: i32 = (QUALITY_MAX as i32 - QUALITY_MIN as i32) / 10;

/// The slider's range: `minValue` and `maxValue` of `CLIPS_BITRATE`.
const QUALITY_MIN: f32 = 10.;
const QUALITY_MAX: f32 = 100.;
/// `markers`, and `useDefaultValue`.
const QUALITY_MARKS: [u8; 5] = [10, 25, 50, 75, 100];
const QUALITY_DEFAULT: u8 = 75;

fn quality_fraction(percent: u8) -> f32 {
    (f32::from(percent) - QUALITY_MIN) / (QUALITY_MAX - QUALITY_MIN)
}

/// The slider's value at a position along the track, in whole percents (`step` 1).
fn quality_at(fraction: f32) -> u8 {
    (QUALITY_MIN + fraction.clamp(0., 1.) * (QUALITY_MAX - QUALITY_MIN)).round() as u8
}

/// A slider being dragged: where its bar is on screen, and the value so far.
struct Drag {
    bounds: Bounds<Pixels>,
    percent: u8,
}

/// The Clips page's state on `Root`.
pub(in crate::ui) struct ClipsState {
    store: ClipsStore,
    keybind: Vec<[u32; 3]>,
    /// The keybind recorder is listening, and the `[deviceType, code]` pairs held since it started.
    recording: bool,
    recording_held: Vec<[u32; 2]>,
    modifiers: Modifiers,
    open: Option<ClipSelect>,
    drag: Option<Drag>,
    hovered: bool,
    focus: RefCell<Option<FocusHandle>>,
    /// The folder dialog is open.
    picking: bool,
    /// The capture engine's side: the clips known, the save counter, the keybind hook (`engine.rs`).
    pub(in crate::ui) engine: engine::Engine,
}

impl ClipsState {
    /// `canShowReminders()`: the Reminders switch.
    pub(in crate::ui) fn reminders_enabled(&self) -> bool {
        self.store.settings.reminders_enabled
    }

    /// At start: the stored settings, with the default location resolved as the store does.
    pub(in crate::ui) fn restore() -> Self {
        let (mut store, keybind) = if storage::is_capture_run() {
            (ClipsStore::default(), None)
        } else {
            (
                storage::get(STORED)
                    .map_or_else(ClipsStore::default, |saved| ClipsStore::from_value(&saved)),
                storage::get(KEYBIND_STORED).and_then(|saved| keybind_from_value(&saved)),
            )
        };
        if store.settings.storage_location == DEFAULT_LOCATION
            && let Some(location) = resolve_default_location(videos_folder(), documents_folder())
        {
            store.settings.storage_location = location;
            storage::set(STORED, store.to_value());
        }
        Self {
            store,
            keybind: keybind.unwrap_or_else(default_keybind),
            recording: false,
            recording_held: Vec::new(),
            modifiers: Modifiers::default(),
            open: None,
            drag: None,
            hovered: false,
            focus: RefCell::new(None),
            picking: false,
            engine: engine::Engine::default(),
        }
    }

    #[cfg(test)]
    fn settings(&self) -> &ClipsSettings {
        &self.store.settings
    }

    fn save(&self) {
        storage::set(STORED, self.store.to_value());
        // The recorder follows the settings (clipping on or off, the length it keeps).
        self.engine.configure(&self.store.settings);
    }

    fn save_keybind(&self) {
        storage::set(KEYBIND_STORED, json!(self.keybind));
    }
}

/// The tests' view of the state, without a window.
impl ClipsState {
    #[cfg(test)]
    fn detached(store: ClipsStore) -> Self {
        Self {
            store,
            keybind: default_keybind(),
            recording: false,
            recording_held: Vec::new(),
            modifiers: Modifiers::default(),
            open: None,
            drag: None,
            hovered: false,
            focus: RefCell::new(None),
            picking: false,
            engine: engine::Engine::default(),
        }
    }
}

fn with_alpha(mut value: Rgba, alpha: f32) -> Rgba {
    value.a = alpha;
    value
}

/// `CLIPS_ENABLE`'s subtitle (`4Qw3NO`), `CLIPS_KEYBIND`'s (`QyB/jK`) and the other rows' strings.
const ENABLE_DESCRIPTION: Runs = &[(
    "Enables you to capture clips while you're streaming by using the clipping keybind.",
    false,
)];
const KEYBIND_DESCRIPTION: Runs = &[(
    "Set up your keybind to capture clips. The default is ALT + C.",
    false,
)];
const LENGTH_DESCRIPTION: Runs = &[("By default, clips are 30 seconds long.", false)];
const FRAME_RATE_DESCRIPTION: Runs = &[(
    "This is the maximum frame rate your clips will be recorded at.",
    false,
)];
const RESOLUTION_DESCRIPTION: Runs = &[(
    "This is the maximum resolution your clips will be recorded in.",
    false,
)];
const QUALITY_DESCRIPTION: Runs = &[(
    "This is the quality your clips will be recorded at. Higher quality results in larger file sizes.",
    false,
)];
const REMINDERS_DESCRIPTION: Runs = &[(
    "After a clip is captured, show a reminder for new clips",
    false,
)];
const STORAGE_DESCRIPTION: Runs = &[("Choose where to store your clips.", false)];
/// `Z+MfqT`, in `CLIPS_QUALITY_INFOBOX`.
const QUALITY_NOTE: &str = "Please note that clips are currently captured at the same quality as your stream when streaming.";

/// `heading-xl/medium` with its description 4px under it, 20px of space, then the 1px marker the scroll-spy watches.
fn category_head(
    title: &'static str,
    description: Option<AnyElement>,
    marker: AnyElement,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .pb(px(20.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(24.))
                        .line_height(px(32.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(title),
                )
                .children(description.map(|description| {
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(description)
                })),
        )
        .child(div().relative().h(px(1.)).child(marker))
}

/// The pieces of the keybind recorder's buttons: `sm` icon-only secondary buttons, 32 square.
fn icon_button(id: &'static str, icon: &'static str, disabled: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .parity_with_border(
            if icon == "clips_keyboard" {
                "clips-edit-keybind"
            } else {
                "clips-reset-keybind"
            },
            1.,
        )
        .flex_none()
        .size(px(32.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--control-secondary-border-default"))
        .bg(color("--control-secondary-background-default"))
        .flex()
        .items_center()
        .justify_center()
        .when(disabled, |this| this.opacity(0.5))
        .when(!disabled, |this| {
            this.cursor_pointer().hover(|style| {
                style
                    .bg(color("--control-secondary-background-hover"))
                    .border_color(color("--control-secondary-border-hover"))
            })
        })
        .child(mask_icon(icon, 16., "--control-secondary-text-default"))
}

impl Root {
    /// `sw` in module 734066: the entry is listed for Nitro (`TIER_2`; Nitro Basic and Classic rank below it) on a
    /// platform whose engine can clip, which is Windows here.
    pub(in crate::ui) fn clips_listed(&self) -> bool {
        cfg!(windows) && self.premium() == PremiumType::Nitro
    }

    /// `useClipsEnabled` (module 572164): the entry is listed and Enable Clipping is on.
    fn clips_enabled(&self) -> bool {
        self.clips_listed() && self.clips.store.settings.clips_enabled
    }

    fn clips_on_page(&self) -> bool {
        self.settings
            .as_ref()
            .is_some_and(|open| open.page == Page::Clips)
    }

    /// The Clips page: Clipping, Capture Settings and Storage, with a divider between each.
    pub(super) fn clips_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let enabled = self.clips_enabled();
        vec![
            div()
                .parity("clips-general")
                .child(self.clips_general(window, cx))
                .into_any_element(),
            div()
                .parity("clips-divider")
                .my(px(40.))
                .h(px(1.))
                .bg(color("--border-subtle"))
                .into_any_element(),
            div()
                .parity("clips-capture")
                .child(self.clips_capture(enabled, window, cx))
                .into_any_element(),
            divider(),
            div()
                .parity("clips-storage")
                .child(self.clips_storage(enabled, cx))
                .into_any_element(),
        ]
    }

    fn clips_general(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.clips_enabled();
        let marker = self
            .anchor_probe(format!("settings-marker:{GENERAL_CATEGORY}"))
            .into_any_element();
        // The card: a header holding Enable Clipping and a body holding the keybind, in a 12px rounded frame.
        let card = div()
            .parity_with_border("clips-card", 1.)
            .flex()
            .flex_col()
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-muted"))
            .child(
                div()
                    .parity_below("clips-card-header", 1.)
                    .px(px(20.))
                    .py(px(12.))
                    .rounded_t(px(11.))
                    .border_b_1()
                    .border_color(color("--border-muted"))
                    .bg(color("--background-mod-muted"))
                    .child(div().py(px(8.)).child(self.switch_row(
                        ENABLE,
                        "Enable Clipping",
                        false,
                        ENABLE_DESCRIPTION,
                        self.clips.store.settings.clips_enabled && self.clips_listed(),
                        false,
                        PANEL - 42.,
                        window,
                        cx,
                    ))),
            )
            .child(
                div()
                    .parity("clips-card-body")
                    .p(px(20.))
                    .rounded_b(px(11.))
                    .bg(color("--background-mod-subtle"))
                    .child(self.clips_keybind_row(enabled, PANEL - 42., window, cx)),
            );
        div()
            .flex()
            .flex_col()
            .child(category_head(
                "Clipping",
                Some(self.text_with_link(
                    "clips-learn-more",
                    "Clip any fun or amazing gaming moments by saving them directly to your computer. ",
                    "Learn more",
                    "",
                    cx,
                    |_, cx| cx.open_url(LEARN_MORE),
                )),
                marker,
            ))
            .child(card)
            .into_any_element()
    }

    /// `CLIPS_KEYBIND`: the label with its note, and the key recorder (module 368484) with its two buttons.
    fn clips_keybind_row(
        &self,
        enabled: bool,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const RECORDER: f32 = 240.;
        let disabled = !enabled;
        let recording = self.clips.recording && enabled;
        let red = color("--red-400");
        let critical = color("--text-feedback-critical");
        let (text, text_color, weight) = if recording {
            (String::new(), critical, 600.)
        } else if self.clips.keybind.is_empty() {
            (
                "No Keybind Set".to_owned(),
                color("--input-placeholder-text-default"),
                400.,
            )
        } else {
            (
                shortcut_text(&self.clips.keybind),
                color("--text-strong"),
                600.,
            )
        };
        let extent = render::label_extent(window, "Clipping Keybind", false, KEYBIND_DESCRIPTION);
        let label_width = render::label_track(width - 24., extent, RECORDER);
        let edit = self.with_tip(
            "clips-edit-keybind",
            tooltip::Tip::text("Edit keybind", tooltip::Side::Top),
            icon_button("clips-edit-keybind-button", "clips_keyboard", disabled).when(
                !disabled,
                |this| {
                    this.on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.clips_toggle_recording();
                        cx.notify();
                    }))
                },
            ),
            cx,
        );
        let reset = self.with_tip(
            "clips-reset-keybind",
            tooltip::Tip::text("Reset to default", tooltip::Side::Top),
            icon_button("clips-reset-keybind-button", "clips_retry", disabled).when(
                !disabled,
                |this| {
                    this.on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.clips_reset_keybind();
                        cx.notify();
                    }))
                },
            ),
            cx,
        );
        let recorder = div()
            .parity_with_border("clips-keybind", 1.)
            .id("clips-keybind-recorder")
            .flex_none()
            .w(px(RECORDER))
            .h(px(44.))
            .rounded(px(8.))
            .border_1()
            .bg(color("--input-background-default"))
            .flex()
            .items_center()
            .when(disabled, |this| this.opacity(0.3).cursor_not_allowed())
            // `.recording`: a red border. Its pulsing glow is left out, as the Voice & Video recorder leaves it out.
            .when(recording, |this| this.border_color(with_alpha(red, 0.6)))
            .when(!recording, |this| {
                this.border_color(color("--input-border-default"))
            })
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clips_toggle_recording();
                        cx.notify();
                    }))
            })
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
            .child(
                div()
                    .flex_none()
                    .pr(px(6.))
                    .flex()
                    .gap(px(4.))
                    .child(edit)
                    .child(reset),
            );
        div()
            .py(px(8.))
            .flex()
            .justify_between()
            .gap(px(24.))
            .child(
                render::label_block("Clipping Keybind", false, KEYBIND_DESCRIPTION)
                    .w(px(label_width)),
            )
            .child(recorder)
            .into_any_element()
    }

    fn clips_capture(
        &self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &self.clips.store.settings;
        let marker = self
            .anchor_probe(format!("settings-marker:{CAPTURE_CATEGORY}"))
            .into_any_element();
        let length = ClipLength::ALL
            .iter()
            .position(|value| *value == settings.clips_length)
            .unwrap_or(0);
        let frame_rate = FrameRate::ALL
            .iter()
            .position(|value| *value == settings.clips_quality.frame_rate)
            .unwrap_or(0);
        let resolution = Resolution::ALL
            .iter()
            .position(|value| *value == settings.clips_quality.resolution)
            .unwrap_or(0);
        div()
            .flex()
            .flex_col()
            .child(category_head("Capture Settings", None, marker))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(self.clips_select_row(
                        ClipSelect::Length,
                        "Clip Length",
                        LENGTH_DESCRIPTION,
                        length,
                        enabled,
                        cx,
                    ))
                    .child(self.clips_select_row(
                        ClipSelect::FrameRate,
                        "Clip Frame Rate",
                        FRAME_RATE_DESCRIPTION,
                        frame_rate,
                        enabled,
                        cx,
                    ))
                    .child(self.clips_select_row(
                        ClipSelect::Resolution,
                        "Clip Resolution",
                        RESOLUTION_DESCRIPTION,
                        resolution,
                        enabled,
                        cx,
                    ))
                    .child(self.clips_quality_row(enabled, cx))
                    .child(
                        div().py(px(8.)).child(
                            div()
                                .parity_with_border("clips-notice", 1.)
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .p(px(8.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(color("--border-feedback-info"))
                                .bg(color("--background-feedback-mod-info"))
                                .child(mask_icon(
                                    "settings_notice_info",
                                    24.,
                                    "--icon-feedback-info",
                                ))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        .child(QUALITY_NOTE),
                                ),
                        ),
                    )
                    .child(div().py(px(8.)).child(self.switch_row(
                        REMINDERS,
                        "Enable Reminders",
                        false,
                        REMINDERS_DESCRIPTION,
                        settings.reminders_enabled,
                        !enabled,
                        PANEL,
                        window,
                        cx,
                    ))),
            )
            .into_any_element()
    }

    /// A label with its select, side by side (`horizontal-responsive`): the field is 206 wide, its list 4px off it.
    /// The list opens upward where it would run past the window (the popper flips it).
    fn clips_select_row(
        &self,
        select: ClipSelect,
        label: &'static str,
        description: Runs,
        chosen: usize,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const FIELD: f32 = 206.;
        let disabled = !enabled;
        let key = select.key();
        let labels = select.labels();
        let open = enabled && self.clips.open == Some(select);
        let shown = labels.get(chosen).copied().unwrap_or("");
        let list_height = labels.len() as f32 * 40. + 10.;
        let anchor = format!("clips-select:{key}");
        let up = self
            .tip_anchors
            .borrow()
            .get(&SharedString::from(anchor.clone()))
            .is_some_and(|bounds| f32::from(bounds.bottom()) + 4. + list_height > self.viewport.1);
        let rows: Vec<AnyElement> = labels
            .iter()
            .enumerate()
            .map(|(index, option)| {
                div()
                    .id(SharedString::from(format!("{key}-option-{index}")))
                    .flex_none()
                    .h(px(40.))
                    .pl(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(*option),
                    )
                    .when(index == chosen, |this| {
                        this.child(div().p(px(4.)).child(crate::ui::icon(
                            "select_check",
                            20.,
                            "--text-strong",
                        )))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.clips_choose(select, index);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let field = render::select_field_open(shown, Some(FIELD), open)
            .id(SharedString::from(format!("{key}-field")))
            .when(select == ClipSelect::Length, |this| {
                this.parity_with_border("clips-length-select", 1.)
            })
            .relative()
            .when(disabled, |this| this.opacity(0.5).cursor_not_allowed())
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.clips.open = if this.clips.open == Some(select) {
                            None
                        } else {
                            Some(select)
                        };
                        cx.notify();
                    }))
            })
            .child(self.anchor_probe(anchor));
        div()
            .parity(key)
            .min_h(px(48.))
            .py(px(8.))
            .flex()
            .gap(px(24.))
            .child(
                render::label_block(label, false, description)
                    .w(px(PANEL - 24. - FIELD))
                    .when(disabled, |this| this.opacity(0.5)),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .h(px(40.))
                    .child(field)
                    .when(open, |this| {
                        this.child(
                            deferred(
                                div()
                                    .id(SharedString::from(format!("{key}-list")))
                                    .occlude()
                                    .absolute()
                                    .when(!up, |this| this.top(px(44.)))
                                    .when(up, |this| this.bottom(px(44.)))
                                    .left_0()
                                    .right_0()
                                    .p(px(4.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-surface-higher"))
                                    .shadow(vec![gpui::BoxShadow {
                                        color: gpui::hsla(0., 0., 0., 0.16),
                                        offset: point(px(0.), px(2.)),
                                        blur_radius: px(8.),
                                        spread_radius: px(0.),
                                        inset: false,
                                    }])
                                    .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                                        if this.clips.open == Some(select) {
                                            this.clips.open = None;
                                            cx.notify();
                                        }
                                    }))
                                    .child(div().flex().flex_col().children(rows)),
                            )
                            .with_priority(2),
                        )
                    }),
            )
            .into_any_element()
    }

    /// `CLIPS_BITRATE`: the label, and a slider with marks at 10, 25, 50, 75 and 100 % (`slider_OnqacX hasMarks`).
    fn clips_quality_row(&self, enabled: bool, cx: &mut Context<Self>) -> AnyElement {
        let disabled = !enabled;
        let percent = self.clips_percent();
        div()
            .parity("clips_bitrate")
            .min_h(px(48.))
            .py(px(8.))
            .flex()
            .flex_col()
            .child(
                render::label_block("Clip Quality", false, QUALITY_DESCRIPTION)
                    .mb(px(8.))
                    .when(disabled, |this| this.opacity(0.5)),
            )
            .child(self.clips_slider(percent, disabled, cx))
            .into_any_element()
    }

    /// The value shown: the drag's while dragging, else the stored one.
    fn clips_percent(&self) -> u8 {
        self.clips.drag.as_ref().map_or(
            self.clips.store.settings.clips_quality.bitrate_percent,
            |drag| drag.percent,
        )
    }

    /// The slider: marks above a 4px bar at 34px, the 16px grabber centred on the bar. Hovered or dragged, the bar
    /// is 6px and the grabber 20px (the slider of module 205513). The default mark's label is green.
    fn clips_slider(&self, percent: u8, disabled: bool, cx: &mut Context<Self>) -> AnyElement {
        let fraction = quality_fraction(percent);
        let dragging = self.clips.drag.is_some();
        let active = !disabled && (dragging || self.clips.hovered);
        let (bar_height, grabber) = if active { (6., 20.) } else { (4., 16.) };
        let focus = self
            .clips
            .focus
            .borrow_mut()
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let width = PANEL;
        let track = width - 10.;
        let last = QUALITY_MARKS.len() - 1;
        let marks = QUALITY_MARKS.iter().enumerate().map(|(index, mark)| {
            let at = quality_fraction(*mark);
            let x = 5. + track * at;
            let label_color = if *mark == QUALITY_DEFAULT {
                "--text-feedback-positive"
            } else {
                "--text-subtle"
            };
            // The first and last labels are pulled in so they end at the track's edges.
            let shift = if index == 0 {
                0.5
            } else if index == last {
                -0.5
            } else {
                0.
            };
            div()
                .absolute()
                .top_0()
                .left(px(x - 12.))
                .w(px(24.))
                .h(px(48.))
                .flex()
                .flex_col()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .relative()
                        .left(relative(shift))
                        .pl(px(1.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(color(label_color))
                        .whitespace_nowrap()
                        .child(format!("{mark}%")),
                )
                .child(
                    div()
                        .w(px(2.))
                        .h(px(24.))
                        .rounded(px(12.))
                        .bg(color("--slider-track-background")),
                )
        });
        div()
            .parity("clips-slider")
            .id("clips-slider")
            .relative()
            .h(px(48.))
            .w_full()
            .track_focus(&focus)
            .when(!disabled, |this| {
                this.on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                    // Module 205513 `handleKeyDown`: one step per press (left/down toward the start, right/up
                    // toward the end), a page step of a tenth of the range, Home and End to the ends.
                    let direction: i32 = match event.keystroke.key.as_str() {
                        "left" | "down" => -1,
                        "right" | "up" => 1,
                        "pagedown" => -PAGE_STEP,
                        "pageup" => PAGE_STEP,
                        "home" => -(QUALITY_MAX as i32),
                        "end" => QUALITY_MAX as i32,
                        _ => return,
                    };
                    this.clips_nudge(direction, cx);
                    cx.stop_propagation();
                }))
            })
            .when(disabled, |this| this.opacity(0.6).cursor_not_allowed())
            .child(self.anchor_probe("clips-slider"))
            .children(marks)
            .child(
                div()
                    .absolute()
                    .top(px(36. - bar_height / 2.))
                    .left_0()
                    .w_full()
                    .h(px(bar_height))
                    .rounded(px(4.))
                    .overflow_hidden()
                    .bg(color("--slider-track-background"))
                    .child(
                        div()
                            .h_full()
                            .w(px(width * fraction))
                            .border_1()
                            .border_color(gpui::hsla(0., 0., 1., 0.1))
                            .bg(color("--background-brand")),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(36. - grabber / 2.))
                    .left(px(5. + track * fraction - grabber / 2.))
                    .size(px(grabber))
                    .rounded_full()
                    .border_1()
                    .border_color(color("--border-normal"))
                    .bg(gpui::white()),
            )
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.clips.hovered = *hovered;
                        if *hovered {
                            this.clips_show_bubble();
                        } else if this.clips.drag.is_none() {
                            this.tip = None;
                        }
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            if let Some(handle) = this.clips.focus.borrow().as_ref() {
                                window.focus(handle, cx);
                            }
                            this.clips_start_drag(event.position.x, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        if event.dragging() && this.clips.drag.is_some() {
                            this.clips_move_drag(event.position.x, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.clips_end_drag(cx)),
                    )
            })
            .when(dragging, |this| this.child(self.clips_drag_listener()))
            .into_any_element()
    }

    /// While the slider is dragged, moves and the release anywhere in the window reach it.
    fn clips_drag_listener(&self) -> impl IntoElement {
        let weak = self.weak.clone();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moved = weak.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Bubble && event.dragging() {
                        let _ = moved.update(cx, |this, cx| {
                            this.clips_move_drag(event.position.x, cx);
                        });
                    }
                });
                let released = weak.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Bubble && event.button == MouseButton::Left {
                        let _ = released.update(cx, |this, cx| this.clips_end_drag(cx));
                    }
                });
            },
        )
        .absolute()
        .w(px(0.))
        .h(px(0.))
    }

    /// The bubble over the grabber (`formatValue`: the value and a percent sign).
    fn clips_show_bubble(&mut self) {
        let Some(bounds) = self
            .tip_anchors
            .borrow()
            .get(&SharedString::from("clips-slider"))
            .copied()
        else {
            return;
        };
        let fraction = quality_fraction(self.clips_percent());
        let track = f32::from(bounds.size.width) - 10.;
        let center = point(
            bounds.left() + px(5. + track * fraction),
            bounds.top() + px(36.),
        );
        self.tip = Some(tooltip::Active::new(
            "clips-bubble".into(),
            tooltip::Tip::body(
                tooltip::Body::Text(format!("{}%", self.clips_percent()).into()),
                tooltip::Side::Top,
            ),
            Bounds::centered_at(center, size(px(20.), px(20.))),
        ));
    }

    fn clips_start_drag(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(bounds) = self
            .tip_anchors
            .borrow()
            .get(&SharedString::from("clips-slider"))
            .copied()
        else {
            return;
        };
        self.clips.drag = Some(Drag {
            bounds,
            percent: self.clips.store.settings.clips_quality.bitrate_percent,
        });
        self.clips_move_drag(x, cx);
    }

    fn clips_move_drag(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(drag) = self.clips.drag.as_mut() else {
            return;
        };
        let track = f32::from(drag.bounds.size.width) - 10.;
        let fraction = ((f32::from(x - drag.bounds.left()) - 5.) / track).clamp(0., 1.);
        drag.percent = quality_at(fraction);
        self.clips_show_bubble();
        cx.notify();
    }

    /// `onChangeEnd`: the value is written when the grabber is let go.
    fn clips_end_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.clips.drag.take() else {
            return;
        };
        self.clips_set_percent(drag.percent);
        self.tip = None;
        cx.notify();
    }

    /// An arrow key on the focused slider: one step toward the end the arrow points to.
    fn clips_nudge(&mut self, direction: i32, cx: &mut Context<Self>) {
        let current = i32::from(self.clips.store.settings.clips_quality.bitrate_percent);
        let next = (current + direction).clamp(QUALITY_MIN as i32, QUALITY_MAX as i32);
        self.clips_set_percent(next as u8);
        cx.notify();
    }

    /// `GS({...clipsQuality, bitratePercent})`.
    fn clips_set_percent(&mut self, percent: u8) {
        self.clips.store.settings.clips_quality.bitrate_percent = percent;
        self.clips.save();
    }

    /// An option picked from a select: `GS`, `h$`.
    fn clips_choose(&mut self, select: ClipSelect, index: usize) {
        let settings = &mut self.clips.store.settings;
        match select {
            ClipSelect::Length => {
                if let Some(length) = ClipLength::ALL.get(index) {
                    settings.clips_length = *length;
                }
            }
            ClipSelect::FrameRate => {
                if let Some(rate) = FrameRate::ALL.get(index) {
                    settings.clips_quality.frame_rate = *rate;
                }
            }
            ClipSelect::Resolution => {
                if let Some(resolution) = Resolution::ALL.get(index) {
                    settings.clips_quality.resolution = *resolution;
                }
            }
        }
        self.clips.open = None;
        self.clips.save();
    }

    fn clips_storage(&self, enabled: bool, cx: &mut Context<Self>) -> AnyElement {
        let disabled = !enabled;
        let location = self.clips.store.settings.storage_location.clone();
        let marker = self
            .anchor_probe(format!("settings-marker:{STORAGE_CATEGORY}"))
            .into_any_element();
        let input = div()
            .parity_with_border("clips-storage-input", 1.)
            .flex_1()
            .min_w(px(0.))
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .when(disabled, |this| this.opacity(0.5).cursor_not_allowed())
            .child(
                // The input is 38 tall with 1px of top padding: its text centres on a half pixel, which Chromium
                // rounds down to the lower line.
                div()
                    .h(px(38.))
                    .pt(px(2.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(location),
            );
        let edit = div()
            .parity("clips-edit-location")
            .id("clips-edit-location")
            .flex_none()
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled, |this| {
                this.on_click(cx.listener(|this, _, _, cx| this.clips_pick_location(cx)))
            })
            .child(button(
                "clips-edit-location-button",
                "Edit Location",
                ButtonKind::Secondary,
            ));
        div()
            .flex()
            .flex_col()
            .child(category_head("Storage", None, marker))
            .child(
                div().py(px(8.)).child(
                    div()
                        .parity("clips-storage-row")
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(render::label_block(
                            "Storage Location",
                            false,
                            STORAGE_DESCRIPTION,
                        ))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(input)
                                .child(edit),
                        ),
                ),
            )
            .into_any_element()
    }

    /// Edit Location: the native folder picker (`showOpenDialog` with `openDirectory` and `createDirectory`). A
    /// capture run never opens it; the harness's `clips location` step stands in for its answer.
    fn clips_pick_location(&mut self, cx: &mut Context<Self>) {
        if !self.clips_enabled() || self.clips.picking || storage::is_capture_run() {
            return;
        }
        self.clips.picking = true;
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let chosen = match picked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let _ = this.update(cx, |this, cx| {
                this.clips.picking = false;
                if let Some(path) = chosen {
                    this.clips_set_location(&path, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// `HU`: the new location is stored (the official action then reloads the clips found there).
    fn clips_set_location(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.clips.store.settings.storage_location = path.to_string_lossy().into_owned();
        self.clips.save();
        self.clips_reload_directory(cx);
    }

    /// The Enable Clipping and Enable Reminders switches (`yO`, `Mt`): a click on the generated switch row wrote
    /// its new value into `setting_choices`; it moves into the store. True when `key` is one of these.
    pub(in crate::ui) fn clips_setting_changed(&mut self, key: &'static str) -> bool {
        if key != ENABLE && key != REMINDERS {
            return false;
        }
        let on = matches!(self.setting_choices.remove(key), Some(Choice::On(true)));
        if key == ENABLE {
            self.clips.store.set_enabled(on);
            if !on {
                self.clips.recording = false;
                self.clips.recording_held.clear();
                self.clips.open = None;
            }
        } else {
            self.clips.store.settings.reminders_enabled = on;
        }
        self.clips.save();
        self.clips_sync_engine();
        self.sync_clips_hook();
        true
    }

    fn clips_toggle_recording(&mut self) {
        if !self.clips_enabled() {
            return;
        }
        self.clips.recording = !self.clips.recording;
        self.clips.recording_held.clear();
        self.sync_clips_hook();
    }

    /// Reset to default: the keybind is `alt+c` again.
    fn clips_reset_keybind(&mut self) {
        if !self.clips_enabled() {
            return;
        }
        self.clips.keybind = default_keybind();
        self.clips.save_keybind();
        self.sync_clips_hook();
    }

    /// Whether the recorder listens now: it is shown, and Enable Clipping is on.
    pub(in crate::ui) fn clips_recording(&self) -> bool {
        self.clips.recording && self.clips_on_page() && self.clips_enabled()
    }

    /// A key went down while recording: the combo grows and is saved at once (`handleNativeChange`). Escape ends
    /// recording without saving. True when the recorder took the key.
    pub(in crate::ui) fn clips_key_down(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.clips_recording() {
            return false;
        }
        // The modifiers held with the key count as pressed before it.
        self.clips_modifiers(keystroke.modifiers, cx);
        let key = keystroke.key.as_str();
        if key == "escape" {
            self.clips.recording = false;
            self.clips.recording_held.clear();
            self.sync_clips_hook();
        } else if let Some(code) = windows_code(key) {
            self.clips_record_part(KEYBOARD_KEY, code);
        }
        cx.notify();
        true
    }

    pub(in crate::ui) fn clips_key_up(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) {
        if !self.clips.recording {
            return;
        }
        if let Some(code) = windows_code(&keystroke.key) {
            self.clips_release(KEYBOARD_KEY, code);
        }
        self.clips_modifiers(keystroke.modifiers, cx);
        cx.notify();
    }

    /// gpui reports modifier keys only as a change of the set held.
    pub(in crate::ui) fn clips_modifiers(&mut self, modifiers: Modifiers, cx: &mut Context<Self>) {
        if !self.clips.recording {
            self.clips.modifiers = modifiers;
            return;
        }
        let before = super::voice::modifier_codes(&self.clips.modifiers);
        let after = super::voice::modifier_codes(&modifiers);
        self.clips.modifiers = modifiers;
        for code in &before {
            if !after.contains(code) {
                self.clips_release(KEYBOARD_KEY, *code);
            }
        }
        if self.clips_recording() {
            for code in after {
                if !before.contains(&code) {
                    self.clips_record_part(KEYBOARD_KEY, code);
                }
            }
        }
        cx.notify();
    }

    /// A mouse button while recording becomes part of the keybind. True when the recorder took it.
    pub(in crate::ui) fn clips_mouse(
        &mut self,
        button: MouseButton,
        down: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.clips.recording {
            return false;
        }
        let Some(code) = Root::mouse_code(button) else {
            return false;
        };
        if down && self.clips_recording() {
            self.clips_record_part(MOUSE_BUTTON, code);
            cx.notify();
            return true;
        }
        if !down {
            self.clips_release(MOUSE_BUTTON, code);
            cx.notify();
        }
        false
    }

    fn clips_record_part(&mut self, kind: u32, code: u32) {
        if self.clips.recording_held.contains(&[kind, code]) {
            return;
        }
        self.clips.recording_held.push([kind, code]);
        self.clips.keybind = self
            .clips
            .recording_held
            .iter()
            .map(|[kind, code]| [*kind, *code, super::voice::WINDOWS])
            .collect();
        self.clips.save_keybind();
        self.sync_clips_hook();
    }

    /// The desktop recorder takes one combo and ends when the last of its keys is let go.
    fn clips_release(&mut self, kind: u32, code: u32) {
        let before = self.clips.recording_held.len();
        self.clips
            .recording_held
            .retain(|held| *held != [kind, code]);
        if before > 0 && self.clips.recording_held.is_empty() {
            self.clips.recording = false;
            self.sync_clips_hook();
        }
    }

    /// The harness's `clips` steps: `clips get`, and `clips location <path>` for the answer a folder dialog would give.
    #[cfg(feature = "capture")]
    pub(crate) fn script_clips(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        match words {
            ["get"] => format!(
                "{} keybind {} recording {}",
                self.clips.store.to_value(),
                shortcut_text(&self.clips.keybind),
                self.clips.recording
            ),
            ["location", path @ ..] if !path.is_empty() => {
                if !self.clips_enabled() {
                    return "Edit Location is inert while Enable Clipping is off".to_owned();
                }
                let path = PathBuf::from(path.join(" "));
                self.clips_set_location(&path, cx);
                format!("location = {}", path.display())
            }
            // Enable Clipping, as the switch does (the account must be Nitro: `premium 2`).
            ["enable"] => {
                self.clips.store.set_enabled(true);
                self.clips.save();
                self.clips_sync_engine();
                "enabled".to_owned()
            }
            // The save keybind, as the hook would deliver it (the harness cannot press a global key).
            ["save"] => {
                let outcome = match self.clips_save(cx) {
                    engine::SaveStart::Started => "started".to_owned(),
                    engine::SaveStart::NoOp(reason) => format!("no-op {}", reason.code()),
                };
                format!("save {outcome}")
            }
            // Gives the clip path a visible game without the scan (which would also send presence): the harness
            // window of a stand-in process, or none.
            #[cfg(windows)]
            ["game", "off"] => {
                self.clips.engine.test_game = true;
                self.clips_game_changed(None, cx);
                "game off".to_owned()
            }
            // A test game without a window: the pattern source stands in for the capture.
            #[cfg(windows)]
            ["game", "test", name @ ..] => {
                self.clips.engine.test_game = true;
                let game = crate::clips_capture::Game {
                    pid: 0,
                    window: crate::clips_capture::TEST_WINDOW,
                    name: name.join(" "),
                    application_id: None,
                };
                self.clips_game_changed(Some(game), cx);
                "game test".to_owned()
            }
            #[cfg(windows)]
            ["game", pid, name @ ..] => {
                let Ok(pid) = pid.parse::<u32>() else {
                    return format!("bad pid {pid}");
                };
                let Some(window) = dc_detect::main_window(pid) else {
                    return format!("pid {pid} has no main window");
                };
                self.clips.engine.test_game = true;
                let game = crate::clips_capture::Game {
                    pid,
                    window,
                    name: name.join(" "),
                    application_id: None,
                };
                self.clips_game_changed(Some(game), cx);
                format!("game {pid} window {window:#x}")
            }
            // The page's controls by their setters, for what the page's scroll cannot reach in a capture run:
            // `clips set length|rate|resolution <index into the select's options>` and `clips set percent <1-100>`.
            ["set", what, value] => {
                let Ok(number) = value.parse::<usize>() else {
                    return format!("bad value {value}");
                };
                match *what {
                    "length" => self.clips_choose(ClipSelect::Length, number),
                    "rate" => self.clips_choose(ClipSelect::FrameRate, number),
                    "resolution" => self.clips_choose(ClipSelect::Resolution, number),
                    "percent" => self.clips_set_percent(
                        number.clamp(QUALITY_MIN as usize, QUALITY_MAX as usize) as u8,
                    ),
                    _ => return format!("unknown setting {what}"),
                }
                self.clips.save();
                self.clips_sync_engine();
                cx.notify();
                format!("{what} = {value}")
            }
            ["status"] => self.clips_status_line(),
            ["list"] => self.clips_list_line(),
            _ => {
                "usage: clips get | clips location <path> | clips save | clips status | clips list"
                    .to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_stores() {
        let settings = ClipsSettings::default();
        assert!(!settings.clips_enabled);
        assert_eq!(settings.storage_location, "default");
        assert_eq!(settings.clips_quality.resolution, Resolution::P1080);
        assert_eq!(settings.clips_quality.frame_rate, FrameRate::Fps60);
        assert_eq!(settings.clips_quality.bitrate_percent, 75);
        assert_eq!(settings.clips_length.millis(), 30_000);
        assert!(settings.reminders_enabled);
        assert!(!settings.decoupled_clips_enabled);
        assert_eq!(settings.max_auto_clips, 20);
        assert!(settings.clip_signals.enable_distributed_signals);
        assert!(settings.clip_signals.enable_game_signals);
        assert_eq!(settings.enable_autoclipping, None);
        assert_eq!(ClipsStore::default().enabled_source, EnabledSource::Unset);
    }

    #[test]
    fn a_saved_store_comes_back_whole() {
        let mut store = ClipsStore::default();
        store.set_enabled(true);
        store.settings.clips_length = ClipLength::Minutes2;
        store.settings.clips_quality = ClipsQuality {
            resolution: Resolution::Source,
            frame_rate: FrameRate::Fps30,
            bitrate_percent: 40,
        };
        store.settings.reminders_enabled = false;
        store.settings.storage_location = "D:\\Clips".to_owned();
        let value = store.to_value();
        assert_eq!(value["clipsSettings"]["clipsLength"], 120_000);
        assert_eq!(value["clipsSettings"]["clipsQuality"]["resolution"], 0);
        assert_eq!(
            value["clipsDefaultSettingsState"]["clipsEnabledSource"],
            "user"
        );
        assert_eq!(ClipsStore::from_value(&value), store);
        // Through the file's own text as well.
        let text = serde_json::to_string(&value).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(ClipsStore::from_value(&parsed), store);
    }

    #[test]
    fn missing_or_old_fields_read_as_the_migrations_fill_them() {
        assert_eq!(ClipsStore::from_value(&Value::Null), ClipsStore::default());
        // A quality that is a bare number (migration 10) is the default one; a 15 FPS rate becomes 30 (migration 19).
        let old = json!({"clipsSettings": {"clipsQuality": 5, "clipsEnabled": true}});
        let store = ClipsStore::from_value(&old);
        assert_eq!(
            store.settings.clips_quality,
            ClipsSettings::default().clips_quality
        );
        assert!(store.settings.clips_enabled);
        let fifteen = json!({"clipsSettings": {"clipsQuality":
            {"resolution": 720, "frameRate": 15, "bitratePercent": 300}}});
        let quality = ClipsStore::from_value(&fifteen).settings.clips_quality;
        assert_eq!(quality.frame_rate, FrameRate::Fps30);
        assert_eq!(quality.resolution, Resolution::P720);
        assert_eq!(quality.bitrate_percent, 100);
        // A length the page does not list falls back to 30 seconds.
        let odd = json!({"clipsSettings": {"clipsLength": 45_000, "storageLocation": ""}});
        let store = ClipsStore::from_value(&odd);
        assert_eq!(store.settings.clips_length, ClipLength::Seconds30);
        assert_eq!(store.settings.storage_location, "default");
    }

    #[test]
    fn choosing_enable_clipping_makes_the_choice_the_users() {
        let mut store = ClipsStore::default();
        store.set_enabled(true);
        assert!(store.settings.clips_enabled);
        assert_eq!(store.enabled_source, EnabledSource::User);
    }

    #[test]
    fn the_default_location_is_discord_clips_under_videos() {
        let videos = PathBuf::from("C:\\Users\\someone\\Videos");
        let documents = PathBuf::from("C:\\Users\\someone\\Documents");
        assert_eq!(
            resolve_default_location(Some(videos.clone()), Some(documents.clone())).as_deref(),
            Some(videos.join("Discord Clips").to_string_lossy().as_ref())
        );
        // Without a Videos folder the store falls back to Documents itself.
        assert_eq!(
            resolve_default_location(None, Some(documents.clone())).as_deref(),
            Some(documents.to_string_lossy().as_ref())
        );
        assert_eq!(resolve_default_location(None, None), None);
    }

    #[test]
    fn the_select_options_are_the_pages() {
        assert_eq!(
            ClipSelect::Length.labels(),
            ["30 Seconds", "1 Minute", "2 Minutes"]
        );
        assert_eq!(ClipSelect::FrameRate.labels(), ["30FPS", "60FPS"]);
        assert_eq!(
            ClipSelect::Resolution.labels(),
            ["480p", "720p", "1080p", "1440p", "Source"]
        );
    }

    #[test]
    fn the_slider_steps_in_whole_percents_from_10_to_100() {
        assert_eq!(quality_at(0.), 10);
        assert_eq!(quality_at(1.), 100);
        assert_eq!(quality_at(-3.), 10);
        assert_eq!(quality_at(quality_fraction(75)), 75);
        // The marks sit where their values are on the track.
        assert_eq!(quality_fraction(10), 0.);
        assert_eq!(quality_fraction(100), 1.);
        assert!((quality_fraction(75) - 65. / 90.).abs() < 1e-6);
        for percent in 10..=100u8 {
            assert_eq!(quality_at(quality_fraction(percent)), percent);
        }
    }

    #[test]
    fn the_keybind_defaults_to_alt_c() {
        let keybind = default_keybind();
        assert_eq!(shortcut_text(&keybind), "ALT + C");
        assert_eq!(keybind_from_value(&json!(keybind)), Some(keybind));
        assert_eq!(keybind_from_value(&json!("alt+c")), None);
        assert_eq!(keybind_from_value(&json!([[0, 1]])), None);
    }

    #[test]
    fn the_page_step_is_a_tenth_of_the_range() {
        assert_eq!(PAGE_STEP, 9);
    }

    #[test]
    fn the_stored_state_reaches_a_state_without_a_window() {
        let mut store = ClipsStore::default();
        store.set_enabled(true);
        let state = ClipsState::detached(store.clone());
        assert_eq!(state.settings(), &store.settings);
        assert!(!state.recording);
    }
}
