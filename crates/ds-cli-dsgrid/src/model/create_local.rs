//! `ds dsgrid model create-local` — one empty working copy on this machine.
//!
//! Named `create-local` rather than `create` because the reverted family used
//! the bare verb for something else entirely: registering a model in a
//! project. This creates nothing outside this machine.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
    Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::local_models::{Op, Origin};
use serde_json::{Value, json};

use crate::model::UNSUPPORTED_GRID_CRS;
use crate::model::workspace;

const NAME_ARG: Arg = Arg {
    name: "name",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Display name for the new model. The application names it if omitted.",
};

const CRS_ARG: Arg = Arg {
    name: "crs",
    kind: ArgKind::Value,
    value: "<crs>",
    required: false,
    default: None,
    choices: &[],
    summary: "Projected metric coordinate system, e.g. EPSG:32735. The app's default if omitted.",
};

/// This family's refusals, plus the engine's answer to a coordinate system it
/// does not author.
const CREATE_REFUSALS: &[Refusal; 1 + workspace::REFUSALS.len()] = &create_refusals();
const fn create_refusals() -> [Refusal; 1 + workspace::REFUSALS.len()] {
    let mut all = [UNSUPPORTED_GRID_CRS; 1 + workspace::REFUSALS.len()];
    let mut index = 0;
    while index < workspace::REFUSALS.len() {
        all[1 + index] = workspace::REFUSALS[index];
        index += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.model.create-local",
    path: &["dsgrid", "model", "create-local"],
    contract: 1,
    summary: "Create one empty DS Grid working copy on this machine and open it.",
    purpose: "\
Writes one empty model into this machine's own catalogue and opens it as the \
active copy — the single local command that changes which copy an editing \
session starts from, so the receipt says so rather than leaving it to be \
discovered. It needs no sign-in, no project and no application, and reaches \
nothing governed: publishing a revision is `ds dsgrid publish-version`.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        NAME_ARG,
        CRS_ARG,
        workspace::LANE_ARG,
        workspace::ACCOUNT_ARG,
    ],
    output: "\
`status: created`, the new opaque `model` id, its `name`, `crs` and first \
`revision`, plus `active_model` and `became_active`.",
    examples: &[Example {
        command: "ds dsgrid model create-local --name \"Kamonyi MV\" --crs EPSG:32735 --output json",
        note: "Read .data.model; the new model is already the active one.",
        runnable: false,
    }],
    refusals: CREATE_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // The engine authors the package and decides which coordinate systems it
    // can author; nothing about a CRS is guessed here against a stale list.
    let model = ds_grid_exchange::create_blank_model(&ds_grid_exchange::BlankModelRequest {
        coordinate_system: inputs.value("crs"),
        ..Default::default()
    })
    .map_err(|error| {
        Failure::invalid(UNSUPPORTED_GRID_CRS.code, error.to_string())
            .remedy(UNSUPPORTED_GRID_CRS.remedy)
    })?;
    let identity = workspace::identity(&model.bytes)?;
    let id = workspace::mint_id();
    let name = inputs
        .value("name")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(|| format!("Local model {id}"), str::to_owned);

    let outcome = workspace::execute(
        inputs,
        Op::Register {
            id: id.clone(),
            display_name: name,
            origin: Origin::Created,
            crs: identity.crs,
            model_revision: identity.model_revision,
            bytes: identity.bytes,
            sha256: identity.sha256,
            created_at: None,
            project: None,
            // A new empty model is what the operator is about to work on.
            activate: true,
        },
        Some(&model.bytes),
    )?;
    let created = outcome
        .model
        .as_ref()
        .ok_or_else(|| Failure::internal("local_model_store_unavailable", "nothing was created"))?;
    Ok(json!({
        "status": "created",
        "model": workspace::row(created, outcome.catalogue.active.as_deref()),
        "active_model": outcome.catalogue.active,
        "became_active": outcome.active_changed,
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "created {} · {}\n",
        data["model"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or(""),
    );
    if let Some(crs) = data["crs"].as_str() {
        out.push_str(&format!("  crs        {crs}\n"));
    }
    out.push_str(&format!(
        "  revision   {}\n  active     {}\n",
        data["revision"].as_str().unwrap_or("—"),
        if data["became_active"].as_bool().unwrap_or(false) {
            "yes, this model now occupies Profile"
        } else {
            "unchanged"
        },
    ));
    out
}
