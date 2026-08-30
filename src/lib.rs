//! Shared deterministic command/state semantics for DS native and web clients.
//!
//! This crate owns no transport, credential store, browser API, renderer, or
//! shell behavior. Its first bounded slice validates a Firebase login response
//! identically for the native `ds` client and the browser WASM adapter.

use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroize;

pub const LOGIN_CONTRACT_SCHEMA: &str = "ds.auth.login-result/v1";
pub const MAX_LOGIN_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginErrorKind {
    InvalidInput,
    UnreadableResponse,
    IdentityMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginError {
    kind: LoginErrorKind,
    message: &'static str,
}

impl LoginError {
    const fn new(kind: LoginErrorKind, message: &'static str) -> Self {
        Self { kind, message }
    }

    pub const fn kind(&self) -> LoginErrorKind {
        self.kind
    }
}

impl fmt::Display for LoginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for LoginError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginPrincipal {
    uid: String,
    email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    photo_url: Option<String>,
}

impl LoginPrincipal {
    pub fn uid(&self) -> &str {
        &self.uid
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    pub fn photo_url(&self) -> Option<&str> {
        self.photo_url.as_deref()
    }
}

/// A validated login response. Token fields remain private and are zeroized on
/// drop. Hosts may borrow or take them only to install their protected session.
#[derive(Debug)]
pub struct ValidatedLogin {
    principal: LoginPrincipal,
    id_token: String,
    refresh_token: String,
    expires_at_epoch_seconds: u64,
}

impl ValidatedLogin {
    pub fn principal(&self) -> &LoginPrincipal {
        &self.principal
    }

    pub fn id_token(&self) -> &str {
        &self.id_token
    }

    pub fn refresh_token(&self) -> &str {
        &self.refresh_token
    }

    pub const fn expires_at_epoch_seconds(&self) -> u64 {
        self.expires_at_epoch_seconds
    }

    pub fn take_id_token(&mut self) -> String {
        std::mem::take(&mut self.id_token)
    }

    pub fn take_refresh_token(&mut self) -> String {
        std::mem::take(&mut self.refresh_token)
    }

    /// Bounded browser handoff. It is deliberately not a command receipt and
    /// must never be traced or logged because it contains credential material.
    pub fn to_browser_json(&self) -> Result<String, LoginError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct BrowserLogin<'a> {
            schema: &'static str,
            user: &'a LoginPrincipal,
            id_token: &'a str,
            refresh_token: &'a str,
            expires_at_epoch_seconds: u64,
        }

        serde_json::to_string(&BrowserLogin {
            schema: LOGIN_CONTRACT_SCHEMA,
            user: &self.principal,
            id_token: &self.id_token,
            refresh_token: &self.refresh_token,
            expires_at_epoch_seconds: self.expires_at_epoch_seconds,
        })
        .map_err(|_| unreadable_response())
    }
}

