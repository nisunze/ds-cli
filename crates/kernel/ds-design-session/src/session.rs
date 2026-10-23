//! Session state — Phase A.5.
//!
//! `SessionHandle` owns a layer's lon/lat features plus a projected editing
//! copy and an R-tree index over that projected geometry. `pick(lng, lat,
//! tolerance_m)` projects the click into the session plane and returns the
//! canonical FeatureId, sourced from one authoritative index. This closes the
//! "wrong-feature-highlighted" bug class structurally — there are no
//! parallel indexes to disagree.
//!
//! ## Phase A.5 — what shipped
//!
//! - Open a session from a feature list (Point + LineString + Polygon).
//! - Project lon/lat features into a local tangent x/y editing plane.
//! - Pick the nearest feature within a metric tolerance.
//! - **Mutate the index in place**: `insert` / `replace` / `remove` keep the
//!   R-tree synchronized as drawing-session edits happen in TS. This is
//!   the foundation for "creates and splits during an active edit are safe
//!   to pick on the very next click." See
//!   [`ds-web/docs/design-session-state-contract.md`] for the cross-stack
//!   wiring.
//! - Phase C DataFrame tables are feature-gated behind `enable_dataframes`.
//!   When enabled, `SessionHandle` owns one `LayerTable` per layer and keeps
//!   it synchronized with `insert` / `replace` / `remove`.
//! - Phase D mutation diffs and undo/redo are owned here. Vertex edits,
//!   splits, and feature add/remove/replace all update geometry, bbox,
//!   R-tree, DataFrame tables, and history through the same apply path.
//!
//! ## Mutation contract
//!
//! Every drawing-session mutation in ds-web (`operations.ts:addFeature`,
//! `splitLineFeatureById`, `updateFeatureGeometry`, `deleteFeature*`) MUST
//! call the corresponding handle method in the same call so the in-memory
//! `gdfs` and the index can never diverge. Skipping the index update is a
//! contract violation; the next pick on the new feature would fall back to
//! MapLibre's tile cache, reintroducing the bug class this contract closes.
//!
//! `replace` remains available for whole-feature swaps, but vertex-level
//! edits use `move_vertex` / `insert_vertex` / `delete_vertex` so the core
//! can emit a precise undoable diff.
//!
//! ## Interaction ordering
//!
//! The edit-session model owns interaction z-order. Point-vs-line conflicts
//! are resolved in favor of the point within the caller's pick tolerance;
//! other geometry families keep exact projected distance ordering, with
//! domain z-priority as a tie-breaker. This keeps session picking independent
//! of MapLibre render caches while matching what the operator sees on the map:
//! a line can be selected elsewhere along its path, but a point only has its
//! own small target.

#[cfg(feature = "enable_dataframes")]
use crate::dataframe::{BulkCellEditResult, CellEditDiff, LayerRow, LayerTable, RowWindow};
use crate::identity::{geojson_feature_id, geojson_feature_identity_candidates};
use crate::mutation::{FeatureChange, MutationLog, SessionDiff};
use crate::projection::{
    ProjectionConfig, interpolate_projected, projected_distance, projected_point_segment_distance,
    projected_segment_t,
};
pub use crate::projection::{ProjectionMetadata, ProjectionMode};
// The lasso predicate is not the session's to own. `ds_geo::region` holds the
// one implementation; this module only decides which mounted candidates to
// hand it, and in which coordinate space.
use crate::region::{
    line_intersects_polygon, line_within_polygon, point_in_polygon, point_in_ring,
    polygon_intersects_polygon, polygon_within_polygon,
};
use ds_geo_lite::index::{AABB, RTree, RTreeObject};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// Foundation vertex-dedupe tolerance, in projected metres. Any two
/// vertices closer than this on the projected plane are treated as the
/// same node by session sanitization, geometry editing, and downstream
/// domain processing (network builders re-export this under their own
/// names — see `ds_network::network::NODE_DEDUPE_TOLERANCE_M`).
pub const VERTEX_DEDUPE_TOLERANCE_M: f64 = 0.5;

const SCOPE_SEPARATOR: char = ':';
const PICK_DISTANCE_TIE_M: f64 = 1e-3;
const SNAP_ENDPOINT_EXCLUSION_T: f64 = 0.01;
const SNAP_DISTANCE_TIE_M: f64 = 0.25;
const HOVER_DISTANCE_TIE_M: f64 = 0.25;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
pub struct FeatureId(pub String);

impl FeatureId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// Geometry on the wire: GeoJSON shape, exactly.
///
/// Internal Rust fields stay named `coords` / `rings` to read naturally
/// at call sites (`geom.coords`, `geom.rings`), but the JSON
/// representation matches GeoJSON 1:1 (`{"type": "Point", "coordinates": …}`,
/// `{"type": "LineString", "coordinates": [[…]]}`,
/// `{"type": "Polygon", "coordinates": [[[…]]]}`).
///
/// This is the single source of truth for any geometry leaving the WASM
/// boundary — TS consumes the shape directly without a reconstruction
/// helper. Previously the snake-case `"point"` / `"line_string"` shape
/// forced every renderer to translate, and that translation was
/// duplicated in five places. Don't reintroduce a translator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[serde(tag = "type")]
pub enum Geometry {
    /// `coords` is `[lng, lat]`.
    #[serde(rename = "Point")]
    Point {
        #[serde(rename = "coordinates")]
        coords: [f64; 2],
    },
    /// `coords` is `[[lng, lat], …]` with at least 2 vertices.
    #[serde(rename = "LineString")]
    LineString {
        #[serde(rename = "coordinates")]
        coords: Vec<[f64; 2]>,
    },
    /// `rings[0]` is the outer ring; `rings[1..]` are inner holes. Each
    /// ring is `[[lng, lat], …]` with the first vertex repeated as the
    /// last (GeoJSON convention).
    #[serde(rename = "Polygon")]
    Polygon {
        #[serde(rename = "coordinates")]
        rings: Vec<Vec<[f64; 2]>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum ProjectedGeometry {
    Point { coords: [f64; 2] },
    LineString { coords: Vec<[f64; 2]> },
    Polygon { rings: Vec<Vec<[f64; 2]>> },
}

fn projection_from_features(features: &[Feature]) -> ProjectionMetadata {
    projection_from_features_with_config(features, ProjectionConfig::default())
}

fn projection_from_features_with_config(
    features: &[Feature],
    config: ProjectionConfig,
) -> ProjectionMetadata {
    projection_from_feature_refs_with_config(features.iter(), config)
}

/// Compute the tangent-plane origin from borrowed features — no `Feature`
/// clone. Scope mount folds all still-mounted features then the incoming ones
/// through this so it never deep-clones O(M+N) features just to read
/// coordinate extent. `LonLatBounds` min/max is order- and duplicate-
/// independent for finite coords, so the origin is bit-identical to
/// materializing the features into a `Vec` first.
fn projection_from_feature_refs_with_config<'a, I>(
    features: I,
    config: ProjectionConfig,
) -> ProjectionMetadata
where
    I: IntoIterator<Item = &'a Feature>,
{
    ProjectionMetadata::from_lonlat_coords_with_config(
        features
            .into_iter()
            .flat_map(|feature| geometry_lonlat_coords(&feature.geometry)),
        config,
    )
}

fn geometry_lonlat_coords(geometry: &Geometry) -> Vec<[f64; 2]> {
    match geometry {
        Geometry::Point { coords } => vec![*coords],
        Geometry::LineString { coords } => coords.clone(),
        Geometry::Polygon { rings } => rings.iter().flatten().copied().collect(),
    }
}

fn project_geometry(projection: &ProjectionMetadata, geometry: &Geometry) -> ProjectedGeometry {
    match geometry {
        Geometry::Point { coords } => ProjectedGeometry::Point {
            coords: projection.project_coord(*coords),
        },
        Geometry::LineString { coords } => ProjectedGeometry::LineString {
            coords: coords
                .iter()
                .map(|coord| projection.project_coord(*coord))
                .collect(),
        },
        Geometry::Polygon { rings } => ProjectedGeometry::Polygon {
            rings: rings
                .iter()
                .map(|ring| {
                    ring.iter()
                        .map(|coord| projection.project_coord(*coord))
                        .collect()
                })
                .collect(),
        },
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(missing_as_null)
)]
pub struct Feature {
    pub id: FeatureId,
    pub geometry: Geometry,
    #[serde(default)]
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub properties: Map<String, Value>,
}

/// Edit/compute-boundary input for per-feature design-version provenance.
///
/// Callers may pass only dirty layers for an existing transformer. A first
/// create must pass every layer because every feature is new to that version
/// history. The returned layers preserve the input GeoJSON shape verbatim
/// apart from `properties.v_first` / `properties.v_last`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionStampInput {
    pub layers: Map<String, Value>,
    #[serde(default)]
    pub base_layers: Map<String, Value>,
    pub version: u64,
    #[serde(default)]
    pub first_create: bool,
    /// Treat every supplied feature as changed even when no comparable cloud
    /// baseline is available (for example a recovered browser-local room).
    #[serde(default)]
    pub force_changed: bool,
    /// A missing base row is not proof of creation when the caller supplied a
    /// recovered partial baseline. In that lane preserve any existing
    /// `v_first` instead of claiming the current design version introduced the row.
    #[serde(default)]
    pub assume_existing: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionStampResult {
    pub layers: Map<String, Value>,
    pub stamped_feature_count: usize,
    pub stamped_by_layer: Map<String, Value>,
}

/// Stamp GeoJSON features with the transformer's deliberate design version.
/// The optimistic save counter is transport concurrency only and must never
/// enter `v_first` / `v_last`.
///
/// This is intentionally a pure kernel operation. It may run only after an
/// operator has deliberately opened a design version; ordinary working-copy
/// saves and optimistic-concurrency retries must never invoke it.
///
/// `v_first` and `v_last` are excluded from content comparison so retrying a
/// prospective save cannot make provenance fields themselves look like a
/// domain edit. Existing features retain `v_first`; changed features advance
/// `v_last`; new features receive both fields. Version zero is real: it is the
/// baseline before the first deliberate transformer version. Existing legacy
/// rows with missing lineage start at v0. At design v0, stale positive stamps
/// written by the retired save-counter contract are normalized to v0 instead
/// of being presented as history that never existed.
pub fn stamp_geojson_layers_version(
    mut input: VersionStampInput,
) -> Result<VersionStampResult, String> {
    let mut stamped_feature_count = 0usize;
    let mut stamped_by_layer = Map::new();
    for (layer_name, layer) in &mut input.layers {
        let features = layer
            .get_mut("features")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| format!("layer {layer_name}: expected a FeatureCollection"))?;
        let mut base_features = Vec::new();
        let mut base_by_id: HashMap<String, usize> = HashMap::new();
        if let Some(base_layer) = input.base_layers.get(layer_name) {
            base_features = geojson_features(base_layer)
                .map_err(|err| format!("base layer {layer_name}: {err}"))?
                .into_iter()
                .collect();
            for (base_index, feature) in base_features.iter().enumerate() {
                for id in geojson_feature_identity_candidates(feature) {
                    if let Some(existing_index) = base_by_id.insert(id.clone(), base_index)
                        && existing_index != base_index
                    {
                        return Err(format!(
                            "base layer {layer_name}: stable identity {id:?} is shared by features {existing_index} and {base_index}"
                        ));
                    }
                }
            }
        }

        let mut layer_stamped = 0usize;
        for (feature_index, feature) in features.iter_mut().enumerate() {
            let feature_ids = geojson_feature_identity_candidates(feature);
            if feature_ids.is_empty() {
                return Err(format!(
                    "layer {layer_name}, feature {feature_index}: stable id is required for version stamping"
                ));
            }
            let mut matched_base_indexes = feature_ids
                .iter()
                .filter_map(|id| base_by_id.get(id).copied())
                .collect::<HashSet<_>>();
            if matched_base_indexes.len() > 1 {
                let mut indexes = matched_base_indexes.drain().collect::<Vec<_>>();
                indexes.sort_unstable();
                return Err(format!(
                    "layer {layer_name}, feature {feature_index}: stable identities resolve to multiple base features {indexes:?}"
                ));
            }
            let base = matched_base_indexes
                .drain()
                .next()
                .and_then(|index| base_features.get(index).copied());
            let is_new = input.first_create || (base.is_none() && !input.assume_existing);
            let changed = input.force_changed
                || input.first_create
                || base.is_none_or(|before| !geojson_feature_content_equal(before, feature));

            let current_revision = input.version;
            let (v_first, v_last) = if is_new {
                (current_revision, current_revision)
            } else {
                let provenance = base.unwrap_or(&*feature);
                let (first, last) = feature_design_version_baseline(
                    provenance,
                    current_revision,
                    layer_name,
                    feature_index,
                )?;
                (first, if changed { current_revision } else { last })
            };

            let properties = feature
                .as_object_mut()
                .ok_or_else(|| {
                    format!(
                        "layer {layer_name}, feature {feature_index}: expected a Feature object"
                    )
                })?
                .entry("properties")
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .ok_or_else(|| {
                    format!(
                        "layer {layer_name}, feature {feature_index}: properties must be an object"
                    )
                })?;
            let first_value = Value::Number(v_first.into());
            let last_value = Value::Number(v_last.into());
            if properties.get("v_first") == Some(&first_value)
                && properties.get("v_last") == Some(&last_value)
            {
                continue;
            }
            properties.insert("v_first".to_string(), first_value);
            properties.insert("v_last".to_string(), last_value);
            layer_stamped += 1;
        }
        if layer_stamped > 0 {
            stamped_by_layer.insert(layer_name.clone(), Value::Number(layer_stamped.into()));
            stamped_feature_count += layer_stamped;
        }
    }

    Ok(VersionStampResult {
        layers: input.layers,
        stamped_feature_count,
        stamped_by_layer,
    })
}

fn feature_design_version_baseline(
    feature: &Value,
    current_version: u64,
    layer_name: &str,
    feature_index: usize,
) -> Result<(u64, u64), String> {
    // v0 is the deliberate baseline design version. Positive values here came
    // from the retired save-counter stamping contract and are not design
    // history for a transformer that is still deliberately at v0.
    if current_version == 0 {
        return Ok((0, 0));
    }
    let properties = feature.get("properties").and_then(Value::as_object);
    let read = |key: &str| -> Result<Option<u64>, String> {
        let Some(value) = properties.and_then(|values| values.get(key)) else {
            return Ok(None);
        };
        value.as_u64().map(Some).ok_or_else(|| {
            format!("layer {layer_name}, feature {feature_index}: {key} must be a non-negative integer design version")
        })
    };
    let given_first = read("v_first")?;
    let given_last = read("v_last")?;
    let first = given_first.or(given_last).unwrap_or(0);
    let last = given_last.unwrap_or(first);
    if first > last || last > current_version {
        return Err(format!(
            "layer {layer_name}, feature {feature_index}: invalid design-version provenance v_first={first}, v_last={last}, current={current_version}"
        ));
    }
    Ok((first, last))
}

fn geojson_feature_content_equal(left: &Value, right: &Value) -> bool {
    if left.get("geometry") != right.get("geometry") {
        return false;
    }
    geojson_properties_without_version_equal(left.get("properties"), right.get("properties"))
}

fn geojson_properties_without_version_equal(left: Option<&Value>, right: Option<&Value>) -> bool {
    let left = left.and_then(Value::as_object);
    let right = right.and_then(Value::as_object);
    let meaningful_len = |map: Option<&Map<String, Value>>| {
        map.map_or(0, |values| {
            values
                .keys()
                .filter(|key| key.as_str() != "v_first" && key.as_str() != "v_last")
                .count()
        })
    };
    if meaningful_len(left) != meaningful_len(right) {
        return false;
    }
    left.into_iter().flatten().all(|(key, value)| {
        matches!(key.as_str(), "v_first" | "v_last")
            || right.and_then(|values| values.get(key)) == Some(value)
    })
}

impl Feature {
    pub fn new(id: impl Into<String>, geometry: Geometry) -> Self {
        Self {
            id: FeatureId::new(id),
            geometry,
            properties: Map::new(),
        }
    }

    pub fn with_properties(
        id: impl Into<String>,
        geometry: Geometry,
        properties: Map<String, Value>,
    ) -> Self {
        Self {
            id: FeatureId::new(id),
            geometry,
            properties,
        }
    }
}

/// Internal R-tree element. Stores the FeatureId + cached envelope so the
/// rtree query stays in O(log n) without re-walking geometry. PartialEq
/// is by id only — that's how `RTree::remove` finds an element when we
/// know its id and can recompute the bbox from `features`.
struct IndexedFeature {
    id: FeatureId,
    bbox: AABB<[f64; 2]>,
}

impl PartialEq for IndexedFeature {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl RTreeObject for IndexedFeature {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.bbox
    }
}

/// Selection mode for `SessionHandle::select`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    /// Replace the entire selected set with the new ids.
    Replace,
    /// Add the new ids to the existing selection (deduped).
    Add,
    /// Toggle each id: present → remove, absent → add.
    Toggle,
}

/// Spatial predicate used by polygon selection.
///
/// The wire name this session has always published, aliased onto
/// [`crate::region::RegionPredicate`]. It is a NAME, not a second type: the
/// browser lasso, `ds design features select --region` and this session all
/// carry one predicate.
pub type PolygonSelectionPredicate = crate::region::RegionPredicate;

