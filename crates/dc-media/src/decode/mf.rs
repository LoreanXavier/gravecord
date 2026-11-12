//! Media Foundation H.264 decoder transform on its own multi-threaded CPU path, giving NV12 that is cropped here straight
//! out of the locked output buffer. Adapted from Serein's `crates/platform/src/video/live_windows.rs` (MIT OR
//! Apache-2.0).
//!
//! Serein hands the transform a DXGI device manager for DXVA. Every picture then has to come back from the GPU through
//! `ConvertToContiguousBuffer`, and on a 2560x1440 Go Live stream that read-back cost 42 ms a picture (worst 65 ms,
//! `bench::bench_go_live_1440p60_picture_path`, RTX 3090): the decoder could not keep up with 30 fps, let alone 60. The
//! CPU path of the same transform takes 7 ms a picture, so the call view gets every picture of a 60 fps stream.
#![allow(unsafe_code)]

use crate::mfrt::Runtime;
use crate::picture::pack_plane;
use crate::{Error, Layout, Picture, Result};
use windows::Win32::{
    Media::MediaFoundation::*,
    System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
};

const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
/// 100 ns units per picture at a nominal 30 fps; only monotonicity matters to the decoder.
const TICKS_PER_PICTURE: i64 = 333_333;

fn unavailable(what: &str, error: impl std::fmt::Display) -> Error {
    Error::unavailable(format!("Media Foundation H.264 decoder: {what}: {error}"))
}

fn failed(what: &str, error: impl std::fmt::Display) -> Error {
    Error::failed(format!("Media Foundation H.264 decoder: {what}: {error}"))
}

// Field order: COM objects release before the runtime shuts down.
pub(super) struct Decoder {
    transform: IMFTransform,
    _runtime: Runtime,
    pictures: i64,
    /// Pictures released so far; while it keeps up with `pictures` the transform runs without delay.
    released: i64,
    output: Option<OutputFormat>,
    provides_samples: bool,
}

/// How long a decode waits, in 1 ms steps, for a picture the transform's decoding threads are still finishing, while
/// the transform has released one picture per access unit so far. A transform that holds pictures back stops this.
const SETTLE_STEPS: u32 = 8;

#[derive(Clone, Copy)]
struct OutputFormat {
    width: u32,
    height: u32,
    stride: u32,
    crop: (u32, u32, u32, u32),
}

