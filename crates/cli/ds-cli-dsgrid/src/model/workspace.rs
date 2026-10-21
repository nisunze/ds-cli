//! This machine's DS Grid working copies, without an application.
//!
//! The catalogue and its packages are the machine's own: a lane, a DS account,
//! and a directory beside the local-layer catalogue `ds map local …` already
//! keeps. `ds_command_kernel::local_models` decides everything about a working
//! copy; `ds_layer_store::local_models` persists it; this module is only the
//! command surface's side of that — the two flags that name the catalogue, the
//! identity read off a package, and the refusals re-raised under the kernel's
//! own names.
//!
//! No sign-in, no project, no window. A working copy is a fact about a
//! machine, and a machine that has never held one answers with an empty
//! catalogue rather than a refusal.

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use ds_command_kernel::local_models::{Catalogue, LocalModel, Op, Origin, Outcome, Scope};
use ds_layer_store::local_models::StoreError;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const LANE_ARG: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::LANE.summary("Which lane's catalogue.");

pub const ACCOUNT_ARG: Arg = Arg::value(
    "account",
    "<uid>",
    "The DS account whose working copies these are; two people on one machine never share a catalogue.",
)
.required();

pub const UNKNOWN_MODEL: Refusal = Refusal {
    code: ds_command_kernel::local_models::UNKNOWN_MODEL,
    when: "no working copy on this machine carries that id",
    remedy: "run `ds dsgrid model list` and use an id from it",
};
pub const NAME_TAKEN: Refusal = Refusal {
    code: ds_command_kernel::local_models::DUPLICATE_NAME,
    when: "this machine already holds a working copy under that name",
    remedy: "choose a name you can tell apart from the one already held",
};
pub const CATALOGUE_FULL: Refusal = Refusal {
    code: ds_command_kernel::local_models::CATALOGUE_FULL,
    when: "this machine holds the most working copies a catalogue keeps",
    remedy: "forget a copy you have finished with, then retry",
};
pub const REQUEST_INVALID: Refusal = Refusal {
    code: ds_command_kernel::local_models::MALFORMED,
    when: "a name, coordinate system or identity is empty, untrimmed or too long",
    remedy: "pass a bounded trimmed name; the package supplies its own identity",
};
pub const SCOPE_MISMATCH: Refusal = Refusal {
    code: ds_command_kernel::local_models::SCOPE_MISMATCH,
    when: "the catalogue on this machine belongs to another lane or DS account",
    remedy: "pass the --lane and --account the catalogue was written under",
};
pub const STORE_UNAVAILABLE: Refusal = Refusal {
    code: "local_model_store_unavailable",
    when: "the machine's catalogue or a package cannot be read or written",
    remedy: "check the local data directory; DS_LAYER_HOME may name an absolute shared directory",
};

pub const DRAFT_PENDING: Refusal = Refusal {
    code: "local_model_draft_pending",
    when: "an old-format working copy has a pending draft that cannot be incidentally rebased during activation",
    remedy: "resolve the local draft through its owning workflow before activating the old-format copy",
};
pub const REVISION_CONFLICT: Refusal = Refusal {
    code: "local_model_revision_conflict",
    when: "the local catalogue digest differs from the exact package being activated",
    remedy: "read the working copy again and resolve its catalogue/package mismatch before activation",
};

/// Every refusal these commands can return: the kernel's closed vocabulary
/// plus this host's one IO failure. Composed, so a new kernel refusal is
/// documented the moment it exists.
const BASE_REFUSALS: &[Refusal] = &[
    crate::package::PROTECTED_MODEL,
    UNKNOWN_MODEL,
    NAME_TAKEN,
    CATALOGUE_FULL,
    REQUEST_INVALID,
    SCOPE_MISMATCH,
    STORE_UNAVAILABLE,
];
pub const REFUSALS: &[Refusal] = &crate::native::extend_refusals::<
    { BASE_REFUSALS.len() + crate::native::REFUSALS.len() },
>(BASE_REFUSALS);

