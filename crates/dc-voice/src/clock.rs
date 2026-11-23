//! Media pacing and the isolated media runtime.
//!
//! `isolated` is adapted from Serein `crates/discord-voice/src/timer.rs`, MIT OR Apache-2.0.
//! Serein raises the Windows timer resolution with `timeBeginPeriod` (unsafe FFI) because
//! tokio timers there round up to the 15.6 ms system tick. This crate forbids unsafe code,
//! so the 20 ms cadence comes from a thread that sleeps with `std::thread::sleep` instead,
//! which uses a high-resolution waitable timer on current Windows.
use std::future::Future;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub(crate) const TICK: Duration = Duration::from_millis(20);

/// One message per 20 ms, deadline based so it does not drift. A late wake skips ticks
/// rather than bursting, like Serein's `MissedTickBehavior::Skip`. The thread ends once the
/// receiver is dropped.
pub(crate) fn ticks() -> Result<mpsc::Receiver<()>, &'static str> {
    let (send, receive) = mpsc::channel(1);
    std::thread::Builder::new()
        .name("dc-voice-clock".into())
        .spawn(move || {
            // Windows slows a throttled thread while the app is in the background (a game in front after Alt-Tab).
            let _unthrottled = dc_media::throttle::Guard::acquire();
            let mut next = Instant::now() + TICK;
            loop {
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                }
                if let Err(mpsc::error::TrySendError::Closed(())) = send.try_send(()) {
                    return;
                }
                next += TICK;
                let now = Instant::now();
                if now >= next {
                    next = now + TICK;
                }
            }
        })
        .map_err(|_| "Media clock thread could not start")?;
    Ok(receive)
}

struct PacerShared {
    /// The next deadline, and whether the owner is gone.
    state: std::sync::Mutex<(Option<Instant>, bool)>,
    changed: std::sync::Condvar,
}

/// Wakes a media loop at its pacer's deadlines. Idle threads block on a condition variable. Active deadlines use
/// the high-resolution sleep, in at most 1 ms pieces so an earlier deadline or shutdown is noticed promptly.
/// A timed condition-variable wait can overshoot by a Windows timer tick, expiring the 10 ms probe allowance.
pub(crate) struct PacerClock {
    shared: std::sync::Arc<PacerShared>,
}

impl PacerClock {
    /// The clock, and the wakes it sends: one per deadline reached. The thread ends with the clock.
    pub(crate) fn start() -> Result<(Self, mpsc::Receiver<()>), &'static str> {
        let shared = std::sync::Arc::new(PacerShared {
            state: std::sync::Mutex::new((None, false)),
            changed: std::sync::Condvar::new(),
        });
        let (send, receive) = mpsc::channel(1);
        let thread = shared.clone();
        std::thread::Builder::new()
            .name("dc-voice-pacer".into())
            .spawn(move || {
                let _unthrottled = dc_media::throttle::Guard::acquire();
                let mut state = thread.state.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if state.1 {
                        return;
                    }
                    let Some(deadline) = state.0 else {
                        state = thread
                            .changed
                            .wait(state)
                            .unwrap_or_else(|e| e.into_inner());
                        continue;
                    };
                    let now = Instant::now();
                    if deadline > now {
                        let delay = (deadline - now).min(Duration::from_millis(1));
                        drop(state);
                        std::thread::sleep(delay);
                        state = thread.state.lock().unwrap_or_else(|e| e.into_inner());
                        continue;
                    }
                    state.0 = None;
                    drop(state);
                    if let Err(mpsc::error::TrySendError::Closed(())) = send.try_send(()) {
                        return;
                    }
                    state = thread.state.lock().unwrap_or_else(|e| e.into_inner());
                }
            })
            .map_err(|_| "Pacer clock thread could not start")?;
        Ok((Self { shared }, receive))
    }

    /// The pacer's next deadline; `None` when it has nothing to send.
    pub(crate) fn set(&self, deadline: Option<Instant>) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.0 != deadline {
            state.0 = deadline;
            self.shared.changed.notify_one();
        }
    }
}

impl Drop for PacerClock {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.1 = true;
        self.shared.changed.notify_one();
    }
}

/// Counts 20 ms frames owed since a start, for a loop that must move one frame per 20 ms of real time even when its
/// tick came late and the clock skipped one (`ticks` skips rather than bursts).
#[derive(Default)]
pub(crate) struct FrameClock {
    start: Option<Instant>,
    taken: u64,
}

impl FrameClock {
    /// Most frames handed out at once; a longer stall starts the count again rather than burst.
    const MAX_CATCH_UP: u64 = 3;

    /// Frames due at `now`: one per tick on time, more after a skipped tick.
    pub(crate) fn due(&mut self, now: Instant) -> u64 {
        let start = *self.start.get_or_insert(now);
        self.owed(now.saturating_duration_since(start).as_micros() as u64)
    }

