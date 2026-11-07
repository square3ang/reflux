//! Passkeys / security keys as a native WebAuthn client. We build the `clientDataJSON` a browser
//! would; Fluxer checks its `origin` against the instance's allowed web origins, which is what the
//! official desktop app's native addon also fills in itself. Sign-in and sudo use `get`, adding a
//! passkey `create`.
//!
//! - Windows: webauthn.dll (`WebAuthNAuthenticator*`), the system dialog browsers use. It covers
//!   Windows Hello (platform), USB/NFC/BLE keys and phones (hybrid); Windows blocks direct FIDO
//!   HID access for unelevated apps, so this is the only path there.
//! - Linux and macOS: Mozilla's `authenticator` (Firefox's CTAP1/2 stack) on USB HID keys.
//!   Linux has no platform authenticator to offer. macOS platform passkeys (iCloud Keychain)
//!   go through ASAuthorization, which only works in a signed app carrying an
//!   associated-domains `webcredentials:` entitlement the RP's apple-app-site-association file
//!   lists — a third-party client can't get that for fluxer.app, so macOS stays USB-only.
//!   shortcut: no hybrid (phone) flow off Windows.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use authenticator::Pin;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use reflux_model::{InstanceDiscovery, WebAuthnAuthenticationOptions};
use serde_json::json;

/// What the key needs from the person, for the passkey screen. On Windows the system dialog
/// asks for device choice and PIN itself.
#[cfg_attr(windows, allow(dead_code))]
pub enum Status {
    Touch,
    /// Several keys: touch the one to use.
    SelectDevice,
    /// The PIN (again, with `attempts` left, when the last one was wrong).
    Pin {
        reply: Sender<Pin>,
        wrong: bool,
        attempts: Option<u8>,
    },
}

/// Cancels a running request (the screen was left).
#[derive(Clone)]
pub struct Cancel(Arc<dyn Fn() + Send + Sync>);

impl Cancel {
    pub fn cancel(&self) {
        (self.0)()
    }
}

/// The web origin a browser would report for `rp_id`: the instance web app, or its API client
/// host (fluxer.com's legacy RP `fluxer.app` lives on `web.fluxer.app`).
pub fn origin_for(d: &InstanceDiscovery, rp_id: &str) -> String {
    let origin = |u: &str| {
        url::Url::parse(u).ok().map(|u| {
            (
                u.origin().ascii_serialization(),
                u.host_str().unwrap_or("").to_owned(),
            )
        })
    };
    let candidates: Vec<_> = [&d.endpoints.webapp, &d.endpoints.api_client]
        .into_iter()
        .filter_map(|u| origin(u))
        .collect();
    candidates
        .iter()
        .find(|(_, host)| host == rp_id || host.ends_with(&format!(".{rp_id}")))
        .or(candidates.first())
        .map(|(o, _)| o.clone())
        .unwrap_or_else(|| format!("https://{rp_id}"))
}

pub fn client_data_json(challenge: &str, origin: &str) -> String {
    client_data("webauthn.get", challenge, origin)
}

fn client_data(kind: &str, challenge: &str, origin: &str) -> String {
    json!({"type": kind, "challenge": challenge, "origin": origin, "crossOrigin": false})
        .to_string()
}

/// Starts `navigator.credentials.get()`; blocks until done. `status` gets the prompts; the result
/// is the `PublicKeyCredential` JSON Fluxer takes as `response`.
pub fn get(
    opts: &WebAuthnAuthenticationOptions,
    origin: &str,
    status: impl Fn(Status) + Send + 'static,
    cancel: impl FnOnce(Cancel),
) -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    return win::get(opts, origin, status, cancel);
    #[cfg(not(windows))]
    hid::get(opts, origin, status, cancel)
}

/// `navigator.credentials.create()` (`performRegistration`) from Fluxer's
/// `PublicKeyCredentialCreationOptionsJSON`; blocks until done. The result is the
/// `RegistrationResponseJSON` `POST /users/@me/mfa/webauthn/credentials` takes.
pub fn create(
    opts: &serde_json::Value,
    origin: &str,
    status: impl Fn(Status) + Send + 'static,
    cancel: impl FnOnce(Cancel),
) -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    return win::create(opts, origin, status, cancel);
    #[cfg(not(windows))]
    hid::create(opts, origin, status, cancel)
}