/// Which catalogue this invocation is about.
pub fn scope(inputs: &Inputs) -> Result<Scope, Failure> {
    Ok(Scope {
        lane: inputs.require("lane")?.trim().to_owned(),
        uid: inputs.require("account")?.trim().to_owned(),
    })
}

fn root() -> Result<std::path::PathBuf, Failure> {
    ds_layer_store::local_models::default_root().map_err(|error| {
        Failure::unavailable(STORE_UNAVAILABLE.code, error).remedy(STORE_UNAVAILABLE.remedy)
    })
}

/// Re-raise the kernel's refusal under its own name.
///
/// The vocabulary is closed and shared with every host, so a caller that has
/// planned for `local_model_not_found` once has planned for it everywhere. The
/// codes are written out literally here rather than forwarded from the error,
/// because a forwarded code is one `refusal_coverage.rs` cannot read — and a
/// code no command documents is exactly what that suite exists to catch.
pub fn refuse(error: StoreError) -> Failure {
    let message = error.message().to_owned();
    match error.code() {
        ds_command_kernel::local_models::UNKNOWN_MODEL => {
            Failure::invalid("local_model_not_found", message).remedy(UNKNOWN_MODEL.remedy)
        }
        ds_command_kernel::local_models::DUPLICATE_NAME => {
            Failure::invalid("local_model_name_taken", message).remedy(NAME_TAKEN.remedy)
        }
        ds_command_kernel::local_models::MALFORMED => {
            Failure::invalid("local_model_request_invalid", message).remedy(REQUEST_INVALID.remedy)
        }
        ds_command_kernel::local_models::CATALOGUE_FULL => {
            Failure::conflict("local_model_catalogue_full", message).remedy(CATALOGUE_FULL.remedy)
        }
        ds_command_kernel::local_models::SCOPE_MISMATCH => {
            Failure::conflict("local_model_scope_mismatch", message).remedy(SCOPE_MISMATCH.remedy)
        }
        _ => Failure::unavailable("local_model_store_unavailable", message)
            .remedy(STORE_UNAVAILABLE.remedy),
    }
}

pub fn read(inputs: &Inputs) -> Result<Catalogue, Failure> {
    read_in(&scope(inputs)?)
}

/// Read one catalogue already scoped — by the flags, or by the restored
/// native credential's own lane and uid.
pub fn read_in(scope: &Scope) -> Result<Catalogue, Failure> {
    ds_layer_store::local_models::read_at(&root()?, scope).map_err(refuse)
}

pub fn execute(inputs: &Inputs, op: Op, package: Option<&[u8]>) -> Result<Outcome, Failure> {
    execute_in(&scope(inputs)?, op, package)
}

/// Apply one operation to a catalogue already located by [`locate`].
pub fn execute_in(scope: &Scope, op: Op, package: Option<&[u8]>) -> Result<Outcome, Failure> {
    execute_in_with_client(scope, op, package, None)
}

/// Creation already captures its owner. Catalogue guards reuse that handle;
/// other mutations capture lazily only if an opaque package requires admission.
pub(crate) fn execute_in_with_client(
    scope: &Scope,
    op: Op,
    package: Option<&[u8]>,
    client: Option<&ds_network_client::CapturedNetworkClient>,
) -> Result<Outcome, Failure> {
    let root = root()?;
    let mut captured = None;
    let guarded = match &op {
        // Acquiring a verified project revision is a read view, not model creation.
        Op::Register {
            origin: Origin::Project,
            ..
        } => None,
        Op::Register { .. } => Some((None, "create")),
        Op::Revise { id, .. } => Some((Some(id), "save")),
        Op::StageDraft { id, .. } => Some((Some(id), "save draft")),
        Op::Forget { id } => Some((Some(id), "delete")),
        Op::Link { id, .. } | Op::Unlink { id } => Some((Some(id), "link")),
        Op::SetActive { .. } => None,
    };
    if let Some((id, operation)) = guarded {
        if let Some(id) = id
            && ds_layer_store::local_models::read_at(&root, scope)
                .map_err(refuse)?
                .models
                .iter()
                .any(|row| &row.id == id)
        {
            let dir = ds_layer_store::local_models::scope_dir(&root, scope).map_err(refuse)?;
            let path = ds_layer_store::local_models::package_path(&dir, id).map_err(refuse)?;
            let bytes = crate::package::read_bytes(&path.to_string_lossy())?;
            // Catalogue metadata operations also govern opaque/historical
            // ordinary payloads with zero addon probes. Engineering admission
            // runs only for a declared combined identity.
            if crate::package::declared_model_id(&bytes)
                .is_some_and(|id| ds_network_contracts::grid::is_combined_identity(&id))
            {
                let owner = lazy_owner(client, &mut captured)?;
                crate::package::authorize_write_bytes(
                    owner,
                    &path.to_string_lossy(),
                    &bytes,
                    operation,
                )?;
            }
        }
        if let Some(bytes) = package {
            let owner = lazy_owner(client, &mut captured)?;
            crate::package::authorize_write_bytes(owner, "working copy", bytes, operation)?;
        }
    }
    ds_layer_store::local_models::execute_at(&root, scope, op, package).map_err(refuse)
}

