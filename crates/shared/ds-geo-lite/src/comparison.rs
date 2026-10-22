//! Deterministic, domain-neutral comparison of named GeoJSON layer maps.
//!
//! Identity matching is exact and alias-aware.  Geometry and property
//! comparison are also exact; callers may exclude transport/provenance
//! property keys, but this kernel does not know any domain-specific fields.

use crate::identity::geojson_feature_identity_candidates;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const DEFAULT_MAX_DETAIL_FEATURES: usize = 500;

fn default_max_detail_features() -> usize {
    DEFAULT_MAX_DETAIL_FEATURES
}

/// Public comparison input.  The camel-case wire names match the browser
/// package; snake-case aliases keep the documented JSON envelope usable from
/// non-TypeScript callers.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub struct GeoJsonLayerComparisonInput {
    #[serde(default, alias = "local_layers")]
    pub local_layers: BTreeMap<String, Value>,
    #[serde(default, alias = "cloud_layers")]
    pub cloud_layers: BTreeMap<String, Value>,
    /// Empty means the sorted union of local and cloud layer names.
    #[serde(default, alias = "layer_names")]
    pub layer_names: Vec<String>,
    #[serde(default, alias = "ignored_property_keys")]
    pub ignored_property_keys: Vec<String>,
    #[serde(default = "default_max_detail_features", alias = "max_detail_features")]
    pub max_detail_features: usize,
    /// Restricts only bounded details/review collections. Exact counts and
    /// change bounds always describe every divergence in the selected layers.
    #[serde(default, alias = "detail_change_classes")]
    pub detail_change_classes: Vec<ComparisonChangeClass>,
}

/// A class which may receive one bounded detail record.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub enum ComparisonChangeClass {
    LocalOnly,
    CloudOnly,
    AttributeOnly,
    GeometryOnly,
    AttributeAndGeometry,
    Ambiguous,
    Unmatchable,
}

impl ComparisonChangeClass {
    const ALL: [Self; 7] = [
        Self::LocalOnly,
        Self::CloudOnly,
        Self::AttributeOnly,
        Self::GeometryOnly,
        Self::AttributeAndGeometry,
        Self::Ambiguous,
        Self::Unmatchable,
    ];
}

/// Exact comparison totals.  `matched` counts one local/cloud pair.  The
/// ambiguous/unmatchable fields count feature records (both sides), so each
/// side reconciles independently:
///
/// `localFeatureCount = matched + localOnly + localAmbiguousUnmatchable`
///
/// and equivalently for cloud.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub struct ComparisonCounts {
    pub local_feature_count: usize,
    pub cloud_feature_count: usize,
    pub matched: usize,
    pub local_only: usize,
    pub cloud_only: usize,
    pub unchanged: usize,
    pub attribute_only_changed: usize,
    pub geometry_only_changed: usize,
    pub attribute_and_geometry_changed: usize,
    pub ambiguous_unmatchable: usize,
    pub local_ambiguous_unmatchable: usize,
    pub cloud_ambiguous_unmatchable: usize,
    pub ambiguous_identity_features: usize,
    pub unmatchable_features: usize,
}

impl ComparisonCounts {
    fn add_assign(&mut self, other: &Self) {
        self.local_feature_count += other.local_feature_count;
        self.cloud_feature_count += other.cloud_feature_count;
        self.matched += other.matched;
        self.local_only += other.local_only;
        self.cloud_only += other.cloud_only;
        self.unchanged += other.unchanged;
        self.attribute_only_changed += other.attribute_only_changed;
        self.geometry_only_changed += other.geometry_only_changed;
        self.attribute_and_geometry_changed += other.attribute_and_geometry_changed;
        self.ambiguous_unmatchable += other.ambiguous_unmatchable;
        self.local_ambiguous_unmatchable += other.local_ambiguous_unmatchable;
        self.cloud_ambiguous_unmatchable += other.cloud_ambiguous_unmatchable;
        self.ambiguous_identity_features += other.ambiguous_identity_features;
        self.unmatchable_features += other.unmatchable_features;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub enum IdentityIssueKind {
    AmbiguousIdentity,
    MissingIdentity,
}

/// Explicit evidence for a feature/component which could not be matched.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub struct IdentityIssue {
    pub kind: IdentityIssueKind,
    pub aliases: Vec<String>,
    pub local_feature_indexes: Vec<usize>,
    pub cloud_feature_indexes: Vec<usize>,
    pub component_local_feature_count: usize,
    pub component_cloud_feature_count: usize,
    pub component_alias_count: usize,
}

/// One bounded comparison detail.  Index arrays contain source
/// FeatureCollection indexes. Ambiguous alias components can contain more
/// than one feature on either side; no row-order pairing is attempted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub struct FeatureComparisonDetail {
    pub layer_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    pub identity_aliases: Vec<String>,
    pub change_class: ComparisonChangeClass,
    pub local_feature_indexes: Vec<usize>,
    pub cloud_feature_indexes: Vec<usize>,
    pub changed_property_keys: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_geometry_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_geometry_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_issue: Option<IdentityIssue>,
}

/// GeoJSON review collection. Features are copies of selected input features
/// annotated only in the review copy with reserved `__ds_comparison_*`
/// properties so MapLibre can style/filter them without joining another
/// payload. Source features are never mutated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub struct GeoJsonFeatureCollection {
    #[serde(rename = "type")]
    pub geojson_type: String,
    pub features: Vec<Value>,
}

impl Default for GeoJsonFeatureCollection {
    fn default() -> Self {
        Self {
            geojson_type: "FeatureCollection".to_string(),
            features: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub struct ComparisonReviewCollections {
    pub local_only: GeoJsonFeatureCollection,
    pub cloud_only: GeoJsonFeatureCollection,
    pub changed_local: GeoJsonFeatureCollection,
    pub changed_cloud: GeoJsonFeatureCollection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub struct LayerComparisonResult {
    pub layer_name: String,
    pub counts: ComparisonCounts,
    pub changed_attribute_key_counts: BTreeMap<String, usize>,
    pub change_bounds: Option<[f64; 4]>,
    pub total_detail_count: usize,
    pub returned_detail_count: usize,
    pub details_truncated: bool,
    pub details: Vec<FeatureComparisonDetail>,
    pub review_collections: ComparisonReviewCollections,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub struct GeoJsonLayerComparisonResult {
    pub totals: ComparisonCounts,
    pub layers: Vec<LayerComparisonResult>,
    pub changed_attribute_key_counts: BTreeMap<String, usize>,
    pub ignored_property_keys: Vec<String>,
    pub max_detail_features: usize,
    pub detail_change_classes: Vec<ComparisonChangeClass>,
    pub change_bounds: Option<[f64; 4]>,
    pub total_detail_count: usize,
    pub returned_detail_count: usize,
    pub details_truncated: bool,
}

/// One refusal owner for native and serialized comparison callers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComparisonError {
    InvalidRequest(String),
    InvalidResponse(String),
}
impl std::fmt::Display for ComparisonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "comparison_request_invalid: {message}"),
            Self::InvalidResponse(message) => write!(f, "comparison_response_invalid: {message}"),
        }
    }
}
impl std::error::Error for ComparisonError {}

