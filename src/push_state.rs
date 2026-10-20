//! Messaging push on one browser profile: the Kernel record, its transitions
//! and the fail-closed projection the service worker reads.
//!
//! One record per Firebase app id under the host's lane and audience. It is not
//! an account's row: it says which account owns this profile's push
//! registration, so an account switch or a sign-out must reach it. The session
//! owner is the principal the host bound (the anonymous principal is no owner);
//! a caller never names an owner.
//!
//! A registration is active only while the session owner, the opt-in owner and
//! the registered owner are one account. Every transition that can end a
//! registration rotates the 128-bit generation (chained from the previous one
//! and the host's random nonce), so a registration attempt that started under
//! an earlier generation can never commit.
//!
//! The service worker cannot open SQLite. Every committed write re-states this
//! store's projection `{owner, epoch, generation}` in one Kernel-named OPFS
//! file ([`EffectFile::PushProjection`]), and the service worker shows a
//! notification only for an exact epoch match. The order is fail-closed: every
//! write that can end a registration deletes the projection before its
//! transaction ([`effects_before`]); every write answers the projection to
//! write (active) or delete (inactive) after its commit. A crash between the
//! two leaves the file absent, never stale. Activation alone deletes nothing
//! first and writes after its commit.
use crate::durable_store::{sha256, Effect, EffectFile, Identity, Input, Outcome, Principal};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const STORE: &str = "push_state";
pub const INVALID: &str = "push_state_invalid";
pub const PROJECTION_SCHEMA: &str = "ds.push-projection/v1";

const MAX_APP_ID: usize = 256;
const MAX_UID: usize = 128;
const MAX_FID: usize = 512;

pub const TABLE: &str = "CREATE TABLE IF NOT EXISTS push_state (profile TEXT NOT NULL, app_id TEXT NOT NULL, generation TEXT NOT NULL CHECK(length(generation)=32), session_owner TEXT, opt_in_owner TEXT, active_owner TEXT, fid TEXT, installation_hash TEXT, epoch TEXT, PRIMARY KEY(profile, app_id), CHECK((active_owner IS NULL)=(fid IS NULL) AND (fid IS NULL)=(installation_hash IS NULL) AND (installation_hash IS NULL)=(epoch IS NULL)))";
const ROW: &str = "SELECT generation,session_owner,opt_in_owner,active_owner,fid,installation_hash,epoch FROM push_state WHERE profile=?1 AND app_id=?2";
const PUT: &str = "INSERT INTO push_state(profile,app_id,generation,session_owner,opt_in_owner,active_owner,fid,installation_hash,epoch) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(profile,app_id) DO UPDATE SET generation=excluded.generation,session_owner=excluded.session_owner,opt_in_owner=excluded.opt_in_owner,active_owner=excluded.active_owner,fid=excluded.fid,installation_hash=excluded.installation_hash,epoch=excluded.epoch";

/// What a registration check expects of the record it started under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expect {
    /// The bound account still owns the session and its opt-in.
    Registering,
    /// The bound account still owns the session after disabling, with no
    /// registration and no opt-in of its own.
    Disabled,
}

/// One transaction's work. `nonce` is 32 lowercase hex characters of fresh
/// host randomness; the Kernel chains it into the next generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// The record as the bound session sees it (the active registration only
    /// when `epoch`, if named, matches exactly).
    Read {
        app_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        epoch: Option<String>,
    },
    /// Whether an attempt that started under `generation` is still current.
    Check {
        app_id: String,
        generation: String,
        expect: Expect,
    },
    /// The host's session changed or is being confirmed: a new owner ends the
    /// previous owner's registration.
    ObserveSession { app_id: String, nonce: String },
    /// The bound account opts this profile into push.
    OptIn { app_id: String, nonce: String },
    /// The bound account disables push on this profile; answers the
    /// registration it held so the host can revoke it. A disable by an
    /// account that no longer owns the session is stale and changes nothing.
    Disable { app_id: String, nonce: String },
    /// Activate the registration an attempt obtained, fenced by the
    /// generation it started under.
    CommitRegistration {
        app_id: String,
        generation: String,
        fid: String,
        installation_hash: String,
        epoch: String,
    },
    /// The messaging SDK dropped this installation id.
    ClearFid {
        app_id: String,
        fid: String,
        nonce: String,
    },
    /// Fail closed: forget every record of this profile, all app ids.
    ClearAll,
}

