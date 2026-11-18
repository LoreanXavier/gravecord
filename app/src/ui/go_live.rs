//! The Go Live picker (`tJ` of module 742527, strings of 254429): what to share, at which quality, and the stored
//! choices (`ApplicationStreamingSettingsStore`, module 929921, kept in storage.json under its persist key). Measured in
//! `design/dom/popouts/stream-modal-*`.
//!
//! The pure rules (the quality table, what an account and the call's server allow, the presets, how a choice becomes
//! capture settings) are plain functions so they are tested without a window. The sources are listed off the UI
//! thread the moment the picker opens, and each thumbnail is delivered as soon as it is ready.

use super::video_tiles::render_image;
use super::{Root, color, icon};
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use dc_media::screen::{self, Resolution, Settings, Source, SourceId};
use dc_state::premium::{Feature, PremiumType};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseDownEvent, RenderImage, SharedString,
    Window, deferred, div, prelude::*, px, relative, rgba, svg,
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::debug;

const STORE_KEY: &str = "ApplicationStreamingSettingStore";
/// `thumbnailSize` of the picker: 447x251 pixels.
const THUMB: (u32, u32) = (447, 251);
/// The source list is asked for again a second after the last answer (two while games run, which this client does
/// not detect): `l.start(n, e)` of `tJ`.
const REFRESH: Duration = Duration::from_millis(1000);
/// `GUILD_BOOSTING_PINK` (module 646514): the Nitro wheel beside the premium choices.
const BOOSTING_PINK: u32 = 0xff73faff;

/// The Nitro wheel in the `PREMIUM_TIER_2` gradient of module 404374.
pub(in crate::ui) fn nitro_ribbon_icon() -> Arc<gpui::Image> {
    static IMAGE: std::sync::LazyLock<Arc<gpui::Image>> = std::sync::LazyLock::new(|| {
        Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Svg,
            include_bytes!("../../assets/icons/golive_nitro_gradient.svg").to_vec(),
        ))
    });
    IMAGE.clone()
}
/// `getArticleURL(0x53d41ade17)` of the empty Devices tab.
const DEVICES_HELP: &str = "https://support.discord.com/hc/en-us/articles/360040816151";

// ---- pure rules ----

/// `StreamPreset` of module 753070 (the desktop ones; Auto stays off while `2025-10-go-live-auto-quality` is not
/// assigned, which this client does not read).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Preset {
    /// "Gaming": smoother video.
    Video = 1,
    /// "Screenshare": clearer text.
    Documents = 2,
    Custom = 3,
}

impl Preset {
    fn from_number(n: u64) -> Option<Self> {
        match n {
            1 => Some(Self::Video),
            2 => Some(Self::Documents),
            3 => Some(Self::Custom),
            _ => None,
        }
    }

    /// `to` of module 742527.
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Video => dc_intl::tr("Gaming"),
            Self::Documents => dc_intl::tr("Screenshare"),
            Self::Custom => dc_intl::tr("Custom"),
        }
    }

    /// The footer's first word (`v` of `tq`): none for Custom.
    fn summary(self) -> Option<&'static str> {
        match self {
            Self::Video => Some(dc_intl::tr("Smoother video")),
            Self::Documents => Some(dc_intl::tr("Clearer text")),
            Self::Custom => None,
        }
    }
}

/// What a row of the quality table asks of the account: `MID_STREAMING_QUALITY` or `HIGH_STREAMING_QUALITY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Need {
    Mid,
    High,
}

/// One row of `ZV` (module 753070): a pair, the preset it belongs to, and what unlocks it.
struct Rule {
    resolution: Resolution,
    fps: u32,
    preset: Option<Preset>,
    quality: Option<Need>,
    /// `guildPremiumTier`: the call's server at this boost tier or above unlocks it too.
    tier: Option<u64>,
}

const fn rule(
    resolution: Resolution,
    fps: u32,
    preset: Option<Preset>,
    quality: Option<Need>,
    tier: Option<u64>,
) -> Rule {
    Rule {
        resolution,
        fps,
        preset,
        quality,
        tier,
    }
}

/// `ZV` of module 753070, in its order. `TVA.TIER_1` is 1 and `TIER_2` is 2.
const RULES: [Rule; 18] = {
    use Resolution::*;
    [
        rule(Source, 60, None, Some(Need::High), None),
        rule(Source, 30, None, Some(Need::High), None),
        rule(Source, 15, None, Some(Need::High), None),
        rule(Source, 5, Some(Preset::Documents), None, None),
        rule(P1440, 60, None, Some(Need::Mid), Some(2)),
        rule(P1440, 30, None, Some(Need::Mid), Some(2)),
        rule(P1440, 15, None, Some(Need::Mid), Some(2)),
        rule(P1080, 60, None, Some(Need::Mid), Some(2)),
        rule(P1080, 30, None, Some(Need::Mid), Some(2)),
        rule(P1080, 15, None, Some(Need::Mid), Some(2)),
        rule(P720, 60, None, Some(Need::Mid), Some(1)),
        rule(P720, 30, None, None, None),
        rule(P720, 15, None, None, None),
        rule(P720, 5, None, None, None),
        rule(P480, 60, None, Some(Need::Mid), Some(1)),
        rule(P480, 30, None, None, None),
        rule(P480, 15, None, None, None),
        rule(P480, 5, None, None, None),
    ]
};

/// Who streams: the subscription (`premium_type`, 0 none, 1 Classic, 2 Nitro, 3 Basic) and the boost tier of the
/// server of the call (`premiumTier`, none in a DM call).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Account {
    pub premium: Option<u64>,
    pub guild_tier: Option<u64>,
}

impl Account {
    /// `canStreamQuality` of module 722178: Mid with `STREAM_MID_QUALITY` (Classic, Nitro), High with
    /// `STREAM_HIGH_QUALITY` (Nitro).
    fn can(self, need: Need) -> bool {
        let tier = PremiumType::from_wire(self.premium);
        match need {
            Need::Mid => tier.has(Feature::StreamMidQuality),
            Need::High => tier.has(Feature::StreamHighQuality),
        }
    }

    /// The SD/HD switch and the "Stream in HD resolution with Nitro" ribbon show while High is out of reach (`X` of
    /// `tJ`).
    pub(super) fn needs_hd_upsell(self) -> bool {
        !self.can(Need::High)
    }
}

/// `canStreamWithSettings` (module 676030): a row of the table for the pair (of this preset, or of any) that the
/// account or the server unlocks.
pub(super) fn can_stream(
    account: Account,
    preset: Preset,
    resolution: Resolution,
    fps: u32,
) -> bool {
    RULES.iter().any(|row| {
        row.preset.is_none_or(|own| own == preset)
            && row.resolution == resolution
            && row.fps == fps
            && (row.quality.is_none() && row.tier.is_none()
                || row.quality.is_some_and(|need| account.can(need))
                || row
                    .tier
                    .is_some_and(|need| account.guild_tier.is_some_and(|tier| tier >= need)))
    })
}

pub(super) fn option_shows_premium(
    account: Account,
    preset: Preset,
    resolution: Resolution,
    fps: u32,
) -> bool {
    !can_stream(account, preset, resolution, fps)
}

/// `getStreamSettingsForPreset` (module 476697): the first pair of the preset's list the account may use; none for
/// Custom, whose pair is the chosen one.
pub(super) fn preset_settings(preset: Preset, account: Account) -> Option<(Resolution, u32)> {
    let list: &[(Resolution, u32)] = match preset {
        Preset::Video => &[
            (Resolution::P1440, 60),
            (Resolution::P1080, 60),
            (Resolution::P720, 60),
            (Resolution::P720, 30),
        ],
        Preset::Documents => &[(Resolution::Source, 15), (Resolution::Source, 5)],
        Preset::Custom => &[],
    };
    list.iter()
        .copied()
        .find(|(resolution, fps)| can_stream(account, preset, *resolution, *fps))
}

/// `tc`: "Source" or "720p".
pub(super) fn resolution_text(resolution: Resolution) -> String {
    match resolution {
        Resolution::Source => dc_intl::tr("Source").to_owned(),
        other => format!("{}p", other.height()),
    }
}

/// The subtext of a Stream Mode row (`G5O1Mz`, `8tcFL5`): the preset's pair for this account, or the menu's own
/// defaults (720p 30 and Source 15) when it has none.
pub(super) fn preset_subtext(preset: Preset, account: Account) -> Option<String> {
    match preset {
        Preset::Video => {
            let (resolution, fps) =
                preset_settings(preset, account).unwrap_or((Resolution::P720, 30));
            Some(format!(
                "Smoother video ({}, {fps}fps)",
                resolution_text(resolution)
            ))
        }
        Preset::Documents => {
            let (resolution, fps) =
                preset_settings(preset, account).unwrap_or((Resolution::Source, 15));
            Some(format!(
                "Clearer text ({}, {fps}fps)",
                resolution_text(resolution)
            ))
        }
        Preset::Custom => None,
    }
}

/// The text of the quality pill on a stream tile (module 872363, `text-transform: none`): `720p 30FPS`,
/// `Source 15FPS` ("Source" is `XjXqzh`).
pub(super) fn quality_badge(resolution: Resolution, fps: u32) -> String {
    quality_text(
        match resolution {
            Resolution::Source => None,
            other => Some(other.height()),
        },
        fps,
    )
}

