//! `ds solar input prepare` — governed intake to native prepared artifacts.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use ds_solar_contracts::EngineIdentity;
use ds_solar_project::{intake::GovernedCityIntake, preparation::prepare_cached};
const MAX_INTAKE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;

pub static COMMAND: Command = Command {
    id: "solar.input.prepare",
    path: &["solar", "input", "prepare"],
    contract: 2,
    summary: "Prepare one governed Solar intake from a verified local cache.",
    purpose: "Prepare one intake from a populated local reference cache through the linked engine-free ds-solar-project owner. The fresh private directory holds the prepared city input, its publication claim and a validated Server submission envelope; submit that with server solar submit. Headless and cache-only: a cache miss refuses instead of reaching the network.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "intake",
            "<path>",
            "Private ds-solar.governed-city-intake/v1 file from `ds solar input capture`.",
        )
        .required(),
        Arg::value(
            "cache",
            "<dir>",
            "Existing verified ds-solar reference cache; never a provider URL.",
        )
        .required(),
        Arg::value(
            "out",
            "<dir>",
            "Absent directory for the prepared input and private publication claim.",
        )
        .required(),
    ],
    output: "City id, output directory, prepared-input, publication-claim and Server-submission paths with byte counts and SHA-256, and preparation_owner (ds-solar-project, source_sha or null, contracts_build_manifest_sha256). Intake contents and cache records are never emitted.",
    examples: &[Example {
        command: "ds solar input prepare --intake ./pala.intake.json --cache ./solar-reference-cache --out ./pala-prepared --output json",
        note: "Prepare one captured city without Desktop or a network call.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "solar_prepare_intake_unsafe",
            when: "--intake is missing, symlinked, non-file, public on Unix, empty, or above 32 MiB",
            remedy: "pass the unchanged private output of `ds solar input capture`",
        },
        Refusal {
            code: "solar_prepare_cache_unsafe",
            when: "--cache is missing, symlinked, or not a directory",
            remedy: "pass one existing verified ds-solar reference cache directory",
        },
        Refusal {
            code: "solar_prepare_output_exists",
            when: "--out already exists",
            remedy: "choose a fresh directory; headless preparation never overwrites",
        },
        Refusal {
            code: "solar_prepare_output_unsafe",
            when: "the output directory cannot be created, or the owner emits anything but one prepared input, one claim and a valid Server envelope",
            remedy: "choose a safe writable fresh directory and use the matching ds release",
        },
        Refusal {
            code: "engine_refused",
            when: "the linked preparation owner rejects the governed intake or the verified cache has no exact reference bundle",
            remedy: "read detail.engine; acquire the governed reference cache through a reviewed DS route, then retry unchanged",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let intake = PathBuf::from(inputs.require("intake")?);
    let cache = PathBuf::from(inputs.require("cache")?);
    let out = PathBuf::from(inputs.require("out")?);

    validate_intake(&intake)?;
    validate_cache(&cache)?;
    ensure_absent(&out)?;
    let intake = read_intake(&intake)?;
    let (input, claim) = prepare_cached(intake, &cache).map_err(owner_refused)?;
    create_private_output(&out)?;
    let city = &input.city.identity.city_id;
    write_json_new(&out.join(format!("{city}.prepared.json")), &input)?;
    write_json_new(
        &out.join(format!("{city}.prepared-publication-claim.json")),
        &claim,
    )?;

    let prepared = verify_output(&out)?;
    let mut input_bytes = Vec::new();
    open_no_follow(&prepared.input.path)?
        .take(MAX_ARTIFACT_BYTES + 1)
        .read_to_end(&mut input_bytes)
        .map_err(|_| unsafe_output())?;
    let mut claim_bytes = Vec::new();
    open_no_follow(&prepared.claim.path)?
        .take(MAX_ARTIFACT_BYTES + 1)
        .read_to_end(&mut claim_bytes)
        .map_err(|_| unsafe_output())?;
    let envelope = ds_compute_runtime::encode_solar_server_submission(&input_bytes, &claim_bytes)
        .map_err(|_| unsafe_output())?;
    let submission_path = out.join(format!("{}.server-submission.json", prepared.city));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut submission_file = options
        .open(&submission_path)
        .map_err(|_| unsafe_output())?;
    submission_file
        .write_all(&envelope)
        .and_then(|_| submission_file.sync_all())
        .map_err(|_| unsafe_output())?;
    let submission = hash_artifact(&submission_path)?;
    Ok(json!({
        "city": prepared.city,
        "out": out,
        "artifacts": [
            {
                "role": "prepared_input",
                "path": prepared.input.path,
                "bytes": prepared.input.bytes,
                "sha256": prepared.input.sha256,
            },
            {
                "role": "publication_claim",
                "path": prepared.claim.path,
                "bytes": prepared.claim.bytes,
                "sha256": prepared.claim.sha256,
            },
            {
                "role": "server_submission", "path": submission.path,
                "bytes": submission.bytes, "sha256": submission.sha256,
            }
        ],
        "preparation_owner": preparation_owner(),
        "network": false,
        "cache_mode": "verified-local-only",
    }))
}

fn preparation_owner() -> Value {
    let identity = EngineIdentity::current();
    json!({
        "name": "ds-solar-project",
        "source_sha": (!identity.source_sha.is_empty()).then_some(identity.source_sha),
        "contracts_build_manifest_sha256": EngineIdentity::publication_manifest_sha256(),
    })
}

fn owner_refused(error: impl std::fmt::Display) -> Failure {
    let message = error
        .to_string()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(6)
        .map(|line| line.chars().take(200).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    Failure::failed("engine_refused", "governed Solar cache-only preparation did not complete")
        .remedy("read detail.engine; retain the unchanged intake and acquire its exact reference cache through a reviewed DS route before retrying a cache miss")
        .detail(json!({"owner":"ds-solar-project","operation":"prepare_cached","engine":message}))
}

fn read_intake(path: &Path) -> Result<GovernedCityIntake, Failure> {
    let file = open_no_follow(path).map_err(|_| unsafe_intake())?;
    let metadata = file.metadata().map_err(|_| unsafe_intake())?;
    if !metadata.is_file()
        || !private_metadata(&metadata)
        || metadata.len() == 0
        || metadata.len() > MAX_INTAKE_BYTES
    {
        return Err(unsafe_intake());
    }
    same_file_identity(path, &metadata).map_err(|_| unsafe_intake())?;
    let mut bytes = Vec::new();
    file.take(MAX_INTAKE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unsafe_intake())?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > MAX_INTAKE_BYTES {
        return Err(unsafe_intake());
    }
    same_file_identity(path, &metadata).map_err(|_| unsafe_intake())?;
    serde_json::from_slice(&bytes).map_err(owner_refused)
}

fn write_json_new(path: &Path, value: &impl serde::Serialize) -> Result<(), Failure> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|_| unsafe_output())?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
        return Err(unsafe_output());
    }
    let parent = path.parent().ok_or_else(unsafe_output)?;
    let mut stage = tempfile::NamedTempFile::new_in(parent).map_err(|_| unsafe_output())?;
    stage
        .write_all(&bytes)
        .and_then(|_| stage.as_file().sync_all())
        .map_err(|_| unsafe_output())?;
    stage.persist_noclobber(path).map_err(|_| unsafe_output())?;
    ds_solar_project::files::sync_dir(parent).map_err(|_| unsafe_output())
}

