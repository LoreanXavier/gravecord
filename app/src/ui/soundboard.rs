//! The voice panel's Soundboard (module 225123 gateway events, 967850 `soundButton`, 449933 grid, 681801 picker, 916885
//! upsell, 991119 header, 593586 panel button). Measured in `design/dom/popouts/voice-panel-soundboard-darker-cozy.json`,
//! `voice-sb-hover-darker-cozy.json` and `voice-sb-volume-darker-cozy.json`.
//!
//! What it does: `GET /soundboard-default-sounds` and op 31 `REQUEST_SOUNDBOARD_SOUNDS` (answered by
//! `SOUNDBOARD_SOUNDS`) fill the lists, `GUILD_SOUNDBOARD_SOUND_*` keep them current. A click on a sound posts
//! `POST /channels/{channel}/send-soundboard-sound` and plays it here at once; `VOICE_CHANNEL_EFFECT_SEND` plays other
//! people's sounds. A sound is an mp3 on the CDN (`/soundboard-sounds/{id}`), decoded once and played on the voice
//! speaker at `min(soundVolume * M(soundboardVolume) / 100 * min(outputVolume / 100, 1), 1)`.
//!
//! The TIP line names the Open Soundboard (Hold) keybind and opens the Soundboard Keybind modal (module 29681); the
//! wheel it opens over the game is `overlay_wheel.rs`. Left out, as the build has no dump or no counterpart: Frequently Used and the sticky section headers, the sound button's locked-click
//! upsell modal, the tooltip delay (app tooltips open at once), and a per-user "mute soundboard" (no setting for it).

use super::audio_menu::volume_to_slider;
use super::motion::{PopKind, PopSide, Popout as _};
use super::tooltip::{Body, Side, Tip};
use super::voice_panel::Popout;
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::{Request, Update};
use crate::ui::motion::Transitions as _;
use dc_state::{Id, permissions};
use dc_voice::sound::{self, Decoded, Handle, Output};
use gpui::{
    AnyElement, BoxShadow, Context, Entity, Focusable, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, ScrollHandle, SharedString, Subscription, Window, deferred, div, hsla, img,
    linear_color_stop, linear_gradient, point, prelude::*, px, svg,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::debug;

/// The guild id the official client gives the default sounds.
pub(super) const DISCORD: &str = "0";
const DEFAULTS_KEY: &str = "soundboard:defaults";
const SENT_KEY: &str = "soundboard:sent";
const UPSELL_DISMISSED: &str = "soundboard.upsell_dismissed";
const FAVORITES: &str = "soundboard.favorites";
const CDN: &str = "https://cdn.discordapp.com/soundboard-sounds/";
/// `MORE_SOUNDBOARD`: 200 slots; every server has 8 (the Soundboard settings page counts the same way).
const BASE_SLOTS: usize = 8;
const MORE_SLOTS: usize = 200;
const PLAY_START: &str = "SOUNDBOARD_PLAY_START";
const PLAY_END: &str = "SOUNDBOARD_PLAY_END";

/// Download Sound's limit on a fetched file (soundboard uploads are far smaller).
const DOWNLOAD_MAX_BYTES: usize = 8 << 20;

/// The file type of a downloaded sound (`switch (e.type)` of module 923819): `audio/mpeg` and `audio/mpeg3` are mp3,
/// `audio/ogg` is ogg; anything else cannot be named and the download fails.
pub(super) fn sound_extension(content_type: &str) -> Option<&'static str> {
    match content_type.split(';').next().unwrap_or_default().trim() {
        "audio/mpeg" | "audio/mpeg3" => Some("mp3"),
        "audio/ogg" => Some("ogg"),
        _ => None,
    }
}

/// The picker's box: 8px right of the panel, its bottom on the soundboard button's.
const PICKER_LEFT: f32 = 362.;
const PICKER_BOTTOM: f32 = 78.;
const PICKER_WIDTH: f32 = 531.;
const PICKER_HEIGHT: f32 = 520.;
const HEADER_HEIGHT: f32 = 64.;
/// Gift Nitro's picker: its header holds a title and a line above the search field (`customGiftHeader_0dhArF`).
const GIFT_HEADER_HEIGHT: f32 = 112.;
/// The keybind tip at the foot of that picker: 52 high, 64 in from the picker's left, 16 above its bottom, 451 wide.
const GIFT_TIP_LEFT: f32 = 64.;
const GIFT_TIP_BOTTOM: f32 = 16.;
const GIFT_TIP_WIDTH: f32 = 451.;
const GIFT_TIP_HEIGHT: f32 = 52.;
/// `DismissibleContent.SOUNDBOARD_KEYBIND_TIP` (module 554146).
const KEYBIND_TIP_CONTENT: u16 = 232;
const RAIL_WIDTH: f32 = 48.;
const NOTICE_HEIGHT: f32 = 154.;
const LIST_WIDTH: f32 = 483.;
const SECTION_HEIGHT: f32 = 32.;
const ROW_HEIGHT: f32 = 48.;
const BUTTON_WIDTH: f32 = 148.;
const BUTTON_HEIGHT: f32 = 40.;
const PER_ROW: usize = 3;
/// The volume menu inside the picker's box.
const MENU_LEFT: f32 = 507.;
const MENU_TOP: f32 = 30.;
const MENU_WIDTH: f32 = 155.7;
const MENU_HEIGHT: f32 = 76.;
/// A section is "current" once its top is this close to the list's top (the official scroll spy's slack).
const SPY_SLACK: f32 = 20.;

// ---- the data ----

/// One sound (`soundboard_sounds` in the gateway, the body of `/soundboard-default-sounds`).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Sound {
    pub id: String,
    /// The guild it belongs to; [`DISCORD`] for the defaults.
    pub guild: String,
    pub name: String,
    pub emoji_id: Option<String>,
    pub emoji_name: Option<String>,
    /// 0 to 1.
    pub volume: f64,
    pub available: bool,
}

fn text_of(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

impl Sound {
    pub(super) fn from_value(value: &Value, guild: &str) -> Option<Self> {
        Some(Self {
            id: text_of(&value["sound_id"])?,
            guild: text_of(&value["guild_id"]).unwrap_or_else(|| guild.to_owned()),
            name: value["name"].as_str()?.to_owned(),
            emoji_id: text_of(&value["emoji_id"]),
            emoji_name: text_of(&value["emoji_name"]),
            volume: value["volume"].as_f64().unwrap_or(1.),
            available: value["available"].as_bool().unwrap_or(true),
        })
    }

    pub(super) fn list(value: &Value, guild: &str) -> Vec<Self> {
        let mut sounds: Vec<Self> = value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|sound| Self::from_value(sound, guild))
            .collect();
        sort(&mut sounds);
        sounds
    }

    /// The body of `POST /channels/{channel}/send-soundboard-sound`: the default sounds have no source guild.
    pub(super) fn send_body(&self) -> Value {
        let mut body = json!({
            "sound_id": self.id,
            "emoji_id": self.emoji_id,
            "emoji_name": self.emoji_name,
        });
        if self.guild != DISCORD {
            body["source_guild_id"] = json!(self.guild);
        }
        body
    }
}

/// By snowflake, then the available ones first.
fn sort(sounds: &mut [Sound]) {
    sounds.sort_by_key(|sound| (!sound.available, sound.id.parse::<u64>().unwrap_or(0)));
}

