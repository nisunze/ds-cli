//! Sequential, resumable governed Survey entry import.

use std::collections::BTreeSet;
use std::fs::{File, Metadata, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{SurveyEntryCreateOrigin, SurveyEntryCreateReceipt, SurveyEntryCreateRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

const SOURCE_MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const LINE_MAX_BYTES: usize = 1024 * 1024;
const ITEM_MAX: usize = 100_000;
const CHECKPOINT_MAX_BYTES: u64 = 64 * 1024;
const RECEIPT_MAX_BYTES: u64 = 128 * 1024 * 1024;
const CHECKPOINT_SCHEMA: &str = "ds.survey.entries.import-checkpoint/v1";
const RECEIPT_SCHEMA: &str = "ds.survey.entries.import-receipt/v1";
static STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

const FORM: Arg = Arg::value("form", "<form-slug>", "One governed form for every row.").required();
const FILE: Arg = Arg::value(
    "file",
    "<ndjson-path>",
    "Regular non-link canonical entry NDJSON; validated twice before auth.",
)
.required();
const CHECKPOINT: Arg = Arg::value(
    "checkpoint",
    "<json-path>",
    "Private atomic checkpoint bound to source, principal, project, and form.",
)
.required();
const RECEIPT: Arg = Arg::value(
    "receipt",
    "<ndjson-path>",
    "Private synced redacted item receipt.",
)
.required();
const ON_ERROR: Arg = Arg::value(
    "on-error",
    "<stop|continue>",
    "Stop after the first exact row-local refusal, or record it and continue.",
)
.default("stop")
.choices(&["stop", "continue"]);
const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "survey_entries_import_source_invalid",
        when: "the NDJSON source is missing, linked, non-regular, empty, oversized, unreadable, or outside its item/line bounds",
        remedy: "pass one unchanged regular non-link NDJSON file within the documented bounds",
    },
    Refusal {
        code: "survey_entries_import_line_invalid",
        when: "a row is blank, invalid JSON, has unknown or missing keys, null optional buckets, non-canonical context, or violates the shared create grammar",
        remedy: "shape every row to the exact canonical import object shown by this command",
    },
    Refusal {
        code: "survey_entries_import_duplicate_identity",
        when: "two rows reuse an idempotency key or the same canonical context/document identity",
        remedy: "give every logical source row one unique stable document identity and opaque idempotency key",
    },
    Refusal {
        code: "survey_entries_import_source_changed",
        when: "the source bytes differ between validation or before a row is submitted",
        remedy: "restore the exact checkpoint-bound source bytes and resume without editing in place",
    },
    Refusal {
        code: "survey_entries_import_path_conflict",
        when: "source, checkpoint, and receipt resolve to the same path or unsafe parent",
        remedy: "use three distinct ordinary paths under non-link directories",
    },
    Refusal {
        code: "survey_entries_import_checkpoint_unsafe",
        when: "the checkpoint is linked, non-regular, non-private, oversized, malformed, or cannot be atomically replaced",
        remedy: "repair or remove only the exact checkpoint after preserving its evidence, then resume with the same source",
    },
    Refusal {
        code: "survey_entries_import_checkpoint_mismatch",
        when: "the checkpoint is bound to different source bytes, lane, audience, principal, project, form, or receipt",
        remedy: "restore the exact original inputs and selected project, or choose fresh checkpoint and receipt paths for a new import",
    },
    Refusal {
        code: "survey_entries_import_receipt_unsafe",
        when: "the receipt is linked, non-regular, non-private, oversized, malformed, non-contiguous, or cannot be synced",
        remedy: "preserve and repair the exact receipt; never delete committed evidence merely to restart",
    },
    Refusal {
        code: "survey_entries_import_receipt_mismatch",
        when: "receipt evidence disagrees with the checkpoint or canonical source row",
        remedy: "restore the matching checkpoint, receipt, and source trio before resuming",
    },
    Refusal {
        code: "survey_entries_import_state_incomplete",
        when: "only a checkpoint exists, or a receipt-only recovery contains item events",
        remedy: "restore the matching state pair; a header-only receipt may be recovered automatically",
    },
    Refusal {
        code: "survey_entries_import_state_conflict",
        when: "another process owns this exact import checkpoint",
        remedy: "let that import finish, then resume the same state pair",
    },
    Refusal {
        code: "survey_entries_import_windows_state_unavailable",
        when: "Windows has no proven owner-private resumable-import state adapter",
        remedy: "use Linux or install a future ds build with a proven Windows protected-state root",
    },
    Refusal {
        code: "survey_entries_import_paused",
        when: "a create outcome is not exact row-local terminal, is uncertain, or is retryable",
        remedy: "resolve the reported bounded cause, then resume the exact source with unchanged document ids and idempotency keys",
    },
    Refusal {
        code: "survey_entry_create_invalid",
        when: "the fixed service rejects a locally valid canonical row",
        remedy: "verify the form contract and canonical row before resuming",
    },
    Refusal {
        code: "survey_entry_create_auth_rejected",
        when: "the fixed create route rejects the restored native session",
        remedy: "sign in again, then resume the exact import",
    },
    Refusal {
        code: "survey_entry_create_permission_denied",
        when: "the verified user lacks entries.create authority for the form",
        remedy: "request entries.create authority for the selected project and form",
    },
    Refusal {
        code: "survey_entry_create_scope_not_found",
        when: "the selected project, form, or context ancestor is unavailable",
        remedy: "verify the selected project, form, and context ancestors",
    },
    Refusal {
        code: "survey_entry_create_form_disabled",
        when: "the form is disabled for entry creation",
        remedy: "enable the project form before resuming",
    },
    Refusal {
        code: "survey_entry_create_project_read_only",
        when: "the selected project lifecycle is read-only",
        remedy: "select the intended active writable project",
    },
    Refusal {
        code: "survey_entry_create_idempotency_conflict",
        when: "a row key is bound to another mutation",
        remedy: "restore the original exact source row and key; never replace the key to force a replay",
    },
    Refusal {
        code: "survey_entry_create_already_exists",
        when: "a document exists without exact create replay evidence",
        remedy: "resolve the source/document collision before continuing",
    },
    Refusal {
        code: "survey_entry_create_refused",
        when: "the backend coarsely refuses a locally valid row",
        remedy: "verify the form, identity, context, and payload bounds",
    },
    Refusal {
        code: "survey_entry_create_failed",
        when: "the fixed create service has a temporary failure",
        remedy: "after recovery, resume with the exact source and keys",
    },
    Refusal {
        code: "survey_entry_create_unreadable",
        when: "the create receipt violates its closed identity or clock contract",
        remedy: "verify the backend release and update ds before resuming",
    },
    Refusal {
        code: "native_profile_not_configured",
        when: "the exact packaged native profile is unavailable",
        remedy: "install one complete ds release",
    },
    Refusal {
        code: "native_profile_digest_mismatch",
        when: "the packaged catalogue differs from the build pin",
        remedy: "reinstall one complete ds release",
    },
    Refusal {
        code: "native_profile_unsafe",
        when: "the packaged native catalogue is unsafe or malformed",
        remedy: "reinstall one complete ds release",
    },
    Refusal {
        code: "headless_signed_out",
        when: "the selected lane has no restorable native user",
        remedy: "run ds auth login --email <address>",
    },
    Refusal {
        code: "headless_project_not_selected",
        when: "the user has no audience-fenced selected project",
        remedy: "run ds auth project use --project <exact-id>",
    },
    Refusal {
        code: "project_context_stale",
        when: "saved project context belongs to another identity, lane, or audience",
        remedy: "select the intended project again",
    },
    Refusal {
        code: "native_state_unsafe",
        when: "protected native state is unsafe or unreadable",
        remedy: "repair the owner-only DS config directory",
    },
    Refusal {
        code: "native_state_unavailable",
        when: "protected native state cannot be accessed",
        remedy: "repair the owner-only DS config directory",
    },
    Refusal {
        code: "native_state_protection_unavailable",
        when: "this build has no protected-state adapter",
        remedy: "install a supported native ds build",
    },
    Refusal {
        code: "native_state_root_invalid",
        when: "the configured state root is not absolute",
        remedy: "unset it or provide an absolute path",
    },
    Refusal {
        code: "native_state_conflict",
        when: "another native operation holds the protected-state lease",
        remedy: "retry after that operation finishes",
    },
    Refusal {
        code: "native_cleanup_required",
        when: "revoked identity cleanup cannot clear project context",
        remedy: "repair protected state and run auth logout",
    },
    Refusal {
        code: "auth_rejected",
        when: "identity restoration rejects the saved credential before import",
        remedy: "verify the account and sign in again if revoked",
    },
    Refusal {
        code: "auth_revoked",
        when: "Firebase permanently revokes the native session",
        remedy: "sign in again interactively",
    },
    Refusal {
        code: "auth_identity_mismatch",
        when: "Firebase returns an identity outside the bound session",
        remedy: "sign in again and report a repeated mismatch",
    },
    Refusal {
        code: "auth_transient",
        when: "native identity restoration is temporarily unavailable",
        remedy: "retry without changing import state",
    },
    Refusal {
        code: "auth_response_unreadable",
        when: "native identity restoration returns an unreadable response",
        remedy: "retry once, then update ds if it persists",
    },
];

