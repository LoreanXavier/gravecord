//! Video in a call and in Go Live streams: receive (reorder, depacketize, DAVE, decode), send (packetize, DAVE, pace,
//! retransmit) and the hub that carries decoded pictures to the app.
//!
//! The audio path never waits for any of this: decoded pictures are handed over on decoder threads, every queue is
//! bounded, and a full queue drops the picture and asks the sender for a keyframe.
pub(crate) mod depacketize;
#[cfg(test)]
mod loopback;
#[cfg(test)]
mod lossy;
pub(crate) mod playout;
pub(crate) mod receive;
pub(crate) mod reorder;
pub(crate) mod send;
pub(crate) mod sender;
pub(crate) mod sps;

#[cfg(test)]
use crate::Frame;
use crate::Stereo;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak, atomic::AtomicU32},
};

/// Frames of watched stream audio waiting for the call's clock (120 ms) at most. A stream connection pushes one every
/// 20 ms of its clock; the call mixes one per tick of its own. Older frames are dropped rather than let the stream drift
/// behind.
const STREAM_AUDIO_FRAMES: usize = 6;
/// RMS (about -50 dBFS) from which a mixed voice frame counts as someone speaking, for stream attenuation. The
/// official engine's compressor is native code; this threshold is ours (BLOCKED.md, stream attenuation).
#[cfg(test)]
const VOICE_AUDIBLE: f32 = 0.003;

/// Where a picture comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoSource {
    /// A participant's camera in the call.
    Camera,
    /// A Go Live stream of the user named by `RemoteFrame::user`; the number is that user's id too, so a watcher of
    /// several streams keeps them apart.
    Stream(u64),
}

/// One decoded picture of a remote source, or of our own camera as the local preview (`user` is then our own id).
/// The picture is NV12 or I420 as the decoder produced it, or RGBA for the local preview; see
/// [`dc_media::Picture::to_rgba`].
#[derive(Clone, Debug)]
pub struct RemoteFrame {
    pub user: u64,
    pub source: VideoSource,
    pub picture: dc_media::Picture,
}

/// Receives decoded pictures on a decoder thread. It must return quickly and never block.
pub type VideoSink = Arc<dyn Fn(RemoteFrame) + Send + Sync>;

/// The sink registration and the stream audio level, shared by a call connection and the stream connections the app
/// starts next to it. Cheap to clone.
#[derive(Clone)]
pub struct VideoHub {
    sink: Arc<Mutex<Option<VideoSink>>>,
    /// Level of each watched stream's audio in the mixer, 0.0 to 2.0, by the stream's owner (the app's per-stream
    /// volume); 1.0 for an owner without one.
    stream_volumes: Arc<Mutex<HashMap<u64, f32>>>,
    /// Watched stream audio in stereo, as each sender captured it, by the stream's owner.
    stream_audio: Arc<Mutex<HashMap<u64, StreamAudio>>>,
    /// Stream attenuation (`setSidechainCompression`): the strength, 1 to 100, or 0 while it is off.
    sidechain: Arc<AtomicU32>,
    /// Numbers the stream connections that take a queue.
    stream_connections: Arc<std::sync::atomic::AtomicU64>,
    /// Shared call/stream low-latency audio playout bus attached by the active call.
    playout: Arc<Mutex<Option<Weak<crate::playout::CallPlayout>>>>,
    /// Active Go Live stream telemetry surfaced in [`crate::Controls::rtc_stats`].
    stream_stats: crate::rtc_stats::StreamStatsMap,
    /// The call simulator's observer of delivered pictures (`sim` feature); it never changes what the sink gets.
    #[cfg(feature = "sim")]
    tap: Arc<Mutex<Option<crate::sim::Tap>>>,
}

impl Default for VideoHub {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoHub {
    pub fn new() -> Self {
        Self {
            sink: Arc::default(),
            stream_volumes: Arc::default(),
            stream_audio: Arc::default(),
            sidechain: Arc::default(),
            stream_connections: Arc::default(),
            playout: Arc::default(),
            stream_stats: crate::rtc_stats::StreamStatsMap::default(),
            #[cfg(feature = "sim")]
            tap: Arc::default(),
        }
    }

