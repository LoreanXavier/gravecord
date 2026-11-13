//! The server's end of the media transport: `rtpsize` AEAD opening and sealing of RTP and RTCP, the IP discovery reply,
//! and the RTCP an SFU reads from and writes to its participants.
//!
//! Written from the client's side in `crates/dc-voice/src/rtp.rs` (the layout is symmetric: the clear RTP header and
//! extension preamble, or the first eight RTCP bytes, are the associated data; a 32-bit counter nonce trails and leads
//! an otherwise zero cipher nonce). It is a second implementation on purpose: the client's packets must open here
//! without sharing its code.
use aes_gcm::Aes256Gcm;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305,
    aead::{Aead, Payload},
};

pub(crate) const OPUS: u8 = 120;
pub(crate) const H264: u8 = 101;
pub(crate) const H264_RTX: u8 = 102;
/// The transport-wide sequence number's one-byte extension id.
pub(crate) const TRANSPORT_SEQUENCE: u8 = 5;
const TAG: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Aes256Gcm,
    XChaCha20,
}

impl Mode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Aes256Gcm => "aead_aes256_gcm_rtpsize",
            Self::XChaCha20 => "aead_xchacha20_poly1305_rtpsize",
        }
    }
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "aead_aes256_gcm_rtpsize" => Some(Self::Aes256Gcm),
            "aead_xchacha20_poly1305_rtpsize" => Some(Self::XChaCha20),
            _ => None,
        }
    }
}

enum Cipher {
    Aes(Box<Aes256Gcm>),
    XChaCha(Box<XChaCha20Poly1305>),
}

/// One participant's transport key. Sealing counts its own nonces (server to participant).
pub(crate) struct Crypto {
    cipher: Cipher,
    counter: u32,
}

/// An opened RTP packet: the clear header (with the extension preamble, when there is one), the decrypted extension
/// elements, and the payload as sent (DAVE ciphertext, with any padding).
#[derive(Clone, Debug)]
pub(crate) struct Opened {
    pub header: Vec<u8>,
    pub elements: Vec<u8>,
    pub payload: Vec<u8>,
    pub payload_type: u8,
    pub sequence: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub transport_sequence: Option<u16>,
}

impl Crypto {
    pub(crate) fn new(mode: Mode, key: &[u8; 32]) -> Self {
        let cipher = match mode {
            Mode::Aes256Gcm => Cipher::Aes(Box::new(Aes256Gcm::new(key.into()))),
            Mode::XChaCha20 => Cipher::XChaCha(Box::new(XChaCha20Poly1305::new(key.into()))),
        };
        Self { cipher, counter: 0 }
    }

    fn crypt(&self, encrypt: bool, suffix: [u8; 4], aad: &[u8], msg: &[u8]) -> Option<Vec<u8>> {
        let payload = Payload { msg, aad };
        match &self.cipher {
            Cipher::Aes(cipher) => {
                let mut nonce = aes_gcm::Nonce::default();
                nonce[..4].copy_from_slice(&suffix);
                if encrypt {
                    cipher.encrypt(&nonce, payload).ok()
                } else {
                    cipher.decrypt(&nonce, payload).ok()
                }
            }
            Cipher::XChaCha(cipher) => {
                let mut nonce = chacha20poly1305::XNonce::default();
                nonce[..4].copy_from_slice(&suffix);
                if encrypt {
                    cipher.encrypt(&nonce, payload).ok()
                } else {
                    cipher.decrypt(&nonce, payload).ok()
                }
            }
        }
    }

    /// `aad` stays clear, `body` is sealed, the nonce trails.
    pub(crate) fn seal(&mut self, aad: &[u8], body: &[u8]) -> Option<Vec<u8>> {
        self.counter = self.counter.checked_add(1)?;
        let suffix = self.counter.to_be_bytes();
        let sealed = self.crypt(true, suffix, aad, body)?;
        let mut out = Vec::with_capacity(aad.len() + sealed.len() + 4);
        out.extend_from_slice(aad);
        out.extend_from_slice(&sealed);
        out.extend_from_slice(&suffix);
        Some(out)
    }