pub fn compare_geojson_layers(
    input: GeoJsonLayerComparisonInput,
) -> Result<GeoJsonLayerComparisonResult, ComparisonError> {
    compare_geojson_layers_inner(input).map_err(ComparisonError::InvalidRequest)
}

/// Bind a decoded reply to the actual input scope. This validates interpretation
/// without running comparison again, checking releases, or mutating either side.
pub fn validate_comparison_response(
    input: &GeoJsonLayerComparisonInput,
    result: GeoJsonLayerComparisonResult,
) -> Result<GeoJsonLayerComparisonResult, ComparisonError> {
    let invalid = |message: &str| ComparisonError::InvalidResponse(message.into());
    let layers = selected_layer_names(
        input.layer_names.clone(),
        &input.local_layers,
        &input.cloud_layers,
    )
    .map_err(ComparisonError::InvalidRequest)?;
    if result
        .layers
        .iter()
        .map(|layer| &layer.layer_name)
        .ne(layers.iter())
    {
        return Err(invalid("reply layers do not match the requested scope"));
    }
    fn reconciles(counts: &ComparisonCounts) -> bool {
        let n = |value: usize| value as u128;
        n(counts.local_feature_count)
            == n(counts.matched) + n(counts.local_only) + n(counts.local_ambiguous_unmatchable)
            && n(counts.cloud_feature_count)
                == n(counts.matched) + n(counts.cloud_only) + n(counts.cloud_ambiguous_unmatchable)
            && n(counts.matched)
                == n(counts.unchanged)
                    + n(counts.attribute_only_changed)
                    + n(counts.geometry_only_changed)
                    + n(counts.attribute_and_geometry_changed)
            && n(counts.ambiguous_unmatchable)
                == n(counts.local_ambiguous_unmatchable) + n(counts.cloud_ambiguous_unmatchable)
            && n(counts.ambiguous_unmatchable)
                == n(counts.ambiguous_identity_features) + n(counts.unmatchable_features)
    }
    fn bounds_valid(bounds: Option<[f64; 4]>) -> bool {
        bounds.is_none_or(|b| b.iter().all(|n| n.is_finite()) && b[0] <= b[2] && b[1] <= b[3])
    }
    let mut totals = ComparisonCounts::default();
    let mut returned = 0usize;
    let mut total = 0usize;
    for layer in &result.layers {
        let local = layer_features(
            &layer.layer_name,
            "local",
            input.local_layers.get(&layer.layer_name),
        )
        .map_err(ComparisonError::InvalidRequest)?
        .len();
        let cloud = layer_features(
            &layer.layer_name,
            "cloud",
            input.cloud_layers.get(&layer.layer_name),
        )
        .map_err(ComparisonError::InvalidRequest)?
        .len();
        if layer.counts.local_feature_count != local
            || layer.counts.cloud_feature_count != cloud
            || !reconciles(&layer.counts)
        {
            return Err(invalid(
                "reply counts do not reconcile with the input features",
            ));
        }
        if layer.returned_detail_count != layer.details.len()
            || layer.returned_detail_count > layer.total_detail_count
            || layer.details_truncated != (layer.returned_detail_count < layer.total_detail_count)
            || !bounds_valid(layer.change_bounds)
        {
            return Err(invalid("reply details or bounds are inconsistent"));
        }
        for collection in [
            &layer.review_collections.local_only,
            &layer.review_collections.cloud_only,
            &layer.review_collections.changed_local,
            &layer.review_collections.changed_cloud,
        ] {
            if collection.geojson_type != "FeatureCollection"
                || collection.features.iter().any(|feature| {
                    feature.get("type").and_then(Value::as_str) != Some("Feature")
                        || feature
                            .get("properties")
                            .is_some_and(|props| !props.is_null() && !props.is_object())
                })
            {
                return Err(invalid(
                    "reply review collection is not a GeoJSON FeatureCollection",
                ));
            }
        }
        for detail in &layer.details {
            if detail.layer_name != layer.layer_name
                || detail
                    .local_feature_indexes
                    .iter()
                    .any(|index| *index >= local)
                || detail
                    .cloud_feature_indexes
                    .iter()
                    .any(|index| *index >= cloud)
            {
                return Err(invalid(
                    "reply detail addresses a different layer or an unavailable feature",
                ));
            }
        }
        totals.add_assign(&layer.counts);
        returned = returned
            .checked_add(layer.returned_detail_count)
            .ok_or_else(|| invalid("reply detail count overflow"))?;
        total = total
            .checked_add(layer.total_detail_count)
            .ok_or_else(|| invalid("reply detail count overflow"))?;
    }
    if totals != result.totals
        || returned != result.returned_detail_count
        || total != result.total_detail_count
        || returned > input.max_detail_features
        || result.max_detail_features != input.max_detail_features
        || result.details_truncated != (returned < total)
        || !bounds_valid(result.change_bounds)
    {
        return Err(invalid(
            "reply totals, detail budget or bounds do not match the request",
        ));
    }
    let ignored = input
        .ignored_property_keys
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let classes = if input.detail_change_classes.is_empty() {
        ComparisonChangeClass::ALL
            .into_iter()
            .collect::<BTreeSet<_>>()
    } else {
        input.detail_change_classes.iter().copied().collect()
    };
    if result.ignored_property_keys.iter().ne(ignored.iter())
        || result.detail_change_classes.iter().ne(classes.iter())
    {
        return Err(invalid("reply options do not match the request"));
    }
    Ok(result)
}

