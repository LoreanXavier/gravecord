//! Checks that need the real machine: a camera, the screen, the audio engine, a GPU encoder. All `#[ignore]`d; run
//! them by hand with `cargo test -p dc-media -j 1 -- --ignored --nocapture --test-threads 1`. Nothing here writes a file,
//! opens a window, turns on a microphone or plays a sound.
#![cfg(windows)]
#![allow(clippy::chunks_exact_to_as_chunks)]

use dc_media::camera::{self, Camera};
use dc_media::decode::Decoder;
use dc_media::screen::{self, Resolution, Settings, SourceId, Worker};
use dc_media::{Backend, Layout};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn non_black(rgba: &[u8]) -> usize {
    rgba.chunks_exact(4)
        .filter(|p| p[0] > 16 || p[1] > 16 || p[2] > 16)
        .count()
}

#[test]
#[ignore = "needs a real camera device"]
fn the_camera_lists_devices() {
    let devices = camera::devices().unwrap();
    println!("camera devices: {devices:?}");
    assert!(!devices.is_empty());
}

#[test]
#[ignore = "needs the OBS Virtual Camera device"]
fn obs_virtual_camera_delivers_640x480_frames() {
    let devices = camera::devices().unwrap();
    let obs = devices
        .iter()
        .find(|d| d.name.contains("OBS"))
        .expect("OBS Virtual Camera is not listed");
    println!("using {obs:?}");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let lit = Arc::new(Mutex::new(0usize));
    let lit_sink = lit.clone();
    let started = Instant::now();
    let started_camera = Camera::start(Some(&obs.id), move |frame| {
        // Only a count is kept: the picture may show the owner's scene.
        let count = non_black(&frame.preview.data);
        let mut best = lit_sink.lock().unwrap();
        *best = (*best).max(count);
        let encoded = frame
            .encoded
            .as_ref()
            .map(|e| (e.data.len(), e.keyframe, e.timestamp));
        sink.lock().unwrap().push((
            frame.preview.width,
            frame.preview.height,
            frame.preview.layout,
            encoded,
        ));
    });
    // OBS's filter lists no video format until OBS itself has started its virtual camera. Connecting to it then
    // crashes the host process, so the crate refuses with an error instead; that refusal is the expected result here.
    let camera = match started_camera {
        Ok(camera) => camera,
        Err(error) if error.to_string().contains("offers no video format") => {
            println!("OBS virtual camera is not started in OBS: {error}");
            return;
        }
        Err(error) => panic!("camera did not start: {error}"),
    };
    println!("opened after {:?}", started.elapsed());
    std::thread::sleep(Duration::from_secs(3));
    camera.force_keyframe();
    std::thread::sleep(Duration::from_secs(1));
    println!("backend {:?}, error {:?}", camera.backend(), camera.error());
    drop(camera);
    let seen = seen.lock().unwrap();
    let encoded: Vec<_> = seen.iter().filter_map(|f| f.3).collect();
    println!(
        "frames {} in about 4 s, encoded {}, keyframes {}",
        seen.len(),
        encoded.len(),
        encoded.iter().filter(|e| e.1).count()
    );
    println!(
        "most non-black pixels in one frame: {}",
        lit.lock().unwrap()
    );
    assert!(!seen.is_empty(), "no frames arrived");
    assert!(
        seen.iter()
            .all(|f| f.0 == 640 && f.1 == 480 && f.2 == Layout::Rgba)
    );
    assert!(
        !encoded.is_empty() && encoded[0].1,
        "the first access unit must be a keyframe"
    );
}

#[test]
#[ignore = "lists the real displays and windows"]
fn sources_lists_displays() {
    let sources = screen::sources().unwrap();
    for s in &sources {
        println!(
            "{:?} {:?} {}x{} @{} icon {}",
            s.id,
            s.name,
            s.width,
            s.height,
            s.refresh_hz,
            s.icon.is_some()
        );
    }
    assert!(sources.iter().any(|s| matches!(s.id, SourceId::Display(_))));
}

#[test]
#[ignore = "captures the primary display"]
fn the_primary_display_has_a_thumbnail() {
    let sources = screen::sources().unwrap();
    let display = sources
        .iter()
        .find(|s| s.name.contains("primary"))
        .or_else(|| {
            sources
                .iter()
                .find(|s| matches!(s.id, SourceId::Display(_)))
        })
        .unwrap();
    let thumb = screen::thumbnail(&display.id, 320, 180).unwrap();
    let rgba = thumb.to_rgba().unwrap();
    println!(
        "thumbnail {}x{}, non-black pixels {}",
        thumb.width,
        thumb.height,
        non_black(&rgba)
    );
    assert!(thumb.width <= 320 && thumb.height <= 180 && thumb.width > 0);
    assert!(non_black(&rgba) > 0);
}

