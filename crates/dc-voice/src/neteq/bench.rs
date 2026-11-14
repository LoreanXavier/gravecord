//! Test bench of the voice receive path: no devices, no network. A deterministic speech-like signal (voiced harmonics
//! under moving formants, syllables, talk spurts and pauses) is encoded as a Discord peer sends it (Opus VoIP 64 kb/s,
//! in-band FEC at the client's `packetLossRate` 0.3, five silence frames after each spurt, then nothing while the RTP
//! timestamp runs on), goes through a network model (delay, jitter, Wi-Fi stalls that release packets in bursts,
//! random and bursty loss, reordering, sender clock drift), and is played by a receiver popping 20 ms frames on its
//! own clock. The played signal is aligned to the clean decode window by window and measured.
//!
//! Two receivers: the old one (Serein's fixed window, `crate::jitter::Jitter`, and the old mixer's decode loop, kept
//! here test-only so the comparison can be re-run) and [`super::NetEq`].
use super::{NetEq, OUTPUT_SIZE};
use crate::playout::{CallPlayout, PlayoutReader};
use opus2::{Application, Bitrate, Channels, Decoder, Encoder};
use std::sync::{Arc, OnceLock, atomic::AtomicU32};

const RATE: usize = 48_000;
const FRAME: usize = 960;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.uniform()
    }
    fn exponential(&mut self, mean: f64) -> f64 {
        -mean * (1.0 - self.uniform()).max(1e-12).ln()
    }
    fn normal(&mut self) -> f64 {
        let (a, b) = (self.uniform().max(1e-12), self.uniform());
        (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
    }
}

/// The source: 60 s of speech-like audio, and per 20 ms frame whether the talker is in a spurt.
struct Source {
    pcm: Vec<f32>,
    talking: Vec<bool>,
}

fn source() -> &'static Source {
    static SOURCE: OnceLock<Source> = OnceLock::new();
    SOURCE.get_or_init(|| {
        let seconds = 60;
        let total = seconds * RATE;
        let mut rng = Rng(0x5eed_1234_abcd_0001);
        let mut pcm = vec![0.0f32; total];
        let mut talking_samples = vec![false; total];
        let mut at = (0.3 * RATE as f64) as usize;
        let mut phases = [0.0f64; 24];
        while at < total {
            let spurt_end = (at + (rng.range(1.2, 3.0) * RATE as f64) as usize).min(total);
            let mut s = at;
            while s < spurt_end {
                // One syllable: its own pitch glide and formants, a raised-cosine envelope.
                let length = (rng.range(0.12, 0.26) * RATE as f64) as usize;
                let end = (s + length).min(spurt_end);
                let (f0_start, f0_end) = (rng.range(105.0, 200.0), rng.range(105.0, 200.0));
                let (f1, f2) = (rng.range(400.0, 850.0), rng.range(1100.0, 2300.0));
                let loudness = rng.range(0.18, 0.32);
                for n in s..end {
                    let t = (n - s) as f64 / (end - s).max(1) as f64;
                    let f0 = f0_start + (f0_end - f0_start) * t;
                    let envelope = (std::f64::consts::PI * t).sin().powf(0.6);
                    let mut sample = 0.0;
                    for (k, phase) in phases.iter_mut().enumerate() {
                        let frequency = f0 * (k + 1) as f64;
                        if frequency > 4000.0 {
                            break;
                        }
                        *phase += std::f64::consts::TAU * frequency / RATE as f64;
                        let resonance =
                            |formant: f64| 1.0 / (1.0 + ((frequency - formant) / 180.0).powi(2));
                        let amplitude = (resonance(f1) + 0.6 * resonance(f2)) / (k + 1) as f64;
                        sample += amplitude * phase.sin();
                    }
                    pcm[n] = (sample * envelope * loudness) as f32;
                    talking_samples[n] = true;
                }
                // A short gap between syllables, still inside the spurt.
                s = end + (rng.range(0.0, 0.05) * RATE as f64) as usize;
                for flag in talking_samples.iter_mut().take(s.min(spurt_end)).skip(end) {
                    *flag = true;
                }
            }
            at = spurt_end + (rng.range(0.4, 1.2) * RATE as f64) as usize;
        }
        let talking = talking_samples
            .chunks(FRAME)
            .map(|frame| frame.iter().any(|t| *t))
            .collect();
        Source { pcm, talking }
    })
}

