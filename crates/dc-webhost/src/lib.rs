//! The web content host. GPUI cannot run web content, and Embedded Activities are third-party pages that speak to the
//! client through the Embedded App SDK. This crate holds the two halves of that:
//!
//! * [`WebHost`]: a system webview (WebView2 through wry, pinned exactly) on a popup window owned by the app window,
//!   showing a relay page that frames the activity and forwards `postMessage` traffic to Rust over wry's IPC. The
//!   relay page is the only JS of the product ([`RELAY_PAGE`]).
//! * [`sdk::Bridge`]: the host side of the SDK's wire protocol (handshake, READY, commands, errors), in plain Rust with
//!   no webview, so it is tested without one.
//!
//! The compositing rule (Appendix C spike 5): the child surface paints above GPUI, so the app hides it while one of
//! its own overlays (a menu, a modal, a tooltip) is open and shows it again afterwards. The surface is cut to the
//! visible part of its box and follows the app window by itself ([`surface`]).

pub mod sdk;
pub mod surface;

pub use surface::{Area, Layout, Placement, Px, layout};

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use serde_json::{Value, json};
use std::num::NonZeroIsize;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::{Rect, WebContext, WebView, WebViewBuilder};

/// The relay page: frames the activity with the attributes the official client gives its iframe (358415 `em`) and
/// passes messages both ways. Nothing else runs here; every decision is made in Rust.
pub const RELAY_PAGE: &str = r#"<!doctype html>
<meta charset="utf-8">
<style>html,body,iframe{margin:0;border:0;width:100%;height:100%;background:transparent;overflow:hidden}iframe{color-scheme:dark}</style>
<iframe id="frame" sandbox="allow-pointer-lock allow-scripts allow-same-origin allow-forms" allow="autoplay; encrypted-media" referrerpolicy="origin"></iframe>"#;

/// The relay's script, installed as the page's initialization script (it runs in the relay page only, before any
/// page script, and again after each navigation of the top frame).
pub const RELAY_SCRIPT: &str = r#"(function(){
if(window.relay)return;
const ipc=m=>window.chrome.webview.postMessage(JSON.stringify(m));
let fr=null;
const start=()=>{
fr=document.getElementById('frame');
if(!fr)return;
fr.addEventListener('load',()=>ipc({t:'load'}));
ipc({t:'ready'});
};
window.addEventListener('message',e=>{if(fr&&e.source===fr.contentWindow)ipc({t:'msg',origin:e.origin,data:e.data})});
window.relay={open:s=>{fr.src=s},post:(d,o)=>{if(fr&&fr.contentWindow)fr.contentWindow.postMessage(d,o)},close:()=>{if(fr)fr.src='about:blank'}};
if(document.getElementById('frame'))start();else document.addEventListener('DOMContentLoaded',start);
})();"#;

/// The origin the captcha page reports its messages under, in place of a framed page's origin.
pub const CAPTCHA_ORIGIN: &str = "gravecord-captcha";

/// The captcha page: renders hCaptcha's own widget (its script, loaded from hCaptcha) for the sitekey the server sent,
/// and reports the result to Rust over the same IPC the relay uses. The page decides nothing and solves nothing: the
/// token comes from a person completing the widget. `host=discord.com` is the hostname hCaptcha checks the sitekey
/// against, as the official page's own origin does. `__SITEKEY__` and `__RQDATA__` are JSON strings ([`captcha_page`]).
const CAPTCHA_TEMPLATE: &str = r#"<!doctype html>
<meta charset="utf-8">
<style>html,body{margin:0;background:transparent;overflow:hidden}#c{display:flex;justify-content:center}</style>
<div id="c"></div>
<script>
const ipc=m=>window.chrome.webview.postMessage(JSON.stringify({t:'msg',origin:'gravecord-captcha',data:m}));
window.captchaReady=function(){
const id=hcaptcha.render('c',{sitekey:__SITEKEY__,theme:'dark',
callback:t=>ipc({token:t}),
'expired-callback':()=>ipc({expired:true}),
'error-callback':e=>ipc({error:String(e)})});
const rq=__RQDATA__;
if(rq)hcaptcha.setData(id,{rqdata:rq});
ipc({rendered:true});
};
</script>
<script src="https://js.hcaptcha.com/1/api.js?render=explicit&onload=captchaReady&host=discord.com" async defer onerror="ipc({error:'script'})"></script>"#;