impl Decoder {
    pub(super) fn new() -> Result<Self> {
        let runtime = Runtime::open()?;
        // SAFETY: plain COM and Media Foundation calls on the thread that started the runtime; every out-pointer refers
        // to an initialized local.
        unsafe {
            let transform: IMFTransform =
                CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| unavailable("create", e))?;
            if let Ok(attributes) = transform.GetAttributes() {
                let _ = attributes.SetUINT32(&CODECAPI_AVLowLatencyMode, 1);
            }
            let input = MFCreateMediaType().map_err(|e| unavailable("media type", e))?;
            input
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(|e| unavailable("major type", e))?;
            input
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
                .map_err(|e| unavailable("subtype", e))?;
            transform
                .SetInputType(0, &input, 0)
                .map_err(|e| unavailable("input type", e))?;
            let mut decoder = Self {
                transform,
                _runtime: runtime,
                pictures: 0,
                released: 0,
                output: None,
                provides_samples: false,
            };
            decoder.negotiate_output()?;
            decoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(|e| unavailable("begin streaming", e))?;
            decoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(|e| unavailable("start of stream", e))?;
            Ok(decoder)
        }
    }

    /// Queues one Annex B access unit and returns the newest picture the decoder released, if any.
    pub(super) fn decode(&mut self, access_unit: &[u8]) -> Result<Option<Picture>> {
        // SAFETY: the buffer length is validated before the copy; Lock is balanced by Unlock.
        unsafe {
            let length = u32::try_from(access_unit.len()).map_err(|e| failed("size", e))?;
            let buffer = MFCreateMemoryBuffer(length).map_err(|e| failed("buffer", e))?;
            let mut data = std::ptr::null_mut();
            let mut capacity = 0;
            buffer
                .Lock(&mut data, Some(&mut capacity), None)
                .map_err(|e| failed("lock", e))?;
            if data.is_null() || (capacity as usize) < access_unit.len() {
                let _ = buffer.Unlock();
                return Err(Error::failed(
                    "Media Foundation H.264 decoder: buffer too small",
                ));
            }
            std::ptr::copy_nonoverlapping(access_unit.as_ptr(), data, access_unit.len());
            buffer.Unlock().map_err(|e| failed("unlock", e))?;
            buffer
                .SetCurrentLength(length)
                .map_err(|e| failed("length", e))?;
            let sample = MFCreateSample().map_err(|e| failed("sample", e))?;
            sample
                .AddBuffer(&buffer)
                .map_err(|e| failed("add buffer", e))?;
            sample
                .SetSampleTime(self.pictures * TICKS_PER_PICTURE)
                .map_err(|e| failed("sample time", e))?;
            sample
                .SetSampleDuration(TICKS_PER_PICTURE)
                .map_err(|e| failed("sample duration", e))?;
            self.pictures += 1;
            let mut newest = None;
            match self.transform.ProcessInput(0, &sample, 0) {
                Ok(()) => {}
                Err(error) if error.code() == MF_E_NOTACCEPTING => {
                    newest = self.drain()?;
                    self.transform
                        .ProcessInput(0, &sample, 0)
                        .map_err(|e| failed("process input", e))?;
                }
                Err(error) => return Err(failed("process input", error)),
            }
            let mut picture = self.drain()?.or(newest);
            // The transform decodes on threads of its own: now and then (about one call in 130 here) the picture of
            // this access unit is not ready yet when its output is asked for, and would only come out with the next
            // one, a picture late (never, for the last picture of a still desktop).
            let in_step = self.released + 1 >= self.pictures;
            let mut steps = 0;
            while picture.is_none() && in_step && steps < SETTLE_STEPS {
                std::thread::sleep(std::time::Duration::from_millis(1));
                picture = self.drain()?;
                steps += 1;
            }
            if picture.is_some() {
                self.released = self.pictures;
            }
            Ok(picture)
        }
    }

    fn negotiate_output(&mut self) -> Result<()> {
        // SAFETY: media type enumeration on a live transform with validated indices.
        unsafe {
            let mut index = 0;
            loop {
                let candidate = self
                    .transform
                    .GetOutputAvailableType(0, index)
                    .map_err(|e| unavailable("no NV12 output", e))?;
                index += 1;
                if candidate
                    .GetGUID(&MF_MT_SUBTYPE)
                    .map_err(|e| unavailable("output subtype", e))?
                    != MFVideoFormat_NV12
                {
                    continue;
                }
                self.transform
                    .SetOutputType(0, &candidate, 0)
                    .map_err(|e| unavailable("output type", e))?;
                break;
            }
            let info = self
                .transform
                .GetOutputStreamInfo(0)
                .map_err(|e| unavailable("stream info", e))?;
            self.provides_samples = info.dwFlags
                & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                    | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32)
                != 0;
            let current = self
                .transform
                .GetOutputCurrentType(0)
                .map_err(|e| unavailable("current output type", e))?;
            let size = current.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
            let (width, height) = ((size >> 32) as u32, size as u32);
            if width == 0 || height == 0 {
                // Before the first SPS the decoder has no size yet; it announces one with a stream change.
                self.output = None;
                return Ok(());
            }
            let stride = current
                .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                .ok()
                .filter(|stride| *stride >= width)
                .unwrap_or(width);
            let mut crop = (0, 0, width, height);
            let mut bytes = [0_u8; std::mem::size_of::<MFVideoArea>()];
            let mut written = 0;
            if current
                .GetBlob(
                    &MF_MT_MINIMUM_DISPLAY_APERTURE,
                    &mut bytes,
                    Some(&mut written),
                )
                .is_ok()
                && written as usize == bytes.len()
            {
                // MFVideoArea contains only integer fields; the byte copy may be unaligned.
                let area = std::ptr::read_unaligned(bytes.as_ptr().cast::<MFVideoArea>());
                let (x, y) = (
                    area.OffsetX.value.max(0) as u32,
                    area.OffsetY.value.max(0) as u32,
                );
                let (w, h) = (area.Area.cx.max(0) as u32, area.Area.cy.max(0) as u32);
                if w > 0 && h > 0 && x + w <= width && y + h <= height {
                    crop = (x, y, w, h);
                }
            }
            if crop.2 as usize * crop.3 as usize > super::MAX_PICTURE_PIXELS
                || (stride as usize) * (height as usize) * 3 / 2 > MAX_OUTPUT_BYTES
            {
                return Err(Error::failed(
                    "Media Foundation H.264 decoder: picture too large",
                ));
            }
            self.output = Some(OutputFormat {
                width,
                height,
                stride,
                crop,
            });
            Ok(())
        }
    }

    /// Pulls every available picture and returns the newest; a format change renegotiates NV12 and continues.
    fn drain(&mut self) -> Result<Option<Picture>> {
        let mut newest = None;
        loop {
            // SAFETY: the output buffer struct is fully initialized; samples we allocate are released with the struct,
            // samples the transform provides are taken over below.
            let sample = unsafe {
                let own = if self.provides_samples {
                    None
                } else {
                    let info = self
                        .transform
                        .GetOutputStreamInfo(0)
                        .map_err(|e| failed("stream info", e))?;
                    if info.cbSize == 0 || info.cbSize as usize > MAX_OUTPUT_BYTES {
                        return Err(Error::failed(
                            "Media Foundation H.264 decoder: unusable output size",
                        ));
                    }
                    let buffer =
                        MFCreateMemoryBuffer(info.cbSize).map_err(|e| failed("buffer", e))?;
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
                let provided = std::mem::ManuallyDrop::into_inner(output.pSample);
                drop(std::mem::ManuallyDrop::into_inner(output.pEvents));
                match result {
                    Ok(()) => provided.or(own),
                    Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                        return Ok(newest);
                    }
                    Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                        self.negotiate_output()?;
                        continue;
                    }
                    Err(error) => return Err(failed("process output", error)),
                }
            };
            let Some(sample) = sample else {
                return Ok(newest);
            };
            let format = self.output.ok_or_else(|| {
                Error::failed("Media Foundation H.264 decoder: picture before its format")
            })?;
            newest = Some(read_picture(&sample, format)?);
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: stop immediately; pending pictures are disposable.
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}

