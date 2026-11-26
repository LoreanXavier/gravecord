//! GifMaker (Equicord `src/equicordplugins/gifMaker`, GPL-3.0-or-later): a GIF made from a picture, an animated GIF
//! or WebP, or a video, at a chosen size and with an optional caption. The original draws each frame on a browser
//! canvas (`encodeFrames` in its `utils/encoder.ts`): the frame stretched to the chosen size under a caption band, or
//! with a speech bubble drawn over its top, then encodes them with `gifenc`. Here the frames are decoded natively
//! (the `image` decoders, H.264 through openh264), stretched, the caption drawn with tiny-skia from the glyph
//! outlines of the app's own font, and each frame is encoded as soon as it is made, so only one frame is held. Not
//! part of the pinned client.

use super::quoter::Face;
use dc_state::Message;
use image::{AnimationDecoder, RgbaImage};
use tiny_skia::{Color, FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// `clamp(…, 32, 1024, 32)` of the Width and Height boxes.
pub(crate) const MIN_SIDE: u32 = 32;
pub(crate) const MAX_SIDE: u32 = 1024;
/// `MAX_FRAMES`.
pub(crate) const MAX_FRAMES: usize = 200;
/// `INTERNAL_FPS`: a video is sampled 30 times a second.
const INTERNAL_FPS: f64 = 30.;
/// The `speechbubble` tip's width along the curve (`tipWidth`).
const TIP_WIDTH: f32 = 0.2;

/// `CaptionMode`: the three buttons under Captions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    None,
    Caption,
    Bubble,
}

impl Mode {
    pub(crate) const ALL: [Self; 3] = [Self::None, Self::Caption, Self::Bubble];

    /// The value the original stores (`lastCaptionMode`).
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Caption => "caption",
            Self::Bubble => "speechbubble",
        }
    }

    pub(crate) fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|mode| mode.key() == key)
            .unwrap_or(Self::None)
    }

    /// The button's name (`CaptionDefinition.name`).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Caption => "Caption",
            Self::Bubble => "Bubble",
        }
    }
}

/// `GifMakerOptions` (without the font family, which is the app's own here).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Options {
    pub width: u32,
    pub height: u32,
    pub mode: Mode,
    pub text: String,
    pub size: f32,
    /// Where the bubble's tip points, in the GIF's pixels.
    pub tip: (f32, f32),
    /// Where along the bubble's edge the tip starts, 0 to 0.8.
    pub tip_base: f32,
}

/// What a menu row makes a GIF of (`getMediaInfo`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Media {
    pub url: String,
    pub video: bool,
    pub size: Option<(u32, u32)>,
}

/// `MEDIA_TYPES`.
const MEDIA_TYPES: [&str; 8] = [
    "image/png",
    "image/jpeg",
    "image/jpg",
    "image/webp",
    "image/gif",
    "video/mp4",
    "video/webm",
    "video/quicktime",
];

/// The address's path without its query or fragment, lower case.
fn path_of(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase()
}

fn has_extension(url: &str, extensions: &[&str]) -> bool {
    let path = path_of(url);
    extensions
        .iter()
        .any(|extension| path.ends_with(&format!(".{extension}")))
}

/// `GIF_EXT_RE`.
fn is_gif(url: &str) -> bool {
    has_extension(url, &["gif"])
}

/// `MEDIA_EXT_RE` and `VIDEO_EXT_RE`: a link to a picture or a video.
pub(crate) fn media_of_link(url: &str) -> Option<Media> {
    has_extension(
        url,
        &["png", "jpg", "jpeg", "webp", "gif", "mp4", "webm", "mov"],
    )
    .then(|| Media {
        url: url.to_owned(),
        video: has_extension(url, &["mp4", "webm", "mov"]),
        size: None,
    })
}

