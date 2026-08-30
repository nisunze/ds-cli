//! `ds solar input capture` — selected-project snapshot to native owner intake.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{DISCOVERY_TIMEOUT, DS_SOLAR};

const GOVERNED_INTAKE_SCHEMA: &str = "ds-solar.governed-city-intake/v1";
const INTAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const MAX_INTAKE_BYTES: u64 = 32 * 1024 * 1024;

const CITY: Arg = Arg::value(
    "city",
    "<canonical-id>",
    "One exact Solar city in the selected headless project.",
)
.required();
const OUT: Arg = Arg::value(
    "out",
    "<path>",
    "Absent path for one ds-solar.governed-city-intake/v1 document.",
)
.required();
const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

macro_rules! refusal {
    ($code:literal, $when:literal, $remedy:literal) => {
        Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        }
    };
}

pub static COMMAND: Command = Command {
    id: "solar.input.capture",
    path: &["solar", "input", "capture"],
    contract: 1,
    summary: "Capture one governed project Solar city for native preparation.",
    purpose: "Preflights the installed ds-solar governed-intake schema before any authenticated read, restores the native user, derives eds_project/<selected-project>/eds_solar inside the fixed client call, requests only desktop_snapshot for one canonical city, and streams the exact bounded data object directly to `ds-solar intake --snapshot -`. The owner writes one fresh governed intake. Short-lived media download URLs exist only in private bounded memory and the stdin pipe: they never enter argv, stdout, a temporary file, or the resulting intake. There is no project/root override, generic API call, Desktop dependency, or processing-lane claim.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[CITY, OUT, LANE],
    output: "The fresh governed-intake path and SHA-256; selected lane/project/city; server snapshot/fingerprint and hashed receipt provenance plus expiry; bounded source/media counts; and the verified ds-solar owner schema. No snapshot body, raw receipt id, token, signed URL, or owner stdout is emitted.",
    examples: &[Example {
        command: "ds solar input capture --city pala --out ./pala.intake.json --output json",
        note: "Capture one selected-project city for `ds-solar prepare --governed-intake`.",
        runnable: false,
    }],
    refusals: &[
        refusal!(
            "native_profile_not_configured",
            "the packaged native profile is unavailable",
            "install one complete ds release"
        ),
        refusal!(
            "native_profile_digest_mismatch",
            "the packaged profile differs from this build",
            "reinstall one complete ds release"
        ),
        refusal!(
            "native_profile_unsafe",
            "the packaged profile is unsafe or malformed",
            "reinstall one complete ds release"
        ),
        refusal!(
            "headless_signed_out",
            "the lane has no restorable native user",
            "run ds auth login --email <address>"
        ),
        refusal!(
            "headless_project_not_selected",
            "the restored user has no audience-fenced selected project",
            "run ds auth project use --project <exact-id>"
        ),
        refusal!(
            "project_context_stale",
            "the project context belongs to another identity or audience",
            "select the project again with ds auth project use"
        ),
        refusal!(
            "native_state_unsafe",
            "protected native state is unsafe or unreadable",
            "repair the owner-only DS config directory"
        ),
        refusal!(
            "native_state_unavailable",
            "protected native state cannot be accessed",
            "repair the owner-only DS config directory"
        ),
        refusal!(
            "native_state_protection_unavailable",
            "this build has no protected-state adapter",
            "install a supported native ds build"
        ),
        refusal!(
            "native_state_root_invalid",
            "the configured state root is not absolute",
            "unset it or provide an absolute path"
        ),
        refusal!(
            "native_state_conflict",
            "another native operation holds the state lease",
            "retry after that operation finishes"
        ),
        refusal!(
            "native_cleanup_required",
            "revoked identity cleanup could not clear context",
            "repair protected state and run auth logout"
        ),
        refusal!(
            "auth_input_invalid",
            "the city id is outside the canonical contract",
            "pass one exact city id from the selected project"
        ),
        refusal!(
            "auth_rejected",
            "the fixed gateway refuses the verified request",
            "verify the account and its project access"
        ),
        refusal!(
            "auth_revoked",
            "Firebase permanently revoked the session",
            "sign in again interactively"
        ),
        refusal!(
            "auth_identity_mismatch",
            "Firebase returned another identity",
            "sign in again and report a repeated mismatch"
        ),
        refusal!(
            "auth_transient",
            "the fixed native service is temporarily unavailable",
            "retry without changing local state"
        ),
        refusal!(
            "auth_response_unreadable",
            "the Solar snapshot violates its exact response contract",
            "retry once, then update ds if it persists"
        ),
        refusal!(
            "solar_city_not_found",
            "the city does not exist in the selected project",
            "pass one exact live city id from that project"
        ),
        refusal!(
            "solar_engine_missing",
            "the packaged ds-solar owner is absent",
            "reinstall the complete ds release"
        ),
        refusal!(
            "solar_intake_stdin_unavailable",
            "ds-solar does not advertise the governed stdin-intake schema",
            "install a ds-solar build that supports intake --snapshot - and ds-solar.governed-city-intake/v1"
        ),
        refusal!(
            "solar_intake_output_exists",
            "--out already exists",
            "choose a fresh path; capture never overwrites"
        ),
        refusal!(
            "solar_intake_output_unsafe",
            "the owner output is missing, symlinked, non-file, or oversized",
            "choose a safe writable fresh path and retry"
        ),
        refusal!(
            "callee_contract_mismatch",
            "ds-solar build-info is not its JSON identity",
            "update ds and ds-solar to matching releases"
        ),
        refusal!(
            "callee_input_bounded",
            "the typed stdin document is empty or exceeds 32 MiB",
            "report the governed response; do not bypass the bound"
        ),
        refusal!(
            "callee_stdin_failed",
            "ds-solar did not consume the complete typed stdin document",
            "update ds and ds-solar to matching releases"
        ),
        refusal!(
            "callee_timed_out",
            "ds-solar did not finish intake within five minutes",
            "investigate the owner process before retrying"
        ),
        refusal!(
            "callee_wait_failed",
            "the ds-solar child could not be observed",
            "investigate the local process environment"
        ),
        refusal!(
            "engine_refused",
            "ds-solar rejected the governed snapshot",
            "read the bounded engine detail and repair the governed input"
        ),
    ],
    reference: Some("docs/reference/solar.md"),
    availability,
};

