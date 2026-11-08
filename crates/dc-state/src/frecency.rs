//! The official `Frecency` class (module 283047), with the configurations of the emoji store (module 121681).
//!
//! Each key keeps its total uses and up to `max_samples` recent use times. A use's weight falls with its age in days
//! (`o`: 100 within 3 days, 70 within 15, 50 within 30, 30 within 45, 10 within 80, else 1), scaled by a bonus; the
//! summed score and the total feed `compute_frecency`, and the highest frecencies make the frequently used list.

/// `o` in module 283047.
pub fn weight(days: i64) -> f64 {
    if days <= 3 {
        100.
    } else if days <= 15 {
        70.
    } else if days <= 30 {
        50.
    } else if days <= 45 {
        30.
    } else if days <= 80 {
        10.
    } else {
        1.
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `eA`: emoji picker uses; frecency `ceil(total * score / recent count)`.
    Emoji,
    /// `ef`: reactions; frecency `trunc(1000 * (total / max_total * 0.2 + score / 1000 * 0.8))`.
    Reaction,
    /// Guilds and channels (module 205761): steeper day weights, frecency as for emoji, 100 kept.
    Channel,
}

/// `computeWeight` of the channel frecency in module 205761.
pub fn channel_weight(days: i64) -> f64 {
    match days {
        0 => 100.,
        1 => 70.,
        2 | 3 => 50.,
        4..=6 => 30.,
        _ => 10.,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Usage {
    pub total_uses: u32,
    pub recent_uses: Vec<u64>,
}

pub struct Frecency {
    kind: Kind,
    max_samples: usize,
    items: usize,
    usage: Vec<(String, Usage)>,
}

impl Frecency {
    /// Both emoji configurations use the defaults (`maxSamples` 10) and `numFrequentlyItems` 42 (`eu`).
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            max_samples: 10,
            items: if kind == Kind::Channel { 100 } else { 42 },
            usage: Vec::new(),
        }
    }

    /// `overwriteHistory`: the stored history replaces what is known.
    pub fn overwrite(&mut self, history: Vec<(String, Usage)>) {
        self.usage = history;
    }

    /// `track`: one more use now (or at `at`), keeping the newest `max_samples` times.
    pub fn track(&mut self, key: &str, at_ms: u64) {
        match self.usage.iter_mut().find(|(known, _)| known == key) {
            Some((_, usage)) => {
                usage.total_uses += 1;
                usage.recent_uses.push(at_ms);
                usage.recent_uses.sort_unstable();
                while usage.recent_uses.len() > self.max_samples {
                    usage.recent_uses.remove(0);
                }
            }
            None => self.usage.push((
                key.to_owned(),
                Usage {
                    total_uses: 1,
                    recent_uses: vec![at_ms],
                },
            )),
        }
    }

    pub fn usage(&self) -> &[(String, Usage)] {
        &self.usage
    }

    /// `compute` + `frequently`: keys by falling frecency, those `usable` rejects left out, at most 42. Ties keep
    /// insertion order, as lodash's stable `sortBy` does.
    pub fn frequently(&self, now_ms: u64, usable: impl Fn(&str) -> bool) -> Vec<String> {
        let max_total = self
            .usage
            .iter()
            .map(|(_, usage)| usage.total_uses)
            .max()
            .unwrap_or(0);
        let mut scored: Vec<(String, f64)> = self
            .usage
            .iter()
            .filter_map(|(key, usage)| {
                // `computeBonus` is 100 for emoji, so the factor is 1.
                let score: f64 = usage
                    .recent_uses
                    .iter()
                    .take(self.max_samples)
                    .map(|time| {
                        // moment's `diff(..., "days")` truncates toward zero.
                        let days = (now_ms as i64 - *time as i64) / 86_400_000;
                        if self.kind == Kind::Channel {
                            channel_weight(days)
                        } else {
                            weight(days)
                        }
                    })
                    .sum();
                if score <= 0. || usage.recent_uses.is_empty() {
                    return None;
                }
                let total = f64::from(usage.total_uses);
                let frecency = match self.kind {
                    Kind::Emoji | Kind::Channel => {
                        (total * (score / usage.recent_uses.len() as f64)).ceil()
                    }
                    Kind::Reaction if max_total == 0 => 0.,
                    Kind::Reaction => {
                        (1000. * (total / f64::from(max_total) * 0.2 + score / 1000. * 0.8)).trunc()
                    }
                };
                Some((key.clone(), frecency))
            })
            .filter(|(key, _)| usable(key))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored
            .into_iter()
            .take(self.items)
            .map(|(key, _)| key)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400_000;

    #[test]
    fn weights_follow_the_age_bands() {
        assert_eq!(weight(0), 100.);
        assert_eq!(weight(3), 100.);
        assert_eq!(weight(4), 70.);
        assert_eq!(weight(31), 30.);
        assert_eq!(weight(81), 1.);
    }

    #[test]
    fn reactions_rank_recent_use_over_old_volume() {
        let now = 1_000 * DAY;
        let mut frecency = Frecency::new(Kind::Reaction);
        frecency.overwrite(vec![
            (
                "old".into(),
                Usage {
                    total_uses: 50,
                    recent_uses: vec![now - 200 * DAY; 10],
                },
            ),
            (
                "fresh".into(),
                Usage {
                    total_uses: 3,
                    recent_uses: vec![now - DAY, now - DAY, now],
                },
            ),
        ]);
        // old: 1000 * (1 * .2 + 10/1000 * .8) = 208; fresh: 1000 * (.06 * .2 + 300/1000 * .8) = 252.
        assert_eq!(frecency.frequently(now, |_| true), ["fresh", "old"]);
        assert_eq!(frecency.frequently(now, |key| key != "fresh"), ["old"]);
    }

    #[test]
    fn tracking_keeps_ten_samples() {
        let mut frecency = Frecency::new(Kind::Emoji);
        for day in 0..12 {
            frecency.track("joy", day * DAY);
        }
        let usage = &frecency.usage()[0].1;
        assert_eq!(usage.total_uses, 12);
        assert_eq!(usage.recent_uses.len(), 10);
        assert_eq!(usage.recent_uses[0], 2 * DAY);
    }
}