/// `getMediaInfo` for a message: its first picture or video attachment, a `.gif` link in its text, then its embeds
/// (a GIF picture, a video, any picture).
pub(crate) fn media_of_message(message: &Message) -> Option<Media> {
    if let Some(attachment) = message.attachments.iter().find(|attachment| {
        attachment
            .content_type
            .as_deref()
            .is_some_and(|kind| MEDIA_TYPES.iter().any(|known| kind.starts_with(known)))
    }) && !attachment.proxy_url.is_empty()
    {
        let kind = attachment.content_type.as_deref().unwrap_or_default();
        return Some(Media {
            url: attachment.proxy_url.clone(),
            video: kind.starts_with("video/") && !is_gif(&attachment.proxy_url),
            size: attachment.width.zip(attachment.height),
        });
    }
    if let Some(link) = message
        .content
        .split_whitespace()
        .map(|word| word.trim_end_matches([',', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '.']))
        .find(|word| (word.starts_with("https://") || word.starts_with("http://")) && is_gif(word))
    {
        return Some(Media {
            url: link.to_owned(),
            video: false,
            size: None,
        });
    }
    for embed in &message.embeds {
        let picture = embed.image.as_ref().or(embed.thumbnail.as_ref());
        let picture_url =
            picture.map(|media| media.proxy_url.clone().unwrap_or_else(|| media.url.clone()));
        let size = picture.map(|media| (media.width, media.height));
        if let Some(gif) = [embed.url.clone(), picture_url.clone()]
            .into_iter()
            .flatten()
            .find(|url| is_gif(url))
        {
            return Some(Media {
                url: gif,
                video: false,
                size,
            });
        }
        if let Some(video) = &embed.video {
            return Some(Media {
                url: video.proxy_url.clone().unwrap_or_else(|| video.url.clone()),
                video: true,
                size: Some((video.width, video.height)),
            });
        }
        if let Some(url) = picture_url {
            return Some(Media {
                url,
                video: false,
                size,
            });
        }
    }
    None
}

/// `getInitialSize`: the source's size, scaled down to fit `max` when it is larger; without a source size the
/// stored one.
pub(crate) fn initial_size(
    max: (u32, u32),
    source: Option<(u32, u32)>,
    stored: (u32, u32),
) -> (u32, u32) {
    match source {
        Some((width, height)) if width > 0 && height > 0 => {
            if width <= max.0 && height <= max.1 {
                return (width, height);
            }
            let scale =
                (f64::from(max.0) / f64::from(width)).min(f64::from(max.1) / f64::from(height));
            (
                (f64::from(width) * scale).round() as u32,
                (f64::from(height) * scale).round() as u32,
            )
        }
        _ => stored,
    }
}

/// `clamp(Number(value), 32, 1024, 32)`: nothing or zero becomes 32.
pub(crate) fn clamp_side(value: f64) -> u32 {
    if !value.is_finite() || value == 0. {
        return MIN_SIDE;
    }
    value.clamp(f64::from(MIN_SIDE), f64::from(MAX_SIDE)) as u32
}

