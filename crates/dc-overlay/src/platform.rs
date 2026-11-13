//! Windows: the out-of-process overlay window's styles and placement, the foreground watcher, and the desktop check.
//!
//! The official native module (`discord_desktop_overlay`) watches the foreground with `SetWinEventHook`
//! (`EVENT_SYSTEM_FOREGROUND`), keeps its window over the game with `SetWindowPos`, and turns input on and off
//! (`setInteractionEnabled`). Its Electron window is frameless, transparent, always on top, out of the taskbar
//! (`skipTaskbar`, `type: "toolbar"`) and shown without activation (`showInactive`). Here the same is done with the
//! extended styles of a gpui window:
//!
//! - `WS_EX_TOOLWINDOW`: no taskbar button, not in Alt+Tab.
//! - `WS_EX_TOPMOST`, placed with `HWND_TOPMOST` and `SWP_NOACTIVATE`.
//! - `WS_EX_LAYERED | WS_EX_TRANSPARENT` while locked: clicks go through to the game. `WS_EX_NOACTIVATE` keeps it from
//!   taking the focus. Unlocked, both go and the window takes input.
//! - `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`: our own screen share and the clip recorder never show it.
//!
//! Nothing here reads input or another process's memory: only window positions, styles and the owning process id.

use crate::decision::{Foreground, FullscreenType, Rect};
use std::ptr;
use std::sync::Arc;
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    ScreenToClient,
};
use windows_sys::Win32::System::StationsAndDesktops::{
    GetThreadDesktop, GetUserObjectInformationW, UOI_NAME,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::Shell::{QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, GWL_EXSTYLE, GetClientRect, GetCursorPos, GetForegroundWindow,
    GetMessageW, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, HWND_TOPMOST,
    IsIconic, IsZoomed, LWA_ALPHA, MSG, OBJID_WINDOW, PostThreadMessageW, SW_HIDE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowDisplayAffinity,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, WDA_EXCLUDEFROMCAPTURE, WINEVENT_OUTOFCONTEXT,
    WM_QUIT, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

/// The name of the desktop this thread draws on (`Default` for the user's own).
pub fn desktop_name() -> Option<String> {
    // SAFETY: the desktop handle of this thread is not owned and not closed.
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    if desktop.is_null() {
        return None;
    }
    let mut buffer = [0u16; 256];
    let mut needed = 0u32;
    // SAFETY: the buffer is 512 bytes as stated; `needed` is a valid out pointer.
    let ok = unsafe {
        GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 2) as u32,
            &mut needed,
        )
    };
    if ok == 0 {
        return None;
    }
    let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..length]))
}

fn hwnd(value: isize) -> HWND {
    value as HWND
}

/// How a window is shown. Exclusive fullscreen is what the shell reports as a running Direct3D fullscreen
/// application; a window that covers its whole monitor is borderless fullscreen.
fn fullscreen_type(window: HWND) -> FullscreenType {
    // SAFETY: plain queries on a window handle; a stale handle only makes them fail.
    unsafe {
        if IsIconic(window) != 0 {
            return FullscreenType::Minimized;
        }
        let mut state = 0;
        if SHQueryUserNotificationState(&mut state) == 0
            && state == QUNS_RUNNING_D3D_FULL_SCREEN
            && GetForegroundWindow() == window
        {
            return FullscreenType::Fullscreen;
        }
        if IsZoomed(window) != 0 {
            return FullscreenType::Maximized;
        }
        let mut rect = RECT::default();
        if GetWindowRect(window, &mut rect) == 0 {
            return FullscreenType::Unknown;
        }
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT::default(),
            rcWork: RECT::default(),
            dwFlags: 0,
        };
        if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
            let screen = info.rcMonitor;
            if rect.left <= screen.left
                && rect.top <= screen.top
                && rect.right >= screen.right
                && rect.bottom >= screen.bottom
            {
                return FullscreenType::BorderlessFullscreen;
            }
        }
        FullscreenType::Windowed
    }
}

/// The client area of a window on screen.
fn client_rect(window: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: plain queries with valid out pointers.
    unsafe {
        if GetClientRect(window, &mut rect) == 0 || ClientToScreen(window, &mut origin) == 0 {
            return None;
        }
    }
    Some(Rect {
        x: origin.x,
        y: origin.y,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    })
}

/// The foreground window now.
pub fn foreground() -> Option<Foreground> {
    // SAFETY: plain query.
    let window = unsafe { GetForegroundWindow() };
    describe(window)
}

fn describe(window: HWND) -> Option<Foreground> {
    if window.is_null() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer.
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if pid == 0 {
        return None;
    }
    Some(Foreground {
        hwnd: window as isize,
        pid,
        client: client_rect(window).unwrap_or_default(),
        fullscreen: fullscreen_type(window),
    })
}

type Callback = dyn Fn(Option<Foreground>) + Send + Sync;

struct Shared {
    on_change: Box<Callback>,
    last: std::sync::Mutex<Option<Foreground>>,
}

