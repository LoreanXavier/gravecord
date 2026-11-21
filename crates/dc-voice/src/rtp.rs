//! RTP framing, UDP IP discovery and the `rtpsize` AEAD transport encryption.
//!
//! Adapted from Serein `crates/discord-voice/src/crypto.rs` (`Encryption`, `Rtp`) and
//! `crates/discord-voice/src/transport.rs` (`discovery`, `udp_keepalive`, `public_ip`),
//! MIT OR Apache-2.0. Changes: AES-256-GCM added beside XChaCha20-Poly1305; packets are routed by payload type (Opus,
//! H.264 and its RTX stream); RTCP feedback builders and parsers (PLI, NACK, receiver reports, REMB); the soundshare
//! extension for Go Live audio. The Rust here stays `unsafe`-free.
use crate::bwe::{SentHistory, TransportCc};
use aes_gcm::Aes256Gcm;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305,
    aead::{Aead, Payload},
};
use std::net::IpAddr;
use tokio::time::Instant;

/// Largest datagram accepted or produced.
pub(crate) const MAX_PACKET: usize = 4096;
/// Opus payload type, as advertised in SELECT_PROTOCOL's `codecs`.
pub(crate) const OPUS_PAYLOAD_TYPE: u8 = 120;
/// H.264 and its RTX stream. The official client lists `[AV1], [H265], H264, VP8` and gives entry `i` payload type
/// `101 + 2 * i` with RTX `102 + 2 * i` (module 31595, `codecs`); this client offers H.264 alone, so it is entry 0.
pub(crate) const H264_PAYLOAD_TYPE: u8 = 101;
pub(crate) const H264_RTX_PAYLOAD_TYPE: u8 = 102;
/// One 20 ms frame at 48 kHz.
pub(crate) const FRAME_SAMPLES: u32 = 960;
/// The one-byte RTP header extension that carries the transport-wide sequence number
/// (`draft-holmer-rmcat-transport-wide-cc-extensions-01`). The native engine registers its extensions with fixed ids,
/// the same for audio and video: 1 audio level (audio only), 2 toffset, 3 abs-send-time, 4 video orientation,
/// 5 transport-wide cc, 6 playout delay, 7 video content type, 8 video timing, 9 Discord's speaker (audio only), 10 mid,
/// 11 rtp-stream-id, 12 repaired-rtp-stream-id (`discord_voice.node` of the pinned build, the two lists built at
/// RVA 0x17a600 and 0x17aa70).
pub(crate) const TRANSPORT_SEQUENCE_EXTENSION: u8 = 5;
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

/// RTCP packet types run from 192 to 223 in the second byte; RTP payload types with their marker bit never land there
/// for the types Discord uses (RFC 5761).
pub(crate) fn is_rtcp(packet: &[u8]) -> bool {
    packet.len() >= 2 && packet[0] >> 6 == 2 && (192..=223).contains(&packet[1])
}

/// The share of our packets the server reports lost, in percent, from a compound RTCP packet's sender and receiver
/// reports (RFC 3550: fraction lost is the byte after the block's SSRC, over 256). The worst report wins. A malformed
/// compound packet counts for nothing.
// Serein (MIT OR Apache-2.0): `parse_feedback` in discord-voice/src/crypto.rs.
pub(crate) fn outbound_loss(mut remaining: &[u8], ssrc: u32) -> Option<f32> {
    let mut lost: Option<u8> = None;
    while !remaining.is_empty() {
        if remaining.len() < 4 || !is_rtcp(remaining) {
            return None;
        }
        let size = (usize::from(u16::from_be_bytes([remaining[2], remaining[3]])) + 1) * 4;
        if size > remaining.len() {
            return None;
        }
        let packet = &remaining[..size];
        let start = match packet[1] {
            200 => Some(28),
            201 => Some(8),
            _ => None,
        };
        if let Some(start) = start {
            let end = start + usize::from(packet[0] & 0x1f) * 24;
            if packet.len() < end {
                return None;
            }
            for report in packet[start..end].as_chunks::<24>().0 {
                if report[..4] == ssrc.to_be_bytes() {
                    lost = Some(lost.map_or(report[4], |known| known.max(report[4])));
                }
            }
        }
        remaining = &remaining[size..];
    }
    lost.map(|fraction| f32::from(fraction) * 100. / 256.)
}

/// A reception report block for one source (RFC 3550 section 6.4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReportBlock {
    pub ssrc: u32,
    /// Share of packets lost since the last report, over 256.
    pub fraction_lost: u8,
    /// Signed 24-bit total.
    pub cumulative_lost: i32,
    /// Extended highest sequence number received.
    pub highest: u32,
    pub jitter: u32,
    /// Middle 32 bits of the last sender report's NTP time, and the delay since it in 1/65536 s.
    pub last_sender_report: u32,
    pub delay: u32,
}