    pub(crate) fn open_rtp(&self, packet: &[u8]) -> Option<Opened> {
        if packet.len() < 12 + TAG + 4 || packet[0] >> 6 != 2 {
            return None;
        }
        let payload_type = packet[1] & 0x7f;
        if !matches!(payload_type, OPUS | H264 | H264_RTX) {
            return None;
        }
        let csrc_end = 12 + usize::from(packet[0] & 15) * 4;
        let extended = packet[0] & 0x10 != 0;
        let header_len = csrc_end + if extended { 4 } else { 0 };
        if header_len + TAG + 4 > packet.len() {
            return None;
        }
        let suffix: [u8; 4] = packet[packet.len() - 4..].try_into().ok()?;
        let mut clear = self.crypt(
            false,
            suffix,
            &packet[..header_len],
            &packet[header_len..packet.len() - 4],
        )?;
        let mut elements = Vec::new();
        if extended {
            let words = u16::from_be_bytes([packet[csrc_end + 2], packet[csrc_end + 3]]);
            let length = usize::from(words) * 4;
            if length > clear.len() {
                return None;
            }
            elements = clear.drain(..length).collect();
        }
        let transport_sequence = (extended && packet[csrc_end..csrc_end + 2] == [0xbe, 0xde])
            .then(|| find_element(&elements, TRANSPORT_SEQUENCE))
            .flatten()
            .and_then(|(at, length)| {
                (length == 2).then(|| u16::from_be_bytes([elements[at], elements[at + 1]]))
            });
        Some(Opened {
            header: packet[..header_len].to_vec(),
            elements,
            payload: clear,
            payload_type,
            sequence: u16::from_be_bytes([packet[2], packet[3]]),
            timestamp: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
            ssrc: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
            transport_sequence,
        })
    }

    /// Seals an opened packet for another participant; its transport-wide sequence number becomes `transport` (each
    /// hop numbers its own packets, as an SFU does).
    pub(crate) fn seal_rtp(&mut self, packet: &Opened, transport: Option<u16>) -> Option<Vec<u8>> {
        let mut elements = packet.elements.clone();
        if let (Some(sequence), Some((at, 2))) =
            (transport, find_element(&elements, TRANSPORT_SEQUENCE))
        {
            elements[at..at + 2].copy_from_slice(&sequence.to_be_bytes());
        }
        let mut body = elements;
        body.extend_from_slice(&packet.payload);
        self.seal(&packet.header, &body)
    }

    pub(crate) fn open_rtcp(&self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.len() < 8 + TAG + 4 || !is_rtcp(packet) {
            return None;
        }
        let suffix: [u8; 4] = packet[packet.len() - 4..].try_into().ok()?;
        let body = self.crypt(false, suffix, &packet[..8], &packet[8..packet.len() - 4])?;
        let mut compound = packet[..8].to_vec();
        compound.extend(body);
        Some(compound)
    }

    /// One RTCP packet (or a compound): the first eight bytes are clear.
    pub(crate) fn seal_rtcp(&mut self, compound: &[u8]) -> Option<Vec<u8>> {
        if compound.len() < 8 {
            return None;
        }
        self.seal(&compound[..8], &compound[8..])
    }
}

/// Where element `id`'s data starts in a one-byte extension block, and its length.
fn find_element(block: &[u8], id: u8) -> Option<(usize, usize)> {
    let mut at = 0;
    while at < block.len() {
        let first = block[at];
        let element = first >> 4;
        if element == 0 {
            at += 1;
            continue;
        }
        if element == 15 {
            return None;
        }
        let length = usize::from(first & 0x0f) + 1;
        if at + 1 + length > block.len() {
            return None;
        }
        if element == id {
            return Some((at + 1, length));
        }
        at += 1 + length;
    }
    None
}

pub(crate) fn is_rtcp(packet: &[u8]) -> bool {
    packet.len() >= 2 && packet[0] >> 6 == 2 && (192..=223).contains(&packet[1])
}

/// The discovery request: type 1, length 70, the SSRC.
pub(crate) fn discovery_ssrc(packet: &[u8]) -> Option<u32> {
    (packet.len() == 74 && packet[..4] == [0, 1, 0, 70])
        .then(|| u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]))
}

