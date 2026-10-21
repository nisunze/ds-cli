//! Validate the exact package with Network before admitting native publication.
//! Source revision and engine metadata are derived here, never trusted from UI.
use ds_command_kernel::grid_publication::{
    revision_identifier, Approval, CompositionSource, Intent, MigrationSource, MAX_MODEL_BYTES,
};
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::grid::{
    NativeGridPublicationFactsV1, NativeGridPublicationModeV1, NativeGridPublicationRequestV1,
    NativeGridQuerySourcePin, NATIVE_GRID_PUBLICATION_SCHEMA_V1,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub project: String,
    pub model_id: Option<String>,
    pub model_kind: String,
    pub expected_head_revision_id: Option<String>,
    pub display_name: Option<String>,
    pub reason: Option<String>,
    /// Start the next version instead of saving into the current one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bump_version: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_stage_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_level_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<Approval>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operation_summary: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composition_sources: Vec<CompositionSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_source: Option<MigrationSource>,
}

/// A project holds one automatic combined model, in one linked graph. Its
/// binding is derived, never authored: this graph ID, the model ID of
/// [`combined_model_id`] and, when it is created, this display name.
pub const COMBINED_GRAPH_ID: &str = "combined";
pub const COMBINED_DISPLAY_NAME: &str = "Combined model";

/// The project model ID of the engine's combined identity:
/// `combined.<16 hex>` -> `combined-<16 hex>`.
pub fn combined_model_id(identity: &str) -> Result<String, String> {
    if !ds_network_contracts::grid::is_combined_identity(identity) {
        return Err(format!(
            "combined_identity_invalid: {identity} is not a derived combined-model identity"
        ));
    }
    Ok(identity.replacen('.', "-", 1))
}

/// The project revision ID publication assigns to these exact package bytes.
pub fn publication_revision(bytes: &[u8]) -> Result<String, String> {
    let facts = publication_facts(bytes, NativeGridPublicationModeV1::Inspect)?;
    revision_identifier(&facts.source_revision_id)
        .ok_or_else(|| "grid_publication_revision_invalid".to_owned())
}

fn publication_facts(
    bytes: &[u8],
    mode: NativeGridPublicationModeV1,
) -> Result<NativeGridPublicationFactsV1, String> {
    if bytes.is_empty() || bytes.len() > MAX_MODEL_BYTES {
        return Err("model_too_large: expected a nonempty package up to 256 MiB".into());
    }
    let scratch = crate::network_staging::private_stage()?;
    let client = CapturedNetworkClient::capture(1, &|| false).map_err(publication_failure)?;
    let request = NativeGridPublicationRequestV1 {
        schema: NATIVE_GRID_PUBLICATION_SCHEMA_V1.into(),
        source: NativeGridQuerySourcePin {
            package_sha256: format!("{:x}", Sha256::digest(bytes)),
            expected_revision: None,
            engineering_input_root: None,
        },
        mode,
    };
    client
        .grid_publication(&request, bytes, scratch.path(), &|| false)
        .map(|outcome| outcome.result)
        .map_err(publication_failure)
}

fn publication_failure(error: ds_network_client::NetworkFailure) -> String {
    let code = match error.owner_code() {
        Some("package_refused") => "model_invalid",
        Some(code) => code,
        None => error.code(),
    };
    let message = &error.message;
    if message.starts_with(&format!("{code}:")) {
        message.into()
    } else {
        format!("{code}: {message}")
    }
}

/// User publication is a submodel operation. Combined publication belongs
/// exclusively to the reconciled, atomic linked-generation boundary below.
pub fn prepare(request: &Request, bytes: &[u8]) -> Result<Intent, String> {
    prepare_inner(request, bytes, false)
}

