//! Finds displays and top-level windows with the Win32 calls, and a window's icon. Nothing here captures anything.
//! Display names and refresh rates follow what Serein reads through `windows-capture`'s `Monitor` (MIT OR Apache-2.0),
//! done here with the underlying GDI calls.
#![allow(unsafe_code)]

use super::sources::{DisplayInfo, WindowInfo, assemble};
use super::{Source, SourceId};
use crate::{Error, Layout, Picture, Result};
use std::collections::HashMap;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        Graphics::{
            Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Gdi::*,
        },
        System::Threading::GetCurrentProcessId,
        UI::WindowsAndMessaging::*,
    },
    core::{BOOL, PCWSTR},
};

pub(super) fn sources() -> Result<Vec<Source>> {
    let displays = displays()?;
    let refresh: HashMap<u64, u32> = displays.iter().map(|d| (d.handle, d.refresh_hz)).collect();
    let windows = windows(&refresh)?;
    // SAFETY: no arguments.
    let own = unsafe { GetCurrentProcessId() };
    Ok(assemble(displays, windows, own, |hwnd| {
        window_icon(HWND(hwnd as usize as _))
    }))
}

/// Position-independent handle of a monitor, as stored in [`SourceId::Display`].
pub(super) fn handle_value(handle: HMONITOR) -> u64 {
    handle.0 as usize as u64
}

