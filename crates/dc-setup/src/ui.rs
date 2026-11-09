//! The setup window: one native Win32 window, painted by hand in the app's dark Discord colours. Four pages (location,
//! progress, finished, failed) and no option pages.
#![allow(unsafe_code)]

use crate::install::{self, Plan};
use crate::payload::Archive;
use crate::{icon_gen, paths, winsys};
use dc_tokens::Theme;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_RETURN, VK_SPACE};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const WM_PROGRESS: u32 = WM_APP + 1;
const WM_FINISHED: u32 = WM_APP + 2;
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
    Install,
    Finish,
    Close,
    Desktop,
    Launch,
    WebView,
}

#[derive(Default)]
struct Shared {
    permille: AtomicU32,
    status: Mutex<String>,
    result: Mutex<Option<Result<(), String>>>,
}

struct Fonts {
    title: HFONT,
    heading: HFONT,
    body: HFONT,
    small: HFONT,
}

struct App {
    hwnd: HWND,
    dpi: i32,
    fonts: Option<Fonts>,
    page: Page,
    dir: PathBuf,
    desktop: bool,
    launch: bool,
    webview_missing: bool,
    hover: Option<Button>,
    error: String,
    archive: Option<Arc<Archive>>,
    stub: Arc<Vec<u8>>,
    shared: Arc<Shared>,
    version: String,
}

struct Palette {
    window: COLORREF,
    panel: COLORREF,
    track: COLORREF,
    text: COLORREF,
    muted: COLORREF,
    brand: COLORREF,
    brand_hover: COLORREF,
    secondary: COLORREF,
    secondary_hover: COLORREF,
    danger: COLORREF,
    white: COLORREF,
}

fn token(name: &str) -> (u8, u8, u8) {
    let value = Theme::Dark.color(name);
    ((value >> 24) as u8, (value >> 16) as u8, (value >> 8) as u8)
}

fn rgb((r, g, b): (u8, u8, u8)) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn lighten((r, g, b): (u8, u8, u8), amount: u32) -> (u8, u8, u8) {
    let up = |c: u8| (c as u32 + (255 - c as u32) * amount / 100) as u8;
    (up(r), up(g), up(b))
}

fn palette() -> Palette {
    let brand = token("--background-brand");
    let secondary = token("--background-surface-highest");
    Palette {
        window: rgb(token("--background-base-lower")),
        panel: rgb(token("--background-base-lowest")),
        track: rgb(token("--background-surface-highest")),
        text: rgb(token("--text-default")),
        muted: rgb(token("--text-muted")),
        brand: rgb(brand),
        brand_hover: rgb(lighten(brand, 12)),
        secondary: rgb(secondary),
        secondary_hover: rgb(lighten(secondary, 10)),
        danger: rgb(token("--status-danger")),
        white: rgb((255, 255, 255)),
    }
}

impl App {
    fn px(&self, value: i32) -> i32 {
        value * self.dpi / 96
    }

