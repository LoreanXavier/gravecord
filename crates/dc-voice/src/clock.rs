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
