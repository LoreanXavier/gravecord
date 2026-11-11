//! Cuts a downloaded picture down to the longest side the window can show, so what the decoded copy holds (four bytes a
//! pixel, in memory and in the sprite atlas) follows the screen and not the size of the artwork the server keeps.

use image::{DynamicImage, ImageFormat, imageops::FilterType};
use std::io::Cursor;

/// `bytes` scaled so its longest side is `side` pixels, in the same format (a JPEG stays a JPEG, everything else
/// becomes a PNG so no edge or transparency is lost), or `None` when it is already that small or cannot be read.
pub(crate) fn fit(bytes: &[u8], side: u32) -> Option<Vec<u8>> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return None;
    }
    let reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let (width, height) = reader.into_dimensions().ok()?;
    if width.max(height) <= side {
        return None;
    }
    let decoded = image::load_from_memory_with_format(bytes, format).ok()?;
    let (width, height) = target(width, height, side);
    let scaled = decoded.resize_exact(width, height, FilterType::CatmullRom);
    let mut out = Vec::new();
    match (format, &scaled) {
        (ImageFormat::Jpeg, _) => {
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
            DynamicImage::ImageRgb8(scaled.to_rgb8())
                .write_with_encoder(encoder)
                .ok()?;
        }
        _ => {
            let encoder = image::codecs::png::PngEncoder::new_with_quality(
                &mut out,
                image::codecs::png::CompressionType::Fast,
                image::codecs::png::FilterType::Adaptive,
            );
            DynamicImage::ImageRgba8(scaled.to_rgba8())
                .write_with_encoder(encoder)
                .ok()?;
        }
    }
    Some(out)
}

/// `bytes` cut down to what a box of `box_width` x `box_height` physical pixels needs when the picture covers it (the
/// shorter fit: both sides at least the box), rounded up like the CDN's `size=` (a quarter more first, for a window that grows). `cap` is the most any
/// picture keeps (`side_for`). `None` when it is already that small or cannot be read.
pub(crate) fn fit_box(bytes: &[u8], box_width: f32, box_height: f32, cap: u32) -> Option<Vec<u8>> {
    let (width, height) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    fit(
        bytes,
        side_for_box(width, height, box_width, box_height, cap),
    )
}

/// The longest side a `width` x `height` picture needs to cover a box, never above `cap`.
pub(crate) fn side_for_box(
    width: u32,
    height: u32,
    box_width: f32,
    box_height: f32,
    cap: u32,
) -> u32 {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let factor = (box_width / w).max(box_height / h) * 1.25;
    // The next power of two, the way the CDN's own `size=` parameter rounds (16 to 4096): a picture shrunk to exactly
    // its box and then sampled again by the renderer comes out soft.
    ((w.max(h) * factor).ceil() as u32)
        .next_power_of_two()
        .clamp(16, cap.max(16))
}

/// The size with the longest side `side` and the same proportions, each side at least one pixel.
fn target(width: u32, height: u32, side: u32) -> (u32, u32) {
    let scale = f64::from(side) / f64::from(width.max(height));
    (
        ((f64::from(width) * scale).round() as u32).max(1),
        ((f64::from(height) * scale).round() as u32).max(1),
    )
}

/// The longest side to keep Shop pictures at for a window of `width` x `height` logical pixels at `scale`: the most the
/// window can draw one picture at, with a quarter more for a window that grows.
pub(crate) fn side_for(width: f32, height: f32, scale: f32) -> u32 {
    ((width.max(height) * scale.max(1.) * 1.25).round() as u32).clamp(1280, 4096)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([
                (x % 256) as u8,
                (y % 256) as u8,
                40,
                if x < 4 { 0 } else { 255 },
            ])
        });
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn a_large_picture_is_cut_to_the_longest_side_with_its_proportions() {
        let cut = fit(&png(3000, 1200), 1500).expect("cut");
        let (width, height) = image::ImageReader::new(Cursor::new(&cut))
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap();
        assert_eq!((width, height), (1500, 600));
    }

    #[test]
    fn a_picture_within_the_side_is_left_as_it_came() {
        assert!(fit(&png(800, 600), 1500).is_none());
        assert!(fit(&png(1500, 10), 1500).is_none());
        assert!(fit(b"not an image", 1500).is_none());
    }

    #[test]
    fn transparency_survives_the_cut() {
        let cut = fit(&png(3000, 1000), 1000).expect("cut");
        let back = image::load_from_memory(&cut).unwrap().to_rgba8();
        assert_eq!(back.get_pixel(0, 5)[3], 0);
        assert_eq!(back.get_pixel(900, 5)[3], 255);
    }

    #[test]
    fn a_covered_box_needs_both_sides_and_never_more_than_the_cap() {
        // A 3000 x 1000 banner in a 400 x 400 box: the height decides, 400 / 1000 of 3000 is 1200, a quarter more 1500,
        // up to 2048.
        assert_eq!(side_for_box(3000, 1000, 400., 400., 4096), 2048);
        // A square 2000 picture in a 200 box: 250, up to 256.
        assert_eq!(side_for_box(2000, 2000, 200., 200., 4096), 256);
        assert_eq!(side_for_box(3000, 1000, 4000., 4000., 1600), 1600);
    }

    #[test]
    fn the_side_follows_the_window_and_its_scale() {
        assert_eq!(side_for(1280., 720., 1.), 1600);
        assert_eq!(side_for(1280., 720., 2.), 3200);
        assert_eq!(side_for(800., 600., 1.), 1280);
        assert_eq!(side_for(3840., 2160., 2.), 4096);
    }
}
