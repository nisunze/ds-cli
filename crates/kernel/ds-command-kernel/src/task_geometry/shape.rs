//! Shaping — the resolved objects become the task's ONE geometry.
//!
//! Lightweight geometry, on the kernel side of the boundary
//! (`docs/contracts/ds-kernel-network-boundary.md`): a convex hull, a metric
//! buffer and a station clip. The metric work is done in a local
//! equirectangular frame about the centroid on the WGS84 sphere — for a few
//! hundred metres around three poles the error is millimetres, and it keeps
//! the whole rule free of a projection library. Every function here is pure
//! and deterministic: the same objects give the same ring, byte for byte.

use ds_geo_lite::EARTH_RADIUS_M;

/// A WGS84 position, `[lon, lat]`.
pub type Position = [f64; 2];

/// How many points approximate the circle around each hull vertex. Sixteen
/// keeps a 25 m buffer within 0.5 m of a true circle and a three-pole area
/// under sixty positions.
pub const ARC_POINTS: usize = 16;

/// Coordinates are answered to 7 decimals — about a centimetre — so a receipt
/// is readable and a re-run compares equal.
pub const DECIMALS: i32 = 7;

/// A local metric frame: metres east/north of an origin on the sphere.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    origin: Position,
    cos_lat: f64,
}

impl Frame {
    /// The frame about the mean of `positions`. Callers never pass an empty
    /// list; an empty one gets the equator, which is harmless and unreachable.
    pub fn about(positions: &[Position]) -> Self {
        let n = positions.len().max(1) as f64;
        let lon = positions.iter().map(|p| p[0]).sum::<f64>() / n;
        let lat = positions.iter().map(|p| p[1]).sum::<f64>() / n;
        Self {
            origin: [lon, lat],
            cos_lat: lat.to_radians().cos().max(1e-9),
        }
    }

    pub fn to_metres(&self, position: Position) -> [f64; 2] {
        let dlon = (position[0] - self.origin[0]).to_radians();
        let dlat = (position[1] - self.origin[1]).to_radians();
        [dlon * self.cos_lat * EARTH_RADIUS_M, dlat * EARTH_RADIUS_M]
    }

    pub fn to_position(&self, metres: [f64; 2]) -> Position {
        let lon = self.origin[0] + (metres[0] / (self.cos_lat * EARTH_RADIUS_M)).to_degrees();
        let lat = self.origin[1] + (metres[1] / EARTH_RADIUS_M).to_degrees();
        [round(lon), round(lat)]
    }
}

pub fn round(value: f64) -> f64 {
    let scale = 10f64.powi(DECIMALS);
    (value * scale).round() / scale
}

fn cross(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Andrew's monotone chain: the convex hull, counter-clockwise, without the
/// closing point. One point gives one point, two give two; collinear and
/// duplicate points fall out.
pub fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut sorted: Vec<[f64; 2]> = points.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }
    let mut lower: Vec<[f64; 2]> = Vec::new();
    for &p in &sorted {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<[f64; 2]> = Vec::new();
    for &p in sorted.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The convex hull of `positions`, buffered by `buffer_m`, as one closed
/// counter-clockwise ring in WGS84: the hull of an `ARC_POINTS`-gon of radius
/// `buffer_m` about every hull vertex. Two points give a capsule, three a
/// rounded triangle — a "swamp crossing at 74/76/77" becomes a visible area.
pub fn buffered_hull(positions: &[Position], buffer_m: f64) -> Vec<Position> {
    let frame = Frame::about(positions);
    let metres: Vec<[f64; 2]> = positions.iter().map(|&p| frame.to_metres(p)).collect();
    let mut cloud: Vec<[f64; 2]> = Vec::with_capacity(metres.len() * ARC_POINTS);
    for vertex in convex_hull(&metres) {
        for step in 0..ARC_POINTS {
            let angle = std::f64::consts::TAU * step as f64 / ARC_POINTS as f64;
            cloud.push([
                vertex[0] + buffer_m * angle.cos(),
                vertex[1] + buffer_m * angle.sin(),
            ]);
        }
    }
    let mut ring: Vec<Position> = convex_hull(&cloud)
        .into_iter()
        .map(|m| frame.to_position(m))
        .collect();
    // Rounding to a centimetre can fold two arc points onto one another;
    // a ring never repeats a position except to close.
    ring.dedup();
    if let Some(&first) = ring.first() {
        ring.push(first);
    }
    ring
}

/// One vertex of an alignment: where it is and how far along the route it
/// sits, in the engine's own chainage (metres from the first node).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: Position,
    pub station_m: f64,
}