/// One sent packet.
#[derive(Clone)]
struct Sent {
    sequence: u16,
    timestamp: u32,
    payload: Vec<u8>,
}

/// What a Discord peer sends for the source (`fec: !0, packetLossRate: .3`, module 31595), and the clean decode on the
/// sender's timeline (zeros where nothing was sent).
struct Stream {
    packets: Vec<(usize, Sent)>,
    reference: Vec<f32>,
}

fn stream() -> &'static Stream {
    static STREAM: OnceLock<Stream> = OnceLock::new();
    STREAM.get_or_init(|| {
        let source = source();
        let mut encoder = Encoder::new(48_000, Channels::Mono, Application::Voip).unwrap();
        encoder.set_bitrate(Bitrate::Bits(64_000)).unwrap();
        encoder.set_vbr(true).unwrap();
        encoder.set_complexity(10).unwrap();
        encoder.set_signal(opus2::Signal::Voice).unwrap();
        encoder.set_inband_fec(true).unwrap();
        encoder.set_packet_loss_perc(30).unwrap();
        let mut decoder = Decoder::new(48_000, Channels::Mono).unwrap();
        let mut packets = Vec::new();
        let mut reference = vec![0.0f32; source.pcm.len()];
        let mut sequence = 0u16;
        let mut silence = 0;
        let mut was_talking = false;
        for (k, frame) in source.pcm.as_chunks::<FRAME>().0.iter().enumerate() {
            let talking = source.talking[k];
            if !talking && was_talking {
                silence = 5;
            }
            was_talking = talking;
            let payload = if talking {
                let mut out = [0u8; 1275];
                let n = encoder.encode_float(frame, &mut out).unwrap();
                out[..n].to_vec()
            } else if silence > 0 {
                silence -= 1;
                let mut discard = [0u8; 1275];
                let _ = encoder.encode_float(&[0.0f32; FRAME], &mut discard);
                davey::OPUS_SILENCE_PACKET.to_vec()
            } else {
                continue;
            };
            let mut decoded = [0.0f32; FRAME];
            decoder.decode_float(&payload, &mut decoded, false).unwrap();
            reference[k * FRAME..(k + 1) * FRAME].copy_from_slice(&decoded);
            packets.push((
                k,
                Sent {
                    sequence,
                    timestamp: (k * FRAME) as u32,
                    payload,
                },
            ));
            sequence = sequence.wrapping_add(1);
        }
        Stream { packets, reference }
    })
}

/// A network between the peer and us.
#[derive(Clone, Copy)]
pub(crate) struct Network {
    pub name: &'static str,
    pub delay_ms: f64,
    /// Mean of an exponential extra delay.
    pub jitter_ms: f64,
    /// Gaussian extra delay with packets free to overtake each other (reordering).
    pub reorder_sigma_ms: f64,
    /// Wi-Fi stalls a second: the link holds packets for 100 to 300 ms, then releases them together.
    pub stalls_per_second: f64,
    pub loss: f64,
    /// Gilbert loss: the chance to enter a loss burst, and to stay in it.
    pub burst_enter: f64,
    pub burst_stay: f64,
    /// The sender's clock against ours.
    pub drift_ppm: f64,
    /// Our own receive loop stops for 100 ms this often a second (late ticks).
    pub loop_stalls_per_second: f64,
}

impl Network {
    const fn constant(name: &'static str, delay_ms: f64) -> Self {
        Self {
            name,
            delay_ms,
            jitter_ms: 1.0,
            reorder_sigma_ms: 0.0,
            stalls_per_second: 0.0,
            loss: 0.0,
            burst_enter: 0.0,
            burst_stay: 0.0,
            drift_ppm: 0.0,
            loop_stalls_per_second: 0.0,
        }
    }
}

