//! RTP framing, UDP IP discovery and the `rtpsize` AEAD transport encryption.
//!
//! Adapted from Serein `crates/discord-voice/src/crypto.rs` (`Encryption`, `Rtp`) and
//! `crates/discord-voice/src/transport.rs` (`discovery`, `udp_keepalive`, `public_ip`),
//! MIT OR Apache-2.0. Changes: audio only (no RTCP feedback, video payload types, RTX or
//! soundshare extension), and AES-256-GCM added beside XChaCha20-Poly1305.
use aes_gcm::Aes256Gcm;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305,
    aead::{Aead, Payload},
};
use std::net::IpAddr;

/// Largest datagram accepted or produced.
pub(crate) const MAX_PACKET: usize = 4096;
/// Opus payload type, as advertised in SELECT_PROTOCOL's `codecs`.
pub(crate) const OPUS_PAYLOAD_TYPE: u8 = 120;
/// One 20 ms frame at 48 kHz.
pub(crate) const FRAME_SAMPLES: u32 = 960;
const TAG: usize = 16;
const NONCE_SUFFIX: usize = 4;
const DISCOVERY_LEN: usize = 74;

/// Transport encryption modes this crate implements, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Aes256GcmRtpSize,
    XChaCha20Poly1305RtpSize,
}
impl Mode {
    pub const PREFERENCE: [Mode; 2] = [Mode::Aes256GcmRtpSize, Mode::XChaCha20Poly1305RtpSize];
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Aes256GcmRtpSize => "aead_aes256_gcm_rtpsize",
            Mode::XChaCha20Poly1305RtpSize => "aead_xchacha20_poly1305_rtpsize",
        }
    }
    /// The best mode the server offers in READY's `modes`. Like Serein, nothing else is
    /// acceptable: no unauthenticated or legacy `xsalsa20` modes.
    pub fn choose<'a>(offered: impl IntoIterator<Item = &'a str> + Clone) -> Option<Mode> {
        Self::PREFERENCE
            .into_iter()
            .find(|mode| offered.clone().into_iter().any(|m| m == mode.as_str()))
    }
}

enum Cipher {
    Aes(Box<Aes256Gcm>),
    XChaCha(Box<XChaCha20Poly1305>),
}
impl Cipher {
    /// The four nonce bytes carried on the wire lead an otherwise zero nonce
    /// (12 bytes for AES-GCM, 24 for XChaCha20).
    fn seal(&self, suffix: [u8; 4], aad: &[u8], msg: &[u8]) -> Option<Vec<u8>> {
        let payload = Payload { msg, aad };
        match self {
            Cipher::Aes(cipher) => {
                let mut nonce = aes_gcm::Nonce::default();
                nonce[..4].copy_from_slice(&suffix);
                cipher.encrypt(&nonce, payload).ok()
            }
            Cipher::XChaCha(cipher) => {
                let mut nonce = chacha20poly1305::XNonce::default();
                nonce[..4].copy_from_slice(&suffix);
                cipher.encrypt(&nonce, payload).ok()
            }
        }
    }
    fn open(&self, suffix: [u8; 4], aad: &[u8], msg: &[u8]) -> Option<Vec<u8>> {
        let payload = Payload { msg, aad };
        match self {
            Cipher::Aes(cipher) => {
                let mut nonce = aes_gcm::Nonce::default();
                nonce[..4].copy_from_slice(&suffix);
                cipher.decrypt(&nonce, payload).ok()
            }
            Cipher::XChaCha(cipher) => {
                let mut nonce = chacha20poly1305::XNonce::default();
                nonce[..4].copy_from_slice(&suffix);
                cipher.decrypt(&nonce, payload).ok()
            }
        }
    }
}

/// One authenticated, transport-decrypted RTP packet; the payload is still DAVE ciphertext.
#[derive(Debug)]
pub(crate) struct Rtp {
    pub ssrc: u32,
    pub sequence: u16,
    pub payload: Vec<u8>,
}

