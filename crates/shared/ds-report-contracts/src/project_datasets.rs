//! The project-dataset exchange the Kernel and the Reporter share: the exact
//! rectangle coverage a room holds, the area rule each dataset's provider
//! selects with, the Google Open Buildings tiling and the Reporter's typed
//! footprint and contour tasks. The Kernel's dataset cache plans and stores
//! over these (`ds-command-kernel` `project_dataset_cache`).

use std::collections::BTreeSet;

use ds_geo_lite::spatial_predicate::{selects_area, AreaSelection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "ds.project-dataset-cache/v1";
pub const MAX_COVERAGE_CELLS: usize = 4_096;
/// 1e-9° is ~0.1 mm. Rounding inputs once keeps every downstream decomposition
/// exact on the rounded values and makes digests reproducible across hosts.
pub const QUANTUM: f64 = 1e9;
pub const MAX_LATITUDE: f64 = 85.0;

pub fn round(value: f64) -> f64 {
    (value * QUANTUM).round() / QUANTUM
}

/// Quantising a buffered edge must never move it inward: half a quantum of
/// shrinkage is enough to make a "500 m" buffer measure 499.99995 m, and a
/// coverage that is fractionally short is a coverage that lies.
pub fn round_out(value: f64, outward_is_up: bool) -> f64 {
    let scaled = value * QUANTUM;
    if outward_is_up {
        scaled.ceil() / QUANTUM
    } else {
        scaled.floor() / QUANTUM
    }
}

/// A canonical set of disjoint axis-aligned rectangles whose union is the
/// covered region. Disconnected design clusters stay disconnected, so a
/// coverage can never collapse into one enormous envelope spanning unrelated
/// sites; and held coverage accumulated over many acquisitions stays exact,
/// because the difference and union arithmetic below is exact.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    #[serde(default)]
    pub cells: Vec<[f64; 4]>,
}

impl Coverage {
    pub fn rects(&self) -> Result<Vec<Rect>, String> {
        if self.cells.len() > MAX_COVERAGE_CELLS {
            return Err("coverage exceeds its cell limit".into());
        }
        self.cells.iter().copied().map(Rect::from_bounds).collect()
    }
    pub fn from_rects(rects: Vec<Rect>) -> Self {
        let mut cells: Vec<[f64; 4]> = rects.into_iter().map(|rect| rect.bounds()).collect();
        cells.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Self { cells }
    }
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
    /// GeoJSON projection of the same exact set, one Polygon per cell.
    pub fn geometry(&self) -> Option<Value> {
        if self.cells.is_empty() {
            return None;
        }
        let polygons: Vec<Value> = self
            .cells
            .iter()
            .map(|&[w, s, e, n]| json!([[[w, s], [e, s], [e, n], [w, n], [w, s]]]))
            .collect();
        Some(json!({"type": "MultiPolygon", "coordinates": polygons}))
    }
    pub fn digest(&self) -> String {
        digest_of(&json!({"schema": SCHEMA, "cells": self.cells}))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub w: f64,
    pub s: f64,
    pub e: f64,
    pub n: f64,
}

impl Rect {
    pub fn from_bounds(bounds: [f64; 4]) -> Result<Self, String> {
        let [w, s, e, n] = bounds;
        if !bounds.iter().all(|value| value.is_finite())
            || w > e
            || s > n
            || !(-180.0..=180.0).contains(&w)
            || !(-180.0..=180.0).contains(&e)
            || !(-90.0..=90.0).contains(&s)
            || !(-90.0..=90.0).contains(&n)
        {
            return Err("a coverage rectangle must be finite, ordered WGS84 bounds".into());
        }
        Ok(Self {
            w: round(w),
            s: round(s),
            e: round(e),
            n: round(n),
        })
    }
    pub fn bounds(self) -> [f64; 4] {
        [self.w, self.s, self.e, self.n]
    }
    pub fn degenerate(self) -> bool {
        self.w >= self.e || self.s >= self.n
    }
    pub fn contains(self, x: f64, y: f64) -> bool {
        self.w <= x && x <= self.e && self.s <= y && y <= self.n
    }
    pub fn intersects(self, other: Self) -> bool {
        self.w <= other.e && other.w <= self.e && self.s <= other.n && other.s <= self.n
    }
    pub fn union(self, other: Self) -> Self {
        Self {
            w: self.w.min(other.w),
            s: self.s.min(other.s),
            e: self.e.max(other.e),
            n: self.n.max(other.n),
        }
    }
}

pub fn digest_of(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}

/// Key-sorted JSON so a digest depends on content, never on map order.
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let entries: Vec<String> = map
                .iter()
                .map(|(key, child)| format!("{}:{}", Value::String(key.clone()), canonical(child)))
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Value::Array(items) => {
            let entries: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", entries.join(","))
        }
        other => other.to_string(),
    }
}

