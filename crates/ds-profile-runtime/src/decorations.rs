//! Display-only primitives. Coordinates, text, field order and pens are native.
use ds_command_kernel::{
    profile_display::{DisplayOption as O, DisplayState},
    profile_encoding::{DisplayRow, Encoding},
};
use ds_grid_engine::profile_atlas::ProfileAtlasPoint;
use ds_grid_engine::{
    profile_labels::{compose_structure_label, StructureLabelValues},
    ProfileAtlasScene,
};
use ds_grid_model::{StructureLabelField, StructureLabelPolicy};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Primitive {
    pub option: O,
    pub encoding: Encoding,
    pub points: Vec<ProfileAtlasPoint>,
    pub labels: Vec<Label>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Label {
    pub point: ProfileAtlasPoint,
    pub text: String,
    pub offset_px: [f64; 2],
}
pub fn project(
    scene: &ProfileAtlasScene,
    state: &DisplayState,
    rows: &[DisplayRow],
    policy: &StructureLabelPolicy,
) -> Vec<Primitive> {
    let pen = |option| {
        rows.iter()
            .find(|row| row.option == option)
            .unwrap()
            .encoding
            .clone()
    };
    let mut out = Vec::new();
    if state.visibility[&O::TerrainOrdinates] {
        for band in &scene.bands {
            for terrain in &band.terrain_points {
                let mut foot = terrain.point;
                foot.scene_y = scene.plot_bounds.max_y;
                out.push(Primitive {
                    option: O::TerrainOrdinates,
                    encoding: pen(O::TerrainOrdinates),
                    points: vec![terrain.point, foot],
                    labels: vec![Label {
                        point: terrain.point,
                        text: format!("{:.1} m", terrain.point.elevation_m),
                        offset_px: [4.0, 14.0],
                    }],
                });
            }
        }
    }
    if state.visibility[&O::SpanDistances] {
        use ds_grid_engine::profile_atlas::ProfileAtlasClearanceGuideKind as G;
        let lanes = |kind| {
            crate::label_lanes::Lanes::new(
                scene
                    .clearance_guides
                    .iter()
                    .filter(move |guide| guide.kind == kind)
                    .flat_map(|guide| {
                        guide
                            .segments
                            .iter()
                            .map(|segment| segment.points.as_slice())
                    }),
            )
        };
        let ground = lanes(G::GroundOffset);
        let wire = lanes(G::LowestWireDrop);
        for span in &scene.conductor_spans {
            if let Some(label) = &span.span_label {
                let mut point = label.point;
                let a = ground.y(point.scene_x);
                let b = wire.y(point.scene_x);
                if a.is_some() || b.is_some() {
                    point.scene_y = (a.unwrap_or(point.scene_y) + b.unwrap_or(point.scene_y)) / 2.0;
                }
                out.push(Primitive {
                    option: O::SpanDistances,
                    encoding: pen(O::SpanDistances),
                    points: vec![],
                    labels: vec![Label {
                        point,
                        text: label.text.clone(),
                        offset_px: [0.0, 0.0],
                    }],
                });
            }
        }
    }
    if state.visibility[&O::SectionLabels] {
        let mut seen = std::collections::BTreeSet::new();
        for span in &scene.conductor_spans {
            if let Some(label) = &span.span_label {
                if seen.insert(span.section_id.clone()) {
                    out.push(Primitive {
                        option: O::SectionLabels,
                        encoding: pen(O::SectionLabels),
                        points: vec![],
                        labels: vec![Label {
                            point: label.point,
                            text: section_label(
                                span.section_id.as_str(),
                                scene
                                    .evidence
                                    .section_basis
                                    .iter()
                                    .find(|basis| basis.section_id == span.section_id)
                                    .and_then(|basis| basis.maximum_support_tension_rts_percent),
                            ),
                            offset_px: [0.0, 24.0],
                        }],
                    });
                }
            }
        }
    }
    if state.visibility[&O::StructureLabels] {
        for band in &scene.bands {
            for structure in &band.structures {
                let values = StructureLabelValues {
                    number: structure.engineering_number.as_deref(),
                    station_m: Some(structure.ground.chain_station_m),
                    structure_type: &structure.engineering_name,
                    height_m: structure.height_m,
                    alignment: &band.label,
                    comments: [
                        structure.comment_1.as_deref(),
                        structure.comment_2.as_deref(),
                        structure.comment_3.as_deref(),
                    ],
                };
                let mut offset = -12.0;
                for (option, fields) in [
                    (O::StructureNumbers, vec![StructureLabelField::Number]),
                    (O::StructureNames, vec![StructureLabelField::Type]),
                    (
                        O::StructureComments,
                        vec![
                            StructureLabelField::Comment1,
                            StructureLabelField::Comment2,
                            StructureLabelField::Comment3,
                        ],
                    ),
                ] {
                    if !state.visibility[&option] {
                        continue;
                    }
                    let mut single = policy.clone();
                    single.fields = fields;
                    let text = compose_structure_label(&values, &single);
                    if text.is_empty() {
                        continue;
                    }
                    out.push(Primitive {
                        option,
                        encoding: pen(option),
                        points: vec![],
                        labels: vec![Label {
                            point: *structure.top.as_ref().unwrap_or(&structure.ground),
                            text,
                            offset_px: [6.0, offset],
                        }],
                    });
                    offset -= 15.0;
                }
                if structure.is_alignment_angle {
                    let text = structure
                        .angle_deviation_deg
                        .map(|value| format!("Dev {value:+.1}°"))
                        .unwrap_or_else(|| "Dev unresolved".into());
                    out.push(Primitive {
                        option: O::StructureLabels,
                        encoding: pen(O::StructureLabels),
                        points: vec![],
                        labels: vec![Label {
                            point: *structure.top.as_ref().unwrap_or(&structure.ground),
                            text,
                            offset_px: [6.0, offset],
                        }],
                    });
                }
            }
        }
    }
    out
}

// A native solver fact, explicitly labelled against rated strength. It is not
// the independent clearance verdict or a qualified section capacity PASS.
fn section_label(section: &str, maximum_support_tension_rts_percent: Option<f64>) -> String {
    match maximum_support_tension_rts_percent.filter(|value| value.is_finite() && *value >= 0.0) {
        Some(percent) => format!("{section} · Max support RTS {percent:.1}%"),
        None => format!("{section} · Max support RTS unknown"),
    }
}

#[cfg(test)]
mod section_usage_tests {
    use super::*;
    #[test]
    fn section_label_names_native_rated_strength_basis_and_never_relabels_clearance_as_usage() {
        assert_eq!(
            section_label("TS-21", Some(37.25)),
            "TS-21 · Max support RTS 37.2%"
        );
        assert_eq!(
            section_label("TS-21", None),
            "TS-21 · Max support RTS unknown"
        );
        assert_eq!(
            section_label("TS-21", Some(f64::NAN)),
            "TS-21 · Max support RTS unknown"
        );
        assert_eq!(
            section_label("TS-21", Some(0.0)),
            "TS-21 · Max support RTS 0.0%"
        );
        assert!(!section_label("TS-21", None).contains("G"));
    }
}
