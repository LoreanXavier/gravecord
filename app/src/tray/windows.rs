//! UI-thread-owned Shell notification icon. Callbacks retain state across nested native menus.
use super::{Action, Icon, State};
use crate::{branding::IconSet, net::Update};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::OnceLock,
};
use tokio::sync::mpsc::UnboundedSender;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};

const SUBCLASS: usize = 3;
const ICON_ID: u32 = 1;
const KEY_SELECT: u32 = NIN_SELECT | NINF_KEY;
/// Applies the desired state off the render path, and retries a failed update on a backoff.
const TIMER_ID: usize = 0x6743;
/// `SetTimer` rounds anything below 10 ms up to it.
const COALESCE_MS: u32 = 10;

/// Milliseconds to wait before the next try after `failures` consecutive failures: 0.5 s, doubling, at most 30 s.
fn retry_delay(failures: u32) -> u32 {
    (500u32 << failures.saturating_sub(1).min(7)).min(30_000)
}

fn callback_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("gravecord.TrayCallback")) })
}
fn restart_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}
fn setup_quit_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("gravecord.QuitForSetup")) })
}

struct OwnedIcon(HICON);
impl Drop for OwnedIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyIcon(self.0);
        }
    }
}
struct Menu(HMENU);
impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
    }
}
type IconKey = (IconSet, Icon, u32);
struct Native {
    hwnd: HWND,
    desired: Cell<State>,
    applied: Cell<Option<IconKey>>,
    present: Cell<bool>,
    enabled: Cell<bool>,
    exiting: Cell<bool>,
    hooked: Cell<bool>,
    warned: Cell<bool>,
    /// Consecutive failed updates; 0 once one works.
    failures: Cell<u32>,
    timer_set: Cell<bool>,
    icons: RefCell<Vec<(IconKey, OwnedIcon)>>,
    sender: UnboundedSender<Update>,
}

pub(super) struct Binding(Rc<Native>);
impl Binding {
    pub(super) fn new(
        window: &gpui::Window,
        sender: UnboundedSender<Update>,
        state: State,
    ) -> Option<Self> {
        let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window).ok()?.as_raw()
        else {
            return None;
        };
        if callback_message() == 0 || restart_message() == 0 || setup_quit_message() == 0 {
            return None;
        }
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        let mut existing = 0;
        if unsafe { GetWindowSubclass(hwnd, Some(callback), SUBCLASS, Some(&mut existing)) }
            .as_bool()
        {
            return None;
        }
        let native = Rc::new(Native {
            hwnd,
            desired: Cell::new(state),
            applied: Cell::new(None),
            present: Cell::new(false),
            enabled: Cell::new(true),
            exiting: Cell::new(false),
            hooked: Cell::new(false),
            warned: Cell::new(false),
            failures: Cell::new(0),
            timer_set: Cell::new(false),
            icons: RefCell::new(Vec::new()),
            sender,
        });
        let pointer = Rc::into_raw(native.clone());
        if !unsafe { SetWindowSubclass(native.hwnd, Some(callback), SUBCLASS, pointer as usize) }
            .as_bool()
        {
            unsafe {
                drop(Rc::from_raw(pointer));
            }
            return None;
        }
        native.hooked.set(true);
        // This runs inside a render. The shell call happens a moment later, from the window's message loop.
        native.request();
        Some(Self(native))
    }
    pub(super) fn refresh(&self, state: State) {
        self.0.desired.set(state);
        self.0.request();
    }
}
impl Drop for Binding {
    fn drop(&mut self) {
        let native = &self.0;
        native.enabled.set(false);
        native.stop_timer();
        native.remove();
        if native.hooked.get()
            && unsafe { RemoveWindowSubclass(native.hwnd, Some(callback), SUBCLASS) }.as_bool()
        {
            native.hooked.set(false);
            unsafe {
                drop(Rc::from_raw(Rc::as_ptr(native)));
            }
        }
        // If removal fails, the disabled subclass keeps its reference until WM_NCDESTROY.
    }
}

