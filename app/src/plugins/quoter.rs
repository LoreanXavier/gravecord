//! Quoter (Equicord `src/equicordplugins/quoter`, GPL-3.0-or-later): the quote picture made from a message. The
//! original draws it on a browser canvas (`createQuoteImage` in its `utils.tsx`): 1200 by 600 on black, the author's
//! avatar over the left 600 by 600, an optional grayscale pass, a black fade over the avatar's right part, the quote
//! centred in the right part at the largest size that fits, then "- name" and "@username" under it and an optional
//! watermark in the corner; saved as PNG or as a one-frame GIF. Here the same picture is drawn with tiny-skia from
//! the glyph outlines of the app's own font. Not part of the pinned client.

use std::collections::HashMap;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, Path, PathBuilder, Pixmap, PixmapPaint,
    Point, Rect, Shader, SpreadMode, Transform,
};

/// `CANVAS_CONFIG`.
pub(crate) const WIDTH: u32 = 1200;
pub(crate) const HEIGHT: u32 = 600;
const QUOTE_AREA_WIDTH: f32 = 520.;
const QUOTE_AREA_X: f32 = 640.;
const MAX_CONTENT_HEIGHT: f32 = 480.;
/// `FONT_SIZES`.
const INITIAL: f32 = 42.;
const MINIMUM: f32 = 18.;
const DECREMENT: f32 = 2.;
const LINE_HEIGHT: f32 = 1.25;
const AUTHOR: f32 = 0.60;
const USERNAME: f32 = 0.45;
const AUTHOR_MINIMUM: f32 = 22.;
const USERNAME_MINIMUM: f32 = 18.;
const WATERMARK_SIZE: f32 = 18.;
/// `SPACING`.
const AUTHOR_TOP: f32 = 60.;
const USERNAME_GAP: f32 = 10.;
const GRADIENT_WIDTH: f32 = 400.;
const WATERMARK_PADDING: f32 = 20.;
/// `CUSTOM_EMOJI_*`: an emoji is 1.15 of the text size, its top 0.85 of that above the baseline.
const EMOJI_SIZE: f32 = 1.15;
const EMOJI_BASELINE: f32 = 0.85;
/// The custom emoji's place in the text (`CUSTOM_EMOJI_PLACEHOLDER`).
pub(crate) const EMOJI_MARK: char = '\u{FFFC}';
/// The watermark's most characters (`watermark.slice(0, 32)`).
pub(crate) const WATERMARK_MAX: usize = 32;

/// A custom emoji of the quote: `<:name:id>` or `<a:name:id>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CustomEmoji {
    pub id: u64,
    pub name: String,
    pub animated: bool,
}

/// `extractCustomEmojis`: the text with every custom emoji replaced by [`EMOJI_MARK`], and the emojis in order.
pub(crate) fn extract_custom_emojis(text: &str) -> (String, Vec<CustomEmoji>) {
    let mut out = String::with_capacity(text.len());
    let mut emojis = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let parsed = (|| {
            let (animated, body) = if let Some(body) = tail.strip_prefix("<a:") {
                (true, body)
            } else {
                (false, tail.strip_prefix("<:")?)
            };
            let colon = body.find(':')?;
            let name = &body[..colon];
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return None;
            }
            let after = &body[colon + 1..];
            let close = after.find('>')?;
            let id = &after[..close];
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let used = tail.len() - after.len() + close + 1;
            Some((
                CustomEmoji {
                    id: id.parse().ok()?,
                    name: name.to_owned(),
                    animated,
                },
                used,
            ))
        })();
        match parsed {
            Some((emoji, used)) => {
                out.push(EMOJI_MARK);
                emojis.push(emoji);
                rest = &tail[used..];
            }
            None => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    (out, emojis)
}

/// `generateFileNamePreview`: the first ten characters of the first six words.
pub(crate) fn file_name_preview(message: &str) -> String {
    let words: Vec<&str> = message.split(' ').collect();
    let preview = words[..words.len().min(6)].join(" ");
    preview.chars().take(10).collect()
}

