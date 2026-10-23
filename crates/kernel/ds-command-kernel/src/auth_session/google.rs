//! Google sign-in by a plain browser redirect, decided here: no third-party
//! script runs in the page. The Kernel mints the state and the nonce and keeps
//! them in the profile's store; the page only navigates to [`begin`]'s URL.
//! Google returns to the callback route with the ID token in the URL fragment
//! (OpenID Connect implicit flow, `response_type=id_token`), which the page
//! never parses: the Kernel reads it, checks it against the pending redirect
//! ([`verify`]) and exchanges it with Firebase (`signInWithIdp`), which
//! verifies Google's signature.
//!
//! The OAuth client id is a constant of the build (`DS_GOOGLE_CLIENT_ID`,
//! from the project's `GOOGLE_OAUTH_CLIENT_ID`). The callback is
//! `<page origin><mount>/auth/google`, which must be an Authorized redirect URI
//! of that client.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gateway_exchange::firebase_auth::AuthRefusal;

pub const AUTHORIZATION_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
/// The callback route below the page's mount point.
pub const CALLBACK_ROUTE: &str = "/auth/google";
/// How long a redirect may take before its callback is refused.
pub const REDIRECT_TTL_SECONDS: u64 = 600;
const ISSUERS: [&str; 2] = ["accounts.google.com", "https://accounts.google.com"];
const MAX_PATH: usize = 2048;
const MAX_FRAGMENT: usize = 16 * 1024;

pub const UNCONFIGURED: &str = "google_sign_in_unconfigured";
pub const INVALID: &str = "google_sign_in_invalid";
pub const CANCELLED: &str = "google_sign_in_cancelled";
pub const FAILED: &str = "google_sign_in_failed";
pub const NOT_PENDING: &str = "google_sign_in_not_pending";
pub const STATE_MISMATCH: &str = "google_sign_in_state_mismatch";
pub const NONCE_MISMATCH: &str = "google_sign_in_nonce_mismatch";
pub const EXPIRED: &str = "google_sign_in_expired";
pub const UNAVAILABLE: &str = "google_sign_in_unavailable";

/// One pending redirect, held in the profile's store until its callback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redirect {
    pub state: String,
    pub nonce: String,
    pub redirect_uri: String,
    /// The page path the person returns to: same-origin, never a URL.
    pub return_path: String,
    pub created_at_epoch_seconds: u64,
}

impl Redirect {
    pub fn validate(&self) -> Result<(), String> {
        if !super::hex32(&self.state) || !super::hex32(&self.nonce) {
            return Err(format!(
                "{}: a redirect's state and nonce are 32 lowercase hex digits",
                super::INVALID
            ));
        }
        if self.redirect_uri.len() > MAX_PATH || return_path(&self.return_path).is_err() {
            return Err(format!(
                "{}: a redirect's addresses exceed their bounds",
                super::INVALID
            ));
        }
        Ok(())
    }
}

fn refusal(code: &str, message: &str) -> AuthRefusal {
    AuthRefusal {
        code: code.into(),
        message: message.into(),
    }
}

fn unconfigured() -> AuthRefusal {
    refusal(
        UNCONFIGURED,
        "Google sign-in is not configured for this build.",
    )
}

/// The page's own origin: `https`, or `http` on a loopback host only.
fn origin(origin: &str) -> Result<url::Url, AuthRefusal> {
    let invalid = || {
        refusal(
            INVALID,
            "Google sign-in needs this page's own https origin.",
        )
    };
    let parsed = url::Url::parse(origin).map_err(|_| invalid())?;
    let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    let exact = parsed.path() == "/"
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none();
    if !exact || !(parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback)) {
        return Err(invalid());
    }
    Ok(parsed)
}

