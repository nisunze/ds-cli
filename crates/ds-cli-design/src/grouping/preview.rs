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
        crate::group::PROJECTION_TRANSFORMERS_ARG,
        crate::group::PROJECTION_DEFINITION_IDS_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The server plan including tuple groups, source suggestions and plan_digest.",
    examples: &[],
    refusals: &[crate::NOT_PAIRED, crate::REFUSED],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};
pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(&descriptor,&CONSUMER_GROUPING_PREVIEW,json!({"transformers":crate::group::projection_transformers(inputs)?,"definition-ids":crate::group::projection_definition_ids(inputs)?,"bindings":inputs.value("bindings").unwrap_or("[]")}),crate::READ_TIMEOUT).map_err(crate::classify_design_failure)
}
pub fn render(data: &Value) -> String {
    format!(
        "consumer grouping plan {}",
        data["plan_digest"].as_str().unwrap_or("?")
    )
}
