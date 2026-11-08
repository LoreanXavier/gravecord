//! Video in a call and in Go Live streams: receive (reorder, depacketize, DAVE, decode), send (packetize, DAVE, pace,
//! retransmit) and the hub that carries decoded pictures to the app.
//!
//! The audio path never waits for any of this: decoded pictures are handed over on decoder threads, every queue is
//! bounded, and a full queue drops the picture and asks the sender for a keyframe.
pub(crate) mod depacketize;
#[cfg(test)]
mod loopback;
pub(crate) mod receive;
pub(crate) mod reorder;
pub(crate) mod send;
pub(crate) mod sender;
pub(crate) mod sps;

use crate::Frame;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, atomic::AtomicU32},
};

/// Frames of watched stream audio waiting for the call's clock (120 ms). A stream connection pushes one every 20 ms;
/// the call mixes one per tick. Older frames are dropped rather than let the stream drift behind.
const STREAM_AUDIO_FRAMES: usize = 6;

/// Where a picture comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoSource {
    /// A participant's camera in the call.
    Camera,
    /// A Go Live stream of the user named by `RemoteFrame::user`; the number is that user's id too, so a watcher of
    /// several streams keeps them apart.
    Stream(u64),
}

/// One decoded picture of a remote source, or of our own camera as the local preview (`user` is then our own id).
/// The picture is NV12 or I420 as the decoder produced it, or RGBA for the local preview; see
/// [`dc_media::Picture::to_rgba`].
#[derive(Clone, Debug)]
pub struct RemoteFrame {
    pub user: u64,
    pub source: VideoSource,
    pub picture: dc_media::Picture,
}

/// Receives decoded pictures on a decoder thread. It must return quickly and never block.
pub type VideoSink = Arc<dyn Fn(RemoteFrame) + Send + Sync>;

/// The sink registration and the stream audio level, shared by a call connection and the stream connections the app
/// starts next to it. Cheap to clone.
#[derive(Clone)]
pub struct VideoHub {
    sink: Arc<Mutex<Option<VideoSink>>>,
    /// Level of watched stream audio in the mixer, 0.0 to 2.0, as `f32` bits (the app's per-stream volume).
    stream_volume: Arc<AtomicU32>,
    stream_audio: Arc<Mutex<VecDeque<Frame>>>,
}

impl Default for VideoHub {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoHub {
    pub fn new() -> Self {
        Self {
            sink: Arc::default(),
            stream_volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            stream_audio: Arc::default(),
        }
    }

    /// Registers the sink for every connection that shares this hub; `None` removes it.
    pub fn set_sink(&self, sink: Option<VideoSink>) {
        *self.sink.lock().unwrap_or_else(|e| e.into_inner()) = sink;
    }

    pub fn has_sink(&self) -> bool {
        self.sink
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    pub(crate) fn deliver(&self, frame: RemoteFrame) {
        let sink = self.sink.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(sink) = sink {
            sink(frame);
        }
    }

    pub(crate) fn push_stream_audio(&self, frame: Frame) {
        let mut queue = self.stream_audio.lock().unwrap_or_else(|e| e.into_inner());
        if queue.len() >= STREAM_AUDIO_FRAMES {
            queue.pop_front();
        }
        queue.push_back(frame);
    }

    /// The next frame of watched stream audio, scaled by the stream level.
    pub(crate) fn pop_stream_audio(&self) -> Option<Frame> {
        let mut frame = self
            .stream_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()?;
        let volume = self.stream_volume();
        if volume != 1.0 {
            for sample in &mut frame {
                *sample *= volume;
            }
        }
        Some(frame)
    }

    pub(crate) fn clear_stream_audio(&self) {
        self.stream_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// The level of watched stream audio, relative to the 1.0 default.
    pub fn set_stream_volume(&self, volume: f32) {
        self.stream_volume.store(
            volume.clamp(0.0, 2.0).to_bits(),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    pub fn stream_volume(&self) -> f32 {
        f32::from_bits(
            self.stream_volume
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn the_hub_delivers_to_the_current_sink_only() {
        let hub = VideoHub::new();
        let picture = dc_media::Picture::rgba(1, 1, vec![0; 4]).unwrap();
        let frame = RemoteFrame {
            user: 1,
            source: VideoSource::Camera,
            picture,
        };
        hub.deliver(frame.clone());
        assert!(!hub.has_sink());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        hub.set_sink(Some(Arc::new(move |f| {
            assert_eq!(f.user, 1);
            seen.fetch_add(1, Ordering::Relaxed);
        })));
        hub.clone().deliver(frame.clone());
        hub.set_sink(None);
        hub.deliver(frame);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(hub.stream_volume(), 1.0);
        for i in 0..8 {
            hub.push_stream_audio([i as f32; 960]);
        }
        hub.set_stream_volume(0.5);
        assert_eq!(
            hub.pop_stream_audio().unwrap()[0],
            1.0,
            "two oldest dropped, level applied"
        );
        hub.clear_stream_audio();
        assert!(hub.pop_stream_audio().is_none());
        hub.set_stream_volume(1.0);
        hub.set_stream_volume(9.0);
        assert_eq!(hub.stream_volume(), 2.0);
    }
}