/// Canonical dataset ids a room knows how to acquire and refresh.
pub const BUILDINGS_DATASET_ID: &str = "google_open_buildings";
pub const CONTOURS_DATASET_ID: &str = "elevation_contours";

/// The area rule this dataset's provider selects features with.
///
/// A refresh removes the rows an area already holds before the replacement
/// lands, so it MUST remove them under the same rule the provider fetches
/// under. Removing more than the provider will return loses a real building —
/// a footprint crossing the edge of the area is exactly that case — and
/// removing less leaves a duplicate of one behind. There is therefore no
/// default here: a dataset whose rule is not recorded cannot be held or
/// refreshed, and refusing is better than guessing at something destructive.
///
/// * `google_open_buildings` selects by **intersection**. Its governed door is
///   Earth Engine `listFeatures`, whose `region` is an intersects predicate,
///   and no further centre test is applied downstream — the provider's own
///   receipt records `region_predicate: "upstream listFeatures region
///   (intersects)"`.
/// * `elevation_contours` selects by **intersection**: contours are generated
///   for the area and clipped to it.
///
/// A provider that selects by centre — Microsoft's building index does, and
/// records `region_predicate: "centroid inside AOI"` — must be registered here
/// as `CentroidInside` before any of its rows may enter a room.
///
/// National reference subsets are addressed by per-resource digests, so their
/// rule is recorded against the *provider*: the desktop reference cache selects
/// rows whose geometry intersects the area (`reference_cache::intersects_area`),
/// which is the same rule its subsets are admitted and refreshed under.
pub const REFERENCE_CACHE_PROVIDER: &str = "desktop:reference_resource_cache";

/// The cloud-resident datasets (foundation-datasets contract R2/R3) seed a
/// project's cells through ds-brain's bounded read, whose boundary bound is
/// `ST_INTERSECTS(geometry, boundary)`: a parcel crossing the cell's edge
/// belongs to that cell, exactly as a footprint does. Recorded against the
/// provider because the datasets are addressed by catalogue digests.
pub const CLOUD_READ_PROVIDER: &str = "brain_data_distribution";

pub fn acquisition_selection(dataset_id: &str, provider: &str) -> Result<AreaSelection, String> {
    match (dataset_id, provider) {
        (BUILDINGS_DATASET_ID | CONTOURS_DATASET_ID, _) => Ok(AreaSelection::Intersects),
        (_, REFERENCE_CACHE_PROVIDER | CLOUD_READ_PROVIDER) => Ok(AreaSelection::Intersects),
        (other, provider) => Err(format!(
            "dataset `{other}` from provider `{provider}` does not record the area rule its provider selects with; register it in acquisition_selection before a room may hold or refresh it"
        )),
    }
}

/// How far outside the requested area a provider's intersects predicate may
/// still say yes, in degrees (about one metre at project latitudes).
///
/// The providers decide intersection on the sphere with their own error
/// margin — Earth Engine's `region` is geodesic and tolerant to roughly a
/// metre — while the kernel decides it exactly in the plane. A footprint whose
/// edge lies within centimetres of the area's boundary is therefore an
/// upstream yes and a local no, and the seed that met one refused a whole
/// acquisition over a row the provider selected correctly. The margin is the
/// provider's rule written down, not a relaxation of it: a row farther out
/// than any provider margin is still the overfetch or boundary defect the
/// refusal exists to catch.
///
/// The desktop reference cache has no margin: its index answers this very
/// predicate (`reference_cache::intersects_area`) exactly, in the plane.
pub const PROVIDER_BOUNDARY_MARGIN_DEG: f64 = 1e-5;

