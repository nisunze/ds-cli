//! Explicit native scene-base preparation and calculated geometry transport.
//! Display/analysis payloads remain independently owned projection costs.
use super::*;
use ds_grid_engine::profile_calculated_delta::CalculatedSceneBase;
use ds_grid_engine::SessionHistoryState;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculatedSceneBaseRequest {
    pub scope: String,
    pub model_id: String,
    pub expected_history: SessionHistoryState,
}

#[cfg(test)]
#[path = "calculated_transfer_cost.rs"]
mod cost;
#[cfg(test)]
#[path = "calculated_transfer_tests.rs"]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartDeltaCalculationRequest {
    pub base_signature: String,
    pub calculation: StartCalculationRequest,
}

impl Runtime {
    pub(super) fn calculated_scene_base(
        &mut self,
        request: CalculatedSceneBaseRequest,
    ) -> Result<Value, String> {
        let started = Instant::now();
        if self
            .publications
            .get(&request.scope)
            .is_some_and(|publication| !publication.confirmed)
        {
            return Err("profile_publication_pending: confirm or discard the native result before preparing its scene base".into());
        }
        let held = self.held.get_mut(&request.scope).ok_or(
            "profile_calculated_delta_base_unavailable: initialize this model scope first",
        )?;
        if held.source.model_id != request.model_id {
            return Err(
                "profile_replay_model_mismatch: scene base belongs to another model".into(),
            );
        }
        if held.session.history_state() != request.expected_history {
            return Err(
                "profile_replay_stale: scene base history differs from the observed model".into(),
            );
        }
        let rebuilt = held.calculated_base.is_none();
        if rebuilt {
            let scene = held.calculated_scene.as_ref()
                .ok_or("profile_calculated_delta_base_unavailable: explicit full calculated projection is required after case selection or unavailable projection")?;
            held.calculated_base = Some(Arc::new(CalculatedSceneBase::new(scene.as_ref().clone())));
        }
        let base = held.calculated_base.as_ref().unwrap();
        Ok(
            json!({"scope":request.scope,"model_id":held.source.model_id,
            "history":held.session.history_state(),"computed_revision":base.scene().model_revision,
            "engineering_input_root":base.scene().engineering_input_root,
            "axis_pin_digest":base.scene().axis_pin.digest,"base_signature":base.signature(),
            "metrics":{"base_prepare_ms":started.elapsed().as_secs_f64()*1000.,
                "base_rebuilt":usize::from(rebuilt),"calculated_cases":0}}),
        )
    }

    pub(super) fn require_calculated_base(
        &self,
        scope: &str,
        model: &str,
        signature: &str,
    ) -> Result<(), String> {
        if signature.len() != 71
            || !signature.starts_with("sha256:")
            || !signature[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("invalid_profile_view: invalid calculated scene base signature".into());
        }
        let held = self.held.get(scope)
            .ok_or("profile_calculated_delta_base_unavailable: prepare the retained native scene base first")?;
        if held.source.model_id != model {
            return Err(
                "profile_replay_model_mismatch: scene delta belongs to another model".into(),
            );
        }
        let base = held.calculated_base.as_ref()
            .ok_or("profile_calculated_delta_base_unavailable: prepare the retained native scene base first")?;
        if base.signature() != signature {
            return Err("profile_calculated_delta_stale: retained calculated scene changed".into());
        }
        Ok(())
    }

    pub(super) fn transfer_calculated_scene(
        &mut self,
        scope: &str,
        scene: ds_grid_engine::ProfileAtlasScene,
    ) -> Result<(Value, Option<Value>, Value), String> {
        let started = Instant::now();
        let requested = self.delta_transfers.remove(scope);
        let held = self.held.get_mut(scope).expect("admitted projected scope");
        let (wire, delta, bytes) = if let Some(signature) = requested {
            let base = held
                .calculated_base
                .as_ref()
                .filter(|base| base.signature() == signature)
                .ok_or("profile_calculated_delta_stale: calculation lost its exact scene base")?;
            let (next, delta) = base.delta_to(scene)?;
            let bytes = serde_json::to_vec(&delta)
                .map_err(|error| error.to_string())?
                .len();
            let delta = serde_json::to_value(delta).map_err(|error| error.to_string())?;
            held.calculated_base = Some(Arc::new(next));
            held.calculated_scene = None;
            (Value::Null, Some(delta), bytes)
        } else {
            let scene = Arc::new(scene);
            let wire = serde_json::to_value(scene.as_ref()).map_err(|error| error.to_string())?;
            held.calculated_scene = Some(scene);
            held.calculated_base = None;
            // Full payload byte encoding remains with the host. Do not add a
            // second complete serialization solely for disposable telemetry.
            (wire, None, 0)
        };
        let metrics = json!({"scene_transfer_ms":started.elapsed().as_secs_f64()*1000.,
            "calculated_delta_bytes":bytes,"calculated_delta":usize::from(delta.is_some())});
        Ok((wire, delta, metrics))
    }
}