impl Shared {
    fn report(&self) {
        let now = foreground();
        let changed = match self.last.lock() {
            Ok(mut last) if *last != now => {
                *last = now;
                true
            }
            _ => false,
        };
        if changed {
            (self.on_change)(now);
        }
    }
}

thread_local! {
    static ACTIVE: std::cell::RefCell<Option<Arc<Shared>>> = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    // Location changes come for every window, the cursor and carets too: only the foreground window's own move or
    // resize counts.
    if event == EVENT_OBJECT_LOCATIONCHANGE
        // SAFETY: plain query.
        && (object != OBJID_WINDOW || window != unsafe { GetForegroundWindow() })
    {
        return;
    }
    let shared = ACTIVE.with(|active| active.borrow().clone());
    if let Some(shared) = shared {
        shared.report();
    }
}

/// The running watcher. Dropping it removes the hooks.
pub struct Watcher {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

/// Starts watching the foreground window on a thread of its own: `on_change` gets the new foreground (or none) each
/// time it, its size or its state changes, and once at the start.
pub fn watch(
    on_change: impl Fn(Option<Foreground>) + Send + Sync + 'static,
) -> Result<Watcher, &'static str> {
    let shared = Arc::new(Shared {
        on_change: Box::new(on_change),
        last: std::sync::Mutex::new(None),
    });
    let (ready, installed) = std::sync::mpsc::channel::<Result<u32, &'static str>>();
    let thread = std::thread::Builder::new()
        .name("dc-overlay-focus".into())
        .spawn(move || {
            ACTIVE.with(|active| *active.borrow_mut() = Some(shared.clone()));
            run(&ready, &shared);
        })
        .map_err(|_| "Could not start the foreground watcher thread")?;
    match installed.recv() {
        Ok(Ok(thread_id)) => Ok(Watcher {
            thread_id,
            thread: Some(thread),
        }),
        other => {
            let _ = thread.join();
            Err(other
                .unwrap_or(Err("The foreground watcher thread ended"))
                .unwrap_err())
        }
    }
}

