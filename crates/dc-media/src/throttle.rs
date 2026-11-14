//! Keeps Windows from slowing media worker threads down while the application's windows are covered or minimized.
//!
//! Instead of opting the entire process out of power throttling or raising the system-wide timer resolution with
//! `timeBeginPeriod(1)` (which interferes with the OS compositor and foreground games), each [`Guard`] opts only the
//! acquiring thread out of Windows EcoQoS (`ThreadPowerThrottling`) until dropped.
#![allow(unsafe_code)]

#[cfg(windows)]
use std::collections::HashMap;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};

/// Holds the acquiring thread out of Windows power throttling until dropped. Guards are counted per thread: the
/// system default policy returns when the last guard for that thread drops.
pub struct Guard {
    #[cfg(windows)]
    tid: u32,
    #[cfg(windows)]
    thread: Option<OwnedHandle>,
}

#[cfg(windows)]
static THREAD_GUARDS: OnceLock<Mutex<HashMap<u32, usize>>> = OnceLock::new();

#[cfg(windows)]
fn thread_guards() -> &'static Mutex<HashMap<u32, usize>> {
    THREAD_GUARDS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Opts `thread` out of (`true`) or back into (`false`) Windows thread power throttling.
#[cfg(windows)]
fn exempt_thread_from_power_throttling(thread: windows::Win32::Foundation::HANDLE, exempt: bool) {
    use windows::Win32::System::Threading::{
        SetThreadInformation, THREAD_POWER_THROTTLING_CURRENT_VERSION,
        THREAD_POWER_THROTTLING_EXECUTION_SPEED, THREAD_POWER_THROTTLING_STATE,
        ThreadPowerThrottling,
    };
    // A zero control mask hands the throttling policy back to the system.
    let state = THREAD_POWER_THROTTLING_STATE {
        Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: if exempt {
            THREAD_POWER_THROTTLING_EXECUTION_SPEED
        } else {
            0
        },
        StateMask: 0,
    };
    // SAFETY: `thread` is a valid thread handle for this call; `state` outlives the call and its size matches.
    let _ = unsafe {
        SetThreadInformation(
            thread,
            ThreadPowerThrottling,
            (&raw const state).cast(),
            size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
        )
    };
}

#[cfg(windows)]
fn duplicate_current_thread() -> Option<OwnedHandle> {
    use windows::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE};
    use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread};

    let mut dup = HANDLE::default();
    // SAFETY: both source and target process handles are the current process pseudo-handle; `dup` receives a newly
    // duplicated real thread handle on success, wrapped in `OwnedHandle` for deterministic closing.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            GetCurrentThread(),
            GetCurrentProcess(),
            &raw mut dup,
            0,
            false,
            DUPLICATE_SAME_ACCESS,
        )
    };
    ok.ok()
        .map(|()| unsafe { OwnedHandle::from_raw_handle(dup.0) })
}

impl Guard {
    pub fn acquire() -> Self {
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::{GetCurrentThread, GetCurrentThreadId};

            // SAFETY: `GetCurrentThreadId` and `GetCurrentThread` are plain queries for the calling thread.
            let tid = unsafe { GetCurrentThreadId() };
            let thread = duplicate_current_thread();
            let mut guards = thread_guards().lock().unwrap_or_else(|e| e.into_inner());
            let count = guards.entry(tid).or_insert(0);
            if *count == 0 {
                // SAFETY: `GetCurrentThread` returns the valid current-thread pseudo-handle for immediate use.
                exempt_thread_from_power_throttling(unsafe { GetCurrentThread() }, true);
            }
            *count += 1;
            Self { tid, thread }
        }
        #[cfg(not(windows))]
        Self {}
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let mut guards = thread_guards().lock().unwrap_or_else(|e| e.into_inner());
            let clear = if let Some(count) = guards.get_mut(&self.tid) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    guards.remove(&self.tid);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            drop(guards);
            if clear && let Some(handle) = self.thread.as_ref() {
                let raw = windows::Win32::Foundation::HANDLE(handle.as_raw_handle());
                exempt_thread_from_power_throttling(raw, false);
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
