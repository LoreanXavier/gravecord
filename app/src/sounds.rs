//! UI sounds, ported from the bundle: `Ak` (module 400492) plays one, the soundpack tables are module 458640, the
//! soundpack choice and the holiday event gate are modules 312671, 289739, 552122 and 382633, and the per-sound switches
//! are `NotificationSettingsStore` (803224).
//!
//! `Ak(sound, volume = 1, onDone, pack, { outputChannel })`: the pack's table names the file (`table[sound] ?? sound`),
//! the file plays at `min(outputVolume / 100 * volume, 1)` on the system default speaker (`DEFAULT`) or the speaker
//! chosen in Voice & Video (`VOICE`, used only for the sound heard when a device is picked). `Ak` itself never checks
//! the per-sound switches; the hooks that play events do (`useSound`, module 596339: `isSoundDisabled(sound)` first,
//! volume 0.4), and the Preview Sound links do not.
//!
//! The files are not bundled: each is fetched once from `https://discord.com/assets/<file>`, kept under
//! `%LOCALAPPDATA%\gravecord\sounds` and decoded in memory. Capture runs play nothing and fetch nothing; they only keep a
//! trace of what would have played, for the harness.

mod table;

use crate::storage;
use dc_state::experiments::Assignments;
use dc_voice::sound::{Decoded, Handle, Output};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};
use tracing::{debug, warn};

/// Where the build serves its assets.
const ASSET_ROOT: &str = "https://discord.com/assets/";
/// The volume `useSound` plays event sounds at.
pub const EVENT_VOLUME: f32 = 0.4;

/// `Soundpack` (module 235079).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pack {
    Classic,
    Retro,
    Bubble,
    Ducky,
    Lofi,
    Asmr,
    Discodo,
    Halloween,
    WinterHoliday,
}

impl Pack {
    const ALL: [Pack; 9] = [
        Pack::Classic,
        Pack::Retro,
        Pack::Bubble,
        Pack::Ducky,
        Pack::Lofi,
        Pack::Asmr,
        Pack::Discodo,
        Pack::Halloween,
        Pack::WinterHoliday,
    ];

    /// The value the store keeps.
    pub fn id(self) -> &'static str {
        match self {
            Pack::Classic => "classic",
            Pack::Retro => "retro",
            Pack::Bubble => "bop",
            Pack::Ducky => "ducky",
            Pack::Lofi => "lofi",
            Pack::Asmr => "asmr",
            Pack::Discodo => "discodo",
            Pack::Halloween => "halloween",
            Pack::WinterHoliday => "winter_holiday",
        }
    }

    fn from_id(id: &str) -> Option<Pack> {
        Pack::ALL.into_iter().find(|pack| pack.id() == id)
    }

    /// Module 458640's table for the pack. Classic names every sound by itself, which is also what a missing entry
    /// falls back to, so it needs no table.
    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Pack::Classic => &[],
            Pack::Retro => &[("message1", "bit_message1")],
            Pack::Bubble => &[("message1", "bop_message1")],
            Pack::Ducky => &[("message1", "ducky_message1")],
            Pack::Lofi => &[("message1", "lofi_message1")],
            Pack::Asmr => &[("message1", "asmr_message1")],
            Pack::Discodo => &[("message1", "discodo")],
            Pack::Halloween => &[
                ("call_calling", "halloween_call_calling"),
                ("call_ringing", "halloween_call_ringing"),
                ("message1", "halloween_message1"),
                ("deafen", "halloween_deafen"),
                ("undeafen", "halloween_undeafen"),
                ("mute", "halloween_mute"),
                ("unmute", "halloween_unmute"),
                ("disconnect", "halloween_disconnect"),
                ("user_join", "halloween_user_join"),
                ("user_leave", "halloween_user_leave"),
            ],
            Pack::WinterHoliday => &[
                ("call_calling", "winter_call_calling"),
                ("call_ringing", "winter_call_ringing"),
                ("deafen", "winter_deafen"),
                ("undeafen", "winter_undeafen"),
                ("mute", "winter_mute"),
                ("unmute", "winter_unmute"),
                ("disconnect", "winter_disconnect"),
                ("user_join", "winter_user_join"),
                ("user_leave", "winter_user_leave"),
            ],
        }
    }
}

