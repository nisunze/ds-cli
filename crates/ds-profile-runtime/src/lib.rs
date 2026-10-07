//! Native Profile execution shared by the desktop host and headless callers.
//! Initialization pins a package once. Refresh replays the typed journal in RAM;
//! neither refresh nor history navigation writes or packs a package.
use ds_command_kernel::profile_display::{DisplayPatch, DisplayState};
use ds_command_kernel::profile_encoding::{
    DisplayRow, Encoding, clearance_violation_encoding, resolve, review_box_ink,
    review_status_colors,
};
mod authored_display;
mod command_view;
mod decorations;
mod label_lanes;
mod symbols;
use ds_grid_engine::{GridSession, ProfileAtlasOptions};
mod analysis;
pub use analysis::analyze;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
mod replay;
pub use replay::{InitializeRequest, RefreshRequest, ReleaseRequest};
mod publication;
pub use publication::{ConfirmPublicationRequest, DiscardPublicationRequest, PublicationPin};
mod replay_cost;
mod calculation_jobs;
pub use calculation_jobs::{AdmitCalculationRequest, CalculationJobRequest, StartCalculationRequest};
mod result_queries;
mod cached_cases;
pub use result_queries::{QueryArguments, QueryCursor, QueryKind, QueryResultsRequest};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRequest {
    pub scope: String,
    pub expected_revision: String,
    pub package_bytes: Vec<u8>,
    pub options: ProfileAtlasOptions,
    pub style_document: Value,
    pub patch: DisplayPatch,
    pub analyze: bool,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Project(Box<ProjectRequest>),
    Initialize(Box<InitializeRequest>),
    Refresh(Box<RefreshRequest>),
    /// Replay authored RAM edits without projecting or calculating engineering.
    Observe(Box<RefreshRequest>),
    /// Presentation changes may reuse admitted results but cannot calculate.
    Present(Box<RefreshRequest>),
    ConfirmPublication(ConfirmPublicationRequest),
    DiscardPublication(DiscardPublicationRequest),
    StartCalculation(Box<StartCalculationRequest>),
    CalculationStatus(CalculationJobRequest),
    CancelCalculation(CalculationJobRequest),
    AdmitCalculation(AdmitCalculationRequest),
    QueryResults(QueryResultsRequest),
    Release(ReleaseRequest),
    CaseLayer(CaseRequest),
    Layout(ds_command_kernel::profile_layout::Request),
    SelectionControls(ds_command_kernel::grid_selection_controls::Request),
}
impl From<ProjectRequest> for Request {
    fn from(value: ProjectRequest) -> Self {
        Self::Project(Box::new(value))
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseRequest {
    pub scope: String,
    pub model_id: String,
    pub expected_revision: String,
    pub expected_history: ds_grid_engine::SessionHistoryState,
    pub display_case: ds_grid_engine::profile_display_case::ProfileDisplayCase,
    pub select: bool,
    pub cached_only: bool,
    pub sequence: u64,
}
impl From<CaseRequest> for Request {
    fn from(value: CaseRequest) -> Self {
        Self::CaseLayer(value)
    }
}

#[derive(Clone)]
struct HeldSource {
    model_id: String,
    presentation: ds_grid_model::GridPresentationPolicy,
    assets: Arc<Vec<ds_grid_exchange::AssetBytes>>,
}
#[derive(Clone)]
struct HeldView {
    session: GridSession,
    source: HeldSource,
    demand_cache: ds_grid_engine::StructureUsageScreeningCache,
    review: Arc<ds_grid_engine::profile_engineering::ProfileReviewView>,
    authored: Option<Arc<ds_grid_engine::profile_atlas::ProfileAuthoredView>>,
    authored_display: authored_display::Groups,
    options: ProfileAtlasOptions,
    rows: Vec<DisplayRow>,
    label_policy: ds_grid_model::StructureLabelPolicy,
    last_weather_sequence: u64,
    pending_receipts: Vec<ds_grid_engine::CommandOutcome>,
    pending_reconcile: bool,
    result_index: Arc<result_queries::Index>,
    query_scope: ds_grid_engine::recalculation_scope::RecalculationScope,
}
#[derive(Debug, Clone, Serialize)]
pub struct Projection {
    pub state: DisplayState,
    pub rows: Vec<DisplayRow>,
    pub groups: Vec<ds_command_kernel::profile_controls::ControlGroup>,
    pub style_controls: Value,
    pub symbols: Vec<symbols::Symbol>,
    pub authored_structures: authored_display::Groups,
    pub selection_families: Vec<ds_command_kernel::grid_selection_controls::Family>,
    pub legend: Vec<ds_command_kernel::profile_encoding::LegendEntry>,
    /// One pen per drawn cable engineering name; hosts paint each span with its cable pen.
    pub cables: Vec<ds_command_kernel::profile_encoding::CablePen>,
    pub label_fields: Vec<String>,
    pub scene_options: ProfileAtlasOptions,
    pub primitives: Vec<decorations::Primitive>,
    pub clearance_violation_encoding: Encoding,
    pub review_box_ink: String,
    pub review_status_colors: BTreeMap<String, String>,
}
#[derive(Default)]
pub struct Runtime {
    selections: BTreeMap<String, ds_command_kernel::grid_selection_controls::State>,
    views: BTreeMap<String, DisplayState>,
    analyses: BTreeMap<String, Value>,
    held: BTreeMap<String, HeldView>,
    publications: BTreeMap<String, publication::PendingPublication>,
    calculation_jobs: calculation_jobs::Jobs,
    /// Identity changes even when display inputs change away and back.
    calculation_contexts: BTreeMap<String, Arc<()>>,
}
impl Runtime {
    pub fn execute(&mut self, request: impl Into<Request>) -> Result<Value, String> {
        let request = request.into();
        let changed_context = match &request {
            Request::Project(r) => Some(r.scope.clone()),
            Request::Initialize(r) => Some(r.scope.clone()),
            Request::Refresh(r) | Request::Present(r) => Some(r.scope.clone()),
            Request::CaseLayer(r) if r.select => Some(r.scope.clone()),
            Request::DiscardPublication(r) => Some(r.scope.clone()),
            Request::AdmitCalculation(r) => Some(r.scope.clone()),
            Request::Release(r) => Some(r.scope.clone()),
            _ => None,
        };
        let result = self.execute_with_publication(request);
        if result.is_ok() {
            if let Some(scope) = changed_context {
                if self.held.contains_key(&scope) {
                    self.calculation_contexts.insert(scope, Arc::new(()));
                } else {
                    self.calculation_contexts.remove(&scope);
                    self.calculation_jobs.release_scope(&scope);
                }
            }
        }
        result
    }
    fn execute_inner(&mut self, request: Request) -> Result<Value, String> {
        match request {
            Request::Project(request) => self.project(*request),
            Request::Initialize(request) => self.initialize(*request),
            Request::Refresh(request) => self.refresh(*request, true),
            Request::Observe(request) => self.refresh(*request, false),
            Request::Present(request) => self.present(*request),
            Request::ConfirmPublication(request) => self.confirm_publication(request),
            Request::DiscardPublication(request) => self.discard_publication(request),
            Request::StartCalculation(request) => self.start_calculation(*request),
            Request::CalculationStatus(request) => self.calculation_status(request),
            Request::CancelCalculation(request) => self.cancel_calculation(request),
            Request::AdmitCalculation(request) => self.admit_calculation(request),
            Request::QueryResults(request) => self.query_results(request),
            Request::Release(request) => self.release(request),
            Request::CaseLayer(request) => self.case_layer(request),
            Request::Layout(request) => {
                Ok(json!(ds_command_kernel::profile_layout::resolve(request)?))
            }
            Request::SelectionControls(request) => {
                let state = self
                    .selections
                    .get(&request.scope)
                    .cloned()
                    .unwrap_or_default();
                let (state, result) =
                    ds_command_kernel::grid_selection_controls::execute(&state, &request)?;
                if !self.selections.contains_key(&request.scope) && self.selections.len() >= 256 {
                    return Err("invalid_grid_lasso: selection context capacity reached".into());
                }
                self.selections.insert(request.scope, state);
                Ok(result)
            }
        }
    }
    fn project(&mut self, request: ProjectRequest) -> Result<Value, String> {
        if request.scope.is_empty()
            || request.scope.len() > 1024
            || request.package_bytes.len() > 512 * 1024 * 1024
        {
            return Err("invalid_profile_view: request exceeds native bounds".into());
        }
        self.ensure_scope_available(&request.scope)?;
        let (package, _) = ds_grid_exchange::dsgrid::ingest(&request.package_bytes)
            .map_err(|error| format!("profile_package_invalid: {error}"))?;
        let session = ds_grid_exchange::linked_models::open_session(&package);
        let source = HeldSource {
            model_id: package.manifest.model.model_id.to_string(),
            presentation: package.manifest.model.presentation,
            assets: Arc::new(package.assets),
        };
        self.project_session(request, session, source, Default::default(), &[])
    }
    fn project_session(
        &mut self,
        mut request: ProjectRequest,
        session: GridSession,
        source: HeldSource,
        mut demand_cache: ds_grid_engine::StructureUsageScreeningCache,
        receipts: &[ds_grid_engine::CommandOutcome],
    ) -> Result<Value, String> {
        self.ensure_scope_available(&request.scope)?;
        let model_id = &source.model_id;
        let history = session.history_state();
        let revision = session.current_revision();
        if revision.revision_id.to_string() != request.expected_revision {
            return Err(
                "profile_selection_stale: checkpoint differs from the held model revision".into(),
            );
        }
        let current = self.views.get(&request.scope).cloned().unwrap_or_default();
        let mut state = current.patched(&request.patch)?;
        if request.analyze {
            state.visibility.insert(
                ds_command_kernel::profile_display::DisplayOption::Review,
                true,
            );
        }
        let styles = (|| {
            Ok::<_, String>((
                resolve(&state, &request.style_document)?,
                review_status_colors(&request.style_document)?,
                review_box_ink(&request.style_document)?,
            ))
        })();
        if !request.analyze {
            if let Err(error) = &styles {
                return Err(error.clone());
            }
        }
        request.options = project_options(request.options, &state)?;
        // Named cases must exist. Failed selection cannot replace the current state.
        if let ds_grid_engine::profile_display_case::ProfileDisplayCase::AnalysisCase {
            analysis_case_id,
        } = &request.options.display_case
        {
            if !session
                .snapshot()
                .analysis_cases
                .iter()
                .any(|case| &case.id == analysis_case_id)
            {
                return Err(
                    "invalid_profile_view: analysis case is not in the active model".into(),
                );
            }
        }
        // Strength/usage selection is independent of the displayed weather case.
        request.options.engineering_selection = None;
        let scene_options = request.options.clone();
        let requested_case = request.options.display_case.clone();
        let mut previous = self.held.remove(&request.scope);
        let same_model = previous
            .as_ref()
            .is_some_and(|held| held.source.model_id == *model_id);
        let last_weather_sequence = previous
            .as_ref()
            .filter(|_| same_model)
            .map_or(0, |held| held.last_weather_sequence);
        let mut review = previous
            .as_mut()
            .filter(|_| same_model)
            .map(|held| std::mem::take(&mut held.review))
            .unwrap_or_default();
        Arc::make_mut(&mut review).install_case(requested_case);
        // Analyze may return an explicit unavailable projection. A style
        // refusal must not advance the retained review against the old session.
        let scene = if let Err(error) = &styles {
            Err(error.clone())
        } else {
            Arc::make_mut(&mut review)
                .scene_after_with_cache(
                    &session,
                    request.options,
                    if same_model { receipts } else { &[] },
                    &mut demand_cache,
                )
                .map_err(|error| format!("profile_scene_unavailable: {error}"))
        };
        let projection_error = styles.as_ref().err().or(scene.as_ref().err()).cloned();
        if let Some(error) = &projection_error {
            // A refused projection must retain the previously admitted checkpoint,
            // weather sequence and disposable cache. Transfer ownership back rather
            // than cloning the potentially large projection.
            if let Some(mut held) = previous.take() {
                Arc::make_mut(&mut review).install_case(
                    serde_json::from_value(serde_json::to_value(&current.display_case).unwrap())
                        .expect("admitted display case"),
                );
                held.review = review;
                self.held.insert(request.scope.clone(), held);
            }
            if !request.analyze {
                return Err(error.clone());
            }
            let analysis = analysis::analyze_with_cache(
                &session,
                &scene_options.display_case,
                scene.as_ref().ok(),
                Some(error),
                &mut demand_cache,
            );
            let command_view = command_view::project(
                &state,
                None,
                scene.as_ref().ok(),
                Some(&analysis),
                !session.snapshot().structures.is_empty(),
            );
            self.analyses
                .insert(request.scope.clone(), analysis.clone());
            self.commit_view(&request.scope, state.clone());
            let mut response = json!({"model_id":model_id, "model_revision":request.expected_revision, "history":history, "retained_asset_count":source.assets.len(), "scene":null, "analysis":null, "display_state":state, "projection_unavailable":error, "command_view":null, "ran":true});
            response["analysis"] = analysis;
            response["command_view"] = command_view;
            return Ok(response);
        }
        let mut scene = scene.unwrap();
        let (rows, review_status_colors, review_box_ink) = styles.unwrap();
        let mut label_policy = source.presentation.effective_profile_structure_labels();
        label_policy.fields = serde_json::from_value(json!(label_fields(&state)))
            .map_err(|error| error.to_string())?;
        ds_grid_engine::profile_labels::compose_scene_structure_labels(&mut scene, &label_policy);
        let grid = &rows
            .iter()
            .find(|row| row.option == ds_command_kernel::profile_display::DisplayOption::Grid)
            .unwrap()
            .encoding;
        for stroke in [
            &mut scene.foundation.station_grid,
            &mut scene.foundation.elevation_grid,
        ] {
            stroke.color = grid.color.clone();
            stroke.width_px = grid.width_px;
            stroke.dash_px = grid.dash_px.clone();
        }
        let analysis = if request.analyze {
            let value = analysis::analyze_with_cache(
                &session,
                &scene_options.display_case,
                Some(&scene),
                None,
                &mut demand_cache,
            );
            self.analyses.insert(request.scope.clone(), value.clone());
            Some(value)
        } else {
            self.analyses
                .get(&request.scope)
                .filter(|value| same_model && value["model_revision"] == request.expected_revision)
                .cloned()
        };
        if !same_model && !request.analyze {
            self.analyses.remove(&request.scope);
        }
        let command_view = command_view::project(
            &state,
            Some(&rows),
            Some(&scene),
            analysis.as_ref(),
            !session.snapshot().structures.is_empty(),
        );
        self.commit_view(&request.scope, state.clone());
        let authored = review.authored_view(&label_policy).map(Arc::new);
        let result_index = Arc::new(previous.as_ref().filter(|_| same_model)
            .and_then(|held| held.result_index.identical_projection(&scene))
            .unwrap_or_else(|| result_queries::Index::new(&session, &scene, &demand_cache)));
        self.held.insert(
            request.scope.clone(),
            HeldView {
                session,
                source: source.clone(),
                demand_cache,
                review,
                authored,
                authored_display: Default::default(),
                options: scene_options.clone(),
                rows: rows.clone(),
                label_policy: label_policy.clone(),
                last_weather_sequence,
                pending_receipts: Vec::new(),
                pending_reconcile: false,
                result_index,
                query_scope: Default::default(),
            },
        );
        let primitives = decorations::project(&scene, &state, &rows, &label_policy);
        let cables = ds_command_kernel::profile_encoding::cable_pens(
            scene
                .conductor_spans
                .iter()
                .map(|span| span.cable_engineering_name.as_str()),
            &rows,
            &state.cable_colors,
        );
        let base_axis = self.held[&request.scope]
            .review
            .base_axis_digest()
            .unwrap()
            .to_string();
        let legend = ds_command_kernel::profile_encoding::styled_legend(&state, &rows, &cables);
        let symbols = symbols::project(&scene, &state, &rows, &legend);
        let (authored_structures, symbols, primitives) =
            authored_display::partition(symbols, primitives, &scene);
        self.held.get_mut(&request.scope).unwrap().authored_display = authored_structures.clone();
        let mut scene = serde_json::to_value(scene).map_err(|error| error.to_string())?;
        scene["case_base_axis_digest"] = json!(base_axis);
        scene["display"] = serde_json::to_value(Projection {
            label_fields: label_fields(&state),
            groups: ds_command_kernel::profile_controls::groups(&state, &rows, &cables),
            style_controls: ds_command_kernel::profile_style::controls(&state, &rows),
            symbols,
            authored_structures,
            selection_families: ds_command_kernel::grid_selection_controls::profile_families(
                &state,
            ),
            state: state.clone(),
            legend,
            cables,
            rows,
            scene_options,
            primitives,
            review_status_colors,
            clearance_violation_encoding: clearance_violation_encoding(&request.style_document)?,
            review_box_ink,
        })
        .unwrap();
        // Keep analysis once on the native wire. The desktop transports that same
        // evidence alongside its held scene without serializing a second copy.
        // json! serializes borrowed expressions even when they are already
        // Values. Move these trees into the envelope rather than rebuilding
        // the complete scene and its issue payload after their first encoding.
        let mut response = json!({ "model_id":model_id, "model_revision":request.expected_revision, "history":history, "retained_asset_count":source.assets.len(), "scene":null, "analysis":null, "display_state":state, "projection_unavailable":null, "command_view":null, "ran": request.analyze });
        response["scene"] = scene;
        response["analysis"] = analysis.unwrap_or(Value::Null);
        response["command_view"] = command_view;
        response["calculation"] = json!({"policy":"explicit", "model_revision":request.expected_revision,
            "computed_revision":request.expected_revision, "required":false,
            "scope":ds_grid_engine::recalculation_scope::RecalculationScope::default()});
        Ok(response)
    }
    fn ensure_scope_available(&self, scope: &str) -> Result<(), String> {
        // Every admitted scope is leased until explicit release. UUID ordering
        // and recency cannot establish that another client's session is unused.
        if !self.views.contains_key(scope) && self.views.len() >= 8 {
            return Err("profile_replay_capacity: release an unused Profile scope before initializing another".into());
        }
        Ok(())
    }
    fn commit_view(&mut self, scope: &str, state: DisplayState) {
        self.views.insert(scope.to_owned(), state);
    }
}

fn project_options(
    mut options: ProfileAtlasOptions,
    state: &DisplayState,
) -> Result<ProfileAtlasOptions, String> {
    options.vertical_exaggeration = Some(state.vertical_exaggeration);
    options.terrain_corridor_half_width_m = Some(state.terrain.corridor_half_width_m);
    options.terrain_gap_tolerance_m = Some(state.terrain.gap_tolerance_m);
    options.clearance_ground_offset_m = Some(state.terrain.ground_clearance_offset_m);
    options.clearance_lowest_wire_drop_m = Some(state.terrain.lowest_wire_drop_m);
    options.display_case =
        serde_json::from_value(serde_json::to_value(&state.display_case).unwrap())
            .map_err(|error| format!("invalid_profile_view: {error}"))?;
    options.engineering_selection = None;
    Ok(options)
}

fn label_fields(state: &DisplayState) -> Vec<String> {
    use ds_command_kernel::profile_display::DisplayOption;
    let mut fields = Vec::new();
    if state.visibility[&DisplayOption::StructureNumbers] {
        fields.push("number".into());
    }
    if state.visibility[&DisplayOption::StructureNames] {
        fields.push("type".into());
    }
    if state.visibility[&DisplayOption::StructureComments] {
        fields.extend(["comment1".into(), "comment2".into(), "comment3".into()]);
    }
    fields
}

#[cfg(test)]
#[path = "../../../../ds-network/crates/ds-grid-engine/tests/common/mod.rs"]
mod common;

#[cfg(test)]
mod transactional_projection_tests {
    use super::*;
    use ds_grid_model::{EntityId, ModelCrs};
    pub(super) fn request() -> ProjectRequest {
        request_for(common::fixture())
    }
    pub(super) fn request_for(snapshot: ds_grid_model::GridModelSnapshot) -> ProjectRequest {
        let session = GridSession::open(snapshot);
        let (artifacts, _) = ds_grid_exchange::dsgrid::emit(
            session.snapshot(),
            &ds_grid_exchange::PackOptions {
                model_id: EntityId::new("profile-test").unwrap(),
                model_revision: 0,
                coordinate_system: ModelCrs::new("EPSG:32736").unwrap(),
                presentation: Default::default(),
                library_pins: vec![],
                library_needs: vec![],
                assets: vec![ds_grid_exchange::AssetBytes {
                    invariant_leaf: "source-evidence.txt".into(),
                    bytes: b"retained exact evidence".to_vec(),
                }],
                exchange_bindings: Default::default(),
            },
        )
        .unwrap();
        ProjectRequest {
            scope: "one".into(),
            expected_revision: session.current_revision().revision_id.to_string(),
            package_bytes: artifacts.artifacts[0].bytes.clone(),
            options: Default::default(),
            style_document: serde_json::from_str(include_str!(
                "../../../tests/fixtures/profile-display-style.json"
            ))
            .unwrap(),
            patch: Default::default(),
            analyze: false,
        }
    }
    pub(super) fn refresh(
        runtime: &Runtime,
        input: &ProjectRequest,
        base: &str,
        session: &GridSession,
    ) -> RefreshRequest {
        RefreshRequest {
            publication_id: None,
            scope: input.scope.clone(),
            model_id: "profile-test".into(),
            base_revision: serde_json::from_value(json!(base)).unwrap(),
            base_history: runtime.held[&input.scope].session.history_state(),
            expected_revision: session.current_revision().revision_id.clone(),
            target_history: session.history_state(),
            journal_json: serde_json::to_string(&session.retained_journal()).unwrap(),
            options: input.options.clone(),
            style_document: input.style_document.clone(),
            patch: Default::default(),
            analyze: false,
        }
    }
    pub(super) fn describe(session: &mut GridSession, id: &str, text: &str) {
        session
            .apply_command(ds_grid_engine::CommandEnvelope::new(
                id,
                session.current_revision().revision_id.clone(),
                ds_grid_engine::GridCommand::DescribeStructure {
                    id: ds_grid_model::StructureId::new("str-2").unwrap(),
                    description: Some(text.into()),
                },
            ))
            .unwrap();
    }
    #[test]
    fn journal_refresh_matches_fresh_projection_and_navigates_undo_redo_and_branches() {
        let input = request();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let assets = runtime.held[&input.scope].source.assets.clone();
        let mut worker = GridSession::open(common::fixture());
        describe(&mut worker, "one", "changed once");
        describe(&mut worker, "two", "changed twice");
        let edited = worker.current_revision().revision_id.to_string();
        let replay = runtime
            .execute(refresh(&runtime, &input, &input.expected_revision, &worker))
            .unwrap();
        let fresh = Runtime::default()
            .project_session(
                ProjectRequest {
                    expected_revision: edited.clone(),
                    ..input.clone()
                },
                GridSession::open(worker.snapshot().clone()),
                HeldSource {
                    model_id: "profile-test".into(),
                    presentation: Default::default(),
                    assets: assets.clone(),
                },
                Default::default(),
                &[],
            )
            .unwrap();
        assert_eq!(replay["scene"], fresh["scene"]);
        assert_eq!(replay["command_view"], fresh["command_view"]);
        assert!(Arc::ptr_eq(
            &assets,
            &runtime.held[&input.scope].source.assets
        ));
        assert_eq!(assets[0].bytes, b"retained exact evidence");
        worker.undo().unwrap();
        let middle = worker.current_revision().revision_id.to_string();
        runtime
            .execute(refresh(&runtime, &input, &edited, &worker))
            .unwrap();
        assert_eq!(
            runtime.held[&input.scope].session.snapshot(),
            worker.snapshot()
        );
        worker.redo().unwrap();
        runtime
            .execute(refresh(&runtime, &input, &middle, &worker))
            .unwrap();
        worker.undo().unwrap();
        describe(&mut worker, "branch", "new branch");
        runtime
            .execute(refresh(&runtime, &input, &edited, &worker))
            .unwrap();
        assert_eq!(
            runtime.held[&input.scope].session.snapshot(),
            worker.snapshot()
        );
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            worker.history_state()
        );
    }
    fn two_band_request() -> (ProjectRequest, GridSession) {
        use ds_grid_model::{AlignmentId, RouteEdgeId};
        let mut snapshot = common::fixture();
        for (index, (x_m, y_m, z_m)) in [(0., 0., 100.), (300., 0., 102.), (600., 40., 98.)]
            .into_iter()
            .enumerate()
        {
            snapshot
                .terrain_points
                .push(ds_grid_model::TerrainPointRow {
                    id: ds_grid_model::TerrainPointId::new(format!("profile-ground-{index}"))
                        .unwrap(),
                    x_m,
                    y_m,
                    z_m,
                    feature_class: "ground".into(),
                    description: None,
                    required_clearance_m: None,
                    source_id: None,
                });
        }
        let mut second = snapshot.alignments[0].clone();
        second.id = AlignmentId::new("independent-band").unwrap();
        second.label = "Independent band".into();
        snapshot.alignments.push(second.clone());
        for mut edge in snapshot.route_edges.clone() {
            edge.id = RouteEdgeId::new(format!("independent-{}", edge.id)).unwrap();
            edge.alignment_id = second.id.clone();
            snapshot.route_edges.push(edge);
        }
        (request_for(snapshot.clone()), GridSession::open(snapshot))
    }
    fn refresh_surface(
        runtime: &mut Runtime,
        input: &ProjectRequest,
        worker: &GridSession,
        computed: usize,
        reused: usize,
    ) {
        let base = runtime.held[&input.scope]
            .session
            .current_revision()
            .revision_id
            .to_string();
        let result = runtime
            .execute(refresh(runtime, input, &base, worker))
            .unwrap();
        assert_eq!(
            runtime.held[&input.scope].review.surface_work().unwrap(),
            ds_grid_engine::profile_atlas::ProfileSurfaceWork {
                computed_bands: computed,
                reused_bands: reused,
                computed_ground: if reused == 0 { computed } else { 0 },
                reused_ground: if reused == 0 { 0 } else { computed + reused },
                ..Default::default()
            }
        );
        let mut fresh_request = request_for(worker.snapshot().clone());
        fresh_request.patch =
            serde_json::from_value(serde_json::to_value(&runtime.views[&input.scope]).unwrap())
                .unwrap();
        let fresh = Runtime::default().execute(fresh_request).unwrap();
        assert_eq!(result["scene"], fresh["scene"]);
        assert_eq!(
            result["command_view"]["display"],
            fresh["command_view"]["display"]
        );
        assert_eq!(
            result["command_view"]["review"],
            fresh["command_view"]["review"]
        );
        if result["analysis"].is_null() {
            assert_eq!(result["command_view"], fresh["command_view"]);
        }
    }
    #[test]
    fn refresh_reuses_surface_from_native_receipts_and_reconciles_navigation() {
        let (input, mut worker) = two_band_request();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        describe(&mut worker, "one", "first local edit");
        refresh_surface(&mut runtime, &input, &worker, 1, 1);
        describe(&mut worker, "two", "coalesced first edit");
        describe(&mut worker, "three", "coalesced last edit");
        let base = runtime.held[&input.scope]
            .session
            .current_revision()
            .revision_id
            .to_string();
        let before = runtime.held[&input.scope].session.history_state();
        let mut refused = refresh(&runtime, &input, &base, &worker);
        refused.analyze = true;
        refused.style_document = Value::Null;
        assert!(runtime.execute(refused).is_err());
        assert_eq!(runtime.held[&input.scope].session.history_state(), before);
        refresh_surface(&mut runtime, &input, &worker, 1, 1);
        worker.undo().unwrap();
        refresh_surface(&mut runtime, &input, &worker, 2, 0);
        worker.redo().unwrap();
        refresh_surface(&mut runtime, &input, &worker, 2, 0);
        worker.undo().unwrap();
        refresh_surface(&mut runtime, &input, &worker, 2, 0);
        // A new branch extends the admitted cursor even though it replaces a
        // retained redo tail. It must use native receipts from that cursor.
        describe(&mut worker, "branch", "replacement branch");
        refresh_surface(&mut runtime, &input, &worker, 1, 1);
    }
    #[test]
    fn observed_deletion_removes_native_structure_presentation_without_terrain() {
        let (input, mut worker) = two_band_request();
        let mut runtime = Runtime::default();
        let initial = runtime.execute(input.clone()).unwrap();
        assert!(initial["scene"]["display"]["authored_structures"]["str-2"].is_object());
        worker
            .apply_command(ds_grid_engine::CommandEnvelope::new(
                "delete",
                worker.current_revision().revision_id.clone(),
                ds_grid_engine::GridCommand::DeleteProfileEntities {
                    ids: vec![EntityId::new("str-2").unwrap()],
                },
            ))
            .unwrap();
        let result = runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        assert_eq!(result["authored_display_delta"], json!({"str-2": null}));
        assert_eq!(result["authored_delta"]["work"]["computed_ground"], 0);
        assert!(result["scene"].is_null());
        assert_eq!(
            runtime.held[&input.scope]
                .review
                .base_scene()
                .unwrap()
                .model_revision
                .as_str(),
            input.expected_revision
        );
    }
    #[test]
    fn discarded_observation_composes_the_next_delta_from_the_published_history() {
        let (input, mut worker) = two_band_request();
        let mut runtime = Runtime::default();
        let initial = runtime.execute(input.clone()).unwrap();
        let published = runtime.held[&input.scope].session.history_state();
        let computed = runtime.held[&input.scope].review.clone();
        describe(&mut worker, "unpublished-B", "B");
        let mut request = refresh(&runtime, &input, &input.expected_revision, &worker);
        request.publication_id = Some("observe-B".into());
        let b = runtime
            .execute(Request::Observe(Box::new(request.clone())))
            .unwrap();
        let pin: PublicationPin = serde_json::from_value(b["publication"].clone()).unwrap();
        assert!(Arc::ptr_eq(&computed, &runtime.held[&input.scope].review));
        // No later request may silently adopt an unconfirmed native base.
        assert!(runtime
            .execute(Request::Observe(Box::new(request)))
            .unwrap_err()
            .starts_with("profile_publication_pending:"));
        let mut wrong = pin.clone();
        wrong.axis_pin_digest = Some("different-axis".into());
        assert!(runtime
            .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                scope: input.scope.clone(),
                publication: wrong
            }))
            .is_err());
        let mut wrong = pin.clone();
        wrong.history.undo_depth += 1;
        wrong.history.history_pin = "repeated-content-other-history".into();
        assert!(runtime
            .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                scope: input.scope.clone(),
                publication: wrong,
            }))
            .is_err());
        runtime
            .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                scope: input.scope.clone(),
                publication: pin.clone(),
            }))
            .unwrap();
        // Renderer refusal after worker admission can still discard this one
        // publication. Authored RAM is never rolled back or packed.
        describe(&mut worker, "unpublished-C", "C");
        runtime
            .execute(Request::DiscardPublication(DiscardPublicationRequest {
                scope: input.scope.clone(),
                model_id: pin.model_id,
                publication_id: pin.publication_id,
                target_history: pin.history,
            }))
            .unwrap();
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            published
        );
        assert!(Arc::ptr_eq(&computed, &runtime.held[&input.scope].review));
        let c = runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        assert_eq!(
            c["authored_delta"]["base_revision"],
            initial["model_revision"]
        );
        assert_eq!(
            c["authored_delta"]["computed_revision"],
            initial["model_revision"]
        );
        assert_eq!(c["authored_delta"]["work"]["computed_ground"], 0);
        assert_eq!(c["metrics"]["applied_entries"], 2);
        assert_eq!(c["history"], json!(worker.history_state()));
        assert_eq!(
            runtime.held[&input.scope].review.base_scene(),
            computed.base_scene()
        );
    }

    #[test]
    fn dropped_calculation_restores_pending_scope_and_allows_another_authored_edit() {
        let (input, mut worker) = two_band_request();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        describe(&mut worker, "observed-B", "B");
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        let published = runtime.held[&input.scope].session.history_state();
        let pending = runtime.held[&input.scope].pending_receipts.clone();
        let computed = runtime.held[&input.scope].review.clone();
        let base = worker.current_revision().revision_id.to_string();
        let mut request = refresh(&runtime, &input, &base, &worker);
        request.publication_id = Some("calculate-B".into());
        request.analyze = true;
        let b = runtime.execute(request).unwrap();
        assert_eq!(
            b["scene"]["model_revision"],
            json!(worker.current_revision().revision_id)
        );
        describe(&mut worker, "during-calculation-C", "C");
        let mut discard = DiscardPublicationRequest {
            scope: input.scope.clone(),
            model_id: "profile-test".into(),
            publication_id: "calculate-B".into(),
            target_history: published.clone(),
        };
        discard.target_history.history_pin = "unrelated-history".into();
        assert!(runtime
            .execute(Request::DiscardPublication(discard.clone()))
            .is_err());
        discard.target_history = published.clone();
        runtime
            .execute(Request::DiscardPublication(discard))
            .unwrap();
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            published
        );
        assert_eq!(runtime.held[&input.scope].pending_receipts, pending);
        assert!(Arc::ptr_eq(&computed, &runtime.held[&input.scope].review));
        let c = runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime, &input, &base, &worker,
            ))))
            .unwrap();
        assert_eq!(
            c["authored_delta"]["base_revision"],
            json!(published.model_revision)
        );
        assert_eq!(
            c["authored_delta"]["computed_revision"],
            json!(computed.base_scene().unwrap().model_revision)
        );
        assert_eq!(c["calculation"]["required"], true);
        assert_eq!(c["authored_delta"]["work"]["computed_ground"], 0);
        assert!(c["analysis"].is_null());
        assert!(runtime.analyses.get(&input.scope).is_none());
    }

    #[test]
    fn publication_initialization_discard_is_exact_and_isolated_between_scopes() {
        let (input, worker) = two_band_request();
        let seed = ds_grid_exchange::profile_session::ProfileSessionSeed::capture(
            &worker,
            EntityId::new("profile-test").unwrap(),
            Default::default(),
            &[],
        );
        let initialize = |scope: &str, id: &str| InitializeRequest {
            scope: scope.into(),
            publication_id: Some(id.into()),
            seed_json: serde_json::to_string(&seed).unwrap(),
            restore_state: None,
            options: input.options.clone(),
            style_document: input.style_document.clone(),
            patch: Default::default(),
            analyze: false,
        };
        let mut runtime = Runtime::default();
        let a = runtime.execute(initialize("a", "publication-a")).unwrap();
        let b = runtime.execute(initialize("b", "publication-b")).unwrap();
        let b_before = runtime.held["b"].session.history_state();
        let pin: PublicationPin = serde_json::from_value(a["publication"].clone()).unwrap();
        let mut discard = DiscardPublicationRequest {
            scope: "a".into(),
            model_id: "wrong-model".into(),
            publication_id: pin.publication_id,
            target_history: pin.history,
        };
        assert!(runtime
            .execute(Request::DiscardPublication(discard.clone()))
            .is_err());
        discard.model_id = "profile-test".into();
        runtime
            .execute(Request::DiscardPublication(discard))
            .unwrap();
        assert!(!runtime.held.contains_key("a"));
        assert!(!runtime.views.contains_key("a"));
        assert!(!runtime.publications.contains_key("a"));
        assert_eq!(runtime.held["b"].session.history_state(), b_before);
        let pin: PublicationPin = serde_json::from_value(b["publication"].clone()).unwrap();
        let confirm: Request = serde_json::from_value(json!({
            "operation":"confirm_publication", "scope":"b", "publication":pin,
        }))
        .unwrap();
        runtime.execute(confirm).unwrap();
        assert!(serde_json::from_value::<Request>(json!({
            "operation":"discard_publication", "scope":"b", "model_id":"profile-test",
            "publication_id":"publication-b", "target_history":b_before, "accept_stale":true,
        }))
        .is_err());
        assert_eq!(
            runtime
                .execute(ReleaseRequest {
                    scope: "b".into(),
                    model_id: "profile-test".into()
                })
                .unwrap()["released"],
            true
        );
        assert!(runtime.publications.is_empty());
        runtime.execute(initialize("a", "replacement-a")).unwrap();
    }
    #[test]
    fn native_terrain_observation_transports_work_without_advancing_engineering() {
        use ds_grid_engine::{CommandEnvelope, GridCommand};
        let (input, mut worker) = two_band_request();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let computed = runtime.held[&input.scope]
            .review
            .base_scene()
            .unwrap()
            .clone();
        let mut base = input.expected_revision.clone();
        for (index, elevation) in [102.2, 102.0, 102.2, 102.0].into_iter().enumerate() {
            worker
                .apply_command(CommandEnvelope::new(
                    format!("terrain-observe-{index}"),
                    worker.current_revision().revision_id.clone(),
                    GridCommand::SetTerrainPointElevation {
                        id: ds_grid_model::TerrainPointId::new("profile-ground-1").unwrap(),
                        elevation_m: elevation,
                    },
                ))
                .unwrap();
            let result = runtime
                .execute(Request::Observe(Box::new(refresh(
                    &runtime, &input, &base, &worker,
                ))))
                .unwrap();
            assert_eq!(result["ran"], false);
            assert!(result["scene"].is_null());
            assert!(result["analysis"].is_null());
            assert!(result["authored_unavailable"].is_null());
            assert_eq!(
                result["authored_delta"]["work"]["local_terrain_point_projections"],
                2
            );
            assert_eq!(result["authored_delta"]["work"]["local_ground_rebuilds"], 2);
            assert_eq!(
                runtime.held[&input.scope].review.base_scene(),
                Some(&computed)
            );
            assert_eq!(
                result["calculation"]["computed_revision"],
                computed.model_revision.to_string()
            );
            assert_eq!(
                result["history"],
                serde_json::to_value(worker.history_state()).unwrap()
            );
            base = worker.current_revision().revision_id.to_string();
        }
    }
    #[test]
    fn observe_accumulates_edits_without_projection_and_refresh_reuses_unchanged_alignment() {
        let (input, mut worker) = two_band_request();
        let mut runtime = Runtime::default();
        let initial = runtime.execute(input.clone()).unwrap();
        let original_scene = runtime.held[&input.scope]
            .review
            .base_scene()
            .unwrap()
            .clone();
        let assets = runtime.held[&input.scope].source.assets.clone();
        let mut base = input.expected_revision.clone();
        for (id, text) in [("one", "first"), ("two", "second")] {
            describe(&mut worker, id, text);
            let result = runtime
                .execute(Request::Observe(Box::new(refresh(
                    &runtime, &input, &base, &worker,
                ))))
                .unwrap();
            assert_eq!(result["deferred"], true);
            assert_eq!(result["ran"], false);
            assert!(result["scene"].is_null());
            assert!(result["analysis"].is_null());
            assert!(result["authored_unavailable"].is_null());
            let delta: ds_grid_engine::profile_atlas::ProfileAuthoredDelta =
                serde_json::from_value(result["authored_delta"].clone()).unwrap();
            assert_eq!(delta.work.computed_ground, 0);
            assert_eq!(delta.work.reused_ground, 2);
            assert_eq!(delta.model_revision, worker.current_revision().revision_id);
            assert_eq!(delta.computed_revision, original_scene.model_revision);
            assert!(delta.changes.iter().all(|change| !change
                .path
                .iter()
                .any(|part| part == "ground" || part == "terrain_points")));
            // Description is an authored Properties fact, separate from the
            // canonical staking comments painted as labels. Its geometry row
            // changes without duplicating unchanged glyph/label groups.
            assert!(result["authored_display_delta"]
                .as_object()
                .unwrap()
                .is_empty());
            assert_eq!(result["metrics"]["applied_entries"], 1);
            assert!(result["metrics"]["authored_delta_bytes"].as_u64().unwrap() < 64 * 1024);
            assert_eq!(
                result["calculation"]["computed_revision"],
                initial["model_revision"]
            );
            assert_eq!(result["calculation"]["required"], true);
            assert_eq!(result["calculation"]["scope"]["whole_model"], false);
            assert_eq!(
                runtime.held[&input.scope].review.base_scene(),
                Some(&original_scene)
            );
            assert!(Arc::ptr_eq(
                &assets,
                &runtime.held[&input.scope].source.assets
            ));
            base = worker.current_revision().revision_id.to_string();
        }
        assert_eq!(runtime.held[&input.scope].pending_receipts.len(), 2);
        // The eventual explicit calculation receives both native receipts.
        // Only the authored alignment is projected; the other band is reused.
        refresh_surface(&mut runtime, &input, &worker, 1, 1);
        assert!(runtime.held[&input.scope].pending_receipts.is_empty());
        let calculated = runtime.held[&input.scope]
            .review
            .base_scene()
            .unwrap()
            .clone();
        worker.undo().unwrap();
        let result = runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime, &input, &base, &worker,
            ))))
            .unwrap();
        assert_eq!(result["calculation"]["scope"]["whole_model"], true);
        assert_eq!(
            runtime.held[&input.scope].review.base_scene(),
            Some(&calculated)
        );
        let base = worker.current_revision().revision_id.to_string();
        let mut request = refresh(&runtime, &input, &base, &worker);
        request.analyze = true;
        assert!(runtime
            .execute(Request::Observe(Box::new(request)))
            .unwrap_err()
            .starts_with("invalid_profile_view:"));
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            worker.history_state()
        );
    }
    #[test]
    fn presentation_reuses_results_and_refuses_hidden_calculation_after_an_edit() {
        let input = request();
        let mut runtime = Runtime::default();
        let opened = runtime.execute(input.clone()).unwrap();
        let mut worker = GridSession::open(common::fixture());
        let mut presentation = refresh(&runtime, &input, &input.expected_revision, &worker);
        presentation.patch.visibility.insert(
            ds_command_kernel::profile_display::DisplayOption::Wire,
            false,
        );
        let rendered = runtime
            .execute(Request::Present(Box::new(presentation)))
            .unwrap();
        assert_eq!(
            rendered["scene"]["engineering"],
            opened["scene"]["engineering"]
        );
        assert_eq!(
            rendered["scene"]["conductor_spans"],
            opened["scene"]["conductor_spans"]
        );
        assert_eq!(rendered["ran"], false);
        describe(&mut worker, "one", "changed");
        let pending = refresh(&runtime, &input, &input.expected_revision, &worker);
        assert!(runtime
            .execute(Request::Present(Box::new(pending.clone())))
            .unwrap_err()
            .starts_with("profile_scene_unavailable:"));
        assert_eq!(
            runtime.held[&input.scope]
                .session
                .current_revision()
                .revision_id
                .to_string(),
            input.expected_revision
        );
        runtime
            .execute(Request::Observe(Box::new(pending)))
            .unwrap();
        let head = worker.current_revision().revision_id.to_string();
        let presentation = refresh(&runtime, &input, &head, &worker);
        assert!(runtime
            .execute(Request::Present(Box::new(presentation)))
            .unwrap_err()
            .starts_with("profile_scene_unavailable:"));
        assert_eq!(runtime.held[&input.scope].pending_receipts.len(), 1);
        runtime
            .execute(refresh(&runtime, &input, &head, &worker))
            .unwrap();
        let mut presentation = refresh(&runtime, &input, &head, &worker);
        presentation.patch.terrain = Some(ds_command_kernel::profile_display::TerrainSettings {
            corridor_half_width_m: 25.0,
            ..runtime.views[&input.scope].terrain.clone()
        });
        assert!(runtime
            .execute(Request::Present(Box::new(presentation)))
            .unwrap_err()
            .starts_with("profile_scene_unavailable:"));
    }
    #[test]
    fn retained_future_command_does_not_expand_target_surface_scope() {
        let (input, mut worker) = two_band_request();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        describe(&mut worker, "active", "active local command");
        let mut weather = worker.snapshot().weather_states[0].clone();
        weather.temperature_c += 1.;
        worker
            .apply_command(ds_grid_engine::CommandEnvelope::new(
                "future-weather",
                worker.current_revision().revision_id.clone(),
                ds_grid_engine::GridCommand::UpdateWeatherState {
                    id: weather.id.clone(),
                    row: weather,
                },
            ))
            .unwrap();
        worker.undo().unwrap();
        refresh_surface(&mut runtime, &input, &worker, 1, 1);
        assert_eq!(
            runtime.held[&input.scope]
                .session
                .history_state()
                .redo_depth,
            1
        );
        worker.redo().unwrap();
        refresh_surface(&mut runtime, &input, &worker, 2, 0);
    }
    #[test]
    fn unavailable_analyze_style_cannot_advance_review_against_old_held_session() {
        let (input, mut worker) = two_band_request();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let before = runtime.held[&input.scope].session.history_state();
        describe(&mut worker, "one", "new head not yet admitted");
        let mut unavailable = request_for(worker.snapshot().clone());
        unavailable.style_document = Value::Null;
        unavailable.analyze = true;
        let result = runtime.execute(unavailable).unwrap();
        assert!(result["scene"].is_null());
        assert_eq!(runtime.held[&input.scope].session.history_state(), before);
        assert_eq!(
            runtime.held[&input.scope]
                .review
                .surface_work()
                .unwrap()
                .computed_bands,
            2
        );
        refresh_surface(&mut runtime, &input, &worker, 1, 1);
    }
    #[test]
    fn repeated_content_revision_preserves_the_exact_journal_cursor() {
        let input = request();
        let mut worker = GridSession::open(common::fixture());
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let stale = refresh(&runtime, &input, &input.expected_revision, &worker);
        describe(&mut worker, "one", "temporary description");
        worker
            .apply_command(ds_grid_engine::CommandEnvelope::new(
                "two",
                worker.current_revision().revision_id.clone(),
                ds_grid_engine::GridCommand::DescribeStructure {
                    id: ds_grid_model::StructureId::new("str-2").unwrap(),
                    description: None,
                },
            ))
            .unwrap();
        assert_eq!(
            worker.current_revision().revision_id.to_string(),
            input.expected_revision
        );
        runtime
            .execute(refresh(&runtime, &input, &input.expected_revision, &worker))
            .unwrap();
        assert!(runtime
            .execute(stale)
            .unwrap_err()
            .starts_with("profile_replay_stale:"));
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            worker.history_state()
        );
        let mut bad = refresh(&runtime, &input, &input.expected_revision, &worker);
        let mut journal = worker.retained_journal();
        journal.entries[1].command_id = "one".into();
        bad.journal_json = serde_json::to_string(&journal).unwrap();
        assert!(runtime
            .execute(bad)
            .unwrap_err()
            .starts_with("profile_replay_diverged:"));
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            worker.history_state()
        );
    }
    #[test]
    fn unresolved_attachment_draft_refreshes_in_ram_without_pack_validation() {
        use ds_grid_model::*;
        let mut snapshot = common::fixture();
        let mut target = snapshot.structure_types[0].clone();
        target.id = StructureTypeId::new("target-type").unwrap();
        target.engineering_name = "Unresolved target".into();
        snapshot.structure_types.push(target);
        let mut geometry = snapshot.structure_geometry[0].clone();
        geometry.id = StructureMemberId::new("target-geometry").unwrap();
        geometry.structure_type_id = StructureTypeId::new("target-type").unwrap();
        snapshot.structure_geometry.push(geometry);
        snapshot.tension_sections.push(TensionSectionRow {
            id: TensionSectionId::new("draft-section").unwrap(),
            cable_id: CableId::new("cb-1").unwrap(),
            criterion_set_id: None,
            sag: AuthoredSag::HorizontalTension { tension_n: 20_000. },
            sag_condition: Some(CableCondition::Initial),
            sag_weather_state_id: Some(WeatherStateId::new("ws-everyday").unwrap()),
            ruling_span_m: Some(300.),
            details: Default::default(),
        });
        for (sequence, id) in ["str-1", "str-2", "str-3"].into_iter().enumerate() {
            snapshot
                .tension_section_supports
                .push(TensionSectionSupportRow {
                    section_id: TensionSectionId::new("draft-section").unwrap(),
                    sequence: sequence as u32,
                    structure_id: StructureId::new(id).unwrap(),
                    attachment_point_id: Some(AttachmentPointId::new("ap-left").unwrap()),
                    insulator_model: None,
                });
        }
        let input = request_for(snapshot.clone());
        let mut worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        worker
            .apply_command(ds_grid_engine::CommandEnvelope::new(
                "draft-retype",
                worker.current_revision().revision_id.clone(),
                ds_grid_engine::GridCommand::RetypeStructure {
                    id: StructureId::new("str-2").unwrap(),
                    structure_type_id: StructureTypeId::new("target-type").unwrap(),
                },
            ))
            .unwrap();
        assert!(
            !worker.is_valid(),
            "the unresolved draft must fail package validation"
        );
        let result = runtime
            .execute(refresh(&runtime, &input, &input.expected_revision, &worker))
            .unwrap();
        assert_eq!(
            result["model_revision"],
            worker.current_revision().revision_id.to_string()
        );
        assert_eq!(
            runtime.held[&input.scope].session.validation(),
            worker.validation()
        );
        assert_eq!(
            runtime.held[&input.scope].session.snapshot(),
            worker.snapshot()
        );
        let seed = ds_grid_exchange::profile_session::ProfileSessionSeed::capture(
            &worker,
            EntityId::new("profile-test").unwrap(),
            Default::default(),
            &[],
        );
        let mut initialized = Runtime::default();
        initialized
            .execute(InitializeRequest {
                publication_id: None,
                scope: input.scope.clone(),
                seed_json: serde_json::to_string(&seed).unwrap(),
                restore_state: None,
                options: input.options.clone(),
                style_document: input.style_document.clone(),
                patch: Default::default(),
                analyze: true,
            })
            .unwrap();
        assert_eq!(
            initialized.held[&input.scope].session.validation(),
            worker.validation()
        );
        let draft = worker.current_revision().revision_id.to_string();
        worker.undo().unwrap();
        initialized
            .execute(refresh(&initialized, &input, &draft, &worker))
            .unwrap();
        assert!(initialized.held[&input.scope].session.is_valid());
    }
    #[test]
    fn initialization_wire_replays_prior_edits_retains_redo_and_refuses_scope_reuse() {
        let input = request();
        let mut worker = GridSession::open(common::fixture());
        describe(&mut worker, "one", "initial edit");
        describe(&mut worker, "two", "redo tail");
        worker.undo().unwrap();
        let seed = ds_grid_exchange::profile_session::ProfileSessionSeed::capture(
            &worker,
            EntityId::new("profile-test").unwrap(),
            Default::default(),
            &[ds_grid_exchange::AssetBytes {
                invariant_leaf: "source.def".into(),
                bytes: b"native evidence".to_vec(),
            }],
        );
        let wire = json!({"operation":"initialize", "scope":input.scope, "seed_json":serde_json::to_string(&seed).unwrap(),
            "options":input.options, "style_document":input.style_document, "patch":{}, "analyze":true});
        let mut runtime = Runtime::default();
        let result = runtime
            .execute(serde_json::from_value::<Request>(wire.clone()).unwrap())
            .unwrap();
        assert_eq!(result["history"], json!(worker.history_state()));
        assert_eq!(result["retained_asset_count"], 1);
        assert_eq!(
            result["analysis"]["model_revision"],
            worker.current_revision().revision_id.to_string()
        );
        assert_eq!(
            runtime.held[&input.scope].session.retained_journal(),
            worker.retained_journal()
        );
        assert!(runtime
            .execute(serde_json::from_value::<Request>(wire).unwrap())
            .unwrap_err()
            .starts_with("profile_replay_scope_initialized:"));
        let middle = worker.current_revision().revision_id.to_string();
        worker.redo().unwrap();
        runtime
            .execute(refresh(&runtime, &input, &middle, &worker))
            .unwrap();
        let tip = worker.current_revision().revision_id.to_string();
        let entry = GridSession::open(common::fixture()).history_state();
        let current = worker.history_state();
        worker
            .discard_edits_at_head(&ds_grid_engine::SessionDiscardRequest {
                expected_revision: current.model_revision,
                expected_undo_depth: current.undo_depth,
                expected_history_pin: current.history_pin,
                target_revision: entry.model_revision,
                target_undo_depth: 0,
                target_redo_depth: 0,
                target_history_pin: entry.history_pin,
            })
            .unwrap();
        runtime
            .execute(refresh(&runtime, &input, &tip, &worker))
            .unwrap();
        assert_eq!(
            runtime.held[&input.scope].session.retained_journal(),
            worker.retained_journal()
        );
        assert!(runtime
            .held
            .get_mut(&input.scope)
            .unwrap()
            .session
            .redo()
            .is_err());
    }
    #[test]
    fn recovered_initialization_preserves_native_choices_and_refuses_invalid_restore_atomically() {
        let mut input = request();
        input.patch = serde_json::from_value(
            json!({"visibility":{"wire":false}, "vertical_exaggeration":8.0}),
        )
        .unwrap();
        let admitted = Runtime::default().execute(input.clone()).unwrap();
        let seed = ds_grid_exchange::profile_session::ProfileSessionSeed::capture(
            &GridSession::open(common::fixture()),
            EntityId::new("profile-test").unwrap(),
            Default::default(),
            &[],
        );
        let mut wire = json!({"operation":"initialize", "scope":"recovered", "seed_json":serde_json::to_string(&seed).unwrap(),
            "restore_state":admitted["display_state"], "options":input.options, "style_document":input.style_document,
            "patch":{"visibility":{"ground":false}}, "analyze":false});
        let mut restored = Runtime::default();
        let receipt = restored
            .execute(serde_json::from_value::<Request>(wire.clone()).unwrap())
            .unwrap();
        assert_eq!(receipt["display_state"]["vertical_exaggeration"], 8.0);
        assert_eq!(receipt["display_state"]["visibility"]["wire"], false);
        assert_eq!(receipt["display_state"]["visibility"]["ground"], false);
        wire["scope"] = json!("invalid");
        wire["restore_state"]["vertical_exaggeration"] = json!(-1);
        assert!(restored
            .execute(serde_json::from_value::<Request>(wire.clone()).unwrap())
            .is_err());
        assert!(!restored.views.contains_key("invalid"));
        assert!(!restored.held.contains_key("invalid"));
        wire["restore_state"] = admitted["display_state"].clone();
        wire["style_document"] = Value::Null;
        assert!(restored
            .execute(serde_json::from_value::<Request>(wire).unwrap())
            .is_err());
        assert!(!restored.views.contains_key("invalid"));
        assert!(!restored.held.contains_key("invalid"));
        assert!(restored.held.contains_key("recovered"));
    }
    #[test]
    fn opaque_wire_preserves_signed_zero_and_refuses_corruption_transactionally() {
        let mut snapshot = common::fixture();
        snapshot.structures[1].profile_offset_m = Some(-0.0);
        snapshot.structures[1].orientation_rad = -0.0;
        let input = request_for(snapshot.clone());
        let mut worker = GridSession::open(snapshot);
        let seed = ds_grid_exchange::profile_session::ProfileSessionSeed::capture(
            &worker,
            EntityId::new("profile-test").unwrap(),
            Default::default(),
            &[],
        );
        let seed_json = serde_json::to_string(&seed).unwrap();
        assert!(seed_json.contains("-0.0"));
        let wire = json!({"operation":"initialize", "scope":input.scope, "seed_json":seed_json,
            "options":input.options, "style_document":input.style_document, "patch":{}, "analyze":false});
        let mut runtime = Runtime::default();
        runtime
            .execute(serde_json::from_str::<Request>(&wire.to_string()).unwrap())
            .unwrap();
        assert_eq!(
            runtime.held[&input.scope].session.current_revision(),
            worker.current_revision()
        );
        assert!(runtime.held[&input.scope].session.snapshot().structures[1]
            .profile_offset_m
            .unwrap()
            .is_sign_negative());
        let mut corrupted = wire.clone();
        corrupted["scope"] = json!("corrupt");
        corrupted["seed_json"] = json!(seed_json.replace("-0.0", "0"));
        assert!(runtime
            .execute(serde_json::from_value::<Request>(corrupted).unwrap())
            .unwrap_err()
            .starts_with("profile_seed_invalid:"));
        assert!(!runtime.held.contains_key("corrupt"));
        let mut obsolete = wire;
        obsolete.as_object_mut().unwrap().remove("seed_json");
        obsolete["seed"] = json!(seed);
        assert!(serde_json::from_value::<Request>(obsolete).is_err());

        let row = worker.snapshot().structures[1].clone();
        worker
            .apply_command(ds_grid_engine::CommandEnvelope::new(
                "move-zero",
                worker.current_revision().revision_id.clone(),
                ds_grid_engine::GridCommand::MoveStructure {
                    id: row.id,
                    x_m: row.x_m,
                    y_m: row.y_m,
                    z_m: row.z_m,
                    orientation_rad: -0.0,
                },
            ))
            .unwrap();
        let mut request = refresh(&runtime, &input, &input.expected_revision, &worker);
        // A JS outer JSON envelope quotes the opaque journal, preserving its bytes.
        request.journal_json =
            serde_json::from_str(&serde_json::to_string(&request.journal_json).unwrap()).unwrap();
        assert!(request.journal_json.contains("-0.0"));
        let original = request.journal_json.clone();
        request.journal_json = original.replace("-0.0", "0");
        assert!(runtime
            .execute(request)
            .unwrap_err()
            .starts_with("profile_replay_diverged:"));
        assert_eq!(
            runtime.held[&input.scope]
                .session
                .current_revision()
                .revision_id
                .to_string(),
            input.expected_revision
        );
        runtime
            .execute(refresh(&runtime, &input, &input.expected_revision, &worker))
            .unwrap();
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            worker.history_state()
        );
        assert!(runtime.held[&input.scope].session.snapshot().structures[1]
            .orientation_rad
            .is_sign_negative());
        let mut malformed = refresh(
            &runtime,
            &input,
            worker.current_revision().revision_id.as_str(),
            &worker,
        );
        malformed.journal_json = "{".into();
        assert!(runtime
            .execute(malformed)
            .unwrap_err()
            .starts_with("profile_replay_invalid:"));
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            worker.history_state()
        );
    }
    #[test]
    fn scope_capacity_preserves_live_sessions_and_release_frees_exact_assets() {
        let input = request();
        let worker = GridSession::open(common::fixture());
        let seed = ds_grid_exchange::profile_session::ProfileSessionSeed::capture(
            &worker,
            EntityId::new("profile-test").unwrap(),
            Default::default(),
            &[ds_grid_exchange::AssetBytes {
                invariant_leaf: "retained.def".into(),
                bytes: b"native retained asset".to_vec(),
            }],
        );
        let initialize = |scope: &str| InitializeRequest {
            publication_id: None,
            scope: scope.into(),
            seed_json: serde_json::to_string(&seed).unwrap(),
            restore_state: None,
            options: input.options.clone(),
            style_document: input.style_document.clone(),
            patch: Default::default(),
            analyze: true,
        };
        let mut runtime = Runtime::default();
        runtime.execute(initialize("a-live")).unwrap();
        let weak = Arc::downgrade(&runtime.held["a-live"].source.assets);
        for i in 0..7 {
            runtime.execute(initialize(&format!("other-{i}"))).unwrap();
        }
        for i in 0..3 {
            assert!(runtime
                .execute(initialize(&format!("refused-{i}")))
                .unwrap_err()
                .starts_with("profile_replay_capacity:"));
            let mut current = input.clone();
            current.scope = "a-live".into();
            runtime
                .execute(refresh(
                    &runtime,
                    &current,
                    &input.expected_revision,
                    &worker,
                ))
                .unwrap();
        }
        assert_eq!(runtime.held.len(), 8);
        assert_eq!(runtime.views.len(), 8);
        assert_eq!(runtime.analyses.len(), 8);
        assert!(runtime
            .execute(ReleaseRequest {
                scope: "a-live".into(),
                model_id: "different-model".into()
            })
            .unwrap_err()
            .starts_with("profile_replay_model_mismatch:"));
        assert!(weak.upgrade().is_some());
        let released = runtime
            .execute(
                serde_json::from_value::<Request>(json!({
                    "operation":"release", "scope":"a-live", "model_id":"profile-test",
                }))
                .unwrap(),
            )
            .unwrap();
        assert_eq!(released["released"], true);
        assert!(
            weak.upgrade().is_none(),
            "release drops the native retained source bytes"
        );
        assert!(!runtime.held.contains_key("a-live"));
        assert!(!runtime.views.contains_key("a-live"));
        assert!(!runtime.analyses.contains_key("a-live"));
        assert_eq!(
            runtime
                .execute(ReleaseRequest {
                    scope: "a-live".into(),
                    model_id: "profile-test".into()
                })
                .unwrap()["released"],
            false
        );
        let mut current = input.clone();
        current.scope = "a-live".into();
        let stale = RefreshRequest {
            publication_id: None,
            scope: current.scope.clone(),
            model_id: "profile-test".into(),
            base_revision: worker.current_revision().revision_id.clone(),
            base_history: worker.history_state(),
            expected_revision: worker.current_revision().revision_id.clone(),
            target_history: worker.history_state(),
            journal_json: serde_json::to_string(&worker.retained_journal()).unwrap(),
            options: current.options.clone(),
            style_document: current.style_document.clone(),
            patch: Default::default(),
            analyze: false,
        };
        assert!(runtime
            .execute(stale)
            .unwrap_err()
            .starts_with("profile_replay_base_unavailable:"));
        runtime.execute(initialize("replacement-slot")).unwrap();
        assert!(runtime.held.contains_key("other-0"));
        assert_eq!(runtime.held.len(), 8);
    }
    fn section_demand_fixture() -> ds_grid_model::GridModelSnapshot {
        use ds_grid_model::*;
        let mut snapshot = common::fixture();
        snapshot.tension_sections.push(TensionSectionRow {
            id: TensionSectionId::new("cache-section").unwrap(),
            cable_id: snapshot.cables[0].id.clone(),
            criterion_set_id: Some(snapshot.criterion_sets[0].id.clone()),
            sag: AuthoredSag::HorizontalTension { tension_n: 5000. },
            sag_condition: Some(CableCondition::Initial),
            sag_weather_state_id: Some(snapshot.weather_states[0].id.clone()),
            ruling_span_m: None,
            details: Default::default(),
        });
        for (sequence, structure) in snapshot.structures.iter().enumerate() {
            snapshot
                .tension_section_supports
                .push(TensionSectionSupportRow {
                    section_id: snapshot.tension_sections[0].id.clone(),
                    sequence: sequence as u32,
                    structure_id: structure.id.clone(),
                    attachment_point_id: Some(snapshot.attachment_points[0].id.clone()),
                    insulator_model: None,
                });
        }
        let mut adjacent = snapshot.tension_sections[0].clone();
        adjacent.id = TensionSectionId::new("unchanged-section").unwrap();
        for (sequence, index) in [0, 2].into_iter().enumerate() {
            snapshot
                .tension_section_supports
                .push(TensionSectionSupportRow {
                    section_id: adjacent.id.clone(),
                    sequence: sequence as u32,
                    structure_id: snapshot.structures[index].id.clone(),
                    attachment_point_id: Some(snapshot.attachment_points[1].id.clone()),
                    insulator_model: None,
                });
        }
        snapshot.tension_sections.push(adjacent);
        snapshot
    }
    #[test]
    fn future_redo_weather_never_invalidates_current_cursor_section_demands() {
        let snapshot = section_demand_fixture();
        let cases = snapshot.analysis_cases.len();
        let input = request_for(snapshot.clone());
        let mut worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        describe(
            &mut worker,
            "active-description",
            "only one section changed",
        );
        let mut weather = worker.snapshot().weather_states[0].clone();
        weather.temperature_c += 1.;
        worker
            .apply_command(ds_grid_engine::CommandEnvelope::new(
                "future-weather",
                worker.current_revision().revision_id.clone(),
                ds_grid_engine::GridCommand::UpdateWeatherState {
                    id: weather.id.clone(),
                    row: weather,
                },
            ))
            .unwrap();
        worker.undo().unwrap();
        let mut corrupt = refresh(&runtime, &input, &input.expected_revision, &worker);
        let mut journal: Value = serde_json::from_str(&corrupt.journal_json).unwrap();
        journal["entries"][1]["resulting_revision"] = json!("fnv1a64:0000000000000000");
        corrupt.journal_json = journal.to_string();
        let before = runtime.held[&input.scope].session.history_state();
        assert!(runtime
            .execute(corrupt)
            .unwrap_err()
            .starts_with("profile_replay_diverged:"));
        assert_eq!(runtime.held[&input.scope].session.history_state(), before);
        let result = runtime
            .execute(refresh(&runtime, &input, &input.expected_revision, &worker))
            .unwrap();
        let work = runtime.held[&input.scope].demand_cache.last_demand_work();
        assert_eq!(
            work.computed_cases, cases,
            "future redo must not clear the unchanged section"
        );
        assert_eq!(work.reused_cases, cases);
        assert_eq!(result["metrics"]["replayed_entries"], 2);
        assert_eq!(result["metrics"]["applied_entries"], 1);
        assert_eq!(result["metrics"]["last_demand_computed_cases"], cases);
        assert_eq!(result["metrics"]["last_demand_reused_cases"], cases);
        let metrics = result["metrics"].as_object().unwrap();
        assert!(metrics
            .values()
            .all(|value| value.as_f64().is_some_and(|n| n.is_finite() && n >= 0.)));
        assert!(
            metrics["replay_apply_total_ms"].as_f64().unwrap()
                <= metrics["replay_ms"].as_f64().unwrap() + 0.001
        );
        assert_eq!(
            runtime.held[&input.scope].session.history_state(),
            worker.history_state()
        );
        let fresh = Runtime::default()
            .execute(request_for(worker.snapshot().clone()))
            .unwrap();
        assert_eq!(result["scene"], fresh["scene"]);
        let base = worker.current_revision().revision_id.to_string();
        worker.redo().unwrap();
        let redone = runtime
            .execute(refresh(&runtime, &input, &base, &worker))
            .unwrap();
        let fresh = Runtime::default()
            .execute(request_for(worker.snapshot().clone()))
            .unwrap();
        assert_eq!(redone["scene"], fresh["scene"]);
        assert_eq!(redone["metrics"]["replayed_entries"], 0);
        assert_eq!(redone["metrics"]["last_demand_computed_cases"], cases * 2);
    }
    #[test]
    fn refresh_carries_demand_cache_through_native_receipts_but_not_navigation() {
        use ds_grid_model::*;
        let snapshot = section_demand_fixture();
        let selection = ds_grid_engine::structure_usage::StructureUsageScreeningRequest {
            criterion_set_id: snapshot.criterion_sets[0].id.clone(),
            analysis_case_ids: snapshot
                .analysis_cases
                .iter()
                .map(|case| case.id.clone())
                .collect(),
            weight_span_basis: WeightSpanBasis::Conventional,
            structure_ids: vec![],
        };
        let input = request_for(snapshot.clone());
        let mut worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let screen = |held: &mut HeldView| {
            held.demand_cache
                .screen(
                    held.session.snapshot(),
                    &held.session.current_revision().revision_id,
                    &selection,
                )
                .unwrap()
        };
        assert_eq!(
            runtime.held[&input.scope]
                .demand_cache
                .last_demand_work()
                .computed_cases,
            2 * selection.analysis_case_ids.len()
        );
        screen(runtime.held.get_mut(&input.scope).unwrap());
        assert_eq!(
            runtime.held[&input.scope]
                .demand_cache
                .last_demand_work()
                .computed_cases,
            0
        );
        describe(
            &mut worker,
            "annotation",
            "only one incident section invalidates",
        );
        runtime
            .execute(refresh(&runtime, &input, &input.expected_revision, &worker))
            .unwrap();
        let held = runtime.held.get_mut(&input.scope).unwrap();
        assert_eq!(
            held.demand_cache.last_demand_work().computed_cases,
            selection.analysis_case_ids.len()
        );
        assert_eq!(
            held.demand_cache.last_demand_work().reused_cases,
            selection.analysis_case_ids.len()
        );
        let cached = screen(held);
        assert_eq!(held.demand_cache.last_demand_work().computed_cases, 0);
        assert_eq!(
            cached,
            ds_grid_engine::structure_usage::structure_usage_screening(
                worker.snapshot(),
                &selection
            )
            .unwrap()
        );
        let analysis_request = ds_grid_engine::DefaultAnalysisRequest {
            expected_revision: held.session.current_revision().revision_id.clone(),
            expected_engineering_input_root: held
                .session
                .current_revision()
                .roots
                .engineering_input_root
                .clone(),
            max_rows: 2000,
        };
        let report = held
            .session
            .analyze_model_defaults_with_cache(&analysis_request, &mut held.demand_cache)
            .unwrap();
        assert_eq!(
            report,
            held.session
                .analyze_model_defaults(&analysis_request)
                .unwrap()
        );
        assert_eq!(held.demand_cache.last_demand_work().computed_cases, 0);
        assert_eq!(
            held.demand_cache.last_demand_work().reused_cases,
            2 * selection.analysis_case_ids.len()
        );
        let edited = worker.current_revision().revision_id.to_string();
        worker.undo().unwrap();
        runtime
            .execute(refresh(&runtime, &input, &edited, &worker))
            .unwrap();
        let held = runtime.held.get_mut(&input.scope).unwrap();
        assert!(held.demand_cache.last_demand_work().computed_cases > 0);
        assert_eq!(held.demand_cache.last_demand_work().reused_cases, 0);
        screen(held);
        assert_eq!(held.demand_cache.last_demand_work().computed_cases, 0);
    }
    #[test]
    fn analyze_populates_shared_demands_before_its_issue_screen() {
        use ds_grid_model::*;
        let mut snapshot = common::fixture();
        snapshot.tension_sections.push(TensionSectionRow {
            id: TensionSectionId::new("analyzed-section").unwrap(),
            cable_id: snapshot.cables[0].id.clone(),
            criterion_set_id: Some(snapshot.criterion_sets[0].id.clone()),
            sag: AuthoredSag::HorizontalTension { tension_n: 5000. },
            sag_condition: Some(CableCondition::Initial),
            sag_weather_state_id: Some(snapshot.weather_states[0].id.clone()),
            ruling_span_m: None,
            details: Default::default(),
        });
        for (sequence, structure) in snapshot.structures.iter().enumerate() {
            snapshot
                .tension_section_supports
                .push(TensionSectionSupportRow {
                    section_id: snapshot.tension_sections[0].id.clone(),
                    sequence: sequence as u32,
                    structure_id: structure.id.clone(),
                    attachment_point_id: Some(snapshot.attachment_points[0].id.clone()),
                    insulator_model: None,
                });
        }
        let session = GridSession::open(snapshot);
        let mut cache = ds_grid_engine::StructureUsageScreeningCache::default();
        let case = ds_grid_engine::profile_display_case::ProfileDisplayCase::default();
        let cached = analysis::analyze_with_cache(&session, &case, None, None, &mut cache);
        assert_eq!(cached, analysis::analyze(&session, &case, None, None));
        assert_eq!(cached["report"]["status"], "ok");
        assert_eq!(
            cache.last_demand_work().computed_cases,
            0,
            "the issue screen must not solve default analysis demands again"
        );
        assert!(cache.last_demand_work().reused_cases > 0);
    }
    #[test]
    fn later_replay_or_projection_refusal_preserves_history_display_assets_and_analysis() {
        let mut input = request();
        input.analyze = true;
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let analysis = runtime.analyses[&input.scope].clone();
        let history = runtime.held[&input.scope].session.history_state();
        let mut worker = GridSession::open(common::fixture());
        describe(&mut worker, "one", "first staged edit");
        describe(&mut worker, "two", "second staged edit");
        let mut bad = refresh(&runtime, &input, &input.expected_revision, &worker);
        let mut journal = worker.retained_journal();
        journal.entries[1].resulting_revision =
            serde_json::from_value(json!("wrong-result")).unwrap();
        bad.expected_revision = journal.entries[1].resulting_revision.clone();
        journal.history.model_revision = bad.expected_revision.clone();
        bad.journal_json = serde_json::to_string(&journal).unwrap();
        bad.target_history.model_revision = bad.expected_revision.clone();
        assert!(runtime
            .execute(bad)
            .unwrap_err()
            .starts_with("profile_replay_diverged:"));
        let admitted = runtime.views[&input.scope].clone();
        runtime
            .views
            .get_mut(&input.scope)
            .unwrap()
            .terrain
            .gap_tolerance_m = -1.0;
        let bad = refresh(&runtime, &input, &input.expected_revision, &worker);
        assert!(runtime
            .execute(bad)
            .unwrap_err()
            .starts_with("profile_scene_unavailable:"));
        runtime.views.insert(input.scope.clone(), admitted);
        assert_eq!(runtime.held[&input.scope].session.history_state(), history);
        assert_eq!(runtime.analyses[&input.scope], analysis);
        runtime
            .execute(refresh(&runtime, &input, &input.expected_revision, &worker))
            .unwrap();
        assert!(runtime
            .execute(refresh(&runtime, &input, &input.expected_revision, &worker))
            .unwrap_err()
            .starts_with("profile_replay_stale:"));
    }
    #[test]
    fn replay_isolated_by_scope_and_model_even_with_identical_content_revisions() {
        let one = request();
        let mut two = one.clone();
        two.scope = "two".into();
        let mut runtime = Runtime::default();
        runtime.execute(one.clone()).unwrap();
        runtime.execute(two.clone()).unwrap();
        let mut worker = GridSession::open(common::fixture());
        describe(&mut worker, "one", "scoped edit");
        let mut wrong = refresh(&runtime, &one, &one.expected_revision, &worker);
        wrong.model_id = "replacement-model".into();
        assert!(runtime
            .execute(wrong)
            .unwrap_err()
            .starts_with("profile_replay_model_mismatch:"));
        runtime
            .execute(refresh(&runtime, &one, &one.expected_revision, &worker))
            .unwrap();
        assert_eq!(
            runtime.held[&two.scope]
                .session
                .current_revision()
                .revision_id
                .to_string(),
            two.expected_revision
        );
        let mut missing = refresh(&runtime, &two, &two.expected_revision, &worker);
        missing.scope = "missing".into();
        assert!(runtime
            .execute(missing)
            .unwrap_err()
            .starts_with("profile_replay_base_unavailable:"));
        let mut wire = serde_json::json!({"operation":"refresh", "scope":"one", "model_id":"profile-test",
            "base_revision":one.expected_revision, "base_history":runtime.held[&one.scope].session.history_state(),
            "target_history":worker.history_state(), "expected_revision":worker.current_revision().revision_id,
            "entries":worker.command_history(), "options":one.options, "style_document":one.style_document,
            "patch":{}, "analyze":false});
        wire["entries"][0]["command"]["command_kind"] = json!("unknown_command");
        assert!(serde_json::from_value::<Request>(wire).is_err());
    }
    #[test]
    fn package_derived_combined_protection_survives_native_replay() {
        let mut input = request();
        let (package, _) = ds_grid_exchange::dsgrid::ingest(&input.package_bytes).unwrap();
        let model_id = "combined.0123456789abcdef";
        let bytes = ds_grid_exchange::pack(
            &package.snapshot,
            &ds_grid_exchange::PackOptions {
                model_id: EntityId::new(model_id).unwrap(),
                model_revision: 0,
                coordinate_system: ModelCrs::new("EPSG:32736").unwrap(),
                presentation: Default::default(),
                library_pins: vec![],
                library_needs: vec![],
                assets: package.assets,
                exchange_bindings: Default::default(),
            },
        )
        .unwrap();
        input.package_bytes = bytes;
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let mut unprotected_worker = GridSession::open(common::fixture());
        describe(&mut unprotected_worker, "write", "must refuse");
        let mut attempt = refresh(
            &runtime,
            &input,
            &input.expected_revision,
            &unprotected_worker,
        );
        attempt.model_id = model_id.into();
        let error = runtime.execute(attempt).unwrap_err();
        assert!(error.starts_with("profile_replay_diverged:"), "{error}");
        assert!(
            error.contains(ds_grid_engine::model_protection::PROTECTED_CODE),
            "{error}"
        );
        assert_eq!(
            runtime.held[&input.scope]
                .session
                .current_revision()
                .revision_id
                .to_string(),
            input.expected_revision
        );
    }
    #[test]
    fn boxed_project_request_preserves_exact_native_wire_fields_and_package_bytes() {
        let original = request();
        let wire = serde_json::json!({
            "operation": "project", "scope": original.scope,
            "expected_revision": original.expected_revision,
            "package_bytes": original.package_bytes, "options": original.options,
            "style_document": original.style_document, "patch": original.patch,
            "analyze": original.analyze
        });
        let bytes = serde_json::to_vec(&wire).unwrap();
        let Request::Project(decoded) = serde_json::from_slice(&bytes).unwrap() else {
            panic!("project wire decoded as case request");
        };
        assert_eq!(decoded.package_bytes, original.package_bytes);
        assert_eq!(decoded.scope, original.scope);
        assert_eq!(decoded.expected_revision, original.expected_revision);
        assert_eq!(
            serde_json::to_value(&decoded.options).unwrap(),
            wire["options"]
        );
        assert_eq!(decoded.style_document, wire["style_document"]);
        assert_eq!(serde_json::to_value(&decoded.patch).unwrap(), wire["patch"]);
        assert_eq!(decoded.analyze, original.analyze);
    }
    #[test]
    fn refused_scene_projection_restores_owned_held_cache_without_cloning() {
        let mut runtime = Runtime::default();
        let mut original = request();
        original.patch.display_case = Some(serde_json::from_value(json!({
            "mode":"analysis_case", "analysis_case_id":common::fixture().analysis_cases[0].id
        })).unwrap());
        runtime.execute(original.clone()).unwrap();
        let prepared = runtime
            .execute(CaseRequest {
                scope: original.scope.clone(),
                model_id: "profile-test".into(),
                expected_history: runtime.held[&original.scope].session.history_state(),
                expected_revision: original.expected_revision.clone(),
                display_case:
                    ds_grid_engine::profile_display_case::ProfileDisplayCase::AnalysisCase {
                        analysis_case_id: common::fixture().analysis_cases[0].id.clone(),
                    },
                select: false,
                cached_only: false,
                sequence: 41,
            })
            .unwrap();
        let selected = runtime
            .execute(CaseRequest {
                scope: original.scope.clone(),
                model_id: "profile-test".into(),
                expected_history: runtime.held[&original.scope].session.history_state(),
                expected_revision: original.expected_revision.clone(),
                display_case:
                    ds_grid_engine::profile_display_case::ProfileDisplayCase::AnalysisCase {
                        analysis_case_id: common::fixture().analysis_cases[0].id.clone(),
                    },
                select: true,
                cached_only: true,
                sequence: 41,
            })
            .unwrap();
        let before = runtime.held[&original.scope]
            .review
            .base_scene()
            .unwrap()
            .clone();
        let before_layer = serde_json::to_value(
            runtime.held[&original.scope]
                .review
                .selected_layer()
                .unwrap(),
        )
        .unwrap();
        runtime
            .held
            .get_mut(&original.scope)
            .unwrap()
            .last_weather_sequence = 41;
        let admitted_state = runtime.views[&original.scope].clone();
        // Exercise the engine's genuine InvalidOptions refusal after review
        // ownership transfer. A corrupted internal display state is deliberately
        // injected here; the public patch boundary also rejects this value.
        runtime
            .views
            .get_mut(&original.scope)
            .unwrap()
            .terrain
            .gap_tolerance_m = -1.0;
        let error = runtime.execute(original.clone()).unwrap_err();
        assert!(error.starts_with("profile_scene_unavailable:"), "{error}");
        let held = runtime
            .held
            .get(&original.scope)
            .expect("refusal must retain previous checkpoint");
        assert_eq!(held.last_weather_sequence, 41);
        assert_eq!(
            serde_json::to_value(held.review.base_scene().unwrap()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        assert_eq!(
            serde_json::to_value(held.review.selected_layer().unwrap()).unwrap(),
            before_layer
        );
        runtime.views.insert(original.scope.clone(), admitted_state);
        let after = runtime
            .execute(CaseRequest {
                scope: original.scope.clone(),
                model_id: "profile-test".into(),
                expected_history: runtime.held[&original.scope].session.history_state(),
                expected_revision: original.expected_revision.clone(),
                display_case:
                    ds_grid_engine::profile_display_case::ProfileDisplayCase::AnalysisCase {
                        analysis_case_id: common::fixture().analysis_cases[0].id.clone(),
                    },
                select: true,
                cached_only: true,
                sequence: 42,
            })
            .expect("prior prepared weather cache remains usable after projection refusal");
        assert_eq!(after["command_view"], selected["command_view"]);
        assert_eq!(after["display_state"], selected["display_state"]);
        let retained = runtime
            .execute(CaseRequest {
                expected_history: runtime.held[&original.scope].session.history_state(),
                scope: original.scope,
                model_id: "profile-test".into(),
                expected_revision: original.expected_revision,
                display_case:
                    ds_grid_engine::profile_display_case::ProfileDisplayCase::AnalysisCase {
                        analysis_case_id: common::fixture().analysis_cases[0].id.clone(),
                    },
                select: false,
                cached_only: false,
                sequence: 43,
            })
            .unwrap();
        assert_eq!(retained["bytes"], prepared["bytes"]);
    }
}
