//! Video of the call and Go Live streams: the camera button, the pictures `dc-voice` decodes, and the life of a stream
//! (gateway ops 18 to 22 and the four `STREAM_*` dispatches, then the stream's own voice connection).
//!
//! Pictures are kept as the latest one per (user, source). The sink runs on decoder threads, stores and wakes a task on
//! the UI side, which converts the new picture on a worker and repaints when it is ready: every picture of a 60 fps
//! stream can be shown, as soon as it is decoded.

use super::Root;
use super::video_tiles::{Key, Slots, render_image};
use crate::net::Request;
use dc_proto::op::Op;
use dc_voice::{
    RemoteFrame, StreamControls, StreamEvent, StreamSource, VideoSource,
    streams::{self, Dispatch, EndReason, Role, StreamKey, StreamStatus, StreamUpdate, Tracker},
};
use gpui::{Context, RenderImage};
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

/// The body of the Camera Unavailable alert: the official text when no camera was found (`bhqSWl`), otherwise what
/// went wrong, so a camera which is listed but would not open is not reported as missing.
fn camera_failure_text(reason: &str) -> String {
    if reason == dc_media::camera::NOT_FOUND {
        "Uh oh, we can't detect any camera.".to_owned()
    } else {
        reason.to_owned()
    }
}

/// How often the picture task looks whether the call is still there when no picture wakes it.
///
/// It used to poll for pictures every 33 ms instead. gpui's timers on Windows run on thread-pool timers at the 15.6 ms
/// system tick, so that poll came every 47 ms: about 21 repaints a second, unevenly spaced, whatever the stream sent.
const IDLE_CHECK: Duration = Duration::from_millis(250);
/// A capture-only stream (the harness's `stream start`) ends by itself after this long.
#[cfg(feature = "capture")]
const HARNESS_STREAM_LIMIT: Duration = Duration::from_secs(10);
/// The lowest settings of the synthetic source: a 320 x 180 pattern at 5 frames a second.
#[cfg(feature = "capture")]
const HARNESS_SOURCE: (u32, u32, u32, u32) = (320, 180, 5, 150_000);

/// The latest picture of every source, written by decoder threads, with a sequence number per picture.
#[derive(Default)]
pub(super) struct Frames {
    latest: Mutex<HashMap<Key, (u64, Arc<dc_media::Picture>)>>,
    dirty: AtomicBool,
    seen: AtomicU64,
    /// Wakes the picture task. A wake with nobody waiting is kept for the next wait, so none is lost.
    wake: tokio::sync::Notify,
}

impl Frames {
    fn store(&self, frame: RemoteFrame) {
        let seq = self.seen.fetch_add(1, Ordering::Relaxed) + 1;
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((frame.user, frame.source), (seq, Arc::new(frame.picture)));
        if !self.dirty.swap(true, Ordering::AcqRel) {
            self.wake.notify_one();
        }
    }

    /// Until a picture arrives or `idle` ends, whichever is first.
    async fn arrival(&self, idle: impl std::future::Future<Output = ()>) {
        let mut woken = std::pin::pin!(self.wake.notified());
        let mut idle = std::pin::pin!(idle);
        std::future::poll_fn(|cx| {
            if woken.as_mut().poll(cx).is_ready() || idle.as_mut().poll(cx).is_ready() {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        })
        .await;
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
        if !self.dirty.swap(true, Ordering::AcqRel) {
            self.wake.notify_one();
        }
    }

    /// The pictures `slots` has no image of yet, newest only, except those of sources `paused` holds; each is marked
    /// as converting.
    fn pending(
        &self,
        slots: &mut Slots<Arc<RenderImage>>,
        paused: impl Fn(&Key) -> bool,
    ) -> Vec<(Key, u64, Arc<dc_media::Picture>)> {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(key, (seq, _))| !paused(key) && slots.wanted(**key, *seq))
            .map(|(key, (seq, picture))| (*key, *seq, picture.clone()))
            .collect()
    }

