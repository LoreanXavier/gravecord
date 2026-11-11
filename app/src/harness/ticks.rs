//! `perf ticks <ms> <label>`: what the window really does for `ms` while the script waits (nothing is forced), per
//! second and per drawn frame: draws, renders of the main view and of each region, the draw phases, the platform
//! frame closure, next-frame callbacks, sprite atlas uploads, foreground tasks, and the UI thread's CPU. Capture only.

use std::time::{Duration, Instant};

/// Everything the line reports, read at one moment.
#[derive(Clone, Copy, Default)]
struct Totals {
    cpu: u64,
    ui_cpu: u64,
    renders: u64,
    build_ms: f64,
    rows: u64,
    rows_ms: f64,
    phases: (u64, u64, u64, u64),
    frames: (u64, u64, u64),
    tick: [u64; 8],
    regions: [(u64, u64); crate::ui::regions::KINDS],
}

fn read() -> Totals {
    let (renders, build_ms, rows, rows_ms) = crate::perf::build_totals();
    Totals {
        cpu: crate::perf::cpu_ms(),
        ui_cpu: crate::perf::thread_cpu_ms(),
        renders,
        build_ms,
        rows,
        rows_ms,
        phases: gpui::draw_phase_totals(),
        frames: gpui::frame_totals(),
        tick: gpui::tick_totals(),
        regions: crate::ui::regions::counters(),
    }
}

pub(super) async fn run(ms: u64, label: &str, cx: &mut gpui::AsyncApp) -> String {
    let before = read();
    let begun = Instant::now();
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
    let after = read();
    let wall = begun.elapsed().as_secs_f64();
    let draws = (after.phases.3 - before.phases.3).max(1) as f64;
    let per_s = |now: u64, then: u64| (now - then) as f64 / wall;
    let ms_per_draw = |now: u64, then: u64| (now - then) as f64 / 1e6 / draws;
    let ms_per_s = |now: u64, then: u64| (now - then) as f64 / 1e6 / wall;
    let mut line = format!(
        "perf ticks {label}: wall_s={wall:.1} cpu_pct={:.1} ui_cpu_pct={:.1} draws_per_s={:.1} renders_per_s={:.1} \
         root_build_ms_per_render={:.3} rows_per_s={:.1} rows_ms_per_row={:.3} \
         layout_ms_per_draw={:.3} prepaint_ms_per_draw={:.3} paint_ms_per_draw={:.3} \
         present_ms_per_s={:.2} frame_closure_calls_per_s={:.1} frame_closure_ms_per_s={:.2} \
         callbacks_per_s={:.1} callbacks_ms_per_s={:.2} atlas_calls_per_s={:.1} atlas_ms_per_s={:.2} \
         tasks_per_s={:.1} tasks_ms_per_s={:.2} draw_ms_per_s={:.2}",
        (after.cpu - before.cpu) as f64 * 100. / (wall * 1000.),
        (after.ui_cpu - before.ui_cpu) as f64 * 100. / (wall * 1000.),
        draws / wall,
        (after.renders - before.renders) as f64 / wall,
        (after.build_ms - before.build_ms) / (after.renders - before.renders).max(1) as f64,
        (after.rows - before.rows) as f64 / wall,
        (after.rows_ms - before.rows_ms) / (after.rows - before.rows).max(1) as f64,
        ms_per_draw(after.phases.0, before.phases.0),
        ms_per_draw(after.phases.1, before.phases.1),
        ms_per_draw(after.phases.2, before.phases.2),
        ms_per_s(after.frames.1, before.frames.1),
        per_s(after.tick[1], before.tick[1]),
        ms_per_s(after.tick[0], before.tick[0]),
        per_s(after.tick[3], before.tick[3]),
        ms_per_s(after.tick[2], before.tick[2]),
        per_s(after.tick[5], before.tick[5]),
        ms_per_s(after.tick[4], before.tick[4]),
        per_s(after.tick[7], before.tick[7]),
        ms_per_s(after.tick[6], before.tick[6]),
        ms_per_s(after.frames.0, before.frames.0),
    );
    for (index, name) in crate::ui::regions::NAMES.iter().enumerate() {
        let (count, ns) = after.regions[index];
        let (count0, ns0) = before.regions[index];
        line.push_str(&format!(
            " region_{name}_per_s={:.1} region_{name}_ms_per_render={:.3}",
            (count - count0) as f64 / wall,
            (ns - ns0) as f64 / 1e6 / (count - count0).max(1) as f64
        ));
    }
    line
}
