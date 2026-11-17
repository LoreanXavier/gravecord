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
//! difference is retried twice with a fresh reference (a minute can turn over between two runs and change the time of
//! a message sent in the scenario); a third difference fails the check and leaves a diff image next to the frames.

use anyhow::{Context, Result, bail};
use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// The scenario of the negative control ([`Mode::Control`]).
const CONTROL_SCENARIO: &str = "double-click";
const SCENARIOS: &str = "tests/plugins-off/scenarios.txt";
/// Startup, the script and the capture: the longest scenario takes about a minute.
const RUN_LIMIT: Duration = Duration::from_secs(240);

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
    /// The negative control: MessageClickActions alone on. Its double-click scenario must differ from the reference
    /// and count effects, or the check could not see a plugin at all.
    Control,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Reference => "reference",
            Mode::MasterOff => "master-off",
            Mode::EachOff => "each-off",
            Mode::Control => "control",
        }
    }

    /// The script of the run: the state set up first, the effect counter cleared, the scenario, and the counter read.
    fn script(self, scenario: &str) -> String {
        // The motion clock moves two seconds on, to a multiple of 2.4 s (the typing dots' period), and stops before the
        // capture: every transition has ended, and a caret or a looping animation is in the same phase in every run.
        let scenario = format!("{scenario}; motion clock +2000@2400; wait 300");
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
            Mode::Control => format!(
                "plugin master on; plugin all off; plugin on MessageClickActions; plugin effects; {scenario}; plugin effects"
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
    let pid = super::headless::spawn(exe, &args, repo)?;
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
    Ok((frame, text))
}

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
    for scenario in chosen {
        let mut reference = capture(repo, &exe, &out, Mode::Reference, scenario)?.0;
        for mode in [Mode::MasterOff, Mode::EachOff] {
            let mut verdict = Ok(());
            for attempt in 0..3 {
                if attempt > 0 {
                    reference = capture(repo, &exe, &out, Mode::Reference, scenario)?.0;
                }
                let (frame, log) = capture(repo, &exe, &out, mode, scenario)?;
                verdict = match (quiet(&log), compare(&reference, &frame)) {
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
                            "{count} pixels differ in {bbox:?}, see {}",
                            path.display()
                        ))
                    }
                    (Some(true), None) => Ok(()),
                };
                if verdict.is_ok() {
                    break;
                }
            }
            match verdict {
                Ok(()) => println!("ok    {} {}", scenario.name, mode.name()),
                Err(why) => {
                    println!("FAIL  {} {}: {why}", scenario.name, mode.name());
                    failures.push(format!("{} {}", scenario.name, mode.name()));
                }
            }
        }
    }
    // The control runs whenever the double-click scenario does.
    if let Some(scenario) = all
        .iter()
        .find(|scenario| scenario.name == CONTROL_SCENARIO)
        && (args.is_empty() || args.contains(&CONTROL_SCENARIO))
    {
        let reference = capture(repo, &exe, &out, Mode::Reference, scenario)?.0;
        let (frame, log) = capture(repo, &exe, &out, Mode::Control, scenario)?;
        if quiet(&log) == Some(false) && compare(&reference, &frame).is_some() {
            println!(
                "ok    {CONTROL_SCENARIO} control: MessageClickActions on is seen (frame and effects differ)"
            );
        } else {
            println!("FAIL  {CONTROL_SCENARIO} control: MessageClickActions on was not seen");
            failures.push(format!("{CONTROL_SCENARIO} control"));
        }
    }
    if !failures.is_empty() {
        bail!(
            "plugins-off: {} run(s) differ from the pinned client: {}",
            failures.len(),
            failures.join(", ")
        );
    }
    println!("plugins-off: every scenario is identical to the reference with the plugins off");
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
        assert!(script.ends_with("wait 10; motion clock +2000@2400; wait 300; plugin effects"));
        assert_eq!(
            Mode::Reference.script("wait 10"),
            "wait 10; motion clock +2000@2400; wait 300"
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