/// The file the picture is saved or sent as: `<preview> - <username>.<png|gif>`.
pub(crate) fn file_name(message: &str, username: &str, gif: bool) -> String {
    let name = format!(
        "{} - {username}.{}",
        file_name_preview(message),
        if gif { "gif" } else { "png" }
    );
    // A name the file system cannot hold loses those characters.
    name.chars()
        .filter(|c| {
            !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control()
        })
        .collect()
}

/// A loaded face: the font file, read when it is drawn.
pub(crate) struct Face<'a> {
    face: ttf_parser::Face<'a>,
}

impl<'a> Face<'a> {
    pub(crate) fn parse(data: &'a [u8]) -> Option<Self> {
        Some(Self {
            face: ttf_parser::Face::parse(data, 0).ok()?,
        })
    }

    fn scale(&self, size: f32) -> f32 {
        size / f32::from(self.face.units_per_em())
    }

    fn advance(&self, c: char, size: f32) -> f32 {
        let glyph = self.face.glyph_index(c).unwrap_or(ttf_parser::GlyphId(0));
        f32::from(self.face.glyph_hor_advance(glyph).unwrap_or(0)) * self.scale(size)
    }

    /// `measureText(text).width`: the advances of the characters.
    pub(crate) fn measure(&self, text: &str, size: f32) -> f32 {
        text.chars().map(|c| self.advance(c, size)).sum()
    }

    /// The outlines of `text` with its baseline's start at `x`, `y`.
    fn outline(&self, text: &str, size: f32, x: f32, y: f32) -> Option<Path> {
        let mut builder = PathBuilder::new();
        let scale = self.scale(size);
        let mut pen_x = x;
        for c in text.chars() {
            let glyph = self.face.glyph_index(c).unwrap_or(ttf_parser::GlyphId(0));
            let mut pen = Pen {
                builder: &mut builder,
                scale,
                x: pen_x,
                baseline: y,
            };
            self.face.outline_glyph(glyph, &mut pen);
            pen_x += f32::from(self.face.glyph_hor_advance(glyph).unwrap_or(0)) * scale;
        }
        builder.finish()
    }
}

struct Pen<'a> {
    builder: &'a mut PathBuilder,
    scale: f32,
    x: f32,
    baseline: f32,
}

impl Pen<'_> {
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + x * self.scale, self.baseline - y * self.scale)
    }
}

impl ttf_parser::OutlineBuilder for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.builder.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.builder.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x, y) = self.point(x, y);
        self.builder.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x2, y2) = self.point(x2, y2);
        let (x, y) = self.point(x, y);
        self.builder.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.builder.close();
    }
}

/// `measureTextWithCustomEmojis`.
fn measure_with_emojis(face: &Face, text: &str, size: f32) -> f32 {
    text.split(EMOJI_MARK)
        .map(|part| face.measure(part, size))
        .sum::<f32>()
        + text.matches(EMOJI_MARK).count() as f32 * size * EMOJI_SIZE
}

/// `calculateTextLines`: words onto lines of at most `max_width`; a word wider than a line is cut by characters.
fn text_lines(face: &Face, text: &str, size: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for word in text.split(' ') {
        if measure_with_emojis(face, word, size) > max_width {
            if !current.is_empty() {
                lines.push(current.join(" "));
                current.clear();
            }
            let mut chunk = String::new();
            for c in word.chars() {
                let mut test = chunk.clone();
                test.push(c);
                if measure_with_emojis(face, &test, size) > max_width {
                    if !chunk.is_empty() {
                        lines.push(std::mem::take(&mut chunk));
                    }
                    chunk.push(c);
                } else {
                    chunk = test;
                }
            }
            if !chunk.is_empty() {
                lines.push(chunk);
            }
        } else {
            let mut test = current.clone();
            test.push(word);
            if measure_with_emojis(face, &test.join(" "), size) > max_width && !current.is_empty() {
                lines.push(current.join(" "));
                current = vec![word];
            } else {
                current.push(word);
            }
        }
    }
    if !current.is_empty() {
        lines.push(current.join(" "));
    }
    lines
}

/// `FontSizeCalculation`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fit {
    pub size: f32,
    pub line_height: f32,
    pub author_size: f32,
    pub username_size: f32,
    pub lines: Vec<String>,
    pub total_height: f32,
}

