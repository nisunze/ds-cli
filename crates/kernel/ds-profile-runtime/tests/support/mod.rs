use ds_grid_engine::GridSession;
use ds_grid_exchange::{profile_session::ProfileSessionSeed, AssetBytes};
use ds_grid_model::{EntityId, GridPresentationPolicy};
use ds_profile_runtime::{InitializeRequest, RefreshRequest};
use serde_json::Value;

pub fn initialize(
    session: &GridSession,
    model_id: &str,
    scope: &str,
    presentation: GridPresentationPolicy,
    assets: &[AssetBytes],
) -> InitializeRequest {
    let seed = ProfileSessionSeed::capture(
        session,
        EntityId::new(model_id).unwrap(),
        presentation,
        assets,
    );
    InitializeRequest {
        scope: scope.into(),
        publication_id: None,
        seed_json: serde_json::to_string(&seed).unwrap(),
        restore_state: None,
        options: Default::default(),
        style_document: serde_json::from_str(include_str!(
            "../../../ds-command-kernel/tests/fixtures/profile-display-style.json"
        ))
        .unwrap(),
        patch: Default::default(),
        analyze: false,
    }
}

/// Refresh an initialized lease from its exact admitted history. A display-only
/// fixture retains the original seed session; an edit passes its native worker.
pub fn refresh(
    input: &InitializeRequest,
    admitted: &Value,
    worker: Option<&GridSession>,
) -> RefreshRequest {
    let seed: ProfileSessionSeed = serde_json::from_str(&input.seed_json).unwrap();
    let model_id = seed.model_id.to_string();
    let opened = seed.open().unwrap();
    let worker = worker.unwrap_or(&opened.session);
    RefreshRequest {
        scope: input.scope.clone(),
        publication_id: None,
        model_id,
        base_revision: serde_json::from_value(admitted["history"]["model_revision"].clone())
            .unwrap(),
        base_history: serde_json::from_value(admitted["history"].clone()).unwrap(),
        expected_revision: worker.current_revision().revision_id.clone(),
        target_history: worker.history_state(),
        journal_json: serde_json::to_string(&worker.retained_journal()).unwrap(),
        options: input.options.clone(),
        style_document: input.style_document.clone(),
        patch: input.patch.clone(),
        analyze: input.analyze,
        calculated_base_signature: None,
    }
}