/// The effects a host completes before it opens the transaction of
/// `operation`: every write that can end a registration deletes the
/// projection first. Reads and activation name none.
pub fn effects_before(operation: &Operation) -> Vec<Effect> {
    match operation {
        Operation::Read { .. }
        | Operation::Check { .. }
        | Operation::CommitRegistration { .. } => vec![],
        _ => vec![Effect::DeleteFile {
            file: EffectFile::PushProjection,
        }],
    }
}

/// The record as one session sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub generation: String,
    /// The bound account owns the session and opted in: a registration may
    /// start under `generation`.
    pub registrable: bool,
    /// The bound account opted this profile into push.
    pub enabled: bool,
    /// The bound account's active registration.
    pub active: Option<Active>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Active {
    pub owner: String,
    pub fid: String,
    pub installation_hash: String,
    pub epoch: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Record {
    generation: String,
    session_owner: Option<String>,
    opt_in_owner: Option<String>,
    active: Option<Active>,
}

impl Record {
    fn fresh(nonce: &str) -> Self {
        Self {
            generation: next_generation(None, nonce),
            ..Self::default()
        }
    }
    /// End any registration and rotate the generation.
    fn without_active(&self, nonce: &str) -> Self {
        Self {
            generation: next_generation(Some(&self.generation), nonce),
            session_owner: self.session_owner.clone(),
            opt_in_owner: self.opt_in_owner.clone(),
            active: None,
        }
    }
    /// The active registration, only while session, opt-in and registration
    /// name one account.
    fn live(&self) -> Option<&Active> {
        self.active.as_ref().filter(|active| {
            self.session_owner.as_deref() == Some(active.owner.as_str())
                && self.opt_in_owner.as_deref() == Some(active.owner.as_str())
        })
    }
    fn view(&self, account: Option<&str>, epoch: Option<&str>) -> View {
        let owns = account.is_some() && self.session_owner.as_deref() == account;
        let enabled = account.is_some() && self.opt_in_owner.as_deref() == account;
        View {
            generation: self.generation.clone(),
            registrable: owns && enabled,
            enabled,
            active: self
                .live()
                .filter(|active| owns && epoch.is_none_or(|epoch| epoch == active.epoch))
                .cloned(),
        }
    }
    /// The projection the committed record states.
    fn projection(&self, app_id: &str) -> Effect {
        match self.live() {
            Some(active) => Effect::WriteFile {
                file: EffectFile::PushProjection,
                contents: json!({
                    "schema": PROJECTION_SCHEMA,
                    "app_id": app_id,
                    "owner": active.owner,
                    "epoch": active.epoch,
                    "generation": self.generation,
                })
                .to_string(),
            },
            None => Effect::DeleteFile {
                file: EffectFile::PushProjection,
            },
        }
    }
}

fn invalid(message: &str) -> String {
    format!("{INVALID}: {message}")
}
fn sql(error: rusqlite::Error) -> String {
    format!("{INVALID}: {error}")
}

/// An exact identity: bounded, with no whitespace or control character.
fn exact(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
fn app(app_id: &str) -> Result<&str, String> {
    if exact(app_id, MAX_APP_ID) {
        Ok(app_id)
    } else {
        Err(invalid("the Firebase app id is not an exact identity"))
    }
}
fn nonce(nonce: &str) -> Result<&str, String> {
    if hex(nonce, 32) {
        Ok(nonce)
    } else {
        Err(invalid("a nonce is 32 lowercase hex characters of fresh randomness"))
    }
}
fn next_generation(previous: Option<&str>, nonce: &str) -> String {
    sha256(format!("ds.push-state.generation/v1\0{}\0{nonce}", previous.unwrap_or("")).as_bytes())
        [..32]
        .to_owned()
}

/// The profile a record belongs to: the host's lane and audience, never the
/// account, so a sign-out and an account switch reach the same record.
fn profile(identity: &Identity) -> Result<String, String> {
    serde_json::to_string(&(&identity.lane, &identity.audience))
        .map(|canonical| sha256(canonical.as_bytes()))
        .map_err(|e| e.to_string())
}
/// The bound account, or none for the anonymous principal.
fn bound(identity: &Identity) -> Option<&str> {
    match &identity.principal {
        Principal::Account(uid) => Some(uid),
        Principal::Anonymous => None,
    }
}
/// The bound account, refusing the anonymous principal for an account action.
fn account<'a>(identity: &'a Identity, action: &str) -> Result<&'a str, String> {
    let account = bound(identity).ok_or_else(|| invalid(&format!("{action} needs a signed-in account")))?;
    if exact(account, MAX_UID) {
        Ok(account)
    } else {
        Err(invalid("the bound account is not an exact identity"))
    }
}

