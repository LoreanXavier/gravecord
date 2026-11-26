//! The voice message player: the pill drawn in place of a file card for a message with the `IS_VOICE_MESSAGE` flag.
//!
//! Sources, all in the pinned build: the pill is `tl` in module 383055 with the stylesheet `967062.….css`
//! (`container_seLrDE`, `playButtonContainer_seLrDE`, `playbackRate_seLrDE`, `volumeButton*`, `duration_seLrDE`,
//! `rippleContainer_seLrDE`); the waveform canvas is module 408822 (`b`), its bar animation module 539451 and its
//! averaging module 934581; the volume control is module 20504. The values and the pure functions are in
//! `dc_voice::voice_wave`; the sound is `dc_voice::voice_player`.

use super::audio_menu::{slider_to_volume, volume_to_slider};
use super::{Root, color, icon, motion};
use crate::parity::Parity;
use dc_state::{Attachment, Message};
use dc_voice::voice_player::{self, Clip};
use dc_voice::voice_wave::{self, Segment};
use gpui::{
    AnyElement, Bounds, DispatchPhase, FontFeatures, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Rgba, canvas, div, fill, point,
    prelude::*, px, size,
};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use tracing::info;

/// A clip fetched for a message.
#[derive(Clone)]
pub(super) enum Load {
    Loading,
    Ready(Arc<Clip>),
    Failed,
}

pub(super) struct Drag {
    /// Playing when the drag began: it carries on from the new place on release (module 383055 pauses while dragging).
    resume: bool,
}

#[derive(Default)]
pub(super) struct Pill {
    /// Played once: the unplayed look (every bar lit) is over until the pill resets at the end.
    pub(super) played: bool,
    /// Seconds into the message while it is not the one playing (`MEDIA_PLAYBACK_POSITION_UPDATE`).
    pub(super) cached: f64,
    pub(super) want_play: bool,
    segments: Vec<Segment>,
    pub(super) drag: Option<Drag>,
    pub(super) wave: Bounds<Pixels>,
    pub(super) track: Bounds<Pixels>,
    /// The playback reached its end at this motion time; the pill resets 500 ms later.
    pub(super) ended_at: Option<f64>,
    pub(super) over: u8,
    pub(super) hide_at: Option<f64>,
    /// An audio attachment card: the pointer is over it (the metadata loads the first time), over its seek bar, and
    /// the share of the bar the pointer was last at (the preview segment keeps its width when the pointer leaves).
    pub(super) bar_over: bool,
    pub(super) preview: f32,
    pub(super) bar: Bounds<Pixels>,
    /// An audio attachment card: the motion time the length became known (the buffered part of the rail follows).
    pub(super) ready_at: Option<f64>,
}

/// `MediaPlayerVolume` (module 587481): `{volume, muted}` in the client storage; a number is clamped to 0..1 and
/// anything else reads as 1, a non-boolean `muted` as false.
pub(super) fn volume_of(value: Option<&serde_json::Value>) -> (f32, bool) {
    let volume = value
        .and_then(|stored| stored["volume"].as_f64())
        .map_or(1., |volume| volume.clamp(0., 1.)) as f32;
    let muted = value
        .and_then(|stored| stored["muted"].as_bool())
        .unwrap_or(false);
    (volume, muted)
}

fn shared_volume_cell() -> &'static Mutex<(f32, bool, f32)> {
    static SHARED_MEDIA_VOLUME: OnceLock<Mutex<(f32, bool, f32)>> = OnceLock::new();
    SHARED_MEDIA_VOLUME.get_or_init(|| {
        let (volume, muted) = volume_of(crate::storage::get("MediaPlayerVolume").as_ref());
        let last_nonzero = if volume > 0. { volume } else { 1. };
        Mutex::new((volume, muted, last_nonzero))
    })
}

pub(super) fn stored_volume() -> (f32, bool) {
    shared_volume_cell()
        .lock()
        .map(|state| (state.0, state.1))
        .unwrap_or((1., false))
}

pub(super) fn unmute_restore_volume() -> f32 {
    shared_volume_cell()
        .lock()
        .map(|state| if state.2 > 0. { state.2 } else { 1. })
        .unwrap_or(1.)
}

pub(super) fn store_volume(volume: f32, muted: bool) {
    let volume = volume.clamp(0., 1.);
    if let Ok(mut state) = shared_volume_cell().lock() {
        state.0 = volume;
        state.1 = muted;
        if volume > 0. {
            state.2 = volume;
        }
    }
    crate::storage::set(
        "MediaPlayerVolume",
        serde_json::json!({"volume": volume, "muted": muted}),
    );
}

/// Everything the pills share.
pub(super) struct Hub {
    loads: Arc<Mutex<HashMap<String, Load>>>,
    requested: HashSet<String>,
    pub(super) pills: HashMap<String, Pill>,
    /// Playback rate of voice messages, kept for all of them (`MEDIA_PLAYBACK_RATE_UPDATE`, type VOICE_MESSAGE).
    speed: f32,
    /// `MediaPlayerVolume`, 0 to 1, 1 by default (module 587481).
    pub(super) volume: f32,
    pub(super) muted: bool,
    pub(super) volume_drag: Option<String>,
    /// The answer of the last send, for the harness: the new message's id or the failure.
    #[cfg(feature = "capture")]
    last_send: Option<String>,
}

impl Hub {
    pub(super) fn new() -> Self {
        let (volume, muted) = stored_volume();
        Self {
            loads: Arc::new(Mutex::new(HashMap::new())),
            requested: HashSet::new(),
            pills: HashMap::new(),
            speed: 1.,
            volume,
            muted,
            volume_drag: None,
            #[cfg(feature = "capture")]
            last_send: None,
        }
    }

    pub(super) fn preload(&mut self, key: &str, clip: Arc<Clip>) {
        self.requested.insert(key.to_owned());
        if let Ok(mut loads) = self.loads.lock() {
            loads.insert(key.to_owned(), Load::Ready(clip));
        }
    }

    pub(super) fn load(&self, key: &str) -> Option<Load> {
        self.loads
            .lock()
            .ok()
            .and_then(|loads| loads.get(key).cloned())
    }

    pub(super) fn request(&mut self, key: &str, url: &str) {
        if !self.requested.insert(key.to_owned()) {
            return;
        }
        if let Ok(mut loads) = self.loads.lock() {
            loads.insert(key.to_owned(), Load::Loading);
        }
        let (loads, key, url) = (self.loads.clone(), key.to_owned(), url.to_owned());
        let _ = std::thread::Builder::new()
            .name("dc-voice-fetch".into())
            .spawn(move || {
                // A message of the audio attachment kind decodes any file; a voice message is Ogg Opus.
                let made = fetch(&url)
                    .and_then(|bytes| {
                        if key.starts_with(super::audio_attachment::KEY) {
                            Clip::from_file(bytes).map_err(str::to_owned)
                        } else {
                            Clip::from_ogg(&bytes).map_err(str::to_owned)
                        }
                    })
                    .map_or(Load::Failed, |clip| Load::Ready(Arc::new(clip)));
                if let Ok(mut loads) = loads.lock() {
                    loads.insert(key, made);
                }
            });
    }

    pub(super) fn pill(&mut self, key: &str) -> &mut Pill {
        self.pills.entry(key.to_owned()).or_default()
    }

    pub(super) fn sync_stored_volume(&mut self) {
        if self.volume_drag.is_none() {
            let (volume, muted) = stored_volume();
            self.volume = volume;
            self.muted = muted;
        }
    }

    fn apply_output(&self) {
        let engine = voice_player::engine();
        engine.set_speed(self.speed);
        engine.set_volume(self.volume);
        engine.set_muted(self.muted);
    }

    pub(super) fn start_if_ready(&mut self, key: &str, duration: f64) {
        self.sync_stored_volume();
        if !self.pills.get(key).is_some_and(|pill| pill.want_play) {
            return;
        }
        match self.load(key) {
            Some(Load::Ready(clip)) => {
                self.apply_output();
                let pill = self.pill(key);
                pill.want_play = false;
                pill.ended_at = None;
                let from = if pill.cached >= duration - 0.05 {
                    0.
                } else {
                    pill.cached
                };
                pill.played = true;
                voice_player::engine().play(key, clip, from);
            }
            Some(Load::Failed) => {
                self.pill(key).want_play = false;
            }
            _ => {}
        }
    }

    /// No Play waits for its download any more (the channel it was clicked in closed).
    pub(super) fn drop_play_intents(&mut self) {
        for pill in self.pills.values_mut() {
            pill.want_play = false;
        }
    }

    /// The play or pause button.
    pub(super) fn toggle(&mut self, key: &str, url: &str, duration: f64) {
        self.sync_stored_volume();
        let status = voice_player::engine().status();
        if status.key.as_deref() == Some(key) && status.playing {
            voice_player::engine().pause();
            return;
        }
        self.request(key, url);
        self.pill(key).want_play = true;
        self.start_if_ready(key, duration);
    }

    /// Plays when `play` and not playing, pauses when not `play` and playing (a harness step, not a button).
    #[cfg(any(test, feature = "capture"))]
    pub(super) fn toggle_if(&mut self, key: &str, duration: f64, play: bool) {
        let status = voice_player::engine().status();
        let playing = status.key.as_deref() == Some(key) && status.playing;
        if play != playing {
            self.toggle(key, "", duration);
        }
    }

    /// Moves the playhead to `fraction` of the message (module 408822 `d`).
    pub(super) fn seek(&mut self, key: &str, fraction: f64, duration: f64) {
        let at = (fraction.clamp(0., 1.) * duration).min(duration);
        let status = voice_player::engine().status();
        let pill = self.pill(key);
        pill.cached = at;
        pill.played = true;
        pill.ended_at = None;
        if status.key.as_deref() == Some(key) {
            voice_player::engine().seek(at);
        }
    }

    pub(super) fn drag_start(&mut self, key: &str) {
        let status = voice_player::engine().status();
        let resume = status.key.as_deref() == Some(key) && status.playing;
        if resume {
            voice_player::engine().pause();
        }
        self.pill(key).drag = Some(Drag { resume });
    }

