//! `cargo xtask drift`: finds screens that exist in gravecord but do not look like the official client.
//!
//! Two halves, so the analysis runs on any folder of frames:
//!
//! - `drift capture`: every line of `tests/drift/screens.txt` (`name | harness script`) is one headless launch of
//!   the prebuilt capture app, answered by a local replay of a recording (`replay.rs`), inert and without settings
//!   writes. A script names its frames with `shot @<screen>/<variant>`; a line without one keeps the final frame as
//!   `<name>`. Frames go to `<out>/frames/<screen>__<variant>.png` with the `.layout.json` and `.scene.json` the
//!   capture writes beside each (`app/src/parity.rs`).
//! - `drift analyze [dir...]`: every frame with a `.layout.json` (the sweep's folders, or `design/parity`'s older
//!   `<screen>-<variant>-latest.png` frames) is checked against what exists for it:
//!   - pixel mismatch against `design/golden/windows/<screen>/<variant>@1.png`, with a diff image;
//!   - the layout-tree compare of `layout.rs` against `design/dom/<screen>/<variant>.json`;
//!   - structural checks that need no golden: a probe clipped by an overflow-hidden ancestor, a probe past the
//!     window, siblings overlapping (verified against the official boxes when a DOM dump exists), a dialog that is
//!     not centred over its scrim or is larger than the window, and a probe narrower than its class's literal
//!     `min-width` (or off its literal `width`) in the official stylesheets.
//!
//!   It writes `report.md`, `findings.tsv`, `diff/` and `annotated/` (our frame with each finding's boxes: ours red,
//!   official green, the clip blue) to the output folder (default `target/drift`, which git ignores).
//!
//! What it cannot see: text cut by an ellipsis (gpui draws the truncated string; no box is clipped), elements without
//! a `parity(id)` probe unless they paint a quad, and screens no script reaches.

use crate::{layout, parity};
use anyhow::{Context, Result, bail};
use image::{Rgba, RgbaImage};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MANIFEST: &str = "tests/drift/screens.txt";
const PLUGINS_OFF: &str = "tests/plugins-off/scenarios.txt";
const RUN_LIMIT: Duration = Duration::from_secs(240);
/// A probe drawn this far past its clip is cut, not rounding or its own border.
const CLIP_SLACK: f64 = 2.5;
/// Without an official box, a cut this deep is a squeeze, not a design that hangs over its edge.
const UNVERIFIED_CUT: f64 = 8.0;
/// Overlaps smaller than this on either axis are shared borders.
const OVERLAP_SLACK: f64 = 1.5;
const CENTRE_SLACK: f64 = 2.0;

// ---------------------------------------------------------------------------------------------------------------------
// Command line

pub fn run(repo: &Path, args: &[&str]) -> Result<()> {
    let mut out = repo.join("target").join("drift");
    let mut manifest = repo.join(MANIFEST);
    let mut replay = None;
    let mut only: Vec<String> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut scenarios = true;
    let (mode, rest) = match args {
        ["capture", rest @ ..] => ("capture", rest),
        ["analyze", rest @ ..] => ("analyze", rest),
        rest => ("all", rest),
    };
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        let mut value = || rest.next().copied().context("flag without a value");
        match *arg {
            "--out" => out = PathBuf::from(value()?),
            "--manifest" => manifest = PathBuf::from(value()?),
            "--replay" => replay = Some(PathBuf::from(value()?)),
            "--only" => only.extend(value()?.split(',').map(str::to_owned)),
            "--no-scenarios" => scenarios = false,
            flag if flag.starts_with("--") => bail!(
                "usage: cargo xtask drift [capture|analyze] [--out DIR] [--manifest FILE] [--replay DIR] [--only a,b] [--no-scenarios] [dir...]"
            ),
            dir => dirs.push(PathBuf::from(dir)),
        }
    }
    fs::create_dir_all(&out)?;
    if mode != "analyze" {
        let replay = replay
            .or_else(default_replay)
            .context("no recording: pass --replay <folder> (a `GRAVECORD_RECORD` recording)")?;
        capture(repo, &manifest, &replay, &out, &only, scenarios)?;
    }
    if mode != "capture" {
        if dirs.is_empty() {
            dirs.push(out.join("frames"));
        }
        let report = analyze(repo, &dirs, &out)?;
        println!("{report}");
    }
    Ok(())
}

fn default_replay() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("GRAVECORD_DRIFT_REPLAY") {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("USERPROFILE")?;
    let dir = PathBuf::from(home)
        .join("gravecord-replay")
        .join("live-1118");
    dir.exists().then_some(dir)
}

// ---------------------------------------------------------------------------------------------------------------------
// Capture

struct Entry {
    name: String,
    script: String,
}

fn manifest_entries(text: &str) -> Vec<Entry> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (name, script) = line.split_once('|')?;
            Some(Entry {
                name: name.trim().to_owned(),
                script: script.trim().to_owned(),
            })
        })
        .collect()
}

/// `screen/variant` as a frame's file stem.
fn frame_stem(key: &str) -> String {
    key.replacen('/', "__", 1)
}

/// The script with every `shot @<screen>/<variant>` pointed into the frame folder, and the frames it names.
fn expand_shots(script: &str, frames: &Path) -> (String, Vec<String>) {
    let mut named = Vec::new();
    let steps: Vec<String> = script
        .split(';')
        .map(|step| {
            let step = step.trim();
            match step.strip_prefix("shot @") {
                Some(key) => {
                    let key = key.trim();
                    named.push(key.to_owned());
                    format!(
                        "shot {}",
                        frames.join(format!("{}.png", frame_stem(key))).display()
                    )
                }
                None => step.to_owned(),
            }
        })
        .collect();
    (steps.join("; "), named)
}

