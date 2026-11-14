//! The receive side of one speaker's Opus stream, ported from libwebrtc's NetEq (`modules/audio_coding/neteq`), which
//! the official engine (`discord_voice.node`, libwebrtc with `neteq_impl.cc`, `delay_constraints.cc`,
//! `packet_buffer.cc`, the `WebRTC-Audio-NetEqDelayManagerConfig` trial) plays every remote voice and soundshare
//! through.
//!
//! Packets go in as they arrive ([`NetEq::insert_packet`], `NetEqImpl::InsertPacket`); every 10 ms one block comes
//! out ([`NetEq::get_audio`], `NetEqImpl::GetAudio`). Arrival times are measured on the playout clock's ticks, as there,
//! so the network's and the speaker's clocks never need to agree: the buffer only has to stay near its target.
//!
//! * Target delay: [`delay::DelayManager`], the 95th percentile of the relative arrival delay over 500 ms intervals,
//!   with a forget factor of 0.983, and a reordering cost term.
//! * Every block: [`decision::DecisionLogic`] picks normal decoding, accelerate (one pitch period out), preemptive
//!   expand (one in), or concealment, against the playout delay and the target.
//! * Loss: the packet after a lost one carries its in-band FEC (`AudioDecoderOpusImpl::ParsePayload` splits it into a
//!   secondary packet at the earlier timestamp); without it, the Opus decoder's own loss concealment
//!   (`NetEqImpl::DoCodecPlc`, as with `WebRTC-Audio-OpusGeneratePlc`, whose name the pinned engine carries).
//!
//! Not ported, and what stands in: NetEq's own `Expand`/`Merge` (LPC-based concealment and its merge into the next
//! packet) — the codec PLC above replaces them, so `Merge` never comes up; `Expand::Muted` — concealment with an empty
//! buffer is muted after [`MUTED_AFTER_SAMPLES`]; `BackgroundNoise` (fixed fallback energy in the speech test of the
//! stretch); RFC 3389 comfort noise and DTMF (not on Discord's audio payload); smart flushing (off by default);
//! NACK (`NackTracker`).
pub(crate) mod arrival;
pub(crate) mod decision;
pub(crate) mod delay;
pub(crate) mod packet_buffer;
pub(crate) mod time_stretch;

#[cfg(test)]
mod bench;

use decision::{DecisionLogic, Mode, NextPacket, Operation, PacketArrivedInfo, Status, is_expand};
use opus2::{Channels, Decoder};
use packet_buffer::{Insert, Packet, PacketBuffer};
use time_stretch::Stretch;

/// The tick timer's period (`TickTimer`'s default `ms_per_tick`): one tick per [`NetEq::get_audio`].
pub(crate) const MS_PER_TICK: u64 = 10;
/// `output_size_samples_`: `kOutputSizeMs` (10) at 48 kHz.
pub(crate) const OUTPUT_SIZE: usize = 480;
const SAMPLES_10_MS: usize = 480;
const SAMPLES_20_MS: usize = 960;
const SAMPLES_30_MS: usize = 1440;
/// `NetEq::Config::max_packets_in_buffer`'s default.
pub(crate) const MAX_PACKETS_IN_BUFFER: usize = 200;
/// `minimumJitterBufferLevel` of the client's transport options (module 31595): 0.
const MINIMUM_DELAY_MS: i32 = 0;
/// `MaxFrameSizePerChannel`: 120 ms.
const MAX_FRAME: usize = 5760;
/// Played samples kept behind the playout point, for borrowing back (`SyncBuffer`'s history).
const HISTORY: usize = SAMPLES_30_MS * 2;
/// Concealment with nothing in the buffer turns into silence (no decoding) after this long. Stands in for
/// `Expand::Muted`, which the codec PLC path never reaches.
pub(crate) const MUTED_AFTER_SAMPLES: usize = 48 * 250;
/// Below this peak (about -70 dBFS) a concealment block is silence: counted as noise, and muting may start.
const SILENT_PEAK: f32 = 3e-4;
/// Opus loss concealment runs in 10 ms steps (`WebRtcOpus_PlcDuration`, `kPlcFrameMs`).
const PLC_SAMPLES: usize = 480;

