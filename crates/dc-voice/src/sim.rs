//! The call simulator's side of this crate (`sim` feature, used by `crates/dc-callsim` and capture builds only).
//!
//! After [`activate`] the whole process behaves as if Discord's voice servers were the local simulator, and nothing
//! else is reachable:
//!
//! * a voice endpoint must be `127.0.0.1:<port>`; it is opened as `ws://` (no TLS). Any other endpoint, a
//!   `*.discord.media` one included, is refused before a socket opens ([`endpoint_url`]).
//! * a voice server may advertise only a loopback media address (the real check refuses loopback).
//! * the call's audio devices are never opened: [`Plan`] says per voice session whether the "microphone" is silence or
//!   a tone, and what the "speaker" plays is measured ([`AudioReport`]) and dropped.
//! * cameras and screen shares draw test pictures (`dc_media::sim`).
//!
//! The switch is process-wide and cannot be undone. Without it, none of this code runs: every hook checks
//! [`active`] first.
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicI64, Ordering},
};
use std::time::Instant;

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Turns the simulation on for this process (and `dc_media::sim` with it).
pub fn activate() {
    dc_media::sim::activate();
    ACTIVE.store(true, Ordering::Release);
}

pub fn active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Endpoints refused while the simulation is on: a count that must stay zero in a simulated run.
static REFUSED: AtomicI64 = AtomicI64::new(0);

/// How many voice endpoints that are not the local simulator were refused so far.
pub fn refused_endpoints() -> i64 {
    REFUSED.load(Ordering::Relaxed)
}

/// `127.0.0.1:<port>` as `ws://127.0.0.1:<port>/?v=9`; anything else is refused and counted.
pub(crate) fn endpoint_url(raw: &str) -> Result<String, &'static str> {
    let port = raw
        .strip_prefix("127.0.0.1:")
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0);
    match port {
        Some(port) => Ok(format!("ws://127.0.0.1:{port}/?v=9")),
        None => {
            REFUSED.fetch_add(1, Ordering::Relaxed);
            Err("Voice endpoint is not the local call simulator")
        }
    }
}

/// What the simulated audio devices of one voice session do.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// The microphone: `None` sends nothing (as a silent microphone behind the voice gate does), `Some(hz)` a sine at
    /// that frequency and [`Plan::level`].
    pub tone_hz: Option<f32>,
    /// Peak level of the tone, 0 to 1.
    pub level: f32,
    /// Frequencies whose level the speaker side measures (for telling several voices and streams apart in one mix).
    pub listen_hz: Vec<f32>,
    /// A simulated share of this session: what it draws, and the tone its stream audio carries (none for silence).
    pub share: Option<dc_media::sim::Content>,
    pub share_tone_hz: Option<f32>,
}

/// What the speaker of one session played, as measured.
#[derive(Clone, Debug, Default)]
pub struct AudioReport {
    /// 20 ms frames handed to the speaker, and how many carried sound (RMS above about -50 dBFS).
    pub frames: u64,
    pub audible: u64,
    /// The longest time between two audible frames once sound had started, ms.
    pub longest_gap_ms: u64,
    /// RMS of the last second of frames, and per listened frequency its amplitude over the last second.
    pub rms: f32,
    pub tones: Vec<(f32, f32)>,
    /// Frames the microphone side produced.
    pub sent: u64,
}

#[derive(Default)]
struct Session {
    plan: Plan,
    report: AudioReport,
    last_audible: Option<Instant>,
    /// Sums of the current second: squares and sample count, then per tone the summed amplitudes and frames.
    window: (f64, u64, Vec<f64>, u64),
    window_start: Option<Instant>,
}

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    SESSIONS.get_or_init(Mutex::default)
}

/// Sets the plan of a voice session (the session id of its `Join` or `StreamJoin`). Takes effect for the devices
/// opened after this; a session without a plan is silent and measured all the same.
pub fn set_plan(session: &str, plan: Plan) {
    let mut all = sessions().lock().unwrap_or_else(|e| e.into_inner());
    let entry = all.entry(session.to_owned()).or_default();
    entry.window.2 = vec![0.0; plan.listen_hz.len()];
    entry.plan = plan;
}

