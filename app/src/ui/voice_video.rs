//! Video of the call and Go Live streams: the camera button, the pictures `dc-voice` decodes, and the life of a stream
//! (gateway ops 18 to 22 and the four `STREAM_*` dispatches, then the stream's own voice connection).
//!
//! Pictures are kept as the latest one per (user, source). The sink runs on decoder threads and only stores; a task
//! on the UI side repaints at most thirty times a second when something new arrived.

use super::Root;
use crate::net::Request;
use dc_proto::op::Op;
use dc_voice::{
    RemoteFrame, StreamControls, StreamEvent, StreamSource, VideoSource,
    streams::{self, Dispatch, EndReason, Role, StreamKey, StreamStatus, StreamUpdate, Tracker},
};
use gpui::Context;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;
use tracing::{info, warn};

/// The most repaints per second the pictures cause.
const REPAINTS_PER_SECOND: u64 = 30;
/// A capture-only stream (the harness's `stream start`) ends by itself after this long.
#[cfg(feature = "capture")]
const HARNESS_STREAM_LIMIT: Duration = Duration::from_secs(10);
/// The lowest settings of the synthetic source: a 320 x 180 pattern at 5 frames a second.
#[cfg(feature = "capture")]
const HARNESS_SOURCE: (u32, u32, u32, u32) = (320, 180, 5, 150_000);

/// The latest picture of every source, written by decoder threads.
#[derive(Default)]
pub(super) struct Frames {
    latest: Mutex<HashMap<(u64, VideoSource), dc_media::Picture>>,
    dirty: AtomicBool,
    seen: AtomicU64,
}

impl Frames {
    fn store(&self, frame: RemoteFrame) {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((frame.user, frame.source), frame.picture);
        self.seen.fetch_add(1, Ordering::Relaxed);
        self.dirty.store(true, Ordering::Release);
    }

    /// A picture arrived since the last call.
    fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    fn clear(&self) {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.dirty.store(true, Ordering::Release);
    }

    /// The picture a tile paints.
    #[cfg(test)]
    fn latest(&self, user: u64, source: VideoSource) -> Option<dc_media::Picture> {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(user, source))
            .cloned()
    }
}

/// One stream this client sends or watches.
struct Live {
    tracker: Tracker,
    controls: StreamControls,
    /// What a sender captures; taken when the connection starts.
    source: Option<StreamSource>,
    last: StreamUpdate,
}

#[derive(Default)]
pub(super) struct Video {
    frames: Arc<Frames>,
    pumping: bool,
    streams: HashMap<StreamKey, Live>,
}

impl Video {
    /// `describe` for capture logs: pictures received so far and the streams with their status.
    #[cfg(feature = "capture")]
    pub(super) fn describe(&self) -> String {
        let mut streams: Vec<String> = self
            .streams
            .values()
            .map(|live| {
                format!(
                    "{:?}/{:?}/viewers={}",
                    live.last.role, live.last.status, live.last.viewers
                )
            })
            .collect();
        streams.sort();
        format!(
            "frames={} streams=[{}]",
            self.frames.seen.load(Ordering::Relaxed),
            streams.join(",")
        )
    }
}

