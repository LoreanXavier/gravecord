//! One receive stream per authenticated remote speaker, mixed onto one 20 ms clock. Each speaker's packets go through
//! its own [`NetEq`] (libwebrtc's jitter buffer, as the official engine plays every voice), which decodes, conceals and
//! stretches; the mixer sums what they give, at each user's volume.
//!
//! Adapted from Serein `crates/discord-voice/src/mixer.rs`, MIT OR Apache-2.0. Changes: a table sets each user's gain
//! and local mute, see `set_table`; Serein's fixed 40 ms window (`jitter.rs`) is replaced by the NetEq port.
use crate::neteq::{NetEq, OUTPUT_SIZE, Stats};
use crate::{Frame, dave::MAX_PARTICIPANTS};

struct Speaker {
    user: u64,
    ssrc: u32,
    neteq: NetEq,
    activity: u8,
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
    /// User to (volume as a factor, muted here): what the person chose in that user's menu.
    mix: std::collections::HashMap<u64, (f32, bool)>,
    /// Decodes and mixes in stereo (a watched stream's soundshare), else mono (the call's voices).
    stereo: bool,
    /// Counters of speakers that left, and the totals at the last report.
    retired: Stats,
    reported: Stats,
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
    fn channels(&self) -> usize {
        if self.stereo { 2 } else { 1 }
    }
    /// Replaces the per-user table; users without an entry play at unity.
    pub fn set_table(&mut self, table: &std::collections::HashMap<u64, (f32, bool)>) {
        if self.mix != *table {
            self.mix.clone_from(table);
        }
    }
    fn gain(&self, user: u64) -> f32 {
        match self.mix.get(&user) {
            Some((_, true)) => 0.0,
            Some((volume, false)) => volume.clamp(0.0, 2.0),
            None => 1.0,
        }
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
        self.remove(user);
        if self.speakers.len() >= MAX_PARTICIPANTS - 1 {
            return Err("Voice decoder budget exceeded");
        }
        self.speakers.push(Speaker {
            user,
            ssrc,
            neteq: NetEq::new(self.stereo)?,
            activity: 0,
        });
        Ok(())
    }
    pub fn remove(&mut self, user: u64) {
        let mut retired = self.retired;
        self.speakers.retain(|s| {
            if s.user == user {
                retired = add(retired, s.neteq.stats());
                false
            } else {
                true
            }
        });
        self.retired = retired;
    }
    pub fn user(&self, ssrc: u32) -> Option<u64> {
        self.speakers
            .iter()
            .find(|s| s.ssrc == ssrc)
            .map(|s| s.user)
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
        }
    }
    /// Users whose decoded audio crossed the activity level within the last 200 ms.
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
    /// Mixes 20 ms into `output` (960 samples per channel): two 10 ms blocks of every speaker's NetEq. False when
    /// nobody had anything but muted output.
    fn mix_into(&mut self, output: &mut [f32]) -> bool {
        let channels = self.channels();
        let mut active = false;
        let gains: Vec<f32> = self.speakers.iter().map(|s| self.gain(s.user)).collect();
        let mut block = vec![0.0f32; OUTPUT_SIZE * channels];
        for (speaker, gain) in self.speakers.iter_mut().zip(gains) {
            let mut energy = 0.0;
            for half in output.chunks_exact_mut(OUTPUT_SIZE * channels) {
                speaker.neteq.get_audio(&mut block);
                if speaker.neteq.is_muted() {
                    continue;
                }
                active = true;
                for (mixed, sample) in half.iter_mut().zip(&block) {
                    if sample.is_finite() {
                        *mixed += sample * gain;
                        // Speaking indicators follow the voice itself, not the local volume.
                        energy += sample * sample;
                    }
                }
            }
            speaker.activity = crate::activity::hold(energy, speaker.activity);
        }
        // Serein: hard limiting bounds simultaneous speakers.
        output
            .iter_mut()
            .for_each(|sample| *sample = sample.clamp(-1.0, 1.0));
        active
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
