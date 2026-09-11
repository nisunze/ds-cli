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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BufferPolicy {
    pub design_buffer_m: f64,
    pub isolated_buffer_m: f64,
    pub isolated_rule: IsolatedRule,
    /// True while `isolated_rule` is an unratified assumption. Hosts surface
    /// this; it never changes geometry.
    #[serde(default)]
    pub provisional: bool,
}

impl Default for BufferPolicy {
    fn default() -> Self {
        Self {
            design_buffer_m: 200.,
            isolated_buffer_m: 1000.,
            isolated_rule: IsolatedRule::PointOnly,
            provisional: false,
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
        Ok(())
    }
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
        .map(|((_, rect, _), &metres)| buffered(*rect, metres))
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
        buffer_policy: *policy,
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
/// rows whose bounds intersect the area (`reference_cache::intersects_area`),
/// which is the same rule its subsets are admitted and refreshed under.
pub const REFERENCE_CACHE_PROVIDER: &str = "desktop:reference_resource_cache";

pub fn acquisition_selection(dataset_id: &str, provider: &str) -> Result<AreaSelection, String> {
    match (dataset_id, provider) {
        (BUILDINGS_DATASET_ID | CONTOURS_DATASET_ID, _) => Ok(AreaSelection::Intersects),
        (_, REFERENCE_CACHE_PROVIDER) => Ok(AreaSelection::Intersects),
        (other, provider) => Err(format!(
            "dataset `{other}` from provider `{provider}` does not record the area rule its provider selects with; register it in acquisition_selection before a room may hold or refresh it"
        )),
    }
}

/// Does this dataset's acquisition select `feature` for `area`?
///
/// Admitting an acquired row and deleting the row it replaces are the same
/// question, so they are the same call. Nothing may re-answer it locally.
pub fn acquisition_selects(
    dataset_id: &str,
    provider: &str,
    feature: &Value,
    area: &Value,
) -> Result<bool, String> {
    crate::reference_cache::selects_area(
        feature,
        area,
        acquisition_selection(dataset_id, provider)?,
    )
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
        if !current.is_empty() && holding.source_version != current {
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

/// Decode a complete source-quality building acquisition before it can extend coverage.
/// Provider coverage uncertainty is retained, including a complete empty response.
pub fn decode_buildings(response: &Value, area: &Value) -> Result<Value, String> {
    crate::reference_cache::validate_area(area)?;
    if response["status"] != "success" || response["provider"] != "google" {
        return Err("building acquisition did not return the requested provider".into());
    }
    if response["coverage"]["truncated"].as_bool() != Some(false) {
        return Err("building acquisition is incomplete or has no completeness receipt".into());
    }
    let collection = &response["layers"]["buildings_context"];
    let features = collection["features"]
        .as_array()
        .ok_or("building acquisition has no footprint collection")?;
    if collection["type"] != "FeatureCollection"
        || response["counts"]["buildings_context"].as_u64() != Some(features.len() as u64)
    {
        return Err("building acquisition count does not match its footprint collection".into());
    }
    for feature in features {
        if feature["type"] != "Feature"
            || !feature["properties"].is_object()
            || !matches!(
                feature["geometry"]["type"].as_str(),
                Some("Polygon" | "MultiPolygon")
            )
            || crate::reference_cache::feature_bounds(feature).is_none()
            || !acquisition_selects(BUILDINGS_DATASET_ID, "", feature, area)?
        {
            return Err("building acquisition contains an invalid or out-of-area footprint".into());
        }
    }
    validate_source_receipt(&response["source_receipt"])
        .map_err(|error| format!("building acquisition receipt: {error}"))?;
    let receipt = response["source_receipt"]
        .as_object()
        .ok_or("building acquisition has no source receipt")?;
    let version = ["dataset_version", "asset"]
        .iter()
        .find_map(|key| {
            receipt
                .get(*key)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
        })
        .unwrap_or("");
    if version.len() > 512 {
        return Err("building source version exceeds its bound".into());
    }
    Ok(json!({"features": features, "sourceVersion":version,
        "sourceReceipt":{"provider":receipt,"coverage":response["coverage"]}}))
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

/// Which canonical datasets this project holds, and what each one covers.
///
/// One projection, read by the Project Control card and by the command line.
/// Its whole point is that those two can never disagree, which is exactly why
/// it cannot be a browser function: a field defaulted differently on either
/// side is a silent divergence in what an operator is told they hold.
///
/// Nothing here fetches, refreshes or removes a row. Absent is reported as
/// absent, staleness travels with the row that is stale, and no dataset ever
/// speaks for another.
pub fn overview(project: &str, policy: &BufferPolicy, rows: &[OverviewRow]) -> Value {
    let datasets: Vec<Value> = rows
        .iter()
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
    let empty_coverage = json!({"cells": []});
    json!({
        "dataset_id": dataset.id,
        "label": dataset.label,
        "provider": dataset.provider,
        "quality": dataset.quality,
        "parameters": dataset.parameters,
        // Absent is a distinct state from empty: nobody has asked yet.
        "seeded": exists,
        "ready": member("ready", json!(false)) == Value::Bool(true),
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
    DecodeBuildings {
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
        Request::DecodeBuildings {
            schema,
            response,
            area,
        } => {
            require_schema(&schema)?;
            Ok(decode_buildings(&response, &area)?)
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
mod tests {
    use super::*;

    fn catalog() -> CatalogEntry {
        CatalogEntry {
            id: "buildings".into(),
            label: "Buildings".into(),
            provider: "ds".into(),
            quality: "source".into(),
            parameters: BTreeMap::new(),
        }
    }

    /// The projection the card and the command line both read. Absent is not
    /// empty, a partial room keeps its own defaults, and no dataset speaks for
    /// another.
    #[test]
    fn an_overview_reports_absence_apart_from_emptiness() {
        let policy = buffer_policy(&[50.0]).unwrap();
        let never = OverviewRow {
            dataset: catalog(),
            held: Value::Null,
        };
        let requested_then_failed = OverviewRow {
            dataset: CatalogEntry {
                id: "roads".into(),
                ..catalog()
            },
            held: json!({
                "exists": true, "ready": false, "index_state": "absent",
                "last_error": "provider refused", "pending_queries": 0,
                "requested": {"cells": ["a"]}, "completed": {"cells": []},
                "feature_count": 0, "fresh": true
            }),
        };
        let covered = OverviewRow {
            dataset: CatalogEntry {
                id: "parcels".into(),
                ..catalog()
            },
            held: json!({
                "exists": true, "ready": true, "index_state": "ready",
                "feature_count": 12, "source_version": "v2",
                "available_version": "v3", "fresh": false,
                "stale": {"cells": ["a"]}, "stale_count": 1,
                "requested": {"cells": ["a"]}, "completed": {"cells": ["a"]},
                "queries": (0..25).map(|index| json!({"id": index})).collect::<Vec<_>>()
            }),
        };
        let answer = overview("p1", &policy, &[never, requested_then_failed, covered]);
        assert_eq!(answer["project"], "p1");
        assert_eq!(answer["available"], 3);
        assert_eq!(answer["seeded"], 2);
        let rows = answer["datasets"].as_array().unwrap();
        // Never seeded: absent, not an empty room, and it inherits the policy.
        assert_eq!(rows[0]["seeded"], false);
        assert_eq!(rows[0]["index_state"], "absent");
        assert_eq!(rows[0]["fresh"], true);
        assert_eq!(rows[0]["completed"], json!({"cells": []}));
        assert_eq!(
            rows[0]["buffer_policy"],
            serde_json::to_value(&policy).unwrap()
        );
        assert_eq!(rows[0]["created_at"], Value::Null);
        // Requested but failed: seeded and unready, with its own error.
        assert_eq!(rows[1]["seeded"], true);
        assert_eq!(rows[1]["ready"], false);
        assert_eq!(rows[1]["last_error"], "provider refused");
        // Superseded: every feature kept, staleness reported beside the row.
        assert_eq!(rows[2]["fresh"], false);
        assert_eq!(rows[2]["stale_count"], 1);
        assert_eq!(rows[2]["feature_count"], 12);
        // The query log is bounded so a long history cannot become the answer.
        assert_eq!(rows[2]["queries"].as_array().unwrap().len(), 20);
    }

    /// A room that says it does not exist reports as absent even when stale
    /// members survive beside the flag — `exists` is the one gate.
    #[test]
    fn a_tombstoned_room_reports_as_absent() {
        let policy = buffer_policy(&[50.0]).unwrap();
        let answer = overview(
            "p1",
            &policy,
            &[OverviewRow {
                dataset: catalog(),
                held: json!({"exists": false, "feature_count": 9, "index_state": "ready"}),
            }],
        );
        assert_eq!(answer["seeded"], 0);
        assert_eq!(answer["datasets"][0]["feature_count"], 0);
        assert_eq!(answer["datasets"][0]["index_state"], "absent");
    }

    #[test]
    fn a_district_sized_chain_queries_member_rectangles_and_focus_narrows_them() {
        // Forty designed sites chained 350 m apart along two 7 km legs: every
        // 200 m buffer overlaps its neighbour, so they fuse into one cluster
        // whose envelope (~7 km x 7 km) is far past MAX_ENVELOPE_M2.
        let step = 0.00315; // ~350 m
        let mut extents = Vec::new();
        for i in 0..20 {
            let x = 30.0 + step * i as f64;
            extents.push(extent(
                &format!("x{i}"),
                Some([x, -2.0, x + 0.0001, -1.9999]),
            ));
            let y = -2.0 + step * i as f64;
            extents.push(extent(
                &format!("y{i}"),
                Some([30.0, y, 30.0001, y + 0.0001]),
            ));
        }
        let whole = plan(&scope(), &buildings(), &policy(), &extents).unwrap();
        assert_eq!(whole.clusters.len(), 1);
        assert!(whole.clusters[0].split);
        assert_eq!(whole.clusters[0].members.len(), 40);
        assert_eq!(whole.coverage.cells.len(), 40);
        for &[w, s, e, n] in &whole.coverage.cells {
            assert!(rect_area_m2(Rect { w, s, e, n }) < 1_000_000.0);
        }
        let one = plan_focused(
            &scope(),
            &buildings(),
            &policy(),
            &extents,
            &["x7".to_string()],
        )
        .unwrap();
        assert_eq!(one.coverage.cells.len(), 1);
        assert_eq!(one.clusters[0].cells, 1);
        assert_eq!(one.clusters[0].members.len(), 40);
        let none = plan_focused(
            &scope(),
            &buildings(),
            &policy(),
            &extents,
            &["absent".to_string()],
        )
        .unwrap();
        assert!(none.empty);
        // A small pair keeps its single envelope and is not split.
        let pair = [
            extent("a", Some([30.5, -2.5, 30.501, -2.499])),
            extent("b", Some([30.5012, -2.5, 30.5022, -2.499])),
        ];
        let small = plan(&scope(), &buildings(), &policy(), &pair).unwrap();
        assert_eq!(small.coverage.cells.len(), 1);
        assert!(!small.clusters[0].split);
    }

    #[test]
    fn shared_buffers_never_shrink_below_project_acquisition_defaults() {
        assert_eq!(buffer_policy(&[100.]).unwrap().design_buffer_m, 200.);
        assert_eq!(buffer_policy(&[100.]).unwrap().isolated_buffer_m, 1000.);
        assert_eq!(buffer_policy(&[500.]).unwrap().design_buffer_m, 500.);
        assert_eq!(buffer_policy(&[1500.]).unwrap().isolated_buffer_m, 1500.);
        assert!(buffer_policy(&[f64::NAN]).is_err());
        assert!(!buffer_policy(&[]).unwrap().provisional);
    }

    #[test]
    fn building_decoder_rejects_partial_and_malformed_results() {
        let area = json!({"type":"Polygon","coordinates":[[[29.,-3.],[31.,-3.],[31.,-1.],[29.,-1.],[29.,-3.]]]});
        let valid = json!({"status":"success","provider":"google",
            "layers":{"buildings_context":{"type":"FeatureCollection","features":[]}},
            "counts":{"buildings_context":0},"coverage":{"truncated":false,"dataset_covers_area":null},
            "source_receipt":{"dataset_version":"v3","asset":"provider asset","elapsed_ms":77}});
        let result = decode_buildings(&valid, &area).unwrap();
        assert_eq!(result["sourceVersion"], "v3");
        assert_eq!(
            result["sourceReceipt"]["coverage"]["dataset_covers_area"],
            Value::Null
        );
        let mut partial = valid.clone();
        partial["coverage"]["truncated"] = json!(true);
        assert!(decode_buildings(&partial, &area).is_err());
        let mut absent = valid.clone();
        absent["layers"] = json!({});
        assert!(decode_buildings(&absent, &area).is_err());
        let mut malformed = valid.clone();
        malformed["layers"]["buildings_context"]["features"] =
            json!([{"type":"Feature","geometry":null,"properties":{}}]);
        malformed["counts"]["buildings_context"] = json!(1);
        assert!(decode_buildings(&malformed, &area).is_err());
    }

    fn scope() -> Scope {
        Scope {
            principal: "uid-1".into(),
            project: "project-a".into(),
        }
    }

    fn buildings() -> Dataset {
        Dataset {
            id: "google_open_buildings".into(),
            provider: "ds-brain:query_print_context".into(),
            quality: Quality::Source,
            parameters: BTreeMap::from([
                ("min_area_m2".into(), json!(25)),
                ("min_confidence".into(), json!(0.7)),
            ]),
            source_version: String::new(),
            crs: default_crs(),
        }
    }

    fn policy() -> BufferPolicy {
        BufferPolicy {
            design_buffer_m: 200.0,
            isolated_buffer_m: 1_000.0,
            isolated_rule: IsolatedRule::PointOnly,
            provisional: true,
        }
    }

    fn extent(name: &str, bounds: Option<[f64; 4]>) -> Extent {
        Extent {
            name: name.into(),
            bounds,
        }
    }

    // An independent WGS84 yardstick. A spherical haversine would be the wrong
    // ruler here — it disagrees with the ellipsoid by ~0.1% and would read a
    // correct 500 m buffer as 499.4 m, so the test would be measuring its own
    // approximation rather than the code.
    const SEMI_MAJOR: f64 = 6_378_137.0;
    const ECCENTRICITY_SQUARED: f64 = 0.006_694_379_990_141_316;

    /// Metres north between two latitudes on the same meridian.
    fn northing(from: f64, to: f64) -> f64 {
        let phi = ((from + to) / 2.0).to_radians();
        let meridian = SEMI_MAJOR * (1.0 - ECCENTRICITY_SQUARED)
            / (1.0 - ECCENTRICITY_SQUARED * phi.sin().powi(2)).powf(1.5);
        (to - from).to_radians().abs() * meridian
    }

    /// Metres east between two longitudes along one parallel.
    fn easting(latitude: f64, from: f64, to: f64) -> f64 {
        let phi = latitude.to_radians();
        let prime_vertical = SEMI_MAJOR / (1.0 - ECCENTRICITY_SQUARED * phi.sin().powi(2)).sqrt();
        (to - from).to_radians().abs() * prime_vertical * phi.cos()
    }

    #[test]
    fn buffer_is_metric_at_every_latitude_and_never_short() {
        for latitude in [0.0, -1.94, 45.0, -60.0] {
            let rect = Rect::from_bounds([30.0, latitude, 30.0, latitude]).unwrap();
            let grown = buffered(rect, 500.0);
            for distance in [
                northing(latitude, grown.n),
                northing(grown.s, latitude),
                easting(latitude, 30.0, grown.e),
                easting(latitude, grown.w, 30.0),
            ] {
                assert!(
                    (500.0..502.0).contains(&distance),
                    "latitude {latitude} produced {distance} m"
                );
            }
        }
    }

    #[test]
    fn disconnected_designs_stay_separate_and_neighbours_fuse() {
        // Two transformers 200 m apart fuse under a 500 m buffer; a third sits
        // a whole degree away and must never join their rectangle.
        let plan = plan(
            &scope(),
            &buildings(),
            &policy(),
            &[
                extent("T1", Some([30.0, -2.0, 30.002, -1.998])),
                extent("T2", Some([30.0025, -2.0, 30.004, -1.998])),
                extent("T3", Some([31.0, -2.0, 31.002, -1.998])),
            ],
        )
        .unwrap();
        assert_eq!(plan.clusters.len(), 2);
        assert_eq!(plan.clusters[0].members, vec!["T1", "T2"]);
        assert_eq!(plan.clusters[1].members, vec!["T3"]);
        // The fused pair is one rectangle, not two, and the far transformer is
        // a separate one — never a single envelope spanning the degree between.
        assert_eq!(plan.coverage.cells.len(), 2);
        let spans: Vec<f64> = plan
            .coverage
            .cells
            .iter()
            .map(|&[w, _, e, _]| e - w)
            .collect();
        assert!(spans.iter().all(|span| *span < 0.05), "{spans:?}");
        assert!(!plan.empty);
    }

    #[test]
    fn a_cluster_costs_one_query_however_many_transformers_it_holds() {
        // A chain of transformers along a road is one site. Fusing to the
        // cluster envelope keeps it one acquisition; the exact rectilinear
        // union of the same chain would be O(n) rectangles, which would turn a
        // district into hundreds of provider round trips — the exact cost this
        // cache exists to avoid.
        let chain: Vec<Extent> = (0..40)
            .map(|step| {
                let west = 30.0 + f64::from(step) * 0.004;
                extent(
                    &format!("T{step}"),
                    Some([west, -2.0, west + 0.002, -1.998]),
                )
            })
            .collect();
        let plan = plan(&scope(), &buildings(), &policy(), &chain).unwrap();
        assert_eq!(plan.clusters.len(), 1);
        assert_eq!(plan.clusters[0].members.len(), 40);
        assert_eq!(plan.coverage.cells.len(), 1);
        assert!(!plan.clusters[0].isolated);
    }

    #[test]
    fn an_undesigned_transformer_takes_the_isolated_buffer_under_each_rule() {
        let point = [extent("solo", Some([30.0, -2.0, 30.0, -2.0]))];
        let wide = plan(&scope(), &buildings(), &policy(), &point).unwrap();
        assert_eq!(wide.clusters[0].buffer_m, 1_000.0);
        let [w, _, e, _] = wide.coverage.cells[0];
        assert!((2_000.0..2_005.0).contains(&easting(-2.0, w, e)));

        // A designed single transformer separates the two candidate rules:
        // point-only keeps it at 200 m, single-cluster widens it to 1 km.
        let designed = [extent("solo", Some([30.0, -2.0, 30.002, -1.998]))];
        let narrow = plan(&scope(), &buildings(), &policy(), &designed).unwrap();
        assert_eq!(narrow.clusters[0].buffer_m, 200.0);
        let by_cluster = BufferPolicy {
            isolated_rule: IsolatedRule::SingleTransformerCluster,
            ..policy()
        };
        let widened = plan(&scope(), &buildings(), &by_cluster, &designed).unwrap();
        assert_eq!(widened.clusters[0].buffer_m, 1_000.0);
        assert_ne!(narrow.coverage, widened.coverage);
        // The policy that produced a coverage is part of its acquisition
        // identity, so answering the open question is an ordinary extension.
        assert_ne!(narrow.query_digest, widened.query_digest);
    }

    #[test]
    fn a_transformer_without_geometry_is_named_not_guessed() {
        let plan = plan(
            &scope(),
            &buildings(),
            &policy(),
            &[extent("no-geometry", None)],
        )
        .unwrap();
        assert_eq!(plan.skipped, vec!["no-geometry"]);
        assert_eq!(plan.located_count, 0);
        assert!(plan.empty && plan.coverage_geometry.is_none());
    }

    #[test]
    fn held_coverage_answers_a_repeat_and_only_the_delta_is_a_gap() {
        let held = Coverage {
            cells: vec![[0.0, 0.0, 2.0, 2.0]],
        };
        let inside = Coverage {
            cells: vec![[0.5, 0.5, 1.5, 1.5]],
        };
        let repeat = assess(&held, &inside).unwrap();
        assert!(repeat.covered && repeat.gaps.is_empty());

        // A design that grew east asks only for the strip it added.
        let grown = Coverage {
            cells: vec![[0.0, 0.0, 3.0, 2.0]],
        };
        let extended = assess(&held, &grown).unwrap();
        assert!(!extended.covered);
        assert_eq!(extended.gaps.cells, vec![[2.0, 0.0, 3.0, 2.0]]);
        assert_eq!(extended.surplus_count, 0);

        // Folding that acquisition in makes the next identical ask free.
        let after = extend(&held, &extended.gaps).unwrap();
        assert_eq!(after.cells, vec![[0.0, 0.0, 3.0, 2.0]]);
        assert!(assess(&after, &grown).unwrap().covered);
    }

    #[test]
    fn a_deleted_transformer_leaves_surplus_and_destroys_nothing() {
        let held = Coverage {
            cells: vec![[0.0, 0.0, 1.0, 1.0], [5.0, 5.0, 6.0, 6.0]],
        };
        let wanted = Coverage {
            cells: vec![[0.0, 0.0, 1.0, 1.0]],
        };
        let result = assess(&held, &wanted).unwrap();
        assert!(result.covered);
        assert_eq!(result.surplus.cells, vec![[5.0, 5.0, 6.0, 6.0]]);
    }

    #[test]
    fn a_hole_between_clusters_is_never_paid_for_twice() {
        // Four sites around an empty middle. The fused coverage must not become
        // one filled square — the middle was never wanted.
        let ring = Coverage {
            cells: vec![
                [0.0, 0.0, 3.0, 1.0],
                [0.0, 2.0, 3.0, 3.0],
                [0.0, 1.0, 1.0, 2.0],
                [2.0, 1.0, 3.0, 2.0],
            ],
        };
        let merged = extend(&Coverage::default(), &ring).unwrap();
        let centre = Coverage {
            cells: vec![[1.4, 1.4, 1.6, 1.6]],
        };
        let result = assess(&merged, &centre).unwrap();
        assert!(!result.covered);
        assert_eq!(result.gaps.cells, vec![[1.4, 1.4, 1.6, 1.6]]);
    }

    #[test]
    fn extending_exact_coverage_refuses_to_replace_many_cells_with_their_envelope() {
        let held = Coverage {
            cells: (0..MAX_COVERAGE_CELLS)
                .map(|index| {
                    let west = index as f64 * 0.002;
                    [west, 0.0, west + 0.001, 0.001]
                })
                .collect(),
        };
        let west = MAX_COVERAGE_CELLS as f64 * 0.002;
        let completed = Coverage {
            cells: vec![[west, 0.0, west + 0.001, 0.001]],
        };

        let error = extend(&held, &completed).unwrap_err();
        assert!(error.contains("exact completed coverage"), "{error}");

        // The refusal matters because the envelope would claim an unpaid hole
        // between every pair of cells.
        let hole = Coverage {
            cells: vec![[0.00125, 0.00025, 0.00175, 0.00075]],
        };
        assert!(!assess(&held, &hole).unwrap().covered);
    }

    #[test]
    fn identity_separates_the_room_address_from_the_acquisition() {
        let coverage = Coverage {
            cells: vec![[0.0, 0.0, 1.0, 1.0]],
        };
        let mut fresh = buildings();
        fresh.source_version = "v2".into();
        // A new provider version is freshness, not a different room.
        assert_eq!(
            dataset_key(&scope(), &buildings()),
            dataset_key(&scope(), &fresh)
        );
        assert_ne!(
            query_digest(&scope(), &buildings(), &policy(), &coverage),
            query_digest(&scope(), &fresh, &policy(), &coverage)
        );
        // Another principal, another project, other filters, and a tile-quality
        // extract are all different rooms.
        let other_principal = Scope {
            principal: "uid-2".into(),
            ..scope()
        };
        let other_project = Scope {
            project: "project-b".into(),
            ..scope()
        };
        let lossy = Dataset {
            quality: Quality::TileExtract,
            ..buildings()
        };
        let mut filtered = buildings();
        filtered
            .parameters
            .insert("min_confidence".into(), json!(0.9));
        let mine = dataset_key(&scope(), &buildings());
        for other in [
            dataset_key(&other_principal, &buildings()),
            dataset_key(&other_project, &buildings()),
            dataset_key(&scope(), &lossy),
            dataset_key(&scope(), &filtered),
        ] {
            assert_ne!(mine, other);
        }
        // Two datasets in the same project never share a room, so one can never
        // answer for the other's readiness.
        let contours = Dataset {
            id: "elevation_contours".into(),
            ..buildings()
        };
        assert_ne!(mine, dataset_key(&scope(), &contours));
    }

    #[test]
    fn identity_is_stable_across_parameter_order() {
        let mut reordered = buildings();
        reordered.parameters.clear();
        reordered
            .parameters
            .insert("min_confidence".into(), json!(0.7));
        reordered.parameters.insert("min_area_m2".into(), json!(25));
        assert_eq!(
            dataset_key(&scope(), &buildings()),
            dataset_key(&scope(), &reordered)
        );
    }

    #[test]
    fn requests_are_validated_before_any_geometry_is_trusted() {
        assert!(evaluate(b"{}").is_err());
        let wrong = json!({"action":"assess","schema":"ds.other/v1","held":{"cells":[]},"wanted":{"cells":[]}});
        assert!(evaluate(wrong.to_string().as_bytes()).is_err());
        let inverted = json!({"action":"assess","schema":SCHEMA,"held":{"cells":[[2.0,0.0,1.0,1.0]]},"wanted":{"cells":[]}});
        assert!(evaluate(inverted.to_string().as_bytes()).is_err());
        let unprojected = Dataset {
            crs: "EPSG:3857".into(),
            ..buildings()
        };
        assert!(plan(&scope(), &unprojected, &policy(), &[]).is_err());
        assert!(plan(
            &scope(),
            &buildings(),
            &BufferPolicy {
                design_buffer_m: -1.0,
                ..policy()
            },
            &[]
        )
        .is_err());
    }

    #[test]
    fn one_named_rule_governs_both_acquisition_and_the_deletion_that_replaces_it() {
        // The buildings door fetches by intersection, so a footprint crossing
        // the edge of an area belongs to that area. Deletion asks the same
        // question through the same call — there is no second predicate to
        // drift.
        assert_eq!(
            acquisition_selection(BUILDINGS_DATASET_ID, "").unwrap(),
            AreaSelection::Intersects
        );
        let area =
            json!({"type":"Polygon","coordinates":[[[0.,0.],[1.,0.],[1.,1.],[0.,1.],[0.,0.]]]});
        let crossing = json!({"type":"Feature","properties":{},
            "geometry":{"type":"Polygon","coordinates":[[[0.9,0.4],[1.3,0.4],[1.3,0.6],[0.9,0.6],[0.9,0.4]]]}});
        assert!(acquisition_selects(BUILDINGS_DATASET_ID, "", &crossing, &area).unwrap());
        assert_eq!(
            acquisition_selection("4eebae72digest", REFERENCE_CACHE_PROVIDER).unwrap(),
            AreaSelection::Intersects
        );
        // An unregistered dataset has no rule, and a room may not invent a
        // destructive one for it.
        let refused = acquisition_selection("some_new_dataset", "some-provider").unwrap_err();
        assert!(refused.contains("acquisition_selection"), "{refused}");
        assert!(
            acquisition_selects("some_new_dataset", "some-provider", &crossing, &area).is_err()
        );
    }

    #[test]
    fn an_oversized_or_malformed_source_receipt_is_refused_not_trimmed() {
        assert!(validate_source_receipt(&json!({"dataset_version": "v3"})).is_ok());
        assert!(validate_source_receipt(&json!("v3")).is_err());
        let huge = json!({"body": "x".repeat(MAX_SOURCE_RECEIPT_BYTES)});
        let error = validate_source_receipt(&huge).unwrap_err();
        assert!(error.contains("over the"), "{error}");
        // The building decoder shares the one bound rather than repeating it.
        let area = json!({"type":"Polygon","coordinates":[[[29.,-3.],[31.,-3.],[31.,-1.],[29.,-1.],[29.,-3.]]]});
        let response = json!({"status":"success","provider":"google",
            "layers":{"buildings_context":{"type":"FeatureCollection","features":[]}},
            "counts":{"buildings_context":0},"coverage":{"truncated":false},
            "source_receipt": huge});
        assert!(decode_buildings(&response, &area)
            .unwrap_err()
            .contains("receipt"));
    }

    #[test]
    fn an_abandoned_acquisition_expires_at_a_bounded_age_and_is_never_pending() {
        let now = 10 * ACQUISITION_TICKET_TTL_MS;
        assert_eq!(
            acquisition_state("requested", now - ACQUISITION_TICKET_TTL_MS, now).unwrap(),
            "requested"
        );
        assert_eq!(
            acquisition_state("requested", now - ACQUISITION_TICKET_TTL_MS - 1, now).unwrap(),
            "expired"
        );
        // Expiry is not failure: nothing went wrong and nothing was written.
        assert_eq!(acquisition_state("failed", 0, now).unwrap(), "failed");
        assert_eq!(acquisition_state("completed", 0, now).unwrap(), "completed");
        assert!(acquisition_state("half-done", 0, now).is_err());
        // A clock that moves backwards must not resurrect an old ticket as
        // fresh work, nor overflow.
        assert!(!acquisition_expired(now, 0));
        assert!(!acquisition_expired(i64::MIN, i64::MIN));
    }

    #[test]
    fn freshness_reports_obsolete_coverage_and_never_removes_it() {
        let held = |version: &str, cells: Vec<[f64; 4]>| HeldVersion {
            source_version: version.into(),
            coverage: Coverage { cells },
        };
        let rows = [
            held("v1", vec![[0.0, 0.0, 1.0, 1.0]]),
            held("v2", vec![[2.0, 0.0, 3.0, 1.0]]),
            held("", vec![[5.0, 0.0, 6.0, 1.0]]),
        ];
        let report = freshness("v2", &rows).unwrap();
        assert!(!report.fresh);
        assert_eq!(report.stale.cells, vec![[0.0, 0.0, 1.0, 1.0]]);
        assert_eq!(report.unattributed.cells, vec![[5.0, 0.0, 6.0, 1.0]]);
        assert_eq!(report.versions, vec!["v1".to_string(), "v2".to_string()]);
        assert_eq!(
            report.stale_geometry.as_ref().unwrap()["type"],
            "MultiPolygon"
        );

        // Coverage that is stale is still coverage: this reports it, and the
        // caller still holds every cell it held before.
        let all = Coverage {
            cells: vec![[0.0, 0.0, 1.0, 1.0], [2.0, 0.0, 3.0, 1.0]],
        };
        assert!(
            assess(
                &all,
                &Coverage {
                    cells: vec![[0.0, 0.0, 1.0, 1.0]]
                }
            )
            .unwrap()
            .covered
        );

        // No known current version is no evidence of obsolescence.
        assert!(freshness("", &rows).unwrap().fresh);
        // And once everything is re-acquired under v2, nothing is stale.
        let refreshed = [held("v2", vec![[0.0, 0.0, 1.0, 1.0], [2.0, 0.0, 3.0, 1.0]])];
        assert!(freshness("v2", &refreshed).unwrap().fresh);
    }

    #[test]
    fn the_wire_surface_answers_a_full_round_trip() {
        let request = json!({
            "action": "plan", "schema": SCHEMA,
            "scope": {"principal": "uid-1", "project": "project-a"},
            "dataset": {"id": "google_open_buildings", "provider": "ds-brain:query_print_context",
                "quality": "source", "parameters": {"min_area_m2": 25}},
            "policy": {"design_buffer_m": 500.0, "isolated_buffer_m": 1000.0,
                "isolated_rule": "point_only", "provisional": true},
            "extents": [{"name": "T1", "bounds": [30.0, -2.0, 30.002, -1.998]}]
        });
        let planned: Value =
            serde_json::from_str(&evaluate(request.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(planned["schema"], SCHEMA);
        assert_eq!(planned["coverage_geometry"]["type"], "MultiPolygon");
        assert_eq!(planned["query_digest"].as_str().unwrap().len(), 64);
        let assessed = json!({"action":"assess","schema":SCHEMA,
            "held": planned["coverage"], "wanted": planned["coverage"]});
        let result: Value =
            serde_json::from_str(&evaluate(assessed.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(result["covered"], true);
        assert_eq!(result["gap_count"], 0);
    }
}
