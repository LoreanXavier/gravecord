//! `hitch start` / `hitch stop <label>`: a frame ticker that draws the window every 16.7 ms while the script does other
//! things (opens a channel, scrolls, switches server, opens a picker), and what the worst frames were. A frame costs the
//! time its draw took plus how late the ticker woke, which is the time the UI thread was busy with something else
//! (an image arriving, a fetched page being applied). Capture only.

use crate::ui::Root;
use gpui::{AnyWindowHandle, AsyncApp, Entity};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const FRAME: Duration = Duration::from_micros(16_700);

/// One frame of the ticker.
#[derive(Clone, Copy, Debug)]
pub(super) struct Sample {
    pub at_ms: f64,
    /// The draw, and the time the ticker woke after it was due.
    pub draw_ms: f64,
    pub late_ms: f64,
    /// Of the draw: building the main view (`Root::render`) and its message rows.
    pub build_ms: f64,
    pub rows_ms: f64,
}

impl Sample {
    pub(super) fn cost(&self) -> f64 {
        self.draw_ms + self.late_ms
    }
}

static SAMPLES: Mutex<Vec<Sample>> = Mutex::new(Vec::new());
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Counts over the two frame limits, the longest frame, and the five worst with what they spent.
pub(super) fn report(samples: &[Sample]) -> String {
    let over = |limit: f64| {
        samples
            .iter()
            .filter(|sample| sample.cost() > limit)
            .count()
    };
    let mut worst: Vec<&Sample> = samples.iter().collect();
    worst.sort_by(|a, b| b.cost().total_cmp(&a.cost()));
    let mut line = format!(
        "frames={} over_16.7ms={} over_33ms={} longest_ms={:.1}",
        samples.len(),
        over(16.7),
        over(33.),
        worst.first().map_or(0., |sample| sample.cost()),
    );
    line.push_str(" worst=[");
    for sample in worst.iter().take(5) {
        line.push_str(&format!(
            "t={:.0}ms cost={:.1} (draw {:.1} [build {:.1} rows {:.1}] late {:.1}); ",
            sample.at_ms,
            sample.cost(),
            sample.draw_ms,
            sample.build_ms,
            sample.rows_ms,
            sample.late_ms
        ));
    }
    line.push(']');
    line
}

pub(super) fn start(window: AnyWindowHandle, root: Entity<Root>, cx: &mut AsyncApp) -> String {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return "hitch: already running".to_owned();
    }
    SAMPLES.lock().expect("samples").clear();
    cx.spawn(async move |cx| {
        let begun = Instant::now();
        let mut tick: u32 = 1;
        while RUNNING.load(Ordering::SeqCst) {
            let due = begun + FRAME * tick;
            let now = Instant::now();
            if due > now {
                cx.background_executor().timer(due - now).await;
            }
            let woke = Instant::now();
            let late_ms = woke.saturating_duration_since(due).as_secs_f64() * 1000.;
            let (_, build0, _, rows0) = crate::perf::build_totals();
            cx.update(|cx| {
                root.update(cx, |_, cx| cx.notify());
                let _ = window.update(cx, |_, window, cx| window.draw(cx).clear(cx));
            });
            let drawn = Instant::now();
            let (_, build1, _, rows1) = crate::perf::build_totals();
            SAMPLES.lock().expect("samples").push(Sample {
                at_ms: woke.duration_since(begun).as_secs_f64() * 1000.,
                draw_ms: drawn.duration_since(woke).as_secs_f64() * 1000.,
                late_ms,
                build_ms: build1 - build0,
                rows_ms: rows1 - rows0,
            });
            // A frame that overran skips the ticks it covered, as a window does.
            let elapsed = Instant::now().duration_since(begun);
            tick = (tick + 1).max((elapsed.as_secs_f64() / FRAME.as_secs_f64()).ceil() as u32);
        }
    })
    .detach();
    "hitch: ticker running".to_owned()
}

pub(super) fn stop(label: &str) -> String {
    RUNNING.store(false, Ordering::SeqCst);
    let samples = SAMPLES.lock().expect("samples").clone();
    format!("hitch {label}: {}", report(&samples))
}
