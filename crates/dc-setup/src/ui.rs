//! The setup window: one native Win32 window, painted by hand in the app's dark colours. Four pages (location,
//! progress, finished, failed) and no option pages. Type, colours and controls come from `draw`; the dialogs the
//! uninstaller and the retry prompt use are in `dialog`.
#![allow(unsafe_code)]

use crate::draw::{self, ButtonView, CheckView, Face, Fonts, Palette, Scale};
use crate::install::{self, Plan};
use crate::paths::{self, ShotButton, ShotMode, ShotPage};
use crate::payload::Archive;
use crate::{dialog, elide, winsys, wording};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const WM_PROGRESS: u32 = WM_APP + 1;
const WM_FINISHED: u32 = WM_APP + 2;
const TIMER_COPIED: usize = 1;
/// Client size at 96 dpi.
const WIDTH: i32 = 520;
const HEIGHT: i32 = 360;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Welcome,
    Installing,
    Done,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Button {
    Change,
    Desktop,
    Install,
    Launch,
    Close,
    Copy,
    Link,
}

#[derive(Default)]
struct Shared {
    permille: AtomicU32,
    status: Mutex<String>,
    result: Mutex<Option<Result<(), String>>>,
}

struct App {
    hwnd: HWND,
    scale: Scale,
    fonts: Fonts,
    page: Page,
    dir: PathBuf,
    /// The folder already holds an install of ours, so setup updates it.
    updating: bool,
    desktop: bool,
    webview_missing: bool,
    hover: Option<Button>,
    pressed: Option<Button>,
    focus: Option<Button>,
    /// The focus ring shows only after the keyboard has been used.
    keyboard: bool,
    tracking: bool,
    /// After Copy details: whether the clipboard took the text. Cleared by a timer.
    copied: Option<bool>,
    error: String,
    archive: Option<Arc<Archive>>,
    stub: Arc<Vec<u8>>,
    shared: Arc<Shared>,
    version: Option<String>,
}

impl App {
    fn px(&self, value: i32) -> i32 {
        self.scale.px(value)
    }

    fn rect(&self, left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        self.scale.rect(left, top, right, bottom)
    }

    /// The controls of the current page with their rectangles, in tab order.
    fn buttons(&self) -> Vec<(Button, RECT)> {
        let link = self.rect(46, 246, 270, 266);
        let mut list = Vec::new();
        match self.page {
            Page::Welcome => {
                list.push((Button::Change, self.rect(404, 134, 488, 170)));
                list.push((Button::Desktop, self.rect(32, 186, 320, 212)));
                if self.webview_missing {
                    list.push((Button::Link, link));
                }
                list.push((Button::Install, self.rect(368, 298, 488, 336)));
            }
            Page::Installing => {}
            Page::Done => {
                if self.webview_missing {
                    list.push((Button::Link, link));
                }
                list.push((Button::Close, self.rect(232, 298, 320, 336)));
                list.push((Button::Launch, self.rect(328, 298, 488, 336)));
            }
            Page::Failed => {
                list.push((Button::Copy, self.rect(268, 298, 392, 336)));
                list.push((Button::Close, self.rect(400, 298, 488, 336)));
            }
        }
        list
    }

    fn rect_of(&self, id: Button) -> Option<RECT> {
        self.buttons()
            .into_iter()
            .find_map(|(button, rect)| (button == id).then_some(rect))
    }

    /// The button Enter presses on this page.
    fn primary(&self) -> Option<Button> {
        match self.page {
            Page::Welcome => Some(Button::Install),
            Page::Done => Some(Button::Launch),
            Page::Failed => Some(Button::Close),
            Page::Installing => None,
        }
    }

    fn face(&self, id: Button) -> Face {
        Face {
            hover: self.hover == Some(id),
            pressed: self.pressed == Some(id),
            focus: self.keyboard && self.focus == Some(id),
        }
    }