fn displays() -> Result<Vec<DisplayInfo>> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` is the address of the Vec passed below, alive for the whole enumeration.
        let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        list.push(monitor);
        BOOL(1)
    }
    let mut monitors: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only pushes into the Vec whose address is passed.
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut monitors as *mut _ as isize),
        )
    };
    if !ok.as_bool() {
        return Err(Error::unavailable("displays could not be enumerated"));
    }
    let mut out = Vec::new();
    let single = monitors.len() == 1;
    for (index, monitor) in monitors.into_iter().enumerate() {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: `info` is a correctly sized MONITORINFOEXW whose first member is MONITORINFO.
        if !unsafe { GetMonitorInfoW(monitor, &mut info as *mut _ as *mut MONITORINFO) }.as_bool() {
            continue;
        }
        let device = PCWSTR(info.szDevice.as_ptr());
        // The mode in pixels: a monitor rectangle is scaled for processes that are not DPI aware.
        let mut mode = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        // SAFETY: `device` points into `info`, which outlives the call; `mode` has its size set.
        let have_mode =
            unsafe { EnumDisplaySettingsW(device, ENUM_CURRENT_SETTINGS, &mut mode) }.as_bool();
        let rect = info.monitorInfo.rcMonitor;
        let (width, height, refresh_hz) = if have_mode && mode.dmPelsWidth > 0 {
            (mode.dmPelsWidth, mode.dmPelsHeight, mode.dmDisplayFrequency)
        } else {
            (
                (rect.right - rect.left).max(0) as u32,
                (rect.bottom - rect.top).max(0) as u32,
                0,
            )
        };
        // 0 and 1 mean "hardware default" in DEVMODE.
        let refresh_hz = if refresh_hz <= 1 { 0 } else { refresh_hz };
        out.push(DisplayInfo {
            handle: handle_value(monitor),
            name: display_name(index, single),
            width,
            height,
            refresh_hz,
        });
    }
    Ok(out)
}

/// The names the native picker gives screens, which the Go Live modal (`eA` of module 742527) reads: "Entire screen"
/// for a lone monitor, else "Screen 1", "Screen 2", in enumeration order.
fn display_name(index: usize, single: bool) -> String {
    if single {
        "Entire screen".to_owned()
    } else {
        format!("Screen {}", index + 1)
    }
}

fn windows(refresh: &HashMap<u64, u32>) -> Result<Vec<WindowInfo>> {
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the address of the Vec passed below, alive for the whole enumeration.
        let list = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
        list.push(hwnd);
        BOOL(1)
    }
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into the Vec whose address is passed.
    unsafe { EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize)) }
        .map_err(|e| Error::unavailable(format!("windows could not be enumerated: {e}")))?;
    let mut out = Vec::new();
    for hwnd in handles {
        // SAFETY: plain queries on a window handle that may have closed since it was listed; each call then fails or
        // returns zero, which the filters treat as "not listed".
        unsafe {
            let visible = IsWindowVisible(hwnd).as_bool();
            if !visible {
                continue;
            }
            let length = GetWindowTextLengthW(hwnd);
            if length <= 0 {
                continue;
            }
            let mut title = vec![0u16; length as usize + 1];
            let written = GetWindowTextW(hwnd, &mut title).max(0) as usize;
            let title = String::from_utf16_lossy(&title[..written]);
            let mut class = [0u16; 256];
            let class_len = GetClassNameW(hwnd, &mut class).max(0) as usize;
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let tool_window = style & WS_EX_TOOLWINDOW.0 != 0 && style & WS_EX_APPWINDOW.0 == 0;
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut _ as *mut _,
                size_of::<u32>() as u32,
            );
            let mut bounds = RECT::default();
            let framed = DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut bounds as *mut _ as *mut _,
                size_of::<RECT>() as u32,
            )
            .is_ok();
            if !framed {
                let _ = GetWindowRect(hwnd, &mut bounds);
            }
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            out.push(WindowInfo {
                hwnd: hwnd.0 as usize as u64,
                title,
                class: String::from_utf16_lossy(&class[..class_len]),
                pid,
                width: (bounds.right - bounds.left).max(0) as u32,
                height: (bounds.bottom - bounds.top).max(0) as u32,
                visible,
                minimized: IsIconic(hwnd).as_bool(),
                cloaked: cloaked != 0,
                tool_window,
                refresh_hz: refresh.get(&handle_value(monitor)).copied().unwrap_or(0),
            });
        }
    }
    Ok(out)
}

/// The window's own icon (or its class icon) as RGBA, at most 64x64. Best effort: `None` when it has none.
fn window_icon(hwnd: HWND) -> Option<Picture> {
    // SAFETY: message and class queries on a handle that may be gone; failures come back as zero.
    let icon = unsafe {
        let mut result = 0usize;
        let mut icon = 0usize;
        for kind in [ICON_BIG, ICON_SMALL2, ICON_SMALL] {
            let sent = SendMessageTimeoutW(
                hwnd,
                WM_GETICON,
                WPARAM(kind as usize),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                50,
                Some(&mut result),
            );
            if sent.0 != 0 && result != 0 {
                icon = result;
                break;
            }
        }
        if icon == 0 {
            icon = GetClassLongPtrW(hwnd, GCLP_HICON);
        }
        if icon == 0 {
            icon = GetClassLongPtrW(hwnd, GCLP_HICONSM);
        }
        HICON(icon as _)
    };
    if icon.is_invalid() {
        return None;
    }
    let picture = icon_pixels(icon)?;
    let raw = crate::convert::Raw {
        width: picture.width as usize,
        height: picture.height as usize,
        stride: picture.width as usize * 4,
        order: crate::convert::PixelOrder::Rgba,
        data: &picture.data,
    };
    crate::convert::downscale_rgba(raw, 64, 64).ok()
}

/// Reads an icon's colour and mask bitmaps into RGBA. The icon belongs to its window and is not destroyed here.
fn icon_pixels(icon: HICON) -> Option<Picture> {
    // SAFETY: GetIconInfo returns two bitmaps we own and delete; GetDIBits fills a buffer sized for the bitmap.
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(icon, &mut info).ok()?;
        let result = (|| {
            if info.hbmColor.is_invalid() {
                return None;
            }
            let mut bitmap = BITMAP::default();
            if GetObjectW(
                info.hbmColor.into(),
                size_of::<BITMAP>() as i32,
                Some(&mut bitmap as *mut _ as *mut _),
            ) == 0
            {
                return None;
            }
            let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);
            if !(1..=256).contains(&width) || !(1..=256).contains(&height) {
                return None;
            }
            let dc = GetDC(None);
            let mut header = BITMAPINFO::default();
            header.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            header.bmiHeader.biWidth = width;
            header.bmiHeader.biHeight = -height;
            header.bmiHeader.biPlanes = 1;
            header.bmiHeader.biBitCount = 32;
            header.bmiHeader.biCompression = BI_RGB.0;
            let mut color = vec![0u8; (width * height * 4) as usize];
            let got = GetDIBits(
                dc,
                info.hbmColor,
                0,
                height as u32,
                Some(color.as_mut_ptr().cast()),
                &mut header,
                DIB_RGB_COLORS,
            );
            let mut mask = vec![0u8; (width * height * 4) as usize];
            let mask_got = if info.hbmMask.is_invalid() {
                0
            } else {
                GetDIBits(
                    dc,
                    info.hbmMask,
                    0,
                    height as u32,
                    Some(mask.as_mut_ptr().cast()),
                    &mut header,
                    DIB_RGB_COLORS,
                )
            };
            ReleaseDC(None, dc);
            if got == 0 {
                return None;
            }
            // Icons with their own alpha carry it in the colour bitmap; older ones rely on the AND mask.
            let has_alpha = color.chunks_exact(4).any(|pixel| pixel[3] != 0);
            let mut rgba = Vec::with_capacity(color.len());
            for (index, pixel) in color.chunks_exact(4).enumerate() {
                let alpha = if has_alpha {
                    pixel[3]
                } else if mask_got != 0 && mask[index * 4] != 0 {
                    0
                } else {
                    255
                };
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], alpha]);
            }
            Some(Picture {
                width: width as u32,
                height: height as u32,
                layout: Layout::Rgba,
                data: rgba,
            })
        })();
        let _ = DeleteObject(info.hbmColor.into());
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(info.hbmMask.into());
        }
        result.filter(Picture::is_valid)
    }
}

/// The id a window handle is listed under.
#[allow(dead_code)]
pub(super) fn window_id(hwnd: HWND) -> SourceId {
    SourceId::Window(hwnd.0 as usize as u64)
}

#[cfg(test)]
mod tests {
    use super::display_name;

    #[test]
    fn screens_take_the_native_picker_names() {
        assert_eq!(display_name(0, true), "Entire screen");
        assert_eq!(display_name(0, false), "Screen 1");
        assert_eq!(display_name(1, false), "Screen 2");
    }
}
