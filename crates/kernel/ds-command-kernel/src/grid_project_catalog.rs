//! The retained project model catalogue: one Kernel-owned SQLite schema, its
//! transitions and its one sync step, executed unchanged by native hosts
//! (rusqlite) and the browser (SQLite-WASM on OPFS). Hosts execute the bounded
//! SQL and the one gateway read this owner returns; they decide nothing.
//!
//! A head the authorized listing names for the first time is persisted at once
//! with its first-seen time, so every later read paints it immediately,
//! offline included. A refresh is one cheap source-basis comparison
//! ([`crate::freshness`]): an equal basis moves only the observation time; a
//! changed basis persists newcomers, moves changed heads (their retained exact
//! version stays until the bytes owner replaces it) and retires heads the
//! complete listing no longer names. A failed refresh retains and labels the
//! held rows; a refused credential revokes their reuse until a deliberate
//! refresh succeeds. Package bytes are never acquired here.
use crate::freshness::{self, Freshness};
use crate::printing::mv_models;
use crate::sql_effect::{ResultRows, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// The presentation projection every edge returns.
pub const SCHEMA: &str = "ds.grid-project-catalog/v1";
/// The generated gateway operation the browser's mechanical port sends.
pub const GATEWAY_OPERATION: &str = "domains.grid.models.action";
pub const PAGE_SIZE: u16 = 100;
/// The UI's read patience per page; a timeout leaves held rows available.
pub const PAGE_TIMEOUT_MS: u64 = 15_000;
pub const MAX_MODELS: usize = mv_models::MAX_MODELS;
pub const MAX_PAGES: usize = 64;
pub const MAX_HEAD_BYTES: usize = 16 * 1024;
pub const MAX_VERSION_BYTES: usize = 64 * 1024;
const MAX_CURSOR_BYTES: usize = 4096;
const MAX_HELD_BYTES: u32 = 48 * 1024 * 1024;
const MAX_SAFE: u64 = 9_007_199_254_740_991;

pub const INVALID: &str = "grid_catalog_invalid";
pub const VERSION_STALE: &str = "grid_catalog_version_stale";
pub const REVOKED: &str = "auth_context_mismatch";
pub const REFRESH_FAILED: &str = "refresh_failed";
pub const ACCESS_REFUSED: &str = "access_refused";

pub const TABLE_HEADS: &str = "CREATE TABLE IF NOT EXISTS grid_project_catalog_heads (scope TEXT NOT NULL, model_id TEXT NOT NULL, head TEXT NOT NULL, head_sha256 TEXT NOT NULL, first_seen_at_ms INTEGER NOT NULL CHECK(first_seen_at_ms>=0 AND first_seen_at_ms<=9007199254740991), retained_version TEXT, PRIMARY KEY(scope, model_id))";
pub const TABLE_OBSERVATIONS: &str = "CREATE TABLE IF NOT EXISTS grid_project_catalog_observations (scope TEXT PRIMARY KEY NOT NULL, basis_sha256 TEXT, observed_at_ms INTEGER, needs_refresh INTEGER NOT NULL CHECK(needs_refresh IN (0,1)), last_error TEXT, revoked INTEGER NOT NULL CHECK(revoked IN (0,1)), revision INTEGER NOT NULL CHECK(revision>0 AND revision<=9007199254740991), CHECK((basis_sha256 IS NULL)=(observed_at_ms IS NULL)))";

const OBSERVATION: &str = "SELECT basis_sha256,observed_at_ms,needs_refresh,last_error,revoked,revision FROM grid_project_catalog_observations WHERE scope=?1";
const HEADS: &str = "SELECT model_id,head,first_seen_at_ms,retained_version FROM grid_project_catalog_heads WHERE scope=?1 ORDER BY model_id";
const FENCE: &str =
    "SELECT revision,basis_sha256 FROM grid_project_catalog_observations WHERE scope=?1";
const HELD_IDS: &str = "SELECT model_id,head_sha256 FROM grid_project_catalog_heads WHERE scope=?1";
const RETIRE: &str = "DELETE FROM grid_project_catalog_heads WHERE scope=?1 AND model_id IN (SELECT value FROM json_each(?2))";
const UPSERT: &str = "INSERT INTO grid_project_catalog_heads(scope,model_id,head,head_sha256,first_seen_at_ms,retained_version) SELECT ?1,json_extract(value,'$.model_id'),json_extract(value,'$.head'),json_extract(value,'$.sha256'),?3,NULL FROM json_each(?2) WHERE true ON CONFLICT(scope,model_id) DO UPDATE SET head=excluded.head,head_sha256=excluded.head_sha256";
const OBSERVED: &str = "INSERT INTO grid_project_catalog_observations VALUES(?1,?2,?3,0,NULL,0,1) ON CONFLICT(scope) DO UPDATE SET basis_sha256=excluded.basis_sha256,observed_at_ms=excluded.observed_at_ms,needs_refresh=0,last_error=NULL,revoked=0,revision=grid_project_catalog_observations.revision+1";
const INVALIDATE: &str = "INSERT INTO grid_project_catalog_observations VALUES(?1,NULL,NULL,1,NULL,0,1) ON CONFLICT(scope) DO UPDATE SET needs_refresh=1,revision=grid_project_catalog_observations.revision+1";
const MARK_FAILED: &str = "UPDATE grid_project_catalog_observations SET needs_refresh=1,last_error=?2,revision=revision+1 WHERE scope=?1";
const REVOKE: &str = "INSERT INTO grid_project_catalog_observations VALUES(?1,NULL,NULL,1,?2,1,1) ON CONFLICT(scope) DO UPDATE SET needs_refresh=1,revoked=1,last_error=excluded.last_error,revision=grid_project_catalog_observations.revision+1";
const RETAIN_HEAD: &str =
    "SELECT head,head_sha256 FROM grid_project_catalog_heads WHERE scope=?1 AND model_id=?2";
const RETAIN: &str = "UPDATE grid_project_catalog_heads SET retained_version=?3 WHERE scope=?1 AND model_id=?2 AND head_sha256=?4";

/// The authoritative head fields a catalogue row keeps. One projection serves
/// the native client's list receipts and this store.
const HEAD_FIELDS: [&str; 22] = [
    "model_id",
    "display_name",
    "description",
    "model_kind",
    "state",
    "lifecycle",
    "head_revision_id",
    "head_version",
    "head_model_digest",
    "manifest_model_id",
    "manifest_model_revision",
    "design_stage_id",
    "detail_level_id",
    "approval_status",
    "approval_level_id",
    "created_at",
    "created_by",
    "updated_at",
    "updated_by",
    "deleted_at",
    "deleted_by",
    "delete_reason",
];

pub fn head_projection(row: &Value) -> Value {
    let mut head = json!({});
    for key in HEAD_FIELDS {
        if let Some(value) = row.get(key).filter(|v| !v.is_null()) {
            head[key] = value.clone();
        }
    }
    head
}

/// The closed `list_models` request every edge sends.
pub fn list_request(
    project: &str,
    limit: u16,
    cursor: Option<&str>,
    include_deleted: bool,
) -> Value {
    json!({"action":"list_models","project_id":project,"page_size":limit,"cursor":cursor.unwrap_or(""),"include_deleted":include_deleted})
}

/// Catalogue model identities: bounded, trimmed, no control or path separator.
pub fn model_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.trim() == value
}
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn canonical(value: &Value) -> Result<String, String> {
    let mut value = value.clone();
    value.sort_all_objects();
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub principal: String,
    pub lane: String,
    pub audience: String,
    pub project: String,
}
impl Scope {
    pub fn key(&self) -> Result<String, String> {
        if [&self.principal, &self.lane, &self.audience]
            .iter()
            .any(|s| !bounded(s, 500))
            || !crate::execution_context::valid_project(&self.project)
        {
            return Err("grid catalogue needs a complete identity and project fence".into());
        }
        serde_json::to_string(self).map_err(|e| e.to_string())
    }
}