/// `measureTextLines`: the words of `text` onto lines at most `max_width` wide.
fn text_lines(face: &Face, text: &str, size: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split(' ') {
        let test = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if face.measure(&test, size) > max_width && !current.is_empty() {
            lines.push(std::mem::replace(&mut current, word.to_owned()));
        } else {
            current = test;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// `getCaptionHeight`: the band over the picture in Caption mode.
pub(crate) fn caption_height(face: &Face, options: &Options) -> u32 {
    if options.mode != Mode::Caption || options.text.is_empty() {
        return 0;
    }
    let lines = text_lines(
        face,
        &options.text,
        options.size,
        options.width as f32 - 20.,
    );
    (lines.len() as f32 * options.size * 1.2 + 20.).ceil() as u32
}

fn solid(r: u8, g: u8, b: u8) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(r, g, b, 255));
    paint.anti_alias = true;
    paint
}

/// `bezierPoint`.
fn bezier(t: f32, start: (f32, f32), control: (f32, f32), end: (f32, f32)) -> (f32, f32) {
    let u = 1. - t;
    (
        u * u * start.0 + 2. * u * t * control.0 + t * t * end.0,
        u * u * start.1 + 2. * u * t * control.1 + t * t * end.1,
    )
}

/// `moveAway`.
fn move_away(point: (f32, f32), from: (f32, f32), distance: f32) -> (f32, f32) {
    let (dx, dy) = (point.0 - from.0, point.1 - from.1);
    let length = (dx * dx + dy * dy).sqrt();
    if length == 0. {
        return point;
    }
    let scale = distance / length;
    (point.0 + dx * scale, point.1 + dy * scale)
}

fn path(build: impl FnOnce(&mut PathBuilder)) -> Option<Path> {
    let mut builder = PathBuilder::new();
    build(&mut builder);
    builder.finish()
}

/// `renderSpeechbubble`: a white cap over the top tenth with a gentle arc, outlined in black, and a tip pointing at
/// `tip` from `tip_base` along the arc.
fn speech_bubble(pixmap: &mut Pixmap, width: f32, height: f32, tip: (f32, f32), tip_base: f32) {
    let start = (0., height * 0.1);
    let control = (width * 0.5, height * 0.2);
    let end = (width, height * 0.1);
    let white = solid(255, 255, 255);
    let black = solid(0, 0, 0);
    let stroke = Stroke {
        width: 2.,
        ..Stroke::default()
    };
    let identity = Transform::identity();
    if let Some(cap) = path(|p| {
        p.move_to(0., 0.);
        p.line_to(start.0, start.1);
        p.quad_to(control.0, control.1, end.0, end.1);
        p.line_to(width, 0.);
        p.close();
    }) {
        pixmap.fill_path(&cap, &white, FillRule::Winding, identity, None);
    }
    if let Some(arc) = path(|p| {
        p.move_to(start.0, start.1);
        p.quad_to(control.0, control.1, end.0, end.1);
    }) {
        pixmap.stroke_path(&arc, &black, &stroke, identity, None);
    }
    let base1 = bezier(tip_base, start, control, end);
    let base2 = bezier(tip_base + TIP_WIDTH, start, control, end);
    let (a, b) = (move_away(base1, tip, 5.), move_away(base2, tip, 5.));
    if let Some(back) = path(|p| {
        p.move_to(a.0, a.1);
        p.line_to(tip.0, tip.1);
        p.line_to(b.0, b.1);
        p.close();
    }) {
        pixmap.fill_path(&back, &white, FillRule::Winding, identity, None);
    }
    if let Some(outline) = path(|p| {
        p.move_to(base1.0, base1.1);
        p.line_to(tip.0, tip.1);
        p.line_to(base2.0, base2.1);
    }) {
        pixmap.stroke_path(&outline, &black, &stroke, identity, None);
    }
}

/// What is drawn over every frame (the caption band with its text, or the bubble), premultiplied, the GIF's size.
fn overlay(face: &Face, options: &Options, caption: u32) -> Option<Pixmap> {
    let (width, height) = (options.width, options.height + caption);
    let mut pixmap = Pixmap::new(width, height)?;
    match options.mode {
        Mode::None => {}
        Mode::Caption if caption > 0 => {
            // `captionCaption.render`: white band, black lines centred, `textBaseline = "top"`.
            let rect = Rect::from_xywh(0., 0., width as f32, caption as f32)?;
            pixmap.fill_rect(rect, &solid(255, 255, 255), Transform::identity(), None);
            let size = options.size;
            let lines = text_lines(face, &options.text, size, width as f32 - 20.);
            let line_height = size * 1.2;
            let area = lines.len() as f32 * line_height + 20.;
            let top = (caption as f32 - area) / 2.;
            let black = solid(0, 0, 0);
            for (index, line) in lines.iter().enumerate() {
                let x = (width as f32 - face.measure(line, size)) / 2.;
                let y = top + 10. + index as f32 * line_height + face.ascent(size);
                if let Some(glyphs) = face.outline(line, size, x, y) {
                    pixmap.fill_path(
                        &glyphs,
                        &black,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
        Mode::Caption => {}
        Mode::Bubble => speech_bubble(
            &mut pixmap,
            width as f32,
            height as f32,
            options.tip,
            options.tip_base,
        ),
    }
    Some(pixmap)
}

/// The frame stretched to the chosen size under the caption band, with the overlay drawn over it (source-over).
fn compose(frame: &RgbaImage, options: &Options, caption: u32, over: &Pixmap) -> RgbaImage {
    let stretched = image::imageops::resize(
        frame,
        options.width,
        options.height,
        image::imageops::FilterType::Triangle,
    );
    let mut out = RgbaImage::new(options.width, options.height + caption);
    image::imageops::replace(&mut out, &stretched, 0, i64::from(caption));
    for (pixel, top) in out.pixels_mut().zip(over.pixels()) {
        let alpha = u32::from(top.alpha());
        if alpha == 0 {
            continue;
        }
        let rest = 255 - alpha;
        let [r, g, b, a] = pixel.0;
        // Premultiplied: the overlay's colour plus the frame's under what the overlay leaves.
        let under = |channel: u8| u32::from(channel) * u32::from(a) / 255 * rest / 255;
        let out_alpha = alpha + u32::from(a) * rest / 255;
        let mix = |own: u8, channel: u8| {
            let premultiplied = u32::from(own) + under(channel);
            (premultiplied * 255 / out_alpha.max(1)).min(255) as u8
        };
        pixel.0 = [
            mix(top.red(), r),
            mix(top.green(), g),
            mix(top.blue(), b),
            out_alpha.min(255) as u8,
        ];
    }
    out
}

/// What the bytes are (`sniffType`): the file decides, not its address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Mp4,
    Gif,
    Webp,
    Picture,
}

pub(crate) fn sniff(bytes: &[u8]) -> Source {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        Source::Mp4
    } else if bytes.starts_with(b"GIF") {
        Source::Gif
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Source::Webp
    } else {
        Source::Picture
    }
}

const CANCELLED: &str = "cancelled";

/// `createGif`: the GIF of `bytes` with `options`. `stale` is asked before each frame; a newer request stops this one.
pub(crate) fn make(
    bytes: &[u8],
    face: &Face,
    options: &Options,
    stale: &dyn Fn() -> bool,
) -> Result<Vec<u8>, String> {
    let caption = caption_height(face, options);
    let over = overlay(face, options, caption).ok_or("Invalid size")?;
    let mut out = Vec::new();
    // Frames are written as they come, each with its own palette; the GIF loops (`gifenc`'s default).
    let mut encoder = image::codecs::gif::GifEncoder::new_with_speed(&mut out, 10);
    encoder
        .set_repeat(image::codecs::gif::Repeat::Infinite)
        .map_err(|error| error.to_string())?;
    let mut written = 0usize;
    let mut frame = |picture: &RgbaImage, delay: u32| -> Result<(), String> {
        if stale() {
            return Err(CANCELLED.into());
        }
        let delay = image::Delay::from_numer_denom_ms(delay.max(1), 1);
        encoder
            .encode_frame(image::Frame::from_parts(
                compose(picture, options, caption, &over),
                0,
                0,
                delay,
            ))
            .map_err(|error| error.to_string())?;
        written += 1;
        Ok(())
    };
    match sniff(bytes) {
        Source::Mp4 => video_frames(bytes, &mut frame)?,
        Source::Gif => {
            let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))
                .map_err(|error| error.to_string())?;
            animation(decoder.into_frames(), &mut frame)?;
        }
        Source::Webp => {
            let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes))
                .map_err(|error| error.to_string())?;
            if decoder.has_animation() {
                animation(decoder.into_frames(), &mut frame)?;
            } else {
                still(bytes, &mut frame)?;
            }
        }
        Source::Picture => still(bytes, &mut frame)?,
    }
    drop(encoder);
    if written == 0 {
        return Err("No frames".into());
    }
    Ok(out)
}

