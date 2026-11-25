//! Paces the microphone into RTP packets: one packet per captured 20 ms frame, timestamped by the frames themselves.
//!
//! The official engine (libwebrtc's `AudioSendStream` in `discord_voice.node`) encodes on the capture side as each
//! 20 ms of microphone audio completes, and its RTP timestamp counts captured samples: the microphone's clock paces
//! the packets, and the network side only carries them. The receiver's NetEq then sees the microphone's own timing,
//! and late or bunched frames are only jitter, which it absorbs.
//!
//! What this replaces (Serein's pacer, kept test-only below as [`tests::Lookahead`]): a 20 ms wall-clock tick took one
//! frame with one frame of lookahead, and a tick that found none ended the talk spurt with an Opus silence packet. The
//! audio worker delivers frames in bursts (its sleep, the noise suppressor's wait), and the voice loop misses ticks
//! while it works through a burst of network traffic, so mid-speech a tick often found nothing: a 20 ms hole of
//! silence went out where the voice belonged (a crackle at every listener), the timestamps ran on the tick while the
//! audio ran on the microphone, and a stall of four ticks threw the queued speech away.
//!
//! Here every frame the worker captured goes out in order, however the ticks fall, with the timestamp of its place
//! on the microphone's timeline. The worker says which frames the voice gate lets out, so the end of a spurt is known
//! rather than guessed from a late frame. While nothing is captured (muted, or no microphone), the timestamp follows
//! the wall clock, as Yapfast's does across stalls, so the next spurt starts where a listener expects it.
use crate::Frame;
use std::sync::mpsc::Receiver;
use std::time::Instant;

/// Samples per channel of one 20 ms frame at 48 kHz.
const FRAME_SAMPLES: u32 = 960;
/// One tick of the voice loop, in microseconds.
const TICK_US: u128 = 20_000;
/// Opus silence frames that close a talk spurt, as Serein sends them.
pub(crate) const CLOSING_SILENCE: u8 = 5;
/// The most frames one tick takes; the capture channel holds 16.
const MAX_PER_TICK: usize = 16;
/// Wall-clock ticks in a row without a frame in an open spurt before the microphone counts as gone and the spurt
/// closes: 200 ms. Counted afresh from every frame, so a microphone's drift against the wall clock never adds up here.
const STALL_TICKS: i64 = 10;
/// Outside a spurt, how many frames the wall clock may run ahead of the captured ones before the timestamp follows it.
const MAX_BEHIND: i64 = 5;
/// The most a fast microphone's surplus is remembered, in frames.
const MAX_AHEAD: i64 = -50;

/// One microphone frame from the audio worker, processed, and whether the voice gate lets it out.
#[derive(Clone, Copy)]
pub(crate) struct Captured {
    pub frame: Frame,
    pub open: bool,
}

/// What the voice loop does this tick, in order.
// A frame rides in its step by value: the loop reuses one vector of steps, and boxing would allocate every 20 ms.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
pub(crate) enum Step {
    /// Op 5 speaking on or off.
    Speaking(bool),
    /// A voice frame to encode and send at this RTP timestamp.
    Voice { frame: Frame, timestamp: u32 },
    /// One of the Opus silence frames closing a spurt.
    Silence { timestamp: u32 },
}

pub(crate) struct SendPacer {
    /// The RTP timestamp of the next frame.
    timestamp: u32,
    speaking: bool,
    /// Closing silence frames still owed.
    silence: u8,
    /// The wall clock's start, and the ticks counted from it so far: each tick's share is the rounded total less what
    /// was counted, so rounding never accumulates.
    origin: Option<Instant>,
    ticks: i64,
    /// Frames the wall clock has passed that the timestamp has not (negative: the microphone is ahead). Only acted on
    /// between spurts.
    behind: i64,
    /// Wall-clock ticks since the last frame arrived.
    empty: i64,
    /// The last voice frame of the open spurt, for the activity meter on a tick whose frame is late.
    last_voice: Option<Frame>,
}

