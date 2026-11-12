//! The system calls: `WebAuthNAuthenticatorGetAssertion` and `WebAuthNAuthenticatorMakeCredential` of `webauthn.dll`,
//! which show the system's own security key / Windows Hello prompt over `window`. Every pointer handed to the API
//! points into a value that lives until the call returns; every result is copied out and freed with the API's own
//! free function.
#![allow(unsafe_code)]

use crate::response::{RawAssertion, RawAttestation};
use crate::{AssertionRequest, CreationRequest, Credential, Error};
use std::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::Networking::WindowsWebServices::*;
use windows::core::{HSTRING, PCWSTR};

/// `pAllowCredentialList` / `pExcludeCredentialList` came with API version 4 (Windows 10 2004).
pub const MIN_API_VERSION: u32 = WEBAUTHN_API_VERSION_4;

pub fn api_version() -> u32 {
    // SAFETY: a plain query of the loaded API.
    unsafe { WebAuthNGetApiVersionNumber() }
}

/// The user closing the prompt (`NTE_USER_CANCELLED`, `ERROR_CANCELLED`), the key not answering in time
/// (`ERROR_TIMEOUT`), no matching credential (`NTE_NOT_FOUND`) and a credential that exists already (`NTE_EXISTS`):
/// the cases a browser reports as `NotAllowedError` or `InvalidStateError`.
fn map_error(error: &windows::core::Error) -> Error {
    let code = error.code().0 as u32;
    match code {
        0x8009_0036 | 0x8007_05B4 | 0x8009_0011 | 0x8009_000F | 0x8007_04C7 => Error::Cancelled,
        other => Error::Failed(other),
    }
}

fn hwnd(window: isize) -> HWND {
    HWND(window as *mut c_void)
}

/// The id and transports of a list of credentials in the shape the API reads. `ids` keeps the bytes alive.
struct CredentialList {
    entries: Vec<WEBAUTHN_CREDENTIAL_EX>,
    pointers: Vec<*mut WEBAUTHN_CREDENTIAL_EX>,
    list: WEBAUTHN_CREDENTIAL_LIST,
}

impl CredentialList {
    /// `ids` must outlive the list.
    fn new(credentials: &[Credential], ids: &mut [Vec<u8>]) -> Box<Self> {
        let entries: Vec<WEBAUTHN_CREDENTIAL_EX> = credentials
            .iter()
            .zip(ids.iter_mut())
            .map(|(credential, id)| WEBAUTHN_CREDENTIAL_EX {
                dwVersion: WEBAUTHN_CREDENTIAL_EX_CURRENT_VERSION,
                cbId: credential.id.len() as u32,
                pbId: id.as_mut_ptr(),
                pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
                dwTransports: credential.transports,
            })
            .collect();
        let mut boxed = Box::new(Self {
            entries,
            pointers: Vec::new(),
            list: WEBAUTHN_CREDENTIAL_LIST {
                cCredentials: 0,
                ppCredentials: std::ptr::null_mut(),
            },
        });
        let pointers: Vec<*mut WEBAUTHN_CREDENTIAL_EX> = boxed
            .entries
            .iter_mut()
            .map(|entry| entry as *mut WEBAUTHN_CREDENTIAL_EX)
            .collect();
        boxed.pointers = pointers;
        boxed.list = WEBAUTHN_CREDENTIAL_LIST {
            cCredentials: boxed.entries.len() as u32,
            ppCredentials: boxed.pointers.as_mut_ptr(),
        };
        boxed
    }
}

fn copy(pointer: *const u8, length: u32) -> Vec<u8> {
    if pointer.is_null() || length == 0 {
        return Vec::new();
    }
    // SAFETY: the API returned `length` readable bytes at `pointer`.
    unsafe { std::slice::from_raw_parts(pointer, length as usize) }.to_vec()
}