    pub(super) fn drag_end(&mut self, key: &str, duration: f64) {
        let resume = self.pill(key).drag.take().is_some_and(|drag| drag.resume);
        if resume {
            self.pill(key).want_play = true;
            self.start_if_ready(key, duration);
        }
    }

    fn cycle_speed(&mut self) {
        self.speed = voice_wave::next_speed(self.speed);
        voice_player::engine().set_speed(self.speed);
        info!(speed = self.speed, "voice message speed");
    }

    pub(super) fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0., 1.);
        self.muted = self.volume <= 0.;
        store_volume(self.volume, self.muted);
        let engine = voice_player::engine();
        engine.set_volume(self.volume);
        engine.set_muted(self.muted);
    }

    pub(super) fn toggle_mute(&mut self) {
        self.sync_stored_volume();
        self.muted = !self.muted;
        if !self.muted && self.volume <= 0. {
            self.volume = unmute_restore_volume();
        }
        store_volume(self.volume, self.muted);
        let engine = voice_player::engine();
        engine.set_volume(self.volume);
        engine.set_muted(self.muted);
    }

    /// What this frame shows for `key`; also carries the pill through the end of a playback.
    fn view(&mut self, key: &str, duration: f64) -> View {
        self.sync_stored_volume();
        let status = voice_player::engine().status();
        let now = motion::now();
        let me = status.key.as_deref() == Some(key);
        let loading = self.pills.get(key).is_some_and(|pill| pill.want_play)
            && matches!(self.load(key), Some(Load::Loading));
        self.start_if_ready(key, duration);
        let slider_drag = self.volume_drag.as_deref() == Some(key);
        let pill = self.pill(key);
        if me && pill.drag.is_none() {
            pill.cached = status.position.min(duration);
        }
        if me && status.ended && pill.played && pill.ended_at.is_none() {
            pill.ended_at = Some(now);
        }
        if let Some(at) = pill.ended_at
            && now - at >= 500.
        {
            pill.ended_at = None;
            pill.played = false;
            pill.cached = 0.;
        }
        let playing = me && status.playing;
        if playing || loading || pill.ended_at.is_some() {
            motion::keep_animating();
        }
        View {
            playing,
            played: pill.played,
            current: pill.cached,
            slider_open: slider_drag || pill.over > 0 || pill.hide_at.is_some_and(|at| now < at),
        }
    }
}

struct View {
    playing: bool,
    played: bool,
    current: f64,
    slider_open: bool,
}

struct WaveSpec<'a> {
    key: &'a str,
    salt: usize,
    duration: f64,
    waveform: Option<Vec<f32>>,
    width: f32,
    played: bool,
    playing: bool,
    current: f64,
    fluid: bool,
    neutral: bool,
}

/// `Update::Fetched` key of a voice message send, followed by the channel.
pub(super) const VOICE_SEND_KEY: &str = "voice-send:";

/// Bytes of an attachment; a `file:` address (capture runs) is read from disk.
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    if let Some(path) = url.strip_prefix("file:") {
        return std::fs::read(path.trim_start_matches("//")).map_err(|error| error.to_string());
    }
    // Largest voice message file the desktop app reads (`0x1900000`).
    const MAX: usize = 25 * 1024 * 1024;
    crate::sounds::fetch_url_limited(url, MAX)
}

pub(super) fn tnum() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_owned(), 1)]))
}

/// A colour with `by` of black laid over it (the 20% black overlay of the pressed play button).
fn shade(base: Rgba, by: f32) -> Rgba {
    Rgba {
        r: base.r * (1. - by),
        g: base.g * (1. - by),
        b: base.b * (1. - by),
        a: base.a,
    }
}

fn gradient(from: Rgba, at: f32, to: Rgba, end: f32) -> gpui::Background {
    gradient_at(151.11, from, at, to, end)
}

/// The play button's `linear-gradient(<angle>deg, a at%, b at%)`.
fn gradient_at(angle: f32, from: Rgba, at: f32, to: Rgba, end: f32) -> gpui::Background {
    gpui::linear_gradient(
        angle,
        gpui::linear_color_stop(from, at),
        gpui::linear_color_stop(to, end),
    )
}

/// `d` in module 408822: where the pointer is across the waveform, 0 to 1.
pub(super) fn fraction_of(x: Pixels, bounds: Bounds<Pixels>) -> f64 {
    let width = f64::from(bounds.size.width);
    if width <= 0. {
        return 0.;
    }
    ((f64::from(x) - f64::from(bounds.origin.x)) / width).clamp(0., 1.)
}

/// Where the pointer is along the volume rail, 0 at the bottom, 1 at the top.
pub(super) fn position_of(y: Pixels, track: Bounds<Pixels>) -> f32 {
    let height = f32::from(track.size.height);
    if height <= 0. {
        return 0.;
    }
    (1. - (f32::from(y) - f32::from(track.origin.y)) / height).clamp(0., 1.)
}

/// Token triple `(background, active, inactive)` for `S` and `C` in module 408822. When `neutral`
/// (`neutralPlaybackColors` on recorded composer drafts in `989161` `e6`) is true, background stays
/// `--background-mod-muted`, played bars use `--interactive-text-active`, and unplayed bars use
/// `--interactive-text-default` (never `--brand-430` or `--white`).
pub(super) fn wave_color_tokens(
    playing: bool,
    played: bool,
    neutral: bool,
) -> (&'static str, &'static str, &'static str) {
    let brand_playing = playing && !neutral;
    let muted = if brand_playing {
        "--brand-430"
    } else {
        "--background-mod-muted"
    };
    let active = if brand_playing {
        "--white"
    } else if played {
        "--interactive-text-active"
    } else {
        "--interactive-text-default"
    };
    let inactive = if played && !neutral {
        muted
    } else {
        "--interactive-text-default"
    };
    (muted, active, inactive)
}

/// `rippleContainer_seLrDE` and `ripple_seLrDE`: while playing a `--brand-530` circle centred 24 px from the left grows
/// to twice the pill's width in 0.2 s (`width: 0` to `200%`), clipped by the pill's radius of 24; a 1 px
/// `--border-subtle` line round the pill.
fn ripple(playing: bool, key: &str) -> AnyElement {
    let grown = motion::value(
        motion::key(&("vm-ripple", key)),
        if playing { 1. } else { 0. },
        motion::css(200., motion::Ease::EaseInOut),
    );
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    if grown > 0. {
                        // The circle's overlap with the pill is drawn as a rounded rectangle: the circle while it is
                        // small, the whole pill at the end.
                        let width = f32::from(bounds.size.width);
                        let height = f32::from(bounds.size.height);
                        // The circle is centred 24 px from the left and at the middle inside the 1 px border
                        // (`left: 24px; top: 50%` of `rippleContainer_seLrDE`), its diameter is 200% of the
                        // inner width: the radius ends at the inner width.
                        let radius = (width - 2.) * grown;
                        let left = (25. - radius).max(0.);
                        let right = (25. + radius).min(width);
                        let top = (height / 2. - radius).max(0.);
                        let bottom = (height / 2. + radius).min(height);
                        let area = Bounds::new(
                            point(bounds.origin.x + px(left), bounds.origin.y + px(top)),
                            size(px(right - left), px(bottom - top)),
                        );
                        let round = 24f32.min((right - left) / 2.).min((bottom - top) / 2.);
                        window.paint_quad(fill(area, color("--brand-530")).corner_radii(px(round)));
                    }
                    window.paint_quad(
                        gpui::outline(bounds, color("--border-subtle"), gpui::BorderStyle::Solid)
                            .corner_radii(px(24.)),
                    );
                },
            )
            .size_full(),
        )
        .into_any_element()
}

impl Root {
    /// The pill of one voice message attachment.
    pub(super) fn voice_pill(&self, message: &Message, attachment: &Attachment) -> AnyElement {
        let hub = self.voice_hub.clone();
        let key = format!("{}:{}", message.id, attachment.id);
        let url = attachment.url.clone();
        let duration = attachment.duration_secs.filter(|seconds| *seconds > 0.);
        let span = duration.unwrap_or(0.);
        let view = hub.borrow_mut().view(&key, span);
        let waveform = attachment
            .waveform
            .as_deref()
            .and_then(voice_wave::decode_waveform);
        let wave_width = voice_wave::pill_wave_width(span);
        let (speed, volume, muted) = {
            let hub = hub.borrow();
            (hub.speed, hub.volume, hub.muted)
        };
        let playing = view.playing;
        let time = voice_wave::pill_time(duration, view.played, view.current);
        let salt = (attachment.id % 1_000_000) as usize;

        let play_button = {
            let (hub, key, url) = (hub.clone(), key.clone(), url.clone());
            let base = div()
                .id(("voice-play", salt))
                .parity("voice-play")
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(32.))
                .my(px(-4.))
                .ml(px(-4.))
                .rounded_full()
                .cursor_pointer()
                .on_click(move |_, window, _| {
                    hub.borrow_mut().toggle(&key, &url, span);
                    window.refresh();
                });
            // `playButtonContainer_seLrDE`: white while playing, otherwise the brand gradients at 151.11deg.
            let styled = if playing {
                base.bg(color("--white"))
                    .active(|style| style.bg(color("--brand-360")))
            } else if matches!(super::theme(), super::Theme::Light) {
                // `.theme-light .container_seLrDE:not(.playing_seLrDE) .playButtonContainer_seLrDE` (and `:hover`,
                // `:active`, which darkens by the 20% black layer the dark theme uses too).
                base.bg(gradient(color("--brand-400"), 0., color("--brand-500"), 1.))
                    .hover(|style| {
                        style.bg(gradient_at(
                            135.,
                            color("--brand-360"),
                            0.,
                            color("--brand-400"),
                            1.,
                        ))
                    })
                    .active(|style| {
                        style.bg(gradient_at(
                            135.,
                            shade(color("--brand-400"), 0.2),
                            0.,
                            shade(color("--brand-500"), 0.2),
                            1.,
                        ))
                    })
            } else {
                base.bg(gradient(
                    color("--brand-460"),
                    0.1655,
                    color("--brand-560"),
                    1.0436,
                ))
                .hover(|style| {
                    style.bg(gradient(
                        color("--brand-400"),
                        0.1778,
                        color("--brand-460"),
                        0.8222,
                    ))
                })
                .active(|style| {
                    style.bg(gradient(
                        shade(color("--brand-460"), 0.2),
                        0.1655,
                        shade(color("--brand-560"), 0.2),
                        1.0436,
                    ))
                })
            };
            styled.child(icon(
                if playing { "clips_pause" } else { "clips_play" },
                18.,
                if playing { "--brand-500" } else { "--white" },
            ))
        };

