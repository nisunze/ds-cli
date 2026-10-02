//! Shared plan/profile sheet presentation; geometry remains engine-owned.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PanelOrder {
    #[default]
    ProfileTop,
    PlanTop,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StructureLabelOrientation {
    /// Station-aligned labels read upward within the profile panel.
    #[default]
    Vertical,
    /// Legacy horizontal structure ledger below the two drawing panels.
    Horizontal,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureLabelOrientation {
    #[default]
    Horizontal,
    Vertical,
    FollowGround,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureLabelPlacement {
    #[default]
    Above,
    Below,
    Staggered,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct FeatureLabelStyle {
    /// Case-insensitive feature codes/classes to label. Empty means all
    /// meaningful features; generic ground points require explicit selection.
    pub codes: Vec<String>,
    pub orientation: FeatureLabelOrientation,
    pub font_size_pt: f64,
    pub placement: FeatureLabelPlacement,
}

impl Default for FeatureLabelStyle {
    fn default() -> Self {
        Self {
            codes: Vec::new(),
            orientation: FeatureLabelOrientation::Horizontal,
            font_size_pt: 5.0,
            placement: FeatureLabelPlacement::Above,
        }
    }
}

impl FeatureLabelStyle {
    pub fn validate(&self) -> Result<(), String> {
        if !self.font_size_pt.is_finite() || !(4.0..=12.0).contains(&self.font_size_pt) {
            return Err("feature_label_style.font_size_pt must be 4..12".into());
        }
        if self.codes.len() > 64
            || self.codes.iter().any(|code| {
                code.trim().is_empty() || code.len() > 48 || code.chars().any(|c| c.is_control())
            })
        {
            return Err("feature_label_style.codes requires at most 64 nonempty codes of at most 48 characters".into());
        }
        Ok(())
    }

    pub fn allows(&self, class: &str, name: &str, feature_code: Option<u32>) -> bool {
        if self.codes.is_empty() {
            let label = if name.trim().is_empty() { class } else { name };
            return !matches!(label.trim().to_ascii_lowercase().as_str(), "ground" | "gp");
        }
        self.codes.iter().any(|code| {
            let code = code.trim();
            code.eq_ignore_ascii_case(class)
                || code.eq_ignore_ascii_case(name)
                || feature_code.is_some_and(|number| code.parse::<u32>().ok() == Some(number))
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanAnglePolicy {
    /// Keep the physical bend when its rotated footprint fits the panel.
    #[default]
    PreserveIfFit,
    /// Cut at authored angle points even when the bend fits on one panel.
    SplitAtAuthored,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BreakSupportContext {
    /// Each support is drawn at its physical station once.
    #[default]
    Once,
    /// Repeat the bounding support identities beside both elevation rulers.
    RepeatLabels,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SheetFormat {
    Simple,
    Advanced,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InkMode {
    #[default]
    Monochrome,
    ReferenceAccents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StructureLabelField {
    Station,
    EngineeringNumber,
    EngineeringName,
    StructureType,
    StructureDescription,
    DisplayLabel,
    Comment1,
    Comment2,
    Comment3,
    Height,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpecialClearanceHairAnchor {
    GroundOffsetGuide,
    SurveyFeature,
}

impl SheetFormat {
    pub fn horizontal_scale(self) -> u32 {
        match self {
            Self::Simple => 2000,
            Self::Advanced => 1000,
        }
    }

    pub fn vertical_scale(self) -> u32 {
        match self {
            Self::Simple => 500,
            Self::Advanced => 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SheetSettings {
    pub format: SheetFormat,
    #[serde(default)]
    pub ink_mode: InkMode,
    pub project_title: String,
    pub sheet_title: String,
    #[serde(default)]
    pub horizontal_scale: Option<u32>,
    #[serde(default)]
    pub vertical_scale: Option<u32>,
    #[serde(default)]
    pub plan_scale: Option<u32>,
    #[serde(default)]
    pub panel_order: PanelOrder,
    #[serde(default)]
    pub structure_label_orientation: StructureLabelOrientation,
    #[serde(default)]
    pub show_feature_codes: Option<bool>,
    #[serde(default)]
    pub feature_label_style: FeatureLabelStyle,
    #[serde(default)]
    pub show_clearance_thresholds: Option<bool>,
    /// Drawing basis only; absolute canonical thresholds and margins stay exact.
    /// Absence retains the ground-offset-guide presentation of existing setups.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub special_clearance_hair_anchor: Option<SpecialClearanceHairAnchor>,
    /// Up to three ordered ledger lines, each composed from canonical native
    /// structure facts. An empty value uses the format's project default.
    #[serde(default)]
    pub structure_label_rows: Vec<Vec<StructureLabelField>>,
    #[serde(default = "default_true")]
    pub long_axis_plot: bool,
    #[serde(default)]
    pub plan_angle_policy: PlanAnglePolicy,
    #[serde(default = "default_angle_gap")]
    pub angle_gap_mm: f64,
    #[serde(default)]
    pub minimum_angle_deg: f64,
    #[serde(default = "default_buffer_m")]
    pub plan_buffer_m: f64,
    #[serde(default = "default_true")]
    pub show_profile_grid: bool,
    #[serde(default)]
    pub show_attachment_points: Option<bool>,
    #[serde(default)]
    pub show_span_labels: Option<bool>,
    #[serde(default = "default_true")]
    pub show_obstacle_sticks: bool,
    #[serde(default = "default_true")]
    pub show_profile_continuations: bool,
    #[serde(default = "default_true")]
    pub profile_elevation_breaks: bool,
    #[serde(default)]
    pub break_support_context: BreakSupportContext,
    #[serde(default = "default_drawing_revision")]
    pub drawing_revision: String,
    #[serde(default)]
    pub drawing_date: String,
    #[serde(default = "default_country")]
    pub title_country: String,
    #[serde(default = "default_employer")]
    pub title_employer: String,
    #[serde(default)]
    pub title_consultant: String,
    #[serde(default)]
    pub title_contractor: String,
    #[serde(default)]
    pub title_programme: String,
    #[serde(default)]
    pub title_subject: String,
}

fn default_true() -> bool {
    true
}
fn default_angle_gap() -> f64 {
    7.0
}
fn default_buffer_m() -> f64 {
    6.0
}
fn default_drawing_revision() -> String {
    "v0".into()
}
fn default_country() -> String {
    "Republic of Rwanda".into()
}
fn default_employer() -> String {
    "EDCL".into()
}

impl SheetSettings {
    pub fn new(format: SheetFormat, project_title: impl Into<String>) -> Self {
        Self {
            format,
            ink_mode: InkMode::default(),
            project_title: project_title.into(),
            sheet_title: "MV plan & profile".into(),
            horizontal_scale: None,
            vertical_scale: None,
            plan_scale: None,
            panel_order: PanelOrder::default(),
            structure_label_orientation: StructureLabelOrientation::default(),
            show_feature_codes: None,
            feature_label_style: FeatureLabelStyle::default(),
            special_clearance_hair_anchor: None,
            show_clearance_thresholds: None,
            structure_label_rows: Vec::new(),
            long_axis_plot: true,
            plan_angle_policy: PlanAnglePolicy::default(),
            angle_gap_mm: default_angle_gap(),
            minimum_angle_deg: 0.0,
            plan_buffer_m: default_buffer_m(),
            show_profile_grid: true,
            show_attachment_points: None,
            show_span_labels: None,
            show_obstacle_sticks: false,
            show_profile_continuations: true,
            profile_elevation_breaks: true,
            break_support_context: BreakSupportContext::default(),
            drawing_revision: default_drawing_revision(),
            drawing_date: String::new(),
            title_country: default_country(),
            title_employer: default_employer(),
            title_consultant: String::new(),
            title_contractor: String::new(),
            title_programme: String::new(),
            title_subject: String::new(),
        }
    }

    pub fn scales(&self) -> Result<(u32, u32), String> {
        if self.format == SheetFormat::Simple
            && (self.horizontal_scale.is_some()
                || self.vertical_scale.is_some()
                || self.plan_scale.is_some())
        {
            return Err("scale overrides require advanced format".into());
        }
        let horizontal = self
            .horizontal_scale
            .unwrap_or(self.format.horizontal_scale());
        let vertical = self.vertical_scale.unwrap_or(self.format.vertical_scale());
        if !(500..=10000).contains(&horizontal) || !(100..=5000).contains(&vertical) {
            return Err("horizontal scale must be 500..10000 and vertical scale 100..5000".into());
        }
        Ok((horizontal, vertical))
    }

    pub fn label_rows(&self) -> Result<Vec<Vec<StructureLabelField>>, String> {
        let rows = if self.structure_label_rows.is_empty() {
            match self.format {
                SheetFormat::Simple => vec![
                    vec![StructureLabelField::EngineeringNumber],
                    vec![StructureLabelField::StructureType],
                ],
                SheetFormat::Advanced => vec![
                    vec![StructureLabelField::EngineeringNumber],
                    vec![StructureLabelField::StructureType],
                    vec![
                        StructureLabelField::Comment1,
                        StructureLabelField::Comment2,
                        StructureLabelField::Comment3,
                    ],
                ],
            }
        } else {
            self.structure_label_rows.clone()
        };
        if rows.len() > 3 || rows.iter().any(|row| row.is_empty() || row.len() > 4) {
            return Err("structure_label_rows requires 1..3 lines of 1..4 fields".into());
        }
        Ok(rows)
    }

    pub fn plan_options(&self) -> Result<(), String> {
        if self
            .plan_scale
            .is_some_and(|scale| !(500..=10000).contains(&scale))
            || !self.angle_gap_mm.is_finite()
            || !(2.0..=30.0).contains(&self.angle_gap_mm)
            || !self.minimum_angle_deg.is_finite()
            || !(0.0..=90.0).contains(&self.minimum_angle_deg)
            || !self.plan_buffer_m.is_finite()
            || !(0.0..=50.0).contains(&self.plan_buffer_m)
        {
            return Err(
                "plan_scale must be 500..10000, angle_gap_mm 2..30, minimum_angle_deg 0..90, and plan_buffer_m 0..50"
                    .into(),
            );
        }
        Ok(())
    }
}
