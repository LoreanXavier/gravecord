use super::Launch;
use md5::{Digest, Md5};
use std::{path::Path, sync::Arc, thread::JoinHandle, time::Duration};
use tokio::sync::mpsc::{Receiver, channel, error::TrySendError};
use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0},
        System::Threading::{
            CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
            WaitForSingleObject,
        },
        UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow},
    },
    core::HSTRING,
};

/// How long a later launch waits for the running instance to say it handled the request.
const ACK_WAIT: Duration = Duration::from_secs(3);

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

/// Sets the acknowledgement event. The running instance calls it once the window has been restored, so a later launch
/// knows the first one is alive and answering, not merely that an event exists.
#[derive(Clone)]
pub struct Acknowledger(Arc<Event>);
impl Acknowledger {
    pub fn acknowledge(&self) {
        unsafe {
            let _ = SetEvent(self.0.raw());
        }
    }
}

pub struct Instance {
    stop: Arc<Event>,
    ack: Arc<Event>,
    worker: Option<JoinHandle<()>>,
    requests: Option<Receiver<()>>,
}
impl Instance {
    pub fn take_requests(&mut self) -> Option<Receiver<()>> {
        self.requests.take()
    }
    pub fn acknowledger(&self) -> Acknowledger {
        Acknowledger(self.ack.clone())
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

fn ack_name(request: &str) -> String {
    format!("{request}.ack")
}

pub fn claim() -> anyhow::Result<Launch> {
    if crate::storage::is_capture_run() || std::env::args().any(|arg| arg == "--multi-instance") {
        return Ok(Launch::Primary(None));
    }
    let Some(path) = crate::storage::data_dir() else {
        return Ok(Launch::Primary(None));
    };
    claim_named(&name(&path), true, ACK_WAIT)
}

fn claim_named(name: &str, foreground: bool, ack_wait: Duration) -> anyhow::Result<Launch> {
    let ack_event = HSTRING::from(ack_name(name));
    let name = HSTRING::from(name);
    let handle = unsafe { CreateEventW(None, false, false, &name) }?;
    // Read immediately: later Win32 calls would replace this thread's last-error value.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let request = Event(handle);
    // Auto-reset: one acknowledgement answers one waiting launch. Whoever creates it first, both sides share it.
    let ack = Arc::new(Event(unsafe {
        CreateEventW(None, false, false, &ack_event)
    }?));
    if existed {
        if foreground {
            unsafe {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
            }
        }
        // An acknowledgement left over from an earlier launch that gave up must not answer this one.
        unsafe { ResetEvent(ack.raw()) }?;
        unsafe { SetEvent(request.raw()) }?;
        let answer = unsafe { WaitForSingleObject(ack.raw(), ack_wait.as_millis() as u32) };
        return Ok(if answer == WAIT_OBJECT_0 {
            Launch::Forwarded
        } else {
            Launch::Unresponsive
        });
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
        ack,
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
        assert_ne!(
            ack_name(&name(Path::new("C:/Data/A"))),
            name(Path::new("C:/Data/A"))
        );
    }

    const ENV: &str = "GRAVECORD_TEST_INSTANCE_KEY";
    const ENV_WAIT: &str = "GRAVECORD_TEST_ACK_WAIT_MS";
    const CHILD: &str = "instance::windows::tests::second_launch_child";

    /// Runs in the child process only: makes the second launch against the key the parent made.
    #[test]
    fn second_launch_child() {
        let Ok(key) = std::env::var(ENV) else {
            return;
        };
        assert!(key.starts_with("Local\\gravecord-test-"));
        let wait = std::env::var(ENV_WAIT)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5000);
        let launch = claim_named(&key, false, Duration::from_millis(wait)).unwrap();
        println!(
            "RESULT:{}",
            match launch {
                Launch::Forwarded => "forwarded",
                Launch::Unresponsive => "unresponsive",
                Launch::Primary(_) => "primary",
            }
        );
    }

    fn second_launch(key: &str, wait_ms: u64) -> std::process::Child {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD, "--nocapture"])
            .env(ENV, key)
            .env(ENV_WAIT, wait_ms.to_string())
            .creation_flags(CREATE_NO_WINDOW.0)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn wait_request(requests: &mut Receiver<()>) {
        let started = Instant::now();
        loop {
            if requests.try_recv().is_ok() {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "restore request missing"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_second_launch_is_forwarded_once_the_first_acknowledges() {
        let key = format!("Local\\gravecord-test-{}", uuid::Uuid::new_v4());
        let Launch::Primary(Some(mut primary)) = claim_named(&key, false, ACK_WAIT).unwrap() else {
            panic!("first claim");
        };
        let mut requests = primary.take_requests().unwrap();
        let acknowledger = primary.acknowledger();
        let child = second_launch(&key, 5000);
        wait_request(&mut requests);
        acknowledger.acknowledge();
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{stdout}");
        assert!(stdout.contains("RESULT:forwarded"), "{stdout}");
        drop(primary);
        assert!(matches!(
            requests.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        ));
        assert!(matches!(
            claim_named(&key, false, ACK_WAIT).unwrap(),
            Launch::Primary(Some(_))
        ));
    }

    #[test]
    fn a_second_launch_reports_a_first_instance_that_never_answers() {
        let key = format!("Local\\gravecord-test-{}", uuid::Uuid::new_v4());
        let Launch::Primary(Some(mut primary)) = claim_named(&key, false, ACK_WAIT).unwrap() else {
            panic!("first claim");
        };
        let mut requests = primary.take_requests().unwrap();
        let started = Instant::now();
        // The primary receives the request but never acknowledges, like a hung UI thread.
        let child = second_launch(&key, 400);
        wait_request(&mut requests);
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{stdout}");
        assert!(stdout.contains("RESULT:unresponsive"), "{stdout}");
        assert!(started.elapsed() >= Duration::from_millis(400));
    }

    #[test]
    fn a_stale_acknowledgement_does_not_answer_a_later_launch() {
        let key = format!("Local\\gravecord-test-{}", uuid::Uuid::new_v4());
        let Launch::Primary(Some(primary)) = claim_named(&key, false, ACK_WAIT).unwrap() else {
            panic!("first claim");
        };
        // Acknowledged while nobody waits: the event stays set.
        primary.acknowledger().acknowledge();
        let launch = claim_named(&key, false, Duration::from_millis(300)).unwrap();
        assert!(matches!(launch, Launch::Unresponsive));
    }
}