/// The invocation's captured owner, or the lane's addon captured once.
fn lazy_owner<'a>(
    client: Option<&'a ds_network_client::CapturedNetworkClient>,
    captured: &'a mut Option<ds_network_client::CapturedNetworkClient>,
) -> Result<&'a ds_network_client::CapturedNetworkClient, Failure> {
    if let Some(client) = client {
        return Ok(client);
    }
    if captured.is_none() {
        *captured = Some(crate::native::capture()?);
    }
    Ok(captured.as_ref().expect("captured above"))
}

/// One working copy found on this machine: its catalogue, its row and the
/// package file beside it.
pub struct Located {
    pub scope: Scope,
    pub row: LocalModel,
    pub path: std::path::PathBuf,
}

/// Find a working copy by id.
///
/// With `--account` the catalogue is named and the lookup is exact. Without
/// it every catalogue of the lane on this machine is read: a local id is
/// minted random and unique, so it is found in one catalogue or none — and
/// the one case where two accounts on one machine hold the same id is a
/// refusal that names the remedy rather than a guess. This is what lets the
/// contract's script name a working copy by id alone.
pub fn locate(inputs: &Inputs, id: &str) -> Result<Located, Failure> {
    let root = root()?;
    let lane = inputs.value("lane").unwrap_or("stable").trim().to_owned();
    let account = inputs
        .value("account")
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let scopes: Vec<Scope> = match account {
        Some(uid) => vec![Scope {
            lane,
            uid: uid.to_owned(),
        }],
        None => {
            let lane_dir = root.join("models").join(&lane);
            let mut scopes = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&lane_dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        scopes.push(Scope {
                            lane: lane.clone(),
                            uid: entry.file_name().to_string_lossy().into_owned(),
                        });
                    }
                }
            }
            scopes.sort_by(|a, b| a.uid.cmp(&b.uid));
            scopes
        }
    };
    let mut found: Vec<(Scope, LocalModel)> = Vec::new();
    for scope in scopes {
        let catalogue = ds_layer_store::local_models::read_at(&root, &scope).map_err(refuse)?;
        if let Some(row) = catalogue.models.iter().find(|row| row.id == id) {
            found.push((scope, row.clone()));
        }
    }
    match found.len() {
        0 => Err(Failure::invalid(
            "local_model_not_found",
            format!("no working copy `{id}` on this machine"),
        )
        .remedy(UNKNOWN_MODEL.remedy)
        .next("ds dsgrid model list")),
        1 => {
            let (scope, row) = found.remove(0);
            let dir = ds_layer_store::local_models::scope_dir(&root, &scope).map_err(refuse)?;
            let path = ds_layer_store::local_models::package_path(&dir, &row.id).map_err(refuse)?;
            Ok(Located { scope, row, path })
        }
        _ => Err(Failure::invalid(
            "local_model_ambiguous",
            format!("`{id}` is held by {} accounts on this machine", found.len()),
        )
        .remedy("pass --account <uid> to say whose working copy you mean")
        .detail(json!({
            "accounts": found.iter().map(|(scope, _)| scope.uid.clone()).collect::<Vec<_>>(),
        }))),
    }
}