/// `u[e] ?? e`: the sound name the pack plays for `sound`.
pub fn pack_sound(pack: Pack, sound: &str) -> &str {
    pack.table()
        .iter()
        .find(|(name, _)| *name == sound)
        .map_or(sound, |(_, mapped)| mapped)
}

/// Module 696354: the asset file of a sound name, if the build has one.
pub fn asset_file(name: &str) -> Option<&'static str> {
    table::FILES
        .binary_search_by_key(&name, |(known, _)| known)
        .ok()
        .map(|at| table::FILES[at].1)
}

// ----- the holiday event (module 382633) and the soundpack store (312671, 289739) -----

/// A holiday event: a time window and a server experiment, as `382633` declares them.
pub struct Event {
    /// The user experiment; variation 1 turns it on, anything else (or no assignment) leaves it off.
    pub experiment: &'static str,
    pub start_ms: i64,
    pub end_ms: i64,
    pub soundpack: Pack,
}

/// `2026-09-halloween-holiday`: 17913888e5 to 17936388e5 ms, soundpack `HALLOWEEN`.
pub const HALLOWEEN_2026: Event = Event {
    experiment: "2026-09-halloween-holiday",
    start_ms: 1_791_388_800_000,
    end_ms: 1_793_638_800_000,
    soundpack: Pack::Halloween,
};

impl Event {
    /// `552122` `isEligible`: the experiment is on for this account and now is inside the window.
    pub fn is_eligible(&self, now_ms: i64, assignments: &Assignments) -> bool {
        assignments.variant(self.experiment) == Some(1)
            && now_ms >= self.start_ms
            && now_ms <= self.end_ms
    }
}

/// `SoundpackStore`'s state.
#[derive(Clone, Debug, PartialEq)]
struct SoundpackState {
    soundpack: Pack,
    last_experiment: Option<String>,
}

const SOUNDPACK_KEY: &str = "SoundpackStore";

impl SoundpackState {
    fn load() -> Self {
        let value = storage::get(SOUNDPACK_KEY).unwrap_or(Value::Null);
        Self {
            // `initialize`: an unknown pack becomes classic.
            soundpack: value["soundpack"]
                .as_str()
                .and_then(Pack::from_id)
                .unwrap_or(Pack::Classic),
            last_experiment: value["lastSoundpackExperimentId"]
                .as_str()
                .map(str::to_owned),
        }
    }

    fn save(&self) {
        storage::set(
            SOUNDPACK_KEY,
            json!({
                "soundpack": self.soundpack.id(),
                "lastSoundpackExperimentId": self.last_experiment,
            }),
        );
    }
}

/// `getSoundpack()`.
pub fn current_pack() -> Pack {
    SoundpackState::load().soundpack
}

/// What `289739` `updateSoundpack` does with the current pack: `Some((pack, forExperimentId))` is the `SET_SOUNDPACK` it
/// dispatches. An eligible event whose experiment has not set the pack yet sets its pack; with no event and a pack other
/// than classic, the pack goes back to classic.
fn soundpack_update(
    current: &SoundpackState,
    event: &Event,
    eligible: bool,
) -> Option<(Pack, Option<&'static str>)> {
    if eligible || current.soundpack == Pack::Classic {
        (eligible
            && current.last_experiment.as_deref() != Some(event.experiment)
            && current.soundpack != event.soundpack)
            .then_some((event.soundpack, Some(event.experiment)))
    } else {
        Some((Pack::Classic, None))
    }
}

