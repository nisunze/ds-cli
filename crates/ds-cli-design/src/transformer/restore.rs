//! `ds design transformer restore` — bring a retired transformer back.

use ds_cli_auth::{RetirementAction, RetirementRequest};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use super::{LANE_ARG, TRANSFORMER_ARG};

pub static COMMAND: Command = Command {
    id: "design.transformer.restore",
    path: &["design", "transformer", "restore"],
    contract: 1,
    summary: "Restore retired transformers to the active set (needs --yes).",
    purpose: "\
After CLI confirmation, restores the native user and asks the governed report \
service to clear the retirement of the named transformers in only its \
audience-fenced selected project. Only a transformer retired through this \
family restores; a document soft-deleted by another path carries no retirement \
record and is refused per name (`no_retirement_record`). The same governance \
as retirement applies and every name is answered in order.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, LANE_ARG],
    output: "\
Lane and selected-project identity/status, the requested names, applied/failed \
counts, and one result per name: `applied` with the restoration timestamp, or \
a closed `refusal` (`not_found`, `not_retired`, `no_retirement_record`, \
`special_document`, `governance_locked`, `not_owner`, `failed`) with its message.",
    examples: &[Example {
        command: "ds design transformer restore --transformer TX-1 --yes",
        note: "The transformer reappears in listings, reports and the next tile run.",
        runnable: false,
    }],
    refusals: super::NATIVE_WRITE_REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformers = super::transformer_set(inputs, true)?;
    let request = RetirementRequest::restore(transformers).map_err(|error| {
        Failure::invalid("invalid_transformer_scope", error.to_string())
            .remedy(super::INVALID_SCOPE.remedy)
    })?;
    let headless = ds_cli_auth::transformer_retirement(
        inputs.require("lane")?,
        RetirementAction::Restore,
        &request,
    )?;
    let mut output = super::project_receipt(&headless);
    let receipt = super::receipt_json(headless.result(), &request);
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(receipt.as_object().expect("receipt is an object").clone());
    Ok(output)
}

pub fn render(data: &Value) -> String {
    super::render_receipt("restored", data)
}
