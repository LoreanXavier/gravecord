//! Delivery measured where a viewer sees it: when each decoded picture of a source arrived, from which the frame rate
//! over time, the longest gap between pictures and the time to the first picture follow.
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Arrivals of one source.
#[derive(Clone, Debug, Default)]
pub struct Pictures {
    /// When watching (or the camera) was asked for; the first picture is timed from here.
    pub armed: Option<Instant>,
    pub first: Option<Instant>,
    pub count: u64,
    pub size: (u32, u32),
    /// Arrival times of the last 120 s.
    times: VecDeque<Instant>,
}

/// Numbers over a window of the arrivals.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub pictures: u64,
    pub fps: f64,
    pub longest_gap_ms: u64,
    pub first_ms: Option<u64>,
    pub size: (u32, u32),
    /// Pictures in each whole second of the window, oldest first.
    pub per_second: Vec<u32>,
}

impl Pictures {
    pub fn record(&mut self, now: Instant, width: u32, height: u32) {
        self.count += 1;
        self.size = (width, height);
        self.first.get_or_insert(now);
        self.times.push_back(now);
        while self
            .times
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) > Duration::from_secs(120))
        {
            self.times.pop_front();
        }
    }

    /// The last `window` before `now`. The longest gap counts the time from the window's start (or the first picture)
    /// to the first picture in it, and from the last picture to `now`, so a stream that stopped shows as a gap.
    pub fn summary(&self, now: Instant, window: Duration) -> Summary {
        let start = now.checked_sub(window).unwrap_or(now);
        let start = match self.first {
            Some(first) if first > start => first,
            _ => start,
        };
        let inside: Vec<Instant> = self.times.iter().copied().filter(|t| *t >= start).collect();
        let mut longest = Duration::ZERO;
        let mut previous = start;
        for t in &inside {
            longest = longest.max(t.saturating_duration_since(previous));
            previous = *t;
        }
        if self.first.is_some() {
            longest = longest.max(now.saturating_duration_since(previous));
        }
        let span = now.saturating_duration_since(start).as_secs_f64();
        let seconds = span.floor() as usize;
        let mut per_second = vec![0u32; seconds];
        for t in &inside {
            let index = t.saturating_duration_since(start).as_secs() as usize;
            if let Some(slot) = per_second.get_mut(index) {
                *slot += 1;
            }
        }
        Summary {
            pictures: inside.len() as u64,
            fps: if span > 0.0 {
                inside.len() as f64 / span
            } else {
                0.0
            },
            longest_gap_ms: longest.as_millis() as u64,
            first_ms: self
                .first
                .zip(self.armed)
                .map(|(first, armed)| first.saturating_duration_since(armed).as_millis() as u64),
            size: self.size,
            per_second,
        }
    }
}

impl Summary {
    pub fn line(&self) -> String {
        format!(
            "{} pictures, {:.1} fps, longest gap {} ms, first after {}, {}x{}",
            self.pictures,
            self.fps,
            self.longest_gap_ms,
            self.first_ms
                .map_or("-".to_owned(), |ms| format!("{ms} ms")),
            self.size.0,
            self.size.1
        )
    }
}

/// The sources one viewer receives, shared with its sink. Keys are (user, kind) with kind "camera" or "stream".
#[derive(Clone, Default)]
pub struct Board(Arc<Mutex<HashMap<(u64, &'static str), Pictures>>>);

impl Board {
    pub fn record(&self, user: u64, kind: &'static str, width: u32, height: u32) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry((user, kind))
            .or_default()
            .record(Instant::now(), width, height);
    }

    /// Starts timing `(user, kind)` from now: a new watch, so earlier arrivals are forgotten.
    pub fn arm(&self, user: u64, kind: &'static str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(
            (user, kind),
            Pictures {
                armed: Some(Instant::now()),
                ..Pictures::default()
            },
        );
    }

    pub fn summary(&self, user: u64, kind: &'static str, window: Duration) -> Option<Summary> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(user, kind))
            .map(|pictures| pictures.summary(Instant::now(), window))
    }

    pub fn keys(&self) -> Vec<(u64, &'static str)> {
        let mut keys: Vec<_> = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect();
        keys.sort();
        keys
    }

    pub fn clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_gaps_and_first_picture() {
        let mut pictures = Pictures::default();
        let start = Instant::now();
        pictures.armed = Some(start);
        // 30 fps for two seconds with one 300 ms hole.
        let mut t = start + Duration::from_millis(250);
        for i in 0..60 {
            if (30..39).contains(&i) {
                t += Duration::from_micros(33_333);
                continue;
            }
            pictures.record(t, 320, 180);
            t += Duration::from_micros(33_333);
        }
        let summary = pictures.summary(t, Duration::from_secs(10));
        assert_eq!(summary.first_ms, Some(250));
        assert_eq!(summary.pictures, 51);
        assert!(
            (295..=340).contains(&summary.longest_gap_ms),
            "{}",
            summary.longest_gap_ms
        );
        assert!((24.0..27.0).contains(&summary.fps), "{}", summary.fps);
        assert_eq!(summary.size, (320, 180));
        // Nothing more for a second: that is the gap now.
        let later = pictures.summary(t + Duration::from_secs(1), Duration::from_secs(10));
        assert!(later.longest_gap_ms >= 1000);
    }
}
