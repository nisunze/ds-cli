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

pub const LANE_ARG: Arg = Arg::value("lane", "<stable|canary>", "Which lane's catalogue.")
    .default("stable")
    .choices(&["stable", "canary"]);

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

/// Every refusal these commands can return: the kernel's closed vocabulary
/// plus this host's one IO failure. Composed, so a new kernel refusal is
/// documented the moment it exists.
pub const REFUSALS: &[Refusal] = &[
    UNKNOWN_MODEL,
    NAME_TAKEN,
    CATALOGUE_FULL,
    REQUEST_INVALID,
    SCOPE_MISMATCH,
    STORE_UNAVAILABLE,
];

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
    ds_layer_store::local_models::read_at(&root()?, &scope(inputs)?).map_err(refuse)
}

pub fn execute(inputs: &Inputs, op: Op, package: Option<&[u8]>) -> Result<Outcome, Failure> {
    execute_in(&scope(inputs)?, op, package)
}

/// Apply one operation to a catalogue already located by [`locate`].
pub fn execute_in(scope: &Scope, op: Op, package: Option<&[u8]>) -> Result<Outcome, Failure> {
    ds_layer_store::local_models::execute_at(&root()?, scope, op, package).map_err(refuse)
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
    let package = ds_grid_exchange::package::unpack(bytes).map_err(|error| {
        let message = error.to_string();
        // A package this build's schema has moved past is a different
        // situation from bytes that were never a package: the remedy is to
        // re-convert from the PLS-CADD source, not to look for another file.
        if message.contains("schema") {
            Failure::invalid("package_decode_failed", message)
                .remedy("this package predates the current canonical schema; re-convert it from its PLS-CADD workspace with `ds dsgrid-exchange convert`")
                .next("ds dsgrid-exchange convert --source <workspace> --target dsgrid --crs <crs> --out <dir>")
        } else {
            Failure::invalid("not_a_dsgrid_package", message)
                .remedy("pass a .dsgrid this build's engine can open")
        }
    })?;
    let session = ds_grid_engine::GridSession::open(package.snapshot);
    Ok(PackageIdentity {
        crs: package.manifest.model.coordinate_system.to_string(),
        model_revision: package.manifest.model.model_revision,
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len() as u64,
        authored_revision: session.current_revision().revision_id.as_str().to_string(),
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_id_is_one_path_segment() {
        for _ in 0..16 {
            let id = mint_id();
            assert!(
                ds_command_kernel::local_models::id_is_safe(&id),
                "{id} is not one safe path segment"
            );
        }
    }

    #[test]
    fn every_kernel_refusal_this_family_can_raise_is_documented() {
        // The kernel's vocabulary is closed; a code it can return that no
        // command documents is exactly what `refusal_coverage.rs` refuses.
        for code in [
            ds_command_kernel::local_models::UNKNOWN_MODEL,
            ds_command_kernel::local_models::DUPLICATE_NAME,
            ds_command_kernel::local_models::CATALOGUE_FULL,
            ds_command_kernel::local_models::MALFORMED,
            ds_command_kernel::local_models::SCOPE_MISMATCH,
        ] {
            assert!(
                REFUSALS.iter().any(|refusal| refusal.code == code),
                "the kernel can refuse with `{code}` and no command says so"
            );
        }
    }

    #[test]
    fn a_refusal_keeps_the_kernels_own_name() {
        let refused = refuse(StoreError::Refused {
            code: ds_command_kernel::local_models::UNKNOWN_MODEL.to_owned(),
            message: "no local model m-9 on this machine".into(),
        });
        assert_eq!(
            refused.code(),
            ds_command_kernel::local_models::UNKNOWN_MODEL
        );

        let unavailable = refuse(StoreError::Store("disk is full".into()));
        assert_eq!(unavailable.code(), STORE_UNAVAILABLE.code);
    }
}
