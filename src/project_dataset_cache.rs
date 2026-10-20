//! Coverage policy and dataset identity for the project reference-data cache.
//!
//! One project holds *bounded extracts* of canonical datasets instead of whole
//! national downloads. This module owns every decision that shape implies —
//! how far around a design we consider "the project", how disconnected design
//! clusters fuse, what is already held, what is still missing, and what exactly
//! one acquisition asked for. Hosts own storage, transport and UI; nothing here
//! performs I/O.
//!
//! Three separations are load-bearing and easy to get wrong:
//!
//! * **Room address vs acquisition identity.** A room is addressed by
//!   (principal, project, dataset, query parameters). The source *version* is
//!   part of an acquisition's identity, never of the address — otherwise a new
//!   provider version would silently orphan data the project already paid for.
//!   Freshness is a reported property; coverage is a geometric one.
//! * **Requested vs completed coverage.** Asking for an area is not holding it.
//!   Callers pass what completed; this module never infers completion.
//! * **Dataset independence.** Coverage answers are computed per dataset. There
//!   is no operation here that can make one dataset's extent speak for another.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::reference_cache::AreaSelection;

pub const SCHEMA: &str = "ds.project-dataset-cache/v1";

/// Bounds chosen so a large national project stays interactive. Each is a hard
/// refusal or an explicit, reported simplification — never a silent truncation.
pub const MAX_EXTENTS: usize = 5_000;
pub const MAX_GRID_CELLS: usize = 4_000_000;
pub const MAX_COVERAGE_CELLS: usize = 4_096;
/// A fused cluster is queried as one envelope only while that envelope stays
/// under this area. Above it the plan queries the members' own buffered
/// rectangles instead: a district of chained sites must never become one
/// district-sized provider request, which a paid or page-bounded source
/// answers with a bill, a truncation, or both.
pub const MAX_ENVELOPE_M2: f64 = 25_000_000.0;
/// Per-source receipt ceiling. A receipt is provenance, not payload: one over
/// this size is refused whole rather than trimmed to fit, because a truncated
/// receipt no longer proves what it claims.
pub const MAX_SOURCE_RECEIPT_BYTES: usize = 64 * 1024;
/// How much of a provider's own footprint identifier a refusal may quote. A
/// refusal is persisted as a room's `last_error`, so it carries a name, never a
/// payload.
const MAX_FOOTPRINT_REFERENCE_CHARS: usize = 64;
/// How long a recorded acquisition may stay pending before it is treated as
/// abandoned.
///
/// It has to be bounded. Pending work suppresses a dataset's readiness and
/// fences its area against a retry, so a killed tab or a lost worker would
/// otherwise leave a room "preparing" for ever. Thirty minutes is several times
/// the longest a governed acquisition can run — the tiler's own building fetch
/// deadline is four minutes — so expiry only ever lands on work that is gone.
pub const ACQUISITION_TICKET_TTL_MS: i64 = 30 * 60 * 1_000;
/// 1e-9° is ~0.1 mm. Rounding inputs once keeps every downstream decomposition
/// exact on the rounded values and makes digests reproducible across hosts.
const QUANTUM: f64 = 1e9;
const MAX_LATITUDE: f64 = 85.0;

fn round(value: f64) -> f64 {
    (value * QUANTUM).round() / QUANTUM
}

/// Quantising a buffered edge must never move it inward: half a quantum of
/// shrinkage is enough to make a "500 m" buffer measure 499.99995 m, and a
/// coverage that is fractionally short is a coverage that lies.
fn round_out(value: f64, outward_is_up: bool) -> f64 {
    let scaled = value * QUANTUM;
    if outward_is_up {
        scaled.ceil() / QUANTUM
    } else {
        scaled.floor() / QUANTUM
    }
}

/// Declared fidelity of what lands in the room. Vector tiles are a rendering
/// contract and may be lossy, so a tile extract is never allowed to pass itself
/// off as source-quality analysis data; the distinction is part of the room
/// address, so the two can never merge into one coverage.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Source,
    TileExtract,
}

/// Which transformers earn the wider isolated buffer.
///
/// The owner has not yet ruled between these. The value that produced a
/// coverage is recorded with it, so answering later widens or narrows a
/// *wanted* extent through the ordinary acquisition path instead of
/// invalidating anything already held.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IsolatedRule {
    /// Provisional default: only a transformer with no designed network — a
    /// bare point, or a zero-area extent — is treated as isolated.
    PointOnly,
    /// Any fused cluster holding exactly one transformer is treated as
    /// isolated, designed or not.
    SingleTransformerCluster,
}

/// The paper a design prints on, as far as the capture needs to know it:
/// the map frame's proportion (width over height) and the share of the
/// frame a fitted design may take once tables and the title block have
/// taken theirs. The sheet shows the frame's own shape scaled by the
/// design, so this — not a metre count — decides how much ground around
/// the design must be held.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SheetShape {
    pub aspect: f64,
    /// Usable share of the frame's width and height for the fitted design.
    pub usable: [f64; 2],
}

/// The landscape ISO sheet an LV drawing set prints on (A0 and A3 share
/// the proportion), with schedules on one side and the block on the other.
pub const LANDSCAPE_SHEET: SheetShape = SheetShape {
    aspect: 1.414,
    usable: [0.5, 0.9],
};
/// The same sheet turned: schedules and block along the bottom.
pub const PORTRAIT_SHEET: SheetShape = SheetShape {
    aspect: 0.707,
    usable: [0.9, 0.5],
};
/// Ground shown beyond the fitted frame, so a contour never ends inside it.
const CAPTURE_MARGIN: f64 = 1.05;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BufferPolicy {
    /// The floor around a designed extent, metres; the sheet shape usually
    /// asks for more.
    pub design_buffer_m: f64,
    pub isolated_buffer_m: f64,
    pub isolated_rule: IsolatedRule,
    /// True while `isolated_rule` is an unratified assumption. Hosts surface
    /// this; it never changes geometry.
    #[serde(default)]
    pub provisional: bool,
    /// The sheets a design prints on; the capture covers what every one of
    /// them shows, so a frame in either orientation is filled. Empty — the
    /// default, and absent on the wire — is the metre buffer alone: a
    /// policy authored without sheets reads back exactly as it was given.
    /// A seed that prints names its sheets (`both_orientations`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<SheetShape>,
}

/// The sheets an LV drawing set prints on, either way round.
pub fn both_orientations() -> Vec<SheetShape> {
    vec![LANDSCAPE_SHEET, PORTRAIT_SHEET]
}

impl Default for BufferPolicy {
    fn default() -> Self {
        Self {
            design_buffer_m: 200.,
            isolated_buffer_m: 1000.,
            isolated_rule: IsolatedRule::PointOnly,
            provisional: false,
            sheets: Vec::new(),
        }
    }
}

pub fn buffer_policy(requested: &[f64]) -> Result<BufferPolicy, String> {
    let mut policy = BufferPolicy::default();
    for value in requested {
        if !value.is_finite() || !(0.0..=50_000.0).contains(value) {
            return Err("requested buffer must be between 0 and 50000 metres".into());
        }
        policy.design_buffer_m = policy.design_buffer_m.max(*value);
    }
    policy.isolated_buffer_m = policy.isolated_buffer_m.max(policy.design_buffer_m);
    Ok(policy)
}

impl BufferPolicy {
    fn validate(&self) -> Result<(), String> {
        for meters in [self.design_buffer_m, self.isolated_buffer_m] {
            if !meters.is_finite() || !(0.0..=50_000.0).contains(&meters) {
                return Err("buffer distances must be between 0 and 50000 metres".into());
            }
        }
        if self.sheets.len() > 8
            || self.sheets.iter().any(|sheet| {
                !(0.2..=5.0).contains(&sheet.aspect)
                    || sheet
                        .usable
                        .iter()
                        .any(|share| !(0.1..=1.0).contains(share))
            })
        {
            return Err("sheet shapes need an aspect 0.2-5 and usable shares 0.1-1".into());
        }
        Ok(())
    }
}

