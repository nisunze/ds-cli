//! `ds design model catalog` — the project's retained MV model catalogue.
//!
//! The Kernel owns the SQLite schema, the source-basis comparison and the
//! projection (`ds_command_kernel::grid_project_catalog`); this command only
//! names the project, the lane and the read. The Desktop shell attaches to it
//! and the browser runs the same Kernel step over its own SQLite.

use std::io::{Read, Write};
use std::path::Path;

use ds_cli_auth::grid_mv_catalogue::{self, CatalogRead};
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const INPUT: Refusal = Refusal {
    code: "design_model_catalog_input_invalid",
    when: "--retain is combined with --read refresh, or its file is not one exact version JSON",
    remedy: "Pass --retain with --read held and the exact version document of one held head.",
};
const OUTPUT: Refusal = Refusal {
    code: "design_model_catalog_output_invalid",
    when: "--out is not a fresh absolute path in an existing private directory",
    remedy: "Choose a new absolute path in an existing private directory.",
};
const STALE: Refusal = Refusal {
    code: "grid_catalog_version_stale",
    when: "the retained version does not name the held head of that model",
    remedy: "Refresh the catalogue, then retain the exact current head version.",
};
const REVOKED: Refusal = Refusal {
    code: "auth_context_mismatch",
    when: "the held catalogue's access was refused by an earlier refresh",
    remedy: "Deliberately refresh after authorization is restored.",
};
const STORE: Refusal = Refusal {
    code: "mv_model_catalogue_invalid",
    when: "this machine's catalogue store is unreadable or the listing is incomplete",
    remedy: "Refresh the project model catalogue; report the store error if it repeats.",
};
const CHANGED: Refusal = Refusal {
    code: "project_context_changed",
    when: "the signed-in account changed while the catalogue was read",
    remedy: "Repeat the read under the account that should own it.",
};
const OWN: [Refusal; 6] = [INPUT, OUTPUT, STALE, REVOKED, STORE, CHANGED];
/// The same shared native vocabulary `design model list` declares: the grid
/// owner's refusals and the named-project native client's.
const SHARED: usize =
    ds_cli_auth::GRID_MODEL_REFUSALS.len() + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len();
const fn refusals() -> [Refusal; SHARED + 6] {
    let mut rows = [INPUT; SHARED + 6];
    let mut index = 0;
    while index < OWN.len() {
        rows[index] = OWN[index];
        index += 1;
    }
    let mut grid = 0;
    while grid < ds_cli_auth::GRID_MODEL_REFUSALS.len() {
        rows[index + grid] = ds_cli_auth::GRID_MODEL_REFUSALS[grid];
        grid += 1;
    }
    let mut named = 0;
    while named < ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() {
        rows[index + grid + named] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals[named];
        named += 1;
    }
    rows
}
const REFUSALS: &[Refusal] = &refusals();

pub static COMMAND: Command = Command {
    id: "design.model.catalog",
    path: &["design", "model", "catalog"],
    contract: 1,
    summary: "Read the project's retained MV model catalogue, offline first.",
    purpose: "Read the project's model heads from this machine's Kernel SQLite rows (--read held: no transport), or refresh them with one authorized listing compared with the held source basis (--read refresh): first arrivals persist at once, retired heads leave and a failed listing labels the held rows. --retain records one held head's verified version. Never downloads packages.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        crate::LANE_ARG,
        Arg::value(
            "read",
            "<mode>",
            "Held rows only, or one authorized refresh.",
        )
        .choices(&["held", "refresh"])
        .default("held"),
        Arg::value(
            "retain",
            "<json-file>",
            "Exact version document whose verified bytes this host holds.",
        ),
        Arg::value(
            "out",
            "<absolute-path>",
            "Write the projection to a fresh private JSON file.",
        ),
    ],
    output: "ds.grid-project-catalog/v1: project, lane, observation (source, state, observed_at_ms, basis_sha256, revision, last_error), heads[], mv_models[] (label order, retained_version, retained_current), changes after a refresh, refresh_error. --out returns ds.grid-project-catalog-file/v1 (path, sha256, size_bytes, lane, project, principal, observation).",
    examples: &[Example {
        command: "ds design model catalog --project <id> --read refresh --output json",
        note: "`.data.changes.added` names heads persisted for the first time.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &["newly created MV models", "cached project heads"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid(refusal: Refusal, why: impl std::fmt::Display) -> Failure {
    Failure::invalid(refusal.code, why.to_string()).remedy(refusal.remedy)
}

fn retained_version(path: &str) -> Result<(String, Value), Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| invalid(INPUT, e))?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(INPUT, e))?;
    if bytes.len() > 64 * 1024 {
        return Err(invalid(INPUT, "the version document exceeds 64 KiB"));
    }
    let version: Value = serde_json::from_slice(&bytes).map_err(|e| invalid(INPUT, e))?;
    let model = version["model_id"]
        .as_str()
        .filter(|id| ds_command_kernel::grid_project_catalog::model_id(id))
        .ok_or_else(|| invalid(INPUT, "the version document names no model_id"))?
        .to_owned();
    Ok((model, version))
}

