//! The start's timeline: the first moment each stage of the way from the process start to a complete first screen was
//! reached (the window, the cache read, the gateway's READY, the open channel's messages, its pictures). Each stage is
//! kept once, with the milliseconds since the process started, and written as a debug event (`target: "gravecord::start"`)
//! that names only the stage and the time: no ids, no content. The capture harness prints them in its `perf start` line.

use std::sync::Mutex;
use std::time::Instant;

static STAGES: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());
static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Called first thing in `main`: the origin of the times of a build that cannot ask the system for the process's
/// creation time.
pub fn init() {
    let _ = ORIGIN.get_or_init(Instant::now);
}

/// Milliseconds since the process started.
pub fn now_ms() -> u64 {
    #[cfg(feature = "capture")]
    {
        crate::perf::since_process_start_ms()
    }
    #[cfg(not(feature = "capture"))]
    {
        ORIGIN
            .get_or_init(Instant::now)
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}

/// Records `stage` the first time it is reached; later calls keep the first time.
pub fn mark(stage: &'static str) {
    let Ok(mut stages) = STAGES.lock() else {
        return;
    };
    if stages.iter().any(|(name, _)| *name == stage) {
        return;
    }
    let ms = now_ms();
    stages.push((stage, ms));
    drop(stages);
    tracing::debug!(target: "gravecord::start", stage, ms, "start stage");
}

/// Records `stage` as reached `ago` before now (a stage timed inside a network crate, `dc_proto::replay::stage`).
pub fn mark_ago(stage: &'static str, ago: std::time::Duration) {
    let Ok(mut stages) = STAGES.lock() else {
        return;
    };
    if stages.iter().any(|(name, _)| *name == stage) {
        return;
    }
    let ago: u64 = ago.as_millis().try_into().unwrap_or(u64::MAX);
    let ms = now_ms().saturating_sub(ago);
    stages.push((stage, ms));
    drop(stages);
    tracing::debug!(target: "gravecord::start", stage, ms, "start stage");
}

/// Records how long the first `work` of its kind took (a READY applied on the UI thread, say), as `dur_<work>`.
pub fn took(work: &'static str, since: Instant) {
    let ms = since.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    let Ok(mut stages) = STAGES.lock() else {
        return;
    };
    if stages.iter().any(|(name, _)| *name == work) {
        return;
    }
    stages.push((work, ms));
    drop(stages);
    tracing::debug!(target: "gravecord::start", work, ms, "start work took");
}

/// Whether `stage` has been reached.
pub fn reached(stage: &'static str) -> bool {
    STAGES
        .lock()
        .is_ok_and(|stages| stages.iter().any(|(name, _)| *name == stage))
}

/// Every stage so far as `cp_st_<stage>_ms=<ms>` fields, in the order they were reached.
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub fn fields() -> String {
    STAGES
        .lock()
        .map(|stages| {
            stages
                .iter()
                .map(|(name, ms)| format!("cp_st_{name}_ms={ms}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stage_keeps_its_first_time() {
        mark("test_stage_once");
        let first = fields();
        std::thread::sleep(std::time::Duration::from_millis(5));
        mark("test_stage_once");
        assert_eq!(first, fields());
        assert!(reached("test_stage_once"));
        assert!(!reached("test_stage_never"));
        assert!(fields().contains("cp_st_test_stage_once_ms="));
    }
}
