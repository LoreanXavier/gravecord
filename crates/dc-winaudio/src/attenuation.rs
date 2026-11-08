//! Global attenuation: while someone speaks, other applications' audio is lowered by a percent, and put back exactly
//! when the speaking stops, the call ends, or the app quits (`setAttenuation`, module 31595: the engine turns it on
//! when the factor `(100 - percent) / 100` is below 1, and applies it while `attenuateWhileSpeakingSelf` /
//! `attenuateWhileSpeakingOthers` and the matching side speaks).
//!
//! The decisions and the bookkeeping here are plain Rust over a [`Backend`], so they are tested with a fake one. The
//! Windows backend (`wasapi.rs`) reads and sets each other application's session volume through
//! `IAudioSessionManager2` / `ISimpleAudioVolume`.
//!
//! Safety nets, because another application's volume is not ours to leave changed:
//! - a session whose volume somebody else changed while we held it keeps their new value as its baseline;
//! - every change is written to a journal file before it is made, and [`recover`] puts the sessions of a run that was
//!   killed back at startup (only those still at the level we set).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// One audio session of another application.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// The session's instance identifier (unique per device and process).
    pub id: String,
    /// Its master volume, 0 to 1.
    pub volume: f32,
}

/// Where sessions come from and go to.
pub trait Backend {
    /// Every other application's session on every active playback device. Never this process's own.
    fn sessions(&mut self) -> Result<Vec<Session>, String>;
    fn set_volume(&mut self, id: &str, volume: f32) -> Result<(), String>;
}

impl Backend for Box<dyn Backend> {
    fn sessions(&mut self) -> Result<Vec<Session>, String> {
        (**self).sessions()
    }
    fn set_volume(&mut self, id: &str, volume: f32) -> Result<(), String> {
        (**self).set_volume(id, volume)
    }
}

/// How far a read volume may sit from the one we set before it counts as changed by somebody else.
const CHANGED: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Ducked {
    /// The volume before we touched it, restored bit for bit.
    original: f32,
    /// The volume we set.
    applied: f32,
}

/// The sessions we have lowered, and what to put back.
pub struct Attenuator<B: Backend> {
    backend: B,
    ducked: HashMap<String, Ducked>,
    journal: Option<PathBuf>,
}

impl<B: Backend> Attenuator<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            ducked: HashMap::new(),
            journal: None,
        }
    }

    /// Where the held sessions are written before they are changed and removed once they are put back.
    pub fn set_journal(&mut self, path: Option<PathBuf>) {
        if self.journal != path {
            self.journal = path;
            self.write_journal();
        }
    }

    fn write_journal(&self) {
        let Some(path) = &self.journal else {
            return;
        };
        let held = self.held();
        if held.is_empty() {
            let _ = std::fs::remove_file(path);
            return;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, encode_journal(&held));
    }

    pub fn is_ducked(&self) -> bool {
        !self.ducked.is_empty()
    }

    /// `(id, original, applied)` of every session held down, for the journal.
    pub fn held(&self) -> Vec<(String, f32, f32)> {
        let mut held: Vec<_> = self
            .ducked
            .iter()
            .map(|(id, ducked)| (id.clone(), ducked.original, ducked.applied))
            .collect();
        held.sort_by(|a, b| a.0.cmp(&b.0));
        held
    }

    /// Lowers every other session to `factor` (0 to 1) times its own volume, new sessions included. Called again
    /// while the attenuation lasts, it follows new sessions and a changed factor.
    pub fn attenuate(&mut self, factor: f32) {
        let factor = factor.clamp(0., 1.);
        let sessions = match self.backend.sessions() {
            Ok(sessions) => sessions,
            Err(error) => {
                tracing::warn!("audio sessions unavailable: {error}");
                return;
            }
        };
        let present: HashSet<&str> = sessions.iter().map(|session| session.id.as_str()).collect();
        let mut changed = self.ducked.len();
        // A session that closed has nothing to put back.
        self.ducked.retain(|id, _| present.contains(id.as_str()));
        changed -= self.ducked.len();
        // First decide, and write the journal, then touch the volumes: a kill in between still leaves the journal.
        let mut sets: Vec<(&str, f32)> = Vec::new();
        for session in &sessions {
            let tracked = self.ducked.get(&session.id);
            if tracked.is_none() && factor >= 1. {
                continue;
            }
            let original = match tracked {
                Some(ducked) if (session.volume - ducked.applied).abs() <= CHANGED => {
                    ducked.original
                }
                // Somebody else moved it while it was down: their value is the baseline now.
                Some(_) | None => session.volume,
            };
            let applied = original * factor;
            if tracked != Some(&Ducked { original, applied }) {
                changed += 1;
            }
            self.ducked
                .insert(session.id.clone(), Ducked { original, applied });
            if (session.volume - applied).abs() > f32::EPSILON {
                sets.push((&session.id, applied));
            }
        }
        if changed > 0 {
            self.write_journal();
        }
        let mut failed = false;
        for (id, applied) in sets {
            if let Err(error) = self.backend.set_volume(id, applied) {
                tracing::warn!("audio session volume not set: {error}");
                self.ducked.remove(id);
                failed = true;
            }
        }
        if failed {
            self.write_journal();
        }
    }

    /// Puts every lowered session back to its original volume, unless somebody else has moved it since. True when
    /// nothing is left to restore; false when the sessions could not be read (it is tried again).
    pub fn restore(&mut self) -> bool {
        if self.ducked.is_empty() {
            return true;
        }
        let sessions = match self.backend.sessions() {
            Ok(sessions) => sessions,
            Err(error) => {
                tracing::warn!("audio sessions unavailable to restore: {error}");
                return false;
            }
        };
        let current: HashMap<&str, f32> = sessions
            .iter()
            .map(|session| (session.id.as_str(), session.volume))
            .collect();
        let mut failed = HashMap::new();
        for (id, ducked) in std::mem::take(&mut self.ducked) {
            let Some(now) = current.get(id.as_str()) else {
                continue;
            };
            if (now - ducked.applied).abs() > CHANGED {
                continue;
            }
            if self.backend.set_volume(&id, ducked.original).is_err() {
                failed.insert(id, ducked);
            }
        }
        self.ducked = failed;
        self.write_journal();
        self.ducked.is_empty()
    }
}