/// Whether an error is only a newer request taking over.
pub(crate) fn is_cancel(error: &str) -> bool {
    error == CANCELLED
}

fn still(
    bytes: &[u8],
    frame: &mut dyn FnMut(&RgbaImage, u32) -> Result<(), String>,
) -> Result<(), String> {
    let picture = image::load_from_memory(bytes)
        .map_err(|error| error.to_string())?
        .to_rgba8();
    // A single frame keeps the original's default delay.
    frame(&picture, (1000. / INTERNAL_FPS).round() as u32)
}

/// The frames of an animated GIF or WebP, at their own delays, at most `MAX_FRAMES`.
fn animation(
    frames: image::Frames<'_>,
    frame: &mut dyn FnMut(&RgbaImage, u32) -> Result<(), String>,
) -> Result<(), String> {
    for decoded in frames.take(MAX_FRAMES) {
        let decoded = decoded.map_err(|error| error.to_string())?;
        let (numer, denom) = decoded.delay().numer_denom_ms();
        let delay = numer / denom.max(1);
        frame(decoded.buffer(), delay)?;
    }
    Ok(())
}

/// The sample times of a video of `duration` seconds (`createGifFromVideo`): 30 a second, at most `MAX_FRAMES`, each
/// shown for the same delay.
pub(crate) fn video_samples(duration: f64) -> (usize, f64, u32) {
    let count = ((duration * INTERNAL_FPS).floor() as usize).clamp(1, MAX_FRAMES);
    let interval = duration / count as f64;
    (count, interval, (interval * 1000.).round() as u32)
}

