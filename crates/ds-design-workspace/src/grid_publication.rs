//! Validate the exact package with Network before admitting native publication.
//! Source revision and engine metadata are derived here, never trusted from UI.
use ds_command_kernel::grid_publication::{
    Approval, CompositionSource, Intent, MigrationSource, MAX_MODEL_BYTES,
};
use ds_grid_engine::GridSession;
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

pub fn prepare(request: &Request, bytes: &[u8]) -> Result<Intent, String> {
    if bytes.is_empty() || bytes.len() > MAX_MODEL_BYTES {
        return Err("model_too_large: expected a nonempty package up to 256 MiB".into());
    }
    let decoded =
        ds_grid_exchange::package::unpack(bytes).map_err(|e| format!("model_invalid: {e}"))?;
    let validation = ds_grid_model::validate_snapshot(&decoded.snapshot);
    if !validation.is_valid() {
        return Err(format!(
            "model_invalid: {} validation findings; inspect the source before publication",
            validation.issues.len()
        ));
    }
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
            )
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
        manifest_model_id: decoded.manifest.model.model_id.as_str().into(),
        manifest_model_revision: decoded.manifest.model.model_revision,
        source_revision_id: GridSession::open(decoded.snapshot)
            .current_revision()
            .revision_id
            .as_str()
            .into(),
        model_schema_version: ds_grid_model::SCHEMA_VERSION.to_string(),
        engine_version: ds_grid_engine::ENGINE_VERSION.into(),
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
