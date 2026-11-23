//! What Electron's `powerMonitor` "resume" and the renderer's `online`/`offline` events tell the official client
//! (module 225123 hands them to the gateway socket, module 921747 to each RTC connection), read from Windows: the
//! network connectivity hint (`NotifyNetworkConnectivityHintChange`) and suspend/resume notifications
//! (`PowerRegisterSuspendResumeNotification`, `PBT_APMRESUMEAUTOMATIC`). Both call back on system threads; the
//! events go to a channel and the registrations live as long as the returned guard.
use dc_gateway::SystemEvent;
use tokio::sync::mpsc;

/// Online, offline or neither for a connectivity level: `navigator.onLine` is true with any connected network
/// (local access counts), false with none. Unknown and hidden levels say nothing.
pub(crate) fn online(level: i32) -> Option<bool> {
    match level {
        // NetworkConnectivityLevelHintNone.
        1 => Some(false),
        // LocalAccess, InternetAccess, ConstrainedInternetAccess.
        2..=4 => Some(true),
        _ => None,
    }
}

/// The event for a change of `online`, none for the first reading or no change.
pub(crate) fn transition(last: Option<bool>, now: Option<bool>) -> Option<SystemEvent> {
    match (last, now) {
        (Some(false), Some(true)) => Some(SystemEvent::NetworkOnline),
        (Some(true), Some(false)) => Some(SystemEvent::NetworkOffline),
        _ => None,
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::Mutex;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::IpHelper::{
        CancelMibChangeNotify2, NotifyNetworkConnectivityHintChange,
    };
    use windows::Win32::Networking::WinSock::NL_NETWORK_CONNECTIVITY_HINT;
    use windows::Win32::System::Power::{
        DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY, PowerRegisterSuspendResumeNotification,
        PowerUnregisterSuspendResumeNotification,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC};

    struct Sink {
        events: mpsc::UnboundedSender<SystemEvent>,
        online: Mutex<Option<bool>>,
    }

    pub(crate) struct Watch {
        sink: *mut Sink,
        network: Option<HANDLE>,
        power: Option<HPOWERNOTIFY>,
        params: *mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS,
    }
    // The pointers are only touched by the callbacks (through `Sink`'s own locking) and by `Drop`.
    unsafe impl Send for Watch {}

    unsafe extern "system" fn network(
        context: *const core::ffi::c_void,
        hint: NL_NETWORK_CONNECTIVITY_HINT,
    ) {
        // SAFETY: `context` is the `Sink` the guard keeps alive until both registrations are cancelled.
        let sink = unsafe { &*(context as *const Sink) };
        let now = online(hint.ConnectivityLevel.0);
        let mut last = sink.online.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(event) = transition(*last, now) {
            let _ = sink.events.send(event);
        }
        if now.is_some() {
            *last = now;
        }
    }

    unsafe extern "system" fn power(
        context: *const core::ffi::c_void,
        kind: u32,
        _setting: *const core::ffi::c_void,
    ) -> u32 {
        if kind == PBT_APMRESUMEAUTOMATIC {
            // SAFETY: as in `network`.
            let sink = unsafe { &*(context as *const Sink) };
            let _ = sink.events.send(SystemEvent::PowerResumed);
        }
        0
    }

    pub(crate) fn start(events: mpsc::UnboundedSender<SystemEvent>) -> Watch {
        let sink = Box::into_raw(Box::new(Sink {
            events,
            online: Mutex::new(None),
        }));
        let mut handle = HANDLE::default();
        // SAFETY: the callback and its context outlive the registration (cancelled in `Drop`).
        let network = unsafe {
            NotifyNetworkConnectivityHintChange(
                Some(network),
                Some(sink as *const core::ffi::c_void),
                true,
                &mut handle,
            )
        }
        .is_ok()
        .then_some(handle);
        let params = Box::into_raw(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(power),
            Context: sink as *mut core::ffi::c_void,
        }));
        let mut registration: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: `params` stays allocated until the registration is removed.
        let power = unsafe {
            PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                HANDLE(params as *mut core::ffi::c_void),
                &mut registration,
            )
        }
        .is_ok()
        .then_some(HPOWERNOTIFY(registration as isize));
        if network.is_none() || power.is_none() {
            tracing::warn!(
                network = network.is_some(),
                power = power.is_some(),
                "system event registration incomplete"
            );
        }
        Watch {
            sink,
            network,
            power,
            params,
        }
    }

    impl Drop for Watch {
        fn drop(&mut self) {
            // SAFETY: both registrations were made by `start` and are removed once; the callbacks have returned
            // when the cancel calls do, so the context can go.
            unsafe {
                if let Some(handle) = self.network.take() {
                    let _ = CancelMibChangeNotify2(handle);
                }
                if let Some(power) = self.power.take() {
                    let _ = PowerUnregisterSuspendResumeNotification(power);
                }
                drop(Box::from_raw(self.params));
                drop(Box::from_raw(self.sink));
            }
        }
    }
}

#[cfg(windows)]
pub(crate) use imp::start;

#[cfg(not(windows))]
pub(crate) struct Watch;
#[cfg(not(windows))]
pub(crate) fn start(_: mpsc::UnboundedSender<SystemEvent>) -> Watch {
    Watch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connectivity_levels_map_to_online_and_offline_once() {
        assert_eq!(online(0), None, "unknown");
        assert_eq!(online(1), Some(false));
        assert_eq!(
            online(2),
            Some(true),
            "local access is online, as navigator.onLine"
        );
        assert_eq!(online(3), Some(true));
        assert_eq!(online(4), Some(true));
        assert_eq!(online(5), None, "hidden");
        assert_eq!(transition(None, Some(true)), None, "the first reading");
        assert_eq!(transition(Some(true), Some(true)), None);
        assert_eq!(
            transition(Some(true), Some(false)),
            Some(SystemEvent::NetworkOffline)
        );
        assert_eq!(
            transition(Some(false), Some(true)),
            Some(SystemEvent::NetworkOnline)
        );
        assert_eq!(transition(Some(false), None), None);
    }
}
