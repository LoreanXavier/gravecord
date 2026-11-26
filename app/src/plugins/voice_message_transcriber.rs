//! VoiceMessageTranscriber (Equicord `src/equicordplugins/voiceMessageTranscriber.desktop`, GPL-3.0-or-later): the
//! audio a model is given (mono at 16 kHz, `decodeAudio`), the transcript's text (`formatTranscript`), the model and
//! language tables, and the engine a transcription runs on. The button and the dialogs are in
//! `ui/plugin_hooks/voice_message_transcriber.rs`. Not part of the pinned client.
//!
//! GAP: the app has no on-device speech model runner yet. The original runs Whisper (transformers.js on ONNX
//! Runtime Web) with the model chosen in `selectedModel`; here [`engine`] finds none, and a transcription ends with
//! [`NO_ENGINE`]. What remains for a runner: the Whisper encoder and decoder graphs on an ONNX runtime the app can
//! carry (tract 0.21, already in the tree for DeepFilterNet, is not proven to load them), the log-mel features (80
//! bins, 25 ms windows every 10 ms), the tokenizer's byte-level decoding, greedy decoding with the timestamp tokens and
//! language detection from the first step's logits, the compression-ratio retry, and the model files downloaded on
//! demand into [`models_dir`] by a button the user presses.

use std::path::PathBuf;

/// What a transcription ends with while the app has no engine.
pub(crate) const NO_ENGINE: &str = "No on-device speech model runner is built into this app yet, so voice messages cannot be transcribed.";
/// The sample rate Whisper takes.
pub(crate) const RATE: usize = 16_000;

/// `MODEL_SIZES`: each model's download, quantized, full and for the GPU.
pub(crate) const MODELS: &[(&str, &str, &str, &str)] = &[
    ("onnx-community/whisper-tiny", "~40 MB", "~150 MB", "~75 MB"),
    (
        "onnx-community/whisper-base",
        "~77 MB",
        "~290 MB",
        "~145 MB",
    ),
    (
        "onnx-community/whisper-small",
        "~250 MB",
        "~970 MB",
        "~490 MB",
    ),
    (
        "onnx-community/whisper-large-v3-turbo",
        "~1.1 GB",
        "~3.2 GB",
        "~1.6 GB",
    ),
];

/// `LEGACY_MODELS`: the old model names `migrateLegacyModel` moves to the new ones.
pub(crate) const LEGACY_MODELS: &[(&str, &str)] = &[
    ("Xenova/whisper-tiny", "onnx-community/whisper-tiny"),
    ("Xenova/whisper-base", "onnx-community/whisper-base"),
    ("Xenova/whisper-small", "onnx-community/whisper-small"),
    (
        "Xenova/whisper-medium",
        "onnx-community/whisper-large-v3-turbo",
    ),
];

/// The model's download size for the chosen settings (`ModelOption`).
pub(crate) fn model_size(model: &str, quantized: bool, gpu: bool) -> Option<&'static str> {
    let (_, small, full, on_gpu) = MODELS.iter().find(|(id, ..)| *id == model)?;
    Some(if gpu {
        on_gpu
    } else if quantized {
        small
    } else {
        full
    })
}