fn validate_intake(path: &Path) -> Result<(), Failure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| unsafe_intake())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_INTAKE_BYTES
        || !private_metadata(&metadata)
    {
        return Err(unsafe_intake());
    }
    Ok(())
}

fn validate_cache(path: &Path) -> Result<(), Failure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| unsafe_cache())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(unsafe_cache());
    }
    Ok(())
}

fn ensure_absent(path: &Path) -> Result<(), Failure> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(Failure::conflict(
            "solar_prepare_output_exists",
            "the headless Solar preparation output already exists",
        )
        .remedy("choose a fresh --out directory; preparation never overwrites")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(unsafe_output()),
    }
}

#[cfg(unix)]
fn create_private_output(path: &Path) -> Result<(), Failure> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder.create(path).map_err(|_| unsafe_output())
}

#[cfg(not(unix))]
fn create_private_output(path: &Path) -> Result<(), Failure> {
    fs::create_dir(path).map_err(|_| unsafe_output())
}

#[derive(Debug)]
struct PreparedOutput {
    city: String,
    input: Artifact,
    claim: Artifact,
}

#[derive(Debug)]
struct Artifact {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

fn verify_output(out: &Path) -> Result<PreparedOutput, Failure> {
    let metadata = fs::symlink_metadata(out).map_err(|_| unsafe_output())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || !private_metadata(&metadata) {
        return Err(unsafe_output());
    }

    let mut paths = fs::read_dir(out)
        .map_err(|_| unsafe_output())?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|_| unsafe_output()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    if paths.len() != 2 {
        return Err(unsafe_output());
    }

    let input_path = paths
        .iter()
        .find(|path| path.to_string_lossy().ends_with(".prepared.json"))
        .ok_or_else(unsafe_output)?;
    let claim_path = paths
        .iter()
        .find(|path| {
            path.to_string_lossy()
                .ends_with(".prepared-publication-claim.json")
        })
        .ok_or_else(unsafe_output)?;
    let input_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(unsafe_output)?;
    let claim_name = claim_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(unsafe_output)?;
    let city = input_name
        .strip_suffix(".prepared.json")
        .filter(|city| !city.is_empty())
        .ok_or_else(unsafe_output)?;
    if claim_name.strip_suffix(".prepared-publication-claim.json") != Some(city) {
        return Err(unsafe_output());
    }

    Ok(PreparedOutput {
        city: city.to_owned(),
        input: hash_artifact(input_path)?,
        claim: hash_artifact(claim_path)?,
    })
}

fn hash_artifact(path: &Path) -> Result<Artifact, Failure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| unsafe_output())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_ARTIFACT_BYTES
    {
        return Err(unsafe_output());
    }
    let mut file = open_no_follow(path)?;
    let handle_metadata = file.metadata().map_err(|_| unsafe_output())?;
    same_file_identity(path, &handle_metadata)?;
    let mut hasher = Sha256::new();
    let bytes = std::io::copy(
        &mut Read::by_ref(&mut file).take(MAX_ARTIFACT_BYTES + 1),
        &mut hasher,
    )
    .map_err(|_| unsafe_output())?;
    if bytes != handle_metadata.len() || bytes > MAX_ARTIFACT_BYTES {
        return Err(unsafe_output());
    }
    same_file_identity(path, &handle_metadata)?;
    Ok(Artifact {
        path: path.to_owned(),
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> Result<File, Failure> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| unsafe_output())
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> Result<File, Failure> {
    OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|_| unsafe_output())
}

