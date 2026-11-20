//! Keeps Windows from slowing media worker threads down while the application's windows are covered or minimized.
//!
//! Instead of opting the entire process out of power throttling or raising the system-wide timer resolution with
//! `timeBeginPeriod(1)` (which interferes with the OS compositor and foreground games), each [`Guard`] opts only the
//! acquiring thread out of Windows EcoQoS (`ThreadPowerThrottling`) until dropped.
#![allow(unsafe_code)]

use std::time::Duration;

#[cfg(windows)]
use std::collections::HashMap;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};

/// Sub-millisecond sleep timer backed by Windows' `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION` waitable timer.
///
/// Unlike `std::thread::sleep` (which rounds up to Windows' 15.625 ms system clock tick without a global
/// `timeBeginPeriod(1)` call), a high-resolution waitable timer wakes within ~0.5 ms without disturbing the OS
/// compositor or foreground apps.
pub struct Waiter {
    #[cfg(windows)]
    handle: Option<windows::Win32::Foundation::HANDLE>,
}

// SAFETY: A Win32 waitable timer handle has no thread affinity and is owned exclusively by this `Waiter`.
#[cfg(windows)]
unsafe impl Send for Waiter {}

impl Default for Waiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Waiter {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::{
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, TIMER_ALL_ACCESS,
            };
            use windows::core::PCWSTR;
            // SAFETY: passing null timer attributes and unnamed timer asks Windows for a private handle.
            let handle = unsafe {
                CreateWaitableTimerExW(
                    None,
                    PCWSTR::null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS.0,
                )
            }
            .ok();
            Self { handle }
        }
        #[cfg(not(windows))]
        Self {}
    }

    pub fn sleep(&self, duration: Duration) {
        if duration.is_zero() {
            return;
        }
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::{SetWaitableTimer, WaitForSingleObject};
            let Some(handle) = self.handle else {
                std::thread::sleep(duration);
                return;
            };
            let ticks = (duration.as_nanos().min(i64::MAX as u128) as i64 / 100).max(1);
            let due = -ticks;
            // SAFETY: `handle` is an open waitable timer owned by `self`; `due` lives through `SetWaitableTimer`.
            let armed = unsafe { SetWaitableTimer(handle, &due, 0, None, None, false) }.is_ok();
            if armed {
                let timeout_ms = duration.as_millis().min(u32::MAX as u128 - 50) as u32 + 50;
                // SAFETY: `handle` stays open until `Drop`.
                unsafe {
                    let _ = WaitForSingleObject(handle, timeout_ms);
                }
            } else {
                std::thread::sleep(duration);
            }
        }
        #[cfg(not(windows))]
        std::thread::sleep(duration);
    }
}

#[cfg(windows)]
impl Drop for Waiter {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: closing the owned timer handle on drop.
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
        }
    }
}

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

    #[test]
    fn waiter_handles_zero_and_short_waits() {
        let waiter = Waiter::new();
        waiter.sleep(Duration::ZERO);
        waiter.sleep(Duration::from_millis(1));
    }
}
