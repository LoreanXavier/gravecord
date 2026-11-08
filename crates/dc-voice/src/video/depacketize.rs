//! RTP payloads of one H.264 source back into Annex B access units (RFC 6184: single NAL, STAP-A, FU-A).
//!
//! Adapted from Serein `crates/discord-voice/src/video_receive.rs` (`Assembler`, H.264 part only), MIT OR Apache-2.0.
//! The access units are still DAVE ciphertext here; the decryption keeps the NAL structure intact.

/// An access unit larger than this is a corrupt or hostile stream.
pub(crate) const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024 + 64 * 1024;
const START_CODE: [u8; 4] = [0, 0, 0, 1];

#[derive(Default)]
pub(crate) struct Assembler {
    timestamp: u32,
    next_sequence: Option<u16>,
    frame: Vec<u8>,
    fragmenting: bool,
    broken: bool,
    started: bool,
    /// Loss survives frame resets until the receiver has asked for recovery.
    pub lost: bool,
}

impl Assembler {
    /// Feeds the next packet in sequence order. Returns the finished access unit at a marker.
    pub fn push(
        &mut self,
        sequence: u16,
        timestamp: u32,
        marker: bool,
        payload: &[u8],
    ) -> Option<Vec<u8>> {
        // Empty payloads are padding or keepalives: they use a sequence number and carry no picture.
        if payload.is_empty() {
            if self
                .next_sequence
                .is_some_and(|expected| expected != sequence)
            {
                self.broken = true;
                self.lost = true;
            }
            self.next_sequence = Some(sequence.wrapping_add(1));
            return None;
        }
        if self.started && timestamp != self.timestamp {
            // A new picture began before the previous marker: the partial one is gone.
            self.lost = true;
            self.reset_frame();
            self.timestamp = timestamp;
        }
        if !self.started {
            self.started = true;
            self.timestamp = timestamp;
        }
        if self
            .next_sequence
            .is_some_and(|expected| expected != sequence)
        {
            self.broken = true;
        }
        self.next_sequence = Some(sequence.wrapping_add(1));
        if !self.broken && self.append(payload).is_err() {
            self.broken = true;
        }
        self.lost |= self.broken;
        if !marker {
            return None;
        }
        let complete = (!self.broken && !self.fragmenting && !self.frame.is_empty())
            .then(|| std::mem::take(&mut self.frame));
        self.lost |= complete.is_none();
        self.reset_frame();
        self.started = false;
        complete
    }

    fn reset_frame(&mut self) {
        self.frame.clear();
        if self.frame.capacity() > 256 * 1024 {
            self.frame = Vec::new();
        }
        self.fragmenting = false;
        self.broken = false;
    }

    fn extend(&mut self, parts: &[&[u8]]) -> Result<(), ()> {
        let total: usize = parts.iter().map(|part| part.len()).sum();
        if self.frame.len() + total > MAX_FRAME_BYTES {
            return Err(());
        }
        for part in parts {
            self.frame.extend_from_slice(part);
        }
        Ok(())
    }

