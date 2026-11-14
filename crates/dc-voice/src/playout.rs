//! Device-pulled call and watched-stream audio playout.
//!
//! Decrypted RTP packets and roster/mix control commands enter a short bounded ingress queue from `session` and
//! `stream_conn`. The speaker playback callback pulls 10 ms stereo blocks directly through [`PlayoutReader::pull_10ms`]
//! using non-blocking `try_lock`, feeds them through [`Mixer`]/[`NetEq`] with zero heap allocations, publishes active
//! remote talkers lock-free via an `AtomicU64` slot bitmask, and emits 20 ms mono reference frames on an `rtrb` ring for
//! AEC3 (`Processor::render`) and diagnostic recording.

use crate::dave::MAX_PARTICIPANTS;
use crate::mixer::{Mixer, Report, SpeakerStatsSnapshot};
use crate::neteq::{NetEq, OUTPUT_SIZE};
use crate::{Frame, Stereo};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};

const INGRESS_PACKET_CAP: usize = 256;
const INGRESS_CONTROL_CAP: usize = 128;
const RETIRED_DECODER_CAP: usize = 512;
const RENDER_TAP_RING: usize = 32;
const VOICE_AUDIBLE: f32 = 0.002;
const STATS_PUBLISH_INTERVAL_BLOCKS: u64 = 20;

struct PreparedDecoder(Box<NetEq>);

impl std::fmt::Debug for PreparedDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreparedDecoder(..)")
    }
}

/// One Opus RTP payload buffered briefly between network decrypt and the playback pull callback.
pub(crate) struct IngressPacket {
    pub stream_owner: Option<u64>,
    pub ssrc: u32,
    pub sequence: u16,
    pub timestamp: u32,
    pub len: u16,
    pub opus: [u8; 1275],
}

/// Control messages processed before mixing each 10 ms block. Constructed off-RT so RT never allocates.
#[derive(Debug)]
enum PlayoutControl {
    AnnounceVoice {
        user: u64,
        ssrc: u32,
        decoder: PreparedDecoder,
    },
    RemoveVoice {
        user: u64,
    },
    ClearVoices,
    SetUserMix {
        user: u64,
        volume: f32,
        muted: bool,
    },
    AnnounceStream {
        owner: u64,
        ssrc: u32,
        volume: f32,
        decoder: PreparedDecoder,
    },
    RemoveStream {
        owner: u64,
    },
    ClearStream {
        owner: u64,
    },
    ClearAllStreams,
    SetStreamVolume {
        owner: u64,
        volume: f32,
    },
}

struct PlayoutIngress {
    packets: VecDeque<IngressPacket>,
    controls: VecDeque<PlayoutControl>,
    dropped_packets: u64,
}

impl PlayoutIngress {
    fn new() -> Self {
        Self {
            packets: VecDeque::with_capacity(INGRESS_PACKET_CAP),
            controls: VecDeque::with_capacity(INGRESS_CONTROL_CAP),
            dropped_packets: 0,
        }
    }

    fn push_packet(
        &mut self,
        stream_owner: Option<u64>,
        ssrc: u32,
        sequence: u16,
        timestamp: u32,
        opus: &[u8],
    ) {
        if opus.is_empty() || opus.len() > 1275 {
            return;
        }
        if self.packets.len() >= INGRESS_PACKET_CAP {
            let _ = self.packets.pop_front();
            self.dropped_packets += 1;
        }
        let mut buf = [0u8; 1275];
        buf[..opus.len()].copy_from_slice(opus);
        self.packets.push_back(IngressPacket {
            stream_owner,
            ssrc,
            sequence,
            timestamp,
            len: opus.len() as u16,
            opus: buf,
        });
    }

    fn push_control(&mut self, control: PlayoutControl) {
        if self.controls.len() >= INGRESS_CONTROL_CAP {
            let _ = self.controls.pop_front();
        }
        self.controls.push_back(control);
    }
}

/// Non-RT roster shadow kept in sync with `voice_mixer` so `session` can map SSRC -> user synchronously.
#[derive(Default)]
struct RosterShadow {
    ssrc_to_user: HashMap<u32, u64>,
    user_to_ssrc: HashMap<u64, u32>,
}

