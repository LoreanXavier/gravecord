//! `cargo xtask plugins-off [name...]`: proves that the built-in plugins leave the app as the pinned client while they
//! are off.
//!
//! Every scenario in `tests/plugins-off/scenarios.txt` is captured headless (`headless.rs`) in three runs of the same
//! capture build:
//!
//! - `reference`: `GRAVECORD_PLUGINS_BYPASS=1`, every hook site takes the pinned client's path without reading the
//!   plugin state (`app/src/plugins.rs`, `bypassed`);
//! - `master-off`: the master switch off, every plugin turned on and every setting changed from its default;
//! - `each-off`: the master switch on and every plugin turned off.
//!
//! The two plugin runs must be pixel-identical to the reference, and their harness log must end with
//! `plugin: effects {}`: no plugin passed its gate, wrote its storage or compiled its rules during the scenario. A
//! difference is retried up to three times, each time with one more plugin-free reference (a minute can turn over between two runs
//! and change the time of a message sent in the scenario, and the GPU can draw a picture's edge pixels differently
//! from run to run): a run passes when every pixel equals that pixel of a reference, except where the references
//! themselves disagree (drawn differently with no plugin code at all), which the report counts. A fourth difference
//! fails the check and leaves a diff image next to the frames.

use anyhow::{Context, Result, bail};
use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// The negative controls ([`Mode::Control`]): a scenario, the plugin set up for it, and what it must show. Each must be
/// caught (its frame differs where the plugin-free references agree, and it counts effects), or the check could not see
/// a plugin there. The second draws beside the chat's media gallery, whose picture edges vary between plugin-free runs
/// (the only pixels a run may leave unproven), so a plugin drawing next to unstable pixels is shown to be caught.
const CONTROLS: &[(&str, &str, &str)] = &[
    (
        "double-click",
        "plugin on MessageClickActions",
        "MessageClickActions on",
    ),
    (
        "media-gallery",
        "plugin on BetterRoleDot; plugin set BetterRoleDot bothStyles true",
        "BetterRoleDot's role dots beside the names, next to the media gallery's unstable picture edges",
    ),
];
const SCENARIOS: &str = "tests/plugins-off/scenarios.txt";
/// Startup, the script and the capture: the longest scenario takes about a minute.
const RUN_LIMIT: Duration = Duration::from_secs(240);
/// The settle step before the capture, and how long it may take.
const SETTLE: &str = "settle 20000";

/// The least time between two signed-in launches: Discord flags an account that signs in many times in a few
/// minutes (twice on 2026-11-18). `GRAVECORD_LAUNCH_GAP_S` changes it.
const LAUNCH_GAP: Duration = Duration::from_secs(60);

fn launch_gap() -> Duration {
    std::env::var("GRAVECORD_LAUNCH_GAP_S")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .map_or(LAUNCH_GAP, Duration::from_secs)
}

thread_local! {
    static LAST_LAUNCH: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// The settle step's line, for a failure's report.
fn settled(log: &str) -> &str {
    log.lines()
        .rev()
        .find(|line| line.starts_with("settle:"))
        .unwrap_or("settle: no line (the harness stopped before it)")
}

struct Scenario {
    name: String,
    script: String,
}

fn scenarios(repo: &Path) -> Result<Vec<Scenario>> {
    let text = std::fs::read_to_string(repo.join(SCENARIOS)).context("reading the scenarios")?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (name, script) = line.split_once('|')?;
            Some(Scenario {
                name: name.trim().to_owned(),
                script: script.trim().to_owned(),
            })
        })
        .collect())
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Reference,
    MasterOff,
    EachOff,
    /// A negative control: one plugin set up alone ([`CONTROLS`]).
    Control(&'static str),
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Reference => "reference",
            Mode::MasterOff => "master-off",
            Mode::EachOff => "each-off",
            Mode::Control(_) => "control",
        }
    }

    /// The script of the run: the state set up first, the effect counter cleared, the scenario, and the counter read.
    fn script(self, scenario: &str) -> String {
        // The motion clock moves two seconds on, to a multiple of 2.4 s (the typing dots' period), and stops before the
        // capture: every transition has ended, and a caret or a looping animation is in the same phase in every run.
        // GIF tiles hold their first frame (`gifs still`), since they play on the real clock. Then the window must
        // stay the same picture for a second (`settle`): what loads at its own pace (pictures,
        // GIF results, call tiles) has arrived, however loaded the machine is.
        let scenario = format!(
            "{scenario}; motion clock +2000@2400; gifs still; {SETTLE}; wait 300; signed-in"
        );
        match self {
            Mode::Reference => scenario,
            Mode::MasterOff => format!(
                "plugin master off; plugin all on; plugin scramble; plugin effects; {scenario}; plugin effects"
            ),
            Mode::EachOff => {
                format!(
                    "plugin master on; plugin all off; plugin effects; {scenario}; plugin effects"
                )
            }
            Mode::Control(setup) => format!(
                "plugin master on; plugin all off; {setup}; plugin effects; {scenario}; plugin effects"
            ),
        }
    }
}

