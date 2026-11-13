//! The animated backgrounds' decoders, played by `dc_media::background::animated`: MP4 through the H.264 reader the
//! spinner and GIFV clips use (`crate::spinner::H264Track`) and openh264, GIF through the image decoder the GIF tiles
//! use. Both keep only the encoded file and decode one frame at a time.

use dc_media::Picture;
use dc_media::background::animated::FrameDecoder;
use image::AnimationDecoder;
use openh264::formats::YUVSource;
use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

/// The largest frame accepted, as the GIFV clips (`video_clip`).
const MAX_PIXELS: usize = 4096 * 4096;

/// A GIF frame that claims less than this shows for [`GIF_DEFAULT_DELAY`] instead: the defaults of the GIF demuxer of
/// FFmpeg (`min_delay` 2 and `default_delay` 10, in hundredths), the decoder the official engine is built with
/// ("Ffmpeg H264 Decoder (SW)" in `discord_voice.node`). Inferred: the engine's own GIF path is not in the snapshot.
const GIF_MIN_DELAY: Duration = Duration::from_millis(20);
const GIF_DEFAULT_DELAY: Duration = Duration::from_millis(100);

/// An H.264 MP4, its samples already in Annex B, decoded in order and started over on rewind.
pub(super) struct Mp4 {
    units: Vec<Vec<u8>>,
    durations: Vec<Duration>,
    decoder: Option<openh264::decoder::Decoder>,
    next_unit: usize,
    next_frame: usize,
    flushed: bool,
    ready: VecDeque<Picture>,
}

impl Mp4 {
    pub(super) fn read(bytes: &[u8]) -> Result<Self, String> {
        let track = crate::spinner::H264Track::read(bytes)?;
        if track.len() == 0 {
            return Err("no frames".into());
        }
        let durations = track.durations().ok_or("invalid clip timing")?;
        let units = (0..track.len())
            .map(|index| track.sample(index))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            units,
            durations,
            decoder: None,
            next_unit: 0,
            next_frame: 0,
            flushed: false,
            ready: VecDeque::new(),
        })
    }

    fn keep(ready: &mut VecDeque<Picture>, yuv: &openh264::decoder::DecodedYUV<'_>) -> bool {
        let (width, height) = yuv.dimensions();
        if width == 0 || height == 0 || width * height > MAX_PIXELS {
            return false;
        }
        let mut rgba = vec![0; width * height * 4];
        yuv.write_rgba8(&mut rgba);
        match Picture::rgba(width as u32, height as u32, rgba) {
            Some(picture) => {
                ready.push_back(picture);
                true
            }
            None => false,
        }
    }
}

impl FrameDecoder for Mp4 {
    fn next_frame(&mut self) -> Option<(Duration, Picture)> {
        if self.decoder.is_none() {
            self.decoder = openh264::decoder::Decoder::new().ok();
        }
        while self.ready.is_empty() {
            let decoder = self.decoder.as_mut()?;
            if let Some(unit) = self.units.get(self.next_unit) {
                self.next_unit += 1;
                let options = openh264::decoder::DecodeOptions::new()
                    .flush_after_decode(openh264::decoder::Flush::NoFlush);
                if let Some(yuv) = decoder.decode_with_options(unit, options).ok()?
                    && !Self::keep(&mut self.ready, &yuv)
                {
                    return None;
                }
            } else if !self.flushed {
                self.flushed = true;
                for yuv in decoder.flush_remaining().ok()? {
                    if !Self::keep(&mut self.ready, &yuv) {
                        return None;
                    }
                }
            } else {
                return None;
            }
        }
        let picture = self.ready.pop_front()?;
        let duration = self
            .durations
            .get(self.next_frame)
            .copied()
            .unwrap_or(Duration::from_millis(33));
        self.next_frame += 1;
        Some((duration, picture))
    }

    fn rewind(&mut self) -> bool {
        self.decoder = None;
        self.next_unit = 0;
        self.next_frame = 0;
        self.flushed = false;
        self.ready.clear();
        true
    }
}

/// A GIF, decoded again from its bytes on every loop.
pub(super) struct Gif {
    bytes: Arc<[u8]>,
    frames: Option<image::Frames<'static>>,
}

impl Gif {
    pub(super) fn read(bytes: Arc<[u8]>) -> Result<Self, String> {
        let mut gif = Self {
            bytes,
            frames: None,
        };
        if !gif.rewind() {
            return Err("not a GIF".into());
        }
        Ok(gif)
    }
}

/// How long a GIF frame of `delay` shows.
fn gif_delay(delay: Duration) -> Duration {
    if delay < GIF_MIN_DELAY {
        GIF_DEFAULT_DELAY
    } else {
        delay
    }
}

impl FrameDecoder for Gif {
    fn next_frame(&mut self) -> Option<(Duration, Picture)> {
        let frame = self.frames.as_mut()?.next()?.ok()?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let delay = Duration::from_millis(u64::from(numerator) / u64::from(denominator.max(1)));
        let buffer = frame.into_buffer();
        let (width, height) = buffer.dimensions();
        if width as usize * height as usize > MAX_PIXELS {
            return None;
        }
        Some((
            gif_delay(delay),
            Picture::rgba(width, height, buffer.into_raw())?,
        ))
    }

    fn rewind(&mut self) -> bool {
        match image::codecs::gif::GifDecoder::new(Cursor::new(self.bytes.clone())) {
            Ok(decoder) => {
                self.frames = Some(decoder.into_frames());
                true
            }
            Err(_) => {
                self.frames = None;
                false
            }
        }
    }
}