    fn hit(&self, x: i32, y: i32) -> Option<Button> {
        self.buttons().into_iter().find_map(|(button, rect)| {
            (x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom).then_some(button)
        })
    }

    /// Moves the keyboard focus to the next control (or the previous one) in tab order.
    fn tab(&mut self, backwards: bool) {
        let order: Vec<Button> = self
            .buttons()
            .into_iter()
            .map(|(button, _)| button)
            .collect();
        if order.is_empty() {
            return;
        }
        let at = self.focus.and_then(|f| order.iter().position(|b| *b == f));
        let next = match (at, backwards) {
            (None, false) => 0,
            (None, true) => order.len() - 1,
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
        };
        self.focus = Some(order[next]);
        self.keyboard = true;
        self.repaint();
    }

    fn set_page(&mut self, page: Page) {
        self.page = page;
        self.hover = None;
        self.pressed = None;
        self.focus = None;
        self.keyboard = false;
        self.copied = None;
        if self.hwnd.0.is_null() {
            return;
        }
        // The copy cannot be cancelled half way without leaving a broken install, so the caption's close button is
        // greyed while it runs.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_COPIED);
            let menu = GetSystemMenu(self.hwnd, false);
            let state = if page == Page::Installing {
                MF_GRAYED
            } else {
                MF_ENABLED
            };
            let _ = EnableMenuItem(menu, SC_CLOSE, MF_BYCOMMAND | state);
        }
    }

    fn click(&mut self, button: Button) {
        match button {
            Button::Change => {
                if let Some(picked) = winsys::pick_folder(self.hwnd, &self.dir) {
                    match paths::resolve_from_disk(&picked) {
                        Ok(dir) => {
                            self.updating = dir.join(paths::MANIFEST).is_file();
                            self.dir = dir;
                        }
                        Err(message) => {
                            dialog::error(Some(self.hwnd), "Choose another folder", &message);
                        }
                    }
                    self.repaint();
                }
            }
            Button::Install => self.start_install(),
            Button::Desktop => {
                self.desktop = !self.desktop;
                self.repaint();
            }
            Button::Launch => {
                winsys::launch(&self.dir.join("gravecord.exe"), &self.dir);
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
            Button::Close => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            Button::Copy => {
                let version = self
                    .version
                    .as_ref()
                    .map(|v| format!(" {v}"))
                    .unwrap_or_default();
                let details = format!(
                    "gravecord setup{version}\nInstall folder: {}\n\n{}",
                    self.dir.display(),
                    self.error
                );
                self.copied = Some(winsys::copy_text(self.hwnd, &details));
                unsafe {
                    SetTimer(Some(self.hwnd), TIMER_COPIED, 2500, None);
                }
                self.repaint();
            }
            Button::Link => winsys::open_url(winsys::WEBVIEW2_URL),
        }
    }

    fn repaint(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn start_install(&mut self) {
        let Some(archive) = self.archive.clone() else {
            self.fail(
                "This setup file has no program attached. Download gravecord-setup.exe again."
                    .into(),
            );
            return;
        };
        if let Err(message) = paths::check_install_dir(&self.dir) {
            self.fail(message);
            return;
        }
        self.set_page(Page::Installing);
        self.shared.permille.store(0, Ordering::Relaxed);
        *self.shared.status.lock().unwrap() = "Preparing".into();
        self.repaint();

        // Over an install, its key and shortcut names stay; the checkbox decides the desktop shortcut.
        let old = install::read_manifest(&self.dir);
        let desktop = winsys::desktop_dir().ok();
        let mut plan = Plan::keeping(self.dir.clone(), old.as_ref(), desktop.as_deref());
        plan.desktop_shortcut = self.desktop;
        plan.register |= self.desktop;
        let shared = self.shared.clone();
        let stub = self.stub.clone();
        let window = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let hwnd = HWND(window as *mut c_void);
            let result = run_install(&archive, &stub, &plan, &shared, hwnd);
            *shared.result.lock().unwrap() = Some(result.map_err(|error| format!("{error:#}")));
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_FINISHED, WPARAM(0), LPARAM(0));
            }
        });
    }

    fn fail(&mut self, message: String) {
        self.error = message;
        self.set_page(Page::Failed);
        self.repaint();
    }

    fn paint(&self, hdc: HDC) {
        let colors = draw::palette();
        let fonts = &self.fonts;
        unsafe {
            SetBkMode(hdc, TRANSPARENT);
            draw::logo(hdc, &colors, self.px(32), self.px(26), self.px(48));
            draw::text(
                hdc,
                fonts.title,
                colors.text,
                self.rect(92, 20, 488, 54),
                "gravecord",
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            let subtitle = match &self.version {
                Some(version) => format!("Native Discord client  \u{b7}  version {version}"),
                None => "Native Discord client".to_owned(),
            };
            draw::text(
                hdc,
                fonts.caption,
                colors.muted,
                self.rect(92, 54, 488, 74),
                &subtitle,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );

            match self.page {
                Page::Welcome => self.paint_welcome(hdc, &colors),
                Page::Installing => self.paint_progress(hdc, &colors),
                Page::Done => self.paint_done(hdc, &colors),
                Page::Failed => self.paint_failed(hdc, &colors),
            }
        }
    }

    unsafe fn push(&self, hdc: HDC, colors: &Palette, id: Button, label: &str, primary: bool) {
        let Some(rect) = self.rect_of(id) else { return };
        unsafe {
            draw::button(
                hdc,
                &self.fonts,
                colors,
                self.scale,
                &ButtonView {
                    rect,
                    label,
                    primary,
                    face: self.face(id),
                },
            );
        }
    }

    unsafe fn paint_welcome(&self, hdc: HDC, colors: &Palette) {
        let fonts = &self.fonts;
        unsafe {
            draw::text(
                hdc,
                fonts.caption,
                colors.muted,
                self.rect(32, 108, 488, 128),
                "Install location",
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            let field = self.rect(32, 134, 396, 170);
            draw::round(hdc, field, self.px(8), colors.panel);
            // A path too long for the field keeps its drive and its last folders, as Windows path boxes do.
            let room = field.right - field.left - self.px(24);
            let font = fonts.body;
            let shown = elide::elide_path(&self.dir.to_string_lossy(), &|candidate| {
                draw::text_width(hdc, font, candidate) <= room
            });
            draw::text(
                hdc,
                font,
                colors.text,
                RECT {
                    left: field.left + self.px(12),
                    right: field.right - self.px(12),
                    ..field
                },
                &shown,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            self.push(hdc, colors, Button::Change, "Change", false);

            if let Some(rect) = self.rect_of(Button::Desktop) {
                draw::checkbox(
                    hdc,
                    fonts,
                    colors,
                    self.scale,
                    &CheckView {
                        rect,
                        label: "Create a desktop shortcut",
                        on: self.desktop,
                        face: self.face(Button::Desktop),
                    },
                );
            }
            self.paint_webview(hdc, colors);

            let note = if self.updating {
                "gravecord is already installed here. Setup updates it and keeps your settings."
            } else {
                "Installs for your user only. No administrator rights needed."
            };
            draw::wrapped_centered(
                hdc,
                fonts.caption,
                colors.muted,
                self.rect(32, 298, 352, 336),
                note,
            );
            self.push(
                hdc,
                colors,
                Button::Install,
                if self.updating { "Update" } else { "Install" },
                true,
            );
        }
    }

    unsafe fn paint_progress(&self, hdc: HDC, colors: &Palette) {
        let fonts = &self.fonts;
        let permille = self.shared.permille.load(Ordering::Relaxed).min(1000) as i32;
        let status = self.shared.status.lock().unwrap().clone();
        unsafe {
            draw::text(
                hdc,
                fonts.title,
                colors.text,
                self.rect(32, 110, 488, 146),
                if self.updating {
                    "Updating gravecord"
                } else {
                    "Installing gravecord"
                },
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            draw::text(
                hdc,
                fonts.body,
                colors.text,
                self.rect(32, 168, 400, 192),
                wording::step_words(&status),
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            draw::text(
                hdc,
                fonts.body,
                colors.muted,
                self.rect(400, 168, 488, 192),
                &format!("{}%", permille / 10),
                DT_RIGHT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            let bar = self.rect(32, 200, 488, 208);
            let height = bar.bottom - bar.top;
            draw::round(hdc, bar, height / 2, colors.track);
            if permille > 0 {
                let wide = ((bar.right - bar.left) * permille / 1000).max(height);
                draw::round(
                    hdc,
                    RECT {
                        right: bar.left + wide,
                        ..bar
                    },
                    height / 2,
                    colors.brand,
                );
            }
            draw::text(
                hdc,
                fonts.caption,
                colors.muted,
                self.rect(32, 224, 488, 244),
                "Please keep this window open.",
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
        }
    }

    unsafe fn paint_done(&self, hdc: HDC, colors: &Palette) {
        let fonts = &self.fonts;
        unsafe {
            let mark = self.rect(32, 110, 68, 146);
            draw::disc(hdc, mark, colors.success);
            draw::tick(hdc, mark, colors.white, self.scale);
            draw::text(
                hdc,
                fonts.title,
                colors.text,
                self.rect(80, 110, 488, 146),
                if self.updating {
                    "gravecord is updated."
                } else {
                    "gravecord is installed."
                },
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            draw::text(
                hdc,
                fonts.body,
                colors.muted,
                self.rect(80, 150, 488, 174),
                "Start it now, or open it later from the Start menu.",
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            self.paint_webview(hdc, colors);
            self.push(hdc, colors, Button::Close, "Close", false);
            self.push(hdc, colors, Button::Launch, "Launch gravecord", true);
        }
    }

    unsafe fn paint_failed(&self, hdc: HDC, colors: &Palette) {
        let fonts = &self.fonts;
        unsafe {
            let mark = self.rect(32, 110, 68, 146);
            draw::disc(hdc, mark, colors.danger);
            draw::text(
                hdc,
                fonts.title,
                colors.white,
                mark,
                "!",
                DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            draw::text(
                hdc,
                fonts.title,
                colors.text,
                self.rect(80, 110, 488, 146),
                "Setup could not finish",
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            let panel = self.rect(32, 160, 488, 252);
            draw::round(hdc, panel, self.px(8), colors.panel);
            let inner = RECT {
                left: panel.left + self.px(14),
                top: panel.top + self.px(10),
                right: panel.right - self.px(14),
                bottom: panel.bottom - self.px(10),
            };
            // The full text always goes to the clipboard; on screen a very long one drops to the smaller size, and
            // past that ends in an ellipsis.
            let width = inner.right - inner.left;
            let needed = draw::wrapped_height(hdc, fonts.body, width, &self.error);
            let room = inner.bottom - inner.top;
            let (font, ends) = if needed <= room {
                (fonts.body, DRAW_TEXT_FORMAT(0))
            } else if draw::wrapped_height(hdc, fonts.caption, width, &self.error) <= room {
                (fonts.caption, DRAW_TEXT_FORMAT(0))
            } else {
                (fonts.caption, DT_END_ELLIPSIS)
            };
            draw::text(
                hdc,
                font,
                colors.text,
                inner,
                &self.error,
                DT_LEFT | DT_WORDBREAK | DT_NOPREFIX | ends,
            );
            draw::wrapped_centered(
                hdc,
                fonts.caption,
                colors.muted,
                self.rect(32, 258, 488, 292),
                "Close any program that may be using gravecord, then run setup again. If it still fails, copy the details and send them with your report.",
            );
            let label = match self.copied {
                None => "Copy details",
                Some(true) => "Copied",
                Some(false) => "Could not copy",
            };
            self.push(hdc, colors, Button::Copy, label, false);
            self.push(hdc, colors, Button::Close, "Close", true);
        }
    }

    /// The notice about the missing WebView2 Runtime: a panel with the explanation and the link.
    unsafe fn paint_webview(&self, hdc: HDC, colors: &Palette) {
        if !self.webview_missing {
            return;
        }
        let fonts = &self.fonts;
        unsafe {
            draw::round(hdc, self.rect(32, 220, 488, 274), self.px(8), colors.panel);
            draw::text(
                hdc,
                fonts.caption,
                colors.muted,
                self.rect(46, 228, 474, 246),
                "The WebView2 Runtime is not installed. Captcha and activities need it.",
                DT_LEFT | DT_WORDBREAK | DT_NOPREFIX,
            );
            let Some(rect) = self.rect_of(Button::Link) else {
                return;
            };
            let face = self.face(Button::Link);
            let label = "Open the Microsoft download page";
            let shown = RECT {
                right: rect.left + draw::text_width(hdc, fonts.link, label),
                ..rect
            };
            draw::text(
                hdc,
                if face.hover {
                    fonts.link
                } else {
                    fonts.caption
                },
                if face.hover {
                    colors.link_hover
                } else {
                    colors.link
                },
                shown,
                label,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            if face.focus {
                draw::ring(hdc, shown, self.px(4), colors, self.scale);
            }
        }
    }
}

fn run_install(
    archive: &Archive,
    stub: &[u8],
    plan: &Plan,
    shared: &Shared,
    hwnd: HWND,
) -> anyhow::Result<()> {
    close_running(&plan.dir.join("gravecord.exe"), Some(hwnd))?;
    install::install(archive, stub, plan, &mut |permille, label| {
        shared.permille.store(permille, Ordering::Relaxed);
        *shared.status.lock().unwrap() = label.to_owned();
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_PROGRESS, WPARAM(0), LPARAM(0));
        }
    })
}

/// Asks a running gravecord at `exe` to close; if it will not, tells the user and lets them retry or give up.
pub fn close_running(exe: &Path, owner: Option<HWND>) -> anyhow::Result<()> {
    while !winsys::ask_to_close(exe, std::time::Duration::from_secs(8)) {
        if !dialog::ask_retry(owner) {
            anyhow::bail!("gravecord is still running; close it and run setup again.");
        }
    }
    Ok(())
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
        let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if app.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let app = &mut *app;
        let point = |lparam: LPARAM| (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32);
        match message {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                paint_buffered(app, hdc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_PRINTCLIENT => {
                paint_buffered(app, HDC(wparam.0 as *mut c_void));
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEMOVE => {
                let (x, y) = point(lparam);
                if !app.tracking {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    app.tracking = TrackMouseEvent(&mut track).is_ok();
                }
                let hover = app.hit(x, y);
                if hover != app.hover {
                    app.hover = hover;
                    app.repaint();
                }
                LRESULT(0)
            }
            draw::WM_MOUSELEAVE => {
                app.tracking = false;
                if app.hover.take().is_some() {
                    app.repaint();
                }
                LRESULT(0)
            }
            WM_SETCURSOR => {
                if app.hover.is_some() && (lparam.0 & 0xffff) as u32 == HTCLIENT {
                    SetCursor(LoadCursorW(None, IDC_HAND).ok());
                    return LRESULT(1);
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = point(lparam);
                app.pressed = app.hit(x, y);
                if app.pressed.is_some() {
                    SetCapture(hwnd);
                    app.focus = app.pressed;
                    app.keyboard = false;
                }
                app.repaint();
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = point(lparam);
                let pressed = app.pressed.take();
                let _ = ReleaseCapture();
                app.repaint();
                if let (Some(pressed), Some(over)) = (pressed, app.hit(x, y))
                    && pressed == over
                {
                    app.click(over);
                }
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                if app.pressed.take().is_some() {
                    app.repaint();
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let key = wparam.0 as u16;
                let repeat = lparam.0 & (1 << 30) != 0;
                if key == VK_TAB.0 {
                    let back = GetKeyState(VK_SHIFT.0 as i32) < 0;
                    app.tab(back);
                } else if key == VK_RETURN.0 && !repeat {
                    // Enter presses the focused push button; on a checkbox, a link or nothing it presses the primary one.
                    let target = match app.focus {
                        Some(Button::Desktop | Button::Link) | None => app.primary(),
                        other => other,
                    };
                    if let Some(button) = target {
                        app.click(button);
                    }
                } else if key == VK_SPACE.0 && !repeat {
                    if let Some(button) = app.focus
                        && app.buttons().iter().any(|(b, _)| *b == button)
                    {
                        app.click(button);
                    }
                } else if key == VK_ESCAPE.0 && app.page != Page::Installing {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_TIMER => {
                if wparam.0 == TIMER_COPIED {
                    let _ = KillTimer(Some(hwnd), TIMER_COPIED);
                    app.copied = None;
                    app.repaint();
                }
                LRESULT(0)
            }
            WM_PROGRESS => {
                app.repaint();
                LRESULT(0)
            }
            WM_FINISHED => {
                let result = app.shared.result.lock().unwrap().take();
                match result {
                    Some(Ok(())) => {
                        app.set_page(Page::Done);
                        app.repaint();
                    }
                    Some(Err(message)) => app.fail(message),
                    None => {}
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                app.scale = Scale((wparam.0 & 0xffff) as i32);
                app.fonts = Fonts::new(app.scale);
                let suggested = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                app.repaint();
                LRESULT(0)
            }
            WM_CLOSE => {
                // The copy cannot be cancelled half way without leaving a broken install.
                if app.page != Page::Installing {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

unsafe fn paint_buffered(app: &App, target: HDC) {
    unsafe {
        // The client may be a few pixels larger than the layout (frame rounding): the window colour fills all of it.
        let mut client = RECT::default();
        let _ = GetClientRect(app.hwnd, &mut client);
        let (width, height) = (
            client.right.max(app.px(WIDTH)),
            client.bottom.max(app.px(HEIGHT)),
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
        app.paint(memory);
        let _ = BitBlt(target, 0, 0, width, height, Some(memory), 0, 0, SRCCOPY);
        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
    }
}

/// What the screenshot option asks for.
pub struct Shot {
    pub path: PathBuf,
    pub page: ShotPage,
    /// Progress for the progress pages, in thousandths.
    pub progress: Option<u32>,
    pub dpi: Option<u32>,
    pub version: Option<String>,
    pub button: Option<(ShotButton, ShotMode)>,
}

pub struct Start {
    pub dir: PathBuf,
    /// Where the uninstaller dialogs say the settings live.
    pub data_dir: PathBuf,
    pub archive: Option<Archive>,
    pub stub: Vec<u8>,
    pub shot: Option<Shot>,
}

/// The step label the installer would report at this progress.
fn label_at(permille: u32) -> &'static str {
    match permille {
        0..900 => "Copying files",
        900..960 => "Adding shortcuts",
        960..1000 => "Registering the app",
        _ => "Done",
    }
}

/// Paints one of the dialogs instead of a page. Returns the exit code, or `None` for a page.
fn dialog_shot(start: &Start, shot: &Shot) -> Option<i32> {
    let spec = match shot.page {
        ShotPage::Uninstall => dialog::confirm_removal(&start.dir),
        ShotPage::KeepData => dialog::keep_data(&start.data_dir),
        ShotPage::Removed => dialog::removed(),
        ShotPage::Running => dialog::still_running(),
        _ => return None,
    };
    let last = spec.labels.len() - 1;
    let button = shot.button.and_then(|(button, mode)| match button {
        ShotButton::Primary => Some((spec.default, mode)),
        ShotButton::Secondary => Some((if spec.default == 0 { last } else { 0 }, mode)),
        _ => None,
    });
    Some(if dialog::screenshot(&spec, shot.dpi, button, &shot.path) {
        0
    } else {
        1
    })
}

/// Runs the window until it closes. Returns the process exit code.
pub fn run(start: Start) -> i32 {
    winsys::com_init();
    if let Some(shot) = &start.shot
        && let Some(code) = dialog_shot(&start, shot)
    {
        return code;
    }
    let shot = start.shot;
    let carried = start
        .archive
        .as_ref()
        .map(|a| a.version.clone())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
    let version = match shot.as_ref().and_then(|s| s.version.clone()) {
        Some(text) => wording::version_label(&text),
        None => wording::version_label(&carried),
    };
    let screenshot_page = shot.as_ref().map(|s| s.page);
    let updating = match screenshot_page {
        Some(page) => matches!(
            page,
            ShotPage::Update | ShotPage::UpdateProgress | ShotPage::UpdateDone
        ),
        None => start.dir.join(paths::MANIFEST).is_file(),
    };
    let mut app = Box::new(App {
        hwnd: HWND::default(),
        scale: Scale(96),
        fonts: Fonts::new(Scale(96)),
        page: Page::Welcome,
        updating,
        // Over an install the box starts as it chose, so an update keeps its desktop shortcut.
        desktop: screenshot_page.is_none() && updating && {
            let old = install::read_manifest(&start.dir);
            let desktop = winsys::desktop_dir().ok();
            Plan::keeping(start.dir.clone(), old.as_ref(), desktop.as_deref()).desktop_shortcut
        },
        dir: start.dir,
        // A screenshot never depends on what this machine has installed.
        webview_missing: match screenshot_page {
            Some(page) => page == ShotPage::Webview,
            None => !winsys::webview2_present(),
        },
        hover: None,
        pressed: None,
        focus: None,
        keyboard: false,
        tracking: false,
        copied: None,
        error: String::new(),
        version,
        archive: start.archive.map(Arc::new),
        stub: Arc::new(start.stub),
        shared: Arc::new(Shared::default()),
    });
    if let (Some(shot), Some(page)) = (&shot, screenshot_page) {
        match page {
            ShotPage::Progress | ShotPage::UpdateProgress => {
                let permille = shot.progress.unwrap_or(420);
                app.page = Page::Installing;
                app.shared.permille.store(permille, Ordering::Relaxed);
                *app.shared.status.lock().unwrap() = label_at(permille).into();
            }
            ShotPage::Done | ShotPage::UpdateDone => app.page = Page::Done,
            ShotPage::Error => {
                app.page = Page::Failed;
                app.error =
                    "Could not replace C:\\Users\\example\\AppData\\Local\\Programs\\gravecord\\gravecord.exe: Access is denied. (os error 5)"
                        .into();
            }
            _ => {}
        }
    }
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return 1;
        };
        let class = w!("GravecordSetup");
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
            return 1;
        }
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let app_ptr = Box::into_raw(app);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("gravecord setup"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WIDTH,
            HEIGHT,
            None,
            None,
            Some(module.into()),
            Some(app_ptr as *const c_void),
        ) {
            Ok(hwnd) => hwnd,
            Err(_) => return 1,
        };
        let app = &mut *app_ptr;
        app.hwnd = hwnd;
        let real_dpi = GetDpiForWindow(hwnd).max(96) as i32;
        let forced = shot.as_ref().and_then(|s| s.dpi);
        app.scale = Scale(forced.map(|d| d as i32).unwrap_or(real_dpi));
        app.fonts = Fonts::new(app.scale);
        if app.page == Page::Installing {
            app.set_page(Page::Installing);
        }
        winsys::dark_caption(hwnd, draw::palette().window.0, draw::palette().text.0);

        // Size the outer window so the client area is exactly WIDTH x HEIGHT at this scale.
        let (ow, oh) = if forced.is_some() {
            // A forced scale differs from the monitor's, so the frame is measured instead of computed.
            let (mut outer, mut inner) = (RECT::default(), RECT::default());
            let _ = GetWindowRect(hwnd, &mut outer);
            let _ = GetClientRect(hwnd, &mut inner);
            (
                (outer.right - outer.left) - (inner.right - inner.left) + app.px(WIDTH),
                (outer.bottom - outer.top) - (inner.bottom - inner.top) + app.px(HEIGHT),
            )
        } else {
            let mut outer = RECT {
                left: 0,
                top: 0,
                right: app.px(WIDTH),
                bottom: app.px(HEIGHT),
            };
            let _ = AdjustWindowRectExForDpi(
                &mut outer,
                style,
                false,
                WINDOW_EX_STYLE(0),
                app.scale.0 as u32,
            );
            (outer.right - outer.left, outer.bottom - outer.top)
        };
        let x = (GetSystemMetrics(SM_CXSCREEN) - ow) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - oh) / 2;
        let _ = SetWindowPos(hwnd, None, x.max(0), y.max(0), ow, oh, SWP_NOZORDER);

        if let Some((button, mode)) = shot.as_ref().and_then(|s| s.button) {
            let target = match button {
                ShotButton::Change => Button::Change,
                ShotButton::Desktop => Button::Desktop,
                ShotButton::Install | ShotButton::Primary => Button::Install,
                ShotButton::Launch => Button::Launch,
                ShotButton::Close | ShotButton::Secondary => Button::Close,
                ShotButton::Copy => Button::Copy,
                ShotButton::Link => Button::Link,
            };
            match mode {
                ShotMode::Hover => app.hover = Some(target),
                ShotMode::Pressed => {
                    app.hover = Some(target);
                    app.pressed = Some(target);
                }
                ShotMode::Focus => {
                    app.focus = Some(target);
                    app.keyboard = true;
                }
            }
        }
        let _ = ShowWindow(
            hwnd,
            if shot.is_some() {
                SW_SHOWNOACTIVATE
            } else {
                SW_SHOW
            },
        );

        let code = if let Some(shot) = &shot {
            // PrintWindow renders the whole window, caption included, without needing it on a visible desktop.
            let _ = UpdateWindow(hwnd);
            match capture(hwnd, &shot.path) {
                Ok(()) => 0,
                Err(_) => 1,
            }
        } else {
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            0
        };
        let _ = DestroyWindow(hwnd);
        drop(Box::from_raw(app_ptr));
        code
    }
}

/// Saves the whole window, caption included, as a PNG.
pub(crate) unsafe fn capture(hwnd: HWND, path: &Path) -> anyhow::Result<()> {
    unsafe {
        // Let the window receive its first paint and frame messages before it is read back.
        let until = std::time::Instant::now() + std::time::Duration::from_millis(300);
        let mut message = MSG::default();
        while std::time::Instant::now() < until {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_UPDATENOW | RDW_FRAME,
        );
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect)?;
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let old = SelectObject(memory, bitmap.into());
        let printed = PrintWindow(hwnd, memory, PRINT_WINDOW_FLAGS(2)).as_bool();
        let len = (width * height * 4) as usize;
        let mut rgba = std::slice::from_raw_parts(bits as *const u8, len).to_vec();
        for pixel in rgba.chunks_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        anyhow::ensure!(printed, "PrintWindow failed");
        std::fs::write(
            path,
            crate::icon_gen::png(width as u32, height as u32, &rgba),
        )?;
        Ok(())
    }
}
