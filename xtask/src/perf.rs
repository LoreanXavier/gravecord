//! `cargo xtask perf [--release] [--runs N] [--laps N] [--scenario S] [--label L] [--no-build] [--no-wait] [--idle-secs N]`
//!
//! Runs the capture build on the hidden desktop through the harness and reads the numbers its `perf` and `frames`
//! steps print to the run's log (`app/src/perf.rs`). Every run is one process doing the same scripted steps: start,
//! idle, typing, scrolling, then laps of the main destinations. The budgets are the project plan section 8.5.
//!
//! Scenarios: `all` (default), `start`, `idle`, `typing`, `scroll`, `soak`. The report goes to `design/perf/<label>.md`.
//! The real server is only selected, scrolled and read: no write is sent from a harness run.

use crate::headless;
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// The real server's chat channel; `channel click` refuses rules, Guide and voice channels.
const REAL_CHANNEL: &str = "1554855652386410528";
/// A collectibles profile effect that the harness shows over an open profile.
const EFFECT_SKU: &str = "1564419597501989044";

const BUDGET_IDLE_RSS_MB: f64 = 150.;
const TARGET_IDLE_RSS_MB: f64 = 100.;
const BUDGET_START_MS: f64 = 1500.;
const TARGET_START_MS: f64 = 1000.;
const BUDGET_INPUT_MS: f64 = 16.;
const BUDGET_FRAME_MS: f64 = 16.7;
const BUDGET_IDLE_CPU_PCT: f64 = 1.;

struct Options {
    release: bool,
    runs: usize,
    laps: usize,
    scenario: String,
    label: String,
    build: bool,
    wait: bool,
    idle_secs: u64,
    timeout: Duration,
    /// `--exe <path>`: measure this build instead of the one in `target` (a kept copy of a base build, say).
    exe: Option<PathBuf>,
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut options = Options {
        release: false,
        runs: 3,
        laps: 4,
        scenario: "all".to_owned(),
        label: "perf".to_owned(),
        build: true,
        wait: true,
        idle_secs: 30,
        timeout: Duration::from_secs(40 * 60),
        exe: None,
    };
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        match *flag {
            "--release" => options.release = true,
            "--no-build" => options.build = false,
            "--no-wait" => options.wait = false,
            "--runs" | "--laps" | "--scenario" | "--label" | "--idle-secs" | "--timeout-min"
            | "--exe" => {
                let value = rest.next().copied().context("flag without a value")?;
                match *flag {
                    "--exe" => {
                        options.exe = Some(PathBuf::from(value));
                        options.build = false;
                    }
                    "--runs" => options.runs = value.parse()?,
                    "--laps" => options.laps = value.parse()?,
                    "--scenario" => options.scenario = value.to_owned(),
                    "--label" => options.label = value.to_owned(),
                    "--idle-secs" => options.idle_secs = value.parse()?,
                    _ => options.timeout = Duration::from_secs(value.parse::<u64>()? * 60),
                }
            }
            other => bail!(
                "usage: cargo xtask perf [--release] [--runs N] [--laps N] [--scenario all|start|idle|typing|scroll|soak|stickers|hitch|anim] [--label L] [--no-build] [--no-wait] [--idle-secs N] [--timeout-min N] [--exe PATH], got {other}"
            ),
        }
    }
    if ![
        "all", "start", "idle", "typing", "scroll", "soak", "stickers", "hitch", "anim",
    ]
    .contains(&options.scenario.as_str())
    {
        bail!("unknown scenario {}", options.scenario);
    }
    Ok(options)
}

