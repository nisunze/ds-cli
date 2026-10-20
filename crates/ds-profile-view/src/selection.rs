use crate::model::*;
#[derive(Clone, Copy, Default, Debug)]
pub struct Modifiers {
    pub toggle: bool,
    pub range: bool,
    pub cycle: bool,
}
pub fn range(scene: &Scene, a: &str, b: &str) -> Vec<String> {
    let attachments: Vec<_> = scene
        .selection_order
        .attachments
        .iter()
        .map(AttachmentRef::key)
        .collect();
    let alignments: Vec<_> = scene.bands.iter().map(|b| b.alignment_id.clone()).collect();
    for order in [
        &scene.selection_order.structures,
        &scene.selection_order.terrain_points,
        &scene.selection_order.tension_sections,
        &attachments,
        &alignments,
    ] {
        if let (Some(a), Some(b)) = (
            order.iter().position(|x| x == a),
            order.iter().position(|x| x == b),
        ) {
            return order[a.min(b)..=a.max(b)].to_vec();
        }
    }
    vec![b.into()]
}
pub fn choose(
    existing: &[String],
    picked: Option<&str>,
    expanded: Option<&[String]>,
    modifiers: Modifiers,
) -> (Vec<String>, Option<String>) {
    let Some(id) = picked else {
        return if modifiers.toggle || modifiers.range || modifiers.cycle {
            (existing.to_vec(), existing.last().cloned())
        } else {
            (vec![], None)
        };
    };
    let mut ids = if modifiers.toggle {
        existing.to_vec()
    } else {
        vec![]
    };
    if let Some(range) = expanded {
        for id in range {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
    } else if modifiers.toggle && ids.iter().any(|x| x == id) {
        ids.retain(|x| x != id);
    } else {
        ids.push(id.into());
    }
    let primary = if ids.iter().any(|x| x == id) {
        Some(id.into())
    } else {
        ids.last().cloned()
    };
    (ids, primary)
}
pub fn station_candidate(start: f64, dx: f64, scale: f64, axis: &StationAxis) -> Option<f64> {
    let w = axis.maximum_scene_x - axis.minimum_scene_x;
    let length = axis.maximum_chain_station_m - axis.minimum_chain_station_m;
    if ![start, dx, scale, w, length].iter().all(|x| x.is_finite())
        || scale <= 0.
        || w <= 0.
        || length <= 0.
    {
        None
    } else {
        Some(start + dx / scale * length / w)
    }
}
pub fn ground_candidate(start: f64, dy: f64, scale: f64, axis: &Axis) -> Option<(f64, bool)> {
    let units = scale * axis.vertical_px_per_m;
    if ![start, dy, units, axis.domain_min_m, axis.domain_max_m]
        .iter()
        .all(|x| x.is_finite())
        || units <= 0.
        || axis.domain_min_m > axis.domain_max_m
    {
        return None;
    }
    let target = start - dy / units;
    let bounded = target.clamp(axis.domain_min_m, axis.domain_max_m);
    Some((bounded, bounded != target))
}