/// Non-allocating snapshot copied periodically off the playback path for telemetry and logging.
#[derive(Clone, Debug)]
pub(crate) struct PlayoutStatsCache {
    pub voice_report: Report,
    pub voice_speakers: Vec<SpeakerStatsSnapshot>,
    pub stream_speakers: Vec<SpeakerStatsSnapshot>,
    pub underruns: u64,
    pub stream_underruns: u64,
    pub dropped_ingress: u64,
}

impl Default for PlayoutStatsCache {
    fn default() -> Self {
        Self {
            voice_report: Report::default(),
            voice_speakers: Vec::with_capacity(MAX_PARTICIPANTS),
            stream_speakers: Vec::with_capacity(MAX_PARTICIPANTS),
            underruns: 0,
            stream_underruns: 0,
            dropped_ingress: 0,
        }
    }
}

struct PlayoutCore {
    voice_mixer: Mixer,
    stream_mixer: Mixer,
    garbage_tx: rtrb::Producer<Box<NetEq>>,
    scratch_packets: VecDeque<IngressPacket>,
    scratch_controls: VecDeque<PlayoutControl>,
    ingress: Arc<Mutex<PlayoutIngress>>,
    sidechain: Arc<AtomicU32>,
    speaking_mask: Arc<AtomicU64>,
    slot_occupied: Arc<AtomicU64>,
    slot_users: Arc<[AtomicU64; MAX_PARTICIPANTS]>,
    render_tap: Option<rtrb::Producer<Frame>>,
    ref_frame: Frame,
    ref_half: usize,
    loopback_rx: Option<rtrb::Consumer<Stereo>>,
    loopback_frame: Stereo,
    loopback_half: usize,
    loopback_primed: bool,
    blocks_pulled: u64,
    underruns: u64,
    stream_underruns: u64,
    last_playing: bool,
    stats_cache: Arc<Mutex<PlayoutStatsCache>>,
}

impl PlayoutCore {
    fn apply_control(&mut self, control: PlayoutControl) {
        let garbage_tx = &mut self.garbage_tx;
        let mut retire = |old: Box<NetEq>| {
            let _ = garbage_tx.push(old);
        };
        match control {
            PlayoutControl::AnnounceVoice {
                user,
                ssrc,
                decoder,
            } => {
                if let Some(old_slot) = self.voice_mixer.slot_of(user) {
                    let bit = 1u64 << old_slot;
                    self.slot_occupied.fetch_and(!bit, Ordering::Release);
                    self.speaking_mask.fetch_and(!bit, Ordering::Release);
                }
                if self
                    .voice_mixer
                    .announce_prepared(user, ssrc, decoder.0, &mut retire)
                    .is_ok()
                    && let Some(slot) = self.voice_mixer.slot_of(user)
                {
                    let idx = usize::from(slot);
                    if idx < MAX_PARTICIPANTS {
                        self.slot_users[idx].store(user, Ordering::Release);
                        self.slot_occupied.fetch_or(1u64 << slot, Ordering::Release);
                    }
                }
            }
            PlayoutControl::RemoveVoice { user } => {
                if let Some(slot) = self.voice_mixer.slot_of(user) {
                    let bit = 1u64 << slot;
                    self.speaking_mask.fetch_and(!bit, Ordering::Release);
                    self.slot_occupied.fetch_and(!bit, Ordering::Release);
                }
                self.voice_mixer.remove_with(user, retire);
            }
            PlayoutControl::ClearVoices => {
                self.voice_mixer.clear();
                self.speaking_mask.store(0, Ordering::Release);
            }
            PlayoutControl::SetUserMix {
                user,
                volume,
                muted,
            } => {
                self.voice_mixer.set_user_mix(user, volume, muted);
            }
            PlayoutControl::AnnounceStream {
                owner,
                ssrc,
                volume,
                decoder,
            } => {
                let _ = self
                    .stream_mixer
                    .announce_prepared(owner, ssrc, decoder.0, retire);
                self.stream_mixer.set_user_mix(owner, volume, false);
            }
            PlayoutControl::RemoveStream { owner } => {
                self.stream_mixer.remove_with(owner, retire);
            }
            PlayoutControl::ClearStream { owner } => {
                self.stream_mixer.clear_user(owner);
            }
            PlayoutControl::ClearAllStreams => {
                self.stream_mixer.clear();
            }
            PlayoutControl::SetStreamVolume { owner, volume } => {
                self.stream_mixer.set_user_mix(owner, volume, false);
            }
        }
    }

