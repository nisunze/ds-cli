//! Native control presentation shared by CLI receipts and the Profile renderer.
//! Every control submits DisplayPatch; hosts do not choose groups, limits or defaults.
use crate::profile_display::{
    DisplayOption as O, DisplayState, MAX_EXAGGERATION, MAX_TERRAIN_VALUE, MIN_EXAGGERATION,
};
use crate::profile_encoding::{CablePen, DisplayRow};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct NumericControl {
    pub label_key: &'static str,
    pub value: f64,
    pub minimum: f64,
    pub exclusive_minimum: bool,
    pub maximum: f64,
    /// A complete current typed patch. Replace only the named value before submitting.
    pub request: Value,
    pub value_path: Vec<&'static str>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ControlGroup {
    pub id: &'static str,
    pub label_key: &'static str,
    pub initially_open: bool,
    pub rows: Vec<DisplayRow>,
    pub numbers: Vec<NumericControl>,
    pub cables: Vec<CablePen>,
}
fn group(option: O) -> &'static str {
    match option {
        O::Grid => "general",
        O::Ground | O::SideProfiles | O::TerrainPoints | O::TerrainOrdinates | O::ClearanceLine => {
            "terrain"
        }
        O::Structures | O::Wire | O::Embedded | O::OffsetEmbedmentHeight | O::RouteDeviation => {
            "structures"
        }
        O::StructureLabels
        | O::StructureNumbers
        | O::StructureNames
        | O::StructureComments
        | O::SectionLabels
        | O::SpanDistances => "labels",
        O::Review | O::StructureUsage | O::SectionState | O::SectionUsage | O::Analysis => {
            "analysis"
        }
    }
}
pub fn groups(state: &DisplayState, rows: &[DisplayRow], cables: &[CablePen]) -> Vec<ControlGroup> {
    ["general", "terrain", "structures", "labels", "analysis"]
        .into_iter()
        .map(|id| {
            let numbers = match id {
                "general" => vec![NumericControl {
                    label_key: "gprof_vertical_exaggeration",
                    value: state.vertical_exaggeration,
                    minimum: MIN_EXAGGERATION,
                    exclusive_minimum: false,
                    maximum: MAX_EXAGGERATION,
                    request: json!({"vertical_exaggeration": state.vertical_exaggeration}),
                    value_path: vec!["vertical_exaggeration"],
                }],
                "terrain" => [
                    (
                        "corridor_half_width_m",
                        "gprof_terrain_half_width",
                        state.terrain.corridor_half_width_m,
                        false,
                    ),
                    (
                        "gap_tolerance_m",
                        "gprof_terrain_gap",
                        state.terrain.gap_tolerance_m,
                        true,
                    ),
                    (
                        "ground_clearance_offset_m",
                        "gprof_terrain_ground_offset",
                        state.terrain.ground_clearance_offset_m,
                        false,
                    ),
                    (
                        "lowest_wire_drop_m",
                        "gprof_terrain_wire_drop",
                        state.terrain.lowest_wire_drop_m,
                        false,
                    ),
                ]
                .into_iter()
                .map(
                    |(field, label_key, value, exclusive_minimum)| NumericControl {
                        label_key,
                        value,
                        minimum: 0.0,
                        exclusive_minimum,
                        maximum: MAX_TERRAIN_VALUE,
                        request: json!({"terrain":state.terrain}),
                        value_path: vec!["terrain", field],
                    },
                )
                .collect(),
                _ => vec![],
            };
            ControlGroup {
                id,
                label_key: match id {
                    "general" => "gprof_display_group_general",
                    "terrain" => "gprof_display_group_terrain",
                    "structures" => "gprof_display_group_structures",
                    "labels" => "gprof_display_group_labels",
                    _ => "gprof_display_group_analysis",
                },
                initially_open: id == "general",
                rows: rows
                    .iter()
                    .filter(|row| group(row.option) == id)
                    .cloned()
                    .collect(),
                numbers,
                cables: if id == "structures" {
                    cables.to_vec()
                } else {
                    vec![]
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_display::DisplayPatch;
    #[test]
    fn every_native_option_appears_once_and_numeric_requests_obey_the_owner() {
        let state = DisplayState::default();
        let styles =
            serde_json::from_str(include_str!("../tests/fixtures/profile-display-style.json"))
                .unwrap();
        let rows = crate::profile_encoding::resolve(&state, &styles).unwrap();
        let groups = groups(&state, &rows, &[]);
        let options = groups
            .iter()
            .flat_map(|group| group.rows.iter().map(|row| row.option))
            .collect::<Vec<_>>();
        assert_eq!(options.len(), O::ALL.len());
        assert_eq!(
            options
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            O::ALL.into_iter().collect()
        );
        for control in groups.iter().flat_map(|group| &group.numbers) {
            let patch: DisplayPatch = serde_json::from_value(control.request.clone()).unwrap();
            assert_eq!(state.patched(&patch).unwrap(), state);
        }
        let mut scale = groups[0].numbers[0].request.clone();
        scale["vertical_exaggeration"] = json!(101.0);
        assert!(state
            .patched(&serde_json::from_value(scale).unwrap())
            .is_err());
        let mut terrain = groups[1].numbers[1].request.clone();
        terrain["terrain"]["gap_tolerance_m"] = json!(0.0);
        assert!(state
            .patched(&serde_json::from_value(terrain).unwrap())
            .is_err());
    }
}
