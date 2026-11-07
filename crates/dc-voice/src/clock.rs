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