// --- the authorized listing ------------------------------------------------

/// One decoded `list_models` page, already reduced to head projections.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub models: Vec<Value>,
    pub more: bool,
    pub next_cursor: String,
}
#[derive(Debug, PartialEq, Eq)]
pub enum PageError {
    /// The credential was refused (401/403): held rows must not be reused.
    Unauthorized,
    /// Any other refusal or unreachable answer: held rows stay labelled.
    Failed,
    Invalid,
}
/// Admit one page's `data` object for `project`, already unwrapped from its
/// envelope by the host's transport owner.
pub fn page_from_data(project: &str, data: &Value, limit: usize) -> Result<Page, PageError> {
    let rows = match &data["models"] {
        Value::Null => vec![],
        Value::Array(rows) => rows.clone(),
        _ => return Err(PageError::Invalid),
    };
    let more = data["has_more"].as_bool().ok_or(PageError::Invalid)?;
    let next_cursor = match &data["next_cursor"] {
        Value::Null => "",
        Value::String(cursor) => cursor.as_str(),
        _ => return Err(PageError::Invalid),
    };
    if rows.len() > limit
        || next_cursor.len() > MAX_CURSOR_BYTES
        || (more && next_cursor.is_empty())
    {
        return Err(PageError::Invalid);
    }
    let mut ids = BTreeSet::new();
    let mut models = Vec::with_capacity(rows.len());
    for row in rows {
        let model = row["model_id"]
            .as_str()
            .filter(|v| model_id(v))
            .ok_or(PageError::Invalid)?;
        if !ids.insert(model.to_owned()) || row["project_id"] != project {
            return Err(PageError::Invalid);
        }
        models.push(head_projection(&row));
    }
    Ok(Page {
        models,
        more,
        next_cursor: next_cursor.into(),
    })
}
/// Admit one gateway answer exactly as the browser's mechanical port observed it.
pub fn decode_page(project: &str, status: u16, body: &Value) -> Result<Page, PageError> {
    match status {
        200 | 201 => {}
        401 | 403 => return Err(PageError::Unauthorized),
        _ => return Err(PageError::Failed),
    }
    if body["success"] != true || !body["data"].is_object() {
        return Err(PageError::Invalid);
    }
    page_from_data(project, &body["data"], PAGE_SIZE as usize)
}

