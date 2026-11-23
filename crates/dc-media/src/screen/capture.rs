//! Windows Graphics Capture written directly over the `windows` crate: a free-threaded frame pool on a D3D11 device,
//! one staging texture, and tightly packed BGRA frames handed to a callback. Serein uses the `windows-capture` crate
//! (`crates/discord-voice/src/screen/capture_windows.rs`, MIT OR Apache-2.0) which needs `windows` 0.61; this does the
//! same job on 0.62 so the workspace keeps one version. The consent handling and the error wording are Serein's.
#![allow(unsafe_code)]

use super::SourceId;
use crate::{Error, Result};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{
            Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind,
            GraphicsCaptureItem, GraphicsCaptureSession,
        },
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus,
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::*,
            Dxgi::{Common::DXGI_SAMPLE_DESC, IDXGIDevice},
            Gdi::HMONITOR,
        },
        System::Com::CoIncrementMTAUsage,
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RO_INIT_MULTITHREADED, RoInitialize,
        },
    },
    core::{IInspectable, Interface},
};

const FRAME_POOL_LIMIT: usize = 4;
const POOL_BUFFERS: i32 = 3;

struct ClosingFrame(Option<windows::Graphics::Capture::Direct3D11CaptureFrame>);

impl ClosingFrame {
    fn as_ref(&self) -> Option<&windows::Graphics::Capture::Direct3D11CaptureFrame> {
        self.0.as_ref()
    }

    fn close_now(&mut self) {
        if let Some(frame) = self.0.take() {
            let _ = frame.Close();
        }
    }
}

impl Drop for ClosingFrame {
    fn drop(&mut self) {
        self.close_now();
    }
}

fn drain_latest<T>(mut next: impl FnMut() -> Option<T>) -> Option<T> {
    let mut latest = next()?;
    while let Some(newer) = next() {
        latest = newer;
    }
    Some(latest)
}

struct BufferPool(Mutex<Vec<Vec<u8>>>);

impl BufferPool {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(Vec::with_capacity(FRAME_POOL_LIMIT))))
    }

    fn take(&self, needed: usize) -> Vec<u8> {
        let mut pool = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut buf = if let Some(index) = pool.iter().position(|b| b.capacity() >= needed) {
            pool.swap_remove(index)
        } else {
            pool.pop().unwrap_or_default()
        };
        buf.clear();
        if buf.capacity() < needed {
            buf.reserve(needed);
        }
        buf
    }

    fn put(&self, mut buf: Vec<u8>) {
        if buf.capacity() == 0 {
            return;
        }
        buf.clear();
        let mut pool = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if pool.len() < FRAME_POOL_LIMIT {
            pool.push(buf);
        }
    }
}

/// A captured picture: tightly packed BGRA, alpha ignored.
pub(crate) struct Frame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    /// When the system handed the picture over, before the read-back: its RTP timestamp, so the encode time does not
    /// jitter the viewers' playout.
    pub at: Instant,
    pool: Option<Arc<BufferPool>>,
}

impl Drop for Frame {
    fn drop(&mut self) {
        let data = std::mem::take(&mut self.data);
        if let Some(pool) = self.pool.take() {
            pool.put(data);
        }
    }
}

/// `E_ACCESSDENIED`; Windows 11's "Screenshots and screen recording" privacy setting returns it.
const ACCESS_DENIED: u32 = 0x8007_0005;
/// Windows 11 blocks programmatic capture for applications the owner has not allowed.
const DENIED: &str = "Windows blocked screen capture for this app; allow it in Settings, Privacy & security, Screenshots and screen recording";
/// How often a pending consent wait checks whether the share was cancelled.
const CONSENT_POLL: Duration = Duration::from_millis(50);
/// A consent prompt nobody answers must not hold a share forever.
const CONSENT_TIMEOUT: Duration = Duration::from_secs(20);

/// Moves a value to another thread when the caller guarantees it is used under a lock, one thread at a time.
struct Locked<T>(T);
// SAFETY: every use goes through the Mutex that owns it, so the value is touched by one thread at a time, and D3D11
// devices created without the single-threaded flag are free-threaded.
unsafe impl<T> Send for Locked<T> {}