pub(crate) fn conditions() -> Vec<Network> {
    let c = Network::constant;
    vec![
        c("constant 50 ms", 50.0),
        c("constant 200 ms", 200.0),
        c("constant 477 ms", 477.0),
        c("constant 800 ms", 800.0),
        Network {
            jitter_ms: 20.0,
            ..c("477 ms + jitter 20 ms", 477.0)
        },
        Network {
            jitter_ms: 20.0,
            stalls_per_second: 0.3,
            ..c("477 ms + jitter + Wi-Fi stalls", 477.0)
        },
        Network {
            jitter_ms: 30.0,
            stalls_per_second: 0.8,
            ..c("477 ms hotspot (heavy stalls)", 477.0)
        },
        Network {
            jitter_ms: 5.0,
            loss: 0.01,
            ..c("loss 1%", 100.0)
        },
        Network {
            jitter_ms: 5.0,
            loss: 0.03,
            ..c("loss 3%", 100.0)
        },
        Network {
            jitter_ms: 5.0,
            loss: 0.10,
            ..c("loss 10%", 100.0)
        },
        Network {
            jitter_ms: 5.0,
            burst_enter: 0.02,
            burst_stay: 0.6,
            ..c("bursty loss", 100.0)
        },
        Network {
            reorder_sigma_ms: 15.0,
            ..c("reordering", 100.0)
        },
        Network {
            jitter_ms: 5.0,
            drift_ppm: 200.0,
            ..c("drift +200 ppm", 100.0)
        },
        Network {
            jitter_ms: 5.0,
            drift_ppm: -200.0,
            ..c("drift -200 ppm", 100.0)
        },
        Network {
            jitter_ms: 5.0,
            loop_stalls_per_second: 0.3,
            ..c("late receive ticks", 100.0)
        },
    ]
}