/// A video's frames at the sample times: every frame is decoded (the next ones are predicted from it), the one
/// showing at each sample time is stretched and encoded, the rest dropped at once.
fn video_frames(
    bytes: &[u8],
    frame: &mut dyn FnMut(&RgbaImage, u32) -> Result<(), String>,
) -> Result<(), String> {
    use openh264::formats::YUVSource;
    let track = crate::spinner::H264Track::read(bytes)?;
    let durations = track.durations().ok_or("Invalid video")?;
    let duration: f64 = durations.iter().map(std::time::Duration::as_secs_f64).sum();
    if track.len() == 0 || duration <= 0. {
        return Err("Invalid video".into());
    }
    let (count, interval, delay) = video_samples(duration);
    let mut decoder = openh264::decoder::Decoder::new().map_err(|error| error.to_string())?;
    let mut sent = 0usize;
    let mut shown = 0usize;
    let mut clock = 0f64;
    let mut take = |yuv: &openh264::decoder::DecodedYUV<'_>,
                    frame: &mut dyn FnMut(&RgbaImage, u32) -> Result<(), String>|
     -> Result<(), String> {
        let ends = clock
            + durations
                .get(shown)
                .map_or(interval, std::time::Duration::as_secs_f64);
        shown += 1;
        // The samples that fall while this frame shows.
        let mut picture = None;
        while sent < count && (sent as f64 * interval) < ends {
            if picture.is_none() {
                let (width, height) = yuv.dimensions();
                if width * height > 4096 * 4096 {
                    return Err("Video too large".into());
                }
                let mut rgba = vec![0; width * height * 4];
                yuv.write_rgba8(&mut rgba);
                picture = RgbaImage::from_raw(width as u32, height as u32, rgba);
            }
            let Some(picture) = picture.as_ref() else {
                return Err("Invalid frame".into());
            };
            frame(picture, delay)?;
            sent += 1;
        }
        clock = ends;
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
            take(&yuv, frame)?;
        }
    }
    for yuv in decoder
        .flush_remaining()
        .map_err(|error| error.to_string())?
    {
        take(&yuv, frame)?;
    }
    if sent == 0 {
        return Err("No frames".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face_bytes() -> Option<Vec<u8>> {
        crate::fonts::load_cached()?.into_iter().next()
    }

    fn options(mode: Mode) -> Options {
        Options {
            width: 64,
            height: 48,
            mode,
            text: "hello there".into(),
            size: 12.,
            tip: (40., 40.),
            tip_base: 0.1,
        }
    }

    #[test]
    fn sizes_fit_the_maximum_and_are_clamped() {
        assert_eq!(
            initial_size((1024, 576), Some((640, 480)), (256, 256)),
            (640, 480)
        );
        assert_eq!(
            initial_size((1024, 576), Some((1920, 1080)), (256, 256)),
            (1024, 576)
        );
        assert_eq!(initial_size((1024, 576), None, (300, 200)), (300, 200));
        assert_eq!(clamp_side(0.), 32);
        assert_eq!(clamp_side(5000.), 1024);
        assert_eq!(clamp_side(10.), 32);
        assert_eq!(clamp_side(500.), 500);
    }

    #[test]
    fn links_and_bytes_are_told_apart() {
        assert!(media_of_link("https://x.test/a.MP4?x=1").is_some_and(|media| media.video));
        assert!(media_of_link("https://x.test/a.gif").is_some_and(|media| !media.video));
        assert!(media_of_link("https://x.test/a.txt").is_none());
        assert_eq!(sniff(b"\0\0\0\x18ftypmp42...."), Source::Mp4);
        assert_eq!(sniff(b"GIF89a"), Source::Gif);
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Source::Webp);
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n"), Source::Picture);
    }

    #[test]
    fn a_video_is_sampled_thirty_times_a_second_up_to_two_hundred() {
        assert_eq!(video_samples(2.), (60, 2. / 60., 33));
        let (count, _, delay) = video_samples(20.);
        assert_eq!((count, delay), (200, 100));
        assert_eq!(video_samples(0.01).0, 1);
    }

    #[test]
    fn a_still_becomes_a_one_frame_gif_with_its_caption_band() {
        let Some(bytes) = face_bytes() else {
            return;
        };
        let face = Face::parse(&bytes).unwrap();
        let mut png = Vec::new();
        RgbaImage::from_pixel(10, 10, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let caption = options(Mode::Caption);
        let band = caption_height(&face, &caption);
        assert!(band > 20, "{band}");
        let gif = make(&png, &face, &caption, &|| false).unwrap();
        let decoded = image::load_from_memory(&gif).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (64, 48 + band));
        // The band is white at its corner and the picture red under it.
        assert_eq!(decoded.get_pixel(0, 0).0[..3], [255, 255, 255]);
        let red = decoded.get_pixel(32, band + 24).0;
        assert!(red[0] > 200 && red[1] < 60, "{red:?}");
        // A newer request stops the work.
        assert!(make(&png, &face, &caption, &|| true).is_err_and(|error| is_cancel(&error)));
    }

    #[test]
    fn the_bubble_covers_the_top_and_frames_keep_their_delays() {
        let Some(bytes) = face_bytes() else {
            return;
        };
        let face = Face::parse(&bytes).unwrap();
        // A two-frame GIF: blue then green, 50ms and 120ms.
        let mut source = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut source);
            for (colour, ms) in [([0, 0, 255, 255], 50), ([0, 255, 0, 255], 120)] {
                encoder
                    .encode_frame(image::Frame::from_parts(
                        RgbaImage::from_pixel(8, 8, image::Rgba(colour)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(ms, 1),
                    ))
                    .unwrap();
            }
        }
        let bubble = options(Mode::Bubble);
        let gif = make(&source, &face, &bubble, &|| false).unwrap();
        let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&gif)).unwrap();
        let frames: Vec<_> = decoder.into_frames().collect::<Result<_, _>>().unwrap();
        assert_eq!(frames.len(), 2);
        let delays: Vec<_> = frames.iter().map(|f| f.delay().numer_denom_ms()).collect();
        assert_eq!(delays, [(50, 1), (120, 1)]);
        let first = frames[0].buffer();
        assert_eq!(first.dimensions(), (64, 48));
        // White at the top, the frame's blue at the bottom.
        assert_eq!(first.get_pixel(32, 1).0[..3], [255, 255, 255]);
        assert_eq!(first.get_pixel(5, 46).0[..3], [0, 0, 255]);
    }
}