/// The part of a captured texture a share sends, in texture pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A window's client area inside its capture, from screen rectangles: the capture covers the window's extended frame
/// bounds (`frame`, left/top/right/bottom), the client area starts at `client_origin` on screen and is `client_size`
/// big. The part of the client area inside the `content` WGC delivered, cut to even sides, so a share at the source's
/// size encodes the client area pixel for pixel (a borderless window's client area can reach a pixel past its frame).
/// `None` when there is no usable client area (a minimized window): the whole frame.
pub(crate) fn client_crop(
    frame: (i32, i32, i32, i32),
    client_origin: (i32, i32),
    client_size: (u32, u32),
    content: (u32, u32),
) -> Option<Crop> {
    let left = i64::from(client_origin.0) - i64::from(frame.0);
    let top = i64::from(client_origin.1) - i64::from(frame.1);
    let right = (left + i64::from(client_size.0)).min(i64::from(content.0));
    let bottom = (top + i64::from(client_size.1)).min(i64::from(content.1));
    let (x, y) = (left.max(0), top.max(0));
    let width = u32::try_from(right - x).ok()? & !1;
    let height = u32::try_from(bottom - y).ok()? & !1;
    let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
    (width >= 2 && height >= 2).then_some(Crop {
        x,
        y,
        width,
        height,
    })
}

struct Reader {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    d3d: IDirect3DDevice,
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    /// A shared window, cropped to its client area; `None` for a display.
    window: Option<HWND>,
    last_size: SizeInt32,
    buffers: Arc<BufferPool>,
}

pub(crate) struct Capture {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    item: GraphicsCaptureItem,
    arrived: i64,
    closed: i64,
    /// Keeps the capture thread out of Windows EcoQoS throttling for the lifetime of the share.
    _unthrottled: crate::throttle::Guard,
}