/// `LANGUAGES`: Whisper's language codes and names.
pub(crate) const LANGUAGES: &[(&str, &str)] = &[
    ("en", "english"),
    ("zh", "chinese"),
    ("de", "german"),
    ("es", "spanish/castilian"),
    ("ru", "russian"),
    ("ko", "korean"),
    ("fr", "french"),
    ("ja", "japanese"),
    ("pt", "portuguese"),
    ("tr", "turkish"),
    ("pl", "polish"),
    ("ca", "catalan/valencian"),
    ("nl", "dutch/flemish"),
    ("ar", "arabic"),
    ("sv", "swedish"),
    ("it", "italian"),
    ("id", "indonesian"),
    ("hi", "hindi"),
    ("fi", "finnish"),
    ("vi", "vietnamese"),
    ("he", "hebrew"),
    ("uk", "ukrainian"),
    ("el", "greek"),
    ("ms", "malay"),
    ("cs", "czech"),
    ("ro", "romanian/moldavian/moldovan"),
    ("da", "danish"),
    ("hu", "hungarian"),
    ("ta", "tamil"),
    ("no", "norwegian"),
    ("th", "thai"),
    ("ur", "urdu"),
    ("hr", "croatian"),
    ("bg", "bulgarian"),
    ("lt", "lithuanian"),
    ("la", "latin"),
    ("mi", "maori"),
    ("ml", "malayalam"),
    ("cy", "welsh"),
    ("sk", "slovak"),
    ("te", "telugu"),
    ("fa", "persian"),
    ("lv", "latvian"),
    ("bn", "bengali"),
    ("sr", "serbian"),
    ("az", "azerbaijani"),
    ("sl", "slovenian"),
    ("kn", "kannada"),
    ("et", "estonian"),
    ("mk", "macedonian"),
    ("br", "breton"),
    ("eu", "basque"),
    ("is", "icelandic"),
    ("hy", "armenian"),
    ("ne", "nepali"),
    ("mn", "mongolian"),
    ("bs", "bosnian"),
    ("kk", "kazakh"),
    ("sq", "albanian"),
    ("sw", "swahili"),
    ("gl", "galician"),
    ("mr", "marathi"),
    ("pa", "punjabi/panjabi"),
    ("si", "sinhala/sinhalese"),
    ("km", "khmer"),
    ("sn", "shona"),
    ("yo", "yoruba"),
    ("so", "somali"),
    ("af", "afrikaans"),
    ("oc", "occitan"),
    ("ka", "georgian"),
    ("be", "belarusian"),
    ("tg", "tajik"),
    ("sd", "sindhi"),
    ("gu", "gujarati"),
    ("am", "amharic"),
    ("yi", "yiddish"),
    ("lo", "lao"),
    ("uz", "uzbek"),
    ("fo", "faroese"),
    ("ht", "haitian creole/haitian"),
    ("ps", "pashto/pushto"),
    ("tk", "turkmen"),
    ("nn", "nynorsk"),
    ("mt", "maltese"),
    ("sa", "sanskrit"),
    ("lb", "luxembourgish/letzeburgesch"),
    ("my", "myanmar/burmese"),
    ("bo", "tibetan"),
    ("tl", "tagalog"),
    ("mg", "malagasy"),
    ("as", "assamese"),
    ("tt", "tatar"),
    ("haw", "hawaiian"),
    ("ln", "lingala"),
    ("ha", "hausa"),
    ("ba", "bashkir"),
    ("jw", "javanese"),
    ("su", "sundanese"),
];

/// The language list's label: "Auto Detect", or the name with its first letter capitalised.
pub(crate) fn language_label(code: &str) -> String {
    if code == "auto" {
        return "Auto Detect".to_owned();
    }
    let name = LANGUAGES
        .iter()
        .find(|(id, _)| *id == code)
        .map_or(code, |(_, name)| *name);
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// What a transcription is asked for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Options {
    pub model: String,
    pub quantized: bool,
    pub gpu: bool,
    /// `None`: detected from the audio.
    pub language: Option<String>,
    pub translate: bool,
}

/// A piece of the transcript and its start and end in seconds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Chunk {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Transcript {
    pub text: String,
    pub chunks: Vec<Chunk>,
}

