//! `ds map layer show|hide` — remember canonical project layers visible or
//! hidden for this machine's user, without a desktop.
//!
//! The kernel decides what the gesture writes (the family: primary, labels,
//! boundaries, 3D shapes) and how each canonical row then reads; the native
//! layer store persists the preferences per lane and project; the drawer on
//! this machine and `ds map layer list` read the same answer.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::native::LANE_ARG;

const LAYER_ARG: Arg = Arg {
    name: "layer",
    kind: ArgKind::Repeated,
    value: "<config-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "Canonical layer id from `ds map layer list`. Repeat for several.",
};

const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    purpose: &'static str,
    output: &'static str,
    examples: &'static [Example],
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose,
        chapter: Chapter::Survey,
        effect: Effect::LocalFileWrite,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[LAYER_ARG, LANE_ARG],
        output,
        examples,
        refusals: super::native::NATIVE_VISIBILITY_REFUSALS,
        reference: Some("docs/reference/map.md"),
        availability: ds_cli_auth::native_availability,
    }
}

pub static SHOW: Command = command(
    "map.layer.show",
    &["map", "layer", "show"],
    "Remember canonical project layers as visible for this machine's user.",
    "Reads the selected project's assembled layer document, asks the shared layer kernel what showing each canonical layer writes (its primary and companion runtime layers), and persists the preferences in the native layer store for this lane and project. Idempotent: showing a visible layer changes nothing. Authored `visibility: none` and 3D-only companions stay hidden at runtime and say so.",
    "Lane, project, the remembered layers with their runtime ids and folded visibility, which runtime layers changed, `persisted: native_local` and the store revision.",
    &[Example {
        command: "ds map layer show --layer survey/poles --layer design/lv_lines --output json",
        note: "Take ids from `ds map layer list`; runtime ids are refused.",
        runnable: false,
    }],
);

pub static HIDE: Command = command(
    "map.layer.hide",
    &["map", "layer", "hide"],
    "Remember canonical project layers as hidden for this machine's user.",
    "The exact counterpart of `map layer show`: the kernel names every runtime layer the family holds, the native layer store remembers them hidden for this lane and project, and `map layer list` reads the same answer. Idempotent.",
    "Lane, project, the remembered layers with their runtime ids and folded visibility, which runtime layers changed, `persisted: native_local` and the store revision.",
    &[Example {
        command: "ds map layer hide --layer survey/poles --output json",
        note: "Hides the poles and their label companion together.",
        runnable: false,
    }],
);

pub fn run_show(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    set(inputs, true)
}
pub fn run_hide(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    set(inputs, false)
}

fn set(inputs: &Inputs, visible: bool) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let wanted: Vec<String> = inputs
        .repeated("layer")
        .iter()
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
        .collect();
    if wanted.is_empty() {
        return Err(Failure::invalid("unknown_layer", "--layer names at least one canonical layer id")
            .remedy("copy ids from `ds map layer list --output json`"));
    }
    let headless = ds_cli_auth::layer_config(lane, false)?;
    let project = headless.project_id().to_owned();
    let document = headless.result().document().clone();
    let preferences = super::read_preferences(lane, &project)?;
    let identity = super::ask_layer_state(
        &json!({"schema": ds_command_kernel::layer_state::SCHEMA, "project": project, "document": document, "preferences": preferences, "op": {"kind": "classify"}}),
    )?;
    let mut runtime_ids = Vec::new();
    let mut unknown = Vec::new();
    for id in &wanted {
        let members: Vec<&str> = identity["layers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| row["canonical_id"].as_str() == Some(id))
            .filter_map(|row| row["id"].as_str())
            .collect();
        if members.is_empty() {
            unknown.push(id.as_str());
        }
        runtime_ids.extend(members.into_iter().map(str::to_owned));
    }
    if !unknown.is_empty() {
        return Err(Failure::invalid(
            "unknown_layer",
            format!("not canonical layers of the selected project: {}", unknown.join(", ")),
        )
        .remedy("copy ids from `ds map layer list --output json`"));
    }
    let transition = super::ask_layer_state(
        &json!({"schema": ds_command_kernel::layer_state::SCHEMA, "project": project, "document": document, "preferences": preferences, "op": {"kind": "set", "ids": runtime_ids, "visible": visible, "expand": true}}),
    )?;
    let next: std::collections::BTreeMap<String, bool> =
        serde_json::from_value(transition["preferences"].clone()).expect("kernel preferences are a boolean map");
    let receipt = ds_layer_store::visibility::replace(lane, &project, &next).map_err(|message| {
        Failure::invalid("local_layer_refused", message).remedy(super::LOCAL_STORE_REFUSAL.remedy)
    })?;
    let catalog = super::ask_layer_state(
        &json!({"schema": ds_command_kernel::layer_state::SCHEMA, "project": project, "document": document, "preferences": next, "op": {"kind": "catalog", "limit": 500}}),
    )?;
    let rows: Vec<Value> = catalog["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["id"].as_str().is_some_and(|id| wanted.iter().any(|w| w == id)))
        .cloned()
        .collect();
    Ok(json!({
        "lane": headless.lane(),
        "project": project,
        "visible": visible,
        "layers": rows,
        "changed": transition["changed"],
        "writes": transition["writes"],
        "persisted": receipt["persisted"],
        "revision": receipt["revision"],
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} {} canonical layers for {} · saved locally (revision {})\n",
        if data["visible"].as_bool().unwrap_or(false) { "showed" } else { "hid" },
        data["layers"].as_array().map_or(0, Vec::len),
        data["project"].as_str().unwrap_or("?"),
        data["revision"],
    );
    for row in data["layers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{:<38} {}\n",
            row["id"].as_str().unwrap_or("?"),
            super::list::visibility_word(&row["visibility"]),
        ));
    }
    out
}
