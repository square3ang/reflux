//! Authentication requests/responses, errors and CAPTCHA (`http-api/authentication.mdx`,
//! `http-api/index.md`, `topics/captcha.md`).

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, de::Error as _};

use crate::{PartialUser, Snowflake, Timestamp};

model! {
    /// `POST /auth/login`. Send `login` (email or username) or legacy `email`.
    pub struct LoginRequest {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub login: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub email: Option<String>,
        pub password: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub invite_code: Option<String>,
    }

    /// `POST /auth/register`. A body with only `global_name` creates an instant account.
    pub struct RegisterRequest {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub email: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub username: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub global_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub password: Option<String>,
        /// `YYYY-MM-DD`.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub date_of_birth: Option<String>,
        #[serde(skip_serializing_if = "crate::is_false")]
        pub consent: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub invite_code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub registration_url_code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub theme: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub setup_token: Option<String>,
    }

    /// `POST /auth/login/mfa/totp` (TOTP or backup code).
    pub struct TotpLoginRequest {
        pub code: String,
        pub ticket: String,
    }

    /// `POST /auth/login/mfa/webauthn/authentication-options`.
    pub struct MfaTicket {
        pub ticket: String,
    }

    /// `POST /auth/login/mfa/webauthn` (with `ticket`) and `/auth/webauthn/authenticate` (without).
    pub struct WebAuthnLoginRequest {
        /// Browser `PublicKeyCredential` JSON (WebAuthn assertion object).
        pub response: serde_json::Value,
        pub challenge: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub ticket: Option<String>,
    }

    /// Options for `navigator.credentials.get()`; kept loosely typed for the platform authenticator.
    pub struct WebAuthnAuthenticationOptions {
        pub challenge: String,
        pub timeout: u32,
        #[serde(rename = "rpId")]
        pub rp_id: String,
        #[serde(rename = "allowCredentials")]
        pub allow_credentials: Vec<serde_json::Value>,
        #[serde(rename = "userVerification")]
        pub user_verification: String,
    }

    /// `POST /auth/identify` (identifier-first sign-in).
    pub struct IdentifyAccountRequest {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub email: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub login: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub device_proof: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub registration_url_code: Option<String>,
    }

    pub struct IdentifyAccountResponse {
        pub ticket: String,
        /// `existing` or `new`.
        pub account: String,
        /// `password` or `code`.
        pub sign_in_method: Option<String>,
        pub registration: Option<IdentifyRegistration>,
    }

    pub struct IdentifyRegistration {
        pub mode: String,
        pub consent_required: bool,
        pub requires_password: bool,
        pub instant_account_availability: Option<String>,
    }

    /// `POST /auth/email-code/send` response.
    pub struct EmailCodeSent {
        /// `sign_in` or `sign_up`.
        pub purpose: String,
        pub code_expires_at: Timestamp,
        pub resend_available_at: Timestamp,
    }

    /// `POST /auth/email-code/verify`.
    pub struct VerifyEmailCodeRequest {
        pub ticket: String,
        pub code: String,
        #[serde(skip_serializing_if = "crate::is_false")]
        pub consent: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub registration_url_code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub invite_code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub username: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub global_name: Option<String>,
    }

    /// Authentication token response.
    pub struct AuthToken {
        pub token: String,
        pub user_id: Snowflake,
        pub user: PartialUser,
        pub device_proof: Option<String>,
        /// Guild entry to open after registration (shape in `users/current-user.mdx`).
        pub entry: Option<serde_json::Value>,
        /// SSO completion only.
        pub redirect_to: Option<String>,
    }

    /// MFA challenge response.
    pub struct MfaChallenge {
        pub mfa: bool,
        pub ticket: String,
        /// From `totp`, `webauthn`, `backup_codes`.
        pub allowed_methods: Vec<String>,
        pub totp: bool,
        pub webauthn: bool,
        pub backup_codes: bool,
    }

    pub struct RegistrationPending {
        pub registration_pending_approval: bool,
        pub user_id: Snowflake,
    }

    /// `GET /auth/ip-authorization/poll`-style state of an IP authorization wait.
    pub struct IpAuthorizationPoll {
        pub completed: bool,
        pub token: Option<String>,
        pub user_id: Option<Snowflake>,
        pub user: Option<PartialUser>,
    }

    /// SSO status (also in instance discovery).
    pub struct SsoStatus {
        pub enabled: bool,
        pub enforced: bool,
        pub username_from_provider: bool,
        pub display_name: Option<String>,
        pub button_label: Option<String>,
        pub redirect_uri: String,
    }

    pub struct SsoStart {
        pub authorization_url: String,
        pub state: String,
        pub redirect_uri: String,
        pub client_binding: String,
    }

    pub struct AuthSession {
        pub id_hash: String,
        pub client_info: Option<ClientInfo>,
        pub masked_ip: Option<String>,
        pub approx_last_used_at: Option<Timestamp>,
        pub current: bool,
    }

    pub struct ClientInfo {
        pub platform: Option<String>,
        pub os: Option<String>,
        pub browser: Option<String>,
        pub device: String,
        pub location: Option<ClientLocation>,
    }

    pub struct ClientLocation {
        pub city: Option<String>,
        pub region: Option<String>,
        pub country: Option<String>,
    }

    /// Error response. Extra fields by code (captcha, IP authorization `ticket`, rate limit
    /// `retry_after`, account tier `action`/`tier`/`need`, enforcement `notice`, ...) are typed
    /// where common and otherwise kept in `extra`.
    pub struct ApiError {
        pub code: String,
        pub message: String,
        pub errors: Vec<ValidationError>,
        /// `altcha` on `CAPTCHA_REQUIRED` / `INVALID_CAPTCHA`.
        pub captcha_provider: Option<String>,
        /// ALTCHA v2 challenge `{parameters, signature}`. Copy unchanged into the token.
        pub altcha_challenge: Option<AltchaChallenge>,
        /// `IP_AUTHORIZATION_REQUIRED` ticket.
        pub ticket: Option<String>,
        pub retry_after: Option<f64>,
        #[serde(flatten)]
        pub extra: HashMap<String, serde_json::Value>,
    }

    pub struct ValidationError {
        pub path: String,
        pub code: Option<String>,
        pub message: String,
    }

    // shortcut: `parameters` is a serde_json::Value (sorted keys without `preserve_order`); fine
    // if the server re-canonicalizes, otherwise switch to RawValue when the solver is built.
    pub struct AltchaChallenge {
        pub parameters: serde_json::Value,
        pub signature: String,
    }

    /// `X-Captcha-Token` payload before base64.
    pub struct AltchaToken {
        pub challenge: AltchaChallenge,
        pub solution: AltchaSolution,
    }

    pub struct AltchaSolution {
        pub counter: u64,
        #[serde(rename = "derivedKey")]
        pub derived_key: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub time: Option<f64>,
    }
}

