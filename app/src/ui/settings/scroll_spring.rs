//! The scrollers' animated scroll (module 354730): a spring on the scroll position, `tension: 200, friction: 35,
//! mass: 2, clamp: true`, integrated in fixed 1/240 s steps with what is left of a frame interpolated, at rest when
//! one step's change of speed is under `.001` or, clamped, the moment it reaches or passes the target. The settings
//! sidebar and content scrollers use it for `scrollIntoView({animate: true})` (modules 397274, 584648).

/// One integration step, in seconds (`r = 1/240`).
const STEP: f64 = 1. / 240.;
const TENSION: f64 = 200.;
const FRICTION: f64 = 35.;
const MASS: f64 = 2.;
const THRESHOLD: f64 = 0.001;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ScrollSpring {
    /// The integrated position (`from`), in scroll pixels from the top.
    from: f64,
    target: f64,
    velocity: f64,
    accumulator: f64,
    /// The time of the last frame, in ms; none before the first frame of a run, which only records it.
    last: Option<f64>,
    animating: bool,
    /// The position last handed to the scroller, to tell a wheel scroll that took over.
    shown: f64,
}

impl ScrollSpring {
    pub(super) fn is_animating(&self) -> bool {
        self.animating
    }

    /// `to({to, from, animate})`: an animated run starts from rest at `from`; one already running keeps its speed
    /// and only moves its target.
    pub(in crate::ui) fn start(&mut self, from: f64, to: f64) {
        self.target = to;
        self.from = from;
        self.shown = from;
        self.accumulator = 0.;
        if !self.animating {
            self.animating = true;
            self.velocity = 0.;
            self.last = None;
        }
    }

    pub(in crate::ui) fn target(&self) -> f64 {
        self.target
    }

    pub(in crate::ui) fn set_shown(&mut self, shown: f64) {
        self.shown = shown;
    }

    /// Lets go: a wheel scroll or a jump took the scroller over.
    pub(super) fn abort(&mut self) {
        self.animating = false;
        self.accumulator = 0.;
    }

    /// The position the scroller last got from this spring.
    pub(super) fn shown(&self) -> f64 {
        self.shown
    }

    fn updates(&self, velocity: f64, position: f64) -> (f64, f64, f64) {
        let acceleration = (-TENSION * (position - self.target) + -FRICTION * velocity) / MASS;
        let velocity = velocity + acceleration * STEP;
        (position + velocity * STEP, velocity, acceleration)
    }

    /// `update(now)`: the position for the frame at `now` (ms), or None on a run's first frame, which only takes
    /// the time.
    pub(super) fn frame(&mut self, now: f64) -> Option<f64> {
        if !self.animating {
            return None;
        }
        let Some(last) = self.last else {
            self.last = Some(now);
            return None;
        };
        self.accumulator = ((now - last) / 1000. + self.accumulator).min(2.);
        while self.accumulator > STEP {
            self.accumulator -= STEP;
            let (position, velocity, acceleration) = self.updates(self.velocity, self.from);
            self.velocity = velocity;
            let crossed = position == self.target
                || (position < self.target && self.from > self.target)
                || (position > self.target && self.from < self.target);
            if crossed || (acceleration * STEP).abs() < THRESHOLD {
                self.from = self.target;
                self.shown = self.target;
                self.abort();
                return Some(self.target);
            }
            self.from = position;
        }
        let mut position = self.from;
        if self.accumulator > 0. {
            let (next, _, _) = self.updates(self.velocity, position);
            position += (next - position) * (self.accumulator / STEP);
        }
        self.last = Some(now);
        self.shown = position;
        Some(position)
    }
}

#[cfg(test)]
mod tests {
    use super::ScrollSpring;

    fn run(from: f64, to: f64) -> Vec<f64> {
        let mut spring = ScrollSpring::default();
        spring.start(from, to);
        let mut frames = Vec::new();
        let mut now = 1000.;
        while spring.is_animating() && frames.len() < 400 {
            if let Some(position) = spring.frame(now) {
                frames.push(position);
            }
            now += 16.;
        }
        frames
    }

    /// The first frame of a run only takes the time; the spring then moves without overshoot (clamped) and lands
    /// on the target exactly.
    #[test]
    fn the_scroll_glides_to_its_target_without_passing_it() {
        let frames = run(0., 300.);
        assert!(
            frames[0] > 0. && frames[0] < 10.,
            "first step {}",
            frames[0]
        );
        assert!(frames.windows(2).all(|pair| pair[1] >= pair[0]));
        assert_eq!(*frames.last().unwrap(), 300.);
        assert!(frames.iter().all(|position| *position <= 300.));
        // tension 200, friction 35, mass 2: about half way after 200 ms, settled well within a second.
        let half = frames
            .iter()
            .position(|position| *position >= 150.)
            .unwrap();
        assert!((8..=16).contains(&half), "half way at frame {half}");
        assert!(frames.len() < 70, "{} frames", frames.len());
    }

    #[test]
    fn a_run_upwards_mirrors_one_downwards() {
        let down = run(0., 200.);
        let up = run(200., 0.);
        assert_eq!(down.len(), up.len());
        for (a, b) in down.iter().zip(&up) {
            assert!((a - (200. - b)).abs() < 1e-6);
        }
    }

    #[test]
    fn a_retarget_keeps_the_speed() {
        let mut spring = ScrollSpring::default();
        spring.start(0., 100.);
        spring.frame(0.);
        let first = spring.frame(16.).unwrap();
        spring.start(first, 400.);
        let second = spring.frame(32.).unwrap();
        assert!(
            second - first > first,
            "kept its speed: {first} then {second}"
        );
    }
}