impl SendPacer {
    pub(crate) fn new(timestamp: u32) -> Self {
        Self {
            timestamp,
            speaking: false,
            silence: 0,
            origin: None,
            ticks: 0,
            behind: 0,
            empty: 0,
            last_voice: None,
        }
    }

    /// A spurt is open (op 5 said speaking), for the speaking state a resumed control socket repeats.
    pub(crate) fn speaking(&self) -> bool {
        self.speaking
    }

    /// Ticks of wall clock since the last call: the first call counts one.
    fn due(&mut self, now: Instant) -> i64 {
        let origin = *self.origin.get_or_insert(now);
        let total = ((now.saturating_duration_since(origin).as_micros() + TICK_US / 2) / TICK_US)
            as i64
            + 1;
        let due = (total - self.ticks).max(0);
        self.ticks += due;
        due
    }

    fn advance(&mut self, frames: i64) {
        if frames > 0 {
            self.timestamp = self
                .timestamp
                .wrapping_add(FRAME_SAMPLES.wrapping_mul(frames as u32));
            self.behind -= frames;
        }
    }

    /// One closing silence frame at the current timestamp; the last one turns speaking off.
    fn close_one(&mut self, steps: &mut Vec<Step>) {
        if self.silence == 0 {
            self.silence = CLOSING_SILENCE;
        }
        steps.push(Step::Silence {
            timestamp: self.timestamp,
        });
        self.silence -= 1;
        if self.silence == 0 {
            self.speaking = false;
            self.last_voice = None;
            steps.push(Step::Speaking(false));
        }
    }

    /// Alone in the call: nothing goes out. The newest frame the gate lets out, for the local speaking indicator.
    pub(crate) fn idle(&mut self, input: &Receiver<Captured>, now: Instant) -> Option<Frame> {
        self.behind += self.due(now);
        let mut latest = None;
        for _ in 0..MAX_PER_TICK {
            let Ok(captured) = input.try_recv() else {
                break;
            };
            if captured.open {
                latest = Some(captured.frame);
            }
        }
        self.speaking = false;
        self.silence = 0;
        self.empty = 0;
        self.last_voice = None;
        self.advance(self.behind.max(0));
        latest
    }

