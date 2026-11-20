//! Receiving video: RTP of announced sources is reordered, repaired (NACK, RTX), reassembled into access units and,
//! once DAVE has opened them, decoded on one thread per source. Everything is bounded; nothing here blocks the caller.
//!
//! Adapted from Serein `crates/discord-voice/src/video_receive.rs` (`Receivers`, `decode_loop` in outline) and
//! `transport.rs` (`decrypt_video`, `receive_video`), MIT OR Apache-2.0. Differences: H.264 only, one decoder thread per
//! source whose results go straight to the app's sink, and no playout buffer (a camera is shown as it arrives).
use super::{
    RemoteFrame, VideoHub, VideoSource, depacketize::Assembler, playout::Playout, reorder::Window,
};
use crate::{
    rtp::{self, H264_PAYLOAD_TYPE, H264_RTX_PAYLOAD_TYPE, Rtp},
    transport_cc::TransportFeedback,
};
use dc_media::decode::{DecodeError, Decoder};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};
use tokio::time::Instant;

/// Sources one connection accepts (Serein's `MAX_SOURCES`).
pub(crate) const MAX_SOURCES: usize = 16;
/// Decoder threads one connection runs (Serein's `MAX_DECODERS`).
const MAX_DECODERS: usize = 8;
/// Access units waiting for one decoder.
const DECODER_QUEUE: usize = 8;
/// Access units a decoder thread holds for their playout time; past this the oldest is decoded at once.
const MAX_HELD: usize = 32;
/// The longest a decoder thread sleeps before it looks for new access units again.
const HOLD_STEP: Duration = Duration::from_millis(10);
/// A source that wants a keyframe is asked again this often while none comes (libwebrtc's video receive stream:
/// `max_wait_for_keyframe`, 200 ms).
const PLI_INTERVAL: Duration = Duration::from_millis(200);
/// Audio sources a connection keeps reception statistics for; a new one replaces the longest silent.
const MAX_AUDIO_SOURCES: usize = 64;
/// A decoder that failed to start is not tried again for this long.
const DECODER_RETRY: Duration = Duration::from_secs(5);

/// One complete access unit, still DAVE ciphertext.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Received {
    pub user: u64,
    pub ssrc: u32,
    pub timestamp: u32,
    pub data: Vec<u8>,
}