/// [`quality_badge`] from a height (`None` for the source's own size) and a frame rate.
pub(super) fn quality_text(height: Option<u32>, fps: u32) -> String {
    match height {
        Some(height) => format!("{height}p {fps}FPS"),
        None => format!("{} {fps}FPS", dc_intl::t("XjXqzh")),
    }
}

/// Module 872363 `g = HB(k) || OI(k)`: the pill carries the Nitro wheel when the frame rate or the resolution is not
/// one a free account may stream (650338 against the table of 753070): 60 FPS, or 1080p, 1440p or Source at any rate
/// but 5. The streamer's or the viewer's Nitro does not enter.
pub(super) fn quality_is_premium(height: Option<u32>, fps: u32) -> bool {
    fps >= 60 || (fps != 5 && height.is_none_or(|height| height >= 1080))
}

/// [`quality_is_premium`] read back from a pill's text.
pub(super) fn label_is_premium(label: &str) -> bool {
    let mut parts = label.split(' ');
    let (Some(resolution), Some(rate)) = (parts.next(), parts.next()) else {
        return false;
    };
    let Some(fps) = rate.strip_suffix("FPS").and_then(|fps| fps.parse().ok()) else {
        return false;
    };
    let height = resolution
        .strip_suffix('p')
        .and_then(|height| height.parse().ok());
    quality_is_premium(height, fps)
}

/// The stored choices of `ApplicationStreamingSettingsStore`: preset Video, 720p, 30 fps and the sound by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Prefs {
    pub preset: Preset,
    pub resolution: Resolution,
    pub fps: u32,
    /// `soundshareEnabled`: "Mute stream audio" is its inverse.
    pub sound: bool,
    /// "Hide stream preview" (Advanced). The official keeps it in the synced user settings; here it stays local.
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

    /// `STREAM_UPDATE_SETTINGS` / `MEDIA_ENGINE_SET_GO_LIVE_SOURCE`: the store keeps what a stream started with.
    pub fn save(&self) {
        crate::storage::set(
            STORE_KEY,
            json!({
                "preset": self.preset as u64,
                "resolution": self.resolution.height(),
                "fps": self.fps,
                "soundshareEnabled": self.sound,
                "hide_preview": self.hide_preview,
            }),
        );
    }

    fn from_value(value: Option<&Value>) -> Self {
        let mut prefs = Self::default();
        let Some(value) = value else {
            return prefs;
        };
        // `initialize`: an unknown preset reads as Video.
        if let Some(preset) = value["preset"].as_u64().and_then(Preset::from_number) {
            prefs.preset = preset;
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
        // `soundshare` is how this client stored it before.
        if let Some(on) = value["soundshareEnabled"]
            .as_bool()
            .or_else(|| value["soundshare"].as_bool())
        {
            prefs.sound = on;
        }
        if let Some(on) = value["hide_preview"].as_bool() {
            prefs.hide_preview = on;
        }
        prefs
    }

    /// `useCreateGoLiveModalState`: the stored choices, with a custom pair the account may not use put back to 720p
    /// 30.
    pub(super) fn for_modal(self, account: Account) -> Self {
        let mut prefs = self;
        if !can_stream(account, Preset::Custom, prefs.resolution, prefs.fps) {
            prefs.resolution = Resolution::P720;
            prefs.fps = 30;
        }
        prefs
    }

    /// What the footer and the stream show: the preset's pair (`tm`), or the chosen one for Custom.
    pub(super) fn shown(&self, account: Account) -> (Resolution, u32) {
        preset_settings(self.preset, account).unwrap_or((self.resolution, self.fps))
    }

    /// `startStreamWithSource` (module 474834): the preset's pair; a custom pair the account may not use becomes
    /// Video at 720p 30.
    pub(super) fn start_choice(&self, account: Account) -> (Preset, Resolution, u32) {
        match preset_settings(self.preset, account) {
            Some((resolution, fps)) => (self.preset, resolution, fps),
            None if can_stream(account, self.preset, self.resolution, self.fps) => {
                (self.preset, self.resolution, self.fps)
            }
            None => (Preset::Video, Resolution::P720, 30),
        }
    }
}

/// A Frame Rate choice of the in-call Stream Quality menu (`L` of module 600125): whether the account may use it at
/// the stored resolution, and that resolution.
pub(super) fn frame_rate_choice(prefs: &Prefs, account: Account, fps: u32) -> (bool, Resolution) {
    (
        can_stream(account, Preset::Custom, prefs.resolution, fps),
        prefs.resolution,
    )
}

/// A Resolution choice of the same menu (`V`): with Screenshare's 5 fps the frame rate becomes 30 (`D`).
pub(super) fn resolution_choice(
    prefs: &Prefs,
    account: Account,
    resolution: Resolution,
) -> (bool, u32) {
    let fps = if prefs.preset == Preset::Documents {
        30
    } else {
        prefs.fps
    };
    (can_stream(account, Preset::Custom, resolution, fps), fps)
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

/// `eA` renames the native screen names: "Entire screen" and "Screen N".
fn screen_name(name: &str) -> String {
    if name == "Entire screen" {
        return dc_intl::tr("Entire Screen").to_owned();
    }
    match name.strip_prefix("Screen ").map(str::parse::<u32>) {
        Some(Ok(index)) => format!("Screen {index}"),
        _ => name.to_owned(),
    }
}

// ---- state ----

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum Tab {
    #[default]
    Applications,
    Screen,
    Devices,
}

impl Tab {
    fn lists(self, id: &SourceId) -> bool {
        match self {
            Tab::Applications => matches!(id, SourceId::Window(_)),
            Tab::Screen => matches!(id, SourceId::Display(_)),
            Tab::Devices => false,
        }
    }
}

/// The submenu of the options menu that is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Sub {
    Resolution,
    FrameRate,
    Advanced,
}

/// What the content area shows (`e7`): the spinner until the first list arrives, the empty Devices page, or the
/// cards (an empty grid when there are none, as the official draws).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Content {
    Loading,
    NoDevices,
    Cards,
}

pub(super) fn content(fetching: bool, tab: Tab, devices: usize) -> Content {
    if fetching {
        Content::Loading
    } else if tab == Tab::Devices && devices == 0 {
        Content::NoDevices
    } else {
        Content::Cards
    }
}

/// Which thumbnails a refresh takes, in order: every source of the shown tab (screens first on Entire Screen), then
/// the other tab's sources that have none yet, so switching tabs never shows an empty preview.
pub(super) fn thumb_order(
    sources: &[SourceId],
    tab: Tab,
    have: &HashSet<SourceId>,
) -> Vec<SourceId> {
    let mut order: Vec<SourceId> = sources.iter().copied().filter(|id| tab.lists(id)).collect();
    for id in sources {
        if !tab.lists(id) && !have.contains(id) {
            order.push(*id);
        }
    }
    order
}

/// The last listing, kept between opens so the picker paints at once: sources, thumbnails and icons.
#[derive(Default)]
struct Listing {
    sources: Vec<Source>,
    thumbs: HashMap<SourceId, Arc<RenderImage>>,
    icons: HashMap<SourceId, Arc<RenderImage>>,
    devices: Vec<dc_media::camera::CameraDevice>,
    /// A first list arrived in this run of the client.
    fetched: bool,
}

thread_local! {
    static LISTING: RefCell<Listing> = RefCell::new(Listing::default());
    /// The Nitro upsell of streaming quality (module 826789) is open; `true` when closing it closes the picker too
    /// (`tf({onClose: t})`).
    static UPSELL: Cell<Option<bool>> = const { Cell::new(None) };
}

/// What one card of the grid shows.
struct Card {
    id_prefix: &'static str,
    index: usize,
    thumb: Option<Arc<RenderImage>>,
    name: String,
    icon: Option<Arc<RenderImage>>,
    fallback_icon: &'static str,
    cta: &'static str,
    hovered: bool,
    selected: bool,
}

/// What the Go Live modal holds while it is open.
pub(super) struct State {
    pub game: Option<super::go_live_game::GameShare>,
    pub tab: Tab,
    pub prefs: Prefs,
    pub selected_device: Option<String>,
    pub hover: Option<usize>,
    pub options: bool,
    pub sub: Option<Sub>,
    /// `fetchingSources`: no list has arrived yet.
    pub fetching: bool,
    /// Bumped on each open: a refresh loop of an older open stops.
    pub generation: u64,
    pub opened_at: Instant,
}

impl State {
    pub(super) fn new(generation: u64, account: Account) -> Self {
        Self {
            game: None,
            tab: Tab::Applications,
            prefs: Prefs::load().for_modal(account),
            selected_device: None,
            hover: None,
            options: false,
            sub: None,
            fetching: !LISTING.with(|listing| listing.borrow().fetched),
            generation,
            opened_at: Instant::now(),
        }
    }

    /// The sources the current tab lists: their ids and shown names.
    pub fn listed(&self) -> Vec<(SourceId, String)> {
        LISTING.with(|listing| {
            listing
                .borrow()
                .sources
                .iter()
                .filter(|source| self.tab.lists(&source.id))
                .map(|source| {
                    let name = match source.id {
                        SourceId::Display(_) => screen_name(&source.name),
                        SourceId::Window(_) => source.name.clone(),
                    };
                    (source.id, name)
                })
                .collect()
        })
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
            name: "Entire screen".to_owned(),
            width: 1920,
            height: 1080,
            refresh_hz: 60,
            icon: None,
        },
    ]
}

