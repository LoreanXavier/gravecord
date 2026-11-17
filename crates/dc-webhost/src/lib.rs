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
//! its own overlays (a menu, a modal, a tooltip) is open and shows it again afterwards.

pub mod sdk;

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use serde_json::{Value, json};
use std::num::NonZeroIsize;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;
use wry::dpi::{LogicalPosition, LogicalSize};
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
    Bounds((f32, f32, f32, f32)),
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

/// A webview child surface. It lives on a thread of its own: WebView2 waits for its environment and controller with a
/// message pump, and a pump on the UI thread would run the UI framework's queued work while it is mid-update.
pub struct WebHost {
    commands: Sender<Command>,
    events: Receiver<HostEvent>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        position: LogicalPosition::new(x as f64, y as f64).into(),
        size: LogicalSize::new(width.max(1.) as f64, height.max(1.) as f64).into(),
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

/// The surface's own top-level window, owned by the app window. A child window of another thread's window would tie the
/// two threads' input queues together (and stall the UI thread); an owned popup is separate and still stays above its
/// owner, follows its minimizing, and is positioned over the stage by screen coordinates.
#[cfg(windows)]
struct Popup {
    hwnd: windows::Win32::Foundation::HWND,
    owner: windows::Win32::Foundation::HWND,
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
        // SAFETY: a plain STATIC window on this thread; the owner handle is the app's live main window.
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
        Some(Self { hwnd, owner })
    }

    fn handle(&self) -> NonZeroIsize {
        NonZeroIsize::new(self.hwnd.0 as isize).expect("window handle")
    }

