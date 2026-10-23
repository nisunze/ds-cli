//! Firebase Authentication's REST exchanges for the page's sign-in: password
//! sign-in, account creation, Google sign-in, the password-reset mail and the
//! session refresh. The Firebase project and its public key are constants of
//! the build ([`crate::gateway_target`]); every session answer is validated by
//! [`crate::validate_firebase_login_response`] and bound by the Kernel's own
//! session ([`crate::auth_session`]), never handed to the page, and a refusal
//! carries Firebase's code and the sentence a person reads.

use serde::Serialize;
use serde_json::{json, Value};

pub const IDENTITY_TOOLKIT: &str = "https://identitytoolkit.googleapis.com/v1/accounts";
pub const SECURE_TOKEN: &str = "https://securetoken.googleapis.com/v1/token";
/// The address Firebase names in a sign-in-methods probe; never visited.
const CONTINUE_URI: &str = "https://datasolutions.rw/";
pub const UNCONFIGURED: &str = "firebase_unconfigured";
pub const NETWORK: &str = "network";
pub const INVALID_IDENTITY: &str = "invalid_identity";
const NETWORK_SENTENCE: &str = "Network error. Check your connection and try again.";
const INVALID_IDENTITY_SENTENCE: &str =
    "Authentication response did not include a valid user identity.";

/// One refused authentication call: Firebase's code (or `network`,
/// `invalid_identity`, `firebase_unconfigured`) and what a person reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthRefusal {
    pub code: String,
    pub message: String,
}

impl AuthRefusal {
    pub fn network() -> Self {
        Self::new(NETWORK, NETWORK_SENTENCE)
    }

    pub fn invalid_identity() -> Self {
        Self::new(INVALID_IDENTITY, INVALID_IDENTITY_SENTENCE)
    }

    pub fn unconfigured() -> Self {
        Self::new(
            UNCONFIGURED,
            "Firebase sign-in is not configured for this build.",
        )
    }

    pub fn oversized_input() -> Self {
        Self::new(
            "auth_input_too_large",
            "Authentication input exceeds its size limit.",
        )
    }

    fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }
}

/// The calls whose request bodies the page never builds.
pub enum Call<'a> {
    SignIn {
        email: &'a str,
        password: &'a str,
    },
    SignUp {
        email: &'a str,
        password: &'a str,
    },
    Google {
        id_token: &'a str,
        request_uri: &'a str,
    },
    PasswordReset {
        email: &'a str,
    },
    SignInMethods {
        email: &'a str,
    },
}

impl Call<'_> {
    pub fn validate(&self) -> Result<(), AuthRefusal> {
        let bytes = match self {
            Self::SignIn { email, password } | Self::SignUp { email, password } => {
                email.len().saturating_add(password.len())
            }
            Self::Google {
                id_token,
                request_uri,
            } => id_token.len().saturating_add(request_uri.len()),
            Self::PasswordReset { email } | Self::SignInMethods { email } => email.len(),
        };
        if bytes > crate::MAX_LOGIN_RESPONSE_BYTES {
            return Err(AuthRefusal::oversized_input());
        }
        Ok(())
    }
    /// The endpoint, before the build's key is appended.
    pub fn endpoint(&self) -> String {
        let method = match self {
            Self::SignIn { .. } => "signInWithPassword",
            Self::SignUp { .. } => "signUp",
            Self::Google { .. } => "signInWithIdp",
            Self::PasswordReset { .. } => "sendOobCode",
            Self::SignInMethods { .. } => "createAuthUri",
        };
        format!("{IDENTITY_TOOLKIT}:{method}")
    }

    pub fn body(&self) -> Value {
        match self {
            Self::SignIn { email, password } | Self::SignUp { email, password } => {
                json!({"email": email, "password": password, "returnSecureToken": true})
            }
            Self::Google {
                id_token,
                request_uri,
            } => json!({
                "postBody": url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("id_token", id_token)
                    .append_pair("providerId", "google.com")
                    .finish(),
                "requestUri": request_uri,
                "returnSecureToken": true,
            }),
            Self::PasswordReset { email } => {
                json!({"requestType": "PASSWORD_RESET", "email": email})
            }
            Self::SignInMethods { email } => {
                json!({"identifier": email, "continueUri": CONTINUE_URI})
            }
        }
    }

    /// The sentence a refusal falls back to when Firebase names nothing known.
    pub fn fallback(&self) -> &'static str {
        match self {
            Self::SignIn { .. } => "Login failed.",
            Self::SignUp { .. } => "Could not create the account.",
            Self::Google { .. } => "Google sign-in failed.",
            Self::PasswordReset { .. } => "Password reset failed.",
            Self::SignInMethods { .. } => "Sign-in methods are unavailable.",
        }
    }

    /// Whether this refusal makes "which methods does this account have?"
    /// the next question.
    pub fn asks_for_methods(&self, code: &str) -> bool {
        match self {
            Self::SignIn { .. } => matches!(
                code,
                "EMAIL_NOT_FOUND"
                    | "USER_NOT_FOUND"
                    | "INVALID_PASSWORD"
                    | "INVALID_LOGIN_CREDENTIALS"
                    | "INVALID_CREDENTIAL"
            ),
            Self::SignUp { .. } => code == "EMAIL_EXISTS",
            _ => false,
        }
    }
}