#[cfg(not(feature = "capture"))]
fn list_devices() -> Vec<dc_media::camera::CameraDevice> {
    dc_voice::camera_devices()
}

/// Capture runs list no camera.
#[cfg(feature = "capture")]
fn list_devices() -> Vec<dc_media::camera::CameraDevice> {
    Vec::new()
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

/// A picture with nothing in it: a capture's first frame can come back before the source drew. It is not shown in
/// place of a real one, and the next refresh asks again.
pub(super) fn is_blank(picture: &dc_media::Picture) -> bool {
    matches!(picture.layout, dc_media::Layout::Rgba)
        && picture
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[0] < 3 && pixel[1] < 3 && pixel[2] < 3)
}

/// The listed sources of the last listing, for the picker's refresh order.
fn listing_order(tab: Tab) -> Vec<SourceId> {
    LISTING.with(|listing| {
        let listing = listing.borrow();
        let sources: Vec<SourceId> = listing.sources.iter().map(|source| source.id).collect();
        let have: HashSet<SourceId> = listing.thumbs.keys().copied().collect();
        thumb_order(&sources, tab, &have)
    })
}

impl Root {
    /// Who streams, for the quality rules: the subscription and the call's server's boost tier.
    pub(super) fn stream_account(&self) -> Account {
        let guild_tier = self
            .call
            .as_ref()
            .filter(|call| call.guild != 0)
            .and_then(|call| {
                self.stores
                    .guilds
                    .iter()
                    .find(|guild| guild.id == call.guild)
            })
            .map(|guild| guild.premium_tier);
        Account {
            premium: self.premium_wire(),
            guild_tier,
        }
    }

    /// The Share Your Screen buttons of the panel and the bar, and Change Stream. Lists the sources at once.
    pub(super) fn open_go_live(&mut self, cx: &mut Context<Self>) {
        if self.call.is_none() {
            return;
        }
        let generation = self.go_live_generation + 1;
        self.go_live_generation = generation;
        let account = self.stream_account();
        self.go_live = Some(State::new(generation, account));
        self.go_live_refresh(generation, cx);
        cx.notify();
    }

    /// Closes the picker. The listing and its pictures stay for the next open.
    pub(super) fn close_go_live(&mut self, cx: &mut Context<Self>) {
        self.go_live = None;
        cx.notify();
    }

    /// Opens the streaming upsell (`tf`): `close_picker` when closing it closes the picker too.
    pub(super) fn open_stream_upsell(&mut self, close_picker: bool, cx: &mut Context<Self>) {
        UPSELL.with(|upsell| upsell.set(Some(close_picker)));
        cx.notify();
    }

    fn close_stream_upsell(&mut self, cx: &mut Context<Self>) {
        if UPSELL.with(|upsell| upsell.take()) == Some(true) {
            self.close_go_live(cx);
        }
        cx.notify();
    }

