//! Queries consume complete typed native facts retained by explicit calculation.
//! No query enters projection, demand assembly, screening or package persistence.
use super::*;
use ds_grid_engine::{ClearanceStatus, RevisionId, SessionHistoryState};
use ds_grid_physics::EngineeringStatus;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_ITEMS: usize = 200;
const MAX_BYTES: usize = 256 * 1024;
static RESULT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

mod analysis_queries;

/// Independent admitted result sources; reading never changes the display case.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuerySource {
    #[default]
    Profile,
    ModelAnalysis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryKind {
    Usage,
    Failures,
    NegativeLoading,
    ViolatingSections,
    Qualification,
    Blockers,
    Findings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryCursor {
    pub result_id: String,
    pub scope: String,
    pub model_id: String,
    pub history: SessionHistoryState,
    pub kind: QueryKind,
    #[serde(default)]
    pub source: QuerySource,
    pub entity_ids: Vec<String>,
    pub offset: usize,
}

/// Shared CLI/MCP input DTO. The transport supplies the captured native
/// scope/model/history/root; all bounds and engineering predicates remain here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryArguments {
    pub kind: QueryKind,
    #[serde(default)]
    pub source: QuerySource,
    #[serde(default)]
    pub entity_ids: Vec<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub cursor: Option<QueryCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryResultsRequest {
    pub scope: String,
    pub model_id: String,
    pub expected_history: SessionHistoryState,
    pub expected_engineering_input_root: String,
    pub kind: QueryKind,
    #[serde(default)]
    pub source: QuerySource,
    /// Empty means the complete cached corpus. IDs name native entities,
    /// never scene-marker IDs or text predicates.
    #[serde(default)]
    pub entity_ids: Vec<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub cursor: Option<QueryCursor>,
}
fn default_limit() -> usize {
    50
}

#[derive(Clone)]
struct Fact {
    entity_id: Option<String>,
    value: Arc<Value>,
}
#[derive(Clone, Default)]
struct Facts {
    rows: Vec<Fact>,
    by_entity: BTreeMap<String, Vec<usize>>,
}
impl Facts {
    fn insert(&mut self, entity_id: Option<String>, value: Arc<Value>) {
        if let Some(id) = &entity_id {
            self.by_entity
                .entry(id.clone())
                .or_default()
                .push(self.rows.len());
        }
        self.rows.push(Fact { entity_id, value });
    }
    fn positions(&self, ids: &[String]) -> Vec<usize> {
        let mut positions: Vec<_> = ids
            .iter()
            .filter_map(|id| self.by_entity.get(id))
            .flatten()
            .copied()
            .collect();
        positions.sort_unstable();
        positions.dedup();
        positions
    }
}

/// One immutable index is shared by live editing, rollback and job capture.
/// Counts/indexes are built before query caps, from the identical native cache
/// selection and whole-model Profile evidence, never the capped analysis JSON.
#[derive(Clone)]
pub(super) struct Index {
    id: String,
    revision: RevisionId,
    root: String,
    case: ds_grid_engine::profile_display_case::ProfileDisplayCase,
    basis: Value,
    source: QuerySource,
    usage_available: bool,
    findings_available: bool,
    entities: BTreeSet<String>,
    structures: BTreeSet<String>,
    sections: BTreeSet<String>,
    facts: BTreeMap<QueryKind, Facts>,
}

fn bounded_text(message: &str) -> Value {
    let count = message.chars().count();
    json!({"message":message.chars().take(1024).collect::<String>(),
        "message_char_count":count,"message_truncated":count > 1024})
}
fn usage_fact(row: &ds_grid_engine::StructureUsageScreeningRow) -> Arc<Value> {
    let mut value = serde_json::to_value(row).expect("native screening row");
    for field in ["blockers", "unevaluated_checks", "advisories"] {
        let count = value[field].as_array().expect("native evidence list").len();
        value.as_object_mut().unwrap().remove(field);
        value[format!("{field}_count")] = json!(count);
    }
    Arc::new(json!({"kind":"structure_usage","row":value}))
}

impl Index {
    pub(super) fn at_input_root(&self, root: &str) -> bool {
        self.root == root
    }
    fn next_id() -> String {
        format!(
            "profile-results-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("native clock")
                .as_nanos(),
            RESULT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        )
    }
    /// An equal-input package projection can reuse the review without entering
    /// screening. Preserve its already retained complete facts too; a newly
    /// empty demand cache must not make those facts disappear.
    pub(super) fn identical_projection(
        &self,
        scene: &ds_grid_engine::ProfileAtlasScene,
    ) -> Option<Self> {
        if self.revision != scene.model_revision
            || self.root != scene.engineering_input_root
            || self.case != scene.display_case.requested
            || self.basis["usage"] != json!(scene.engineering.usage_request)
        {
            return None;
        }
        let mut index = self.clone();
        index.id = Self::next_id();
        Some(index)
    }
    pub(super) fn new(
        session: &GridSession,
        scene: &ds_grid_engine::ProfileAtlasScene,
        cache: &ds_grid_engine::StructureUsageScreeningCache,
    ) -> Self {
        let report = scene
            .engineering
            .usage_request
            .as_ref()
            .and_then(|request| cache.cached_report(&scene.model_revision, request));
        let usage_available = report.is_some_and(|report| {
            scene
                .engineering
                .usage_request
                .as_ref()
                .unwrap()
                .structure_ids
                .is_empty()
                && report.rows.len() == session.snapshot().structures.len()
                && report.summary.structure_count == report.rows.len()
        });
        let basis = json!({"usage":scene.engineering.usage_request,
            "usage_classification":report.map(|report| &report.classification),
            "usage_summary":report.map(|report| &report.summary),
            "usage_available":usage_available,
            "clearance_scope":"profile_display_case",
            "blocker_scope":"profile_curve_clearance_and_structure_usage",
            "blocker_count_unit":"native_evidence_record",
            "qualification_scope":"structure_usage_screening"});
        let mut index = Self {
            id: Self::next_id(),
            revision: scene.model_revision.clone(),
            root: scene.engineering_input_root.clone(),
            case: scene.display_case.requested.clone(),
            basis,
            source: QuerySource::Profile,
            usage_available,
            findings_available: false,
            structures: session
                .snapshot()
                .structures
                .iter()
                .map(|row| row.id.to_string())
                .collect(),
            sections: session
                .snapshot()
                .tension_sections
                .iter()
                .map(|row| row.id.to_string())
                .collect(),
            entities: session
                .snapshot()
                .structures
                .iter()
                .map(|row| row.id.to_string())
                .chain(
                    session
                        .snapshot()
                        .tension_sections
                        .iter()
                        .map(|row| row.id.to_string()),
                )
                .chain(
                    session
                        .snapshot()
                        .alignments
                        .iter()
                        .map(|row| row.id.to_string()),
                )
                .collect(),
            facts: BTreeMap::new(),
        };
        for kind in [
            QueryKind::Usage,
            QueryKind::Failures,
            QueryKind::NegativeLoading,
            QueryKind::ViolatingSections,
            QueryKind::Qualification,
            QueryKind::Blockers,
            QueryKind::Findings,
        ] {
            index.facts.insert(kind, Facts::default());
        }
        // Aggregate distinct violating sections. All contributor identities are
        // counted before their bounded presentation list is made.
        let mut sections: BTreeMap<String, Value> = BTreeMap::new();
        if let Some(report) = report {
            for row in &report.rows {
                let id = row.structure_id.to_string();
                let fact = usage_fact(row);
                index.add(QueryKind::Usage, Some(id.clone()), fact.clone());
                index.add(QueryKind::Qualification, Some(id.clone()), fact.clone());
                if row.engineering_status == Some(EngineeringStatus::Fail) {
                    index.add(QueryKind::Failures, Some(id.clone()), fact.clone());
                    for section in &row.failing_tension_section_ids {
                        let section = section.to_string();
                        let entry = sections.entry(section.clone()).or_insert_with(||
                            json!({"kind":"violating_section","section_id":section,
                                "capacity_failure_structure_ids":[],"capacity_failure_structure_count":0}));
                        let count = entry["capacity_failure_structure_count"].as_u64().unwrap();
                        entry["capacity_failure_structure_count"] = json!(count + 1);
                        if count < 50 {
                            entry["capacity_failure_structure_ids"]
                                .as_array_mut()
                                .unwrap()
                                .push(json!(id));
                        }
                    }
                }
                if row.uplift_case_present
                    || row.minimum_signed_weight_span_m.is_some_and(|v| v < 0.0)
                {
                    index.add(QueryKind::NegativeLoading, Some(id.clone()), fact);
                }
                for (kind, messages) in [
                    ("structure_demand_or_capacity", &row.blockers),
                    ("unevaluated_structure_check", &row.unevaluated_checks),
                ] {
                    for message in messages {
                        index.blocker(Some(id.clone()), kind, None, message);
                    }
                }
            }
        }
        for evidence in &scene.evidence.section_clearance {
            let id = evidence.section_id.to_string();
            if evidence.status == ClearanceStatus::Fail {
                let entry = sections.entry(id.clone()).or_insert_with(|| {
                    json!({"kind":"violating_section","section_id":id,
                        "capacity_failure_structure_ids":[],"capacity_failure_structure_count":0})
                });
                entry["profile_clearance"] = json!(evidence);
            } else if evidence.status == ClearanceStatus::Indeterminate {
                index.add(
                    QueryKind::Blockers,
                    Some(id),
                    Arc::new(json!({
                    "kind":"profile_clearance_indeterminate","evidence":evidence})),
                );
            }
        }
        for marker in &scene.engineering.markers {
            if marker.kind
                == ds_grid_engine::profile_engineering::ProfileEngineeringMarkerKind::SectionState
                && marker.source_status == Some(EngineeringStatus::Fail)
            {
                if let Some(section) = &marker.section_id {
                    let id = section.to_string();
                    let entry = sections.entry(id.clone()).or_insert_with(||
                        json!({"kind":"violating_section","section_id":id,
                            "capacity_failure_structure_ids":[],"capacity_failure_structure_count":0}));
                    entry["section_state"] = json!({"source_status":marker.source_status,
                        "classification":marker.source_classification,"usage_percent":marker.usage_percent});
                }
            }
        }
        for (id, mut value) in sections {
            value["capacity_failure_structures_truncated"] =
                json!(value["capacity_failure_structure_count"].as_u64().unwrap() > 50);
            index.add(QueryKind::ViolatingSections, Some(id), Arc::new(value));
        }
        for unavailable in &scene.evidence.curve_unavailable {
            index.blocker(
                Some(unavailable.section_id.to_string()),
                "profile_curve",
                Some(json!(unavailable.code)),
                &unavailable.message,
            );
        }
        for band in &scene.bands {
            if let Some(unavailable) = &band.evidence.projection_unavailable {
                index.blocker(
                    Some(band.alignment_id.to_string()),
                    "profile_projection",
                    Some(json!(unavailable.code)),
                    &unavailable.message,
                );
            }
        }
        for message in &scene.engineering.limitations {
            index.blocker(None, "profile_review", None, message);
        }
        if !usage_available {
            index.blocker(None, "usage_calculation_unavailable", Some(json!("no_complete_matching_cached_report")),
                "The native calculation retained no complete usage report for this envelope. Inspect blockers; repair inputs and explicitly calculate.");
        }
        index
    }
    fn add(&mut self, kind: QueryKind, id: Option<String>, value: Arc<Value>) {
        self.facts.get_mut(&kind).unwrap().insert(id, value);
    }
    fn blocker(&mut self, id: Option<String>, kind: &str, code: Option<Value>, message: &str) {
        let mut value = bounded_text(message);
        value["kind"] = json!(kind);
        value["code"] = json!(code);
        self.add(QueryKind::Blockers, id, Arc::new(value));
    }
}

impl Runtime {
    pub(super) fn query_results(&self, mut request: QueryResultsRequest) -> Result<Value, String> {
        let started = std::time::Instant::now();
        if request.scope.is_empty()
            || request.scope.len() > 1024
            || request.model_id.is_empty()
            || request.model_id.len() > 256
            || request.limit == 0
            || request.limit > MAX_ITEMS
            || request.expected_engineering_input_root.len() > 256
            || request.expected_history.history_pin.len() > 256
            || request.entity_ids.len() > 128
            || request
                .entity_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 256)
        {
            return Err("invalid_profile_query: query identity or bounds are invalid".into());
        }
        request.entity_ids.sort();
        request.entity_ids.dedup();
        if request.kind == QueryKind::Findings && request.source == QuerySource::Profile {
            return Err("invalid_profile_query: design-point findings require source model_analysis; explicitly Analyze that source before reading it".into());
        }
        let held = self.held.get(&request.scope).ok_or(
            "profile_results_unavailable: initialize and explicitly calculate this scope first",
        )?;
        if held.source.model_id != request.model_id {
            return Err("profile_replay_model_mismatch: query belongs to another model".into());
        }
        if self
            .publications
            .get(&request.scope)
            .is_some_and(|p| !p.confirmed)
        {
            return Err("profile_publication_pending: confirm or discard the calculated candidate before querying".into());
        }
        let revision = held.session.current_revision();
        let history = held.session.history_state();
        if request.expected_history != history
            || request.expected_engineering_input_root != revision.roots.engineering_input_root
        {
            return Err("profile_query_stale: query history or authored input root changed".into());
        }
        let index = match request.source {
            QuerySource::Profile => &held.result_index,
            QuerySource::ModelAnalysis => held.analysis_index.as_ref().ok_or(
                "profile_calculation_required: no model-bound analysis was admitted for these inputs; explicitly Analyze before reading this source",
            )?,
        };
        if index.source != request.source {
            return Err(
                "profile_results_unavailable: native result source differs from the query".into(),
            );
        }
        let state_case: ds_grid_engine::profile_display_case::ProfileDisplayCase =
            serde_json::from_value(json!(self.views[&request.scope].display_case))
                .expect("admitted case");
        if matches!(
            request.kind,
            QueryKind::ViolatingSections | QueryKind::Blockers
        ) && request.source == QuerySource::Profile
            && state_case != index.case
        {
            return Err("profile_calculation_required: no result index was admitted for this displayed clearance case".into());
        }
        let scope = &held.query_scope;
        let current_inputs = index.root == revision.roots.engineering_input_root;
        if !current_inputs && request.source == QuerySource::ModelAnalysis {
            return Err("profile_calculation_required: model-bound analysis inputs changed; explicitly Analyze before querying this source".into());
        }
        if request.source == QuerySource::ModelAnalysis
            && request.kind != QueryKind::Blockers
            && !index.usage_available
        {
            return Err("profile_calculation_required: model-bound Analyze retained no admitted envelope; inspect its blockers and repair inputs before explicit Analyze".into());
        }
        if !current_inputs {
            let affected = held.pending_reconcile
                || scope.whole_model
                || (request.entity_ids.is_empty()
                    && (!scope.structures.is_empty()
                        || !scope.demand_sections.is_empty()
                        || !scope.context_sections.is_empty()
                        || !scope.spans.is_empty()
                        || !scope.alignments.is_empty()))
                || request.entity_ids.iter().any(|id| {
                    ds_grid_model::StructureId::new(id.clone())
                        .is_ok_and(|v| scope.structures.contains(&v))
                        || ds_grid_model::TensionSectionId::new(id.clone()).is_ok_and(|v| {
                            scope.context_sections.contains(&v)
                                || scope.demand_sections.contains(&v)
                        })
                        || ds_grid_model::AlignmentId::new(id.clone())
                            .is_ok_and(|v| scope.alignments.contains(&v))
                });
            if affected || held.pending_receipts.is_empty() {
                return Err("profile_calculation_required: requested facts depend on pending authored edits; explicitly calculate before reading them".into());
            }
        }
        if matches!(
            request.kind,
            QueryKind::Usage
                | QueryKind::Failures
                | QueryKind::NegativeLoading
                | QueryKind::Qualification
        ) && !index.usage_available
        {
            return Err("profile_calculation_required: no complete matching cached usage envelope is available; inspect blockers and explicitly calculate".into());
        }
        if request.kind == QueryKind::Findings && !index.findings_available {
            return Err("profile_calculation_required: no complete model-bound design clearance findings were admitted; explicitly Analyze and inspect blockers".into());
        }
        let entity_family = match request.kind {
            QueryKind::Usage
            | QueryKind::Failures
            | QueryKind::NegativeLoading
            | QueryKind::Qualification => &index.structures,
            QueryKind::ViolatingSections => &index.sections,
            QueryKind::Blockers | QueryKind::Findings => &index.entities,
        };
        if request
            .entity_ids
            .iter()
            .any(|id| !entity_family.contains(id))
        {
            return Err(
                "invalid_profile_query: an entity is absent from this query's native family".into(),
            );
        }
        let offset = if let Some(cursor) = &request.cursor {
            if cursor.result_id != index.id
                || cursor.scope != request.scope
                || cursor.model_id != request.model_id
                || cursor.history != history
                || cursor.kind != request.kind
                || cursor.source != request.source
                || cursor.entity_ids != request.entity_ids
            {
                return Err("profile_query_stale: page cursor belongs to another head, selection or result index".into());
            }
            cursor.offset
        } else {
            0
        };
        let facts = &index.facts[&request.kind];
        let selected =
            (!request.entity_ids.is_empty()).then(|| facts.positions(&request.entity_ids));
        let total = selected.as_ref().map_or(facts.rows.len(), Vec::len);
        if offset > total {
            return Err(
                "invalid_profile_query: page offset exceeds the native result count".into(),
            );
        }
        let mut response = json!({"schema_version":1,"scope":request.scope,"model_id":request.model_id,
            "history":history,"model_revision":revision.revision_id,
            "engineering_input_root":revision.roots.engineering_input_root,
            "computed_revision":index.revision,"computed_engineering_input_root":index.root,
            "freshness":if current_inputs {"current_inputs"} else {"unchanged_dependencies"},
            "result_id":index.id,"source":index.source,
            "display_case":if index.source == QuerySource::Profile {json!(index.case)} else {Value::Null},
            "basis":index.basis,"kind":request.kind,
            "total":total,"offset":offset,"items":[],"next_cursor":null,"truncated":false});
        // Reserve room for the maximum cursor and metrics; even an unusually
        // large native envelope must refuse instead of transferring unbounded data.
        let next = |offset| QueryCursor {
            result_id: index.id.clone(),
            scope: request.scope.clone(),
            model_id: request.model_id.clone(),
            history: history.clone(),
            kind: request.kind,
            source: request.source,
            entity_ids: request.entity_ids.clone(),
            offset,
        };
        let mut bytes = serde_json::to_vec(&response).unwrap().len()
            + serde_json::to_vec(&next(total)).unwrap().len()
            + 1024;
        if bytes > MAX_BYTES {
            return Err(
                "profile_query_output_limit: native query basis exceeds the bounded response"
                    .into(),
            );
        }
        let mut items = Vec::new();
        for position in offset..total.min(offset + request.limit) {
            let fact = &facts.rows[selected
                .as_ref()
                .map_or(position, |selected| selected[position])];
            let item = json!({"entity_id":fact.entity_id,"fact":*fact.value});
            let item_bytes = serde_json::to_vec(&item).unwrap().len() + 1;
            if bytes + item_bytes > MAX_BYTES {
                if items.is_empty() {
                    return Err(
                        "profile_query_output_limit: one native fact exceeds the bounded response"
                            .into(),
                    );
                }
                break;
            }
            bytes += item_bytes;
            items.push(item);
        }
        let end = offset + items.len();
        response["items"] = json!(items);
        response["truncated"] = json!(end < total);
        if end < total {
            response["next_cursor"] = json!(next(end));
        }
        response["metrics"] = json!({"query_ms":started.elapsed().as_secs_f64()*1000.0,
            "returned_items":end-offset,"calculated_cases":0});
        if serde_json::to_vec(&response).unwrap().len() > MAX_BYTES {
            return Err(
                "profile_query_output_limit: native query response exceeds its byte limit".into(),
            );
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactional_projection_tests::{describe, refresh, request_for};
    use ds_grid_model::*;

    pub(super) fn fixture() -> GridModelSnapshot {
        let mut snapshot = common::fixture();
        snapshot.structures[1].z_m = 0.0; // A genuine valley support, with signed demand.
        for (i, (x_m, y_m, z_m)) in [(0.0, 0.0, 100.0), (300.0, 0.0, 0.0), (600.0, 40.0, 98.0)]
            .into_iter()
            .enumerate()
        {
            snapshot.terrain_points.push(TerrainPointRow {
                id: TerrainPointId::new(format!("query-ground-{i}")).unwrap(),
                x_m,
                y_m,
                z_m,
                feature_class: "ground".into(),
                description: None,
                required_clearance_m: None,
                source_id: None,
            });
        }
        let mut detached = snapshot.structures[0].clone();
        detached.id = StructureId::new("detached").unwrap();
        detached.station_m = Some(50.0);
        detached.x_m = 50.0;
        snapshot.structures.push(detached);
        snapshot.tension_sections.push(TensionSectionRow {
            id: TensionSectionId::new("query-section").unwrap(),
            cable_id: snapshot.cables[0].id.clone(),
            criterion_set_id: Some(snapshot.criterion_sets[0].id.clone()),
            sag: AuthoredSag::HorizontalTension {
                tension_n: 20_000.0,
            },
            sag_condition: Some(CableCondition::Initial),
            sag_weather_state_id: Some(snapshot.weather_states[0].id.clone()),
            ruling_span_m: None,
            details: Default::default(),
        });
        for (sequence, id) in ["str-1", "str-2", "str-3"].into_iter().enumerate() {
            snapshot
                .tension_section_supports
                .push(TensionSectionSupportRow {
                    section_id: TensionSectionId::new("query-section").unwrap(),
                    sequence: sequence as u32,
                    structure_id: StructureId::new(id).unwrap(),
                    attachment_point_id: Some(snapshot.attachment_points[0].id.clone()),
                    insulator_model: None,
                });
        }
        snapshot.capacity_artifacts[0].realization_digest =
            extract_element(&snapshot, snapshot.structure_types[0].id.entity())
                .unwrap()
                .snapshot_fingerprint();
        snapshot
    }
    pub(super) fn request(runtime: &Runtime, scope: &str, kind: QueryKind) -> QueryResultsRequest {
        let held = &runtime.held[scope];
        QueryResultsRequest {
            scope: scope.into(),
            model_id: held.source.model_id.clone(),
            expected_history: held.session.history_state(),
            expected_engineering_input_root: held
                .session
                .current_revision()
                .roots
                .engineering_input_root
                .clone(),
            kind,
            entity_ids: vec![],
            source: QuerySource::Profile,
            limit: 50,
            cursor: None,
        }
    }
    pub(super) fn execute(
        runtime: &mut Runtime,
        request: QueryResultsRequest,
    ) -> Result<Value, String> {
        let mut wire = json!(request);
        wire["operation"] = json!("query_results");
        runtime.execute(serde_json::from_value::<Request>(wire).unwrap())
    }

    #[test]
    fn all_query_kinds_match_fresh_typed_facts_without_solver_or_scene_mutation() {
        let input = request_for(fixture());
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let scene = runtime.held[&input.scope]
            .review
            .base_scene()
            .unwrap()
            .clone();
        let selected = scene.engineering.usage_request.clone().unwrap();
        let cache = &runtime.held[&input.scope].demand_cache;
        assert!(cache
            .cached_report(&scene.model_revision, &selected)
            .is_some());
        let mut wrong_envelope = selected.clone();
        wrong_envelope.analysis_case_ids.pop();
        assert!(cache
            .cached_report(&scene.model_revision, &wrong_envelope)
            .is_none());
        let fresh =
            ds_grid_engine::structure_usage::structure_usage_screening(&fixture(), &selected)
                .unwrap();
        let work = runtime.held[&input.scope].demand_cache.last_demand_work();
        let review = runtime.held[&input.scope].review.clone();
        let history = runtime.held[&input.scope].session.history_state();
        for kind in [
            QueryKind::Usage,
            QueryKind::Failures,
            QueryKind::NegativeLoading,
            QueryKind::Qualification,
            QueryKind::ViolatingSections,
            QueryKind::Blockers,
        ] {
            let query = request(&runtime, &input.scope, kind);
            let reply = execute(&mut runtime, query).unwrap();
            assert_eq!(reply["freshness"], "current_inputs");
            assert_eq!(reply["metrics"]["calculated_cases"], 0);
            assert!(serde_json::to_vec(&reply).unwrap().len() <= MAX_BYTES);
            if matches!(
                kind,
                QueryKind::Usage
                    | QueryKind::Failures
                    | QueryKind::NegativeLoading
                    | QueryKind::Qualification
            ) {
                let expected: Vec<_> = fresh
                    .rows
                    .iter()
                    .filter(|row| match kind {
                        QueryKind::Failures => {
                            row.engineering_status == Some(EngineeringStatus::Fail)
                        }
                        QueryKind::NegativeLoading => {
                            row.uplift_case_present
                                || row.minimum_signed_weight_span_m.is_some_and(|v| v < 0.0)
                        }
                        _ => true,
                    })
                    .collect();
                assert_eq!(reply["total"], expected.len());
                for (item, row) in reply["items"].as_array().unwrap().iter().zip(expected) {
                    assert_eq!(item["entity_id"], row.structure_id.as_str());
                    assert_eq!(&item["fact"], usage_fact(row).as_ref());
                }
            }
        }
        let negative = fresh
            .rows
            .iter()
            .filter(|row| row.uplift_case_present)
            .count();
        assert!(
            negative > 0,
            "valley fixture must exercise actual native negative loading: {:?}",
            fresh.rows
        );
        assert!(
            fresh
                .rows
                .iter()
                .any(|row| row.engineering_status == Some(EngineeringStatus::Fail)),
            "applicable fixture capacity must exercise proven failure"
        );
        assert_eq!(
            runtime.held[&input.scope].demand_cache.last_demand_work(),
            work
        );
        assert!(Arc::ptr_eq(&runtime.held[&input.scope].review, &review));
        assert_eq!(runtime.held[&input.scope].session.history_state(), history);
    }

    #[test]
    fn paging_pins_result_head_kind_selection_and_bounds() {
        let input = request_for(fixture());
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let mut query = request(&runtime, &input.scope, QueryKind::Usage);
        query.limit = 1;
        let first = execute(&mut runtime, query.clone()).unwrap();
        assert_eq!(first["total"], 4);
        assert_eq!(first["items"].as_array().unwrap().len(), 1);
        query.cursor = serde_json::from_value(first["next_cursor"].clone()).unwrap();
        let second = execute(&mut runtime, query.clone()).unwrap();
        assert_eq!(second["offset"], 1);
        assert_ne!(first["items"], second["items"]);
        let mut bad = query.clone();
        bad.kind = QueryKind::Failures;
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        let mut bad = query.clone();
        bad.entity_ids = vec!["str-1".into()];
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        let mut bad = query.clone();
        bad.cursor.as_mut().unwrap().offset = 5;
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let mut bad = query.clone();
        bad.limit = MAX_ITEMS + 1;
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let mut bad = query.clone();
        bad.model_id = "foreign".into();
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_replay_model_mismatch:"));
        let mut bad = query.clone();
        bad.expected_engineering_input_root = "foreign".into();
        assert!(execute(&mut runtime, bad)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        runtime.execute(input).unwrap(); // A fresh calculation at equal authored content.
        assert!(execute(&mut runtime, query)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        let mut unknown = request(&runtime, "one", QueryKind::Usage);
        unknown.entity_ids = vec!["absent".into()];
        assert!(execute(&mut runtime, unknown)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let mut wrong_family = request(&runtime, "one", QueryKind::Failures);
        wrong_family.entity_ids = vec!["query-section".into()];
        assert!(execute(&mut runtime, wrong_family)
            .unwrap_err()
            .starts_with("invalid_profile_query:"));
        let captured = request(&runtime, "one", QueryKind::Usage);
        runtime
            .execute(Request::Release(ReleaseRequest {
                scope: "one".into(),
                model_id: "profile-test".into(),
            }))
            .unwrap();
        assert!(execute(&mut runtime, captured)
            .unwrap_err()
            .starts_with("profile_results_unavailable:"));
    }

    #[test]
    fn edits_invalidate_every_incident_row_but_preserve_unaffected_reads_and_rollback() {
        let snapshot = fixture();
        let input = request_for(snapshot.clone());
        let mut worker = GridSession::open(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let mut untouched = request(&runtime, &input.scope, QueryKind::Usage);
        untouched.entity_ids = vec!["detached".into()];
        let old = execute(&mut runtime, untouched).unwrap();
        let captured = request(&runtime, &input.scope, QueryKind::Usage);
        describe(&mut worker, "query-edit", "pending edit");
        let replay = refresh(&runtime, &input, &input.expected_revision, &worker);
        runtime.execute(Request::Observe(Box::new(replay))).unwrap();
        assert!(execute(&mut runtime, captured)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
        for id in ["str-1", "str-2", "str-3", "query-section"] {
            let mut query = request(&runtime, &input.scope, QueryKind::Blockers);
            query.entity_ids = vec![id.into()];
            assert!(
                execute(&mut runtime, query)
                    .unwrap_err()
                    .starts_with("profile_calculation_required:"),
                "{id}"
            );
        }
        let mut unaffected = request(&runtime, &input.scope, QueryKind::Usage);
        unaffected.entity_ids = vec!["detached".into()];
        let reused = execute(&mut runtime, unaffected).unwrap();
        assert_eq!(old["items"], reused["items"]);
        assert_eq!(reused["freshness"], "unchanged_dependencies");
        assert_eq!(old["computed_revision"], reused["computed_revision"]);
        assert_ne!(reused["model_revision"], reused["computed_revision"]);
        let mut calculate = refresh(
            &runtime,
            &input,
            &worker.current_revision().revision_id.to_string(),
            &worker,
        );
        calculate.publication_id = Some("query-publication".into());
        let calculated = runtime.execute(calculate).unwrap();
        let candidate = request(&runtime, &input.scope, QueryKind::Usage);
        assert!(execute(&mut runtime, candidate.clone())
            .unwrap_err()
            .starts_with("profile_publication_pending:"));
        runtime
            .execute(Request::DiscardPublication(DiscardPublicationRequest {
                scope: input.scope.clone(),
                model_id: "profile-test".into(),
                publication_id: "query-publication".into(),
                target_history: serde_json::from_value(calculated["history"].clone()).unwrap(),
            }))
            .unwrap();
        assert!(execute(&mut runtime, candidate)
            .unwrap_err()
            .starts_with("profile_calculation_required:"));
        let mut untouched = request(&runtime, &input.scope, QueryKind::Usage);
        untouched.entity_ids = vec!["detached".into()];
        assert_eq!(
            execute(&mut runtime, untouched).unwrap()["result_id"],
            old["result_id"]
        );
        runtime
            .execute(refresh(
                &runtime,
                &input,
                &worker.current_revision().revision_id.to_string(),
                &worker,
            ))
            .unwrap();
        let query = request(&runtime, &input.scope, QueryKind::Usage);
        assert_eq!(
            execute(&mut runtime, query).unwrap()["freshness"],
            "current_inputs"
        );
    }

    #[test]
    fn missing_envelope_blockers_are_readable_and_unknown_never_becomes_zero_failure() {
        let mut snapshot = common::fixture();
        snapshot.criterion_case_bindings.clear();
        let input = request_for(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        for kind in [
            QueryKind::Usage,
            QueryKind::Failures,
            QueryKind::NegativeLoading,
            QueryKind::Qualification,
        ] {
            let query = request(&runtime, &input.scope, kind);
            assert!(execute(&mut runtime, query)
                .unwrap_err()
                .starts_with("profile_calculation_required:"));
        }
        let query = request(&runtime, &input.scope, QueryKind::Blockers);
        let reply = execute(&mut runtime, query).unwrap();
        assert!(reply["total"].as_u64().unwrap() > 0);
        assert_eq!(reply["basis"]["usage_available"], false);
    }

    #[test]
    fn negative_loading_within_declared_signed_bounds_is_not_a_failure() {
        let mut snapshot = fixture();
        for knot in &mut snapshot.span_limit_angle_knots {
            knot.minimum_signed_weight_span_m = -1_000_000.0;
            knot.allowable_wind_span_m = 1_000_000.0;
        }
        for maximum in &mut snapshot.span_limit_weight_maxima {
            maximum.maximum_signed_weight_span_m = 1_000_000.0;
        }
        let input = request_for(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let query = request(&runtime, &input.scope, QueryKind::NegativeLoading);
        let negative = execute(&mut runtime, query).unwrap();
        assert!(negative["total"].as_u64().unwrap() > 0);
        for item in negative["items"].as_array().unwrap() {
            assert_eq!(item["fact"]["row"]["uplift_failure"], false);
        }
        let query = request(&runtime, &input.scope, QueryKind::Failures);
        assert_eq!(execute(&mut runtime, query).unwrap()["total"], 0);
    }

    #[test]
    fn complete_query_counts_survive_analysis_caps_and_deleted_entity_history_aba() {
        let mut snapshot = common::fixture();
        for i in 0..2003 {
            let mut row = snapshot.structures[0].clone();
            row.id = StructureId::new(format!("extra-{i}")).unwrap();
            row.station_m = Some(i as f64 / 10.0);
            snapshot.structures.push(row);
        }
        let mut input = request_for(snapshot.clone());
        input.analyze = true;
        let mut runtime = Runtime::default();
        let analyzed = runtime.execute(input.clone()).unwrap();
        assert_eq!(
            analyzed["analysis"]["report"]["report"]["structures"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            2000
        );
        let mut query = request(&runtime, &input.scope, QueryKind::Usage);
        query.limit = 2;
        let page = execute(&mut runtime, query.clone()).unwrap();
        assert_eq!(page["total"], 2006);
        assert_eq!(page["items"].as_array().unwrap().len(), 2);
        query.cursor = serde_json::from_value(page["next_cursor"].clone()).unwrap();
        let mut worker = GridSession::open(snapshot);
        worker
            .apply_command(ds_grid_engine::CommandEnvelope::new(
                "delete-query-row",
                worker.current_revision().revision_id.clone(),
                ds_grid_engine::GridCommand::DeleteStructure {
                    id: StructureId::new("extra-0").unwrap(),
                },
            ))
            .unwrap();
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        let mut deleted = request(&runtime, &input.scope, QueryKind::Usage);
        deleted.entity_ids = vec!["extra-0".into()];
        assert!(execute(&mut runtime, deleted)
            .unwrap_err()
            .starts_with("profile_calculation_required:"));
        worker.undo().unwrap();
        let base = runtime.held[&input.scope]
            .session
            .current_revision()
            .revision_id
            .to_string();
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime, &input, &base, &worker,
            ))))
            .unwrap();
        assert!(execute(&mut runtime, query)
            .unwrap_err()
            .starts_with("profile_query_stale:"));
    }

    #[test]
    #[ignore = "measured dense query gate; run alone with disk scratch"]
    fn dense_cached_query_costs() {
        let mut snapshot = fixture();
        for i in 0..10_000 {
            let mut row = snapshot.structures[3].clone();
            row.id = StructureId::new(format!("dense-query-{i}")).unwrap();
            row.station_m = Some(i as f64 / 100.0);
            snapshot.structures.push(row);
        }
        let input = request_for(snapshot);
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let index = runtime.held[&input.scope].result_index.clone();
        let work = runtime.held[&input.scope].demand_cache.last_demand_work();
        let mut measured = Vec::new();
        for kind in [
            QueryKind::Usage,
            QueryKind::Failures,
            QueryKind::NegativeLoading,
            QueryKind::Qualification,
            QueryKind::ViolatingSections,
            QueryKind::Blockers,
        ] {
            let mut times = Vec::new();
            let mut transferred = 0;
            let mut total = 0;
            for _ in 0..12 {
                let mut query = request(&runtime, &input.scope, kind);
                query.limit = 50;
                let started = std::time::Instant::now();
                let response = execute(&mut runtime, query).unwrap();
                let bytes = serde_json::to_vec(&response).unwrap();
                times.push(started.elapsed().as_secs_f64() * 1000.0);
                assert!(bytes.len() <= MAX_BYTES);
                transferred = bytes.len();
                total = response["total"].as_u64().unwrap();
            }
            times.sort_by(f64::total_cmp);
            measured.push(
                json!({"kind":kind,"median_ms":times[6],"minimum_ms":times[0],
                "maximum_ms":times[11],"returned_bytes":transferred,"total":total}),
            );
        }
        assert!(Arc::ptr_eq(
            &index,
            &runtime.held[&input.scope].result_index
        ));
        assert_eq!(
            runtime.held[&input.scope].demand_cache.last_demand_work(),
            work
        );
        println!(
            "PROFILE_QUERY_COST {}",
            json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},
            "structure_count":10_004,"page_limit":50,"queries":measured,
            "includes":"wire DTO decode, native query and JSON byte encoding",
            "excludes":"IPC, renderer, explicit-calculation index construction and installed artifact"})
        );
    }
}