impl Root {
    /// A call was created: pictures of it go to the store, and the repaint task runs while the call lasts.
    pub(super) fn video_call_started(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &self.call else {
            return;
        };
        let frames = self.video.frames.clone();
        frames.clear();
        call.controls
            .set_video_sink(Some(Arc::new(move |frame| frames.store(frame))));
        if self.video.pumping {
            return;
        }
        self.video.pumping = true;
        let gap = Duration::from_millis(1000 / REPAINTS_PER_SECOND);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(gap).await;
                let alive = this.update(cx, |root, cx| {
                    if root.call.is_none() {
                        root.video.pumping = false;
                        return false;
                    }
                    if root.video.frames.take_dirty() {
                        cx.notify();
                    }
                    true
                });
                if !alive.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The camera button of the panel and the bar: turns our camera on with the chosen device (`audio.video_device`,
    /// else the first one; an empty id lets the call pick the default), or off.
    pub(super) fn voice_camera_clicked(&mut self, cx: &mut Context<Self>) {
        let Some(call) = &self.call else {
            return;
        };
        let controls = &call.controls;
        if controls.camera_wanted() {
            controls.set_camera(None);
        } else {
            let device = crate::storage::get("audio.video_device")
                .and_then(|value| value.as_str().map(str::to_owned))
                .filter(|id| !id.is_empty())
                .or_else(|| dc_voice::camera_devices().into_iter().next().map(|d| d.id))
                .unwrap_or_default();
            controls.set_camera(Some(device));
        }
        cx.notify();
    }

    /// `Event::VideoState` and `Event::CameraFailed`; true when the event was one of them.
    pub(super) fn on_video_event(&mut self, event: &dc_voice::Event) -> bool {
        match event {
            dc_voice::Event::VideoState { camera } => {
                info!(camera, "camera state");
                if let Some(call) = self.call.as_mut() {
                    call.camera = *camera;
                    let (guild, channel) = (call.guild, call.channel);
                    self.send_voice_state(guild, Some(channel));
                }
                true
            }
            dc_voice::Event::CameraFailed { reason } => {
                warn!("camera failed: {reason}");
                true
            }
            _ => false,
        }
    }

    fn stream_previews_on(&self) -> bool {
        !matches!(
            self.setting_choices.get("streaming_show_stream_previews"),
            Some(super::settings::Choice::On(false))
        )
    }

    /// Our user id and the voice session id of the call, which a stream connection reuses.
    fn stream_identity(&self) -> Option<(u64, String)> {
        let me = self.stores.current_user.as_ref()?.id;
        let call = self.call.as_ref()?;
        let session = self
            .stores
            .voice_members(call.guild, call.channel)
            .into_iter()
            .find(|state| state.user_id == me)?
            .session_id
            .clone();
        Some((me, session))
    }

    /// Whether anyone but us is in the call's channel.
    #[cfg(feature = "capture")]
    fn others_present(&self) -> bool {
        let (Some(me), Some(call)) = (self.stores.current_user.as_ref(), self.call.as_ref()) else {
            return true;
        };
        self.stores
            .voice_members(call.guild, call.channel)
            .iter()
            .any(|state| state.user_id != me.id)
    }

    /// Go Live from `source`: op 18, then the dispatches lead to the stream connection.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))] // The Share Your Screen button (not built yet) calls this.
    pub(super) fn stream_start(&mut self, source: StreamSource, cx: &mut Context<Self>) {
        let Some((me, session)) = self.stream_identity() else {
            return;
        };
        let Some(call) = &self.call else {
            return;
        };
        let (guild, channel) = (call.guild, call.channel);
        let guild = (guild != 0).then_some(guild);
        let key = match guild {
            Some(guild) => StreamKey::Guild {
                guild,
                channel,
                owner: me,
            },
            None => StreamKey::Call { channel, owner: me },
        };
        if self.video.streams.contains_key(&key) {
            return;
        }
        let controls = StreamControls::new();
        controls.set_previews(self.stream_previews_on());
        let tracker = Tracker::new(key, Role::Send, me, session, Instant::now());
        let last = tracker.current();
        self.video.streams.insert(
            key,
            Live {
                tracker,
                controls,
                source: Some(source),
                last,
            },
        );
        info!("stream: create");
        let _ = self.requests.send(Request::StreamOp {
            op: Op::StreamCreate,
            data: streams::create(guild, channel, None),
        });
        self.stream_watchdog(key, cx);
        cx.notify();
    }

    #[cfg(feature = "capture")]
    /// The capture-only start of the harness: the synthetic pattern at the lowest settings, ten seconds at most, and
    /// only when nobody else is in the channel. The answer says what happened.
    pub(crate) fn stream_start_synthetic(&mut self, cx: &mut Context<Self>) -> &'static str {
        if self.call.as_ref().is_none_or(|call| !call.connected) {
            return "not in a connected call";
        }
        if self.others_present() {
            return "refused: someone else is in the channel";
        }
        let (width, height, fps, bitrate) = HARNESS_SOURCE;
        self.stream_start(
            StreamSource::Synthetic {
                width,
                height,
                fps,
                bitrate,
            },
            cx,
        );
        let own = self.video.streams.keys().copied().find(|key| {
            self.stores
                .current_user
                .as_ref()
                .is_some_and(|me| key.owner() == me.id)
        });
        if let Some(key) = own {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(HARNESS_STREAM_LIMIT).await;
                let _ = this.update(cx, |root, cx| root.stream_stop_key(key, cx));
            })
            .detach();
        }
        "started"
    }

    /// Ends the stream we send, if any (op 19 and the connection).
    pub(crate) fn stream_stop(&mut self, cx: &mut Context<Self>) {
        let own: Vec<StreamKey> = self
            .video
            .streams
            .iter()
            .filter(|(_, live)| live.last.role == Role::Send)
            .map(|(key, _)| *key)
            .collect();
        for key in own {
            self.stream_stop_key(key, cx);
        }
    }

    fn stream_stop_key(&mut self, key: StreamKey, cx: &mut Context<Self>) {
        let Some(mut live) = self.video.streams.remove(&key) else {
            return;
        };
        info!("stream: stop");
        live.controls.stop();
        let _ = live.tracker.stopped();
        let _ = self.requests.send(Request::StreamOp {
            op: Op::StreamDelete,
            data: streams::delete(&key),
        });
        cx.notify();
    }

    /// Watching a stream: op 20; the connection starts when both dispatches arrive.
    #[allow(dead_code)] // The stream tile of the call view (not built yet) calls this.
    pub(super) fn stream_watch(&mut self, key: StreamKey, cx: &mut Context<Self>) {
        let Some((me, session)) = self.stream_identity() else {
            return;
        };
        if self.video.streams.contains_key(&key) {
            return;
        }
        let tracker = Tracker::new(key, Role::Watch, me, session, Instant::now());
        let last = tracker.current();
        self.video.streams.insert(
            key,
            Live {
                tracker,
                controls: StreamControls::new(),
                source: None,
                last,
            },
        );
        let _ = self.requests.send(Request::StreamOp {
            op: Op::StreamWatch,
            data: streams::watch(&key),
        });
        self.stream_watchdog(key, cx);
        cx.notify();
    }

    /// The 30 second rendezvous bound: fails the stream when the gateway never names its server.
    fn stream_watchdog(&mut self, key: StreamKey, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(streams::RENDEZVOUS_TIMEOUT + Duration::from_secs(1))
                .await;
            let _ = this.update(cx, |root, cx| {
                let update = root
                    .video
                    .streams
                    .get_mut(&key)
                    .and_then(|live| live.tracker.tick(Instant::now()));
                if let Some(update) = update {
                    root.stream_changed(update, true, cx);
                }
            });
        })
        .detach();
    }

    /// The `STREAM_*` dispatches of the main gateway.
    pub(super) fn stream_dispatch(&mut self, name: &str, data: &Value, cx: &mut Context<Self>) {
        if !name.starts_with("STREAM_") {
            return;
        }
        let Some(dispatch) = streams::parse_dispatch(name, data) else {
            return;
        };
        let key = dispatch.key();
        let Some(live) = self.video.streams.get_mut(&key) else {
            return;
        };
        let step = live.tracker.dispatch(&dispatch, Instant::now());
        let ended_by_server = matches!(dispatch, Dispatch::Delete { .. });
        if let Some(join) = step.join {
            let source = live.source.take();
            let controls = live.controls.clone();
            let hub = match &self.call {
                Some(call) => call.controls.hub(),
                None => return,
            };
            info!("stream: both dispatches in, connecting");
            let _ = self.requests.send(Request::StreamConnect {
                join,
                source,
                hub,
                controls,
            });
        }
        if let Some(update) = step.update {
            self.stream_changed(update, !ended_by_server, cx);
        }
    }

    /// An event of a stream's own voice connection.
    pub(super) fn on_stream_event(
        &mut self,
        key: StreamKey,
        event: StreamEvent,
        cx: &mut Context<Self>,
    ) {
        info!(?event, "stream connection event");
        let Some(live) = self.video.streams.get_mut(&key) else {
            return;
        };
        if let Some(update) = live.tracker.connection(&event) {
            self.stream_changed(update, true, cx);
        }
    }

    /// Records a status change; an ended stream is dropped, and the gateway told when it does not know yet.
    fn stream_changed(&mut self, update: StreamUpdate, tell_gateway: bool, cx: &mut Context<Self>) {
        info!(status = ?update.status, viewers = update.viewers, "stream status");
        let key = update.key;
        if let StreamStatus::Ended(reason) = update.status {
            if let Some(live) = self.video.streams.remove(&key) {
                live.controls.stop();
                if tell_gateway && reason != EndReason::StreamFull {
                    let _ = self.requests.send(Request::StreamOp {
                        op: Op::StreamDelete,
                        data: streams::delete(&key),
                    });
                }
            }
        } else if let Some(live) = self.video.streams.get_mut(&key) {
            live.last = update;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(user: u64, source: VideoSource) -> RemoteFrame {
        RemoteFrame {
            user,
            source,
            picture: dc_media::Picture {
                width: 2,
                height: 2,
                layout: dc_media::Layout::Rgba,
                data: vec![0; 16],
            },
        }
    }

    #[test]
    fn the_latest_picture_per_source_is_kept_and_repaints_coalesce() {
        let frames = Frames::default();
        assert!(!frames.take_dirty());
        frames.store(frame(1, VideoSource::Camera));
        frames.store(frame(1, VideoSource::Camera));
        frames.store(frame(1, VideoSource::Stream(1)));
        assert!(frames.take_dirty(), "one repaint for three pictures");
        assert!(!frames.take_dirty());
        assert!(frames.latest(1, VideoSource::Camera).is_some());
        assert!(frames.latest(2, VideoSource::Camera).is_none());
        assert_eq!(frames.seen.load(Ordering::Relaxed), 3);
        frames.clear();
        assert!(frames.latest(1, VideoSource::Stream(1)).is_none());
    }
}