/// A same-origin page path: absolute, never protocol-relative, no control
/// characters or backslashes, bounded.
fn return_path(path: &str) -> Result<&str, AuthRefusal> {
    let path = if path.is_empty() { "/" } else { path };
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.len() > MAX_PATH
        || path.chars().any(|c| c.is_control() || c == '\\')
    {
        return Err(refusal(
            INVALID,
            "Google sign-in returns only to a page of this site.",
        ));
    }
    Ok(path)
}

/// The callback path below the page's mount: ends with [`CALLBACK_ROUTE`].
fn callback_path(path: &str) -> Result<&str, AuthRefusal> {
    let valid = path.starts_with('/')
        && !path.starts_with("//")
        && path.ends_with(CALLBACK_ROUTE)
        && path.len() <= MAX_PATH
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        && !path.split('/').any(|segment| segment == "..");
    if valid {
        Ok(path)
    } else {
        Err(refusal(
            INVALID,
            "Google sign-in returns only to this site's sign-in route.",
        ))
    }
}

/// The pending redirect and the authorization URL the page navigates to.
pub fn begin(
    client_id: Option<&str>,
    page_origin: &str,
    callback: &str,
    returning: &str,
    state: &str,
    nonce: &str,
    now: u64,
) -> Result<(Redirect, String), AuthRefusal> {
    let client_id = client_id
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(unconfigured)?;
    let redirect_uri = origin(page_origin)?
        .join(callback_path(callback)?)
        .map_err(|_| {
            refusal(
                INVALID,
                "Google sign-in needs this page's own https origin.",
            )
        })?
        .to_string();
    let redirect = Redirect {
        state: state.into(),
        nonce: nonce.into(),
        redirect_uri: redirect_uri.clone(),
        return_path: return_path(returning)?.into(),
        created_at_epoch_seconds: now,
    };
    redirect
        .validate()
        .map_err(|_| refusal(INVALID, "Google sign-in could not start."))?;
    let url = url::Url::parse_with_params(
        AUTHORIZATION_ENDPOINT,
        [
            ("client_id", client_id),
            ("redirect_uri", redirect_uri.as_str()),
            ("response_type", "id_token"),
            ("scope", "openid email profile"),
            ("state", state),
            ("nonce", nonce),
            ("prompt", "select_account"),
        ],
    )
    .map_err(|_| refusal(INVALID, "Google sign-in could not start."))?;
    Ok((redirect, url.to_string()))
}

/// What Google sent back in the fragment.
#[derive(Clone, PartialEq, Eq)]
pub struct Callback {
    pub state: String,
    pub outcome: Outcome,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Outcome {
    IdToken(String),
    /// Google's `error` code (`access_denied` when the person declined).
    Error(String),
}

impl std::fmt::Debug for Callback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let outcome = match &self.outcome {
            Outcome::IdToken(_) => "id_token",
            Outcome::Error(error) => error,
        };
        f.debug_struct("Callback")
            .field("state", &self.state)
            .field("outcome", &outcome)
            .finish()
    }
}

/// Parse the callback fragment (`#id_token=…&state=…`, or `#error=…`).
/// Unknown parameters Google adds (`authuser`, `prompt`, `hd`) are ignored.
pub fn parse_callback(fragment: &str) -> Result<Callback, AuthRefusal> {
    let invalid = || {
        refusal(
            INVALID,
            "This page completes a Google sign-in, and the answer it received is incomplete.",
        )
    };
    let fragment = fragment.strip_prefix('#').unwrap_or(fragment);
    if fragment.is_empty() || fragment.len() > MAX_FRAGMENT {
        return Err(invalid());
    }
    let (mut state, mut id_token, mut error) = (None, None, None);
    for (key, value) in url::form_urlencoded::parse(fragment.as_bytes()) {
        let slot = match key.as_ref() {
            "state" => &mut state,
            "id_token" => &mut id_token,
            "error" => &mut error,
            _ => continue,
        };
        if slot.replace(value.into_owned()).is_some() {
            return Err(invalid());
        }
    }
    let state = state
        .filter(|state| super::hex32(state))
        .ok_or_else(invalid)?;
    let outcome = match (id_token, error) {
        (Some(token), None) if !token.is_empty() => Outcome::IdToken(token),
        (None, Some(error)) if !error.is_empty() && error.len() <= 128 => Outcome::Error(error),
        _ => return Err(invalid()),
    };
    Ok(Callback { state, outcome })
}