impl Capture {
    /// Starts capturing `source`. `want` is asked before each frame is read back, so a frame the consumer would drop
    /// costs no copy. `deliver` runs on a system thread-pool thread for each frame that was wanted. `cancel` abandons
    /// a pending consent prompt.
    pub(crate) fn start(
        source: SourceId,
        cursor: bool,
        fps: u32,
        cancel: &AtomicBool,
        want: impl Fn() -> bool + Send + Sync + 'static,
        deliver: impl FnMut(Frame) + Send + 'static,
        on_closed: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        // SAFETY: WinRT activation needs an initialized apartment on this thread; an already-initialized mode is
        // harmless because only the calls below decide the outcome. The process-wide MTA usage keeps the
        // multithreaded apartment (and the activation factories cached in it) alive when the last thread that
        // initialized it exits, which otherwise leaves later captures calling a freed factory.
        unsafe {
            static MTA: std::sync::OnceLock<()> = std::sync::OnceLock::new();
            MTA.get_or_init(|| {
                let _ = CoIncrementMTAUsage();
            });
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            return Err(Error::unavailable(
                "Windows Graphics Capture is unavailable on this Windows build",
            ));
        }
        // Windows 11 keeps a per-application consent; an application that never asks is denied and the Settings page
        // lists no request. Asking here shows the system prompt once.
        request_access(cancel)?;
        let item = create_item(source)?;
        let size = item.Size().map_err(|e| refusal(&e))?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(Error::unavailable("the selected source has no size"));
        }
        let (device, context) = create_device()?;
        let dxgi: IDXGIDevice = device.cast().map_err(|e| failed("dxgi device", &e))?;
        // SAFETY: `dxgi` is a live DXGI device on the D3D11 device above.
        let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
            .and_then(|inspectable| inspectable.cast())
            .map_err(|e| failed("WinRT device", &e))?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            POOL_BUFFERS,
            size,
        )
        .map_err(|e| failed("frame pool", &e))?;
        let session = pool
            .CreateCaptureSession(&item)
            .map_err(|e| failed("capture session", &e))?;
        {
            let _ = session.SetIsCursorCaptureEnabled(cursor);
        }
        // Pace WGC callbacks proportionally to the requested stream rate (with a 10% phase margin) instead of waking
        // every 1 ms regardless of stream FPS. Builds without the property keep the default.
        let _ = session.SetMinUpdateInterval(windows::Foundation::TimeSpan {
            Duration: min_update_interval_100ns(fps),
        });
        // Hide the yellow border where the system allows it; older builds keep the default.
        let _ = session.SetIsBorderRequired(false);
        let reader = Arc::new(Mutex::new(Locked((
            Reader {
                device,
                context,
                d3d,
                staging: None,
                window: match source {
                    SourceId::Window(handle) => Some(HWND(handle as usize as _)),
                    SourceId::Display(_) => None,
                },
                last_size: size,
                buffers: BufferPool::new(),
            },
            Box::new(deliver) as Box<dyn FnMut(Frame) + Send>,
        ))));
        let handler = {
            let reader = reader.clone();
            TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |pool, _| {
                // Keep WinRT's free-threaded frame-arrival thread-pool thread out of Windows EcoQoS throttling
                // while the app is unfocused (e.g. Alt-Tabbed to a game/fullscreen window during screen share).
                thread_local! {
                    static CALLBACK_UNTHROTTLED: crate::throttle::Guard = crate::throttle::Guard::acquire();
                }
                CALLBACK_UNTHROTTLED.with(|_| {});
                let Some(pool) = pool.as_ref() else {
                    return Ok(());
                };
                let Some(frame) =
                    drain_latest(|| pool.TryGetNextFrame().ok().map(|f| ClosingFrame(Some(f))))
                else {
                    return Ok(());
                };
                if !want() {
                    return Ok(());
                }
                let at = Instant::now();
                let Ok(mut guard) = reader.try_lock() else {
                    return Ok(());
                };
                let Locked((reader, deliver)) = &mut *guard;
                if let Ok(Some(captured)) = read_frame(reader, pool, frame, at) {
                    deliver(captured);
                }
                Ok(())
            })
        };
        let arrived = pool
            .FrameArrived(&handler)
            .map_err(|e| failed("frame events", &e))?;
        let closed_handler =
            TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
                on_closed();
                Ok(())
            });
        let closed = item
            .Closed(&closed_handler)
            .map_err(|e| failed("close events", &e))?;
        session.StartCapture().map_err(|e| refusal(&e))?;
        Ok(Self {
            pool,
            session,
            item,
            arrived,
            closed,
            _unthrottled: crate::throttle::Guard::acquire(),
        })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.pool.RemoveFrameArrived(self.arrived);
        let _ = self.item.RemoveClosed(self.closed);
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

fn create_item(source: SourceId) -> Result<GraphicsCaptureItem> {
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|e| failed("capture interop", &e))?;
    // SAFETY: the handle is only used for this call; a stale one makes it fail.
    unsafe {
        match source {
            SourceId::Display(handle) => interop
                .CreateForMonitor::<GraphicsCaptureItem>(HMONITOR(handle as usize as _))
                .map_err(|e| {
                    if e.code().0 as u32 == ACCESS_DENIED {
                        Error::unavailable(DENIED)
                    } else {
                        Error::unavailable("Selected display is no longer available")
                    }
                }),
            SourceId::Window(handle) => interop
                .CreateForWindow::<GraphicsCaptureItem>(HWND(handle as usize as _))
                .map_err(|e| {
                    if e.code().0 as u32 == ACCESS_DENIED {
                        Error::unavailable(DENIED)
                    } else {
                        Error::unavailable(
                            "Selected window is no longer available or cannot be captured",
                        )
                    }
                }),
        }
    }
}

fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    // SAFETY: the out-pointers refer to initialized locals.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .map_err(|_| Error::unavailable("The graphics driver could not start screen capture"))?;
    match (device, context) {
        (Some(device), Some(context)) => Ok((device, context)),
        _ => Err(Error::unavailable(
            "The graphics driver could not start screen capture",
        )),
    }
}

