//! Port of the backoff class in module 158390.

use rand::Rng;
use std::time::Duration;

#[derive(Debug)]
pub struct Backoff {
    min: f64,
    max: f64,
    current: f64,
    fails: u32,
}

impl Backoff {
    pub fn new(min: Duration, max: Duration) -> Self {
        let min = min.as_secs_f64() * 1000.0;
        Self {
            min,
            max: max.as_secs_f64() * 1000.0,
            current: min,
            fails: 0,
        }
    }

    pub fn fails(&self) -> u32 {
        self.fails
    }

    pub fn succeed(&mut self) {
        self.fails = 0;
        self.current = self.min;
    }

    pub fn fail(&mut self, rng: &mut impl Rng) -> Duration {
        self.fails += 1;
        let step = 2.0 * self.current * rng.random::<f64>();
        self.current = (self.current + step).min(self.max);
        Duration::from_secs_f64(self.current / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_between_min_and_max_and_resets() {
        let mut rng = rand::rng();
        let mut backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(60));
        let mut last = Duration::from_secs(1);
        for _ in 0..200 {
            let delay = backoff.fail(&mut rng);
            assert!(delay >= last && delay <= Duration::from_secs(60));
            last = delay;
        }
        assert_eq!(backoff.fails(), 200);
        backoff.succeed();
        assert_eq!(backoff.fails(), 0);
        assert!(backoff.fail(&mut rng) <= Duration::from_secs(3));
    }
}
