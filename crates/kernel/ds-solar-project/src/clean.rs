//! Clear derived local runs; authored inputs, parameters, maps and reference cache remain.
use crate::{Workspace, files, identifier};
use anyhow::{Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub workspace: PathBuf,
    pub project_id: String,
}

pub(crate) fn lock(path: &Path, exclusive: bool) -> Result<std::fs::File> {
    let path = path.join("compute.lock");
    if std::fs::symlink_metadata(&path).is_ok() {
        files::ordinary_file(&path)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    let acquired = if exclusive {
        file.try_lock_exclusive()
    } else {
        FileExt::try_lock_shared(&file)
    };
    acquired.map_err(|_| {
        anyhow::anyhow!("Solar computation is active; wait for it to finish before cleaning")
    })?;
    Ok(file)
}

pub fn execute(control: Control) -> Result<Value> {
    ensure!(
        identifier(&control.project_id),
        "invalid explicit Solar project"
    );
    let mut workspace = Workspace::open(&control.workspace)?;
    ensure!(
        workspace.identity.project_id == control.project_id,
        "cleanup belongs to another Solar project"
    );
    let _lock = lock(&workspace.path, true)?;
    let runs = workspace.path.join("runs");
    if runs.exists() {
        files::ordinary_dir(&runs)?;
        std::fs::remove_dir_all(&runs)?;
    }
    files::private_dir(&runs)?;
    let tx = workspace.db.transaction()?;
    tx.execute_batch("DELETE FROM runs; DELETE FROM outbox WHERE kind='run'; DELETE FROM meta WHERE key LIKE 'published:%'; DROP INDEX IF EXISTS run_upload;")?;
    tx.commit()?;
    Ok(
        json!({"schema":"ds.solar-local-clean/v1","project_id":control.project_id,"cleaned":true,"recompute_required":true}),
    )
}
