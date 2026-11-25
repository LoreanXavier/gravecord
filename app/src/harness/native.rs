//! Input through the owned window's Win32 procedure, without moving the desktop pointer.
use gpui::{AnyWindowHandle, AsyncApp, Pixels, Point};

#[cfg(windows)]
fn owned_hidden_window(window: &gpui::Window) -> Result<windows::Win32::Foundation::HWND, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::{HANDLE, HWND},
        System::{
            StationsAndDesktops::{GetThreadDesktop, GetUserObjectInformationW, UOI_NAME},
            Threading::GetCurrentThreadId,
        },
        UI::WindowsAndMessaging::GetWindowThreadProcessId,
    };
    let handle = HasWindowHandle::window_handle(window).map_err(|error| error.to_string())?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err("not a Win32 window".into());
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let mut process = 0;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process)) };
    if process != std::process::id() || thread != unsafe { GetCurrentThreadId() } {
        return Err("window is not owned by this UI thread".into());
    }
    let desktop = unsafe { GetThreadDesktop(thread) }.map_err(|error| error.to_string())?;
    let mut needed = 0;
    let _ = unsafe {
        GetUserObjectInformationW(HANDLE(desktop.0), UOI_NAME, None, 0, Some(&mut needed))
    };
    if needed == 0 || !(needed as usize).is_multiple_of(std::mem::size_of::<u16>()) {
        return Err("desktop name unavailable".into());
    }
    let mut name = vec![0u16; needed as usize / std::mem::size_of::<u16>()];
    unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            needed,
            None,
        )
    }
    .map_err(|error| error.to_string())?;
    let end = name
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(name.len());
    // The desktop created by xtask::headless; refuse a manually launched capture on the user's desktop.
    if String::from_utf16_lossy(&name[..end]) != "gravecord-headless" {
        return Err("window is not on the hidden test desktop".into());
    }
    Ok(hwnd)
}

pub(crate) fn allow_fullscreen(window: &mut gpui::Window) -> Result<(), String> {
    #[cfg(windows)]
    {
        owned_hidden_window(window)?;
        window.allow_native_fullscreen_in_inert_tests();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Err("native fullscreen test requires Windows".into())
    }
}

pub(crate) fn window_state(window: &gpui::Window) -> String {
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::RECT,
            UI::WindowsAndMessaging::{
                GWL_STYLE, GetWindowLongPtrW, GetWindowRect, IsIconic, IsZoomed,
            },
        };
        let hwnd = match owned_hidden_window(window) {
            Ok(hwnd) => hwnd,
            Err(error) => return error,
        };
        let mut rect = RECT::default();
        if let Err(error) = unsafe { GetWindowRect(hwnd, &mut rect) } {
            return error.to_string();
        }
        format!(
            "style={:#x} rect=[{},{},{},{}] minimized={} maximized={} inert={} gpui_inert={}",
            unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) },
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            unsafe { IsIconic(hwnd) }.as_bool(),
            unsafe { IsZoomed(hwnd) }.as_bool(),
            crate::inert::on(),
            gpui::INERT.load(std::sync::atomic::Ordering::Relaxed),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        "native state requires Windows".into()
    }
}

pub(crate) fn close(window: &mut gpui::Window, cx: &mut gpui::App) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::{LPARAM, WPARAM},
            UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
        };
        let hwnd = owned_hidden_window(window)?;
        window.allow_close_in_inert_tests();
        cx.allow_quit_in_inert_tests();
        unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
            .map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (window, cx);
        Err("native close test requires Windows".into())
    }
}

pub(crate) fn activate(window: &gpui::Window, active: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::{LPARAM, WPARAM},
            UI::WindowsAndMessaging::{SendMessageW, WA_ACTIVE, WA_INACTIVE, WM_ACTIVATE},
        };
        let hwnd = owned_hidden_window(window)?;
        let state = if active { WA_ACTIVE } else { WA_INACTIVE };
        unsafe {
            SendMessageW(
                hwnd,
                WM_ACTIVATE,
                Some(WPARAM(state as usize)),
                Some(LPARAM(0)),
            )
        };
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (window, active);
        Err("native window test requires Windows".into())
    }
}

