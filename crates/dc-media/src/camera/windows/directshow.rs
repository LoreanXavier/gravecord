//! Compatibility path for virtual cameras registered only with DirectShow (OBS Virtual Camera is one). Adapted from
//! Serein's `crates/discord-voice/src/camera/windows/directshow.rs` (MIT OR Apache-2.0): the same sample-grabber graph,
//! delivering letterboxed 640x480 RGBA and reporting when the graph is running.
#![allow(unsafe_code)]
#![allow(non_snake_case)] // Custom COM interfaces preserve the Qedit.h method names.

use super::{FRAME_INTERVAL, HEIGHT, INVALID, TIMEOUT, UNAVAILABLE, WIDTH, format};
use crate::camera::{CameraDevice, DeviceRef};
use crate::{Error, Result};
use std::{
    ffi::c_void,
    mem::ManuallyDrop,
    sync::{Mutex, atomic::AtomicBool, atomic::Ordering, mpsc},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Media::{DirectShow::*, MediaFoundation::*},
        System::{
            Com::{StructuredStorage::IPropertyBag, *},
            Variant::{VARIANT, VT_BSTR, VariantClear},
        },
    },
    core::{BOOL, GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, implement, interface, w},
};

type R<T> = std::result::Result<T, &'static str>;

const SAMPLE_GRABBER: GUID = GUID::from_u128(0xc1f400a0_3f08_11d3_9f0b_006008039e37);
const NULL_RENDERER: GUID = GUID::from_u128(0xc1f400a4_3f08_11d3_9f0b_006008039e37);
const MAX_WIDTH: usize = 1920;
const MAX_HEIGHT: usize = 1080;
const MAX_BYTES: usize = MAX_WIDTH * MAX_HEIGHT * 4;

// Qedit.h interfaces are absent from windows-rs metadata. Keep the SDK ABI order.
#[interface("6b652fff-11fe-4fce-92ad-0266b5d7c78f")]
unsafe trait ISampleGrabber: IUnknown {
    fn SetOneShot(&self, value: BOOL) -> HRESULT;
    fn SetMediaType(&self, media: *const AM_MEDIA_TYPE) -> HRESULT;
    fn GetConnectedMediaType(&self, media: *mut AM_MEDIA_TYPE) -> HRESULT;
    fn SetBufferSamples(&self, value: BOOL) -> HRESULT;
    fn GetCurrentBuffer(&self, length: *mut i32, buffer: *mut i32) -> HRESULT;
    fn GetCurrentSample(&self, sample: *mut *mut c_void) -> HRESULT;
    fn SetCallback(&self, callback: *mut c_void, method: i32) -> HRESULT;
}

#[interface("0579154a-2b53-4994-b0d0-e773148eff85")]
unsafe trait ISampleGrabberCB: IUnknown {
    fn SampleCB(&self, time: f64, sample: *mut c_void) -> HRESULT;
    fn BufferCB(&self, time: f64, buffer: *mut u8, length: i32) -> HRESULT;
}