/// Compare local and cloud named GeoJSON FeatureCollections.
fn compare_geojson_layers_inner(
    input: GeoJsonLayerComparisonInput,
) -> Result<GeoJsonLayerComparisonResult, String> {
    let ignored_property_keys = input
        .ignored_property_keys
        .into_iter()
        .collect::<BTreeSet<_>>();
    let detail_change_classes = if input.detail_change_classes.is_empty() {
        ComparisonChangeClass::ALL
            .into_iter()
            .collect::<BTreeSet<_>>()
    } else {
        input
            .detail_change_classes
            .into_iter()
            .collect::<BTreeSet<_>>()
    };
    let layer_names =
        selected_layer_names(input.layer_names, &input.local_layers, &input.cloud_layers)?;

    let mut totals = ComparisonCounts::default();
    let mut overall_key_counts = BTreeMap::new();
    let mut overall_bounds = BoundsAccumulator::default();
    let mut layers = Vec::with_capacity(layer_names.len());
    let mut remaining_details = input.max_detail_features;
    let mut total_detail_count = 0usize;
    let mut returned_detail_count = 0usize;

    for layer_name in layer_names {
        let layer_result = compare_layer(
            &layer_name,
            input.local_layers.get(&layer_name),
            input.cloud_layers.get(&layer_name),
            &ignored_property_keys,
            &detail_change_classes,
            remaining_details,
        )?;
        remaining_details = remaining_details.saturating_sub(layer_result.returned_detail_count);
        total_detail_count += layer_result.total_detail_count;
        returned_detail_count += layer_result.returned_detail_count;
        totals.add_assign(&layer_result.counts);
        for (key, count) in &layer_result.changed_attribute_key_counts {
            *overall_key_counts.entry(key.clone()).or_insert(0) += count;
        }
        if let Some(bounds) = layer_result.change_bounds {
            overall_bounds.include_bounds(bounds);
        }
        layers.push(layer_result);
    }

    Ok(GeoJsonLayerComparisonResult {
        totals,
        layers,
        changed_attribute_key_counts: overall_key_counts,
        ignored_property_keys: ignored_property_keys.into_iter().collect(),
        max_detail_features: input.max_detail_features,
        detail_change_classes: detail_change_classes.into_iter().collect(),
        change_bounds: overall_bounds.finish(),
        total_detail_count,
        returned_detail_count,
        details_truncated: returned_detail_count < total_detail_count,
    })
}

fn selected_layer_names(
    requested: Vec<String>,
    local: &BTreeMap<String, Value>,
    cloud: &BTreeMap<String, Value>,
) -> Result<Vec<String>, String> {
    let mut names = BTreeSet::new();
    if requested.is_empty() {
        names.extend(local.keys().cloned());
        names.extend(cloud.keys().cloned());
    } else {
        for name in requested {
            if name.is_empty() {
                return Err("layerNames must not contain an empty name".to_string());
            }
            names.insert(name);
        }
    }
    Ok(names.into_iter().collect())
}

struct LayerFeature<'a> {
    value: &'a Value,
    aliases: Vec<String>,
}

fn layer_features<'a>(
    layer_name: &str,
    side: &str,
    layer: Option<&'a Value>,
) -> Result<Vec<LayerFeature<'a>>, String> {
    let Some(layer) = layer else {
        return Ok(Vec::new());
    };
    if layer.get("type").and_then(Value::as_str) != Some("FeatureCollection") {
        return Err(format!(
            "{side} layer {layer_name}: expected a GeoJSON FeatureCollection"
        ));
    }
    let features = layer
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{side} layer {layer_name}: features must be an array"))?;
    features
        .iter()
        .enumerate()
        .map(|(index, feature)| {
            if feature.get("type").and_then(Value::as_str) != Some("Feature") {
                return Err(format!(
                    "{side} layer {layer_name}, feature {index}: expected a GeoJSON Feature"
                ));
            }
            if let Some(properties) = feature.get("properties")
                && !properties.is_null()
                && !properties.is_object()
            {
                return Err(format!(
                    "{side} layer {layer_name}, feature {index}: properties must be an object or null"
                ));
            }
            Ok(LayerFeature {
                value: feature,
                aliases: geojson_feature_identity_candidates(feature),
            })
        })
        .collect()
}

#[derive(Default)]
struct IdentityComponent {
    local_positions: Vec<usize>,
    cloud_positions: Vec<usize>,
    aliases: BTreeSet<String>,
}

struct DetailCandidate {
    detail: FeatureComparisonDetail,
    local_positions: Vec<usize>,
    cloud_positions: Vec<usize>,
}

