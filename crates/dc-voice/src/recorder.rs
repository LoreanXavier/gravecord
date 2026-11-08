//! Diagnostic Audio Recording: while the setting is on, a call keeps its last five minutes of voice and writes them as
//! WAV files into the voice folder when the recording ends (the setting turned off, the call over). The official string
//! says "The last five minutes of voice is saved to voice module folder" (`xl9+I6`); the native module's own dump format
//! is not readable, so these are plain 16-bit mono WAV files at 48 kHz, one per track:
//! - `microphone`: what the microphone delivered, before any processing;
//! - `sent`: the same after echo cancellation, noise suppression and the voice gate's processing;
//! - `speaker`: the mix played to the speakers.
//!
//! A track holds only the frames that existed: while muted the microphone delivers none, so a muted stretch is not
//! in the microphone and sent files.

use crate::Frame;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// 48 kHz mono.
pub const SAMPLE_RATE: u32 = 48_000;
/// "The last five minutes".
pub const KEEP_SECONDS: usize = 300;
const KEEP_SAMPLES: usize = SAMPLE_RATE as usize * KEEP_SECONDS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    Microphone = 0,
    Sent = 1,
    Speaker = 2,
}

impl Track {
    const ALL: [Track; 3] = [Track::Microphone, Track::Sent, Track::Speaker];

    fn name(self) -> &'static str {
        match self {
            Track::Microphone => "microphone",
            Track::Sent => "sent",
            Track::Speaker => "speaker",
        }
    }
}

pub struct Recorder {
    folder: PathBuf,
    /// Seconds since 1970 when the recording began, in the file names.
    started: u64,
    tracks: [VecDeque<i16>; 3],
    keep: usize,
}

fn to_i16(sample: f32) -> i16 {
    let sample = if sample.is_finite() { sample } else { 0. };
    (sample.clamp(-1., 1.) * f32::from(i16::MAX)).round() as i16
}

impl Recorder {
    pub fn new(folder: PathBuf) -> Self {
        Self::with_limit(folder, KEEP_SAMPLES)
    }

    fn with_limit(folder: PathBuf, keep: usize) -> Self {
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        Self {
            folder,
            started,
            tracks: Default::default(),
            keep,
        }
    }

    pub fn record(&mut self, track: Track, frame: &Frame) {
        let samples = &mut self.tracks[track as usize];
        samples.extend(frame.iter().map(|sample| to_i16(*sample)));
        if samples.len() > self.keep {
            let excess = samples.len() - self.keep;
            samples.drain(..excess);
        }
    }

    #[cfg(test)]
    fn seconds(&self, track: Track) -> f32 {
        self.tracks[track as usize].len() as f32 / SAMPLE_RATE as f32
    }

    /// Writes each track that has audio, and returns the files.
    pub fn finish(self) -> Vec<PathBuf> {
        let mut written = Vec::new();
        if std::fs::create_dir_all(&self.folder).is_err() {
            return written;
        }
        for track in Track::ALL {
            let samples = &self.tracks[track as usize];
            if samples.is_empty() {
                continue;
            }
            let path = self
                .folder
                .join(format!("voice-{}-{}.wav", self.started, track.name()));
            if write_wav(&path, samples.iter().copied(), samples.len(), SAMPLE_RATE).is_ok() {
                written.push(path);
            }
        }
        written
    }
}

/// A 16-bit mono PCM WAV file.
pub fn write_wav(
    path: &Path,
    samples: impl Iterator<Item = i16>,
    count: usize,
    rate: u32,
) -> std::io::Result<()> {
    use std::io::Write;
    let data_bytes = (count * 2) as u32;
    let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?; // PCM
    file.write_all(&1u16.to_le_bytes())?; // mono
    file.write_all(&rate.to_le_bytes())?;
    file.write_all(&(rate * 2).to_le_bytes())?; // bytes per second
    file.write_all(&2u16.to_le_bytes())?; // block align
    file.write_all(&16u16.to_le_bytes())?; // bits
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    for sample in samples {
        file.write_all(&sample.to_le_bytes())?;
    }
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_wav(path: &Path) -> (u32, Vec<i16>) {
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
        let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16, "16 bit");
        assert_eq!(&bytes[36..40], b"data");
        let size = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        assert_eq!(bytes.len(), 44 + size);
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize,
            36 + size
        );
        let samples = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair))
            .collect();
        (rate, samples)
    }

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("dc-voice-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn the_wav_file_reads_back_exactly() {
        let folder = folder("wav");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("a.wav");
        let samples = [0i16, 1, -1, i16::MAX, i16::MIN, 1234];
        write_wav(&path, samples.iter().copied(), samples.len(), 48_000).unwrap();
        let (rate, back) = read_wav(&path);
        assert_eq!((rate, back), (48_000, samples.to_vec()));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn only_the_last_stretch_is_kept() {
        let folder = folder("keep");
        // Room for three frames: the fourth pushes the first out.
        let mut recorder = Recorder::with_limit(folder.clone(), 3 * 960);
        for level in [0.1f32, 0.2, 0.3, 0.4] {
            recorder.record(Track::Microphone, &[level; 960]);
        }
        assert_eq!(recorder.seconds(Track::Microphone), 3. * 960. / 48_000.);
        let files = recorder.finish();
        assert_eq!(files.len(), 1, "silent tracks are not written");
        let (_, samples) = read_wav(&files[0]);
        assert_eq!(samples.len(), 3 * 960);
        assert_eq!(samples[0], to_i16(0.2));
        assert_eq!(*samples.last().unwrap(), to_i16(0.4));
        assert!(files[0].to_string_lossy().ends_with("-microphone.wav"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn five_minutes_is_the_default_window() {
        assert_eq!(KEEP_SAMPLES, 14_400_000);
    }

    #[test]
    fn each_track_has_its_own_file() {
        let folder = folder("tracks");
        let mut recorder = Recorder::new(folder.clone());
        recorder.record(Track::Microphone, &[0.5; 960]);
        recorder.record(Track::Sent, &[-0.5; 960]);
        recorder.record(Track::Speaker, &[f32::NAN; 960]);
        let mut files = recorder.finish();
        files.sort();
        assert_eq!(files.len(), 3);
        let names: Vec<_> = files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|name| name.ends_with("-microphone.wav")));
        assert!(names.iter().any(|name| name.ends_with("-sent.wav")));
        assert!(names.iter().any(|name| name.ends_with("-speaker.wav")));
        for file in &files {
            let (rate, samples) = read_wav(file);
            assert_eq!((rate, samples.len()), (48_000, 960));
            if file.to_string_lossy().ends_with("-speaker.wav") {
                assert!(
                    samples.iter().all(|sample| *sample == 0),
                    "non-finite becomes silence"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&folder);
    }
}