pub(crate) fn minimize_or_restore(window: &gpui::Window, minimize: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::{LPARAM, WPARAM},
            UI::WindowsAndMessaging::{PostMessageW, SC_MINIMIZE, SC_RESTORE, WM_SYSCOMMAND},
        };
        let hwnd = owned_hidden_window(window)?;
        let command = if minimize { SC_MINIMIZE } else { SC_RESTORE };
        unsafe {
            PostMessageW(
                Some(hwnd),
                WM_SYSCOMMAND,
                WPARAM(command as usize),
                LPARAM(0),
            )
        }
        .map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (window, minimize);
        Err("native window test requires Windows".into())
    }
}

#[cfg(windows)]
mod trace {
    use std::sync::Mutex;
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Controls::WM_MOUSELEAVE,
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{WM_MOUSEMOVE, WM_NCDESTROY, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE},
        },
    };
    static EVENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    const ID: usize = 2;
    pub(super) fn install(hwnd: HWND) -> bool {
        unsafe { SetWindowSubclass(hwnd, Some(callback), ID, 0) }.as_bool()
    }
    pub(super) fn take() -> Vec<&'static str> {
        EVENTS
            .lock()
            .map(|mut events| std::mem::take(&mut *events))
            .unwrap_or_default()
    }
    unsafe extern "system" fn callback(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _: usize,
        _: usize,
    ) -> LRESULT {
        let name = match message {
            WM_MOUSEMOVE => Some("WM_MOUSEMOVE"),
            WM_MOUSELEAVE => Some("WM_MOUSELEAVE"),
            WM_NCMOUSEMOVE => Some("WM_NCMOUSEMOVE"),
            WM_NCMOUSELEAVE => Some("WM_NCMOUSELEAVE"),
            _ => None,
        };
        if let Some(name) = name
            && let Ok(mut events) = EVENTS.lock()
            && events.len() < 32
        {
            events.push(name);
        }
        if message == WM_NCDESTROY {
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(callback), ID);
            }
        }
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}

pub(super) fn state(window: AnyWindowHandle, cx: &mut AsyncApp) -> String {
    let observed = cx.update(|cx| {
        window
            .update(cx, |_, window, _| {
                (window.is_window_hovered(), window.mouse_position())
            })
            .ok()
    });
    #[cfg(windows)]
    let events = trace::take();
    #[cfg(not(windows))]
    let events: Vec<&str> = Vec::new();
    format!("window={observed:?} messages={events:?}")
}

#[cfg(windows)]
fn packed_point(x: i32, y: i32) -> Option<isize> {
    let (x, y) = (i16::try_from(x).ok()?, i16::try_from(y).ok()?);
    Some((u32::from(x as u16) | (u32::from(y as u16) << 16)) as isize)
}