impl<B: Backend> Drop for Attenuator<B> {
    /// Whatever ends this (a panic included), other applications get their volumes back.
    fn drop(&mut self) {
        for _ in 0..3 {
            if self.restore() {
                break;
            }
        }
    }
}

/// The user's choices: the strength and when it applies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// 0 to 100. 0 disables attenuation.
    pub percent: u8,
    /// `attenuateWhileSpeakingSelf`.
    pub on_self: bool,
    /// `attenuateWhileSpeakingOthers`.
    pub on_others: bool,
}

/// What the attenuation should be right now: `Some(factor)` to hold other applications at that fraction of their
/// volume, `None` to leave them alone (`isAttenuating()` is `factor < 1`).
pub fn target(
    settings: Settings,
    in_call: bool,
    self_speaking: bool,
    others_speaking: bool,
) -> Option<f32> {
    let factor = f32::from(100 - settings.percent.min(100)) / 100.;
    if !in_call || factor >= 1. {
        return None;
    }
    let speaking = (settings.on_self && self_speaking) || (settings.on_others && others_speaking);
    speaking.then_some(factor)
}

/// A journal line per held session: original and applied volume as exact bits, then the session id.
pub fn encode_journal(held: &[(String, f32, f32)]) -> String {
    held.iter()
        .map(|(id, original, applied)| {
            format!(
                "{:08x}\t{:08x}\t{id}\n",
                original.to_bits(),
                applied.to_bits()
            )
        })
        .collect()
}

pub fn decode_journal(text: &str) -> Vec<(String, f32, f32)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let original = f32::from_bits(u32::from_str_radix(parts.next()?, 16).ok()?);
            let applied = f32::from_bits(u32::from_str_radix(parts.next()?, 16).ok()?);
            let id = parts.next()?.to_owned();
            Some((id, original, applied))
        })
        .collect()
}

/// Puts back what a run that never got to restore left held down: each journalled session that is still at the
/// level we set goes back to its original. Then the journal is removed.
pub fn recover(backend: &mut impl Backend, journal: &std::path::Path) -> usize {
    let Ok(text) = std::fs::read_to_string(journal) else {
        return 0;
    };
    let held = decode_journal(&text);
    let mut restored = 0;
    if let Ok(sessions) = backend.sessions() {
        for (id, original, applied) in held {
            let Some(session) = sessions.iter().find(|session| session.id == id) else {
                continue;
            };
            if (session.volume - applied).abs() <= CHANGED
                && backend.set_volume(&id, original).is_ok()
            {
                restored += 1;
            }
        }
        let _ = std::fs::remove_file(journal);
    }
    restored
}

struct Live {
    settings: Settings,
    in_call: bool,
    self_speaking: bool,
    others_speaking: bool,
    journal: Option<PathBuf>,
    quit: bool,
    /// Bumped by every change, so the thread never sleeps through one that came while it was busy.
    version: u64,
}