/// The wall clock in milliseconds since the epoch (`Date.now()`).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// The step the bundle takes when the experiment store changes (READY brings the assignments). `now_ms` is the clock.
pub fn refresh_soundpack(now_ms: i64, assignments: &Assignments) {
    let current = SoundpackState::load();
    let eligible = HALLOWEEN_2026.is_eligible(now_ms, assignments);
    if let Some((soundpack, experiment)) = soundpack_update(&current, &HALLOWEEN_2026, eligible) {
        debug!(pack = soundpack.id(), "soundpack changed");
        SoundpackState {
            soundpack,
            // `forExperimentId: undefined` keeps the last one; null (back to classic) clears it.
            last_experiment: experiment.map(str::to_owned),
        }
        .save();
    }
}

// ----- the per-sound switches (NotificationSettingsStore) -----

const NOTIFICATIONS_KEY: &str = "notifications";

/// `disabledSounds` and `disableAllSounds` from the `notifications` store.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Switches {
    pub disabled: Vec<String>,
    pub all: bool,
}

impl Switches {
    pub fn load() -> Self {
        let value = storage::get(NOTIFICATIONS_KEY).unwrap_or(Value::Null);
        Self {
            disabled: value["disabledSounds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|sound| sound.as_str().map(str::to_owned))
                .collect(),
            all: value["disableAllSounds"].as_bool().unwrap_or(false),
        }
    }

    /// Writes only these two fields; the rest of the store stays as it was.
    fn save(&self) {
        let mut value = storage::get(NOTIFICATIONS_KEY)
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        value["disabledSounds"] = json!(self.disabled);
        value["disableAllSounds"] = json!(self.all);
        storage::set(NOTIFICATIONS_KEY, value);
    }

    /// `isSoundDisabled`.
    pub fn is_disabled(&self, sound: &str) -> bool {
        self.all || self.disabled.iter().any(|known| known == sound)
    }

    /// The switch as the list shows it (`useValue`: not disabled, and not everything off).
    pub fn is_on(&self, sound: &str) -> bool {
        !self.is_disabled(sound)
    }

    /// The list item's `setValue`: the sound leaves `disabledSounds`, and goes back in when turned off.
    pub fn set_on(&mut self, sound: &str, on: bool) {
        self.disabled.retain(|known| known != sound);
        if !on {
            self.disabled.push(sound.to_owned());
        }
    }
}

/// `isSoundDisabled(sound)`.
pub fn is_sound_disabled(sound: &str) -> bool {
    Switches::load().is_disabled(sound)
}

// ----- the settings rows over those switches -----

const LIST_PREFIX: &str = "sounds_list_item_";
/// `SELECTED_CHANNEL_NOTIFICATIONS` and `DISABLE_ALL_NOTIFICATION_SOUNDS` (module 780964).
pub const SELECTED_CHANNEL: &str = "selected_channel_notifications";
pub const DISABLE_ALL: &str = "disable_all_notification_sounds";
const NOTIFY_SELECTED_KEY: &str = "notifyMessagesInSelectedChannel";

/// The sound a settings row previews: a list item's own, or `message3` for the selected channel row (`aZ("message3")`).
pub fn row_sound(key: &str) -> Option<&str> {
    if key == SELECTED_CHANNEL {
        Some("message3")
    } else {
        key.strip_prefix(LIST_PREFIX)
    }
}

/// What a row's switch shows. A list item is on while its sound is not disabled; the selected channel row also needs
/// the notify flag; Disable All is on when everything is off.
fn read_switch(switches: &Switches, notify_selected: bool, key: &str) -> Option<bool> {
    match key {
        SELECTED_CHANNEL => Some(notify_selected && !switches.all),
        DISABLE_ALL => Some(switches.all),
        _ => key
            .strip_prefix(LIST_PREFIX)
            .map(|sound| switches.is_on(sound)),
    }
}

/// A row's `setValue`. False for a key that is not one of the rows.
fn write_switch(switches: &mut Switches, notify_selected: &mut bool, key: &str, on: bool) -> bool {
    match key {
        SELECTED_CHANNEL => *notify_selected = on,
        DISABLE_ALL => switches.all = on,
        _ => match key.strip_prefix(LIST_PREFIX) {
            Some(sound) => switches.set_on(sound, on),
            None => return false,
        },
    }
    true
}

fn notify_selected() -> bool {
    storage::get(NOTIFICATIONS_KEY)
        .and_then(|value| value[NOTIFY_SELECTED_KEY].as_bool())
        .unwrap_or(false)
}

/// The value a sound row of the settings pages shows; none for other keys.
pub fn switch_value(key: &str) -> Option<bool> {
    read_switch(&Switches::load(), notify_selected(), key)
}

/// A sound row's switch was flipped: store it. False for other keys.
pub fn set_switch(key: &str, on: bool) -> bool {
    let mut switches = Switches::load();
    let mut notify = notify_selected();
    if !write_switch(&mut switches, &mut notify, key, on) {
        return false;
    }
    switches.save();
    let mut value = storage::get(NOTIFICATIONS_KEY)
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    value[NOTIFY_SELECTED_KEY] = json!(notify);
    storage::set(NOTIFICATIONS_KEY, value);
    true
}

// ----- playing -----

fn cache() -> &'static Mutex<HashMap<&'static str, Arc<Decoded>>> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Arc<Decoded>>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

/// What a capture run would have played, newest last.
fn trace() -> &'static Mutex<Vec<String>> {
    static TRACE: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    TRACE.get_or_init(Mutex::default)
}

