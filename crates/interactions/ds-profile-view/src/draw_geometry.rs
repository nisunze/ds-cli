//! Display annotations over the engine's already projected profile scene.
//! These helpers never solve a conductor or change authored engineering facts.
use crate::model::{Junction, Point, Scene, Screen};
use std::collections::{BTreeSet, HashMap};

pub const JUNCTION_HINT_LENGTH_PX: f64 = 12.;
pub const JUNCTION_HINT_HEAD_PX: f64 = 3.;

/// Place an authored span annotation between the already projected offset
/// guides. Gaps retain the authored boundary; no new curve is interpolated.
pub fn span_label_anchor(scene: &Scene, authored: Point) -> Point {
    let (mut ground, mut wire) = (None, None);
    for guide in &scene.clearance_guides {
        if !matches!(guide.kind.as_str(), "ground_offset" | "lowest_wire_drop") {
            continue;
        }
        let y = guide.segments.iter().find_map(|segment| {
            segment.points.windows(2).find_map(|edge| {
                let (a, b) = (edge[0], edge[1]);
                let x = authored.scene_x;
                if x < a.scene_x.min(b.scene_x) || x > a.scene_x.max(b.scene_x) {
                    return None;
                }
                let dx = b.scene_x - a.scene_x;
                Some(if dx.abs() < 1e-12 {
                    (a.scene_y + b.scene_y) / 2.
                } else {
                    a.scene_y + (x - a.scene_x) / dx * (b.scene_y - a.scene_y)
                })
            })
        });
        if y.is_some() {
            match guide.kind.as_str() {
                "ground_offset" => ground = y,
                "lowest_wire_drop" => wire = y,
                _ => unreachable!(),
            }
        }
    }
    Point {
        scene_y: (ground.unwrap_or(authored.scene_y) + wire.unwrap_or(authored.scene_y)) / 2.,
        ..authored
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ScreenArrow {
    pub start: Screen,
    pub end: Screen,
    pub head: [Screen; 3],
}

/// Keep the annotation's apparent size fixed, including when its authored
/// direction has zero length. Call this after projecting both endpoints.
pub fn junction_screen_arrow(start: Screen, authored_end: Screen) -> ScreenArrow {
    let dx = authored_end.x - start.x;
    let dy = authored_end.y - start.y;
    let length = dx.hypot(dy);
    let (ux, uy) = if length > 0. {
        (dx / length, dy / length)
    } else {
        (1., 0.)
    };
    let end = Screen {
        x: start.x + ux * JUNCTION_HINT_LENGTH_PX,
        y: start.y + uy * JUNCTION_HINT_LENGTH_PX,
    };
    let base = Screen {
        x: end.x - ux * JUNCTION_HINT_HEAD_PX,
        y: end.y - uy * JUNCTION_HINT_HEAD_PX,
    };
    ScreenArrow {
        start,
        end,
        head: [
            end,
            Screen {
                x: base.x - uy * JUNCTION_HINT_HEAD_PX,
                y: base.y + ux * JUNCTION_HINT_HEAD_PX,
            },
            Screen {
                x: base.x + uy * JUNCTION_HINT_HEAD_PX,
                y: base.y - ux * JUNCTION_HINT_HEAD_PX,
            },
        ],
    }
}

/// A junction's physical support may appear at the parent handoff and at the
/// start of its child band. Resolve only the explicit alignment/support pair.
pub fn junction_scene_starts(scene: &Scene, hint: &Junction) -> Vec<Point> {
    let authored = hint.direction_arrow.shaft_start;
    let mut starts = vec![Point::xy(authored.scene_x, authored.scene_y)];
    let child = scene
        .bands
        .iter()
        .find(|band| band.alignment_id == hint.child_alignment_id)
        .and_then(|band| {
            band.structures
                .iter()
                .find(|support| support.entity_id == hint.junction_structure_id)
        });
    if let Some(child) = child {
        if (child.ground.scene_x - authored.scene_x).abs() > 1e-6 {
            starts.push(Point::xy(child.ground.scene_x, authored.scene_y));
        }
    }
    starts
}

struct Sample {
    point: Point,
    spans: BTreeSet<usize>,
}

fn sample_key(point: Point) -> String {
    format!(
        "{}:{}",
        fixed_six_identity(point.chain_station_m),
        fixed_six_identity(point.scene_x)
    )
}

fn fixed_six_identity(value: f64) -> String {
    if !value.is_finite() || value.abs() >= 1e21 {
        return format!("wide:{value}");
    }
    // Match toFixed(6)'s nearest decimal integer, with exact ties away from
    // zero. Formatting alone uses ties-to-even; multiplying f64 by 1e6 can
    // manufacture a tie. The binary significand keeps this station key exact.
    let bits = value.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = bits & ((1u64 << 52) - 1);
    let (mantissa, power) = if exponent == 0 {
        (mantissa, -1074)
    } else {
        (mantissa | (1u64 << 52), exponent - 1075)
    };
    let scaled = u128::from(mantissa) * 1_000_000;
    let rounded = if power >= 0 {
        scaled << power as u32
    } else {
        let shift = (-power) as u32;
        if shift >= 128 {
            0
        } else {
            let whole = scaled >> shift;
            let remainder = scaled - (whole << shift);
            whole + u128::from(remainder >= (1u128 << (shift - 1)))
        }
    };
    format!("{}{rounded}", if value < 0. { "-" } else { "" })
}

fn finish_segment(current: &mut Vec<Point>, segments: &mut Vec<Vec<Point>>) {
    if current.len() >= 2 {
        // These samples were ordered by scene X, so the painter may safely
        // trim solid paths without sorting or inspecting every sample again.
        current[0].path_order = 1;
        segments.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

/// Offset the lowest existing samples at each published profile station.
/// Adjacent samples join only if they share an actual span occurrence, so
/// independent stringing runs and cross-alignment handoffs cannot create chords.
pub fn lowest_wire_drop(scene: &Scene, metres: f64) -> Vec<Vec<Point>> {
    if !metres.is_finite() || metres <= 0. {
        return vec![];
    }
    let mut by_key = HashMap::<String, usize>::new();
    let mut samples = Vec::<Sample>::new();
    for (span_index, span) in scene.conductor_spans.iter().enumerate() {
        if span.cross_alignment {
            continue;
        }
        for &point in &span.points {
            let key = sample_key(point);
            if let Some(&index) = by_key.get(&key) {
                let sample = &mut samples[index];
                sample.spans.insert(span_index);
                if point.elevation_m < sample.point.elevation_m {
                    sample.point = point;
                }
            } else {
                by_key.insert(key, samples.len());
                samples.push(Sample {
                    point,
                    spans: BTreeSet::from([span_index]),
                });
            }
        }
    }
    samples.sort_by(|a, b| {
        a.point
            .scene_x
            .total_cmp(&b.point.scene_x)
            .then(a.point.scene_y.total_cmp(&b.point.scene_y))
    });
    let mut segments = Vec::new();
    let mut current = Vec::new();
    let mut previous: Option<&Sample> = None;
    for sample in &samples {
        if previous.is_some_and(|p| p.spans.is_disjoint(&sample.spans)) {
            finish_segment(&mut current, &mut segments);
        }
        current.push(Point {
            elevation_m: sample.point.elevation_m - metres,
            scene_y: sample.point.scene_y + metres * scene.scale.vertical_scene_units_per_m,
            ..sample.point
        });
        previous = Some(sample);
    }
    finish_segment(&mut current, &mut segments);
    segments
}
