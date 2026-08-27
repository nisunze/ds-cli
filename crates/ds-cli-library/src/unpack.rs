use crate::{engine_failure, read, sha256};
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::unpack_library;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub static COMMAND: Command = Command {
    id: "library.unpack",
    path: &["library", "unpack"],
    contract: 1,
    summary: "Verify and unpack one library release into a new local directory.",
    purpose: "Authenticates the complete release first, then materializes only its declared safe relative members. Existing paths are never overwritten.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("release", "<path>", "The .dsgrid-library release.").required(),
        Arg::value("out", "<dir>", "New output directory.").required(),
    ],
    output: "Output directory, immutable identity, content root and member count.",
    examples: &[Example {
        command: "ds library unpack --release ./standards.dsgrid-library --out ./unpacked --output json",
        note: "Verify before extracting.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "library_unpack_failed",
            when: "release verification or safe extraction fails",
            remedy: "use an untampered schema-v1 release",
        },
        Refusal {
            code: "output_exists",
            when: "the output directory already exists",
            remedy: "choose a new directory",
        },
    ],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let release_path = inputs.require("release")?;
    let out = PathBuf::from(inputs.require("out")?);
    if out.exists() {
        return Err(Failure::conflict(
            "output_exists",
            format!("`{}` already exists", out.display()),
        ));
    }
    let bytes = read(release_path)?;
    let release =
        unpack_library(&bytes).map_err(|error| engine_failure("library_unpack_failed", error))?;
    let declared = release
        .manifest
        .objects
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    fs::create_dir_all(&out)
        .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
    let extraction = (|| -> Result<(), Failure> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
            .map_err(|error| engine_failure("library_unpack_failed", error))?;
        for index in 0..archive.len() {
            let mut member = archive
                .by_index(index)
                .map_err(|error| engine_failure("library_unpack_failed", error))?;
            if member.name() == "manifest.json" {
                continue;
            }
            if !declared.contains(member.name()) || member.enclosed_name().is_none() {
                return Err(Failure::failed(
                    "library_unpack_failed",
                    "release contains an undeclared or unsafe member",
                ));
            }
            let target = out.join(Path::new(member.name()));
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
            }
            let mut body = Vec::new();
            member
                .read_to_end(&mut body)
                .map_err(|error| Failure::failed("library_unpack_failed", error.to_string()))?;
            fs::write(target, body)
                .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
        }
        fs::write(
            out.join("manifest.json"),
            serde_json::to_vec_pretty(&release.manifest).unwrap(),
        )
        .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
        Ok(())
    })();
    if let Err(error) = extraction {
        let _ = fs::remove_dir_all(&out);
        return Err(error);
    }
    Ok(json!({
        "written": out,
        "artifact_id": release.manifest.artifact_id,
        "version": release.manifest.revision_id,
        "content_root_digest": release.manifest.content_root_digest,
        "member_count": declared.len() + 1,
        "execution_owner": "ds",
        "deterministic_completion": "the verified release was extracted to a previously absent directory using only declared safe members",
        "pls_cadd_ui_handoff": { "required": false, "condition": "Extraction never opens PLS-CADD; only native solver/check or explicit visual acceptance requires it.", "artifact": release_path, "digest": format!("sha256:{}", sha256(&bytes)), "post_ui_reimport": "Re-import any native-saved workspace as a new authority candidate." },
        "engineer_decision": "Engineer decides whether extracted content is authoritative for the intended use."
    }))
}
pub fn render(data: &Value) -> String {
    format!(
        "unpacked {}/{} -> {}",
        data["artifact_id"],
        data["version"],
        data["written"].as_str().unwrap_or("")
    )
}