/// Maps the staging texture and reads its BGRA rows into a pooled host buffer.
///
/// # Safety
/// The caller must guarantee that `CopySubresourceRegion` into `reader.staging` already ran under the reader lock.
unsafe fn map_staging(
    reader: &Reader,
    width: u32,
    height: u32,
    at: Instant,
) -> windows::core::Result<Frame> {
    let Some((staging, _, _)) = reader.staging.as_ref() else {
        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
            0x8000_4005_u32 as i32,
        )));
    };
    unsafe {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        reader
            .context
            .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        let row_bytes = width as usize * 4;
        let needed = row_bytes * height as usize;
        let mut data = reader.buffers.take(needed);
        let pitch = mapped.RowPitch as usize;
        let source = mapped.pData.cast::<u8>();
        if !source.is_null() && pitch >= row_bytes {
            if pitch == row_bytes {
                data.extend_from_slice(std::slice::from_raw_parts(source, needed));
            } else {
                for row in 0..height as usize {
                    data.extend_from_slice(std::slice::from_raw_parts(
                        source.add(row * pitch),
                        row_bytes,
                    ));
                }
            }
        }
        reader.context.Unmap(staging, 0);
        if data.len() != needed {
            reader.buffers.put(data);
            return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                0x8000_4005_u32 as i32,
            )));
        }
        Ok(Frame {
            width,
            height,
            data,
            at,
            pool: Some(Arc::clone(&reader.buffers)),
        })
    }
}

/// Where a shared window's client area sits in its capture right now.
fn window_crop(window: HWND, content: (u32, u32)) -> Option<Crop> {
    let (frame, origin, size) = super::enumerate::client_geometry(window)?;
    client_crop(frame, origin, size, content)
}

/// Copies the frame's texture (a window's client area only) to the staging texture, releases the WGC frame, and
/// reads that same picture back. Each delivered frame is the one that just arrived, stamped with its own arrival: an
/// earlier ping-pong ring handed over the previous picture, so a still source never delivered its last one and the
/// first frame after a pause went out with the stale timestamp of the frame before it.
fn read_frame(
    reader: &mut Reader,
    pool: &Direct3D11CaptureFramePool,
    mut frame: ClosingFrame,
    at: Instant,
) -> windows::core::Result<Option<Frame>> {
    let raw_frame = frame.as_ref().ok_or_else(|| {
        windows::core::Error::from_hresult(windows::core::HRESULT(0x8000_4005_u32 as i32))
    })?;
    let content = raw_frame.ContentSize()?;
    if content.Width <= 0 || content.Height <= 0 {
        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
            0x8000_4005_u32 as i32,
        )));
    }
    let resized =
        content.Width != reader.last_size.Width || content.Height != reader.last_size.Height;
    let surface = raw_frame.Surface()?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    // SAFETY: the texture is the frame's own; CopySubresourceRegion, Map and Unmap execute sequentially under the
    // reader lock, and the WGC frame closes before any pool.Recreate or Map.
    unsafe {
        let texture: ID3D11Texture2D = access.GetInterface()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        let visible = (
            (content.Width as u32).min(desc.Width),
            (content.Height as u32).min(desc.Height),
        );
        let crop = reader
            .window
            .and_then(|window| window_crop(window, visible))
            .unwrap_or(Crop {
                x: 0,
                y: 0,
                width: visible.0,
                height: visible.1,
            });
        if reader
            .staging
            .as_ref()
            .is_none_or(|(_, w, h)| *w != crop.width || *h != crop.height)
        {
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Width: crop.width,
                Height: crop.height,
                MipLevels: 1,
                ArraySize: 1,
                Format: desc.Format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut staging = None;
            reader
                .device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
            reader.staging = staging.map(|tex| (tex, crop.width, crop.height));
        }
        let Some((staging, _, _)) = reader.staging.as_ref() else {
            return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                0x8000_4005_u32 as i32,
            )));
        };
        let region = D3D11_BOX {
            left: crop.x,
            top: crop.y,
            front: 0,
            right: crop.x + crop.width,
            bottom: crop.y + crop.height,
            back: 1,
        };
        reader
            .context
            .CopySubresourceRegion(staging, 0, 0, 0, 0, &texture, 0, Some(&region));
        // Release the WGC frame pool slot before pool.Recreate and the CPU Map so swapchains are never held.
        drop(texture);
        drop(access);
        drop(surface);
        frame.close_now();
        if resized {
            // The source was resized: later frames come at the new size.
            reader.last_size = content;
            pool.Recreate(
                &reader.d3d,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                POOL_BUFFERS,
                content,
            )?;
        }
        map_staging(reader, crop.width, crop.height, at).map(Some)
    }
}

