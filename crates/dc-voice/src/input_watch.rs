//! Whether the microphone delivers any input at all, as the official `InputWatcher` (module 754712, class `el`)
//! decides it: the native engine reports `Silence(isSilent)`, and the watcher confirms each change after a delay, 1.5 s
//! before it reports input and 5 s before it reports silence (`stateChangeTimeout.start(t ? 1500 : 5e3, ...)`, one
//! timer that each report restarts). `inputDetected` stays unknown (`undefined`) until the first confirmation.
//!
//! The native `Silence` event is not readable; here a frame is silent when its peak is no more than [`SILENCE_PEAK`]
//! (about -80 dBFS: a muted or disconnected microphone delivers exact zeros or a few bits of dither, a live one
//! sits well above).

use std::time::{Duration, Instant};

/// How long input has to last before it is reported (`t ? 1500`).
pub const DETECT_AFTER: Duration = Duration::from_millis(1500);
/// How long silence has to last before it is reported (`5e3`).
pub const SILENCE_AFTER: Duration = Duration::from_millis(5000);
/// A frame whose loudest sample is no more than this is silence.
pub const SILENCE_PEAK: f32 = 1e-4;

#[derive(Debug, Default)]
pub struct InputWatcher {
    /// The confirmation in flight: when it lands and what it says (true: input detected).
    pending: Option<(Instant, bool)>,
    /// `inputDetected`: None until confirmed.
    detected: Option<bool>,
    /// `getInputDetectedThisConnection()`: input was confirmed at least once since the connection began.
    this_connection: bool,
    /// The last frame's silence, so only changes become `Silence` events.
    silent: Option<bool>,
}

impl InputWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// One microphone frame's loudest sample. A change of silence is a `Silence` event.
    pub fn frame(&mut self, peak: f32, now: Instant) {
        let silent = peak.is_nan() || peak <= SILENCE_PEAK;
        if self.silent != Some(silent) {
            self.silent = Some(silent);
            self.handle_silence(silent, now);
        }
    }

    /// `handleSilence(e)`: (re)starts the single timer for the opposite state.
    fn handle_silence(&mut self, silent: bool, now: Instant) {
        let input = !silent;
        let delay = if input { DETECT_AFTER } else { SILENCE_AFTER };
        self.pending = Some((now + delay, input));
    }

    /// Lets a due confirmation land. Returns the new `inputDetected` when it changed from what was reported.
    pub fn tick(&mut self, now: Instant) -> Option<bool> {
        let (at, input) = self.pending?;
        if now < at {
            return None;
        }
        self.pending = None;
        let changed = self.detected != Some(input);
        self.detected = Some(input);
        if input {
            self.this_connection = true;
        }
        changed.then_some(input)
    }

    /// The microphone starts afresh (a new call, unmuting): nothing is known again.
    pub fn reset(&mut self) {
        self.pending = None;
        self.detected = None;
        self.silent = None;
    }

    pub fn detected(&self) -> Option<bool> {
        self.detected
    }

    pub fn detected_this_connection(&self) -> bool {
        self.this_connection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn input_is_reported_after_one_and_a_half_seconds() {
        let start = Instant::now();
        let mut watcher = InputWatcher::new();
        watcher.frame(0.2, start);
        assert_eq!(watcher.tick(at(start, 1499)), None);
        assert_eq!(watcher.detected(), None, "unknown until confirmed");
        assert_eq!(watcher.tick(at(start, 1500)), Some(true));
        assert_eq!(watcher.detected(), Some(true));
        assert!(watcher.detected_this_connection());
        assert_eq!(watcher.tick(at(start, 9000)), None, "reported once");
    }

    #[test]
    fn silence_is_reported_after_five_seconds() {
        let start = Instant::now();
        let mut watcher = InputWatcher::new();
        watcher.frame(0.0, start);
        assert_eq!(watcher.tick(at(start, 4999)), None);
        assert_eq!(watcher.tick(at(start, 5000)), Some(false));
        assert_eq!(watcher.detected(), Some(false));
        assert!(!watcher.detected_this_connection());
    }

    #[test]
    fn a_change_restarts_the_single_timer() {
        let start = Instant::now();
        let mut watcher = InputWatcher::new();
        watcher.frame(0.0, start);
        // Input comes back at 3 s: the 5 s silence report is dropped and 1.5 s of input is awaited.
        watcher.frame(0.3, at(start, 3000));
        assert_eq!(watcher.tick(at(start, 4499)), None);
        assert_eq!(watcher.tick(at(start, 4500)), Some(true));
        assert_eq!(watcher.detected(), Some(true));
    }

    #[test]
    fn frames_of_the_same_state_do_not_restart_the_timer() {
        let start = Instant::now();
        let mut watcher = InputWatcher::new();
        for step in 0..100 {
            watcher.frame(0.0, at(start, step * 20));
        }
        // Silence began at 0; the report is due at 5 s although frames kept coming.
        assert_eq!(watcher.tick(at(start, 5000)), Some(false));
    }

    #[test]
    fn the_noise_floor_of_a_live_microphone_is_input() {
        let start = Instant::now();
        let mut watcher = InputWatcher::new();
        watcher.frame(0.001, start);
        assert_eq!(watcher.tick(at(start, 1500)), Some(true));
        let mut dead = InputWatcher::new();
        dead.frame(0.00003, start);
        assert_eq!(dead.tick(at(start, 5000)), Some(false));
    }

    #[test]
    fn reset_forgets_what_was_known_but_not_that_input_was_heard() {
        let start = Instant::now();
        let mut watcher = InputWatcher::new();
        watcher.frame(0.5, start);
        watcher.tick(at(start, 1500));
        watcher.reset();
        assert_eq!(watcher.detected(), None);
        assert!(
            watcher.detected_this_connection(),
            "per connection, not per unmute"
        );
    }
}