    /// Moves the popup over `bounds` (logical, in the owner's client area) and shows or hides it.
    fn place(&self, bounds: (f32, f32, f32, f32), visible: bool) {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::HiDpi::GetDpiForWindow;
        use windows::Win32::UI::WindowsAndMessaging::{
            SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetWindowPos, ShowWindow,
        };
        // SAFETY: calls on handles this thread made or the owner's, which outlives the popup.
        unsafe {
            let scale = GetDpiForWindow(self.owner).max(96) as f32 / 96.;
            let mut origin = POINT {
                x: (bounds.0 * scale).round() as i32,
                y: (bounds.1 * scale).round() as i32,
            };
            let _ = ClientToScreen(self.owner, &mut origin);
            let _ = SetWindowPos(
                self.hwnd,
                None,
                origin.x,
                origin.y,
                (bounds.2 * scale).round().max(1.) as i32,
                (bounds.3 * scale).round().max(1.) as i32,
                SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
            );
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
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

fn run(
    parent: Parent,
    data_dir: PathBuf,
    bounds: (f32, f32, f32, f32),
    page: Page,
    commands: Receiver<Command>,
    sender: Sender<HostEvent>,
) {
    #[cfg(windows)]
    // SAFETY: once per thread, before any COM use.
    unsafe {
        let _ = windows::Win32::System::Ole::OleInitialize(None);
    }
    #[cfg(windows)]
    let popup = Popup::new(parent.0);
    #[cfg(windows)]
    let Some(popup) = popup else {
        let _ = sender.send(HostEvent::Failed("no surface window".to_owned()));
        return;
    };
    #[cfg(windows)]
    let parent = Parent(popup.handle());
    let mut bounds = bounds;
    let mut visible = true;
    #[cfg(windows)]
    popup.place(bounds, visible);
    let mut context = WebContext::new(Some(data_dir));
    let ipc = sender.clone();
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(0., 0., bounds.2, bounds.3))
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
    let mut ticks = 0u32;
    loop {
        ticks += 1;
        #[cfg(windows)]
        if ticks.is_multiple_of(12) {
            popup.place(bounds, visible);
        }
        if !pump() {
            return;
        }
        match commands.recv_timeout(Duration::from_millis(8)) {
            Ok(Command::Open(url)) => {
                let _ = view.evaluate_script(&format!("relay.open({})", json!(url)));
            }
            Ok(Command::Post(message, origin)) => {
                let _ = view.evaluate_script(&format!("relay.post({message},{})", json!(origin)));
            }
            Ok(Command::Bounds(b)) => {
                bounds = b;
                let _ = view.set_bounds(rect(0., 0., b.2, b.3));
                #[cfg(windows)]
                popup.place(bounds, visible);
            }
            Ok(Command::Visible(shown)) => {
                visible = shown;
                let _ = view.set_visible(shown);
                #[cfg(windows)]
                popup.place(bounds, visible);
            }
            Ok(Command::Close) => {
                let _ = view.evaluate_script("relay.close()");
            }
            Ok(Command::Shutdown) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

impl WebHost {
    /// Makes the surface a child of `window` at the given logical bounds. `data_dir` keeps the pages' cookies and
    /// storage between launches. The surface comes up on its own thread: [`HostEvent::Ready`] says it is there,
    /// [`HostEvent::Failed`] that it will not be.
    pub fn new(
        window: &impl HasWindowHandle,
        data_dir: PathBuf,
        bounds: (f32, f32, f32, f32),
    ) -> Result<Self, Error> {
        Self::with_page(window, data_dir, bounds, RELAY_PAGE.to_owned())
    }

    /// A surface showing the captcha page of one challenge ([`captcha_page`]); its answers are [`captcha_event`]s.
    pub fn captcha(
        window: &impl HasWindowHandle,
        data_dir: PathBuf,
        bounds: (f32, f32, f32, f32),
        sitekey: &str,
        rqdata: Option<&str>,
    ) -> Result<Self, Error> {
        Self::with_page(window, data_dir, bounds, captcha_page(sitekey, rqdata))
    }

    /// A surface loading `url` as its top page, for a provider's embedded player (YouTube, TikTok, Twitch, Spotify).
    /// `referer` is sent with the first request, as the page that frames the player in the official client would.
    pub fn player(
        window: &impl HasWindowHandle,
        data_dir: PathBuf,
        bounds: (f32, f32, f32, f32),
        url: &str,
        referer: Option<&str>,
    ) -> Result<Self, Error> {
        Self::with_page_kind(
            window,
            data_dir,
            bounds,
            Page::Url {
                url: url.to_owned(),
                referer: referer.map(str::to_owned),
            },
        )
    }

    fn with_page(
        window: &impl HasWindowHandle,
        data_dir: PathBuf,
        bounds: (f32, f32, f32, f32),
        html: String,
    ) -> Result<Self, Error> {
        Self::with_page_kind(window, data_dir, bounds, Page::Html(html))
    }

    fn with_page_kind(
        window: &impl HasWindowHandle,
        data_dir: PathBuf,
        bounds: (f32, f32, f32, f32),
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
        let thread = std::thread::Builder::new()
            .name("dc-webhost".to_owned())
            .spawn(move || {
                run(
                    parent,
                    data_dir,
                    bounds,
                    page,
                    command_receiver,
                    event_sender,
                )
            })
            .map_err(|_| Error::NoHandle)?;
        Ok(Self {
            commands: command_sender,
            events,
            thread: Some(thread),
        })
    }

    /// The events since the last call.
    pub fn poll(&self) -> Vec<HostEvent> {
        self.events.try_iter().collect()
    }

    /// Loads the activity page in the frame.
    pub fn open(&self, url: &str) {
        let _ = self.commands.send(Command::Open(url.to_owned()));
    }

    /// Posts `message` to the framed page, to `origin` only.
    pub fn post(&self, message: &Value, origin: &str) {
        let _ = self
            .commands
            .send(Command::Post(message.to_string(), origin.to_owned()));
    }

    pub fn set_bounds(&self, bounds: (f32, f32, f32, f32)) {
        let _ = self.commands.send(Command::Bounds(bounds));
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.commands.send(Command::Visible(visible));
    }

    /// Empties the frame (the activity was left).
    pub fn close(&self) {
        let _ = self.commands.send(Command::Close);
    }
}

impl Drop for WebHost {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        // The surface's thread ends on its next turn; one stuck in the web view's start (a window that never gets a
        // message pump) must not hold the app open, so the wait is bounded and the thread is left to the process.
        if let Some(thread) = self.thread.take() {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !thread.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
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
