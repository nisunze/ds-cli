//! Bounded native apply evidence, including commands validated in a redo tail.
use ds_grid_engine::{apply_timings::ApplyPhaseTimings, GridSession};
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct ReplayCost {
    entries: usize,
    phases: ApplyPhaseTimings,
}

impl ReplayCost {
    pub fn record(&mut self, session: &GridSession) {
        self.entries += 1;
        if let Some(sample) = session.last_apply_phase_timings() {
            macro_rules! add {
                ($($field:ident),+ $(,)?) => { $(self.phases.$field += sample.$field;)+ };
            }
            add!(
                admission_ms,
                clone_snapshot_ms,
                command_ms,
                stored_geometry_ms,
                validation_ms,
                revision_ms,
                delta_ms,
                commit_ms,
                total_ms
            );
        }
    }

    pub fn append(&self, metrics: &mut Value) {
        metrics["replayed_entries"] = json!(self.entries);
        for (phase, ms) in serde_json::to_value(self.phases)
            .unwrap()
            .as_object()
            .unwrap()
        {
            metrics[format!("replay_apply_{phase}")] = ms.clone();
        }
    }
}