pub fn plan(session: &str) -> Plan {
    sessions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
        .map(|session| session.plan.clone())
        .unwrap_or_default()
}

/// What the speaker of `session` played so far.
pub fn report(session: &str) -> AudioReport {
    sessions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
        .map(|session| session.report.clone())
        .unwrap_or_default()
}

/// Starts the measurement of `session` again (a new scenario).
pub fn reset_report(session: &str) {
    if let Some(entry) = sessions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(session)
    {
        entry.report = AudioReport::default();
        entry.last_audible = None;
        entry.window = (0.0, 0, vec![0.0; entry.plan.listen_hz.len()], 0);
        entry.window_start = None;
    }
}

/// The amplitude of `hz` in a 48 kHz mono frame (Goertzel), as a sine of that peak would give. Exact for multiples of
/// 50 Hz in a 20 ms frame (one bin each); other frequencies leak into their neighbours.
pub fn tone_amplitude(frame: &[f32], hz: f32) -> f32 {
    let n = frame.len() as f32;
    let k = (n * hz / 48_000.0).round();
    let omega = 2.0 * std::f32::consts::PI * k / n;
    let coefficient = 2.0 * omega.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for sample in frame {
        let s0 = sample + coefficient * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - coefficient * s1 * s2;
    2.0 * power.max(0.0).sqrt() / n
}

const AUDIBLE_RMS: f32 = 0.003;

/// One stereo frame the session's speaker played.
pub(crate) fn played(session: &str, frame: &crate::Stereo) {
    let mono = crate::to_mono(frame);
    let rms = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len() as f32).sqrt();
    let now = Instant::now();
    let mut all = sessions().lock().unwrap_or_else(|e| e.into_inner());
    let entry = all.entry(session.to_owned()).or_default();
    entry.report.frames += 1;
    if rms > AUDIBLE_RMS {
        entry.report.audible += 1;
        if let Some(last) = entry.last_audible {
            let gap = now.saturating_duration_since(last).as_millis() as u64;
            entry.report.longest_gap_ms = entry.report.longest_gap_ms.max(gap);
        }
        entry.last_audible = Some(now);
    }
    let listen = entry.plan.listen_hz.clone();
    if entry.window.2.len() != listen.len() {
        entry.window.2 = vec![0.0; listen.len()];
    }
    entry.window.0 += f64::from(rms * rms) * mono.len() as f64;
    entry.window.1 += mono.len() as u64;
    for (sum, hz) in entry.window.2.iter_mut().zip(&listen) {
        *sum += f64::from(tone_amplitude(&mono, *hz));
    }
    entry.window.3 += 1;
    let start = *entry.window_start.get_or_insert(now);
    if now.saturating_duration_since(start).as_millis() >= 1000 {
        let frames = entry.window.3.max(1) as f64;
        entry.report.rms = (entry.window.0 / entry.window.1.max(1) as f64).sqrt() as f32;
        entry.report.tones = listen
            .iter()
            .zip(&entry.window.2)
            .map(|(hz, sum)| (*hz, (*sum / frames) as f32))
            .collect();
        entry.window = (0.0, 0, vec![0.0; listen.len()], 0);
        entry.window_start = Some(now);
    }
}

/// One frame the session's microphone sent.
pub(crate) fn sent(session: &str) {
    if let Some(entry) = sessions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(session)
    {
        entry.report.sent += 1;
    }
}

/// A 20 ms frame of a sine: `phase` carries over from frame to frame.
pub(crate) fn tone(hz: f32, level: f32, phase: &mut f32) -> crate::Frame {
    let mut frame = [0.0; 960];
    for sample in frame.iter_mut() {
        *sample = (*phase * std::f32::consts::TAU).sin() * level;
        *phase = (*phase + hz / 48_000.0).fract();
    }
    frame
}

/// Live voice connections of this process (call, stream) and live simulated devices, for checking that a left call
/// left nothing running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub calls: i64,
    pub streams: i64,
    pub audio_workers: i64,
}

