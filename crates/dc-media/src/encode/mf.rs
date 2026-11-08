//! Media Foundation hardware H.264 encoder: the first asynchronous hardware transform that takes NV12 and gives H.264.
//! Adapted from Serein's `crates/discord-voice/src/video_encode_windows.rs` (MIT OR Apache-2.0).
#![allow(unsafe_code)]

use super::{Config, Output, Profile};
use crate::mfrt::Runtime;
use crate::{Error, Result, h264};
use std::time::{Duration, Instant};
use windows::{
    Win32::{
        Media::MediaFoundation::*,
        System::{Com::CoTaskMemFree, Variant::VARIANT},
    },
    core::Interface,
};

fn unavailable(what: &str, error: impl std::fmt::Display) -> Error {
    Error::unavailable(format!("hardware H.264 encoder: {what}: {error}"))
}

fn failed(what: &str, error: impl std::fmt::Display) -> Error {
    Error::failed(format!("hardware H.264 encoder: {what}: {error}"))
}

pub(super) struct Encoder {
    transform: IMFTransform,
    events: IMFMediaEventGenerator,
    codec: ICodecAPI,
    _activate: Activated,
    _runtime: Runtime,
    frame: i64,
    duration: i64,
    need_input: usize,
    have_output: usize,
    provides_samples: bool,
    max_bytes: usize,
    max_buffer_bytes: usize,
    size: usize,
}

struct Activated(IMFActivate);

impl Drop for Activated {
    fn drop(&mut self) {
        // SAFETY: the activation and its object stay on the Media Foundation thread.
        unsafe {
            let _ = self.0.ShutdownObject();
        }
    }
}

impl Encoder {
    /// Opens the first hardware encoder that accepts this configuration. A machine with several (an NVIDIA card next
    /// to an integrated GPU) lists them all, and some refuse a configuration that another takes.
    pub(super) fn new(config: Config) -> Result<Self> {
        let _runtime = Runtime::open()?;
        // SAFETY: enumeration only; every returned activation object is owned by the Vec.
        let candidates = unsafe { hardware_encoders()? };
        let mut last = Error::unavailable("no hardware H.264 encoder");
        for (index, activate) in candidates.into_iter().enumerate() {
            match Self::open(config, activate) {
                Ok(encoder) => return Ok(encoder),
                Err(error) => {
                    tracing::debug!(
                        "hardware H.264 encoder {index} refused the configuration: {error}"
                    );
                    last = error;
                }
            }
        }
        Err(last)
    }

    fn open(config: Config, activate: IMFActivate) -> Result<Self> {
        let runtime = Runtime::open()?;
        // SAFETY: Media Foundation owns returned COM objects; the activation array is cleared before its CoTaskMem
        // allocation is released.
        unsafe {
            let activate = Activated(activate);
            let transform: IMFTransform = activate
                .0
                .ActivateObject()
                .map_err(|e| unavailable("activate", e))?;
            let attributes = transform
                .GetAttributes()
                .map_err(|e| unavailable("attributes", e))?;
            if attributes.GetUINT32(&MF_TRANSFORM_ASYNC).unwrap_or(0) == 0 {
                return Err(Error::unavailable(
                    "hardware H.264 encoder is not asynchronous",
                ));
            }
            attributes
                .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
                .map_err(|e| unavailable("unlock", e))?;
            let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
            let codec: ICodecAPI = transform.cast().map_err(|e| unavailable("codec api", e))?;
            let _ = codec.SetValue(&CODECAPI_AVLowLatencyMode, &VARIANT::from(true));
            let _ = codec.SetValue(
                &CODECAPI_AVEncCommonRateControlMode,
                &VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32),
            );
            let _ = codec.SetValue(
                &CODECAPI_AVEncCommonMeanBitRate,
                &VARIANT::from(config.bitrate),
            );
            let _ = codec.SetValue(
                &CODECAPI_AVEncMPVDefaultBPictureCount,
                &VARIANT::from(0_u32),
            );
            let _ = codec.SetValue(&CODECAPI_AVEncMPVGOPSize, &VARIANT::from(config.gop()));

            let output = video_type(config, MFVideoFormat_H264)?;
            output
                .SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate)
                .map_err(|e| unavailable("bitrate", e))?;
            // Advisory: an encoder that rejects the attribute keeps its own default profile.
            let _ = output.SetUINT32(
                &MF_MT_MPEG2_PROFILE,
                match config.profile {
                    Profile::Baseline => eAVEncH264VProfile_Base.0 as u32,
                    Profile::Main => eAVEncH264VProfile_Main.0 as u32,
                },
            );
            transform
                .SetOutputType(0, &output, 0)
                .map_err(|e| unavailable("output type", e))?;

