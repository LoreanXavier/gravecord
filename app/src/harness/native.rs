//! Input through the owned window's Win32 procedure, without moving the desktop pointer.
use gpui::{AnyWindowHandle, AsyncApp, Pixels, Point};

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
