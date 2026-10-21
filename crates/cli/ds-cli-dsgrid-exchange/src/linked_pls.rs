//! Linked PLS provenance for this unregistered exchange crate only.
//!
//! The Core `dsgrid` domain reads working copies and PLS provenance through
//! the Network addon (DECOUPLING-003). `dsgrid-exchange sync` is not yet
//! restored through that port, so it keeps the former in-process reads here,
//! beside its only consumer, until its own process operation lands. Nothing
//! registered in `ds` reaches this module.

use std::path::{Path, PathBuf};

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_dsgrid::model::pls_source::{MODEL_NOT_FROM_PLS, WORKSPACE_NOT_FOUND};
use ds_command_kernel::local_models::LocalModel;
use ds_grid_exchange::conversion::{PlsPackageSource, dsgrid_package_pls_source};
use ds_grid_exchange::package::{GridPackage, unpack};
use serde_json::json;

/// The working copy's catalogue row and its decoded package.
pub(crate) struct OpenedModel {
    pub row: LocalModel,
    pub package: GridPackage,
}

pub(crate) fn open(inputs: &Inputs, id: &str) -> Result<OpenedModel, Failure> {
    let held = ds_cli_dsgrid::model::pls_source::open(inputs, id)?;
    let package = unpack(&held.package_bytes).map_err(|error| {
        Failure::invalid("not_a_dsgrid_package", error.to_string())
            .remedy("the store holds bytes this build's engine cannot open; re-import the package")
    })?;
    Ok(OpenedModel {
        row: held.row,
        package,
    })
}

pub(crate) fn pls_source(id: &str, package: &GridPackage) -> Result<PlsPackageSource, Failure> {
    dsgrid_package_pls_source(package)
        .map_err(|detail| {
            Failure::failed(
                "not_a_dsgrid_package",
                format!("the package of `{id}` carries unreadable PLS provenance"),
            )
            .detail(json!({ "detail": detail }))
        })?
        .ok_or_else(|| {
            Failure::invalid(
                "model_not_from_pls",
                format!("`{id}` was not imported from a PLS-CADD workspace"),
            )
            .remedy(MODEL_NOT_FROM_PLS.remedy)
        })
}

/// A workspace folder read the way `inspect` reads it; the sync plan checks
/// its members against the link's pinned digest.
pub(crate) struct ReadWorkspace {
    pub path: PathBuf,
    pub members: Vec<(String, Vec<u8>)>,
    pub streamed_volume: Option<String>,
}

pub(crate) fn read_workspace(raw: &str) -> Result<ReadWorkspace, Failure> {
    let path = Path::new(raw);
    if !path.is_dir() {
        return Err(
            Failure::invalid("workspace_not_found", format!("`{raw}` is not a directory"))
                .remedy(WORKSPACE_NOT_FOUND.remedy),
        );
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut byte_len = 0u64;
    let mut file_count = 0usize;
    let members = ds_cli_dsgrid::folder::read_folder(&absolute, &mut byte_len, &mut file_count)?;
    Ok(ReadWorkspace {
        streamed_volume: ds_cli_dsgrid::folder::streamed_volume_hint(&absolute),
        path: absolute,
        members,
    })
}