/// The captcha page for one challenge. The values are put in as JSON strings, so nothing the server sends can end the
/// string or the script.
pub fn captcha_page(sitekey: &str, rqdata: Option<&str>) -> String {
    let quote = |text: &str| json!(text).to_string().replace("</", "<\\/");
    CAPTCHA_TEMPLATE
        .replace("__SITEKEY__", &quote(sitekey))
        .replace("__RQDATA__", &rqdata.map_or("null".to_owned(), quote))
}

/// What the captcha page said, from a [`HostEvent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptchaEvent {
    /// The widget is on screen.
    Rendered,
    /// A person solved it: the response token (`captcha_key`).
    Solved(String),
    Expired,
    /// The widget or its script failed.
    Failed,
}

/// Reads a captcha page's message. Only messages under [`CAPTCHA_ORIGIN`] count: nothing a framed page posts can pass
/// for a solution.
pub fn captcha_event(event: &HostEvent) -> Option<CaptchaEvent> {
    let HostEvent::Message { origin, data } = event else {
        return None;
    };
    if origin != CAPTCHA_ORIGIN {
        return None;
    }
    if let Some(token) = data["token"].as_str().filter(|token| !token.is_empty()) {
        return Some(CaptchaEvent::Solved(token.to_owned()));
    }
    if data["expired"].as_bool() == Some(true) {
        return Some(CaptchaEvent::Expired);
    }
    if data["rendered"].as_bool() == Some(true) {
        return Some(CaptchaEvent::Rendered);
    }
    data.get("error").map(|_| CaptchaEvent::Failed)
}

/// What the surface shows first: a page of its own, or a web address loaded as the top page (a provider's player),
/// with the `Referer` the official client's frame would send.
enum Page {
    Html(String),
    Url {
        url: String,
        referer: Option<String>,
    },
}

/// What the surface reports.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// The relay page is up and can open an activity.
    Ready,
    /// The framed page finished loading (the host answers with HELLO).
    Loaded,
    /// A message the framed page posted, with the origin the browser vouched for.
    Message { origin: String, data: Value },
    /// The surface could not be made.
    Failed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the window has no Win32 handle")]
    NoHandle,
}

/// What the owner can ask of the surface; run on the surface's own thread.
enum Command {
    Open(String),
    Post(String, String),
    Place(Placement),
    Visible(bool),
    Close,
    Shutdown,
}

/// The parent's `HWND`, passed to the surface's thread.
struct Parent(NonZeroIsize);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = raw_window_handle::Win32WindowHandle::new(self.0);
        // SAFETY: the handle names the app's main window, which outlives the surface (the owner drops it first).
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

/// Wakes the surface's thread when a command is sent, so a scroll moves the surface at once and the thread sleeps
/// otherwise.
struct Waker {
    #[cfg(windows)]
    event: windows::Win32::Foundation::HANDLE,
}

// SAFETY: an event handle may be signalled from any thread.
unsafe impl Send for Waker {}
// SAFETY: as above; `SetEvent` is thread safe.
unsafe impl Sync for Waker {}

impl Waker {
    fn new() -> Self {
        Self {
            // SAFETY: an unnamed auto-reset event, closed in `drop`.
            #[cfg(windows)]
            event: unsafe {
                windows::Win32::System::Threading::CreateEventW(None, false, false, None)
            }
            .unwrap_or_default(),
        }
    }

    fn wake(&self) {
        #[cfg(windows)]
        if !self.event.is_invalid() {
            // SAFETY: the handle is this waker's own event, open until it drops.
            let _ = unsafe { windows::Win32::System::Threading::SetEvent(self.event) };
        }
    }
}

impl Drop for Waker {
    fn drop(&mut self) {
        #[cfg(windows)]
        if !self.event.is_invalid() {
            // SAFETY: the event was made by `new` and nothing uses it once the last owner drops.
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.event) };
        }
    }
}

/// Sends placements to one surface from wherever its box is painted (a clone of the surface's command line).
#[derive(Clone)]
pub struct Placer {
    commands: Sender<Command>,
    waker: Arc<Waker>,
}

impl Placer {
    pub fn place(&self, placement: Placement) {
        let _ = self.commands.send(Command::Place(placement));
        self.waker.wake();
    }

    /// A placer with no surface behind it, and what it was sent: for tests of the code that places surfaces.
    pub fn recording() -> (Self, Recording) {
        let (commands, received) = channel();
        (
            Self {
                commands,
                waker: Arc::new(Waker::new()),
            },
            Recording(received),
        )
    }
}

