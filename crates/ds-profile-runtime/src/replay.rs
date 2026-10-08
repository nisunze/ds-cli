use super::*;
use ds_grid_engine::{CommandEnvelope, RevisionId, SessionHistoryState};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitializeRequest {
    pub scope: String,
    /// Stage disposable results until the host confirms worker/render admission.
    #[serde(default)]
    pub publication_id: Option<String>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshRequest {
    pub scope: String,
    #[serde(default)]
    pub publication_id: Option<String>,
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
        self.publications.remove(&request.scope);
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
        let admission_ms = started.elapsed().as_secs_f64() * 1000.0;
        let journal_started = std::time::Instant::now();
        let journal: ds_grid_engine::journal::SessionJournal =
            serde_json::from_str(&request.journal_json)
                .map_err(|error| format!("profile_replay_invalid: {error}"))?;
        if journal.history != request.target_history {
            return Err(
                "profile_replay_diverged: journal cursor differs from requested history".into(),
            );
        }
        let entries = journal.entries;
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
        let journal_ms = journal_started.elapsed().as_secs_f64() * 1000.0;
        let mut session = held.session.clone();
        let replay_started = std::time::Instant::now();
        let mut replay_cost = replay_cost::ReplayCost::default();
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
            for (offset, entry) in entries[start..].iter().enumerate() {
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
                replay_cost.record(&session);
                // Validate the complete retained journal, but a redo-tail
                // command is not an admitted current-cursor invalidation.
                if start + offset < request.target_history.undo_depth {
                    cache.advance_command(&outcome);
                }
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
        let replay_ms = replay_started.elapsed().as_secs_f64() * 1000.0;
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
                Ok(Some((view, delta))) => (Some(Arc::new(view)), Some(delta), None),
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
            let display = authored
                .as_ref()
                .filter(|_| authored_delta.is_some())
                .map(|view| {
                    authored_display::project(
                        view.geometry(),
                        &state,
                        &held.rows,
                        &held.label_policy,
                    )
                });
            let authored_display_delta = display
                .as_ref()
                .map(|display| authored_display::delta(&held.authored_display, display));
            let held = self.held.get_mut(&request.scope).expect("admitted base");
            held.session = session;
            held.demand_cache = cache;
            held.pending_receipts = pending;
            held.pending_reconcile = pending_reconcile;
            held.authored = authored;
            if let Some(display) = display {
                held.authored_display = display;
            }
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
            held.query_scope = scope.clone();
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
                "authored_delta":authored_delta, "authored_display_delta":authored_display_delta, "authored_unavailable":authored_unavailable,
                "calculation":{"policy":"explicit", "model_revision":request.expected_revision,
                    "computed_revision":computed, "required":computed != Some(&request.expected_revision),
                    "scope":scope}});
            response["metrics"] = json!({"admission_ms":admission_ms,"journal_ms":journal_ms,"replay_ms":replay_ms,
                "authored_ms":authored_ms,"total_native_ms":started.elapsed().as_secs_f64()*1000.0,
                "authored_delta_bytes":delta_bytes,"authored_display_delta_bytes":authored_display_delta.as_ref().map(|delta| serde_json::to_vec(delta).unwrap().len()),"applied_entries":receipts.len(),"retained_entries":entries.len()});
            replay_cost.append(&mut response["metrics"]);
            return Ok(response);
        }
        if pending_reconcile {
            pending.clear();
        }
        let calculation_started = std::time::Instant::now();
        let mut result =
            self.render_replayed(project, session, source, cache, request.analyze, &pending)?;
        let held = &self.held[&request.scope];
        let surface = held.review.surface_work().unwrap_or_default();
        let references = held.review.reference_work().unwrap_or_default();
        let cases = held.review.case_work().unwrap_or_default();
        let demands = held.demand_cache.last_demand_work();
        result["metrics"] = json!({"admission_ms":admission_ms,"journal_ms":journal_ms,"replay_ms":replay_ms,
            "calculation_ms":calculation_started.elapsed().as_secs_f64()*1000.0,
            "total_native_ms":started.elapsed().as_secs_f64()*1000.0,
            "applied_entries":receipts.len(),"retained_entries":entries.len(),
            "last_surface_computed_bands":surface.computed_bands,"last_surface_reused_bands":surface.reused_bands,
            "last_surface_computed_ground":surface.computed_ground,"last_surface_reused_ground":surface.reused_ground,
            "last_surface_local_terrain_point_projections":surface.local_terrain_point_projections,
            "last_surface_local_ground_rebuilds":surface.local_ground_rebuilds,
            "last_reference_computed_sections":references.computed_sections,"last_reference_reused_sections":references.reused_sections,
            "last_reference_terrain_checked_sections":references.terrain_checked_sections,"last_reference_terrain_reused_sections":references.terrain_reused_sections,
            "last_reference_terrain_inspected_ground_edges":references.terrain_inspected_ground_edges,
            "last_reference_terrain_inspected_observations":references.terrain_inspected_observations,
            "last_case_computed_sections":cases.computed_sections,"last_case_reused_sections":cases.reused_sections,
            "last_case_attempted_transitions":cases.attempted_transitions,"last_case_cached_sections":cases.cached_sections,
            "last_case_cached_samples":cases.cached_samples,"last_case_cache_admitted":usize::from(cases.cache_admitted),
            "last_demand_computed_cases":demands.computed_cases,"last_demand_reused_cases":demands.reused_cases,
            "last_demand_terrain_geometry_checks":demands.terrain_geometry_checks,
            "last_demand_terrain_geometry_reused_cases":demands.terrain_geometry_reused_cases,
            "last_demand_resolved_support_paths":demands.resolved_support_paths});
        replay_cost.append(&mut result["metrics"]);
        Ok(result)
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
            let (analysis, index) = analysis::analyze_for_queries(
                &held.session,
                &held.options.display_case,
                held.review.base_scene(),
                None,
                &mut held.demand_cache,
            );
            held.analysis_index = Some(index);
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