/// What the picker knows about the caller and the call (`Ir`'s inputs).
#[derive(Clone, Debug, Default)]
pub(super) struct Ctx {
    /// The guild of the call's channel.
    pub current: Option<Id>,
    /// Nitro: sounds from every server (`canUseSoundboardEverywhere`).
    pub everywhere: bool,
    /// USE_EXTERNAL_SOUNDS in the call's channel.
    pub external: bool,
    pub can_create: bool,
    pub slots: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Access {
    Open,
    /// Another server's sound without the right to use it: drawn with a lock, never sent.
    Locked,
    /// Its server lost the boost level: "Requires higher Server Boost Level".
    Unavailable,
}

/// `Ir`: may this sound be played into this call?
pub(super) fn access(sound: &Sound, ctx: &Ctx) -> Access {
    let same = ctx
        .current
        .is_some_and(|guild| sound.guild == guild.to_string());
    let default = sound.guild == DISCORD;
    if !((ctx.everywhere && ctx.external) || same || default) {
        Access::Locked
    } else if !sound.available {
        Access::Unavailable
    } else {
        Access::Open
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Favorites,
    Guild(Id),
    Discord,
    Search,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Section {
    pub kind: Kind,
    pub title: String,
    pub sounds: Vec<Sound>,
    /// An Add Sound button ends the list: whether the user may use it.
    pub add: Option<bool>,
}

impl Section {
    fn key(&self) -> String {
        match &self.kind {
            Kind::Favorites => "favorites".to_owned(),
            Kind::Guild(guild) => format!("guild:{guild}"),
            Kind::Discord => "discord".to_owned(),
            Kind::Search => "search".to_owned(),
        }
    }

    fn items(&self) -> usize {
        self.sounds.len() + usize::from(self.add.is_some())
    }

    pub(super) fn height(&self, collapsed: bool) -> f32 {
        if collapsed {
            SECTION_HEIGHT
        } else {
            SECTION_HEIGHT + self.items().div_ceil(PER_ROW) as f32 * ROW_HEIGHT
        }
    }
}

/// How many sounds a server holds (module 473145 `fA` without boosts).
pub(super) fn slots(features: &[String]) -> usize {
    if features.iter().any(|feature| feature == "MORE_SOUNDBOARD") {
        MORE_SLOTS
    } else {
        BASE_SLOTS
    }
}

/// The picker's sections: Favorites, the call's server, Discord Sounds (last for Nitro users), then the servers whose
/// sounds this user may see.
pub(super) fn sections(
    ctx: &Ctx,
    guilds: &[(Id, String)],
    sounds: &HashMap<Id, Vec<Sound>>,
    defaults: &[Sound],
    favorites: &[String],
) -> Vec<Section> {
    let mut out = Vec::new();
    let everything = || {
        defaults.iter().chain(
            guilds
                .iter()
                .flat_map(|(id, _)| sounds.get(id).into_iter().flatten()),
        )
    };
    let favored: Vec<Sound> = favorites
        .iter()
        .filter_map(|id| everything().find(|sound| &sound.id == id).cloned())
        .collect();
    if !favored.is_empty() {
        out.push(Section {
            kind: Kind::Favorites,
            title: "Favorites".to_owned(),
            sounds: favored,
            add: None,
        });
    }
    let discord = Section {
        kind: Kind::Discord,
        title: "Discord Sounds".to_owned(),
        sounds: defaults.to_vec(),
        add: None,
    };
    if let Some((id, name)) = ctx
        .current
        .and_then(|current| guilds.iter().find(|(id, _)| *id == current))
    {
        let own = sounds.get(id).cloned().unwrap_or_default();
        let add = (own.len() < ctx.slots && ctx.can_create) || own.is_empty();
        out.push(Section {
            kind: Kind::Guild(*id),
            title: name.clone(),
            add: add.then_some(ctx.can_create),
            sounds: own,
        });
    }
    if !ctx.everywhere {
        out.push(discord.clone());
    }
    if ctx.external {
        for (id, name) in guilds.iter().filter(|(id, _)| Some(*id) != ctx.current) {
            let own = sounds.get(id).cloned().unwrap_or_default();
            if !own.is_empty() {
                out.push(Section {
                    kind: Kind::Guild(*id),
                    title: name.clone(),
                    sounds: own,
                    add: None,
                });
            }
        }
    }
    if ctx.everywhere {
        out.push(discord);
    }
    out
}

/// Every pixel the sections' tops sit below the list's top.
pub(super) fn tops(sections: &[Section], collapsed: &HashSet<String>) -> Vec<f32> {
    let mut at = 0.;
    sections
        .iter()
        .map(|section| {
            let top = at;
            at += section.height(collapsed.contains(&section.key()));
            top
        })
        .collect()
}

/// The section the rail lights: the last one whose top the scroll has reached.
pub(super) fn active_section(tops: &[f32], scrolled: f32) -> usize {
    tops.iter()
        .rposition(|top| *top <= scrolled + SPY_SLACK)
        .unwrap_or(0)
}

fn subsequence(text: &str, query: &str) -> bool {
    let mut wanted = query.chars();
    let mut next = wanted.next();
    for character in text.chars() {
        if Some(character) == next {
            next = wanted.next();
        }
    }
    next.is_none()
}

/// The search score: the name first, then the emoji's; 0 is no match, usable sounds get 100 on top.
pub(super) fn score(sound: &Sound, query: &str, usable: bool) -> u32 {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return 0;
    }
    let name = sound.name.to_lowercase();
    let emoji = sound.emoji_name.as_deref().unwrap_or("").to_lowercase();
    let has_emoji = !emoji.is_empty();
    let base = if name == query {
        8
    } else if has_emoji && emoji == query {
        7
    } else if name.starts_with(&query) {
        6
    } else if has_emoji && emoji.starts_with(&query) {
        5
    } else if name.ends_with(&query) {
        4
    } else if has_emoji && emoji.ends_with(&query) {
        3
    } else if subsequence(&name, &query) {
        2
    } else if has_emoji && subsequence(&emoji, &query) {
        1
    } else {
        0
    };
    if base > 0 && usable { base + 100 } else { base }
}

/// "Search Results": every sound of every server, best match first (equal scores keep their order).
pub(super) fn search(
    ctx: &Ctx,
    query: &str,
    guilds: &[(Id, String)],
    sounds: &HashMap<Id, Vec<Sound>>,
    defaults: &[Sound],
) -> Section {
    let mut found: Vec<(u32, Sound)> = defaults
        .iter()
        .chain(
            guilds
                .iter()
                .flat_map(|(id, _)| sounds.get(id).into_iter().flatten()),
        )
        .filter_map(|sound| {
            let usable = access(sound, ctx) == Access::Open;
            let score = score(sound, query, usable);
            (score > 0).then(|| (score, sound.clone()))
        })
        .collect();
    found.sort_by_key(|found| std::cmp::Reverse(found.0));
    Section {
        kind: Kind::Search,
        title: "Search Results".to_owned(),
        sounds: found.into_iter().map(|(_, sound)| sound).collect(),
        add: None,
    }
}

/// `M(soundboardVolume) / 100`: a stored volume to the gain the slider position means.
pub(super) fn slider_gain(soundboard_volume: f32) -> f32 {
    volume_to_slider(soundboard_volume) / 100.
}

/// `min(soundVolume * (M(soundboardVolume) / 100) * min(outputVolume / 100, 1), 1)`.
pub(super) fn playback_volume(sound_volume: f64, soundboard_volume: f32, output_gain: f32) -> f32 {
    (sound_volume as f32 * slider_gain(soundboard_volume) * output_gain.min(1.)).clamp(0., 1.)
}

/// `dc_voice::sound::play` multiplies its factor by the output volume itself; this is the factor that lands on
/// `official`.
pub(super) fn engine_factor(official: f32, output_gain: f32) -> f32 {
    if output_gain <= 0. {
        0.
    } else {
        official / output_gain
    }
}

/// `VOICE_CHANNEL_EFFECT_SEND` as far as sounds go.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Effect {
    pub sound_id: Option<String>,
    pub volume: f64,
    pub user: Option<Id>,
    pub channel: Option<Id>,
}

impl Effect {
    pub(super) fn from_value(value: &Value) -> Self {
        let id = |value: &Value| value.as_str().and_then(|text| text.parse::<Id>().ok());
        Self {
            sound_id: text_of(&value["sound_id"]),
            volume: value["sound_volume"].as_f64().unwrap_or(1.),
            user: id(&value["user_id"]),
            channel: id(&value["channel_id"]),
        }
    }
}

/// Is this effect a sound to play here: in our call's channel, a sound, from someone else, while we hear and have not
/// muted them.
pub(super) fn heard(
    effect: &Effect,
    call_channel: Option<Id>,
    me: Option<Id>,
    deafened: bool,
    muted: &HashSet<Id>,
) -> bool {
    let (Some(call), Some(user)) = (call_channel, effect.user) else {
        return false;
    };
    effect.sound_id.is_some()
        && effect.channel == Some(call)
        && Some(user) != me
        && !deafened
        && !muted.contains(&user)
}

/// The panel button's disabled reason, in the official order (`x()`): server mute, suppression, deafening.
pub(super) fn disabled_reason(mute: bool, suppress: bool, deaf: bool) -> Option<&'static str> {
    if mute {
        Some("Soundboard disabled when server muted")
    } else if suppress {
        Some("Soundboard disabled when suppressed")
    } else if deaf {
        Some("Soundboard disabled when deafened")
    } else {
        None
    }
}

/// The Discord epoch, for an account's age.
fn created_ms(user: Id) -> i64 {
    ((user >> 22) + 1_420_070_400_000) as i64
}

const THIRTY_DAYS_MS: i64 = 30 * 24 * 60 * 60 * 1000;

// ---- the state ----

/// The open picker.
pub(crate) struct Picker {
    search: Entity<Editor>,
    scroll: ScrollHandle,
    rail: ScrollHandle,
    menu: bool,
    _events: Subscription,
}

#[derive(Default)]
pub(crate) struct Board {
    defaults: Vec<Sound>,
    guilds: HashMap<Id, Vec<Sound>>,
    defaults_requested: bool,
    requested: HashSet<Id>,
    pub(super) picker: Option<Picker>,
    /// The picker "Add sound effect" of Gift Nitro opens (module 650588): the same picker over the default sounds
    /// only, its window position (the left and top of the button that opened it), and whether its keybind tip was
    /// closed.
    pub(super) gift_picker: Option<(Picker, gpui::Point<gpui::Pixels>)>,
    gift_tip_closed: bool,
    upsell_dismissed: bool,
    favorites: Vec<String>,
    collapsed: HashSet<String>,
    /// Sounds playing now, by sound and by who started them (`PLAY_START` / `PLAY_END`).
    playing: HashMap<String, usize>,
    user_playing: HashMap<Id, usize>,
    handles: Vec<Handle>,
    decoded: Arc<Mutex<HashMap<String, Arc<Decoded>>>>,
}

impl Board {
    pub(crate) fn new() -> Self {
        Self {
            upsell_dismissed: crate::storage::get(UPSELL_DISMISSED)
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            favorites: crate::storage::get(FAVORITES)
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default(),
            ..Self::default()
        }
    }

    fn replace(&mut self, guild: Id, mut sounds: Vec<Sound>) {
        sort(&mut sounds);
        self.guilds.insert(guild, sounds);
    }

    fn upsert(&mut self, guild: Id, sound: Sound) {
        let list = self.guilds.entry(guild).or_default();
        match list.iter_mut().find(|known| known.id == sound.id) {
            Some(known) => *known = sound,
            None => list.push(sound),
        }
        sort(list);
    }

    fn remove(&mut self, guild: Id, sound: &str) {
        if let Some(list) = self.guilds.get_mut(&guild) {
            list.retain(|known| known.id != sound);
        }
    }

    fn started(&mut self, sound: &str, user: Option<Id>) {
        *self.playing.entry(sound.to_owned()).or_default() += 1;
        if let Some(user) = user {
            *self.user_playing.entry(user).or_default() += 1;
        }
    }

    fn ended(&mut self, sound: &str, user: Option<Id>) {
        if let Some(count) = self.playing.get_mut(sound) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.playing.remove(sound);
            }
        }
        if let Some(user) = user
            && let Some(count) = self.user_playing.get_mut(&user)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.user_playing.remove(&user);
            }
        }
    }

    /// `isUserPlayingSounds`: the voice member's ring.
    pub(super) fn user_playing(&self, user: Id) -> bool {
        self.user_playing.contains_key(&user)
    }

    /// Ends every sound still playing (leaving the channel, deafening).
    pub(super) fn stop_all(&mut self) {
        for handle in self.handles.drain(..) {
            handle.stop();
        }
    }

    fn toggle_favorite(&mut self, sound: &str) {
        if let Some(at) = self.favorites.iter().position(|known| known == sound) {
            self.favorites.remove(at);
        } else {
            self.favorites.push(sound.to_owned());
        }
        crate::storage::set(FAVORITES, json!(self.favorites));
    }
}

fn parse_id(value: &Value) -> Option<Id> {
    value.as_str().and_then(|text| text.parse::<Id>().ok())
}

impl Root {
    // ---- panel button ----

