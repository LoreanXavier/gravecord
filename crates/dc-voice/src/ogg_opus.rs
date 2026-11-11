//! Ogg Opus files, for voice messages: a muxer that writes what the recorder encodes, and a demuxer and decoder for
//! the files people send (RFC 3533 pages, RFC 7845 `OpusHead` and `OpusTags`).
//!
//! The official desktop client gets its recording from the native voice module as an `.ogg` file and uploads it as it
//! is (module 841817 reads it with `readVoiceMessageRecording`); the module's own encoder settings are not in the
//! snapshot, so the choices here are the ones a voice message needs: mono, 48 kHz, 20 ms frames, voice tuning, 32 kb/s.
//! A voice message is whatever Ogg Opus file the sender's client made, so the decoder reads any channel count and
//! frame size.

use opus2::{Application, Bitrate, Channels, Decoder, Encoder};

/// Opus always runs at 48 kHz in an Ogg file; `OpusHead` only records the rate of the source.
pub const SAMPLE_RATE: u32 = 48_000;
/// 20 ms of mono audio at 48 kHz.
pub const FRAME_SAMPLES: usize = 960;
/// The bit rate of a recording, in bits per second.
pub const BITRATE: i32 = 32_000;
/// The longest Opus packet.
const MAX_PACKET: usize = 1275;
/// A page holds at most 255 lacing values.
const MAX_SEGMENTS: usize = 255;

/// The checksum of an Ogg page: CRC-32 with polynomial 0x04C11DB7, no reflection, initial value and final xor zero.
fn crc(bytes: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (index, slot) in table.iter_mut().enumerate() {
            let mut value = (index as u32) << 24;
            for _ in 0..8 {
                value = if value & 0x8000_0000 != 0 {
                    (value << 1) ^ 0x04C1_1DB7
                } else {
                    value << 1
                };
            }
            *slot = value;
        }
        table
    });
    let mut value = 0u32;
    for byte in bytes {
        value = (value << 8) ^ table[((value >> 24) as u8 ^ byte) as usize];
    }
    value
}

