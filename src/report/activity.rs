//! Read-only report progress. Hosts supply IO facts; this owner separates
//! generation from publication and never grants freshness from file presence.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationPhase {
    Queued,
    Preparing,
    Generating,
    Generated,
    Failed,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generation {
    pub name: String,
    pub phase: GenerationPhase,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub started_at_ms: Option<u64>,
    #[serde(default)]
    pub client_publish_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Scope {
    pub kind: String,
    pub project: String,
}

/// The native sync store projection; this is not a census of retained batch directories.
#[derive(Debug, Deserialize)]
pub struct Artifact {
    pub scope: Scope,
    pub engine: String,
    pub operation: String,
    pub client_publish_id: String,
    pub state: String,
    pub produced_at_ms: u64,
    pub updated_at_ms: u64,
    pub size_bytes: u64,
    pub state_reason: Option<String>,
    pub outputs: Vec<Output>,
}

#[derive(Debug, Deserialize)]
pub struct Output {
    pub output_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ReceiptIdentity {
    pub engine: String,
}
#[derive(Debug, Deserialize)]
pub struct Receipt {
    pub receipt_id: String,
    pub scope: Scope,
    pub action: String,
    pub identity: Option<ReceiptIdentity>,
    pub outcome: String,
    pub detail: Option<String>,
    pub at_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub project: String,
    pub now_ms: u64,
    #[serde(default = "stall_threshold")]
    pub stall_after_ms: u64,
    pub rows: Vec<Generation>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub required_outputs: Vec<String>,
    #[serde(default)]
    pub receipts: Vec<Receipt>,
}

fn stall_threshold() -> u64 {
    120_000
}

#[derive(Debug, Serialize)]
pub struct Row<'a> {
    pub name: &'a str,
    pub generation_phase: GenerationPhase,
    pub publication_phase: &'a str,
    pub publication_updated_at_ms: Option<u64>,
    pub updated_at_ms: u64,
    pub started_at_ms: Option<u64>,
    pub error: Option<&'a str>,
    pub freshness_reason: &'static str,
    pub missing_outputs: Vec<&'a str>,
}

#[derive(Debug, Default, Serialize)]
pub struct Outbox<'a> {
    pub pending_batches: usize,
    pub pending_bytes: u64,
    pub conflicts: usize,
    pub refused: usize,
    pub oldest_pending_at_ms: Option<u64>,
    pub latest_issue_at_ms: Option<u64>,
    pub latest_issue: Option<&'a str>,
    /// Recorded attempts in the supplied receipt window, not lifetime totals.
    pub recorded_attempts: usize,
    pub last_attempt_at_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct View<'a> {
    pub schema: &'static str,
    pub project: &'a str,
    pub total: usize,
    pub generated: usize,
    pub published: usize,
    pub failed: usize,
    pub completed: usize,
    pub rows: Vec<Row<'a>>,
    pub outbox: Outbox<'a>,
    pub stalled: bool,
    pub waiting_on: Option<&'a str>,
    pub last_generation_progress_at_ms: Option<u64>,
}

