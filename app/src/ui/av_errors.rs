//! The AV warnings over a stream's and a camera's tile: the error manager of module 822209 over the accumulated
//! statistics of `MediaEngineStatsStore` (module 624694), and the warning icon of module 93067.
//!
//! - Statistics: each sample of a connection's video (packets sent or received, packets lost, the encode or decode
//!   frame rate) is kept, up to 31; the short window is the last 15 samples against the one before them (`E(i, 15)`),
//!   the long window the last 30 against the one that left (`s[i].shift()`). A window's frame rate is the mean of its
//!   samples', its loss the lost packets over sent-or-received plus lost. A rule reads a window only once both have 15
//!   samples (`F`).
//! - Rules (`q`): watching a stream (not our own, not paused), `stream-view-low-fps` when either window's frame rate is
//!   under `B` of the stream's maximum frame rate (3, 8, 15 or 30) once incoming video has been on for 10 seconds,
//!   `stream-view-high-packet-loss` over 10% loss; our own stream with viewers, `stream-send-low-encode-fps` under `B`
//!   of what we send; our camera, `camera-send-low-encode-fps` under 10 frames a second. A stream's warning is the
//!   first of `r` in module 821715 (sound share, packet loss, frame rate, network).
//! - The icon (module 93067, `root_0_PsGx`): the 20px CircleErrorIcon in `--yellow-300`; over it (or focused) a 200px
//!   popout under it (`popout_0_PsGx`: `--background-surface-high`, 8px 12px, the inset 1px `--border-subtle` and
//!   the high shadow) with the message (275731 for a stream, 289552 for our camera) and "Error: {code}" linking the
//!   AV error codes article, 8px under it; it closes 250 ms after the pointer leaves both.
//!
//! GAP: the media engine's sampling interval is not in the bundle (the native engine emits `connection-stats`); the
//! statistics are sampled once a second here. The sink-wants checks of the two send rules (`hasActiveRemoteWants`, no
//! wants within 20 seconds) are taken as met while anyone watches; `stream-send-high-packet-loss`,
//! `stream-send-network-quality` and `stream-soundshare-failed` are not raised (the sent video's lost packets, the
//! stream connection's quality and the sound-share hook error are not reported by `dc-voice`).

use super::{Root, color, icon};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// `w`/`x`: incoming video must have been on this long.
const INCOMING_SETTLE: Duration = Duration::from_secs(10);
/// `F`'s default `n`.
const MIN_POINTS: usize = 15;
/// `AV_ERROR_CODES` ("30952914470807", module 652215) through `getArticleURL`.
const CODES_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/30952914470807";

/// One sample of a connection's video.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Sample {
    pub packets: u64,
    pub lost: u64,
    pub fps: f32,
}

/// A window's accumulated statistics (`c` of module 624694).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Accumulated {
    pub frame_rate: f32,
    pub loss_rate: f32,
    pub points: usize,
}

/// The last `n` samples against the one before them.
pub(super) fn window(history: &VecDeque<Sample>, n: usize) -> Option<Accumulated> {
    let newest = history.back()?;
    let (old, points): (Option<&Sample>, Vec<&Sample>) = if history.len() > n {
        (
            history.get(history.len() - 1 - n),
            history.iter().skip(history.len() - n).collect(),
        )
    } else {
        (None, history.iter().collect())
    };
    let (packets, lost) = match old {
        Some(old) => (
            newest.packets.saturating_sub(old.packets),
            newest.lost.saturating_sub(old.lost),
        ),
        None => (newest.packets, newest.lost),
    };
    let frame_rate = points.iter().map(|s| s.fps).sum::<f32>() / points.len() as f32;
    let total = packets + lost;
    Some(Accumulated {
        frame_rate,
        loss_rate: if total == 0 {
            0.
        } else {
            lost as f32 / total as f32
        },
        points: points.len(),
    })
}

/// `F`: both windows, once each has 15 samples.
pub(super) fn windows(history: &VecDeque<Sample>) -> Option<(Accumulated, Accumulated)> {
    let short = window(history, 15)?;
    let long = window(history, 30)?;
    (short.points >= MIN_POINTS && long.points >= MIN_POINTS).then_some((short, long))
}

