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

/// Consecutive pictures the hardware encoder may take without emitting output before it is reopened (~1.5–3 s at
/// 30–60 fps). A stall is not a failure: a foreground game after Alt-Tab can starve the GPU's encoder for a while, and
/// a fresh session recovers it, where openh264 at the same size would cost the CPU and the picture for good.
const HARDWARE_STALL: u32 = 90;

/// After a hardware failure openh264 carries the stream, and the hardware encoder is tried again this often. Our own
/// rule, not an official value: often enough that one bad moment does not cost a share's quality, rare enough that a
/// broken driver costs one failed open every ten seconds.
const HARDWARE_RETRY: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) struct Output {
    pub data: Vec<u8>,
    pub keyframe: bool,
    /// The capture time of the input picture this output encodes, as given to [`Encoder::encode_at`].
    pub at: std::time::Duration,
}

pub(crate) struct Encoder {
    config: Config,
    /// The rate the caller asked for.
    target: u32,
    /// The rate the running encoder was given: the target, or for hardware the target paced to the input rate.
    bitrate: u32,
    /// Capture times of the recent inputs, for the input frame rate.
    inputs: std::collections::VecDeque<std::time::Duration>,
    #[cfg(windows)]
    hardware: Option<mf::Encoder>,
    software: Option<SoftwareEncoder>,
    empty_run: u32,
    /// openh264 carries the stream because the hardware encoder failed; it is tried again at `retry_at`.
    fell_back: bool,
    #[cfg_attr(not(windows), allow(dead_code))]
    retry_at: Option<std::time::Instant>,
    /// The time base of [`Encoder::encode`], for callers that do not give a capture time.
    origin: std::time::Instant,
    /// The hardware encoder refused a live rate change once: from then on it is reopened only for large moves.
    live_refused: bool,
    /// An oversized picture was dropped; the next accepted picture forces a keyframe.
    oversized_keyframe: bool,
}

/// A live rate change smaller than this share of the current rate (one twentieth) waits for a larger one when the rate
/// goes up. libwebrtc hands every change to the encoder (`VideoStreamEncoder::SetEncoderRates` skips only equal ones);
/// this is our own rule, to keep a hardware encoder from being reconfigured on every small step of the estimator. A
/// lower rate is always taken: at a pacing factor of 1 an encoder above the target only builds queue.
const LIVE_RATE_STEP: u32 = 20;