/// Per-session transport encryption: the clear RTP header (and extension preamble) is the
/// associated data, the rest is sealed, and a 32-bit big-endian counter nonce trails.
pub(crate) struct Transport {
    cipher: Cipher,
    counter: u32,
}
impl Transport {
    pub fn new(mode: Mode, key: &[u8; 32]) -> Self {
        let cipher = match mode {
            Mode::Aes256GcmRtpSize => Cipher::Aes(Box::new(Aes256Gcm::new(key.into()))),
            Mode::XChaCha20Poly1305RtpSize => {
                Cipher::XChaCha(Box::new(XChaCha20Poly1305::new(key.into())))
            }
        };
        Self { cipher, counter: 0 }
    }
    pub fn seal(&mut self, header: &[u8; 12], frame: &[u8]) -> Result<Vec<u8>, &'static str> {
        if frame.len() > MAX_PACKET - 12 - TAG - NONCE_SUFFIX {
            return Err("Voice packet exceeds transport limit");
        }
        // Serein: a wrapped counter would reuse a nonce under the same key.
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or("Voice transport nonce exhausted; rejoin the call")?;
        let suffix = self.counter.to_be_bytes();
        let sealed = self
            .cipher
            .seal(suffix, header, frame)
            .ok_or("Voice transport encryption failed")?;
        let mut packet = Vec::with_capacity(header.len() + sealed.len() + NONCE_SUFFIX);
        packet.extend_from_slice(header);
        packet.extend_from_slice(&sealed);
        packet.extend_from_slice(&suffix);
        Ok(packet)
    }
    /// Authenticate and decrypt one Opus RTP packet. Anything else (RTCP, video, malformed or
    /// forged datagrams) is `None`.
    pub fn open(&self, packet: &[u8]) -> Option<Rtp> {
        if packet.len() < 12 + TAG + NONCE_SUFFIX
            || packet.len() > MAX_PACKET
            || packet[0] >> 6 != 2
            || packet[1] & 0x7f != OPUS_PAYLOAD_TYPE
        {
            return None;
        }
        let csrc_end = 12 + usize::from(packet[0] & 15) * 4;
        let extended = packet[0] & 0x10 != 0;
        let header_len = csrc_end + if extended { 4 } else { 0 };
        if header_len + TAG + NONCE_SUFFIX > packet.len() {
            return None;
        }
        let suffix: [u8; 4] = packet[packet.len() - NONCE_SUFFIX..].try_into().ok()?;
        let mut frame = self.cipher.open(
            suffix,
            &packet[..header_len],
            &packet[header_len..packet.len() - NONCE_SUFFIX],
        )?;
        if extended {
            // `rtpsize`: the extension preamble is clear, its body is encrypted with the payload.
            let words = u16::from_be_bytes(packet[csrc_end + 2..csrc_end + 4].try_into().ok()?);
            let extension_len = usize::from(words) * 4;
            if extension_len > frame.len() {
                return None;
            }
            frame.drain(..extension_len);
        }
        if packet[0] & 0x20 != 0 {
            let padding = usize::from(*frame.last()?);
            if padding == 0 || padding > frame.len() {
                return None;
            }
            frame.truncate(frame.len() - padding);
        }
        Some(Rtp {
            ssrc: u32::from_be_bytes(packet[8..12].try_into().ok()?),
            sequence: u16::from_be_bytes(packet[2..4].try_into().ok()?),
            payload: frame,
        })
    }
}

/// RTP version 2, no padding, extension or CSRCs, Opus payload type.
pub(crate) fn header(sequence: u16, timestamp: u32, ssrc: u32) -> [u8; 12] {
    let mut header = [0; 12];
    header[0] = 0x80;
    header[1] = OPUS_PAYLOAD_TYPE;
    header[2..4].copy_from_slice(&sequence.to_be_bytes());
    header[4..8].copy_from_slice(&timestamp.to_be_bytes());
    header[8..12].copy_from_slice(&ssrc.to_be_bytes());
    header
}

/// IP discovery request: type 1, length 70, our SSRC, then 66 zero bytes.
pub(crate) fn discovery_request(ssrc: u32) -> [u8; DISCOVERY_LEN] {
    let mut probe = [0; DISCOVERY_LEN];
    probe[..4].copy_from_slice(&[0, 1, 0, 70]);
    probe[4..8].copy_from_slice(&ssrc.to_be_bytes());
    probe
}