/// The identity a package declares, read by the engine that understands one.
pub struct PackageIdentity {
    pub crs: String,
    pub model_revision: u64,
    pub sha256: String,
    pub bytes: u64,
    /// The authored head the engine derives from the content (`rev:…`).
    pub authored_revision: String,
}

/// Read a package's own identity, refusing anything that is not one.
///
/// The CRS and the revision are the package's, never the operator's: a working
/// copy that claimed a coordinate system its bytes do not declare would be a
/// catalogue row that lies about the file beside it.
pub fn identity(bytes: &[u8]) -> Result<PackageIdentity, Failure> {
    identity_with_client(bytes, &crate::native::capture()?)
}

/// Reuse an invocation's captured owner when identity precedes admission.
/// Full package decoding and authored revision derivation stay in Network.
pub(crate) fn identity_with_client(
    bytes: &[u8],
    client: &ds_network_client::CapturedNetworkClient,
) -> Result<PackageIdentity, Failure> {
    let scratch = crate::native::scratch()?;
    let inspection = client
        .grid_inspect(bytes, scratch.path(), &|| false)
        .map_err(|error| {
            if error.owner_code() != Some("package_refused") {
                return crate::native::map_failure(error);
            }
            let message = error.to_string();
            if message.contains("schema") {
                Failure::invalid("package_decode_failed", message)
                    .remedy("use ds dsgrid package migrate --path <source> --expected-source-sha256 <capture> --dry-run; local model set-active auto-heals through that same migration")
            } else {
                Failure::invalid("not_a_dsgrid_package", message)
                    .remedy("pass a .dsgrid this build's engine can open")
            }
        })?
        .result;
    Ok(PackageIdentity {
        crs: inspection.coordinate_system.to_string(),
        model_revision: inspection.model_revision,
        sha256: inspection.source.package_sha256,
        bytes: bytes.len() as u64,
        authored_revision: inspection.source.expected_revision,
    })
}

/// Mint one id for a working copy. Lowercase hex, so it is one path segment
/// wherever a host keeps its packages.
pub fn mint_id() -> String {
    let random = uuid::Uuid::new_v4().simple().to_string();
    format!("local-{}", &random[..16])
}

