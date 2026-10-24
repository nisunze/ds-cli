//! The page's signed-in session, owned here (architecture sentences 2 and 3,
//! development philosophy §6): Rust holds the principal, decides when its
//! bearer is refreshed and what a failed refresh means, and keeps the record
//! in the Kernel's own store (`store`, §7). TypeScript starts the session,
//! renders its [`AuthSessionView`] and forwards the person's intents; it never
//! reads, writes or forwards a token.
//!
//! [`Session`] is the whole decision: which record is held, whether its ID
//! token is fresh, what a refresh answer does to it, and which write the
//! store needs. Every change advances the session's generation, so a refresh
//! that started before a sign-in, a sign-out or another account's sign-in can
//! never touch the newer session. The browser executor (`browser`) performs
//! the exchanges and the store calls the decisions name.
//!
//! The record lives in the browser profile's `auth_session` store (OPFS
//! SQLite in the storage worker), never in Core's protected Store: Core holds
//! its own principal. The ID token is kept with the refresh token so a reload
//! restores the session offline with no network round trip; both are bounded
//! secrets that never leave Rust except as the request's bearer.

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::gateway_exchange::Refusal;

pub mod google;
#[cfg(feature = "durable-store")]
pub mod store;

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod browser;

pub const SCHEMA: &str = "ds.auth.session/v1";
/// A held ID token is refreshed once it is this old, so a request never
/// carries a token in its last ten minutes …
pub const REFRESH_AFTER_SECONDS: u64 = 50 * 60;
/// … or once it is this close to its own expiry.
pub const EXPIRY_MARGIN_SECONDS: u64 = 120;

/// The refresh token was revoked or expired, the account disabled, or the
/// answer named another account: the session ended.
pub const REVOKED: &str = "auth_session_revoked";
/// The held token expired and no refresh can run without a network.
pub const OFFLINE: &str = "auth_refresh_offline";
/// The held token expired and the refresh got no usable answer this time.
pub const UNAVAILABLE: &str = "auth_refresh_unavailable";
/// A record that does not admit.
pub const INVALID: &str = "auth_session_invalid";
/// A sign-in intent before the page started its session.
pub const NOT_STARTED: &str = "auth_session_not_started";

const MAX_SECRET: usize = 16 * 1024;
const MAX_UID: usize = 128;
const MAX_EMAIL: usize = 320;
const MAX_TEXT: usize = 2048;

/// The signed-in account, as the page shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct AuthUser {
    pub uid: String,
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub photo_url: Option<String>,
}

impl AuthUser {
    /// The login contract's identity rule: Firebase account ids are opaque
    /// (case, spaces and slashes are kept), never padded or empty.
    fn validate(&self) -> Result<(), String> {
        if !opaque(&self.uid, MAX_UID) {
            return Err(invalid("the account id is not an exact identity"));
        }
        if !email(&self.email) {
            return Err(invalid("the account email is not an email address"));
        }
        for text in [&self.display_name, &self.photo_url].into_iter().flatten() {
            if text.len() > MAX_TEXT || text.chars().any(char::is_control) {
                return Err(invalid("a display field exceeds its bound"));
            }
        }
        Ok(())
    }
}

/// Why the last session ended without the person signing out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum AuthEnded {
    /// Firebase refused the refresh: revoked, expired, disabled or another
    /// account.
    Revoked,
}

/// What the page renders. It carries no credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AuthSessionView {
    /// The held record is being read.
    Restoring,
    SignedOut {
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        ended: Option<AuthEnded>,
    },
    SignedIn {
        user: AuthUser,
    },
}

/// The held session: what the store keeps and the page's Kernel holds.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub schema: String,
    /// 32 lowercase hex digits minted at sign-in; a refresh keeps it, so a
    /// write can tell this session from a later one.
    pub session: String,
    pub user: AuthUser,
    pub id_token: String,
    pub refresh_token: String,
    pub expires_at_epoch_seconds: u64,
    /// When the ID token was obtained (0 when unknown: refreshed first).
    pub obtained_at_epoch_seconds: u64,
}

