//! The Go Live picker (module 742527, strings of 254429): what to share, at which quality, and the stored choices
//! (`ApplicationStreamingSettingStore`, kept in storage.json). Measured in `design/dom/popouts/stream-modal-*`.
//!
//! The pure rules (presets, what a subscription allows, the badge text, how a choice becomes capture settings) are
//! plain functions so they are tested without a window. The window thumbnails refresh every second on a worker.

use super::video_tiles::render_image;
use super::{Root, color, icon};
use dc_media::screen::{self, Resolution, Settings, Source, SourceId, StreamQuality};
use dc_state::premium::{Feature, PremiumType};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, RenderImage, SharedString, deferred, div,
    prelude::*, px, relative, rgba,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const STORE_KEY: &str = "ApplicationStreamingSettingStore";
/// `thumbnailSize` of the picker: 447x251 pixels.
const THUMB: (u32, u32) = (447, 251);
/// The source list refreshes every second (every two while games run, which this client does not detect).
const REFRESH: Duration = Duration::from_millis(1000);

// ---- pure rules ----

/// `StreamPreset` of module 753070.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Preset {
    /// "Gaming": smoother video.
    Video = 1,
    /// "Screenshare": clearer text.
    Documents = 2,
    Custom = 3,
}

impl Preset {
    fn from_number(n: u64) -> Self {
        match n {
            2 => Self::Documents,
            3 => Self::Custom,
            _ => Self::Video,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Video => "Gaming",
            Self::Documents => "Screenshare",
            Self::Custom => "Custom",
        }
    }

    fn subtext(self) -> Option<&'static str> {
        match self {
            Self::Video => Some("Smoother video (720p, 30fps)"),
            Self::Documents => Some("Clearer text (Source, 5fps)"),
            Self::Custom => None,
        }
    }

    fn summary(self) -> Option<&'static str> {
        match self {
            Self::Video => Some("Smoother video"),
            Self::Documents => Some("Clearer text"),
            Self::Custom => None,
        }
    }
}

/// The stored choices with the official defaults: preset Video, 720p, 30 fps, share the sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Prefs {
    pub preset: Preset,
    pub resolution: Resolution,
    pub fps: u32,
    /// `soundshareEnabled`: "Mute stream audio" is its inverse.
    pub sound: bool,
    /// "Hide stream preview" (Advanced).
    pub hide_preview: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            preset: Preset::Video,
            resolution: Resolution::P720,
            fps: 30,
            sound: true,
            hide_preview: false,
        }
    }
}

impl Prefs {
    pub fn load() -> Self {
        Self::from_value(crate::storage::get(STORE_KEY).as_ref())
    }

    pub fn save(&self) {
        crate::storage::set(
            STORE_KEY,
            json!({
                "preset": self.preset as u64,
                "resolution": self.resolution.height(),
                "fps": self.fps,
                "soundshare": self.sound,
                "hide_preview": self.hide_preview,
            }),
        );
    }

    fn from_value(value: Option<&Value>) -> Self {
        let mut prefs = Self::default();
        let Some(value) = value else {
            return prefs;
        };
        if let Some(n) = value["preset"].as_u64() {
            prefs.preset = Preset::from_number(n);
        }
        if let Some(resolution) = value["resolution"]
            .as_u64()
            .and_then(|h| Resolution::from_height(h as u32))
        {
            prefs.resolution = resolution;
        }
        if let Some(fps) = value["fps"].as_u64().map(|f| f as u32)
            && screen::FRAME_RATES.contains(&fps)
        {
            prefs.fps = fps;
        }
        if let Some(on) = value["soundshare"].as_bool() {
            prefs.sound = on;
        }
        if let Some(on) = value["hide_preview"].as_bool() {
            prefs.hide_preview = on;
        }
        prefs
    }

    /// What the choices ask for: the preset's own pair, or the custom one, held to what the subscription allows.
    pub fn effective(&self, premium: Option<u64>) -> (Resolution, u32) {
        preset_settings(self.preset, self.resolution, self.fps, premium)
    }
}

