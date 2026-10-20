//! Authored working copies. Metadata-only changes never load or replace bytes;
//! checkpoints and removals commit with the rest of the local workspace.
use super::*;
use crate::byte_parts::{Blobs, Received, Share, Stage};

const TABLE: &str = "CREATE TABLE IF NOT EXISTS grid_models (owner TEXT NOT NULL, model_id TEXT NOT NULL, updated_ms INTEGER NOT NULL, record TEXT NOT NULL, blob INTEGER NOT NULL, PRIMARY KEY(owner,model_id))";
const CONTENT: Blobs = Blobs {
    blobs: "grid_models_blobs",
    parts: "grid_models_parts",
    code: INVALID,
    content_addressed: false,
};
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_LISTED: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub ds_project: String,
    pub model_id: String,
    pub revision_id: String,
    pub version: u64,
    pub digest: String,
}
impl Binding {
    fn admit(&self) -> Result<(), String> {
        if !crate::execution_context::valid_project(&self.ds_project)
            || !exact(&self.model_id, MAX_ID)
            || !exact(&self.revision_id, MAX_TEXT)
            || self.version == 0
            || self.version > MAX_SAFE
            || self.digest.len() != 64
            || !self
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid(
                "a project binding names its exact project, model, version and digest",
            ));
        }
        Ok(())
    }
}

/// Display/provenance metadata supplied by the worker's caller. Size and
/// digest are derived from the committed payload, never trusted from the UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub model_id: String,
    pub name: String,
    pub revision: String,
    pub updated: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_owner: Option<WorkspaceOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_binding: Option<Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_project_origin: Option<Binding>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    #[serde(flatten)]
    pub model: Model,
    pub size_bytes: u64,
    pub content_digest: String,
}
impl Model {
    fn admit(&self) -> Result<(), String> {
        id(&self.model_id, "a model id")?;
        if !exact(&self.name, MAX_TEXT)
            || !exact(&self.revision, MAX_TEXT)
            || self.updated > MAX_SAFE
            || self
                .manifest_model_id
                .as_deref()
                .is_some_and(|v| !exact(v, MAX_ID))
            || self
                .imported_from
                .as_deref()
                .is_some_and(|v| !exact(v, MAX_TEXT))
        {
            return Err(invalid(
                "a working copy names its model, display name, revision and time",
            ));
        }
        for binding in [&self.project_binding, &self.retired_project_origin]
            .into_iter()
            .flatten()
        {
            binding.admit()?;
        }
        Ok(())
    }
}