/// Response of login, register, email-code verify and SSO completion.
#[derive(Clone, Debug, PartialEq)]
pub enum AuthResponse {
    Token(AuthToken),
    Mfa(MfaChallenge),
    PendingApproval(RegistrationPending),
}

impl<'de> Deserialize<'de> for AuthResponse {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Every field is defaulted, so discriminate on the marker fields instead of `untagged`.
        let v = serde_json::Value::deserialize(d)?;
        let flag = |k: &str| v.get(k).and_then(|x| x.as_bool()) == Some(true);
        let r = if flag("mfa") {
            serde_json::from_value(v).map(Self::Mfa)
        } else if flag("registration_pending_approval") {
            serde_json::from_value(v).map(Self::PendingApproval)
        } else if v.get("token").is_some_and(|t| t.is_string()) {
            serde_json::from_value(v).map(Self::Token)
        } else {
            return Err(D::Error::custom("auth response has no token, mfa or approval marker"));
        };
        r.map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_response_variants() {
        let t: AuthResponse =
            serde_json::from_str(r#"{"token":"abc","user_id":"5","user":{"id":"5","username":"me"}}"#)
                .unwrap();
        assert!(matches!(t, AuthResponse::Token(ref a) if a.token == "abc"));
        let m: AuthResponse = serde_json::from_str(
            r#"{"mfa":true,"ticket":"t","allowed_methods":["totp","webauthn"],"totp":true,"webauthn":true,"backup_codes":false}"#,
        )
        .unwrap();
        assert!(matches!(m, AuthResponse::Mfa(ref c) if c.ticket == "t" && c.webauthn));
        let p: AuthResponse =
            serde_json::from_str(r#"{"registration_pending_approval":true,"user_id":"9"}"#).unwrap();
        assert!(matches!(p, AuthResponse::PendingApproval(_)));
        assert!(serde_json::from_str::<AuthResponse>("{}").is_err());
    }

    #[test]
    fn captcha_error() {
        let e: ApiError = serde_json::from_str(
            r#"{"code":"CAPTCHA_REQUIRED","message":"x","captcha_provider":"altcha",
            "altcha_challenge":{"parameters":{"algorithm":"PBKDF2/SHA-256","cost":5000},"signature":"sig"},
            "action":"message"}"#,
        )
        .unwrap();
        assert_eq!(e.altcha_challenge.unwrap().signature, "sig");
        assert_eq!(e.extra["action"], "message");
    }

    #[test]
    fn validation_error_example() {
        let e: ApiError = serde_json::from_str(
            r#"{"code":"INVALID_FORM_BODY","message":"Invalid form body.","errors":[
            {"path":"name","code":"INVALID_FORMAT","message":"The provided value is in an invalid format."}]}"#,
        )
        .unwrap();
        assert_eq!(e.errors[0].path, "name");
    }
}