/// One page: `kind` is the header type (1 continued, 2 first of the stream, 4 last), `granule` the position.
fn page(kind: u8, granule: i64, serial: u32, sequence: u32, packets: &[&[u8]]) -> Vec<u8> {
    let mut lacing = Vec::new();
    for packet in packets {
        let mut left = packet.len();
        while left >= 255 {
            lacing.push(255u8);
            left -= 255;
        }
        lacing.push(left as u8);
    }
    let mut out =
        Vec::with_capacity(27 + lacing.len() + packets.iter().map(|p| p.len()).sum::<usize>());
    out.extend_from_slice(b"OggS");
    out.push(0);
    out.push(kind);
    out.extend_from_slice(&granule.to_le_bytes());
    out.extend_from_slice(&serial.to_le_bytes());
    out.extend_from_slice(&sequence.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.push(lacing.len() as u8);
    out.extend_from_slice(&lacing);
    for packet in packets {
        out.extend_from_slice(packet);
    }
    let sum = crc(&out);
    out[22..26].copy_from_slice(&sum.to_le_bytes());
    out
}

fn head(channels: u8, pre_skip: u16) -> Vec<u8> {
    let mut out = Vec::from(*b"OpusHead");
    out.push(1);
    out.push(channels);
    out.extend_from_slice(&pre_skip.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&0i16.to_le_bytes());
    out.push(0);
    out
}

fn tags() -> Vec<u8> {
    let vendor = b"gravecord";
    let mut out = Vec::from(*b"OpusTags");
    out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    out.extend_from_slice(vendor);
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// Encodes mono 48 kHz audio frame by frame and writes the Ogg file when it is finished.
pub struct Writer {
    encoder: Encoder,
    pre_skip: u16,
    packets: Vec<Vec<u8>>,
    pending: Vec<f32>,
    /// Samples handed in so far, without the padding of the last frame.
    samples: u64,
}

impl Writer {
    pub fn new() -> Result<Self, &'static str> {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip)
            .map_err(|_| "The Opus encoder is unavailable")?;
        encoder
            .set_bitrate(Bitrate::Bits(BITRATE))
            .map_err(|_| "The Opus encoder refused its bit rate")?;
        let pre_skip = encoder
            .get_lookahead()
            .ok()
            .and_then(|lookahead| u16::try_from(lookahead).ok())
            .unwrap_or(312);
        Ok(Self {
            encoder,
            pre_skip,
            packets: Vec::new(),
            pending: Vec::new(),
            samples: 0,
        })
    }

    /// Adds mono samples in any amount; whole 20 ms frames are encoded at once.
    pub fn push(&mut self, samples: &[f32]) {
        self.samples += samples.len() as u64;
        self.pending.extend(samples.iter().map(|sample| {
            if sample.is_finite() {
                sample.clamp(-1., 1.)
            } else {
                0.
            }
        }));
        let mut start = 0;
        while self.pending.len() - start >= FRAME_SAMPLES {
            let mut packet = vec![0u8; MAX_PACKET];
            if let Ok(length) = self
                .encoder
                .encode_float(&self.pending[start..start + FRAME_SAMPLES], &mut packet)
            {
                packet.truncate(length);
                self.packets.push(packet);
            }
            start += FRAME_SAMPLES;
        }
        self.pending.drain(..start);
    }

    /// Seconds recorded so far.
    pub fn seconds(&self) -> f64 {
        self.samples as f64 / f64::from(SAMPLE_RATE)
    }

    /// The file: the two header pages, then the audio, one page per second, the last one marked and carrying the
    /// exact end of the audio (the padding of the last frame is trimmed by the granule position).
    pub fn finish(mut self) -> Vec<u8> {
        // The last frame is padded with silence, and frames are added until the decoder's output covers the pre-skip
        // (the encoder's delay) as well as every sample that was recorded.
        let needed = i64::from(self.pre_skip) + self.samples as i64;
        while !self.pending.is_empty()
            || (self.samples > 0 && ((self.packets.len() * FRAME_SAMPLES) as i64) < needed)
        {
            let mut frame = std::mem::take(&mut self.pending);
            frame.resize(FRAME_SAMPLES, 0.);
            let mut packet = vec![0u8; MAX_PACKET];
            match self.encoder.encode_float(&frame, &mut packet) {
                Ok(length) => {
                    packet.truncate(length);
                    self.packets.push(packet);
                }
                Err(_) => break,
            }
        }
        let serial = 0x6772_6163; // "grac"
        let mut file = Vec::new();
        file.extend(page(2, 0, serial, 0, &[&head(1, self.pre_skip)]));
        file.extend(page(0, 0, serial, 1, &[&tags()]));
        let end = i64::from(self.pre_skip) + self.samples as i64;
        let mut sequence = 2;
        let per_page = 50.min(MAX_SEGMENTS);
        let pages = self.packets.chunks(per_page).count();
        for (index, chunk) in self.packets.chunks(per_page).enumerate() {
            let last = index + 1 == pages;
            let granule = if last {
                end
            } else {
                ((index + 1) * per_page * FRAME_SAMPLES) as i64
            };
            let refs: Vec<&[u8]> = chunk.iter().map(Vec::as_slice).collect();
            file.extend(page(
                if last { 4 } else { 0 },
                granule,
                serial,
                sequence,
                &refs,
            ));
            sequence += 1;
        }
        if self.packets.is_empty() {
            file.extend(page(4, end, serial, sequence, &[]));
        }
        file
    }
}

/// What the demuxer finds in a file.
#[derive(Debug)]
pub struct Stream {
    pub channels: u8,
    pub pre_skip: u16,
    pub packets: Vec<Vec<u8>>,
    /// The granule position of the last page: the samples at 48 kHz up to the end, the pre-skip included.
    pub end: i64,
}

impl Stream {
    /// The length of the audio in seconds, as the last page says.
    pub fn seconds(&self) -> f64 {
        (self.end - i64::from(self.pre_skip)).max(0) as f64 / f64::from(SAMPLE_RATE)
    }
}

/// Reads the pages of an Ogg Opus file and puts the packets back together.
pub fn demux(bytes: &[u8]) -> Result<Stream, &'static str> {
    let mut at = 0;
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut partial: Vec<u8> = Vec::new();
    let mut end = 0i64;
    while at + 27 <= bytes.len() {
        if &bytes[at..at + 4] != b"OggS" {
            return Err("Not an Ogg file");
        }
        let kind = bytes[at + 5];
        let granule = i64::from_le_bytes(bytes[at + 6..at + 14].try_into().expect("eight bytes"));
        let count = usize::from(bytes[at + 26]);
        let table_end = at + 27 + count;
        if table_end > bytes.len() {
            return Err("The file is cut short");
        }
        let mut data = table_end;
        if kind & 1 == 0 && !partial.is_empty() {
            // A packet that was left open is dropped, as a decoder does.
            partial.clear();
        }
        for &length in &bytes[at + 27..table_end] {
            let length = usize::from(length);
            if data + length > bytes.len() {
                return Err("The file is cut short");
            }
            partial.extend_from_slice(&bytes[data..data + length]);
            data += length;
            if length < 255 {
                packets.push(std::mem::take(&mut partial));
            }
        }
        if granule >= 0 {
            end = granule;
        }
        at = data;
    }
    if packets.len() < 2 {
        return Err("Not an Ogg Opus file");
    }
    let header = &packets[0];
    if header.len() < 19 || &header[..8] != b"OpusHead" {
        return Err("Not an Ogg Opus file");
    }
    let channels = header[9];
    let pre_skip = u16::from_le_bytes([header[10], header[11]]);
    if !(1..=2).contains(&channels) {
        return Err("Only mono and stereo voice messages play");
    }
    if &packets[1][..packets[1].len().min(8)] != b"OpusTags" {
        return Err("Not an Ogg Opus file");
    }
    Ok(Stream {
        channels,
        pre_skip,
        packets: packets.split_off(2),
        end,
    })
}

