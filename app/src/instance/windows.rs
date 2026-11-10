use super::Launch;
use md5::{Digest, Md5};
use std::{path::Path, sync::Arc, thread::JoinHandle};
use tokio::sync::mpsc::{Receiver, channel, error::TrySendError};
use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0},
        System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects},
        UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow},
    },
    core::HSTRING,
};

struct Event(HANDLE);
// Kernel event handles support concurrent waits/signals. The last owner closes the handle.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}
impl Event {
    fn raw(&self) -> HANDLE {
        self.0
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub struct Instance {
    stop: Arc<Event>,
    worker: Option<JoinHandle<()>>,
    requests: Option<Receiver<()>>,
}
impl Instance {
    pub fn take_requests(&mut self) -> Option<Receiver<()>> {
        self.requests.take()
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = SetEvent(self.stop.raw());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn name(path: &Path) -> String {
    let normalized = path
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    format!(
        "Local\\gravecord.{}.show",
        format_args!("{:x}", Md5::digest(normalized.as_bytes()))
    )
}

pub fn claim() -> anyhow::Result<Launch> {
    if crate::storage::is_capture_run() || std::env::args().any(|arg| arg == "--multi-instance") {
        return Ok(Launch::Primary(None));
    }
    let Some(path) = crate::storage::data_dir() else {
        return Ok(Launch::Primary(None));
    };
    claim_named(&name(&path), true)
}

fn claim_named(name: &str, foreground: bool) -> anyhow::Result<Launch> {
    let name = HSTRING::from(name);
    let handle = unsafe { CreateEventW(None, false, false, &name) }?;
    // Read immediately: later Win32 calls would replace this thread's last-error value.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let request = Event(handle);
    if existed {
        if foreground {
            unsafe {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
            }
        }
        unsafe { SetEvent(request.raw()) }?;
        return Ok(Launch::Forwarded);
    }
    let stop = Arc::new(Event(unsafe { CreateEventW(None, true, false, None) }?));
    let cancel = stop.clone();
    // Multiple launches only need one pending restore; a hidden window cannot accumulate a queue.
    let (sender, requests) = channel(1);
    let worker = std::thread::Builder::new()
        .name("gravecord-instance".into())
        .spawn(move || {
            let handles = [cancel.raw(), request.raw()];
            loop {
                let event = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                if event.0 != WAIT_OBJECT_0.0 + 1 {
                    break;
                }
                match sender.try_send(()) {
                    Ok(()) | Err(TrySendError::Full(())) => {}
                    Err(TrySendError::Closed(())) => break,
                }
            }
            // This thread alone owns the named handle. Exiting releases the claim even after a wait error.
        })?;
    Ok(Launch::Primary(Some(Instance {
        stop,
        worker: Some(worker),
        requests: Some(requests),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::windows::process::CommandExt,
        process::Command,
        time::{Duration, Instant},
    };
    use windows::Win32::System::Threading::CREATE_NO_WINDOW;

    #[cfg(feature = "capture")]
    #[test]
    fn capture_does_not_claim_or_signal_the_normal_profile() {
        assert!(matches!(claim().unwrap(), Launch::Primary(None)));
    }

    #[test]
    fn profile_case_and_separators_share_a_claim_but_other_profiles_do_not() {
        assert_eq!(
            name(Path::new("C:/Data/Gravecord/")),
            name(Path::new("c:\\data\\gravecord"))
        );
        assert_ne!(
            name(Path::new("C:/Data/Gravecord")),
            name(Path::new("C:/Data/Other"))
        );
    }

    #[test]
    fn another_process_forwards_without_starting_a_second_primary() {
        const ENV: &str = "GRAVECORD_TEST_INSTANCE_KEY";
        if let Ok(key) = std::env::var(ENV) {
            assert!(key.starts_with("Local\\gravecord-test-"));
            assert!(matches!(
                claim_named(&key, false).unwrap(),
                Launch::Forwarded
            ));
            return;
        }
        let key = format!("Local\\gravecord-test-{}", uuid::Uuid::new_v4());
        let Launch::Primary(Some(mut primary)) = claim_named(&key, false).unwrap() else {
            panic!("first claim");
        };
        let mut requests = primary.take_requests().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "instance::windows::tests::another_process_forwards_without_starting_a_second_primary", "--nocapture"])
            .env(ENV, &key).creation_flags(CREATE_NO_WINDOW.0).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let started = Instant::now();
        loop {
            if requests.try_recv().is_ok() {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "restore request missing"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(primary);
        assert!(matches!(
            requests.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        ));
        assert!(matches!(
            claim_named(&key, false).unwrap(),
            Launch::Primary(Some(_))
        ));
    }
}
