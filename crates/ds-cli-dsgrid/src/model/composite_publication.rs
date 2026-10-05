//! Capture explicit submodel bindings, derive the combined model's, then use
//! the ordinary native publisher.
use ds_cli_auth::{
    GridModelsCommand, LinkedGridIntent, LinkedGridParticipant, LinkedGridRetirement,
};
use ds_cli_contract::{Failure, Inputs};
use ds_design_workspace::grid_publication::{self, Request};
use ds_grid_exchange::linked_models::{Encoded, LinkedCheckpoint};
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
/// The project binding of one linked generation. Only submodels are bound:
/// the combined model's binding is derived (graph `combined`, model
/// `combined-<16 hex>` of its identity, display name "Combined model" when
/// created, and its expected head from the reconciled checkpoint).
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub project: String,
    pub expected_generation: Option<u64>,
    pub model_kind: String,
    /// Every submodel's package identity -> project model and captured head.
    pub participants: BTreeMap<String, Target>,
    pub retire: Option<LinkedGridRetirement>,
    pub reason: Option<String>,
}

pub fn schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Publication)).expect("typed schema serializes")
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("composite_publication_invalid", message).remedy(
        "capture one explicit project, graph generation and every submodel-to-project model binding before applying",
    )
}

/// The checkpoint a generation is published from, and how it was produced.
pub struct Candidate<'a> {
    pub checkpoint: &'a LinkedCheckpoint,
    pub encoded: &'a Encoded,
    /// Exact source bytes of a split; only an initial split retires them.
    pub origin: Option<&'a [u8]>,
    /// Exact combined package of the reconciled previous generation.
    pub previous_combined: Option<&'a [u8]>,
}

pub fn prepare(
    inputs: &Inputs,
    candidate: &Candidate<'_>,
) -> Result<Option<(String, GridModelsCommand, Value)>, Failure> {
    let Some(path) = inputs.value("publication") else {
        return Ok(None);
    };
    let request: Publication =
        serde_json::from_slice(&super::composite::read(path, 8 * 1024 * 1024)?)
            .map_err(|e| invalid(e.to_string()))?;
    let graph = &candidate.checkpoint.state.graph;
    if request.participants.contains_key(&graph.composite) {
        return Err(invalid(
            "the combined model's binding is derived; bind only the submodels",
        ));
    }
    if request.participants.keys().ne(graph.parts.iter()) {
        return Err(invalid(
            "publication must bind exactly every submodel of the checkpoint",
        ));
    }
    if let Some(retire) = &request.retire {
        let source = candidate
            .origin
            .ok_or_else(|| invalid("only an initial split may retire its original"))?;
        if retire.expected_model_digest != format!("{:x}", Sha256::digest(source)) {
            return Err(invalid(
                "retirement digest differs from the exact split source bytes",
            ));
        }
    }
    let combined_model = grid_publication::combined_model_id(&graph.composite).map_err(invalid)?;
    let combined = match request.expected_generation {
        None => Target {
            model_id: combined_model.clone(),
            expected_head_revision_id: String::new(),
            display_name: Some(grid_publication::COMBINED_DISPLAY_NAME.into()),
        },
        Some(_) => Target {
            model_id: combined_model.clone(),
            expected_head_revision_id: grid_publication::publication_revision(
                candidate.previous_combined.ok_or_else(|| {
                    invalid("a later generation is published from the checkpoint it reconciles (--bundle)")
                })?,
            )
            .map_err(invalid)?,
            display_name: None,
        },
    };
    let mut parts: Vec<_> = graph
        .parts
        .iter()
        .map(|id| request.participants[id].model_id.clone())
        .collect();
    parts.sort();
    let intent = LinkedGridIntent {
        project: request.project.clone(),
        graph_id: grid_publication::COMBINED_GRAPH_ID.into(),
        expected_generation: request.expected_generation,
        generation: graph.generation,
        composite: combined_model,
        parts,
        canonical_digest: graph.canonical_digest.clone(),
        retire: request.retire,
    };
    let mut participants = Vec::new();
    let mut bindings = BTreeMap::new();
    for (id, package) in &candidate.encoded.packages {
        let target = request.participants.get(id).unwrap_or(&combined);
        let metadata = Request {
            project: request.project.clone(),
            model_kind: request.model_kind.clone(),
            display_name: target.display_name.clone(),
            reason: request.reason.clone(),
            ..Default::default()
        };
        let native = grid_publication::prepare_linked(
            &metadata,
            package,
            &target.model_id,
            &target.expected_head_revision_id,
        )
        .map_err(invalid)?;
        bindings.insert(
            id.clone(),
            json!({"model_id": target.model_id, "expected_head_revision_id": target.expected_head_revision_id,
                "derived": id == &graph.composite}),
        );
        participants.push(LinkedGridParticipant {
            intent: native,
            bytes: package.clone(),
        });
    }
    let plan = intent
        .plan(&request.project, &participants, &candidate.encoded.bytes)
        .map_err(|e| invalid(e.to_string()))?;
    // An unchanged participant keeps its head: its package is its head's
    // exact bytes, so its revision is the captured head and nothing is written.
    for version in &plan.versions {
        let binding = bindings
            .values_mut()
            .find(|binding| binding["model_id"] == version.intent.model_id.as_str())
            .expect("every planned version is a bound participant");
        binding["revision_id"] = json!(version.revision_id);
        binding["action"] = json!(if version.intent.expected_head_revision_id.is_empty() {
            "create"
        } else if version.intent.expected_head_revision_id == version.revision_id {
            "keep"
        } else {
            "new_revision"
        });
    }
    let summary = json!({"status":"dry_run","project":request.project,"graph_id":intent.graph_id,
        "generation":intent.generation,"expected_generation":intent.expected_generation,
        "bindings":bindings,"request":plan.request});
    Ok(Some((
        request.project,
        GridModelsCommand::PublishLinked {
            intent: Box::new(intent),
            participants,
            checkpoint: candidate.encoded.bytes.clone(),
        },
        summary,
    )))
}
