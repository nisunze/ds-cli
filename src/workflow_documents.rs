//! Portable source pins and immutable reusable definitions for the existing
//! native DAG. Hosts acquire bytes; this owner verifies and projects them.
use crate::{
    design::{self, Snapshot},
    design_repair::{EvidenceSource, Scope},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const MAX_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    File {
        path: String,
        sha256: String,
    },
    Workspace {
        path: String,
        project: String,
        transformer: String,
        revision: String,
    },
    Transformer {
        lane: String,
        project: String,
        transformer: String,
        version: u64,
        content_digest: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    Json,
    Snapshot,
    Evidence,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub source: Source,
    pub projection: Projection,
    pub scope: Option<Scope>,
    pub id: Option<String>,
    pub layer: Option<String>,
    pub crs: Option<String>,
    #[serde(default)]
    pub units: BTreeMap<String, BTreeMap<String, String>>,
}
fn err(code: &str, message: &str) -> String {
    format!("{code}: {message}")
}
pub fn project(binding: &Binding, raw: Value, raw_sha256: Option<&str>) -> Result<Value, String> {
    if serde_json::to_vec(&raw).map_err(|e| e.to_string())?.len() > MAX_BYTES {
        return Err(err(
            "workflow_budget_exceeded",
            "document input exceeds 8 MiB",
        ));
    }
    if let Source::File { sha256, .. } = &binding.source {
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(err(
                "workflow_document_invalid",
                "file source requires an exact SHA-256",
            ));
        }
        if raw_sha256 != Some(sha256.as_str()) {
            return Err(err(
                "workflow_source_changed",
                "file bytes differ from the declared SHA-256",
            ));
        }
    }
    match binding.projection {
        Projection::Json => {
            if !raw.is_object() {
                return Err(err(
                    "workflow_document_invalid",
                    "document must be an object",
                ));
            }
            Ok(raw)
        }
        Projection::Snapshot => {
            let snapshot: Snapshot = serde_json::from_value(raw)
                .map_err(|e| err("workflow_document_invalid", &e.to_string()))?;
            snapshot
                .validate_retained()
                .map_err(|e| err("workflow_document_invalid", &e))?;
            verify_snapshot_source(binding, &snapshot)?;
            serde_json::to_value(snapshot).map_err(|e| e.to_string())
        }
        Projection::Evidence => {
            let scope = binding.scope.as_ref().ok_or_else(|| {
                err(
                    "workflow_authority_missing",
                    "evidence requires explicit scope",
                )
            })?;
            scope
                .validate()
                .map_err(|e| err("workflow_document_invalid", &e.message))?;
            let (layers, crs, revision) = if raw["schema"] == design::SCHEMA {
                let snapshot: Snapshot = serde_json::from_value(raw).map_err(|e| e.to_string())?;
                snapshot
                    .validate_retained()
                    .map_err(|e| err("workflow_document_invalid", &e))?;
                verify_snapshot_source(binding, &snapshot)?;
                let revision = design::digest(&snapshot)?;
                (snapshot.layers, Some(snapshot.crs), Some(revision))
            } else if raw["type"] == "FeatureCollection" {
                let layer = binding.layer.as_ref().ok_or_else(|| {
                    err(
                        "workflow_document_invalid",
                        "a FeatureCollection evidence source requires a layer name",
                    )
                })?;
                (
                    BTreeMap::from([(layer.clone(), raw)]),
                    binding.crs.clone(),
                    None,
                )
            } else {
                let layers = raw.get("layers").cloned().unwrap_or(raw);
                (
                    serde_json::from_value(layers).map_err(|e| e.to_string())?,
                    binding.crs.clone(),
                    None,
                )
            };
            let source = EvidenceSource {
                schema: "ds.repair.evidence/v1".into(),
                id: binding
                    .id
                    .clone()
                    .ok_or_else(|| err("workflow_document_invalid", "evidence requires an id"))?,
                scope: scope.clone(),
                content_digest: design::digest(&layers)?,
                raw_file_sha256: raw_sha256.map(str::to_owned),
                revision,
                crs,
                units: binding.units.clone(),
                layers,
            };
            serde_json::to_value(source).map_err(|e| e.to_string())
        }
    }
}
fn verify_snapshot_source(binding: &Binding, snapshot: &Snapshot) -> Result<(), String> {
    match &binding.source {
        Source::Workspace {
            project,
            transformer,
            revision,
            ..
        } => {
            if snapshot.transformer != *transformer
                || design::digest(snapshot)? != *revision
                || binding
                    .scope
                    .as_ref()
                    .is_some_and(|s| s.project != *project)
            {
                return Err(err(
                    "workflow_source_changed",
                    "workspace project, transformer or revision differs",
                ));
            }
        }
        Source::Transformer {
            project,
            transformer,
            content_digest,
            ..
        } => {
            if snapshot.transformer != *transformer
                || crate::report_export::jcs::layers_content_digest(&snapshot.layers)?
                    != *content_digest
                || binding
                    .scope
                    .as_ref()
                    .is_some_and(|s| s.project != *project)
            {
                return Err(err(
                    "workflow_source_changed",
                    "saved transformer source differs",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema: String,
    pub name: String,
    pub note: String,
    pub document: Value,
    pub operation_versions: BTreeMap<String, Value>,
    pub digest: String,
}
/// Definitions retain recipe/versions only. Run bindings and credentials never
/// become reusable library data; drafts may retain defaults until saved.
pub fn capture(
    name: &str,
    note: &str,
    mut document: Value,
    versions: BTreeMap<String, Value>,
) -> Result<Definition, String> {
    design::identifier(name)?;
    if note.len() > 2048 || note.chars().any(char::is_control) {
        return Err(err("workflow_document_invalid", "invalid change note"));
    }
    if document["schema"] != "ds.vector-workflow/v1" {
        return Err(err(
            "workflow_document_invalid",
            "expected existing native workflow schema",
        ));
    }
    if serde_json::to_vec(&document)
        .map_err(|e| e.to_string())?
        .len()
        > MAX_BYTES
    {
        return Err(err("workflow_budget_exceeded", "definition exceeds 8 MiB"));
    }
    if let Some(inputs) = document["inputs"].as_object() {
        for value in inputs.values() {
            if (value["type"] == "document" || value["type"] == "layer")
                && value.get("default").is_some()
            {
                return Err(err(
                    "workflow_document_invalid",
                    "saved definitions require per-run document/layer bindings",
                ));
            }
        }
    }
    fn forbidden(value: &Value) -> bool {
        match value {
            Value::Object(o) => {
                o.get("schema")
                    .is_some_and(|v| v == design::SCHEMA || v == "ds.repair.evidence/v1")
                    || o.get("type")
                        .is_some_and(|v| v == "FeatureCollection" || v == "Feature")
                    || o.iter().any(|(k, v)| {
                        matches!(
                            k.to_ascii_lowercase().as_str(),
                            "authorization"
                                | "bearer"
                                | "token"
                                | "access_token"
                                | "refresh_token"
                                | "credentials"
                                | "password"
                                | "secret"
                                | "file"
                                | "workspace"
                        ) || forbidden(v)
                    })
            }
            Value::Array(a) => a.iter().any(forbidden),
            _ => false,
        }
    }
    if forbidden(&document) {
        return Err(err(
            "workflow_document_invalid",
            "definition contains credentials or embedded source acquisition; bind inputs per run",
        ));
    }
    let steps = document["steps"]
        .as_array_mut()
        .ok_or_else(|| err("workflow_document_invalid", "steps must be an array"))?;
    let mut used = BTreeMap::new();
    for step in steps {
        let id = step["tool"]
            .as_str()
            .ok_or_else(|| err("workflow_document_invalid", "step tool missing"))?
            .to_owned();
        let version = versions.get(&id).cloned().ok_or_else(|| {
            err(
                "workflow_op_version_unsupported",
                "tool has no admitted descriptor",
            )
        })?;
        if !(version.as_u64().is_some_and(|v| v > 0)
            || version.as_str().is_some_and(|v| {
                !v.is_empty() && v.len() <= 128 && !v.chars().any(char::is_whitespace)
            }))
        {
            return Err(err(
                "workflow_op_version_unsupported",
                "operation contracts start at version one",
            ));
        }
        if step.get("tool_version").is_some_and(|v| v != &version) {
            return Err(err(
                "workflow_op_version_unsupported",
                "step pins a different contract",
            ));
        }
        step["tool_version"] = version.clone();
        used.insert(id, version);
    }
    let mut result = Definition {
        schema: "ds.workflow.definition/v1".into(),
        name: name.into(),
        note: note.into(),
        document,
        operation_versions: used,
        digest: String::new(),
    };
    result.digest = design::digest(&result)?;
    Ok(result)
}
pub fn verify(definition: &Definition) -> Result<(), String> {
    let rebuilt = capture(
        &definition.name,
        &definition.note,
        definition.document.clone(),
        definition.operation_versions.clone(),
    )?;
    if serde_json::to_value(definition).map_err(|e| e.to_string())?
        != serde_json::to_value(&rebuilt).map_err(|e| e.to_string())?
    {
        return Err(err(
            "workflow_source_changed",
            "immutable definition has changed",
        ));
    }
    Ok(())
}