fn prepare_inner(
    request: &Request,
    bytes: &[u8],
    reconciled_generation: bool,
) -> Result<Intent, String> {
    if bytes.is_empty() || bytes.len() > MAX_MODEL_BYTES {
        return Err("model_too_large: expected a nonempty package up to 256 MiB".into());
    }
    let mode = if reconciled_generation {
        NativeGridPublicationModeV1::ReconciledGeneration
    } else {
        NativeGridPublicationModeV1::UserPublish
    };
    let facts = publication_facts(bytes, mode)?;
    if request.model_id.is_some() && request.display_name.is_some() {
        return Err(
            "project_model_rename_unsupported: publish cannot rename an existing model".into(),
        );
    }
    if request.model_id.is_none() && request.display_name.is_none() {
        return Err("new_project_model_incomplete: a new model requires a display name".into());
    }
    let expected = match (&request.expected_head_revision_id, &request.model_id) {
        (Some(value), _) => value.clone(),
        (None, None) => String::new(),
        (None, Some(_)) => {
            return Err(
                "publish_expected_head_required: review and name the exact current head".into(),
            );
        }
    };
    if request.model_id.is_some() && expected.is_empty() {
        return Err(
            "publish_expected_head_required: existing models require a nonempty reviewed head"
                .into(),
        );
    }
    let model_id = request.model_id.clone().unwrap_or_else(|| {
        // The creation intent is stable across process interruption. A retry
        // must reuse both object and model identity, not create a duplicate.
        let key = serde_json::to_vec(&json!([
            request.project,
            request.display_name,
            format!("{:x}", Sha256::digest(bytes))
        ]))
        .expect("strings serialize");
        format!("model_{:x}", Sha256::digest(key))
    });
    let intent = Intent {
        project: request.project.clone(),
        model_id,
        model_kind: request.model_kind.clone(),
        expected_head_revision_id: expected,
        manifest_model_id: facts.source.model_id.as_str().into(),
        manifest_model_revision: facts.manifest_model_revision,
        source_revision_id: facts.source_revision_id,
        model_schema_version: facts.model_schema.to_string(),
        engine_version: facts.engine_version,
        display_name: request.display_name.clone(),
        reason: request.reason.clone(),
        bump_version: request.bump_version,
        milestone: request.milestone.clone(),
        description: request.description.clone(),
        design_stage_id: request.design_stage_id.clone(),
        detail_level_id: request.detail_level_id.clone(),
        approval: request.approval.clone(),
        operation_summary: request.operation_summary.clone(),
        composition_sources: request.composition_sources.clone(),
        migration_source: request.migration_source.clone(),
    };
    intent.plan(&request.project, bytes).map_err(|code| {
        // Name the field family a governance refusal is about, so a caller
        // does not have to reverse-engineer which flag carried it.
        let hint = match code {
            "grid_publication_bump_without_head" => {
                ": a new model starts at v1; --bump-version needs an existing head"
            }
            "grid_publication_governance_invalid" => {
                ": stage and detail ids must match ^[a-z0-9][a-z0-9_-]{1,127}$"
            }
            "grid_publication_approval_invalid" => {
                ": approval is draft|submitted|approved|rejected; a decision needs a level id and a reason"
            }
            "grid_publication_operation_summary_invalid" => {
                ": at most 100 trimmed entries of 1..300 characters"
            }
            "grid_publication_composition_invalid" => {
                ": 2..100 distinct sources in order, each model:revision with its sha256"
            }
            "grid_publication_migration_invalid" => {
                ": typed migration evidence is admissible on a model's first revision only"
            }
            "grid_publication_metadata_invalid" => {
                ": text must be trimmed; milestone <= 300, description and reason <= 2000"
            }
            _ => "",
        };
        format!("{code}{hint}")
    })?;
    Ok(intent)
}

/// Linked graphs capture an explicit destination ID even for a new part.
/// Source validation and governance still use the ordinary publication owner.
pub fn prepare_linked(
    request: &Request,
    bytes: &[u8],
    model_id: &str,
    expected_head: &str,
) -> Result<Intent, String> {
    let mut captured = request.clone();
    captured.model_id = (!expected_head.is_empty()).then(|| model_id.to_owned());
    captured.expected_head_revision_id =
        (!expected_head.is_empty()).then(|| expected_head.to_owned());
    let mut intent = prepare_inner(&captured, bytes, true)?;
    intent.model_id = model_id.to_owned();
    intent
        .plan(&request.project, bytes)
        .map_err(str::to_owned)?;
    Ok(intent)
}
