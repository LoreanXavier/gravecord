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

#[cfg(windows)]
mod native {
    use super::{MAX_PROCESSES, Process};
    use std::collections::HashSet;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetForegroundWindow, GetWindow, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsWindowVisible,
    };

    fn wide(buffer: &[u16]) -> String {
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
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
                    out.push(Process { pid, name, path });
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            Ok(out)
        }
    }

    unsafe extern "system" fn each_window(window: *mut core::ffi::c_void, param: isize) -> i32 {
        // SAFETY: `param` is the address of the set `visible_window_pids` passes to `EnumWindows` below.
        unsafe {
            let pids = &mut *(param as *mut HashSet<u32>);
            if IsWindowVisible(window) != 0
                && GetWindowTextLengthW(window) > 0
                && GetWindow(window, GW_OWNER).is_null()
            {
                let mut pid = 0u32;
                GetWindowThreadProcessId(window, &mut pid);
                if pid != 0 {
                    pids.insert(pid);
                }
            }
        }
        1
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
}
