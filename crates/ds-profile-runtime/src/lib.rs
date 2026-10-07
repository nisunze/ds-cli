//! Native Profile execution shared by the desktop host and headless callers.
//! Initialization pins a package once. Refresh replays the typed journal in RAM;
//! neither refresh nor history navigation writes or packs a package.
use ds_command_kernel::profile_display::{DisplayPatch, DisplayState};
use ds_command_kernel::profile_encoding::{
    clearance_violation_encoding, resolve, review_box_ink, review_status_colors, DisplayRow,
    Encoding,
};
mod command_view;
mod decorations;
mod label_lanes;
mod symbols;
use ds_grid_engine::{GridSession, ProfileAtlasOptions};
mod analysis;
pub use analysis::analyze;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
mod replay;
pub use replay::{InitializeRequest, RefreshRequest, ReleaseRequest};

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
    pub expected_revision: String,
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
struct HeldView {
    session: GridSession,
    source: HeldSource,
    demand_cache: ds_grid_engine::StructureUsageScreeningCache,
    review: ds_grid_engine::profile_engineering::ProfileReviewView,
    options: ProfileAtlasOptions,
    rows: Vec<DisplayRow>,
    label_policy: ds_grid_model::StructureLabelPolicy,
    last_weather_sequence: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Projection {
    pub state: DisplayState,
    pub rows: Vec<DisplayRow>,
    pub groups: Vec<ds_command_kernel::profile_controls::ControlGroup>,
    pub style_controls: Value,
    pub symbols: Vec<symbols::Symbol>,
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
}
impl Runtime {
    pub fn execute(&mut self, request: impl Into<Request>) -> Result<Value, String> {
        match request.into() {
            Request::Project(request) => self.project(*request),
            Request::Initialize(request) => self.initialize(*request),
            Request::Refresh(request) => self.refresh(*request),
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
    fn case_layer(&mut self, request: CaseRequest) -> Result<Value, String> {
        let held = self
            .held
            .get_mut(&request.scope)
            .ok_or("profile_case_base_unavailable: draw the active Profile first")?;
        if held.session.current_revision().revision_id.to_string() != request.expected_revision {
            return Err(
                "profile_selection_stale: cached Profile differs from the held revision".into(),
            );
        }
        if request.select && request.sequence <= held.last_weather_sequence {
            return Err(
                "profile_selection_stale: a newer weather selection already took over".into(),
            );
        }
        let current = self
            .views
            .get(&request.scope)
            .ok_or("profile_case_base_unavailable: display state is missing")?;
        let patch = DisplayPatch {
            display_case: Some(
                serde_json::from_value(serde_json::to_value(&request.display_case).unwrap())
                    .unwrap(),
            ),
            ..Default::default()
        };
        let state = current.patched(&patch)?;
        if request.cached_only {
            if !request.select {
                return Err("invalid_profile_view: cached receipt requires selection".into());
            }
            held.review
                .select_prepared_case(
                    &held.session,
                    request.display_case.clone(),
                    held.options.clone(),
                )
                .map_err(|e| format!("profile_case_unavailable: {e}"))?;
        }
        let mut case_primitives = None;
        let bytes = if request.cached_only {
            None
        } else {
            let mut scene = held
                .review
                .base_scene()
                .ok_or("profile_case_base_unavailable: scene is missing")?
                .clone();
            let layer = held
                .review
                .case_layer(
                    &held.session,
                    request.display_case.clone(),
                    held.options.clone(),
                    request.select,
                )
                .map_err(|e| format!("profile_case_unavailable: {e}"))?;
            layer.apply_to_scene(&mut scene);
            case_primitives = Some(
                decorations::project(&scene, &state, &held.rows, &held.label_policy)
                    .into_iter()
                    .filter(|primitive| {
                        matches!(
                            primitive.option,
                            ds_command_kernel::profile_display::DisplayOption::SpanDistances
                                | ds_command_kernel::profile_display::DisplayOption::SectionLabels
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            Some(layer.to_bytes().map_err(|e| e.to_string())?)
        };
        let scene = held
            .review
            .base_scene()
            .ok_or("profile_case_base_unavailable: scene is missing")?;
        let layer = held
            .review
            .prepared_layer(&request.display_case)
            .ok_or("profile_case_base_unavailable: requested layer is missing")?;
        // Only the held revision's analysis counts, so a weather switch after an edit
        // never reports the previous revision's analysis as current.
        let mut command_view = command_view::project(
            &state,
            Some(&held.rows),
            Some(scene),
            self.analyses
                .get(&request.scope)
                .filter(|value| value["model_revision"] == request.expected_revision),
            !held.session.snapshot().structures.is_empty(),
        );
        command_view["review"]["display_case"] =
            serde_json::to_value(&layer.display_case.requested).map_err(|e| e.to_string())?;
        command_view["review"]["selected_case_index"] = json!(layer.selected_case_index);
        let receipt = json!({"model_revision":request.expected_revision,"display_case":request.display_case,
            "axis_pin_digest":layer.axis_pin.digest,"bytes":bytes,"case_primitives":case_primitives,"display_state":state,"command_view":command_view});
        if request.select {
            held.last_weather_sequence = request.sequence;
            self.views.insert(request.scope, state);
        }
        Ok(receipt)
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
        request.options.vertical_exaggeration = Some(state.vertical_exaggeration);
        request.options.terrain_corridor_half_width_m = Some(state.terrain.corridor_half_width_m);
        request.options.terrain_gap_tolerance_m = Some(state.terrain.gap_tolerance_m);
        request.options.clearance_ground_offset_m = Some(state.terrain.ground_clearance_offset_m);
        request.options.clearance_lowest_wire_drop_m = Some(state.terrain.lowest_wire_drop_m);
        request.options.display_case =
            serde_json::from_value(serde_json::to_value(&state.display_case).unwrap())
                .map_err(|error| format!("invalid_profile_view: {error}"))?;
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
        review.install_case(requested_case);
        // Analyze may return an explicit unavailable projection. A style
        // refusal must not advance the retained review against the old session.
        let scene = if let Err(error) = &styles {
            Err(error.clone())
        } else {
            review
                .scene_after(
                    &session,
                    request.options,
                    if same_model { receipts } else { &[] },
                )
                .map_err(|error| format!("profile_scene_unavailable: {error}"))
        };
        let projection_error = styles.as_ref().err().or(scene.as_ref().err()).cloned();
        if let Some(error) = &projection_error {
            // A refused projection must retain the previously admitted checkpoint,
            // weather sequence and disposable cache. Transfer ownership back rather
            // than cloning the potentially large projection.
            if let Some(mut held) = previous.take() {
                review.install_case(
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
            return Ok(
                json!({"model_id":model_id, "model_revision":request.expected_revision, "history":history, "retained_asset_count":source.assets.len(), "scene":null, "analysis":analysis, "display_state":state, "projection_unavailable":error, "command_view":command_view, "ran":true}),
            );
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
        self.held.insert(
            request.scope.clone(),
            HeldView {
                session,
                source: source.clone(),
                demand_cache,
                review,
                options: scene_options.clone(),
                rows: rows.clone(),
                label_policy: label_policy.clone(),
                last_weather_sequence,
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
        let base_axis = self.held[&request.scope].review.base_axis_digest().unwrap();
        let legend = ds_command_kernel::profile_encoding::styled_legend(&state, &rows, &cables);
        let symbols = symbols::project(&scene, &state, &rows, &legend);
        let mut scene = serde_json::to_value(scene).map_err(|error| error.to_string())?;
        scene["case_base_axis_digest"] = json!(base_axis);
        scene["display"] = serde_json::to_value(Projection {
            label_fields: label_fields(&state),
            groups: ds_command_kernel::profile_controls::groups(&state, &rows, &cables),
            style_controls: ds_command_kernel::profile_style::controls(&state, &rows),
            symbols,
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
        Ok(
            json!({ "model_id":model_id, "model_revision":request.expected_revision, "history":history, "retained_asset_count":source.assets.len(), "scene": scene, "analysis": analysis, "display_state":state, "projection_unavailable":null, "command_view":command_view, "ran": request.analyze }),
        )
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
    fn request() -> ProjectRequest {
        request_for(common::fixture())
    }
    fn request_for(snapshot: ds_grid_model::GridModelSnapshot) -> ProjectRequest {
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
    fn refresh(
        runtime: &Runtime,
        input: &ProjectRequest,
        base: &str,
        session: &GridSession,
    ) -> RefreshRequest {
        RefreshRequest {
            scope: input.scope.clone(),
            model_id: "profile-test".into(),
            base_revision: serde_json::from_value(json!(base)).unwrap(),
            base_history: runtime.held[&input.scope].session.history_state(),
            expected_revision: session.current_revision().revision_id.clone(),
            target_history: session.history_state(),
            entries: session.retained_journal().entries,
            options: input.options.clone(),
            style_document: input.style_document.clone(),
            patch: Default::default(),
            analyze: false,
        }
    }
    fn describe(session: &mut GridSession, id: &str, text: &str) {
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
        bad.entries[1].command_id = "one".into();
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
                scope: input.scope.clone(),
                seed,
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
        let wire = json!({"operation":"initialize", "scope":input.scope, "seed":seed,
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
        let mut wire = json!({"operation":"initialize", "scope":"recovered", "seed":seed,
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
            scope: scope.into(),
            seed: seed.clone(),
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
            scope: current.scope.clone(),
            model_id: "profile-test".into(),
            base_revision: worker.current_revision().revision_id.clone(),
            base_history: worker.history_state(),
            expected_revision: worker.current_revision().revision_id.clone(),
            target_history: worker.history_state(),
            entries: vec![],
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
    #[test]
    fn refresh_carries_demand_cache_through_native_receipts_but_not_navigation() {
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
        screen(runtime.held.get_mut(&input.scope).unwrap());
        describe(
            &mut worker,
            "annotation",
            "only one incident section invalidates",
        );
        runtime
            .execute(refresh(&runtime, &input, &input.expected_revision, &worker))
            .unwrap();
        let held = runtime.held.get_mut(&input.scope).unwrap();
        let cached = screen(held);
        assert_eq!(
            held.demand_cache.last_demand_work().computed_cases,
            selection.analysis_case_ids.len()
        );
        assert_eq!(
            held.demand_cache.last_demand_work().reused_cases,
            selection.analysis_case_ids.len()
        );
        assert_eq!(
            cached,
            ds_grid_engine::structure_usage::structure_usage_screening(
                worker.snapshot(),
                &selection
            )
            .unwrap()
        );
        let edited = worker.current_revision().revision_id.to_string();
        worker.undo().unwrap();
        runtime
            .execute(refresh(&runtime, &input, &edited, &worker))
            .unwrap();
        let held = runtime.held.get_mut(&input.scope).unwrap();
        screen(held);
        assert!(held.demand_cache.last_demand_work().computed_cases > 0);
        assert_eq!(held.demand_cache.last_demand_work().reused_cases, 0);
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
        bad.entries[1].resulting_revision = serde_json::from_value(json!("wrong-result")).unwrap();
        bad.expected_revision = bad.entries[1].resulting_revision.clone();
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
        let original = request();
        runtime.execute(original.clone()).unwrap();
        let prepared = runtime
            .execute(CaseRequest {
                scope: original.scope.clone(),
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
                scope: original.scope,
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
