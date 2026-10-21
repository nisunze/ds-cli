use crate::primitives::{haversine, interpolation};
use crate::types::{Coord, StationedPoint};

/// Create evenly-spaced points along a polyline.
///
/// - `coords`:       ordered line vertices
/// - `interval_m`:   spacing in meters (> 0)
/// - `include_ends`: always emit first and last vertex
pub fn points_along(coords: &[Coord], interval_m: f64, include_ends: bool) -> Vec<StationedPoint> {
    if coords.len() < 2 || !interval_m.is_finite() || interval_m <= 0.0 {
        return vec![];
    }

    let mut out = Vec::new();

    if include_ends {
        out.push(StationedPoint {
            coord: coords[0],
            distance_m: 0.0,
        });
    }

    let mut accumulated: f64 = 0.0;
    let mut next_target: f64 = interval_m;

    for w in coords.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg_dist = haversine::distance(a, b);
        let seg_start = accumulated;
        accumulated += seg_dist;

        while next_target <= accumulated {
            let frac = if seg_dist > 0.0 {
                (next_target - seg_start) / seg_dist
            } else {
                0.0
            };
            out.push(StationedPoint {
                coord: interpolation::lerp(a, b, frac),
                distance_m: next_target,
            });
            next_target += interval_m;
        }
    }

    if include_ends {
        let last = coords[coords.len() - 1];
        let dup = out.last().is_some_and(|p| {
            (p.coord.lng - last.lng).abs() < 1e-12 && (p.coord.lat - last.lat).abs() < 1e-12
        });
        if !dup {
            out.push(StationedPoint {
                coord: last,
                distance_m: accumulated,
            });
        }
    }

    out
}

/// Create evenly-spaced points along a polyline and preserve every authored
/// vertex as an exact sample.
///
/// This is the profile/stationing contract: regular samples describe the
/// terrain between vertices, while exact vertex samples ensure bends and other
/// authored key stations cannot disappear between intervals.
pub fn points_along_with_vertices(coords: &[Coord], interval_m: f64) -> Vec<StationedPoint> {
    let mut out = points_along(coords, interval_m, true);
    if out.is_empty() {
        return out;
    }

    let mut station_m = 0.0;
    for (index, coord) in coords.iter().copied().enumerate() {
        if index > 0 {
            station_m += haversine::distance(coords[index - 1], coord);
        }

        // Prefer the exact authored coordinate when a regular interval lands
        // on the same station (or close enough for floating-point geodesics).
        if let Some(existing) = out
            .iter_mut()
            .find(|point| (point.distance_m - station_m).abs() < 1e-7)
        {
            existing.coord = coord;
            existing.distance_m = station_m;
        } else {
            out.push(StationedPoint {
                coord,
                distance_m: station_m,
            });
        }
    }

    out.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coord(lng: f64, lat: f64) -> Coord {
        Coord::new(lng, lat)
    }

    #[test]
    fn vertex_preserving_samples_include_a_corner_between_intervals() {
        let corner = coord(30.001, -2.0);
        let coords = [coord(30.0, -2.0), corner, coord(30.001, -1.999)];

        let sampled = points_along_with_vertices(&coords, 150.0);

        assert!(
            sampled
                .iter()
                .any(|point| { point.coord.lng == corner.lng && point.coord.lat == corner.lat })
        );
        assert!(
            sampled
                .windows(2)
                .all(|pair| pair[0].distance_m <= pair[1].distance_m)
        );
    }

    #[test]
    fn vertex_on_an_interval_is_not_duplicated() {
        let start = coord(30.0, -2.0);
        let corner = coord(30.001, -2.0);
        let end = coord(30.002, -2.0);
        let interval_m = haversine::distance(start, corner);

        let sampled = points_along_with_vertices(&[start, corner, end], interval_m);
        let at_corner = sampled
            .iter()
            .filter(|point| (point.distance_m - interval_m).abs() < 1e-7)
            .collect::<Vec<_>>();

        assert_eq!(at_corner.len(), 1);
        assert_eq!(at_corner[0].coord.lng, corner.lng);
        assert_eq!(at_corner[0].coord.lat, corner.lat);
    }

    #[test]
    fn duplicate_vertices_do_not_create_duplicate_stations() {
        let start = coord(30.0, -2.0);
        let end = coord(30.001, -2.0);

        let sampled = points_along_with_vertices(&[start, start, end], 50.0);

        assert_eq!(
            sampled
                .iter()
                .filter(|point| point.distance_m.abs() < 1e-7)
                .count(),
            1
        );
    }
}
