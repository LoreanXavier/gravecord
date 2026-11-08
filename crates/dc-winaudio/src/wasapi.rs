//! The Windows audio session backend: every other application's session on every active playback device, through
//! `IMMDeviceEnumerator` -> `IAudioSessionManager2` -> `IAudioSessionControl2`, and their volumes through
//! `ISimpleAudioVolume`. Sessions of this process and the system sounds session are never listed.
#![allow(unsafe_code)]

use crate::attenuation::{Backend, Session};
use std::collections::HashMap;
use windows::Win32::Media::Audio::{
    AudioSessionStateExpired, DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::core::Interface;

/// COM initialised for the thread that holds it.
struct Com;

impl Com {
    fn init() -> Result<Self, String> {
        // S_OK, S_FALSE (already initialised) and the like are all non-negative.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_err() {
            return Err(format!("COM did not start: {result:?}"));
        }
        Ok(Self)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub struct SystemBackend {
    enumerator: IMMDeviceEnumerator,
    own: u32,
    /// The volume controls found by the last [`Backend::sessions`], by session id.
    controls: HashMap<String, ISimpleAudioVolume>,
    // Dropped last: COM stays up while the interfaces above are released.
    _com: Com,
}

impl SystemBackend {
    /// Must be built on the thread that will use it.
    pub fn new() -> Result<Self, String> {
        let com = Com::init()?;
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|error| format!("no device enumerator: {error}"))?;
        Ok(Self {
            enumerator,
            own: unsafe { GetCurrentProcessId() },
            controls: HashMap::new(),
            _com: com,
        })
    }

    fn read(&mut self) -> windows::core::Result<Vec<Session>> {
        let mut found = Vec::new();
        self.controls.clear();
        let devices = unsafe {
            self.enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?
        };
        for index in 0..unsafe { devices.GetCount()? } {
            let Ok(device) = (unsafe { devices.Item(index) }) else {
                continue;
            };
            let Ok(manager) =
                (unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) })
            else {
                continue;
            };
            let Ok(list) = (unsafe { manager.GetSessionEnumerator() }) else {
                continue;
            };
            let count = unsafe { list.GetCount() }.unwrap_or(0);
            for position in 0..count {
                let Ok(control) = (unsafe { list.GetSession(position) }) else {
                    continue;
                };
                let Ok(detail) = control.cast::<IAudioSessionControl2>() else {
                    continue;
                };
                if unsafe { detail.IsSystemSoundsSession() }.0 == 0 {
                    continue;
                }
                if unsafe { detail.GetProcessId() }.is_ok_and(|pid| pid == self.own) {
                    continue;
                }
                if unsafe { control.GetState() }
                    .is_ok_and(|state| state == AudioSessionStateExpired)
                {
                    continue;
                }
                let Ok(identifier) = (unsafe { detail.GetSessionInstanceIdentifier() }) else {
                    continue;
                };
                let id = unsafe { identifier.to_string() };
                unsafe { CoTaskMemFree(Some(identifier.0 as *const _)) };
                let Ok(id) = id else {
                    continue;
                };
                let Ok(volume) = control.cast::<ISimpleAudioVolume>() else {
                    continue;
                };
                let Ok(level) = (unsafe { volume.GetMasterVolume() }) else {
                    continue;
                };
                found.push(Session {
                    id: id.clone(),
                    volume: level,
                });
                self.controls.insert(id, volume);
            }
        }
        Ok(found)
    }
}

impl Backend for SystemBackend {
    fn sessions(&mut self) -> Result<Vec<Session>, String> {
        self.read()
            .map_err(|error| format!("audio sessions could not be read: {error}"))
    }

    fn set_volume(&mut self, id: &str, volume: f32) -> Result<(), String> {
        let control = self
            .controls
            .get(id)
            .ok_or_else(|| "the audio session is gone".to_owned())?;
        unsafe { control.SetMasterVolume(volume.clamp(0., 1.), std::ptr::null()) }
            .map_err(|error| format!("audio session volume could not be set: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attenuation::Attenuator;

    /// Live, on the real sessions: lowers every other application by 1 % (inaudible), checks each moved by that and
    /// no session of this process was touched, then restores and checks every volume is bit for bit what it was.
    /// The attenuator restores on drop, so a failing assertion still gives the volumes back.
    #[test]
    #[ignore = "changes other applications' real volumes on this machine; run by hand with --ignored"]
    fn the_real_sessions_go_down_by_the_factor_and_come_back_exactly() {
        let Ok(mut reader) = SystemBackend::new() else {
            eprintln!("no audio session manager here");
            return;
        };
        let before = reader.sessions().unwrap();
        eprintln!("{} other-application sessions", before.len());
        let mut attenuator = Attenuator::new(SystemBackend::new().unwrap());
        attenuator.attenuate(0.99);
        let during = reader.sessions().unwrap();
        for old in &before {
            if let Some(now) = during.iter().find(|session| session.id == old.id) {
                assert!(
                    (now.volume - old.volume * 0.99).abs() < 1e-5,
                    "{} went from {} to {}",
                    old.id,
                    old.volume,
                    now.volume
                );
            }
        }
        assert!(attenuator.restore());
        let after = reader.sessions().unwrap();
        for old in &before {
            if let Some(now) = after.iter().find(|session| session.id == old.id) {
                assert_eq!(
                    now.volume.to_bits(),
                    old.volume.to_bits(),
                    "{} restored",
                    old.id
                );
            }
        }
    }
}
