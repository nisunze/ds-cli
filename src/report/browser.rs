//! Report consumption effect program. Native and browser share publication
//! walks, exact tickets, inventory projection, planning and lease transitions.
//! This slice admits no producer uploads; the host only executes named effects.
use super::{
    browser_store::{LeaseAction, Operation as StoreOperation},
    inventory as view,
};
use crate::{
    sync::{
        self,
        acquisition::{self, HeadStep, HeadWalk},
    },
    sync_store::{
        report_heads::{self, ReportHeadObservation},
        Fence,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub fence: Fence,
    pub project: String,
    pub transformer: String,
    pub variant: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Inventory {
        scope: Scope,
        refresh: bool,
    },
    Artifact {
        scope: Scope,
        source: Source,
        output_id: String,
        head_revision: u64,
        sha256: String,
        size_bytes: u64,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Cloud,
    SyncDownload,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub key: String,
    pub output_id: String,
    pub head_revision: u64,
    pub sha256: String,
    pub size_bytes: u64,
    pub filename: String,
    pub content_type: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Store {
        owner: String,
        operation: Value,
        continuation: Continuation,
    },
    Gateway {
        body: Value,
        continuation: Continuation,
    },
    Probe {
        outputs: Vec<Selection>,
        continuation: Continuation,
    },
    Download {
        url: String,
        output: Selection,
        continuation: Continuation,
    },
    Bytes {
        output: Selection,
    },
    Done {
        view: Value,
    },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Continuation {
    run: Run,
    stage: Stage,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    request: Request,
    worker: String,
    lease: bool,
    current: bool,
    observation: Option<ReportHeadObservation>,
    selected: Vec<Selection>,
    verified: Vec<view::RetainedOutput>,
    pending: Vec<Selection>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
enum Stage {
    Held,
    Take,
    Renew { next: Next },
    Walk { walk: HeadWalk },
    Persist,
    Probe,
    Ticket { output: Selection },
    Download { output: Selection },
    Release { view: Value },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "next", rename_all = "snake_case", deny_unknown_fields)]
enum Next {
    Walk { body: Value, walk: HeadWalk },
    Persist { heads: Vec<Value> },
    Ticket { output: Selection },
    Download { url: String, output: Selection },
    Probe,
}
impl Run {
    fn scope(&self) -> &Scope {
        match &self.request {
            Request::Inventory { scope, .. } | Request::Artifact { scope, .. } => scope,
        }
    }
    fn refresh(&self) -> bool {
        matches!(self.request, Request::Inventory { refresh: true, .. })
    }
    fn head(&self) -> Result<Option<crate::compute_artifact_inventory::Head>, String> {
        let s = self.scope();
        self.observation
            .as_ref()
            .and_then(|o| {
                o.heads.iter().find(|h| {
                    h["engine"] == "network_reporter"
                        && h["operation"] == format!("export-{}", s.transformer)
                        && h["variant"] == s.variant
                })
            })
            .map(|h| {
                crate::compute_artifact_inventory::decode_head(
                    &serde_json::to_vec(h).map_err(|e| e.to_string())?,
                )
            })
            .transpose()
    }
    fn select(&mut self) -> Result<(), String> {
        let Some(head) = self.head()? else {
            if matches!(self.request, Request::Artifact { .. }) {
                return Err("report publication is not held".into());
            }
            self.selected.clear();
            return Ok(());
        };
        let scope = self.scope();
        self.selected = head
            .outputs
            .iter()
            .map(|o| {
                let key = serde_json::to_vec(&json!([
                    scope.fence,
                    scope.project,
                    head.engine,
                    head.operation,
                    head.variant,
                    head.head_revision,
                    o.output_id,
                    o.sha256,
                    o.size_bytes
                ]))
                .map_err(|e| e.to_string())?;
                Ok(Selection {
                    key: format!("{:x}", Sha256::digest(key)),
                    output_id: o.output_id.clone(),
                    head_revision: head.head_revision,
                    sha256: o.sha256.clone(),
                    size_bytes: o.size_bytes,
                    filename: crate::compute_artifact_inventory::filename(
                        &head.engine,
                        &head.operation,
                        o,
                    ),
                    content_type: o.content_type.clone(),
                })
            })
            .collect::<Result<_, String>>()?;
        if let Request::Artifact {
            output_id,
            head_revision,
            sha256,
            size_bytes,
            ..
        } = &self.request
        {
            let selected = self
                .selected
                .iter()
                .find(|o| {
                    &o.output_id == output_id
                        && o.head_revision == *head_revision
                        && &o.sha256 == sha256
                        && o.size_bytes == *size_bytes
                })
                .cloned()
                .ok_or("report selection differs from the retained publication")?;
            self.selected = vec![selected];
        }
        Ok(())
    }
}
fn store(run: Run, stage: Stage, owner: &str, operation: Value) -> Step {
    Step::Store {
        owner: owner.into(),
        operation,
        continuation: Continuation { run, stage },
    }
}
fn lease(run: Run, stage: Stage, now: u64, action: LeaseAction) -> Result<Step, String> {
    let s = run.scope();
    let op = StoreOperation::Lease {
        fence: s.fence.clone(),
        project: s.project.clone(),
        worker: run.worker.clone(),
        now,
        lease_action: action,
    };
    Ok(store(
        run,
        stage,
        "browser",
        serde_json::to_value(op).map_err(|e| e.to_string())?,
    ))
}
fn admitted_lease(value: &Value) -> Result<(), String> {
    let t: crate::sync_store::Transition =
        serde_json::from_value(value["transition"].clone()).map_err(|e| e.to_string())?;
    if let Some(refusal) = t.refusals.first() {
        return Err(format!("{}: {}", refusal.code, refusal.detail));
    }
    Ok(())
}
pub fn start(request: Request, worker: String, now: u64) -> Result<Step, String> {
    let run = Run {
        request,
        worker,
        lease: false,
        current: false,
        observation: None,
        selected: vec![],
        verified: vec![],
        pending: vec![],
    };
    let s = run.scope();
    report_heads::validate_scope(&s.fence, &s.project, now)?;
    if s.variant != "default"
        || s.transformer.is_empty()
        || s.transformer.len() > 200
        || s.transformer.trim() != s.transformer
        || s.transformer
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err("invalid report inventory scope".into());
    }
    let op = report_heads::effects::Operation::Read {
        fence: s.fence.clone(),
        project: s.project.clone(),
    };
    Ok(store(
        run,
        Stage::Held,
        "heads",
        serde_json::to_value(op).map_err(|e| e.to_string())?,
    ))
}
fn guarded(run: Run, next: Next, now: u64) -> Result<Step, String> {
    if run.lease {
        lease(run, Stage::Renew { next }, now, LeaseAction::Renew)
    } else {
        issue(run, next, now)
    }
}
fn issue(mut run: Run, next: Next, now: u64) -> Result<Step, String> {
    Ok(match next {
        Next::Walk { body, walk } => Step::Gateway {
            body,
            continuation: Continuation {
                run,
                stage: Stage::Walk { walk },
            },
        },
        Next::Persist { heads } => {
            let s = run.scope();
            let op = report_heads::effects::Operation::Observe {
                fence: s.fence.clone(),
                project: s.project.clone(),
                at_ms: now,
                heads: heads.clone(),
            };
            run.observation = Some(ReportHeadObservation {
                observed_at_ms: now,
                heads,
            });
            store(
                run,
                Stage::Persist,
                "heads",
                serde_json::to_value(op).map_err(|e| e.to_string())?,
            )
        }
        Next::Probe => {
            run.select()?;
            Step::Probe {
                outputs: run.selected.clone(),
                continuation: Continuation {
                    run,
                    stage: Stage::Probe,
                },
            }
        }
        Next::Ticket { output } => {
            let head = run.head()?.ok_or("report head disappeared")?;
            Step::Gateway {
                body: acquisition::download_body(&run.scope().project, &head, &output.output_id)?,
                continuation: Continuation {
                    run,
                    stage: Stage::Ticket { output },
                },
            }
        }
        Next::Download { url, output } => Step::Download {
            url,
            output: output.clone(),
            continuation: Continuation {
                run,
                stage: Stage::Download { output },
            },
        },
    })
}
fn walk(run: Run, step: HeadStep, now: u64) -> Result<Step, String> {
    guarded(
        run,
        match step {
            HeadStep::Gateway { body, continuation } => Next::Walk {
                body,
                walk: continuation,
            },
            HeadStep::Done { heads } => Next::Persist { heads },
        },
        now,
    )
}
fn final_view(run: &Run) -> Result<Value, String> {
    let s = run.scope();
    let unobserved = || view::ReadFact {
        state: view::ReadState::NotObserved,
        error: None,
    };
    let mut snapshot = view::Snapshot {
        owner: s.fence.account.clone(),
        project: s.project.clone(),
        transformer: s.transformer.clone(),
        variant: s.variant.clone(),
        expected_input_base_fingerprint: None,
        local_read: unobserved(),
        sync_read: unobserved(),
        cloud_read: unobserved(),
        local_sets: vec![],
        sync_artifacts: vec![],
        cloud_head: None,
        legacy_cloud: None,
        required_outputs: vec![],
    };
    view::attach_observed_head(&mut snapshot, run.observation.as_ref(), run.current, None)?;
    view::project_with_retained(&snapshot, &run.verified)
}
fn finish(run: Run, now: u64) -> Result<Step, String> {
    let view = final_view(&run)?;
    if run.lease {
        lease(run, Stage::Release { view }, now, LeaseAction::Release)
    } else {
        Ok(Step::Done { view })
    }
}
fn next_download(mut run: Run, now: u64) -> Result<Step, String> {
    if run.pending.is_empty() {
        return finish(run, now);
    }
    let output = run.pending.remove(0);
    guarded(run, Next::Ticket { output }, now)
}
fn download_needed(run: &Run, now: u64) -> Result<bool, String> {
    let s = run.scope();
    let remote: Vec<_> = run
        .observation
        .as_ref()
        .map(|o| o.heads.iter().map(acquisition::remote_row).collect())
        .unwrap_or_default();
    let selected = sync::Identity {
        engine: "network_reporter".into(),
        operation: format!("export-{}", s.transformer),
        variant: s.variant.clone(),
    };
    let local = remote
        .iter()
        .filter(|h| {
            h.identity == selected
                && h.outputs.iter().all(|o| {
                    run.verified.iter().any(|v| {
                        v.output_id == o.output_id
                            && v.sha256 == o.sha256
                            && v.size_bytes == o.size_bytes
                            && v.head_revision as i64 == h.head_revision
                    })
                })
        })
        .map(|h| sync::LocalArtifact {
            identity: h.identity.clone(),
            sha256: h.sha256.clone(),
            size_bytes: h.outputs.iter().fold(0u64, |sum, o| sum.saturating_add(o.size_bytes)),
            produced_at_ms: h.published_at_ms,
            base_revision: Some(h.revision.clone()),
            input_base_fingerprint: h.input_base_fingerprint.clone(),
            readable: true,
            engine_release: String::new(),
            resource: String::new(),
        })
        .collect();
    let input = sync::Input {
        schema: sync::SCHEMA.into(),
        project: s.project.clone(),
        personal: false,
        install_id: s.fence.install_id.clone(),
        now_ms: now,
        online: true,
        trigger: sync::Trigger::Manual,
        remote_read_at_ms: run.observation.as_ref().map(|o| o.observed_at_ms),
        grant: None,
        grants: vec![],
        source_grants: vec![],
        local,
        remote: remote
            .iter()
            .map(|h| sync::RemoteHead {
                identity: h.identity.clone(),
                revision: h.revision.clone(),
                input_base_fingerprint: h.input_base_fingerprint.clone(),
                sha256: h.sha256.clone(),
                published_at_ms: h.published_at_ms,
            })
            .collect(),
        published_outputs: remote
            .iter()
            .flat_map(|h| {
                h.outputs.iter().map(|o| sync::PublishedOutput {
                    identity: h.identity.clone(),
                    output_id: o.output_id.clone(),
                    sha256: o.sha256.clone(),
                })
            })
            .collect(),
        download_interest: Some(vec![selected]),
    };
    let plan = sync::plan(&input)?;
    if plan.summary.refused > 0 || plan.summary.conflicts > 0 {
        return Err("report acquisition plan refused".into());
    }
    Ok(plan
        .actions
        .iter()
        .any(|a| matches!(a, sync::Action::Download { .. })))
}
pub fn resume(continuation: Continuation, value: Value, now: u64) -> Result<Step, String> {
    let Continuation { mut run, stage } = continuation;
    match stage {
        Stage::Held => {
            run.observation =
                serde_json::from_value(value["observation"].clone()).map_err(|e| e.to_string())?;
            if run.refresh() {
                lease(run, Stage::Take, now, LeaseAction::Take)
            } else {
                issue(run, Next::Probe, now)
            }
        }
        Stage::Take => {
            admitted_lease(&value)?;
            run.lease = true;
            let step = HeadWalk::start(&run.scope().project)?;
            walk(run, step, now)
        }
        Stage::Renew { next } => {
            admitted_lease(&value)?;
            issue(run, next, now)
        }
        Stage::Walk { walk: state } => walk(run, state.resume(value)?, now),
        Stage::Persist => {
            run.current = true;
            issue(run, Next::Probe, now)
        }
        Stage::Probe => {
            let proofs = value
                .as_array()
                .ok_or("invalid retained byte observations")?;
            if proofs.len() != run.selected.len() {
                return Err("retained byte observation count changed".into());
            }
            for (proof, selected) in proofs.iter().zip(&run.selected) {
                if proof.is_null() {
                    continue;
                }
                if proof["sha256"] != selected.sha256 || proof["size_bytes"] != selected.size_bytes
                {
                    return Err("retained report bytes fail digest or size verification".into());
                }
                run.verified.push(view::RetainedOutput {
                    output_id: selected.output_id.clone(),
                    head_revision: selected.head_revision,
                    sha256: selected.sha256.clone(),
                    size_bytes: selected.size_bytes,
                });
            }
            if let Request::Artifact { source, .. } = &run.request {
                let output = run
                    .selected
                    .first()
                    .ok_or("selected output absent")?
                    .clone();
                if *source == Source::SyncDownload {
                    if run.verified.len() != 1 {
                        return Err("the exact retained output is unavailable".into());
                    }
                    return Ok(Step::Bytes { output });
                }
                return guarded(run, Next::Ticket { output }, now);
            }
            if run.refresh() && download_needed(&run, now)? {
                run.pending = run.selected.clone();
                run.verified.clear();
                next_download(run, now)
            } else {
                finish(run, now)
            }
        }
        Stage::Ticket { output } => {
            let head = run.head()?.ok_or("selected publication absent")?;
            let declaration = head
                .outputs
                .iter()
                .find(|o| o.output_id == output.output_id)
                .ok_or("selected output absent")?;
            let url = acquisition::admitted_ticket_url(&head, declaration, &value)
                .map_err(|e| format!("report ticket refused: {e:?}"))?
                .to_string();
            let parsed = url::Url::parse(&url).map_err(|_| "invalid report ticket URL")?;
            if parsed.scheme() != "https"
                || parsed.host_str() != Some("storage.googleapis.com")
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.port_or_known_default() != Some(443)
                || url.len() > 8192
                || !parsed.query_pairs().any(|(k, v)| {
                    k == "generation"
                        && !v.is_empty()
                        && v.bytes().all(|c| c.is_ascii_digit())
                        && v != "0"
                })
            {
                return Err("report ticket is not a governed immutable object".into());
            }
            guarded(run, Next::Download { url, output }, now)
        }
        Stage::Download { output } => {
            if value["sha256"] != output.sha256 || value["size_bytes"] != output.size_bytes {
                return Err("downloaded report failed exact byte verification".into());
            }
            if matches!(run.request, Request::Artifact { .. }) {
                return Ok(Step::Bytes { output });
            }
            run.verified.push(view::RetainedOutput {
                output_id: output.output_id,
                head_revision: output.head_revision,
                sha256: output.sha256,
                size_bytes: output.size_bytes,
            });
            next_download(run, now)
        }
        Stage::Release { view } => {
            admitted_lease(&value)?;
            Ok(Step::Done { view })
        }
    }
}
/// An interrupted host releases only the exact worker it acquired. Crashes
/// retain the existing lease expiry; there is no second lock or retry schedule.
pub fn cleanup(request: &Request, worker: &str, now: u64) -> Result<Value, String> {
    let s = match request {
        Request::Inventory { scope, .. } | Request::Artifact { scope, .. } => scope,
    };
    serde_json::to_value(StoreOperation::Lease {
        fence: s.fence.clone(),
        project: s.project.clone(),
        worker: worker.into(),
        now,
        lease_action: LeaseAction::Release,
    })
    .map_err(|e| e.to_string())
}

pub fn evaluate(bytes: &[u8]) -> Result<String, String> {
    #[derive(Deserialize)]
    #[serde(tag = "call", rename_all = "snake_case", deny_unknown_fields)]
    enum Call {
        Start {
            request: Request,
            worker: String,
            now: u64,
        },
        Resume {
            continuation: Continuation,
            value: Value,
            now: u64,
        },
        Cleanup {
            request: Request,
            worker: String,
            now: u64,
        },
    }
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("report program input exceeds its bound".into());
    }
    let value = match serde_json::from_slice(bytes).map_err(|e| e.to_string())? {
        Call::Start {
            request,
            worker,
            now,
        } => serde_json::to_value(start(request, worker, now)?),
        Call::Resume {
            continuation,
            value,
            now,
        } => serde_json::to_value(resume(continuation, value, now)?),
        Call::Cleanup {
            request,
            worker,
            now,
        } => {
            return serde_json::to_string(&cleanup(&request, &worker, now)?)
                .map_err(|e| e.to_string())
        }
    }
    .map_err(|e| e.to_string())?;
    serde_json::to_string(&value).map_err(|e| e.to_string())
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name=reportBrowser)]
pub fn wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            fence: Fence {
                account: "owner".into(),
                deployment: "lane-audience".into(),
                install_id: "installation".into(),
            },
            project: "project".into(),
            transformer: "test".into(),
            variant: "default".into(),
        }
    }
    fn observation() -> Value {
        json!({"observation":{"observed_at_ms":1,"heads":[{"work_id":"work1","engine":"network_reporter","operation":"export-test","variant":"default","input_base_fingerprint":"a".repeat(64),"head_revision":1,"updated_at":"2026-10-18T00:00:00Z","outputs":[{"output_id":"xlsx","format":"xlsx","content_type":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet","filename":"test.xlsx","sha256":"b".repeat(64),"size_bytes":123}]}]}})
    }
    fn held(request: Request) -> Step {
        let Step::Store { continuation, .. } = start(request, "worker".into(), 2).unwrap() else {
            panic!()
        };
        resume(continuation, observation(), 3).unwrap()
    }
    #[test]
    fn held_inventory_has_only_sql_and_physical_reads_and_never_requests_authority() {
        let Step::Probe {
            outputs,
            continuation,
        } = held(Request::Inventory {
            scope: scope(),
            refresh: false,
        })
        else {
            panic!()
        };
        assert_eq!(outputs.len(), 1);
        let Step::Done { view } = resume(continuation, json!([null]), 4).unwrap() else {
            panic!()
        };
        assert_eq!(view["cloud_read"]["state"], "held");
        assert!(view["cloud_read"]["error"].is_null());
        assert!(view["outputs"][0]["retained"].is_null());
        assert!(view["outputs"][0]["generated_at_ms"].is_null());
    }
    #[test]
    fn retained_copy_never_falls_back_to_cloud_and_exact_proofs_are_fenced() {
        let request = Request::Artifact {
            scope: scope(),
            source: Source::SyncDownload,
            output_id: "xlsx".into(),
            head_revision: 1,
            sha256: "b".repeat(64),
            size_bytes: 123,
        };
        let Step::Probe {
            outputs,
            continuation,
        } = held(request.clone())
        else {
            panic!()
        };
        let key = outputs[0].key.clone();
        assert!(resume(continuation, json!([null]), 4).is_err());
        let Step::Probe { continuation, .. } = held(request.clone()) else {
            panic!()
        };
        assert!(resume(
            continuation,
            json!([{"sha256":"c".repeat(64),"size_bytes":123}]),
            4
        )
        .is_err());
        let Step::Probe { continuation, .. } = held(request) else {
            panic!()
        };
        assert!(matches!(
            resume(
                continuation,
                json!([{"sha256":"b".repeat(64),"size_bytes":123}]),
                4
            )
            .unwrap(),
            Step::Bytes { .. }
        ));
        let mut other = scope();
        other.fence.account = "another".into();
        let Step::Probe { outputs, .. } = held(Request::Inventory {
            scope: other,
            refresh: false,
        }) else {
            panic!()
        };
        assert_ne!(key, outputs[0].key);
    }
    #[test]
    fn exact_artifact_revision_cannot_select_newer_equal_bytes() {
        let request = Request::Artifact {
            scope: scope(),
            source: Source::SyncDownload,
            output_id: "xlsx".into(),
            head_revision: 2,
            sha256: "b".repeat(64),
            size_bytes: 123,
        };
        let Step::Store { continuation, .. } = start(request, "worker".into(), 2).unwrap() else {
            panic!()
        };
        assert!(resume(continuation, observation(), 3).is_err());
    }
}