#[cfg(windows)]
pub(super) async fn move_pointer(
    window: AnyWindowHandle,
    point: Point<Pixels>,
    cx: &mut AsyncApp,
) -> String {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::{HWND, LPARAM, POINT, WPARAM},
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::{
            HTCLIENT, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_MOUSEMOVE, WM_NCHITTEST,
            WM_NCMOUSEMOVE,
        },
    };
    let target = cx.update(|cx| {
        window
            .update(cx, |_, window, _| {
                let RawWindowHandle::Win32(handle) =
                    HasWindowHandle::window_handle(window).ok()?.as_raw()
                else {
                    return None;
                };
                let hwnd = HWND(handle.hwnd.get() as *mut _);
                if !trace::install(hwnd) {
                    return None;
                }
                let scale = window.scale_factor();
                let (x, y) = (
                    (f32::from(point.x) * scale).round() as i32,
                    (f32::from(point.y) * scale).round() as i32,
                );
                let client = packed_point(x, y)?;
                let mut screen = POINT { x, y };
                if !unsafe { ClientToScreen(hwnd, &mut screen) }.as_bool() {
                    return None;
                }
                Some((
                    handle.hwnd.get() as usize,
                    client,
                    packed_point(screen.x, screen.y)?,
                ))
            })
            .ok()
            .flatten()
    });
    let Some((handle, client, screen)) = target else {
        return "window or coordinates unavailable".into();
    };
    // The UI must be free to receive the sent messages; never send inside Window::update.
    cx.background_executor()
        .spawn(async move {
            let hwnd = HWND(handle as *mut _);
            let send = |message, wparam, lparam| {
                let mut result = 0;
                let sent = unsafe {
                    SendMessageTimeoutW(
                        hwnd,
                        message,
                        WPARAM(wparam),
                        LPARAM(lparam),
                        SMTO_ABORTIFHUNG,
                        2000,
                        Some(&mut result),
                    )
                };
                (sent.0 != 0).then_some(result)
            };
            let Some(hit) = send(WM_NCHITTEST, 0, screen) else {
                return "hit test failed or timed out".into();
            };
            let (message, wparam, position, name) = if hit as u32 == HTCLIENT {
                (WM_MOUSEMOVE, 0, client, "WM_MOUSEMOVE")
            } else {
                (WM_NCMOUSEMOVE, hit, screen, "WM_NCMOUSEMOVE")
            };
            match send(message, wparam, position) {
                Some(_) => format!("{name} hit={}", hit as isize),
                None => format!("{name} failed or timed out; hit={}", hit as isize),
            }
        })
        .await
}

/// Sends `WM_MOUSELEAVE` to the window, as Windows does when the tracked pointer leaves its client area.
#[cfg(windows)]
pub(super) async fn leave(window: AnyWindowHandle, cx: &mut AsyncApp) -> String {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::{
            Controls::WM_MOUSELEAVE,
            WindowsAndMessaging::{SMTO_ABORTIFHUNG, SendMessageTimeoutW},
        },
    };
    let handle = cx.update(|cx| {
        window
            .update(cx, |_, window, _| {
                let RawWindowHandle::Win32(handle) =
                    HasWindowHandle::window_handle(window).ok()?.as_raw()
                else {
                    return None;
                };
                Some(handle.hwnd.get() as usize)
            })
            .ok()
            .flatten()
    });
    let Some(handle) = handle else {
        return "window unavailable".into();
    };
    cx.background_executor()
        .spawn(async move {
            let mut result = 0;
            let sent = unsafe {
                SendMessageTimeoutW(
                    HWND(handle as *mut _),
                    WM_MOUSELEAVE,
                    WPARAM(0),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    2000,
                    Some(&mut result),
                )
            };
            if sent.0 != 0 {
                "WM_MOUSELEAVE".to_owned()
            } else {
                "WM_MOUSELEAVE failed or timed out".to_owned()
            }
        })
        .await
}

#[cfg(not(windows))]
pub(super) async fn leave(_: AnyWindowHandle, _: &mut AsyncApp) -> String {
    "Win32 input is unavailable".into()
}

#[cfg(not(windows))]
pub(super) async fn move_pointer(_: AnyWindowHandle, _: Point<Pixels>, _: &mut AsyncApp) -> String {
    "Win32 input is unavailable".into()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn screen_coordinates_preserve_signed_words_and_reject_overflow() {
        let value = packed_point(-120, -45).unwrap();
        assert_eq!(value as u16 as i16, -120);
        assert_eq!((value >> 16) as u16 as i16, -45);
        assert!(packed_point(32768, 0).is_none());
        assert!(packed_point(0, -32769).is_none());
    }
}

/// `window move <x> <y>`: puts the window's top-left corner at a screen point, as a drag of its title bar does.
pub(crate) fn move_window(window: &gpui::Window, x: i32, y: i32) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
        };
        let hwnd = owned_hidden_window(window)?;
        unsafe {
            SetWindowPos(
                hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (window, x, y);
        Err("native window test requires Windows".into())
    }
}