fn availability() -> Availability {
    match ds_cli_auth::native_availability() {
        Availability::Available => DS_SOLAR.availability(),
        unavailable => unavailable,
    }
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let output = PathBuf::from(inputs.require("out")?);
    ensure_absent(&output)?;
    preflight_owner_intake()?;

    let headless = ds_cli_auth::solar_snapshot(inputs.require("lane")?, inputs.require("city")?)?;
    let snapshot = headless.snapshot();
    let args = [
        OsString::from("--snapshot"),
        OsString::from("-"),
        OsString::from("--out"),
        output.as_os_str().to_owned(),
    ];
    let completed =
        DS_SOLAR.call_with_stdin("intake", &args, snapshot.intake_bytes(), INTAKE_TIMEOUT)?;
    if !completed.succeeded() {
        return Err(DS_SOLAR.failure_from(&completed, "intake"));
    }
    let expected = ExpectedIntake {
        ds_project: snapshot.ds_project().to_owned(),
        city: snapshot.template_id().to_owned(),
        root: format!("eds_project/{}/eds_solar", snapshot.ds_project()),
        snapshot_sha256: snapshot.snapshot_sha256().to_owned(),
        input_base_fingerprint: snapshot.input_base_fingerprint().to_owned(),
        snapshot_receipt_id: snapshot.snapshot_receipt_id().to_owned(),
        snapshot_receipt_expires_at: snapshot.snapshot_receipt_expires_at().to_owned(),
        firestore_read_time: snapshot.firestore_read_time().to_owned(),
        snapshot_bytes: snapshot.snapshot_bytes(),
        source_document_count: snapshot.source_document_count(),
        media_download_count: snapshot.media_download_count(),
    };
    let verified = safe_output(&output, &expected)?;
    let snapshot_receipt_sha256 = format!(
        "{:x}",
        Sha256::digest(snapshot.snapshot_receipt_id().as_bytes())
    );

    Ok(json!({
        "out": output,
        "intake_sha256": verified.sha256,
        "intake_bytes": verified.bytes,
        "schema_version": verified.schema_version,
        "city_content_digest": verified.city_content_digest,
        "lane": headless.lane(),
        "project": {
            "ds_project": snapshot.ds_project(),
            "project_name": headless.project_name(),
            "status": headless.project_status(),
        },
        "city": snapshot.template_id(),
        "source": {
            "snapshot_sha256": snapshot.snapshot_sha256(),
            "input_base_fingerprint": snapshot.input_base_fingerprint(),
            "snapshot_receipt_sha256": snapshot_receipt_sha256,
            "snapshot_receipt_expires_at": snapshot.snapshot_receipt_expires_at(),
            "snapshot_bytes": snapshot.snapshot_bytes(),
            "source_document_count": snapshot.source_document_count(),
            "media_download_count": snapshot.media_download_count(),
        },
        "handoff": "bounded-owner-stdin",
    }))
}

