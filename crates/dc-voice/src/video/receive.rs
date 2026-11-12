//! Receiving video: RTP of announced sources is reordered, repaired (NACK, RTX), reassembled into access units and,
//! once DAVE has opened them, decoded on one thread per source. Everything is bounded; nothing here blocks the caller.
//!
//! Adapted from Serein `crates/discord-voice/src/video_receive.rs` (`Receivers`, `decode_loop` in outline) and
//! `transport.rs` (`decrypt_video`, `receive_video`), MIT OR Apache-2.0. Differences: H.264 only, one decoder thread per
//! source whose results go straight to the app's sink, and no playout buffer (a camera is shown as it arrives).
use super::{
    RemoteFrame, VideoHub, VideoSource, depacketize::Assembler, playout::Playout, reorder::Window,
};
use crate::rtp::{self, H264_PAYLOAD_TYPE, H264_RTX_PAYLOAD_TYPE, Rtp};
use dc_media::decode::{DecodeError, Decoder};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
/// A source that wants a keyframe is asked at most this often (Serein: two a second).
const PLI_INTERVAL: Duration = Duration::from_millis(500);
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
}

/// RFC 3550 reception statistics of one source, for the receiver report.
#[derive(Default)]
struct Stats {
    base: u32,
    highest: Option<u32>,
    received: u32,
    expected_prior: u32,
    received_prior: u32,
}

impl Stats {
    fn observe(&mut self, sequence: u16) {
        self.received = self.received.wrapping_add(1);
        match self.highest {
            None => {
                self.base = u32::from(sequence);
                self.highest = Some(u32::from(sequence));
            }
            Some(highest) => {
                let delta = sequence.wrapping_sub(highest as u16);
                if delta != 0 && delta < 0x8000 {
                    self.highest = Some(highest.wrapping_add(u32::from(delta)));
                }
            }
        }
    }

    /// The block for the interval since the last one, or `None` before the first packet.
    fn block(&mut self, ssrc: u32) -> Option<rtp::ReportBlock> {
        let highest = self.highest?;
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
        Some(rtp::ReportBlock {
            ssrc,
            fraction_lost: fraction,
            cumulative_lost: lost.clamp(-0x80_0000, 0x7f_ffff) as i32,
            highest,
            jitter: 0,
            last_sender_report: 0,
            delay: 0,
        })
    }
}

#[derive(Default)]
pub(crate) struct Receivers {
    sources: Vec<Source>,
    next_report: Option<Instant>,
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
            stats: Stats::default(),
        });
        Ok(())
    }

    pub fn remove(&mut self, user: u64) {
        self.sources.retain(|s| s.user != user);
    }

    /// Queues one authenticated RTP packet of payload type 101 or 102. Unknown SSRCs are dropped.
    pub fn push(&mut self, mut rtp: Rtp, now: Instant) {
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
        source.stats.observe(sequence);
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

    /// The feedback to send now: NACKs for observed holes, and a PLI per source that wants a keyframe (capped at two a
    /// second each). `own_ssrc` is our audio SSRC, the RTCP sender.
    pub fn feedback(&mut self, own_ssrc: u32, now: Instant) -> Vec<([u8; 8], Vec<u8>)> {
        let mut out = Vec::new();
        for source in &mut self.sources {
            if let Some(nack) = source.window.nack(own_ssrc, source.ssrc, now) {
                out.push(nack);
            }
            if source.awaiting
                && source
                    .last_pli
                    .is_none_or(|at| now.saturating_duration_since(at) >= PLI_INTERVAL)
            {
                source.last_pli = Some(now);
                let (header, body) = rtp::pli(own_ssrc, source.ssrc);
                out.push((header, body.to_vec()));
            }
        }
        // A receiver report a second keeps the sender's rate control informed (RFC 3550).
        if self.next_report.is_none_or(|at| now >= at) {
            self.next_report = Some(now + Duration::from_secs(1));
            let blocks: Vec<_> = self
                .sources
                .iter_mut()
                .filter_map(|s| s.stats.block(s.ssrc))
                .collect();
            if !blocks.is_empty() {
                let (header, body) = rtp::receiver_report(own_ssrc, &blocks);
                out.push((header, body));
            }
        }
        out
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

struct Slot {
    send: mpsc::SyncSender<Job>,
    /// The decoder refused something and wants a keyframe.
    lost: Arc<AtomicBool>,
    playout: Playout,
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

    fn spawn(&self, user: u64) -> Option<Slot> {
        let (send, receive) = mpsc::sync_channel::<Job>(DECODER_QUEUE);
        let lost = Arc::new(AtomicBool::new(false));
        let flag = lost.clone();
        let hub = self.hub.clone();
        let source = (self.kind)(user);
        let (started_send, started) = mpsc::sync_channel::<bool>(1);
        let thread = std::thread::Builder::new()
            .name("dc-voice-decode".into())
            .spawn(move || {
                let mut decoder = match Decoder::new() {
                    Ok(decoder) => {
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
                // Access units wait here for their playout time, less what a decode takes. The wait is a sleep
                // (a high-resolution timer); a timed channel wait would round to Windows' 15.6 ms tick.
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
                        std::thread::sleep((start - now).min(HOLD_STEP));
                        continue;
                    }
                    let Some(job) = held.pop_front() else {
                        continue;
                    };
                    let decoding = std::time::Instant::now();
                    match decoder.decode(&job.data) {
                        Ok(Some(picture)) => hub.deliver(RemoteFrame {
                            user,
                            source,
                            picture,
                        }),
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
