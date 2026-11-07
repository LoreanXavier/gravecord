//! Starts programs on a hidden Win32 desktop, so their windows never reach the owner's screen.
//!
//! The desktop is never switched to, so nothing on it is displayed, not even for a moment at launch. It lives as long as
//! a process is attached to it. CDP and gpui's offscreen rendering still work there, which is all captures need.

use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;
use std::{mem, ptr};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_ALL, STILL_ACTIVE};
use windows_sys::Win32::System::StationsAndDesktops::CreateDesktopW;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DETACHED_PROCESS, GetExitCodeProcess, PROCESS_INFORMATION, STARTUPINFOW,
};

const DESKTOP: &str = "gravecord-headless";
const ATTACH_WAIT: Duration = Duration::from_secs(5);

fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain([0]).collect()
}

/// Windows command-line quoting: backslashes only need doubling in front of a quote.
fn quote(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        return argument.to_owned();
    }
    let mut out = String::from('"');
    let mut slashes = 0;
    for c in argument.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                slashes = 0;
            }
            _ => slashes = 0,
        }
        if c == '"' {
            out.push('"');
            continue;
        }
        out.push(c);
    }
    out.push_str(&"\\".repeat(slashes));
    out.push('"');
    out
}

pub fn spawn(exe: &Path, args: &[String], cwd: &Path) -> Result<u32> {
    let name = wide(DESKTOP);
    // Opens the desktop if it already exists.
    let desktop = unsafe {
        CreateDesktopW(
            name.as_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            GENERIC_ALL,
            ptr::null(),
        )
    };
    if desktop.is_null() {
        bail!("CreateDesktopW failed: {}", std::io::Error::last_os_error());
    }

    let mut line = quote(&exe.display().to_string());
    for argument in args {
        line.push(' ');
        line.push_str(&quote(argument));
    }
    let mut line = wide(line);
    let exe = wide(exe);
    let cwd = wide(cwd);
    let mut station = wide(format!("WinSta0\\{DESKTOP}"));

    let mut startup: STARTUPINFOW = unsafe { mem::zeroed() };
    startup.cb = mem::size_of::<STARTUPINFOW>() as u32;
    startup.lpDesktop = station.as_mut_ptr();
    let mut process: PROCESS_INFORMATION = unsafe { mem::zeroed() };

    let created = unsafe {
        CreateProcessW(
            exe.as_ptr(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            DETACHED_PROCESS,
            ptr::null(),
            cwd.as_ptr(),
            &startup,
            &mut process,
        )
    };
    if created == 0 {
        return Err(std::io::Error::last_os_error()).context("CreateProcessW failed");
    }
    // A process attaches to its desktop only when user32 initializes. Exiting now would close the last handle and destroy
    // the desktop first, and the child would die in loader init. So hold it until the child has had time to attach.
    std::thread::sleep(ATTACH_WAIT);
    let mut code = 0;
    let read = unsafe { GetExitCodeProcess(process.hProcess, &mut code) };
    unsafe {
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    if read != 0 && code != STILL_ACTIVE as u32 && code != 0 {
        bail!("process exited during startup with code {code:#x}");
    }
    Ok(process.dwProcessId)
}

pub fn run(repo: &Path, args: &[&str]) -> Result<()> {
    let [exe, rest @ ..] = args else {
        bail!("usage: cargo xtask headless <exe> [args]");
    };
    let rest: Vec<String> = rest.iter().map(|argument| (*argument).to_owned()).collect();
    let pid = spawn(&repo.join(exe), &rest, repo)?;
    println!("started pid {pid} on the hidden desktop");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn quotes_only_when_needed() {
        assert_eq!(quote("--flag=1"), "--flag=1");
        assert_eq!(quote(r"C:\a\b"), r"C:\a\b");
        assert_eq!(quote("a b"), r#""a b""#);
        assert_eq!(quote(""), r#""""#);
    }

    #[test]
    fn doubles_backslashes_before_quotes() {
        assert_eq!(quote(r"C:\a b\"), r#""C:\a b\\""#);
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
    }
}
