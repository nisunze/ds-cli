//! Create-new local artifact writes shared by native Fast-LV commands.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use ds_cli_contract::Failure;
use sha2::{Digest, Sha256};

static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

pub(super) struct ArtifactContract {
    pub name: &'static str,
    pub stage_tag: &'static str,
    pub exists_code: &'static str,
    pub write_code: &'static str,
    pub exists_remedy: &'static str,
    pub write_remedy: &'static str,
}

pub(super) const RESULT: ArtifactContract = ArtifactContract {
    name: "Fast LV result",
    stage_tag: "ds-fast-lv-result",
    exists_code: "fast_lv_output_exists",
    write_code: "fast_lv_output_write_failed",
    exists_remedy: "Choose a new --out path; existing results are never overwritten.",
    write_remedy: "Choose a writable absent path and retry from the unchanged input.",
};

pub(super) const PROJECT_REQUEST: ArtifactContract = ArtifactContract {
    name: "Fast LV project request",
    stage_tag: "ds-fast-lv-project-request",
    exists_code: "fast_lv_request_output_exists",
    write_code: "fast_lv_request_output_write_failed",
    exists_remedy: "Choose a new --out path; existing requests are never overwritten.",
    write_remedy: "Choose a writable absent path and retry the unchanged transformer export.",
};

pub(super) fn ensure_absent(path: &Path, contract: &ArtifactContract) -> Result<(), Failure> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            return Err(Failure::conflict(
                contract.exists_code,
                format!("The {} path already exists.", contract.name),
            )
            .remedy(contract.exists_remedy));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Failure::failed(
                contract.write_code,
                format!("Could not inspect the {} path: {error}", contract.name),
            )
            .remedy(contract.write_remedy));
        }
    }
    Ok(())
}

pub(super) fn write_new(
    path: &Path,
    bytes: &[u8],
    contract: &ArtifactContract,
) -> Result<(), Failure> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = path.file_name().ok_or_else(|| {
        Failure::failed(
            contract.write_code,
            format!("The {} path has no file name.", contract.name),
        )
        .remedy(contract.write_remedy)
    })?;
    let stage = parent.join(format!(
        ".{}.{}-{}-{}.tmp",
        file_name.to_string_lossy(),
        contract.stage_tag,
        std::process::id(),
        NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
    ));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::hard_link(&stage, path)?;
        // The hard link is already the complete immutable artifact. A failed
        // stage cleanup is not permission to report failure after publishing
        // it (and a retry would honestly find `--out` occupied).
        let _ = std::fs::remove_file(&stage);
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&stage);
        if std::fs::symlink_metadata(path).is_ok() {
            return Err(Failure::conflict(
                contract.exists_code,
                format!("The {} path already exists.", contract.name),
            )
            .remedy(contract.exists_remedy));
        }
        return Err(Failure::failed(
            contract.write_code,
            format!("Could not write the {}: {error}", contract.name),
        )
        .remedy(contract.write_remedy));
    }
    Ok(())
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