impl Native {
    fn data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: ICON_ID,
            ..Default::default()
        }
    }
    fn icon(&self, key: IconKey) -> Option<HICON> {
        if let Some((_, icon)) = self.icons.borrow().iter().find(|(known, _)| *known == key) {
            return Some(icon.0);
        }
        let (size, pixels) = crate::branding::tray_pixels(key.0, key.1, key.2)?;
        let icon = crate::notifications::native_icon::create(size as i32, &pixels).ok()?;
        self.icons.borrow_mut().push((key, OwnedIcon(icon)));
        Some(icon)
    }
    /// Asks for the desired state to be applied soon. While updates keep failing the backoff timer decides when to
    /// try again, so a render (or a burst of them) never calls the shell.
    fn request(&self) {
        if self.failures.get() == 0 {
            self.schedule(COALESCE_MS);
        }
    }
    fn schedule(&self, milliseconds: u32) {
        if !self.enabled.get() || !self.hooked.get() || self.timer_set.get() {
            return;
        }
        if unsafe { SetTimer(Some(self.hwnd), TIMER_ID, milliseconds, None) } != 0 {
            self.timer_set.set(true);
        }
    }
    fn stop_timer(&self) {
        if self.timer_set.replace(false) {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_ID);
            }
        }
    }
    fn apply(&self) {
        if !self.enabled.get() || !self.hooked.get() {
            return;
        }
        let state = self.desired.get();
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(1) as u32;
        let key = (state.icons, state.icon(), size);
        if self.present.get() && self.applied.get() == Some(key) {
            return;
        }
        let Some(icon) = self.icon(key) else {
            self.failed();
            return;
        };
        let mut data = self.data();
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = callback_message();
        data.hIcon = icon;
        for (slot, unit) in data.szTip.iter_mut().zip("gravecord".encode_utf16()) {
            *slot = unit;
        }
        let existed = self.present.get();
        let operation = if existed { NIM_MODIFY } else { NIM_ADD };
        if !unsafe { Shell_NotifyIconW(operation, &data) }.as_bool() {
            // A failed MODIFY may mean Explorer discarded the icon. Re-add on the next refresh.
            if existed {
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                }
            }
            self.present.set(false);
            self.applied.set(None);
            self.failed();
            return;
        }
        if !existed {
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            if !unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) }.as_bool() {
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                }
                self.failed();
                return;
            }
        }
        self.present.set(true);
        self.applied.set(Some(key));
        self.warned.set(false);
        self.failures.set(0);
    }
    fn failed(&self) {
        if !self.warned.replace(true) {
            tracing::warn!("system tray update unavailable");
        }
        // Retry later on a growing delay; the Explorer restart message retries at once on its own.
        let failures = self.failures.get().saturating_add(1);
        self.failures.set(failures);
        self.schedule(retry_delay(failures));
        if !self.present.get() && !unsafe { IsWindowVisible(self.hwnd) }.as_bool() {
            self.show_without_focus();
        }
    }
    /// Makes the window visible without taking the foreground: the app may be starting in the background and the
    /// icon that would have represented it is missing, but that is no reason to steal focus from what the user is doing.
    fn show_without_focus(&self) {
        unsafe {
            let command = if IsIconic(self.hwnd).as_bool() {
                SW_RESTORE
            } else {
                SW_SHOWNOACTIVATE
            };
            let _ = ShowWindow(self.hwnd, command);
        }
    }
    fn remove(&self) {
        if self.present.replace(false) {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &self.data());
            }
        }
        self.applied.set(None);
    }
    fn show(&self) {
        unsafe {
            let command = if IsIconic(self.hwnd).as_bool() {
                SW_RESTORE
            } else {
                SW_SHOW
            };
            let _ = ShowWindow(self.hwnd, command);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }
    fn menu(&self, wparam: WPARAM) {
        let Ok(handle) = (unsafe { CreatePopupMenu() }) else {
            return;
        };
        let menu = Menu(handle);
        let items = self.desired.get().menu();
        for item in &items {
            let (flags, id, label) = match item {
                None => (MF_SEPARATOR, 0, Vec::new()),
                Some(item) => {
                    let mut flags = MF_STRING;
                    if !item.enabled {
                        flags |= MF_GRAYED;
                    }
                    if item.checked {
                        flags |= MF_CHECKED;
                    }
                    (
                        flags,
                        item.action.map_or(0, |action| action as usize),
                        item.label.encode_utf16().chain(Some(0)).collect(),
                    )
                }
            };
            if unsafe { AppendMenuW(menu.0, flags, id, PCWSTR(label.as_ptr())) }.is_err() {
                return;
            }
        }
        let mut point = POINT {
            x: wparam.0 as u16 as i16 as i32,
            y: (wparam.0 >> 16) as u16 as i16 as i32,
        };
        if point.x == -1 && point.y == -1 && unsafe { GetCursorPos(&mut point) }.is_err() {
            return;
        }
        let command = unsafe {
            let _ = SetForegroundWindow(self.hwnd);
            let command = TrackPopupMenu(
                menu.0,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                self.hwnd,
                None,
            )
            .0 as usize;
            let _ = PostMessageW(Some(self.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            command
        };
        let action = items.into_iter().flatten().find_map(|item| {
            item.action
                .filter(|action| *action as usize == command && item.enabled)
        });
        if let Some(action) = action
            && self.enabled.get()
            && self.desired.get().allows(action)
        {
            if action == Action::VoiceSettings {
                self.show();
            }
            if action == Action::Quit {
                self.exiting.set(true);
            }
            let _ = self.sender.send(Update::Tray(action));
        }
    }
}

impl Native {
    fn hides_close(&self) -> bool {
        !self.exiting.get() && self.desired.get().hide_on_close(self.present.get())
    }
}

unsafe extern "system" fn callback(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    let pointer = data as *const Native;
    // TrackPopupMenu pumps messages; hold a separate Rc while another callback can destroy the window.
    let native = unsafe {
        Rc::increment_strong_count(pointer);
        Rc::from_raw(pointer)
    };
    if message == WM_NCDESTROY {
        native.enabled.set(false);
        native.remove();
        native.hooked.set(false);
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(callback), id);
            drop(Rc::from_raw(pointer));
        }
    } else if native.enabled.get() {
        if message != 0 && message == setup_quit_message() {
            // Setup posts this before WM_CLOSE. Keep the following close out of the tray path,
            // even if a network update refreshes the desired tray state before shutdown runs.
            native.exiting.set(true);
            let _ = native.sender.send(Update::Tray(Action::Quit));
            return LRESULT(0);
        } else if message == WM_TIMER && wparam.0 == TIMER_ID {
            native.stop_timer();
            native.apply();
            return LRESULT(0);
        } else if message == restart_message() {
            // Explorer was restarted: its tray is empty and new, so start over without waiting out any backoff.
            native.stop_timer();
            native.failures.set(0);
            native.present.set(false);
            native.applied.set(None);
            native.apply();
        } else if message == WM_SETTINGCHANGE {
            native.request();
        } else if message == WM_CLOSE && native.hides_close() {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            return LRESULT(0);
        } else if message == callback_message() && (lparam.0 as u32 >> 16) == ICON_ID {
            match lparam.0 as u32 & 0xffff {
                NIN_SELECT | KEY_SELECT => native.show(),
                WM_CONTEXTMENU => native.menu(wparam),
                _ => {}
            }
            return LRESULT(0);
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_updates_back_off_instead_of_retrying_every_frame() {
        let delays: Vec<u32> = (1..=9).map(retry_delay).collect();
        assert_eq!(
            delays,
            [500, 1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]
        );
        assert_eq!(retry_delay(0), 500);
        assert_eq!(retry_delay(u32::MAX), 30_000);
    }

    #[test]
    fn setup_quit_dispatch_bypasses_tray_even_after_state_refresh() {
        let message = setup_quit_message();
        assert_ne!(message, 0);
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let native = Rc::new(Native {
            hwnd: HWND::default(),
            desired: Cell::new(State::default()),
            applied: Cell::new(None),
            present: Cell::new(true),
            enabled: Cell::new(true),
            exiting: Cell::new(false),
            hooked: Cell::new(false),
            warned: Cell::new(false),
            failures: Cell::new(0),
            timer_set: Cell::new(false),
            icons: RefCell::new(Vec::new()),
            sender,
        });
        assert!(native.hides_close());
        // This message branch only queues an action; it never uses the HWND or touches the shell.
        unsafe {
            callback(
                HWND::default(),
                message,
                WPARAM(0),
                LPARAM(0),
                SUBCLASS,
                Rc::as_ptr(&native) as usize,
            );
        }
        assert!(matches!(
            receiver.try_recv(),
            Ok(Update::Tray(Action::Quit))
        ));
        native.desired.set(State::default());
        assert!(!native.hides_close());
        assert_eq!(Rc::strong_count(&native), 1);
    }
}
