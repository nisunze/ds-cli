//! Profile control-station classification for generic alignments.
//!
//! The profile renderer consumes typed ordinates; it does not rediscover
//! topology from pixels. This operation assigns local chainage to authored
//! starts, ends and direction changes, then promotes geometric junctions to
//! taps. A branch endpoint may meet the middle of another alignment segment,
//! so taps can be virtual points on the receiving alignment.

use rstar::{AABB, RTree, RTreeObject};

use crate::{
    primitives::{bearing, haversine},
    types::Coord,
};

const ANGLE_POINT_MIN_DEFLECTION_DEG: f64 = 0.1;
const TAP_TOLERANCE_M: f64 = 0.5;
const STATION_DEDUP_TOLERANCE_M: f64 = 0.001;
const M_PER_DEG_LAT: f64 = 111_320.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileStationRole {
    Start,
    End,
    AnglePoint,
    Tap,
}

impl ProfileStationRole {
    pub fn code(self) -> u8 {
        match self {
            Self::Start => 0,
            Self::End => 1,
            Self::AnglePoint => 2,
            Self::Tap => 3,
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::AnglePoint => 0,
            Self::Start | Self::End => 1,
            Self::Tap => 2,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ClassifiedProfileStation {
    pub line_index: usize,
    /// `None` means a virtual point projected into the middle of a receiving
    /// alignment segment. Its station and coordinate remain exact outputs.
    pub vertex_index: Option<usize>,
    pub role: ProfileStationRole,
    pub station_m: f64,
    pub coord: Coord,
}

#[derive(Clone)]
struct IndexedSegment {
    line_index: usize,
    start_station_m: f64,
    length_m: f64,
    start: Coord,
    end: Coord,
    envelope: AABB<[f64; 2]>,
}

impl RTreeObject for IndexedSegment {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.envelope
    }
}

fn cumulative_stations(line: &[Coord]) -> Vec<f64> {
    let mut stations = Vec::with_capacity(line.len());
    stations.push(0.0);
    for index in 1..line.len() {
        stations.push(stations[index - 1] + haversine::distance(line[index - 1], line[index]));
    }
    stations
}

fn add_station(out: &mut Vec<ClassifiedProfileStation>, station: ClassifiedProfileStation) {
    if let Some(existing) = out.iter_mut().find(|candidate| {
        candidate.line_index == station.line_index
            && (candidate.station_m - station.station_m).abs() <= STATION_DEDUP_TOLERANCE_M
    }) {
        if station.role.priority() > existing.role.priority() {
            *existing = station;
        } else if station.role == existing.role
            && existing.vertex_index.is_none()
            && station.vertex_index.is_some()
        {
            // Prefer the authored identity when a virtual tap coincides with it.
            *existing = station;
        }
        return;
    }
    out.push(station);
}

fn deflection_degrees(incoming: f64, outgoing: f64) -> f64 {
    let delta = (outgoing - incoming).abs() % 360.0;
    delta.min(360.0 - delta)
}

fn projected_fraction_and_offset_m(point: Coord, start: Coord, end: Coord) -> Option<(f64, f64)> {
    let m_per_deg_lng = M_PER_DEG_LAT * point.lat.to_radians().cos();
    if m_per_deg_lng.abs() <= f64::EPSILON {
        return None;
    }
    let ax = (start.lng - point.lng) * m_per_deg_lng;
    let ay = (start.lat - point.lat) * M_PER_DEG_LAT;
    let bx = (end.lng - point.lng) * m_per_deg_lng;
    let by = (end.lat - point.lat) * M_PER_DEG_LAT;
    let vx = bx - ax;
    let vy = by - ay;
    let length_squared = vx * vx + vy * vy;
    if length_squared <= f64::EPSILON {
        return None;
    }
    let fraction = (-(ax * vx + ay * vy) / length_squared).clamp(0.0, 1.0);
    let qx = ax + fraction * vx;
    let qy = ay + fraction * vy;
    Some((fraction, qx.hypot(qy)))
}

/// Classify profile control stations for a set of independently stationed
/// alignments. Output is sorted by `(line_index, station_m)`.
pub fn classify_profile_stations(lines: &[Vec<Coord>]) -> Vec<ClassifiedProfileStation> {
    let cumulative: Vec<Vec<f64>> = lines.iter().map(|line| cumulative_stations(line)).collect();
    let mut out = Vec::new();

    for (line_index, line) in lines.iter().enumerate() {
        let Some(stations) = cumulative.get(line_index) else {
            continue;
        };
        let Some(&total_m) = stations.last() else {
            continue;
        };
        if line.len() < 2 || total_m <= f64::EPSILON {
            continue;
        }
        add_station(
            &mut out,
            ClassifiedProfileStation {
                line_index,
                vertex_index: Some(0),
                role: ProfileStationRole::Start,
                station_m: 0.0,
                coord: line[0],
            },
        );
        for vertex_index in 1..line.len() - 1 {
            let vertex = line[vertex_index];
            let previous = (0..vertex_index)
                .rev()
                .find(|&index| haversine::distance(line[index], vertex) > f64::EPSILON);
            let next = (vertex_index + 1..line.len())
                .find(|&index| haversine::distance(vertex, line[index]) > f64::EPSILON);
            let (Some(previous), Some(next)) = (previous, next) else {
                continue;
            };
            let deflection = deflection_degrees(
                bearing::initial(line[previous], vertex),
                bearing::initial(vertex, line[next]),
            );
            if deflection > ANGLE_POINT_MIN_DEFLECTION_DEG {
                add_station(
                    &mut out,
                    ClassifiedProfileStation {
                        line_index,
                        vertex_index: Some(vertex_index),
                        role: ProfileStationRole::AnglePoint,
                        station_m: stations[vertex_index],
                        coord: vertex,
                    },
                );
            }
        }
        add_station(
            &mut out,
            ClassifiedProfileStation {
                line_index,
                vertex_index: Some(line.len() - 1),
                role: ProfileStationRole::End,
                station_m: total_m,
                coord: line[line.len() - 1],
            },
        );
    }

    let segments = lines
        .iter()
        .enumerate()
        .flat_map(|(line_index, line)| {
            line.windows(2).enumerate().filter_map({
                let stations = &cumulative[line_index];
                move |(segment_index, pair)| {
                    let length_m = haversine::distance(pair[0], pair[1]);
                    (length_m > f64::EPSILON).then(|| IndexedSegment {
                        line_index,
                        start_station_m: stations[segment_index],
                        length_m,
                        start: pair[0],
                        end: pair[1],
                        envelope: AABB::from_corners(
                            [pair[0].lng.min(pair[1].lng), pair[0].lat.min(pair[1].lat)],
                            [pair[0].lng.max(pair[1].lng), pair[0].lat.max(pair[1].lat)],
                        ),
                    })
                }
            })
        })
        .collect::<Vec<_>>();
    let segment_index = RTree::bulk_load(segments);
    // A degree of longitude is never longer than a degree of latitude, so this
    // angular query envelope is conservative at every latitude. Exact metre
    // distance is still checked below.
    let query_radius_deg = TAP_TOLERANCE_M / M_PER_DEG_LAT;

    for (line_index, line) in lines.iter().enumerate() {
        if line.len() < 2
            || cumulative[line_index]
                .last()
                .is_none_or(|total_m| *total_m <= f64::EPSILON)
        {
            continue;
        }
        for (vertex_index, &vertex) in line.iter().enumerate() {
            let query = AABB::from_corners(
                [vertex.lng - query_radius_deg, vertex.lat - query_radius_deg],
                [vertex.lng + query_radius_deg, vertex.lat + query_radius_deg],
            );
            for segment in segment_index.locate_in_envelope_intersecting(&query) {
                if segment.line_index == line_index {
                    continue;
                }
                let Some((fraction, offset_m)) =
                    projected_fraction_and_offset_m(vertex, segment.start, segment.end)
                else {
                    continue;
                };
                if offset_m > TAP_TOLERANCE_M {
                    continue;
                }
                let own_station_m = cumulative[line_index][vertex_index];
                add_station(
                    &mut out,
                    ClassifiedProfileStation {
                        line_index,
                        vertex_index: Some(vertex_index),
                        role: ProfileStationRole::Tap,
                        station_m: own_station_m,
                        coord: vertex,
                    },
                );
                add_station(
                    &mut out,
                    ClassifiedProfileStation {
                        line_index: segment.line_index,
                        vertex_index: None,
                        role: ProfileStationRole::Tap,
                        station_m: segment.start_station_m + fraction * segment.length_m,
                        coord: Coord::new(
                            segment.start.lng + fraction * (segment.end.lng - segment.start.lng),
                            segment.start.lat + fraction * (segment.end.lat - segment.start.lat),
                        ),
                    },
                );
            }
        }
    }

    out.sort_by(|left, right| {
        left.line_index
            .cmp(&right.line_index)
            .then_with(|| left.station_m.total_cmp(&right.station_m))
            .then_with(|| left.role.code().cmp(&right.role.code()))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(lng: f64, lat: f64) -> Coord {
        Coord::new(lng, lat)
    }

    #[test]
    fn emits_endpoints_and_only_true_direction_changes() {
        let lines = vec![vec![c(30.0, -2.0), c(30.001, -2.0), c(30.001, -1.999)]];
        let stations = classify_profile_stations(&lines);
        assert_eq!(stations.len(), 3);
        assert_eq!(stations[0].role, ProfileStationRole::Start);
        assert_eq!(stations[1].role, ProfileStationRole::AnglePoint);
        assert_eq!(stations[1].vertex_index, Some(1));
        assert_eq!(stations[2].role, ProfileStationRole::End);

        let collinear = vec![vec![c(30.0, -2.0), c(30.001, -2.0), c(30.002, -2.0)]];
        let stations = classify_profile_stations(&collinear);
        assert_eq!(stations.len(), 2);
        assert_eq!(stations[0].role, ProfileStationRole::Start);
        assert_eq!(stations[1].role, ProfileStationRole::End);
    }

    #[test]
    fn branch_endpoint_creates_a_virtual_tap_on_the_trunk() {
        let lines = vec![
            vec![c(30.0, -2.0), c(30.002, -2.0)],
            vec![c(30.001, -2.001), c(30.001, -2.0)],
        ];
        let stations = classify_profile_stations(&lines);
        let trunk_tap = stations
            .iter()
            .find(|station| station.line_index == 0 && station.role == ProfileStationRole::Tap)
            .expect("trunk tap");
        assert_eq!(trunk_tap.vertex_index, None);
        assert!(
            (trunk_tap.station_m - haversine::distance(lines[0][0], c(30.001, -2.0))).abs() < 0.05
        );
        let branch_tap = stations
            .iter()
            .find(|station| station.line_index == 1 && station.role == ProfileStationRole::Tap)
            .expect("branch tap");
        assert_eq!(branch_tap.vertex_index, Some(1));
        assert!(stations.iter().all(|station| {
            !(station.line_index == 1
                && station.vertex_index == Some(1)
                && station.role == ProfileStationRole::End)
        }));
    }

    #[test]
    fn shared_authored_vertex_is_a_tap_on_every_line() {
        let shared = c(30.001, -2.0);
        let lines = vec![
            vec![c(30.0, -2.0), shared, c(30.002, -2.0)],
            vec![shared, c(30.001, -1.999)],
        ];
        let stations = classify_profile_stations(&lines);
        for line_index in 0..2 {
            assert!(stations.iter().any(|station| {
                station.line_index == line_index
                    && station.role == ProfileStationRole::Tap
                    && station.vertex_index.is_some()
            }));
        }
    }

    #[test]
    fn skips_degenerate_lines_and_sorts_by_local_station() {
        let same = c(30.0, -2.0);
        let lines = vec![vec![same], vec![same, same], vec![same, c(30.001, -2.0)]];
        let stations = classify_profile_stations(&lines);
        assert!(stations.iter().all(|station| station.line_index == 2));
        assert_eq!(stations[0].station_m, 0.0);
        assert!(stations[1].station_m > stations[0].station_m);
    }
}
