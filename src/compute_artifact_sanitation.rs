//! Explicit queue housekeeping. Archival retains provenance and local bytes.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Input {
    owner: String,
    project: String,
    rows: Vec<Row>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Row {
    id: String,
    fingerprint: String,
    engine: String,
    operation: String,
    variant: String,
    status: String,
    phase: String,
    created_at: u64,
    archived: bool,
    outputs: usize,
    held_outputs: usize,
    engine_version: String,
    engine_build_manifest_sha256: Option<String>,
    failure_code: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct Decision {
    id: String,
    operation: String,
    action: &'static str,
    reason: &'static str,
}

pub fn evaluate(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Sanitation exceeds its byte bound".into());
    }
    let mut input: Input = serde_json::from_slice(bytes).map_err(|_| "Invalid sanitation input")?;
    if input.owner.is_empty()
        || input.owner.len() > 128
        || input.project.is_empty()
        || input.project.len() > 128
        || input.rows.len() > 4096
    {
        return Err("Sanitation requires one owner/project and at most 4096 rows".into());
    }
    input.rows.sort_by(|a, b| a.id.cmp(&b.id));
    let mut ids = BTreeSet::new();
    let mut latest = BTreeMap::<(&str, &str), &Row>::new();
    for row in &input.rows {
        if !ids.insert(&row.id)
            || row.id.is_empty()
            || row.id.len() > 512
            || row.operation.is_empty()
            || row.operation.len() > 128
            || row.variant.len() > 128
            || row.outputs > 32
            || row.fingerprint.len() != 64
            || !row
                .fingerprint
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || row.held_outputs > row.outputs
        {
            return Err("Invalid sanitation row identity or inventory".into());
        }
        if row.engine == "network_reporter" && row.outputs > 0 && row.held_outputs == row.outputs {
            let entry = latest.entry((&row.operation, &row.variant)).or_insert(row);
            if row.created_at > entry.created_at {
                *entry = row;
            }
        }
    }
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&input).map_err(|_| "Sanitation encoding failed")?)
    );
    let mut decisions = Vec::new();
    for row in &input.rows {
        let superseded = latest
            .get(&(row.operation.as_str(), row.variant.as_str()))
            .is_some_and(|newer| newer.created_at > row.created_at);
        let retry = crate::compute_artifact_sync::assess(
            &serde_json::to_vec(&serde_json::json!({
                "schema": crate::compute_artifact_sync::SCHEMA, "engine": row.engine,
                "status": row.status, "phase": row.phase, "engine_version": row.engine_version,
                "engine_build_manifest_sha256": row.engine_build_manifest_sha256,
                "failure_code": row.failure_code, "error": row.error,
            }))
            .map_err(|_| "Sanitation encoding failed")?,
        )?;
        let (action, reason) = if row.archived {
            ("keep", "already_archived")
        } else if row.engine != "network_reporter" {
            ("keep", "other_workflow")
        } else if row.status == "pending" || row.status == "syncing" {
            ("keep", "active_publication")
        } else if row.status == "synced"
            || (superseded && matches!(row.status.as_str(), "failed" | "conflict"))
        {
            (
                "archive",
                if row.status == "synced" {
                    "published_history"
                } else {
                    "superseded_local_inventory"
                },
            )
        } else if retry.retry_eligible {
            ("retry", "recoverable_failure")
        } else {
            ("keep", "requires_attention")
        };
        decisions.push(Decision {
            id: row.id.clone(),
            operation: row.operation.clone(),
            action,
            reason,
        });
    }
    serde_json::to_string(&serde_json::json!({
        "schema": "ds.compute-artifact.sanitation/v1", "project": input.project,
        "digest": digest, "total": decisions.len(), "decisions": decisions,
    }))
    .map_err(|_| "Sanitation encoding failed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn row(id: &str, created: u64) -> Value {
        json!({
            "id": id, "fingerprint": "c".repeat(64), "engine": "network_reporter",
            "operation": "export-agasharu", "variant": "default", "status": "conflict",
            "phase": "pending", "created_at": created, "archived": false,
            "outputs": 2, "held_outputs": 2,
            "engine_version": format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
            "engine_build_manifest_sha256": "b".repeat(64), "failure_code": null,
            "error": "compute artifact conflict: stored work changed before finalize"
        })
    }
    fn plan(rows: Vec<Value>) -> Value {
        serde_json::from_str(
            &evaluate(
                &serde_json::to_vec(&json!({"owner":"owner", "project":"project", "rows":rows}))
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn archives_superseded_failure_and_recovers_current_without_deleting_bytes() {
        let p = plan(vec![row("old", 1), row("new", 2)]);
        assert_eq!(p["decisions"][0]["action"], "retry");
        assert_eq!(p["decisions"][1]["action"], "archive");
        let mut changed = row("new", 2);
        changed["fingerprint"] = json!("d".repeat(64));
        assert_ne!(p["digest"], plan(vec![row("old", 1), changed])["digest"]);
    }
    #[test]
    fn keeps_active_other_workflows_and_unresolved_current_failures() {
        for (field, value) in [
            ("engine", json!("solar")),
            ("status", json!("syncing")),
            ("status", json!("pending")),
            ("archived", json!(true)),
            (
                "error",
                json!("compute artifact conflict: network reporter input receipt is stale"),
            ),
        ] {
            let mut r = row("one", 1);
            r[field] = value;
            assert_eq!(plan(vec![r])["decisions"][0]["action"], "keep");
        }
        let mut partial = row("new", 2);
        partial["held_outputs"] = json!(1);
        assert_eq!(
            plan(vec![row("old", 1), partial])["decisions"][1]["action"],
            "retry"
        );
    }
}
