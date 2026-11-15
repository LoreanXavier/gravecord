//! The low-level keyboard and mouse hooks (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`), on a thread of their own with a message
//! loop, because Windows delivers hook calls through the thread that installed them.
//!
//! The hooks only read: every event is passed on with `CallNextHookEx`, so no key or button is swallowed. What the
//! callbacks keep is which keybind keys are down (in the [`Matcher`]); no key, code or text is logged or stored.
//! Windows removes a low-level hook whose callback is slow, so the callbacks do a lock, a few comparisons and the
//! caller's `on_change`, which must be quick too.

use crate::matcher::{MOUSE_BUTTON, Matcher, Part};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT,
    PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
    WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN,
    WM_XBUTTONUP,
};

const WM_SYNC_MOUSE_HOOK: u32 = WM_APP + 1;

fn has_mouse_part(shortcut: &[Part]) -> bool {
    shortcut.iter().any(|&[kind, _, _]| kind == MOUSE_BUTTON)
}

/// What the callbacks share with the thread that started the hook.
struct Shared {
    matcher: Mutex<Matcher>,
    want_mouse: AtomicBool,
    mouse_installed: AtomicBool,
    on_change: Box<dyn Fn(bool) + Send + Sync>,
}

impl Shared {
    fn keyboard(&self, message: u32, code: u32) {
        let down = match message {
            WM_KEYDOWN | WM_SYSKEYDOWN => true,
            WM_KEYUP | WM_SYSKEYUP => false,
            _ => return,
        };
        self.apply(|matcher| matcher.key(code, down));
    }

    /// `mouse_data` is `MSLLHOOKSTRUCT::mouseData`, whose high word names the X button.
    fn mouse(&self, message: u32, mouse_data: u32) {
        let Some((code, down)) = mouse_event(message, mouse_data) else {
            return;
        };
        self.apply(|matcher| matcher.button(code, down));
    }

    fn prune_stale_modifiers(&self, current_vk: u32) {
        self.apply(|matcher| {
            if matcher.held_keys().is_empty() {
                return None;
            }
            let stale: Vec<u32> = matcher
                .held_keys()
                .iter()
                .copied()
                .filter(|&vk| {
                    vk != current_vk
                        && is_modifier_vk(vk)
                        && (unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000) == 0
                })
                .collect();
            let mut changed = None;
            for vk in stale {
                if let Some(held) = matcher.force_up(vk) {
                    changed = Some(held);
                }
            }
            changed
        });
    }

    fn apply(&self, change: impl FnOnce(&mut Matcher) -> Option<bool>) {
        let changed = match self.matcher.lock() {
            Ok(mut matcher) => change(&mut matcher),
            Err(_) => None,
        };
        if let Some(held) = changed {
            (self.on_change)(held);
        }
    }
}

/// The native button number of a mouse message and whether it went down. The primary (left) button is ignored, as the
/// official recorder ignores it (module 368484: `s !== +!isWindows`, which is 0 on Windows). The others count from it as
/// Windows raw input numbers them: right 1, middle 2, X1 3, X2 4.
fn mouse_event(message: u32, mouse_data: u32) -> Option<(u32, bool)> {
    let x_button = || match mouse_data >> 16 {
        1 => Some(3),
        2 => Some(4),
        _ => None,
    };
    match message {
        WM_RBUTTONDOWN => Some((1, true)),
        WM_RBUTTONUP => Some((1, false)),
        WM_MBUTTONDOWN => Some((2, true)),
        WM_MBUTTONUP => Some((2, false)),
        WM_XBUTTONDOWN => x_button().map(|code| (code, true)),
        WM_XBUTTONUP => x_button().map(|code| (code, false)),
        _ => None,
    }
}

fn is_modifier_vk(vk: u32) -> bool {
    matches!(vk, 16..=18 | 91 | 92 | 160..=165)
}