            let input = video_type(config, MFVideoFormat_NV12)?;
            input
                .SetUINT32(&MF_MT_DEFAULT_STRIDE, config.width)
                .map_err(|e| unavailable("stride", e))?;
            transform
                .SetInputType(0, &input, 0)
                .map_err(|e| unavailable("input type", e))?;

            let info = transform
                .GetOutputStreamInfo(0)
                .map_err(|e| unavailable("stream info", e))?;
            // cbSize is the minimum output-buffer capacity, not the encoded length: allow a raw-picture-sized buffer
            // and enforce the transport's cap on the encoded sample afterwards.
            let max_buffer_bytes = (config.width as usize)
                .checked_mul(config.height as usize)
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| Error::unavailable("picture too large"))?
                .max(config.max_bytes);
            let provides_samples = info.dwFlags
                & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                    | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32)
                != 0;
            if !provides_samples {
                output_buffer_size(&info, max_buffer_bytes)?;
            }
            let events = transform
                .cast()
                .map_err(|e| unavailable("event generator", e))?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(|e| unavailable("begin streaming", e))?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(|e| unavailable("start of stream", e))?;
            Ok(Self {
                transform,
                events,
                codec,
                _activate: activate,
                _runtime: runtime,
                frame: 0,
                duration: 10_000_000 / i64::from(config.fps),
                max_bytes: config.max_bytes,
                max_buffer_bytes,
                need_input: 0,
                have_output: 0,
                provides_samples,
                size: config.width as usize * config.height as usize * 3 / 2,
            })
        }
    }

    pub(super) fn set_bitrate(&mut self, bitrate: u32) -> Result<()> {
        // SAFETY: dynamic codec control stays on the encoder's owning thread.
        unsafe {
            self.codec
                .SetValue(&CODECAPI_AVEncCommonMeanBitRate, &VARIANT::from(bitrate))
                .map_err(|e| failed("set bitrate", e))
        }
    }

    /// Encodes one tight NV12 picture.
    pub(super) fn encode(&mut self, nv12: &[u8], force_keyframe: bool) -> Result<Option<Output>> {
        if nv12.len() != self.size {
            return Err(Error::invalid("hardware encoder input has the wrong size"));
        }
        self.wait_for_input()?;
        if force_keyframe {
            // SAFETY: codec control is called on the owning thread before this input sample.
            unsafe {
                self.codec
                    // Microsoft specifies ULONG (VT_UI4), not VARIANT_BOOL. A rejected command otherwise silently
                    // sends the stream to software.
                    .SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &force_keyframe_value())
                    .map_err(|e| failed("force keyframe", e))?;
            }
        }
        // SAFETY: the allocation is exactly the validated NV12 size; Lock is balanced by Unlock.
        unsafe {
            let length = u32::try_from(nv12.len()).map_err(|e| failed("size", e))?;
            let buffer = MFCreateMemoryBuffer(length).map_err(|e| failed("input buffer", e))?;
            let mut data = std::ptr::null_mut();
            let mut capacity = 0;
            buffer
                .Lock(&mut data, Some(&mut capacity), None)
                .map_err(|e| failed("lock", e))?;
            if data.is_null() || capacity < length {
                let _ = buffer.Unlock();
                return Err(Error::failed(
                    "hardware H.264 encoder: input buffer too small",
                ));
            }
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), data, nv12.len());
            buffer.Unlock().map_err(|e| failed("unlock", e))?;
            buffer
                .SetCurrentLength(length)
                .map_err(|e| failed("length", e))?;
            let sample = MFCreateSample().map_err(|e| failed("sample", e))?;
            sample
                .AddBuffer(&buffer)
                .map_err(|e| failed("add buffer", e))?;
            sample
                .SetSampleTime(self.frame * self.duration)
                .map_err(|e| failed("sample time", e))?;
            sample
                .SetSampleDuration(self.duration)
                .map_err(|e| failed("sample duration", e))?;
            self.frame += 1;
            self.transform
                .ProcessInput(0, &sample, 0)
                .map_err(|e| failed("process input", e))?;
        }
        self.wait_for_output()?;
        self.take_output().map(Some)
    }

    fn wait_for_input(&mut self) -> Result<()> {
        self.wait_until(|encoder| encoder.need_input != 0)?;
        self.need_input -= 1;
        Ok(())
    }

    fn wait_for_output(&mut self) -> Result<()> {
        self.wait_until(|encoder| encoder.have_output != 0)?;
        self.have_output -= 1;
        Ok(())
    }

    fn wait_until(&mut self, ready: impl Fn(&Self) -> bool) -> Result<()> {
        let deadline = Instant::now() + Duration::from_millis(250);
        while !ready(self) {
            // SAFETY: event polling stays on the thread that owns this transform.
            let event = unsafe { self.events.GetEvent(MF_EVENT_FLAG_NO_WAIT) };
            match event {
                Ok(event) => unsafe {
                    if event
                        .GetStatus()
                        .map_err(|e| failed("event status", e))?
                        .is_err()
                    {
                        return Err(Error::failed("hardware H.264 encoder reported an error"));
                    }
                    match event.GetType().map_err(|e| failed("event type", e))? as i32 {
                        kind if kind == METransformNeedInput.0 => self.need_input += 1,
                        kind if kind == METransformHaveOutput.0 => self.have_output += 1,
                        kind if kind == MEError.0 => {
                            return Err(Error::failed("hardware H.264 encoder failed"));
                        }
                        _ => {}
                    }
                },
                Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => {
                    if Instant::now() >= deadline {
                        return Err(Error::failed("hardware H.264 encoder timed out"));
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(failed("events", error)),
            }
        }
        Ok(())
    }

    fn take_output(&self) -> Result<Output> {
        // SAFETY: the output struct is fully initialized. We take ownership of either the transform-provided sample or
        // our cloned sample before releasing its native fields.
        unsafe {
            let own = if self.provides_samples {
                None
            } else {
                let info = self
                    .transform
                    .GetOutputStreamInfo(0)
                    .map_err(|e| failed("stream info", e))?;
                let size = output_buffer_size(&info, self.max_buffer_bytes)?;
                let buffer = MFCreateMemoryBuffer(size).map_err(|e| failed("output buffer", e))?;
                let sample = MFCreateSample().map_err(|e| failed("sample", e))?;
                sample
                    .AddBuffer(&buffer)
                    .map_err(|e| failed("add buffer", e))?;
                Some(sample)
            };
            let mut output = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: std::mem::ManuallyDrop::new(own.clone()),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            };
            let mut status = 0;
            let result =
                self.transform
                    .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status);
            let sample = std::mem::ManuallyDrop::into_inner(output.pSample).or(own);
            drop(std::mem::ManuallyDrop::into_inner(output.pEvents));
            result.map_err(|e| failed("process output", e))?;
            let sample =
                sample.ok_or_else(|| Error::failed("hardware H.264 encoder gave no sample"))?;
            let data = sample_bytes(&sample, self.max_bytes)?;
            let keyframe = h264::is_keyframe(&data);
            Ok(Output { data, keyframe })
        }
    }
}