/// The steps of one lap through the main destinations, ending where it began, with the process's numbers after it.
fn lap(number: usize) -> String {
    let step = |name: &str| format!("perf sample lap{number}.{name}; ");
    let mut out = String::new();
    out.push_str("click 36 109; wait 1500; go home; wait 1200; go dm 0; wait 1500; ");
    out.push_str(&step("dm"));
    out.push_str(&format!(
        "click 36 157; wait 1500; channel click {REAL_CHANNEL}; wait 2500;          click 1000 55; wait 1000; scroll 800 400 -400; wait 400; scroll 1150 300 300; wait 600; click 1000 55; wait 400; "
    ));
    out.push_str(&step("real"));
    out.push_str("go shop; wait 2500; ");
    out.push_str(&step("shop"));
    out.push_str("go discover; wait 2500; ");
    out.push_str(&step("discover"));
    out.push_str("go quests; wait 2500; ");
    out.push_str(&step("quests"));
    out.push_str(
        "settings appearance; wait 800; settings voice-video; wait 800; settings accessibility; wait 800; click 1214 64; wait 500; ",
    );
    out.push_str(&step("settings"));
    out.push_str("click 36 109; wait 1000; click 1162 683; wait 1500; key escape; wait 500; ");
    out.push_str(&step("stickers"));
    out.push_str(&format!(
        "click 411 374; wait 800; profile-effect {EFFECT_SKU}; wait 2500; profile-effect off; key escape; wait 500; "
    ));
    out.push_str(&step("effect"));
    out.push_str(&format!(
        "click 36 109; wait 1000; perf sample lap{number}; perf caches lap{number}; "
    ));
    out
}

fn script(options: &Options) -> String {
    let all = options.scenario == "all";
    let mut steps = String::from(
        "perf start; perf sample start; perf caches start; perf heap; perf ws; perf badge; ",
    );
    // The stages that come after the first content (the pictures, the member list), before anything is clicked.
    steps.push_str("wait 3000; perf timeline; ");
    if all || options.scenario == "stickers" {
        // The first paint of a channel with stickers (the fixture's text channel is open at the start), then the picker
        // opening for the first time and a second time.
        steps.push_str(
            "frames watch 3000 first_channel_stickers; sticker state; go home; wait 800; click 36 109; wait 1500;              frames click 1162 683 picker_open; frames watch 4000 picker_first; sticker state; key escape; wait 800;              frames click 1162 683 picker_reopen; frames watch 3000 picker_second; sticker state; key escape; wait 800; ",
        );
    }
    if all || options.scenario == "idle" || options.scenario == "start" {
        steps.push_str(&format!(
            "click 36 109; wait 2000; perf sample idle0; wait {ms}; perf sample idle1; perf caches idle1; perf heap; perf ws; wait {ms}; perf sample idle2; go home; wait 2000; perf sample home0; wait {ms}; perf sample home1; ",
            ms = options.idle_secs * 1000
        ));
    }
    if all || options.scenario == "scroll" {
        // Fast switching between the fixture server and the real one (only selecting them).
        steps.push_str("click 36 109; wait 1500; frames alternate 10 36 109 36 157 server_switch; click 36 109; wait 1500; ");
    }
    if all || options.scenario == "typing" {
        steps.push_str(
            "click 36 109; wait 800; click 700 683; wait 300; frames type the quick brown fox jumps over the lazy dog; key ctrl-a; key backspace; wait 300; ",
        );
    }
    if all || options.scenario == "scroll" {
        steps.push_str(&format!(
            "click 36 157; wait 2500; channel click {REAL_CHANNEL}; wait 4000; \
             frames scroll 800 400 -90 60 chat_up; frames scroll 800 400 90 60 chat_down; \
             click 1000 55; wait 1500; frames scroll 1150 300 90 40 members_down; frames scroll 1150 300 -90 40 members_up; click 1000 55; wait 500; \
             go shop; wait 4000; frames scroll 800 400 90 40 shop_down; frames scroll 800 400 -90 40 shop_up; \
             click 36 109; wait 1500; "
        ));
    }
    if all || options.scenario == "soak" {
        for number in 1..=options.laps {
            steps.push_str(&lap(number));
        }
    }
    steps.push_str("perf sample end; perf caches end; perf heap");
    steps
}

