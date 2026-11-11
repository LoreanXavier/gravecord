//! The app's side of the Clips engine (`crates/dc-clips`): the save keybind, the save itself with the official limits and
//! outcomes, the clips found in the storage folder, and the settings pushed to the recorder.
//!
//! Two sources feed the buffer: the stream's own encoder (Go Live) and, with no stream, the capture of the visible
//! game's window (`crate::clips_capture`, the decoupled branch of `yd`). Auto clips are not here; the gallery, the
//! editor and the toast are in `ui::clips`. What follows module 489496 `yd` (the save), module 419490 (the store's counters and its handling of
//! `CLIPS_SAVE_CLIP_*` and `CLIPS_LOAD_DIRECTORY_SUCCESS`) and module 596339 (the keybind).
use super::*;
use crate::net::Update;
use dc_clips::{Clip, ClipMetadata, SnapshotError, SourceInfo};
use dc_voice::sound::Output;
use std::collections::BTreeMap;
use std::time::Duration;

/// `getIsAtMaxSaveClipOperations`: the counter at which a new save is refused.
pub(super) const MAX_SAVES_IN_FLIGHT: u32 = 15;
/// `ut`: how long a save may run before `CLIPS_SAVE_CLIP_TIMEOUT` is dispatched.
const SAVE_TIMEOUT: Duration = Duration::from_millis(60_000);
/// The volume of `clip_save` and `clip_error` (`Ak("clip_save", .5)`).
const SAVE_SOUND_VOLUME: f32 = 0.5;
/// How many lines of the engine's own trace are kept, for the harness.
const TRACE_LINES: usize = 64;

/// `R.RC`: why a save did nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum NoOp {
    MaxConcurrentSaves,
    NoEligibleSource,
    BufferWarmingUp,
    RecordingNotReady,
}

impl NoOp {
    pub(in crate::ui) fn code(self) -> &'static str {
        match self {
            NoOp::MaxConcurrentSaves => "max_concurrent_saves",
            NoOp::NoEligibleSource => "no_eligible_source",
            NoOp::BufferWarmingUp => "buffer_warming_up",
            NoOp::RecordingNotReady => "recording_not_ready",
        }
    }
}

/// What pressing the keybind came to at once; the save itself finishes later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum SaveStart {
    Started,
    NoOp(NoOp),
}

/// The engine's part of the Clips state: the store's non-persisted fields (`clips`, `newClipIds`, `hasClips`, the save
/// counter, `lastClipsError`), the keybind hook, and a short trace of what happened.
#[derive(Default)]
pub(in crate::ui) struct Engine {
    /// `m` in the store: every clip loaded from the folder or saved since, by id.
    pub clips: BTreeMap<String, Clip>,
    /// `newClipIds`.
    pub new_clip_ids: Vec<String>,
    pub has_clips: bool,
    /// `S`: saves started and not finished.
    pub in_flight: u32,
    /// `L`: the last engine error, shown in the entry points' tooltip.
    pub last_error: Option<String>,
    /// The account is listed for Clips (Nitro): the settings the recorder gets depend on it.
    listed: std::cell::Cell<bool>,
    #[cfg(windows)]
    hook: Option<dc_hotkey::Hook>,
    /// What the engine did, newest last: dispatches, sounds and the texts of the notifications.
    pub trace: Vec<String>,
    /// `getVisibleGame()` with its `windowHandle`: the game the decoupled path records.
    #[cfg(windows)]
    pub game: Option<crate::clips_capture::Game>,
    /// The running capture of that game.
    #[cfg(windows)]
    pub capture: Option<crate::clips_capture::GameCapture>,
    #[cfg(windows)]
    pub capture_starting: bool,
    /// What the capture runs with, so a change of the page's quality restarts it: height, frame rate, percent.
    #[cfg(windows)]
    pub capture_quality: Option<(u32, u32, u32)>,
    /// The one-second loop that keeps the capture in step with the game, the settings and the stream runs.
    pub ticking: bool,
    /// Set by the harness: the visible game is given by a script, and the scan does not replace it.
    pub test_game: bool,
    /// The Quick Bar state and picture cache (`ui::clips`).
    pub ui: crate::ui::clips::State,
    /// When the visible game became the visible one, and how long it is left alone before its window is captured.
    #[cfg(windows)]
    pub game_since: Option<std::time::Instant>,
    #[cfg(windows)]
    pub game_delay: Duration,
}

