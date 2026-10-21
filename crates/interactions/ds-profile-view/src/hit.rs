//! One index per immutable scene. Only nearby scene-space geometry is visited.
use crate::{camera::Camera, model::*};
#[derive(Clone)]
struct Segment {
    a: Point,
    b: Point,
    id: String,
    kind: &'static str,
    attachment: Option<AttachmentRef>,
    min_x: f64,
    max_x: f64,
}
#[derive(Default)]
pub struct HitIndex {
    segments: Vec<Segment>,
    prefix_max: Vec<f64>,
}
impl HitIndex {
    pub fn new(scene: &Scene) -> Self {
        let mut segments = Vec::new();
        let mut add = |a: Point, b: Point, id: String, kind, attachment| {
            segments.push(Segment {
                a,
                b,
                id,
                kind,
                attachment,
                min_x: a.scene_x.min(b.scene_x),
                max_x: a.scene_x.max(b.scene_x),
            })
        };
        for band in &scene.bands {
            for g in &band.ground {
                for p in g.points.windows(2) {
                    add(p[0], p[1], band.alignment_id.clone(), "alignment", None);
                }
            }
            for side in &band.side_profiles {
                for segment in &side.segments {
                    for p in segment.points.windows(2) {
                        add(p[0], p[1], band.alignment_id.clone(), "alignment", None);
                    }
                }
            }
            for s in &band.structures {
                add(
                    s.base,
                    s.top.unwrap_or(s.ground),
                    s.entity_id.clone(),
                    "structure",
                    None,
                );
                for a in &s.attachment_points {
                    let r = AttachmentRef {
                        structure_id: s.entity_id.clone(),
                        attachment_point_id: a.attachment_point_id.clone(),
                    };
                    add(a.point, a.point, r.key(), "attachment_point", Some(r));
                }
            }
            for p in &band.terrain_points {
                add(p.point, p.point, p.entity_id.clone(), "ground_point", None);
            }
        }
        for s in &scene.conductor_spans {
            if !s.cross_alignment && scene.current() {
                for p in s.points.windows(2) {
                    add(p[0], p[1], s.section_id.clone(), "tension_section", None);
                }
            }
        }
        segments.sort_by(|a, b| a.min_x.total_cmp(&b.min_x));
        let mut max = f64::NEG_INFINITY;
        let prefix_max = segments
            .iter()
            .map(|s| {
                max = max.max(s.max_x);
                max
            })
            .collect();
        Self {
            segments,
            prefix_max,
        }
    }
    pub fn pick(
        &self,
        camera: &Camera,
        screen: Screen,
        v: &Visibility,
        tolerance_px: f64,
    ) -> Vec<Candidate> {
        let p = camera.unproject(screen);
        let scale = camera.projection().scale;
        let radius = tolerance_px / scale;
        let end = self
            .segments
            .partition_point(|s| s.min_x <= p.scene_x + radius);
        let start = self.prefix_max[..end].partition_point(|x| *x < p.scene_x - radius);
        let mut hits = std::collections::BTreeMap::<String, Candidate>::new();
        for s in &self.segments[start..end] {
            if s.max_x < p.scene_x - radius
                || p.scene_y + radius < s.a.scene_y.min(s.b.scene_y)
                || p.scene_y - radius > s.a.scene_y.max(s.b.scene_y)
            {
                continue;
            }
            let shown = match s.kind {
                "structure" => v.show_structures,
                "attachment_point" => v.show_attachments && v.show_structures,
                "ground_point" => v.show_terrain_points,
                "tension_section" => v.show_wire,
                _ => v.show_ground,
            };
            if !shown {
                continue;
            }
            let d = distance(p, s.a, s.b);
            if d > radius {
                continue;
            }
            let hit = Candidate {
                entity_id: s.id.clone(),
                kind: s.kind.into(),
                distance_scene_units: d,
                distance_px: d * scale,
                attachment_ref: s.attachment.clone(),
            };
            if hits.get(&s.id).is_none_or(|h| d < h.distance_scene_units) {
                hits.insert(s.id.clone(), hit);
            }
        }
        let mut hits: Vec<_> = hits.into_values().collect();
        let priority = |kind: &str| match kind {
            "attachment_point" => 0,
            "ground_point" => 1,
            "structure" => 2,
            "tension_section" => 3,
            _ => 4,
        };
        hits.sort_by(|a, b| {
            (a.distance_px + priority(&a.kind) as f64 * 0.5)
                .total_cmp(&(b.distance_px + priority(&b.kind) as f64 * 0.5))
                .then(a.entity_id.cmp(&b.entity_id))
        });
        hits
    }
}
pub fn distance(p: Point, a: Point, b: Point) -> f64 {
    let dx = b.scene_x - a.scene_x;
    let dy = b.scene_y - a.scene_y;
    let n = dx * dx + dy * dy;
    let t = if n > 0. {
        ((p.scene_x - a.scene_x) * dx + (p.scene_y - a.scene_y) * dy) / n
    } else {
        0.
    }
    .clamp(0., 1.);
    (p.scene_x - a.scene_x - t * dx).hypot(p.scene_y - a.scene_y - t * dy)
}
pub fn contains(rect: [f64; 4], anchor: Screen, p: Screen) -> bool {
    p.x >= anchor.x + rect[0]
        && p.x <= anchor.x + rect[0] + rect[2]
        && p.y >= anchor.y + rect[1]
        && p.y <= anchor.y + rect[1] + rect[3]
}