    /// Forgets the pictures of sources `keep` rejects (a camera that went off must not flash its last picture).
    fn retain(&self, keep: impl Fn(&Key) -> bool) {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|key, _| keep(key));
    }

    /// The picture a tile paints.
    #[cfg(test)]
    fn latest(&self, user: u64, source: VideoSource) -> Option<Arc<dc_media::Picture>> {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(user, source))
            .map(|(_, picture)| picture.clone())
    }
}

/// `allowMultiple` of `watchStream`: asked for, or two streams of others are watched already.
fn allows_multiple(forced: bool, others_watched: usize) -> bool {
    forced || others_watched >= 2
}

/// One stream this client sends or watches.
struct Live {
    tracker: Tracker,
    controls: StreamControls,
    /// What a sender captures; taken when the connection starts.
    source: Option<StreamSource>,
    last: StreamUpdate,
    /// What the picker chose, for the panel row and the tile's badge.
    started: Option<Started>,
    /// What a watched stream says it sends at most (`StreamEvent::Quality`).
    remote_quality: Option<(Option<u32>, u32)>,
}

/// What a stream we send was started with.
#[derive(Clone, Debug)]
pub(super) struct Started {
    pub name: String,
    pub info: dc_media::screen::Source,
    pub quality: (dc_media::screen::Resolution, u32),
    pub icon: Option<Arc<RenderImage>>,
}

/// What the call view needs of a stream: its state and, for ours, what it was started with.
#[derive(Clone, Debug)]
pub(super) struct StreamInfo {
    pub role: Role,
    /// The text of the quality pill (`720P 30FPS`, `SOURCE 60FPS`): what we send, or what the watched stream says.
    pub label: Option<String>,
    pub name: Option<String>,
    pub icon: Option<Arc<RenderImage>>,
    /// Who watches the stream (`getViewerIds`).
    pub viewer_ids: Vec<u64>,
}

#[derive(Default)]
pub(super) struct Video {
    frames: Arc<Frames>,
    /// The gpui image of every source's latest picture.
    pub(super) images: Slots<Arc<RenderImage>>,
    pumping: bool,
    streams: HashMap<StreamKey, Live>,
    /// A stream asked to be watched before this account's voice state of the call arrived (Watch Stream from
    /// outside the call): the owner and whether several were asked for. Sent once the state is there.
    pending_watch: Option<(u64, bool)>,
}