    fn rect(&self, left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left: self.px(left),
            top: self.px(top),
            right: self.px(right),
            bottom: self.px(bottom),
        }
    }

    fn make_fonts(&mut self) {
        if let Some(old) = self.fonts.take() {
            unsafe {
                for font in [old.title, old.heading, old.body, old.small] {
                    let _ = DeleteObject(font.into());
                }
            }
        }
        let make = |height: i32, weight: i32| unsafe {
            CreateFontW(
                -(height * self.dpi / 96),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                w!("Segoe UI"),
            )
        };
        self.fonts = Some(Fonts {
            title: make(28, 600),
            heading: make(20, 600),
            body: make(14, 400),
            small: make(12, 400),
        });
    }

    /// The buttons of the current page with their rectangles, in logical pixels.
    fn buttons(&self) -> Vec<(Button, RECT)> {
        let mut list = Vec::new();
        match self.page {
            Page::Welcome => {
                list.push((Button::Change, self.rect(404, 142, 488, 178)));
                list.push((Button::Desktop, self.rect(32, 196, 400, 224)));
                list.push((Button::Install, self.rect(368, 298, 488, 336)));
                if self.webview_missing {
                    list.push((Button::WebView, self.rect(32, 270, 250, 290)));
                }
            }
            Page::Installing => {}
            Page::Done => {
                list.push((Button::Launch, self.rect(32, 196, 400, 224)));
                list.push((Button::Finish, self.rect(368, 298, 488, 336)));
                if self.webview_missing {
                    list.push((Button::WebView, self.rect(32, 270, 250, 290)));
                }
            }
            Page::Failed => list.push((Button::Close, self.rect(368, 298, 488, 336))),
        }
        list
    }

    fn hit(&self, x: i32, y: i32) -> Option<Button> {
        self.buttons().into_iter().find_map(|(button, rect)| {
            (x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom).then_some(button)
        })
    }

    fn click(&mut self, button: Button) {
        match button {
            Button::Change => {
                if let Some(picked) = winsys::pick_folder(self.hwnd, &self.dir) {
                    let busy = std::fs::read_dir(&picked)
                        .is_ok_and(|mut entries| entries.next().is_some());
                    let ours = install::read_manifest(&picked).is_some();
                    self.dir = paths::resolve_install_dir(&picked, busy, ours);
                    self.repaint();
                }
            }
            Button::Install => self.start_install(),
            Button::Desktop => {
                self.desktop = !self.desktop;
                self.repaint();
            }
            Button::Launch => {
                self.launch = !self.launch;
                self.repaint();
            }
            Button::Finish => {
                if self.launch {
                    winsys::launch(&self.dir.join("gravecord.exe"), &self.dir);
                }
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
            Button::Close => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            Button::WebView => winsys::open_url(winsys::WEBVIEW2_URL),
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
        self.page = Page::Installing;
        self.shared.permille.store(0, Ordering::Relaxed);
        *self.shared.status.lock().unwrap() = "Preparing".into();
        self.repaint();

        let mut plan = Plan::new(self.dir.clone());
        plan.desktop_shortcut = self.desktop;
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
        self.page = Page::Failed;
        self.repaint();
    }

    fn paint(&self, hdc: HDC) {
        let Some(fonts) = &self.fonts else { return };
        let colors = palette();
        let (width, height) = (self.px(WIDTH), self.px(HEIGHT));
        unsafe {
            fill(
                hdc,
                RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
                colors.window,
            );
            SetBkMode(hdc, TRANSPARENT);

            self.draw_logo(hdc, &colors);
            text(
                hdc,
                fonts.title,
                colors.text,
                self.rect(104, 26, 488, 62),
                "gravecord",
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            );
            let subtitle = if self.version.is_empty() {
                "Native Discord client".to_owned()
            } else {
                format!("Native Discord client  \u{b7}  version {}", self.version)
            };
            text(
                hdc,
                fonts.small,
                colors.muted,
                self.rect(104, 62, 488, 84),
                &subtitle,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            );

            match self.page {
                Page::Welcome => self.paint_welcome(hdc, fonts, &colors),
                Page::Installing => self.paint_progress(hdc, fonts, &colors),
                Page::Done => {
                    text(
                        hdc,
                        fonts.heading,
                        colors.text,
                        self.rect(32, 124, 488, 160),
                        "gravecord is installed.",
                        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
                    );
                    self.check(
                        hdc,
                        fonts,
                        &colors,
                        Button::Launch,
                        "Launch gravecord",
                        self.launch,
                    );
                    self.paint_webview(hdc, fonts, &colors);
                    self.button(hdc, fonts, &colors, Button::Finish, "Finish", true);
                }
                Page::Failed => {
                    text(
                        hdc,
                        fonts.heading,
                        colors.danger,
                        self.rect(32, 124, 488, 160),
                        "Setup could not finish",
                        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
                    );
                    text(
                        hdc,
                        fonts.body,
                        colors.text,
                        self.rect(32, 168, 488, 280),
                        &self.error,
                        DT_LEFT | DT_WORDBREAK,
                    );
                    self.button(hdc, fonts, &colors, Button::Close, "Close", true);
                }
            }
        }
    }

    unsafe fn paint_welcome(&self, hdc: HDC, fonts: &Fonts, colors: &Palette) {
        unsafe {
            text(
                hdc,
                fonts.small,
                colors.muted,
                self.rect(32, 118, 488, 138),
                "Install location",
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            );
            round(hdc, self.rect(32, 142, 396, 178), 8, colors.panel);
            let mut path = self.rect(44, 142, 386, 178);
            let mut wide: Vec<u16> = self.dir.to_string_lossy().encode_utf16().collect();
            SelectObject(hdc, fonts.body.into());
            SetTextColor(hdc, colors.text);
            DrawTextW(
                hdc,
                &mut wide,
                &mut path,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_PATH_ELLIPSIS,
            );
            self.button(hdc, fonts, colors, Button::Change, "Change", false);
            self.check(
                hdc,
                fonts,
                colors,
                Button::Desktop,
                "Create a desktop shortcut",
                self.desktop,
            );
            self.paint_webview(hdc, fonts, colors);
            text(
                hdc,
                fonts.small,
                colors.muted,
                self.rect(32, 310, 350, 330),
                "Installs for your user only. No administrator rights needed.",
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            );
            self.button(hdc, fonts, colors, Button::Install, "Install", true);
        }
    }

    unsafe fn paint_progress(&self, hdc: HDC, fonts: &Fonts, colors: &Palette) {
        let permille = self.shared.permille.load(Ordering::Relaxed).min(1000) as i32;
        let status = self.shared.status.lock().unwrap().clone();
        unsafe {
            text(
                hdc,
                fonts.body,
                colors.text,
                self.rect(32, 140, 400, 166),
                &status,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            );
            text(
                hdc,
                fonts.body,
                colors.muted,
                self.rect(400, 140, 488, 166),
                &format!("{}%", permille / 10),
                DT_RIGHT | DT_SINGLELINE | DT_VCENTER,
            );
            let bar = self.rect(32, 176, 488, 186);
            round(hdc, bar, self.px(10), colors.track);
            let fill_to = bar.left + (bar.right - bar.left) * permille / 1000;
            if fill_to - bar.left > self.px(10) {
                round(
                    hdc,
                    RECT {
                        right: fill_to,
                        ..bar
                    },
                    self.px(10),
                    colors.brand,
                );
            }
        }
    }

    unsafe fn paint_webview(&self, hdc: HDC, fonts: &Fonts, colors: &Palette) {
        if !self.webview_missing {
            return;
        }
        unsafe {
            text(
                hdc,
                fonts.small,
                colors.muted,
                self.rect(32, 240, 488, 270),
                "The WebView2 Runtime is not installed. Captcha and activities need it; the rest works without.",
                DT_LEFT | DT_WORDBREAK,
            );
            let hot = self.hover == Some(Button::WebView);
            text(
                hdc,
                fonts.small,
                if hot {
                    colors.white
                } else {
                    rgb(token("--text-link"))
                },
                self.rect(32, 270, 250, 290),
                "Open the Microsoft download page",
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            );
        }
    }

    unsafe fn button(
        &self,
        hdc: HDC,
        fonts: &Fonts,
        colors: &Palette,
        id: Button,
        label: &str,
        primary: bool,
    ) {
        let Some((_, rect)) = self.buttons().into_iter().find(|(button, _)| *button == id) else {
            return;
        };
        let hot = self.hover == Some(id);
        let fill = match (primary, hot) {
            (true, false) => colors.brand,
            (true, true) => colors.brand_hover,
            (false, false) => colors.secondary,
            (false, true) => colors.secondary_hover,
        };
        unsafe {
            round(hdc, rect, self.px(8), fill);
            text(
                hdc,
                fonts.body,
                colors.white,
                rect,
                label,
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            );
        }
    }

    unsafe fn check(
        &self,
        hdc: HDC,
        fonts: &Fonts,
        colors: &Palette,
        id: Button,
        label: &str,
        on: bool,
    ) {
        let Some((_, rect)) = self.buttons().into_iter().find(|(button, _)| *button == id) else {
            return;
        };
        let size = self.px(20);
        let top = rect.top + (rect.bottom - rect.top - size) / 2;
        let boxed = RECT {
            left: rect.left,
            top,
            right: rect.left + size,
            bottom: top + size,
        };
        unsafe {
            if on {
                round(hdc, boxed, self.px(5), colors.brand);
                let pen = CreatePen(PS_SOLID, self.px(2).max(2), colors.white);
                let old = SelectObject(hdc, pen.into());
                let (x, y, s) = (boxed.left, boxed.top, size);
                let _ = MoveToEx(hdc, x + s * 22 / 100, y + s * 52 / 100, None);
                let _ = LineTo(hdc, x + s * 42 / 100, y + s * 70 / 100);
                let _ = LineTo(hdc, x + s * 78 / 100, y + s * 30 / 100);
                SelectObject(hdc, old);
                let _ = DeleteObject(pen.into());
            } else {
                round(hdc, boxed, self.px(5), colors.muted);
                let inner = RECT {
                    left: boxed.left + 2,
                    top: boxed.top + 2,
                    right: boxed.right - 2,
                    bottom: boxed.bottom - 2,
                };
                round(hdc, inner, self.px(4), colors.window);
            }
            let label_rect = RECT {
                left: boxed.right + self.px(10),
                ..rect
            };
            text(
                hdc,
                fonts.body,
                colors.text,
                label_rect,
                label,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            );
        }
    }

    /// The logo is the executable's icon art, composited on the window colour so it needs no alpha blit.
    unsafe fn draw_logo(&self, hdc: HDC, colors: &Palette) {
        let size = self.px(56);
        let pixels = icon_gen::render_rgba(size as u32);
        let background = [
            colors.window.0 & 0xff,
            (colors.window.0 >> 8) & 0xff,
            (colors.window.0 >> 16) & 0xff,
        ];
        let mut bgra = Vec::with_capacity(pixels.len());
        for pixel in pixels.chunks(4) {
            let alpha = pixel[3] as u32;
            let mixed = |channel: usize, base: u32| {
                ((pixel[channel] as u32 * alpha + base * (255 - alpha)) / 255) as u8
            };
            bgra.extend([
                mixed(2, background[2]),
                mixed(1, background[1]),
                mixed(0, background[0]),
                255,
            ]);
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            SetDIBitsToDevice(
                hdc,
                self.px(32),
                self.px(26),
                size as u32,
                size as u32,
                0,
                0,
                0,
                size as u32,
                bgra.as_ptr().cast(),
                &info,
                DIB_RGB_COLORS,
            );
        }
    }
}

unsafe fn fill(hdc: HDC, rect: RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(hdc, &rect, brush);
        let _ = DeleteObject(brush.into());
    }
}

