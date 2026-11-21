//! The running process list, read only.
//!
//! Serein (`vendor/serein/crates/platform/src/processes.rs`) runs `tasklist` and keeps only image names; this
//! reads the same table through Toolhelp and asks each process for its full image path (`QueryFullProcessImageNameW`
//! with limited-information access), so `obs/obs.exe` style entries and the registered games page's path hover can
//! work. Processes that refuse the query keep their image name. Yapfast has no process scanner.

use std::collections::HashSet;

/// Bounds the work and memory a hostile process table can force (the bound Serein uses).
pub const MAX_PROCESSES: usize = 4096;

/// One running process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    /// The image file name, `game.exe`.
    pub name: String,
    /// The full image path as the system reports it, or the file name when the query is refused.
    pub path: String,
    /// Whether the process runs elevated (admin privileges) on Windows (module 119191 / 397369).
    pub elevated: bool,
}

/// Every process this user may see.
pub fn running_processes() -> std::io::Result<Vec<Process>> {
    native::running_processes()
}

/// Process ids that own a visible, titled, unowned top-level window: what the registered games page offers for
/// "Add it!" (the official candidate games are processes with a window).
pub fn visible_window_pids() -> HashSet<u32> {
    native::visible_window_pids()
}

/// The process that owns the foreground window, when there is one: the game being played.
pub fn foreground_pid() -> Option<u32> {
    native::foreground_pid()
}

/// The window a process draws its picture in: its largest visible, titled, unowned top-level window (`windowHandle` of
/// the official client's visible game), as a handle value. `None` when the process has no such window.
pub fn main_window(pid: u32) -> Option<u64> {
    native::main_window(pid)
}

/// How long ago `pid` started, when readable.
pub fn process_age(pid: u32) -> Option<std::time::Duration> {
    native::process_age(pid)
}

/// Whether `pid` is elevated (running with administrator privileges) on Windows.
pub fn is_process_elevated(pid: u32) -> bool {
    native::is_process_elevated(pid)
}

/// Whether this process itself runs elevated on Windows.
pub fn is_current_process_elevated() -> bool {
    native::is_current_process_elevated()
}

