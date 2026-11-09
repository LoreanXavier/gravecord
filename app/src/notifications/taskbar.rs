//! Procedural taskbar overlays. Owned handles stay on the notification worker.
use windows::{
    Win32::{
        Foundation::HWND,
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        UI::{
            Shell::{ITaskbarList3, TaskbarList},
            WindowsAndMessaging::{
                DestroyIcon, FLASHW_ALL, FLASHW_STOP, FLASHWINFO, FlashWindowEx,
                GetForegroundWindow, GetWindowThreadProcessId, HICON,
            },
        },
    },
    core::HSTRING,
};

fn badge_index(count: i64) -> Option<usize> {
    match count {
        -1 => Some(10),
        0 => None,
        count => Some((count.clamp(1, 10) - 1) as usize),
    }
}

pub(super) struct Taskbar {
    hwnd: HWND,
    api: Option<ITaskbarList3>,
    icons: [Option<HICON>; 11],
    applied: Option<Option<usize>>,
    count: i64,
}
impl Taskbar {
    pub fn new(handle: usize) -> Self {
        Self {
            hwnd: HWND(handle as *mut _),
            api: None,
            icons: [None; 11],
            applied: None,
            count: 0,
        }
    }
    pub fn ready(&mut self) -> windows::core::Result<()> {
        let api: ITaskbarList3 =
            unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)? };
        unsafe {
            api.HrInit()?;
        }
        self.api = Some(api);
        self.applied = None;
        self.badge(self.count)
    }
    pub fn badge(&mut self, count: i64) -> windows::core::Result<()> {
        self.count = count;
        let index = badge_index(count);
        if self.applied == Some(index) {
            return Ok(());
        }
        let Some(api) = &self.api else {
            return Ok(());
        };
        let (icon, description) = if let Some(index) = index {
            if self.icons[index].is_none() {
                let font = (index != 10).then(crate::fonts::semibold).flatten();
                let Some(pixels) = super::badge_art::raster(index, font.as_deref()) else {
                    return Err(windows::core::Error::new(
                        windows::core::HRESULT(0x80004005u32 as i32),
                        "badge font unavailable",
                    ));
                };
                self.icons[index] = Some(super::native_icon::create(16, &pixels)?);
            }
            let description = if count == -1 {
                "Unread messages".to_owned()
            } else {
                format!("{index} notifications")
            };
            (self.icons[index].unwrap_or_default(), description)
        } else {
            (HICON::default(), "No Notifications".to_owned())
        };
        unsafe {
            api.SetOverlayIcon(self.hwnd, icon, &HSTRING::from(description))?;
        }
        self.applied = Some(index);
        Ok(())
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
}
