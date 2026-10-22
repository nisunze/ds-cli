//! Selection coordinator — single source of truth for resolving a
//! map-clicked feature's identity to its position inside one of the
//! attribute-table tabs.
//!
//! ds-web passes a point-in-time table snapshot through the pure lookup export;
//! matching rules and precedence live here for every window.
//!
//! Rules (mirroring the original TS):
//!  1. Collect every distinct candidate id from the rendered feature id and
//!     `properties.id` / `doc_id` / `feature_id` / `_sketches_id`.
//!  2. Walk every registered tab in its exact native model/revision/source
//!     context; any candidate id-map hit produces a match.
//!  3. Classify each match as "direct" (the tab's `form_slug`,
//!     `layer_id`, design-layer or sketch-layer property aligns with the
//!     lookup options) or "fallback" (id matched but no preference signal).
//!  4. First direct match wins. If no direct, first fallback wins.
//!     Order across tabs is ascending by tab id (deterministic).
//!
//! Contract: [`ds-network/docs/contracts/selection-contract.md`].
//! Phase 3α of the cross-window-bridge consolidation; see also
//! `docs/editing-contract.md` in ds-web.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

const DESIGN_LAYER_PROP: &str = "__ds_design_layer";

/// Wire input for `set_tab` — the caller's view of one tab's
/// id-to-row-index mapping plus the metadata needed to classify
/// matches as direct vs fallback.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(hashmap_as_object, missing_as_null)
)]
pub struct TabIndexInput {
    pub tab_id: String,
    /// The tab's `id` field in the TS `TableTab` (e.g. `survey-customers`,
    /// `lv:lv_lines`, `sketch-abc`). Used for `layer_id` preference matching
    /// and the design-layer prefix check.
    pub layer_id: String,
    /// Optional form slug (survey tabs only). Survey-side preference
    /// matching uses this against `properties.form_slug` / `table_name`.
    pub form_slug: Option<String>,
    /// Revision-pinned native-model identity. A lookup carrying a domain
    /// context may only match a tab with this exact address.
    #[serde(default)]
    pub domain_context: Option<SelectionDomainContext>,
    /// `(id, row_index)` pairs. Position in the array does not have to
    /// equal the row index — the row index is taken from the tuple.
    pub id_map: Vec<(String, usize)>,
}

/// Wire input for a feature lookup — the candidate properties payload
/// plus optional preference signals (the TS site passes `layerId` /
/// `formSlug` derived from the click context).
#[derive(Clone, Debug, Deserialize)]
pub struct FeatureLookupInput {
    pub properties: HashMap<String, Value>,
    #[serde(default)]
    pub layer_id: Option<String>,
    #[serde(default)]
    pub form_slug: Option<String>,
    #[serde(default)]
    pub feature_id: Option<String>,
    #[serde(default)]
    pub domain_context: Option<SelectionDomainContext>,
}

