//! Media Foundation camera capture, with DirectShow for devices registered only there. Native callbacks copy one bounded
//! frame and never encode. Adapted from Serein's `crates/discord-voice/src/camera/windows.rs` (MIT OR Apache-2.0); this
//! one delivers RGBA, takes a stop flag instead of Serein's shared state, and reports when the device is open.
#![allow(unsafe_code)]

mod directshow;

use super::{CameraDevice, DeviceRef, FPS, HEIGHT, WIDTH, format};
use crate::Result;
use crate::mfrt::Runtime;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{Media::MediaFoundation::*, System::Com::CoTaskMemFree},
    core::{HRESULT, Interface, Ref, implement},
};

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const MAX_FRAME_BYTES: usize = (WIDTH * 4 + 4096) * HEIGHT;
const INVALID: &str = "Camera did not provide a bounded 640x480 RGB frame";
const UNAVAILABLE: &str = "Camera is busy or unavailable. Check Windows Settings > Privacy & security > Camera and allow desktop apps to access your camera.";
const TIMEOUT: &str = "Camera stopped delivering frames; check the device and try again";
const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / FPS as u64);

type R<T> = std::result::Result<T, &'static str>;

struct Source(IMFMediaSource);
impl Drop for Source {
    fn drop(&mut self) {
        // SAFETY: owned media source; Shutdown releases the camera, including on errors.
        unsafe {
            let _ = self.0.Shutdown();
        }
    }
}

struct ReadResult {
    changed: bool,
    rgba: Option<Vec<u8>>,
}

