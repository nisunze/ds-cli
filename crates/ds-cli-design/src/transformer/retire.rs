//! `ds design transformer retire` — reversible, non-destructive retirement.

use ds_cli_auth::{RetirementAction, RetirementRequest};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use super::{LANE_ARG, REASON_ARG, TRANSFORMER_ARG};

pub static COMMAND: Command = Command {
    id: "design.transformer.retire",
    path: &["design", "transformer", "retire"],
    contract: 1,
    summary: "Retire transformers reversibly from the active set (needs --yes).",
    purpose: "\
After CLI confirmation, restores the native user and asks the governed report \
service to retire the named transformers in only its audience-fenced selected \
project. A retired transformer leaves every listing, report, tile run and \
count while its document, layers, versions, attachments and artifacts stay in \
place; `ds design transformer restore` brings it back. ds-brain decides per \
name: membership and lifecycle, `design.edit` or `transformer.delete`, the \
edit lease, and creator ownership for non-admins. Every name is answered in \
order; one refusal never cancels the others. This is not deletion, which stays \
a destructive paired-application action.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, REASON_ARG, LANE_ARG],
    output: "\
Lane and selected-project identity/status, the requested names and reason, \
applied/failed counts, and one result per name: `applied` with the retirement \
timestamp, or a closed `refusal` (`not_found`, `already_retired`, \
`no_retirement_record`, `special_document`, `governance_locked`, `not_owner`, \
`failed`) with its message.",
    examples: &[Example {
        command: "ds design transformer retire --transformer TX-1 --reason \"superseded by the 2026 survey\" --yes",
        note: "Inspect first with `ds design transformer inventory --transformer TX-1`.",
        runnable: false,
    }],
    refusals: super::NATIVE_WRITE_REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformers = super::transformer_set(inputs, true)?;
    let reason = super::reason(inputs)?;
    let request = RetirementRequest::retire(transformers, reason).map_err(|error| {
        Failure::invalid("invalid_reason", error.to_string()).remedy(super::INVALID_REASON.remedy)
    })?;
    let headless = ds_cli_auth::transformer_retirement(
        inputs.require("lane")?,
        RetirementAction::Retire,
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
    super::render_receipt("retired", data)
}
