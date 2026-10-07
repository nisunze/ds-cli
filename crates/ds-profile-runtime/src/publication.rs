//! Explicit admission of disposable Profile results. Authored edits remain in
//! their worker session even when a speculative native result is discarded.
use super::*;
use ds_grid_engine::{RevisionId, SessionHistoryState};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationPin {
    pub publication_id: String,
    pub model_id: String,
    pub history: SessionHistoryState,
    pub computed_revision: Option<RevisionId>,
    pub axis_pin_digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmPublicationRequest {
    pub scope: String,
    pub publication: PublicationPin,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscardPublicationRequest {
    pub scope: String,
    pub model_id: String,
    /// Captured before transport, so a lost response can still be discarded.
    pub publication_id: String,
    pub target_history: SessionHistoryState,
}

struct Checkpoint {
    held: Option<HeldView>,
    state: Option<DisplayState>,
    analysis: Option<Value>,
}

pub(super) struct PendingPublication {
    pub(super) pin: PublicationPin,
    checkpoint: Checkpoint,
    pub(super) confirmed: bool,
}

impl Runtime {
    pub(super) fn execute_with_publication(&mut self, request: Request) -> Result<Value, String> {
        let immediate = None;
        let binding = match &request {
            Request::Initialize(r) => Some((&r.scope, &r.publication_id)),
            Request::Refresh(r) | Request::Observe(r) | Request::Present(r) => {
                Some((&r.scope, &r.publication_id))
            }
            Request::AdmitCalculation(r) => Some((&r.scope, &r.publication_id)),
            Request::Project(r) => Some((&r.scope, &immediate)),
            _ => None,
        };
        if let Some((scope, _)) = binding {
            if self.publications.get(scope).is_some_and(|p| !p.confirmed) {
                return Err("profile_publication_pending: confirm or discard the outstanding native result before requesting another".into());
            }
        }
        let staging =
            binding.and_then(|(scope, id)| id.as_ref().map(|id| (scope.clone(), id.clone())));
        let Some((scope, publication_id)) = staging else {
            // A new calculation or weather action retires a confirmed rollback
            // checkpoint; it cannot bypass an unconfirmed candidate.
            let retire = if let Request::CaseLayer(r) = &request {
                if self
                    .publications
                    .get(&r.scope)
                    .is_some_and(|p| !p.confirmed)
                {
                    return Err("profile_publication_pending: admit the outstanding native result before selecting a case".into());
                }
                r.select.then(|| r.scope.clone())
            } else if let Some((scope, _)) = binding {
                Some(scope.clone())
            } else {
                None
            };
            let result = self.execute_inner(request);
            if result.is_ok() {
                if let Some(scope) = retire {
                    self.publications.remove(&scope);
                }
            }
            return result;
        };
        if publication_id.is_empty() || publication_id.len() > 128 {
            return Err("invalid_profile_view: invalid publication identity".into());
        }
        if self
            .publications
            .get(&scope)
            .is_some_and(|p| p.pin.publication_id == publication_id)
        {
            return Err(
                "profile_publication_stale: a publication identity cannot be reused".into(),
            );
        }
        let started = std::time::Instant::now();
        let checkpoint = Checkpoint {
            // The large calculated and authored geometry are immutable Arcs;
            // local staging shares them rather than cloning their dense fields.
            held: self.held.get(&scope).cloned(),
            state: self.views.get(&scope).cloned(),
            analysis: self.analyses.get(&scope).cloned(),
        };
        let checkpoint_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut result = match self.execute_inner(request) {
            Ok(result) => result,
            Err(error) => {
                self.restore_publication_checkpoint(&scope, checkpoint);
                return Err(error);
            }
        };
        let Some(held) = self.held.get(&scope) else {
            self.restore_publication_checkpoint(&scope, checkpoint);
            return Err("profile_publication_unavailable: result has no native held view".into());
        };
        let computed = held.review.base_scene();
        let pin = PublicationPin {
            publication_id,
            model_id: held.source.model_id.clone(),
            history: held.session.history_state(),
            computed_revision: computed.map(|scene| scene.model_revision.clone()),
            axis_pin_digest: computed.map(|scene| scene.axis_pin.digest.clone()),
        };
        self.publications.insert(
            scope,
            PendingPublication {
                pin: pin.clone(),
                checkpoint,
                confirmed: false,
            },
        );
        result["publication"] = json!(pin);
        if !result["metrics"].is_object() {
            result["metrics"] = json!({});
        }
        result["metrics"]["publication_checkpoint_ms"] = json!(checkpoint_ms);
        Ok(result)
    }

    pub(super) fn confirm_publication(
        &mut self,
        request: ConfirmPublicationRequest,
    ) -> Result<Value, String> {
        let pending = self
            .publications
            .get_mut(&request.scope)
            .ok_or("profile_publication_unavailable: no staged result exists in this scope")?;
        if request.publication != pending.pin {
            return Err("profile_publication_stale: history, calculated revision or axis differs from the staged result".into());
        }
        pending.confirmed = true;
        // Keep one checkpoint until the next request. A host can still discard
        // if its renderer refuses after worker admission; never unwind edits.
        Ok(json!({"publication":pending.pin,"confirmed":true}))
    }

    pub(super) fn discard_publication(
        &mut self,
        request: DiscardPublicationRequest,
    ) -> Result<Value, String> {
        let pending = self
            .publications
            .get(&request.scope)
            .ok_or("profile_publication_unavailable: no staged result exists in this scope")?;
        if request.model_id != pending.pin.model_id
            || request.publication_id != pending.pin.publication_id
            || request.target_history != pending.pin.history
        {
            return Err(
                "profile_publication_stale: discard differs from the outstanding model and history"
                    .into(),
            );
        }
        let pending = self
            .publications
            .remove(&request.scope)
            .expect("checked publication");
        self.restore_publication_checkpoint(&request.scope, pending.checkpoint);
        Ok(
            json!({"model_id":request.model_id,"publication_id":request.publication_id,
            "discarded":true,"history":self.held.get(&request.scope).map(|held| held.session.history_state())}),
        )
    }

    fn restore_publication_checkpoint(&mut self, scope: &str, checkpoint: Checkpoint) {
        match checkpoint.held {
            Some(held) => {
                self.held.insert(scope.into(), held);
            }
            None => {
                self.held.remove(scope);
            }
        }
        match checkpoint.state {
            Some(state) => {
                self.views.insert(scope.into(), state);
            }
            None => {
                self.views.remove(scope);
            }
        }
        match checkpoint.analysis {
            Some(analysis) => {
                self.analyses.insert(scope.into(), analysis);
            }
            None => {
                self.analyses.remove(scope);
            }
        }
    }
}