/// The ground a designed extent needs held: the metre buffer as a floor,
/// and for every sheet the frame's shape scaled by the design — a design
/// fitted into the usable share of the frame leaves the rest of the frame
/// showing context, and that context must be there.
pub(crate) fn capture(rect: Rect, metres: f64, sheets: &[SheetShape]) -> Rect {
    let mut out = buffered(rect, metres);
    if rect.degenerate() {
        return out;
    }
    let mid = (rect.s + rect.n) / 2.0;
    let width_m = (rect.e - rect.w) * metres_per_degree_longitude(mid);
    let height_m = (rect.n - rect.s) * metres_per_degree_latitude(mid);
    for sheet in sheets {
        // Ground per unit of frame height when the design fills its usable share.
        let per_unit = (width_m / (sheet.usable[0] * sheet.aspect)).max(height_m / sheet.usable[1]);
        let shown_w = sheet.aspect * per_unit * CAPTURE_MARGIN;
        let shown_h = per_unit * CAPTURE_MARGIN;
        let half_w = (shown_w / 2.0) / metres_per_degree_longitude(mid);
        let half_h = (shown_h / 2.0) / metres_per_degree_latitude(mid);
        let centre = [(rect.w + rect.e) / 2.0, mid];
        let sheet_rect = Rect {
            w: round_out((centre[0] - half_w).max(-180.0), false),
            s: round_out((centre[1] - half_h).max(-MAX_LATITUDE), false),
            e: round_out((centre[0] + half_w).min(180.0), true),
            n: round_out((centre[1] + half_h).min(MAX_LATITUDE), true),
        };
        out = out.union(sheet_rect);
    }
    out
}

/// The canonical dataset a room holds an extract of.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Dataset {
    /// Canonical dataset id chosen by the project's workflow requirement, never
    /// "every row in the catalog".
    pub id: String,
    /// The governed door acquisition goes through.
    pub provider: String,
    pub quality: Quality,
    /// Provider-owned filters and thresholds, echoed verbatim. This module does
    /// not invent, default or reinterpret a single one of them.
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
    /// Provider version/receipt for the rows held. Empty before first
    /// completion. Deliberately excluded from the room address.
    #[serde(default)]
    pub source_version: String,
    #[serde(default = "default_crs")]
    pub crs: String,
}

fn default_crs() -> String {
    "EPSG:4326".into()
}

impl Dataset {
    fn validate(&self) -> Result<(), String> {
        if !bounded_token(&self.id) || !bounded_token(&self.provider) {
            return Err("dataset id and provider must be short printable tokens".into());
        }
        if self.crs != "EPSG:4326" {
            return Err("project dataset coverage is WGS84 (EPSG:4326) only".into());
        }
        if self.source_version.len() > 512 {
            return Err("dataset source version is too long".into());
        }
        if self.parameters.len() > 64 {
            return Err("dataset carries too many query parameters".into());
        }
        Ok(())
    }
}

fn bounded_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'"' && byte != b'\\')
}

/// Where a room lives. Not an identity of what it holds.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub principal: String,
    pub project: String,
}

impl Scope {
    fn validate(&self) -> Result<(), String> {
        if self.principal.trim().is_empty() || self.project.trim().is_empty() {
            return Err("a project dataset room needs both a principal and a project".into());
        }
        if self.principal.len() > 256 || self.project.len() > 256 {
            return Err("principal or project identifier is too long".into());
        }
        Ok(())
    }
}