thread_local! {
    /// The callbacks have no user data, but Windows runs them on the thread that installed the hook, so each hook
    /// thread keeps its own state here. Several hooks (push to talk, the clip keybind) can run side by side.
    static ACTIVE: std::cell::RefCell<Option<Arc<Shared>>> = const { std::cell::RefCell::new(None) };
}

fn active() -> Option<Arc<Shared>> {
    ACTIVE.with(|active| active.borrow().clone())
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && lparam != 0
        && let Some(shared) = active()
    {
        // SAFETY: for HC_ACTION, lParam points at the KBDLLHOOKSTRUCT of this event.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        shared.prune_stale_modifiers(info.vkCode);
        shared.keyboard(wparam as u32, info.vkCode);
    }
    // SAFETY: passing the call on unchanged is what a hook must do.
    unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Fast-path return for WM_MOUSEMOVE and unbound events without touching TLS or locks.
    if code == HC_ACTION as i32
        && lparam != 0
        && matches!(
            wparam as u32,
            WM_RBUTTONDOWN
                | WM_RBUTTONUP
                | WM_MBUTTONDOWN
                | WM_MBUTTONUP
                | WM_XBUTTONDOWN
                | WM_XBUTTONUP
        )
        && let Some(shared) = active()
    {
        // SAFETY: for HC_ACTION, lParam points at the MSLLHOOKSTRUCT of this event.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        shared.mouse(wparam as u32, info.mouseData);
    }
    // SAFETY: as above.
    unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
}

/// A running hook. Dropping it, or [`Hook::stop`], removes the hooks and lets go of push to talk.
pub struct Hook {
    shared: Arc<Shared>,
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

/// Installs the hooks on a new thread. `on_change(true)` is called when the whole keybind goes down and
/// `on_change(false)` when it is no longer down, including when the hook stops with it held.
pub fn start(
    shortcut: Vec<Part>,
    on_change: impl Fn(bool) + Send + Sync + 'static,
) -> Result<Hook, &'static str> {
    let want_mouse = has_mouse_part(&shortcut);
    let shared = Arc::new(Shared {
        matcher: Mutex::new(Matcher::new(shortcut)),
        want_mouse: AtomicBool::new(want_mouse),
        mouse_installed: AtomicBool::new(false),
        on_change: Box::new(on_change),
    });
    let (ready, installed) = std::sync::mpsc::channel::<Result<u32, &'static str>>();
    let for_thread = shared.clone();
    let thread = std::thread::Builder::new()
        .name("dc-hotkey".into())
        .spawn(move || {
            ACTIVE.with(|active| *active.borrow_mut() = Some(for_thread.clone()));
            run(&ready, &for_thread);
        });
    let thread = match thread {
        Ok(thread) => thread,
        Err(_) => return Err("Could not start the keyboard hook thread"),
    };
    match installed.recv() {
        Ok(Ok(thread_id)) => Ok(Hook {
            shared,
            thread_id,
            thread: Some(thread),
        }),
        other => {
            let _ = thread.join();
            Err(other
                .unwrap_or(Err("The keyboard hook thread ended"))
                .unwrap_err())
        }
    }
}