fn compare_layer(
    layer_name: &str,
    local_layer: Option<&Value>,
    cloud_layer: Option<&Value>,
    ignored_property_keys: &BTreeSet<String>,
    detail_change_classes: &BTreeSet<ComparisonChangeClass>,
    max_returned_details: usize,
) -> Result<LayerComparisonResult, String> {
    let local = layer_features(layer_name, "local", local_layer)?;
    let cloud = layer_features(layer_name, "cloud", cloud_layer)?;
    let mut counts = ComparisonCounts {
        local_feature_count: local.len(),
        cloud_feature_count: cloud.len(),
        ..ComparisonCounts::default()
    };
    let mut changed_key_counts = BTreeMap::new();
    let mut change_bounds = BoundsAccumulator::default();

    let total_nodes = local.len() + cloud.len();
    let mut union_find = UnionFind::new(total_nodes);
    let mut alias_owner = BTreeMap::<String, usize>::new();
    for (node, aliases) in local
        .iter()
        .map(|feature| &feature.aliases)
        .chain(cloud.iter().map(|feature| &feature.aliases))
        .enumerate()
    {
        for alias in aliases {
            if let Some(previous) = alias_owner.get(alias).copied() {
                union_find.union(previous, node);
            } else {
                alias_owner.insert(alias.clone(), node);
            }
        }
    }

    let mut components = BTreeMap::<usize, IdentityComponent>::new();
    for (position, feature) in local.iter().enumerate() {
        let root = union_find.find(position);
        let component = components.entry(root).or_default();
        component.local_positions.push(position);
        component.aliases.extend(feature.aliases.iter().cloned());
    }
    for (position, feature) in cloud.iter().enumerate() {
        let node = local.len() + position;
        let root = union_find.find(node);
        let component = components.entry(root).or_default();
        component.cloud_positions.push(position);
        component.aliases.extend(feature.aliases.iter().cloned());
    }

    let mut candidates = Vec::new();
    for component in components.into_values() {
        let aliases = component.aliases.into_iter().collect::<Vec<_>>();
        let local_positions = component.local_positions;
        let cloud_positions = component.cloud_positions;
        match (
            local_positions.len(),
            cloud_positions.len(),
            aliases.is_empty(),
        ) {
            (1, 0, false) => {
                counts.local_only += 1;
                include_positions_bounds(&mut change_bounds, &local, &local_positions);
                candidates.push(side_only_candidate(
                    layer_name,
                    ComparisonChangeClass::LocalOnly,
                    &local,
                    local_positions,
                    aliases,
                ));
            }
            (0, 1, false) => {
                counts.cloud_only += 1;
                include_positions_bounds(&mut change_bounds, &cloud, &cloud_positions);
                candidates.push(side_only_candidate(
                    layer_name,
                    ComparisonChangeClass::CloudOnly,
                    &cloud,
                    cloud_positions,
                    aliases,
                ));
            }
            (1, 1, false) => {
                counts.matched += 1;
                let local_position = local_positions[0];
                let cloud_position = cloud_positions[0];
                let local_feature = local[local_position].value;
                let cloud_feature = cloud[cloud_position].value;
                let changed_property_keys =
                    changed_property_keys(local_feature, cloud_feature, ignored_property_keys);
                let geometry_changed =
                    local_feature.get("geometry") != cloud_feature.get("geometry");
                let change_class = match (changed_property_keys.is_empty(), geometry_changed) {
                    (true, false) => {
                        counts.unchanged += 1;
                        None
                    }
                    (false, false) => {
                        counts.attribute_only_changed += 1;
                        Some(ComparisonChangeClass::AttributeOnly)
                    }
                    (true, true) => {
                        counts.geometry_only_changed += 1;
                        Some(ComparisonChangeClass::GeometryOnly)
                    }
                    (false, true) => {
                        counts.attribute_and_geometry_changed += 1;
                        Some(ComparisonChangeClass::AttributeAndGeometry)
                    }
                };
                let Some(change_class) = change_class else {
                    continue;
                };
                for key in &changed_property_keys {
                    *changed_key_counts.entry(key.clone()).or_insert(0) += 1;
                }
                include_positions_bounds(&mut change_bounds, &local, &local_positions);
                include_positions_bounds(&mut change_bounds, &cloud, &cloud_positions);
                let shared_identity = local[local_position]
                    .aliases
                    .iter()
                    .filter(|alias| cloud[cloud_position].aliases.contains(alias))
                    .min()
                    .cloned();
                candidates.push(DetailCandidate {
                    detail: FeatureComparisonDetail {
                        layer_name: layer_name.to_string(),
                        identity: shared_identity,
                        identity_aliases: aliases,
                        change_class,
                        local_feature_indexes: local_positions.clone(),
                        cloud_feature_indexes: cloud_positions.clone(),
                        changed_property_keys,
                        local_geometry_type: geometry_type(local_feature),
                        cloud_geometry_type: geometry_type(cloud_feature),
                        identity_issue: None,
                    },
                    local_positions,
                    cloud_positions,
                });
            }
            (_, _, true) => {
                // Missing identities never join a component, so this branch is
                // one feature on exactly one side. Keep it explicit rather
                // than pretending it is a side-only identity match result.
                let local_count = local_positions.len();
                let cloud_count = cloud_positions.len();
                counts.unmatchable_features += local_count + cloud_count;
                counts.ambiguous_unmatchable += local_count + cloud_count;
                counts.local_ambiguous_unmatchable += local_count;
                counts.cloud_ambiguous_unmatchable += cloud_count;
                include_positions_bounds(&mut change_bounds, &local, &local_positions);
                include_positions_bounds(&mut change_bounds, &cloud, &cloud_positions);
                let issue = IdentityIssue {
                    kind: IdentityIssueKind::MissingIdentity,
                    aliases: Vec::new(),
                    local_feature_indexes: local_positions.clone(),
                    cloud_feature_indexes: cloud_positions.clone(),
                    component_local_feature_count: local_count,
                    component_cloud_feature_count: cloud_count,
                    component_alias_count: 0,
                };
                candidates.push(DetailCandidate {
                    detail: FeatureComparisonDetail {
                        layer_name: layer_name.to_string(),
                        identity: None,
                        identity_aliases: Vec::new(),
                        change_class: ComparisonChangeClass::Unmatchable,
                        local_feature_indexes: local_positions.clone(),
                        cloud_feature_indexes: cloud_positions.clone(),
                        changed_property_keys: Vec::new(),
                        local_geometry_type: component_geometry_type(&local, &local_positions),
                        cloud_geometry_type: component_geometry_type(&cloud, &cloud_positions),
                        identity_issue: Some(issue),
                    },
                    local_positions,
                    cloud_positions,
                });
            }
            _ => {
                let local_count = local_positions.len();
                let cloud_count = cloud_positions.len();
                counts.ambiguous_identity_features += local_count + cloud_count;
                counts.ambiguous_unmatchable += local_count + cloud_count;
                counts.local_ambiguous_unmatchable += local_count;
                counts.cloud_ambiguous_unmatchable += cloud_count;
                include_positions_bounds(&mut change_bounds, &local, &local_positions);
                include_positions_bounds(&mut change_bounds, &cloud, &cloud_positions);
                let component_alias_count = aliases.len();
                for position in local_positions {
                    candidates.push(ambiguous_candidate(
                        layer_name,
                        &local,
                        position,
                        true,
                        local_count,
                        cloud_count,
                        component_alias_count,
                    ));
                }
                for position in cloud_positions {
                    candidates.push(ambiguous_candidate(
                        layer_name,
                        &cloud,
                        position,
                        false,
                        local_count,
                        cloud_count,
                        component_alias_count,
                    ));
                }
            }
        }
    }

    candidates.sort_by(|left, right| {
        left.detail
            .identity
            .is_none()
            .cmp(&right.detail.identity.is_none())
            .then_with(|| left.detail.identity.cmp(&right.detail.identity))
            .then_with(|| left.detail.change_class.cmp(&right.detail.change_class))
            .then_with(|| {
                left.detail
                    .local_feature_indexes
                    .cmp(&right.detail.local_feature_indexes)
            })
            .then_with(|| {
                left.detail
                    .cloud_feature_indexes
                    .cmp(&right.detail.cloud_feature_indexes)
            })
    });
    let mut review_collections = ComparisonReviewCollections::default();
    let mut details = Vec::new();
    let total_detail_count = candidates
        .iter()
        .filter(|candidate| detail_change_classes.contains(&candidate.detail.change_class))
        .count();
    for candidate in candidates
        .into_iter()
        .filter(|candidate| detail_change_classes.contains(&candidate.detail.change_class))
        .take(max_returned_details)
    {
        add_review_features(&candidate, &local, &cloud, &mut review_collections);
        details.push(candidate.detail);
    }
    let returned_detail_count = details.len();

    Ok(LayerComparisonResult {
        layer_name: layer_name.to_string(),
        counts,
        changed_attribute_key_counts: changed_key_counts,
        change_bounds: change_bounds.finish(),
        total_detail_count,
        returned_detail_count,
        details_truncated: returned_detail_count < total_detail_count,
        details,
        review_collections,
    })
}