fn same_file_identity(path: &Path, handle_metadata: &fs::Metadata) -> Result<(), Failure> {
    #[cfg(not(unix))]
    let _ = handle_metadata;
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
fn private_metadata(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o077 == 0
}

#[cfg(not(unix))]
fn private_metadata(_metadata: &fs::Metadata) -> bool {
    true
}

fn unsafe_intake() -> Failure {
    Failure::invalid(
        "solar_prepare_intake_unsafe",
        "the governed Solar intake is not one safe bounded private file",
    )
    .remedy("pass the unchanged private output of `ds solar input capture`")
}

fn unsafe_cache() -> Failure {
    Failure::invalid(
        "solar_prepare_cache_unsafe",
        "the Solar reference cache is not one safe existing directory",
    )
    .remedy("pass one existing verified ds-solar reference cache directory")
}

fn unsafe_output() -> Failure {
    Failure::failed(
        "solar_prepare_output_unsafe",
        "the headless Solar prepared output is not one safe private two-artifact directory",
    )
    .remedy("choose a safe writable fresh --out directory and use the matching ds release")
}

pub fn render(value: &Value) -> String {
    format!(
        "Governed Solar input prepared for {}.\nPrepared directory: {}\nArtifacts: 3\nNetwork: no",
        value["city"].as_str().unwrap_or(""),
        value["out"].as_str().unwrap_or(""),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ds-solar-input-prepare-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn contract_is_cache_only_and_has_no_authority_escape() {
        assert_eq!(COMMAND.contract, 2);
        assert_eq!(availability(), Availability::Available);
        assert_eq!(COMMAND.authority, Authority::None);
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
        assert_eq!(COMMAND.path, ["solar", "input", "prepare"]);
        for forbidden in [
            "project",
            "root",
            "reference-url",
            "weather-token",
            "api-key",
            "overwrite",
            "city",
        ] {
            assert!(
                COMMAND
                    .args
                    .iter()
                    .all(|argument| argument.name != forbidden)
            );
        }
        assert!(COMMAND.purpose.contains("cache-only"));
        assert!(COMMAND.purpose.contains("cache miss refuses"));
    }

    #[test]
    fn output_is_create_new_and_private() {
        let root = temporary_root("private");
        let out = root.join("prepared");
        fs::create_dir(&root).unwrap();
        assert!(ensure_absent(&out).is_ok());
        create_private_output(&out).unwrap();
        assert_eq!(
            ensure_absent(&out).unwrap_err().code(),
            "solar_prepare_output_exists"
        );
        #[cfg(unix)]
        assert!(private_metadata(&fs::metadata(&out).unwrap()));
        fs::remove_dir(out).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn inventory_requires_one_matching_prepared_pair() {
        let root = temporary_root("inventory");
        fs::create_dir(&root).unwrap();
        create_private_output(&root.join("prepared")).unwrap();
        let out = root.join("prepared");
        fs::write(out.join("pala.prepared.json"), b"prepared").unwrap();
        fs::write(out.join("pala.prepared-publication-claim.json"), b"claim").unwrap();
        let verified = verify_output(&out).unwrap();
        assert_eq!(verified.city, "pala");
        assert_eq!(verified.input.bytes, 8);
        assert_eq!(verified.claim.bytes, 5);

        fs::write(out.join("unexpected.json"), b"extra").unwrap();
        assert_eq!(
            verify_output(&out).unwrap_err().code(),
            "solar_prepare_output_unsafe"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn intake_must_be_private_and_not_a_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root = temporary_root("intake");
        fs::create_dir(&root).unwrap();
        let intake = root.join("city.intake.json");
        fs::write(&intake, b"{}").unwrap();
        fs::set_permissions(&intake, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            validate_intake(&intake).unwrap_err().code(),
            "solar_prepare_intake_unsafe"
        );
        fs::set_permissions(&intake, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(validate_intake(&intake).is_ok());
        let linked = root.join("linked.intake.json");
        symlink(&intake, &linked).unwrap();
        assert_eq!(
            validate_intake(&linked).unwrap_err().code(),
            "solar_prepare_intake_unsafe"
        );
        fs::remove_file(linked).unwrap();
        fs::remove_file(intake).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