/// `B`: the frame rate under which a stream of `max` frames a second counts as low.
pub(super) fn low_fps_floor(max: u32) -> f32 {
    match max {
        0..=5 => 3.,
        6..=15 => 8.,
        16..=30 => 15.,
        _ => 30.,
    }
}

/// The warnings this client raises (`iy` of module 487329) with their codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AvError {
    StreamViewHighPacketLoss,
    StreamViewLowFps,
    StreamSendLowFps,
    CameraSendLowFps,
}

impl AvError {
    pub fn code(self) -> u32 {
        match self {
            AvError::StreamViewLowFps => 2002,
            AvError::StreamViewHighPacketLoss => 2003,
            AvError::StreamSendLowFps => 2005,
            AvError::CameraSendLowFps => 2013,
        }
    }

    /// `r` of module 821715: packet loss before frame rate.
    fn rank(self) -> u8 {
        match self {
            AvError::StreamViewHighPacketLoss => 1,
            _ => 2,
        }
    }

    /// The message of module 275731 or 289552.
    fn message(self) -> &'static str {
        match self {
            AvError::StreamSendLowFps => dc_intl::t("1f1LHp"),
            AvError::StreamViewHighPacketLoss | AvError::StreamViewLowFps => dc_intl::t("BcOifw"),
            AvError::CameraSendLowFps => dc_intl::t("1NyoVt"),
        }
    }
}

/// The rules for a watched stream.
pub(super) fn view_errors(
    windows: Option<(Accumulated, Accumulated)>,
    max_fps: Option<u32>,
    incoming_settled: bool,
) -> Vec<AvError> {
    let mut out = Vec::new();
    let Some((short, long)) = windows else {
        return out;
    };
    if incoming_settled && let Some(max) = max_fps {
        let floor = low_fps_floor(max);
        if short.frame_rate < floor || long.frame_rate < floor {
            out.push(AvError::StreamViewLowFps);
        }
    }
    if incoming_settled && (short.loss_rate * 100. > 10. || long.loss_rate * 100. > 10.) {
        out.push(AvError::StreamViewHighPacketLoss);
    }
    out
}

/// The first warning of a stream by module 821715's order.
pub(super) fn stream_warning(errors: &[AvError]) -> Option<AvError> {
    errors.iter().copied().min_by_key(|error| error.rank())
}

/// The histories and the warnings they raise.
#[derive(Default)]
pub(super) struct AvErrors {
    /// By owner of the watched or sent stream (0 is our camera).
    history: HashMap<(u64, bool), VecDeque<Sample>>,
    /// When each watched stream's video began.
    since: HashMap<u64, Instant>,
    /// The warning of each stream owner, and of our camera.
    pub streams: HashMap<u64, AvError>,
    pub camera: Option<AvError>,
    sampling: bool,
    /// The icon whose popout is open, and when the pointer left it (the 250 ms close delay).
    hovered: Option<(String, Option<Instant>)>,
}

fn push(history: &mut VecDeque<Sample>, sample: Sample) {
    history.push_back(sample);
    while history.len() > 31 {
        history.pop_front();
    }
}