    /// One tick: what was captured since the last, as steps for the loop. `sending`: the call is secure and the
    /// microphone neither muted nor deafened. The newest voice frame, for the speaking indicator and the meter.
    pub(crate) fn tick(
        &mut self,
        input: &Receiver<Captured>,
        sending: bool,
        now: Instant,
        steps: &mut Vec<Step>,
    ) -> Option<Frame> {
        let due = self.due(now);
        self.behind += due;
        if !sending {
            self.empty = 0;
            for _ in 0..MAX_PER_TICK {
                if input.try_recv().is_err() {
                    break;
                }
            }
            // Muted mid-spurt: the closing silence goes out one frame a tick, on the wall clock.
            if self.speaking {
                self.close_one(steps);
                self.advance(1);
            }
            self.advance(self.behind.max(0));
            return None;
        }
        let mut frames = 0;
        let mut latest = None;
        for _ in 0..MAX_PER_TICK {
            let Ok(captured) = input.try_recv() else {
                break;
            };
            frames += 1;
            if captured.open {
                if !self.speaking {
                    self.speaking = true;
                    steps.push(Step::Speaking(true));
                }
                self.silence = 0;
                steps.push(Step::Voice {
                    frame: captured.frame,
                    timestamp: self.timestamp,
                });
                self.last_voice = Some(captured.frame);
                latest = Some(captured.frame);
            } else if self.speaking {
                self.close_one(steps);
            }
            self.advance(1);
        }
        self.behind = self.behind.max(MAX_AHEAD);
        self.empty = if frames == 0 { self.empty + due } else { 0 };
        if frames == 0 && self.speaking && self.empty > STALL_TICKS {
            // The microphone stopped delivering in the middle of a spurt: close it on the wall clock.
            self.close_one(steps);
            self.advance(1);
        }
        // Never inside a spurt: a jump there would be a hole for every listener.
        if !self.speaking && self.behind > MAX_BEHIND {
            self.advance(self.behind - MAX_BEHIND);
        }
        latest.or(if self.speaking { self.last_voice } else { None })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::neteq::{NetEq, OUTPUT_SIZE};
    use opus2::{Application, Bitrate, Channels, Encoder};
    use std::collections::HashMap;
    use std::sync::mpsc::{SyncSender, sync_channel};
    use std::time::Duration;

    /// The pacer this replaces (Serein's one-frame lookahead) with the session's old send decision around it: a frame
    /// a tick, an Opus silence frame when a tick finds none, the timestamp on the tick, and queued speech dropped
    /// after a stall of 80 ms. Test-only, so the comparison can be re-run.
    #[derive(Default)]
    pub(crate) struct Lookahead {
        pending: Option<Frame>,
        speaking: bool,
        silence: u8,
        timestamp: u32,
        last: Option<Instant>,
    }

    impl Lookahead {
        fn tick(&mut self, input: &Receiver<Frame>, now: Instant, steps: &mut Vec<Step>) {
            let stalled = self
                .last
                .is_some_and(|last| now.duration_since(last) >= Duration::from_millis(80));
            self.last = Some(now);
            let latest = if stalled {
                self.pending = None;
                for _ in 0..8 {
                    if input.try_recv().is_err() {
                        break;
                    }
                }
                None
            } else {
                let frame = self.pending.take();
                self.pending = input.try_recv().ok();
                frame
            };
            let active = latest.is_some();
            if active && !self.speaking {
                steps.push(Step::Speaking(true));
                self.speaking = true;
            }
            if !active && self.speaking && self.silence == 0 {
                self.silence = CLOSING_SILENCE;
            }
            if active || self.silence > 0 {
                match latest {
                    Some(frame) => steps.push(Step::Voice {
                        frame,
                        timestamp: self.timestamp,
                    }),
                    None => {
                        self.silence -= 1;
                        steps.push(Step::Silence {
                            timestamp: self.timestamp,
                        });
                    }
                }
                if !active && self.silence == 0 {
                    steps.push(Step::Speaking(false));
                    self.speaking = false;
                }
                if active {
                    self.silence = 0;
                }
            }
            self.timestamp = self.timestamp.wrapping_add(FRAME_SAMPLES);
        }
    }

    struct Rng(u64);

    impl Rng {
        fn uniform(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    const SPEECH_FRAMES: usize = 750;

    /// 15 s of a steady voiced tone, as frames; any hole in it is audible and measurable.
    fn speech() -> Vec<Frame> {
        (0..SPEECH_FRAMES)
            .map(|k| {
                std::array::from_fn(|i| {
                    let n = (k * 960 + i) as f32;
                    0.3 * (n * std::f32::consts::TAU * 210.0 / 48_000.0).sin()
                })
            })
            .collect()
    }

    fn id(frame: &Frame) -> [u32; 4] {
        [frame[0], frame[1], frame[2], frame[3]].map(f32::to_bits)
    }

    /// When things happen, in milliseconds: frame k is complete on the microphone's clock (`drift_ppm` off the wall
    /// clock); the audio worker wakes about every 15.6 ms (a 5 ms park on the Windows tick) and now and then 15 ms
    /// late (the noise suppressor's wait), and hands over what is complete; the voice loop handles the 20 ms clock's
    /// ticks, but now and then works 25 to 90 ms through a burst of network traffic, keeping one tick and losing the
    /// rest (the clock's channel holds one).
    struct Timeline {
        ready: Vec<f64>,
        wakes: Vec<f64>,
        ticks: Vec<f64>,
    }

    fn timeline(seed: u64, drift_ppm: f64) -> Timeline {
        let mut rng = Rng(seed);
        let ready: Vec<f64> = (0..SPEECH_FRAMES)
            .map(|k| (k + 1) as f64 * 20.0 * (1.0 + drift_ppm * 1e-6))
            .collect();
        let end = ready[SPEECH_FRAMES - 1] + 400.0;
        let mut wakes = Vec::new();
        let mut at = 0.0;
        while at < end {
            at += 15.625 + rng.uniform() * 2.0;
            if rng.uniform() < 0.05 {
                at += 15.0;
            }
            wakes.push(at);
        }
        let mut ticks = Vec::new();
        let (mut free_at, mut pending) = (0.0f64, None::<f64>);
        let handle = |at: f64, ticks: &mut Vec<f64>, rng: &mut Rng, free_at: &mut f64| {
            ticks.push(at);
            if rng.uniform() < 0.03 {
                *free_at = at + 25.0 + rng.uniform() * 65.0;
            }
        };
        let mut i = 1;
        loop {
            let clock = f64::from(i) * 20.0 + rng.uniform() * 1.5;
            if clock > end {
                break;
            }
            if let Some(kept) = pending
                && free_at <= clock
            {
                pending = None;
                handle(free_at.max(kept), &mut ticks, &mut rng, &mut free_at);
            }
            if clock >= free_at {
                handle(clock, &mut ticks, &mut rng, &mut free_at);
            } else if pending.is_none() {
                pending = Some(clock);
            }
            i += 1;
        }
        Timeline {
            ready,
            wakes,
            ticks,
        }
    }

    /// Runs the worker's hand-over and the loop's ticks in time order; `tick` gets each handled tick's time.
    fn drive<T>(
        timeline: &Timeline,
        frames: &[Frame],
        send: &SyncSender<T>,
        wrap: impl Fn(Frame) -> T,
        mut tick: impl FnMut(f64),
    ) {
        let mut next_frame = 0;
        let mut wakes = timeline.wakes.iter().peekable();
        for &at in &timeline.ticks {
            while let Some(&&wake) = wakes.peek() {
                if wake > at {
                    break;
                }
                wakes.next();
                while next_frame < frames.len() && timeline.ready[next_frame] <= wake {
                    // A full channel drops the frame, as the worker's `try_send` does.
                    let _ = send.try_send(wrap(frames[next_frame]));
                    next_frame += 1;
                }
            }
            tick(at);
        }
    }

    fn sent_by_new(timeline: &Timeline, frames: &[Frame]) -> Vec<(f64, Step)> {
        let (send, receive) = sync_channel::<Captured>(16);
        let start = Instant::now();
        let mut pacer = SendPacer::new(7_000);
        let mut sent = Vec::new();
        let mut steps = Vec::new();
        drive(
            timeline,
            frames,
            &send,
            |frame| Captured { frame, open: true },
            |at| {
                let now = start + Duration::from_micros((at * 1000.0) as u64);
                let _ = pacer.tick(&receive, true, now, &mut steps);
                sent.extend(steps.drain(..).map(|step| (at, step)));
            },
        );
        sent
    }

    fn sent_by_old(timeline: &Timeline, frames: &[Frame]) -> Vec<(f64, Step)> {
        let (send, receive) = sync_channel::<Frame>(8);
        let start = Instant::now();
        let mut pacer = Lookahead::default();
        let mut sent = Vec::new();
        let mut steps = Vec::new();
        drive(
            timeline,
            frames,
            &send,
            |frame| frame,
            |at| {
                let now = start + Duration::from_micros((at * 1000.0) as u64);
                pacer.tick(&receive, now, &mut steps);
                sent.extend(steps.drain(..).map(|step| (at, step)));
            },
        );
        sent
    }

    #[derive(Debug, Default)]
    struct Sent {
        /// Silence packets sent before the last voice packet: holes in the middle of speech.
        holes: usize,
        /// Captured frames never sent.
        lost: usize,
        /// Voice packets whose timestamp is not their frame's place on the microphone's timeline.
        misplaced: usize,
    }

    fn inspect(sent: &[(f64, Step)], frames: &[Frame]) -> Sent {
        let index: HashMap<[u32; 4], usize> =
            frames.iter().enumerate().map(|(k, f)| (id(f), k)).collect();
        let voice: Vec<(usize, u32)> = sent
            .iter()
            .filter_map(|(_, step)| match step {
                Step::Voice { frame, timestamp } => Some((index[&id(frame)], *timestamp)),
                _ => None,
            })
            .collect();
        let last_voice = sent
            .iter()
            .rposition(|(_, step)| matches!(step, Step::Voice { .. }))
            .unwrap_or(0);
        let (first, first_ts) = voice[0];
        Sent {
            holes: sent[..last_voice]
                .iter()
                .filter(|(_, step)| matches!(step, Step::Silence { .. }))
                .count(),
            lost: frames.len() - voice.len(),
            misplaced: voice
                .iter()
                .filter(|(k, ts)| {
                    *ts != first_ts.wrapping_add(
                        ((*k as u32).wrapping_sub(first as u32)).wrapping_mul(FRAME_SAMPLES),
                    )
                })
                .count(),
        }
    }

    /// The packets over a steady 40 ms path into the receiver (NetEq, pulled every 10 ms on its own clock): the 10 ms
    /// blocks inside the speech that came out near silent.
    fn dropouts(sent: &[(f64, Step)]) -> usize {
        let mut encoder = Encoder::new(48_000, Channels::Mono, Application::Voip).unwrap();
        encoder.set_bitrate(Bitrate::Bits(64_000)).unwrap();
        encoder.set_inband_fec(true).unwrap();
        encoder.set_packet_loss_perc(30).unwrap();
        let mut packets = Vec::new();
        let mut sequence = 0u16;
        for (at, step) in sent {
            let mut out = [0u8; 1275];
            let (timestamp, payload) = match step {
                Step::Voice { frame, timestamp } => {
                    let n = encoder.encode_float(frame, &mut out).unwrap();
                    (*timestamp, out[..n].to_vec())
                }
                Step::Silence { timestamp } => {
                    let _ = encoder.encode_float(&[0.0; 960], &mut out);
                    (*timestamp, vec![0xF8, 0xFF, 0xFE])
                }
                Step::Speaking(_) => continue,
            };
            packets.push((at + 40.0, sequence, timestamp, payload));
            sequence = sequence.wrapping_add(1);
        }
        let mut neteq = NetEq::new(false).unwrap();
        let mut output = Vec::new();
        let mut block = [0.0f32; OUTPUT_SIZE];
        let mut next = 0;
        let end = packets.last().unwrap().0 + 500.0;
        let mut now = 0.0;
        while now < end {
            while next < packets.len() && packets[next].0 <= now {
                let (_, sequence, timestamp, payload) = &packets[next];
                neteq.insert_packet(*sequence, *timestamp, payload);
                next += 1;
            }
            neteq.get_audio(&mut block);
            output.extend_from_slice(&block);
            now += 10.0;
        }
        let audible = |block: &[f32]| {
            (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt() > 0.07
        };
        let blocks: Vec<bool> = output.chunks(OUTPUT_SIZE).map(audible).collect();
        let first = blocks.iter().position(|b| *b).unwrap();
        let last = blocks.iter().rposition(|b| *b).unwrap();
        // The stream simply stops after the last frame: the concealment tail of the last 250 ms is not speech.
        blocks[first + 5..last.saturating_sub(25)]
            .iter()
            .filter(|b| !**b)
            .count()
    }

    #[test]
    fn worker_bursts_and_lost_ticks_never_punch_holes_in_speech() {
        let frames = speech();
        for (seed, drift) in [
            (0x5eed_0001, 0.0),
            (0x5eed_0002, -300.0),
            (0x5eed_0003, 300.0),
        ] {
            let timeline = timeline(seed, drift);
            let new = inspect(&sent_by_new(&timeline, &frames), &frames);
            assert_eq!(new.holes, 0, "seed {seed:x}: {new:?}");
            assert_eq!(new.lost, 0, "seed {seed:x}: {new:?}");
            assert_eq!(new.misplaced, 0, "seed {seed:x}: {new:?}");
            // The old pacer, on the same timeline, put silence into the speech and lost or misplaced frames.
            let old = inspect(&sent_by_old(&timeline, &frames), &frames);
            assert!(
                old.holes > 0 && old.lost + old.misplaced > 0,
                "seed {seed:x}: {old:?}"
            );
        }
    }

    #[test]
    fn the_listener_hears_continuous_speech_where_the_old_pacer_made_dropouts() {
        let frames = speech();
        let timeline = timeline(0x5eed_0004, -200.0);
        let new = dropouts(&sent_by_new(&timeline, &frames));
        let old = dropouts(&sent_by_old(&timeline, &frames));
        assert_eq!(new, 0, "dropouts with the capture-clocked pacer");
        assert!(old > 0, "the old pacer's dropouts: {old}");
    }

    fn frame(value: f32) -> Frame {
        [value; 960]
    }

    fn captured(value: f32, open: bool) -> Captured {
        Captured {
            frame: frame(value),
            open,
        }
    }

    #[test]
    fn a_spurt_opens_and_closes_with_the_gate_and_five_silence_frames() {
        let (send, receive) = sync_channel::<Captured>(16);
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut pacer = SendPacer::new(0);
        let mut steps = Vec::new();
        send.try_send(captured(0.1, true)).unwrap();
        send.try_send(captured(0.2, true)).unwrap();
        assert_eq!(
            pacer.tick(&receive, true, at(20), &mut steps),
            Some(frame(0.2))
        );
        assert_eq!(
            steps,
            [
                Step::Speaking(true),
                Step::Voice {
                    frame: frame(0.1),
                    timestamp: 0
                },
                Step::Voice {
                    frame: frame(0.2),
                    timestamp: 960
                },
            ]
        );
        steps.clear();
        // A late frame: nothing goes out and the spurt stays open.
        assert_eq!(
            pacer.tick(&receive, true, at(40), &mut steps),
            Some(frame(0.2))
        );
        assert!(steps.is_empty());
        send.try_send(captured(0.3, true)).unwrap();
        for _ in 0..CLOSING_SILENCE {
            send.try_send(captured(0.0, false)).unwrap();
        }
        let _ = pacer.tick(&receive, true, at(60), &mut steps);
        assert_eq!(
            steps[0],
            Step::Voice {
                frame: frame(0.3),
                timestamp: 1920
            }
        );
        assert_eq!(
            steps[1..],
            [
                Step::Silence { timestamp: 2880 },
                Step::Silence { timestamp: 3840 },
                Step::Silence { timestamp: 4800 },
                Step::Silence { timestamp: 5760 },
                Step::Silence { timestamp: 6720 },
                Step::Speaking(false),
            ]
        );
        steps.clear();
        // The gate closed: its frames keep the timeline and nothing goes out.
        send.try_send(captured(0.0, false)).unwrap();
        let _ = pacer.tick(&receive, true, at(80), &mut steps);
        assert!(steps.is_empty());
        send.try_send(captured(0.4, true)).unwrap();
        let _ = pacer.tick(&receive, true, at(100), &mut steps);
        assert_eq!(
            steps[1],
            Step::Voice {
                frame: frame(0.4),
                timestamp: 8640
            }
        );
    }

    #[test]
    fn muting_closes_the_spurt_and_the_timestamp_then_runs_on_the_wall_clock() {
        let (send, receive) = sync_channel::<Captured>(16);
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut pacer = SendPacer::new(0);
        let mut steps = Vec::new();
        send.try_send(captured(0.1, true)).unwrap();
        let _ = pacer.tick(&receive, true, at(20), &mut steps);
        steps.clear();
        for tick in 0..u64::from(CLOSING_SILENCE) {
            assert_eq!(
                pacer.tick(&receive, false, at(40 + 20 * tick), &mut steps),
                None
            );
        }
        assert_eq!(steps.len(), usize::from(CLOSING_SILENCE) + 1);
        assert_eq!(steps[0], Step::Silence { timestamp: 960 });
        assert_eq!(steps.last(), Some(&Step::Speaking(false)));
        steps.clear();
        // About a second muted, with the ticks a stall lost: the next frame sits that much later on the timeline.
        let _ = pacer.tick(&receive, false, at(1120), &mut steps);
        send.try_send(captured(0.5, true)).unwrap();
        let _ = pacer.tick(&receive, true, at(1140), &mut steps);
        assert_eq!(
            steps[1],
            Step::Voice {
                frame: frame(0.5),
                timestamp: 960 * 56
            }
        );
    }

    /// Thirty minutes of an always-open gate (lowest sensitivity, or music) from a microphone 300 ppm slower than the
    /// system clock: its frames fall further and further behind the wall clock, which must never end the spurt or put
    /// silence or a timestamp jump into it.
    #[test]
    fn a_slow_microphone_never_breaks_a_long_spurt() {
        const FRAMES: usize = 30 * 60 * 50;
        let (send, receive) = sync_channel::<Captured>(16);
        let start = Instant::now();
        let mut pacer = SendPacer::new(0);
        let mut steps = Vec::new();
        let (mut silences, mut voices, mut expected) = (0usize, 0usize, 0u32);
        let mut next_frame = 0usize;
        let mut wake = 0f64;
        let mut tick = 0u64;
        while next_frame < FRAMES {
            tick += 1;
            let at = tick as f64 * 20.0;
            // The worker hands over what the microphone completed, every 15.6 ms.
            while wake + 15.625 <= at {
                wake += 15.625;
                while next_frame < FRAMES && (next_frame + 1) as f64 * 20.0 * (1.0 + 300e-6) <= wake
                {
                    send.try_send(captured(0.3, true)).unwrap();
                    next_frame += 1;
                }
            }
            let _ = pacer.tick(
                &receive,
                true,
                start + Duration::from_micros((at * 1000.0) as u64),
                &mut steps,
            );
            for step in steps.drain(..) {
                match step {
                    Step::Silence { .. } => silences += 1,
                    Step::Voice { timestamp, .. } => {
                        assert_eq!(timestamp, expected, "frame {voices}");
                        expected = expected.wrapping_add(FRAME_SAMPLES);
                        voices += 1;
                    }
                    Step::Speaking(on) => assert!(on && voices == 0, "speaking {on} at {voices}"),
                }
            }
        }
        let _ = pacer.tick(
            &receive,
            true,
            start + Duration::from_micros(((tick + 1) as f64 * 20_000.0) as u64),
            &mut steps,
        );
        voices += steps
            .iter()
            .filter(|step| matches!(step, Step::Voice { .. }))
            .count();
        assert_eq!(silences, 0);
        assert_eq!(voices, FRAMES);
    }

    #[test]
    fn a_microphone_that_stops_mid_spurt_closes_it_after_200_ms() {
        let (send, receive) = sync_channel::<Captured>(16);
        let start = Instant::now();
        let mut pacer = SendPacer::new(0);
        let mut steps = Vec::new();
        send.try_send(captured(0.1, true)).unwrap();
        let _ = pacer.tick(&receive, true, start, &mut steps);
        steps.clear();
        let mut tick = 0;
        while !steps.contains(&Step::Speaking(false)) {
            tick += 1;
            let _ = pacer.tick(
                &receive,
                true,
                start + Duration::from_millis(20 * tick),
                &mut steps,
            );
            assert!(tick < 40, "the spurt never closed");
        }
        assert!(matches!(steps[0], Step::Silence { .. }));
        assert!(tick > STALL_TICKS as u64, "closed after {tick} ticks");
    }
}
