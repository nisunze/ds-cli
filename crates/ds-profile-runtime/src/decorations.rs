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
    pub rotation_radians: f64,
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
    if state.visibility[&O::OffsetEmbedmentHeight] && state.visibility[&O::Structures] {
        for structure in scene.bands.iter().flat_map(|band| &band.structures) {
            let (points, text, encoding) = match structure.setting_depth_m {
                Some(depth) => (
                    vec![structure.ground.clone(), structure.base.clone()],
                    format!("{depth:.1} m"),
                    pen(O::OffsetEmbedmentHeight),
                ),
                None => (vec![], "◌".into(), pen(O::Embedded)),
            };
            out.push(Primitive {
                option: O::OffsetEmbedmentHeight,
                encoding,
                points,
                labels: vec![Label {
                    point: structure.base.clone(),
                    text,
                    offset_px: [6.0, 0.0],
                    rotation_radians: 0.0,
                }],
            });
        }
    }
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
                        rotation_radians: 0.0,
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
                        rotation_radians: 0.0,
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
                            // The exact cable engineering name reads in the wires; the opaque
                            // section identity stays in the bitmap summary and selection.
                            text: section_label(
                                span.cable_engineering_name.as_str(),
                                scene
                                    .evidence
                                    .section_basis
                                    .iter()
                                    .find(|basis| basis.section_id == span.section_id)
                                    .and_then(|basis| basis.maximum_support_tension_rts_percent),
                            ),
                            offset_px: [0.0, 24.0],
                            rotation_radians: 0.0,
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
                            // Start above the attachment boxes (-8..8 px) so they never cover a name.
                            offset_px: [-offset - 6.0, -12.0],
                            rotation_radians: -std::f64::consts::FRAC_PI_2,
                        }],
                    });
                    offset -= 15.0;
                }
                if state.visibility[&O::RouteDeviation] && structure.is_alignment_angle {
                    let text = structure
                        .angle_deviation_deg
                        .map(|value| format!("Dev {value:+.1}°"))
                        .unwrap_or_else(|| "Dev unresolved".into());
                    out.push(Primitive {
                        option: O::RouteDeviation,
                        encoding: pen(O::RouteDeviation),
                        points: vec![],
                        labels: vec![Label {
                            point: *structure.top.as_ref().unwrap_or(&structure.ground),
                            text,
                            offset_px: [-offset - 6.0, -12.0],
                            rotation_radians: -std::f64::consts::FRAC_PI_2,
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
fn section_label(cable: &str, maximum_support_tension_rts_percent: Option<f64>) -> String {
    match maximum_support_tension_rts_percent.filter(|value| value.is_finite() && *value >= 0.0) {
        Some(percent) => format!("{cable} · Max support RTS {percent:.1}%"),
        None => format!("{cable} · Max support RTS unknown"),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structure_text_is_vertical_and_only_on_request() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid"
        ))
        .unwrap();
        let (package, _) = ds_grid_exchange::dsgrid::ingest(&bytes).unwrap();
        let mut scene = ds_grid_engine::GridSession::open(package.snapshot)
            .profile_atlas_scene(Default::default())
            .unwrap();
        let structure = &mut scene.bands[0].structures[0];
        structure.comment_1 = Some("TAP DOF E-LINK".into());
        structure.is_alignment_angle = true;
        structure.angle_deviation_deg = Some(-48.6);
        let state = DisplayState::default();
        assert!(!state.visibility[&O::StructureComments]);
        assert!(!state.visibility[&O::RouteDeviation]);
        let document = serde_json::from_str(include_str!(
            "../../../tests/fixtures/profile-display-style.json"
        ))
        .unwrap();
        let rows = ds_command_kernel::profile_encoding::resolve(&state, &document).unwrap();
        let primitives = project(&scene, &state, &rows, &Default::default());
        // Owner, 2026-10-03: by default only the structure number is written per structure.
        assert!(primitives.iter().all(|p| !matches!(
            p.option,
            O::StructureNames | O::StructureComments | O::RouteDeviation
        )));
        assert!(primitives.iter().any(|p| p.option == O::StructureNumbers));
        let mut named = state.clone();
        named.visibility.insert(O::StructureNames, true);
        let primitives = project(&scene, &named, &rows, &Default::default());
        assert!(primitives.iter().any(|p| p.option == O::StructureNames));
        let identities = primitives
            .iter()
            .filter(|p| matches!(p.option, O::StructureNames | O::StructureNumbers))
            .flat_map(|p| &p.labels)
            .collect::<Vec<_>>();
        assert!(!identities.is_empty());
        assert!(identities
            .iter()
            .all(|label| label.rotation_radians == -std::f64::consts::FRAC_PI_2));
        let mut shown = state;
        shown.visibility.insert(O::StructureComments, true);
        shown.visibility.insert(O::RouteDeviation, true);
        let primitives = project(&scene, &shown, &rows, &Default::default());
        assert!(primitives.iter().any(|p| p.option == O::StructureComments
            && p.labels.iter().any(|l| l.text.contains("TAP DOF E-LINK"))));
        assert!(primitives
            .iter()
            .any(|p| p.option == O::RouteDeviation && p.labels[0].text == "Dev -48.6°"));
    }
    #[test]
    fn offset_embedment_uses_the_canonical_foot_labels_declared_depth_and_marks_unknown_neutrally()
    {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid"
        ))
        .unwrap();
        let (package, _) = ds_grid_exchange::dsgrid::ingest(&bytes).unwrap();
        let mut scene = ds_grid_engine::GridSession::open(package.snapshot)
            .profile_atlas_scene(Default::default())
            .unwrap();
        let structure = &mut scene.bands[0].structures[0];
        structure.setting_depth_m = Some(2.0);
        structure.base.elevation_m = structure.ground.elevation_m - 2.0;
        structure.base.scene_y = structure.ground.scene_y + 10.0;
        let ground = structure.ground.clone();
        let foot = structure.base.clone();
        let state = DisplayState::default();
        assert!(state.visibility[&O::OffsetEmbedmentHeight]);
        let document = serde_json::from_str(include_str!(
            "../../../tests/fixtures/profile-display-style.json"
        ))
        .unwrap();
        let rows = ds_command_kernel::profile_encoding::resolve(&state, &document).unwrap();
        let primitives = project(&scene, &state, &rows, &Default::default());
        let dimension = primitives
            .iter()
            .find(|primitive| primitive.option == O::OffsetEmbedmentHeight)
            .unwrap();
        assert_eq!(dimension.points, vec![ground, foot]);
        assert_eq!(dimension.labels[0].text, "2.0 m");
        scene.bands[0].structures[0].setting_depth_m = None;
        let unknown = project(&scene, &state, &rows, &Default::default());
        let dimension = unknown
            .iter()
            .find(|primitive| primitive.option == O::OffsetEmbedmentHeight)
            .unwrap();
        assert!(dimension.points.is_empty());
        assert_eq!(dimension.labels[0].text, "◌");
        assert_eq!(
            dimension.encoding.color,
            rows.iter()
                .find(|row| row.option == O::Embedded)
                .unwrap()
                .encoding
                .color
        );
        let mut hidden = state;
        hidden.visibility.insert(O::OffsetEmbedmentHeight, false);
        assert!(project(&scene, &hidden, &rows, &Default::default())
            .iter()
            .all(|primitive| primitive.option != O::OffsetEmbedmentHeight));
    }
}
