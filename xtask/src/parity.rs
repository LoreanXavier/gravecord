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
const CHANNEL_TOLERANCE: u8 = 24;
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

fn differs(a: &Rgba<u8>, b: &Rgba<u8>) -> bool {
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
    let exe = repo.join("target").join("debug").join("gravecord.exe");
    let mut args = vec![
        "--capture".to_owned(),
        out.display().to_string(),
        "2000".to_owned(),
    ];
    if let Some(script) = script {
        args.extend(["--script".to_owned(), script.to_owned()]);
    }
    headless::spawn(&exe, &args, repo)?;
    let started = Instant::now();
    while !out.exists() {
        if started.elapsed() > RENDER_TIMEOUT {
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
    let map = repo
        .join("tests")
        .join("parity")
        .join(format!("{screen}.json"));
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

#[cfg(test)]
mod tests {
    use super::*;

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
