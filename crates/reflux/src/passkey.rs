//! Passkeys / security keys as a native WebAuthn client: Mozilla's `authenticator` (Firefox's
//! CTAP1/2 stack) talks to USB HID keys on Linux, Windows and macOS, and we build the
//! `clientDataJSON` a browser would. Fluxer checks its `origin` against the instance's allowed web
//! origins, which is what the official desktop app's native addon also fills in itself.
//! Sign-in and sudo use `get`, adding a passkey `create`.
//! shortcut: USB keys only — no platform authenticators (Windows Hello via webauthn.dll, macOS
//! ASAuthorization, which needs entitlements) and no hybrid/phone flow; add them per platform.

use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use authenticator::authenticatorservice::{AuthenticatorService, SignArgs};
use authenticator::ctap2::server::{PublicKeyCredentialDescriptor, UserVerificationRequirement};
use authenticator::statecallback::StateCallback;
use authenticator::{Pin, StatusPinUv, StatusUpdate};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use reflux_model::{InstanceDiscovery, WebAuthnAuthenticationOptions};
use serde_json::json;
use sha2::{Digest, Sha256};

/// What the key needs from the person, for the passkey screen.
pub enum Status {
    Touch,
    /// Several keys: touch the one to use.
    SelectDevice,
    /// The PIN (again, with `attempts` left, when the last one was wrong).
    Pin { reply: Sender<Pin>, wrong: bool, attempts: Option<u8> },
}

/// Cancels a running request (the screen was left).
#[derive(Clone)]
pub struct Cancel(Arc<Mutex<AuthenticatorService>>);

impl Cancel {
    pub fn cancel(&self) {
        if let Ok(mut m) = self.0.lock() {
            let _ = m.cancel();
        }
    }
}

/// The web origin a browser would report for `rp_id`: the instance web app, or its API client
/// host (fluxer.com's legacy RP `fluxer.app` lives on `web.fluxer.app`).
pub fn origin_for(d: &InstanceDiscovery, rp_id: &str) -> String {
    let origin = |u: &str| url::Url::parse(u).ok().map(|u| (u.origin().ascii_serialization(), u.host_str().unwrap_or("").to_owned()));
    let candidates: Vec<_> = [&d.endpoints.webapp, &d.endpoints.api_client].into_iter().filter_map(|u| origin(u)).collect();
    candidates
        .iter()
        .find(|(_, host)| host == rp_id || host.ends_with(&format!(".{rp_id}")))
        .or(candidates.first())
        .map(|(o, _)| o.clone())
        .unwrap_or_else(|| format!("https://{rp_id}"))
}

pub fn client_data_json(challenge: &str, origin: &str) -> String {
    json!({"type": "webauthn.get", "challenge": challenge, "origin": origin, "crossOrigin": false}).to_string()
}

/// Starts `navigator.credentials.get()` on the attached keys; blocks until done. `status` gets the
/// prompts; the result is the `PublicKeyCredential` JSON Fluxer takes as `response`.
pub fn get(
    opts: &WebAuthnAuthenticationOptions,
    origin: &str,
    status: impl Fn(Status) + Send + 'static,
    cancel: impl FnOnce(Cancel),
) -> Result<serde_json::Value, String> {
    let mut service = AuthenticatorService::new().map_err(|e| format!("{e:?}"))?;
    service.add_u2f_usb_hid_platform_transports();
    let service = Arc::new(Mutex::new(service));
    cancel(Cancel(service.clone()));

    let allow_list: Vec<PublicKeyCredentialDescriptor> = opts
        .allow_credentials
        .iter()
        .filter_map(|c| c.get("id")?.as_str().and_then(|id| B64.decode(id.trim_end_matches('=')).ok()))
        .map(|id| PublicKeyCredentialDescriptor { id, transports: vec![] })
        .collect();
    let client_data = client_data_json(&opts.challenge, origin);
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
    service.lock().map_err(|e| e.to_string())?.sign(timeout, args, status_tx, callback).map_err(|e| format!("{e:?}"))?;
    let result = rx.recv().map_err(|e| e.to_string())?.map_err(describe)?;

    let a = result.assertion;
    // CTAP2 may leave out the credential when the allow list had exactly one.
    let id = a.credentials.map(|c| c.id).or_else(|| (allow_list.len() == 1).then(|| allow_list[0].id.clone())).ok_or("The security key didn't name its credential.")?;
    let id = B64.encode(id);
    Ok(json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "response": {
            "clientDataJSON": B64.encode(client_data),
            "authenticatorData": B64.encode(a.auth_data.to_vec()),
            "signature": B64.encode(a.signature),
            "userHandle": a.user.map(|u| B64.encode(u.id)),
        },
        "clientExtensionResults": {},
        "authenticatorAttachment": "cross-platform",
    }))
}