/// A bounded walk over every page of one project's heads. Empty pages with
/// `more` continue; repeated cursors, crossed projects and duplicate logical
/// identities refuse the whole walk.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Walk {
    project: String,
    cursor: Option<String>,
    seen: Vec<String>,
    models: Vec<Value>,
    pages: usize,
}
pub enum Walked {
    More(Walk),
    /// `{project, models, more:false, next_cursor:""}`.
    Complete(Value),
}
impl Walk {
    pub fn new(project: &str) -> Result<Self, String> {
        if !crate::execution_context::valid_project(project) {
            return Err(INVALID.into());
        }
        Ok(Self {
            project: project.into(),
            cursor: None,
            seen: vec![],
            models: vec![],
            pages: 0,
        })
    }
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }
    pub fn body(&self) -> Value {
        list_request(&self.project, PAGE_SIZE, self.cursor.as_deref(), false)
    }
    pub fn accept(mut self, page: Page) -> Result<Walked, String> {
        self.pages += 1;
        if self.pages > MAX_PAGES {
            return Err(INVALID.into());
        }
        if self.models.len() + page.models.len() > MAX_MODELS {
            return Err(INVALID.into());
        }
        let mut ids: BTreeSet<String> = self
            .models
            .iter()
            .filter_map(|held| held["model_id"].as_str().map(str::to_owned))
            .collect();
        for model in page.models {
            let id = model["model_id"].as_str().ok_or(INVALID)?;
            if !ids.insert(id.to_owned()) {
                return Err(INVALID.into());
            }
            self.models.push(model);
        }
        if self.models.len() > MAX_MODELS {
            return Err(INVALID.into());
        }
        if !page.more {
            return Ok(Walked::Complete(
                json!({"project":self.project,"models":self.models,"more":false,"next_cursor":""}),
            ));
        }
        if page.next_cursor.is_empty()
            || page.next_cursor.len() > MAX_CURSOR_BYTES
            || self.seen.contains(&page.next_cursor)
        {
            return Err(INVALID.into());
        }
        self.seen.push(page.next_cursor.clone());
        self.cursor = Some(page.next_cursor);
        Ok(Walked::More(self))
    }
}

/// One admitted listed head, canonical and digest-pinned.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Listed {
    pub model_id: String,
    pub head: String,
    pub sha256: String,
}
/// Admit a complete listing and compute its source basis: the digest of every
/// head's exact canonical projection, in model order.
pub fn admit_listing(project: &str, listing: &Value) -> Result<(Vec<Listed>, String), String> {
    if !crate::execution_context::valid_project(project)
        || listing["project"] != project
        || listing["more"] != false
    {
        return Err(INVALID.into());
    }
    let rows = listing["models"]
        .as_array()
        .filter(|rows| rows.len() <= MAX_MODELS)
        .ok_or(INVALID)?;
    let mut listed = BTreeMap::new();
    for row in rows {
        let head = head_projection(row);
        let id = head["model_id"]
            .as_str()
            .filter(|v| model_id(v))
            .ok_or(INVALID)?
            .to_owned();
        if !head["head_revision_id"]
            .as_str()
            .is_some_and(|v| bounded(v, 256))
        {
            return Err(INVALID.into());
        }
        if mv_models::is_project_mv_head(&head) {
            mv_models::admit_mv_head(&head).map_err(|e| e.to_string())?;
        }
        let text = canonical(&head)?;
        if text.len() > MAX_HEAD_BYTES {
            return Err(INVALID.into());
        }
        let sha256 = self::sha256(text.as_bytes());
        if listed
            .insert(
                id.clone(),
                Listed {
                    model_id: id,
                    head: text,
                    sha256,
                },
            )
            .is_some()
        {
            return Err(INVALID.into());
        }
    }
    let basis = sha256(
        serde_json::to_string(
            &listed
                .values()
                .map(|row| [row.model_id.as_str(), row.sha256.as_str()])
                .collect::<Vec<_>>(),
        )
        .map_err(|e| e.to_string())?
        .as_bytes(),
    );
    Ok((listed.into_values().collect(), basis))
}

// --- held state --------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub basis_sha256: Option<String>,
    pub observed_at_ms: Option<u64>,
    pub needs_refresh: bool,
    pub last_error: Option<String>,
    pub revoked: bool,
    pub revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HeldHead {
    pub head: Value,
    pub first_seen_at_ms: u64,
    pub retained_version: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Held {
    pub observation: Observation,
    pub heads: Vec<HeldHead>,
}
impl Held {
    /// The held complete listing, as the transport walk shapes one.
    pub fn listing(&self, project: &str) -> Value {
        json!({"project":project,"models":self.heads.iter().map(|row| row.head.clone()).collect::<Vec<_>>(),"more":false,"next_cursor":""})
    }
    /// Rows exist and may be served: observed once and not revoked.
    pub fn servable(&self) -> bool {
        self.observation.basis_sha256.is_some() && !self.observation.revoked
    }
}
/// What one commit changed, for receipts and first-arrival evidence.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Changes {
    /// Heads persisted for the first time by this commit.
    pub added: Vec<String>,
    pub moved: Vec<String>,
    pub retired: Vec<String>,
    pub unchanged: u64,
    /// The held source basis equalled the listing's: no row was written.
    pub fresh: bool,
    /// A concurrent commit moved the store first: nothing was written.
    pub superseded: bool,
}