#[implement(IMFSourceReaderCallback)]
struct Callback {
    send: SyncSender<R<ReadResult>>,
    stride: Arc<AtomicI32>,
}
impl IMFSourceReaderCallback_Impl for Callback_Impl {
    fn OnReadSample(
        &self,
        status: HRESULT,
        _: u32,
        flags: u32,
        _: i64,
        sample: Ref<'_, IMFSample>,
    ) -> windows::core::Result<()> {
        let failed = MF_SOURCE_READERF_ERROR.0
            | MF_SOURCE_READERF_ENDOFSTREAM.0
            | MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED.0;
        let changed = flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0;
        let result = if status.is_err() || flags & failed as u32 != 0 {
            Err(UNAVAILABLE)
        } else if changed {
            // Some decoders finish negotiation on their first sample. Do not touch that sample until the worker has
            // revalidated the resulting media type.
            Ok(ReadResult {
                changed,
                rgba: None,
            })
        } else {
            sample
                .as_ref()
                .map(|sample| copy_sample(sample, self.stride.load(Ordering::Acquire)))
                .transpose()
                .map(|rgba| ReadResult { changed, rgba })
        };
        // Only one ReadSample is outstanding; no COM sample is retained in the channel.
        let _ = self.send.try_send(result);
        Ok(())
    }
    fn OnFlush(&self, _: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnEvent(&self, _: u32, event: Ref<'_, IMFMediaEvent>) -> windows::core::Result<()> {
        // SAFETY: the event is borrowed only during this callback.
        if let Some(event) = event.as_ref()
            && unsafe { event.GetStatus() }.is_ok_and(|status| status.is_err())
        {
            let _ = self.send.try_send(Err(UNAVAILABLE));
        }
        Ok(())
    }
}

/// Runs the DirectShow graph on a thread of its own: its filters (OBS's virtual camera among them) expect a
/// single-threaded apartment, while this thread is in the multithreaded one Media Foundation and the hardware encoder
/// hold. Frames come back over a channel.
fn run_directshow(
    stop: &AtomicBool,
    device: Option<DeviceRef<'_>>,
    opened: &mut dyn FnMut(),
    emit: &mut dyn FnMut(Vec<u8>) -> Result<()>,
) -> Result<()> {
    enum Message {
        Opened,
        Frame(Vec<u8>),
        Done(Result<()>),
    }
    let inner_stop = AtomicBool::new(false);
    let (send, receive) = mpsc::sync_channel::<Message>(2);
    thread::scope(|scope| {
        let graph = scope.spawn(|| {
            let (opened_send, frame_send) = (send.clone(), send.clone());
            let result = directshow::run(
                &inner_stop,
                device,
                &mut || {
                    let _ = opened_send.send(Message::Opened);
                },
                &mut |frame| {
                    frame_send
                        .send(Message::Frame(frame))
                        .map_err(|_| crate::Error::failed("camera consumer ended"))
                },
            );
            let _ = send.send(Message::Done(result));
        });
        let result = loop {
            if stop.load(Ordering::Acquire) {
                inner_stop.store(true, Ordering::Release);
            }
            match receive.recv_timeout(Duration::from_millis(50)) {
                Ok(Message::Opened) => opened(),
                Ok(Message::Frame(frame)) => {
                    if let Err(error) = emit(frame) {
                        inner_stop.store(true, Ordering::Release);
                        let _ = graph.join();
                        break Err(error);
                    }
                }
                Ok(Message::Done(result)) => break result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
            }
        };
        inner_stop.store(true, Ordering::Release);
        result
    })
}

pub(super) fn run(
    stop: &AtomicBool,
    device: Option<DeviceRef<'_>>,
    opened: &mut dyn FnMut(),
    emit: &mut dyn FnMut(Vec<u8>) -> Result<()>,
) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let _runtime = Runtime::open()?;
    let cameras = camera_sources()?;
    if matches!(device, Some(DeviceRef::DirectShow(_))) || (cameras.is_empty() && device.is_none())
    {
        return run_directshow(stop, device, opened, emit);
    }
    let source = selected_camera(cameras, device)?;
    let (send, receive) = mpsc::sync_channel(1);
    let stride = Arc::new(AtomicI32::new(0));
    let callback: IMFSourceReaderCallback = Callback {
        send,
        stride: stride.clone(),
    }
    .into();
    // SAFETY: COM objects stay on this MTA thread; the callback owns only thread-safe Rust state. All out parameters
    // are initialized and errors retain RAII cleanup.
    let reader = unsafe {
        let attributes = attributes(2)?;
        attributes
            .SetUnknown(&MF_SOURCE_READER_ASYNC_CALLBACK, &callback)
            .map_err(|_| UNAVAILABLE)?;
        attributes
            .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
            .map_err(|_| UNAVAILABLE)?;
        let reader =
            MFCreateSourceReaderFromMediaSource(&source.0, &attributes).map_err(|_| UNAVAILABLE)?;
        reader
            .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
            .map_err(|_| UNAVAILABLE)?;
        // Native dimensions are selected first to bound upstream decoder input.
        let mut choices = Vec::new();
        for index in 0..256 {
            let Ok(native) = reader.GetNativeMediaType(VIDEO, index) else {
                break;
            };
            let size = native.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
            let rate = native.GetUINT64(&MF_MT_FRAME_RATE).unwrap_or(0);
            let fps = ((rate >> 32) != 0 && rate as u32 != 0)
                .then(|| (rate >> 32) as f64 / (rate as u32) as f64);
            if let Some(mut rank) = format::rank(
                (size >> 32) as usize,
                (size as u32) as usize,
                fps.unwrap_or(f64::from(FPS)),
            ) {
                // Some drivers omit timing; keep them as the bounded fallback.
                if fps.is_none() {
                    rank.1 = u64::MAX;
                }
                choices.push((rank, native));
            }
        }
        choices.sort_by_key(|(rank, _)| *rank);
        let output = MFCreateMediaType().map_err(|_| INVALID)?;
        output
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|_| INVALID)?;
        output
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .map_err(|_| INVALID)?;
        output
            .SetUINT64(&MF_MT_FRAME_SIZE, frame_size())
            .map_err(|_| INVALID)?;
        // Output negotiation may otherwise select a different capture mode. Pin each bounded native mode on the source
        // before adding RGB conversion.
        let native_reader = reader
            .cast::<IMFSourceReaderEx>()
            .map_err(|_| UNAVAILABLE)?;
        let mut converted_stride = None;
        for (_, native) in choices {
            if native_reader.SetNativeMediaType(VIDEO, &native).is_err()
                || reader.SetCurrentMediaType(VIDEO, None, &output).is_err()
            {
                continue;
            }
            // Select only after the complete bounded output layout is usable.
            if let Ok(value) = output_stride(&reader) {
                converted_stride = Some(value);
                break;
            }
        }
        stride.store(
            converted_stride.ok_or(
                "Windows could not convert a supported camera mode up to 1280x720 to bounded RGB video",
            )?,
            Ordering::Release,
        );
        reader
            .SetStreamSelection(VIDEO, true)
            .map_err(|_| UNAVAILABLE)?;
        reader
    };
    opened();
    let mut last_frame = Instant::now();
    while !stop.load(Ordering::Acquire) {
        let requested = Instant::now();
        // SAFETY: the async reader returns immediately; callbacks never request more samples. A stalled device cannot
        // trap the thread in a synchronous ReadSample.
        unsafe { reader.ReadSample(VIDEO, 0, None, None, None, None) }.map_err(|_| UNAVAILABLE)?;
        let result = loop {
            if stop.load(Ordering::Acquire) {
                return Ok(());
            }
            if last_frame.elapsed() >= Duration::from_secs(5) {
                return Err(TIMEOUT.into());
            }
            match receive.recv_timeout(Duration::from_millis(50)) {
                Ok(result) => break result?,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(TIMEOUT.into()),
            }
        };
        if result.changed {
            stride.store(output_stride(&reader)?, Ordering::Release);
        }
        if let Some(rgba) = result.rgba {
            last_frame = Instant::now();
            if !stop.load(Ordering::Acquire) {
                emit(rgba)?;
            }
        }
        if let Some(remaining) = FRAME_INTERVAL.checked_sub(requested.elapsed()) {
            thread::sleep(remaining);
        }
    }
    Ok(())
}