impl Drop for ValidatedLogin {
    fn drop(&mut self) {
        self.id_token.zeroize();
        self.refresh_token.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirebaseLoginWire {
    local_id: String,
    email: String,
    id_token: String,
    refresh_token: String,
    expires_in: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    photo_url: String,
    #[serde(default)]
    provider_user_info: Vec<ProviderUserInfo>,
    #[serde(default)]
    raw_user_info: String,
}

impl Drop for FirebaseLoginWire {
    fn drop(&mut self) {
        self.id_token.zeroize();
        self.refresh_token.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderUserInfo {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    photo_url: String,
}

pub fn validate_firebase_login_response(
    response: &[u8],
    expected_email: Option<&str>,
    firebase_project_id: &str,
    now_epoch_seconds: u64,
) -> Result<ValidatedLogin, LoginError> {
    validate_audience(firebase_project_id)?;
    if response.len() > MAX_LOGIN_RESPONSE_BYTES {
        return Err(unreadable_response());
    }
    let mut wire: FirebaseLoginWire =
        serde_json::from_slice(response).map_err(|_| unreadable_response())?;
    let principal = validate_identity(
        &wire.local_id,
        &wire.email,
        expected_email.unwrap_or(&wire.email),
        first_nonempty([
            wire.display_name.as_str(),
            wire.provider_user_info
                .iter()
                .find_map(|provider| clean_optional(&provider.display_name))
                .unwrap_or_default(),
            raw_user_info_field(&wire.raw_user_info, &["name", "displayName"])
                .as_deref()
                .unwrap_or_default(),
        ]),
        first_nonempty([
            wire.photo_url.as_str(),
            wire.provider_user_info
                .iter()
                .find_map(|provider| clean_optional(&provider.photo_url))
                .unwrap_or_default(),
            raw_user_info_field(&wire.raw_user_info, &["picture", "photoUrl", "photoURL"])
                .as_deref()
                .unwrap_or_default(),
        ]),
    )?;
    let claim_expiry = validate_firebase_id_token(
        &wire.id_token,
        principal.uid(),
        principal.email(),
        firebase_project_id,
        now_epoch_seconds,
    )?;
    validate_secret(&wire.refresh_token)?;
    let expires_in = parse_expiry(&wire.expires_in)?;
    Ok(ValidatedLogin {
        principal,
        id_token: std::mem::take(&mut wire.id_token),
        refresh_token: std::mem::take(&mut wire.refresh_token),
        expires_at_epoch_seconds: claim_expiry.min(now_epoch_seconds.saturating_add(expires_in)),
    })
}

/// Validate the identity-binding claims on an ID token obtained directly from
/// Firebase's fixed authentication endpoint. The caller remains responsible
/// for cryptographic verification at the governing API boundary.
pub fn validate_firebase_id_token(
    token: &str,
    uid: &str,
    email: &str,
    firebase_project_id: &str,
    now_epoch_seconds: u64,
) -> Result<u64, LoginError> {
    validate_audience(firebase_project_id)?;
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(identity_mismatch());
    };
    if header.is_empty() || payload.is_empty() || signature.is_empty() {
        return Err(identity_mismatch());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| identity_mismatch())?;
    let claims: Value = serde_json::from_slice(&bytes).map_err(|_| identity_mismatch())?;
    let subject = exact_claim(&claims, "sub")?;
    let claim_email = exact_claim(&claims, "email")?;
    let claim_audience = exact_claim(&claims, "aud")?;
    let issuer = exact_claim(&claims, "iss")?;
    let exp = claims
        .get("exp")
        .and_then(Value::as_u64)
        .filter(|exp| *exp > now_epoch_seconds)
        .ok_or_else(identity_mismatch)?;
    let expected_issuer = format!("https://securetoken.google.com/{firebase_project_id}");
    if subject != uid
        || claim_email != email
        || claim_audience != firebase_project_id
        || issuer != expected_issuer
    {
        return Err(identity_mismatch());
    }
    Ok(exp)
}

fn validate_identity(
    uid: &str,
    response_email: &str,
    expected_email: &str,
    display_name: Option<String>,
    photo_url: Option<String>,
) -> Result<LoginPrincipal, LoginError> {
    if uid.is_empty()
        || uid.trim() != uid
        || uid.chars().count() > 128
        || uid.chars().any(char::is_control)
        || response_email.trim() != response_email
        || !response_email.eq_ignore_ascii_case(expected_email)
        || !valid_email(response_email)
    {
        return Err(identity_mismatch());
    }
    Ok(LoginPrincipal {
        uid: uid.to_owned(),
        email: response_email.to_owned(),
        display_name,
        photo_url,
    })
}

fn valid_email(email: &str) -> bool {
    !email.is_empty()
        && email.trim() == email
        && email.chars().count() <= 320
        && !email.chars().any(char::is_control)
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.is_empty()
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        })
}

fn validate_audience(audience: &str) -> Result<(), LoginError> {
    if audience.is_empty()
        || audience.trim() != audience
        || audience.len() > 128
        || audience.chars().any(char::is_control)
    {
        return Err(LoginError::new(
            LoginErrorKind::InvalidInput,
            "Firebase project identity is outside the closed login contract",
        ));
    }
    Ok(())
}

fn validate_secret(secret: &str) -> Result<(), LoginError> {
    if secret.is_empty() || secret.len() > 16 * 1024 || secret.contains(['\r', '\n']) {
        return Err(unreadable_response());
    }
    Ok(())
}

fn parse_expiry(value: &str) -> Result<u64, LoginError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|seconds| (60..=86_400).contains(seconds))
        .ok_or_else(unreadable_response)
}

