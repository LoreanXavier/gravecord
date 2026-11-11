//! The decoded pictures of the app: a byte budget and a least-recently-painted order over what gpui keeps for every
//! `img()` of an encoded image (the decoded pixels in its asset cache and the sprite atlas texture on the GPU).
//!
//! The encoded images stay in `Root::images`, which is small (a few KiB each); what grows without bound is the decoded
//! side, four bytes a pixel and frame. gpui reports each picture an `Img` painted (`gpui::take_painted_images`); this
//! module remembers when each was last painted and, on a timer, drops the decoded copy of the ones that are not on
//! screen: over the budget the longest unpainted first, and anything unpainted for a while even under it. A picture
//! painted later is decoded again from its encoded bytes by the same `img()` that draws it.
//!
//! Nothing that is on screen is ever dropped: the pictures a window painted in its latest frames are protected until
//! that window paints again, so a window that stays still keeps its pictures.

use gpui::{App, PaintedImage, RenderImage};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Most decoded bytes kept for pictures that are not on screen. The idle budget of the whole process is 150 MB
///; a screen of avatars, emoji and attachments decodes to about 30 MB, so 32 MiB of recently
/// seen pictures beside it keeps the last views warm without letting the total drift past the budget.
pub(super) const BUDGET: usize = 32 * 1024 * 1024;
/// A picture not painted for this long is dropped even when the cache is under its budget, so a view that was closed
/// gives its memory back.
pub(super) const IDLE_AFTER: Duration = Duration::from_secs(45);
/// How often the cache looks at what was painted.
const PERIOD: Duration = Duration::from_secs(2);

/// Bytes of a decoded picture as the atlas holds it: 4 per pixel and frame.
pub(super) fn render_bytes(image: &RenderImage) -> usize {
    (0..image.frame_count())
        .map(|frame| {
            let size = image.size(frame);
            size.width.0.max(0) as usize * size.height.0.max(0) as usize * 4
        })
        .sum()
}

struct Entry<V> {
    value: V,
    bytes: usize,
    painted: Instant,
}

/// The bookkeeping, apart from gpui so it can be tested: pictures by id with their size and the moment they were last
/// painted, and per window the ids its latest frames painted.
pub(super) struct Tracker<V> {
    entries: HashMap<u64, Entry<V>>,
    on_screen: HashMap<u64, HashSet<u64>>,
    used: usize,
    budget: usize,
    idle_after: Duration,
    evicted: usize,
}

impl<V> Tracker<V> {
    pub(super) fn new(budget: usize, idle_after: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            on_screen: HashMap::new(),
            used: 0,
            budget,
            idle_after,
            evicted: 0,
        }
    }

    /// `window` painted these pictures (id, value, size) since the last call. They count as painted at `now` and are
    /// protected until that window paints again.
    pub(super) fn painted(
        &mut self,
        window: u64,
        pictures: impl IntoIterator<Item = (u64, V, usize)>,
        now: Instant,
    ) {
        let mut ids = HashSet::new();
        for (id, value, bytes) in pictures {
            ids.insert(id);
            match self.entries.get_mut(&id) {
                Some(entry) => {
                    self.used = self.used - entry.bytes + bytes;
                    *entry = Entry {
                        value,
                        bytes,
                        painted: now,
                    };
                }
                None => {
                    self.used += bytes;
                    self.entries.insert(
                        id,
                        Entry {
                            value,
                            bytes,
                            painted: now,
                        },
                    );
                }
            }
        }
        if !ids.is_empty() {
            self.on_screen.insert(window, ids);
        }
    }

    /// Removes and returns the pictures to drop at `now`: those unpainted for the idle time, then the longest
    /// unpainted until the rest fits the budget. A picture a window painted in its latest frames is never returned.
    pub(super) fn evict(&mut self, now: Instant) -> Vec<V> {
        let protected: HashSet<u64> = self.on_screen.values().flatten().copied().collect();
        let mut candidates: Vec<(u64, Instant)> = self
            .entries
            .iter()
            .filter(|(id, _)| !protected.contains(id))
            .map(|(id, entry)| (*id, entry.painted))
            .collect();
        candidates.sort_by_key(|(_, painted)| *painted);
        let mut dropped = Vec::new();
        for (id, painted) in candidates {
            let stale = now.saturating_duration_since(painted) >= self.idle_after;
            if !stale && self.used <= self.budget {
                // Sorted oldest first: everything after this one is newer and the budget holds.
                break;
            }
            if let Some(entry) = self.entries.remove(&id) {
                self.used -= entry.bytes;
                self.evicted += 1;
                dropped.push(entry.value);
            }
        }
        dropped
    }

    #[cfg(any(test, feature = "capture"))]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(any(test, feature = "capture"))]
    pub(super) fn used(&self) -> usize {
        self.used
    }

    #[cfg(any(test, feature = "capture"))]
    pub(super) fn evicted(&self) -> usize {
        self.evicted
    }
}

thread_local! {
    static CACHE: RefCell<Tracker<PaintedImage>> = RefCell::new(Tracker::new(BUDGET, IDLE_AFTER));
}