/// What a [`Placer::recording`] was sent.
pub struct Recording(Receiver<Command>);

impl Recording {
    /// The placements sent since the last call.
    pub fn take(&self) -> Vec<Placement> {
        self.0
            .try_iter()
            .filter_map(|command| match command {
                Command::Place(placement) => Some(placement),
                _ => None,
            })
            .collect()
    }
}

/// What the surface's window last did, read back for the capture harness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    /// The surface's window, as a handle number (0 before it exists).
    pub window: isize,
    /// Whether it is on screen.
    pub visible: bool,
    /// Its rectangle on the screen, device pixels.
    pub screen: Px,
    /// The web view's rectangle in it.
    pub view: Px,
}

/// A webview child surface. It lives on a thread of its own: WebView2 waits for its environment and controller with a
/// message pump, and a pump on the UI thread would run the UI framework's queued work while it is mid-update.
pub struct WebHost {
    commands: Sender<Command>,
    events: Receiver<HostEvent>,
    waker: Arc<Waker>,
    shown: Arc<Mutex<Shown>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn physical(rect: Px) -> Rect {
    Rect {
        position: PhysicalPosition::new(rect.left, rect.top).into(),
        size: PhysicalSize::new(rect.width().max(1) as u32, rect.height().max(1) as u32).into(),
    }
}

#[cfg(windows)]
fn pump() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
    };
    let mut message = MSG::default();
    // SAFETY: plain message loop calls on the thread that owns the window.
    unsafe {
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            if message.message == WM_QUIT {
                return false;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    true
}

#[cfg(not(windows))]
fn pump() -> bool {
    true
}

/// Sleeps until a message arrives for the thread, a command is sent, or `timeout` passes.
#[cfg(windows)]
fn wait(waker: &Waker, timeout: Duration) {
    use windows::Win32::UI::WindowsAndMessaging::{
        MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, QS_ALLINPUT,
    };
    let handles = [waker.event];
    let handles = (!waker.event.is_invalid()).then_some(&handles[..]);
    // SAFETY: waits on this waker's own event and the thread's queue.
    unsafe {
        MsgWaitForMultipleObjectsEx(
            handles,
            timeout.as_millis() as u32,
            QS_ALLINPUT,
            MWMO_INPUTAVAILABLE,
        );
    }
}

#[cfg(not(windows))]
fn wait(_: &Waker, timeout: Duration) {
    std::thread::sleep(timeout.min(Duration::from_millis(8)));
}

/// How often the surface checks its owner window without being told to: a net under the owner's own events.
const RECHECK: Duration = Duration::from_millis(250);

#[cfg(windows)]
mod owner_events {
    //! The owner window's moves, resizes, minimizing and hiding, as WinEvents delivered to the surface's thread (out
    //! of context, through its message queue). The surface follows the window from its own thread, so it follows even
    //! while the app's thread is in the window's move loop and draws nothing.
    use std::cell::Cell;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_SHOW,
        EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GetWindowThreadProcessId,
        OBJID_WINDOW, WINEVENT_OUTOFCONTEXT,
    };

    thread_local! {
        static OWNER: Cell<isize> = const { Cell::new(0) };
        static CHANGED: Cell<bool> = const { Cell::new(false) };
    }

    unsafe extern "system" fn changed(
        _: HWINEVENTHOOK,
        _: u32,
        hwnd: HWND,
        object: i32,
        child: i32,
        _: u32,
        _: u32,
    ) {
        if object == OBJID_WINDOW.0 && child == 0 && hwnd.0 as isize == OWNER.with(Cell::get) {
            CHANGED.with(|changed| changed.set(true));
        }
    }

    /// The hooks of one surface thread, removed when it ends.
    pub(super) struct Hooks(Vec<HWINEVENTHOOK>);

    impl Hooks {
        pub(super) fn install(owner: HWND) -> Self {
            OWNER.with(|cell| cell.set(owner.0 as isize));
            let mut process = 0;
            // SAFETY: reads the owner window's thread and process; the window is alive.
            let thread = unsafe { GetWindowThreadProcessId(owner, Some(&mut process)) };
            let ranges = [
                (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
                (EVENT_OBJECT_SHOW, EVENT_OBJECT_HIDE),
                (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
            ];
            let hooks = ranges
                .into_iter()
                .filter_map(|(first, last)| {
                    // SAFETY: an out-of-context hook on this thread for the owner's thread only; removed in `drop`.
                    let hook = unsafe {
                        SetWinEventHook(
                            first,
                            last,
                            None,
                            Some(changed),
                            process,
                            thread,
                            WINEVENT_OUTOFCONTEXT,
                        )
                    };
                    (!hook.is_invalid()).then_some(hook)
                })
                .collect();
            Self(hooks)
        }

        /// Whether the owner moved, resized, hid or showed since the last call.
        pub(super) fn take_changed(&self) -> bool {
            CHANGED.with(|changed| changed.replace(false))
        }
    }

    impl Drop for Hooks {
        fn drop(&mut self) {
            for hook in self.0.drain(..) {
                // SAFETY: hooks this thread installed.
                unsafe {
                    let _ = UnhookWinEvent(hook);
                }
            }
        }
    }
}

/// The surface's own top-level window, owned by the app window. A child window of another thread's window would tie the
/// two threads' input queues together (and stall the UI thread); an owned popup is separate, stays above its owner
/// (never above the windows over it), hides with the owner's minimizing, and is positioned by screen coordinates. It
/// covers only the visible part of its box ([`layout`]), so it never draws outside the view that holds the box nor
/// outside the app window.
#[cfg(windows)]
struct Popup {
    hwnd: windows::Win32::Foundation::HWND,
    owner: windows::Win32::Foundation::HWND,
    /// Whether the popup is on screen, and where (screen pixels).
    on_screen: Option<Px>,
}

#[cfg(windows)]
impl Popup {
    fn new(owner: NonZeroIsize) -> Option<Self> {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
        };
        use windows::core::w;
        let owner = HWND(owner.get() as *mut std::ffi::c_void);
        // SAFETY: a plain STATIC window on this thread; the owner handle is the app's live main window. It is made
        // hidden: it shows once it has a place.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                Some(owner),
                None,
                None,
                None,
            )
        }
        .ok()?;
        Some(Self {
            hwnd,
            owner,
            on_screen: None,
        })
    }

    fn handle(&self) -> NonZeroIsize {
        NonZeroIsize::new(self.hwnd.0 as isize).expect("window handle")
    }

    /// Where the popup goes now: its layout in the owner's client area, or `None` while nothing of it may show (the
    /// app hid it, its box is out of view, or the owner is minimized or hidden).
    fn target(&self, placement: Placement, wanted: bool) -> Option<Layout> {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, IsIconic, IsWindowVisible};
        if !wanted {
            return None;
        }
        // SAFETY: reads of the owner window, which outlives the popup.
        unsafe {
            if !IsWindowVisible(self.owner).as_bool() || IsIconic(self.owner).as_bool() {
                return None;
            }
            let mut client = RECT::default();
            GetClientRect(self.owner, &mut client).ok()?;
            layout(placement, (client.right, client.bottom))
        }
    }

    /// The window to put the popup behind when it shows: the one right above its owner, so the popup lands just over
    /// the app and under whatever covers the app. `None`: the top of the non-topmost windows (the owner is the top one
    /// already, or the window above it is topmost).
    fn above_owner(&self) -> Option<windows::Win32::Foundation::HWND> {
        use windows::Win32::UI::WindowsAndMessaging::{
            GW_HWNDPREV, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, WS_EX_TOPMOST,
        };
        // SAFETY: reads of window handles; a stale one only makes `SetWindowPos` fail.
        unsafe {
            let above = GetWindow(self.owner, GW_HWNDPREV).ok()?;
            let topmost = GetWindowLongPtrW(above, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
            (!topmost).then_some(above)
        }
    }

    /// Puts the popup where `target` says, touching the window only where something changed. Returns whether it is
    /// on screen and where.
    fn apply(&mut self, target: Option<Layout>) -> Option<Px> {
        use windows::Win32::Foundation::{HWND, POINT};
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::{
            HWND_TOP, SW_HIDE, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos,
            ShowWindow,
        };
        let Some(layout) = target else {
            if self.on_screen.take().is_some() {
                // SAFETY: the popup is this thread's own window.
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
            }
            return None;
        };
        let mut origin = POINT {
            x: layout.window.left,
            y: layout.window.top,
        };
        // SAFETY: the owner outlives the popup.
        let _ = unsafe { ClientToScreen(self.owner, &mut origin) };
        let screen = Px {
            left: origin.x,
            top: origin.y,
            right: origin.x + layout.window.width(),
            bottom: origin.y + layout.window.height(),
        };
        if self.on_screen == Some(screen) {
            return Some(screen);
        }
        let showing = self.on_screen.is_none();
        // Shown again: placed right above the owner. Already shown: the owner carries it in the z-order.
        let (after, flags) = if showing {
            match self.above_owner() {
                Some(above) if above == self.hwnd => (None, SWP_NOZORDER | SWP_SHOWWINDOW),
                Some(above) => (Some(above), SWP_SHOWWINDOW),
                None => (Some(HWND_TOP), SWP_SHOWWINDOW),
            }
        } else {
            (None::<HWND>, SWP_NOZORDER)
        };
        // SAFETY: the popup is this thread's own window; `after` is a live window or HWND_TOP.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                after,
                screen.left,
                screen.top,
                screen.width(),
                screen.height(),
                flags | SWP_NOACTIVATE,
            );
        }
        self.on_screen = Some(screen);
        Some(screen)
    }
}