impl Engine {
    /// `setClipsRecordingEnabled` and `setClipBufferLength`: the recorder keeps the stream's last `clipsLength` while
    /// clipping is on for this account, and nothing otherwise.
    pub(super) fn configure(&self, settings: &ClipsSettings) {
        dc_clips::recorder().configure(
            settings.clips_enabled && self.listed.get(),
            settings.clips_length.millis(),
        );
    }

    fn note(&mut self, line: impl Into<String>) {
        let line = line.into();
        tracing::debug!("clips: {line}");
        self.trace.push(line);
        if self.trace.len() > TRACE_LINES {
            self.trace.remove(0);
        }
    }

    /// `CLIPS_LOAD_DIRECTORY_SUCCESS`: the loaded clips replace the known ones; `hasClips` follows.
    fn directory_loaded(&mut self, clips: Vec<Clip>) {
        self.clips = clips
            .into_iter()
            .filter(|clip| clip.meta.is_candidate != Some(true))
            .map(|clip| (clip.meta.id.clone(), clip))
            .collect();
        self.has_clips = !self.clips.is_empty();
    }

    /// `CLIPS_SAVE_CLIP`: the counter goes down, the clip is stored with its new-clip mark.
    fn clip_saved(&mut self, clip: Clip) {
        self.in_flight = self.in_flight.saturating_sub(1);
        if clip.meta.is_candidate != Some(true) {
            self.new_clip_ids.push(clip.meta.id.clone());
            self.clips.insert(clip.meta.id.clone(), clip);
            self.has_clips = true;
        }
    }
}

/// The wall clock in milliseconds since the Unix epoch.
fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// A data URL for the clip's first picture, made as the official client makes it when the engine returns none
/// (module 956050: the picture scaled to fit 640 x 360, JPEG at 0.9). Empty when the picture cannot be decoded (the
/// document allows an empty thumbnail).
fn thumbnail(first_unit: &[u8]) -> String {
    use base64::Engine as _;
    use image::{DynamicImage, RgbaImage, codecs::jpeg::JpegEncoder, imageops};
    let picture = dc_media::decode::Decoder::software()
        .ok()
        .and_then(|mut decoder| decoder.decode(first_unit).ok().flatten());
    let Some(picture) = picture else {
        return String::new();
    };
    let Some(rgba) = picture.to_rgba() else {
        return String::new();
    };
    let Some(image) = RgbaImage::from_raw(picture.width, picture.height, rgba) else {
        return String::new();
    };
    let scale = (640.0 / f64::from(picture.width)).min(360.0 / f64::from(picture.height));
    let width = ((f64::from(picture.width) * scale) as u32).max(1);
    let height = ((f64::from(picture.height) * scale) as u32).max(1);
    let image = imageops::resize(&image, width, height, imageops::FilterType::Triangle);
    let rgb = DynamicImage::ImageRgba8(image).to_rgb8();
    let mut jpeg = Vec::new();
    if JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode_image(&rgb)
        .is_err()
    {
        return String::new();
    }
    format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    )
}

/// What the save thread hands back.
struct Saved {
    clip: Clip,
}

enum SaveFailure {
    /// `buffer_warming_up`: no keyframe held yet.
    WarmingUp,
    Error(String),
}

