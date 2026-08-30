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
    "Private atomic resume checkpoint bound to this source, principal, project, and form.",
)
.required();
const RECEIPT: Arg = Arg::value(
    "receipt",
    "<ndjson-path>",
    "Private append-and-sync redacted item receipt.",
)
.required();
const ON_ERROR: Arg = Arg::value(
    "on-error",
    "<stop|continue>",
    "Stop after the first permanent row refusal, or record it and continue.",
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
        code: "survey_entries_import_paused",
        when: "one row has an uncertain or retryable outcome and the contiguous checkpoint cannot advance",
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
    purpose: "Validates closed NDJSON twice before auth or network. Restores one session, freezes the selected project and form, then runs sequential governed creates. A redacted receipt is synced before its private checkpoint advances; resume uses the exact source, ids, and opaque keys without auto-retry. Metadata accepts only created_at. The authority owns project, creator, operation, origin, audit, and replication fields. No provenance, project override, per-row form, concurrency, fallback, direct store, Desktop, transport, identity, or generated id/key escape is exposed.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[FORM, FILE, CHECKPOINT, RECEIPT, ON_ERROR, LANE],
    output: "Bounded source, project/form, count, progress, state-path, and mirror summary. Row receipts contain identities and verified commit clocks but no payload, field names/values, coordinates, idempotency material, token, or email.",
    examples: &[Example {
        command: "ds survey entries import --form lv_poles_survey --file ./survey123.ndjson --checkpoint ./survey123.checkpoint.json --receipt ./survey123.receipt.ndjson --yes --output json",
        note: "Sequentially imports one immutable, pre-shaped canonical source into the selected project.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
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
    },
    Committed {
        line: usize,
        record_sha256: String,
        form: String,
        doc_id: String,
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
        doc_id: String,
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
    fn doc_id(&self) -> Option<&str> {
        match self {
            Self::Manifest { .. } => None,
            Self::Committed { doc_id, .. } | Self::Refused { doc_id, .. } => Some(doc_id),
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

enum LocalState {
    Fresh,
    ReceiptOnly(ReceiptState),
    Resume(Box<Checkpoint>, ReceiptState),
}

struct Paths {
    source: PathBuf,
    checkpoint: PathBuf,
    receipt: PathBuf,
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

    let first = validate_source(&paths.source, form)?;
    let second = validate_source(&paths.source, form)?;
    if first.source_sha256 != second.source_sha256
        || first.source_bytes != second.source_bytes
        || first.records.len() != second.records.len()
        || first
            .records
            .iter()
            .zip(&second.records)
            .any(|(a, b)| a.record_sha256 != b.record_sha256 || a.doc_id_sha256 != b.doc_id_sha256)
    {
        return Err(source_changed());
    }
    let plan = second;
    let local = load_local_state(&paths, &plan, form, lane)?;

    // Only after the complete caller-controlled source and local state have
    // been parsed do profile discovery, auth restoration, and project access begin.
    let mut session = ds_cli_auth::survey_import_session(lane)?;
    let receipt_path_sha256 = digest_text(&paths.receipt.to_string_lossy());
    let mut checkpoint = checkpoint_for(&plan, form, &session, &receipt_path_sha256);
    let mut receipt_state = match local {
        LocalState::Fresh => {
            let manifest = manifest_for(&plan, form, &session);
            create_receipt(&paths.receipt, &manifest)?;
            write_checkpoint(&paths.checkpoint, &checkpoint)?;
            ReceiptState {
                manifest,
                events: Vec::new(),
            }
        }
        LocalState::ReceiptOnly(state) => {
            validate_manifest(&state.manifest, &plan, form, &session)?;
            if !state.events.is_empty() {
                return Err(state_incomplete());
            }
            write_checkpoint(&paths.checkpoint, &checkpoint)?;
            state
        }
        LocalState::Resume(existing, state) => {
            validate_checkpoint(&existing, &checkpoint)?;
            validate_manifest(&state.manifest, &plan, form, &session)?;
            checkpoint = *existing;
            state
        }
    };
    reconcile_checkpoint(&paths.checkpoint, &mut checkpoint, &receipt_state)?;

    let file = open_source(&paths.source)?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut line_number = 0usize;
    while let Some(raw) = read_bounded_line(&mut reader)? {
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
                    doc_id: parsed.request.doc_id().to_owned(),
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
    if line_number != plan.records.len() || format!("{:x}", digest.finalize()) != plan.source_sha256
    {
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
    if metadata.len() == 0 || metadata.len() > SOURCE_MAX_BYTES {
        return Err(source_invalid());
    }
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut records = Vec::new();
    let mut keys = BTreeSet::new();
    let mut identities = BTreeSet::new();
    while let Some(raw) = read_bounded_line(&mut reader)? {
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
        source_bytes: metadata.len(),
        records,
    })
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
    if source == checkpoint || source == receipt || checkpoint == receipt {
        return Err(path_conflict());
    }
    Ok(Paths {
        source,
        checkpoint,
        receipt,
    })
}

fn resolve_path(raw: &str) -> Result<PathBuf, Failure> {
    let supplied = Path::new(raw);
    let leaf = supplied
        .file_name()
        .filter(|value| !value.is_empty())
        .ok_or_else(path_conflict)?;
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
            repair_receipt_tail(&paths.receipt)?;
            let state = read_receipt(&paths.receipt)?;
            validate_manifest_pre_auth(&state.manifest, plan, form, lane)?;
            Ok(LocalState::ReceiptOnly(state))
        }
        (true, false) => Err(state_incomplete()),
        (true, true) => {
            repair_receipt_tail(&paths.receipt)?;
            let checkpoint = read_checkpoint(&paths.checkpoint)?;
            let receipt = read_receipt(&paths.receipt)?;
            validate_checkpoint_pre_auth(&checkpoint, plan, form, lane, &paths.receipt)?;
            validate_manifest_pre_auth(&receipt.manifest, plan, form, lane)?;
            validate_receipt_events_pre_auth(&receipt, plan, form)?;
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

fn read_receipt(path: &Path) -> Result<ReceiptState, Failure> {
    let bytes = read_private(path, RECEIPT_MAX_BYTES).map_err(|_| receipt_unsafe())?;
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return Err(receipt_unsafe());
    }
    let mut lines = bytes
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
    Ok(ReceiptState { manifest, events })
}

fn repair_receipt_tail(path: &Path) -> Result<(), Failure> {
    let bytes = read_private(path, RECEIPT_MAX_BYTES).map_err(|_| receipt_unsafe())?;
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        return Ok(());
    }
    let Some(last) = bytes.iter().rposition(|byte| *byte == b'\n') else {
        return Err(receipt_unsafe());
    };
    let file = open_private(path, true, RECEIPT_MAX_BYTES).map_err(|_| receipt_unsafe())?;
    file.set_len((last + 1) as u64)
        .and_then(|_| file.sync_all())
        .map_err(|_| receipt_unsafe())
}

fn create_receipt(path: &Path, manifest: &ReceiptLine) -> Result<(), Failure> {
    let mut file = create_private(path).map_err(|_| receipt_unsafe())?;
    let mut bytes = serde_json::to_vec(manifest).map_err(|_| receipt_unsafe())?;
    bytes.push(b'\n');
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| receipt_unsafe())
}

fn append_receipt(path: &Path, event: &ReceiptLine) -> Result<(), Failure> {
    let mut file = open_append_private(path).map_err(|_| receipt_unsafe())?;
    let mut bytes = serde_json::to_vec(event).map_err(|_| receipt_unsafe())?;
    bytes.push(b'\n');
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
    }
}

fn validate_checkpoint_pre_auth(
    checkpoint: &Checkpoint,
    plan: &ImportPlan,
    form: &str,
    lane: &str,
    receipt: &Path,
) -> Result<(), Failure> {
    if checkpoint.schema != CHECKPOINT_SCHEMA
        || checkpoint.source_sha256 != plan.source_sha256
        || checkpoint.source_bytes != plan.source_bytes
        || checkpoint.total != plan.records.len()
        || checkpoint.form != form
        || checkpoint.lane != lane
        || checkpoint.receipt_path_sha256 != digest_text(&receipt.to_string_lossy())
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
) -> Result<(), Failure> {
    match manifest {
        ReceiptLine::Manifest {
            schema,
            source_sha256,
            source_bytes,
            total,
            lane: saved_lane,
            form: saved_form,
            ..
        } if schema == RECEIPT_SCHEMA
            && source_sha256 == &plan.source_sha256
            && *source_bytes == plan.source_bytes
            && *total == plan.records.len()
            && saved_lane == lane
            && saved_form == form =>
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
            || event.doc_id().map(digest_text).as_deref() != Some(expected.doc_id_sha256.as_str())
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

fn validate_manifest(
    manifest: &ReceiptLine,
    plan: &ImportPlan,
    form: &str,
    session: &ds_cli_auth::HeadlessSurveyImportSession,
) -> Result<(), Failure> {
    if manifest != &manifest_for(plan, form, session) {
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
        || event.doc_id() != Some(parsed.request.doc_id())
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
        doc_id: receipt.doc_id().to_owned(),
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
            | "survey_entry_create_permission_denied"
            | "survey_entry_create_scope_not_found"
            | "survey_entry_create_form_disabled"
            | "survey_entry_create_project_read_only"
            | "survey_entry_create_idempotency_conflict"
            | "survey_entry_create_already_exists"
            | "survey_entry_create_refused"
    )
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
    }

    #[test]
    fn receipt_tail_repair_and_checkpoint_reconciliation_are_contiguous() {
        let dir = temp_dir();
        let receipt = dir.join("receipt.ndjson");
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
        };
        create_receipt(&receipt, &manifest).unwrap();
        let mut file = OpenOptions::new().append(true).open(&receipt).unwrap();
        file.write_all(b"{partial").unwrap();
        file.sync_all().unwrap();
        repair_receipt_tail(&receipt).unwrap();
        let mut state = read_receipt(&receipt).unwrap();
        assert!(state.events.is_empty());

        state.events.push(ReceiptLine::Refused {
            line: 1,
            record_sha256: "d".repeat(64),
            form: "f".into(),
            doc_id: "doc".into(),
            code: "survey_entry_create_refused".into(),
        });
        let plan = ImportPlan {
            source_sha256: "a".repeat(64),
            source_bytes: 10,
            records: vec![RecordPlan {
                record_sha256: "d".repeat(64),
                doc_id_sha256: digest_text("doc"),
            }],
        };
        validate_receipt_events_pre_auth(&state, &plan, "f").unwrap();
        let checkpoint_path = dir.join("checkpoint.json");
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
            receipt_path_sha256: "e".repeat(64),
            next_line: 1,
            committed: 0,
            refused: 0,
        };
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