    /// Lists the sources on a worker the moment the picker opens, then takes each thumbnail on a worker and shows it
    /// as soon as it is there; again a second after each round, until the picker closes.
    fn go_live_refresh(&mut self, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut round = 0u32;
            loop {
                let opened = match this.update(cx, |root, _| {
                    root.go_live
                        .as_ref()
                        .filter(|state| state.generation == generation)
                        .map(|state| state.opened_at)
                }) {
                    Ok(Some(opened)) => opened,
                    _ => return,
                };
                let known: HashSet<SourceId> =
                    LISTING.with(|listing| listing.borrow().icons.keys().copied().collect());
                let started = Instant::now();
                let (sources, icons, devices) = cx
                    .background_executor()
                    .spawn(async move {
                        let sources = list_sources();
                        let icons: Vec<(SourceId, Arc<RenderImage>)> = sources
                            .iter()
                            .filter(|source| !known.contains(&source.id))
                            .filter_map(|source| {
                                Some((source.id, source.icon.as_ref().and_then(render_image)?))
                            })
                            .collect();
                        (sources, icons, list_devices())
                    })
                    .await;
                debug!(
                    "go live: round {round}: {} sources listed in {} ms, {} ms after the picker opened",
                    sources.len(),
                    started.elapsed().as_millis(),
                    opened.elapsed().as_millis()
                );
                let Ok(Some(tab)) = this.update(cx, |root, cx| {
                    root.go_live_listed(generation, sources, icons, devices, cx)
                }) else {
                    return;
                };
                for id in listing_order(tab) {
                    let taken = Instant::now();
                    let picture = cx
                        .background_executor()
                        .spawn(async move {
                            let picture = list_thumbnail(&id)?;
                            let blank = is_blank(&picture);
                            Some((blank, if blank { None } else { render_image(&picture) }))
                        })
                        .await;
                    let blank = picture.as_ref().is_some_and(|(blank, _)| *blank);
                    debug!(
                        "go live: {} thumbnail in {} ms ({}), {} ms after the picker opened",
                        match id {
                            SourceId::Display(_) => "screen",
                            SourceId::Window(_) => "window",
                        },
                        taken.elapsed().as_millis(),
                        if picture.is_none() {
                            "none"
                        } else if blank {
                            "blank, kept the last"
                        } else {
                            "shown"
                        },
                        opened.elapsed().as_millis()
                    );
                    let image = picture.and_then(|(_, image)| image);
                    let alive = this
                        .update(cx, |root, cx| root.go_live_thumb(generation, id, image, cx))
                        .unwrap_or(false);
                    if !alive {
                        return;
                    }
                }
                round += 1;
                cx.background_executor().timer(REFRESH).await;
            }
        })
        .detach();
    }

    /// A list arrived: it replaces the last one (pictures of sources that went away are released). Returns the shown
    /// tab while the picker of `generation` is open.
    fn go_live_listed(
        &mut self,
        generation: u64,
        sources: Vec<Source>,
        icons: Vec<(SourceId, Arc<RenderImage>)>,
        devices: Vec<dc_media::camera::CameraDevice>,
        cx: &mut Context<Self>,
    ) -> Option<Tab> {
        let mut dropped = Vec::new();
        LISTING.with(|listing| {
            let mut listing = listing.borrow_mut();
            let present: HashSet<SourceId> = sources.iter().map(|source| source.id).collect();
            listing.thumbs.retain(|id, image| {
                let keep = present.contains(id);
                if !keep {
                    dropped.push(image.clone());
                }
                keep
            });
            listing.icons.retain(|id, image| {
                let keep = present.contains(id);
                if !keep {
                    dropped.push(image.clone());
                }
                keep
            });
            for (id, image) in icons {
                if let Some(old) = listing.icons.insert(id, image) {
                    dropped.push(old);
                }
            }
            listing.sources = sources;
            listing.devices = devices;
            listing.fetched = true;
        });
        for image in dropped {
            cx.drop_image(image, None);
        }
        let state = self
            .go_live
            .as_mut()
            .filter(|state| state.generation == generation)?;
        state.fetching = false;
        cx.notify();
        Some(state.tab)
    }

    /// A thumbnail arrived. Returns whether the picker of `generation` is still open.
    fn go_live_thumb(
        &mut self,
        generation: u64,
        id: SourceId,
        image: Option<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) -> bool {
        let alive = self
            .go_live
            .as_ref()
            .is_some_and(|state| state.generation == generation);
        if let Some(image) = image {
            let listed = LISTING.with(|listing| {
                let listing = listing.borrow();
                listing.sources.iter().any(|source| source.id == id)
            });
            if listed {
                if let Some(old) =
                    LISTING.with(|listing| listing.borrow_mut().thumbs.insert(id, image))
                {
                    cx.drop_image(old, None);
                }
            } else {
                cx.drop_image(image, None);
            }
        }
        if alive {
            cx.notify();
        }
        alive
    }

    /// A source was picked: Go Live with it at once (`el` of `tJ`), and the store keeps what it started with.
    fn go_live_pick(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(state) = &self.go_live else {
            return;
        };
        let Some((id, name)) = state.listed().get(index).cloned() else {
            return;
        };
        let Some(source) = LISTING.with(|listing| {
            listing
                .borrow()
                .sources
                .iter()
                .find(|source| source.id == id)
                .cloned()
        }) else {
            return;
        };
        let account = self.stream_account();
        let prefs = state.prefs;
        let (preset, resolution, fps) = prefs.start_choice(account);
        let Some((settings, resolution_used, fps_used)) =
            settings_for(&source, resolution, fps, prefs.sound)
        else {
            self.put_app_toast("This source can't be shared.".to_owned(), false, cx);
            return;
        };
        Prefs {
            preset,
            resolution,
            fps,
            ..prefs
        }
        .save();
        let icon = LISTING.with(|listing| listing.borrow().icons.get(&id).cloned());
        self.close_go_live(cx);
        // Change Stream: the old stream ends first (the official switches the source of the running one).
        let focus = self.vv.focus.tile;
        self.stream_restart(
            dc_voice::StreamSource::Screen(settings),
            super::voice_video::Started {
                name,
                info: source,
                quality: (resolution_used, fps_used),
                icon,
            },
            focus,
            cx,
        );
    }

    /// Stream Quality or Share Stream Audio changed while streaming: the store takes the new choices; the capture's
    /// size, rate and audio are fixed when it starts, so the stream restarts from the same source, keeping the call
    /// view's focus.
    pub(super) fn restart_stream_with(&mut self, prefs: Prefs, cx: &mut Context<Self>) {
        prefs.save();
        let Some(started) = self.own_started() else {
            cx.notify();
            return;
        };
        let account = self.stream_account();
        let (_, resolution, fps) = prefs.start_choice(account);
        let Some((settings, resolution, fps)) =
            settings_for(&started.info, resolution, fps, prefs.sound)
        else {
            return;
        };
        let focus = self.vv.focus.tile;
        self.stream_restart(
            dc_voice::StreamSource::Screen(settings),
            super::voice_video::Started {
                quality: (resolution, fps),
                ..started
            },
            focus,
            cx,
        );
    }

    /// The modal over a dimmed window, and the streaming upsell over it when it is open.
    pub(super) fn go_live_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let upsell = UPSELL
            .with(|upsell| upsell.get())
            .map(|_| self.stream_upsell(cx));
        let modal = self.go_live_modal(window, cx);
        match (modal, upsell) {
            (None, None) => None,
            (Some(one), None) | (None, Some(one)) => Some(one),
            (Some(modal), Some(upsell)) => Some(
                div()
                    .absolute()
                    .inset_0()
                    .child(modal)
                    .child(upsell)
                    .into_any_element(),
            ),
        }
    }

    fn go_live_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.go_live.as_ref()?;
        let account = self.stream_account();
        let upsell = account.needs_hd_upsell();
        let (resolution, fps) = state.prefs.shown(account);
        let is_game = state.game.is_some();
        let choose_channel = state.game.as_ref().is_some_and(|game| game.choose_channel);

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
                        state.selected_device = None;
                        // `set_source_type` to the camera: Screenshare becomes Gaming, Source becomes 720p.
                        if tab == Tab::Devices {
                            if state.prefs.preset == Preset::Documents {
                                state.prefs.preset = Preset::Video;
                            }
                            if state.prefs.resolution == Resolution::Source {
                                state.prefs.resolution = Resolution::P720;
                            }
                        }
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
                dc_intl::tr("Applications"),
                "golive_tab_apps",
                cx,
            ))
            .child(tab(
                Tab::Screen,
                dc_intl::tr("Entire Screen"),
                "golive_tab_screen",
                cx,
            ))
            .child(tab(
                Tab::Devices,
                dc_intl::tr("Devices"),
                "golive_tab_devices",
                cx,
            ));

        let devices: Vec<(String, String)> = LISTING.with(|listing| {
            listing
                .borrow()
                .devices
                .iter()
                .map(|device| (device.id.clone(), device.name.clone()))
                .collect()
        });
        let shown = content(state.fetching, state.tab, devices.len());
        let cards: Vec<AnyElement> = if shown != Content::Cards {
            Vec::new()
        } else if state.tab == Tab::Devices {
            devices
                .iter()
                .enumerate()
                .map(|(index, (id, name))| {
                    let chosen = state.selected_device.as_deref() == Some(id.as_str());
                    let id = id.clone();
                    self.go_live_card(
                        Card {
                            id_prefix: "device",
                            index,
                            thumb: None,
                            name: name.clone(),
                            icon: None,
                            fallback_icon: "golive_tab_devices",
                            cta: dc_intl::tr("Select"),
                            hovered: state.hover == Some(index),
                            selected: chosen,
                        },
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
            let (thumbs, icons) = LISTING.with(|listing| {
                let listing = listing.borrow();
                (listing.thumbs.clone(), listing.icons.clone())
            });
            let (id_prefix, fallback_icon) = match state.tab {
                Tab::Applications => ("apps", "golive_tab_apps"),
                Tab::Screen => ("screen", "golive_tab_screen"),
                Tab::Devices => ("device", "golive_tab_devices"),
            };
            state
                .listed()
                .into_iter()
                .enumerate()
                .map(|(index, (id, name))| {
                    self.go_live_card(
                        Card {
                            id_prefix,
                            index,
                            thumb: thumbs.get(&id).cloned(),
                            name,
                            icon: icons.get(&id).cloned(),
                            fallback_icon,
                            cta: dc_intl::tr("Share Screen"),
                            hovered: state.hover == Some(index),
                            selected: false,
                        },
                        cx,
                        move |this: &mut Root, cx: &mut Context<Root>| this.go_live_pick(index, cx),
                    )
                })
                .collect()
        };

        let selected_device = state
            .selected_device
            .as_ref()
            .filter(|_| state.tab == Tab::Devices)
            .and_then(|id| devices.iter().find(|(known, _)| known == id))
            .map(|(_, name)| name.clone())
            .or_else(|| state.game.as_ref().map(|game| game.name.clone()));
        let bullet = || div().text_color(rgba(0x97979f1f)).child("\u{2022}");
        let mut detail_row = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-muted"));
        // `tq`: with a source chosen, the settings glyph and the preset's name lead; else the preset's word. Then
        // the resolution and the frame rate, each after a bullet, and "Stream Muted" when muted.
        if selected_device.is_some() {
            detail_row = detail_row
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(icon("voice_view_gear", 12., "--text-muted"))
                        .child(state.prefs.preset.title()),
                )
                .child(bullet());
        } else if let Some(summary) = state.prefs.preset.summary() {
            detail_row = detail_row.child(summary);
        }
        detail_row = detail_row
            .child(bullet())
            .child(resolution_text(resolution))
            .child(bullet())
            .child(format!("{fps}fps"));
        if !state.prefs.sound {
            detail_row = detail_row.child(bullet()).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("vm_voice_x", 12., "--text-muted"))
                    .child(dc_intl::tr("Stream Muted")),
            );
        }

        let pill_item = |label: &'static str, selected: bool, premium: bool| {
            div()
                // Fixed SD/HD labels, measured in the pinned confirmation DOM.
                .w(px(if label == "SD" { 49.234375 } else { 50.84375 }))
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
        // `tI`: shown while HD is out of reach. It always reads SD; HD opens the upsell, and closing that closes the
        // picker.
        let sd_hd = upsell.then(|| {
            div()
                .h(px(40.))
                .p(px(4.))
                .gap(px(4.))
                .rounded(px(12.))
                .bg(color("--background-base-lowest"))
                .flex()
                .child(div().id("golive-sd").child(pill_item("SD", true, false)))
                .child(
                    div()
                        .id("golive-hd")
                        .cursor_pointer()
                        .child(pill_item("HD", false, true))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_stream_upsell(true, cx);
                        })),
                )
        });
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
            .child(self.anchor_probe("golive-gear"))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(state) = this.go_live.as_mut() {
                    state.options = !state.options;
                    state.sub = None;
                }
                cx.notify();
            }))
            .child(icon("voice_view_gear", 20., "--text-strong"));
        // The Devices tab's "Stream", off until a device is chosen.
        let stream_button =
            (state.tab == Tab::Devices || (is_game && !choose_channel)).then(|| {
                let ready = if let Some(game) = &state.game {
                    game.source.is_some()
                        && game.pending_channel.is_none()
                        && self.game_stream_allowed()
                } else {
                    selected_device.is_some()
                };
                div()
                    .id("golive-stream")
                    .min_w(px(100.))
                    .h(px(40.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(color("--control-primary-background-default"))
                    .text_color(color("--control-primary-text-default"))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .when(!ready, |this| this.opacity(0.5))
                    .when(ready, |this| {
                        this.cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this
                                    .go_live
                                    .as_ref()
                                    .is_some_and(|state| state.game.is_some())
                                {
                                    this.game_share_start(cx);
                                } else {
                                    this.put_app_toast(
                                        "This source can't be shared.".to_owned(),
                                        false,
                                        cx,
                                    );
                                }
                            }))
                    })
                    .child(dc_intl::tr("Stream"))
            });

        let game_icon = state.game.as_ref().map(|game| {
            let picture = self.overlay_game_icon(game.application_id.as_deref());
            div()
                .size(px(40.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .when_some(picture.clone(), |element, picture| {
                    element.child(gpui::img(picture).size_full().rounded(px(8.)))
                })
                .when(picture.is_none(), |element| {
                    element.child(icon("inventory_unknown_game", 40., "--text-strong"))
                })
        });
        let footer = div()
            .parity("golive-footer")
            .w_full()
            .flex_none()
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(
                div()
                    .parity("golive-footer-content")
                    .px(px(24.))
                    .pt(px(16.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(32.))
                    .child(
                        div()
                            .parity("golive-summary")
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .min_w(px(0.))
                            .children(game_icon)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .min_w(px(0.))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .truncate()
                                            .child(match &selected_device {
                                                Some(name) => SharedString::from(name.clone()),
                                                None => {
                                                    SharedString::from(state.prefs.preset.title())
                                                }
                                            }),
                                    )
                                    .child(detail_row),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .parity("golive-buttons")
                            .children(sd_hd)
                            .children(stream_button)
                            .child(gear),
                    ),
            )
            .when(upsell, |this| this.child(self.go_live_ribbon(cx)));

        let menu = state
            .options
            .then(|| self.go_live_options(state, account, upsell, cx));

        let body: AnyElement = if is_game {
            self.game_share_body(window, cx)?
        } else {
            match shown {
                Content::Loading => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(super::login::wandering_cubes())
                    .into_any_element(),
                Content::NoDevices => self.go_live_no_devices(cx),
                Content::Cards => div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .content_start()
                    .gap_x(px(16.))
                    .gap_y(px(24.))
                    .children(cards)
                    .into_any_element(),
            }
        };

        let modal_width = if is_game {
            960f32.min((f32::from(window.viewport_size().width) - 48.).max(0.))
        } else {
            960.
        };
        let modal_height = if is_game {
            let desired = if choose_channel {
                530.
            } else {
                (modal_width - 2.) * 9. / 16. + if upsell { 112. } else { 56. } + 26.
            };
            desired.min((f32::from(window.viewport_size().height) - 112.).max(0.))
        } else {
            608.
        };

        let modal = div()
            .id("golive-modal")
            .parity_with_border("golive-modal", 1.)
            .occlude()
            .relative()
            .w(px(modal_width))
            .h(px(modal_height))
            .pt(px(8.))
            .pb(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .when(!is_game, |this| {
                this.child(
                    div()
                        .h(px(72.))
                        .flex_none()
                        .px(px(24.))
                        .py(px(16.))
                        .child(tabs),
                )
            })
            .child(
                div()
                    .id("golive-content")
                    .flex_1()
                    .min_h(px(0.))
                    .when(!is_game, |this| {
                        this.overflow_y_scroll().px(px(24.)).py(px(8.))
                    })
                    .when(choose_channel, |this| this.px(px(16.)).pt(px(16.)))
                    .child(body),
            )
            .child(footer)
            .children(menu);
        let pose = crate::ui::motion::modal_pose("golive");
        Some(
            deferred(
                div()
                    .id("golive-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| this.close_go_live(cx)))
                    .child(modal.posed(pose)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `e9`: no capture device. The camera glyph, the two lines and Learn More.
    fn go_live_no_devices(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .mb(px(16.))
                    .child(icon("golive_tab_devices", 24., "--icon-muted")),
            )
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr(
                        "You don\u{2019}t have any Capture Devices available.",
                    )),
            )
            .child(
                div()
                    .mb(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-subtle"))
                    .child(dc_intl::tr(
                        "In the future, you can use this tab to share from specific devices like your Camera.",
                    )),
            )
            .child(
                div()
                    .id("golive-devices-learn")
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-link"))
                    .hover(|style| style.underline())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.follow_link(DEVICES_HELP, "Learn More", cx);
                    }))
                    .child(dc_intl::tr("Learn More")),
            )
            .into_any_element()
    }

    /// One source (`e4`): a 16:9 preview on black with a 2px ring when hovered (brand when chosen), its icon and name
    /// below. Hover dims it with the call to action.
    fn go_live_card(
        &self,
        card: Card,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        let Card {
            id_prefix,
            index,
            thumb,
            name,
            icon: icon_image,
            fallback_icon,
            cta,
            hovered,
            selected,
        } = card;
        let hover_id = index;
        div()
            .id(SharedString::from(format!(
                "golive-card-{id_prefix}-{index}"
            )))
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
                    .rounded(px(12.))
                    .when(hovered || selected, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top(px(-4.))
                                .left(px(-4.))
                                .right(px(-4.))
                                .bottom(px(-4.))
                                .rounded(px(12.))
                                .border_2()
                                .border_color(if selected {
                                    color("--brand-500")
                                } else {
                                    color("--background-mod-strong")
                                }),
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
                                        .bg(color("--opacity-black-40"))
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
                    .child(match icon_image {
                        Some(image) => gpui::img(image)
                            .size(px(16.))
                            .rounded(px(2.))
                            .flex_none()
                            .into_any_element(),
                        None => icon(fallback_icon, 16., "--interactive-icon-default")
                            .into_any_element(),
                    })
                    .child(div().truncate().child(name)),
            )
            .into_any_element()
    }

    /// "Stream in HD resolution with Nitro" (`tD`): a gradient bar with Get Nitro. The Nitro link opens the upsell
    /// (closing it closes the picker); Get Nitro closes the picker and starts the purchase.
    fn go_live_ribbon(&self, cx: &mut Context<Self>) -> AnyElement {
        // `sLJ3EV`: the sentence, and its `$link` run.
        let runs = dc_intl::format_runs("sLJ3EV", &[]);
        let lead: String = runs
            .iter()
            .filter(|(_, tag)| tag.is_none())
            .map(|(text, _)| text.as_str())
            .collect();
        let link: String = runs
            .iter()
            .filter(|(_, tag)| tag.is_some())
            .map(|(text, _)| text.as_str())
            .collect();
        div()
            .parity("golive-ribbon")
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
                    .child(lead.trim_end().to_owned())
                    .child(
                        div()
                            .id("golive-nitro-link")
                            .font_weight(FontWeight(700.))
                            .cursor_pointer()
                            .underline()
                            .child(link)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_stream_upsell(true, cx);
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
                                this.close_go_live(cx);
                                this.open_purchase(false, cx);
                            }))
                            .child(gpui::img(nitro_ribbon_icon()).size(px(16.)))
                            .child(dc_intl::tr("Get Nitro")),
                    ),
            )
            .into_any_element()
    }

    /// The gear's menu (`tC`, `stream-options`): Stream Mode; with Custom, Screen Resolution and Frame Rate; Mute
    /// stream audio; Advanced. A choice the account may not use closes the menu and opens the upsell.
    fn go_live_options(
        &self,
        state: &State,
        account: Account,
        ribbon: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tint: Tint = app_tint;
        let mut rows: Vec<(AnyElement, f32)> = Vec::new();
        rows.push((group_label(dc_intl::tr("Stream Mode"), tint), 36.));
        let presets: &[Preset] = if state.tab == Tab::Devices {
            &[Preset::Video, Preset::Custom]
        } else {
            &[Preset::Video, Preset::Documents, Preset::Custom]
        };
        for &preset in presets {
            let subtext = preset_subtext(preset, account);
            let height = if subtext.is_some() { 52. } else { 36. };
            rows.push((
                radio_row(
                    SharedString::from(format!("stream-preset-{}", preset as u8)),
                    div().child(preset.title()).into_any_element(),
                    subtext,
                    state.prefs.preset == preset,
                    tint,
                    cx,
                    move |this, cx| {
                        // A radio keeps the menu open (`MenuRadioItem` only reports the interaction).
                        if let Some(state) = this.go_live.as_mut() {
                            state.prefs.preset = preset;
                        }
                        cx.notify();
                    },
                )
                .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                    if *inside && let Some(state) = this.go_live.as_mut() {
                        state.sub = None;
                        cx.notify();
                    }
                }))
                .into_any_element(),
                height,
            ));
        }
        let custom = state.prefs.preset == Preset::Custom;
        let sub_row = |id: &'static str, label: &'static str, sub: Sub, cx: &mut Context<Root>| {
            caret_row(
                id,
                label,
                state.sub == Some(sub),
                tint,
                cx,
                move |this, open, cx| {
                    if let Some(state) = this.go_live.as_mut() {
                        state.sub = if open { Some(sub) } else { None };
                    }
                    cx.notify();
                },
            )
            .into_any_element()
        };
        // The offset of each submenu's row from the menu's top, for the submenu's own top.
        let mut row_tops: HashMap<Sub, f32> = HashMap::new();
        let mut top = 9.;
        if custom {
            rows.push((separator(tint), 17.));
            rows.push((
                sub_row(
                    "stream-option-resolution",
                    dc_intl::tr("Screen Resolution"),
                    Sub::Resolution,
                    cx,
                ),
                36.,
            ));
            rows.push((
                sub_row(
                    "stream-option-frame-rate",
                    dc_intl::tr("Frame Rate"),
                    Sub::FrameRate,
                    cx,
                ),
                36.,
            ));
        }
        rows.push((separator(tint), 17.));
        rows.push((
            check_row(
                "stream-option-mute",
                dc_intl::tr("Mute stream audio"),
                !state.prefs.sound,
                tint,
                cx,
                |this, cx| {
                    if let Some(state) = this.go_live.as_mut() {
                        state.prefs.sound = !state.prefs.sound;
                    }
                    cx.notify();
                },
            )
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                if *inside && let Some(state) = this.go_live.as_mut() {
                    state.sub = None;
                    cx.notify();
                }
            }))
            .into_any_element(),
            36.,
        ));
        rows.push((
            sub_row("advanced-items", dc_intl::tr("Advanced"), Sub::Advanced, cx),
            36.,
        ));
        // Record where the submenu rows sit (the menu's 8px padding and 1px border above the first row).
        for (index, (_, height)) in rows.iter().enumerate() {
            let sub = match (custom, index) {
                (true, i) if i == presets.len() + 2 => Some(Sub::Resolution),
                (true, i) if i == presets.len() + 3 => Some(Sub::FrameRate),
                (_, i) if i == rows.len() - 1 => Some(Sub::Advanced),
                _ => None,
            };
            if let Some(sub) = sub {
                row_tops.insert(sub, top);
            }
            top += height;
        }
        let menu_height = 2. + 16. + rows.iter().map(|(_, height)| height).sum::<f32>();

        let submenu = state.sub.map(|sub| {
            let mut items: Vec<AnyElement> = Vec::new();
            match sub {
                Sub::Resolution => {
                    for resolution in [
                        Resolution::P720,
                        Resolution::P1080,
                        Resolution::P1440,
                        Resolution::Source,
                    ] {
                        if state.tab == Tab::Devices && resolution == Resolution::Source {
                            continue;
                        }
                        let label = resolution_text(resolution);
                        let fps = state.prefs.fps;
                        items.push(
                            radio_row(
                                SharedString::from(format!(
                                    "stream-option-resolution-{}",
                                    resolution.height()
                                )),
                                if option_shows_premium(
                                    account,
                                    state.prefs.preset,
                                    resolution,
                                    fps,
                                ) {
                                    premium_label(label)
                                } else {
                                    div().child(label).into_any_element()
                                },
                                None,
                                state.prefs.resolution == resolution,
                                tint,
                                cx,
                                move |this, cx| {
                                    let account = this.stream_account();
                                    let Some(state) = this.go_live.as_mut() else {
                                        return;
                                    };
                                    if can_stream(account, state.prefs.preset, resolution, fps) {
                                        state.prefs.resolution = resolution;
                                    } else {
                                        state.options = false;
                                        state.sub = None;
                                        this.open_stream_upsell(false, cx);
                                    }
                                    cx.notify();
                                },
                            )
                            .into_any_element(),
                        );
                    }
                }
                Sub::FrameRate => {
                    for fps in [15u32, 30, 60] {
                        let label = format!("{fps}fps");
                        let resolution = state.prefs.resolution;
                        items.push(
                            radio_row(
                                SharedString::from(format!("stream-option-frame-rate-{fps}")),
                                if option_shows_premium(
                                    account,
                                    state.prefs.preset,
                                    resolution,
                                    fps,
                                ) {
                                    premium_label(label)
                                } else {
                                    div().child(label).into_any_element()
                                },
                                None,
                                state.prefs.fps == fps,
                                tint,
                                cx,
                                move |this, cx| {
                                    let account = this.stream_account();
                                    let Some(state) = this.go_live.as_mut() else {
                                        return;
                                    };
                                    if can_stream(account, state.prefs.preset, resolution, fps) {
                                        state.prefs.fps = fps;
                                    } else {
                                        state.options = false;
                                        state.sub = None;
                                        this.open_stream_upsell(false, cx);
                                    }
                                    cx.notify();
                                },
                            )
                            .into_any_element(),
                        );
                    }
                }
                Sub::Advanced => {
                    items.push(
                        check_row(
                            "stream-option-share-preview",
                            dc_intl::tr("Hide stream preview"),
                            state.prefs.hide_preview,
                            tint,
                            cx,
                            |this, cx| {
                                if let Some(state) = this.go_live.as_mut() {
                                    state.prefs.hide_preview = !state.prefs.hide_preview;
                                }
                                cx.notify();
                            },
                        )
                        .into_any_element(),
                    );
                }
            }
            let offset = row_tops.get(&sub).copied().unwrap_or(9.) - 9.;
            menu_frame("stream-options-sub", tint)
                .min_w(px(188.))
                .mt(px(offset))
                .children(items)
        });
        div()
            .id("stream-options-layer")
            .absolute()
            .right(px(25.44))
            // 8px over the gear, whose top is 113px over the modal's bottom with the ribbon and 57 without.
            .bottom(px(if ribbon { 121. } else { 65. }))
            .h(px(menu_height))
            .flex()
            .items_start()
            .gap(px(4.))
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                // The gear's own press toggles the menu.
                let on_gear = this
                    .tip_anchors
                    .borrow()
                    .get("golive-gear")
                    .is_some_and(|bounds| bounds.contains(&event.position));
                if !on_gear && let Some(state) = this.go_live.as_mut() {
                    state.options = false;
                    state.sub = None;
                    cx.notify();
                }
            }))
            .children(submenu)
            .child(
                menu_frame("stream-options", tint)
                    .min_w(px(188.))
                    .children(rows.into_iter().map(|(row, _)| row)),
            )
            .into_any_element()
    }

    /// The streaming upsell (module 826789 in the premium upsell dialog, module 831318, `premium_upsell.rs`):
    /// "Unlock HD streaming in 4k at 60 fps", View All Nitro Perks and Get Nitro.
    fn stream_upsell(&self, cx: &mut Context<Self>) -> AnyElement {
        self.premium_upsell(
            super::premium_upsell::Spec {
                id: "stream-upsell",
                ids: [
                    "stream-upsell-close",
                    "stream-upsell-perks",
                    "stream-upsell-get",
                ],
                header: super::premium_upsell::Header::Video,
                title: dc_intl::tr("Unlock HD streaming in 4k at 60 fps").into(),
                subtitle: dc_intl::tr(
                    "Stream your clutchest moments in 4k resolution with Nitro. Or stream funny cat videos, we don't judge.",
                )
                .into(),
                secondary: dc_intl::tr("View All Nitro Perks").into(),
                on_secondary: Box::new(|this, _, cx| {
                    UPSELL.with(|upsell| upsell.set(None));
                    this.close_go_live(cx);
                    this.open_nitro(cx);
                }),
                on_primary: Box::new(|this, _, cx| {
                    UPSELL.with(|upsell| upsell.set(None));
                    this.close_go_live(cx);
                    this.open_purchase(false, cx);
                }),
                close: Box::new(|this, _, cx| this.close_stream_upsell(cx)),
                priority: 3,
            },
            cx,
        )
    }

    /// Capture only: the picker's state for scripts (`golive state`), and `golive open`, `golive close`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_go_live(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        match words {
            ["connected"] => {
                if !crate::inert::on() {
                    return "simulated connection requires inert capture".into();
                }
                let pending = self.go_live.as_ref().and_then(|state| state.game.as_ref())
                    .and_then(|game| game.pending_channel);
                let Some((guild, channel)) = pending.filter(|&(guild, channel)|
                    (guild, channel) == (1_568_692_564_637_524_109, 1_568_692_565_136_375_901)
                    || (guild, channel) == (0, 1_568_693_333_864_480_939)) else {
                    return "no pending game share in an allowed fixture channel".into();
                };
                if !self.call.as_ref().is_some_and(|call| call.guild == guild && call.channel == channel) {
                    return "pending game share did not join its target".into();
                }
                let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) else {
                    return "no test user".into();
                };
                self.stores.dispatch("VOICE_STATE_UPDATE", &json!({
                    "guild_id": (guild != 0).then(|| guild.to_string()), "channel_id": channel.to_string(),
                    "user_id": me.to_string(), "session_id": self.stores.session_id.clone().unwrap_or_else(|| "harness".into()),
                    "self_mute": true, "self_deaf": false, "self_video": false,
                }));
                self.call.as_mut().unwrap().connected = true;
                self.game_share_pending(cx);
                "simulated fixture connection; network remains inert".into()
            }
            ["open"] => {
                self.open_go_live(cx);
                if self.go_live.is_some() {
                    "opened".to_owned()
                } else {
                    "not in a call".to_owned()
                }
            }
            ["close"] => {
                self.close_go_live(cx);
                "closed".to_owned()
            }
            ["state"] => {
                let account = self.stream_account();
                let stored = Prefs::load();
                let modal = match &self.go_live {
                    Some(state) => format!(
                        "open tab={:?} fetching={} listed={} options={} sub={:?} prefs={:?}/{:?}/{}/sound={} shown={:?} sd_hd={}",
                        state.tab,
                        state.fetching,
                        state.listed().len(),
                        state.options,
                        state.sub,
                        state.prefs.preset,
                        state.prefs.resolution,
                        state.prefs.fps,
                        state.prefs.sound,
                        state.prefs.shown(account),
                        account.needs_hd_upsell(),
                    ),
                    None => "closed".to_owned(),
                };
                let thumbs = LISTING.with(|listing| listing.borrow().thumbs.len());
                let game = self
                    .go_live
                    .as_ref()
                    .and_then(|state| state.game.as_ref())
                    .map(|game| {
                        format!(
                            "choose_channel={} source={:?} preview={} pending={:?}",
                            game.choose_channel,
                            game.source.as_ref().map(|source| source.id),
                            game.preview.is_some(),
                            game.pending_channel
                        )
                    });
                format!(
                    "{modal}; game={game:?}; thumbs={thumbs}; upsell={:?}; stored={:?}/{:?}/{}/sound={}; focus={:?}; stream={:?}",
                    UPSELL.with(|upsell| upsell.get()),
                    stored.preset,
                    stored.resolution,
                    stored.fps,
                    stored.sound,
                    self.vv.focus.tile,
                    self.own_started().map(|started| started.quality),
                )
            }
            _ => "usage: golive open | golive close | golive state | golive connected (inert fixture only)".to_owned(),
        }
    }
}

