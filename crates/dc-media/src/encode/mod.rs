//! H.264 encoding shared by the camera and the screen share: the platform's hardware encoder first, openh264 when there
//! is none or when it fails mid-stream. Adapted from Serein's `crates/discord-voice/src/video_encode.rs`,
//! `camera.rs` (`CameraEncoder`) and `screen.rs` (`ScreenEncoder`, `encoder`, `encode_yuv`), MIT OR Apache-2.0.
//!
//! One difference from Serein: the camera does not send an IDR per picture. It keeps a two-second group of pictures and
//! a keyframe on request, like the screen, so 600 kb/s buys a usable picture and a viewer's keyframe request (PLI) has
//! something to ask for.

use crate::convert::{Chroma, i420_to_nv12, nv12_to_i420};
use crate::{Error, Result};
use openh264::{
    OpenH264API,
    encoder::{
        BitRate, Complexity, Encoder as SoftwareEncoder, EncoderConfig, FrameRate, FrameType,
        IntraFramePeriod, Profile as SoftwareProfile, RateControlMode, UsageType,
    },
    formats::YUVSlices,
};

#[cfg(windows)]
mod mf;

/// Which encoder produced the pictures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The Media Foundation hardware H.264 encoder (a GPU's fixed-function block).
    Hardware,
    /// openh264 on the CPU.
    Software,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Profile {
    Baseline,
    Main,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Content {
    Camera,
    Screen,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Config {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    /// The largest access unit the caller's transport accepts.
    pub max_bytes: usize,
    pub profile: Profile,
    pub content: Content,
}

impl Config {
    fn validate(&self) -> Result<()> {
        if self.width < 2
            || self.height < 2
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
            || self.fps == 0
            || self.bitrate == 0
        {
            return Err(Error::invalid(
                "encoder size must be even and rates nonzero",
            ));
        }
        Ok(())
    }

    fn gop(&self) -> u32 {
        self.fps * 2
    }
}

/// Consecutive pictures the hardware encoder may swallow without output before it counts as stuck and the software
/// encoder takes over (two seconds at 15 fps), so an encoder that accepts input and never answers cannot silence a
/// stream. From Serein's `HARDWARE_STALL`.
const HARDWARE_STALL: u32 = 30;

pub(crate) struct Output {
    pub data: Vec<u8>,
    pub keyframe: bool,
}

pub(crate) struct Encoder {
    config: Config,
    bitrate: u32,
    #[cfg(windows)]
    hardware: Option<mf::Encoder>,
    software: Option<SoftwareEncoder>,
    empty_run: u32,
    fell_back: bool,
}

impl Encoder {
    /// Hardware when the machine offers it, openh264 otherwise.
    pub(crate) fn new(config: Config) -> Result<Self> {
        config.validate()?;
        #[cfg(windows)]
        {
            match mf::Encoder::new(config) {
                Ok(hardware) => {
                    return Ok(Self {
                        config,
                        bitrate: config.bitrate,
                        hardware: Some(hardware),
                        software: None,
                        empty_run: 0,
                        fell_back: false,
                    });
                }
                Err(error) => tracing::debug!("hardware H.264 encoder unavailable: {error}"),
            }
        }
        Self::software(config)
    }

    /// openh264 only, whatever the machine offers.
    pub(crate) fn software(config: Config) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            bitrate: config.bitrate,
            #[cfg(windows)]
            hardware: None,
            software: Some(software_encoder(config, config.bitrate)?),
            empty_run: 0,
            fell_back: false,
        })
    }

    pub(crate) fn backend(&self) -> Backend {
        #[cfg(windows)]
        if self.hardware.is_some() {
            return Backend::Hardware;
        }
        Backend::Software
    }

    /// Whether a hardware encoder failed or stalled and openh264 took over.
    pub(crate) fn fell_back(&self) -> bool {
        self.fell_back
    }

    /// The 4:2:0 layout the active encoder takes without a shuffle.
    pub(crate) fn chroma(&self) -> Chroma {
        match self.backend() {
            Backend::Hardware => Chroma::Interleaved,
            Backend::Software => Chroma::Planar,
        }
    }

    /// Moves the target rate. `Ok(true)` when the encoder had to restart, so the next picture is a keyframe.
    pub(crate) fn set_bitrate(&mut self, bitrate: u32) -> Result<bool> {
        if bitrate == self.bitrate {
            return Ok(false);
        }
        #[cfg(windows)]
        if let Some(hardware) = self.hardware.as_mut() {
            if hardware.set_bitrate(bitrate).is_ok() {
                self.bitrate = bitrate;
                return Ok(false);
            }
            // Reopen a native encoder that refuses a live change at the new rate; release the scarce session first.
            self.hardware = None;
            let config = Config {
                bitrate,
                ..self.config
            };
            match mf::Encoder::new(config) {
                Ok(hardware) => self.hardware = Some(hardware),
                Err(_) => self.software = Some(software_encoder(config, bitrate)?),
            }
            self.bitrate = bitrate;
            return Ok(true);
        }
        // openh264 has no safe live rate setter and a restart costs an IDR: follow only large moves.
        if !crate::screen::software_rate_change(self.bitrate, bitrate) {
            return Ok(false);
        }
        self.software = Some(software_encoder(self.config, bitrate)?);
        self.bitrate = bitrate;
        Ok(true)
    }

    /// Encodes one picture of `config.width` x `config.height`, `yuv` laid out as `chroma` says. `None` when the encoder
    /// held or dropped it to keep the rate. A hardware failure switches to openh264 and answers with a keyframe.
    pub(crate) fn encode(
        &mut self,
        yuv: &[u8],
        chroma: Chroma,
        force_keyframe: bool,
    ) -> Result<Option<Output>> {
        let (width, height) = (self.config.width as usize, self.config.height as usize);
        if yuv.len() != width * height * 3 / 2 {
            return Err(Error::invalid(
                "encoder input does not match the configured size",
            ));
        }
        #[cfg(windows)]
        if let Some(hardware) = self.hardware.as_mut() {
            let nv12 = match chroma {
                Chroma::Interleaved => std::borrow::Cow::Borrowed(yuv),
                Chroma::Planar => std::borrow::Cow::Owned(i420_to_nv12(yuv, width, height)?),
            };
            match hardware.encode(&nv12, force_keyframe) {
                Ok(Some(output)) if output.data.len() <= self.config.max_bytes => {
                    self.empty_run = 0;
                    return Ok(Some(output));
                }
                Ok(None) => {
                    self.empty_run += 1;
                    if self.empty_run < HARDWARE_STALL {
                        return Ok(None);
                    }
                    tracing::warn!("hardware H.264 encoder stalled; using openh264");
                }
                Ok(Some(_)) => tracing::warn!("hardware H.264 picture too large; using openh264"),
                Err(error) => {
                    tracing::warn!("hardware H.264 encoder failed ({error}); using openh264")
                }
            }
            // The viewer must restart from a keyframe once the software encoder takes over.
            self.hardware = None;
            self.fell_back = true;
            self.empty_run = 0;
            self.software = Some(software_encoder(self.config, self.bitrate)?);
            return self.encode_software(yuv, chroma, true);
        }
        self.encode_software(yuv, chroma, force_keyframe)
    }

    fn encode_software(
        &mut self,
        yuv: &[u8],
        chroma: Chroma,
        force_keyframe: bool,
    ) -> Result<Option<Output>> {
        let (width, height) = (self.config.width as usize, self.config.height as usize);
        let i420 = match chroma {
            Chroma::Planar => std::borrow::Cow::Borrowed(yuv),
            Chroma::Interleaved => std::borrow::Cow::Owned(nv12_to_i420(yuv, width, height)?),
        };
        let (y, rest) = i420.split_at(width * height);
        let (u, v) = rest.split_at(width * height / 4);
        let source = YUVSlices::new((y, u, v), (width, height), (width, width / 2, width / 2));
        let encoder = self
            .software
            .as_mut()
            .ok_or_else(|| Error::unavailable("no H.264 encoder is running"))?;
        if force_keyframe {
            encoder.force_intra_frame();
        }
        let encoded = encoder
            .encode(&source)
            .map_err(|e| Error::failed(format!("H.264 encoding failed: {e}")))?;
        let keyframe = matches!(encoded.frame_type(), FrameType::IDR | FrameType::I);
        if matches!(encoded.frame_type(), FrameType::Skip | FrameType::Invalid) {
            return Ok(None);
        }
        let mut length = 0usize;
        for layer in 0..encoded.num_layers() {
            let layer = encoded
                .layer(layer)
                .ok_or_else(|| Error::failed("encoder returned an invalid layer"))?;
            for nal in 0..layer.nal_count() {
                length += layer
                    .nal_unit(nal)
                    .ok_or_else(|| Error::failed("encoder returned an invalid NAL unit"))?
                    .len();
            }
        }
        if length > self.config.max_bytes {
            return Err(Error::failed(format!(
                "encoded picture of {length} bytes exceeds the {} byte limit; choose a lower quality",
                self.config.max_bytes
            )));
        }
        let mut data = Vec::with_capacity(length);
        encoded.write_vec(&mut data);
        Ok((!data.is_empty()).then_some(Output { data, keyframe }))
    }
}