/// The scripts of the `hitch` scenario, one process each so every first time is a first time: what the longest frames
/// were while (a) a channel full of pictures, embeds and stickers opened, (b) older messages were scrolled into view,
/// (c) two servers were switched between and (d) the emoji and the sticker picker opened. The ticker of `hitch start`
/// draws a frame every 16.7 ms and counts those over 16.7 and 33 ms.
fn hitch_scripts() -> Vec<(&'static str, String)> {
    let end = "perf sample end; perf caches end";
    vec![
        (
            "channel",
            format!(
                "wait 1500; click 36 157; wait 1500; hitch start; channel click {REAL_CHANNEL}; wait 4500; hitch stop channel_first; {end}"
            ),
        ),
        (
            "scroll",
            format!(
                "wait 1500; click 36 157; wait 1500; channel click {REAL_CHANNEL}; wait 3500; hitch start; {} hitch stop scroll_older; {end}",
                "scroll 800 400 400; wait 70; ".repeat(40)
            ),
        ),
        (
            "servers",
            format!(
                "wait 2500; hitch start; {} hitch stop server_switch; {end}",
                "click 36 157; wait 600; click 36 109; wait 600; ".repeat(5)
            ),
        ),
        (
            "pickers",
            format!(
                "wait 2500; hitch start; click 1202 683; wait 2500; key escape; wait 500; click 1162 683; wait 3500; key escape; wait 500; hitch stop pickers_first; hitch start; click 1202 683; wait 1500; key escape; wait 500; click 1162 683; wait 2000; key escape; wait 300; hitch stop pickers_second; {end}"
            ),
        ),
    ]
}

/// One sticker playing in the fixture's channel: the cost of a frame redrawn back to back and where it went.
fn anim_script() -> String {
    "perf start; wait 3000; perf sample anim0; perf ticks 12000 a_one_sticker; click 1202 683; wait 3500; perf ticks 12000 b_gif_trending; key escape; wait 1500; perf ticks 6000 c_idle; perf sample anim1; "
        .to_owned()
}

fn busy_compiling() -> bool {
    Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq rustc.exe", "/NH"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains("rustc.exe"))
        .unwrap_or(false)
}

/// Waits for the machine to stop compiling, so a timing run is not disturbed. Gives up after ten minutes and says so.
fn wait_quiet() -> bool {
    let started = Instant::now();
    while busy_compiling() {
        if started.elapsed() > Duration::from_secs(600) {
            println!("note: rustc is still running; the numbers of this run may be disturbed");
            return false;
        }
        sleep(Duration::from_secs(15));
    }
    true
}

fn build(repo: &Path, release: bool) -> Result<PathBuf> {
    let mut command = Command::new("cargo");
    command.args([
        "build",
        "-p",
        "gravecord",
        "--features",
        "capture",
        "--quiet",
    ]);
    // Six jobs unless `CARGO_BUILD_JOBS` asks for another number (a shared machine runs builds at four).
    let jobs = std::env::var("CARGO_BUILD_JOBS").unwrap_or_else(|_| "6".to_owned());
    command.args(["-j", &jobs]);
    if release {
        command.arg("--release");
    }
    let status = command.current_dir(repo).status()?;
    if !status.success() {
        bail!("gravecord did not build");
    }
    Ok(exe_of(repo, release))
}

fn exe_of(repo: &Path, release: bool) -> PathBuf {
    // Where cargo puts it: `CARGO_TARGET_DIR` when set, else the repository's `target`.
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| repo.join("target"), PathBuf::from)
        .join(if release { "release" } else { "debug" })
        .join("gravecord.exe")
}

fn kill(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output();
}

/// Runs the script once; the text of the log it wrote.
fn run_once(
    repo: &Path,
    exe: &Path,
    script: &str,
    out: &Path,
    timeout: Duration,
) -> Result<String> {
    let _ = fs::remove_file(out);
    let log = out.with_extension("log");
    let _ = fs::remove_file(&log);
    let args = vec![
        "--capture".to_owned(),
        out.display().to_string(),
        "100".to_owned(),
        "--script".to_owned(),
        script.to_owned(),
    ];
    let pid = headless::spawn(exe, &args, repo)?;
    let started = Instant::now();
    while !out.exists() {
        if started.elapsed() > timeout {
            kill(pid);
            bail!("no frame after {timeout:?}");
        }
        sleep(Duration::from_millis(500));
    }
    sleep(Duration::from_secs(2));
    kill(pid);
    fs::read_to_string(&log).context("reading the run's log")
}