/// Reads what the windows painted since the last call and drops the decoded copies that are due: gpui's cached pixels
/// and the picture's atlas texture.
pub(super) fn sweep(cx: &mut App) {
    let now = Instant::now();
    let dropped = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        for (window, pictures) in gpui::take_painted_images() {
            cache.painted(
                window,
                pictures.into_iter().map(|(image, render)| {
                    let bytes = render_bytes(&render);
                    (image.id, (image, render), bytes)
                }),
                now,
            );
        }
        cache.evict(now)
    });
    for (image, render) in dropped {
        cx.drop_image(render, None);
        image.remove_asset(cx);
    }
}

/// Starts the timer that keeps the cache within its budget, for the life of the app.
pub(crate) fn start(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(PERIOD).await;
            cx.update(sweep);
        }
    })
    .detach();
}

/// The pictures kept and their decoded bytes, for the perf harness.
#[cfg(feature = "capture")]
pub(super) fn perf_size() -> (crate::perf::Size, usize) {
    CACHE.with(|cache| {
        let cache = cache.borrow();
        (
            crate::perf::Size {
                items: cache.len(),
                bytes: cache.used(),
            },
            cache.evicted(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> (Tracker<u64>, Instant) {
        (Tracker::new(100, Duration::from_secs(60)), Instant::now())
    }

    fn at(base: Instant, seconds: u64) -> Instant {
        base + Duration::from_secs(seconds)
    }

    #[test]
    fn stays_under_the_budget_by_dropping_the_longest_unpainted() {
        let (mut cache, base) = tracker();
        cache.painted(1, [(1, 1, 60)], at(base, 0));
        cache.painted(1, [(2, 2, 60)], at(base, 1));
        // Window 1 now shows only picture 2, so 1 is the one to go.
        let dropped = cache.evict(at(base, 2));
        assert_eq!(dropped, vec![1]);
        assert!(cache.used() <= 100);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn never_drops_what_a_window_just_painted() {
        let (mut cache, base) = tracker();
        // Three pictures of 60 bytes on one screen: 180 bytes against a budget of 100.
        cache.painted(1, [(1, 1, 60), (2, 2, 60), (3, 3, 60)], at(base, 0));
        assert!(cache.evict(at(base, 1)).is_empty());
        assert_eq!(cache.used(), 180);
        // Not even after a long time: the window has not painted again, so its frame is still what is on screen.
        assert!(cache.evict(at(base, 600)).is_empty());
    }

    #[test]
    fn a_window_that_stays_still_keeps_its_pictures_while_another_paints() {
        let (mut cache, base) = tracker();
        cache.painted(1, [(1, 1, 60)], at(base, 0));
        cache.painted(2, [(2, 2, 60)], at(base, 100));
        cache.painted(2, [(3, 3, 60)], at(base, 101));
        let dropped = cache.evict(at(base, 102));
        assert_eq!(dropped, vec![2]);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn drops_in_least_recently_painted_order() {
        let (mut cache, base) = tracker();
        cache.painted(1, [(1, 1, 40)], at(base, 0));
        cache.painted(1, [(2, 2, 40)], at(base, 1));
        cache.painted(1, [(3, 3, 40)], at(base, 2));
        cache.painted(1, [(4, 4, 40)], at(base, 3));
        // 160 bytes, budget 100, window 1 shows only 4: 1 and 2 go first, then it fits.
        assert_eq!(cache.evict(at(base, 4)), vec![1, 2]);
        assert_eq!(cache.used(), 80);
    }

    #[test]
    fn painting_again_makes_a_picture_recent() {
        let (mut cache, base) = tracker();
        cache.painted(1, [(1, 1, 60)], at(base, 0));
        cache.painted(1, [(2, 2, 60)], at(base, 1));
        cache.painted(1, [(1, 1, 60)], at(base, 2));
        cache.painted(1, [(3, 3, 10)], at(base, 3));
        // 130 bytes: 2 is the longest unpainted, and dropping it is enough.
        assert_eq!(cache.evict(at(base, 4)), vec![2]);
    }

    #[test]
    fn an_idle_picture_goes_even_under_the_budget() {
        let (mut cache, base) = tracker();
        cache.painted(1, [(1, 1, 10)], at(base, 0));
        cache.painted(1, [(2, 2, 10)], at(base, 30));
        assert!(cache.evict(at(base, 31)).is_empty());
        assert_eq!(cache.evict(at(base, 61)), vec![1]);
        assert_eq!(cache.used(), 10);
    }

    #[test]
    fn a_picture_painted_after_its_drop_is_tracked_again() {
        let (mut cache, base) = tracker();
        cache.painted(1, [(1, 1, 60)], at(base, 0));
        cache.painted(1, [(2, 2, 60)], at(base, 1));
        assert_eq!(cache.evict(at(base, 2)), vec![1]);
        // Picture 1 comes back on screen (decoded again, so a new value): it is known and sized again.
        cache.painted(1, [(1, 11, 60)], at(base, 3));
        assert_eq!(cache.used(), 120);
        assert_eq!(cache.evicted(), 1);
        assert_eq!(cache.evict(at(base, 4)), vec![2]);
    }

    #[test]
    fn the_budget_is_never_exceeded_by_pictures_off_screen() {
        let (mut cache, base) = tracker();
        for step in 0..50u64 {
            cache.painted(1, [(step, step, 30)], at(base, step));
            cache.evict(at(base, step));
            // Everything but the picture on screen fits in the budget.
            assert!(cache.used() <= 100 || cache.len() == 1);
        }
    }
}
