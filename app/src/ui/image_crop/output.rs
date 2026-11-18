//! The pixels of the image crop modal (module 142630): the decoded picture, the turned previews the editing area
//! draws, and the cropped result. A still picture goes through `iL` of module 830917 (the canvas draw of `R7`, then
//! `toDataURL("image/png")`); a GIF goes through `ny` of module 424632 (every frame cropped and turned at natural
//! size, kept a GIF).

use super::math::{Draw, Size};
use base64::Engine;
use image::{
    AnimationDecoder, DynamicImage, Frame, ImageDecoder, RgbaImage, imageops, imageops::FilterType,
};
use std::io::Cursor;
use std::sync::Arc;

/// The longest side the editing area's preview keeps: the drawn picture is at most 432 wide at zoom 2, so this is
/// sharp at every zoom while a 6000 px photo is not uploaded to the GPU whole.
const PREVIEW_SIDE: u32 = 1536;

/// A picture opened in the cropper.
pub(crate) struct Source {
    /// The original file.
    pub bytes: Vec<u8>,
    /// The upright first frame (EXIF orientation applied, as the browser's `naturalWidth` is).
    pub upright: RgbaImage,
    pub kind: Kind,
    /// The preview, scaled down, in BGRA for the atlas.
    preview: RgbaImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Still,
    /// `isGIF`: cropped frame by frame (`ny`).
    Gif,
    /// An animated WebP (`isAnimatedWebPFile`).
    AnimatedWebp,
}

impl Source {
    pub(crate) fn natural(&self) -> Size {
        Size::new(
            f64::from(self.upright.width()),
            f64::from(self.upright.height()),
        )
    }

    /// The preview turned clockwise by `rotation` degrees, ready for `img`.
    pub(crate) fn preview(&self, rotation: u32) -> Arc<gpui::RenderImage> {
        let turned = match rotation {
            90 => imageops::rotate90(&self.preview),
            180 => imageops::rotate180(&self.preview),
            270 => imageops::rotate270(&self.preview),
            _ => self.preview.clone(),
        };
        Arc::new(gpui::RenderImage::new(vec![Frame::new(turned)]))
    }
}

/// Decodes a picture for the cropper. `None` when it does not decode (the modal's `onError`: "Failed to load the
/// image.").
pub(crate) fn decode(bytes: Vec<u8>) -> Option<Source> {
    let reader = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .ok()?;
    let format = reader.format();
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut picture = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(orientation) = orientation {
        picture.apply_orientation(orientation);
    }
    let kind = match format {
        Some(image::ImageFormat::Gif) => Kind::Gif,
        Some(image::ImageFormat::WebP) => {
            let animated = image::codecs::webp::WebPDecoder::new(Cursor::new(&bytes))
                .is_ok_and(|decoder| decoder.has_animation());
            if animated {
                Kind::AnimatedWebp
            } else {
                Kind::Still
            }
        }
        _ => Kind::Still,
    };
    let upright = picture.to_rgba8();
    if upright.width() == 0 || upright.height() == 0 {
        return None;
    }
    let mut preview = if upright.width().max(upright.height()) > PREVIEW_SIDE {
        let scale = f64::from(PREVIEW_SIDE) / f64::from(upright.width().max(upright.height()));
        imageops::resize(
            &upright,
            ((f64::from(upright.width()) * scale).round() as u32).max(1),
            ((f64::from(upright.height()) * scale).round() as u32).max(1),
            FilterType::Triangle,
        )
    } else {
        upright.clone()
    };
    for pixel in preview.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(Source {
        bytes,
        upright,
        kind,
        preview,
    })
}

/// The cropped picture and its type.
pub(crate) struct Cropped {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

impl Cropped {
    pub(crate) fn data_uri(&self) -> String {
        format!(
            "data:{};base64,{}",
            self.mime,
            base64::engine::general_purpose::STANDARD.encode(&self.bytes)
        )
    }

