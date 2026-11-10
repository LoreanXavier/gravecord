//! The IPC frame: an 8 byte header (little-endian opcode, then little-endian payload length) and a JSON payload.
//! Follows `RPCServer:IPC` in module 998921 (`T` encodes, `processMessages` decodes).

use serde_json::Value;

/// Frame opcodes (`h` in module 998921).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opcode {
    Handshake = 0,
    Frame = 1,
    Close = 2,
    Ping = 3,
    Pong = 4,
}

impl Opcode {
    fn from_i32(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::Handshake,
            1 => Self::Frame,
            2 => Self::Close,
            3 => Self::Ping,
            4 => Self::Pong,
            _ => return None,
        })
    }
}

/// `MAX_BUFFER_SIZE`: bytes held for one socket.
pub const MAX_BUFFER: usize = 5_242_880;
/// A payload larger than this is refused ("message too large").
pub const MAX_MESSAGE: usize = 3_145_728;

/// A frame that cannot be read. The message is the official one; the socket closes with `CLOSE_UNSUPPORTED`.
#[derive(Debug, PartialEq, Eq)]
pub struct FrameError(pub String);

/// `T`: the frame for `payload` (header and JSON text).
pub fn encode(opcode: Opcode, payload: &Value) -> Vec<u8> {
    let text = payload.to_string();
    let mut out = Vec::with_capacity(8 + text.len());
    out.extend_from_slice(&(opcode as i32).to_le_bytes());
    out.extend_from_slice(&(text.len() as i32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    out
}

/// Reassembles frames from the bytes a socket delivers (`read` and `processMessages`).
#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
    header: Option<(Opcode, usize)>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), FrameError> {
        if self.buffer.len() + bytes.len() > MAX_BUFFER {
            return Err(FrameError("total buffer size exceeded".into()));
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    /// The next whole frame, or `None` when more bytes are needed.
    pub fn next_frame(&mut self) -> Result<Option<(Opcode, Value)>, FrameError> {
        if self.header.is_none() {
            if self.buffer.len() < 8 {
                return Ok(None);
            }
            let opcode = i32::from_le_bytes(self.buffer[0..4].try_into().expect("four bytes"));
            let size = i32::from_le_bytes(self.buffer[4..8].try_into().expect("four bytes"));
            let opcode = Opcode::from_i32(opcode)
                .filter(|_| size >= 0)
                .ok_or_else(|| FrameError("protocol error".into()))?;
            if size as usize > MAX_MESSAGE {
                return Err(FrameError("message too large".into()));
            }
            self.buffer.drain(..8);
            self.header = Some((opcode, size as usize));
        }
        let (opcode, size) = self.header.expect("set above");
        if self.buffer.len() < size {
            return Ok(None);
        }
        let payload: Vec<u8> = self.buffer.drain(..size).collect();
        self.header = None;
        let value =
            serde_json::from_slice(&payload).map_err(|error| FrameError(error.to_string()))?;
        Ok(Some((opcode, value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_frame_is_an_eight_byte_little_endian_header_and_json() {
        let bytes = encode(Opcode::Frame, &json!({"a": 1}));
        assert_eq!(&bytes[0..4], &[1, 0, 0, 0]);
        assert_eq!(&bytes[4..8], &[7, 0, 0, 0]);
        assert_eq!(&bytes[8..], br#"{"a":1}"#);
    }

    #[test]
    fn the_length_counts_bytes_not_characters() {
        let bytes = encode(Opcode::Frame, &json!("é"));
        assert_eq!(i32::from_le_bytes(bytes[4..8].try_into().unwrap()), 4);
    }

    #[test]
    fn frames_split_across_reads_and_glued_together_both_decode() {
        let mut wire = encode(Opcode::Handshake, &json!({"v": 1, "client_id": "1"}));
        wire.extend(encode(Opcode::Ping, &json!("x")));
        let mut decoder = Decoder::default();
        for chunk in wire.chunks(3) {
            decoder.push(chunk).unwrap();
        }
        let first = decoder.next_frame().unwrap().unwrap();
        assert_eq!(first.0, Opcode::Handshake);
        assert_eq!(first.1["client_id"], "1");
        assert_eq!(decoder.next_frame().unwrap().unwrap().0, Opcode::Ping);
        assert_eq!(decoder.next_frame().unwrap(), None);
    }

    #[test]
    fn an_unknown_opcode_or_a_negative_size_is_a_protocol_error() {
        for (opcode, size) in [(9i32, 0i32), (1, -1)] {
            let mut decoder = Decoder::default();
            let mut wire = opcode.to_le_bytes().to_vec();
            wire.extend(size.to_le_bytes());
            decoder.push(&wire).unwrap();
            assert_eq!(
                decoder.next_frame(),
                Err(FrameError("protocol error".into()))
            );
        }
    }

    #[test]
    fn the_size_limits_are_the_official_ones() {
        let mut decoder = Decoder::default();
        let mut wire = 1i32.to_le_bytes().to_vec();
        wire.extend((MAX_MESSAGE as i32 + 1).to_le_bytes());
        decoder.push(&wire).unwrap();
        assert_eq!(
            decoder.next_frame(),
            Err(FrameError("message too large".into()))
        );
        let mut decoder = Decoder::default();
        decoder.push(&vec![0; MAX_BUFFER]).unwrap();
        assert_eq!(
            decoder.push(&[0]),
            Err(FrameError("total buffer size exceeded".into()))
        );
    }

    #[test]
    fn a_payload_that_is_not_json_is_an_error() {
        let mut decoder = Decoder::default();
        let mut wire = 1i32.to_le_bytes().to_vec();
        wire.extend(3i32.to_le_bytes());
        wire.extend(b"abc");
        decoder.push(&wire).unwrap();
        assert!(decoder.next_frame().is_err());
    }
}