// ---- the menu component (`menu_M3qk4j`) ----

/// The palette a menu draws with: the app's theme in the picker, `midnight` over the call view.
pub(super) type Tint = fn(&str) -> gpui::Rgba;

/// The app's theme.
pub(super) fn app_tint(name: &str) -> gpui::Rgba {
    color(name)
}

/// `menu_M3qk4j`: 8px of padding, the 1px subtle border, 8px corners, the higher surface and its shadow. A press in
/// it stays in it.
pub(super) fn menu_frame(id: impl Into<gpui::ElementId>, tint: Tint) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .occlude()
        .p(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(tint("--border-subtle"))
        .bg(tint("--background-surface-higher"))
        .shadow(vec![gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.24),
            offset: gpui::point(px(0.), px(12.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        }])
        .flex()
        .flex_col()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// `groupLabel_M3qk4j`: 14px medium, muted, 8px in, 36 high.
pub(super) fn group_label(text: &str, tint: Tint) -> AnyElement {
    div()
        .h(px(36.))
        .flex_none()
        .px(px(8.))
        .pt(px(8.))
        .text_size(px(14.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(tint("--text-muted"))
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

/// `separator_M3qk4j`: a 1px line with 8px above and below.
pub(super) fn separator(tint: Tint) -> AnyElement {
    div()
        .my(px(8.))
        .h(px(1.))
        .flex_none()
        .bg(tint("--border-subtle"))
        .into_any_element()
}

/// `item_M3qk4j`: 8px of padding, 2px corners; hovered it is the focused item (`focused_M3qk4j`, 4px corners).
fn item(id: impl Into<gpui::ElementId>, height: f32, tint: Tint) -> gpui::Stateful<gpui::Div> {
    item_with_hover(id, height, tint("--interactive-background-hover"))
}

/// An item with its own focused colour (a danger item's is `--background-feedback-mod-critical`).
fn item_with_hover(
    id: impl Into<gpui::ElementId>,
    height: f32,
    hover: gpui::Rgba,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(height))
        .flex_none()
        .p(px(8.))
        .rounded(px(2.))
        .cursor_pointer()
        .flex()
        .items_center()
        .hover(move |style| style.bg(hover).rounded(px(4.)))
}

/// The label column of an item: `body-sm/medium` in the strong text colour, and the subtext (`body-xs/normal`).
fn item_label(label: AnyElement, subtext: Option<String>, tint: Tint) -> gpui::Div {
    // `label_M3qk4j`: `flex: 1 1 auto`, so a menu sized by its content is as wide as its longest label.
    div()
        .flex_grow(1.)
        .flex_shrink(1.)
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(tint("--text-strong"))
                .child(label),
        )
        .children(subtext.map(|text| {
            div()
                .mt(px(2.))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(400.))
                .text_color(tint("--text-muted"))
                .child(text)
        }))
}

/// A label with the Nitro wheel after it (`tA`, `premiumOptionContainer_B6ppCQ`): 4px apart, 32px kept free after.
pub(super) fn premium_label(text: String) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .mr(px(32.))
        .child(text)
        .child(
            svg()
                .path("icons/nitro_wheel.svg")
                .size(px(16.))
                .flex_none()
                .text_color(rgba(BOOSTING_PINK)),
        )
        .into_any_element()
}

