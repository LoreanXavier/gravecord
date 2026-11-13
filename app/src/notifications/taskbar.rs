//! Selected taskbar overlays. Owned handles stay on the notification worker.
//!
//! Desktop core 6820 (`appBadge`) maps the web count (596339 `Ni`) to one of eleven files of `app/images/badges`:
//! -1 is `badge-11.ico` (a white dot in the red circle: unread messages, nothing counted), 0 clears the overlay,
//! 1..9 are `badge-1.ico`..`badge-9.ico` and anything from 10 up is `badge-10.ico` ("9+"). It writes each ICO to the
//! user-data folder and hands `nativeImage.createFromPath(...)` to `win.setOverlayIcon`.
//!
//! What Electron gives the shell for such an image is not in the snapshot; it is read from Electron's and Chromium's
//! source (`NativeImage(isolate, hicon_path)`, `TaskbarHost::SetOverlayIcon`, `IconUtil`): an ICO path keeps, as its
//! picture, `LoadImage(path, IMAGE_ICON, 256, 256, LR_LOADFROMFILE)` (the 48 px frame stretched by Windows), turned
//! into a bitmap with `DrawIconEx` and back into an icon with the bitmap's bytes unchanged. The taskbar then scales
//! that 256 px icon to its overlay size for the display's DPI. [`official_overlay`] repeats those calls, so the shell
//! receives the same icon and scales it the same way. Handing it the 16 px frame instead (what this file did before)
//! shows the coarser hand-made 16 px drawing at 100 % and an enlarged blurry one above it.
use std::path::{Path, PathBuf};
use windows::{
    Win32::{
        Foundation::{E_FAIL, HWND},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, HGDIOBJ, SelectObject,
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        UI::{
            Shell::{ITaskbarList3, TaskbarList},
            WindowsAndMessaging::{
                DI_MASK, DI_NORMAL, DestroyIcon, DrawIconEx, FLASHW_ALL, FLASHW_STOP, FLASHWINFO,
                FlashWindowEx, GetForegroundWindow, GetWindowThreadProcessId, HICON, IMAGE_ICON,
                LR_LOADFROMFILE, LoadImageW,
            },
        },
    },
    core::{Error, HSTRING, Result},
};

/// 6820: -1 is the eleventh picture, 0 none, and 1.. the first ten with "9+" for ten and more.
pub(crate) fn badge_index(count: i64) -> Option<usize> {
    match count {
        ..=-1 => Some(10),
        0 => None,
        count => Some((count.clamp(1, 10) - 1) as usize),
    }
}

/// The overlay's accessible description, exactly as 6820 builds it: "Unread messages", "No Notifications", or the
/// zero-based picture index followed by " notifications" (one notification reads "0 notifications", ten or more read
/// "9 notifications").
fn badge_description(count: i64) -> String {
    if count <= -1 {
        return "Unread messages".to_owned();
    }
    match badge_index(count) {
        None => "No Notifications".to_owned(),
        Some(index) => format!("{index} notifications"),
    }
}

/// The official badge file for a picture index. Both icon sets use these (see `branding::badge_at`).
fn badge_file(index: usize) -> Option<PathBuf> {
    if index >= 11 {
        return None;
    }
    Some(crate::find_assets()?.join(format!("branding/original/badges/badge-{}.ico", index + 1)))
}

/// The size Electron loads an ICO path at (`GetHICON(256)`).
pub(crate) const OVERLAY_SIZE: i32 = 256;

struct Icon(HICON);
impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyIcon(self.0);
        }
    }
}

/// Chromium `IconUtil::CreateSkBitmapFromHICON` over `LoadImage(path, 256, 256, LR_LOADFROMFILE)`: top-down RGBA rows
/// as the bitmap holds them (premultiplied by `DrawIconEx`). `native_icon::create` stores them unchanged as the icon's
/// colour, as `CreateHICONFromSkBitmap` does. Memory bitmaps only: no window or desktop surface is used.
pub(crate) fn official_overlay(path: &Path) -> Result<Vec<u8>> {
    let size = OVERLAY_SIZE;
    let icon = Icon(HICON(
        unsafe {
            LoadImageW(
                None,
                &HSTRING::from(path.as_os_str()),
                IMAGE_ICON,
                size,
                size,
                LR_LOADFROMFILE,
            )?
        }
        .0,
    ));
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
    let mut bits = std::ptr::null_mut();
    let bitmap = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
    let dc = unsafe { CreateCompatibleDC(None) };
    let result = (|| {
        if bits.is_null() || dc.is_invalid() {
            return Err(Error::new(E_FAIL, "overlay bitmap unavailable"));
        }
        let length = (size * size * 4) as usize;
        let previous = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
        // The bitmap's extent is exactly `length` bytes (32 bpp, `size` square).
        let pixels = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), length) };
        // First the AND mask, kept for a picture without alpha; then the picture itself over cleared pixels.
        pixels.fill(0);
        let masked = unsafe { DrawIconEx(dc, 0, 0, icon.0, size, size, 0, None, DI_MASK) };
        let opaque: Vec<bool> = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| *p == [0; 4])
            .collect();
        pixels.fill(0);
        let drawn = unsafe { DrawIconEx(dc, 0, 0, icon.0, size, size, 0, None, DI_NORMAL) };
        let mut rgba = pixels.to_vec();
        unsafe {
            SelectObject(dc, previous);
        }
        masked?;
        drawn?;
        if rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
            for (pixel, opaque) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(opaque) {
                pixel[3] = if opaque { 255 } else { 0 };
            }
        }
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        Ok(rgba)
    })();
    unsafe {
        let _ = DeleteDC(dc);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
    result
}