#[implement(ISampleGrabberCB)]
struct Callback {
    send: mpsc::SyncSender<R<Vec<u8>>>,
    last: Mutex<(Instant, bool)>,
    bytes: usize,
    dimensions: (usize, usize, bool),
}
impl ISampleGrabberCB_Impl for Callback_Impl {
    unsafe fn SampleCB(&self, _: f64, sample: *mut c_void) -> HRESULT {
        let Ok(mut last) = self.last.try_lock() else {
            return HRESULT(0);
        };
        // SAFETY: the borrowed sample stays valid until this callback returns. Never retain COM samples or encode on
        // the native streaming thread.
        let result = unsafe {
            (|| {
                if last.1 {
                    return Err(INVALID);
                }
                let sample = IMediaSample::from_raw_borrowed(&sample).ok_or(INVALID)?;
                let length = sample.GetActualDataLength();
                let capacity = sample.GetSize();
                if length < 0
                    || capacity < length
                    || capacity as usize > MAX_BYTES
                    || length as usize != self.bytes
                {
                    return Err(INVALID);
                }
                let changed = sample.GetMediaType().map_err(|_| INVALID)?;
                if !changed.is_null() {
                    let media = Media::take(changed);
                    if rgb_dimensions(&media.0)? != self.dimensions {
                        return Err("Camera changed its video format; restart the camera");
                    }
                }
                if last.0.elapsed() < FRAME_INTERVAL {
                    return Ok(None);
                }
                last.0 = Instant::now();
                let pointer = sample.GetPointer().map_err(|_| INVALID)?;
                if pointer.is_null() {
                    return Err(INVALID);
                }
                Ok(Some(
                    std::slice::from_raw_parts(pointer, length as usize).to_vec(),
                ))
            })()
        };
        // Exactly one frame, at most MAX_BYTES, waits for the worker.
        match result {
            Ok(Some(bytes)) => {
                let _ = self.send.try_send(Ok(bytes));
            }
            Ok(None) => {}
            Err(error) => {
                last.1 = true;
                let _ = self.send.try_send(Err(error));
            }
        }
        HRESULT(0)
    }
    unsafe fn BufferCB(&self, _: f64, _: *mut u8, _: i32) -> HRESULT {
        HRESULT(0)
    }
}

struct Media(AM_MEDIA_TYPE);
impl Media {
    unsafe fn take(pointer: *mut AM_MEDIA_TYPE) -> Self {
        // SAFETY: the caller obtained this owned CoTaskMem block from DirectShow.
        unsafe {
            let media = pointer.read();
            CoTaskMemFree(Some(pointer.cast()));
            Self(media)
        }
    }
}
impl Drop for Media {
    fn drop(&mut self) {
        // SAFETY: DirectShow transfers these two owned fields to the caller.
        unsafe {
            CoTaskMemFree(Some(self.0.pbFormat.cast()));
            ManuallyDrop::drop(&mut self.0.pUnk);
        }
    }
}

fn monikers() -> R<Vec<(String, String, IMoniker)>> {
    // SAFETY: the caller initialized COM on this thread. Binding storage reads only registration metadata;
    // BindToObject is reserved for explicit capture below.
    unsafe {
        let system: ICreateDevEnum =
            CoCreateInstance(&CLSID_SystemDeviceEnum, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| UNAVAILABLE)?;
        let mut enumeration = None;
        system
            .CreateClassEnumerator(&CLSID_VideoInputDeviceCategory, &mut enumeration, 0)
            .map_err(|_| UNAVAILABLE)?;
        let Some(enumeration) = enumeration else {
            return Ok(Vec::new());
        };
        let mut devices = Vec::new();
        for _ in 0..32 {
            let mut entry = [None];
            if enumeration.Next(&mut entry, None) != HRESULT(0) {
                break;
            }
            let Some(moniker) = entry[0].take() else {
                break;
            };
            let Ok(display) = moniker.GetDisplayName(None, None) else {
                continue;
            };
            let id = if display.is_null() {
                None
            } else {
                let mut length = 0;
                while length <= 4096 && *display.0.add(length) != 0 {
                    length += 1;
                }
                (length <= 4096).then(|| {
                    String::from_utf16_lossy(std::slice::from_raw_parts(display.0, length))
                })
            };
            CoTaskMemFree(Some(display.0.cast()));
            let Some(id) = id.filter(|id| !id.is_empty() && id.len() <= 4090) else {
                continue;
            };
            let Ok(bag) = moniker.BindToStorage::<_, _, IPropertyBag>(None, None) else {
                continue;
            };
            let mut value = VARIANT::default();
            let read = bag.Read(w!("FriendlyName"), &mut value, None);
            let name = if read.is_ok() && value.Anonymous.Anonymous.vt == VT_BSTR {
                let bstr = &value.Anonymous.Anonymous.Anonymous.bstrVal;
                (bstr.len() <= 256).then(|| bstr.to_string())
            } else {
                None
            };
            let _ = VariantClear(&mut value);
            let name = name
                .filter(|name| !name.is_empty() && name.len() <= 256 && !name.contains('\0'))
                .unwrap_or_else(|| "Windows camera".into());
            devices.push((format!("dshow:{id}"), name, moniker));
        }
        Ok(devices)
    }
}