/// The sounds played (or, in a capture run, that would have played) since the app started.
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub fn played() -> Vec<String> {
    trace().lock().map(|list| list.clone()).unwrap_or_default()
}

static CURRENT_USER: AtomicU64 = AtomicU64::new(0);

/// Records the signed-in user id so global sound playback honors Streamer Mode sound suppression.
pub fn set_current_user(user: Option<u64>) {
    CURRENT_USER.store(user.unwrap_or(0), Ordering::Relaxed);
}

fn streamer_sounds_disabled() -> bool {
    let user = CURRENT_USER.load(Ordering::Relaxed);
    user != 0
        && crate::notifications::switch_value("streaming_enable_streamer_mode", Some(user))
            .unwrap_or(false)
        && crate::notifications::switch_value("streamer_mode_disable_sounds", Some(user))
            .unwrap_or(true)
}

/// `Ak(sound, volume, undefined, undefined, { outputChannel })`: play a sound now (as soon as its file is here).
pub fn play_with(sound: &str, volume: f32, output: Output) -> Handle {
    play_inner(sound, volume, output, false)
}

fn play_inner(sound: &str, volume: f32, output: Output, looped: bool) -> Handle {
    let handle = Handle::new();
    if streamer_sounds_disabled() {
        debug!(sound, "sound suppressed by streamer mode");
        return handle;
    }
    debug!(sound, volume, ?output, looped, "play sound");
    if let Ok(mut list) = trace().lock() {
        list.push(sound.to_owned());
    }
    if storage::is_capture_run() || std::env::var_os("GRAVECORD_INERT").is_some() {
        return handle;
    }
    let name = pack_sound(current_pack(), sound);
    let Some(file) = asset_file(name) else {
        debug!(sound, "no sound file for this name");
        return handle;
    };
    let start_playback = move |decoded: Arc<Decoded>, h: &Handle| {
        if looped {
            dc_voice::sound::play_looped(decoded, volume, output, h);
        } else {
            dc_voice::sound::play(decoded, volume, output, h);
        }
    };
    let cached = cache()
        .lock()
        .ok()
        .and_then(|files| files.get(file).cloned());
    if let Some(decoded) = cached {
        start_playback(decoded, &handle);
        return handle;
    }
    let waiting = handle.clone();
    let spawned = std::thread::Builder::new()
        .name("gravecord-sound".into())
        .spawn(move || match load(file) {
            Ok(decoded) => {
                let decoded = Arc::new(decoded);
                if let Ok(mut files) = cache().lock() {
                    files.insert(file, decoded.clone());
                }
                if !waiting.is_stopped() {
                    start_playback(decoded, &waiting);
                }
            }
            Err(error) => warn!(file, "sound unavailable: {error}"),
        });
    if spawned.is_err() {
        warn!("could not start the sound loader");
    }
    handle
}