/// Asks Windows for programmatic and best-effort borderless capture consent, giving up at once when `cancel` is set.
fn request_access(cancel: &AtomicBool) -> Result<()> {
    let Ok(operation) =
        GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Programmatic)
    else {
        // Builds without the consent API report their decision through the capture item instead.
        return Ok(());
    };
    let deadline = Instant::now() + CONSENT_TIMEOUT;
    while operation.Status().is_ok_and(|status| status.0 == 0) {
        if cancel.load(Ordering::Acquire) {
            return Err(Error::unavailable("Screen sharing was cancelled"));
        }
        if Instant::now() >= deadline {
            // Nobody answered; let the capture item give its own verdict.
            return Ok(());
        }
        std::thread::sleep(CONSENT_POLL);
    }
    match operation.GetResults() {
        // Only a decision the owner made is trusted: an unpackaged executable reports NotDeclaredByApp even where
        // capture is already allowed, and that decision still belongs to the capture item.
        Ok(status)
            if status == AppCapabilityAccessStatus::DeniedByUser
                || status == AppCapabilityAccessStatus::DeniedBySystem =>
        {
            return Err(Error::unavailable(DENIED));
        }
        _ => {}
    }
    if let Ok(borderless) =
        GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)
    {
        let border_deadline = Instant::now() + CONSENT_TIMEOUT.min(Duration::from_millis(250));
        while borderless.Status().is_ok_and(|status| status.0 == 0) {
            if cancel.load(Ordering::Acquire) {
                return Err(Error::unavailable("Screen sharing was cancelled"));
            }
            if Instant::now() >= border_deadline {
                break;
            }
            std::thread::sleep(CONSENT_POLL);
        }
    }
    Ok(())
}

fn refusal(error: &windows::core::Error) -> Error {
    if error.code().0 as u32 == ACCESS_DENIED {
        Error::unavailable(DENIED)
    } else {
        Error::unavailable(format!("Screen capture could not be started: {error}"))
    }
}

fn failed(what: &str, error: &windows::core::Error) -> Error {
    Error::unavailable(format!(
        "Screen capture could not be started ({what}): {error}"
    ))
}

