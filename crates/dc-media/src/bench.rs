//! Measurements of the Go Live picture path at 2560x1440: what each stage costs per picture on this machine. They are
//! `#[ignore]`d because they open the hardware encoder and decoder and take seconds; run them with
//! `cargo test -p dc-media bench -- --ignored --nocapture`. Nothing is captured and no device is opened: the pictures
//! are synthetic.

use crate::convert::{Chroma, PixelOrder, Raw, to_yuv420};
use crate::decode::Decoder;
use crate::encode::{Backend, Config, Content, Encoder, Profile};
use crate::{Layout, Picture, h264};
use std::time::{Duration, Instant};

const WIDTH: usize = 2560;
const HEIGHT: usize = 1440;
const FPS: u32 = 60;
const BITRATE: u32 = 9_000_000;

/// A desktop-like BGRA picture: a diagonal ramp, a moving block and fine noise text-like detail.
fn desktop(index: usize) -> Vec<u8> {
    let mut bgra = vec![0u8; WIDTH * HEIGHT * 4];
    let mut state = 0x2545_F491u32 ^ index as u32;
    for (i, pixel) in bgra.chunks_exact_mut(4).enumerate() {
        let (x, y) = (i % WIDTH, i / WIDTH);
        let ramp = ((x + y + index * 8) % 512) as u8 / 2;
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let text = if (y / 18).is_multiple_of(3) && (x / 9).is_multiple_of(2) {
            (state & 0x7f) as u8
        } else {
            0
        };
        let block = (x / 200 + index).is_multiple_of(13) && (y / 200) % 3 == 1;
        pixel.copy_from_slice(&[
            ramp.saturating_add(text),
            if block { 220 } else { ramp / 2 },
            text,
            255,
        ]);
    }
    bgra
}

fn per(total: Duration, count: usize) -> f64 {
    total.as_secs_f64() * 1000.0 / count.max(1) as f64
}