fn preflight_owner_intake() -> Result<(), Failure> {
    let identity = DS_SOLAR.call_json("build-info", &[], DISCOVERY_TIMEOUT)?;
    let supports = identity
        .as_object()
        .filter(|object| object.get("name").and_then(Value::as_str) == Some("ds-solar-engine"))
        .and_then(|object| object.get("schemas"))
        .and_then(Value::as_array)
        .is_some_and(|schemas| {
            schemas
                .iter()
                .any(|schema| schema.as_str() == Some(GOVERNED_INTAKE_SCHEMA))
        });
    if supports {
        Ok(())
    } else {
        Err(Failure::unavailable(
            "solar_intake_stdin_unavailable",
            "the installed ds-solar does not advertise its governed stdin-intake contract",
        )
        .remedy(
            "install a ds-solar build that supports intake --snapshot - and ds-solar.governed-city-intake/v1",
        ))
    }
}

fn ensure_absent(path: &Path) -> Result<(), Failure> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(Failure::conflict(
            "solar_intake_output_exists",
            "the governed Solar intake output already exists",
        )
        .remedy("choose a fresh --out path; capture never overwrites")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Failure::unavailable(
            "solar_intake_output_unsafe",
            "the governed Solar intake output path cannot be inspected safely",
        )
        .remedy("choose a safe writable fresh --out path")),
    }
}

fn safe_output(path: &Path, expected: &ExpectedIntake) -> Result<VerifiedIntake, Failure> {
    let mut file = open_output(path)?;
    let handle_metadata = file.metadata().map_err(|_| unsafe_output())?;
    if !handle_metadata.is_file()
        || handle_metadata.len() == 0
        || handle_metadata.len() > MAX_INTAKE_BYTES
        || !private_output_metadata(&handle_metadata)
    {
        return Err(unsafe_output());
    }
    same_output_identity(path, &handle_metadata)?;
    let mut bytes = Vec::with_capacity(handle_metadata.len() as usize);
    file.by_ref()
        .take(MAX_INTAKE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unsafe_output())?;
    if bytes.is_empty()
        || bytes.len() as u64 != handle_metadata.len()
        || bytes.len() as u64 > MAX_INTAKE_BYTES
    {
        return Err(unsafe_output());
    }
    same_output_identity(path, &handle_metadata)?;
    verify_intake(&bytes, expected)
}