        let wave = self.voice_wave(WaveSpec {
            key: &key,
            salt,
            duration: span,
            waveform,
            width: wave_width,
            played: view.played,
            playing,
            current: view.current,
            fluid: false,
            neutral: false,
        });

        let duration_text = div()
            .flex_none()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_features(tnum())
            .text_color(color(if playing {
                "--white"
            } else {
                "--interactive-text-default"
            }))
            .child(time);

        let speed_button = {
            let hub = hub.clone();
            let label = voice_wave::speed_label(speed);
            div()
                .id(("voice-speed", salt))
                .parity("voice-speed")
                .py(px(4.))
                .cursor_pointer()
                .on_click(move |_, window, _| {
                    hub.borrow_mut().cycle_speed();
                    window.refresh();
                })
                .child(
                    div()
                        .w(px(32.))
                        .rounded(px(4.))
                        .text_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .bg(color(if playing {
                            "--interactive-background-active"
                        } else {
                            "--background-mod-subtle"
                        }))
                        .text_color(color(if playing {
                            "--brand-200"
                        } else {
                            "--interactive-text-default"
                        }))
                        .hover(|style| {
                            style
                                .bg(color("--interactive-background-hover"))
                                .text_color(color(if playing {
                                    "--white"
                                } else {
                                    "--interactive-text-active"
                                }))
                        })
                        .child(label),
                )
        };

        let volume_control =
            self.voice_volume(&key, salt, volume, muted, playing, view.slider_open);

