//! The panel's owned presentation contract and the typed payloads it emits.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct PanelModel {
    pub structure_numbers: BTreeMap<String, String>,
    pub enabled: bool,
    pub title: String,
    pub model_name: String,
    pub popout_mode: bool,
    pub can_popout: bool,
    #[serde(alias = "genericMode")]
    pub generic: bool,
    pub display_feedback: DisplayFeedback,
    pub scene_present: bool,
    pub unavailable_curve_section_count: usize,
    pub calculation_required: bool,
    pub reconciling: bool,
    pub provisional: bool,
    pub moving_structure: bool,
    pub committed_move_awaiting_scene: bool,
    pub generic_ui_model: Option<GenericUi>,
    pub model_revision: Option<String>,
    pub coordinate_system: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub height_px: f64,
    pub show_fit: bool,
    pub show_zoom: bool,
    pub can_edit_model: bool,
    pub can_save_project_version: bool,
    pub can_create: bool,
    pub can_lasso: bool,
    pub can_filter: bool,
    pub edit_context_active: bool,
    pub undo_available: bool,
    pub redo_available: bool,
    pub desktop: bool,
    pub native_report: bool,
    pub dev: bool,
    pub read_only: bool,
    pub lasso_active: bool,
    pub lasso_busy: bool,
    pub show_readiness: bool,
    pub preparation: Preparation,
    pub readiness: Option<serde_json::Value>,
    pub reconciliation: Option<Reconciliation>,
    pub create: Create,
    pub dialogs: Dialogs,
    pub translations: BTreeMap<String, String>,
}
impl PanelModel {
    pub fn label(&self, key: &str, fallback: &str) -> String {
        self.translations
            .get(key)
            .cloned()
            .unwrap_or_else(|| fallback.into())
    }
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Preparation {
    pub phase: String,
    pub label: String,
    pub detail: String,
    pub busy: bool,
    pub tone: String,
    pub interactive: bool,
    pub can_admit: bool,
    pub can_discard: bool,
    pub can_reconcile: bool,
    pub can_retry: bool,
    pub can_rebuild: bool,
    pub initial_uninterruptible: bool,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Reconciliation {
    pub phase: String,
    pub tone: String,
    pub detail: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Create {
    pub mode: Option<String>,
    pub options: Option<CreateOptions>,
    pub choice: String,
    pub busy: bool,
    pub first_support_id: Option<String>,
    pub pending: bool,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CreateOptions {
    pub structure_types: Vec<Choice>,
    pub ground_features: Vec<Choice>,
    pub section_templates: Vec<Choice>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Choice {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Dialogs {
    pub retype: Option<Retype>,
    pub sag: Option<Sag>,
    pub selection: Option<Selection>,
    pub delete: Option<Delete>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Retype {
    pub model_id: String,
    pub expected_revision: String,
    pub structure_ids: Vec<String>,
    pub current: bool,
    pub loading: bool,
    pub busy: bool,
    pub error: String,
    pub definitions: Vec<Definition>,
    pub preview: Option<RetypePreview>,
    pub applied: Option<serde_json::Value>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Sag {
    pub model_id: String,
    pub expected_revision: String,
    pub section_ids: Vec<String>,
    pub loading: bool,
    pub busy: bool,
    pub error: String,
    pub choices: Option<SagChoices>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Selection {
    pub model_id: String,
    pub expected_revision: String,
    pub current: bool,
    pub loading: bool,
    pub busy: bool,
    pub error: String,
    pub result: Option<FilterResult>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Delete {
    pub model_id: String,
    pub revision: String,
    pub structure_ids: Vec<String>,
    pub busy: bool,
    pub error: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Close,
    Popout,
    Filter,
    EditToggle,
    Undo,
    Redo,
    Rebuild,
    Reconcile,
    JobStatus,
    JobCancel,
    JobAdmit,
    Tables,
    Staking,
    Data,
    Library,
    FeatureCodes,
    SpottingProgress,
    Results,
    Validate,
    SpanCheck,
    CalculateAll,
    AutoStringing,
    SelectedAutoStringing,
    Process,
    Issues,
    SelectedIssues,
    Properties,
    Retype,
    Delete,
    SectionCriteria,
    ParentStructure,
    AddAlignment,
    CreateCancel,
    CreatePlace,
    PreviewType,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DialogKind {
    Retype,
    Sag,
    Selection,
    Delete,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum DisplayCase {
    GreatestSag,
    SagReference,
    AnalysisCase { analysis_case_id: String },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Style {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Terrain {
    pub corridor_half_width_m: f64,
    pub gap_tolerance_m: f64,
    pub ground_clearance_offset_m: f64,
    pub lowest_wire_drop_m: f64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DisplayPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub styles: Option<BTreeMap<String, Option<Style>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<BTreeMap<String, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_case: Option<DisplayCase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_exaggeration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terrain: Option<Terrain>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cable_colors: Option<BTreeMap<String, Option<String>>>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ControlGroup {
    pub id: String,
    pub label_key: String,
    pub initially_open: bool,
    pub rows: Vec<super::model::DisplayRow>,
    pub numbers: Vec<NumericControl>,
    pub cables: Vec<super::model::Cable>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct NumericControl {
    pub label_key: String,
    pub value: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub exclusive_minimum: bool,
    pub request: serde_json::Value,
    pub value_path: Vec<String>,
}
impl NumericControl {
    pub fn patch(&self, value: f64) -> Result<DisplayPatch, String> {
        if !value.is_finite()
            || value < self.minimum
            || value > self.maximum
            || self.exclusive_minimum && value == self.minimum
        {
            return Err("Value is outside the control's allowed range".into());
        }
        let mut request = self.request.clone();
        let mut cursor = &mut request;
        for key in &self.value_path {
            cursor = cursor
                .get_mut(key)
                .ok_or_else(|| "Invalid native numeric control".to_string())?;
        }
        *cursor = serde_json::Value::from(value);
        serde_json::from_value(request).map_err(|e| e.to_string())
    }
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct StyleControls {
    pub targets: Vec<StyleTarget>,
    pub request: serde_json::Value,
    pub schema: serde_json::Value,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct StyleTarget {
    pub target: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Legend {
    pub line: bool,
    pub kind: String,
    pub label: String,
    pub glyph: String,
    pub encoding: super::model::Encoding,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct WeatherChoice {
    pub value: DisplayCase,
    pub label: String,
    #[serde(default)]
    pub disabled: bool,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RetypeRequest {
    pub expected_revision: String,
    pub structure_ids: Vec<String>,
    pub structure_type_id: String,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct HeadCommand {
    pub command_id: String,
    pub command: ModelCommand,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "command_kind", rename_all = "snake_case")]
pub enum ModelCommand {
    DeleteProfileEntities {
        ids: Vec<String>,
    },
    SetTensionSectionsSagMode {
        ids: Vec<String>,
        sag: SagMode,
        sag_condition: Option<String>,
        sag_weather_state_id: Option<String>,
        criterion_set_id: String,
        ruling_span_m: Option<f64>,
    },
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SagMode {
    FromCriteria,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FilterQuery {
    pub filters: Vec<Filter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats_columns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_ids: Option<Vec<String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value2: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Contains,
    Equals,
    NotEquals,
    StartsWith,
    EndsWith,
    Gt,
    Gte,
    Lt,
    Lte,
    Between,
    IsEmpty,
    NotEmpty,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct DisplayFeedback {
    pub sequence: u64,
    pub busy: bool,
    pub error: Option<FeedbackError>,
    pub patch: Option<DisplayPatch>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct FeedbackError {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct GenericUi {
    pub domain: String,
    pub capabilities: GenericCapabilities,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct GenericCapabilities {
    pub add_alignments: bool,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Definition {
    pub entity_id: String,
    pub structure_type_id: Option<String>,
    pub label: String,
    pub table: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RetypePreview {
    pub r#type: RetypeType,
    pub revision: String,
    pub refusal_on_write: Option<String>,
    pub findings: RetypeFindings,
    pub structures: Vec<RetypeStructure>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RetypeFindings {
    pub cleared: Vec<Finding>,
    pub remaining: Vec<Finding>,
    pub created: Vec<Finding>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Finding {
    pub finding: String,
    pub rule_id: String,
    pub source_clause: String,
    pub structure_id: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RetypeStructure {
    pub structure_id: String,
    pub number: String,
    pub type_before: String,
    pub type_after: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SagChoices {
    pub selected_section_ids: Vec<String>,
    pub physical_section_ids: Vec<String>,
    pub cable_ids: Vec<String>,
    pub options: Vec<SagChoice>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SagChoice {
    pub criterion_set_id: String,
    pub criterion_set_label: String,
    pub condition: Option<String>,
    pub weather_state_id: Option<String>,
    pub weather_state_label: String,
    pub availability: String,
    pub assigned_section_ids: Vec<String>,
    pub failures: Vec<SagFailure>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SagFailure {
    pub message: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct FilterResult {
    pub revision: String,
    pub table_kind: String,
    pub total_rows: usize,
    pub matched_count: usize,
    pub entity_ids: Vec<String>,
    pub columns: Vec<FilterColumn>,
    pub value_stats: Vec<FilterStats>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct FilterColumn {
    pub name: String,
    pub data_type: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct FilterStats {
    pub column: String,
    pub values: Vec<String>,
    pub numeric_min: Option<f64>,
    pub numeric_max: Option<f64>,
    pub capped: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RetypeType {
    pub id: String,
    pub name: String,
}