    /// Why the panel's soundboard button is dead, in the tooltip's words.
    pub(super) fn soundboard_disabled(&self) -> Option<&'static str> {
        let call = self.call.as_ref()?;
        let me = self.stores.current_user.as_ref()?.id;
        let (mute, suppress) = self
            .stores
            .voice_members(call.guild, call.channel)
            .into_iter()
            .find(|state| state.user_id == me)
            .map_or((false, false), |state| (state.mute, state.suppress));
        disabled_reason(mute, suppress, self.self_deaf)
    }

    pub(super) fn toggle_soundboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.soundboard_disabled().is_some() || self.call.is_none() {
            return;
        }
        self.toggle_panel_popout(Popout::Soundboard, cx);
        if self.panel.popout == Some(Popout::Soundboard) {
            self.soundboard_open(window, cx);
        }
    }

    fn soundboard_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Find the perfect sound".to_owned());
            editor
        });
        let events = cx.subscribe(&search, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Cancel => this.close_panel_popout(cx),
                EditorEvent::Edited => {
                    if let Some(picker) = this.soundboard.picker.as_ref() {
                        picker.scroll.set_offset(point(px(0.), px(0.)));
                    }
                }
                EditorEvent::Submit(_) => {}
            }
            cx.notify();
        });
        window.focus(&gpui::Focusable::focus_handle(search.read(cx), cx), cx);
        self.soundboard.picker = Some(Picker {
            search,
            scroll: ScrollHandle::new(),
            rail: ScrollHandle::new(),
            menu: false,
            _events: events,
        });
        self.soundboard_request();
        let known: Vec<Sound> = self
            .soundboard
            .defaults
            .iter()
            .chain(self.soundboard.guilds.values().flatten())
            .cloned()
            .collect();
        self.request_sound_images(&known);
        cx.notify();
    }

    /// The defaults, and every server's sounds once per session (op 31).
    fn soundboard_request(&mut self) {
        if !self.soundboard.defaults_requested {
            self.soundboard.defaults_requested = true;
            let _ = self.requests.send(Request::Fetch {
                key: DEFAULTS_KEY.to_owned(),
                route: "/soundboard-default-sounds".to_owned(),
            });
        }
        let missing: Vec<Id> = self
            .stores
            .guilds
            .iter()
            .map(|guild| guild.id)
            .filter(|guild| !self.soundboard.requested.contains(guild))
            .collect();
        if !missing.is_empty() {
            self.soundboard.requested.extend(missing.iter().copied());
            let _ = self
                .requests
                .send(Request::SoundboardSounds { guilds: missing });
        }
    }

    fn request_sound_images(&mut self, sounds: &[Sound]) {
        for sound in sounds {
            if let Some(id) = &sound.emoji_id {
                self.request_image(
                    format!("custom:{id}"),
                    format!("https://cdn.discordapp.com/emojis/{id}.webp?size=48"),
                );
            } else if let Some(name) = &sound.emoji_name {
                self.request_emoji_image(name);
            }
        }
    }

    // ---- gateway ----

    pub(super) fn soundboard_dispatch(&mut self, name: &str, data: &Value, cx: &mut Context<Self>) {
        match name {
            "SOUNDBOARD_SOUNDS" | "GUILD_SOUNDBOARD_SOUNDS_UPDATE" => {
                let Some(guild) = parse_id(&data["guild_id"]) else {
                    return;
                };
                let sounds = Sound::list(&data["soundboard_sounds"], &guild.to_string());
                self.request_sound_images(&sounds);
                self.soundboard.replace(guild, sounds);
                self.soundboard_changed(guild, cx);
            }
            "GUILD_SOUNDBOARD_SOUND_CREATE" | "GUILD_SOUNDBOARD_SOUND_UPDATE" => {
                let Some(guild) = parse_id(&data["guild_id"]) else {
                    return;
                };
                if let Some(sound) = Sound::from_value(data, &guild.to_string()) {
                    self.request_sound_images(std::slice::from_ref(&sound));
                    self.soundboard.upsert(guild, sound);
                    self.soundboard_changed(guild, cx);
                }
            }
            "GUILD_SOUNDBOARD_SOUND_DELETE" => {
                let (Some(guild), Some(sound)) =
                    (parse_id(&data["guild_id"]), text_of(&data["sound_id"]))
                else {
                    return;
                };
                self.soundboard.remove(guild, &sound);
                self.soundboard_changed(guild, cx);
            }
            "VOICE_CHANNEL_EFFECT_SEND" => {
                let effect = Effect::from_value(data);
                let me = self.stores.current_user.as_ref().map(|user| user.id);
                if heard(
                    &effect,
                    self.call.as_ref().map(|call| call.channel),
                    me,
                    self.self_deaf,
                    &self.context.soundboard_muted,
                ) && let Some(sound) = &effect.sound_id
                {
                    self.soundboard_play(sound, effect.volume, effect.user, 1.);
                }
            }
            PLAY_START | PLAY_END => {
                let Some(sound) = text_of(&data["sound_id"]) else {
                    return;
                };
                let user = parse_id(&data["user_id"]);
                if name == PLAY_START {
                    self.soundboard.started(&sound, user);
                } else {
                    self.soundboard.ended(&sound, user);
                }
                cx.notify();
            }
            _ => {}
        }
    }

    /// A server's list changed: the Soundboard page of that server's settings reads its own copy.
    fn soundboard_changed(&mut self, guild: Id, cx: &mut Context<Self>) {
        if self
            .server_settings
            .as_ref()
            .is_some_and(|open| open.guild == guild)
        {
            self.request_sounds(guild);
        }
        cx.notify();
    }

    pub(super) fn soundboard_fetched(&mut self, key: &str, body: &Value, cx: &mut Context<Self>) {
        match key {
            DEFAULTS_KEY => {
                let sounds = Sound::list(body, DISCORD);
                self.request_sound_images(&sounds);
                self.soundboard.defaults = sounds;
            }
            SENT_KEY => {
                debug!(
                    status = body["status"].as_u64().unwrap_or(0),
                    ok = body["ok"].as_bool().unwrap_or(false),
                    "soundboard sound sent"
                );
            }
            _ => {}
        }
        cx.notify();
    }

    // ---- playing ----

    /// Fetches, decodes and plays a sound on the voice speaker. `scale` is 1 for the sound's own volume.
    fn soundboard_play(&mut self, sound: &str, volume: f64, user: Option<Id>, scale: f32) {
        // The id goes into a URL: only digits.
        if sound.is_empty() || !sound.bytes().all(|byte| byte.is_ascii_digit()) {
            return;
        }
        let output_gain = dc_voice::prefs().output_gain();
        let official = (playback_volume(volume, super::settings::soundboard_volume(), output_gain)
            * scale)
            .clamp(0., 1.);
        let factor = engine_factor(official, output_gain);
        let handle = Handle::new();
        self.soundboard.handles.retain(|known| !known.is_finished());
        self.soundboard.handles.push(handle.clone());
        let (cache, updates, id) = (
            self.soundboard.decoded.clone(),
            self.updates.clone(),
            sound.to_owned(),
        );
        let spawned = std::thread::Builder::new()
            .name("gravecord-soundboard".into())
            .spawn(move || {
                let cached = cache.lock().ok().and_then(|known| known.get(&id).cloned());
                let decoded = match cached {
                    Some(decoded) => decoded,
                    None => {
                        let bytes = match crate::sounds::fetch_url(&format!("{CDN}{id}")) {
                            Ok(bytes) => bytes,
                            Err(error) => {
                                debug!("soundboard file unavailable: {error}");
                                return;
                            }
                        };
                        match sound::decode_any(bytes) {
                            Ok(decoded) => {
                                let decoded = Arc::new(decoded);
                                if let Ok(mut known) = cache.lock() {
                                    known.insert(id.clone(), decoded.clone());
                                }
                                decoded
                            }
                            Err(error) => {
                                debug!("soundboard file not playable: {error}");
                                return;
                            }
                        }
                    }
                };
                if handle.is_stopped() {
                    return;
                }
                let event = |name: &str| Update::Dispatch {
                    name: name.to_owned(),
                    data: json!({"sound_id": id, "user_id": user.map(|user| user.to_string())}),
                };
                let _ = updates.send(event(PLAY_START));
                sound::play(decoded.clone(), factor, Output::Voice, &handle);
                let (started, length) = (Instant::now(), decoded.duration());
                while !handle.is_stopped() && started.elapsed() < length && !handle.is_finished() {
                    std::thread::sleep(Duration::from_millis(50));
                }
                let _ = updates.send(event(PLAY_END));
            });
        if spawned.is_err() {
            debug!("could not start the soundboard player");
        }
    }

    /// Leaving the channel or deafening: whatever the soundboard plays stops.
    pub(super) fn soundboard_stop(&mut self) {
        self.soundboard.stop_all();
    }

    fn soundboard_ctx(&self) -> Ctx {
        let Some(call) = self.call.as_ref() else {
            return Ctx::default();
        };
        let me = self.stores.me();
        let guild = self.stores.guild(call.guild);
        let channel = guild.and_then(|guild| guild.channels.iter().find(|c| c.id == call.channel));
        let external = matches!((guild, channel, me), (Some(guild), Some(channel), Some(me))
            if permissions::can_in_channel(guild, channel, me, permissions::USE_EXTERNAL_SOUNDS));
        let can_create = matches!((guild, me), (Some(guild), Some(me))
            if permissions::can_in_guild(guild, me, permissions::CREATE_GUILD_EXPRESSIONS)
                || permissions::can_in_guild(guild, me, permissions::MANAGE_GUILD_EXPRESSIONS));
        Ctx {
            current: guild.map(|guild| guild.id),
            everywhere: dc_state::premium::PremiumType::from_wire(self.premium_wire())
                .has(dc_state::premium::Feature::SoundboardEverywhere),
            external,
            can_create,
            slots: guild.map_or(BASE_SLOTS, |guild| slots(&guild.features)),
        }
    }

    /// A press on a sound: it goes to the channel and plays here at once.
    fn soundboard_click(&mut self, sound: &Sound, cx: &mut Context<Self>) {
        let Some(channel) = self.call.as_ref().map(|call| call.channel) else {
            return;
        };
        if self.soundboard_disabled().is_some()
            || access(sound, &self.soundboard_ctx()) != Access::Open
        {
            return;
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(SENT_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/send-soundboard-sound"),
            body: Some(sound.send_body()),
        });
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        self.soundboard_play(&sound.id, sound.volume, me, 1.);
        cx.notify();
    }

    /// A press on a sound from elsewhere (the overlay's soundboard wheel, module 177303 `p.Ak`).
    pub(super) fn soundboard_click_sound(&mut self, sound: &Sound, cx: &mut Context<Self>) {
        self.soundboard_click(sound, cx);
    }

    /// The wheel opens (`f.E7()`, `loadIfNecessary`): the sounds and their emoji are asked for once.
    pub(super) fn soundboard_load_for_wheel(&mut self) {
        self.soundboard_request();
        let known: Vec<Sound> = self
            .soundboard
            .defaults
            .iter()
            .chain(self.soundboard.guilds.values().flatten())
            .cloned()
            .collect();
        self.request_sound_images(&known);
    }

    /// `C` of module 177303 over `y.Y(channel, true)`: the call's server first, then (with Soundboard Everywhere and
    /// Use External Sounds) every other server, then the defaults; favourites first, only available sounds.
    pub(super) fn soundboard_wheel_sounds(&self) -> Vec<Sound> {
        let ctx = self.soundboard_ctx();
        let current = self
            .call
            .as_ref()
            .map(|call| call.guild)
            .filter(|guild| *guild != 0);
        let mut guilds: Vec<String> = current.iter().map(|guild| guild.to_string()).collect();
        if ctx.everywhere && (current.is_none() || ctx.external) {
            for guild in &self.stores.guilds {
                if Some(guild.id) != current {
                    guilds.push(guild.id.to_string());
                }
            }
        }
        let all: Vec<&Sound> = self
            .soundboard
            .defaults
            .iter()
            .chain(self.soundboard.guilds.values().flatten())
            .collect();
        let plain: Vec<dc_overlay::wheel::Sound> = all
            .iter()
            .map(|sound| dc_overlay::wheel::Sound {
                id: sound.id.clone(),
                guild: sound.guild.clone(),
                available: sound.available,
            })
            .collect();
        dc_overlay::wheel::order(&plain, &guilds, &self.soundboard.favorites)
            .into_iter()
            .filter_map(|picked| {
                all.iter()
                    .find(|sound| sound.id == picked.id && sound.guild == picked.guild)
                    .map(|sound| (*sound).clone())
            })
            .collect()
    }

    /// A sound of the wheel (`S.Ay` with `className: soundButton_JhiVh1`, 96 by 52 with the border and high shadows):
    /// its emoji and name; the selected one (`focused`) shows the hover look with the play glyph.
    pub(super) fn soundboard_wheel_item(&self, sound: &Sound, focused: bool) -> AnyElement {
        let picture = sound
            .emoji_id
            .as_ref()
            .and_then(|id| self.images.get(&format!("custom:{id}")))
            .or_else(|| {
                sound
                    .emoji_name
                    .as_ref()
                    .and_then(|name| self.images.get(&format!("emoji:{name}")))
            })
            .cloned();
        let onyx = super::overlay::onyx;
        div()
            .relative()
            .w(px(96.))
            .h(px(52.))
            .rounded(px(8.))
            .bg(onyx("--background-base-lower"))
            // `--shadow-border` then `--shadow-high` of the dark themes.
            .shadow(vec![
                BoxShadow {
                    color: hsla(0., 0., 1., 0.08),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.24),
                    offset: point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
            .child(
                div()
                    .size_full()
                    .px(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if focused {
                        onyx("--border-subtle")
                    } else {
                        gpui::rgba(0)
                    })
                    .bg(onyx("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(20.))
                            .flex_none()
                            .children(picture.map(|picture| img(picture).size(px(20.)))),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(onyx("--text-strong"))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(sound.name.clone()),
                    ),
            )
            .when(focused, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(7.))
                        .bg(onyx("--background-surface-highest"))
                        .opacity(0.8)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path("icons/sb_play.svg")
                                .size(px(20.))
                                .text_color(onyx("--text-strong")),
                        ),
                )
            })
            .into_any_element()
    }

    /// The pointer entered or left the bar's soundboard button or its popup: the defaults load on the first entry.
    pub(super) fn soundboard_hover(&mut self, inside: bool, cx: &mut Context<Self>) {
        if inside && !self.vv.suggest {
            self.soundboard_request();
            let known: Vec<Sound> = self.soundboard.defaults.clone();
            self.request_sound_images(&known);
        }
        self.vv.suggest = inside;
        cx.notify();
    }

    /// Suggested Sounds (the hover popup over the bar's soundboard button): the first six default sounds in a 3x2
    /// grid with See All. Measured in `design/dom/popouts/stream-stopped-darker-cozy.json`: 494x152, centred over the
    /// button, padding 16, tiles 148x40 with an 8px gap.
    pub(super) fn soundboard_suggest(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        const WIDTH: f32 = 494.;
        const HEIGHT: f32 = 152.;
        if !self.vv.suggest
            || self.panel.popout == Some(Popout::Soundboard)
            || self.soundboard_disabled().is_some()
        {
            return Vec::new();
        }
        let Some(anchor) = self
            .tip_anchors
            .borrow()
            .get("call-soundboard-tip")
            .copied()
        else {
            return Vec::new();
        };
        let sounds: Vec<&Sound> = self
            .soundboard
            .defaults
            .iter()
            .filter(|sound| sound.available)
            .take(6)
            .collect();
        if sounds.is_empty() {
            return Vec::new();
        }
        let ctx = self.soundboard_ctx();
        let left = f32::from(anchor.left()) + f32::from(anchor.size.width) / 2.
            - WIDTH / 2.
            - self.combined_sidebar_width();
        let top = f32::from(anchor.top()) - 17. - HEIGHT - 32.;
        let rows: Vec<AnyElement> = sounds
            .chunks(3)
            .enumerate()
            .map(|(row, chunk)| {
                div()
                    .flex()
                    .gap(px(8.))
                    .children(chunk.iter().map(|sound| {
                        self.soundboard_button(
                            &format!("suggest{row}"),
                            sound,
                            &ctx,
                            Side::Top,
                            false,
                            cx,
                        )
                    }))
                    .into_any_element()
            })
            .collect();
        vec![
            deferred(
                // The padding below bridges the gap to the button so the pointer can cross it.
                div()
                    .id("soundboard-suggest")
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .pb(px(8.))
                    .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                        this.soundboard_hover(*inside, cx);
                    }))
                    .child(
                        div()
                            .w(px(WIDTH))
                            .h(px(HEIGHT))
                            .p(px(16.))
                            .rounded(px(12.))
                            .bg(color("--background-base-lower"))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .occlude()
                            .child(
                                div()
                                    .h(px(24.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(icon("voice_soundboard", 16., "--text-default"))
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(18.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--text-default"))
                                                    .child("Suggested Sounds"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("soundboard-see-all")
                                            .cursor_pointer()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-link"))
                                            .hover(|style| style.underline())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.vv.suggest = false;
                                                this.toggle_soundboard(window, cx);
                                            }))
                                            .child("See All"),
                                    ),
                            )
                            .children(rows),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        ]
    }

    /// The speaker on a hovered sound: heard here only, nothing is sent.
    fn soundboard_preview(&mut self, sound: &Sound) {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        self.soundboard_play(&sound.id, sound.volume, me, 1.);
    }

    // ---- the picker ----

    /// Add Sound: that server's Soundboard settings, with the upload dialog when the user may upload.
    fn soundboard_add(
        &mut self,
        guild: Id,
        allowed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_panel_popout(cx);
        self.open_server_settings(guild, cx);
        if let Some(open) = self.server_settings.as_mut() {
            open.page = super::server_settings::Page::Soundboard;
        }
        self.request_sounds(guild);
        if allowed {
            self.open_sound_dialog(guild, window, cx);
        }
        cx.notify();
    }

    fn upsell_wanted(&self) -> bool {
        if self.soundboard.upsell_dismissed {
            return false;
        }
        let nitro = dc_state::premium::PremiumType::from_wire(self.premium_wire())
            .has(dc_state::premium::Feature::SoundboardEverywhere);
        let old = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|user| crate::sounds::now_ms() - created_ms(user.id) > THIRTY_DAYS_MS);
        nitro || old
    }

    pub(super) fn soundboard_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.soundboard_picker_in(false, window, cx)
    }

    /// "Add sound effect" of Gift Nitro opens the picker over the button's left and top corner.
    pub(super) fn soundboard_gift_open(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.soundboard.gift_picker.is_some() {
            self.soundboard_gift_close(window, cx);
            return;
        }
        let search = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Find the perfect sound".to_owned());
            editor
        });
        let events = cx.subscribe(&search, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Cancel => this.soundboard.gift_picker = None,
                EditorEvent::Edited => {
                    if let Some((picker, _)) = this.soundboard.gift_picker.as_ref() {
                        picker.scroll.set_offset(point(px(0.), px(0.)));
                    }
                }
                EditorEvent::Submit(_) => {}
            }
            cx.notify();
        });
        window.focus(&gpui::Focusable::focus_handle(search.read(cx), cx), cx);
        self.soundboard.gift_picker = Some((
            Picker {
                search,
                scroll: ScrollHandle::new(),
                rail: ScrollHandle::new(),
                menu: false,
                _events: events,
            },
            at,
        ));
        // Only the default sounds are listed (`defaultSoundsOnly`).
        if !self.soundboard.defaults_requested {
            self.soundboard.defaults_requested = true;
            let _ = self.requests.send(Request::Fetch {
                key: DEFAULTS_KEY.to_owned(),
                route: "/soundboard-default-sounds".to_owned(),
            });
        }
        let known = self.soundboard.defaults.clone();
        self.request_sound_images(&known);
        cx.notify();
    }

    pub(super) fn soundboard_gift_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.soundboard.gift_picker.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// A sound picked in Gift Nitro's picker: kept on the step (module 650588 `onSelect`), the picker closes.
    fn soundboard_gift_choose(
        &mut self,
        sound: &Sound,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_gift_sound(
            sound.id.clone(),
            sound.name.clone(),
            sound
                .emoji_id
                .is_none()
                .then(|| sound.emoji_name.clone())
                .flatten(),
        );
        self.soundboard_gift_close(window, cx);
    }

    /// The open picker of the call's popout, or of Gift Nitro's sound effect.
    fn soundboard_picker_mut(&mut self, gift: bool) -> Option<&mut Picker> {
        if gift {
            self.soundboard
                .gift_picker
                .as_mut()
                .map(|(picker, _)| picker)
        } else {
            self.soundboard.picker.as_mut()
        }
    }

    pub(super) fn soundboard_gift_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.soundboard_picker_in(true, window, cx)
    }

    /// The picker of the call's soundboard popout (`gift` false) or of Gift Nitro's sound effect (module 650588:
    /// `defaultSoundsOnly`, `listPadding` [54, 8, 8, 8], a header of its own, the sounds' "Add" overlay).
    fn soundboard_picker_in(
        &mut self,
        gift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let picker = if gift {
            &self.soundboard.gift_picker.as_ref()?.0
        } else {
            self.soundboard.picker.as_ref()?
        };
        let gift_at = self
            .soundboard
            .gift_picker
            .as_ref()
            .map(|(_, at)| *at)
            .filter(|_| gift);
        let head = if gift {
            GIFT_HEADER_HEIGHT
        } else {
            HEADER_HEIGHT
        };
        let (search, scroll, rail_scroll, menu) = (
            picker.search.clone(),
            picker.scroll.clone(),
            picker.rail.clone(),
            picker.menu,
        );
        let ctx = self.soundboard_ctx();
        let guilds: Vec<(Id, String)> = self
            .stores
            .guilds
            .iter()
            .map(|guild| (guild.id, guild.name.clone()))
            .collect();
        let query = search.read(cx).text().trim().to_owned();
        let searching = !query.is_empty();
        let mut base = sections(
            &ctx,
            &guilds,
            &self.soundboard.guilds,
            &self.soundboard.defaults,
            &self.soundboard.favorites,
        );
        if gift {
            base.retain(|section| section.kind == Kind::Discord);
        }
        let shown = if searching {
            let mut found = search_section(&ctx, &query, &guilds, &self.soundboard);
            if gift {
                found.sounds.retain(|sound| sound.guild == DISCORD);
            }
            vec![found]
        } else {
            base.clone()
        };
        let notice = !gift && self.upsell_wanted();
        let list_top = if notice { head + NOTICE_HEIGHT } else { head };
        let list_height = PICKER_HEIGHT - list_top;
        let shown_tops = tops(&shown, &self.soundboard.collapsed);
        let scrolled = -f32::from(scroll.offset().y);
        let active = (!searching).then(|| active_section(&shown_tops, scrolled));
        let focused = search.read(cx).focus_handle(cx).is_focused(window);

        // The category rail: one tile per section, the lit one has the pill.
        let base_tops = tops(&base, &self.soundboard.collapsed);
        let mut rail_items = Vec::new();
        for (index, section) in base.iter().enumerate() {
            let selected = active == Some(index);
            let target = base_tops[index];
            let scroll = scroll.clone();
            let tile = match &section.kind {
                Kind::Guild(guild) => {
                    let picture = self.images.get(&format!("guild:{guild}")).cloned();
                    let acronym = dc_state::Guild::acronym_of(&section.title);
                    div()
                        .size(px(32.))
                        .rounded(px(8.))
                        .overflow_hidden()
                        .bg(color("--background-base-low"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.))
                        .line_height(px(12.))
                        .text_color(color("--text-default"))
                        .map(|this| match picture {
                            Some(picture) => this.child(img(picture).size(px(32.))),
                            None => this.child(acronym),
                        })
                        .into_any_element()
                }
                kind => div()
                    .size(px(32.))
                    .p(px(4.))
                    .rounded(px(4.))
                    .when(selected, |this| this.bg(color("--background-base-low")))
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .child(icon(
                        if *kind == Kind::Favorites {
                            "sb_star"
                        } else {
                            "sb_discord"
                        },
                        24.,
                        "--interactive-text-default",
                    ))
                    .into_any_element(),
            };
            rail_items.push(
                div()
                    .id(SharedString::from(format!("sb-rail-{index}")))
                    .relative()
                    .flex_none()
                    .size(px(32.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let max = f32::from(scroll.max_offset().y).max(0.);
                        scroll.set_offset(point(px(0.), px(-target.min(max))));
                        cx.notify();
                    }))
                    .child(tile)
                    .when(selected, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(px(-12.))
                                .top(px(4.))
                                .w(px(8.))
                                .h(px(24.))
                                .rounded(px(4.))
                                .bg(color("--text-strong")),
                        )
                    })
                    .into_any_element(),
            );
        }
        let rail = div()
            .id("sb-rail")
            .absolute()
            .left_0()
            .top(px(HEADER_HEIGHT + 1.))
            .w(px(RAIL_WIDTH))
            .h(px(PICKER_HEIGHT - HEADER_HEIGHT - 1.))
            .bg(color("--background-base-lower"))
            .overflow_y_scroll()
            .track_scroll(&rail_scroll)
            .child(
                div()
                    .px(px(8.))
                    // The gift picker's list padding is 54 at the top (`listPadding` [54, 8, 8, 8]).
                    .pt(px(if gift { 54. } else { 8. }))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(rail_items),
            );

        // The list.
        let mut content = Vec::new();
        for (index, section) in shown.iter().enumerate() {
            content.push(self.soundboard_section(
                section,
                shown_tops[index],
                list_top,
                scrolled,
                &ctx,
                searching,
                gift,
                cx,
            ));
        }
        if searching && shown[0].sounds.is_empty() {
            content.clear();
        }
        let empty = searching && shown[0].sounds.is_empty();
        let list = div()
            .absolute()
            .left(px(RAIL_WIDTH))
            .top(px(list_top))
            .w(px(LIST_WIDTH))
            .h(px(list_height))
            .child(
                div()
                    .id("sb-list")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .child(
                        div()
                            .w(px(LIST_WIDTH - 8.))
                            .flex()
                            .flex_col()
                            .children(content),
                    ),
            )
            .child(super::settings::auto_scrollbar(scroll.clone()))
            .when(empty, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--primary-400"))
                        .child("No sounds match your search"),
                )
            });

        let notice_view = notice.then(|| self.soundboard_notice(cx));

        // The header: the search field and the volume icon.
        let header = div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(head))
            // The gift header is the surface colour (36, 36, 41 in the capture), the call picker's the base one.
            .bg(color(if gift {
                "--background-surface-high"
            } else {
                "--background-base-low"
            }))
            .shadow(vec![
                BoxShadow {
                    color: hsla(0., 0., 0., 0.2),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(0.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.04),
                    offset: point(px(0.), px(1.5)),
                    blur_radius: px(0.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.04),
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(0.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
            .child(
                div()
                    .absolute()
                    .left(px(12.))
                    .top(px(if gift { 60. } else { 12. }))
                    .w(px(471.))
                    .h(px(40.))
                    .rounded(px(8.))
                    // Focused, the capture draws the active colour 2px wide.
                    .when(focused, |this| this.border_2().pl(px(3.)))
                    .when(!focused, |this| this.border_1().pl(px(4.)))
                    .border_color(if focused {
                        color("--input-border-active")
                    } else {
                        color("--input-border-default")
                    })
                    .rule(crate::ui::motion::Rule::TextField)
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .size(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                svg()
                                    .path("icons/picker_search.svg")
                                    .size(px(16.))
                                    .text_color(color("--input-text-default")),
                            ),
                    )
                    .child(div().flex_1().min_w(px(0.)).pr(px(12.)).child(search)),
            )
            .child(
                div()
                    .id("sb-volume")
                    .absolute()
                    .left(px(495.))
                    .top(px(if gift { 66.5 } else { 18.5 }))
                    .w(px(24.))
                    .h(px(27.))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            if let Some(picker) = this.soundboard_picker_mut(gift) {
                                picker.menu = !picker.menu;
                            }
                            // The press leaves the search field, as the official icon button takes focus.
                            window.blur(cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(icon("sb_speaker", 24., "--interactive-text-default")),
            );

        let picker_box = div()
            .id("soundboard-picker")
            .relative()
            .size_full()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--background-base-low"))
            .shadow(vec![
                BoxShadow {
                    color: hsla(0., 0., 1., 0.08),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.24),
                    offset: point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if let Some(picker) = this.soundboard_picker_mut(gift)
                        && picker.menu
                    {
                        picker.menu = false;
                        cx.notify();
                    }
                }),
            )
            .child(rail)
            .child(list)
            .children(notice_view)
            .child(header)
            .when(gift, |this| {
                // The title and its line over the search field (`customGiftHeader_0dhArF`: 12px in, body-md/semibold
                // and body-sm/normal).
                this.child(
                    div()
                        .absolute()
                        .left(px(12.))
                        .top(px(12.))
                        .w(px(507.))
                        .flex()
                        .flex_col()
                        .text_color(color("--text-default"))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .child("Add sound effect"),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .child("This will play when your friend opens their gift"),
                        ),
                )
            })
            // The TIP line under the sounds, in both pickers.
            .children(self.soundboard_gift_tip(cx));

        let wrapper = if let Some(at) = gift_at {
            // Above the button, held inside the window (the capture's picker is clamped to the top edge).
            let viewport = f32::from(window.viewport_size().height);
            let top = (f32::from(at.y) - 8. - PICKER_HEIGHT)
                .clamp(0., (viewport - PICKER_HEIGHT).max(0.));
            // Layout snaps to whole pixels: the button's 224.5 is 225 in the capture.
            let left = f32::from(at.x).round();
            div()
                .id("soundboard-gift-popout")
                .popout("soundboard-gift-popout", PopKind::Translate, PopSide::Top)
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(PICKER_WIDTH))
                .h(px(PICKER_HEIGHT))
                .on_mouse_down_out(
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        let on_menu =
                            this.soundboard
                                .gift_picker
                                .as_ref()
                                .is_some_and(|(picker, _)| {
                                    let (x, y) =
                                        (f32::from(event.position.x), f32::from(event.position.y));
                                    let menu_top =
                                        top + MENU_TOP + GIFT_HEADER_HEIGHT - HEADER_HEIGHT;
                                    picker.menu
                                        && (left + MENU_LEFT..left + MENU_LEFT + MENU_WIDTH)
                                            .contains(&x)
                                        && (menu_top..menu_top + MENU_HEIGHT).contains(&y)
                                });
                        if !on_menu {
                            this.soundboard_gift_close(window, cx);
                            cx.stop_propagation();
                        }
                    }),
                )
                .child(picker_box)
                .when(menu, |this| {
                    this.child(self.soundboard_menu(Some(left), cx))
                })
        } else {
            div()
                .id("soundboard-popout")
                .popout("soundboard-popout", PopKind::Fade, PopSide::Right)
                .absolute()
                .left(px(PICKER_LEFT))
                .bottom(px(PICKER_BOTTOM))
                .w(px(PICKER_WIDTH))
                .h(px(PICKER_HEIGHT))
                .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    // The volume menu hangs outside the picker's box but is part of it.
                    let on_menu = this.soundboard.picker.as_ref().is_some_and(|picker| {
                        let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
                        let top = f32::from(window.viewport_size().height)
                            - PICKER_BOTTOM
                            - PICKER_HEIGHT
                            + MENU_TOP;
                        picker.menu
                            && (PICKER_LEFT + MENU_LEFT..PICKER_LEFT + MENU_LEFT + MENU_WIDTH)
                                .contains(&x)
                            && (top..top + MENU_HEIGHT).contains(&y)
                    });
                    if !on_menu {
                        if this.vv.bar_press.get() != Some(event.position) {
                            cx.stop_propagation();
                        }
                        this.close_panel_popout(cx);
                    }
                }))
                .child(picker_box)
                .when(menu, |this| this.child(self.soundboard_menu(None, cx)))
        };
        // Under the tooltips (priority 2), which open over its buttons.
        Some(deferred(wrapper).with_priority(1).into_any_element())
    }

    /// `keybindHint_mb9pZg` at the foot of Gift Nitro's picker: a warning icon, the line with its "Edit Keybind"
    /// link and a close button (design/dom/dm/gift-step2-sound-picker-darker-cozy.json: 451x52, 8px in, icon 20px).
    /// The TIP line at the foot of the picker (module 327289, `udMTth`): "TIP: Hold {keybind} to use Soundboard in a
    /// game overlay. Edit Keybind." with the Open Soundboard (Hold) keybind; Edit Keybind opens the Soundboard
    /// Keybind modal (module 29681). Shown on the desktop while the keybind exists and `SOUNDBOARD_KEYBIND_TIP`
    /// (content 232) is not dismissed; the close button dismisses it.
    fn soundboard_gift_tip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let keybind = self.soundboard_hold_text()?;
        if self.soundboard.gift_tip_closed || self.stores.content_dismissed(KEYBIND_TIP_CONTENT) {
            return None;
        }
        let words: Vec<String> = format!("Hold {keybind} to use Soundboard in a game overlay.")
            .split(' ')
            .map(str::to_owned)
            .collect();
        Some(
            div()
                .absolute()
                .left(px(GIFT_TIP_LEFT))
                .bottom(px(GIFT_TIP_BOTTOM))
                .w(px(GIFT_TIP_WIDTH))
                .h(px(GIFT_TIP_HEIGHT))
                .px(px(8.))
                .rounded(px(8.))
                .bg(color("--background-base-lower"))
                .flex()
                .items_center()
                .child(
                    div()
                        .mr(px(8.))
                        .child(icon("ob_warning", 20., "--text-feedback-warning")),
                )
                .child(
                    // The line wraps like the capture's (391px wide): the words flow, "Edit Keybind" is the link.
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_wrap()
                        .gap_x(px(3.5))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(div().font_weight(FontWeight(700.)).child("TIP:"))
                        .children(words.into_iter().map(|word| div().child(word)))
                        .child(
                            div()
                                .flex()
                                .child(
                                    div()
                                        .id("sb-edit-keybind")
                                        .cursor_pointer()
                                        .text_color(color("--text-link"))
                                        .hover(|style| style.underline())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            // `i()` closes the picker first.
                                            if this.soundboard.gift_picker.is_some() {
                                                this.soundboard_gift_close(window, cx);
                                            } else {
                                                this.close_panel_popout(cx);
                                            }
                                            this.open_soundboard_keybind();
                                            cx.notify();
                                        }))
                                        .child(dc_intl::tr("Edit Keybind")),
                                )
                                .child("."),
                        ),
                )
                .child(
                    div()
                        .id("sb-gift-tip-close")
                        .size(px(16.))
                        .flex_none()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.soundboard.gift_tip_closed = true;
                            this.soundboard_dismiss_keybind_tip();
                            cx.notify();
                        }))
                        .child(icon("sb_close", 16., "--interactive-text-default")),
                )
                .into_any_element(),
        )
    }

    /// `Dr(SOUNDBOARD_KEYBIND_TIP)`: the content's bit in `dismissed_contents` (held back in a harness run).
    fn soundboard_dismiss_keybind_tip(&mut self) {
        if self.stores.content_dismissed(KEYBIND_TIP_CONTENT) {
            return;
        }
        let patch = self.stores.dismiss_content(KEYBIND_TIP_CONTENT);
        if patch.is_empty() || crate::storage::settings_writes_blocked() {
            return;
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: "/users/@me/settings-proto/1".into(),
            body: Some(json!({"settings": patch})),
        });
    }

    /// "Soundboard Volume": a context menu holding the slider.
    ///
    /// `gift` is the window x of the gift picker's left edge when the menu hangs from that picker, whose header is
    /// 48px taller.
    fn soundboard_menu(&self, gift: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let is_gift = gift.is_some();
        let value = super::menu_slider::amplitude_to_perceptual(
            super::settings::soundboard_volume() / 100.,
            100,
        );
        div()
            .id("soundboard-menu")
            .absolute()
            .left(px(MENU_LEFT))
            .top(px(if is_gift {
                MENU_TOP + GIFT_HEADER_HEIGHT - HEADER_HEIGHT
            } else {
                MENU_TOP
            }))
            .w(px(MENU_WIDTH))
            .h(px(MENU_HEIGHT))
            .p(px(1.))
            .rounded(px(8.))
            .bg(color("--background-surface-higher"))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.24),
                offset: point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().p(px(8.)).child(self.menu_slider_row(
                super::menu_slider::MenuSliderConfig {
                    id: if is_gift {
                        "gift-soundboard-slider".into()
                    } else {
                        "soundboard-slider".into()
                    },
                    label: dc_intl::t!("kbFsAD").to_owned(),
                    min: 0,
                    max: 100,
                    value,
                    palette: super::menu_slider::ThemePalette::Default,
                    format: super::menu_slider::percent_format(),
                    on_change: std::rc::Rc::new(|_, pct, cx| {
                        let volume = (super::menu_slider::perceptual_to_amplitude(pct, 100) * 100.)
                            .round()
                            .clamp(0., 100.);
                        super::settings::set_soundboard_volume(volume);
                        cx.notify();
                    }),
                },
                cx,
            )))
            .into_any_element()
    }

    /// The Nitro notice over the list (module 916885): a gradient border, the close, the title, the text and the button.
    fn soundboard_notice(&self, cx: &mut Context<Self>) -> AnyElement {
        let premium = self.premium_wire();
        let text = match premium {
            Some(2) => {
                "Choose a Soundboard sound to automatically play whenever you join a voice channel. Now available with Nitro."
            }
            None => {
                "Choose a Soundboard sound to automatically play whenever you join a voice channel. Only with Nitro."
            }
            _ => {
                "Upgrade your Nitro to automatically play a Soundboard sound whenever you join a voice channel."
            }
        };
        let nitro = premium == Some(2);
        let tint = |name: &str, alpha: f32| {
            let mut rgba = color(name);
            rgba.a = alpha;
            rgba
        };
        div()
            .absolute()
            .left(px(RAIL_WIDTH))
            .top(px(HEADER_HEIGHT))
            .w(px(LIST_WIDTH))
            .h(px(NOTICE_HEIGHT))
            .child(
                div()
                    .mx(px(12.))
                    .mt(px(8.))
                    .p(px(2.))
                    .h(px(138.))
                    .rounded(px(8.))
                    .bg(linear_gradient(
                        45.,
                        linear_color_stop(color("--premium-tier-2-purple-for-gradients"), 0.),
                        linear_color_stop(color("--premium-tier-2-pink-for-gradients"), 1.),
                    ))
                    .child(
                        div()
                            .relative()
                            .size_full()
                            .rounded(px(6.))
                            .bg(color("--background-base-low"))
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(6.))
                                    .bg(linear_gradient(
                                        90.,
                                        linear_color_stop(
                                            tint("--premium-tier-2-purple-for-gradients", 0.1),
                                            0.,
                                        ),
                                        linear_color_stop(
                                            tint("--premium-tier-2-pink-for-gradients", 0.1),
                                            1.,
                                        ),
                                    )),
                            )
                            .child(
                                div()
                                    .relative()
                                    .size_full()
                                    .px(px(12.))
                                    .py(px(12.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .h(px(18.))
                                            .child(icon("sb_nitro_wheel", 18., "--text-strong"))
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(18.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child("Drop into voice channels in style"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-default"))
                                            .child(text),
                                    )
                                    .child(
                                        div()
                                            .id("sb-notice-button")
                                            .h(px(40.))
                                            .rounded(px(8.))
                                            .bg(color("--green-new-50"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(gpui::rgb(0xffffff))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if nitro {
                                                    this.close_panel_popout(cx);
                                                    this.open_settings_page("voice-video", cx);
                                                } else {
                                                    cx.open_url("https://discord.com/nitro");
                                                }
                                            }))
                                            .when(!nitro, |this| {
                                                this.child(
                                                    // `premiumIcon`: a 24 by 20 box, its glyph 18.3 wide.
                                                    div()
                                                        .w(px(24.))
                                                        .h(px(20.))
                                                        .mr(px(4.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .child(icon(
                                                            "sb_nitro_button",
                                                            20.5,
                                                            "--white",
                                                        )),
                                                )
                                            })
                                            .child(if nitro {
                                                "Check it out"
                                            } else {
                                                "Get Nitro"
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("sb-notice-close")
                                    .absolute()
                                    .top(px(8.))
                                    .right(px(8.))
                                    .size(px(16.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.soundboard.upsell_dismissed = true;
                                        crate::storage::set(UPSELL_DISMISSED, json!(true));
                                        cx.notify();
                                    }))
                                    .child(icon("sb_close", 16., "--interactive-text-default")),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// A section: its sticky-looking header and its rows of three.
    #[allow(clippy::too_many_arguments)]
    fn soundboard_section(
        &self,
        section: &Section,
        top: f32,
        list_top: f32,
        scrolled: f32,
        ctx: &Ctx,
        searching: bool,
        gift: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = section.key();
        let collapsed = self.soundboard.collapsed.contains(&key);
        let toggle = key.clone();
        let icon_view: Option<AnyElement> = match &section.kind {
            Kind::Guild(guild) => {
                let picture = self.images.get(&format!("guild:{guild}")).cloned();
                Some(match picture {
                    Some(picture) => img(picture).size(px(16.)).rounded_full().into_any_element(),
                    None => div()
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.))
                        .line_height(px(12.))
                        .font_weight(FontWeight(400.))
                        .text_color(color("--text-default"))
                        .child(dc_state::Guild::acronym_of(&section.title))
                        .into_any_element(),
                })
            }
            Kind::Discord => {
                Some(icon("sb_discord", 16., "--interactive-text-default").into_any_element())
            }
            Kind::Favorites => {
                Some(icon("sb_star", 16., "--interactive-text-default").into_any_element())
            }
            Kind::Search => None,
        };
        let header = div()
            .id(SharedString::from(format!("sb-header-{key}")))
            .h(px(SECTION_HEIGHT))
            .pl(px(8.))
            .pr(px(4.))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(600.))
            .text_color(color("--interactive-text-default"))
            .hover(|style| style.text_color(color("--interactive-text-active")))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.soundboard.collapsed.remove(&toggle) {
                    this.soundboard.collapsed.insert(toggle.clone());
                }
                cx.notify();
            }))
            .children(icon_view)
            .child(div().mx(px(4.)).child(section.title.clone()))
            .when(!searching, |this| {
                this.child(
                    svg()
                        .path("icons/sb_chevron.svg")
                        .size(px(16.))
                        .flex_none()
                        .text_color(color("--interactive-text-default"))
                        .when(collapsed, |this| {
                            this.with_transformation(gpui::Transformation::rotate(gpui::radians(
                                -std::f32::consts::FRAC_PI_2,
                            )))
                        }),
                )
            });
        let mut rows = Vec::new();
        if !collapsed {
            let guild = match &section.kind {
                Kind::Guild(guild) => Some(*guild),
                _ => None,
            };
            let mut items: Vec<Option<&Sound>> = section.sounds.iter().map(Some).collect();
            if section.add.is_some() {
                items.push(None);
            }
            for (row, chunk) in items.chunks(PER_ROW).enumerate() {
                let button_bottom =
                    list_top + top + SECTION_HEIGHT + row as f32 * ROW_HEIGHT + BUTTON_HEIGHT
                        - scrolled;
                let side = if button_bottom + 50. > PICKER_HEIGHT {
                    Side::Top
                } else {
                    Side::Bottom
                };
                let buttons: Vec<AnyElement> = chunk
                    .iter()
                    .enumerate()
                    .map(|(column, item)| match item {
                        Some(sound) => self.soundboard_button(&key, sound, ctx, side, gift, cx),
                        None => self.soundboard_add_button(
                            guild,
                            section.add.unwrap_or(false),
                            column,
                            cx,
                        ),
                    })
                    .collect();
                rows.push(
                    div()
                        .h(px(ROW_HEIGHT))
                        .pl(px(8.))
                        .pb(px(8.))
                        .flex()
                        .gap(px(8.))
                        .children(buttons),
                );
            }
        }
        div()
            .flex()
            .flex_col()
            .bg(color("--background-base-low"))
            .child(header)
            .children(rows)
            .into_any_element()
    }

    /// `soundAddButton`: dashed, "Add Sound" with a plus.
    fn soundboard_add_button(
        &self,
        guild: Option<Id>,
        allowed: bool,
        column: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let button = div()
            .id(SharedString::from(format!("sb-add-{column}")))
            .w(px(BUTTON_WIDTH))
            .h(px(BUTTON_HEIGHT))
            .rounded(px(8.))
            .bg(color("--background-base-lower"))
            .border_1()
            .border_dashed()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-muted"))
            .when(allowed, |this| {
                this.cursor_pointer()
                    .hover(|style| {
                        style
                            .border_color(color("--border-normal"))
                            .text_color(color("--text-default"))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(guild) = guild {
                            this.soundboard_add(guild, true, window, cx);
                        }
                    }))
            })
            .when(!allowed, |this| this.opacity(0.5))
            .child(div().mr(px(3.)).child(icon("sb_plus", 18., "--text-muted")))
            .child("Add Sound");
        if allowed {
            button.into_any_element()
        } else {
            self.with_tip(
                format!("sb-add-tip-{column}"),
                Tip::body(
                    Body::Text(
                        "You don't have permissions to upload sounds to this server.".into(),
                    ),
                    Side::Top,
                ),
                button,
                cx,
            )
            .into_any_element()
        }
    }

    /// `soundButton`: the emoji and the name; over it the preview speaker, the play mark and the star.
    /// `canUseCustomCallSounds` (Nitro).
    fn custom_call_sounds(&self) -> bool {
        dc_state::premium::PremiumType::from_wire(self.stores.account.premium_type)
            .has(dc_state::premium::Feature::CustomCallSounds)
    }

    /// A right press on a sound (`v` of module 496502): `sound-button-context` opens when the sound's server is known
    /// or the account can use custom call sounds; `call_guild` is the call's server (`activeCallGuildId`).
    pub(super) fn open_sound_menu(
        &mut self,
        sound: &Sound,
        call_guild: Option<Id>,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let known = sound
            .guild
            .parse::<Id>()
            .ok()
            .is_some_and(|guild| self.stores.guild(guild).is_some());
        if !known && !self.custom_call_sounds() {
            return;
        }
        self.context.picker_sound = Some((sound.clone(), call_guild));
        self.open_context_menu(super::context_menu::Target::PickerSound, at, cx);
    }

    /// The facts of module 923819's rows for `sound`.
    pub(super) fn sound_facts(
        &self,
        sound: &Sound,
        call_guild: Option<Id>,
        developer: bool,
    ) -> super::context_menu::SoundFacts {
        let own = sound
            .guild
            .parse::<Id>()
            .ok()
            .and_then(|guild| self.stores.guild(guild));
        super::context_menu::SoundFacts {
            can_manage: own.zip(self.stores.me()).is_some_and(|(guild, me)| {
                permissions::can_in_guild(guild, me, permissions::MANAGE_GUILD_EXPRESSIONS)
            }),
            entrance: call_guild.is_some() && self.custom_call_sounds(),
            default_sound: sound.guild == DISCORD,
            developer,
        }
    }

    /// Set as Server / Global Entrance Sound (`un` of module 458976): the guild's `joinSound` in the settings.
    pub(super) fn set_entrance_sound(&mut self, guild: u64, sound: &Sound) {
        let (Ok(id), sound_guild) = (
            sound.id.parse::<u64>(),
            sound.guild.parse::<u64>().unwrap_or(0),
        ) else {
            return;
        };
        let patch = self.stores.set_join_sound(guild, id, sound_guild);
        tracing::info!(guild, "soundboard: entrance sound set");
        if crate::storage::settings_writes_blocked() {
            crate::inert::note("held entrance sound settings write");
            return;
        }
        self.send_proto_patch(patch);
    }

    /// Download Sound (`E` of module 923819): the file is fetched, its type names the extension (`audio/mpeg` mp3,
    /// `audio/ogg` ogg, anything else fails), and a save dialog takes `{id}.{ext}`; any failure shows Sound Download
    /// Failed. The fetch and the write run off the UI thread.
    pub(super) fn download_sound(&mut self, sound: &Sound, cx: &mut Context<Self>) {
        if sound.id.is_empty() || !sound.id.bytes().all(|byte| byte.is_ascii_digit()) {
            return;
        }
        let url = format!("{CDN}{}", sound.id);
        let id = sound.id.clone();
        let (sender, fetched) = tokio::sync::oneshot::channel();
        let spawned = std::thread::Builder::new()
            .name("gravecord-sound-download".into())
            .spawn(move || {
                let result = crate::sounds::fetch_url_typed(&url, DOWNLOAD_MAX_BYTES)
                    .ok()
                    .and_then(|(bytes, kind)| Some((bytes, sound_extension(&kind)?)));
                let _ = sender.send(result);
            });
        if spawned.is_err() {
            self.sound_download_failed(cx);
            return;
        }
        let inert = crate::inert::on();
        cx.spawn(async move |this, cx| {
            let Ok(Some((bytes, extension))) = fetched.await else {
                let _ = this.update(cx, |this, cx| this.sound_download_failed(cx));
                return;
            };
            if inert {
                // A harness run opens no save dialog and writes no file.
                tracing::info!(
                    extension,
                    size = bytes.len(),
                    "soundboard: Download Sound fetched (inert: no save dialog)"
                );
                return;
            }
            let directory = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            let name = format!("{id}.{extension}");
            let Ok(chosen) =
                this.update(cx, |_, cx| cx.prompt_for_new_path(&directory, Some(&name)))
            else {
                return;
            };
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            let written = cx
                .background_executor()
                .spawn(async move { std::fs::write(path, bytes).is_ok() })
                .await;
            if !written {
                let _ = this.update(cx, |this, cx| this.sound_download_failed(cx));
            }
        })
        .detach();
    }

    /// Sound Download Failed (module 157559 `show`): the title, the apology and Okay.
    fn sound_download_failed(&mut self, cx: &mut Context<Self>) {
        self.chat_extra.alert = Some(super::chat_notices::Alert::show(
            "sound-download-failed",
            dc_intl::t("mK3tDH"),
            Some(dc_intl::t("jLlfDN").to_owned()),
            None,
            Some(dc_intl::t("BddRzS").to_owned()),
            super::chat_notices::Then::Close,
        ));
        cx.notify();
    }

    fn soundboard_button(
        &self,
        section: &str,
        sound: &Sound,
        ctx: &Ctx,
        side: Side,
        gift: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = access(sound, ctx);
        let open = state == Access::Open;
        let key = format!("{section}-{}", sound.id);
        let group = SharedString::from(format!("sb-group-{key}"));
        let playing = self.soundboard.playing.contains_key(&sound.id);
        let favorite = self.soundboard.favorites.contains(&sound.id);
        let picture = sound
            .emoji_id
            .as_ref()
            .and_then(|id| self.images.get(&format!("custom:{id}")))
            .or_else(|| {
                sound
                    .emoji_name
                    .as_ref()
                    .and_then(|name| self.images.get(&format!("emoji:{name}")))
            })
            .cloned();
        let (pressed, previewed, starred) = (sound.clone(), sound.clone(), sound.clone());
        let menu_sound = sound.clone();
        let labels = sound_labels(sound.emoji_name.as_deref(), &sound.name, favorite);
        let secondary = |id: String| {
            div()
                .id(SharedString::from(id))
                .size(px(16.))
                .flex_none()
                .cursor_pointer()
        };
        let overlay = div()
            .absolute()
            .inset_0()
            .rounded(px(7.))
            .opacity(0.)
            .group_hover(group.clone(), |style| style.opacity(1.))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(7.))
                    .opacity(0.8)
                    .bg(color("--background-surface-highest")),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        secondary(format!("sb-preview-{key}"))
                            .aria_label(labels.preview.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.soundboard_preview(&previewed);
                            }))
                            .child(icon("sb_speaker", 16., "--interactive-text-default")),
                    )
                    .child(div().flex_1().flex().justify_center().map(|this| {
                        if gift {
                            // `soundButtonOverlay: ADD`: a plus and "Add" (`addButton_N99eHz`).
                            this.items_center()
                                .gap(px(4.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(icon("sb_plus", 18., "--text-strong"))
                                .child("Add")
                        } else {
                            this.child(icon("sb_play", 20., "--text-strong"))
                        }
                    }))
                    .child(
                        secondary(format!("sb-star-{key}"))
                            .aria_label(labels.favorite.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.soundboard.toggle_favorite(&starred.id);
                                cx.notify();
                            }))
                            .child(icon(
                                "sb_star",
                                16.,
                                if favorite {
                                    "--text-feedback-warning"
                                } else {
                                    "--interactive-text-default"
                                },
                            )),
                    ),
            );
        let body = div()
            .id(SharedString::from(format!("sb-button-{key}")))
            .aria_label(labels.play)
            .group(group)
            .relative()
            .w(px(BUTTON_WIDTH))
            .h(px(BUTTON_HEIGHT))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .border_1()
            .border_color(if playing {
                color("--brand-500")
            } else {
                gpui::rgba(0)
            })
            .when(!open, |this| this.opacity(0.5))
            .when(open, |this| {
                this.cursor_pointer()
                    .hover(|style| {
                        if playing {
                            style
                        } else {
                            style.border_color(color("--border-subtle"))
                        }
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if gift {
                            // `suppressPlaySound`: a press picks the sound for the gift and plays nothing.
                            this.soundboard_gift_choose(&pressed, window, cx);
                        } else {
                            this.soundboard_click(&pressed, cx);
                        }
                    }))
                    // `onContextMenu: T && !eC` (module 533491, `enableSecondaryActions` from 327289): a usable
                    // sound's `sound-button-context` for the call's server.
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                            // A DM call has no server (`h?.guild_id` is null).
                            let call_guild = this
                                .call
                                .as_ref()
                                .map(|call| call.guild)
                                .filter(|guild| *guild != 0);
                            this.open_sound_menu(&menu_sound, call_guild, event.position, cx);
                            cx.stop_propagation();
                        }),
                    )
            })
            .child(
                div()
                    .size_full()
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(20.))
                            .flex_none()
                            .children(picture.map(|picture| img(picture).size(px(20.)))),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(sound.name.clone()),
                    ),
            )
            .when(open, |this| this.child(overlay))
            .when(state == Access::Locked, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("channel_lock", 20., "--interactive-text-default")),
                )
            });
        let wrapper = div()
            .rounded(px(8.))
            .bg(color("--background-base-lower"))
            .child(body);
        let tip = if state == Access::Unavailable {
            "Requires higher Server Boost Level".to_owned()
        } else {
            sound.name.clone()
        };
        self.with_tip(
            format!("sb-tip-{key}"),
            Tip::body(Body::Text(tip.into()), side),
            wrapper,
            cx,
        )
        .into_any_element()
    }
}