/// One transformer's design extent. `bounds` is absent for a transformer that
/// has a location but no designed network.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Extent {
    pub name: String,
    #[serde(default)]
    pub bounds: Option<[f64; 4]>,
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
    fn from_rects(rects: Vec<Rect>) -> Self {
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
    fn digest(&self) -> String {
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
    fn from_bounds(bounds: [f64; 4]) -> Result<Self, String> {
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
    fn bounds(self) -> [f64; 4] {
        [self.w, self.s, self.e, self.n]
    }
    fn degenerate(self) -> bool {
        self.w >= self.e || self.s >= self.n
    }
    fn contains(self, x: f64, y: f64) -> bool {
        self.w <= x && x <= self.e && self.s <= y && y <= self.n
    }
    fn intersects(self, other: Self) -> bool {
        self.w <= other.e && other.w <= self.e && self.s <= other.n && other.s <= self.n
    }
    fn union(self, other: Self) -> Self {
        Self {
            w: self.w.min(other.w),
            s: self.s.min(other.s),
            e: self.e.max(other.e),
            n: self.n.max(other.n),
        }
    }
}

// WGS84 local scale. Accurate to well under a metre at project latitudes, and
// applied conservatively below so a requested buffer is never short.
fn metres_per_degree_latitude(latitude: f64) -> f64 {
    let phi = latitude.to_radians();
    111_132.92 - 559.82 * (2.0 * phi).cos() + 1.175 * (4.0 * phi).cos() - 0.0023 * (6.0 * phi).cos()
}

fn metres_per_degree_longitude(latitude: f64) -> f64 {
    let phi = latitude.to_radians();
    111_412.84 * phi.cos() - 93.5 * (3.0 * phi).cos() + 0.118 * (5.0 * phi).cos()
}

/// Expand a rectangle by a metric distance, erring outward.
///
/// Degrees are not metres and the conversion is latitude-dependent, so each
/// axis uses the *worst* scale over the span it will occupy: the latitude
/// nearest the equator for the north/south growth (where a degree is shortest)
/// and the highest latitude reached for east/west (where a degree is
/// narrowest). The result is guaranteed to be at least `metres` wide.
pub(crate) fn buffered(rect: Rect, metres: f64) -> Rect {
    if metres <= 0.0 {
        return rect;
    }
    let nearest_equator = if rect.s <= 0.0 && rect.n >= 0.0 {
        0.0
    } else {
        rect.s.abs().min(rect.n.abs())
    };
    let latitude_degrees = metres / metres_per_degree_latitude(nearest_equator);
    let s = (rect.s - latitude_degrees).max(-MAX_LATITUDE);
    let n = (rect.n + latitude_degrees).min(MAX_LATITUDE);
    let furthest = s.abs().max(n.abs());
    let scale = metres_per_degree_longitude(furthest);
    let (w, e) = if scale <= 1.0 {
        (-180.0, 180.0)
    } else {
        let longitude_degrees = metres / scale;
        if longitude_degrees >= 180.0 {
            (-180.0, 180.0)
        } else {
            (
                (rect.w - longitude_degrees).max(-180.0),
                (rect.e + longitude_degrees).min(180.0),
            )
        }
    };
    Rect {
        w: round_out(w, false),
        s: round_out(s, false),
        e: round_out(e, true),
        n: round_out(n, true),
    }
}

/// Group rectangles whose buffered coverage touches or overlaps.
///
/// This is what stops one project-wide envelope from spanning unrelated sites.
/// Groups are merged to a fixed point on their envelopes, so the envelopes this
/// returns are pairwise disjoint.
fn clusters(rects: &[Rect]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..rects.len()).collect();
    fn find(parent: &mut [usize], mut node: usize) -> usize {
        while parent[node] != node {
            parent[node] = parent[parent[node]];
            node = parent[node];
        }
        node
    }
    for a in 0..rects.len() {
        for b in (a + 1)..rects.len() {
            if rects[a].intersects(rects[b]) {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    parent[ra] = rb;
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..rects.len() {
        let root = find(&mut parent, index);
        groups.entry(root).or_default().push(index);
    }
    let mut result: Vec<Vec<usize>> = groups.into_values().collect();
    // Two groups can still have overlapping envelopes even when no member
    // rectangles touch (an L-shaped group and a small one in its notch). Merge
    // on envelopes until stable so the reported clusters are truly disjoint.
    for _ in 0..result.len().max(1) {
        let envelopes: Vec<Rect> = result
            .iter()
            .map(|members| {
                members
                    .iter()
                    .map(|&index| rects[index])
                    .reduce(Rect::union)
                    .expect("a cluster always holds at least one rectangle")
            })
            .collect();
        let mut merged = false;
        'outer: for a in 0..envelopes.len() {
            for b in (a + 1)..envelopes.len() {
                if envelopes[a].intersects(envelopes[b]) {
                    let moved = result.remove(b);
                    result[a].extend(moved);
                    result[a].sort_unstable();
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            break;
        }
    }
    result.sort_by_key(|members| members.first().copied().unwrap_or(0));
    result
}

fn axis(include: &[Rect], exclude: &[Rect], low: f64, high: f64, horizontal: bool) -> Vec<f64> {
    let mut values = vec![low, high];
    for rect in include.iter().chain(exclude.iter()) {
        let (a, b) = if horizontal {
            (rect.w, rect.e)
        } else {
            (rect.s, rect.n)
        };
        for value in [a, b] {
            if value > low && value < high {
                values.push(value);
            }
        }
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values.dedup();
    values
}

/// Maximal disjoint rectangles covering `include` minus `exclude`, exactly.
///
/// Every edge of every input becomes a grid line, so a cell is wholly inside or
/// wholly outside each input and its midpoint decides it. Covered cells are
/// merged into runs and runs into rectangles, which keeps the output canonical
/// rather than one rectangle per cell.
fn decompose(include: &[Rect], exclude: &[Rect]) -> Result<(Vec<Rect>, bool), String> {
    let live: Vec<Rect> = include
        .iter()
        .copied()
        .filter(|rect| !rect.degenerate())
        .collect();
    let Some(envelope) = live.iter().copied().reduce(Rect::union) else {
        return Ok((Vec::new(), false));
    };
    let relevant: Vec<Rect> = exclude
        .iter()
        .copied()
        .filter(|rect| !rect.degenerate() && rect.intersects(envelope))
        .collect();
    let xs = axis(&live, &relevant, envelope.w, envelope.e, true);
    let ys = axis(&live, &relevant, envelope.s, envelope.n, false);
    let (nx, ny) = (xs.len().saturating_sub(1), ys.len().saturating_sub(1));
    if nx == 0 || ny == 0 {
        return Ok((Vec::new(), false));
    }
    if nx.saturating_mul(ny) > MAX_GRID_CELLS {
        // Refusing outright would strand a large project; instead report the
        // conservative envelope and say so. Callers never see a silent answer.
        return Ok((vec![envelope], true));
    }
    let mut covered = vec![false; nx * ny];
    for j in 0..ny {
        let y = (ys[j] + ys[j + 1]) / 2.0;
        for i in 0..nx {
            let x = (xs[i] + xs[i + 1]) / 2.0;
            covered[j * nx + i] = live.iter().any(|rect| rect.contains(x, y))
                && !relevant.iter().any(|rect| rect.contains(x, y));
        }
    }
    let mut output: Vec<Rect> = Vec::new();
    let mut open: Vec<(usize, usize, usize)> = Vec::new();
    for j in 0..=ny {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        if j < ny {
            let mut start: Option<usize> = None;
            for i in 0..nx {
                match (covered[j * nx + i], start) {
                    (true, None) => start = Some(i),
                    (false, Some(from)) => {
                        runs.push((from, i - 1));
                        start = None;
                    }
                    _ => (),
                }
            }
            if let Some(from) = start {
                runs.push((from, nx - 1));
            }
        }
        let mut next: Vec<(usize, usize, usize)> = Vec::new();
        for (from, to, since) in open.drain(..) {
            if runs.iter().any(|&(a, b)| a == from && b == to) {
                next.push((from, to, since));
            } else {
                output.push(Rect {
                    w: xs[from],
                    s: ys[since],
                    e: xs[to + 1],
                    n: ys[j],
                });
            }
        }
        for &(from, to) in &runs {
            if !next.iter().any(|&(a, b, _)| a == from && b == to) {
                next.push((from, to, j));
            }
        }
        open = next;
    }
    if output.len() > MAX_COVERAGE_CELLS {
        return Ok((vec![envelope], true));
    }
    output.sort_by(|a, b| {
        a.bounds()
            .partial_cmp(&b.bounds())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok((output, false))
}

fn digest_of(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}

/// Key-sorted JSON so a digest depends on content, never on map order.
fn canonical(value: &Value) -> String {
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

/// Content identity of one acquired feature.
///
/// Overlapping acquisitions are the normal case — two adjacent transformers
/// legitimately ask for overlapping areas — so a store needs a stable way to
/// recognise a row it already holds. Providers do not promise a stable id, so
/// the whole canonical feature is the key.
pub fn feature_key(feature: &Value) -> String {
    digest_of(feature)
}

/// Address of the room holding this dataset for this project.
///
/// Source version is deliberately absent: a provider re-publication must not
/// orphan an extract the project already holds.
pub fn dataset_key(scope: &Scope, dataset: &Dataset) -> String {
    digest_of(&json!({
        "schema": SCHEMA,
        "principal": scope.principal,
        "project": scope.project,
        "dataset": dataset.id,
        "provider": dataset.provider,
        "quality": dataset.quality,
        "parameters": dataset.parameters,
        "crs": dataset.crs,
    }))
}

/// Identity of one acquisition: everything the room address covers, plus the
/// source version and the exact geometry asked for.
pub fn query_digest(
    scope: &Scope,
    dataset: &Dataset,
    policy: &BufferPolicy,
    coverage: &Coverage,
) -> String {
    digest_of(&json!({
        "schema": SCHEMA,
        "dataset_key": dataset_key(scope, dataset),
        "source_version": dataset.source_version,
        "buffer_policy": policy,
        "coverage": coverage.cells,
    }))
}

#[derive(Debug, Serialize)]
pub struct Cluster {
    pub members: Vec<String>,
    pub buffer_m: f64,
    pub envelope: [f64; 4],
    pub isolated: bool,
    /// True when the envelope exceeded `MAX_ENVELOPE_M2` and the plan queries
    /// the members' own rectangles instead.
    pub split: bool,
    /// Query cells this cluster contributed after any focus narrowing.
    pub cells: usize,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub schema: &'static str,
    pub dataset_key: String,
    pub query_digest: String,
    pub coverage_digest: String,
    pub buffer_policy: BufferPolicy,
    pub clusters: Vec<Cluster>,
    pub coverage: Coverage,
    pub coverage_geometry: Option<Value>,
    pub extent_count: usize,
    pub located_count: usize,
    pub skipped: Vec<String>,
    pub empty: bool,
}

#[derive(Debug, Serialize)]
pub struct Assessment {
    pub schema: &'static str,
    pub covered: bool,
    pub gaps: Coverage,
    pub gap_geometry: Option<Value>,
    pub gap_count: usize,
    /// Held coverage no longer wanted — a deleted or shrunken design. Reported
    /// so a host can offer to reclaim it; never destroyed here, and never a
    /// reason to refetch.
    pub surplus: Coverage,
    pub surplus_count: usize,
    pub simplified: bool,
}

fn buffer_for(rule: IsolatedRule, policy: &BufferPolicy, designed: bool) -> f64 {
    match rule {
        IsolatedRule::PointOnly if !designed => policy.isolated_buffer_m,
        _ => policy.design_buffer_m,
    }
}

/// Fuse every transformer's buffered design extent into the project's coverage.
pub fn plan(
    scope: &Scope,
    dataset: &Dataset,
    policy: &BufferPolicy,
    extents: &[Extent],
) -> Result<Plan, String> {
    plan_focused(scope, dataset, policy, extents, &[])
}

fn rect_area_m2(rect: Rect) -> f64 {
    let mid = ((rect.s + rect.n) / 2.0).abs();
    let height = (rect.n - rect.s).max(0.0) * metres_per_degree_latitude(mid);
    let width = (rect.e - rect.w).max(0.0) * metres_per_degree_longitude(mid);
    height * width
}

/// The area of one `[west, south, east, north]` rectangle, in square metres,
/// measured the way coverage is planned. A cloud-resident read
/// (`docs/contracts/foundation-datasets.md` R3) is bounded by
/// [`MAX_ENVELOPE_M2`] through this: the same line that stops a district of
/// chained sites from becoming one provider request stops a rectangle of
/// customers from becoming a table pull.
pub fn bounds_area_m2(bounds: [f64; 4]) -> Result<f64, String> {
    Ok(rect_area_m2(Rect::from_bounds(bounds)?))
}

/// `plan`, narrowed to the clusters that contain a transformer named in
/// `focus`. A print asks for its own transformer only: it must not pay for,
/// or wait on, the rest of the project. An empty `focus` keeps everything.
pub fn plan_focused(
    scope: &Scope,
    dataset: &Dataset,
    policy: &BufferPolicy,
    extents: &[Extent],
    focus: &[String],
) -> Result<Plan, String> {
    scope.validate()?;
    dataset.validate()?;
    policy.validate()?;
    if extents.len() > MAX_EXTENTS {
        return Err(format!(
            "a project coverage plan reads at most {MAX_EXTENTS} transformer extents"
        ));
    }
    let mut named: Vec<(String, Rect, bool)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for extent in extents {
        match extent.bounds {
            Some(bounds) => {
                let rect = Rect::from_bounds(bounds)?;
                let designed = bounds[0] != bounds[2] || bounds[1] != bounds[3];
                named.push((extent.name.clone(), rect, designed));
            }
            None => skipped.push(extent.name.clone()),
        }
    }
    // Under the single-transformer-cluster rule the buffer depends on the
    // clustering, which depends on the buffer. Cluster once at the design
    // buffer, widen the singletons, then re-cluster the widened set.
    let mut buffers: Vec<f64> = named
        .iter()
        .map(|(_, _, designed)| buffer_for(policy.isolated_rule, policy, *designed))
        .collect();
    if policy.isolated_rule == IsolatedRule::SingleTransformerCluster {
        let first: Vec<Rect> = named
            .iter()
            .zip(&buffers)
            .map(|((_, rect, _), &metres)| buffered(*rect, metres))
            .collect();
        for group in clusters(&first) {
            if group.len() == 1 {
                buffers[group[0]] = policy.isolated_buffer_m;
            }
        }
    }
    let expanded: Vec<Rect> = named
        .iter()
        .zip(&buffers)
        .map(|((_, rect, designed), &metres)| {
            if *designed {
                capture(*rect, metres, &policy.sheets)
            } else {
                buffered(*rect, metres)
            }
        })
        .collect();
    let groups = clusters(&expanded);
    let mut cells: Vec<Rect> = Vec::new();
    let mut reported: Vec<Cluster> = Vec::new();
    let in_focus =
        |index: usize| focus.is_empty() || focus.iter().any(|name| *name == named[index].0);
    for group in groups {
        let envelope = group
            .iter()
            .map(|&index| expanded[index])
            .reduce(Rect::union)
            .expect("a cluster always holds at least one rectangle");
        let wanted: Vec<usize> = group
            .iter()
            .copied()
            .filter(|&index| in_focus(index))
            .collect();
        if wanted.is_empty() {
            continue;
        }
        // One query area per fused cluster while its envelope stays small: the
        // concave over-fetch is cheaper than O(members) provider round trips.
        // Past MAX_ENVELOPE_M2 that trade inverts — chained sites would turn
        // into one district-sized request — so the members' own rectangles
        // become the cells. Cross-cluster fusion never happens either way;
        // `clusters` guarantees disjoint envelopes.
        let split = rect_area_m2(envelope) > MAX_ENVELOPE_M2;
        let cluster_cells: Vec<Rect> = if split {
            wanted.iter().map(|&index| expanded[index]).collect()
        } else {
            vec![envelope]
        };
        reported.push(Cluster {
            members: group.iter().map(|&index| named[index].0.clone()).collect(),
            buffer_m: group
                .iter()
                .map(|&index| buffers[index])
                .fold(0.0_f64, f64::max),
            envelope: envelope.bounds(),
            isolated: group.len() == 1,
            split,
            cells: cluster_cells.len(),
        });
        cells.extend(cluster_cells);
    }
    // Clusters are disjoint by construction, so no cross-cluster merge is
    // needed; a final canonicalisation keeps the digest order-independent.
    let coverage = Coverage::from_rects(cells);
    if coverage.cells.len() > MAX_COVERAGE_CELLS {
        return Err("project coverage exceeds its cell limit".into());
    }
    Ok(Plan {
        schema: SCHEMA,
        dataset_key: dataset_key(scope, dataset),
        query_digest: query_digest(scope, dataset, policy, &coverage),
        coverage_digest: coverage.digest(),
        buffer_policy: policy.clone(),
        clusters: reported,
        coverage_geometry: coverage.geometry(),
        extent_count: extents.len(),
        located_count: named.len(),
        empty: coverage.is_empty(),
        coverage,
        skipped,
    })
}

/// Compare completed coverage against wanted coverage for ONE dataset.
///
/// `held` must be what an acquisition actually *completed*, never what it
/// requested. A complete acquisition that found nothing still counts as held —
/// asking again would spend money to learn the same emptiness.
pub fn assess(held: &Coverage, wanted: &Coverage) -> Result<Assessment, String> {
    let held_rects = held.rects()?;
    let wanted_rects = wanted.rects()?;
    let (gaps, gap_simplified) = decompose(&wanted_rects, &held_rects)?;
    let (surplus, surplus_simplified) = decompose(&held_rects, &wanted_rects)?;
    let gaps = Coverage::from_rects(gaps);
    let surplus = Coverage::from_rects(surplus);
    Ok(Assessment {
        schema: SCHEMA,
        covered: gaps.is_empty(),
        gap_geometry: gaps.geometry(),
        gap_count: gaps.cells.len(),
        surplus_count: surplus.cells.len(),
        simplified: gap_simplified || surplus_simplified,
        gaps,
        surplus,
    })
}

/// Fold a completed acquisition into held coverage.
pub fn extend(held: &Coverage, completed: &Coverage) -> Result<Coverage, String> {
    let mut rects = held.rects()?;
    rects.extend(completed.rects()?);
    let (merged, simplified) = decompose(&rects, &[])?;
    if simplified {
        return Err(
            "exact completed coverage exceeds its bounded representation; split or compact the project dataset room"
                .into(),
        );
    }
    Ok(Coverage::from_rects(merged))
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
    if crate::reference_cache::selects_area(feature, area, selection)? {
        return Ok(true);
    }
    if selection != AreaSelection::Intersects || provider == REFERENCE_CACHE_PROVIDER {
        return Ok(false);
    }
    // Within the margin: the feature's bounds, grown by the margin, meet the
    // area. Bounds are a superset of the geometry, so this admits nothing the
    // exact test would refuse by more than the margin plus the feature's own
    // extent — a nearby building, never a distant one.
    let Some([w, s, e, n]) = crate::reference_cache::feature_bounds(feature) else {
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
    crate::reference_cache::selects_area(&grown, area, AreaSelection::Intersects)
}

/// Reject an oversized or malformed source receipt outright.
///
/// A refusal, never a truncation: half a receipt would still be stored beside
/// the rows as though it accounted for them.
pub fn validate_source_receipt(receipt: &Value) -> Result<(), String> {
    if !receipt.is_object() {
        return Err("a source receipt must be a JSON object".into());
    }
    let bytes = serde_json::to_vec(receipt)
        .map_err(|error| error.to_string())?
        .len();
    if bytes > MAX_SOURCE_RECEIPT_BYTES {
        return Err(format!(
            "source receipt is {bytes} bytes, over the {MAX_SOURCE_RECEIPT_BYTES}-byte bound; record a reference to the provider response rather than the response"
        ));
    }
    Ok(())
}

/// Whether a pending acquisition recorded at `requested_at` has been abandoned.
pub fn acquisition_expired(requested_at: i64, now: i64) -> bool {
    now.saturating_sub(requested_at) > ACQUISITION_TICKET_TTL_MS
}

/// Reported state of one recorded acquisition.
///
/// `expired` is deliberately not `failed`: nothing is known to have gone wrong
/// and nothing was written — the attempt simply stopped answering. Reporting it
/// as pending is what makes a count spin for ever.
pub fn acquisition_state(
    status: &str,
    requested_at: i64,
    now: i64,
) -> Result<&'static str, String> {
    match status {
        "requested" if acquisition_expired(requested_at, now) => Ok("expired"),
        "requested" => Ok("requested"),
        "completed" => Ok("completed"),
        "failed" => Ok("failed"),
        other => Err(format!("unknown acquisition status `{other}`")),
    }
}

/// One completed acquisition's provenance: the coverage it still accounts for,
/// and the source version it completed under.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeldVersion {
    #[serde(default)]
    pub source_version: String,
    #[serde(default)]
    pub coverage: Coverage,
}

#[derive(Debug, Serialize)]
pub struct Freshness {
    pub schema: &'static str,
    /// The newest source version this room has observed from the provider.
    pub current: String,
    /// Every distinct recorded version still accounted for by held coverage.
    pub versions: Vec<String>,
    /// Held coverage completed under a version the provider has since replaced.
    /// This is what an explicit refresh re-acquires; it is never deleted here.
    pub stale: Coverage,
    pub stale_geometry: Option<Value>,
    pub stale_count: usize,
    /// Held coverage carrying no recorded version. Not obsolete: there is no
    /// evidence either way, and inventing some would spend provider money.
    pub unattributed: Coverage,
    pub unattributed_count: usize,
    /// True when nothing held is known to have been superseded.
    pub fresh: bool,
    pub simplified: bool,
}

/// Why a room is, or is not, ready — decided once
/// (`docs/contracts/foundation-datasets.md` R5).
///
/// `ready` was always the same conjunction — the index answers, nothing is
/// mid-flight, something completed, and completed covers requested — but only
/// the boolean left the store, so a held, indexed, fresh room reporting
/// `ready:false` gave an operator nothing to act on. The reasons are tested in
/// this order and the first failing one is named; a ready room names none.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Readiness {
    pub ready: bool,
    /// `index_absent` | `index_incomplete` | `acquisition_pending` |
    /// `nothing_completed` | `coverage_gap` | `acquisition_failed`, or `None`.
    pub ready_reason: Option<String>,
}

/// Readiness reasons a row can carry without a room: a dataset the catalogue
/// lists but nobody seeded, a cloud-resident dataset that is never held here,
/// and a bundle the catalogue has not published.
pub const READY_REASON_NOT_SEEDED: &str = "not_seeded";
pub const READY_REASON_CLOUD_RESIDENT: &str = "cloud_resident";
pub const READY_REASON_BUNDLE_UNPUBLISHED: &str = "bundle_unpublished";

pub fn readiness(
    index_state: &str,
    pending_queries: i64,
    completed: &Coverage,
    requested: &Coverage,
    last_error: Option<&str>,
) -> Result<Readiness, String> {
    let reason = if index_state == "absent" {
        Some("index_absent".to_owned())
    } else if index_state == "incomplete" {
        Some("index_incomplete".to_owned())
    } else if pending_queries > 0 {
        Some(format!("acquisition_pending ({pending_queries})"))
    } else if completed.is_empty() {
        Some("nothing_completed".to_owned())
    } else {
        let assessment = assess(completed, requested)?;
        if !assessment.covered {
            Some(format!("coverage_gap ({} cells)", assessment.gap_count))
        } else {
            None
        }
    };
    // A failed acquisition that still left the room covered is reported as a
    // fact, not as unreadiness: held data answers, and the error is on the row.
    let reason = match (reason, last_error) {
        (Some(reason), Some(error)) if reason == "nothing_completed" && !error.is_empty() => {
            Some(format!("acquisition_failed: {}", quoted(error)))
        }
        (reason, _) => reason,
    };
    Ok(Readiness {
        ready: reason.is_none(),
        ready_reason: reason,
    })
}

/// Report which held coverage was completed under an obsolete source version.
///
/// Freshness is a property of held rows, never an instruction. This returns the
/// coverage an explicit refresh would re-acquire and destroys nothing: a room
/// that is out of date still answers every read it could answer before, because
/// stale data is data the project paid for and a deleted row helps nobody.
///
/// Two abstentions are deliberate. With no `current` version known there is no
/// evidence of a newer publication, so nothing is obsolete; and a holding with
/// no recorded version is reported `unattributed` rather than assumed stale.
pub fn freshness(current: &str, held: &[HeldVersion]) -> Result<Freshness, String> {
    if current.len() > 512 {
        return Err("dataset source version is too long".into());
    }
    let mut versions: BTreeSet<String> = BTreeSet::new();
    let mut stale_rects: Vec<Rect> = Vec::new();
    let mut unattributed_rects: Vec<Rect> = Vec::new();
    for holding in held {
        let rects = holding.coverage.rects()?;
        if rects.is_empty() {
            continue;
        }
        if holding.source_version.is_empty() {
            unattributed_rects.extend(rects);
            continue;
        }
        versions.insert(holding.source_version.clone());
        if crate::freshness::compare(
            Some(holding.source_version.as_str()),
            (!current.is_empty()).then_some(current),
        ) == crate::freshness::Freshness::Stale
        {
            stale_rects.extend(rects);
        }
    }
    let (stale, stale_simplified) = decompose(&stale_rects, &[])?;
    let (unattributed, unattributed_simplified) = decompose(&unattributed_rects, &[])?;
    let stale = Coverage::from_rects(stale);
    let unattributed = Coverage::from_rects(unattributed);
    Ok(Freshness {
        schema: SCHEMA,
        current: current.to_owned(),
        versions: versions.into_iter().collect(),
        stale_geometry: stale.geometry(),
        stale_count: stale.cells.len(),
        unattributed_count: unattributed.cells.len(),
        fresh: stale.is_empty(),
        simplified: stale_simplified || unattributed_simplified,
        stale,
        unattributed,
    })
}

/// The code a host raises when a seed has nothing to act on.
///
/// Typed rather than a sentence: the operator-facing wording belongs to the
/// surface that refuses (the CLI names `--dataset`, a card would name a
/// button), and both must be refusing the same thing.
pub const SEED_SCOPE_REFUSAL: &str = "project_dataset_scope_invalid";
/// How many datasets one seed may be offered. A project declares a workflow's
/// worth of context layers, never a catalogue's.
pub const MAX_SEED_CANDIDATES: usize = 512;
const MAX_SEED_LABEL_CHARS: usize = 256;
const MAX_DATASET_ID_CHARS: usize = 128;

/// A dataset a project declares, or one it already holds.
///
/// `label` is what a host displays for it, and it is what orders a seed: the
/// order datasets are acquired in is a decision, so it is made here instead of
/// falling out of a host's map iteration or a catalogue's publication order.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SeedCandidate {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// A room already exists for this dataset. Meaningless on a declaration,
    /// which is what the project needs rather than what this machine has.
    #[serde(default)]
    pub seeded: bool,
}

fn dataset_reference(id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty()
        || id.chars().count() > MAX_DATASET_ID_CHARS
        || id.chars().any(char::is_control)
    {
        return Err("a seed target needs one bounded dataset id".into());
    }
    Ok(id.to_owned())
}

/// Which datasets one `seed` acts on, in the order it must acquire them.
///
/// Naming a dataset explicitly is the act of adding one the project has not
/// declared, so an explicit name is the whole answer — nothing else is seeded
/// beside it, and nothing validates it here beyond its shape, because whether
/// a host can hold it is a property of that host's registry.
///
/// Without one, the answer is what the project **declares** (its workflow's
/// own context datasets) UNION what it already **holds**. The union is the
/// decision:
///
/// * Declared alone would drop a dataset an operator deliberately added with
///   `--dataset`, silently stopping its refresh.
/// * Held alone is what this used to be, and it could never be satisfied on a
///   machine that held nothing — first use, the one case where an operator
///   most needs the command, refused itself.
/// * Neither is "everything published for this country": a dataset the project
///   does not declare and does not hold is never introduced by accident.
///
/// A held row that was never seeded is a dataset this project *could* hold,
/// not one it holds, and enters a seed only by being declared. Nothing
/// declared and nothing held is a typed refusal, never an empty success: a run
/// that acquired nothing must not read as a completed seed.
pub fn seed_targets(
    declared: &[SeedCandidate],
    held: &[SeedCandidate],
    explicit: &str,
) -> Result<Vec<String>, String> {
    let explicit = explicit.trim();
    if !explicit.is_empty() {
        return Ok(vec![dataset_reference(explicit)?]);
    }
    if declared.len() > MAX_SEED_CANDIDATES || held.len() > MAX_SEED_CANDIDATES {
        return Err(format!(
            "more than {MAX_SEED_CANDIDATES} datasets were offered to one seed"
        ));
    }
    let mut targets: Vec<(usize, String, String)> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (group, candidates) in [(0usize, declared), (1usize, held)] {
        for candidate in candidates {
            if group == 1 && !candidate.seeded {
                continue;
            }
            let id = dataset_reference(&candidate.id)?;
            let label = candidate.label.trim();
            if label.chars().count() > MAX_SEED_LABEL_CHARS {
                return Err("a seed target's label exceeds its bound".into());
            }
            if !seen.insert(id.clone()) {
                continue;
            }
            let order = if label.is_empty() {
                id.clone()
            } else {
                label.to_owned()
            };
            targets.push((group, order, id));
        }
    }
    if targets.is_empty() {
        return Err(SEED_SCOPE_REFUSAL.into());
    }
    targets.sort();
    Ok(targets.into_iter().map(|(_, _, id)| id).collect())
}

/// How one provider row — a building footprint, a contour line — is named
/// in a refusal.
///
/// The provider's own stable identifier when it carries one, otherwise the
/// feature's position in the response. Bounded and stripped of control
/// characters: this string is persisted as the room's `last_error` and read by
/// an operator, so it can neither be unbounded nor carry line breaks.
fn feature_reference(index: usize, feature: &Value) -> String {
    let identifier = feature
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| feature["properties"].get("full_plus_code")?.as_str())
        .or_else(|| feature["properties"].get("id")?.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match identifier {
        Some(identifier) => format!("#{index} `{}`", quoted(identifier)),
        None => format!("#{index}"),
    }
}

/// A provider-supplied string as a refusal may quote it.
///
/// Bounded in CHARACTERS and stripped of control characters. Both halves are
/// load-bearing: every string quoted here came from a provider response, so
/// slicing one by bytes would panic the moment a multi-byte character crossed
/// the bound, and a line break in it would break the status line a refusal is
/// read in.
fn quoted(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_FOOTPRINT_REFERENCE_CHARS)
        .collect()
}

fn bounds_text(bounds: [f64; 4]) -> String {
    format!(
        "[{:.6}, {:.6}, {:.6}, {:.6}]",
        bounds[0], bounds[1], bounds[2], bounds[3]
    )
}

/// Decode one bounded cloud read (foundation-datasets contract R3) as a
/// seeding page: the receipt must be complete for the cell — a truncated
/// answer (more rows than the cap inside one cell) can never extend coverage,
/// because a room that holds part of a cell would read as covered.
pub fn decode_cloud_read(response: &Value, area: &Value, layer: &str) -> Result<Value, String> {
    crate::reference_cache::validate_area(area)?;
    if response["dataset"]["layer"].as_str() != Some(layer) {
        return Err(format!("the cloud read did not answer for {layer}"));
    }
    if response["dataset"]["residency"].as_str() != Some("cloud") {
        return Err("the cloud read's dataset is not cloud-resident".into());
    }
    let features = response["features"]
        .as_array()
        .ok_or("the cloud read has no feature collection")?;
    let returned = response["rows_returned"].as_u64();
    let total = response["rows_total"].as_u64();
    if returned != Some(features.len() as u64) {
        return Err("the cloud read's row count does not match its features".into());
    }
    if response["truncated"] != Value::Bool(false) || total != returned {
        return Err(format!(
            "the cloud read is truncated for this cell ({} of {} rows): the cell is too dense to hold whole — narrow the design buffer",
            returned.unwrap_or(0),
            total.unwrap_or(0)
        ));
    }
    for (index, feature) in features.iter().enumerate() {
        if feature["type"] != "Feature"
            || feature["geometry"]
                .get("type")
                .and_then(Value::as_str)
                .is_none()
        {
            return Err(format!(
                "cloud read feature {index} is not a GeoJSON Feature with a geometry"
            ));
        }
        // What a BigQuery export can carry and DS never holds — above all an
        // inverted or full-globe polygon — stops here, before a room holds it.
        crate::geometry_admission::admit_geometry(&feature["geometry"])
            .map_err(|refusal| format!("cloud read feature {index} is not admitted: {refusal}"))?;
    }
    let version = response["dataset"]["version"].as_str().unwrap_or("");
    if version.len() > 512 {
        return Err("the cloud read's dataset version exceeds its bound".into());
    }
    Ok(json!({"features": features, "sourceVersion": version,
        "sourceReceipt": {"dataset": response["dataset"], "query": response["query"], "rows_total": total}}))
}

/// The two layers a contour acquisition arrives in, one per `contour_kind`.
///
/// A room holds both kinds as one dataset — every line carries its own
/// `contour_kind` — and the print-context assembler splits them back into
/// these layer ids, so the ids are named here once for both directions.
pub const CONTOURS_INDEX_LAYER: &str = "contours_index";
pub const CONTOURS_INTERMEDIATE_LAYER: &str = "contours_intermediate";
/// The longest provider text a contour receipt records verbatim.
const MAX_CONTOUR_RECEIPT_TEXT_CHARS: usize = 512;

/// Admit one acquired contour line, or say exactly why it cannot be admitted.
///
/// Every distinguishable fault is its own sentence naming the layer and the row, the out-of-area case carries
/// both rectangles and the rule, and every case is a refusal — a room never
/// holds a page it cannot fully account for. One fault is new here: a line
/// whose recorded `contour_kind` is not the kind of the layer it arrived in.
/// The print-context assembler splits a room's lines on that property, so a
/// mislabelled line would print in the wrong style, and the provider's own
/// split is the only evidence of which kind it is.
fn admit_contour(
    layer: &str,
    kind: &str,
    index: usize,
    feature: &Value,
    area: &Value,
) -> Result<(), String> {
    let reference = feature_reference(index, feature);
    if feature["type"] != "Feature" || !feature["properties"].is_object() {
        return Err(format!(
            "contour line {reference} in `{layer}` has an invalid shape: a provider row must be a GeoJSON Feature with a properties object"
        ));
    }
    let geometry = feature["geometry"]["type"].as_str().unwrap_or("");
    if !matches!(geometry, "LineString" | "MultiLineString") {
        let named = if geometry.is_empty() {
            "absent".to_owned()
        } else {
            format!("`{}`", quoted(geometry))
        };
        return Err(format!(
            "contour line {reference} in `{layer}` geometry is {named}, not LineString/MultiLineString"
        ));
    }
    let Some(bounds) = crate::reference_cache::feature_bounds(feature) else {
        return Err(format!(
            "contour line {reference} in `{layer}` has no bounds: its geometry carries no usable coordinate"
        ));
    };
    let selected =
        acquisition_selects(CONTOURS_DATASET_ID, "", feature, area).map_err(|error| {
            format!("contour line {reference} in `{layer}` has unreadable geometry: {error}")
        })?;
    if !selected {
        let area_bounds = crate::reference_cache::area_bounds(area)
            .map(bounds_text)
            .unwrap_or_else(|_| "[unreadable]".to_owned());
        return Err(format!(
            "contour line {reference} in `{layer}` lies outside the requested area (rule: intersects; line bounds {}; area bounds {area_bounds})",
            bounds_text(bounds)
        ));
    }
    match feature["properties"]["contour_kind"].as_str() {
        Some(recorded) if recorded == kind => Ok(()),
        Some(recorded @ ("index" | "intermediate")) => Err(format!(
            "contour line {reference} in `{layer}` records contour_kind `{recorded}`, but that layer holds only `{kind}` lines"
        )),
        Some(other) => Err(format!(
            "contour line {reference} in `{layer}` records contour_kind `{}`, not index/intermediate",
            quoted(other)
        )),
        None => Err(format!(
            "contour line {reference} in `{layer}` records no contour_kind"
        )),
    }
}

/// One provider text a contour receipt records verbatim: present, non-empty
/// and bounded, never trimmed or rewritten.
fn contour_receipt_text(response: &Value, key: &str) -> Result<String, String> {
    let named = key.replace('_', " ");
    let value = response[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("contour acquisition names no {named}"))?;
    if value.chars().count() > MAX_CONTOUR_RECEIPT_TEXT_CHARS {
        return Err(format!("contour {named} exceeds its bound"));
    }
    Ok(value.to_owned())
}

/// Decode a complete contour acquisition before it can extend coverage.
///
/// The tiler answers `/v1/contours/fetch` with two `FeatureCollection`s —
/// `contours_intermediate` and `contours_index` — and their counts, plus the
/// facts of the sampling that produced them. Contours are generated for the
/// whole requested area and clipped to it, so there is no truncation receipt
/// to read: completeness is each count agreeing with its collection, and
/// every line lying in the area under the dataset's own `Intersects` rule.
///
/// The admitted page holds both kinds in one `features` array — each line
/// carries its `contour_kind`, which is what the print-context assembler
/// splits on — and a synthesised source receipt, because the tiler sends
/// none: the elevation source and its version, the intervals the lines were
/// cut at, the effective sampling spacing and grid, and how many samples
/// resolved. `sourceVersion` is the tiler's `source_version` (the DEM base
/// URL, or the Terrarium zoom when the DEM did not cover the area), so a
/// room re-acquires when the surface it was cut from changes.
///
/// A complete empty page is admitted: flat ground has no contour, and asking
/// for it again for ever is what spends sampling for no rows.
pub fn decode_contours(response: &Value, area: &Value) -> Result<Value, String> {
    crate::reference_cache::validate_area(area)?;
    if response["status"] != "success" {
        return Err("contour acquisition did not succeed".into());
    }
    let mut features: Vec<Value> = Vec::new();
    for (layer, kind, count_key) in [
        (
            CONTOURS_INTERMEDIATE_LAYER,
            "intermediate",
            "intermediate_features",
        ),
        (CONTOURS_INDEX_LAYER, "index", "index_features"),
    ] {
        let collection = &response["layers"][layer];
        let rows = collection["features"]
            .as_array()
            .ok_or_else(|| format!("contour acquisition has no `{layer}` collection"))?;
        if collection["type"] != "FeatureCollection" {
            return Err(format!(
                "contour acquisition `{layer}` is not a FeatureCollection"
            ));
        }
        if response[count_key].as_u64() != Some(rows.len() as u64) {
            return Err(format!(
                "contour acquisition `{count_key}` count does not match its `{layer}` collection"
            ));
        }
        for (index, feature) in rows.iter().enumerate() {
            admit_contour(layer, kind, index, feature, area)?;
        }
        features.extend(rows.iter().cloned());
    }
    let source = contour_receipt_text(response, "source")?;
    let source_version = contour_receipt_text(response, "source_version")?;
    let minor_interval_m = response["minor_interval_m"].as_u64().unwrap_or(0);
    let index_interval_m = response["index_interval_m"].as_u64().unwrap_or(0);
    if minor_interval_m == 0
        || index_interval_m < minor_interval_m
        || !index_interval_m.is_multiple_of(minor_interval_m)
        || index_interval_m > 1_000
    {
        return Err("contour acquisition records invalid intervals: index_interval_m must be a multiple of minor_interval_m and no greater than 1000".into());
    }
    let effective_spacing_m = response["effective_spacing_m"]
        .as_f64()
        .filter(|spacing| spacing.is_finite() && *spacing > 0.)
        .ok_or("contour acquisition records no effective sampling spacing")?;
    let grid = response["grid"]
        .as_array()
        .filter(|axes| axes.len() == 2)
        .and_then(|axes| {
            let columns = axes[0].as_u64().filter(|count| *count >= 2)?;
            let rows = axes[1].as_u64().filter(|count| *count >= 2)?;
            Some([columns, rows])
        })
        .ok_or("contour acquisition records no sampling grid")?;
    let sampled_points = response["sampled_points"].as_u64().unwrap_or(0);
    let unresolved_points = response["unresolved_points"].as_u64().unwrap_or(u64::MAX);
    if sampled_points == 0 || unresolved_points >= sampled_points {
        return Err("contour acquisition resolved no elevation sample".into());
    }
    let receipt = json!({
        "source": source,
        "source_version": source_version,
        "minor_interval_m": minor_interval_m,
        "index_interval_m": index_interval_m,
        "effective_spacing_m": effective_spacing_m,
        "grid": grid,
        "sampled_points": sampled_points,
        "unresolved_points": unresolved_points,
    });
    validate_source_receipt(&receipt)
        .map_err(|error| format!("contour acquisition receipt: {error}"))?;
    Ok(json!({"features": features, "sourceVersion": source_version, "sourceReceipt": receipt}))
}

/// One catalog entry as the client's dataset registry declares it. Echoed into
/// the row verbatim; this module invents no label and no parameter.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub quality: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
    /// `bundle` (held per project from a published bundle, or acquired
    /// through a provider) or `cloud` (never held here; queried on demand).
    /// Empty reads as `bundle`.
    #[serde(default)]
    pub residency: String,
    /// The catalogue publishes no bundle for this row yet (bundle residency
    /// only): reported as unpublished, never as an incomplete download.
    #[serde(default)]
    pub unpublished: bool,
}

/// A catalog entry paired with the project's own room for it, if any.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverviewRow {
    pub dataset: CatalogEntry,
    /// The room as the local store holds it. `null` means nobody has asked yet
    /// — a distinct state from an empty room, and the one the projection is
    /// careful to keep distinct.
    #[serde(default)]
    pub held: Value,
}

/// A retired broad layer remains in old rooms for recovery, but is not an
/// active dataset. Its digest is not an alias for any detailed replacement.
/// Hosts use the same decision for explicit requests and automatic seeding.
pub fn retired_dataset_layer(dataset: &CatalogEntry) -> Option<&str> {
    // Retirement belongs to Rwanda's foundation catalogue. A source using
    // the same layer name in another country is not that retired dataset.
    if dataset
        .parameters
        .get("country")
        .and_then(Value::as_str)
        .is_some_and(|country| !country.eq_ignore_ascii_case("Rwanda"))
    {
        return None;
    }
    let layer = dataset
        .parameters
        .get("layer")
        .and_then(Value::as_str)
        .unwrap_or(&dataset.id);
    crate::printing::context::retired_alternatives(layer).map(|_| layer)
}

/// Which canonical datasets this project holds, and what each one covers.
///
/// One projection, read by the Project Control card and by the command line.
/// Its whole point is that those two can never disagree, which is exactly why
/// it cannot be a browser function: a field defaulted differently on either
/// side is a silent divergence in what an operator is told they hold.
///
/// Nothing here fetches, refreshes or removes held bytes. Retired broad layers
/// are omitted even if their historical rooms still exist. Absent is reported as
/// absent, staleness travels with the row that is stale, and no dataset ever
/// speaks for another.
pub fn overview(project: &str, policy: &BufferPolicy, rows: &[OverviewRow]) -> Value {
    let datasets: Vec<Value> = rows
        .iter()
        .filter(|row| retired_dataset_layer(&row.dataset).is_none())
        .map(|row| overview_row(&row.dataset, &row.held, policy))
        .collect();
    let seeded = datasets
        .iter()
        .filter(|row| row["seeded"] == Value::Bool(true))
        .count();
    json!({
        "project": project,
        "datasets": datasets,
        "seeded": seeded,
        "available": datasets.len(),
    })
}

fn overview_row(dataset: &CatalogEntry, held: &Value, policy: &BufferPolicy) -> Value {
    let exists = held["exists"] == Value::Bool(true);
    let held = if exists { held } else { &Value::Null };
    let member = |key: &str, fallback: Value| -> Value {
        match held.get(key) {
            Some(Value::Null) | None => fallback,
            Some(value) => value.clone(),
        }
    };
    let cloud = dataset.residency == "cloud";
    let residency = if cloud { "cloud" } else { "bundle" };
    // The room's own readiness when it exists; otherwise the reason there is
    // no room to be ready. A cloud row without a room says where it lives;
    // once seeded through its bounded read it reads like any other room.
    let ready_reason = if cloud && !exists {
        json!(READY_REASON_CLOUD_RESIDENT)
    } else if !exists {
        json!(if dataset.unpublished {
            READY_REASON_BUNDLE_UNPUBLISHED
        } else {
            READY_REASON_NOT_SEEDED
        })
    } else {
        member("ready_reason", Value::Null)
    };
    let empty_coverage = json!({"cells": []});
    json!({
        "dataset_id": dataset.id,
        "label": dataset.label,
        "provider": dataset.provider,
        "quality": dataset.quality,
        "parameters": dataset.parameters,
        "residency": residency,
        // Absent is a distinct state from empty: nobody has asked yet.
        "seeded": exists,
        "ready": member("ready", json!(false)) == Value::Bool(true),
        "ready_reason": ready_reason,
        // A room is one machine's holding. Saying so on every row is what
        // stops "Linux has it" from being read as "the project has it":
        // nothing in this cache publishes a dataset, and two installations of
        // the same project legitimately hold different coverage.
        "local_holding": exists,
        "shared": false,
        "feature_count": member("feature_count", json!(0)),
        "index_state": member("index_state", json!("absent")),
        "source_version": member("source_version", json!("")),
        // Freshness travels with the row rather than being asked for
        // separately: an operator reading what is held has to be able to see
        // that some of it was completed under a version the provider has since
        // replaced. It is a report — nothing here refetches or removes a row.
        "available_version": member("available_version", json!("")),
        "source_versions": member("source_versions", json!([])),
        "fresh": member("fresh", json!(true)) != Value::Bool(false),
        "stale": member("stale", empty_coverage.clone()),
        "stale_count": member("stale_count", json!(0)),
        "buffer_policy": member("buffer_policy", serde_json::to_value(policy).unwrap_or(Value::Null)),
        "requested": member("requested", empty_coverage.clone()),
        "completed": member("completed", empty_coverage),
        "created_at": member("created_at", Value::Null),
        "updated_at": member("updated_at", Value::Null),
        "last_error": member("last_error", Value::Null),
        "pending_queries": member("pending_queries", json!(0)),
        // Abandoned attempts. Reported apart from pending so neither spins.
        "expired_queries": member("expired_queries", json!(0)),
        "queries": Value::Array(
            member("queries", json!([]))
                .as_array()
                .map(|rows| rows.iter().take(20).cloned().collect())
                .unwrap_or_default(),
        ),
    })
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    BufferPolicy {
        schema: String,
        #[serde(default)]
        requested_buffers_m: Vec<f64>,
    },
    DecodeContours {
        schema: String,
        response: Value,
        area: Value,
    },
    Plan {
        schema: String,
        scope: Scope,
        dataset: Dataset,
        policy: BufferPolicy,
        extents: Vec<Extent>,
        #[serde(default)]
        focus: Vec<String>,
    },
    Assess {
        schema: String,
        held: Coverage,
        wanted: Coverage,
    },
    Extend {
        schema: String,
        held: Coverage,
        completed: Coverage,
    },
    Freshness {
        schema: String,
        #[serde(default)]
        current: String,
        #[serde(default)]
        held: Vec<HeldVersion>,
    },
    Identity {
        schema: String,
        scope: Scope,
        dataset: Dataset,
        policy: BufferPolicy,
        coverage: Coverage,
    },
    SeedTargets {
        schema: String,
        #[serde(default)]
        declared: Vec<SeedCandidate>,
        #[serde(default)]
        held: Vec<SeedCandidate>,
        /// One dataset named on the command line, empty when none was.
        #[serde(default)]
        explicit: String,
    },
    Overview {
        schema: String,
        #[serde(default)]
        project: String,
        policy: BufferPolicy,
        #[serde(default)]
        rows: Vec<OverviewRow>,
    },
}

fn require_schema(schema: &str) -> Result<(), String> {
    if schema != SCHEMA {
        return Err(format!("project dataset cache request must use {SCHEMA}"));
    }
    Ok(())
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    let request: Request =
        serde_json::from_slice(input).map_err(|error| format!("invalid request: {error}"))?;
    let value = match request {
        Request::BufferPolicy {
            schema,
            requested_buffers_m,
        } => {
            require_schema(&schema)?;
            serde_json::to_value(buffer_policy(&requested_buffers_m)?)
        }
        Request::DecodeContours {
            schema,
            response,
            area,
        } => {
            require_schema(&schema)?;
            Ok(decode_contours(&response, &area)?)
        }
        Request::Plan {
            schema,
            scope,
            dataset,
            policy,
            extents,
            focus,
        } => {
            require_schema(&schema)?;
            serde_json::to_value(plan_focused(&scope, &dataset, &policy, &extents, &focus)?)
        }
        Request::Assess {
            schema,
            held,
            wanted,
        } => {
            require_schema(&schema)?;
            serde_json::to_value(assess(&held, &wanted)?)
        }
        Request::Extend {
            schema,
            held,
            completed,
        } => {
            require_schema(&schema)?;
            let coverage = extend(&held, &completed)?;
            serde_json::to_value(json!({
                "schema": SCHEMA,
                "coverage_digest": coverage.digest(),
                "coverage_geometry": coverage.geometry(),
                "coverage": coverage,
            }))
        }
        Request::Freshness {
            schema,
            current,
            held,
        } => {
            require_schema(&schema)?;
            serde_json::to_value(freshness(&current, &held)?)
        }
        Request::SeedTargets {
            schema,
            declared,
            held,
            explicit,
        } => {
            require_schema(&schema)?;
            Ok(json!({
                "schema": SCHEMA,
                "targets": seed_targets(&declared, &held, &explicit)?,
            }))
        }
        Request::Identity {
            schema,
            scope,
            dataset,
            policy,
            coverage,
        } => {
            require_schema(&schema)?;
            scope.validate()?;
            dataset.validate()?;
            policy.validate()?;
            coverage.rects()?;
            serde_json::to_value(json!({
                "schema": SCHEMA,
                "dataset_key": dataset_key(&scope, &dataset),
                "query_digest": query_digest(&scope, &dataset, &policy, &coverage),
                "coverage_digest": coverage.digest(),
            }))
        }
        Request::Overview {
            schema,
            project,
            policy,
            rows,
        } => {
            require_schema(&schema)?;
            policy.validate()?;
            Ok(overview(&project, &policy, &rows))
        }
    }
    .map_err(|error| error.to_string())?;
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "project_dataset_cache_tests.rs"]
mod tests;