/// `calculateOptimalFontSize`: from 42 down by 2 to 18, the first size whose lines and author rows fit in 480.
pub(crate) fn fit(face: &Face, quote: &str) -> Fit {
    let measure = |size: f32, author: f32, username: f32| {
        let lines = text_lines(face, quote, size, QUOTE_AREA_WIDTH);
        let line_height = size * LINE_HEIGHT;
        let total =
            lines.len() as f32 * line_height + AUTHOR_TOP + author + USERNAME_GAP + username;
        Fit {
            size,
            line_height,
            author_size: author,
            username_size: username,
            lines,
            total_height: total,
        }
    };
    let mut size = INITIAL;
    while size >= MINIMUM {
        let found = measure(
            size,
            AUTHOR_MINIMUM.max(size * AUTHOR),
            USERNAME_MINIMUM.max(size * USERNAME),
        );
        if found.total_height <= MAX_CONTENT_HEIGHT {
            return found;
        }
        size -= DECREMENT;
    }
    measure(MINIMUM, AUTHOR_MINIMUM, USERNAME_MINIMUM)
}

/// What the picture shows.
pub(crate) struct Quote<'a> {
    /// The quote with its mentions already written as `@username` (`fixUpQuote`).
    pub text: &'a str,
    /// `globalName || username`.
    pub name: &'a str,
    pub username: &'a str,
    /// The avatar's pixels, straight RGBA, and its side.
    pub avatar: Option<(&'a [u8], u32, u32)>,
    pub grayscale: bool,
    pub watermark: Option<&'a str>,
    /// The ids of the custom emojis [`EMOJI_MARK`] stands for, in order ([`extract_custom_emojis`]).
    pub emoji_ids: &'a [u64],
    /// The custom emojis' pictures by id: straight RGBA and their size.
    pub emoji_images: &'a HashMap<u64, (Vec<u8>, u32, u32)>,
}

fn paint_of(rgb: (u8, u8, u8)) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(Color::from_rgba8(rgb.0, rgb.1, rgb.2, 255)),
        anti_alias: true,
        ..Paint::default()
    }
}

/// A picture of straight RGBA into a pixmap (tiny-skia keeps premultiplied pixels).
fn pixmap_of(rgba: &[u8], width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    for (out, pixel) in pixmap
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(rgba.as_chunks::<4>().0)
    {
        let alpha = u16::from(pixel[3]);
        for channel in 0..3 {
            out[channel] = (u16::from(pixel[channel]) * alpha / 255) as u8;
        }
        out[3] = pixel[3];
    }
    Some(pixmap)
}

fn draw_scaled(target: &mut Pixmap, picture: &Pixmap, x: f32, y: f32, width: f32, height: f32) {
    let transform = Transform::from_row(
        width / picture.width() as f32,
        0.,
        0.,
        height / picture.height() as f32,
        x,
        y,
    );
    target.draw_pixmap(
        0,
        0,
        picture.as_ref(),
        &PixmapPaint {
            quality: tiny_skia::FilterQuality::Bilinear,
            ..PixmapPaint::default()
        },
        transform,
        None,
    );
}

/// `applyGrayscaleFilter`: white drawn over everything in the `saturation` mode, which keeps each pixel's luminosity
/// (0.3, 0.59, 0.11) with no saturation.
fn grayscale(pixmap: &mut Pixmap) {
    for pixel in pixmap.data_mut().as_chunks_mut::<4>().0 {
        let lum =
            0.3 * f32::from(pixel[0]) + 0.59 * f32::from(pixel[1]) + 0.11 * f32::from(pixel[2]);
        let lum = lum.round().clamp(0., 255.) as u8;
        pixel[0] = lum;
        pixel[1] = lum;
        pixel[2] = lum;
    }
}