    pub(crate) fn gpui_image(&self) -> Arc<gpui::Image> {
        let format = match self.mime {
            "image/gif" => gpui::ImageFormat::Gif,
            "image/webp" => gpui::ImageFormat::Webp,
            _ => gpui::ImageFormat::Png,
        };
        Arc::new(gpui::Image::from_bytes(format, self.bytes.clone()))
    }
}

/// The source rectangle in whole pixels, as a canvas or the crop worker reads it: transparent where it leaves the
/// picture.
fn cut(picture: &RgbaImage, x: i64, y: i64, width: u32, height: u32) -> RgbaImage {
    let mut out = RgbaImage::new(width.max(1), height.max(1));
    for (dx, dy, pixel) in out.enumerate_pixels_mut() {
        let (sx, sy) = (x + i64::from(dx), y + i64::from(dy));
        if sx >= 0 && sy >= 0 && sx < i64::from(picture.width()) && sy < i64::from(picture.height())
        {
            *pixel = *picture.get_pixel(sx as u32, sy as u32);
        }
    }
    out
}

fn turn(picture: &RgbaImage, rotation: u32) -> RgbaImage {
    match rotation {
        90 => imageops::rotate90(picture),
        180 => imageops::rotate180(picture),
        270 => imageops::rotate270(picture),
        _ => picture.clone(),
    }
}

/// `iL`: the source rectangle drawn turned onto the canvas (never larger than the type's largest size), as a PNG.
pub(crate) fn crop_still(picture: &RgbaImage, draw: Draw) -> RgbaImage {
    let source = cut(
        picture,
        draw.source_x.round() as i64,
        draw.source_y.round() as i64,
        draw.source_width.round().max(1.) as u32,
        draw.source_height.round().max(1.) as u32,
    );
    let turned = turn(&source, draw.rotation);
    let (width, height) = (draw.canvas_width.max(1), draw.canvas_height.max(1));
    if turned.dimensions() == (width, height) {
        turned
    } else {
        imageops::resize(&turned, width, height, FilterType::Triangle)
    }
}

/// `ny`'s rectangle: rounded, started inside the picture and cut at its far edges.
fn worker_rect(natural: (u32, u32), draw: Draw) -> (u32, u32, u32, u32) {
    let x = draw.source_x.round().max(0.) as u32;
    let y = draw.source_y.round().max(0.) as u32;
    let width = (draw.source_width.round() as u32).min(natural.0.saturating_sub(x));
    let height = (draw.source_height.round() as u32).min(natural.1.saturating_sub(y));
    (x, y, width.max(1), height.max(1))
}

/// Crops the picture as Apply does.
pub(crate) fn crop(source: &Source, draw: Draw) -> Result<Cropped, String> {
    match source.kind {
        Kind::Gif => crop_gif(&source.bytes, draw),
        // GAP: animated WebP goes up whole; the crop worker's WebP encoder (module 424632 `ny`, format "webp") has no
        // counterpart in the `image` crate.
        Kind::AnimatedWebp => Ok(Cropped {
            mime: "image/webp",
            bytes: source.bytes.clone(),
        }),
        Kind::Still => {
            let out = crop_still(&source.upright, draw);
            let mut bytes = Vec::new();
            DynamicImage::ImageRgba8(out)
                .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
                .map_err(|error| error.to_string())?;
            Ok(Cropped {
                mime: "image/png",
                bytes,
            })
        }
    }
}

/// `ny`: every frame cut to the rectangle and turned, at natural size, kept a GIF that loops.
fn crop_gif(bytes: &[u8], draw: Draw) -> Result<Cropped, String> {
    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .map_err(|error| error.to_string())?;
    let natural = decoder.dimensions();
    let (x, y, width, height) = worker_rect(natural, draw);
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|error| error.to_string())?;
    let cropped = frames.into_iter().map(|frame| {
        let delay = frame.delay();
        let buffer = frame.into_buffer();
        let piece = imageops::crop_imm(&buffer, x, y, width, height).to_image();
        Frame::from_parts(turn(&piece, draw.rotation), 0, 0, delay)
    });
    let mut out = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new_with_speed(&mut out, 10);
        encoder
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .map_err(|error| error.to_string())?;
        encoder
            .encode_frames(cropped)
            .map_err(|error| error.to_string())?;
    }
    Ok(Cropped {
        mime: "image/gif",
        bytes: out,
    })
}

