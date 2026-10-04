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
}
impl DisplayOption {
    pub const ALL: [Self; 18] = [
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
}
impl Default for DisplayState {
    fn default() -> Self {
        Self {
            visibility: DisplayOption::ALL
                .into_iter()
                .map(|option| {
                    (
                        option,
                        // Owner, 2026-10-03: the Profile opens on PLS-CADD-style attachment
                        // boxes, structure numbers and one usage label per structure. Type names,
                        // comments and route deviation stay one Labels choice away.
                        !matches!(
                            option,
                            DisplayOption::SideProfiles
                                | DisplayOption::TerrainOrdinates
                                | DisplayOption::SectionLabels
                                | DisplayOption::Embedded
                                | DisplayOption::StructureNames
                                | DisplayOption::StructureComments
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
}
// Omission retains state; explicit null is an invalid typed value. In particular,
// JavaScript non-finite numbers serialized to null cannot silently become omission.
fn present_value<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}
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
            .any(|v| !v.is_finite() || !(0.0..=10_000.0).contains(v))
                || terrain.gap_tolerance_m <= 0.0
            {
                return Err("invalid_profile_view: terrain values must be finite and within 0..10000; gap tolerance must be positive".into());
            }
            next.terrain = terrain.clone();
        }
        if let Some(value) = patch.vertical_exaggeration {
            if !value.is_finite() || !(0.5..=100.0).contains(&value) {
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
        next.visibility.extend(patch.visibility.clone());
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_profile_shows_review_boxes_usage_and_numbers_without_type_names() {
        let state = DisplayState::default();
        for shown in [
            DisplayOption::Review,
            DisplayOption::Structures,
            DisplayOption::Wire,
            DisplayOption::Ground,
            DisplayOption::StructureLabels,
            DisplayOption::StructureNumbers,
        ] {
            assert!(state.visibility[&shown], "{shown:?} must be on by default");
        }
        for hidden in [
            DisplayOption::StructureNames,
            DisplayOption::StructureComments,
            DisplayOption::RouteDeviation,
        ] {
            assert!(
                !state.visibility[&hidden],
                "{hidden:?} must be on request only"
            );
        }
        // Still one Labels choice away.
        let patch: DisplayPatch =
            serde_json::from_str(r#"{"visibility":{"structure_names":true}}"#).unwrap();
        assert!(state.patched(&patch).unwrap().visibility[&DisplayOption::StructureNames]);
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
}