/// Pure lookup input. Carries a point-in-time tab snapshot and lookup payload
/// together, so hosts do not need to keep a mutable coordinator handle as
/// source-of-truth state.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(hashmap_as_object, missing_as_null)
)]
pub struct FeatureLookupTabsInput {
    #[serde(default)]
    pub tabs: Vec<TabIndexInput>,
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub properties: HashMap<String, Value>,
    #[serde(default)]
    pub layer_id: Option<String>,
    #[serde(default)]
    pub form_slug: Option<String>,
    #[serde(default)]
    pub feature_id: Option<String>,
    #[serde(default)]
    pub domain_context: Option<SelectionDomainContext>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SelectionDomainContext {
    Grid {
        model_id: String,
        model_revision: String,
        source_layer: String,
    },
}

/// Wire output for a successful lookup. Maps to TS `FeatureTabMatch`
/// (minus `row` and `tab` — the caller hydrates those locally).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
pub struct FeatureLookupResult {
    pub tab_id: String,
    pub row_index: usize,
}

/// Admit a browser lookup response before the host hydrates its held row.
/// Additive response fields are compatible; identity and index requirements
/// match the browser boundary on every native word size.
pub fn admit_feature_lookup_result(
    result: Option<FeatureLookupResult>,
    held_tab_ids: &[String],
) -> Result<Option<FeatureLookupResult>, String> {
    let Some(result) = result else {
        return Ok(None);
    };
    if result.tab_id.is_empty()
        || u64::try_from(result.row_index).map_or(true, |index| index > 9_007_199_254_740_991)
    {
        return Err(
            "selection_response_incompatible: lookup identity or row index is invalid".into(),
        );
    }
    if !held_tab_ids.contains(&result.tab_id) {
        return Err(
            "selection_response_incompatible: returned tab is outside the held snapshot".into(),
        );
    }
    Ok(Some(result))
}

/// Widened selection result — identity + geometry + bbox + domain tag.
///
/// Produced by [`crate::session::SessionHandle::selection_resolution`].
/// Closes ds-web's "MV doesn't highlight on click" gap by carrying the
/// full geometry alongside identity in a single trip — no transformer-
/// cache lookup, no async hop, no source-type branching at the call
/// site.
///
/// `domain` is a free-form tag (`"lv"`, `"mv"`, `"temp"`, `"design"`)
/// that ds-web uses for highlight tinting. The default tag is
/// `"design"` until a [`crate::session::SessionHandle`] is given a
/// domain plug-in.
///
/// Contract: `docs/contracts/geojson-foundation-crate.md` §6.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionResolution {
    pub tab_id: String,
    pub row_index: usize,
    pub feature_id: String,
    pub geometry: crate::session::Geometry,
    pub domain: String,
    /// `[min_lon, min_lat, max_lon, max_lat]` — for autozoom requests.
    pub bbox: [f64; 4],
}

#[derive(Clone, Debug)]
struct TabSnapshot {
    layer_id: String,
    form_slug: Option<String>,
    domain_context: Option<SelectionDomainContext>,
    id_map: HashMap<String, usize>,
}

/// In-memory cache of every registered tab's id map. The frontend
/// keeps this in sync with the `tableTabs` store; lookups are O(1) per
/// tab against a `HashMap<id, row_index>`.
#[derive(Clone, Debug, Default)]
pub struct SelectionCoordinator {
    /// BTreeMap to make fallback order deterministic (ascending by
    /// tab_id) — important for tests and for predictable behavior
    /// when two tabs both contain the same id and no preference
    /// signal disambiguates them.
    tabs: BTreeMap<String, TabSnapshot>,
}