fn frame_size() -> u64 {
    ((WIDTH as u64) << 32) | HEIGHT as u64
}

fn attributes(count: u32) -> R<IMFAttributes> {
    let mut attributes = None;
    // SAFETY: valid initialized out pointer, called after MF startup.
    unsafe { MFCreateAttributes(&mut attributes, count) }.map_err(|_| UNAVAILABLE)?;
    attributes.ok_or(UNAVAILABLE)
}

fn camera_sources() -> R<Vec<IMFActivate>> {
    // SAFETY: MF owns the returned count-sized array of COM pointers. Release every activation and CoTaskMemFree the
    // array, even when activation fails.
    unsafe {
        let attributes = attributes(1)?;
        attributes
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|_| UNAVAILABLE)?;
        let mut devices = std::ptr::null_mut();
        let mut count = 0;
        MFEnumDeviceSources(&attributes, &mut devices, &mut count).map_err(|_| UNAVAILABLE)?;
        if devices.is_null() {
            return Ok(Vec::new());
        }
        let entries = std::slice::from_raw_parts_mut(devices, count as usize);
        let result = entries
            .iter_mut()
            .take(32)
            .filter_map(Option::take)
            .collect();
        for entry in entries {
            *entry = None;
        }
        CoTaskMemFree(Some(devices.cast()));
        Ok(result)
    }
}

fn device_string(device: &IMFActivate, key: &windows::core::GUID, limit: usize) -> Option<String> {
    // SAFETY: fixed-size destination; GetString verifies its capacity. No native allocation retained.
    unsafe {
        let mut buffer = [0u16; 4096];
        let length = device.GetStringLength(key).ok()? as usize;
        if length == 0 || length >= buffer.len() {
            return None;
        }
        device
            .GetString(key, &mut buffer[..length + 1], None)
            .ok()?;
        let value = String::from_utf16(&buffer[..length]).ok()?;
        (value.len() <= limit && !value.contains('\0')).then_some(value)
    }
}