/// `canUseStreamQuality` for the account: Mid needs Classic or Nitro, High needs Nitro (`premium_type`), the ladder is
/// `dc_media::screen::required_quality`. Source at 5 fps is the Screenshare preset's own pair.
pub(super) fn can_stream(
    premium: Option<u64>,
    resolution: Resolution,
    fps: u32,
    preset: Preset,
) -> bool {
    let Some(need) = screen::required_quality(resolution, fps) else {
        return false;
    };
    if resolution == Resolution::Source && fps == 5 && preset != Preset::Documents {
        return false;
    }
    let tier = PremiumType::from_wire(premium);
    match need {
        StreamQuality::Standard => true,
        StreamQuality::Mid => tier.has(Feature::StreamMidQuality),
        StreamQuality::High => tier.has(Feature::StreamHighQuality),
    }
}

/// `getPresetSettings`: the first pair of the preset's list the account may use.
pub(super) fn preset_settings(
    preset: Preset,
    custom_resolution: Resolution,
    custom_fps: u32,
    premium: Option<u64>,
) -> (Resolution, u32) {
    let list: &[(Resolution, u32)] = match preset {
        Preset::Video => &[
            (Resolution::P1440, 60),
            (Resolution::P1080, 60),
            (Resolution::P720, 60),
            (Resolution::P720, 30),
        ],
        Preset::Documents => &[(Resolution::Source, 15), (Resolution::Source, 5)],
        Preset::Custom => {
            return if can_stream(premium, custom_resolution, custom_fps, Preset::Custom) {
                (custom_resolution, custom_fps)
            } else {
                (Resolution::P720, 30)
            };
        }
    };
    list.iter()
        .copied()
        .find(|(resolution, fps)| can_stream(premium, *resolution, *fps, preset))
        .unwrap_or((Resolution::P720, 30))
}

/// The text of the quality pill on a stream tile: `720P 30FPS`, `SOURCE 15FPS`.
pub(super) fn quality_badge(resolution: Resolution, fps: u32) -> String {
    match resolution {
        Resolution::Source => format!("SOURCE {fps}FPS"),
        other => format!("{}P {fps}FPS", other.height()),
    }
}

/// Capture settings for a source: the wanted pair when the source takes it, else the nearest lower offered pair.
pub(super) fn settings_for(
    source: &Source,
    resolution: Resolution,
    fps: u32,
    audio: bool,
) -> Option<(Settings, Resolution, u32)> {
    if let Some(settings) = Settings::new(source, resolution, fps, true, audio) {
        return Some((settings, resolution, fps));
    }
    // The wanted pair does not fit this source (a 720p window under 1080p): walk the resolutions down.
    let mut order = vec![
        Resolution::P1440,
        Resolution::P1080,
        Resolution::P720,
        Resolution::P480,
    ];
    order.retain(|r| r.height() <= resolution.height() || resolution == Resolution::Source);
    for candidate in order {
        for rate in screen::offered_frame_rates(candidate) {
            if rate > fps {
                continue;
            }
            if let Some(settings) = Settings::new(source, candidate, rate, true, audio) {
                return Some((settings, candidate, rate));
            }
        }
    }
    None
}

// ---- state ----

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum Tab {
    #[default]
    Applications,
    Screen,
    Devices,
}

/// What the Go Live modal holds while it is open.
pub(super) struct State {
    pub tab: Tab,
    pub prefs: Prefs,
    pub sources: Vec<Source>,
    pub thumbs: HashMap<SourceId, Arc<RenderImage>>,
    pub icons: HashMap<SourceId, Arc<RenderImage>>,
    pub devices: Vec<dc_media::camera::CameraDevice>,
    pub selected_device: Option<String>,
    pub hover: Option<usize>,
    pub options: bool,
    pub advanced: bool,
    /// Bumped on each open: a refresh loop of an older open stops.
    pub generation: u64,
}

impl State {
    fn new(generation: u64) -> Self {
        Self {
            tab: Tab::Applications,
            prefs: Prefs::load(),
            sources: Vec::new(),
            thumbs: HashMap::new(),
            icons: HashMap::new(),
            devices: dc_voice::camera_devices(),
            selected_device: None,
            hover: None,
            options: false,
            advanced: false,
            generation,
        }
    }

    /// The sources the current tab lists.
    pub fn listed(&self) -> Vec<&Source> {
        self.sources
            .iter()
            .filter(|source| match self.tab {
                Tab::Applications => matches!(source.id, SourceId::Window(_)),
                Tab::Screen => matches!(source.id, SourceId::Display(_)),
                Tab::Devices => false,
            })
            .collect()
    }
}

