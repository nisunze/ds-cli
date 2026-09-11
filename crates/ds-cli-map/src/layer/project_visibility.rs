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
use serde_json::Value;

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
    let request = ds_layer_ops::VisibilityRequest {
        layers: inputs.repeated("layer").to_vec(),
        visible,
    };
    let mut documents = ds_layer_ops::Native::new(inputs.require("lane")?);
    ds_layer_ops::set_visibility(
        &mut documents,
        &ds_layer_ops::Preferences::native()?,
        &request,
    )
}

pub fn render(data: &Value) -> String {
    ds_layer_ops::render_visibility(data)
}
