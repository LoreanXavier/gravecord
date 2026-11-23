//! The clip path of a game without a stream (module 489496 `yd`, the decoupled branch: clips on, a visible game with a
//! `windowHandle`, and a capture source for it). The game's window is captured and encoded by the same worker Go Live
//! uses (`dc_media::screen::Worker`, Windows Graphics Capture and a hardware H.264 encoder) and its pictures go to the
//! clip buffer (`dc_clips::Tap`) instead of a network sender. The game's own sound is the process loopback of its
//! process tree, so nothing else on the machine ends up in a clip.
//!
//! The picture settings are the Clips page's: height (0 = the window's own size), frame rate and the bitrate percent
//! (`setClipsQualitySettings(width, height, fps, bitratePercent)`, module 516130 `applyNativeClipsSettings`).

use dc_clips::{SourceInfo, Tap, TapGuard, Track};
use dc_media::screen::{self, AudioCapture, Resolution, Settings, SourceId, Worker};
use dc_media::synthetic::Synthetic;

/// The window handle of a test game: no window is captured (Windows Graphics Capture reports a zero size for a window
/// on the hidden desktop the harness runs on) and the moving test pattern is encoded in its place, at the size of a
/// 1080p window.
pub const TEST_WINDOW: u64 = 0;
const TEST_WINDOW_SIZE: (u32, u32) = (1920, 1080);

/// A detected game the clip path may capture: `getVisibleGame()` with its `windowHandle`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    pub pid: u32,
    /// The game's main window (an HWND value).
    pub window: u64,
    pub name: String,
    /// The detectable game's application id, when the process matched one.
    pub application_id: Option<String>,
}

/// The encoder's target rate for the page's bitrate percent, in bits per second: `en` of module 31595, which
/// `setClipsSource` hands the clips recorder as `bitrateKbps`: `round(6000 * clamp(percent, 10, 100) / 100)` kb/s, the
/// same at every size and frame rate (4.5 Mb/s at the default 75 %).
pub fn target_bitrate(percent: u32) -> u32 {
    (6_000 * percent.clamp(10, 100) + 50) / 100 * 1_000
}

/// The box `setClipsQualitySettings(width, height, ...)` gets for a page resolution (module 516130
/// `applyNativeClipsSettings`): 4:3 up to 480, 16:9 above. `None` for Source (0): the window's own size.
pub fn quality_box(resolution_height: u32) -> Option<(u32, u32)> {
    match resolution_height {
        0 => None,
        h if h <= 480 => Some((h / 3 * 4, h)),
        h => Some((h / 9 * 16, h)),
    }
}

/// The picture size for a window of `width` x `height` in the page's box: the window's aspect, fitted inside the box,
/// never larger than the window, even numbers. `None` when the window is too small to encode.
pub fn capture_size(width: u32, height: u32, resolution_height: u32) -> Option<(u32, u32)> {
    if width < 2 || height < 2 {
        return None;
    }
    let (box_width, box_height) =
        quality_box(resolution_height).unwrap_or((screen::MAX_WIDTH, screen::MAX_HEIGHT));
    let scale = (f64::from(box_width) / f64::from(width))
        .min(f64::from(box_height) / f64::from(height))
        .min(1.0);
    let out_width = ((f64::from(width) * scale).round() as u32) & !1;
    let out_height = ((f64::from(height) * scale).round() as u32) & !1;
    (out_width >= 2 && out_height >= 2).then_some((out_width, out_height))
}

