//! Retained reads: the one Kernel store for every plain cache of a Brain
//! read, run by the same rusqlite code in Core's protected Store (Desktop and
//! headless) and in the browser's storage worker (hosted Web).
//!
//! A retained read is an answer a flow fetched through the gateway exchange
//! and keeps so the next open paints at once and an offline window still has
//! it. It is never authority: the flow that wrote it re-admits it through its
//! own typed answer, so a stored value that no longer deserializes reads as
//! absent. Work (drafts, queues, idempotency keys) never lives here.
//!
//! Each [`Kind`] declares its fence (the account, or one project of it), its
//! key grammar, the byte bound of one value and how many rows one owner keeps;
//! a write past the row bound evicts that owner's least recently used rows of
//! the kind. The host binds the identity ([`crate::durable_store`]); a caller
//! names the kind, the project of a project kind and the key. Every write,
//! invalidation and removal advances the kind's own generation
//! (`retained_reads.<kind>`), never another kind's; a conditional write names
//! the generation its flow read and is refused when it moved.
use crate::durable_store::{generation, Input, Outcome};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const STORE: &str = "retained_reads";
pub const TABLE: &str = "CREATE TABLE IF NOT EXISTS retained_reads (owner TEXT NOT NULL, kind TEXT NOT NULL, project TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, saved_at_ms INTEGER NOT NULL CHECK(saved_at_ms>=0 AND saved_at_ms<=9007199254740991), used_at_ms INTEGER NOT NULL CHECK(used_at_ms>=0 AND used_at_ms<=9007199254740991), invalidated INTEGER NOT NULL CHECK(invalidated IN (0,1)), PRIMARY KEY(owner,kind,project,key))";
/// A malformed call: an unknown kind, a project or key its grammar refuses.
pub const INVALID: &str = "retained_reads_invalid";
/// A value or row set past its kind's bound.
pub const TOO_LARGE: &str = "retained_reads_too_large";
/// A conditional write whose kind moved since its flow read it.
pub const STALE: &str = "retained_reads_stale";
const MAX_SAFE: u64 = 9_007_199_254_740_991;
const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

/// Whom a kind's rows belong to besides the bound identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fence {
    /// The signed-in account; a call names no project.
    Account,
    /// One project of the account; a call names that project.
    Project,
}

/// What a kind admits as a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyGrammar {
    /// One row per fence; the key is empty.
    Single,
    /// A 64-character lowercase hex digest.
    Digest,
    /// An opaque printable token of at most this many bytes.
    Token(usize),
    /// One of a closed list.
    Choice(&'static [&'static str]),
}
impl KeyGrammar {
    fn admits(self, key: &str) -> bool {
        match self {
            KeyGrammar::Single => key.is_empty(),
            KeyGrammar::Digest => {
                key.len() == 64
                    && key
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }
            KeyGrammar::Token(max) => {
                !key.is_empty()
                    && key.len() <= max
                    && key.trim() == key
                    && !key.chars().any(char::is_control)
            }
            KeyGrammar::Choice(names) => names.contains(&key),
        }
    }
}

/// Everything one kind declares.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub fence: Fence,
    pub key: KeyGrammar,
    /// The largest stored value, in bytes of its JSON text.
    pub max_value_bytes: usize,
    /// How many rows of the kind one owner keeps, across its projects.
    pub max_rows: usize,
}

macro_rules! kinds {
    ($($(#[$doc:meta])* $variant:ident = $name:literal: $fence:ident, $key:expr, $bytes:expr, $rows:expr;)*) => {
        /// The closed vocabulary of retained reads.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
                    $(Kind::$variant => Rule { fence: Fence::$fence, key: $key, max_value_bytes: $bytes, max_rows: $rows },)*
                }
            }
        }
    };
}