/// Locks the sample's buffer and crops the picture straight out of it, so the decoder's output is copied once.
fn read_picture(sample: &IMFSample, format: OutputFormat) -> Result<Picture> {
    // SAFETY: the native length is checked first; Lock's pointer is borrowed only until Unlock and the slice length is
    // validated against the reported capacity.
    unsafe {
        if sample.GetTotalLength().map_err(|e| failed("length", e))? as usize > MAX_OUTPUT_BYTES {
            return Err(Error::failed(
                "Media Foundation H.264 decoder: output too large",
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
            .map_err(|e| failed("lock", e))?;
        let result = if length > capacity
            || length as usize > MAX_OUTPUT_BYTES
            || length == 0
            || data.is_null()
        {
            Err(Error::failed(
                "Media Foundation H.264 decoder: output buffer is invalid",
            ))
        } else {
            crop_nv12(std::slice::from_raw_parts(data, length as usize), format)
        };
        buffer.Unlock().map_err(|e| failed("unlock", e))?;
        result
    }
}
/// Crops one strided NV12 picture to its display aperture as tightly packed NV12. An odd origin cannot start between
/// chroma pairs, so that rare case converts the enclosing even aperture to RGBA and trims it.
fn crop_nv12(bytes: &[u8], format: OutputFormat) -> Result<Picture> {
    let invalid =
        || Error::failed("Media Foundation H.264 decoder: output does not match its format");
    let (stride, height) = (format.stride as usize, format.height as usize);
    let (x0, y0, width, out_height) = (
        format.crop.0 as usize,
        format.crop.1 as usize,
        format.crop.2 as usize,
        format.crop.3 as usize,
    );
    let luma_len = stride * height;
    if x0 + width > format.width as usize
        || y0 + out_height > height
        || bytes.len() < luma_len + stride * height.div_ceil(2)
    {
        return Err(invalid());
    }
    let (odd_x, odd_y) = (x0 % 2, y0 % 2);
    if odd_x + odd_y > 0 {
        let aligned = crop_nv12(
            bytes,
            OutputFormat {
                crop: (
                    format.crop.0 - odd_x as u32,
                    format.crop.1 - odd_y as u32,
                    format.crop.2 + odd_x as u32,
                    format.crop.3 + odd_y as u32,
                ),
                ..format
            },
        )?;
        let aligned_width = aligned.width as usize;
        let rgba = aligned.to_rgba().ok_or_else(invalid)?;
        let mut data = Vec::with_capacity(width * out_height * 4);
        for row in rgba.chunks_exact(aligned_width * 4).skip(odd_y) {
            data.extend_from_slice(&row[odd_x * 4..]);
        }
        return Picture::rgba(format.crop.2, format.crop.3, data).ok_or_else(invalid);
    }
    let (luma, chroma) = bytes.split_at(luma_len);
    let len = Layout::Nv12
        .len(format.crop.2, format.crop.3)
        .ok_or_else(invalid)?;
    let mut data = Vec::with_capacity(len);
    pack_plane(
        &mut data,
        &luma[y0 * stride + x0..],
        stride,
        width,
        out_height,
    )
    .and_then(|()| {
        pack_plane(
            &mut data,
            &chroma[(y0 / 2) * stride + x0..],
            stride,
            width.div_ceil(2) * 2,
            out_height.div_ceil(2),
        )
    })
    .ok_or_else(invalid)?;
    let picture = Picture {
        width: format.crop.2,
        height: format.crop.3,
        layout: Layout::Nv12,
        data,
    };
    picture.is_valid().then_some(picture).ok_or_else(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lone keyframe comes out of the call that fed it, every time: the loopback test once found nothing there
    /// (one run in four, then one in 25), because the transform's own decoding threads had not finished the picture
    /// when its output was asked for.
    #[test]
    fn a_keyframe_comes_out_of_the_call_that_fed_it() {
        let mut generator = crate::synthetic::Generator::new(640, 480, 15, 900_000).unwrap();
        let keyframe = generator.next(false).unwrap().unwrap();
        assert!(keyframe.keyframe);
        let mut missing = 0;
        for _ in 0..200 {
            let mut decoder = Decoder::new().unwrap();
            if decoder.decode(&keyframe.data).unwrap().is_none() {
                missing += 1;
            }
        }
        assert_eq!(missing, 0, "decodes of 200 that gave no picture");
    }

    #[test]
    fn nv12_crops_to_the_aperture_and_bounds() {
        let format = OutputFormat {
            width: 4,
            height: 2,
            stride: 4,
            crop: (0, 0, 2, 2),
        };
        let mut bytes = vec![235u8; 8];
        bytes.extend([128u8, 128, 7, 7]);
        let frame = crop_nv12(&bytes, format).unwrap();
        assert_eq!((frame.width, frame.height), (2, 2));
        assert_eq!(frame.data, [235, 235, 235, 235, 128, 128]);
        let rgba = frame.to_rgba().unwrap();
        assert!(rgba.chunks_exact(4).all(|px| px == [255, 255, 255, 255]));
        assert!(crop_nv12(&bytes[..6], format).is_err());
        let shifted = OutputFormat {
            crop: (2, 0, 2, 2),
            ..format
        };
        assert_eq!(&crop_nv12(&bytes, shifted).unwrap().data[4..], &[7, 7]);
        // An odd origin keeps chroma aligned through the RGBA path.
        let odd = OutputFormat {
            crop: (1, 0, 2, 2),
            ..format
        };
        let picture = crop_nv12(&bytes, odd).unwrap();
        assert_eq!(
            (picture.layout, picture.width, picture.height),
            (Layout::Rgba, 2, 2)
        );
        let aligned = crop_nv12(
            &bytes,
            OutputFormat {
                crop: (0, 0, 3, 2),
                ..format
            },
        )
        .unwrap()
        .to_rgba()
        .unwrap();
        assert_eq!(&picture.data[..8], &aligned[4..12]);
        assert_eq!(&picture.data[8..], &aligned[16..24]);
    }
}