pub static COMMAND: Command = Command {
    id: "survey.entries.import",
    path: &["survey", "entries", "import"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Import bounded canonical Survey entry NDJSON headlessly.",
    purpose: "Validates all NDJSON twice before auth, freezes project/form, then runs sequential governed creates. Syncs a redacted receipt before checkpoint; resume never auto-retries. Caller owns data and created_at; authority owns identity and audit. No provenance, override, concurrency, fallback, Desktop, or transport escape.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[FORM, FILE, CHECKPOINT, RECEIPT, ON_ERROR, LANE],
    output: "Bounded progress/state/mirror summary and digest receipts; no payload, fields, coordinates, keys, token, or email.",
    examples: &[Example {
        command: "ds survey entries import --form lv_poles_survey --file ./survey123.ndjson --checkpoint ./survey123.checkpoint.json --receipt ./survey123.receipt.ndjson --yes --output json",
        note: "Imports one immutable canonical source sequentially.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: available,
};

fn available() -> Availability {
    windows_import_availability(cfg!(windows))
}

fn windows_import_availability(is_windows: bool) -> Availability {
    if is_windows {
        Availability::unavailable(
            "survey_entries_import_windows_state_unavailable",
            "resumable Survey import cannot prove owner-private durable state on Windows",
            "use Linux or install a future ds build with a proven Windows protected-state root",
        )
    } else {
        Availability::Available
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportRow {
    doc_id: String,
    idempotency_key: String,
    data: Map<String, Value>,
    metadata: ImportMetadata,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_present_non_null")]
    context_key: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_present_non_null")]
    geometry: Option<Value>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_present_non_null")]
    connectivity: Option<Map<String, Value>>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_present_non_null")]
    detailed_location: Option<Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportMetadata {
    created_at: String,
}

fn deserialize_present_non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)?
        .map(Some)
        .ok_or_else(|| serde::de::Error::custom("an optional import bucket cannot be null"))
}

struct ParsedRow {
    request: SurveyEntryCreateRequest,
    record_sha256: String,
    doc_id_sha256: String,
    key_sha256: String,
    identity_sha256: String,
}

struct RecordPlan {
    record_sha256: String,
    doc_id_sha256: String,
}

struct ImportPlan {
    source_sha256: String,
    source_bytes: u64,
    source_identity: FileIdentity,
    records: Vec<RecordPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    schema: String,
    source_sha256: String,
    source_bytes: u64,
    total: usize,
    lane: String,
    credential_audience_sha256: String,
    principal_sha256: String,
    project_id: String,
    form: String,
    checkpoint_path_sha256: String,
    receipt_path_sha256: String,
    next_line: usize,
    committed: usize,
    refused: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ReceiptLine {
    Manifest {
        schema: String,
        source_sha256: String,
        source_bytes: u64,
        total: usize,
        lane: String,
        credential_audience_sha256: String,
        principal_sha256: String,
        project_id: String,
        form: String,
        checkpoint_path_sha256: String,
        receipt_path_sha256: String,
    },
    Committed {
        line: usize,
        record_sha256: String,
        form: String,
        doc_id_sha256: String,
        client_version: i64,
        firestore: String,
        bigquery_mirror: String,
        replication_clock: String,
        replication_authority: String,
    },
    Refused {
        line: usize,
        record_sha256: String,
        form: String,
        doc_id_sha256: String,
        code: String,
    },
}

impl ReceiptLine {
    fn line(&self) -> Option<usize> {
        match self {
            Self::Manifest { .. } => None,
            Self::Committed { line, .. } | Self::Refused { line, .. } => Some(*line),
        }
    }
    fn record_sha256(&self) -> Option<&str> {
        match self {
            Self::Manifest { .. } => None,
            Self::Committed { record_sha256, .. } | Self::Refused { record_sha256, .. } => {
                Some(record_sha256)
            }
        }
    }
    fn doc_id_sha256(&self) -> Option<&str> {
        match self {
            Self::Manifest { .. } => None,
            Self::Committed { doc_id_sha256, .. } | Self::Refused { doc_id_sha256, .. } => {
                Some(doc_id_sha256)
            }
        }
    }
    fn is_committed(&self) -> bool {
        matches!(self, Self::Committed { .. })
    }
}

struct ReceiptState {
    manifest: ReceiptLine,
    events: Vec<ReceiptLine>,
}

struct ReceiptObserved {
    state: ReceiptState,
    complete_len: u64,
    full_sha256: String,
    has_partial_tail: bool,
}

enum LocalState {
    Fresh,
    ReceiptOnly(ReceiptObserved),
    Resume(Box<Checkpoint>, ReceiptObserved),
}

struct Paths {
    source: PathBuf,
    checkpoint: PathBuf,
    receipt: PathBuf,
    receipt_lock: PathBuf,
    checkpoint_lock: PathBuf,
}

struct ImportLock {
    _file: File,
}

struct ImportLocks {
    _locks: Vec<ImportLock>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    volume: u32,
    #[cfg(windows)]
    index: u64,
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let form = inputs.require("form")?;
    let lane = inputs.require("lane")?;
    let continue_permanent = inputs.require("on-error")? == "continue";
    let paths = resolve_paths(
        inputs.require("file")?,
        inputs.require("checkpoint")?,
        inputs.require("receipt")?,
    )?;
    validate_existing_path_identities(&paths)?;

    let first = validate_source(&paths.source, form)?;
    let second = validate_source(&paths.source, form)?;
    if !plans_match(&first, &second) {
        return Err(source_changed());
    }
    let plan = second;
    let _locks = acquire_import_locks(&paths)?;
    let local = load_local_state(&paths, &plan, form, lane)?;

    // Only after the complete caller-controlled source and local state have
    // been parsed do profile discovery, auth restoration, and project access begin.
    let mut session = ds_cli_auth::survey_import_session(lane)?;
    let receipt_path_sha256 = digest_text(&paths.receipt.to_string_lossy());
    let checkpoint_path_sha256 = digest_text(&paths.checkpoint.to_string_lossy());
    let mut checkpoint = checkpoint_for(
        &plan,
        form,
        &session,
        &checkpoint_path_sha256,
        &receipt_path_sha256,
    );
    let mut receipt_state = match local {
        LocalState::Fresh => {
            let manifest = manifest_for(
                &plan,
                form,
                &session,
                &checkpoint_path_sha256,
                &receipt_path_sha256,
            );
            create_receipt(&paths.receipt, &manifest)?;
            write_checkpoint(&paths.checkpoint, &checkpoint)?;
            ReceiptState {
                manifest,
                events: Vec::new(),
            }
        }
        LocalState::ReceiptOnly(observed) => {
            validate_manifest(&observed.state.manifest, &checkpoint)?;
            verify_receipt_unchanged(&paths.receipt, &observed)?;
            write_checkpoint(&paths.checkpoint, &checkpoint)?;
            observed.state
        }
        LocalState::Resume(existing, observed) => {
            validate_checkpoint(&existing, &checkpoint)?;
            validate_manifest(&observed.state.manifest, &checkpoint)?;
            verify_receipt_unchanged(&paths.receipt, &observed)?;
            if observed.has_partial_tail {
                truncate_bound_receipt_tail(&paths.receipt, observed.complete_len)?;
            }
            checkpoint = *existing;
            observed.state
        }
    };
    reconcile_checkpoint(&paths.checkpoint, &mut checkpoint, &receipt_state)?;

    let file = open_source(&paths.source)?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut line_number = 0usize;
    let mut source_bytes = 0u64;
    while let Some(raw) = read_bounded_line(&mut reader)? {
        if !add_bounded_bytes(&mut source_bytes, raw.len(), SOURCE_MAX_BYTES) {
            return Err(source_changed());
        }
        line_number += 1;
        digest.update(&raw);
        let parsed =
            parse_row(form, &raw).map_err(|error| error.detail(json!({ "line": line_number })))?;
        let expected = plan
            .records
            .get(line_number - 1)
            .ok_or_else(source_changed)?;
        if parsed.record_sha256 != expected.record_sha256
            || parsed.doc_id_sha256 != expected.doc_id_sha256
        {
            return Err(source_changed().detail(json!({ "line": line_number })));
        }
        if line_number < checkpoint.next_line {
            let event = receipt_state
                .events
                .get(line_number - 1)
                .ok_or_else(receipt_mismatch)?;
            validate_event(event, line_number, &parsed, form)?;
            continue;
        }
        if line_number != checkpoint.next_line {
            return Err(receipt_mismatch());
        }

        match session.create(&parsed.request) {
            Ok(receipt) => {
                let event = committed_event(line_number, &parsed, form, &receipt);
                append_receipt(&paths.receipt, &event)?;
                receipt_state.events.push(event);
                checkpoint.committed += 1;
                checkpoint.next_line += 1;
                write_checkpoint(&paths.checkpoint, &checkpoint)?;
            }
            Err(error) if permanent_row_refusal(error.code()) => {
                let event = ReceiptLine::Refused {
                    line: line_number,
                    record_sha256: parsed.record_sha256,
                    form: form.to_owned(),
                    doc_id_sha256: parsed.doc_id_sha256,
                    code: error.code().to_owned(),
                };
                append_receipt(&paths.receipt, &event)?;
                receipt_state.events.push(event);
                checkpoint.refused += 1;
                checkpoint.next_line += 1;
                write_checkpoint(&paths.checkpoint, &checkpoint)?;
                if !continue_permanent {
                    return Err(
                        error.detail(json!({ "line": line_number, "checkpoint_advanced": true }))
                    );
                }
            }
            Err(error) => {
                return Err(Failure::unavailable(
                    "survey_entries_import_paused",
                    "the import stopped before its contiguous checkpoint could advance",
                )
                .remedy("resolve the bounded cause, then resume the exact source with unchanged document ids and idempotency keys")
                .detail(json!({ "line": line_number, "cause": error.code() })));
            }
        }
    }
    if line_number != plan.records.len()
        || source_bytes != plan.source_bytes
        || format!("{:x}", digest.finalize()) != plan.source_sha256
    {
        return Err(source_changed());
    }
    let final_plan = validate_source(&paths.source, form).map_err(|_| source_changed())?;
    if !plans_match(&plan, &final_plan) {
        return Err(source_changed());
    }

    let complete = checkpoint.next_line == plan.records.len() + 1;
    let status = if complete && checkpoint.refused > 0 {
        "complete_with_refusals"
    } else if complete {
        "complete"
    } else {
        "paused"
    };
    Ok(json!({
        "source_sha256": plan.source_sha256,
        "source_bytes": plan.source_bytes,
        "lane": session.lane(),
        "project": { "ds_project": session.project_id(), "project_name": session.project_name(), "status": session.project_status() },
        "form": form,
        "total": plan.records.len(),
        "committed": checkpoint.committed,
        "permanently_refused": checkpoint.refused,
        "pending": plan.records.len().saturating_sub(checkpoint.next_line.saturating_sub(1)),
        "next_line": checkpoint.next_line,
        "complete": complete,
        "status": status,
        "checkpoint": paths.checkpoint,
        "receipt": paths.receipt,
        "firestore": "terminal rows recorded",
        "bigquery_mirror": "unconfirmed",
    }))
}

fn validate_source(path: &Path, form: &str) -> Result<ImportPlan, Failure> {
    let file = open_source(path)?;
    let metadata = file.metadata().map_err(|_| source_invalid())?;
    let source_identity = opened_file_identity(&file).map_err(|_| source_invalid())?;
    if metadata.len() == 0 || metadata.len() > SOURCE_MAX_BYTES {
        return Err(source_invalid());
    }
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut records = Vec::new();
    let mut source_bytes = 0u64;
    let mut keys = BTreeSet::new();
    let mut identities = BTreeSet::new();
    while let Some(raw) = read_bounded_line(&mut reader)? {
        if !add_bounded_bytes(&mut source_bytes, raw.len(), SOURCE_MAX_BYTES) {
            return Err(source_invalid());
        }
        digest.update(&raw);
        if records.len() >= ITEM_MAX {
            return Err(source_invalid());
        }
        let parsed = parse_row(form, &raw)
            .map_err(|error| error.detail(json!({ "line": records.len() + 1 })))?;
        if !keys.insert(parsed.key_sha256) || !identities.insert(parsed.identity_sha256) {
            return Err(duplicate_identity().detail(json!({ "line": records.len() + 1 })));
        }
        records.push(RecordPlan {
            record_sha256: parsed.record_sha256,
            doc_id_sha256: parsed.doc_id_sha256,
        });
    }
    if records.is_empty() {
        return Err(source_invalid());
    }
    Ok(ImportPlan {
        source_sha256: format!("{:x}", digest.finalize()),
        source_bytes,
        source_identity,
        records,
    })
}

fn plans_match(left: &ImportPlan, right: &ImportPlan) -> bool {
    left.source_sha256 == right.source_sha256
        && left.source_bytes == right.source_bytes
        && left.source_identity == right.source_identity
        && left.records.len() == right.records.len()
        && left
            .records
            .iter()
            .zip(&right.records)
            .all(|(a, b)| a.record_sha256 == b.record_sha256 && a.doc_id_sha256 == b.doc_id_sha256)
}

fn parse_row(form: &str, raw: &[u8]) -> Result<ParsedRow, Failure> {
    let json_bytes = strip_line_ending(raw);
    if json_bytes.is_empty() {
        return Err(line_invalid());
    }
    let row: ImportRow = serde_json::from_slice(json_bytes).map_err(|_| line_invalid())?;
    if row
        .context_key
        .as_deref()
        .is_some_and(|value| value.contains('%'))
    {
        return Err(line_invalid());
    }
    let mut request = SurveyEntryCreateRequest::new(
        form,
        row.doc_id,
        row.idempotency_key,
        row.data,
        row.metadata.created_at,
        SurveyEntryCreateOrigin::Unknown,
    )
    .map_err(|_| line_invalid())?;
    if let Some(value) = row.context_key {
        request = request
            .with_context_key(value)
            .map_err(|_| line_invalid())?;
    }
    if let Some(value) = row.geometry {
        request = request.with_geometry(value).map_err(|_| line_invalid())?;
    }
    if let Some(value) = row.connectivity {
        request = request
            .with_connectivity(value)
            .map_err(|_| line_invalid())?;
    }
    if let Some(value) = row.detailed_location {
        request = request
            .with_detailed_location(value)
            .map_err(|_| line_invalid())?;
    }
    let context = request.context_key().unwrap_or("");
    Ok(ParsedRow {
        record_sha256: digest_bytes(raw),
        doc_id_sha256: digest_text(request.doc_id()),
        key_sha256: digest_text(request.idempotency_key()),
        identity_sha256: digest_text(&format!("{context}\0{}", request.doc_id())),
        request,
    })
}

fn read_bounded_line<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, Failure> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(|_| source_invalid())?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some(line))
            };
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if line.len().saturating_add(take) > LINE_MAX_BYTES {
            return Err(source_invalid());
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if line.last() == Some(&b'\n') {
            return Ok(Some(line));
        }
    }
}

fn strip_line_ending(raw: &[u8]) -> &[u8] {
    let without_lf = raw.strip_suffix(b"\n").unwrap_or(raw);
    without_lf.strip_suffix(b"\r").unwrap_or(without_lf)
}

fn resolve_paths(source: &str, checkpoint: &str, receipt: &str) -> Result<Paths, Failure> {
    let source = resolve_path(source)?;
    let checkpoint = resolve_path(checkpoint)?;
    let receipt = resolve_path(receipt)?;
    let receipt_lock = state_lock_path(&receipt, "receipt")?;
    let checkpoint_lock = state_lock_path(&checkpoint, "checkpoint")?;
    if paths_alias_lexically(&source, &checkpoint)
        || paths_alias_lexically(&source, &receipt)
        || paths_alias_lexically(&source, &receipt_lock)
        || paths_alias_lexically(&source, &checkpoint_lock)
        || paths_alias_lexically(&checkpoint, &receipt)
        || paths_alias_lexically(&checkpoint, &receipt_lock)
        || paths_alias_lexically(&checkpoint, &checkpoint_lock)
        || paths_alias_lexically(&receipt, &receipt_lock)
        || paths_alias_lexically(&receipt, &checkpoint_lock)
        || paths_alias_lexically(&receipt_lock, &checkpoint_lock)
    {
        return Err(path_conflict());
    }
    Ok(Paths {
        source,
        checkpoint,
        receipt,
        receipt_lock,
        checkpoint_lock,
    })
}

fn state_lock_path(state_path: &Path, kind: &str) -> Result<PathBuf, Failure> {
    let canonical = std::fs::canonicalize(state_path).unwrap_or_else(|_| state_path.to_path_buf());
    let path = canonical.to_string_lossy();
    #[cfg(windows)]
    let path = path.to_ascii_lowercase();
    let binding = digest_text(path.as_ref());
    Ok(state_path.with_file_name(format!(".ds-survey-import-{kind}-{binding}.lock")))
}

fn paths_alias_lexically(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn acquire_import_locks(paths: &Paths) -> Result<ImportLocks, Failure> {
    let mut lock_paths = vec![
        paths.receipt_lock.as_path(),
        paths.checkpoint_lock.as_path(),
    ];
    lock_paths.sort_unstable();
    lock_paths.dedup();
    let mut locks = Vec::with_capacity(lock_paths.len());
    for path in lock_paths {
        locks.push(acquire_import_lock(path)?);
    }
    Ok(ImportLocks { _locks: locks })
}

fn acquire_import_lock(path: &Path) -> Result<ImportLock, Failure> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let file = match open_private(path, true, 0) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_private(path).map_err(|_| state_conflict())?
            }
            Err(_) => return Err(checkpoint_unsafe()),
        };
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(ImportLock { _file: file });
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return if error.kind() == std::io::ErrorKind::WouldBlock {
                Err(state_conflict())
            } else {
                Err(checkpoint_unsafe())
            };
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .share_mode(0)
            .custom_flags(0x0020_0000);
        let file = options.open(path).map_err(|error| {
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock
            ) {
                state_conflict()
            } else {
                checkpoint_unsafe()
            }
        })?;
        let metadata = file.metadata().map_err(|_| checkpoint_unsafe())?;
        if !safe_regular(&metadata, 0, true) || !windows_opened_link_count_is_one(&file) {
            return Err(checkpoint_unsafe());
        }
        return Ok(ImportLock { _file: file });
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(checkpoint_unsafe())
    }
}

