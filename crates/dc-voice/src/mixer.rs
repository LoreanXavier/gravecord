//! One receive stream per authenticated remote speaker, mixed onto one 20 ms clock. Each speaker's packets go through
//! its own [`NetEq`] (libwebrtc's jitter buffer, as the official engine plays every voice), which decodes, conceals and
//! stretches; the mixer sums what they give, at each user's volume.
//!
//! Adapted from Serein `crates/discord-voice/src/mixer.rs`, MIT OR Apache-2.0. Changes: a table sets each user's gain
//! and local mute, see `set_table`; Serein's fixed 40 ms window (`jitter.rs`) is replaced by the NetEq port.
#[cfg(test)]
use crate::Frame;
use crate::dave::MAX_PARTICIPANTS;
use crate::neteq::{NetEq, OUTPUT_SIZE, Stats};

struct Speaker {
    user: u64,
    ssrc: u32,
    slot: u8,
    gain: f32,
    neteq: Box<NetEq>,
    activity: u8,
    half_energy: f32,
    half_ready: bool,
    audio_level: f32,
}

const MAX_MIX_ENTRIES: usize = MAX_PARTICIPANTS * 4;

/// Snapshot of one active speaker's mixer/NetEq state for `rtc_stats` without allocating on the RT path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SpeakerStatsSnapshot {
    pub user: u64,
    pub ssrc: u32,
    pub stats: Stats,
    pub packets_discarded: u64,
    pub target_delay_ms: i32,
    pub jitter_buffer_ms: usize,
    pub audio_level: f32,
    pub decoding_muted_output: bool,
}

/// What the receive streams did since the last report, for the call's debug log every five seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Report {
    pub speakers: usize,
    /// The largest target delay and buffer level among the speakers now.
    pub target_delay_ms: i32,
    pub buffer_level_ms: usize,
    /// Counters over the interval, all speakers together.
    pub since: Stats,
}

#[derive(Default)]
pub(crate) struct Mixer {
    speakers: Vec<Speaker>,
    /// User to (volume as a factor, muted here): fixed-capacity table so RT mix updates never hash-reallocate.
    mix: Vec<(u64, f32, bool)>,
    /// Decodes and mixes in stereo (a watched stream's soundshare), else mono (the call's voices).
    stereo: bool,
    /// Counters of speakers that left, and the totals at the last report.
    retired: Stats,
    reported: Stats,
    /// Slot occupancy bitmask (`0..64`) for the lock-free speaking bitmask.
    slots_in_use: u64,
    next_slot_cursor: u8,
}

fn add(a: Stats, b: Stats) -> Stats {
    Stats {
        packets_received: a.packets_received + b.packets_received,
        packets_lost: a.packets_lost + b.packets_lost,
        packets_late: a.packets_late + b.packets_late,
        fec_recovered: a.fec_recovered + b.fec_recovered,
        concealed_samples: a.concealed_samples + b.concealed_samples,
        concealment_events: a.concealment_events + b.concealment_events,
        accelerated_samples: a.accelerated_samples + b.accelerated_samples,
        expanded_samples: a.expanded_samples + b.expanded_samples,
        flushes: a.flushes + b.flushes,
        underrun_samples: a.underrun_samples + b.underrun_samples,
        op_normal: a.op_normal + b.op_normal,
        op_expand: a.op_expand + b.op_expand,
        op_accelerate: a.op_accelerate + b.op_accelerate,
        op_preemptive_expand: a.op_preemptive_expand + b.op_preemptive_expand,
        op_cng: a.op_cng + b.op_cng,
        muted_samples: a.muted_samples + b.muted_samples,
    }
}

fn since(now: Stats, then: Stats) -> Stats {
    Stats {
        packets_received: now.packets_received.saturating_sub(then.packets_received),
        packets_lost: now.packets_lost.saturating_sub(then.packets_lost),
        packets_late: now.packets_late.saturating_sub(then.packets_late),
        fec_recovered: now.fec_recovered.saturating_sub(then.fec_recovered),
        concealed_samples: now.concealed_samples.saturating_sub(then.concealed_samples),
        concealment_events: now
            .concealment_events
            .saturating_sub(then.concealment_events),
        accelerated_samples: now
            .accelerated_samples
            .saturating_sub(then.accelerated_samples),
        expanded_samples: now.expanded_samples.saturating_sub(then.expanded_samples),
        flushes: now.flushes.saturating_sub(then.flushes),
        underrun_samples: now.underrun_samples.saturating_sub(then.underrun_samples),
        op_normal: now.op_normal.saturating_sub(then.op_normal),
        op_expand: now.op_expand.saturating_sub(then.op_expand),
        op_accelerate: now.op_accelerate.saturating_sub(then.op_accelerate),
        op_preemptive_expand: now
            .op_preemptive_expand
            .saturating_sub(then.op_preemptive_expand),
        op_cng: now.op_cng.saturating_sub(then.op_cng),
        muted_samples: now.muted_samples.saturating_sub(then.muted_samples),
    }
}

