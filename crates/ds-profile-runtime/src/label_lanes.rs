//! Bounded interval lookup for native label lanes; never scan terrain per span.
use ds_grid_engine::profile_atlas::ProfileAtlasPoint;
#[derive(Clone)]
struct Edge {
    a: ProfileAtlasPoint,
    b: ProfileAtlasPoint,
    order: usize,
    min: f64,
    max: f64,
}
pub struct Lanes {
    edges: Vec<Edge>,
    prefix_max: Vec<f64>,
}
impl Lanes {
    pub fn new<'a>(segments: impl Iterator<Item = &'a [ProfileAtlasPoint]>) -> Self {
        let mut edges = Vec::new();
        for points in segments {
            for pair in points.windows(2) {
                edges.push(Edge {
                    a: pair[0],
                    b: pair[1],
                    order: edges.len(),
                    min: pair[0].scene_x.min(pair[1].scene_x),
                    max: pair[0].scene_x.max(pair[1].scene_x),
                });
            }
        }
        edges.sort_by(|a, b| a.min.total_cmp(&b.min));
        let mut high = f64::NEG_INFINITY;
        let prefix_max = edges
            .iter()
            .map(|edge| {
                high = high.max(edge.max);
                high
            })
            .collect();
        Self { edges, prefix_max }
    }
    pub fn y(&self, x: f64) -> Option<f64> {
        let mut index = self.edges.partition_point(|edge| edge.min <= x);
        let mut selected: Option<&Edge> = None;
        while index > 0 && self.prefix_max[index - 1] >= x {
            index -= 1;
            let edge = &self.edges[index];
            if edge.max >= x && selected.is_none_or(|old| edge.order < old.order) {
                selected = Some(edge);
            }
        }
        selected.map(|edge| {
            let width = edge.b.scene_x - edge.a.scene_x;
            if width.abs() < 1e-12 {
                return (edge.a.scene_y + edge.b.scene_y) / 2.0;
            }
            edge.a.scene_y + (x - edge.a.scene_x) / width * (edge.b.scene_y - edge.a.scene_y)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn point(x: f64) -> ProfileAtlasPoint {
        ProfileAtlasPoint {
            scene_x: x,
            scene_y: x,
            station_m: x,
            chain_station_m: x,
            elevation_m: x,
        }
    }
    #[test]
    fn interpolation_preserves_gaps_and_the_source_order() {
        let first = vec![point(0.0), point(10.0)];
        let second = vec![point(20.0), point(30.0)];
        let lanes = Lanes::new([first.as_slice(), second.as_slice()].into_iter());
        assert_eq!(lanes.y(5.0), Some(5.0));
        assert_eq!(lanes.y(15.0), None);
        assert_eq!(lanes.y(25.0), Some(25.0));
    }
}
