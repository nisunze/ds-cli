//! Browser adapter for Kernel-owned design working state.
//!
//! This is an in-memory editing owner. Durable SQLite/application admission is
//! separate; it does not infer project, principal or model authority.
use ds_design_session::{selection as core_selection, session as core_session};
use serde_json::{Map, Value};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
thread_local! {
    static LAST_HOVER_LOG_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// One layer's row window as the session holds it: its own feature ids,
/// geometry and property values, in source row order. The page reads it as
/// typed JSON, so no column codec is built for rows that never left the page.
fn row_window_json(window: &ds_design_session::dataframe::RowWindow) -> Result<String, String> {
    serde_json::to_string(window).map_err(|err| err.to_string())
}

/// The window's rows as the Network unit's `tableChunk` request: exactly the
/// features and first-seen columns the Kernel encoded itself until it linked
/// no Arrow codec, so the unit's chunk is byte-identical to the former one.
fn row_window_chunk_request(
    window: ds_design_session::dataframe::RowWindow,
) -> Result<String, String> {
    let features = window
        .rows
        .into_iter()
        .map(|row| {
            Ok(serde_json::json!({
                "type": "Feature",
                "id": row.feature_id,
                "geometry": serde_json::to_value(row.geometry).map_err(|err| err.to_string())?,
                "properties": row.properties,
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    serde_json::to_string(&ds_network_contracts::table_query::TableChunkRequest::new(
        features,
    ))
    .map_err(|err| err.to_string())
}

fn log_hash(value: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")[..10].to_string()
}

fn log_id(value: &str) -> String {
    format!("id#{}", log_hash(value))
}

fn log_coord(lng: f64, lat: f64) -> String {
    log_hash(&format!("{lng:.6},{lat:.6}"))
}

#[inline]
fn log_info(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&JsValue::from_str(&format!("[ds-design-session] {msg}")));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = msg;
}

#[inline]
fn log_warn(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::warn_1(&JsValue::from_str(&format!("[ds-design-session] {msg}")));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = msg;
}

#[inline]
fn log_hover_info(key: String, msg: String) {
    LAST_HOVER_LOG_KEY.with(|last| {
        let mut last = last.borrow_mut();
        if last.as_deref() == Some(key.as_str()) {
            return;
        }
        *last = Some(key);
        log_info(&msg);
    });
}

fn parse_json<T: serde::de::DeserializeOwned>(input_json: &str) -> Result<T, JsError> {
    serde_json::from_str(input_json).map_err(|err| JsError::new(&err.to_string()))
}

fn stringify_json<T: serde::Serialize>(value: &T) -> Result<String, JsError> {
    serde_json::to_string(value).map_err(|err| JsError::new(&err.to_string()))
}

/// Legacy JSON ABI over the same authoritative lookup types and algorithm.
/// New browser callers use the generated typed `findFeatureInTabs` entry.
#[wasm_bindgen]
pub fn find_feature_in_tabs_v2(input_json: &str) -> Result<Option<String>, JsError> {
    let input: core_selection::FeatureLookupTabsInput = parse_json(input_json)?;
    match core_selection::find_feature_in_tabs(input) {
        Some(result) => Ok(Some(stringify_json(&result)?)),
        None => Ok(None),
    }
}

/// Retain the legacy JSON value semantics inside Rust. In particular, a JS
/// array cannot become a properties map, and nonfinite numbers normalize as
/// JSON before the authoritative request/result types admit them.
#[cfg(target_arch = "wasm32")]
fn selection_json(value: JsValue) -> Result<String, JsValue> {
    js_sys::JSON::stringify(&value)?
        .as_string()
        .ok_or_else(|| JsError::new("selection_snapshot_invalid: payload is not JSON").into())
}

#[cfg(target_arch = "wasm32")]
fn typed_selection_result(
    result: Option<core_selection::FeatureLookupResult>,
    held_tab_ids: &[String],
) -> Result<Option<tsify::Ts<core_selection::FeatureLookupResult>>, JsValue> {
    use tsify::Tsify;
    core_selection::admit_feature_lookup_result(result, held_tab_ids)
        .map_err(|error| JsError::new(&error))?
        .map(|result| {
            result
                .into_ts()
                .map_err(|error| JsError::new(&error.to_string()).into())
        })
        .transpose()
}

/// One-shot lookup from the generated owning request to its owning result.
/// No mutable coordinator or independent browser protocol is installed.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = findFeatureInTabs)]
pub fn find_feature_in_tabs(
    request: tsify::Ts<core_selection::FeatureLookupTabsInput>,
) -> Result<Option<tsify::Ts<core_selection::FeatureLookupResult>>, JsValue> {
    let input: core_selection::FeatureLookupTabsInput =
        parse_json(&selection_json(request.into())?)?;
    let held_tab_ids: Vec<_> = input.tabs.iter().map(|tab| tab.tab_id.clone()).collect();
    typed_selection_result(core_selection::find_feature_in_tabs(input), &held_tab_ids)
}

/// Admit the actual callable's reply before a browser hydrates its held row.
/// The leaf owner validates fields and membership; extra response facts remain
/// compatible and never select a different protocol or implementation.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = decodeFeatureLookupResult)]
pub fn decode_feature_lookup_result(
    response: Option<tsify::Ts<core_selection::FeatureLookupResult>>,
    held_tab_ids: Vec<String>,
) -> Result<Option<tsify::Ts<core_selection::FeatureLookupResult>>, JsValue> {
    let result = response
        .map(|response| {
            let response_json = selection_json(response.into())?;
            // serde structs also accept positional arrays; the browser reply
            // contract requires an object, including after a custom toJSON.
            if !response_json.starts_with('{') {
                return Err(JsError::new(
                    "selection_response_incompatible: lookup response must be an object",
                )
                .into());
            }
            let result: core_selection::FeatureLookupResult = parse_json(&response_json)?;
            Ok::<_, JsValue>(result)
        })
        .transpose()?;
    typed_selection_result(result, &held_tab_ids)
}

/// Cross-window selection coordinator (Phase 3α). Owns the per-tab
/// id-map index and resolves a feature's identity to a
/// `(tab_id, row_index)` pair. ds-web's `findFeatureInTabs` runs
/// through this on both the main window and any pop-out — the
/// wire only carries action tokens, never the id map itself.
///
/// Lifecycle:
///   1. `new SelectionCoordinator()` when the frontend first needs lookup.
///   2. `setTab(json)` whenever the `tableTabs` store changes.
///   3. `findFeatureInTabs(json)` per click — sync, cheap.
#[wasm_bindgen]
pub struct SelectionCoordinator {
    inner: core_selection::SelectionCoordinator,
}

#[wasm_bindgen]
impl SelectionCoordinator {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: core_selection::SelectionCoordinator::new(),
        }
    }

    /// Insert or replace a tab's id map. Input shape — see
    /// `core_selection::TabIndexInput`.
    #[wasm_bindgen(js_name = "setTab")]
    pub fn set_tab(&mut self, input_json: &str) -> Result<(), JsError> {
        let input: core_selection::TabIndexInput = parse_json(input_json)?;
        self.inner.set_tab(input);
        Ok(())
    }

    #[wasm_bindgen(js_name = "removeTab")]
    pub fn remove_tab(&mut self, tab_id: &str) {
        self.inner.remove_tab(tab_id);
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    #[wasm_bindgen(js_name = "tabCount")]
    pub fn tab_count(&self) -> usize {
        self.inner.tab_count()
    }

    /// Resolve a feature identity to its tab + row. Returns `null`
    /// when none of the candidate id keys produce a hit. JSON output
    /// shape — see `core_selection::FeatureLookupResult`.
    #[wasm_bindgen(js_name = "findFeatureInTabs")]
    pub fn find_feature_in_tabs(&self, input_json: &str) -> Result<Option<String>, JsError> {
        let input: core_selection::FeatureLookupInput = parse_json(input_json)?;
        match self.inner.find_feature_in_tabs(input) {
            Some(result) => Ok(Some(
                serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))?,
            )),
            None => Ok(None),
        }
    }
}

impl Default for SelectionCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Phase A.5 of the design-session-state contract — opaque handle holding
/// one drawing session's worth of features and an R-tree index over them.
/// JS calls `openSession` with a JSON feature payload, then `pick` to
/// resolve clicks. `insertFeature` / `replaceFeature` / `removeFeature`
/// keep the index in lock-step with TS-side mutations from
/// `lib/editing/operations.ts`.
///
/// The JSON shape is GeoJSON-native — `id` is `{layerName}/{featureId}`
/// (so a `pick` answer carries the layer without a second round-trip into
/// the TS gdfs dict), and `geometry` is exactly the MapLibre/GeoJSON form
/// every TS consumer already speaks:
/// ```json
/// [
///   { "id": "lv_pole/abc", "geometry": { "type": "Point", "coordinates": [30.06, -1.95] } },
///   { "id": "lv_line/xyz", "geometry": { "type": "LineString", "coordinates": [[30.06,-1.95],[30.061,-1.951]] } },
///   { "id": "sketches_polygons/xyz", "geometry": { "type": "Polygon", "coordinates": [[[…]]] } }
/// ]
/// ```
///
/// `id` is `{layerName}/{featureId}` so a `pick` answer carries the layer
/// without a second round-trip into the TS gdfs dict.
///
/// The browser application build also owns one
/// Polars-backed table per layer. Row reads and property-only edits go
/// through that table instead of rebuilding from JS-side `gdfs`.
#[wasm_bindgen]
pub struct SessionHandle {
    inner: core_session::SessionHandle,
}

#[wasm_bindgen]
impl SessionHandle {
    /// Build a session from a JSON feature list.
    #[wasm_bindgen(js_name = "openSession")]
    pub fn open_session(features_json: &str) -> Result<SessionHandle, JsError> {
        let features: Vec<core_session::Feature> = match serde_json::from_str(features_json) {
            Ok(v) => v,
            Err(e) => {
                log_warn(&format!("openSession: failed to parse feature JSON: {e}"));
                return Err(JsError::new(&e.to_string()));
            }
        };
        let count = features.len();
        let inner = core_session::SessionHandle::open(features);
        log_info(&format!("openSession: indexed {count} features"));
        Ok(SessionHandle { inner })
    }

    /// Number of features indexed.
    #[wasm_bindgen(js_name = "featureCount")]
    pub fn feature_count(&self) -> usize {
        self.inner.feature_count()
    }

    /// Projection metadata for the temporary editing plane used by the
    /// session's R-tree and exact metric hit tests. Feature payloads remain
    /// lon/lat; this describes the internal x/y copy.
    #[wasm_bindgen(js_name = "projectionMetadata")]
    pub fn projection_metadata(&self) -> Result<String, JsError> {
        stringify_json(&self.inner.projection_metadata())
    }

    /// Project one WGS84 lon/lat coordinate into the session's temporary
    /// metric edit plane. This is display/API plumbing only; the session
    /// remains the owner of the projection math.
    #[wasm_bindgen(js_name = "projectPoint")]
    pub fn project_point(&self, lng: f64, lat: f64) -> Vec<f64> {
        self.inner
            .project_coord(lng, lat)
            .map(|[x, y]| vec![x, y])
            .unwrap_or_default()
    }

    /// Mount or replace one scoped edit session inside this tab-level
    /// handle. Input feature ids are `layer/feature`; Rust prefixes them
    /// as `scope:layer/feature` internally.
    #[wasm_bindgen(js_name = "mountScope")]
    pub fn mount_scope(&mut self, scope: &str, features_json: &str) -> Result<usize, JsError> {
        let features: Vec<core_session::Feature> = parse_json(features_json)?;
        let count = self
            .inner
            .mount_scope(scope, features)
            .map_err(|err| JsError::new(&err))?;
        log_info(&format!("mountScope({scope}): indexed {count} features"));
        Ok(count)
    }

    /// Mount or replace one scoped edit session from raw GeoJSON layer
    /// envelopes. Rust owns feature id extraction, geometry interpretation,
    /// unsupported geometry filtering, and spatial/DataFrame indexing.
    #[wasm_bindgen(js_name = "mountScopeLayers")]
    pub fn mount_scope_layers(&mut self, scope: &str, layers_json: &str) -> Result<usize, JsError> {
        let layers: Vec<core_session::SessionLayerInput> = parse_json(layers_json)?;
        let count = self
            .inner
            .mount_scope_layers(scope, layers)
            .map_err(|err| JsError::new(&err))?;
        log_info(&format!(
            "mountScopeLayers({scope}): indexed {count} features"
        ));
        Ok(count)
    }

    /// Mount or replace one scoped edit session with an explicit projection
    /// policy. `projection_json` is `ProjectionConfig`; `{mode:"auto"}` uses
    /// Rwanda NIX/ITRF2005 and non-Rwanda UTM auto-zone defaults.
    #[wasm_bindgen(js_name = "mountScopeWithProjection")]
    pub fn mount_scope_with_projection(
        &mut self,
        scope: &str,
        features_json: &str,
        projection_json: &str,
    ) -> Result<usize, JsError> {
        let features: Vec<core_session::Feature> = parse_json(features_json)?;
        let projection_config: ds_design_session::projection::ProjectionConfig =
            parse_json(projection_json)?;
        let count = self
            .inner
            .mount_scope_with_projection(scope, features, projection_config)
            .map_err(|err| JsError::new(&err))?;
        log_info(&format!(
            "mountScopeWithProjection({scope}, mode={:?}): indexed {count} features",
            projection_config.mode
        ));
        Ok(count)
    }

    /// Mount raw GeoJSON layers with an explicit projection policy.
    #[wasm_bindgen(js_name = "mountScopeLayersWithProjection")]
    pub fn mount_scope_layers_with_projection(
        &mut self,
        scope: &str,
        layers_json: &str,
        projection_json: &str,
    ) -> Result<usize, JsError> {
        let layers: Vec<core_session::SessionLayerInput> = parse_json(layers_json)?;
        let projection_config: ds_design_session::projection::ProjectionConfig =
            parse_json(projection_json)?;
        let count = self
            .inner
            .mount_scope_layers_with_projection(scope, layers, projection_config)
            .map_err(|err| JsError::new(&err))?;
        log_info(&format!(
            "mountScopeLayersWithProjection({scope}, mode={:?}): indexed {count} features",
            projection_config.mode
        ));
        Ok(count)
    }

    /// Engine-owned wake/readiness state for one mounted edit scope.
    /// ds-web consumes this after `mountScope*` instead of inferring readiness
    /// from TypeScript object state.
    #[wasm_bindgen(js_name = "scopeStatusJson")]
    pub fn scope_status_json(&self, scope: &str) -> Result<String, JsError> {
        let status = self
            .inner
            .scope_status(scope)
            .map_err(|err| JsError::new(&err))?;
        stringify_json(&status)
    }

    /// Unmount one scoped edit session from this tab-level handle.
    #[wasm_bindgen(js_name = "unmountScope")]
    pub fn unmount_scope(&mut self, scope: &str) -> Result<usize, JsError> {
        let count = self
            .inner
            .unmount_scope(scope)
            .map_err(|err| JsError::new(&err))?;
        log_info(&format!("unmountScope({scope}): removed {count} features"));
        Ok(count)
    }

    /// Pick the nearest feature within `tolerance_m` of the click. Returns
    /// the feature id or `null` (`None`) if nothing is close enough.
    #[wasm_bindgen]
    pub fn pick(&self, lng: f64, lat: f64, tolerance_m: f64) -> Option<String> {
        let hit = self
            .inner
            .pick(lng, lat, tolerance_m)
            .map(|id| id.0.clone());
        match &hit {
            Some(id) => log_info(&format!(
                "pick(pos#{}, tol={tolerance_m}m) → {}",
                log_coord(lng, lat),
                log_id(id)
            )),
            None => log_info(&format!(
                "pick(pos#{}, tol={tolerance_m}m) → none",
                log_coord(lng, lat)
            )),
        }
        hit
    }

    /// Return every feature id within `radius_m` of `(lng, lat)`,
    /// nearest-first. Drives snap candidate selection — instead of
    /// walking every feature in `session.gdfs` looking for vertex /
    /// edge / midpoint snaps, the snap engine asks the index for the
    /// (small) candidate set first.
    #[wasm_bindgen(js_name = "pickRadius")]
    pub fn pick_radius(&self, lng: f64, lat: f64, radius_m: f64) -> Vec<String> {
        let hits = self.inner.pick_radius(lng, lat, radius_m);
        log_info(&format!(
            "pickRadius(pos#{}, r={radius_m}m) → {} hit(s)",
            log_coord(lng, lat),
            hits.len()
        ));
        hits.into_iter().map(|id| id.0).collect()
    }

    /// Query the Rust-owned snap evaluator. The input is a JSON
    /// `SnapQuery`; the return is a JSON `SnapResult` string or `null`.
    #[wasm_bindgen(js_name = "snapJson")]
    pub fn snap_json(&self, query_json: &str) -> Result<Option<String>, JsError> {
        let query: core_session::SnapQuery = parse_json(query_json)?;
        let hit = self.inner.snap(query);
        let message = match &hit {
            Some(result) => {
                let feature_id = result
                    .feature_id
                    .as_ref()
                    .map(|id| id.0.as_str())
                    .unwrap_or("self");
                format!(
                    "snapJson → {:?} {} ({:.2}m)",
                    result.target_kind,
                    if feature_id == "self" {
                        "self".to_string()
                    } else {
                        log_id(feature_id)
                    },
                    result.distance_m
                )
            }
            None => "snapJson → none".to_string(),
        };
        log_info(&message);
        match hit {
            Some(result) => Ok(Some(stringify_json(&result)?)),
            None => Ok(None),
        }
    }

    /// Query the Rust-owned hover evaluator. The input is a JSON
    /// `HoverQuery`; the return is a JSON `HoverResult` string or `null`.
    #[wasm_bindgen(js_name = "hoverJson")]
    pub fn hover_json(&self, query_json: &str) -> Result<Option<String>, JsError> {
        let query: core_session::HoverQuery = parse_json(query_json)?;
        let hit = self.inner.hover_at(query);
        let (log_key, message) = match &hit {
            Some(result) => (
                format!("{:?}:{}", result.target_kind, log_id(&result.feature_id.0)),
                format!(
                    "hoverJson → {:?} {} ({:.2}m)",
                    result.target_kind,
                    log_id(&result.feature_id.0),
                    result.distance_m
                ),
            ),
            None => ("none".to_string(), "hoverJson → none".to_string()),
        };
        log_hover_info(log_key, message);
        match hit {
            Some(result) => Ok(Some(stringify_json(&result)?)),
            None => Ok(None),
        }
    }

    // ── Selection state (Phase B) ──────────────────────────────────
    //
    // Wire shape (`SelectionState` JSON):
    //   { "selected": ["lv_lines/abc"], "hovered": "lv_poles/xyz", "last_edited": null }
    //
    // ds-web mirrors this into a Svelte store on every state-changing
    // call; never writes to the fields directly.

    /// Read the current selection record.
    #[wasm_bindgen(js_name = "selection")]
    pub fn selection_state(&self) -> Result<String, JsError> {
        stringify_json(self.inner.selection())
    }

    /// Mutate the selected set. `mode` is one of `"replace"` / `"add"`
    /// / `"toggle"` (snake_case to match the Rust enum's serde repr).
    /// Returns the new `SelectionState`.
    #[wasm_bindgen(js_name = "select")]
    pub fn select_features(&mut self, ids_json: &str, mode_json: &str) -> Result<String, JsError> {
        let ids: Vec<String> = parse_json(ids_json)?;
        let mode: core_session::SelectionMode = parse_json(mode_json)?;
        let feature_ids = ids.into_iter().map(core_session::FeatureId).collect();
        let next = self.inner.select(feature_ids, mode);
        stringify_json(&next)
    }

    /// Select every feature whose indexed geometry intersects a selector
    /// polygon. Input is `core_session::PolygonSelectionQuery`; output is the
    /// same `SelectionState` JSON shape as `select`.
    #[wasm_bindgen(js_name = "selectByPolygonJson")]
    pub fn select_by_polygon_json(&mut self, query_json: &str) -> Result<String, JsError> {
        let query: core_session::PolygonSelectionQuery = parse_json(query_json)?;
        let target_count = query.target_layers.len();
        let next = self
            .inner
            .select_by_polygon(query)
            .map_err(|err| JsError::new(&err))?;
        log_info(&format!(
            "selectByPolygonJson(target_layers={target_count}) → {} selected",
            next.selected.len()
        ));
        stringify_json(&next)
    }

    /// Set the hovered feature. Pass `null` (literal JSON) to clear.
    #[wasm_bindgen(js_name = "hover")]
    pub fn hover_feature(&mut self, id_json: &str) -> Result<String, JsError> {
        let id: Option<String> = parse_json(id_json)?;
        let next = self.inner.hover(id.map(core_session::FeatureId));
        stringify_json(&next)
    }

    /// Stamp `last_edited`. Operations.* fires this so the UI knows
    /// which row to focus after a split / edit / create.
    #[wasm_bindgen(js_name = "markEdited")]
    pub fn mark_edited(&mut self, id: &str) -> Result<String, JsError> {
        let next = self
            .inner
            .mark_edited(core_session::FeatureId(id.to_string()));
        stringify_json(&next)
    }

    /// Clear `selected` + `hovered` + `last_edited`.
    #[wasm_bindgen(js_name = "clearSelection")]
    pub fn clear_selection(&mut self) -> Result<String, JsError> {
        let next = self.inner.clear_selection();
        stringify_json(&next)
    }

    /// Retired: read `getRowWindow`. A page built before the row-window
    /// exchange still finds this export at start, so it boots; the call
    /// answers the typed `ui_unit_update_required` refusal (no Arrow).
    #[wasm_bindgen(js_name = "getRows")]
    pub fn get_rows_retired(
        &self,
        _layer_name: &str,
        _offset: usize,
        _limit: usize,
    ) -> Result<Vec<u8>, JsError> {
        Err(JsError::new(&crate::table_query::retired_page_export(
            "SessionHandle.getRows",
        )))
    }

    /// Return one layer's bounded row window, or `undefined` when the layer
    /// has no session features. (It replaced `getRows`, the same window as
    /// Arrow IPC bytes, on 24 Oct 2026: the Kernel UI links no Arrow codec.)
    #[wasm_bindgen(js_name = "getRowWindow")]
    pub fn get_row_window(
        &self,
        layer_name: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Option<tsify::Ts<ds_design_session::dataframe::RowWindow>>, JsError> {
        let window = self
            .inner
            .get_rows(layer_name, offset, limit)
            .map_err(|err| JsError::new(&err))?;
        let Some(window) = window else {
            return Ok(None);
        };
        let text = row_window_json(&window).map_err(|err| JsError::new(&err))?;
        js_sys::JSON::parse(&text)
            .map(|value| Some(tsify::Ts::new_unchecked(value)))
            .map_err(|_| JsError::new("design_session_response_invalid: row window is not JSON"))
    }

    /// The same window as the Network unit's `tableChunk` request (GeoJSON
    /// features in row order with the session's ids, and their columns), or
    /// `undefined` when the layer has no session features. The unit encodes
    /// the IPC chunk its table operations read.
    #[wasm_bindgen(js_name = "getTableChunkRequest")]
    pub fn get_table_chunk_request(
        &self,
        layer_name: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Option<String>, JsError> {
        let window = self
            .inner
            .get_rows(layer_name, offset, limit)
            .map_err(|err| JsError::new(&err))?;
        window
            .map(|window| row_window_chunk_request(window).map_err(|err| JsError::new(&err)))
            .transpose()
    }

    /// Phase C read probe. Returns one row as JSON, or JSON `null` when
    /// either the layer or feature id is absent.
    #[wasm_bindgen(js_name = "getRowJson")]
    pub fn get_row_json(&self, layer_name: &str, feature_id: &str) -> Result<String, JsError> {
        let row = self
            .inner
            .get_row(layer_name, feature_id)
            .map_err(|err| JsError::new(&err))?;
        stringify_json(&row)
    }

    /// Update one property cell in the session-owned DataFrame and mirror
    /// the value into the session feature map. Returns a JSON diff, or
    /// JSON `null` when the feature is absent.
    #[wasm_bindgen(js_name = "editCellJson")]
    pub fn edit_cell_json(
        &mut self,
        layer_name: &str,
        feature_id: &str,
        column: &str,
        value_json: &str,
    ) -> Result<String, JsError> {
        let value: serde_json::Value = parse_json(value_json)?;
        let diff = self
            .inner
            .edit_cell(layer_name, feature_id, column, value)
            .map_err(|err| JsError::new(&err))?;
        stringify_json(&diff)
    }

    /// Update many property cells in one session-owned DataFrame transaction
    /// and mirror the values into the session feature map. Returns JSON
    /// cell-level diffs plus any missing feature ids.
    #[wasm_bindgen(js_name = "editCellsJson")]
    pub fn edit_cells_json(
        &mut self,
        layer_name: &str,
        feature_ids_json: &str,
        values_json: &str,
    ) -> Result<String, JsError> {
        let feature_ids: Vec<String> = parse_json(feature_ids_json)?;
        let values: Map<String, Value> = parse_json(values_json)?;
        let result = self
            .inner
            .edit_cells(layer_name, &feature_ids, values)
            .map_err(|err| JsError::new(&err))?;
        stringify_json(&result)
    }

    /// Delete one property column from every row in a layer's
    /// session-owned DataFrame and mirror that removal into the session
    /// feature map. Returns the number of rows changed.
    #[wasm_bindgen(js_name = "deleteColumn")]
    pub fn delete_column(&mut self, layer_name: &str, column: &str) -> Result<usize, JsError> {
        self.inner
            .delete_column(layer_name, column)
            .map_err(|err| JsError::new(&err))
    }

    /// Insert a new feature. The id is `{layerName}/{featureId}`; the
    /// caller mints fresh ids and never inserts over an existing one.
    /// Returns `JsError` on id collision so the contract violation is
    /// surfaced rather than silently swallowed.
    #[wasm_bindgen(js_name = "insertFeature")]
    pub fn insert_feature(&mut self, feature_json: &str) -> Result<(), JsError> {
        let feat: core_session::Feature = match serde_json::from_str(feature_json) {
            Ok(v) => v,
            Err(e) => {
                log_warn(&format!("insertFeature: failed to parse feature JSON: {e}"));
                return Err(JsError::new(&e.to_string()));
            }
        };
        let id = feat.id.0.clone();
        self.inner.insert(feat).map_err(|err| {
            log_warn(&format!("insertFeature({id}) failed: {err}"));
            JsError::new(&err)
        })?;
        log_info(&format!("insertFeature({id}) ok"));
        Ok(())
    }

    /// Insert a GeoJSON Feature into one layer. Rust extracts id + geometry;
    /// ds-web passes the feature payload and does not classify geometry first.
    #[wasm_bindgen(js_name = "insertGeoJsonFeature")]
    pub fn insert_geojson_feature(
        &mut self,
        layer_name: &str,
        feature_json: &str,
    ) -> Result<bool, JsError> {
        let feature: serde_json::Value = parse_json(feature_json)?;
        let Some(feat) = core_session::session_feature_from_geojson(layer_name, feature)
            .map_err(|err| JsError::new(&err))?
        else {
            log_warn(&format!(
                "insertGeoJsonFeature({layer_name}) skipped: missing id or unsupported geometry"
            ));
            return Ok(false);
        };
        let id = feat.id.0.clone();
        self.inner.insert(feat).map_err(|err| {
            log_warn(&format!("insertGeoJsonFeature({id}) failed: {err}"));
            JsError::new(&err)
        })?;
        log_info(&format!("insertGeoJsonFeature({id}) ok"));
        Ok(true)
    }

    /// Create a GeoJSON feature in one layer, with Rust minting the feature id.
    /// Returns the applied `SessionDiff` JSON, or `null` when the geometry is
    /// unsupported.
    #[wasm_bindgen(js_name = "createGeoJsonFeature")]
    pub fn create_geojson_feature(
        &mut self,
        layer_name: &str,
        geometry_json: &str,
        properties_json: &str,
    ) -> Result<Option<String>, JsError> {
        let geometry: serde_json::Value = parse_json(geometry_json)?;
        let properties_value: serde_json::Value = parse_json(properties_json)?;
        let properties = match properties_value {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        let diff = self
            .inner
            .create_geojson_feature(layer_name, geometry, properties)
            .map_err(|err| {
                log_warn(&format!("createGeoJsonFeature({layer_name}) failed: {err}"));
                JsError::new(&err)
            })?;
        log_info(&format!(
            "createGeoJsonFeature({layer_name}) → {}",
            if diff.is_some() { "created" } else { "skipped" }
        ));
        stringify_optional_diff(diff)
    }

    /// Replace a feature's geometry under the same id. Returns `true` if
    /// a feature was replaced, `false` if the id was absent (the new
    /// feature IS still inserted in that case — same effect as
    /// `insertFeature`, no surprise to a caller who asked for "make
    /// this be the geometry now").
    #[wasm_bindgen(js_name = "replaceFeature")]
    pub fn replace_feature(&mut self, feature_json: &str) -> Result<bool, JsError> {
        let feat: core_session::Feature = match serde_json::from_str(feature_json) {
            Ok(v) => v,
            Err(e) => {
                log_warn(&format!(
                    "replaceFeature: failed to parse feature JSON: {e}"
                ));
                return Err(JsError::new(&e.to_string()));
            }
        };
        let id = feat.id.0.clone();
        let replaced = self.inner.replace(feat).map_err(|err| {
            log_warn(&format!("replaceFeature({id}) failed: {err}"));
            JsError::new(&err)
        })?;
        log_info(&format!(
            "replaceFeature({id}) → {}",
            if replaced.is_some() {
                "replaced"
            } else {
                "inserted"
            }
        ));
        Ok(replaced.is_some())
    }

    /// Replace a GeoJSON Feature in one layer. Returns `true` when an
    /// existing feature was replaced, `false` when the id was absent or
    /// the GeoJSON payload was not indexable.
    #[wasm_bindgen(js_name = "replaceGeoJsonFeature")]
    pub fn replace_geojson_feature(
        &mut self,
        layer_name: &str,
        feature_json: &str,
    ) -> Result<bool, JsError> {
        let feature: serde_json::Value = parse_json(feature_json)?;
        let Some(feat) = core_session::session_feature_from_geojson(layer_name, feature)
            .map_err(|err| JsError::new(&err))?
        else {
            log_warn(&format!(
                "replaceGeoJsonFeature({layer_name}) skipped: missing id or unsupported geometry"
            ));
            return Ok(false);
        };
        let id = feat.id.0.clone();
        let replaced = self.inner.replace(feat).map_err(|err| {
            log_warn(&format!("replaceGeoJsonFeature({id}) failed: {err}"));
            JsError::new(&err)
        })?;
        log_info(&format!(
            "replaceGeoJsonFeature({id}) → {}",
            if replaced.is_some() {
                "replaced"
            } else {
                "inserted"
            }
        ));
        Ok(replaced.is_some())
    }

    /// Replace a GeoJSON Feature that must already exist in one layer. Returns
    /// the Rust-owned diff, or null when the feature is missing / not indexable.
    #[wasm_bindgen(js_name = "replaceGeoJsonFeatureDiff")]
    pub fn replace_geojson_feature_diff(
        &mut self,
        layer_name: &str,
        feature_json: &str,
    ) -> Result<Option<String>, JsError> {
        let feature: serde_json::Value = parse_json(feature_json)?;
        let diff = self
            .inner
            .replace_existing_geojson_feature(layer_name, feature)
            .map_err(|err| {
                log_warn(&format!(
                    "replaceGeoJsonFeatureDiff({layer_name}) failed: {err}"
                ));
                JsError::new(&err)
            })?;
        log_info(&format!(
            "replaceGeoJsonFeatureDiff({layer_name}) → {}",
            if diff.is_some() {
                "replaced"
            } else {
                "not present"
            }
        ));
        stringify_optional_diff(diff)
    }

    /// Remove a feature by id. Returns `true` when the id was present,
    /// `false` when it wasn't (idempotent — JS callers can fire-and-
    /// forget on uncertain branches).
    #[wasm_bindgen(js_name = "removeFeature")]
    pub fn remove_feature(&mut self, id: &str) -> Result<bool, JsError> {
        let key = core_session::FeatureId::new(id);
        let removed = self.inner.remove(&key).map_err(|err| {
            log_warn(&format!("removeFeature({id}) failed: {err}"));
            JsError::new(&err)
        })?;
        log_info(&format!(
            "removeFeature({id}) → {}",
            if removed.is_some() {
                "removed"
            } else {
                "not present"
            }
        ));
        Ok(removed.is_some())
    }

    /// Remove a feature and return the applied `SessionDiff` JSON.
    #[wasm_bindgen(js_name = "removeFeatureDiff")]
    pub fn remove_feature_diff(&mut self, id: &str) -> Result<Option<String>, JsError> {
        let key = core_session::FeatureId::new(id);
        let diff = self.inner.remove_diff(&key).map_err(|err| {
            log_warn(&format!("removeFeatureDiff({id}) failed: {err}"));
            JsError::new(&err)
        })?;
        log_info(&format!(
            "removeFeatureDiff({id}) → {}",
            if diff.is_some() {
                "removed"
            } else {
                "not present"
            }
        ));
        stringify_optional_diff(diff)
    }

    /// Remove multiple feature ids from one layer as a single Rust mutation.
    #[wasm_bindgen(js_name = "removeFeatures")]
    pub fn remove_features(
        &mut self,
        layer_name: &str,
        feature_ids_json: &str,
    ) -> Result<Option<String>, JsError> {
        let feature_ids: Vec<String> = parse_json(feature_ids_json)?;
        let diff = self
            .inner
            .remove_features(layer_name, feature_ids)
            .map_err(|err| {
                log_warn(&format!("removeFeatures({layer_name}) failed: {err}"));
                JsError::new(&err)
            })?;
        stringify_optional_diff(diff)
    }

    /// Remove all indexed features from one layer as a single Rust mutation.
    #[wasm_bindgen(js_name = "removeLayer")]
    pub fn remove_layer(&mut self, layer_name: &str) -> Result<Option<String>, JsError> {
        let diff = self.inner.remove_layer(layer_name).map_err(|err| {
            log_warn(&format!("removeLayer({layer_name}) failed: {err}"));
            JsError::new(&err)
        })?;
        stringify_optional_diff(diff)
    }

    /// Replace a full layer from raw GeoJSON Feature JSON. Rust owns identity
    /// normalization, DataFrame rebuild, spatial index rebuild, and undo diff.
    #[wasm_bindgen(js_name = "replaceLayerGeoJsonFeatures")]
    pub fn replace_layer_geojson_features(
        &mut self,
        layer_name: &str,
        features_json: &str,
    ) -> Result<Option<String>, JsError> {
        let features: Vec<serde_json::Value> = parse_json(features_json)?;
        let diff = self
            .inner
            .replace_layer_geojson_features(layer_name, features)
            .map_err(|err| {
                log_warn(&format!(
                    "replaceLayerGeoJsonFeatures({layer_name}) failed: {err}"
                ));
                JsError::new(&err)
            })?;
        log_info(&format!(
            "replaceLayerGeoJsonFeatures({layer_name}) → {} change(s)",
            diff.as_ref().map(|diff| diff.changes.len()).unwrap_or(0)
        ));
        stringify_optional_diff(diff)
    }

    #[wasm_bindgen(js_name = "insertVertex")]
    pub fn insert_vertex(
        &mut self,
        id: &str,
        ring_index: usize,
        segment_index: usize,
        lng: f64,
        lat: f64,
    ) -> Result<Option<String>, JsError> {
        let diff = self
            .inner
            .insert_vertex(
                &core_session::FeatureId::new(id),
                ring_index,
                segment_index,
                [lng, lat],
            )
            .map_err(|err| JsError::new(&err))?;
        stringify_optional_diff(diff)
    }

    #[wasm_bindgen(js_name = "moveVertex")]
    pub fn move_vertex(
        &mut self,
        id: &str,
        ring_index: usize,
        vertex_index: usize,
        lng: f64,
        lat: f64,
    ) -> Result<Option<String>, JsError> {
        let diff = self
            .inner
            .move_vertex(
                &core_session::FeatureId::new(id),
                ring_index,
                vertex_index,
                [lng, lat],
            )
            .map_err(|err| JsError::new(&err))?;
        stringify_optional_diff(diff)
    }

    #[wasm_bindgen(js_name = "deleteVertex")]
    pub fn delete_vertex(
        &mut self,
        id: &str,
        ring_index: usize,
        vertex_index: usize,
    ) -> Result<Option<String>, JsError> {
        let diff = self
            .inner
            .delete_vertex(&core_session::FeatureId::new(id), ring_index, vertex_index)
            .map_err(|err| JsError::new(&err))?;
        stringify_optional_diff(diff)
    }

    #[wasm_bindgen(js_name = "moveFeature")]
    pub fn move_feature(
        &mut self,
        id: &str,
        from_lng: f64,
        from_lat: f64,
        to_lng: f64,
        to_lat: f64,
    ) -> Result<Option<String>, JsError> {
        let diff = self
            .inner
            .move_feature(
                &core_session::FeatureId::new(id),
                [from_lng, from_lat],
                [to_lng, to_lat],
            )
            .map_err(|err| JsError::new(&err))?;
        stringify_optional_diff(diff)
    }

    #[wasm_bindgen(js_name = "extendLineTo")]
    pub fn extend_line_to(
        &mut self,
        source_id: &str,
        target_id: &str,
        intent_lng: f64,
        intent_lat: f64,
    ) -> Result<Option<String>, JsError> {
        let intent =
            (intent_lng.is_finite() && intent_lat.is_finite()).then_some([intent_lng, intent_lat]);
        let diff = self
            .inner
            .extend_line_to(
                &core_session::FeatureId::new(source_id),
                &core_session::FeatureId::new(target_id),
                intent,
            )
            .map_err(|err| JsError::new(&err))?;
        stringify_optional_diff(diff)
    }

    #[wasm_bindgen(js_name = "joinLines")]
    pub fn join_lines(
        &mut self,
        source_id: &str,
        target_id: &str,
        tolerance_m: f64,
    ) -> Result<Option<String>, JsError> {
        let diff = self
            .inner
            .join_lines(
                &core_session::FeatureId::new(source_id),
                &core_session::FeatureId::new(target_id),
                tolerance_m,
            )
            .map_err(|err| JsError::new(&err))?;
        stringify_optional_diff(diff)
    }

    #[wasm_bindgen(js_name = "splitLine")]
    pub fn split_line(
        &mut self,
        layer_name: &str,
        feature_id: &str,
        segments_json: &str,
    ) -> Result<Option<String>, JsError> {
        let segments: Vec<Vec<[f64; 2]>> = parse_json(segments_json)?;
        let diff = self
            .inner
            .split_line(layer_name, feature_id, segments)
            .map_err(|err| JsError::new(&err))?;
        stringify_optional_diff(diff)
    }

    #[wasm_bindgen(js_name = "canUndo")]
    pub fn can_undo(&self) -> bool {
        self.inner.can_undo()
    }

    #[wasm_bindgen(js_name = "canRedo")]
    pub fn can_redo(&self) -> bool {
        self.inner.can_redo()
    }

    #[wasm_bindgen(js_name = "undo")]
    pub fn undo(&mut self) -> Result<Option<String>, JsError> {
        let diff = self.inner.undo().map_err(|err| JsError::new(&err))?;
        stringify_optional_diff(diff)
    }

    #[wasm_bindgen(js_name = "redo")]
    pub fn redo(&mut self) -> Result<Option<String>, JsError> {
        let diff = self.inner.redo().map_err(|err| JsError::new(&err))?;
        stringify_optional_diff(diff)
    }

    #[wasm_bindgen(js_name = "commit")]
    pub fn commit(&mut self) {
        self.inner.commit();
    }
}

fn stringify_optional_diff(
    diff: Option<ds_design_session::mutation::SessionDiff>,
) -> Result<Option<String>, JsError> {
    match diff {
        Some(diff) => Ok(Some(
            serde_json::to_string(&diff).map_err(|e| JsError::new(&e.to_string()))?,
        )),
        None => Ok(None),
    }
}

/// Typed admission of the JSON replies above; one decoder per reply family.
#[cfg(target_arch = "wasm32")]
#[path = "design_session_reply.rs"]
mod reply;

#[cfg(test)]
#[path = "design_session_tests.rs"]
mod tests;