fn resolve_path(raw: &str) -> Result<PathBuf, Failure> {
    let supplied = Path::new(raw);
    let leaf = supplied
        .file_name()
        .filter(|value| !value.is_empty())
        .ok_or_else(path_conflict)?;
    #[cfg(windows)]
    if windows_leaf_is_ads_like(&leaf.to_string_lossy()) {
        return Err(path_conflict());
    }
    let parent = supplied
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let supplied_parent = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| path_conflict())?
            .join(parent)
    };
    for ancestor in supplied_parent.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor).map_err(|_| path_conflict())?;
        if metadata.file_type().is_symlink() || is_reparse(&metadata) {
            return Err(path_conflict());
        }
    }
    let parent = std::fs::canonicalize(parent).map_err(|_| path_conflict())?;
    let metadata = std::fs::symlink_metadata(&parent).map_err(|_| path_conflict())?;
    if !safe_parent(&metadata) {
        return Err(path_conflict());
    }
    Ok(parent.join(leaf))
}

fn validate_existing_path_identities(paths: &Paths) -> Result<(), Failure> {
    let mut identities = Vec::new();
    for path in [
        &paths.source,
        &paths.checkpoint,
        &paths.receipt,
        &paths.receipt_lock,
        &paths.checkpoint_lock,
    ] {
        if let Some(identity) = existing_opened_identity(path)? {
            if identities.contains(&identity) {
                return Err(path_conflict());
            }
            identities.push(identity);
        }
    }
    Ok(())
}