struct Source {
    ssrc: u32,
    rtx: u32,
    user: u64,
    assembler: Assembler,
    window: Window,
    /// The decoder or the depacketizer lost the reference chain; only a keyframe restarts the picture.
    awaiting: bool,
    last_pli: Option<Instant>,
    stats: Stats,
    bytes_received: u64,
    nacks: u64,
    plis: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct InboundVideoSourceSnapshot {
    pub user: u64,
    pub ssrc: u32,
    pub rtx_ssrc: u32,
    pub packets_received: u64,
    pub packets_lost: u64,
    pub bytes_received: u64,
    pub jitter_ms: f32,
    pub nacks: u64,
    pub plis: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DecodedVideoSnapshot {
    pub width: u32,
    pub height: u32,
    pub frames_received: u64,
    pub frames_decoded: u64,
    pub keyframes_decoded: u64,
    pub decoder_name: Option<String>,
}

/// RTP clock rates of the two media kinds.
const VIDEO_CLOCK: u32 = 90_000;
const AUDIO_CLOCK: u32 = 48_000;

/// RFC 3550 reception statistics of one source, for the receiver report: loss over the original packets only (a
/// repair through RTX is not a received original, as libwebrtc counts it), interarrival jitter (appendix A.8) and the
/// echo of the last sender report.
struct Stats {
    /// RTP clock rate: 90 kHz video, 48 kHz Opus.
    clock: u32,
    base: u32,
    highest: Option<u32>,
    received: u32,
    expected_prior: u32,
    received_prior: u32,
    /// Where arrival times are counted from, and the last relative transit time in clock units.
    epoch: Option<Instant>,
    transit: Option<u32>,
    last_timestamp: Option<u32>,
    /// Interarrival jitter times 16, as appendix A.8 keeps it.
    jitter: u32,
    /// The middle 32 bits of the last sender report's NTP time, and when it arrived.
    last_sender_report: Option<(u32, Instant)>,
    last_arrival: Option<Instant>,
}

/// A source that sent nothing for this long is left out of receiver reports (libwebrtc's `kStatisticsTimeoutMs`).
const STATISTICS_TIMEOUT: Duration = Duration::from_secs(8);

impl Stats {
    fn new(clock: u32) -> Self {
        Self {
            clock,
            base: 0,
            highest: None,
            received: 0,
            expected_prior: 0,
            received_prior: 0,
            epoch: None,
            transit: None,
            last_timestamp: None,
            jitter: 0,
            last_sender_report: None,
            last_arrival: None,
        }
    }

    fn observe(&mut self, sequence: u16, timestamp: u32, now: Instant) {
        self.received = self.received.wrapping_add(1);
        self.last_arrival = Some(now);
        let in_order = match self.highest {
            None => {
                self.base = u32::from(sequence);
                self.highest = Some(u32::from(sequence));
                true
            }
            Some(highest) => {
                let delta = sequence.wrapping_sub(highest as u16);
                if delta != 0 && delta < 0x8000 {
                    self.highest = Some(highest.wrapping_add(u32::from(delta)));
                    true
                } else {
                    false
                }
            }
        };
        // Jitter from packets in order with a new timestamp: the packets of one picture leave together.
        if in_order && self.last_timestamp != Some(timestamp) {
            self.last_timestamp = Some(timestamp);
            let epoch = *self.epoch.get_or_insert(now);
            let arrival = (now.saturating_duration_since(epoch).as_micros()
                * u128::from(self.clock)
                / 1_000_000) as u32;
            let transit = arrival.wrapping_sub(timestamp);
            if let Some(previous) = self.transit {
                let difference = (transit.wrapping_sub(previous) as i32).unsigned_abs();
                // J += (|D| - J) / 16, with J kept times 16.
                self.jitter = self
                    .jitter
                    .wrapping_add(difference)
                    .wrapping_sub((self.jitter + 8) >> 4);
            }
            self.transit = Some(transit);
        }
    }

    fn packets_lost(&self) -> u64 {
        let Some(highest) = self.highest else {
            return 0;
        };
        let expected = i64::from(highest.wrapping_sub(self.base).wrapping_add(1));
        (expected - i64::from(self.received)).max(0) as u64
    }

    fn jitter_ms(&self) -> f32 {
        (self.jitter >> 4) as f32 * 1000.0 / self.clock.max(1) as f32
    }

    /// The block for the interval since the last one, or `None` before the first packet.
    fn block(&mut self, ssrc: u32, now: Instant) -> Option<rtp::ReportBlock> {
        let highest = self.highest?;
        if self
            .last_arrival
            .is_none_or(|at| now.saturating_duration_since(at) >= STATISTICS_TIMEOUT)
        {
            return None;
        }
        let expected = highest.wrapping_sub(self.base).wrapping_add(1);
        let lost = i64::from(expected) - i64::from(self.received);
        let expected_interval = expected.wrapping_sub(self.expected_prior);
        let received_interval = self.received.wrapping_sub(self.received_prior);
        self.expected_prior = expected;
        self.received_prior = self.received;
        let lost_interval = i64::from(expected_interval) - i64::from(received_interval);
        let fraction = if expected_interval == 0 || lost_interval <= 0 {
            0
        } else {
            ((lost_interval << 8) / i64::from(expected_interval)).min(255) as u8
        };
        let (last_sender_report, delay) = self.last_sender_report.map_or((0, 0), |(middle, at)| {
            (
                middle,
                (now.saturating_duration_since(at).as_secs_f64() * 65_536.0) as u32,
            )
        });
        Some(rtp::ReportBlock {
            ssrc,
            fraction_lost: fraction,
            cumulative_lost: lost.clamp(-0x80_0000, 0x7f_ffff) as i32,
            highest,
            jitter: self.jitter >> 4,
            last_sender_report,
            delay,
        })
    }
}

#[derive(Default)]
pub(crate) struct Receivers {
    sources: Vec<Source>,
    audio: Vec<(u32, Stats)>,
    next_report: Option<Instant>,
    transport: Option<TransportFeedback>,
}

impl Receivers {
    /// Binds a user's video and RTX SSRC (op 12). A user has one source; a new announcement replaces the old one.
    pub fn announce(&mut self, user: u64, ssrc: u32, rtx: u32) -> Result<(), &'static str> {
        if ssrc == 0 || rtx == 0 || ssrc == rtx {
            return Err("Invalid video SSRC assignment");
        }
        if self.sources.iter().any(|s| {
            (s.ssrc == ssrc || s.rtx == rtx || s.ssrc == rtx || s.rtx == ssrc) && s.user != user
        }) {
            return Err("Video SSRC already belongs to another user");
        }
        if let Some(existing) = self.sources.iter_mut().find(|s| s.user == user) {
            if existing.ssrc == ssrc {
                existing.rtx = rtx;
                return Ok(());
            }
            self.sources.retain(|s| s.user != user);
        }
        if self.sources.len() >= MAX_SOURCES {
            return Err("Too many video sources");
        }
        self.sources.push(Source {
            ssrc,
            rtx,
            user,
            assembler: Assembler::default(),
            window: Window::default(),
            // A decoder cannot start from a delta picture.
            awaiting: true,
            last_pli: None,
            stats: Stats::new(VIDEO_CLOCK),
            bytes_received: 0,
            nacks: 0,
            plis: 0,
        });
        Ok(())
    }

    pub fn remove(&mut self, user: u64) {
        self.sources.retain(|s| s.user != user);
    }

    /// Queues one authenticated RTP packet of payload type 101 or 102. Unknown SSRCs are dropped.
    pub fn push(&mut self, mut rtp: Rtp, now: Instant) {
        let original = rtp.payload_type == H264_PAYLOAD_TYPE;
        let (ssrc, sequence) = if rtp.payload_type == H264_RTX_PAYLOAD_TYPE {
            // RFC 4588: the original sequence number leads the payload.
            if rtp.payload.len() < 3 {
                return;
            }
            let Some(source) = self.sources.iter().find(|s| s.rtx == rtp.ssrc) else {
                return;
            };
            let sequence = u16::from_be_bytes([rtp.payload[0], rtp.payload[1]]);
            rtp.payload.drain(..2);
            (source.ssrc, sequence)
        } else if rtp.payload_type == H264_PAYLOAD_TYPE {
            (rtp.ssrc, rtp.sequence)
        } else {
            return;
        };
        let Some(source) = self.sources.iter_mut().find(|s| s.ssrc == ssrc) else {
            return;
        };
        if original {
            source.stats.observe(sequence, rtp.timestamp, now);
        }
        source.bytes_received += rtp.payload.len() as u64;
        if source
            .window
            .push(sequence, rtp.timestamp, rtp.marker, &rtp.payload, now)
        {
            // A bound forced the window to give up on the chain.
            source.awaiting = true;
        }
    }

    /// Takes every access unit that is ready, in order per source.
    pub fn drain(&mut self, now: Instant, out: &mut Vec<Received>) {
        for source in &mut self.sources {
            loop {
                let (packet, lost) = source.window.pop(now);
                if lost {
                    source.awaiting = true;
                }
                let Some(packet) = packet else { break };
                if let Some(data) = source.assembler.push(
                    packet.sequence,
                    packet.timestamp,
                    packet.marker,
                    &packet.payload,
                ) {
                    out.push(Received {
                        user: source.user,
                        ssrc: source.ssrc,
                        timestamp: packet.timestamp,
                        data,
                    });
                }
                if std::mem::take(&mut source.assembler.lost) {
                    source.awaiting = true;
                }
            }
        }
    }

    /// One authenticated Opus packet, for the receiver report.
    pub fn observe_audio(&mut self, ssrc: u32, sequence: u16, timestamp: u32, now: Instant) {
        if let Some((_, stats)) = self.audio.iter_mut().find(|(known, _)| *known == ssrc) {
            stats.observe(sequence, timestamp, now);
        } else {
            if self.audio.len() >= MAX_AUDIO_SOURCES
                && let Some(oldest) =
                    (0..self.audio.len()).min_by_key(|i| self.audio[*i].1.last_arrival)
            {
                self.audio.swap_remove(oldest);
            }
            let mut stats = Stats::new(AUDIO_CLOCK);
            stats.observe(sequence, timestamp, now);
            self.audio.push((ssrc, stats));
        }
    }

    /// The transport-wide sequence number of any authenticated media packet (audio or video, extension 5).
    pub fn observe_transport(&mut self, ssrc: u32, sequence: Option<u16>, now: Instant) {
        let feedback = self
            .transport
            .get_or_insert_with(|| TransportFeedback::new(now.into_std()));
        match sequence {
            Some(sequence) => feedback.receive(ssrc, sequence, now.into_std()),
            None => feedback.unnumbered += 1,
        }
    }

    /// Numbered and unnumbered media packets seen so far, for the log.
    pub fn transport_counts(&self) -> (u64, u64) {
        self.transport
            .as_ref()
            .map_or((0, 0), |feedback| (feedback.numbered, feedback.unnumbered))
    }

    /// A decrypted compound RTCP packet from the server: its sender reports are echoed in our receiver reports.
    pub fn sender_reports(&mut self, compound: &[u8], now: Instant) {
        for (ssrc, middle) in rtp::sender_reports(compound) {
            let stats = self
                .sources
                .iter_mut()
                .filter(|source| source.ssrc == ssrc)
                .map(|source| &mut source.stats)
                .chain(
                    self.audio
                        .iter_mut()
                        .filter(|(known, _)| *known == ssrc)
                        .map(|(_, stats)| stats),
                );
            for stats in stats {
                stats.last_sender_report = Some((middle, now));
            }
        }
    }

    /// A dropped or undecodable picture invalidates every later prediction until an IDR.
    pub fn require_keyframe(&mut self, user: u64) {
        for source in self.sources.iter_mut().filter(|s| s.user == user) {
            source.awaiting = true;
        }
    }

    /// Whether a decrypted access unit may go to the decoder: keyframes always, predictions only while the reference
    /// chain is intact.
    pub fn accept(&mut self, user: u64, keyframe: bool) -> bool {
        let Some(source) = self.sources.iter_mut().find(|s| s.user == user) else {
            return false;
        };
        if keyframe {
            source.awaiting = false;
            return true;
        }
        !source.awaiting
    }

    /// The feedback to send now: NACKs for the holes that are due, a PLI per source that wants a keyframe (again every
    /// 200 ms while it waits), transport-wide feedback every 100 ms, and a receiver report a second. `own_ssrc` is our
    /// audio SSRC, the RTCP sender. Cheap enough to call on every arrival; most calls return nothing.
    pub fn feedback(&mut self, own_ssrc: u32, now: Instant) -> Vec<([u8; 8], Vec<u8>)> {
        let mut out = Vec::new();
        for source in &mut self.sources {
            if let Some(nack) = source.window.nack(own_ssrc, source.ssrc, now) {
                source.nacks += 1;
                out.push(nack);
            }
            if source.awaiting
                && source
                    .last_pli
                    .is_none_or(|at| now.saturating_duration_since(at) >= PLI_INTERVAL)
            {
                source.last_pli = Some(now);
                source.plis += 1;
                let (header, body) = rtp::pli(own_ssrc, source.ssrc);
                out.push((header, body.to_vec()));
            }
        }
        if let Some(transport) = self.transport.as_mut() {
            out.extend(transport.due(own_ssrc, now.into_std()));
        }
        // A receiver report a second keeps the sender's rate control informed (RFC 3550; libwebrtc's video report
        // interval). Audio sources are reported with the video ones.
        if self.next_report.is_none_or(|at| now >= at) {
            self.next_report = Some(now + Duration::from_secs(1));
            let blocks: Vec<_> = self
                .sources
                .iter_mut()
                .filter_map(|s| s.stats.block(s.ssrc, now))
                .chain(
                    self.audio
                        .iter_mut()
                        .filter_map(|(ssrc, stats)| stats.block(*ssrc, now)),
                )
                .collect();
            if !blocks.is_empty() {
                let (header, body) = rtp::receiver_report(own_ssrc, &blocks);
                out.push((header, body));
            }
        }
        out
    }

    pub fn audio_jitter_ms(&self, ssrc: u32) -> f32 {
        self.audio
            .iter()
            .find(|(known, _)| *known == ssrc)
            .map_or(0.0, |(_, stats)| stats.jitter_ms())
    }

    pub fn video_sources_snapshot(&self) -> Vec<InboundVideoSourceSnapshot> {
        self.sources
            .iter()
            .map(|s| InboundVideoSourceSnapshot {
                user: s.user,
                ssrc: s.ssrc,
                rtx_ssrc: s.rtx,
                packets_received: u64::from(s.stats.received),
                packets_lost: s.stats.packets_lost(),
                bytes_received: s.bytes_received,
                jitter_ms: s.stats.jitter_ms(),
                nacks: s.nacks,
                plis: s.plis,
            })
            .collect()
    }
}

/// Opens every access unit that is complete and gives it to its decoder; the units a decoder cannot take, and the
/// sources whose decoder lost its reference, wait for a keyframe. Returns how many went to a decoder.
///
/// The call and the stream loops run this as soon as a packet arrives, not only on their 20 ms tick: on the tick a
/// 60 fps stream was handed over at 50 Hz, one picture in six landed on the same tick as the one before and was never
/// seen (`lossy::a_clean_stream_is_handed_over_evenly_only_when_drained_on_arrival`).
pub(crate) fn hand_over(
    receivers: &mut Receivers,
    decoders: &mut Decoders,
    dave: &mut crate::dave::Dave,
    now: Instant,
) -> u32 {
    let mut ready = Vec::new();
    receivers.drain(now, &mut ready);
    let mut offered = 0;
    for unit in ready {
        if !dave.contains(unit.user) {
            continue;
        }
        let Ok(plain) = dave
            .session
            .decrypt(unit.user, davey::MediaType::VIDEO, &unit.data)
        else {
            receivers.require_keyframe(unit.user);
            continue;
        };
        let keyframe = dc_media::h264::is_keyframe(&plain);
        if !receivers.accept(unit.user, keyframe) {
            continue;
        }
        offered += 1;
        if decoders.offer(unit.user, plain, unit.timestamp, now) == Offer::Dropped {
            receivers.require_keyframe(unit.user);
        }
    }
    for user in decoders.take_lost() {
        receivers.require_keyframe(user);
    }
    offered
}

/// An access unit and when its picture is due on screen.
struct Job {
    data: Vec<u8>,
    due: std::time::Instant,
}

#[derive(Default)]
struct DecoderStats {
    width: AtomicU32,
    height: AtomicU32,
    frames_received: AtomicU64,
    frames_decoded: AtomicU64,
    keyframes_decoded: AtomicU64,
    backend: Mutex<Option<String>>,
}

struct Slot {
    send: mpsc::SyncSender<Job>,
    /// The decoder refused something and wants a keyframe.
    lost: Arc<AtomicBool>,
    playout: Playout,
    stats: Arc<DecoderStats>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        // Closing the channel ends the thread; it is not joined here so a slow hardware decoder cannot stall the
        // audio loop.
        let _ = self.thread.take();
    }
}