/// Cuts the window, makes the thumbnail and writes the file. Runs off the interface thread.
fn run_save(dir: PathBuf, mut meta: ClipMetadata) -> Result<Saved, SaveFailure> {
    let snapshot = match dc_clips::recorder().snapshot() {
        Ok(snapshot) => snapshot,
        Err(SnapshotError::WarmingUp) => return Err(SaveFailure::WarmingUp),
        Err(other) => return Err(SaveFailure::Error(other.to_string())),
    };
    meta.application_name = snapshot.source.name.clone();
    meta.application_id = snapshot.source.application_id.clone();
    meta.thumbnail = thumbnail(&snapshot.first_keyframe);
    let path = dir.join(dc_clips::file_name(&meta.application_name, &meta.id));
    let size = snapshot
        .write(&path, &mut meta)
        .map_err(|error| SaveFailure::Error(format!("could not write the clip: {error}")))?;
    Ok(Saved {
        clip: Clip {
            meta,
            filepath: path,
            size_bytes: size,
        },
    })
}

impl Root {
    /// The account's tier decides whether the recorder may keep anything; it is read where it can change (the
    /// connection opening, a stream starting, a setting changing).
    pub(super) fn clips_sync_engine(&mut self) {
        self.clips.engine.listed.set(self.clips_listed());
        self.clips.engine.configure(&self.clips.store.settings);
    }

    /// The game the decoupled path records changed (`getVisibleGame()`): `None` when there is none or it has no window.
    #[cfg(windows)]
    pub(in crate::ui) fn clips_game_changed(
        &mut self,
        game: Option<crate::clips_capture::Game>,
        cx: &mut Context<Self>,
    ) {
        if self.clips.engine.game != game {
            // A game that is new to the list waits before its window is captured (module 516130); a test game does not.
            let new_process = self.clips.engine.game.as_ref().map(|game| game.pid)
                != game.as_ref().map(|game| game.pid);
            if new_process {
                self.clips.engine.game_since = Some(std::time::Instant::now());
                self.clips.engine.game_delay = if self.clips.engine.test_game {
                    Duration::ZERO
                } else {
                    crate::clips_capture::NEW_GAME_DELAY
                };
            }
            self.clips.engine.note(match &game {
                Some(game) => format!(
                    "visible game {} pid {} window {:#x}",
                    game.name, game.pid, game.window
                ),
                None => "visible game none".to_owned(),
            });
            // `CLIPS_SESSION_STOP` / `CLIPS_SESSION_START` (module 516130): the game that was running offers its clips,
            // and a session that starts with no game before it closes the panel.
            let previous = self.clips.engine.game.take();
            match previous.and_then(|previous| previous.application_id) {
                Some(id) => self.clips_session_stop(Some(id), cx),
                None => self.clips_session_start(cx),
            }
            self.clips.engine.game = game;
        }
        self.clips_ensure_ticking(cx);
        self.clips_sync_capture(cx);
    }

    /// Starts the one-second loop once.
    pub(in crate::ui) fn clips_ensure_ticking(&mut self, cx: &mut Context<Self>) {
        if self.clips.engine.ticking {
            return;
        }
        self.clips.engine.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this
                    .update(cx, |this, cx| {
                        #[cfg(windows)]
                        this.clips_sync_capture(cx);
                        this.clips_ui_tick(cx);
                    })
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
    }