fn force_keyframe_value() -> VARIANT {
    VARIANT::from(1_u32)
}

fn output_buffer_size(info: &MFT_OUTPUT_STREAM_INFO, limit: usize) -> Result<u32> {
    if info.cbSize == 0 || info.cbSize as usize > limit {
        return Err(Error::failed(
            "hardware H.264 encoder output buffer size is unusable",
        ));
    }
    Ok(info.cbSize)
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: stop immediately; unsent video is disposable and flushing avoids blocking teardown.
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}

unsafe fn hardware_encoders() -> Result<Vec<IMFActivate>> {
    // SAFETY: MFTEnumEx allocates the array of activation objects; we take them all, clear the array and free it.
    unsafe {
        let input = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let mut entries = std::ptr::null_mut();
        let mut count = 0;
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_ASYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut entries,
            &mut count,
        )
        .map_err(|e| unavailable("enumerate", e))?;
        if entries.is_null() {
            return Err(Error::unavailable("no hardware H.264 encoder"));
        }
        let entries_slice = std::slice::from_raw_parts_mut(entries, count as usize);
        let found: Vec<IMFActivate> = entries_slice.iter_mut().filter_map(Option::take).collect();
        CoTaskMemFree(Some(entries.cast()));
        if found.is_empty() {
            return Err(Error::unavailable("no hardware H.264 encoder"));
        }
        Ok(found)
    }
}