/// IP discovery response: type 2, length 70, our SSRC, a NUL-terminated address, a port.
pub(crate) fn discovery_response(packet: &[u8], ssrc: u32) -> Result<(IpAddr, u16), &'static str> {
    if packet.len() != DISCOVERY_LEN
        || packet[..4] != [0, 2, 0, 70]
        || packet[4..8] != ssrc.to_be_bytes()
    {
        return Err("Invalid voice UDP discovery reply");
    }
    let end = packet[8..72]
        .iter()
        .position(|b| *b == 0)
        .ok_or("Invalid voice discovery address")?;
    let address = std::str::from_utf8(&packet[8..8 + end])
        .map_err(|_| "Invalid voice discovery address")?
        .parse()
        .map_err(|_| "Invalid voice discovery address")?;
    let port = u16::from_be_bytes([packet[72], packet[73]]);
    if port == 0 {
        return Err("Invalid voice discovery port");
    }
    Ok((address, port))
}

/// Serein's native UDP ping (unverified against the pinned build): signaling heartbeats alone
/// do not keep an idle media path open, notably while muted.
pub(crate) fn keepalive(sequence: u32) -> [u8; 8] {
    let mut packet = [0x13, 0x37, 0xca, 0xfe, 0, 0, 0, 0];
    packet[4..].copy_from_slice(&sequence.to_le_bytes());
    packet
}