fn exact_claim<'a>(claims: &'a Value, key: &str) -> Result<&'a str, LoginError> {
    claims
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.trim() == *value)
        .ok_or_else(identity_mismatch)
}

fn first_nonempty<const N: usize>(values: [&str; N]) -> Option<String> {
    values
        .into_iter()
        .find_map(clean_optional)
        .map(str::to_owned)
}

fn clean_optional(value: &str) -> Option<&str> {
    (!value.is_empty() && value.trim() == value && !value.chars().any(char::is_control))
        .then_some(value)
}

fn raw_user_info_field(raw: &str, keys: &[&str]) -> Option<String> {
    if raw.is_empty() || raw.len() > 16 * 1024 {
        return None;
    }
    let value: Value = serde_json::from_str(raw).ok()?;
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .and_then(clean_optional)
            .map(str::to_owned)
    })
}

fn unreadable_response() -> LoginError {
    LoginError::new(
        LoginErrorKind::UnreadableResponse,
        "the Firebase response is outside the closed login contract",
    )
}

fn identity_mismatch() -> LoginError {
    LoginError::new(
        LoginErrorKind::IdentityMismatch,
        "Firebase UID, email, JWT subject, audience, issuer, or expiry did not remain bound",
    )
}

#[cfg(feature = "wasm")]
mod wasm {
    use wasm_bindgen::prelude::*;

    use super::{validate_firebase_id_token, validate_firebase_login_response};