/// Counters of one stream since it began, for the call's debug log.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Stats {
    pub packets_received: u64,
    /// Sequence numbers never received (`cumulative lost` as RTCP counts it).
    pub packets_lost: u64,
    /// Primary packets that came after their time had passed and were thrown away.
    pub packets_late: u64,
    /// Packets played from the next packet's in-band FEC.
    pub fec_recovered: u64,
    /// Samples made by loss concealment while there was voice to conceal.
    pub concealed_samples: u64,
    /// Concealment that started (`EndExpandEvent` counts them).
    pub concealment_events: u64,
    /// Samples taken out by accelerate.
    pub accelerated_samples: u64,
    /// Samples added by preemptive expand.
    pub expanded_samples: u64,
    /// Whole-buffer flushes (`kFlushed`).
    pub flushes: u64,
    /// Samples output as zeros because nothing at all was ready.
    pub underrun_samples: u64,
    /// 10 ms blocks finished in normal decoding mode.
    pub op_normal: u64,
    /// 10 ms blocks filled by codec PLC / expansion.
    pub op_expand: u64,
    /// 10 ms blocks shortened by accelerate.
    pub op_accelerate: u64,
    /// 10 ms blocks lengthened by preemptive expand.
    pub op_preemptive_expand: u64,
    /// 10 ms blocks filled by internal comfort noise.
    pub op_cng: u64,
    /// Samples output while in NetEq's post-concealment muted state.
    pub muted_samples: u64,
}

/// The decoded-but-unplayed audio and a little of what was played (`SyncBuffer`), interleaved.
struct SyncBuffer {
    channels: usize,
    samples: Vec<f32>,
    /// The first frame not yet played.
    next: usize,
    /// The RTP timestamp just after the last decoded frame (`end_timestamp`).
    end_timestamp: u32,
}

impl SyncBuffer {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }

    fn future_length(&self) -> usize {
        self.frames() - self.next
    }

    fn push_back(&mut self, samples: &[f32]) {
        self.samples.extend_from_slice(samples);
    }

    fn increase_end_timestamp(&mut self, increment: u32) {
        self.end_timestamp = self.end_timestamp.wrapping_add(increment);
    }

    /// `ReadInterleavedFromEnd` and the length reduction of a borrow: the last `frames`, appended to `out` and removed
    /// from the end. The play position stays, so frames borrowed from what was already played count as played again once
    /// they are put back.
    fn take_from_end(&mut self, frames: usize, out: &mut Vec<f32>) {
        let frames = frames.min(self.frames());
        let at = (self.frames() - frames) * self.channels;
        out.extend_from_slice(&self.samples[at..]);
        self.samples.truncate(at);
    }

    /// Plays `out.len() / channels` frames; zeros where nothing is ready. The number of zero frames.
    fn read(&mut self, out: &mut [f32]) -> usize {
        let wanted = out.len() / self.channels;
        let available = self.future_length().min(wanted);
        let start = self.next * self.channels;
        out[..available * self.channels]
            .copy_from_slice(&self.samples[start..start + available * self.channels]);
        out[available * self.channels..].fill(0.0);
        self.next += available;
        if self.next > HISTORY {
            let drop = self.next - HISTORY;
            self.samples.drain(..drop * self.channels);
            self.next -= drop;
        }
        wanted - available
    }

    fn clear(&mut self) {
        self.samples.clear();
        self.next = 0;
    }
}

/// `WebRtcOpus_PacketHasFec`: whether the SILK layer of the first frame carries LBRR (in-band FEC) data.
pub(crate) fn packet_has_fec(payload: &[u8]) -> bool {
    if payload.is_empty() || payload[0] & 0x80 != 0 {
        return false;
    }
    let Ok(per_frame) = opus2::packet::get_samples_per_frame(payload, 48_000) else {
        return false;
    };
    let silk_frames = match (per_frame / 48).max(10) {
        10 | 20 => 1,
        40 => 2,
        60 => 3,
        _ => return false,
    };
    let channels = match opus2::packet::get_nb_channels(payload) {
        Ok(Channels::Stereo) => 2,
        Ok(_) => 1,
        Err(_) => return false,
    };
    let Ok(parsed) = opus2::packet::parse(payload) else {
        return false;
    };
    let Some(first) = parsed.frames.first().and_then(|frame| frame.first()) else {
        return false;
    };
    (0..channels).any(|n| {
        let shift = (n + 1) * (silk_frames + 1) - 1;
        first & (0x80u8 >> shift) != 0
    })
}

/// `WebRtcOpus_DurationEst`: samples per channel a packet decodes to; 0 when it cannot be read.
fn packet_duration(payload: &[u8]) -> usize {
    let (Ok(frames), Ok(per_frame)) = (
        opus2::packet::get_nb_frames(payload),
        opus2::packet::get_samples_per_frame(payload, 48_000),
    ) else {
        return 0;
    };
    let samples = frames * per_frame;
    if samples > MAX_FRAME { 0 } else { samples }
}

/// `WebRtcOpus_FecDurationEst`.
fn fec_duration(payload: &[u8]) -> usize {
    if !packet_has_fec(payload) {
        return 0;
    }
    match opus2::packet::get_samples_per_frame(payload, 48_000) {
        Ok(samples) if (SAMPLES_10_MS..=MAX_FRAME).contains(&samples) => samples,
        _ => 0,
    }
}

