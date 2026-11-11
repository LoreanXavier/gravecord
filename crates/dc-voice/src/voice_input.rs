//! The microphone for a voice message recording: the chosen device through the same processors a call uses (echo
//! cancellation, noise suppression and automatic gain control as set in Voice & Video), but without the voice gate, so
//! every 20 ms of the recording arrives and the file is as long as the timer says. Module 198620 starts the native
//! recording with those same three settings and no gate.
//!
//! This is a child of `audio` so it can use the device opening and the rings that module keeps private.
use super::{Frame, Gate, open_input, prefs};
use crate::process::Processor;
use std::sync::{Arc, atomic::Ordering};

pub(crate) struct Microphone {
    _stream: cpal::Stream,
    ring: rtrb::Consumer<Frame>,
    gate: Arc<Gate>,
    processor: Processor,
}

impl Microphone {
    pub fn open() -> Result<Self, &'static str> {
        let host = cpal::default_host();
        let gate = Arc::new(Gate::default());
        gate.ready.store(true, Ordering::Release);
        let (stream, ring) = open_input(&host, &gate)?;
        let mut processor = Processor::new();
        processor.reset();
        Ok(Self {
            _stream: stream,
            ring,
            gate,
            processor,
        })
    }

    /// The next processed frame, if the microphone has delivered one.
    pub fn next(&mut self) -> Option<Frame> {
        let mut frame = self.ring.pop().ok()?;
        self.processor.capture(&mut frame, &prefs().processing);
        Some(frame)
    }

    /// The device went away.
    pub fn failed(&self) -> bool {
        self.gate.input_failed.load(Ordering::Acquire)
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        self.gate.stopped.store(true, Ordering::Release);
    }
}
