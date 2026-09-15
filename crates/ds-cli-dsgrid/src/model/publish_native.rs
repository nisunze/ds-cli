//! File and argument adapter for the shared native publication owner.
use ds_cli_contract::{Inputs, outcome::Failure};
use ds_design_workspace::grid_publication::{self, Request};
use serde_json::Value;

pub fn run(inputs: &Inputs, path: &str) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let bytes = crate::package::read_bytes(path)?;
    let request = Request {
        project: project.into(),
        model_id: inputs.value("project-model").map(str::to_owned),
        model_kind: inputs.require("kind")?.into(),
        expected_head_revision_id: inputs.value("expected-head").map(str::to_owned),
        display_name: inputs.value("name").map(str::to_owned),
        reason: inputs.value("reason").map(str::to_owned),
    };
    let intent = grid_publication::prepare(&request, &bytes).map_err(|message| {
        let failure = match message.split(':').next().unwrap_or("") {
            "publish_expected_head_required" => {
                Failure::invalid("publish_expected_head_required", message)
            }
            "model_too_large" => Failure::invalid("model_too_large", message),
            _ => Failure::invalid("model_invalid", message),
        };
        failure.remedy("inspect the package and exact publication inputs before retrying")
    })?;
    ds_cli_auth::grid_models_for_project(
        inputs.value("lane").unwrap_or("stable"),
        project,
        &ds_cli_auth::GridModelsCommand::Publish { intent, bytes },
    )
    .map(|receipt| receipt.data)
}