pub(crate) struct NetEq {
    channels: usize,
    decoder: Decoder,
    packet_buffer: PacketBuffer,
    controller: DecisionLogic,
    sync: SyncBuffer,
    /// `TickTimer::ticks()`.
    ticks: u64,
    last_mode: Mode,
    new_codec: bool,
    first_packet: bool,
    /// `timestamp_`.
    timestamp: u32,
    decoder_frame_length: usize,
    /// The tick concealment or comfort noise began at (`generated_noise_stopwatch_`).
    generated_noise_start: Option<u64>,
    /// A packet has been decoded (`current_rtp_payload_type_` is set), so the decoder can conceal.
    decoded_any: bool,
    /// Samples of concealment since the last decoded packet.
    concealment_run: usize,
    /// The last 10 ms block was muted output.
    muted: bool,
    /// The last concealment block was silence (concealing after a talk spurt ended in silence frames).
    concealment_silent: bool,
    /// The Opus decoder's DTX state (`in_dtx_mode`).
    in_dtx: bool,
    highest_sequence: Option<i64>,
    first_sequence: i64,
    stats: Stats,
    decoded: Vec<f32>,
    scratch: Vec<f32>,
    stretch_in: Vec<f32>,
    extracted_packets: Vec<Packet>,
    free_payloads: Vec<Vec<u8>>,
}