// ---- where sources come from ----

#[cfg(not(feature = "capture"))]
fn list_sources() -> Vec<Source> {
    screen::sources().unwrap_or_default()
}

/// Capture runs never list the real screen or real windows: a fixed list with generated thumbnails.
#[cfg(feature = "capture")]
fn list_sources() -> Vec<Source> {
    vec![
        Source {
            id: SourceId::Window(1),
            name: "General | gravecord fixture - Discord".to_owned(),
            width: 1280,
            height: 720,
            refresh_hz: 60,
            icon: Some(fake_icon()),
        },
        Source {
            id: SourceId::Display(1),
            name: "Screen 1".to_owned(),
            width: 1920,
            height: 1080,
            refresh_hz: 60,
            icon: None,
        },
    ]
}

/// A 16px blurple disc standing in for a window's icon.
#[cfg(feature = "capture")]
fn fake_icon() -> dc_media::Picture {
    let mut data = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16i32 {
        for x in 0..16i32 {
            let inside = (2 * x - 15).pow(2) + (2 * y - 15).pow(2) <= 15 * 15;
            data.extend_from_slice(&[88, 101, 242, if inside { 255 } else { 0 }]);
        }
    }
    dc_media::Picture::rgba(16, 16, data).expect("icon")
}

#[cfg(not(feature = "capture"))]
fn list_thumbnail(id: &SourceId) -> Option<dc_media::Picture> {
    screen::thumbnail(id, THUMB.0, THUMB.1).ok()
}

#[cfg(feature = "capture")]
fn list_thumbnail(id: &SourceId) -> Option<dc_media::Picture> {
    let (width, height) = THUMB;
    let shade = match id {
        SourceId::Window(_) => 40u8,
        SourceId::Display(_) => 70u8,
    };
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let v = shade.saturating_add(((x + y) / 8) as u8);
            data.extend_from_slice(&[v, v.saturating_add(10), v.saturating_add(30), 255]);
        }
    }
    dc_media::Picture::rgba(width, height, data)
}

impl Root {
    /// The Share Your Screen buttons of the panel and the bar. While a stream of ours runs the bar's own click stops
    /// it instead (`Tv` of module 596339); the panel button opens the picker.
    pub(super) fn open_go_live(&mut self, cx: &mut Context<Self>) {
        if self.call.is_none() {
            return;
        }
        let generation = self.go_live_generation + 1;
        self.go_live_generation = generation;
        self.go_live = Some(State::new(generation));
        self.go_live_refresh(generation, cx);
        cx.notify();
    }

    pub(super) fn close_go_live(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.go_live.take() {
            for image in state.thumbs.into_values().chain(state.icons.into_values()) {
                cx.drop_image(image, None);
            }
        }
        cx.notify();
    }

    /// Lists the sources and their thumbnails on a worker, once a second, until the modal closes.
    fn go_live_refresh(&mut self, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                let Ok(tab) = this.update(cx, |root, _| {
                    root.go_live
                        .as_ref()
                        .filter(|state| state.generation == generation)
                        .map(|state| state.tab)
                }) else {
                    return;
                };
                let Some(tab) = tab else {
                    return;
                };
                let fetched = cx
                    .background_executor()
                    .spawn(async move {
                        let sources = list_sources();
                        let mut thumbs = Vec::new();
                        let mut icons = Vec::new();
                        for source in &sources {
                            let wanted = match tab {
                                Tab::Applications => matches!(source.id, SourceId::Window(_)),
                                Tab::Screen => matches!(source.id, SourceId::Display(_)),
                                Tab::Devices => false,
                            };
                            if wanted
                                && let Some(image) =
                                    list_thumbnail(&source.id).and_then(|p| render_image(&p))
                            {
                                thumbs.push((source.id, image));
                            }
                            if let Some(image) = source.icon.as_ref().and_then(render_image) {
                                icons.push((source.id, image));
                            }
                        }
                        (sources, thumbs, icons)
                    })
                    .await;
                let alive = this
                    .update(cx, |root, cx| {
                        let Some(state) = root
                            .go_live
                            .as_mut()
                            .filter(|state| state.generation == generation)
                        else {
                            for (_, image) in fetched.1.iter().chain(fetched.2.iter()) {
                                cx.drop_image(image.clone(), None);
                            }
                            return false;
                        };
                        let (sources, thumbs, icons) = fetched;
                        state.sources = sources;
                        for (id, image) in thumbs {
                            if let Some(old) = state.thumbs.insert(id, image) {
                                cx.drop_image(old, None);
                            }
                        }
                        for (id, image) in icons {
                            if let Some(old) = state.icons.insert(id, image) {
                                cx.drop_image(old, None);
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    return;
                }
                cx.background_executor().timer(REFRESH).await;
            }
        })
        .detach();
    }