/// `formatTimestamp`: `mm:ss`.
pub(crate) fn timestamp(seconds: f64) -> String {
    let seconds = seconds.max(0.) as u64;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// `formatTranscript`: the text, or each chunk on its own line after its span.
pub(crate) fn format(transcript: &Transcript, timestamps: bool) -> String {
    if !timestamps {
        return transcript.text.clone();
    }
    transcript
        .chunks
        .iter()
        .map(|chunk| {
            format!(
                "[{} - {}] {}",
                timestamp(chunk.start),
                timestamp(chunk.end),
                chunk.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `decodeAudio` at 16 kHz: the 48 kHz mono samples of a voice message, low-passed below 8 kHz (a 65-tap windowed
/// sinc) and kept one in three.
pub(crate) fn to_model_rate(samples: &[f32]) -> Vec<f32> {
    const TAPS: usize = 65;
    const STEP: usize = 48_000 / RATE;
    let half = TAPS / 2;
    let cutoff = 0.9 / (2. * STEP as f64);
    let mut kernel: Vec<f64> = (0..TAPS)
        .map(|index| {
            let at = index as f64 - half as f64;
            let sinc = if at == 0. {
                2. * cutoff
            } else {
                (2. * std::f64::consts::PI * cutoff * at).sin() / (std::f64::consts::PI * at)
            };
            let window = 0.42
                - 0.5 * (2. * std::f64::consts::PI * index as f64 / (TAPS - 1) as f64).cos()
                + 0.08 * (4. * std::f64::consts::PI * index as f64 / (TAPS - 1) as f64).cos();
            sinc * window
        })
        .collect();
    let sum: f64 = kernel.iter().sum();
    for tap in &mut kernel {
        *tap /= sum;
    }
    (0..samples.len().div_ceil(STEP))
        .map(|out| {
            let center = out * STEP;
            let mut total = 0.;
            for (index, tap) in kernel.iter().enumerate() {
                let at = center as isize + index as isize - half as isize;
                if at >= 0 && (at as usize) < samples.len() {
                    total += f64::from(samples[at as usize]) * tap;
                }
            }
            total as f32
        })
        .collect()
}

/// Where the model files are kept, under this run's data folder.
pub(crate) fn models_dir() -> Option<PathBuf> {
    crate::storage::data_dir().map(|dir| dir.join("plugins").join("voice-message-transcriber"))
}

/// The bytes the model folder holds (`getCacheSize`).
pub(crate) fn cache_size() -> u64 {
    fn walk(dir: &std::path::Path) -> u64 {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| {
                let Ok(kind) = entry.file_type() else {
                    return 0;
                };
                if kind.is_dir() {
                    walk(&entry.path())
                } else {
                    entry.metadata().map_or(0, |meta| meta.len())
                }
            })
            .sum()
    }
    models_dir().map_or(0, |dir| walk(&dir))
}

/// `clearCache`.
pub(crate) fn clear_cache() {
    if let Some(dir) = models_dir() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// What a transcription runs on: 16 kHz mono audio in, the transcript out, partial transcripts as they grow.
pub(crate) trait Engine: Send + Sync {
    fn transcribe(
        &self,
        audio: &[f32],
        options: &Options,
        partial: &mut dyn FnMut(&Transcript),
    ) -> Result<Transcript, String>;
}

/// The engine of this build: none yet (see the module's GAP). A capture build has a stand-in for its frames
/// (`plugin transcriber stand-in <text>`), which only cuts the given words over the audio's length.
pub(crate) fn engine() -> Option<Box<dyn Engine>> {
    #[cfg(feature = "capture")]
    if let Some(text) = STAND_IN.lock().ok().and_then(|text| text.clone()) {
        return Some(Box::new(StandIn(text)));
    }
    None
}

#[cfg(feature = "capture")]
pub(crate) static STAND_IN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(feature = "capture")]
struct StandIn(String);

#[cfg(feature = "capture")]
impl Engine for StandIn {
    fn transcribe(
        &self,
        audio: &[f32],
        _options: &Options,
        partial: &mut dyn FnMut(&Transcript),
    ) -> Result<Transcript, String> {
        let words: Vec<&str> = self.0.split_whitespace().collect();
        let length = audio.len() as f64 / RATE as f64;
        let per = length / words.len().max(1) as f64;
        let mut transcript = Transcript::default();
        for (index, pair) in words.chunks(2).enumerate() {
            let text = format!(" {}", pair.join(" "));
            transcript.text.push_str(&text);
            transcript.chunks.push(Chunk {
                start: index as f64 * 2. * per,
                end: ((index + 1) as f64 * 2. * per).min(length),
                text: text.trim().to_owned(),
            });
            partial(&transcript);
        }
        transcript.text = transcript.text.trim().to_owned();
        Ok(transcript)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcripts_read_with_and_without_timestamps() {
        let transcript = Transcript {
            text: "hello there general".to_owned(),
            chunks: vec![
                Chunk {
                    start: 0.,
                    end: 1.5,
                    text: "hello there".to_owned(),
                },
                Chunk {
                    start: 61.2,
                    end: 75.9,
                    text: "general".to_owned(),
                },
            ],
        };
        assert_eq!(format(&transcript, false), "hello there general");
        assert_eq!(
            format(&transcript, true),
            "[00:00 - 00:01] hello there\n[01:01 - 01:15] general"
        );
        assert_eq!(language_label("auto"), "Auto Detect");
        assert_eq!(language_label("es"), "Spanish/castilian");
        assert_eq!(
            model_size("onnx-community/whisper-base", true, false),
            Some("~77 MB")
        );
        assert_eq!(
            model_size("onnx-community/whisper-base", true, true),
            Some("~145 MB")
        );
        assert_eq!(model_size("x", true, true), None);
        assert_eq!(LANGUAGES.len(), 99);
    }

    #[test]
    fn audio_is_brought_to_sixteen_kilohertz() {
        // A second of 48 kHz becomes a second of 16 kHz; a 440 Hz tone keeps its level, a 12 kHz one is gone.
        let tone = |hz: f64| -> Vec<f32> {
            (0..48_000)
                .map(|at| (2. * std::f64::consts::PI * hz * at as f64 / 48_000.).sin() as f32)
                .collect()
        };
        let rms = |samples: &[f32]| {
            (samples[100..samples.len() - 100]
                .iter()
                .map(|s| f64::from(*s).powi(2))
                .sum::<f64>()
                / (samples.len() - 200) as f64)
                .sqrt()
        };
        let low = to_model_rate(&tone(440.));
        assert_eq!(low.len(), RATE);
        assert!(
            (rms(&low) - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.02,
            "{}",
            rms(&low)
        );
        let high = to_model_rate(&tone(12_000.));
        assert!(rms(&high) < 0.01, "{}", rms(&high));
        assert!(engine().is_none());
    }
}