/// What to capture for a window of `width` x `height` at the page's quality: the size is the page's box (the
/// window's own when the page says source or the window is smaller: nothing is scaled up), the rate the page's.
/// `None` when the window is too small to capture.
pub fn settings_for(
    window: u64,
    width: u32,
    height: u32,
    resolution_height: u32,
    fps: u32,
    percent: u32,
) -> Option<Settings> {
    let (out_width, out_height) = capture_size(width, height, resolution_height)?;
    // The bitrate ladder is keyed by the largest preset not above the picture's height.
    let resolution = Resolution::ALL
        .into_iter()
        .filter(|preset| preset.height() > 0 && preset.height() <= out_height)
        .max_by_key(|preset| preset.height())
        .unwrap_or(Resolution::ALL[0]);
    // Not in the pinned client: ClipsEnhancements' extra rates are kept (`ui/plugin_hooks/clips_enhancements.rs`);
    // the bit rate ladder takes any rate.
    let extra_rate = !crate::plugins::bypassed()
        && crate::plugins::on("ClipsEnhancements")
        && [45, 90, 120, 144, 165, 240].contains(&fps);
    let fps = if extra_rate || screen::required_quality(resolution, fps).is_some() {
        fps
    } else {
        30
    };
    Some(Settings {
        source: SourceId::Window(window),
        width: out_width,
        height: out_height,
        fps,
        bitrate: target_bitrate(percent),
        cursor: false,
        audio: false,
    })
}

/// How long after a game is first detected its window capture starts: `handleClipsInitOnGamesChange` of module
/// 516130 fires the clips init `setTimeout(..., 3e4)` after a `RUNNING_GAMES_CHANGE` that added the visible game, and
/// at once for a game that was already running.
pub const NEW_GAME_DELAY: std::time::Duration = std::time::Duration::from_secs(30);

/// How long ago the visible game was first detected, for [`Situation::game_age`]: from its `first_seen` time in the
/// running list, or from `now` when the list has no time for it.
pub fn game_age(
    first_seen: Option<std::time::Instant>,
    now: std::time::Instant,
) -> std::time::Duration {
    first_seen.map_or(std::time::Duration::ZERO, |seen| {
        now.saturating_duration_since(seen)
    })
}

/// The capture that runs: its game, the quality it started with and why it ended, if it did.
pub type Running<'a> = (&'a Game, (u32, u32, u32), Option<String>);

/// What the clip path knows when it decides whether the game window is captured.
pub struct Situation<'a> {
    /// Enable Clipping is on and the account is listed for it.
    pub enabled: bool,
    /// Our own stream feeds the buffer (one capture serves both).
    pub own_stream: bool,
    /// A storage location was chosen.
    pub location_set: bool,
    pub game: Option<&'a Game>,
    /// How long the game has been the visible one, and the wait a new game gets.
    pub game_age: std::time::Duration,
    pub delay: std::time::Duration,
    /// The page's quality now: height, frame rate, percent.
    pub quality: (u32, u32, u32),
    /// The running capture, if any: its game, the quality it started with and why it ended, if it did.
    pub running: Option<Running<'a>>,
    /// A start is already underway.
    pub starting: bool,
}

/// What to do about the capture.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// Stop the running capture, and why.
    pub stop: Option<String>,
    pub start: bool,
}

/// The decoupled branch of `yd` (module 489496): capture the visible game window while Clipping is on, nothing of
/// ours streams and a game with a window is visible; stop when that no longer holds, the game or the quality changes,
/// or the capture ended by itself.
pub fn plan(s: &Situation) -> Plan {
    let wanted = s.enabled && !s.own_stream && s.location_set && s.game.is_some();
    let mut stop = None;
    let mut running = false;
    if let Some((game, quality, ended)) = &s.running {
        running = true;
        if let Some(why) = ended {
            stop = Some(why.clone());
        } else if s.own_stream {
            stop = Some("a stream took over the clip buffer".to_owned());
        } else if !wanted {
            stop = Some("clipping off, no game or no storage location".to_owned());
        } else if s.game != Some(*game) {
            stop = Some("the game changed".to_owned());
        } else if *quality != s.quality {
            stop = Some("the quality changed".to_owned());
        }
        if stop.is_some() {
            running = false;
        }
    }
    Plan {
        stop,
        start: wanted && !running && !s.starting && s.game_age >= s.delay,
    }
}

