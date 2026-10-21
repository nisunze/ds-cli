//! Perpendicular projection of points onto a polyline (station / offset).
//!
//! The profile view's corridor step: drop a perpendicular from each candidate
//! point onto the nearest segment of the alignment and report its chainage
//! (station along the line) plus perpendicular offset. Points whose offset
//! exceeds `max_offset_m` are dropped; survivors come back sorted by station.
//!
//! Geometry runs in a local equirectangular metre frame anchored at the line
//! start. At corridor scale (≤ tens of km, sub-100 m offsets) the distortion
//! is far below survey/DEM accuracy. Ported from ds-web
//! `map/elevation/project-points.ts` so the browser keeps one geometry owner.

use crate::types::Coord;

/// Metres per degree of latitude in the local equirectangular frame. Matches
/// the constant the ds-web TS implementation used, so results stay
/// bit-comparable across the migration (deliberately NOT derived from
/// `EARTH_RADIUS_M`, which would give 111 194.9).
const M_PER_DEG_LAT: f64 = 111_320.0;

/// One surviving projection. `input_index` references the caller's point
/// order so per-point payloads (labels, elevations) can be re-joined without
/// re-deriving them here.
pub struct ProjectedOntoLine {
    pub input_index: usize,
    /// Chainage along the line from its start (metres).
    pub station_m: f64,
    /// Perpendicular distance from the line (metres).
    pub offset_m: f64,
    /// Foot of the perpendicular on the line.
    pub projected: Coord,
}

/// Project `points` onto `line`, keeping only those within `max_offset_m`.
pub fn project_onto_line(
    line: &[Coord],
    points: &[Coord],
    max_offset_m: f64,
) -> Vec<ProjectedOntoLine> {
    if line.len() < 2 || !max_offset_m.is_finite() || max_offset_m < 0.0 {
        return Vec::new();
    }
    let lng0 = line[0].lng;
    let lat0 = line[0].lat;
    let m_per_lng = M_PER_DEG_LAT * lat0.to_radians().cos();
    if !m_per_lng.is_finite() || m_per_lng.abs() <= f64::EPSILON {
        return Vec::new();
    }
    let to_xy = |c: Coord| [(c.lng - lng0) * m_per_lng, (c.lat - lat0) * M_PER_DEG_LAT];

    let verts: Vec<[f64; 2]> = line.iter().map(|&c| to_xy(c)).collect();
    let mut cum = Vec::with_capacity(verts.len());
    cum.push(0.0f64);
    for i in 1..verts.len() {
        let d = (verts[i][0] - verts[i - 1][0]).hypot(verts[i][1] - verts[i - 1][1]);
        cum.push(cum[i - 1] + d);
    }

    let mut out = Vec::new();
    for (input_index, &p) in points.iter().enumerate() {
        let [px, py] = to_xy(p);
        let mut best_d2 = f64::INFINITY;
        let mut best_station = 0.0;
        let mut best_x = px;
        let mut best_y = py;
        for i in 1..verts.len() {
            let [ax, ay] = verts[i - 1];
            let [bx, by] = verts[i];
            let vx = bx - ax;
            let vy = by - ay;
            let len2 = vx * vx + vy * vy;
            let t = if len2 > 0.0 {
                (((px - ax) * vx + (py - ay) * vy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let qx = ax + t * vx;
            let qy = ay + t * vy;
            let dx = px - qx;
            let dy = py - qy;
            let d2 = dx * dx + dy * dy;
            if d2 < best_d2 {
                best_d2 = d2;
                best_station = cum[i - 1] + t * len2.sqrt();
                best_x = qx;
                best_y = qy;
            }
        }
        let offset = best_d2.sqrt();
        if offset <= max_offset_m {
            out.push(ProjectedOntoLine {
                input_index,
                station_m: best_station,
                offset_m: offset,
                projected: Coord::new(lng0 + best_x / m_per_lng, lat0 + best_y / M_PER_DEG_LAT),
            });
        }
    }
    out.sort_by(|a, b| a.station_m.total_cmp(&b.station_m));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mirrors the ds-web adapter tests that shipped with the TS
    // implementation (tests/elevation-projection.test.ts) so behavior is
    // pinned across the TS→Rust migration.

    #[test]
    fn keeps_the_perpendicular_foot_coordinate() {
        let line = [Coord::new(30.0, -2.0), Coord::new(30.001, -2.0)];
        let points = [Coord::new(30.0005, -1.9999)];
        let projected = project_onto_line(&line, &points, 20.0);

        assert_eq!(projected.len(), 1);
        let p = &projected[0];
        assert_eq!(p.input_index, 0);
        assert!((p.projected.lng - 30.0005).abs() < 1e-6);
        assert!((p.projected.lat - -2.0).abs() < 1e-6);
        assert!((p.offset_m - 11.132).abs() < 0.01);
        assert!(p.station_m > 0.0);
    }

    #[test]
    fn drops_points_outside_the_corridor() {
        let line = [Coord::new(30.0, -2.0), Coord::new(30.001, -2.0)];
        let points = [Coord::new(30.0005, -1.99)];
        assert!(project_onto_line(&line, &points, 20.0).is_empty());
    }

    #[test]
    fn sorts_survivors_by_station() {
        let line = [Coord::new(30.0, -2.0), Coord::new(30.002, -2.0)];
        let points = [
            Coord::new(30.0015, -2.00005),
            Coord::new(30.0002, -1.99995),
            Coord::new(30.001, -2.0),
        ];
        let projected = project_onto_line(&line, &points, 50.0);
        assert_eq!(projected.len(), 3);
        assert_eq!(
            projected.iter().map(|p| p.input_index).collect::<Vec<_>>(),
            vec![1, 2, 0],
        );
        assert!(projected[0].station_m <= projected[1].station_m);
        assert!(projected[1].station_m <= projected[2].station_m);
    }

    #[test]
    fn degenerate_line_returns_empty() {
        assert!(
            project_onto_line(&[Coord::new(30.0, -2.0)], &[Coord::new(30.0, -2.0)], 10.0)
                .is_empty()
        );
    }
}