        let pill = div()
            .id(("voice-pill", salt))
            .parity("voice-pill")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .p(px(12.))
            .max_w_full()
            .min_w_0()
            .rounded(px(24.))
            .bg(color("--background-mod-subtle"))
            .child(ripple(playing, &key))
            .child(play_button)
            .child(wave)
            .child(duration_text)
            .child(speed_button)
            .child(volume_control)
            // Not in the pinned client: VoiceDownload's button after the volume (`plugin_hooks.rs`).
            .children(super::plugin_hooks::voice_download_button(
                &attachment.url,
                salt,
            ))
            // Not in the pinned client: VoiceMessageTranscriber's button and box
            // (`plugin_hooks/voice_message_transcriber.rs`).
            .children(super::plugin_hooks::transcriber_button(&key, &url, salt));
        match super::plugin_hooks::transcriber_embed(&key, salt) {
            Some(embed) => div().flex().flex_col().child(pill).child(embed).into_any_element(),
            None => pill.into_any_element(),
        }
    }

    fn voice_wave(&self, spec: WaveSpec<'_>) -> AnyElement {
        let WaveSpec {
            key,
            salt,
            duration,
            waveform,
            width: fixed_width,
            played,
            playing,
            current,
            fluid,
            neutral,
        } = spec;
        let hub = self.voice_hub.clone();
        let (paint_hub, paint_key) = (hub.clone(), key.to_owned());
        let (down_hub, down_key) = (hub, key.to_owned());
        let tween = |name: &str, target: Rgba| {
            motion::color(
                motion::key(&("vm-wave", key, name)),
                target,
                motion::css(200., motion::Ease::Linear),
            )
        };
        // Colours of `S` and `C` in module 408822; `j` there is playing without neutral colours.
        let (bg_token, active_token, inactive_token) = wave_color_tokens(playing, played, neutral);
        let background = tween("bg", color(bg_token));
        let active = tween("active", color(active_token));
        let inactive = tween("inactive", color(inactive_token));
        let container = div()
            .id(("voice-wave", salt))
            .h(px(24.))
            .cursor_pointer()
            .when(fluid, |this| this.flex_1().min_w(px(0.)).ml(px(4.)))
            .when(!fluid, |this| this.flex_none().w(px(fixed_width)));
        container
            .on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, _| {
                    let mut hub = down_hub.borrow_mut();
                    let bounds = hub.pill(&down_key).wave;
                    hub.drag_start(&down_key);
                    let fraction = fraction_of(event.position.x, bounds);
                    hub.seek(&down_key, fraction, duration);
                    window.refresh();
                },
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let width = if fluid {
                            f32::from(bounds.size.width)
                        } else {
                            fixed_width
                        };
                        let bars = voice_wave::bar_count(width);
                        let values =
                            voice_wave::bars(waveform.as_deref().or(Some(&[0.; 5])), bars, fluid);
                        let lit = voice_wave::lit_bars(played, current, duration, bars);
                        // The canvas is 32 px high with a 4 px margin above and below (`.waveform` in 408822).
                        let area = Bounds::new(
                            point(bounds.origin.x, bounds.origin.y - px(4.)),
                            size(bounds.size.width, px(32.)),
                        );
                        let now = motion::now();
                        let mut animating = false;
                        let mut tones: Vec<(f32, bool)> = Vec::with_capacity(values.len());
                        {
                            let mut hub = paint_hub.borrow_mut();
                            let pill = hub.pill(&paint_key);
                            pill.wave = bounds;
                            if pill.segments.len() != values.len() {
                                pill.segments = values
                                    .iter()
                                    .enumerate()
                                    .map(|(index, value)| {
                                        Segment::new(if index < lit { *value } else { 0. })
                                    })
                                    .collect();
                            }
                            for (index, segment) in pill.segments.iter_mut().enumerate() {
                                if index < lit {
                                    segment.animate_to(values[index], now);
                                } else {
                                    segment.reset(now);
                                }
                                animating |= segment.is_animating(now);
                                tones.push((
                                    segment.current(now).max(values[index] - 0.1),
                                    segment.is_reset,
                                ));
                            }
                            if pill.drag.is_some() {
                                let (move_hub, move_key) = (paint_hub.clone(), paint_key.clone());
                                let (up_hub, up_key) = (paint_hub.clone(), paint_key.clone());
                                window.on_mouse_event(
                                    move |event: &MouseMoveEvent, phase, window, _| {
                                        if phase != DispatchPhase::Capture {
                                            return;
                                        }
                                        let mut hub = move_hub.borrow_mut();
                                        let bounds = hub.pill(&move_key).wave;
                                        let fraction = fraction_of(event.position.x, bounds);
                                        hub.seek(&move_key, fraction, duration);
                                        window.refresh();
                                    },
                                );
                                window.on_mouse_event(move |_: &MouseUpEvent, phase, window, _| {
                                    if phase != DispatchPhase::Capture {
                                        return;
                                    }
                                    up_hub.borrow_mut().drag_end(&up_key, duration);
                                    window.refresh();
                                });
                            }
                        }
                        if animating {
                            motion::keep_animating();
                        }
                        let mut bar = |index: usize, value: f32, floor: bool, tone: Rgba| {
                            let height = voice_wave::bar_height(value, floor);
                            if height <= 0. {
                                return;
                            }
                            // `y` in 408822: a bar 2 px wide every 6 px, centred on the 32 px canvas, radius 1.
                            let rect = Bounds::new(
                                point(
                                    area.origin.x + px(6. * index as f32),
                                    area.origin.y + px(16. - height / 2.),
                                ),
                                size(px(2.), px(height)),
                            );
                            window.paint_quad(fill(rect, tone).corner_radii(px(1.)));
                        };
                        for (index, value) in values.iter().enumerate() {
                            bar(index, *value, true, background);
                        }
                        for (index, (value, reset)) in tones.iter().enumerate() {
                            if *reset {
                                bar(index, *value, false, inactive);
                            } else {
                                bar(index, *value, true, active);
                            }
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    fn voice_volume(
        &self,
        key: &str,
        salt: usize,
        volume: f32,
        muted: bool,
        playing: bool,
        open: bool,
    ) -> AnyElement {
        let hub = self.voice_hub.clone();
        let position = volume_to_slider(volume * 100.) / 100.;
        // Module 20504: `VoiceX` at the minimum or muted, `VoiceLow` under half, `VoiceNormal` above.
        let glyph = if muted || position <= 0. {
            "vm_voice_x"
        } else if position < 0.5 {
            "vm_voice_low"
        } else {
            "sound_speaker"
        };
        // `volumeButtonIcon_seLrDE`: `--interactive-text-default`, `--brand-200` while playing.
        let tone = if playing {
            "--brand-200"
        } else {
            "--interactive-text-default"
        };
        let hovered = hub
            .borrow()
            .pills
            .get(key)
            .is_some_and(|pill| pill.over > 0);
        let hover_tone = if playing {
            "--white"
        } else {
            "--interactive-text-hover"
        };
        let (hover_hub, hover_key) = (hub.clone(), key.to_owned());
        let click_hub = hub;
        let button = div()
            .id(("voice-volume", salt))
            .parity("voice-volume")
            .flex_none()
            .cursor_pointer()
            .on_click(move |_, window, _| {
                click_hub.borrow_mut().toggle_mute();
                window.refresh();
            })
            .on_hover(move |hovered, window, _| {
                let mut hub = hover_hub.borrow_mut();
                let pill = hub.pill(&hover_key);
                if *hovered {
                    pill.over = 1;
                    pill.hide_at = None;
                } else {
                    pill.over = 0;
                    // The slider hides 150 ms after the pointer leaves (`_hoverTimeout`).
                    pill.hide_at = Some(motion::now() + 150.);
                }
                window.refresh();
            })
            .child(icon(glyph, 24., if hovered { hover_tone } else { tone }));
        let mut holder = div().relative().flex().flex_none().child(button);
        if open {
            holder = holder.child(self.voice_slider(key, salt, position, 28., 4.));
        }
        holder.into_any_element()
    }

    pub(super) fn voice_slider(
        &self,
        key: &str,
        salt: usize,
        position: f32,
        bottom: f32,
        left: f32,
    ) -> AnyElement {
        let hub = self.voice_hub.clone();
        let (down_hub, down_key) = (hub.clone(), key.to_owned());
        let (paint_hub, paint_key) = (hub, key.to_owned());
        // `mediaBarInteractionVolume_s1cr3c`: 14 by 88 (8 px padding round a 6 by 72 rail), `rgba(0,0,0,.68)`, radius 8, 4 px
        // above the button; `mediaBarProgress_s1cr3c` is `rgb(88,101,242)` (`--brand-500`), the grabber is 10 px and
        // only seen while dragging (`transform: scale(0)` otherwise).
        div()
            .id(("voice-slider", salt))
            .absolute()
            .bottom(px(bottom))
            .left(px(left))
            .w(px(14.))
            .h(px(88.))
            .rounded(px(8.))
            .bg(color("--opacity-black-68"))
            .on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, _| {
                    let mut hub = down_hub.borrow_mut();
                    hub.volume_drag = Some(down_key.clone());
                    let track = hub.pill(&down_key).track;
                    let value = position_of(event.position.y, track);
                    hub.set_volume(slider_to_volume(value * 100.) / 100.);
                    window.refresh();
                },
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let track = Bounds::new(
                            point(bounds.origin.x + px(4.), bounds.origin.y + px(8.)),
                            size(px(6.), bounds.size.height - px(16.)),
                        );
                        {
                            let mut hub = paint_hub.borrow_mut();
                            hub.pill(&paint_key).track = track;
                            if hub.volume_drag.is_some() {
                                let (move_hub, move_key) = (paint_hub.clone(), paint_key.clone());
                                let (up_hub, up_key) = (paint_hub.clone(), paint_key.clone());
                                window.on_mouse_event(
                                    move |event: &MouseMoveEvent, phase, window, _| {
                                        if phase != DispatchPhase::Capture {
                                            return;
                                        }
                                        let mut hub = move_hub.borrow_mut();
                                        let track = hub.pill(&move_key).track;
                                        let value = position_of(event.position.y, track);
                                        hub.set_volume(slider_to_volume(value * 100.) / 100.);
                                        window.refresh();
                                    },
                                );
                                window.on_mouse_event(move |_: &MouseUpEvent, phase, window, _| {
                                    if phase != DispatchPhase::Capture {
                                        return;
                                    }
                                    let mut hub = up_hub.borrow_mut();
                                    hub.volume_drag = None;
                                    hub.pill(&up_key).hide_at = Some(motion::now() + 150.);
                                    window.refresh();
                                });
                            }
                        }
                        let dragging = paint_hub.borrow().volume_drag.is_some();
                        // `mediaBarWrapperVolume_s1cr3c`: `rgba(201,202,206,.3)`, the `--primary-300` at 30%.
                        let mut rail = color("--primary-300");
                        rail.a *= 0.3;
                        window.paint_quad(fill(track, rail).corner_radii(px(3.)));
                        let filled = track.size.height * position.clamp(0., 1.);
                        let top = track.origin.y + track.size.height - filled;
                        let bar =
                            Bounds::new(point(track.origin.x, top), size(track.size.width, filled));
                        window.paint_quad(fill(bar, color("--brand-500")).corner_radii(px(3.)));
                        let grabber = Bounds::new(
                            point(track.origin.x - px(2.), top - px(5.)),
                            size(px(10.), px(10.)),
                        );
                        if dragging {
                            window.paint_quad(
                                fill(grabber, color("--brand-500")).corner_radii(px(5.)),
                            );
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }
}

/// Capture only: the steps `vm <verb>`.
#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_vm(&mut self, words: &[&str], cx: &mut gpui::Context<Self>) -> String {
        let hub = self.voice_hub.clone();
        let report = match words {
            ["speed"] => {
                hub.borrow_mut().cycle_speed();
                format!("voice: speed {}", voice_wave::speed_label(hub.borrow().speed))
            }
            ["volume", value] => match value.parse::<f32>() {
                Ok(value) => {
                    hub.borrow_mut().set_volume(value / 100.);
                    format!("voice: volume {value}")
                }
                Err(_) => "voice: volume wants a number from 0 to 100".to_owned(),
            },
            ["mute"] => {
                hub.borrow_mut().toggle_mute();
                format!("voice: muted {}", hub.borrow().muted)
            }
            ["state"] => {
                let status = voice_player::engine().status();
                let hub = hub.borrow();
                format!(
                    "voice: key {:?} playing {} ended {} position {:.3} rendered {} error {:?} speed {} volume {} muted {}",
                    status.key,
                    status.playing,
                    status.ended,
                    status.position,
                    status.rendered,
                    status.error,
                    hub.speed,
                    hub.volume,
                    hub.muted,
                )
            }
            ["theme", name] => {
                let theme = match *name {
                    "light" => super::Theme::Light,
                    "dark" => super::Theme::Dark,
                    "midnight" => super::Theme::Midnight,
                    _ => super::Theme::Darker,
                };
                super::set_theme(theme);
                format!("voice: theme {name}")
            }
            ["rec", rest @ ..] => self.script_rec(rest, cx),
            ["send-fixture"] => self.voice_send_fixture(),
            ["sent"] => format!(
                "voice: last send {}",
                hub.borrow().last_send.as_deref().unwrap_or("none yet")
            ),
            ["wait", ms] => {
                let ms = ms.parse::<u64>().unwrap_or(0);
                std::thread::sleep(std::time::Duration::from_millis(ms));
                format!("voice: waited {ms}")
            }
            ["demo", rest @ ..] => self.voice_demo(rest),
            [verb @ ("play" | "pause"), ..] => match self.voice_demo_key.clone() {
                Some(key) => {
                    let duration = self.voice_demo_seconds;
                    hub.borrow_mut().toggle_if(&key, duration, *verb == "play");
                    format!("voice: {verb} {key}")
                }
                None => "voice: no demo message, run `voice demo` first".to_owned(),
            },
            ["seek", at] => match (self.voice_demo_key.clone(), at.parse::<f64>()) {
                (Some(key), Ok(at)) => {
                    let duration = self.voice_demo_seconds;
                    hub.borrow_mut().seek(&key, at / duration, duration);
                    format!("voice: seek {at}")
                }
                _ => "voice: seek wants a demo message and seconds".to_owned(),
            },
            _ => "voice: usage `vm demo [seconds] [save <path>] | play | pause | seek <s> | speed | volume <0-100> | mute | state | wait <ms>`".to_owned(),
        };
        cx.notify();
        report
    }

    /// `vm send-fixture`: posts `tests/fixtures/voice_message.ogg` as a voice message. Only the fixture server's
    /// #general and the test DM take it; no microphone is involved. The answer is `vm sent`.
    fn voice_send_fixture(&mut self) -> String {
        const GENERAL: u64 = 1_568_692_565_136_375_900;
        const TEST_DM: u64 = 1_568_693_333_864_480_939;
        let Some(channel) = self.channel.filter(|id| *id == GENERAL || *id == TEST_DM) else {
            return "voice: send-fixture only posts in the fixture #general or the test DM"
                .to_owned();
        };
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/voice_message.ogg"
        );
        let file = match std::fs::read(path) {
            Ok(file) => file,
            Err(error) => return format!("voice: cannot read the fixture: {error}"),
        };
        let (clip, pcm) = match (Clip::from_ogg(&file), dc_voice::ogg_opus::decode(&file)) {
            (Ok(clip), Ok(pcm)) => (clip, pcm),
            _ => return "voice: the fixture does not decode".to_owned(),
        };
        // One level per 40 ms, the recorder's own measure, cut to 256 values.
        let levels: Vec<u8> = pcm
            .samples
            .chunks(1920)
            .map(|window| voice_wave::level_byte(voice_wave::level_db(window)))
            .collect();
        let waveform = voice_wave::encode_waveform(&levels);
        let duration = clip.seconds();
        let key = format!("{}{}", VOICE_SEND_KEY, channel);
        let _ = self.requests.send(crate::net::Request::SendVoice {
            key,
            channel,
            file,
            duration_secs: duration,
            waveform: waveform.clone(),
            reply: None,
            // Not in the pinned client: SilentMessageToggle (`plugin_hooks/silent_message_toggle.rs`).
            extra_flags: self.silent_message_take_flag(),
        });
        format!(
            "voice: send requested in {channel}, {duration:.3} s, waveform {} bytes ({waveform})",
            levels.len().min(256)
        )
    }
}

impl Root {
    /// The answer of `Request::SendVoice`.
    pub(super) fn voice_sent(&mut self, body: &serde_json::Value, cx: &mut gpui::Context<Self>) {
        let line = if body["ok"].as_bool() == Some(true) {
            format!(
                "sent, message {}",
                body["body"]["id"].as_str().unwrap_or("?")
            )
        } else {
            // `GjXjfk`
            self.put_app_toast(dc_intl::t!("GjXjfk").to_owned(), false, cx);
            format!("failed, status {} {}", body["status"], body["body"])
        };
        info!("voice message {line}");
        #[cfg(feature = "capture")]
        {
            self.voice_hub.borrow_mut().last_send = Some(line);
        }
        #[cfg(not(feature = "capture"))]
        let _ = line;
        cx.notify();
    }
}