/// Firebase's error message, as given (`error.message`, or a bare `error`).
fn raw_error(body: &[u8]) -> Option<String> {
    if body.len() > crate::MAX_LOGIN_RESPONSE_BYTES {
        return None;
    }
    let value: Value = serde_json::from_slice(body).ok()?;
    let error = &value["error"];
    error["message"]
        .as_str()
        .or_else(|| error.as_str())
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_owned)
}

/// The bare upper-case code of a Firebase error message (`EMAIL_NOT_FOUND`
/// from `auth/email-not-found` or `WEAK_PASSWORD : Password should be…`).
fn bare_code(message: &str) -> String {
    let message = message.trim();
    let message = message
        .strip_prefix("auth/")
        .or_else(|| message.strip_prefix("AUTH/"))
        .unwrap_or(message);
    let code = message
        .split([' ', ':'])
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase()
        .replace('-', "_");
    if code.is_empty()
        || code.len() > 128
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return "rejected".to_owned();
    }
    code
}

/// What a person reads for a Firebase error message.
fn sentence(message: &str, fallback: &str) -> String {
    let code = bare_code(message);
    let known = match code.as_str() {
        "EMAIL_NOT_FOUND" | "USER_NOT_FOUND" => "No account exists for that email address.",
        "INVALID_PASSWORD" => "Incorrect password.",
        "INVALID_LOGIN_CREDENTIALS" | "INVALID_CREDENTIAL" => "Incorrect email or password.",
        "USER_DISABLED" => "This account has been disabled.",
        "EMAIL_EXISTS" => "An account already exists for that email address.",
        "OPERATION_NOT_ALLOWED" => "Email/password sign-in is not enabled for this project.",
        "TOO_MANY_ATTEMPTS_TRY_LATER" => "Too many failed attempts. Try again in a few minutes.",
        "NETWORK_REQUEST_FAILED" => NETWORK_SENTENCE,
        "WEAK_PASSWORD" => "Password is too weak. Use at least 6 characters.",
        "MISSING_PASSWORD" => "Enter your password.",
        "INVALID_EMAIL" => "Enter a valid email address.",
        "ADMIN_ONLY_OPERATION" => "This sign-in method is restricted for this project.",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_owned();
    }
    if code == "rejected" {
        return fallback.to_owned();
    }
    // Firebase's free-text detail is never surfaced: it can carry input data.
    let spaced = code.replace('_', " ").to_lowercase();
    let mut letters = spaced.chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_else(|| fallback.to_owned())
}

/// The refusal a non-success answer carries.
pub fn refusal(body: &[u8], fallback: &str) -> AuthRefusal {
    match raw_error(body) {
        Some(message) => AuthRefusal {
            code: bare_code(&message),
            message: sentence(&message, fallback),
        },
        None => AuthRefusal {
            code: "rejected".to_owned(),
            message: fallback.to_owned(),
        },
    }
}

/// The sign-in methods Firebase holds for an email, or `None` when it cannot
/// say (email-enumeration protection, an unreadable answer).
pub fn sign_in_methods(status: u16, body: &[u8]) -> Option<Vec<String>> {
    if !(200..300).contains(&status) || body.len() > crate::MAX_LOGIN_RESPONSE_BYTES {
        return None;
    }
    let value: Value = serde_json::from_slice(body).ok()?;
    if value["registered"] == Value::Bool(false) {
        return Some(Vec::new());
    }
    let methods = value["signinMethods"]
        .as_array()
        .or_else(|| value["allProviders"].as_array())?;
    Some(
        methods
            .iter()
            .filter_map(|method| method.as_str().map(str::to_owned))
            .collect(),
    )
}