/// The pixels the overlay for `count` holds (None when it is cleared), for capture renders.
pub(crate) fn overlay_pixels(count: i64) -> Option<Result<Vec<u8>>> {
    let index = badge_index(count)?;
    Some(
        badge_file(index)
            .ok_or_else(|| Error::new(E_FAIL, "badge file unavailable"))
            .and_then(|path| official_overlay(&path)),
    )
}

pub(super) struct Taskbar {
    hwnd: HWND,
    api: Option<ITaskbarList3>,
    icons: [Option<HICON>; 11],
    applied: Option<(Option<usize>, String)>,
    count: i64,
    icon_set: crate::branding::IconSet,
}
impl Taskbar {
    pub fn new(handle: usize) -> Self {
        Self {
            hwnd: HWND(handle as *mut _),
            api: None,
            icons: [None; 11],
            applied: None,
            count: 0,
            icon_set: crate::branding::selected(),
        }
    }
    pub fn ready(&mut self) -> Result<()> {
        let api: ITaskbarList3 =
            unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)? };
        unsafe {
            api.HrInit()?;
        }
        self.api = Some(api);
        self.applied = None;
        self.badge(self.count)
    }
    pub fn badge(&mut self, count: i64) -> Result<()> {
        self.count = count;
        let index = badge_index(count);
        let description = badge_description(count);
        if self.applied.as_ref() == Some(&(index, description.clone())) {
            return Ok(());
        }
        let Some(api) = &self.api else {
            return Ok(());
        };
        let icon = if let Some(index) = index {
            if self.icons[index].is_none() {
                let pixels = overlay_pixels(count)
                    .unwrap_or_else(|| Err(Error::new(E_FAIL, "badge image unavailable")))?;
                self.icons[index] = Some(super::native_icon::create(OVERLAY_SIZE, &pixels)?);
            }
            self.icons[index].unwrap_or_default()
        } else {
            HICON::default()
        };
        unsafe {
            api.SetOverlayIcon(self.hwnd, icon, &HSTRING::from(description.as_str()))?;
        }
        self.applied = Some((index, description));
        Ok(())
    }
    pub fn set_badge(&mut self, count: i64, set: crate::branding::IconSet) -> Result<()> {
        if self.icon_set != set {
            if let Some(api) = &self.api {
                unsafe {
                    api.SetOverlayIcon(
                        self.hwnd,
                        HICON::default(),
                        &HSTRING::from("No Notifications"),
                    )?;
                }
            }
            for icon in &mut self.icons {
                if let Some(icon) = icon.take() {
                    unsafe {
                        let _ = DestroyIcon(icon);
                    }
                }
            }
            self.icon_set = set;
            self.applied = None;
        }
        self.badge(count)
    }
    pub fn flash(&self, enabled: bool) {
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(self.hwnd, Some(&mut owner));
        }
        if owner != std::process::id() {
            return;
        }
        let enabled = enabled && unsafe { GetForegroundWindow() } != self.hwnd;
        // Chromium's HWNDMessageHandler::FlashFrame uses four flashes at the system's default interval.
        let info = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd: self.hwnd,
            dwFlags: if enabled { FLASHW_ALL } else { FLASHW_STOP },
            uCount: if enabled { 4 } else { 0 },
            dwTimeout: 0,
        };
        unsafe {
            let _ = FlashWindowEx(&info);
        }
    }
}
impl Drop for Taskbar {
    fn drop(&mut self) {
        self.flash(false);
        let _ = self.badge(0);
        for icon in self.icons.iter().flatten() {
            unsafe {
                let _ = DestroyIcon(*icon);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_badges_separate_unreads_counts_and_overflow() {
        assert_eq!(badge_index(-1), Some(10));
        assert_eq!(badge_index(0), None);
        assert_eq!(badge_index(1), Some(0));
        assert_eq!(badge_index(9), Some(8));
        assert_eq!(badge_index(10), Some(9));
        assert_eq!(badge_index(1000), Some(9));
    }

    #[test]
    fn the_overlay_description_uses_the_official_zero_based_index() {
        assert_eq!(badge_description(0), "No Notifications");
        assert_eq!(badge_description(-1), "Unread messages");
        assert_eq!(badge_description(1), "0 notifications");
        assert_eq!(badge_description(9), "8 notifications");
        assert_eq!(badge_description(10), "9 notifications");
        assert_eq!(badge_description(250), "9 notifications");
    }

    fn overlay(index: usize) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "assets/branding/original/badges/badge-{}.ico",
            index + 1
        ));
        official_overlay(&path).expect("official badge loads at 256")
    }

    #[test]
    fn the_overlay_is_the_stretched_large_frame_at_256_with_premultiplied_colour() {
        for index in 0..11 {
            let pixels = overlay(index);
            assert_eq!(pixels.len(), 256 * 256 * 4);
            let rows = pixels.as_chunks::<4>().0;
            // Corners are outside the circle (the files keep white at alpha 1 there), the centre row holds the red
            // disc and its white mark.
            assert!(rows[0][3] <= 1 && rows[256 * 256 - 1][3] <= 1);
            let middle = &rows[128 * 256..129 * 256];
            assert!(
                middle
                    .iter()
                    .any(|p| p[3] >= 250 && p[0] > 200 && p[1] < 120)
            );
            assert!(
                middle
                    .iter()
                    .any(|p| p[3] >= 250 && p[..3].iter().all(|c| *c > 240))
            );
            // Premultiplied: no channel exceeds its alpha.
            assert!(rows.iter().all(|p| p[..3].iter().all(|c| *c <= p[3])));
            // Stretched from 48 px: soft edges, many partly covered pixels on the rim.
            assert!(rows.iter().filter(|p| p[3] > 0 && p[3] < 255).count() > 200);
        }
        // The dot and "9+" are different pictures.
        assert_ne!(overlay(9), overlay(10));
    }
}