/// The `key=value` fields of one log line.
fn fields(line: &str) -> BTreeMap<String, f64> {
    let mut map = BTreeMap::new();
    for word in line.split_whitespace() {
        if let Some((key, value)) = word.split_once('=')
            && let Ok(value) = value.trim_end_matches(',').parse::<f64>()
        {
            map.insert(key.to_owned(), value);
        }
    }
    map
}

/// Every number one run's log holds, by name.
fn metrics(log: &str) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    let mut samples: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    let mb = |bytes: f64| bytes / 1024. / 1024.;
    for line in log.lines() {
        if let Some(rest) = line.strip_prefix("perf timeline:") {
            // The start stages (`app/src/start_trace.rs`) as `cp_st_<stage>_ms`, the first moment each was reached.
            for (key, value) in fields(rest) {
                if let Some(name) = key.strip_prefix("cp_") {
                    out.insert(format!("start.{name}"), value);
                }
            }
        } else if let Some(rest) = line.strip_prefix("perf start:") {
            let values = fields(rest);
            for key in [
                "fonts_ms",
                "window_ms",
                "ready_ms",
                "content_ms",
                "splash_done_ms",
            ] {
                out.insert(
                    format!("start.{key}"),
                    values.get(key).copied().unwrap_or(0.),
                );
            }
            for (key, value) in &values {
                if let Some(name) = key.strip_prefix("cp_") {
                    // `cp_<name>_ms` is the moment the start reached that point; the others are bytes.
                    let value = if name.ends_with("_ms") {
                        *value
                    } else {
                        mb(*value)
                    };
                    out.insert(format!("start.{name}"), value);
                }
            }
        } else if let Some(rest) = line.strip_prefix("perf sample ")
            && let Some((label, values)) = rest.split_once(':')
        {
            samples.insert(label.trim().to_owned(), fields(values));
        } else if let Some(rest) = line.strip_prefix("perf caches ")
            && let Some((label, values)) = rest.split_once(':')
        {
            let label = label.trim();
            for word in values.split_whitespace() {
                let Some((name, value)) = word.split_once('=') else {
                    continue;
                };
                if name == "cache_bytes_total" {
                    out.insert(format!("{label}.cache_mb"), mb(value.parse().unwrap_or(0.)));
                } else if let Some((items, bytes)) = value.split_once('/') {
                    out.insert(format!("{label}.{name}.items"), items.parse().unwrap_or(0.));
                    out.insert(
                        format!("{label}.{name}.mb"),
                        mb(bytes.parse().unwrap_or(0.)),
                    );
                }
            }
        } else if let Some((kind, rest)) = line
            .strip_prefix("frames scroll ")
            .map(|rest| ("scroll", rest))
            .or_else(|| {
                line.strip_prefix("frames watch ")
                    .map(|rest| ("watch", rest))
            })
            && let Some((label, values)) = rest.split_once(':')
        {
            let values = fields(values);
            for key in ["median_ms", "p95_ms", "worst_ms"] {
                if let Some(value) = values.get(key) {
                    out.insert(format!("{kind}.{}.{key}", label.trim()), *value);
                }
            }
        } else if let Some(rest) = line.strip_prefix("hitch ")
            && let Some((label, values)) = rest.split_once(':')
        {
            let values = fields(values);
            for (key, name) in [
                ("frames", "frames"),
                ("over_16.7ms", "over_16_7ms"),
                ("over_33ms", "over_33ms"),
                ("longest_ms", "longest_ms"),
            ] {
                if let Some(value) = values.get(key) {
                    out.insert(format!("hitch.{}.{name}", label.trim()), *value);
                }
            }
        } else if let Some(rest) = line.strip_prefix("frames animate ")
            && let Some((label, values)) = rest.split_once(':')
        {
            let values = fields(values);
            for key in [
                "frame_mean_ms",
                "build_mean_ms",
                "rows_per_frame",
                "gpui_request_layout_ms",
                "gpui_prepaint_ms",
                "gpui_paint_ms",
                "worst_ms",
            ] {
                if let Some(value) = values.get(key) {
                    out.insert(format!("animate.{}.{key}", label.trim()), *value);
                }
            }
        } else if let Some(rest) = line.strip_prefix("perf badge refresh:") {
            for key in ["ms_each", "guilds", "channels"] {
                if let Some(value) = fields(rest).get(key) {
                    out.insert(format!("badge.{key}"), *value);
                }
            }
        } else if let Some(rest) = line.strip_prefix("frames click ")
            && let Some((label, values)) = rest.split_once(':')
        {
            if let Some(value) = fields(values).get("ms") {
                out.insert(format!("click.{}.ms", label.trim()), *value);
            }
        } else if let Some(rest) = line.strip_prefix("frames type:") {
            let values = fields(rest);
            for key in ["median_ms", "p95_ms", "worst_ms"] {
                if let Some(value) = values.get(key) {
                    out.insert(format!("type.{key}"), *value);
                }
            }
        }
    }
    for (label, values) in &samples {
        if let Some(value) = values.get("working_set") {
            out.insert(format!("{label}.rss_mb"), mb(*value));
        }
        if let Some(value) = values.get("private") {
            out.insert(format!("{label}.private_mb"), mb(*value));
        }
        if let Some(value) = values.get("threads") {
            out.insert(format!("{label}.threads"), *value);
        }
        for (field, name) in [
            ("heap", "heap_mb"),
            ("heap_peak", "heap_peak_mb"),
            ("commit_image", "commit_image_mb"),
            ("commit_mapped", "commit_mapped_mb"),
            ("commit_private", "commit_private_mb"),
        ] {
            if let Some(value) = values.get(field) {
                out.insert(format!("{label}.{name}"), mb(*value));
            }
        }
    }
    // CPU over each idle window, as a share of one core.
    for (from, to) in [("idle0", "idle1"), ("idle1", "idle2"), ("home0", "home1")] {
        if let (Some(a), Some(b)) = (samples.get(from), samples.get(to)) {
            let cpu = b["cpu_ms"] - a["cpu_ms"];
            let wall = (b["wall_ms"] - a["wall_ms"]).max(1.);
            out.insert(format!("{to}.cpu_pct"), 100. * cpu / wall);
            out.insert(
                format!("{to}.renders_per_s"),
                (b["renders"] - a["renders"]) / (wall / 1000.),
            );
        }
    }
    out
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        (sorted[middle - 1] + sorted[middle]) / 2.
    }
}

