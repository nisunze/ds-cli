//! Measured text and host collation enter the permanent Canvas label owner.
//! Model labels stay verbatim; this adapter only returns screen placements.
use crate::model::Screen;
use ds_canvas::profile_label_layout::{self as core, MeasuredLabel, OrderRequest};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

pub use core::{Align, Baseline, Bounds, Options, Orientation, Placement};

/// Measured station labels reserve the exact global ends before alignment
/// boundaries and ordinary ticks. Only text is suppressed; ticks stay native.
pub fn station_rows(candidates: &[(f64, f64, bool)]) -> Vec<(usize, usize)> {
    if candidates.is_empty() {
        return vec![];
    }
    let (mut leftmost, mut rightmost) = (0, 0);
    for i in 1..candidates.len() {
        if candidates[i].0 < candidates[leftmost].0 {
            leftmost = i;
        }
        if candidates[i].0 > candidates[rightmost].0 {
            rightmost = i;
        }
    }
    let priority = |i: usize| {
        if i == leftmost || i == rightmost {
            2
        } else {
            u8::from(candidates[i].2)
        }
    };
    let mut ordered: Vec<_> = (0..candidates.len()).collect();
    ordered.sort_by(|&a, &b| {
        priority(b)
            .cmp(&priority(a))
            .then(candidates[a].0.total_cmp(&candidates[b].0))
    });
    let mut occupied = [Vec::<(f64, f64)>::new(), Vec::new()];
    let mut placed = Vec::new();
    for i in ordered {
        let (x, width, _) = candidates[i];
        let half = width.max(0.) / 2. + 3.;
        let interval = (x - half, x + half);
        if let Some(row) = occupied.iter().position(|row| {
            row.iter()
                .all(|&(left, right)| interval.1 <= left || interval.0 >= right)
        }) {
            occupied[row].push(interval);
            placed.push((i, row));
        }
    }
    placed.sort_by(|&(a, ar), &(b, br)| {
        candidates[a]
            .0
            .total_cmp(&candidates[b].0)
            .then(ar.cmp(&br))
    });
    placed
}

pub fn station_text(metres: f64) -> String {
    let metres = (metres.max(0.) * 10.).round() / 10.;
    let kilometres = (metres / 1000.).floor();
    format!("{kilometres:.0}+{:05.1}", metres - kilometres * 1000.)
}

#[derive(Clone, Debug)]
pub struct LabelRequest {
    pub id: String,
    pub label: String,
    pub anchor: Screen,
    pub orientation: Orientation,
    pub priority: f64,
    pub required: bool,
}

#[derive(Clone, Debug)]
pub struct PlacedLabel {
    pub input_index: usize,
    pub lines: Vec<String>,
    pub placement: Placement,
    pub bounds: Bounds,
}

pub fn options(width: f64, height: f64) -> Options {
    Options {
        width,
        height,
        line_height: 13.,
        font_height: 11.,
        padding: 3.,
        margin: 4.,
        cell_size: 64.,
    }
}

pub fn orientation(value: &str) -> Option<Orientation> {
    Some(match value {
        "auto" => Orientation::Auto,
        "right" => Orientation::Right,
        "left" => Orientation::Left,
        "above" => Orientation::Above,
        "below" => Orientation::Below,
        "vertical" => Orientation::Vertical,
        _ => return None,
    })
}

/// `measure` receives the original request index so a painter can apply its
/// exact font first. `collate` is the host's locale service, not byte ordering.
pub fn layout_labels(
    requests: &[LabelRequest],
    options: Options,
    mut measure: impl FnMut(usize, &str) -> f64,
    mut collate: impl FnMut(&str, &str) -> Ordering,
) -> Vec<PlacedLabel> {
    let mut seen = BTreeSet::new();
    let mut ids: Vec<_> = requests
        .iter()
        .map(|r| r.id.as_str())
        .filter(|id| seen.insert(*id))
        .collect();
    ids.sort_by(|a, b| collate(a, b));
    let mut ranks = BTreeMap::new();
    let mut rank = 0;
    for (i, id) in ids.iter().enumerate() {
        if i > 0 && collate(ids[i - 1], id) != Ordering::Equal {
            rank += 1;
        }
        ranks.insert(*id, rank);
    }
    let order: Vec<_> = requests
        .iter()
        .map(|r| OrderRequest {
            anchor: core::Point {
                x: r.anchor.x,
                y: r.anchor.y,
            },
            priority: r.priority,
            collation_rank: ranks[r.id.as_str()],
            enabled: !r.label.is_empty(),
        })
        .collect();
    let mut lines = BTreeMap::new();
    let mut measured = Vec::new();
    for index in core::label_order(&order, options.width, options.height) {
        let request = &requests[index];
        let text: Vec<_> = request
            .label
            .split('\n')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if text.is_empty() {
            continue;
        }
        let text_width =
            text.iter()
                .map(|line| measure(index, line))
                .fold(f64::NEG_INFINITY, |a, b| {
                    if a.is_nan() || b.is_nan() {
                        f64::NAN
                    } else {
                        a.max(b)
                    }
                });
        measured.push(MeasuredLabel {
            input_index: index,
            anchor: core::Point {
                x: request.anchor.x,
                y: request.anchor.y,
            },
            orientation: request.orientation,
            required: request.required,
            text_width,
            line_count: text.len(),
        });
        lines.insert(index, text);
    }
    core::layout_labels(&measured, options)
        .into_iter()
        .map(|placed| PlacedLabel {
            input_index: placed.input_index,
            lines: lines
                .remove(&placed.input_index)
                .expect("placed label was measured"),
            placement: placed.placement,
            bounds: placed.bounds,
        })
        .collect()
}