/// The discovery reply: type 2, length 70, the SSRC, the address as seen, NUL padded, the port.
pub(crate) fn discovery_reply(ssrc: u32, from: std::net::SocketAddr) -> [u8; 74] {
    let mut reply = [0u8; 74];
    reply[..4].copy_from_slice(&[0, 2, 0, 70]);
    reply[4..8].copy_from_slice(&ssrc.to_be_bytes());
    let address = from.ip().to_string();
    reply[8..8 + address.len().min(63)]
        .copy_from_slice(&address.as_bytes()[..address.len().min(63)]);
    reply[72..].copy_from_slice(&from.port().to_be_bytes());
    reply
}

/// The client's UDP keepalive (Serein's `0x1337cafe` ping).
pub(crate) fn is_keepalive(packet: &[u8]) -> bool {
    packet.len() == 8 && packet[..4] == [0x13, 0x37, 0xca, 0xfe]
}

/// One packet of a compound RTCP datagram.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Rtcp<'a> {
    SenderReport {
        ssrc: u32,
        ntp_middle: u32,
        raw: &'a [u8],
    },
    ReceiverReport,
    /// PLI or NACK about `media` (whole packet, for relaying).
    Pli {
        media: u32,
        raw: &'a [u8],
    },
    Nack {
        media: u32,
        raw: &'a [u8],
    },
    TransportFeedback,
    Remb,
    Other,
}

/// The packets of a decrypted compound, or `None` when it is malformed.
pub(crate) fn parse_compound(mut remaining: &[u8]) -> Option<Vec<Rtcp<'_>>> {
    let mut out = Vec::new();
    while !remaining.is_empty() {
        if remaining.len() < 4 || !is_rtcp(remaining) {
            return None;
        }
        let size = (usize::from(u16::from_be_bytes([remaining[2], remaining[3]])) + 1) * 4;
        if size > remaining.len() {
            return None;
        }
        let raw = &remaining[..size];
        let count = raw[0] & 0x1f;
        let media =
            || (raw.len() >= 12).then(|| u32::from_be_bytes([raw[8], raw[9], raw[10], raw[11]]));
        out.push(match (raw[1], count) {
            (200, _) if raw.len() >= 28 => Rtcp::SenderReport {
                ssrc: u32::from_be_bytes([raw[4], raw[5], raw[6], raw[7]]),
                ntp_middle: u32::from_be_bytes([raw[10], raw[11], raw[12], raw[13]]),
                raw,
            },
            (201, _) => Rtcp::ReceiverReport,
            (206, 1) => Rtcp::Pli {
                media: media()?,
                raw,
            },
            (205, 1) => Rtcp::Nack {
                media: media()?,
                raw,
            },
            (205, 15) => Rtcp::TransportFeedback,
            (206, 15) => Rtcp::Remb,
            _ => Rtcp::Other,
        });
        remaining = &remaining[size..];
    }
    Some(out)
}

/// A reception report about one source (RFC 3550 6.4.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Block {
    pub ssrc: u32,
    pub fraction_lost: u8,
    pub cumulative_lost: i32,
    pub highest: u32,
    pub jitter: u32,
    pub last_sender_report: u32,
    pub delay: u32,
}

/// A receiver report from `sender` (the server's own SSRC) with up to 31 blocks.
pub(crate) fn receiver_report(sender: u32, blocks: &[Block]) -> Vec<u8> {
    let blocks = &blocks[..blocks.len().min(31)];
    let mut packet = vec![0x80 | blocks.len() as u8, 201];
    packet.extend(((1 + 6 * blocks.len()) as u16).to_be_bytes());
    packet.extend(sender.to_be_bytes());
    for block in blocks {
        packet.extend(block.ssrc.to_be_bytes());
        let loss =
            (u32::from(block.fraction_lost) << 24) | (block.cumulative_lost as u32 & 0x00ff_ffff);
        for word in [
            loss,
            block.highest,
            block.jitter,
            block.last_sender_report,
            block.delay,
        ] {
            packet.extend(word.to_be_bytes());
        }
    }
    packet
}

