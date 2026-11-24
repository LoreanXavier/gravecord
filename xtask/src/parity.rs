//! `cargo xtask parity [--screen X]`: renders gravecord offscreen on the hidden desktop and diffs it against a golden.
//!
//! Pixel diff only so far. The layout-tree diff and calibrated thresholds (section 8.3) are open:
//! this prints the measured mismatch and never reports a pass.

use crate::{headless, layout};
use anyhow::{Context, Result, bail};
use image::{Rgba, RgbaImage};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// A channel differing by this much counts as different. Below it is antialiasing noise.
pub(crate) const CHANNEL_TOLERANCE: u8 = 24;
const RENDER_TIMEOUT: Duration = Duration::from_secs(60);

struct Region {
    name: &'static str,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

const REGIONS: [Region; 7] = [
    Region {
        name: "titlebar",
        x: 0,
        y: 0,
        width: 1280,
        height: 32,
    },
    Region {
        name: "rail",
        x: 0,
        y: 32,
        width: 72,
        height: 620,
    },
    Region {
        name: "sidebar",
        x: 72,
        y: 32,
        width: 303,
        height: 620,
    },
    Region {
        name: "user panel",
        x: 0,
        y: 652,
        width: 375,
        height: 68,
    },
    Region {
        name: "header",
        x: 375,
        y: 32,
        width: 905,
        height: 50,
    },
    Region {
        name: "messages",
        x: 375,
        y: 82,
        width: 905,
        height: 570,
    },
    Region {
        name: "composer",
        x: 375,
        y: 652,
        width: 905,
        height: 68,
    },
];

pub(crate) fn differs(a: &Rgba<u8>, b: &Rgba<u8>) -> bool {
    (0..3).any(|channel| a[channel].abs_diff(b[channel]) > CHANNEL_TOLERANCE)
}

fn mismatch(a: &RgbaImage, b: &RgbaImage, region: &Region) -> (u64, u64) {
    let (mut different, mut total) = (0, 0);
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            total += 1;
            if differs(a.get_pixel(x, y), b.get_pixel(x, y)) {
                different += 1;
            }
        }
    }
    (different, total)
}

/// Golden on the left half of each pixel pair's meaning: red where they differ, dimmed golden elsewhere.
fn diff_image(ours: &RgbaImage, golden: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(ours.width(), ours.height(), |x, y| {
        let (a, b) = (ours.get_pixel(x, y), golden.get_pixel(x, y));
        if differs(a, b) {
            Rgba([255, 40, 40, 255])
        } else {
            Rgba([b[0] / 3, b[1] / 3, b[2] / 3, 255])
        }
    })
}

/// The name of a run's own copy of the app.
fn is_run_copy(name: &str) -> bool {
    name.starts_with("gravecord-run-") && name.ends_with(".exe")
}

/// Each run starts its own copy of the built app, beside it. A running app holds its file, so an app that is still
/// running, or stuck, would otherwise stop the next build from replacing `gravecord.exe` for every other run. Copies
/// of finished runs are removed here; one that is still running cannot be, and is left for a later run.
pub(crate) fn run_copy(built: &Path) -> Result<PathBuf> {
    let dir = built.parent().context("the built app has no folder")?;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if is_run_copy(&entry.file_name().to_string_lossy()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let copy = dir.join(format!("gravecord-run-{}-{stamp}.exe", std::process::id()));
    // Another build may be writing the file at this moment: try again shortly.
    let mut tries = 0;
    loop {
        match fs::copy(built, &copy) {
            Ok(_) => return Ok(copy),
            Err(error) if tries < 10 => {
                tries += 1;
                let _ = error;
                sleep(Duration::from_millis(500));
            }
            Err(error) => {
                return Err(error).with_context(|| format!("copying {}", built.display()));
            }
        }
    }
}

/// Where Cargo puts its output: `CARGO_TARGET_DIR` when set (a second build directory lets two runs build at the
/// same time), else `target` in the repository.
pub(crate) fn target_dir(repo: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) if !dir.is_empty() => {
            let dir = PathBuf::from(dir);
            if dir.is_absolute() {
                dir
            } else {
                repo.join(dir)
            }
        }
        _ => repo.join("target"),
    }
}

fn render(repo: &Path, out: &Path, script: Option<&str>) -> Result<()> {
    let status = Command::new("cargo")
        .args([
            "build",
            "-p",
            "gravecord",
            "--features",
            "capture",
            "--quiet",
        ])
        .current_dir(repo)
        .status()?;
    if !status.success() {
        bail!("gravecord did not build");
    }
    let _ = fs::remove_file(out);
    let exe = run_copy(&target_dir(repo).join("debug").join("gravecord.exe"))?;
    let mut args = vec![
        "--capture".to_owned(),
        out.display().to_string(),
        // `GRAVECORD_SETTLE_MS` shortens the wait after the script: the call view hides its controls two seconds after
        // the pointer stops, so its frames with the controls showing are taken sooner.
        std::env::var("GRAVECORD_SETTLE_MS").unwrap_or_else(|_| "2000".to_owned()),
    ];
    if let Some(script) = script {
        args.extend(["--script".to_owned(), script.to_owned()]);
    }
    headless::spawn(&exe, &args, repo)?;
    let started = Instant::now();
    while !out.exists() {
        // `GRAVECORD_RENDER_SECONDS`: a script that waits on a live call needs longer than the default.
        let limit = std::env::var("GRAVECORD_RENDER_SECONDS")
            .ok()
            .and_then(|text| text.parse().ok())
            .map_or(RENDER_TIMEOUT, Duration::from_secs);
        if started.elapsed() > limit {
            bail!("no frame after {RENDER_TIMEOUT:?}");
        }
        sleep(Duration::from_millis(200));
    }
    sleep(Duration::from_millis(300));
    Ok(())
}

