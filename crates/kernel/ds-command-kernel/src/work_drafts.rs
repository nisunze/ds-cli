//! Work drafts: what a person typed, composed or computed on this device and
//! has not sent anywhere, held by the same rusqlite code in Core's protected
//! Store and the browser's storage worker. A draft is work, never a cache: no
//! retention evicts it, and a kind full to its bound refuses the next draft
//! instead of dropping an older one.
//!
//! Each [`Kind`] declares its fence (the account, or one project of it), its
//! key length, the byte bound of one value and how many rows one account
//! keeps. The host binds the identity
//! ([`crate::durable_store`]); a signed-out identity holds no drafts. A row
//! carries its value and an optional small `meta` that `list` answers
//! without reading any value.
use crate::durable_store::{Identity, Input, Outcome, Principal};
use crate::execution_context::valid_project;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const STORE: &str = "work_drafts";
/// A malformed call: an unknown kind, a project or key its grammar refuses.
pub const INVALID: &str = "work_drafts_invalid";
/// A value past its kind's bound, or a kind holding as many rows as it keeps.
pub const FULL: &str = "work_drafts_full";
/// No signed-in account: drafts belong to one.
pub const SIGNED_OUT: &str = "work_drafts_signed_out";
const TABLE: &str = "CREATE TABLE IF NOT EXISTS work_drafts (owner TEXT NOT NULL, kind TEXT NOT NULL, project TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, meta TEXT, saved_at_ms INTEGER NOT NULL CHECK(saved_at_ms>=0 AND saved_at_ms<=9007199254740991), PRIMARY KEY(owner,kind,project,key))";
const MAX_SAFE: u64 = 9_007_199_254_740_991;
const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;
/// The largest `meta`, in bytes of its JSON text.
const MAX_META_BYTES: usize = 4 * KIB;

/// Whom a kind's rows belong to besides the bound account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fence {
    Account,
    Project,
}

/// Everything one kind declares.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub fence: Fence,
    /// A key is an opaque printable token of at most this many bytes.
    pub max_key_bytes: usize,
    pub max_value_bytes: usize,
    /// How many rows of the kind one account keeps, across its projects.
    pub max_rows: usize,
}

macro_rules! kinds {
    ($($(#[$doc:meta])* $variant:ident = $name:literal: $fence:ident, $key:expr, $bytes:expr, $rows:expr;)*) => {
        /// The closed vocabulary of work drafts.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum Kind {
            $($(#[$doc])* #[serde(rename = $name)] $variant,)*
        }
        impl Kind {
            pub const ALL: &'static [Kind] = &[$(Kind::$variant),*];
            pub fn name(self) -> &'static str {
                match self { $(Kind::$variant => $name,)* }
            }
            pub fn rule(self) -> Rule {
                match self {
                    $(Kind::$variant => Rule { fence: Fence::$fence, max_key_bytes: $key, max_value_bytes: $bytes, max_rows: $rows },)*
                }
            }
        }
    };
}

kinds! {
    /// An unsent message, keyed by conversation id.
    MessagingMessage = "messaging_message": Account, 256, 16 * KIB, 2048;
    /// A survey form definition being edited, keyed by form slug.
    FormEditor = "form_editor": Account, 256, 4 * MIB, 512;
    /// A computed transformer version comparison, keyed by its artifact id;
    /// `meta` names its transformer and label.
    DesignComparison = "design_comparison": Project, 256, 32 * MIB, 1024;
    /// A survey entry being filled, keyed by form slug: its values without
    /// picked files, its location and connectivity, and the form revision.
    SurveyEntry = "survey_entry": Project, 256, MIB, 512;
    /// The form settings a person staged for one project in Form
    /// Management, keyed by the project id.
    ProjectForms = "project_forms": Account, 256, 4 * MIB, 256;
}

/// One transaction's work; the host binds the identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// One row or `null`.
    Read {
        kind: Kind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default)]
        key: String,
    },
    /// Every row of the kind in its fence, newest first: keys, `meta` and
    /// times, never values.
    List {
        kind: Kind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
    },
    /// Keep one draft, replacing the key's previous one.
    Put {
        kind: Kind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default)]
        key: String,
        value: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
        now: u64,
    },
    /// Remove only submitted project-form snapshots that still equal the held
    /// edits, atomically. Edits made after submission remain work.
    ProjectFormsSubmitted {
        key: String,
        submitted: Value,
        now: u64,
    },
    /// Forget one draft; an absent one is success.
    Delete {
        kind: Kind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default)]
        key: String,
    },
}