/// Whether a live change from `current` to `target` is worth making.
pub(crate) fn live_rate_change(current: u32, target: u32) -> bool {
    target < current
        || u64::from(target - current) * u64::from(LIVE_RATE_STEP) >= u64::from(current)
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
                        target: config.bitrate,
                        bitrate: config.bitrate,
                        inputs: std::collections::VecDeque::new(),
                        hardware: Some(hardware),
                        software: None,
                        empty_run: 0,
                        fell_back: false,
                        retry_at: None,
                        origin: std::time::Instant::now(),
                        live_refused: false,
                        oversized_keyframe: false,
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
            target: config.bitrate,
            bitrate: config.bitrate,
            inputs: std::collections::VecDeque::new(),
            #[cfg(windows)]
            hardware: None,
            software: Some(software_encoder(config, config.bitrate)?),
            empty_run: 0,
            fell_back: false,
            retry_at: None,
            origin: std::time::Instant::now(),
            live_refused: false,
            oversized_keyframe: false,
        })
    }

    pub(crate) fn backend(&self) -> Backend {
        #[cfg(windows)]
        if self.hardware.is_some() {
            return Backend::Hardware;
        }
        Backend::Software
    }

    /// Whether openh264 carries the stream because the hardware encoder failed; false again once hardware reopens.
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
        self.target = bitrate;
        self.update_rate()
    }

    /// The frame rate pictures have been arriving at over the last [`INPUT_RATE_WINDOW`]; the configured rate until
    /// a second of them has been seen.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn input_fps(&self) -> f64 {
        input_fps(&self.inputs, self.config.fps)
    }

    fn record_input(&mut self, at: std::time::Duration) {
        self.inputs.push_back(at);
        while self
            .inputs
            .front()
            .is_some_and(|first| at.saturating_sub(*first) > INPUT_RATE_WINDOW)
        {
            self.inputs.pop_front();
        }
    }

    /// Gives the running encoder the rate it should have now. A hardware encoder spends its rate per picture at its
    /// configured frame rate whatever the sample times say (`hardware_rate_follows_sample_time_when_capture_slows`),
    /// so it is given the target scaled to the input rate. openh264 reads the timestamps itself.
    fn update_rate(&mut self) -> Result<bool> {
        #[cfg(windows)]
        let bitrate = if self.hardware.is_some() && !self.live_refused {
            paced_rate(self.target, self.config.fps, self.input_fps())
        } else {
            self.target
        };
        #[cfg(not(windows))]
        let bitrate = self.target;
        if bitrate == self.bitrate {
            return Ok(false);
        }
        #[cfg(windows)]
        if let Some(hardware) = self.hardware.as_mut() {
            if self.live_refused {
                // Each reopen costs a session and a keyframe: follow only the large moves, as for openh264.
                if !crate::screen::software_rate_change(self.bitrate, bitrate) {
                    return Ok(false);
                }
            } else {
                if !live_rate_change(self.bitrate, bitrate) {
                    return Ok(false);
                }
                if hardware.set_bitrate(bitrate).is_ok() {
                    self.bitrate = bitrate;
                    return Ok(false);
                }
                self.live_refused = true;
            }
            // Reopen a native encoder that refuses a live change at the new rate; release the scarce session first.
            self.hardware = None;
            let config = Config {
                bitrate,
                ..self.config
            };
            self.bitrate = bitrate;
            match mf::Encoder::new(config) {
                Ok(hardware) => self.hardware = Some(hardware),
                Err(_) => self.fall_back()?,
            }
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

    /// [`Self::encode_at`] stamped with the time since this encoder opened, for callers without a capture time.
    pub(crate) fn encode(
        &mut self,
        yuv: &[u8],
        chroma: Chroma,
        force_keyframe: bool,
    ) -> Result<Option<Output>> {
        let at = self.origin.elapsed();
        self.encode_at(yuv, chroma, force_keyframe, at)
    }

    /// Encodes one picture of `config.width` x `config.height`, `yuv` laid out as `chroma` says, captured `at` (any
    /// rising time base; each output carries the `at` of its own input back). `None` when the encoder held or dropped
    /// it to keep the rate. A stalled hardware encoder is reopened; a failed one hands the stream to openh264, which
    /// answers with a keyframe, and is tried again every [`HARDWARE_RETRY`].
    pub(crate) fn encode_at(
        &mut self,
        yuv: &[u8],
        chroma: Chroma,
        force_keyframe: bool,
        at: std::time::Duration,
    ) -> Result<Option<Output>> {
        let (width, height) = (self.config.width as usize, self.config.height as usize);
        if yuv.len() != width * height * 3 / 2 {
            return Err(Error::invalid(
                "encoder input does not match the configured size",
            ));
        }
        #[cfg(windows)]
        self.retry_hardware(std::time::Instant::now());
        self.record_input(at);
        if self.update_rate()? {
            self.oversized_keyframe = true;
        }
        let force_keyframe = force_keyframe || std::mem::take(&mut self.oversized_keyframe);
        #[cfg(windows)]
        if let Some(hardware) = self.hardware.as_mut() {
            let nv12 = match chroma {
                Chroma::Interleaved => std::borrow::Cow::Borrowed(yuv),
                Chroma::Planar => std::borrow::Cow::Owned(i420_to_nv12(yuv, width, height)?),
            };
            match hardware.encode(&nv12, force_keyframe, at) {
                Ok(Some(output)) if output.data.len() <= self.config.max_bytes => {
                    self.empty_run = 0;
                    return Ok(Some(output));
                }
                Ok(Some(_)) => {
                    tracing::warn!(
                        "hardware H.264 picture exceeded max_bytes; dropping and requesting keyframe"
                    );
                    self.oversized_keyframe = true;
                    return Ok(None);
                }
                Ok(None) => {
                    self.empty_run += 1;
                    if self.empty_run < HARDWARE_STALL {
                        if force_keyframe {
                            self.oversized_keyframe = true;
                        }
                        return Ok(None);
                    }
                    tracing::warn!("hardware H.264 encoder stalled; reopening it");
                    // Release the scarce session before opening its replacement.
                    self.hardware = None;
                    self.empty_run = 0;
                    match mf::Encoder::new(self.current_config()) {
                        Ok(hardware) => {
                            self.hardware = Some(hardware);
                            // The new session starts a new reference chain.
                            self.oversized_keyframe = true;
                            return Ok(None);
                        }
                        Err(error) => tracing::warn!(
                            "hardware H.264 encoder did not reopen ({error}); using openh264"
                        ),
                    }
                }
                Err(error) => {
                    tracing::warn!("hardware H.264 encoder failed ({error}); using openh264")
                }
            }
            self.fall_back()?;
            // The viewer must restart from a keyframe once the software encoder takes over.
            return self.encode_software(yuv, chroma, true, at);
        }
        self.encode_software(yuv, chroma, force_keyframe, at)
    }

    /// The configuration a reopened encoder starts with: the caller's target, paced again once pictures arrive.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn current_config(&mut self) -> Config {
        self.bitrate = self.target;
        Config {
            bitrate: self.target,
            ..self.config
        }
    }

    /// Hands the stream to openh264 after a hardware failure and schedules the next hardware attempt.
    #[cfg(windows)]
    fn fall_back(&mut self) -> Result<()> {
        self.hardware = None;
        self.fell_back = true;
        self.empty_run = 0;
        self.oversized_keyframe = false;
        self.retry_at = Some(std::time::Instant::now() + HARDWARE_RETRY);
        self.bitrate = self.target;
        self.software = Some(software_encoder(self.config, self.bitrate)?);
        Ok(())
    }

    /// Back to the hardware encoder once its retry is due and it opens; the next picture is then a keyframe.
    #[cfg(windows)]
    fn retry_hardware(&mut self, now: std::time::Instant) {
        if !retry_due(self.fell_back, self.retry_at, now) {
            return;
        }
        match mf::Encoder::new(self.current_config()) {
            Ok(hardware) => {
                tracing::debug!("hardware H.264 encoder reopened; leaving openh264");
                self.hardware = Some(hardware);
                self.software = None;
                self.fell_back = false;
                self.retry_at = None;
                self.empty_run = 0;
                self.oversized_keyframe = true;
            }
            Err(error) => {
                tracing::debug!("hardware H.264 encoder still unavailable: {error}");
                self.retry_at = Some(now + HARDWARE_RETRY);
            }
        }
    }

    /// Drains any encoded pictures still queued in the hardware backend without feeding new input.
    pub(crate) fn take_pending_output(&mut self) -> Result<Option<Output>> {
        #[cfg(windows)]
        if let Some(hardware) = self.hardware.as_mut() {
            match hardware.take_pending_output() {
                Ok(Some(output)) => {
                    self.empty_run = 0;
                    return Ok(Some(output));
                }
                Ok(None) => return Ok(None),
                Err(error) => {
                    tracing::warn!("hardware H.264 encoder failed ({error}); using openh264");
                    self.fall_back()?;
                    self.oversized_keyframe = true;
                }
            }
        }
        Ok(None)
    }

    fn encode_software(
        &mut self,
        yuv: &[u8],
        chroma: Chroma,
        force_keyframe: bool,
        at: std::time::Duration,
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
            .encode_at(
                &source,
                openh264::Timestamp::from_millis(u64::try_from(at.as_millis()).unwrap_or(u64::MAX)),
            )
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
            tracing::warn!(
                "encoded picture of {length} bytes exceeds the {} byte limit; dropping and forcing keyframe",
                self.config.max_bytes
            );
            self.oversized_keyframe = true;
            return Ok(None);
        }
        let mut data = Vec::with_capacity(length);
        encoded.write_vec(&mut data);
        Ok((!data.is_empty()).then_some(Output { data, keyframe, at }))
    }
}

