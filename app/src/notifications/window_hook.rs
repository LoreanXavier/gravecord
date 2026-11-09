//! Watches taskbar-button creation on the GPUI window's own thread, including Explorer restarts.
use super::Command;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
    mpsc::SyncSender,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                DestroyIcon, HICON, RegisterWindowMessageW, SendMessageW, WM_NCDESTROY, WM_SETICON,
            },
        },
    },
    core::w,
};

const SUBCLASS_ID: usize = 1;
struct State {
    ready: bool,
    binding: usize,
    sender: Option<SyncSender<Command>>,
    generation: Option<Arc<AtomicUsize>>,
    icons: Option<WindowIcons>,
}

struct WindowIcons {
    set: crate::branding::IconSet,
    small: HICON,
    large: HICON,
}
impl Drop for WindowIcons {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyIcon(self.small);
            let _ = DestroyIcon(self.large);
        }
    }
}

pub(super) fn refresh_icons(window: &gpui::Window) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    let mut pointer = 0;
    if !unsafe { GetWindowSubclass(hwnd, Some(callback), SUBCLASS_ID, Some(&mut pointer)) }
        .as_bool()
    {
        return;
    }
    let set = crate::branding::selected();
    if unsafe { &*(pointer as *const State) }
        .icons
        .as_ref()
        .is_some_and(|icons| icons.set == set)
    {
        return;
    }
    let Some(small) =
        crate::branding::app_pixels(set, 16).and_then(|p| super::native_icon::create(16, &p).ok())
    else {
        return;
    };
    let Some(large) =
        crate::branding::app_pixels(set, 32).and_then(|p| super::native_icon::create(32, &p).ok())
    else {
        unsafe {
            let _ = DestroyIcon(small);
        }
        return;
    };
    // Only our top-level window owns these handles. Replace both before releasing the previous pair.
    unsafe {
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(0)),
            Some(LPARAM(small.0 as isize)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(1)),
            Some(LPARAM(large.0 as isize)),
        );
        (*(pointer as *mut State)).icons = Some(WindowIcons { set, small, large });
    }
}
fn message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) })
}
fn hwnd(window: &gpui::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
        _ => None,
    }
}

pub(super) fn install(window: &gpui::Window) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    let mut old = 0;
    if unsafe { GetWindowSubclass(hwnd, Some(callback), SUBCLASS_ID, Some(&mut old)) }.as_bool() {
        return;
    }
    if message() == 0 {
        return;
    }
    let state = Box::into_raw(Box::new(State {
        ready: false,
        binding: 0,
        sender: None,
        generation: None,
        icons: None,
    }));
    if !unsafe { SetWindowSubclass(hwnd, Some(callback), SUBCLASS_ID, state as usize) }.as_bool() {
        unsafe {
            drop(Box::from_raw(state));
        }
        tracing::warn!("taskbar window observer unavailable");
    }
}

pub(super) struct Binding {
    hwnd: HWND,
    id: usize,
}
pub(super) fn bind(
    window: &gpui::Window,
    sender: &SyncSender<Command>,
    generation: &Arc<AtomicUsize>,
) -> Option<Binding> {
    let hwnd = hwnd(window)?;
    let mut pointer = 0;
    if !unsafe { GetWindowSubclass(hwnd, Some(callback), SUBCLASS_ID, Some(&mut pointer)) }
        .as_bool()
    {
        return None;
    }
    // install, bind, callback and Binding::drop all run on this window's thread.
    let state = unsafe { &mut *(pointer as *mut State) };
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    state.binding = NEXT.fetch_add(1, Ordering::Relaxed);
    state.sender = Some(sender.clone());
    state.generation = Some(generation.clone());
    if state.ready {
        generation.fetch_add(1, Ordering::Release);
        let _ = sender.try_send(Command::TaskbarReady);
    }
    Some(Binding {
        hwnd,
        id: state.binding,
    })
}
impl Drop for Binding {
    fn drop(&mut self) {
        let mut pointer = 0;
        if unsafe { GetWindowSubclass(self.hwnd, Some(callback), SUBCLASS_ID, Some(&mut pointer)) }
            .as_bool()
        {
            let state = unsafe { &mut *(pointer as *mut State) };
            if state.binding == self.id {
                state.sender = None;
                state.generation = None;
            }
        }
    }
}

unsafe extern "system" fn callback(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(callback), id);
            drop(Box::from_raw(data as *mut State));
        }
    } else if msg == message() {
        let state = unsafe { &mut *(data as *mut State) };
        state.ready = true;
        if let Some(generation) = &state.generation {
            generation.fetch_add(1, Ordering::Release);
        }
        if let Some(sender) = &state.sender {
            let _ = sender.try_send(Command::TaskbarReady);
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
