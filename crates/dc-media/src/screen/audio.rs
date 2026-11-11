//! System audio for a screen share: WASAPI process loopback that excludes this process tree, so the call's own sound
//! is never sent back (no echo). Never the microphone. Adapted from Serein's `screen/audio_windows.rs`
//! (`crates/discord-voice`, MIT OR Apache-2.0); the epoch gating for encryption transitions is dropped, since this
//! crate hands chunks to a callback and has no transport.
//!
//! Chunks are 48 kHz stereo interleaved `f32`, sanitized to finite values in `[-1, 1]`.
#![allow(unsafe_code)]

use crate::{Error, Result};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// The most samples (both channels) one chunk may hold: 100 ms.
pub const MAX_AUDIO_SAMPLES: usize = 9_600;

/// One packet of system audio.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioChunk {
    /// Interleaved stereo, 48 kHz, so `samples.len()` is even.
    pub samples: Vec<f32>,
}

/// Clamps to `[-1, 1]` and turns NaN and infinities into silence.
pub(crate) fn sanitize(samples: &mut [f32]) {
    for sample in samples {
        *sample = if sample.is_finite() {
            sample.clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }
}

/// Running capture; dropping it stops the thread and joins it.
pub struct AudioCapture {
    #[cfg(windows)]
    inner: native::Running,
}

impl AudioCapture {
    /// Starts capturing system audio, calling `on_chunk` on the capture thread. Fails when the build lacks process
    /// loopback (Windows 10 build 20348 or newer is required).
    #[cfg(windows)]
    pub fn start(on_chunk: impl FnMut(AudioChunk) + Send + 'static) -> Result<Self> {
        Ok(Self {
            inner: native::Running::start(Box::new(on_chunk), native::Mode::ExcludeOwnTree)?,
        })
    }

    /// Starts capturing only what the process `pid` and its children play (process loopback in include mode): a
    /// game's own sound, without the call or anything else on the machine. Clips use it for a game without a stream.
    #[cfg(windows)]
    pub fn start_process(
        pid: u32,
        on_chunk: impl FnMut(AudioChunk) + Send + 'static,
    ) -> Result<Self> {
        Ok(Self {
            inner: native::Running::start(Box::new(on_chunk), native::Mode::OnlyProcessTree(pid))?,
        })
    }

    #[cfg(not(windows))]
    pub fn start_process(
        _pid: u32,
        _on_chunk: impl FnMut(AudioChunk) + Send + 'static,
    ) -> Result<Self> {
        Err(Error::Unsupported("process audio capture"))
    }

    #[cfg(not(windows))]
    pub fn start(_on_chunk: impl FnMut(AudioChunk) + Send + 'static) -> Result<Self> {
        Err(Error::Unsupported("system audio capture"))
    }

    /// Whether the capture thread has ended (a failure, since only dropping stops it).
    pub fn finished(&self) -> bool {
        #[cfg(windows)]
        {
            self.inner.finished()
        }
        #[cfg(not(windows))]
        {
            true
        }
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        mem::{ManuallyDrop, size_of},
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc::{SyncSender, sync_channel},
        },
        time::{Duration, Instant},
    };
    use windows::{
        Win32::{
            Foundation::{E_FAIL, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
            Media::Audio::*,
            System::{
                Com::{
                    BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
                    StructuredStorage::{
                        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                    },
                },
                Threading::{CreateEventW, WaitForSingleObject},
                Variant::VT_BLOB,
            },
        },
        core::{HRESULT, Interface, Ref, implement},
    };

    type Callback = Box<dyn FnMut(AudioChunk) + Send>;

    pub(super) struct Running {
        thread: Option<std::thread::JoinHandle<()>>,
        stop: Arc<AtomicBool>,
    }

    /// Whose sound is captured.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Mode {
        /// Everything on the machine except this process tree (Go Live: no echo of the call).
        ExcludeOwnTree,
        /// Only this process and its children.
        OnlyProcessTree(u32),
    }

    impl Running {
        pub(super) fn start(callback: Callback, mode: Mode) -> Result<Self> {
            let stop = Arc::new(AtomicBool::new(false));
            let (started, result) = sync_channel(1);
            let thread_stop = stop.clone();
            let thread = std::thread::Builder::new()
                .name("dc-media-audio".into())
                .spawn(move || {
                    let mut callback = callback;
                    if run(&thread_stop, &mut callback, started, mode).is_err() {
                        thread_stop.store(true, Ordering::Release);
                    }
                })
                .map_err(|_| Error::unavailable("System audio worker could not start"))?;
            let running = Self {
                thread: Some(thread),
                stop,
            };
            result.recv_timeout(Duration::from_secs(4)).map_err(|_| {
                Error::unavailable(
                    "Echo-free system audio could not start; Windows build 20348 or newer is required",
                )
            })?;
            Ok(running)
        }

        pub(super) fn finished(&self) -> bool {
            self.thread
                .as_ref()
                .is_none_or(std::thread::JoinHandle::is_finished)
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    #[implement(IActivateAudioInterfaceCompletionHandler)]
    struct Completion {
        send: SyncSender<()>,
        // Keeps the activation blob alive even if cancellation wins the completion race.
        _params: Arc<AUDIOCLIENT_ACTIVATION_PARAMS>,
    }

    impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
        fn ActivateCompleted(
            &self,
            _: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
        ) -> windows::core::Result<()> {
            let _ = self.send.try_send(());
            Ok(())
        }
    }

    pub(super) fn activation_params(mode: Mode) -> AUDIOCLIENT_ACTIVATION_PARAMS {
        let (pid, loopback) = match mode {
            Mode::ExcludeOwnTree => (
                std::process::id(),
                PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            ),
            Mode::OnlyProcessTree(pid) => (pid, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE),
        };
        AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: loopback,
                },
            },
        }
    }

    fn run(
        stop: &AtomicBool,
        callback: &mut Callback,
        started: SyncSender<()>,
        mode: Mode,
    ) -> windows::core::Result<()> {
        // This dedicated thread initializes and releases all COM objects in one MTA.
        // SAFETY: balanced with CoUninitialize below.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        let mut started = Some(started);
        let result = (|| {
            while !stop.load(Ordering::Acquire) {
                capture(stop, callback, &mut started, mode)?;
            }
            Ok(())
        })();
        // SAFETY: matches the CoInitializeEx above, after every COM object of `capture` was dropped.
        unsafe { CoUninitialize() };
        result
    }

    fn capture(
        stop: &AtomicBool,
        callback: &mut Callback,
        started: &mut Option<SyncSender<()>>,
        mode: Mode,
    ) -> windows::core::Result<()> {
        if stop.load(Ordering::Acquire) {
            return Err(E_FAIL.into());
        }
        let params = Arc::new(activation_params(mode));
        let (send, done) = sync_channel(1);
        let completion: IActivateAudioInterfaceCompletionHandler = Completion {
            send,
            _params: params.clone(),
        }
        .into();
        // The blob points into `params`; PROPVARIANT's Drop would call PropVariantClear and try to free that Arc
        // allocation with CoTaskMemFree. The completion handler owns `params` for the whole async activation.
        let variant = ManuallyDrop::new(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB {
                            cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                            pBlobData: Arc::as_ptr(&params).cast_mut().cast(),
                        },
                    },
                    ..Default::default()
                }),
            },
        });
        // SAFETY: the operation retains the agile completion handler and the immutable blob.
        let operation = unsafe {
            ActivateAudioInterfaceAsync(
                VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
                &IAudioClient::IID,
                Some(&*variant),
                &completion,
            )?
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(E_FAIL.into());
            }
            if done.recv_timeout(Duration::from_millis(50)).is_ok() {
                break;
            }
        }
        let mut status = HRESULT(0);
        let mut activated = None;
        // SAFETY: completion has fired; GetActivateResult returns an owned COM reference.
        unsafe { operation.GetActivateResult(&mut status, &mut activated)? };
        drop(operation);
        status.ok()?;
        // SAFETY: plain event creation; wrapped at once so it is closed after the clients are released.
        let event = unsafe { CreateEventW(None, false, false, None)? };
        // SAFETY: the handle was just created and is owned here.
        let event = unsafe { OwnedHandle::from_raw_handle(event.0) };
        let client: IAudioClient = activated.ok_or(E_FAIL)?.cast()?;
        let format = WAVEFORMATEX {
            wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
            nChannels: CHANNELS as u16,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * 8,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        // SAFETY: `format` outlives the call; the native converter gives stereo 48 kHz whatever the output format is.
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                0,
                0,
                &format,
                None,
            )?;
        }
        // SAFETY: queries and wiring of the initialized client; the event outlives the stream.
        let capture: IAudioCaptureClient = unsafe {
            if client.GetBufferSize()? as usize > MAX_AUDIO_SAMPLES / 2 {
                return Err(E_FAIL.into());
            }
            let capture = client.GetService()?;
            client.SetEventHandle(HANDLE(event.as_raw_handle()))?;
            capture
        };
        // SAFETY: Start/Stop bracket the packet loop on this thread.
        unsafe { client.Start()? };
        if let Some(started) = started.take() {
            let _ = started.try_send(());
        }
        let result = packets(stop, callback, &capture, &event);
        // Always stop before releasing the capture client and event, including after packet errors.
        // SAFETY: the stream was started above.
        let stopped = unsafe { client.Stop() };
        result.and(stopped)
    }

    fn packets(
        stop: &AtomicBool,
        callback: &mut Callback,
        capture: &IAudioCaptureClient,
        event: &OwnedHandle,
    ) -> windows::core::Result<()> {
        while !stop.load(Ordering::Acquire) {
            // SAFETY: the event handle is open for the whole loop.
            match unsafe { WaitForSingleObject(HANDLE(event.as_raw_handle()), 50) } {
                WAIT_TIMEOUT => continue,
                WAIT_OBJECT_0 => {}
                _ => return Err(E_FAIL.into()),
            }
            // Bound work per wakeup as well as the bytes in each packet.
            for _ in 0..32 {
                // SAFETY: packet access follows the GetBuffer/ReleaseBuffer protocol; the slice is copied before
                // ReleaseBuffer.
                unsafe {
                    if stop.load(Ordering::Acquire) || capture.GetNextPacketSize()? == 0 {
                        break;
                    }
                    let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0, 0);
                    capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    let count = (frames as usize).saturating_mul(CHANNELS);
                    let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                    let valid = count <= MAX_AUDIO_SAMPLES
                        && (silent
                            || (!data.is_null() && data.align_offset(align_of::<f32>()) == 0));
                    if valid && count != 0 {
                        let mut samples = if silent {
                            vec![0.0; count]
                        } else {
                            std::slice::from_raw_parts(data.cast::<f32>(), count).to_vec()
                        };
                        sanitize(&mut samples);
                        callback(AudioChunk { samples });
                    }
                    capture.ReleaseBuffer(frames)?;
                    if !valid {
                        return Err(E_FAIL.into());
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_stream_capture_excludes_our_entire_process_tree() {
            let params = activation_params(Mode::ExcludeOwnTree);
            assert_eq!(
                params.ActivationType,
                AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK
            );
            // SAFETY: the union was built as ProcessLoopbackParams.
            let process = unsafe { params.Anonymous.ProcessLoopbackParams };
            assert_eq!(process.TargetProcessId, std::process::id());
            assert_eq!(
                process.ProcessLoopbackMode,
                PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE
            );
        }

        #[test]
        fn a_game_capture_takes_only_that_process_tree() {
            let params = activation_params(Mode::OnlyProcessTree(42));
            // SAFETY: the union was built as ProcessLoopbackParams.
            let process = unsafe { params.Anonymous.ProcessLoopbackParams };
            assert_eq!(process.TargetProcessId, 42);
            assert_eq!(
                process.ProcessLoopbackMode,
                PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_clamped_and_made_finite() {
        let mut samples = [
            0.5,
            -2.0,
            3.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -1.0,
        ];
        sanitize(&mut samples);
        assert_eq!(samples, [0.5, -1.0, 1.0, 0.0, 0.0, 0.0, -1.0]);
    }
}