fn target_dir(repo: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| repo.join("target"), PathBuf::from)
}

fn running(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(&pid.to_string()))
}

/// One headless capture; returns the frame and the harness log.
fn capture(
    repo: &Path,
    exe: &Path,
    out: &Path,
    mode: Mode,
    scenario: &Scenario,
) -> Result<(RgbaImage, String)> {
    let png = out.join(format!("{}.{}.png", scenario.name, mode.name()));
    let log = png.with_extension("log");
    let _ = std::fs::remove_file(&png);
    let _ = std::fs::remove_file(&log);
    // The child inherits this process's environment (`headless::spawn` passes none of its own).
    // SAFETY: xtask is single-threaded here; nothing else reads the environment while it changes.
    unsafe {
        for (key, value) in [
            ("GRAVECORD_INERT", "1"),
            ("GRAVECORD_NO_SETTINGS_WRITE", "1"),
            ("GRAVECORD_CACHE", "1"),
        ] {
            std::env::set_var(key, value);
        }
        if mode == Mode::Reference {
            std::env::set_var("GRAVECORD_PLUGINS_BYPASS", "1");
        } else {
            std::env::remove_var("GRAVECORD_PLUGINS_BYPASS");
        }
    }
    let args = vec![
        "--capture".to_owned(),
        png.display().to_string(),
        "300".to_owned(),
        "--script".to_owned(),
        mode.script(&scenario.script),
    ];
    if let Some(last) = LAST_LAUNCH.with(std::cell::Cell::get) {
        let wait = launch_gap().saturating_sub(last.elapsed());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
    let pid = super::headless::spawn(exe, &args, repo)?;
    LAST_LAUNCH.with(|cell| cell.set(Some(Instant::now())));
    let started = Instant::now();
    while running(pid) {
        if started.elapsed() > RUN_LIMIT {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output();
            bail!(
                "{} {} did not finish in {RUN_LIMIT:?}",
                scenario.name,
                mode.name()
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let frame = image::open(&png)
        .with_context(|| format!("{} {}: no frame", scenario.name, mode.name()))?
        .to_rgba8();
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    // A refused login turns every later run into the login screen: stop rather than sign in again with it.
    if text.lines().any(|line| line == "signed-in: no") {
        bail!(
            "{} {}: the stored login was refused (401 or 4004); stopping so the account is not signed in again",
            scenario.name,
            mode.name()
        );
    }
    Ok((frame, text))
}

/// A run against every plugin-free reference so far: a pixel differs only when it matches none of them at that place
/// and the place is not unstable. Unstable is where the plugin-free references disagree among themselves, and the pixels
/// touching those (the GPU draws a picture's edge pixels differently from run to run with no plugin code at all, so
/// those pixels prove nothing either way). Also the number of unstable pixels.
fn compare_any(
    references: &[RgbaImage],
    frame: &RgbaImage,
) -> (Option<(usize, [u32; 4], RgbaImage)>, Unstable) {
    let first = &references[0];
    let (w, h) = first.dimensions();
    if references
        .iter()
        .any(|reference| reference.dimensions() != (w, h))
        || frame.dimensions() != (w, h)
    {
        return (compare(first, frame), Unstable::default());
    }
    let disagree: Vec<bool> = first
        .enumerate_pixels()
        .map(|(x, y, pixel)| {
            references[1..]
                .iter()
                .any(|other| other.get_pixel(x, y) != pixel)
        })
        .collect();
    let at = |x: u32, y: u32| (y * w + x) as usize;
    let unstable = |x: u32, y: u32| {
        (x.saturating_sub(1)..=(x + 1).min(w - 1))
            .any(|nx| (y.saturating_sub(1)..=(y + 1).min(h - 1)).any(|ny| disagree[at(nx, ny)]))
    };
    let mut varying = 0;
    let mut spread = [u32::MAX, u32::MAX, 0, 0];
    for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
        if unstable(x, y) {
            varying += 1;
            spread = [
                spread[0].min(x),
                spread[1].min(y),
                spread[2].max(x),
                spread[3].max(y),
            ];
        }
    }
    let varying = Unstable {
        pixels: varying,
        spread,
    };
    let Some((_, _, mut out)) = compare(first, frame) else {
        return (None, varying);
    };
    let mut count = 0;
    let mut bbox = [u32::MAX, u32::MAX, 0, 0];
    for (x, y, pixel) in frame.enumerate_pixels() {
        if unstable(x, y)
            || references
                .iter()
                .any(|reference| reference.get_pixel(x, y) == pixel)
        {
            if out.get_pixel(x, y) == &Rgba([255, 0, 255, 255]) {
                let Rgba([r, g, b, _]) = *pixel;
                out.put_pixel(x, y, Rgba([r / 4, g / 4, b / 4 + 64, 255]));
            }
            continue;
        }
        count += 1;
        bbox = [
            bbox[0].min(x),
            bbox[1].min(y),
            bbox[2].max(x),
            bbox[3].max(y),
        ];
    }
    ((count > 0).then_some((count, bbox, out)), varying)
}

/// The pixels a run leaves unproven: where the plugin-free references disagree, and their neighbours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Unstable {
    pixels: usize,
    /// Their bounding box (x, y, last x, last y).
    spread: [u32; 4],
}

impl std::fmt::Display for Unstable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.pixels == 0 {
            return write!(f, "no unstable pixels");
        }
        write!(f, "{} unstable pixels in {:?}", self.pixels, self.spread)
    }
}

