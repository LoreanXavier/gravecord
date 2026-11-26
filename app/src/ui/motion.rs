//! The motion layer: what the official client's CSS transitions and react-spring animations become here.
//!
//! * `Ease`: the CSS timing functions (`cubic-bezier`, the `ease*` keywords, `linear(...)`), solved the way Chromium
//!   solves them.
//! * `Spring`: react-spring's `SpringValue.advance` (module 666764), stepped in whole milliseconds as the original
//!   does, with `tension`, `friction`, `mass`, `clamp`, `precision`, `restVelocity` and `velocity`.
//! * `animate` / `value` / `color`: a tween per element key. A target that changes mid-flight starts from the value
//!   on screen, and a reversal shortens the new run, as a CSS transition does (CSS Transitions Level 1, section 3).
//! * `spring_value`: the same for a spring, keeping its velocity when the target moves.
//! * `Presence`: keeps an element mounted while its exit animation runs (react-spring's `useTransition` `leave`).
//! * The redraw driver: `begin_frame` at the top of the root's render and `driver()` at the end of its tree. Frames
//!   are requested only while some value is still moving; an idle window requests none.
//! * Reduced motion: `set_reduced`. Official rule (module 775602, `useReducedMotion`): with "Sync with computer
//!   setting" on the system's setting decides, otherwise the app's own switch does. Transitions the stylesheets put
//!   under `.full-motion` (`Gate::FullMotion`) snap, springs follow their own `reducedMotion.enabled` branch, and
//!   the stylesheet rules that stay in force under `.reduce-motion` (`Gate::Always`) keep animating.
//!
//! The clock can be frozen by the capture harness (`motion clock <ms>`), so a frame shows what the curve gives at an
//! exact time after an interaction.

// The module is the motion layer's whole surface (every official easing, spring preset and builder, each covered by
// its tests); the app calls what the families ported so far need.
#![allow(dead_code)]

use gpui::{AnyElement, IntoElement, Rgba, Styled, canvas, div};

pub(crate) mod arrival;
pub(crate) mod composer;
pub(crate) mod controls;
pub(crate) mod departure;
pub(crate) mod dm_rail;
pub(crate) mod layers;
pub(crate) mod messages;
pub(crate) mod rail;
pub(crate) mod toasts;
pub(crate) mod typing;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Instant;

/// `Math.min(64, elapsed)` in the frame loop of module 572377: the longest step a spring takes in one frame.
pub const FRAME_CAP_MS: f64 = 64.;

// ---------------------------------------------------------------------------------------------------------------
// Easing
// ---------------------------------------------------------------------------------------------------------------

/// A CSS timing function.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::enum_variant_names)] // the names are the CSS keywords
pub enum Ease {
    Linear,
    /// `ease`: `cubic-bezier(.25, .1, .25, 1)`.
    Ease,
    /// `ease-in`: `cubic-bezier(.42, 0, 1, 1)`.
    EaseIn,
    /// `ease-out`: `cubic-bezier(0, 0, .58, 1)`.
    EaseOut,
    /// `ease-in-out`: `cubic-bezier(.42, 0, .58, 1)`.
    EaseInOut,
    Bezier(f32, f32, f32, f32),
    /// `Easing.out(Easing.quad)` of the Animated library: `1 - (1 - x)²`.
    OutQuad,
    /// `linear(a 0%, b 10%, ...)`: (input progress, output) pairs, both from 0 to 1.
    Stops(&'static [(f32, f32)]),
}

impl Ease {
    /// The eased value for a progress in `0..=1`.
    pub fn apply(self, x: f64) -> f64 {
        let x = x.clamp(0., 1.);
        match self {
            Ease::Linear => x,
            Ease::Ease => bezier(0.25, 0.1, 0.25, 1., x),
            Ease::EaseIn => bezier(0.42, 0., 1., 1., x),
            Ease::EaseOut => bezier(0., 0., 0.58, 1., x),
            Ease::EaseInOut => bezier(0.42, 0., 0.58, 1., x),
            Ease::Bezier(x1, y1, x2, y2) => bezier(x1 as f64, y1 as f64, x2 as f64, y2 as f64, x),
            Ease::OutQuad => 1. - (1. - x) * (1. - x),
            Ease::Stops(stops) => {
                let mut previous = (0., 0.);
                for &(at, value) in stops {
                    let (at, value) = (at as f64, value as f64);
                    if x <= at {
                        let span = at - previous.0;
                        return if span <= 0. {
                            value
                        } else {
                            previous.1 + (value - previous.1) * (x - previous.0) / span
                        };
                    }
                    previous = (at, value);
                }
                previous.1
            }
        }
    }
}

/// `cubic-bezier(x1, y1, x2, y2)` at time `x`: Newton's method with a bisection fallback (the solver of
/// Chromium's `UnitBezier`).
fn bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let cx = 3. * x1;
    let bx = 3. * (x2 - x1) - cx;
    let ax = 1. - cx - bx;
    let cy = 3. * y1;
    let by = 3. * (y2 - y1) - cy;
    let ay = 1. - cy - by;
    let sample_x = |t: f64| ((ax * t + bx) * t + cx) * t;
    let sample_y = |t: f64| ((ay * t + by) * t + cy) * t;
    let slope_x = |t: f64| (3. * ax * t + 2. * bx) * t + cx;
    const EPSILON: f64 = 1e-7;
    let mut t = x;
    let mut solved = false;
    for _ in 0..8 {
        let error = sample_x(t) - x;
        if error.abs() < EPSILON {
            solved = true;
            break;
        }
        let slope = slope_x(t);
        if slope.abs() < 1e-6 {
            break;
        }
        t -= error / slope;
    }
    if !solved {
        let (mut low, mut high) = (0., 1.);
        t = x;
        while low < high {
            let error = sample_x(t);
            if (error - x).abs() < EPSILON {
                break;
            }
            if x > error {
                low = t;
            } else {
                high = t;
            }
            let next = (high - low) * 0.5 + low;
            if next == t {
                break;
            }
            t = next;
        }
    }
    sample_y(t)
}

// ---------------------------------------------------------------------------------------------------------------
// Springs
// ---------------------------------------------------------------------------------------------------------------

/// A react-spring config. Unset fields keep react-spring's defaults (`mass: 1`, `velocity: 0`, `clamp: false`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringConfig {
    pub tension: f64,
    pub friction: f64,
    pub mass: f64,
    pub clamp: bool,
    pub precision: Option<f64>,
    pub rest_velocity: Option<f64>,
    pub velocity: f64,
}

impl SpringConfig {
    pub const fn new(tension: f64, friction: f64) -> Self {
        Self {
            tension,
            friction,
            mass: 1.,
            clamp: false,
            precision: None,
            rest_velocity: None,
            velocity: 0.,
        }
    }

    pub const fn mass(mut self, mass: f64) -> Self {
        self.mass = mass;
        self
    }

    pub const fn clamp(mut self, clamp: bool) -> Self {
        self.clamp = clamp;
        self
    }

    pub const fn precision(mut self, precision: f64) -> Self {
        self.precision = Some(precision);
        self
    }
}

/// The named presets of module 666764 (`config.default` is `{tension: 170, friction: 26}`).
pub mod presets {
    use super::SpringConfig;
    pub const DEFAULT: SpringConfig = SpringConfig::new(170., 26.);
    pub const GENTLE: SpringConfig = SpringConfig::new(120., 14.);
    pub const WOBBLY: SpringConfig = SpringConfig::new(180., 12.);
    pub const STIFF: SpringConfig = SpringConfig::new(210., 20.);
    pub const SLOW: SpringConfig = SpringConfig::new(280., 60.);
    pub const MOLASSES: SpringConfig = SpringConfig::new(280., 120.);
    /// Tooltips, module 280336.
    pub const TOOLTIP: SpringConfig = SpringConfig::new(2400., 52.);
}

/// One animated number, advanced the way `SpringValue.advance` does it: one integration step per millisecond.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub config: SpringConfig,
    from: f64,
    to: f64,
    position: f64,
    /// Units per millisecond, as in the original.
    velocity: f64,
    started_velocity: bool,
    done: bool,
    /// The time, in ms on the motion clock, up to which the spring has been stepped.
    stepped_to: f64,
}