#[cfg(unix)]
fn private_output_metadata(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o077 == 0
}

#[cfg(not(unix))]
fn private_output_metadata(_metadata: &fs::Metadata) -> bool {
    true
}

fn same_output_identity(path: &Path, handle_metadata: &fs::Metadata) -> Result<(), Failure> {
    let path_metadata = fs::symlink_metadata(path).map_err(|_| unsafe_output())?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        return Err(unsafe_output());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if handle_metadata.dev() != path_metadata.dev()
            || handle_metadata.ino() != path_metadata.ino()
        {
            return Err(unsafe_output());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn open_output(path: &Path) -> Result<File, Failure> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| unsafe_output())
}

#[cfg(not(unix))]
fn open_output(path: &Path) -> Result<File, Failure> {
    OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|_| unsafe_output())
}

struct ExpectedIntake {
    ds_project: String,
    city: String,
    root: String,
    snapshot_sha256: String,
    input_base_fingerprint: String,
    snapshot_receipt_id: String,
    snapshot_receipt_expires_at: String,
    firestore_read_time: String,
    snapshot_bytes: usize,
    source_document_count: usize,
    media_download_count: usize,
}

struct VerifiedIntake {
    sha256: String,
    bytes: usize,
    schema_version: String,
    city_content_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GovernedIntakeWire {
    schema_version: String,
    city_input: CityInputWire,
    authority: IntakeAuthorityWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CityInputWire {
    schema_version: String,
    identity: CityIdentityWire,
    site: Value,
    revision: RevisionWire,
    run_options: Value,
    seeded: Value,
    content_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CityIdentityWire {
    project_id: String,
    root: String,
    city_id: String,
    display_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionWire {
    source: String,
    captured_at: String,
    #[serde(default)]
    record_updated_at_ms: Option<i64>,
    #[serde(default)]
    revision: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntakeAuthorityWire {
    source_snapshot_sha256: String,
    input_base_fingerprint: String,
    snapshot_receipt_id: String,
    snapshot_receipt_expires_at: String,
    firestore_read_time: String,
    snapshot_bytes: usize,
    source_document_count: usize,
    media_download_count: usize,
}

fn verify_intake(bytes: &[u8], expected: &ExpectedIntake) -> Result<VerifiedIntake, Failure> {
    let intake: GovernedIntakeWire = serde_json::from_slice(bytes).map_err(|_| unsafe_output())?;
    let digest = intake.city_input.content_digest.as_bytes();
    if intake.schema_version != GOVERNED_INTAKE_SCHEMA
        || intake.city_input.schema_version != "ds-solar.city-input/v1"
        || intake.city_input.identity.project_id != expected.ds_project
        || intake.city_input.identity.city_id != expected.city
        || intake.city_input.identity.root != expected.root
        || intake.city_input.identity.display_name.is_empty()
        || intake.city_input.identity.display_name.trim() != intake.city_input.identity.display_name
        || intake
            .city_input
            .identity
            .display_name
            .chars()
            .any(char::is_control)
        || intake.city_input.revision.source != "ds-brain"
        || intake.city_input.revision.captured_at != intake.authority.firestore_read_time
        || intake.city_input.revision.revision.as_deref()
            != Some(expected.input_base_fingerprint.as_str())
        || intake.city_input.revision.record_updated_at_ms.is_some()
        || digest.len() != 71
        || !digest.starts_with(b"sha256:")
        || !digest[7..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(*byte, b'a'..=b'f'))
        || !intake.city_input.site.is_object()
        || !intake.city_input.run_options.is_object()
        || !intake.city_input.seeded.is_object()
        || intake.authority.source_snapshot_sha256 != expected.snapshot_sha256
        || intake.authority.input_base_fingerprint != expected.input_base_fingerprint
        || intake.authority.snapshot_receipt_id != expected.snapshot_receipt_id
        || !same_utc_rfc3339_spelling(
            &intake.authority.snapshot_receipt_expires_at,
            &expected.snapshot_receipt_expires_at,
        )
        || !same_utc_rfc3339_spelling(
            &intake.authority.firestore_read_time,
            &expected.firestore_read_time,
        )
        || intake.authority.firestore_read_time.is_empty()
        || intake.authority.firestore_read_time.len() > 128
        || intake.authority.firestore_read_time.trim() != intake.authority.firestore_read_time
        || intake
            .authority
            .firestore_read_time
            .chars()
            .any(char::is_control)
        || intake.authority.snapshot_bytes != expected.snapshot_bytes
        || intake.authority.source_document_count != expected.source_document_count
        || intake.authority.media_download_count != expected.media_download_count
    {
        return Err(unsafe_output());
    }
    Ok(VerifiedIntake {
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len(),
        schema_version: intake.schema_version,
        city_content_digest: intake.city_input.content_digest,
    })
}

// ds-brain's Go producer spells UTC with `Z`; ds-solar parses that authority
// and chrono's canonical writer spells the same instant with `+00:00`.
fn same_utc_rfc3339_spelling(left: &str, right: &str) -> bool {
    fn utc_suffix(value: &str) -> &str {
        value.strip_suffix('Z').unwrap_or(value)
    }
    if left.ends_with('Z') && right.ends_with("+00:00") {
        utc_suffix(left) == right.strip_suffix("+00:00").unwrap_or(right)
    } else if right.ends_with('Z') && left.ends_with("+00:00") {
        utc_suffix(right) == left.strip_suffix("+00:00").unwrap_or(left)
    } else {
        left == right
    }
}

fn unsafe_output() -> Failure {
    Failure::failed(
        "solar_intake_output_unsafe",
        "ds-solar did not create one safe bounded governed-intake file",
    )
    .remedy("choose a safe writable fresh --out path and retry")
}

pub fn render(value: &Value) -> String {
    format!(
        "Governed Solar intake captured for {}.\nIntake: {}\nSHA-256: {}",
        value["city"].as_str().unwrap_or(""),
        value["out"].as_str().unwrap_or(""),
        value["intake_sha256"].as_str().unwrap_or(""),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_has_no_project_root_desktop_or_snapshot_file_escape() {
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
        assert_eq!(COMMAND.path, ["solar", "input", "capture"]);
        for forbidden in ["project", "root", "desktop-descriptor", "snapshot"] {
            assert!(
                COMMAND
                    .args
                    .iter()
                    .all(|argument| argument.name != forbidden)
            );
        }
        assert!(COMMAND.purpose.contains("stdin"));
        assert!(COMMAND.purpose.contains("never enter argv"));
    }

    #[test]
    fn output_precondition_is_create_new() {
        let path =
            std::env::temp_dir().join(format!("ds-solar-capture-output-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        assert!(ensure_absent(&path).is_ok());
        fs::write(&path, b"{}").unwrap();
        assert_eq!(
            ensure_absent(&path).unwrap_err().code(),
            "solar_intake_output_exists"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn renderer_never_prints_snapshot_receipt_or_media_authority() {
        let rendered = render(&json!({
            "city": "city_1",
            "out": "city.intake.json",
            "intake_sha256": "a".repeat(64),
            "source": {
                "snapshot_receipt_id": "receipt-secret",
                "download_url": "https://signed.example/secret"
            }
        }));
        assert!(!rendered.contains("receipt-secret"));
        assert!(!rendered.contains("signed.example"));
    }

    fn expected() -> ExpectedIntake {
        ExpectedIntake {
            ds_project: "project-1".to_owned(),
            city: "city_1".to_owned(),
            root: "eds_project/project-1/eds_solar".to_owned(),
            snapshot_sha256: "a".repeat(64),
            input_base_fingerprint: "b".repeat(64),
            snapshot_receipt_id: "123e4567-e89b-42d3-a456-426614174000".to_owned(),
            snapshot_receipt_expires_at: "2030-03-24T12:00:00+00:00".to_owned(),
            firestore_read_time: "2030-03-17T12:00:00Z".to_owned(),
            snapshot_bytes: 1234,
            source_document_count: 4,
            media_download_count: 0,
        }
    }

    fn intake_fixture() -> Value {
        json!({
            "schema_version": GOVERNED_INTAKE_SCHEMA,
            "city_input": {
                "schema_version": "ds-solar.city-input/v1",
                "identity": {
                    "project_id": "project-1",
                    "root": "eds_project/project-1/eds_solar",
                    "city_id": "city_1",
                    "display_name": "City One"
                },
                "site": {},
                "revision": {
                    "source": "ds-brain",
                    "captured_at": "2030-03-17T12:00:00+00:00",
                    "revision": "b".repeat(64)
                },
                "run_options": {},
                "seeded": {},
                "content_digest": format!("sha256:{}", "c".repeat(64))
            },
            "authority": {
                "source_snapshot_sha256": "a".repeat(64),
                "input_base_fingerprint": "b".repeat(64),
                "snapshot_receipt_id": "123e4567-e89b-42d3-a456-426614174000",
                "snapshot_receipt_expires_at": "2030-03-24T12:00:00+00:00",
                "firestore_read_time": "2030-03-17T12:00:00+00:00",
                "snapshot_bytes": 1234,
                "source_document_count": 4,
                "media_download_count": 0
            }
        })
    }

    #[test]
    fn owner_intake_is_verified_against_the_captured_authority() {
        let exact = serde_json::to_vec(&intake_fixture()).unwrap();
        let verified = verify_intake(&exact, &expected()).unwrap();
        assert_eq!(verified.schema_version, GOVERNED_INTAKE_SCHEMA);
        assert_eq!(
            verified.city_content_digest,
            format!("sha256:{}", "c".repeat(64))
        );

        for mutate in [
            |value: &mut Value| value["city_input"]["identity"]["project_id"] = json!("other"),
            |value: &mut Value| value["city_input"]["identity"]["city_id"] = json!("other"),
            |value: &mut Value| {
                value["authority"]["source_snapshot_sha256"] = json!("d".repeat(64))
            },
            |value: &mut Value| {
                value["authority"]["download_url"] = json!("https://signed.example/secret")
            },
            |value: &mut Value| value["unexpected"] = json!(true),
        ] {
            let mut invalid = intake_fixture();
            mutate(&mut invalid);
            assert_eq!(
                verify_intake(&serde_json::to_vec(&invalid).unwrap(), &expected())
                    .err()
                    .expect("mismatched or expanded intake must be refused")
                    .code(),
                "solar_intake_output_unsafe"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn output_identity_refuses_a_path_swapped_after_open() {
        let root =
            std::env::temp_dir().join(format!("ds-solar-capture-swap-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("intake.json");
        let original = root.join("original.json");
        fs::write(&path, b"original").unwrap();
        let handle = open_output(&path).unwrap();
        let metadata = handle.metadata().unwrap();
        fs::rename(&path, &original).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert_eq!(
            same_output_identity(&path, &metadata).unwrap_err().code(),
            "solar_intake_output_unsafe"
        );
        fs::remove_file(path).unwrap();
        fs::remove_file(original).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn owner_intake_must_remain_private_on_disk() {
        use std::os::unix::fs::PermissionsExt;

        let root =
            std::env::temp_dir().join(format!("ds-solar-capture-mode-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("intake.json");
        fs::write(&path, serde_json::to_vec(&intake_fixture()).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            safe_output(&path, &expected())
                .err()
                .expect("group-readable authority must be refused")
                .code(),
            "solar_intake_output_unsafe"
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(safe_output(&path, &expected()).is_ok());
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