/// RTCP receiver report: the header the transport leaves in the clear (eight bytes) and the sealed rest.
pub(crate) fn receiver_report(sender: u32, blocks: &[ReportBlock]) -> ([u8; 8], Vec<u8>) {
    let blocks = &blocks[..blocks.len().min(31)];
    let mut header = [0x80 | blocks.len() as u8, 201, 0, 0, 0, 0, 0, 0];
    header[2..4].copy_from_slice(&((1 + 6 * blocks.len()) as u16).to_be_bytes());
    header[4..].copy_from_slice(&sender.to_be_bytes());
    let mut body = Vec::with_capacity(blocks.len() * 24);
    for block in blocks {
        body.extend_from_slice(&block.ssrc.to_be_bytes());
        let loss =
            (u32::from(block.fraction_lost) << 24) | (block.cumulative_lost as u32 & 0x00ff_ffff);
        for word in [
            loss,
            block.highest,
            block.jitter,
            block.last_sender_report,
            block.delay,
        ] {
            body.extend_from_slice(&word.to_be_bytes());
        }
    }
    (header, body)
}

/// RFC 4585 picture loss indication: the sender must send a keyframe.
// Serein (MIT OR Apache-2.0): `pli` in discord-voice/src/video_receive.rs.
pub(crate) fn pli(sender: u32, media: u32) -> ([u8; 8], [u8; 4]) {
    let mut header = [0x81, 206, 0, 2, 0, 0, 0, 0];
    header[4..].copy_from_slice(&sender.to_be_bytes());
    (header, media.to_be_bytes())
}

/// RFC 4585 generic NACK for up to 64 sequence numbers of `media`, packed as pid + 16-bit mask pairs.
// Serein (MIT OR Apache-2.0): `Window::nack` in discord-voice/src/video_reorder.rs.
pub(crate) fn nack(sender: u32, media: u32, sequences: &[u16]) -> Option<([u8; 8], Vec<u8>)> {
    if sequences.is_empty() {
        return None;
    }
    let mut body = media.to_be_bytes().to_vec();
    let sequences = &sequences[..sequences.len().min(64)];
    let mut at = 0;
    while at < sequences.len() {
        let pid = sequences[at];
        at += 1;
        let mut mask = 0u16;
        while at < sequences.len() {
            let distance = sequences[at].wrapping_sub(pid);
            if !(1..=16).contains(&distance) {
                break;
            }
            mask |= 1 << (distance - 1);
            at += 1;
        }
        body.extend_from_slice(&pid.to_be_bytes());
        body.extend_from_slice(&mask.to_be_bytes());
    }
    let mut header = [0x81, 205, 0, 0, 0, 0, 0, 0];
    header[2..4].copy_from_slice(&((body.len() / 4 + 1) as u16).to_be_bytes());
    header[4..].copy_from_slice(&sender.to_be_bytes());
    Some((header, body))
}

/// What a viewer's authenticated RTCP asks of the sender of `video_ssrc`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Feedback {
    /// The worst loss fraction (over 256) the reports name for this source.
    pub loss: Option<u8>,
    /// The highest cumulative lost packet count (24-bit signed, clamped `>= 0`) reported for this source.
    pub cumulative_lost: Option<u64>,
    /// A picture loss indication for this source.
    pub keyframe: bool,
    /// Sequence numbers to resend, at most 128.
    pub nacks: Vec<u16>,
    /// REMB estimate in bits per second.
    pub bitrate: Option<u32>,
    /// Transport-wide feedback messages (RTPFB FMT 15). They describe the transport, not one source, so every one is
    /// kept whatever media SSRC it names.
    pub transport: Vec<TransportCc>,
}

