//! Pace microphone callback batches into 20 ms packets without discarding normal speech.
//!
//! Adapted from Serein `crates/discord-voice/src/capture.rs`, MIT OR Apache-2.0. Unchanged
//! apart from formatting.
use crate::Frame;
use std::sync::mpsc::Receiver;

#[derive(Default)]
pub(crate) struct CapturePacer {
    // One frame of lookahead absorbs callback/worker jitter. The input channel
    // remains capped at eight frames: at most nine frames / 34,560 PCM bytes total.
    pending: Option<Frame>,
}

impl CapturePacer {
    /// Local detection while alone: consume audio without retaining it for transmission.
    pub fn preview(&mut self, input: &Receiver<Frame>) -> Option<Frame> {
        self.pending = None;
        let mut latest = None;
        for _ in 0..8 {
            let Ok(frame) = input.try_recv() else {
                break;
            };
            latest = Some(frame);
        }
        latest
    }

    pub fn next(&mut self, input: &Receiver<Frame>, enabled: bool, stalled: bool) -> Option<Frame> {
        if !enabled || stalled {
            self.pending = None;
            for _ in 0..8 {
                if input.try_recv().is_err() {
                    break;
                }
            }
            return None;
        }
        let frame = self.pending.take();
        self.pending = input.try_recv().ok();
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_frame_lookahead_and_flush_when_disabled() {
        let (send, receive) = std::sync::mpsc::sync_channel(8);
        let mut pacer = CapturePacer::default();
        send.send([1.0; 960]).unwrap();
        send.send([2.0; 960]).unwrap();
        assert!(
            pacer.next(&receive, true, false).is_none(),
            "lookahead fills first"
        );
        assert_eq!(pacer.next(&receive, true, false).unwrap()[0], 1.0);
        send.send([3.0; 960]).unwrap();
        assert!(pacer.next(&receive, false, false).is_none());
        assert!(receive.try_recv().is_err(), "muted audio is discarded");
        assert!(
            pacer.next(&receive, true, false).is_none(),
            "and so is the lookahead"
        );
    }
}