    /// The call simulator watches what this hub delivers (`sim` feature): who, which source and the size of every
    /// picture, as it goes to the sink.
    #[cfg(feature = "sim")]
    pub fn set_tap(&self, tap: Option<crate::sim::Tap>) {
        *self.tap.lock().unwrap_or_else(|e| e.into_inner()) = tap;
    }

    /// Registers the sink for every connection that shares this hub; `None` removes it.
    pub fn set_sink(&self, sink: Option<VideoSink>) {
        *self.sink.lock().unwrap_or_else(|e| e.into_inner()) = sink;
    }

    pub fn has_sink(&self) -> bool {
        self.sink
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    pub(crate) fn deliver(&self, frame: RemoteFrame) {
        #[cfg(feature = "sim")]
        if let Some(tap) = self.tap.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            tap(
                frame.user,
                frame.source,
                frame.picture.width,
                frame.picture.height,
            );
        }
        let sink = self.sink.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(sink) = sink {
            sink(frame);
        }
    }

    /// A connection that watches `owner`'s stream takes the owner's queue, empty; the number it gets tells it from
    /// a connection it replaced, whose late frames and cleanup then no longer count.
    pub(crate) fn begin_stream_audio(&self, owner: u64) -> u64 {
        let connection = self
            .stream_connections
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        self.stream_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                owner,
                StreamAudio {
                    connection,
                    ..StreamAudio::default()
                },
            );
        connection
    }

    pub(crate) fn push_stream_audio(&self, owner: u64, connection: u64, frame: Stereo) {
        if let Some(audio) = self
            .stream_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&owner)
            .filter(|audio| audio.connection == connection)
        {
            audio.push(frame);
        }
    }

    /// The next frame of watched stream audio: every watched stream at its own level, summed, and, while stream
    /// attenuation is on and `voices` (what people say in the same step) is audible, scaled by
    /// `(100 - strength) / 100` as the global attenuation maps its percent (module 31595).
    #[cfg(test)]
    pub(crate) fn pop_stream_audio(&self, voices: Option<&Frame>) -> Option<Stereo> {
        let duck = self.duck(voices);
        let volumes = self
            .stream_volumes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut out: Option<Stereo> = None;
        for (owner, audio) in self
            .stream_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter_mut()
        {
            let Some(mut frame) = audio.pop() else {
                continue;
            };
            let volume = volumes.get(owner).copied().unwrap_or(1.0) * duck;
            if volume != 1.0 {
                for sample in &mut frame {
                    *sample *= volume;
                }
            }
            match out.as_mut() {
                Some(out) => {
                    for (sum, sample) in out.iter_mut().zip(frame) {
                        *sum += sample;
                    }
                }
                None => out = Some(frame),
            }
        }
        // Two streams at twice their level may add up past full scale.
        if let Some(out) = out.as_mut() {
            for sample in out.iter_mut() {
                *sample = sample.clamp(-1.0, 1.0);
            }
        }
        out
    }

    /// The gain stream attenuation asks for while `voices` speak: 1.0 when it is off or nobody is audible.
    #[cfg(test)]
    fn duck(&self, voices: Option<&Frame>) -> f32 {
        let strength = self.sidechain_strength();
        if strength == 0 {
            return 1.0;
        }
        let audible = voices.is_some_and(|frame| {
            !frame.is_empty()
                && (frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32)
                    .sqrt()
                    > VOICE_AUDIBLE
        });
        if audible {
            f32::from(100 - strength) / 100.0
        } else {
            1.0
        }
    }

    /// Stream attenuation: `strength` 1 to 100 turns it on, 0 off.
    pub fn set_sidechain_strength(&self, strength: u8) {
        self.sidechain.store(
            u32::from(strength.min(100)),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    pub fn sidechain_strength(&self) -> u8 {
        self.sidechain.load(std::sync::atomic::Ordering::Relaxed) as u8
    }

    /// Every watched stream's waiting audio goes (the call is deafened or stalled).
    pub(crate) fn clear_stream_audio(&self) {
        for audio in self
            .stream_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values_mut()
        {
            audio.clear();
        }
    }

    /// The waiting audio of `owner`'s stream goes.
    pub(crate) fn clear_stream_audio_of(&self, owner: u64) {
        if let Some(audio) = self
            .stream_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&owner)
        {
            audio.clear();
        }
    }

    /// `owner`'s stream is no longer watched: its queue and counts go.
    pub(crate) fn forget_stream_audio(&self, owner: u64, connection: u64) {
        let mut all = self.stream_audio.lock().unwrap_or_else(|e| e.into_inner());
        if all
            .get(&owner)
            .is_some_and(|audio| audio.connection == connection)
        {
            all.remove(&owner);
        }
    }

    /// Times `owner`'s stream audio ran dry while playing, and frames dropped to keep it from lagging, for the log.
    pub(crate) fn stream_audio_counts(&self, owner: u64) -> (u64, u64) {
        self.stream_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&owner)
            .map_or((0, 0), |audio| (audio.underruns, audio.trimmed))
    }

    /// The level of `owner`'s stream audio, relative to the 1.0 default.
    pub fn set_stream_volume(&self, owner: u64, volume: f32) {
        let clamped = volume.clamp(0.0, 2.0);
        self.stream_volumes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(owner, clamped);
        if let Some(playout) = self.playout() {
            playout.set_stream_volume(owner, clamped);
        }
    }

    pub fn stream_volume(&self, owner: u64) -> f32 {
        self.stream_volumes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&owner)
            .copied()
            .unwrap_or(1.0)
    }

    pub(crate) fn sidechain_arc(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.sidechain)
    }

    pub(crate) fn attach_playout(&self, playout: &Arc<crate::playout::CallPlayout>) {
        *self.playout.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::downgrade(playout));
        let volumes: Vec<(u64, f32)> = self
            .stream_volumes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(&owner, &vol)| (owner, vol))
            .collect();
        for (owner, vol) in volumes {
            playout.set_stream_volume(owner, vol);
        }
    }

    pub(crate) fn playout(&self) -> Option<Arc<crate::playout::CallPlayout>> {
        self.playout
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(Weak::upgrade)
    }

    pub(crate) fn stream_stats(&self) -> &crate::rtc_stats::StreamStatsMap {
        &self.stream_stats
    }
}

