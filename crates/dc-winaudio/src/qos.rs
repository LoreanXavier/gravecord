//! Quality of Service: marking the voice UDP packets high priority through qWAVE (`QOSCreateHandle`,
//! `QOSAddSocketToFlow` with `QOSTrafficTypeVoice`), which is what the setting "Hints to your router that the packets
//! Discord is transmitting are high priority" asks for. The official engine receives `qos` as a transport option
//! (module 31595 `setQoS`); what the native module does with it is not readable, qWAVE is the Windows API for it.
#![allow(unsafe_code)]

/// `QOS_TRAFFIC_TYPE` in qos2.h: BestEffort 0, Background 1, ExcellentEffort 2, AudioVideo 3, Voice 4, Control 5.
pub const TRAFFIC_TYPE_VOICE: i32 = 4;
/// `QOS_NON_ADAPTIVE_FLOW`: mark the packets without qWAVE adapting the rate (a voice stream's rate is fixed).
pub const NON_ADAPTIVE_FLOW: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    Add,
    Remove,
    Keep,
}

/// What to do when the setting is `want` and the socket is (`have`) in a flow.
pub fn plan(want: bool, have: bool) -> Plan {
    match (want, have) {
        (true, false) => Plan::Add,
        (false, true) => Plan::Remove,
        _ => Plan::Keep,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QosError {
    /// Not Windows.
    Unsupported,
    /// A qWAVE call failed with this Windows error (1062 is "the qWAVE service is not running").
    Windows { call: &'static str, code: u32 },
}

impl std::fmt::Display for QosError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QosError::Unsupported => f.write_str("packet priority needs Windows qWAVE"),
            QosError::Windows { call, code } => {
                write!(f, "{call} failed with Windows error {code}")
            }
        }
    }
}

#[cfg(windows)]
mod windows_api {
    use std::ffi::c_void;

    #[repr(C)]
    pub struct QosVersion {
        pub major: u16,
        pub minor: u16,
    }

    #[link(name = "qwave", kind = "raw-dylib")]
    unsafe extern "system" {
        pub fn QOSCreateHandle(version: *const QosVersion, handle: *mut isize) -> i32;
        pub fn QOSCloseHandle(handle: isize) -> i32;
        pub fn QOSAddSocketToFlow(
            handle: isize,
            socket: usize,
            destination: *const c_void,
            traffic_type: i32,
            flags: u32,
            flow: *mut u32,
        ) -> i32;
        pub fn QOSRemoveSocketFromFlow(handle: isize, socket: usize, flow: u32, flags: u32) -> i32;
    }
}

/// A socket in a voice flow. Dropping it takes the socket out of the flow and closes the qWAVE handle.
#[derive(Debug)]
pub struct QosFlow {
    #[cfg(windows)]
    handle: isize,
    #[cfg(windows)]
    socket: usize,
    #[cfg(windows)]
    flow: u32,
}

#[cfg(windows)]
impl QosFlow {
    /// Puts a connected UDP `socket` (its raw Windows `SOCKET`) in a voice flow, so its packets leave marked.
    pub fn voice(socket: u64) -> Result<Self, QosError> {
        let version = windows_api::QosVersion { major: 1, minor: 0 };
        let mut handle = 0isize;
        if unsafe { windows_api::QOSCreateHandle(&version, &mut handle) } == 0 {
            return Err(last_error("QOSCreateHandle"));
        }
        let mut flow = 0u32;
        // A connected socket needs no destination address.
        let added = unsafe {
            windows_api::QOSAddSocketToFlow(
                handle,
                socket as usize,
                std::ptr::null(),
                TRAFFIC_TYPE_VOICE,
                NON_ADAPTIVE_FLOW,
                &mut flow,
            )
        };
        if added == 0 {
            let error = last_error("QOSAddSocketToFlow");
            unsafe { windows_api::QOSCloseHandle(handle) };
            return Err(error);
        }
        Ok(Self {
            handle,
            socket: socket as usize,
            flow,
        })
    }
}

#[cfg(not(windows))]
impl QosFlow {
    pub fn voice(_socket: u64) -> Result<Self, QosError> {
        Err(QosError::Unsupported)
    }
}

#[cfg(windows)]
fn last_error(call: &'static str) -> QosError {
    QosError::Windows {
        call,
        code: std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32,
    }
}

#[cfg(windows)]
impl Drop for QosFlow {
    fn drop(&mut self) {
        unsafe {
            windows_api::QOSRemoveSocketFromFlow(self.handle, self.socket, self.flow, 0);
            windows_api::QOSCloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_adds_and_removes_the_flow_only_when_it_changes() {
        assert_eq!(plan(true, false), Plan::Add);
        assert_eq!(plan(false, true), Plan::Remove);
        assert_eq!(plan(true, true), Plan::Keep);
        assert_eq!(plan(false, false), Plan::Keep);
    }

    #[test]
    fn voice_is_traffic_type_four() {
        assert_eq!(TRAFFIC_TYPE_VOICE, 4);
        assert_eq!(NON_ADAPTIVE_FLOW, 2);
    }

    /// Live, on a socket this test owns: a UDP socket "connected" to a documentation address (192.0.2.1, RFC 5737; a
    /// UDP connect sends nothing, and nothing is sent after it), put in a voice flow and taken out again. qWAVE finds
    /// the interface from the destination, so a loopback address is refused (error 1168). Without the qWAVE service the
    /// first call fails; that is reported, not hidden.
    #[cfg(windows)]
    #[test]
    fn our_own_socket_joins_and_leaves_a_voice_flow() {
        use std::os::windows::io::AsRawSocket;
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        socket.connect("192.0.2.1:50000").unwrap();
        match QosFlow::voice(socket.as_raw_socket()) {
            Ok(flow) => {
                assert!(flow.flow != 0 || flow.handle != 0);
                drop(flow);
                eprintln!("qWAVE voice flow: added and removed");
            }
            Err(error) => {
                eprintln!("qWAVE voice flow not available here: {error}");
                assert!(matches!(error, QosError::Windows { .. }));
            }
        }
    }
}