kinds! {
    /// One page of a project's Assets index, keyed by its query digest.
    AssetsIndexPage = "assets_index_page": Project, KeyGrammar::Digest, 8 * MIB, 80;
    /// A project's assembled map state (the `/layers` document).
    MapState = "map_state": Project, KeyGrammar::Single, 48 * MIB, 24;
    /// The last SRE activity window the account read.
    SreActivity = "sre_activity": Account, KeyGrammar::Single, 32 * MIB, 1;
    /// The account's messaging conversation summaries.
    MessagingConversations = "messaging_conversations": Account, KeyGrammar::Single, 2 * MIB, 1;
    /// One conversation's recent messages, keyed by conversation id.
    MessagingTimeline = "messaging_timeline": Account, KeyGrammar::Token(256), 2 * MIB, 24;
    /// A project's Project Work graph.
    ProjectWorkGraph = "project_work_graph": Project, KeyGrammar::Single, 16 * MIB, 32;
    /// Rwanda administrative children, keyed `level:parent_code`.
    AdminChildren = "admin_children": Account, KeyGrammar::Token(64), MIB, 1024;
    /// The governance page: role definitions and, for a user manager, the
    /// user registry.
    GovernancePage = "governance_page": Account, KeyGrammar::Single, 4 * MIB, 1;
    /// A durable media-link grant of one project's media tree, keyed by project.
    MediaGrant = "media_grant": Account, KeyGrammar::Token(128), 16 * KIB, 64;
    /// A project's style catalogue: the Style Center's metadata.
    StyleCatalog = "style_catalog": Project, KeyGrammar::Single, 16 * MIB, 32;
    /// A project's geographic dataset catalogue.
    ReferenceCatalogue = "reference_catalogue": Project, KeyGrammar::Single, 8 * MIB, 32;
    /// The capabilities an account holds, for the platform (`platform`) or one
    /// project (`project:<id>`).
    AccountCapabilities = "account_capabilities": Account, KeyGrammar::Token(160), 2 * MIB, 64;
}

/// The generation row one kind reads and advances.
pub fn generation_key(kind: Kind) -> String {
    format!("{STORE}.{}", kind.name())
}

/// One row of a replaced set.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub key: String,
    pub value: Value,
}

/// One transaction's work; the host binds the identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// One row or `null`; reading refreshes the row's use.
    Read {
        kind: Kind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default)]
        key: String,
        now: u64,
    },
    /// Every row of the kind in its fence, most recently used first.
    List {
        kind: Kind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        now: u64,
    },
    /// Keep one value; refused as stale when `expect_generation` names a
    /// generation the kind has moved past.
    Write {
        kind: Kind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default)]
        key: String,
        value: Value,
        now: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect_generation: Option<u64>,
    },
    /// Make the fence's rows exactly this set, atomically.
    ReplaceSet {
        kind: Kind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        rows: Vec<Row>,
        now: u64,
    },
    /// Mark rows stale without removing them: one key, one project, or (no
    /// project named for a project kind) every project of the account.
    Invalidate {
        kind: Kind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    /// Remove rows, with the same reach as `invalidate`.
    Forget {
        kind: Kind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
}
impl Operation {
    pub fn kind(&self) -> Kind {
        match self {
            Operation::Read { kind, .. }
            | Operation::List { kind, .. }
            | Operation::Write { kind, .. }
            | Operation::ReplaceSet { kind, .. }
            | Operation::Invalidate { kind, .. }
            | Operation::Forget { kind, .. } => *kind,
        }
    }
}

fn invalid(kind: Kind, message: &str) -> String {
    format!("{INVALID}: {}: {message}", kind.name())
}
fn sql(error: rusqlite::Error) -> String {
    format!("{INVALID}: {error}")
}

/// The project column of a call: exactly the project of a project kind, the
/// empty text of an account kind. `sweep` admits a project kind's account-wide
/// reach (no project named).
fn project(kind: Kind, project: Option<&str>, sweep: bool) -> Result<Option<String>, String> {
    match (kind.rule().fence, project) {
        (Fence::Account, None) => Ok(Some(String::new())),
        (Fence::Project, Some(id)) if crate::execution_context::valid_project(id) => {
            Ok(Some(id.into()))
        }
        (Fence::Project, None) if sweep => Ok(None),
        (Fence::Account, Some(_)) => Err(invalid(kind, "an account read names no project")),
        _ => Err(invalid(kind, "a project read names one valid project")),
    }
}
fn key(kind: Kind, key: &str) -> Result<(), String> {
    if kind.rule().key.admits(key) {
        Ok(())
    } else {
        Err(invalid(kind, "the key does not match the kind's grammar"))
    }
}
fn time(kind: Kind, now: u64) -> Result<i64, String> {
    if now > MAX_SAFE {
        return Err(invalid(kind, "the time exceeds its bound"));
    }
    Ok(now as i64)
}
fn text(kind: Kind, value: &Value) -> Result<String, String> {
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    if text.len() > kind.rule().max_value_bytes {
        return Err(format!(
            "{TOO_LARGE}: {}: a value holds at most {} bytes",
            kind.name(),
            kind.rule().max_value_bytes
        ));
    }
    Ok(text)
}
/// A held row as an answer: an unreadable value reads as absent.
fn held(value: String, saved_at_ms: i64, invalidated: i64) -> Value {
    match serde_json::from_str::<Value>(&value) {
        Ok(value) => {
            json!({"value": value, "saved_at_ms": saved_at_ms, "invalidated": invalidated != 0})
        }
        Err(_) => Value::Null,
    }
}
/// Keep one owner's rows of a kind within its bound: the least recently used
/// go first, never a row this call just wrote (`kept`, a JSON array of keys
/// of `project`).
fn evict(
    connection: &Connection,
    owner: &str,
    kind: Kind,
    project: &str,
    kept: &str,
) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM retained_reads WHERE rowid IN (SELECT rowid FROM retained_reads WHERE owner=?1 AND kind=?2 AND NOT (project=?3 AND key IN (SELECT value FROM json_each(?4))) ORDER BY used_at_ms ASC, saved_at_ms ASC, project ASC, key ASC LIMIT max(0,(SELECT count(*) FROM retained_reads WHERE owner=?1 AND kind=?2)-?5))",
            params![owner, kind.name(), project, kept, kind.rule().max_rows as i64],
        )
        .map_err(sql)?;
    Ok(())
}
fn upsert(
    connection: &Connection,
    owner: &str,
    kind: Kind,
    project: &str,
    key: &str,
    text: &str,
    now: i64,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO retained_reads(owner,kind,project,key,value,saved_at_ms,used_at_ms,invalidated) VALUES(?1,?2,?3,?4,?5,?6,?6,0) ON CONFLICT(owner,kind,project,key) DO UPDATE SET value=excluded.value,saved_at_ms=excluded.saved_at_ms,used_at_ms=excluded.used_at_ms,invalidated=0",
            params![owner, kind.name(), project, key, text, now],
        )
        .map_err(sql)?;
    Ok(())
}
/// The `WHERE` reach of an invalidation or removal.
fn reach(
    kind: Kind,
    project: Option<&str>,
    row: Option<&str>,
) -> Result<(Option<String>, Option<String>), String> {
    let column = self::project(kind, project, true)?;
    if let Some(row) = row {
        if column.is_none() {
            return Err(invalid(kind, "a key is named inside its project"));
        }
        key(kind, row)?;
    }
    Ok((column, row.map(str::to_owned)))
}
const REACH: &str =
    "owner=?1 AND kind=?2 AND (?3 IS NULL OR project=?3) AND (?4 IS NULL OR key=?4)";