impl NetEq {
    pub(crate) fn new(stereo: bool) -> Result<Self, &'static str> {
        let channels = if stereo { 2 } else { 1 };
        let decoder = Decoder::new(
            48_000,
            if stereo {
                Channels::Stereo
            } else {
                Channels::Mono
            },
        )
        .map_err(|_| "Opus decoder initialization failed")?;
        let mut free_payloads = Vec::with_capacity(MAX_PACKETS_IN_BUFFER + 8);
        for _ in 0..(MAX_PACKETS_IN_BUFFER + 8) {
            free_payloads.push(Vec::with_capacity(1275));
        }
        Ok(Self {
            channels,
            decoder,
            packet_buffer: PacketBuffer::new(MAX_PACKETS_IN_BUFFER),
            controller: DecisionLogic::new(MAX_PACKETS_IN_BUFFER as i32, MINIMUM_DELAY_MS, 0),
            sync: SyncBuffer {
                channels,
                samples: Vec::with_capacity((HISTORY + MAX_FRAME * 3) * channels),
                next: 0,
                end_timestamp: 0,
            },
            ticks: 0,
            last_mode: Mode::Normal,
            new_codec: false,
            first_packet: true,
            timestamp: 0,
            decoder_frame_length: 3 * OUTPUT_SIZE,
            generated_noise_start: None,
            decoded_any: false,
            concealment_run: 0,
            muted: false,
            concealment_silent: false,
            in_dtx: false,
            highest_sequence: None,
            first_sequence: 0,
            stats: Stats::default(),
            decoded: vec![0.0; MAX_FRAME * 2 * channels],
            scratch: Vec::with_capacity((MAX_FRAME + SAMPLES_30_MS) * channels),
            stretch_in: Vec::with_capacity((MAX_FRAME + SAMPLES_30_MS) * channels),
            extracted_packets: Vec::with_capacity(16),
            free_payloads,
        })
    }

    pub(crate) fn stats(&self) -> Stats {
        self.stats
    }

    pub(crate) fn packets_discarded(&self) -> (u64, u64) {
        (
            self.packet_buffer.discarded_primary,
            self.packet_buffer.discarded_secondary,
        )
    }

    fn recycle_packet(free_payloads: &mut Vec<Vec<u8>>, mut packet: Packet) {
        if free_payloads.len() < free_payloads.capacity() {
            packet.payload.clear();
            free_payloads.push(packet.payload);
        }
    }

    fn take_payload(&mut self, data: &[u8]) -> Vec<u8> {
        let mut buf = self
            .free_payloads
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(1275));
        buf.clear();
        buf.extend_from_slice(data);
        buf
    }

    /// `TargetDelayMs`: the delay the buffer aims for now.
    pub(crate) fn target_delay_ms(&self) -> i32 {
        self.controller.target_level_ms()
    }

    /// What waits now, packets and decoded audio, in milliseconds (`FilteredCurrentDelayMs` unfiltered).
    pub(crate) fn buffer_level_ms(&self) -> usize {
        (self
            .packet_buffer
            .span_samples(self.decoder_frame_length, false, self.ticks)
            + self.sync.future_length())
            / 48
    }

    /// Whether the last block was silence that nothing will follow soon (muted output, nothing waiting).
    pub(crate) fn is_muted(&self) -> bool {
        self.muted
    }

    /// `FlushBuffers`: everything waiting goes; the next packet starts the stream again.
    pub(crate) fn flush(&mut self) {
        let free = &mut self.free_payloads;
        let _ = self
            .packet_buffer
            .flush_with(|packet| Self::recycle_packet(free, packet));
        self.sync.clear();
        self.first_packet = true;
        self.generated_noise_start = None;
        self.concealment_run = 0;
        self.muted = false;
        // Nothing to conceal from until the next packet.
        self.decoded_any = false;
    }

    fn count_sequence(&mut self, sequence: u16) {
        self.stats.packets_received += 1;
        let highest = match self.highest_sequence {
            None => {
                self.first_sequence = i64::from(sequence);
                i64::from(sequence)
            }
            Some(highest) => {
                highest.max(highest + i64::from(sequence.wrapping_sub(highest as u16) as i16))
            }
        };
        self.highest_sequence = Some(highest);
        let expected = (highest - self.first_sequence + 1).max(0) as u64;
        self.stats.packets_lost = expected.saturating_sub(self.stats.packets_received);
    }

    /// `NetEqImpl::InsertPacket` for one Opus RTP payload.
    pub(crate) fn insert_packet(&mut self, sequence: u16, timestamp: u32, payload: &[u8]) {
        if payload.is_empty() || payload.len() > 1275 {
            return;
        }
        self.count_sequence(sequence);
        let was_first = self.first_packet;
        if was_first {
            let free = &mut self.free_payloads;
            let _ = self
                .packet_buffer
                .flush_with(|packet| Self::recycle_packet(free, packet));
            self.sync
                .increase_end_timestamp(timestamp.wrapping_sub(self.timestamp));
            self.timestamp = timestamp;
            self.first_packet = false;
            self.new_codec = true;
        }
        // `AudioDecoderOpusImpl::ParsePayload`: the FEC copy first, then the primary.
        let mut parsed: [Option<Packet>; 2] = [None, None];
        let mut count = 0;
        let redundant = fec_duration(payload);
        let fec_ts = timestamp.wrapping_sub(redundant as u32);
        if redundant > 0
            && !self.new_codec
            && !decision::is_obsolete_timestamp(fec_ts, self.sync.end_timestamp, 0)
        {
            parsed[count] = Some(Packet {
                timestamp: fec_ts,
                priority: 1,
                payload: self.take_payload(payload),
                duration: redundant,
                inserted: self.ticks,
            });
            count += 1;
        }
        parsed[count] = Some(Packet {
            timestamp,
            priority: 0,
            payload: self.take_payload(payload),
            duration: packet_duration(payload),
            inserted: self.ticks,
        });
        count += 1;
        for packet in parsed.into_iter().take(count).flatten() {
            let primary = packet.priority == 0;
            let mut info = PacketArrivedInfo {
                packet_length_samples: packet.duration,
                main_timestamp: packet.timestamp,
                buffer_flush: false,
            };
            let free = &mut self.free_payloads;
            if self
                .packet_buffer
                .insert_with(packet, |discarded| Self::recycle_packet(free, discarded))
                == Insert::Flushed
            {
                self.new_codec = true;
                info.buffer_flush = true;
                self.stats.flushes += 1;
            }
            if primary {
                let _ = self
                    .controller
                    .packet_arrived(!self.new_codec, &info, self.ticks);
            }
        }
    }

    /// `NetEqImpl::GetAudio`: the next 10 ms, `OUTPUT_SIZE` frames interleaved into `out`.
    pub(crate) fn get_audio(&mut self, out: &mut [f32]) {
        debug_assert_eq!(out.len(), OUTPUT_SIZE * self.channels);
        self.ticks += 1;
        // The muted state (`enable_muted_state`): nothing to play and concealment long done, or nothing ever came.
        if self.packet_buffer.is_empty()
            && (!self.decoded_any
                || (is_expand(self.last_mode)
                    && (self.concealment_run >= MUTED_AFTER_SAMPLES || self.concealment_silent)))
        {
            out.fill(0.0);
            self.muted = true;
            self.stats.muted_samples += OUTPUT_SIZE as u64;
            return;
        }
        self.muted = false;
        let mut packets = std::mem::take(&mut self.extracted_packets);
        packets.clear();
        let operation = self.get_decision(&mut packets);
        let decoded_frames = self.decode(&packets, operation);
        let had_packets = !packets.is_empty();
        for packet in packets.drain(..) {
            Self::recycle_packet(&mut self.free_payloads, packet);
        }
        self.extracted_packets = packets;
        let operation = if decoded_frames.is_none() && had_packets {
            // A decoding error: conceal instead.
            Operation::Expand
        } else {
            operation
        };
        let decoded_frames = decoded_frames.unwrap_or(0);
        match operation {
            Operation::Normal | Operation::Merge | Operation::Undefined => {
                self.stats.op_normal += 1;
                self.do_normal(decoded_frames);
            }
            Operation::Expand => {
                self.stats.op_expand += 1;
                self.do_codec_plc();
            }
            Operation::Accelerate | Operation::FastAccelerate => {
                self.stats.op_accelerate += 1;
                self.do_accelerate(decoded_frames);
            }
            Operation::PreemptiveExpand => {
                self.stats.op_preemptive_expand += 1;
                self.do_preemptive_expand(decoded_frames);
            }
            Operation::CodecInternalCng => {
                self.stats.op_cng += 1;
                self.do_codec_internal_cng();
            }
        }
        let missing = self.sync.read(out);
        self.stats.underrun_samples += missing as u64;
        if !matches!(
            self.last_mode,
            Mode::CodecInternalCng | Mode::Expand | Mode::CodecPlc
        ) {
            self.generated_noise_start = None;
        }
    }

    /// `NetEqImpl::GetDecision`, with the packets to decode put in `packets` (`ExtractPackets`).
    fn get_decision(&mut self, packets: &mut Vec<Packet>) -> Operation {
        let mut end_timestamp = self.sync.end_timestamp;
        if !self.new_codec {
            let free = &mut self.free_payloads;
            self.stats.packets_late +=
                self.packet_buffer
                    .discard_old_packets_with(end_timestamp, 5 * 48_000, |packet| {
                        Self::recycle_packet(free, packet)
                    });
        }
        let next = self.packet_buffer.peek_next().map(|packet| NextPacket {
            timestamp: packet.timestamp,
            is_dtx: packet.is_dtx(),
        });
        // `expand_->overlap_length()` is 0 here: no `Expand` runs to overlap.
        let samples_left = self.sync.future_length();
        if matches!(
            self.last_mode,
            Mode::AccelerateSuccess
                | Mode::AccelerateLowEnergy
                | Mode::PreemptiveExpandSuccess
                | Mode::PreemptiveExpandLowEnergy
        ) {
            self.controller
                .add_sample_memory(-((samples_left + OUTPUT_SIZE) as i32));
        }
        let generated_noise_samples = self.generated_noise_start.map_or(0, |start| {
            (self.ticks - start) as usize * OUTPUT_SIZE + self.controller.noise_fast_forward()
        });
        let status = Status {
            target_timestamp: end_timestamp,
            expand_mutefactor: 16384,
            last_mode: self.last_mode,
            next_packet: next,
            generated_noise_samples,
            span_samples: self.packet_buffer.span_samples(
                self.decoder_frame_length,
                false,
                self.ticks,
            ),
            span_samples_wait_time: self.packet_buffer.span_samples(
                self.decoder_frame_length,
                true,
                self.ticks,
            ),
            dtx_or_cng: self.packet_buffer.contains_dtx(),
            sync_buffer_samples: samples_left,
        };
        let mut operation = self.controller.get_decision(&status, self.ticks);
        let stretching = |operation: Operation| {
            matches!(
                operation,
                Operation::Merge
                    | Operation::Accelerate
                    | Operation::FastAccelerate
                    | Operation::PreemptiveExpand
            )
        };
        if next.is_some_and(|packet| packet.is_dtx) && stretching(operation) {
            operation = Operation::Normal;
        }
        if samples_left >= OUTPUT_SIZE && !stretching(operation) {
            return Operation::Normal;
        }
        if self.last_mode == Mode::CodecPlc && operation != Operation::Expand {
            // `EndExpandEvent`.
            self.concealment_run = 0;
        }
        if self.new_codec || operation == Operation::Undefined {
            let Some(packet) = next else {
                return Operation::Expand;
            };
            self.timestamp = packet.timestamp;
            if operation != Operation::CodecInternalCng {
                operation = Operation::Normal;
            }
            self.sync
                .increase_end_timestamp(self.timestamp.wrapping_sub(end_timestamp));
            end_timestamp = self.timestamp;
            self.new_codec = false;
            self.controller.soft_reset(self.ticks);
        }
        let mut required_samples = OUTPUT_SIZE;
        match operation {
            Operation::Expand => {
                self.timestamp = end_timestamp;
                return operation;
            }
            Operation::CodecInternalCng => return operation,
            Operation::Accelerate | Operation::FastAccelerate => {
                if samples_left >= SAMPLES_30_MS {
                    self.controller.set_sample_memory(samples_left as i32);
                    self.controller.set_prev_time_scale(true);
                    return operation;
                } else if samples_left >= SAMPLES_10_MS
                    && self.decoder_frame_length >= SAMPLES_30_MS
                {
                    return Operation::Normal;
                } else if samples_left < SAMPLES_20_MS && self.decoder_frame_length < SAMPLES_30_MS
                {
                    required_samples = 2 * OUTPUT_SIZE;
                    operation = Operation::Normal;
                }
            }
            Operation::PreemptiveExpand => {
                if samples_left >= SAMPLES_30_MS
                    || (samples_left >= SAMPLES_10_MS && self.decoder_frame_length >= SAMPLES_30_MS)
                {
                    self.controller.set_sample_memory(samples_left as i32);
                    self.controller.set_prev_time_scale(true);
                    return operation;
                }
                if samples_left < SAMPLES_20_MS && self.decoder_frame_length < SAMPLES_30_MS {
                    required_samples = 2 * OUTPUT_SIZE;
                }
            }
            _ => {}
        }
        let mut extracted = 0;
        if let Some(packet) = next {
            self.sync
                .increase_end_timestamp(packet.timestamp.wrapping_sub(end_timestamp));
            extracted = self.extract_packets(required_samples, packets);
        }
        if matches!(
            operation,
            Operation::Accelerate | Operation::FastAccelerate | Operation::PreemptiveExpand
        ) {
            self.controller
                .set_sample_memory((samples_left + extracted) as i32);
            self.controller.set_prev_time_scale(true);
        }
        if matches!(operation, Operation::Accelerate | Operation::FastAccelerate)
            && extracted + samples_left < SAMPLES_30_MS
        {
            operation = Operation::Normal;
        }
        self.timestamp = self.sync.end_timestamp;
        operation
    }

    /// `ExtractPackets`: consecutive packets until `required_samples` are covered.
    fn extract_packets(&mut self, required_samples: usize, packets: &mut Vec<Packet>) -> usize {
        let Some(first_timestamp) = self
            .packet_buffer
            .peek_next()
            .map(|packet| packet.timestamp)
        else {
            return 0;
        };
        let mut extracted = 0;
        while let Some(packet) = self.packet_buffer.pop_next() {
            self.timestamp = packet.timestamp;
            let duration = if packet.duration > 0 {
                packet.duration
            } else {
                self.decoder_frame_length
            };
            extracted = packet.timestamp.wrapping_sub(first_timestamp) as usize + duration;
            let end = packet.timestamp.wrapping_add(duration as u32);
            packets.push(packet);
            let more = extracted < required_samples
                && self
                    .packet_buffer
                    .peek_next()
                    .is_some_and(|next| next.timestamp == end);
            if !more {
                break;
            }
        }
        let free = &mut self.free_payloads;
        let _ = self
            .packet_buffer
            .discard_old_packets_with(self.timestamp, 0, |packet| {
                Self::recycle_packet(free, packet)
            });
        extracted
    }

    /// `Decode`/`DecodeLoop`: the packets into `self.decoded`, primary or from FEC. Frames per channel decoded; `None`
    /// on a decoder error.
    fn decode(&mut self, packets: &[Packet], operation: Operation) -> Option<usize> {
        let channels = self.channels;
        let mut length = 0;
        let mut comfort_noise = false;
        for packet in packets {
            let room = &mut self.decoded[length * channels..];
            let result = if packet.priority == 0 {
                self.decoder.decode_float(&packet.payload, room, false)
            } else {
                let samples = packet.duration.min(room.len() / channels);
                self.decoder
                    .decode_float(&packet.payload, &mut room[..samples * channels], true)
            };
            let frames = match result {
                Ok(frames) if frames > 0 => frames,
                _ => return None,
            };
            if packet.priority > 0 {
                self.stats.fec_recovered += 1;
            }
            // `DetermineAudioType`: one or two bytes are DTX, comfort noise.
            comfort_noise = packet.payload.len() <= 2;
            self.in_dtx = comfort_noise;
            self.decoder_frame_length = frames;
            self.decoded_any = true;
            length += frames;
        }
        if length > 0 && !comfort_noise {
            self.sync.increase_end_timestamp(length as u32);
        }
        if comfort_noise && length > 0 && operation != Operation::Expand {
            self.last_mode = Mode::CodecInternalCng;
        }
        Some(length)
    }

    fn do_normal(&mut self, frames: usize) {
        let samples = frames * self.channels;
        self.sync.push_back(&self.decoded[..samples]);
        if frames != 0 && !self.in_dtx {
            self.last_mode = Mode::Normal;
            self.concealment_run = 0;
        } else if frames != 0 {
            self.last_mode = Mode::CodecInternalCng;
            self.generated_noise_start.get_or_insert(self.ticks);
        }
    }

    /// `DoCodecPlc` with `AudioDecoderOpusImpl::GeneratePlc`: 10 ms of the decoder's concealment.
    fn do_codec_plc(&mut self) {
        let channels = self.channels;
        let requested = OUTPUT_SIZE.saturating_sub(self.sync.future_length());
        let mut produced = 0;
        let mut peak = 0.0f32;
        while produced < requested.max(1) {
            let block = &mut self.decoded[..PLC_SAMPLES * channels];
            let frames = match self.decoder.decode_float(&[], block, false) {
                Ok(frames) if frames > 0 => frames,
                _ => {
                    block.fill(0.0);
                    PLC_SAMPLES
                }
            };
            peak = self.decoded[..frames * channels]
                .iter()
                .fold(peak, |peak, s| peak.max(s.abs()));
            self.sync.push_back(&self.decoded[..frames * channels]);
            produced += frames;
        }
        // `ExpandedVoiceSamples` against `ExpandedNoiseSamples`: concealment of silence is not concealed voice.
        self.concealment_silent = peak < SILENT_PEAK;
        if !self.concealment_silent {
            if self.last_mode != Mode::CodecPlc {
                self.stats.concealment_events += 1;
            }
            self.stats.concealed_samples += produced as u64;
        }
        self.concealment_run += produced;
        self.last_mode = Mode::CodecPlc;
        self.generated_noise_start.get_or_insert(self.ticks);
    }

    /// `DoCodecInternalCng`: the Opus decoder's comfort noise after a DTX packet.
    fn do_codec_internal_cng(&mut self) {
        let channels = self.channels;
        let block = &mut self.decoded[..OUTPUT_SIZE * channels];
        let frames = match self.decoder.decode_float(&[], block, false) {
            Ok(frames) if frames > 0 => frames,
            _ => {
                block.fill(0.0);
                OUTPUT_SIZE
            }
        };
        self.sync.push_back(&self.decoded[..frames * channels]);
        self.last_mode = Mode::CodecInternalCng;
        self.generated_noise_start.get_or_insert(self.ticks);
    }

    /// 30 ms to work on: what was decoded, with what it lacks borrowed from the end of the sync buffer. Populates
    /// `self.stretch_in` (in 16-bit units) and returns how many borrowed frames had already been played.
    fn stretch_input(&mut self, decoded_frames: usize) -> usize {
        let channels = self.channels;
        let future = self.sync.future_length();
        self.stretch_in.clear();
        let mut old_borrowed = 0;
        if decoded_frames < SAMPLES_30_MS {
            let borrowed = SAMPLES_30_MS - decoded_frames;
            old_borrowed = borrowed.saturating_sub(future);
            self.sync.take_from_end(borrowed, &mut self.stretch_in);
        }
        self.stretch_in
            .extend_from_slice(&self.decoded[..decoded_frames * channels]);
        for sample in &mut self.stretch_in {
            *sample *= 32768.0;
        }
        old_borrowed
    }

    fn put_back_scratch(&mut self) {
        for sample in &mut self.scratch {
            *sample /= 32768.0;
        }
        self.sync.push_back(&self.scratch);
        self.sync.next = self.sync.next.min(self.sync.frames());
    }

    /// `DoAccelerate` (`enable_fast_accelerate` is false by default, so fast accelerate stretches as accelerate does).
    fn do_accelerate(&mut self, decoded_frames: usize) {
        let _ = self.stretch_input(decoded_frames);
        self.scratch.clear();
        let (result, removed) =
            time_stretch::accelerate(&self.stretch_in, self.channels, false, &mut self.scratch);
        self.stats.accelerated_samples += removed as u64;
        self.last_mode = match result {
            Stretch::Success => Mode::AccelerateSuccess,
            Stretch::SuccessLowEnergy => Mode::AccelerateLowEnergy,
            Stretch::Unchanged | Stretch::Error => Mode::AccelerateFail,
        };
        self.put_back_scratch();
        self.concealment_run = 0;
    }

    /// `DoPreemptiveExpand`.
    fn do_preemptive_expand(&mut self, decoded_frames: usize) {
        let old_borrowed = self.stretch_input(decoded_frames);
        self.scratch.clear();
        let (result, added) = time_stretch::preemptive_expand(
            &self.stretch_in,
            self.channels,
            old_borrowed,
            &mut self.scratch,
        );
        self.stats.expanded_samples += added as u64;
        self.last_mode = match result {
            Stretch::Success => Mode::PreemptiveExpandSuccess,
            Stretch::SuccessLowEnergy => Mode::PreemptiveExpandLowEnergy,
            Stretch::Unchanged | Stretch::Error => Mode::PreemptiveExpandFail,
        };
        self.put_back_scratch();
        self.concealment_run = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opus2::{Application, Encoder};

    fn encoder() -> Encoder {
        let mut encoder = Encoder::new(48_000, Channels::Mono, Application::Voip).unwrap();
        encoder.set_bitrate(opus2::Bitrate::Bits(64_000)).unwrap();
        encoder.set_inband_fec(true).unwrap();
        encoder.set_packet_loss_perc(30).unwrap();
        encoder
    }

    fn packets(count: usize) -> Vec<Vec<u8>> {
        let mut encoder = encoder();
        (0..count)
            .map(|k| {
                let pcm: Vec<f32> = (0..960)
                    .map(|i| {
                        ((k * 960 + i) as f32 * 0.03).sin()
                            * 0.3
                            * (1.0 + ((k * 960 + i) as f32 * 0.0004).sin())
                    })
                    .collect();
                let mut out = [0u8; 1275];
                let n = encoder.encode_float(&pcm, &mut out).unwrap();
                out[..n].to_vec()
            })
            .collect()
    }

    #[test]
    fn opus_fec_is_found_in_voip_packets() {
        let packets = packets(20);
        assert!(packets[5..].iter().any(|p| packet_has_fec(p)));
        assert!(!packet_has_fec(&[]));
        assert!(!packet_has_fec(&[0xF8, 0xFF, 0xFE]), "CELT-only silence");
        assert_eq!(packet_duration(&packets[0]), 960);
    }

    #[test]
    fn a_steady_stream_plays_every_sample_once() {
        let packets = packets(200);
        let mut neteq = NetEq::new(false).unwrap();
        let mut out = [0.0f32; OUTPUT_SIZE];
        let mut played = 0;
        for (k, packet) in packets.iter().enumerate() {
            neteq.insert_packet(k as u16, k as u32 * 960, packet);
            for _ in 0..2 {
                neteq.get_audio(&mut out);
                played += 1;
            }
        }
        let stats = neteq.stats();
        assert_eq!(stats.packets_lost, 0);
        assert_eq!(stats.packets_late, 0);
        assert_eq!(stats.flushes, 0);
        assert_eq!(played, 400);
        assert!(neteq.target_delay_ms() <= 80);
    }

    #[test]
    fn a_lost_packet_comes_back_from_the_next_ones_fec() {
        let packets = packets(100);
        let mut neteq = NetEq::new(false).unwrap();
        let mut out = [0.0f32; OUTPUT_SIZE];
        for (k, packet) in packets.iter().enumerate() {
            // Two packets of lead, so the next one is there when the lost one is due.
            if k != 50 {
                neteq.insert_packet(k as u16, k as u32 * 960, packet);
            }
            if k >= 2 {
                for _ in 0..2 {
                    neteq.get_audio(&mut out);
                }
            }
        }
        let stats = neteq.stats();
        assert_eq!(stats.packets_lost, 1);
        assert!(stats.fec_recovered >= 1, "{stats:?}");
    }

    #[test]
    fn a_flush_starts_the_stream_again() {
        let packets = packets(10);
        let mut neteq = NetEq::new(true).unwrap();
        let mut out = [0.0f32; OUTPUT_SIZE * 2];
        neteq.get_audio(&mut out);
        assert!(neteq.is_muted(), "nothing came yet");
        neteq.insert_packet(0, 0, &packets[0]);
        neteq.get_audio(&mut out);
        assert!(!neteq.is_muted());
        neteq.flush();
        neteq.insert_packet(1, 960, &packets[1]);
        neteq.get_audio(&mut out);
        assert_eq!(neteq.stats().packets_received, 2);
    }

    #[test]
    fn get_audio_and_steady_insert_never_reallocate_scratch_buffers() {
        let packets = packets(60);
        let mut neteq = NetEq::new(false).unwrap();
        let mut out = [0.0f32; OUTPUT_SIZE];
        // Pre-warm with one packet so the initial payload pool capacity is established.
        neteq.insert_packet(0, 0, &packets[0]);
        neteq.get_audio(&mut out);

        let sync_ptr = neteq.sync.samples.as_ptr();
        let sync_cap = neteq.sync.samples.capacity();
        let decoded_ptr = neteq.decoded.as_ptr();
        let decoded_cap = neteq.decoded.capacity();
        let scratch_ptr = neteq.scratch.as_ptr();
        let scratch_cap = neteq.scratch.capacity();
        let stretch_ptr = neteq.stretch_in.as_ptr();
        let stretch_cap = neteq.stretch_in.capacity();
        let extracted_ptr = neteq.extracted_packets.as_ptr();
        let extracted_cap = neteq.extracted_packets.capacity();

        for (k, packet) in packets.iter().enumerate().skip(1) {
            if k % 17 != 0 {
                neteq.insert_packet(k as u16, k as u32 * 960, packet);
            }
            for _ in 0..2 {
                neteq.get_audio(&mut out);
            }
        }
        // Trigger extra PLC and accelerate/preemptive expand ticks via jitter burst.
        for _ in 0..30 {
            neteq.get_audio(&mut out);
        }
        for (k, packet) in packets[..10].iter().enumerate() {
            neteq.insert_packet((60 + k) as u16, (60 + k) as u32 * 960, packet);
        }
        for _ in 0..20 {
            neteq.get_audio(&mut out);
        }

        assert_eq!(neteq.sync.samples.as_ptr(), sync_ptr);
        assert_eq!(neteq.sync.samples.capacity(), sync_cap);
        assert_eq!(neteq.decoded.as_ptr(), decoded_ptr);
        assert_eq!(neteq.decoded.capacity(), decoded_cap);
        assert_eq!(neteq.scratch.as_ptr(), scratch_ptr);
        assert_eq!(neteq.scratch.capacity(), scratch_cap);
        assert_eq!(neteq.stretch_in.as_ptr(), stretch_ptr);
        assert_eq!(neteq.stretch_in.capacity(), stretch_cap);
        assert_eq!(neteq.extracted_packets.as_ptr(), extracted_ptr);
        assert_eq!(neteq.extracted_packets.capacity(), extracted_cap);
    }
}