fn invalid(message: impl std::fmt::Display) -> String {
    format!("{INVALID}: {message}")
}
fn sql(error: rusqlite::Error) -> String {
    invalid(error)
}
fn account(identity: &Identity) -> Result<&str, String> {
    match &identity.principal {
        Principal::Account(uid) => Ok(uid),
        Principal::Anonymous => Err(format!("{SIGNED_OUT}: sign in to keep drafts")),
    }
}

/// The project column of a call: the project of a project kind, empty for an
/// account kind.
fn column(kind: Kind, project: Option<&str>) -> Result<String, String> {
    match (kind.rule().fence, project) {
        (Fence::Account, None) => Ok(String::new()),
        (Fence::Project, Some(id)) if valid_project(id) => Ok(id.into()),
        (Fence::Account, Some(_)) => Err(invalid(format!("{} names no project", kind.name()))),
        _ => Err(invalid(format!("{} names one valid project", kind.name()))),
    }
}
fn key(kind: Kind, key: &str) -> Result<(), String> {
    if kind == Kind::ProjectForms && !valid_project(key) {
        return Err(invalid("project forms name one valid project"));
    }
    let max = kind.rule().max_key_bytes;
    if !key.is_empty()
        && key.len() <= max
        && key.trim() == key
        && !key.chars().any(char::is_control)
    {
        Ok(())
    } else {
        Err(invalid(format!(
            "a {} key is a printable token",
            kind.name()
        )))
    }
}
fn texts(
    kind: Kind,
    value: &Value,
    meta: Option<&Value>,
) -> Result<(String, Option<String>), String> {
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    if text.len() > kind.rule().max_value_bytes {
        return Err(format!(
            "{FULL}: a {} draft holds at most {} bytes",
            kind.name(),
            kind.rule().max_value_bytes
        ));
    }
    let meta = match meta {
        None | Some(Value::Null) => None,
        Some(meta @ Value::Object(_)) => {
            let text = serde_json::to_string(meta).map_err(|e| e.to_string())?;
            if text.len() > MAX_META_BYTES {
                return Err(invalid("a draft's meta holds at most 4 KiB"));
            }
            Some(text)
        }
        Some(_) => return Err(invalid("a draft's meta is an object")),
    };
    Ok((text, meta))
}

/// Whether the account has room for one more row of the kind at this key.
fn room(
    connection: &Connection,
    owner: &str,
    kind: Kind,
    project: &str,
    row: &str,
) -> Result<bool, String> {
    let (held, exists): (i64, bool) = connection
        .query_row(
            "SELECT count(*), coalesce(max(project=?3 AND key=?4),0) FROM work_drafts WHERE owner=?1 AND kind=?2",
            params![owner, kind.name(), project, row],
            |found| Ok((found.get(0)?, found.get::<_, i64>(1)? != 0)),
        )
        .map_err(sql)?;
    Ok(exists || (held as usize) < kind.rule().max_rows)
}

fn put(
    connection: &Connection,
    owner: &str,
    kind: Kind,
    project: &str,
    row: &str,
    (text, meta): (String, Option<String>),
    now: u64,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO work_drafts(owner,kind,project,key,value,meta,saved_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(owner,kind,project,key) DO UPDATE SET value=excluded.value,meta=excluded.meta,saved_at_ms=excluded.saved_at_ms",
            params![owner, kind.name(), project, row, text, meta, now as i64],
        )
        .map_err(sql)?;
    Ok(())
}