impl Mixer {
    /// A mixer whose frames are interleaved stereo, for stream audio: the sender encodes its system audio in stereo
    /// and a mono decoder folded it down.
    pub fn stereo() -> Self {
        Self {
            stereo: true,
            ..Self::default()
        }
    }
    /// Pre-allocates the speaker and mix tables up to the maximum budget so RT paths never allocate.
    pub fn preallocate(&mut self) {
        self.speakers
            .reserve(MAX_PARTICIPANTS.saturating_sub(1 + self.speakers.len()));
        if self.mix.capacity() < MAX_MIX_ENTRIES {
            self.mix
                .reserve_exact(MAX_MIX_ENTRIES - self.mix.capacity());
        }
    }
    fn channels(&self) -> usize {
        if self.stereo { 2 } else { 1 }
    }
    /// Replaces the per-user table; users without an entry play at unity.
    #[cfg(test)]
    pub fn set_table(&mut self, table: &std::collections::HashMap<u64, (f32, bool)>) {
        self.mix.clear();
        for (&user, &(volume, muted)) in table.iter().take(MAX_MIX_ENTRIES) {
            self.mix.push((user, volume, muted));
        }
        for speaker in &mut self.speakers {
            speaker.gain = gain_for(&self.mix, speaker.user);
        }
    }
    /// Updates a single user's volume and mute without heap allocation once preallocated.
    pub fn set_user_mix(&mut self, user: u64, volume: f32, muted: bool) {
        if let Some(entry) = self.mix.iter_mut().find(|(u, _, _)| *u == user) {
            entry.1 = volume;
            entry.2 = muted;
        } else if self.mix.len() < self.mix.capacity()
            || (self.mix.capacity() == 0 && self.mix.len() < MAX_MIX_ENTRIES)
        {
            if self.mix.capacity() == 0 {
                self.mix.reserve_exact(MAX_MIX_ENTRIES);
            }
            self.mix.push((user, volume, muted));
        } else if let Some(slot) = self
            .mix
            .iter()
            .position(|(u, _, _)| !self.speakers.iter().any(|s| s.user == *u))
            .or_else(|| (!self.mix.is_empty()).then_some(0))
        {
            self.mix[slot] = (user, volume, muted);
        }
        let gain = gain_for(&self.mix, user);
        for speaker in &mut self.speakers {
            if speaker.user == user {
                speaker.gain = gain;
            }
        }
    }
    fn alloc_slot(&mut self) -> u8 {
        for offset in 0..64u8 {
            let slot = self.next_slot_cursor.wrapping_add(offset) % 64;
            if (self.slots_in_use & (1u64 << slot)) == 0 {
                self.slots_in_use |= 1u64 << slot;
                self.next_slot_cursor = (slot + 1) % 64;
                return slot;
            }
        }
        0
    }
    /// Constructs a preallocated speaker [`NetEq`] off the real-time audio thread.
    pub(crate) fn new_speaker_decoder(stereo: bool) -> Result<Box<NetEq>, &'static str> {
        NetEq::new(stereo).map(Box::new)
    }
    /// Inserts a preallocated speaker decoder without allocating or dropping on the caller thread.
    pub(crate) fn announce_prepared(
        &mut self,
        user: u64,
        ssrc: u32,
        neteq: Box<NetEq>,
        mut retire: impl FnMut(Box<NetEq>),
    ) -> Result<(), &'static str> {
        if self
            .speakers
            .iter()
            .any(|s| s.ssrc == ssrc && s.user != user)
        {
            retire(neteq);
            return Err("Voice SSRC belongs to another participant");
        }
        if self
            .speakers
            .iter()
            .any(|s| s.user == user && s.ssrc == ssrc)
        {
            retire(neteq);
            return Ok(());
        }
        self.remove_with(user, &mut retire);
        if self.speakers.len() >= MAX_PARTICIPANTS - 1 {
            retire(neteq);
            return Err("Voice decoder budget exceeded");
        }
        let slot = self.alloc_slot();
        let gain = gain_for(&self.mix, user);
        self.speakers.push(Speaker {
            user,
            ssrc,
            slot,
            gain,
            neteq,
            activity: 0,
            half_energy: 0.0,
            half_ready: false,
            audio_level: 0.0,
        });
        Ok(())
    }
    pub fn announce(&mut self, user: u64, ssrc: u32) -> Result<(), &'static str> {
        if self
            .speakers
            .iter()
            .any(|s| s.ssrc == ssrc && s.user != user)
        {
            return Err("Voice SSRC belongs to another participant");
        }
        if self
            .speakers
            .iter()
            .any(|s| s.user == user && s.ssrc == ssrc)
        {
            return Ok(());
        }
        if self.speakers.len() >= MAX_PARTICIPANTS - 1
            && !self.speakers.iter().any(|s| s.user == user)
        {
            return Err("Voice decoder budget exceeded");
        }
        let neteq = Self::new_speaker_decoder(self.stereo)?;
        self.announce_prepared(user, ssrc, neteq, drop)
    }
    /// Removes a user's speaker without dropping its decoder on the caller thread.
    pub(crate) fn remove_with(&mut self, user: u64, mut retire: impl FnMut(Box<NetEq>)) {
        let mut retired = self.retired;
        let mut slots_in_use = self.slots_in_use;
        while let Some(pos) = self.speakers.iter().position(|s| s.user == user) {
            let speaker = self.speakers.swap_remove(pos);
            retired = add(retired, speaker.neteq.stats());
            slots_in_use &= !(1u64 << speaker.slot);
            retire(speaker.neteq);
        }
        self.retired = retired;
        self.slots_in_use = slots_in_use;
    }
    pub fn remove(&mut self, user: u64) {
        self.remove_with(user, drop);
    }
    pub fn user(&self, ssrc: u32) -> Option<u64> {
        self.speakers
            .iter()
            .find(|s| s.ssrc == ssrc)
            .map(|s| s.user)
    }
    pub fn slot_of(&self, user: u64) -> Option<u8> {
        self.speakers
            .iter()
            .find(|s| s.user == user)
            .map(|s| s.slot)
    }
    /// One received Opus packet of `ssrc`, by its RTP sequence number and timestamp.
    pub fn push(&mut self, ssrc: u32, sequence: u16, timestamp: u32, opus: &[u8]) {
        if let Some(speaker) = self.speakers.iter_mut().find(|s| s.ssrc == ssrc) {
            speaker.neteq.insert_packet(sequence, timestamp, opus);
        }
    }
    pub fn clear(&mut self) {
        for speaker in &mut self.speakers {
            speaker.neteq.flush();
            speaker.activity = 0;
            speaker.half_energy = 0.0;
            speaker.half_ready = false;
            speaker.audio_level = 0.0;
        }
    }
    pub fn clear_user(&mut self, user: u64) {
        for speaker in &mut self.speakers {
            if speaker.user == user {
                speaker.neteq.flush();
                speaker.activity = 0;
                speaker.half_energy = 0.0;
                speaker.half_ready = false;
                speaker.audio_level = 0.0;
            }
        }
    }
    /// Bitmask indexed by active speaker roster slot (`0..64`) for lock-free speaking publication.
    pub fn speaking_mask(&self) -> u64 {
        let mut mask = 0u64;
        for speaker in &self.speakers {
            if speaker.activity > 0 {
                mask |= 1u64 << speaker.slot;
            }
        }
        mask
    }
    /// Copies active speakers' NetEq statistics into `out` (zero allocation when `out` is reused).
    pub fn snapshot_speakers(&self, out: &mut Vec<SpeakerStatsSnapshot>) {
        out.clear();
        for speaker in &self.speakers {
            let (discarded_primary, discarded_secondary) = speaker.neteq.packets_discarded();
            out.push(SpeakerStatsSnapshot {
                user: speaker.user,
                ssrc: speaker.ssrc,
                stats: speaker.neteq.stats(),
                packets_discarded: discarded_primary + discarded_secondary,
                target_delay_ms: speaker.neteq.target_delay_ms(),
                jitter_buffer_ms: speaker.neteq.buffer_level_ms(),
                audio_level: speaker.audio_level,
                decoding_muted_output: speaker.neteq.is_muted(),
            });
        }
    }
    /// Users whose decoded audio crossed the activity level within the last 200 ms.
    #[cfg(test)]
    pub fn speaking(&self) -> impl Iterator<Item = u64> + '_ {
        self.speakers
            .iter()
            .filter(|s| s.activity > 0)
            .map(|s| s.user)
    }
    /// The receive streams since the last call of this (no user ids: counts and levels only).
    pub fn report(&mut self) -> Report {
        let total = self
            .speakers
            .iter()
            .fold(self.retired, |sum, s| add(sum, s.neteq.stats()));
        let report = Report {
            speakers: self.speakers.len(),
            target_delay_ms: self
                .speakers
                .iter()
                .map(|s| s.neteq.target_delay_ms())
                .max()
                .unwrap_or(0),
            buffer_level_ms: self
                .speakers
                .iter()
                .map(|s| s.neteq.buffer_level_ms())
                .max()
                .unwrap_or(0),
            since: since(total, self.reported),
        };
        self.reported = total;
        report
    }
    /// Mix one 20 ms mono frame.
    #[cfg(test)]
    pub fn pop(&mut self) -> Option<Frame> {
        debug_assert!(!self.stereo);
        let mut output = [0.0; 960];
        self.mix_into(&mut output).then_some(output)
    }
    /// Mix one 20 ms frame of a stereo mixer, interleaved.
    pub fn pop_stereo(&mut self) -> Option<crate::Stereo> {
        debug_assert!(self.stereo);
        let mut output = [0.0; 1920];
        self.mix_into(&mut output).then_some(output)
    }
    /// Mixes one 10 ms block (`OUTPUT_SIZE * channels` samples) into `output` with zero heap allocations.
    pub fn mix_10ms(&mut self, output: &mut [f32]) -> bool {
        let channels = self.channels();
        debug_assert_eq!(output.len(), OUTPUT_SIZE * channels);
        output.fill(0.0);
        let mut active = false;
        let mut block = [0.0f32; OUTPUT_SIZE * 2];
        let block = &mut block[..OUTPUT_SIZE * channels];
        let samples_per_20ms = (OUTPUT_SIZE * 2 * channels) as f32;
        for speaker in &mut self.speakers {
            speaker.neteq.get_audio(block);
            let mut energy = 0.0f32;
            if !speaker.neteq.is_muted() {
                active = true;
                let gain = speaker.gain;
                for (mixed, sample) in output.iter_mut().zip(block.iter()) {
                    if sample.is_finite() {
                        *mixed += *sample * gain;
                        // Speaking indicators follow the voice itself, not the local volume.
                        energy += *sample * *sample;
                    }
                }
            }
            if speaker.half_ready {
                let energy_20ms = speaker.half_energy + energy;
                speaker.audio_level = (energy_20ms / samples_per_20ms).sqrt().clamp(0.0, 1.0);
                speaker.activity = crate::activity::hold(energy_20ms, speaker.activity);
                speaker.half_energy = 0.0;
                speaker.half_ready = false;
            } else {
                speaker.half_energy = energy;
                speaker.half_ready = true;
            }
        }
        // Serein: hard limiting bounds simultaneous speakers.
        for sample in output.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
        active
    }
    /// Mixes 20 ms into `output` (960 samples per channel): two 10 ms blocks of every speaker's NetEq. False when
    /// nobody had anything but muted output.
    fn mix_into(&mut self, output: &mut [f32]) -> bool {
        let channels = self.channels();
        let mut active = false;
        for half in output.chunks_exact_mut(OUTPUT_SIZE * channels) {
            active |= self.mix_10ms(half);
        }
        active
    }
}