fn capture(
    repo: &Path,
    manifest: &Path,
    replay: &Path,
    out: &Path,
    only: &[String],
    scenarios: bool,
) -> Result<()> {
    let text =
        fs::read_to_string(manifest).with_context(|| format!("reading {}", manifest.display()))?;
    let mut entries = manifest_entries(&text);
    if scenarios {
        // The plugins-off scenarios reach places the manifest does not: their final frame, settled, as
        // `plugins-off/<name>`.
        let text = fs::read_to_string(repo.join(PLUGINS_OFF)).unwrap_or_default();
        entries.extend(manifest_entries(&text).into_iter().map(|entry| Entry {
            name: format!("plugins-off/{}", entry.name),
            script: format!("{}; settle 8000", entry.script),
        }));
    }
    let entries: Vec<Entry> = entries
        .into_iter()
        .filter(|entry| only.is_empty() || only.iter().any(|name| entry.name.contains(name)))
        .collect();
    let frames = out.join("frames");
    let runs = out.join("runs");
    fs::create_dir_all(&frames)?;
    fs::create_dir_all(&runs)?;
    let built = parity::target_dir(repo).join("debug").join("gravecord.exe");
    if !built.exists() {
        bail!(
            "{} is missing: build it first with `cargo build -p gravecord --features capture`",
            built.display()
        );
    }
    let exe = parity::run_copy(&built)?;
    // The child inherits this process's environment (`headless::spawn` passes none of its own).
    // SAFETY: xtask is single-threaded here; the replay servers' threads never read the environment.
    unsafe {
        for (key, value) in [
            ("GRAVECORD_INERT", "1"),
            ("GRAVECORD_NO_SETTINGS_WRITE", "1"),
        ] {
            std::env::set_var(key, value);
        }
    }
    let mut failures = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let (script, named) = expand_shots(&entry.script, &frames);
        let final_png = if named.is_empty() {
            frames.join(format!("{}.png", frame_stem(&entry.name)))
        } else {
            runs.join(format!("{}.png", entry.name.replace('/', "__")))
        };
        for key in &named {
            let _ = fs::remove_file(frames.join(format!("{}.png", frame_stem(key))));
        }
        let _ = fs::remove_file(&final_png);
        let server = crate::replay::start(
            replay,
            0,
            crate::replay::Options {
                ready_delay_ms: None,
                no_latency: false,
                log: Some(runs.join(format!("{}.replay.log", entry.name.replace('/', "__")))),
            },
        )?;
        // SAFETY: as above.
        unsafe {
            std::env::set_var("GRAVECORD_REPLAY", &server.url);
            std::env::set_var(
                "GRAVECORD_REPLAY_ACCOUNT",
                server.account.as_deref().unwrap_or("0"),
            );
        }
        let args = vec![
            "--capture".to_owned(),
            final_png.display().to_string(),
            "300".to_owned(),
            "--script".to_owned(),
            script,
        ];
        let started = Instant::now();
        print!("[{}/{}] {} ... ", index + 1, entries.len(), entry.name);
        let pid = crate::headless::spawn(&exe, &args, repo)?;
        let mut killed = false;
        while crate::plugins_off::running(pid) {
            if started.elapsed() > RUN_LIMIT {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .output();
                killed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let missing: Vec<&String> = named
            .iter()
            .filter(|key| !frames.join(format!("{}.png", frame_stem(key))).exists())
            .collect();
        if killed || !final_png.exists() || !missing.is_empty() {
            println!("failed after {:.0?}", started.elapsed());
            failures.push(format!(
                "{}: {}{}",
                entry.name,
                if killed { "timed out; " } else { "" },
                if missing.is_empty() {
                    "no final frame".to_owned()
                } else {
                    format!(
                        "missing {}",
                        missing
                            .iter()
                            .map(|key| key.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            ));
        } else {
            println!(
                "{} frame(s) in {:.0?}",
                named.len().max(1),
                started.elapsed()
            );
        }
    }
    let _ = fs::remove_file(&exe);
    fs::write(out.join("capture-failures.txt"), failures.join("\n") + "\n")?;
    if !failures.is_empty() {
        println!(
            "{} capture(s) failed, see capture-failures.txt",
            failures.len()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Geometry

type Rect = [f64; 4];

fn right(r: &Rect) -> f64 {
    r[0] + r[2]
}

fn bottom(r: &Rect) -> f64 {
    r[1] + r[3]
}

fn area(r: &Rect) -> f64 {
    r[2].max(0.) * r[3].max(0.)
}

fn intersection(a: &Rect, b: &Rect) -> Option<Rect> {
    let x = a[0].max(b[0]);
    let y = a[1].max(b[1]);
    let w = right(a).min(right(b)) - x;
    let h = bottom(a).min(bottom(b)) - y;
    (w > 0. && h > 0.).then_some([x, y, w, h])
}

fn contains(outer: &Rect, inner: &Rect, slack: f64) -> bool {
    inner[0] >= outer[0] - slack
        && inner[1] >= outer[1] - slack
        && right(inner) <= right(outer) + slack
        && bottom(inner) <= bottom(outer) + slack
}

fn fmt_rect(r: &Rect) -> String {
    format!("{:.0},{:.0} {:.0}x{:.0}", r[0], r[1], r[2], r[3])
}

// ---------------------------------------------------------------------------------------------------------------------
// Findings

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Severity {
    Low,
    Medium,
    High,
}

impl Severity {
    fn name(self) -> &'static str {
        match self {
            Severity::High => "high",
            Severity::Medium => "medium",
            Severity::Low => "low",
        }
    }
}

#[derive(Clone, Debug)]
struct Finding {
    severity: Severity,
    kind: &'static str,
    /// The parity id, when the finding is about one.
    id: Option<String>,
    what: String,
    /// Size of the fault in pixels, for ranking within a severity.
    magnitude: f64,
    ours: Option<Rect>,
    official: Option<Rect>,
    clip: Option<Rect>,
}

impl Finding {
    fn new(severity: Severity, kind: &'static str, what: String, magnitude: f64) -> Self {
        Self {
            severity,
            kind,
            id: None,
            what,
            magnitude,
            ours: None,
            official: None,
            clip: None,
        }
    }
}

struct Probe {
    id: String,
    rect: Rect,
    mask: Rect,
}

#[derive(Clone)]
struct Quad {
    rect: Rect,
    mask: Rect,
    alpha: f64,
    border: f64,
}

struct Scene {
    window: (f64, f64),
    probes: Vec<Probe>,
    quads: Vec<Quad>,
}

fn numbers(row: &[Value]) -> Vec<f64> {
    row.iter().filter_map(Value::as_f64).collect()
}

fn read_scene(path: &Path) -> Option<Scene> {
    let value: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let window = numbers(value["window"].as_array()?);
    let probes = value["probes"]
        .as_array()?
        .iter()
        .filter_map(|row| {
            let row = row.as_array()?;
            let id = row.first()?.as_str()?.to_owned();
            let n = numbers(&row[1..]);
            (n.len() == 8).then(|| Probe {
                id,
                rect: [n[0], n[1], n[2], n[3]],
                mask: [n[4], n[5], n[6], n[7]],
            })
        })
        .collect();
    let quads = value["quads"]
        .as_array()?
        .iter()
        .filter_map(|row| {
            let n = numbers(row.as_array()?);
            (n.len() >= 10).then(|| Quad {
                rect: [n[0], n[1], n[2], n[3]],
                mask: [n[4], n[5], n[6], n[7]],
                alpha: n[8],
                border: n[9],
            })
        })
        .collect();
    Some(Scene {
        window: (*window.first()?, *window.get(1)?),
        probes,
        quads,
    })
}

/// A scene from `layout.json` alone (older frames): every probe, unclipped, in a 1280x720 window.
fn scene_from_layout(path: &Path, window: (f64, f64)) -> Option<Scene> {
    let value: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let probes = value
        .as_object()?
        .iter()
        .filter_map(|(id, rect)| {
            let n = numbers(rect.as_array()?);
            (n.len() == 4).then(|| Probe {
                id: id.clone(),
                rect: [n[0], n[1], n[2], n[3]],
                mask: [-1e6, -1e6, 2e6, 2e6],
            })
        })
        .collect();
    Some(Scene {
        window,
        probes,
        quads: Vec::new(),
    })
}

/// The largest coordinate difference between two boxes.
fn delta(a: &Rect, b: &Rect) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).abs())
        .fold(0., f64::max)
}

/// Probes cut by an overflow-hidden ancestor. A probe hidden entirely is scrolled away, not cut. Horizontal cuts are
/// the ones that point at a squeezed layout; vertical cuts are mostly scrollers and rank low. A probe where the
/// official box is (to a pixel) is cut as the official one is; without an official box a cut under
/// [`UNVERIFIED_CUT`] is a design that hangs over its edge (the rail's pill) and ranks low.
fn clipped_probes(scene: &Scene, official: &HashMap<String, Option<Rect>>) -> Vec<Finding> {
    let (w, h) = scene.window;
    let window: Rect = [0., 0., w, h];
    let mut worst: BTreeMap<String, Finding> = BTreeMap::new();
    for probe in &scene.probes {
        let r = &probe.rect;
        let m = &probe.mask;
        if area(r) < 1. || intersection(r, m).is_none() {
            continue;
        }
        let theirs = official.get(&probe.id).copied().flatten();
        if theirs.is_some_and(|theirs| delta(r, &theirs) <= 1.) {
            continue;
        }
        let left = m[0] - r[0];
        let right_cut = right(r) - right(m);
        let top = m[1] - r[1];
        let bottom_cut = bottom(r) - bottom(m);
        // A mask that is the window is the window's edge: `overflow_window` reports that.
        let mask_is_window = contains(m, &window, 1.);
        let horizontal = left.max(right_cut);
        let vertical = top.max(bottom_cut);
        let finding = if horizontal > CLIP_SLACK && !mask_is_window {
            let side = if right_cut >= left { "right" } else { "left" };
            Some(Finding::new(
                if theirs.is_some() || horizontal >= UNVERIFIED_CUT {
                    Severity::High
                } else {
                    Severity::Low
                },
                "clipped",
                format!(
                    "`{}` is {:.0}px wide but its overflow-hidden ancestor shows only {:.0}px: {:.0}px cut on the {side}",
                    probe.id,
                    r[2],
                    intersection(r, m).map_or(0., |i| i[2]),
                    horizontal
                ),
                horizontal,
            ))
        } else if vertical > CLIP_SLACK && !mask_is_window {
            let side = if bottom_cut >= top { "bottom" } else { "top" };
            Some(Finding::new(
                Severity::Low,
                "clipped-vertical",
                format!(
                    "`{}` ({:.0}px tall) has {:.0}px cut at the {side} by an ancestor (often a scroller)",
                    probe.id, r[3], vertical
                ),
                vertical,
            ))
        } else {
            None
        };
        if let Some(mut finding) = finding {
            finding.id = Some(probe.id.clone());
            finding.ours = Some(*r);
            finding.clip = Some(*m);
            let keep = worst.get(&probe.id).is_none_or(|old| {
                (old.severity, old.magnitude) < (finding.severity, finding.magnitude)
            });
            if keep {
                worst.insert(probe.id.clone(), finding);
            }
        }
    }
    worst.into_values().collect()
}

/// How far a pixel must be from the column's background to count as drawn content (glyph ink).
const INK: u8 = 64;

/// Content cut at the right edge of a clip: text or an icon that runs into an overflow-hidden edge, as a line that
/// does not wrap does. Laid out content keeps its padding from such an edge, so ink in the last columns inside the
/// clip, in short runs with background above and below (a line of text, not a border or a picture), means something
/// continues past it. The clips are the masks of the painted quads; an edge with a thin quad on it (a scroll bar)
/// is skipped. Works on pixels: no box of the cut text exists.
fn edge_cuts(scene: &Scene, frame: &RgbaImage, golden: Option<&RgbaImage>) -> Vec<Finding> {
    let (w, h) = (frame.width() as i64, frame.height() as i64);
    let mut masks: Vec<Rect> = Vec::new();
    for quad in &scene.quads {
        let m = quad.mask;
        let rounded = [m[0].round(), m[1].round(), m[2].round(), m[3].round()];
        if rounded[2] >= 40.
            && rounded[3] >= 16.
            && right(&rounded) < w as f64 - 1.
            && !masks.contains(&rounded)
        {
            masks.push(rounded);
        }
    }
    let ink = |image: &RgbaImage, x: i64, y: i64, background: &Rgba<u8>| {
        let pixel = image.get_pixel(x as u32, y as u32);
        (0..3).any(|channel| pixel[channel].abs_diff(background[channel]) > INK)
    };
    let mut found = Vec::new();
    for mask in masks {
        let edge = right(&mask) as i64 - 1;
        let (top, bottom_row) = (mask[1].max(0.) as i64, (bottom(&mask) as i64).min(h));
        if edge < 8 || edge >= w || bottom_row - top < 16 {
            continue;
        }
        // A scroll bar or a border drawn on the edge.
        let on_edge = scene.quads.iter().any(|quad| {
            quad.rect[2] <= 12.
                && quad.rect[3] >= 24.
                && (right(&quad.rect) - right(&mask)).abs() <= 12.
                && intersection(&quad.rect, &mask).is_some()
        });
        if on_edge {
            continue;
        }
        // Ink in a column, against that column's background (its most common colour).
        let column = |image: &RgbaImage, x: i64| -> Vec<bool> {
            let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
            for y in top..bottom_row {
                let p = image.get_pixel(x as u32, y as u32);
                *counts.entry([p[0], p[1], p[2]]).or_default() += 1;
            }
            let background = counts
                .into_iter()
                .max_by_key(|(_, count)| *count)
                .map_or(Rgba([0, 0, 0, 255]), |(c, _)| Rgba([c[0], c[1], c[2], 255]));
            (top..bottom_row)
                .map(|y| ink(image, x, y, &background))
                .collect()
        };
        // The last three columns inside the clip, each with the column three further in: a border painted over the
        // clip's last column (the editor's panel at some sizes) hides the cut text in that one column only.
        let marks = |image: &RgbaImage| -> Vec<bool> {
            let columns: Vec<Vec<bool>> = (0..6).map(|k| column(image, edge - k)).collect();
            (0..(bottom_row - top) as usize)
                .map(|row| (0..3).any(|k| columns[k][row] && columns[k + 3][row]))
                .collect()
        };
        let marked = marks(frame);
        // The official frame at the same place: ink it has there too (a badge that touches the edge in both) is design.
        let official = golden
            .filter(|golden| golden.dimensions() == frame.dimensions())
            .map(marks); // Runs of marked rows, joined across gaps of up to two rows (the holes inside glyphs).
        let mut runs: Vec<(i64, i64)> = Vec::new();
        for (index, &mark) in marked.iter().enumerate() {
            if !mark {
                continue;
            }
            let y = top + index as i64;
            match runs.last_mut() {
                Some((_, end)) if y - *end <= 3 => *end = y,
                _ => runs.push((y, y)),
            }
        }
        for (start, end) in runs {
            let tall = end - start + 1;
            let marks = (start..=end).filter(|y| marked[(y - top) as usize]).count();
            // A glyph line is 4 to 24 rows; longer runs are pictures and borders, shorter ones antialiasing.
            if !(4..=24).contains(&tall) || marks < 3 {
                continue;
            }
            if official
                .as_ref()
                .is_some_and(|official| (start..=end).any(|y| official[(y - top) as usize]))
            {
                continue;
            }
            // Pixels of a layer above (a dialog over the rail) are not this clip's content: an opaque quad painted
            // after the clip's last one that covers the cut, from outside this clip, hides it.
            let point: Rect = [edge as f64 - 1., (start + end) as f64 / 2., 1., 1.];
            let last = scene
                .quads
                .iter()
                .rposition(|quad| delta(&quad.mask, &mask) <= 0.5)
                .unwrap_or(0);
            let hidden = scene.quads[last + 1..].iter().any(|quad| {
                quad.alpha >= 0.9
                    && !contains(&mask, &quad.mask, 0.5)
                    && intersection(&quad.rect, &quad.mask)
                        .is_some_and(|visible| contains(&visible, &point, 0.))
            });
            if hidden {
                continue;
            }
            let mut finding = Finding::new(
                Severity::Medium,
                "edge-cut",
                format!(
                    "content runs into the right edge of the clip {} at y {start}..{end}: text or an icon continues past x {}",
                    fmt_rect(&mask),
                    edge + 1
                ),
                tall as f64,
            );
            let cut: Rect = [edge as f64 - 4., start as f64, 4., tall as f64];
            // The probe drawing it, for the source file: the smallest one around the cut.
            finding.id = scene
                .probes
                .iter()
                .filter(|probe| contains(&probe.rect, &cut, 0.5))
                .min_by(|a, b| area(&a.rect).total_cmp(&area(&b.rect)))
                .map(|probe| probe.id.clone());
            if let Some(id) = &finding.id {
                let _ = write!(finding.what, " (inside `{id}`)");
            }
            finding.ours = Some([edge as f64 - 40., start as f64 - 2., 44., tall as f64 + 4.]);
            finding.clip = Some(mask);
            found.push(finding);
        }
    }
    found
}

/// Probes reaching past the window: a layout wider or taller than the window it is in.
fn overflow_window(scene: &Scene) -> Vec<Finding> {
    let (w, h) = scene.window;
    let mut found: BTreeMap<String, Finding> = BTreeMap::new();
    for probe in &scene.probes {
        let r = &probe.rect;
        if area(r) < 1. {
            continue;
        }
        // Scrolled-away content lies outside the window too, but under a scroller's mask.
        let mask_is_window = probe.mask[2] >= w - 1. && probe.mask[3] >= h - 1.;
        if !mask_is_window {
            continue;
        }
        let horizontal = (-r[0]).max(right(r) - w);
        let vertical = (-r[1]).max(bottom(r) - h);
        let (severity, amount, axis) = if horizontal > 1. {
            (Severity::High, horizontal, "horizontally")
        } else if vertical > 1. {
            (Severity::Medium, vertical, "vertically")
        } else {
            continue;
        };
        let mut finding = Finding::new(
            severity,
            "past-window",
            format!(
                "`{}` at {} runs {amount:.0}px past the {w:.0}x{h:.0} window {axis}",
                probe.id,
                fmt_rect(r)
            ),
            amount,
        );
        finding.id = Some(probe.id.clone());
        finding.ours = Some(*r);
        found.insert(probe.id.clone(), finding);
    }
    found.into_values().collect()
}

/// Each mapped id against its official box. A list draws an id once per row, so the instance nearest the official box
/// is the one compared (`layout.json` keeps only the last, which turned a list's tail into thousands of pixels).
fn layout_findings(scene: &Scene, official: &HashMap<String, Option<Rect>>) -> Vec<Finding> {
    let mut ids: Vec<&String> = official.keys().collect();
    ids.sort();
    let mut found = Vec::new();
    for id in ids {
        let instances: Vec<Rect> = scene
            .probes
            .iter()
            .filter(|probe| &probe.id == id && area(&probe.rect) >= 1.)
            .map(|probe| probe.rect)
            .collect();
        let Some(last) = instances.last().copied() else {
            let mut finding = Finding::new(
                Severity::Low,
                "layout-missing",
                format!("`{id}` is mapped for this screen but was not drawn"),
                0.,
            );
            finding.id = Some(id.clone());
            found.push(finding);
            continue;
        };
        let Some(theirs) = official[id] else {
            let mut finding = Finding::new(
                Severity::Low,
                "layout-unmatched",
                format!(
                    "`{id}`: its official class is not in this DOM dump (map entry or dump is off)"
                ),
                0.,
            );
            finding.id = Some(id.clone());
            finding.ours = Some(last);
            found.push(finding);
            continue;
        };
        let ours = instances
            .iter()
            .min_by(|a, b| delta(a, &theirs).total_cmp(&delta(b, &theirs)))
            .copied()
            .unwrap_or(last);
        let worst = delta(&ours, &theirs);
        if worst <= 0.51 {
            continue;
        }
        let mut parts = Vec::new();
        for (index, name) in ["x", "y", "width", "height"].iter().enumerate() {
            if (ours[index] - theirs[index]).abs() > 0.51 {
                parts.push(format!(
                    "{name} {:.0} (official {:.0})",
                    ours[index], theirs[index]
                ));
            }
        }
        let size_off = (ours[2] - theirs[2]).abs().max((ours[3] - theirs[3]).abs());
        let only_y = (ours[0] - theirs[0]).abs() <= 0.51
            && (ours[2] - theirs[2]).abs() <= 0.51
            && (ours[3] - theirs[3]).abs() <= 0.51;
        let window: Rect = [0., 0., scene.window.0, scene.window.1];
        let (severity, note) = if intersection(&theirs, &window).is_none() {
            // The dump's first element of that class is off-screen: the map's class picks another element here.
            (
                Severity::Low,
                " (the official match is off-screen: the map's class picks another element)",
            )
        } else if worst > scene.window.0.max(scene.window.1) {
            // Thousands of pixels: the probe measures a scroller's content where the dump has its viewport, or the
            // frame is scrolled elsewhere. Not a drawn difference.
            (
                Severity::Low,
                " (larger than the window: scroll state or probe placement)",
            )
        } else if only_y && (ours[1] - theirs[1]).abs() > 40. {
            (
                Severity::Low,
                " (only y differs: likely scroll position or data above)",
            )
        } else if size_off > 4. || worst > 16. {
            (Severity::High, "")
        } else if worst > 2. {
            (Severity::Medium, "")
        } else {
            (Severity::Low, "")
        };
        let mut finding = Finding::new(
            severity,
            "layout",
            format!("`{id}`: {}{note}", parts.join(", ")),
            worst,
        );
        finding.id = Some(id.clone());
        finding.ours = Some(ours);
        finding.official = Some(theirs);
        found.push(finding);
    }
    found
}

/// The last instance of each probe id: what `layout.json` holds.
fn last_probes(scene: &Scene) -> Vec<(&str, Rect)> {
    let mut by_id: BTreeMap<&str, Rect> = BTreeMap::new();
    for probe in &scene.probes {
        if area(&probe.rect) >= 1. {
            by_id.insert(&probe.id, probe.rect);
        }
    }
    by_id.into_iter().collect()
}

/// Each probe's parent: the smallest other probe that contains it.
fn parents(probes: &[(&str, Rect)]) -> Vec<Option<usize>> {
    probes
        .iter()
        .enumerate()
        .map(|(index, (_, rect))| {
            probes
                .iter()
                .enumerate()
                .filter(|(other, (_, outer))| {
                    *other != index
                        && contains(outer, rect, 0.5)
                        // Two equal boxes: the earlier id is the parent, so they do not parent each other.
                        && (!contains(rect, outer, 0.5) || *other < index)
                })
                .min_by(|a, b| area(&a.1.1).total_cmp(&area(&b.1.1)))
                .map(|(other, _)| other)
        })
        .collect()
}

/// Siblings (same parent probe) that overlap. With official boxes, an overlap the official client has too is kept
/// out, and one where official boxes are apart is high; without them it is medium and marked unverified.
fn overlapping_siblings(scene: &Scene, official: &HashMap<String, Option<Rect>>) -> Vec<Finding> {
    let probes = last_probes(scene);
    let parent = parents(&probes);
    let mut found = Vec::new();
    for a in 0..probes.len() {
        for b in a + 1..probes.len() {
            if parent[a] != parent[b] {
                continue;
            }
            let ((id_a, ra), (id_b, rb)) = (&probes[a], &probes[b]);
            if contains(ra, rb, 0.5) || contains(rb, ra, 0.5) {
                continue;
            }
            let Some(overlap) = intersection(ra, rb) else {
                continue;
            };
            if overlap[2] <= OVERLAP_SLACK || overlap[3] <= OVERLAP_SLACK {
                continue;
            }
            let theirs = (
                official.get(*id_a).copied().flatten(),
                official.get(*id_b).copied().flatten(),
            );
            let (severity, note) = match theirs {
                (Some(oa), Some(ob)) => match intersection(&oa, &ob) {
                    Some(o) if o[2] > OVERLAP_SLACK && o[3] > OVERLAP_SLACK => continue,
                    _ => (Severity::High, "official boxes do not overlap"),
                },
                // The shell overlaps by design (the user panel over the rail, the composer over the messages):
                // without the official boxes an overlap is only a lead.
                _ => (Severity::Low, "unverified: no official boxes for both"),
            };
            let mut finding = Finding::new(
                severity,
                "overlap",
                format!(
                    "`{id_a}` ({}) and `{id_b}` ({}) overlap by {:.0}x{:.0}px; {note}",
                    fmt_rect(ra),
                    fmt_rect(rb),
                    overlap[2],
                    overlap[3]
                ),
                overlap[2].min(overlap[3]),
            );
            finding.id = Some(format!("{id_a} / {id_b}"));
            finding.ours = Some(overlap);
            found.push(finding);
        }
    }
    found
}

/// The dialog over a scrim: the last translucent quad covering the window, then the largest opaque quad painted after
/// it. Reported when it is not centred over the scrim or does not fit in the window.
fn modal_checks(scene: &Scene) -> Vec<Finding> {
    let (w, h) = scene.window;
    let window_area = w * h;
    let Some((scrim_index, scrim)) = scene.quads.iter().enumerate().rev().find(|(_, quad)| {
        quad.rect[2] >= w - 2.
            && quad.rect[3] >= h - 48.
            && quad.alpha > 0.05
            && quad.alpha < 0.95
            && quad.border == 0.
    }) else {
        return Vec::new();
    };
    let visible = |quad: &Quad| intersection(&quad.rect, &quad.mask).map_or(0., |i| area(&i));
    let panels: Vec<&Quad> = scene.quads[scrim_index + 1..]
        .iter()
        .filter(|quad| {
            quad.alpha >= 0.9
                && area(&quad.rect) >= 0.04 * window_area
                && area(&quad.rect) < 0.97 * window_area
        })
        .collect();
    let Some(largest) = panels
        .iter()
        .max_by(|a, b| visible(a).total_cmp(&visible(b)))
    else {
        return Vec::new();
    };
    // A dialog can be several panels side by side (the profile editor's panel and stage): the largest, grown by the
    // panels touching it.
    // Each panel as drawn: the part its clip shows (a scroller's content runs past the dialog).
    let shown = |quad: &Quad| intersection(&quad.rect, &quad.mask);
    let Some(mut dialog) = shown(largest) else {
        return Vec::new();
    };
    loop {
        let grown = panels
            .iter()
            .filter_map(|quad| shown(quad))
            .fold(dialog, |union, rect| {
                let near = [union[0] - 2., union[1] - 2., union[2] + 4., union[3] + 4.];
                if intersection(&near, &rect).is_some() {
                    let x = union[0].min(rect[0]);
                    let y = union[1].min(rect[1]);
                    [
                        x,
                        y,
                        right(&union).max(right(&rect)) - x,
                        bottom(&union).max(bottom(&rect)) - y,
                    ]
                } else {
                    union
                }
            });
        if grown == dialog {
            break;
        }
        dialog = grown;
    }
    let r = &dialog;
    let s = &scrim.rect;
    let mut found = Vec::new();
    let dx = (r[0] + r[2] / 2.) - (s[0] + s[2] / 2.);
    let dy = (r[1] + r[3] / 2.) - (s[1] + s[3] / 2.);
    let window: Rect = [0., 0., w, h];
    // Larger than the window: a panel of the dialog clipped only by the window (not by a scroller inside it) whose
    // box runs past it.
    let oversized = panels
        .iter()
        .filter(|quad| contains(&quad.mask, &window, 1.) && intersection(&quad.rect, r).is_some())
        .map(|quad| quad.rect)
        .find(|rect| !contains(&window, rect, 1.));
    if let Some(big) = oversized {
        let mut finding = Finding::new(
            Severity::High,
            "modal-size",
            format!(
                "the dialog over the scrim is {:.0}x{:.0} at {:.0},{:.0}, larger than the {w:.0}x{h:.0} window",
                big[2], big[3], big[0], big[1]
            ),
            (right(&big) - w)
                .max(bottom(&big) - h)
                .max(-big[0])
                .max(-big[1])
                .max(1.),
        );
        finding.ours = Some(big);
        found.push(finding);
    }
    if dx.abs() > CENTRE_SLACK || dy.abs() > CENTRE_SLACK {
        let mut finding = Finding::new(
            if dx.abs() > CENTRE_SLACK {
                Severity::High
            } else {
                Severity::Medium
            },
            "modal-centre",
            format!(
                "the dialog ({}) is off the scrim's centre by {dx:+.0}px across and {dy:+.0}px down",
                fmt_rect(r)
            ),
            dx.abs().max(dy.abs()),
        );
        finding.ours = Some(*r);
        finding.clip = Some(*s);
        found.push(finding);
    }
    found
}

// ---------------------------------------------------------------------------------------------------------------------
// Official stylesheets

/// One style rule's declarations for a class, with its `@media` condition when it has one.
#[derive(Clone, Debug)]
struct ClassRule {
    media: Option<String>,
    declarations: Vec<(String, String)>,
}

#[derive(Default)]
struct Styles {
    rules: HashMap<String, Vec<ClassRule>>,
}

/// The single class a selector is exactly (`.panel_BfN7RR`), else none.
fn exact_class(selector: &str) -> Option<&str> {
    let class = selector.trim().strip_prefix('.')?;
    class
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        .then_some(class)
        .filter(|class| !class.is_empty())
}

fn parse_css(text: &str, styles: &mut Styles) {
    let bytes = text.as_bytes();
    let mut stack: Vec<Option<String>> = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = text[index + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| index + 2 + end + 2);
                start = index;
                continue;
            }
            b'{' => {
                let prelude = text[start..index].trim();
                if prelude.starts_with('@') {
                    stack.push(Some(prelude.to_owned()));
                    index += 1;
                    start = index;
                    continue;
                }
                // A style rule: its block up to the matching brace (nested rules inside are skipped).
                let mut depth = 1;
                let mut end = index + 1;
                while end < bytes.len() && depth > 0 {
                    match bytes[end] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    end += 1;
                }
                let block = &text[index + 1..end.saturating_sub(1)];
                let media: Vec<&str> = stack.iter().flatten().map(String::as_str).collect();
                let condition = (!media.is_empty()).then(|| media.join(" && "));
                let declarations: Vec<(String, String)> = block
                    .split(';')
                    .filter(|part| !part.contains('{'))
                    .filter_map(|part| {
                        let (name, value) = part.split_once(':')?;
                        Some((
                            name.trim().to_ascii_lowercase(),
                            value
                                .trim()
                                .trim_end_matches("!important")
                                .trim()
                                .to_owned(),
                        ))
                    })
                    .collect();
                for selector in prelude.split(',') {
                    if let Some(class) = exact_class(selector) {
                        styles
                            .rules
                            .entry(class.to_owned())
                            .or_default()
                            .push(ClassRule {
                                media: condition.clone(),
                                declarations: declarations.clone(),
                            });
                    }
                }
                index = end;
                start = index;
                continue;
            }
            b'}' => {
                stack.pop();
                index += 1;
                start = index;
                continue;
            }
            b';' if stack.last().is_none_or(Option::is_some) => {
                // An at-statement (`@import ...;`) or stray text between rules.
                index += 1;
                start = index;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
}

fn load_styles(repo: &Path) -> Styles {
    let mut styles = Styles::default();
    let folder = repo.join("design").join("snapshot").join("assets");
    if let Ok(entries) = fs::read_dir(folder) {
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "css"))
            .collect();
        paths.sort();
        for path in paths {
            if let Ok(text) = fs::read_to_string(&path) {
                parse_css(&text, &mut styles);
            }
        }
    }
    styles
}

fn length_px(value: &str) -> Option<f64> {
    let value = value.trim();
    if let Some(number) = value.strip_suffix("px") {
        return number.trim().parse().ok();
    }
    if let Some(number) = value.strip_suffix("rem") {
        return number.trim().parse::<f64>().ok().map(|rem| rem * 16.);
    }
    (value == "0").then_some(0.)
}

/// Whether a `@media` prelude holds for a window, or none when it cannot be told (anything but widths and heights).
fn media_matches(prelude: &str, window: (f64, f64)) -> Option<bool> {
    let query = prelude.strip_prefix("@media")?.trim();
    let mut any = false;
    for alternative in query.split(',') {
        let mut all = true;
        for part in alternative.split(" and ") {
            let part = part.trim();
            if part.is_empty() || part == "screen" || part == "all" {
                continue;
            }
            let inner = part.strip_prefix('(')?.strip_suffix(')')?;
            let (feature, value) = inner.split_once(':')?;
            let value = length_px(value)?;
            let holds = match feature.trim() {
                "min-width" => window.0 >= value,
                "max-width" => window.0 <= value,
                "min-height" => window.1 >= value,
                "max-height" => window.1 <= value,
                _ => return None,
            };
            all &= holds;
        }
        any |= all;
    }
    Some(any)
}

/// The declarations of a class that apply in this window, in stylesheet order; none when a condition cannot be told.
fn applicable(rules: &[ClassRule], window: (f64, f64)) -> Option<Vec<&(String, String)>> {
    let mut out = Vec::new();
    for rule in rules {
        let holds = match &rule.media {
            None => true,
            Some(conditions) => {
                let mut holds = true;
                for condition in conditions.split(" && ") {
                    if condition.starts_with("@media") {
                        holds &= media_matches(condition, window)?;
                    } else if condition.starts_with("@layer") {
                    } else {
                        return None;
                    }
                }
                holds
            }
        };
        if holds {
            out.extend(rule.declarations.iter());
        }
    }
    Some(out)
}

/// Horizontal padding plus border of a content-box element, from literal declarations; none if one is not literal.
fn horizontal_extra(declarations: &[&(String, String)]) -> Option<f64> {
    let mut padding = [0.; 2];
    let mut border = [0.; 2];
    let mut border_box = false;
    for (name, value) in declarations {
        let sides = |value: &str| -> Option<[f64; 2]> {
            let parts: Vec<&str> = value.split_whitespace().collect();
            let get = |index: usize| parts.get(index).and_then(|part| length_px(part));
            match parts.len() {
                1 => Some([get(0)?; 2]),
                2 | 3 => Some([get(1)?; 2]),
                4 => Some([get(3)?, get(1)?]),
                _ => None,
            }
        };
        match name.as_str() {
            "box-sizing" => border_box = value == "border-box",
            "padding" => padding = sides(value)?,
            "padding-inline" => {
                let parts: Vec<f64> = value
                    .split_whitespace()
                    .map(length_px)
                    .collect::<Option<_>>()?;
                padding = [*parts.first()?, *parts.get(1).or(parts.first())?];
            }
            "padding-left" | "padding-inline-start" => padding[0] = length_px(value)?,
            "padding-right" | "padding-inline-end" => padding[1] = length_px(value)?,
            "border" | "border-left" | "border-right" => {
                if value == "none" || value == "0" {
                    continue;
                }
                let width = value
                    .split_whitespace()
                    .find_map(length_px)
                    .unwrap_or(if value.contains("solid") { 3. } else { 0. });
                match name.as_str() {
                    "border" => border = [width; 2],
                    "border-left" => border[0] = width,
                    _ => border[1] = width,
                }
            }
            "border-width" => border = sides(value)?,
            _ => {}
        }
    }
    Some(if border_box {
        0.
    } else {
        padding[0] + padding[1] + border[0] + border[1]
    })
}

/// Our width against the literal `min-width`, `width` and `max-width` the official stylesheets give the probe's
/// class. Values with `var()` or `calc()` are not evaluated and are counted in `skipped`.
fn css_checks(
    scene: &Scene,
    map: &Value,
    styles: &Styles,
    wanted: impl Fn(&str) -> bool,
    skipped: &mut usize,
) -> Vec<Finding> {
    let mut found = Vec::new();
    let Some(entries) = map.as_object() else {
        return found;
    };
    let probes: HashMap<&str, Rect> = last_probes(scene).into_iter().collect();
    for (id, target) in entries {
        if !wanted(id) {
            continue;
        }
        let (Some(class), Some(ours)) = (target["class"].as_str(), probes.get(id.as_str())) else {
            continue;
        };
        let Some(rules) = styles.rules.get(class) else {
            continue;
        };
        let Some(declarations) = applicable(rules, scene.window) else {
            *skipped += 1;
            continue;
        };
        let last = |property: &str| {
            declarations
                .iter()
                .rev()
                .find(|(name, _)| name == property)
                .map(|(_, value)| value.as_str())
        };
        let Some(extra) = horizontal_extra(&declarations) else {
            *skipped += 1;
            continue;
        };
        for property in ["min-width", "width", "max-width"] {
            let Some(value) = last(property) else {
                continue;
            };
            let Some(px) = length_px(value) else {
                if value.contains("var(") || value.contains("calc(") || value.contains("min(") {
                    *skipped += 1;
                }
                continue;
            };
            let expected = px + extra;
            let width = ours[2];
            let (fails, severity) = match property {
                "min-width" => (width + 0.5 < expected, Severity::High),
                "width" => ((width - expected).abs() > 1., Severity::Medium),
                _ => (width > expected + 0.5, Severity::Medium),
            };
            if fails {
                let mut finding = Finding::new(
                    severity,
                    "css-width",
                    format!(
                        "`{id}` is {width:.0}px wide; official `.{class}` has `{property}: {value}`{} = {expected:.0}px",
                        if extra > 0. {
                            format!(" plus {extra:.0}px padding/border")
                        } else {
                            String::new()
                        }
                    ),
                    (width - expected).abs(),
                );
                finding.id = Some(id.clone());
                finding.ours = Some(*ours);
                found.push(finding);
            }
        }
    }
    found
}

// ---------------------------------------------------------------------------------------------------------------------
// Pixels

fn pixel_diff(ours: &RgbaImage, golden: &RgbaImage) -> (f64, RgbaImage) {
    let mut different = 0u64;
    let image = RgbaImage::from_fn(ours.width(), ours.height(), |x, y| {
        let (a, b) = (ours.get_pixel(x, y), golden.get_pixel(x, y));
        if parity::differs(a, b) {
            different += 1;
            Rgba([255, 40, 40, 255])
        } else {
            Rgba([b[0] / 3, b[1] / 3, b[2] / 3, 255])
        }
    });
    let total = u64::from(ours.width()) * u64::from(ours.height());
    (100. * different as f64 / total.max(1) as f64, image)
}

fn outline(image: &mut RgbaImage, rect: &Rect, colour: Rgba<u8>) {
    let (w, h) = (image.width() as i64, image.height() as i64);
    let x0 = rect[0].round() as i64;
    let y0 = rect[1].round() as i64;
    let x1 = right(rect).round() as i64 - 1;
    let y1 = bottom(rect).round() as i64 - 1;
    let mut put = |x: i64, y: i64| {
        if (0..w).contains(&x) && (0..h).contains(&y) {
            image.put_pixel(x as u32, y as u32, colour);
        }
    };
    for thickness in 0..2 {
        for x in x0..=x1 {
            put(x, y0 + thickness);
            put(x, y1 - thickness);
        }
        for y in y0..=y1 {
            put(x0 + thickness, y);
            put(x1 - thickness, y);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Analysis

struct Analysis<'a> {
    repo: &'a Path,
    golden_screens: BTreeSet<String>,
    styles: Styles,
    sources: HashMap<String, BTreeSet<String>>,
}

/// `screen`/`variant` of a frame: `<screen>__<variant>` from the sweep, `<screen>-<variant>-latest` from
/// `design/parity`, where the screen is the longest known screen name the stem starts with.
fn split_key(stem: &str, screens: &BTreeSet<String>) -> (String, String) {
    if let Some((screen, variant)) = stem.split_once("__") {
        return (screen.to_owned(), variant.to_owned());
    }
    let stem = stem.strip_suffix("-latest").unwrap_or(stem);
    screens
        .iter()
        .filter(|screen| {
            stem.strip_prefix(screen.as_str())
                .is_some_and(|rest| rest.starts_with('-') && rest.len() > 1)
        })
        .max_by_key(|screen| screen.len())
        .map_or_else(
            || (stem.to_owned(), String::new()),
            |screen| (screen.clone(), stem[screen.len() + 1..].to_owned()),
        )
}

/// Every `parity("id")` (and its border variants) in app/src, with the files that draw it.
fn parity_sources(repo: &Path) -> HashMap<String, BTreeSet<String>> {
    let mut found: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut stack = vec![repo.join("app").join("src")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let relative = path
                .strip_prefix(repo)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            for (at, _) in text.match_indices(".parity") {
                let rest = &text[at..];
                let Some(open) = rest.find("(\"") else {
                    continue;
                };
                if open > 20 {
                    continue;
                }
                let rest = &rest[open + 2..];
                if let Some(end) = rest.find('"') {
                    found
                        .entry(rest[..end].to_owned())
                        .or_default()
                        .insert(relative.clone());
                }
            }
        }
    }
    found
}

/// Ids shared by most screens say nothing about where a screen's fault is.
const GENERIC_IDS: [&str; 6] = [
    "titlebar",
    "rail",
    "sidebar",
    "user-panel",
    "header",
    "composer",
];

fn likely_sources(context: &Analysis, id: Option<&str>, probes: &[(&str, Rect)]) -> String {
    let mut files: BTreeSet<String> = BTreeSet::new();
    if let Some(id) = id {
        for part in id.split(" / ") {
            if let Some(found) = context.sources.get(part) {
                files.extend(found.iter().cloned());
            }
        }
    }
    if files.is_empty() {
        // The files drawing this frame's non-generic probes, the most-cited first.
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for (probe, _) in probes {
            if GENERIC_IDS.contains(probe) {
                continue;
            }
            for file in context.sources.get(*probe).into_iter().flatten() {
                *counts.entry(file).or_default() += 1;
            }
        }
        let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
        ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        files.extend(ranked.into_iter().take(2).map(|(file, _)| file.to_owned()));
    }
    files.into_iter().take(3).collect::<Vec<_>>().join(", ")
}

struct FrameReport {
    key: String,
    frame: PathBuf,
    modified: String,
    pixel: Option<f64>,
    pixel_note: Option<String>,
    layout_checked: usize,
    layout_note: Option<String>,
    has_scene: bool,
    findings: Vec<Finding>,
    sources: Vec<String>,
    diff: Option<PathBuf>,
    annotated: Option<PathBuf>,
}

impl FrameReport {
    fn score(&self) -> f64 {
        let weight = |finding: &Finding| match finding.severity {
            Severity::High => 100. + finding.magnitude.min(400.),
            Severity::Medium => 20. + finding.magnitude.min(100.) / 4.,
            Severity::Low => 1.,
        };
        self.findings.iter().map(weight).sum::<f64>() + self.pixel.unwrap_or(0.)
    }
}

fn mtime(path: &Path) -> std::time::SystemTime {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or(std::time::UNIX_EPOCH)
}

fn modified(path: &Path) -> String {
    let Ok(time) = fs::metadata(path).and_then(|meta| meta.modified()) else {
        return String::new();
    };
    let seconds = time
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // Days since the epoch to a civil date (Howard Hinnant's algorithm), UTC.
    let days = (seconds / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year}-{month:02}-{day:02}")
}

fn analyze_frame(context: &Analysis, frame: &Path, out: &Path) -> Result<FrameReport> {
    let stem = frame
        .file_stem()
        .context("frame without a name")?
        .to_string_lossy()
        .into_owned();
    let (screen, variant) = split_key(&stem, &context.golden_screens);
    let key = format!("{screen}/{variant}");
    let design = context.repo.join("design");
    let layout_path = frame.with_extension("layout.json");
    let scene_path = frame.with_extension("scene.json");
    let ours = image::open(frame)
        .with_context(|| format!("reading {}", frame.display()))?
        .to_rgba8();
    let window = (f64::from(ours.width()), f64::from(ours.height()));
    let has_scene = scene_path.exists();
    let scene = read_scene(&scene_path)
        .or_else(|| scene_from_layout(&layout_path, window))
        .context("no layout")?;
    let mut report = FrameReport {
        key: key.clone(),
        frame: frame.to_owned(),
        modified: modified(frame),
        pixel: None,
        pixel_note: None,
        layout_checked: 0,
        layout_note: None,
        has_scene,
        findings: Vec::new(),
        sources: Vec::new(),
        diff: None,
        annotated: None,
    };
    let safe = frame_stem(&key);

    // Pixels.
    let golden_path = design
        .join("golden")
        .join("windows")
        .join(&screen)
        .join(format!("{variant}@1.png"));
    let golden = image::open(&golden_path)
        .ok()
        .map(|golden| golden.to_rgba8());
    match &golden {
        None => report.pixel_note = Some("no golden".into()),
        Some(golden) => {
            if golden.dimensions() == ours.dimensions() {
                let (percent, image) = pixel_diff(&ours, golden);
                let path = out.join("diff").join(format!("{safe}.png"));
                fs::create_dir_all(path.parent().context("diff folder")?)?;
                image.save(&path)?;
                report.pixel = Some(percent);
                report.diff = Some(path);
            } else {
                report.pixel_note = Some(format!(
                    "frame {}x{}, golden {}x{}",
                    ours.width(),
                    ours.height(),
                    golden.width(),
                    golden.height()
                ));
            }
        }
    }

    // Layout tree against the official DOM.
    let dom = design
        .join("dom")
        .join(&screen)
        .join(format!("{variant}.json"));
    let maps = context.repo.join("tests").join("parity");
    let map_path = parity::map_for(&maps, &screen, &variant);
    let map: Value = fs::read(&map_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null);
    let mut official: HashMap<String, Option<Rect>> = HashMap::new();
    if !map_path.exists() {
        report.layout_note = Some("no parity map".into());
    } else if !dom.exists() {
        report.layout_note = Some("no DOM dump".into());
    } else {
        match layout::official_boxes(&dom, &map_path) {
            Ok(boxes) => official = boxes.into_iter().collect(),
            Err(error) => report.layout_note = Some(format!("compare failed: {error:#}")),
        }
        report.layout_checked = official.len();
        report.findings.extend(layout_findings(&scene, &official));
    }
    // Structural checks.
    report.findings.extend(clipped_probes(&scene, &official));
    if has_scene {
        report.findings.extend(overflow_window(&scene));
        report
            .findings
            .extend(edge_cuts(&scene, &ours, golden.as_ref()));
    }
    report
        .findings
        .extend(overlapping_siblings(&scene, &official));
    report.findings.extend(modal_checks(&scene));
    let mut skipped = 0;
    // The stylesheets are a fallback for ids the DOM dump does not place: a measured box beats a rule that a more
    // specific selector may override.
    let unplaced: BTreeSet<&str> = official
        .iter()
        .filter(|(_, rect)| rect.is_none())
        .map(|(id, _)| id.as_str())
        .collect();
    report.findings.extend(css_checks(
        &scene,
        &map,
        &context.styles,
        |id| official.is_empty() || unplaced.contains(id),
        &mut skipped,
    ));

    report.findings.sort_by(|a, b| {
        (b.severity, b.magnitude)
            .partial_cmp(&(a.severity, a.magnitude))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let probes = last_probes(&scene);
    report.sources = report
        .findings
        .iter()
        .map(|finding| likely_sources(context, finding.id.as_deref(), &probes))
        .collect();

    // Our frame with the boxes of its findings.
    if report
        .findings
        .iter()
        .any(|finding| finding.severity >= Severity::Medium && finding.ours.is_some())
    {
        let mut image = ours.clone();
        for finding in report
            .findings
            .iter()
            .filter(|f| f.severity >= Severity::Medium)
        {
            if let Some(clip) = &finding.clip {
                outline(&mut image, clip, Rgba([60, 140, 255, 255]));
            }
            if let Some(official) = &finding.official {
                outline(&mut image, official, Rgba([40, 220, 90, 255]));
            }
            if let Some(ours) = &finding.ours {
                outline(&mut image, ours, Rgba([255, 50, 50, 255]));
            }
        }
        let path = out.join("annotated").join(format!("{safe}.png"));
        fs::create_dir_all(path.parent().context("annotated folder")?)?;
        image.save(&path)?;
        report.annotated = Some(path);
    }
    Ok(report)
}

fn frames_in(dir: &Path) -> Vec<PathBuf> {
    let mut frames: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "png")
                && !path.to_string_lossy().ends_with("-diff.png")
                && path.with_extension("layout.json").exists()
        })
        .collect();
    frames.sort();
    frames
}

fn analyze(repo: &Path, dirs: &[PathBuf], out: &Path) -> Result<String> {
    let design = repo.join("design");
    let mut golden_screens: BTreeSet<String> = BTreeSet::new();
    for folder in [design.join("golden").join("windows"), design.join("dom")] {
        for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                golden_screens.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    let context = Analysis {
        repo,
        golden_screens,
        styles: load_styles(repo),
        sources: parity_sources(repo),
    };
    let mut reports = Vec::new();
    let mut unreadable = Vec::new();
    for dir in dirs {
        for frame in frames_in(dir) {
            match analyze_frame(&context, &frame, out) {
                Ok(report) => reports.push(report),
                Err(error) => unreadable.push(format!("{}: {error:#}", frame.display())),
            }
        }
    }
    // The same screen from two folders: a sweep frame (with its scene) beats an older one, else the later file wins.
    let rank = |report: &FrameReport| (report.has_scene, mtime(&report.frame));
    let mut by_key: BTreeMap<String, FrameReport> = BTreeMap::new();
    for report in reports {
        let newer = by_key
            .get(&report.key)
            .is_none_or(|old| rank(old) <= rank(&report));
        if newer {
            by_key.insert(report.key.clone(), report);
        }
    }
    let mut reports: Vec<FrameReport> = by_key.into_values().collect();
    reports.sort_by(|a, b| b.score().total_cmp(&a.score()));
    write_report(repo, &context, &reports, &unreadable, out)
}

fn rel(repo: &Path, path: &Path) -> String {
    path.strip_prefix(repo)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn write_report(
    repo: &Path,
    context: &Analysis,
    reports: &[FrameReport],
    unreadable: &[String],
    out: &Path,
) -> Result<String> {
    let design = repo.join("design");
    // Paths inside the output folder are written relative to it, so the folder can be moved whole; others relative to
    // the repository.
    let shown = |path: &Path| match path.strip_prefix(out) {
        Ok(inside) => inside.to_string_lossy().replace('\\', "/"),
        Err(_) => rel(repo, path),
    };
    let mut md = String::new();
    let mut tsv = String::from(
        "rank\tseverity\tkind\tscreen\tid\twhat\tframe\tevidence\tsource\tframe_date\n",
    );
    let count = |severity: Severity| {
        reports
            .iter()
            .flat_map(|report| &report.findings)
            .filter(|finding| finding.severity == severity)
            .count()
    };
    let _ = writeln!(md, "# Visual drift report\n");
    let _ = writeln!(
        md,
        "{} frames analysed: {} high, {} medium, {} low findings. Ranked by severity, then size in pixels. \
         Evidence: `annotated/` (ours red, official green, clip blue), `diff/` (pixels that differ from the golden in red).\n",
        reports.len(),
        count(Severity::High),
        count(Severity::Medium),
        count(Severity::Low)
    );
    let _ = writeln!(
        md,
        "Not detectable here: text shortened with an ellipsis (no box is cut), elements without a `parity(id)` probe \
         (only their painted quads are seen, in the dialog check), and screens without a capture script.\n"
    );

    // Every medium or high finding, across frames.
    let mut ranked: Vec<(&FrameReport, &Finding, &str)> = reports
        .iter()
        .flat_map(|report| {
            report
                .findings
                .iter()
                .zip(&report.sources)
                .map(move |(finding, source)| (report, finding, source.as_str()))
        })
        .collect();
    ranked.sort_by(|a, b| {
        (b.1.severity, b.1.magnitude)
            .partial_cmp(&(a.1.severity, a.1.magnitude))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Frames this sweep captured carry clip masks and quads (`scene.json`) and show today's code; older frames (from
    // `design/parity`) may show faults fixed since, so they get a table of their own.
    let mut tables = [String::new(), String::new()];
    for table in &mut tables {
        let _ = writeln!(
            table,
            "| # | sev | kind | screen | what | evidence | likely source |"
        );
        let _ = writeln!(table, "|---|---|---|---|---|---|---|");
    }
    for (rank, (report, finding, source)) in ranked.iter().enumerate() {
        let evidence = report
            .annotated
            .as_ref()
            .filter(|_| finding.ours.is_some())
            .or(report.diff.as_ref())
            .map_or_else(|| shown(&report.frame), |path| shown(path));
        let _ = writeln!(
            tsv,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            rank + 1,
            finding.severity.name(),
            finding.kind,
            report.key,
            finding.id.as_deref().unwrap_or(""),
            finding.what.replace('\t', " "),
            shown(&report.frame),
            evidence,
            source,
            report.modified
        );
        if finding.severity >= Severity::Medium {
            let table = &mut tables[usize::from(!report.has_scene)];
            let _ = writeln!(
                table,
                "| {} | {} | {} | `{}` | {} | `{}` | {} |",
                rank + 1,
                finding.severity.name(),
                finding.kind,
                if report.has_scene {
                    report.key.clone()
                } else {
                    format!("{} ({})", report.key, report.modified)
                },
                finding.what.replace('|', "\\|"),
                evidence,
                source
            );
        }
    }
    let _ = writeln!(
        md,
        "## Ranked findings, frames captured by this sweep (high and medium)\n\n{}",
        tables[0]
    );
    let _ = writeln!(
        md,
        "## Ranked findings, older frames from design/parity (high and medium; the frame date is beside the screen, \
         and a fault may be fixed since)\n\n{}",
        tables[1]
    );

    let _ = writeln!(md, "\n## Screens by score\n");
    let _ = writeln!(
        md,
        "| screen | frame date | pixel % | layout ids | high | medium | low | notes |"
    );
    let _ = writeln!(md, "|---|---|---|---|---|---|---|---|");
    for report in reports {
        let n = |severity| {
            report
                .findings
                .iter()
                .filter(|finding| finding.severity == severity)
                .count()
        };
        let mut notes: Vec<String> = Vec::new();
        notes.extend(report.pixel_note.clone());
        notes.extend(report.layout_note.clone());
        if !report.has_scene {
            notes.push("older frame: no clip masks or quads".into());
        }
        let _ = writeln!(
            md,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} |",
            report.key,
            report.modified,
            report.pixel.map_or("-".into(), |p| format!("{p:.2}")),
            report.layout_checked,
            n(Severity::High),
            n(Severity::Medium),
            n(Severity::Low),
            notes.join("; ")
        );
    }

    // What could not be checked.
    let captured: BTreeSet<&str> = reports.iter().map(|report| report.key.as_str()).collect();
    let mut uncaptured = Vec::new();
    let golden_root = design.join("golden").join("windows");
    for screen in &context.golden_screens {
        for entry in fs::read_dir(golden_root.join(screen))
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(variant) = name.strip_suffix("@1.png") {
                let key = format!("{screen}/{variant}");
                if !captured.contains(key.as_str()) {
                    uncaptured.push(key);
                }
            }
        }
    }
    let no_golden: Vec<&str> = reports
        .iter()
        .filter(|report| report.pixel_note.as_deref() == Some("no golden"))
        .map(|report| report.key.as_str())
        .collect();
    let no_dom: Vec<&str> = reports
        .iter()
        .filter(|report| report.layout_note.is_some())
        .map(|report| report.key.as_str())
        .collect();
    let _ = writeln!(md, "\n## Not checked\n");
    let _ = writeln!(
        md,
        "- Frames without a golden (structural checks only): {}\n",
        no_golden.len()
    );
    for key in &no_golden {
        let _ = writeln!(md, "  - `{key}`");
    }
    let _ = writeln!(
        md,
        "- Frames without a DOM dump or parity map (no layout-tree compare): {}\n",
        no_dom.len()
    );
    for key in &no_dom {
        let _ = writeln!(md, "  - `{key}`");
    }
    let _ = writeln!(
        md,
        "- Goldens with no frame here (no capture script reached them): {}\n",
        uncaptured.len()
    );
    for key in &uncaptured {
        let _ = writeln!(md, "  - `{key}`");
    }
    if !unreadable.is_empty() {
        let _ = writeln!(
            md,
            "- Frames that could not be read: {}\n",
            unreadable.len()
        );
        for line in unreadable {
            let _ = writeln!(md, "  - {line}");
        }
    }
    fs::write(out.join("report.md"), &md)?;
    fs::write(out.join("findings.tsv"), &tsv)?;
    Ok(format!(
        "drift: {} frames, {} high, {} medium findings; {} goldens without a frame. Report: {}",
        reports.len(),
        count(Severity::High),
        count(Severity::Medium),
        uncaptured.len(),
        out.join("report.md").display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(id: &str, rect: Rect, mask: Rect) -> Probe {
        Probe {
            id: id.into(),
            rect,
            mask,
        }
    }

    fn scene(probes: Vec<Probe>, quads: Vec<Quad>) -> Scene {
        Scene {
            window: (1280., 720.),
            probes,
            quads,
        }
    }

    const WINDOW: Rect = [0., 0., 1280., 720.];

    #[test]
    fn a_probe_cut_on_the_right_by_its_ancestor_is_high() {
        let s = scene(
            vec![probe(
                "field",
                [100., 100., 328., 40.],
                [90., 0., 250., 720.],
            )],
            vec![],
        );
        let found = clipped_probes(&s, &HashMap::new());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::High);
        assert!(
            found[0].what.contains("cut on the right"),
            "{}",
            found[0].what
        );
        assert!((found[0].magnitude - 88.).abs() < 0.01);
    }

    #[test]
    fn scrolled_away_and_border_deep_cuts_are_not_findings() {
        let s = scene(
            vec![
                probe("gone", [0., 900., 100., 40.], [0., 0., 1280., 700.]),
                probe("bordered", [10., 10., 102., 40.], [11., 11., 100., 38.]),
            ],
            vec![],
        );
        assert!(clipped_probes(&s, &HashMap::new()).is_empty());
    }

    #[test]
    fn vertical_cuts_rank_low() {
        let s = scene(
            vec![probe("row", [0., 680., 300., 40.], [0., 100., 400., 600.])],
            vec![],
        );
        assert_eq!(
            clipped_probes(&s, &HashMap::new())[0].severity,
            Severity::Low
        );
    }

    #[test]
    fn past_the_window_needs_the_window_as_clip() {
        let s = scene(
            vec![
                probe("wide", [0., 0., 1400., 100.], WINDOW),
                probe("scrolled", [0., 0., 1400., 100.], [0., 0., 500., 500.]),
            ],
            vec![],
        );
        let found = overflow_window(&s);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id.as_deref(), Some("wide"));
    }

    #[test]
    fn siblings_overlap_unless_official_does_too() {
        let s = scene(
            vec![
                probe("panel", [0., 0., 600., 600.], WINDOW),
                probe("left", [0., 0., 300., 100.], WINDOW),
                probe("right", [280., 0., 300., 100.], WINDOW),
                probe("inner", [10., 10., 50., 50.], WINDOW),
            ],
            vec![],
        );
        let none = HashMap::new();
        let found = overlapping_siblings(&s, &none);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].severity, Severity::Low);
        let mut official = HashMap::new();
        official.insert("left".to_owned(), Some([0., 0., 300., 100.]));
        official.insert("right".to_owned(), Some([300., 0., 300., 100.]));
        assert_eq!(
            overlapping_siblings(&s, &official)[0].severity,
            Severity::High
        );
        official.insert("right".to_owned(), Some([280., 0., 300., 100.]));
        assert!(overlapping_siblings(&s, &official).is_empty());
    }

    fn quad(rect: Rect, alpha: f64) -> Quad {
        Quad {
            rect,
            mask: WINDOW,
            alpha,
            border: 0.,
        }
    }

    #[test]
    fn a_dialog_off_centre_or_larger_than_the_window_is_found() {
        let centred = scene(
            vec![],
            vec![
                quad(WINDOW, 1.),
                quad(WINDOW, 0.7),
                quad([340., 160., 600., 400.], 1.),
            ],
        );
        assert!(modal_checks(&centred).is_empty());
        let off = scene(
            vec![],
            vec![quad(WINDOW, 0.7), quad([100., 160., 600., 400.], 1.)],
        );
        let found = modal_checks(&off);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "modal-centre");
        let big = scene(
            vec![],
            vec![quad(WINDOW, 0.7), quad([-20., 40., 1320., 640.], 1.)],
        );
        assert!(modal_checks(&big).iter().any(|f| f.kind == "modal-size"));
    }

    #[test]
    fn css_rules_are_indexed_by_exact_class_with_their_media() {
        let mut styles = Styles::default();
        parse_css(
            ".a_1{min-width:328px;padding:0 8px}.a_1 .b{width:9px}.c,.a_1{width:400px}\
             @media (max-width:485px){.a_1{min-width:100px}}/* .a_1{width:1px} */\
             @supports (x:y){.a_1{width:2px}}",
            &mut styles,
        );
        let rules = &styles.rules["a_1"];
        assert_eq!(rules.len(), 4);
        assert_eq!(rules[2].media.as_deref(), Some("@media (max-width:485px)"));
        assert!(
            applicable(rules, (1280., 720.)).is_none(),
            "@supports cannot be told"
        );
        let decls = applicable(&rules[..3], (1280., 720.)).unwrap();
        assert!(!decls.iter().any(|(_, value)| value == "100px"));
        assert_eq!(horizontal_extra(&decls), Some(16.));
        let narrow = applicable(&rules[..3], (400., 720.)).unwrap();
        assert!(narrow.iter().any(|(_, value)| value == "100px"));
    }

    #[test]
    fn a_probe_narrower_than_its_class_min_width_is_high() {
        let mut styles = Styles::default();
        parse_css(
            ".panel_X{min-width:328px;box-sizing:border-box;padding:16px}",
            &mut styles,
        );
        let s = scene(vec![probe("panel", [0., 0., 240., 500.], WINDOW)], vec![]);
        let map = serde_json::json!({"panel": {"class": "panel_X"}});
        let mut skipped = 0;
        let found = css_checks(&s, &map, &styles, |_| true, &mut skipped);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::High);
        assert!(found[0].what.contains("328"));
    }

    #[test]
    fn media_queries_of_sizes_are_evaluated_and_others_are_not() {
        assert_eq!(
            media_matches("@media (min-width:900px)", (1280., 720.)),
            Some(true)
        );
        assert_eq!(
            media_matches(
                "@media screen and (max-height:550px),screen and (max-width:485px)",
                (1280., 720.)
            ),
            Some(false)
        );
        assert_eq!(
            media_matches("@media (min-width:80rem)", (1280., 720.)),
            Some(true)
        );
        assert_eq!(
            media_matches("@media (prefers-reduced-motion:reduce)", (1280., 720.)),
            None
        );
    }

    #[test]
    fn frame_names_split_into_screen_and_variant() {
        let screens: BTreeSet<String> = ["settings", "settings-full", "channel"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            split_key("settings-profile-editor-darker-cozy-latest", &screens),
            ("settings".into(), "profile-editor-darker-cozy".into())
        );
        assert_eq!(
            split_key("settings__server-profile-1280", &screens),
            ("settings".into(), "server-profile-1280".into())
        );
        assert_eq!(
            split_key("unknown-x", &screens),
            ("unknown-x".into(), String::new())
        );
    }

    #[test]
    fn shots_point_into_the_frame_folder() {
        let (script, named) = expand_shots(
            "wait 100; shot @settings/a-darker-cozy; settings b; shot @settings/b",
            Path::new("out"),
        );
        assert_eq!(named, ["settings/a-darker-cozy", "settings/b"]);
        assert!(script.contains("shot out"));
        assert!(script.contains("settings__a-darker-cozy.png"));
    }

    #[test]
    fn a_cut_where_the_official_box_is_or_a_shallow_overhang_is_not_high() {
        let s = scene(
            vec![
                probe("pill", [-4., 100., 8., 40.], [0., 32., 72., 600.]),
                probe("same", [100., 100., 328., 40.], [90., 0., 250., 720.]),
            ],
            vec![],
        );
        let mut official = HashMap::new();
        official.insert("same".to_owned(), Some([100., 100., 328., 40.]));
        let found = clipped_probes(&s, &official);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id.as_deref(), Some("pill"));
        assert_eq!(found[0].severity, Severity::Low);
    }

    #[test]
    fn the_row_nearest_the_official_box_is_compared() {
        let s = scene(
            vec![
                probe("card", [395., 370., 272., 246.], WINDOW),
                probe("card", [990., 15708., 268., 242.], WINDOW),
            ],
            vec![],
        );
        let mut official = HashMap::new();
        official.insert("card".to_owned(), Some([395., 370., 272., 246.]));
        official.insert("gone".to_owned(), Some([0., 0., 10., 10.]));
        let found = layout_findings(&s, &official);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, "layout-missing");
        official.insert("card".to_owned(), Some([395., 370., 300., 246.]));
        let found = layout_findings(&s, &official);
        assert!(found.iter().any(|f| f.kind == "layout"
            && f.severity == Severity::High
            && f.what.contains("width 272 (official 300)")));
    }

    #[test]
    fn a_line_running_into_a_clip_edge_is_found_and_padded_text_is_not() {
        let background = Rgba([30, 30, 30, 255]);
        let mut frame = RgbaImage::from_pixel(400, 100, background);
        let clip = Quad {
            rect: [10., 10., 100., 20.],
            mask: [0., 0., 200., 100.],
            alpha: 1.,
            border: 0.,
        };
        let s = Scene {
            window: (400., 100.),
            probes: vec![probe("panel", [0., 0., 200., 100.], [0., 0., 400., 100.])],
            quads: vec![clip],
        };
        // Padded text: ends 16px before the edge.
        for y in [40, 41, 43, 44, 46, 47] {
            for x in 150..184 {
                frame.put_pixel(x, y, Rgba([220, 220, 220, 255]));
            }
        }
        assert!(edge_cuts(&s, &frame, None).is_empty());
        // The same line running to the edge.
        for y in [40, 41, 43, 44, 46, 47] {
            for x in 184..200 {
                frame.put_pixel(x, y, Rgba([220, 220, 220, 255]));
            }
        }
        let found = edge_cuts(&s, &frame, None);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].what.contains("inside `panel`"));
        // Under a dialog painted later from outside the clip, the pixels are the dialog's.
        let mut covered = s.quads.clone();
        covered.push(Quad {
            rect: [150., 0., 250., 100.],
            mask: [0., 0., 400., 100.],
            alpha: 1.,
            border: 0.,
        });
        let under = Scene {
            window: s.window,
            probes: vec![],
            quads: covered,
        };
        assert!(edge_cuts(&under, &frame, None).is_empty());
        // A border down the whole edge is not text.
        let mut bordered = RgbaImage::from_pixel(400, 100, background);
        for y in 0..100 {
            for x in 196..200 {
                bordered.put_pixel(x, y, Rgba([90, 90, 200, 255]));
            }
        }
        assert!(edge_cuts(&s, &bordered, None).is_empty());
        // The official frame cut the same way: design, not drift.
        assert!(edge_cuts(&s, &frame, Some(&frame)).is_empty());
    }
}