    fn append(&mut self, payload: &[u8]) -> Result<(), ()> {
        let Some(&indicator) = payload.first() else {
            return Err(());
        };
        match indicator & 0x1f {
            1..=23 => {
                if self.fragmenting {
                    return Err(());
                }
                self.extend(&[&START_CODE, payload])
            }
            24 => {
                if self.fragmenting {
                    return Err(());
                }
                let mut at = 1;
                while at < payload.len() {
                    if at + 2 > payload.len() {
                        return Err(());
                    }
                    let size = usize::from(u16::from_be_bytes([payload[at], payload[at + 1]]));
                    at += 2;
                    if size == 0 || at + size > payload.len() {
                        return Err(());
                    }
                    self.extend(&[&START_CODE, &payload[at..at + size]])?;
                    at += size;
                }
                Ok(())
            }
            28 => {
                if payload.len() < 2 {
                    return Err(());
                }
                let header = payload[1];
                let start = header & 0x80 != 0;
                let end = header & 0x40 != 0;
                if start == self.fragmenting {
                    return Err(());
                }
                if start {
                    let nal_header = (indicator & 0xe0) | (header & 0x1f);
                    self.extend(&[&START_CODE, &[nal_header], &payload[2..]])?;
                } else {
                    self.extend(&[&payload[2..]])?;
                }
                self.fragmenting = !end;
                Ok(())
            }
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_stap_and_fragmented_nal_units_rebuild_annex_b() {
        let mut assembler = Assembler::default();
        // Single NAL (type 1), a STAP-A of two, and a FU-A of type 5 in three parts, one access unit.
        assert_eq!(assembler.push(1, 90, false, &[0x41, 9, 9]), None);
        let mut stap = vec![0x18];
        for nal in [&[0x67u8, 1][..], &[0x68, 2, 3][..]] {
            stap.extend((nal.len() as u16).to_be_bytes());
            stap.extend(nal);
        }
        assert_eq!(assembler.push(2, 90, false, &stap), None);
        assert_eq!(assembler.push(3, 90, false, &[0x7c, 0x85, 1, 2]), None);
        assert_eq!(assembler.push(4, 90, false, &[0x7c, 0x05, 3]), None);
        let unit = assembler.push(5, 90, true, &[0x7c, 0x45, 4]).unwrap();
        let expected: Vec<u8> = [
            &[0, 0, 0, 1, 0x41, 9, 9][..],
            &[0, 0, 0, 1, 0x67, 1],
            &[0, 0, 0, 1, 0x68, 2, 3],
            &[0, 0, 0, 1, 0x65, 1, 2, 3, 4],
        ]
        .concat();
        assert_eq!(unit, expected);
        assert!(!assembler.lost);
    }

    #[test]
    fn a_gap_or_a_bad_fragment_drops_the_picture_and_flags_loss() {
        let mut assembler = Assembler::default();
        assert_eq!(assembler.push(10, 1, false, &[0x7c, 0x85, 1]), None);
        // Sequence 11 never arrived.
        assert_eq!(assembler.push(12, 1, true, &[0x7c, 0x45, 2]), None);
        assert!(assembler.lost);
        // The next picture is clean again but loss stays flagged until the caller clears it.
        let unit = assembler.push(13, 2, true, &[0x65, 7]).unwrap();
        assert_eq!(unit, [0, 0, 0, 1, 0x65, 7]);
        assert!(assembler.lost);
        assembler.lost = false;

        // A new timestamp before the marker abandons the partial picture.
        let mut assembler = Assembler::default();
        assembler.push(1, 1, false, &[0x41, 1]);
        assert_eq!(
            assembler.push(2, 2, true, &[0x41, 2]).unwrap(),
            [0, 0, 0, 1, 0x41, 2]
        );
        assert!(assembler.lost);

        // Unsupported types, truncated STAP-A and a fragment end without a start.
        for payload in [&[0x1d, 0][..], &[0x18, 0, 5, 1][..], &[0x7c, 0x45, 1][..]] {
            let mut assembler = Assembler::default();
            assert_eq!(assembler.push(1, 1, true, payload), None);
            assert!(assembler.lost);
        }
    }

    #[test]
    fn padding_keeps_the_chain_when_in_order_and_breaks_it_when_not() {
        let mut assembler = Assembler::default();
        assembler.push(1, 1, false, &[0x41, 1]);
        assert_eq!(assembler.push(2, 1, false, &[]), None);
        assert_eq!(
            assembler.push(3, 1, true, &[0x41, 2]).unwrap(),
            [0, 0, 0, 1, 0x41, 1, 0, 0, 0, 1, 0x41, 2]
        );
        assert!(!assembler.lost);
        assembler.push(4, 2, false, &[]);
        assembler.push(9, 2, false, &[]);
        assert!(assembler.lost);
    }
}