/// The budget of a metric: its limit and its target, if it has one.
fn budget(name: &str) -> Option<(f64, Option<f64>)> {
    match name {
        // The loading screen no longer shows (it starts done), so `splash_done_ms` stays 0. What the user waits for
        // is the open channel's live page with its pictures, and its first messages (from the cache when it is on).
        "start.splash_done_ms"
        | "start.st_frame_messages_ms"
        | "start.st_frame_live_messages_ms"
        | "start.st_frame_pictures_ms" => Some((BUDGET_START_MS, Some(TARGET_START_MS))),
        "idle1.rss_mb" | "idle2.rss_mb" | "home1.rss_mb" => {
            Some((BUDGET_IDLE_RSS_MB, Some(TARGET_IDLE_RSS_MB)))
        }
        "idle1.cpu_pct" | "idle2.cpu_pct" | "home1.cpu_pct" => Some((BUDGET_IDLE_CPU_PCT, None)),
        "type.median_ms" | "type.p95_ms" => Some((BUDGET_INPUT_MS, None)),
        n if (n.starts_with("scroll.") || n.starts_with("watch.")) && n.ends_with(".p95_ms") => {
            Some((BUDGET_FRAME_MS, None))
        }
        _ => None,
    }
}

pub fn run(repo: &Path, args: &[&str]) -> Result<()> {
    let options = parse(args)?;
    let exe = if let Some(exe) = options.exe.clone() {
        exe
    } else if options.build {
        build(repo, options.release)?
    } else {
        exe_of(repo, options.release)
    };
    let dir = repo.join("design").join("perf");
    fs::create_dir_all(&dir)?;
    let scripts: Vec<(String, String)> = match options.scenario.as_str() {
        "hitch" => hitch_scripts()
            .into_iter()
            .map(|(name, script)| (name.to_owned(), script))
            .collect(),
        "anim" => vec![("anim".to_owned(), anim_script())],
        _ => vec![("run".to_owned(), script(&options))],
    };
    let profile = if options.release { "release" } else { "debug" };
    let mut runs: Vec<BTreeMap<String, f64>> = Vec::new();
    let mut disturbed = false;
    for number in 1..=options.runs {
        if options.wait {
            disturbed |= !wait_quiet();
        }
        let out = dir.join(format!("{}-run{number}.png", options.label));
        println!(
            "run {number}/{} ({profile}, scenario {})",
            options.runs, options.scenario
        );
        let mut merged = BTreeMap::new();
        for (name, script) in &scripts {
            let log = run_once(repo, &exe, script, &out, options.timeout)?;
            let file = if scripts.len() == 1 {
                format!("{}-run{number}.txt", options.label)
            } else {
                format!("{}-run{number}-{name}.txt", options.label)
            };
            fs::write(dir.join(file), &log)?;
            let _ = fs::remove_file(&out);
            let _ = fs::remove_file(out.with_extension("layout.json"));
            let _ = fs::remove_file(out.with_extension("log"));
            merged.extend(metrics(&log));
        }
        runs.push(merged);
    }
    let mut names: Vec<&String> = runs.iter().flat_map(BTreeMap::keys).collect();
    names.sort();
    names.dedup();
    let mut report = format!(
        "# perf {} ({profile} capture build, {} runs, scenario {}, laps {})\n\n",
        options.label, options.runs, options.scenario, options.laps
    );
    if disturbed {
        report.push_str(
            "Some run started while rustc was running: those numbers may be disturbed.\n\n",
        );
    }
    report
        .push_str("| metric | median | min | max | budget | result |\n|---|---|---|---|---|---|\n");
    for name in names {
        let values: Vec<f64> = runs
            .iter()
            .filter_map(|run| run.get(name))
            .copied()
            .collect();
        if values.is_empty() {
            continue;
        }
        let middle = median(&values);
        let low = values.iter().copied().fold(f64::INFINITY, f64::min);
        let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let (limit, result) = match budget(name) {
            Some((limit, target)) => (
                match target {
                    Some(target) => format!("<= {limit} (target {target})"),
                    None => format!("<= {limit}"),
                },
                if middle <= limit { "pass" } else { "FAIL" },
            ),
            None => (String::new(), ""),
        };
        report.push_str(&format!(
            "| {name} | {middle:.2} | {low:.2} | {high:.2} | {limit} | {result} |\n"
        ));
    }
    fs::write(dir.join(format!("{}.md", options.label)), &report)?;
    println!("{report}");
    println!(
        "raw logs: {}",
        dir.join(format!("{}-run*.txt", options.label)).display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_read_numbers_and_skip_words() {
        let map = fields(" wall_ms=12 cpu_ms=3 label=x n=4,");
        assert_eq!(map["wall_ms"], 12.);
        assert_eq!(map["n"], 4.);
        assert!(!map.contains_key("label"));
    }

    #[test]
    fn idle_cpu_is_the_share_of_one_core() {
        let log = "perf sample idle0: wall_ms=1000 cpu_ms=100 working_set=1048576 private=0 threads=3 renders=10\n\
                   perf sample idle1: wall_ms=11000 cpu_ms=600 working_set=2097152 private=0 threads=3 renders=20\n";
        let out = metrics(log);
        assert!((out["idle1.cpu_pct"] - 5.).abs() < 1e-9);
        assert!((out["idle1.renders_per_s"] - 1.).abs() < 1e-9);
        assert!((out["idle1.rss_mb"] - 2.).abs() < 1e-9);
    }

    #[test]
    fn median_of_even_and_odd() {
        assert_eq!(median(&[3., 1., 2.]), 2.);
        assert_eq!(median(&[4., 1., 2., 3.]), 2.5);
    }

    #[test]
    fn every_scenario_ends_with_the_final_sample() {
        let options = parse(&["--scenario", "typing"]).unwrap();
        assert!(script(&options).ends_with("perf sample end; perf caches end; perf heap"));
        assert!(parse(&["--scenario", "nope"]).is_err());
    }
}