pub fn get_assertion(
    window: isize,
    request: &AssertionRequest,
    client_data: &[u8],
) -> Result<RawAssertion, Error> {
    if api_version() < MIN_API_VERSION {
        return Err(Error::Unavailable);
    }
    let rp = HSTRING::from(request.rp_id.as_str());
    let mut data = client_data.to_vec();
    let client = WEBAUTHN_CLIENT_DATA {
        dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
        cbClientDataJSON: data.len() as u32,
        pbClientDataJSON: data.as_mut_ptr(),
        pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
    };
    let mut ids: Vec<Vec<u8>> = request.allow.iter().map(|c| c.id.clone()).collect();
    let mut allow = CredentialList::new(&request.allow, &mut ids);
    let mut options = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS {
        dwVersion: WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_4,
        dwTimeoutMilliseconds: request.timeout_ms,
        dwAuthenticatorAttachment: WEBAUTHN_AUTHENTICATOR_ATTACHMENT_ANY,
        dwUserVerificationRequirement: request.user_verification as u32,
        ..Default::default()
    };
    if !request.allow.is_empty() {
        options.pAllowCredentialList = &mut allow.list;
    }
    // SAFETY: every pointer refers to a local that outlives the call.
    let answer =
        unsafe { WebAuthNAuthenticatorGetAssertion(hwnd(window), &rp, &client, Some(&options)) }
            .map_err(|error| map_error(&error))?;
    if answer.is_null() {
        return Err(Error::Failed(0));
    }
    // SAFETY: a non-null result of the call above, freed once below.
    let raw = unsafe {
        let assertion = &*answer;
        let raw = RawAssertion {
            credential_id: copy(assertion.Credential.pbId, assertion.Credential.cbId),
            authenticator_data: copy(assertion.pbAuthenticatorData, assertion.cbAuthenticatorData),
            signature: copy(assertion.pbSignature, assertion.cbSignature),
            user_handle: (assertion.cbUserId > 0)
                .then(|| copy(assertion.pbUserId, assertion.cbUserId)),
            // `dwUsedTransport` is a version 4 field of the result.
            transport: if assertion.dwVersion >= 4 {
                assertion.dwUsedTransport
            } else {
                0
            },
        };
        WebAuthNFreeAssertion(answer);
        raw
    };
    Ok(raw)
}

pub fn create_credential(
    window: isize,
    request: &CreationRequest,
    client_data: &[u8],
) -> Result<RawAttestation, Error> {
    if api_version() < MIN_API_VERSION {
        return Err(Error::Unavailable);
    }
    let rp_id = HSTRING::from(request.rp_id.as_str());
    let rp_name = HSTRING::from(request.rp_name.as_str());
    let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
        pwszId: PCWSTR(rp_id.as_ptr()),
        pwszName: PCWSTR(rp_name.as_ptr()),
        pwszIcon: PCWSTR::null(),
    };
    let user_name = HSTRING::from(request.user_name.as_str());
    let display = HSTRING::from(request.user_display_name.as_str());
    let mut user_id = request.user_id.clone();
    let user = WEBAUTHN_USER_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
        cbId: user_id.len() as u32,
        pbId: user_id.as_mut_ptr(),
        pwszName: PCWSTR(user_name.as_ptr()),
        pwszIcon: PCWSTR::null(),
        pwszDisplayName: PCWSTR(display.as_ptr()),
    };
    let mut parameters: Vec<WEBAUTHN_COSE_CREDENTIAL_PARAMETER> = request
        .algorithms
        .iter()
        .map(|algorithm| WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
            dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
            pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
            lAlg: *algorithm,
        })
        .collect();
    let cose = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS {
        cCredentialParameters: parameters.len() as u32,
        pCredentialParameters: parameters.as_mut_ptr(),
    };
    let mut data = client_data.to_vec();
    let client = WEBAUTHN_CLIENT_DATA {
        dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
        cbClientDataJSON: data.len() as u32,
        pbClientDataJSON: data.as_mut_ptr(),
        pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
    };
    let mut ids: Vec<Vec<u8>> = request.exclude.iter().map(|c| c.id.clone()).collect();
    let mut exclude = CredentialList::new(&request.exclude, &mut ids);
    let mut options = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS {
        dwVersion: WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_4,
        dwTimeoutMilliseconds: request.timeout_ms,
        dwAuthenticatorAttachment: request.attachment,
        bRequireResidentKey: request.resident_key.into(),
        dwUserVerificationRequirement: request.user_verification as u32,
        dwAttestationConveyancePreference: request.attestation,
        ..Default::default()
    };
    if !request.exclude.is_empty() {
        options.pExcludeCredentialList = &mut exclude.list;
    }
    // SAFETY: every pointer refers to a local that outlives the call.
    let answer = unsafe {
        WebAuthNAuthenticatorMakeCredential(
            hwnd(window),
            &rp,
            &user,
            &cose,
            &client,
            Some(&options),
        )
    }
    .map_err(|error| map_error(&error))?;
    if answer.is_null() {
        return Err(Error::Failed(0));
    }
    // SAFETY: a non-null result of the call above, freed once below.
    let raw = unsafe {
        let attestation = &*answer;
        let raw = RawAttestation {
            credential_id: copy(attestation.pbCredentialId, attestation.cbCredentialId),
            attestation_object: copy(
                attestation.pbAttestationObject,
                attestation.cbAttestationObject,
            ),
            authenticator_data: copy(
                attestation.pbAuthenticatorData,
                attestation.cbAuthenticatorData,
            ),
            // `dwUsedTransport` is a version 3 field of the result.
            transport: if attestation.dwVersion >= 3 {
                attestation.dwUsedTransport
            } else {
                0
            },
        };
        WebAuthNFreeCredentialAttestation(Some(answer));
        raw
    };
    if raw.credential_id.is_empty() || raw.attestation_object.is_empty() {
        return Err(Error::Failed(0));
    }
    Ok(raw)
}