enum Source {
    Window(Worker),
    Test(Synthetic),
}

/// A running capture of one game: dropping it stops the capture and frees the buffer.
pub struct GameCapture {
    pub game: Game,
    source: Source,
    _audio: Option<AudioCapture>,
    tap: Tap,
    _guard: TapGuard,
    /// Why the game's sound is not recorded, when it is not.
    pub audio_error: Option<String>,
    /// The size and rate actually captured.
    pub settings: Settings,
}

impl GameCapture {
    /// Starts capturing `game`'s window into the clip buffer. The buffer is announced with the game's name (the clip's
    /// application name) and emptied.
    pub fn start(
        game: Game,
        resolution_height: u32,
        fps: u32,
        percent: u32,
    ) -> Result<Self, String> {
        let (game, (window_width, window_height)) = if game.window == TEST_WINDOW {
            (game, TEST_WINDOW_SIZE)
        } else {
            let mut game = game;
            let direct = screen::window_dimensions(game.window).or_else(|| {
                let fresh = dc_detect::main_window(game.pid)?;
                game.window = fresh;
                screen::window_dimensions(fresh)
            });
            if let Some(dims) = direct {
                (game, dims)
            } else {
                let sources = screen::sources().map_err(|error| error.to_string())?;
                let source = sources
                    .iter()
                    .find(|source| source.id == SourceId::Window(game.window))
                    .or_else(|| {
                        let fresh = dc_detect::main_window(game.pid)?;
                        game.window = fresh;
                        sources
                            .iter()
                            .find(|source| source.id == SourceId::Window(fresh))
                    })
                    .ok_or_else(|| "the game's window is not capturable".to_owned())?;
                (game, (source.width, source.height))
            }
        };
        let settings = settings_for(
            game.window,
            window_width,
            window_height,
            resolution_height,
            fps,
            percent,
        )
        .ok_or_else(|| "the game's window is too small".to_owned())?;
        let recorder = dc_clips::recorder();
        recorder.announce_source(SourceInfo {
            name: game.name.clone(),
            application_id: game.application_id.clone(),
        });
        let tap = recorder.attach();
        let guard = TapGuard(tap.clone());
        let video_tap = tap.clone();
        let source = if game.window == TEST_WINDOW {
            Source::Test(
                Synthetic::start_detailed(
                    settings.width,
                    settings.height,
                    settings.fps,
                    settings.bitrate,
                    move |frame| video_tap.push_video(&frame),
                )
                .map_err(|error| error.to_string())?,
            )
        } else {
            Source::Window(
                Worker::start(settings, move |frame| video_tap.push_video(&frame), |_| {})
                    .map_err(|error| error.to_string())?,
            )
        };
        let sound_tap = tap.clone();
        let (audio, audio_error) = if game.window == TEST_WINDOW {
            (None, Some("a test game has no sound".to_owned()))
        } else {
            match AudioCapture::start_process(game.pid, move |chunk| {
                sound_tap.push_audio(Track::Application, &chunk.samples);
            }) {
                Ok(audio) => (Some(audio), None),
                Err(error) => (None, Some(error.to_string())),
            }
        };
        Ok(Self {
            game,
            source,
            _audio: audio,
            tap,
            _guard: guard,
            audio_error,
            settings,
        })
    }

    /// Why the capture has ended, when it has: the game's window went away, or a stream took the buffer over (its
    /// sender feeds the same clips, so this capture steps aside).
    pub fn ended(&self) -> Option<String> {
        if !self.tap.is_live() {
            return Some("a stream took over the clip buffer".to_owned());
        }
        match &self.source {
            Source::Window(worker) if worker.finished() => Some(
                worker
                    .error()
                    .unwrap_or_else(|| "the capture ended".to_owned()),
            ),
            Source::Test(test) if test.finished() => Some("the test source ended".to_owned()),
            _ => None,
        }
    }

