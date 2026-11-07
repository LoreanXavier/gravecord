//! Display-only speaking activity; never gates or changes transmitted audio.
//!
//! Adapted from Serein `crates/discord-voice/src/activity.rs` (`hold`, `hold_at`),
//! MIT OR Apache-2.0. The input gate and preview meter were dropped.

/// Serein's local indicator threshold, -45 dBFS. Detects sound, not speech.
pub(crate) const LOCAL_THRESHOLD_DB: i16 = -45;

/// Remote speakers: about -45 dBFS mean energy over one 960-sample frame.
pub(crate) fn hold(energy: f32, previous: u8) -> u8 {
    if energy.is_finite() && energy > 960.0 * 0.000_031_623 {
        10 // 200 ms at the transport's 20 ms cadence.
    } else {
        previous.saturating_sub(1)
    }
}

/// Local microphone against a threshold in dBFS.
pub(crate) fn hold_at(energy: f32, previous: u8, threshold: i16) -> u8 {
    if energy.is_finite()
        && energy > 960.0 * 10.0_f32.powf(f32::from(threshold.clamp(-80, 0)) / 10.0)
    {
        10
    } else {
        previous.saturating_sub(1)
    }
}

pub(crate) fn energy(frame: &crate::Frame) -> f32 {
    frame.iter().filter(|s| s.is_finite()).map(|s| s * s).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_holds_for_ten_frames() {
        let loud = energy(&[0.1; 960]);
        let quiet = energy(&[0.001; 960]);
        assert_eq!(hold(loud, 0), 10);
        assert_eq!(hold(quiet, 10), 9);
        assert_eq!(hold(f32::NAN, 1), 0);
        assert_eq!(hold_at(loud, 0, LOCAL_THRESHOLD_DB), 10);
        assert_eq!(hold_at(quiet, 3, LOCAL_THRESHOLD_DB), 2);
    }
}
