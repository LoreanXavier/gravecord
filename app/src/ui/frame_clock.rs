//! Which frame of an animated picture a text run draws now. Pictures inside text are painted by the text element
//! itself (not by an `img` element), so they pick their own frame from one shared clock.
use gpui::{RenderImage, Window};
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

/// The frame of `data` to draw, looping; asks for the next frame to be drawn when the picture has more than one.
/// Nothing plays in a window that is not focused (module 565645: the animated file is asked for only while the
/// window is focused).
pub(crate) fn frame_index(data: &RenderImage, window: &mut Window) -> usize {
    static START: OnceLock<Instant> = OnceLock::new();
    let count = data.frame_count();
    if count <= 1 || !window.is_window_active() {
        return 0;
    }
    let delays: Vec<Duration> = (0..count)
        .map(|index| super::gif_tiles::browser_delay(Duration::from(data.delay(index))))
        .collect();
    let total: Duration = delays.iter().sum();
    window.request_animation_frame();
    let at = frame_at(&delays, total, START.get_or_init(Instant::now).elapsed());
    at.min(count - 1)
}

fn frame_at(delays: &[Duration], total: Duration, elapsed: Duration) -> usize {
    if total.is_zero() {
        return 0;
    }
    let mut left = Duration::from_nanos((elapsed.as_nanos() % total.as_nanos()) as u64);
    for (index, delay) in delays.iter().enumerate() {
        if left < *delay {
            return index;
        }
        left -= *delay;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_loops_over_the_frames() {
        let delays = [Duration::from_millis(40), Duration::from_millis(60)];
        let total = Duration::from_millis(100);
        assert_eq!(frame_at(&delays, total, Duration::from_millis(0)), 0);
        assert_eq!(frame_at(&delays, total, Duration::from_millis(39)), 0);
        assert_eq!(frame_at(&delays, total, Duration::from_millis(40)), 1);
        assert_eq!(frame_at(&delays, total, Duration::from_millis(99)), 1);
        assert_eq!(frame_at(&delays, total, Duration::from_millis(100)), 0);
        assert_eq!(frame_at(&delays, total, Duration::from_millis(1_040)), 1);
    }
}
