//! Voice control socket opcodes, read from `RTCControlSocket` in module 921747. The bundle uses bare numbers, so the names
//! here are ours; each one follows what the original does with that op.

macro_rules! opcodes {
    ($($name:ident = $value:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub enum VoiceOp {
            $($name = $value,)*
        }

        impl VoiceOp {
            pub fn from_u64(value: u64) -> Option<Self> {
                match value {
                    $($value => Some(Self::$name),)*
                    _ => None,
                }
            }
        }
    };
}

opcodes! {
    Identify = 0,
    SelectProtocol = 1,
    Ready = 2,
    Heartbeat = 3,
    SelectProtocolAck = 4,
    Speaking = 5,
    HeartbeatAck = 6,
    Resume = 7,
    Hello = 8,
    Resumed = 9,
    ClientConnect = 11,
    Video = 12,
    ClientDisconnect = 13,
    SessionUpdate = 14,
    MediaSinkWants = 15,
    VoiceBackendVersion = 16,
    Flags = 18,
    Platform = 20,
    DavePrepareTransition = 21,
    DaveExecuteTransition = 22,
    DaveTransitionReady = 23,
    DavePrepareEpoch = 24,
    MlsExternalSender = 25,
    MlsKeyPackage = 26,
    MlsProposals = 27,
    MlsCommitWelcome = 28,
    MlsPrepareCommitTransition = 29,
    MlsWelcome = 30,
    MlsInvalidCommitWelcome = 31,
    NoRoute = 32,
    SfuUpdate = 33,
}

/// Close codes from the same module. 48xx are the client's own.
pub mod close {
    pub const AUTHENTICATION_FAILED: u16 = 4004;
    pub const INVALID_SESSION: u16 = 4006;
    pub const SERVER_NOT_FOUND: u16 = 4011;
    pub const SERVER_CRASH: u16 = 4015;
    pub const CANCELED: u16 = 4016;
    pub const HEARTBEAT_TIMEOUT: u16 = 4800;
    pub const UNRESUMABLE: u16 = 4801;
    pub const RESET_BACKOFF: u16 = 4802;
    pub const REPEATED_MLS_INVALID_MESSAGES: u16 = 4803;
    pub const DAVE_DOWNGRADE_REFUSED: u16 = 4804;
    pub const DAVE_INVALID_MESSAGE: u16 = 4805;
}

/// A binary frame: a big-endian sequence number, the opcode, then the payload. Client frames are the opcode and payload.
pub fn binary_frame(op: VoiceOp, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 1);
    frame.push(op as u8);
    frame.extend_from_slice(payload);
    frame
}

#[cfg(test)]
mod tests {
    use super::{VoiceOp, binary_frame};

    #[test]
    fn numbers_match_the_control_socket() {
        assert_eq!(VoiceOp::Hello as u8, 8);
        assert_eq!(VoiceOp::from_u64(33), Some(VoiceOp::SfuUpdate));
        assert_eq!(VoiceOp::from_u64(10), None);
    }

    #[test]
    fn client_binary_frames_lead_with_the_opcode() {
        assert_eq!(binary_frame(VoiceOp::MlsKeyPackage, &[1, 2]), [26, 1, 2]);
    }
}