    /// A source was picked: Go Live with it at the stored quality, at once (module 742527 `onSelect`).
    fn go_live_pick(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(state) = &self.go_live else {
            return;
        };
        let Some(source) = state.listed().get(index).map(|source| (*source).clone()) else {
            return;
        };
        let premium = self.stores.account.premium_type;
        let (resolution, fps) = state.prefs.effective(premium);
        let Some((settings, resolution, fps)) =
            settings_for(&source, resolution, fps, state.prefs.sound)
        else {
            self.put_app_toast("This source can't be shared.".to_owned(), false, cx);
            return;
        };
        let name = source.name.clone();
        let icon = self
            .go_live
            .as_mut()
            .and_then(|state| state.icons.remove(&source.id));
        self.close_go_live(cx);
        // Change Stream: the old stream ends first.
        self.stream_stop(cx);
        self.stream_start(
            dc_voice::StreamSource::Screen(settings),
            super::voice_video::Started {
                name,
                info: source.clone(),
                quality: (resolution, fps),
                icon,
            },
            cx,
        );
    }

    /// Stream Quality or Share Stream Audio changed while streaming: the capture's size, rate and audio are fixed when
    /// it starts, so the stream restarts from the same source with the new choices.
    pub(super) fn restart_stream_with(&mut self, prefs: Prefs, cx: &mut Context<Self>) {
        prefs.save();
        let Some(started) = self.own_started() else {
            return;
        };
        let premium = self.stores.account.premium_type;
        let (resolution, fps) = prefs.effective(premium);
        let Some((settings, resolution, fps)) =
            settings_for(&started.info, resolution, fps, prefs.sound)
        else {
            return;
        };
        self.stream_stop(cx);
        self.stream_start(
            dc_voice::StreamSource::Screen(settings),
            super::voice_video::Started {
                quality: (resolution, fps),
                ..started
            },
            cx,
        );
    }

    /// The modal, over a dimmed window.
    pub(super) fn go_live_layer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.go_live.as_ref()?;
        let premium = self.stores.account.premium_type;
        let nitro = PremiumType::from_wire(premium).has(Feature::StreamHighQuality);
        let (resolution, fps) = state.prefs.effective(premium);
        let hd = state.prefs.preset == Preset::Custom && resolution.height() >= 1080;

        let tab = |tab: Tab, label: &'static str, glyph: &'static str, cx: &mut Context<Root>| {
            let selected = state.tab == tab;
            div()
                .id(SharedString::from(format!("golive-tab-{glyph}")))
                .w(px(298.))
                .h(px(32.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(14.))
                .font_weight(FontWeight(500.))
                .when(selected, |this| {
                    this.bg(color("--background-surface-high"))
                        .text_color(color("--control-secondary-text-default"))
                })
                .when(!selected, |this| {
                    this.text_color(color("--interactive-text-default"))
                        .hover(|style| style.text_color(color("--interactive-text-hover")))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this.go_live.as_mut() {
                        state.tab = tab;
                        state.hover = None;
                    }
                    cx.notify();
                }))
                .child(icon(glyph, 16., "--interactive-icon-default"))
                .child(label)
        };
        let tabs = div()
            .w_full()
            .h(px(40.))
            .p(px(4.))
            .gap(px(4.))
            .rounded(px(12.))
            .bg(color("--background-base-lowest"))
            .flex()
            .child(tab(
                Tab::Applications,
                "Applications",
                "golive_tab_apps",
                cx,
            ))
            .child(tab(Tab::Screen, "Entire Screen", "golive_tab_screen", cx))
            .child(tab(Tab::Devices, "Devices", "golive_tab_devices", cx));