/// `MenuRadioItem`: the label (and its subtext) and the 20px radio of the radio group (`radioIndicator_oJuRbG`). A
/// click does not close the menu.
pub(super) fn radio_row(
    id: impl Into<gpui::ElementId>,
    label: AnyElement,
    subtext: Option<String>,
    checked: bool,
    tint: Tint,
    cx: &mut Context<Root>,
    run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let height = if subtext.is_some() { 52. } else { 36. };
    item(id, height, tint)
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            run(this, cx);
        }))
        .child(item_label(label, subtext, tint))
        .child(
            div()
                .ml(px(8.))
                .size(px(20.))
                .flex_none()
                .child(super::settings::radio_dot(checked)),
        )
}

/// `MenuCheckboxItem`: the label and the 20px checkbox indicator (`checkboxIndicator_vTUVyc`, its drawn check).
pub(super) fn check_row(
    id: impl Into<gpui::ElementId>,
    label: &str,
    checked: bool,
    tint: Tint,
    cx: &mut Context<Root>,
    run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    item(id, 36., tint)
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            run(this, cx);
        }))
        .child(item_label(
            div()
                .whitespace_nowrap()
                .child(SharedString::from(label.to_owned()))
                .into_any_element(),
            None,
            tint,
        ))
        .child(
            div()
                .ml(px(8.))
                .flex_none()
                .child(super::server_onboarding::checkbox(checked, false)),
        )
}

