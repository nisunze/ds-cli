//! Durable Survey mutation bytes. The kernel alone owns replay decisions.
use crate::{Error, Result, Store};
use ds_command_kernel::survey_entry_outbox::{self as kernel, Fence, Header, PayloadPatch};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const MAX_SURVEY_ENTRY_ROWS: usize = 10_000;
pub const MAX_SURVEY_ENTRY_BATCH: usize = 500;
pub const MAX_SURVEY_ENTRY_BYTES: usize = 8 * 1024 * 1024;
const MAX_BATCH_BYTES: usize = 32 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_SAFE_INT: u64 = 9_007_199_254_740_991;
const TABLE: &str = "CREATE TABLE IF NOT EXISTS survey_entry_outbox(
 account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
 id TEXT NOT NULL, project TEXT NOT NULL, form TEXT NOT NULL, doc TEXT NOT NULL,
 operation TEXT NOT NULL, header TEXT NOT NULL, row TEXT NOT NULL,
 PRIMARY KEY(account,deployment,install_id,id));
 CREATE INDEX IF NOT EXISTS survey_entry_project ON survey_entry_outbox(account,deployment,install_id,project,form,doc);
 CREATE UNIQUE INDEX IF NOT EXISTS survey_entry_create ON survey_entry_outbox(account,deployment,install_id,project,form,doc) WHERE operation='create';";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SurveyEntryScope {
    pub account: String,
    pub deployment: String,
    pub install_id: String,
}