/// What the store answered for the callback's state.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "found", rename_all = "snake_case")]
pub enum Taken {
    /// No redirect is pending: a replay, or a visit without a sign-in.
    None,
    /// Another redirect is pending: the callback is not this browser's.
    Mismatch,
    /// The pending redirect, removed in the same transaction.
    Taken { redirect: Redirect },
}

/// Check the callback against the redirect it answers and return the ID
/// token Firebase exchanges: state, age, Google's verdict, then the token's
/// nonce, audience, issuer and expiry. Firebase verifies the signature.
pub fn verify(
    taken: Taken,
    callback: Callback,
    client_id: Option<&str>,
    now: u64,
) -> Result<(String, Redirect), AuthRefusal> {
    let client_id = client_id
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(unconfigured)?;
    let pending = match taken {
        Taken::None => {
            return Err(refusal(
                NOT_PENDING,
                "No Google sign-in is in progress in this browser. Start it again.",
            ))
        }
        Taken::Mismatch => {
            return Err(refusal(
                STATE_MISMATCH,
                "This Google answer belongs to another sign-in. Start it again.",
            ))
        }
        Taken::Taken { redirect } => redirect,
    };
    if pending.state != callback.state {
        return Err(refusal(
            STATE_MISMATCH,
            "This Google answer belongs to another sign-in. Start it again.",
        ));
    }
    if now
        > pending
            .created_at_epoch_seconds
            .saturating_add(REDIRECT_TTL_SECONDS)
    {
        return Err(refusal(
            EXPIRED,
            "The Google sign-in took too long. Start it again.",
        ));
    }
    let token = match callback.outcome {
        Outcome::Error(error) if error == "access_denied" => {
            return Err(refusal(CANCELLED, "Google sign-in was cancelled"))
        }
        Outcome::Error(_) => return Err(refusal(FAILED, "Google sign-in failed.")),
        Outcome::IdToken(token) => token,
    };
    let claims = claims(&token).ok_or_else(|| {
        refusal(
            INVALID,
            "Google's answer did not carry a readable identity token.",
        )
    })?;
    let text = |key: &str| claims.get(key).and_then(Value::as_str);
    if text("nonce") != Some(pending.nonce.as_str()) {
        return Err(refusal(
            NONCE_MISMATCH,
            "This Google answer was not issued for this sign-in. Start it again.",
        ));
    }
    let audience_matches = match claims.get("aud") {
        Some(Value::String(audience)) => audience == client_id,
        Some(Value::Array(audiences)) => audiences.iter().any(|audience| audience == client_id),
        _ => false,
    };
    let issuer = text("iss").is_some_and(|issuer| ISSUERS.contains(&issuer));
    let current = claims
        .get("exp")
        .and_then(Value::as_u64)
        .is_some_and(|exp| exp > now);
    if !audience_matches || !issuer || !current {
        return Err(refusal(
            INVALID,
            "Google's answer was not issued for this site.",
        ));
    }
    Ok((token, pending))
}

/// The unverified claims of a compact JWT.
fn claims(token: &str) -> Option<serde_json::Map<String, Value>> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if header.is_empty() || signature.is_empty() || token.len() > MAX_FRAGMENT {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    match serde_json::from_slice(&bytes).ok()? {
        Value::Object(claims) => Some(claims),
        _ => None,
    }
}

/// The origin Firebase is told the answer came back to (`requestUri`).
pub fn request_uri(redirect: &Redirect) -> String {
    url::Url::parse(&redirect.redirect_uri)
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "google_tests.rs"]
mod tests;