fn scalar_u64(value: &Value, what: &str) -> Result<u64, String> {
    value
        .as_u64()
        .or_else(|| {
            value
                .as_f64()
                .filter(|v| v.fract() == 0.0 && *v >= 0.0 && *v <= MAX_SAFE as f64)
                .map(|v| v as u64)
        })
        .filter(|v| *v <= MAX_SAFE)
        .ok_or_else(|| format!("invalid catalogue {what}"))
}
fn flag(value: &Value, what: &str) -> Result<bool, String> {
    match scalar_u64(value, what)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(format!("invalid catalogue {what}")),
    }
}
fn optional_text(value: &Value, what: &str) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(format!("invalid catalogue {what}")),
    }
}
fn decode_observation(result: &ResultRows) -> Result<Option<Observation>, String> {
    let Some(row) = result.rows.first() else {
        return Ok(None);
    };
    if result.rows.len() != 1 || row.len() != 6 {
        return Err("invalid catalogue observation row".into());
    }
    let basis_sha256 = optional_text(&row[0], "basis")?;
    if basis_sha256
        .as_deref()
        .is_some_and(|v| !crate::printing::preview_progress::digest(v))
    {
        return Err("invalid catalogue basis".into());
    }
    let observed_at_ms = match &row[1] {
        Value::Null => None,
        value => Some(scalar_u64(value, "observation time")?),
    };
    let last_error = optional_text(&row[3], "error")?;
    if last_error
        .as_deref()
        .is_some_and(|v| ![REFRESH_FAILED, ACCESS_REFUSED, INVALID].contains(&v))
    {
        return Err("invalid catalogue error".into());
    }
    let revision = scalar_u64(&row[5], "revision")?;
    if revision == 0 || basis_sha256.is_some() != observed_at_ms.is_some() {
        return Err("invalid catalogue observation".into());
    }
    Ok(Some(Observation {
        basis_sha256,
        observed_at_ms,
        needs_refresh: flag(&row[2], "refresh flag")?,
        last_error,
        revoked: flag(&row[4], "revocation")?,
        revision,
    }))
}
fn decode_heads(result: &ResultRows) -> Result<Vec<HeldHead>, String> {
    let mut heads = Vec::with_capacity(result.rows.len());
    for row in &result.rows {
        if row.len() != 4 {
            return Err("invalid catalogue head row".into());
        }
        let id = row[0]
            .as_str()
            .filter(|v| model_id(v))
            .ok_or("invalid catalogue model")?;
        let text = row[1].as_str().ok_or("invalid catalogue head")?;
        if text.len() > MAX_HEAD_BYTES {
            return Err("catalogue head exceeds its bound".into());
        }
        let head: Value = serde_json::from_str(text).map_err(|_| "catalogue head is corrupt")?;
        if head["model_id"] != id {
            return Err("catalogue head identity mismatch".into());
        }
        let retained_version = match &row[3] {
            Value::Null => None,
            Value::String(text) if text.len() <= MAX_VERSION_BYTES => Some(
                serde_json::from_str(text).map_err(|_| "catalogue retained version is corrupt")?,
            ),
            _ => return Err("invalid catalogue retained version".into()),
        };
        heads.push(HeldHead {
            head,
            first_seen_at_ms: scalar_u64(&row[2], "first-seen time")?,
            retained_version,
        });
    }
    Ok(heads)
}

// --- storage operations ------------------------------------------------------

