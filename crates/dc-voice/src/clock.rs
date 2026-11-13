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

/// The longest a waiting pacer thread sleeps before it looks at its deadline again, so a deadline moved earlier is met
/// within this much (`PacingController::kMinSleepTime`, 1 ms).
const PACER_POLL: Duration = Duration::from_millis(1);

struct PacerShared {
    /// The next deadline, and whether the owner is gone.
    state: std::sync::Mutex<(Option<Instant>, bool)>,
    changed: std::sync::Condvar,
}

/// Wakes a media loop at its pacer's deadlines with the high-resolution sleep of [`ticks`], not tokio's timer, which
/// rounds up to the 15.6 ms system tick on Windows: libwebrtc's pacer posts its process task as a high-precision delayed
/// task for the same reason (`PostDelayedHighPrecisionTask` in `task_queue_paced_sender.cc`). The thread waits on a
/// condition variable while there is no deadline and sleeps in steps of at most `PACER_POLL` while there is one.
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
                loop {
                    let deadline = {
                        let mut state = thread.state.lock().unwrap_or_else(|e| e.into_inner());
                        while state.0.is_none() && !state.1 {
                            state = thread
                                .changed
                                .wait(state)
                                .unwrap_or_else(|e| e.into_inner());
                        }
                        if state.1 {
                            return;
                        }
                        state.0
                    };
                    let Some(deadline) = deadline else { continue };
                    let now = Instant::now();
                    if deadline > now {
                        std::thread::sleep((deadline - now).min(PACER_POLL));
                        continue;
                    }
                    {
                        let mut state = thread.state.lock().unwrap_or_else(|e| e.into_inner());
                        if state.0 == Some(deadline) {
                            state.0 = None;
                        }
                    }
                    if let Err(mpsc::error::TrySendError::Closed(())) = send.try_send(()) {
                        return;
                    }
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

    /// The send side's cadence: one tick per 20 ms on the high-resolution thread while other threads keep cores busy,
    /// never two at once (a late wake skips, it does not burst), so one audio packet leaves per tick.
    #[test]
    fn ticks_stay_on_a_steady_20_ms_cadence_under_load() {
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
        assert!(
            (18.0..=22.0).contains(&median),
            "median {median} ms: {gaps:?}"
        );
        assert!(gaps.iter().all(|gap| *gap > 1.0), "ticks bunched: {gaps:?}");
    }
}
