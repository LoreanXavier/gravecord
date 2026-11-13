//! Keeps Windows from slowing media work down while the application's windows are covered or minimized.
//!
//! Adapted from Serein's `discord-voice/src/timer.rs` (MIT/Apache-2.0; see DECISIONS.md). Windows rounds timed waits
//! up to its 15.6 ms tick unless a process asks for a finer one, and Windows 11 ignores that request and may move the
//! process to efficiency cores once all of its windows are covered, which is exactly where a client sits while a game
//! is shared. While a [`Guard`] is alive the process is opted out of that power throttling and the system timer runs at
//! 1 ms, as browsers do during a call.
#![allow(unsafe_code)]

/// Holds the process out of power throttling, and the timer at 1 ms, until dropped. Guards are counted: the system
/// defaults come back when the last one goes.
pub struct Guard {
    #[cfg(windows)]
    raised: bool,
}

#[cfg(windows)]
static GUARDS: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

/// Opts the process out of (`true`) or back into (`false`) Windows power throttling.
#[cfg(windows)]
fn exempt_from_power_throttling(exempt: bool) {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, SetProcessInformation,
    };
    // A zero control mask hands both policies back to the system.
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: if exempt {
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION
        } else {
            0
        },
        StateMask: 0,
    };
    // SAFETY: the pseudo-handle needs no closing; the struct outlives the call and its size is passed exactly. An
    // older Windows refuses the flags it does not know, which only keeps the default.
    let _ = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&raw const state).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
}

impl Guard {
    pub fn acquire() -> Self {
        #[cfg(windows)]
        {
            let mut count = GUARDS.lock().unwrap_or_else(|e| e.into_inner());
            if *count == 0 {
                exempt_from_power_throttling(true);
            }
            *count += 1;
            drop(count);
            // SAFETY: a process-wide request, balanced by `timeEndPeriod` in `Drop` when it succeeded.
            let raised = unsafe { windows::Win32::Media::timeBeginPeriod(1) } == 0;
            Self { raised }
        }
        #[cfg(not(windows))]
        Self {}
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if self.raised {
                // SAFETY: balances the successful `timeBeginPeriod(1)` of `acquire`.
                unsafe {
                    windows::Win32::Media::timeEndPeriod(1);
                }
            }
            let mut count = GUARDS.lock().unwrap_or_else(|e| e.into_inner());
            *count = count.saturating_sub(1);
            if *count == 0 {
                exempt_from_power_throttling(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guards_nest_and_release() {
        let first = Guard::acquire();
        let second = Guard::acquire();
        drop(first);
        drop(second);
        let again = Guard::acquire();
        drop(again);
    }
}