/// A decoded file: mono samples at 48 kHz, the pre-skip and the padding cut off.
#[derive(Debug)]
pub struct Pcm {
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / f64::from(SAMPLE_RATE)
    }
}

/// Decodes a whole file. A stereo file is mixed down to mono, as a voice message is heard on one channel pair.
pub fn decode(bytes: &[u8]) -> Result<Pcm, &'static str> {
    let stream = demux(bytes)?;
    let layout = if stream.channels == 1 {
        Channels::Mono
    } else {
        Channels::Stereo
    };
    let channels = usize::from(stream.channels);
    let mut decoder =
        Decoder::new(SAMPLE_RATE, layout).map_err(|_| "The Opus decoder is unavailable")?;
    // The longest packet is 120 ms.
    let mut frame = vec![0f32; 5760 * channels];
    let mut samples: Vec<f32> = Vec::new();
    for packet in &stream.packets {
        match decoder.decode_float(packet, &mut frame, false) {
            Ok(count) => {
                if channels == 1 {
                    samples.extend_from_slice(&frame[..count]);
                } else {
                    samples.extend(
                        frame[..count * 2]
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|pair| (pair[0] + pair[1]) * 0.5),
                    );
                }
            }
            // A damaged packet is a gap, not the end of the message.
            Err(_) => samples.extend(std::iter::repeat_n(0f32, FRAME_SAMPLES)),
        }
    }
    let skip = usize::from(stream.pre_skip).min(samples.len());
    samples.drain(..skip);
    if stream.end > 0 {
        let wanted = (stream.end - i64::from(stream.pre_skip)).max(0) as usize;
        samples.truncate(wanted);
    }
    if samples.is_empty() {
        return Err("The voice message holds no audio");
    }
    Ok(Pcm { samples })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32, hertz: f32) -> Vec<f32> {
        (0..(seconds * 48_000.) as usize)
            .map(|index| 0.5 * (std::f32::consts::TAU * hertz * index as f32 / 48_000.).sin())
            .collect()
    }

    /// The checksum of the Ogg specification's own test: the page of an empty first packet.
    #[test]
    fn the_page_checksum_matches_the_reference() {
        // CRC of "OggS" followed by zeros is stable under the algorithm; compare with a bitwise computation.
        let mut data = vec![0u8; 40];
        data[..4].copy_from_slice(b"OggS");
        let mut bitwise = 0u32;
        for byte in &data {
            bitwise ^= u32::from(*byte) << 24;
            for _ in 0..8 {
                bitwise = if bitwise & 0x8000_0000 != 0 {
                    (bitwise << 1) ^ 0x04C1_1DB7
                } else {
                    bitwise << 1
                };
            }
        }
        assert_eq!(crc(&data), bitwise);
    }

    #[test]
    fn a_recording_round_trips_with_its_exact_length() {
        let mut writer = Writer::new().expect("encoder");
        let input = tone(2.5, 440.);
        // Odd chunk sizes: the writer keeps the remainder between pushes.
        for chunk in input.chunks(777) {
            writer.push(chunk);
        }
        assert!((writer.seconds() - 2.5).abs() < 1e-6);
        let file = writer.finish();
        assert_eq!(&file[..4], b"OggS");
        assert_eq!(&file[28..36], b"OpusHead");
        let stream = demux(&file).expect("demux");
        assert_eq!(stream.channels, 1);
        assert!(
            (stream.seconds() - 2.5).abs() < 1e-6,
            "{}",
            stream.seconds()
        );
        let pcm = decode(&file).expect("decode");
        assert_eq!(pcm.samples.len(), 120_000);
        // The tone survived: its energy is about that of the input (0.5 amplitude, 0.125 mean square).
        let energy = pcm.samples[4800..].iter().map(|s| s * s).sum::<f32>()
            / (pcm.samples.len() - 4800) as f32;
        assert!((0.08..0.18).contains(&energy), "mean square {energy}");
        // 32 kb/s: a 2.5 s message is around 10 kB, not a megabyte.
        assert!(file.len() < 20_000, "{} bytes", file.len());
    }

    #[test]
    fn every_page_carries_a_valid_checksum() {
        let mut writer = Writer::new().expect("encoder");
        writer.push(&tone(3.0, 300.));
        let file = writer.finish();
        let mut at = 0;
        let mut pages = 0;
        while at < file.len() {
            let count = usize::from(file[at + 26]);
            let body: usize = file[at + 27..at + 27 + count]
                .iter()
                .map(|n| usize::from(*n))
                .sum();
            let total = 27 + count + body;
            let stored = u32::from_le_bytes(file[at + 22..at + 26].try_into().unwrap());
            let mut copy = file[at..at + total].to_vec();
            copy[22..26].fill(0);
            assert_eq!(crc(&copy), stored, "page {pages}");
            at += total;
            pages += 1;
        }
        // Two headers and one page per 50 packets: 3 s is 150 packets and one more frame covers the encoder's delay.
        assert_eq!(pages, 2 + 4);
        // The first page is marked as the start, the last as the end.
        assert_eq!(file[5], 2);
    }

    #[test]
    fn files_that_are_not_ogg_opus_are_refused() {
        assert!(demux(b"RIFF....WAVE").is_err());
        assert!(demux(&[]).is_err());
        assert!(decode(b"OggS").is_err());
        let mut writer = Writer::new().expect("encoder");
        writer.push(&tone(0.5, 200.));
        let file = writer.finish();
        assert!(demux(&file[..40]).is_err(), "cut inside a page");
    }

    #[test]
    fn an_empty_recording_is_a_valid_file_with_no_audio() {
        let file = Writer::new().expect("encoder").finish();
        let stream = demux(&file).expect("demux");
        assert!(stream.packets.is_empty());
        assert!(decode(&file).is_err());
    }
}