fn existing_opened_identity(path: &Path) -> Result<Option<FileIdentity>, Failure> {
    let observed = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(path_conflict()),
    };
    if observed.file_type().is_symlink() || !observed.is_file() || is_reparse(&observed) {
        return Err(path_conflict());
    }
    let file = open_read_no_follow(path).map_err(|_| path_conflict())?;
    let opened = file.metadata().map_err(|_| path_conflict())?;
    if !opened.is_file() || is_reparse(&opened) {
        return Err(path_conflict());
    }
    let opened_identity = opened_file_identity(&file).map_err(|_| path_conflict())?;
    #[cfg(unix)]
    if file_identity(&observed) != Some(opened_identity) {
        return Err(path_conflict());
    }
    Ok(Some(opened_identity))
}

#[cfg(unix)]
fn file_identity(metadata: &Metadata) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    Some(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(unix)]
fn opened_file_identity(file: &File) -> std::io::Result<FileIdentity> {
    file.metadata()
        .ok()
        .and_then(|metadata| file_identity(&metadata))
        .ok_or_else(unsafe_file)
}

#[cfg(windows)]
fn opened_file_identity(file: &File) -> std::io::Result<FileIdentity> {
    let information = windows_file_information(file)?;
    Ok(FileIdentity {
        volume: information.volume_serial_number,
        index: (u64::from(information.file_index_high) << 32)
            | u64::from(information.file_index_low),
    })
}

#[cfg(not(any(unix, windows)))]
fn opened_file_identity(_file: &File) -> std::io::Result<FileIdentity> {
    Err(unsafe_file())
}

#[cfg(any(windows, test))]
fn windows_leaf_is_ads_like(leaf: &str) -> bool {
    leaf.contains(':')
}

#[cfg(any(windows, test))]
fn windows_file_identity_alias(
    left: (Option<u32>, Option<u64>),
    right: (Option<u32>, Option<u64>),
) -> bool {
    matches!((left, right), ((Some(lv), Some(li)), (Some(rv), Some(ri))) if lv == rv && li == ri)
}

#[cfg(windows)]
#[repr(C)]
struct WindowsFileTime {
    low: u32,
    high: u32,
}

#[cfg(windows)]
#[repr(C)]
struct WindowsFileInformation {
    file_attributes: u32,
    creation_time: WindowsFileTime,
    last_access_time: WindowsFileTime,
    last_write_time: WindowsFileTime,
    volume_serial_number: u32,
    file_size_high: u32,
    file_size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[cfg(windows)]
fn windows_file_information(file: &File) -> std::io::Result<WindowsFileInformation> {
    use std::ffi::c_void;
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(
            file: *mut c_void,
            information: *mut WindowsFileInformation,
        ) -> i32;
    }
    let mut information = MaybeUninit::<WindowsFileInformation>::uninit();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { information.assume_init() })
}

#[cfg(windows)]
fn windows_opened_link_count_is_one(file: &File) -> bool {
    windows_file_information(file).is_ok_and(|information| information.number_of_links == 1)
}

fn open_source(path: &Path) -> Result<File, Failure> {
    let observed = std::fs::symlink_metadata(path).map_err(|_| source_invalid())?;
    if !safe_regular(&observed, SOURCE_MAX_BYTES, false) {
        return Err(source_invalid());
    }
    let file = open_read_no_follow(path).map_err(|_| source_invalid())?;
    let opened = file.metadata().map_err(|_| source_invalid())?;
    if !safe_regular(&opened, SOURCE_MAX_BYTES, false) {
        return Err(source_invalid());
    }
    #[cfg(unix)]
    if !same_unix_file(&observed, &opened) {
        return Err(source_invalid());
    }
    Ok(file)
}

fn load_local_state(
    paths: &Paths,
    plan: &ImportPlan,
    form: &str,
    lane: &str,
) -> Result<LocalState, Failure> {
    let checkpoint_exists = path_exists_safe(&paths.checkpoint, CHECKPOINT_MAX_BYTES)
        .map_err(|_| checkpoint_unsafe())?;
    let receipt_exists =
        path_exists_safe(&paths.receipt, RECEIPT_MAX_BYTES).map_err(|_| receipt_unsafe())?;
    match (checkpoint_exists, receipt_exists) {
        (false, false) => Ok(LocalState::Fresh),
        (false, true) => {
            let observed = observe_receipt(&paths.receipt)?;
            validate_manifest_pre_auth(&observed.state.manifest, plan, form, lane, paths)?;
            if observed.has_partial_tail || !observed.state.events.is_empty() {
                return Err(state_incomplete());
            }
            Ok(LocalState::ReceiptOnly(observed))
        }
        (true, false) => Err(state_incomplete()),
        (true, true) => {
            let checkpoint = read_checkpoint(&paths.checkpoint)?;
            let receipt = observe_receipt(&paths.receipt)?;
            validate_checkpoint_pre_auth(&checkpoint, plan, form, lane, paths)?;
            validate_manifest_pre_auth(&receipt.state.manifest, plan, form, lane, paths)?;
            validate_receipt_events_pre_auth(&receipt.state, plan, form)?;
            Ok(LocalState::Resume(Box::new(checkpoint), receipt))
        }
    }
}

fn path_exists_safe(path: &Path, max: u64) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !safe_regular(&metadata, max, true) {
                return Err(unsafe_file());
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn read_checkpoint(path: &Path) -> Result<Checkpoint, Failure> {
    let bytes = read_private(path, CHECKPOINT_MAX_BYTES).map_err(|_| checkpoint_unsafe())?;
    serde_json::from_slice(&bytes).map_err(|_| checkpoint_unsafe())
}

fn observe_receipt(path: &Path) -> Result<ReceiptObserved, Failure> {
    let bytes = read_private(path, RECEIPT_MAX_BYTES).map_err(|_| receipt_unsafe())?;
    if bytes.is_empty() {
        return Err(receipt_unsafe());
    }
    let complete_len = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .ok_or_else(receipt_unsafe)?;
    let has_partial_tail = complete_len != bytes.len();
    let mut lines = bytes[..complete_len]
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty());
    let manifest: ReceiptLine = serde_json::from_slice(lines.next().ok_or_else(receipt_unsafe)?)
        .map_err(|_| receipt_unsafe())?;
    if !matches!(manifest, ReceiptLine::Manifest { .. }) {
        return Err(receipt_unsafe());
    }
    let mut events = Vec::new();
    for bytes in lines {
        if events.len() >= ITEM_MAX {
            return Err(receipt_unsafe());
        }
        let event: ReceiptLine = serde_json::from_slice(bytes).map_err(|_| receipt_unsafe())?;
        if event.line() != Some(events.len() + 1) {
            return Err(receipt_unsafe());
        }
        events.push(event);
    }
    Ok(ReceiptObserved {
        state: ReceiptState { manifest, events },
        complete_len: complete_len as u64,
        full_sha256: digest_bytes(&bytes),
        has_partial_tail,
    })
}

