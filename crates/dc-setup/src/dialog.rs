//! The small dark dialogs that stand in for message boxes: a message, and one or two buttons. They share the setup
//! window's colours, fonts, caption and button looks. If a dialog window cannot be made, the plain message box is used.
#![allow(unsafe_code)]

use crate::draw::{self, ButtonView, Face, Fonts, Scale};
use crate::paths::ShotMode;
use crate::{ui, winsys};
use std::ffi::c_void;
use std::path::Path;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT,
    TrackMouseEvent, VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

/// Client width at 96 dpi.
const WIDTH: i32 = 440;
const MARGIN: i32 = 28;
const BUTTON_HEIGHT: i32 = 38;

/// What a dialog says and offers. Buttons run left to right; the default one is drawn in the brand colour.
pub struct Spec {
    pub title: String,
    pub text: String,
    pub labels: Vec<String>,
    pub default: usize,
    /// The button that Escape and the caption's close button choose.
    pub cancel: usize,
}

/// The uninstaller's first question. Button 1 is Remove.
pub fn confirm_removal(dir: &Path) -> Spec {
    Spec {
        title: "Uninstall gravecord".into(),
        text: format!("Remove gravecord from {}?", dir.display()),
        labels: vec!["Cancel".into(), "Remove".into()],
        default: 1,
        cancel: 0,
    }
}

/// The uninstaller's second question. Button 1 keeps the data, and so does Escape.
pub fn keep_data(data_dir: &Path) -> Spec {
    Spec {
        title: "Uninstall gravecord".into(),
        text: format!(
            "Keep your settings and sign-in data?\n\n{}\n\nIf you keep it, a later install picks up where you left off.",
            data_dir.display()
        ),
        labels: vec!["Delete data".into(), "Keep data".into()],
        default: 1,
        cancel: 1,
    }
}

pub fn removed() -> Spec {
    notice("Uninstall gravecord", "gravecord was removed.")
}

/// Asked while an update waits for the running app. Button 1 is Retry.
pub fn still_running() -> Spec {
    Spec {
        title: "gravecord is running".into(),
        text: "gravecord is still open. Close it, then choose Retry.".into(),
        labels: vec!["Cancel".into(), "Retry".into()],
        default: 1,
        cancel: 0,
    }
}

fn notice(title: &str, text: &str) -> Spec {
    Spec {
        title: title.into(),
        text: text.into(),
        labels: vec!["Close".into()],
        default: 0,
        cancel: 0,
    }
}

/// A message with one Close button.
pub fn info(owner: Option<HWND>, title: &str, text: &str) {
    if show(owner, &notice(title, text)).is_none() {
        winsys::message(owner, title, text, winsys::Kind::Info);
    }
}

/// An error message with one Close button.
pub fn error(owner: Option<HWND>, title: &str, text: &str) {
    if show(owner, &notice(title, text)).is_none() {
        winsys::message(owner, title, text, winsys::Kind::Error);
    }
}

/// Asks to remove. True for Remove.
pub fn ask_removal(owner: Option<HWND>, dir: &Path) -> bool {
    let spec = confirm_removal(dir);
    match show(owner, &spec) {
        Some(choice) => choice == 1,
        None => winsys::message(owner, &spec.title, &spec.text, winsys::Kind::AskYes),
    }
}

/// Asks whether to keep the settings and sign-in data. True keeps it.
pub fn ask_keep_data(owner: Option<HWND>, data_dir: &Path) -> bool {
    let spec = keep_data(data_dir);
    match show(owner, &spec) {
        Some(choice) => choice == 1,
        None => winsys::message(owner, &spec.title, &spec.text, winsys::Kind::AskYes),
    }
}

/// Asks to retry while the app is still open. True retries.
pub fn ask_retry(owner: Option<HWND>) -> bool {
    let spec = still_running();
    match show(owner, &spec) {
        Some(choice) => choice == 1,
        None => winsys::message(owner, &spec.title, &spec.text, winsys::Kind::AskRetry),
    }
}