/// Closed storage operations. Each runs inside one host transaction and ends by
/// returning the held state after it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Schema,
    Read {
        scope: Scope,
    },
    Commit {
        scope: Scope,
        listing: Value,
        now: u64,
        expected: Option<u64>,
    },
    /// The bytes owner verified and holds this exact version of one held head.
    RetainVersion {
        scope: Scope,
        model_id: String,
        version: Value,
    },
    Invalidate {
        scope: Scope,
    },
    MarkFailed {
        scope: Scope,
        expected: Option<u64>,
    },
    Revoke {
        scope: Scope,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum Continuation {
    Schema,
    Finish,
    Observation {
        scope: Scope,
        changes: Option<Changes>,
    },
    Heads {
        observation: Observation,
        changes: Option<Changes>,
    },
    Fence {
        scope: Scope,
        listed: Vec<Listed>,
        basis: String,
        now: u64,
        expected: Option<u64>,
    },
    Diff {
        scope: Scope,
        listed: Vec<Listed>,
        basis: String,
        now: u64,
    },
    Retired {
        scope: Scope,
        upserts: Vec<Listed>,
        basis: String,
        now: u64,
        changes: Changes,
    },
    Upserted {
        scope: Scope,
        basis: String,
        now: u64,
        changes: Changes,
    },
    Written {
        scope: Scope,
        changes: Option<Changes>,
    },
    RetainFence {
        scope: Scope,
        model_id: String,
        version: String,
    },
    RetainHead {
        scope: Scope,
        model_id: String,
        version: String,
        revision: String,
        digest: String,
        number: u64,
    },
    MarkFence {
        scope: Scope,
        expected: Option<u64>,
    },
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    Sql {
        statement: Statement,
        continuation: Continuation,
    },
    Done {
        held: Option<Held>,
        changes: Option<Changes>,
    },
}
fn sql(statement: Statement, continuation: Continuation) -> Step {
    Step::Sql {
        statement,
        continuation,
    }
}
fn read_observation(scope: Scope, changes: Option<Changes>) -> Result<Step, String> {
    Ok(sql(
        Statement::new(OBSERVATION, vec![json!(scope.key()?)], 1, 64 * 1024),
        Continuation::Observation { scope, changes },
    ))
}
fn bare(digest: &str) -> String {
    digest
        .strip_prefix("sha256:")
        .unwrap_or(digest)
        .to_ascii_lowercase()
}
/// Admit an exact version the bytes owner holds: identity, revision, version
/// number and a verified package digest and length.
fn admit_version(model: &str, version: &Value) -> Result<(String, String, String, u64), String> {
    let text = canonical(version)?;
    let revision = version["revision_id"]
        .as_str()
        .filter(|v| bounded(v, 256))
        .ok_or(INVALID)?;
    let digest = version["model"]["digest"]
        .as_str()
        .map(bare)
        .filter(|v| crate::printing::preview_progress::digest(v))
        .ok_or(INVALID)?;
    let number = version["version"]
        .as_u64()
        .filter(|v| *v > 0)
        .ok_or(INVALID)?;
    if version["model_id"] != model
        || !version["model"]["byte_length"]
            .as_u64()
            .is_some_and(|v| v > 0)
        || text.len() > MAX_VERSION_BYTES
    {
        return Err(INVALID.into());
    }
    Ok((text, revision.into(), digest, number))
}
pub fn start(operation: Operation) -> Result<Step, String> {
    Ok(match operation {
        Operation::Schema => sql(
            Statement::new(TABLE_HEADS, vec![], 0, 2),
            Continuation::Schema,
        ),
        Operation::Read { scope } => read_observation(scope, None)?,
        Operation::Commit {
            scope,
            listing,
            now,
            expected,
        } => {
            if now > MAX_SAFE {
                return Err(INVALID.into());
            }
            let (listed, basis) = admit_listing(&scope.project, &listing)?;
            sql(
                Statement::new(FENCE, vec![json!(scope.key()?)], 1, 1024),
                Continuation::Fence {
                    scope,
                    listed,
                    basis,
                    now,
                    expected,
                },
            )
        }
        Operation::RetainVersion {
            scope,
            model_id,
            version,
        } => {
            admit_version(&model_id, &version)?;
            sql(
                Statement::new(OBSERVATION, vec![json!(scope.key()?)], 1, 64 * 1024),
                Continuation::RetainFence {
                    scope,
                    model_id,
                    version: canonical(&version)?,
                },
            )
        }
        Operation::Invalidate { scope } => sql(
            Statement::new(INVALIDATE, vec![json!(scope.key()?)], 0, 2),
            Continuation::Written {
                scope,
                changes: None,
            },
        ),
        Operation::MarkFailed { scope, expected } => sql(
            Statement::new(FENCE, vec![json!(scope.key()?)], 1, 1024),
            Continuation::MarkFence { scope, expected },
        ),
        Operation::Revoke { scope } => sql(
            Statement::new(
                REVOKE,
                vec![json!(scope.key()?), json!(ACCESS_REFUSED)],
                0,
                2,
            ),
            Continuation::Written {
                scope,
                changes: None,
            },
        ),
    })
}
fn no_rows(result: &ResultRows, what: &str) -> Result<(), String> {
    if result.rows.is_empty() {
        Ok(())
    } else {
        Err(format!("unexpected catalogue {what} rows"))
    }
}
pub fn resume(continuation: Continuation, result: ResultRows) -> Result<Step, String> {
    Ok(match continuation {
        Continuation::Schema => {
            no_rows(&result, "schema")?;
            sql(
                Statement::new(TABLE_OBSERVATIONS, vec![], 0, 2),
                Continuation::Finish,
            )
        }
        Continuation::Finish => {
            no_rows(&result, "write")?;
            Step::Done {
                held: None,
                changes: None,
            }
        }
        Continuation::Observation { scope, changes } => match decode_observation(&result)? {
            None => Step::Done {
                held: None,
                changes,
            },
            Some(observation) => sql(
                Statement::new(
                    HEADS,
                    vec![json!(scope.key()?)],
                    MAX_MODELS as u32,
                    MAX_HELD_BYTES,
                ),
                Continuation::Heads {
                    observation,
                    changes,
                },
            ),
        },
        Continuation::Heads {
            observation,
            changes,
        } => Step::Done {
            held: Some(Held {
                observation,
                heads: decode_heads(&result)?,
            }),
            changes,
        },
        Continuation::Fence {
            scope,
            listed,
            basis,
            now,
            expected,
        } => {
            let (current, held_basis) = match result.rows.as_slice() {
                [] => (None, None),
                [row] if row.len() == 2 => (
                    Some(scalar_u64(&row[0], "revision")?),
                    optional_text(&row[1], "basis")?,
                ),
                _ => return Err("invalid catalogue fence rows".into()),
            };
            if current != expected {
                // A concurrent commit answered first; its rows stand.
                return read_observation(
                    scope,
                    Some(Changes {
                        superseded: true,
                        ..Changes::default()
                    }),
                );
            }
            if freshness::compare(held_basis.as_deref(), Some(basis.as_str())) == Freshness::Fresh {
                let changes = Changes {
                    unchanged: listed.len() as u64,
                    fresh: true,
                    ..Changes::default()
                };
                sql(
                    Statement::new(
                        OBSERVED,
                        vec![json!(scope.key()?), json!(basis), json!(now)],
                        0,
                        2,
                    ),
                    Continuation::Written {
                        scope,
                        changes: Some(changes),
                    },
                )
            } else {
                sql(
                    Statement::new(
                        HELD_IDS,
                        vec![json!(scope.key()?)],
                        MAX_MODELS as u32,
                        4 * 1024 * 1024,
                    ),
                    Continuation::Diff {
                        scope,
                        listed,
                        basis,
                        now,
                    },
                )
            }
        }
        Continuation::Diff {
            scope,
            listed,
            basis,
            now,
        } => {
            let mut held = BTreeMap::new();
            for row in &result.rows {
                match row.as_slice() {
                    [Value::String(id), Value::String(sha)] => {
                        held.insert(id.clone(), sha.clone());
                    }
                    _ => return Err("invalid catalogue held identity row".into()),
                }
            }
            let names: BTreeSet<&str> = listed.iter().map(|row| row.model_id.as_str()).collect();
            let mut changes = Changes {
                retired: held
                    .keys()
                    .filter(|id| !names.contains(id.as_str()))
                    .cloned()
                    .collect(),
                ..Changes::default()
            };
            let mut upserts = Vec::new();
            for row in listed {
                match held.get(&row.model_id) {
                    None => changes.added.push(row.model_id.clone()),
                    Some(sha) if *sha != row.sha256 => changes.moved.push(row.model_id.clone()),
                    Some(_) => {
                        changes.unchanged += 1;
                        continue;
                    }
                }
                upserts.push(row);
            }
            if changes.retired.is_empty() {
                return resume(
                    Continuation::Retired {
                        scope,
                        upserts,
                        basis,
                        now,
                        changes,
                    },
                    ResultRows {
                        rows: vec![],
                        changes: 0,
                    },
                );
            }
            sql(
                Statement::new(
                    RETIRE,
                    vec![
                        json!(scope.key()?),
                        json!(serde_json::to_string(&changes.retired).map_err(|e| e.to_string())?),
                    ],
                    0,
                    2,
                ),
                Continuation::Retired {
                    scope,
                    upserts,
                    basis,
                    now,
                    changes,
                },
            )
        }
        Continuation::Retired {
            scope,
            upserts,
            basis,
            now,
            changes,
        } => {
            no_rows(&result, "retirement")?;
            if upserts.is_empty() {
                return resume(
                    Continuation::Upserted {
                        scope,
                        basis,
                        now,
                        changes,
                    },
                    ResultRows {
                        rows: vec![],
                        changes: 0,
                    },
                );
            }
            sql(
                Statement::new(
                    UPSERT,
                    vec![
                        json!(scope.key()?),
                        json!(serde_json::to_string(&upserts).map_err(|e| e.to_string())?),
                        json!(now),
                    ],
                    0,
                    2,
                ),
                Continuation::Upserted {
                    scope,
                    basis,
                    now,
                    changes,
                },
            )
        }
        Continuation::Upserted {
            scope,
            basis,
            now,
            changes,
        } => {
            no_rows(&result, "upsert")?;
            sql(
                Statement::new(
                    OBSERVED,
                    vec![json!(scope.key()?), json!(basis), json!(now)],
                    0,
                    2,
                ),
                Continuation::Written {
                    scope,
                    changes: Some(changes),
                },
            )
        }
        Continuation::Written { scope, changes } => {
            no_rows(&result, "write")?;
            read_observation(scope, changes)?
        }
        Continuation::RetainFence {
            scope,
            model_id,
            version,
        } => {
            let observation = decode_observation(&result)?;
            if observation.as_ref().is_none_or(|v| v.revoked) {
                return Err(REVOKED.into());
            }
            let parsed: Value = serde_json::from_str(&version).map_err(|e| e.to_string())?;
            let (_, revision, digest, number) = admit_version(&model_id, &parsed)?;
            sql(
                Statement::new(
                    RETAIN_HEAD,
                    vec![json!(scope.key()?), json!(model_id)],
                    1,
                    (MAX_HEAD_BYTES + 1024) as u32,
                ),
                Continuation::RetainHead {
                    scope,
                    model_id,
                    version,
                    revision,
                    digest,
                    number,
                },
            )
        }
        Continuation::RetainHead {
            scope,
            model_id,
            version,
            revision,
            digest,
            number,
        } => {
            let [row] = result.rows.as_slice() else {
                return Err(VERSION_STALE.into());
            };
            let (Some(text), Some(sha)) = (
                row.first().and_then(Value::as_str),
                row.get(1).and_then(Value::as_str),
            ) else {
                return Err("invalid catalogue retained head row".into());
            };
            let head: Value =
                serde_json::from_str(text).map_err(|_| "catalogue head is corrupt")?;
            if head["head_revision_id"] != revision.as_str()
                || head["head_model_digest"].as_str().map(bare).as_deref() != Some(digest.as_str())
                || head["head_version"].as_u64() != Some(number)
            {
                return Err(VERSION_STALE.into());
            }
            sql(
                Statement::new(
                    RETAIN,
                    vec![
                        json!(scope.key()?),
                        json!(model_id),
                        json!(version),
                        json!(sha),
                    ],
                    0,
                    2,
                ),
                Continuation::Written {
                    scope,
                    changes: None,
                },
            )
        }
        Continuation::MarkFence { scope, expected } => {
            let current = match result.rows.as_slice() {
                [] => None,
                [row] if row.len() == 2 => Some(scalar_u64(&row[0], "revision")?),
                _ => return Err("invalid catalogue fence rows".into()),
            };
            if current.is_none() || current != expected {
                // Nothing held to label, or a newer answer already stands.
                read_observation(scope, None)?
            } else {
                sql(
                    Statement::new(
                        MARK_FAILED,
                        vec![json!(scope.key()?), json!(REFRESH_FAILED)],
                        0,
                        2,
                    ),
                    Continuation::Written {
                        scope,
                        changes: None,
                    },
                )
            }
        }
    })
}

// --- the presentation projection -------------------------------------------

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Held,
    Refresh,
}
/// The one projection Design, Project Control and printing render: every held
/// head (newest change first) and the project's MV models (label order), with
/// the observation that produced them. Revoked or never-observed rows are not
/// served.
pub fn view(
    project: &str,
    held: Option<&Held>,
    source: Source,
    error: Option<&str>,
    changes: Option<&Changes>,
) -> Result<Value, String> {
    let observation = held.map(|held| &held.observation);
    let state = match held {
        None => "unread",
        Some(held) if held.observation.revoked => "revoked",
        Some(held) if held.observation.basis_sha256.is_none() => "unread",
        Some(held)
            if error.is_some()
                || held.observation.needs_refresh
                || held.observation.last_error.is_some() =>
        {
            "held"
        }
        Some(_) => "ready",
    };
    let served: &[HeldHead] = match held {
        Some(held) if held.servable() => &held.heads,
        _ => &[],
    };
    let mut heads = Vec::with_capacity(served.len());
    let mut mv = Vec::new();
    for row in served {
        let mut head = row.head.clone();
        if !head["model_id"].as_str().is_some_and(model_id) {
            return Err(INVALID.into());
        }
        head["first_seen_at_ms"] = json!(row.first_seen_at_ms);
        if mv_models::is_project_mv_head(&row.head) {
            let exact = mv_models::admit_mv_head(&row.head).map_err(|e| e.to_string())?;
            let retained_current = row.retained_version.as_ref().is_some_and(|version| {
                version["revision_id"] == exact.revision
                    && version["model"]["digest"].as_str().map(bare).as_deref()
                        == Some(exact.digest)
            });
            mv.push(json!({"model_id":exact.model_id,"label":exact.label,"head_revision_id":exact.revision,
                "head_model_digest":exact.digest,"head_version":exact.version,"first_seen_at_ms":row.first_seen_at_ms,
                "retained_version":row.retained_version,"retained_current":retained_current}));
        }
        heads.push(head);
    }
    heads.sort_by(|a, b| {
        b["updated_at"]
            .as_str()
            .unwrap_or("")
            .cmp(a["updated_at"].as_str().unwrap_or(""))
            .then_with(|| a["model_id"].as_str().cmp(&b["model_id"].as_str()))
    });
    mv.sort_by(|a, b| {
        let (la, lb) = (
            a["label"].as_str().unwrap_or(""),
            b["label"].as_str().unwrap_or(""),
        );
        la.to_lowercase()
            .cmp(&lb.to_lowercase())
            .then_with(|| la.cmp(lb))
            .then_with(|| a["model_id"].as_str().cmp(&b["model_id"].as_str()))
    });
    let last_error = error
        .map(str::to_owned)
        .or_else(|| observation.and_then(|v| v.last_error.clone()));
    Ok(json!({
        "schema": SCHEMA,
        "project": project,
        "observation": {
            "source": if source == Source::Refresh { "refresh" } else { "held" },
            "state": state,
            "observed_at_ms": observation.and_then(|v| v.observed_at_ms),
            "basis_sha256": observation.and_then(|v| v.basis_sha256.clone()),
            "revision": observation.map(|v| v.revision),
            "last_error": last_error,
        },
        "heads": heads,
        "mv_models": mv,
        "changes": changes,
    }))
}