        let cards: Vec<AnyElement> = if state.tab == Tab::Devices {
            state
                .devices
                .iter()
                .enumerate()
                .map(|(index, device)| {
                    let chosen = state.selected_device.as_deref() == Some(device.id.as_str());
                    let id = device.id.clone();
                    self.go_live_card(
                        index,
                        None,
                        device.name.clone(),
                        None,
                        if chosen { "Selected" } else { "Select" },
                        state.hover == Some(index),
                        cx,
                        move |this: &mut Root, cx: &mut Context<Root>| {
                            if let Some(state) = this.go_live.as_mut() {
                                state.selected_device = Some(id.clone());
                            }
                            cx.notify();
                        },
                    )
                })
                .collect()
        } else {
            state
                .listed()
                .into_iter()
                .enumerate()
                .map(|(index, source)| {
                    self.go_live_card(
                        index,
                        state.thumbs.get(&source.id).cloned(),
                        source.name.clone(),
                        state.icons.get(&source.id).cloned(),
                        "Share Screen",
                        state.hover == Some(index),
                        cx,
                        move |this: &mut Root, cx: &mut Context<Root>| this.go_live_pick(index, cx),
                    )
                })
                .collect()
        };
        let empty = cards.is_empty();

        let details = {
            let mut parts: Vec<String> = Vec::new();
            if let Some(summary) = state.prefs.preset.summary() {
                parts.push(summary.to_owned());
            }
            parts.push(match resolution {
                Resolution::Source => "Source".to_owned(),
                other => format!("{}p", other.height()),
            });
            parts.push(format!("{fps}fps"));
            if state.tab == Tab::Devices {
                parts.push(
                    state
                        .selected_device
                        .as_ref()
                        .and_then(|id| state.devices.iter().find(|d| &d.id == id))
                        .map(|device| device.name.clone())
                        .unwrap_or_default(),
                );
                parts.retain(|part| !part.is_empty());
            }
            parts
        };
        let mut detail_row = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-muted"));
        for (index, part) in details.into_iter().enumerate() {
            if index > 0 {
                detail_row = detail_row.child(div().text_color(rgba(0x97979f1f)).child("\u{2022}"));
            }
            detail_row = detail_row.child(part);
        }

