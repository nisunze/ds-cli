//! Explicit retained-byte copy. Selecting a local producer and selecting an
//! acquired shared publication are different sources; neither falls back.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
};

pub static COMMAND: Command = Command {
    id: "report.inventory.copy",
    path: &["report", "inventory", "copy"],
    contract: 1,
    summary: "Copy exact already retained report bytes without network or generation.",
    purpose: "Selects committed local producer bytes or a verified Sync download explicitly. Checks the native account/project/immutable output pins, exact retained head for downloaded bytes and physical SHA/size before returning a fresh private destination. No publication, cloud call, fallback or generation.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        crate::project::PROJECT_ARG,
        crate::project::LANE_ARG,
        Arg::value("transformer", "<key>", "Exact report transformer.").required(),
        Arg::value("source", "<kind>", "Explicit retained source.")
            .required()
            .choices(&["committed", "sync_download"]),
        Arg::value("output-id", "<id>", "Exact selected output id.").required(),
        Arg::value("sha256", "<hex>", "Exact selected output SHA-256.").required(),
        Arg::value(
            "size-bytes",
            "<positive-integer>",
            "Exact selected byte count.",
        )
        .required(),
        Arg::value(
            "head-revision",
            "<positive-integer>",
            "Required immutable revision for source sync_download; omitted for committed.",
        ),
        Arg::value(
            "out",
            "<absolute-path>",
            "Fresh private destination in an existing directory.",
        )
        .required(),
        Arg::value(
            "core-state-dir",
            "<absolute-path>",
            "Same state root as installed Core if overridden.",
        ),
    ],
    output: "ds.report-copy/v1: source, head_revision (null for committed), project, transformer, lane, output_id, path, sha256, size_bytes, filename, content_type.",
    examples: &[],
    refusals: crate::outbox::INVENTORY.refusals,
    reference: Some("docs/reference/report.md"),
    search: &["offline", "download", "retained", "local report"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn failed(e: impl std::fmt::Display) -> Failure {
    Failure::failed("report_inventory_unreadable", e.to_string())
}

pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = i.require("lane")?;
    let project = i.require("project")?;
    let transformer = i.require("transformer")?;
    let source = i.require("source")?;
    let id = i.require("output-id")?;
    let sha = i.require("sha256")?;
    let size = i.require("size-bytes")?.parse::<u64>().map_err(failed)?;
    let out = Path::new(i.require("out")?);
    if !out.is_absolute()
        || out.exists()
        || !out.parent().is_some_and(Path::is_dir)
        || !ds_command_kernel::report_export::exact_lower_sha256(sha)
        || size == 0
    {
        return Err(failed(
            "Use exact byte pins and a fresh absolute destination.",
        ));
    }
    let fence = crate::inventory::fence(lane)?;
    let mut snapshot = crate::outbox::inventory_snapshot(i)?;
    if snapshot.owner != fence.account {
        return Err(failed("Account changed while selecting retained bytes."));
    }
    let state = crate::outbox::server_state(i)?;
    let (file, filename, content_type) = match source {
        "committed" => {
            if i.value("head-revision").is_some() {
                return Err(failed("Committed copies do not select a cloud revision."));
            }
            let view = ds_command_kernel::report::inventory::project(&snapshot).map_err(failed)?;
            let artifact = view
                .outputs
                .iter()
                .filter_map(|o| o.local)
                .find(|a| a.output_id == id && a.sha256 == sha && a.size_bytes == size)
                .ok_or_else(|| failed("The exact committed output is unavailable."))?;
            let authority = ds_report_artifacts::ArtifactReadAuthority {
                owner_uid: snapshot.owner.clone(),
                project_id: project.into(),
                operation: format!("export-{transformer}"),
                variant: snapshot.variant.clone(),
                output_id: id.into(),
                sha256: sha.into(),
                size_bytes: size,
                content_type: artifact.content_type.clone(),
            };
            let opened = ds_report_artifacts::open_artifact_from_root_with_gate(
                &state.join("report-artifacts"),
                &artifact.locator,
                |receipt| authority.authorize(receipt, &artifact.locator),
            )
            .map_err(failed)?;
            (
                opened.file,
                artifact.filename.clone(),
                artifact.content_type.clone(),
            )
        }
        "sync_download" => {
            let revision = i.require("head-revision")?.parse::<u64>().map_err(failed)?;
            let store = ds_sync_store::Store::open_read_only(&state.join("store.sqlite"))
                .map_err(failed)?;
            crate::inventory::attach_cloud(&mut snapshot, store.as_ref(), &fence, false, None)?;
            let cloud = snapshot
                .cloud_head
                .as_ref()
                .ok_or_else(|| failed("No exact retained publication head."))?;
            if cloud.head.head_revision != revision {
                return Err(failed("The retained head revision changed."));
            }
            let declaration = cloud
                .head
                .outputs
                .iter()
                .find(|o| o.output_id == id && o.sha256 == sha && o.size_bytes == size)
                .ok_or_else(|| failed("The retained output differs from selected pins."))?;
            let root=ds_sync_runtime::VerifiedReads::new(state.join("sync-downloads")).retained_head(project,&cloud.head).map_err(failed)?.ok_or_else(||failed("The exact shared output has not been acquired, or its retained bytes failed verification."))?;
            let view = ds_command_kernel::report::inventory::project(&snapshot).map_err(failed)?;
            let output = view
                .outputs
                .iter()
                .find(|o| o.output_id == id)
                .ok_or_else(|| failed("Selected output is not in inventory."))?;
            (
                ds_report_artifacts::confined_fs::HeldDirectory::open_absolute(&root)
                    .map_err(failed)?
                    .ok_or_else(|| failed("Retained directory disappeared."))?
                    .open_regular_file(std::ffi::OsStr::new(id), "retained report output")
                    .map_err(failed)?,
                output.filename.clone(),
                declaration.content_type.clone(),
            )
        }
        _ => return Err(failed("Select one explicit retained source.")),
    };
    copy_verified(file, out, sha, size, &|| {
        if crate::inventory::fence(lane)? != fence {
            Err(failed("Account changed before retained output delivery."))
        } else {
            Ok(())
        }
    })?;
    Ok(
        json!({"schema":"ds.report-copy/v1","source":source,"head_revision":i.value("head-revision").and_then(|v|v.parse::<u64>().ok()),"project":project,"transformer":transformer,"lane":lane,"output_id":id,"path":out,"sha256":sha,"size_bytes":size,"filename":filename,"content_type":content_type}),
    )
}