/// `Ak(sound)`: volume 1 on the default speaker.
pub fn play(sound: &str) -> Handle {
    play_with(sound, 1.0, Output::Default)
}

/// Loops a call ring sound (`call_ringing` / `call_calling`) at full volume (`1.0`) unless disabled.
pub fn play_ring_loop(sound: &str) -> Option<Handle> {
    if is_sound_disabled(sound) {
        debug!(sound, "ring sound switched off");
        return None;
    }
    Some(play_inner(sound, 1.0, Output::Default, true))
}

/// Pre-fetches core voice, stream, and ring sounds in the background once per process when audio is active.
pub fn prefetch_call_sounds() {
    if cfg!(test) || storage::is_capture_run() || std::env::var_os("GRAVECORD_INERT").is_some() {
        return;
    }
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        const CALL_SOUNDS: &[&str] = &[
            "call_ringing",
            "call_calling",
            "user_join",
            "user_leave",
            "user_moved",
            "stream_started",
            "stream_ended",
            "camera_on",
            "camera_off",
            "mute",
            "unmute",
            "deafen",
            "undeafen",
            "disconnect",
        ];
        let _ = std::thread::Builder::new()
            .name("gravecord-sound-prefetch".into())
            .spawn(|| {
                let pack = current_pack();
                for sound in CALL_SOUNDS {
                    let name = pack_sound(pack, sound);
                    let Some(file) = asset_file(name) else {
                        continue;
                    };
                    let cached = cache()
                        .lock()
                        .ok()
                        .is_some_and(|files| files.contains_key(file));
                    if cached {
                        continue;
                    }
                    if let Ok(decoded) = load(file)
                        && let Ok(mut files) = cache().lock()
                    {
                        files.insert(file, Arc::new(decoded));
                    }
                }
            });
    });
}

/// `useSound`'s player (module 596339 `Sr`): an event's sound, unless its switch is off, at volume 0.4.
pub fn play_event(sound: &str) -> Option<Handle> {
    if is_sound_disabled(sound) {
        debug!(sound, "sound switched off");
        return None;
    }
    Some(play_with(sound, EVENT_VOLUME, Output::Default))
}

/// The sound heard when a microphone or speaker is picked (`setInputDevice` / `setOutputDevice`, module 827343):
/// `mention3` on the voice speaker, whatever the switches say.
pub fn play_device_changed() -> Handle {
    play_with("mention3", 1.0, Output::Voice)
}

/// Preview Sound links: the sound that was playing stops first (`aZ`), and the one playing ends when the page goes (`aK`).
pub fn preview(sound: &str) {
    stop_preview();
    let handle = play(sound);
    if let Ok(mut current) = previewing().lock() {
        *current = Some(handle);
    }
}

pub fn stop_preview() {
    if let Ok(mut current) = previewing().lock()
        && let Some(handle) = current.take()
    {
        handle.stop();
    }
}

fn previewing() -> &'static Mutex<Option<Handle>> {
    static CURRENT: OnceLock<Mutex<Option<Handle>>> = OnceLock::new();
    CURRENT.get_or_init(Mutex::default)
}

// ----- fetching -----