/// Parses a decrypted compound RTCP packet: reports (RFC 3550), PLI and NACK (RFC 4585) and REMB, keeping what names
/// `video_ssrc`. A malformed trailing packet discards everything before it.
// Serein (MIT OR Apache-2.0): `parse_feedback` in discord-voice/src/crypto.rs.
pub(crate) fn feedback(mut remaining: &[u8], video_ssrc: u32) -> Option<Feedback> {
    if video_ssrc == 0 {
        return None;
    }
    let mut feedback = Feedback::default();
    while !remaining.is_empty() {
        if remaining.len() < 4 || !is_rtcp(remaining) {
            return None;
        }
        let size = (usize::from(u16::from_be_bytes([remaining[2], remaining[3]])) + 1) * 4;
        if size > remaining.len() {
            return None;
        }
        let mut content = &remaining[..size];
        if content[0] & 0x20 != 0 {
            let padding = usize::from(content[size - 1]);
            if padding == 0 || padding > size - 4 {
                return None;
            }
            content = &content[..size - padding];
        }
        let count = usize::from(content[0] & 0x1f);
        match content[1] {
            kind @ (200 | 201) => {
                let start = if kind == 200 { 28 } else { 8 };
                let end = start + count * 24;
                if content.len() < end || !content.len().is_multiple_of(4) {
                    return None;
                }
                for report in content[start..end].as_chunks::<24>().0 {
                    if report[..4] == video_ssrc.to_be_bytes() {
                        feedback.loss =
                            Some(feedback.loss.map_or(report[4], |loss| loss.max(report[4])));
                        let raw = ((i32::from(report[5]) << 16)
                            | (i32::from(report[6]) << 8)
                            | i32::from(report[7]))
                            << 8
                            >> 8;
                        let cum = u64::try_from(raw).unwrap_or(0);
                        feedback.cumulative_lost =
                            Some(feedback.cumulative_lost.map_or(cum, |known| known.max(cum)));
                    }
                }
            }
            kind @ (205 | 206) => {
                if content.len() < 12 {
                    return None;
                }
                let matching = content[8..12] == video_ssrc.to_be_bytes();
                match (kind, count) {
                    (205, 15) => {
                        // Transport-wide feedback carries byte-granular small deltas, so after stripping RFC 3550
                        // padding (`P = 1`, 1..=3 bytes) `content.len()` is legitimately not a multiple of 4.
                        // A block this parser refuses is skipped alone: the keyframe requests and repairs beside
                        // it in the compound still count.
                        if feedback.transport.len() < 16
                            && let Some(report) = crate::bwe::parse_transport_cc(content)
                        {
                            feedback.transport.push(report);
                        }
                    }
                    (206, 1) => {
                        if content.len() != 12 {
                            return None;
                        }
                        feedback.keyframe |= matching;
                    }
                    (205, 1) => {
                        if content.len() == 12 || !content.len().is_multiple_of(4) {
                            return None;
                        }
                        if matching {
                            for item in content[12..].as_chunks::<4>().0 {
                                let pid = u16::from_be_bytes([item[0], item[1]]);
                                let mask = u16::from_be_bytes([item[2], item[3]]);
                                for offset in 0..=16u16 {
                                    if (offset == 0 || mask & (1 << (offset - 1)) != 0)
                                        && feedback.nacks.len() < 128
                                    {
                                        let sequence = pid.wrapping_add(offset);
                                        if !feedback.nacks.contains(&sequence) {
                                            feedback.nacks.push(sequence);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    (206, 15) if content.get(12..16) == Some(b"REMB") => {
                        if content.len() < 20
                            || !content.len().is_multiple_of(4)
                            || content.len() != 20 + usize::from(content[16]) * 4
                        {
                            return None;
                        }
                        let exponent = u32::from(content[17] >> 2);
                        let mantissa = (u32::from(content[17] & 3) << 16)
                            | (u32::from(content[18]) << 8)
                            | u32::from(content[19]);
                        let bitrate = if mantissa == 0 {
                            0
                        } else {
                            if exponent >= 32 || mantissa > (u32::MAX >> exponent) {
                                return None;
                            }
                            mantissa << exponent
                        };
                        if content[20..]
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .any(|ssrc| *ssrc == video_ssrc.to_be_bytes())
                        {
                            feedback.bitrate =
                                Some(feedback.bitrate.map_or(bitrate, |old| old.min(bitrate)));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        remaining = &remaining[size..];
    }
    Some(feedback)
}

/// The sender reports of a decrypted compound RTCP packet: each sender's SSRC with the middle 32 bits of its NTP time,
/// which a receiver report echoes as LSR (RFC 3550 section 6.4.1). A malformed compound packet gives nothing.
pub(crate) fn sender_reports(mut remaining: &[u8]) -> Vec<(u32, u32)> {
    let mut reports = Vec::new();
    while !remaining.is_empty() {
        if remaining.len() < 4 || !is_rtcp(remaining) {
            return Vec::new();
        }
        let size = (usize::from(u16::from_be_bytes([remaining[2], remaining[3]])) + 1) * 4;
        if size > remaining.len() {
            return Vec::new();
        }
        let packet = &remaining[..size];
        if packet[1] == 200 && packet.len() >= 28 {
            let ssrc = u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]);
            let middle = u32::from_be_bytes([packet[10], packet[11], packet[12], packet[13]]);
            reports.push((ssrc, middle));
        }
        remaining = &remaining[size..];
    }
    reports
}

// One authenticated, transport-decrypted RTP packet; the payload is still DAVE ciphertext.
#[derive(Debug)]
pub(crate) struct Rtp {
    pub payload_type: u8,
    pub marker: bool,
    pub ssrc: u32,
    pub sequence: u16,
    pub timestamp: u32,
    /// The transport-wide sequence number of extension 5, when the packet carries one.
    pub transport_sequence: Option<u16>,
    pub payload: Vec<u8>,
}

/// The transport-wide sequence number in a decrypted RFC 8285 one-byte extension block (`0xBEDE` profile): element 5
/// with two bytes of data. Padding bytes (id 0) are skipped; id 15 ends the block. Two-byte blocks carry none here.
pub(crate) fn transport_sequence(profile: [u8; 2], mut block: &[u8]) -> Option<u16> {
    if profile != [0xbe, 0xde] {
        return None;
    }
    while let Some((&first, rest)) = block.split_first() {
        let id = first >> 4;
        if id == 0 {
            block = rest;
            continue;
        }
        if id == 15 {
            return None;
        }
        let length = usize::from(first & 0x0f) + 1;
        if rest.len() < length {
            return None;
        }
        if id == TRANSPORT_SEQUENCE_EXTENSION && length == 2 {
            return Some(u16::from_be_bytes([rest[0], rest[1]]));
        }
        block = &rest[length..];
    }
    None
}

/// Per-session transport encryption: the clear RTP header (and extension preamble) is the
/// associated data, the rest is sealed, and a 32-bit big-endian counter nonce trails.
pub(crate) struct Transport {
    cipher: Cipher,
    counter: u32,
    /// Every RTP packet this transport numbered and sealed, for the transport-wide feedback that comes back.
    pub sent: SentHistory,
}
impl Transport {
    pub fn new(mode: Mode, key: &[u8; 32]) -> Self {
        let cipher = match mode {
            Mode::Aes256GcmRtpSize => Cipher::Aes(Box::new(Aes256Gcm::new(key.into()))),
            Mode::XChaCha20Poly1305RtpSize => {
                Cipher::XChaCha(Box::new(XChaCha20Poly1305::new(key.into())))
            }
        };
        Self {
            cipher,
            counter: 0,
            sent: SentHistory::default(),
        }
    }
    pub fn mode_name(&self) -> &'static str {
        match self.cipher {
            Cipher::Aes(_) => "aead_aes256_gcm_rtpsize",
            Cipher::XChaCha(_) => "aead_xchacha20_poly1305_rtpsize",
        }
    }
    /// One RTP packet (Opus, H.264 or a retransmission) with the transport-wide sequence number in a one-byte RFC 8285
    /// extension (`TRANSPORT_EXTENSION`), recorded in [`Self::sent`] as leaving at `now`. As with every `rtpsize`
    /// extension, the four-byte preamble is authenticated in the clear and the element is sealed with the payload.
    pub fn seal(
        &mut self,
        header: &[u8; 12],
        frame: &[u8],
        now: Instant,
    ) -> Result<Vec<u8>, &'static str> {
        let sequence = self.sent.next().to_be_bytes();
        self.seal_numbered(
            header,
            &[
                TRANSPORT_SEQUENCE_EXTENSION << 4 | 1,
                sequence[0],
                sequence[1],
                0,
            ],
            frame,
            now,
        )
    }
    /// Go Live audio: RFC 8285 extension 9 marks it as soundshare, next to the transport-wide sequence number.
    // Serein (MIT OR Apache-2.0): `seal_soundshare` in discord-voice/src/crypto.rs. The extension is unofficial there
    // too: the native module that sends it cannot be read from the pinned build.
    pub fn seal_soundshare(
        &mut self,
        header: &[u8; 12],
        frame: &[u8],
        now: Instant,
    ) -> Result<Vec<u8>, &'static str> {
        let sequence = self.sent.next().to_be_bytes();
        self.seal_numbered(
            header,
            &[
                0x90,
                0x04,
                TRANSPORT_SEQUENCE_EXTENSION << 4 | 1,
                sequence[0],
                sequence[1],
                0,
                0,
                0,
            ],
            frame,
            now,
        )
    }
    /// `elements` is the one-byte extension block, padded to whole words.
    fn seal_numbered(
        &mut self,
        header: &[u8; 12],
        elements: &[u8],
        frame: &[u8],
        now: Instant,
    ) -> Result<Vec<u8>, &'static str> {
        let mut extended = [0; 16];
        extended[..12].copy_from_slice(header);
        extended[0] |= 0x10;
        extended[12..14].copy_from_slice(&[0xbe, 0xde]);
        extended[14..].copy_from_slice(&((elements.len() / 4) as u16).to_be_bytes());
        let mut payload = Vec::with_capacity(elements.len() + frame.len());
        payload.extend_from_slice(elements);
        payload.extend_from_slice(frame);
        let wire = self.seal_packet(&extended, &payload)?;
        self.sent.sent(wire.len(), now);
        Ok(wire)
    }
    /// One RTCP packet: the eight-byte header stays clear as associated data, the rest is sealed, and the four-byte
    /// nonce trails, as in `rtpsize` RTP.
    pub fn seal_rtcp(&mut self, header: &[u8; 8], body: &[u8]) -> Result<Vec<u8>, &'static str> {
        self.seal_packet(header, body)
    }
    fn seal_packet(&mut self, header: &[u8], frame: &[u8]) -> Result<Vec<u8>, &'static str> {
        if frame.len() > MAX_PACKET - header.len() - TAG - NONCE_SUFFIX {
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
    /// Authenticate one control datagram: the first eight bytes stay clear as associated data, the rest is sealed
    /// and the four-byte nonce trails, as in `rtpsize` RTP. Returns the whole compound packet.
    // Serein (MIT OR Apache-2.0): `open_rtcp` in discord-voice/src/crypto.rs.
    pub fn open_rtcp(&self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.len() < 8 + TAG + NONCE_SUFFIX || packet.len() > MAX_PACKET || !is_rtcp(packet) {
            return None;
        }
        let suffix: [u8; 4] = packet[packet.len() - NONCE_SUFFIX..].try_into().ok()?;
        let body = self.cipher.open(
            suffix,
            &packet[..8],
            &packet[8..packet.len() - NONCE_SUFFIX],
        )?;
        let mut compound = Vec::with_capacity(8 + body.len());
        compound.extend_from_slice(&packet[..8]);
        compound.extend_from_slice(&body);
        Some(compound)
    }
    /// A viewer's RTCP about our video source, authenticated and parsed.
    #[cfg(test)]
    pub fn feedback(&self, packet: &[u8], video_ssrc: u32) -> Option<Feedback> {
        feedback(&self.open_rtcp(packet)?, video_ssrc)
    }
    /// Authenticate and decrypt one RTP packet of a payload type this client offered: Opus, H.264 or H.264 RTX.
    /// Anything else (RTCP, other payload types, malformed or forged datagrams) is `None`.
    pub fn open(&self, packet: &[u8]) -> Option<Rtp> {
        if packet.len() < 12 + TAG + NONCE_SUFFIX
            || packet.len() > MAX_PACKET
            || packet[0] >> 6 != 2
            || !matches!(
                packet[1] & 0x7f,
                OPUS_PAYLOAD_TYPE | H264_PAYLOAD_TYPE | H264_RTX_PAYLOAD_TYPE
            )
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
        let mut transport_sequence = None;
        if extended {
            // `rtpsize`: the extension preamble is clear, its body is encrypted with the payload.
            let words = u16::from_be_bytes(packet[csrc_end + 2..csrc_end + 4].try_into().ok()?);
            let extension_len = usize::from(words) * 4;
            if extension_len > frame.len() {
                return None;
            }
            transport_sequence = self::transport_sequence(
                [packet[csrc_end], packet[csrc_end + 1]],
                &frame[..extension_len],
            );
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
            payload_type: packet[1] & 0x7f,
            marker: packet[1] & 0x80 != 0,
            ssrc: u32::from_be_bytes(packet[8..12].try_into().ok()?),
            sequence: u16::from_be_bytes(packet[2..4].try_into().ok()?),
            timestamp: u32::from_be_bytes(packet[4..8].try_into().ok()?),
            transport_sequence,
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

/// RTP version 2 header of a video packet: `marker` ends an access unit.
pub(crate) fn video_header(
    payload_type: u8,
    marker: bool,
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
) -> [u8; 12] {
    let mut header = [0; 12];
    header[0] = 0x80;
    header[1] = payload_type | u8::from(marker) << 7;
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
    // The call simulator listens on loopback, and only there.
    #[cfg(feature = "sim")]
    if crate::sim::active() {
        return ip.is_loopback();
    }
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
            let first = transport.seal_packet(&header, b"one").unwrap();
            let second = transport.seal_packet(&header, b"two").unwrap();
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
                transport.seal_packet(&header, b"x").is_err(),
                "nonce reuse refused"
            );
        }
    }

    #[test]
    fn a_receiver_report_gives_the_loss_of_our_own_ssrc() {
        let mut report = vec![0x81, 201, 0, 7];
        report.extend(9u32.to_be_bytes());
        report.extend(77u32.to_be_bytes());
        report.push(64);
        report.extend([0; 19]);
        assert_eq!(outbound_loss(&report, 77), Some(25.));
        assert_eq!(outbound_loss(&report, 78), None);
        // Cut short, or followed by garbage: nothing is believed.
        assert_eq!(outbound_loss(&report[..20], 77), None);
        let mut trailing = report.clone();
        trailing.extend([0x80, 201, 0]);
        assert_eq!(outbound_loss(&trailing, 77), None);
    }

    #[test]
    fn rtcp_is_sealed_after_its_eight_byte_header() {
        let key = [7; 32];
        let transport = Transport::new(Mode::XChaCha20Poly1305RtpSize, &key);
        let header = [0x80, 201, 0, 1, 0, 0, 0, 9];
        let suffix = 5u32.to_be_bytes();
        let sealed = transport.cipher.seal(suffix, &header, &[0; 4]).unwrap();
        let mut packet = header.to_vec();
        packet.extend(&sealed);
        packet.extend(suffix);
        assert_eq!(transport.open_rtcp(&packet).unwrap()[..8], header);
        packet[1] = 120;
        assert!(transport.open_rtcp(&packet).is_none());
    }

    #[test]
    fn open_authenticates_strips_extensions_and_padding() {
        for mode in Mode::PREFERENCE {
            let mut transport = Transport::new(mode, &[7; 32]);
            let packet = transport
                .seal_packet(&header(5, 4800, 42), b"dave frame")
                .unwrap();
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
            let wire = transport.seal_packet(&padded, &[4, 5, 0, 0, 3]).unwrap();
            assert_eq!(transport.open(&wire).unwrap().payload, [4, 5]);
            let wire = transport.seal_packet(&padded, &[4, 5, 0]).unwrap();
            assert!(transport.open(&wire).is_none(), "zero padding length");

            // Video is routed by payload type, with its marker; other payload types never open.
            let video = video_header(H264_PAYLOAD_TYPE, true, 8, 90_000, 42);
            let wire = transport.seal_packet(&video, b"h264").unwrap();
            let rtp = transport.open(&wire).unwrap();
            assert_eq!(
                (rtp.payload_type, rtp.marker, rtp.sequence, rtp.timestamp),
                (101, true, 8, 90_000)
            );
            assert_eq!(rtp.payload, b"h264");
            let rtx = video_header(H264_RTX_PAYLOAD_TYPE, false, 9, 90_000, 43);
            let wire = transport.seal_packet(&rtx, b"x").unwrap();
            let rtp = transport.open(&wire).unwrap();
            assert_eq!((rtp.payload_type, rtp.marker), (102, false));
            let wire = transport.seal_packet(&header(1, 960, 42), b"a").unwrap();
            assert_eq!(
                transport.open(&wire).unwrap().payload_type,
                OPUS_PAYLOAD_TYPE
            );
            let other = video_header(96, true, 8, 0, 42);
            let wire = transport.seal_packet(&other, b"p").unwrap();
            assert!(transport.open(&wire).is_none());
        }
    }

    #[test]
    fn pli_nack_and_receiver_report_builders() {
        let (header, body) = pli(0x0102_0304, 0x0a0b_0c0d);
        assert_eq!(header, [0x81, 206, 0, 2, 1, 2, 3, 4]);
        assert_eq!(body, [0x0a, 0x0b, 0x0c, 0x0d]);

        // 10, 11, 13 share one pid with a mask; 40 and 65535 -> 1 start new items.
        let (header, body) = nack(7, 9, &[10, 11, 13, 40, 65_535, 0]).unwrap();
        assert_eq!(&header[..4], &[0x81, 205, 0, 5]);
        assert_eq!(&header[4..], &7u32.to_be_bytes());
        assert_eq!(&body[..4], &9u32.to_be_bytes());
        assert_eq!(&body[4..8], &[0, 10, 0, 0b0000_0101]);
        assert_eq!(&body[8..12], &[0, 40, 0, 0]);
        assert_eq!(
            &body[12..16],
            &[0xff, 0xff, 0, 1],
            "65535 then 0 across the wrap"
        );
        assert!(nack(7, 9, &[]).is_none());

        let block = ReportBlock {
            ssrc: 5,
            fraction_lost: 64,
            cumulative_lost: -2,
            highest: 70_000,
            jitter: 3,
            last_sender_report: 0x1111,
            delay: 0x2222,
        };
        let (header, body) = receiver_report(77, &[block]);
        assert_eq!(&header[..4], &[0x81, 201, 0, 7]);
        assert_eq!(body.len(), 24);
        // Our own parser agrees with the builder: the loss of source 5 is 64 over 256.
        let mut compound = header.to_vec();
        compound.extend(&body);
        assert_eq!(feedback(&compound, 5).unwrap().loss, Some(64));
        assert_eq!(outbound_loss(&compound, 5), Some(25.));
        assert_eq!(&body[5..8], &[0xff, 0xff, 0xfe], "signed 24-bit total");
    }

    #[test]
    fn viewer_feedback_is_parsed_for_our_source_only() {
        let ours = 0x0000_0102u32;
        let mut compound = Vec::new();
        let (header, body) = pli(9, ours);
        compound.extend(header);
        compound.extend(body);
        let (header, body) = nack(9, ours, &[100, 101, 117]).unwrap();
        compound.extend(header);
        compound.extend(&body);
        let (header, body) = nack(9, ours + 1, &[500]).unwrap();
        compound.extend(header);
        compound.extend(&body);
        let found = feedback(&compound, ours).unwrap();
        assert!(found.keyframe);
        assert_eq!(found.nacks, [100, 101, 117]);
        assert_eq!(found.bitrate, None);
        // A PLI for someone else's source asks nothing of us.
        let (header, body) = pli(9, ours + 5);
        let mut other = header.to_vec();
        other.extend(body);
        assert!(!feedback(&other, ours).unwrap().keyframe);

        // REMB: 8 bytes unique id + exponent 4, mantissa 1000 -> 16000 bit/s, naming our source.
        let mut remb = vec![0x8f, 206, 0, 5];
        remb.extend(9u32.to_be_bytes());
        remb.extend(0u32.to_be_bytes());
        remb.extend(b"REMB");
        remb.extend([
            1,
            (4 << 2) | (1000 >> 16) as u8,
            (1000 >> 8) as u8,
            1000u16 as u8,
        ]);
        remb.extend(ours.to_be_bytes());
        assert_eq!(feedback(&remb, ours).unwrap().bitrate, Some(16_000));

        // Garbage after a good packet discards the lot, and a zero SSRC is never ours.
        compound.extend([0x80, 201, 0]);
        assert!(feedback(&compound, ours).is_none());
        assert!(feedback(&remb, 0).is_none());
    }

    #[test]
    fn feedback_travels_sealed_and_only_authenticated_counts() {
        let key = [3; 32];
        let mut viewer = Transport::new(Mode::Aes256GcmRtpSize, &key);
        let sender = Transport::new(Mode::Aes256GcmRtpSize, &key);
        let (header, body) = pli(9, 55);
        let wire = viewer.seal_rtcp(&header, &body).unwrap();
        assert!(sender.feedback(&wire, 55).unwrap().keyframe);
        let mut forged = wire.clone();
        let last = forged.len() - 6;
        forged[last] ^= 1;
        assert!(sender.feedback(&forged, 55).is_none());
    }

    #[test]
    fn the_transport_wide_sequence_number_is_read_from_extension_5_only() {
        let transport = Transport::new(Mode::Aes256GcmRtpSize, &[7; 32]);
        // Audio level (id 1, one byte), padding, then id 5 with two bytes: 0x1234.
        let mut header = video_header(H264_PAYLOAD_TYPE, true, 8, 90_000, 42).to_vec();
        header[0] |= 0x10;
        header.extend([0xbe, 0xde, 0, 2]);
        let suffix = [0, 0, 0, 1];
        let body = [0x10, 0x7f, 0x00, 0x51, 0x12, 0x34, 0, 0, b'h', b'2'];
        let sealed = transport.cipher.seal(suffix, &header, &body).unwrap();
        let mut wire = header.clone();
        wire.extend(sealed);
        wire.extend(suffix);
        let rtp = transport.open(&wire).unwrap();
        assert_eq!(rtp.transport_sequence, Some(0x1234));
        assert_eq!(rtp.payload, b"h2");
        // Other ids, a wrong length, a two-byte profile or a cut element carry none.
        assert_eq!(transport_sequence([0xbe, 0xde], &[0x20, 1, 2]), None);
        assert_eq!(transport_sequence([0xbe, 0xde], &[0x52, 1, 2, 3]), None);
        assert_eq!(transport_sequence([0x10, 0x00], &[0x51, 1, 2]), None);
        assert_eq!(transport_sequence([0xbe, 0xde], &[0x51, 1]), None);
        assert_eq!(transport_sequence([0xbe, 0xde], &[0xf0, 0x51, 1, 2]), None);
        // A packet without extensions has none either.
        let mut sender = Transport::new(Mode::Aes256GcmRtpSize, &[7; 32]);
        let plain = sender.seal_packet(&super::header(1, 960, 9), b"a").unwrap();
        assert_eq!(transport.open(&plain).unwrap().transport_sequence, None);
    }

    #[test]
    fn sender_reports_give_their_ssrc_and_middle_ntp_bits() {
        let mut report = vec![0x80, 200, 0, 6];
        report.extend(55u32.to_be_bytes());
        report.extend([0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
        report.extend([0; 12]);
        let mut compound = report.clone();
        compound.extend(pli(9, 55).0);
        compound.extend(pli(9, 55).1);
        assert_eq!(sender_reports(&compound), [(55, 0x3344_5566)]);
        compound.push(0x80);
        assert!(sender_reports(&compound).is_empty(), "malformed tail");
    }

    #[test]
    fn soundshare_audio_carries_an_encrypted_extension_that_open_strips() {
        let mut transport = Transport::new(Mode::XChaCha20Poly1305RtpSize, &[7; 32]);
        let wire = transport
            .seal_soundshare(&header(3, 960, 8), b"opus", Instant::now())
            .unwrap();
        assert_eq!(wire[0] & 0x10, 0x10, "extension bit");
        assert_eq!(&wire[12..16], &[0xbe, 0xde, 0, 2], "preamble in the clear");
        let rtp = transport.open(&wire).unwrap();
        assert_eq!(
            (rtp.payload_type, rtp.payload.as_slice()),
            (120, &b"opus"[..])
        );
    }

    #[test]
    fn every_sent_packet_is_numbered_on_one_transport_wide_counter() {
        let now = Instant::now();
        let mut sender = Transport::new(Mode::Aes256GcmRtpSize, &[5; 32]);
        let receiver = Transport::new(Mode::Aes256GcmRtpSize, &[5; 32]);
        let audio = sender.seal(&header(1, 960, 9), b"opus", now).unwrap();
        let video = sender
            .seal(
                &video_header(H264_PAYLOAD_TYPE, true, 7, 90, 10),
                b"h264",
                now,
            )
            .unwrap();
        let rtx = sender
            .seal(
                &video_header(H264_RTX_PAYLOAD_TYPE, false, 3, 90, 11),
                b"x",
                now,
            )
            .unwrap();
        let share = sender
            .seal_soundshare(&header(2, 1920, 9), b"music", now)
            .unwrap();
        // Preamble in the clear, one word of elements (two with the soundshare element), then the sealed rest.
        assert_eq!(&audio[12..16], &[0xbe, 0xde, 0, 1]);
        assert_eq!(&share[12..16], &[0xbe, 0xde, 0, 2]);
        assert_eq!(audio.len(), 16 + 4 + 4 + 16 + 4);
        for (n, (wire, payload)) in [
            (audio, &b"opus"[..]),
            (video, b"h264"),
            (rtx, b"x"),
            (share, b"music"),
        ]
        .into_iter()
        .enumerate()
        {
            let rtp = receiver.open(&wire).unwrap();
            assert_eq!(rtp.transport_sequence, Some(n as u16));
            assert_eq!(rtp.payload, payload);
        }
        assert_eq!(sender.sent.next(), 4);

        // The feedback that comes back is parsed whatever media SSRC it names, next to the source's own feedback.
        let mut compound = vec![0x8f, 205, 0, 5, 0, 0, 0, 1, 0, 0, 0, 99, 0, 0, 0, 1];
        compound.extend([0, 0, 0, 0, 0x20, 1, 4, 0]);
        compound.extend(pli(1, 10).0);
        compound.extend(pli(1, 10).1);
        let found = feedback(&compound, 10).unwrap();
        assert!(found.keyframe);
        assert_eq!(found.transport.len(), 1);
        assert_eq!(found.transport[0].arrivals, [Some(1000)]);
    }

    #[test]
    fn padded_transport_cc_feedback_parses_standalone_and_in_compound_rtcp() {
        // Discord's SFU pads Transport-CC packets (PT 205, FMT 15) with RFC 3550 P=1 (0xaf) whenever
        // the unpadded 1-byte-delta payload does not land on a 4-byte boundary (1, 2, or 3 padding bytes).
        for (deltas, expected_pad) in [
            (&[4u8][..], 1usize),            // 20 + 2 + 1 = 23 -> +1 = 24
            (&[4u8, 8, 12, 16][..], 2usize), // 20 + 2 + 4 = 26 -> +2 = 28
            (&[4u8, 8, 12][..], 3usize),     // 20 + 2 + 3 = 25 -> +3 = 28
        ] {
            let count = deltas.len() as u16;
            let unpadded = 20 + 2 + deltas.len();
            let actual_pad = (4 - (unpadded % 4)) % 4;
            assert_eq!(actual_pad, expected_pad);
            let total = unpadded + actual_pad;
            let words = (total / 4 - 1) as u16;
            let mut pkt = vec![0xaf, 205];
            pkt.extend(words.to_be_bytes());
            pkt.extend(1u32.to_be_bytes()); // sender SSRC
            pkt.extend(99u32.to_be_bytes()); // media SSRC
            pkt.extend(42u16.to_be_bytes()); // base seq
            pkt.extend(count.to_be_bytes()); // status count
            pkt.extend([0, 0, 1, 7]); // ref time = 1, fb count = 7
            // Run-length chunk: symbol 1 (small delta) * count.
            pkt.extend((0x2000u16 | count).to_be_bytes());
            pkt.extend_from_slice(deltas);
            pkt.extend(std::iter::repeat_n(0u8, actual_pad - 1));
            pkt.push(actual_pad as u8);
            assert_eq!(pkt.len(), total);

            // Standalone padded TWCC packet.
            let parsed = feedback(&pkt, 10).expect("standalone padded TWCC must parse");
            assert_eq!(parsed.transport.len(), 1);
            assert_eq!(parsed.transport[0].base, 42);
            assert_eq!(parsed.transport[0].arrivals.len(), deltas.len());

            // Compound RTCP: padded TWCC followed by a PLI for our video SSRC.
            let mut compound = pkt.clone();
            let (pli_hdr, pli_body) = pli(1, 10);
            compound.extend(pli_hdr);
            compound.extend(pli_body);
            let parsed = feedback(&compound, 10).expect("compound padded TWCC + PLI must parse");
            assert!(parsed.keyframe);
            assert_eq!(parsed.transport.len(), 1);
            assert_eq!(parsed.transport[0].arrivals.len(), deltas.len());
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
