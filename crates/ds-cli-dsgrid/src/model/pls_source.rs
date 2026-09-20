//! The PLS-CADD side of a working copy: the package it holds, the source
//! tree that package preserved, and the workspace folder it is linked to.
//!
//! `ds dsgrid model link` pins a folder to a working copy; `ds dsgrid
//! model show|list` print the pin; `ds dsgrid-exchange sync` writes into
//! the pinned folder. All of them open the same package from the same store
//! and read the same folder with the same walk, so the digest one prints is
//! the digest the others check. That is what this module holds — nothing
//! here decides what a sync writes; `ds_grid_exchange::pls_cadd_workspace_sync`
//! does.

use std::path::{Path, PathBuf};

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Refusal;
use ds_command_kernel::local_models::{LocalModel, PlsSourceLink};
use ds_grid_exchange::conversion::{PlsPackageSource, SourceCandidate, dsgrid_package_pls_source};
use ds_grid_exchange::package::{GridPackage, unpack};
use ds_grid_exchange::pls_cadd_workspace_sync::{declared_program_version, member_versions};
use serde_json::{Value, json};

use crate::folder;
use crate::model::workspace;

pub const WORKSPACE_NOT_FOUND: Refusal = Refusal {
    code: "workspace_not_found",
    when: "--workspace (or --into) is not a directory",
    remedy: "pass the PLS-CADD workspace folder that holds the .don",
};
pub const WORKSPACE_NOT_LINKED: Refusal = Refusal {
    code: "workspace_not_linked",
    when: "the working copy carries no pls_source link",
    remedy: "run `ds dsgrid model link`, or import the package from beside its exchange-report.json",
};
pub const WORKSPACE_DIGEST_MOVED: Refusal = Refusal {
    code: "workspace_digest_moved",
    when: "the workspace's bytes no longer digest to what the link pinned (PLS-CADD saved since)",
    remedy: "re-import and re-link; `ds dsgrid-exchange pull` (contract 02 §3, not landed) will re-read it in place",
};
pub const WORKSPACE_NOT_THIS_PACKAGE: Refusal = Refusal {
    code: "workspace_not_this_package",
    when: "the folder does not digest to the workspace this working copy was imported from",
    remedy: "link the folder the package was converted from, or re-import this folder as a new working copy",
};
pub const MODEL_NOT_FROM_PLS: Refusal = Refusal {
    code: "model_not_from_pls",
    when: "the working copy was not imported from a PLS-CADD workspace or backup",
    remedy: "only a copy converted from PLS-CADD links or syncs; export a DS-authored model with `convert --target pls-folder`",
};

/// The working copy's catalogue row and its package, opened once.
pub struct OpenedModel {
    pub row: LocalModel,
    pub package: GridPackage,
    pub package_bytes: Vec<u8>,
    pub package_path: PathBuf,
    pub active: Option<String>,
}

/// Open one working copy by id: the row from this machine's catalogue, the
/// package from its store, verified by the engine.
pub fn open(inputs: &Inputs, id: &str) -> Result<OpenedModel, Failure> {
    let catalogue = workspace::read(inputs)?;
    let row = catalogue
        .models
        .iter()
        .find(|model| model.id == id)
        .cloned()
        .ok_or_else(|| {
            Failure::invalid(
                workspace::UNKNOWN_MODEL.code,
                format!("no working copy on this machine carries the id `{id}`"),
            )
            .remedy(workspace::UNKNOWN_MODEL.remedy)
            .next("ds dsgrid model list")
        })?;
    let root = ds_layer_store::local_models::default_root().map_err(|error| {
        Failure::unavailable(workspace::STORE_UNAVAILABLE.code, error)
            .remedy(workspace::STORE_UNAVAILABLE.remedy)
    })?;
    let dir = ds_layer_store::local_models::scope_dir(&root, &workspace::scope(inputs)?)
        .map_err(workspace::refuse)?;
    let package_path =
        ds_layer_store::local_models::package_path(&dir, &row.id).map_err(workspace::refuse)?;
    let package_bytes = std::fs::read(&package_path).map_err(|error| {
        Failure::unavailable(
            workspace::STORE_UNAVAILABLE.code,
            format!(
                "the package of `{id}` cannot be read at {}: {error}",
                package_path.display()
            ),
        )
        .remedy(workspace::STORE_UNAVAILABLE.remedy)
    })?;
    let package = unpack(&package_bytes).map_err(|error| {
        Failure::invalid("not_a_dsgrid_package", error.to_string())
            .remedy("the store holds bytes this build's engine cannot open; re-import the package")
    })?;
    Ok(OpenedModel {
        row,
        package,
        package_bytes,
        package_path,
        active: catalogue.active,
    })
}

/// The PLS provenance the package carries, or the named refusal when it
/// was not imported from PLS-CADD.
pub fn pls_source(id: &str, package: &GridPackage) -> Result<PlsPackageSource, Failure> {
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
                MODEL_NOT_FROM_PLS.code,
                format!("`{id}` was not imported from a PLS-CADD workspace"),
            )
            .remedy(MODEL_NOT_FROM_PLS.remedy)
        })
}

/// A workspace folder read the way `inspect` reads it, with its digest.
pub struct ReadWorkspace {
    pub path: PathBuf,
    pub members: Vec<(String, Vec<u8>)>,
    pub digest: String,
    pub streamed_volume: Option<String>,
}

pub fn read_workspace(raw: &str) -> Result<ReadWorkspace, Failure> {
    let path = Path::new(raw);
    if !path.is_dir() {
        return Err(Failure::invalid(
            WORKSPACE_NOT_FOUND.code,
            format!("`{raw}` is not a directory"),
        )
        .remedy(WORKSPACE_NOT_FOUND.remedy));
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
    let members = folder::read_folder(&absolute, &mut byte_len, &mut file_count)?;
    let digest = SourceCandidate::folder("", members.clone()).digest();
    Ok(ReadWorkspace {
        streamed_volume: folder::streamed_volume_hint(&absolute),
        path: absolute,
        members,
        digest,
    })
}

/// Build the link record for a folder that digests to the package's origin.
pub fn link_for(workspace: &ReadWorkspace) -> PlsSourceLink {
    PlsSourceLink {
        path: workspace.path.to_string_lossy().into_owned(),
        digest: workspace.digest.clone(),
        pls_version: declared_program_version(&workspace.members),
        member_versions: member_versions(&workspace.members),
        member_count: workspace.members.len() as u64,
        // The store stamps its clock.
        linked_at: String::new(),
    }
}

/// The `pls_source` projection every row prints.
pub fn link_json(link: Option<&PlsSourceLink>) -> Value {
    match link {
        None => Value::Null,
        Some(link) => json!({
            "path": link.path,
            "digest": link.digest,
            "pls_version": link.pls_version,
            "member_versions": link.member_versions,
            "member_count": link.member_count,
            "linked_at": link.linked_at,
        }),
    }
}

/// One human line for a link: `DON 57 · CRI 94 · FEA 15 …` after the path.
pub fn link_line(link: &PlsSourceLink) -> String {
    let versions = link
        .member_versions
        .iter()
        .map(|(family, version)| format!("{family} {version}"))
        .collect::<Vec<_>>()
        .join(" · ");
    format!(
        "  linked     {}\n             PLS-CADD {} · {} members · {}\n             {}\n",
        link.path, link.pls_version, link.member_count, versions, link.digest
    )
}