#[cfg(windows)]
impl Drop for Popup {
    fn drop(&mut self) {
        // SAFETY: the window was made on this thread, which is the one dropping it.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

/// What one surface thread keeps between turns.
struct Surface {
    placement: Placement,
    /// The app wants it shown (no overlay of its own over it).
    wanted: bool,
    /// The web view's rectangle and visibility as last set.
    view: Option<Px>,
    view_visible: bool,
    shown: Arc<Mutex<Shown>>,
}

impl Surface {
    /// Makes the window and the view match the placement, the app's wish and the owner's state.
    #[cfg(windows)]
    fn apply(&mut self, popup: &mut Popup, view: &WebView) {
        let target = popup.target(self.placement, self.wanted);
        if let Some(layout) = target
            && self.view != Some(layout.view)
        {
            let _ = view.set_bounds(physical(layout.view));
            self.view = Some(layout.view);
        }
        let on_screen = popup.apply(target);
        let visible = on_screen.is_some();
        if visible != self.view_visible {
            // A hidden view stops drawing (WebView2 suspends its rendering), so a player out of view costs nothing.
            let _ = view.set_visible(visible);
            self.view_visible = visible;
        }
        if let Ok(mut shown) = self.shown.lock() {
            *shown = Shown {
                window: popup.hwnd.0 as isize,
                visible,
                screen: on_screen.unwrap_or_default(),
                view: self.view.unwrap_or_default(),
            };
        }
    }

    #[cfg(not(windows))]
    fn apply(&mut self, view: &WebView) {
        let target = layout(self.placement, (i32::MAX, i32::MAX)).filter(|_| self.wanted);
        if let Some(layout) = target {
            let _ = view.set_bounds(physical(layout.window));
        }
        let _ = view.set_visible(target.is_some());
    }
}

fn run(
    parent: Parent,
    data_dir: PathBuf,
    placement: Placement,
    page: Page,
    commands: Receiver<Command>,
    sender: Sender<HostEvent>,
    (waker, shown): (Arc<Waker>, Arc<Mutex<Shown>>),
) {
    #[cfg(windows)]
    // SAFETY: once per thread, before any COM use.
    unsafe {
        let _ = windows::Win32::System::Ole::OleInitialize(None);
    }
    #[cfg(windows)]
    let popup = Popup::new(parent.0);
    #[cfg(windows)]
    let Some(mut popup) = popup else {
        let _ = sender.send(HostEvent::Failed("no surface window".to_owned()));
        return;
    };
    #[cfg(windows)]
    let hooks = owner_events::Hooks::install(popup.owner);
    #[cfg(windows)]
    let parent = Parent(popup.handle());
    let mut surface = Surface {
        placement,
        wanted: true,
        view: None,
        view_visible: true,
        shown,
    };
    let mut context = WebContext::new(Some(data_dir));
    let ipc = sender.clone();
    // Made at the box's whole size; the first `apply` moves it into place.
    let size = placement.frame.snapped();
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(physical(Px {
            left: 0,
            top: 0,
            right: size.width(),
            bottom: size.height(),
        }))
        .with_transparent(true)
        .with_autoplay(true);
    let builder = match page {
        Page::Html(html) => builder.with_html(html),
        Page::Url { url, referer } => {
            let mut headers = wry::http::HeaderMap::new();
            if let Some(value) = referer.and_then(|r| wry::http::HeaderValue::from_str(&r).ok()) {
                headers.insert(wry::http::header::REFERER, value);
            }
            builder.with_url(url).with_headers(headers)
        }
    };
    let view: WebView = match builder
        .with_initialization_script(RELAY_SCRIPT)
        .with_ipc_handler(move |request| {
            let Ok(message) = serde_json::from_str::<Value>(request.body()) else {
                return;
            };
            let event = match message["t"].as_str() {
                Some("ready") => HostEvent::Ready,
                Some("load") => HostEvent::Loaded,
                Some("msg") => HostEvent::Message {
                    origin: message["origin"].as_str().unwrap_or_default().to_owned(),
                    data: message["data"].clone(),
                },
                _ => return,
            };
            let _ = ipc.send(event);
        })
        // Links the page opens never get a window of their own.
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .build_as_child(&parent)
    {
        Ok(view) => view,
        Err(error) => {
            let _ = sender.send(HostEvent::Failed(error.to_string()));
            return;
        }
    };
    let mut dirty = true;
    let mut checked = std::time::Instant::now();
    loop {
        if !pump() {
            return;
        }
        loop {
            match commands.try_recv() {
                Ok(Command::Open(url)) => {
                    let _ = view.evaluate_script(&format!("relay.open({})", json!(url)));
                }
                Ok(Command::Post(message, origin)) => {
                    let _ =
                        view.evaluate_script(&format!("relay.post({message},{})", json!(origin)));
                }
                Ok(Command::Place(placement)) => {
                    dirty |= placement != surface.placement;
                    surface.placement = placement;
                }
                Ok(Command::Visible(wanted)) => {
                    dirty |= wanted != surface.wanted;
                    surface.wanted = wanted;
                }
                Ok(Command::Close) => {
                    let _ = view.evaluate_script("relay.close()");
                }
                Ok(Command::Shutdown) | Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
        }
        #[cfg(windows)]
        {
            dirty |= hooks.take_changed();
        }
        if !dirty && checked.elapsed() >= RECHECK {
            dirty = true;
        }
        if dirty {
            #[cfg(windows)]
            surface.apply(&mut popup, &view);
            #[cfg(not(windows))]
            surface.apply(&view);
            dirty = false;
            checked = std::time::Instant::now();
        }
        wait(&waker, RECHECK);
    }
}

impl WebHost {
    /// Makes the surface over `placement` of `window` (device pixels of its client area). `data_dir` keeps the pages'
    /// cookies and storage between launches. The surface comes up on its own thread: [`HostEvent::Ready`] says it is
    /// there, [`HostEvent::Failed`] that it will not be.
    pub fn new(
        window: &impl HasWindowHandle,
        data_dir: PathBuf,
        placement: Placement,
    ) -> Result<Self, Error> {
        Self::with_page(window, data_dir, placement, RELAY_PAGE.to_owned())
    }

    /// A surface showing the captcha page of one challenge ([`captcha_page`]); its answers are [`captcha_event`]s.
    pub fn captcha(
        window: &impl HasWindowHandle,
        data_dir: PathBuf,
        placement: Placement,
        sitekey: &str,
        rqdata: Option<&str>,
    ) -> Result<Self, Error> {
        Self::with_page(window, data_dir, placement, captcha_page(sitekey, rqdata))
    }

    /// A surface loading `url` as its top page, for a provider's embedded player (YouTube, TikTok, Twitch, Spotify).
    /// `referer` is sent with the first request, as the page that frames the player in the official client would.
    pub fn player(
        window: &impl HasWindowHandle,
        data_dir: PathBuf,
        placement: Placement,
        url: &str,
        referer: Option<&str>,
    ) -> Result<Self, Error> {
        Self::with_page_kind(
            window,
            data_dir,
            placement,
            Page::Url {
                url: url.to_owned(),
                referer: referer.map(str::to_owned),
            },
        )
    }

    fn with_page(
        window: &impl HasWindowHandle,
        data_dir: PathBuf,
        placement: Placement,
        html: String,
    ) -> Result<Self, Error> {
        Self::with_page_kind(window, data_dir, placement, Page::Html(html))
    }

    fn with_page_kind(
        window: &impl HasWindowHandle,
        data_dir: PathBuf,
        placement: Placement,
        page: Page,
    ) -> Result<Self, Error> {
        let RawWindowHandle::Win32(handle) = window
            .window_handle()
            .map_err(|_| Error::NoHandle)?
            .as_raw()
        else {
            return Err(Error::NoHandle);
        };
        let parent = Parent(handle.hwnd);
        let (command_sender, command_receiver) = channel();
        let (event_sender, events) = channel();
        let waker = Arc::new(Waker::new());
        let shown = Arc::new(Mutex::new(Shown::default()));
        let thread = std::thread::Builder::new()
            .name("dc-webhost".to_owned())
            .spawn({
                let waker = waker.clone();
                let shown = shown.clone();
                move || {
                    run(
                        parent,
                        data_dir,
                        placement,
                        page,
                        command_receiver,
                        event_sender,
                        (waker, shown),
                    )
                }
            })
            .map_err(|_| Error::NoHandle)?;
        Ok(Self {
            commands: command_sender,
            events,
            waker,
            shown,
            thread: Some(thread),
        })
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
        self.waker.wake();
    }

    /// The events since the last call.
    pub fn poll(&self) -> Vec<HostEvent> {
        self.events.try_iter().collect()
    }

    /// Loads the activity page in the frame.
    pub fn open(&self, url: &str) {
        self.send(Command::Open(url.to_owned()));
    }

    /// Posts `message` to the framed page, to `origin` only.
    pub fn post(&self, message: &Value, origin: &str) {
        self.send(Command::Post(message.to_string(), origin.to_owned()));
    }

    /// Moves the surface to `placement`.
    pub fn set_placement(&self, placement: Placement) {
        self.send(Command::Place(placement));
    }

    /// A handle that moves the surface from where its box is painted.
    pub fn placer(&self) -> Placer {
        Placer {
            commands: self.commands.clone(),
            waker: self.waker.clone(),
        }
    }

    /// Shows the surface, or hides it while one of the app's own overlays is over it.
    pub fn set_visible(&self, visible: bool) {
        self.send(Command::Visible(visible));
    }

    /// Empties the frame (the activity was left).
    pub fn close(&self) {
        self.send(Command::Close);
    }

    /// What the surface's window last did.
    pub fn shown(&self) -> Shown {
        self.shown.lock().map(|shown| *shown).unwrap_or_default()
    }
}

impl Drop for WebHost {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        // The surface's thread ends on its next turn (the waker wakes it now) and takes its window and view with it.
        // The app's thread never waits for that: tearing a web view down can take a while, and a frame must not.
        drop(self.thread.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_carries_the_sitekey_and_rqdata_as_strings() {
        let page = captcha_page("test-key", Some("rq-data"));
        assert!(page.contains("sitekey:\"test-key\""));
        assert!(page.contains("const rq=\"rq-data\";"));
        assert!(page.contains("js.hcaptcha.com/1/api.js"));
        assert!(captcha_page("k", None).contains("const rq=null;"));
    }

    #[test]
    fn nothing_the_server_sends_can_break_out_of_the_script() {
        let page = captcha_page(
            "\"};alert(1);//",
            Some("</script><script>alert(2)</script>"),
        );
        assert!(!page.contains("</script><script>alert(2)"));
        assert!(page.contains(r"<\/script>"));
        assert!(page.contains("\\\"};alert(1);//"));
    }

    #[test]
    fn only_the_captcha_origin_can_report_a_token() {
        let message = |origin: &str, data: Value| HostEvent::Message {
            origin: origin.to_owned(),
            data,
        };
        assert_eq!(
            captcha_event(&message(
                CAPTCHA_ORIGIN,
                json!({"token": "FAKE-TEST-TOKEN"})
            )),
            Some(CaptchaEvent::Solved("FAKE-TEST-TOKEN".to_owned()))
        );
        assert_eq!(
            captcha_event(&message("https://evil.example", json!({"token": "x"}))),
            None
        );
        assert_eq!(
            captcha_event(&message(CAPTCHA_ORIGIN, json!({"token": ""}))),
            None
        );
        assert_eq!(
            captcha_event(&message(CAPTCHA_ORIGIN, json!({"rendered": true}))),
            Some(CaptchaEvent::Rendered)
        );
        assert_eq!(
            captcha_event(&message(CAPTCHA_ORIGIN, json!({"expired": true}))),
            Some(CaptchaEvent::Expired)
        );
        assert_eq!(
            captcha_event(&message(CAPTCHA_ORIGIN, json!({"error": "script"}))),
            Some(CaptchaEvent::Failed)
        );
        assert_eq!(captcha_event(&HostEvent::Ready), None);
    }
}