/// How far back the input frame rate is measured: libwebrtc's `VideoStreamEncoder` averages the input frame rate it
/// hands the encoder over `kFrameRateAvergingWindowSizeMs` = 3 s.
const INPUT_RATE_WINDOW: std::time::Duration = std::time::Duration::from_secs(3);

/// The lowest frame rate a share offers (`FRAME_RATES`): a slower input is not given a bigger share of the rate per
/// picture than a 5 fps share would get.
const SLOWEST_INPUT_FPS: f64 = 5.;

/// Pictures per second over `inputs` (capture times, oldest first); `configured` until they span a second.
fn input_fps(inputs: &std::collections::VecDeque<std::time::Duration>, configured: u32) -> f64 {
    match (inputs.front(), inputs.back()) {
        (Some(first), Some(last))
            if last.saturating_sub(*first) >= std::time::Duration::from_secs(1) =>
        {
            (inputs.len() - 1) as f64 / last.saturating_sub(*first).as_secs_f64()
        }
        _ => f64::from(configured),
    }
}

/// The rate to give an encoder that spends `rate / configured` per picture so that pictures arriving at `input_fps`
/// still add up to `target` a second, as libwebrtc does by handing the encoder the measured frame rate.
#[cfg_attr(not(windows), allow(dead_code))]
fn paced_rate(target: u32, configured: u32, input_fps: f64) -> u32 {
    let configured = f64::from(configured.max(1));
    let input = input_fps.clamp(SLOWEST_INPUT_FPS.min(configured), configured);
    (f64::from(target) * configured / input)
        .round()
        .min(f64::from(u32::MAX)) as u32
}

