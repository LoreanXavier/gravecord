//! Compatibility path for virtual cameras registered only with DirectShow (OBS Virtual Camera is one). Adapted from
//! Serein's `crates/discord-voice/src/camera/windows/directshow.rs` (MIT OR Apache-2.0): the same sample-grabber graph,
//! delivering letterboxed 640x480 RGBA and reporting when the graph is running.
#![allow(unsafe_code)]
#![allow(non_snake_case)] // Custom COM interfaces preserve the Qedit.h method names.

use super::{FRAME_INTERVAL, HEIGHT, TIMEOUT, UNAVAILABLE, WIDTH, format};
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
const NV12: GUID = GUID::from_u128(0x3231564e_0000_0010_8000_00aa00389b71);
const YUY2: GUID = GUID::from_u128(0x32595559_0000_0010_8000_00aa00389b71);
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
    format: Format,
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
                    return Err("Camera graph already reported an error");
                }
                let sample = IMediaSample::from_raw_borrowed(&sample)
                    .ok_or("Camera delivered a sample the graph could not read")?;
                let length = sample.GetActualDataLength();
                let capacity = sample.GetSize();
                if length < 0
                    || capacity < length
                    || capacity as usize > MAX_BYTES
                    || length as usize != self.bytes
                {
                    return Err("Camera sample size does not match the negotiated RGB24 frame");
                }
                let changed = sample
                    .GetMediaType()
                    .map_err(|_| "Camera sample media type could not be read")?;
                if !changed.is_null() {
                    let media = Media::take(changed);
                    if frame_format(&media.0)? != self.format {
                        return Err("Camera changed its video format; restart the camera");
                    }
                }
                if last.0.elapsed() < FRAME_INTERVAL {
                    return Ok(None);
                }
                last.0 = Instant::now();
                let pointer = sample
                    .GetPointer()
                    .map_err(|_| "Camera sample data could not be mapped")?;
                if pointer.is_null() {
                    return Err("Camera sample has no data pointer");
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

/// How the sample grabber hands us a frame. OBS's virtual camera offers only 1920x1080 NV12, YUY2 and I420: asking the
/// grabber for those directly avoids the Smart Tee and AVI Decompressor which `RenderStream` puts in front of it, and
/// through which no frame of that camera ever arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    /// 24-bit BGR, rows padded to four bytes.
    Rgb24,
    /// Planar luma, then interleaved U and V at half resolution.
    Nv12,
    /// Packed Y0 U Y1 V.
    Yuy2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Format {
    layout: Layout,
    width: usize,
    height: usize,
    /// Rows run from the bottom of the picture up (RGB only; YUV frames are always top-down).
    bottom_up: bool,
}
impl Format {
    fn bytes(&self) -> usize {
        match self.layout {
            Layout::Rgb24 => (self.width * 3).next_multiple_of(4) * self.height,
            Layout::Nv12 => self.width * self.height * 3 / 2,
            Layout::Yuy2 => self.width * self.height * 2,
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
        return Err("Camera media type is not video or has no format block");
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
            return Err("Camera video format header is neither VideoInfo nor VideoInfo2");
        }
    };
    if !(1..=MAX_WIDTH as i32).contains(&bitmap.biWidth)
        || !(1..=MAX_HEIGHT as u32).contains(&bitmap.biHeight.unsigned_abs())
        || bitmap.biSizeImage as usize > MAX_BYTES
    {
        return Err("Camera frame size is outside 1x1 to 1920x1080");
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
        return Err("Camera output is not RGB24 VideoInfo");
    }
    // SAFETY: `dimensions` validated the complete VIDEOINFOHEADER allocation.
    let bitmap = unsafe {
        media
            .pbFormat
            .cast::<VIDEOINFOHEADER>()
            .read_unaligned()
            .bmiHeader
    };
    // biPlanes is not checked: the colour converter OBS's NV12 goes through reports 3 for its RGB24 output.
    if bitmap.biBitCount != 24 || bitmap.biCompression != 0 {
        return Err("Camera RGB frame is not 24-bit uncompressed");
    }
    Ok(dimensions)
}

/// The layout and size of a connected media type, whichever of the supported subtypes it is.
fn frame_format(media: &AM_MEDIA_TYPE) -> R<Format> {
    if media.subtype == MEDIASUBTYPE_RGB24 {
        let (width, height, bottom_up) = rgb_dimensions(media)?;
        return Ok(Format {
            layout: Layout::Rgb24,
            width,
            height,
            bottom_up,
        });
    }
    let (width, height, _) = dimensions(media)?;
    let layout = if media.subtype == NV12 {
        Layout::Nv12
    } else if media.subtype == YUY2 {
        Layout::Yuy2
    } else {
        return Err("Camera output is neither RGB24, NV12 nor YUY2");
    };
    if width % 2 != 0 || height % 2 != 0 {
        return Err("Camera YUV frame has an odd width or height");
    }
    Ok(Format {
        layout,
        width,
        height,
        bottom_up: false,
    })
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
        .ok_or(crate::camera::NOT_FOUND)?;
    // SAFETY: the only activation path, called after an explicit camera-on gesture. Graph, format negotiation and
    // teardown all remain on this thread.
    let (capture, receive, format) = unsafe {
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
        capture
            .grabber
            .SetBufferSamples(BOOL(0))
            .ok()
            .map_err(|_| "Sample grabber refused to disable buffering")?;
        graph
            .AddFilter(&filter, w!("Camera frames"))
            .map_err(|_| UNAVAILABLE)?;
        let sink: IBaseFilter = CoCreateInstance(&NULL_RENDERER, None, CLSCTX_INPROC_SERVER)
            .map_err(|_| UNAVAILABLE)?;
        graph
            .AddFilter(&sink, w!("Camera sink"))
            .map_err(|_| UNAVAILABLE)?;
        let ask = |subtype: GUID, formattype: GUID| -> R<()> {
            let requested = AM_MEDIA_TYPE {
                majortype: MEDIATYPE_Video,
                subtype,
                formattype,
                ..Default::default()
            };
            capture
                .grabber
                .SetMediaType(&requested)
                .ok()
                .map_err(|_| "Sample grabber refused the requested media type")
        };
        let wire = |from: &IBaseFilter, to: &IBaseFilter| -> R<()> {
            let output = pin(from, PINDIR_OUTPUT).ok_or("Camera filter has no output pin")?;
            let input = pin(to, PINDIR_INPUT).ok_or("Graph filter has no input pin")?;
            graph
                .Connect(&output, &input)
                .map_err(|_| "Windows could not connect this camera to the frame grabber")
        };
        // Pin by pin first, taking the camera's own YUV or RGB format: no converter is involved and a virtual camera
        // which only lists YUV connects. A format the camera does not list fails the connection without side effects.
        let mut direct = false;
        for (subtype, formattype) in [
            (NV12, GUID::zeroed()),
            (YUY2, GUID::zeroed()),
            (MEDIASUBTYPE_RGB24, FORMAT_VideoInfo),
        ] {
            ask(subtype, formattype)?;
            if wire(&source, &filter).is_ok() {
                direct = true;
                break;
            }
        }
        if direct {
            wire(&filter, &sink)?;
        } else {
            // The capture-category route lets the graph insert a colour converter to RGB24 (a Smart Tee and an AVI
            // Decompressor in practice), for cameras which connect no other way.
            ask(MEDIASUBTYPE_RGB24, FORMAT_VideoInfo)?;
            builder
                .RenderStream(
                    Some(&PIN_CATEGORY_CAPTURE),
                    &MEDIATYPE_Video,
                    &source,
                    &filter,
                    &sink,
                )
                .map_err(|_| "Windows could not convert this camera to RGB video")?;
        }
        let mut media = Media(AM_MEDIA_TYPE::default());
        capture
            .grabber
            .GetConnectedMediaType(&mut media.0)
            .ok()
            .map_err(|_| "Sample grabber has no connected media type after the graph was wired")?;
        let format = frame_format(&media.0)?;
        let bytes = format.bytes();
        // Check the actual negotiated allocator before any filter starts delivering.
        let pins = filter
            .EnumPins()
            .map_err(|_| "Sample grabber pins could not be listed")?;
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
                    .map_err(|_| "Sample grabber input has no readable allocator")?;
                if !(1..=8).contains(&properties.cBuffers)
                    || properties.cbBuffer < bytes as i32
                    || properties.cbBuffer as usize > MAX_BYTES
                    || !(0..=4096).contains(&properties.cbPrefix)
                {
                    return Err(
                        "Sample grabber allocator is outside the bounded buffer size".into(),
                    );
                }
                bounded_allocator = true;
            }
        }
        if !bounded_allocator {
            return Err("Sample grabber has no memory input pin".into());
        }
        let (send, receive) = mpsc::sync_channel(1);
        let callback: ISampleGrabberCB = Callback {
            send,
            last: Mutex::new((Instant::now() - FRAME_INTERVAL, false)),
            bytes,
            format,
        }
        .into();
        capture
            .grabber
            .SetCallback(callback.as_raw(), 0)
            .ok()
            .map_err(|_| "Sample grabber refused the frame callback")?;
        capture.callback = Some(callback);
        if !stop.load(Ordering::Acquire) {
            // A live source stamps samples on its own clock. With a graph clock the null renderer holds each sample
            // until its stamp comes round, which stalls the source after the first frame.
            if let Ok(media_filter) = graph.cast::<IMediaFilter>() {
                let _ = media_filter.SetSyncSource(None);
            }
            capture.control.Run().map_err(|_| UNAVAILABLE)?;
        }
        (capture, receive, format)
    };
    opened();
    let mut last = Instant::now();
    while !stop.load(Ordering::Acquire) {
        match receive.recv_timeout(Duration::from_millis(50)) {
            Ok(bytes) => {
                let rgba = convert_frame(&bytes?, &format)?;
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

fn convert_frame(bytes: &[u8], format: &Format) -> R<Vec<u8>> {
    match format.layout {
        Layout::Rgb24 => rgba_frame(bytes, format.width, format.height, format.bottom_up),
        Layout::Nv12 | Layout::Yuy2 => rgba_yuv(bytes, format),
    }
}

/// `width` x `height` scaled to fit 640x480 RGBA: the aspect ratio is kept, the rest is opaque black. `pixel` gives the
/// RGB of the source pixel at (x, y), top row first. Nearest-neighbour; replace it with filtered scaling if
/// virtual-camera quality measurements ask for it.
fn letterbox(width: usize, height: usize, pixel: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
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
        for x in 0..draw_width {
            let dest = ((top + y) * WIDTH + left + x) * 4;
            rgba[dest..dest + 3].copy_from_slice(&pixel(x * width / draw_width, source_y));
        }
    }
    rgba
}

/// A 24-bit BGR frame with rows padded to four bytes, as 640x480 RGBA; a bottom-up frame is turned over.
fn rgba_frame(bytes: &[u8], width: usize, height: usize, bottom_up: bool) -> R<Vec<u8>> {
    if !(1..=MAX_WIDTH).contains(&width) || !(1..=MAX_HEIGHT).contains(&height) {
        return Err("Camera frame size is outside 1x1 to 1920x1080 before conversion");
    }
    let pitch = (width * 3).next_multiple_of(4);
    if bytes.len() != pitch * height || bytes.len() > MAX_BYTES {
        return Err("Camera frame length does not match its width, height and row padding");
    }
    Ok(letterbox(width, height, |x, y| {
        let y = if bottom_up { height - 1 - y } else { y };
        let source = y * pitch + x * 3;
        [bytes[source + 2], bytes[source + 1], bytes[source]]
    }))
}

/// Limited-range YCbCr to RGB: BT.709 for HD frames, BT.601 below.
fn yuv_rgb(y: u8, u: u8, v: u8, hd: bool) -> [u8; 3] {
    let c = 1.164 * (f32::from(y) - 16.0);
    let (d, e) = (f32::from(u) - 128.0, f32::from(v) - 128.0);
    let (r, g, b) = if hd {
        (c + 1.793 * e, c - 0.213 * d - 0.533 * e, c + 2.112 * d)
    } else {
        (c + 1.596 * e, c - 0.392 * d - 0.813 * e, c + 2.017 * d)
    };
    [r, g, b].map(|channel| channel.round().clamp(0.0, 255.0) as u8)
}

/// An NV12 or YUY2 frame as 640x480 RGBA.
fn rgba_yuv(bytes: &[u8], format: &Format) -> R<Vec<u8>> {
    let (width, height) = (format.width, format.height);
    if !(2..=MAX_WIDTH).contains(&width)
        || !(2..=MAX_HEIGHT).contains(&height)
        || width % 2 != 0
        || height % 2 != 0
    {
        return Err("Camera YUV frame size is outside 2x2 to 1920x1080 or odd");
    }
    if bytes.len() != format.bytes() {
        return Err("Camera YUV frame length does not match its width and height");
    }
    let hd = height >= 720;
    Ok(match format.layout {
        Layout::Nv12 => letterbox(width, height, |x, y| {
            let chroma = width * height + (y / 2) * width + (x & !1);
            yuv_rgb(bytes[y * width + x], bytes[chroma], bytes[chroma + 1], hd)
        }),
        _ => letterbox(width, height, |x, y| {
            let pair = (y * width + (x & !1)) * 2;
            yuv_rgb(
                bytes[(y * width + x) * 2],
                bytes[pair + 1],
                bytes[pair + 3],
                hd,
            )
        }),
    })
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

    #[test]
    fn yuv_frames_of_a_1080p_virtual_camera_scale_to_640x480() {
        // Limited-range black, white and a saturated red in BT.709.
        assert_eq!(yuv_rgb(16, 128, 128, true), [0, 0, 0]);
        assert_eq!(yuv_rgb(235, 128, 128, true), [255, 255, 255]);
        let red = yuv_rgb(63, 102, 240, true);
        assert!(red[0] > 240 && red[1] < 16 && red[2] < 16, "{red:?}");
        // 1920x1080 NV12: top half white, bottom half black.
        let nv12 = Format {
            layout: Layout::Nv12,
            width: 1920,
            height: 1080,
            bottom_up: false,
        };
        let mut frame = vec![16u8; nv12.bytes()];
        frame[..1920 * 540].fill(235);
        frame[1920 * 1080..].fill(128);
        assert_eq!(frame.len(), 1920 * 1080 * 3 / 2);
        let rgba = convert_frame(&frame, &nv12).unwrap();
        assert_eq!(rgba.len(), WIDTH * HEIGHT * 4);
        // 16:9 into 4:3 leaves 60 black rows above and below (480 - 360) / 2.
        assert_eq!(&rgba[..4], &[0, 0, 0, 255]);
        assert_eq!(&rgba[WIDTH * 60 * 4..][..4], &[255, 255, 255, 255]);
        assert_eq!(&rgba[WIDTH * 239 * 4..][..4], &[255, 255, 255, 255]);
        assert_eq!(&rgba[WIDTH * 240 * 4..][..4], &[0, 0, 0, 255]);
        assert_eq!(&rgba[WIDTH * 419 * 4..][..4], &[0, 0, 0, 255]);
        // YUY2 holds the same picture: Y0 U Y1 V.
        let yuy2 = Format {
            layout: Layout::Yuy2,
            ..nv12
        };
        let mut packed = vec![0u8; yuy2.bytes()];
        for (row, line) in packed.chunks_exact_mut(1920 * 2).enumerate() {
            let luma = if row < 540 { 235 } else { 16 };
            for quad in line.chunks_exact_mut(4) {
                quad.copy_from_slice(&[luma, 128, luma, 128]);
            }
        }
        assert_eq!(convert_frame(&packed, &yuy2).unwrap(), rgba);
        // Wrong lengths and odd sizes are refused.
        assert!(convert_frame(&frame[1..], &nv12).is_err());
        assert!(convert_frame(&packed[..packed.len() - 2], &yuy2).is_err());
        let odd = Format {
            width: 1919,
            ..nv12
        };
        assert!(rgba_yuv(&frame, &odd).is_err());
    }
}