    /// [`FrameClock::due`] `elapsed` microseconds after the first call. A tick is counted from half way to it, so a
    /// wake a little early or late on the grid neither owes nothing nor owes two.
    pub(crate) fn owed(&mut self, elapsed: u64) -> u64 {
        let tick = TICK.as_micros() as u64;
        let expected = (elapsed + tick / 2) / tick + 1;
        let mut owed = expected.saturating_sub(self.taken);
        if owed > Self::MAX_CATCH_UP {
            self.taken = expected - 1;
            owed = 1;
        }
        self.taken += owed;
        owed
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Run one media loop on its own thread and single-threaded runtime, so gateway, image and
/// UI tasks on the shared runtime cannot delay audio ticks. Dropping the returned future
/// stops the loop at its next await.
pub(crate) async fn isolated<F, Fut, T>(name: &'static str, start: F) -> Result<T, &'static str>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, &'static str>>,
    T: Send + 'static,
{
    let (mut done, result) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let _unthrottled = dc_media::throttle::Guard::acquire();
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                let _ = done.send(Err("Media runtime could not start"));
                return;
            };
            let outcome = runtime.block_on(async {
                let work = start();
                tokio::pin!(work);
                tokio::select! {
                    outcome = &mut work => Some(outcome),
                    () = done.closed() => None,
                }
            });
            if let Some(outcome) = outcome {
                let _ = done.send(outcome);
            }
        })
        .map_err(|_| "Media thread could not start")?;
    result
        .await
        .unwrap_or(Err("Media thread stopped unexpectedly"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pacer_rearms_cancels_and_stops_without_spurious_idle_wakes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (clock, mut wakes) = PacerClock::start().unwrap();
            clock.set(Some(Instant::now() + Duration::from_secs(30)));
            clock.set(Some(Instant::now() + Duration::from_millis(2)));
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), wakes.recv())
                    .await
                    .unwrap(),
                Some(())
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(50), wakes.recv())
                    .await
                    .is_err()
            );
            clock.set(Some(Instant::now() + Duration::from_millis(100)));
            clock.set(None);
            assert!(
                tokio::time::timeout(Duration::from_millis(150), wakes.recv())
                    .await
                    .is_err()
            );
            clock.set(Some(Instant::now() + Duration::from_secs(30)));
            drop(clock);
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), wakes.recv())
                    .await
                    .unwrap(),
                None
            );
        });
    }

    #[test]
    #[ignore = "measures native pacer deadline latency without audio or capture devices"]
    fn measure_pacer_deadline_latency() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (clock, mut wakes) = PacerClock::start().unwrap();
        let mut late = runtime.block_on(async {
            let mut late = Vec::new();
            for index in 0..100 {
                let due = Instant::now() + Duration::from_millis(1 + index % 5);
                clock.set(Some(due));
                tokio::time::timeout(Duration::from_secs(2), wakes.recv())
                    .await
                    .expect("pacer wake")
                    .expect("pacer alive");
                late.push(Instant::now().saturating_duration_since(due).as_secs_f64() * 1000.);
            }
            late
        });
        late.sort_by(f64::total_cmp);
        println!(
            "pacer deadline lateness: median {:.3} ms, p95 {:.3} ms, max {:.3} ms; {} / 100 exceed the 10 ms probe deadline",
            late[50],
            late[95],
            late[99],
            late.iter().filter(|&&ms| ms > 10.).count()
        );
    }

    /// The send side's cadence: one tick per 20 ms on the high-resolution thread while other threads keep cores busy,
    /// never two at once (a late wake skips, it does not burst), so one audio packet leaves per tick.
    #[test]
    fn ticks_stay_on_a_steady_20_ms_cadence_under_load() {
        // The clock's own load is the two busy threads below. When the machine is also loaded from outside (parallel
        // builds), one second of measurement can land in a stall that no clock could hide; a clock that drifts or
        // bunches fails every attempt, so up to three are taken.
        let mut failures = Vec::new();
        for _ in 0..3 {
            match measure_cadence() {
                Ok(()) => return,
                Err(failure) => failures.push(failure),
            }
        }
        panic!("off the 20 ms grid in every attempt: {failures:?}");
    }

    fn measure_cadence() -> Result<(), String> {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let busy: Vec<_> = (0..2)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    let mut x = 0u64;
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                    }
                    x
                })
            })
            .collect();
        let mut ticks = ticks().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let times: Vec<Instant> = runtime.block_on(async {
            let mut times = Vec::new();
            for _ in 0..60 {
                ticks.recv().await.unwrap();
                times.push(Instant::now());
            }
            times
        });
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for thread in busy {
            let _ = thread.join();
        }
        let gaps: Vec<f64> = times
            .windows(2)
            .map(|w| (w[1] - w[0]).as_secs_f64() * 1000.0)
            .collect();
        // A wake late by more than a tick skips (the deadline moves on), so a loaded machine may show 40 ms, and one
        // late by less is followed by a shorter gap back onto the grid; most gaps are one tick, and two ticks never
        // come together.
        let mut sorted = gaps.clone();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        if !(18.0..=22.0).contains(&median) {
            return Err(format!("median {median} ms: {gaps:?}"));
        }
        if !gaps.iter().all(|gap| *gap > 1.0) {
            return Err(format!("ticks bunched: {gaps:?}"));
        }
        Ok(())
    }
}
