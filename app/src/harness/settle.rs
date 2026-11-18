//! `settle <max ms>`: waits until the window stops changing. The window is drawn every 250ms; once four drawings in a
//! row (one second) are the same picture and no image or fetch request is on its way (`net::in_flight`), the step
//! ends, at the latest after `max ms`. Scripts whose frame depends on
//! loads that finish at their own pace (pictures, GIF results, call tiles) use it in place of a guessed `wait`, so a
//! loaded machine only makes the step longer, not the frame different.

use gpui::{AnyWindowHandle, AsyncApp};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as _, Hasher as _};
use std::time::{Duration, Instant};

/// Drawings between checks.
const STEP: Duration = Duration::from_millis(250);
/// Equal drawings in a row that count as settled.
const SAME: u32 = 4;

/// The picture's hash.
fn hash_of(image: &image::RgbaImage) -> u64 {
    let mut hasher = DefaultHasher::new();
    image.dimensions().hash(&mut hasher);
    image.as_raw().hash(&mut hasher);
    hasher.finish()
}

/// Counts equal hashes in a row; true once `SAME` have been seen.
#[derive(Default)]
struct Run {
    last: Option<u64>,
    same: u32,
}

impl Run {
    fn push(&mut self, hash: u64) -> bool {
        if self.last == Some(hash) {
            self.same += 1;
        } else {
            self.last = Some(hash);
            self.same = 1;
        }
        self.same >= SAME
    }
}

pub(super) async fn run(rest: &[&str], window: AnyWindowHandle, cx: &mut AsyncApp) -> String {
    let Some(max) = rest.first().and_then(|ms| ms.parse::<u64>().ok()) else {
        return "settle: usage: settle <max ms>".to_owned();
    };
    let started = Instant::now();
    let mut run = Run::default();
    while started.elapsed() < Duration::from_millis(max) {
        let image = cx.update(|cx| {
            window
                .update(cx, |_, window, cx| {
                    window.draw(cx).clear(cx);
                    window.render_to_image().ok()
                })
                .ok()
                .flatten()
        });
        // A picture or answer still on its way would change the window after the run; it does not count as still.
        let waiting = crate::net::in_flight();
        if let Some(image) = image
            && run.push(hash_of(&image) ^ waiting as u64)
            && waiting == 0
        {
            return format!("settle: still after {} ms", started.elapsed().as_millis());
        }
        cx.background_executor().timer(STEP).await;
    }
    format!(
        "settle: still changing after {max} ms ({} requests on their way)",
        crate::net::in_flight()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_equal_drawings_in_a_row_settle() {
        let mut run = Run::default();
        assert!(!run.push(1));
        assert!(!run.push(1));
        assert!(!run.push(2));
        assert!(!run.push(2));
        assert!(!run.push(2));
        assert!(run.push(2));
    }
}
