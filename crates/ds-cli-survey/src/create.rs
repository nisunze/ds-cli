//! Create one governed Survey entry without a map or Desktop session.

use std::fs::{File, Metadata, OpenOptions};
use std::io::Read;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{
    SURVEY_ENTRY_CREATE_MAX_PAYLOAD_BYTES, SurveyEntryCreateOrigin, SurveyEntryCreateRequest,
};
use serde_json::{Map, Value, json};

const FORM: Arg = Arg::value("form", "<form-slug>", "Exact governed form slug.").required();
const DOC_ID: Arg = Arg::value(
    "doc-id",
    "<document-id>",
    "New Firestore document identity; no slash or reserved identity.",
)
.required();
const IDEMPOTENCY_KEY: Arg = Arg::value(
    "idempotency-key",
    "<opaque-key>",
    "Opaque replay key bound to this exact create; never returned in output.",
)
.required();
const CREATED_AT: Arg = Arg::value(
    "created-at",
    "<RFC3339>",
    "Device creation time; normalized to canonical UTC before auth.",
)
.required();
const DOCUMENT: Arg = Arg::value(
    "document",
    "<json-path>",
    "Regular non-symlink JSON file: required data object and optional non-null geometry, connectivity, and detailed_location.",
)
.required();
const CONTEXT_KEY: Arg = Arg::value(
    "context-key",
    "<ancestor-chain>",
    "Optional governed form:document ancestor chain.",
);
const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

pub(crate) const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "survey_entry_create_invalid",
        when: "the form, document id, replay key, timestamp, context, or typed JSON values violate the closed create grammar",
        remedy: "recheck the exact form, document id, timestamp, context, and JSON document",
    },
    Refusal {
        code: "survey_entry_create_document_invalid",
        when: "the document is missing, symlinked, not regular, oversized, invalid JSON, has unknown or missing keys, or uses null for an optional value",
        remedy: "pass one regular non-symlink JSON object no larger than 900 KiB with data and only the documented optional keys",
    },
    Refusal {
        code: "survey_entry_create_auth_rejected",
        when: "the native session or fixed create route rejects the verified identity",
        remedy: "sign in again and verify the selected project and form authority",
    },
    Refusal {
        code: "survey_entry_create_permission_denied",
        when: "the verified user lacks entries.create authority for this project form",
        remedy: "request entries.create authority for the selected project and form",
    },
    Refusal {
        code: "survey_entry_create_scope_not_found",
        when: "the selected project, governed form, or context ancestor is unavailable",
        remedy: "verify the selected project, form, and optional context key",
    },
    Refusal {
        code: "survey_entry_create_form_disabled",
        when: "the Survey form is not enabled for entry creation in the selected project",
        remedy: "enable the project form before creating entries",
    },
    Refusal {
        code: "survey_entry_create_project_read_only",
        when: "the selected project lifecycle does not permit Survey entry creation",
        remedy: "select an active writable project",
    },
    Refusal {
        code: "survey_entry_create_idempotency_conflict",
        when: "the opaque replay key is already bound to a different mutation",
        remedy: "use the original exact request for replay, or a fresh key for a distinct create",
    },
    Refusal {
        code: "survey_entry_create_already_exists",
        when: "the Survey document exists and this request is not its exact replay",
        remedy: "choose a new document id, or replay the original exact request and key",
    },
    Refusal {
        code: "survey_entry_create_failed",
        when: "the governed Survey create service fails temporarily",
        remedy: "after service recovery, retry the exact document with the same idempotency key",
    },
    Refusal {
        code: "survey_entry_create_refused",
        when: "the backend coarsely refuses an already validated create without a recognized service code",
        remedy: "recheck the form, document identity, context, and document bounds",
    },
    Refusal {
        code: "survey_entry_create_unreadable",
        when: "the response violates its closed identity, version, clock, or authority contract",
        remedy: "verify the backend release and update ds before retrying",
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
        when: "the user has no audience-fenced project selection",
        remedy: "run ds auth project use --project <exact-id>",
    },
    Refusal {
        code: "project_context_stale",
        when: "the saved project belongs to another identity, lane, or audience",
        remedy: "select the project again with ds auth project use",
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
        when: "another native operation holds the state lease",
        remedy: "retry after that operation finishes",
    },
    Refusal {
        code: "native_cleanup_required",
        when: "revoked identity cleanup cannot clear project context",
        remedy: "repair protected state and run auth logout",
    },
    Refusal {
        code: "auth_rejected",
        when: "identity restoration rejects the saved credential before the create call",
        remedy: "verify the account and sign in again if the credential was revoked",
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
        when: "native identity restoration is temporarily unavailable before the create call",
        remedy: "retry without changing local state",
    },
    Refusal {
        code: "auth_response_unreadable",
        when: "native identity restoration returns an unreadable response before the create call",
        remedy: "retry once, then sign in again or update ds if it persists",
    },
];