impl Spring {
    pub fn new(config: SpringConfig, at: f64, now: f64) -> Self {
        Self {
            config,
            from: at,
            to: at,
            position: at,
            velocity: 0.,
            started_velocity: false,
            done: true,
            stepped_to: now,
        }
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    pub fn target(&self) -> f64 {
        self.to
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Moves the target. A moving spring keeps its position and velocity; a finished one starts from rest (the
    /// `reset` of an `AnimatedValue` drops `lastVelocity` once the value is done).
    pub fn retarget(&mut self, to: f64, now: f64) {
        if to == self.to {
            return;
        }
        // Bring the spring up to `now` against the old target first.
        self.step_to(now);
        if self.done {
            self.velocity = 0.;
        }
        self.from = self.position;
        self.to = to;
        self.started_velocity = false;
        self.done = false;
        self.stepped_to = now;
    }

    /// Steps to `now` (whole milliseconds, and never more than `FRAME_CAP_MS` per call, as in the frame loop).
    pub fn step_to(&mut self, now: f64) {
        if self.done {
            self.stepped_to = now;
            return;
        }
        let mut steps = (now - self.stepped_to).floor();
        if steps < 1. {
            return;
        }
        self.stepped_to += steps;
        // The frame loop hands `advance` at most 64 ms at a time; a long gap is several frames.
        while steps > 0. && !self.done {
            let chunk = steps.min(FRAME_CAP_MS);
            self.advance(chunk as u32);
            steps -= chunk;
        }
    }

    /// `SpringValue.advance(dt)` for one number: `dt` single-millisecond integration steps.
    pub fn advance(&mut self, dt: u32) {
        if self.done {
            return;
        }
        let config = self.config;
        if config.tension <= 0. {
            self.done = true;
            return;
        }
        let from = self.from;
        let to = self.to;
        if !self.started_velocity {
            self.started_velocity = true;
            if self.velocity == 0. {
                self.velocity = config.velocity;
            }
        }
        let v0 = self.velocity;
        let mut velocity = self.velocity;
        let mut position = self.position;
        let precision = config.precision.unwrap_or(if from == to {
            0.005
        } else {
            (0.001 * (to - from).abs()).min(1.)
        });
        let rest_velocity = config.rest_velocity.unwrap_or(precision / 10.);
        let bounce = if config.clamp { Some(0.) } else { None };
        let upward = if from == to { v0 > 0. } else { from < to };
        let mut done = false;
        for _ in 0..dt {
            done = (to - position).abs() <= precision;
            if velocity.abs() <= rest_velocity && done {
                break;
            }
            done = false;
            if let Some(bounce) = bounce
                && (position == to || (position > to) == upward)
            {
                velocity = -velocity * bounce;
                position = to;
            }
            let acceleration = (-(1e-6 * config.tension) * (position - to)
                + -(0.001 * config.friction) * velocity)
                / config.mass;
            velocity += acceleration;
            position += velocity;
        }
        self.velocity = velocity;
        if done {
            self.done = true;
            self.position = to;
            self.velocity = 0.;
        } else {
            self.position = position;
        }
    }
}

/// The spring of the layer stack (module 913420 / `Animated.spring` of the web build of React Native, module
/// 222574): fourth-order Runge-Kutta in 1 ms steps, the origami tension and friction converted
/// (module 735556), at rest when speed and distance are both under .001.
#[derive(Clone, Copy, Debug)]
pub struct RnSpring {
    stiffness: f64,
    damping: f64,
    to: f64,
    position: f64,
    velocity: f64,
    done: bool,
    stepped_to: f64,
}

impl RnSpring {
    /// `Animated.spring(value, {tension, friction})`.
    pub fn new(tension: f64, friction: f64, at: f64, now: f64) -> Self {
        Self {
            stiffness: (tension - 30.) * 3.62 + 194.,
            damping: (friction - 8.) * 3. + 25.,
            to: at,
            position: at,
            velocity: 0.,
            done: true,
            stepped_to: now,
        }
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    pub fn target(&self) -> f64 {
        self.to
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn retarget(&mut self, to: f64, now: f64) {
        if to == self.to {
            return;
        }
        self.step_to(now);
        self.to = to;
        self.done = false;
        self.stepped_to = now;
    }

    pub fn step_to(&mut self, now: f64) {
        if self.done {
            self.stepped_to = now;
            return;
        }
        let mut steps = (now - self.stepped_to).floor();
        if steps < 1. {
            return;
        }
        self.stepped_to += steps;
        while steps > 0. && !self.done {
            let chunk = steps.min(FRAME_CAP_MS);
            for _ in 0..chunk as u32 {
                self.rk4();
            }
            steps -= chunk;
            if self.velocity.abs() <= 0.001 && (self.to - self.position).abs() <= 0.001 {
                self.position = self.to;
                self.velocity = 0.;
                self.done = true;
            }
        }
    }

    fn rk4(&mut self) {
        let (k, c, to) = (self.stiffness, self.damping, self.to);
        let (e, t) = (self.position, self.velocity);
        let a_velocity = t;
        let a_acceleration = k * (to - e) - c * t;
        let temp_position = e + 0.001 * a_velocity / 2.;
        let temp_velocity = t + 0.001 * a_acceleration / 2.;
        let b_velocity = temp_velocity;
        let b_acceleration = k * (to - temp_position) - c * temp_velocity;
        let temp_position = e + 0.001 * b_velocity / 2.;
        let temp_velocity = t + 0.001 * b_acceleration / 2.;
        let c_velocity = temp_velocity;
        let c_acceleration = k * (to - temp_position) - c * temp_velocity;
        let temp_position = e + 0.001 * c_velocity / 2.;
        let temp_velocity = t + 0.001 * c_acceleration / 2.;
        let d_velocity = temp_velocity;
        let d_acceleration = k * (to - temp_position) - c * temp_velocity;
        let dxdt = (a_velocity + 2. * (b_velocity + c_velocity) + d_velocity) / 6.;
        let dvdt = (a_acceleration + 2. * (b_acceleration + c_acceleration) + d_acceleration) / 6.;
        self.position = e + dxdt * 0.001;
        self.velocity = t + dvdt * 0.001;
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The clock
// ---------------------------------------------------------------------------------------------------------------

/// How the stylesheet gates a transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// `.full-motion .x { transition: ... }`: gone under reduced motion, the value snaps.
    FullMotion,
    /// A plain `transition`: stays in force under reduced motion.
    Always,
}

/// One CSS `transition` entry: duration, timing function, delay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Css {
    pub duration_ms: f64,
    pub ease: Ease,
    pub delay_ms: f64,
    pub gate: Gate,
}

impl Css {
    pub const fn new(duration_ms: f64, ease: Ease) -> Self {
        Self {
            duration_ms,
            ease,
            delay_ms: 0.,
            gate: Gate::Always,
        }
    }

    pub const fn delay(mut self, delay_ms: f64) -> Self {
        self.delay_ms = delay_ms;
        self
    }

    pub const fn full_motion(mut self) -> Self {
        self.gate = Gate::FullMotion;
        self
    }
}

/// A plain value in `Css` terms: `transition: <property> <seconds>s <ease>`.
pub const fn css(duration_ms: f64, ease: Ease) -> Css {
    Css::new(duration_ms, ease)
}

type V4 = [f64; 4];

#[derive(Clone, Copy, Debug)]
struct Tween {
    from: V4,
    to: V4,
    start: f64,
    duration: f64,
    ease: Ease,
    /// The reversing shortening factor of the run that is going (1 for a first run).
    factor: f64,
    touched: u64,
}

impl Tween {
    fn progress(&self, now: f64) -> f64 {
        if self.duration <= 0. {
            1.
        } else {
            ((now - self.start) / self.duration).clamp(0., 1.)
        }
    }

    fn at(&self, now: f64) -> V4 {
        if now >= self.start + self.duration {
            return self.to;
        }
        let eased = self.ease.apply(self.progress(now));
        let mut out = [0.; 4];
        for (i, value) in out.iter_mut().enumerate() {
            *value = self.from[i] + (self.to[i] - self.from[i]) * eased;
        }
        out
    }

    fn finished(&self, now: f64) -> bool {
        now >= self.start + self.duration
    }
}

struct SpringSlot {
    spring: Spring,
    touched: u64,
}

struct RnSlot {
    spring: RnSpring,
    touched: u64,
}

#[derive(Default)]
struct Motion {
    start: Option<Instant>,
    /// Set by the capture harness: the clock stands still at this many milliseconds.
    frozen: Option<f64>,
    /// The time every read of the current frame uses.
    frame_now: Option<f64>,
    tweens: HashMap<u64, Tween>,
    springs: HashMap<u64, SpringSlot>,
    rn_springs: HashMap<u64, RnSlot>,
    hovers: HashMap<u64, (bool, f64)>,
    /// For keyframe animations that follow a two-state control: the last state and when it changed.
    toggles: HashMap<u64, (bool, f64)>,
    leaving: std::collections::HashSet<u64>,
    frame: u64,
    active: bool,
    /// The views that asked for another frame in their last render, and the view being built.
    active_views: std::collections::HashSet<gpui::EntityId>,
    views: HashMap<gpui::EntityId, ViewFrames>,
    view: Option<gpui::EntityId>,
    reduced: bool,
    reduced_override: Option<bool>,
    /// Frames requested because something was moving, over the process's life.
    requested: u64,
}

thread_local! {
    static MOTION: RefCell<Motion> = RefCell::new(Motion::default());
}

fn with<R>(f: impl FnOnce(&mut Motion) -> R) -> R {
    MOTION.with(|motion| f(&mut motion.borrow_mut()))
}

impl Motion {
    fn clock(&mut self) -> f64 {
        if let Some(frozen) = self.frozen {
            return frozen;
        }
        let start = *self.start.get_or_insert_with(Instant::now);
        start.elapsed().as_secs_f64() * 1000.
    }

    fn now(&mut self) -> f64 {
        match self.frame_now {
            Some(now) => now,
            None => self.clock(),
        }
    }
}

/// How many draws a finished value is kept after it was last read. A region that is not redrawn does not read its
/// values, so a value is not let go of just because the last few draws did not read it.
const SWEEP_AFTER: u64 = 36_000;

/// What the motion store knows of one view (the main view or a region): the draw it last rendered in and the one
/// before.
#[derive(Clone, Copy, Default)]
struct ViewFrames {
    current: u64,
    previous: u64,
}

impl Motion {
    /// Records that something on screen is still moving, for the view being built, or for every view when no view is
    /// being built (the caller then draws somewhere this module cannot tell).
    fn mark_active(&mut self) {
        match self.view {
            Some(view) => {
                self.active_views.insert(view);
            }
            None => self.active = true,
        }
    }

    /// The draw a value must have been read in, at the latest, to count as shown in the previous render of the view
    /// being built (a value that was not is mounting again).
    fn gap_floor(&self) -> u64 {
        match self.view.and_then(|view| self.views.get(&view)) {
            Some(frames) => frames.previous,
            None => self.frame.saturating_sub(1),
        }
    }
}

/// Called at the top of a view's render (the main view's and each region's): the view starts a new render, so what
/// it asked for in the last one is forgotten, and the reads that follow belong to it.
pub fn begin_view(view: gpui::EntityId) {
    with(|motion| {
        let frame = motion.frame;
        let current = motion.views.get(&view).map_or(0, |frames| frames.current);
        motion.views.insert(
            view,
            ViewFrames {
                current: frame,
                previous: current,
            },
        );
        motion.active_views.remove(&view);
        motion.view = Some(view);
    });
}

/// The views that asked for another frame in their last render, and whether something drawn where no view was
/// known is moving (the capture harness notifies them when it draws by hand).
#[cfg(feature = "capture")]
pub fn animating_views() -> (Vec<gpui::EntityId>, bool) {
    with(|motion| (motion.active_views.iter().copied().collect(), motion.active))
}

/// The reads that follow belong to `view` (its rows are built after its render ended).
pub fn enter_view(view: gpui::EntityId) {
    with(|motion| motion.view = Some(view));
}

/// The reads that follow belong to no known view: whatever moves there is redrawn everywhere.
pub fn leave_view() {
    with(|motion| motion.view = None);
}

/// The view being built now, if known.
pub fn current_view() -> Option<gpui::EntityId> {
    with(|motion| motion.view)
}

/// A key for the stores from anything hashable.
pub fn key<H: Hash + ?Sized>(value: &H) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Called at the top of the root's render: fixes the time of this frame, applies the reduced-motion decision and
/// forgets finished values nobody read in the previous frame.
pub fn begin_frame(reduced: bool) {
    with(|motion| {
        let now = motion.clock();
        motion.frame_now = Some(now);
        motion.reduced = motion.reduced_override.unwrap_or(reduced);
        motion.frame += 1;
        motion.active = false;
        let frame = motion.frame;
        if motion.tweens.len() + motion.springs.len() > 256 && frame % 32 == 0 {
            motion
                .tweens
                .retain(|_, t| t.touched + SWEEP_AFTER >= frame || !t.finished(now));
            motion
                .springs
                .retain(|_, s| s.touched + SWEEP_AFTER >= frame || !s.spring.is_done());
            motion.hovers.retain(|_, h| h.0 || now - h.1 < 5000.);
        }
    });
}

/// Whether motion is reduced for this frame.
pub fn reduced() -> bool {
    with(|motion| motion.reduced)
}

/// Forces the reduced-motion decision (capture harness).
pub fn force_reduced(value: Option<bool>) {
    with(|motion| motion.reduced_override = value);
}

/// The milliseconds on the motion clock for this frame.
pub fn now() -> f64 {
    with(|motion| motion.now())
}

/// Freezes the clock at `ms` (capture harness), or lets it run again with `None`.
pub fn freeze(ms: Option<f64>) {
    with(|motion| {
        motion.frozen = ms;
        motion.frame_now = None;
    });
}

pub fn is_frozen() -> bool {
    with(|motion| motion.frozen.is_some())
}

/// How many frames the driver asked for since the process began.
pub fn frames_requested() -> u64 {
    with(|motion| motion.requested)
}

/// Whether anything read this frame is still moving (what the driver acts on).
pub fn is_active() -> bool {
    with(|motion| motion.active || !motion.active_views.is_empty())
}

// ---------------------------------------------------------------------------------------------------------------
// Tweens
// ---------------------------------------------------------------------------------------------------------------

/// The value for `target` under a CSS transition, starting at `initial` the first time the key is seen. Every
/// later change of the target starts from the value that is on screen.
pub fn animate_from(key: u64, initial: V4, target: V4, css: Css) -> V4 {
    with(|motion| {
        let now = motion.now();
        let frame = motion.frame;
        let snap = motion.reduced && css.gate == Gate::FullMotion;
        let duration = if snap { 0. } else { css.duration_ms };
        let delay = if snap { 0. } else { css.delay_ms };
        let entry = motion.tweens.get(&key).copied();
        let mut tween = match entry {
            None => Tween {
                from: initial,
                to: target,
                start: now + delay,
                // Nothing to run when the first value is the target.
                duration: if initial == target { 0. } else { duration },
                ease: css.ease,
                factor: 1.,
                touched: frame,
            },
            Some(mut tween) => {
                tween.touched = frame;
                if tween.to != target {
                    let current = tween.at(now);
                    let running = !tween.finished(now) && now >= tween.start;
                    let (mut run_duration, mut factor) = (duration, 1.);
                    if running && tween.from == target {
                        // A reversal: the new run is only as long as the way back (CSS Transitions 1, 3.1).
                        let eased = tween.ease.apply(tween.progress(now));
                        factor = (eased * tween.factor + (1. - tween.factor))
                            .abs()
                            .clamp(0., 1.);
                        run_duration = duration * factor;
                    }
                    tween = Tween {
                        from: current,
                        to: target,
                        start: now + delay,
                        duration: run_duration,
                        ease: css.ease,
                        factor,
                        touched: frame,
                    };
                }
                tween
            }
        };
        if snap && !tween.finished(now) {
            tween.duration = 0.;
        }
        let value = tween.at(now);
        let moving = !tween.finished(now);
        motion.tweens.insert(key, tween);
        if moving {
            motion.mark_active();
        }
        value
    })
}

/// `animate_from` that starts at the target the first time (a CSS transition does not run on first style).
pub fn animate(key: u64, target: V4, css: Css) -> V4 {
    animate_from(key, target, target, css)
}

/// One number under a CSS transition.
pub fn value(key: u64, target: f32, css: Css) -> f32 {
    animate(key, [target as f64, 0., 0., 0.], css)[0] as f32
}

/// One number under a CSS transition that runs from `initial` the first time (an enter).
pub fn value_from(key: u64, initial: f32, target: f32, css: Css) -> f32 {
    animate_from(
        key,
        [initial as f64, 0., 0., 0.],
        [target as f64, 0., 0., 0.],
        css,
    )[0] as f32
}

fn premultiplied(color: Rgba) -> V4 {
    let alpha = color.a as f64;
    [
        color.r as f64 * alpha,
        color.g as f64 * alpha,
        color.b as f64 * alpha,
        alpha,
    ]
}

fn from_premultiplied(value: V4) -> Rgba {
    let alpha = value[3].clamp(0., 1.);
    if alpha <= 0. {
        return Rgba {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 0.,
        };
    }
    Rgba {
        r: (value[0] / alpha).clamp(0., 1.) as f32,
        g: (value[1] / alpha).clamp(0., 1.) as f32,
        b: (value[2] / alpha).clamp(0., 1.) as f32,
        a: alpha as f32,
    }
}

/// A colour under a CSS transition. Colours interpolate in premultiplied sRGB, as CSS specifies.
pub fn color(key: u64, target: Rgba, css: Css) -> Rgba {
    from_premultiplied(animate(key, premultiplied(target), css))
}

// ---------------------------------------------------------------------------------------------------------------
// Springs in the store
// ---------------------------------------------------------------------------------------------------------------

/// The position of the spring under `key`, which starts at `initial` when the key is new and moves toward
/// `target`.
pub fn spring_from(key: u64, initial: f32, target: f32, config: SpringConfig) -> f32 {
    with(|motion| {
        let now = motion.now();
        let frame = motion.frame;
        let slot = motion.springs.entry(key).or_insert_with(|| {
            let mut spring = Spring::new(config, initial as f64, now);
            spring.retarget(target as f64, now);
            SpringSlot {
                spring,
                touched: frame,
            }
        });
        slot.touched = frame;
        slot.spring.config = config;
        if slot.spring.target() != target as f64 {
            slot.spring.retarget(target as f64, now);
        }
        slot.spring.step_to(now);
        let position = slot.spring.position() as f32;
        if !slot.spring.is_done() {
            motion.mark_active();
        }
        position
    })
}

/// `spring_from` starting at the target.
pub fn spring_value(key: u64, target: f32, config: SpringConfig) -> f32 {
    spring_from(key, target, target, config)
}

/// The position of a layer-stack spring under `key` (see `RnSpring`).
pub fn rn_spring_from(key: u64, initial: f32, target: f32, tension: f64, friction: f64) -> f32 {
    with(|motion| {
        let now = motion.now();
        let frame = motion.frame;
        let slot = motion.rn_springs.entry(key).or_insert_with(|| {
            let mut spring = RnSpring::new(tension, friction, initial as f64, now);
            spring.retarget(target as f64, now);
            RnSlot {
                spring,
                touched: frame,
            }
        });
        slot.touched = frame;
        if slot.spring.target() != target as f64 {
            slot.spring.retarget(target as f64, now);
        }
        slot.spring.step_to(now);
        let position = slot.spring.position() as f32;
        if !slot.spring.is_done() {
            motion.mark_active();
        }
        position
    })
}

/// A value that runs from `initial` to `target` once when the key is first seen after a gap of at least one frame
/// (an element that mounts), and stays there while it is drawn every frame. `drive` says how it moves.
pub fn enter(key: u64, drive: Drive) -> f32 {
    enter_between(key, 0., 1., drive)
}

/// `enter` between any two values. The target may change while the element stays mounted (an exit): the value then
/// retargets from where it is, keeping its velocity.
pub fn enter_between(key: u64, from: f32, to: f32, drive: Drive) -> f32 {
    let remounted = with(|motion| {
        let seen = motion
            .tweens
            .get(&key)
            .map(|t| t.touched)
            .or_else(|| motion.springs.get(&key).map(|s| s.touched))
            .or_else(|| motion.rn_springs.get(&key).map(|s| s.touched));
        match seen {
            Some(seen) if seen < motion.gap_floor() => {
                motion.tweens.remove(&key);
                motion.springs.remove(&key);
                motion.rn_springs.remove(&key);
                true
            }
            _ => false,
        }
    });
    let _ = remounted;
    match drive {
        Drive::Css(css) => value_from(key, from, to, css),
        Drive::Spring(config) => spring_from(key, from, to, config),
        Drive::Rn(tension, friction) => rn_spring_from(key, from, to, tension, friction),
    }
}

/// `{mass: 1, tension: 1000, friction: 48}`, the modal layer's spring (`hP` in module 150591).
pub const MODAL_ENTER: SpringConfig = SpringConfig::new(1000., 48.);

/// `{mass: 1, tension: 1200, friction: 80}`, the dialog's spring while it leaves (`o` in module 150591).
pub const MODAL_EXIT: SpringConfig = SpringConfig::new(1200., 80.);

/// How a layer is drawn this frame.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    /// The scrim's opacity factor (0..1).
    pub scrim: f32,
    pub opacity: f32,
    pub scale: f32,
}

/// Marks the layer `id` as leaving: it stays drawn while its exit runs, and `leave_settled` says when it is over.
/// False when it was already leaving.
pub fn begin_leave(id: &str) -> bool {
    let key = key(&("leave", id));
    with(|motion| {
        motion.mark_active();
        motion.leaving.insert(key)
    })
}

pub fn is_leaving(id: &str) -> bool {
    let key = key(&("leave", id));
    with(|motion| motion.leaving.contains(&key))
}

/// Ends the leave of `id`; its next draw is a fresh mount.
pub fn end_leave(id: &str) {
    let key = key(&("leave", id));
    with(|motion| {
        motion.leaving.remove(&key);
    });
}

/// Whether every spring and tween under the given keys has come to rest.
pub fn settled(keys: &[u64]) -> bool {
    with(|motion| {
        let now = motion.frame_now.unwrap_or(0.);
        keys.iter().all(|key| {
            motion.tweens.get(key).is_none_or(|t| t.finished(now))
                && motion.springs.get(key).is_none_or(|s| s.spring.is_done())
                && motion
                    .rn_springs
                    .get(key)
                    .is_none_or(|s| s.spring.is_done())
        })
    })
}

/// The keys `modal_pose` and `layer_pose` use for the layer `id`.
pub fn pose_keys(id: &str) -> [u64; 4] {
    [
        key(&("modal-scrim", id)),
        key(&("modal-body", id)),
        key(&("layer-opacity", id)),
        key(&("layer-scale", id)),
    ]
}

/// A modal's pose. Entering: the scrim fades in on `hP` (module 462824, `useTransition`), the dialog on the same
/// spring after 64 ms, from opacity 0 and `scale(.9)` to 1 and `scale(1)` (module 150591). Leaving: the scrim on
/// `hP` back to 0, the dialog on `{1200, 80}` with no delay. Reduced motion keeps the scale at 1.
pub fn modal_pose(id: &str) -> Pose {
    let leaving = is_leaving(id);
    let target = if leaving { 0. } else { 1. };
    let scrim = enter_between(
        key(&("modal-scrim", id)),
        0.,
        target,
        Drive::Spring(MODAL_ENTER),
    );
    let waited =
        leaving || enter(key(&("modal-wait", id)), Drive::Css(css(64., Ease::Linear))) >= 1.;
    let body = if waited {
        let config = if leaving { MODAL_EXIT } else { MODAL_ENTER };
        enter_between(key(&("modal-body", id)), 0., target, Drive::Spring(config))
    } else {
        with(|motion| motion.mark_active());
        0.
    };
    Pose {
        scrim: scrim.clamp(0., 1.),
        opacity: body.clamp(0., 1.),
        scale: if reduced() { 1. } else { 0.9 + 0.1 * body },
    }
}

impl Pose {
    /// The scrim's colour this frame: `base` (the resting scrim, `#000000b8` or `--background-scrim`) with its alpha
    /// scaled by the scrim factor (the scrim's `opacity` spring, module 462824).
    pub fn dim(&self, base: Rgba) -> Rgba {
        Rgba {
            a: base.a * self.scrim,
            ..base
        }
    }
}

/// The legacy modal root's transition (`EO` in module 935462): `{duration: 200, easing: Easing.out(Easing.quad)}`
/// both ways, no delay.
pub const LEGACY_MODAL: Css = css(200., Ease::OutQuad);

thread_local! {
    /// The layer element ids of the dialogs drawn on the legacy modal root, so their ghosts leave on its curve.
    static LEGACY_LAYERS: RefCell<std::collections::HashSet<String>> =
        RefCell::new(std::collections::HashSet::new());
}

/// A dialog on the legacy modal root (`EO` in module 935462, `ModalRoot` of the older dialogs): the scrim fades in on
/// `hP` like every modal (module 462824 in the modal stack, module 638495), the dialog at once from opacity 0 and
/// `scale(.7)` to 1 and `scale(1)` over 200 ms `Easing.out(quad)`. `subtle` (`animation: SUBTLE`) and reduced motion
/// keep `scale(1)`. `layer` is the id of the dialog's layer element (the root of its `deferred`), so its exit runs the
/// same curve.
pub fn legacy_modal_pose(id: &str, layer: &str, subtle: bool) -> Pose {
    LEGACY_LAYERS.with(|layers| {
        let mut layers = layers.borrow_mut();
        if !layers.contains(layer) {
            layers.insert(layer.to_owned());
        }
    });
    let scrim = enter_between(
        key(&("modal-scrim", id)),
        0.,
        1.,
        Drive::Spring(MODAL_ENTER),
    );
    let body = enter(key(&("legacy-modal-body", id)), Drive::Css(LEGACY_MODAL));
    Pose {
        scrim: scrim.clamp(0., 1.),
        opacity: body.clamp(0., 1.),
        scale: legacy_scale(body, subtle),
    }
}

/// The legacy root's scale for a progress (0 closed, 1 open).
fn legacy_scale(progress: f32, subtle: bool) -> f32 {
    if subtle || reduced() {
        1.
    } else {
        0.7 + 0.3 * progress
    }
}

/// Whether the closed layer named `name` (its element id, as the layer system prints it) is a legacy-root dialog.
fn legacy_layer(name: &str) -> bool {
    LEGACY_LAYERS.with(|layers| {
        layers
            .borrow()
            .iter()
            .any(|layer| name.contains(&format!("\"{layer}\"")))
    })
}

/// `.posed(pose)` on a dialog's box: its opacity and its scale about its centre (`kS` in module 150591), so every
/// dialog that opens over a scrim enters and leaves like the design system modal.
pub trait Posed: gpui::InteractiveElement + Styled + Sized {
    fn posed(self, pose: Pose) -> Self {
        let element = if pose.opacity < 1. {
            self.opacity(pose.opacity)
        } else {
            self
        };
        if pose.scale != 1. {
            element.paint_scale(pose.scale, gpui::point(0.5, 0.5))
        } else {
            element
        }
    }
}

impl<T: gpui::InteractiveElement + Styled + Sized> Posed for T {}

/// A full-window layer's pose (`LayerContainer`, module 596339): it enters from `scale(1.1)` and opacity 0 and leaves
/// back to them, on the layer-stack spring `{tension: 100, friction: 10}`. Reduced motion keeps the scale at 1.
pub fn layer_pose(id: &str) -> Pose {
    let leaving = is_leaving(id);
    let (open, scale) = if leaving { (0., 1.1) } else { (1., 1.) };
    let opacity = enter_between(key(&("layer-opacity", id)), 0., open, Drive::Rn(100., 10.));
    let scaled = enter_between(key(&("layer-scale", id)), 1.1, scale, Drive::Rn(100., 10.));
    Pose {
        scrim: opacity.clamp(0., 1.),
        opacity: opacity.clamp(0., 1.),
        scale: if reduced() { 1. } else { scaled },
    }
}

/// The three official popout entrances (`.fade_98D4cN`, `.scale_98D4cN`, `.translate_98D4cN` in stylesheet 448400,
/// chosen by `animation` on the popout; reduced motion turns every one into a fade, module 44808).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopKind {
    /// Opacity 0 to 1 over .08s ease-out.
    Fade,
    /// `scale(.01)` and opacity 0 to 1 and 1 over .12s ease-out, about the edge that faces the target.
    Scale,
    /// Slides 10 px toward its place over .2s ease-out; opacity does not change.
    Translate,
}

/// The side of the target the popout opens on (`position`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopSide {
    Top,
    Bottom,
    Left,
    Right,
    Center,
}

/// How a popout is drawn this frame.
#[derive(Clone, Copy, Debug)]
pub struct PopPose {
    pub opacity: f32,
    pub scale: f32,
    /// Offset in px.
    pub dx: f32,
    pub dy: f32,
    /// The scale origin as fractions of the popout's box.
    pub origin: (f32, f32),
}

/// The pose of the popout `id` that opened at the first frame it is drawn. All three start 10 ms after the mount
/// (`h.start(10, ...)` in module 44808).
pub fn popout_pose(id: &str, kind: PopKind, side: PopSide) -> PopPose {
    let kind = if reduced() { PopKind::Fade } else { kind };
    let (duration, ease) = match kind {
        PopKind::Fade => (80., Ease::EaseOut),
        PopKind::Scale => (120., Ease::EaseOut),
        PopKind::Translate => (200., Ease::EaseOut),
    };
    let progress = enter(
        key(&("popout", id)),
        Drive::Css(css(duration, ease).delay(10.)),
    );
    let rest = 1. - progress;
    match kind {
        PopKind::Fade => PopPose {
            opacity: progress,
            scale: 1.,
            dx: 0.,
            dy: 0.,
            origin: (0.5, 0.5),
        },
        PopKind::Scale => PopPose {
            opacity: progress,
            scale: 0.01 + 0.99 * progress,
            dx: 0.,
            dy: 0.,
            origin: match side {
                PopSide::Top => (0.5, 1.),
                PopSide::Bottom => (0.5, 0.),
                PopSide::Left => (1., 0.),
                // `.animatorRight` is given `top left` and then `top center`; the later rule wins.
                PopSide::Right => (0.5, 0.),
                // No `.scale.animatorCenter` rule: the initial `transform-origin`, the box's centre.
                PopSide::Center => (0.5, 0.5),
            },
        },
        PopKind::Translate => {
            let (x, y) = match side {
                PopSide::Top | PopSide::Center => (0., -10.),
                PopSide::Bottom => (0., 10.),
                PopSide::Left => (-10., 0.),
                PopSide::Right => (10., 0.),
            };
            PopPose {
                opacity: 1.,
                scale: 1.,
                dx: x * rest,
                dy: y * rest,
                origin: (0.5, 0.5),
            }
        }
    }
}

/// `.popout(id, kind, side)` on a popout's root element: draws it with the official entrance.
pub trait Popout: Styled + gpui::InteractiveElement + Sized {
    fn popout(self, id: &str, kind: PopKind, side: PopSide) -> Self {
        let pose = popout_pose(id, kind, side);
        let mut element = self;
        if pose.opacity < 1. {
            element = element.opacity(pose.opacity);
        }
        if pose.scale != 1. {
            element = element.paint_scale(pose.scale, gpui::point(pose.origin.0, pose.origin.1));
        }
        if pose.dx != 0. || pose.dy != 0. {
            element = element.ml(gpui::px(pose.dx)).mt(gpui::px(pose.dy));
        }
        element
    }
}

impl<T: Styled + gpui::InteractiveElement + Sized> Popout for T {}

// ---------------------------------------------------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------------------------------------------------

/// Records whether the pointer is over the element with this key. Written by the probe below.
pub fn set_hover(key: u64, hovered: bool) {
    with(|motion| {
        let now = motion.clock();
        motion.hovers.insert(key, (hovered, now));
    });
}

/// Whether the pointer is over the element with this key, as the last frame's probe saw it.
pub fn hovered(key: u64) -> bool {
    with(|motion| motion.hovers.get(&key).is_some_and(|hover| hover.0))
}

/// The hover state as a number, 0 or 1, run through a transition: what `:hover { x: ... }` plus `transition` does.
pub fn hover_value(key: u64, css: Css) -> f32 {
    value(key, if hovered(key) { 1. } else { 0. }, css)
}

/// An invisible overlay that reports the pointer over its parent to the store under `key`. The parent must be
/// `relative` (or absolute). It does not take mouse input from what is below it.
pub fn hover_probe(key: u64) -> AnyElement {
    use gpui::{InteractiveElement, StatefulInteractiveElement};
    div()
        .id(gpui::ElementId::Integer(key))
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .on_hover(move |inside, window, _| {
            if hovered(key) != *inside {
                set_hover(key, *inside);
                window.refresh();
            }
        })
        .into_any_element()
}

/// Mixes two colours at `t`, premultiplied.
pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let (a, b) = (premultiplied(a), premultiplied(b));
    let t = t as f64;
    let mut out = [0.; 4];
    for i in 0..4 {
        out[i] = a[i] + (b[i] - a[i]) * t;
    }
    from_premultiplied(out)
}

/// Milliseconds since the control under `key` changed to `on`, while that is less than `within_ms`; `None` at rest.
/// The first state a key is seen in is its rest state, so nothing animates when a control first appears.
pub fn toggled(key: u64, on: bool, within_ms: f64) -> Option<f64> {
    with(|motion| {
        let now = motion.now();
        let entry = motion.toggles.entry(key).or_insert((on, f64::NEG_INFINITY));
        if entry.0 != on {
            *entry = (on, now);
        }
        let elapsed = now - entry.1;
        if elapsed >= 0. && elapsed < within_ms {
            motion.mark_active();
            Some(elapsed)
        } else {
            None
        }
    })
}

/// A CSS keyframe track for one property: `stops` are (offset 0..1, value); `ease` is the animation's timing
/// function, which applies between each pair of keyframes. `t` is the progress of the whole animation.
pub fn keyframes(stops: &[(f32, f32)], ease: Ease, t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    let mut previous = stops[0];
    if t <= previous.0 {
        return previous.1;
    }
    for &next in &stops[1..] {
        if t <= next.0 {
            let span = (next.0 - previous.0).max(f32::EPSILON);
            let eased = ease.apply(((t - previous.0) / span) as f64) as f32;
            return previous.1 + (next.1 - previous.1) * eased;
        }
        previous = next;
    }
    previous.1
}

thread_local! {
    static WINDOW_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Whether the window has focus, as the root last saw it when it began a frame (`app-focused`).
pub fn window_active() -> bool {
    WINDOW_ACTIVE.with(|active| active.get())
}

pub fn set_window_active(active: bool) {
    WINDOW_ACTIVE.with(|cell| cell.set(active));
}

/// Asks for a frame after this one: something that never settles (a looping animation) is on screen.
pub fn keep_animating() {
    with(|motion| motion.mark_active());
}

/// The typing indicator's dot, `typing-dot-pulse_U9Tvvz` (stylesheet of `.dot_U9Tvvz`): 1.2 s, `steps(12)` between
/// keyframes, `infinite`, the dots delayed by -1.2 s, -1.05 s and -0.9 s. Keyframes: opacity .32 and scale .8 at
/// 0%, 50% and 100%, opacity 1 and scale 1 at 25%. Reduced motion slows it to 2.4 s with `steps(24)`. A window
/// without focus (`.stop-animation`) shows every dot at opacity 1.
pub fn typing_dot(index: usize, window_active: bool) -> (f32, f32) {
    if !window_active {
        return (1., 1.);
    }
    let (period, steps, stagger) = if reduced() {
        (2400., 24., 300.)
    } else {
        (1200., 12., 150.)
    };
    let time = now() + period - stagger * index as f64;
    let phase = (time % period) / period;
    keep_animating();
    let (from, to, start, end) = if phase < 0.25 {
        ((0.32, 0.8), (1., 1.), 0., 0.25)
    } else if phase < 0.5 {
        ((1., 1.), (0.32, 0.8), 0.25, 0.5)
    } else {
        ((0.32, 0.8), (0.32, 0.8), 0.5, 1.)
    };
    let stepped = (((phase - start) / (end - start)) * steps).floor() / steps;
    let mix = |a: f64, b: f64| (a + (b - a) * stepped) as f32;
    (mix(from.0, to.0), mix(from.1, to.1))
}

// ---------------------------------------------------------------------------------------------------------------
// The number that rolls
// ---------------------------------------------------------------------------------------------------------------

/// What a rolling number shows this frame (module 114166, the reaction count): the value it moves to, and, while it
/// moves, the value it leaves. `y` is each one's `translateY` as a fraction of the height (-1 above, 0 in place,
/// +1 below).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roll {
    pub current: u64,
    pub current_y: f32,
    pub previous: Option<(u64, f32)>,
}

#[derive(Clone, Copy)]
struct RollState {
    shown: u64,
    run: Option<(u64, f64, bool)>,
    next: Option<u64>,
}

/// `Easing.inOut(Easing.back())` of React Native's `Animated`, `s = 1.70158`.
pub fn back_in_out(t: f64) -> f64 {
    let back = |x: f64| x * x * ((1.70158 + 1.) * x - 1.70158);
    let t = t.clamp(0., 1.);
    if t < 0.5 {
        back(t * 2.) / 2.
    } else {
        1. - back((1. - t) * 2.) / 2.
    }
}

/// The count under `key` as it is now. A change while the window has focus rolls it over 300 ms, the old number
/// leaving upward and the new one arriving from below when it grew, the other way round when it shrank; a change
/// made while the window has no focus is shown at once. A change that arrives mid-roll waits for it to end.
pub fn number_roll(key: u64, value: u64) -> Roll {
    const DURATION: f64 = 300.;
    let focused = window_active();
    thread_local! {
        static ROLLS: RefCell<HashMap<u64, RollState>> = RefCell::new(HashMap::new());
    }
    let now = now();
    ROLLS.with(|rolls| {
        let mut rolls = rolls.borrow_mut();
        let state = rolls.entry(key).or_insert(RollState {
            shown: value,
            run: None,
            next: None,
        });
        if let Some((_, start, _)) = state.run
            && now - start >= DURATION
        {
            state.run = None;
            if let Some(next) = state.next.take()
                && next != state.shown
            {
                state.run = focused.then_some((state.shown, now, next > state.shown));
                state.shown = next;
            }
        }
        if state.run.is_some() {
            if value != state.shown {
                state.next = Some(value);
            }
        } else if value != state.shown {
            state.run = focused.then_some((state.shown, now, value > state.shown));
            state.shown = value;
        }
        match state.run {
            Some((previous, start, grew)) => {
                let t = back_in_out(((now - start) / DURATION).clamp(0., 1.)) as f32;
                keep_animating();
                // The leaving value runs from 0 to -1 (grew) or +1 (shrank); the arriving one from +1 or -1 to 0.
                let (leave, arrive) = if grew { (-1., 1.) } else { (1., -1.) };
                Roll {
                    current: state.shown,
                    current_y: arrive * (1. - t),
                    previous: Some((previous, leave * t)),
                }
            }
            None => Roll {
                current: state.shown,
                current_y: 0.,
                previous: None,
            },
        }
    })
}

/// An icon's `transform: rotate(..)` run through a `transition`: the angle (radians) it shows this frame.
pub fn rotation(key: u64, target: f32, css: Css) -> gpui::Transformation {
    gpui::Transformation::rotate(gpui::radians(value(key, target, css)))
}

/// `transform: rotateX(0 | 180deg)` run through a `transition`, as an icon transformation: seen flat it is a vertical
/// squash by `cos(angle)`, so the chevron closes to a line and opens the other way up.
pub fn flip(key: u64, flipped: bool, css: Css) -> gpui::Transformation {
    let angle = value(key, if flipped { std::f32::consts::PI } else { 0. }, css);
    gpui::Transformation::scale(gpui::size(1., angle.cos()))
}

// ---------------------------------------------------------------------------------------------------------------
// Transitions on an element's style
// ---------------------------------------------------------------------------------------------------------------

/// The style properties a `transition` covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Props {
    pub background: bool,
    pub color: bool,
    pub border_color: bool,
    pub opacity: bool,
}