    fn drain_ingress_nonblocking(&mut self) -> Option<u64> {
        let mut ing = match self.ingress.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return None,
            Err(TryLockError::Poisoned(e)) => e.into_inner(),
        };
        std::mem::swap(&mut ing.controls, &mut self.scratch_controls);
        std::mem::swap(&mut ing.packets, &mut self.scratch_packets);
        let dropped = ing.dropped_packets;
        drop(ing);

        while let Some(control) = self.scratch_controls.pop_front() {
            self.apply_control(control);
        }
        for packet in self.scratch_packets.drain(..) {
            let payload = &packet.opus[..usize::from(packet.len)];
            if packet.stream_owner.is_some() {
                self.stream_mixer
                    .push(packet.ssrc, packet.sequence, packet.timestamp, payload);
            } else {
                self.voice_mixer
                    .push(packet.ssrc, packet.sequence, packet.timestamp, payload);
            }
        }
        Some(dropped)
    }

    fn publish_stats(&mut self, dropped_ingress: Option<u64>) {
        if let Ok(mut cache) = self.stats_cache.try_lock() {
            self.voice_mixer
                .snapshot_speakers(&mut cache.voice_speakers);
            self.stream_mixer
                .snapshot_speakers(&mut cache.stream_speakers);
            cache.voice_report = self.voice_mixer.report();
            cache.underruns = self.underruns;
            cache.stream_underruns = self.stream_underruns;
            if let Some(dropped) = dropped_ingress {
                cache.dropped_ingress = dropped;
            }
        }
    }

    fn push_render_tap(&mut self) {
        if self.ref_half == 0 {
            self.ref_half = 1;
        } else {
            if let Some(tap) = self.render_tap.as_mut() {
                let _ = tap.push(self.ref_frame);
            }
            self.ref_half = 0;
        }
    }

    fn mix_loopback(&mut self, out: &mut [f32; OUTPUT_SIZE * 2], loopback: bool) {
        let Some(rx) = self.loopback_rx.as_mut() else {
            return;
        };
        if !loopback {
            while rx.pop().is_ok() {}
            self.loopback_half = 0;
            self.loopback_primed = false;
            return;
        }
        if self.loopback_half == 0 {
            if !self.loopback_primed && rx.slots() >= 2 {
                self.loopback_primed = true;
            }
            self.loopback_frame = if self.loopback_primed {
                match rx.pop() {
                    Ok(frame) => frame,
                    Err(_) => {
                        self.loopback_primed = false;
                        [0.0; OUTPUT_SIZE * 4]
                    }
                }
            } else {
                [0.0; OUTPUT_SIZE * 4]
            };
            let src = &self.loopback_frame[..OUTPUT_SIZE * 2];
            for (dst, &sample) in out.iter_mut().zip(src.iter()) {
                *dst = (*dst + sample).clamp(-1.0, 1.0);
            }
            self.loopback_half = 1;
        } else {
            let src = &self.loopback_frame[OUTPUT_SIZE * 2..];
            for (dst, &sample) in out.iter_mut().zip(src.iter()) {
                *dst = (*dst + sample).clamp(-1.0, 1.0);
            }
            self.loopback_half = 0;
        }
    }

    /// Mixes the next 10 ms stereo block (`960` interleaved samples) into `out` with zero heap allocations.
    /// Returns `true` when any voice or watched stream produced unmuted audio.
    fn pull_10ms_with_loopback(
        &mut self,
        out: &mut [f32; OUTPUT_SIZE * 2],
        playing: bool,
        loopback: bool,
    ) -> bool {
        let dropped_ingress = self.drain_ingress_nonblocking();

        if !playing {
            if self.last_playing {
                self.voice_mixer.clear();
                self.stream_mixer.clear();
                self.speaking_mask.store(0, Ordering::Release);
                self.last_playing = false;
            }
            out.fill(0.0);
            self.ref_frame[self.ref_half * OUTPUT_SIZE..(self.ref_half + 1) * OUTPUT_SIZE].fill(0.0);
            self.push_render_tap();
            self.mix_loopback(out, loopback);
            self.blocks_pulled += 1;
            if self
                .blocks_pulled
                .is_multiple_of(STATS_PUBLISH_INTERVAL_BLOCKS)
            {
                self.publish_stats(dropped_ingress);
            }
            return false;
        }
        self.last_playing = true;

        let mut voices_mono = [0.0f32; OUTPUT_SIZE];
        let voice_active = self.voice_mixer.mix_10ms(&mut voices_mono);
        self.speaking_mask
            .store(self.voice_mixer.speaking_mask(), Ordering::Release);

        let mut stream_stereo = [0.0f32; OUTPUT_SIZE * 2];
        let stream_active = self.stream_mixer.mix_10ms(&mut stream_stereo);

        let duck = if voice_active {
            let strength = self.sidechain.load(Ordering::Relaxed).min(100);
            if strength == 0 {
                1.0
            } else {
                let energy: f32 = voices_mono.iter().map(|s| s * s).sum();
                if (energy / OUTPUT_SIZE as f32).sqrt() > VOICE_AUDIBLE {
                    (100 - strength) as f32 / 100.0
                } else {
                    1.0
                }
            }
        } else {
            1.0
        };

        for i in 0..OUTPUT_SIZE {
            let voice = voices_mono[i];
            let left = (voice + stream_stereo[i * 2] * duck).clamp(-1.0, 1.0);
            let right = (voice + stream_stereo[i * 2 + 1] * duck).clamp(-1.0, 1.0);
            out[i * 2] = left;
            out[i * 2 + 1] = right;
            self.ref_frame[self.ref_half * OUTPUT_SIZE + i] = (left + right) * 0.5;
        }

        let active = voice_active || stream_active;
        self.push_render_tap();
        self.mix_loopback(out, loopback);

        self.blocks_pulled += 1;
        if self
            .blocks_pulled
            .is_multiple_of(STATS_PUBLISH_INTERVAL_BLOCKS)
        {
            self.publish_stats(dropped_ingress);
        }
        active
    }
}