pub(super) fn devices() -> R<Vec<CameraDevice>> {
    Ok(monikers()?
        .into_iter()
        .map(|(id, name, _)| CameraDevice { id, name })
        .collect())
}

fn dimensions(media: &AM_MEDIA_TYPE) -> R<(usize, usize, bool)> {
    if media.majortype != MEDIATYPE_Video
        || media.pbFormat.is_null()
        || media.cbFormat as usize > 65536
    {
        return Err(INVALID);
    }
    // SAFETY: validate the advertised format block size before reading its header; read_unaligned also handles filters
    // which do not align their format blocks.
    let bitmap = unsafe {
        if media.formattype == FORMAT_VideoInfo
            && media.cbFormat as usize >= size_of::<VIDEOINFOHEADER>()
        {
            media
                .pbFormat
                .cast::<VIDEOINFOHEADER>()
                .read_unaligned()
                .bmiHeader
        } else if media.formattype == FORMAT_VideoInfo2
            && media.cbFormat as usize >= size_of::<VIDEOINFOHEADER2>()
        {
            media
                .pbFormat
                .cast::<VIDEOINFOHEADER2>()
                .read_unaligned()
                .bmiHeader
        } else {
            return Err(INVALID);
        }
    };
    if !(1..=MAX_WIDTH as i32).contains(&bitmap.biWidth)
        || !(1..=MAX_HEIGHT as u32).contains(&bitmap.biHeight.unsigned_abs())
        || bitmap.biSizeImage as usize > MAX_BYTES
    {
        return Err(INVALID);
    }
    Ok((
        bitmap.biWidth as usize,
        bitmap.biHeight.unsigned_abs() as usize,
        bitmap.biHeight > 0,
    ))
}

fn rgb_dimensions(media: &AM_MEDIA_TYPE) -> R<(usize, usize, bool)> {
    let dimensions = dimensions(media)?;
    if media.subtype != MEDIASUBTYPE_RGB24 || media.formattype != FORMAT_VideoInfo {
        return Err(INVALID);
    }
    // SAFETY: `dimensions` validated the complete VIDEOINFOHEADER allocation.
    let bitmap = unsafe {
        media
            .pbFormat
            .cast::<VIDEOINFOHEADER>()
            .read_unaligned()
            .bmiHeader
    };
    if bitmap.biBitCount != 24 || bitmap.biPlanes != 1 || bitmap.biCompression != 0 {
        return Err(INVALID);
    }
    Ok(dimensions)
}

/// Whether the source's first output pin lists at least one media type.
fn offers_a_format(source: &IBaseFilter) -> bool {
    let Some(output) = pin(source, PINDIR_OUTPUT) else {
        return false;
    };
    // SAFETY: plain enumeration; each returned type is an owned CoTaskMem block freed by `Media`.
    unsafe {
        let Ok(types) = output.EnumMediaTypes() else {
            return false;
        };
        let mut found = [std::ptr::null_mut::<AM_MEDIA_TYPE>()];
        let mut count = 0u32;
        let ok = types.Next(&mut found, Some(&mut count)) == HRESULT(0) && !found[0].is_null();
        if ok {
            drop(Media::take(found[0]));
        }
        ok
    }
}

/// The first pin of `filter` that points `direction`.
fn pin(filter: &IBaseFilter, direction: PIN_DIRECTION) -> Option<IPin> {
    // SAFETY: plain enumeration of a filter this thread owns.
    unsafe {
        let pins = filter.EnumPins().ok()?;
        for _ in 0..16 {
            let mut found = [None];
            if pins.Next(&mut found, None) != HRESULT(0) {
                break;
            }
            let candidate = found[0].take()?;
            if candidate.QueryDirection().ok()? == direction {
                return Some(candidate);
            }
        }
        None
    }
}