/// Does this dataset's acquisition select `feature` for `area`?
///
/// Admitting an acquired row and deleting the row it replaces are the same
/// question, so they are the same call. Nothing may re-answer it locally. An
/// intersects rule carries the provider's boundary margin, so a row the
/// provider selected at its edge is selected here at the same edge — on
/// admission and on the deletion that replaces it.
///
/// The margin grows a row's bounds, not its geometry, so it selects a road
/// whose bounds cross into the area where the road itself does not. A remote
/// provider may return that road; the desktop reference cache never does, so
/// a margin there would delete, on any later tile's replacement, a road an
/// earlier seed holds and the index never hands back. It selects exactly.
pub fn acquisition_selects(
    dataset_id: &str,
    provider: &str,
    feature: &Value,
    area: &Value,
) -> Result<bool, String> {
    let selection = acquisition_selection(dataset_id, provider)?;
    if selects_area(feature, area, selection)? {
        return Ok(true);
    }
    if selection != AreaSelection::Intersects || provider == REFERENCE_CACHE_PROVIDER {
        return Ok(false);
    }
    // Within the margin: the feature's bounds, grown by the margin, meet the
    // area. Bounds are a superset of the geometry, so this admits nothing the
    // exact test would refuse by more than the margin plus the feature's own
    // extent — a nearby building, never a distant one.
    let Some([w, s, e, n]) = ds_geo_lite::viewport::geojson_bounds(feature) else {
        return Ok(false);
    };
    let m = PROVIDER_BOUNDARY_MARGIN_DEG;
    let grown = serde_json::json!({
        "type": "Feature",
        "properties": {},
        "geometry": {"type": "Polygon", "coordinates": [[
            [w - m, s - m], [e + m, s - m], [e + m, n + m], [w - m, n + m], [w - m, s - m]
        ]]}
    });
    selects_area(&grown, area, AreaSelection::Intersects)
}

/// Google Open Buildings v3, as published: one gzip CSV per S2 level-6 cell,
/// no header, rows partitioned by footprint centroid.
pub mod google_open_buildings {
    use super::*;

    pub const BUCKET: &str = "open-buildings-data";
    pub const PREFIX: &str = "v3/polygons_s2_level_6_gzip_no_header/";
    pub const S2_LEVEL: u8 = 6;
    /// The version a revision acquired from this publisher records.
    pub const VERSION: &str = "google-open-buildings/v3/polygons_s2_level_6";
    pub const COLUMNS: [&str; 6] = [
        "latitude",
        "longitude",
        "area_in_meters",
        "confidence",
        "geometry",
        "full_plus_code",
    ];
    /// A footprint is filed under its centroid's cell. One whose outline
    /// reaches into a gap can have its centroid this far outside it.
    pub const CENTROID_MARGIN_DEG: f64 = 0.01;
    /// Sampling step of the covering search; far below a level-6 cell.
    const STEP_DEG: f64 = 0.002;
    /// The most tiles one seed may read; a project is a district, not a
    /// continent.
    pub const MAX_TILES: usize = 24;

