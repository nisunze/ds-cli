//! Capture explicit project bindings, then use the ordinary native publisher.
use ds_cli_auth::{
    GridModelsCommand, LinkedGridIntent, LinkedGridParticipant, LinkedGridRetirement,
};
use ds_cli_contract::{Failure, Inputs};
use ds_design_workspace::grid_publication::{self, Request};
use ds_grid_exchange::linked_models;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub model_id: String,
    pub expected_head_revision_id: String,
    pub display_name: Option<String>,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub project: String,
    pub graph_id: String,
    pub expected_generation: Option<u64>,
    pub model_kind: String,
    pub participants: BTreeMap<String, Target>,
    pub retire: Option<LinkedGridRetirement>,
    pub reason: Option<String>,
}

pub fn schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Publication)).expect("typed schema serializes")
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("composite_publication_invalid",message).remedy("capture one explicit project, graph generation and every package-to-project model binding before applying")
}

pub fn prepare(
    inputs: &Inputs,
    bytes: &[u8],
    origin: Option<&[u8]>,
) -> Result<Option<(String, GridModelsCommand, Value)>, Failure> {
    let Some(path) = inputs.value("publication") else {
        return Ok(None);
    };
    let request: Publication =
        serde_json::from_slice(&super::composite::read(path, 8 * 1024 * 1024)?)
            .map_err(|e| invalid(e.to_string()))?;
    let checkpoint = linked_models::decode(bytes).map_err(|e| invalid(e.to_string()))?;
    if request
        .participants
        .keys()
        .ne(checkpoint.state.models.keys())
    {
        return Err(invalid(
            "publication must bind exactly every checkpoint participant",
        ));
    }
    if let Some(retire) = &request.retire {
        let source =
            origin.ok_or_else(|| invalid("only an initial split may retire its original"))?;
        if retire.expected_model_digest != format!("{:x}", Sha256::digest(source)) {
            return Err(invalid(
                "retirement digest differs from the exact split source bytes",
            ));
        }
    }
    let composite = request.participants[&checkpoint.state.graph.composite]
        .model_id
        .clone();
    let mut parts: Vec<_> = checkpoint
        .state
        .graph
        .parts
        .iter()
        .map(|id| request.participants[id].model_id.clone())
        .collect();
    parts.sort();
    let intent = LinkedGridIntent {
        project: request.project.clone(),
        graph_id: request.graph_id,
        expected_generation: request.expected_generation,
        generation: checkpoint.state.graph.generation,
        composite,
        parts,
        canonical_digest: checkpoint.state.graph.canonical_digest,
        retire: request.retire,
    };
    let mut participants = Vec::new();
    for (id, package) in linked_models::exact_packages(bytes).map_err(|e| invalid(e.to_string()))? {
        let target = &request.participants[&id];
        let metadata = Request {
            project: request.project.clone(),
            model_kind: request.model_kind.clone(),
            display_name: target.display_name.clone(),
            reason: request.reason.clone(),
            ..Default::default()
        };
        let native = grid_publication::prepare_linked(
            &metadata,
            &package,
            &target.model_id,
            &target.expected_head_revision_id,
        )
        .map_err(invalid)?;
        participants.push(LinkedGridParticipant {
            intent: native,
            bytes: package,
        });
    }
    let plan = intent
        .plan(&request.project, &participants, bytes)
        .map_err(|e| invalid(e.to_string()))?;
    let summary = json!({"status":"dry_run","project":request.project,"graph_id":intent.graph_id,
        "generation":intent.generation,"expected_generation":intent.expected_generation,"request":plan.request});
    Ok(Some((
        request.project,
        GridModelsCommand::PublishLinked {
            intent: Box::new(intent),
            participants,
            checkpoint: bytes.to_vec(),
        },
        summary,
    )))
}