/// The first frame of an animated file, for its tile.
pub(super) fn first_frame(bytes: &[u8], gif: bool) -> Option<Picture> {
    if gif {
        Gif::read(Arc::from(bytes))
            .ok()?
            .next_frame()
            .map(|(_, p)| p)
    } else {
        Mp4::read(bytes).ok()?.next_frame().map(|(_, p)| p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-frame GIF: red for 50 ms, then blue with no delay of its own.
    fn two_frame_gif() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut out);
            encoder
                .set_repeat(image::codecs::gif::Repeat::Infinite)
                .unwrap();
            let red = image::RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 255]));
            let blue = image::RgbaImage::from_pixel(4, 2, image::Rgba([0, 0, 255, 255]));
            encoder
                .encode_frame(image::Frame::from_parts(
                    red,
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(50, 1),
                ))
                .unwrap();
            encoder
                .encode_frame(image::Frame::from_parts(
                    blue,
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(0, 1),
                ))
                .unwrap();
        }
        out
    }

    #[test]
    fn a_gif_steps_through_its_frames_and_starts_over() {
        let mut gif = Gif::read(Arc::from(two_frame_gif().as_slice())).unwrap();
        let (first, picture) = gif.next_frame().unwrap();
        assert_eq!(first, Duration::from_millis(50));
        assert_eq!((picture.width, picture.height), (4, 2));
        assert_eq!(&picture.data[..4], &[255, 0, 0, 255]);
        let (second, picture) = gif.next_frame().unwrap();
        assert_eq!(second, GIF_DEFAULT_DELAY, "a frame without a delay");
        assert_eq!(&picture.data[..4], &[0, 0, 255, 255]);
        assert!(gif.next_frame().is_none(), "the end of the clip");
        assert!(gif.rewind());
        assert_eq!(&gif.next_frame().unwrap().1.data[..4], &[255, 0, 0, 255]);
        assert!(first_frame(&two_frame_gif(), true).is_some());
    }

    /// An H.264 MP4 of flat 64x48 frames of these brightnesses, 1/30 s each, written by the clips' MP4 writer.
    fn tiny_mp4(lumas: &[u8]) -> Vec<u8> {
        use openh264::OpenH264API;
        use openh264::encoder::{Encoder, EncoderConfig};
        use openh264::formats::YUVSlices;
        let (w, h) = (64usize, 48usize);
        let mut encoder =
            Encoder::with_api_config(OpenH264API::from_source(), EncoderConfig::new()).unwrap();
        let (mut sps, mut pps, mut samples) = (None, None, Vec::new());
        for &luma in lumas {
            let (y, u, v) = (
                vec![luma; w * h],
                vec![128; w * h / 4],
                vec![128; w * h / 4],
            );
            let source = YUVSlices::new((&y, &u, &v), (w, h), (w, w / 2, w / 2));
            let annex = encoder.encode(&source).unwrap().to_vec();
            for nal in dc_media::h264::nals(&annex) {
                match nal[0] & 0x1f {
                    dc_media::h264::NAL_SPS => sps = Some(nal.to_vec()),
                    dc_media::h264::NAL_PPS => pps = Some(nal.to_vec()),
                    _ => {}
                }
            }
            samples.push(dc_clips::mp4::VideoSample {
                data: dc_clips::mp4::to_length_prefixed(&annex),
                duration: dc_clips::mp4::VIDEO_TIMESCALE / 30,
                sync: dc_media::h264::is_keyframe(&annex),
            });
        }
        let (sps, pps) = (sps.unwrap(), pps.unwrap());
        let info = dc_clips::sps::parse(&sps).unwrap();
        let video = dc_clips::mp4::VideoInput {
            sps,
            pps,
            info,
            samples,
        };
        let mut out = Vec::new();
        dc_clips::mp4::write(&mut out, &video, &[], &[]).unwrap();
        out
    }

    #[test]
    fn an_mp4_steps_through_its_frames_at_their_own_rate_and_starts_over() {
        let bytes = tiny_mp4(&[30, 120, 220]);
        let mut clip = Mp4::read(&bytes).unwrap();
        let mut seen = Vec::new();
        while let Some((duration, picture)) = clip.next_frame() {
            assert_eq!((picture.width, picture.height), (64, 48));
            assert!(
                (duration.as_secs_f64() - 1.0 / 30.0).abs() < 0.002,
                "{duration:?}"
            );
            seen.push(picture.data[(24 * 64 + 32) * 4]);
        }
        assert_eq!(seen.len(), 3);
        assert!(seen[0] < seen[1] && seen[1] < seen[2], "{seen:?}");
        assert!(clip.rewind());
        let again = clip.next_frame().unwrap().1;
        assert_eq!(again.data[(24 * 64 + 32) * 4], seen[0]);
        assert!(first_frame(&bytes, false).is_some());
    }

    #[test]
    fn junk_is_no_clip() {
        assert!(Gif::read(Arc::from(&b"not a gif"[..])).is_err());
        assert!(Mp4::read(b"not an mp4").is_err());
        assert!(first_frame(b"nothing", false).is_none());
    }

    #[test]
    fn the_shortest_gif_delays_are_replaced() {
        assert_eq!(gif_delay(Duration::ZERO), GIF_DEFAULT_DELAY);
        assert_eq!(gif_delay(Duration::from_millis(10)), GIF_DEFAULT_DELAY);
        assert_eq!(
            gif_delay(Duration::from_millis(20)),
            Duration::from_millis(20)
        );
    }
}
