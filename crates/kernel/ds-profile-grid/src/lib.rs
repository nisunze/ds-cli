//! Portable profile drawing vocabulary and bounded 1/2/5 grid positions.
//! No model, revision, terrain, clearance or engineering calculation.

use serde::{Deserialize, Serialize};

pub const PROFILE_HORIZONTAL_SCENE_UNITS_PER_M: f64 = 0.5;
pub const PROFILE_ALIGNMENT_GAP_M: f64 = 100.0;
pub const PROFILE_PLOT_LEFT: f64 = 108.0;
pub const PROFILE_RIGHT_MARGIN: f64 = 36.0;
pub const PROFILE_PLOT_TOP: f64 = 34.0;
pub const PROFILE_BOTTOM_MARGIN: f64 = 72.0;
pub const PROFILE_MIN_PLOT_HEIGHT: f64 = 120.0;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileControlRole {
    Start,
    End,
    AnglePoint,
    Tap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileStroke {
    pub color: String,
    pub width_px: f64,
    pub dash_px: Vec<f64>,
}

impl ProfileStroke {
    fn new(color: &str, width_px: f64, dash_px: &[f64]) -> Self {
        Self {
            color: color.to_string(),
            width_px,
            dash_px: dash_px.to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileOrdinateStyles {
    pub start: ProfileStroke,
    pub end: ProfileStroke,
    pub angle_point: ProfileStroke,
    pub tap: ProfileStroke,
}

impl ProfileOrdinateStyles {
    pub fn for_role(&self, role: ProfileControlRole) -> &ProfileStroke {
        match role {
            ProfileControlRole::Start => &self.start,
            ProfileControlRole::End => &self.end,
            ProfileControlRole::AnglePoint => &self.angle_point,
            ProfileControlRole::Tap => &self.tap,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileFoundation {
    pub station_axis: String,
    pub station_grid: ProfileStroke,
    pub alignment_boundary_grid: ProfileStroke,
    pub elevation_grid: ProfileStroke,
    pub plot_boundary: ProfileStroke,
    pub ordinates: ProfileOrdinateStyles,
}

impl Default for ProfileFoundation {
    fn default() -> Self {
        Self {
            station_axis: "virtual_chain_station_m".to_string(),
            station_grid: ProfileStroke::new("#1e293b", 0.7, &[]),
            alignment_boundary_grid: ProfileStroke::new("#334155", 1.0, &[]),
            elevation_grid: ProfileStroke::new("#1e293b", 0.7, &[]),
            plot_boundary: ProfileStroke::new("#475569", 1.0, &[]),
            ordinates: ProfileOrdinateStyles {
                start: ProfileStroke::new("rgba(34, 197, 94, 0.82)", 1.1, &[3.0, 4.0]),
                end: ProfileStroke::new("rgba(239, 68, 68, 0.82)", 1.1, &[3.0, 4.0]),
                angle_point: ProfileStroke::new("rgba(14, 165, 233, 0.80)", 1.1, &[3.0, 4.0]),
                tap: ProfileStroke::new("rgba(249, 115, 22, 0.92)", 1.4, &[6.0, 3.0]),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProfileStationAxis {
    pub minimum_chain_station_m: f64,
    pub maximum_chain_station_m: f64,
    pub minimum_scene_x: f64,
    pub maximum_scene_x: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileStationGridTick {
    pub alignment_id: String,
    pub value_m: f64,
    pub scene_x: f64,
    pub boundary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileElevationGridTick {
    pub value_m: f64,
    pub scene_y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileStationGridLevel {
    pub step_m: f64,
    pub ticks: Vec<ProfileStationGridTick>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileElevationGridLevel {
    pub step_m: f64,
    pub ticks: Vec<ProfileElevationGridTick>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileEngineeringGrid {
    pub target_screen_spacing_px: f64,
    pub station_levels: Vec<ProfileStationGridLevel>,
    pub elevation_levels: Vec<ProfileElevationGridLevel>,
}

fn nice_step(minimum: f64) -> f64 {
    if !minimum.is_finite() || minimum <= 0.0 {
        return 1.0;
    }
    let power = 10_f64.powf(minimum.log10().floor());
    let normalized = minimum / power;
    let factor = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    factor * power
}

fn candidate_steps(minimum: f64, maximum: f64) -> Vec<f64> {
    let mut steps = Vec::new();
    let mut step = nice_step(minimum);
    for _ in 0..64 {
        if steps
            .last()
            .is_none_or(|last: &f64| (*last - step).abs() > 1e-12)
        {
            steps.push(step);
        }
        if step > maximum.max(minimum) * 10.0 {
            break;
        }
        step = nice_step(step * 1.01);
    }
    steps
}

fn regular_values(minimum: f64, maximum: f64, step: f64) -> Vec<f64> {
    let first = (minimum / step).ceil() * step;
    let mut values = Vec::new();
    let mut value = first;
    while value <= maximum + step * 1e-9 && values.len() < 96 {
        values.push(value);
        value += step;
    }
    values
}

/// Precompute every bounded 1/2/5 engineering-grid level. A renderer selects
/// the level appropriate to its camera scale; it never calculates station
/// positions, crosses an atlas gap, or changes a Rust-selected interval.
pub fn build_engineering_grid(
    axes: &[(String, ProfileStationAxis)],
    domain_min_m: f64,
    domain_max_m: f64,
    vertical_scene_units_per_m: f64,
) -> ProfileEngineeringGrid {
    let valid_axes = axes
        .iter()
        .filter(|(_, axis)| {
            axis.minimum_chain_station_m.is_finite()
                && axis.maximum_chain_station_m.is_finite()
                && axis.minimum_scene_x.is_finite()
                && axis.maximum_scene_x.is_finite()
                && axis.maximum_chain_station_m - axis.minimum_chain_station_m > 1e-9
                && axis.maximum_scene_x - axis.minimum_scene_x > 1e-9
        })
        .collect::<Vec<_>>();
    let station_span = valid_axes
        .iter()
        .map(|(_, axis)| axis.maximum_chain_station_m - axis.minimum_chain_station_m)
        .sum::<f64>()
        .max(1.0);
    let boundary_budget = valid_axes.len().saturating_mul(2).min(94);
    let station_minimum_step = station_span / (96 - boundary_budget).max(2) as f64;
    let station_levels = if valid_axes.is_empty() {
        Vec::new()
    } else {
        candidate_steps(station_minimum_step, station_span)
            .into_iter()
            .filter_map(|step_m| {
                let mut ticks = Vec::new();
                for (alignment_id, axis) in &valid_axes {
                    let span_m = axis.maximum_chain_station_m - axis.minimum_chain_station_m;
                    let span_scene = axis.maximum_scene_x - axis.minimum_scene_x;
                    let mut values = regular_values(
                        axis.minimum_chain_station_m,
                        axis.maximum_chain_station_m,
                        step_m,
                    );
                    values.push(axis.minimum_chain_station_m);
                    values.push(axis.maximum_chain_station_m);
                    values.sort_by(f64::total_cmp);
                    values.dedup_by(|left, right| (*left - *right).abs() < 1e-7);
                    ticks.extend(values.into_iter().map(|value_m| ProfileStationGridTick {
                        alignment_id: (*alignment_id).clone(),
                        value_m,
                        scene_x: axis.minimum_scene_x
                            + (value_m - axis.minimum_chain_station_m) / span_m * span_scene,
                        boundary: (value_m - axis.minimum_chain_station_m).abs() < 1e-7
                            || (value_m - axis.maximum_chain_station_m).abs() < 1e-7,
                    }));
                }
                (ticks.len() <= 96).then_some(ProfileStationGridLevel { step_m, ticks })
            })
            .collect()
    };

    let elevation_span = (domain_max_m - domain_min_m).max(1.0);
    let elevation_levels = candidate_steps(elevation_span / 96.0, elevation_span)
        .into_iter()
        .filter_map(|step_m| {
            let ticks = regular_values(domain_min_m, domain_max_m, step_m)
                .into_iter()
                .map(|value_m| ProfileElevationGridTick {
                    value_m,
                    scene_y: PROFILE_PLOT_TOP
                        + (domain_max_m - value_m) * vertical_scene_units_per_m,
                })
                .collect::<Vec<_>>();
            (ticks.len() <= 96).then_some(ProfileElevationGridLevel { step_m, ticks })
        })
        .collect();
    ProfileEngineeringGrid {
        target_screen_spacing_px: 72.0,
        station_levels,
        elevation_levels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_levels_keep_exact_boundaries_and_never_tick_the_alignment_gap() {
        let axes = vec![
            (
                "main".to_string(),
                ProfileStationAxis {
                    minimum_chain_station_m: 0.0,
                    maximum_chain_station_m: 2_000.0,
                    minimum_scene_x: 108.0,
                    maximum_scene_x: 1_108.0,
                },
            ),
            (
                "branch".to_string(),
                ProfileStationAxis {
                    minimum_chain_station_m: 2_100.0,
                    maximum_chain_station_m: 5_100.0,
                    minimum_scene_x: 1_158.0,
                    maximum_scene_x: 2_658.0,
                },
            ),
        ];
        let grid = build_engineering_grid(&axes, 1_380.0, 1_430.0, 5.0);
        assert!(!grid.station_levels.is_empty());
        for level in &grid.station_levels {
            assert!(level.ticks.len() <= 96);
            assert!(
                level
                    .ticks
                    .iter()
                    .any(|tick| tick.value_m == 0.0 && tick.boundary)
            );
            assert!(
                level
                    .ticks
                    .iter()
                    .any(|tick| tick.value_m == 5_100.0 && tick.boundary)
            );
            assert!(
                !level
                    .ticks
                    .iter()
                    .any(|tick| tick.scene_x > 1_108.0 && tick.scene_x < 1_158.0)
            );
        }
    }
}