fn refused(detail: &str) -> Error {
    Error::Kernel(format!("survey_entry_store_refused: {detail}"))
}
fn text(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
impl SurveyEntryScope {
    pub fn validate(&self) -> Result<()> {
        if !text(&self.account, 512)
            || !text(&self.deployment, 2048)
            || !text(&self.install_id, 512)
        {
            return Err(refused("invalid account, deployment or installation"));
        }
        Ok(())
    }
}
fn has_table(connection: &Connection) -> Result<bool> {
    let kind: Option<String> = connection
        .query_row(
            "SELECT type FROM sqlite_master WHERE name='survey_entry_outbox'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match kind.as_deref() {
        None => Ok(false),
        Some("table") => Ok(true),
        _ => Err(refused("Survey outbox is not a table")),
    }
}

/// Project only the exact Kernel Header; opaque payload/overlay fields never
/// enter a scheduler request. Nullable optional browser fields keep their
/// established absence semantics; no owner or generation is manufactured.
fn project_header(row: &Value) -> Result<Header> {
    let object = row
        .as_object()
        .ok_or_else(|| refused("Survey row must be an object"))?;
    if object.get("kind").is_some_and(|v| v != "survey")
        || !object.get("payload").is_some_and(Value::is_object)
        || object
            .get("overlay")
            .is_some_and(|v| !v.is_null() && !v.is_object())
    {
        return Err(refused("invalid Survey kind, payload or overlay"));
    }
    let mut out = Map::new();
    for key in [
        "id",
        "project_id",
        "form_id",
        "doc_id",
        "operation",
        "idempotency_key",
        "created_at",
        "owner_uid",
        "generation",
        "lease",
        "status",
        "attempts",
        "next_attempt_at",
        "conflict_rebased",
        "error",
        "failure_code",
        "subeffect_phase",
        "legacy_confirmed",
    ] {
        if let Some(value) = object.get(key) {
            out.insert(key.into(), value.clone());
        }
    }
    if out.get("doc_id").is_none_or(Value::is_null) {
        if let Some(value) = object.get("entry_id") {
            out.insert("doc_id".into(), value.clone());
        }
    }
    for (key, default) in [
        ("lease", json!(0)),
        ("next_attempt_at", json!(0)),
        ("conflict_rebased", json!(false)),
        ("legacy_confirmed", json!(false)),
    ] {
        if out.get(key).is_none_or(Value::is_null) {
            out.insert(key.into(), default);
        }
    }
    let header: Header = serde_json::from_value(Value::Object(out))?;
    if serde_json::to_vec(&header)?.len() > MAX_HEADER_BYTES {
        return Err(refused("Survey header exceeds its byte bound"));
    }
    Ok(header)
}
fn header_equal(left: &Header, right: &Header) -> Result<bool> {
    Ok(serde_json::to_value(left)? == serde_json::to_value(right)?)
}
fn owned(scope: &SurveyEntryScope, header: &Header) -> Result<()> {
    if header.owner_uid.as_deref() != Some(scope.account.as_str()) {
        return Err(refused("foreign or ownerless Survey row"));
    }
    Ok(())
}
fn admitted(scope: &SurveyEntryScope, headers: &[Header], now: u64) -> Result<Vec<Header>> {
    let request = json!({"schema":kernel::SCHEMA, "now":now, "headers":[],
        "event":{"kind":"admit_batch", "current_uid":scope.account, "headers":headers}});
    let reply = kernel::evaluate(&serde_json::to_vec(&request)?).map_err(Error::Kernel)?;
    let reply: Value = serde_json::from_str(&reply)?;
    let actions = reply["actions"]
        .as_array()
        .ok_or_else(|| refused("invalid Kernel admission reply"))?;
    let mut out = Vec::with_capacity(headers.len());
    for action in actions {
        if action["kind"] != "admitted" {
            return Err(refused("Kernel refused Survey admission"));
        }
        out.push(serde_json::from_value(action["header"].clone())?);
    }
    if out.len() != headers.len() {
        return Err(refused("incomplete Kernel admission reply"));
    }
    Ok(out)
}
fn validate_owned(scope: &SurveyEntryScope, headers: &[Header]) -> Result<()> {
    let mut projects = std::collections::BTreeMap::<&str, Vec<Header>>::new();
    for header in headers {
        owned(scope, header)?;
        projects
            .entry(&header.project_id)
            .or_default()
            .push(header.clone());
    }
    for batch in projects.values() {
        admitted(scope, batch, 0)?;
    }
    Ok(())
}
fn encode(row: &Value) -> Result<String> {
    let raw = serde_json::to_string(row)?;
    if raw.len() > MAX_SURVEY_ENTRY_BYTES {
        return Err(refused("Survey row exceeds 8 MiB"));
    }
    Ok(raw)
}
fn prepare(scope: &SurveyEntryScope, rows: &[Value]) -> Result<Vec<Header>> {
    scope.validate()?;
    if rows.is_empty() || rows.len() > MAX_SURVEY_ENTRY_BATCH {
        return Err(refused("Survey batch must contain 1..500 rows"));
    }
    let mut bytes = 0usize;
    let mut ids = std::collections::BTreeSet::new();
    let mut headers = Vec::with_capacity(rows.len());
    for row in rows {
        bytes = bytes.saturating_add(encode(row)?.len());
        if bytes > MAX_BATCH_BYTES {
            return Err(refused("Survey batch exceeds 32 MiB"));
        }
        let header = project_header(row)?;
        owned(scope, &header)?;
        if !ids.insert(header.id.clone()) {
            return Err(refused("duplicate Survey row ID in batch"));
        }
        headers.push(header);
    }
    Ok(headers)
}
fn merge_header(row: &mut Value, header: &Header) -> Result<()> {
    let object = row
        .as_object_mut()
        .ok_or_else(|| refused("invalid stored Survey object"))?;
    // Remove old optional lifecycle fields before applying an exact Kernel Header.
    for key in [
        "owner_uid",
        "generation",
        "error",
        "failure_code",
        "subeffect_phase",
    ] {
        object.remove(key);
    }
    let Value::Object(fields) = serde_json::to_value(header)? else {
        unreachable!()
    };
    object.extend(fields);
    // OutboxEntry preserves the established nullable error field.
    object.entry("error").or_insert(Value::Null);
    Ok(())
}
fn insert(
    connection: &Connection,
    scope: &SurveyEntryScope,
    header: &Header,
    raw: &str,
) -> Result<()> {
    connection.execute("INSERT INTO survey_entry_outbox(account,deployment,install_id,id,project,form,doc,operation,header,row) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![scope.account,scope.deployment,scope.install_id,header.id,header.project_id,header.form_id,header.doc_id,serde_json::to_value(&header.operation)?.as_str(),serde_json::to_string(header)?,raw])?;
    Ok(())
}
fn capacity(connection: &Connection, scope: &SurveyEntryScope) -> Result<()> {
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM survey_entry_outbox WHERE account=?1 AND deployment=?2 AND install_id=?3", params![scope.account,scope.deployment,scope.install_id], |r| r.get(0))?;
    if count > MAX_SURVEY_ENTRY_ROWS as i64 {
        return Err(refused("Survey header capacity exceeds 10000 rows"));
    }
    Ok(())
}
fn read_row(
    connection: &Connection,
    scope: &SurveyEntryScope,
    id: &str,
) -> Result<Option<(String, Header)>> {
    let held: Option<(Option<String>, Option<String>)> = connection.query_row(
        "SELECT CASE WHEN length(CAST(row AS BLOB))<=8388608 THEN row END, CASE WHEN length(CAST(header AS BLOB))<=65536 THEN header END FROM survey_entry_outbox WHERE account=?1 AND deployment=?2 AND install_id=?3 AND id=?4",
        params![scope.account,scope.deployment,scope.install_id,id], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    let Some((raw, header)) = held else {
        return Ok(None);
    };
    let raw = raw.ok_or_else(|| refused("stored Survey row exceeds its bound"))?;
    let header: Header = serde_json::from_str(
        &header.ok_or_else(|| refused("stored Survey header exceeds its bound"))?,
    )?;
    owned(scope, &header)?;
    let row: Value = serde_json::from_str(&raw)?;
    if header.id != id || !header_equal(&header, &project_header(&row)?)? {
        return Err(refused("stored Survey row/header identity mismatch"));
    }
    Ok(Some((raw, header)))
}
fn matches(header: &Header, expected: &Fence) -> bool {
    header.id == expected.id
        && header.owner_uid == expected.owner_uid
        && header.generation == expected.generation
        && header.lease == expected.lease
        && header.status == expected.status
        && header.subeffect_phase == expected.subeffect_phase
}
fn immutable(left: &Header, right: &Header) -> bool {
    left.id == right.id
        && left.project_id == right.project_id
        && left.form_id == right.form_id
        && left.doc_id == right.doc_id
        && left.operation == right.operation
        && left.idempotency_key == right.idempotency_key
        && left.created_at == right.created_at
}

fn headers_on(
    connection: &Connection,
    scope: &SurveyEntryScope,
    project: Option<&str>,
) -> Result<Vec<Header>> {
    if !has_table(connection)? {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare("SELECT CASE WHEN length(CAST(header AS BLOB))<=65536 THEN header END FROM survey_entry_outbox WHERE account=?1 AND deployment=?2 AND install_id=?3 AND (?4 IS NULL OR project=?4) ORDER BY id LIMIT 10001")?;
    let rows = statement.query_map(
        params![scope.account, scope.deployment, scope.install_id, project],
        |r| r.get::<_, Option<String>>(0),
    )?;
    let mut out = Vec::new();
    for raw in rows {
        if out.len() == MAX_SURVEY_ENTRY_ROWS {
            return Err(refused("Survey header capacity exceeds 10000 rows"));
        }
        let raw = raw?.ok_or_else(|| refused("stored Survey header exceeds its bound"))?;
        let header: Header = serde_json::from_str(&raw)?;
        owned(scope, &header)?;
        if project.is_some_and(|p| p != header.project_id) {
            return Err(refused("stored Survey project mismatch"));
        }
        out.push(header);
    }
    Ok(out)
}
fn cas_apply_on(
    connection: &Connection,
    scope: &SurveyEntryScope,
    expected: &Fence,
    header: &Header,
    patch: Option<&PayloadPatch>,
) -> Result<bool> {
    let Some((raw, held)) = read_row(connection, scope, &expected.id)? else {
        return Ok(false);
    };
    if !matches(&held, expected) || !immutable(&held, header) {
        return Ok(false);
    }
    validate_owned(scope, std::slice::from_ref(header))?;
    if patch.is_some_and(|p| p.expected_version > MAX_SAFE_INT) {
        return Err(refused("invalid expected_version patch"));
    }
    let mut row: Value = serde_json::from_str(&raw)?;
    merge_header(&mut row, header)?;
    if let Some(patch) = patch {
        row["payload"]["expected_version"] = json!(patch.expected_version);
    }
    let changed = connection.execute("UPDATE survey_entry_outbox SET header=?1,row=?2 WHERE account=?3 AND deployment=?4 AND install_id=?5 AND id=?6", params![serde_json::to_string(header)?,encode(&row)?,scope.account,scope.deployment,scope.install_id,expected.id])?;
    Ok(changed == 1)
}
fn cas_remove_on(
    connection: &Connection,
    scope: &SurveyEntryScope,
    expected: &Fence,
) -> Result<bool> {
    let Some((_, held)) = read_row(connection, scope, &expected.id)? else {
        return Ok(false);
    };
    if !matches(&held, expected) {
        return Ok(false);
    }
    let removed = connection.execute("DELETE FROM survey_entry_outbox WHERE account=?1 AND deployment=?2 AND install_id=?3 AND id=?4", params![scope.account,scope.deployment,scope.install_id,expected.id])?;
    Ok(removed == 1)
}

impl Store {
    /// Evaluate Kernel batch admission and atomically retain every resulting row.
    pub fn survey_entry_admit_batch(
        &mut self,
        scope: &SurveyEntryScope,
        mut rows: Vec<Value>,
        now: u64,
    ) -> Result<()> {
        let headers = prepare(scope, &rows)?;
        let admitted = admitted(scope, &headers, now)?;
        for (row, header) in rows.iter_mut().zip(&admitted) {
            merge_header(row, header)?;
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(TABLE)?;
        for (row, header) in rows.iter().zip(&admitted) {
            insert(&transaction, scope, header, &encode(row)?)?;
        }
        capacity(&transaction, scope)?;
        transaction.commit()?;
        Ok(())
    }

    /// Header-only bounded read; never loads payloads or creates a table.
    pub fn survey_entry_headers(
        &self,
        scope: &SurveyEntryScope,
        project: Option<&str>,
    ) -> Result<Vec<Header>> {
        scope.validate()?;
        if project.is_some_and(|p| !text(p, 1500)) {
            return Err(refused("invalid project"));
        }
        headers_on(&self.connection, scope, project)
    }

    pub fn survey_entry_row(&self, scope: &SurveyEntryScope, id: &str) -> Result<Option<Value>> {
        scope.validate()?;
        if !text(id, 4096) {
            return Err(refused("invalid Survey row ID"));
        }
        if !has_table(&self.connection)? {
            return Ok(None);
        }
        read_row(&self.connection, scope, id)?
            .map(|(raw, _)| serde_json::from_str(&raw).map_err(Error::from))
            .transpose()
    }

    /// Apply only the supplied Kernel lifecycle/header and version patch,
    /// protected by the exact owner/generation/lease/status/phase fence.
    pub fn survey_entry_cas_apply(
        &mut self,
        scope: &SurveyEntryScope,
        expected: &Fence,
        header: &Header,
        patch: Option<&PayloadPatch>,
    ) -> Result<bool> {
        scope.validate()?;
        if !text(&expected.id, 4096) {
            return Err(refused("invalid Survey row ID"));
        }
        if !has_table(&self.connection)? {
            return Ok(false);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = cas_apply_on(&transaction, scope, expected, header, patch)?;
        transaction.commit()?;
        Ok(changed)
    }

    pub fn survey_entry_cas_remove(
        &mut self,
        scope: &SurveyEntryScope,
        expected: &Fence,
    ) -> Result<bool> {
        scope.validate()?;
        if !text(&expected.id, 4096) {
            return Err(refused("invalid Survey row ID"));
        }
        if !has_table(&self.connection)? {
            return Ok(false);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = cas_remove_on(&transaction, scope, expected)?;
        transaction.commit()?;
        Ok(removed)
    }

    /// Persist the Kernel's exact project-scoped transition before exposing any
    /// dispatch/auth/version effects. No transaction spans an external effect.
    pub fn survey_entry_transition(
        &mut self,
        scope: &SurveyEntryScope,
        project: &str,
        event: Value,
        now: u64,
    ) -> Result<Value> {
        scope.validate()?;
        if !text(project, 1500) {
            return Err(refused("invalid project"));
        }
        let object = event
            .as_object()
            .ok_or_else(|| refused("invalid Survey event"))?;
        if matches!(
            object.get("kind").and_then(Value::as_str),
            Some("admit" | "admit_batch")
        ) {
            return Err(refused(
                "Survey admission requires the opaque payload admission API",
            ));
        }
        if object
            .get("current_uid")
            .is_some_and(|v| v.as_str() != Some(scope.account.as_str()))
        {
            return Err(refused(
                "Survey event account does not match captured scope",
            ));
        }
        if object
            .get("project_id")
            .is_some_and(|v| v.as_str() != Some(project))
        {
            return Err(refused(
                "Survey event project does not match requested project",
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let headers = headers_on(&transaction, scope, Some(project))?;
        let request = json!({"schema":kernel::SCHEMA,"now":now,"headers":headers,"event":event});
        let raw = kernel::evaluate(&serde_json::to_vec(&request)?).map_err(Error::Kernel)?;
        let reply: Value = serde_json::from_str(&raw)?;
        let actions = reply["actions"]
            .as_array()
            .ok_or_else(|| refused("invalid Kernel transition reply"))?;
        for action in actions {
            if let Some(raw_header) = action.get("header") {
                let header: Header = serde_json::from_value(raw_header.clone())?;
                owned(scope, &header)?;
                if header.project_id != project {
                    return Err(refused("Kernel effect escaped requested project"));
                }
            }
            match action["kind"].as_str() {
                Some("persist") => {
                    let expected: Fence = serde_json::from_value(action["expected"].clone())?;
                    let header: Header = serde_json::from_value(action["header"].clone())?;
                    if !headers.iter().any(|h| h.id == expected.id) {
                        return Err(refused("Kernel write escaped requested project"));
                    }
                    let patch = action
                        .get("payload_patch")
                        .map(|value| {
                            value["expected_version"]
                                .as_u64()
                                .map(|expected_version| PayloadPatch { expected_version })
                                .ok_or_else(|| refused("invalid Kernel payload patch"))
                        })
                        .transpose()?;
                    if !cas_apply_on(&transaction, scope, &expected, &header, patch.as_ref())? {
                        return Err(refused("Kernel transition fence no longer matches"));
                    }
                }
                Some("remove") => {
                    let expected: Fence = serde_json::from_value(action["expected"].clone())?;
                    if !headers.iter().any(|h| h.id == expected.id) {
                        return Err(refused("Kernel removal escaped requested project"));
                    }
                    if !cas_remove_on(&transaction, scope, &expected)? {
                        return Err(refused("Kernel removal fence no longer matches"));
                    }
                }
                Some(
                    "dispatch" | "wake_at" | "refresh_auth" | "read_entry_version" | "surface",
                ) => {}
                _ => return Err(refused("unsupported Kernel Survey action")),
            }
        }
        transaction.commit()?;
        Ok(reply)
    }
}

#[cfg(test)]
mod tests;