/// Voice servers advertise public addresses; anything else would point media at the LAN.
pub(crate) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_broadcast()
                && !ip.is_multicast()
                && !ip.is_unspecified()
                && !ip.is_documentation()
                && ip.octets()[0] != 0
                && ip.octets()[0] < 240
                && !(ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]))
        }
        IpAddr::V6(ip) => {
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && ip.to_ipv4_mapped().is_none()
                && !(ip.segments()[0] == 0x2001 && ip.segments()[1] == 0xdb8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_choice_prefers_aes_and_refuses_unknown_modes() {
        let both = [
            "xsalsa20_poly1305",
            "aead_xchacha20_poly1305_rtpsize",
            "aead_aes256_gcm_rtpsize",
        ];
        assert_eq!(Mode::choose(both), Some(Mode::Aes256GcmRtpSize));
        let chacha = ["aead_xchacha20_poly1305_rtpsize", "xsalsa20_poly1305_lite"];
        assert_eq!(Mode::choose(chacha), Some(Mode::XChaCha20Poly1305RtpSize));
        assert_eq!(Mode::choose(["xsalsa20_poly1305", "aead_aes256_gcm"]), None);
    }

    #[test]
    fn rtp_header_layout() {
        let header = header(0x0102, 0x0304_0506, 0x0708_090a);
        assert_eq!(header, [0x80, 120, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn discovery_round_trip_and_rejections() {
        let request = discovery_request(0xdead_beef);
        assert_eq!(request.len(), 74);
        assert_eq!(&request[..8], &[0, 1, 0, 70, 0xde, 0xad, 0xbe, 0xef]);
        assert!(request[8..].iter().all(|b| *b == 0));

        let mut reply = [0u8; 74];
        reply[..4].copy_from_slice(&[0, 2, 0, 70]);
        reply[4..8].copy_from_slice(&0xdead_beefu32.to_be_bytes());
        reply[8..8 + 11].copy_from_slice(b"203.0.113.9");
        reply[72..].copy_from_slice(&50_000u16.to_be_bytes());
        let (address, port) = discovery_response(&reply, 0xdead_beef).unwrap();
        assert_eq!(address, "203.0.113.9".parse::<IpAddr>().unwrap());
        assert_eq!(port, 50_000);

        assert!(discovery_response(&reply, 1).is_err(), "another SSRC");
        assert!(
            discovery_response(&reply[..73], 0xdead_beef).is_err(),
            "truncated"
        );
        let mut request_type = reply;
        request_type[1] = 1;
        assert!(discovery_response(&request_type, 0xdead_beef).is_err());
        let mut unterminated = reply;
        unterminated[8..72].fill(b'1');
        assert!(discovery_response(&unterminated, 0xdead_beef).is_err());
        let mut no_port = reply;
        no_port[72..].fill(0);
        assert!(discovery_response(&no_port, 0xdead_beef).is_err());
    }

    #[test]
    fn keepalive_counter_is_little_endian() {
        assert_eq!(keepalive(0x0102_0304), [0x13, 0x37, 0xca, 0xfe, 4, 3, 2, 1]);
    }

    #[test]
    fn nonce_suffix_counts_up_and_leads_the_cipher_nonce() {
        for mode in Mode::PREFERENCE {
            let key = [7; 32];
            let mut transport = Transport::new(mode, &key);
            let header = header(1, 960, 9);
            let first = transport.seal(&header, b"one").unwrap();
            let second = transport.seal(&header, b"two").unwrap();
            assert_eq!(&first[first.len() - 4..], &[0, 0, 0, 1]);
            assert_eq!(&second[second.len() - 4..], &[0, 0, 0, 2]);
            assert_eq!(&first[..12], &header);
            assert_eq!(first.len(), 12 + 3 + 16 + 4);

            // An independent cipher with the nonce laid out by hand opens the packet.
            let body = &second[12..second.len() - 4];
            let payload = Payload {
                msg: body,
                aad: &header,
            };
            let clear = match mode {
                Mode::Aes256GcmRtpSize => {
                    let mut nonce = [0u8; 12];
                    nonce[3] = 2;
                    Aes256Gcm::new(&key.into())
                        .decrypt(&nonce.into(), payload)
                        .unwrap()
                }
                Mode::XChaCha20Poly1305RtpSize => {
                    let mut nonce = [0u8; 24];
                    nonce[3] = 2;
                    XChaCha20Poly1305::new(&key.into())
                        .decrypt(&nonce.into(), payload)
                        .unwrap()
                }
            };
            assert_eq!(clear, b"two");

            transport.counter = u32::MAX;
            assert!(
                transport.seal(&header, b"x").is_err(),
                "nonce reuse refused"
            );
        }
    }

    #[test]
    fn open_authenticates_strips_extensions_and_padding() {
        for mode in Mode::PREFERENCE {
            let mut transport = Transport::new(mode, &[7; 32]);
            let packet = transport.seal(&header(5, 4800, 42), b"dave frame").unwrap();
            let rtp = transport.open(&packet).unwrap();
            assert_eq!((rtp.ssrc, rtp.sequence), (42, 5));
            assert_eq!(rtp.payload, b"dave frame");
            for i in 0..packet.len() {
                let mut corrupt = packet.clone();
                corrupt[i] ^= 0x40;
                assert!(
                    transport.open(&corrupt).is_none(),
                    "byte {i} is authenticated"
                );
                assert!(transport.open(&packet[..i]).is_none(), "truncated to {i}");
            }
            assert!(Transport::new(mode, &[8; 32]).open(&packet).is_none());

            // Extension preamble in the clear, its one-word body sealed with the payload.
            let mut extended = header(6, 5760, 42).to_vec();
            extended[0] |= 0x10;
            extended.extend([0xbe, 0xde, 0, 1]);
            let suffix = [0, 0, 0, 9];
            let sealed = transport
                .cipher
                .seal(suffix, &extended, &[0x10, 0xff, 0, 0, 1, 2, 3])
                .unwrap();
            let mut wire = extended.clone();
            wire.extend(sealed);
            wire.extend(suffix);
            assert_eq!(transport.open(&wire).unwrap().payload, [1, 2, 3]);

            // Padding: the last byte counts the padding bytes, itself included.
            let mut padded = header(7, 6720, 42);
            padded[0] |= 0x20;
            let wire = transport.seal(&padded, &[4, 5, 0, 0, 3]).unwrap();
            assert_eq!(transport.open(&wire).unwrap().payload, [4, 5]);
            let wire = transport.seal(&padded, &[4, 5, 0]).unwrap();
            assert!(transport.open(&wire).is_none(), "zero padding length");

            // Video and RTCP never reach the audio path.
            let mut video = header(8, 0, 42);
            video[1] = 101;
            let wire = transport.seal(&video, b"h264").unwrap();
            assert!(transport.open(&wire).is_none());
        }
    }

    #[test]
    fn only_public_media_addresses() {
        for ip in [
            "10.0.0.1",
            "192.168.1.1",
            "127.0.0.1",
            "100.64.0.1",
            "::1",
            "fd00::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["66.22.196.1", "2606:4700::1"] {
            assert!(public_ip(ip.parse().unwrap()), "{ip}");
        }
    }
}