/// Returns true if an Annex-B H.264 access unit contains an IDR slice NAL unit (type 5).
pub(crate) fn h264_has_idr(frame: &[u8]) -> bool {
    let mut i = 0;
    while i + 3 < frame.len() {
        let nal_offset = if i + 4 < frame.len() && frame[i..i + 4] == [0, 0, 0, 1] {
            Some(i + 4)
        } else if frame[i..i + 3] == [0, 0, 1] {
            Some(i + 3)
        } else {
            None
        };
        if let Some(pos) = nal_offset {
            if let Some(&hdr) = frame.get(pos)
                && (hdr & 0x1f) == 5
            {
                return true;
            }
            i = pos;
        } else {
            i += 1;
        }
    }
    false
}

/// Watched stream audio between the stream connection's clock and the call's. Both tick every 20 ms but neither
/// waits for the other; the stream side moves one frame per 20 ms of real time even across a late tick
/// (`clock::FrameClock`), so the queue only absorbs the phase and the jitter between the two
/// (`stream_audio_across_two_clocks_neither_drops_out_nor_lags`).
#[derive(Default)]
pub(crate) struct StreamAudio {
    /// The stream connection that fills this queue (`VideoHub::begin_stream_audio`).
    connection: u64,
    queue: VecDeque<Stereo>,
    playing: bool,
    /// Times the call found no frame while the stream was playing, and frames dropped at the cap.
    pub underruns: u64,
    pub trimmed: u64,
}

impl StreamAudio {
    pub(crate) fn push(&mut self, frame: Stereo) {
        if self.queue.len() >= STREAM_AUDIO_FRAMES {
            self.queue.pop_front();
            self.trimmed += 1;
        }
        self.queue.push_back(frame);
    }

    #[cfg(test)]
    pub(crate) fn pop(&mut self) -> Option<Stereo> {
        let frame = self.queue.pop_front();
        match frame {
            Some(_) => self.playing = true,
            None if self.playing => {
                self.playing = false;
                self.underruns += 1;
            }
            None => {}
        }
        frame
    }