    /// The public object name of one level-6 tile.
    pub fn object(token: &str) -> Result<String, String> {
        if token.len() != 4
            || !token
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(format!("{token:?} is not an S2 level-6 token"));
        }
        Ok(format!("{PREFIX}{token}_buildings.csv.gz"))
    }

    /// Every tile whose rows can intersect the cells: the S2 level-6 cells
    /// meeting each cell grown by the centroid margin, found by sampling at a
    /// step no cell corner can hide between.
    pub fn tiles(cells: &Coverage) -> Result<Vec<String>, String> {
        let mut tokens = BTreeSet::new();
        for [w, s, e, n] in cells
            .rects()?
            .into_iter()
            .map(|rect| [rect.w, rect.s, rect.e, rect.n])
        {
            let grow = CENTROID_MARGIN_DEG + 2.0 * STEP_DEG;
            let (w, s, e, n) = (w - grow, s - grow, e + grow, n + grow);
            let columns = ((e - w) / STEP_DEG).ceil() as usize;
            let rows = ((n - s) / STEP_DEG).ceil() as usize;
            if columns.saturating_mul(rows) > 4_000_000 {
                return Err("a publisher tile search cell is wider than a project extent".into());
            }
            for row in 0..=rows {
                let lat = (s + row as f64 * STEP_DEG).min(n).clamp(-90.0, 90.0);
                for column in 0..=columns {
                    let lng = (w + column as f64 * STEP_DEG).min(e);
                    tokens.insert(s2_token(lat, lng, S2_LEVEL));
                }
            }
            if tokens.len() > MAX_TILES {
                return Err(format!(
                    "the missing coverage reaches more than {MAX_TILES} Google Open Buildings tiles; narrow the seed"
                ));
            }
        }
        Ok(tokens.into_iter().collect())
    }
}

/// The offline engine tasks a seed hands its acquired bytes to. Core builds
/// the request with explicit local inputs; the Reporter converts or derives
/// and answers with files and digests. Both sides read these types, so the
/// exchange is one typed contract, and a later field is additive.
pub mod tasks {
    use std::path::PathBuf;

    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    pub const FOOTPRINTS_SUBCOMMAND: &str = "prepare-publisher-footprints";
    pub const FOOTPRINTS_REQUEST: &str = "ds.reporter.publisher-footprints-request/v1";
    pub const FOOTPRINTS_RESULT: &str = "ds.reporter.publisher-footprints-result/v1";
    pub const CONTOUR_PLAN_SUBCOMMAND: &str = "plan-contour-lattice";
    pub const CONTOUR_PLAN_REQUEST: &str = "ds.reporter.contour-lattice-request/v1";
    pub const CONTOUR_PLAN_RESULT: &str = "ds.reporter.contour-lattice-result/v1";
    pub const CONTOURS_SUBCOMMAND: &str = "derive-contours";
    pub const CONTOURS_REQUEST: &str = "ds.reporter.contours-request/v1";
    pub const CONTOURS_RESULT: &str = "ds.reporter.contours-result/v1";
    /// The footprint normalization every publisher row goes through.
    pub const FOOTPRINT_CONVERSION: &str = "ds.publisher-footprints/polygon-2d-v2";
    /// Contours are cut on fixed national-grid tiles of this side, so the
    /// same ground always yields the same line pieces whichever cell asked.
    pub const CONTOUR_TILE_M: f64 = 1_000.0;