fn configure(builder: &ICaptureGraphBuilder2, source: &IBaseFilter) -> R<()> {
    // SAFETY: owned graph on this thread, only bounded native formats may be selected.
    unsafe {
        let mut pointer = std::ptr::null_mut();
        builder
            .FindInterface(
                Some(&PIN_CATEGORY_CAPTURE),
                Some(&MEDIATYPE_Video),
                source,
                &IAMStreamConfig::IID,
                &mut pointer,
            )
            .ok();
        // Virtual cameras (OBS) may not expose IAMStreamConfig or capabilities: keep the filter's own default format;
        // the bounded checks on the negotiated type and allocator still apply.
        if pointer.is_null() {
            return Ok(());
        }
        let config = IAMStreamConfig::from_raw(pointer);
        let (mut count, mut size) = (0, 0);
        if config
            .GetNumberOfCapabilities(&mut count, &mut size)
            .is_err()
        {
            return Ok(());
        }
        if !(1..=256).contains(&count) || !(1..=4096).contains(&size) {
            return Ok(());
        }
        let mut capabilities = vec![0; size as usize];
        let mut choices = Vec::new();
        for index in 0..count {
            let mut pointer = std::ptr::null_mut();
            capabilities.fill(0);
            let result = config.GetStreamCaps(index, &mut pointer, capabilities.as_mut_ptr());
            if pointer.is_null() {
                continue;
            }
            let media = Media::take(pointer);
            if result.is_err() {
                continue;
            }
            if let Ok((width, height, _)) = dimensions(&media.0) {
                // `dimensions` checked the header size; the capability block may be unaligned.
                let interval = if media.0.formattype == FORMAT_VideoInfo {
                    std::ptr::addr_of_mut!(
                        (*media.0.pbFormat.cast::<VIDEOINFOHEADER>()).AvgTimePerFrame
                    )
                } else {
                    std::ptr::addr_of_mut!(
                        (*media.0.pbFormat.cast::<VIDEOINFOHEADER2>()).AvgTimePerFrame
                    )
                };
                if size as usize >= size_of::<VIDEO_STREAM_CONFIG_CAPS>() {
                    let caps = capabilities
                        .as_ptr()
                        .cast::<VIDEO_STREAM_CONFIG_CAPS>()
                        .read_unaligned();
                    if let Some(fps) = format::nearest_fps(
                        10_000_000.0 / caps.MaxFrameInterval as f64,
                        10_000_000.0 / caps.MinFrameInterval as f64,
                    ) {
                        interval.write_unaligned(
                            ((10_000_000.0 / fps).round() as i64)
                                .clamp(caps.MinFrameInterval, caps.MaxFrameInterval),
                        );
                    }
                }
                let duration = interval.read_unaligned();
                let fps = (duration > 0).then(|| 10_000_000.0 / duration as f64);
                if let Some(mut rank) = format::rank_with_ceiling(
                    width,
                    height,
                    fps.unwrap_or(f64::from(crate::camera::FPS)),
                    MAX_WIDTH,
                    MAX_HEIGHT,
                ) {
                    // Keep the bounded fallback when the driver omits timing: unknown rates follow measured ones at
                    // the same native resolution.
                    if fps.is_none() {
                        rank.1 = u64::MAX;
                    }
                    choices.push((rank, media));
                }
            }
        }
        choices.sort_by_key(|(rank, _)| *rank);
        for (_, media) in choices {
            if config.SetFormat(&media.0).is_ok() {
                return Ok(());
            }
        }
        Err("Camera does not offer a supported capture mode up to 1920x1080")
    }
}