fn meta_value(text: Option<String>) -> Value {
    text.and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// Run one operation inside the caller's transaction.
pub fn execute(
    connection: &Connection,
    input: Input,
    operation: Operation,
) -> Result<Outcome, String> {
    if input.bytes.is_some() {
        return Err(invalid("work drafts carry their values as JSON"));
    }
    connection.execute_batch(TABLE).map_err(sql)?;
    let identity = input.identity;
    let owner = identity.owner();
    let mut answer = Map::new();
    let wrote = match operation {
        Operation::Read {
            kind,
            project,
            key: row,
        } => {
            account(identity)?;
            let project = column(kind, project.as_deref())?;
            key(kind, &row)?;
            let found = connection
                .query_row(
                    "SELECT value,meta,saved_at_ms FROM work_drafts WHERE owner=?1 AND kind=?2 AND project=?3 AND key=?4",
                    params![owner, kind.name(), project, row],
                    |found| Ok((found.get::<_, String>(0)?, found.get::<_, Option<String>>(1)?, found.get::<_, i64>(2)?)),
                )
                .optional()
                .map_err(sql)?;
            let held = found.and_then(|(value, meta, saved)| {
                let value = serde_json::from_str::<Value>(&value).ok()?;
                Some(json!({"value": value, "meta": meta_value(meta), "saved_at_ms": saved}))
            });
            answer.insert("held".into(), held.unwrap_or(Value::Null));
            false
        }
        Operation::List { kind, project } => {
            account(identity)?;
            let project = column(kind, project.as_deref())?;
            let mut statement = connection
                .prepare("SELECT key,meta,saved_at_ms FROM work_drafts WHERE owner=?1 AND kind=?2 AND project=?3 ORDER BY saved_at_ms DESC, key ASC")
                .map_err(sql)?;
            let rows = statement
                .query_map(params![owner, kind.name(), project], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, i64>(2)?))
                })
                .map_err(sql)?
                .map(|row| row.map(|(key, meta, saved)| json!({"key": key, "meta": meta_value(meta), "saved_at_ms": saved})))
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql)?;
            answer.insert("rows".into(), rows.into());
            false
        }
        Operation::Put {
            kind,
            project,
            key: row,
            value,
            meta,
            now,
        } => {
            account(identity)?;
            let project = column(kind, project.as_deref())?;
            key(kind, &row)?;
            if now > MAX_SAFE {
                return Err(invalid("the time exceeds its bound"));
            }
            let texts = texts(kind, &value, meta.as_ref())?;
            if kind == Kind::ProjectForms && !value.is_object() {
                return Err(invalid("project form drafts must be an object"));
            }
            if kind == Kind::ProjectForms && value.as_object().is_some_and(Map::is_empty) {
                let removed = connection.execute("DELETE FROM work_drafts WHERE owner=?1 AND kind=?2 AND project=?3 AND key=?4", params![owner, kind.name(), project, row]).map_err(sql)?;
                answer.insert("removed".into(), removed.into());
                removed > 0
            } else {
                if !room(connection, &owner, kind, &project, &row)? {
                    return Err(format!(
                        "{FULL}: this account keeps at most {} {} drafts",
                        kind.rule().max_rows,
                        kind.name()
                    ));
                }
                put(connection, &owner, kind, &project, &row, texts, now)?;
                answer.insert("written".into(), 1.into());
                true
            }
        }
        Operation::ProjectFormsSubmitted {
            key: row,
            submitted,
            now,
        } => {
            account(identity)?;
            let kind = Kind::ProjectForms;
            key(kind, &row)?;
            if now > MAX_SAFE || !submitted.is_object() {
                return Err(invalid("invalid submitted project form drafts"));
            }
            let found: Option<String> = connection.query_row("SELECT value FROM work_drafts WHERE owner=?1 AND kind=?2 AND project='' AND key=?3", params![owner, kind.name(), row], |found| found.get(0)).optional().map_err(sql)?;
            if let Some(found) = found {
                let source: Value = serde_json::from_str(&found)
                    .map_err(|_| invalid("invalid held project form drafts"))?;
                let next = crate::survey::project_forms::evaluate(
                    &json!({"mode":"remove_submitted","source":source,"submitted":submitted}),
                )
                .map_err(invalid)?;
                let empty = next.as_object().is_some_and(Map::is_empty);
                if empty {
                    connection.execute("DELETE FROM work_drafts WHERE owner=?1 AND kind=?2 AND project='' AND key=?3", params![owner, kind.name(), row]).map_err(sql)?;
                } else {
                    put(
                        connection,
                        &owner,
                        kind,
                        "",
                        &row,
                        texts(kind, &next, None)?,
                        now,
                    )?;
                }
                answer.insert("removed".into(), empty.into());
                source != next
            } else {
                false
            }
        }
        Operation::Delete {
            kind,
            project,
            key: row,
        } => {
            account(identity)?;
            let project = column(kind, project.as_deref())?;
            key(kind, &row)?;
            let removed = connection
                .execute(
                    "DELETE FROM work_drafts WHERE owner=?1 AND kind=?2 AND project=?3 AND key=?4",
                    params![owner, kind.name(), project, row],
                )
                .map_err(sql)?;
            answer.insert("removed".into(), removed.into());
            removed > 0
        }
    };
    Ok(if wrote {
        Outcome::written(answer)
    } else {
        Outcome::read(answer)
    })
}

#[cfg(test)]
#[path = "work_drafts_tests.rs"]
mod tests;