fn min_update_interval_100ns(fps: u32) -> i64 {
    (10_000_000_i64 / i64::from(fps.clamp(1, 240))) * 9 / 10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_update_interval_scales_with_target_fps() {
        assert_eq!(min_update_interval_100ns(30), 299_999);
        assert_eq!(min_update_interval_100ns(60), 149_999);
        assert!(min_update_interval_100ns(0) > min_update_interval_100ns(30));
    }

    #[test]
    fn frame_drop_recycles_buffer_up_to_pool_limit() {
        let pool = BufferPool::new();
        let data = pool.take(4096);
        let ptr = data.as_ptr();
        let cap = data.capacity();
        assert!(cap >= 4096);

        let frame = Frame {
            width: 32,
            height: 32,
            data,
            at: Instant::now(),
            pool: Some(Arc::clone(&pool)),
        };
        drop(frame);

        let reused = pool.take(4096);
        assert_eq!(reused.as_ptr(), ptr);
        assert_eq!(reused.capacity(), cap);
        assert!(reused.is_empty());

        for _ in 0..(FRAME_POOL_LIMIT + 3) {
            pool.put(vec![0u8; 4096]);
        }
        assert_eq!(pool.0.lock().unwrap().len(), FRAME_POOL_LIMIT);
    }

    #[test]
    fn drain_latest_drops_superseded_frames_immediately_and_handles_empty() {
        use std::cell::Cell;
        use std::collections::VecDeque;
        use std::rc::Rc;

        struct Counted(usize, Rc<Cell<usize>>);
        impl Drop for Counted {
            fn drop(&mut self) {
                self.1.set(self.1.get() + 1);
            }
        }

        let empty: Option<Counted> = drain_latest(|| None);
        assert!(empty.is_none());

        let drops = Rc::new(Cell::new(0));
        let mut queue: VecDeque<Counted> =
            (1..=4).map(|id| Counted(id, Rc::clone(&drops))).collect();
        let kept = drain_latest(|| queue.pop_front()).unwrap();
        assert_eq!(kept.0, 4);
        assert_eq!(drops.get(), 3);
        drop(kept);
        assert_eq!(drops.get(), 4);
    }

    #[test]
    /// The crop assumes a window's capture covers its extended frame bounds: the capture item's size must equal them.
    #[test]
    #[ignore = "reads the geometry of the windows on this desktop"]
    fn window_captures_cover_the_extended_frame_bounds() {
        let mut checked = 0;
        for source in super::super::sources().unwrap() {
            let SourceId::Window(handle) = source.id else {
                continue;
            };
            let window = HWND(handle as usize as _);
            let Some((frame, origin, client)) = super::super::enumerate::client_geometry(window)
            else {
                continue;
            };
            let Ok(item) = create_item(source.id) else {
                continue;
            };
            let size = item.Size().unwrap();
            println!(
                "{:?}: item {}x{}, frame {:?} = {}x{}, client at {:?} {:?}, shared {}x{}",
                source.name,
                size.Width,
                size.Height,
                frame,
                frame.2 - frame.0,
                frame.3 - frame.1,
                origin,
                client,
                source.width,
                source.height
            );
            assert_eq!(
                (size.Width, size.Height),
                (frame.2 - frame.0, frame.3 - frame.1)
            );
            checked += 1;
        }
        println!("{checked} windows checked");
    }

    #[test]
    fn window_shares_crop_to_the_client_area_on_even_sides() {
        // A 1000x700 frame at (100, 50); the client area starts 8 px in and 31 px down and is 984x661.
        let crop = client_crop((100, 50, 1100, 750), (108, 81), (984, 661), (1000, 700)).unwrap();
        assert_eq!(
            crop,
            Crop {
                x: 8,
                y: 31,
                width: 984,
                height: 660
            }
        );
        // A client area larger than the delivered content is clamped to it.
        let crop = client_crop((0, 0, 800, 600), (0, 30), (800, 600), (800, 600)).unwrap();
        assert_eq!((crop.width, crop.height), (800, 570));
        // A client area with no size (minimized) or outside the frame keeps the whole frame.
        // A borderless client area reaching past the frame keeps the part inside it.
        let crop = client_crop((0, 0, 800, 600), (-1, -1), (802, 602), (800, 600)).unwrap();
        assert_eq!((crop.x, crop.y, crop.width, crop.height), (0, 0, 800, 600));
        assert_eq!(
            client_crop((0, 0, 800, 600), (0, 0), (0, 0), (800, 600)),
            None
        );
        assert_eq!(
            client_crop((0, 0, 800, 600), (900, 0), (10, 10), (800, 600)),
            None
        );
    }

    /// The first frames of the primary display: each one handed over is the picture that just arrived, so its
    /// arrival time is fresh when it is delivered.
    #[test]
    #[ignore = "captures the primary display"]
    fn delivered_frames_are_the_ones_that_just_arrived() {
        let display = super::super::sources()
            .unwrap()
            .into_iter()
            .find(|source| matches!(source.id, SourceId::Display(_)))
            .expect("a display");
        let (send, receive) = std::sync::mpsc::sync_channel::<(Instant, Instant)>(8);
        let cancel = AtomicBool::new(false);
        let _capture = Capture::start(
            display.id,
            false,
            60,
            &cancel,
            || true,
            move |frame| {
                let _ = send.try_send((frame.at, Instant::now()));
            },
            || {},
        )
        .unwrap();
        let (at, delivered) = receive
            .recv_timeout(Duration::from_secs(3))
            .expect("a first frame");
        // The first frame comes at once, not when a second one arrives.
        assert!(delivered.duration_since(at) < Duration::from_millis(100));
    }
}
