//! `ds map layer default` — govern project-wide starting visibility.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use super::native::LANE_ARG;
use super::target::{self, Target};

const LAYER_ARG: Arg = Arg {
    name: "layer",
    kind: ArgKind::Repeated,
    value: "<config-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "Canonical layer id from `ds map layer list`. Repeat for several.",
};

pub static COMMAND: Command = Command {
    id: "map.layer.default",
    path: &["map", "layer", "default"],
    contract: 1,
    summary: "Set project-wide starting visibility (needs --yes).",
    purpose: "Saves the starting visibility for canonical project layers through ds-brain. The shared Rust layer kernel applies it only when this user has no remembered choice, so a user's explicit show or hide remains authoritative. The project is explicit on Server calls and fenced for the full read/admit/write operation.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        LAYER_ARG,
        Arg::value("visible", "<true|false>", "Project-wide starting state.")
            .required()
            .choices(&["true", "false"]),
        LANE_ARG,
        target::TARGET_ARG,
        target::PROJECT_ARG,
        target::STATE_DIR_ARG,
    ],
    output: "Project, exact canonical ids and starting visibility, applied/persisted flags, and updated count.",
    examples: &[Example {
        command: "ds map layer default --layer gt/sector_boundaries --visible false --yes --output json",
        note: "Existing user choices remain unchanged.",
        runnable: false,
    }],
    refusals: super::native::LAYER_ORDER_REFUSALS,
    reference: Some("docs/reference/map.md"),
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if target::resolve(inputs)? == Target::Server {
        return ds_cli_server::layers_default_visibility(inputs, context);
    }
    let visible = inputs.require("visible")? == "true";
    let defaults = inputs
        .repeated("layer")
        .iter()
        .map(|layer_id| ds_layer_ops::VisibilityDefault {
            layer_id: layer_id.to_owned(),
            visible,
        })
        .collect();
    let mut documents = target::desktop_documents(inputs)?;
    ds_layer_ops::set_default_visibility(
        &mut documents,
        &ds_layer_ops::DefaultVisibilityRequest { defaults },
    )
}

pub fn render(data: &Value) -> String {
    ds_layer_ops::render_default_visibility(data)
}