impl Video {
    /// The owners of the streams this client sends or watches.
    pub(super) fn owners(&self) -> Vec<u64> {
        self.streams.keys().map(|key| key.owner()).collect()
    }

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

/// `pauseSelfStreamPreviewWhenUnfocused` of `UnsyncedUserSettingsStore` (module 964404): on until switched off.
pub(super) fn pause_own_preview_unfocused() -> bool {
    crate::storage::get("UnsyncedUserSettingsStore")
        .and_then(|store| store.get("pauseSelfStreamPreviewWhenUnfocused")?.as_bool())
        .unwrap_or(true)
}

/// Harness only: frames of what an unfocused client draws.
#[cfg(feature = "capture")]
pub(super) static FAKE_UNFOCUSED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether no window of the client has the focus.
pub(super) fn client_unfocused(cx: &gpui::App) -> bool {
    #[cfg(feature = "capture")]
    if FAKE_UNFOCUSED.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    cx.active_window().is_none()
}

pub(super) fn set_pause_own_preview_unfocused(on: bool) {
    let mut store = crate::storage::get("UnsyncedUserSettingsStore")
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    store["pauseSelfStreamPreviewWhenUnfocused"] = serde_json::json!(on);
    crate::storage::set("UnsyncedUserSettingsStore", store);
}

impl Root {
    /// A call was created: pictures of it go to the store, and the repaint task runs while the call lasts.
    pub(super) fn video_call_started(&mut self, cx: &mut Context<Self>) {
        if self.call.is_none() {
            return;
        }
        let frames = self.video.frames.clone();
        frames.clear();
        self.video_release(cx);
        let Some(call) = &self.call else {
            return;
        };
        call.controls
            .set_video_sink(Some(Arc::new(move |frame| frames.store(frame))));
        self.apply_stream_attenuation();
        if self.video.pumping {
            return;
        }
        self.video.pumping = true;
        let frames = self.video.frames.clone();
        cx.spawn(async move |this, cx| {
            loop {
                frames
                    .arrival(cx.background_executor().timer(IDLE_CHECK))
                    .await;
                let alive = this.update(cx, |root, cx| {
                    if root.call.is_none() {
                        root.video.pumping = false;
                        root.video.frames.clear();
                        root.video_release(cx);
                        return false;
                    }
                    if root.video.frames.take_dirty() {
                        root.video_convert(cx);
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

    /// Whether the picture of a source can still be on a tile: a camera is on in the call's voice states (ours by
    /// `Event::VideoState`), a stream is one we send or watch.
    fn video_source_live(&self, key: &Key) -> bool {
        let Some(call) = &self.call else {
            return false;
        };
        let (user, source) = *key;
        match source {
            VideoSource::Camera => {
                if self
                    .stores
                    .current_user
                    .as_ref()
                    .is_some_and(|me| me.id == user)
                {
                    call.camera
                } else {
                    self.stores
                        .voice_members(call.guild, call.channel)
                        .iter()
                        .any(|state| state.user_id == user && state.self_video)
                }
            }
            VideoSource::Stream(owner) => self.video.streams.keys().any(|key| key.owner() == owner),
        }
    }

    /// Starts the conversion of every picture that has no image yet, each on a worker, and releases the images of
    /// sources that are gone.
    fn video_convert(&mut self, cx: &mut Context<Self>) {
        let keep: Vec<Key> = self
            .video
            .frames
            .latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect();
        let gone: Vec<Key> = keep
            .into_iter()
            .filter(|key| !self.video_source_live(key))
            .collect();
        if !gone.is_empty() {
            self.video.frames.retain(|key| !gone.contains(key));
            for image in self.video.images.retain(|key| !gone.contains(key)) {
                cx.drop_image(image, None);
            }
        }
        // The sharer's own tile holds its picture while no window of the client is focused
        // (`pauseSelfStreamPreviewWhenUnfocused`, on unless the user turns it off: module 964404; module 555148 pauses
        // the self stream's video then). The switch is the stream menu's `Pause Preview When Discord Isn't Focused`. Every
        // picture of a 60 fps share would otherwise be converted for a window nobody looks at.
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let unfocused = pause_own_preview_unfocused() && client_unfocused(cx);
        let paused =
            |key: &Key| unfocused && Some(key.0) == me && key.1 == VideoSource::Stream(key.0);
        for (key, seq, picture) in self.video.frames.pending(&mut self.video.images, paused) {
            cx.spawn(async move |this, cx| {
                let image = cx
                    .background_executor()
                    .spawn(async move { render_image(&picture) })
                    .await;
                let _ = this.update(cx, |root, cx| {
                    if let Some(old) = root.video.images.finish(key, seq, image) {
                        cx.drop_image(old, None);
                    }
                    // A newer picture that came during the conversion goes next, without waiting for another one.
                    root.video_convert(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Every image goes back to gpui (the call ended).
    pub(super) fn video_release(&mut self, cx: &mut Context<Self>) {
        for image in self.video.images.clear() {
            cx.drop_image(image, None);
        }
    }

    /// The camera button of the panel and the bar: turns our camera on with the chosen device (`audio.video_device`,
    /// else the first one; an empty id lets the call pick the default), or off.
    pub(super) fn voice_camera_clicked(&mut self, cx: &mut Context<Self>) {
        let turning_on = self
            .call
            .as_ref()
            .is_some_and(|call| !call.controls.camera_wanted());
        if turning_on && self.always_preview_video() {
            // "Always preview video": the modal opens first; its Turn On Camera calls `voice_camera_start`.
            self.open_camera_preview(cx);
            return;
        }
        self.voice_camera_toggle(cx);
    }

    /// Turns our camera on (the preview modal's Turn On Camera).
    pub(in crate::ui) fn voice_camera_start(&mut self, cx: &mut Context<Self>) {
        if self
            .call
            .as_ref()
            .is_some_and(|call| !call.controls.camera_wanted())
        {
            self.voice_camera_toggle(cx);
        }
    }

    fn voice_camera_toggle(&mut self, cx: &mut Context<Self>) {
        // The Test Video camera and the call's camera never share the device.
        self.camera_test_stop(cx);
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
                self.camera_error = Some(camera_failure_text(reason));
                true
            }
            _ => false,
        }
    }

    fn stream_previews_on(&self) -> bool {
        match self.setting_choices.get("streaming_show_stream_previews") {
            Some(super::settings::Choice::On(on)) => *on,
            _ => crate::storage::get("audio.stream_previews")
                .and_then(|value| value.as_bool())
                .unwrap_or(true),
        }
    }

    /// Show Stream Previews changed: the stream we send follows at once.
    pub(super) fn stream_apply_previews(&self, on: bool) {
        for live in self.video.streams.values() {
            if live.last.role == Role::Send {
                live.controls.set_previews(on);
            }
        }
    }

    /// Our user id and the voice session id of the call, which a stream connection reuses.
    fn stream_identity(&self) -> Option<(u64, String)> {
        let me = self.stores.current_user.as_ref()?.id;
        let call = self.call.as_ref()?;
        // The voice state must be this connection's own: the account may be in the call from another device, and a
        // stream asked for with that device's session is refused.
        let own = self.stores.session_id.as_deref();
        let session = self
            .stores
            .voice_members(call.guild, call.channel)
            .into_iter()
            .find(|state| state.user_id == me && own.is_none_or(|own| own == state.session_id))?
            .session_id
            .clone();
        Some((me, session))
    }

    /// Whether we are sending a stream (`getCurrentUserActiveStream() != null`).
    pub(in crate::ui) fn own_stream_active(&self) -> bool {
        self.video
            .streams
            .values()
            .any(|live| live.last.role == Role::Send)
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
    pub(super) fn stream_start(
        &mut self,
        source: StreamSource,
        started: Started,
        cx: &mut Context<Self>,
    ) {
        let Some((me, session)) = self.stream_identity() else {
            return;
        };
        let Some(call) = &self.call else {
            return;
        };
        let (guild, channel) = (call.guild, call.channel);
        let guild = (guild != 0).then_some(guild);
        self.clips_stream_starting(&started.name);
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
                started: Some(started),
                remote_quality: None,
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

    /// The harness's `voice` steps. They only ever name the fixture guild's General voice channel, so a script cannot
    /// join anything else: `voice join`, `voice leave`, `voice state`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_voice(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        const FIXTURE_GUILD: u64 = 1_568_692_564_637_524_109;
        const FIXTURE_VOICE: u64 = 1_568_692_565_136_375_901;
        match words {
            ["join"] => {
                self.join_voice(FIXTURE_GUILD, FIXTURE_VOICE, cx);
                "joining the fixture voice channel".to_owned()
            }
            ["leave"] => {
                self.leave_voice(cx);
                "left".to_owned()
            }
            ["state"] => match &self.call {
                Some(call) => format!(
                    "in call, connected {}, others present {}",
                    call.connected,
                    self.others_present()
                ),
                None => "not in a call".to_owned(),
            },
            ["engine"] => dc_voice::prefs().engine_report(),
            ["status", rest @ ..] => self.script_voice_status(rest),
            _ => "usage: voice join | voice leave | voice state | voice engine | voice status <text|clear>"
                .to_owned(),
        }
    }

    /// Capture only: a stream shown as live without a connection, ours (`watch` false) or the test person's that this
    /// client watches. Nothing is sent.
    #[cfg(feature = "capture")]
    pub(crate) fn fake_stream(
        &mut self,
        owner: u64,
        watch: bool,
        cx: &mut Context<Self>,
    ) -> &'static str {
        let Some((me, session)) = self.stream_identity() else {
            return "not in a call";
        };
        let Some(call) = &self.call else {
            return "not in a call";
        };
        let key = if call.guild != 0 {
            StreamKey::Guild {
                guild: call.guild,
                channel: call.channel,
                owner,
            }
        } else {
            StreamKey::Call {
                channel: call.channel,
                owner,
            }
        };
        let role = if watch { Role::Watch } else { Role::Send };
        let tracker = Tracker::new(key, role, me, session, Instant::now());
        let last = tracker.current();
        self.video.streams.insert(
            key,
            Live {
                tracker,
                controls: StreamControls::new(),
                source: None,
                last,
                started: None,
                remote_quality: watch.then_some((Some(1440), 60)),
            },
        );
        if watch {
            // A stand-in picture, so the tile has something to draw: 64px squares, a lighter one each third.
            self.video_call_started(cx);
            let (width, height) = (640u32, 360u32);
            let mut data = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                for x in 0..width {
                    let cell = (x / 64 + y / 64) % 3;
                    let shade = [40u8, 110, 200][cell as usize];
                    data.extend_from_slice(&[shade, (x / 3) as u8, (y / 2) as u8, 255]);
                }
            }
            self.video.frames.store(dc_voice::RemoteFrame {
                user: owner,
                source: dc_voice::VideoSource::Stream(owner),
                picture: dc_media::Picture {
                    width,
                    height,
                    layout: dc_media::Layout::Rgba,
                    data,
                },
            });
        }
        cx.notify();
        "a stream is shown without a connection; nothing was sent"
    }

    #[cfg(feature = "capture")]
    /// The capture-only start of the harness: the synthetic pattern at the lowest settings, ten seconds at most, and
    /// only when nobody else is in the channel. The answer says what happened.
    pub(crate) fn stream_start_synthetic(
        &mut self,
        with_others: bool,
        cx: &mut Context<Self>,
    ) -> &'static str {
        if self.call.as_ref().is_none_or(|call| !call.connected) {
            return "not in a connected call";
        }
        if self.others_present() && !with_others {
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
            Started {
                name: "Synthetic".to_owned(),
                info: dc_media::screen::Source {
                    id: dc_media::screen::SourceId::Display(1),
                    name: "Synthetic".to_owned(),
                    width: 320,
                    height: 180,
                    refresh_hz: 5,
                    icon: None,
                },
                quality: (dc_media::screen::Resolution::P480, 5),
                icon: None,
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
                let limit = if with_others {
                    Duration::from_secs(120)
                } else {
                    HARNESS_STREAM_LIMIT
                };
                cx.background_executor().timer(limit).await;
                let _ = this.update(cx, |root, cx| root.stream_stop_key(key, cx));
            })
            .detach();
        }
        "started"
    }

    /// The call is left: every stream watched or sent in it ends, and no watch waits for it any more.
    pub(crate) fn streams_stop_all(&mut self, cx: &mut Context<Self>) {
        self.video.pending_watch = None;
        let keys: Vec<StreamKey> = self.video.streams.keys().copied().collect();
        for key in keys {
            self.stream_stop_key(key, cx);
        }
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
                started: None,
                remote_quality: None,
            },
        );
        // The Stream Volume and Mute of this owner, as last chosen.
        self.apply_stream_volume(key.owner());
        let _ = self.requests.send(Request::StreamOp {
            op: Op::StreamWatch,
            data: streams::watch(&key),
        });
        self.stream_watchdog(key, cx);
        cx.notify();
    }

    /// The alert of `handleCameraUnavailable`: Camera Unavailable, the reason, Okay.
    pub(super) fn camera_error_modal(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        use gpui::{IntoElement, div, prelude::*};
        let reason = self.camera_error.clone()?;
        Some(super::modal::render_small(
            super::modal::Spec {
                id: "camera-error-modal",
                title: "Camera Unavailable".into(),
                subtitle: Some(div().child(reason).into_any_element()),
                body: None,
                buttons: vec![super::modal::button(
                    "camera-error-okay",
                    "Okay",
                    super::modal::Tone::Primary,
                    |this, _, _| this.camera_error = None,
                )],
                close: Box::new(|this, _, _| this.camera_error = None),
            },
            cx,
        ))
    }

    /// What our own stream was started with.
    pub(super) fn own_started(&self) -> Option<Started> {
        let me = self.stores.current_user.as_ref()?.id;
        self.video
            .streams
            .iter()
            .find(|(key, live)| key.owner() == me && live.last.role == Role::Send)
            .and_then(|(_, live)| live.started.clone())
    }

    /// Picks the camera in Voice & Video or the camera menu: stored in `audio.video_device`, and switched to at once
    /// when our camera is on.
    pub(super) fn choose_camera(&mut self, id: String, cx: &mut Context<Self>) {
        crate::storage::set("audio.video_device", Value::String(id.clone()));
        if let Some(call) = &self.call
            && call.controls.camera_wanted()
        {
            call.controls.set_camera(Some(id));
        }
        cx.notify();
    }

    /// The stream of `owner` in our call, if we send it, watch it, or the voice state says they go live.
    pub(super) fn video_stream_of(&self, owner: u64) -> Option<StreamInfo> {
        let (_, live) = self
            .video
            .streams
            .iter()
            .find(|(key, _)| key.owner() == owner)?;
        Some(StreamInfo {
            role: live.last.role,
            label: live
                .started
                .as_ref()
                .map(|started| super::go_live::quality_badge(started.quality.0, started.quality.1))
                .or_else(|| {
                    live.remote_quality.map(|(height, fps)| match height {
                        Some(height) => format!("{height}P {fps}FPS"),
                        None => format!("SOURCE {fps}FPS"),
                    })
                }),
            name: live.started.as_ref().map(|started| started.name.clone()),
            icon: live
                .started
                .as_ref()
                .and_then(|started| started.icon.clone()),
            viewer_ids: live.last.viewer_ids.clone(),
        })
    }

    /// The stream we send, if any.
    /// Whether this client sends a stream or watches one.
    pub(in crate::ui) fn in_a_stream(&self) -> bool {
        !self.video.streams.is_empty()
    }

    pub(super) fn own_stream(&self) -> Option<StreamInfo> {
        let me = self.stores.current_user.as_ref()?.id;
        self.video_stream_of(me)
            .filter(|info| info.role == Role::Send)
    }

    /// Watch Stream on a tile of `owner`'s stream in our call (`watchStream` of module 401843). Several streams
    /// stay watched when that was asked for (`forceMultiple`: the eye button, a shift click) or two of others
    /// already are; else the others stop and the tile is focused.
    pub(super) fn watch_stream_of(&mut self, owner: u64, cx: &mut Context<Self>) {
        self.watch_stream_with(owner, false, cx);
    }

    pub(super) fn watch_stream_with(
        &mut self,
        owner: u64,
        force_multiple: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = &self.call else {
            return;
        };
        if self.stream_identity().is_none() {
            // The call was just joined: the watch needs the session of our own voice state.
            self.video.pending_watch = Some((owner, force_multiple));
            return;
        }
        let (guild, channel) = (call.guild, call.channel);
        let others: Vec<u64> = self
            .watched_streams()
            .into_iter()
            .map(|(other, _)| other)
            .filter(|other| *other != owner)
            .collect();
        let multiple = allows_multiple(force_multiple, others.len());
        if !multiple {
            for other in others {
                self.unwatch_stream_of(other, cx);
            }
            self.vv.focus.tile = Some((owner, super::video_tiles::TileKind::Stream));
        }
        let key = if guild != 0 {
            StreamKey::Guild {
                guild,
                channel,
                owner,
            }
        } else {
            StreamKey::Call { channel, owner }
        };
        self.stream_watch(key, cx);
    }

    /// Watch Stream from a direct message whose call this account has not joined (`handleSelectParticipant` of
    /// module 540462: `selectVoiceChannel`, then `watchStream`).
    pub(super) fn watch_stream_joining(&mut self, owner: u64, cx: &mut Context<Self>) {
        if self.guild.is_none()
            && !self.dm_call_in(self.channel)
            && let Some(channel) = self.channel
        {
            self.join_dm_call(channel, false, cx);
        }
        self.watch_stream_of(owner, cx);
    }

    /// Whether `owner` is live in the call here.
    fn streams_in_call(&self, owner: u64) -> bool {
        self.call.as_ref().is_some_and(|call| {
            self.stores
                .voice_members(call.guild, call.channel)
                .into_iter()
                .any(|state| state.user_id == owner && state.self_stream)
        })
    }

    /// Our voice state arrived, or the call is gone: the watch that waited for it is sent or dropped.
    pub(super) fn stream_pending_watch(&mut self, cx: &mut Context<Self>) {
        if self.call.is_none() {
            self.video.pending_watch = None;
            return;
        }
        if self.stream_identity().is_some()
            && let Some((owner, multiple)) = self.video.pending_watch.take()
            && self.streams_in_call(owner)
        {
            self.watch_stream_with(owner, multiple, cx);
        }
    }

    /// Gives the call's stream audio mix the level stored for `owner`'s stream.
    pub(super) fn apply_stream_volume(&self, owner: u64) {
        if let Some(call) = &self.call {
            call.controls
                .hub()
                .set_stream_volume(owner, super::stream_volume::gain(owner));
        }
    }

    /// Stops watching `owner`'s stream (the tile menu).
    pub(super) fn unwatch_stream_of(&mut self, owner: u64, cx: &mut Context<Self>) {
        let key = self
            .video
            .streams
            .iter()
            .find(|(key, live)| key.owner() == owner && live.last.role == Role::Watch)
            .map(|(key, _)| *key);
        if let Some(key) = key {
            self.stream_stop_key(key, cx);
        }
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
        if let StreamEvent::Quality { height, fps } = event {
            live.remote_quality = Some((height, fps));
            cx.notify();
            return;
        }
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
            // `SE`: a viewer came or went while the stream stayed live; we are not counted among a stream's viewers.
            let was = live.last.clone();
            if was.status == StreamStatus::Live && update.status == StreamStatus::Live {
                let own = |role: Role, viewers: usize| match role {
                    Role::Watch => viewers.saturating_sub(1),
                    Role::Send => viewers,
                };
                if let Some(sound) = super::voice_sounds::viewer_sound(
                    own(was.role, was.viewers),
                    own(update.role, update.viewers),
                ) {
                    crate::sounds::play_event(sound);
                }
            }
            live.last = update;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_third_stream_joins_the_two_already_watched() {
        assert!(!super::allows_multiple(false, 0));
        assert!(!super::allows_multiple(false, 1));
        assert!(super::allows_multiple(false, 2));
        assert!(super::allows_multiple(true, 0));
    }

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

    #[test]
    fn a_picture_wakes_the_task_at_once_and_none_is_lost() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let frames = Frames::default();
        // Nothing arrived: the idle check ends the wait.
        let started = std::time::Instant::now();
        // The timer is made inside the runtime.
        runtime.block_on(async {
            frames
                .arrival(tokio::time::sleep(Duration::from_millis(30)))
                .await
        });
        assert!(started.elapsed() >= Duration::from_millis(25));
        // A picture stored before the task waits is not lost, and the wait ends without the idle check.
        frames.store(frame(1, VideoSource::Stream(1)));
        let started = std::time::Instant::now();
        runtime.block_on(frames.arrival(std::future::pending()));
        assert!(started.elapsed() < Duration::from_millis(25));
        assert!(frames.take_dirty());
        // Pictures until the task took the last one wake it once.
        frames.store(frame(1, VideoSource::Stream(1)));
        frames.store(frame(1, VideoSource::Stream(1)));
        runtime.block_on(frames.arrival(std::future::pending()));
        assert!(frames.take_dirty());
        // The timer is made inside the runtime.
        runtime.block_on(async {
            frames
                .arrival(tokio::time::sleep(Duration::from_millis(5)))
                .await
        });
        assert!(!frames.take_dirty());
    }
}

#[cfg(test)]
mod camera_text_tests {
    use super::camera_failure_text;

    #[test]
    fn only_a_missing_camera_reads_as_undetected() {
        assert_eq!(
            camera_failure_text(dc_media::camera::NOT_FOUND),
            "Uh oh, we can't detect any camera."
        );
        let busy = "Camera is busy or unavailable.";
        assert_eq!(camera_failure_text(busy), busy);
        assert_eq!(
            camera_failure_text("Camera RGB frame is not 24-bit uncompressed"),
            "Camera RGB frame is not 24-bit uncompressed"
        );
    }
}
