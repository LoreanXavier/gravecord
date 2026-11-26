//! Audio endpoint change notifications: `IMMDeviceEnumerator::RegisterEndpointNotificationCallback` with an
//! `IMMNotificationClient`, so the device list is read again when Windows says a microphone or speaker was added,
//! removed, enabled, disabled, renamed or made the default, instead of every two seconds.
#![allow(unsafe_code)]

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::{PCWSTR, implement};

type Changed = Box<dyn Fn() + Send + Sync>;

#[implement(IMMNotificationClient)]
struct Client {
    changed: Changed,
}

impl IMMNotificationClient_Impl for Client_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        (self.changed)();
        Ok(())
    }

    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        (self.changed)();
        Ok(())
    }

    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        (self.changed)();
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        _: EDataFlow,
        _: ERole,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        (self.changed)();
        Ok(())
    }

    // A renamed device changes the listed names; other property changes (volume, format) are read once more and
    // compare equal.
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        (self.changed)();
        Ok(())
    }
}

/// COM initialised for the thread that holds it, until it is dropped.
struct Com;

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// The registration: notifications arrive (on a system thread) for as long as it lives. Build and drop it on the same
/// thread, which it initialises for COM.
pub struct EndpointWatch {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
    // Dropped last: COM stays up while the interfaces above are released.
    _com: Com,
}

impl EndpointWatch {
    /// Calls `changed` on every endpoint change from now on.
    pub fn new(changed: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        // S_OK, S_FALSE (already initialised) and the like are all non-negative.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_err() {
            return Err(format!("COM did not start: {result:?}"));
        }
        let com = Com;
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|error| format!("no device enumerator: {error}"))?;
        let client: IMMNotificationClient = Client {
            changed: Box::new(changed),
        }
        .into();
        unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }
            .map_err(|error| format!("no endpoint notifications: {error}"))?;
        Ok(Self {
            enumerator,
            client,
            _com: com,
        })
    }
}

impl Drop for EndpointWatch {
    fn drop(&mut self) {
        let _ = unsafe {
            self.enumerator
                .UnregisterEndpointNotificationCallback(&self.client)
        };
    }
}
