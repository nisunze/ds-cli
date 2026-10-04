//! Native Profile execution shared by the desktop host and headless callers.
//! The immutable checkpoint is pinned before work; no package is ever written.
use ds_command_kernel::profile_display::{DisplayPatch, DisplayState};
use ds_command_kernel::profile_encoding::{
    clearance_violation_encoding, resolve, review_box_ink, review_status_colors, DisplayRow,
    Encoding,
};
mod command_view;
mod decorations;
mod label_lanes;
use ds_grid_engine::{GridSession, ProfileAtlasOptions};
mod analysis;
pub use analysis::analyze;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

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
    CaseLayer(CaseRequest),
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

struct HeldView {
    session: GridSession,
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
    pub legend: Vec<ds_command_kernel::profile_encoding::LegendEntry>,
    pub label_fields: Vec<String>,
    pub scene_options: ProfileAtlasOptions,
    pub primitives: Vec<decorations::Primitive>,
    pub clearance_violation_encoding: Encoding,
    pub review_box_ink: String,
    pub review_status_colors: BTreeMap<String, String>,
}
#[derive(Default)]
pub struct Runtime {
    views: BTreeMap<String, DisplayState>,
    analyses: BTreeMap<String, Value>,
    held: BTreeMap<String, HeldView>,
}
impl Runtime {
    pub fn execute(&mut self, request: impl Into<Request>) -> Result<Value, String> {
        match request.into() {
            Request::Project(request) => self.project(*request),
            Request::CaseLayer(request) => self.case_layer(request),
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
        let mut command_view = command_view::project(
            &state,
            Some(&held.rows),
            Some(scene),
            self.analyses.get(&request.scope),
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
    fn project(&mut self, mut request: ProjectRequest) -> Result<Value, String> {
        if request.scope.is_empty()
            || request.scope.len() > 1024
            || request.package_bytes.len() > 512 * 1024 * 1024
        {
            return Err("invalid_profile_view: request exceeds native bounds".into());
        }
        let (package, _) = ds_grid_exchange::dsgrid::ingest(&request.package_bytes)
            .map_err(|error| format!("profile_package_invalid: {error}"))?;
        let presentation = package.manifest.model.presentation;
        let session = GridSession::open(package.snapshot);
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
        let last_weather_sequence = previous
            .as_ref()
            .map_or(0, |held| held.last_weather_sequence);
        let mut review = previous
            .as_mut()
            .map(|held| std::mem::take(&mut held.review))
            .unwrap_or_default();
        review.install_case(requested_case);
        let scene = review
            .scene(&session, request.options)
            .map_err(|error| format!("profile_scene_unavailable: {error}"));
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
            let analysis = analyze(
                &session,
                &scene_options.display_case,
                scene.as_ref().ok(),
                Some(error),
            );
            let command_view =
                command_view::project(&state, None, scene.as_ref().ok(), Some(&analysis));
            self.analyses
                .insert(request.scope.clone(), analysis.clone());
            self.commit_view(&request.scope, state.clone());
            return Ok(
                json!({"scene":null, "analysis":analysis, "display_state":state, "projection_unavailable":error, "command_view":command_view, "ran":true}),
            );
        }
        let mut scene = scene.unwrap();
        let (rows, review_status_colors, review_box_ink) = styles.unwrap();
        let mut label_policy = presentation.effective_profile_structure_labels();
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
            let value = analyze(&session, &scene_options.display_case, Some(&scene), None);
            self.analyses.insert(request.scope.clone(), value.clone());
            Some(value)
        } else {
            self.analyses
                .get(&request.scope)
                .filter(|value| value["model_revision"] == request.expected_revision)
                .cloned()
        };
        let command_view =
            command_view::project(&state, Some(&rows), Some(&scene), analysis.as_ref());
        self.commit_view(&request.scope, state.clone());
        self.held.insert(
            request.scope.clone(),
            HeldView {
                session,
                review,
                options: scene_options.clone(),
                rows: rows.clone(),
                label_policy: label_policy.clone(),
                last_weather_sequence,
            },
        );
        let primitives = decorations::project(&scene, &state, &rows, &label_policy);
        let base_axis = self.held[&request.scope].review.base_axis_digest().unwrap();
        let mut scene = serde_json::to_value(scene).map_err(|error| error.to_string())?;
        scene["case_base_axis_digest"] = json!(base_axis);
        scene["display"] = serde_json::to_value(Projection {
            label_fields: label_fields(&state),
            state: state.clone(),
            legend: ds_command_kernel::profile_encoding::legend(&rows),
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
            json!({ "scene": scene, "analysis": analysis, "display_state":state, "projection_unavailable":null, "command_view":command_view, "ran": request.analyze }),
        )
    }
    fn commit_view(&mut self, scope: &str, state: DisplayState) {
        self.views.insert(scope.to_owned(), state);
        while self.views.len() > 8 {
            let key = self
                .views
                .keys()
                .find(|key| key.as_str() != scope)
                .unwrap()
                .clone();
            self.views.remove(&key);
            self.analyses.remove(&key);
            self.held.remove(&key);
        }
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
        let session = GridSession::open(common::fixture());
        let (artifacts, _) = ds_grid_exchange::dsgrid::emit(
            session.snapshot(),
            &ds_grid_exchange::PackOptions {
                model_id: EntityId::new("profile-test").unwrap(),
                model_revision: 0,
                coordinate_system: ModelCrs::new("EPSG:32736").unwrap(),
                presentation: Default::default(),
                library_pins: vec![],
                library_needs: vec![],
                assets: vec![],
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