    /// `(captured, encoded)` pictures so far, for the log.
    pub fn counts(&self) -> (u64, u64) {
        match &self.source {
            Source::Window(worker) => (worker.captured(), worker.encoded()),
            Source::Test(test) => (test.produced(), test.produced()),
        }
    }

    /// The encoder behind the worker, for the log.
    pub fn backend(&self) -> String {
        match &self.source {
            Source::Window(worker) => worker
                .backend()
                .map_or_else(|| "none".to_owned(), |backend| format!("{backend:?}")),
            Source::Test(_) => "test pattern (openh264)".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_rate_is_the_official_share_of_six_megabits() {
        // Module 31595 `en`: 6000 kb/s times the percent, held to 10..=100.
        assert_eq!(target_bitrate(100), 6_000_000);
        assert_eq!(target_bitrate(75), 4_500_000);
        assert_eq!(target_bitrate(50), 3_000_000);
        assert_eq!(target_bitrate(25), 1_500_000);
        assert_eq!(target_bitrate(10), 600_000);
        assert_eq!(target_bitrate(0), 600_000);
        assert_eq!(target_bitrate(500), 6_000_000);
        // Math.round of the kb/s: 33 % is 1980 kb/s.
        assert_eq!(target_bitrate(33), 1_980_000);
    }

    #[test]
    fn the_page_box_is_the_official_one() {
        // Module 516130: width = r <= 480 ? r / 3 * 4 : r / 9 * 16.
        assert_eq!(quality_box(480), Some((640, 480)));
        assert_eq!(quality_box(720), Some((1280, 720)));
        assert_eq!(quality_box(1080), Some((1920, 1080)));
        assert_eq!(quality_box(1440), Some((2560, 1440)));
        assert_eq!(quality_box(0), None);
    }

    #[test]
    fn the_size_fits_the_box_and_never_goes_up() {
        assert_eq!(capture_size(2560, 1440, 1080), Some((1920, 1080)));
        // A 16:9 window in the 4:3 box of 480.
        assert_eq!(capture_size(1920, 1080, 480), Some((640, 360)));
        // A window smaller than the box is kept; Source keeps the window, rounded down to even.
        assert_eq!(capture_size(1280, 720, 1080), Some((1280, 720)));
        assert_eq!(capture_size(1281, 721, 0), Some((1280, 720)));
        // A tall window is bounded by the height.
        assert_eq!(capture_size(1000, 2000, 720), Some((360, 720)));
        assert_eq!(capture_size(1, 1, 0), None);
        // Source (0) clamps ultrawide/5K windows within 3840x2160.
        let source_5k = capture_size(5120, 2160, 0).unwrap();
        assert!(source_5k.0 <= 3840 && source_5k.1 <= 2160);
        assert_eq!(source_5k, (3840, 1620));
    }

    #[test]
    fn settings_follow_the_page() {
        let big = settings_for(7, 2560, 1440, 1080, 60, 75).unwrap();
        assert_eq!((big.width, big.height, big.fps), (1920, 1080, 60));
        assert_eq!(big.source, SourceId::Window(7));
        assert!(!big.audio, "the game sound has its own capture");
        assert!(big.validate().is_ok());
        let low = settings_for(7, 2560, 1440, 720, 30, 75).unwrap();
        assert_eq!((low.width, low.height, low.fps), (1280, 720, 30));
        // The rate follows the slider only, as official.
        assert_eq!(low.bitrate, big.bitrate);
        assert_eq!(big.bitrate, 4_500_000);
        let quiet = settings_for(7, 2560, 1440, 720, 30, 10).unwrap();
        assert!(quiet.bitrate < low.bitrate, "the slider lowers the rate");
        assert!(quiet.validate().is_ok());
    }

    #[test]
    fn a_rate_the_ladder_does_not_offer_falls_back_to_thirty() {
        let settings = settings_for(7, 1920, 1080, 1080, 24, 75).unwrap();
        assert_eq!(settings.fps, 30);
        assert!(settings.validate().is_ok());
    }

    fn game(pid: u32) -> Game {
        Game {
            pid,
            window: 5,
            name: "G".into(),
            application_id: None,
        }
    }

    fn base(game: &Game) -> Situation<'_> {
        Situation {
            enabled: true,
            own_stream: false,
            location_set: true,
            game: Some(game),
            game_age: std::time::Duration::from_secs(60),
            delay: NEW_GAME_DELAY,
            quality: (1080, 60, 75),
            running: None,
            starting: false,
        }
    }

    #[test]
    fn a_visible_game_with_clipping_on_starts_a_capture() {
        let g = game(1);
        assert_eq!(
            plan(&base(&g)),
            Plan {
                stop: None,
                start: true
            }
        );
    }

    #[test]
    fn a_new_game_waits_thirty_seconds_from_its_detection() {
        let g = game(1);
        let mut s = base(&g);
        s.game_age = std::time::Duration::from_secs(29);
        assert!(!plan(&s).start);
        s.game_age = std::time::Duration::from_secs(30);
        assert!(plan(&s).start);
        // The age counts from the first detection, not from becoming the visible game.
        let now = std::time::Instant::now();
        let seen = now - std::time::Duration::from_secs(45);
        assert_eq!(
            game_age(Some(seen), now),
            std::time::Duration::from_secs(45)
        );
        assert_eq!(game_age(None, now), std::time::Duration::ZERO);
    }

    #[test]
    fn nothing_starts_without_clipping_a_game_a_location_or_with_a_stream() {
        let g = game(1);
        for edit in 0..4 {
            let mut s = base(&g);
            match edit {
                0 => s.enabled = false,
                1 => s.game = None,
                2 => s.location_set = false,
                _ => s.own_stream = true,
            }
            assert!(!plan(&s).start, "case {edit}");
        }
    }

    #[test]
    fn the_capture_stops_when_the_game_exits_or_clipping_goes_off() {
        let g = game(1);
        let mut s = base(&g);
        s.running = Some((&g, (1080, 60, 75), None));
        assert_eq!(
            plan(&s),
            Plan {
                stop: None,
                start: false
            }
        );
        s.game = None;
        assert!(plan(&s).stop.is_some() && !plan(&s).start);
        let mut s = base(&g);
        s.enabled = false;
        s.running = Some((&g, (1080, 60, 75), None));
        assert!(plan(&s).stop.is_some());
    }

    #[test]
    fn a_stream_takes_the_buffer_and_one_capture_feeds_both() {
        let g = game(1);
        let mut s = base(&g);
        s.own_stream = true;
        s.running = Some((&g, (1080, 60, 75), None));
        let p = plan(&s);
        assert!(
            p.stop.is_some() && !p.start,
            "the stream sender feeds the clips"
        );
    }

    #[test]
    fn a_changed_quality_or_game_restarts_and_an_ended_capture_is_replaced() {
        let g = game(1);
        let other = game(2);
        let mut s = base(&g);
        s.quality = (720, 30, 75);
        s.running = Some((&g, (1080, 60, 75), None));
        assert_eq!(plan(&s).stop.as_deref(), Some("the quality changed"));
        assert!(plan(&s).start, "the new quality starts at once");
        let mut s = base(&other);
        s.running = Some((&g, (1080, 60, 75), None));
        assert_eq!(plan(&s).stop.as_deref(), Some("the game changed"));
        let mut s = base(&g);
        s.running = Some((&g, (1080, 60, 75), Some("window closed".into())));
        let p = plan(&s);
        assert_eq!(p.stop.as_deref(), Some("window closed"));
        assert!(p.start);
        s.starting = true;
        assert!(!plan(&s).start);
    }
}