    pub(crate) fn clear(&mut self) {
        self.queue.clear();
        self.playing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn the_hub_delivers_to_the_current_sink_only() {
        let hub = VideoHub::new();
        let picture = dc_media::Picture::rgba(1, 1, vec![0; 4]).unwrap();
        let frame = RemoteFrame {
            user: 1,
            source: VideoSource::Camera,
            picture,
        };
        hub.deliver(frame.clone());
        assert!(!hub.has_sink());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        hub.set_sink(Some(Arc::new(move |f| {
            assert_eq!(f.user, 1);
            seen.fetch_add(1, Ordering::Relaxed);
        })));
        hub.clone().deliver(frame.clone());
        hub.set_sink(None);
        hub.deliver(frame);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(hub.stream_volume(7), 1.0);
        let seven = hub.begin_stream_audio(7);
        for i in 0..8 {
            hub.push_stream_audio(7, seven, [i as f32; 1920]);
        }
        hub.set_stream_volume(7, 0.5);
        assert_eq!(
            hub.pop_stream_audio(None).unwrap()[0],
            1.0,
            "two oldest dropped, level applied"
        );
        hub.clear_stream_audio();
        assert!(hub.pop_stream_audio(None).is_none());
        hub.set_stream_volume(7, 1.0);
        hub.set_stream_volume(7, 9.0);
        assert_eq!(hub.stream_volume(7), 2.0);
    }

    #[test]
    fn two_watched_streams_keep_their_own_level_and_queue() {
        let hub = VideoHub::new();
        hub.set_stream_volume(1, 0.5);
        hub.set_stream_volume(2, 0.0);
        let one = hub.begin_stream_audio(1);
        let two = hub.begin_stream_audio(2);
        hub.push_stream_audio(1, one, [0.4; 1920]);
        hub.push_stream_audio(2, two, [0.4; 1920]);
        assert!((hub.pop_stream_audio(None).unwrap()[0] - 0.2).abs() < 1e-6);
        // The muted one unmuted, the other untouched: both are heard, one frame each per step.
        hub.set_stream_volume(2, 1.0);
        hub.push_stream_audio(1, one, [0.4; 1920]);
        hub.push_stream_audio(2, two, [0.4; 1920]);
        assert!((hub.pop_stream_audio(None).unwrap()[0] - 0.6).abs() < 1e-6);
        assert!(hub.pop_stream_audio(None).is_none());
        hub.push_stream_audio(1, one, [0.4; 1920]);
        hub.clear_stream_audio_of(1);
        hub.forget_stream_audio(2, two);
        assert!(hub.pop_stream_audio(None).is_none());
        // A connection that replaced another keeps its queue when the old one cleans up late.
        let newer = hub.begin_stream_audio(1);
        hub.push_stream_audio(1, one, [0.9; 1920]);
        hub.forget_stream_audio(1, one);
        hub.push_stream_audio(1, newer, [0.4; 1920]);
        assert!((hub.pop_stream_audio(None).unwrap()[0] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn stream_attenuation_lowers_the_stream_only_while_voices_are_audible() {
        let hub = VideoHub::new();
        let loud: Frame = [0.1; 960];
        let quiet: Frame = [0.0; 960];
        let seven = hub.begin_stream_audio(7);
        hub.push_stream_audio(7, seven, [0.5; 1920]);
        assert_eq!(hub.pop_stream_audio(Some(&loud)).unwrap()[0], 0.5, "off");
        hub.set_sidechain_strength(40);
        assert_eq!(hub.sidechain_strength(), 40);
        hub.push_stream_audio(7, seven, [0.5; 1920]);
        assert!((hub.pop_stream_audio(Some(&loud)).unwrap()[0] - 0.3).abs() < 1e-6);
        hub.push_stream_audio(7, seven, [0.5; 1920]);
        assert_eq!(hub.pop_stream_audio(Some(&quiet)).unwrap()[0], 0.5);
        hub.push_stream_audio(7, seven, [0.5; 1920]);
        assert_eq!(hub.pop_stream_audio(None).unwrap()[0], 0.5);
    }

    /// One 20 ms clock as `clock::ticks` runs: a grid from `offset`, each wake up to `jitter` late, and now and then a
    /// stall of 25 to 60 ms after which the grid restarts from the late wake. Wake times in microseconds over `seconds`.
    fn ticks(seed: u32, offset: u64, seconds: u64, stalls_per_mille: u32) -> Vec<u64> {
        let mut random = seed.max(1);
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            random
        };
        let mut wakes = Vec::new();
        let mut due = offset + 20_000;
        while due < seconds * 1_000_000 {
            let mut wake = due + u64::from(next() % 2_000);
            if next() % 1000 < stalls_per_mille {
                wake += 25_000 + u64::from(next() % 35_000);
            }
            wakes.push(wake);
            due += 20_000;
            if wake >= due {
                due = wake + 20_000;
            }
        }
        wakes
    }

    #[derive(Debug, Default)]
    struct Heard {
        dropouts: u64,
        mean_ms: f64,
        worst_ms: u64,
    }

    /// The stream loop on one clock pushes frames, the call loop on another pops one per tick. `catch_up` pushes what
    /// the stream loop owes after a late tick (`FrameClock`); without it one frame per tick, as before.
    fn two_clocks(seed: u32, catch_up: bool, stalls: u32) -> Heard {
        let producer = ticks(seed, 7_000, 120, stalls);
        let consumer = ticks(seed.wrapping_mul(31).wrapping_add(17), 0, 120, stalls);
        let mut events: Vec<(u64, bool)> = producer.iter().map(|t| (*t, true)).collect();
        events.extend(consumer.iter().map(|t| (*t, false)));
        events.sort();
        let mut queue = StreamAudio::default();
        let start = producer[0];
        let mut frames = crate::clock::FrameClock::default();
        let mut heard = Heard::default();
        let mut delays = Vec::new();
        let mut playing = false;
        for (at, push) in events {
            if push {
                let owed = if catch_up { frames.owed(at - start) } else { 1 };
                for _ in 0..owed {
                    // The frame carries when it was pushed.
                    let mut frame = [0.0; 1920];
                    frame[0] = at as f32;
                    queue.push(frame);
                }
            } else {
                let frame = queue.pop();
                match frame {
                    Some(frame) => {
                        playing = true;
                        delays.push(at - frame[0] as u64);
                    }
                    None if playing => heard.dropouts += 1,
                    None => {}
                }
            }
        }
        heard.mean_ms = delays.iter().sum::<u64>() as f64 / delays.len() as f64 / 1000.0;
        heard.worst_ms = delays.iter().max().copied().unwrap_or(0) / 1000;
        heard
    }

    #[test]
    fn stream_audio_across_two_clocks_neither_drops_out_nor_lags() {
        let total = |runs: &[Heard]| runs.iter().map(|h| h.dropouts).sum::<u64>();
        let mean = |runs: &[Heard]| runs.iter().map(|h| h.mean_ms).sum::<f64>() / runs.len() as f64;
        let worst = |runs: &[Heard]| runs.iter().map(|h| h.worst_ms).max().unwrap_or(0);
        // Jitter only, then a stall of 25 to 60 ms on each clock about every 7 s.
        for stalls in [0, 3] {
            let before: Vec<Heard> = (1..=8u32)
                .map(|seed| two_clocks(seed * 7919, false, stalls))
                .collect();
            let after: Vec<Heard> = (1..=8u32)
                .map(|seed| two_clocks(seed * 7919, true, stalls))
                .collect();
            println!(
                "16 minutes, {stalls} stalls per 1000 ticks: one frame per tick {} dropouts, {:.1} ms mean added delay, {} ms worst; one frame per 20 ms of real time {} dropouts, {:.1} ms mean, {} ms worst",
                total(&before),
                mean(&before),
                worst(&before),
                total(&after),
                mean(&after),
                worst(&after)
            );
            if stalls == 0 {
                assert_eq!(total(&after), 0, "no dropouts from jitter alone");
                assert!(mean(&after) < 20.0, "only the phase between the clocks");
            } else {
                assert!(total(&after) * 2 <= total(&before), "dropouts");
                assert!(mean(&after) < 100.0, "mean added delay");
            }
        }
    }
}
