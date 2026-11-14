//! Media Foundation hardware H.264 encoder: the first asynchronous hardware transform that takes NV12 and gives H.264.
//! Adapted from Serein's `crates/discord-voice/src/video_encode_windows.rs` (MIT OR Apache-2.0).
#![allow(unsafe_code)]

use super::{Config, Output, Profile};
use crate::mfrt::Runtime;
use crate::{Error, Result, h264};
use windows::{
    Win32::{
        Media::MediaFoundation::*,
        System::{Com::CoTaskMemFree, Variant::VARIANT},
    },
    core::{IUnknown, Interface},
};

fn unavailable(what: &str, error: impl std::fmt::Display) -> Error {
    Error::unavailable(format!("hardware H.264 encoder: {what}: {error}"))
}

fn failed(what: &str, error: impl std::fmt::Display) -> Error {
    Error::failed(format!("hardware H.264 encoder: {what}: {error}"))
}

const SAMPLE_POOL_LIMIT: usize = 4;

struct PooledSample {
    sample: IMFSample,
    buffer: IMFMediaBuffer,
    capacity: u32,
}

impl PooledSample {
    unsafe fn new(capacity: u32) -> Result<Self> {
        // SAFETY: `MFCreateMemoryBuffer` and `MFCreateSample` initialize valid COM objects on this thread.
        unsafe {
            let buffer = MFCreateMemoryBuffer(capacity).map_err(|e| failed("buffer", e))?;
            let sample = MFCreateSample().map_err(|e| failed("sample", e))?;
            sample
                .AddBuffer(&buffer)
                .map_err(|e| failed("add buffer", e))?;
            Ok(Self {
                sample,
                buffer,
                capacity,
            })
        }
    }

    unsafe fn is_reusable(&self, min_capacity: u32) -> bool {
        if self.capacity < min_capacity {
            return false;
        }
        // An idle pooled pair has 1 ref on `sample` (ours) and 2 refs on `buffer` (ours + `sample`).
        // SAFETY: checking COM refcounts via IUnknown AddRef+Release restores counts identically.
        unsafe { com_refcount(&self.sample) == 1 && com_refcount(&self.buffer) == 2 }
    }
}