    /// The decoupled path (module 489496 `yd`, `B = U && x && k`): while Enable Clipping is on, a visible game has a
    /// window and no stream of ours feeds the buffer, the game's window is captured with the page's quality. It stops
    /// when the game goes, Clipping is turned off, a stream takes the buffer (its sender feeds the same clips: one
    /// capture serves both) or the quality changes (it restarts).
    #[cfg(windows)]
    pub(in crate::ui) fn clips_sync_capture(&mut self, cx: &mut Context<Self>) {
        self.clips_sync_engine();
        let quality = &self.clips.store.settings.clips_quality;
        let wanted_quality = (
            quality.resolution.height() as u32,
            quality.frame_rate.fps() as u32,
            quality.bitrate_percent as u32,
        );
        let game = self.clips.engine.game.clone();
        let location = &self.clips.store.settings.storage_location;
        let location_set = !location.is_empty() && location != DEFAULT_LOCATION;
        let enabled = self.clips_enabled();
        let own_stream = self.own_stream_active();
        let engine = &mut self.clips.engine;
        let ended = engine.capture.as_ref().and_then(|capture| capture.ended());
        let plan = crate::clips_capture::plan(&crate::clips_capture::Situation {
            enabled,
            own_stream,
            location_set,
            game: game.as_ref(),
            game_age: engine
                .game_since
                .map_or(Duration::ZERO, |since| since.elapsed()),
            delay: engine.game_delay,
            quality: wanted_quality,
            running: engine
                .capture
                .as_ref()
                .zip(engine.capture_quality)
                .map(|(capture, quality)| (&capture.game, quality, ended)),
            starting: engine.capture_starting,
        });
        if let Some(why) = plan.stop
            && let Some(capture) = engine.capture.take()
        {
            let (captured, encoded) = capture.counts();
            let line = format!(
                "clip buffer stopped for {}: {why} ({captured} pictures captured, {encoded} encoded)",
                capture.game.name
            );
            tracing::info!("{line}");
            engine.note(line);
        }
        if !plan.start {
            return;
        }
        let Some(game) = game else { return };
        engine.capture_starting = true;
        let (height, fps, percent) = wanted_quality;
        let started = game.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    crate::clips_capture::GameCapture::start(started, height, fps, percent)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let engine = &mut this.clips.engine;
                engine.capture_starting = false;
                match result {
                    Ok(capture) => {
                        let line = format!(
                            "clip buffer started for {}: {}x{} at {} fps, {} b/s, encoder {}, game sound {}",
                            capture.game.name,
                            capture.settings.width,
                            capture.settings.height,
                            capture.settings.fps,
                            capture.settings.bitrate,
                            capture.backend(),
                            capture.audio_error.as_deref().unwrap_or("recorded")
                        );
                        tracing::info!("{line}");
                        engine.capture = Some(capture);
                        engine.capture_quality = Some((height, fps, percent));
                        engine.last_error = None;
                        engine.note(line);
                    }
                    Err(error) => {
                        let line = format!("clip buffer could not start: {error}");
                        tracing::info!("{line}");
                        engine.note(line);
                        engine.last_error = Some(error);
                        // Try again in ten seconds, not every second.
                        engine.game_since = Some(std::time::Instant::now());
                        engine.game_delay = Duration::from_secs(10);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A stream of ours is about to start: the recorder learns what it shows (`sourceName`) and the settings are
    /// current.
    pub(in crate::ui) fn clips_stream_starting(&mut self, source_name: &str) {
        self.clips_sync_engine();
        dc_clips::recorder().announce_source(SourceInfo {
            name: source_name.to_owned(),
            application_id: None,
        });
    }

    /// The connection opened: the engine is configured, the keybind watched, and the clips in the storage folder
    /// loaded (`Fb` in module 489496 runs at `CONNECTION_OPEN`).
    pub(in crate::ui) fn clips_connection_open(&mut self, cx: &mut Context<Self>) {
        self.clips_sync_engine();
        self.sync_clips_hook();
        self.clips_reload_directory(cx);
    }

    /// `loadClipsDirectory(storageLocation)` and `CLIPS_LOAD_DIRECTORY_SUCCESS`. Does nothing while the location is
    /// still the unresolved default.
    pub(super) fn clips_reload_directory(&mut self, cx: &mut Context<Self>) {
        let location = self.clips.store.settings.storage_location.clone();
        if location == DEFAULT_LOCATION || location.is_empty() {
            return;
        }
        let dir = PathBuf::from(location);
        cx.spawn(async move |this, cx| {
            let scan = dir.clone();
            let clips = cx
                .background_executor()
                .spawn(async move { dc_clips::load_directory(&scan) })
                .await;
            let _ = this.update(cx, |this, cx| {
                // The folder may have changed while the scan ran.
                if Path::new(&this.clips.store.settings.storage_location) == dir.as_path() {
                    let count = clips.len();
                    this.clips.engine.directory_loaded(clips);
                    this.clips
                        .engine
                        .note(format!("CLIPS_LOAD_DIRECTORY_SUCCESS clips={count}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The system-wide `SAVE_CLIP` keybind (module 596339): watched while Enable Clipping is on, never in a capture run,
    /// and not while the keybind is being recorded. It fires on key down only.
    pub(super) fn sync_clips_hook(&mut self) {
        let wanted = !storage::is_capture_run()
            && !self.clips.recording
            && self.clips_enabled()
            && !self.clips.keybind.is_empty();
        #[cfg(windows)]
        {
            if !wanted {
                if let Some(hook) = self.clips.engine.hook.take() {
                    hook.stop();
                }
                return;
            }
            let shortcut = self.clips.keybind.clone();
            match &self.clips.engine.hook {
                Some(hook) => hook.set_shortcut(shortcut),
                None => {
                    let updates = self.updates.clone();
                    match dc_hotkey::start(shortcut, move |down| {
                        if down {
                            let _ = updates.send(Update::ClipSaveKey);
                        }
                    }) {
                        Ok(hook) => self.clips.engine.hook = Some(hook),
                        Err(error) => tracing::warn!("clip keybind hook not started: {error}"),
                    }
                }
            }
        }
        #[cfg(not(windows))]
        let _ = wanted;
    }

    /// The keybind went down (`[H.hCu.SAVE_CLIP]`): ignored unless `T()` is true, then the manual signal and the save.
    pub(in crate::ui) fn clips_save_key(&mut self, cx: &mut Context<Self>) {
        if self.clips.recording || !self.clips_enabled() {
            return;
        }
        self.clips_save(cx);
    }

    /// `yd` for a manual clip on the Go Live path.
    pub(in crate::ui) fn clips_save(&mut self, cx: &mut Context<Self>) -> SaveStart {
        self.clips_sync_engine();
        let noop = |this: &mut Self, reason: NoOp| {
            this.clips
                .engine
                .note(format!("CLIPS_SAVE_CLIP_NO_OP reason={}", reason.code()));
            SaveStart::NoOp(reason)
        };
        if self.clips.engine.in_flight >= MAX_SAVES_IN_FLIGHT {
            return noop(self, NoOp::MaxConcurrentSaves);
        }
        // The native module is part of this client, so it is always loaded. A source: the account is clipping and
        // streaming (`F`), or clipping with a visible game whose window is being captured (`B`).
        let streaming = self.own_stream_active();
        #[cfg(windows)]
        let decoupled = self.clips.engine.game.is_some() && self.clips.engine.capture.is_some();
        #[cfg(not(windows))]
        let decoupled = false;
        if !self.clips_enabled() || !(streaming || decoupled) {
            return noop(self, NoOp::NoEligibleSource);
        }
        let recorder = dc_clips::recorder();
        if !recorder.has_source() || !recorder.is_ready() {
            return noop(self, NoOp::RecordingNotReady);
        }
        let location = self.clips.store.settings.storage_location.clone();
        if location == DEFAULT_LOCATION || location.is_empty() {
            self.clips.engine.last_error = Some("No storage location".to_owned());
            return noop(self, NoOp::NoEligibleSource);
        }
        // `CLIPS_SAVE_CLIP_START`, the `clip_save` sound for a manual clip, and the "Saving clip" notification.
        self.clips.engine.in_flight += 1;
        let sound = crate::sounds::play_with("clip_save", SAVE_SOUND_VOLUME, Output::Default);
        self.clips.engine.note(format!(
            "CLIPS_SAVE_CLIP_START clipType={} clipMethod=manual",
            if streaming { "streamer" } else { "decoupled" }
        ));
        self.clips.engine.note("sound clip_save volume=0.5");
        self.clips
            .engine
            .note("notification \"Saving clip\" (not drawn: no capture of it)");
        let me = self
            .stores
            .current_user
            .as_ref()
            .map(|user| user.id.to_string())
            .into_iter()
            .collect::<Vec<_>>();
        let mut meta = ClipMetadata::new_manual(
            uuid::Uuid::new_v4().to_string(),
            epoch_ms(),
            String::new(),
            me,
        );
        let moment = dc_clips::now_ms();
        let signal = serde_json::json!({"type": "manual"});
        meta.timeline = vec![serde_json::json!({"signal": signal, "timestamp": moment})];
        meta.decision = Some(serde_json::json!({"signal": signal, "timestamp": moment}));
        if let Some(call) = &self.call {
            meta.channel_id = Some(call.channel.to_string());
            meta.guild_id = (call.guild != 0).then(|| call.guild.to_string());
        }
        let id = meta.id.clone();
        let dir = PathBuf::from(location);
        cx.spawn(async move |this, cx| {
            let watched = id.clone();
            let result = cx
                .background_executor()
                .spawn(async move { run_save(dir, meta) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.clips_save_finished(&watched, result, sound, cx);
            });
        })
        .detach();
        // The 60 second watchdog: `CLIPS_SAVE_CLIP_TIMEOUT` changes nothing but the record of it.
        let pending = self.clips.engine.in_flight;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_TIMEOUT).await;
            let _ = this.update(cx, |this, _| {
                if this.clips.engine.in_flight >= pending && pending > 0 {
                    this.clips
                        .engine
                        .note("CLIPS_SAVE_CLIP_TIMEOUT elapsedMs=60000");
                }
            });
        })
        .detach();
        cx.notify();
        SaveStart::Started
    }

    fn clips_save_finished(
        &mut self,
        id: &str,
        result: Result<Saved, SaveFailure>,
        sound: dc_voice::sound::Handle,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(Saved { clip }) => {
                self.clips.engine.note(format!(
                    "CLIPS_SAVE_CLIP id={id} file={} bytes={} lengthMs={}",
                    clip.filepath.display(),
                    clip.size_bytes,
                    clip.meta.length
                ));
                self.clips.engine.clip_saved(clip);
                self.clips_after_save(cx);
            }
            Err(SaveFailure::WarmingUp) => {
                // A no-op that gives the counter back: the sound stops, nothing is shown.
                sound.stop();
                self.clips.engine.in_flight = self.clips.engine.in_flight.saturating_sub(1);
                self.clips.engine.note(format!(
                    "CLIPS_SAVE_CLIP_NO_OP reason={}",
                    NoOp::BufferWarmingUp.code()
                ));
            }
            Err(SaveFailure::Error(message)) => {
                // `CLIPS_SAVE_CLIP_ERROR`: the sound stops, `clip_error` plays, "Unable to save clip" is shown.
                sound.stop();
                crate::sounds::play_with("clip_error", SAVE_SOUND_VOLUME, Output::Default);
                self.clips.engine.in_flight = self.clips.engine.in_flight.saturating_sub(1);
                self.clips
                    .engine
                    .note(format!("CLIPS_SAVE_CLIP_ERROR {message}"));
                self.clips.engine.note("sound clip_error volume=0.5");
                self.clips
                    .engine
                    .note("notification \"Unable to save clip\" (not drawn: no capture of it)");
                self.clips.engine.last_error = Some(message);
            }
        }
        cx.notify();
    }

    /// The harness's `clips status`.
    #[cfg(feature = "capture")]
    pub(super) fn clips_status_line(&self) -> String {
        let engine = &self.clips.engine;
        let recorder = dc_clips::recorder();
        let (video, application, voice) = recorder.held();
        format!(
            "enabled {} source {} ready {} held video {} B, audio packets {}/{} in_flight {} clips {} new {:?} has_clips {} last_error {:?}\n{}",
            recorder.is_enabled(),
            recorder.has_source(),
            recorder.is_ready(),
            video,
            application,
            voice,
            engine.in_flight,
            engine.clips.len(),
            engine.new_clip_ids,
            engine.has_clips,
            engine.last_error,
            engine.trace.join("\n")
        )
    }

    /// The harness's `clips list`: what the store knows of each clip.
    #[cfg(feature = "capture")]
    pub(super) fn clips_list_line(&self) -> String {
        let mut lines = Vec::new();
        for clip in self.clips.engine.clips.values() {
            lines.push(format!(
                "{} {} {} bytes length {} ms app {:?} method {} new {}",
                clip.meta.id,
                clip.filepath.display(),
                clip.size_bytes,
                clip.meta.length,
                clip.meta.application_name,
                clip.meta.clip_method,
                self.clips.engine.new_clip_ids.contains(&clip.meta.id)
            ));
        }
        if lines.is_empty() {
            "no clips".to_owned()
        } else {
            lines.join("\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: &str, candidate: bool) -> Clip {
        let mut meta = ClipMetadata::new_manual(id.into(), 1, "G".into(), vec![]);
        meta.is_candidate = Some(candidate);
        Clip {
            meta,
            filepath: PathBuf::from(format!("{id}.mp4")),
            size_bytes: 10,
        }
    }

    #[test]
    fn the_save_counter_and_the_new_clip_mark_follow_the_store() {
        let mut engine = Engine {
            in_flight: 2,
            ..Engine::default()
        };
        engine.clip_saved(clip("a", false));
        assert_eq!(engine.in_flight, 1);
        assert_eq!(engine.new_clip_ids, ["a"]);
        assert!(engine.has_clips);
        // A candidate frees a slot but is neither stored nor marked.
        engine.clip_saved(clip("b", true));
        assert_eq!(engine.in_flight, 0);
        assert_eq!(engine.clips.len(), 1);
        // The counter never goes below zero.
        engine.clip_saved(clip("c", false));
        assert_eq!(engine.in_flight, 0);
    }

    #[test]
    fn a_loaded_directory_replaces_the_clips_but_keeps_the_new_marks() {
        let mut engine = Engine::default();
        engine.clip_saved(clip("old", false));
        engine.directory_loaded(vec![clip("x", false), clip("y", true)]);
        assert_eq!(engine.clips.keys().collect::<Vec<_>>(), ["x"]);
        assert!(engine.has_clips);
        assert_eq!(engine.new_clip_ids, ["old"]);
        engine.directory_loaded(Vec::new());
        assert!(!engine.has_clips);
    }

    #[test]
    fn the_limits_are_the_official_ones() {
        assert_eq!(MAX_SAVES_IN_FLIGHT, 15);
        assert_eq!(SAVE_TIMEOUT, Duration::from_secs(60));
        assert_eq!(NoOp::MaxConcurrentSaves.code(), "max_concurrent_saves");
        assert_eq!(NoOp::NoEligibleSource.code(), "no_eligible_source");
        assert_eq!(NoOp::BufferWarmingUp.code(), "buffer_warming_up");
        assert_eq!(NoOp::RecordingNotReady.code(), "recording_not_ready");
    }

    #[test]
    fn the_trace_keeps_the_newest_lines() {
        let mut engine = Engine::default();
        for index in 0..(TRACE_LINES + 5) {
            engine.note(format!("line {index}"));
        }
        assert_eq!(engine.trace.len(), TRACE_LINES);
        assert_eq!(
            engine.trace.last().unwrap(),
            &format!("line {}", TRACE_LINES + 4)
        );
    }
}