fn validate_output(path: &Path) -> Result<(), Failure> {
    if !path.is_absolute()
        || path.to_str().is_none()
        || path.try_exists().map_err(|e| invalid(OUTPUT, e))?
        || !path.parent().is_some_and(Path::is_dir)
    {
        return Err(invalid(
            OUTPUT,
            "Use a fresh absolute path in an existing private directory",
        ));
    }
    ds_layer_store::snapshot_privacy::directory(path.parent().unwrap())
        .map_err(|e| invalid(OUTPUT, e))
}

fn deliver(
    path: &Path,
    output: &Value,
    lane: &str,
    project: &str,
    identity: &ds_cli_auth::ProviderIdentity,
) -> Result<Value, Failure> {
    let bytes = serde_json::to_vec(output).map_err(|e| invalid(OUTPUT, e))?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid(OUTPUT, "the catalogue projection exceeds 64 MiB"));
    }
    #[cfg(windows)]
    let mut file = ds_cli_auth::windows_host::create_private_output_file(path)
        .map_err(|e| invalid(OUTPUT, e))?;
    #[cfg(not(windows))]
    let mut file = ds_layer_store::private::open_options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| invalid(OUTPUT, e))?;
    let written = (|| {
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| invalid(OUTPUT, e))?;
        if ds_cli_auth::headless_identity_for_named_project(lane)? != *identity {
            return Err(Failure::failed(
                CHANGED.code,
                "The account changed before the catalogue was delivered.",
            ));
        }
        Ok(())
    })();
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(
        json!({"schema":"ds.grid-project-catalog-file/v1","lane":lane,"project":project,
        "principal":identity.uid(),"path":path,"sha256":format!("{:x}", Sha256::digest(&bytes)),
        "size_bytes":bytes.len(),"observation":output["observation"]}),
    )
}

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let lane = inputs.require("lane")?;
    let read = match inputs.value("read").unwrap_or("held") {
        "held" => CatalogRead::Held,
        "refresh" => CatalogRead::Refresh,
        other => {
            return Err(invalid(
                INPUT,
                format!("--read {other} is not held or refresh"),
            ));
        }
    };
    let out = inputs.value("out").map(Path::new);
    if let Some(path) = out {
        validate_output(path)?;
    }
    let (identity, mut catalog) = match inputs.value("retain") {
        Some(_) if read == CatalogRead::Refresh => {
            return Err(invalid(
                INPUT,
                "--retain records held state; use --read held",
            ));
        }
        Some(path) => {
            let (model, version) = retained_version(path)?;
            grid_mv_catalogue::retain_catalog_version(lane, project, &model, version)?
        }
        None => grid_mv_catalogue::catalog(lane, project, read)?,
    };
    catalog["lane"] = json!(lane);
    match out {
        Some(path) => deliver(path, &catalog, lane, project, &identity),
        None => Ok(catalog),
    }
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default() + "\n"
}

#[cfg(test)]
#[path = "model_catalog_tests.rs"]
mod tests;
