use super::*;
use ds_grid_engine::{CommandEnvelope, RevisionId, SessionHistoryState};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitializeRequest {
    pub scope: String,
    /// Exact native JSON; JS object transport loses signed zero and revision identity.
    pub seed_json: String,
    /// Pass through a prior admitted native state after runtime loss. Rust
    /// validates it with the ordinary patch controls before applying new input.
    #[serde(default)]
    pub restore_state: Option<DisplayState>,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRequest {
    pub scope: String,
    pub model_id: String,
}
impl From<ReleaseRequest> for Request {
    fn from(value: ReleaseRequest) -> Self {
        Self::Release(value)
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
    pub journal_json: String,
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
    pub(super) fn present(&mut self, request: RefreshRequest) -> Result<Value, String> {
        let held = self
            .held
            .get(&request.scope)
            .ok_or("profile_replay_base_unavailable: initialize this model scope first")?;
        let state = self.views[&request.scope].patched(&request.patch)?;
        let options = project_options(request.options.clone(), &state)?;
        let mut allowed = held.options.clone();
        allowed.display_case = options.display_case.clone();
        // Rendering controls cannot quietly turn an outstanding edit or a
        // changed coordinate interpretation into an engineering calculation.
        if request.analyze
            || request.target_history != held.session.history_state()
            || held
                .review
                .base_scene()
                .is_none_or(|scene| scene.model_revision != request.expected_revision)
            || options != allowed
            || held.review.prepared_layer(&options.display_case).is_none()
        {
            return Err("profile_scene_unavailable: explicit Refresh or Analyze is required before presenting changed calculation inputs".into());
        }
        self.refresh(request, true)
    }
    pub(super) fn release(&mut self, request: ReleaseRequest) -> Result<Value, String> {
        if request.scope.is_empty() || request.scope.len() > 1024 || request.model_id.is_empty() {
            return Err("invalid_profile_view: invalid release identity".into());
        }
        if self
            .held
            .get(&request.scope)
            .is_some_and(|held| held.source.model_id != request.model_id)
        {
            return Err("profile_replay_model_mismatch: release belongs to another model".into());
        }
        let released = self.views.remove(&request.scope).is_some();
        self.held.remove(&request.scope);
        self.analyses.remove(&request.scope);
        Ok(json!({"scope":request.scope, "model_id":request.model_id, "released":released}))
    }
    pub(super) fn initialize(&mut self, request: InitializeRequest) -> Result<Value, String> {
        if request.scope.is_empty() || request.scope.len() > 1024 {
            return Err("invalid_profile_view: invalid initialization scope".into());
        }
        if self.held.contains_key(&request.scope) {
            return Err(
                "profile_replay_scope_initialized: use a fresh scope for initialization".into(),
            );
        }
        self.ensure_scope_available(&request.scope)?;
        let restoration = request
            .restore_state
            .map(|state| {
                let patch: DisplayPatch =
                    serde_json::from_value(serde_json::to_value(state).unwrap())
                        .map_err(|error| format!("invalid_profile_view: {error}"))?;
                DisplayState::default().patched(&patch)
            })
            .transpose()?;
        if request.seed_json.len() > 1024 * 1024 * 1024 {
            return Err("profile_seed_invalid: seed exceeds native transport bounds".into());
        }
        let seed: ds_grid_exchange::profile_session::ProfileSessionSeed =
            serde_json::from_str(&request.seed_json)
                .map_err(|error| format!("profile_seed_invalid: {error}"))?;
        let initialized = seed.open()?;
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
        let scope = project.scope.clone();
        let prior = restoration.map(|state| self.views.insert(scope.clone(), state));
        let result = self.render_replayed(
            project,
            initialized.session,
            HeldSource {
                model_id: initialized.model_id.to_string(),
                presentation: initialized.presentation,
                assets: Arc::new(initialized.assets),
            },
            Default::default(),
            request.analyze,
            &[],
        );
        if result.is_err() {
            if let Some(previous) = prior {
                if let Some(state) = previous {
                    self.views.insert(scope, state);
                } else {
                    self.views.remove(&scope);
                }
            }
        }
        result
    }
    pub(super) fn refresh(
        &mut self,
        request: RefreshRequest,
        calculate: bool,
    ) -> Result<Value, String> {
        let started = std::time::Instant::now();
        if !calculate
            && (request.analyze
                || serde_json::to_value(&request.patch).unwrap()
                    != serde_json::to_value(DisplayPatch::default()).unwrap())
        {
            return Err(
                "invalid_profile_view: observe retains display controls and cannot analyze".into(),
            );
        }
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
        if request.journal_json.len() > 64 * 1024 * 1024 {
            return Err("profile_replay_invalid: journal exceeds native transport bounds".into());
        }
        let journal: ds_grid_engine::journal::SessionJournal =
            serde_json::from_str(&request.journal_json)
                .map_err(|error| format!("profile_replay_invalid: {error}"))?;
        if journal.history != request.target_history {
            return Err(
                "profile_replay_diverged: journal cursor differs from requested history".into(),
            );
        }
        let entries = journal.entries;
        let journal_ms = started.elapsed().as_secs_f64() * 1000.0;
        if entries.len() > 16_384 {
            return Err("profile_replay_invalid: journal exceeds native bounds".into());
        }
        if entries
            .windows(2)
            .any(|pair| pair[0].resulting_revision != pair[1].parent_revision)
            || request.target_history.model_revision != request.expected_revision
            || request
                .target_history
                .undo_depth
                .checked_add(request.target_history.redo_depth)
                != Some(entries.len())
        {
            return Err(
                "profile_replay_diverged: journal does not form the requested revision chain"
                    .into(),
            );
        }
        let mut session = held.session.clone();
        let replay_started = std::time::Instant::now();
        let mut cache = held.demand_cache.clone();
        let initial = session.history_state().initial_revision;
        if entries
            .first()
            .is_some_and(|entry| entry.parent_revision != initial)
        {
            return Err("profile_replay_history_unavailable: journal does not start at the initialized model".into());
        }
        while session.redo().is_ok() {}
        let retained = session.command_history();
        let start = retained
            .iter()
            .zip(&entries)
            .take_while(|(a, b)| a == b)
            .count();
        // Choose the exact common command prefix, never a content-only match:
        // setting a value back to its original value can repeat a revision id.
        while session.history_state().undo_depth > start {
            session.undo().expect("retained prefix exists");
        }
        let mut receipts = Vec::new();
        if start < entries.len() {
            for entry in &entries[start..] {
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
                receipts.push(outcome);
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
        // Only replay extending the admitted cursor has receipt lineage from
        // the held scene. Undo/redo and divergent prefixes reconcile explicitly.
        // Commands retained beyond the target cursor never enter its surface scope.
        let extending =
            start == request.base_history.undo_depth && request.target_history.undo_depth >= start;
        let unchanged = request.base_history == request.target_history;
        let mut pending_reconcile = held.pending_reconcile;
        let mut pending = held.pending_receipts.clone();
        if extending {
            receipts.truncate(request.target_history.undo_depth - start);
            pending.extend(receipts.iter().cloned());
        } else if !unchanged {
            pending_reconcile = true;
            pending.clear();
        }
        if pending.len() > 16_384 {
            pending_reconcile = true;
            pending.clear();
        }
        if !calculate {
            let replay_ms = replay_started.elapsed().as_secs_f64() * 1000.0;
            let authored_started = std::time::Instant::now();
            // Admission stays transactional, but observation never enters scene,
            // weather, usage or analysis preparation. Retain the calculated view
            // and its axis in RAM; the response is bounded freshness metadata.
            let state = self.views[&request.scope].clone();
            resolve(&state, &project.style_document)?;
            review_status_colors(&project.style_document)?;
            review_box_ink(&project.style_document)?;
            // Stage authored geometry before admission, independently of the
            // retained calculated review. Navigation derives geometry fresh,
            // without running any engineering calculation.
            let authored = held
                .authored
                .as_ref()
                .zip(held.review.base_scene())
                .map(|(view, computed)| {
                    view.advance(
                        session.snapshot(),
                        session.current_revision(),
                        held.options.clone(),
                        if extending { &receipts } else { &[] },
                        computed,
                        &held.label_policy,
                    )
                })
                .transpose();
            let (authored, authored_delta, authored_unavailable) = match authored {
                Ok(Some((view, delta))) => (Some(view), Some(delta), None),
                Ok(None) => (
                    None,
                    None,
                    Some("profile_authored_base_unavailable".to_string()),
                ),
                Err(error) => (
                    held.authored.clone(),
                    None,
                    Some(format!("profile_authored_geometry_unavailable: {error}")),
                ),
            };
            let held = self.held.get_mut(&request.scope).expect("admitted base");
            held.session = session;
            held.demand_cache = cache;
            held.pending_receipts = pending;
            held.pending_reconcile = pending_reconcile;
            held.authored = authored;
            let mut scope = ds_grid_engine::recalculation_scope::RecalculationScope {
                whole_model: pending_reconcile,
                ..Default::default()
            };
            for outcome in &held.pending_receipts {
                let change = &outcome.delta.recalculation_scope;
                scope.whole_model |= change.whole_model;
                scope
                    .demand_sections
                    .extend(change.demand_sections.iter().cloned());
                scope.structures.extend(change.structures.iter().cloned());
                scope
                    .context_sections
                    .extend(change.context_sections.iter().cloned());
                scope.spans.extend(change.spans.iter().cloned());
                scope.alignments.extend(change.alignments.iter().cloned());
            }
            let computed = held.review.base_scene().map(|scene| &scene.model_revision);
            let authored_ms = authored_started.elapsed().as_secs_f64() * 1000.0;
            let delta_bytes = authored_delta.as_ref().map(|delta| {
                serde_json::to_vec(delta)
                    .expect("native delta serializes")
                    .len()
            });
            let mut response = json!({"model_id":request.model_id, "model_revision":request.expected_revision,
                "history":held.session.history_state(), "retained_asset_count":held.source.assets.len(),
                "display_state":state, "scene":null, "analysis":null,
                "command_view":{"display":null,"review":null,"analysis":null,
                    "analysis_state":state.analysis_state(false, !held.session.snapshot().structures.is_empty()),
                    "calculation_policy":"explicit"},
                "projection_unavailable":null, "ran":false, "deferred":true,
                "authored_delta":authored_delta, "authored_unavailable":authored_unavailable,
                "calculation":{"policy":"explicit", "model_revision":request.expected_revision,
                    "computed_revision":computed, "required":computed != Some(&request.expected_revision),
                    "scope":scope}});
            response["metrics"] = json!({"journal_ms":journal_ms,"replay_ms":replay_ms,
                "authored_ms":authored_ms,"total_native_ms":started.elapsed().as_secs_f64()*1000.0,
                "authored_delta_bytes":delta_bytes,"applied_entries":receipts.len(),"retained_entries":entries.len()});
            return Ok(response);
        }
        if pending_reconcile {
            pending.clear();
        }
        self.render_replayed(project, session, source, cache, request.analyze, &pending)
    }
    fn render_replayed(
        &mut self,
        mut project: ProjectRequest,
        session: GridSession,
        source: HeldSource,
        cache: ds_grid_engine::StructureUsageScreeningCache,
        analyze: bool,
        receipts: &[ds_grid_engine::CommandOutcome],
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
        let mut result = self.project_session(project, session, source, cache, receipts)?;
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