/// CTAP transport bits (webauthn.h `WEBAUTHN_CTAP_TRANSPORT_*`) as WebAuthn transport names.
fn transports(bits: u32) -> Vec<&'static str> {
    [
        (1, "usb"),
        (2, "nfc"),
        (4, "ble"),
        (16, "internal"),
        (32, "hybrid"),
    ]
    .into_iter()
    .filter(|(b, _)| bits & b != 0)
    .map(|(_, n)| n)
    .collect()
}

/// `authenticatorAttachment` for the transport used: the built-in authenticator is "platform".
fn attachment(bits: u32) -> &'static str {
    if bits & 16 != 0 {
        "platform"
    } else {
        "cross-platform"
    }
}

fn assertion_json(
    id: &[u8],
    client_data: &str,
    auth_data: &[u8],
    signature: &[u8],
    user: Option<&[u8]>,
    transport: u32,
) -> serde_json::Value {
    let id = B64.encode(id);
    json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "response": {
            "clientDataJSON": B64.encode(client_data),
            "authenticatorData": B64.encode(auth_data),
            "signature": B64.encode(signature),
            "userHandle": user.map(|u| B64.encode(u)),
        },
        "clientExtensionResults": {},
        "authenticatorAttachment": attachment(transport),
    })
}

fn registration_json(
    id: &[u8],
    client_data: &str,
    attestation: &[u8],
    transport: u32,
) -> serde_json::Value {
    let id = B64.encode(id);
    json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "response": {
            "clientDataJSON": B64.encode(client_data),
            "attestationObject": B64.encode(attestation),
            "transports": transports(transport),
        },
        "clientExtensionResults": {},
        "authenticatorAttachment": attachment(transport),
    })
}