fn verify_receipt_unchanged(path: &Path, observed: &ReceiptObserved) -> Result<(), Failure> {
    let bytes = read_private(path, RECEIPT_MAX_BYTES).map_err(|_| receipt_unsafe())?;
    if digest_bytes(&bytes) != observed.full_sha256 {
        return Err(receipt_mismatch());
    }
    Ok(())
}

fn truncate_bound_receipt_tail(path: &Path, complete_len: u64) -> Result<(), Failure> {
    let file = open_private(path, true, RECEIPT_MAX_BYTES).map_err(|_| receipt_unsafe())?;
    file.set_len(complete_len)
        .and_then(|_| file.sync_all())
        .map_err(|_| receipt_unsafe())
}

fn create_receipt(path: &Path, manifest: &ReceiptLine) -> Result<(), Failure> {
    let mut file = create_private(path).map_err(|_| receipt_unsafe())?;
    let mut bytes = serde_json::to_vec(manifest).map_err(|_| receipt_unsafe())?;
    bytes.push(b'\n');
    if bytes.len() as u64 > RECEIPT_MAX_BYTES {
        return Err(receipt_unsafe());
    }
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| receipt_unsafe())
}

fn append_receipt(path: &Path, event: &ReceiptLine) -> Result<(), Failure> {
    let mut file = open_append_private(path).map_err(|_| receipt_unsafe())?;
    let mut bytes = serde_json::to_vec(event).map_err(|_| receipt_unsafe())?;
    bytes.push(b'\n');
    let current = file.metadata().map_err(|_| receipt_unsafe())?.len();
    if current
        .checked_add(bytes.len() as u64)
        .is_none_or(|next| next > RECEIPT_MAX_BYTES)
    {
        return Err(receipt_unsafe());
    }
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| receipt_unsafe())
}

fn write_checkpoint(path: &Path, checkpoint: &Checkpoint) -> Result<(), Failure> {
    let bytes = serde_json::to_vec(checkpoint).map_err(|_| checkpoint_unsafe())?;
    if bytes.len() as u64 > CHECKPOINT_MAX_BYTES {
        return Err(checkpoint_unsafe());
    }
    let stage = stage_path(path);
    let mut file = create_private(&stage).map_err(|_| checkpoint_unsafe())?;
    let result = file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| atomic_replace(&stage, path))
        .and_then(|_| {
            sync_parent(path.parent().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent")
            })?)
        });
    if result.is_err() {
        let _ = std::fs::remove_file(&stage);
    }
    result.map_err(|_| checkpoint_unsafe())
}

fn stage_path(path: &Path) -> PathBuf {
    let leaf = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("checkpoint");
    path.with_file_name(format!(
        ".{leaf}.{}.{}.stage",
        std::process::id(),
        STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn checkpoint_for(
    plan: &ImportPlan,
    form: &str,
    session: &ds_cli_auth::HeadlessSurveyImportSession,
    checkpoint_path_sha256: &str,
    receipt_path_sha256: &str,
) -> Checkpoint {
    Checkpoint {
        schema: CHECKPOINT_SCHEMA.to_owned(),
        source_sha256: plan.source_sha256.clone(),
        source_bytes: plan.source_bytes,
        total: plan.records.len(),
        lane: session.lane().to_owned(),
        credential_audience_sha256: session.credential_audience_sha256().to_owned(),
        principal_sha256: session.principal_sha256().to_owned(),
        project_id: session.project_id().to_owned(),
        form: form.to_owned(),
        checkpoint_path_sha256: checkpoint_path_sha256.to_owned(),
        receipt_path_sha256: receipt_path_sha256.to_owned(),
        next_line: 1,
        committed: 0,
        refused: 0,
    }
}

fn manifest_for(
    plan: &ImportPlan,
    form: &str,
    session: &ds_cli_auth::HeadlessSurveyImportSession,
    checkpoint_path_sha256: &str,
    receipt_path_sha256: &str,
) -> ReceiptLine {
    ReceiptLine::Manifest {
        schema: RECEIPT_SCHEMA.to_owned(),
        source_sha256: plan.source_sha256.clone(),
        source_bytes: plan.source_bytes,
        total: plan.records.len(),
        lane: session.lane().to_owned(),
        credential_audience_sha256: session.credential_audience_sha256().to_owned(),
        principal_sha256: session.principal_sha256().to_owned(),
        project_id: session.project_id().to_owned(),
        form: form.to_owned(),
        checkpoint_path_sha256: checkpoint_path_sha256.to_owned(),
        receipt_path_sha256: receipt_path_sha256.to_owned(),
    }
}

fn validate_checkpoint_pre_auth(
    checkpoint: &Checkpoint,
    plan: &ImportPlan,
    form: &str,
    lane: &str,
    paths: &Paths,
) -> Result<(), Failure> {
    if checkpoint.schema != CHECKPOINT_SCHEMA
        || checkpoint.source_sha256 != plan.source_sha256
        || checkpoint.source_bytes != plan.source_bytes
        || checkpoint.total != plan.records.len()
        || checkpoint.form != form
        || checkpoint.lane != lane
        || checkpoint.checkpoint_path_sha256 != digest_text(&paths.checkpoint.to_string_lossy())
        || checkpoint.receipt_path_sha256 != digest_text(&paths.receipt.to_string_lossy())
        || checkpoint.next_line == 0
        || checkpoint.next_line > checkpoint.total + 1
        || checkpoint.committed + checkpoint.refused != checkpoint.next_line - 1
    {
        return Err(checkpoint_mismatch());
    }
    Ok(())
}

fn validate_checkpoint(actual: &Checkpoint, expected: &Checkpoint) -> Result<(), Failure> {
    let mut normalized = actual.clone();
    normalized.next_line = 1;
    normalized.committed = 0;
    normalized.refused = 0;
    if &normalized != expected {
        return Err(checkpoint_mismatch());
    }
    Ok(())
}

fn validate_manifest_pre_auth(
    manifest: &ReceiptLine,
    plan: &ImportPlan,
    form: &str,
    lane: &str,
    paths: &Paths,
) -> Result<(), Failure> {
    match manifest {
        ReceiptLine::Manifest {
            schema,
            source_sha256,
            source_bytes,
            total,
            lane: saved_lane,
            form: saved_form,
            checkpoint_path_sha256,
            receipt_path_sha256,
            ..
        } if schema == RECEIPT_SCHEMA
            && source_sha256 == &plan.source_sha256
            && *source_bytes == plan.source_bytes
            && *total == plan.records.len()
            && saved_lane == lane
            && saved_form == form
            && checkpoint_path_sha256 == &digest_text(&paths.checkpoint.to_string_lossy())
            && receipt_path_sha256 == &digest_text(&paths.receipt.to_string_lossy()) =>
        {
            Ok(())
        }
        _ => Err(receipt_mismatch()),
    }
}

fn validate_receipt_events_pre_auth(
    receipt: &ReceiptState,
    plan: &ImportPlan,
    form: &str,
) -> Result<(), Failure> {
    if receipt.events.len() > plan.records.len() {
        return Err(receipt_mismatch());
    }
    for (index, event) in receipt.events.iter().enumerate() {
        let expected = &plan.records[index];
        if event.line() != Some(index + 1)
            || event.record_sha256() != Some(expected.record_sha256.as_str())
            || event.doc_id_sha256() != Some(expected.doc_id_sha256.as_str())
        {
            return Err(receipt_mismatch());
        }
        match event {
            ReceiptLine::Committed {
                form: saved_form,
                client_version,
                firestore,
                bigquery_mirror,
                replication_clock,
                replication_authority,
                ..
            } if saved_form == form
                && *client_version == 0
                && firestore == "committed"
                && bigquery_mirror == "unconfirmed"
                && replication_clock == "metadata.firestore_updated_at"
                && replication_authority == "firestore_server_timestamp" => {}
            ReceiptLine::Refused {
                form: saved_form,
                code,
                ..
            } if saved_form == form && permanent_row_refusal(code) => {}
            _ => return Err(receipt_mismatch()),
        }
    }
    Ok(())
}

fn validate_manifest(manifest: &ReceiptLine, checkpoint: &Checkpoint) -> Result<(), Failure> {
    let expected = ReceiptLine::Manifest {
        schema: RECEIPT_SCHEMA.to_owned(),
        source_sha256: checkpoint.source_sha256.clone(),
        source_bytes: checkpoint.source_bytes,
        total: checkpoint.total,
        lane: checkpoint.lane.clone(),
        credential_audience_sha256: checkpoint.credential_audience_sha256.clone(),
        principal_sha256: checkpoint.principal_sha256.clone(),
        project_id: checkpoint.project_id.clone(),
        form: checkpoint.form.clone(),
        checkpoint_path_sha256: checkpoint.checkpoint_path_sha256.clone(),
        receipt_path_sha256: checkpoint.receipt_path_sha256.clone(),
    };
    if manifest != &expected {
        return Err(receipt_mismatch());
    }
    Ok(())
}

fn reconcile_checkpoint(
    path: &Path,
    checkpoint: &mut Checkpoint,
    receipt: &ReceiptState,
) -> Result<(), Failure> {
    let terminal = checkpoint.next_line - 1;
    if receipt.events.len() == terminal {
        return Ok(());
    }
    if receipt.events.len() != terminal + 1 {
        return Err(receipt_mismatch());
    }
    let event = receipt.events.last().ok_or_else(receipt_mismatch)?;
    if event.is_committed() {
        checkpoint.committed += 1;
    } else {
        checkpoint.refused += 1;
    }
    checkpoint.next_line += 1;
    write_checkpoint(path, checkpoint)
}

fn validate_event(
    event: &ReceiptLine,
    line: usize,
    parsed: &ParsedRow,
    form: &str,
) -> Result<(), Failure> {
    if event.line() != Some(line)
        || event.record_sha256() != Some(parsed.record_sha256.as_str())
        || event.doc_id_sha256() != Some(parsed.doc_id_sha256.as_str())
    {
        return Err(receipt_mismatch());
    }
    match event {
        ReceiptLine::Committed { form: saved, .. } | ReceiptLine::Refused { form: saved, .. }
            if saved == form =>
        {
            Ok(())
        }
        _ => Err(receipt_mismatch()),
    }
}

fn committed_event(
    line: usize,
    parsed: &ParsedRow,
    form: &str,
    receipt: &SurveyEntryCreateReceipt,
) -> ReceiptLine {
    ReceiptLine::Committed {
        line,
        record_sha256: parsed.record_sha256.clone(),
        form: form.to_owned(),
        doc_id_sha256: parsed.doc_id_sha256.clone(),
        client_version: receipt.client_version(),
        firestore: "committed".to_owned(),
        bigquery_mirror: "unconfirmed".to_owned(),
        replication_clock: receipt.replication_clock().to_owned(),
        replication_authority: receipt.replication_authority().to_owned(),
    }
}

fn permanent_row_refusal(code: &str) -> bool {
    matches!(
        code,
        "survey_entry_create_invalid"
            | "survey_entry_create_idempotency_conflict"
            | "survey_entry_create_already_exists"
    )
}

fn add_bounded_bytes(total: &mut u64, added: usize, limit: u64) -> bool {
    let Some(next) = total.checked_add(added as u64) else {
        return false;
    };
    if next > limit {
        return false;
    }
    *total = next;
    true
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn digest_text(value: &str) -> String {
    digest_bytes(value.as_bytes())
}

fn read_private(path: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    let file = open_private(path, false, max)?;
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "oversized",
        ));
    }
    Ok(bytes)
}

fn open_private(path: &Path, write: bool, max: u64) -> std::io::Result<File> {
    let observed = std::fs::symlink_metadata(path)?;
    if !safe_regular(&observed, max, true) {
        return Err(unsafe_file());
    }
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    configure_no_follow(&mut options);
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !safe_regular(&opened, max, true) {
        return Err(unsafe_file());
    }
    #[cfg(unix)]
    if !same_unix_file(&observed, &opened) {
        return Err(unsafe_file());
    }
    #[cfg(windows)]
    if !windows_opened_link_count_is_one(&file) {
        return Err(unsafe_file());
    }
    Ok(file)
}

fn open_append_private(path: &Path) -> std::io::Result<File> {
    let observed = std::fs::symlink_metadata(path)?;
    if !safe_regular(&observed, RECEIPT_MAX_BYTES, true) {
        return Err(unsafe_file());
    }
    let mut options = OpenOptions::new();
    options.append(true);
    configure_no_follow(&mut options);
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !safe_regular(&opened, RECEIPT_MAX_BYTES, true) {
        return Err(unsafe_file());
    }
    #[cfg(unix)]
    if !same_unix_file(&observed, &opened) {
        return Err(unsafe_file());
    }
    #[cfg(windows)]
    if !windows_opened_link_count_is_one(&file) {
        return Err(unsafe_file());
    }
    Ok(file)
}

fn create_private(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    options.open(path)
}

fn configure_no_follow(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
}

fn safe_regular(metadata: &Metadata, max: u64, private: bool) -> bool {
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > max
        || is_reparse(metadata)
    {
        return false;
    }
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return false;
        }
    }
    true
}