pub fn row(model: &LocalModel, active: Option<&str>) -> Value {
    json!({
        "model": model.id,
        "name": model.display_name,
        "active": active == Some(model.id.as_str()),
        "origin": match model.origin {
            Origin::Created => "created",
            Origin::Imported => "imported",
            Origin::Project => "project",
        },
        "crs": model.crs,
        "revision": model.model_revision,
        "size_bytes": model.bytes,
        "content_digest": model.sha256,
        "created_at": model.created_at,
        "head_revision": model.head_revision,
        "revised_at": model.revised_at,
        // The link to a live PLS-CADD workspace (contract 02); null until
        // `ds dsgrid model link` records one.
        "pls_source": model.pls_source.as_ref().and_then(|link| serde_json::to_value(link).ok()),
        "project_binding": model.project.as_ref().map(|pin| json!({
            "project": pin.project_id,
            "model": pin.model_id,
            "revision": pin.revision_id,
            "digest": pin.digest,
        })),
        "head_revision": model.head_revision,
        "revised_at": model.revised_at,
        "pls_source": crate::model::pls_source::link_json(model.pls_source.as_ref()),
    })
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;

/// The meaningful local model-open boundary. Every subsequent operation uses
/// the saved strict-current package. Raw/library/template readers are untouched.
pub fn prepare_open(located: &Located) -> Result<Option<Value>, Failure> {
    prepare_open_at(&root()?, located)
}

fn prepare_open_at(
    root_path: &std::path::Path,
    located: &Located,
) -> Result<Option<Value>, Failure> {
    use ds_network_contracts::grid_operation::{
        GridOperationV1, PackageMigrateParamsV1, PackageMigrateResultV1,
    };
    let source = crate::package::read_bytes(
        located
            .path
            .to_str()
            .ok_or_else(|| Failure::invalid("model_unreadable", "package path is not UTF-8"))?,
    )?;
    let source_sha = format!("{:x}", Sha256::digest(&source));
    if source_sha != located.row.sha256 {
        return Err(Failure::invalid(
            "local_model_revision_conflict",
            "local catalogue digest and package disagree",
        )
        .remedy(REVISION_CONFLICT.remedy));
    }
    let client = crate::native::capture()?;
    let mut outcome = crate::native::operation_with(
        &client,
        &GridOperationV1::PackageMigrate(PackageMigrateParamsV1::default()),
        Some(&source),
        &[],
    )
    .map_err(|failure| {
        if failure.code() == "package_migration_invalid" {
            failure.remedy(
                "retain the original package; unsupported historical facts require an owning migration",
            )
        } else {
            failure
        }
    })?;
    let migrated: PackageMigrateResultV1 = serde_json::from_value(outcome.result.result.clone())
        .map_err(|error| Failure::failed("network_result_invalid", error.to_string()))?;
    if migrated.already_current {
        return Ok(None);
    }
    // A draft has its own immutable base/checkpoint graph; do not rebase it as
    // an incidental model-open. The existing catalogue authority owns refusal.
    if located.row.draft.is_some() {
        return Err(Failure::invalid(
            "local_model_draft_pending",
            "resolve the local draft before format migration",
        )
        .remedy(DRAFT_PENDING.remedy));
    }
    let bytes = outcome.take_artifact("model.dsgrid").ok_or_else(|| {
        Failure::failed(
            "network_result_invalid",
            "the format migration returned no current package",
        )
    })?;
    let identity = identity_with_client(&bytes, &client)?;
    if identity.model_revision != located.row.model_revision {
        return Err(Failure::invalid(
            "package_migration_invalid",
            "format migration changed authored package revision",
        ));
    }
    let cache_root = root_path.join("model-format-migrations");
    std::fs::create_dir_all(&cache_root)
        .map_err(|error| Failure::failed("package_migration_output", error.to_string()))?;
    let cache = cache_root.join(format!("{}-{}", source_sha, migrated.output_codec_sha256));
    let files = if cache.exists() {
        let saved_source = std::fs::read(cache.join("original.dsgrid"));
        let saved_output = std::fs::read(cache.join("model.dsgrid"));
        let saved_receipt = std::fs::read(cache.join("migration.receipt.json"));
        let valid = saved_source.as_ref().is_ok_and(|b| b == &source)
            && saved_output.as_ref().is_ok_and(|b| b == &bytes)
            && saved_receipt
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                == Some(migrated.receipt.clone());
        if !valid {
            return Err(Failure::invalid(
                "package_migration_output",
                "existing content-addressed migration output is incomplete or differs; never overwrite it",
            ));
        }
        json!({"package_path":cache.join("model.dsgrid"), "receipt_path":cache.join("migration.receipt.json")})
    } else {
        crate::package_migrate::materialize_preserving_source(
            &cache,
            &bytes,
            &migrated.receipt,
            &migrated.source_sha256,
            Some(&source),
        )?
    };
    // Original/current bytes and receipt exist before the authoritative SHA
    // fence. A racing writer or pending draft refuses without replacing it.
    ds_layer_store::local_models::execute_at(
        root_path,
        &located.scope,
        Op::Revise {
            id: located.row.id.clone(),
            expected_sha256: Some(source_sha.clone()),
            model_revision: identity.model_revision,
            bytes: identity.bytes,
            sha256: identity.sha256.clone(),
            head_revision: Some(identity.authored_revision.clone()),
            revised_at: None,
        },
        Some(&bytes),
    )
    .map_err(refuse)?;
    Ok(Some(
        json!({"source_sha256":source_sha,"current_sha256":identity.sha256,
        "source_authored_revision":located.row.head_revision,"current_authored_revision":identity.authored_revision,
        "original_package_path":cache.join("original.dsgrid"),"files":files,
        "receipt":migrated.receipt,"published":false,"authored_model_revision_advanced":false}),
    ))
}

#[cfg(test)]
#[path = "workspace_format_open_tests.rs"]
mod format_open_tests;

#[cfg(test)]
#[path = "../../tests/support/workspace_identity.rs"]
mod identity_process_tests;