#[cfg(test)]
mod tests {
    use super::super::math::{Editor, UploadType};
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    /// A picture in four coloured quarters: red top left, green top right, blue bottom left, white bottom right.
    fn quarters(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba(match (x < width / 2, y < height / 2) {
                (true, true) => RED,
                (false, true) => GREEN,
                (true, false) => BLUE,
                (false, false) => WHITE,
            })
        })
    }

    fn corners(out: &RgbaImage) -> [[u8; 4]; 4] {
        let (w, h) = (out.width() - 1, out.height() - 1);
        [
            out.get_pixel(0, 0).0,
            out.get_pixel(w, 0).0,
            out.get_pixel(0, h).0,
            out.get_pixel(w, h).0,
        ]
    }

    #[test]
    fn an_untouched_square_comes_out_whole() {
        let picture = quarters(200, 200);
        let editor = Editor::new(UploadType::Avatar, Size::new(200., 200.), 200.);
        let out = crop_still(&picture, editor.draw());
        assert_eq!(out.dimensions(), (200, 200));
        assert_eq!(corners(&out), [RED, GREEN, BLUE, WHITE]);
    }

    #[test]
    fn a_quarter_turn_turns_the_output_clockwise() {
        let picture = quarters(200, 200);
        let mut editor = Editor::new(UploadType::Avatar, Size::new(200., 200.), 200.);
        editor.rotate();
        let out = crop_still(&picture, editor.draw());
        // Turned clockwise: blue comes to the top left, red to the top right.
        assert_eq!(corners(&out), [BLUE, RED, WHITE, GREEN]);
    }

    #[test]
    fn zoomed_and_moved_the_output_is_the_quarter_under_the_crop() {
        let picture = quarters(200, 200);
        let mut editor = Editor::new(UploadType::Avatar, Size::new(200., 200.), 200.);
        editor.set_zoom(2.);
        // The picture moved down and right: the crop sits over its top left quarter.
        editor.move_to(100., 100.);
        let out = crop_still(&picture, editor.draw());
        assert_eq!(out.dimensions(), (100, 100));
        assert_eq!(corners(&out), [RED; 4]);
        // Turned a quarter, the same drag shows the quarter that came to the top left: blue.
        let mut turned = Editor::new(UploadType::Avatar, Size::new(200., 200.), 200.);
        turned.rotate();
        turned.set_zoom(2.);
        turned.move_to(100., 100.);
        let out = crop_still(&picture, turned.draw());
        assert_eq!(corners(&out), [BLUE; 4]);
    }

    #[test]
    fn a_big_picture_is_scaled_down_to_the_largest_size() {
        let picture = quarters(3000, 2000);
        let editor = Editor::new(UploadType::Avatar, Size::new(3000., 2000.), 350.);
        let out = crop_still(&picture, editor.draw());
        assert_eq!(out.dimensions(), (1024, 1024));
        // The centre square of a 3000 wide picture: the quarters meet in its middle.
        assert_eq!(corners(&out), [RED, GREEN, BLUE, WHITE]);
    }

    #[test]
    fn the_worker_rectangle_stays_inside_the_picture() {
        let draw = Draw {
            source_x: -0.4,
            source_y: 10.6,
            source_width: 120.2,
            source_height: 300.,
            canvas_width: 120,
            canvas_height: 120,
            rotation: 0,
        };
        assert_eq!(worker_rect((100, 200), draw), (0, 11, 100, 189));
    }

    #[test]
    fn a_gif_is_cropped_frame_by_frame_and_stays_a_gif() {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            let frames = [quarters(40, 20), quarters(40, 20)]
                .into_iter()
                .map(Frame::new);
            encoder.encode_frames(frames).expect("encodes");
        }
        let source = decode(bytes).expect("decodes");
        assert_eq!(source.kind, Kind::Gif);
        let mut editor = Editor::new(UploadType::Avatar, source.natural(), 200.);
        editor.rotate();
        let out = crop(&source, editor.draw()).expect("crops");
        assert_eq!(out.mime, "image/gif");
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(&out.bytes)).expect("gif");
        assert_eq!(decoder.dimensions(), (20, 20));
        let frames = decoder.into_frames().collect_frames().expect("frames");
        assert_eq!(frames.len(), 2);
    }

    #[test]
    fn a_still_comes_out_as_a_png_data_uri() {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(quarters(64, 32))
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("png");
        let source = decode(bytes).expect("decodes");
        assert_eq!(source.kind, Kind::Still);
        let editor = Editor::new(UploadType::Avatar, source.natural(), 200.);
        let out = crop(&source, editor.draw()).expect("crops");
        assert!(out.data_uri().starts_with("data:image/png;base64,"));
        assert!(decode(b"not a picture".to_vec()).is_none());
    }
}
