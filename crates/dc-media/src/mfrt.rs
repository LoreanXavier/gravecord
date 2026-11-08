//! COM and Media Foundation start-up held by the thread that owns the objects.
//!
//! Adapted from the `Runtime` guards in Serein's `crates/discord-voice/src/camera/windows.rs` and
//! `crates/platform/src/video/media_foundation.rs` (MIT OR Apache-2.0). Locals holding COM objects must be declared
//! after the guard so they release first; the guard is `!Send` because both calls belong to one thread.
#![allow(unsafe_code)]

use crate::{Error, Result};
use std::{marker::PhantomData, rc::Rc};
use windows::Win32::{
    Media::MediaFoundation::{MF_VERSION, MFSTARTUP_NOSOCKET, MFShutdown, MFStartup},
    System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
};

pub(crate) struct Runtime(PhantomData<Rc<()>>);

impl Runtime {
    pub(crate) fn open() -> Result<Self> {
        // SAFETY: balanced by Drop on the same thread; a failed MFStartup undoes the COM call before returning.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| {
                    Error::unavailable(format!("COM could not start on this thread: {e}"))
                })?;
            if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) {
                CoUninitialize();
                return Err(Error::unavailable(format!(
                    "Windows Media Foundation is unavailable (Windows N needs the Media Feature Pack): {e}"
                )));
            }
        }
        Ok(Self(PhantomData))
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // SAFETY: balanced with `open` on this thread, after every COM object of the owner has dropped.
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}
