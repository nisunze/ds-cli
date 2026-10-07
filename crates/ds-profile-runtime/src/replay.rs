use super::*;
use ds_grid_engine::{CommandEnvelope, JournalEntry, RevisionId};

/// A model-bound, head-fenced refresh. Entries are the worker's active journal
/// prefix from the initialized package. Empty entries restore that opened state.
/// Native receipts alone advance disposable demand caches. Exact entry identity
/// distinguishes different cursors that happen to have equal authored content.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshRequest {
    pub scope: String,
    pub model_id: String,
    pub base_revision: RevisionId,
    pub expected_revision: RevisionId,
    pub entries: Vec<JournalEntry>,
    pub options: ProfileAtlasOptions,
    pub style_document: Value,
    pub patch: DisplayPatch,
    pub analyze: bool,
}
impl From<RefreshRequest> for Request {
    fn from(value: RefreshRequest) -> Self {
        Self::Refresh(Box::new(value))
    }
}

impl Runtime {
    pub(super) fn refresh(&mut self, mut request: RefreshRequest) -> Result<Value, String> {
        let held = self
            .held
            .get(&request.scope)
            .ok_or("profile_replay_base_unavailable: initialize this model scope first")?;
        if request.model_id != held.source.model_id {
            return Err(
                "profile_replay_model_mismatch: initialize the replacement model in a new scope"
                    .into(),
            );
        }
        if held.session.current_revision().revision_id != request.base_revision {
            return Err("profile_replay_stale: held head differs from the replay base".into());
        }
        if request.entries.len() > 16_384 {
            return Err("profile_replay_invalid: journal exceeds native bounds".into());
        }
        if request
            .entries
            .windows(2)
            .any(|pair| pair[0].resulting_revision != pair[1].parent_revision)
            || request
                .entries
                .last()
                .is_some_and(|entry| entry.resulting_revision != request.expected_revision)
        {
            return Err(
                "profile_replay_diverged: journal does not form the requested revision chain"
                    .into(),
            );
        }
        let mut session = held.session.clone();
        let mut cache = held.demand_cache.clone();
        let initial = session.history_state().initial_revision;
        if request
            .entries
            .first()
            .is_some_and(|entry| entry.parent_revision != initial)
        {
            return Err("profile_replay_history_unavailable: journal does not start at the initialized model".into());
        }
        while session.redo().is_ok() {}
        let retained = session.command_history();
        let start = retained
            .iter()
            .zip(&request.entries)
            .take_while(|(a, b)| a == b)
            .count();
        // Choose the exact common command prefix, never a content-only match:
        // setting a value back to its original value can repeat a revision id.
        while session.history_state().undo_depth > start {
            session.undo().expect("retained prefix exists");
        }
        if start < request.entries.len() {
            for entry in &request.entries[start..] {
                if session.current_revision().revision_id != entry.parent_revision {
                    return Err(
                        "profile_replay_diverged: journal parent differs from the staged head"
                            .into(),
                    );
                }
                let outcome = session
                    .apply_command(CommandEnvelope::new(
                        entry.command_id.clone(),
                        entry.parent_revision.clone(),
                        entry.command.clone(),
                    ))
                    .map_err(|error| format!("profile_replay_diverged: {error}"))?;
                if outcome.idempotent_replay
                    || outcome.revision.revision_id != entry.resulting_revision
                {
                    return Err(
                        "profile_replay_diverged: native command result differs from journal"
                            .into(),
                    );
                }
                cache.advance_command(&outcome);
            }
        }
        if session.current_revision().revision_id != request.expected_revision {
            return Err(
                "profile_replay_diverged: staged head differs from the requested head".into(),
            );
        }
        // Preflight options/styles before transferring projection cache ownership.
        // A failed refresh is a transaction refusal, including Analyze requests;
        // the previous display, session and analysis stay admitted together.
        if request.analyze {
            request.patch.visibility.insert(
                ds_command_kernel::profile_display::DisplayOption::Review,
                true,
            );
        }
        let state = self
            .views
            .get(&request.scope)
            .cloned()
            .unwrap_or_default()
            .patched(&request.patch)?;
        resolve(&state, &request.style_document)?;
        review_status_colors(&request.style_document)?;
        review_box_ink(&request.style_document)?;
        let source = held.source.clone();
        let project = ProjectRequest {
            scope: request.scope.clone(),
            expected_revision: request.expected_revision.to_string(),
            package_bytes: vec![],
            options: request.options,
            style_document: request.style_document,
            patch: request.patch,
            analyze: false,
        };
        // Projection refusal restores the old held cache in project_session.
        // Analyze is run only after that projection has been admitted.
        let mut result = self.project_session(project, session, source, cache)?;
        if request.analyze {
            let held = self
                .held
                .get_mut(&request.scope)
                .expect("admitted projection");
            let analysis = analysis::analyze_with_cache(
                &held.session,
                &held.options.display_case,
                held.review.base_scene(),
                None,
                &mut held.demand_cache,
            );
            let state = self.views[&request.scope].clone();
            result["command_view"] = command_view::project(
                &state,
                Some(&held.rows),
                held.review.base_scene(),
                Some(&analysis),
                !held.session.snapshot().structures.is_empty(),
            );
            self.analyses.insert(request.scope, analysis.clone());
            result["analysis"] = analysis;
            result["ran"] = json!(true);
        }
        Ok(result)
    }
}
