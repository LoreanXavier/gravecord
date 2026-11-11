//! Lottie sticker art (`format_type` 3). The official client plays these with lottie-web on a canvas (module 148355);
//! here `rasterlottie` (pure Rust, MIT OR Apache-2.0, over tiny-skia) renders a frame to a PNG, off the UI thread.
//! The approach follows Serein's avatar worker, which uses the same crate.

use image::ImageEncoder;
use rasterlottie::{Animation, RasterFrame, RenderConfig, Renderer, Rgba8};

/// Microseconds spent parsing and preparing files, microseconds spent rendering frames, and how many of each, for the
/// capture log (`sticker state`).
pub static PARSE_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static PARSES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static RENDER_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static RENDERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

type Prepared = (rasterlottie::PreparedAnimation, Animation);

/// Render workers. A file always goes to the same worker (`id % WORKERS`), because a prepared animation is not
/// `Send`: it is parsed there once and every later batch for that file reuses it.
const WORKERS: usize = 2;
/// Estimated bytes of prepared animations one worker keeps (a prepared animation is about thirty times its JSON), and
/// how long an unused one stays.
const WORKER_BUDGET: usize = 48 * 1024 * 1024;
const PREPARED_IDLE: std::time::Duration = std::time::Duration::from_secs(30);

type Reply = Vec<(usize, Option<image::RgbaImage>)>;

struct Task {
    id: u64,
    json: std::sync::Arc<Vec<u8>>,
    frames: usize,
    indices: Vec<usize>,
    size: u32,
    reply: std::sync::mpsc::Sender<Reply>,
}

fn workers() -> &'static [std::sync::mpsc::Sender<Task>] {
    static POOL: std::sync::OnceLock<Vec<std::sync::mpsc::Sender<Task>>> =
        std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        (0..WORKERS)
            .map(|index| {
                let (send, receive) = std::sync::mpsc::channel::<Task>();
                std::thread::Builder::new()
                    .name(format!("lottie-{index}"))
                    .spawn(move || worker(receive))
                    .expect("a lottie worker thread");
                send
            })
            .collect()
    })
}

struct Kept {
    id: u64,
    bytes: usize,
    used: std::time::Instant,
    prepared: Prepared,
}

fn worker(receive: std::sync::mpsc::Receiver<Task>) {
    let mut kept: Vec<Kept> = Vec::new();
    loop {
        match receive.recv_timeout(PREPARED_IDLE) {
            Ok(task) => {
                let reply = task.reply.clone();
                let _ = reply.send(render_task(&mut kept, &task));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => kept.clear(),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn render_task(kept: &mut Vec<Kept>, task: &Task) -> Reply {
    use std::sync::atomic::Ordering::Relaxed;
    let now = std::time::Instant::now();
    kept.retain(|entry| now.duration_since(entry.used) < PREPARED_IDLE);
    let at = match kept.iter().position(|entry| entry.id == task.id) {
        Some(at) => {
            kept[at].used = now;
            Some(at)
        }
        None => {
            let started = std::time::Instant::now();
            let built = parse(&task.json).and_then(|animation| {
                Some((Renderer::default().prepare(&animation).ok()?, animation))
            });
            PARSE_US.fetch_add(started.elapsed().as_micros() as u64, Relaxed);
            PARSES.fetch_add(1, Relaxed);
            built.map(|prepared| {
                let bytes = task.json.len() * 30;
                kept.push(Kept {
                    id: task.id,
                    bytes,
                    used: now,
                    prepared,
                });
                kept.len() - 1
            })
        }
    };
    let result = at;
    let reply = task
        .indices
        .iter()
        .map(|index| {
            let started = std::time::Instant::now();
            let frame = result.and_then(|at| {
                let (prepared, animation) = &kept[at].prepared;
                let frame_at = animation.in_point + (*index).min(task.frames - 1) as f32;
                let rendered = prepared
                    .render_frame(frame_at, config_for(animation, task.size))
                    .ok()?;
                let (pixels, width, height) = unpremultiply(rendered);
                image::RgbaImage::from_raw(width, height, pixels)
            });
            RENDER_US.fetch_add(started.elapsed().as_micros() as u64, Relaxed);
            RENDERS.fetch_add(1, Relaxed);
            (*index, frame)
        })
        .collect();
    // Oldest first out until the estimate fits; the entry just used stays.
    while kept.iter().map(|entry| entry.bytes).sum::<usize>() > WORKER_BUDGET && kept.len() > 1 {
        let oldest = (0..kept.len())
            .filter(|index| Some(*index) != result)
            .min_by_key(|index| kept[*index].used);
        match oldest {
            Some(index) => {
                kept.remove(index);
            }
            None => break,
        }
    }
    reply
}

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
    let rendered = Renderer::default()
        .render_frame(animation, at, config_for(animation, size))
        .ok()?;
    Some(unpremultiply(rendered))
}

fn config_for(animation: &Animation, size: u32) -> RenderConfig {
    let scale = size as f32 / animation.width.max(animation.height) as f32;
    RenderConfig::new(Rgba8::TRANSPARENT, scale)
}

/// The renderer returns premultiplied pixels; PNG and GPUI's images hold straight alpha.
fn unpremultiply(rendered: RasterFrame) -> (Vec<u8>, u32, u32) {
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
    (pixels, rendered.width, rendered.height)
}

/// A Lottie file played frame by frame. The official player (module 445972, rlottie) reads the frame count
/// and rate from the file: `lottie_frame_count` is the in point to the out point, and frame `i` is drawn at the in
/// point plus `i`.
///
/// Only the file itself is kept. The parsed animation is about thirty times the size of the JSON, so it lives for
/// one render job and is dropped with it.
pub struct Source {
    /// Key of this file in its worker's prepared-animation cache.
    id: u64,
    json: std::sync::Arc<Vec<u8>>,
    frames: usize,
    frame_rate: f32,
}

impl Source {
    pub fn parse(json: &[u8]) -> Option<Self> {
        let animation = parse(json)?;
        let frames = (animation.duration_frames().floor() as usize).max(1);
        (animation.frame_rate > 0.).then(|| Self {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            json: std::sync::Arc::new(json.to_vec()),
            frames,
            frame_rate: animation.frame_rate,
        })
    }

    pub fn frame_count(&self) -> usize {
        self.frames
    }

    /// The file's frame rate in frames per second.
    pub fn frame_rate(&self) -> f32 {
        self.frame_rate
    }

    #[cfg(test)]
    /// Frame `index` as straight-alpha RGBA pixels, `size` on the longer side.
    pub fn render(&self, index: usize, size: u32) -> Option<image::RgbaImage> {
        self.render_many(&[index], size).pop()?.1
    }

    /// The frames `indices`, each as straight-alpha RGBA pixels, `size` on the longer side. The file is parsed and
    /// prepared once for the whole batch.
    pub fn render_many(
        &self,
        indices: &[usize],
        size: u32,
    ) -> Vec<(usize, Option<image::RgbaImage>)> {
        let (reply, answer) = std::sync::mpsc::channel();
        let pool = workers();
        let sent = pool[self.id as usize % pool.len()].send(Task {
            id: self.id,
            json: self.json.clone(),
            frames: self.frames,
            indices: indices.to_vec(),
            size,
            reply,
        });
        match sent.ok().and_then(|()| answer.recv().ok()) {
            Some(frames) => frames,
            None => indices.iter().map(|index| (*index, None)).collect(),
        }
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