/// The hook thread: install keyboard hook (and mouse hook only if needed), pump messages until `WM_QUIT`, remove them.
fn run(ready: &std::sync::mpsc::Sender<Result<u32, &'static str>>, shared: &Shared) {
    // SAFETY: a null name asks for this process's own module handle, which hooks in this process need.
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    // SAFETY: both procedures match HOOKPROC and live as long as the process.
    let keyboard = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) };
    if keyboard.is_null() {
        let _ = ready.send(Err("Windows refused the keyboard hook"));
        return;
    }
    let mut mouse: HHOOK = if shared.want_mouse.load(Ordering::Acquire) {
        // SAFETY: as above.
        unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) }
    } else {
        ptr::null_mut()
    };
    shared
        .mouse_installed
        .store(!mouse.is_null(), Ordering::Release);
    // The mouse hook is optional: without it, only the keyboard works.
    let mut message = MSG {
        hwnd: ptr::null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
    };
    // Create the thread message queue before publishing `thread_id` so an immediate
    // `PostThreadMessageW` from `set_shortcut` or `Drop` is never dropped.
    // SAFETY: `message` is valid and `PM_NOREMOVE` initializes the queue without consuming events.
    unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_NOREMOVE) };
    // SAFETY: plain call.
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
    // SAFETY: `message` is a valid MSG; GetMessageW returns 0 on WM_QUIT and -1 on error.
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        if message.message == WM_SYNC_MOUSE_HOOK {
            let wanted = shared.want_mouse.load(Ordering::Acquire);
            if wanted && mouse.is_null() {
                // SAFETY: `mouse_proc` matches HOOKPROC and lives as long as the process.
                mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) };
            } else if !wanted && !mouse.is_null() {
                unhook(mouse);
                mouse = ptr::null_mut();
            }
            shared
                .mouse_installed
                .store(!mouse.is_null(), Ordering::Release);
        }
    }
    unhook(keyboard);
    unhook(mouse);
    shared.mouse_installed.store(false, Ordering::Release);
}

fn unhook(hook: HHOOK) {
    if !hook.is_null() {
        // SAFETY: a handle this thread installed.
        unsafe { UnhookWindowsHookEx(hook) };
    }
}

impl Hook {
    /// The keybind changed on the settings page.
    pub fn set_shortcut(&self, shortcut: Vec<Part>) {
        let want_mouse = has_mouse_part(&shortcut);
        let prev = self.shared.want_mouse.swap(want_mouse, Ordering::AcqRel);
        self.shared.apply(|matcher| matcher.set_shortcut(shortcut));
        if prev != want_mouse {
            // SAFETY: posts a wakeup so the hook thread reconciles WH_MOUSE_LL on its own thread.
            unsafe { PostThreadMessageW(self.thread_id, WM_SYNC_MOUSE_HOOK, 0, 0) };
        }
    }