pub static COMMAND: Command = Command {
    id: "survey.entries.create",
    path: &["survey", "entries", "create"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Create one governed Survey entry headlessly.",
    purpose: "Parses one closed local JSON document and validates all caller-controlled identity, timestamp, context, GeoJSON, and payload bounds before profile or auth access; restores the native user; loads only its audience-fenced selected project and releases that lease before one fixed create-only backend call. The backend atomically binds the idempotency key, entry write, report watermark, and tile-stale fence. The receipt proves Firestore commit only; BigQuery mirror presence remains unconfirmed until a later governed read. There is no project, URL, method, body, token, origin, operation, retry, fallback, force, caller-authority, or Desktop override.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        FORM,
        DOC_ID,
        IDEMPOTENCY_KEY,
        CREATED_AT,
        DOCUMENT,
        CONTEXT_KEY,
        LANE,
    ],
    output: "Receipt only: lane, selected project identity, form and document identity, client version, Firestore committed, BigQuery mirror unconfirmed, and the verified replication clock/authority. It never returns request data or the idempotency key.",
    examples: &[Example {
        command: "ds survey entries create --form lv_poles_survey --doc-id pole-104 --idempotency-key '<opaque-key>' --created-at 2026-08-30T12:00:00Z --document ./pole-104.json --yes --output json",
        note: "Creates exactly one entry in the selected project after explicit confirmation.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // The complete caller-controlled grammar, including the local file, is
    // consumed before profile discovery, auth restoration, or project state.
    let request = parse(inputs)?;
    let headless = ds_cli_auth::survey_entry_create(inputs.require("lane")?, &request)?;
    let receipt = headless.receipt();
    Ok(json!({
        "lane": headless.lane(),
        "project": {
            "ds_project": receipt.project_id(),
            "project_name": headless.project_name(),
            "status": headless.project_status(),
        },
        "form": receipt.form_id(),
        "doc_id": receipt.doc_id(),
        "client_version": receipt.client_version(),
        "firestore": "committed",
        "bigquery_mirror": "unconfirmed",
        "replication": {
            "clock": receipt.replication_clock(),
            "authority": receipt.replication_authority(),
        },
    }))
}

fn parse(inputs: &Inputs) -> Result<SurveyEntryCreateRequest, Failure> {
    let document = load_document(inputs.require("document")?)?;
    let mut request = SurveyEntryCreateRequest::new(
        inputs.require("form")?,
        inputs.require("doc-id")?,
        inputs.require("idempotency-key")?,
        document.data,
        inputs.require("created-at")?,
        SurveyEntryCreateOrigin::Unknown,
    )
    .map_err(|_| invalid())?;
    if let Some(context_key) = inputs.value("context-key") {
        request = request
            .with_context_key(context_key)
            .map_err(|_| invalid())?;
    }
    if let Some(geometry) = document.geometry {
        request = request.with_geometry(geometry).map_err(|_| invalid())?;
    }
    if let Some(connectivity) = document.connectivity {
        request = request
            .with_connectivity(connectivity)
            .map_err(|_| invalid())?;
    }
    if let Some(detailed_location) = document.detailed_location {
        request = request
            .with_detailed_location(detailed_location)
            .map_err(|_| invalid())?;
    }
    Ok(request)
}

struct CreateDocument {
    data: Map<String, Value>,
    geometry: Option<Value>,
    connectivity: Option<Map<String, Value>>,
    detailed_location: Option<Map<String, Value>>,
}

fn load_document(raw: &str) -> Result<CreateDocument, Failure> {
    let path = Path::new(raw);
    let (file, byte_len) = open_document_file(path)?;
    let mut bytes = Vec::with_capacity(byte_len as usize);
    file.take(SURVEY_ENTRY_CREATE_MAX_PAYLOAD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| document_invalid())?;
    if bytes.len() > SURVEY_ENTRY_CREATE_MAX_PAYLOAD_BYTES {
        return Err(document_invalid());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| document_invalid())?;
    let mut object = value.as_object().cloned().ok_or_else(document_invalid)?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "data" | "geometry" | "connectivity" | "detailed_location"
        )
    }) {
        return Err(document_invalid());
    }
    let data = object
        .remove("data")
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(document_invalid)?;
    let geometry = non_null_optional(&mut object, "geometry")?;
    let connectivity = optional_object(&mut object, "connectivity")?;
    let detailed_location = optional_object(&mut object, "detailed_location")?;
    Ok(CreateDocument {
        data,
        geometry,
        connectivity,
        detailed_location,
    })
}