pub(super) fn schema(connection: &Connection) -> Result<(), String> {
    connection.execute_batch(TABLE).map_err(sql)?;
    connection.execute_batch(&CONTENT.schema()).map_err(sql)
}
fn held(
    connection: &Connection,
    owner: &str,
    model: &str,
) -> Result<Option<(Record, i64)>, String> {
    id(model, "a model id")?;
    connection
        .query_row(
            "SELECT record,blob FROM grid_models WHERE owner=?1 AND model_id=?2",
            params![owner, model],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(sql)?
        .map(|(text, blob)| {
            serde_json::from_str(&text)
                .map(|record| (record, blob))
                .map_err(|_| invalid("a stored working copy is corrupt"))
        })
        .transpose()
}
fn write(connection: &Connection, owner: &str, record: &Record, blob: i64) -> Result<(), String> {
    record.model.admit()?;
    connection.execute(
        "INSERT INTO grid_models(owner,model_id,updated_ms,record,blob) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner,model_id) DO UPDATE SET updated_ms=excluded.updated_ms,record=excluded.record,blob=excluded.blob",
        params![owner, record.model.model_id, time(record.model.updated)?, text(record, 64 * 1024, "a working copy")?, blob],
    ).map_err(sql)?;
    Ok(())
}
fn answer(value: Value, wrote: bool) -> Outcome {
    kind(
        if wrote {
            Outcome::written(fields(value))
        } else {
            Outcome::read(fields(value))
        },
        "models",
    )
}
pub(super) fn checkpoint(
    connection: &Connection,
    owner: &str,
    mut model: Model,
    share: Share,
    bytes: Option<Vec<u8>>,
) -> Result<Outcome, String> {
    model.admit()?;
    let operation = text(&(&model, share.byte_length), 64 * 1024, "a checkpoint")?;
    let stage = Stage {
        code: INVALID,
        owner,
        store: STORE,
    };
    let payload = match stage.receive(
        connection,
        &operation,
        share,
        bytes,
        MAX_BYTES,
        model.updated,
    )? {
        Received::Staged {
            staging,
            received,
            parts,
        } => {
            return Ok(answer(
                Received::staged_answer(staging, received, parts),
                false,
            ))
        }
        Received::Complete(payload) => payload,
    };
    let previous = held(connection, owner, &model.model_id)?;
    if previous.is_none() {
        let count: i64 = connection
            .query_row(
                "SELECT count(*) FROM grid_models WHERE owner=?1",
                params![owner],
                |row| row.get(0),
            )
            .map_err(sql)?;
        if count >= MAX_LISTED as i64 {
            return Err(invalid("the working copy catalog is full"));
        }
    }
    if let Some((previous, _)) = &previous {
        if previous.model.retired_project_origin.is_some() {
            model.retired_project_origin = previous.model.retired_project_origin.clone();
        }
    }
    let digest = payload.sha256(connection, INVALID)?;
    let blob = CONTENT.write(connection, owner, &payload, &digest)?;
    let record = Record {
        model,
        size_bytes: blob.byte_length,
        content_digest: digest,
    };
    write(connection, owner, &record, blob.id)?;
    if let Some((_, old_blob)) = previous {
        CONTENT.delete(connection, old_blob)?;
    }
    payload.release(connection, INVALID)?;
    Ok(answer(json!({"model": record}), true))
}
pub(super) fn list(connection: &Connection, owner: &str) -> Result<Vec<Record>, String> {
    let records = rows(connection,
        "SELECT record FROM grid_models WHERE owner=?1 ORDER BY updated_ms DESC,model_id LIMIT 10001", &[&owner])?;
    if records.len() > MAX_LISTED {
        return Err(invalid(
            "the working copy catalog exceeds its listing bound",
        ));
    }
    Ok(records)
}
pub(super) fn get(
    connection: &Connection,
    owner: &str,
    model: &str,
    part: u64,
    expected: Option<&str>,
) -> Result<Outcome, String> {
    let Some((record, blob_id)) = held(connection, owner, model)? else {
        return Ok(answer(json!({"model": null}), false));
    };
    if expected.is_some_and(|expected| expected != record.content_digest) {
        return Err(invalid("the working copy changed during its read"));
    }
    let blob = CONTENT
        .get(connection, blob_id)?
        .ok_or_else(|| invalid("the working copy bytes are missing"))?;
    let bytes = CONTENT
        .part(connection, &blob, part)?
        .ok_or_else(|| invalid("the working copy part is missing or corrupt"))?;
    Ok(Outcome {
        bytes: Some(bytes),
        ..answer(
            json!({"model": record, "parts": blob.parts, "version": blob.sha256}),
            false,
        )
    })
}
pub(super) fn rename(
    connection: &Connection,
    owner: &str,
    model: &str,
    name: &str,
) -> Result<Outcome, String> {
    let (mut record, blob) =
        held(connection, owner, model)?.ok_or_else(|| invalid("no such local working copy"))?;
    if !name.trim().is_empty() {
        record.model.name = name.trim().into();
    }
    write(connection, owner, &record, blob)?;
    Ok(answer(json!({"model": record}), true))
}
pub(super) fn bind(
    connection: &Connection,
    owner: &str,
    model: &str,
    binding: Binding,
    workspace: Option<WorkspaceOwner>,
) -> Result<Outcome, String> {
    binding.admit()?;
    let Some((mut record, blob)) = held(connection, owner, model)? else {
        return Ok(answer(json!({"bound": false}), false));
    };
    record.model.project_binding = Some(binding);
    if workspace.is_some() {
        record.model.workspace_owner = workspace;
    }
    write(connection, owner, &record, blob)?;
    Ok(answer(json!({"bound": true}), true))
}
pub(super) fn project_models(
    connection: &Connection,
    owner: &str,
    project: &str,
    model: &str,
    local: Option<&str>,
) -> Result<Outcome, String> {
    if !crate::execution_context::valid_project(project) {
        return Err(invalid("an exact project is required"));
    }
    id(model, "a project model id")?;
    let records = if let Some(local) = local {
        held(connection, owner, local)?
            .into_iter()
            .map(|(r, _)| r)
            .collect()
    } else {
        list(connection, owner)?
    };
    let mut changed = Vec::new();
    for mut record in records {
        if !record
            .model
            .project_binding
            .as_ref()
            .is_some_and(|b| b.ds_project == project && b.model_id == model)
        {
            continue;
        }
        let (_, blob) = held(connection, owner, &record.model.model_id)?
            .ok_or_else(|| invalid("a working copy disappeared"))?;
        if local.is_some() {
            record.model.workspace_owner = Some(WorkspaceOwner::MvDesign);
        } else {
            record.model.retired_project_origin = record.model.project_binding.take();
        }
        write(connection, owner, &record, blob)?;
        changed.push(record.model.model_id);
    }
    Ok(answer(
        json!({"assigned": !changed.is_empty(), "detached": changed}),
        true,
    ))
}
pub(super) fn remove(connection: &Connection, owner: &str, model: &str) -> Result<(), String> {
    if let Some((_, blob)) = held(connection, owner, model)? {
        connection
            .execute(
                "DELETE FROM grid_models WHERE owner=?1 AND model_id=?2",
                params![owner, model],
            )
            .map_err(sql)?;
        CONTENT.delete(connection, blob)?;
    }
    Ok(())
}
pub(super) fn clear(connection: &Connection, owner: &str) -> Result<Outcome, String> {
    for record in list(connection, owner)? {
        remove(connection, owner, &record.model.model_id)?;
    }
    Ok(answer(json!({}), true))
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