    #[wasm_bindgen(js_name = validateFirebaseLogin)]
    pub fn validate_firebase_login(
        response_json: &[u8],
        expected_email: Option<String>,
        firebase_project_id: &str,
        now_epoch_seconds: u64,
    ) -> Result<String, JsValue> {
        validate_firebase_login_response(
            response_json,
            expected_email.as_deref(),
            firebase_project_id,
            now_epoch_seconds,
        )
        .and_then(|login| login.to_browser_json())
        .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = validateFirebaseIdToken)]
    pub fn validate_firebase_token(
        token: &str,
        uid: &str,
        email: &str,
        firebase_project_id: &str,
        now_epoch_seconds: u64,
    ) -> Result<u64, JsValue> {
        validate_firebase_id_token(token, uid, email, firebase_project_id, now_epoch_seconds)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_700_000_000;
    const PROJECT: &str = "ds-testing";
    const UID: &str = "Case-Sensitive UID/01";
    const EMAIL: &str = "operator@example.com";

    fn token(uid: &str, email: &str, audience: &str, issuer: &str, exp: u64) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "sub": uid,
                "email": email,
                "aud": audience,
                "iss": issuer,
                "exp": exp,
            }))
            .unwrap(),
        );
        format!("{header}.{claims}.signature")
    }

    fn response(overrides: Value) -> Vec<u8> {
        let mut value = serde_json::json!({
            "localId": UID,
            "email": EMAIL,
            "idToken": token(
                UID,
                EMAIL,
                PROJECT,
                &format!("https://securetoken.google.com/{PROJECT}"),
                NOW + 3_600,
            ),
            "refreshToken": "protected-refresh",
            "expiresIn": "3600",
            "providerUserInfo": [{
                "displayName": "DS Operator",
                "photoUrl": "https://images.example/operator.png"
            }]
        });
        if let (Some(target), Some(source)) = (value.as_object_mut(), overrides.as_object()) {
            target.extend(source.clone());
        }
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn validates_one_bounded_login_for_native_and_browser_projection() {
        let login =
            validate_firebase_login_response(&response(Value::Null), Some(EMAIL), PROJECT, NOW)
                .unwrap();
        assert_eq!(login.principal().uid(), UID);
        assert_eq!(login.principal().display_name(), Some("DS Operator"));
        assert_eq!(login.expires_at_epoch_seconds(), NOW + 3_600);
        let browser: Value = serde_json::from_str(&login.to_browser_json().unwrap()).unwrap();
        assert_eq!(browser["schema"], LOGIN_CONTRACT_SCHEMA);
        assert_eq!(browser["user"]["uid"], UID);
        assert_eq!(browser["refreshToken"], "protected-refresh");
    }

    #[test]
    fn fails_closed_on_identity_audience_issuer_and_expiry_mismatch() {
        let cases = [
            token(
                "another-uid",
                EMAIL,
                PROJECT,
                &format!("https://securetoken.google.com/{PROJECT}"),
                NOW + 3_600,
            ),
            token(
                UID,
                "other@example.com",
                PROJECT,
                &format!("https://securetoken.google.com/{PROJECT}"),
                NOW + 3_600,
            ),
            token(
                UID,
                EMAIL,
                "other-project",
                &format!("https://securetoken.google.com/{PROJECT}"),
                NOW + 3_600,
            ),
            token(
                UID,
                EMAIL,
                PROJECT,
                "https://attacker.example/ds-testing",
                NOW + 3_600,
            ),
            token(
                UID,
                EMAIL,
                PROJECT,
                &format!("https://securetoken.google.com/{PROJECT}"),
                NOW,
            ),
        ];
        for id_token in cases {
            let error = validate_firebase_login_response(
                &response(serde_json::json!({ "idToken": id_token })),
                Some(EMAIL),
                PROJECT,
                NOW,
            )
            .unwrap_err();
            assert_eq!(error.kind(), LoginErrorKind::IdentityMismatch);
        }
    }

    #[test]
    fn rejects_unbounded_or_malformed_login_envelopes() {
        assert_eq!(
            validate_firebase_login_response(
                &vec![b'x'; MAX_LOGIN_RESPONSE_BYTES + 1],
                Some(EMAIL),
                PROJECT,
                NOW,
            )
            .unwrap_err()
            .kind(),
            LoginErrorKind::UnreadableResponse,
        );
        assert_eq!(
            validate_firebase_login_response(b"{}", Some(EMAIL), PROJECT, NOW)
                .unwrap_err()
                .kind(),
            LoginErrorKind::UnreadableResponse,
        );
    }
}

#[cfg(all(test, target_arch = "wasm32", feature = "wasm"))]
mod wasm_tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use serde_json::Value;
    use wasm_bindgen_test::*;

    use super::wasm::validate_firebase_login;

    #[wasm_bindgen_test]
    fn browser_export_uses_the_native_login_contract() {
        let now = 1_700_000_000_u64;
        let project = "ds-testing";
        let email = "operator@example.com";
        let uid = "uid-1";
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#);
        let claims = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "sub": uid,
                "email": email,
                "aud": project,
                "iss": format!("https://securetoken.google.com/{project}"),
                "exp": now + 3600,
            }))
            .unwrap(),
        );
        let response = serde_json::to_vec(&serde_json::json!({
            "localId": uid,
            "email": email,
            "idToken": format!("{header}.{claims}.signature"),
            "refreshToken": "refresh",
            "expiresIn": "3600",
        }))
        .unwrap();
        let result =
            validate_firebase_login(&response, Some(email.to_owned()), project, now).unwrap();
        let result: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["user"]["uid"], uid);
    }
}
