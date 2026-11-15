//! Audio into the buffer: PCM at 48 kHz in, Opus packets of 20 ms out, each with the time its first sample was captured.
//!
//! The Go Live stream's own audio is already sent as stereo Opus at 128 kb/s (`stream_conn`); the buffer encodes the same
//! PCM the same way, once, so a clip's audio is what the viewers heard. The user's own voice is mono (the processed
//! microphone frames) at 64 kb/s.
use crate::buffer::{AudioPacket, PACKET_MS, PACKET_SAMPLES};
use opus2::{Application, Bitrate, Channels, Decoder, Encoder};

pub struct TrackEncoder {
    encoder: Encoder,
    channels: usize,
    pending: Vec<f32>,
    /// Where the next packet starts at the earliest: right after the previous one.
    next_start: u64,
}

impl TrackEncoder {
    pub fn new(channels: u8) -> Result<Self, opus2::Error> {
        let layout = if channels == 1 {
            Channels::Mono
        } else {
            Channels::Stereo
        };
        let mut encoder = Encoder::new(48_000, layout, Application::Audio)?;
        encoder.set_bitrate(Bitrate::Bits(if channels == 1 { 64_000 } else { 128_000 }))?;
        Ok(Self {
            encoder,
            channels: usize::from(channels),
            pending: Vec::new(),
            next_start: 0,
        })
    }

    pub fn channels(&self) -> u8 {
        self.channels as u8
    }

    /// The decoder's pre-skip for a stream from this encoder, in 48 kHz samples (`dOps` carries it).
    pub fn pre_skip(&mut self) -> u16 {
        self.encoder
            .get_lookahead()
            .ok()
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(312)
    }

    /// Drops what is waiting, as when the source went quiet for good.
    pub fn reset(&mut self) {
        self.pending.clear();
    }

    /// Adds interleaved samples that were captured up to `now_ms` and returns the packets they complete. A packet starts
    /// where its first sample was captured, but never before the end of the one ahead of it.
    pub fn push(&mut self, samples: &[f32], now_ms: u64) -> Vec<AudioPacket> {
        let frame = PACKET_SAMPLES * self.channels;
        let waiting_ms = (self.pending.len() + samples.len()) as u64 / self.channels as u64 / 48;
        let mut start = now_ms.saturating_sub(waiting_ms);
        self.pending.extend(samples.iter().map(|s| {
            if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }));
        let mut out = Vec::new();
        let mut buffer = [0u8; 1275];
        while self.pending.len() >= frame {
            let Ok(length) = self
                .encoder
                .encode_float(&self.pending[..frame], &mut buffer)
            else {
                self.pending.clear();
                break;
            };
            self.pending.drain(..frame);
            start = start.max(self.next_start);
            out.push(AudioPacket {
                start_ms: start,
                data: buffer[..length].to_vec(),
            });
            start += PACKET_MS;
            self.next_start = start;
        }
        out
    }

    /// An Opus packet of silence for this layout.
    pub fn silence(channels: u8) -> Vec<u8> {
        let Ok(mut encoder) = Self::new(channels) else {
            return vec![0xF8, 0xFF, 0xFE];
        };
        let zeros = vec![0.0f32; PACKET_SAMPLES * usize::from(channels)];
        let mut buffer = [0u8; 1275];
        match encoder.encoder.encode_float(&zeros, &mut buffer) {
            Ok(length) => buffer[..length].to_vec(),
            Err(_) => vec![0xF8, 0xFF, 0xFE],
        }
    }
}

fn decode_track_raw(channels: u8, pre_skip: u16, packets: &[Vec<u8>]) -> (usize, Vec<f32>) {
    let ch = if channels == 1 { 1 } else { 2 };
    if packets.is_empty() || channels == 0 {
        return (ch, Vec::new());
    }
    let layout = if ch == 1 {
        Channels::Mono
    } else {
        Channels::Stereo
    };
    let Ok(mut decoder) = Decoder::new(48_000, layout) else {
        return (ch, Vec::new());
    };
    let mut frame = vec![0.0f32; PACKET_SAMPLES * ch];
    let mut pcm = Vec::with_capacity(packets.len() * PACKET_SAMPLES * ch);
    for packet in packets {
        let count = match decoder.decode_float(packet, &mut frame, false) {
            Ok(samples) => samples.min(PACKET_SAMPLES) * ch,
            Err(_) => {
                frame.fill(0.0);
                PACKET_SAMPLES * ch
            }
        };
        pcm.extend(frame[..count].iter().map(|&s| {
            if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }));
    }
    let skip = usize::from(pre_skip) * ch;
    let drop_len = skip.min(pcm.len());
    pcm.drain(..drop_len);
    (ch, pcm)
}