/// How an access unit fared at the decoders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Offer {
    Queued,
    /// The queue was full or no decoder is available: the picture was dropped.
    Dropped,
}

/// One decoder thread per source. A `Decoder` is `!Send`, so each is created on its own thread.
pub(crate) struct Decoders {
    hub: VideoHub,
    kind: fn(u64) -> VideoSource,
    slots: HashMap<u64, Slot>,
    failed: HashMap<u64, Instant>,
}

impl Decoders {
    pub fn new(hub: VideoHub, kind: fn(u64) -> VideoSource) -> Self {
        Self {
            hub,
            kind,
            slots: HashMap::new(),
            failed: HashMap::new(),
        }
    }

    /// Queues an access unit with RTP `timestamp`, complete at `now`, for its source's decoder, which shows the
    /// picture at its playout time (`playout.rs`).
    pub fn offer(&mut self, user: u64, data: Vec<u8>, timestamp: u32, now: Instant) -> Offer {
        if !self.hub.has_sink() {
            return Offer::Dropped;
        }
        if !self.slots.contains_key(&user) {
            if self.slots.len() >= MAX_DECODERS
                || self
                    .failed
                    .get(&user)
                    .is_some_and(|at| now.saturating_duration_since(*at) < DECODER_RETRY)
            {
                return Offer::Dropped;
            }
            match self.spawn(user) {
                Some(slot) => {
                    self.slots.insert(user, slot);
                }
                None => {
                    self.failed.insert(user, now);
                    return Offer::Dropped;
                }
            }
        }
        let Some(slot) = self.slots.get_mut(&user) else {
            return Offer::Dropped;
        };
        slot.stats.frames_received.fetch_add(1, Ordering::Relaxed);
        let due = slot.playout.due(timestamp, now).into_std();
        match slot.send.try_send(Job { data, due }) {
            Ok(()) => Offer::Queued,
            Err(mpsc::TrySendError::Full(_)) => Offer::Dropped,
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.slots.remove(&user);
                self.failed.insert(user, now);
                Offer::Dropped
            }
        }
    }

    /// Users whose decoder asked for a keyframe since the last call.
    pub fn take_lost(&mut self) -> Vec<u64> {
        self.slots
            .iter()
            .filter(|(_, slot)| slot.lost.swap(false, Ordering::AcqRel))
            .map(|(user, _)| *user)
            .collect()
    }

    pub fn remove(&mut self, user: u64) {
        self.slots.remove(&user);
        self.failed.remove(&user);
    }

    pub fn video_stats(&self, user: u64) -> Option<DecodedVideoSnapshot> {
        let slot = self.slots.get(&user)?;
        Some(DecodedVideoSnapshot {
            width: slot.stats.width.load(Ordering::Relaxed),
            height: slot.stats.height.load(Ordering::Relaxed),
            frames_received: slot.stats.frames_received.load(Ordering::Relaxed),
            frames_decoded: slot.stats.frames_decoded.load(Ordering::Relaxed),
            keyframes_decoded: slot.stats.keyframes_decoded.load(Ordering::Relaxed),
            decoder_name: slot
                .stats
                .backend
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        })
    }

    fn spawn(&self, user: u64) -> Option<Slot> {
        let (send, receive) = mpsc::sync_channel::<Job>(DECODER_QUEUE);
        let lost = Arc::new(AtomicBool::new(false));
        let flag = lost.clone();
        let hub = self.hub.clone();
        let source = (self.kind)(user);
        let stats = Arc::new(DecoderStats::default());
        let decoder_stats = Arc::clone(&stats);
        let (started_send, started) = mpsc::sync_channel::<bool>(1);
        let thread = std::thread::Builder::new()
            .name("dc-voice-decode".into())
            .spawn(move || {
                let _unthrottled = dc_media::throttle::Guard::acquire();
                let waiter = dc_media::throttle::Waiter::new();
                let mut decoder = match Decoder::new() {
                    Ok(decoder) => {
                        *decoder_stats
                            .backend
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()) =
                            Some(format!("{:?}", decoder.backend()));
                        let _ = started_send.send(true);
                        decoder
                    }
                    Err(error) => {
                        tracing::debug!("video decoder unavailable: {error}");
                        let _ = started_send.send(false);
                        return;
                    }
                };
                tracing::debug!(backend = ?decoder.backend(), "video decoder started");
                // Access units wait here for their playout time, less what a decode takes, using a high-resolution
                // waitable timer so waits don't round up to Windows' 15.6 ms system clock tick.
                let mut held: std::collections::VecDeque<Job> = std::collections::VecDeque::new();
                let mut cost = Duration::from_millis(5);
                loop {
                    loop {
                        match receive.try_recv() {
                            Ok(job) => held.push_back(job),
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => return,
                        }
                    }
                    let Some(front) = held.front() else {
                        match receive.recv() {
                            Ok(job) => {
                                held.push_back(job);
                                continue;
                            }
                            Err(_) => return,
                        }
                    };
                    let start = front.due.checked_sub(cost).unwrap_or(front.due);
                    let now = std::time::Instant::now();
                    if start > now && held.len() <= MAX_HELD {
                        waiter.sleep((start - now).min(HOLD_STEP));
                        continue;
                    }
                    let Some(job) = held.pop_front() else {
                        continue;
                    };
                    let decoding = std::time::Instant::now();
                    match decoder.decode(&job.data) {
                        Ok(Some(picture)) => {
                            decoder_stats.width.store(picture.width, Ordering::Relaxed);
                            decoder_stats
                                .height
                                .store(picture.height, Ordering::Relaxed);
                            decoder_stats.frames_decoded.fetch_add(1, Ordering::Relaxed);
                            if super::h264_has_idr(&job.data) {
                                decoder_stats
                                    .keyframes_decoded
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                            hub.deliver(RemoteFrame {
                                user,
                                source,
                                picture,
                            });
                        }
                        Ok(None) => {}
                        Err(DecodeError::NeedKeyframe | DecodeError::Failed) => {
                            flag.store(true, Ordering::Release);
                        }
                        Err(DecodeError::Invalid) => {}
                    }
                    cost = (cost * 7 + decoding.elapsed()) / 8;
                    if decoder.needs_keyframe() {
                        flag.store(true, Ordering::Release);
                    }
                }
            })
            .ok()?;
        started
            .recv_timeout(Duration::from_secs(10))
            .ok()
            .filter(|ok| *ok)?;
        Some(Slot {
            send,
            lost,
            playout: Playout::default(),
            stats,
            thread: Some(thread),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rtp(payload_type: u8, ssrc: u32, sequence: u16, marker: bool, payload: &[u8]) -> Rtp {
        Rtp {
            payload_type,
            marker,
            ssrc,
            sequence,
            timestamp: 1000,
            transport_sequence: None,
            payload: payload.to_vec(),
        }
    }

    #[test]
    fn announcements_bind_one_source_per_user_within_limits() {
        let mut receivers = Receivers::default();
        receivers.announce(1, 10, 11).unwrap();
        receivers.announce(1, 10, 12).unwrap();
        assert!(
            receivers.announce(2, 10, 20).is_err(),
            "SSRC of another user"
        );
        assert!(receivers.announce(2, 20, 20).is_err());
        assert!(receivers.announce(2, 0, 21).is_err());
        receivers.announce(1, 30, 31).unwrap();
        assert!(receivers.sources.iter().any(|s| s.user == 1) && receivers.sources.len() == 1);
        for user in 2..=16u64 {
            receivers
                .announce(user, user as u32 * 100, user as u32 * 100 + 1)
                .unwrap();
        }
        assert!(
            receivers.announce(99, 9_000, 9_001).is_err(),
            "source limit"
        );
        receivers.remove(2);
        assert!(!receivers.sources.iter().any(|s| s.user == 2));
    }

    #[test]
    fn a_picture_flows_through_reorder_rtx_and_loss_recovery() {
        let now = Instant::now();
        let mut receivers = Receivers::default();
        receivers.announce(5, 100, 101).unwrap();
        let mut out = Vec::new();

        // Keyframe in three packets, the middle one arrives by RTX after a NACK.
        receivers.push(rtp(101, 100, 1, false, &[0x7c, 0x85, 1]), now);
        receivers.push(rtp(101, 100, 3, true, &[0x7c, 0x45, 3]), now);
        receivers.drain(now, &mut out);
        assert!(out.is_empty());
        let feedback = receivers.feedback(7, now + Duration::from_millis(10));
        assert!(
            feedback.iter().any(|(header, _)| header[1] == 205),
            "a NACK for the hole"
        );
        assert!(
            feedback.iter().any(|(header, _)| header[1] == 206),
            "and a PLI: no keyframe yet"
        );
        let mut repair = vec![0, 2];
        repair.extend([0x7c, 0x05, 2]);
        receivers.push(
            rtp(102, 101, 900, false, &repair),
            now + Duration::from_millis(20),
        );
        receivers.drain(now + Duration::from_millis(20), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].user, 5);
        assert_eq!(out[0].data, [0, 0, 0, 1, 0x65, 1, 2, 3]);
        // A keyframe clears the wait; a delta picture is then accepted.
        assert!(receivers.accept(5, true));
        assert!(receivers.accept(5, false));
        receivers.require_keyframe(5);
        assert!(!receivers.accept(5, false));
        assert!(!receivers.accept(6, true), "unknown user");
    }

    #[test]
    fn reports_carry_loss_of_originals_jitter_sender_report_echo_and_transport_feedback() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut receivers = Receivers::default();
        receivers.announce(5, 100, 101).unwrap();
        // Ten pictures 33 ms apart (3000 ticks at 90 kHz), the odd ones arriving 2 ms late; sequence 4 is lost and
        // comes back through RTX, which does not count as a received original.
        for n in 0..10u16 {
            if n == 4 {
                continue;
            }
            let mut packet = rtp(101, 100, n, true, &[0x41, 1]);
            packet.timestamp = u32::from(n) * 3000;
            let late = if n % 2 == 1 { 2 } else { 0 };
            receivers.push(packet, at(u64::from(n) * 100 / 3 + late));
            receivers.observe_transport(100, Some(n), at(u64::from(n) * 100 / 3 + late));
        }
        let mut repair = vec![0, 4];
        repair.extend([0x41, 1]);
        receivers.push(rtp(102, 101, 1, true, &repair), at(400));
        receivers.observe_audio(300, 7, 960, at(10));
        receivers.observe_audio(300, 8, 1920, at(30));
        // The server's sender report for the video source, 250 ms before the report.
        let mut report = vec![0x80, 200, 0, 6];
        report.extend(100u32.to_be_bytes());
        report.extend([0, 0, 0xab, 0xcd, 0x12, 0x34, 0, 0]);
        report.extend([0; 12]);
        receivers.sender_reports(&report, at(150));
        let feedback = receivers.feedback(7, at(400));
        let rr = feedback
            .iter()
            .find(|(header, _)| header[1] == 201)
            .expect("a receiver report");
        assert_eq!(rr.0[0] & 0x1f, 2, "video and audio blocks");
        let mut compound = rr.0.to_vec();
        compound.extend(&rr.1);
        let video = &rr.1[..24];
        assert_eq!(&video[..4], &100u32.to_be_bytes());
        assert_eq!(video[4], 25, "one original in ten lost: 25/256");
        assert_eq!(&video[5..8], &[0, 0, 1]);
        assert_eq!(&video[8..12], &9u32.to_be_bytes(), "highest sequence");
        let jitter = u32::from_be_bytes(video[12..16].try_into().unwrap());
        // About 2 ms (180 ticks) of alternating delay, smoothed over 1/16 steps.
        assert!((20..=180).contains(&jitter), "jitter {jitter}");
        assert_eq!(&video[16..20], &0xabcd_1234u32.to_be_bytes(), "LSR");
        let delay = u32::from_be_bytes(video[20..24].try_into().unwrap());
        assert!(
            (16_000..=17_000).contains(&delay),
            "DLSR of 250 ms: {delay}"
        );
        assert_eq!(&rr.1[24..28], &300u32.to_be_bytes());
        // Transport-wide feedback for the nine numbered packets, from our SSRC about the video source.
        let twcc = feedback
            .iter()
            .find(|(header, _)| header[0] == 0x8f && header[1] == 205)
            .expect("transport-wide feedback");
        assert_eq!(&twcc.0[4..8], &7u32.to_be_bytes());
        assert_eq!(&twcc.1[..4], &100u32.to_be_bytes());
        assert_eq!(
            &twcc.1[4..8],
            &[0, 0, 0, 10],
            "base 0, ten statuses with 4 lost"
        );
        assert!(
            receivers.feedback(7, at(450)).is_empty(),
            "nothing due 50 ms later"
        );
    }

    #[test]
    fn plis_are_capped_and_rtx_of_strangers_is_ignored() {
        let now = Instant::now();
        let mut receivers = Receivers::default();
        receivers.announce(5, 100, 101).unwrap();
        let first = receivers.feedback(7, now);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].0[1], 206);
        assert!(
            receivers
                .feedback(7, now + Duration::from_millis(100))
                .is_empty()
        );
        assert_eq!(
            receivers
                .feedback(7, now + Duration::from_millis(500))
                .len(),
            1
        );
        // Unknown RTX SSRC, short RTX, unknown payload type and unknown media SSRC change nothing.
        receivers.push(rtp(102, 555, 1, true, &[0, 1, 2]), now);
        receivers.push(rtp(102, 101, 1, true, &[0, 1]), now);
        receivers.push(rtp(96, 100, 1, true, &[0x41, 1]), now);
        receivers.push(rtp(101, 999, 1, true, &[0x41, 1]), now);
        let mut out = Vec::new();
        receivers.drain(now, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn decoders_drop_without_a_sink_and_decode_with_one() {
        use dc_media::synthetic::Generator;
        let hub = VideoHub::new();
        let mut decoders = Decoders::new(hub.clone(), |_| VideoSource::Camera);
        let mut generator = Generator::new(160, 120, 15, 300_000).unwrap();
        let frame = generator.next(false).unwrap().unwrap();
        let now = Instant::now();
        assert_eq!(
            decoders.offer(1, frame.data.clone(), 0, now),
            Offer::Dropped
        );

        let (send, receive) = mpsc::channel();
        hub.set_sink(Some(Arc::new(move |f| {
            let _ = send.send(f);
        })));
        assert_eq!(decoders.offer(1, frame.data, 0, now), Offer::Queued);
        let decoded = receive.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!((decoded.user, decoded.source), (1, VideoSource::Camera));
        assert_eq!((decoded.picture.width, decoded.picture.height), (160, 120));

        // A delta picture without a reference asks for a keyframe.
        let mut fresh = Decoders::new(hub, |_| VideoSource::Camera);
        let delta = generator.next(false).unwrap().unwrap();
        assert!(!delta.keyframe);
        assert_eq!(fresh.offer(2, delta.data, 0, now), Offer::Queued);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if fresh.take_lost() == [2] {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the decoder never asked for a keyframe");
    }
}