/// Handle passed directly into `audio::Playback` for OS callback pulls.
pub(crate) struct PlayoutReader {
    core: Arc<Mutex<PlayoutCore>>,
    contention_underruns: Arc<AtomicU64>,
}

impl PlayoutReader {
    /// Pulls 10 ms (`480` stereo pairs) non-blockingly for the speaker callback.
    #[cfg(test)]
    pub(crate) fn pull_10ms(&mut self, out: &mut [f32; OUTPUT_SIZE * 2], playing: bool) -> bool {
        self.pull_10ms_with_loopback(out, playing, false)
    }

    pub(crate) fn pull_10ms_with_loopback(
        &mut self,
        out: &mut [f32; OUTPUT_SIZE * 2],
        playing: bool,
        loopback: bool,
    ) -> bool {
        match self.core.try_lock() {
            Ok(mut core) => core.pull_10ms_with_loopback(out, playing, loopback),
            Err(TryLockError::Poisoned(e)) => {
                e.into_inner()
                    .pull_10ms_with_loopback(out, playing, loopback)
            }
            Err(TryLockError::WouldBlock) => {
                self.contention_underruns.fetch_add(1, Ordering::Relaxed);
                out.fill(0.0);
                false
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        let mut dummy = [0.0f32; OUTPUT_SIZE * 2];
        let _ = self.pull_10ms_with_loopback(&mut dummy, false, false);
    }
}

/// Shared call + watched-stream playout bus connecting network ingress to hardware pull playback.
pub(crate) struct CallPlayout {
    ingress: Arc<Mutex<PlayoutIngress>>,
    core: Arc<Mutex<PlayoutCore>>,
    roster: Mutex<RosterShadow>,
    garbage_rx: Mutex<rtrb::Consumer<Box<NetEq>>>,
    speaking_mask: Arc<AtomicU64>,
    slot_occupied: Arc<AtomicU64>,
    slot_users: Arc<[AtomicU64; MAX_PARTICIPANTS]>,
    contention_underruns: Arc<AtomicU64>,
    stats_cache: Arc<Mutex<PlayoutStatsCache>>,
}

impl CallPlayout {
    pub(crate) fn new(sidechain: Arc<AtomicU32>) -> Self {
        let ingress = Arc::new(Mutex::new(PlayoutIngress::new()));
        let (garbage_tx, garbage_rx) = rtrb::RingBuffer::new(RETIRED_DECODER_CAP);
        let speaking_mask = Arc::new(AtomicU64::new(0));
        let slot_occupied = Arc::new(AtomicU64::new(0));
        let slot_users = Arc::new(std::array::from_fn(|_| AtomicU64::new(0)));
        let stats_cache = Arc::new(Mutex::new(PlayoutStatsCache::default()));

        let mut voice_mixer = Mixer::default();
        voice_mixer.preallocate();
        let mut stream_mixer = Mixer::stereo();
        stream_mixer.preallocate();

        let core = Arc::new(Mutex::new(PlayoutCore {
            voice_mixer,
            stream_mixer,
            garbage_tx,
            scratch_packets: VecDeque::with_capacity(INGRESS_PACKET_CAP),
            scratch_controls: VecDeque::with_capacity(INGRESS_CONTROL_CAP),
            ingress: Arc::clone(&ingress),
            sidechain,
            speaking_mask: Arc::clone(&speaking_mask),
            slot_occupied: Arc::clone(&slot_occupied),
            slot_users: Arc::clone(&slot_users),
            render_tap: None,
            ref_frame: [0.0; 960],
            ref_half: 0,
            loopback_rx: None,
            loopback_frame: [0.0; 1920],
            loopback_half: 0,
            loopback_primed: false,
            blocks_pulled: 0,
            underruns: 0,
            stream_underruns: 0,
            last_playing: true,
            stats_cache: Arc::clone(&stats_cache),
        }));

        Self {
            ingress,
            core,
            roster: Mutex::new(RosterShadow::default()),
            garbage_rx: Mutex::new(garbage_rx),
            speaking_mask,
            slot_occupied,
            slot_users,
            contention_underruns: Arc::new(AtomicU64::new(0)),
            stats_cache,
        }
    }

    fn reap_garbage(&self) -> usize {
        let mut reaped = 0;
        let mut rx = self.garbage_rx.lock().unwrap_or_else(|e| e.into_inner());
        while let Ok(dead) = rx.pop() {
            drop(dead);
            reaped += 1;
        }
        reaped
    }

    pub(crate) fn user_for_ssrc(&self, ssrc: u32) -> Option<u64> {
        self.roster
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ssrc_to_user
            .get(&ssrc)
            .copied()
    }

    pub(crate) fn announce(&self, user: u64, ssrc: u32) -> Result<(), &'static str> {
        self.reap_garbage();
        let mut roster = self.roster.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&owner) = roster.ssrc_to_user.get(&ssrc)
            && owner != user
        {
            return Err("Voice SSRC belongs to another participant");
        }
        if roster.user_to_ssrc.get(&user).copied() == Some(ssrc) {
            return Ok(());
        }
        if !roster.user_to_ssrc.contains_key(&user)
            && roster.user_to_ssrc.len() >= MAX_PARTICIPANTS - 1
        {
            return Err("Voice decoder budget exceeded");
        }
        let decoder = PreparedDecoder(Mixer::new_speaker_decoder(false)?);
        if let Some(old_ssrc) = roster.user_to_ssrc.insert(user, ssrc) {
            roster.ssrc_to_user.remove(&old_ssrc);
        }
        roster.ssrc_to_user.insert(ssrc, user);
        drop(roster);

        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_control(PlayoutControl::AnnounceVoice {
                user,
                ssrc,
                decoder,
            });
        Ok(())
    }