/// When each packet arrives (receiver time, ms), lost ones left out, in arrival order.
fn arrivals(network: &Network, seed: u64) -> Vec<(f64, Sent)> {
    let stream = stream();
    let mut rng = Rng(seed);
    let duration_ms = (source().pcm.len() / 48) as f64;
    // Stall windows: [start, end).
    let mut stalls = Vec::new();
    if network.stalls_per_second > 0.0 {
        let mut t = rng.exponential(1000.0 / network.stalls_per_second);
        while t < duration_ms + network.delay_ms + 1000.0 {
            let length = rng.range(100.0, 300.0);
            stalls.push((t, t + length));
            t += length + rng.exponential(1000.0 / network.stalls_per_second);
        }
    }
    let mut in_burst = false;
    let mut out = Vec::new();
    let mut last_arrival = f64::MIN;
    for (k, packet) in &stream.packets {
        let lost_random = rng.uniform() < network.loss;
        in_burst = if in_burst {
            rng.uniform() < network.burst_stay
        } else {
            rng.uniform() < network.burst_enter
        };
        let send = *k as f64 * 20.0 * (1.0 + network.drift_ppm * 1e-6);
        let mut arrival = send + network.delay_ms + rng.exponential(network.jitter_ms.max(1e-9));
        if network.reorder_sigma_ms > 0.0 {
            arrival += (rng.normal() * network.reorder_sigma_ms).abs();
        } else {
            // A FIFO link: nothing overtakes.
            arrival = arrival.max(last_arrival + 0.05);
        }
        if let Some((_, end)) = stalls
            .iter()
            .find(|(start, end)| arrival >= *start && arrival < *end)
        {
            arrival = arrival.max(*end + 0.3 * rng.uniform());
            if network.reorder_sigma_ms == 0.0 {
                arrival = arrival.max(last_arrival + 0.05);
            }
        }
        last_arrival = last_arrival.max(arrival);
        if lost_random || in_burst {
            continue;
        }
        out.push((arrival, packet.clone()));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// A receive path under test: packets in, 20 ms frames out on our clock.
trait Receiver {
    fn push(&mut self, packet: &Sent);
    /// `None`: nothing was produced (the old path's silent frame).
    fn pop(&mut self) -> Option<[f32; FRAME]>;
    fn counters(&self) -> Counters;
    /// The old path flushes everything when its loop stalled (`stalled` in the session's tick).
    fn loop_stalled(&mut self);
    /// Frames owed after a stall are popped at once (`FrameClock`) by the new path, not by the old.
    fn catches_up(&self) -> bool;
    /// Decoupled hardware-pull playout continues pulling on device cadence across ingest stalls.
    fn decoupled(&self) -> bool {
        false
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct Counters {
    pub concealed_samples: u64,
    pub stretched_samples: u64,
    pub fec_recovered: u64,
    pub late_packets: u64,
    pub flushes: u64,
}

/// The old path, as `mixer.rs` ran it with `jitter.rs` before this bench (one speaker, mono).
struct Legacy {
    jitter: crate::jitter::Jitter,
    decoder: Decoder,
    pcm: Vec<f32>,
    offset: usize,
    length: usize,
    counters: Counters,
}

impl Legacy {
    fn new() -> Self {
        Self {
            jitter: crate::jitter::Jitter::default(),
            decoder: Decoder::new(48_000, Channels::Mono).unwrap(),
            pcm: vec![0.0; 5760],
            offset: 0,
            length: 0,
            counters: Counters::default(),
        }
    }
}

impl Receiver for Legacy {
    fn push(&mut self, packet: &Sent) {
        self.jitter.push(packet.sequence, packet.payload.clone());
    }

    fn pop(&mut self) -> Option<[f32; FRAME]> {
        let mut output = [0.0f32; FRAME];
        let mut filled = 0;
        let mut decoded = 0;
        let mut active = false;
        while filled < FRAME {
            if self.offset == self.length {
                if decoded == 8 {
                    break;
                }
                let Some(opus) = self.jitter.pop() else {
                    break;
                };
                decoded += 1;
                let limit = if opus.is_empty() {
                    self.counters.concealed_samples += 960;
                    self.decoder
                        .get_last_packet_duration()
                        .unwrap_or(960)
                        .clamp(120, 5760) as usize
                } else {
                    self.pcm.len()
                };
                let Ok(length) = self
                    .decoder
                    .decode_float(&opus, &mut self.pcm[..limit], false)
                else {
                    continue;
                };
                self.offset = 0;
                self.length = length;
            }
            let count = (FRAME - filled).min(self.length - self.offset);
            output[filled..filled + count]
                .copy_from_slice(&self.pcm[self.offset..self.offset + count]);
            self.offset += count;
            filled += count;
            active |= count != 0;
        }
        active.then_some(output)
    }

    fn counters(&self) -> Counters {
        self.counters
    }

    fn loop_stalled(&mut self) {
        self.jitter.clear();
        self.offset = 0;
        self.length = 0;
        self.counters.flushes += 1;
    }

    fn catches_up(&self) -> bool {
        false
    }
}

impl Receiver for NetEq {
    fn push(&mut self, packet: &Sent) {
        self.insert_packet(packet.sequence, packet.timestamp, &packet.payload);
    }

    fn pop(&mut self) -> Option<[f32; FRAME]> {
        let mut frame = [0.0f32; FRAME];
        for half in frame.as_chunks_mut::<OUTPUT_SIZE>().0 {
            self.get_audio(half);
        }
        Some(frame)
    }

    fn counters(&self) -> Counters {
        let stats = self.stats();
        Counters {
            concealed_samples: stats.concealed_samples,
            stretched_samples: stats.accelerated_samples + stats.expanded_samples,
            fec_recovered: stats.fec_recovered,
            late_packets: stats.packets_late,
            flushes: stats.flushes,
        }
    }

    fn loop_stalled(&mut self) {}

    fn catches_up(&self) -> bool {
        true
    }
}

struct DecoupledPlayout {
    playout: Arc<CallPlayout>,
    reader: PlayoutReader,
    _tap: rtrb::Consumer<crate::Frame>,
}

impl DecoupledPlayout {
    fn new() -> Self {
        let playout = Arc::new(CallPlayout::new(Arc::new(AtomicU32::new(0))));
        playout.announce(1, 1).expect("speaker slot");
        let (reader, tap) = playout.open_reader();
        Self {
            playout,
            reader,
            _tap: tap,
        }
    }
}

impl Receiver for DecoupledPlayout {
    fn push(&mut self, packet: &Sent) {
        self.playout
            .push_packet(1, packet.sequence, packet.timestamp, &packet.payload);
    }

    fn pop(&mut self) -> Option<[f32; FRAME]> {
        let mut frame = [0.0f32; FRAME];
        let mut stereo_half = [0.0f32; OUTPUT_SIZE * 2];
        for half in frame.as_chunks_mut::<OUTPUT_SIZE>().0 {
            self.reader.pull_10ms(&mut stereo_half, true);
            for (dst, lr) in half.iter_mut().zip(stereo_half.as_chunks::<2>().0) {
                *dst = (lr[0] + lr[1]) * 0.5;
            }
        }
        Some(frame)
    }

    fn counters(&self) -> Counters {
        let snap = self.playout.publish_stats_now();
        snap.voice_speakers
            .first()
            .map_or(Counters::default(), |spk| Counters {
                concealed_samples: spk.stats.concealed_samples,
                stretched_samples: spk.stats.accelerated_samples + spk.stats.expanded_samples,
                fec_recovered: spk.stats.fec_recovered,
                late_packets: spk.stats.packets_late,
                flushes: spk.stats.flushes,
            })
    }

    fn loop_stalled(&mut self) {}

    fn catches_up(&self) -> bool {
        true
    }

    fn decoupled(&self) -> bool {
        true
    }
}

/// What a run measured.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Measured {
    pub concealed_ms: f64,
    pub stretched_ms: f64,
    pub fec_recovered: u64,
    pub late_packets: u64,
    pub flushes: u64,
    /// 20 ms frames played as silence while the aligned source was speaking.
    pub underruns: u64,
    /// Sample jumps larger than anything in the clean decode, counted once per 10 ms.
    pub discontinuities: u64,
    /// Energy-weighted mean of the best normalised correlation per 100 ms speech window, 0 to 1.
    pub similarity: f64,
    /// Latency over the network delay: mean, at the end, and the largest.
    pub added_latency_mean_ms: f64,
    pub added_latency_end_ms: f64,
    pub added_latency_max_ms: f64,
}

fn run(receiver: &mut dyn Receiver, network: &Network, seed: u64) -> Measured {
    let packets = arrivals(network, seed);
    let mut rng = Rng(seed ^ 0x9e37_79b9_7f4a_7c15);
    let duration_ms = (source().pcm.len() / 48) as f64 + network.delay_ms + 1500.0;
    let mut output: Vec<f32> = Vec::with_capacity((duration_ms as usize + 100) * 48);
    let mut next_packet = 0;
    let mut tick = 0u64;
    let mut stall_until = -1.0;
    let mut next_loop_stall = if network.loop_stalls_per_second > 0.0 {
        rng.exponential(1000.0 / network.loop_stalls_per_second)
    } else {
        f64::INFINITY
    };
    let mut owed: usize = 0;
    loop {
        let now = tick as f64 * 20.0;
        if now > duration_ms {
            break;
        }
        tick += 1;
        if now >= next_loop_stall {
            stall_until = now + 100.0;
            next_loop_stall =
                now + 100.0 + rng.exponential(1000.0 / network.loop_stalls_per_second);
        }
        let stalled_now = now < stall_until;
        if stalled_now && !receiver.decoupled() {
            // Legacy coupled loop: packets wait in the socket, no frame is made.
            owed += 1;
            continue;
        }
        if !stalled_now {
            while next_packet < packets.len() && packets[next_packet].0 <= now {
                receiver.push(&packets[next_packet].1);
                next_packet += 1;
            }
        }
        let frames = if !stalled_now && owed > 0 {
            let stalled = owed;
            owed = 0;
            if receiver.catches_up() {
                // `FrameClock::MAX_CATCH_UP`: three at most; the time beyond is gone, and the device played silence.
                output.extend(std::iter::repeat_n(0.0, stalled.saturating_sub(2) * FRAME));
                1 + stalled.min(2)
            } else {
                // The old tick: a stall of 80 ms or more flushes the mixer, and the missed frames were silence.
                if stalled * 20 >= 80 {
                    receiver.loop_stalled();
                }
                output.extend(std::iter::repeat_n(0.0, stalled * FRAME));
                1
            }
        } else {
            1
        };
        for _ in 0..frames {
            match receiver.pop() {
                Some(frame) => output.extend_from_slice(&frame),
                None => output.extend_from_slice(&[0.0; FRAME]),
            }
        }
    }
    measure(&output, receiver.counters(), network)
}

/// Downsampled by six (8 kHz) by averaging, for the alignment search.
fn decimate(signal: &[f32]) -> Vec<f32> {
    signal
        .chunks(6)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect()
}

fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let (mut ab, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y) in a.iter().zip(b) {
        ab += f64::from(*x) * f64::from(*y);
        aa += f64::from(*x) * f64::from(*x);
        bb += f64::from(*y) * f64::from(*y);
    }
    if aa <= 0.0 || bb <= 0.0 {
        0.0
    } else {
        ab / (aa * bb).sqrt()
    }
}

fn measure(output: &[f32], counters: Counters, network: &Network) -> Measured {
    let reference = &stream().reference;
    let talking = &source().talking;
    let threshold = 1.5
        * reference
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
    let mut discontinuities = 0;
    let mut last_click = 0usize;
    for (i, w) in output.windows(2).enumerate() {
        if (w[1] - w[0]).abs() > threshold && (discontinuities == 0 || i - last_click > 480) {
            discontinuities += 1;
            last_click = i;
        }
    }
    // Alignment: 100 ms windows of 8 kHz copies; a full search first, then within 150 ms of the last lag.
    let r8 = decimate(reference);
    let o8 = decimate(output);
    let window = 800;
    let mut lag: Option<isize> = None;
    let mut weighted = 0.0;
    let mut weights = 0.0;
    let mut latencies = Vec::new();
    let mut underruns = 0u64;
    for start in (0..r8.len().saturating_sub(window)).step_by(window) {
        let r = &r8[start..start + window];
        let energy: f64 = r.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
        let frame = start * 6 / FRAME;
        if energy < 1e-3 || !talking.get(frame).copied().unwrap_or(false) {
            continue;
        }
        let (low, high, step) = match lag {
            None => (0isize, ((network.delay_ms + 1000.0) * 8.0) as isize, 2isize),
            Some(l) => (l - 1200, l + 1200, 2),
        };
        let mut best = (f64::MIN, 0isize);
        let mut candidate = low.max(-(start as isize));
        while candidate <= high {
            let at = start as isize + candidate;
            if at >= 0 && (at as usize) + window <= o8.len() {
                let c = correlation(r, &o8[at as usize..at as usize + window]);
                if c > best.0 {
                    best = (c, candidate);
                }
            }
            candidate += step;
        }
        for fine in best.1 - 2..=best.1 + 2 {
            let at = start as isize + fine;
            if at >= 0 && (at as usize) + window <= o8.len() {
                let c = correlation(r, &o8[at as usize..at as usize + window]);
                if c > best.0 {
                    best = (c, fine);
                }
            }
        }
        if best.0 == f64::MIN {
            continue;
        }
        lag = Some(best.1);
        weighted += best.0.max(0.0) * energy;
        weights += energy;
        let latency_ms = best.1 as f64 / 8.0;
        latencies.push(latency_ms - network.delay_ms);
        // Silent 20 ms frames of the output where this window's source speaks.
        let out_start = (start as isize + best.1) * 6;
        for f in 0..5 {
            let at = out_start + (f * FRAME) as isize;
            let source_at = start * 6 + f * FRAME;
            if at < 0 || at as usize + FRAME > output.len() || source_at + FRAME > reference.len() {
                continue;
            }
            let source_rms = (reference[source_at..source_at + FRAME]
                .iter()
                .map(|x| x * x)
                .sum::<f32>()
                / FRAME as f32)
                .sqrt();
            let played = &output[at as usize..at as usize + FRAME];
            if source_rms > 0.02 && played.iter().all(|x| x.abs() < 1e-4) {
                underruns += 1;
            }
        }
    }
    let tail = latencies
        .len()
        .saturating_sub(latencies.len() / 10)
        .min(latencies.len().saturating_sub(1));
    let end = &latencies[tail..];
    Measured {
        concealed_ms: counters.concealed_samples as f64 / 48.0,
        stretched_ms: counters.stretched_samples as f64 / 48.0,
        fec_recovered: counters.fec_recovered,
        late_packets: counters.late_packets,
        flushes: counters.flushes,
        underruns,
        discontinuities,
        similarity: if weights > 0.0 {
            weighted / weights
        } else {
            0.0
        },
        added_latency_mean_ms: latencies.iter().sum::<f64>() / latencies.len().max(1) as f64,
        added_latency_end_ms: end.iter().sum::<f64>() / end.len().max(1) as f64,
        added_latency_max_ms: latencies.iter().copied().fold(f64::MIN, f64::max),
    }
}

fn row(name: &str, path: &str, m: &Measured) -> String {
    format!(
        "| {name} | {path} | {:.0} | {:.0} | {} | {} | {} | {} | {} | {:.3} | {:.0} | {:.0} | {:.0} |",
        m.concealed_ms,
        m.stretched_ms,
        m.fec_recovered,
        m.late_packets,
        m.flushes,
        m.underruns,
        m.discontinuities,
        m.similarity,
        m.added_latency_mean_ms,
        m.added_latency_end_ms,
        m.added_latency_max_ms
    )
}

pub(crate) fn measure_old(network: &Network) -> Measured {
    run(&mut Legacy::new(), network, 7)
}

pub(crate) fn measure_new(network: &Network) -> Measured {
    run(&mut NetEq::new(false).unwrap(), network, 7)
}

pub(crate) fn measure_playout(network: &Network) -> Measured {
    run(&mut DecoupledPlayout::new(), network, 7)
}

/// The whole table, old against new and decoupled CallPlayout. `cargo test -p dc-voice --release bench_table -- --ignored --nocapture`.
#[test]
#[ignore = "prints the before/after table; slow in debug builds"]
fn bench_table() {
    println!(
        "| network | path | concealed ms | stretched ms | FEC | late | flushes | silent frames | clicks | similarity | added latency mean | end | max |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for network in conditions() {
        println!("{}", row(network.name, "old", &measure_old(&network)));
        println!("{}", row(network.name, "NetEq", &measure_new(&network)));
        println!(
            "{}",
            row(network.name, "CallPlayout", &measure_playout(&network))
        );
    }
}

#[test]
fn constant_477_ms_with_jitter_only_adds_latency() {
    let network = Network {
        jitter_ms: 20.0,
        ..Network::constant("477 ms + jitter 20 ms", 477.0)
    };
    let m = measure_new(&network);
    assert_eq!(m.discontinuities, 0, "{m:?}");
    assert_eq!(m.underruns, 0, "{m:?}");
    assert_eq!(m.late_packets, 0, "{m:?}");
    // Nothing was lost: concealment only while the target delay first grows to the jitter (130 ms in 60 s measured).
    assert!(m.concealed_ms <= 200.0, "{m:?}");
    assert!(m.similarity > 0.97, "{m:?}");
    assert!(m.added_latency_end_ms < 300.0, "{m:?}");
}

#[test]
fn stalls_that_release_bursts_are_absorbed() {
    let network = Network {
        jitter_ms: 20.0,
        stalls_per_second: 0.3,
        ..Network::constant("477 ms + jitter + Wi-Fi stalls", 477.0)
    };
    let new = measure_new(&network);
    let old = measure_old(&network);
    assert!(new.similarity > old.similarity, "{new:?} vs {old:?}");
    assert!(
        new.underruns * 4 <= old.underruns.max(1),
        "{new:?} vs {old:?}"
    );
    assert_eq!(new.flushes, 0);
    assert!(new.added_latency_end_ms < 450.0, "{new:?}");
}

#[test]
fn random_loss_is_covered_by_fec_and_concealment_only() {
    let network = Network {
        jitter_ms: 5.0,
        loss: 0.03,
        ..Network::constant("loss 3%", 100.0)
    };
    let m = measure_new(&network);
    assert!(m.fec_recovered > 0, "{m:?}");
    assert_eq!(m.discontinuities, 0, "{m:?}");
    assert!(m.similarity > 0.85, "{m:?}");
}

#[test]
fn sender_drift_is_absorbed_by_stretching() {
    for drift_ppm in [200.0, -200.0] {
        let network = Network {
            jitter_ms: 5.0,
            drift_ppm,
            ..Network::constant("drift", 100.0)
        };
        let m = measure_new(&network);
        assert_eq!(m.discontinuities, 0, "{drift_ppm}: {m:?}");
        assert_eq!(m.underruns, 0, "{drift_ppm}: {m:?}");
        assert!(m.added_latency_max_ms < 250.0, "{drift_ppm}: {m:?}");
    }
}

#[test]
fn late_receive_ticks_has_zero_underruns_with_decoupled_playout() {
    let network = Network {
        jitter_ms: 15.0,
        loop_stalls_per_second: 0.25,
        ..Network::constant("late receive ticks", 50.0)
    };
    let m = measure_playout(&network);
    assert_eq!(m.underruns, 0, "{m:?}");
    assert_eq!(m.discontinuities, 0, "{m:?}");
    assert!(m.similarity > 0.97, "{m:?}");
}