    /// Removes the hooks and releases push to talk.
    pub fn stop(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        // SAFETY: posts WM_QUIT to the hook thread's queue.
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        let _ = thread.join();
        // Whatever was down is forgotten, and a held push to talk ends.
        self.shared.apply(|matcher| matcher.release_all());
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::{KEYBOARD_KEY, MOUSE_BUTTON};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn shared(shortcut: Vec<Part>) -> (Arc<Shared>, Arc<Mutex<Vec<bool>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        let want_mouse = has_mouse_part(&shortcut);
        let shared = Arc::new(Shared {
            want_mouse: AtomicBool::new(want_mouse),
            mouse_installed: AtomicBool::new(false),
            matcher: Mutex::new(Matcher::new(shortcut)),
            on_change: Box::new(move |held| sink.lock().unwrap().push(held)),
        });
        (shared, log)
    }

    #[test]
    fn keyboard_messages_hold_and_release_the_keybind() {
        let (shared, log) = shared(vec![[KEYBOARD_KEY, 162, 1], [KEYBOARD_KEY, 120, 1]]);
        shared.keyboard(WM_KEYDOWN, 162);
        shared.keyboard(WM_SYSKEYDOWN, 120);
        shared.keyboard(WM_SYSKEYUP, 120);
        shared.keyboard(WM_KEYDOWN, 120);
        shared.keyboard(WM_KEYUP, 120);
        shared.keyboard(0x0102, 65); // WM_CHAR is not a key event.
        assert_eq!(*log.lock().unwrap(), [true, false, true, false]);
    }

    #[test]
    fn mouse_messages_map_to_native_button_numbers_and_skip_the_primary() {
        assert_eq!(mouse_event(WM_RBUTTONDOWN, 0), Some((1, true)));
        assert_eq!(mouse_event(WM_MBUTTONUP, 0), Some((2, false)));
        assert_eq!(mouse_event(WM_XBUTTONDOWN, 1 << 16), Some((3, true)));
        assert_eq!(mouse_event(WM_XBUTTONUP, 2 << 16), Some((4, false)));
        assert_eq!(mouse_event(WM_XBUTTONDOWN, 3 << 16), None);
        // WM_LBUTTONDOWN and WM_MOUSEMOVE are not buttons we use.
        assert_eq!(mouse_event(0x0201, 0), None);
        assert_eq!(mouse_event(0x0200, 0), None);

        let (shared, log) = shared(vec![[MOUSE_BUTTON, 3, 1]]);
        shared.mouse(WM_XBUTTONDOWN, 1 << 16);
        shared.mouse(WM_XBUTTONUP, 1 << 16);
        shared.mouse(WM_RBUTTONDOWN, 0);
        assert_eq!(*log.lock().unwrap(), [true, false]);
    }

    /// The hooks install on a thread with a message loop, a second one runs beside the first (push to talk and the clip
    /// keybind) with its own state, and stopping removes the hooks and lets go of a held keybind. No input is sent: it
    /// only watches the system, and the callback keeps nothing, so running this beside a person typing is harmless.
    #[test]
    fn hooks_run_side_by_side_and_stop_releases_push_to_talk() {
        let changes = Arc::new(AtomicUsize::new(0));
        let held = Arc::new(Mutex::new(false));
        let (count, state) = (changes.clone(), held.clone());
        let hook = start(vec![[KEYBOARD_KEY, 135, 1]], move |now| {
            count.fetch_add(1, Ordering::SeqCst);
            *state.lock().unwrap() = now;
        })
        .expect("the hook installs");
        let other_changes = Arc::new(AtomicUsize::new(0));
        let counted = other_changes.clone();
        let second = start(vec![[KEYBOARD_KEY, 136, 1]], move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
        })
        .expect("a second hook runs beside the first");

        // Stand in for the callback seeing F24 go down: only the hook that watches it changes.
        hook.shared.keyboard(WM_KEYDOWN, 135);
        assert!(*held.lock().unwrap());
        assert_eq!(other_changes.load(Ordering::SeqCst), 0);
        hook.stop();
        assert!(!*held.lock().unwrap(), "stopping released push to talk");
        assert_eq!(changes.load(Ordering::SeqCst), 2);
        second.stop();

        // And it can start again.
        start(Vec::new(), |_| {})
            .expect("a fresh start works")
            .stop();
    }

    #[test]
    fn mouse_hook_installs_only_for_mouse_bindings() {
        use std::time::{Duration, Instant};

        let wait_mouse = |hook: &Hook, expected: bool| {
            let deadline = Instant::now() + Duration::from_secs(1);
            while hook.shared.mouse_installed.load(Ordering::Acquire) != expected
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(hook.shared.mouse_installed.load(Ordering::Acquire), expected);
        };

        assert!(!has_mouse_part(&[]));
        assert!(!has_mouse_part(&[[KEYBOARD_KEY, 162, 1]]));
        assert!(has_mouse_part(&[[MOUSE_BUTTON, 3, 1]]));

        let hook = start(vec![[KEYBOARD_KEY, 135, 1]], |_| {}).expect("keyboard hook installs");
        assert!(
            !hook.shared.want_mouse.load(Ordering::Acquire),
            "keyboard-only binding must not request WH_MOUSE_LL"
        );
        wait_mouse(&hook, false);

        hook.set_shortcut(vec![[MOUSE_BUTTON, 3, 1]]);
        assert!(hook.shared.want_mouse.load(Ordering::Acquire));
        wait_mouse(&hook, true);

        hook.set_shortcut(vec![[KEYBOARD_KEY, 136, 1]]);
        assert!(!hook.shared.want_mouse.load(Ordering::Acquire));
        wait_mouse(&hook, false);

        hook.stop();
    }
}