impl Props {
    pub const BACKGROUND: Props = Props {
        background: true,
        color: false,
        border_color: false,
        opacity: false,
    };
    pub const COLOR: Props = Props {
        background: false,
        color: true,
        border_color: false,
        opacity: false,
    };
    pub const BACKGROUND_COLOR: Props = Props {
        background: true,
        color: true,
        border_color: false,
        opacity: false,
    };
    pub const OPACITY: Props = Props {
        background: false,
        color: false,
        border_color: false,
        opacity: true,
    };
    pub const ALL_COLORS_AND_OPACITY: Props = Props {
        background: true,
        color: true,
        border_color: true,
        opacity: true,
    };
    pub const ALL_COLORS: Props = Props {
        background: true,
        color: true,
        border_color: true,
        opacity: false,
    };
}

fn to_rgba(color: gpui::Hsla) -> Rgba {
    Rgba::from(color)
}

/// Shared stylesheet transitions, written once and applied by kind (`.rule(Rule::X)`). Each one is a rule of the
/// official stylesheets; the file and class are noted on it.
#[derive(Clone, Copy, Debug)]
pub enum Rule {
    /// `button_F7qvhY` (384209): `background-color, color, border-color, opacity 50ms ease-in`, and the `:hover` rule
    /// `.15s ease-out`. Every design-system button variant.
    Button,
    /// `circleIconButton_g8YUxD` (311106): `color, background-color .15s ease-out`.
    CircleIconButton,
    /// `peopleListItem_y75PxA` (249938): `.full-motion` `background-color .2s ease`. Friends list rows.
    PeopleRow,
    /// `listBoxItem_b5PEJH` (448400): `background-color 50ms ease-in`, hover `.15s ease-out`. Select options.
    ListBoxItem,
    /// `reaction_MmKct8` (17256): `background-color, border-color .1s ease`.
    Reaction,
    /// `.mention`, `.channelMention`, `.roleMention` (markup): `background-color, color .05s ease-out`. Applied inside
    /// the inline text engine (`inline.rs`), where a mention is painted, not through `.rule`.
    Mention,
    /// `button_hfM7yP` (the composer's icon buttons): `all .2s`, colour and fill on hover.
    ComposerButton,
    /// `wrapper_UcGqzv` (the text input): `border-color .1s`, which a hover or the focus changes.
    TextField,
    /// `.full-motion .item_6zZYhF` (790853): `background-color .3s ease`, the tab item of the friends bar.
    TabItem,
    /// `categoryItemDefaultCategory_ufks4w` (module 342379): `background-color .1s ease-in-out`, the unicode category
    /// buttons of the emoji picker's rail.
    PickerCategory,
}

