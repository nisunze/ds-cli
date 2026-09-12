use crate::{CONSUMER_GROUPING_PREVIEW, DESCRIPTOR_ARG};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
pub static COMMAND: Command = Command {
    id: "design.consumer-grouping.preview",
    path: &["design", "consumer-grouping", "preview"],
    contract: 1,
    summary: "Preview typed Solar report grouping.",
    purpose: "Builds the digest-bound solar_report grouping plan from explicit definition ids and optional source bindings.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        crate::grouping::PURPOSE_ARG,
        crate::group::PROJECTION_TRANSFORMERS_ARG,
        crate::grouping::DEFINITION_IDS_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The server plan including tuple groups, source suggestions and plan_digest.",
    examples: &[],
    refusals: &[
        crate::NOT_PAIRED,
        crate::PROJECT_NOT_OPEN,
        crate::REFUSED,
        crate::INVALID_VALUE_LIST,
        crate::TOO_MANY,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};
pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    // Every caller-controlled list is bounded before pairing: an empty or
    // over-long selection is the caller's own answer, and discovering the
    // application first would report it as a pairing state instead.
    let arguments = json!({
        "purpose": crate::grouping::purpose(inputs)?,
        "transformers": crate::group::projection_transformers(inputs)?,
        "definition-ids": crate::grouping::definition_ids(inputs)?,
        "bindings": inputs.value("bindings").unwrap_or("[]"),
    });
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &CONSUMER_GROUPING_PREVIEW,
        arguments,
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}
pub fn render(data: &Value) -> String {
    format!(
        "consumer grouping plan {}",
        data["plan_digest"].as_str().unwrap_or("?")
    )
}