fn gain_for(mix: &[(u64, f32, bool)], user: u64) -> f32 {
    match mix.iter().find(|&&(entry_user, _, _)| entry_user == user) {
        Some((_, _, true)) => 0.0,
        Some((_, volume, false)) => volume.clamp(0.0, 2.0),
        None => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opus2::{Application, Channels, Decoder, Encoder};

    fn opus(frequency: f32) -> Vec<u8> {
        let mut encoder = Encoder::new(48_000, Channels::Mono, Application::Voip).unwrap();
        let pcm: Frame = std::array::from_fn(|i| (i as f32 * frequency).sin() * 0.2);
        let mut encoded = [0; 1275];
        let length = encoder.encode_float(&pcm, &mut encoded).unwrap();
        encoded[..length].to_vec()
    }

    #[test]
    fn a_user_can_be_muted_or_turned_down_here() {
        let mut full = Mixer::default();
        let mut half = Mixer::default();
        let mut muted = Mixer::default();
        for mixer in [&mut full, &mut half, &mut muted] {
            mixer.announce(1, 11).unwrap();
            mixer.push(11, 7, 7 * 960, &opus(0.05));
        }
        half.set_table(&[(1, (0.5, false))].into());
        muted.set_table(&[(1, (1.0, true))].into());
        let (a, b, c) = (
            full.pop().unwrap(),
            half.pop().unwrap(),
            muted.pop().unwrap(),
        );
        assert!(a.iter().any(|s| s.abs() > 0.01));
        for i in 0..960 {
            assert!((b[i] - a[i] * 0.5).abs() < 0.0001);
            assert_eq!(c[i], 0.0);
        }
        // The speaking indicator still sees the voice.
        assert_eq!(muted.speaking().count(), 1);
    }

    #[test]
    fn independent_streams_mix_on_one_clock_and_release_on_leave() {
        let mut together = Mixer::default();
        let mut alice = Mixer::default();
        let mut bob = Mixer::default();
        for (user, ssrc, single, frequency) in [(1, 11, &mut alice, 0.03), (2, 22, &mut bob, 0.08)]
        {
            together.announce(user, ssrc).unwrap();
            single.announce(user, ssrc).unwrap();
            // Same sequence from separate SSRCs must never collide or share decoder state.
            let packet = opus(frequency);
            together.push(ssrc, 7, 6720, &packet);
            single.push(ssrc, 7, 6720, &packet);
        }
        assert!(together.announce(3, 11).is_err(), "SSRC owned by user 1");
        let mixed = together.pop().unwrap();
        let a = alice.pop().unwrap();
        let b = bob.pop().unwrap();
        assert!(a.iter().any(|s| s.abs() > 0.01));
        assert!(b.iter().any(|s| s.abs() > 0.01));
        for i in 0..960 {
            assert!((mixed[i] - (a[i] + b[i]).clamp(-1.0, 1.0)).abs() < 0.0001);
        }
        assert_eq!(together.speaking().count(), 2);
        let report = together.report();
        assert_eq!(report.speakers, 2);
        assert_eq!(report.since.packets_received, 2);
        together.remove(1);
        assert_eq!(together.user(11), None);
        assert_eq!(together.report().since.packets_received, 0, "counted once");
        together.clear();
        assert!(together.pop().is_none());
        for user in 3..65 {
            together.announce(user, user as u32 + 100).unwrap();
        }
        assert!(together.announce(65, 165).is_err(), "decoder budget");
    }

    #[test]
    fn short_and_long_packets_and_a_lost_one_keep_realtime_ticks() {
        for samples in [240usize, 2880] {
            let mut mixer = Mixer::default();
            mixer.announce(1, 11).unwrap();
            let mut encoder = Encoder::new(48_000, Channels::Mono, Application::Audio).unwrap();
            let mut reference = Decoder::new(48_000, Channels::Mono).unwrap();
            let packets = 2 * 48_000 / samples;
            let mut frames = 0;
            let mut time = 0;
            for sequence in 0..packets {
                let input: Vec<f32> = (0..samples)
                    .map(|i| ((sequence * samples + i) as f32 * 0.037).sin() * 0.2)
                    .collect();
                let mut encoded = [0; 1275];
                let len = encoder.encode_float(&input, &mut encoded).unwrap();
                let mut decoded = vec![0.0; samples];
                assert_eq!(
                    reference
                        .decode_float(&encoded[..len], &mut decoded, false)
                        .unwrap(),
                    samples
                );
                if sequence != 3 {
                    mixer.push(
                        11,
                        sequence as u16,
                        (sequence * samples) as u32,
                        &encoded[..len],
                    );
                }
                // The receiver's clock: one 20 ms frame per 20 ms of sent audio.
                time += samples;
                while time >= 960 {
                    time -= 960;
                    let frame = mixer.pop().expect("one 20 ms frame per playout tick");
                    frames += 1;
                    if frames > 10 {
                        assert!(
                            frame.iter().any(|s| s.abs() > 0.01),
                            "duration {samples}: frame {frames} silent"
                        );
                    }
                }
            }
            assert_eq!(frames, packets * samples / 960);
        }
    }
}