#[test]
#[ignore = "measures the 1440p60 picture path with the hardware encoder and decoder"]
fn bench_go_live_1440p60_picture_path() {
    const PICTURES: usize = 120;
    let sources: Vec<Vec<u8>> = (0..8).map(desktop).collect();
    let mut nv12 = vec![0u8; WIDTH * HEIGHT * 3 / 2];

    // Capture readback to the encoder's input: BGRA -> NV12.
    let started = Instant::now();
    for index in 0..PICTURES {
        let raw = Raw {
            width: WIDTH,
            height: HEIGHT,
            stride: WIDTH * 4,
            order: PixelOrder::Bgra,
            data: &sources[index % sources.len()],
        };
        to_yuv420(raw, WIDTH, HEIGHT, Chroma::Interleaved, &mut nv12).unwrap();
    }
    println!(
        "convert BGRA->NV12 {WIDTH}x{HEIGHT}: {:.2} ms per picture",
        per(started.elapsed(), PICTURES)
    );
    // The uploaded thumbnail, still cut down to 640 wide on the encode thread.
    let started = Instant::now();
    for index in 0..30 {
        let raw = Raw {
            width: WIDTH,
            height: HEIGHT,
            stride: WIDTH * 4,
            order: PixelOrder::Bgra,
            data: &sources[index % sources.len()],
        };
        std::hint::black_box(crate::convert::downscale_rgba(raw, 640, 640).unwrap());
    }
    println!(
        "preview BGRA->640 wide RGBA: {:.2} ms per picture",
        per(started.elapsed(), 30)
    );
    let started = Instant::now();
    for _ in 0..PICTURES {
        std::hint::black_box(nv12.clone());
    }
    println!(
        "local full-size NV12 handoff: {:.2} ms per picture",
        per(started.elapsed(), PICTURES)
    );
    let local = Picture {
        width: WIDTH as u32,
        height: HEIGHT as u32,
        layout: Layout::Nv12,
        data: nv12.clone(),
    };
    let started = Instant::now();
    for _ in 0..30 {
        std::hint::black_box(local.to_bgra().unwrap());
    }
    println!(
        "local full-size NV12->BGRA UI worker: {:.2} ms per picture",
        per(started.elapsed(), 30)
    );
    let inputs: Vec<Vec<u8>> = sources
        .iter()
        .map(|bgra| {
            let mut out = vec![0u8; WIDTH * HEIGHT * 3 / 2];
            let raw = Raw {
                width: WIDTH,
                height: HEIGHT,
                stride: WIDTH * 4,
                order: PixelOrder::Bgra,
                data: bgra,
            };
            to_yuv420(raw, WIDTH, HEIGHT, Chroma::Interleaved, &mut out).unwrap();
            out
        })
        .collect();

    // The encoder the share would use (hardware when the machine has one).
    let config = Config {
        width: WIDTH as u32,
        height: HEIGHT as u32,
        fps: FPS,
        bitrate: BITRATE,
        max_bytes: h264::MAX_ACCESS_UNIT,
        profile: Profile::Main,
        content: Content::Screen,
    };
    let mut encoder = Encoder::new(config).unwrap();
    let backend = encoder.backend();
    let chroma = encoder.chroma();
    let mut units = Vec::new();
    let started = Instant::now();
    let mut worst = Duration::ZERO;
    for index in 0..PICTURES {
        let input = match chroma {
            Chroma::Interleaved => inputs[index % inputs.len()].clone(),
            Chroma::Planar => {
                crate::convert::nv12_to_i420(&inputs[index % inputs.len()], WIDTH, HEIGHT).unwrap()
            }
        };
        let one = Instant::now();
        if let Some(output) = encoder.encode(&input, chroma, index == 0).unwrap() {
            units.push(output);
        }
        worst = worst.max(one.elapsed());
    }
    let encode = started.elapsed();
    let bytes: usize = units.iter().map(|unit| unit.data.len()).sum();
    let keyframes: Vec<usize> = units
        .iter()
        .filter(|unit| unit.keyframe)
        .map(|unit| unit.data.len())
        .collect();
    println!(
        "encode {backend:?}: {:.2} ms per picture (worst {:.2} ms), {} units, {:.0} kB average, keyframes {:?} bytes, {:.2} Mb/s at {FPS} fps",
        per(encode, PICTURES),
        worst.as_secs_f64() * 1000.0,
        units.len(),
        bytes as f64 / units.len().max(1) as f64 / 1000.0,
        keyframes,
        bytes as f64 * 8.0 * f64::from(FPS) / units.len().max(1) as f64 / 1e6,
    );
    assert!(!units.is_empty());

    for (name, decoder) in [
        ("media foundation", Decoder::hardware()),
        ("software", Decoder::software()),
    ] {
        let Ok(mut decoder) = decoder else {
            println!("decode {name}: unavailable");
            continue;
        };
        let started = Instant::now();
        let mut pictures: Vec<Picture> = Vec::new();
        let mut worst = Duration::ZERO;
        let mut first_out = None;
        for (index, unit) in units.iter().enumerate() {
            let one = Instant::now();
            if let Ok(Some(picture)) = decoder.decode(&unit.data) {
                first_out.get_or_insert(index);
                pictures.push(picture);
            }
            worst = worst.max(one.elapsed());
        }
        let elapsed = started.elapsed();
        println!(
            "  worst unit {:.2} ms, first picture after unit {first_out:?}",
            worst.as_secs_f64() * 1000.0
        );
        let Some(last) = pictures.last() else {
            println!("decode {name}: no pictures");
            continue;
        };
        println!(
            "decode {name} ({:?}): {:.2} ms per unit, {} pictures {}x{} {:?}, {:.1} MB per picture",
            decoder.backend(),
            per(elapsed, units.len()),
            pictures.len(),
            last.width,
            last.height,
            last.layout,
            last.data.len() as f64 / 1e6
        );
        // Picture quality: luma PSNR of every decoded picture against what was encoded.
        if pictures.len() == PICTURES {
            let luma = WIDTH * HEIGHT;
            let mut total = 0.0;
            for (index, picture) in pictures.iter().enumerate() {
                let source = &inputs[index % inputs.len()][..luma];
                let error: f64 = picture.data[..luma]
                    .iter()
                    .zip(source)
                    .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
                    .sum::<f64>()
                    / luma as f64;
                total += 10.0 * (255.0f64 * 255.0 / error.max(1e-9)).log10();
            }
            println!("  luma PSNR {:.2} dB", total / PICTURES as f64);
        }
        // What the call view does with every picture before gpui sees it.
        let started = Instant::now();
        let count = pictures.len().min(20);
        for picture in pictures.iter().take(count) {
            let mut rgba = picture.to_rgba().unwrap();
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            std::hint::black_box(&rgba);
        }
        println!(
            "tile image (to_rgba + BGRA swap): {:.2} ms per picture, {:.1} MB per picture",
            per(started.elapsed(), count),
            (WIDTH * HEIGHT * 4) as f64 / 1e6
        );
        if Layout::Rgba != last.layout {
            let started = Instant::now();
            let mut out = vec![0u8; WIDTH * HEIGHT * 4];
            for picture in pictures.iter().take(count) {
                picture.write_bgra(&mut out).unwrap();
                std::hint::black_box(&out);
            }
            println!(
                "tile image (write_bgra): {:.2} ms per picture",
                per(started.elapsed(), count)
            );
        }
    }
    if backend == Backend::Hardware {
        println!("(hardware encoder in use)");
    }
}