/// The authoritative selection record for one open session. Phase B
/// of the design-session-state contract — the Svelte selection store
/// becomes a thin projection of this struct on the ds-web side. JS
/// never writes to the selected / hovered fields directly; mutations
/// go through `select` / `hover` / `clear_selection`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(missing_as_null)
)]
pub struct SelectionState {
    /// Multi-selection — for marquee + add/toggle modes. Deduped, in
    /// insertion order. Empty when nothing is selected.
    pub selected: Vec<FeatureId>,
    /// Single hovered feature; mutually independent from selected.
    #[serde(deserialize_with = "crate::reply::written_nullable")]
    pub hovered: Option<FeatureId>,
    /// The feature most recently mutated (added, geometry-changed, or
    /// property-edited). Used by the UI to scroll the attribute table
    /// to the row, focus a property panel, etc.
    #[serde(deserialize_with = "crate::reply::written_nullable")]
    pub last_edited: Option<FeatureId>,
}

/// Engine-owned status for one mounted edit scope. ds-web uses this as the
/// wake signal for toolbar/CRS/snap/hover readiness instead of inferring
/// readiness from JavaScript store state.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(missing_as_null)
)]
#[serde(rename_all = "camelCase")]
pub struct ScopeStatus {
    pub phase: String,
    pub scope: String,
    pub mounted: bool,
    /// Number of raw GeoJSON features received by the last layer mount for this scope.
    pub input_feature_count: usize,
    pub feature_count: usize,
    /// Input features that were not indexed because the WASM session could not own them.
    pub skipped_feature_count: usize,
    pub skipped_missing_id_count: usize,
    pub skipped_unsupported_geometry_count: usize,
    pub total_feature_count: usize,
    pub projection: ProjectionMetadata,
    pub layers: Vec<ScopeLayerStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(missing_as_null)
)]
#[serde(rename_all = "camelCase")]
pub struct ScopeLayerStatus {
    pub name: String,
    #[serde(deserialize_with = "crate::reply::written_nullable")]
    pub geometry_type: Option<String>,
    pub input_feature_count: usize,
    pub feature_count: usize,
    pub skipped_feature_count: usize,
    pub skipped_missing_id_count: usize,
    pub skipped_unsupported_geometry_count: usize,
}

/// Raw GeoJSON layer envelope accepted by the session mount path. This lets
/// Rust own feature id extraction, geometry classification, and indexable
/// geometry conversion instead of having ds-web reconstruct a parallel
/// geometry parser before calling WASM.
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
pub struct SessionLayerInput {
    pub name: String,
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "unknown")
    )]
    pub geojson: Value,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[serde(rename_all = "camelCase")]
pub struct PolygonSelectionQuery {
    /// A GeoJSON Polygon; the session admits it.
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "unknown")
    )]
    pub polygon: Value,
    #[serde(default)]
    pub target_layers: Vec<String>,
    #[serde(default)]
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "RegionPredicate")
    )]
    pub predicate: PolygonSelectionPredicate,
    pub mode: SelectionMode,
    /// Upper bound on exact-geometry predicate evaluations. A selector that
    /// covers the whole session would otherwise loop every mounted feature
    /// synchronously — the documented main-thread freeze footgun (see
    /// `ds-web/docs/wasm-freeze-suspicion-handoff.md`). Defaults generous
    /// enough that a normal "select everything in view" lasso on a real
    /// transformer never trips it, while a pathological/accidental global
    /// scan errors out instead of freezing the browser.
    #[serde(default = "default_polygon_selection_max_candidates")]
    pub max_candidates: usize,
}

