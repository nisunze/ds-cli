//! `ds solar input prepare` — governed intake to native prepared artifacts.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{DISCOVERY_TIMEOUT, DS_SOLAR, PREPARE_TIMEOUT};

const GOVERNED_INTAKE_SCHEMA: &str = "ds-solar.governed-city-intake/v1";
const PREPARED_INPUT_SCHEMA: &str = "ds-solar.prepared-city-input/v1";
const MAX_INTAKE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;

pub static COMMAND: Command = Command {
    id: "solar.input.prepare",
    path: &["solar", "input", "prepare"],
    contract: 1,
    summary: "Prepare one governed Solar intake from a verified local cache.",
    purpose: "Runs the fixed native `ds-solar prepare --governed-intake` contract for one intake and one already populated local reference cache. The output is a fresh private directory containing the prepared city input and its publication handoff claim. This is headless, cache-only preparation: it has no Desktop dependency, project override, provider URL, token, API key, overwrite, fixture, or generic engine argument. A cache miss refuses instead of reaching the network.",
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
    output: "The city id, fresh private output directory, exact prepared-input and publication-claim paths, byte counts and SHA-256 digests, plus the immutable ds-solar build manifest digest. Intake contents, receipt authority, cache records and owner stdout are never emitted.",
    examples: &[Example {
        command: "ds solar input prepare --intake ./pala.intake.json --cache ./solar-reference-cache --out ./pala-prepared --output json",
        note: "Prepare one captured city without Desktop or a network call.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "solar_engine_missing",
            when: "the packaged ds-solar owner is absent",
            remedy: "reinstall the complete ds release",
        },
        Refusal {
            code: "solar_prepare_schema_unavailable",
            when: "ds-solar does not advertise governed-intake and prepared-input schemas",
            remedy: "install matching ds and ds-solar releases",
        },
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
            when: "the private output directory cannot be created or the owner emits anything except one safe prepared input and one claim",
            remedy: "choose a safe writable fresh directory and install matching ds and ds-solar releases",
        },
        Refusal {
            code: "engine_refused",
            when: "ds-solar rejects the governed intake or the verified cache has no exact reference bundle",
            remedy: "read detail.engine; acquire the governed reference cache through a reviewed DS route, then retry unchanged",
        },
        Refusal {
            code: "callee_contract_mismatch",
            when: "ds-solar build-info is not its JSON identity",
            remedy: "update ds and ds-solar to matching releases",
        },
        Refusal {
            code: "callee_timed_out",
            when: "preparation exceeds the thirty-minute bound",
            remedy: "investigate the local cache and owner process before retrying",
        },
        Refusal {
            code: "callee_wait_failed",
            when: "the ds-solar child cannot be observed",
            remedy: "investigate the local process environment",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    availability,
};

fn availability() -> Availability {
    DS_SOLAR.availability()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let intake = PathBuf::from(inputs.require("intake")?);
    let cache = PathBuf::from(inputs.require("cache")?);
    let out = PathBuf::from(inputs.require("out")?);

    validate_intake(&intake)?;
    validate_cache(&cache)?;
    ensure_absent(&out)?;
    let engine = preflight_owner_prepare()?;
    create_private_output(&out)?;

    let args = [
        OsString::from("--governed-intake"),
        intake.as_os_str().to_owned(),
        OsString::from("--cache"),
        cache.as_os_str().to_owned(),
        OsString::from("--out"),
        out.as_os_str().to_owned(),
    ];
    let completed = DS_SOLAR.call("prepare", &args, PREPARE_TIMEOUT)?;
    if !completed.succeeded() {
        return Err(DS_SOLAR.failure_from(&completed, "prepare").remedy(
            "this command is cache-only; acquire the governed reference cache through a reviewed DS route before retrying a cache miss",
        ));
    }

    let prepared = verify_output(&out)?;
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
            }
        ],
        "engine": {
            "source_sha": engine.get("source_sha"),
            "build_manifest_sha256": engine.get("build_manifest_sha256"),
        },
        "network": false,
        "cache_mode": "verified-local-only",
    }))
}

fn preflight_owner_prepare() -> Result<Value, Failure> {
    let identity = DS_SOLAR.call_json("build-info", &[], DISCOVERY_TIMEOUT)?;
    let supports = identity
        .as_object()
        .filter(|object| object.get("name").and_then(Value::as_str) == Some("ds-solar-engine"))
        .and_then(|object| object.get("schemas"))
        .and_then(Value::as_array)
        .is_some_and(|schemas| {
            [GOVERNED_INTAKE_SCHEMA, PREPARED_INPUT_SCHEMA]
                .iter()
                .all(|expected| {
                    schemas
                        .iter()
                        .any(|schema| schema.as_str() == Some(expected))
                })
        });
    if supports {
        Ok(identity)
    } else {
        Err(Failure::unavailable(
            "solar_prepare_schema_unavailable",
            "the installed ds-solar does not advertise the governed preparation schemas",
        )
        .remedy("install matching ds and ds-solar releases"))
    }
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
    .remedy("choose a safe writable fresh --out directory and install matching ds and ds-solar releases")
}

pub fn render(value: &Value) -> String {
    format!(
        "Governed Solar input prepared for {}.\nPrepared directory: {}\nArtifacts: 2\nNetwork: no",
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