fn software_encoder(config: Config, bitrate: u32) -> Result<SoftwareEncoder> {
    let settings = EncoderConfig::new()
        .bitrate(BitRate::from_bps(bitrate))
        .max_frame_rate(FrameRate::from_hz(config.fps as f32))
        .rate_control_mode(RateControlMode::Bitrate)
        .intra_frame_period(IntraFramePeriod::from_num_frames(config.gop()))
        .profile(match config.profile {
            Profile::Baseline => SoftwareProfile::Baseline,
            Profile::Main => SoftwareProfile::Main,
        });
    let settings = match config.content {
        Content::Camera => settings
            .usage_type(UsageType::CameraVideoRealTime)
            .num_threads(1),
        Content::Screen => settings
            .usage_type(UsageType::ScreenContentRealTime)
            .complexity(if cfg!(windows) {
                Complexity::Low
            } else {
                Complexity::Medium
            })
            .num_threads(
                std::thread::available_parallelism()
                    .map_or(2, |count| count.get().clamp(2, 8) as u16),
            ),
    };
    SoftwareEncoder::with_api_config(OpenH264API::from_source(), settings)
        .map_err(|e| Error::unavailable(format!("openh264 could not start: {e}")))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn config(width: u32, height: u32) -> Config {
        Config {
            width,
            height,
            fps: 30,
            bitrate: 2_000_000,
            max_bytes: 2 * 1024 * 1024,
            profile: Profile::Main,
            content: Content::Screen,
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "opens the machine's hardware H.264 encoder"]
    fn the_hardware_encoder_opens_and_produces_keyframes() {
        for (w, h) in [(640, 480), (1280, 720), (1920, 1080)] {
            let started = std::time::Instant::now();
            match mf::Encoder::new(config(w, h)) {
                Ok(mut encoder) => {
                    let nv12 = vec![128u8; (w * h * 3 / 2) as usize];
                    let mut keyframes = 0;
                    let mut units = 0;
                    for i in 0..30 {
                        if let Some(out) = encoder.encode(&nv12, i == 0).unwrap() {
                            units += 1;
                            keyframes += usize::from(out.keyframe);
                        }
                    }
                    println!(
                        "{w}x{h}: hardware opened in {:?}, {units} units, {keyframes} keyframes",
                        started.elapsed()
                    );
                    assert!(units > 0 && keyframes > 0);
                }
                Err(error) => println!("{w}x{h}: hardware encoder unavailable: {error}"),
            }
        }
    }

    #[test]
    fn bad_configs_and_inputs_are_refused() {
        assert!(Encoder::software(config(0, 0)).is_err());
        assert!(Encoder::software(config(3, 2)).is_err());
        assert!(
            Encoder::software(Config {
                fps: 0,
                ..config(64, 64)
            })
            .is_err()
        );
        let mut encoder = Encoder::software(config(64, 64)).unwrap();
        assert_eq!(encoder.backend(), Backend::Software);
        assert_eq!(encoder.chroma(), Chroma::Planar);
        for length in [0, 64 * 64 * 3 / 2 - 1, 64 * 64 * 3 / 2 + 1] {
            assert!(
                encoder
                    .encode(&vec![0; length], Chroma::Planar, true)
                    .is_err()
            );
        }
    }

    #[test]
    fn oversized_pictures_are_an_error_not_a_truncation() {
        // Noise does not compress; a tiny limit must be reported, not silently sent cut.
        let mut encoder = Encoder::software(Config {
            max_bytes: 64,
            ..config(128, 128)
        })
        .unwrap();
        let noise: Vec<u8> = (0..128 * 128 * 3 / 2)
            .map(|i| ((i * 2654435761usize) >> 7) as u8)
            .collect();
        assert!(encoder.encode(&noise, Chroma::Planar, true).is_err());
    }

    #[test]
    fn rate_changes_restart_the_software_encoder_only_when_large() {
        let mut encoder = Encoder::software(config(64, 64)).unwrap();
        assert!(!encoder.set_bitrate(2_000_000).unwrap());
        assert!(!encoder.set_bitrate(2_100_000).unwrap());
        assert!(encoder.set_bitrate(1_000_000).unwrap());
        assert!(!encoder.set_bitrate(1_000_000).unwrap());
    }
}