fn run(ready: &std::sync::mpsc::Sender<Result<u32, &'static str>>, shared: &Shared) {
    let hook = |from: u32, to: u32| {
        // SAFETY: an out-of-context hook with a procedure that lives as long as the process.
        unsafe {
            SetWinEventHook(
                from,
                to,
                ptr::null_mut(),
                Some(event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        }
    };
    let hooks = [
        hook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
        hook(EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
        hook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
    ];
    if hooks[0].is_null() {
        unhook(&hooks);
        let _ = ready.send(Err("Windows refused the foreground hook"));
        return;
    }
    // SAFETY: plain call.
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
    shared.report();
    let mut message: MSG = MSG {
        hwnd: ptr::null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: POINT { x: 0, y: 0 },
    };
    // SAFETY: `message` is a valid MSG; 0 is WM_QUIT, -1 an error.
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {}
    unhook(&hooks);
}

fn unhook(hooks: &[HWINEVENTHOOK]) {
    for hook in hooks.iter().filter(|hook| !hook.is_null()) {
        // SAFETY: a hook this thread installed.
        unsafe { UnhookWinEvent(*hook) };
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            // SAFETY: posts WM_QUIT to the watcher's queue.
            unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
            let _ = thread.join();
        }
    }
}

/// The extended styles of the overlay window for a lock state.
pub fn overlay_styles(current: isize, locked: bool) -> isize {
    let always = (WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED) as isize;
    let locked_only = (WS_EX_TRANSPARENT | WS_EX_NOACTIVATE) as isize;
    let base = (current | always) & !locked_only;
    if locked { base | locked_only } else { base }
}

/// Gives the window the overlay's styles for the lock state and keeps it out of screen captures.
pub fn style(window: isize, locked: bool) {
    let window = hwnd(window);
    // SAFETY: style changes on a window this process created.
    unsafe {
        let current = GetWindowLongPtrW(window, GWL_EXSTYLE);
        SetWindowLongPtrW(window, GWL_EXSTYLE, overlay_styles(current, locked));
        // A layered window needs its attributes once: fully opaque, the picture's own alpha does the rest.
        SetLayeredWindowAttributes(window, 0, 255, LWA_ALPHA);
        SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE);
    }
}

/// The window rectangle whose client area is `client`, for a window whose frame adds `frame` (left, top, right,
/// bottom) around its client area.
pub fn outer_rect(client: Rect, frame: (i32, i32, i32, i32)) -> Rect {
    Rect {
        x: client.x - frame.0,
        y: client.y - frame.1,
        width: client.width + frame.0 + frame.2,
        height: client.height + frame.1 + frame.3,
    }
}

/// What the window's frame adds around its client area now (a gpui pop-up keeps resize borders).
fn frame_of(window: HWND) -> (i32, i32, i32, i32) {
    let mut outer = RECT::default();
    let mut inner = RECT::default();
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: plain queries with valid out pointers.
    unsafe {
        if GetWindowRect(window, &mut outer) == 0
            || GetClientRect(window, &mut inner) == 0
            || ClientToScreen(window, &mut origin) == 0
        {
            return (0, 0, 0, 0);
        }
    }
    let left = origin.x - outer.left;
    let top = origin.y - outer.top;
    (
        left,
        top,
        outer.right - outer.left - left - inner.right,
        outer.bottom - outer.top - top - inner.bottom,
    )
}

/// Shows the window with its client area over `rect` (the game's client area), above every other, without
/// activating it.
pub fn place(window: isize, rect: Rect) {
    let window = hwnd(window);
    let outer = outer_rect(rect, frame_of(window));
    // SAFETY: placement of a window this process created.
    unsafe {
        SetWindowPos(
            window,
            HWND_TOPMOST,
            outer.x,
            outer.y,
            outer.width,
            outer.height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

pub fn hide(window: isize) {
    // SAFETY: as above.
    unsafe { ShowWindow(hwnd(window), SW_HIDE) };
}

/// The clickable regions of a locked overlay. `WS_EX_TRANSPARENT` is all or nothing, and `HTTRANSPARENT` from
/// `WM_NCHITTEST` only passes a click to windows of the same thread (never to the game), while a window region would
/// also clip what is drawn outside it. So the cursor is followed instead: over a zone the window takes the click
/// (still without being activated), anywhere else it is click-through. `cursor` is the cursor in the window's client
/// area with the area's size, both in device pixels.
pub fn cursor(window: isize) -> Option<((f32, f32), (f32, f32))> {
    let window = hwnd(window);
    let mut point = POINT { x: 0, y: 0 };
    let mut rect = RECT::default();
    // SAFETY: plain queries with valid out pointers; a stale handle only makes them fail.
    unsafe {
        if GetCursorPos(&mut point) == 0
            || ScreenToClient(window, &mut point) == 0
            || GetClientRect(window, &mut rect) == 0
        {
            return None;
        }
    }
    Some((
        (point.x as f32, point.y as f32),
        (
            (rect.right - rect.left) as f32,
            (rect.bottom - rect.top) as f32,
        ),
    ))
}

/// The extended styles for a locked window that takes clicks now (`through` false) or lets them through.
pub fn click_through_styles(current: isize, through: bool) -> isize {
    let transparent = WS_EX_TRANSPARENT as isize;
    if through {
        current | transparent
    } else {
        current & !transparent
    }
}

/// Turns click-through on or off for a locked window; it is never activated either way (`WS_EX_NOACTIVATE` stays).
pub fn set_click_through(window: isize, through: bool) {
    let window = hwnd(window);
    // SAFETY: a style change on a window this process created.
    unsafe {
        let current = GetWindowLongPtrW(window, GWL_EXSTYLE);
        let wanted = click_through_styles(current, through);
        if wanted != current {
            SetWindowLongPtrW(window, GWL_EXSTYLE, wanted);
        }
    }
}

/// Hands the keyboard to a window: the overlay when it unlocks, the game when it locks again.
pub fn focus(window: isize) {
    // SAFETY: plain call; Windows may refuse, which leaves the focus where it is.
    unsafe { SetForegroundWindow(hwnd(window)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_is_click_through_and_never_activates() {
        let locked = overlay_styles(0, true);
        for flag in [
            WS_EX_TOOLWINDOW,
            WS_EX_TOPMOST,
            WS_EX_LAYERED,
            WS_EX_TRANSPARENT,
            WS_EX_NOACTIVATE,
        ] {
            assert_ne!(locked & flag as isize, 0);
        }
        let unlocked = overlay_styles(locked, false);
        assert_eq!(unlocked & WS_EX_TRANSPARENT as isize, 0);
        assert_eq!(unlocked & WS_EX_NOACTIVATE as isize, 0);
        assert_ne!(unlocked & WS_EX_TOOLWINDOW as isize, 0);
        // Other styles of the window stay.
        assert_ne!(overlay_styles(0x0200_0000, true) & 0x0200_0000, 0);
    }

    #[test]
    fn a_zone_under_the_cursor_takes_the_click_without_activating() {
        let locked = overlay_styles(0, true);
        let taking = click_through_styles(locked, false);
        assert_eq!(taking & WS_EX_TRANSPARENT as isize, 0);
        assert_ne!(taking & WS_EX_NOACTIVATE as isize, 0);
        assert_eq!(click_through_styles(taking, true), locked);
    }

    #[test]
    fn the_client_area_covers_the_games() {
        let game = Rect {
            x: 100,
            y: 50,
            width: 1280,
            height: 720,
        };
        assert_eq!(outer_rect(game, (0, 0, 0, 0)), game);
        assert_eq!(
            outer_rect(game, (8, 0, 8, 8)),
            Rect {
                x: 92,
                y: 50,
                width: 1296,
                height: 728
            }
        );
    }

    #[test]
    fn the_desktop_has_a_name() {
        assert!(desktop_name().is_some_and(|name| !name.is_empty()));
    }
}