/// `navigator.credentials.create()` (`performRegistration`) on the attached keys from Fluxer's
/// `PublicKeyCredentialCreationOptionsJSON`; blocks until done. The result is the
/// `RegistrationResponseJSON` `POST /users/@me/mfa/webauthn/credentials` takes.
pub fn create(
    opts: &serde_json::Value,
    origin: &str,
    status: impl Fn(Status) + Send + 'static,
    cancel: impl FnOnce(Cancel),
) -> Result<serde_json::Value, String> {
    use authenticator::authenticatorservice::RegisterArgs;
    use authenticator::ctap2::server::{PublicKeyCredentialParameters, PublicKeyCredentialUserEntity, RelyingParty, ResidentKeyRequirement};
    let s = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
    let bytes = |v: &serde_json::Value| B64.decode(v.as_str().unwrap_or_default().trim_end_matches('=')).map_err(|e| e.to_string());
    let rp_id = s(&opts["rp"]["id"]);
    let challenge = s(&opts["challenge"]);
    let client_data = json!({"type": "webauthn.create", "challenge": challenge, "origin": origin, "crossOrigin": false}).to_string();
    let sel = &opts["authenticatorSelection"];
    let mut service = AuthenticatorService::new().map_err(|e| format!("{e:?}"))?;
    service.add_u2f_usb_hid_platform_transports();
    let service = Arc::new(Mutex::new(service));
    cancel(Cancel(service.clone()));
    let args = RegisterArgs {
        client_data_hash: Sha256::digest(client_data.as_bytes()).into(),
        relying_party: RelyingParty { id: rp_id, name: opts["rp"]["name"].as_str().map(str::to_owned) },
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
            .filter_map(|p| PublicKeyCredentialParameters::try_from(p["alg"].as_i64()? as i32).ok())
            .collect(),
        exclude_list: opts["excludeCredentials"].as_array().into_iter().flatten().filter_map(|c| bytes(&c["id"]).ok()).map(|id| PublicKeyCredentialDescriptor { id, transports: vec![] }).collect(),
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
    service.lock().map_err(|e| e.to_string())?.register(timeout, args, status_tx, callback).map_err(|e| format!("{e:?}"))?;
    let made = rx.recv().map_err(|e| e.to_string())?.map_err(describe)?;
    let id = made.att_obj.auth_data.credential_data.as_ref().map(|c| B64.encode(&c.credential_id)).ok_or("The security key didn't return a credential.")?;
    let attestation = serde_cbor::to_vec(&made.att_obj).map_err(|e| e.to_string())?;
    Ok(json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "response": {
            "clientDataJSON": B64.encode(client_data),
            "attestationObject": B64.encode(attestation),
            "transports": ["usb"],
        },
        "clientExtensionResults": {},
        "authenticatorAttachment": "cross-platform",
    }))
}

/// Forwards the key's prompts to `status` on a thread of its own.
fn status_channel(status: impl Fn(Status) + Send + 'static) -> Sender<StatusUpdate> {
    let (status_tx, status_rx) = channel::<StatusUpdate>();
    std::thread::spawn(move || {
        while let Ok(up) = status_rx.recv() {
            match up {
                StatusUpdate::PresenceRequired => status(Status::Touch),
                StatusUpdate::SelectDeviceNotice => status(Status::SelectDevice),
                StatusUpdate::PinUvError(StatusPinUv::PinRequired(reply)) => status(Status::Pin { reply, wrong: false, attempts: None }),
                StatusUpdate::PinUvError(StatusPinUv::InvalidPin(reply, attempts)) => status(Status::Pin { reply, wrong: true, attempts }),
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
        E::PinError(P::PinBlocked) => "Your security key's PIN is blocked. Reset the key to use it again.".into(),
        E::PinError(P::PinAuthBlocked) => "Too many wrong PINs. Unplug the security key and plug it back in.".into(),
        E::PinError(P::PinNotSet) => "Set a PIN on your security key first.".into(),
        E::PinError(P::UvBlocked) => "Too many failed fingerprint attempts. Enter your PIN or unplug the key.".into(),
        other => format!("The security key request failed: {other}."),
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
        let v: serde_json::Value = serde_json::from_str(&client_data_json("abc", "https://web.fluxer.app")).unwrap();
        assert_eq!(v["type"], "webauthn.get");
        assert_eq!(v["challenge"], "abc");
        assert_eq!(v["origin"], "https://web.fluxer.app");
        assert_eq!(v["crossOrigin"], false);
    }
}