fn b64_bytes(v: &serde_json::Value) -> Result<Vec<u8>, String> {
    B64.decode(v.as_str().unwrap_or_default().trim_end_matches('='))
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
mod hid {
    use std::sync::mpsc::{Sender, channel};
    use std::sync::{Arc, Mutex};

    use authenticator::authenticatorservice::{AuthenticatorService, SignArgs};
    use authenticator::ctap2::server::{
        PublicKeyCredentialDescriptor, UserVerificationRequirement,
    };
    use authenticator::statecallback::StateCallback;
    use authenticator::{StatusPinUv, StatusUpdate};
    use reflux_model::WebAuthnAuthenticationOptions;
    use sha2::{Digest, Sha256};

    use super::{Cancel, Status, b64_bytes};

    const USB: u32 = 1;

    fn service(cancel: impl FnOnce(Cancel)) -> Result<Arc<Mutex<AuthenticatorService>>, String> {
        let mut service = AuthenticatorService::new().map_err(|e| format!("{e:?}"))?;
        service.add_u2f_usb_hid_platform_transports();
        let service = Arc::new(Mutex::new(service));
        let s = service.clone();
        cancel(Cancel(Arc::new(move || {
            if let Ok(mut m) = s.lock() {
                let _ = m.cancel();
            }
        })));
        Ok(service)
    }

    pub fn get(
        opts: &WebAuthnAuthenticationOptions,
        origin: &str,
        status: impl Fn(Status) + Send + 'static,
        cancel: impl FnOnce(Cancel),
    ) -> Result<serde_json::Value, String> {
        let service = service(cancel)?;
        let allow_list: Vec<PublicKeyCredentialDescriptor> = opts
            .allow_credentials
            .iter()
            .filter_map(|c| b64_bytes(c.get("id")?).ok())
            .map(|id| PublicKeyCredentialDescriptor {
                id,
                transports: vec![],
            })
            .collect();
        let client_data = super::client_data_json(&opts.challenge, origin);
        let args = SignArgs {
            client_data_hash: Sha256::digest(client_data.as_bytes()).into(),
            origin: origin.to_owned(),
            relying_party_id: opts.rp_id.clone(),
            allow_list: allow_list.clone(),
            user_verification_req: match opts.user_verification.as_str() {
                "required" => UserVerificationRequirement::Required,
                "discouraged" => UserVerificationRequirement::Discouraged,
                _ => UserVerificationRequirement::Preferred,
            },
            user_presence_req: true,
            extensions: Default::default(),
            pin: None,
            use_ctap1_fallback: false,
        };

        let status_tx = status_channel(status);

        let timeout = u64::from(opts.timeout.max(30_000));
        let (tx, rx) = channel();
        let callback = StateCallback::new(Box::new(move |r| {
            let _ = tx.send(r);
        }));
        service
            .lock()
            .map_err(|e| e.to_string())?
            .sign(timeout, args, status_tx, callback)
            .map_err(|e| format!("{e:?}"))?;
        let result = rx.recv().map_err(|e| e.to_string())?.map_err(describe)?;

        let a = result.assertion;
        // CTAP2 may leave out the credential when the allow list had exactly one.
        let id = a
            .credentials
            .map(|c| c.id)
            .or_else(|| (allow_list.len() == 1).then(|| allow_list[0].id.clone()))
            .ok_or("The security key didn't name its credential.")?;
        Ok(super::assertion_json(
            &id,
            &client_data,
            &a.auth_data.to_vec(),
            &a.signature,
            a.user.as_ref().map(|u| &u.id[..]),
            USB,
        ))
    }

    pub fn create(
        opts: &serde_json::Value,
        origin: &str,
        status: impl Fn(Status) + Send + 'static,
        cancel: impl FnOnce(Cancel),
    ) -> Result<serde_json::Value, String> {
        use authenticator::authenticatorservice::RegisterArgs;
        use authenticator::ctap2::server::{
            PublicKeyCredentialParameters, PublicKeyCredentialUserEntity, RelyingParty,
            ResidentKeyRequirement,
        };
        let s = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
        let bytes = b64_bytes;
        let rp_id = s(&opts["rp"]["id"]);
        let challenge = s(&opts["challenge"]);
        let client_data = super::client_data("webauthn.create", &challenge, origin);
        let sel = &opts["authenticatorSelection"];
        let service = service(cancel)?;
        let args = RegisterArgs {
            client_data_hash: Sha256::digest(client_data.as_bytes()).into(),
            relying_party: RelyingParty {
                id: rp_id,
                name: opts["rp"]["name"].as_str().map(str::to_owned),
            },
            origin: origin.to_owned(),
            user: PublicKeyCredentialUserEntity {
                id: bytes(&opts["user"]["id"])?,
                name: opts["user"]["name"].as_str().map(str::to_owned),
                display_name: opts["user"]["displayName"].as_str().map(str::to_owned),
            },
            pub_cred_params: opts["pubKeyCredParams"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| {
                    PublicKeyCredentialParameters::try_from(p["alg"].as_i64()? as i32).ok()
                })
                .collect(),
            exclude_list: opts["excludeCredentials"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| bytes(&c["id"]).ok())
                .map(|id| PublicKeyCredentialDescriptor {
                    id,
                    transports: vec![],
                })
                .collect(),
            user_verification_req: match sel["userVerification"].as_str() {
                Some("required") => UserVerificationRequirement::Required,
                Some("discouraged") => UserVerificationRequirement::Discouraged,
                _ => UserVerificationRequirement::Preferred,
            },
            resident_key_req: match sel["residentKey"].as_str() {
                Some("required") => ResidentKeyRequirement::Required,
                Some("discouraged") => ResidentKeyRequirement::Discouraged,
                _ => ResidentKeyRequirement::Preferred,
            },
            extensions: Default::default(),
            pin: None,
            use_ctap1_fallback: false,
        };
        let status_tx = status_channel(status);
        let timeout = opts["timeout"].as_u64().unwrap_or(60_000).max(30_000);
        let (tx, rx) = channel();
        let callback = StateCallback::new(Box::new(move |r| {
            let _ = tx.send(r);
        }));
        service
            .lock()
            .map_err(|e| e.to_string())?
            .register(timeout, args, status_tx, callback)
            .map_err(|e| format!("{e:?}"))?;
        let made = rx.recv().map_err(|e| e.to_string())?.map_err(describe)?;
        let id = made
            .att_obj
            .auth_data
            .credential_data
            .as_ref()
            .map(|c| c.credential_id.clone())
            .ok_or("The security key didn't return a credential.")?;
        let attestation = serde_cbor::to_vec(&made.att_obj).map_err(|e| e.to_string())?;
        Ok(super::registration_json(
            &id,
            &client_data,
            &attestation,
            USB,
        ))
    }

    /// Forwards the key's prompts to `status` on a thread of its own.
    fn status_channel(status: impl Fn(Status) + Send + 'static) -> Sender<StatusUpdate> {
        let (status_tx, status_rx) = channel::<StatusUpdate>();
        std::thread::spawn(move || {
            while let Ok(up) = status_rx.recv() {
                match up {
                    StatusUpdate::PresenceRequired => status(Status::Touch),
                    StatusUpdate::SelectDeviceNotice => status(Status::SelectDevice),
                    StatusUpdate::PinUvError(StatusPinUv::PinRequired(reply)) => {
                        status(Status::Pin {
                            reply,
                            wrong: false,
                            attempts: None,
                        })
                    }
                    StatusUpdate::PinUvError(StatusPinUv::InvalidPin(reply, attempts)) => {
                        status(Status::Pin {
                            reply,
                            wrong: true,
                            attempts,
                        })
                    }
                    // Several discoverable credentials: take the first, like a one-account key.
                    StatusUpdate::SelectResultNotice(reply, _) => {
                        let _ = reply.send(Some(0));
                    }
                    // Other PIN/UV failures end the request; the callback reports them.
                    _ => {}
                }
            }
        });
        status_tx
    }

    fn describe(e: authenticator::errors::AuthenticatorError) -> String {
        use authenticator::{PinError as P, errors::AuthenticatorError as E};
        match e {
            E::CancelledByUser => "Cancelled.".into(),
            E::PinError(P::PinBlocked) => {
                "Your security key's PIN is blocked. Reset the key to use it again.".into()
            }
            E::PinError(P::PinAuthBlocked) => {
                "Too many wrong PINs. Unplug the security key and plug it back in.".into()
            }
            E::PinError(P::PinNotSet) => "Set a PIN on your security key first.".into(),
            E::PinError(P::UvBlocked) => {
                "Too many failed fingerprint attempts. Enter your PIN or unplug the key.".into()
            }
            other => format!("The security key request failed: {other}."),
        }
    }
}

/// webauthn.dll: Windows shows its own dialog (Hello, keys, phone), so `status` only gets `Touch`.
/// The option structs are passed with an older `dwVersion` so Windows 10 1903+ (API v1/v2) reads
/// only the fields it knows; `pCancellationId` needs options v3 (2004).
#[cfg(windows)]
mod win {
    use std::sync::Arc;

    use reflux_model::WebAuthnAuthenticationOptions;
    use windows::Win32::Foundation::{NTE_EXISTS, NTE_NOT_FOUND, NTE_USER_CANCELLED};
    use windows::Win32::Networking::WindowsWebServices::*;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    use windows::core::{GUID, HSTRING, PCWSTR};

    use super::{Cancel, Status, b64_bytes};

    fn uv(s: Option<&str>) -> u32 {
        match s {
            Some("required") => WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED,
            Some("discouraged") => WEBAUTHN_USER_VERIFICATION_REQUIREMENT_DISCOURAGED,
            _ => WEBAUTHN_USER_VERIFICATION_REQUIREMENT_PREFERRED,
        }
    }

    fn cancellation(cancel: impl FnOnce(Cancel)) -> Option<GUID> {
        let id = unsafe { WebAuthNGetCancellationId() }.ok()?;
        cancel(Cancel(Arc::new(move || unsafe {
            let _ = WebAuthNCancelCurrentOperation(&id);
        })));
        Some(id)
    }

    /// `WEBAUTHN_CREDENTIAL`s pointing into `ids`, which must outlive them.
    fn credentials(ids: &mut [Vec<u8>]) -> Vec<WEBAUTHN_CREDENTIAL> {
        ids.iter_mut()
            .map(|id| WEBAUTHN_CREDENTIAL {
                dwVersion: WEBAUTHN_CREDENTIAL_CURRENT_VERSION,
                cbId: id.len() as u32,
                pbId: id.as_mut_ptr(),
                pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
            })
            .collect()
    }

    fn describe(e: windows::core::Error) -> String {
        match e.code() {
            NTE_USER_CANCELLED => "Cancelled.".into(),
            NTE_NOT_FOUND => "No passkey for this account was found.".into(),
            NTE_EXISTS => "This authenticator already has a passkey for your account.".into(),
            _ => format!(
                "The passkey request failed: {}.",
                e.message().trim_end_matches(['.', '\r', '\n'])
            ),
        }
    }

    unsafe fn bytes<'a>(p: *const u8, n: u32) -> &'a [u8] {
        if p.is_null() || n == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(p, n as usize) }
        }
    }

    pub fn get(
        opts: &WebAuthnAuthenticationOptions,
        origin: &str,
        status: impl Fn(Status) + Send + 'static,
        cancel: impl FnOnce(Cancel),
    ) -> Result<serde_json::Value, String> {
        let client_data = super::client_data_json(&opts.challenge, origin);
        let mut cd = client_data.clone().into_bytes();
        let cdata = WEBAUTHN_CLIENT_DATA {
            dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
            cbClientDataJSON: cd.len() as u32,
            pbClientDataJSON: cd.as_mut_ptr(),
            pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
        };
        let mut ids: Vec<Vec<u8>> = opts
            .allow_credentials
            .iter()
            .filter_map(|c| b64_bytes(c.get("id")?).ok())
            .collect();
        let mut creds = credentials(&mut ids);
        let mut cancel_id = cancellation(cancel);
        let o = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS {
            dwVersion: 4,
            dwTimeoutMilliseconds: opts.timeout.max(30_000) as u32,
            CredentialList: WEBAUTHN_CREDENTIALS {
                cCredentials: creds.len() as u32,
                pCredentials: creds.as_mut_ptr(),
            },
            dwAuthenticatorAttachment: WEBAUTHN_AUTHENTICATOR_ATTACHMENT_ANY,
            dwUserVerificationRequirement: uv(Some(opts.user_verification.as_str())),
            pCancellationId: cancel_id
                .as_mut()
                .map_or(std::ptr::null_mut(), |g| g as *mut GUID),
            ..Default::default()
        };
        status(Status::Touch);
        let rp = HSTRING::from(opts.rp_id.as_str());
        let a = unsafe {
            WebAuthNAuthenticatorGetAssertion(GetForegroundWindow(), &rp, &cdata, Some(&o))
        }
        .map_err(describe)?;
        let out = unsafe {
            let r = &*a;
            let transport = if r.dwVersion >= 4 {
                r.dwUsedTransport
            } else {
                0
            };
            let user = (r.cbUserId > 0).then(|| bytes(r.pbUserId, r.cbUserId));
            let v = super::assertion_json(
                bytes(r.Credential.pbId, r.Credential.cbId),
                &client_data,
                bytes(r.pbAuthenticatorData, r.cbAuthenticatorData),
                bytes(r.pbSignature, r.cbSignature),
                user,
                transport,
            );
            WebAuthNFreeAssertion(a);
            v
        };
        Ok(out)
    }

    pub fn create(
        opts: &serde_json::Value,
        origin: &str,
        status: impl Fn(Status) + Send + 'static,
        cancel: impl FnOnce(Cancel),
    ) -> Result<serde_json::Value, String> {
        let s = |v: &serde_json::Value| HSTRING::from(v.as_str().unwrap_or_default());
        let rp_id = s(&opts["rp"]["id"]);
        let rp_name = opts["rp"]["name"]
            .as_str()
            .map_or_else(|| rp_id.clone(), HSTRING::from);
        let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
            dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
            pwszId: PCWSTR(rp_id.as_ptr()),
            pwszName: PCWSTR(rp_name.as_ptr()),
            pwszIcon: PCWSTR::null(),
        };
        let mut user_id = b64_bytes(&opts["user"]["id"])?;
        let (name, display) = (s(&opts["user"]["name"]), s(&opts["user"]["displayName"]));
        let user = WEBAUTHN_USER_ENTITY_INFORMATION {
            dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
            cbId: user_id.len() as u32,
            pbId: user_id.as_mut_ptr(),
            pwszName: PCWSTR(name.as_ptr()),
            pwszIcon: PCWSTR::null(),
            pwszDisplayName: PCWSTR(display.as_ptr()),
        };
        let mut params: Vec<WEBAUTHN_COSE_CREDENTIAL_PARAMETER> = opts["pubKeyCredParams"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some(WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
                    dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
                    pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
                    lAlg: p["alg"].as_i64()? as i32,
                })
            })
            .collect();
        let params = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS {
            cCredentialParameters: params.len() as u32,
            pCredentialParameters: params.as_mut_ptr(),
        };
        let client_data = super::client_data(
            "webauthn.create",
            opts["challenge"].as_str().unwrap_or_default(),
            origin,
        );
        let mut cd = client_data.clone().into_bytes();
        let cdata = WEBAUTHN_CLIENT_DATA {
            dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
            cbClientDataJSON: cd.len() as u32,
            pbClientDataJSON: cd.as_mut_ptr(),
            pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
        };
        let mut ids: Vec<Vec<u8>> = opts["excludeCredentials"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| b64_bytes(&c["id"]).ok())
            .collect();
        let mut exclude = credentials(&mut ids);
        let sel = &opts["authenticatorSelection"];
        let mut cancel_id = cancellation(cancel);
        let o = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS {
            dwVersion: 3,
            dwTimeoutMilliseconds: opts["timeout"].as_u64().unwrap_or(60_000).max(30_000) as u32,
            CredentialList: WEBAUTHN_CREDENTIALS {
                cCredentials: exclude.len() as u32,
                pCredentials: exclude.as_mut_ptr(),
            },
            dwAuthenticatorAttachment: match sel["authenticatorAttachment"].as_str() {
                Some("platform") => WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM,
                Some("cross-platform") => WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM,
                _ => WEBAUTHN_AUTHENTICATOR_ATTACHMENT_ANY,
            },
            bRequireResidentKey: (sel["residentKey"] == "required"
                || sel["requireResidentKey"] == true)
                .into(),
            dwUserVerificationRequirement: uv(sel["userVerification"].as_str()),
            dwAttestationConveyancePreference: match opts["attestation"].as_str() {
                Some("direct") | Some("enterprise") => {
                    WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_DIRECT
                }
                Some("indirect") => WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_INDIRECT,
                _ => WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE,
            },
            pCancellationId: cancel_id
                .as_mut()
                .map_or(std::ptr::null_mut(), |g| g as *mut GUID),
            ..Default::default()
        };
        status(Status::Touch);
        let a = unsafe {
            WebAuthNAuthenticatorMakeCredential(
                GetForegroundWindow(),
                &rp,
                &user,
                &params,
                &cdata,
                Some(&o),
            )
        }
        .map_err(describe)?;
        let out = unsafe {
            let r = &*a;
            let transport = if r.dwVersion >= 3 {
                r.dwUsedTransport
            } else {
                0
            };
            let v = super::registration_json(
                bytes(r.pbCredentialId, r.cbCredentialId),
                &client_data,
                bytes(r.pbAttestationObject, r.cbAttestationObject),
                transport,
            );
            WebAuthNFreeCredentialAttestation(Some(a));
            v
        };
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_matches_rp_id() {
        let mut d = InstanceDiscovery::default();
        d.endpoints.webapp = "https://fluxer.com".into();
        d.endpoints.api_client = "https://web.fluxer.app/api".into();
        assert_eq!(origin_for(&d, "fluxer.app"), "https://web.fluxer.app");
        assert_eq!(origin_for(&d, "fluxer.com"), "https://fluxer.com");
        assert_eq!(origin_for(&d, "chat.example.org"), "https://fluxer.com");
    }

    #[test]
    fn client_data_is_browser_shaped() {
        let v: serde_json::Value =
            serde_json::from_str(&client_data_json("abc", "https://web.fluxer.app")).unwrap();
        assert_eq!(v["type"], "webauthn.get");
        assert_eq!(v["challenge"], "abc");
        assert_eq!(v["origin"], "https://web.fluxer.app");
        assert_eq!(v["crossOrigin"], false);
    }

    #[test]
    fn transport_maps_to_attachment() {
        assert_eq!(transports(16 | 1), ["usb", "internal"]);
        assert_eq!(attachment(16), "platform");
        assert_eq!(attachment(32), "cross-platform");
        assert_eq!(attachment(0), "cross-platform");
        let r = registration_json(b"id", "{}", b"att", 16);
        assert_eq!(r["response"]["transports"], json!(["internal"]));
        assert_eq!(r["authenticatorAttachment"], "platform");
        let a = assertion_json(b"id", "{}", b"ad", b"sig", None, 1);
        assert!(a["response"]["userHandle"].is_null());
        assert_eq!(a["rawId"], B64.encode(b"id"));
    }
}