impl Rule {
    /// The properties, the transition while the pointer enters, and the one while it leaves.
    pub fn parts(self) -> (Props, Css, Css) {
        match self {
            Rule::Button => (
                Props::ALL_COLORS_AND_OPACITY,
                css(150., Ease::EaseOut),
                css(50., Ease::EaseIn),
            ),
            Rule::CircleIconButton => (
                Props::BACKGROUND_COLOR,
                css(150., Ease::EaseOut),
                css(150., Ease::EaseOut),
            ),
            Rule::PeopleRow => {
                let rule = css(200., Ease::Ease).full_motion();
                (Props::BACKGROUND, rule, rule)
            }
            Rule::ListBoxItem => (
                Props::BACKGROUND,
                css(150., Ease::EaseOut),
                css(50., Ease::EaseIn),
            ),
            Rule::Reaction => (
                Props {
                    background: true,
                    color: false,
                    border_color: true,
                    opacity: false,
                },
                css(100., Ease::Ease),
                css(100., Ease::Ease),
            ),
            Rule::Mention => (
                Props::BACKGROUND_COLOR,
                css(50., Ease::EaseOut),
                css(50., Ease::EaseOut),
            ),
            Rule::ComposerButton => (
                Props::BACKGROUND_COLOR,
                css(200., Ease::Ease),
                css(200., Ease::Ease),
            ),
            Rule::TextField => (
                Props {
                    background: false,
                    color: false,
                    border_color: true,
                    opacity: false,
                },
                css(100., Ease::Ease),
                css(100., Ease::Ease),
            ),
            Rule::PickerCategory => (
                Props::BACKGROUND_COLOR,
                css(100., Ease::EaseInOut),
                css(100., Ease::EaseInOut),
            ),
            Rule::TabItem => {
                let rule = css(300., Ease::Ease).full_motion();
                (Props::BACKGROUND, rule, rule)
            }
        }
    }
}