/// Default candidate ceiling for [`SessionHandle::select_by_polygon`]. Mounted
/// edit sessions realistically hold thousands to low tens of thousands of
/// features, so 100k bounds a runaway global scan without ever clipping a
/// legitimate large lasso. Distinct from [`default_snap_max_candidates`]
/// (100) because snap/hover pick a single feature near a click, whereas a
/// lasso is expected to return many.
fn default_polygon_selection_max_candidates() -> usize {
    100_000
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ScopeInputStats {
    input_feature_count: usize,
    skipped_missing_id_count: usize,
    skipped_unsupported_geometry_count: usize,
    layers: HashMap<String, ScopeLayerInputStats>,
}

impl ScopeInputStats {
    fn skipped_feature_count(&self) -> usize {
        self.skipped_missing_id_count + self.skipped_unsupported_geometry_count
    }

    fn from_features(features: &[Feature]) -> Self {
        let mut stats = ScopeInputStats::default();
        for feature in features {
            stats.input_feature_count += 1;
            let Some(layer_name) = feature_layer_name(&feature.id) else {
                continue;
            };
            stats
                .layers
                .entry(layer_name.to_string())
                .or_default()
                .input_feature_count += 1;
        }
        stats
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ScopeLayerInputStats {
    input_feature_count: usize,
    skipped_missing_id_count: usize,
    skipped_unsupported_geometry_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[serde(rename_all = "snake_case")]
pub enum SnapTargetKind {
    Vertex,
    Edge,
    Midpoint,
    #[serde(rename = "self")]
    SelfSnap,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[serde(rename_all = "camelCase")]
pub struct SnapQuery {
    pub lng: f64,
    pub lat: f64,
    #[serde(default)]
    #[serde(rename = "toleranceM")]
    pub tolerance_m: f64,
    #[serde(default)]
    #[serde(rename = "targetLayers")]
    pub target_layers: Vec<String>,
    #[serde(default)]
    #[serde(rename = "snapToVertices")]
    pub snap_to_vertices: bool,
    #[serde(default)]
    #[serde(rename = "snapToSegments")]
    pub snap_to_segments: bool,
    #[serde(default)]
    #[serde(rename = "snapToMidpoints")]
    pub snap_to_midpoints: bool,
    #[serde(default)]
    #[serde(rename = "snapToSelf")]
    pub snap_to_self: bool,
    #[serde(default)]
    #[serde(rename = "selfVertices")]
    pub self_vertices: Vec<[f64; 2]>,
    #[serde(default)]
    #[serde(rename = "excludeFeatureIds")]
    pub exclude_feature_ids: Vec<FeatureId>,
    #[serde(default = "default_snap_max_candidates")]
    #[serde(rename = "maxCandidates")]
    pub max_candidates: usize,
}

fn default_snap_max_candidates() -> usize {
    100
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(missing_as_null)
)]
#[serde(rename_all = "camelCase")]
pub struct SnapResult {
    pub target_kind: SnapTargetKind,
    /// `null` for a snap to the caller's own vertices.
    #[serde(deserialize_with = "crate::reply::written_nullable")]
    pub feature_id: Option<FeatureId>,
    #[serde(deserialize_with = "crate::reply::written_nullable")]
    pub layer_id: Option<String>,
    pub coord: [f64; 2],
    pub distance_m: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ring_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertex_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<f64>,
}

#[derive(Clone, Debug)]
struct SnapCandidate {
    result: SnapResult,
    priority: u8,
}

#[derive(Clone, Debug)]
struct PickCandidate {
    id: FeatureId,
    distance_m: f64,
    family: InteractionFamily,
    z_priority: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InteractionFamily {
    Point,
    Line,
    Polygon,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[serde(rename_all = "snake_case")]
pub enum HoverTargetKind {
    Vertex,
    Segment,
    Feature,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[serde(rename_all = "camelCase")]
pub struct HoverQuery {
    pub lng: f64,
    pub lat: f64,
    #[serde(default)]
    #[serde(rename = "toleranceM")]
    pub tolerance_m: f64,
    #[serde(default)]
    #[serde(rename = "targetLayers")]
    pub target_layers: Vec<String>,
    #[serde(default = "default_hover_include")]
    #[serde(rename = "includeVertices")]
    pub include_vertices: bool,
    #[serde(default = "default_hover_include")]
    #[serde(rename = "includeSegments")]
    pub include_segments: bool,
    #[serde(default = "default_hover_include")]
    #[serde(rename = "includeFeatures")]
    pub include_features: bool,
    #[serde(default = "default_snap_max_candidates")]
    #[serde(rename = "maxCandidates")]
    pub max_candidates: usize,
}

fn default_hover_include() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(missing_as_null)
)]
#[serde(rename_all = "camelCase")]
pub struct HoverResult {
    pub target_kind: HoverTargetKind,
    pub feature_id: FeatureId,
    #[serde(deserialize_with = "crate::reply::written_nullable")]
    pub layer_id: Option<String>,
    pub distance_m: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coord: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ring_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertex_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<f64>,
}

#[derive(Clone, Debug)]
struct HoverCandidate {
    result: HoverResult,
    priority: u8,
    family: InteractionFamily,
    z_priority: u8,
}

pub struct SessionHandle {
    features: HashMap<FeatureId, Feature>,
    projected: HashMap<FeatureId, ProjectedGeometry>,
    rtree: RTree<IndexedFeature>,
    projection: ProjectionMetadata,
    projection_initialized: bool,
    mounted_scopes: HashSet<String>,
    scope_input_stats: HashMap<String, ScopeInputStats>,
    selection: SelectionState,
    mutations: MutationLog,
    /// Monotonic counter for [`Self::mint_feature_id`]. WASM owns id
    /// generation so TS can't drift across sessions; collisions with
    /// preloaded ids are handled by retrying the next serial.
    next_mint_serial: u64,
    /// Plug-in domain seam — controls the `domain` tag on
    /// [`crate::selection::SelectionResolution`] and is available to
    /// future pick-priority tie-breaks. `None` means "no domain wired"
    /// and the foundation returns `"design"` as the selection tag for
    /// backward compatibility. Set with [`Self::with_domain`] /
    /// [`Self::set_domain`].
    domain: Option<std::sync::Arc<dyn crate::traits::LayerDomain>>,
    #[cfg(feature = "enable_dataframes")]
    tables: HashMap<String, LayerTable>,
}

impl SessionHandle {
    /// Imperative setter. Replaces any existing domain. Pass `None` to
    /// detach the domain and revert to the foundation defaults.
    pub fn set_domain(&mut self, domain: Option<std::sync::Arc<dyn crate::traits::LayerDomain>>) {
        self.domain = domain;
    }

    /// Returns the domain's [`crate::traits::LayerDomain::id`], or
    /// `"design"` when no domain is wired.
    pub fn domain_id(&self) -> &str {
        self.domain.as_ref().map(|d| d.id()).unwrap_or("design")
    }

    /// Resolve a feature into a widened
    /// [`crate::selection::SelectionResolution`]: identity + full
    /// geometry + bbox + domain tag.
    ///
    /// Returns `None` when `feature_id` isn't in this session.
    ///
    /// The session is the authority on what feature has which geometry
    /// — ds-web's click handler reads from this so vector-tile-clipped
    /// geometry vs full-GeoJSON-source no longer matter for highlight.
    ///
    /// Contract: `docs/contracts/geojson-foundation-crate.md` §6.
    pub fn selection_resolution(
        &self,
        tab_id: impl Into<String>,
        row_index: usize,
        feature_id: &FeatureId,
    ) -> Option<crate::selection::SelectionResolution> {
        let feat = self.features.get(feature_id)?;
        let bbox = geometry_lonlat_bbox(&feat.geometry);
        Some(crate::selection::SelectionResolution {
            tab_id: tab_id.into(),
            row_index,
            feature_id: feature_id.0.clone(),
            geometry: feat.geometry.clone(),
            domain: self.domain_id().to_string(),
            bbox,
        })
    }

    pub fn open(features: Vec<Feature>) -> Self {
        let projection = projection_from_features(&features);
        let projection_initialized = !features.is_empty();
        let mut map = HashMap::with_capacity(features.len());
        let mut projected = HashMap::with_capacity(features.len());
        #[cfg(feature = "enable_dataframes")]
        let mut table_features: HashMap<String, Vec<Feature>> = HashMap::new();

        for feat in features {
            #[cfg(feature = "enable_dataframes")]
            {
                if map.contains_key(&feat.id) {
                    remove_table_feature_seed(&mut table_features, &feat.id);
                }
                if let Some((layer_name, feature_id)) = split_layer_feature_id(&feat.id) {
                    table_features
                        .entry(layer_name.to_string())
                        .or_default()
                        .push(Feature::with_properties(
                            feature_id,
                            feat.geometry.clone(),
                            feat.properties.clone(),
                        ));
                }
            }
            map.insert(feat.id.clone(), feat);
        }

        let indexed = map
            .values()
            .map(|feat| {
                let projected_geometry = project_geometry(&projection, &feat.geometry);
                projected.insert(feat.id.clone(), projected_geometry.clone());
                IndexedFeature {
                    id: feat.id.clone(),
                    bbox: projected_geometry_bbox(&projected_geometry),
                }
            })
            .collect();
        let rtree = RTree::bulk_load(indexed);
        #[cfg(feature = "enable_dataframes")]
        let tables = build_layer_tables(table_features);

        SessionHandle {
            features: map,
            projected,
            rtree,
            projection,
            projection_initialized,
            mounted_scopes: HashSet::new(),
            scope_input_stats: HashMap::new(),
            selection: SelectionState::default(),
            mutations: MutationLog::default(),
            next_mint_serial: 0,
            domain: None,
            #[cfg(feature = "enable_dataframes")]
            tables,
        }
    }

    /// Mint a unique, layer-prefixed feature id. Format matches the retired
    /// TS `nextFeatureId` shape (`{prefix}_{8-digit-serial}`) so sort order
    /// and debug logs stay consistent across the migration. Retries on
    /// collision with any pre-existing feature id on the same layer.
    fn mint_feature_id(&mut self, layer_name: &str) -> String {
        let prefix = feature_id_prefix(mint_prefix_layer_name(layer_name));
        loop {
            self.next_mint_serial = self.next_mint_serial.saturating_add(1);
            let candidate = format!("{prefix}_{:08}", self.next_mint_serial);
            let encoded = encode_layer_feature_id(layer_name, &candidate);
            if !self.features.contains_key(&encoded) {
                return candidate;
            }
        }
    }

    fn mint_feature_id_avoiding(
        &mut self,
        layer_name: &str,
        excluded: &HashSet<FeatureId>,
    ) -> String {
        loop {
            let candidate = self.mint_feature_id(layer_name);
            if !excluded.contains(&encode_layer_feature_id(layer_name, &candidate)) {
                return candidate;
            }
        }
    }

    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    pub fn projection_metadata(&self) -> ProjectionMetadata {
        self.projection
    }

    pub fn project_coord(&self, lng: f64, lat: f64) -> Option<[f64; 2]> {
        if !lng.is_finite() || !lat.is_finite() {
            return None;
        }
        Some(self.projection.project_coord([lng, lat]))
    }

    /// Read the authoritative readiness/status for one mounted edit scope.
    /// Empty edit sessions still report `mounted: true` after `mount_scope*`;
    /// feature count alone is not a valid readiness signal.
    pub fn scope_status(&self, scope: &str) -> Result<ScopeStatus, String> {
        let scope = normalize_scope(scope)?;
        let feature_count = self
            .features
            .keys()
            .filter(|id| feature_id_scope(id) == scope)
            .count();
        let mounted = self.mounted_scopes.contains(scope);
        let input_stats = self
            .scope_input_stats
            .get(scope)
            .cloned()
            .unwrap_or_default();
        Ok(ScopeStatus {
            phase: if mounted { "ready" } else { "idle" }.to_string(),
            scope: scope.to_string(),
            mounted,
            input_feature_count: input_stats.input_feature_count,
            feature_count,
            skipped_feature_count: input_stats.skipped_feature_count(),
            skipped_missing_id_count: input_stats.skipped_missing_id_count,
            skipped_unsupported_geometry_count: input_stats.skipped_unsupported_geometry_count,
            total_feature_count: self.feature_count(),
            projection: self.projection_metadata(),
            layers: self.scope_layer_statuses(scope),
        })
    }

    /// Mount or replace one edit scope inside this handle. Scope lifecycle
    /// changes are not undoable edits: they represent a session entering or
    /// leaving the tab-level index, so the mutation log is reset after the
    /// scope swap to avoid undo resurrecting unmounted features.
    pub fn mount_scope(&mut self, scope: &str, features: Vec<Feature>) -> Result<usize, String> {
        self.mount_scope_with_projection(scope, features, ProjectionConfig::default())
    }

    /// Mount or replace one edit scope from raw GeoJSON layer envelopes.
    /// This is the preferred browser path: ds-web passes the session layer
    /// working copy across the API boundary, and Rust owns id extraction,
    /// supported-geometry filtering, layer geometry classification, and
    /// spatial index construction.
    pub fn mount_scope_layers(
        &mut self,
        scope: &str,
        layers: Vec<SessionLayerInput>,
    ) -> Result<usize, String> {
        self.mount_scope_layers_with_projection(scope, layers, ProjectionConfig::default())
    }

    pub fn mount_scope_layers_with_projection(
        &mut self,
        scope: &str,
        layers: Vec<SessionLayerInput>,
        projection_config: ProjectionConfig,
    ) -> Result<usize, String> {
        let parsed = session_features_from_layers(layers)?;
        self.mount_scope_with_projection_inner(
            scope,
            parsed.features,
            projection_config,
            parsed.stats,
        )
    }

    pub fn mount_scope_with_projection(
        &mut self,
        scope: &str,
        features: Vec<Feature>,
        projection_config: ProjectionConfig,
    ) -> Result<usize, String> {
        let stats = ScopeInputStats::from_features(&features);
        self.mount_scope_with_projection_inner(scope, features, projection_config, stats)
    }

    /// Mount one edit scope as a single bulk load: one projection pass, one
    /// R-tree `bulk_load`, and one `LayerTable` build per layer.
    ///
    /// The previous shape cloned every already-mounted feature PLUS the
    /// incoming ones just to read the projection extent, then inserted the
    /// incoming features one at a time — and each `put_feature_no_log` rebuilt
    /// the whole layer DataFrame (`to_features` + `from_features`) per feature.
    /// That is the O(N^2) "first geometry create freezes the tab" shape from
    /// PAIN_HISTORY.md (bit us on `terrain_pls`, 9,784 points). This mounts the
    /// scope in O(N log N): origin from borrowed features, bulk feature insert,
    /// one index rebuild, one table build per layer.
    fn mount_scope_with_projection_inner(
        &mut self,
        scope: &str,
        features: Vec<Feature>,
        projection_config: ProjectionConfig,
        input_stats: ScopeInputStats,
    ) -> Result<usize, String> {
        let scope = normalize_scope(scope)?;
        // A same-scope mount is a foundation refresh, not a new selection
        // intent. Preserve the engine-owned slots and filter them against the
        // refreshed feature set after the bulk replacement. Explicit unmount
        // continues to prune them permanently.
        let prior_selection = self
            .mounted_scopes
            .contains(scope)
            .then(|| self.selection.clone());
        self.unmount_scope_inner(scope)?;

        // Scope every incoming feature (`scope:layer/feature`) up front. A bad
        // id fails the whole mount here — before any feature/index/table state
        // is touched — where the old per-feature loop could leave a partially
        // mounted scope when a later feature's id was malformed.
        let scoped: Vec<Feature> = features
            .into_iter()
            .map(|feature| scoped_feature(scope, feature))
            .collect::<Result<_, _>>()?;

        // Projection origin: fold still-mounted survivors then the incoming
        // features by reference — no clone. Bounds min/max are order- and
        // duplicate-independent, so this is the same origin the old
        // clone-everything path produced for the same inputs.
        if self.features.is_empty() && scoped.is_empty() {
            self.clear_projection();
        } else {
            self.projection = projection_from_feature_refs_with_config(
                self.features.values().chain(scoped.iter()),
                projection_config,
            );
            self.projection_initialized = true;
        }

        // Dedupe last-wins by scoped id — `put_feature_no_log` replaced on a
        // repeated id in its first row, so a later duplicate won — and
        // remember the final input feature so `last_edited` lands where the
        // per-feature loop left it.
        let last_edited = scoped.last().map(|feature| feature.id.clone());
        let mut source_order: Vec<FeatureId> = Vec::with_capacity(scoped.len());
        let mut deduped: HashMap<FeatureId, Feature> = HashMap::with_capacity(scoped.len());
        for feature in scoped {
            let id = feature.id.clone();
            if deduped.insert(id.clone(), feature).is_none() {
                source_order.push(id);
            }
        }
        let count = deduped.len();

        // Build each incoming layer's DataFrame table ONCE from its features
        // (never a per-feature `replace_table_feature` round-trip). The scope
        // was just unmounted so its table keys are gone; inserting replaces any
        // survivor wholesale. Rows keep the source order, as `open` does; the
        // map's iteration order would make row indices depend on the hasher.
        #[cfg(feature = "enable_dataframes")]
        {
            let mut grouped: HashMap<String, Vec<Feature>> = HashMap::new();
            for feature in source_order.iter().map(|id| &deduped[id]) {
                if let Some((table_key, local_id)) = split_layer_feature_id(&feature.id) {
                    grouped.entry(table_key.to_string()).or_default().push(
                        Feature::with_properties(
                            local_id,
                            feature.geometry.clone(),
                            feature.properties.clone(),
                        ),
                    );
                }
            }
            for (table_key, local_features) in grouped {
                let table = LayerTable::from_features(table_key.clone(), &local_features)?;
                self.tables.insert(table_key, table);
            }
        }

        // Bulk-insert the features, then project + `bulk_load` the R-tree ONCE
        // over the whole session (survivors re-projected under the new origin,
        // plus the incoming features).
        self.features.extend(deduped);
        self.rebuild_projected_index();
        if let Some(prior) = prior_selection {
            self.selection.selected = prior
                .selected
                .into_iter()
                .filter(|id| self.features.contains_key(id))
                .collect();
            self.selection.hovered = prior.hovered.filter(|id| self.features.contains_key(id));
            self.selection.last_edited = prior
                .last_edited
                .filter(|id| self.features.contains_key(id));
        } else if let Some(id) = last_edited {
            self.selection.last_edited = Some(id);
        }

        self.mounted_scopes.insert(scope.to_string());
        self.scope_input_stats
            .insert(scope.to_string(), input_stats);
        self.mutations.clear();
        Ok(count)
    }

    /// Remove every feature belonging to a mounted scope. Idempotent when the
    /// scope is absent.
    pub fn unmount_scope(&mut self, scope: &str) -> Result<usize, String> {
        let scope = normalize_scope(scope)?;
        let removed = self.unmount_scope_inner(scope)?;
        if removed > 0 {
            self.mutations.clear();
        }
        Ok(removed)
    }

    pub fn can_undo(&self) -> bool {
        self.mutations.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.mutations.can_redo()
    }

    /// Reset the undo/redo baseline. ds-web calls this after a successful
    /// save/submit so persisted state becomes the new history root.
    pub fn commit(&mut self) {
        self.mutations.clear();
    }

    fn scope_layer_statuses(&self, scope: &str) -> Vec<ScopeLayerStatus> {
        let mut layers: HashMap<String, LayerStatusAccumulator> = HashMap::new();
        if let Some(stats) = self.scope_input_stats.get(scope) {
            for (name, layer_stats) in &stats.layers {
                let entry = layers.entry(name.clone()).or_default();
                entry.input_feature_count = layer_stats.input_feature_count;
                entry.skipped_missing_id_count = layer_stats.skipped_missing_id_count;
                entry.skipped_unsupported_geometry_count =
                    layer_stats.skipped_unsupported_geometry_count;
            }
        }
        for (id, feature) in &self.features {
            let Some((feature_scope, layer_name, _)) = split_scoped_feature_id(id) else {
                continue;
            };
            if feature_scope != scope {
                continue;
            }
            let entry = layers.entry(layer_name.to_string()).or_default();
            entry.feature_count += 1;
            entry.add_geometry(&feature.geometry);
        }
        let mut out = layers
            .into_iter()
            .map(|(name, acc)| ScopeLayerStatus {
                name,
                geometry_type: acc.geometry_type(),
                input_feature_count: acc.input_feature_count,
                feature_count: acc.feature_count,
                skipped_feature_count: acc.skipped_feature_count(),
                skipped_missing_id_count: acc.skipped_missing_id_count,
                skipped_unsupported_geometry_count: acc.skipped_unsupported_geometry_count,
            })
            .collect::<Vec<_>>();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Undo the most recent recorded mutation and return the applied
    /// reverse diff. `None` means there was no history to undo.
    pub fn undo(&mut self) -> Result<Option<SessionDiff>, String> {
        let Some(original) = self.mutations.pop_undo() else {
            return Ok(None);
        };
        let applied = original.reversed("undo");
        self.apply_diff(&applied)?;
        self.mutations.push_redo(original);
        Ok(Some(applied))
    }

    /// Redo the most recently undone mutation and return the applied diff.
    pub fn redo(&mut self) -> Result<Option<SessionDiff>, String> {
        let Some(diff) = self.mutations.pop_redo() else {
            return Ok(None);
        };
        self.apply_diff(&diff)?;
        self.mutations.push_undo(diff.clone());
        Ok(Some(diff))
    }

    /// Read the current selection record. Cheap (returns a borrow).
    pub fn selection(&self) -> &SelectionState {
        &self.selection
    }

    /// Mutate the selected set. Returns the new selection state for
    /// the caller to mirror into Svelte stores in one round-trip.
    pub fn select(&mut self, ids: Vec<FeatureId>, mode: SelectionMode) -> SelectionState {
        match mode {
            SelectionMode::Replace => {
                self.selection.selected.clear();
                for id in ids {
                    if self.features.contains_key(&id) && !self.selection.selected.contains(&id) {
                        self.selection.selected.push(id);
                    }
                }
            }
            SelectionMode::Add => {
                for id in ids {
                    if self.features.contains_key(&id) && !self.selection.selected.contains(&id) {
                        self.selection.selected.push(id);
                    }
                }
            }
            SelectionMode::Toggle => {
                for id in ids {
                    if let Some(pos) = self.selection.selected.iter().position(|x| x == &id) {
                        self.selection.selected.remove(pos);
                    } else if self.features.contains_key(&id) {
                        self.selection.selected.push(id);
                    }
                }
            }
        }
        self.selection.clone()
    }

    /// Select every target-layer feature that intersects a GeoJSON Polygon.
    /// The session R-tree owns candidate filtering; exact geometry predicates
    /// run against the same projected model used by pick/snap/hover.
    pub fn select_by_polygon(
        &mut self,
        query: PolygonSelectionQuery,
    ) -> Result<SelectionState, String> {
        let selector = session_geometry_from_geojson(&query.polygon)
            .ok_or_else(|| "selector polygon must be a GeoJSON Polygon geometry".to_string())?;
        let selector = match project_geometry(&self.projection, &selector) {
            ProjectedGeometry::Polygon { rings } if rings.first().map_or(0, Vec::len) >= 3 => rings,
            _ => return Err("selector polygon must be a GeoJSON Polygon geometry".to_string()),
        };
        let envelope = projected_geometry_bbox(&ProjectedGeometry::Polygon {
            rings: selector.clone(),
        });
        let max_candidates = query.max_candidates.max(1);
        let mut candidate_count = 0usize;
        let mut ids = Vec::new();
        let candidates: Box<dyn Iterator<Item = &_>> =
            if query.predicate == PolygonSelectionPredicate::Outside {
                Box::new(self.rtree.iter())
            } else {
                Box::new(self.rtree.locate_in_envelope_intersecting(&envelope))
            };
        for candidate in candidates {
            // Empty `target_layers` selects NOTHING here — never "all mounted
            // layers". `snap_layer_allowed` overloads empty as all-layers for
            // single-click snap/hover, which is the wrong default for a lasso:
            // it would fan a huge selector out to an exact-geometry scan of the
            // entire session on the main thread.
            if !polygon_layer_allowed(&candidate.id, &query.target_layers) {
                continue;
            }
            candidate_count += 1;
            if candidate_count > max_candidates {
                return Err(format!(
                    "polygon selection exceeded {max_candidates} candidate features; \
                     narrow the selector or the target layers"
                ));
            }
            let Some(projected) = self.projected.get(&candidate.id) else {
                continue;
            };
            let matches = match query.predicate {
                PolygonSelectionPredicate::Intersects => {
                    projected_geometry_intersects_polygon(projected, &selector)
                }
                PolygonSelectionPredicate::Within => {
                    projected_geometry_within_polygon(projected, &selector)
                }
                PolygonSelectionPredicate::Outside => {
                    !projected_geometry_intersects_polygon(projected, &selector)
                }
            };
            if matches {
                ids.push(candidate.id.clone());
            }
        }
        ids.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(self.select(ids, query.mode))
    }

    /// Set the hovered feature (or clear with `None`). Mutually
    /// independent from `selected` — hover doesn't disturb selection.
    pub fn hover(&mut self, id: Option<FeatureId>) -> SelectionState {
        self.selection.hovered = id.filter(|id| self.features.contains_key(id));
        self.selection.clone()
    }

    /// Stamp `last_edited`. Operations.* mutation primitives in ds-web
    /// fire this so the UI knows which feature to focus on after a
    /// split / edit / create.
    pub fn mark_edited(&mut self, id: FeatureId) -> SelectionState {
        self.selection.last_edited = self.features.contains_key(&id).then_some(id);
        self.selection.clone()
    }

    /// Clear all three slots. Called on session close + project switch.
    pub fn clear_selection(&mut self) -> SelectionState {
        self.selection = SelectionState::default();
        self.selection.clone()
    }

    /// Insert a new feature into the index. Returns `Err` if the id is
    /// already present — the caller should use `replace` to swap geometry
    /// on an existing feature, never `insert` over a known id.
    pub fn insert(&mut self, feature: Feature) -> Result<(), String> {
        if self.features.contains_key(&feature.id) {
            return Err(format!(
                "session already contains feature `{}`",
                feature.id.0
            ));
        }
        let id = feature.id.clone();
        self.insert_feature_no_log(feature.clone())?;
        self.record_diff(SessionDiff::single(
            "insert_feature",
            FeatureChange {
                feature_id: id,
                before: None,
                after: Some(feature),
            },
        ));
        Ok(())
    }

    /// Create a new GeoJSON-backed feature in `layer_name`, minting the local
    /// feature id in Rust. `layer_name` may be scoped (`design:lv_poles`) or
    /// unscoped (`lv_poles`); the stored feature id is always
    /// `{layer_name}/{minted_feature_id}` while the visible `properties.id`
    /// is only the minted local id.
    pub fn create_geojson_feature(
        &mut self,
        layer_name: &str,
        geometry: Value,
        properties: Map<String, Value>,
    ) -> Result<Option<SessionDiff>, String> {
        let layer_name = normalize_index_layer_name(layer_name)?.to_string();
        let Some(geometry) = session_geometry_from_geojson(&geometry) else {
            return Ok(None);
        };
        let feature_id = self.mint_feature_id(&layer_name);
        let mut properties = sanitized_created_feature_properties(properties);
        properties.insert("id".to_string(), Value::String(feature_id.clone()));
        let feature = Feature::with_properties(
            encode_layer_feature_id(&layer_name, &feature_id).0,
            geometry,
            properties,
        );
        self.insert_feature_no_log(feature.clone())?;
        let diff = SessionDiff::single(
            "create_feature",
            FeatureChange {
                feature_id: feature.id.clone(),
                before: None,
                after: Some(feature),
            },
        );
        self.record_diff(diff.clone());
        Ok(Some(diff))
    }

    /// Drop a feature from the index. Returns the removed feature, or
    /// `None` when the id wasn't present. Idempotent on the no-match case
    /// so JS callers can fire-and-forget on uncertain branches without
    /// special-casing.
    pub fn remove(&mut self, id: &FeatureId) -> Result<Option<Feature>, String> {
        let Some(diff) = self.remove_diff(id)? else {
            return Ok(None);
        };
        Ok(diff
            .changes
            .into_iter()
            .next()
            .and_then(|change| change.before))
    }

    /// Drop a feature and return the undoable diff Rust actually applied.
    /// This is the mutation API ds-web should use; `remove` remains as the
    /// legacy convenience shape for tests and older wrappers.
    pub fn remove_diff(&mut self, id: &FeatureId) -> Result<Option<SessionDiff>, String> {
        let Some(feat) = self.features.get(id).cloned() else {
            return Ok(None);
        };
        self.remove_feature_no_log(id)?;
        let diff = SessionDiff::single(
            "remove_feature",
            FeatureChange {
                feature_id: id.clone(),
                before: Some(feat),
                after: None,
            },
        );
        self.record_diff(diff.clone());
        Ok(Some(diff))
    }

    /// Drop multiple features from one layer as one undoable mutation.
    pub fn remove_features(
        &mut self,
        layer_name: &str,
        feature_ids: Vec<String>,
    ) -> Result<Option<SessionDiff>, String> {
        let layer_name = normalize_index_layer_name(layer_name)?.to_string();
        self.remove_normalized_layer_features(&layer_name, feature_ids, "remove_features")
    }

    fn remove_normalized_layer_features(
        &mut self,
        layer_name: &str,
        feature_ids: Vec<String>,
        kind: &str,
    ) -> Result<Option<SessionDiff>, String> {
        // Resolve the removable ids, capturing each before-feature so the undo
        // diff keeps its per-id FeatureChange shape (ds-web's normalizeSessionDiff
        // depends on it), then drop the whole batch through ONE table rebuild +
        // ONE index rebuild instead of a full DataFrame round-trip per id — the
        // same Tombstone #2 O(N^2) shape as scope unmount, on batch delete.
        let mut changes = Vec::new();
        let mut seen = HashSet::new();
        let mut removed_ids: HashSet<FeatureId> = HashSet::new();
        #[cfg(feature = "enable_dataframes")]
        let mut local_ids: HashSet<String> = HashSet::new();
        for feature_id in feature_ids {
            if feature_id.trim().is_empty() || !seen.insert(feature_id.clone()) {
                continue;
            }
            let id = encode_layer_feature_id(layer_name, &feature_id);
            let Some(before) = self.features.get(&id).cloned() else {
                continue;
            };
            changes.push(FeatureChange {
                feature_id: id.clone(),
                before: Some(before),
                after: None,
            });
            removed_ids.insert(id);
            #[cfg(feature = "enable_dataframes")]
            local_ids.insert(feature_id);
        }
        if changes.is_empty() {
            return Ok(None);
        }
        // All ids share this one layer (table key == `layer_name`, which
        // normalize_index_layer_name guarantees has no '/'), so a single bulk
        // table removal covers the batch.
        #[cfg(feature = "enable_dataframes")]
        self.remove_table_features(layer_name, &local_ids)?;
        self.drop_feature_ids_bulk(&removed_ids);
        let diff = SessionDiff {
            kind: kind.to_string(),
            changes,
        };
        self.record_diff(diff.clone());
        Ok(Some(diff))
    }

    /// Drop every feature currently mounted in one layer as one undoable
    /// mutation. Empty layers are TS transport metadata only, so no feature
    /// diff is emitted when the layer has no indexed rows.
    pub fn remove_layer(&mut self, layer_name: &str) -> Result<Option<SessionDiff>, String> {
        let layer_name = normalize_index_layer_name(layer_name)?.to_string();
        let mut ids = self
            .features
            .keys()
            .filter_map(|id| {
                split_layer_feature_id_any(id).and_then(|(layer, feature)| {
                    (layer == layer_name).then_some(feature.to_string())
                })
            })
            .collect::<Vec<_>>();
        ids.sort();
        self.remove_normalized_layer_features(&layer_name, ids, "remove_layer")
    }

    /// Replace every feature in a layer from raw GeoJSON Features as one
    /// undoable mutation. Missing incoming ids are minted in Rust; duplicate
    /// incoming ids are reminted so the layer remains indexable.
    pub fn replace_layer_geojson_features(
        &mut self,
        layer_name: &str,
        features: Vec<Value>,
    ) -> Result<Option<SessionDiff>, String> {
        let layer_name = normalize_index_layer_name(layer_name)?.to_string();
        let mut parsed = Vec::new();
        let mut incoming_ids = HashSet::new();

        for feature in features {
            let mut parsed_feature =
                match session_feature_from_geojson_inner(&layer_name, feature.clone())? {
                    Ok(feature) => feature,
                    Err(SessionFeatureSkipReason::MissingId) => {
                        let Some(geometry) = feature
                            .get("geometry")
                            .and_then(session_geometry_from_geojson)
                        else {
                            continue;
                        };
                        let feature_id = self.mint_feature_id_avoiding(&layer_name, &incoming_ids);
                        let mut properties = geojson_feature_properties(&feature);
                        properties.insert("id".to_string(), Value::String(feature_id.clone()));
                        Feature::with_properties(
                            encode_layer_feature_id(&layer_name, &feature_id).0,
                            geometry,
                            properties,
                        )
                    }
                    Err(SessionFeatureSkipReason::UnsupportedGeometry) => continue,
                };

            if !incoming_ids.insert(parsed_feature.id.clone()) {
                let feature_id = self.mint_feature_id_avoiding(&layer_name, &incoming_ids);
                parsed_feature.id = encode_layer_feature_id(&layer_name, &feature_id);
                parsed_feature
                    .properties
                    .insert("id".to_string(), Value::String(feature_id));
                incoming_ids.insert(parsed_feature.id.clone());
            }
            parsed.push(parsed_feature);
        }

        let mut existing_ids = self
            .features
            .keys()
            .filter(|id| {
                split_layer_feature_id_any(id)
                    .is_some_and(|(existing_layer, _)| existing_layer == layer_name)
            })
            .cloned()
            .collect::<Vec<_>>();
        existing_ids.sort_by(|a, b| a.0.cmp(&b.0));

        // Diff shape is preserved exactly: removed existing ids (sorted) first,
        // then inserted features in parse order. But the mutation is applied in
        // bulk — one table rebuild, one index rebuild — never a per-feature
        // `remove_feature_no_log` / `insert_feature_no_log` that rebuilt the
        // whole layer DataFrame per row (the same O(N^2) freeze as scope mount).
        let was_initialized = self.projection_initialized;
        let mut changes = Vec::with_capacity(existing_ids.len() + parsed.len());
        let mut removed_ids: HashSet<FeatureId> = HashSet::with_capacity(existing_ids.len());
        for id in existing_ids {
            if let Some(before) = self.features.get(&id).cloned() {
                changes.push(FeatureChange {
                    feature_id: id.clone(),
                    before: Some(before),
                    after: None,
                });
                removed_ids.insert(id);
            }
        }
        for feature in &parsed {
            changes.push(FeatureChange {
                feature_id: feature.id.clone(),
                before: None,
                after: Some(feature.clone()),
            });
        }
        if changes.is_empty() {
            return Ok(None);
        }

        // Rebuild the layer's DataFrame table once from the parsed features
        // (dropping the old table wholesale), instead of a remove-then-insert
        // full round-trip per row.
        #[cfg(feature = "enable_dataframes")]
        {
            self.tables.remove(&layer_name);
            let locals: Vec<Feature> = parsed
                .iter()
                .filter_map(|feature| {
                    split_layer_feature_id(&feature.id).map(|(_, local)| {
                        Feature::with_properties(
                            local,
                            feature.geometry.clone(),
                            feature.properties.clone(),
                        )
                    })
                })
                .collect();
            if !locals.is_empty() {
                let table = LayerTable::from_features(layer_name.clone(), &locals)?;
                self.tables.insert(layer_name.clone(), table);
            }
        }

        for id in &removed_ids {
            self.features.remove(id);
        }
        self.prune_selection_ids(&removed_ids);
        let last_edited = parsed.last().map(|feature| feature.id.clone());
        let first_parsed = parsed.first().cloned();
        self.features.extend(
            parsed
                .into_iter()
                .map(|feature| (feature.id.clone(), feature)),
        );

        if self.features.is_empty() {
            self.clear_projection();
        } else {
            if !was_initialized {
                // Match the old first-insert projection seed: origin from the
                // first newly-inserted feature only. `replace_layer` never
                // re-fit the plane from the full extent, so an already-mounted
                // session keeps its origin (the common case: this path runs
                // inside an active edit session).
                self.projection = projection_from_feature_refs_with_config(
                    first_parsed.iter(),
                    ProjectionConfig::default(),
                );
                self.projection_initialized = true;
            }
            self.rebuild_projected_index();
        }
        if let Some(id) = last_edited {
            self.selection.last_edited = Some(id);
        }

        let diff = SessionDiff {
            kind: "replace_layer".to_string(),
            changes,
        };
        self.record_diff(diff.clone());
        Ok(Some(diff))
    }

    /// Replace an existing feature's geometry. Tears down the old rtree
    /// entry and re-inserts under the same id. Returns the prior feature
    /// when there was one to replace, `None` when the id wasn't present
    /// (in which case the new feature IS still inserted — same effect as
    /// `insert`, no surprise to the caller who asked for "make this be
    /// the geometry now").
    pub fn replace(&mut self, feature: Feature) -> Result<Option<Feature>, String> {
        let id = feature.id.clone();
        let old = self.features.get(&id).cloned();
        self.put_feature_no_log(feature.clone())?;
        self.record_diff(SessionDiff::single(
            "replace_feature",
            FeatureChange {
                feature_id: id,
                before: old.clone(),
                after: Some(feature),
            },
        ));
        Ok(old)
    }

    /// Replace an existing feature and return the undoable diff. Unlike
    /// `replace`, this refuses the "insert if absent" path: direct geometry
    /// edits should never create a phantom row if the Rust index and TS mirror
    /// have already diverged.
    pub fn replace_existing_diff(
        &mut self,
        feature: Feature,
    ) -> Result<Option<SessionDiff>, String> {
        let id = feature.id.clone();
        let Some(old) = self.features.get(&id).cloned() else {
            return Ok(None);
        };
        self.put_feature_no_log(feature.clone())?;
        let diff = SessionDiff::single(
            "replace_feature",
            FeatureChange {
                feature_id: id,
                before: Some(old),
                after: Some(feature),
            },
        );
        self.record_diff(diff.clone());
        Ok(Some(diff))
    }

    pub fn replace_existing_geojson_feature(
        &mut self,
        layer_name: &str,
        feature: Value,
    ) -> Result<Option<SessionDiff>, String> {
        let Some(feature) = session_feature_from_geojson(layer_name, feature)? else {
            return Ok(None);
        };
        self.replace_existing_diff(feature)
    }

    pub fn insert_vertex(
        &mut self,
        id: &FeatureId,
        ring_index: usize,
        segment_index: usize,
        coord: [f64; 2],
    ) -> Result<Option<SessionDiff>, String> {
        self.mutate_feature_geometry(id, "insert_vertex", |geometry| match geometry {
            Geometry::LineString { coords } => Ok(insert_into_line(coords, segment_index, coord)),
            Geometry::Polygon { rings } => {
                let Some(ring) = rings.get_mut(ring_index) else {
                    return Ok(false);
                };
                Ok(insert_into_line(ring, segment_index, coord))
            }
            Geometry::Point { .. } => Ok(false),
        })
    }

    pub fn move_vertex(
        &mut self,
        id: &FeatureId,
        ring_index: usize,
        vertex_index: usize,
        coord: [f64; 2],
    ) -> Result<Option<SessionDiff>, String> {
        self.mutate_feature_geometry(id, "move_vertex", |geometry| match geometry {
            Geometry::LineString { coords } => Ok(move_in_line(coords, vertex_index, coord)),
            Geometry::Polygon { rings } => {
                let Some(ring) = rings.get_mut(ring_index) else {
                    return Ok(false);
                };
                Ok(move_in_ring(ring, vertex_index, coord))
            }
            Geometry::Point { coords } => {
                if *coords == coord {
                    return Ok(false);
                }
                *coords = coord;
                Ok(true)
            }
        })
    }

    pub fn delete_vertex(
        &mut self,
        id: &FeatureId,
        ring_index: usize,
        vertex_index: usize,
    ) -> Result<Option<SessionDiff>, String> {
        self.mutate_feature_geometry(id, "delete_vertex", |geometry| match geometry {
            Geometry::LineString { coords } => Ok(delete_from_line(coords, vertex_index)),
            Geometry::Polygon { rings } => {
                let Some(ring) = rings.get_mut(ring_index) else {
                    return Ok(false);
                };
                Ok(delete_from_ring(ring, vertex_index))
            }
            Geometry::Point { .. } => Ok(false),
        })
    }

    pub fn move_feature(
        &mut self,
        id: &FeatureId,
        from_coord: [f64; 2],
        to_coord: [f64; 2],
    ) -> Result<Option<SessionDiff>, String> {
        if !valid_lonlat(from_coord) || !valid_lonlat(to_coord) {
            return Ok(None);
        }
        let projection = self.projection;
        let from = projection.project_coord(from_coord);
        let to = projection.project_coord(to_coord);
        let delta = [to[0] - from[0], to[1] - from[1]];
        if projected_distance([0.0, 0.0], delta) <= 1e-9 {
            return Ok(None);
        }
        self.mutate_feature_geometry(id, "move_feature", |geometry| {
            Ok(translate_geometry_projected(geometry, projection, delta))
        })
    }

    pub fn extend_line_to(
        &mut self,
        source_id: &FeatureId,
        target_id: &FeatureId,
        intent_coord: Option<[f64; 2]>,
    ) -> Result<Option<SessionDiff>, String> {
        if source_id == target_id {
            return Ok(None);
        }
        let Some(before) = self.features.get(source_id).cloned() else {
            return Ok(None);
        };
        let Some(target) = self.features.get(target_id) else {
            return Ok(None);
        };
        let (Geometry::LineString { coords: source }, Geometry::LineString { coords: target }) =
            (&before.geometry, &target.geometry)
        else {
            return Ok(None);
        };
        let Some(mut coords) = extend_line_to_target(
            source,
            target,
            self.projection,
            intent_coord.filter(|c| valid_lonlat(*c)),
        ) else {
            return Ok(None);
        };
        dedupe_line_coords(&mut coords, self.projection, VERTEX_DEDUPE_TOLERANCE_M);
        if coords == *source {
            return Ok(None);
        }
        let mut after = before.clone();
        after.geometry = Geometry::LineString { coords };
        self.put_feature_no_log(after.clone())?;
        let diff = SessionDiff::single(
            "extend_line",
            FeatureChange {
                feature_id: source_id.clone(),
                before: Some(before),
                after: Some(after),
            },
        );
        self.record_diff(diff.clone());
        Ok(Some(diff))
    }

    pub fn join_lines(
        &mut self,
        source_id: &FeatureId,
        target_id: &FeatureId,
        tolerance_m: f64,
    ) -> Result<Option<SessionDiff>, String> {
        if source_id == target_id || !tolerance_m.is_finite() || tolerance_m < 0.0 {
            return Ok(None);
        }
        let Some(source_before) = self.features.get(source_id).cloned() else {
            return Ok(None);
        };
        let Some(target_before) = self.features.get(target_id).cloned() else {
            return Ok(None);
        };
        let (Geometry::LineString { coords: source }, Geometry::LineString { coords: target }) =
            (&source_before.geometry, &target_before.geometry)
        else {
            return Ok(None);
        };
        let Some(mut coords) = join_linestrings(source, target, self.projection, tolerance_m)
        else {
            return Ok(None);
        };
        dedupe_line_coords(&mut coords, self.projection, VERTEX_DEDUPE_TOLERANCE_M);
        if coords.len() < 2 {
            return Ok(None);
        }
        let mut source_after = source_before.clone();
        source_after.geometry = Geometry::LineString { coords };
        let diff = SessionDiff {
            kind: "join_lines".to_string(),
            changes: vec![
                FeatureChange {
                    feature_id: source_id.clone(),
                    before: Some(source_before),
                    after: Some(source_after),
                },
                FeatureChange {
                    feature_id: target_id.clone(),
                    before: Some(target_before),
                    after: None,
                },
            ],
        };
        self.apply_diff(&diff)?;
        self.record_diff(diff.clone());
        Ok(Some(diff))
    }

    /// Split a LineString into N segments. WASM mints the ids for the
    /// N-1 newly-created segments; the caller reads them out of the
    /// returned diff. Identity ownership lives here so TS can't drift —
    /// previously TS called `nextFeatureId` and passed ids in, and a
    /// re-entered session would re-mint values WASM already knew about,
    /// producing the "split-line id chaos" operators reported.
    pub fn split_line(
        &mut self,
        layer_name: &str,
        feature_id: &str,
        segments: Vec<Vec<[f64; 2]>>,
    ) -> Result<Option<SessionDiff>, String> {
        if layer_name.trim().is_empty() || feature_id.trim().is_empty() {
            return Err("layer_name and feature_id are required".to_string());
        }
        if segments.len() < 2 || segments.iter().any(|segment| segment.len() < 2) {
            return Ok(None);
        }

        let target_id = encode_layer_feature_id(layer_name, feature_id);
        let Some(before) = self.features.get(&target_id).cloned() else {
            return Ok(None);
        };
        if !matches!(before.geometry, Geometry::LineString { .. }) {
            return Ok(None);
        }

        let mut created_ids: Vec<String> = Vec::with_capacity(segments.len() - 1);
        for _ in 0..segments.len() - 1 {
            created_ids.push(self.mint_feature_id(layer_name));
        }

        let tol = VERTEX_DEDUPE_TOLERANCE_M;
        let projection = self.projection;
        let mut retained_coords = segments[0].clone();
        dedupe_line_coords(&mut retained_coords, projection, tol);
        if retained_coords.len() < 2 {
            return Ok(None);
        }
        let mut retained = before.clone();
        retained.geometry = Geometry::LineString {
            coords: retained_coords,
        };
        retained
            .properties
            .insert("id".to_string(), Value::String(feature_id.to_string()));

        let split_props = copied_split_feature_properties(&before.properties);
        let mut changes = vec![FeatureChange {
            feature_id: target_id,
            before: Some(before),
            after: Some(retained),
        }];

        for (mut coords, id) in segments.into_iter().skip(1).zip(created_ids) {
            dedupe_line_coords(&mut coords, projection, tol);
            if coords.len() < 2 {
                return Ok(None);
            }
            let mut props = split_props.clone();
            props.insert("id".to_string(), Value::String(id.clone()));
            let feature = Feature::with_properties(
                encode_layer_feature_id(layer_name, &id).0,
                Geometry::LineString { coords },
                props,
            );
            changes.push(FeatureChange {
                feature_id: feature.id.clone(),
                before: None,
                after: Some(feature),
            });
        }

        let diff = SessionDiff {
            kind: "split_line".to_string(),
            changes,
        };
        self.apply_diff(&diff)?;
        self.record_diff(diff.clone());
        Ok(Some(diff))
    }

    /// Return every feature id within `radius_m` of `(lng, lat)`,
    /// ordered nearest-first. Drives snap candidate selection in ds-web:
    /// instead of walking every feature in `session.gdfs` looking for
    /// vertex / edge / midpoint snaps, snap.ts asks the index for the
    /// (small) candidate set first.
    ///
    /// Empty `Vec` for negative / non-finite radii — same shape as
    /// `pick`'s `None` for those inputs.
    pub fn pick_radius(&self, lng: f64, lat: f64, radius_m: f64) -> Vec<FeatureId> {
        if radius_m < 0.0 || !radius_m.is_finite() {
            return Vec::new();
        }
        let click = self.projection.project_coord([lng, lat]);
        let envelope = projected_pick_envelope(click, radius_m);
        let mut hits: Vec<PickCandidate> = Vec::new();
        for cand in self.rtree.locate_in_envelope_intersecting(&envelope) {
            let Some(projected) = self.projected.get(&cand.id) else {
                continue;
            };
            let d_m = projected_distance_m(click, projected);
            if d_m.is_finite() && d_m <= radius_m {
                hits.push(PickCandidate {
                    id: cand.id.clone(),
                    distance_m: d_m,
                    family: interaction_family(projected),
                    z_priority: feature_z_priority(&cand.id, projected),
                });
            }
        }
        // Stable sort: close candidates follow model-owned interaction
        // z-order, otherwise nearest projected geometry wins.
        hits.sort_by(pick_candidate_order);
        hits.into_iter().map(|candidate| candidate.id).collect()
    }

    /// Query deterministic snap targets in the Rust-owned projected model.
    /// This is the exact evaluator for vertex, midpoint, perpendicular edge,
    /// and current-drawing self snaps; TypeScript only supplies the pointer,
    /// enabled target kinds, layer filters, and visual tolerance.
    pub fn snap(&self, query: SnapQuery) -> Option<SnapResult> {
        let tolerance_m = query.tolerance_m;
        if tolerance_m < 0.0
            || !tolerance_m.is_finite()
            || !query.lng.is_finite()
            || !query.lat.is_finite()
        {
            return None;
        }
        let click = self.projection.project_coord([query.lng, query.lat]);
        let mut best: Option<SnapCandidate> = None;

        if query.snap_to_self {
            for coord in &query.self_vertices {
                if !coord[0].is_finite() || !coord[1].is_finite() {
                    continue;
                }
                let coord = *coord;
                let projected = self.projection.project_coord(coord);
                let distance_m = projected_distance(click, projected);
                if distance_m <= tolerance_m {
                    accept_snap_candidate(
                        &mut best,
                        SnapCandidate {
                            priority: snap_priority(SnapTargetKind::SelfSnap),
                            result: SnapResult {
                                target_kind: SnapTargetKind::SelfSnap,
                                feature_id: None,
                                layer_id: None,
                                coord,
                                distance_m,
                                ring_index: None,
                                vertex_index: None,
                                segment_index: None,
                                t: None,
                            },
                        },
                    );
                }
            }
        }

        let envelope = projected_pick_envelope(click, tolerance_m);
        let mut candidate_count = 0usize;
        for indexed in self.rtree.locate_in_envelope_intersecting(&envelope) {
            if !snap_layer_allowed(&indexed.id, &query.target_layers) {
                continue;
            }
            if query
                .exclude_feature_ids
                .iter()
                .any(|feature_id| feature_id == &indexed.id)
            {
                continue;
            }
            candidate_count += 1;
            if candidate_count > query.max_candidates.max(1) {
                return None;
            }
            let Some(feature) = self.features.get(&indexed.id) else {
                continue;
            };
            let Some(projected) = self.projected.get(&indexed.id) else {
                continue;
            };
            self.collect_feature_snap_candidates(
                click,
                tolerance_m,
                &query,
                feature,
                projected,
                &mut best,
            );
        }

        best.map(|candidate| candidate.result)
    }

    /// Query deterministic hover targets in the Rust-owned projected model.
    /// The caller supplies the pointer, tolerance, and layer filter; Rust
    /// decides whether the hover is over a vertex, segment, feature body, or
    /// nothing. This keeps UI hover rendering out of MapLibre/TypeScript
    /// feature scans during an active edit session.
    pub fn hover_at(&self, query: HoverQuery) -> Option<HoverResult> {
        let tolerance_m = query.tolerance_m;
        if tolerance_m < 0.0
            || !tolerance_m.is_finite()
            || !query.lng.is_finite()
            || !query.lat.is_finite()
        {
            return None;
        }
        if !query.include_vertices && !query.include_segments && !query.include_features {
            return None;
        }

        let click = self.projection.project_coord([query.lng, query.lat]);
        let envelope = projected_pick_envelope(click, tolerance_m);
        let mut best: Option<HoverCandidate> = None;
        let mut candidate_count = 0usize;

        for indexed in self.rtree.locate_in_envelope_intersecting(&envelope) {
            if !snap_layer_allowed(&indexed.id, &query.target_layers) {
                continue;
            }
            candidate_count += 1;
            if candidate_count > query.max_candidates.max(1) {
                return None;
            }
            let Some(feature) = self.features.get(&indexed.id) else {
                continue;
            };
            let Some(projected) = self.projected.get(&indexed.id) else {
                continue;
            };
            self.collect_feature_hover_candidates(
                click,
                tolerance_m,
                &query,
                feature,
                projected,
                &mut best,
            );
        }

        best.map(|candidate| candidate.result)
    }

    pub fn pick(&self, lng: f64, lat: f64, tolerance_m: f64) -> Option<FeatureId> {
        if tolerance_m < 0.0 || !tolerance_m.is_finite() {
            return None;
        }
        let click = self.projection.project_coord([lng, lat]);
        let envelope = projected_pick_envelope(click, tolerance_m);
        let mut best: Option<PickCandidate> = None;
        for cand in self.rtree.locate_in_envelope_intersecting(&envelope) {
            let Some(projected) = self.projected.get(&cand.id) else {
                continue;
            };
            let d_m = projected_distance_m(click, projected);
            if !d_m.is_finite() || d_m > tolerance_m {
                continue;
            }
            let candidate = PickCandidate {
                id: cand.id.clone(),
                distance_m: d_m,
                family: interaction_family(projected),
                z_priority: feature_z_priority(&cand.id, projected),
            };
            match &best {
                Some(current) => {
                    if pick_candidate_order(&candidate, current) == std::cmp::Ordering::Less {
                        best = Some(candidate);
                    }
                }
                _ => best = Some(candidate),
            }
        }
        best.map(|candidate| candidate.id)
    }

    /// Phase C read probe: read a Polars row window from the persistent
    /// table owned by this session. Production keeps the same ownership
    /// model and upgrades the payload shape once Arrow IPC is available.
    #[cfg(feature = "enable_dataframes")]
    pub fn get_rows(
        &self,
        layer_name: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Option<RowWindow>, String> {
        let Some(table) = self.tables.get(layer_name) else {
            return Ok(None);
        };
        Ok(Some(table.get_rows(offset, limit)?))
    }

    /// Phase C read probe: return one row from the layer DataFrame, or None
    /// when either the layer or feature id is absent.
    #[cfg(feature = "enable_dataframes")]
    pub fn get_row(&self, layer_name: &str, feature_id: &str) -> Result<Option<LayerRow>, String> {
        let Some(table) = self.tables.get(layer_name) else {
            return Ok(None);
        };
        table.get_row(feature_id)
    }

    #[cfg(feature = "enable_dataframes")]
    pub fn edit_cell(
        &mut self,
        layer_name: &str,
        feature_id: &str,
        column: &str,
        value: Value,
    ) -> Result<Option<CellEditDiff>, String> {
        if column.trim().is_empty() {
            return Err("column name is required".to_string());
        }
        let encoded_id = FeatureId::new(format!("{layer_name}/{feature_id}"));
        if !self.features.contains_key(&encoded_id) {
            return Ok(None);
        }
        let before = self.features.get(&encoded_id).cloned();
        let Some(table) = self.tables.get_mut(layer_name) else {
            return Ok(None);
        };
        let diff = match table.edit_cell(feature_id, column, value.clone())? {
            Some(diff) => diff,
            None => return Ok(None),
        };
        if let Some(feature) = self.features.get_mut(&encoded_id) {
            feature.properties.insert(column.to_string(), value);
        }
        self.selection.last_edited = Some(encoded_id.clone());
        if let (Some(before), Some(after)) = (before, self.features.get(&encoded_id).cloned()) {
            self.record_diff(SessionDiff::single(
                "edit_cell",
                FeatureChange {
                    feature_id: after.id.clone(),
                    before: Some(before),
                    after: Some(after),
                },
            ));
        }
        Ok(Some(diff))
    }

    #[cfg(feature = "enable_dataframes")]
    pub fn edit_cells(
        &mut self,
        layer_name: &str,
        feature_ids: &[String],
        values: Map<String, Value>,
    ) -> Result<BulkCellEditResult, String> {
        for column in values.keys() {
            if column.trim().is_empty() {
                return Err("column name is required".to_string());
            }
        }
        if feature_ids.is_empty() || values.is_empty() {
            return Ok(BulkCellEditResult::default());
        }

        let mut requested = Vec::new();
        let mut seen = HashSet::new();
        let mut result = BulkCellEditResult::default();
        for raw_id in feature_ids {
            let feature_id = raw_id.trim();
            if feature_id.is_empty() || !seen.insert(feature_id.to_string()) {
                continue;
            }
            let encoded_id = FeatureId::new(format!("{layer_name}/{feature_id}"));
            if self.features.contains_key(&encoded_id) {
                requested.push((feature_id.to_string(), encoded_id));
            } else {
                result.missing_feature_ids.push(feature_id.to_string());
            }
        }
        if requested.is_empty() {
            return Ok(result);
        }

        let before_by_id: HashMap<FeatureId, Feature> = requested
            .iter()
            .filter_map(|(_, encoded_id)| {
                self.features
                    .get(encoded_id)
                    .cloned()
                    .map(|feature| (encoded_id.clone(), feature))
            })
            .collect();
        let table_feature_ids: Vec<String> = requested
            .iter()
            .map(|(feature_id, _)| feature_id.clone())
            .collect();
        let Some(table) = self.tables.get_mut(layer_name) else {
            result
                .missing_feature_ids
                .extend(table_feature_ids.iter().cloned());
            return Ok(result);
        };

        let mut table_result = table.edit_cells(&table_feature_ids, &values)?;
        result
            .missing_feature_ids
            .append(&mut table_result.missing_feature_ids);
        if table_result.changed.is_empty() {
            return Ok(result);
        }

        let mut changed_ids = HashSet::new();
        for diff in &table_result.changed {
            let encoded_id = FeatureId::new(format!("{layer_name}/{}", diff.feature_id));
            let Some(feature) = self.features.get_mut(&encoded_id) else {
                result.missing_feature_ids.push(diff.feature_id.clone());
                continue;
            };
            if let Some(value) = diff.after.clone() {
                feature.properties.insert(diff.column.clone(), value);
            }
            changed_ids.insert(encoded_id);
        }

        let mut changes = Vec::new();
        for (_, encoded_id) in requested {
            if !changed_ids.contains(&encoded_id) {
                continue;
            }
            let Some(before) = before_by_id.get(&encoded_id).cloned() else {
                continue;
            };
            let Some(after) = self.features.get(&encoded_id).cloned() else {
                continue;
            };
            if before == after {
                continue;
            }
            changes.push(FeatureChange {
                feature_id: after.id.clone(),
                before: Some(before),
                after: Some(after),
            });
            self.selection.last_edited = Some(encoded_id);
        }
        if !changes.is_empty() {
            self.record_diff(SessionDiff {
                kind: "edit_cells".to_string(),
                changes,
            });
        }
        result.changed = table_result.changed;
        Ok(result)
    }

    #[cfg(feature = "enable_dataframes")]
    pub fn delete_column(&mut self, layer_name: &str, column: &str) -> Result<usize, String> {
        if column.trim().is_empty() {
            return Err("column name is required".to_string());
        }
        let prefix = format!("{layer_name}/");
        let before_features: Vec<Feature> = self
            .features
            .iter()
            .filter_map(|(id, feature)| id.0.starts_with(&prefix).then_some(feature.clone()))
            .collect();

        let Some(table) = self.tables.get_mut(layer_name) else {
            return Ok(0);
        };
        let count = table.delete_column(column)?;
        if count == 0 {
            return Ok(0);
        }
        for (id, feature) in &mut self.features {
            if id.0.starts_with(&prefix) {
                feature.properties.remove(column);
            }
        }
        let changes = before_features
            .into_iter()
            .filter_map(|before| {
                let after = self.features.get(&before.id)?.clone();
                (before != after).then_some(FeatureChange {
                    feature_id: after.id.clone(),
                    before: Some(before),
                    after: Some(after),
                })
            })
            .collect();
        self.record_diff(SessionDiff {
            kind: "delete_column".to_string(),
            changes,
        });
        Ok(count)
    }

    fn record_diff(&mut self, diff: SessionDiff) {
        self.mutations.record(diff);
    }

    fn apply_diff(&mut self, diff: &SessionDiff) -> Result<(), String> {
        for change in &diff.changes {
            match &change.after {
                Some(feature) => self.put_feature_no_log(feature.clone())?,
                None => self.remove_feature_no_log(&change.feature_id)?,
            };
        }
        Ok(())
    }

    fn mutate_feature_geometry<F>(
        &mut self,
        id: &FeatureId,
        kind: &str,
        mut mutate: F,
    ) -> Result<Option<SessionDiff>, String>
    where
        F: FnMut(&mut Geometry) -> Result<bool, String>,
    {
        let Some(before) = self.features.get(id).cloned() else {
            return Ok(None);
        };
        let mut after = before.clone();
        if !mutate(&mut after.geometry)? {
            return Ok(None);
        }
        sanitize_geometry_for_drafting(&mut after.geometry, self.projection);
        if before == after {
            return Ok(None);
        }
        self.put_feature_no_log(after.clone())?;
        let diff = SessionDiff::single(
            kind,
            FeatureChange {
                feature_id: id.clone(),
                before: Some(before),
                after: Some(after),
            },
        );
        self.record_diff(diff.clone());
        Ok(Some(diff))
    }

    fn insert_feature_no_log(&mut self, feature: Feature) -> Result<(), String> {
        if !self.projection_initialized {
            self.set_projection_from_features(std::slice::from_ref(&feature));
        }
        #[cfg(feature = "enable_dataframes")]
        self.insert_table_feature(&feature)?;
        self.insert_index_entry(feature.clone());
        self.selection.last_edited = Some(feature.id);
        Ok(())
    }

    fn put_feature_no_log(&mut self, feature: Feature) -> Result<Option<Feature>, String> {
        if !self.projection_initialized {
            self.set_projection_from_features(std::slice::from_ref(&feature));
        }
        #[cfg(feature = "enable_dataframes")]
        self.replace_table_feature(&feature)?;
        let old = self.remove_index_entry(&feature.id);
        self.insert_index_entry(feature.clone());
        self.selection.last_edited = Some(feature.id);
        Ok(old)
    }

    fn remove_feature_no_log(&mut self, id: &FeatureId) -> Result<Option<Feature>, String> {
        if !self.features.contains_key(id) {
            return Ok(None);
        }
        #[cfg(feature = "enable_dataframes")]
        self.remove_table_feature(id)?;
        let feat = self
            .remove_index_entry(id)
            .expect("feature existed before index removal");
        self.prune_selection_id(id);
        if self.features.is_empty() {
            self.clear_projection();
        }
        Ok(Some(feat))
    }

    #[cfg(feature = "enable_dataframes")]
    fn insert_table_feature(&mut self, feature: &Feature) -> Result<(), String> {
        let Some((layer_name, feature_id)) = split_layer_feature_id(&feature.id) else {
            return Ok(());
        };
        let local_feature = Feature::with_properties(
            feature_id,
            feature.geometry.clone(),
            feature.properties.clone(),
        );

        match self.tables.get_mut(layer_name) {
            Some(table) => table.insert_feature(local_feature),
            None => {
                let table = LayerTable::from_features(layer_name, &[local_feature])?;
                self.tables.insert(layer_name.to_string(), table);
                Ok(())
            }
        }
    }

    #[cfg(feature = "enable_dataframes")]
    fn replace_table_feature(&mut self, feature: &Feature) -> Result<(), String> {
        let Some((layer_name, feature_id)) = split_layer_feature_id(&feature.id) else {
            return Ok(());
        };
        let local_feature = Feature::with_properties(
            feature_id,
            feature.geometry.clone(),
            feature.properties.clone(),
        );

        match self.tables.get_mut(layer_name) {
            Some(table) => {
                table.replace_feature(local_feature)?;
                Ok(())
            }
            None => {
                let table = LayerTable::from_features(layer_name, &[local_feature])?;
                self.tables.insert(layer_name.to_string(), table);
                Ok(())
            }
        }
    }

    #[cfg(feature = "enable_dataframes")]
    fn remove_table_feature(&mut self, id: &FeatureId) -> Result<(), String> {
        let Some((layer_name, feature_id)) = split_layer_feature_id(id) else {
            return Ok(());
        };

        let should_drop = match self.tables.get_mut(layer_name) {
            Some(table) => {
                table.remove_feature(feature_id)?;
                table.height() == 0
            }
            None => false,
        };
        if should_drop {
            self.tables.remove(layer_name);
        }
        Ok(())
    }

    /// Drop many rows from one layer's DataFrame in a single table rebuild.
    /// `layer_name` is the table key (`{scope}:{layer}` for scoped features);
    /// `feature_ids` are the local (unscoped) feature ids. Used by the bulk
    /// batch-delete path so removing `count` rows is O(H) not O(count * H).
    #[cfg(feature = "enable_dataframes")]
    fn remove_table_features(
        &mut self,
        layer_name: &str,
        feature_ids: &HashSet<String>,
    ) -> Result<(), String> {
        if feature_ids.is_empty() {
            return Ok(());
        }
        let id_refs: HashSet<&str> = feature_ids.iter().map(String::as_str).collect();
        let should_drop = match self.tables.get_mut(layer_name) {
            Some(table) => {
                table.remove_features(&id_refs)?;
                table.height() == 0
            }
            None => false,
        };
        if should_drop {
            self.tables.remove(layer_name);
        }
        Ok(())
    }

    fn insert_index_entry(&mut self, feature: Feature) {
        let projected = project_geometry(&self.projection, &feature.geometry);
        let bbox = projected_geometry_bbox(&projected);
        self.rtree.insert(IndexedFeature {
            id: feature.id.clone(),
            bbox,
        });
        self.projected.insert(feature.id.clone(), projected);
        self.features.insert(feature.id.clone(), feature);
    }

    fn remove_index_entry(&mut self, id: &FeatureId) -> Option<Feature> {
        let feat = self.features.remove(id)?;
        let projected = self
            .projected
            .remove(id)
            .unwrap_or_else(|| project_geometry(&self.projection, &feat.geometry));
        let target = IndexedFeature {
            id: id.clone(),
            bbox: projected_geometry_bbox(&projected),
        };
        // The bbox in `target` matches the indexed entry exactly because
        // we cache geometry-derived bboxes; `RTree::remove` uses it to
        // narrow the search and PartialEq (by id) for the final match.
        let _ = self.rtree.remove(&target);
        Some(feat)
    }

    fn set_projection_from_features(&mut self, features: &[Feature]) {
        self.set_projection_from_features_with_config(features, ProjectionConfig::default());
    }

    fn set_projection_from_features_with_config(
        &mut self,
        features: &[Feature],
        config: ProjectionConfig,
    ) {
        self.projection = projection_from_features_with_config(features, config);
        self.projection_initialized = !features.is_empty();
        self.rebuild_projected_index();
    }

    fn clear_projection(&mut self) {
        self.projection = ProjectionMetadata::default();
        self.projection_initialized = false;
        self.projected.clear();
        self.rtree = RTree::new();
    }

    fn rebuild_projected_index(&mut self) {
        self.projected.clear();
        let mut indexed = Vec::with_capacity(self.features.len());
        for feature in self.features.values() {
            let projected = project_geometry(&self.projection, &feature.geometry);
            indexed.push(IndexedFeature {
                id: feature.id.clone(),
                bbox: projected_geometry_bbox(&projected),
            });
            self.projected.insert(feature.id.clone(), projected);
        }
        self.rtree = RTree::bulk_load(indexed);
    }

    fn prune_selection_id(&mut self, id: &FeatureId) {
        self.selection.selected.retain(|x| x != id);
        if self.selection.hovered.as_ref() == Some(id) {
            self.selection.hovered = None;
        }
        if self.selection.last_edited.as_ref() == Some(id) {
            self.selection.last_edited = None;
        }
    }

    /// Clear every selection slot (`selected` / `hovered` / `last_edited`) that
    /// points into a bulk-removed id set, in one pass. The per-id
    /// [`Self::prune_selection_id`] is fine for a single edit; a scope teardown
    /// or batch delete must not run it once per feature.
    fn prune_selection_ids(&mut self, removed: &HashSet<FeatureId>) {
        self.selection.selected.retain(|id| !removed.contains(id));
        if self
            .selection
            .hovered
            .as_ref()
            .is_some_and(|id| removed.contains(id))
        {
            self.selection.hovered = None;
        }
        if self
            .selection
            .last_edited
            .as_ref()
            .is_some_and(|id| removed.contains(id))
        {
            self.selection.last_edited = None;
        }
    }

    /// Drop a set of feature ids from the spatial model (features + projected
    /// geometry + selection) in bulk, then rebuild the R-tree ONCE reusing the
    /// current projection origin. Callers own the DataFrame-table removal
    /// (whole-table drop for scope unmount, row filter for layer delete) and
    /// the mutation-log record.
    ///
    /// This never re-fits the projection plane: removing features must not
    /// shift the metric origin under still-mounted scopes, so we keep
    /// `self.projection` as-is (`rebuild_projected_index` reuses it) and only
    /// `clear_projection` when the last feature is gone.
    fn drop_feature_ids_bulk(&mut self, removed: &HashSet<FeatureId>) {
        if removed.is_empty() {
            return;
        }
        self.features.retain(|id, _| !removed.contains(id));
        self.prune_selection_ids(removed);
        if self.features.is_empty() {
            self.clear_projection();
        } else {
            self.rebuild_projected_index();
        }
    }

    fn unmount_scope_inner(&mut self, scope: &str) -> Result<usize, String> {
        self.mounted_scopes.remove(scope);
        self.scope_input_stats.remove(scope);
        // Collect the scope's feature ids once, then tear the whole scope down
        // in bulk. Removing them one at a time used to rebuild the entire layer
        // DataFrame per feature (to_features + from_features), making
        // edit-session exit O(H^2) and freezing the tab on large transformers —
        // see ds-web/docs/PAIN_HISTORY.md tombstone "scope unmount O(N^2)".
        let removed: HashSet<FeatureId> = self
            .features
            .keys()
            .filter(|id| feature_id_scope(id) == scope)
            .cloned()
            .collect();
        let count = removed.len();
        if count == 0 {
            return Ok(0);
        }
        // Drop the scope's DataFrame tables wholesale. Every scoped table key is
        // exactly `{scope}:{layer}` (normalize_scope forbids ':' and '/' inside
        // a scope), so matching on the `{scope}:` prefix is exact — the trailing
        // separator is required so scope `a` never nukes scope `ab`'s tables.
        #[cfg(feature = "enable_dataframes")]
        {
            let table_prefix = format!("{scope}{SCOPE_SEPARATOR}");
            self.tables
                .retain(|layer_key, _| !layer_key.starts_with(&table_prefix));
        }
        self.drop_feature_ids_bulk(&removed);
        Ok(count)
    }

    fn collect_feature_snap_candidates(
        &self,
        click: [f64; 2],
        tolerance_m: f64,
        query: &SnapQuery,
        feature: &Feature,
        projected: &ProjectedGeometry,
        best: &mut Option<SnapCandidate>,
    ) {
        match (&feature.geometry, projected) {
            (
                Geometry::Point { coords },
                ProjectedGeometry::Point {
                    coords: projected_coord,
                },
            ) => {
                if query.snap_to_vertices {
                    self.collect_vertex_snap_candidate(
                        click,
                        tolerance_m,
                        feature,
                        None,
                        Some(0),
                        *coords,
                        *projected_coord,
                        best,
                    );
                }
            }
            (
                Geometry::LineString { coords },
                ProjectedGeometry::LineString {
                    coords: projected_coords,
                },
            ) => {
                self.collect_line_snap_candidates(
                    click,
                    tolerance_m,
                    query,
                    feature,
                    0,
                    coords,
                    projected_coords,
                    best,
                );
            }
            (
                Geometry::Polygon { rings },
                ProjectedGeometry::Polygon {
                    rings: projected_rings,
                },
            ) => {
                for (ring_index, (ring, projected_ring)) in
                    rings.iter().zip(projected_rings).enumerate()
                {
                    self.collect_line_snap_candidates(
                        click,
                        tolerance_m,
                        query,
                        feature,
                        ring_index,
                        ring,
                        projected_ring,
                        best,
                    );
                }
            }
            _ => {}
        }
    }

    // Each argument maps directly to metadata emitted by the accepted snap.
    // Keeping them named here is clearer than hiding interaction state in an
    // opaque tuple shared across the point, line, and polygon branches.
    #[allow(clippy::too_many_arguments)]
    fn collect_line_snap_candidates(
        &self,
        click: [f64; 2],
        tolerance_m: f64,
        query: &SnapQuery,
        feature: &Feature,
        ring_index: usize,
        coords: &[[f64; 2]],
        projected_coords: &[[f64; 2]],
        best: &mut Option<SnapCandidate>,
    ) {
        if query.snap_to_vertices {
            for (vertex_index, (coord, projected_coord)) in
                coords.iter().zip(projected_coords).enumerate()
            {
                self.collect_vertex_snap_candidate(
                    click,
                    tolerance_m,
                    feature,
                    Some(ring_index),
                    Some(vertex_index),
                    *coord,
                    *projected_coord,
                    best,
                );
            }
        }
        if (!query.snap_to_segments && !query.snap_to_midpoints)
            || coords.len() < 2
            || projected_coords.len() < 2
        {
            return;
        }
        for segment_index in 0..coords.len().saturating_sub(1) {
            let a = projected_coords[segment_index];
            let b = projected_coords[segment_index + 1];
            if query.snap_to_midpoints {
                let projected_mid = interpolate_projected(a, b, 0.5);
                let distance_m = projected_distance(click, projected_mid);
                if distance_m <= tolerance_m {
                    accept_snap_candidate(
                        best,
                        SnapCandidate {
                            priority: snap_priority(SnapTargetKind::Midpoint),
                            result: SnapResult {
                                target_kind: SnapTargetKind::Midpoint,
                                feature_id: Some(feature.id.clone()),
                                layer_id: feature_layer_name(&feature.id).map(str::to_string),
                                coord: self.projection.unproject_coord(projected_mid),
                                distance_m,
                                ring_index: Some(ring_index),
                                vertex_index: None,
                                segment_index: Some(segment_index),
                                t: Some(0.5),
                            },
                        },
                    );
                }
            }
            if query.snap_to_segments {
                let t = projected_segment_t(click, a, b).clamp(0.0, 1.0);
                if !(SNAP_ENDPOINT_EXCLUSION_T..=1.0 - SNAP_ENDPOINT_EXCLUSION_T).contains(&t) {
                    continue;
                }
                let projected_hit = interpolate_projected(a, b, t);
                let distance_m = projected_distance(click, projected_hit);
                if distance_m <= tolerance_m {
                    accept_snap_candidate(
                        best,
                        SnapCandidate {
                            priority: snap_priority(SnapTargetKind::Edge),
                            result: SnapResult {
                                target_kind: SnapTargetKind::Edge,
                                feature_id: Some(feature.id.clone()),
                                layer_id: feature_layer_name(&feature.id).map(str::to_string),
                                coord: self.projection.unproject_coord(projected_hit),
                                distance_m,
                                ring_index: Some(ring_index),
                                vertex_index: None,
                                segment_index: Some(segment_index),
                                t: Some(t),
                            },
                        },
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_vertex_snap_candidate(
        &self,
        click: [f64; 2],
        tolerance_m: f64,
        feature: &Feature,
        ring_index: Option<usize>,
        vertex_index: Option<usize>,
        coord: [f64; 2],
        projected_coord: [f64; 2],
        best: &mut Option<SnapCandidate>,
    ) {
        let distance_m = projected_distance(click, projected_coord);
        if distance_m > tolerance_m {
            return;
        }
        accept_snap_candidate(
            best,
            SnapCandidate {
                priority: snap_priority(SnapTargetKind::Vertex),
                result: SnapResult {
                    target_kind: SnapTargetKind::Vertex,
                    feature_id: Some(feature.id.clone()),
                    layer_id: feature_layer_name(&feature.id).map(str::to_string),
                    coord,
                    distance_m,
                    ring_index,
                    vertex_index,
                    segment_index: None,
                    t: None,
                },
            },
        );
    }

    fn collect_feature_hover_candidates(
        &self,
        click: [f64; 2],
        tolerance_m: f64,
        query: &HoverQuery,
        feature: &Feature,
        projected: &ProjectedGeometry,
        best: &mut Option<HoverCandidate>,
    ) {
        let z_priority = feature_z_priority(&feature.id, projected);
        let family = interaction_family(projected);
        match (&feature.geometry, projected) {
            (
                Geometry::Point { coords },
                ProjectedGeometry::Point {
                    coords: projected_coord,
                },
            ) => {
                if query.include_vertices {
                    collect_vertex_hover_candidate(
                        click,
                        tolerance_m,
                        feature,
                        None,
                        Some(0),
                        *coords,
                        *projected_coord,
                        family,
                        z_priority,
                        best,
                    );
                }
            }
            (
                Geometry::LineString { coords },
                ProjectedGeometry::LineString {
                    coords: projected_coords,
                },
            ) => {
                collect_line_hover_candidates(
                    &self.projection,
                    click,
                    tolerance_m,
                    query,
                    feature,
                    0,
                    coords,
                    projected_coords,
                    family,
                    z_priority,
                    best,
                );
            }
            (
                Geometry::Polygon { rings },
                ProjectedGeometry::Polygon {
                    rings: projected_rings,
                },
            ) => {
                for (ring_index, (ring, projected_ring)) in
                    rings.iter().zip(projected_rings).enumerate()
                {
                    collect_line_hover_candidates(
                        &self.projection,
                        click,
                        tolerance_m,
                        query,
                        feature,
                        ring_index,
                        ring,
                        projected_ring,
                        family,
                        z_priority,
                        best,
                    );
                }
            }
            _ => {}
        }

        if query.include_features {
            let distance_m = projected_distance_m(click, projected);
            if distance_m <= tolerance_m {
                accept_hover_candidate(
                    best,
                    HoverCandidate {
                        priority: hover_priority(HoverTargetKind::Feature),
                        family,
                        z_priority,
                        result: HoverResult {
                            target_kind: HoverTargetKind::Feature,
                            feature_id: feature.id.clone(),
                            layer_id: feature_layer_name(&feature.id).map(str::to_string),
                            distance_m,
                            coord: None,
                            ring_index: None,
                            vertex_index: None,
                            segment_index: None,
                            t: None,
                        },
                    },
                );
            }
        }
    }
}

#[cfg(feature = "enable_dataframes")]
fn build_layer_tables(
    table_features: HashMap<String, Vec<Feature>>,
) -> HashMap<String, LayerTable> {
    table_features
        .into_iter()
        .map(|(layer_name, features)| {
            let table = LayerTable::from_features(layer_name.clone(), &features)
                .expect("session layer table builds from valid session features");
            (layer_name, table)
        })
        .collect()
}

#[cfg(feature = "enable_dataframes")]
fn remove_table_feature_seed(table_features: &mut HashMap<String, Vec<Feature>>, id: &FeatureId) {
    let Some((layer_name, feature_id)) = split_layer_feature_id(id) else {
        return;
    };
    let should_drop = match table_features.get_mut(layer_name) {
        Some(features) => {
            features.retain(|feature| feature.id.0 != feature_id);
            features.is_empty()
        }
        None => false,
    };
    if should_drop {
        table_features.remove(layer_name);
    }
}

fn encode_layer_feature_id(layer_name: &str, feature_id: &str) -> FeatureId {
    FeatureId::new(format!("{layer_name}/{feature_id}"))
}

fn mint_prefix_layer_name(layer_name: &str) -> &str {
    layer_name
        .rsplit_once(SCOPE_SEPARATOR)
        .map(|(_, local)| local)
        .unwrap_or(layer_name)
}

/// Prefix for newly-minted feature ids on this layer. Mirrors the shape the
/// retired TS-side `feature_id_prefix` produced (lowercase ascii alnum,
/// collapse runs of separators to `_`, trim leading/trailing underscores,
/// guarantee an alpha start) so ids generated before WASM took over identity
/// minting remain sortable next to fresh ones in the attribute table.
fn feature_id_prefix(layer_name: &str) -> String {
    let mut out = String::with_capacity(layer_name.len());
    let mut last_sep = false;
    for ch in layer_name.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_sep = false;
        } else if !last_sep && !out.is_empty() {
            out.push('_');
            last_sep = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    while out.starts_with('_') {
        out.remove(0);
    }
    if out.is_empty() {
        return "feature".to_string();
    }
    if !out.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return format!("feature_{out}");
    }
    out
}

fn sanitized_created_feature_properties(properties: Map<String, Value>) -> Map<String, Value> {
    properties
        .into_iter()
        .filter(|(key, _)| {
            !matches!(
                key.to_ascii_lowercase().as_str(),
                "geometry" | "geom" | "the_geom" | "wkt" | "geometry_wkt" | "type" | "bbox"
            )
        })
        .collect()
}

fn encode_scoped_layer_feature_id(scope: &str, layer_name: &str, feature_id: &str) -> FeatureId {
    FeatureId::new(format!("{scope}{SCOPE_SEPARATOR}{layer_name}/{feature_id}"))
}

fn normalize_scope(scope: &str) -> Result<&str, String> {
    let scope = scope.trim();
    if scope.is_empty() {
        return Err("scope is required".to_string());
    }
    if scope.contains(SCOPE_SEPARATOR) || scope.contains('/') {
        return Err(format!("scope `{scope}` contains a reserved separator"));
    }
    Ok(scope)
}

fn split_scoped_feature_id(id: &FeatureId) -> Option<(&str, &str, &str)> {
    let (head, feature_id) = id.0.split_once('/')?;
    if feature_id.is_empty() {
        return None;
    }
    let (scope, layer_name) = head.split_once(SCOPE_SEPARATOR)?;
    (!scope.is_empty() && !layer_name.is_empty()).then_some((scope, layer_name, feature_id))
}

fn feature_id_scope(id: &FeatureId) -> &str {
    split_scoped_feature_id(id)
        .map(|(scope, _, _)| scope)
        .unwrap_or("")
}

fn scoped_feature(scope: &str, mut feature: Feature) -> Result<Feature, String> {
    let (layer_name, feature_id) =
        if let Some((_, layer, feature)) = split_scoped_feature_id(&feature.id) {
            (layer, feature)
        } else if let Some((layer, feature)) = split_layer_feature_id_any(&feature.id) {
            (layer, feature)
        } else {
            return Err(format!(
                "feature id `{}` must be `layer/feature` or `scope:layer/feature`",
                feature.id.0
            ));
        };
    feature.id = encode_scoped_layer_feature_id(scope, layer_name, feature_id);
    Ok(feature)
}

fn geometry_type_name(geometry: &Geometry) -> &'static str {
    match geometry {
        Geometry::Point { .. } => "Point",
        Geometry::LineString { .. } => "LineString",
        Geometry::Polygon { .. } => "Polygon",
    }
}

#[derive(Default)]
struct LayerStatusAccumulator {
    input_feature_count: usize,
    feature_count: usize,
    skipped_missing_id_count: usize,
    skipped_unsupported_geometry_count: usize,
    geometry_types: HashSet<&'static str>,
}

impl LayerStatusAccumulator {
    fn add_geometry(&mut self, geometry: &Geometry) {
        self.geometry_types.insert(geometry_type_name(geometry));
    }

    fn geometry_type(&self) -> Option<String> {
        match self.geometry_types.len() {
            0 => None,
            1 => self
                .geometry_types
                .iter()
                .next()
                .map(|kind| (*kind).to_string()),
            _ => Some("Mixed".to_string()),
        }
    }

    fn skipped_feature_count(&self) -> usize {
        self.skipped_missing_id_count + self.skipped_unsupported_geometry_count
    }
}

struct SessionLayerParseResult {
    features: Vec<Feature>,
    stats: ScopeInputStats,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionFeatureSkipReason {
    MissingId,
    UnsupportedGeometry,
}

fn session_features_from_layers(
    layers: Vec<SessionLayerInput>,
) -> Result<SessionLayerParseResult, String> {
    let mut features = Vec::new();
    let mut stats = ScopeInputStats::default();
    for layer in layers {
        let layer_name = normalize_layer_name(&layer.name)?;
        stats.layers.entry(layer_name.to_string()).or_default();
        for (idx, feature) in geojson_features(&layer.geojson)
            .map_err(|err| format!("layer {layer_name}: {err}"))?
            .into_iter()
            .enumerate()
        {
            stats.input_feature_count += 1;
            let layer_stats = stats
                .layers
                .get_mut(layer_name)
                .expect("layer stats entry was created before parsing features");
            layer_stats.input_feature_count += 1;
            match session_features_from_geojson_inner(layer_name, feature.clone()) {
                Ok(Ok(parsed_features)) => features.extend(parsed_features),
                Ok(Err(SessionFeatureSkipReason::MissingId)) => {
                    stats.skipped_missing_id_count += 1;
                    layer_stats.skipped_missing_id_count += 1;
                }
                Ok(Err(SessionFeatureSkipReason::UnsupportedGeometry)) => {
                    stats.skipped_unsupported_geometry_count += 1;
                    layer_stats.skipped_unsupported_geometry_count += 1;
                }
                Err(err) => return Err(format!("layer {layer_name}, feature {idx}: {err}")),
            }
        }
    }
    Ok(SessionLayerParseResult { features, stats })
}

pub fn session_feature_from_geojson(
    layer_name: &str,
    feature: Value,
) -> Result<Option<Feature>, String> {
    match session_feature_from_geojson_inner(layer_name, feature)? {
        Ok(feature) => Ok(Some(feature)),
        Err(_) => Ok(None),
    }
}

fn session_feature_from_geojson_inner(
    layer_name: &str,
    feature: Value,
) -> Result<Result<Feature, SessionFeatureSkipReason>, String> {
    let layer_name = normalize_index_layer_name(layer_name)?;
    if feature.get("type").and_then(Value::as_str) != Some("Feature") {
        return Err("expected Feature".to_string());
    }
    let Some(feature_id) = geojson_feature_id(&feature) else {
        return Ok(Err(SessionFeatureSkipReason::MissingId));
    };
    let Some(geometry) = feature
        .get("geometry")
        .and_then(session_geometry_from_geojson)
    else {
        return Ok(Err(SessionFeatureSkipReason::UnsupportedGeometry));
    };
    Ok(Ok(Feature::with_properties(
        encode_layer_feature_id(layer_name, &feature_id).0,
        geometry,
        geojson_feature_properties(&feature),
    )))
}

fn session_features_from_geojson_inner(
    layer_name: &str,
    feature: Value,
) -> Result<Result<Vec<Feature>, SessionFeatureSkipReason>, String> {
    let layer_name = normalize_index_layer_name(layer_name)?;
    if feature.get("type").and_then(Value::as_str) != Some("Feature") {
        return Err("expected Feature".to_string());
    }
    let Some(feature_id) = geojson_feature_id(&feature) else {
        return Ok(Err(SessionFeatureSkipReason::MissingId));
    };
    let Some(geometry_value) = feature.get("geometry") else {
        return Ok(Err(SessionFeatureSkipReason::UnsupportedGeometry));
    };

    if geometry_value.get("type").and_then(Value::as_str) == Some("MultiLineString") {
        let Some(parts) = session_multiline_parts_from_geojson(geometry_value) else {
            return Ok(Err(SessionFeatureSkipReason::UnsupportedGeometry));
        };
        let mut features = Vec::with_capacity(parts.len());
        for (idx, coords) in parts.into_iter().enumerate() {
            let part_id = multiline_part_feature_id(&feature_id, idx);
            let mut properties = geojson_feature_properties(&feature);
            properties.insert("id".to_string(), Value::String(part_id.clone()));
            if idx > 0 {
                properties.insert(
                    "_ds_parent_feature_id".to_string(),
                    Value::String(feature_id.clone()),
                );
                properties.insert(
                    "_ds_part_index".to_string(),
                    Value::Number(serde_json::Number::from(idx as u64)),
                );
            }
            features.push(Feature::with_properties(
                encode_layer_feature_id(layer_name, &part_id).0,
                Geometry::LineString { coords },
                properties,
            ));
        }
        return Ok(Ok(features));
    }

    match session_feature_from_geojson_inner(layer_name, feature)? {
        Ok(feature) => Ok(Ok(vec![feature])),
        Err(reason) => Ok(Err(reason)),
    }
}

fn session_multiline_parts_from_geojson(geometry: &Value) -> Option<Vec<Vec<[f64; 2]>>> {
    let parts_value = geometry.get("coordinates")?.as_array()?;
    let mut parts = Vec::new();
    for part_value in parts_value {
        let coords = read_lonlat_pairs(part_value)?;
        if coords.len() >= 2 {
            parts.push(coords);
        }
    }
    (!parts.is_empty()).then_some(parts)
}

fn multiline_part_feature_id(feature_id: &str, part_index: usize) -> String {
    if part_index == 0 {
        feature_id.to_string()
    } else {
        format!("{feature_id}__part_{}", part_index + 1)
    }
}

fn normalize_layer_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("layer name is required".to_string());
    }
    if name.contains('/') || name.contains(SCOPE_SEPARATOR) {
        return Err(format!("layer name `{name}` contains a reserved separator"));
    }
    Ok(name)
}

fn normalize_index_layer_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("layer name is required".to_string());
    }
    if name.contains('/') {
        return Err(format!("layer name `{name}` contains a reserved separator"));
    }
    Ok(name)
}

fn geojson_features(value: &Value) -> Result<Vec<&Value>, String> {
    match value.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => value
            .get("features")
            .and_then(Value::as_array)
            .map(|features| features.iter().collect())
            .ok_or_else(|| "FeatureCollection must have a features array".to_string()),
        Some("Feature") => Ok(vec![value]),
        Some(other) => Err(format!(
            "expected FeatureCollection or Feature, got {other}"
        )),
        None => Err("GeoJSON object must have a type".to_string()),
    }
}

fn geojson_feature_properties(feature: &Value) -> Map<String, Value> {
    let mut properties = match feature.get("properties") {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    if let Some(id) = feature.get("id").cloned() {
        properties.entry("id".to_string()).or_insert(id);
    }
    properties
}

fn session_geometry_from_geojson(geometry: &Value) -> Option<Geometry> {
    match geometry.get("type").and_then(Value::as_str) {
        Some("Point") => {
            let coord = read_lonlat_pair(geometry.get("coordinates")?)?;
            Some(Geometry::Point { coords: coord })
        }
        Some("LineString") => {
            let coords = read_lonlat_pairs(geometry.get("coordinates")?)?;
            (coords.len() >= 2).then_some(Geometry::LineString { coords })
        }
        Some("Polygon") => {
            let rings_value = geometry.get("coordinates")?.as_array()?;
            let mut rings = Vec::with_capacity(rings_value.len());
            for ring_value in rings_value {
                let ring = read_lonlat_pairs(ring_value)?;
                if ring.len() < 3 {
                    return None;
                }
                rings.push(ring);
            }
            (!rings.is_empty()).then_some(Geometry::Polygon { rings })
        }
        _ => None,
    }
}

fn read_lonlat_pairs(value: &Value) -> Option<Vec<[f64; 2]>> {
    value
        .as_array()?
        .iter()
        .map(read_lonlat_pair)
        .collect::<Option<Vec<_>>>()
}

fn read_lonlat_pair(value: &Value) -> Option<[f64; 2]> {
    let pair = value.as_array()?;
    if pair.len() < 2 {
        return None;
    }
    let lng = pair[0].as_f64()?;
    let lat = pair[1].as_f64()?;
    (lng.is_finite() && lat.is_finite()).then_some([lng, lat])
}

fn split_layer_feature_id_any(id: &FeatureId) -> Option<(&str, &str)> {
    let (layer_name, feature_id) = id.0.split_once('/')?;
    (!layer_name.is_empty() && !feature_id.is_empty()).then_some((layer_name, feature_id))
}

fn feature_layer_name(id: &FeatureId) -> Option<&str> {
    split_scoped_feature_id(id)
        .map(|(_, layer, _)| layer)
        .or_else(|| split_layer_feature_id_any(id).map(|(layer, _)| layer))
}

fn feature_scoped_layer_name(id: &FeatureId) -> Option<String> {
    split_scoped_feature_id(id).map(|(scope, layer, _)| format!("{scope}{SCOPE_SEPARATOR}{layer}"))
}

fn snap_layer_allowed(id: &FeatureId, target_layers: &[String]) -> bool {
    if target_layers.is_empty() {
        return true;
    }
    let raw_layer = feature_layer_name(id);
    let scoped_layer = feature_scoped_layer_name(id);
    target_layers.iter().any(|target| {
        raw_layer.is_some_and(|layer| target == layer)
            || scoped_layer.as_ref().is_some_and(|layer| target == layer)
    })
}

/// Layer filter for polygon (lasso) selection. Unlike [`snap_layer_allowed`],
/// an empty `target_layers` selects **nothing** instead of every mounted
/// layer. A lasso has no single-click target to disambiguate, so "no target
/// layers" must never silently fan out to an exact-geometry scan of the whole
/// session — that was the documented main-thread freeze footgun (see
/// `ds-web/docs/wasm-freeze-suspicion-handoff.md`). Callers that want an
/// all-layers lasso pass the explicit mounted layer names; ds-web resolves
/// them in `EditSessionIndexImpl.selectByPolygon` before crossing the boundary.
fn polygon_layer_allowed(id: &FeatureId, target_layers: &[String]) -> bool {
    if target_layers.is_empty() {
        return false;
    }
    snap_layer_allowed(id, target_layers)
}

fn interaction_family(projected: &ProjectedGeometry) -> InteractionFamily {
    match projected {
        ProjectedGeometry::Point { .. } => InteractionFamily::Point,
        ProjectedGeometry::LineString { .. } => InteractionFamily::Line,
        ProjectedGeometry::Polygon { .. } => InteractionFamily::Polygon,
    }
}

fn feature_z_priority(id: &FeatureId, projected: &ProjectedGeometry) -> u8 {
    match projected {
        ProjectedGeometry::Point { .. } => point_layer_z_priority(id),
        ProjectedGeometry::LineString { .. } => 20 + line_layer_z_priority(id),
        ProjectedGeometry::Polygon { .. } => 40,
    }
}

fn point_layer_z_priority(id: &FeatureId) -> u8 {
    let Some(layer) = feature_layer_name(id) else {
        return 9;
    };
    match layer {
        "lv_poles" | "mv_poles" | "tapping_poles" | "poles" | "tr" | "transformers" => 0,
        "customers" => 1,
        _ if layer.contains("pole") => 0,
        _ => 5,
    }
}

fn line_layer_z_priority(id: &FeatureId) -> u8 {
    let Some(layer) = feature_layer_name(id) else {
        return 9;
    };
    match layer {
        "service_cables" => 0,
        // The MV line family shares LV-line z-priority.
        "lv_lines" | "mv_lines" | "mv_spans" => 1,
        _ => 5,
    }
}

fn snap_priority(kind: SnapTargetKind) -> u8 {
    match kind {
        SnapTargetKind::Vertex => 0,
        SnapTargetKind::SelfSnap => 1,
        SnapTargetKind::Midpoint => 2,
        SnapTargetKind::Edge => 3,
    }
}

fn accept_snap_candidate(best: &mut Option<SnapCandidate>, candidate: SnapCandidate) {
    let Some(current) = best else {
        *best = Some(candidate);
        return;
    };
    if snap_candidate_order(&candidate, current) == std::cmp::Ordering::Less {
        *current = candidate;
    }
}

// Hover candidates carry both geometric and layer-order identity; the explicit
// inputs prevent either from becoming implicit mutable session state.
#[allow(clippy::too_many_arguments)]
fn collect_line_hover_candidates(
    projection: &ProjectionMetadata,
    click: [f64; 2],
    tolerance_m: f64,
    query: &HoverQuery,
    feature: &Feature,
    ring_index: usize,
    coords: &[[f64; 2]],
    projected_coords: &[[f64; 2]],
    family: InteractionFamily,
    z_priority: u8,
    best: &mut Option<HoverCandidate>,
) {
    if query.include_vertices {
        for (vertex_index, (coord, projected_coord)) in
            coords.iter().zip(projected_coords).enumerate()
        {
            collect_vertex_hover_candidate(
                click,
                tolerance_m,
                feature,
                Some(ring_index),
                Some(vertex_index),
                *coord,
                *projected_coord,
                family,
                z_priority,
                best,
            );
        }
    }

    if !query.include_segments || coords.len() < 2 || projected_coords.len() < 2 {
        return;
    }

    for segment_index in 0..coords.len().saturating_sub(1) {
        let a = projected_coords[segment_index];
        let b = projected_coords[segment_index + 1];
        let t = projected_segment_t(click, a, b).clamp(0.0, 1.0);
        let projected_hit = interpolate_projected(a, b, t);
        let distance_m = projected_distance(click, projected_hit);
        if distance_m <= tolerance_m {
            accept_hover_candidate(
                best,
                HoverCandidate {
                    priority: hover_priority(HoverTargetKind::Segment),
                    family,
                    z_priority,
                    result: HoverResult {
                        target_kind: HoverTargetKind::Segment,
                        feature_id: feature.id.clone(),
                        layer_id: feature_layer_name(&feature.id).map(str::to_string),
                        distance_m,
                        coord: Some(projection.unproject_coord(projected_hit)),
                        ring_index: Some(ring_index),
                        vertex_index: None,
                        segment_index: Some(segment_index),
                        t: Some(t),
                    },
                },
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_vertex_hover_candidate(
    click: [f64; 2],
    tolerance_m: f64,
    feature: &Feature,
    ring_index: Option<usize>,
    vertex_index: Option<usize>,
    coord: [f64; 2],
    projected_coord: [f64; 2],
    family: InteractionFamily,
    z_priority: u8,
    best: &mut Option<HoverCandidate>,
) {
    let distance_m = projected_distance(click, projected_coord);
    if distance_m > tolerance_m {
        return;
    }
    accept_hover_candidate(
        best,
        HoverCandidate {
            priority: hover_priority(HoverTargetKind::Vertex),
            family,
            z_priority,
            result: HoverResult {
                target_kind: HoverTargetKind::Vertex,
                feature_id: feature.id.clone(),
                layer_id: feature_layer_name(&feature.id).map(str::to_string),
                distance_m,
                coord: Some(coord),
                ring_index,
                vertex_index,
                segment_index: None,
                t: None,
            },
        },
    );
}

fn hover_priority(kind: HoverTargetKind) -> u8 {
    match kind {
        HoverTargetKind::Vertex => 0,
        HoverTargetKind::Segment => 1,
        HoverTargetKind::Feature => 2,
    }
}

fn accept_hover_candidate(best: &mut Option<HoverCandidate>, candidate: HoverCandidate) {
    let Some(current) = best else {
        *best = Some(candidate);
        return;
    };
    if hover_candidate_order(&candidate, current) == std::cmp::Ordering::Less {
        *current = candidate;
    }
}

fn point_line_interaction_order(
    left: InteractionFamily,
    right: InteractionFamily,
) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (InteractionFamily::Point, InteractionFamily::Line) => Some(std::cmp::Ordering::Less),
        (InteractionFamily::Line, InteractionFamily::Point) => Some(std::cmp::Ordering::Greater),
        _ => None,
    }
}

fn pick_candidate_order(left: &PickCandidate, right: &PickCandidate) -> std::cmp::Ordering {
    let distance_order = left
        .distance_m
        .partial_cmp(&right.distance_m)
        .unwrap_or(std::cmp::Ordering::Equal);
    if let Some(order) = point_line_interaction_order(left.family, right.family) {
        return order.then_with(|| left.id.0.cmp(&right.id.0));
    }
    if (left.distance_m - right.distance_m).abs() > PICK_DISTANCE_TIE_M {
        return distance_order;
    }
    left.z_priority
        .cmp(&right.z_priority)
        .then_with(|| left.id.0.cmp(&right.id.0))
}

fn snap_candidate_order(left: &SnapCandidate, right: &SnapCandidate) -> std::cmp::Ordering {
    let distance_order = left
        .result
        .distance_m
        .partial_cmp(&right.result.distance_m)
        .unwrap_or(std::cmp::Ordering::Equal);
    if (left.result.distance_m - right.result.distance_m).abs() > SNAP_DISTANCE_TIE_M {
        return distance_order;
    }
    left.priority.cmp(&right.priority).then_with(|| {
        snap_result_stable_key(&left.result).cmp(&snap_result_stable_key(&right.result))
    })
}

fn hover_candidate_order(left: &HoverCandidate, right: &HoverCandidate) -> std::cmp::Ordering {
    let distance_order = left
        .result
        .distance_m
        .partial_cmp(&right.result.distance_m)
        .unwrap_or(std::cmp::Ordering::Equal);
    if let Some(order) = point_line_interaction_order(left.family, right.family) {
        return order
            .then_with(|| left.priority.cmp(&right.priority))
            .then_with(|| {
                hover_result_stable_key(&left.result).cmp(&hover_result_stable_key(&right.result))
            });
    }
    if (left.result.distance_m - right.result.distance_m).abs() > HOVER_DISTANCE_TIE_M {
        return distance_order;
    }
    left.z_priority
        .cmp(&right.z_priority)
        .then_with(|| left.priority.cmp(&right.priority))
        .then_with(|| {
            hover_result_stable_key(&left.result).cmp(&hover_result_stable_key(&right.result))
        })
}

fn snap_result_stable_key(result: &SnapResult) -> String {
    format!(
        "{}:{}:{}:{}",
        result
            .feature_id
            .as_ref()
            .map(|id| id.0.as_str())
            .unwrap_or(""),
        result.ring_index.unwrap_or(usize::MAX),
        result.vertex_index.unwrap_or(usize::MAX),
        result.segment_index.unwrap_or(usize::MAX),
    )
}

fn hover_result_stable_key(result: &HoverResult) -> String {
    format!(
        "{}:{}:{}:{}",
        result.feature_id.0,
        result.ring_index.unwrap_or(usize::MAX),
        result.vertex_index.unwrap_or(usize::MAX),
        result.segment_index.unwrap_or(usize::MAX),
    )
}

#[cfg(feature = "enable_dataframes")]
fn split_layer_feature_id(id: &FeatureId) -> Option<(&str, &str)> {
    split_layer_feature_id_any(id)
}

fn copied_split_feature_properties(properties: &Map<String, Value>) -> Map<String, Value> {
    properties
        .iter()
        .filter_map(|(key, value)| {
            let lower = key.to_lowercase();
            let reserved = matches!(
                lower.as_str(),
                "geometry" | "type" | "bbox" | "id" | "doc_id" | "feature_id"
            );
            (!reserved).then_some((key.clone(), value.clone()))
        })
        .collect()
}

fn same_coord(a: [f64; 2], b: [f64; 2]) -> bool {
    a == b
}

/// Collapse consecutive vertices within `tolerance_m` (projected metric).
/// Preserves first + last vertex of an open line so the line's connection
/// points survive a snap-onto-neighbor mutation.
pub fn dedupe_line_coords(
    coords: &mut Vec<[f64; 2]>,
    projection: ProjectionMetadata,
    tolerance_m: f64,
) {
    if !tolerance_m.is_finite() || tolerance_m <= 0.0 || coords.len() < 3 {
        // < 3: open line needs both endpoints; nothing in the middle to drop.
        return;
    }
    // Interior pass — never visits the last index, so the endpoint survives.
    let mut i = 1;
    while i + 1 < coords.len() {
        let prev = projection.project_coord(coords[i - 1]);
        let curr = projection.project_coord(coords[i]);
        if projected_distance(prev, curr) <= tolerance_m {
            coords.remove(i);
        } else {
            i += 1;
        }
    }
    // Trailing-edge pass — drop the penultimate, not the endpoint.
    while coords.len() > 2 {
        let n = coords.len();
        let pen = projection.project_coord(coords[n - 2]);
        let last = projection.project_coord(coords[n - 1]);
        if projected_distance(pen, last) > tolerance_m {
            break;
        }
        coords.remove(n - 2);
    }
}

/// Collapse consecutive vertices within `tolerance_m` on a closed ring.
/// Ring is `[v0, v1, …, vN, v0]` (first == last). Preserves closure and at
/// least 3 unique vertices + the closure copy (4 total).
pub fn dedupe_ring_coords(
    coords: &mut Vec<[f64; 2]>,
    projection: ProjectionMetadata,
    tolerance_m: f64,
) {
    if !tolerance_m.is_finite() || tolerance_m <= 0.0 || coords.len() < 5 {
        return;
    }
    let closed = coords.first() == coords.last();
    if closed {
        coords.pop();
    }
    let min_open = if closed { 3 } else { 2 };
    let mut i = 1;
    while i < coords.len() && coords.len() > min_open {
        let prev = projection.project_coord(coords[i - 1]);
        let curr = projection.project_coord(coords[i]);
        if projected_distance(prev, curr) <= tolerance_m {
            coords.remove(i);
        } else {
            i += 1;
        }
    }
    if closed {
        // Wrap-around: if new-last is within tol of new-first, drop new-last.
        while coords.len() > min_open {
            let first = projection.project_coord(coords[0]);
            let last = projection.project_coord(*coords.last().unwrap());
            if projected_distance(first, last) > tolerance_m {
                break;
            }
            coords.pop();
        }
        let close = coords[0];
        coords.push(close);
    }
}

/// Post-mutation sanitizer. Reuses the network builder's `VERTEX_DEDUPE_TOLERANCE_M`
/// so the edit surface and the build kernel agree on what "same vertex" means.
fn sanitize_geometry_for_drafting(geometry: &mut Geometry, projection: ProjectionMetadata) {
    let tol = VERTEX_DEDUPE_TOLERANCE_M;
    match geometry {
        Geometry::Point { .. } => {}
        Geometry::LineString { coords } => dedupe_line_coords(coords, projection, tol),
        Geometry::Polygon { rings } => {
            for ring in rings.iter_mut() {
                dedupe_ring_coords(ring, projection, tol);
            }
        }
    }
}

fn valid_lonlat(coord: [f64; 2]) -> bool {
    coord[0].is_finite() && coord[1].is_finite()
}

fn insert_into_line(coords: &mut Vec<[f64; 2]>, segment_index: usize, coord: [f64; 2]) -> bool {
    if coords.len() < 2 || segment_index >= coords.len() - 1 {
        return false;
    }
    if same_coord(coords[segment_index], coord) || same_coord(coords[segment_index + 1], coord) {
        return false;
    }
    coords.insert(segment_index + 1, coord);
    true
}

fn move_in_line(coords: &mut [[f64; 2]], vertex_index: usize, coord: [f64; 2]) -> bool {
    let Some(vertex) = coords.get_mut(vertex_index) else {
        return false;
    };
    if *vertex == coord {
        return false;
    }
    *vertex = coord;
    true
}

fn move_in_ring(ring: &mut [[f64; 2]], vertex_index: usize, coord: [f64; 2]) -> bool {
    if ring.len() < 4 || vertex_index >= ring.len() {
        return false;
    }
    let last = ring.len() - 1;
    if vertex_index == 0 || vertex_index == last {
        if ring[0] == coord && ring[last] == coord {
            return false;
        }
        ring[0] = coord;
        ring[last] = coord;
        return true;
    }
    if ring[vertex_index] == coord {
        return false;
    }
    ring[vertex_index] = coord;
    true
}

fn delete_from_line(coords: &mut Vec<[f64; 2]>, vertex_index: usize) -> bool {
    if coords.len() <= 2 || vertex_index >= coords.len() {
        return false;
    }
    coords.remove(vertex_index);
    true
}

fn delete_from_ring(ring: &mut Vec<[f64; 2]>, vertex_index: usize) -> bool {
    if ring.len() <= 4 || vertex_index >= ring.len() {
        return false;
    }
    let last = ring.len() - 1;
    if vertex_index == 0 || vertex_index == last {
        ring.remove(last);
        ring.remove(0);
        if ring.len() < 3 {
            return false;
        }
        ring.push(ring[0]);
        return true;
    }
    ring.remove(vertex_index);
    let last = ring.len() - 1;
    if ring[0] != ring[last] {
        ring[last] = ring[0];
    }
    true
}

fn translate_geometry_projected(
    geometry: &mut Geometry,
    projection: ProjectionMetadata,
    delta: [f64; 2],
) -> bool {
    fn translate_coord(
        coord: &mut [f64; 2],
        projection: ProjectionMetadata,
        delta: [f64; 2],
    ) -> bool {
        let projected = projection.project_coord(*coord);
        let translated = [projected[0] + delta[0], projected[1] + delta[1]];
        let next = projection.unproject_coord(translated);
        if *coord == next {
            return false;
        }
        *coord = next;
        true
    }

    match geometry {
        Geometry::Point { coords } => translate_coord(coords, projection, delta),
        Geometry::LineString { coords } => {
            let mut changed = false;
            for coord in coords {
                changed |= translate_coord(coord, projection, delta);
            }
            changed
        }
        Geometry::Polygon { rings } => {
            let mut changed = false;
            for ring in rings {
                for coord in ring {
                    changed |= translate_coord(coord, projection, delta);
                }
            }
            changed
        }
    }
}

#[derive(Clone, Copy)]
enum ExtendEndpoint {
    Start,
    End,
}

struct ExtendCandidate {
    endpoint: ExtendEndpoint,
    hit_projected: [f64; 2],
    distance_m: f64,
}

fn extend_line_to_target(
    source: &[[f64; 2]],
    target: &[[f64; 2]],
    projection: ProjectionMetadata,
    intent_coord: Option<[f64; 2]>,
) -> Option<Vec<[f64; 2]>> {
    if source.len() < 2 || target.len() < 2 {
        return None;
    }
    let source_projected = source
        .iter()
        .map(|coord| projection.project_coord(*coord))
        .collect::<Vec<_>>();
    let target_projected = target
        .iter()
        .map(|coord| projection.project_coord(*coord))
        .collect::<Vec<_>>();

    let start = extend_candidate_for_endpoint(
        ExtendEndpoint::Start,
        source_projected[1],
        source_projected[0],
        &target_projected,
    );
    let end = extend_candidate_for_endpoint(
        ExtendEndpoint::End,
        source_projected[source_projected.len() - 2],
        source_projected[source_projected.len() - 1],
        &target_projected,
    );

    let chosen = match (start, end) {
        (Some(start), Some(end)) => {
            if let Some(intent) = intent_coord {
                let intent = projection.project_coord(intent);
                let start_d = projected_distance(intent, source_projected[0]);
                let end_d =
                    projected_distance(intent, source_projected[source_projected.len() - 1]);
                if (start_d - end_d).abs() > 1.0 {
                    if start_d < end_d { start } else { end }
                } else if start.distance_m <= end.distance_m {
                    start
                } else {
                    end
                }
            } else if start.distance_m <= end.distance_m {
                start
            } else {
                end
            }
        }
        (Some(start), None) => start,
        (None, Some(end)) => end,
        (None, None) => return None,
    };

    if chosen.distance_m <= 1e-6 {
        return None;
    }
    let hit = projection.unproject_coord(chosen.hit_projected);
    let mut out = Vec::with_capacity(source.len() + 1);
    match chosen.endpoint {
        ExtendEndpoint::Start => {
            out.push(hit);
            out.extend_from_slice(source);
        }
        ExtendEndpoint::End => {
            out.extend_from_slice(source);
            out.push(hit);
        }
    }
    Some(out)
}

fn extend_candidate_for_endpoint(
    endpoint: ExtendEndpoint,
    from: [f64; 2],
    through: [f64; 2],
    target: &[[f64; 2]],
) -> Option<ExtendCandidate> {
    let mut best: Option<ExtendCandidate> = None;
    for segment in target.windows(2) {
        let Some((hit, distance_m)) = ray_segment_hit(
            through,
            [through[0] - from[0], through[1] - from[1]],
            segment[0],
            segment[1],
        ) else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|candidate| distance_m < candidate.distance_m)
        {
            best = Some(ExtendCandidate {
                endpoint,
                hit_projected: hit,
                distance_m,
            });
        }
    }
    best
}

fn ray_segment_hit(
    origin: [f64; 2],
    ray: [f64; 2],
    seg_a: [f64; 2],
    seg_b: [f64; 2],
) -> Option<([f64; 2], f64)> {
    let seg = [seg_b[0] - seg_a[0], seg_b[1] - seg_a[1]];
    let denom = cross2(ray, seg);
    if denom.abs() <= 1e-9 {
        return None;
    }
    let rel = [seg_a[0] - origin[0], seg_a[1] - origin[1]];
    let t = cross2(rel, seg) / denom;
    let u = cross2(rel, ray) / denom;
    let eps = 1e-9;
    if t < -eps || u < -eps || u > 1.0 + eps {
        return None;
    }
    let hit = [origin[0] + t * ray[0], origin[1] + t * ray[1]];
    Some((hit, projected_distance(origin, hit)))
}

fn cross2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

#[derive(Clone, Copy)]
enum JoinEndpoint {
    StartStart,
    StartEnd,
    EndStart,
    EndEnd,
}

struct JoinCandidate {
    endpoint: JoinEndpoint,
    fused_coord: [f64; 2],
    distance_m: f64,
}

fn join_linestrings(
    source: &[[f64; 2]],
    target: &[[f64; 2]],
    projection: ProjectionMetadata,
    tolerance_m: f64,
) -> Option<Vec<[f64; 2]>> {
    if source.len() < 2 || target.len() < 2 {
        return None;
    }
    let source_start = projection.project_coord(source[0]);
    let source_end = projection.project_coord(*source.last()?);
    let target_start = projection.project_coord(target[0]);
    let target_end = projection.project_coord(*target.last()?);
    let candidates = [
        (JoinEndpoint::StartStart, source_start, target_start),
        (JoinEndpoint::StartEnd, source_start, target_end),
        (JoinEndpoint::EndStart, source_end, target_start),
        (JoinEndpoint::EndEnd, source_end, target_end),
    ];
    let candidate = candidates
        .into_iter()
        .filter_map(|(endpoint, a, b)| {
            let distance_m = projected_distance(a, b);
            (distance_m <= tolerance_m).then_some(JoinCandidate {
                endpoint,
                fused_coord: projection.unproject_coord([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]),
                distance_m,
            })
        })
        .min_by(|a, b| {
            a.distance_m
                .partial_cmp(&b.distance_m)
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;

    let fused = candidate.fused_coord;
    let mut out = Vec::with_capacity(source.len() + target.len() - 1);
    match candidate.endpoint {
        JoinEndpoint::StartStart => {
            out.extend(source.iter().rev().copied());
            if let Some(last) = out.last_mut() {
                *last = fused;
            }
            out.extend_from_slice(&target[1..]);
        }
        JoinEndpoint::StartEnd => {
            out.extend_from_slice(&target[..target.len() - 1]);
            out.push(fused);
            out.extend_from_slice(&source[1..]);
        }
        JoinEndpoint::EndStart => {
            out.extend_from_slice(&source[..source.len() - 1]);
            out.push(fused);
            out.extend_from_slice(&target[1..]);
        }
        JoinEndpoint::EndEnd => {
            out.extend_from_slice(&source[..source.len() - 1]);
            out.push(fused);
            out.extend(target[..target.len() - 1].iter().rev().copied());
        }
    }
    (out.len() >= 2).then_some(out)
}

fn projected_pick_envelope(point: [f64; 2], tolerance_m: f64) -> AABB<[f64; 2]> {
    AABB::from_corners(
        [point[0] - tolerance_m, point[1] - tolerance_m],
        [point[0] + tolerance_m, point[1] + tolerance_m],
    )
}

/// Lon/lat bbox of a [`Geometry`] as `[min_lon, min_lat, max_lon, max_lat]`.
/// Feeds [`SessionHandle::selection_resolution`]'s autozoom payload
/// without a round-trip through the projector.
pub(crate) fn geometry_lonlat_bbox(geom: &Geometry) -> [f64; 4] {
    let mut min_lon = f64::INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lon = f64::NEG_INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    let mut visit = |coords: &[f64; 2]| {
        let [lon, lat] = *coords;
        if lon.is_finite() && lat.is_finite() {
            if lon < min_lon {
                min_lon = lon;
            }
            if lat < min_lat {
                min_lat = lat;
            }
            if lon > max_lon {
                max_lon = lon;
            }
            if lat > max_lat {
                max_lat = lat;
            }
        }
    };
    match geom {
        Geometry::Point { coords } => visit(coords),
        Geometry::LineString { coords } => coords.iter().for_each(&mut visit),
        Geometry::Polygon { rings } => {
            for ring in rings {
                ring.iter().for_each(&mut visit);
            }
        }
    }
    if !min_lon.is_finite() {
        return [0.0, 0.0, 0.0, 0.0];
    }
    [min_lon, min_lat, max_lon, max_lat]
}

fn projected_geometry_bbox(geom: &ProjectedGeometry) -> AABB<[f64; 2]> {
    match geom {
        ProjectedGeometry::Point { coords } => AABB::from_point(*coords),
        ProjectedGeometry::LineString { coords } => projected_coords_bbox(coords.iter().copied()),
        ProjectedGeometry::Polygon { rings } => {
            projected_coords_bbox(rings.iter().flat_map(|r| r.iter().copied()))
        }
    }
}

fn projected_coords_bbox<I: IntoIterator<Item = [f64; 2]>>(iter: I) -> AABB<[f64; 2]> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut any = false;
    for [x, y] in iter {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        any = true;
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    if !any {
        return AABB::from_point([0.0, 0.0]);
    }
    AABB::from_corners([min_x, min_y], [max_x, max_y])
}

fn projected_distance_m(p: [f64; 2], geom: &ProjectedGeometry) -> f64 {
    match geom {
        ProjectedGeometry::Point { coords } => projected_distance(p, *coords),
        ProjectedGeometry::LineString { coords } => projected_distance_to_polyline_m(p, coords),
        ProjectedGeometry::Polygon { rings } => projected_distance_to_polygon_m(p, rings),
    }
}

fn projected_distance_to_polyline_m(p: [f64; 2], coords: &[[f64; 2]]) -> f64 {
    if coords.is_empty() {
        return f64::INFINITY;
    }
    if coords.len() == 1 {
        return projected_distance(p, coords[0]);
    }
    let mut best = f64::INFINITY;
    for window in coords.windows(2) {
        let d = projected_distance_to_segment_m(p, window[0], window[1]);
        if d < best {
            best = d;
        }
    }
    best
}

/// Polygon distance: `0` if the click is strictly inside the outer ring
/// and not in any hole; minimum-edge distance otherwise (or distance to
/// the hole boundary when the click landed in a hole). The `0`-distance
/// answer is what makes a click on the polygon's interior beat any
/// nearby point/line in `pick`'s tie-breaker.
fn projected_distance_to_polygon_m(p: [f64; 2], rings: &[Vec<[f64; 2]>]) -> f64 {
    let Some(outer) = rings.first() else {
        return f64::INFINITY;
    };
    if outer.is_empty() {
        return f64::INFINITY;
    }
    if !point_in_ring(p, outer) {
        // Outside the polygon entirely: distance to the outer boundary
        // is the answer.
        return projected_distance_to_ring_m(p, outer);
    }
    // Inside the outer ring. If the click landed in a hole, the nearest
    // boundary is that hole's edge; the polygon "surface" doesn't contain
    // the click.
    for hole in &rings[1..] {
        if point_in_ring(p, hole) {
            return projected_distance_to_ring_m(p, hole);
        }
    }
    0.0
}

fn projected_distance_to_ring_m(p: [f64; 2], ring: &[[f64; 2]]) -> f64 {
    if ring.len() < 2 {
        return f64::INFINITY;
    }
    let mut best = f64::INFINITY;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        let d = projected_distance_to_segment_m(p, a, b);
        if d < best {
            best = d;
        }
    }
    best
}

fn projected_distance_to_segment_m(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    projected_point_segment_distance(p, a, b)
}

fn projected_geometry_intersects_polygon(
    geometry: &ProjectedGeometry,
    selector: &[Vec<[f64; 2]>],
) -> bool {
    match geometry {
        ProjectedGeometry::Point { coords } => point_in_polygon(*coords, selector),
        ProjectedGeometry::LineString { coords } => line_intersects_polygon(coords, selector),
        ProjectedGeometry::Polygon { rings } => polygon_intersects_polygon(rings, selector),
    }
}

fn projected_geometry_within_polygon(
    geometry: &ProjectedGeometry,
    selector: &[Vec<[f64; 2]>],
) -> bool {
    match geometry {
        ProjectedGeometry::Point { coords } => point_in_polygon(*coords, selector),
        ProjectedGeometry::LineString { coords } => line_within_polygon(coords, selector),
        ProjectedGeometry::Polygon { rings } => polygon_within_polygon(rings, selector),
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