impl Root {
    /// Starts the once-a-second sampling while in a call.
    pub(super) fn av_errors_frame(&mut self, cx: &mut Context<Self>) {
        if self.call.is_none() || self.av_errors.sampling {
            return;
        }
        self.av_errors.sampling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let going = this
                    .update(cx, |this, cx| {
                        if this.call.is_none() {
                            this.av_errors = Default::default();
                            return false;
                        }
                        this.av_errors_sample();
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !going {
                    break;
                }
            }
        })
        .detach();
    }

    fn av_errors_sample(&mut self) {
        let Some(call) = self.call.as_ref() else {
            return;
        };
        let snapshot = call.controls.rtc_stats();
        let me = self.stores.current_user.as_ref().map_or(0, |me| me.id);
        let now = Instant::now();
        let mut streams = HashMap::new();
        for stream in &snapshot.streams {
            let owner = stream.owner_id;
            if stream.sending {
                let Some(video) = stream.outbound.video.as_ref() else {
                    continue;
                };
                let viewers = self
                    .video_stream_of(owner)
                    .is_some_and(|info| !info.viewer_ids.is_empty());
                let max = self.own_started().map(|started| started.quality.1);
                let history = self.av_errors.history.entry((owner, true)).or_default();
                push(
                    history,
                    Sample {
                        packets: video.packets_sent,
                        lost: 0,
                        fps: video.fps_encoded as f32,
                    },
                );
                if viewers && let (Some((short, long)), Some(max)) = (windows(history), max) {
                    let floor = low_fps_floor(max);
                    if short.frame_rate < floor || long.frame_rate < floor {
                        streams.insert(owner, AvError::StreamSendLowFps);
                    }
                }
            } else if owner != me {
                let Some(video) = stream
                    .inbound
                    .get(&owner)
                    .and_then(|user| user.video.as_ref())
                else {
                    continue;
                };
                let max = self.video_stream_max_fps(owner);
                let since = *self.av_errors.since.entry(owner).or_insert(now);
                let history = self.av_errors.history.entry((owner, false)).or_default();
                push(
                    history,
                    Sample {
                        packets: video.packets_received,
                        lost: video.packets_lost,
                        fps: video.fps_decoded as f32,
                    },
                );
                let errors = view_errors(
                    windows(history),
                    max,
                    now.duration_since(since) > INCOMING_SETTLE,
                );
                if let Some(error) = stream_warning(&errors) {
                    streams.insert(owner, error);
                }
            }
        }
        // Our camera: under 10 frames a second in either window.
        let camera = call
            .camera
            .then_some(())
            .and(snapshot.outbound.video.as_ref());
        self.av_errors.camera = None;
        match camera {
            Some(video) => {
                let history = self.av_errors.history.entry((0, true)).or_default();
                push(
                    history,
                    Sample {
                        packets: video.packets_sent,
                        lost: 0,
                        fps: video.fps_encoded as f32,
                    },
                );
                if let Some((short, long)) = windows(history)
                    && (short.frame_rate < 10. || long.frame_rate < 10.)
                {
                    self.av_errors.camera = Some(AvError::CameraSendLowFps);
                }
            }
            None => {
                self.av_errors.history.remove(&(0, true));
            }
        }
        let live: Vec<u64> = snapshot.streams.iter().map(|s| s.owner_id).collect();
        self.av_errors
            .history
            .retain(|(owner, _), _| *owner == 0 || live.contains(owner));
        self.av_errors.since.retain(|owner, _| live.contains(owner));
        self.av_errors.streams = streams;
    }

    /// The most frames a second a watched stream says it sends (`maxFrameRate` of its quality).
    fn video_stream_max_fps(&self, owner: u64) -> Option<u32> {
        self.video_stream_of(owner)
            .and_then(|info| info.label)
            .and_then(|label| {
                label
                    .split_whitespace()
                    .find_map(|part| part.strip_suffix("FPS").and_then(|n| n.parse().ok()))
            })
    }

    /// The warning icon of a stream's (or our camera's, `owner` 0) tile or header, when it has a warning.
    pub(super) fn av_warning_icon(
        &self,
        owner: u64,
        place: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let error = if owner == 0 {
            self.av_errors.camera
        } else {
            self.av_errors.streams.get(&owner).copied()
        }?;
        let key = format!("av-warning-{place}-{owner}");
        let open = self.av_errors.hovered.as_ref().is_some_and(|(open, left)| {
            *open == key && left.is_none_or(|left| left.elapsed() < Duration::from_millis(250))
        });
        let hover_key = key.clone();
        let code = error.code();
        let message = error.message();
        let line = dc_intl::format_runs(
            "If5Q0h",
            &[
                ("errorCode", (code as f64).into()),
                ("helpDeskURL", CODES_ARTICLE.into()),
            ],
        );
        let line: String = line.into_iter().map(|(run, _)| run).collect();
        let popout = open.then(|| {
            div()
                .absolute()
                .top(px(28.))
                .left(px(-90.))
                .w(px(200.))
                .py(px(8.))
                .px(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(message)
                .child(
                    div()
                        .id(SharedString::from(format!("{key}-code")))
                        .mt(px(8.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-muted"))
                        .cursor_pointer()
                        .child(line)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.follow_link(CODES_ARTICLE, "", cx);
                        })),
                )
        });
        Some(
            div()
                .id(SharedString::from(key.clone()))
                .relative()
                .size(px(20.))
                .flex_none()
                .aria_label(format!("{message} Error: {code}"))
                .child(icon("shop_circle_error", 20., "--yellow-300"))
                .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                    if *inside {
                        this.av_errors.hovered = Some((hover_key.clone(), None));
                    } else if let Some((open, left)) = this.av_errors.hovered.as_mut()
                        && *open == hover_key
                    {
                        *left = Some(Instant::now());
                        cx.spawn(async move |this, cx| {
                            cx.background_executor()
                                .timer(Duration::from_millis(260))
                                .await;
                            let _ = this.update(cx, |_, cx| cx.notify());
                        })
                        .detach();
                    }
                    cx.notify();
                }))
                .children(popout)
                .into_any_element(),
        )
    }

    /// Capture only: the owner of the focused tile.
    #[cfg(feature = "capture")]
    pub(crate) fn focused_tile_owner(&self) -> Option<u64> {
        self.vv.focus.tile.map(|(owner, _)| owner)
    }

    /// Capture only: `av state` names the warnings, `av force <owner|camera> <code>` raises one by hand.
    #[cfg(feature = "capture")]
    pub(crate) fn script_av(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        match words {
            ["state"] => format!(
                "streams {:?} camera {:?}",
                self.av_errors.streams.values().collect::<Vec<_>>(),
                self.av_errors.camera
            ),
            ["force", who, code] => {
                let error = match *code {
                    "2002" => AvError::StreamViewLowFps,
                    "2003" => AvError::StreamViewHighPacketLoss,
                    "2005" => AvError::StreamSendLowFps,
                    "2013" => AvError::CameraSendLowFps,
                    _ => return "codes: 2002 2003 2005 2013".to_owned(),
                };
                if *who == "camera" {
                    self.av_errors.camera = Some(error);
                } else if let Ok(owner) = who.parse::<u64>() {
                    self.av_errors.streams.insert(owner, error);
                }
                cx.notify();
                "forced".to_owned()
            }
            _ => "usage: av state | av force <owner|camera> <code>".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(samples: &[(u64, u64, f32)]) -> VecDeque<Sample> {
        samples
            .iter()
            .map(|&(packets, lost, fps)| Sample { packets, lost, fps })
            .collect()
    }

    #[test]
    fn windows_need_fifteen_samples_and_compare_against_the_sample_before() {
        let few = history(&[(10, 0, 30.); 14]);
        assert_eq!(windows(&few), None);
        let samples: Vec<(u64, u64, f32)> = (1..=31).map(|i| (i * 100, i * 2, 30.)).collect();
        let full = history(&samples);
        let (short, long) = windows(&full).unwrap();
        assert_eq!((short.points, long.points), (15, 30));
        // 1500 packets and 30 lost in the short window: under 2%.
        assert!((short.loss_rate - 30. / 1530.).abs() < 1e-6);
        assert!((long.loss_rate - 60. / 3060.).abs() < 1e-6);
    }

    #[test]
    fn view_rules_follow_the_floors_and_the_ten_percent_loss() {
        assert_eq!(low_fps_floor(5), 3.);
        assert_eq!(low_fps_floor(15), 8.);
        assert_eq!(low_fps_floor(30), 15.);
        assert_eq!(low_fps_floor(60), 30.);
        let ok = Accumulated {
            frame_rate: 29.,
            loss_rate: 0.01,
            points: 15,
        };
        let slow = Accumulated {
            frame_rate: 10.,
            ..ok
        };
        let lossy = Accumulated {
            loss_rate: 0.2,
            ..ok
        };
        assert!(view_errors(Some((ok, ok)), Some(30), true).is_empty());
        assert_eq!(
            view_errors(Some((slow, ok)), Some(30), true),
            vec![AvError::StreamViewLowFps]
        );
        // Not before incoming video has been on for ten seconds.
        assert!(view_errors(Some((slow, lossy)), Some(30), false).is_empty());
        let both = view_errors(Some((slow, lossy)), Some(30), true);
        assert_eq!(
            stream_warning(&both),
            Some(AvError::StreamViewHighPacketLoss)
        );
    }
}