/// CSS `transition` for an element: whatever `hover`, `active` and the app's own state do to the covered
/// properties of its style then arrives over `css` instead of at once. The element needs an id.
pub trait Transitions: gpui::InteractiveElement + Sized {
    fn transition(self, props: Props, css: Css) -> Self {
        self.transition_pair(props, css, css)
    }

    /// One shared stylesheet rule, see [`Rule`].
    fn rule(self, rule: Rule) -> Self {
        let (props, hovering, resting) = rule.parts();
        self.transition_pair(props, hovering, resting)
    }

    /// A transition that differs by direction, as the stylesheets write it: the `:hover` rule carries the
    /// duration used while the pointer enters (`hovering`), the base rule the one used while it leaves (`resting`).
    fn transition_pair(self, props: Props, hovering: Css, resting: Css) -> Self {
        self.style_transition(move |style, id, hovered, _| {
            let css = if hovered { hovering } else { resting };
            if props.background {
                let target = match style.background.as_ref().and_then(|fill| fill.color()) {
                    Some(fill) => fill.as_solid().map(to_rgba),
                    None => None,
                };
                let wanted = target.unwrap_or(Rgba {
                    r: 0.,
                    g: 0.,
                    b: 0.,
                    a: 0.,
                });
                let shown = color(id ^ 0x01, wanted, css);
                if target.is_some() || shown.a > 0.001 {
                    style.background = Some(gpui::Fill::Color(shown.into()));
                }
            }
            if props.color
                && let Some(text) = style.text.color
            {
                style.text.color = Some(color(id ^ 0x02, to_rgba(text), css).into());
            }
            if props.border_color
                && let Some(border) = style.border_color
            {
                style.border_color = Some(color(id ^ 0x03, to_rgba(border), css).into());
            }
            if props.opacity {
                let shown = value(id ^ 0x04, style.opacity.unwrap_or(1.), css);
                style.opacity = Some(shown);
            }
        })
    }
}