fn open_document_file(path: &Path) -> Result<(File, u64), Failure> {
    let path_metadata = std::fs::symlink_metadata(path).map_err(|_| document_invalid())?;
    if !safe_regular_metadata(&path_metadata) {
        return Err(document_invalid());
    }
    let file = open_no_follow(path).map_err(|_| document_invalid())?;
    let handle_metadata = file.metadata().map_err(|_| document_invalid())?;
    if !safe_regular_metadata(&handle_metadata) {
        return Err(document_invalid());
    }
    #[cfg(unix)]
    if !same_unix_identity(&path_metadata, &handle_metadata) {
        return Err(document_invalid());
    }
    Ok((file, handle_metadata.len()))
}

fn safe_regular_metadata(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_file()
        || metadata.len() > SURVEY_ENTRY_CREATE_MAX_PAYLOAD_BYTES as u64
    {
        return false;
    }
    #[cfg(windows)]
    if windows_reparse_point(metadata) {
        return false;
    }
    true
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(unix)]
fn same_unix_identity(path_metadata: &Metadata, handle_metadata: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    path_metadata.dev() == handle_metadata.dev() && path_metadata.ino() == handle_metadata.ino()
}

#[cfg(windows)]
fn open_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(windows)]
fn windows_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    has_windows_reparse_attribute(metadata.file_attributes())
}

