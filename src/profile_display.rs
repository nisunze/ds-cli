//! One native Profile display state. Hosts submit patches and paint its projection.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayOption {
    Review,
    Grid,
    Ground,
    SideProfiles,
    ClearanceLine,
    TerrainPoints,
    TerrainOrdinates,
    Structures,
    Wire,
    SectionLabels,
    SpanDistances,
    StructureLabels,
    StructureNumbers,
    StructureNames,
    StructureComments,
    Embedded,
    OffsetEmbedmentHeight,
    RouteDeviation,
    /// The governing usage label under each structure.
    StructureUsage,
    /// The attachment boxes at each structure top (native section end states).
    SectionState,
    /// The section usage bitmap: one box per wire of each attachment set.
    SectionUsage,
    /// The analysis report. On, each held model revision is analyzed once without a
    /// click (see `analysis_state`); off, Analyze stays an explicit action.
    Analysis,
}
impl DisplayOption {
    pub const ALL: [Self; 22] = [
        Self::Review,
        Self::Grid,
        Self::Ground,
        Self::SideProfiles,
        Self::ClearanceLine,
        Self::TerrainPoints,
        Self::TerrainOrdinates,
        Self::Structures,
        Self::Wire,
        Self::SectionLabels,
        Self::SpanDistances,
        Self::StructureLabels,
        Self::StructureNumbers,
        Self::StructureNames,
        Self::StructureComments,
        Self::Embedded,
        Self::OffsetEmbedmentHeight,
        Self::RouteDeviation,
        Self::StructureUsage,
        Self::SectionState,
        Self::SectionUsage,
        Self::Analysis,
    ];
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Review => "gprof_display_review",
            Self::Grid => "gprof_display_grid",
            Self::Ground => "gprof_display_ground",
            Self::SideProfiles => "gprof_terrain_side_profiles",
            Self::ClearanceLine => "gprof_display_clearance",
            Self::TerrainPoints => "gprof_display_ground_points",
            Self::TerrainOrdinates => "gprof_terrain_ordinates",
            Self::Structures => "gprof_display_structures",
            Self::Wire => "gprof_display_conductors",
            Self::SectionLabels => "gprof_display_section_labels",
            Self::SpanDistances => "gprof_display_span_lengths",
            Self::StructureLabels => "gprof_display_structure_labels",
            Self::StructureNumbers => "gprof_label_number",
            Self::StructureNames => "gprof_label_type",
            Self::StructureComments => "gprof_display_comments",
            Self::Embedded => "gprof_display_embedded",
            Self::RouteDeviation => "gprof_display_route_deviation",
            Self::OffsetEmbedmentHeight => "gprof_display_offset_embedment_height",
            Self::StructureUsage => "gprof_display_structure_usage",
            Self::SectionState => "gprof_display_attachment_states",
            Self::SectionUsage => "gprof_display_section_usage",
            Self::Analysis => "gprof_display_analysis",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum DisplayCase {
    GreatestSag,
    SagReference,
    AnalysisCase { analysis_case_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayState {
    pub visibility: BTreeMap<DisplayOption, bool>,
    pub display_case: DisplayCase,
    pub vertical_exaggeration: f64,
    pub terrain: TerrainSettings,
    /// Configured wire colours by exact cable engineering name (`#rrggbb`). Wires are
    /// categorical by cable; an unconfigured cable keeps its default pen.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cable_colors: BTreeMap<String, String>,
}
impl Default for DisplayState {
    fn default() -> Self {
        Self {
            visibility: DisplayOption::ALL
                .into_iter()
                .map(|option| {
                    (
                        option,
                        // Owner, 2026-10-05: structure names and comments are
                        // visible by default alongside numbers and attachment
                        // boxes. Keep operational warning prose in inspection
                        // popups; never hide identifying labels to reduce it.
                        !matches!(
                            option,
                            DisplayOption::SideProfiles
                                | DisplayOption::TerrainOrdinates
                                | DisplayOption::SectionLabels
                                | DisplayOption::Embedded
                                | DisplayOption::RouteDeviation
                        ),
                    )
                })
                .collect(),
            display_case: DisplayCase::GreatestSag,
            vertical_exaggeration: 5.0,
            terrain: TerrainSettings {
                corridor_half_width_m: 15.0,
                gap_tolerance_m: 100.0,
                ground_clearance_offset_m: 6.0,
                lowest_wire_drop_m: 0.0,
            },
            cable_colors: BTreeMap::new(),
        }
    }
}
/// Whether the held model revision still owes its default analysis. Hosts run the
/// explicit Analyze action only while this reads `due`; they never decide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisState {
    /// The analysis switch is on and this revision has no analysis yet.
    Due,
    /// An analysis of exactly this revision is held, automatic or explicit.
    Current,
    /// The switch is off; Analyze stays explicit.
    Off,
    /// The model has no structures to analyze.
    NotApplicable,
}
impl DisplayState {
    /// Owner, 2026-10-05: the analysis report is on by default.
    pub fn analysis_state(&self, revision_analyzed: bool, has_structures: bool) -> AnalysisState {
        if revision_analyzed {
            AnalysisState::Current
        } else if !has_structures {
            AnalysisState::NotApplicable
        } else if self.visibility.get(&DisplayOption::Analysis) == Some(&false) {
            AnalysisState::Off
        } else {
            AnalysisState::Due
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainSettings {
    pub corridor_half_width_m: f64,
    pub gap_tolerance_m: f64,
    pub ground_clearance_offset_m: f64,
    pub lowest_wire_drop_m: f64,
}
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayPatch {
    #[serde(default)]
    pub visibility: BTreeMap<DisplayOption, bool>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub display_case: Option<DisplayCase>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub vertical_exaggeration: Option<f64>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub terrain: Option<TerrainSettings>,
    /// Per-cable wire colour: a `#rrggbb` sets it, `null` returns the cable to its default pen.
    #[serde(default)]
    pub cable_colors: BTreeMap<String, Option<String>>,
}
fn is_hex_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].chars().all(|c| c.is_ascii_hexdigit())
}
// Omission retains state; explicit null is an invalid typed value. In particular,
// JavaScript non-finite numbers serialized to null cannot silently become omission.
fn present_value<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}
pub const MIN_EXAGGERATION: f64 = 0.5;
pub const MAX_EXAGGERATION: f64 = 100.0;
pub const MAX_TERRAIN_VALUE: f64 = 10_000.0;

impl DisplayState {
    /// Validate the whole patch before committing any of it.
    pub fn patched(&self, patch: &DisplayPatch) -> Result<Self, String> {
        let mut next = self.clone();
        if let Some(terrain) = &patch.terrain {
            if [
                terrain.corridor_half_width_m,
                terrain.gap_tolerance_m,
                terrain.ground_clearance_offset_m,
                terrain.lowest_wire_drop_m,
            ]
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=MAX_TERRAIN_VALUE).contains(v))
                || terrain.gap_tolerance_m <= 0.0
            {
                return Err("invalid_profile_view: terrain values must be finite and within 0..10000; gap tolerance must be positive".into());
            }
            next.terrain = terrain.clone();
        }
        if let Some(value) = patch.vertical_exaggeration {
            if !value.is_finite() || !(MIN_EXAGGERATION..=MAX_EXAGGERATION).contains(&value) {
                return Err(
                    "invalid_profile_view: vertical exaggeration must be within 0.5..100".into(),
                );
            }
            next.vertical_exaggeration = value;
        }
        if let Some(case) = &patch.display_case {
            if let DisplayCase::AnalysisCase { analysis_case_id } = case {
                if analysis_case_id.trim().is_empty()
                    || analysis_case_id.len() > 200
                    || analysis_case_id.chars().any(char::is_control)
                {
                    return Err("invalid_profile_view: invalid analysis case identity".into());
                }
            }
            next.display_case = case.clone();
        }
        for (cable, color) in &patch.cable_colors {
            if cable.trim().is_empty() || cable.len() > 200 || cable.chars().any(char::is_control) {
                return Err(
                    "invalid_profile_view: cable colours need an exact cable engineering name"
                        .into(),
                );
            }
            match color {
                Some(color) if is_hex_color(color) => {
                    next.cable_colors
                        .insert(cable.clone(), color.to_ascii_lowercase());
                }
                Some(_) => {
                    return Err("invalid_profile_view: a cable colour must be #rrggbb".into());
                }
                None => {
                    next.cable_colors.remove(cable);
                }
            }
        }
        if next.cable_colors.len() > 64 {
            return Err("invalid_profile_view: at most 64 configured cable colours".into());
        }
        next.visibility.extend(patch.visibility.clone());
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_profile_shows_structure_names_comments_and_review_markers() {
        let state = DisplayState::default();
        for shown in [
            DisplayOption::Review,
            DisplayOption::Structures,
            DisplayOption::Wire,
            DisplayOption::Ground,
            DisplayOption::StructureLabels,
            DisplayOption::StructureNumbers,
            DisplayOption::StructureNames,
            DisplayOption::StructureComments,
            // Owner, 2026-10-05: structure usage, attachment boxes and the section bitmap
            // are settings of their own, on by default.
            DisplayOption::StructureUsage,
            DisplayOption::SectionState,
            DisplayOption::SectionUsage,
        ] {
            assert!(state.visibility[&shown], "{shown:?} must be on by default");
        }
        for hidden in [DisplayOption::RouteDeviation] {
            assert!(
                !state.visibility[&hidden],
                "{hidden:?} must be on request only"
            );
        }
        // Both identifying labels remain adjustable after opening Profile.
        let patch: DisplayPatch = serde_json::from_str(
            r#"{"visibility":{"structure_names":false,"structure_comments":false}}"#,
        )
        .unwrap();
        let hidden = state.patched(&patch).unwrap();
        assert!(!hidden.visibility[&DisplayOption::StructureNames]);
        assert!(!hidden.visibility[&DisplayOption::StructureComments]);
    }
    #[test]
    fn cable_colours_patch_one_cable_at_a_time_and_reset_with_null() {
        let state = DisplayState::default();
        let set: DisplayPatch = serde_json::from_str(
            r##"{"cable_colors":{"ASTER 54.6":"#AABBCC","AAAC 34":"#010203"}}"##,
        )
        .unwrap();
        let next = state.patched(&set).unwrap();
        assert_eq!(next.cable_colors["ASTER 54.6"], "#aabbcc");
        assert_eq!(next.cable_colors.len(), 2);
        let reset: DisplayPatch =
            serde_json::from_str(r#"{"cable_colors":{"AAAC 34":null}}"#).unwrap();
        let next = next.patched(&reset).unwrap();
        assert_eq!(next.cable_colors.keys().collect::<Vec<_>>(), ["ASTER 54.6"]);
        for bad in [
            r#"{"cable_colors":{"ASTER":"red"}}"#,
            r##"{"cable_colors":{" ":"#aabbcc"}}"##,
        ] {
            let patch: DisplayPatch = serde_json::from_str(bad).unwrap();
            assert!(next.patched(&patch).is_err(), "{bad}");
        }
        // States saved before cable colours existed still read.
        let mut old = serde_json::to_value(DisplayState::default()).unwrap();
        old.as_object_mut().unwrap().remove("cable_colors");
        assert!(serde_json::from_value::<DisplayState>(old)
            .unwrap()
            .cable_colors
            .is_empty());
    }
    #[test]
    fn review_is_on_and_patches_are_atomic_and_strict() {
        let state = DisplayState::default();
        assert!(state.visibility[&DisplayOption::Review]);
        let patch: DisplayPatch =
            serde_json::from_str(r#"{"visibility":{"review":false,"wire":false}}"#).unwrap();
        let next = state.patched(&patch).unwrap();
        assert!(!next.visibility[&DisplayOption::Review]);
        assert!(next.visibility[&DisplayOption::Ground]);
        assert!(state
            .patched(&DisplayPatch {
                vertical_exaggeration: Some(f64::NAN),
                ..patch
            })
            .is_err());
        assert!(state.visibility[&DisplayOption::Review]);
        assert!(
            serde_json::from_str::<DisplayPatch>(r#"{"visibility":{"made_up":true}}"#).is_err()
        );
        assert!(serde_json::from_str::<DisplayPatch>(r#"{"showWire":false}"#).is_err());
        assert!(serde_json::from_str::<DisplayPatch>(r#"{"vertical_exaggeration":null}"#).is_err());
        assert!(serde_json::from_str::<DisplayPatch>(r#"{"terrain":null}"#).is_err());
    }
    #[test]
    fn analysis_report_is_on_by_default_and_one_switch_turns_it_off() {
        let state = DisplayState::default();
        assert!(state.visibility[&DisplayOption::Analysis]);
        assert_eq!(
            serde_json::to_value(DisplayOption::Analysis).unwrap(),
            "analysis"
        );
        // Due once per unanalyzed revision; a held analysis of that revision is current.
        assert_eq!(state.analysis_state(false, true), AnalysisState::Due);
        assert_eq!(state.analysis_state(true, true), AnalysisState::Current);
        assert_eq!(
            state.analysis_state(false, false),
            AnalysisState::NotApplicable
        );
        let off: DisplayPatch =
            serde_json::from_str(r#"{"visibility":{"analysis":false}}"#).unwrap();
        let off = state.patched(&off).unwrap();
        assert!(!off.visibility[&DisplayOption::Analysis]);
        assert_eq!(off.analysis_state(false, true), AnalysisState::Off);
        // An explicit Analyze still counts while the default is off.
        assert_eq!(off.analysis_state(true, true), AnalysisState::Current);
        // The switch is visibility only: nothing else in the display moves.
        let mut other = off.clone();
        other.visibility.insert(DisplayOption::Analysis, true);
        assert_eq!(other, state);
        let on: DisplayPatch = serde_json::from_str(r#"{"visibility":{"analysis":true}}"#).unwrap();
        assert_eq!(
            off.patched(&on).unwrap().analysis_state(false, true),
            AnalysisState::Due
        );
        assert!(
            serde_json::from_str::<DisplayPatch>(r#"{"visibility":{"analysis":"on"}}"#).is_err()
        );
        assert_eq!(
            serde_json::to_value(AnalysisState::NotApplicable).unwrap(),
            "not_applicable"
        );
    }
}
