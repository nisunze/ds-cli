//! Transient map interaction geometry for UI hosts.
//!
//! The UI supplies screen coordinates and pointer intent. This crate owns the
//! numeric decisions; project mutation and engineering admission stay outside.

use ds_geo_lite::projection::{ProjectionConfig, ProjectionMetadata, ProjectionSelection};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub const DEFAULT_SNAP_TOLERANCE_PX: f64 = 12.0;
pub const MIN_SNAP_TOLERANCE_PX: f64 = 4.0;
pub const MAX_SNAP_TOLERANCE_PX: f64 = 32.0;
pub const MIN_POLAR_INCREMENT_DEG: f64 = 0.1;
pub const MAX_POLAR_INCREMENT_DEG: f64 = 90.0;
const SCREEN_DISTANCE_EPSILON_PX: f64 = 0.25;
const MIN_EDIT_EDGE_TOLERANCE_PX: f64 = 8.0;
const MAX_EDIT_EDGE_TOLERANCE_PX: f64 = 18.0;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DrawAngleMode {
    Off,
    Ortho,
    Polar,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapCandidateKind {
    #[serde(rename = "self")]
    SelfSnap,
    Vertex,
    Midpoint,
    Edge,
    Grid,
    Buffer,
}

impl SnapCandidateKind {
    const fn priority(self) -> u8 {
        match self {
            Self::SelfSnap => 0,
            Self::Vertex => 1,
            Self::Midpoint => 2,
            Self::Edge => 3,
            Self::Grid => 4,
            Self::Buffer => 5,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct RankedSnapCandidate {
    pub target_kind: SnapCandidateKind,
    pub screen_point: [f64; 2],
    pub distance_m: f64,
    pub owner_order: usize,
    #[serde(default)]
    pub layer_name: String,
    #[serde(default)]
    pub feature_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ScreenVertex {
    pub index: usize,
    pub point: [f64; 2],
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum InteractionCommand {
    NormalizeSnapTolerance {
        value: Option<f64>,
    },
    NormalizePolarIncrement {
        value: Option<f64>,
    },
    NormalizeLengthStep {
        value: Option<f64>,
    },
    ConstrainDrawCoordinate {
        coord: [f64; 2],
        anchor: Option<[f64; 2]>,
        angle_mode: DrawAngleMode,
        polar_increment_deg: f64,
        length_enabled: bool,
        length_step_m: f64,
    },
    CoordinateGridSnap {
        pointer: [f64; 2],
        projection: ProjectionConfig,
        interval_m: f64,
    },
    RankSnapCandidates {
        pointer: [f64; 2],
        tolerance_px: f64,
        candidates: Vec<RankedSnapCandidate>,
    },
    ScreenVertexHit {
        pointer: [f64; 2],
        vertices: Vec<ScreenVertex>,
        tolerance_px: f64,
    },
    ScreenSegmentHit {
        pointer: [f64; 2],
        parts: Vec<Vec<[f64; 2]>>,
        tolerance_px: Option<f64>,
    },
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InteractionResult {
    Number {
        value: Option<f64>,
    },
    Coordinate {
        coord: [f64; 2],
        constrained: bool,
    },
    GridSnap {
        coord: [f64; 2],
        distance_m: f64,
        projection: ProjectionMetadata,
    },
    SnapWinner {
        index: Option<usize>,
    },
    VertexHit {
        index: Option<usize>,
        distance_px: Option<f64>,
    },
    SegmentHit {
        part_index: Option<usize>,
        segment_index: Option<usize>,
        distance_px: Option<f64>,
    },
}

pub fn evaluate(command: InteractionCommand) -> Result<InteractionResult, String> {
    match command {
        InteractionCommand::NormalizeSnapTolerance { value } => Ok(InteractionResult::Number {
            value: Some(normalize_snap_tolerance_px(value)),
        }),
        InteractionCommand::NormalizePolarIncrement { value } => Ok(InteractionResult::Number {
            value: normalize_polar_increment_deg(value),
        }),
        InteractionCommand::NormalizeLengthStep { value } => Ok(InteractionResult::Number {
            value: normalize_length_step_m(value),
        }),
        InteractionCommand::ConstrainDrawCoordinate {
            coord,
            anchor,
            angle_mode,
            polar_increment_deg,
            length_enabled,
            length_step_m,
        } => constrain_draw_coordinate(
            coord,
            anchor,
            angle_mode,
            polar_increment_deg,
            length_enabled,
            length_step_m,
        ),
        InteractionCommand::CoordinateGridSnap {
            pointer,
            projection,
            interval_m,
        } => coordinate_grid_snap(pointer, projection, interval_m),
        InteractionCommand::RankSnapCandidates {
            pointer,
            tolerance_px,
            candidates,
        } => Ok(InteractionResult::SnapWinner {
            index: rank_snap_candidates(pointer, tolerance_px, &candidates)?,
        }),
        InteractionCommand::ScreenVertexHit {
            pointer,
            vertices,
            tolerance_px,
        } => screen_vertex_hit(pointer, &vertices, tolerance_px),
        InteractionCommand::ScreenSegmentHit {
            pointer,
            parts,
            tolerance_px,
        } => screen_segment_hit(pointer, &parts, tolerance_px),
    }
}

pub fn normalize_snap_tolerance_px(value: Option<f64>) -> f64 {
    match value {
        Some(value) if value.is_finite() && value > 0.0 => {
            value.clamp(MIN_SNAP_TOLERANCE_PX, MAX_SNAP_TOLERANCE_PX)
        }
        _ => DEFAULT_SNAP_TOLERANCE_PX,
    }
}

pub fn normalize_polar_increment_deg(value: Option<f64>) -> Option<f64> {
    value
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.clamp(MIN_POLAR_INCREMENT_DEG, MAX_POLAR_INCREMENT_DEG))
}

pub fn normalize_length_step_m(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value >= 0.0)
}

fn constrain_draw_coordinate(
    coord: [f64; 2],
    anchor: Option<[f64; 2]>,
    angle_mode: DrawAngleMode,
    polar_increment_deg: f64,
    length_enabled: bool,
    length_step_m: f64,
) -> Result<InteractionResult, String> {
    finite_point(coord, "coord")?;
    let Some(anchor) = anchor else {
        return Ok(InteractionResult::Coordinate {
            coord,
            constrained: false,
        });
    };
    finite_point(anchor, "anchor")?;
    let angle_enabled = angle_mode != DrawAngleMode::Off;
    let length_enabled = length_enabled && length_step_m > 0.0;
    if !angle_enabled && !length_enabled {
        return Ok(InteractionResult::Coordinate {
            coord,
            constrained: false,
        });
    }
    let polar_increment_deg = if angle_mode == DrawAngleMode::Polar {
        normalize_polar_increment_deg(Some(polar_increment_deg))
            .ok_or_else(|| "polar_increment_deg must be finite and greater than zero".to_string())?
    } else {
        polar_increment_deg
    };
    let length_step_m = if length_enabled {
        normalize_length_step_m(Some(length_step_m))
            .ok_or_else(|| "length_step_m must be finite and non-negative".to_string())?
    } else {
        length_step_m
    };

    let mut distance_m = ds_geo_lite::haversine_m(anchor[0], anchor[1], coord[0], coord[1]);
    if distance_m < 1e-6 {
        return Ok(InteractionResult::Coordinate {
            coord,
            constrained: false,
        });
    }
    let mut bearing_deg = ds_geo_lite::bearing_deg(anchor[0], anchor[1], coord[0], coord[1]);
    if angle_enabled {
        let step = match angle_mode {
            DrawAngleMode::Off => unreachable!(),
            DrawAngleMode::Ortho => 90.0,
            DrawAngleMode::Polar => polar_increment_deg,
        };
        bearing_deg = (bearing_deg / step).round() * step;
    }
    if length_enabled {
        distance_m = length_step_m.max((distance_m / length_step_m).round() * length_step_m);
    }
    let projected = ds_geo_lite::destination_point(anchor[0], anchor[1], bearing_deg, distance_m);
    let coord = [projected[0], projected[1]];
    finite_point(coord, "constrained coord")?;
    Ok(InteractionResult::Coordinate {
        coord,
        constrained: true,
    })
}

fn coordinate_grid_snap(
    pointer: [f64; 2],
    projection: ProjectionConfig,
    interval_m: f64,
) -> Result<InteractionResult, String> {
    finite_point(pointer, "pointer")?;
    if !interval_m.is_finite() || interval_m <= 0.0 {
        return Err("interval_m must be finite and greater than zero".into());
    }
    if projection.mode == ProjectionSelection::Utm
        && projection
            .utm_zone
            .is_some_and(|zone| !(1..=60).contains(&zone))
    {
        return Err("utm_zone must be between 1 and 60".into());
    }
    let projection = ProjectionMetadata::for_origin_with_config(pointer[0], pointer[1], projection);
    let projected = projection.project_coord(pointer);
    finite_point(projected, "projected pointer")?;
    let target = [
        (projected[0] / interval_m).round() * interval_m,
        (projected[1] / interval_m).round() * interval_m,
    ];
    let coord = projection.unproject_coord(target);
    finite_point(coord, "grid coordinate")?;
    Ok(InteractionResult::GridSnap {
        coord,
        distance_m: (target[0] - projected[0]).hypot(target[1] - projected[1]),
        projection,
    })
}

fn rank_snap_candidates(
    pointer: [f64; 2],
    tolerance_px: f64,
    candidates: &[RankedSnapCandidate],
) -> Result<Option<usize>, String> {
    finite_point(pointer, "pointer")?;
    positive_finite(tolerance_px, "tolerance_px")?;
    for candidate in candidates {
        finite_point(candidate.screen_point, "snap candidate screen_point")?;
        if !candidate.distance_m.is_finite() || candidate.distance_m < 0.0 {
            return Err("snap candidate metric distances must be finite and non-negative".into());
        }
    }
    let mut best: Option<(usize, &RankedSnapCandidate, f64)> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        let distance_px =
            (pointer[0] - candidate.screen_point[0]).hypot(pointer[1] - candidate.screen_point[1]);
        if distance_px > tolerance_px {
            continue;
        }
        if best.is_none_or(|(_, current, current_distance)| {
            compare_snap_candidates(candidate, distance_px, current, current_distance)
                == Ordering::Less
        }) {
            best = Some((index, candidate, distance_px));
        }
    }
    Ok(best.map(|(index, _, _)| index))
}

fn compare_snap_candidates(
    left: &RankedSnapCandidate,
    left_distance_px: f64,
    right: &RankedSnapCandidate,
    right_distance_px: f64,
) -> Ordering {
    let delta = left_distance_px - right_distance_px;
    if delta.abs() > SCREEN_DISTANCE_EPSILON_PX {
        return left_distance_px.total_cmp(&right_distance_px);
    }
    left.target_kind
        .priority()
        .cmp(&right.target_kind.priority())
        .then_with(|| left.distance_m.total_cmp(&right.distance_m))
        .then_with(|| left.owner_order.cmp(&right.owner_order))
        .then_with(|| left.layer_name.cmp(&right.layer_name))
        .then_with(|| left.feature_id.cmp(&right.feature_id))
}

fn screen_vertex_hit(
    pointer: [f64; 2],
    vertices: &[ScreenVertex],
    tolerance_px: f64,
) -> Result<InteractionResult, String> {
    finite_point(pointer, "pointer")?;
    positive_finite(tolerance_px, "tolerance_px")?;
    let mut best: Option<(usize, f64)> = None;
    for vertex in vertices {
        finite_point(vertex.point, "vertex point")?;
        let distance_px = (pointer[0] - vertex.point[0]).hypot(pointer[1] - vertex.point[1]);
        if distance_px <= tolerance_px
            && best.is_none_or(|(best_index, best_distance)| {
                distance_px < best_distance
                    || (distance_px == best_distance && vertex.index < best_index)
            })
        {
            best = Some((vertex.index, distance_px));
        }
    }
    Ok(InteractionResult::VertexHit {
        index: best.map(|value| value.0),
        distance_px: best.map(|value| value.1),
    })
}

fn screen_segment_hit(
    pointer: [f64; 2],
    parts: &[Vec<[f64; 2]>],
    tolerance_px: Option<f64>,
) -> Result<InteractionResult, String> {
    finite_point(pointer, "pointer")?;
    let tolerance_px = match tolerance_px {
        Some(value) if value.is_finite() && value > 0.0 => {
            value.clamp(MIN_EDIT_EDGE_TOLERANCE_PX, MAX_EDIT_EDGE_TOLERANCE_PX)
        }
        _ => DEFAULT_SNAP_TOLERANCE_PX,
    };
    let mut best: Option<(usize, usize, f64)> = None;
    for (part_index, part) in parts.iter().enumerate() {
        for point in part {
            finite_point(*point, "segment point")?;
        }
        for (segment_index, pair) in part.windows(2).enumerate() {
            let distance_px = distance_to_segment(pointer, pair[0], pair[1]);
            if distance_px <= tolerance_px
                && best.is_none_or(|(best_part, best_segment, best_distance)| {
                    distance_px < best_distance
                        || (distance_px == best_distance
                            && (part_index, segment_index) < (best_part, best_segment))
                })
            {
                best = Some((part_index, segment_index, distance_px));
            }
        }
    }
    Ok(InteractionResult::SegmentHit {
        part_index: best.map(|value| value.0),
        segment_index: best.map(|value| value.1),
        distance_px: best.map(|value| value.2),
    })
}

fn distance_to_segment(point: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let length_squared = dx * dx + dy * dy;
    if length_squared <= 0.0 {
        return (point[0] - a[0]).hypot(point[1] - a[1]);
    }
    let t = (((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / length_squared).clamp(0.0, 1.0);
    let nearest = [a[0] + t * dx, a[1] + t * dy];
    (point[0] - nearest[0]).hypot(point[1] - nearest[1])
}

fn finite_point(point: [f64; 2], name: &str) -> Result<(), String> {
    if point.into_iter().all(f64::is_finite) {
        Ok(())
    } else {
        Err(format!("{name} must contain finite coordinates"))
    }
}

fn positive_finite(value: f64, name: &str) -> Result<(), String> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(format!("{name} must be finite and greater than zero"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_ranking_preserves_screen_epsilon_and_kind_priority() {
        let candidates = vec![
            RankedSnapCandidate {
                target_kind: SnapCandidateKind::Grid,
                screen_point: [1.0, 0.0],
                distance_m: 0.1,
                owner_order: 2,
                layer_name: String::new(),
                feature_id: String::new(),
            },
            RankedSnapCandidate {
                target_kind: SnapCandidateKind::Vertex,
                screen_point: [1.2, 0.0],
                distance_m: 2.0,
                owner_order: 0,
                layer_name: "poles".into(),
                feature_id: "p1".into(),
            },
        ];
        assert_eq!(
            rank_snap_candidates([0.0, 0.0], 12.0, &candidates).unwrap(),
            Some(1)
        );
    }

    #[test]
    fn precision_constraint_rounds_angle_and_length_in_rust() {
        let result = constrain_draw_coordinate(
            [30.000_91, -2.000_25],
            Some([30.0, -2.0]),
            DrawAngleMode::Ortho,
            15.0,
            true,
            10.0,
        )
        .unwrap();
        let InteractionResult::Coordinate { coord, constrained } = result else {
            panic!("coordinate result")
        };
        assert!(constrained);
        let distance = ds_geo_lite::haversine_m(30.0, -2.0, coord[0], coord[1]);
        assert!((distance / 10.0 - (distance / 10.0).round()).abs() < 1e-6);
    }

    #[test]
    fn grid_snap_uses_the_selected_projection() {
        let result = coordinate_grid_snap(
            [30.0, -2.6],
            ProjectionConfig {
                mode: ProjectionSelection::WebMercator,
                utm_zone: None,
                utm_southern: None,
            },
            100.0,
        )
        .unwrap();
        let InteractionResult::GridSnap {
            coord,
            distance_m,
            projection,
        } = result
        else {
            panic!("grid result")
        };
        assert_eq!(
            projection.mode,
            ds_geo_lite::projection::ProjectionMode::WebMercator
        );
        assert!(coord.into_iter().all(f64::is_finite));
        assert!(distance_m <= 100.0_f64.hypot(100.0));
    }

    #[test]
    fn screen_hits_are_deterministic() {
        let vertex = screen_vertex_hit(
            [10.0, 10.0],
            &[
                ScreenVertex {
                    index: 3,
                    point: [11.0, 10.0],
                },
                ScreenVertex {
                    index: 1,
                    point: [9.0, 10.0],
                },
            ],
            2.0,
        )
        .unwrap();
        assert_eq!(
            vertex,
            InteractionResult::VertexHit {
                index: Some(1),
                distance_px: Some(1.0),
            }
        );

        let segment =
            screen_segment_hit([5.0, 2.0], &[vec![[0.0, 0.0], [10.0, 0.0]]], Some(3.0)).unwrap();
        assert_eq!(
            segment,
            InteractionResult::SegmentHit {
                part_index: Some(0),
                segment_index: Some(0),
                distance_px: Some(2.0),
            }
        );
    }
}