struct Capture {
    control: IMediaControl,
    grabber: ISampleGrabber,
    callback: Option<ISampleGrabberCB>,
}
impl Drop for Capture {
    fn drop(&mut self) {
        // SAFETY: Stop drains streaming callbacks before the thread releases graph references. The callback itself
        // owns all state needed by an in-flight call.
        unsafe {
            let _ = self.control.Stop();
            let _ = self.grabber.SetCallback(std::ptr::null_mut(), 0);
        }
    }
}

/// Runs the DirectShow graph on this thread in a single-threaded apartment.
pub(super) fn run(
    stop: &AtomicBool,
    device: Option<DeviceRef<'_>>,
    opened: &mut dyn FnMut(),
    emit: &mut dyn FnMut(Vec<u8>) -> Result<()>,
) -> Result<()> {
    // SAFETY: balanced with CoUninitialize below on this thread, after every COM object of `run_graph` was dropped.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|e| Error::unavailable(format!("COM could not start on this thread: {e}")))?;
    let result = run_graph(stop, device, opened, emit);
    // SAFETY: matches the CoInitializeEx above.
    unsafe { CoUninitialize() };
    result
}

fn run_graph(
    stop: &AtomicBool,
    device: Option<DeviceRef<'_>>,
    opened: &mut dyn FnMut(),
    emit: &mut dyn FnMut(Vec<u8>) -> Result<()>,
) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let wanted = match device {
        Some(DeviceRef::DirectShow(name)) => Some(format!("dshow:{name}")),
        _ => None,
    };
    let (_, _, moniker) = monikers()?
        .into_iter()
        .find(|(id, _, _)| wanted.as_deref().is_none_or(|selected| selected == id))
        .ok_or("Selected Windows camera is no longer available")?;
    // SAFETY: the only activation path, called after an explicit camera-on gesture. Graph, format negotiation and
    // teardown all remain on this thread.
    let (capture, receive, width, height, bottom_up) = unsafe {
        let source: IBaseFilter = moniker.BindToObject(None, None).map_err(|_| UNAVAILABLE)?;
        let graph: IGraphBuilder = CoCreateInstance(&CLSID_FilterGraph, None, CLSCTX_INPROC_SERVER)
            .map_err(|_| UNAVAILABLE)?;
        let builder: ICaptureGraphBuilder2 =
            CoCreateInstance(&CLSID_CaptureGraphBuilder2, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| UNAVAILABLE)?;
        builder.SetFiltergraph(&graph).map_err(|_| UNAVAILABLE)?;
        graph
            .AddFilter(&source, w!("Camera"))
            .map_err(|_| UNAVAILABLE)?;
        // A virtual camera whose app is not running offers no format at all, and OBS's filter then crashes the host
        // process when a connection is attempted; refuse before connecting anything.
        if !offers_a_format(&source) {
            return Err(
                "This camera offers no video format; start the virtual camera in its own app first"
                    .into(),
            );
        }
        configure(&builder, &source)?;
        let filter: IBaseFilter = CoCreateInstance(&SAMPLE_GRABBER, None, CLSCTX_INPROC_SERVER)
            .map_err(|_| UNAVAILABLE)?;
        let grabber: ISampleGrabber = filter.cast().map_err(|_| UNAVAILABLE)?;
        let mut capture = Capture {
            control: graph.cast().map_err(|_| UNAVAILABLE)?,
            grabber,
            callback: None,
        };
        let requested = AM_MEDIA_TYPE {
            majortype: MEDIATYPE_Video,
            subtype: MEDIASUBTYPE_RGB24,
            formattype: FORMAT_VideoInfo,
            ..Default::default()
        };
        capture
            .grabber
            .SetMediaType(&requested)
            .ok()
            .map_err(|_| INVALID)?;
        capture
            .grabber
            .SetBufferSamples(BOOL(0))
            .ok()
            .map_err(|_| INVALID)?;
        graph
            .AddFilter(&filter, w!("Camera frames"))
            .map_err(|_| UNAVAILABLE)?;
        let sink: IBaseFilter = CoCreateInstance(&NULL_RENDERER, None, CLSCTX_INPROC_SERVER)
            .map_err(|_| UNAVAILABLE)?;
        graph
            .AddFilter(&sink, w!("Camera sink"))
            .map_err(|_| UNAVAILABLE)?;
        // Virtual cameras often do not tag their pin with the capture category, which RenderStream requires; wire
        // the graph pin by pin then (the graph's intelligent connect still inserts any colour converter).
        if builder
            .RenderStream(
                Some(&PIN_CATEGORY_CAPTURE),
                &MEDIATYPE_Video,
                &source,
                &filter,
                &sink,
            )
            .is_err()
        {
            let wire = |from: &IBaseFilter, to: &IBaseFilter| -> R<()> {
                let output = pin(from, PINDIR_OUTPUT).ok_or(UNAVAILABLE)?;
                let input = pin(to, PINDIR_INPUT).ok_or(UNAVAILABLE)?;
                graph
                    .Connect(&output, &input)
                    .map_err(|_| "Windows could not convert this camera to RGB video")
            };
            wire(&source, &filter)?;
            wire(&filter, &sink)?;
        }
        let mut media = Media(AM_MEDIA_TYPE::default());
        capture
            .grabber
            .GetConnectedMediaType(&mut media.0)
            .ok()
            .map_err(|_| INVALID)?;
        let (width, height, bottom_up) = rgb_dimensions(&media.0)?;
        let bytes = (width * 3).next_multiple_of(4) * height;
        // Check the actual negotiated allocator before any filter starts delivering.
        let pins = filter.EnumPins().map_err(|_| INVALID)?;
        let mut bounded_allocator = false;
        for _ in 0..16 {
            let mut pin = [None];
            if pins.Next(&mut pin, None) != HRESULT(0) {
                break;
            }
            let Some(pin) = pin[0].take() else { break };
            if let Ok(input) = pin.cast::<IMemInputPin>() {
                let properties = input
                    .GetAllocator()
                    .and_then(|allocator| allocator.GetProperties())
                    .map_err(|_| INVALID)?;
                if !(1..=8).contains(&properties.cBuffers)
                    || properties.cbBuffer < bytes as i32
                    || properties.cbBuffer as usize > MAX_BYTES
                    || !(0..=4096).contains(&properties.cbPrefix)
                {
                    return Err(INVALID.into());
                }
                bounded_allocator = true;
            }
        }
        if !bounded_allocator {
            return Err(INVALID.into());
        }
        let (send, receive) = mpsc::sync_channel(1);
        let callback: ISampleGrabberCB = Callback {
            send,
            last: Mutex::new((Instant::now() - FRAME_INTERVAL, false)),
            bytes,
            dimensions: (width, height, bottom_up),
        }
        .into();
        capture
            .grabber
            .SetCallback(callback.as_raw(), 0)
            .ok()
            .map_err(|_| INVALID)?;
        capture.callback = Some(callback);
        if !stop.load(Ordering::Acquire) {
            capture.control.Run().map_err(|_| UNAVAILABLE)?;
        }
        (capture, receive, width, height, bottom_up)
    };
    opened();
    let mut last = Instant::now();
    while !stop.load(Ordering::Acquire) {
        match receive.recv_timeout(Duration::from_millis(50)) {
            Ok(bytes) => {
                let rgba = rgba_frame(&bytes?, width, height, bottom_up)?;
                last = Instant::now();
                if !stop.load(Ordering::Acquire) {
                    emit(rgba)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) if last.elapsed() < Duration::from_secs(5) => {}
            _ => return Err(Error::from(TIMEOUT)),
        }
    }
    drop(capture);
    Ok(())
}