/// The symbolic link Media Foundation names the device by, without our `mf:` prefix.
fn device_link(device: &IMFActivate) -> Option<String> {
    device_string(
        device,
        &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
        4093,
    )
}

pub(super) fn devices() -> Result<Vec<CameraDevice>> {
    let _runtime = Runtime::open()?;
    let mf = camera_sources();
    let ds = directshow::devices();
    if mf.is_err() && ds.is_err() {
        return Err(UNAVAILABLE.into());
    }
    let mut devices: Vec<_> = mf
        .unwrap_or_default()
        .iter()
        .filter_map(|device| {
            Some(CameraDevice {
                id: format!("mf:{}", device_link(device)?),
                name: device_string(device, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, 256)?,
            })
        })
        .collect();
    // Keep backend identities distinct: virtual devices may share a friendly name.
    devices.extend(
        ds.unwrap_or_default()
            .into_iter()
            .take(32usize.saturating_sub(devices.len())),
    );
    Ok(devices)
}

fn selected_camera(cameras: Vec<IMFActivate>, selected: Option<DeviceRef<'_>>) -> R<Source> {
    let wanted = match selected {
        Some(DeviceRef::MediaFoundation(link)) => Some(link),
        _ => None,
    };
    let selected = cameras
        .iter()
        .find(|camera| wanted.is_none_or(|link| device_link(camera).as_deref() == Some(link)))
        .ok_or(
            "Selected camera is disconnected or unavailable. Refresh cameras and choose another device.",
        )?;
    // SAFETY: activation happens only on an explicit camera-on action; Source shuts down on drop.
    unsafe {
        selected
            .SetUINT32(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_MAX_BUFFERS, 1)
            .map_err(|_| UNAVAILABLE)?;
        selected
            .ActivateObject::<IMFMediaSource>()
            .map(Source)
            .map_err(|_| UNAVAILABLE)
    }
}

fn output_stride(reader: &IMFSourceReader) -> R<i32> {
    // SAFETY: thread-owned reader. Check type and dimensions before using its stride.
    unsafe {
        let media = reader.GetCurrentMediaType(VIDEO).map_err(|_| INVALID)?;
        if media.GetUINT64(&MF_MT_FRAME_SIZE).map_err(|_| INVALID)? != frame_size()
            || media.GetGUID(&MF_MT_SUBTYPE).map_err(|_| INVALID)? != MFVideoFormat_RGB32
        {
            return Err(INVALID);
        }
        let stride = media
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map(|stride| stride as i32)
            .or_else(|_| MFGetStrideForBitmapInfoHeader(MFVideoFormat_RGB32.data1, WIDTH as u32))
            .map_err(|_| INVALID)?;
        if !(WIDTH * 4..=WIDTH * 4 + 4096).contains(&(stride.unsigned_abs() as usize)) {
            return Err(INVALID);
        }
        Ok(stride)
    }
}