/// The stretch of a polyline between two stations, with both ends placed
/// EXACTLY at the two given positions (the structures'), never at an
/// interpolation of them. Vertices strictly inside the range keep the
/// route's bends. The order of the two ends is by station, whichever way the
/// caller wrote them.
pub fn clip_by_station(
    vertices: &[Vertex],
    from: (Position, f64),
    to: (Position, f64),
) -> Vec<Position> {
    let (a, b) = if from.1 <= to.1 {
        (from, to)
    } else {
        (to, from)
    };
    let mut line = vec![[round(a.0[0]), round(a.0[1])]];
    for vertex in vertices {
        if vertex.station_m > a.1 && vertex.station_m < b.1 {
            line.push([round(vertex.position[0]), round(vertex.position[1])]);
        }
    }
    line.push([round(b.0[0]), round(b.0[1])]);
    line.dedup();
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three poles about 60 m apart near Nyamagabe, as a consultant would
    /// see them on the map.
    fn swamp() -> Vec<Position> {
        vec![
            [29.6112000, -2.3412000],
            [29.6117000, -2.3413000],
            [29.6114000, -2.3417500],
        ]
    }

    fn metres_between(frame: &Frame, a: Position, b: Position) -> f64 {
        let pa = frame.to_metres(a);
        let pb = frame.to_metres(b);
        ((pa[0] - pb[0]).powi(2) + (pa[1] - pb[1]).powi(2)).sqrt()
    }

    #[test]
    fn the_frame_round_trips_to_the_centimetre() {
        let frame = Frame::about(&swamp());
        for p in swamp() {
            let back = frame.to_position(frame.to_metres(p));
            assert!((back[0] - p[0]).abs() < 1e-7 && (back[1] - p[1]).abs() < 1e-7);
        }
        // 0.001° of latitude is 111 m on the sphere.
        let d = metres_between(&frame, [29.6, -2.0], [29.6, -2.001]);
        assert!((d - 111.195).abs() < 0.05, "{d}");
    }

    #[test]
    fn the_hull_is_counter_clockwise_and_drops_interior_and_duplicate_points() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let mut cloud = square.to_vec();
        cloud.push([5.0, 5.0]);
        cloud.push([10.0, 0.0]);
        cloud.push([5.0, 0.0]);
        let hull = convex_hull(&cloud);
        assert_eq!(
            hull,
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
        );
        assert_eq!(convex_hull(&[[1.0, 1.0]]), vec![[1.0, 1.0]]);
        assert_eq!(
            convex_hull(&[[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]]),
            vec![[1.0, 1.0], [3.0, 3.0]]
        );
    }

    #[test]
    fn three_poles_become_a_closed_rounded_area_that_holds_every_pole_with_its_margin() {
        let ring = buffered_hull(&swamp(), 25.0);
        assert_eq!(ring.first(), ring.last(), "closed");
        assert!(
            ring.len() >= 4 && ring.len() <= 3 * ARC_POINTS + 1,
            "{}",
            ring.len()
        );
        let frame = Frame::about(&swamp());
        // Every pole is at least 25 m (minus a centimetre of rounding and the
        // 16-gon's flat) inside the ring: the nearest ring vertex is at least
        // buffer * cos(π/16) away.
        let inner = 25.0 * (std::f64::consts::PI / ARC_POINTS as f64).cos() - 0.02;
        for pole in swamp() {
            let nearest = ring
                .iter()
                .map(|&v| metres_between(&frame, pole, v))
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest >= inner,
                "pole {pole:?} is {nearest} m from the ring"
            );
        }
        // And no ring vertex is farther than the buffer from the hull: the
        // area is a buffer, not a bounding box.
        for &v in &ring {
            let nearest = swamp()
                .iter()
                .map(|&p| metres_between(&frame, p, v))
                .fold(f64::INFINITY, f64::min);
            assert!(nearest <= 25.02 + 60.0, "{nearest}");
        }
        // Counter-clockwise (positive shoelace area), as GeoJSON asks.
        let area: f64 = ring
            .windows(2)
            .map(|w| {
                let a = frame.to_metres(w[0]);
                let b = frame.to_metres(w[1]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            / 2.0;
        assert!(area > 0.0, "area {area}");
        // Deterministic, byte for byte.
        assert_eq!(ring, buffered_hull(&swamp(), 25.0));
    }

    #[test]
    fn two_poles_become_a_capsule_and_one_pole_a_disc() {
        let two = &swamp()[..2];
        let ring = buffered_hull(two, 10.0);
        assert_eq!(ring.first(), ring.last());
        assert!(ring.len() > 4);
        let one = buffered_hull(&swamp()[..1], 10.0);
        assert_eq!(one.len(), ARC_POINTS + 1);
    }

    #[test]
    fn a_range_keeps_the_bends_between_the_two_structures_and_ends_exactly_on_them() {
        let vertices = vec![
            Vertex {
                position: [29.60, -2.30],
                station_m: 0.0,
            },
            Vertex {
                position: [29.61, -2.30],
                station_m: 1000.0,
            },
            Vertex {
                position: [29.61, -2.31],
                station_m: 2000.0,
            },
            Vertex {
                position: [29.62, -2.31],
                station_m: 3000.0,
            },
        ];
        let s74 = ([29.605, -2.30], 500.0);
        let s77 = ([29.615, -2.31], 2500.0);
        let line = clip_by_station(&vertices, s74, s77);
        assert_eq!(
            line,
            vec![
                [29.605, -2.30],
                [29.61, -2.30],
                [29.61, -2.31],
                [29.615, -2.31]
            ]
        );
        // Written the other way round, the same line.
        assert_eq!(clip_by_station(&vertices, s77, s74), line);
        // Two structures on one leg: just the two of them.
        assert_eq!(
            clip_by_station(
                &vertices,
                ([29.602, -2.30], 200.0),
                ([29.608, -2.30], 800.0)
            ),
            vec![[29.602, -2.30], [29.608, -2.30]]
        );
    }
}