/// At most this share of a frame may be unstable (in thousandths); past it the scenario proves too little and fails.
const UNSTABLE_LIMIT_PERMILLE: usize = 5;

/// The pixels that differ and their bounding box.
fn compare(left: &RgbaImage, right: &RgbaImage) -> Option<(usize, [u32; 4], RgbaImage)> {
    if left.dimensions() != right.dimensions() {
        let (w, h) = left.dimensions();
        return Some((usize::MAX, [0, 0, w, h], RgbaImage::new(1, 1)));
    }
    let (w, h) = left.dimensions();
    let mut diff = RgbaImage::new(w, h);
    let mut count = 0;
    let mut bbox = [u32::MAX, u32::MAX, 0, 0];
    for (x, y, pixel) in left.enumerate_pixels() {
        let other = right.get_pixel(x, y);
        if pixel == other {
            let Rgba([r, g, b, _]) = *pixel;
            diff.put_pixel(x, y, Rgba([r / 4, g / 4, b / 4, 255]));
            continue;
        }
        count += 1;
        diff.put_pixel(x, y, Rgba([255, 0, 255, 255]));
        bbox = [
            bbox[0].min(x),
            bbox[1].min(y),
            bbox[2].max(x),
            bbox[3].max(y),
        ];
    }
    (count > 0).then_some((count, bbox, diff))
}

/// The counter line read after the scenario (the second `plugin: effects`; the first clears the setup's writes):
/// `None` when the scenario stopped before it, else whether it is empty.
fn quiet(log: &str) -> Option<bool> {
    let counts: Vec<&str> = log
        .lines()
        .filter(|line| line.starts_with("plugin: effects"))
        .collect();
    let [_, after] = counts.as_slice() else {
        return None;
    };
    Some(after.trim() == "plugin: effects {}")
}

