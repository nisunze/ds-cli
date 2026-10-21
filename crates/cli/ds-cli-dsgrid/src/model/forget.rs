use crate::model::workspace;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::local_models::Op;
use serde_json::{Value, json};
const MODEL_ARG: Arg =
    Arg::value("model", "<model-id>", "The local working copy to forget.").required();
pub static COMMAND: Command = Command {
    id: "dsgrid.model.forget",
    path: &["dsgrid", "model", "forget"],
    contract: 1,
    summary: "Forget one local working copy and remove its package.",
    purpose: "Removes a working copy and its package from this machine. If active, the next newest remaining copy becomes active. Project versions are unaffected.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[MODEL_ARG, workspace::LANE_ARG, workspace::ACCOUNT_ARG],
    output: "status, model, name, active_model, and active_changed.",
    examples: &[Example {
        command: "ds dsgrid model forget --model local-123 --account <uid> --lane stable --output json",
        note: "Remove this machine's local copy and package.",
        runnable: false,
    }],
    refusals: workspace::REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: || Availability::Available,
};
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let id = inputs.require("model")?.trim().to_owned();
    let outcome = workspace::execute(inputs, Op::Forget { id }, None)?;
    let forgotten = outcome.model.as_ref().ok_or_else(|| {
        Failure::internal("local_model_store_unavailable", "nothing was forgotten")
    })?;
    Ok(
        json!({"status": "forgotten", "model": forgotten.id, "name": forgotten.display_name,
        "active_model": outcome.catalogue.active, "active_changed": outcome.active_changed}),
    )
}
pub fn render(data: &Value) -> String {
    format!(
        "forgotten {} - {}\n  active   {}\n",
        data["model"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or("?"),
        data["active_model"].as_str().unwrap_or("none")
    )
}