static CALLS: AtomicI64 = AtomicI64::new(0);
static STREAMS: AtomicI64 = AtomicI64::new(0);
static AUDIO: AtomicI64 = AtomicI64::new(0);

pub fn counts() -> Counts {
    Counts {
        calls: CALLS.load(Ordering::Relaxed),
        streams: STREAMS.load(Ordering::Relaxed),
        audio_workers: AUDIO.load(Ordering::Relaxed),
    }
}

/// Held while one connection loop or audio worker runs.
pub(crate) struct Live(&'static AtomicI64);

impl Live {
    pub(crate) fn call() -> Option<Self> {
        active().then(|| Self::count(&CALLS))
    }
    pub(crate) fn stream() -> Option<Self> {
        active().then(|| Self::count(&STREAMS))
    }
    pub(crate) fn audio() -> Self {
        Self::count(&AUDIO)
    }
    fn count(counter: &'static AtomicI64) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The receiving end of transport-wide feedback, as this crate's receivers run it: the simulated server uses it to
/// answer a sender the way a WebRTC receiver would (`transport_cc::TransportFeedback`, a report every 100 ms).
pub struct TransportFeedback(crate::transport_cc::TransportFeedback);

impl TransportFeedback {
    pub fn new(started: Instant) -> Self {
        Self(crate::transport_cc::TransportFeedback::new(started))
    }
    pub fn receive(&mut self, ssrc: u32, sequence: u16, now: Instant) {
        self.0.receive(ssrc, sequence, now);
    }
    /// Due feedback messages as (clear RTCP header, body) pairs.
    pub fn due(&mut self, sender: u32, now: Instant) -> Vec<([u8; 8], Vec<u8>)> {
        self.0.due(sender, now)
    }
}

/// Keyframe request, NACKed sequences, REMB bitrate, and per transport-wide report its base and arrivals.
pub type Parsed = (bool, Vec<u16>, Option<u32>, Vec<(u16, Vec<Option<i64>>)>);

/// What this crate's parser reads from a decrypted compound RTCP packet sent to the sender of `video_ssrc`: keyframe
/// request, NACKed sequences, REMB bitrate, and per transport-wide report the base and the arrivals. For checking that
/// the simulator's feedback is what the client understands.
pub fn parse_feedback(compound: &[u8], video_ssrc: u32) -> Option<Parsed> {
    let feedback = crate::rtp::feedback(compound, video_ssrc)?;
    Some((
        feedback.keyframe,
        feedback.nacks,
        feedback.bitrate,
        feedback
            .transport
            .into_iter()
            .map(|report| (report.base, report.arrivals))
            .collect(),
    ))
}

/// A picture as the hub hands it to the app's sink: who, which source, the size. For measuring delivery without
/// touching the sink the app registered.
pub type Tap = Arc<dyn Fn(u64, crate::VideoSource, u32, u32) + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_local_simulator_is_an_endpoint() {
        assert_eq!(
            endpoint_url("127.0.0.1:50123").unwrap(),
            "ws://127.0.0.1:50123/?v=9"
        );
        let before = refused_endpoints();
        for bad in [
            "c-ams01-abcd.discord.media:443",
            "127.0.0.1",
            "127.0.0.1:0",
            "localhost:50123",
            "10.0.0.1:50123",
            "127.0.0.1:50123/x",
        ] {
            assert!(endpoint_url(bad).is_err(), "{bad}");
        }
        assert_eq!(refused_endpoints() - before, 6);
    }

    #[test]
    fn tones_are_measured_apart_in_one_mix() {
        let (mut a, mut b) = (0.0, 0.0);
        // Multiples of 50 Hz fall on a bin of a 20 ms frame.
        let low = tone(450.0, 0.5, &mut a);
        let high = tone(1050.0, 0.25, &mut b);
        let mix: Vec<f32> = low.iter().zip(high).map(|(x, y)| x + y).collect();
        assert!((tone_amplitude(&mix, 450.0) - 0.5).abs() < 0.03);
        assert!((tone_amplitude(&mix, 1050.0) - 0.25).abs() < 0.03);
        assert!(tone_amplitude(&mix, 750.0) < 0.03);
    }
}