/// The clip thumbnail decodes the first keyframe with openh264 only (`Decoder::software`), while playback takes the
/// hardware decoder. This checks that a keyframe from the encoder Go Live and clips use (hardware Main profile when
/// the machine has one) decodes on the software path to a picture that is not black. Synthetic pictures only.
#[test]
#[ignore = "opens the hardware encoder; run with --ignored"]
fn a_hardware_keyframe_decodes_on_the_software_path() {
    const W: usize = 1280;
    const H: usize = 720;
    let mut bgra = vec![0u8; W * H * 4];
    for (i, pixel) in bgra.chunks_exact_mut(4).enumerate() {
        let (x, y) = (i % W, i / W);
        pixel.copy_from_slice(&[(x % 256) as u8, (y % 256) as u8, 160, 255]);
    }
    let mut nv12 = vec![0u8; W * H * 3 / 2];
    let raw = Raw {
        width: W,
        height: H,
        stride: W * 4,
        order: PixelOrder::Bgra,
        data: &bgra,
    };
    to_yuv420(raw, W, H, Chroma::Interleaved, &mut nv12).unwrap();
    let config = Config {
        width: W as u32,
        height: H as u32,
        fps: 30,
        bitrate: 6_000_000,
        max_bytes: h264::MAX_ACCESS_UNIT,
        profile: Profile::Main,
        content: Content::Screen,
    };
    let mut encoder = Encoder::new(config).unwrap();
    let chroma = encoder.chroma();
    println!("encoder backend {:?}", encoder.backend());
    let mut keyframe = None;
    for index in 0..30 {
        let input = if chroma == Chroma::Interleaved {
            nv12.clone()
        } else {
            let mut planar = vec![0u8; W * H * 3 / 2];
            let raw = Raw {
                width: W,
                height: H,
                stride: W * 4,
                order: PixelOrder::Bgra,
                data: &bgra,
            };
            to_yuv420(raw, W, H, Chroma::Planar, &mut planar).unwrap();
            planar
        };
        if let Some(output) = encoder.encode(&input, chroma, index == 0).unwrap()
            && h264::is_keyframe(&output.data)
        {
            keyframe = Some(output.data);
            break;
        }
    }
    let keyframe = keyframe.expect("a keyframe within 30 pictures");
    let profile_idc = h264::nals(&keyframe)
        .find(|nal| nal.first().is_some_and(|byte| byte & 0x1f == 7))
        .and_then(|sps| sps.get(1).copied());
    println!(
        "keyframe {} bytes, profile_idc {profile_idc:?}",
        keyframe.len()
    );
    let mut software = Decoder::software().unwrap();
    let decoded = software.decode(&keyframe);
    println!(
        "software decode: {:?}",
        decoded
            .as_ref()
            .map(|p| p.as_ref().map(|p| (p.width, p.height)))
    );
    let picture = decoded
        .ok()
        .flatten()
        .expect("openh264 decodes the hardware keyframe");
    let rgba = picture.to_rgba().unwrap();
    let lit = rgba
        .chunks_exact(4)
        .filter(|p| p[0] > 16 || p[1] > 16 || p[2] > 16)
        .count();
    assert!(
        lit * 2 > rgba.len() / 4,
        "the decoded keyframe is mostly black"
    );
}
