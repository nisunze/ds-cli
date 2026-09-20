//! `ds dsgrid model import-external` — acquire an external `.dsgrid` file.
//!
//! Acquisition, and nothing else. The imported model does **not** take
//! Profile/editing occupancy, exactly as the application's own
//! `Import external model…` does not; the operator chooses it afterwards with
//! `set-active`. Naming it `import-external` rather than `import` is the
//! point: the reverted family's `import` meant "register this in my project",
//! which is a different act with a different authority and now has a different
//! name — `ds dsgrid publish-version`.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
    Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::local_models::{Op, Origin};
use serde_json::{Value, json};

use crate::model::NAME_ARG;
use crate::model::workspace;
use crate::model::{ABSOLUTE_PATH_REQUIRED, MODEL_TOO_LARGE, UNSUPPORTED_MODEL_SOURCE};

const PATH_ARG: Arg = Arg {
    name: "path",
    kind: ArgKind::Value,
    value: "<absolute-path.dsgrid>",
    required: true,
    default: None,
    choices: &[],
    summary: "The .dsgrid package to acquire, by absolute path on this machine.",
};

/// The local path refusals, this family's, and the engine's answer to bytes
/// that are not a package it can open.
const NOT_FOUND: Refusal = Refusal {
    code: "model_not_found",
    when: "the named path does not exist or is not a readable file",
    remedy: "check the path; --path takes a .dsgrid file, not a directory",
};
const NOT_A_PACKAGE: Refusal = Refusal {
    code: "not_a_dsgrid_package",
    when: "the named file is not a .dsgrid package this build's engine can open",
    remedy: "convert a PLS-CADD source with `ds dsgrid-exchange`, or pass a package this build accepts",
};
const SCHEMA_MOVED: Refusal = Refusal {
    code: "package_decode_failed",
    when: "the package is damaged or carries a table schema this build does not decode",
    remedy: "re-convert it from its PLS-CADD workspace with `ds dsgrid-exchange convert`",
};
const IMPORT_OWN: [Refusal; 6] = [
    ABSOLUTE_PATH_REQUIRED,
    UNSUPPORTED_MODEL_SOURCE,
    MODEL_TOO_LARGE,
    NOT_FOUND,
    NOT_A_PACKAGE,
    SCHEMA_MOVED,
];
const IMPORT_REFUSALS: &[Refusal; IMPORT_OWN.len() + workspace::REFUSALS.len()] =
    &import_refusals();
const fn import_refusals() -> [Refusal; IMPORT_OWN.len() + workspace::REFUSALS.len()] {
    let mut all = [NOT_A_PACKAGE; IMPORT_OWN.len() + workspace::REFUSALS.len()];
    let mut index = 0;
    while index < IMPORT_OWN.len() {
        all[index] = IMPORT_OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < workspace::REFUSALS.len() {
        all[IMPORT_OWN.len() + shared] = workspace::REFUSALS[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.model.import-external",
    path: &["dsgrid", "model", "import-external"],
    contract: 1,
    summary: "Acquire an external .dsgrid file as a working copy on this machine.",
    purpose: "\
Brings one `.dsgrid` package the operator already has into this machine's own \
catalogue as a durable working copy, verifying with the engine that the bytes \
are a package this build can open and recording the identity they declare. \
This is source acquisition only: the imported copy does not become the open \
one, so choose it with `ds dsgrid model set-active` when you want to work in \
it. A PLS-CADD workspace or `.bak` is refused by name, because converting one \
is `ds dsgrid-exchange`'s act, not this one's.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        PATH_ARG,
        NAME_ARG,
        workspace::LANE_ARG,
        workspace::ACCOUNT_ARG,
    ],
    output: "\
`status: imported`, the new opaque `model` id, its `name` and `revision`, the \
`imported_from` file name, `source_path`, `size_bytes`, \
`became_active: false` — acquisition never activates — and `auto_link`: \
`linked` with the PLS-CADD workspace path and digest when an \
exchange-report.json beside the package named a folder source that still \
digests to its pin, else `unlinked` with the reason.",
    examples: &[Example {
        command: "ds dsgrid model import-external --path /srv/models/kamonyi.dsgrid --output json",
        note: "Then `ds dsgrid model set-active --model <id>` to work in it.",
        runnable: false,
    }],
    refusals: IMPORT_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let path = crate::model::external_dsgrid_path(inputs.require("path")?, "path")?;
    let file = std::path::Path::new(&path);
    let metadata = std::fs::metadata(file).map_err(|error| {
        Failure::invalid(NOT_FOUND.code, format!("{path} cannot be read: {error}"))
            .remedy(NOT_FOUND.remedy)
    })?;
    if !metadata.is_file() {
        return Err(
            Failure::invalid(NOT_FOUND.code, format!("{path} is not a file"))
                .remedy(NOT_FOUND.remedy),
        );
    }
    if metadata.len() > ds_layer_store::local_models::MAX_PACKAGE_BYTES {
        return Err(Failure::invalid(
            MODEL_TOO_LARGE.code,
            format!(
                "{path} is larger than the {} MiB a working copy may be",
                ds_layer_store::local_models::MAX_PACKAGE_BYTES / (1024 * 1024)
            ),
        )
        .remedy(MODEL_TOO_LARGE.remedy));
    }
    let bytes = std::fs::read(file).map_err(|error| {
        Failure::invalid(NOT_FOUND.code, format!("{path} cannot be read: {error}"))
            .remedy(NOT_FOUND.remedy)
    })?;
    // The engine decides whether these bytes are a package, and the identity
    // recorded is the one they declare — never the operator's description of
    // them.
    let identity = workspace::identity(&bytes)?;
    let id = workspace::mint_id();
    let name = inputs
        .value("name")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || {
                file.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("Imported model {id}"))
            },
            str::to_owned,
        );

    let outcome = workspace::execute(
        inputs,
        Op::Register {
            id: id.clone(),
            display_name: name,
            origin: Origin::Imported,
            crs: identity.crs,
            model_revision: identity.model_revision,
            bytes: identity.bytes,
            sha256: identity.sha256,
            created_at: None,
            project: None,
            head_revision: Some(identity.authored_revision),
            // Acquisition is not a decision to work in it.
            activate: false,
        },
        Some(&bytes),
    )?;
    let mut imported = outcome.model.clone().ok_or_else(|| {
        Failure::internal("local_model_store_unavailable", "nothing was imported")
    })?;

    // An exchange output links itself (contract 02 §1): `ds dsgrid-exchange
    // convert` writes `exchange-report.json` beside the package, naming the
    // folder each source was read from and the digest it pinned. When that
    // report is beside this package, its source is a PLS-CADD folder, the
    // package preserved a tree digesting to the same pin, and the folder
    // still digests to it now, the copy is linked without the operator
    // re-typing the path. Anything short of that leaves the copy unlinked
    // and says why — `ds dsgrid model link` is the explicit act.
    let auto_link = auto_link(inputs, &imported.id, &bytes, file)?;
    if let AutoLink::Linked(link) = &auto_link {
        let relinked = workspace::execute(
            inputs,
            Op::Link {
                id: imported.id.clone(),
                pls_source: link.clone(),
            },
            None,
        )?;
        if let Some(model) = relinked.model {
            imported = model;
        }
    }
    Ok(json!({
        "status": "imported",
        "model": workspace::row(&imported, outcome.catalogue.active.as_deref()),
        "active_model": outcome.catalogue.active,
        "became_active": outcome.active_changed,
        "source": path,
        "auto_link": auto_link.json(),
    }))
}