    /// How the publisher laid its rows out.
    #[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum Publisher {
        /// Google Open Buildings v3 level-6 tiles: CSV without a header.
        GoogleOpenBuildingsV3,
        /// Google Open Buildings CSV with its header row.
        GoogleCsv,
        /// Microsoft Global ML Building Footprints GeoJSON lines.
        MicrosoftGeojsonl,
    }

    /// One verified publisher object, already on local disk.
    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct PublisherObject {
        pub object_id: String,
        pub path: PathBuf,
        pub sha256: String,
        pub size_bytes: u64,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct FootprintsRequest {
        pub schema: String,
        /// The room selection rule rows are admitted under.
        pub dataset_id: String,
        pub provider: String,
        pub publisher: Publisher,
        pub objects: Vec<PublisherObject>,
        /// One complete page is answered for each cell, in order.
        pub cells: Vec<[f64; 4]>,
        /// A fresh directory the engine writes the pages into.
        pub out_dir: PathBuf,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct Page {
        pub cell: [f64; 4],
        pub path: PathBuf,
        pub features: u64,
        pub sha256: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct FootprintsResult {
        pub schema: String,
        pub conversion: String,
        pub pages: Vec<Page>,
        pub raw_rows: u64,
        pub duplicates: u64,
        pub objects: Value,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct ContourPlanRequest {
        pub schema: String,
        pub cell: [f64; 4],
        pub sample_spacing_m: f64,
        /// Where the engine writes the WGS84 sample points, row-major.
        pub out_points: PathBuf,
    }

    /// A sampling lattice in the national grid (NIX ITRF2005 metres),
    /// anchored on whole multiples of the spacing.
    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct Lattice {
        pub origin: [f64; 2],
        pub spacing_m: f64,
        pub columns: usize,
        pub rows: usize,
        /// Fixed contour tiles `[ix, iy]` the lattice answers.
        pub tiles: Vec<[i64; 2]>,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct ContourPlanResult {
        pub schema: String,
        pub lattice: Lattice,
        pub points: PathBuf,
        pub point_count: usize,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct ContoursRequest {
        pub schema: String,
        pub dataset_id: String,
        pub provider: String,
        pub cell: [f64; 4],
        pub lattice: Lattice,
        /// Elevation per lattice node, row-major; `null` where the surface
        /// does not cover the node.
        pub samples: PathBuf,
        pub minor_interval_m: u16,
        pub index_interval_m: u16,
        /// The surface the samples came from, recorded verbatim.
        pub surface: Value,
        pub out: PathBuf,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    pub struct ContoursResult {
        pub schema: String,
        pub page: Page,
        pub intermediate_features: u64,
        pub index_features: u64,
        pub sampled_points: u64,
        pub unresolved_points: u64,
    }
}

const IJ_TO_POS: [[u64; 4]; 4] = [[0, 1, 3, 2], [0, 3, 1, 2], [2, 3, 1, 0], [2, 1, 3, 0]];
const POS_TO_ORIENTATION: [usize; 4] = [1, 0, 0, 3];

/// The S2 cell token holding a WGS84 point at `level` (0..=30): the
/// publisher's own tiling key, computed exactly as S2 defines it (quadratic
/// projection, Hilbert order) so a tile is named, never guessed.
pub fn s2_token(lat: f64, lng: f64, level: u8) -> String {
    let (phi, theta) = (lat.to_radians(), lng.to_radians());
    let (x, y, z) = (phi.cos() * theta.cos(), phi.cos() * theta.sin(), phi.sin());
    let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
    let mut face = if ax > ay {
        if ax > az {
            0
        } else {
            2
        }
    } else if ay > az {
        1
    } else {
        2
    };
    if [x, y, z][face] < 0.0 {
        face += 3;
    }
    let (u, v) = match face {
        0 => (y / x, z / x),
        1 => (-x / y, z / y),
        2 => (-x / z, -y / z),
        3 => (z / x, y / x),
        4 => (z / y, -x / y),
        _ => (-y / z, -x / z),
    };
    let st = |value: f64| {
        if value >= 0.0 {
            0.5 * (1.0 + 3.0 * value).sqrt()
        } else {
            1.0 - 0.5 * (1.0 - 3.0 * value).sqrt()
        }
    };
    const MAX: f64 = (1_u64 << 30) as f64;
    let ij = |value: f64| (st(value) * MAX).floor().clamp(0.0, MAX - 1.0) as u64;
    let (i, j) = (ij(u), ij(v));
    let mut orientation = face & 1;
    let mut position = 0_u64;
    for bit in (0..30).rev() {
        let quadrant = (((i >> bit) & 1) << 1 | ((j >> bit) & 1)) as usize;
        let pos = IJ_TO_POS[orientation][quadrant];
        position = position << 2 | pos;
        orientation ^= POS_TO_ORIENTATION[pos as usize];
    }
    let leaf = (face as u64) << 61 | position << 1 | 1;
    let lsb = 1_u64 << (2 * (30 - u32::from(level.min(30))));
    let id = (leaf & !(lsb.wrapping_mul(2).wrapping_sub(1))) | lsb;
    let hex = format!("{id:016x}");
    hex.trim_end_matches('0').to_owned()
}
