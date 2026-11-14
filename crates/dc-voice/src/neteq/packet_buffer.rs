//! `PacketBuffer` (libwebrtc `modules/audio_coding/neteq/packet_buffer.cc`): packets waiting to be decoded, sorted by
//! RTP timestamp, at most `max_packets_in_buffer` of them; a full buffer is flushed whole (smart flushing, the
//! `WebRTC-Audio-NetEqSmartFlushing` trial, is off by default and not in the snapshot).
use super::decision::{is_newer_timestamp, is_obsolete_timestamp};

/// One encoded frame: a primary Opus packet (`priority` 0) or the in-band FEC of the one before, carried by the next
/// (`priority` 1, `AudioDecoderOpusImpl::ParsePayload`).
#[derive(Clone)]
pub(crate) struct Packet {
    pub timestamp: u32,
    pub priority: u8,
    pub payload: Vec<u8>,
    /// Samples per channel it decodes to (`EncodedAudioFrame::Duration`).
    pub duration: usize,
    /// The tick it was inserted at (`waiting_time`).
    pub inserted: u64,
}

impl Packet {
    /// `OpusFrame::IsDtxPacket`: two bytes or fewer.
    pub(crate) fn is_dtx(&self) -> bool {
        self.payload.len() <= 2
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Insert {
    Ok,
    /// The buffer was full and was emptied before this packet went in (`kFlushed`).
    Flushed,
}

pub(crate) struct PacketBuffer {
    max_number_of_packets: usize,
    buffer: Vec<Packet>,
    /// Packets thrown away as too old to play (`LogPacketDiscarded`, primary and secondary).
    pub discarded_primary: u64,
    pub discarded_secondary: u64,
}

impl PacketBuffer {
    pub(crate) fn new(max_number_of_packets: usize) -> Self {
        Self {
            max_number_of_packets,
            buffer: Vec::with_capacity(max_number_of_packets),
            discarded_primary: 0,
            discarded_secondary: 0,
        }
    }

    /// Empties the buffer, forwarding removed packets to `recycle` for buffer reuse; returns primary count.
    pub(crate) fn flush_with<F: FnMut(Packet)>(&mut self, mut recycle: F) -> u64 {
        let mut primary = 0;
        for packet in self.buffer.drain(..) {
            primary += u64::from(packet.priority == 0);
            if packet.priority > 0 {
                self.discarded_secondary += 1;
            } else {
                self.discarded_primary += 1;
            }
            recycle(packet);
        }
        primary
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.buffer.len()
    }

    fn log_discarded(&mut self, priority: u8) {
        if priority > 0 {
            self.discarded_secondary += 1;
        } else {
            self.discarded_primary += 1;
        }
    }

    /// `PacketBuffer::InsertPacket` with buffer recycling on flush or duplicate replacement.
    pub(crate) fn insert_with<F: FnMut(Packet)>(
        &mut self,
        packet: Packet,
        mut recycle: F,
    ) -> Insert {
        let mut result = Insert::Ok;
        if self.buffer.len() >= self.max_number_of_packets {
            let _ = self.flush_with(&mut recycle);
            result = Insert::Flushed;
        }
        // The last packet from the back that the new one is not older than.
        let position = self
            .buffer
            .iter()
            .rposition(|old| !is_newer_timestamp(old.timestamp, packet.timestamp));
        if let Some(index) = position
            && self.buffer[index].timestamp == packet.timestamp
        {
            if packet.priority < self.buffer[index].priority {
                let old = std::mem::replace(&mut self.buffer[index], packet);
                self.log_discarded(old.priority);
                recycle(old);
            } else {
                self.log_discarded(packet.priority);
                recycle(packet);
            }
            return result;
        }
        let at = position.map_or(0, |index| index + 1);
        self.buffer.insert(at, packet);
        result
    }

    /// `PacketBuffer::InsertPacket`. For two packets of one timestamp the primary wins over FEC, which is what the
    /// `Packet` ordering by priority does there.
    #[cfg(test)]
    pub(crate) fn insert(&mut self, packet: Packet) -> Insert {
        self.insert_with(packet, |_| {})
    }

    pub(crate) fn peek_next(&self) -> Option<&Packet> {
        self.buffer.first()
    }

    pub(crate) fn pop_next(&mut self) -> Option<Packet> {
        (!self.buffer.is_empty()).then(|| self.buffer.remove(0))
    }

    /// `DiscardOldPackets` in-place (zero allocation), passing discarded packets to `recycle`.
    pub(crate) fn discard_old_packets_with<F: FnMut(Packet)>(
        &mut self,
        timestamp_limit: u32,
        horizon_samples: u32,
        mut recycle: F,
    ) -> u64 {
        let mut primary = 0;
        let mut i = 0;
        while i < self.buffer.len() {
            let obsolete = {
                let packet = &self.buffer[i];
                packet.timestamp != timestamp_limit
                    && is_obsolete_timestamp(packet.timestamp, timestamp_limit, horizon_samples)
            };
            if obsolete {
                let packet = self.buffer.remove(i);
                primary += u64::from(packet.priority == 0);
                self.log_discarded(packet.priority);
                recycle(packet);
            } else {
                i += 1;
            }
        }
        primary
    }

    /// `DiscardOldPackets`: everything older than `timestamp_limit` within `horizon_samples` (0: any age). The number of
    /// primary packets that went: they came too late to be played.
    #[cfg(test)]
    pub(crate) fn discard_old_packets(
        &mut self,
        timestamp_limit: u32,
        horizon_samples: u32,
    ) -> u64 {
        self.discard_old_packets_with(timestamp_limit, horizon_samples, |_| {})
    }

    /// `GetSpanSamples`: from the first packet's timestamp to the end of the last one, or to now counting how long it
    /// has waited (`count_waiting_time`).
    pub(crate) fn span_samples(
        &self,
        last_decoded_length: usize,
        count_waiting_time: bool,
        now: u64,
    ) -> usize {
        let (Some(front), Some(back)) = (self.buffer.first(), self.buffer.last()) else {
            return 0;
        };
        let mut span = back.timestamp.wrapping_sub(front.timestamp) as usize;
        let waiting_samples = ((now - back.inserted) * super::MS_PER_TICK) as usize * 48;
        if count_waiting_time {
            span += waiting_samples;
        } else if back.duration > 0 {
            let mut duration = back.duration;
            if back.is_dtx() {
                duration = duration.max(waiting_samples);
            }
            span += duration;
        } else {
            span += last_decoded_length;
        }
        span
    }

    /// `ContainsDtxOrCngPacket`.
    pub(crate) fn contains_dtx(&self) -> bool {
        self.buffer.iter().any(Packet::is_dtx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(timestamp: u32, priority: u8, tag: u8) -> Packet {
        Packet {
            timestamp,
            priority,
            payload: vec![tag; 10],
            duration: 960,
            inserted: 0,
        }
    }

    #[test]
    fn sorted_by_timestamp_across_the_wrap_with_primary_over_fec() {
        let mut buffer = PacketBuffer::new(200);
        buffer.insert(packet(960, 0, 2));
        buffer.insert(packet(u32::MAX - 959, 0, 0));
        buffer.insert(packet(0, 1, 9));
        // The primary of the same timestamp replaces the FEC copy; a later FEC copy is dropped.
        buffer.insert(packet(0, 0, 1));
        buffer.insert(packet(960, 1, 8));
        let order: Vec<u8> = std::iter::from_fn(|| buffer.pop_next())
            .map(|p| p.payload[0])
            .collect();
        assert_eq!(order, vec![0, 1, 2]);
        assert_eq!(buffer.discarded_secondary, 2);
    }

    #[test]
    fn full_buffer_flushes_and_old_packets_are_discarded() {
        let mut buffer = PacketBuffer::new(4);
        for i in 0..4 {
            assert_eq!(buffer.insert(packet(i * 960, 0, 0)), Insert::Ok);
        }
        assert_eq!(buffer.insert(packet(4 * 960, 0, 0)), Insert::Flushed);
        assert_eq!(buffer.len(), 1);
        buffer.insert(packet(5 * 960, 0, 0));
        assert_eq!(buffer.span_samples(960, false, 0), 1920);
        assert_eq!(buffer.span_samples(960, true, 3), 960 + 30 * 48);
        assert_eq!(buffer.discard_old_packets(5 * 960, 5 * 48_000), 1);
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.discarded_primary, 5);
    }
}