// --- the browser sync step ---------------------------------------------------

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadMode {
    /// Paint the retained rows; no transport.
    Held,
    /// One authorized listing and its source-basis comparison.
    Refresh,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlowContinuation {
    Held {
        scope: Scope,
        mode: ReadMode,
    },
    Fetching {
        scope: Scope,
        expected: Option<u64>,
        walk: Walk,
    },
    Committed {
        scope: Scope,
    },
    Settled {
        scope: Scope,
        error: String,
    },
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlowStep {
    Storage {
        operation: Operation,
        continuation: FlowContinuation,
    },
    /// One gateway read for the host's mechanical port; it answers the
    /// observed status and untouched body, or a transport failure.
    Fetch {
        operation: String,
        body: Value,
        timeout_ms: u64,
        continuation: FlowContinuation,
    },
    Done {
        catalog: Value,
    },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlowResult {
    Storage {
        held: Option<Held>,
        changes: Option<Changes>,
    },
    Fetch {
        status: u16,
        body: Value,
        now: u64,
    },
    Failure,
}
pub fn flow_start(scope: Scope, mode: ReadMode) -> Result<FlowStep, String> {
    scope.key()?;
    Ok(FlowStep::Storage {
        operation: Operation::Read {
            scope: scope.clone(),
        },
        continuation: FlowContinuation::Held { scope, mode },
    })
}
fn settle(scope: Scope, expected: Option<u64>, error: &str, unauthorized: bool) -> FlowStep {
    FlowStep::Storage {
        operation: if unauthorized {
            Operation::Revoke {
                scope: scope.clone(),
            }
        } else {
            Operation::MarkFailed {
                scope: scope.clone(),
                expected,
            }
        },
        continuation: FlowContinuation::Settled {
            scope,
            error: error.into(),
        },
    }
}
fn fetch(scope: Scope, expected: Option<u64>, walk: Walk) -> FlowStep {
    FlowStep::Fetch {
        operation: GATEWAY_OPERATION.into(),
        body: walk.body(),
        timeout_ms: PAGE_TIMEOUT_MS,
        continuation: FlowContinuation::Fetching {
            scope,
            expected,
            walk,
        },
    }
}
pub fn flow_resume(continuation: FlowContinuation, result: FlowResult) -> Result<FlowStep, String> {
    Ok(match (continuation, result) {
        (FlowContinuation::Held { scope, mode }, FlowResult::Storage { held, .. }) => match mode {
            ReadMode::Held => FlowStep::Done {
                catalog: view(&scope.project, held.as_ref(), Source::Held, None, None)?,
            },
            ReadMode::Refresh => {
                let walk = Walk::new(&scope.project)?;
                fetch(scope, held.map(|v| v.observation.revision), walk)
            }
        },
        (
            FlowContinuation::Fetching {
                scope,
                expected,
                walk,
            },
            FlowResult::Fetch { status, body, now },
        ) => match decode_page(&scope.project, status, &body) {
            Err(PageError::Unauthorized) => settle(scope, expected, REVOKED, true),
            Err(PageError::Failed) => settle(scope, expected, REFRESH_FAILED, false),
            Err(PageError::Invalid) => settle(scope, expected, INVALID, false),
            Ok(page) => match walk.accept(page) {
                Err(_) => settle(scope, expected, INVALID, false),
                Ok(Walked::More(walk)) => fetch(scope, expected, walk),
                Ok(Walked::Complete(listing)) => {
                    if admit_listing(&scope.project, &listing).is_err() || now > MAX_SAFE {
                        settle(scope, expected, INVALID, false)
                    } else {
                        FlowStep::Storage {
                            operation: Operation::Commit {
                                scope: scope.clone(),
                                listing,
                                now,
                                expected,
                            },
                            continuation: FlowContinuation::Committed { scope },
                        }
                    }
                }
            },
        },
        (
            FlowContinuation::Fetching {
                scope, expected, ..
            },
            FlowResult::Failure,
        ) => settle(scope, expected, REFRESH_FAILED, false),
        (FlowContinuation::Committed { scope }, FlowResult::Storage { held, changes }) => {
            let source = if changes.as_ref().is_some_and(|c| c.superseded) {
                Source::Held
            } else {
                Source::Refresh
            };
            FlowStep::Done {
                catalog: view(
                    &scope.project,
                    held.as_ref(),
                    source,
                    None,
                    changes.as_ref(),
                )?,
            }
        }
        (FlowContinuation::Settled { scope, error }, FlowResult::Storage { held, .. }) => {
            FlowStep::Done {
                catalog: view(
                    &scope.project,
                    held.as_ref(),
                    Source::Held,
                    Some(&error),
                    None,
                )?,
            }
        }
        _ => return Err("invalid grid catalogue continuation".into()),
    })
}

#[derive(Deserialize)]
#[serde(tag = "call", rename_all = "snake_case", deny_unknown_fields)]
enum Call {
    Start {
        operation: Operation,
    },
    Resume {
        continuation: Continuation,
        result: ResultRows,
    },
    FlowStart {
        scope: Scope,
        mode: ReadMode,
    },
    FlowResume {
        continuation: FlowContinuation,
        result: FlowResult,
    },
}
/// The one WASM entry: storage steps for the browser storage worker and the
/// sync step for the page.
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    let value = match serde_json::from_slice(input).map_err(|e| e.to_string())? {
        Call::Start { operation } => serde_json::to_value(start(operation)?),
        Call::Resume {
            continuation,
            result,
        } => serde_json::to_value(resume(continuation, result)?),
        Call::FlowStart { scope, mode } => serde_json::to_value(flow_start(scope, mode)?),
        Call::FlowResume {
            continuation,
            result,
        } => serde_json::to_value(flow_resume(continuation, result)?),
    }
    .map_err(|e| e.to_string())?;
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "grid_project_catalog_tests.rs"]
mod tests;