impl Drop for Record {
    fn drop(&mut self) {
        self.id_token.zeroize();
        self.refresh_token.zeroize();
    }
}

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Record")
            .field("session", &self.session)
            .field("user", &self.user)
            .field("expires_at_epoch_seconds", &self.expires_at_epoch_seconds)
            .finish_non_exhaustive()
    }
}

impl Record {
    /// The record a validated sign-in opens.
    pub fn from_login(login: &crate::ValidatedLogin, session: &str, now: u64) -> Self {
        let principal = login.principal();
        Self {
            schema: SCHEMA.into(),
            session: session.into(),
            user: AuthUser {
                uid: principal.uid().into(),
                email: principal.email().into(),
                display_name: principal.display_name().map(str::to_owned),
                photo_url: principal.photo_url().map(str::to_owned),
            },
            id_token: login.id_token().into(),
            refresh_token: login.refresh_token().into(),
            expires_at_epoch_seconds: login.expires_at_epoch_seconds(),
            obtained_at_epoch_seconds: now,
        }
    }

    /// The same session with a refresh's new tokens; the account's display
    /// fields stay, since a refresh answer names none.
    fn renewed(&self, login: &crate::ValidatedLogin, now: u64) -> Self {
        Self {
            schema: SCHEMA.into(),
            session: self.session.clone(),
            user: self.user.clone(),
            id_token: login.id_token().into(),
            refresh_token: login.refresh_token().into(),
            expires_at_epoch_seconds: login.expires_at_epoch_seconds(),
            obtained_at_epoch_seconds: now,
        }
    }

    /// The record's shape: schema, session id, account and bounded secrets.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(invalid("the record names another schema"));
        }
        if !hex32(&self.session) {
            return Err(invalid("the session id is not 32 lowercase hex digits"));
        }
        self.user.validate()?;
        for secret in [&self.id_token, &self.refresh_token] {
            if !self::secret(secret) {
                return Err(invalid("a token is empty, oversized or not one line"));
            }
        }
        Ok(())
    }

    /// Whether this record belongs to this build's Firebase project and its
    /// own account: its ID token's subject, email, audience and issuer.
    /// Expiry is not checked: an expired token refreshes.
    pub fn admits(&self, project: &str) -> Result<(), String> {
        self.validate()?;
        crate::validate_firebase_id_token(
            &self.id_token,
            &self.user.uid,
            &self.user.email,
            project,
            0,
        )
        .map(|_| ())
        .map_err(|_| invalid("the ID token names another account or project"))
    }

    /// The held token may be sent as it is.
    pub fn fresh(&self, now: u64) -> bool {
        self.expires_at_epoch_seconds > now.saturating_add(EXPIRY_MARGIN_SECONDS)
            && now
                < self
                    .obtained_at_epoch_seconds
                    .saturating_add(REFRESH_AFTER_SECONDS)
    }

    /// The held token has not expired yet.
    pub fn usable(&self, now: u64) -> bool {
        self.expires_at_epoch_seconds > now
    }

    fn credentials(&self) -> Credentials {
        Credentials {
            uid: self.user.uid.clone(),
            email: self.user.email.clone(),
            token: self.id_token.clone(),
        }
    }
}

/// One request's bearer, as the executor attaches it.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub uid: String,
    pub email: String,
    pub token: String,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("uid", &self.uid)
            .finish_non_exhaustive()
    }
}

/// One refresh to run: the session it belongs to and the refresh token.
#[derive(Clone, PartialEq, Eq)]
pub struct RefreshCall {
    pub generation: u64,
    pub uid: String,
    pub email: String,
    pub refresh_token: String,
}

impl Drop for RefreshCall {
    fn drop(&mut self) {
        self.refresh_token.zeroize();
    }
}

impl std::fmt::Debug for RefreshCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshCall")
            .field("generation", &self.generation)
            .field("uid", &self.uid)
            .finish_non_exhaustive()
    }
}

/// What the next request's bearer needs.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// No session: the request goes without a bearer.
    SignedOut,
    /// The held token is fresh.
    Use(Credentials),
    /// Refresh first.
    Refresh(RefreshCall),
}