pub(crate) fn copy_verified(
    mut file: std::fs::File,
    out: &Path,
    sha: &str,
    size: u64,
    guard: &dyn Fn() -> Result<(), Failure>,
) -> Result<(), Failure> {
    guard()?;
    let mut target = crate::inventory::create_private(out)?;
    let result = (|| {
        let mut digest = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buffer).map_err(failed)?;
            if n == 0 {
                break;
            }
            count = count
                .checked_add(n as u64)
                .ok_or_else(|| failed("Output size overflow."))?;
            if count > size {
                return Err(failed("Retained output grew during copy."));
            }
            digest.update(&buffer[..n]);
            target.write_all(&buffer[..n]).map_err(failed)?;
        }
        if count != size || format!("{:x}", digest.finalize()) != sha {
            return Err(failed("Retained bytes differ from selected SHA/size."));
        }
        target.sync_all().map_err(failed)?;
        guard()?;
        Ok::<_, Failure>(())
    })();
    drop(target);
    if let Err(e) = result {
        let _ = std::fs::remove_file(out);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_copy_survives_and_bad_pins_or_late_identity_leave_no_output() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::write(&source, b"immutable report").unwrap();
        let sha = format!("{:x}", Sha256::digest(b"immutable report"));
        let target = dir.path().join("good");
        copy_verified(
            std::fs::File::open(&source).unwrap(),
            &target,
            &sha,
            16,
            &|| Ok(()),
        )
        .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"immutable report");
        assert!(
            copy_verified(
                std::fs::File::open(&source).unwrap(),
                &target,
                &sha,
                16,
                &|| Ok(())
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"immutable report");
        for (label, digest, size) in [
            ("bad_sha", "0".repeat(64), 16),
            ("short", sha.clone(), 15),
            ("long", sha.clone(), 17),
        ] {
            let target = dir.path().join(label);
            assert!(
                copy_verified(
                    std::fs::File::open(&source).unwrap(),
                    &target,
                    &digest,
                    size,
                    &|| Ok(())
                )
                .is_err()
            );
            assert!(!target.exists());
        }
        let target = dir.path().join("moved");
        let calls = std::cell::Cell::new(0);
        let guard = || {
            calls.set(calls.get() + 1);
            if calls.get() > 1 {
                Err(failed("account changed"))
            } else {
                Ok(())
            }
        };
        assert!(
            copy_verified(
                std::fs::File::open(&source).unwrap(),
                &target,
                &sha,
                16,
                &guard
            )
            .is_err()
        );
        assert!(!target.exists());
    }
}