impl<T: gpui::InteractiveElement + Sized> Transitions for T {}

// ---------------------------------------------------------------------------------------------------------------
// Presence
// ---------------------------------------------------------------------------------------------------------------

/// What drives the progress of a `Presence`.
#[derive(Clone, Copy, Debug)]
pub enum Drive {
    Css(Css),
    Spring(SpringConfig),
    /// A layer-stack spring (`{tension, friction}`, see `RnSpring`).
    Rn(f64, f64),
}

/// Keeps a value mounted until its exit animation has finished. `set(Some(v))` shows `v`, `set(None)` starts the
/// exit; `frame` gives the retained value and the progress (0 hidden, 1 shown) until the exit is over.
pub struct Presence<T> {
    key: u64,
    drive: Drive,
    shown: Option<T>,
    wanted: bool,
}

impl<T: Clone> Presence<T> {
    pub fn new<H: Hash + ?Sized>(key_source: &H, drive: Drive) -> Self {
        Self {
            key: key(key_source),
            drive,
            shown: None,
            wanted: false,
        }
    }

    pub fn set(&mut self, value: Option<T>) {
        let was = self.wanted;
        match value {
            Some(value) => {
                self.shown = Some(value);
                self.wanted = true;
            }
            None => self.wanted = false,
        }
        // The change of target starts now, not at the next read.
        if was != self.wanted && self.shown.is_some() {
            self.progress();
        }
    }

    fn progress(&self) -> f32 {
        let target = if self.wanted { 1. } else { 0. };
        match self.drive {
            Drive::Css(css) => value_from(self.key, 0., target, css),
            Drive::Spring(config) => spring_from(self.key, 0., target, config),
            Drive::Rn(tension, friction) => rn_spring_from(self.key, 0., target, tension, friction),
        }
    }

    pub fn is_mounted(&self) -> bool {
        self.shown.is_some()
    }

    /// The value to draw this frame and its progress, or `None` once it has left.
    pub fn frame(&mut self) -> Option<(T, f32)> {
        let value = self.shown.clone()?;
        let progress = self.progress();
        if !self.wanted && self.exited(progress) {
            self.shown = None;
            with(|motion| {
                motion.tweens.remove(&self.key);
                motion.springs.remove(&self.key);
            });
            return None;
        }
        Some((value, progress))
    }