pub fn project(snapshot: &Snapshot) -> Result<View<'_>, String> {
    let invalid = || "Report activity requires bounded facts for one exact project".to_string();
    if snapshot.project.is_empty()
        || snapshot.project.len() > 128
        || !snapshot
            .project
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
        || snapshot.rows.len() > 500
        || snapshot.artifacts.len() > 5000
        || snapshot.required_outputs.len() > 32
        || snapshot.receipts.len() > 5000
        || snapshot
            .required_outputs
            .iter()
            .any(|id| id.is_empty() || id.len() > 160)
        || !(30_000..=900_000).contains(&snapshot.stall_after_ms)
    {
        return Err(invalid());
    }
    let mut names = BTreeSet::new();
    for row in &snapshot.rows {
        if super::names(vec![row.name.clone()]).is_err()
            || !names.insert(&row.name)
            || row.updated_at_ms > snapshot.now_ms
            || row.started_at_ms.is_some_and(|at| at > row.updated_at_ms)
            || row.error.as_ref().is_some_and(|error| error.len() > 4000)
            || row
                .client_publish_id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 128)
        {
            return Err(invalid());
        }
    }
    let mut publications = BTreeSet::new();
    let mut outbox = Outbox::default();
    for artifact in &snapshot.artifacts {
        if artifact.scope.kind != "project" || artifact.scope.project != snapshot.project {
            return Err(invalid());
        }
        if artifact.engine != "network_reporter" {
            continue;
        }
        if !matches!(
            artifact.state.as_str(),
            "held" | "uploading" | "published" | "conflict" | "refused"
        ) || artifact.operation.len() > 160
            || artifact.client_publish_id.is_empty()
            || artifact.client_publish_id.len() > 128
            || !publications.insert(&artifact.client_publish_id)
            || artifact.outputs.len() > 32
            || artifact
                .outputs
                .iter()
                .any(|output| output.output_id.is_empty() || output.output_id.len() > 160)
            || artifact
                .state_reason
                .as_ref()
                .is_some_and(|reason| reason.len() > 4000)
        {
            return Err(invalid());
        }
        if matches!(artifact.state.as_str(), "held" | "uploading") {
            outbox.pending_batches += 1;
            outbox.pending_bytes = outbox
                .pending_bytes
                .checked_add(artifact.size_bytes)
                .ok_or_else(invalid)?;
            if artifact.produced_at_ms > 0 {
                outbox.oldest_pending_at_ms = Some(
                    outbox
                        .oldest_pending_at_ms
                        .map_or(artifact.produced_at_ms, |old| {
                            old.min(artifact.produced_at_ms)
                        }),
                );
            }
        }
        outbox.conflicts += usize::from(artifact.state == "conflict");
        outbox.refused += usize::from(artifact.state == "refused");
        if matches!(artifact.state.as_str(), "conflict" | "refused")
            && outbox
                .latest_issue_at_ms
                .is_none_or(|at| artifact.updated_at_ms >= at)
        {
            outbox.latest_issue_at_ms = Some(artifact.updated_at_ms);
            outbox.latest_issue = Some(
                artifact
                    .state_reason
                    .as_deref()
                    .unwrap_or(artifact.state.as_str()),
            );
        }
    }
    let mut receipt_ids = BTreeSet::new();
    for receipt in &snapshot.receipts {
        if receipt.scope.kind != "project"
            || receipt.scope.project != snapshot.project
            || receipt.receipt_id.is_empty()
            || receipt.receipt_id.len() > 128
            || !receipt_ids.insert(&receipt.receipt_id)
            || receipt.action.len() > 64
            || receipt.outcome.len() > 128
            || receipt
                .detail
                .as_ref()
                .is_some_and(|detail| detail.len() > 4000)
        {
            return Err(invalid());
        }
        if receipt
            .identity
            .as_ref()
            .is_some_and(|identity| identity.engine == "network_reporter")
            && matches!(receipt.action.as_str(), "upload" | "publish")
        {
            outbox.recorded_attempts += 1;
            outbox.last_attempt_at_ms = Some(
                outbox
                    .last_attempt_at_ms
                    .map_or(receipt.at_ms, |at| at.max(receipt.at_ms)),
            );
            if matches!(
                receipt.outcome.as_str(),
                "failed"
                    | "refused"
                    | "conflict"
                    | "error"
                    | "retry_deferred"
                    | "engine_build_not_admitted"
            ) && outbox
                .latest_issue_at_ms
                .is_none_or(|at| receipt.at_ms >= at)
            {
                outbox.latest_issue_at_ms = Some(receipt.at_ms);
                outbox.latest_issue = Some(receipt.detail.as_deref().unwrap_or(&receipt.outcome));
            }
        }
    }
    let mut view = View {
        schema: "ds.report.activity/v1",
        project: &snapshot.project,
        total: snapshot.rows.len(),
        generated: 0,
        published: 0,
        failed: 0,
        completed: 0,
        rows: Vec::new(),
        outbox,
        stalled: false,
        waiting_on: None,
        last_generation_progress_at_ms: snapshot.rows.iter().map(|row| row.updated_at_ms).max(),
    };
    for row in &snapshot.rows {
        let operation = format!("export-{}", row.name);
        let matching = snapshot
            .artifacts
            .iter()
            .filter(|artifact| {
                artifact.engine == "network_reporter" && artifact.operation == operation
            })
            .collect::<Vec<_>>();
        let latest = matching
            .iter()
            .max_by_key(|artifact| artifact.produced_at_ms)
            .copied();
        let current = matching
            .iter()
            .find(|artifact| Some(&artifact.client_publish_id) == row.client_publish_id.as_ref())
            .copied();
        let publication_phase = current.map_or("not_observed", |artifact| artifact.state.as_str());
        let missing_outputs = snapshot
            .required_outputs
            .iter()
            .filter(|id| {
                latest.is_none_or(|artifact| {
                    !artifact
                        .outputs
                        .iter()
                        .any(|output| &output.output_id == *id)
                })
            })
            .map(String::as_str)
            .collect::<Vec<_>>();
        // Existing native generation does not implement receipt reuse. Be
        // truthful about that policy; legacy cloud success is not proof that
        // the selected papers answer the current design or requested outputs.
        let freshness_reason = if latest.is_some_and(|artifact| artifact.state == "conflict") {
            "revision_conflict"
        } else if !missing_outputs.is_empty() {
            "missing_outputs"
        } else {
            "local_regeneration"
        };
        view.generated += usize::from(row.phase == GenerationPhase::Generated);
        view.failed += usize::from(row.phase == GenerationPhase::Failed);
        view.published += usize::from(
            row.phase == GenerationPhase::Generated && publication_phase == "published",
        );
        view.rows.push(Row {
            name: &row.name,
            generation_phase: row.phase,
            publication_phase,
            publication_updated_at_ms: current.map(|artifact| artifact.updated_at_ms),
            updated_at_ms: row.updated_at_ms,
            started_at_ms: row.started_at_ms,
            error: row
                .error
                .as_deref()
                .or_else(|| current.and_then(|artifact| artifact.state_reason.as_deref())),
            freshness_reason,
            missing_outputs,
        });
    }
    view.completed = view.generated + view.failed;
    let pending = snapshot.rows.iter().any(|row| {
        !matches!(
            row.phase,
            GenerationPhase::Generated | GenerationPhase::Failed
        )
    });
    view.stalled = pending
        && view
            .last_generation_progress_at_ms
            .is_some_and(|at| snapshot.now_ms.saturating_sub(at) >= snapshot.stall_after_ms);
    if view.stalled {
        view.waiting_on = snapshot
            .rows
            .iter()
            .find(|row| {
                matches!(
                    row.phase,
                    GenerationPhase::Preparing | GenerationPhase::Generating
                )
            })
            .or_else(|| {
                snapshot
                    .rows
                    .iter()
                    .find(|row| row.phase == GenerationPhase::Queued)
            })
            .map(|row| row.name.as_str());
    }
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot() -> Snapshot {
        serde_json::from_value(json!({"project":"project_a","now_ms":300000,"rows":[
            {"name":"tx_a","phase":"generated","updated_at_ms":100000,"client_publish_id":"new-publication"},
            {"name":"tx_b","phase":"generating","updated_at_ms":110000,"started_at_ms":100000}
        ],"required_outputs":["xlsx","pdf__a0"],"artifacts":[
            {"scope":{"kind":"project","project":"project_a"},"engine":"network_reporter","operation":"export-tx_a","client_publish_id":"older-publication","state":"published","produced_at_ms":1000,"updated_at_ms":2000,"size_bytes":20,"state_reason":null,"outputs":[{"output_id":"xlsx"}]},
            {"scope":{"kind":"project","project":"project_a"},"engine":"network_reporter","operation":"export-tx_a","client_publish_id":"new-publication","state":"held","produced_at_ms":100000,"updated_at_ms":110000,"size_bytes":40,"state_reason":null,"outputs":[{"output_id":"xlsx"},{"output_id":"pdf__a0"}]}
        ]})).unwrap()
    }

    #[test]
    fn generation_does_not_wait_for_publication_or_count_an_older_published_run() {
        let mut s = snapshot();
        let v = project(&s).unwrap();
        assert_eq!((v.generated, v.published, v.completed), (1, 0, 1));
        assert_eq!(v.outbox.pending_batches, 1);
        assert_eq!(v.outbox.oldest_pending_at_ms, Some(100000));
        assert!(v.stalled);
        assert_eq!(v.waiting_on, Some("tx_b"));
        s.artifacts[1].state = "published".into();
        assert_eq!(project(&s).unwrap().published, 1);
        s.artifacts[1].state = "conflict".into();
        let v = project(&s).unwrap();
        assert_eq!(v.published, 0);
        assert_eq!(v.rows[0].freshness_reason, "revision_conflict");
    }

    #[test]
    fn requested_output_inventory_and_project_isolation_are_not_legacy_success() {
        let mut s = snapshot();
        s.artifacts[1].outputs.pop();
        let v = project(&s).unwrap();
        assert_eq!(v.rows[0].missing_outputs, vec!["pdf__a0"]);
        assert_eq!(v.rows[0].freshness_reason, "missing_outputs");
        s.artifacts[0].scope.project = "project_b".into();
        assert!(project(&s).is_err());
    }
    #[test]
    fn terminal_generation_is_not_stalled_when_publication_is_backlogged() {
        let mut s = snapshot();
        s.rows[1].phase = GenerationPhase::Failed;
        s.rows[1].error = Some("renderer stopped".into());
        let v = project(&s).unwrap();
        assert_eq!((v.generated, v.failed, v.completed, v.total), (1, 1, 2, 2));
        assert!(!v.stalled);
        assert!(v.waiting_on.is_none());
        assert_eq!(v.outbox.pending_batches, 1);
        s.rows[1].phase = GenerationPhase::Generating;
        s.artifacts[1].updated_at_ms = s.now_ms;
        assert!(
            project(&s).unwrap().stalled,
            "sync progress must not hide a stalled renderer"
        );
    }

    #[test]
    fn duplicate_rows_and_foreign_receipts_cannot_mix_projects() {
        let mut s = snapshot();
        s.rows.push(s.rows[0].clone());
        assert!(project(&s).is_err());
        s.rows.pop();
        s.rows[0].updated_at_ms = s.now_ms + 1;
        assert!(project(&s).is_err());
        let mut s = snapshot();
        s.receipts = serde_json::from_value(json!([{"receipt_id":"attempt1","scope":{"kind":"project","project":"project_a"},"action":"upload","identity":{"engine":"network_reporter"},"outcome":"failed","detail":"link unavailable","at_ms":200000}])).unwrap();
        let v = project(&s).unwrap();
        assert_eq!(v.outbox.recorded_attempts, 1);
        assert_eq!(v.outbox.last_attempt_at_ms, Some(200000));
        assert_eq!(v.outbox.latest_issue, Some("link unavailable"));
        s.receipts[0].scope.project = "project_b".into();
        assert!(project(&s).is_err());
    }
}