struct State {
    hwnd: HWND,
    owner: Option<HWND>,
    scale: Scale,
    fonts: Fonts,
    text: String,
    labels: Vec<String>,
    default: usize,
    cancel: usize,
    /// Button rectangles in client pixels.
    rects: Vec<RECT>,
    text_rect: RECT,
    client: (i32, i32),
    hover: Option<usize>,
    pressed: Option<usize>,
    focus: Option<usize>,
    keyboard: bool,
    tracking: bool,
    result: Option<usize>,
}

impl State {
    fn face(&self, index: usize) -> Face {
        Face {
            hover: self.hover == Some(index),
            pressed: self.pressed == Some(index),
            focus: self.keyboard && self.focus == Some(index),
        }
    }

    fn hit(&self, x: i32, y: i32) -> Option<usize> {
        self.rects
            .iter()
            .position(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
    }

    fn repaint(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn finish(&mut self, choice: usize) {
        if self.result.is_some() {
            return;
        }
        self.result = Some(choice);
        unsafe {
            if let Some(owner) = self.owner {
                let _ = EnableWindow(owner, true);
            }
            let _ = DestroyWindow(self.hwnd);
        }
    }

    fn paint(&self, hdc: HDC) {
        let colors = draw::palette();
        unsafe {
            draw::fill(
                hdc,
                RECT {
                    left: 0,
                    top: 0,
                    right: self.client.0,
                    bottom: self.client.1,
                },
                colors.window,
            );
            SetBkMode(hdc, TRANSPARENT);
            draw::text(
                hdc,
                self.fonts.body,
                colors.text,
                self.text_rect,
                &self.text,
                DT_LEFT | DT_WORDBREAK | DT_NOPREFIX,
            );
            for (index, label) in self.labels.iter().enumerate() {
                draw::button(
                    hdc,
                    &self.fonts,
                    &colors,
                    self.scale,
                    &ButtonView {
                        rect: self.rects[index],
                        label,
                        primary: index == self.default,
                        face: self.face(index),
                    },
                );
            }
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if state.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let state = &mut *state;
        let point = |lparam: LPARAM| (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32);
        match message {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                paint_buffered(state, hdc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_PRINTCLIENT => {
                paint_buffered(state, HDC(wparam.0 as *mut c_void));
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEMOVE => {
                let (x, y) = point(lparam);
                if !state.tracking {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    state.tracking = TrackMouseEvent(&mut track).is_ok();
                }
                let hover = state.hit(x, y);
                if hover != state.hover {
                    state.hover = hover;
                    state.repaint();
                }
                LRESULT(0)
            }
            draw::WM_MOUSELEAVE => {
                state.tracking = false;
                if state.hover.take().is_some() {
                    state.repaint();
                }
                LRESULT(0)
            }
            WM_SETCURSOR => {
                if state.hover.is_some() && (lparam.0 & 0xffff) as u32 == HTCLIENT {
                    SetCursor(LoadCursorW(None, IDC_HAND).ok());
                    return LRESULT(1);
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = point(lparam);
                state.pressed = state.hit(x, y);
                if state.pressed.is_some() {
                    SetCapture(hwnd);
                    state.keyboard = false;
                    state.focus = state.pressed;
                }
                state.repaint();
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = point(lparam);
                let pressed = state.pressed.take();
                let _ = ReleaseCapture();
                state.repaint();
                if let (Some(pressed), Some(over)) = (pressed, state.hit(x, y))
                    && pressed == over
                {
                    state.finish(over);
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let key = wparam.0 as u16;
                let repeat = lparam.0 & (1 << 30) != 0;
                if key == VK_TAB.0 {
                    let count = state.labels.len();
                    let back = GetKeyState(VK_SHIFT.0 as i32) < 0;
                    state.focus = Some(match (state.focus, back) {
                        (None, false) => 0,
                        (None, true) => count - 1,
                        (Some(i), false) => (i + 1) % count,
                        (Some(i), true) => (i + count - 1) % count,
                    });
                    state.keyboard = true;
                    state.repaint();
                } else if key == VK_RETURN.0 && !repeat {
                    let choice = state.focus.unwrap_or(state.default);
                    state.finish(choice);
                } else if key == VK_SPACE.0 && !repeat {
                    if let Some(choice) = state.focus {
                        state.finish(choice);
                    }
                } else if key == VK_ESCAPE.0 {
                    let cancel = state.cancel;
                    state.finish(cancel);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let cancel = state.cancel;
                state.finish(cancel);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

unsafe fn paint_buffered(state: &State, target: HDC) {
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(state.hwnd, &mut client);
        let (width, height) = (
            client.right.max(state.client.0),
            client.bottom.max(state.client.1),
        );
        let memory = CreateCompatibleDC(Some(target));
        let bitmap = CreateCompatibleBitmap(target, width, height);
        let old = SelectObject(memory, bitmap.into());
        draw::fill(
            memory,
            RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            },
            draw::palette().window,
        );
        state.paint(memory);
        let _ = BitBlt(target, 0, 0, width, height, Some(memory), 0, 0, SRCCOPY);
        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
    }
}

/// How the window is made: either to run, or to be painted into a file.
struct Made {
    hwnd: HWND,
    state: *mut State,
}

/// Creates the (hidden) dialog window and lays it out. `dpi` forces a scale; the owner's is used otherwise.
unsafe fn make(owner: Option<HWND>, spec: &Spec, dpi: Option<u32>) -> Option<Made> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let class = w!("GravecordSetupDialog");
        let mut info = WNDCLASSW::default();
        if GetClassInfoW(Some(module.into()), class, &mut info).is_err() {
            let icon = LoadImageW(
                Some(module.into()),
                PCWSTR(std::ptr::without_provenance(1)),
                IMAGE_ICON,
                0,
                0,
                LR_DEFAULTSIZE,
            )
            .ok();
            let class_info = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hIcon: HICON(icon.map(|handle| handle.0).unwrap_or_default()),
                ..Default::default()
            };
            if RegisterClassW(&class_info) == 0 {
                return None;
            }
        }
        let state = Box::into_raw(Box::new(State {
            hwnd: HWND::default(),
            owner,
            scale: Scale(96),
            fonts: Fonts::new(Scale(96)),
            text: spec.text.clone(),
            labels: spec.labels.clone(),
            default: spec.default.min(spec.labels.len() - 1),
            cancel: spec.cancel.min(spec.labels.len() - 1),
            rects: Vec::new(),
            text_rect: RECT::default(),
            client: (0, 0),
            hover: None,
            pressed: None,
            focus: None,
            keyboard: false,
            tracking: false,
            result: None,
        }));
        let style = WS_POPUP | WS_CAPTION | WS_SYSMENU;
        let title: Vec<u16> = spec.title.encode_utf16().chain([0]).collect();
        let created = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR(title.as_ptr()),
            style,
            0,
            0,
            100,
            100,
            owner,
            None,
            Some(module.into()),
            Some(state as *const c_void),
        );
        let Ok(hwnd) = created else {
            drop(Box::from_raw(state));
            return None;
        };
        let state_ref = &mut *state;
        state_ref.hwnd = hwnd;
        let dpi = dpi.unwrap_or_else(|| GetDpiForWindow(hwnd).max(96));
        let scale = Scale(dpi as i32);
        state_ref.scale = scale;
        state_ref.fonts = Fonts::new(scale);

        // Measure the message at this scale, then place the buttons below it, right aligned.
        let screen = GetDC(None);
        let text_width = scale.px(WIDTH - 2 * MARGIN);
        let text_height =
            draw::wrapped_height(screen, state_ref.fonts.body, text_width, &state_ref.text);
        let mut widths = Vec::new();
        for label in &state_ref.labels {
            let wanted = draw::text_width(screen, state_ref.fonts.strong, label) + scale.px(40);
            widths.push(wanted.max(scale.px(88)));
        }
        ReleaseDC(None, screen);
        let top = scale.px(24);
        let buttons_top = top + text_height + scale.px(24);
        let height = scale.px(BUTTON_HEIGHT);
        let mut right = scale.px(WIDTH - 24);
        let mut rects = vec![RECT::default(); widths.len()];
        for index in (0..widths.len()).rev() {
            rects[index] = RECT {
                left: right - widths[index],
                top: buttons_top,
                right,
                bottom: buttons_top + height,
            };
            right -= widths[index] + scale.px(8);
        }
        state_ref.rects = rects;
        state_ref.text_rect = RECT {
            left: scale.px(MARGIN),
            top,
            right: scale.px(MARGIN) + text_width,
            bottom: top + text_height,
        };
        state_ref.client = (scale.px(WIDTH), buttons_top + height + scale.px(24));

        // The outer size comes from the frame the system gave this window, so the client is exactly ours.
        let (mut outer, mut inner) = (RECT::default(), RECT::default());
        let _ = GetWindowRect(hwnd, &mut outer);
        let _ = GetClientRect(hwnd, &mut inner);
        let frame_w = (outer.right - outer.left) - (inner.right - inner.left);
        let frame_h = (outer.bottom - outer.top) - (inner.bottom - inner.top);
        let mut wanted = RECT {
            left: 0,
            top: 0,
            right: state_ref.client.0,
            bottom: state_ref.client.1,
        };
        let _ = AdjustWindowRectExForDpi(
            &mut wanted,
            style,
            false,
            WINDOW_EX_STYLE(0),
            GetDpiForWindow(hwnd).max(96),
        );
        let (ow, oh) = (
            (wanted.right - wanted.left).max(state_ref.client.0 + frame_w),
            (wanted.bottom - wanted.top).max(state_ref.client.1 + frame_h),
        );
        let area = match owner {
            Some(owner) => {
                let mut rect = RECT::default();
                let _ = GetWindowRect(owner, &mut rect);
                rect
            }
            None => RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN),
                bottom: GetSystemMetrics(SM_CYSCREEN),
            },
        };
        let x = area.left + (area.right - area.left - ow) / 2;
        let y = area.top + (area.bottom - area.top - oh) / 2;
        let _ = SetWindowPos(hwnd, None, x.max(0), y.max(0), ow, oh, SWP_NOZORDER);
        winsys::dark_caption(hwnd, draw::palette().window.0, draw::palette().text.0);
        Some(Made { hwnd, state })
    }
}

/// Shows the dialog and waits for a choice. `None` when the window could not be made.
pub fn show(owner: Option<HWND>, spec: &Spec) -> Option<usize> {
    unsafe {
        let made = make(owner, spec, None)?;
        if let Some(owner) = owner {
            let _ = EnableWindow(owner, false);
        }
        let _ = ShowWindow(made.hwnd, SW_SHOW);
        let _ = SetForegroundWindow(made.hwnd);
        let mut message = MSG::default();
        while (*made.state).result.is_none() && GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let choice = (*made.state).result.unwrap_or((*made.state).cancel);
        if let Some(owner) = owner {
            let _ = EnableWindow(owner, true);
        }
        let _ = DestroyWindow(made.hwnd);
        drop(Box::from_raw(made.state));
        Some(choice)
    }
}

/// Paints the dialog into a PNG without waiting for an answer. `button` is the index to put in a state.
pub fn screenshot(
    spec: &Spec,
    dpi: Option<u32>,
    button: Option<(usize, ShotMode)>,
    path: &Path,
) -> bool {
    unsafe {
        let Some(made) = make(None, spec, dpi) else {
            return false;
        };
        let state = &mut *made.state;
        if let Some((index, mode)) = button
            && index < state.labels.len()
        {
            match mode {
                ShotMode::Hover => state.hover = Some(index),
                ShotMode::Pressed => {
                    state.hover = Some(index);
                    state.pressed = Some(index);
                }
                ShotMode::Focus => {
                    state.focus = Some(index);
                    state.keyboard = true;
                }
            }
        }
        let _ = ShowWindow(made.hwnd, SW_SHOWNOACTIVATE);
        let _ = UpdateWindow(made.hwnd);
        let ok = ui::capture(made.hwnd, path).is_ok();
        let _ = DestroyWindow(made.hwnd);
        drop(Box::from_raw(made.state));
        ok
    }
}