/// REMB (draft-alvestrand-rmcat-remb): `bitrate` for the sources `ssrcs`, from `sender`.
pub(crate) fn remb(sender: u32, bitrate: u64, ssrcs: &[u32]) -> Vec<u8> {
    let mut exponent = 0u32;
    while (bitrate >> exponent) >= (1 << 18) {
        exponent += 1;
    }
    let mantissa = (bitrate >> exponent) as u32;
    let mut packet = vec![0x8f, 206];
    packet.extend(((4 + ssrcs.len()) as u16).to_be_bytes());
    packet.extend(sender.to_be_bytes());
    packet.extend(0u32.to_be_bytes());
    packet.extend(b"REMB");
    packet.push(ssrcs.len() as u8);
    packet.push(((exponent << 2) as u8) | ((mantissa >> 16) as u8 & 3));
    packet.push((mantissa >> 8) as u8);
    packet.push(mantissa as u8);
    for ssrc in ssrcs {
        packet.extend(ssrc.to_be_bytes());
    }
    packet
}

/// A picture loss indication from `sender` about `media`.
#[cfg(test)]
pub(crate) fn pli(sender: u32, media: u32) -> Vec<u8> {
    let mut packet = vec![0x81, 206, 0, 2];
    packet.extend(sender.to_be_bytes());
    packet.extend(media.to_be_bytes());
    packet
}

/// Per-source receive statistics for the receiver reports the server sends a sender (RFC 3550 A.3 and A.8).
#[derive(Clone, Debug)]
pub(crate) struct SourceStats {
    clock: u32,
    base: Option<u16>,
    cycles: u32,
    max: u16,
    received: u64,
    expected_prior: u64,
    received_prior: u64,
    transit: Option<i64>,
    jitter: f64,
    pub last_sender_report: Option<(u32, std::time::Instant)>,
    started: std::time::Instant,
}

impl SourceStats {
    pub(crate) fn new(clock: u32) -> Self {
        Self {
            clock,
            base: None,
            cycles: 0,
            max: 0,
            received: 0,
            expected_prior: 0,
            received_prior: 0,
            transit: None,
            jitter: 0.0,
            last_sender_report: None,
            started: std::time::Instant::now(),
        }
    }

    pub(crate) fn observe(&mut self, sequence: u16, timestamp: u32, now: std::time::Instant) {
        self.received += 1;
        match self.base {
            None => {
                self.base = Some(sequence);
                self.max = sequence;
            }
            Some(_) => {
                let delta = sequence.wrapping_sub(self.max);
                if delta != 0 && delta < 0x8000 {
                    if sequence < self.max {
                        self.cycles += 1 << 16;
                    }
                    self.max = sequence;
                }
            }
        }
        let arrival = (now.duration_since(self.started).as_micros() as i64) * i64::from(self.clock)
            / 1_000_000;
        let transit = arrival - i64::from(timestamp as i32);
        if let Some(previous) = self.transit {
            let d = (transit - previous).abs() as f64;
            self.jitter += (d - self.jitter) / 16.0;
        }
        self.transit = Some(transit);
    }