/// Run one operation inside the caller's transaction. Reads answer
/// `{"held": …}` or `{"rows": […]}`; writes name what they changed. The
/// answer's generation is the kind's own ([`Outcome::generation_key`]).
pub fn execute(
    connection: &Connection,
    input: Input,
    operation: Operation,
) -> Result<Outcome, String> {
    let kind = operation.kind();
    if input.bytes.is_some() {
        return Err(invalid(kind, "retained reads carry their values as JSON"));
    }
    connection.execute_batch(TABLE).map_err(sql)?;
    let owner = input.identity.owner();
    let mut answer = Map::new();
    let wrote = match operation {
        Operation::Read {
            kind,
            project,
            key: row,
            now,
        } => {
            let column = self::project(kind, project.as_deref(), false)?.unwrap_or_default();
            key(kind, &row)?;
            let now = time(kind, now)?;
            let found = connection
                .query_row(
                    "SELECT value,saved_at_ms,invalidated FROM retained_reads WHERE owner=?1 AND kind=?2 AND project=?3 AND key=?4",
                    params![owner, kind.name(), column, row],
                    |found| Ok((found.get::<_, String>(0)?, found.get::<_, i64>(1)?, found.get::<_, i64>(2)?)),
                )
                .optional()
                .map_err(sql)?;
            if found.is_some() {
                connection
                    .execute(
                        "UPDATE retained_reads SET used_at_ms=?5 WHERE owner=?1 AND kind=?2 AND project=?3 AND key=?4",
                        params![owner, kind.name(), column, row, now],
                    )
                    .map_err(sql)?;
            }
            answer.insert(
                "held".into(),
                found.map_or(Value::Null, |(value, saved, invalidated)| {
                    held(value, saved, invalidated)
                }),
            );
            false
        }
        Operation::List { kind, project, now } => {
            let column = self::project(kind, project.as_deref(), false)?.unwrap_or_default();
            let now = time(kind, now)?;
            let mut statement = connection
                .prepare("SELECT key,value,saved_at_ms,invalidated FROM retained_reads WHERE owner=?1 AND kind=?2 AND project=?3 ORDER BY used_at_ms DESC, key ASC LIMIT ?4")
                .map_err(sql)?;
            let rows = statement
                .query_map(
                    params![owner, kind.name(), column, kind.rule().max_rows as i64],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
                .map_err(sql)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql)?;
            connection
                .execute(
                    "UPDATE retained_reads SET used_at_ms=?4 WHERE owner=?1 AND kind=?2 AND project=?3",
                    params![owner, kind.name(), column, now],
                )
                .map_err(sql)?;
            let rows: Vec<Value> = rows
                .into_iter()
                .filter_map(|(key, value, saved, invalidated)| {
                    let mut row = held(value, saved, invalidated);
                    row.as_object_mut()?.insert("key".into(), key.into());
                    Some(row)
                })
                .collect();
            answer.insert("rows".into(), rows.into());
            false
        }
        Operation::Write {
            kind,
            project,
            key: row,
            value,
            now,
            expect_generation,
        } => {
            let column = self::project(kind, project.as_deref(), false)?.unwrap_or_default();
            key(kind, &row)?;
            let now = time(kind, now)?;
            let text = text(kind, &value)?;
            if let Some(expected) = expect_generation {
                let current = generation(connection, &generation_key(kind))?;
                if current != expected {
                    return Err(format!(
                        "{STALE}: {}: the retained read changed while this answer was fetched",
                        kind.name()
                    ));
                }
            }
            upsert(connection, &owner, kind, &column, &row, &text, now)?;
            evict(connection, &owner, kind, &column, &json!([row]).to_string())?;
            answer.insert("written".into(), 1.into());
            true
        }
        Operation::ReplaceSet {
            kind,
            project,
            rows,
            now,
        } => {
            let column = self::project(kind, project.as_deref(), false)?.unwrap_or_default();
            let now = time(kind, now)?;
            if rows.len() > kind.rule().max_rows {
                return Err(format!(
                    "{TOO_LARGE}: {}: a set holds at most {} rows",
                    kind.name(),
                    kind.rule().max_rows
                ));
            }
            let mut keys: Vec<&str> = Vec::with_capacity(rows.len());
            let mut texts = Vec::with_capacity(rows.len());
            for row in &rows {
                key(kind, &row.key)?;
                if keys.contains(&row.key.as_str()) {
                    return Err(invalid(kind, "a set names each key once"));
                }
                keys.push(&row.key);
                texts.push(text(kind, &row.value)?);
            }
            let names = serde_json::to_string(&keys).map_err(|e| e.to_string())?;
            let removed = connection
                .execute(
                    "DELETE FROM retained_reads WHERE owner=?1 AND kind=?2 AND project=?3 AND key NOT IN (SELECT value FROM json_each(?4))",
                    params![owner, kind.name(), column, names],
                )
                .map_err(sql)?;
            for (row, text) in rows.iter().zip(&texts) {
                upsert(connection, &owner, kind, &column, &row.key, text, now)?;
            }
            evict(connection, &owner, kind, &column, &names)?;
            answer.insert("written".into(), rows.len().into());
            answer.insert("removed".into(), removed.into());
            true
        }
        Operation::Invalidate {
            kind,
            project,
            key: row,
        } => {
            let (column, row) = reach(kind, project.as_deref(), row.as_deref())?;
            let changed = connection
                .execute(
                    &format!(
                        "UPDATE retained_reads SET invalidated=1 WHERE {REACH} AND invalidated=0"
                    ),
                    params![owner, kind.name(), column, row],
                )
                .map_err(sql)?;
            answer.insert("invalidated".into(), changed.into());
            true
        }
        Operation::Forget {
            kind,
            project,
            key: row,
        } => {
            let (column, row) = reach(kind, project.as_deref(), row.as_deref())?;
            let removed = connection
                .execute(
                    &format!("DELETE FROM retained_reads WHERE {REACH}"),
                    params![owner, kind.name(), column, row],
                )
                .map_err(sql)?;
            answer.insert("removed".into(), removed.into());
            true
        }
    };
    let mut outcome = if wrote {
        Outcome::written(answer)
    } else {
        Outcome::read(answer)
    };
    outcome.generation_key = Some(generation_key(kind));
    Ok(outcome)
}

#[cfg(test)]
#[path = "retained_reads_tests.rs"]
mod tests;