/// `webframes`: the windows the app's window owns that web views live in (the surfaces of `dc-webhost`), read back
/// from Win32, against the chat list's viewport on the screen. A surface that shows outside the chat (over the header,
/// the composer, or past the window) is `OUTSIDE`; the last line counts them. `chat` is the list's viewport in the
/// window's logical pixels.
pub(crate) fn web_surfaces(window: &gpui::Window, chat: gpui::Bounds<gpui::Pixels>) -> String {
    #[cfg(windows)]
    {
        use std::fmt::Write as _;
        use windows::Win32::{
            Foundation::{HWND, LPARAM, POINT, RECT},
            Graphics::Gdi::ClientToScreen,
            UI::WindowsAndMessaging::{
                EnumWindows, GW_OWNER, GetClassNameW, GetClientRect, GetWindow, GetWindowRect,
                IsIconic, IsWindowVisible,
            },
        };
        let main = match owned_hidden_window(window) {
            Ok(hwnd) => hwnd,
            Err(error) => return format!("webframes: {error}"),
        };
        unsafe extern "system" fn collect(hwnd: HWND, list: LPARAM) -> windows::core::BOOL {
            let list = unsafe { &mut *(list.0 as *mut (HWND, Vec<HWND>)) };
            if unsafe { GetWindow(hwnd, GW_OWNER) }.ok() == Some(list.0) {
                let mut class = [0u16; 32];
                let len = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
                if String::from_utf16_lossy(&class[..len]).eq_ignore_ascii_case("static") {
                    list.1.push(hwnd);
                }
            }
            true.into()
        }
        let mut list = (main, Vec::new());
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut list as *mut _ as isize));
        }
        let scale = window.scale_factor();
        let mut origin = POINT::default();
        let mut client = RECT::default();
        unsafe {
            let _ = ClientToScreen(main, &mut origin);
            let _ = GetClientRect(main, &mut client);
        }
        let edge = |value: gpui::Pixels| (f32::from(value) * scale).round() as i32;
        // The chat's viewport on the screen, cut to the window's client area.
        let chat = RECT {
            left: origin.x + edge(chat.origin.x).max(0),
            top: origin.y + edge(chat.origin.y).max(0),
            right: origin.x + edge(chat.origin.x + chat.size.width).min(client.right),
            bottom: origin.y + edge(chat.origin.y + chat.size.height).min(client.bottom),
        };
        let mut out = String::new();
        let minimized = unsafe { IsIconic(main) }.as_bool();
        let _ = writeln!(
            out,
            "webframes: window origin=({},{}) client={}x{} minimized={minimized} chat=[{},{},{},{}]",
            origin.x,
            origin.y,
            client.right,
            client.bottom,
            chat.left,
            chat.top,
            chat.right,
            chat.bottom
        );
        let mut outside = 0;
        let mut shown = 0;
        for hwnd in &list.1 {
            let mut rect = RECT::default();
            let visible = unsafe { IsWindowVisible(*hwnd) }.as_bool();
            unsafe {
                let _ = GetWindowRect(*hwnd, &mut rect);
            }
            let inside = rect.left >= chat.left
                && rect.top >= chat.top
                && rect.right <= chat.right
                && rect.bottom <= chat.bottom;
            let verdict = match (visible, inside) {
                (false, _) => "hidden",
                (true, true) => "inside",
                (true, false) => "OUTSIDE",
            };
            shown += usize::from(visible);
            outside += usize::from(visible && !inside);
            let _ = writeln!(
                out,
                "webframes: surface visible={visible} rect=[{},{},{},{}] {verdict}",
                rect.left, rect.top, rect.right, rect.bottom
            );
        }
        let _ = writeln!(
            out,
            "webframes: {} surfaces, {shown} shown, {outside} outside",
            list.1.len()
        );
        out
    }
    #[cfg(not(windows))]
    {
        let _ = (window, chat);
        "webframes requires Windows\n".into()
    }
}
