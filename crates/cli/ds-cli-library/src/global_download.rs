//! Exact-byte export of governed global artifacts and indexed native members.
//! This does not publish, migrate a model format, or retire catalog data.
use std::{fs::OpenOptions, io::Write, path::Path};

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_client_core::grid_catalog::ArtifactSelection;
use serde_json::{Value, json};

const LOCAL: Refusal = Refusal {
    code: "catalog_output_invalid",
    when: "the selection is inconsistent or the output already exists or cannot be written",
    remedy: "name one exact artifact coordinate and digest, and use a fresh output path",
};
const INTEGRITY: Refusal = Refusal {
    code: "catalog_artifact_unverified",
    when: "the exact catalog coordinates, locator, byte count or SHA-256 could not be verified",
    remedy: "read the exact release/revision pins; preserve existing bytes and do not retire data until export verifies",
};
const REFUSALS: [Refusal; 19] = super::global_catalog::with_native([LOCAL, INTEGRITY]);

pub static COMMAND: Command = Command {
    id: "library.global.download",
    path: &["library", "global", "download"],
    contract: 1,
    summary: "Export one exact global catalog artifact with verified bytes.",
    purpose: "Download one pinned library manifest, validation report, indexed asset or example model to a fresh file. Indexed members require their exact inventory path, digest and signed storage generation. Reuses the catalog's authenticated exact read and signed content-addressed transfer. Never selects a mutable head, accepts a URL, converts formats, copies ownership or deletes data. A model artifact is not proof of all separately held native members.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg {
            name: "kind",
            kind: ds_cli_contract::spec::ArgKind::Value,
            value: "<kind>",
            required: true,
            default: None,
            choices: &[
                "library-manifest",
                "library-validation",
                "library-asset",
                "example-model",
            ],
            summary: "Exact artifact class.",
        },
        Arg::value("library-id", "<id>", "Library id for a library artifact."),
        Arg::value(
            "release-id",
            "<id>",
            "Exact immutable library release; required with library-id.",
        ),
        Arg::value(
            "relative-path",
            "<path>",
            "Exact inventory path; required only for library-asset.",
        ),
        Arg::value("example-id", "<id>", "Example id for an example model."),
        Arg::value(
            "revision-id",
            "<id>",
            "Exact immutable example revision; required with example-id.",
        ),
        Arg::value(
            "expected-digest",
            "<sha256>",
            "Exact artifact SHA-256: 64 lowercase hexadecimal characters.",
        )
        .required(),
        Arg::value("out", "<path>", "Fresh local file; never overwritten.").required(),
        ds_cli_contract::spec::LANE,
    ],
    output: "Exact immutable coordinates, SHA-256, byte count, verified=true and local output path; no signed delivery URL or bytes in JSON.",
    examples: &[],
    refusals: &REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: &["backup", "native"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid() -> Failure {
    Failure::invalid(
        LOCAL.code,
        "name a single exact artifact selection and fresh output path",
    )
    .remedy(LOCAL.remedy)
}

fn selection(inputs: &Inputs) -> Result<ArtifactSelection, Failure> {
    let library = inputs.value("library-id");
    let release = inputs.value("release-id");
    let example = inputs.value("example-id");
    let revision = inputs.value("revision-id");
    let relative_path = inputs.value("relative-path");
    match (
        inputs.require("kind")?,
        library,
        release,
        example,
        revision,
        relative_path,
    ) {
        ("library-manifest", Some(library), Some(release), None, None, None) => {
            Ok(ArtifactSelection::LibraryManifest {
                library: library.into(),
                release: release.into(),
            })
        }
        ("library-validation", Some(library), Some(release), None, None, None) => {
            Ok(ArtifactSelection::LibraryValidation {
                library: library.into(),
                release: release.into(),
            })
        }
        ("example-model", None, None, Some(example), Some(revision), None) => {
            Ok(ArtifactSelection::ExampleModel {
                example: example.into(),
                revision: revision.into(),
            })
        }
        ("library-asset", Some(library), Some(release), None, None, Some(relative_path)) => {
            Ok(ArtifactSelection::LibraryAsset {
                library: library.into(),
                release: release.into(),
                relative_path: relative_path.into(),
                expected_digest: inputs.require("expected-digest")?.into(),
            })
        }
        _ => Err(invalid()),
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| invalid())?;
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
        return Err(invalid());
    }
    Ok(())
}

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let out = Path::new(inputs.require("out")?);
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(invalid());
    }
    let selection = selection(inputs)?;
    let digest = inputs.require("expected-digest")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let mut receipt =
        ds_cli_auth::grid_catalog_artifact(inputs.require("lane")?, &selection, digest)?;
    let bytes = receipt.bytes.take().ok_or_else(invalid)?;
    write_new(out, &bytes)?;
    receipt.data["out"] = json!(out);
    Ok(receipt.data)
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_or_unpinned_selection_cannot_reach_the_catalog() {
        for extra in [
            vec!["--kind", "example-model", "--example-id", "example_1"],
            vec![
                "--kind",
                "example-model",
                "--example-id",
                "example_1",
                "--revision-id",
                "revision_1",
                "--library-id",
                "library_1",
            ],
            vec!["--kind", "library-manifest", "--library-id", "library_1"],
            vec![
                "--kind",
                "library-validation",
                "--library-id",
                "library_1",
                "--release-id",
                "release_1",
                "--revision-id",
                "revision_1",
            ],
        ] {
            let mut tokens = extra.into_iter().map(str::to_owned).collect::<Vec<_>>();
            tokens.extend([
                "--out".into(),
                "fresh.bin".into(),
                "--expected-digest".into(),
                "a".repeat(64),
            ]);
            let inputs = ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap();
            assert!(selection(&inputs).is_err());
        }
    }

    #[test]
    fn indexed_asset_requires_exact_inventory_path_and_preserves_all_pins() {
        let mut tokens = vec![
            "--kind",
            "library-asset",
            "--library-id",
            "library_1",
            "--release-id",
            "release_1",
            "--expected-digest",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--out",
            "fresh.bin",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        assert!(selection(&ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap()).is_err());
        tokens.extend(["--relative-path".into(), "native/pole.012".into()]);
        let selected =
            selection(&ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap()).unwrap();
        assert!(
            matches!(selected.read_command(), ds_client_core::grid_catalog::Command::ResolveLibraryMember { library_id, release_id, relative_path, expected_digest } if library_id == "library_1" && release_id == "release_1" && relative_path == "native/pole.012" && expected_digest == "a".repeat(64))
        );
        tokens[1] = "library-manifest".into();
        assert!(selection(&ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap()).is_err());
    }

    #[test]
    fn verified_output_never_overwrites_an_existing_file_or_symlink() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../out/library-download-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("exact.bin");
        write_new(&file, b"original\r\n").unwrap();
        assert!(write_new(&file, b"replacement").is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"original\r\n");
        #[cfg(unix)]
        {
            let link = root.join("link.bin");
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert!(write_new(&link, b"replacement").is_err());
            assert_eq!(std::fs::read(&file).unwrap(), b"original\r\n");
            let dangling = root.join("dangling.bin");
            let missing = root.join("missing.bin");
            std::os::unix::fs::symlink(&missing, &dangling).unwrap();
            assert!(write_new(&dangling, b"replacement").is_err());
            assert!(!missing.exists());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