/// What a refresh exchange observed.
#[derive(Debug)]
pub enum Refreshed {
    /// The same account's new session, validated.
    Session(crate::ValidatedLogin),
    /// Firebase refused the refresh token, or answered for another account.
    Ended,
    /// No answer arrived and the platform reports no network.
    Offline,
    /// No usable answer arrived (timeout, server error, unreadable answer).
    Unavailable,
}

/// What a refresh means for the request that waited for it.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Bearer(Credentials),
    /// The session changed while the refresh ran: plan again.
    Superseded,
    Refused(Box<Refusal>),
}

/// The store write a decision needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Persist {
    /// Establish this session, replacing any other.
    SignIn(Record),
    /// Replace the same session's tokens; the store refuses when that
    /// session is no longer the profile's.
    Refresh(Record),
    /// Remove this session, if it is still the profile's.
    SignOut(String),
}

#[derive(Debug)]
enum State {
    Restoring,
    SignedOut {
        ended: Option<AuthEnded>,
    },
    /// `stored`: the store holds this session, so a refresh replaces it
    /// conditionally; otherwise the next write establishes it.
    SignedIn {
        record: Box<Record>,
        stored: bool,
    },
}

/// One page's session and its generation.
#[derive(Debug)]
pub struct Session {
    generation: u64,
    state: State,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            generation: 0,
            state: State::Restoring,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn view(&self) -> AuthSessionView {
        match &self.state {
            State::Restoring => AuthSessionView::Restoring,
            State::SignedOut { ended } => AuthSessionView::SignedOut { ended: *ended },
            State::SignedIn { record, .. } => AuthSessionView::SignedIn {
                user: record.user.clone(),
            },
        }
    }

    /// The signed-in account's id.
    pub fn uid(&self) -> Option<&str> {
        match &self.state {
            State::SignedIn { record, .. } => Some(&record.user.uid),
            _ => None,
        }
    }

    pub fn restoring(&self) -> bool {
        matches!(self.state, State::Restoring)
    }

    fn advance(&mut self, state: State) {
        self.generation += 1;
        self.state = state;
    }

    /// The held record was read: adopt it, unless a sign-in or a sign-out
    /// already decided this page's session.
    pub fn restored(&mut self, record: Option<Record>, stored: bool) {
        if !self.restoring() {
            return;
        }
        self.advance(match record {
            Some(record) => State::SignedIn {
                record: Box::new(record),
                stored,
            },
            None => State::SignedOut { ended: None },
        });
    }

    /// A validated sign-in replaces whatever this page held.
    pub fn sign_in(&mut self, record: Record) -> Persist {
        let persist = Persist::SignIn(record.clone());
        self.advance(State::SignedIn {
            record: Box::new(record),
            stored: false,
        });
        persist
    }

    /// The store acknowledged the session written for `generation`.
    pub fn stored(&mut self, generation: u64) {
        if generation != self.generation {
            return;
        }
        if let State::SignedIn { stored, .. } = &mut self.state {
            *stored = true;
        }
    }

    /// The person signs out: the session ends now and its record is handed
    /// back once, for the cleanup that still speaks as it.
    pub fn sign_out(&mut self) -> Option<Record> {
        let ended = match std::mem::replace(&mut self.state, State::Restoring) {
            State::SignedIn { record, .. } => Some(*record),
            _ => None,
        };
        self.advance(State::SignedOut { ended: None });
        ended
    }

    /// The store says the session `generation` wrote is no longer the
    /// profile's (another tab signed out or signed in): this page follows.
    pub fn ended_elsewhere(&mut self, generation: u64) -> bool {
        if generation != self.generation || self.uid().is_none() {
            return false;
        }
        self.advance(State::SignedOut { ended: None });
        true
    }

    /// What the next request's bearer needs. `force` refreshes a fresh
    /// token too (a request was refused as unauthenticated).
    pub fn plan(&self, now: u64, force: bool) -> Plan {
        match &self.state {
            State::SignedIn { record, .. } if !force && record.fresh(now) => {
                Plan::Use(record.credentials())
            }
            State::SignedIn { record, .. } => Plan::Refresh(RefreshCall {
                generation: self.generation,
                uid: record.user.uid.clone(),
                email: record.user.email.clone(),
                refresh_token: record.refresh_token.clone(),
            }),
            _ => Plan::SignedOut,
        }
    }