/// `createQuoteImage` up to the file: the picture's pixels, straight RGBA (it is opaque).
pub(crate) fn render(regular: &Face, italic: &Face, quote: &Quote) -> Option<Vec<u8>> {
    let mut pixmap = Pixmap::new(WIDTH, HEIGHT)?;
    pixmap.fill(Color::BLACK);
    let side = HEIGHT as f32;
    if let Some((rgba, width, height)) = quote.avatar
        && let Some(avatar) = pixmap_of(rgba, width, height)
    {
        draw_scaled(&mut pixmap, &avatar, 0., 0., side, side);
    }
    if quote.grayscale {
        grayscale(&mut pixmap);
    }
    // `drawGradientOverlay`: transparent to black from x 200 to 600.
    let start = side - GRADIENT_WIDTH;
    if let Some(shader) = LinearGradient::new(
        Point::from_xy(start, 0.),
        Point::from_xy(side, 0.),
        vec![
            GradientStop::new(0., Color::from_rgba8(0, 0, 0, 0)),
            GradientStop::new(1., Color::BLACK),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) && let Some(rect) = Rect::from_xywh(start, 0., GRADIENT_WIDTH, side)
    {
        let paint = Paint {
            shader,
            ..Paint::default()
        };
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }

    let calculation = fit(regular, quote.text);
    let white = paint_of((255, 255, 255));
    let gray = paint_of((0x88, 0x88, 0x88));
    let fill_text =
        |pixmap: &mut Pixmap, face: &Face, text: &str, size: f32, x: f32, y: f32, paint: &Paint| {
            if let Some(path) = face.outline(text, size, x, y) {
                pixmap.fill_path(&path, paint, FillRule::Winding, Transform::identity(), None);
            }
        };
    // `drawQuoteText`: each line centred in the quote area, emojis as pictures (a white square when missing).
    let emoji_size = calculation.size * EMOJI_SIZE;
    let mut emoji_index = 0;
    let mut y = (side - calculation.total_height) / 2.;
    for line in &calculation.lines {
        let offset = (QUOTE_AREA_WIDTH - measure_with_emojis(regular, line, calculation.size)) / 2.;
        let mut x = QUOTE_AREA_X + offset;
        y += calculation.line_height;
        let mut parts = line.split(EMOJI_MARK).peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                fill_text(&mut pixmap, regular, part, calculation.size, x, y, &white);
                x += regular.measure(part, calculation.size);
            }
            if parts.peek().is_some() {
                let picture = quote
                    .emoji_ids
                    .get(emoji_index)
                    .and_then(|id| quote.emoji_images.get(id))
                    .and_then(|(rgba, width, height)| pixmap_of(rgba, *width, *height));
                emoji_index += 1;
                match picture {
                    Some(picture) => draw_scaled(
                        &mut pixmap,
                        &picture,
                        x,
                        y - emoji_size * EMOJI_BASELINE,
                        emoji_size,
                        emoji_size,
                    ),
                    None => fill_text(
                        &mut pixmap,
                        regular,
                        "\u{25A1}",
                        calculation.size,
                        x,
                        y,
                        &white,
                    ),
                }
                x += emoji_size;
            }
        }
    }
    // `drawAuthorInfo`.
    let author = format!("- {}", quote.name);
    let author_x =
        QUOTE_AREA_X + (QUOTE_AREA_WIDTH - italic.measure(&author, calculation.author_size)) / 2.;
    let author_y = y + AUTHOR_TOP;
    fill_text(
        &mut pixmap,
        italic,
        &author,
        calculation.author_size,
        author_x,
        author_y,
        &white,
    );
    let username = format!("@{}", quote.username);
    let username_x = QUOTE_AREA_X
        + (QUOTE_AREA_WIDTH - regular.measure(&username, calculation.username_size)) / 2.;
    let username_y = author_y + USERNAME_GAP + calculation.username_size;
    fill_text(
        &mut pixmap,
        regular,
        &username,
        calculation.username_size,
        username_x,
        username_y,
        &gray,
    );
    // `drawWatermark`.
    if let Some(watermark) = quote.watermark.filter(|text| !text.is_empty()) {
        let text: String = watermark.chars().take(WATERMARK_MAX).collect();
        let x = WIDTH as f32 - regular.measure(&text, WATERMARK_SIZE) - WATERMARK_PADDING;
        let y = side - WATERMARK_PADDING;
        fill_text(&mut pixmap, regular, &text, WATERMARK_SIZE, x, y, &gray);
    }
    Some(pixmap.take())
}