pub fn run(repo: &Path, args: &[&str]) -> Result<()> {
    let mut screen = "channel";
    let mut variant = "text-darker-cozy";
    let mut script = None;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().copied().context("flag without a value")?;
        match *flag {
            "--screen" => screen = value,
            "--variant" => variant = value,
            "--script" => script = Some(value),
            other => bail!(
                "usage: cargo xtask parity [--screen X] [--variant V] [--script S], got {other}"
            ),
        }
    }
    let design = repo.join("design");
    let golden_path: PathBuf = [
        design.as_path(),
        Path::new("golden/windows"),
        Path::new(screen),
        Path::new(&format!("{variant}@1.png")),
    ]
    .iter()
    .collect();
    let dir = design.join("parity");
    fs::create_dir_all(&dir)?;
    let frame = dir.join(format!("{screen}-{variant}-latest.png"));
    render(repo, &frame, script)?;

    let golden = image::open(&golden_path)
        .with_context(|| format!("reading {}", golden_path.display()))?
        .to_rgba8();
    let ours = image::open(&frame).context("reading the frame")?.to_rgba8();
    if golden.dimensions() != ours.dimensions() {
        bail!(
            "frame is {:?}, golden is {:?}",
            ours.dimensions(),
            golden.dimensions()
        );
    }
    let (mut different, mut total) = (0, 0);
    for region in &REGIONS {
        let (d, t) = mismatch(&ours, &golden, region);
        different += d;
        total += t;
        println!("{:<11} {:>6.2}%", region.name, 100.0 * d as f64 / t as f64);
    }
    println!(
        "{:<11} {:>6.2}%  ({screen}, tolerance {CHANNEL_TOLERANCE})",
        "total",
        100.0 * different as f64 / total as f64
    );
    diff_image(&ours, &golden).save(dir.join(format!("{screen}-{variant}-diff.png")))?;

    let dom = design
        .join("dom")
        .join(screen)
        .join(format!("{variant}.json"));
    let maps = repo.join("tests").join("parity");
    let map = map_for(&maps, screen, variant);
    let (checked, mismatches) = layout::compare(&dom, &map, &frame.with_extension("layout.json"))?;
    println!(
        "layout tree: {checked} elements, {} differ",
        mismatches.len()
    );
    for mismatch in &mismatches {
        println!(
            "  {:<18} ours {:?} official {:?}",
            mismatch.id, mismatch.ours, mismatch.official
        );
    }
    Ok(())
}

/// The most specific layout map for a variant: `friends-tab-add.json` for `tab-add-friend-darker-cozy` when it
/// exists, else `friends-tab.json`, else `friends.json`. Longer prefixes of the variant's segments win.
pub(crate) fn map_for(maps: &Path, screen: &str, variant: &str) -> PathBuf {
    let segments: Vec<&str> = variant.split('-').collect();
    (1..=segments.len())
        .rev()
        .map(|count| maps.join(format!("{screen}-{}.json", segments[..count].join("-"))))
        .find(|path| path.exists())
        .unwrap_or_else(|| maps.join(format!("{screen}.json")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_runs_own_copy_is_swept() {
        assert!(super::is_run_copy("gravecord-run-1234-1700000000000.exe"));
        assert!(!super::is_run_copy("gravecord.exe"));
        assert!(!super::is_run_copy("gravecord-run-1234.pdb"));
        assert!(!super::is_run_copy("xtask.exe"));
    }

    use super::*;

    #[test]
    fn longest_variant_prefix_picks_the_map() {
        let dir = std::env::temp_dir().join("gravecord-parity-maps");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("friends-tab.json"), "{}").unwrap();
        fs::write(dir.join("friends-tab-add.json"), "{}").unwrap();
        assert_eq!(
            map_for(&dir, "friends", "tab-add-friend-darker-cozy"),
            dir.join("friends-tab-add.json")
        );
        assert_eq!(
            map_for(&dir, "friends", "tab-all-darker-cozy"),
            dir.join("friends-tab.json")
        );
        assert_eq!(
            map_for(&dir, "friends", "online-darker-cozy"),
            dir.join("friends.json")
        );
    }

    #[test]
    fn identical_images_have_no_mismatch() {
        let image = RgbaImage::from_pixel(1280, 720, Rgba([10, 20, 30, 255]));
        assert_eq!(mismatch(&image, &image, &REGIONS[0]).0, 0);
    }

    #[test]
    fn counts_only_changes_above_the_tolerance() {
        let a = RgbaImage::from_pixel(1280, 720, Rgba([100, 100, 100, 255]));
        let mut b = a.clone();
        b.put_pixel(0, 0, Rgba([100 + CHANNEL_TOLERANCE, 100, 100, 255]));
        b.put_pixel(1, 0, Rgba([100 + CHANNEL_TOLERANCE + 1, 100, 100, 255]));
        assert_eq!(mismatch(&a, &b, &REGIONS[0]), (1, 1280 * 32));
    }

    #[test]
    fn regions_tile_the_window() {
        let area: u32 = REGIONS
            .iter()
            .map(|region| region.width * region.height)
            .sum();
        assert_eq!(area, 1280 * 720);
    }
}