/// The session a sign-in answer opens, validated, as the page binds it.
pub fn session(
    body: &[u8],
    expected_email: Option<&str>,
    project: &str,
    now_epoch_seconds: u64,
) -> Result<crate::ValidatedLogin, AuthRefusal> {
    crate::validate_firebase_login_response(body, expected_email, project, now_epoch_seconds)
        .map_err(|_| AuthRefusal::invalid_identity())
}

/// What a refresh answer means for the session that asked.
pub enum Refreshed {
    /// The same account's new session.
    Session(crate::ValidatedLogin),
    /// The refresh token was revoked or expired, the account disabled, or the
    /// answer named another account: the session ends.
    Ended,
    /// No answer this time (network, server): the held session stays.
    Unavailable,
}

/// The refresh call's form body.
pub fn refresh_body(refresh_token: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("refresh_token", refresh_token)
        .finish()
}

pub fn refreshed(
    status: u16,
    body: &[u8],
    uid: &str,
    email: &str,
    project: &str,
    now_epoch_seconds: u64,
) -> Refreshed {
    if body.len() > crate::MAX_LOGIN_RESPONSE_BYTES {
        return Refreshed::Unavailable;
    }
    if !(200..300).contains(&status) {
        let ended = status == 400
            && raw_error(body).is_some_and(|message| {
                matches!(
                    bare_code(&message).as_str(),
                    "TOKEN_EXPIRED" | "INVALID_REFRESH_TOKEN" | "USER_DISABLED"
                )
            });
        return if ended {
            Refreshed::Ended
        } else {
            Refreshed::Unavailable
        };
    }
    let Ok(answer) = serde_json::from_slice::<Value>(body) else {
        return Refreshed::Unavailable;
    };
    // The refresh answer names its fields in snake case; the validator reads
    // the sign-in answer's.
    let wire = json!({
        "localId": answer["user_id"],
        "email": email,
        "idToken": answer["id_token"],
        "refreshToken": answer["refresh_token"],
        "expiresIn": answer["expires_in"],
    });
    match crate::validate_firebase_login_response(
        wire.to_string().as_bytes(),
        Some(email),
        project,
        now_epoch_seconds,
    ) {
        Ok(login) if login.principal().uid() == uid => Refreshed::Session(login),
        _ => Refreshed::Ended,
    }
}

/// The browser's Firebase exchanges. Every session they open is bound by the
/// Kernel's own session (`crate::auth_session`); none crosses to the page.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod exports {
    use futures_util::StreamExt;
    use js_sys::Reflect;
    use wasm_bindgen::prelude::*;

    use super::{refreshed, refusal, session, sign_in_methods, AuthRefusal, Call, Refreshed};
    use crate::gateway_exchange::browser::within;
    use crate::gateway_target::{FIREBASE_API_KEY, FIREBASE_PROJECT};

    const CALL_TIMEOUT_MS: u64 = 30_000;
    const REFRESH_TIMEOUT_MS: u64 = 8_000;
    const ANSWER_CAP: usize = crate::MAX_LOGIN_RESPONSE_BYTES;

    #[wasm_bindgen(typescript_custom_section)]
    const FIREBASE_AUTH: &'static str = r#"
