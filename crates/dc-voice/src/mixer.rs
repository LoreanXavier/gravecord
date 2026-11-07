//! One bounded decoder and reorder window per authenticated remote speaker, mixed onto one
//! 20 ms clock.
//!
//! Adapted from Serein `crates/discord-voice/src/mixer.rs`, MIT OR Apache-2.0. Changes: no
//! per-user volume table (unity gain), so `pop_with_volumes` became `pop`.
use crate::{Frame, dave::MAX_PARTICIPANTS, jitter::Jitter};
use opus2::{Channels, Decoder};

struct Speaker {
    user: u64,
    ssrc: u32,
    decoder: Decoder,
    jitter: Jitter,
    pcm: Box<[f32; 5760]>,
    offset: usize,
    length: usize,
    activity: u8,
}
#[derive(Default)]
pub(crate) struct Mixer {
    speakers: Vec<Speaker>,
}
impl Mixer {
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
            decoder: Decoder::new(48_000, Channels::Mono)
                .map_err(|_| "Opus decoder initialization failed")?,
            jitter: Jitter::default(),
            pcm: Box::new([0.0; 5760]),
            offset: 0,
            length: 0,
            activity: 0,
        });
        Ok(())
    }
    pub fn remove(&mut self, user: u64) {
        self.speakers.retain(|s| s.user != user);
    }
    pub fn user(&self, ssrc: u32) -> Option<u64> {
        self.speakers
            .iter()
            .find(|s| s.ssrc == ssrc)
            .map(|s| s.user)
    }
    pub fn push(&mut self, ssrc: u32, sequence: u16, opus: Vec<u8>) {
        if let Some(speaker) = self.speakers.iter_mut().find(|s| s.ssrc == ssrc) {
            speaker.jitter.push(sequence, opus);
        }
    }
    pub fn clear(&mut self) {
        for speaker in &mut self.speakers {
            speaker.jitter.clear();
            speaker.pcm.fill(0.0);
            speaker.offset = 0;
            speaker.length = 0;
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
    /// Mix one 20 ms frame, preserving up to 120 ms packets without bursting playback queues.
    pub fn pop(&mut self) -> Option<Frame> {
        let mut output = [0.0; 960];
        let mut active = false;
        for speaker in &mut self.speakers {
            let mut energy = 0.0;
            let mut filled = 0;
            let mut decoded = 0;
            while filled < output.len() {
                if speaker.offset == speaker.length {
                    // The shortest supported packet is 2.5 ms: at most eight decodes per tick.
                    if decoded == 8 {
                        break;
                    }
                    let Some(opus) = speaker.jitter.pop() else {
                        break;
                    };
                    decoded += 1;
                    let limit = if opus.is_empty() {
                        // Preserve the lost packet duration; long concealment drains over ticks.
                        speaker
                            .decoder
                            .get_last_packet_duration()
                            .unwrap_or(960)
                            .clamp(120, 5760) as usize
                    } else {
                        speaker.pcm.len()
                    };
                    let Ok(length) =
                        speaker
                            .decoder
                            .decode_float(&opus, &mut speaker.pcm[..limit], false)
                    else {
                        continue;
                    };
                    speaker.offset = 0;
                    speaker.length = length;
                }
                let count = (output.len() - filled).min(speaker.length - speaker.offset);
                for (mixed, sample) in output[filled..filled + count]
                    .iter_mut()
                    .zip(&speaker.pcm[speaker.offset..speaker.offset + count])
                {
                    if sample.is_finite() {
                        *mixed += sample;
                        energy += sample * sample;
                    }
                }
                speaker.offset += count;
                filled += count;
                active |= count != 0;
            }
            speaker.activity = crate::activity::hold(energy, speaker.activity);
        }
        // Serein: hard limiting bounds simultaneous speakers.
        output
            .iter_mut()
            .for_each(|sample| *sample = sample.clamp(-1.0, 1.0));
        active.then_some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opus2::{Application, Encoder};

    fn opus(frequency: f32) -> Vec<u8> {
        let mut encoder = Encoder::new(48_000, Channels::Mono, Application::Voip).unwrap();
        let pcm: Frame = std::array::from_fn(|i| (i as f32 * frequency).sin() * 0.2);
        let mut encoded = [0; 1275];
        let length = encoder.encode_float(&pcm, &mut encoded).unwrap();
        encoded[..length].to_vec()
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
            together.push(ssrc, 7, packet.clone());
            single.push(ssrc, 7, packet);
        }
        assert!(together.announce(3, 11).is_err(), "SSRC owned by user 1");
        for _ in 0..2 {
            assert!(together.pop().is_none(), "40 ms startup");
            assert!(alice.pop().is_none());
            assert!(bob.pop().is_none());
        }
        let mixed = together.pop().unwrap();
        let a = alice.pop().unwrap();
        let b = bob.pop().unwrap();
        assert!(a.iter().any(|s| s.abs() > 0.01));
        assert!(b.iter().any(|s| s.abs() > 0.01));
        for i in 0..960 {
            assert!((mixed[i] - (a[i] + b[i]).clamp(-1.0, 1.0)).abs() < 0.0001);
        }
        assert_eq!(together.speaking().count(), 2);
        together.remove(1);
        assert_eq!(together.user(11), None);
        together.clear();
        assert!(together.pop().is_none());
        for user in 3..65 {
            together.announce(user, user as u32 + 100).unwrap();
        }
        assert!(together.announce(65, 165).is_err(), "decoder budget");
    }

    #[test]
    fn short_and_lost_packets_keep_realtime_ticks() {
        for samples in [240usize, 2880] {
            let mut mixer = Mixer::default();
            mixer.announce(1, 11).unwrap();
            let mut encoder = Encoder::new(48_000, Channels::Mono, Application::Audio).unwrap();
            let mut reference = Decoder::new(48_000, Channels::Mono).unwrap();
            let mut expected = Vec::new();
            for sequence in 0..4u16 {
                let input: Vec<f32> = (0..samples)
                    .map(|i| (i as f32 * 0.037).sin() * 0.2)
                    .collect();
                let mut encoded = [0; 1275];
                let len = encoder.encode_float(&input, &mut encoded).unwrap();
                let packet = if sequence == 1 {
                    &[][..]
                } else {
                    &encoded[..len]
                };
                let mut decoded = vec![0.0; samples];
                let length = reference.decode_float(packet, &mut decoded, false).unwrap();
                assert_eq!(length, samples);
                expected.extend(decoded);
                if sequence != 1 {
                    mixer.push(11, sequence, packet.to_vec());
                }
            }
            assert!(mixer.pop().is_none());
            assert!(mixer.pop().is_none());
            for expected in expected.as_chunks::<960>().0 {
                let frame = mixer.pop().expect("one 20 ms frame per playout tick");
                for (actual, expected) in frame.iter().zip(expected) {
                    assert!((actual - expected).abs() < 0.0001, "duration {samples}");
                }
            }
        }
    }
}