fn ambiguous_candidate(
    layer_name: &str,
    features: &[LayerFeature<'_>],
    position: usize,
    is_local: bool,
    component_local_feature_count: usize,
    component_cloud_feature_count: usize,
    component_alias_count: usize,
) -> DetailCandidate {
    let mut aliases = features[position].aliases.clone();
    let identity = aliases.first().cloned();
    aliases.sort();
    let (local_positions, cloud_positions) = if is_local {
        (vec![position], Vec::new())
    } else {
        (Vec::new(), vec![position])
    };
    let issue = IdentityIssue {
        kind: IdentityIssueKind::AmbiguousIdentity,
        aliases: aliases.clone(),
        local_feature_indexes: local_positions.clone(),
        cloud_feature_indexes: cloud_positions.clone(),
        component_local_feature_count,
        component_cloud_feature_count,
        component_alias_count,
    };
    DetailCandidate {
        detail: FeatureComparisonDetail {
            layer_name: layer_name.to_string(),
            identity,
            identity_aliases: aliases,
            change_class: ComparisonChangeClass::Ambiguous,
            local_feature_indexes: local_positions.clone(),
            cloud_feature_indexes: cloud_positions.clone(),
            changed_property_keys: Vec::new(),
            local_geometry_type: is_local
                .then(|| geometry_type(features[position].value))
                .flatten(),
            cloud_geometry_type: (!is_local)
                .then(|| geometry_type(features[position].value))
                .flatten(),
            identity_issue: Some(issue),
        },
        local_positions,
        cloud_positions,
    }
}

fn side_only_candidate(
    layer_name: &str,
    change_class: ComparisonChangeClass,
    features: &[LayerFeature<'_>],
    positions: Vec<usize>,
    aliases: Vec<String>,
) -> DetailCandidate {
    let is_local = change_class == ComparisonChangeClass::LocalOnly;
    let identity = positions
        .first()
        .and_then(|position| features[*position].aliases.first())
        .cloned();
    let geometry_type = component_geometry_type(features, &positions);
    let (local_positions, cloud_positions) = if is_local {
        (positions, Vec::new())
    } else {
        (Vec::new(), positions)
    };
    DetailCandidate {
        detail: FeatureComparisonDetail {
            layer_name: layer_name.to_string(),
            identity,
            identity_aliases: aliases,
            change_class,
            local_feature_indexes: local_positions.clone(),
            cloud_feature_indexes: cloud_positions.clone(),
            changed_property_keys: Vec::new(),
            local_geometry_type: is_local.then_some(geometry_type.clone()).flatten(),
            cloud_geometry_type: (!is_local).then_some(geometry_type).flatten(),
            identity_issue: None,
        },
        local_positions,
        cloud_positions,
    }
}

fn add_review_features(
    candidate: &DetailCandidate,
    local: &[LayerFeature<'_>],
    cloud: &[LayerFeature<'_>],
    review: &mut ComparisonReviewCollections,
) {
    let append = |target: &mut GeoJsonFeatureCollection,
                  source: &[LayerFeature<'_>],
                  positions: &[usize],
                  side: &str| {
        target.features.extend(
            positions
                .iter()
                .map(|position| review_feature(source[*position].value, &candidate.detail, side)),
        );
    };
    match candidate.detail.change_class {
        ComparisonChangeClass::LocalOnly => append(
            &mut review.local_only,
            local,
            &candidate.local_positions,
            "local",
        ),
        ComparisonChangeClass::CloudOnly => append(
            &mut review.cloud_only,
            cloud,
            &candidate.cloud_positions,
            "cloud",
        ),
        ComparisonChangeClass::AttributeOnly
        | ComparisonChangeClass::GeometryOnly
        | ComparisonChangeClass::AttributeAndGeometry => {
            append(
                &mut review.changed_local,
                local,
                &candidate.local_positions,
                "local",
            );
            append(
                &mut review.changed_cloud,
                cloud,
                &candidate.cloud_positions,
                "cloud",
            );
        }
        ComparisonChangeClass::Ambiguous | ComparisonChangeClass::Unmatchable => {
            append(
                &mut review.changed_local,
                local,
                &candidate.local_positions,
                "local",
            );
            append(
                &mut review.changed_cloud,
                cloud,
                &candidate.cloud_positions,
                "cloud",
            );
        }
    }
}

fn review_feature(feature: &Value, detail: &FeatureComparisonDetail, side: &str) -> Value {
    let mut feature = feature.clone();
    let Some(object) = feature.as_object_mut() else {
        return feature;
    };
    let properties = object
        .entry("properties")
        .or_insert_with(|| Value::Object(Default::default()));
    if !properties.is_object() {
        *properties = Value::Object(Default::default());
    }
    let properties = properties
        .as_object_mut()
        .expect("review properties were normalized to an object");
    properties.insert(
        "__ds_comparison_layer".to_string(),
        Value::String(detail.layer_name.clone()),
    );
    properties.insert(
        "__ds_comparison_change_class".to_string(),
        serde_json::to_value(detail.change_class)
            .expect("comparison change classes always serialize"),
    );
    properties.insert(
        "__ds_comparison_side".to_string(),
        Value::String(side.to_string()),
    );
    if let Some(identity) = &detail.identity {
        properties.insert(
            "__ds_comparison_identity".to_string(),
            Value::String(identity.clone()),
        );
    }
    feature
}

fn changed_property_keys(
    local: &Value,
    cloud: &Value,
    ignored_property_keys: &BTreeSet<String>,
) -> Vec<String> {
    let local_properties = local.get("properties").and_then(Value::as_object);
    let cloud_properties = cloud.get("properties").and_then(Value::as_object);
    let keys = local_properties
        .into_iter()
        .flat_map(|properties| properties.keys())
        .chain(
            cloud_properties
                .into_iter()
                .flat_map(|properties| properties.keys()),
        )
        .filter(|key| !ignored_property_keys.contains(key.as_str()))
        .collect::<BTreeSet<_>>();
    keys.into_iter()
        .filter(|key| {
            local_properties.and_then(|properties| properties.get(*key))
                != cloud_properties.and_then(|properties| properties.get(*key))
        })
        .cloned()
        .collect()
}

fn geometry_type(feature: &Value) -> Option<String> {
    feature
        .get("geometry")
        .and_then(|geometry| geometry.get("type"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn component_geometry_type(features: &[LayerFeature<'_>], positions: &[usize]) -> Option<String> {
    let types = positions
        .iter()
        .filter_map(|position| geometry_type(features[*position].value))
        .collect::<BTreeSet<_>>();
    match types.len() {
        0 => None,
        1 => types.into_iter().next(),
        _ => Some("Mixed".to_string()),
    }
}

fn include_positions_bounds(
    bounds: &mut BoundsAccumulator,
    features: &[LayerFeature<'_>],
    positions: &[usize],
) {
    for position in positions {
        if let Some(geometry) = features[*position].value.get("geometry") {
            bounds.include_geometry(geometry);
        }
    }
}

#[derive(Default)]
struct BoundsAccumulator {
    bounds: Option<[f64; 4]>,
}

impl BoundsAccumulator {
    fn include_coord(&mut self, x: f64, y: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        match &mut self.bounds {
            Some(bounds) => {
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x);
                bounds[3] = bounds[3].max(y);
            }
            None => self.bounds = Some([x, y, x, y]),
        }
    }

    fn include_geometry(&mut self, geometry: &Value) {
        if geometry.get("type").and_then(Value::as_str) == Some("GeometryCollection") {
            if let Some(geometries) = geometry.get("geometries").and_then(Value::as_array) {
                for geometry in geometries {
                    self.include_geometry(geometry);
                }
            }
            return;
        }
        if let Some(coordinates) = geometry.get("coordinates") {
            self.include_coordinates(coordinates);
        }
    }

    fn include_coordinates(&mut self, value: &Value) {
        let Some(values) = value.as_array() else {
            return;
        };
        if values.len() >= 2
            && let (Some(x), Some(y)) = (values[0].as_f64(), values[1].as_f64())
        {
            self.include_coord(x, y);
            return;
        }
        for value in values {
            self.include_coordinates(value);
        }
    }

    fn include_bounds(&mut self, incoming: [f64; 4]) {
        self.include_coord(incoming[0], incoming[1]);
        self.include_coord(incoming[2], incoming[3]);
    }

    fn finish(self) -> Option<[f64; 4]> {
        self.bounds
    }
}

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(len: usize) -> Self {
        Self {
            parent: (0..len).collect(),
            rank: vec![0; len],
        }
    }

    fn find(&mut self, node: usize) -> usize {
        if self.parent[node] != node {
            self.parent[node] = self.find(self.parent[node]);
        }
        self.parent[node]
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left_root = self.find(left);
        let mut right_root = self.find(right);
        if left_root == right_root {
            return;
        }
        if self.rank[left_root] < self.rank[right_root] {
            std::mem::swap(&mut left_root, &mut right_root);
        }
        self.parent[right_root] = left_root;
        if self.rank[left_root] == self.rank[right_root] {
            self.rank[left_root] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn feature(id: Value, geometry: Value, properties: Value) -> Value {
        json!({
            "type": "Feature",
            "id": id,
            "geometry": geometry,
            "properties": properties
        })
    }

    fn point(id: &str, x: f64, y: f64, properties: Value) -> Value {
        feature(
            json!(id),
            json!({ "type": "Point", "coordinates": [x, y] }),
            properties,
        )
    }

    fn collection(features: Vec<Value>) -> Value {
        json!({ "type": "FeatureCollection", "features": features })
    }

    fn input(local: Value, cloud: Value) -> GeoJsonLayerComparisonInput {
        GeoJsonLayerComparisonInput {
            local_layers: BTreeMap::from([("layer".to_string(), local)]),
            cloud_layers: BTreeMap::from([("layer".to_string(), cloud)]),
            layer_names: Vec::new(),
            ignored_property_keys: Vec::new(),
            max_detail_features: 500,
            detail_change_classes: Vec::new(),
        }
    }

    #[test]
    fn classifies_every_change_class_and_exact_totals() {
        let local = collection(vec![
            point("same", 0.0, 0.0, json!({"name": "same"})),
            point("attr", 1.0, 1.0, json!({"name": "local"})),
            feature(
                json!("geom"),
                json!({"type":"LineString","coordinates":[[2,2],[3,3]]}),
                json!({"name": "same"}),
            ),
            point("both", 5.0, 5.0, json!({"name": "local", "rating": 2})),
            point("local", 7.0, 7.0, json!({})),
        ]);
        let cloud = collection(vec![
            point("same", 0.0, 0.0, json!({"name": "same"})),
            point("attr", 1.0, 1.0, json!({"name": "cloud"})),
            point("geom", 2.0, 2.0, json!({"name": "same"})),
            point("both", 4.0, 4.0, json!({"name": "cloud"})),
            point("cloud", -1.0, -2.0, json!({})),
        ]);
        let result = compare_geojson_layers(input(local, cloud)).unwrap();
        assert_eq!(
            result.totals,
            ComparisonCounts {
                local_feature_count: 5,
                cloud_feature_count: 5,
                matched: 4,
                local_only: 1,
                cloud_only: 1,
                unchanged: 1,
                attribute_only_changed: 1,
                geometry_only_changed: 1,
                attribute_and_geometry_changed: 1,
                ..ComparisonCounts::default()
            }
        );
        assert_eq!(
            result.changed_attribute_key_counts,
            BTreeMap::from([("name".to_string(), 2), ("rating".to_string(), 1)])
        );
        assert_eq!(result.change_bounds, Some([-1.0, -2.0, 7.0, 7.0]));
        let layer = &result.layers[0];
        assert_eq!(layer.total_detail_count, 5);
        assert_eq!(layer.returned_detail_count, 5);
        assert_eq!(layer.review_collections.local_only.features.len(), 1);
        assert_eq!(layer.review_collections.cloud_only.features.len(), 1);
        assert_eq!(layer.review_collections.changed_local.features.len(), 3);
        assert_eq!(layer.review_collections.changed_cloud.features.len(), 3);
        let reviewed_attribute = layer
            .review_collections
            .changed_local
            .features
            .iter()
            .find(|feature| feature["id"] == "attr")
            .unwrap();
        assert_eq!(reviewed_attribute["properties"]["name"], "local");
        assert_eq!(
            reviewed_attribute["properties"]["__ds_comparison_change_class"],
            "attribute_only"
        );
        assert_eq!(
            reviewed_attribute["properties"]["__ds_comparison_layer"],
            "layer"
        );
        let geometry_detail = layer
            .details
            .iter()
            .find(|detail| detail.identity.as_deref() == Some("geom"))
            .unwrap();
        assert_eq!(
            geometry_detail.local_geometry_type.as_deref(),
            Some("LineString")
        );
        assert_eq!(
            geometry_detail.cloud_geometry_type.as_deref(),
            Some("Point")
        );
        let classes = layer
            .details
            .iter()
            .map(|detail| detail.change_class)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            classes,
            BTreeSet::from([
                ComparisonChangeClass::LocalOnly,
                ComparisonChangeClass::CloudOnly,
                ComparisonChangeClass::AttributeOnly,
                ComparisonChangeClass::GeometryOnly,
                ComparisonChangeClass::AttributeAndGeometry,
            ])
        );
    }

    #[test]
    fn aliases_and_numeric_ids_match_without_row_or_geometry_fallback() {
        let local = collection(vec![
            feature(
                json!("new-top"),
                json!({"type":"Point","coordinates":[10,10]}),
                json!({"doc_id":"persisted", "name":"same"}),
            ),
            feature(
                Value::Null,
                json!({"type":"Point","coordinates":[20,20]}),
                json!({"feature_id": 7}),
            ),
        ]);
        let cloud = collection(vec![
            feature(
                json!("old-top"),
                json!({"type":"Point","coordinates":[10,10]}),
                json!({"doc_id":"persisted", "name":"same"}),
            ),
            feature(
                json!(7),
                json!({"type":"Point","coordinates":[20,20]}),
                json!({"feature_id": 7}),
            ),
        ]);
        let result = compare_geojson_layers(input(local, cloud)).unwrap();
        assert_eq!(result.totals.matched, 2);
        // Top-level ids are identity/presentation, not domain attributes.
        assert_eq!(result.totals.unchanged, 2);
        assert_eq!(result.total_detail_count, 0);
    }

    #[test]
    fn duplicate_aliases_and_missing_identity_are_explicit_not_paired() {
        let mut duplicate = point("second", 2.0, 2.0, json!({"doc_id":"shared"}));
        duplicate["properties"]["feature_id"] = json!("other-alias");
        let local = collection(vec![
            point("shared", 1.0, 1.0, json!({})),
            duplicate,
            feature(
                Value::Null,
                json!({"type":"Point","coordinates":[9,9]}),
                json!({}),
            ),
        ]);
        let cloud = collection(vec![point("shared", 0.0, 0.0, json!({}))]);
        let result = compare_geojson_layers(input(local, cloud)).unwrap();
        assert_eq!(result.totals.matched, 0);
        assert_eq!(result.totals.ambiguous_identity_features, 3);
        assert_eq!(result.totals.unmatchable_features, 1);
        assert_eq!(result.totals.ambiguous_unmatchable, 4);
        assert_eq!(result.totals.local_ambiguous_unmatchable, 3);
        assert_eq!(result.totals.cloud_ambiguous_unmatchable, 1);
        let details = &result.layers[0].details;
        assert_eq!(details.len(), 4);
        let ambiguous = details
            .iter()
            .filter(|detail| detail.change_class == ComparisonChangeClass::Ambiguous)
            .collect::<Vec<_>>();
        assert_eq!(ambiguous.len(), 3);
        for detail in ambiguous {
            assert_eq!(
                detail.local_feature_indexes.len() + detail.cloud_feature_indexes.len(),
                1
            );
            let issue = detail.identity_issue.as_ref().unwrap();
            assert_eq!(issue.kind, IdentityIssueKind::AmbiguousIdentity);
            assert_eq!(issue.component_local_feature_count, 2);
            assert_eq!(issue.component_cloud_feature_count, 1);
            assert_eq!(issue.component_alias_count, 3);
        }
        let unmatchable = details
            .iter()
            .find(|detail| detail.change_class == ComparisonChangeClass::Unmatchable)
            .unwrap();
        assert_eq!(
            unmatchable.identity_issue.as_ref().unwrap().kind,
            IdentityIssueKind::MissingIdentity
        );

        let review = &result.layers[0].review_collections;
        assert_eq!(review.changed_local.features.len(), 3);
        assert_eq!(review.changed_cloud.features.len(), 1);
        let review_classes = review
            .changed_local
            .features
            .iter()
            .chain(review.changed_cloud.features.iter())
            .map(|feature| {
                (
                    feature["properties"]["__ds_comparison_change_class"]
                        .as_str()
                        .unwrap(),
                    feature["properties"]["__ds_comparison_side"]
                        .as_str()
                        .unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            review_classes,
            vec![
                ("ambiguous", "local"),
                ("ambiguous", "local"),
                ("unmatchable", "local"),
                ("ambiguous", "cloud"),
            ]
        );
    }

    #[test]
    fn ignored_keys_are_echoed_and_missing_remains_different_from_null() {
        let local = collection(vec![point(
            "a",
            1.0,
            2.0,
            json!({"v_first": 8, "v_last": 9, "nullable": null}),
        )]);
        let cloud = collection(vec![point(
            "a",
            1.0,
            2.0,
            json!({"v_first": 1, "v_last": 2}),
        )]);
        let mut comparison_input = input(local, cloud);
        comparison_input.ignored_property_keys = vec![
            "v_last".to_string(),
            "v_first".to_string(),
            "v_last".to_string(),
        ];
        let result = compare_geojson_layers(comparison_input).unwrap();
        assert_eq!(result.ignored_property_keys, vec!["v_first", "v_last"]);
        assert_eq!(result.totals.attribute_only_changed, 1);
        assert_eq!(
            result.layers[0].details[0].changed_property_keys,
            vec!["nullable"]
        );
    }

    #[test]
    fn added_removed_layers_and_explicit_layer_filter_are_exact() {
        let mut comparison_input = GeoJsonLayerComparisonInput {
            local_layers: BTreeMap::from([
                (
                    "z_local".to_string(),
                    collection(vec![point("l", 1.0, 1.0, json!({}))]),
                ),
                (
                    "ignored".to_string(),
                    collection(vec![point("i", 2.0, 2.0, json!({}))]),
                ),
            ]),
            cloud_layers: BTreeMap::from([(
                "a_cloud".to_string(),
                collection(vec![point("c", 0.0, 0.0, json!({}))]),
            )]),
            layer_names: Vec::new(),
            ignored_property_keys: Vec::new(),
            max_detail_features: 500,
            detail_change_classes: Vec::new(),
        };
        let all = compare_geojson_layers(comparison_input.clone()).unwrap();
        assert_eq!(
            all.layers
                .iter()
                .map(|layer| layer.layer_name.as_str())
                .collect::<Vec<_>>(),
            vec!["a_cloud", "ignored", "z_local"]
        );
        assert_eq!(all.totals.local_only, 2);
        assert_eq!(all.totals.cloud_only, 1);

        comparison_input.layer_names = vec!["z_local".to_string(), "a_cloud".to_string()];
        let filtered = compare_geojson_layers(comparison_input).unwrap();
        assert_eq!(filtered.layers.len(), 2);
        assert_eq!(filtered.totals.local_only, 1);
        assert_eq!(filtered.totals.cloud_only, 1);
    }

    #[test]
    fn details_are_sorted_by_layer_then_identity() {
        let comparison_input = GeoJsonLayerComparisonInput {
            local_layers: BTreeMap::from([
                (
                    "z".to_string(),
                    collection(vec![
                        point("zeta", 0.0, 0.0, json!({})),
                        point("alpha", 0.0, 0.0, json!({})),
                    ]),
                ),
                (
                    "a".to_string(),
                    collection(vec![point("middle", 0.0, 0.0, json!({}))]),
                ),
            ]),
            cloud_layers: BTreeMap::new(),
            layer_names: Vec::new(),
            ignored_property_keys: Vec::new(),
            max_detail_features: 500,
            detail_change_classes: Vec::new(),
        };
        let result = compare_geojson_layers(comparison_input).unwrap();
        assert_eq!(result.layers[0].layer_name, "a");
        assert_eq!(result.layers[1].layer_name, "z");
        assert_eq!(
            result.layers[1]
                .details
                .iter()
                .filter_map(|detail| detail.identity.as_deref())
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
    }

    #[test]
    fn bounds_walk_all_geojson_geometry_nesting() {
        let local = collection(vec![feature(
            json!("all"),
            json!({
                "type": "GeometryCollection",
                "geometries": [
                    {"type":"MultiPoint", "coordinates":[[-4, 8], [2, -3]]},
                    {"type":"Polygon", "coordinates":[[[0,0],[9,0],[9,5],[0,0]]]}
                ]
            }),
            json!({}),
        )]);
        let result = compare_geojson_layers(input(local, collection(vec![]))).unwrap();
        assert_eq!(result.change_bounds, Some([-4.0, -3.0, 9.0, 8.0]));
    }

    #[test]
    fn truncation_caps_payload_but_not_totals_bounds_or_key_counts() {
        let local = collection(vec![
            point("a", 10.0, 10.0, json!({"name":"new"})),
            point("b", 20.0, 20.0, json!({"name":"new"})),
            point("c", 30.0, 30.0, json!({})),
        ]);
        let cloud = collection(vec![
            point("a", 0.0, 0.0, json!({"name":"old"})),
            point("b", 2.0, 2.0, json!({"name":"old"})),
        ]);
        let mut comparison_input = input(local, cloud);
        comparison_input.max_detail_features = 1;
        let result = compare_geojson_layers(comparison_input).unwrap();
        assert_eq!(result.total_detail_count, 3);
        assert_eq!(result.returned_detail_count, 1);
        assert!(result.details_truncated);
        assert_eq!(result.totals.attribute_and_geometry_changed, 2);
        assert_eq!(result.totals.local_only, 1);
        assert_eq!(result.changed_attribute_key_counts["name"], 2);
        assert_eq!(result.change_bounds, Some([0.0, 0.0, 30.0, 30.0]));
        assert_eq!(result.layers[0].details.len(), 1);
        let review_feature_count = result.layers[0]
            .review_collections
            .local_only
            .features
            .len()
            + result.layers[0]
                .review_collections
                .cloud_only
                .features
                .len()
            + result.layers[0]
                .review_collections
                .changed_local
                .features
                .len()
            + result.layers[0]
                .review_collections
                .changed_cloud
                .features
                .len();
        assert!(review_feature_count <= 2);
    }

    #[test]
    fn detail_class_filter_only_narrows_bounded_payload() {
        let local = collection(vec![
            point("attr", 1.0, 1.0, json!({"name":"new"})),
            point("new", 2.0, 2.0, json!({})),
        ]);
        let cloud = collection(vec![point("attr", 1.0, 1.0, json!({"name":"old"}))]);
        let mut comparison_input = input(local, cloud);
        comparison_input.detail_change_classes = vec![ComparisonChangeClass::AttributeOnly];
        let result = compare_geojson_layers(comparison_input).unwrap();
        assert_eq!(result.totals.local_only, 1);
        assert_eq!(result.totals.attribute_only_changed, 1);
        assert_eq!(result.total_detail_count, 1);
        assert_eq!(
            result.layers[0].details[0].change_class,
            ComparisonChangeClass::AttributeOnly
        );
        assert!(
            result.layers[0]
                .review_collections
                .local_only
                .features
                .is_empty()
        );
    }

    #[test]
    fn exact_geometry_and_properties_ignore_other_top_level_fields() {
        let mut local = point("a", 1.0, 2.0, json!({"x": 1}));
        local["bbox"] = json!([0, 0, 10, 10]);
        local["title"] = json!("local presentation");
        let mut cloud = local.clone();
        cloud["bbox"] = json!([-1, -1, 1, 1]);
        cloud["title"] = json!("cloud presentation");
        let unchanged = compare_geojson_layers(input(
            collection(vec![local.clone()]),
            collection(vec![cloud.clone()]),
        ))
        .unwrap();
        assert_eq!(unchanged.totals.unchanged, 1);

        cloud["geometry"]["coordinates"] = json!([1.0, 2.0000000001]);
        let changed =
            compare_geojson_layers(input(collection(vec![local]), collection(vec![cloud])))
                .unwrap();
        assert_eq!(changed.totals.geometry_only_changed, 1);
    }

    #[test]
    fn rejects_non_feature_collection_layers() {
        let error = compare_geojson_layers(input(
            json!({"type":"Feature", "id":"a", "geometry":null, "properties":{}}),
            collection(vec![]),
        ))
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("expected a GeoJSON FeatureCollection")
        );
    }

    #[test]
    fn recommended_snake_case_input_envelope_is_accepted() {
        let parsed: GeoJsonLayerComparisonInput = serde_json::from_value(json!({
            "local_layers": {"layer": collection(vec![])},
            "cloud_layers": {"layer": collection(vec![])},
            "layer_names": ["layer"],
            "ignored_property_keys": ["v_last"],
            "max_detail_features": 12,
            "detail_change_classes": ["geometry_only"]
        }))
        .unwrap();
        let result = compare_geojson_layers(parsed).unwrap();
        assert_eq!(result.max_detail_features, 12);
        assert_eq!(result.ignored_property_keys, vec!["v_last"]);
        assert_eq!(
            result.detail_change_classes,
            vec![ComparisonChangeClass::GeometryOnly]
        );
    }
}
