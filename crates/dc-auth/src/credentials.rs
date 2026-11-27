//! The OS credential store holds the login token, never a file. Windows Credential Manager, generic credentials.
//!
//! Adapted from Serein (MIT OR Apache-2.0): `apps/desktop/src/credentials.rs` and `crates/platform/src/login_keyring.rs`.
//! Kept from there: one worker thread orders every read, write and delete, each waits at most five seconds, a late
//! answer is dropped, and a token read back is validated before it is used. Left out: the plaintext fallback and the
//! Linux secret-service paths (gravecord is Windows only).

use dc_proto::secrets::Token;
use std::sync::mpsc;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use zeroize::Zeroize;

/// The service name every entry lives under.
pub const SERVICE: &str = "gravecord";
/// The entry of the account the app opens with.
pub const ACTIVE: &str = "session";
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialError {
    /// No credential store on this machine.
    NoStore,
    /// The store exists and did not answer or refused.
    Unavailable,
    /// What was stored is not a usable token.
    Invalid,
    TimedOut,
}

/// The per-account entry of Switch Accounts.
pub fn account_entry(user_id: &str) -> String {
    format!("account-{user_id}")
}

/// Drops the whitespace and the one pair of quotes browser storage shows around a token, and refuses what cannot be a
/// token (Serein's `SessionSecret::from_owner_input`).
fn clean(mut value: String) -> Result<Token, CredentialError> {
    let trimmed = value.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(trimmed);
    let usable = !unquoted.is_empty()
        && unquoted.len() <= 4096
        && unquoted.bytes().all(|byte| byte.is_ascii_graphic());
    let token = usable.then(|| Token::new(unquoted));
    value.zeroize();
    token.ok_or(CredentialError::Invalid)
}

enum Operation {
    Load(String),
    Save(String, Token),
    Forget(String),
}

type Answer = Result<Option<Token>, CredentialError>;
type Job = (Operation, mpsc::SyncSender<Answer>, Arc<AtomicBool>);

fn start_worker(execute: impl Fn(Operation) -> Answer + Send + 'static) -> mpsc::SyncSender<Job> {
    let (send, receive) = mpsc::sync_channel::<Job>(1);
    std::thread::spawn(move || {
        while let Ok((operation, reply, cancelled)) = receive.recv() {
            if cancelled.load(Ordering::Acquire) {
                continue;
            }
            let _ = reply.send(execute(operation));
        }
    });
    send
}

fn worker() -> &'static mpsc::SyncSender<Job> {
    static WORKER: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();
    WORKER.get_or_init(|| start_worker(execute))
}