/// A file name the table can hold: 16 hex digits and `.mp3`. Nothing else is ever put in a URL or a path.
fn is_asset_file(file: &str) -> bool {
    file.strip_suffix(".mp3")
        .is_some_and(|stem| stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn disk_path(file: &str) -> Option<std::path::PathBuf> {
    is_asset_file(file)
        .then(|| storage::data_dir().map(|dir| dir.join("sounds").join(file)))
        .flatten()
}

/// The decoded file: from disk when it is there, else fetched from discord.com and kept.
fn load(file: &'static str) -> Result<Decoded, String> {
    if let Some(bytes) = disk_path(file).and_then(|path| std::fs::read(path).ok())
        && let Ok(decoded) = dc_voice::sound::decode_mp3(bytes)
    {
        return Ok(decoded);
    }
    let bytes = fetch(file)?;
    let decoded = dc_voice::sound::decode_mp3(bytes.clone()).map_err(str::to_owned)?;
    if let Some(path) = disk_path(file) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let temporary = path.with_extension("mp3.tmp");
        if std::fs::write(&temporary, &bytes).is_ok() {
            let _ = std::fs::rename(&temporary, &path);
        }
    }
    Ok(decoded)
}

/// The largest sound file accepted. The biggest one the build has is about 100 KB.
const MAX_BYTES: usize = 2 * 1024 * 1024;

fn fetch(file: &str) -> Result<Vec<u8>, String> {
    if !is_asset_file(file) {
        return Err("not an asset file name".to_owned());
    }
    fetch_url(&format!("{ASSET_ROOT}{file}"))
}

/// A sound file from a CDN URL, on a small runtime of its own (the caller is a plain thread).
pub(crate) fn fetch_url(url: &str) -> Result<Vec<u8>, String> {
    fetch_url_limited(url, MAX_BYTES)
}

/// Attachment audio has its own budget; it must not inherit the short UI sounds' limit.
pub(crate) fn fetch_url_limited(url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    fetch_url_typed(url, max_bytes).map(|(bytes, _)| bytes)
}

/// A file from a URL and its `Content-Type` (empty when the answer names none).
pub(crate) fn fetch_url_typed(url: &str, max_bytes: usize) -> Result<(Vec<u8>, String), String> {
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    // One client for Discord's hosts (the bare one these requests always used) and one, plain, for any other.
    static DISCORD: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    static OTHER: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    let runtime = RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
        .ok_or("no runtime")?;
    let client = if dc_http::plain::is_discord_url(url) {
        &DISCORD
    } else {
        &OTHER
    };
    let client = client
        .get_or_init(|| {
            dc_http::plain::builder_for(url)
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .map_err(|error| error.to_string())
        })
        .clone()?;
    let url = url.to_owned();
    runtime.block_on(async move {
        let asked = std::time::Instant::now();
        let mut response = client
            .get(dc_proto::replay::rewrite(&url).as_ref())
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("status {}", response.status()));
        }
        let kind = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        if response
            .content_length()
            .is_some_and(|size| size > max_bytes as u64)
        {
            return Err("file too large".to_owned());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
            if chunk.len() > max_bytes - bytes.len() {
                return Err("file too large".to_owned());
            }
            bytes.extend_from_slice(&chunk);
        }
        dc_proto::replay::record_http("GET", &url, 200, Some(&kind), asked.elapsed(), &bytes);
        Ok((bytes, kind))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_settings_rows_read_and_write_the_store() {
        let mut switches = Switches::default();
        let mut notify = false;
        assert_eq!(
            read_switch(&switches, notify, "sounds_list_item_mute"),
            Some(true)
        );
        assert!(write_switch(
            &mut switches,
            &mut notify,
            "sounds_list_item_mute",
            false
        ));
        assert_eq!(switches.disabled, ["mute"]);
        assert_eq!(
            read_switch(&switches, notify, "sounds_list_item_mute"),
            Some(false)
        );
        assert_eq!(
            read_switch(&switches, notify, "sounds_list_item_deafen"),
            Some(true)
        );
        // Disable All turns every row off without touching the list.
        assert!(write_switch(&mut switches, &mut notify, DISABLE_ALL, true));
        assert_eq!(read_switch(&switches, notify, DISABLE_ALL), Some(true));
        assert_eq!(
            read_switch(&switches, notify, "sounds_list_item_deafen"),
            Some(false)
        );
        assert!(write_switch(
            &mut switches,
            &mut notify,
            SELECTED_CHANNEL,
            true
        ));
        assert_eq!(
            read_switch(&switches, notify, SELECTED_CHANNEL),
            Some(false),
            "all off wins"
        );
        switches.all = false;
        assert_eq!(read_switch(&switches, notify, SELECTED_CHANNEL), Some(true));
        assert_eq!(switches.disabled, ["mute"], "the list kept its own");
        assert!(!write_switch(&mut switches, &mut notify, "echo", true));
        assert_eq!(read_switch(&switches, notify, "echo"), None);
        assert_eq!(row_sound("sounds_list_item_ptt_start"), Some("ptt_start"));
        assert_eq!(row_sound(SELECTED_CHANNEL), Some("message3"));
        assert_eq!(row_sound(DISABLE_ALL), None);
    }

    use super::*;
    use serde_json::json;

    fn assignments(name: &str, variant: i64) -> Assignments {
        Assignments::from_ready(
            &json!({"assignments": {"1": {"7": {"assignments": [[dc_state::experiments::hash(name), variant, 0, 1]]}}}}),
            "7",
        )
    }

    #[test]
    fn every_sound_name_has_a_file_and_the_table_is_sorted() {
        assert_eq!(table::FILES.len(), 93);
        assert!(table::FILES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(table::FILES.iter().all(|(_, file)| is_asset_file(file)));
        // The names of the Voice & Video list and the packs all resolve (module 158935, module 458640).
        for sound in [
            "deafen",
            "undeafen",
            "mute",
            "unmute",
            "camera_on",
            "camera_off",
            "disconnect",
            "ptt_start",
            "ptt_stop",
            "user_join",
            "user_leave",
            "user_moved",
            "call_calling",
            "stream_started",
            "stream_ended",
            "stream_user_joined",
            "stream_user_left",
            "activity_launch",
            "activity_end",
            "activity_user_join",
            "activity_user_left",
            "reconnect",
            "message1",
            "message3",
            "call_ringing",
            "mention3",
        ] {
            assert!(asset_file(sound).is_some(), "{sound}");
        }
        for pack in Pack::ALL {
            for (_, mapped) in pack.table() {
                assert!(asset_file(mapped).is_some(), "{mapped}");
            }
        }
        // Spot checks against dumps/SOUNDS.md through module 696354.
        assert_eq!(asset_file("mute"), Some("2d3b4ba32c34c862.mp3"));
        assert_eq!(asset_file("mention3"), Some("260dfacf63b6cb13.mp3"));
        assert_eq!(asset_file("nope"), None);
    }

    #[test]
    fn a_pack_maps_a_sound_or_leaves_it() {
        assert_eq!(pack_sound(Pack::Classic, "mute"), "mute");
        assert_eq!(pack_sound(Pack::Retro, "message1"), "bit_message1");
        assert_eq!(pack_sound(Pack::Retro, "mute"), "mute", "`u[e] ?? e`");
        assert_eq!(pack_sound(Pack::Halloween, "mute"), "halloween_mute");
        assert_eq!(pack_sound(Pack::Halloween, "ptt_start"), "ptt_start");
        assert_eq!(pack_sound(Pack::Bubble, "message1"), "bop_message1");
        assert_eq!(pack_sound(Pack::Discodo, "message1"), "discodo");
        assert_eq!(
            pack_sound(Pack::WinterHoliday, "user_join"),
            "winter_user_join"
        );
        assert_eq!(Pack::from_id("bop"), Some(Pack::Bubble));
        assert_eq!(Pack::from_id("nope"), None);
    }

    #[test]
    fn the_holiday_needs_the_window_and_the_experiment() {
        let event = &HALLOWEEN_2026;
        let on = assignments(event.experiment, 1);
        let off = assignments(event.experiment, 0);
        let middle = (event.start_ms + event.end_ms) / 2;
        assert!(event.is_eligible(middle, &on));
        assert!(
            event.is_eligible(event.start_ms, &on),
            "the window is inclusive"
        );
        assert!(event.is_eligible(event.end_ms, &on));
        assert!(!event.is_eligible(event.start_ms - 1, &on));
        assert!(!event.is_eligible(event.end_ms + 1, &on));
        assert!(
            !event.is_eligible(middle, &off),
            "variation 0 is the default config"
        );
        assert!(!event.is_eligible(middle, &Assignments::default()));
        // 2026-10-07 16:00 UTC to 2026-11-02 17:00 UTC.
        assert_eq!(event.start_ms, 1_791_388_800_000);
        assert_eq!(event.end_ms, 1_793_638_800_000);
    }

    #[test]
    fn the_soundpack_follows_the_event() {
        let event = &HALLOWEEN_2026;
        let classic = SoundpackState {
            soundpack: Pack::Classic,
            last_experiment: None,
        };
        // An eligible event sets its pack once.
        assert_eq!(
            soundpack_update(&classic, event, true),
            Some((Pack::Halloween, Some(event.experiment)))
        );
        let halloween = SoundpackState {
            soundpack: Pack::Halloween,
            last_experiment: Some(event.experiment.to_owned()),
        };
        assert_eq!(soundpack_update(&halloween, event, true), None);
        // The user switched back to classic during the event: it is not forced again.
        let chose_classic = SoundpackState {
            soundpack: Pack::Classic,
            last_experiment: Some(event.experiment.to_owned()),
        };
        assert_eq!(soundpack_update(&chose_classic, event, true), None);
        // After the event the pack returns to classic and forgets the experiment.
        assert_eq!(
            soundpack_update(&halloween, event, false),
            Some((Pack::Classic, None))
        );
        assert_eq!(soundpack_update(&classic, event, false), None);
    }

    #[test]
    fn the_switches_follow_the_notification_store() {
        let mut switches = Switches::default();
        assert!(switches.is_on("mute") && !switches.is_disabled("mute"));
        switches.set_on("mute", false);
        switches.set_on("mute", false);
        assert_eq!(switches.disabled, ["mute"], "a sound is listed once");
        assert!(switches.is_disabled("mute") && switches.is_on("deafen"));
        switches.set_on("mute", true);
        assert!(switches.disabled.is_empty());
        // `disableAllSounds` turns every switch off without touching the list.
        switches.set_on("unmute", false);
        switches.all = true;
        assert!(switches.is_disabled("deafen") && !switches.is_on("deafen"));
        assert_eq!(switches.disabled, ["unmute"]);
    }

    #[test]
    fn only_table_file_names_reach_a_url_or_a_path() {
        assert!(is_asset_file("2d3b4ba32c34c862.mp3"));
        assert!(!is_asset_file("../2d3b4ba32c34c862.mp3"));
        assert!(!is_asset_file("2d3b4ba32c34c862.exe"));
        assert!(!is_asset_file("2d3b4ba32c34c86.mp3"));
        assert!(!is_asset_file("2d3b4ba32c34c86g.mp3"));
    }

    /// Fetches the sounds the Voice & Video list names from discord.com, decodes them and checks what comes out. It
    /// plays nothing. Needs the network, so it only runs when asked: `cargo test -p gravecord -- --ignored real_sounds`.
    #[test]
    #[ignore = "fetches from discord.com"]
    fn real_sounds_download_and_decode() {
        for sound in [
            "mute",
            "unmute",
            "deafen",
            "user_join",
            "ptt_start",
            "mention3",
        ] {
            let file = asset_file(sound).unwrap();
            let decoded = load(file).unwrap_or_else(|error| panic!("{sound}: {error}"));
            let seconds = decoded.duration().as_secs_f64();
            println!(
                "{sound}: {file} {} Hz, {} channels, {seconds:.3} s",
                decoded.rate, decoded.channels
            );
            assert!(matches!(decoded.rate, 44_100 | 48_000), "{sound}");
            assert!((0.05..5.0).contains(&seconds), "{sound}: {seconds}");
            assert!(decoded.samples.iter().any(|sample| sample.abs() > 0.001));
        }
    }
}