unsafe fn video_type(config: Config, subtype: windows::core::GUID) -> Result<IMFMediaType> {
    // SAFETY: plain attribute setters on a freshly created media type.
    unsafe {
        let media_type = MFCreateMediaType().map_err(|e| unavailable("media type", e))?;
        media_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|e| unavailable("major type", e))?;
        media_type
            .SetGUID(&MF_MT_SUBTYPE, &subtype)
            .map_err(|e| unavailable("subtype", e))?;
        media_type
            .SetUINT64(
                &MF_MT_FRAME_SIZE,
                (u64::from(config.width) << 32) | u64::from(config.height),
            )
            .map_err(|e| unavailable("frame size", e))?;
        media_type
            .SetUINT64(&MF_MT_FRAME_RATE, u64::from(config.fps) << 32 | 1)
            .map_err(|e| unavailable("frame rate", e))?;
        media_type
            .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, 1_u64 << 32 | 1)
            .map_err(|e| unavailable("aspect", e))?;
        media_type
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(|e| unavailable("interlace", e))?;
        Ok(media_type)
    }
}

fn sample_bytes(sample: &IMFSample, max_bytes: usize) -> Result<Vec<u8>> {
    // SAFETY: the length is bounded before allocation; Lock's pointer is borrowed until Unlock.
    unsafe {
        if sample.GetTotalLength().map_err(|e| failed("length", e))? as usize > max_bytes {
            return Err(Error::failed(
                "hardware H.264 picture exceeds the size limit",
            ));
        }
        let buffer = sample
            .ConvertToContiguousBuffer()
            .map_err(|e| failed("contiguous", e))?;
        let mut data = std::ptr::null_mut();
        let mut capacity = 0;
        let mut length = 0;
        buffer
            .Lock(&mut data, Some(&mut capacity), Some(&mut length))
            .map_err(|e| failed("lock output", e))?;
        let result = if data.is_null() || length > capacity || length as usize > max_bytes {
            Err(Error::failed("hardware H.264 output buffer is invalid"))
        } else {
            Ok(std::slice::from_raw_parts(data, length as usize).to_vec())
        };
        buffer.Unlock().map_err(|e| failed("unlock output", e))?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic memory only: no transform, GPU or device is touched.
    #[test]
    fn larger_native_buffer_does_not_relax_the_encoded_limit() {
        let _runtime = Runtime::open().unwrap();
        let limit = 128 * 1024;
        // SAFETY: a memory buffer we create, lock and unlock in order.
        unsafe {
            let capacity = 640 * 480 * 3 / 2;
            assert!(capacity > limit);
            let buffer = MFCreateMemoryBuffer(capacity as u32).unwrap();
            let mut data = std::ptr::null_mut();
            buffer.Lock(&mut data, None, None).unwrap();
            std::ptr::write_bytes(data, 0x2a, capacity);
            buffer.Unlock().unwrap();
            buffer.SetCurrentLength(limit as u32).unwrap();
            let sample = MFCreateSample().unwrap();
            sample.AddBuffer(&buffer).unwrap();
            let encoded = sample_bytes(&sample, limit).unwrap();
            assert_eq!(encoded.len(), limit);
            assert!(encoded.iter().all(|&byte| byte == 0x2a));
            buffer.SetCurrentLength(limit as u32 + 1).unwrap();
            assert!(sample_bytes(&sample, limit).is_err());
        }
        let mut info = MFT_OUTPUT_STREAM_INFO {
            dwFlags: 0,
            cbSize: 1280 * 720 * 3 / 2,
            cbAlignment: 0,
        };
        assert_eq!(
            output_buffer_size(&info, 1280 * 720 * 4).unwrap(),
            info.cbSize
        );
        assert!(output_buffer_size(&info, 1024).is_err());
        info.cbSize = 0;
        assert!(output_buffer_size(&info, 1280 * 720 * 4).is_err());
        info.cbSize = u32::MAX;
        assert!(output_buffer_size(&info, 1280 * 720 * 4).is_err());
    }

    #[test]
    fn force_keyframe_is_an_unsigned_long() {
        let value = force_keyframe_value();
        assert_eq!(value.vt(), windows::Win32::System::Variant::VT_UI4);
        assert_eq!(u32::try_from(&value).unwrap(), 1);
    }
}