/// A 24-bit BGR frame with rows padded to four bytes, as 640x480 RGBA: the aspect ratio is kept, the rest is black, and
/// a bottom-up frame is turned over. Nearest-neighbour; replace it with filtered scaling if virtual-camera quality
/// measurements ask for it.
fn rgba_frame(bytes: &[u8], width: usize, height: usize, bottom_up: bool) -> R<Vec<u8>> {
    if !(1..=MAX_WIDTH).contains(&width) || !(1..=MAX_HEIGHT).contains(&height) {
        return Err(INVALID);
    }
    let pitch = (width * 3).next_multiple_of(4);
    if bytes.len() != pitch * height || bytes.len() > MAX_BYTES {
        return Err(INVALID);
    }
    let mut rgba = vec![0; WIDTH * HEIGHT * 4];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    let (draw_width, draw_height) = if width * HEIGHT > height * WIDTH {
        (WIDTH, (height * WIDTH / width).max(1))
    } else {
        ((width * HEIGHT / height).max(1), HEIGHT)
    };
    let left = (WIDTH - draw_width) / 2;
    let top = (HEIGHT - draw_height) / 2;
    for y in 0..draw_height {
        let source_y = y * height / draw_height;
        let source_y = if bottom_up {
            height - 1 - source_y
        } else {
            source_y
        };
        for x in 0..draw_width {
            let source = source_y * pitch + (x * width / draw_width) * 3;
            let dest = ((top + y) * WIDTH + left + x) * 4;
            rgba[dest..dest + 3].copy_from_slice(&[
                bytes[source + 2],
                bytes[source + 1],
                bytes[source],
            ]);
        }
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_conversion_bounds_padding_orientation_and_resize() {
        let mut header = VIDEOINFOHEADER::default();
        header.bmiHeader.biWidth = WIDTH as i32;
        header.bmiHeader.biHeight = HEIGHT as i32;
        header.bmiHeader.biPlanes = 1;
        header.bmiHeader.biBitCount = 24;
        let mut media = AM_MEDIA_TYPE {
            majortype: MEDIATYPE_Video,
            subtype: MEDIASUBTYPE_RGB24,
            formattype: FORMAT_VideoInfo,
            cbFormat: size_of::<VIDEOINFOHEADER>() as u32,
            pbFormat: (&mut header as *mut VIDEOINFOHEADER).cast(),
            ..Default::default()
        };
        assert_eq!(rgb_dimensions(&media).unwrap(), (WIDTH, HEIGHT, true));
        media.cbFormat -= 1;
        assert!(rgb_dimensions(&media).is_err());
        media.cbFormat += 1;
        media.subtype = MEDIASUBTYPE_RGB32;
        assert!(rgb_dimensions(&media).is_err());
        // Two rows of one BGR pixel, padded to four bytes.
        let bytes = [1, 2, 3, 0, 4, 5, 6, 0];
        let top = rgba_frame(&bytes, 1, 2, false).unwrap();
        let bottom = rgba_frame(&bytes, 1, 2, true).unwrap();
        let center = WIDTH / 2 * 4;
        assert_eq!(&top[center..center + 4], &[3, 2, 1, 255]);
        assert_eq!(&bottom[center..center + 4], &[6, 5, 4, 255]);
        let end = (HEIGHT - 1) * WIDTH * 4 + center;
        assert_eq!(&top[end..end + 4], &[6, 5, 4, 255]);
        // The pillars are opaque black.
        assert_eq!(&top[..4], &[0, 0, 0, 255]);
        let wide = rgba_frame(&vec![255; 16 * 3 * 9], 16, 9, false).unwrap();
        assert_eq!(&wide[..4], &[0, 0, 0, 255]);
        assert_eq!(&wide[WIDTH * 60 * 4..][..4], &[255, 255, 255, 255]);
        assert_eq!(&wide[WIDTH * 419 * 4..][..4], &[255, 255, 255, 255]);
        assert_eq!(&wide[WIDTH * 420 * 4..][..4], &[0, 0, 0, 255]);
        for (width, height) in [(0, 2), (1, 0), (1921, 1), (1, 1081), (usize::MAX, 1)] {
            assert!(rgba_frame(&bytes, width, height, false).is_err());
        }
        assert!(rgba_frame(&bytes[..7], 1, 2, false).is_err());
        assert!(rgba_frame(&bytes, 1, 1, false).is_err());
    }
}
