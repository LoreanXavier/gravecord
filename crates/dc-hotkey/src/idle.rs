//! Read-only system input age. This does not change the account's presence or install an input hook.

#[cfg(windows)]
pub fn system_idle_ms() -> Option<u64> {
    use windows_sys::Win32::{
        System::SystemInformation::GetTickCount,
        UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
    };
    let mut input = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // Both API timestamps use the same wrapping 32-bit millisecond clock.
    unsafe {
        if GetLastInputInfo(&mut input) == 0 {
            return None;
        }
        Some(u64::from(GetTickCount().wrapping_sub(input.dwTime)))
    }
}

#[cfg(not(windows))]
pub fn system_idle_ms() -> Option<u64> {
    None
}