fn read(connection: &Connection, profile: &str, app_id: &str) -> Result<Option<Record>, String> {
    let row = connection
        .query_row(ROW, params![profile, app_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })
        .optional()
        .map_err(sql)?;
    let Some((generation, session_owner, opt_in_owner, owner, fid, hash, epoch)) = row else {
        return Ok(None);
    };
    let active = match (owner, fid, hash, epoch) {
        (Some(owner), Some(fid), Some(installation_hash), Some(epoch)) => Some(Active {
            owner,
            fid,
            installation_hash,
            epoch,
        }),
        (None, None, None, None) => None,
        _ => return Err(invalid("a stored registration is incomplete")),
    };
    if !hex(&generation, 32) {
        return Err(invalid("a stored generation is malformed"));
    }
    Ok(Some(Record {
        generation,
        session_owner,
        opt_in_owner,
        active,
    }))
}

fn write(connection: &Connection, profile: &str, app_id: &str, record: &Record) -> Result<(), String> {
    let active = record.active.as_ref();
    connection
        .execute(
            PUT,
            params![
                profile,
                app_id,
                record.generation,
                record.session_owner,
                record.opt_in_owner,
                active.map(|a| &a.owner),
                active.map(|a| &a.fid),
                active.map(|a| &a.installation_hash),
                active.map(|a| &a.epoch),
            ],
        )
        .map_err(sql)?;
    Ok(())
}

/// The answer of a write: its fields, the committed record as the session
/// sees it, and the projection that record states.
fn written(mut value: Map<String, Value>, record: Option<&Record>, identity: &Identity, app_id: &str) -> Result<Outcome, String> {
    value.insert(
        "record".into(),
        serde_json::to_value(record.map(|record| record.view(bound(identity), None))).map_err(|e| e.to_string())?,
    );
    let mut outcome = Outcome::written(value);
    outcome.after = vec![match record {
        Some(record) => record.projection(app_id),
        None => Effect::DeleteFile {
            file: EffectFile::PushProjection,
        },
    }];
    Ok(outcome)
}
fn fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => Map::new(),
    }
}