#[cfg(windows)]
mod native {
    use super::{MAX_PROCESSES, Process};
    use std::collections::HashSet;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetForegroundWindow, GetWindow, GetWindowRect, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsWindowVisible,
    };

    fn wide(buffer: &[u16]) -> String {
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    fn query_token_elevated(handle: HANDLE) -> Option<bool> {
        use windows_sys::Win32::Security::{
            GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
        };
        use windows_sys::Win32::System::Threading::OpenProcessToken;
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(handle, TOKEN_QUERY, &mut token) == 0 || token.is_null() {
                return None;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut len: u32 = 0;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            );
            CloseHandle(token);
            (ok != 0).then_some(elevation.TokenIsElevated != 0)
        }
    }

    pub fn is_current_process_elevated() -> bool {
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        static SELF_ELEVATED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *SELF_ELEVATED.get_or_init(|| {
            // SAFETY: pseudo-handle for current process, never closed.
            let handle = unsafe { GetCurrentProcess() };
            query_token_elevated(handle).unwrap_or(false)
        })
    }

    pub fn is_process_elevated(pid: u32) -> bool {
        use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, GetLastError};
        // SAFETY: read-only handle queried and closed immediately.
        unsafe {
            let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return GetLastError() == ERROR_ACCESS_DENIED && !is_current_process_elevated();
            }
            let result = match query_token_elevated(handle) {
                Some(elevated) => elevated,
                None => GetLastError() == ERROR_ACCESS_DENIED && !is_current_process_elevated(),
            };
            CloseHandle(handle);
            result
        }
    }

    fn image_path(pid: u32) -> Option<String> {
        // SAFETY: the handle is checked, used for one query and closed.
        unsafe {
            let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buffer = [0u16; 1024];
            let mut length = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
            CloseHandle(handle);
            (ok != 0 && length > 0).then(|| wide(&buffer[..length as usize]))
        }
    }

    pub fn running_processes() -> std::io::Result<Vec<Process>> {
        // SAFETY: the snapshot handle is checked and closed; the entry is initialised with its size as the API asks.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error());
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut out = Vec::new();
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more && out.len() < MAX_PROCESSES {
                let name = wide(&entry.szExeFile);
                let pid = entry.th32ProcessID;
                if pid != 0 && !name.is_empty() {
                    let path = image_path(pid).unwrap_or_else(|| name.clone());
                    let elevated = is_process_elevated(pid);
                    out.push(Process {
                        pid,
                        name,
                        path,
                        elevated,
                    });
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            Ok(out)
        }
    }

    /// Top-level visible window info: `(pid, area, titled)`, excluding cloaked or tool windows.
    unsafe fn window_info(window: *mut core::ffi::c_void) -> Option<(u32, i64, bool)> {
        use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongW, IsIconic, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
        };
        // SAFETY: caller passes a valid window handle enumerated by `EnumWindows`.
        unsafe {
            if IsWindowVisible(window) == 0 || !GetWindow(window, GW_OWNER).is_null() {
                return None;
            }
            let mut cloaked = 0u32;
            if DwmGetWindowAttribute(
                window,
                DWMWA_CLOAKED as u32,
                (&mut cloaked as *mut u32).cast(),
                core::mem::size_of::<u32>() as u32,
            ) == 0
                && cloaked != 0
            {
                return None;
            }
            let ex_style = GetWindowLongW(window, GWL_EXSTYLE) as u32;
            if (ex_style & WS_EX_TOOLWINDOW) != 0 && (ex_style & WS_EX_APPWINDOW) == 0 {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(window, &mut pid);
            if pid == 0 {
                return None;
            }
            let titled = GetWindowTextLengthW(window) > 0;
            if IsIconic(window) != 0 {
                return titled.then_some((pid, 1, true));
            }
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(window, &mut rect) == 0 {
                return None;
            }
            let width = i64::from(rect.right - rect.left);
            let height = i64::from(rect.bottom - rect.top);
            if width < 64 || height < 64 {
                return None;
            }
            Some((pid, width * height, titled))
        }
    }

    unsafe extern "system" fn each_window(window: *mut core::ffi::c_void, param: isize) -> i32 {
        // SAFETY: `param` is the address of the set `visible_window_pids` passes to `EnumWindows` below.
        unsafe {
            let pids = &mut *(param as *mut HashSet<u32>);
            if let Some((pid, area, titled)) = window_info(window)
                && (titled || area >= 320 * 200)
            {
                pids.insert(pid);
            }
        }
        1
    }

    struct Search {
        pid: u32,
        best: Option<(bool, i64, u64)>,
    }

    unsafe extern "system" fn each_candidate(window: *mut core::ffi::c_void, param: isize) -> i32 {
        // SAFETY: `param` is the address of the `Search` that `main_window` passes to `EnumWindows` below.
        unsafe {
            let search = &mut *(param as *mut Search);
            let Some((pid, area, titled)) = window_info(window) else {
                return 1;
            };
            if pid != search.pid {
                return 1;
            }
            if search
                .best
                .is_none_or(|(best_titled, best_area, _)| (titled, area) > (best_titled, best_area))
            {
                search.best = Some((titled, area, window as usize as u64));
            }
        }
        1
    }

    pub fn main_window(pid: u32) -> Option<u64> {
        let mut search = Search { pid, best: None };
        // SAFETY: the callback only touches `search`, which outlives the call.
        unsafe {
            EnumWindows(Some(each_candidate), &mut search as *mut Search as isize);
        }
        search.best.map(|(_, _, window)| window)
    }

    pub fn foreground_pid() -> Option<u32> {
        // SAFETY: both calls only read window state.
        unsafe {
            let window = GetForegroundWindow();
            if window.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(window, &mut pid);
            (pid != 0).then_some(pid)
        }
    }

    pub fn visible_window_pids() -> HashSet<u32> {
        let mut pids = HashSet::new();
        // SAFETY: the callback only touches the set behind the pointer, which outlives the call.
        unsafe {
            EnumWindows(Some(each_window), &mut pids as *mut HashSet<u32> as isize);
        }
        pids
    }

    /// How long the process has been running, from `GetProcessTimes` (`None` if access is denied).
    pub fn process_age(pid: u32) -> Option<std::time::Duration> {
        use std::mem::zeroed;
        use std::time::{Duration, SystemTime, UNIX_EPOCH};
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::GetProcessTimes;

        // SAFETY: `handle` is checked, used only for `GetProcessTimes`, and closed before return.
        unsafe {
            let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut created: FILETIME = zeroed();
            let mut exited: FILETIME = zeroed();
            let mut kernel: FILETIME = zeroed();
            let mut user: FILETIME = zeroed();
            let ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let created_ticks =
                (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
            let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
            // Windows FILETIME epoch (1601-01-01) to Unix epoch (1970-01-01) in 100-ns intervals.
            const UNIX_EPOCH_IN_100NS: u64 = 116_444_736_000_000_000;
            let now_ticks = UNIX_EPOCH_IN_100NS
                .saturating_add(now_unix.as_secs().saturating_mul(10_000_000))
                .saturating_add(u64::from(now_unix.subsec_nanos() / 100));
            let age_100ns = now_ticks.saturating_sub(created_ticks);
            Some(Duration::from_nanos(age_100ns.saturating_mul(100)))
        }
    }
}

#[cfg(not(windows))]
mod native {
    use super::Process;
    use std::collections::HashSet;

    pub fn running_processes() -> std::io::Result<Vec<Process>> {
        Ok(Vec::new())
    }

    pub fn visible_window_pids() -> HashSet<u32> {
        HashSet::new()
    }

    pub fn foreground_pid() -> Option<u32> {
        None
    }

    pub fn main_window(_pid: u32) -> Option<u64> {
        None
    }

    pub fn process_age(_pid: u32) -> Option<std::time::Duration> {
        None
    }

    pub fn is_process_elevated(_pid: u32) -> bool {
        false
    }

    pub fn is_current_process_elevated() -> bool {
        false
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// The scanner finds this very test process by its pid, with a path that ends in its own image name.
    #[test]
    fn the_scanner_finds_its_own_process() {
        let list = running_processes().expect("the process table reads");
        let me = std::process::id();
        let mine = list
            .iter()
            .find(|process| process.pid == me)
            .expect("this process is listed");
        let exe = std::env::current_exe().unwrap();
        assert!(
            mine.path
                .to_lowercase()
                .ends_with(&exe.file_name().unwrap().to_string_lossy().to_lowercase())
        );
        assert!(list.len() <= MAX_PROCESSES);
    }

    #[test]
    fn process_age_reads_current_process() {
        assert!(super::process_age(std::process::id()).is_some());
    }
}