/// Decodes one MP4 Opus track to mono 48 kHz samples after stripping `pre_skip`.
pub fn decode_track_mono(channels: u8, pre_skip: u16, packets: &[Vec<u8>]) -> Vec<f32> {
    let (ch, raw) = decode_track_raw(channels, pre_skip, packets);
    if ch == 1 {
        raw
    } else {
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|&[left, right]| ((left + right) * 0.5).clamp(-1.0, 1.0))
            .collect()
    }
}

/// Decodes one MP4 Opus track to interleaved stereo 48 kHz samples after stripping `pre_skip`.
pub fn decode_track_stereo(channels: u8, pre_skip: u16, packets: &[Vec<u8>]) -> Vec<f32> {
    let (ch, raw) = decode_track_raw(channels, pre_skip, packets);
    if ch == 1 {
        let mut stereo = Vec::with_capacity(raw.len() * 2);
        for sample in raw {
            stereo.push(sample);
            stereo.push(sample);
        }
        stereo
    } else {
        raw
    }
}

/// Encodes interleaved stereo 48 kHz PCM into 20 ms Opus packets and returns `(pre_skip, packets)`.
pub fn encode_stereo_packets(interleaved_stereo: &[f32]) -> (u16, Vec<Vec<u8>>) {
    if interleaved_stereo.is_empty() {
        return (0, Vec::new());
    }
    let Ok(mut encoder) = TrackEncoder::new(2) else {
        return (0, Vec::new());
    };
    let pre_skip = encoder.pre_skip();
    let frame_samples = PACKET_SAMPLES * 2;
    let mut padded = vec![0.0f32; frame_samples];
    let mut buffer = [0u8; 1275];
    let mut packets = Vec::with_capacity(interleaved_stereo.len().div_ceil(frame_samples));
    for chunk in interleaved_stereo.chunks(frame_samples) {
        let src = if chunk.len() == frame_samples && chunk.iter().all(|s| s.is_finite()) {
            chunk
        } else {
            padded.fill(0.0);
            for (dst, &s) in padded.iter_mut().zip(chunk.iter()) {
                *dst = if s.is_finite() {
                    s.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
            }
            &padded[..]
        };
        if let Ok(len) = encoder.encoder.encode_float(src, &mut buffer) {
            packets.push(buffer[..len].to_vec());
        }
    }
    (pre_skip, packets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_packets_come_out_with_capture_times() {
        let mut encoder = TrackEncoder::new(2).unwrap();
        // 50 ms of stereo audio arrives at t = 1000: two packets complete, 10 ms wait.
        let samples = vec![0.1f32; 2400 * 2];
        let packets = encoder.push(&samples, 1_000);
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].start_ms, 950);
        assert_eq!(packets[1].start_ms, 970);
        // The next 10 ms finish the third.
        let more = encoder.push(&vec![0.1f32; 480 * 2], 1_010);
        assert_eq!(more.len(), 1);
        assert_eq!(more[0].start_ms, 990);
        assert!(!more[0].data.is_empty());
    }

    #[test]
    fn packets_never_overlap() {
        let mut encoder = TrackEncoder::new(1).unwrap();
        let first = encoder.push(&vec![0.0f32; 960], 100);
        // The next chunk is stamped earlier than the first packet ended: it follows on.
        let second = encoder.push(&vec![0.0f32; 960], 105);
        assert_eq!(first[0].start_ms, 80);
        assert_eq!(second[0].start_ms, 100);
    }

    #[test]
    fn silence_is_a_valid_small_packet() {
        let packet = TrackEncoder::silence(2);
        assert!(!packet.is_empty() && packet.len() < 200);
    }
}