enum AutoLink {
    Linked(ds_command_kernel::local_models::PlsSourceLink),
    Skipped(String),
}

impl AutoLink {
    fn json(&self) -> Value {
        match self {
            Self::Linked(link) => json!({
                "status": "linked",
                "path": link.path,
                "digest": link.digest,
            }),
            Self::Skipped(reason) => json!({ "status": "unlinked", "reason": reason }),
        }
    }
}

/// The sibling `exchange-report.json` of an exchange output, when the
/// package sits where `convert` wrote it.
fn auto_link(
    inputs: &Inputs,
    id: &str,
    package_bytes: &[u8],
    package_file: &std::path::Path,
) -> Result<AutoLink, Failure> {
    let _ = inputs;
    let report_path = package_file
        .parent()
        .map(|dir| dir.join("exchange-report.json"))
        .filter(|path| path.is_file());
    let Some(report_path) = report_path else {
        return Ok(AutoLink::Skipped(
            "no exchange-report.json beside the package; link with `ds dsgrid model link`"
                .to_string(),
        ));
    };
    let report: Value = std::fs::read(&report_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null);
    let Some(sources) = report["sources"].as_array() else {
        return Ok(AutoLink::Skipped(
            "the exchange report beside the package predates source paths; link with `ds dsgrid model link`".to_string(),
        ));
    };
    let folder_sources = sources
        .iter()
        .filter(|source| source["kind"].as_str() == Some("pls_workspace_folder"))
        .filter_map(|source| {
            Some((
                source["path"].as_str()?.to_string(),
                source["digest"].as_str()?.to_string(),
            ))
        })
        .collect::<Vec<_>>();
    let [(path, pinned)] = folder_sources.as_slice() else {
        return Ok(AutoLink::Skipped(format!(
            "the exchange report names {} PLS-CADD folder source(s); a link needs exactly one",
            folder_sources.len()
        )));
    };
    let package = ds_grid_exchange::package::unpack(package_bytes).map_err(|error| {
        Failure::invalid(NOT_A_PACKAGE.code, error.to_string()).remedy(NOT_A_PACKAGE.remedy)
    })?;
    let Some(source) = ds_grid_exchange::conversion::dsgrid_package_pls_source(&package)
        .map_err(|detail| Failure::failed(NOT_A_PACKAGE.code, detail))?
    else {
        return Ok(AutoLink::Skipped(
            "the package preserved no PLS-CADD workspace".to_string(),
        ));
    };
    if &source.origin_digest != pinned {
        return Ok(AutoLink::Skipped(format!(
            "the package's preserved workspace ({}) is not the report's pinned source ({pinned})",
            source.origin_digest
        )));
    }
    let workspace_read = match crate::model::pls_source::read_workspace(path) {
        Ok(read) => read,
        Err(failure) => {
            return Ok(AutoLink::Skipped(format!(
                "the report's source folder `{path}` cannot be read here: {}",
                failure.message()
            )));
        }
    };
    if workspace_read.digest != *pinned {
        return Ok(AutoLink::Skipped(format!(
            "`{path}` no longer digests to the pinned {pinned} (now {}); re-convert or link explicitly",
            workspace_read.digest
        )));
    }
    let _ = id;
    Ok(AutoLink::Linked(crate::model::pls_source::link_for(
        &workspace_read,
    )))
}

pub fn render(data: &Value) -> String {
    format!(
        "imported {} · {}\n  from       {}\n  revision   {}\n  bytes      {}\n  active     {}\n",
        data["model"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or(""),
        data["imported_from"].as_str().unwrap_or("?"),
        data["revision"].as_str().unwrap_or("—"),
        data["size_bytes"].as_u64().unwrap_or(0),
        match data["active_model"].as_str() {
            Some(active) => format!("unchanged ({active})"),
            None => "unchanged (none)".to_string(),
        },
    )
}