pub fn run(repo: &Path, args: &[&str]) -> Result<()> {
    let target = target_dir(repo);
    let exe = target.join("debug").join("gravecord.exe");
    let status = Command::new("cargo")
        .args(["build", "-p", "gravecord", "--features", "capture"])
        .current_dir(repo)
        .status()
        .context("building the capture build")?;
    if !status.success() {
        bail!("the capture build failed");
    }
    let out = target.join("plugins-off");
    std::fs::create_dir_all(&out)?;
    let all = scenarios(repo)?;
    let chosen: Vec<&Scenario> = all
        .iter()
        .filter(|scenario| args.is_empty() || args.contains(&scenario.name.as_str()))
        .collect();
    if chosen.is_empty() {
        bail!("no scenario matches {args:?}");
    }
    let mut failures = Vec::new();
    // The pixels each run left unproven, for the report: scenario, mode, how many and where.
    let mut unproven: Vec<(String, &str, Unstable)> = Vec::new();
    let mut references_of: std::collections::HashMap<String, Vec<RgbaImage>> = Default::default();
    for scenario in chosen {
        // Every plugin-free reference of this scenario; a difference brings one more (at most four in all).
        let mut references = vec![capture(repo, &exe, &out, Mode::Reference, scenario)?.0];
        for mode in [Mode::MasterOff, Mode::EachOff] {
            let mut verdict = Ok(Unstable::default());
            for attempt in 0..4 {
                if attempt > 0 && references.len() < 4 {
                    references.push(capture(repo, &exe, &out, Mode::Reference, scenario)?.0);
                }
                let (frame, log) = capture(repo, &exe, &out, mode, scenario)?;
                let frames = format!(
                    "reference {}, run {}; {}",
                    out.join(format!("{}.reference.png", scenario.name))
                        .display(),
                    out.join(format!("{}.{}.png", scenario.name, mode.name()))
                        .display(),
                    settled(&log)
                );
                let (difference, varying) = compare_any(&references, &frame);
                verdict = match (quiet(&log), difference) {
                    (None, _) => {
                        Err("no `plugin: effects` line (the harness did not finish)".to_owned())
                    }
                    (Some(false), _) => Err(format!(
                        "a plugin acted: {}",
                        log.lines()
                            .rev()
                            .find(|line| line.starts_with("plugin: effects"))
                            .unwrap_or("")
                    )),
                    (Some(true), Some((count, bbox, diff))) => {
                        let path = out.join(format!("{}.{}.diff.png", scenario.name, mode.name()));
                        let _ = diff.save(&path);
                        Err(format!(
                            "{count} pixels differ in {bbox:?}, diff {}; {frames}",
                            path.display()
                        ))
                    }
                    (Some(true), None)
                        if varying.pixels * 1000
                            > UNSTABLE_LIMIT_PERMILLE
                                * (frame.width() * frame.height()) as usize =>
                    {
                        Err(format!(
                            "{} pixels vary between plugin-free runs (in {:?}), more than a scenario may leave unproven; {frames}",
                            varying.pixels, varying.spread
                        ))
                    }
                    (Some(true), None) => Ok(varying),
                };
                if verdict.is_ok() {
                    break;
                }
            }
            match verdict {
                Ok(varying) if varying.pixels == 0 => {
                    println!("ok    {} {}", scenario.name, mode.name())
                }
                Ok(varying) => {
                    println!(
                        "ok    {} {} (exact except {} unstable pixels in {:?}, see the list below)",
                        scenario.name,
                        mode.name(),
                        varying.pixels,
                        varying.spread
                    );
                    unproven.push((scenario.name.clone(), mode.name(), varying));
                }
                Err(why) => {
                    println!("FAIL  {} {}: {why}", scenario.name, mode.name());
                    failures.push(format!("{} {}", scenario.name, mode.name()));
                }
            }
        }
        references_of.insert(scenario.name.clone(), references);
    }
    // Each control runs whenever its scenario does, against the references its scenario gathered (two at least, so
    // the unstable pixels are the same kind a plugin run may leave unproven).
    for (name, setup, what) in CONTROLS {
        let Some(scenario) = all.iter().find(|scenario| scenario.name == *name) else {
            continue;
        };
        if !(args.is_empty() || args.contains(name)) {
            continue;
        }
        let mut references = references_of.remove(*name).unwrap_or_default();
        while references.len() < 2 {
            references.push(capture(repo, &exe, &out, Mode::Reference, scenario)?.0);
        }
        let (frame, log) = capture(repo, &exe, &out, Mode::Control(setup), scenario)?;
        match (quiet(&log), compare_any(&references, &frame)) {
            (Some(false), (Some((count, bbox, _)), varying)) => println!(
                "ok    {name} control: {what} is caught ({count} pixels differ in {bbox:?} where the plugin-free runs agree; {varying}; effects counted)",
            ),
            _ => {
                println!("FAIL  {name} control: {what} was not caught");
                failures.push(format!("{name} control"));
            }
        }
    }
    if !unproven.is_empty() {
        println!(
            "unstable pixels (drawn differently between plugin-free runs, so not compared; picture edges the GPU samples differently from run to run, with their neighbours):"
        );
        for (name, mode, varying) in &unproven {
            println!(
                "  {name} {mode}: {} pixels in {:?}",
                varying.pixels, varying.spread
            );
        }
    }
    if !failures.is_empty() {
        bail!(
            "plugins-off: {} run(s) differ from the pinned client: {}",
            failures.len(),
            failures.join(", ")
        );
    }
    println!(
        "plugins-off: every scenario is identical to the reference with the plugins off (unstable pixels listed above)"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counter_line_decides_quiet() {
        assert_eq!(
            quiet("plugin: all on\nplugin: effects {\"storage:plugins\":1}\nplugin: effects {}\n"),
            Some(true)
        );
        assert_eq!(
            quiet("plugin: effects {}\nplugin: effects {\"ClearURLs\":1}"),
            Some(false)
        );
        // A scenario that stopped half way never reached its counter.
        assert_eq!(
            quiet("plugin: effects {\"storage:plugins\":2}\nmember profile: script stopped"),
            None
        );
        assert_eq!(quiet("wait"), None);
    }

    #[test]
    fn a_run_must_match_the_references_wherever_they_agree() {
        let pixel = |value: u8| Rgba([value, value, value, 255]);
        let a = RgbaImage::from_pixel(4, 1, pixel(1));
        let mut b = a.clone();
        b.put_pixel(0, 0, pixel(9));
        // The references disagree at x 0, so x 0 and its neighbour x 1 are unstable: any value passes there.
        let mut run = a.clone();
        run.put_pixel(0, 0, pixel(5));
        run.put_pixel(1, 0, pixel(6));
        let (difference, varying) = compare_any(&[a.clone(), b.clone()], &run);
        assert_eq!(
            (difference.map(|found| found.0), varying),
            (
                None,
                Unstable {
                    pixels: 2,
                    spread: [0, 0, 1, 0]
                }
            )
        );
        // Elsewhere the run must be exact.
        run.put_pixel(3, 0, pixel(7));
        assert_eq!(
            compare_any(&[a.clone(), b], &run)
                .0
                .map(|found| (found.0, found.1)),
            Some((1, [3, 0, 3, 0]))
        );
        // With one reference nothing is unstable.
        assert_eq!(compare_any(&[a], &run).0.map(|found| found.0), Some(3));
    }

    #[test]
    fn identical_frames_compare_equal() {
        let a = RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255]));
        let mut b = a.clone();
        assert!(compare(&a, &b).is_none());
        b.put_pixel(2, 1, Rgba([9, 9, 9, 255]));
        let (count, bbox, _) = compare(&a, &b).unwrap();
        assert_eq!((count, bbox), (1, [2, 1, 2, 1]));
    }

    #[test]
    fn plugin_runs_set_up_the_state_and_read_the_counter() {
        let script = Mode::MasterOff.script("wait 10");
        assert!(
            script
                .starts_with("plugin master off; plugin all on; plugin scramble; plugin effects;")
        );
        assert!(script.ends_with(
            "wait 10; motion clock +2000@2400; gifs still; settle 20000; wait 300; signed-in; plugin effects"
        ));
        assert_eq!(
            Mode::Reference.script("wait 10"),
            "wait 10; motion clock +2000@2400; gifs still; settle 20000; wait 300; signed-in"
        );
    }

    #[test]
    fn the_scenarios_parse() {
        let repo = super::super::repo_root();
        let all = scenarios(&repo).unwrap();
        assert!(all.len() >= 10);
        assert!(
            all.iter()
                .all(|scenario| !scenario.script.contains("plugin "))
        );
    }
}