fn safe_parent(metadata: &Metadata) -> bool {
    metadata.is_dir() && !metadata.file_type().is_symlink() && !is_reparse(metadata)
}

#[cfg(unix)]
fn same_unix_file(a: &Metadata, b: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(windows)]
fn is_reparse(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    has_windows_reparse_attribute(metadata.file_attributes())
}
#[cfg(not(windows))]
fn is_reparse(_metadata: &Metadata) -> bool {
    false
}

#[cfg(any(windows, test))]
fn has_windows_reparse_attribute(attributes: u32) -> bool {
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(unix)]
fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}
#[cfg(windows)]
fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(0x0020_0000)
        .open(path)
}
#[cfg(not(any(unix, windows)))]
fn open_read_no_follow(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> std::io::Result<()> {
    File::open(parent)?.sync_all()
}
#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> std::io::Result<()> {
    Ok(())
}

fn unsafe_file() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, "unsafe private file")
}
fn source_invalid() -> Failure {
    Failure::invalid(
        "survey_entries_import_source_invalid",
        "the import source is not one bounded regular non-link NDJSON file",
    )
    .remedy("pass one unchanged regular non-link NDJSON file within the documented bounds")
}
fn line_invalid() -> Failure {
    Failure::invalid(
        "survey_entries_import_line_invalid",
        "one import row violates the closed canonical create grammar",
    )
    .remedy("read this command's help and reshape every row without authority-owned metadata")
}
fn duplicate_identity() -> Failure {
    Failure::invalid(
        "survey_entries_import_duplicate_identity",
        "the source repeats an idempotency key or canonical entry identity",
    )
    .remedy("give every logical row one unique stable document identity and opaque replay key")
}
fn source_changed() -> Failure {
    Failure::conflict(
        "survey_entries_import_source_changed",
        "the import source changed after its pre-auth validation",
    )
    .remedy("restore the exact checkpoint-bound source bytes and resume without editing in place")
}
fn path_conflict() -> Failure {
    Failure::invalid(
        "survey_entries_import_path_conflict",
        "source, checkpoint, and receipt must be distinct ordinary paths under safe parents",
    )
    .remedy("use three distinct ordinary paths under non-link directories")
}
fn checkpoint_unsafe() -> Failure {
    Failure::unavailable(
        "survey_entries_import_checkpoint_unsafe",
        "the private import checkpoint cannot be read or replaced safely",
    )
    .remedy("preserve its evidence, repair the exact path and permissions, then resume")
}
fn checkpoint_mismatch() -> Failure {
    Failure::conflict(
        "survey_entries_import_checkpoint_mismatch",
        "the checkpoint is bound to another import authority or source",
    )
    .remedy("restore the exact original inputs and selected project, or use fresh state paths")
}
fn receipt_unsafe() -> Failure {
    Failure::unavailable(
        "survey_entries_import_receipt_unsafe",
        "the private import receipt cannot be read, repaired, appended, or synced safely",
    )
    .remedy("preserve its evidence and repair the exact receipt before resuming")
}
fn receipt_mismatch() -> Failure {
    Failure::conflict(
        "survey_entries_import_receipt_mismatch",
        "receipt evidence disagrees with the checkpoint or canonical source",
    )
    .remedy("restore the matching checkpoint, receipt, and source trio")
}
fn state_incomplete() -> Failure {
    Failure::conflict(
        "survey_entries_import_state_incomplete",
        "the checkpoint and receipt state pair is incomplete",
    )
    .remedy("restore the matching pair; only a header-only receipt is automatically recoverable")
}
fn state_conflict() -> Failure {
    Failure::conflict(
        "survey_entries_import_state_conflict",
        "another process owns this exact import checkpoint",
    )
    .remedy("let that import finish, then resume the same checkpoint and receipt")
}