/// Run one operation inside the caller's transaction.
pub fn execute(connection: &Connection, input: Input, operation: Operation) -> Result<Outcome, String> {
    if input.bytes.is_some() {
        return Err(invalid("push state carries no payload bytes"));
    }
    connection.execute_batch(TABLE).map_err(sql)?;
    let identity = input.identity;
    let profile = profile(identity)?;
    let session = bound(identity);
    match operation {
        Operation::Read { app_id, epoch } => {
            let app_id = app(&app_id)?;
            if epoch.as_deref().is_some_and(|epoch| !hex(epoch, 32)) {
                return Err(invalid("an epoch is 32 lowercase hex characters"));
            }
            let record = read(connection, &profile, app_id)?;
            Ok(Outcome::read(fields(json!({
                "record": record.map(|record| record.view(session, epoch.as_deref())),
            }))))
        }
        Operation::Check {
            app_id,
            generation,
            expect,
        } => {
            let record = read(connection, &profile, app(&app_id)?)?;
            let current = record.is_some_and(|record| {
                let owns = session.is_some() && record.session_owner.as_deref() == session;
                record.generation == generation
                    && owns
                    && match expect {
                        Expect::Registering => record.opt_in_owner.as_deref() == session,
                        Expect::Disabled => record.opt_in_owner.as_deref() != session && record.active.is_none(),
                    }
            });
            Ok(Outcome::read(fields(json!({"current": current}))))
        }
        Operation::ObserveSession { app_id, nonce: n } => {
            let app_id = app(&app_id)?;
            let n = nonce(&n)?;
            let owner = session.map(str::to_owned);
            let next = match read(connection, &profile, app_id)? {
                Some(current) if current.session_owner == owner => current,
                Some(current) => Record {
                    session_owner: owner,
                    ..current.without_active(n)
                },
                None => Record {
                    session_owner: owner,
                    ..Record::fresh(n)
                },
            };
            write(connection, &profile, app_id, &next)?;
            written(Map::new(), Some(&next), identity, app_id)
        }
        Operation::OptIn { app_id, nonce: n } => {
            let app_id = app(&app_id)?;
            let n = nonce(&n)?;
            let account = account(identity, "an opt-in")?;
            let base = read(connection, &profile, app_id)?.unwrap_or_else(|| Record::fresh(n));
            let next = Record {
                session_owner: Some(account.into()),
                opt_in_owner: Some(account.into()),
                ..base.without_active(n)
            };
            write(connection, &profile, app_id, &next)?;
            written(Map::new(), Some(&next), identity, app_id)
        }
        Operation::Disable { app_id, nonce: n } => {
            let app_id = app(&app_id)?;
            let n = nonce(&n)?;
            let account = account(identity, "disabling push")?;
            match read(connection, &profile, app_id)? {
                Some(current) if current.session_owner.as_deref() == Some(account) => {
                    let previous = current.active.clone().filter(|active| active.owner == account);
                    let mut next = current.without_active(n);
                    if next.opt_in_owner.as_deref() == Some(account) {
                        next.opt_in_owner = None;
                    }
                    write(connection, &profile, app_id, &next)?;
                    written(fields(json!({"stale": false, "previous": previous})), Some(&next), identity, app_id)
                }
                // Another session owns the profile: a late disable changes nothing.
                current => written(fields(json!({"stale": true, "previous": null})), current.as_ref(), identity, app_id),
            }
        }
        Operation::CommitRegistration {
            app_id,
            generation,
            fid,
            installation_hash,
            epoch,
        } => {
            let app_id = app(&app_id)?;
            let account = account(identity, "a registration")?;
            if !hex(&generation, 32) || !exact(&fid, MAX_FID) || !hex(&installation_hash, 64) || !hex(&epoch, 32) {
                return Err(invalid(
                    "a registration names a generation, an exact fid, a 64-hex installation hash and a 32-hex epoch",
                ));
            }
            match read(connection, &profile, app_id)? {
                Some(current)
                    if current.generation == generation
                        && current.session_owner.as_deref() == Some(account)
                        && current.opt_in_owner.as_deref() == Some(account) =>
                {
                    let next = Record {
                        active: Some(Active {
                            owner: account.into(),
                            fid,
                            installation_hash,
                            epoch,
                        }),
                        ..current
                    };
                    write(connection, &profile, app_id, &next)?;
                    written(fields(json!({"applied": true})), Some(&next), identity, app_id)
                }
                current => written(fields(json!({"applied": false})), current.as_ref(), identity, app_id),
            }
        }
        Operation::ClearFid { app_id, fid, nonce: n } => {
            let app_id = app(&app_id)?;
            let n = nonce(&n)?;
            if !exact(&fid, MAX_FID) {
                return Err(invalid("a fid is an exact identity"));
            }
            let next = match read(connection, &profile, app_id)? {
                Some(current) if current.active.as_ref().is_some_and(|active| active.fid == fid) => {
                    let next = current.without_active(n);
                    write(connection, &profile, app_id, &next)?;
                    Some(next)
                }
                current => current,
            };
            written(Map::new(), next.as_ref(), identity, app_id)
        }
        Operation::ClearAll => {
            connection
                .execute("DELETE FROM push_state WHERE profile=?1", params![profile])
                .map_err(sql)?;
            written(Map::new(), None, identity, "")
        }
    }
}

#[cfg(test)]
#[path = "push_state_tests.rs"]
mod tests;