#[cfg(feature = "capture")]
impl Root {
    /// `vmfwd`: the Forward modal on the newest voice message of the open channel (nothing is sent).
    pub(crate) fn voice_forward_modal(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> String {
        let Some(message) = self.channel.and_then(|channel| {
            self.stores
                .messages
                .get(&channel)
                .and_then(|list| list.iter().rev().find(|m| m.is_voice_message()).cloned())
        }) else {
            return "voice: no voice message in this channel".to_owned();
        };
        self.open_forward(message, window, cx);
        "voice: forward modal open".to_owned()
    }

    /// `vm demo [seconds] [save <path>]`: a message with a synthetic recording (the recorder's own output) is put
    /// into the open channel, in memory only; its clip is already loaded so nothing is fetched.
    fn voice_demo(&mut self, rest: &[&str]) -> String {
        let Some(channel) = self.channel else {
            return "voice: no channel is open".to_owned();
        };
        let seconds = rest
            .first()
            .and_then(|text| text.parse::<u64>().ok())
            .unwrap_or(4);
        let recorder = match dc_voice::voice_recorder::Recorder::start_synthetic() {
            Ok(recorder) => recorder,
            Err(error) => return format!("voice: {error}"),
        };
        recorder.advance_synthetic(seconds * 1000);
        let recording = match recorder.stop() {
            Ok(recording) => recording,
            Err(error) => return format!("voice: {error}"),
        };
        if let [_, "save", path] = rest
            && let Err(error) = std::fs::write(path, &recording.file)
        {
            return format!("voice: could not save {path}: {error}");
        }
        let clip = match Clip::from_ogg(&recording.file) {
            Ok(clip) => Arc::new(clip),
            Err(error) => return format!("voice: {error}"),
        };
        let now = chrono::Utc::now();
        let id = ((now.timestamp_millis() - 1_420_070_400_000) as u64) << 22;
        let attachment = 9_100_u64;
        let value = serde_json::json!({
            "id": (id + 1).to_string(),
            "channel_id": channel.to_string(),
            "type": 0,
            "content": "",
            "flags": dc_state::VOICE_MESSAGE_FLAG,
            "timestamp": (now - chrono::Duration::seconds(60))
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
            "author": {"id": "900", "username": "ada"},
            "attachments": [{
                "id": attachment.to_string(),
                "filename": "voice-message.ogg",
                "size": recording.file.len(),
                "url": "file:voice-demo.ogg",
                "content_type": "audio/ogg",
                "duration_secs": recording.duration_secs,
                "waveform": recording.waveform,
            }],
        });
        let Some(message) = dc_state::Message::from_value(&value) else {
            return "voice: the demo message did not parse".to_owned();
        };
        let key = format!("{}:{attachment}", message.id);
        self.voice_hub.borrow_mut().preload(&key, clip);
        self.voice_demo_key = Some(key.clone());
        self.voice_demo_seconds = recording.duration_secs;
        let messages = self.stores.messages.entry(channel).or_default();
        messages.push(message);
        // `reply` (any word after the length): a message answering it, to see the reply preview.
        if rest.contains(&"reply") {
            let answer = serde_json::json!({
                "id": (id + 2).to_string(),
                "channel_id": channel.to_string(),
                "type": 19,
                "content": "an answer",
                "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                "author": {"id": "901", "username": "grace"},
                "message_reference": {"message_id": (id + 1).to_string(), "channel_id": channel.to_string()},
                "referenced_message": value,
            });
            if let Some(answer) = dc_state::Message::from_value(&answer) {
                messages.push(answer);
            }
        }
        // `forward`: a forward of it (`message_snapshots`), to see the copy inside the forward block.
        if rest.contains(&"forward") {
            let forward = serde_json::json!({
                "id": (id + 3).to_string(),
                "channel_id": channel.to_string(),
                "type": 0,
                "content": "",
                "flags": 16384,
                "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                "author": {"id": "901", "username": "grace"},
                "message_reference": {"type": 1, "message_id": (id + 1).to_string(), "channel_id": channel.to_string()},
                "message_snapshots": [{"message": {
                    "content": "",
                    "flags": dc_state::VOICE_MESSAGE_FLAG,
                    "timestamp": value["timestamp"],
                    "attachments": value["attachments"],
                    "embeds": [],
                }}],
            });
            if let Some(forward) = dc_state::Message::from_value(&forward) {
                messages.push(forward);
            }
        }
        // `pin`: the pins popout opens with it as the only pinned message.
        if rest.contains(&"pin") {
            self.fetched.insert(
                format!("pins:{channel}"),
                serde_json::json!({"items": [{"pinned_at": value["timestamp"], "message": value}], "has_more": false}),
            );
            self.pins_open = Some("pins");
        }
        self.rebuild_rows();
        format!(
            "voice: demo {key} {:.2} s, {} bytes, {} waveform bytes",
            recording.duration_secs,
            recording.file.len(),
            recording.levels.len().min(256),
        )
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The recorder in the composer (controller: module 841817; "+" menu gate: module 316576 `canRecordVoiceMessage`,
// experiment `2026-09-desktop-voice-message-recording` of module 74477, off by default).
// ---------------------------------------------------------------------------------------------------------------

use dc_voice::voice_recorder::{Recorder, Recording};

/// A new bar of the live waveform every 48 ms, each easing `u += (N - u) * 0.45` towards its level.
const REC_BAR_STEP_MS: f64 = 48.;

enum RecPhase {
    Idle,
    Recording(Recorder),
    Recorded(Recording),
}

/// The one voice recording of the app (the official controller allows one at a time).
pub(super) struct RecState {
    /// A forced answer for the experiment gate: `GRAVECORD_EXPERIMENT_VOICE_RECORDING=1|0` or `vm rec on|off`.
    pub(super) forced: Option<bool>,
    channel: Option<u64>,
    phase: RecPhase,
    shown: Vec<f32>,
    last_step: f64,
    /// A new recording was asked for in another channel while this one is unsent: the discard modal is open.
    pub(super) confirm: bool,
    /// `isFinalizing` in module 841817 / 316576: encoding is completing, disabling Send and the "+" menu row.
    pub(super) finalizing: bool,
    pending_stop_toast: bool,
}

impl RecState {
    pub(super) fn new() -> Self {
        Self {
            forced: std::env::var("GRAVECORD_EXPERIMENT_VOICE_RECORDING")
                .ok()
                .and_then(|value| match value.as_str() {
                    "1" => Some(true),
                    "0" => Some(false),
                    _ => None,
                }),
            channel: None,
            phase: RecPhase::Idle,
            shown: Vec::new(),
            last_step: 0.,
            confirm: false,
            finalizing: false,
            pending_stop_toast: false,
        }
    }

    fn elapsed_ms(&self) -> u64 {
        match &self.phase {
            RecPhase::Recording(recorder) => recorder.elapsed_ms(),
            RecPhase::Recorded(recording) => (recording.duration_secs * 1000.) as u64,
            RecPhase::Idle => 0,
        }
    }

    /// Whether a recording or draft belongs to `channel`.
    pub(super) fn active_in(&self, channel: u64) -> bool {
        self.channel == Some(channel) && !matches!(self.phase, RecPhase::Idle)
    }

    fn draft_key(channel: u64) -> String {
        format!("voice-draft:{channel}")
    }
}

impl Root {
    /// `eP` in module 316576, the experiment `2026-09-desktop-voice-message-recording` (module 74477, variation 1
    /// enabled): the account's assignment from READY's `apex_experiments`. The official client offers the row on the
    /// fixture account while its READY carries no entry for it, so an account without one is treated as enabled;
    /// only an explicit variation 0 or a forced answer turns it off.
    pub(super) fn voice_recording_enabled(&self) -> bool {
        if let Some(forced) = self.voice_rec.borrow().forced {
            return forced;
        }
        self.stores
            .experiments
            .variant("2026-09-desktop-voice-message-recording")
            != Some(0)
    }

    pub(super) fn voice_recording_finalizing(&self) -> bool {
        self.voice_rec.borrow().finalizing
    }

    /// The modal of module 841817 (`G`): `size: "sm"`, title `YeUur2`, a subtitle naming where the unsent message is
    /// (`8o9Emp` group DM, `UlsYSN` DM, `hjBbhU` otherwise), a vertical action bar with `h7tN5I` (secondary, focused)
    /// and `IQrVoF` (critical). Layout is the app's standard small stacked dialog; no official frame exists (it needs
    /// a recording in progress), and the card naming the other server or conversation is not drawn.
    pub(super) fn voice_discard_modal(&self, cx: &mut gpui::Context<Self>) -> Option<AnyElement> {
        use super::modal::{Spec, Tone, button, render_small_stacked};
        // Catch any channel navigation (`select_home`, favorites, etc.) that switched away from an active recording,
        // or flush the toast queued by `select()`.
        self.voice_record_stop_leaving(self.channel);
        if self.voice_rec.borrow().pending_stop_toast {
            let weak = self.weak.clone();
            cx.defer(move |cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.voice_record_stop_on_navigate(this.channel, cx);
                });
            });
        }
        if !self.voice_rec.borrow().confirm {
            return None;
        }
        let held = self.voice_rec.borrow().channel?;
        let dm = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == held);
        let subtitle = match dm {
            Some(dm) if dm.recipients.len() > 1 => dc_intl::t!("8o9Emp").to_owned(),
            Some(dm) => {
                let username = self
                    .stores
                    .dm_recipient(dm)
                    .map(|user| user.display_name().to_owned())
                    .unwrap_or_default();
                dc_intl::t!("UlsYSN", username = username)
            }
            None => {
                let channel_name = self
                    .stores
                    .guilds
                    .iter()
                    .flat_map(|guild| &guild.channels)
                    .find(|known| known.id == held)
                    .map(|known| known.name.clone())
                    .unwrap_or_default();
                dc_intl::t!("hjBbhU", channelName = channel_name)
            }
        };
        // The card naming where the draft is (children of module 841817 `G`, shown when the draft is in another
        // server or in a DM): `.destination__M7qrc` (48 px tall, 12 px gap, 8 px padding, `--radius-sm` 8 px,
        // hover `--interactive-background-hover`), a 32 px picture, the name in `body-md/semibold` and a
        // `body-xs/medium` `--text-muted` line (channel for a server, tag for a DM).
        let here = self
            .stores
            .guilds
            .iter()
            .find(|guild| {
                guild
                    .channels
                    .iter()
                    .any(|known| Some(known.id) == self.channel)
            })
            .map(|guild| guild.id);
        let there = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == held));
        let card = if dm.is_some() || there.map(|guild| guild.id) != here {
            let (primary, secondary, picture) = match (dm, there) {
                (Some(dm), _) => {
                    let user = self.stores.dm_recipient(dm).cloned();
                    (
                        user.as_ref()
                            .map(|user| user.display_name().to_owned())
                            .unwrap_or_default(),
                        user.as_ref()
                            .map(|user| user.username.clone())
                            .unwrap_or_default(),
                        user.as_ref().and_then(|user| {
                            self.images.get(&super::avatar_source(user).0).cloned()
                        }),
                    )
                }
                (None, Some(guild)) => (
                    guild.name.clone(),
                    guild
                        .channels
                        .iter()
                        .find(|known| known.id == held)
                        .map(|known| known.name.clone())
                        .unwrap_or_default(),
                    self.images.get(&format!("guild:{}", guild.id)).cloned(),
                ),
                _ => (String::new(), String::new(), None),
            };
            Some(
                div()
                    .id("voice-discard-card")
                    .min_h(px(48.))
                    .mx(px(-8.))
                    .px(px(8.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.voice_rec.borrow_mut().confirm = false;
                        if this
                            .stores
                            .private_channels
                            .iter()
                            .any(|known| known.id == held)
                        {
                            this.select_home(Some(held));
                        } else {
                            this.open_channel_mention(held, cx);
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .rounded_full()
                            .overflow_hidden()
                            .bg(color("--background-base-lower"))
                            .when_some(picture, |this, picture| {
                                this.child(gpui::img(picture).size(px(32.)))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(color("--text-default"))
                                    .child(primary),
                            )
                            .when(!secondary.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(color("--text-muted"))
                                        .child(secondary),
                                )
                            }),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };
        Some(render_small_stacked(
            Spec {
                id: "voice-discard-modal",
                title: dc_intl::t!("YeUur2").into(),
                subtitle: Some(div().child(subtitle).into_any_element()),
                body: card,
                buttons: vec![
                    button(
                        "voice-return",
                        dc_intl::t!("h7tN5I"),
                        Tone::Secondary,
                        move |this, _, cx| {
                            this.voice_rec.borrow_mut().confirm = false;
                            if this
                                .stores
                                .private_channels
                                .iter()
                                .any(|known| known.id == held)
                            {
                                this.select_home(Some(held));
                            } else {
                                this.open_channel_mention(held, cx);
                            }
                        },
                    ),
                    button(
                        "voice-discard",
                        dc_intl::t!("IQrVoF"),
                        Tone::Critical,
                        |this, _, cx| {
                            this.voice_rec.borrow_mut().confirm = false;
                            this.voice_record_discard(cx);
                            this.voice_record_start(cx);
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.voice_rec.borrow_mut().confirm = false),
            },
            cx,
        ))
    }

    /// Alt+R: starts a recording, or sends the draft when one is waiting in this channel. Whether it took the key.
    pub(super) fn voice_record_shortcut(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let waiting = self
            .channel
            .is_some_and(|channel| self.voice_rec.borrow().active_in(channel));
        if waiting {
            return false;
        }
        self.voice_record_start(cx).starts_with("voice: recording")
    }

    /// The "+" menu row, Alt+R and the harness start here.
    pub(super) fn voice_record_start(&mut self, cx: &mut gpui::Context<Self>) -> String {
        let Some(channel) = self.channel else {
            return "voice: no channel".to_owned();
        };
        if self.voice_recording_finalizing() {
            return "voice: finalizing".to_owned();
        }
        if !self.record_voice_offered() {
            return "voice: recording is not offered here".to_owned();
        }
        if !matches!(self.voice_rec.borrow().phase, RecPhase::Idle) {
            // One recording at a time: asking in another channel opens the discard modal (module 841817 `G`).
            if self.voice_rec.borrow().channel != Some(channel) {
                self.voice_rec.borrow_mut().confirm = true;
                cx.notify();
                return "voice: unsent recording elsewhere, asking".to_owned();
            }
            return "voice: a recording already exists".to_owned();
        }
        let started = if crate::storage::is_capture_run() {
            Recorder::start_synthetic()
        } else {
            Recorder::start_microphone()
        };
        match started {
            Ok(recorder) => {
                let mut state = self.voice_rec.borrow_mut();
                state.phase = RecPhase::Recording(recorder);
                state.channel = Some(channel);
                state.shown.clear();
                state.last_step = motion::now();
                drop(state);
                cx.notify();
                "voice: recording".to_owned()
            }
            Err(_) => {
                // `H03AqF`
                self.put_app_toast(dc_intl::t!("H03AqF").to_owned(), false, cx);
                "voice: failed to start".to_owned()
            }
        }
    }

    /// Stops an active recording when navigating away from its channel (`989161` `nA` + `841817` `eu`/`yx`), keeping
    /// the recorded draft on that channel, playing `"voice_message_recording_stopped"`, and queueing the toast when no
    /// `Context` is at hand.
    pub(super) fn voice_record_stop_leaving(&self, next_channel: Option<dc_state::Id>) -> bool {
        let should_stop = {
            let state = self.voice_rec.borrow();
            matches!(state.phase, RecPhase::Recording(_)) && state.channel != next_channel
        };
        if !should_stop {
            return false;
        }
        let phase = std::mem::replace(&mut self.voice_rec.borrow_mut().phase, RecPhase::Idle);
        let RecPhase::Recording(recorder) = phase else {
            self.voice_rec.borrow_mut().phase = phase;
            return false;
        };
        self.voice_rec.borrow_mut().finalizing = true;
        let held_channel = self.voice_rec.borrow().channel;
        match recorder.stop() {
            Ok(recording) if !recording.too_short() => {
                if let (Some(channel), Ok(clip)) = (held_channel, Clip::from_ogg(&recording.file)) {
                    self.voice_hub
                        .borrow_mut()
                        .preload(&RecState::draft_key(channel), Arc::new(clip));
                }
                self.voice_rec.borrow_mut().phase = RecPhase::Recorded(recording);
            }
            _ => {
                self.voice_rec.borrow_mut().channel = None;
            }
        }
        self.voice_rec.borrow_mut().finalizing = false;
        if !crate::storage::is_capture_run() && std::env::var_os("GRAVECORD_INERT").is_none() {
            let _ = crate::sounds::play("voice_message_recording_stopped");
        }
        self.voice_rec.borrow_mut().pending_stop_toast = true;
        true
    }

    /// `989161` `nA` + `841817` `eu`/`yx`: stops an active recording when leaving its channel, retains the recorded
    /// draft for that channel, plays `"voice_message_recording_stopped"`, and shows toast `UM2R7g`.
    pub(super) fn voice_record_stop_on_navigate(
        &mut self,
        next_channel: Option<dc_state::Id>,
        cx: &mut gpui::Context<Self>,
    ) {
        let stopped = self.voice_record_stop_leaving(next_channel);
        let queued = std::mem::take(&mut self.voice_rec.borrow_mut().pending_stop_toast);
        if stopped || queued {
            self.put_app_toast(dc_intl::t!("UM2R7g").to_owned(), false, cx);
            cx.notify();
        }
    }

    /// Stop: under half a second the recording is thrown away, else it becomes the draft.
    pub(super) fn voice_record_stop(&mut self, cx: &mut gpui::Context<Self>) -> String {
        let phase = std::mem::replace(&mut self.voice_rec.borrow_mut().phase, RecPhase::Idle);
        let RecPhase::Recording(recorder) = phase else {
            self.voice_rec.borrow_mut().phase = phase;
            return "voice: not recording".to_owned();
        };
        self.voice_rec.borrow_mut().finalizing = true;
        let channel = self.voice_rec.borrow().channel;
        let report = match recorder.stop() {
            Ok(recording) if !recording.too_short() => {
                let seconds = recording.duration_secs;
                if let (Some(channel), Ok(clip)) = (channel, Clip::from_ogg(&recording.file)) {
                    self.voice_hub
                        .borrow_mut()
                        .preload(&RecState::draft_key(channel), Arc::new(clip));
                }
                self.voice_rec.borrow_mut().phase = RecPhase::Recorded(recording);
                format!("voice: recorded {seconds:.3} s")
            }
            Ok(_) => "voice: under half a second, discarded".to_owned(),
            Err(error) => {
                // `UM2R7g`
                self.put_app_toast(dc_intl::t!("UM2R7g").to_owned(), false, cx);
                format!("voice: stop failed: {error}")
            }
        };
        self.voice_rec.borrow_mut().finalizing = false;
        cx.notify();
        report
    }

    /// The trash button: the recording or draft goes away.
    pub(super) fn voice_record_discard(&mut self, cx: &mut gpui::Context<Self>) -> String {
        let phase = std::mem::replace(&mut self.voice_rec.borrow_mut().phase, RecPhase::Idle);
        if let RecPhase::Recording(recorder) = phase {
            recorder.cancel();
        }
        if let Some(channel) = self.voice_rec.borrow().channel {
            let key = RecState::draft_key(channel);
            if voice_player::engine().status().key.as_deref() == Some(key.as_str()) {
                voice_player::engine().stop();
            }
        }
        cx.notify();
        "voice: discarded".to_owned()
    }

    /// The send button and Enter: uploads the draft (a recording in progress is stopped first).
    pub(super) fn voice_record_send(&mut self, cx: &mut gpui::Context<Self>) -> String {
        if matches!(self.voice_rec.borrow().phase, RecPhase::Recording(_)) {
            return self.voice_record_stop(cx);
        }
        let phase = std::mem::replace(&mut self.voice_rec.borrow_mut().phase, RecPhase::Idle);
        let RecPhase::Recorded(recording) = phase else {
            self.voice_rec.borrow_mut().phase = phase;
            return "voice: nothing to send".to_owned();
        };
        let Some(channel) = self.voice_rec.borrow().channel.or(self.channel) else {
            return "voice: no channel".to_owned();
        };
        if voice_player::engine().status().key.as_deref()
            == Some(RecState::draft_key(channel).as_str())
        {
            voice_player::engine().stop();
        }
        let ping = self.reply_ping;
        let reply = self
            .replying
            .take()
            .map(|message| (self.guild, message.id, ping));
        let _ = self.requests.send(crate::net::Request::SendVoice {
            key: format!("{VOICE_SEND_KEY}{channel}"),
            channel,
            file: recording.file,
            duration_secs: recording.duration_secs,
            waveform: recording.waveform,
            reply,
            // Not in the pinned client: SilentMessageToggle (`plugin_hooks/silent_message_toggle.rs`).
            extra_flags: self.silent_message_take_flag(),
        });
        cx.notify();
        format!("voice: send requested in {channel}")
    }

    /// `vm rec ...`.
    #[cfg(feature = "capture")]
    pub(super) fn script_rec(&mut self, words: &[&str], cx: &mut gpui::Context<Self>) -> String {
        match words {
            ["on"] | ["off"] => {
                self.voice_rec.borrow_mut().forced = Some(words[0] == "on");
                cx.notify();
                format!("voice: recording experiment {}", words[0])
            }
            ["start"] => self.voice_record_start(cx),
            // Opens the discard modal as if a recording had been asked for in another channel.
            ["confirm"] => {
                self.voice_rec.borrow_mut().confirm = true;
                cx.notify();
                "voice: discard modal open".to_owned()
            }
            ["advance", ms] => {
                if let RecPhase::Recording(recorder) = &self.voice_rec.borrow().phase {
                    recorder.advance_synthetic(ms.parse().unwrap_or(0));
                }
                cx.notify();
                format!("voice: advanced {ms} ms")
            }
            ["stop"] => self.voice_record_stop(cx),
            ["cancel"] => self.voice_record_discard(cx),
            ["send"] => self.voice_record_send(cx),
            ["play"] | ["pause"] => {
                let target = {
                    let state = self.voice_rec.borrow();
                    match (state.channel, &state.phase) {
                        (Some(channel), RecPhase::Recorded(recording)) => {
                            Some((RecState::draft_key(channel), recording.duration_secs))
                        }
                        _ => None,
                    }
                };
                match target {
                    Some((key, seconds)) => {
                        self.voice_hub
                            .borrow_mut()
                            .toggle_if(&key, seconds, words[0] == "play");
                        cx.notify();
                        format!("voice: {} draft", words[0])
                    }
                    None => "voice: no draft".to_owned(),
                }
            }
            ["state"] => {
                let state = self.voice_rec.borrow();
                let phase = match &state.phase {
                    RecPhase::Idle => "idle",
                    RecPhase::Recording(_) => "recording",
                    RecPhase::Recorded(_) => "recorded",
                };
                format!(
                    "voice: rec {phase} gate {} channel {:?} elapsed {} ms",
                    self.voice_recording_enabled(),
                    state.channel,
                    state.elapsed_ms()
                )
            }
            ["save", path] => match &self.voice_rec.borrow().phase {
                RecPhase::Recorded(recording) => match std::fs::write(path, &recording.file) {
                    Ok(()) => format!("voice: draft saved, {} bytes", recording.file.len()),
                    Err(error) => format!("voice: cannot save: {error}"),
                },
                _ => "voice: no draft".to_owned(),
            },
            _ => "voice: usage `vm rec on|off|start|advance <ms>|stop|cancel|send|play|pause|state|save <path>`"
                .to_owned(),
        }
    }

    fn rec_button(
        &self,
        id: &'static str,
        (tip_key, tip_label): (&'static str, &'static str),
        (icon_name, tint): (&'static str, &'static str),
        cx: &mut gpui::Context<Self>,
        action: impl Fn(&mut Self, &mut gpui::Context<Self>) + 'static,
    ) -> AnyElement {
        let button = div()
            .id(id)
            .parity(id)
            .size(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            // The composer's own buttons (`.button_hfM7yP`): 8px corners, selected background on hover.
            .rounded(px(8.))
            .text_color(color(tint))
            .hover(|style| style.bg(color("--interactive-background-selected")))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.tip = None;
                action(this, cx);
            }))
            .child(icon(icon_name, 20., tint));
        self.with_tip(
            tip_key,
            super::Tip::text(tip_label, super::Side::Top),
            button,
            cx,
        )
        .into_any_element()
    }

    /// `.sendButton_bWT6VE` (`550929.css`): `32x32`, `padding: 4px`, transparent background, `20x20` send icon in
    /// `--text-brand` (hover `--brand-430`, disabled `--icon-muted`).
    fn rec_send_button(&self, disabled: bool, cx: &mut gpui::Context<Self>) -> AnyElement {
        let button = div()
            .id("voice-rec-send")
            .parity("voice-rec-send")
            .size(px(32.))
            .p(px(4.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .when(disabled, |style| {
                style.cursor_not_allowed().text_color(color("--icon-muted"))
            })
            .when(!disabled, |style| {
                style
                    .cursor_pointer()
                    .text_color(color("--text-brand"))
                    .hover(|hover| hover.text_color(color("--brand-430")))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tip = None;
                        this.voice_record_send(cx);
                    }))
            })
            .child(
                gpui::svg()
                    .path("icons/send_message.svg")
                    .size(px(20.))
                    .flex_none(),
            );
        self.with_tip(
            "voice-rec-send-tip",
            super::Tip::text(dc_intl::t!("TXNS7S"), super::Side::Top),
            button,
            cx,
        )
        .into_any_element()
    }

    /// The composer's bar while a recording or its draft belongs to this channel; `None` shows the usual editor row.
    pub(super) fn voice_bar(&self, cx: &mut gpui::Context<Self>) -> Option<AnyElement> {
        let channel = self.channel?;
        let (recording, elapsed) = {
            let state = self.voice_rec.borrow();
            if !state.active_in(channel) {
                return None;
            }
            (
                matches!(state.phase, RecPhase::Recording(_)),
                state.elapsed_ms(),
            )
        };
        if recording && elapsed >= voice_wave::MAX_RECORDING_MS {
            // Reaching the limit stops the recording and keeps it as the draft.
            let phase = std::mem::replace(&mut self.voice_rec.borrow_mut().phase, RecPhase::Idle);
            if let RecPhase::Recording(recorder) = phase
                && let Ok(recording) = recorder.stop()
            {
                if let Ok(clip) = Clip::from_ogg(&recording.file) {
                    self.voice_hub
                        .borrow_mut()
                        .preload(&RecState::draft_key(channel), Arc::new(clip));
                }
                self.voice_rec.borrow_mut().phase = RecPhase::Recorded(recording);
            }
            return self.voice_bar(cx);
        }
        let blink_off = recording
            && voice_wave::timer_blink_interval_ms(elapsed, motion::reduced())
                .is_some_and(|interval| ((motion::now().max(0.) as u64) / interval) % 2 == 1);
        if recording {
            motion::keep_animating();
        }
        let timer_tone = if recording && elapsed >= voice_wave::WARN_MS {
            "--text-feedback-critical"
        } else {
            "--text-default"
        };
        let timer = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .when(blink_off, |this| this.opacity(0.))
            .when(recording, |this| {
                // The record dot, `--text-feedback-critical`, 4x4.
                this.child(
                    div()
                        .size(px(4.))
                        .rounded_full()
                        .bg(color("--text-feedback-critical")),
                )
            })
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight::MEDIUM)
                    .font_features(tnum())
                    .text_color(color(timer_tone))
                    .child(voice_wave::format_time(elapsed as f64 / 1000.)),
            );
        let lead = if recording {
            self.rec_button(
                "voice-rec-stop",
                ("voice-rec-stop-tip", dc_intl::t!("hHBkuG")),
                ("vm_stop", "--text-feedback-critical"),
                cx,
                |this, cx| {
                    this.voice_record_stop(cx);
                },
            )
        } else {
            let key = RecState::draft_key(channel);
            let seconds = elapsed as f64 / 1000.;
            let status = voice_player::engine().status();
            let playing = status.key.as_deref() == Some(key.as_str()) && status.playing;
            self.rec_button(
                "voice-rec-play",
                (
                    "voice-rec-play-tip",
                    if playing {
                        dc_intl::t!("ZcgDJX")
                    } else {
                        dc_intl::t!("RscU7I")
                    },
                ),
                (
                    if playing { "clips_pause" } else { "clips_play" },
                    "--interactive-text-active",
                ),
                cx,
                move |this, cx| {
                    this.voice_hub.borrow_mut().toggle(&key, "", seconds);
                    cx.notify();
                },
            )
        };
        let wave = if recording {
            self.rec_live_wave()
        } else {
            let key = RecState::draft_key(channel);
            let seconds = elapsed as f64 / 1000.;
            let view = self.voice_hub.borrow_mut().view(&key, seconds);
            let levels = match &self.voice_rec.borrow().phase {
                RecPhase::Recorded(recording) => voice_wave::decode_waveform(&recording.waveform),
                _ => None,
            };
            self.voice_wave(WaveSpec {
                key: &key,
                salt: 9_000_000,
                duration: seconds,
                waveform: levels,
                width: 0.,
                played: view.played,
                playing: view.playing,
                current: view.current,
                fluid: true,
                neutral: true,
            })
        };
        // `.divider_bWT6VE`: `1px x 22px` `--border-subtle`.
        let divider = div()
            .w(px(1.))
            .h(px(22.))
            .flex_none()
            .bg(color("--border-subtle"));
        let send_disabled = self.voice_recording_finalizing()
            || self.current_channel().is_some_and(|known| {
                !self.can_in_channel(known, dc_state::permissions::SEND_VOICE_MESSAGES)
            });
        Some(
            div()
                .parity("voice-rec-bar")
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(56.))
                .pl(px(12.))
                .pr(px(6.))
                .child(lead)
                .child(wave)
                .child(timer)
                .child(divider)
                .child(self.rec_button(
                    "voice-rec-trash",
                    ("voice-rec-trash-tip", dc_intl::t!("oyYWHE")),
                    ("clips_trash", "--interactive-text-default"),
                    cx,
                    |this, cx| {
                        this.voice_record_discard(cx);
                    },
                ))
                .child(self.rec_send_button(send_disabled, cx))
                .into_any_element(),
        )
    }

    /// The live waveform (`canvas_6oHqIH` in module 115921): one bar every 6 px, 2 px wide, radius 1, height
    /// `(24 - 2) * v + 2`, `--interactive-text-active`, fading smoothly from `0.0` at left `0%` to `1.0` at `12%`.
    fn rec_live_wave(&self) -> AnyElement {
        let state = self.voice_rec.clone();
        div()
            .flex_1()
            .min_w(px(0.))
            .h(px(24.))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let width = f32::from(bounds.size.width);
                        let count = voice_wave::bar_count(width);
                        let now = motion::now();
                        let mut state = state.borrow_mut();
                        let levels = match &state.phase {
                            RecPhase::Recording(recorder) => recorder.levels(),
                            _ => Vec::new(),
                        };
                        let mut target = vec![0.; count];
                        for (slot, level) in target.iter_mut().rev().zip(levels.iter().rev()) {
                            *slot = f32::from(*level) / 255.;
                        }
                        if state.shown.len() != count {
                            state.shown = vec![0.; count];
                        }
                        let mut steps = 0;
                        while now - state.last_step >= REC_BAR_STEP_MS && steps < 6 {
                            state.last_step += REC_BAR_STEP_MS;
                            steps += 1;
                            for (shown, goal) in state.shown.iter_mut().zip(&target) {
                                *shown += (goal - *shown) * 0.45;
                            }
                        }
                        if now - state.last_step >= REC_BAR_STEP_MS {
                            state.last_step = now;
                        }
                        let base = color("--interactive-text-active");
                        for (index, value) in state.shown.iter().enumerate() {
                            let height = voice_wave::bar_height(value.clamp(0., 1.), true);
                            let x = 6. * index as f32;
                            let fade = voice_wave::live_bar_fade(x, width);
                            let rect = Bounds::new(
                                point(
                                    bounds.origin.x + px(x),
                                    bounds.origin.y + px(12. - height / 2.),
                                ),
                                size(px(2.), px(height)),
                            );
                            window.paint_quad(
                                fill(
                                    rect,
                                    Rgba {
                                        a: base.a * fade,
                                        ..base
                                    },
                                )
                                .corner_radii(px(1.)),
                            );
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    fn audio_response(
        body: Vec<u8>,
        declared_size: Option<usize>,
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/audio.wav", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 16_384);
            }
            let length = declared_size
                .map_or_else(String::new, |size| format!("Content-Length: {size}\r\n"));
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nConnection: close\r\n{length}\r\n"
            )
            .unwrap();
            // A rejected header or body limit closes the connection before all bytes are written.
            let _ = stream.write_all(&body);
        });
        (url, server)
    }

    #[test]
    fn attachment_audio_over_two_mib_downloads_and_decodes_without_the_ui_sound_limit() {
        let data_size = 3 * 1024 * 1024u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_size).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&96_000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        wav.resize(44 + data_size as usize, 0);
        let length = wav.len();
        let (url, server) = audio_response(wav.clone(), Some(length));
        assert_eq!(
            crate::sounds::fetch_url(&url).unwrap_err(),
            "file too large"
        );
        server.join().unwrap();

        let (url, server) = audio_response(wav, Some(length));
        let bytes = fetch(&url).expect("attachment HTTP path accepts more than 2 MiB");
        server.join().unwrap();
        assert_eq!(bytes.len(), length);
        let clip = Clip::from_file(bytes)
            .expect("the downloaded WAV decodes without opening audio hardware");
        assert!((clip.seconds() - f64::from(data_size) / 96_000.).abs() < 0.001);
    }

    #[test]
    fn attachment_audio_checks_declared_and_streamed_download_limits() {
        let (url, server) = audio_response(Vec::new(), Some(25 * 1024 * 1024 + 1));
        assert_eq!(fetch(&url).unwrap_err(), "file too large");
        server.join().unwrap();
        for (length, accepted) in [(8, true), (9, false)] {
            let (url, server) = audio_response(vec![7; length], None);
            let result = crate::sounds::fetch_url_limited(&url, 8);
            server.join().unwrap();
            if accepted {
                assert_eq!(result.unwrap(), vec![7; length]);
            } else {
                assert_eq!(result.unwrap_err(), "file too large");
            }
        }
    }

    #[test]
    fn attachment_audio_m4a_over_two_mib_reaches_the_aac_decoder() {
        // Synthetic 0.25s mono silence, generated with FFmpeg's anullsrc at 48kHz,
        // AAC 64k, faststart and bitexact flags. No captured microphone or other user media.
        let fixture = include_bytes!("../../../tests/fixtures/audio_silence.m4a");
        let expected = Clip::from_file(fixture.to_vec()).expect("synthetic AAC fixture");
        let mut bytes = fixture.to_vec();
        // A valid ISO BMFF free box exercises the HTTP limit without a large checked-in fixture.
        let padding = 3 * 1024 * 1024u32;
        bytes.extend_from_slice(&(padding + 8).to_be_bytes());
        bytes.extend_from_slice(b"free");
        bytes.resize(bytes.len() + padding as usize, 0);
        let length = bytes.len();
        let (url, server) = audio_response(bytes, Some(length));
        let downloaded = fetch(&url).expect("large M4A HTTP body");
        server.join().unwrap();
        assert_eq!(downloaded.len(), length);
        let clip = Clip::from_file(downloaded).expect("AAC decoder after HTTP fetch");
        assert_eq!(clip.len(), expected.len());
        assert!(!clip.is_empty());
    }

    #[test]
    fn the_stored_volume_reads_as_the_official_store_does() {
        use super::volume_of;
        use serde_json::json;
        assert_eq!(volume_of(None), (1., false));
        assert_eq!(
            volume_of(Some(&json!({"volume": 0.25, "muted": true}))),
            (0.25, true)
        );
        assert_eq!(
            volume_of(Some(&json!({"volume": 7, "muted": "yes"}))),
            (1., false)
        );
        assert_eq!(volume_of(Some(&json!({"volume": -3}))), (0., false));
        assert_eq!(volume_of(Some(&json!({"volume": "loud"}))), (1., false));
    }

    use super::*;
    use gpui::{Bounds, point, px, size};

    fn bounds() -> Bounds<Pixels> {
        Bounds::new(point(px(100.), px(40.)), size(px(60.), px(24.)))
    }

    #[test]
    fn the_pointer_across_the_waveform_is_a_share_of_its_width() {
        assert_eq!(fraction_of(px(100.), bounds()), 0.);
        assert_eq!(fraction_of(px(130.), bounds()), 0.5);
        assert_eq!(fraction_of(px(400.), bounds()), 1.);
        assert_eq!(fraction_of(px(-5.), bounds()), 0.);
    }

    #[test]
    fn the_volume_rail_reads_bottom_up() {
        let track = Bounds::new(point(px(0.), px(10.)), size(px(6.), px(40.)));
        assert_eq!(position_of(px(50.), track), 0.);
        assert_eq!(position_of(px(10.), track), 1.);
        assert!((position_of(px(30.), track) - 0.5).abs() < 1e-6);
    }

    /// Replay of a recorded voice message: the fixture file decodes to the length its message payload states.
    #[test]
    fn the_recorded_fixture_plays_as_long_as_it_says() {
        let bytes = include_bytes!("../../../tests/fixtures/voice_message.ogg");
        let clip = Clip::from_ogg(bytes).expect("decodes");
        assert!((clip.seconds() - 6.0).abs() < 0.05, "{}", clip.seconds());
    }

    #[test]
    fn a_pill_plays_pauses_and_seeks_through_the_hub() {
        voice_player::set_silent(true);
        let bytes = include_bytes!("../../../tests/fixtures/voice_message.ogg");
        let clip = Arc::new(Clip::from_ogg(bytes).expect("decodes"));
        let mut hub = Hub::new();
        hub.preload("t:1", clip);
        hub.toggle("t:1", "", 6.);
        // The engine runs on its own thread: the command is taken up a moment after `play`.
        let mut status = voice_player::engine().status();
        for _ in 0..200 {
            if status.key.as_deref() == Some("t:1") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            status = voice_player::engine().status();
        }
        assert_eq!(status.key.as_deref(), Some("t:1"));
        hub.seek("t:1", 0.5, 6.);
        assert!((hub.pill("t:1").cached - 3.).abs() < 1e-9);
        hub.cycle_speed();
        assert_eq!(hub.speed, 1.5);
        hub.toggle_if("t:1", 6., false);
        assert!(!voice_player::engine().status().playing || hub.pill("t:1").played);
    }

    #[test]
    fn neutral_draft_waveform_keeps_neutral_tokens_and_interpolates_fluidly() {
        assert_eq!(
            wave_color_tokens(false, false, true),
            (
                "--background-mod-muted",
                "--interactive-text-default",
                "--interactive-text-default"
            )
        );
        assert_eq!(
            wave_color_tokens(true, true, true),
            (
                "--background-mod-muted",
                "--interactive-text-active",
                "--interactive-text-default"
            )
        );
        assert_eq!(
            wave_color_tokens(false, true, true),
            (
                "--background-mod-muted",
                "--interactive-text-active",
                "--interactive-text-default"
            )
        );
        assert_eq!(
            wave_color_tokens(true, true, false),
            ("--brand-430", "--white", "--brand-430")
        );

        let full_composer_bars = voice_wave::bar_count(596.);
        assert_eq!(full_composer_bars, 100);
        let stretched = voice_wave::bars(Some(&[0.2, 0.8]), full_composer_bars, true);
        assert_eq!(stretched.len(), 100);
        assert!((stretched[0] - 0.2).abs() < 1e-6);
        assert!((stretched[99] - 0.8).abs() < 1e-6);
        assert!(stretched[50] > 0.45 && stretched[50] < 0.55);
    }

    #[test]
    fn recorder_state_tracks_channel_draft_and_synthetic_stop() {
        let mut rec = RecState::new();
        assert!(!rec.active_in(42));
        assert!(!rec.finalizing);
        assert_eq!(RecState::draft_key(42), "voice-draft:42");

        let recorder = Recorder::start_synthetic().expect("synthetic recorder");
        recorder.advance_synthetic(1_500);
        rec.channel = Some(42);
        rec.phase = RecPhase::Recording(recorder);
        assert!(rec.active_in(42));
        assert!(!rec.active_in(99));
        assert_eq!(rec.elapsed_ms(), 1_500);

        let RecPhase::Recording(recorder) = std::mem::replace(&mut rec.phase, RecPhase::Idle)
        else {
            panic!("expected recording");
        };
        let recording = recorder.stop().expect("stop synthetic");
        assert!(!recording.too_short());
        rec.phase = RecPhase::Recorded(recording);
        assert!(rec.active_in(42));
        assert!(rec.elapsed_ms() >= 1_400);
    }
}