/// Whether a stream on openh264 after a hardware failure should try the hardware encoder again at `now`.
#[cfg_attr(not(windows), allow(dead_code))]
fn retry_due(
    fell_back: bool,
    retry_at: Option<std::time::Instant>,
    now: std::time::Instant,
) -> bool {
    fell_back && retry_at.is_some_and(|at| now >= at)
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
                    .map_or(2, |count| count.get().clamp(2, 4) as u16),
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
                        let at = std::time::Duration::from_millis(33 * i);
                        if let Some(out) = encoder.encode(&nv12, i == 0, at).unwrap() {
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

    /// The rate a 60 fps hardware session gives when pictures arrive at 60 or at 15 a second, counted against the
    /// pictures' own sample times. With frame-counter sample times a 15 fps capture got a quarter of the rate.
    #[cfg(windows)]
    #[test]
    #[ignore = "opens the machine's hardware H.264 encoder"]
    fn hardware_rate_follows_sample_time_when_capture_slows() {
        let (w, h) = (1280u32, 720u32);
        let bitrate = 4_000_000;
        for spacing_ms in [16u64, 66] {
            let Ok(mut encoder) = Encoder::new(Config {
                fps: 60,
                bitrate,
                ..config(w, h)
            }) else {
                println!("hardware encoder unavailable");
                return;
            };
            let mut bytes = 0usize;
            let pictures = 120u64;
            for i in 0..pictures {
                // A scrolling pattern with fine detail: compressible, but with more detail than the rate buys.
                let nv12: Vec<u8> = (0..(w * h * 3 / 2) as usize)
                    .map(|p| {
                        let (x, y) = (p as u64 % u64::from(w), p as u64 / u64::from(w));
                        (x / 2 + y / 3 + i * 6 + (x * 13 + y * 7) % 11) as u8
                    })
                    .collect();
                let at = std::time::Duration::from_millis(spacing_ms * i);
                if let Some(out) = encoder
                    .encode_at(&nv12, Chroma::Interleaved, i == 0, at)
                    .unwrap()
                {
                    bytes += out.data.len();
                }
                while let Some(out) = encoder.take_pending_output().unwrap() {
                    bytes += out.data.len();
                }
            }
            let seconds = (spacing_ms * pictures) as f64 / 1000.;
            println!(
                "{spacing_ms} ms apart: {:.0} kb/s against {} kb/s, backend {:?}",
                bytes as f64 * 8. / seconds / 1000.,
                bitrate / 1000,
                encoder.backend()
            );
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
    fn oversized_pictures_drop_and_force_recovery_keyframe_not_truncation() {
        // Noise does not compress; an oversized encoded unit is dropped instead of truncated or killing the stream,
        // and the next compressible picture forces a recovery keyframe.
        let mut encoder = Encoder::software(Config {
            max_bytes: 64,
            ..config(128, 128)
        })
        .unwrap();
        let noise: Vec<u8> = (0..128 * 128 * 3 / 2)
            .map(|i| ((i * 2654435761usize) >> 7) as u8)
            .collect();
        assert!(
            encoder
                .encode(&noise, Chroma::Planar, true)
                .unwrap()
                .is_none()
        );
        assert!(encoder.oversized_keyframe);

        let mut recovering = Encoder::software(Config {
            max_bytes: 900,
            ..config(128, 128)
        })
        .unwrap();
        let solid = vec![128u8; 128 * 128 * 3 / 2];
        assert!(
            recovering
                .encode(&solid, Chroma::Planar, true)
                .unwrap()
                .is_some()
        );
        assert!(
            recovering
                .encode(&noise, Chroma::Planar, false)
                .unwrap()
                .is_none()
        );
        let recovered = recovering
            .encode(&solid, Chroma::Planar, false)
            .unwrap()
            .expect("compressible frame after oversized drop should emit recovery keyframe");
        assert!(recovered.keyframe);
        assert!(recovered.data.len() <= 900);
    }

    #[test]
    fn the_input_rate_paces_the_hardware_budget() {
        use std::time::Duration;
        let ms = Duration::from_millis;
        // At the configured rate nothing changes; a quarter of it gets four times the per-picture budget.
        assert_eq!(paced_rate(4_000_000, 60, 60.), 4_000_000);
        assert_eq!(paced_rate(4_000_000, 60, 15.), 16_000_000);
        // Never below a 5 fps share, never under the target for a faster input.
        assert_eq!(paced_rate(4_000_000, 60, 0.5), 48_000_000);
        assert_eq!(paced_rate(4_000_000, 60, 144.), 4_000_000);
        assert_eq!(paced_rate(4_000_000, 5, 1.), 4_000_000);

        let mut inputs = std::collections::VecDeque::new();
        assert_eq!(input_fps(&inputs, 60), 60.);
        inputs.extend((0..10).map(|i| ms(66 * i)));
        // Under a second of history: still the configured rate.
        assert_eq!(input_fps(&inputs, 60), 60.);
        inputs.extend((10..31).map(|i| ms(66 * i)));
        assert!((input_fps(&inputs, 60) - 1000. / 66.).abs() < 0.01);

        let mut encoder = Encoder::software(config(64, 64)).unwrap();
        for i in 0..200 {
            encoder.record_input(ms(16 * i));
        }
        let span = encoder
            .inputs
            .back()
            .unwrap()
            .saturating_sub(*encoder.inputs.front().unwrap());
        assert!(span <= INPUT_RATE_WINDOW);
    }

    #[test]
    fn hardware_is_retried_only_after_a_failure_and_only_when_due() {
        let now = std::time::Instant::now();
        let later = now + HARDWARE_RETRY;
        // A stream that chose openh264 from the start (no hardware) never retries.
        assert!(!retry_due(false, None, later));
        assert!(!retry_due(false, Some(now), later));
        // After a failure, not before the retry time, then every time it is due.
        assert!(!retry_due(true, Some(later), now));
        assert!(retry_due(true, Some(later), later));
        assert!(retry_due(true, Some(now), later));
    }

    #[test]
    fn outputs_carry_the_capture_time_of_their_input() {
        let mut encoder = Encoder::software(config(64, 64)).unwrap();
        let picture = vec![128u8; 64 * 64 * 3 / 2];
        for (index, ms) in [0u64, 40, 200, 233].into_iter().enumerate() {
            let at = std::time::Duration::from_millis(ms);
            let out = encoder
                .encode_at(&picture, Chroma::Planar, index == 0, at)
                .unwrap()
                .expect("openh264 emits every picture");
            assert_eq!(out.at, at);
        }
        assert!(!encoder.fell_back());
    }

    #[test]
    fn rate_changes_restart_the_software_encoder_only_when_large() {
        let mut encoder = Encoder::software(config(64, 64)).unwrap();
        assert!(!encoder.set_bitrate(2_000_000).unwrap());
        assert!(!encoder.set_bitrate(2_100_000).unwrap());
        assert!(encoder.set_bitrate(1_000_000).unwrap());
        assert!(!encoder.set_bitrate(1_000_000).unwrap());
    }

    #[test]
    fn live_rate_changes_wait_for_a_twentieth_upwards_and_always_go_down() {
        assert!(!live_rate_change(1_000_000, 1_049_999));
        assert!(live_rate_change(1_000_000, 1_050_000));
        assert!(live_rate_change(1_000_000, 999_000));
        assert!(live_rate_change(600_000, 1_800_000));
    }
}