    /// The report block now, and the interval's counters start again.
    pub(crate) fn block(&mut self, ssrc: u32, now: std::time::Instant) -> Option<Block> {
        let base = self.base?;
        let highest = self.cycles + u32::from(self.max);
        let expected = u64::from(highest - u32::from(base)) + 1;
        let lost = expected as i64 - self.received as i64;
        let expected_interval = expected - self.expected_prior;
        let received_interval = self.received - self.received_prior;
        self.expected_prior = expected;
        self.received_prior = self.received;
        let lost_interval = expected_interval as i64 - received_interval as i64;
        let fraction_lost = if expected_interval == 0 || lost_interval <= 0 {
            0
        } else {
            ((lost_interval << 8) / expected_interval as i64).min(255) as u8
        };
        let (last_sender_report, delay) = self.last_sender_report.map_or((0, 0), |(middle, at)| {
            (
                middle,
                (now.duration_since(at).as_micros() as u64 * 65_536 / 1_000_000) as u32,
            )
        });
        Some(Block {
            ssrc,
            fraction_lost,
            cumulative_lost: lost.clamp(-0x80_0000, 0x7f_ffff) as i32,
            highest,
            jitter: self.jitter as u32,
            last_sender_report,
            delay,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_server_seals_the_client_parses() {
        // The client's parser reads the server's REMB and PLI.
        let mut compound = remb(1, 2_500_000, &[77]);
        compound.extend(pli(1, 77));
        let (keyframe, nacks, bitrate, transport) =
            dc_voice::sim::parse_feedback(&compound, 77).unwrap();
        assert!(keyframe);
        assert!(nacks.is_empty() && transport.is_empty());
        // 2.5 Mb/s needs an exponent: the mantissa keeps 18 bits.
        let bitrate = bitrate.unwrap();
        assert!(bitrate <= 2_500_000 && bitrate > 2_490_000, "{bitrate}");
        let report = receiver_report(
            1,
            &[Block {
                ssrc: 77,
                fraction_lost: 64,
                ..Block::default()
            }],
        );
        assert_eq!(parse_compound(&report).unwrap(), [Rtcp::ReceiverReport]);
    }

    #[test]
    fn rtp_round_trips_and_the_transport_number_is_rewritten() {
        for mode in [Mode::Aes256Gcm, Mode::XChaCha20] {
            let key = [5u8; 32];
            let mut client = Crypto::new(mode, &key);
            // A header with the one-byte extension preamble, element 5 = 0x0102, then the payload.
            let mut header = vec![0x90, 101, 0, 7, 0, 0, 0, 9, 0, 0, 0, 42, 0xbe, 0xde, 0, 1];
            let body = [0x51, 1, 2, 0, b'd', b'a', b'v', b'e'];
            let wire = client.seal(&header, &body).unwrap();
            let server = Crypto::new(mode, &key);
            let opened = server.open_rtp(&wire).unwrap();
            assert_eq!(opened.transport_sequence, Some(0x0102));
            assert_eq!(
                (opened.ssrc, opened.sequence, opened.payload_type),
                (42, 7, 101)
            );
            assert_eq!(opened.payload, b"dave");
            let mut out = Crypto::new(mode, &key);
            let forwarded = out.seal_rtp(&opened, Some(0x0a0b)).unwrap();
            let again = server.open_rtp(&forwarded).unwrap();
            assert_eq!(again.transport_sequence, Some(0x0a0b));
            assert_eq!(again.payload, b"dave");
            header[1] = 96;
            let other = client.seal(&header, &body).unwrap();
            assert!(
                server.open_rtp(&other).is_none(),
                "payload types the client never offers"
            );
            let mut tampered = wire.clone();
            tampered[20] ^= 1;
            assert!(server.open_rtp(&tampered).is_none());
        }
    }

    #[test]
    fn reports_count_loss_and_jitter_of_the_interval() {
        let mut stats = SourceStats::new(90_000);
        let start = std::time::Instant::now();
        for sequence in (65_530u16..=65_535).chain(0..10).filter(|s| *s != 3) {
            stats.observe(sequence, 0, start);
        }
        let block = stats.block(9, start).unwrap();
        assert_eq!(block.highest, (1 << 16) + 9);
        assert_eq!(block.cumulative_lost, 1);
        assert_eq!(block.fraction_lost, (256 / 16) as u8);
        assert_eq!(
            stats.block(9, start).unwrap().fraction_lost,
            0,
            "a quiet interval"
        );
    }

    #[test]
    fn discovery_reply_names_the_address_seen() {
        let reply = discovery_reply(5, "127.0.0.1:50001".parse().unwrap());
        assert_eq!(&reply[..8], &[0, 2, 0, 70, 0, 0, 0, 5]);
        assert_eq!(&reply[8..17], b"127.0.0.1");
        assert_eq!(reply[17], 0);
        assert_eq!(u16::from_be_bytes([reply[72], reply[73]]), 50_001);
        let mut request = [0u8; 74];
        request[..4].copy_from_slice(&[0, 1, 0, 70]);
        request[7] = 5;
        assert_eq!(discovery_ssrc(&request), Some(5));
    }
}