/// The accessible names of a sound button and its two small buttons (module 533491): "Play", "Preview" and
/// "Favorite" or "Unfavorite", each with the sound's emoji and name.
pub(super) struct SoundLabels {
    pub play: SharedString,
    pub preview: SharedString,
    pub favorite: SharedString,
}

pub(super) fn sound_labels(emoji: Option<&str>, name: &str, favorite: bool) -> SoundLabels {
    let text = |key: &str| -> SharedString {
        dc_intl::format(
            key,
            &[
                ("emojiName", emoji.unwrap_or_default().to_owned().into()),
                ("soundName", name.to_owned().into()),
            ],
        )
        .into()
    };
    SoundLabels {
        play: text("tuMUJ2"),
        preview: text("/8fYO5"),
        favorite: text(if favorite { "lQLsjc" } else { "Y5DOs4" }),
    }
}

fn search_section(ctx: &Ctx, query: &str, guilds: &[(Id, String)], board: &Board) -> Section {
    search(ctx, query, guilds, &board.guilds, &board.defaults)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_downloaded_sound_is_named_by_its_type() {
        assert_eq!(sound_extension("audio/mpeg"), Some("mp3"));
        assert_eq!(sound_extension("audio/mpeg3"), Some("mp3"));
        assert_eq!(sound_extension("audio/ogg"), Some("ogg"));
        assert_eq!(sound_extension("audio/wav"), None);
        assert_eq!(sound_extension(""), None);
    }

    #[test]
    fn the_sound_menu_rows_follow_module_923819() {
        use super::super::context_menu::{Act, Entry, SoundFacts, sound_items};
        let acts = |facts: SoundFacts| -> Vec<Act> {
            sound_items(facts)
                .into_iter()
                .filter_map(|entry| match entry {
                    Entry::Row(row) => Some(row.act),
                    _ => None,
                })
                .collect()
        };
        // Someone else's server sound, no Nitro, no developer mode: Download Sound alone.
        assert_eq!(acts(SoundFacts::default()), [Act::DownloadSound]);
        // A default sound has no Download Sound; without anything else the menu is empty.
        let default_sound = SoundFacts {
            default_sound: true,
            ..SoundFacts::default()
        };
        assert!(acts(default_sound).is_empty());
        // Everything, in the bundle's order.
        let all = SoundFacts {
            can_manage: true,
            entrance: true,
            default_sound: false,
            developer: true,
        };
        assert_eq!(
            acts(all),
            [
                Act::EditSound,
                Act::EntranceServer,
                Act::EntranceGlobal,
                Act::DownloadSound,
                Act::CopyId
            ]
        );
    }

    #[test]
    fn sound_buttons_are_named_with_the_emoji_and_the_sound() {
        let labels = sound_labels(Some("🦆"), "quack", false);
        assert_eq!(labels.play.as_ref(), "Play 🦆 quack");
        assert_eq!(labels.preview.as_ref(), "Preview 🦆 quack");
        assert_eq!(labels.favorite.as_ref(), "Favorite 🦆 quack");
        assert_eq!(
            sound_labels(Some("🦆"), "quack", true).favorite.as_ref(),
            "Unfavorite 🦆 quack"
        );
    }

    fn sound(id: &str, guild: &str, name: &str) -> Sound {
        Sound {
            id: id.to_owned(),
            guild: guild.to_owned(),
            name: name.to_owned(),
            emoji_id: None,
            emoji_name: Some("\u{1f986}".to_owned()),
            volume: 1.,
            available: true,
        }
    }

    #[test]
    fn a_sound_parses_from_the_gateway_and_the_default_list() {
        let parsed = Sound::from_value(
            &json!({"sound_id": "12", "name": "quack", "emoji_id": null, "emoji_name": "\u{1f986}",
                    "volume": 0.5, "available": true, "guild_id": "7"}),
            DISCORD,
        )
        .unwrap();
        assert_eq!(parsed.guild, "7");
        assert_eq!(parsed.emoji_id, None);
        assert_eq!(parsed.volume, 0.5);
        let defaults = Sound::list(
            &json!([{"sound_id": "2", "name": "airhorn", "volume": 1.0},
                    {"sound_id": "1", "name": "quack", "volume": 1.0}, {"name": "no id"}]),
            DISCORD,
        );
        assert_eq!(
            defaults.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["quack", "airhorn"]
        );
        assert!(
            defaults
                .iter()
                .all(|sound| sound.guild == DISCORD && sound.available)
        );
    }

    #[test]
    fn unavailable_sounds_sort_after_the_available_ones() {
        let mut sounds = vec![
            sound("3", "1", "c"),
            sound("1", "1", "a"),
            sound("2", "1", "b"),
        ];
        sounds[1].available = false;
        sort(&mut sounds);
        assert_eq!(
            sounds.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["2", "3", "1"]
        );
    }

    #[test]
    fn the_send_body_names_the_source_guild_except_for_the_defaults() {
        let own = sound("5", "99", "mine");
        assert_eq!(
            own.send_body(),
            json!({"sound_id": "5", "emoji_id": null, "emoji_name": "\u{1f986}", "source_guild_id": "99"})
        );
        let default = sound("1", DISCORD, "quack");
        let body = default.send_body();
        assert!(body.get("source_guild_id").is_none());
        assert_eq!(body["sound_id"], "1");
    }

    #[test]
    fn access_follows_nitro_external_sounds_and_availability() {
        let mine = sound("1", "10", "a");
        let other = sound("2", "20", "b");
        let default = sound("3", DISCORD, "c");
        let plain = Ctx {
            current: Some(10),
            ..Ctx::default()
        };
        assert_eq!(access(&mine, &plain), Access::Open);
        assert_eq!(access(&default, &plain), Access::Open);
        assert_eq!(access(&other, &plain), Access::Locked);
        let nitro = Ctx {
            everywhere: true,
            external: true,
            ..plain.clone()
        };
        assert_eq!(access(&other, &nitro), Access::Open);
        let no_external = Ctx {
            everywhere: true,
            ..plain.clone()
        };
        assert_eq!(access(&other, &no_external), Access::Locked);
        let mut gone = mine.clone();
        gone.available = false;
        assert_eq!(access(&gone, &plain), Access::Unavailable);
    }

    #[test]
    fn sections_put_the_call_server_first_and_nitro_moves_discord_last() {
        let guilds = vec![
            (10, "Mine".to_owned()),
            (20, "Other".to_owned()),
            (30, "Empty".to_owned()),
        ];
        let mut sounds = HashMap::new();
        sounds.insert(20, vec![sound("2", "20", "b")]);
        let defaults = vec![sound("1", DISCORD, "quack")];
        let ctx = Ctx {
            current: Some(10),
            external: true,
            can_create: true,
            slots: 8,
            ..Ctx::default()
        };
        let kinds =
            |sections: &[Section]| sections.iter().map(|s| s.kind.clone()).collect::<Vec<_>>();
        let plain = sections(&ctx, &guilds, &sounds, &defaults, &[]);
        assert_eq!(
            kinds(&plain),
            [Kind::Guild(10), Kind::Discord, Kind::Guild(20)]
        );
        assert_eq!(plain[0].add, Some(true));
        let nitro = Ctx {
            everywhere: true,
            ..ctx.clone()
        };
        assert_eq!(
            kinds(&sections(&nitro, &guilds, &sounds, &defaults, &[])),
            [Kind::Guild(10), Kind::Guild(20), Kind::Discord]
        );
        let without_external = Ctx {
            external: false,
            can_create: false,
            ..ctx
        };
        let hidden = sections(
            &without_external,
            &guilds,
            &sounds,
            &defaults,
            &["1".to_owned()],
        );
        assert_eq!(
            kinds(&hidden),
            [Kind::Favorites, Kind::Guild(10), Kind::Discord]
        );
        // An empty server still offers Add Sound, disabled without the right to upload.
        assert_eq!(hidden[1].add, Some(false));
    }

    #[test]
    fn add_sound_goes_when_the_server_is_full() {
        let guilds = vec![(10, "Mine".to_owned())];
        let mut sounds = HashMap::new();
        sounds.insert(
            10,
            (1..=8).map(|n| sound(&n.to_string(), "10", "x")).collect(),
        );
        let ctx = Ctx {
            current: Some(10),
            can_create: true,
            slots: 8,
            ..Ctx::default()
        };
        assert_eq!(sections(&ctx, &guilds, &sounds, &[], &[])[0].add, None);
        assert_eq!(slots(&[]), 8);
        assert_eq!(slots(&["MORE_SOUNDBOARD".to_owned()]), 200);
    }

    #[test]
    fn section_geometry_matches_the_capture() {
        // Header 32, the Add Sound row 48; Discord Sounds: 32 and two rows of 48.
        let own = Section {
            kind: Kind::Guild(1),
            title: "gravecord fixture".to_owned(),
            sounds: Vec::new(),
            add: Some(true),
        };
        let defaults = Section {
            kind: Kind::Discord,
            title: "Discord Sounds".to_owned(),
            sounds: (0..6)
                .map(|n| sound(&n.to_string(), DISCORD, "x"))
                .collect(),
            add: None,
        };
        let sections = [own, defaults];
        let mut collapsed = HashSet::new();
        assert_eq!(tops(&sections, &collapsed), [0., 80.]);
        collapsed.insert("guild:1".to_owned());
        assert_eq!(tops(&sections, &collapsed), [0., 32.]);
        assert_eq!(active_section(&[0., 80., 200.], 0.), 0);
        assert_eq!(active_section(&[0., 80., 200.], 61.), 1);
        assert_eq!(active_section(&[0., 80., 200.], 500.), 2);
    }

    #[test]
    fn search_scores_name_before_emoji_and_prefers_usable_sounds() {
        let quack = sound("1", DISCORD, "quack");
        assert_eq!(score(&quack, "QUACK", false), 8);
        assert_eq!(score(&quack, "qua", false), 6);
        assert_eq!(score(&quack, "ack", false), 4);
        assert_eq!(score(&quack, "qk", false), 2);
        assert_eq!(score(&quack, "zzz", true), 0);
        assert_eq!(score(&quack, "quack", true), 108);
        let mut emoji = sound("2", DISCORD, "x");
        emoji.emoji_name = Some("duck".to_owned());
        assert_eq!(score(&emoji, "duck", false), 7);
        assert_eq!(score(&emoji, "du", false), 5);
        assert_eq!(score(&emoji, "dk", false), 1);
    }

    #[test]
    fn search_filters_and_ranks_across_every_server() {
        let guilds = vec![(10, "Mine".to_owned()), (20, "Other".to_owned())];
        let mut sounds = HashMap::new();
        sounds.insert(20, vec![sound("2", "20", "quacker")]);
        let defaults = vec![sound("1", DISCORD, "quack"), sound("3", DISCORD, "airhorn")];
        let ctx = Ctx {
            current: Some(10),
            ..Ctx::default()
        };
        let found = search(&ctx, "quack", &guilds, &sounds, &defaults);
        assert_eq!(found.kind, Kind::Search);
        // The exact match, then the one locked sound with a lower score.
        assert_eq!(
            found
                .sounds
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["quack", "quacker"]
        );
        assert!(
            search(&ctx, "nothing here", &guilds, &sounds, &defaults)
                .sounds
                .is_empty()
        );
    }

    #[test]
    fn the_volume_is_the_official_product_and_the_engine_lands_on_it() {
        // Unity everywhere is unity.
        assert_eq!(playback_volume(1., 100., 1.), 1.);
        // The slider curve: a stored 50 reads as the position 50^(1/2.8).
        let position = volume_to_slider(50.) / 100.;
        assert!((playback_volume(1., 50., 1.) - position).abs() < 1e-6);
        assert!(position > 0.5);
        // The sound's own volume and the output volume (capped at 1) multiply in.
        assert!((playback_volume(0.5, 100., 1.) - 0.5).abs() < 1e-6);
        assert!((playback_volume(1., 100., 0.25) - 0.25).abs() < 1e-6);
        assert_eq!(playback_volume(1., 100., 2.), 1.);
        assert_eq!(playback_volume(1., 0., 1.), 0.);
        // `sound::play` applies the output volume once more: the factor cancels it.
        for gain in [0.25_f32, 1., 1.5] {
            let official = playback_volume(0.8, 70., gain);
            let played = sound::gain(gain, engine_factor(official, gain));
            assert!((played - official).abs() < 1e-6, "{gain}");
        }
        assert_eq!(engine_factor(0.5, 0.), 0.);
    }

    #[test]
    fn the_slider_position_round_trips_through_the_stored_volume() {
        for position in [0., 50., 90., 100.] {
            let stored = super::super::audio_menu::slider_to_volume(position).round();
            assert!(
                (volume_to_slider(stored) - position).abs() < 1.5,
                "{position}"
            );
        }
    }

    #[test]
    fn an_effect_plays_only_in_our_channel_from_someone_else_while_we_hear() {
        let effect = Effect::from_value(&json!({
            "sound_id": "1", "sound_volume": 0.5, "user_id": "7", "channel_id": "100",
        }));
        assert_eq!(effect.volume, 0.5);
        let none = HashSet::new();
        assert!(heard(&effect, Some(100), Some(1), false, &none));
        // Our own echo.
        assert!(!heard(&effect, Some(100), Some(7), false, &none));
        // Another channel, or no call.
        assert!(!heard(&effect, Some(101), Some(1), false, &none));
        assert!(!heard(&effect, None, Some(1), false, &none));
        // Deafened.
        assert!(!heard(&effect, Some(100), Some(1), true, &none));
        // Muted for soundboard.
        assert!(!heard(
            &effect,
            Some(100),
            Some(1),
            false,
            &HashSet::from([7])
        ));
        // An emoji effect has no sound.
        let emoji = Effect::from_value(&json!({"user_id": "7", "channel_id": "100"}));
        assert!(!heard(&emoji, Some(100), Some(1), false, &none));
    }

    #[test]
    fn the_button_names_why_it_is_dead_in_the_official_order() {
        assert_eq!(disabled_reason(false, false, false), None);
        assert_eq!(
            disabled_reason(true, true, true),
            Some("Soundboard disabled when server muted")
        );
        assert_eq!(
            disabled_reason(false, true, true),
            Some("Soundboard disabled when suppressed")
        );
        assert_eq!(
            disabled_reason(false, false, true),
            Some("Soundboard disabled when deafened")
        );
    }

    #[test]
    fn the_board_keeps_up_with_create_update_and_delete() {
        let mut board = Board::default();
        board.replace(10, vec![sound("2", "10", "b")]);
        board.upsert(10, sound("1", "10", "a"));
        let mut renamed = sound("2", "10", "renamed");
        renamed.volume = 0.2;
        board.upsert(10, renamed);
        let names: Vec<&str> = board.guilds[&10].iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["a", "renamed"]);
        board.remove(10, "1");
        assert_eq!(board.guilds[&10].len(), 1);
    }

    #[test]
    fn playing_counts_by_sound_and_by_user() {
        let mut board = Board::default();
        board.started("1", Some(7));
        board.started("1", Some(7));
        assert!(board.user_playing(7));
        board.ended("1", Some(7));
        assert!(board.user_playing(7) && board.playing.contains_key("1"));
        board.ended("1", Some(7));
        assert!(!board.user_playing(7) && board.playing.is_empty());
        board.ended("1", Some(7));
    }

    #[test]
    fn an_account_older_than_thirty_days_is_told_by_its_snowflake() {
        // 2020-01-01 as a snowflake.
        let snowflake = (1_577_836_800_000_u64 - 1_420_070_400_000) << 22;
        assert_eq!(created_ms(snowflake), 1_577_836_800_000);
    }
}