    /// Apply a refresh that ran for `generation`; answers the request that
    /// waited and the store write, if any.
    pub fn refreshed(
        &mut self,
        generation: u64,
        outcome: Refreshed,
        now: u64,
    ) -> (Answer, Option<Persist>) {
        if generation != self.generation {
            return (Answer::Superseded, None);
        }
        let State::SignedIn { record, stored } = &self.state else {
            return (Answer::Superseded, None);
        };
        match outcome {
            Refreshed::Session(login) if login.principal().uid() == record.user.uid => {
                let renewed = record.renewed(&login, now);
                let persist = if *stored {
                    Persist::Refresh(renewed.clone())
                } else {
                    Persist::SignIn(renewed.clone())
                };
                let answer = Answer::Bearer(renewed.credentials());
                let stored = *stored;
                // A refresh keeps the generation: requests planned on this
                // session stay this session's.
                self.state = State::SignedIn {
                    record: Box::new(renewed),
                    stored,
                };
                (answer, Some(persist))
            }
            Refreshed::Session(_) | Refreshed::Ended => {
                let session = record.session.clone();
                self.advance(State::SignedOut {
                    ended: Some(AuthEnded::Revoked),
                });
                (
                    Answer::Refused(Box::new(revoked())),
                    Some(Persist::SignOut(session)),
                )
            }
            Refreshed::Offline | Refreshed::Unavailable if record.usable(now) => {
                (Answer::Bearer(record.credentials()), None)
            }
            Refreshed::Offline => (Answer::Refused(Box::new(offline())), None),
            Refreshed::Unavailable => (Answer::Refused(Box::new(unavailable())), None),
        }
    }
}

/// The session ended: sign in again. Inert, as every signed-out refusal.
pub fn revoked() -> Refusal {
    Refusal {
        signed_out: true,
        ..Refusal::new(REVOKED, "Your session ended. Sign in again.")
    }
}

/// The token expired while offline; the session stays and refreshes once a
/// network returns.
pub fn offline() -> Refusal {
    Refusal {
        transient: true,
        retryable: true,
        ..Refusal::new(
            OFFLINE,
            "You are offline. Your sign-in refreshes when the connection returns.",
        )
    }
}

/// The token expired and the refresh got no usable answer; the session stays.
pub fn unavailable() -> Refusal {
    Refusal {
        transient: true,
        retryable: true,
        ..Refusal::new(
            UNAVAILABLE,
            "Your sign-in could not be refreshed. Try again.",
        )
    }
}

/// What a page does with the held record at start.
#[derive(Debug, PartialEq, Eq)]
pub struct Restore {
    /// The session to adopt.
    pub adopt: Option<Record>,
    /// Remove this stored session id: it no longer admits.
    pub forget: Option<String>,
}

/// The restore: the held record is adopted when it admits for this build's
/// Firebase project, and forgotten when it does not.
pub fn restore(stored: Option<Record>, project: Option<&str>) -> Restore {
    let (Some(project), Some(stored)) = (project, stored) else {
        return Restore {
            adopt: None,
            forget: None,
        };
    };
    if stored.admits(project).is_ok() {
        Restore {
            adopt: Some(stored),
            forget: None,
        }
    } else {
        Restore {
            adopt: None,
            forget: Some(stored.session.clone()),
        }
    }
}

fn invalid(message: &str) -> String {
    format!("{INVALID}: {message}")
}

/// An opaque identity: non-empty, unpadded, bounded, no control character.
fn opaque(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max
        && !value.chars().any(char::is_control)
}

fn email(value: &str) -> bool {
    opaque(value, MAX_EMAIL)
        && value
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
}

fn secret(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_SECRET && !value.contains(['\r', '\n'])
}

pub(crate) fn hex32(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
#[path = "auth_session_tests.rs"]
mod tests;
