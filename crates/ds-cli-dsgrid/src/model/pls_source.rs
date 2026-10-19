//! The PLS-CADD side of a working copy: the package it holds, the source
//! tree that package preserved, and the workspace folder it is linked to.
//!
//! `ds dsgrid model link` pins a folder to a working copy; `ds dsgrid
//! model show|list` print the pin; `ds dsgrid-exchange sync` writes into
//! the pinned folder. All of them open the same package from the same store
//! and read the same folder with the same walk, so the digest one prints is
//! the digest the others check. That is what this module holds; the exchange
//! digest, the program and member versions and the package's preserved origin
//! are read by the Network addon, and nothing here decides what a sync writes.

use std::path::{Path, PathBuf};

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Refusal;
use ds_command_kernel::local_models::{LocalModel, PlsSourceLink};
use ds_network_contracts::grid_operation::{
    GridOperationV1, PackagePlsSourceResultV1, PlsOriginV1, PlsWorkspaceFactsResultV1,
};
use serde_json::{Value, json};

use crate::folder;
use crate::model::workspace;
use crate::model::workspace::{STORE_UNAVAILABLE, UNKNOWN_MODEL};

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
    remedy: "re-import the workspace (`ds dsgrid-exchange convert`, then `ds dsgrid model import-external`) and re-link it with `ds dsgrid model link`",
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

/// The working copy's catalogue row and its package bytes, read once.
pub struct OpenedModel {
    pub row: LocalModel,
    pub package_bytes: Vec<u8>,
    pub package_path: PathBuf,
    pub active: Option<String>,
}

/// Open one working copy by id: the row from this machine's catalogue and
/// the package bytes from its store. The addon decodes them when asked.
pub fn open(inputs: &Inputs, id: &str) -> Result<OpenedModel, Failure> {
    let catalogue = workspace::read(inputs)?;
    let row = catalogue
        .models
        .iter()
        .find(|model| model.id == id)
        .cloned()
        .ok_or_else(|| {
            Failure::invalid(
                "local_model_not_found",
                format!("no working copy on this machine carries the id `{id}`"),
            )
            .remedy(UNKNOWN_MODEL.remedy)
            .next("ds dsgrid model list")
        })?;
    let root = ds_layer_store::local_models::default_root().map_err(|error| {
        Failure::unavailable("local_model_store_unavailable", error)
            .remedy(STORE_UNAVAILABLE.remedy)
    })?;
    let dir = ds_layer_store::local_models::scope_dir(&root, &workspace::scope(inputs)?)
        .map_err(workspace::refuse)?;
    let package_path =
        ds_layer_store::local_models::package_path(&dir, &row.id).map_err(workspace::refuse)?;
    let package_bytes = std::fs::read(&package_path).map_err(|error| {
        Failure::unavailable(
            "local_model_store_unavailable",
            format!(
                "the package of `{id}` cannot be read at {}: {error}",
                package_path.display()
            ),
        )
        .remedy(STORE_UNAVAILABLE.remedy)
    })?;
    Ok(OpenedModel {
        row,
        package_bytes,
        package_path,
        active: catalogue.active,
    })
}

/// The preserved PLS-CADD origin, or `None` for a package authored in DS
/// Grid or converted from another source.
pub fn package_origin(
    client: &ds_network_client::CapturedNetworkClient,
    id: &str,
    package_bytes: &[u8],
) -> Result<Option<PlsOriginV1>, Failure> {
    let outcome = crate::native::operation_with(
        client,
        &GridOperationV1::PackagePlsSource,
        Some(package_bytes),
        &[],
    )
    .map_err(|failure| {
        if failure.code() == "not_a_dsgrid_package" && failure.detail_value().is_some() {
            failure.with_message(format!(
                "the package of `{id}` carries unreadable PLS provenance"
            ))
        } else {
            failure
        }
    })?;
    let source: PackagePlsSourceResultV1 = serde_json::from_value(outcome.result.result)
        .map_err(|error| Failure::failed("network_result_invalid", error.to_string()))?;
    Ok(source.source)
}

/// A workspace folder read the way `inspect` reads it, with the exchange
/// identity the addon derives from its members.
pub struct ReadWorkspace {
    pub path: PathBuf,
    pub members: Vec<(String, Vec<u8>)>,
    pub digest: String,
    pub pls_version: String,
    pub member_versions: std::collections::BTreeMap<String, String>,
    pub streamed_volume: Option<String>,
}

pub fn read_workspace(
    client: &ds_network_client::CapturedNetworkClient,
    raw: &str,
) -> Result<ReadWorkspace, Failure> {
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
    let members = folder::read_folder(&absolute, &mut byte_len, &mut file_count)?;
    let outcome =
        crate::native::operation_with(client, &GridOperationV1::PlsWorkspaceFacts, None, &members)?;
    let facts: PlsWorkspaceFactsResultV1 = serde_json::from_value(outcome.result.result)
        .map_err(|error| Failure::failed("network_result_invalid", error.to_string()))?;
    if facts.member_count != members.len() as u64 {
        return Err(Failure::failed(
            "network_result_invalid",
            "the workspace facts describe another member set",
        ));
    }
    Ok(ReadWorkspace {
        streamed_volume: folder::streamed_volume_hint(&absolute),
        path: absolute,
        members,
        digest: facts.digest,
        pls_version: facts.pls_version,
        member_versions: facts.member_versions,
    })
}

/// Build the link record for a folder that digests to the package's origin.
pub fn link_for(workspace: &ReadWorkspace) -> PlsSourceLink {
    PlsSourceLink {
        path: workspace.path.to_string_lossy().into_owned(),
        digest: workspace.digest.clone(),
        pls_version: workspace.pls_version.clone(),
        member_versions: workspace.member_versions.clone(),
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
