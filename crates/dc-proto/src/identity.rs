//! Client identity: super properties and the IDENTIFY payload, field for field from `TRAFFIC/identify.json`.

use crate::pin;
use crate::qos;
use crate::secrets::Token;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use uuid::Uuid;

/// What the operating system reports. Windows values come from the registry, the same source Chromium reads.
#[derive(Clone, Debug, PartialEq)]
pub struct System {
    pub os: &'static str,
    pub os_version: String,
    pub os_sdk_version: String,
    pub os_build_revision: String,
    pub arch: &'static str,
    pub locale: String,
    pub timezone: String,
}

impl System {
    pub fn detect() -> Self {
        let (os_version, os_sdk_version, os_build_revision) = os_versions();
        Self {
            os: "Windows",
            os_version,
            os_sdk_version,
            os_build_revision,
            arch: "x64",
            locale: sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned()),
            timezone: iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_owned()),
        }
    }
}

#[cfg(windows)]
fn os_versions() -> (String, String, String) {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let dword = |name: &str| -> u32 {
        let (mut value, mut size) = (0u32, 4u32);
        let name = wide(name);
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&raw mut value).cast(),
                &mut size,
            )
        };
        if status == ERROR_SUCCESS { value } else { 0 }
    };
    let text = |name: &str| -> String {
        let mut buffer = [0u16; 64];
        let mut size = (buffer.len() * 2) as u32;
        let name = wide(name);
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return String::new();
        }
        let end = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    };
    let build = text("CurrentBuildNumber");
    let version = format!(
        "{}.{}.{build}",
        dword("CurrentMajorVersionNumber"),
        dword("CurrentMinorVersionNumber")
    );
    (version, build, dword("UBR").to_string())
}

#[cfg(not(windows))]
fn os_versions() -> (String, String, String) {
    (String::new(), String::new(), String::new())
}

/// Ids that stay fixed for one run of the client.
#[derive(Clone, Debug)]
pub struct Launch {
    pub client_launch_id: String,
    /// The original computes this in libdiscore from the JS environment. See `BLOCKED.md`.
    pub launch_signature: String,
    pub client_heartbeat_session_id: String,
}

impl Launch {
    pub fn new() -> Self {
        Self {
            client_launch_id: Uuid::new_v4().to_string(),
            launch_signature: Uuid::new_v4().to_string(),
            client_heartbeat_session_id: Uuid::new_v4().to_string(),
        }
    }
}

impl Default for Launch {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct Identity {
    pub system: System,
    pub launch: Launch,
}

impl Identity {
    pub fn new() -> Self {
        Self {
            system: System::detect(),
            launch: Launch::new(),
        }
    }

    pub fn super_properties(&self, focused: bool) -> Value {
        let system = &self.system;
        let launch = &self.launch;
        json!({
            "os": system.os,
            "browser": "Discord Client",
            "release_channel": pin::RELEASE_CHANNEL,
            "client_version": pin::CLIENT_VERSION,
            "os_version": system.os_version,
            "os_version_is_release": true,
            "os_sdk_version": system.os_sdk_version,
            "os_arch": system.arch,
            "app_arch": system.arch,
            "system_locale": system.locale,
            "has_client_mods": false,
            "client_launch_id": launch.client_launch_id,
            "browser_user_agent": pin::USER_AGENT,
            "browser_version": pin::ELECTRON_VERSION,
            "os_build_revision": system.os_build_revision,
            "client_build_number": pin::CLIENT_BUILD_NUMBER,
            "client_event_source": null,
            "launch_signature": launch.launch_signature,
            "client_app_state": if focused { "focused" } else { "unfocused" },
            "client_heartbeat_session_id": launch.client_heartbeat_session_id,
        })
    }

    pub fn super_properties_header(&self, focused: bool) -> String {
        STANDARD.encode(self.super_properties(focused).to_string())
    }

    pub fn identify(&self, token: &Token, state: &IdentifyState) -> Value {
        let mut properties = self.super_properties(state.focused);
        properties["is_fast_connect"] = json!(false);
        properties["gateway_connect_reasons"] = json!("AppSkeleton");
        if let Some(installation_id) = &state.installation_id {
            properties["installation_id"] = json!(installation_id);
        }
        json!({
            "token": token.expose(),
            "capabilities": pin::GATEWAY_CAPABILITIES,
            "properties": properties,
            "presence": {"status": state.status, "since": 0, "activities": [], "afk": false},
            "compress": false,
            "client_state": {"guild_versions": {}},
            "qos_token": qos::encode_token(state.active, state.qos_derived.as_deref()),
        })
    }
}

impl Default for Identity {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct IdentifyState {
    pub focused: bool,
    pub active: bool,
    /// Module 894768 (`SelfPresenceStore`): until the connection opens the local status is `unknown`, so the
    /// server keeps the status saved on the account; `online` here would show an invisible or do-not-disturb
    /// account as online at every login.
    pub status: String,
    pub qos_derived: Option<String>,
    pub installation_id: Option<String>,
}

impl Default for IdentifyState {
    fn default() -> Self {
        Self {
            focused: false,
            active: false,
            status: "unknown".to_owned(),
            qos_derived: None,
            installation_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Identity {
        Identity {
            system: System {
                os: "Windows",
                os_version: "10.0.26200".into(),
                os_sdk_version: "26200".into(),
                os_build_revision: "9445".into(),
                arch: "x64",
                locale: "en-US".into(),
                timezone: "Europe/Istanbul".into(),
            },
            launch: Launch::new(),
        }
    }

    const RECORDED_ORDER: [&str; 20] = [
        "os",
        "browser",
        "release_channel",
        "client_version",
        "os_version",
        "os_version_is_release",
        "os_sdk_version",
        "os_arch",
        "app_arch",
        "system_locale",
        "has_client_mods",
        "client_launch_id",
        "browser_user_agent",
        "browser_version",
        "os_build_revision",
        "client_build_number",
        "client_event_source",
        "launch_signature",
        "client_app_state",
        "client_heartbeat_session_id",
    ];

    #[test]
    fn super_properties_match_the_recorded_field_order() {
        let value = identity().super_properties(false);
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, RECORDED_ORDER);
        assert_eq!(value["client_build_number"], 667580);
    }

    #[test]
    fn a_login_leaves_the_saved_status_to_the_server() {
        let value = identity().identify(&Token::new("t"), &IdentifyState::default());
        assert_eq!(value["presence"]["status"], "unknown");
        assert_eq!(value["presence"]["activities"], json!([]));
    }

    #[test]
    fn identify_keys_match_the_recording() {
        let value = identity().identify(&Token::new("t"), &IdentifyState::default());
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "token",
                "capabilities",
                "properties",
                "presence",
                "compress",
                "client_state",
                "qos_token"
            ]
        );
        assert_eq!(
            value["properties"]["gateway_connect_reasons"],
            "AppSkeleton"
        );
        assert_eq!(value["qos_token"], "CgA=");
    }

    #[test]
    fn detects_a_plausible_windows_version() {
        let system = System::detect();
        assert!(
            system.os_version.starts_with("10.0."),
            "{}",
            system.os_version
        );
        assert!(system.os_sdk_version.parse::<u32>().unwrap() > 10000);
    }
}