#[test]
#[ignore = "captures the primary display and uses the GPU encoder"]
fn a_five_second_display_share_encodes_and_decodes() {
    let sources = screen::sources().unwrap();
    let display = sources
        .iter()
        .find(|s| s.name.contains("primary"))
        .or_else(|| {
            sources
                .iter()
                .find(|s| matches!(s.id, SourceId::Display(_)))
        })
        .unwrap();
    let settings = Settings::new(display, Resolution::P720, 30, true, false).unwrap();
    println!("sharing {display:?} as {settings:?}");
    let units = Arc::new(Mutex::new(Vec::new()));
    let sink = units.clone();
    let started = Instant::now();
    let worker = Worker::start(
        settings,
        move |unit| sink.lock().unwrap().push(unit),
        |_| panic!("audio was off"),
    )
    .unwrap();
    println!("capture started after {:?}", started.elapsed());
    let begin = Instant::now();
    std::thread::sleep(Duration::from_secs(2));
    worker.force_keyframe();
    worker.set_bitrate(1_000_000);
    std::thread::sleep(Duration::from_secs(3));
    let preview = worker.take_preview();
    let elapsed = begin.elapsed().as_secs_f64();
    let (backend, fell_back, captured, error) = (
        worker.backend(),
        worker.fell_back(),
        worker.captured(),
        worker.error(),
    );
    let dropping = Instant::now();
    drop(worker);
    println!("worker dropped in {:?}", dropping.elapsed());
    let units = units.lock().unwrap();
    let bytes: usize = units.iter().map(|u| u.data.len()).sum();
    let keyframes = units.iter().filter(|u| u.keyframe).count();
    println!(
        "backend {backend:?} fell_back {fell_back} error {error:?}: captured {captured}, encoded {} units \
         ({:.1}/s), {keyframes} keyframes, {} kbit/s over {elapsed:.1}s",
        units.len(),
        units.len() as f64 / elapsed,
        (bytes as f64 * 8.0 / elapsed / 1000.0) as u64
    );
    if let Some(p) = &preview {
        println!("preview {}x{}", p.width, p.height);
    }
    assert!(!units.is_empty() && units[0].keyframe);
    assert!(units.windows(2).all(|w| w[1].timestamp >= w[0].timestamp));

    let mut decoder = Decoder::new().unwrap();
    println!("decoder backend {:?}", decoder.backend());
    let decoding = Instant::now();
    let mut pictures = 0;
    let mut lit = 0;
    let mut checked = 0;
    let mut size = (0, 0);
    let mut decode_time = Duration::ZERO;
    for (index, unit) in units.iter().enumerate() {
        let at = Instant::now();
        let decoded = decoder.decode(&unit.data).unwrap();
        decode_time += at.elapsed();
        if let Some(picture) = decoded {
            pictures += 1;
            size = (picture.width, picture.height);
            // The conversion is slow in a debug build, so only every tenth picture is checked for content.
            if index % 10 == 0 {
                checked += 1;
                if non_black(&picture.to_rgba().unwrap()) > 0 {
                    lit += 1;
                }
            }
        }
    }
    println!("pure decode time {decode_time:?}");
    println!("decode loop took {:?}", decoding.elapsed());
    println!("decoded {pictures} pictures at {size:?}; {lit} of {checked} checked were not black");
    assert!(pictures > 0 && checked > 0 && lit == checked);
    assert_eq!(size, (settings.width, settings.height));
    let _ = Backend::Hardware;
}

#[test]
#[ignore = "opens the system audio loopback"]
fn three_seconds_of_system_audio_have_the_right_format() {
    let chunks = Arc::new(Mutex::new((0usize, 0usize, true)));
    let sink = chunks.clone();
    let capture = screen::AudioCapture::start(move |chunk| {
        let mut state = sink.lock().unwrap();
        state.0 += 1;
        state.1 += chunk.samples.len();
        state.2 &= chunk.samples.len() % 2 == 0
            && chunk.samples.len() <= screen::audio::MAX_AUDIO_SAMPLES
            && chunk
                .samples
                .iter()
                .all(|s| s.is_finite() && s.abs() <= 1.0);
    })
    .unwrap();
    std::thread::sleep(Duration::from_secs(3));
    let finished = capture.finished();
    drop(capture);
    let (count, samples, ok) = *chunks.lock().unwrap();
    println!(
        "{count} chunks, {samples} samples ({:.2} s of audio), format ok {ok}, finished early {finished}",
        samples as f64 / 2.0 / 48_000.0
    );
    assert!(ok && !finished);
}
