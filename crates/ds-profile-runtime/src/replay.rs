use super::*;
use ds_grid_engine::{CommandEnvelope, JournalEntry, RevisionId, SessionHistoryState};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitializeRequest {
    pub scope: String,
    pub seed: ds_grid_exchange::profile_session::ProfileSessionSeed,
    pub options: ProfileAtlasOptions,
    pub style_document: Value,
    pub patch: DisplayPatch,
    pub analyze: bool,
}
impl From<InitializeRequest> for Request {
    fn from(value: InitializeRequest) -> Self {
        Self::Initialize(Box::new(value))
    }
}

/// A model-bound, head-fenced refresh. Entries include the worker's redo tail;
/// target_history pins its exact cursor. Empty entries restore the opened state.
/// Native receipts alone advance disposable demand caches. Exact entry identity
/// distinguishes different cursors that happen to have equal authored content.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshRequest {
    pub scope: String,
    pub model_id: String,
    pub base_revision: RevisionId,
    pub base_history: SessionHistoryState,
    pub expected_revision: RevisionId,
    pub target_history: SessionHistoryState,
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
    pub(super) fn initialize(&mut self, request: InitializeRequest) -> Result<Value, String> {
        if request.scope.is_empty() || request.scope.len() > 1024 {
            return Err("invalid_profile_view: invalid initialization scope".into());
        }
        if self.held.contains_key(&request.scope) {
            return Err(
                "profile_replay_scope_initialized: use a fresh scope for initialization".into(),
            );
        }
        let initialized = request.seed.open()?;
        let project = ProjectRequest {
            scope: request.scope,
            expected_revision: initialized
                .session
                .current_revision()
                .revision_id
                .to_string(),
            package_bytes: vec![],
            options: request.options,
            style_document: request.style_document,
            patch: request.patch,
            analyze: false,
        };
        self.render_replayed(
            project,
            initialized.session,
            HeldSource {
                model_id: initialized.model_id.to_string(),
                presentation: initialized.presentation,
                assets: Arc::new(initialized.assets),
            },
            Default::default(),
            request.analyze,
        )
    }
    pub(super) fn refresh(&mut self, request: RefreshRequest) -> Result<Value, String> {
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
        if held.session.current_revision().revision_id != request.base_revision
            || held.session.history_state() != request.base_history
        {
            return Err(
                "profile_replay_stale: held revision or history differs from the replay base"
                    .into(),
            );
        }
        if request.entries.len() > 16_384 {
            return Err("profile_replay_invalid: journal exceeds native bounds".into());
        }
        if request
            .entries
            .windows(2)
            .any(|pair| pair[0].resulting_revision != pair[1].parent_revision)
            || request.target_history.model_revision != request.expected_revision
            || request
                .target_history
                .undo_depth
                .checked_add(request.target_history.redo_depth)
                != Some(request.entries.len())
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
        let staged = session.history_state();
        session
            .discard_edits_at_head(&ds_grid_engine::SessionDiscardRequest {
                expected_revision: staged.model_revision,
                expected_undo_depth: staged.undo_depth,
                expected_history_pin: staged.history_pin,
                target_revision: request.expected_revision.clone(),
                target_undo_depth: request.target_history.undo_depth,
                target_redo_depth: request.target_history.redo_depth,
                target_history_pin: request.target_history.history_pin.clone(),
            })
            .map_err(|error| format!("profile_replay_diverged: {error}"))?;
        if session.history_state() != request.target_history {
            return Err("profile_replay_diverged: staged cursor or history differs from the requested state".into());
        }
        // Preflight options/styles before transferring projection cache ownership.
        // A failed refresh is a transaction refusal, including Analyze requests;
        // the previous display, session and analysis stay admitted together.
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
        self.render_replayed(project, session, source, cache, request.analyze)
    }
    fn render_replayed(
        &mut self,
        mut project: ProjectRequest,
        session: GridSession,
        source: HeldSource,
        cache: ds_grid_engine::StructureUsageScreeningCache,
        analyze: bool,
    ) -> Result<Value, String> {
        if analyze {
            project.patch.visibility.insert(
                ds_command_kernel::profile_display::DisplayOption::Review,
                true,
            );
        }
        let state = self
            .views
            .get(&project.scope)
            .cloned()
            .unwrap_or_default()
            .patched(&project.patch)?;
        resolve(&state, &project.style_document)?;
        review_status_colors(&project.style_document)?;
        review_box_ink(&project.style_document)?;
        let scope = project.scope.clone();
        // Projection refusal restores the old held cache in project_session.
        // Analyze is run only after that projection has been admitted.
        let mut result = self.project_session(project, session, source, cache)?;
        if analyze {
            let held = self.held.get_mut(&scope).expect("admitted projection");
            let analysis = analysis::analyze_with_cache(
                &held.session,
                &held.options.display_case,
                held.review.base_scene(),
                None,
                &mut held.demand_cache,
            );
            let state = self.views[&scope].clone();
            result["command_view"] = command_view::project(
                &state,
                Some(&held.rows),
                held.review.base_scene(),
                Some(&analysis),
                !held.session.snapshot().structures.is_empty(),
            );
            self.analyses.insert(scope, analysis.clone());
            result["analysis"] = analysis;
            result["ran"] = json!(true);
        }
        Ok(result)
    }
}