fn request_with(send: &mpsc::SyncSender<Job>, operation: Operation, timeout: Duration) -> Answer {
    let (reply, receive) = mpsc::sync_channel(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    send.try_send((operation, reply, cancelled.clone()))
        .map_err(|_| CredentialError::Unavailable)?;
    let answer = receive.recv_timeout(timeout);
    if answer.is_err() {
        cancelled.store(true, Ordering::Release);
    }
    answer.unwrap_or(Err(CredentialError::TimedOut))
}

fn execute(operation: Operation) -> Answer {
    match operation {
        Operation::Load(target) => native::read(&target).and_then(|bytes| match bytes {
            Some(bytes) => String::from_utf8(bytes)
                .map_err(|_| CredentialError::Invalid)
                .and_then(clean)
                .map(Some),
            None => Ok(None),
        }),
        Operation::Save(target, token) => {
            native::write(&target, token.expose().as_bytes()).map(|()| None)
        }
        Operation::Forget(target) => native::delete(&target).map(|()| None),
    }
}

/// A set of entries under one service name. The app uses [`Store::app`]; tests use their own service so they never
/// touch a real login.
#[derive(Clone, Debug)]
pub struct Store {
    service: String,
}

impl Store {
    pub fn app() -> Self {
        Self::new(SERVICE)
    }

    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn target(&self, name: &str) -> String {
        format!("{}/{name}", self.service)
    }

    pub fn load(&self, name: &str) -> Result<Option<Token>, CredentialError> {
        request_with(worker(), Operation::Load(self.target(name)), TIMEOUT)
    }

    pub fn save(&self, name: &str, token: &Token) -> Result<(), CredentialError> {
        let token = clean(token.expose().to_owned())?;
        request_with(worker(), Operation::Save(self.target(name), token), TIMEOUT).map(|_| ())
    }

    pub fn forget(&self, name: &str) -> Result<(), CredentialError> {
        request_with(worker(), Operation::Forget(self.target(name)), TIMEOUT).map(|_| ())
    }
}

#[cfg(windows)]
mod native {
    use super::CredentialError;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
        CredReadW, CredWriteW,
    };

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain([0]).collect()
    }

    pub fn read(target: &str) -> Result<Option<Vec<u8>>, CredentialError> {
        let name = wide(target);
        let mut found: *mut CREDENTIALW = ptr::null_mut();
        let ok = unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) };
        if ok == 0 {
            return match unsafe { GetLastError() } {
                ERROR_NOT_FOUND => Ok(None),
                _ => Err(CredentialError::Unavailable),
            };
        }
        let bytes = unsafe {
            let credential = &*found;
            let slice = std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            );
            let bytes = slice.to_vec();
            CredFree(found.cast());
            bytes
        };
        Ok(Some(bytes))
    }

    pub fn write(target: &str, secret: &[u8]) -> Result<(), CredentialError> {
        let mut name = wide(target);
        let mut user = wide("gravecord");
        let mut blob = secret.to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: name.as_mut_ptr(),
            Comment: ptr::null_mut(),
            LastWritten: unsafe { std::mem::zeroed() },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: ptr::null_mut(),
            TargetAlias: ptr::null_mut(),
            UserName: user.as_mut_ptr(),
        };
        let ok = unsafe { CredWriteW(&credential, 0) };
        zeroize::Zeroize::zeroize(&mut blob);
        if ok == 0 {
            return Err(CredentialError::Unavailable);
        }
        Ok(())
    }

    pub fn delete(target: &str) -> Result<(), CredentialError> {
        let name = wide(target);
        let ok = unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 && unsafe { GetLastError() } != ERROR_NOT_FOUND {
            return Err(CredentialError::Unavailable);
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod native {
    use super::CredentialError;

    pub fn read(_: &str) -> Result<Option<Vec<u8>>, CredentialError> {
        Err(CredentialError::NoStore)
    }
    pub fn write(_: &str, _: &[u8]) -> Result<(), CredentialError> {
        Err(CredentialError::NoStore)
    }
    pub fn delete(_: &str) -> Result<(), CredentialError> {
        Err(CredentialError::NoStore)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_drops_quotes_and_refuses_junk() {
        assert_eq!(clean("  \"abc.def\"\n".into()).unwrap().expose(), "abc.def");
        assert_eq!(clean(String::new()).err(), Some(CredentialError::Invalid));
        assert_eq!(
            clean("has space".into()).err(),
            Some(CredentialError::Invalid)
        );
        assert_eq!(
            clean("tok\u{e9}n".into()).err(),
            Some(CredentialError::Invalid)
        );
    }

    #[test]
    fn stalled_native_work_is_bounded_and_expired_queued_writes_are_skipped() {
        let (release, gate) = mpsc::sync_channel(1);
        let (started, starting) = mpsc::sync_channel(1);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        let send = start_worker(move |_| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                started.send(()).unwrap();
                gate.recv().unwrap();
            }
            Ok(None)
        });
        let first = std::thread::spawn({
            let send = send.clone();
            move || {
                request_with(
                    &send,
                    Operation::Load("t".into()),
                    Duration::from_millis(50),
                )
            }
        });
        starting.recv().unwrap();
        assert_eq!(first.join().unwrap().err(), Some(CredentialError::TimedOut));
        // The worker is still busy; one queued job fits, the next is refused at once.
        let queued = request_with(
            &send,
            Operation::Forget("a".into()),
            Duration::from_millis(20),
        );
        assert_eq!(queued.err(), Some(CredentialError::TimedOut));
        let refused = request_with(
            &send,
            Operation::Forget("b".into()),
            Duration::from_millis(20),
        );
        assert_eq!(refused.err(), Some(CredentialError::Unavailable));
        release.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        // The expired job was skipped, not executed.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// Real Credential Manager round trip under a test-only service name, deleted at the end.
    #[cfg(windows)]
    #[test]
    fn credential_manager_round_trip() {
        let service = format!("gravecord-test-{}", std::process::id());
        let store = Store::new(service);
        let name = account_entry("test-entry");
        let _ = store.forget(&name);
        assert_eq!(
            store.load(&name).unwrap().map(|t| t.expose().to_owned()),
            None
        );
        store.save(&name, &Token::new("TEST.TOKEN.VALUE")).unwrap();
        let loaded = store.load(&name).unwrap().unwrap();
        assert_eq!(loaded.expose(), "TEST.TOKEN.VALUE");
        store.save(&name, &Token::new("TEST.OTHER.VALUE")).unwrap();
        assert_eq!(
            store.load(&name).unwrap().unwrap().expose(),
            "TEST.OTHER.VALUE"
        );
        store.forget(&name).unwrap();
        assert!(store.load(&name).unwrap().is_none());
        // Forgetting what is not there is not an error.
        store.forget(&name).unwrap();
    }
}