/// The file: PNG (`canvas.toBlob`), or a GIF of one frame on a palette of 256 (`gifenc`'s `quantize` and
/// `applyPalette`).
pub(crate) fn encode(rgba: Vec<u8>, gif: bool) -> Option<Vec<u8>> {
    let picture = image::RgbaImage::from_raw(WIDTH, HEIGHT, rgba)?;
    let mut out = Vec::new();
    if gif {
        let mut encoder = image::codecs::gif::GifEncoder::new_with_speed(&mut out, 10);
        encoder.encode_frame(image::Frame::new(picture)).ok()?;
        drop(encoder);
    } else {
        picture
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_emojis_become_marks() {
        let (text, emojis) =
            extract_custom_emojis("hi <:wave:123> and <a:dance_2:456>! <:bad:x> <3");
        assert_eq!(text, "hi \u{FFFC} and \u{FFFC}! <:bad:x> <3");
        assert_eq!(
            emojis,
            vec![
                CustomEmoji {
                    id: 123,
                    name: "wave".into(),
                    animated: false
                },
                CustomEmoji {
                    id: 456,
                    name: "dance_2".into(),
                    animated: true
                },
            ]
        );
    }

    #[test]
    fn file_names_follow_the_preview() {
        assert_eq!(
            file_name_preview("one two three four five six seven"),
            "one two th"
        );
        assert_eq!(file_name_preview("hi"), "hi");
        assert_eq!(
            // The preview is cut at ten characters first, then loses what a file name cannot hold.
            file_name("what? a: test", "ada", false),
            "what a t - ada.png"
        );
        assert_eq!(file_name("x", "ada", true), "x - ada.gif");
    }

    #[test]
    fn grayscale_keeps_the_luminosity() {
        let mut pixmap = Pixmap::new(1, 1).unwrap();
        pixmap.fill(Color::from_rgba8(255, 0, 0, 255));
        grayscale(&mut pixmap);
        assert_eq!(&pixmap.data()[..4], &[77, 77, 77, 255]);
    }

    /// With the app's font from the capture cache: a long quote shrinks, a short one keeps 42, and the picture is
    /// drawn and encoded both ways.
    #[test]
    fn the_quote_fits_and_draws_with_the_cached_font() {
        let Some(faces) = crate::fonts::load_cached() else {
            return;
        };
        let regular = Face::parse(&faces[0]).unwrap();
        let italic = Face::parse(&faces[1]).unwrap();
        assert_eq!(fit(&regular, "short").size, INITIAL);
        let long = "word ".repeat(300);
        let shrunk = fit(&regular, long.trim());
        assert_eq!(shrunk.size, MINIMUM);
        let some =
            "a moderately long quote that wraps over a few lines of the right half ".repeat(4);
        let mid = fit(&regular, some.trim());
        assert!(mid.size < INITIAL && mid.size > MINIMUM);
        assert!(mid.total_height <= MAX_CONTENT_HEIGHT);
        assert!(
            mid.lines
                .iter()
                .all(|line| regular.measure(line, mid.size) <= QUOTE_AREA_WIDTH)
        );
        let images = HashMap::new();
        let avatar: Vec<u8> = [200u8, 200, 200, 255].repeat(4 * 4);
        let quote = Quote {
            text: "hello \u{FFFC} world",
            name: "Ada",
            username: "ada",
            avatar: Some((&avatar, 4, 4)),
            grayscale: true,
            watermark: Some("Made with Gravecord"),
            emoji_ids: &[],
            emoji_images: &images,
        };
        let rgba = render(&regular, &italic, &quote).unwrap();
        assert_eq!(rgba.len(), (WIDTH * HEIGHT * 4) as usize);
        // The avatar's top left is its own gray; the far right edge stays black.
        assert_eq!(&rgba[..3], &[200, 200, 200]);
        assert_eq!(
            &rgba[(WIDTH as usize - 1) * 4..(WIDTH as usize - 1) * 4 + 3],
            &[0, 0, 0]
        );
        let png = encode(rgba.clone(), false).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        let gif = encode(rgba, true).unwrap();
        assert!(gif.starts_with(b"GIF8"));
    }
}
