//! `auth_session`: the browser profile's own session record and its one
//! pending Google redirect (durable-store contract, "Stores").
//!
//! Rows belong to the profile, the host's lane and audience, never to an
//! account: the record is what names the account, so it must be readable
//! before anyone is known, and a sign-out or another account's sign-in must
//! reach it. A write names the session it belongs to: a refresh replaces only
//! its own session, and a sign-out removes only its own, so a tab that
//! refreshes after another tab signed out cannot bring the session back.
//! Core never serves this store; its protected account is its own principal.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::google::Redirect;
use super::Record;
use crate::durable_store::{sha256, Identity, Input, Outcome};

pub const STORE: &str = "auth_session";
pub const INVALID: &str = super::INVALID;
/// A refresh for a session the profile no longer holds.
pub const ENDED: &str = "auth_session_ended";

const MAX_RECORD: usize = 64 * 1024;
const TABLE: &str = "CREATE TABLE IF NOT EXISTS auth_session (profile TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('session','redirect')), record TEXT NOT NULL CHECK(length(record) <= 65536), PRIMARY KEY(profile, kind))";
const ROW: &str = "SELECT record FROM auth_session WHERE profile=?1 AND kind=?2";
const PUT: &str = "INSERT INTO auth_session(profile,kind,record) VALUES(?1,?2,?3) ON CONFLICT(profile,kind) DO UPDATE SET record=excluded.record";
const DELETE: &str = "DELETE FROM auth_session WHERE profile=?1 AND kind=?2";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// The held session, or none.
    Read,
    /// A new sign-in: replaces any session.
    SignIn { record: Record },
    /// The same session's new tokens; refused `auth_session_ended` when the
    /// profile holds another session or none.
    Refresh { record: Record },
    /// Remove this session if the profile still holds it.
    SignOut { session: String },
    /// Hold one pending Google redirect, replacing an older one.
    BeginRedirect { redirect: Redirect },
    /// Take the pending redirect for `state`: removed and answered when the
    /// state matches; a mismatch removes nothing.
    TakeRedirect { state: String },
}

fn invalid(message: &str) -> String {
    format!("{INVALID}: {message}")
}

fn sql(error: rusqlite::Error) -> String {
    format!("{INVALID}: {error}")
}

/// The profile a row belongs to: the host's lane and audience, never the
/// account.
fn profile(identity: &Identity) -> Result<String, String> {
    serde_json::to_string(&(&identity.lane, &identity.audience))
        .map(|canonical| sha256(canonical.as_bytes()))
        .map_err(|e| e.to_string())
}

fn held<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    profile: &str,
    kind: &str,
) -> Result<Option<T>, String> {
    let text: Option<String> = connection
        .query_row(ROW, params![profile, kind], |row| row.get(0))
        .optional()
        .map_err(sql)?;
    // A row this build cannot read is treated as absent; the next write
    // replaces it.
    Ok(text.and_then(|text| serde_json::from_str(&text).ok()))
}

fn put(
    connection: &Connection,
    profile: &str,
    kind: &str,
    value: &impl Serialize,
) -> Result<(), String> {
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    if text.len() > MAX_RECORD {
        return Err(invalid("a record exceeds 64 KiB"));
    }
    connection
        .execute(PUT, params![profile, kind, text])
        .map_err(sql)?;
    Ok(())
}

fn delete(connection: &Connection, profile: &str, kind: &str) -> Result<(), String> {
    connection
        .execute(DELETE, params![profile, kind])
        .map_err(sql)?;
    Ok(())
}

fn fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => Map::new(),
    }
}

pub fn execute(
    connection: &Connection,
    input: Input,
    operation: Operation,
) -> Result<Outcome, String> {
    if input.bytes.is_some() {
        return Err(invalid("auth_session takes no bytes"));
    }
    connection.execute_batch(TABLE).map_err(sql)?;
    let profile = profile(input.identity)?;
    match operation {
        Operation::Read => {
            let session = held::<Record>(connection, &profile, "session")?
                .filter(|record| record.validate().is_ok());
            Ok(Outcome::read(fields(json!({ "session": session }))))
        }
        Operation::SignIn { record } => {
            record.validate()?;
            put(connection, &profile, "session", &record)?;
            Ok(Outcome::written(fields(
                json!({ "session": record.session }),
            )))
        }
        Operation::Refresh { record } => {
            record.validate()?;
            let current = held::<Record>(connection, &profile, "session")?;
            if current.as_ref().map(|held| held.session.as_str()) != Some(record.session.as_str()) {
                return Err(format!(
                    "{ENDED}: this browser's session was signed out or replaced"
                ));
            }
            put(connection, &profile, "session", &record)?;
            Ok(Outcome::written(fields(
                json!({ "session": record.session }),
            )))
        }
        Operation::SignOut { session } => {
            if !super::hex32(&session) {
                return Err(invalid("the session id is not 32 lowercase hex digits"));
            }
            let current = held::<Record>(connection, &profile, "session")?;
            let removed = current.is_some_and(|held| held.session == session);
            if !removed {
                return Ok(Outcome::read(fields(json!({ "removed": false }))));
            }
            delete(connection, &profile, "session")?;
            Ok(Outcome::written(fields(json!({ "removed": true }))))
        }
        Operation::BeginRedirect { redirect } => {
            redirect.validate()?;
            put(connection, &profile, "redirect", &redirect)?;
            Ok(Outcome::written(Map::new()))
        }
        Operation::TakeRedirect { state } => {
            if !super::hex32(&state) {
                return Err(invalid("a redirect state is 32 lowercase hex digits"));
            }
            let Some(redirect) = held::<Redirect>(connection, &profile, "redirect")? else {
                return Ok(Outcome::read(fields(json!({ "found": "none" }))));
            };
            if redirect.state != state {
                return Ok(Outcome::read(fields(json!({ "found": "mismatch" }))));
            }
            delete(connection, &profile, "redirect")?;
            Ok(Outcome::written(fields(
                json!({ "found": "taken", "redirect": redirect }),
            )))
        }
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
