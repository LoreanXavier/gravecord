//! When to show each received picture: at its RTP timestamp plus a delay that covers how late pictures have been
//! arriving, so a stream that comes in bursts (a picture held back behind a retransmitted packet, then released with
//! the ones queued behind it) is still shown one picture per frame interval.
//!
//! The receiver used to hand every picture over the moment it was complete. Behind a single lost packet a 60 fps
//! stream then held still for the repair's round trip and caught up in one jump: at 1 % loss 1,497 of 3,570 pictures
//! came within 4 ms of the one before and were never seen (`lossy::loss_table`). The official client's engine keeps a
//! playout delay for the same reason (libwebrtc's frame timing); this is a small version of it: the delay is the
//! largest lateness of the last two seconds, it rises at once and falls slowly, and it never exceeds [`MAX_DELAY`].
//! A picture later than that is a stall (a keyframe after a freeze), not jitter, and teaches nothing. The first
//! picture and a stream without bursts play with (almost) no added delay.
use std::{collections::VecDeque, time::Duration};
use tokio::time::Instant;

/// The most a picture is held back (the receive window's longest repair wait is 400 ms; the common case is far
/// shorter).
pub(crate) const MAX_DELAY: Duration = Duration::from_millis(300);
/// Pictures whose arrival sets the earliest-arrival line (about four seconds at 60 fps).
const BASELINE: usize = 256;
/// Pictures whose lateness sets the delay (about two seconds at 60 fps).
const LATENESS: usize = 120;
/// How much the delay may fall per picture once the bursts stop (60 ms a second at 60 fps).
const DECAY_US: i64 = 1_000;

#[derive(Default)]
pub(crate) struct Playout {
    origin: Option<Instant>,
    /// The last RTP timestamp and its unwrapped value in 90 kHz ticks.
    last: Option<(u32, i64)>,
    /// Arrival minus media time, in microseconds, of recent pictures.
    offsets: VecDeque<i64>,
    lateness: VecDeque<i64>,
    delay_us: i64,
}

impl Playout {
    /// When the picture with RTP `timestamp` that arrived at `now` should be shown; never before `now` and never
    /// later than [`MAX_DELAY`] after it.
    pub fn due(&mut self, timestamp: u32, now: Instant) -> Instant {
        let origin = *self.origin.get_or_insert(now);
        let ticks = match self.last {
            None => 0,
            Some((last, ticks)) => ticks + i64::from(timestamp.wrapping_sub(last) as i32),
        };
        self.last = Some((timestamp, ticks));
        let media_us = ticks * 1000 / 90;
        let arrived_us = now.saturating_duration_since(origin).as_micros() as i64;
        let offset = arrived_us - media_us;
        if self.offsets.len() == BASELINE {
            self.offsets.pop_front();
        }
        self.offsets.push_back(offset);
        let earliest = self.offsets.iter().copied().min().unwrap_or(offset);
        let late = offset - earliest;
        if late <= MAX_DELAY.as_micros() as i64 {
            if self.lateness.len() == LATENESS {
                self.lateness.pop_front();
            }
            self.lateness.push_back(late);
        }
        let target = self
            .lateness
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
            .min(MAX_DELAY.as_micros() as i64);
        self.delay_us = if target >= self.delay_us {
            target
        } else {
            (self.delay_us - DECAY_US).max(target)
        };
        let due_us = media_us + earliest + self.delay_us;
        let due = origin + Duration::from_micros(due_us.max(0) as u64);
        due.clamp(now, now + MAX_DELAY)
    }

    /// The delay pictures are held now.
    #[cfg(test)]
    pub fn delay(&self) -> Duration {
        Duration::from_micros(self.delay_us as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_TICKS: u32 = 1500;

    #[test]
    fn a_steady_stream_plays_as_it_arrives() {
        let start = Instant::now();
        let mut playout = Playout::default();
        for frame in 0..300u32 {
            let now = start + Duration::from_micros(u64::from(frame) * 16_667);
            let due = playout.due(
                frame
                    .wrapping_mul(FRAME_TICKS)
                    .wrapping_add(u32::MAX - 3000),
                now,
            );
            assert!(due.saturating_duration_since(now) <= Duration::from_millis(1));
        }
        assert!(playout.delay() <= Duration::from_millis(1));
    }

    #[test]
    fn bursts_behind_a_repair_come_out_one_frame_interval_apart() {
        let start = Instant::now();
        let mut playout = Playout::default();
        let mut shown = Vec::new();
        for frame in 0..240u64 {
            let mut arrival = frame * 16_667;
            // Every 30th picture waits 90 ms for a repair and the four behind it are released with it.
            let held = frame % 30;
            if (10..15).contains(&held) {
                arrival = (frame - held + 10) * 16_667 + 90_000;
            }
            let now = start + Duration::from_micros(arrival);
            let due = playout.due((frame as u32) * FRAME_TICKS, now);
            assert!(due >= now);
            shown.push(due.duration_since(start).as_micros() as i64);
        }
        // After the first burst taught the delay, every interval is a frame interval.
        let intervals: Vec<i64> = shown[20..].windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            intervals.iter().all(|i| (i - 16_667).abs() < 1_000),
            "{intervals:?}"
        );
        assert!(playout.delay() >= Duration::from_millis(80));
        assert!(playout.delay() <= MAX_DELAY);
    }

    #[test]
    fn a_stall_teaches_nothing_and_the_delay_falls_once_bursts_stop() {
        let start = Instant::now();
        let mut playout = Playout::default();
        playout.due(0, start);
        // One picture two seconds late (a keyframe after a freeze) is shown at once and leaves no delay behind.
        let late = start + Duration::from_millis(2_016);
        assert_eq!(playout.due(FRAME_TICKS, late), late);
        assert!(playout.delay() <= Duration::from_millis(1));
        // A burst teaches a delay; a steady stream afterwards brings it back down within a few seconds.
        let mut playout = Playout::default();
        for frame in 0..600u64 {
            let arrival = if (10..15).contains(&frame) {
                10 * 16_667 + 90_000
            } else {
                frame * 16_667
            };
            playout.due(
                frame as u32 * FRAME_TICKS,
                start + Duration::from_micros(arrival),
            );
            if frame == 20 {
                assert!(playout.delay() >= Duration::from_millis(80));
            }
        }
        assert!(
            playout.delay() < Duration::from_millis(2),
            "{:?}",
            playout.delay()
        );
    }
}
