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
    /// Processing of its own instead of Voice & Video's (a recording that asks for its echo cancellation and noise
    /// cancellation, as `startLocalAudioRecording` takes them).
    own: Option<crate::process::ProcessingPrefs>,
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
            own: None,
        })
    }

    /// The microphone with Voice & Video's processing except echo cancellation and noise cancellation, which are as
    /// given (noise cancellation is the official Krisp, DeepFilterNet here).
    pub fn open_with(echo: bool, noise: bool) -> Result<Self, &'static str> {
        let mut microphone = Self::open()?;
        let own = crate::process::ProcessingPrefs::new();
        let global = &prefs().processing;
        own.set_room_echo(global.room_echo());
        own.set_automatic_gain_control(global.automatic_gain_control());
        own.set_echo_cancellation(echo);
        own.set_suppression(if noise {
            crate::process::Suppression::DeepFilterNet
        } else {
            crate::process::Suppression::None
        });
        microphone.own = Some(own);
        Ok(microphone)
    }

    /// The next processed frame, if the microphone has delivered one.
    pub fn next(&mut self) -> Option<Frame> {
        let mut frame = self.ring.pop().ok()?;
        let processing = self.own.as_ref().unwrap_or(&prefs().processing);
        self.processor.capture(&mut frame, processing);
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