fn copy_sample(sample: &IMFSample, stride: i32) -> R<Vec<u8>> {
    // SAFETY: the sample is borrowed only during its callback. Validate native byte budgets before mapping and copying;
    // every successful lock is unlocked on all paths.
    unsafe {
        if sample.GetBufferCount().map_err(|_| INVALID)? != 1
            || sample.GetTotalLength().map_err(|_| INVALID)? as usize > MAX_FRAME_BYTES
        {
            return Err(INVALID);
        }
        let buffer = sample.GetBufferByIndex(0).map_err(|_| INVALID)?;
        if buffer.GetMaxLength().map_err(|_| INVALID)? as usize > MAX_FRAME_BYTES {
            return Err(INVALID);
        }
        if let Ok(buffer2d) = buffer.cast::<IMF2DBuffer2>() {
            let (mut top, mut base, mut pitch, mut length) =
                (std::ptr::null_mut(), std::ptr::null_mut(), 0, 0);
            buffer2d
                .Lock2DSize(
                    MF2DBuffer_LockFlags_Read,
                    &mut top,
                    &mut pitch,
                    &mut base,
                    &mut length,
                )
                .map_err(|_| INVALID)?;
            let result = if base.is_null() || top.is_null() || length as usize > MAX_FRAME_BYTES {
                Err(INVALID)
            } else if let Some(first) = (top as usize).checked_sub(base as usize) {
                rgba_rows(
                    std::slice::from_raw_parts(base, length as usize),
                    first,
                    pitch,
                )
            } else {
                Err(INVALID)
            };
            buffer2d.Unlock2D().map_err(|_| INVALID)?;
            return result;
        }
        let (mut base, mut capacity, mut length) = (std::ptr::null_mut(), 0, 0);
        buffer
            .Lock(&mut base, Some(&mut capacity), Some(&mut length))
            .map_err(|_| INVALID)?;
        let result = if base.is_null() || length > capacity || capacity as usize > MAX_FRAME_BYTES {
            Err(INVALID)
        } else {
            let first = if stride < 0 {
                stride.unsigned_abs() as usize * (HEIGHT - 1)
            } else {
                0
            };
            rgba_rows(
                std::slice::from_raw_parts(base, length as usize),
                first,
                stride,
            )
        };
        buffer.Unlock().map_err(|_| INVALID)?;
        result
    }
}

/// 640x480 RGBA from `bytes` holding BGRX rows `stride` apart (negative: bottom-up) starting at byte `first`.
fn rgba_rows(bytes: &[u8], first: usize, stride: i32) -> R<Vec<u8>> {
    let pitch = stride.unsigned_abs() as usize;
    if !(WIDTH * 4..=WIDTH * 4 + 4096).contains(&pitch) || bytes.len() > MAX_FRAME_BYTES {
        return Err(INVALID);
    }
    let last = first
        .checked_add_signed(stride as isize * (HEIGHT - 1) as isize)
        .ok_or(INVALID)?;
    if first
        .max(last)
        .checked_add(WIDTH * 4)
        .is_none_or(|end| end > bytes.len())
    {
        return Err(INVALID);
    }
    let mut rgba = vec![0; WIDTH * HEIGHT * 4];
    for (y, dest) in rgba.chunks_exact_mut(WIDTH * 4).enumerate() {
        let start = first
            .checked_add_signed(stride as isize * y as isize)
            .ok_or(INVALID)?;
        for (pixel, dest) in bytes[start..start + WIDTH * 4]
            .chunks_exact(4)
            .zip(dest.chunks_exact_mut(4))
        {
            dest.copy_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_rows_handle_padding_bottom_up_and_reject_invalid_bounds() {
        let pitch = WIDTH * 4 + 8;
        let mut bytes = vec![0; pitch * HEIGHT];
        bytes[..4].copy_from_slice(&[10, 20, 30, 0]);
        bytes[(HEIGHT - 1) * pitch..(HEIGHT - 1) * pitch + 4].copy_from_slice(&[40, 50, 60, 0]);
        assert_eq!(
            &rgba_rows(&bytes, 0, pitch as i32).unwrap()[..4],
            &[30, 20, 10, 255]
        );
        assert_eq!(
            &rgba_rows(&bytes, (HEIGHT - 1) * pitch, -(pitch as i32)).unwrap()[..4],
            &[60, 50, 40, 255]
        );
        assert!(rgba_rows(&bytes[..pitch], 0, pitch as i32).is_err());
        assert!(rgba_rows(&bytes, 0, -(pitch as i32)).is_err());
        assert!(rgba_rows(&bytes, usize::MAX, pitch as i32).is_err());
        assert!(rgba_rows(&bytes, 0, i32::MIN).is_err());
        assert!(rgba_rows(&bytes, 0, WIDTH as i32 * 4 - 1).is_err());
    }
}