unsafe fn com_refcount<T: Interface>(object: &T) -> u32 {
    let raw = object.as_raw();
    // SAFETY: every COM interface inherits `IUnknown` at offset 0 of its vtable; AddRef then Release is a net no-op.
    unsafe {
        let vtable = object.assume_vtable::<IUnknown>();
        let _ = (vtable.AddRef)(raw);
        (vtable.Release)(raw)
    }
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
    input_pool: Vec<PooledSample>,
    output_pool: Vec<PooledSample>,
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
                input_pool: Vec::with_capacity(SAMPLE_POOL_LIMIT),
                output_pool: Vec::with_capacity(SAMPLE_POOL_LIMIT),
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
        // SAFETY: the allocation is at least the validated NV12 size; Lock is balanced by Unlock.
        unsafe {
            let length = u32::try_from(nv12.len()).map_err(|e| failed("size", e))?;
            let slot = Self::acquire_sample(&mut self.input_pool, length)?;
            let mut data = std::ptr::null_mut();
            let mut capacity = 0;
            slot.buffer
                .Lock(&mut data, Some(&mut capacity), None)
                .map_err(|e| failed("lock", e))?;
            if data.is_null() || capacity < length {
                let _ = slot.buffer.Unlock();
                Self::recycle_sample(&mut self.input_pool, slot);
                return Err(Error::failed(
                    "hardware H.264 encoder: input buffer too small",
                ));
            }
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), data, nv12.len());
            let unlock = slot.buffer.Unlock().map_err(|e| failed("unlock", e));
            if let Err(err) = unlock {
                Self::recycle_sample(&mut self.input_pool, slot);
                return Err(err);
            }
            let prep = slot
                .buffer
                .SetCurrentLength(length)
                .map_err(|e| failed("length", e))
                .and_then(|()| {
                    slot.sample
                        .SetSampleTime(self.frame * self.duration)
                        .map_err(|e| failed("sample time", e))
                })
                .and_then(|()| {
                    slot.sample
                        .SetSampleDuration(self.duration)
                        .map_err(|e| failed("sample duration", e))
                });
            if let Err(err) = prep {
                Self::recycle_sample(&mut self.input_pool, slot);
                return Err(err);
            }
            self.frame += 1;
            let processed = self
                .transform
                .ProcessInput(0, &slot.sample, 0)
                .map_err(|e| failed("process input", e));
            Self::recycle_sample(&mut self.input_pool, slot);
            processed?;
        }
        self.wait_for_output()?;
        self.take_output().map(Some)
    }

    unsafe fn acquire_sample(pool: &mut Vec<PooledSample>, min_capacity: u32) -> Result<PooledSample> {
        if let Some(index) = pool
            .iter()
            .position(|entry| unsafe { entry.is_reusable(min_capacity) })
        {
            return Ok(pool.swap_remove(index));
        }
        // Drop any undersized idle slot before creating a larger one so old small allocations don't linger.
        pool.retain(|entry| !(entry.capacity < min_capacity && unsafe { entry.is_reusable(0) }));
        unsafe { PooledSample::new(min_capacity) }
    }

    fn recycle_sample(pool: &mut Vec<PooledSample>, slot: PooledSample) {
        if pool.len() < SAMPLE_POOL_LIMIT {
            pool.push(slot);
        }
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
        while !ready(self) {
            // SAFETY: blocking wait keeps this thread idle in the kernel until Media Foundation posts an event.
            let event = unsafe { self.events.GetEvent(MF_EVENT_FLAG_NONE) };
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
                Err(error) => return Err(failed("events", error)),
            }
        }
        Ok(())
    }

    fn take_output(&mut self) -> Result<Output> {
        // SAFETY: the output struct is fully initialized. When `!provides_samples`, pooled output samples are
        // cleared and reused once the MFT releases any extra COM references.
        unsafe {
            let mut slot = if self.provides_samples {
                None
            } else {
                let info = self
                    .transform
                    .GetOutputStreamInfo(0)
                    .map_err(|e| failed("stream info", e))?;
                let size = output_buffer_size(&info, self.max_buffer_bytes)?;
                let slot = Self::acquire_sample(&mut self.output_pool, size)?;
                slot.buffer
                    .SetCurrentLength(0)
                    .map_err(|e| failed("reset output buffer", e))?;
                Some(slot)
            };
            let own = slot.as_ref().map(|entry| entry.sample.clone());
            let mut output = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: std::mem::ManuallyDrop::new(own),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            };
            let mut status = 0;
            let result =
                self.transform
                    .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status);
            let sample = std::mem::ManuallyDrop::into_inner(output.pSample);
            drop(std::mem::ManuallyDrop::into_inner(output.pEvents));
            let bytes = match result {
                Ok(()) => {
                    let sample = sample
                        .or_else(|| slot.as_ref().map(|entry| entry.sample.clone()))
                        .ok_or_else(|| Error::failed("hardware H.264 encoder gave no sample"))?;
                    sample_bytes(&sample, self.max_bytes)
                }
                Err(error) => Err(failed("process output", error)),
            };
            if let Some(slot) = slot.take() {
                Self::recycle_sample(&mut self.output_pool, slot);
            }
            let data = bytes?;
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

    #[test]
    fn pooled_sample_reuses_when_idle_and_skips_when_aliased() {
        let _runtime = Runtime::open().unwrap();
        let mut pool = Vec::new();
        // SAFETY: synthetic MF memory objects owned only by this test.
        unsafe {
            let first = Encoder::acquire_sample(&mut pool, 4096).unwrap();
            let raw = first.sample.as_raw();
            assert!(first.is_reusable(4096));
            let alias = first.sample.clone();
            assert!(!first.is_reusable(4096));
            drop(alias);
            assert!(first.is_reusable(4096));
            Encoder::recycle_sample(&mut pool, first);

            let reused = Encoder::acquire_sample(&mut pool, 2048).unwrap();
            assert_eq!(reused.sample.as_raw(), raw);
            let alias = reused.sample.clone();
            Encoder::recycle_sample(&mut pool, reused);

            let second = Encoder::acquire_sample(&mut pool, 4096).unwrap();
            assert_ne!(second.sample.as_raw(), raw);
            drop(alias);
            Encoder::recycle_sample(&mut pool, second);

            let larger = Encoder::acquire_sample(&mut pool, 8192).unwrap();
            assert!(larger.capacity >= 8192);
        }
    }
}
