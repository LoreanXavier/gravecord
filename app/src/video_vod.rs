//! Progressive VOD MP4 decoding and A/V-synchronized playback for attached and proxied videos.
//!
//! Each started [`VodPlayer`] runs H.264 decoding on its own worker thread so COM / Media Foundation (`!Send`
//! [`dc_media::decode::Decoder`]) stays pinned to that thread and the UI thread never blocks on frame decode or I/O.
//! Audio (when present) is decoded once with [`dc_voice::voice_player::Clip::from_file`] and played through the shared
//! voice player engine so video PTS slaves tightly to the live audio playhead and honors `MediaPlayerVolume`. Videos
//! without an audio stream advance cleanly on a monotonic [`Instant`] clock.

use crate::spinner::H264Track;
use dc_media::{Picture, decode::Decoder};
use dc_voice::voice_player::{self, Clip};
use gpui::RenderImage;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

pub(crate) const KEY_PREFIX: &str = "video:";
/// Bounded decoded frame queue (`4..=8` GPUI textures ahead of the playhead).
pub(crate) const RING_CAPACITY: usize = 6;
/// Maximum downloadable VOD MP4 size (100 MiB).
pub(crate) const MAX_VIDEO_BYTES: usize = 100 * 1024 * 1024;
const DEFAULT_FRAME_SECS: f64 = 1.0 / 30.0;
const SEEK_END_EPSILON: f64 = 0.02;
/// How long a Play waits for the audio engine to apply it before its status is believed anyway (a stuck engine). The
/// first sound of a session opens the output device, which takes a few hundred milliseconds on some machines.
const ENGINE_ACK_GRACE: Duration = Duration::from_secs(5);
/// How far the picture's clock may stray from the sound's before it jumps to it instead of easing (a stall of the
/// speaker, a device change). Below it the gap closes by [`AV_GAIN`] at each look, so the steps in which the sound
/// engine reports its playhead do not reach the picture.
const AV_SNAP_SECS: f64 = 0.15;
/// The share of the gap to the sound's clock closed at each frame.
const AV_GAIN: f64 = 0.08;
/// How long the sound's last reported playhead is carried forward on the wall clock: longer than the engine's
/// reports are apart (its loop wakes every 5 ms, the speaker takes 10 ms blocks), short enough that the picture holds
/// soon after the sound when the speaker stalls.
const AV_CARRY_SECS: f64 = 0.05;
/// A frame counts as due this much before its time, for the rounding of 100 ns timestamps.
const PICK_EPSILON: f64 = 0.000_5;
/// Late frames the decoder may skip in a row before it hands one over anyway, so an overloaded machine still sees
/// the picture move.
const MAX_SKIPPED_IN_A_ROW: u32 = 4;

/// The clock the picture follows while the sound plays, as a browser's media element follows its audio output: the
/// sound's last reported playhead is carried forward on the wall clock for a little while ([`AV_CARRY_SECS`]), and the
/// picture's clock eases toward that. It never runs backwards unless the gap is too large to ease
/// ([`AV_SNAP_SECS`]), and it stops soon after the sound does.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AvClock {
    media: f64,
    wall: f64,
    /// The last playhead the sound reported, and when it changed.
    audio: f64,
    audio_at: f64,
    live: bool,
}

impl AvClock {
    /// Forgets the last reading: the next one is taken as it is (after a play, a pause or a seek).
    pub(crate) fn reset(&mut self) {
        self.live = false;
    }

    /// The position to show at `wall` seconds, given the sound engine's playhead `audio` read then.
    pub(crate) fn follow(&mut self, wall: f64, audio: f64) -> f64 {
        if !self.live || audio != self.audio {
            self.audio = audio;
            self.audio_at = wall;
        }
        if !self.live {
            self.live = true;
            self.media = audio;
            self.wall = wall;
            return audio;
        }
        let sound = self.audio + (wall - self.audio_at).clamp(0.0, AV_CARRY_SECS);
        let free = self.media + (wall - self.wall).max(0.0);
        let error = sound - free;
        let next = if error.abs() > AV_SNAP_SECS {
            sound
        } else {
            // Never ahead of where the sound could be by now, and never back.
            (free + error * AV_GAIN)
                .min(self.audio + AV_CARRY_SECS)
                .max(self.media)
        };
        self.media = next;
        self.wall = wall;
        next
    }
}

/// How many frames at the front of the ring (presentation order) are due at `clock`. The last of them is the one to
/// show; the ones before it came too late and are dropped, as a browser drops late video frames.
pub(crate) fn frames_due(ring: impl IntoIterator<Item = f64>, clock: f64) -> usize {
    ring.into_iter()
        .take_while(|pts| *pts <= clock + PICK_EPSILON)
        .count()
}