    fn exited(&self, progress: f32) -> bool {
        let at_rest = with(|motion| {
            let tween_done = motion
                .tweens
                .get(&self.key)
                .is_none_or(|tween| tween.finished(motion.frame_now.unwrap_or(0.)));
            let spring_done = motion
                .springs
                .get(&self.key)
                .is_none_or(|slot| slot.spring.is_done());
            tween_done && spring_done
        });
        at_rest && progress <= 0.0001
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The redraw driver
// ---------------------------------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------------------------------
// Closing layers
// ---------------------------------------------------------------------------------------------------------------

/// A layer that closed and is drawn again, from a copy of what it last painted, while its exit runs. The official
/// layer system keeps a leaving dialog mounted (`leave` in its transition); here the state is gone the moment the
/// dialog closes, by any route, so the copy stands in for it.
struct Ghost {
    key: u64,
    snapshot: gpui::LayerSnapshot,
    /// The dialog's centre (scale origin), or the window's for a full-window layer.
    centre: gpui::Point<gpui::Pixels>,
    /// A full-window layer (`LayerContainer`) leaves to `scale(1.1)`; a dialog to `scale(.9)`.
    layer: bool,
    /// A `SUBTLE` modal (the media viewer, module 2323) only fades (`kS` in module 150591 and `EO` in module 935462
    /// keep `scale(1)`).
    subtle: bool,
    /// A dialog on the legacy root (module 935462): it leaves over 200 ms `Easing.out(quad)` to `scale(.7)`.
    legacy: bool,
}

thread_local! {
    static LAST_GHOST: RefCell<String> = const { RefCell::new(String::new()) };
    static GHOSTS: RefCell<Vec<Ghost>> = const { RefCell::new(Vec::new()) };
}

/// Adds the layers that closed in this frame to the ghosts, draws every ghost at its place on the exit curve, and
/// drops the ones that have finished. Runs at the end of the frame, after every layer has painted.
fn ghosts_step(window: &mut gpui::Window) {
    let painted: Vec<u64> = window
        .painted_layers()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    let closed = window.closed_layers();
    GHOSTS.with(|ghosts| {
        let mut ghosts = ghosts.borrow_mut();
        // A layer that opened again is not leaving.
        ghosts.retain(|ghost| !painted.contains(&ghost.key));
        for (key, id) in closed {
            if ghosts.iter().any(|ghost| ghost.key == key) {
                continue;
            }
            let Some(snapshot) = window.snapshot_layer(key) else {
                continue;
            };
            let (scrim, content) = snapshot.scrim_and_content(window);
            LAST_GHOST.with(|last| {
                *last.borrow_mut() = format!(
                    "{id:?} scrim={scrim} content={content:?} ops={}",
                    snapshot.describe()
                )
            });
            if !scrim {
                continue;
            }
            let viewport = window.viewport_size();
            let name = format!("{id:?}");
            // Server Settings and Channel Settings are layers of the layer stack; User Settings is a modal whose
            // dialog is centred in the window (the window controls painted with it are not part of the dialog).
            let layer = name.contains("server-settings") || name.contains("channel-settings");
            let centred = layer || name.contains("settings-layer");
            let centre = match (centred, content) {
                (false, Some(content)) => content.center(),
                _ => gpui::point(viewport.width / 2., viewport.height / 2.),
            };
            ghosts.push(Ghost {
                key,
                snapshot,
                centre,
                layer,
                subtle: subtle_layer(&name),
                legacy: legacy_layer(&name),
            });
        }
        let reduced = reduced();
        ghosts.retain(|ghost| {
            let progress_key = key(&("ghost", ghost.key));
            let scrim_key = key(&("ghost-scrim", ghost.key));
            let (progress, scrim) = if ghost.layer {
                let progress = rn_spring_from(progress_key, 1., 0., 100., 10.);
                (progress, progress)
            } else if ghost.legacy {
                // The legacy root leaves on its 200 ms tween; the scrim on `hP` as for every modal.
                let progress = value_from(progress_key, 1., 0., LEGACY_MODAL);
                let scrim = spring_from(scrim_key, 1., 0., MODAL_ENTER);
                (progress, scrim)
            } else {
                // The dialog leaves on `{1200, 80}` (`kS`, module 150591) while the scrim fades out on `hP`
                // (`leave: {opacity: 0}` with `config: hP`, module 462824).
                ghost_leave(progress_key, scrim_key)
            };
            let done = settled(&[progress_key, scrim_key]);
            if done && progress <= 0.001 && scrim <= 0.001 {
                with(|motion| {
                    for key in [progress_key, scrim_key] {
                        motion.springs.remove(&key);
                        motion.rn_springs.remove(&key);
                        motion.tweens.remove(&key);
                    }
                });
                return false;
            }
            let opacity = progress.clamp(0., 1.);
            let scale = if reduced || ghost.subtle {
                1.
            } else if ghost.layer {
                1.1 - 0.1 * progress
            } else if ghost.legacy {
                legacy_scale(progress, false)
            } else {
                0.9 + 0.1 * progress
            };
            window.paint_layer_snapshot_split(
                &ghost.snapshot,
                scrim.clamp(0., 1.),
                opacity,
                scale,
                ghost.centre,
            );
            true
        });
    });
}

/// A closed dialog's two leave values this frame, from 1 to 0: the dialog's (on `MODAL_EXIT`) and its scrim's (on
/// `MODAL_ENTER`, the scrim's config both ways in module 462824).
fn ghost_leave(dialog_key: u64, scrim_key: u64) -> (f32, f32) {
    (
        spring_from(dialog_key, 1., 0., MODAL_EXIT),
        spring_from(scrim_key, 1., 0., MODAL_ENTER),
    )
}

/// Whether the closed layer named `name` (its element id, as the layer system prints it) is a `SUBTLE` modal, which
/// leaves without a scale: the media viewer (`animation: o.WM.SUBTLE` in module 2323).
fn subtle_layer(name: &str) -> bool {
    name.contains("\"lightbox\"")
}

/// How many layers are leaving (for the harness).
pub fn ghosts() -> usize {
    GHOSTS.with(|ghosts| ghosts.borrow().len())
}

/// Goes last in the root's tree. After the frame is laid out it asks for the next one if something moves, and
/// asks for nothing otherwise. A frozen clock never asks (the harness draws by hand). What moved while no view was
/// known to be building (`leave_view`) is redrawn in every region as well.
pub fn driver() -> AnyElement {
    // Deferred with the highest priority: it paints after every element whose style moves, so it sees them all.
    gpui::deferred(
        canvas(
            |_, _, _| (),
            |_, _, window, _| {
                ghosts_step(window);
                let root = window.current_view();
                let (want, everywhere) = with(|motion| {
                    let live = motion.frozen.is_none();
                    let everywhere = motion.active && live;
                    let want = (everywhere || motion.active_views.contains(&root)) && live;
                    if want {
                        motion.requested += 1;
                    }
                    (want, everywhere)
                });
                if want {
                    window.request_animation_frame();
                }
                if everywhere {
                    window.on_next_frame(|_, cx| super::regions::notify_all(cx));
                }
            },
        )
        .absolute()
        .size_0(),
    )
    .with_priority(usize::MAX)
    .into_any_element()
}

/// The driver of a region: after the region is laid out it asks for the region's next frame if something in it
/// moves. A region that does not move is not asked for, and its previous frame is reused.
pub fn region_driver() -> AnyElement {
    canvas(
        |_, _, _| (),
        |_, _, window, _| {
            let view = window.current_view();
            let want = with(|motion| {
                let want = motion.active_views.contains(&view) && motion.frozen.is_none();
                if want {
                    motion.requested += 1;
                }
                want
            });
            if want {
                window.request_animation_frame();
            }
        },
    )
    .absolute()
    .size_0()
    .into_any_element()
}

// ---------------------------------------------------------------------------------------------------------------
// The system's reduced-motion setting
// ---------------------------------------------------------------------------------------------------------------

/// The operating system's "animation effects" switch turned off (Windows: `SPI_GETCLIENTAREAANIMATION`).
#[cfg(windows)]
pub fn system_prefers_reduced_motion() -> bool {
    // Capture builds can stand in for the operating system's value: `GRAVECORD_SYSTEM_REDUCED_MOTION=1` or `0`.
    #[cfg(feature = "capture")]
    if let Ok(forced) = std::env::var("GRAVECORD_SYSTEM_REDUCED_MOTION") {
        return forced == "1";
    }
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut enabled: i32 = 1;
    // SAFETY: the pointer is to a live `i32`, the size SPI_GETCLIENTAREAANIMATION writes (a BOOL).
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut enabled as *mut i32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && enabled == 0
}

#[cfg(not(windows))]
pub fn system_prefers_reduced_motion() -> bool {
    false
}

/// The official decision (module 775602): "Sync with computer setting" on follows the system, otherwise the app's
/// own switch.
pub fn decide_reduced(sync_with_system: bool, own_switch: bool, system: bool) -> bool {
    if sync_with_system { system } else { own_switch }
}

/// The system's setting, read at most once a second.
pub fn cached_system_preference() -> bool {
    thread_local! {
        static CACHE: RefCell<Option<(Instant, bool)>> = const { RefCell::new(None) };
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((at, value)) = *cache
            && at.elapsed().as_secs() < 1
        {
            return value;
        }
        let value = system_prefers_reduced_motion();
        *cache = Some((Instant::now(), value));
        value
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Capture harness
// ---------------------------------------------------------------------------------------------------------------

/// The `motion ...` harness steps (see `harness.rs`).
#[cfg(feature = "capture")]
pub(crate) async fn step(
    words: &[&str],
    window: gpui::AnyWindowHandle,
    root: &gpui::Entity<super::Root>,
    cx: &mut gpui::AsyncApp,
) -> String {
    fn draw(cx: &mut gpui::AsyncApp, window: gpui::AnyWindowHandle) {
        cx.update(|cx| {
            let _ = window.update(cx, |_, window, cx| {
                super::regions::prepare_hand_draw(window, cx);
                window.draw(cx).clear(cx);
            });
        });
    }
    match words {
        ["clock", "real"] => {
            freeze(None);
            root.update(cx, |_, cx| cx.notify());
            "motion clock: running".to_owned()
        }
        ["clock", ms] => {
            // `+N@R`: N ms after now, up to the next multiple of R ms, so looping animations stand in the same phase in
            // every run (the plugins-off check lets every transition end, `xtask/src/plugins_off.rs`).
            let (text, align) = match ms.split_once('@') {
                Some((text, align)) => (text, align.parse::<f64>().ok().filter(|r| *r > 0.)),
                None => (*ms, None),
            };
            let ahead = text.strip_prefix('+');
            let Ok(ms) = ahead.unwrap_or(text).parse::<f64>() else {
                return format!("motion clock: not a number: {ms}");
            };
            let ms = if ahead.is_some() { now() + ms } else { ms };
            let ms = align.map_or(ms, |align| (ms / align).ceil() * align);
            // What the last input started begins at the old time, then the clock moves in 16 ms frames, as a window
            // draws them (a spring takes at most 64 ms of one frame, as the official one does).
            draw(cx, window);
            let from = if is_frozen() { now() } else { ms.min(now()) };
            let mut at = from;
            while at + 16. < ms {
                at += 16.;
                freeze(Some(at));
                draw(cx, window);
            }
            freeze(Some(ms));
            draw(cx, window);
            format!("motion clock: {ms} ms")
        }
        ["shot", path] => {
            let path = std::path::PathBuf::from(path);
            cx.update(|cx| crate::write_capture(cx, window, &path));
            format!("motion shot: {} at {} ms", path.display(), now())
        }
        ["reduced", mode] => {
            force_reduced(match *mode {
                "on" => Some(true),
                "off" => Some(false),
                _ => None,
            });
            draw(cx, window);
            format!("motion reduced: {mode}")
        }
        ["idle", ms] => {
            let ms = ms.parse::<u64>().unwrap_or(5000);
            let renders = || crate::perf::RENDERS.load(std::sync::atomic::Ordering::Relaxed);
            let blinks = || crate::editor::BLINK_FRAMES.load(std::sync::atomic::Ordering::Relaxed);
            let (renders_before, frames_before, blinks_before, cpu_before) = (
                renders(),
                frames_requested(),
                blinks(),
                crate::perf::cpu_ms(),
            );
            cx.background_executor()
                .timer(std::time::Duration::from_millis(ms))
                .await;
            format!(
                "motion idle {ms} ms: renders={} frames_requested={} caret_blinks={} cpu_ms={} active={}",
                renders() - renders_before,
                frames_requested() - frames_before,
                blinks() - blinks_before,
                crate::perf::cpu_ms().saturating_sub(cpu_before),
                is_active()
            )
        }
        // Opens the Log Out dialog, a plain confirm, as the stand-in for every dialog.
        ["modal", "open"] => {
            root.update(cx, |root, cx| {
                root.logout_confirm = true;
                cx.notify();
            });
            "motion modal: open".to_owned()
        }
        ["modal", "close"] => {
            root.update(cx, |root, cx| {
                root.logout_confirm = false;
                cx.notify();
            });
            "motion modal: close".to_owned()
        }
        // Opens and closes Server Settings of the open server, the stand-in for every full-window layer.
        ["layer", "open"] => {
            root.update(cx, |root, cx| {
                if let Some(guild) = root.guild {
                    root.open_server_settings(guild, cx);
                }
                cx.notify();
            });
            "motion layer: open".to_owned()
        }
        ["layer", "close"] => {
            root.update(cx, |root, cx| {
                root.server_settings = None;
                cx.notify();
            });
            "motion layer: close".to_owned()
        }
        ["older", mode] => {
            root.update(cx, |root, cx| {
                if let Some(channel) = root.channel {
                    crate::ui::unread::set_window_detached(channel, *mode == "on");
                }
                cx.notify();
            });
            format!("motion older: {mode}")
        }
        // A stand-in voice member (the signed-in user in the first voice channel of the open server), speaking or not.
        ["speaking", mode] => {
            root.update(cx, |root, cx| {
                let (Some(guild), Some(me)) = (
                    root.guild,
                    root.stores.current_user.as_ref().map(|user| user.id),
                ) else {
                    return;
                };
                let voice = root.current_guild().and_then(|guild| {
                    guild
                        .channels
                        .iter()
                        .find(|channel| channel.kind == dc_state::ChannelKind::Voice)
                        .map(|channel| channel.id)
                });
                let Some(voice) = voice else {
                    return;
                };
                let list = root.stores.voice_states.entry(guild).or_default();
                if !list.iter().any(|state| state.user_id == me) {
                    list.push(dc_state::VoiceState {
                        user_id: me,
                        channel_id: voice,
                        ..Default::default()
                    });
                }
                if *mode == "on" {
                    root.speaking.insert(me);
                } else {
                    root.speaking.remove(&me);
                }
                cx.notify();
            });
            format!("motion speaking: {mode}")
        }
        ["toast", "show"] => {
            root.update(cx, |root, cx| root.show_app_toast("Copied!", cx));
            "motion toast: show".to_owned()
        }
        ["toast", "hide"] => {
            root.update(cx, |root, cx| {
                root.app_toast = None;
                cx.notify();
            });
            "motion toast: hide".to_owned()
        }
        // Opens one dialog straight away, for its entrance and leave frames. Nothing is confirmed or sent.
        ["dialog", name] => cx
            .update(|cx| {
                window.update(cx, |_, window, cx| {
                    root.update(cx, |root, cx| {
                        let note = open_dialog(root, name, window, cx);
                        cx.notify();
                        note
                    })
                })
            })
            .unwrap_or_else(|_| "no window".to_owned()),
        ["state"] => format!(
            "motion state: last_ghost={} ghosts={} now={:.1} frozen={} active={} reduced={} frames_requested={}",
            LAST_GHOST.with(|last| last.borrow().clone()),
            ghosts(),
            now(),
            is_frozen(),
            is_active(),
            reduced(),
            frames_requested()
        ),
        other => format!("motion: unknown step {other:?}"),
    }
}

/// The `motion dialog <name>` step: opens the dialog `name` as its button would, with the first other person known or
/// the newest fitting message of the open channel. Returns what it did.
#[cfg(feature = "capture")]
fn open_dialog(
    root: &mut super::Root,
    name: &str,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<super::Root>,
) -> String {
    let me = root.stores.current_user.as_ref().map(|me| me.id);
    let other = root
        .stores
        .users
        .keys()
        .copied()
        .filter(|id| Some(*id) != me)
        .min();
    let messages = root
        .channel
        .and_then(|channel| root.stores.messages.get(&channel))
        .cloned()
        .unwrap_or_default();
    match name {
        "full-profile" | "report" | "ignore" | "block" => {
            let Some(user) = other else {
                return "dialog: no other person known".to_owned();
            };
            match name {
                "full-profile" => root.open_full_profile(user, window, cx),
                "report" => root.open_report_user(user, window, cx),
                "ignore" => root.remediation = Some((user, super::remediation::Remedy::Ignore)),
                _ => root.remediation = Some((user, super::remediation::Remedy::Block)),
            }
            format!("dialog: {name} for {user}")
        }
        "badge" => {
            root.open_badge_modal(me, None, None, true, cx);
            "dialog: badge".to_owned()
        }
        "follow" => match root.channel {
            Some(channel) => {
                root.open_follow(channel, cx);
                "dialog: follow".to_owned()
            }
            None => "dialog: no channel".to_owned(),
        },
        "shelf" => {
            root.open_activity_shelf(window, cx);
            "dialog: activity shelf".to_owned()
        }
        "editor" => root.script_profile_editor(&["open"], cx),
        "styles" => root.script_profile_editor(&["styles"], cx),
        "decoration" => root.script_profile_editor(&["pick", "decoration"], cx),
        "lightbox" => {
            let found = messages.iter().rev().find_map(|message| {
                message
                    .attachments
                    .iter()
                    .find(|attachment| attachment.is_image())
                    .map(|attachment| (message.clone(), attachment.id))
            });
            match found {
                Some((message, id)) => {
                    root.open_lightbox(&message, &format!("attachment:{id}"), cx);
                    format!("dialog: lightbox on {id}")
                }
                None => "dialog: no picture in the channel".to_owned(),
            }
        }
        "file" => {
            let attachment = dc_state::Attachment {
                id: 1,
                filename: "notes.txt".to_owned(),
                title: None,
                url: String::new(),
                proxy_url: String::new(),
                content_type: Some("text/plain".to_owned()),
                width: None,
                height: None,
                size: 64,
                flags: 0,
                description: None,
                duration_secs: None,
                waveform: None,
                ..dc_state::Attachment::default()
            };
            root.texts.insert(
                super::files::text_key(&attachment),
                "first line of the file\nsecond line of the file".to_owned(),
            );
            root.file_expanded = Some(attachment);
            "dialog: file expanded".to_owned()
        }
        "leaving" => {
            root.follow_link("https://example.com/", "example", cx);
            "dialog: leaving".to_owned()
        }
        "forward" => match messages.last().cloned() {
            Some(message) => {
                root.open_forward(message, window, cx);
                "dialog: forward".to_owned()
            }
            None => "dialog: no message".to_owned(),
        },
        "golive" => {
            root.open_go_live(cx);
            format!("dialog: go live (call={})", root.call.is_some())
        }
        "nitro" => {
            root.open_purchase(false, cx);
            "dialog: nitro purchase".to_owned()
        }
        "events" => {
            root.open_guild_events(cx);
            "dialog: events".to_owned()
        }
        "perks" => {
            root.open_perks(super::server_boost::Perks::Banner, cx);
            format!(
                "dialog: boost perks (server settings={})",
                root.server_settings.is_some()
            )
        }
        "risky" => match root.server_settings.as_mut() {
            Some(open) => {
                open.safety.risky_open = true;
                "dialog: risky".to_owned()
            }
            None => "dialog: open server-settings safetysetup first".to_owned(),
        },
        "crunchyroll" | "playstation" | "xbox" | "bluesky" | "domain" => {
            let kind = match name {
                "crunchyroll" => "crunchyroll",
                "playstation" => "playstation",
                "xbox" => "xbox",
                "bluesky" => "bluesky",
                _ => "domain",
            };
            root.open_connection_dialog(kind, cx);
            format!(
                "dialog: connect {kind} (settings={})",
                root.settings.is_some()
            )
        }
        other => format!("dialog: unknown {other}"),
    }
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    MOTION.with(|motion| *motion.borrow_mut() = Motion::default());
}

#[cfg(test)]
mod tests;