struct Shared {
    live: Mutex<Live>,
    wake: Condvar,
}

type Factory = Box<dyn FnOnce() -> Box<dyn Backend> + Send>;

/// The running attenuation: a thread that holds other applications down while the state says so, and puts them back.
pub struct Controller {
    shared: Arc<Shared>,
    factory: Mutex<Option<Factory>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// How often the thread looks at the sessions while it holds them down (new ones appear, volumes get moved).
const POLL: Duration = Duration::from_millis(250);
/// How often it looks while idle, for a state change that missed its wake.
const IDLE: Duration = Duration::from_secs(1);

impl Controller {
    /// `factory` builds the backend on the thread that uses it (COM objects stay on their thread). The thread starts
    /// the first time a call begins.
    pub fn new(factory: impl FnOnce() -> Box<dyn Backend> + Send + 'static) -> Self {
        Self {
            shared: Arc::new(Shared {
                live: Mutex::new(Live {
                    settings: Settings::default(),
                    in_call: false,
                    self_speaking: false,
                    others_speaking: false,
                    journal: None,
                    quit: false,
                    version: 0,
                }),
                wake: Condvar::new(),
            }),
            factory: Mutex::new(Some(Box::new(factory))),
            thread: Mutex::new(None),
        }
    }

    fn update(&self, change: impl FnOnce(&mut Live)) {
        if let Ok(mut live) = self.shared.live.lock() {
            change(&mut live);
            live.version += 1;
        }
        self.shared.wake.notify_all();
    }

    /// Where the journal of held sessions is kept.
    pub fn set_journal(&self, path: Option<PathBuf>) {
        self.update(|live| live.journal = path);
    }

    /// `setAttenuation(percent, self, others)`.
    pub fn configure(&self, settings: Settings) {
        self.update(|live| live.settings = settings);
    }

    /// A call is up (or over: everything is restored).
    pub fn set_in_call(&self, in_call: bool) {
        if in_call {
            self.start();
        }
        self.update(|live| {
            live.in_call = in_call;
            if !in_call {
                live.self_speaking = false;
                live.others_speaking = false;
            }
        });
    }

    /// Who speaks now: this client, and anyone else.
    pub fn set_speaking(&self, self_speaking: bool, others_speaking: bool) {
        self.update(|live| {
            live.self_speaking = self_speaking;
            live.others_speaking = others_speaking;
        });
    }

    fn start(&self) {
        let Ok(mut thread) = self.thread.lock() else {
            return;
        };
        if thread.is_some() {
            return;
        }
        let Some(factory) = self.factory.lock().ok().and_then(|mut slot| slot.take()) else {
            return;
        };
        let shared = self.shared.clone();
        *thread = std::thread::Builder::new()
            .name("dc-attenuation".into())
            .spawn(move || run(&shared, factory))
            .ok();
    }