/// Whether a decoded frame shown from `pts` for `duration` seconds is already over at `clock` and will never be seen.
pub(crate) fn frame_is_late(pts: f64, duration: f64, clock: f64) -> bool {
    pts + duration.max(0.001) < clock
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoadStatus {
    Idle,
    Loading,
    Ready,
    Failed(String),
}

/// The file facts module 338659 `getStats` reports: the codec information and the file's size.
#[derive(Debug)]
pub(crate) struct MediaInfo {
    pub codec: crate::mp4_info::CodecInfo,
    pub file_size: usize,
}

#[derive(Clone)]
pub(crate) struct DecodedFrame {
    pub pts_secs: f64,
    pub duration_secs: f64,
    pub image: Arc<RenderImage>,
}

pub(crate) struct TickResult {
    pub frame: Option<Arc<RenderImage>>,
    pub retired: Vec<Arc<RenderImage>>,
    pub status: LoadStatus,
    pub dimensions: Option<(u32, u32)>,
    pub duration: Option<f64>,
    #[cfg(test)]
    pub has_audio: bool,
    pub playing: bool,
    pub ended: bool,
    pub position: f64,
    pub buffered: bool,
    pub needs_redraw: bool,
}

struct WorkerShared {
    status: LoadStatus,
    preloaded: Option<Arc<Vec<u8>>>,
    dimensions: Option<(u32, u32)>,
    duration_secs: Option<f64>,
    audio: Option<Arc<Clip>>,
    /// What "Stats for Nerds" shows of the file, read once its bytes are in.
    media: Option<Arc<MediaInfo>>,
    /// Frames decoded into the ring (`getVideoPlaybackQuality().totalVideoFrames`).
    decoded_frames: u64,
    /// Frames decoded too late to be shown (`droppedVideoFrames`).
    dropped_frames: u64,
    /// The playhead while the video plays: frames that end before it are not converted for the screen.
    late_before: Option<f64>,
    loaded_at: Option<Instant>,
    ring: VecDeque<DecodedFrame>,
    retired: Vec<Arc<RenderImage>>,
    pending_seek_secs: Option<f64>,
    seek_generation: u64,
    eof_decoded: bool,
    shutdown: bool,
    #[cfg(feature = "capture")]
    forced_clock_secs: Option<f64>,
}

impl WorkerShared {
    fn new(preloaded: Option<Arc<Vec<u8>>>) -> Self {
        Self {
            status: LoadStatus::Idle,
            preloaded,
            dimensions: None,
            duration_secs: None,
            audio: None,
            media: None,
            decoded_frames: 0,
            dropped_frames: 0,
            late_before: None,
            loaded_at: None,
            ring: VecDeque::with_capacity(RING_CAPACITY),
            retired: Vec::new(),
            pending_seek_secs: None,
            seek_generation: 0,
            eof_decoded: false,
            shutdown: false,
            #[cfg(feature = "capture")]
            forced_clock_secs: None,
        }
    }

    fn clear_ring(&mut self) {
        while let Some(frame) = self.ring.pop_front() {
            self.retired.push(frame.image);
        }
        self.eof_decoded = false;
    }
}

struct SyncState {
    shared: Mutex<WorkerShared>,
    wake: Condvar,
}

/// Manages one video stream: loading, off-thread H.264 decode ring, audio sync, and accurate keyframe seeking.
pub(crate) struct VodPlayer {
    key: String,
    url: String,
    sync: Arc<SyncState>,
    started: bool,
    playing: bool,
    ended: bool,
    position_secs: f64,
    volume: f32,
    muted: bool,
    clock_anchor: Option<Instant>,
    clock_base_secs: f64,
    /// The number and time of the last play this video asked of the shared audio engine, until the engine's status
    /// shows it applied ([`voice_player::Status::play_seq`]). The status is published after the engine's next
    /// render, which can take long when the output device opens for the first sound; until then it describes what
    /// came before (another clip, or this one ended) and must not undo the Play click.
    engine_asked: Option<(u64, Instant)>,
    active_seek_generation: u64,
    current_frame: Option<DecodedFrame>,
    /// The frame shown is from before the last seek: it stays until the first one after it is decoded, as a seeking
    /// `<video>` keeps its last picture.
    frame_stale: bool,
    /// The picture's clock while the sound plays.
    av: AvClock,
    epoch: Instant,
    worker: Option<std::thread::JoinHandle<()>>,
    #[cfg(test)]
    test_engine: Option<Arc<voice_player::Engine>>,
    #[cfg(test)]
    last_applied_output: Option<(f32, bool)>,
}

impl VodPlayer {
    pub(crate) fn new(
        key: impl Into<String>,
        url: impl Into<String>,
        preloaded: Option<Arc<Vec<u8>>>,
    ) -> Self {
        Self {
            key: key.into(),
            url: url.into(),
            sync: Arc::new(SyncState {
                shared: Mutex::new(WorkerShared::new(preloaded)),
                wake: Condvar::new(),
            }),
            started: false,
            playing: false,
            ended: false,
            position_secs: 0.0,
            volume: 1.0,
            muted: false,
            clock_anchor: None,
            clock_base_secs: 0.0,
            engine_asked: None,
            active_seek_generation: 0,
            current_frame: None,
            frame_stale: false,
            av: AvClock::default(),
            epoch: Instant::now(),
            worker: None,
            #[cfg(test)]
            test_engine: None,
            #[cfg(test)]
            last_applied_output: None,
        }
    }

    fn start_audio_clip(&mut self, clip: Arc<Clip>, from: f64) {
        let (volume, muted) = (self.volume, self.muted);
        #[cfg(test)]
        {
            self.last_applied_output = Some((volume, muted));
        }
        let key = self.key.clone();
        let engine = self.audio_engine();
        engine.set_speed(1.0);
        engine.set_volume(volume);
        engine.set_muted(muted);
        let seq = engine.play(&key, clip, from);
        self.engine_asked = Some((seq, Instant::now()));
    }

    fn audio_engine(&self) -> &voice_player::Engine {
        #[cfg(test)]
        {
            if let Some(engine) = &self.test_engine {
                return engine;
            }
            voice_player::set_silent(true);
        }
        voice_player::engine()
    }

    /// Replaces any pending source bytes (e.g. offline demo fixtures) before or while starting.
    #[cfg(any(test, feature = "capture"))]
    pub(crate) fn preload_bytes(&mut self, bytes: Arc<Vec<u8>>) {
        if let Ok(mut shared) = self.sync.shared.lock() {
            shared.preloaded = Some(bytes);
            if matches!(shared.status, LoadStatus::Failed(_)) {
                shared.status = LoadStatus::Idle;
                self.started = false;
            }
        }
        self.sync.wake.notify_all();
    }

    /// Spawns the decode thread if not yet started (retries cleanly if a previous load failed).
    pub(crate) fn start_load(&mut self) {
        if self.started {
            return;
        }
        if let Ok(mut shared) = self.sync.shared.lock() {
            shared.status = LoadStatus::Loading;
            shared.shutdown = false;
        }
        self.started = true;
        let sync = self.sync.clone();
        let url = self.url.clone();
        let key = self.key.clone();
        match std::thread::Builder::new()
            .name("dc-video-vod".to_owned())
            .spawn(move || worker_main(key, url, sync))
        {
            Ok(handle) => self.worker = Some(handle),
            Err(error) => {
                if let Ok(mut shared) = self.sync.shared.lock() {
                    shared.status = LoadStatus::Failed(format!("worker spawn failed: {error}"));
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn attach_audio_for_test(&mut self, clip: Arc<Clip>) {
        if self.test_engine.is_none() {
            self.test_engine = Some(Arc::new(voice_player::Engine::start(true)));
        }
        if let Ok(mut shared) = self.sync.shared.lock() {
            let clip_secs = clip.seconds();
            shared.audio = Some(clip);
            shared.duration_secs = Some(shared.duration_secs.unwrap_or(0.0).max(clip_secs));
        }
    }

    #[cfg(feature = "capture")]
    pub(crate) fn force_clock_for_test(&mut self, secs: Option<f64>) {
        if let Ok(mut shared) = self.sync.shared.lock() {
            shared.forced_clock_secs = secs;
        }
        if let Some(secs) = secs {
            self.position_secs = secs;
            self.clock_base_secs = secs;
            self.clock_anchor = Some(Instant::now());
        }
    }

    /// The file facts of "Stats for Nerds", once the file is read.
    pub(crate) fn media_info(&self) -> Option<Arc<MediaInfo>> {
        self.sync
            .shared
            .lock()
            .ok()
            .and_then(|shared| shared.media.clone())
    }

    /// The frames decoded so far.
    pub(crate) fn decoded_frames(&self) -> u64 {
        self.sync
            .shared
            .lock()
            .map_or(0, |shared| shared.decoded_frames)
    }

    /// The position the last seek or tick set.
    #[cfg(test)]
    pub(crate) fn tick_position(&self) -> f64 {
        self.position_secs
    }

    /// The frames that came too late to be shown.
    pub(crate) fn dropped_frames(&self) -> u64 {
        self.sync
            .shared
            .lock()
            .map_or(0, |shared| shared.dropped_frames)
    }

    /// Whether a newer frame than the one shown is due now, or a seek waits: the page needs drawing again. Asked at
    /// every display refresh while the video plays, so it only reads.
    pub(crate) fn frame_due(&self) -> bool {
        if !self.playing {
            return false;
        }
        let clock = match self.clock_anchor {
            Some(anchor) => self.clock_base_secs + anchor.elapsed().as_secs_f64(),
            None => self.position_secs,
        };
        self.sync.shared.lock().is_ok_and(|shared| {
            shared.pending_seek_secs.is_some()
                || shared
                    .ring
                    .front()
                    .is_some_and(|frame| frame.pts_secs <= clock + PICK_EPSILON)
        })
    }

    pub(crate) fn playing(&self) -> bool {
        self.playing
    }

    pub(crate) fn ended(&self) -> bool {
        self.ended
    }

    pub(crate) fn play(&mut self, volume: f32, muted: bool) {
        self.volume = volume.clamp(0.0, 1.0);
        self.muted = muted;
        self.start_load();
        let (audio, duration) = self
            .sync
            .shared
            .lock()
            .map(|shared| (shared.audio.clone(), shared.duration_secs))
            .unwrap_or((None, None));
        let restart = self.ended
            || duration
                .is_some_and(|dur| dur > 0.0 && self.position_secs >= dur - SEEK_END_EPSILON);
        if restart {
            self.seek(0.0);
        }
        self.ended = false;
        self.playing = true;
        self.clock_base_secs = self.position_secs;
        self.clock_anchor = Some(Instant::now());
        self.av.reset();
        if let Some(clip) = audio {
            self.start_audio_clip(clip, self.position_secs);
        } else {
            // Pause any active audio/voice clip when a silent video starts so only one media plays.
            let (vol, is_muted) = (self.volume, self.muted);
            let engine = self.audio_engine();
            engine.pause();
            engine.set_volume(vol);
            engine.set_muted(is_muted);
        }
        self.sync.wake.notify_all();
    }

    pub(crate) fn pause(&mut self) {
        if !self.playing {
            return;
        }
        self.sync_clock_before_stop();
        self.playing = false;
        self.clock_anchor = None;
        self.av.reset();
        if let Ok(mut shared) = self.sync.shared.lock() {
            shared.late_before = None;
        }
        // A play the engine has not applied yet is ours to cancel even though its status still names another clip.
        let pending = self.engine_asked.take().is_some();
        let has_audio = self
            .sync
            .shared
            .lock()
            .is_ok_and(|shared| shared.audio.is_some());
        if has_audio {
            let engine = self.audio_engine();
            if pending || engine.status().key.as_deref() == Some(&self.key) {
                engine.pause();
            }
        }
        self.sync.wake.notify_all();
    }

    pub(crate) fn set_audio_output(&mut self, volume: f32, muted: bool) {
        self.volume = volume.clamp(0.0, 1.0);
        self.muted = muted;
        let has_audio = self
            .sync
            .shared
            .lock()
            .is_ok_and(|shared| shared.audio.is_some());
        if !has_audio {
            return;
        }
        let (vol, is_muted) = (self.volume, self.muted);
        let key = self.key.clone();
        let active_key = self.audio_engine().status().key;
        if active_key.as_deref() == Some(key.as_str())
            || (self.playing && active_key.is_none())
            || (self.playing && self.engine_asked.is_some())
        {
            let engine = self.audio_engine();
            engine.set_volume(vol);
            engine.set_muted(is_muted);
            #[cfg(test)]
            {
                self.last_applied_output = Some((vol, is_muted));
            }
        }
    }

    /// Seeks cleanly to `target_secs`, dropping stale ring textures via `retired`, resetting the H.264 decoder to
    /// the nearest preceding `stss` keyframe, and aligning audio if loaded.
    pub(crate) fn seek(&mut self, target_secs: f64) {
        self.start_load();
        let (audio, duration) = self
            .sync
            .shared
            .lock()
            .map(|shared| (shared.audio.clone(), shared.duration_secs))
            .unwrap_or((None, None));
        let clamped = match duration {
            Some(dur) if dur > 0.0 => target_secs.clamp(0.0, dur),
            _ => target_secs.max(0.0),
        };
        self.position_secs = clamped;
        self.clock_base_secs = clamped;
        self.clock_anchor = self.playing.then(Instant::now);
        self.ended = duration.is_some_and(|dur| dur > 0.0 && clamped >= dur - SEEK_END_EPSILON);
        if self.ended && self.playing {
            self.playing = false;
            self.clock_anchor = None;
            // A play the engine has not applied yet would start the sound under the ended picture.
            if self.engine_asked.take().is_some() {
                self.audio_engine().pause();
            }
        }
        self.av.reset();
        if let Ok(mut shared) = self.sync.shared.lock() {
            shared.seek_generation = shared.seek_generation.wrapping_add(1);
            self.active_seek_generation = shared.seek_generation;
            shared.pending_seek_secs = Some(clamped);
            shared.late_before = None;
            shared.clear_ring();
        }
        // The picture from before the seek stays until the first one after it is decoded.
        self.frame_stale = self.current_frame.is_some();
        if let Some(clip) = audio {
            let status = self.audio_engine().status();
            if self.playing && self.engine_asked.is_some() {
                // The play has not reached the engine yet: it is asked again from the new position.
                self.start_audio_clip(clip, clamped);
            } else if status.key.as_deref() == Some(&self.key) {
                if self.playing {
                    // A play from the new position rather than a seek: the engine answers a play with its number, so
                    // the picture waits at the new position until the sound is there instead of following the old
                    // playhead the engine reports until it reads the seek.
                    self.start_audio_clip(clip, clamped);
                } else {
                    let engine = self.audio_engine();
                    engine.pause();
                    engine.seek(clamped);
                }
            }
        }
        self.sync.wake.notify_all();
    }

    fn sync_clock_before_stop(&mut self) {
        let (audio, duration, forced) = self
            .sync
            .shared
            .lock()
            .map(|shared| {
                (
                    shared.audio.is_some(),
                    shared.duration_secs,
                    #[cfg(feature = "capture")]
                    shared.forced_clock_secs,
                    #[cfg(not(feature = "capture"))]
                    None::<f64>,
                )
            })
            .unwrap_or((false, None, None));
        if let Some(secs) = forced {
            self.position_secs = secs;
            return;
        }
        if audio {
            let status = self.audio_engine().status();
            if status.key.as_deref() == Some(&self.key) {
                self.position_secs = status.position.max(0.0);
                if let Some(dur) = duration {
                    self.position_secs = self.position_secs.min(dur);
                }
                return;
            }
        }
        if let Some(anchor) = self.clock_anchor {
            self.position_secs = self.clock_base_secs + anchor.elapsed().as_secs_f64();
            if let Some(dur) = duration {
                self.position_secs = self.position_secs.min(dur);
            }
        }
    }

    /// Advances playback state for the current UI frame, pulling ready textures and returning retired GPU images for
    /// `cx.drop_image(...)`.
    pub(crate) fn tick(&mut self) -> TickResult {
        let mut retired = Vec::new();
        let (
            status,
            dimensions,
            duration,
            has_audio,
            audio_clip,
            loaded_at,
            seek_gen,
            pending_seek,
            forced_clock,
        ) = match self.sync.shared.lock() {
            Ok(mut shared) => {
                retired.append(&mut shared.retired);
                let forced = {
                    #[cfg(feature = "capture")]
                    {
                        shared.forced_clock_secs
                    }
                    #[cfg(not(feature = "capture"))]
                    {
                        None::<f64>
                    }
                };
                (
                    shared.status.clone(),
                    shared.dimensions,
                    shared.duration_secs,
                    shared.audio.is_some(),
                    shared.audio.clone(),
                    shared.loaded_at,
                    shared.seek_generation,
                    shared.pending_seek_secs.is_some(),
                    forced,
                )
            }
            Err(_) => {
                return TickResult {
                    frame: self.current_frame.as_ref().map(|f| f.image.clone()),
                    retired,
                    status: LoadStatus::Failed("state lock poisoned".to_owned()),
                    dimensions: None,
                    duration: None,
                    #[cfg(test)]
                    has_audio: false,
                    playing: false,
                    ended: false,
                    position: self.position_secs,
                    buffered: false,
                    needs_redraw: false,
                };
            }
        };

        // Start delayed audio playback once the worker finishes loading while waiting in playing state.
        if self.playing && has_audio {
            let engine_status = self.audio_engine().status();
            let waiting_for_engine = match self.engine_asked {
                Some((seq, _)) if engine_status.play_seq >= seq => {
                    self.engine_asked = None;
                    false
                }
                Some((_, asked)) => asked.elapsed() < ENGINE_ACK_GRACE,
                None => false,
            };
            if waiting_for_engine {
                // The sound has not started yet: the picture waits with it (a browser's `play()` before data).
                self.clock_base_secs = self.position_secs;
                self.clock_anchor = Some(Instant::now());
                self.av.reset();
            } else if engine_status.key.as_deref() == Some(&self.key) {
                if !engine_status.playing && !engine_status.ended {
                    if engine_status.error.is_some() {
                        if let Some(anchor) = self.clock_anchor {
                            self.position_secs =
                                self.clock_base_secs + anchor.elapsed().as_secs_f64();
                        } else {
                            self.clock_anchor = Some(Instant::now());
                        }
                    } else {
                        self.playing = false;
                        self.clock_anchor = None;
                    }
                } else if engine_status.ended {
                    let clip_end = audio_clip.as_ref().map_or(0.0, |clip| clip.seconds());
                    if duration.is_none_or(|dur| dur <= clip_end + SEEK_END_EPSILON) {
                        self.position_secs = duration.unwrap_or(clip_end);
                        self.playing = false;
                        self.ended = true;
                        self.clock_anchor = None;
                    } else if self.clock_anchor.is_none() {
                        self.clock_base_secs = clip_end;
                        self.clock_anchor = Some(Instant::now());
                    } else if let Some(anchor) = self.clock_anchor {
                        self.position_secs = self.clock_base_secs + anchor.elapsed().as_secs_f64();
                    }
                } else {
                    // The sound is the master clock; the picture eases toward its reports.
                    let wall = self.epoch.elapsed().as_secs_f64();
                    self.position_secs = self.av.follow(wall, engine_status.position.max(0.0));
                    self.clock_base_secs = self.position_secs;
                    self.clock_anchor = Some(Instant::now());
                }
            } else if engine_status.key.is_some() && engine_status.playing {
                // Another voice message, audio card, or video took exclusive playback.
                self.playing = false;
                self.clock_anchor = None;
            } else if let Some(clip) = audio_clip {
                // Worker just finished loading audio after user clicked Play.
                self.start_audio_clip(clip, self.position_secs);
            }
        } else if self.playing {
            if !matches!(status, LoadStatus::Ready) {
                // Still loading: whether there is sound is not known yet, and nothing plays, so time stands still
                // until the media is ready instead of skipping its start.
                self.clock_base_secs = self.position_secs;
                self.clock_anchor = Some(Instant::now());
            } else if let Some(anchor) = self.clock_anchor {
                self.position_secs = self.clock_base_secs + anchor.elapsed().as_secs_f64();
            } else {
                self.clock_anchor = Some(Instant::now());
            }
        }

        if let Some(forced) = forced_clock {
            self.position_secs = forced;
        }

        if let Some(dur) = duration
            && dur > 0.0
            && self.position_secs >= dur
        {
            self.position_secs = dur;
            if self.playing {
                self.playing = false;
                self.ended = true;
                self.clock_anchor = None;
            }
        }

        let target = self.position_secs;
        let mut wake_worker = false;
        if seek_gen != self.active_seek_generation {
            self.active_seek_generation = seek_gen;
            if let Some(old) = self.current_frame.take() {
                retired.push(old.image);
            }
        }

        if !pending_seek && let Ok(mut shared) = self.sync.shared.lock() {
            // Advance displayed frame only once `seek_generation` matches the current seek.
            if shared.seek_generation == self.active_seek_generation {
                // The newest frame whose time has come; the ones before it were not shown in time.
                let due = frames_due(shared.ring.iter().map(|frame| frame.pts_secs), target);
                let mut newest: Option<DecodedFrame> = None;
                for _ in 0..due {
                    if let Some(frame) = shared.ring.pop_front()
                        && let Some(skipped) = newest.replace(frame)
                    {
                        retired.push(skipped.image);
                        if self.playing {
                            shared.dropped_frames += 1;
                        }
                    }
                }
                if let Some(frame) = newest {
                    if let Some(old) = self.current_frame.replace(frame) {
                        retired.push(old.image);
                    }
                    self.frame_stale = false;
                }
                if (self.current_frame.is_none() || self.frame_stale)
                    && let Some(first) = shared.ring.front()
                    && (first.pts_secs <= target + 0.05 || !self.playing)
                    && let Some(frame) = shared.ring.pop_front()
                {
                    if let Some(old) = self.current_frame.replace(frame) {
                        retired.push(old.image);
                    }
                    self.frame_stale = false;
                }
                shared.late_before = self.playing.then_some(target);
                if shared.eof_decoded
                    && shared.ring.is_empty()
                    && self.playing
                    && !self.frame_stale
                    && let Some(cur) = &self.current_frame
                    && target >= cur.pts_secs + cur.duration_secs.max(DEFAULT_FRAME_SECS)
                {
                    self.playing = false;
                    self.ended = true;
                    self.clock_anchor = None;
                    if let Some(dur) = shared.duration_secs {
                        self.position_secs = dur;
                    }
                }
                wake_worker = !shared.eof_decoded && shared.ring.len() < RING_CAPACITY;
            }
        }
        if wake_worker {
            self.sync.wake.notify_all();
        }

        let buffered = loaded_at.is_some_and(|at| {
            duration.is_some_and(|d| d >= 1.0) && at.elapsed() >= Duration::from_millis(400)
        });
        let needs_redraw = self.playing
            || matches!(status, LoadStatus::Loading)
            || (self.started
                && self.current_frame.is_none()
                && matches!(status, LoadStatus::Ready))
            || pending_seek;

        TickResult {
            frame: self.current_frame.as_ref().map(|f| f.image.clone()),
            retired,
            status,
            dimensions,
            duration,
            #[cfg(test)]
            has_audio,
            playing: self.playing,
            ended: self.ended,
            position: self.position_secs,
            buffered,
            needs_redraw,
        }
    }

    /// Drains all GPU textures held by this player so the caller can drop them via `cx.drop_image(...)`.
    pub(crate) fn take_all_images(&mut self) -> Vec<Arc<RenderImage>> {
        let mut out = Vec::new();
        if let Some(frame) = self.current_frame.take() {
            out.push(frame.image);
        }
        if let Ok(mut shared) = self.sync.shared.lock() {
            shared.shutdown = true;
            shared.clear_ring();
            out.append(&mut shared.retired);
        }
        self.sync.wake.notify_all();
        out
    }
}

impl Drop for VodPlayer {
    fn drop(&mut self) {
        if let Ok(mut shared) = self.sync.shared.lock() {
            shared.shutdown = true;
            shared.clear_ring();
        }
        self.sync.wake.notify_all();
        if self.playing {
            let engine = self.audio_engine();
            if engine.status().key.as_deref() == Some(&self.key) {
                engine.pause();
            }
        }
    }
}

fn worker_main(key: String, url: String, sync: Arc<SyncState>) {
    let bytes = {
        let preloaded = sync
            .shared
            .lock()
            .ok()
            .and_then(|shared| shared.preloaded.clone());
        match preloaded {
            Some(bytes) => Ok(bytes),
            None => fetch_video_bytes(&url).map(Arc::new),
        }
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            if let Ok(mut shared) = sync.shared.lock() {
                shared.status = LoadStatus::Failed(error);
            }
            return;
        }
    };
    if let Ok(mut shared) = sync.shared.lock() {
        shared.media = Some(Arc::new(MediaInfo {
            codec: crate::mp4_info::read(&bytes),
            file_size: bytes.len(),
        }));
        if shared.shutdown {
            return;
        }
    }
    let track = match H264Track::read(&bytes) {
        Ok(track) => track,
        Err(error) => {
            if let Ok(mut shared) = sync.shared.lock() {
                shared.status = LoadStatus::Failed(format!("MP4 parse error ({key}): {error}"));
            }
            return;
        }
    };
    let (samples, total_100ns, reorder_depth) = match track.vod_timing() {
        Ok(timing) => timing,
        Err(error) => {
            if let Ok(mut shared) = sync.shared.lock() {
                shared.status = LoadStatus::Failed(format!("timing error ({key}): {error}"));
            }
            return;
        }
    };
    let mut decoder = match Decoder::new_vod() {
        Ok(decoder) => decoder,
        Err(error) => {
            if let Ok(mut shared) = sync.shared.lock() {
                shared.status = LoadStatus::Failed(format!("decoder init failed: {error}"));
            }
            return;
        }
    };
    decoder.set_vod_reorder(reorder_depth);
    let mut dimensions = track.dimensions();
    let video_secs = (total_100ns as f64) / 10_000_000.0;
    let audio = dc_clips::mp4::read_clip(&mut std::io::Cursor::new(bytes.as_slice()))
        .ok()
        .filter(|clip| !clip.audio.is_empty())
        .map(|clip| clip.decode_mixed_mono(None))
        .filter(|samples| !samples.is_empty())
        .map(|samples| Arc::new(Clip::from_samples(samples)))
        .or_else(|| Clip::from_file(bytes.as_ref().clone()).ok().map(Arc::new));
    let duration_secs = audio
        .as_ref()
        .map_or(video_secs, |clip| video_secs.max(clip.seconds()));

    if let Ok(mut shared) = sync.shared.lock() {
        shared.dimensions = dimensions;
        shared.duration_secs = Some(duration_secs);
        shared.audio = audio;
        shared.loaded_at = Some(Instant::now());
        shared.status = LoadStatus::Ready;
    }

    let sample_durations: Vec<f64> = {
        let mut pts_sorted: Vec<(usize, i64, i64)> = samples
            .iter()
            .map(|s| (s.index, s.pts_100ns, s.duration_100ns))
            .collect();
        pts_sorted.sort_by_key(|entry| (entry.1, entry.0));
        let mut by_idx = vec![DEFAULT_FRAME_SECS; samples.len()];
        for (index, pts, dur) in pts_sorted {
            by_idx[index] = (dur as f64 / 10_000_000.0).max(0.001);
            let _ = pts;
        }
        by_idx
    };
    let lookup_duration = |pts_100ns: i64| -> f64 {
        samples
            .iter()
            .min_by_key(|s| (s.pts_100ns - pts_100ns).abs())
            .and_then(|s| sample_durations.get(s.index).copied())
            .unwrap_or(DEFAULT_FRAME_SECS)
    };

    let mut decode_idx = 0usize;
    let mut skipped_in_a_row = 0u32;

    loop {
        let (seek_req, seek_gen, ring_len, eof_done, shutdown) = {
            let mut shared = match sync.shared.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            while !shared.shutdown
                && shared.pending_seek_secs.is_none()
                && (shared.eof_decoded || shared.ring.len() >= RING_CAPACITY)
            {
                shared = match sync.wake.wait(shared) {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
            }
            (
                shared.pending_seek_secs.take(),
                shared.seek_generation,
                shared.ring.len(),
                shared.eof_decoded,
                shared.shutdown,
            )
        };
        if shutdown {
            return;
        }
        let active_gen = seek_gen;

        if let Some(target_secs) = seek_req {
            let target_100ns = (target_secs * 10_000_000.0).round() as i64;
            let key_idx = track.keyframe_before(&samples, target_100ns);
            if let Err(error) = decoder.flush_vod() {
                warn!("VOD flush failed on {key}: {error}");
            }
            decode_idx = key_idx;
            // Decode forward from the nearest keyframe, keeping only the latest frame covering `target_100ns` plus
            // lookahead frames starting at `target_100ns`.
            // Frames before the target are only converted for the screen once the last of them is known.
            let mut pre_target: Option<(i64, Picture)> = None;
            let mut lookahead: Vec<DecodedFrame> = Vec::with_capacity(RING_CAPACITY);
            let mut seek_done = false;
            let mut aborted = false;

            while !seek_done && decode_idx <= samples.len() {
                if let Ok(shared) = sync.shared.lock()
                    && (shared.shutdown || shared.seek_generation != active_gen)
                {
                    aborted = true;
                    break;
                }
                let batch = if decode_idx < samples.len() {
                    let sample = &samples[decode_idx];
                    decode_idx += 1;
                    match track.vod_sample(sample.index) {
                        Ok(au) => decoder.decode_vod(&au, sample.pts_100ns),
                        Err(error) => {
                            debug!("VOD sample error at {}: {error}", sample.index);
                            continue;
                        }
                    }
                } else {
                    decode_idx += 1;
                    decoder.finish_vod()
                };
                let frames = match batch {
                    Ok(frames) => frames,
                    Err(error) => {
                        debug!("VOD decode error during seek: {error}");
                        continue;
                    }
                };
                for (pts_100ns, picture) in frames {
                    if dimensions.is_none() {
                        dimensions = Some((picture.width, picture.height));
                    }
                    if pts_100ns < target_100ns && lookahead.is_empty() {
                        pre_target = Some((pts_100ns, picture));
                    } else {
                        let Some(image) = picture_to_render_image(&picture) else {
                            continue;
                        };
                        lookahead.push(DecodedFrame {
                            pts_secs: (pts_100ns as f64) / 10_000_000.0,
                            duration_secs: lookup_duration(pts_100ns),
                            image,
                        });
                        if lookahead.len() >= 2 {
                            seek_done = true;
                        }
                    }
                }
                if decode_idx > samples.len() {
                    seek_done = true;
                }
            }

            let pre_target = if aborted {
                None
            } else {
                pre_target.and_then(|(pts_100ns, picture)| {
                    Some(DecodedFrame {
                        pts_secs: (pts_100ns as f64) / 10_000_000.0,
                        duration_secs: lookup_duration(pts_100ns),
                        image: picture_to_render_image(&picture)?,
                    })
                })
            };
            if let Ok(mut shared) = sync.shared.lock() {
                if aborted || shared.shutdown || shared.seek_generation != active_gen {
                    if let Some(old) = pre_target {
                        shared.retired.push(old.image);
                    }
                    for old in lookahead {
                        shared.retired.push(old.image);
                    }
                } else {
                    if shared.dimensions.is_none() {
                        shared.dimensions = dimensions;
                    }
                    if let Some(frame) = pre_target {
                        shared.ring.push_back(frame);
                        shared.decoded_frames += 1;
                    }
                    for frame in lookahead {
                        shared.ring.push_back(frame);
                        shared.decoded_frames += 1;
                    }
                    if decode_idx > samples.len() {
                        shared.eof_decoded = true;
                    }
                }
            }
            continue;
        }

        if eof_done || ring_len >= RING_CAPACITY {
            continue;
        }

        let batch = if decode_idx < samples.len() {
            let sample = &samples[decode_idx];
            decode_idx += 1;
            match track.vod_sample(sample.index) {
                Ok(au) => decoder.decode_vod(&au, sample.pts_100ns),
                Err(error) => {
                    debug!("VOD sample read error: {error}");
                    continue;
                }
            }
        } else {
            decode_idx += 1;
            decoder.finish_vod()
        };

        let frames = match batch {
            Ok(frames) => frames,
            Err(error) => {
                debug!("VOD sample decode error: {error}");
                continue;
            }
        };

        let late_before = sync
            .shared
            .lock()
            .ok()
            .and_then(|shared| shared.late_before);
        let mut converted = Vec::with_capacity(frames.len());
        let mut late = 0u64;
        for (pts_100ns, picture) in frames {
            if dimensions.is_none() {
                dimensions = Some((picture.width, picture.height));
            }
            let pts_secs = (pts_100ns as f64) / 10_000_000.0;
            let duration_secs = lookup_duration(pts_100ns);
            // A frame whose time is already over is decoded (the next ones refer to it) but not converted for the
            // screen, so a decoder that fell behind catches up with the sound instead of staying behind it.
            if skipped_in_a_row < MAX_SKIPPED_IN_A_ROW
                && late_before.is_some_and(|clock| frame_is_late(pts_secs, duration_secs, clock))
            {
                skipped_in_a_row += 1;
                late += 1;
                continue;
            }
            skipped_in_a_row = 0;
            if let Some(image) = picture_to_render_image(&picture) {
                converted.push(DecodedFrame {
                    pts_secs,
                    duration_secs,
                    image,
                });
            }
        }

        if let Ok(mut shared) = sync.shared.lock() {
            if shared.shutdown || shared.seek_generation != active_gen {
                for old in converted {
                    shared.retired.push(old.image);
                }
                continue;
            }
            if shared.dimensions.is_none() {
                shared.dimensions = dimensions;
            }
            shared.decoded_frames += late;
            shared.dropped_frames += late;
            for frame in converted {
                shared.ring.push_back(frame);
                shared.decoded_frames += 1;
            }
            if decode_idx > samples.len() {
                shared.eof_decoded = true;
            }
        }
    }
}

fn picture_to_render_image(picture: &Picture) -> Option<Arc<RenderImage>> {
    let (w, h, bgra) = if picture.width > 1920 || picture.height > 1080 {
        let (w, h, mut rgba) = dc_clips::preview::scaled_rgba_area(picture, 1920, 1080)?;
        for px in rgba.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        (w, h, rgba)
    } else {
        (picture.width, picture.height, picture.to_bgra()?)
    };
    let buffer = image::RgbaImage::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

fn fetch_video_bytes(url: &str) -> Result<Vec<u8>, String> {
    #[cfg(any(test, feature = "capture"))]
    if (cfg!(test) || crate::storage::is_capture_run())
        && let Some(path) = url.strip_prefix("file:")
    {
        let local = path.trim_start_matches("//");
        return std::fs::read(local).map_err(|error| format!("file read failed: {error}"));
    }
    if url.is_empty() {
        return Err("empty video URL".to_owned());
    }
    if !url.starts_with("https://") {
        return Err("refusing non-HTTPS video URL".to_owned());
    }
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    let runtime = RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
        .ok_or("no HTTP runtime")?;
    let url = url.to_owned();
    runtime.block_on(async move {
        let client = dc_http::plain::builder()
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if attempt.url().scheme() != "https" {
                    attempt.error("refusing non-HTTPS redirect")
                } else {
                    attempt.follow()
                }
            }))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| error.to_string())?;
        let response = client
            .get(dc_proto::replay::rewrite(&url).as_ref())
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        if let Some(len) = response.content_length()
            && len as usize > MAX_VIDEO_BYTES
        {
            return Err("video exceeds 100 MiB size limit".to_owned());
        }
        let bytes = response.bytes().await.map_err(|error| error.to_string())?;
        if bytes.len() > MAX_VIDEO_BYTES {
            return Err("video exceeds 100 MiB size limit".to_owned());
        }
        Ok(bytes.to_vec())
    })
}

/// Generates an offline multi-frame H.264 MP4 fixture with `stss` keyframes and distinct colored frames (`[R, G, B]`)
/// for unit tests and headless capture verification.
#[cfg(any(test, feature = "capture"))]
pub(crate) fn synthetic_mp4(
    width: u32,
    height: u32,
    fps: u32,
    palette: &[[u8; 3]],
    keyframe_interval: usize,
) -> Result<Vec<u8>, String> {
    use openh264::encoder::{
        BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode, UsageType,
    };
    use openh264::formats::YUVSlices;

    let (w, h) = (width as usize, height as usize);
    if w < 16 || h < 16 || !w.is_multiple_of(2) || !h.is_multiple_of(2) || palette.is_empty() {
        return Err("invalid synthetic MP4 geometry".to_owned());
    }
    let config = EncoderConfig::new()
        .bitrate(BitRate::from_bps(1_500_000))
        .max_frame_rate(FrameRate::from_hz(fps as f32))
        .rate_control_mode(RateControlMode::Bitrate)
        .intra_frame_period(IntraFramePeriod::from_num_frames(
            keyframe_interval.max(1) as u32
        ))
        .usage_type(UsageType::ScreenContentRealTime);
    let mut encoder = Encoder::with_api_config(openh264::OpenH264API::from_source(), config)
        .map_err(|e| format!("openh264 encoder: {e}"))?;

    let frame_ticks = dc_clips::mp4::VIDEO_TIMESCALE / fps.max(1);
    let mut sps_out: Option<Vec<u8>> = None;
    let mut pps_out: Option<Vec<u8>> = None;
    let mut samples = Vec::with_capacity(palette.len());

    let mut y_plane = vec![16u8; w * h];
    let mut u_plane = vec![128u8; (w / 2) * (h / 2)];
    let mut v_plane = vec![128u8; (w / 2) * (h / 2)];

    for (idx, &[r, g, b]) in palette.iter().enumerate() {
        // BT.601 limited range from RGB.
        let rf = f32::from(r);
        let gf = f32::from(g);
        let bf = f32::from(b);
        let y_val = (16.0 + 0.257 * rf + 0.504 * gf + 0.098 * bf).round() as u8;
        let u_val = (128.0 - 0.148 * rf - 0.291 * gf + 0.439 * bf).round() as u8;
        let v_val = (128.0 + 0.439 * rf - 0.368 * gf - 0.071 * bf).round() as u8;
        y_plane.fill(y_val);
        u_plane.fill(u_val);
        v_plane.fill(v_val);

        // Bright white moving progress stripe so visual frame motion is unmistakable.
        let stripe_w = (w / 6).max(8);
        let step_span = w.saturating_sub(stripe_w).max(1);
        let stripe_x = (idx * step_span) / palette.len().saturating_sub(1).max(1);
        for row in (h / 4)..((h * 3) / 4) {
            for col in stripe_x..(stripe_x + stripe_w).min(w) {
                y_plane[row * w + col] = 235;
                u_plane[(row / 2) * (w / 2) + (col / 2)] = 128;
                v_plane[(row / 2) * (w / 2) + (col / 2)] = 128;
            }
        }

        if idx % keyframe_interval.max(1) == 0 {
            encoder.force_intra_frame();
        }
        let slices = YUVSlices::new((&y_plane, &u_plane, &v_plane), (w, h), (w, w / 2, w / 2));
        let bit = encoder
            .encode(&slices)
            .map_err(|e| format!("encode frame {idx}: {e}"))?;
        let mut annex_b = Vec::new();
        bit.write_vec(&mut annex_b);
        for nal in dc_media::h264::nals(&annex_b) {
            match nal[0] & 31 {
                dc_media::h264::NAL_SPS if sps_out.is_none() => sps_out = Some(nal.to_vec()),
                dc_media::h264::NAL_PPS if pps_out.is_none() => pps_out = Some(nal.to_vec()),
                _ => {}
            }
        }
        let data = dc_clips::mp4::to_length_prefixed(&annex_b);
        let sync = dc_media::h264::is_keyframe(&annex_b);
        samples.push(dc_clips::mp4::VideoSample {
            data,
            duration: frame_ticks,
            sync,
        });
    }

    let sps = sps_out.ok_or("no SPS produced")?;
    let pps = pps_out.ok_or("no PPS produced")?;
    let info = dc_clips::sps::parse(&sps).ok_or("unreadable SPS")?;
    let video = dc_clips::mp4::VideoInput {
        sps,
        pps,
        info,
        samples,
    };
    let mut mp4 = Vec::new();
    dc_clips::mp4::write(&mut mp4, &video, &[], b"{}").map_err(|e| e.to_string())?;
    Ok(mp4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sound engine's playhead as the player reads it: the speaker callback takes 10 ms blocks, the engine's loop
    /// publishes every 5 ms, and the sound card's crystal runs `skew` fast against the wall clock.
    fn reported_audio(wall: f64, skew: f64) -> (f64, f64) {
        let heard = wall * (1.0 + skew);
        let published_at = (wall / 0.005).floor() * 0.005;
        let reported = ((published_at * (1.0 + skew)) / 0.010).floor() * 0.010;
        (heard, reported)
    }

    #[test]
    fn av_clock_smooths_the_stepped_audio_playhead_and_keeps_with_it() {
        for skew in [0.0, 0.003, -0.003] {
            let mut clock = AvClock::default();
            let mut last = f64::MIN;
            let mut worst = 0.0f64;
            // A 144 Hz screen for 20 seconds, its refreshes a little uneven.
            let mut wall = 0.0;
            let mut step = 0;
            while wall < 20.0 {
                let (heard, reported) = reported_audio(wall, skew);
                let shown = clock.follow(wall, reported);
                assert!(shown >= last, "the picture's clock went back at {wall}");
                if last > f64::MIN {
                    // No stalls and no leaps between two refreshes.
                    assert!(shown - last < 0.02, "a leap of {} at {wall}", shown - last);
                }
                if wall > 1.0 {
                    worst = worst.max((shown - heard).abs());
                }
                last = shown;
                step += 1;
                wall += if step % 3 == 0 { 0.0080 } else { 0.0065 };
            }
            // Within a frame of a 60 fps video of the sound, over twenty seconds of a skewed sound card.
            assert!(
                worst < 0.016,
                "drifted {worst} s from the sound (skew {skew})"
            );
        }
    }

    #[test]
    fn av_clock_holds_with_a_stalled_sound_jumps_with_a_moved_one_and_restarts_after_a_reset() {
        let mut clock = AvClock::default();
        assert_eq!(clock.follow(5.0, 2.0), 2.0);
        // Half a second of the speaker stalled: the reports stop while the wall clock goes on. The picture stops
        // too, a little after the sound, and does not run on or jump back and forth.
        let mut wall = 5.0;
        let mut last = 2.0;
        while wall < 5.5 {
            wall += 0.016;
            let shown = clock.follow(wall, 2.0);
            assert!(
                shown >= last && shown <= 2.0 + AV_CARRY_SECS + 1e-9,
                "{shown} at {wall}"
            );
            last = shown;
        }
        // The sound goes on from where it stopped: the picture waits for it rather than going back.
        let resumed = clock.follow(wall + 0.016, 2.01);
        assert!(resumed >= last && resumed - 2.01 < AV_CARRY_SECS);
        // The sound is somewhere else entirely (another device took over): the picture goes there at once.
        let moved = clock.follow(wall + 0.032, 7.0);
        assert!((moved - 7.0).abs() < 1e-9, "eased to {moved}");
        clock.reset();
        assert_eq!(clock.follow(9.0, 40.0), 40.0);
    }

    #[test]
    fn frame_pick_shows_the_newest_due_frame_and_drops_the_late_ones() {
        let ring = [0.0, 1.0 / 30.0, 2.0 / 30.0, 3.0 / 30.0];
        // 40 ms in, the frame of 33 ms is on screen (the one of 0 ms is over).
        assert_eq!(frames_due(ring, 0.040), 2);
        // Exactly at a frame's time it is due.
        assert_eq!(frames_due(ring, 2.0 / 30.0), 3);
        assert_eq!(frames_due(ring, -0.01), 0);
        // Behind by more than the ring: everything is due, only the last is shown.
        assert_eq!(frames_due(ring, 9.0), 4);
        assert!(frame_is_late(0.0, 1.0 / 30.0, 0.034));
        assert!(!frame_is_late(0.0, 1.0 / 30.0, 0.033));
    }

    #[test]
    fn a_30_fps_video_on_a_60_hz_screen_shows_every_frame_twice_in_step_with_the_sound() {
        let frame = 1.0 / 30.0;
        let mut clock = AvClock::default();
        let mut ring: VecDeque<f64> = (0..300).map(|index| f64::from(index) * frame).collect();
        let mut shown: Option<f64> = None;
        let mut times_shown: Vec<u32> = vec![0; 300];
        let mut wall = 0.0;
        while wall < 9.5 {
            let (_, reported) = reported_audio(wall, 0.002);
            let now = clock.follow(wall, reported);
            let due = frames_due(ring.iter().copied(), now);
            for _ in 0..due {
                shown = ring.pop_front();
            }
            let pts = shown.expect("a frame is on screen");
            // The frame on screen is the one whose time it is: never one behind, never ahead.
            assert!(
                pts <= now + PICK_EPSILON && now - pts < frame + 0.001,
                "{pts} at {now}"
            );
            times_shown[(pts / frame).round() as usize] += 1;
            wall += 1.0 / 60.0;
        }
        // Past the first second every frame is shown, and none of them more than three refreshes.
        for (index, count) in times_shown.iter().enumerate().take(270).skip(30) {
            assert!((1..=3).contains(count), "frame {index} shown {count} times");
        }
    }

    fn wait_ready(player: &mut VodPlayer) -> TickResult {
        for _ in 0..200 {
            let tick = player.tick();
            if matches!(tick.status, LoadStatus::Ready) && tick.frame.is_some() {
                return tick;
            }
            if let LoadStatus::Failed(ref err) = tick.status
                && !err.is_empty()
            {
                panic!("VOD player load failed: {:?}", tick.status);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for VOD player frame");
    }

    #[test]
    fn decodes_synthetic_mp4_seeks_to_keyframe_and_advances_silent_clock() {
        // 8 frames at 2 fps = 4.0 s total duration, keyframes every 2 frames (0, 2, 4, 6 -> 0s, 1s, 2s, 3s).
        let palette = [
            [230, 30, 30],  // t=0.0s red
            [230, 30, 30],  // t=0.5s red
            [30, 220, 30],  // t=1.0s green (keyframe 2)
            [30, 220, 30],  // t=1.5s green
            [30, 80, 235],  // t=2.0s blue (keyframe 4)
            [30, 80, 235],  // t=2.5s blue
            [235, 200, 30], // t=3.0s yellow (keyframe 6)
            [235, 200, 30], // t=3.5s yellow
        ];
        let mp4 = synthetic_mp4(64, 48, 2, &palette, 2).unwrap();
        let bytes = Arc::new(mp4);
        let mut player = VodPlayer::new(
            "video:test-silent",
            "https://example.invalid/v.mp4",
            Some(bytes),
        );
        player.start_load();
        let initial = wait_ready(&mut player);
        assert_eq!(initial.dimensions, Some((64, 48)));
        assert!((initial.duration.unwrap() - 4.0).abs() < 0.05);
        assert!(!initial.has_audio);

        // Seek to blue segment (t = 2.1 s) while paused -> decodes from stss keyframe 4 and yields a frame.
        player.seek(2.1);
        let mut seek_tick = None;
        for _ in 0..100 {
            let tick = player.tick();
            if let Some(frame) = tick.frame.as_ref() {
                assert_eq!(
                    frame.size(0),
                    gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(48))
                );
                seek_tick = Some(tick);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let seek_tick = seek_tick.expect("paused seek displays decoded frame");
        assert!((seek_tick.position - 2.1).abs() < 0.01);

        // Playing without audio advances monotonically via Instant.
        player.play(1.0, false);
        assert!(player.playing());
        std::thread::sleep(Duration::from_millis(80));
        let playing_tick = player.tick();
        assert!(playing_tick.position > 2.1);
        player.pause();
        let images = player.take_all_images();
        assert!(!images.is_empty());
    }

    #[test]
    fn slaves_video_clock_to_voice_player_when_audio_is_present() {
        voice_player::set_silent(true);
        let palette = [[200, 40, 40], [40, 200, 40], [40, 40, 200], [200, 200, 40]];
        let mp4 = Arc::new(synthetic_mp4(64, 48, 2, &palette, 2).unwrap());
        let mut player = VodPlayer::new(
            "video:test-audio",
            "https://example.invalid/a.mp4",
            Some(mp4),
        );
        player.start_load();
        let _ = wait_ready(&mut player);
        let clip = Arc::new(Clip::from_samples(vec![0.1; 48_000 * 3]));
        player.attach_audio_for_test(clip);

        player.play(0.75, false);
        let mut advanced = None;
        for _ in 0..50 {
            let tick = player.tick();
            if tick.playing && tick.position > 0.02 {
                advanced = Some(tick);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let tick = advanced.expect("audio-synced video clock advances while playing");
        assert!(tick.has_audio);
        assert!(tick.playing);
        assert!(tick.position > 0.02);

        // Seek propagates to audio engine and pauses cleanly.
        player.pause();
        player.seek(1.5);
        std::thread::sleep(Duration::from_millis(20));
        let after_seek = player.tick();
        assert!((after_seek.position - 1.5).abs() < 0.01);
        player.audio_engine().stop();
        let _ = player.take_all_images();
    }

    /// The first Play click must survive the engine publishing its status late: while it still reports the previous
    /// clip (or this one, not yet playing) the video stays playing and its clock does not run ahead of the sound.
    #[test]
    fn a_play_click_waits_for_the_engine_to_report_it() {
        let palette = [[200, 40, 40], [40, 200, 40], [40, 40, 200], [200, 200, 40]];
        let mp4 = Arc::new(synthetic_mp4(64, 48, 2, &palette, 2).unwrap());
        let mut player = VodPlayer::new(
            "video:test-first-click",
            "https://example.invalid/first.mp4",
            Some(mp4),
        );
        let (engine, reported) = voice_player::Engine::stalled(voice_player::Status {
            key: Some("audio:previous".into()),
            playing: true,
            ..Default::default()
        });
        player.test_engine = Some(Arc::new(engine));
        player.start_load();
        let _ = wait_ready(&mut player);
        player.attach_audio_for_test(Arc::new(Clip::from_samples(vec![0.1; 48_000 * 3])));

        player.play(1.0, false);
        std::thread::sleep(Duration::from_millis(40));
        let tick = player.tick();
        assert!(
            tick.playing,
            "a stale status of the previous clip does not undo the click"
        );
        assert!(
            tick.position < 0.01,
            "the picture waits for the sound: {}",
            tick.position
        );

        // A stale status of this very video having ended (a replay) does not undo it either.
        *reported.lock().unwrap() = voice_player::Status {
            key: Some("video:test-first-click".into()),
            ended: true,
            position: 2.0,
            ..Default::default()
        };
        assert!(player.tick().playing);

        // A seek before the engine applied the play asks for the play again, from the new place.
        player.seek(0.5);
        assert_eq!(player.engine_asked.map(|(seq, _)| seq), Some(2));

        // The engine applied the second play: from now on its status is the clock.
        *reported.lock().unwrap() = voice_player::Status {
            key: Some("video:test-first-click".into()),
            playing: true,
            position: 0.75,
            play_seq: 2,
            ..Default::default()
        };
        let tick = player.tick();
        assert!(tick.playing);
        assert!((tick.position - 0.75).abs() < 0.01);
        assert!(player.engine_asked.is_none());

        // Another clip takes the engine after the click was applied: this video stops, as before.
        *reported.lock().unwrap() = voice_player::Status {
            key: Some("audio:other".into()),
            playing: true,
            play_seq: 3,
            ..Default::default()
        };
        assert!(!player.tick().playing);
        let _ = player.take_all_images();
    }

    #[test]
    fn delayed_audio_load_preserves_muted_and_volume_settings() {
        voice_player::set_silent(true);
        let palette = [[200, 40, 40], [40, 200, 40], [40, 40, 200], [200, 200, 40]];
        let mp4 = Arc::new(synthetic_mp4(64, 48, 2, &palette, 2).unwrap());
        let mut player = VodPlayer::new(
            "video:test-delayed-mute",
            "https://example.invalid/delayed.mp4",
            Some(mp4),
        );
        player.start_load();
        let _ = wait_ready(&mut player);

        player.play(0.4, true);
        assert!(player.muted);
        assert!((player.volume - 0.4).abs() < 1e-6);
        player.set_audio_output(0.25, true);

        let clip = Arc::new(Clip::from_samples(vec![0.1; 48_000 * 3]));
        player.attach_audio_for_test(clip);
        let mut started = None;
        for _ in 0..50 {
            let tick = player.tick();
            if tick.playing && tick.has_audio {
                started = Some(tick);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(started.is_some(), "late-arriving audio starts under tick");
        assert_eq!(player.last_applied_output, Some((0.25, true)));

        player.set_audio_output(0.7, false);
        assert_eq!(player.last_applied_output, Some((0.7, false)));
        player.audio_engine().stop();
        let _ = player.take_all_images();
    }
}
