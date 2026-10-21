//! Saved-local metadata admission. Hosts supply captured metadata and native
//! session facts; this module performs no storage, acquisition, save or solve.
//! SourceFacts must come from the strict package/digest owner after consuming
//! the one held checkpoint. Their byte truth is not established by this module.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_RECORDS: usize = 500;
pub const MAX_PAGE: usize = 50;
/// Mirrors the native stored-package IO owner's `MAX_PACKAGE_BYTES` in
/// `ds-layer-store/src/local_models.rs`; it does not widen metadata JSON.
pub const MAX_STORED_SOURCE_BYTES: usize = 512 * 1024 * 1024;

/// Identity of exact held bytes, without any claim of package/model validity.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct SourceDigest {
    pub digest: String,
    pub size_bytes: u64,
}

fn admit_source_length(length: usize) -> Result<(), Refusal> {
    if length > MAX_STORED_SOURCE_BYTES {
        return refuse(
            "saved_model_source_too_large",
            "the held stored-source bytes exceed 512 MiB",
        );
    }
    Ok(())
}

/// Hash the same private byte view the host will hand to the strict reader,
/// before transferring or detaching it. No decoding, IO or mutation occurs.
/// Empty/malformed bytes still have a digest; the package reader must refuse
/// them independently. Native callers use this directly; WASM exposes the
/// same function separately from the bounded metadata facts endpoint.
pub fn source_digest(bytes: &[u8]) -> Result<SourceDigest, Refusal> {
    admit_source_length(bytes.len())?;
    Ok(SourceDigest {
        digest: format!("{:x}", Sha256::digest(bytes)),
        size_bytes: bytes.len() as u64,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Workspace {
    Temporary,
    MvDesign,
}

/// Generation changes on every context transition, including an account or
/// project that changes away and back while local IO is pending.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    #[serde(deserialize_with = "required_nullable")]
    pub account: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub project: Option<String>,
    pub lane: String,
    pub workspace: Workspace,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBinding {
    pub project: String,
    pub model_id: String,
    pub revision: String,
    pub digest: String,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub model_id: String,
    pub manifest_model_id: Option<String>,
    pub name: String,
    pub revision: String,
    pub digest: Option<String>,
    pub size_bytes: u64,
    pub updated: u64,
    pub workspace_owner: Option<Workspace>,
    pub project_binding: Option<ProjectBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub model_id: String,
    pub revision: String,
    pub digest: String,
}

/// Native SessionHistoryState fields, retained as an opaque history fence.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub model_revision: String,
    pub initial_revision: String,
    pub undo_depth: u64,
    pub redo_depth: u64,
    pub history_pin: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Warm {
    pub model_id: String,
    pub history: History,
    /// The host's admitted durable-state fact is echoed, never inferred from
    /// revision equality or used to overwrite the current worker.
    pub persisted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Activity {
    pub saving: bool,
    pub authoring: bool,
    /// Remains true after visible cancellation until physical worker release.
    pub physical_worker_active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFacts {
    pub manifest_model_id: String,
    pub digest: String,
    pub size_bytes: u64,
    pub coordinate_system: String,
    pub history: History,
}

fn required_nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(deserializer)
}

fn default_limit() -> usize {
    MAX_PAGE
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    SavedList {
        context: Context,
        captured_context: Context,
        records: Vec<Record>,
        #[serde(default)]
        offset: usize,
        #[serde(default = "default_limit")]
        limit: usize,
    },
    SavedOpen {
        context: Context,
        captured_context: Context,
        expected: Pin,
        record: Option<Box<Record>>,
        warm: Option<Box<Warm>>,
        activity: Activity,
        cancelled: bool,
        source: Option<Box<SourceFacts>>,
    },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ListRow {
    pub record: Record,
    /// Legacy metadata missing a manifest identity or digest remains visible;
    /// opening it refuses rather than fetching, repairing or saving it.
    pub pin: Option<Pin>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Outcome {
    SavedList {
        context: Context,
        total: usize,
        offset: usize,
        next_offset: Option<usize>,
        rows: Vec<ListRow>,
        mutated: bool,
    },
    ConsumeStoredSource {
        context: Context,
        record: Box<Record>,
        pin: Pin,
        mutated: bool,
    },
    RetainWarm {
        context: Context,
        stored_pin: Pin,
        live: Box<Warm>,
        mutated: bool,
    },
    AdmitStoredSource {
        context: Context,
        record: Box<Record>,
        pin: Pin,
        source: Box<SourceFacts>,
        mutated: bool,
    },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub message: &'static str,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

fn refuse<T>(code: &'static str, message: &'static str) -> Result<T, Refusal> {
    Err(Refusal { code, message })
}

fn valid_context(value: &Context) -> bool {
    super::identifier(&value.lane)
        && value.account.as_deref().is_none_or(super::identifier)
        && value.project.as_deref().is_none_or(super::identifier)
}

fn fence(context: &Context, captured: &Context) -> Result<(), Refusal> {
    if !valid_context(context) || !valid_context(captured) {
        return refuse("saved_model_request_invalid", "invalid saved-local context");
    }
    if context != captured {
        return refuse(
            "saved_model_context_stale",
            "the saved-local context changed",
        );
    }
    Ok(())
}

fn valid_record(row: &Record) -> bool {
    super::identifier(&row.model_id)
        && row
            .manifest_model_id
            .as_deref()
            .is_none_or(super::identifier)
        && super::identifier(&row.revision)
        && row.size_bytes > 0
        && row.digest.as_deref().is_none_or(valid_digest)
        && row.project_binding.as_ref().is_none_or(|p| {
            super::identifier(&p.project)
                && super::identifier(&p.model_id)
                && super::identifier(&p.revision)
                && valid_digest(&p.digest)
        })
}

fn valid_digest(value: &str) -> bool {
    value.len() <= 71 && value.trim() == value && super::digest(value).is_some()
}

fn pin(row: &Record) -> Option<Pin> {
    row.manifest_model_id.as_ref()?;
    Some(Pin {
        model_id: row.model_id.clone(),
        revision: row.revision.clone(),
        digest: super::digest(row.digest.as_deref()?)?,
    })
}

/// Exact existing workspace admission: bound foreign-project copies are
/// excluded, and an unbound MV copy remains a local/Temporary model.
fn admits(context: &Context, row: &Record) -> bool {
    let binding = row.project_binding.as_ref();
    if binding.is_some_and(|b| context.project.as_ref().is_some_and(|p| p != &b.project)) {
        return false;
    }
    match context.workspace {
        Workspace::MvDesign => {
            row.workspace_owner == Some(Workspace::MvDesign)
                && binding.is_some()
                && context.project.is_some()
        }
        Workspace::Temporary => {
            row.workspace_owner
                .as_ref()
                .unwrap_or(&Workspace::Temporary)
                == &Workspace::Temporary
                || binding.is_none()
        }
    }
}

fn valid_history(value: &History) -> bool {
    super::identifier(&value.model_revision)
        && super::identifier(&value.initial_revision)
        && super::identifier(&value.history_pin)
}

pub fn evaluate(request: Request) -> Result<Outcome, Refusal> {
    match request {
        Request::SavedList {
            context,
            captured_context,
            mut records,
            offset,
            limit,
        } => {
            fence(&context, &captured_context)?;
            if records.len() > MAX_RECORDS
                || limit == 0
                || limit > MAX_PAGE
                || records.iter().any(|r| !valid_record(r))
            {
                return refuse(
                    "saved_model_request_invalid",
                    "invalid saved-local metadata or page bounds",
                );
            }
            let mut seen = std::collections::BTreeSet::new();
            if records.iter().any(|r| !seen.insert(&r.model_id)) {
                return refuse(
                    "saved_model_request_invalid",
                    "duplicate saved-local model identity",
                );
            }
            records.retain(|r| admits(&context, r));
            records.sort_by(|a, b| {
                b.updated
                    .cmp(&a.updated)
                    .then_with(|| a.model_id.cmp(&b.model_id))
            });
            let total = records.len();
            if offset > total {
                return refuse(
                    "saved_model_request_invalid",
                    "saved-local page offset exceeds the total",
                );
            }
            let rows: Vec<ListRow> = records
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|record| ListRow {
                    pin: pin(&record),
                    record,
                })
                .collect();
            let next = offset + rows.len();
            Ok(Outcome::SavedList {
                context,
                total,
                offset,
                next_offset: (next < total).then_some(next),
                rows,
                mutated: false,
            })
        }
        Request::SavedOpen {
            context,
            captured_context,
            expected,
            record,
            warm,
            activity,
            cancelled,
            source,
        } => {
            fence(&context, &captured_context)?;
            if cancelled {
                return refuse(
                    "saved_model_open_cancelled",
                    "the saved-local open was cancelled",
                );
            }
            let record = record.ok_or(Refusal {
                code: "saved_model_not_found",
                message: "the saved-local model is absent",
            })?;
            if !valid_record(&record)
                || !super::identifier(&expected.model_id)
                || !super::identifier(&expected.revision)
                || !valid_digest(&expected.digest)
            {
                return refuse(
                    "saved_model_request_invalid",
                    "invalid saved-local metadata or selector",
                );
            }
            if !admits(&context, &record) {
                return refuse(
                    "saved_model_context_mismatch",
                    "the saved-local copy does not belong to this workspace context",
                );
            }
            let stored = pin(&record).ok_or(Refusal {
                code: "saved_model_unpinned",
                message: "the saved-local copy lacks exact manifest identity or digest",
            })?;
            if expected.model_id != stored.model_id
                || expected.revision != stored.revision
                || super::digest(&expected.digest).as_ref() != Some(&stored.digest)
            {
                return refuse(
                    "saved_model_stale",
                    "the saved-local revision or digest changed",
                );
            }
            if activity.saving || activity.authoring || activity.physical_worker_active {
                return refuse(
                    "saved_model_busy",
                    "the native session has not released its active operation",
                );
            }
            if let Some(live) = warm {
                if source.is_some()
                    || live.model_id != stored.model_id
                    || !valid_history(&live.history)
                {
                    return refuse(
                        "saved_model_request_invalid",
                        "invalid warm saved-local facts or unnecessary source acquisition",
                    );
                }
                return Ok(Outcome::RetainWarm {
                    context,
                    stored_pin: stored,
                    live,
                    mutated: false,
                });
            }
            let Some(source) = source else {
                return Ok(Outcome::ConsumeStoredSource {
                    context,
                    record,
                    pin: stored,
                    mutated: false,
                });
            };
            if record.manifest_model_id.as_deref() != Some(&source.manifest_model_id)
                || !valid_digest(&source.digest)
                || super::digest(&source.digest).as_ref() != Some(&stored.digest)
                || source.size_bytes != record.size_bytes
                || !super::identifier(&source.coordinate_system)
                || !valid_history(&source.history)
                || source.history.model_revision != stored.revision
            {
                return refuse(
                    "saved_model_source_mismatch",
                    "the native checkpoint facts do not match the captured saved-local copy",
                );
            }
            Ok(Outcome::AdmitStoredSource {
                context,
                record,
                pin: stored,
                source,
                mutated: false,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_digest_matches_known_vectors_without_claiming_package_validity() {
        for (bytes, expected) in [
            (
                b"".as_slice(),
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                b"abc".as_slice(),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
        ] {
            let identity = source_digest(bytes).unwrap();
            assert_eq!(identity.digest, expected);
            assert_eq!(identity.size_bytes, bytes.len() as u64);
            assert_eq!(
                serde_json::to_value(identity).unwrap(),
                json!({"digest":expected,"size_bytes":bytes.len()})
            );
        }
    }

    #[test]
    fn source_digest_reads_exact_binary_view_and_leaves_owned_buffer_unchanged() {
        let buffer = vec![0, 255, 128, 1, 0];
        let before = buffer.clone();
        let identity = source_digest(&buffer).unwrap();
        assert_eq!(buffer, before);
        assert_eq!(source_digest(&buffer).unwrap(), identity);
        let mut changed = buffer.clone();
        changed[2] ^= 1;
        assert_ne!(source_digest(&changed).unwrap().digest, identity.digest);
        assert_ne!(source_digest(&buffer[1..]).unwrap(), identity);
        assert_eq!(source_digest(&buffer[1..]).unwrap().size_bytes, 4);
    }

    #[test]
    fn source_digest_size_guard_uses_stored_package_bound_before_hashing() {
        // The same guard source_digest invokes, without allocating 512 MiB.
        assert!(admit_source_length(0).is_ok());
        assert!(admit_source_length(MAX_STORED_SOURCE_BYTES).is_ok());
        assert_eq!(
            admit_source_length(MAX_STORED_SOURCE_BYTES + 1)
                .unwrap_err()
                .code,
            "saved_model_source_too_large"
        );
        assert_eq!(MAX_STORED_SOURCE_BYTES as u64, 512 * 1024 * 1024);
    }

    fn context() -> Context {
        Context {
            account: Some("operator".into()),
            project: Some("project-a".into()),
            lane: "canary".into(),
            workspace: Workspace::Temporary,
            generation: 4,
        }
    }
    fn record(id: &str, updated: u64) -> Record {
        Record {
            model_id: id.into(),
            manifest_model_id: Some("native-model".into()),
            name: "Saved line".into(),
            revision: "rev:saved".into(),
            digest: Some("a".repeat(64)),
            size_bytes: 1234,
            updated,
            workspace_owner: Some(Workspace::Temporary),
            project_binding: None,
        }
    }
    fn history(revision: &str) -> History {
        History {
            model_revision: revision.into(),
            initial_revision: "rev:saved".into(),
            undo_depth: 0,
            redo_depth: 0,
            history_pin: "history:saved".into(),
        }
    }
    fn open() -> Request {
        let row = record("local-saved", 10);
        Request::SavedOpen {
            context: context(),
            captured_context: context(),
            expected: pin(&row).unwrap(),
            record: Some(Box::new(row)),
            warm: None,
            activity: Activity {
                saving: false,
                authoring: false,
                physical_worker_active: false,
            },
            cancelled: false,
            source: None,
        }
    }
    fn source() -> SourceFacts {
        SourceFacts {
            manifest_model_id: "native-model".into(),
            digest: "a".repeat(64),
            size_bytes: 1234,
            coordinate_system: "EPSG:32735".into(),
            history: history("rev:saved"),
        }
    }
    fn list(records: Vec<Record>, offset: usize, limit: usize) -> Request {
        Request::SavedList {
            context: context(),
            captured_context: context(),
            records,
            offset,
            limit,
        }
    }

    #[test]
    fn closed_dtos_refuse_raw_bytes_unknown_fields_and_missing_context_facts() {
        let original = serde_json::to_value(open()).unwrap();
        for key in ["checkpoint_bytes", "path", "save", "analyze"] {
            let mut input = original.clone();
            input[key] = json!(true);
            assert!(serde_json::from_value::<Request>(input).is_err(), "{key}");
        }
        for key in ["account", "project", "lane", "workspace", "generation"] {
            let mut input = original.clone();
            input["context"].as_object_mut().unwrap().remove(key);
            assert!(serde_json::from_value::<Request>(input).is_err(), "{key}");
        }
        let mut input = original;
        input["record"]["dsgrid"] = json!([1, 2, 3]);
        assert!(serde_json::from_value::<Request>(input).is_err());
    }

    #[test]
    fn real_parent_evaluator_keeps_one_mib_cap_and_existing_project_facts() {
        let encoded = serde_json::to_vec(&list(vec![record("saved", 1)], 0, 50)).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&super::super::evaluate(&encoded).unwrap()).unwrap();
        assert_eq!(result["action"], "saved_list");
        assert_eq!(result["rows"][0]["pin"]["model_id"], "saved");
        let mut over = encoded;
        over.resize(1024 * 1024 + 1, b' ');
        assert_eq!(
            super::super::evaluate(&over).unwrap_err(),
            "model management facts exceed 1 MiB"
        );
        let error = super::super::evaluate(br#"{"action":"saved_open","checkpoint_bytes":[1]}"#)
            .unwrap_err();
        assert!(error.starts_with("saved_model_request_invalid:"));
        let old = json!({"action":"working_copy","project_selected":false});
        assert_eq!(
            super::super::evaluate(&serde_json::to_vec(&old).unwrap()).unwrap(),
            "{\"kind\":\"no_project\"}"
        );
    }

    #[test]
    fn list_pages_exact_metadata_in_deterministic_order_without_source_facts() {
        let mut legacy = record("legacy", 1);
        legacy.manifest_model_id = None;
        legacy.digest = None;
        let rows = vec![record("b", 2), legacy.clone(), record("a", 2)];
        let before = serde_json::to_vec(&rows).unwrap();
        let Outcome::SavedList {
            total,
            rows: page,
            next_offset,
            mutated,
            ..
        } = evaluate(list(rows.clone(), 0, 2)).unwrap()
        else {
            panic!("list")
        };
        assert_eq!(total, 3);
        assert_eq!(
            page.iter()
                .map(|r| r.record.model_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(next_offset, Some(2));
        assert!(!mutated);
        let Outcome::SavedList {
            rows: last,
            next_offset,
            ..
        } = evaluate(list(rows.clone(), 2, 2)).unwrap()
        else {
            panic!("list")
        };
        assert_eq!(last[0].record, legacy);
        assert_eq!(last[0].pin, None);
        assert_eq!(next_offset, None);
        assert_eq!(serde_json::to_vec(&rows).unwrap(), before);
        assert!(matches!(
            evaluate(list(Vec::new(), 0, 50)).unwrap(),
            Outcome::SavedList {
                total: 0,
                next_offset: None,
                ..
            }
        ));
    }

    #[test]
    fn list_refuses_bad_bounds_duplicates_and_oversized_or_malformed_metadata() {
        for request in [
            list(vec![], 0, 0),
            list(vec![], 0, 51),
            list(vec![], 1, 1),
            list(vec![record("same", 1), record("same", 2)], 0, 1),
            list(
                (0..501).map(|i| record(&format!("id-{i}"), 1)).collect(),
                0,
                1,
            ),
        ] {
            assert_eq!(
                evaluate(request).unwrap_err().code,
                "saved_model_request_invalid"
            );
        }
        for index in 0..4 {
            let mut row = record("valid", 1);
            match index {
                0 => row.model_id = "x".repeat(257),
                1 => row.revision.clear(),
                2 => row.digest = Some(format!(" {}", "a".repeat(64))),
                _ => row.size_bytes = 0,
            }
            assert_eq!(
                evaluate(list(vec![row], 0, 1)).unwrap_err().code,
                "saved_model_request_invalid"
            );
        }
    }

    #[test]
    fn display_names_are_opaque_passthrough_metadata() {
        // The existing storage owner accepts these display names. They are
        // neither model identity nor permission, and must not brick its list.
        for name in [
            "x".repeat(600),
            "  imported name  ".into(),
            "label\n\t\0suffix".into(),
            String::new(),
        ] {
            let mut row = record("local-saved", 1);
            row.name = name.clone();
            let input = serde_json::to_vec(&list(vec![row.clone()], 0, 50)).unwrap();
            let output: serde_json::Value =
                serde_json::from_str(&super::super::evaluate(&input).unwrap()).unwrap();
            assert_eq!(output["rows"][0]["record"]["name"], name);
            let mut request = open();
            let Request::SavedOpen { record, .. } = &mut request else {
                unreachable!()
            };
            *record = Some(Box::new(row.clone()));
            let Outcome::ConsumeStoredSource { record, .. } = evaluate(request).unwrap() else {
                panic!("source")
            };
            assert_eq!(*record, row);
        }
    }

    #[test]
    fn workspace_projection_matches_existing_local_and_project_copy_admission() {
        let mut bound_mv = record("mv", 4);
        bound_mv.workspace_owner = Some(Workspace::MvDesign);
        bound_mv.project_binding = Some(ProjectBinding {
            project: "project-a".into(),
            model_id: "project-model".into(),
            revision: "r1".into(),
            digest: "b".repeat(64),
            version: 2,
        });
        let mut other = bound_mv.clone();
        other.model_id = "other".into();
        other.project_binding.as_mut().unwrap().project = "project-b".into();
        let mut detached = bound_mv.clone();
        detached.model_id = "detached".into();
        detached.project_binding = None;
        for (workspace, project, expected) in [
            (
                Workspace::Temporary,
                Some("project-a"),
                vec!["detached", "local"],
            ),
            (Workspace::MvDesign, Some("project-a"), vec!["mv"]),
            (Workspace::MvDesign, None, vec![]),
            (Workspace::Temporary, None, vec!["detached", "local"]),
        ] {
            let mut ctx = context();
            ctx.workspace = workspace;
            ctx.project = project.map(str::to_owned);
            let result = evaluate(Request::SavedList {
                context: ctx.clone(),
                captured_context: ctx,
                records: vec![
                    bound_mv.clone(),
                    other.clone(),
                    detached.clone(),
                    record("local", 1),
                ],
                offset: 0,
                limit: 50,
            })
            .unwrap();
            let Outcome::SavedList { rows, .. } = result else {
                panic!("list")
            };
            assert_eq!(
                rows.iter()
                    .map(|r| r.record.model_id.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
        }
        let mut request = open();
        let Request::SavedOpen { record, .. } = &mut request else {
            unreachable!()
        };
        *record = Some(Box::new(other));
        assert_eq!(
            evaluate(request).unwrap_err().code,
            "saved_model_context_mismatch"
        );
    }

    #[test]
    fn context_fences_reject_account_project_lane_workspace_and_aba_generation() {
        for index in 0..5 {
            let mut request = open();
            let Request::SavedOpen { context, .. } = &mut request else {
                unreachable!()
            };
            match index {
                0 => context.account = Some("other".into()),
                1 => context.project = Some("other".into()),
                2 => context.lane = "stable".into(),
                3 => context.workspace = Workspace::MvDesign,
                _ => context.generation += 2,
            }
            assert_eq!(
                evaluate(request).unwrap_err().code,
                "saved_model_context_stale"
            );
        }
        let mut request = list(vec![], 0, 50);
        let Request::SavedList {
            context,
            captured_context,
            ..
        } = &mut request
        else {
            unreachable!()
        };
        context.account = None;
        captured_context.account = None;
        context.project = None;
        captured_context.project = None;
        assert!(
            evaluate(request).is_ok(),
            "anonymous retained local metadata is valid"
        );
    }

    #[test]
    fn selector_missing_unpinned_or_moved_refuses_without_acquisition_plan() {
        for index in 0..5 {
            let mut request = open();
            let Request::SavedOpen {
                expected, record, ..
            } = &mut request
            else {
                unreachable!()
            };
            let code = match index {
                0 => {
                    *record = None;
                    "saved_model_not_found"
                }
                1 => {
                    record.as_mut().unwrap().digest = None;
                    "saved_model_unpinned"
                }
                2 => {
                    expected.model_id = "other".into();
                    "saved_model_stale"
                }
                3 => {
                    expected.revision = "rev:previous".into();
                    "saved_model_stale"
                }
                _ => {
                    expected.digest = "b".repeat(64);
                    "saved_model_stale"
                }
            };
            assert_eq!(evaluate(request).unwrap_err().code, code);
        }
    }

    #[test]
    fn cold_open_requests_one_named_source_then_admits_only_matching_native_facts() {
        let Outcome::ConsumeStoredSource { pin, mutated, .. } = evaluate(open()).unwrap() else {
            panic!("source")
        };
        assert_eq!(pin.model_id, "local-saved");
        assert!(!mutated);
        let mut request = open();
        let Request::SavedOpen { source: facts, .. } = &mut request else {
            unreachable!()
        };
        *facts = Some(Box::new(source()));
        let Outcome::AdmitStoredSource {
            source: admitted,
            mutated,
            ..
        } = evaluate(request.clone()).unwrap()
        else {
            panic!("admit")
        };
        assert_eq!(*admitted, source());
        assert!(!mutated);
        // Re-admission after IO checks the current stored row as well as the
        // originally captured selector: a concurrent Save cannot redirect it.
        let Request::SavedOpen { record, .. } = &mut request else {
            unreachable!()
        };
        record.as_mut().unwrap().digest = Some("b".repeat(64));
        assert_eq!(evaluate(request).unwrap_err().code, "saved_model_stale");
    }

    #[test]
    fn native_source_identity_digest_bytes_crs_and_history_must_match() {
        for index in 0..7 {
            let mut facts = source();
            match index {
                0 => facts.manifest_model_id = "other".into(),
                1 => facts.digest = "b".repeat(64),
                2 => facts.size_bytes += 1,
                3 => facts.coordinate_system.clear(),
                4 => facts.history.model_revision = "rev:other".into(),
                5 => facts.history.history_pin.clear(),
                _ => facts.digest = format!(" {}", facts.digest),
            }
            let mut request = open();
            let Request::SavedOpen { source, .. } = &mut request else {
                unreachable!()
            };
            *source = Some(Box::new(facts));
            assert_eq!(
                evaluate(request).unwrap_err().code,
                "saved_model_source_mismatch"
            );
        }
    }

    #[test]
    fn warm_dirty_history_is_retained_exactly_without_saved_source_or_reset() {
        let mut live = Warm {
            model_id: "local-saved".into(),
            history: history("rev:dirty"),
            persisted: false,
        };
        live.history.undo_depth = 12;
        live.history.redo_depth = 3;
        live.history.history_pin = "history:undo-redo-current".into();
        let mut request = open();
        let Request::SavedOpen { warm, .. } = &mut request else {
            unreachable!()
        };
        *warm = Some(Box::new(live.clone()));
        let Outcome::RetainWarm {
            live: retained,
            stored_pin,
            mutated,
            ..
        } = evaluate(request.clone()).unwrap()
        else {
            panic!("warm")
        };
        assert_eq!(*retained, live);
        assert!(!mutated);
        assert_eq!(stored_pin.revision, "rev:saved");
        let Request::SavedOpen { source: facts, .. } = &mut request else {
            unreachable!()
        };
        *facts = Some(Box::new(source()));
        assert_eq!(
            evaluate(request).unwrap_err().code,
            "saved_model_request_invalid",
            "warm source was not consumed"
        );
    }

    #[test]
    fn cancellation_and_native_activity_gate_every_open_phase_including_physical_release() {
        for phase in 0..3 {
            for blocked in 0..4 {
                let mut request = open();
                let Request::SavedOpen {
                    activity,
                    cancelled,
                    warm,
                    source: facts,
                    ..
                } = &mut request
                else {
                    unreachable!()
                };
                match phase {
                    1 => {
                        *warm = Some(Box::new(Warm {
                            model_id: "local-saved".into(),
                            history: history("rev:dirty"),
                            persisted: false,
                        }))
                    }
                    2 => *facts = Some(Box::new(source())),
                    _ => {}
                }
                match blocked {
                    0 => *cancelled = true,
                    1 => activity.saving = true,
                    2 => activity.authoring = true,
                    _ => activity.physical_worker_active = true,
                }
                let before = serde_json::to_vec(&request).unwrap();
                let error = evaluate(request.clone()).unwrap_err();
                assert_eq!(
                    error.code,
                    if blocked == 0 {
                        "saved_model_open_cancelled"
                    } else {
                        "saved_model_busy"
                    }
                );
                assert_eq!(serde_json::to_vec(&request).unwrap(), before);
            }
        }
    }

    #[test]
    fn native_metadata_size_bound() {
        let rows = (0..MAX_PAGE)
            .map(|i| {
                let mut row = record(&format!("{i:03}{}", "x".repeat(253)), 1);
                row.name = "🌍".repeat(200);
                row.manifest_model_id = Some("m".repeat(256));
                row.revision = "r".repeat(256);
                row.project_binding = Some(ProjectBinding {
                    project: "project-a".into(),
                    model_id: "m".repeat(256),
                    revision: "r".repeat(256),
                    digest: "b".repeat(64),
                    version: u64::MAX,
                });
                row
            })
            .collect();
        let encoded = serde_json::to_vec(&evaluate(list(rows, 0, MAX_PAGE)).unwrap()).unwrap();
        // This size bound belongs only to this fixture, not to all valid
        // display names. The MCP owner must prove its complete enclosing
        // result envelope after the commands exist.
        assert!(
            encoded.len() < 200_000,
            "{} native metadata fixture bytes; full MCP framing remains unproved",
            encoded.len()
        );
    }
}