#[cfg(any(windows, test))]
fn has_windows_reparse_attribute(attributes: u32) -> bool {
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn non_null_optional(object: &mut Map<String, Value>, key: &str) -> Result<Option<Value>, Failure> {
    match object.remove(key) {
        Some(Value::Null) => Err(document_invalid()),
        value => Ok(value),
    }
}

fn optional_object(
    object: &mut Map<String, Value>,
    key: &str,
) -> Result<Option<Map<String, Value>>, Failure> {
    match object.remove(key) {
        None => Ok(None),
        Some(Value::Object(value)) => Ok(Some(value)),
        Some(_) => Err(document_invalid()),
    }
}

fn invalid() -> Failure {
    Failure::invalid(
        "survey_entry_create_invalid",
        "the create identity, timestamp, context, GeoJSON, or payload violates the closed grammar",
    )
    .remedy("read `ds survey entries create --help` and correct the bounded typed inputs")
}

fn document_invalid() -> Failure {
    Failure::invalid(
        "survey_entry_create_document_invalid",
        "the create document is not one safe closed JSON object",
    )
    .remedy("pass one regular non-symlink JSON object no larger than 900 KiB with required data and only documented optional keys")
}

pub fn render(data: &Value) -> String {
    format!(
        "{}/{}  Firestore COMMITTED; BigQuery mirror NOT YET CONFIRMED\n",
        data["project"]["ds_project"].as_str().unwrap_or("project"),
        data["doc_id"].as_str().unwrap_or("document")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::args::parse as parse_args;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn inputs(arguments: &[&str]) -> Inputs {
        parse_args(
            &COMMAND,
            &arguments
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    fn fixture(contents: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ds-survey-create-{}-{}.json",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn valid_args(path: &str) -> Vec<&str> {
        vec![
            "--form",
            "lv_poles_survey",
            "--doc-id",
            "pole-104",
            "--idempotency-key",
            "opaque-replay-104",
            "--created-at",
            "2026-08-30T14:00:00+02:00",
            "--document",
            path,
        ]
    }

    #[test]
    fn closed_document_builds_the_core_request_before_auth() {
        let path = fixture(
            br#"{"data":{"condition":"good"},"geometry":{"type":"Point","coordinates":[30.0,-2.0]},"connectivity":{},"detailed_location":{}}"#,
        );
        let raw = path.to_string_lossy();
        let request = super::parse(&inputs(&valid_args(&raw))).unwrap();
        assert_eq!(request.form(), "lv_poles_survey");
        assert_eq!(request.doc_id(), "pole-104");
        assert_eq!(request.created_at(), "2026-08-30T12:00:00Z");
        assert_eq!(request.origin(), SurveyEntryCreateOrigin::Unknown);
        assert!(request.geometry().is_some());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn document_shape_is_closed_and_optional_values_are_non_null() {
        for body in [
            br#"{}"#.as_slice(),
            br#"{"data":[],"extra":1}"#.as_slice(),
            br#"{"data":{},"geometry":null}"#.as_slice(),
            br#"{"data":{},"connectivity":null}"#.as_slice(),
            br#"{"data":{},"detailed_location":[]}"#.as_slice(),
        ] {
            let path = fixture(body);
            assert_eq!(
                load_document(&path.to_string_lossy()).err().unwrap().code(),
                "survey_entry_create_document_invalid"
            );
            std::fs::remove_file(path).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_document_is_refused() {
        use std::os::unix::fs::symlink;
        let target = fixture(br#"{"data":{}}"#);
        let link = target.with_extension("link.json");
        symlink(&target, &link).unwrap();
        assert_eq!(
            load_document(&link.to_string_lossy()).err().unwrap().code(),
            "survey_entry_create_document_invalid"
        );
        std::fs::remove_file(link).unwrap();
        std::fs::remove_file(target).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn opened_handle_must_retain_the_path_identity() {
        let first = fixture(br#"{"data":{"row":1}}"#);
        let second = fixture(br#"{"data":{"row":2}}"#);
        let first_path = std::fs::symlink_metadata(&first).unwrap();
        let first_handle = open_no_follow(&first).unwrap().metadata().unwrap();
        let second_handle = open_no_follow(&second).unwrap().metadata().unwrap();
        assert!(same_unix_identity(&first_path, &first_handle));
        assert!(!same_unix_identity(&first_path, &second_handle));
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(second).unwrap();
    }

    #[test]
    fn windows_reparse_attribute_check_is_exact() {
        assert!(has_windows_reparse_attribute(0x0000_0400));
        assert!(has_windows_reparse_attribute(0x0000_0420));
        assert!(!has_windows_reparse_attribute(0));
        assert!(!has_windows_reparse_attribute(0x0000_0020));
    }

    #[test]
    fn descriptor_has_only_the_closed_create_grammar() {
        let names = COMMAND
            .args
            .iter()
            .map(|arg| arg.name)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            names,
            std::collections::BTreeSet::from([
                "context-key",
                "created-at",
                "doc-id",
                "document",
                "form",
                "idempotency-key",
                "lane",
            ])
        );
        for forbidden in [
            "project",
            "url",
            "method",
            "body",
            "token",
            "origin",
            "operation",
            "retry",
            "force",
            "authority",
            "desktop-descriptor",
        ] {
            assert!(COMMAND.arg(forbidden).is_none(), "unexpected --{forbidden}");
        }
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.effect, Effect::GlobalWrite);
        assert_eq!(COMMAND.execution, Execution::Sync);
        assert!(COMMAND.effect.needs_confirmation());
    }

    #[test]
    fn human_receipt_states_commit_and_mirror_boundary() {
        let output = render(&json!({
            "project": { "ds_project": "project-a" },
            "doc_id": "pole-104",
        }));
        assert!(output.contains("Firestore COMMITTED"));
        assert!(output.contains("BigQuery mirror NOT YET CONFIRMED"));
    }
}
