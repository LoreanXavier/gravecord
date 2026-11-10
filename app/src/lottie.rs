//! Lottie sticker art (`format_type` 3). The official client plays these with lottie-web on a canvas (module 148355);
//! here `rasterlottie` (pure Rust, MIT OR Apache-2.0, over tiny-skia) renders a frame to a PNG, off the UI thread.
//! The approach follows Serein's avatar worker, which uses the same crate.

use image::ImageEncoder;
use rasterlottie::{Animation, RenderConfig, Renderer, Rgba8};

/// The largest sticker file read (the official files are a few tens of KiB).
const MAX_JSON: usize = 512 * 1024;
/// The largest canvas an animation may declare.
const MAX_DIMENSION: u32 = 1024;

/// The first frame of a Lottie file, `size` pixels on its longer side, as PNG bytes.
pub fn first_frame_png(json: &[u8], size: u32) -> Option<Vec<u8>> {
    frame_png(json, size, None)
}

/// Frame `frame` (in the animation's own frame numbers; `None` is its in point) as PNG bytes.
pub fn frame_png(json: &[u8], size: u32, frame: Option<f32>) -> Option<Vec<u8>> {
    let animation = parse(json)?;
    let at = frame.unwrap_or(animation.in_point);
    let (pixels, width, height) = straight_pixels(&animation, size, at)?;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(png)
}

fn parse(json: &[u8]) -> Option<Animation> {
    if json.len() > MAX_JSON {
        return None;
    }
    let animation = Animation::from_json_str(std::str::from_utf8(json).ok()?).ok()?;
    if animation.width == 0
        || animation.height == 0
        || animation.width > MAX_DIMENSION
        || animation.height > MAX_DIMENSION
    {
        return None;
    }
    Some(animation)
}

/// One frame as straight-alpha RGBA, `size` pixels on the longer side of the canvas.
fn straight_pixels(animation: &Animation, size: u32, at: f32) -> Option<(Vec<u8>, u32, u32)> {
    let scale = size as f32 / animation.width.max(animation.height) as f32;
    let rendered = Renderer::default()
        .render_frame(animation, at, RenderConfig::new(Rgba8::TRANSPARENT, scale))
        .ok()?;
    // The renderer returns premultiplied pixels; PNG and GPUI's images hold straight alpha.
    let mut pixels = rendered.pixels;
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Some((pixels, rendered.width, rendered.height))
}

/// A parsed Lottie file played frame by frame. The official player (module 445972, rlottie) reads the frame count
/// and rate from the file: `lottie_frame_count` is the in point to the out point, and frame `i` is drawn at the in
/// point plus `i`.
pub struct Source {
    animation: Animation,
    frames: usize,
}

impl Source {
    pub fn parse(json: &[u8]) -> Option<Self> {
        let animation = parse(json)?;
        let frames = (animation.duration_frames().floor() as usize).max(1);
        (animation.frame_rate > 0.).then_some(Self { animation, frames })
    }

    pub fn frame_count(&self) -> usize {
        self.frames
    }

    /// The file's frame rate in frames per second.
    pub fn frame_rate(&self) -> f32 {
        self.animation.frame_rate
    }

    /// Frame `index` as straight-alpha RGBA pixels, `size` on the longer side.
    pub fn render(&self, index: usize, size: u32) -> Option<image::RgbaImage> {
        let at = self.animation.in_point + index.min(self.frames - 1) as f32;
        let (pixels, width, height) = straight_pixels(&self.animation, size, at)?;
        image::RgbaImage::from_raw(width, height, pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One 100 by 100 layer holding a filled red rectangle.
    const SQUARE: &str = r#"{"v":"5.5.7","fr":30,"ip":0,"op":30,"w":100,"h":100,"nm":"t","ddd":0,"assets":[],
        "layers":[{"ddd":0,"ind":1,"ty":4,"nm":"s","sr":1,"ks":{"o":{"a":0,"k":100},"r":{"a":0,"k":0},
        "p":{"a":0,"k":[50,50,0]},"a":{"a":0,"k":[0,0,0]},"s":{"a":0,"k":[100,100,100]}},"ao":0,
        "shapes":[{"ty":"gr","it":[{"ty":"rc","d":1,"s":{"a":0,"k":[60,60]},"p":{"a":0,"k":[0,0]},"r":{"a":0,"k":0}},
        {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100},"r":1},
        {"ty":"tr","p":{"a":0,"k":[0,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}],
        "nm":"g"}],"ip":0,"op":30,"st":0,"bm":0}]}"#;

    #[test]
    fn a_frame_renders_to_a_png_with_transparent_corners() {
        let png = first_frame_png(SQUARE.as_bytes(), 50).expect("renders");
        let decoded = image::load_from_memory(&png).expect("png").to_rgba8();
        assert_eq!(decoded.dimensions(), (50, 50));
        assert_eq!(decoded.get_pixel(0, 0)[3], 0);
        let middle = decoded.get_pixel(25, 25);
        assert!(middle[0] > 240 && middle[1] < 15 && middle[3] == 255);
    }

    /// `GRAVECORD_LOTTIE_FILE=<json> cargo test -p gravecord lottie::tests::frame_cost -- --ignored --nocapture`
    /// prints what one frame of a real sticker costs at the picker's and the message list's device sizes.
    #[test]
    #[ignore]
    fn frame_cost() {
        let path = std::env::var("GRAVECORD_LOTTIE_FILE").expect("GRAVECORD_LOTTIE_FILE");
        let json = std::fs::read(path).expect("read");
        let source = Source::parse(&json).expect("parses");
        println!(
            "frames={} fps={}",
            source.frame_count(),
            source.frame_rate()
        );
        for size in [96, 192, 160, 320] {
            let _ = source.render(0, size);
            let started = std::time::Instant::now();
            let runs = source.frame_count().min(60);
            for index in 0..runs {
                source.render(index, size).expect("renders");
            }
            let each = started.elapsed() / runs as u32;
            println!("size={size} per_frame={each:?} bytes={}", size * size * 4);
        }
    }

    #[test]
    fn bad_input_is_refused() {
        assert!(first_frame_png(b"not json", 50).is_none());
        assert!(first_frame_png(&vec![b' '; MAX_JSON + 1], 50).is_none());
    }
}