pub fn render(data: &Value) -> String {
    format!(
        "{}: {}/{} committed, {} refused; BigQuery mirror NOT YET CONFIRMED\n",
        data["status"].as_str().unwrap_or("import"),
        data["committed"].as_u64().unwrap_or(0),
        data["total"].as_u64().unwrap_or(0),
        data["permanently_refused"].as_u64().unwrap_or(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::args::parse as parse_args;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ds-survey-import-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }
    fn row(id: &str, key: &str) -> String {
        format!(
            r#"{{"doc_id":"{id}","idempotency_key":"{key}","data":{{"private":"value"}},"metadata":{{"created_at":"2026-08-30T12:00:00Z"}},"geometry":{{"type":"Point","coordinates":[30,-2]}},"connectivity":{{}},"detailed_location":{{}}}}"#
        )
    }

    #[test]
    fn canonical_rows_validate_and_redacted_plan_retains_no_payload_or_key() {
        let dir = temp_dir();
        let source = dir.join("rows.ndjson");
        std::fs::write(
            &source,
            format!(
                "{}\n{}\n",
                row("one", "secret-key-one"),
                row("two", "secret-key-two")
            ),
        )
        .unwrap();
        let plan = validate_source(&source, "survey_points").unwrap();
        assert_eq!(plan.records.len(), 2);
        let debug = plan.source_sha256.to_string();
        assert!(!debug.contains("private"));
        assert!(!debug.contains("secret-key"));
        assert!(!debug.contains("30"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_or_authority_owned_metadata_is_refused() {
        for text in [
            r#"{"doc_id":"one","idempotency_key":"key","data":{},"metadata":{"created_at":"2026-08-30T12:00:00Z","created_by":"spoof@example.com"}}"#,
            r#"{"doc_id":"one","idempotency_key":"key","data":{},"metadata":{"created_at":"2026-08-30T12:00:00Z"},"project_id":"other"}"#,
            r#"{"doc_id":"one","idempotency_key":"key","data":{},"metadata":{"created_at":"2026-08-30T12:00:00Z"},"source_provenance":{}}"#,
            r#"{"doc_id":"one","idempotency_key":"key","data":{},"metadata":{"created_at":"2026-08-30T12:00:00Z"},"geometry":null}"#,
        ] {
            assert_eq!(
                parse_row("survey_points", text.as_bytes())
                    .err()
                    .unwrap()
                    .code(),
                "survey_entries_import_line_invalid"
            );
        }
    }

    #[test]
    fn duplicate_keys_and_identities_are_refused_before_auth() {
        let dir = temp_dir();
        let source = dir.join("rows.ndjson");
        std::fs::write(
            &source,
            format!("{}\n{}\n", row("one", "same"), row("two", "same")),
        )
        .unwrap();
        assert_eq!(
            validate_source(&source, "survey_points")
                .err()
                .unwrap()
                .code(),
            "survey_entries_import_duplicate_identity"
        );
        std::fs::write(
            &source,
            format!("{}\n{}\n", row("one", "a"), row("one", "b")),
        )
        .unwrap();
        assert_eq!(
            validate_source(&source, "survey_points")
                .err()
                .unwrap()
                .code(),
            "survey_entries_import_duplicate_identity"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn final_source_recheck_detects_replacement_and_mutation() {
        let dir = temp_dir();
        let source = dir.join("source.ndjson");
        let bytes = format!("{}\n", row("one", "key"));
        std::fs::write(&source, &bytes).unwrap();
        let initial = validate_source(&source, "survey_points").unwrap();
        let moved = dir.join("moved.ndjson");
        std::fs::rename(&source, &moved).unwrap();
        std::fs::write(&source, &bytes).unwrap();
        let replacement = validate_source(&source, "survey_points").unwrap();
        assert_eq!(initial.source_sha256, replacement.source_sha256);
        assert!(!plans_match(&initial, &replacement));
        std::fs::write(&source, format!("{}\n", row("two", "key-two"))).unwrap();
        let mutated = validate_source(&source, "survey_points").unwrap();
        assert!(!plans_match(&replacement, &mutated));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn descriptor_exposes_no_concurrency_or_authority_escape() {
        let names = COMMAND
            .args
            .iter()
            .map(|arg| arg.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from(["checkpoint", "file", "form", "lane", "on-error", "receipt"])
        );
        for forbidden in [
            "project",
            "concurrency",
            "retry",
            "origin",
            "operation",
            "created-by",
            "token",
            "url",
            "method",
            "source-provenance",
        ] {
            assert!(COMMAND.arg(forbidden).is_none());
        }
        assert_eq!(COMMAND.effect, Effect::GlobalWrite);
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert!(COMMAND.effect.needs_confirmation());
    }

    #[test]
    fn windows_reparse_attribute_check_is_exact() {
        assert!(has_windows_reparse_attribute(0x0000_0400));
        assert!(has_windows_reparse_attribute(0x0000_0420));
        assert!(!has_windows_reparse_attribute(0));
        assert!(!has_windows_reparse_attribute(0x0000_0020));
        assert!(windows_leaf_is_ads_like("receipt.ndjson:stream"));
        assert!(!windows_leaf_is_ads_like("receipt.ndjson"));
        assert!(windows_file_identity_alias(
            (Some(7), Some(11)),
            (Some(7), Some(11))
        ));
        assert!(!windows_file_identity_alias(
            (Some(7), Some(11)),
            (Some(7), Some(12))
        ));
        assert!(!windows_file_identity_alias(
            (None, Some(11)),
            (None, Some(11))
        ));
        assert!(matches!(
            windows_import_availability(true),
            Availability::Unavailable {
                code: "survey_entries_import_windows_state_unavailable",
                ..
            }
        ));
        assert_eq!(windows_import_availability(false), Availability::Available);
    }

    #[test]
    fn only_exact_row_local_create_refusals_advance() {
        for code in [
            "survey_entry_create_invalid",
            "survey_entry_create_idempotency_conflict",
            "survey_entry_create_already_exists",
        ] {
            assert!(permanent_row_refusal(code));
        }
        for code in [
            "survey_entry_create_permission_denied",
            "survey_entry_create_scope_not_found",
            "survey_entry_create_form_disabled",
            "survey_entry_create_project_read_only",
            "survey_entry_create_refused",
            "survey_entry_create_failed",
        ] {
            assert!(!permanent_row_refusal(code));
        }
    }

    #[test]
    fn streamed_source_and_receipt_growth_stop_at_their_exact_bounds() {
        let mut total = 0;
        assert!(add_bounded_bytes(&mut total, 3, 4));
        assert!(!add_bounded_bytes(&mut total, 2, 4));
        assert_eq!(total, 3, "a refused growth does not change the fence");

        let dir = temp_dir();
        let receipt = dir.join("receipt.ndjson");
        let file = create_private(&receipt).unwrap();
        file.set_len(RECEIPT_MAX_BYTES).unwrap();
        drop(file);
        let event = ReceiptLine::Refused {
            line: 1,
            record_sha256: "a".repeat(64),
            form: "f".into(),
            doc_id_sha256: "b".repeat(64),
            code: "survey_entry_create_invalid".into(),
        };
        assert_eq!(
            append_receipt(&receipt, &event).err().unwrap().code(),
            "survey_entries_import_receipt_unsafe"
        );
        assert_eq!(
            std::fs::metadata(&receipt).unwrap().len(),
            RECEIPT_MAX_BYTES
        );
        let encoded = serde_json::to_string(&event).unwrap();
        assert!(!encoded.contains("doc_id\""));
        assert!(!encoded.contains("\"doc\""));
        assert!(!encoded.contains("idempotency"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn receipt_tail_repair_and_checkpoint_reconciliation_are_contiguous() {
        let dir = temp_dir();
        let receipt = dir.join("receipt.ndjson");
        let checkpoint_path = dir.join("checkpoint.json");
        let checkpoint_path_sha256 = digest_text(&checkpoint_path.to_string_lossy());
        let receipt_path_sha256 = digest_text(&receipt.to_string_lossy());
        let manifest = ReceiptLine::Manifest {
            schema: RECEIPT_SCHEMA.into(),
            source_sha256: "a".repeat(64),
            source_bytes: 10,
            total: 1,
            lane: "stable".into(),
            credential_audience_sha256: "b".repeat(64),
            principal_sha256: "c".repeat(64),
            project_id: "p".into(),
            form: "f".into(),
            checkpoint_path_sha256: checkpoint_path_sha256.clone(),
            receipt_path_sha256: receipt_path_sha256.clone(),
        };
        create_receipt(&receipt, &manifest).unwrap();
        let mut file = OpenOptions::new().append(true).open(&receipt).unwrap();
        file.write_all(b"{partial").unwrap();
        file.sync_all().unwrap();
        let before = std::fs::read(&receipt).unwrap();
        let observed = observe_receipt(&receipt).unwrap();
        assert!(observed.has_partial_tail);
        assert_eq!(std::fs::read(&receipt).unwrap(), before);
        let complete_len = observed.complete_len;
        let mut state = observed.state;
        assert!(state.events.is_empty());

        state.events.push(ReceiptLine::Refused {
            line: 1,
            record_sha256: "d".repeat(64),
            form: "f".into(),
            doc_id_sha256: digest_text("doc"),
            code: "survey_entry_create_invalid".into(),
        });
        let plan = ImportPlan {
            source_sha256: "a".repeat(64),
            source_bytes: 10,
            source_identity: opened_file_identity(&File::open(&receipt).unwrap()).unwrap(),
            records: vec![RecordPlan {
                record_sha256: "d".repeat(64),
                doc_id_sha256: digest_text("doc"),
            }],
        };
        validate_receipt_events_pre_auth(&state, &plan, "f").unwrap();
        let mut checkpoint = Checkpoint {
            schema: CHECKPOINT_SCHEMA.into(),
            source_sha256: "a".repeat(64),
            source_bytes: 10,
            total: 1,
            lane: "stable".into(),
            credential_audience_sha256: "b".repeat(64),
            principal_sha256: "c".repeat(64),
            project_id: "p".into(),
            form: "f".into(),
            checkpoint_path_sha256,
            receipt_path_sha256,
            next_line: 1,
            committed: 0,
            refused: 0,
        };
        let original_paths = resolve_paths(
            dir.join("source.ndjson").to_str().unwrap(),
            checkpoint_path.to_str().unwrap(),
            receipt.to_str().unwrap(),
        )
        .unwrap();
        validate_checkpoint_pre_auth(&checkpoint, &plan, "f", "stable", &original_paths).unwrap();
        validate_manifest_pre_auth(&manifest, &plan, "f", "stable", &original_paths).unwrap();
        let copied_paths = resolve_paths(
            dir.join("source.ndjson").to_str().unwrap(),
            dir.join("copied-checkpoint.json").to_str().unwrap(),
            receipt.to_str().unwrap(),
        )
        .unwrap();
        assert!(
            validate_checkpoint_pre_auth(&checkpoint, &plan, "f", "stable", &copied_paths).is_err()
        );
        assert!(
            validate_manifest_pre_auth(&manifest, &plan, "f", "stable", &copied_paths).is_err()
        );
        validate_manifest(&manifest, &checkpoint).unwrap();
        assert_eq!(std::fs::read(&receipt).unwrap(), before);
        let bound_observation = observe_receipt(&receipt).unwrap();
        verify_receipt_unchanged(&receipt, &bound_observation).unwrap();
        truncate_bound_receipt_tail(&receipt, complete_len).unwrap();
        reconcile_checkpoint(&checkpoint_path, &mut checkpoint, &state).unwrap();
        assert_eq!(checkpoint.next_line, 2);
        assert_eq!(checkpoint.refused, 1);
        assert_eq!(read_checkpoint(&checkpoint_path).unwrap(), checkpoint);
        if let ReceiptLine::Refused { code, .. } = &mut state.events[0] {
            *code = "unknown".into();
        }
        assert_eq!(
            validate_receipt_events_pre_auth(&state, &plan, "f")
                .err()
                .unwrap()
                .code(),
            "survey_entries_import_receipt_mismatch"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unrelated_partial_receipt_is_never_modified_before_binding() {
        let dir = temp_dir();
        let source = dir.join("source.ndjson");
        let checkpoint = dir.join("checkpoint.json");
        let receipt = dir.join("receipt.ndjson");
        std::fs::write(&source, format!("{}\n", row("one", "key"))).unwrap();
        let plan = validate_source(&source, "survey_points").unwrap();
        let paths = resolve_paths(
            source.to_str().unwrap(),
            checkpoint.to_str().unwrap(),
            receipt.to_str().unwrap(),
        )
        .unwrap();
        let manifest = ReceiptLine::Manifest {
            schema: RECEIPT_SCHEMA.into(),
            source_sha256: plan.source_sha256.clone(),
            source_bytes: plan.source_bytes,
            total: plan.records.len(),
            lane: "stable".into(),
            credential_audience_sha256: "a".repeat(64),
            principal_sha256: "b".repeat(64),
            project_id: "p".into(),
            form: "another_form".into(),
            checkpoint_path_sha256: digest_text(&checkpoint.to_string_lossy()),
            receipt_path_sha256: digest_text(&receipt.to_string_lossy()),
        };
        create_receipt(&receipt, &manifest).unwrap();
        let mut file = OpenOptions::new().append(true).open(&receipt).unwrap();
        file.write_all(b"{untrusted-partial").unwrap();
        file.sync_all().unwrap();
        drop(file);
        let before = std::fs::read(&receipt).unwrap();
        assert_eq!(
            load_local_state(&paths, &plan, "survey_points", "stable")
                .err()
                .unwrap()
                .code(),
            "survey_entries_import_receipt_mismatch"
        );
        assert_eq!(std::fs::read(&receipt).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn source_and_private_state_links_or_public_modes_are_refused() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = temp_dir();
        let source = dir.join("source");
        std::fs::write(&source, format!("{}\n", row("one", "key"))).unwrap();
        let link = dir.join("link");
        symlink(&source, &link).unwrap();
        assert_eq!(
            validate_source(&link, "survey_points")
                .err()
                .unwrap()
                .code(),
            "survey_entries_import_source_invalid"
        );
        let state = dir.join("state");
        std::fs::write(&state, b"{}").unwrap();
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_private(&state, 100).is_err());

        let real_parent = dir.join("real-parent");
        std::fs::create_dir(&real_parent).unwrap();
        let linked_parent = dir.join("linked-parent");
        symlink(&real_parent, &linked_parent).unwrap();
        assert_eq!(
            resolve_path(linked_parent.join("state").to_str().unwrap())
                .err()
                .unwrap()
                .code(),
            "survey_entries_import_path_conflict"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn one_checkpoint_has_one_process_owner() {
        let dir = temp_dir();
        let source = dir.join("source.ndjson");
        let receipt = dir.join("receipt.ndjson");
        let first_paths = resolve_paths(
            source.to_str().unwrap(),
            dir.join("one.checkpoint").to_str().unwrap(),
            receipt.to_str().unwrap(),
        )
        .unwrap();
        let second_paths = resolve_paths(
            source.to_str().unwrap(),
            dir.join("two.checkpoint").to_str().unwrap(),
            receipt.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(first_paths.receipt_lock, second_paths.receipt_lock);
        std::fs::write(&receipt, b"created-after-lock-derivation").unwrap();
        let after_create_paths = resolve_paths(
            source.to_str().unwrap(),
            dir.join("three.checkpoint").to_str().unwrap(),
            receipt.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(first_paths.receipt_lock, after_create_paths.receipt_lock);
        let first = acquire_import_lock(&first_paths.receipt_lock).unwrap();
        assert_eq!(
            acquire_import_lock(&second_paths.receipt_lock)
                .err()
                .unwrap()
                .code(),
            "survey_entries_import_state_conflict"
        );
        drop(first);
        acquire_import_lock(&second_paths.receipt_lock).unwrap();

        let third_paths = resolve_paths(
            source.to_str().unwrap(),
            dir.join("one.checkpoint").to_str().unwrap(),
            dir.join("other-receipt").to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(first_paths.checkpoint_lock, third_paths.checkpoint_lock);
        let checkpoint_owner = acquire_import_lock(&first_paths.checkpoint_lock).unwrap();
        assert_eq!(
            acquire_import_lock(&third_paths.checkpoint_lock)
                .err()
                .unwrap()
                .code(),
            "survey_entries_import_state_conflict"
        );
        drop(checkpoint_owner);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn existing_hardlink_aliases_are_refused_by_opened_identity() {
        let dir = temp_dir();
        let source = dir.join("source.ndjson");
        let receipt = dir.join("receipt.ndjson");
        std::fs::write(&source, format!("{}\n", row("one", "key"))).unwrap();
        std::fs::hard_link(&source, &receipt).unwrap();
        let paths = resolve_paths(
            source.to_str().unwrap(),
            dir.join("checkpoint").to_str().unwrap(),
            receipt.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(
            validate_existing_path_identities(&paths)
                .err()
                .unwrap()
                .code(),
            "survey_entries_import_path_conflict"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn command_parser_requires_all_three_paths() {
        let parsed = parse_args(
            &COMMAND,
            &[
                "--form".into(),
                "f".into(),
                "--file".into(),
                "a".into(),
                "--checkpoint".into(),
                "b".into(),
                "--receipt".into(),
                "c".into(),
            ],
        )
        .unwrap();
        assert_eq!(parsed.require("on-error").unwrap(), "stop");
    }
}