        let pill_item = |label: &'static str, selected: bool, premium: bool| {
            div()
                .h(px(32.))
                .px(px(16.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .when(selected, |this| {
                    this.bg(color("--background-surface-high"))
                        .text_color(color("--text-strong"))
                })
                .when(!selected, |this| {
                    this.text_color(if premium {
                        rgba(0xc269c3ff)
                    } else {
                        color("--interactive-text-default")
                    })
                })
                .child(label)
        };
        let sd_hd = div()
            .h(px(40.))
            .p(px(4.))
            .gap(px(4.))
            .rounded(px(12.))
            .bg(color("--background-base-lowest"))
            .flex()
            .child(
                div()
                    .id("golive-sd")
                    .cursor_pointer()
                    .child(pill_item("SD", !hd, false))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.go_live.as_mut() {
                            state.prefs.preset = Preset::Video;
                            state.prefs.save();
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("golive-hd")
                    .cursor_pointer()
                    .child(pill_item("HD", hd, !nitro))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if nitro {
                            if let Some(state) = this.go_live.as_mut() {
                                state.prefs.preset = Preset::Custom;
                                state.prefs.resolution = Resolution::P1080;
                                state.prefs.fps = 60;
                                state.prefs.save();
                            }
                        } else {
                            this.go_live = None;
                            this.open_nitro(cx);
                        }
                        cx.notify();
                    })),
            );
        let gear = div()
            .id("golive-gear")
            .size(px(40.))
            .rounded(px(8.))
            .bg(rgba(0x97979f29))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(rgba(0x97979f3d)))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(state) = this.go_live.as_mut() {
                    state.options = !state.options;
                    state.advanced = false;
                }
                cx.notify();
            }))
            .child(icon("voice_view_gear", 20., "--text-strong"));

        let footer = div()
            .w_full()
            .h(px(112.))
            .flex_none()
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(56.))
                    .px(px(24.))
                    .pt(px(16.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(state.prefs.preset.title()),
                            )
                            .child(detail_row),
                    )
                    .child(div().flex().gap(px(8.)).child(sd_hd).child(gear)),
            )
            .when(!nitro, |this| this.child(self.go_live_ribbon(cx)));

        let menu = state.options.then(|| self.go_live_options(state, cx));

        let modal = div()
            .id("golive-modal")
            .occlude()
            .relative()
            .w(px(960.))
            .h(px(608.))
            .pt(px(8.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(72.))
                    .flex_none()
                    .px(px(24.))
                    .py(px(16.))
                    .child(tabs),
            )
            .child(
                div()
                    .id("golive-content")
                    .h(px(398.))
                    .flex_none()
                    .overflow_y_scroll()
                    .px(px(24.))
                    .py(px(8.))
                    .flex()
                    .flex_wrap()
                    .content_start()
                    .gap_x(px(16.))
                    .gap_y(px(24.))
                    .children(cards)
                    .when(empty, |this| {
                        this.child(
                            div()
                                .w_full()
                                .py(px(48.))
                                .flex()
                                .justify_center()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-muted"))
                                .child(match state.tab {
                                    Tab::Devices => "No devices found.",
                                    _ => "Nothing to share.",
                                }),
                        )
                    }),
            )
            .child(footer)
            .children(menu);
        Some(
            deferred(
                div()
                    .id("golive-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| this.close_go_live(cx)))
                    .child(modal),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// One source: a 439x247 preview with a 2px ring, and its icon and name below. Hover dims it with a button.
    #[allow(clippy::too_many_arguments)]
    fn go_live_card(
        &self,
        index: usize,
        thumb: Option<Arc<RenderImage>>,
        name: String,
        icon_image: Option<Arc<RenderImage>>,
        cta: &'static str,
        hovered: bool,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        let hover_id = index;
        div()
            .id(SharedString::from(format!("golive-card-{index}")))
            .w(px(439.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .cursor_pointer()
            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                if let Some(state) = this.go_live.as_mut() {
                    let next = hovering.then_some(hover_id);
                    if *hovering || state.hover == Some(hover_id) {
                        state.hover = next;
                        cx.notify();
                    }
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                run(this, cx);
            }))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(246.94))
                    .rounded(px(10.))
                    .p(px(2.))
                    .bg(rgba(0x97979f33))
                    .when(hovered, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top(px(-4.))
                                .left(px(-4.))
                                .right(px(-4.))
                                .bottom(px(-4.))
                                .rounded(px(12.))
                                .border_2()
                                .border_color(rgba(0x97979f80)),
                        )
                    })
                    .child(
                        div()
                            .relative()
                            .size_full()
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(rgba(0x000000ff))
                            .children(thumb.map(|image| {
                                gpui::img(image)
                                    .size_full()
                                    .object_fit(gpui::ObjectFit::Contain)
                            }))
                            .when(hovered, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .bg(rgba(0x00000066))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .px(px(24.))
                                                .py(px(12.))
                                                .rounded(px(8.))
                                                .bg(rgba(0xffffffff))
                                                .text_color(rgba(0x000000ff))
                                                .text_size(px(14.))
                                                .line_height(px(18.))
                                                .font_weight(FontWeight(500.))
                                                .child(cta),
                                        ),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .children(icon_image.map(|image| gpui::img(image).size(px(16.)).flex_none()))
                    .child(div().truncate().child(name)),
            )
            .into_any_element()
    }

    /// "Stream in HD resolution with Nitro" (`upsellRibbon`): a gradient bar with Get Nitro.
    fn go_live_ribbon(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .mx(px(16.))
            .mt(px(16.))
            .h(px(40.))
            .px(px(16.))
            .py(px(8.))
            .rounded(px(8.))
            .relative()
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(rgba(0xffffffff))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(relative(0.5))
                    .rounded_l(px(8.))
                    .bg(gpui::linear_gradient(
                        90.,
                        gpui::linear_color_stop(rgba(0x8547c6ff), 0.),
                        gpui::linear_color_stop(rgba(0xb845c1ff), 1.),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .w(relative(0.5))
                    .rounded_r(px(8.))
                    .bg(gpui::linear_gradient(
                        90.,
                        gpui::linear_color_stop(rgba(0xb845c1ff), 0.),
                        gpui::linear_color_stop(rgba(0xab5d8aff), 1.),
                    )),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("golive_lock", 12., "--white"))
                    .child("Stream in HD resolution with ")
                    .child(
                        div()
                            .id("golive-nitro-link")
                            .font_weight(FontWeight(700.))
                            .cursor_pointer()
                            .underline()
                            .child("Nitro")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.go_live = None;
                                this.open_nitro(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(24.))
                            .rounded(px(8.))
                            .bg(rgba(0x0000008f))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("golive_heart", 16., "--white")),
                    )
                    .child(
                        div()
                            .id("golive-get-nitro")
                            .h(px(24.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(rgba(0xffffffff))
                            .text_color(rgba(0x8547c6ff))
                            .text_size(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .cursor_pointer()
                            .font_weight(FontWeight(600.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.go_live = None;
                                this.open_nitro(cx);
                            }))
                            .child(icon("golive_nitro", 16., "--brand-500"))
                            .child("Get Nitro"),
                    ),
            )
            .into_any_element()
    }

    /// The gear menu: Stream Mode, Mute stream audio, Advanced.
    fn go_live_options(&self, state: &State, cx: &mut Context<Self>) -> AnyElement {
        let row = |id: &'static str, preset: Preset, cx: &mut Context<Root>| {
            let selected = state.prefs.preset == preset;
            div()
                .id(id)
                .h(px(if preset.subtext().is_some() { 52. } else { 44. }))
                .px(px(8.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_between()
                .cursor_pointer()
                .hover(|style| style.bg(color("--background-mod-subtle")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this.go_live.as_mut() {
                        state.prefs.preset = preset;
                        state.prefs.save();
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(preset.title()),
                        )
                        .children(preset.subtext().map(|text| {
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(text)
                        })),
                )
                .child(
                    div()
                        .size(px(20.))
                        .rounded_full()
                        .border_2()
                        .border_color(if selected {
                            color("--brand-500")
                        } else {
                            color("--interactive-icon-default")
                        })
                        .when(selected, |this| this.bg(color("--brand-500")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(selected, |this| {
                            this.child(div().size(px(8.)).rounded_full().bg(rgba(0xffffffff)))
                        }),
                )
        };
        let check_row = |id: &'static str,
                         label: &'static str,
                         on: bool,
                         cx: &mut Context<Root>,
                         run: fn(&mut Prefs)| {
            div()
                .id(id)
                .h(px(36.))
                .px(px(8.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_between()
                .cursor_pointer()
                .hover(|style| style.bg(color("--background-mod-subtle")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this.go_live.as_mut() {
                        run(&mut state.prefs);
                        state.prefs.save();
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
                .child(
                    div()
                        .size(px(20.))
                        .rounded(px(4.))
                        .border_2()
                        .border_color(if on {
                            color("--brand-500")
                        } else {
                            color("--interactive-icon-default")
                        })
                        .when(on, |this| this.bg(color("--brand-500")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(on, |this| {
                            this.child(icon("checkbox_check", 14., "--white"))
                        }),
                )
        };
        let menu = div()
            .occlude()
            .w(px(213.56))
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(36.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child("Stream Mode"),
            )
            .child(row("golive-mode-video", Preset::Video, cx))
            .child(row("golive-mode-documents", Preset::Documents, cx))
            .child(row("golive-mode-custom", Preset::Custom, cx))
            .child(
                div()
                    .mt(px(4.))
                    .mb(px(5.))
                    .h(px(1.))
                    .bg(color("--border-subtle")),
            )
            .child(check_row(
                "golive-mute-audio",
                "Mute stream audio",
                !state.prefs.sound,
                cx,
                |prefs| prefs.sound = !prefs.sound,
            ))
            .child(
                div()
                    .id("golive-advanced")
                    .h(px(36.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.go_live.as_mut() {
                            state.advanced = !state.advanced;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Advanced"),
                    )
                    .child(icon("golive_caret", 24., "--interactive-icon-default")),
            );
        let advanced = state.advanced.then(|| {
            div()
                .occlude()
                .w(px(213.56))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .child(check_row(
                    "golive-hide-preview",
                    "Hide stream preview",
                    state.prefs.hide_preview,
                    cx,
                    |prefs| prefs.hide_preview = !prefs.hide_preview,
                ))
        });
        div()
            .absolute()
            .right(px(25.44))
            .bottom(px(121.))
            .flex()
            .items_end()
            .gap(px(8.))
            .children(advanced)
            .child(menu)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(width: u32, height: u32) -> Source {
        Source {
            id: SourceId::Window(7),
            name: "w".into(),
            width,
            height,
            refresh_hz: 60,
            icon: None,
        }
    }

    #[test]
    fn defaults_are_the_official_ones() {
        let prefs = Prefs::default();
        assert_eq!(prefs.preset, Preset::Video);
        assert_eq!((prefs.resolution, prefs.fps), (Resolution::P720, 30));
        assert!(prefs.sound && !prefs.hide_preview);
        assert_eq!(Prefs::from_value(None), prefs);
    }

    #[test]
    fn prefs_round_trip_through_json() {
        let prefs = Prefs {
            preset: Preset::Custom,
            resolution: Resolution::P1080,
            fps: 60,
            sound: false,
            hide_preview: true,
        };
        let value = json!({"preset": 3, "resolution": 1080, "fps": 60, "soundshare": false, "hide_preview": true});
        assert_eq!(Prefs::from_value(Some(&value)), prefs);
        // Junk keeps the defaults.
        let junk = json!({"preset": 99, "fps": 7, "resolution": 5});
        assert_eq!(Prefs::from_value(Some(&junk)), Prefs::default());
    }

    #[test]
    fn the_account_decides_what_is_allowed() {
        use Resolution::*;
        assert!(can_stream(None, P720, 30, Preset::Video));
        assert!(!can_stream(None, P1080, 60, Preset::Video));
        assert!(can_stream(Some(1), P1080, 60, Preset::Custom));
        assert!(!can_stream(Some(1), Source, 30, Preset::Custom));
        assert!(can_stream(Some(2), Source, 60, Preset::Custom));
        // Source at 5 fps belongs to Screenshare only.
        assert!(can_stream(None, Source, 5, Preset::Documents));
        assert!(!can_stream(None, Source, 5, Preset::Video));
        assert!(!can_stream(None, P720, 7, Preset::Custom));
    }

    #[test]
    fn presets_pick_the_first_allowed_pair() {
        use Resolution::*;
        let pick = |preset, premium| preset_settings(preset, P720, 30, premium);
        assert_eq!(pick(Preset::Video, None), (P720, 30));
        assert_eq!(pick(Preset::Video, Some(2)), (P1440, 60));
        assert_eq!(pick(Preset::Video, Some(1)), (P1440, 60));
        assert_eq!(pick(Preset::Documents, None), (Source, 5));
        assert_eq!(pick(Preset::Documents, Some(2)), (Source, 15));
        // A custom pair the account may not use falls back to 720p30.
        assert_eq!(preset_settings(Preset::Custom, P1440, 60, None), (P720, 30));
        assert_eq!(preset_settings(Preset::Custom, P480, 15, None), (P480, 15));
    }

    #[test]
    fn the_badge_reads_like_the_tile_pill() {
        assert_eq!(quality_badge(Resolution::P720, 30), "720P 30FPS");
        assert_eq!(quality_badge(Resolution::Source, 5), "SOURCE 5FPS");
    }

    #[test]
    fn a_small_source_falls_back_to_what_fits() {
        let small = window(1280, 720);
        let (settings, resolution, fps) =
            settings_for(&small, Resolution::P1080, 60, true).unwrap();
        assert!(settings.validate().is_ok());
        assert!(resolution.height() <= 1080);
        assert!(fps <= 60);
        let (_, resolution, fps) =
            settings_for(&window(1920, 1080), Resolution::P720, 30, false).unwrap();
        assert_eq!((resolution, fps), (Resolution::P720, 30));
    }

    #[test]
    fn the_tabs_list_their_own_kinds() {
        let mut state = State::new(1);
        state.sources = vec![
            window(100, 100),
            Source {
                id: SourceId::Display(1),
                ..window(1920, 1080)
            },
        ];
        assert_eq!(state.listed().len(), 1);
        state.tab = Tab::Screen;
        assert!(matches!(state.listed()[0].id, SourceId::Display(1)));
        state.tab = Tab::Devices;
        assert!(state.listed().is_empty());
    }
}