    /// Restores everything and stops the thread. After this the controller does nothing.
    pub fn shutdown(&self) {
        self.update(|live| live.quit = true);
        let handle = self.thread.lock().ok().and_then(|mut thread| thread.take());
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(shared: &Shared, factory: Factory) {
    let mut attenuator = Attenuator::new(factory());
    loop {
        let (wanted, journal, quit, seen) = {
            let Ok(live) = shared.live.lock() else {
                break;
            };
            (
                target(
                    live.settings,
                    live.in_call,
                    live.self_speaking,
                    live.others_speaking,
                ),
                live.journal.clone(),
                live.quit,
                live.version,
            )
        };
        attenuator.set_journal(journal);
        if quit {
            for _ in 0..3 {
                if attenuator.restore() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            break;
        }
        match wanted {
            Some(factor) => attenuator.attenuate(factor),
            None => {
                attenuator.restore();
            }
        }
        let wait = if wanted.is_some() || attenuator.is_ducked() {
            POLL
        } else {
            IDLE
        };
        let Ok(live) = shared.live.lock() else {
            break;
        };
        if live.version == seen {
            let _ = shared.wake.wait_timeout(live, wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Other applications' sessions, shared with the test so it can look and move them.
    #[derive(Clone, Default)]
    struct Fake {
        sessions: Arc<Mutex<Vec<Session>>>,
        sets: Arc<AtomicUsize>,
    }

    impl Fake {
        fn with(volumes: &[(&str, f32)]) -> Self {
            let fake = Self::default();
            *fake.sessions.lock().unwrap() = volumes
                .iter()
                .map(|(id, volume)| Session {
                    id: (*id).into(),
                    volume: *volume,
                })
                .collect();
            fake
        }
        fn volume(&self, id: &str) -> f32 {
            self.sessions
                .lock()
                .unwrap()
                .iter()
                .find(|session| session.id == id)
                .unwrap()
                .volume
        }
        fn set(&self, id: &str, volume: f32) {
            for session in self.sessions.lock().unwrap().iter_mut() {
                if session.id == id {
                    session.volume = volume;
                }
            }
        }
    }

    impl Backend for Fake {
        fn sessions(&mut self) -> Result<Vec<Session>, String> {
            Ok(self.sessions.lock().unwrap().clone())
        }
        fn set_volume(&mut self, id: &str, volume: f32) -> Result<(), String> {
            self.sets.fetch_add(1, Ordering::SeqCst);
            self.set(id, volume);
            Ok(())
        }
    }

    #[test]
    fn the_target_follows_the_strength_the_call_and_who_speaks() {
        let settings = Settings {
            percent: 40,
            on_self: false,
            on_others: true,
        };
        assert_eq!(target(settings, true, false, true), Some(0.6));
        assert_eq!(
            target(settings, true, true, false),
            None,
            "I do not trigger it"
        );
        assert_eq!(target(settings, false, false, true), None, "no call");
        assert_eq!(target(settings, true, false, false), None, "nobody speaks");
        let both = Settings {
            on_self: true,
            ..settings
        };
        assert_eq!(target(both, true, true, false), Some(0.6));
        let off = Settings { percent: 0, ..both };
        assert_eq!(target(off, true, true, true), None, "0% disables it");
        let silence = Settings {
            percent: 100,
            ..both
        };
        assert_eq!(target(silence, true, true, true), Some(0.0), "100% mutes");
    }

    #[test]
    fn attenuating_then_restoring_gives_back_the_exact_volumes() {
        let fake = Fake::with(&[("music", 0.73), ("game", 1.0), ("zero", 0.0)]);
        let mut attenuator = Attenuator::new(fake.clone());
        attenuator.attenuate(0.5);
        assert_eq!(fake.volume("music"), 0.73 * 0.5);
        assert_eq!(fake.volume("game"), 0.5);
        assert_eq!(fake.volume("zero"), 0.0);
        assert!(attenuator.is_ducked());
        assert!(attenuator.restore());
        assert_eq!(fake.volume("music").to_bits(), 0.73f32.to_bits());
        assert_eq!(fake.volume("game").to_bits(), 1.0f32.to_bits());
        assert_eq!(fake.volume("zero").to_bits(), 0.0f32.to_bits());
        assert!(!attenuator.is_ducked());
    }

    #[test]
    fn a_session_that_appears_while_down_is_lowered_and_restored_too() {
        let fake = Fake::with(&[("music", 0.8)]);
        let mut attenuator = Attenuator::new(fake.clone());
        attenuator.attenuate(0.25);
        fake.sessions.lock().unwrap().push(Session {
            id: "browser".into(),
            volume: 0.6,
        });
        attenuator.attenuate(0.25);
        assert_eq!(fake.volume("browser"), 0.6 * 0.25);
        // Asking again changes nothing for the ones already down.
        let sets = fake.sets.load(Ordering::SeqCst);
        attenuator.attenuate(0.25);
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets);
        assert!(attenuator.restore());
        assert_eq!(fake.volume("browser").to_bits(), 0.6f32.to_bits());
        assert_eq!(fake.volume("music").to_bits(), 0.8f32.to_bits());
    }

    #[test]
    fn a_changed_strength_is_applied_from_the_original_not_the_lowered_volume() {
        let fake = Fake::with(&[("music", 1.0)]);
        let mut attenuator = Attenuator::new(fake.clone());
        attenuator.attenuate(0.5);
        attenuator.attenuate(0.2);
        assert!((fake.volume("music") - 0.2).abs() < 1e-6);
        assert!(attenuator.restore());
        assert_eq!(fake.volume("music").to_bits(), 1.0f32.to_bits());
    }

    #[test]
    fn a_volume_somebody_else_moved_is_theirs() {
        let fake = Fake::with(&[("music", 1.0), ("game", 1.0)]);
        let mut attenuator = Attenuator::new(fake.clone());
        attenuator.attenuate(0.5);
        // The user drags the music slider to 0.9 while it is down; the game is left alone.
        fake.set("music", 0.9);
        attenuator.attenuate(0.5);
        assert!(
            (fake.volume("music") - 0.45).abs() < 1e-6,
            "kept at the factor of their value"
        );
        // They move it again right before the restore: it is not ours to put back.
        fake.set("music", 0.3);
        assert!(attenuator.restore());
        assert_eq!(fake.volume("music"), 0.3);
        assert_eq!(fake.volume("game").to_bits(), 1.0f32.to_bits());
    }

    #[test]
    fn a_session_that_closed_needs_nothing() {
        let fake = Fake::with(&[("music", 0.5), ("game", 1.0)]);
        let mut attenuator = Attenuator::new(fake.clone());
        attenuator.attenuate(0.5);
        fake.sessions
            .lock()
            .unwrap()
            .retain(|session| session.id != "game");
        assert!(attenuator.restore());
        assert_eq!(fake.volume("music").to_bits(), 0.5f32.to_bits());
    }

    #[test]
    fn no_attenuation_leaves_every_session_alone() {
        let fake = Fake::with(&[("music", 0.5)]);
        let mut attenuator = Attenuator::new(fake.clone());
        attenuator.attenuate(1.0);
        assert_eq!(fake.sets.load(Ordering::SeqCst), 0);
        assert!(!attenuator.is_ducked());
    }

    #[test]
    fn the_journal_round_trips_exactly_and_recovers_a_killed_run() {
        let fake = Fake::with(&[("a\tb", 0.37), ("c", 0.9)]);
        let mut attenuator = Attenuator::new(fake.clone());
        attenuator.attenuate(0.5);
        let text = encode_journal(&attenuator.held());
        let decoded = decode_journal(&text);
        assert_eq!(decoded, attenuator.held().into_iter().collect::<Vec<_>>());
        let path =
            std::env::temp_dir().join(format!("dc-winaudio-journal-{}.txt", std::process::id()));
        std::fs::write(&path, &text).unwrap();
        // The run dies here. A new run recovers: both are still at the level we set.
        let mut fresh = fake.clone();
        assert_eq!(recover(&mut fresh, &path), 2);
        assert_eq!(fake.volume("a\tb").to_bits(), 0.37f32.to_bits());
        assert_eq!(fake.volume("c").to_bits(), 0.9f32.to_bits());
        assert!(!path.exists());
        // One the user has moved since is left as it is.
        let mut attenuator = Attenuator::new(fake.clone());
        attenuator.attenuate(0.5);
        std::fs::write(&path, encode_journal(&attenuator.held())).unwrap();
        fake.set("c", 0.2);
        assert_eq!(recover(&mut fresh, &path), 1);
        assert_eq!(fake.volume("c"), 0.2);
    }

    fn wait_for(what: &str, check: impl Fn() -> bool) {
        for _ in 0..200 {
            if check() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out waiting for {what}");
    }

    #[test]
    fn the_controller_holds_while_speaking_and_restores_when_it_stops_or_the_call_ends() {
        let fake = Fake::with(&[("music", 0.8)]);
        let backend = fake.clone();
        let controller = Controller::new(move || Box::new(backend) as Box<dyn Backend>);
        let journal =
            std::env::temp_dir().join(format!("dc-winaudio-run-{}.txt", std::process::id()));
        controller.set_journal(Some(journal.clone()));
        controller.configure(Settings {
            percent: 50,
            on_self: false,
            on_others: true,
        });
        controller.set_in_call(true);
        controller.set_speaking(false, true);
        wait_for("the music to go down", || fake.volume("music") == 0.4);
        assert!(journal.exists(), "the journal names what is held");
        controller.set_speaking(false, false);
        wait_for("the music to come back", || {
            fake.volume("music").to_bits() == 0.8f32.to_bits()
        });
        wait_for("the journal to go", || !journal.exists());
        // Speaking again, then the call ends mid-speech.
        controller.set_speaking(false, true);
        wait_for("the music to go down again", || fake.volume("music") == 0.4);
        controller.set_in_call(false);
        wait_for("the music to come back after the call", || {
            fake.volume("music").to_bits() == 0.8f32.to_bits()
        });
        // Shutting down while held restores too.
        controller.set_in_call(true);
        controller.set_speaking(false, true);
        wait_for("the music to go down a third time", || {
            fake.volume("music") == 0.4
        });
        controller.shutdown();
        assert_eq!(fake.volume("music").to_bits(), 0.8f32.to_bits());
        assert!(!journal.exists());
    }

    #[test]
    fn dropping_the_controller_restores() {
        let fake = Fake::with(&[("music", 0.6)]);
        let backend = fake.clone();
        {
            let controller = Controller::new(move || Box::new(backend) as Box<dyn Backend>);
            controller.configure(Settings {
                percent: 100,
                on_self: true,
                on_others: true,
            });
            controller.set_in_call(true);
            controller.set_speaking(true, false);
            wait_for("silence", || fake.volume("music") == 0.0);
        }
        assert_eq!(fake.volume("music").to_bits(), 0.6f32.to_bits());
    }
}