unsafe fn round(hdc: HDC, rect: RECT, radius: i32, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(hdc, brush.into());
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let _ = RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right + 1,
            rect.bottom + 1,
            radius,
            radius,
        );
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(brush.into());
    }
}

unsafe fn text(
    hdc: HDC,
    font: HFONT,
    color: COLORREF,
    mut rect: RECT,
    value: &str,
    format: DRAW_TEXT_FORMAT,
) {
    let mut wide: Vec<u16> = value.encode_utf16().collect();
    unsafe {
        SelectObject(hdc, font.into());
        SetTextColor(hdc, color);
        DrawTextW(hdc, &mut wide, &mut rect, format);
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
pub fn close_running(exe: &std::path::Path, owner: Option<HWND>) -> anyhow::Result<()> {
    while !winsys::ask_to_close(exe, std::time::Duration::from_secs(8)) {
        let retry = winsys::message(
            owner,
            "gravecord is running",
            "gravecord is still open. Close it, then choose Retry.",
            winsys::Kind::AskRetry,
        );
        if !retry {
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
                let (x, y) = (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32);
                let hover = app.hit(x, y);
                if hover != app.hover {
                    app.hover = hover;
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
            WM_LBUTTONUP => {
                let (x, y) = (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32);
                if let Some(button) = app.hit(x, y) {
                    app.click(button);
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let key = wparam.0 as u16;
                if key == VK_RETURN.0 {
                    match app.page {
                        Page::Welcome => app.click(Button::Install),
                        Page::Done => app.click(Button::Finish),
                        Page::Failed => app.click(Button::Close),
                        Page::Installing => {}
                    }
                } else if key == VK_ESCAPE.0 && app.page != Page::Installing {
                    let _ = DestroyWindow(hwnd);
                } else if key == VK_SPACE.0 {
                    match app.page {
                        Page::Welcome => app.click(Button::Desktop),
                        Page::Done => app.click(Button::Launch),
                        _ => {}
                    }
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
                        app.page = Page::Done;
                        app.repaint();
                    }
                    Some(Err(message)) => app.fail(message),
                    None => {}
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                app.dpi = (wparam.0 & 0xffff) as i32;
                app.make_fonts();
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
        fill(
            memory,
            RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            },
            palette().window,
        );
        app.paint(memory);
        let _ = BitBlt(target, 0, 0, width, height, Some(memory), 0, 0, SRCCOPY);
        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
    }
}

pub struct Start {
    pub dir: PathBuf,
    pub archive: Option<Archive>,
    pub stub: Vec<u8>,
    pub screenshot: Option<PathBuf>,
    pub state: Option<String>,
}

/// Runs the window until it closes. Returns the process exit code.
pub fn run(start: Start) -> i32 {
    winsys::com_init();
    let mut app = Box::new(App {
        hwnd: HWND::default(),
        dpi: 96,
        fonts: None,
        page: Page::Welcome,
        dir: start.dir,
        desktop: false,
        launch: true,
        webview_missing: !winsys::webview2_present(),
        hover: None,
        error: String::new(),
        version: start
            .archive
            .as_ref()
            .map(|a| a.version.clone())
            .unwrap_or_default(),
        archive: start.archive.map(Arc::new),
        stub: Arc::new(start.stub),
        shared: Arc::new(Shared::default()),
    });
    if let Some(state) = start.state.as_deref() {
        match state {
            "progress" => {
                app.page = Page::Installing;
                app.shared.permille.store(420, Ordering::Relaxed);
                *app.shared.status.lock().unwrap() = "Copying files".into();
            }
            "done" => app.page = Page::Done,
            "failed" => {
                app.page = Page::Failed;
                app.error =
                    "Could not replace C:\\example\\gravecord.exe: Access is denied.".into();
            }
            "welcome-webview" => app.webview_missing = true,
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
        app.dpi = GetDpiForWindow(hwnd).max(96) as i32;
        app.make_fonts();
        // Size the outer window so the client area is exactly WIDTH x HEIGHT at this dpi.
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: app.px(WIDTH),
            bottom: app.px(HEIGHT),
        };
        let _ =
            AdjustWindowRectExForDpi(&mut outer, style, false, WINDOW_EX_STYLE(0), app.dpi as u32);
        let (ow, oh) = (outer.right - outer.left, outer.bottom - outer.top);
        let x = (GetSystemMetrics(SM_CXSCREEN) - ow) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - oh) / 2;
        let _ = SetWindowPos(hwnd, None, x.max(0), y.max(0), ow, oh, SWP_NOZORDER);
        let _ = ShowWindow(
            hwnd,
            if start.screenshot.is_some() {
                SW_SHOWNOACTIVATE
            } else {
                SW_SHOW
            },
        );

        let code = if let Some(path) = &start.screenshot {
            // PrintWindow renders the whole window, caption included, without needing it on a visible desktop.
            let _ = UpdateWindow(hwnd);
            match capture(hwnd, path) {
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

unsafe fn capture(hwnd: HWND, path: &std::path::Path) -> anyhow::Result<()> {
    unsafe {
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
        std::fs::write(path, icon_gen::png(width as u32, height as u32, &rgba))?;
        Ok(())
    }
}
