//! Bounded, silent H.264 clips for GIFV. Audio tracks are never opened.
use crate::spinner::H264Track;
use image::RgbaImage;
use openh264::formats::YUVSource;
use std::time::Duration;

pub const MAX_ENCODED: usize = 16 * 1024 * 1024;
/// The inline cache reserves this much for each pending clip.
pub const MAX_DECODED: usize = 16 * 1024 * 1024;
const MAX_FRAMES: usize = 600;

pub struct Clip {
    pub frames: Vec<(Duration, RgbaImage)>,
    pub bytes: usize,
}

fn fitted(width: u32, height: u32, count: usize, maximum: (u32, u32)) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || count == 0 || maximum.0 == 0 || maximum.1 == 0 {
        return None;
    }
    let memory_scale =
        (MAX_DECODED as f64 / count as f64 / 4. / f64::from(width) / f64::from(height)).sqrt();
    let scale = (f64::from(maximum.0) / f64::from(width))
        .min(f64::from(maximum.1) / f64::from(height))
        .min(memory_scale)
        .min(1.);
    Some((
        (f64::from(width) * scale).floor().max(1.) as u32,
        (f64::from(height) * scale).floor().max(1.) as u32,
    ))
}

pub fn decode(bytes: &[u8], maximum: (u32, u32)) -> Result<Clip, String> {
    if bytes.len() > MAX_ENCODED {
        return Err("encoded clip exceeds budget".into());
    }
    let track = H264Track::read(bytes)?;
    if track.len() == 0 || track.len() > MAX_FRAMES {
        return Err("clip exceeds frame budget".into());
    }
    let durations = track.durations().ok_or("invalid clip timing")?;
    let mut decoder = openh264::decoder::Decoder::new().map_err(|error| error.to_string())?;
    let mut frames = Vec::new();
    let mut total = 0usize;
    let mut keep = |yuv: &openh264::decoder::DecodedYUV<'_>| -> Result<(), String> {
        let (width, height) = yuv.dimensions();
        let pixels = width.checked_mul(height).ok_or("frame size overflow")?;
        if pixels > 4096 * 4096 {
            return Err("decoded frame exceeds canvas budget".into());
        }
        let (out_width, out_height) =
            fitted(width as u32, height as u32, track.len(), maximum).ok_or("empty frame")?;
        let mut rgba = vec![0; pixels.checked_mul(4).ok_or("frame size overflow")?];
        yuv.write_rgba8(&mut rgba);
        let frame =
            RgbaImage::from_raw(width as u32, height as u32, rgba).ok_or("invalid frame")?;
        let frame = if (out_width, out_height) == (width as u32, height as u32) {
            frame
        } else {
            image::imageops::resize(
                &frame,
                out_width,
                out_height,
                image::imageops::FilterType::Triangle,
            )
        };
        total = total
            .checked_add(frame.as_raw().len())
            .ok_or("clip size overflow")?;
        if total > MAX_DECODED {
            return Err("decoded clip exceeds budget".into());
        }
        frames.push(frame);
        Ok(())
    };
    for index in 0..track.len() {
        let unit = track.sample(index)?;
        let options = openh264::decoder::DecodeOptions::new()
            .flush_after_decode(openh264::decoder::Flush::NoFlush);
        if let Some(yuv) = decoder
            .decode_with_options(&unit, options)
            .map_err(|error| error.to_string())?
        {
            keep(&yuv)?;
        }
    }
    for yuv in decoder
        .flush_remaining()
        .map_err(|error| error.to_string())?
    {
        keep(&yuv)?;
    }
    if frames.len() != durations.len() {
        return Err("decoded frame count differs from timing table".into());
    }
    Ok(Clip {
        frames: durations.into_iter().zip(frames).collect(),
        bytes: total,
    })
}

fn yuv_to_rgba(yuv: &openh264::decoder::DecodedYUV<'_>) -> Option<RgbaImage> {
    let (width, height) = yuv.dimensions();
    let pixels = width.checked_mul(height)?;
    if pixels == 0 || pixels > 4096 * 4096 {
        return None;
    }
    let mut rgba = vec![0; pixels.checked_mul(4)?];
    yuv.write_rgba8(&mut rgba);
    RgbaImage::from_raw(width as u32, height as u32, rgba)
}

/// Decodes the first H.264 video frame from an MP4 payload into RGBA pixels.
pub(crate) fn mp4_first_frame_rgba(bytes: &[u8]) -> Option<RgbaImage> {
    let track = H264Track::read(bytes).ok()?;
    let limit = track.len().min(16);
    if limit == 0 {
        return None;
    }
    let mut decoder = openh264::decoder::Decoder::new().ok()?;
    for index in 0..limit {
        let unit = track.sample(index).ok()?;
        let options = openh264::decoder::DecodeOptions::new()
            .flush_after_decode(openh264::decoder::Flush::NoFlush);
        if let Ok(Some(yuv)) = decoder.decode_with_options(&unit, options)
            && let Some(frame) = yuv_to_rgba(&yuv)
        {
            return Some(frame);
        }
    }
    if let Ok(remaining) = decoder.flush_remaining()
        && let Some(yuv) = remaining.into_iter().next()
    {
        return yuv_to_rgba(&yuv);
    }
    None
}

/// Decodes the first H.264 video frame from an MP4 payload as a bounded PNG thumbnail.
pub(crate) fn mp4_first_frame_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let rgba = mp4_first_frame_rgba(bytes)?;
    let image = image::DynamicImage::ImageRgba8(rgba);
    let thumb = if image.width() > 480 || image.height() > 480 {
        image.thumbnail(480, 480)
    } else {
        image
    };
    let mut png = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_fit_preserves_aspect_and_caps_total_decoded_memory() {
        assert_eq!(fitted(498, 498, 45, (400, 300)), Some((300, 300)));
        assert_eq!(fitted(640, 210, 72, (400, 300)), Some((400, 131)));
        let (width, height) = fitted(3840, 2160, 600, (3840, 2160)).unwrap();
        assert!((width as usize * height as usize * 4 * 600) <= MAX_DECODED);
        assert!(decode(b"not an MP4", (400, 300)).is_err());
    }

    #[test]
    #[ignore = "requires a local H.264 clip in GRAVECORD_GIFV_TEST_FILE"]
    fn local_clip_decodes_colored_timed_frames() {
        let path = std::env::var("GRAVECORD_GIFV_TEST_FILE").unwrap();
        let clip = decode(&std::fs::read(path).unwrap(), (400, 300)).unwrap();
        assert!(clip.frames.len() > 1);
        assert!(clip.frames.windows(2).any(|pair| pair[0].1 != pair[1].1));
        let duration: Duration = clip.frames.iter().map(|(duration, _)| *duration).sum();
        assert!(!duration.is_zero());
        eprintln!(
            "decoded {} frames, {:.3}s, {} bytes",
            clip.frames.len(),
            duration.as_secs_f64(),
            clip.bytes
        );
    }
}