/// A plain `MenuItem` with its leading icon (`iconContainerLeft_M3qk4j`, 20px, 8px before the label); `danger` is
/// `colorDanger_M3qk4j`.
pub(super) fn icon_row(
    id: impl Into<gpui::ElementId>,
    glyph: &'static str,
    label: &str,
    danger: bool,
    tint: Tint,
    cx: &mut Context<Root>,
    run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let (glyph_tint, hover, text) = if danger {
        (
            tint("--text-feedback-critical"),
            tint("--background-feedback-mod-critical"),
            tint("--text-feedback-critical"),
        )
    } else {
        (
            tint("--interactive-text-default"),
            tint("--interactive-background-hover"),
            tint("--text-strong"),
        )
    };
    item_with_hover(id, 36., hover)
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            run(this, cx);
        }))
        .child(
            div().mr(px(8.)).child(
                svg()
                    .path(format!("icons/{glyph}.svg"))
                    .size(px(20.))
                    .flex_none()
                    .text_color(glyph_tint),
            ),
        )
        .child(
            div()
                .flex_1()
                .whitespace_nowrap()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(text)
                .child(SharedString::from(label.to_owned())),
        )
}

/// A `MenuItem` with children: the label and the caret (`caret_M3qk4j`, 20px, muted). Hovering or pressing it opens
/// its submenu (a press does not close it); `run` gets whether it should be open. It reads as focused while its submenu is open.
pub(super) fn caret_row(
    id: impl Into<gpui::ElementId>,
    label: &str,
    open: bool,
    tint: Tint,
    cx: &mut Context<Root>,
    run: impl Fn(&mut Root, bool, &mut Context<Root>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let run = std::rc::Rc::new(run);
    let hovered = run.clone();
    item(id, 36., tint)
        .when(open, |this| {
            this.bg(tint("--interactive-background-hover"))
                .rounded(px(4.))
        })
        .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
            if *inside {
                (*hovered)(this, true, cx);
            }
        }))
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            (*run)(this, true, cx);
        }))
        .child(item_label(
            div()
                .whitespace_nowrap()
                .child(SharedString::from(label.to_owned()))
                .into_any_element(),
            None,
            tint,
        ))
        .child(
            div().ml(px(8.)).child(
                svg()
                    .path("icons/menu_caret.svg")
                    .size(px(20.))
                    .flex_none()
                    .text_color(tint("--text-muted")),
            ),
        )
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

    fn account(premium: u64) -> Account {
        Account {
            premium: Some(premium),
            guild_tier: None,
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
    fn prefs_round_trip_through_the_store_shape() {
        let prefs = Prefs {
            preset: Preset::Custom,
            resolution: Resolution::P1080,
            fps: 60,
            sound: false,
            hide_preview: true,
        };
        let value = json!({"preset": 3, "resolution": 1080, "fps": 60, "soundshareEnabled": false, "hide_preview": true});
        assert_eq!(Prefs::from_value(Some(&value)), prefs);
        // The key this client wrote before still reads.
        let old = json!({"preset": 3, "resolution": 1080, "fps": 60, "soundshare": false, "hide_preview": true});
        assert_eq!(Prefs::from_value(Some(&old)), prefs);
        // Source is stored as 0, like `RESOLUTION_SOURCE`.
        let source = json!({"preset": 2, "resolution": 0, "fps": 15});
        assert_eq!(
            Prefs::from_value(Some(&source)).resolution,
            Resolution::Source
        );
        // Junk keeps the defaults.
        let junk = json!({"preset": 99, "fps": 7, "resolution": 5});
        assert_eq!(Prefs::from_value(Some(&junk)), Prefs::default());
    }

    #[test]
    fn the_table_decides_what_each_account_may_use() {
        use Resolution::*;
        let none = account(0);
        let classic = account(1);
        let nitro = account(2);
        let basic = account(3);
        // 720p 30 and below for everyone.
        assert!(can_stream(none, Preset::Custom, P720, 30));
        assert!(can_stream(basic, Preset::Custom, P480, 15));
        // 720p 60, 1080p, 1440p: Mid (Classic, Nitro); Basic has no streaming perk.
        assert!(!can_stream(none, Preset::Custom, P720, 60));
        assert!(!can_stream(basic, Preset::Custom, P1080, 30));
        assert!(can_stream(classic, Preset::Custom, P1440, 60));
        // Source above 5 fps: High, Nitro only.
        assert!(!can_stream(classic, Preset::Custom, Source, 30));
        assert!(can_stream(nitro, Preset::Custom, Source, 60));
        // Source 5 fps belongs to Screenshare only, for anyone.
        assert!(can_stream(none, Preset::Documents, Source, 5));
        assert!(!can_stream(nitro, Preset::Custom, Source, 5));
        // Pairs outside the table never.
        assert!(!can_stream(nitro, Preset::Custom, P1080, 5));
        assert!(!can_stream(nitro, Preset::Custom, P720, 7));
    }

    #[test]
    fn a_boosted_server_unlocks_its_tiers() {
        use Resolution::*;
        let tier = |tier| Account {
            premium: Some(0),
            guild_tier: Some(tier),
        };
        assert!(can_stream(tier(1), Preset::Custom, P720, 60));
        assert!(!can_stream(tier(1), Preset::Custom, P1080, 30));
        assert!(can_stream(tier(2), Preset::Custom, P1440, 60));
        assert!(can_stream(tier(3), Preset::Custom, P1080, 60));
        // Source stays Nitro's.
        assert!(!can_stream(tier(3), Preset::Custom, Source, 30));
    }

    #[test]
    fn presets_pick_the_first_pair_the_account_may_use() {
        use Resolution::*;
        assert_eq!(preset_settings(Preset::Video, account(0)), Some((P720, 30)));
        assert_eq!(preset_settings(Preset::Video, account(3)), Some((P720, 30)));
        assert_eq!(
            preset_settings(Preset::Video, account(1)),
            Some((P1440, 60))
        );
        assert_eq!(
            preset_settings(Preset::Video, account(2)),
            Some((P1440, 60))
        );
        assert_eq!(
            preset_settings(Preset::Documents, account(0)),
            Some((Source, 5))
        );
        assert_eq!(
            preset_settings(Preset::Documents, account(1)),
            Some((Source, 5))
        );
        assert_eq!(
            preset_settings(Preset::Documents, account(2)),
            Some((Source, 15))
        );
        assert_eq!(preset_settings(Preset::Custom, account(2)), None);
    }

    #[test]
    fn the_stream_mode_rows_name_the_accounts_own_pairs() {
        assert_eq!(
            preset_subtext(Preset::Video, account(0)).as_deref(),
            Some("Smoother video (720p, 30fps)")
        );
        assert_eq!(
            preset_subtext(Preset::Documents, account(0)).as_deref(),
            Some("Clearer text (Source, 5fps)")
        );
        assert_eq!(
            preset_subtext(Preset::Video, account(2)).as_deref(),
            Some("Smoother video (1440p, 60fps)")
        );
        assert_eq!(
            preset_subtext(Preset::Documents, account(2)).as_deref(),
            Some("Clearer text (Source, 15fps)")
        );
        assert_eq!(preset_subtext(Preset::Custom, account(2)), None);
    }

    #[test]
    fn only_accounts_without_high_quality_see_sd_hd_and_the_ribbon() {
        assert!(account(0).needs_hd_upsell());
        assert!(account(1).needs_hd_upsell());
        assert!(account(3).needs_hd_upsell());
        assert!(!account(2).needs_hd_upsell());
    }

    #[test]
    fn the_modal_and_the_start_follow_the_official_fallbacks() {
        use Resolution::*;
        let custom = Prefs {
            preset: Preset::Custom,
            resolution: P1440,
            fps: 60,
            ..Prefs::default()
        };
        // A custom pair the account may not use opens as 720p 30, and starts as Gaming 720p 30.
        let opened = custom.for_modal(account(0));
        assert_eq!((opened.resolution, opened.fps), (P720, 30));
        assert_eq!(custom.start_choice(account(0)), (Preset::Video, P720, 30));
        assert_eq!(custom.start_choice(account(2)), (Preset::Custom, P1440, 60));
        // A preset starts with its own pair, whatever the stored one.
        let video = Prefs {
            preset: Preset::Video,
            ..custom
        };
        assert_eq!(video.start_choice(account(2)), (Preset::Video, P1440, 60));
        assert_eq!(video.shown(account(0)), (P720, 30));
        // Custom shows its chosen pair.
        assert_eq!(custom.shown(account(2)), (P1440, 60));
    }

    #[test]
    fn the_in_call_quality_choices_follow_module_600125() {
        use Resolution::*;
        let screenshare = Prefs {
            preset: Preset::Documents,
            resolution: Source,
            fps: 5,
            ..Prefs::default()
        };
        // A resolution from Screenshare takes 30 fps.
        assert_eq!(
            resolution_choice(&screenshare, account(2), P1080),
            (true, 30)
        );
        assert_eq!(
            resolution_choice(&screenshare, account(0), P1080),
            (false, 30)
        );
        assert_eq!(
            resolution_choice(&screenshare, account(0), P720),
            (true, 30)
        );
        // A frame rate keeps the stored resolution.
        let custom = Prefs {
            preset: Preset::Custom,
            resolution: P720,
            fps: 30,
            ..Prefs::default()
        };
        assert_eq!(frame_rate_choice(&custom, account(0), 60), (false, P720));
        assert_eq!(frame_rate_choice(&custom, account(0), 15), (true, P720));
        assert_eq!(frame_rate_choice(&custom, account(1), 60), (true, P720));
    }

    #[test]
    fn the_badge_reads_like_the_tile_pill() {
        assert_eq!(quality_badge(Resolution::P720, 30), "720p 30FPS");
        assert_eq!(quality_badge(Resolution::Source, 5), "Source 5FPS");
        // The Nitro wheel: 60 FPS, or 1080p and up (Source too) unless at 5 FPS.
        assert!(!label_is_premium("720p 30FPS"));
        assert!(label_is_premium("720p 60FPS"));
        assert!(label_is_premium("1080p 30FPS"));
        assert!(label_is_premium("1440p 15FPS"));
        assert!(!label_is_premium("1440p 5FPS"));
        assert!(label_is_premium("Source 30FPS"));
        assert!(!label_is_premium("Source 5FPS"));
        assert!(!label_is_premium("480p 30FPS"));
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
    fn the_spinner_shows_until_the_first_list_and_never_an_empty_string() {
        assert_eq!(content(true, Tab::Applications, 0), Content::Loading);
        assert_eq!(content(true, Tab::Devices, 0), Content::Loading);
        assert_eq!(content(false, Tab::Applications, 0), Content::Cards);
        assert_eq!(content(false, Tab::Screen, 0), Content::Cards);
        assert_eq!(content(false, Tab::Devices, 0), Content::NoDevices);
        assert_eq!(content(false, Tab::Devices, 1), Content::Cards);
    }

    #[test]
    fn thumbnails_come_for_the_shown_tab_first_then_the_missing_others() {
        let window_a = SourceId::Window(1);
        let window_b = SourceId::Window(2);
        let screen = SourceId::Display(9);
        let sources = [window_a, screen, window_b];
        let mut have = HashSet::new();
        assert_eq!(
            thumb_order(&sources, Tab::Applications, &have),
            vec![window_a, window_b, screen]
        );
        assert_eq!(
            thumb_order(&sources, Tab::Screen, &have),
            vec![screen, window_a, window_b]
        );
        // Once the other tab has its pictures, a round refreshes the shown tab only.
        have.insert(screen);
        assert_eq!(
            thumb_order(&sources, Tab::Applications, &have),
            vec![window_a, window_b]
        );
        have.insert(window_a);
        have.insert(window_b);
        assert_eq!(thumb_order(&sources, Tab::Screen, &have), vec![screen]);
        assert_eq!(
            thumb_order(&sources, Tab::Devices, &HashSet::new()),
            vec![window_a, screen, window_b]
        );
    }

    #[test]
    fn a_blank_first_frame_is_not_shown() {
        let black = dc_media::Picture::rgba(2, 1, vec![0, 0, 0, 255, 1, 2, 0, 255]).unwrap();
        assert!(is_blank(&black));
        let drawn = dc_media::Picture::rgba(2, 1, vec![0, 0, 0, 255, 90, 2, 0, 255]).unwrap();
        assert!(!is_blank(&drawn));
    }

    #[test]
    fn native_screen_names_read_like_the_official() {
        assert_eq!(screen_name("Entire screen"), "Entire Screen");
        assert_eq!(screen_name("Screen 2"), "Screen 2");
        assert_eq!(screen_name("Something"), "Something");
    }

    #[test]
    fn nitro_wheel_follows_account_and_boost_entitlements() {
        let free = account(0);
        let basic = account(3);
        let nitro = account(2);
        let boosted_t2 = Account {
            premium: Some(0),
            guild_tier: Some(2),
        };
        let boosted_t3 = Account {
            premium: Some(0),
            guild_tier: Some(3),
        };

        // Free user: 720p/30fps has no Nitro wheel; 1080p, 1440p, Source, and 60fps show it.
        assert!(!option_shows_premium(
            free,
            Preset::Custom,
            Resolution::P720,
            30
        ));
        assert!(option_shows_premium(
            free,
            Preset::Custom,
            Resolution::P720,
            60
        ));
        assert!(option_shows_premium(
            free,
            Preset::Custom,
            Resolution::P1080,
            30
        ));
        assert!(option_shows_premium(
            free,
            Preset::Custom,
            Resolution::P1440,
            30
        ));
        assert!(option_shows_premium(
            free,
            Preset::Custom,
            Resolution::Source,
            30
        ));

        // Nitro Basic does not grant StreamMidQuality or StreamHighQuality.
        assert!(!option_shows_premium(
            basic,
            Preset::Custom,
            Resolution::P720,
            30
        ));
        assert!(option_shows_premium(
            basic,
            Preset::Custom,
            Resolution::P720,
            60
        ));
        assert!(option_shows_premium(
            basic,
            Preset::Custom,
            Resolution::P1080,
            30
        ));

        // Nitro grants Mid and High streaming quality everywhere: no Nitro wheel on any option.
        for resolution in [
            Resolution::P720,
            Resolution::P1080,
            Resolution::P1440,
            Resolution::Source,
        ] {
            for fps in [15, 30, 60] {
                assert!(!option_shows_premium(
                    nitro,
                    Preset::Custom,
                    resolution,
                    fps
                ));
            }
        }

        // Guild Boost Tier 2/3 unlocks up to 1440p 60fps for free users, leaving only Source locked.
        for boosted in [boosted_t2, boosted_t3] {
            assert!(!option_shows_premium(
                boosted,
                Preset::Custom,
                Resolution::P1080,
                60
            ));
            assert!(!option_shows_premium(
                boosted,
                Preset::Custom,
                Resolution::P1440,
                60
            ));
            assert!(option_shows_premium(
                boosted,
                Preset::Custom,
                Resolution::Source,
                30
            ));
        }
    }
}