    pub(crate) fn remove(&self, user: u64) {
        self.reap_garbage();
        let mut roster = self.roster.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ssrc) = roster.user_to_ssrc.remove(&user) {
            roster.ssrc_to_user.remove(&ssrc);
        }
        drop(roster);

        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_control(PlayoutControl::RemoveVoice { user });
    }

    pub(crate) fn clear(&self) {
        self.reap_garbage();
        self.speaking_mask.store(0, Ordering::Release);
        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_control(PlayoutControl::ClearVoices);
    }

    pub(crate) fn set_user_mix(&self, user: u64, volume: f32, muted: bool) {
        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_control(PlayoutControl::SetUserMix {
                user,
                volume,
                muted,
            });
    }

    pub(crate) fn sync_user_mix_table(&self, table: &HashMap<u64, (f32, bool)>) {
        let mut ing = self.ingress.lock().unwrap_or_else(|e| e.into_inner());
        for (&user, &(volume, muted)) in table {
            ing.push_control(PlayoutControl::SetUserMix {
                user,
                volume,
                muted,
            });
        }
    }

    pub(crate) fn push_packet(&self, ssrc: u32, sequence: u16, timestamp: u32, opus: &[u8]) {
        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_packet(None, ssrc, sequence, timestamp, opus);
    }

    pub(crate) fn announce_stream(&self, owner: u64, ssrc: u32, volume: f32) {
        self.reap_garbage();
        let Ok(neteq) = Mixer::new_speaker_decoder(true) else {
            return;
        };
        let decoder = PreparedDecoder(neteq);
        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_control(PlayoutControl::AnnounceStream {
                owner,
                ssrc,
                volume,
                decoder,
            });
    }

    pub(crate) fn remove_stream(&self, owner: u64) {
        self.reap_garbage();
        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_control(PlayoutControl::RemoveStream { owner });
    }

    pub(crate) fn clear_stream(&self, owner: u64) {
        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_control(PlayoutControl::ClearStream { owner });
    }

    pub(crate) fn clear_all_streams(&self) {
        self.reap_garbage();
        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_control(PlayoutControl::ClearAllStreams);
    }

    pub(crate) fn set_stream_volume(&self, owner: u64, volume: f32) {
        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_control(PlayoutControl::SetStreamVolume { owner, volume });
    }

    pub(crate) fn push_stream_packet(
        &self,
        owner: u64,
        ssrc: u32,
        sequence: u16,
        timestamp: u32,
        opus: &[u8],
    ) {
        self.ingress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_packet(Some(owner), ssrc, sequence, timestamp, opus);
    }

    /// Populates `out` with currently speaking user IDs read lock-free from the `AtomicU64` slot bitmask.
    pub(crate) fn speaking_users(&self, out: &mut Vec<u64>) {
        out.clear();
        let occupied = self.slot_occupied.load(Ordering::Acquire);
        let mut active = self.speaking_mask.load(Ordering::Acquire) & occupied;
        while active != 0 {
            let slot = active.trailing_zeros() as usize;
            active &= active - 1;
            if slot < MAX_PARTICIPANTS {
                out.push(self.slot_users[slot].load(Ordering::Acquire));
            }
        }
    }

    /// Creates a pull reader for `Playback` and its paired 20 ms mono AEC3/recorder render tap consumer.
    pub(crate) fn open_reader(&self) -> (PlayoutReader, rtrb::Consumer<Frame>) {
        let (tap_prod, tap_cons) = rtrb::RingBuffer::new(RENDER_TAP_RING);
        {
            let mut core = self.core.lock().unwrap_or_else(|e| e.into_inner());
            core.render_tap = Some(tap_prod);
            core.ref_half = 0;
        }
        (
            PlayoutReader {
                core: Arc::clone(&self.core),
                contention_underruns: Arc::clone(&self.contention_underruns),
            },
            tap_cons,
        )
    }

    /// Creates a lock-free loopback producer played directly inside `PlayoutReader` without feeding AEC3 `render_tap`.
    pub(crate) fn open_loopback(&self) -> rtrb::Producer<Stereo> {
        let (prod, cons) = rtrb::RingBuffer::new(16);
        let mut core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        core.loopback_rx = Some(cons);
        core.loopback_half = 0;
        core.loopback_primed = false;
        prod
    }

    /// Pulls one 20 ms stereo frame (two 10 ms blocks) for headless/simulated operation; returns `None` when silent.
    pub(crate) fn pull_20ms(&self, playing: bool) -> Option<Stereo> {
        let mut out = [0.0f32; 1920];
        let mut core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        let (first, second) = out.split_at_mut(OUTPUT_SIZE * 2);
        let a = core.pull_10ms_with_loopback(first.try_into().expect("first 10 ms half"), playing, false);
        let b = core.pull_10ms_with_loopback(second.try_into().expect("second 10 ms half"), playing, false);
        (a || b).then_some(out)
    }

    /// Returns the latest telemetry snapshot without blocking the playback callback.
    pub(crate) fn stats_snapshot(&self) -> PlayoutStatsCache {
        self.reap_garbage();
        let mut snapshot = self
            .stats_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        snapshot.underruns += self.contention_underruns.load(Ordering::Relaxed);
        snapshot
    }

    #[cfg(test)]
    pub(crate) fn publish_stats_now(&self) -> PlayoutStatsCache {
        let mut core = self.core.lock().unwrap_or_else(|e| e.into_inner());
        let dropped = core.drain_ingress_nonblocking();
        core.publish_stats(dropped);
        drop(core);
        self.stats_snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opus2::{Application, Channels, Encoder};

    fn opus_sine(freq_rad_per_sample: f32, count: usize) -> Vec<Vec<u8>> {
        let mut encoder = Encoder::new(48_000, Channels::Mono, Application::Voip).unwrap();
        (0..count)
            .map(|k| {
                let pcm: Frame = std::array::from_fn(|i| {
                    ((k * 960 + i) as f32 * freq_rad_per_sample).sin() * 0.25
                });
                let mut out = [0u8; 1275];
                let n = encoder.encode_float(&pcm, &mut out).unwrap();
                out[..n].to_vec()
            })
            .collect()
    }

    fn opus_stereo_sine(count: usize) -> Vec<Vec<u8>> {
        let mut encoder = Encoder::new(48_000, Channels::Stereo, Application::Audio).unwrap();
        (0..count)
            .map(|k| {
                let mut pcm = [0.0f32; 1920];
                for i in 0..960 {
                    let s = ((k * 960 + i) as f32 * 0.05).sin() * 0.3;
                    pcm[i * 2] = s;
                    pcm[i * 2 + 1] = -s;
                }
                let mut out = [0u8; 1275];
                let n = encoder.encode_float(&pcm, &mut out).unwrap();
                out[..n].to_vec()
            })
            .collect()
    }

    #[test]
    fn device_pull_decodes_publishes_speaking_and_feeds_render_tap() {
        let sidechain = Arc::new(AtomicU32::new(0));
        let playout = CallPlayout::new(sidechain);
        let (mut reader, mut tap) = playout.open_reader();

        playout.announce(42, 1001).unwrap();
        let packets = opus_sine(0.04, 6);
        for (i, p) in packets.iter().enumerate() {
            playout.push_packet(1001, i as u16, i as u32 * 960, p);
        }

        let mut half = [0.0f32; 960];
        let mut non_silent_pulls = 0usize;
        for _ in 0..6 {
            if reader.pull_10ms(&mut half, true) {
                non_silent_pulls += 1;
            }
        }
        assert!(non_silent_pulls >= 2);

        // Two 10 ms pulls produce one 960-sample mono reference frame on render_tap.
        let mut rendered = 0usize;
        while let Ok(mono) = tap.pop() {
            assert!(mono.iter().any(|s| s.abs() > 0.01));
            rendered += 1;
        }
        assert!(rendered >= 1);

        let mut speakers = Vec::new();
        playout.speaking_users(&mut speakers);
        assert_eq!(speakers, vec![42]);

        // Clearing or removing stops speaking immediately.
        playout.remove(42);
        let _ = reader.pull_10ms(&mut half, true);
        playout.speaking_users(&mut speakers);
        assert!(speakers.is_empty());
    }

    #[test]
    fn deafen_flushes_mixers_and_silences_output() {
        let sidechain = Arc::new(AtomicU32::new(0));
        let playout = CallPlayout::new(sidechain);
        let (mut reader, mut tap) = playout.open_reader();

        playout.announce(7, 77).unwrap();
        let packets = opus_sine(0.03, 5);
        for (i, p) in packets.iter().enumerate() {
            playout.push_packet(77, i as u16, i as u32 * 960, p);
        }
        let mut half = [0.0f32; 960];
        assert!(reader.pull_10ms(&mut half, true));
        assert!(half.iter().any(|s| s.abs() > 0.01));

        // Drain any tap frames from active period.
        while tap.pop().is_ok() {}

        // Pulling with playing=false silences output, flushes NetEq, and emits a continuous silent reference frame.
        assert!(!reader.pull_10ms(&mut half, false));
        assert!(half.iter().all(|&s| s == 0.0));
        let trailing = tap.pop().expect("continuous 20 ms render reference on deafen");
        assert!(!reader.pull_10ms(&mut half, false));
        assert!(!reader.pull_10ms(&mut half, false));
        let silent = tap.pop().expect("continuous silence frame while deafened");
        assert!(silent.iter().all(|&s| s == 0.0));
        let _ = trailing;

        let mut speakers = Vec::new();
        playout.speaking_users(&mut speakers);
        assert!(speakers.is_empty());

        // Re-enabling without new packets stays silent because flush occurred.
        assert!(!reader.pull_10ms(&mut half, true));
        assert!(half.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn watched_stream_audio_joins_in_stereo_and_ducks_on_voice() {
        let sidechain = Arc::new(AtomicU32::new(50));
        let playout = CallPlayout::new(sidechain);
        let (mut reader, _) = playout.open_reader();

        playout.announce_stream(99, 555, 1.0);
        let stream_pkts = opus_stereo_sine(6);
        for (i, p) in stream_pkts.iter().enumerate() {
            playout.push_stream_packet(99, 555, i as u16, i as u32 * 960, p);
        }
        let mut half = [0.0f32; 960];
        // Skip Opus's 6.5 ms initial encoder lookahead before measuring steady-state RMS.
        assert!(reader.pull_10ms(&mut half, true));
        assert!(reader.pull_10ms(&mut half, true));
        assert!(reader.pull_10ms(&mut half, true));
        let stream_only_rms = (half.iter().map(|s| s * s).sum::<f32>() / 960.0).sqrt();
        assert!(stream_only_rms > 0.05);

        // Add an audible speaker at unity gain so sidechain attenuation reduces stream contribution cleanly.
        playout.announce(10, 111).unwrap();
        let voice_pkts = opus_sine(0.04, 6);
        for (i, p) in voice_pkts.iter().enumerate() {
            playout.push_packet(111, i as u16, i as u32 * 960, p);
        }
        let mut mixed_half = [0.0f32; 960];
        // Skip voice encoder's initial lookahead ramp so voice exceeds the sidechain threshold.
        assert!(reader.pull_10ms(&mut mixed_half, true));
        assert!(reader.pull_10ms(&mut mixed_half, true));
        assert!(reader.pull_10ms(&mut mixed_half, true));
        // L and R contain symmetric voice + antisymmetric stereo stream: `(L - R) * 0.5` isolates the ducked stream!
        let ducked_stream_rms = (mixed_half
            .as_chunks::<2>()
            .0
            .iter()
            .map(|lr| {
                let s = (lr[0] - lr[1]) * 0.5;
                s * s
            })
            .sum::<f32>()
            / 480.0)
            .sqrt();
        assert!(
            ducked_stream_rms < stream_only_rms * 0.75,
            "expected sidechain duck: ducked={ducked_stream_rms} vs stream_only={stream_only_rms}"
        );
        let _ = playout.publish_stats_now();
    }

    #[test]
    fn rt_pull_queues_retired_decoders_without_dropping_on_audio_thread() {
        let sidechain = Arc::new(AtomicU32::new(0));
        let playout = CallPlayout::new(sidechain);
        let (mut reader, _) = playout.open_reader();

        playout.announce(42, 1001).unwrap();
        let mut half = [0.0f32; 960];
        let _ = reader.pull_10ms(&mut half, true);

        // Replace SSRC for the same user, update mix, and push a >32-packet burst on ingress.
        playout.announce(42, 1002).unwrap();
        playout.set_user_mix(42, 0.75, false);
        let burst = opus_sine(0.05, 40);
        for (i, p) in burst.iter().enumerate() {
            playout.push_packet(1002, i as u16, i as u32 * 960, p);
        }
        assert!(reader.pull_10ms(&mut half, true));
        assert_eq!(playout.reap_garbage(), 1);

        // Removing the speaker during RT pull retires its decoder onto the non-RT reaper ring.
        playout.remove(42);
        let _ = reader.pull_10ms(&mut half, true);
        assert_eq!(playout.reap_garbage(), 1);
    }

    #[test]
    fn open_loopback_plays_audio_without_polluting_render_tap() {
        let sidechain = Arc::new(AtomicU32::new(0));
        let playout = CallPlayout::new(sidechain);
        let (mut reader, mut tap) = playout.open_reader();
        let mut loopback_tx = playout.open_loopback();

        loopback_tx.push([0.4f32; 1920]).unwrap();
        loopback_tx.push([0.4f32; 1920]).unwrap();

        let mut half = [0.0f32; 960];
        let _ = reader.pull_10ms_with_loopback(&mut half, false, true);
        assert!(half.iter().all(|&s| (s - 0.4).abs() < 1e-6));
        let _ = reader.pull_10ms_with_loopback(&mut half, false, true);
        assert!(half.iter().all(|&s| (s - 0.4).abs() < 1e-6));

        let reference = tap.pop().expect("continuous render_tap frame");
        assert!(
            reference.iter().all(|&s| s == 0.0),
            "Mic Test loopback audio must never enter AEC3 render_tap"
        );
    }
}