impl SelectionCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a tab. Re-sending an existing `tab_id`
    /// fully replaces its id map — the coordinator never merges
    /// across set_tab calls.
    pub fn set_tab(&mut self, input: TabIndexInput) {
        let mut id_map = HashMap::with_capacity(input.id_map.len());
        for (id, idx) in input.id_map {
            id_map.insert(id, idx);
        }
        self.tabs.insert(
            input.tab_id,
            TabSnapshot {
                layer_id: input.layer_id,
                form_slug: input.form_slug,
                domain_context: input.domain_context,
                id_map,
            },
        );
    }

    pub fn remove_tab(&mut self, tab_id: &str) {
        self.tabs.remove(tab_id);
    }

    pub fn clear(&mut self) {
        self.tabs.clear();
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Resolve a feature identity into its `(tab_id, row_index)`
    /// location across every registered tab. Returns `None` when
    /// none of the candidate id keys produce a hit.
    pub fn find_feature_in_tabs(&self, input: FeatureLookupInput) -> Option<FeatureLookupResult> {
        let prop_ids = lookup_ids(input.feature_id.as_deref(), &input.properties);
        if prop_ids.is_empty() {
            return None;
        }
        let preferred_form_slug = input
            .form_slug
            .map(|value| value.trim().to_string())
            .or_else(|| property_string(&input.properties, "form_slug"))
            .or_else(|| property_string(&input.properties, "table_name"))
            .unwrap_or_default();
        let preferred_layer_id = input.layer_id.unwrap_or_default();
        let preferred_design_layer =
            property_string(&input.properties, DESIGN_LAYER_PROP).unwrap_or_default();
        let preferred_sketch_layer = property_string(&input.properties, "_sketches_layer")
            .or_else(|| sketch_layer_from_runtime_id(preferred_layer_id.trim()))
            .unwrap_or_default();

        let mut direct: Option<FeatureLookupResult> = None;
        let mut fallback: Option<FeatureLookupResult> = None;

        for (tab_id, tab) in &self.tabs {
            if input.domain_context.is_some()
                && tab.domain_context.as_ref() != input.domain_context.as_ref()
            {
                continue;
            }
            let Some(row_index) = prop_ids
                .iter()
                .find_map(|prop_id| tab.id_map.get(prop_id).copied())
            else {
                continue;
            };
            let result = FeatureLookupResult {
                tab_id: tab_id.clone(),
                row_index,
            };

            let form_slug_matches = !preferred_form_slug.is_empty()
                && tab.form_slug.as_deref() == Some(preferred_form_slug.as_str());
            let layer_matches =
                !preferred_layer_id.is_empty() && tab.layer_id == preferred_layer_id;
            let design_matches = !preferred_design_layer.is_empty()
                && (tab.layer_id == format!("lv:{preferred_design_layer}")
                    || tab.layer_id == format!("mv:{preferred_design_layer}"));
            let sketch_matches = !preferred_sketch_layer.is_empty()
                && tab.layer_id == format!("sketch-{preferred_sketch_layer}");

            if form_slug_matches || layer_matches || design_matches || sketch_matches {
                if direct.is_none() {
                    direct = Some(result);
                }
            } else if fallback.is_none() {
                fallback = Some(result);
            }
        }

        direct.or(fallback)
    }
}

/// Pure one-shot lookup over an inline tab snapshot. This is the preferred
/// boundary for browser hosts: Svelte owns the current table state, Rust owns
/// the deterministic matching rules, and no mutable WASM coordinator can go
/// stale between windows.
pub fn find_feature_in_tabs(input: FeatureLookupTabsInput) -> Option<FeatureLookupResult> {
    let mut coordinator = SelectionCoordinator::new();
    for tab in input.tabs {
        coordinator.set_tab(tab);
    }
    coordinator.find_feature_in_tabs(FeatureLookupInput {
        properties: input.properties,
        layer_id: input.layer_id,
        form_slug: input.form_slug,
        feature_id: input.feature_id,
        domain_context: input.domain_context,
    })
}

/// Every distinct candidate identity in precedence order. A rendered engine
/// id is authoritative when present; property aliases remain available so a
/// tab can match a different public-key projection of the same feature.
fn lookup_ids(feature_id: Option<&str>, properties: &HashMap<String, Value>) -> Vec<String> {
    let mut candidates = Vec::with_capacity(5);
    if let Some(feature_id) = feature_id {
        push_identity(&mut candidates, feature_id.trim().to_string());
    }
    for key in ["id", "doc_id", "feature_id", "_sketches_id"] {
        let candidate = match properties.get(key) {
            Some(Value::String(value)) => value.trim().to_string(),
            Some(Value::Number(value)) => value.to_string(),
            Some(Value::Bool(value)) => value.to_string(),
            _ => continue,
        };
        push_identity(&mut candidates, candidate);
    }
    candidates
}

fn push_identity(candidates: &mut Vec<String>, candidate: String) {
    if !candidate.is_empty() && !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn property_string(properties: &HashMap<String, Value>, key: &str) -> Option<String> {
    match properties.get(key)? {
        Value::Null => None,
        Value::String(value) => Some(value.trim().to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn sketch_layer_from_runtime_id(runtime_id: &str) -> Option<String> {
    let logical = runtime_id.strip_prefix("sketches-")?;
    let logical = ["-fill", "-line", "-circle", "-symbol", "-label"]
        .iter()
        .find_map(|suffix| logical.strip_suffix(suffix))
        .unwrap_or(logical)
        .trim();
    (!logical.is_empty()).then(|| logical.to_string())
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