/** How a Firebase sign-in call rejects. */
export interface FirebaseAuthRefusal extends Error {
    code: string;
    /** The account's sign-in methods, when the refusal asks for them; null when Firebase cannot say. */
    methods?: string[] | null;
}
"#;

    fn configured() -> Result<(&'static str, &'static str), AuthRefusal> {
        match (FIREBASE_API_KEY, FIREBASE_PROJECT) {
            (Some(key), Some(project)) => Ok((key, project)),
            _ => Err(AuthRefusal::unconfigured()),
        }
    }

    pub(crate) fn now_epoch_seconds() -> u64 {
        (js_sys::Date::now() / 1000.0).max(0.0) as u64
    }

    /// POST to Firebase; `None` when no answer arrived in time.
    async fn post(
        url: String,
        content_type: &str,
        body: String,
        timeout_ms: u64,
    ) -> Option<(u16, Vec<u8>)> {
        let request = reqwest::Client::new()
            .post(url)
            .header("Content-Type", content_type)
            .body(body);
        let exchange = async move {
            let response = request.send().await.ok()?;
            let status = response.status().as_u16();
            if response
                .content_length()
                .is_some_and(|length| length > ANSWER_CAP as u64)
            {
                return None;
            }
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.ok()?;
                if chunk.len() > ANSWER_CAP.saturating_sub(bytes.len()) {
                    return None;
                }
                bytes.extend_from_slice(&chunk);
            }
            Some((status, bytes))
        };
        within(exchange, timeout_ms).await.flatten()
    }

    async fn call(call: &Call<'_>, key: &str) -> Option<(u16, Vec<u8>)> {
        post(
            format!("{}?key={key}", call.endpoint()),
            "application/json",
            call.body().to_string(),
            CALL_TIMEOUT_MS,
        )
        .await
    }

    /// The refusal as the `Error` the page catches (`FirebaseAuthRefusal`).
    pub(crate) fn rejection(
        refused: &AuthRefusal,
        methods: Option<Option<Vec<String>>>,
    ) -> JsValue {
        let error = js_sys::Error::new(&refused.message);
        error.set_name("FirebaseAuthRefusal");
        let _ = Reflect::set(&error, &"code".into(), &refused.code.as_str().into());
        if let Some(methods) = methods {
            let value = match methods {
                Some(methods) => methods
                    .iter()
                    .map(|method| JsValue::from_str(method))
                    .collect::<js_sys::Array>()
                    .into(),
                None => JsValue::NULL,
            };
            let _ = Reflect::set(&error, &"methods".into(), &value);
        }
        error.into()
    }

    /// Run a session-opening call: the validated login, or the refusal as
    /// the page's `Error`, with the account's sign-in methods when the
    /// refusal asks for them.
    pub(crate) async fn open(
        call: Call<'_>,
        expected_email: Option<&str>,
    ) -> Result<crate::ValidatedLogin, JsValue> {
        call.validate()
            .map_err(|refused| rejection(&refused, None))?;
        let (key, project) = configured().map_err(|refused| rejection(&refused, None))?;
        let Some((status, body)) = self::call(&call, key).await else {
            return Err(rejection(&AuthRefusal::network(), None));
        };
        if (200..300).contains(&status) {
            return session(&body, expected_email, project, now_epoch_seconds())
                .map_err(|refused| rejection(&refused, None));
        }
        let refused = refusal(&body, call.fallback());
        let methods = match (&call, call.asks_for_methods(&refused.code)) {
            (Call::SignIn { email, .. } | Call::SignUp { email, .. }, true) => {
                let probe = Call::SignInMethods { email };
                Some(
                    self::call(&probe, key)
                        .await
                        .and_then(|(status, body)| sign_in_methods(status, &body)),
                )
            }
            _ => None,
        };
        Err(rejection(&refused, methods))
    }

    /// Whether the platform reports any network link (`navigator.onLine`).
    fn link_up() -> bool {
        Reflect::get(&js_sys::global(), &"navigator".into())
            .and_then(|navigator| Reflect::get(&navigator, &"onLine".into()))
            .ok()
            .and_then(|online| online.as_bool())
            .unwrap_or(true)
    }

    /// Refresh the session of `uid` and `email` with its refresh token.
    pub(crate) async fn refresh(
        refresh_token: &str,
        uid: &str,
        email: &str,
    ) -> crate::auth_session::Refreshed {
        use crate::auth_session::Refreshed as Session;
        if refresh_token
            .len()
            .saturating_add(uid.len())
            .saturating_add(email.len())
            > ANSWER_CAP
        {
            return Session::Ended;
        }
        let Ok((key, project)) = configured() else {
            return Session::Unavailable;
        };
        let answer = post(
            format!("{}?key={key}", super::SECURE_TOKEN),
            "application/x-www-form-urlencoded",
            super::refresh_body(refresh_token),
            REFRESH_TIMEOUT_MS,
        )
        .await;
        match answer {
            None if !link_up() => Session::Offline,
            None => Session::Unavailable,
            Some((status, body)) => {
                match refreshed(status, &body, uid, email, project, now_epoch_seconds()) {
                    Refreshed::Session(login) => Session::Session(login),
                    Refreshed::Ended => Session::Ended,
                    Refreshed::Unavailable => Session::Unavailable,
                }
            }
        }
    }

    /// Send the password-reset mail.
    #[wasm_bindgen(js_name = firebaseSendPasswordReset)]
    pub async fn firebase_send_password_reset(email: String) -> Result<(), JsValue> {
        let (key, _) = configured().map_err(|refused| rejection(&refused, None))?;
        let reset = Call::PasswordReset { email: &email };
        reset
            .validate()
            .map_err(|refused| rejection(&refused, None))?;
        match call(&reset, key).await {
            None => Err(rejection(&AuthRefusal::network(), None)),
            Some((status, _)) if (200..300).contains(&status) => Ok(()),
            Some((_, body)) => Err(rejection(&refusal(&body, reset.fallback()), None)),
        }
    }
}

#[cfg(test)]
#[path = "firebase_auth_tests.rs"]
mod tests;
