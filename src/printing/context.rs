//! What a print context layer *is*, which ones a new template starts with,
//! and whether a catalogue layer may be selected — decided once.
//!
//! The Printing setup page authored these as literals: the three derived
//! sources with their buffers, thresholds and intervals, complete physical
//! print styles for the layers those sources expand into, the rule
//! that a new project template starts with every ready or downloadable
//! catalogue layer except parcels, and the rule that a layer with no dataset
//! cannot be switched on. The kernel already bounded every parameter; now it
//! also owns the product values. Hosts pass catalogue facts and receive rows
//! and documents; they fetch, download and paint.
//!
//! `retained_admission` is the last decision: whether a derived context a
//! computer retained from an earlier preparation (elevation contours, the
//! project's MV models) may be reused for a print over a given extent. A
//! preview never acquires or projects context; what is not retained is a
//! named omission.

use super::preview::Refusal;
use super::{Layout, PrintContextLayer, PrintContextSource, PrintCorridor};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Catalogue layers a default selection never includes: cadastral parcels
/// clutter an engineering sheet at every scale a network is drawn at.
pub const DEFAULT_CATALOG_EXCLUSIONS: [&str; 1] = ["rwanda_upi_parcels"];

/// Rwanda's broad alternate representations. Prefer selected detailed classes
/// by default only when every listed class is held or downloadable. These are
/// presentation alternatives, not a claim that two sources have equal coverage.
const DETAILED_ALTERNATIVES: [(&str, &[&str]); 2] = [
    ("powerlines", &["hv_line", "mv_line", "lv_line"]),
    (
        "elementary_school",
        &["primary_schools", "secondary_schools"],
    ),
];

/// The detailed layers a retired broad representation answers by name with
/// (`docs/contracts/foundation-datasets.md` R4/R6): `powerlines` and
/// `elementary_school` were retired on 2026-09-18 and are never seeded again;
/// a request naming one is refused with these alternatives.
pub fn retired_alternatives(layer: &str) -> Option<&'static [&'static str]> {
    DETAILED_ALTERNATIVES
        .iter()
        .find(|(broad, _)| *broad == layer)
        .map(|(_, detailed)| *detailed)
}

fn default_omission(resource: &CatalogResource, resources: &[CatalogResource]) -> Option<String> {
    if DEFAULT_CATALOG_EXCLUSIONS.contains(&resource.layer.as_str()) {
        return Some("Cadastral parcels are not selected by default.".into());
    }
    if !resource.country.eq_ignore_ascii_case("Rwanda") {
        return None;
    }
    DETAILED_ALTERNATIVES.iter().find_map(|(broad, detailed)| {
        (*broad == resource.layer && detailed.iter().all(|id| resources.iter().any(|row|
            row.layer == *id && row.country.eq_ignore_ascii_case(&resource.country) && selectable(row)
        ))).then(|| format!("Detailed alternatives selected by default: {}. Enable this alternate representation explicitly if needed.", detailed.join(", ")))
    })
}

/// Retained coverage around every printed feature, in metres, for the
/// derived (per-transformer prepared) context sources.
pub const DERIVED_BUFFER_M: f64 = 200.;

/// Default print-only metric corridor; independent of retained coverage.
pub fn default_corridor() -> PrintCorridor {
    PrintCorridor {
        distance_m: 6.,
        color: None,
        width_mm: None,
        opacity: None,
        dash_mm: None,
    }
}

/// Context sources prepared per transformer by a new template.
pub fn derived_layers() -> Vec<PrintContextLayer> {
    vec![
        PrintContextLayer {
            id: "project_dsgrid_mv".into(),
            label: "MV · project DS Grid models".into(),
            source: PrintContextSource::ProjectDsgridMv {
                buffer_m: DERIVED_BUFFER_M,
                corridor: Some(default_corridor()),
            },
        },
        PrintContextLayer {
            id: "buildings_context".into(),
            label: "Building footprints".into(),
            source: PrintContextSource::GoogleOpenBuildings {
                buffer_m: DERIVED_BUFFER_M,
                min_area_m2: 25.,
                min_confidence: 0.7,
            },
        },
        PrintContextLayer {
            id: "elevation_contours".into(),
            label: "Elevation contours · 10 m / 50 m".into(),
            source: PrintContextSource::ElevationContours {
                buffer_m: DERIVED_BUFFER_M,
                minor_interval_m: 10,
                index_interval_m: 50,
                sample_spacing_m: 10.,
            },
        },
    ]
}

/// Semantic identity of a held context representation; the API resolver alone
/// chooses the governed document and revision. Acquisition defaults contain no ink.
pub fn style_key(
    entity_class: &str,
    source_kind: &str,
    role: &str,
) -> crate::style_resolution::Key {
    crate::style_resolution::Key {
        entity_class: entity_class.into(),
        source_kind: source_kind.into(),
        target: crate::style_resolution::Target::Print,
        role: role.into(),
        ink: Default::default(),
    }
}

pub fn derived_style_keys() -> BTreeMap<String, crate::style_resolution::Key> {
    let mut keys = BTreeMap::new();
    for layer in derived_layers() {
        for id in expanded_layer_ids(&layer) {
            let key = match layer.source {
                PrintContextSource::ProjectDsgridMv { .. } => {
                    style_key(&id, "project_model", "mv_context")
                }
                _ => style_key(&id, "derived_geojson", "context"),
            };
            keys.insert(id, key);
        }
    }
    keys
}

/// Retain explicit authored pens/keys; new choices are semantic keys, never pens.
pub fn bind_style_keys(layout: &mut Layout, layer: &PrintContextLayer) {
    for id in expanded_layer_ids(layer) {
        if layout.styles.contains_key(&id)
            || layout.style_refs.contains_key(&id)
            || layout.style_keys.contains_key(&id)
        {
            continue;
        }
        let key = match &layer.source {
            PrintContextSource::ProjectDsgridMv { .. } => {
                style_key(&id, "project_model", "mv_context")
            }
            PrintContextSource::Catalog { layer, .. } => {
                style_key(layer, "cold_existing", "context")
            }
            PrintContextSource::LocalLayer { .. } => style_key(&id, "live_geojson", "context"),
            PrintContextSource::GoogleOpenBuildings { .. } => {
                style_key("buildings_context", "derived_geojson", "context")
            }
            _ => style_key(&id, "derived_geojson", "context"),
        };
        layout.style_keys.insert(id, key);
    }
}

/// The print layer ids one context source expands into. A source that
/// prints as one layer expands to its own id.
pub fn expanded_layer_ids(layer: &PrintContextLayer) -> Vec<String> {
    match layer.source {
        PrintContextSource::ElevationContours { .. } => {
            vec!["contours_intermediate".into(), "contours_index".into()]
        }
        PrintContextSource::ProjectDsgridMv { ref corridor, .. } => {
            let mut ids = super::map::MV_PRINT_LAYER_IDS
                .iter()
                .map(|id| (*id).to_owned())
                .collect::<Vec<_>>();
            if corridor.is_some() {
                ids.push("dsgrid_mv_corridor".into());
            }
            ids
        }
        _ => vec![layer.id.clone()],
    }
}

/// Whether a context source prints anything on this layout: at least one of
/// the layers it expands into is not hidden by the layout's own style
/// override.
///
/// A source can be selected (it is in `context_layers`) while every layer it
/// prints as is hidden (`style_overrides[..].visible == false`). Those are two
/// switches for one intent, and when they disagree the source is selected in
/// name only: acquiring it costs the provider and the sheet shows nothing,
/// while a receipt that counted it as printed would claim a deliverable that
/// is not on the page. Every decision about what to acquire and what to
/// report asks this one question.
pub fn prints_in(layout: &Layout, layer: &PrintContextLayer) -> bool {
    expanded_layer_ids(layer).iter().any(|id| {
        layout
            .style_overrides
            .get(id)
            .and_then(|style| style.visible)
            != Some(false)
    })
}

/// A context source the layout selects but hides entirely, named for the
/// reader: which source, and which of its layers the layout hides.
pub fn hidden_context(layout_id: &str, layer: &PrintContextLayer) -> Value {
    serde_json::json!({
        "layout": layout_id,
        "context": layer.id,
        "label": layer.label,
        "hidden_layers": expanded_layer_ids(layer),
        "note": format!(
            "{} is selected on layout {layout_id}, but the layout hides every layer it prints as ({}); it is not acquired and does not print. Switch it on again to show it.",
            layer.label,
            expanded_layer_ids(layer).join(", ")
        ),
    })
}

/// What the host knows about one catalogue dataset on this machine.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CatalogResource {
    pub layer: String,
    pub label: String,
    pub country: String,
    /// A local index of the dataset is installed and complete.
    #[serde(default)]
    pub ready: bool,
    /// A published bundle exists that printing can download when connected.
    #[serde(default)]
    pub downloadable: bool,
    /// The catalogue marks the dataset unavailable.
    #[serde(default)]
    pub unavailable: bool,
}

/// One catalogue dataset as a template may use it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CatalogRow {
    pub layer: PrintContextLayer,
    /// May be switched on: a dataset is held or can be fetched.
    pub selectable: bool,
    /// A new template starts with it on.
    pub default_on: bool,
    /// Why an available alternate does not start selected; it remains opt-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_omission: Option<String>,
    /// Why it cannot be switched on, when it cannot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
}

fn catalog_layer(resource: &CatalogResource) -> PrintContextLayer {
    PrintContextLayer {
        id: resource.layer.clone(),
        label: resource.label.clone(),
        source: PrintContextSource::Catalog {
            country: resource.country.clone(),
            layer: resource.layer.clone(),
        },
    }
}

fn selectable(resource: &CatalogResource) -> bool {
    !resource.unavailable && (resource.ready || resource.downloadable)
}

/// Every catalogue dataset as a selectable row, with the default decision.
pub fn catalog(resources: &[CatalogResource]) -> Result<Vec<CatalogRow>, String> {
    if resources.len() > 512 {
        return Err("more than 512 catalogue resources".into());
    }
    resources
        .iter()
        .map(|resource| {
            if !super::identifier(&resource.layer) || resource.label.trim().is_empty() {
                return Err("invalid catalogue resource identity".into());
            }
            let selectable = selectable(resource);
            let default_omission = default_omission(resource, resources);
            Ok(CatalogRow {
                layer: catalog_layer(resource),
                selectable,
                default_on: selectable && default_omission.is_none(),
                default_omission,
                refusal: (!selectable).then(|| "printing_context_no_dataset".to_owned()),
            })
        })
        .collect()
}

/// The selection a new project template starts with: every default-on
/// catalogue dataset, then the derived sources, with governed semantic keys bound
/// wherever the template has authored none. Existing choices are kept.
pub fn default_selection(
    mut layout: Layout,
    resources: &[CatalogResource],
) -> Result<Layout, String> {
    let rows = catalog(resources)?;
    let mut context_layers: Vec<PrintContextLayer> = rows
        .into_iter()
        .filter(|row| row.default_on)
        .map(|row| row.layer)
        .collect();
    context_layers.extend(derived_layers());
    for layer in &context_layers {
        bind_style_keys(&mut layout, layer);
    }
    layout.context_layers = context_layers;
    super::validate(&layout)?;
    Ok(layout)
}

/// Switch one context layer on or off. Enabling a catalogue layer needs its
/// dataset held or fetchable — unless it is already selected, in which case
/// the operator may always switch it off and on again without losing it.
/// Enabling a derived source binds governed keys for the layers it
/// prints as. Enabling also shows those layers: an explicit `visible: false`
/// the layout carried is cleared, because switching a source on means the
/// operator wants to see it — otherwise it is acquired and never drawn.
pub fn toggle(
    mut layout: Layout,
    layer: PrintContextLayer,
    enabled: bool,
    resource: Option<&CatalogResource>,
) -> Result<Layout, String> {
    let already = layout.context_layers.iter().any(|row| row.id == layer.id);
    if enabled && !already {
        if let Some(resource) = resource {
            if !selectable(resource) {
                return Err("printing_context_no_dataset".into());
            }
        }
    }
    layout.context_layers.retain(|row| row.id != layer.id);
    if enabled {
        bind_style_keys(&mut layout, &layer);
        for id in expanded_layer_ids(&layer) {
            if let Some(style) = layout.style_overrides.get_mut(&id) {
                if style.visible == Some(false) {
                    style.visible = None;
                }
            }
        }
        layout.context_layers.push(layer);
    }
    super::validate(&layout)?;
    Ok(layout)
}

/// The schema every retained derived context carries.
pub const RETAINED_SCHEMA: &str = "ds.transformer-print-context/v1";

/// One derived context as a computer retained it from an earlier
/// preparation, keyed by its source parameters (`transformer-print-context`
/// v2 entries). The host hands the entry it holds as data; whether it is the
/// one this print may reuse is decided here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetainedContext {
    #[serde(default)]
    pub schema: String,
    /// The source parameters the context was prepared with, in the canonical
    /// spelling of [`retained_parameters`].
    #[serde(default)]
    pub parameters: String,
    /// The buffered extent the context covers: `[west, south, east, north]`.
    #[serde(default)]
    pub coverage: [f64; 4],
    /// The prepared layers by expanded print layer id, each a
    /// `FeatureCollection`.
    #[serde(default)]
    pub layers: Value,
    /// The provider's version of what was prepared.
    #[serde(default, alias = "sourceVersion")]
    pub source_version: String,
    /// Warnings the preparation reported; reused with the layers.
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Whether a retained context may be reused for one print.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum RetainedVerdict {
    /// The retained layers print, with the warnings retained beside them.
    Use {
        layers: BTreeMap<String, Value>,
        warnings: Vec<String>,
    },
    /// The layer is named on the sheet as omitted, with why. Never an error:
    /// a derived context is optional map context, and a preview never
    /// acquires it.
    Omit { reason: String },
}

/// Whether the retained extent covers the wanted one, both
/// `[west, south, east, north]`.
pub fn covers(held: [f64; 4], wanted: [f64; 4]) -> bool {
    held[0] <= wanted[0] && held[1] <= wanted[1] && held[2] >= wanted[2] && held[3] >= wanted[3]
}

/// The canonical spelling of a source's parameters, as the retained entries
/// are keyed: the browser wrote `JSON.stringify(source, sortedKeys)`, which
/// is the RFC 8785 form of the source with one quirk — the sorted key list
/// is a replacer, so a nested object keeps only the keys the top level also
/// has (a `corridor` spells as `{}`). Entries already retained carry that
/// spelling, so it is the contract.
pub fn retained_parameters(source: &PrintContextSource) -> Result<String, String> {
    let value = serde_json::to_value(source).map_err(|e| e.to_string())?;
    let allowed: Vec<String> = value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    fn prune(value: &Value, allowed: &[String]) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .filter(|(key, _)| allowed.contains(key))
                    .map(|(key, child)| (key.clone(), prune(child, allowed)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(|v| prune(v, allowed)).collect()),
            other => other.clone(),
        }
    }
    let bytes = crate::report_export::jcs::canonicalize(&prune(&value, &allowed))?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// The document layer ids one context layer prints as: what its preparation
/// emits. The project MV projection prints as every layer `map::mv_layers`
/// writes — routes, structures and angle points; contours expand as the template does; any other source prints
/// as its own id. A retained preparation may carry only these ids, and a
/// host names an omission of the layer under them.
pub fn printed_layer_ids(layer: &PrintContextLayer) -> Vec<String> {
    match layer.source {
        PrintContextSource::ProjectDsgridMv { .. } => super::map::MV_PRINT_LAYER_IDS
            .iter()
            .map(|id| (*id).to_owned())
            .collect(),
        _ => expanded_layer_ids(layer),
    }
}

/// Whether the source is one a computer retains from a preparation (rather
/// than reads from a held room or a project holding).
fn retained_kind(source: &PrintContextSource) -> bool {
    matches!(
        source,
        PrintContextSource::ElevationContours { .. } | PrintContextSource::ProjectDsgridMv { .. }
    )
}

/// May `retained` stand in for `layer` over the `wanted` extent? Reused only
/// when it carries the retained schema, was prepared with exactly this
/// source's parameters, covers the wanted extent and names only layers the
/// source's preparation emits, each a `FeatureCollection`. Otherwise — including
/// nothing retained, or a source kind no computer retains — the layer is a
/// named omission.
pub fn retained_admission(
    layer: &PrintContextLayer,
    retained: Option<&RetainedContext>,
    wanted: [f64; 4],
) -> RetainedVerdict {
    let omit = |reason: &str| RetainedVerdict::Omit {
        reason: reason.into(),
    };
    if !retained_kind(&layer.source) {
        return omit("not a retained context kind");
    }
    let Some(retained) = retained else {
        return omit("not retained on this computer");
    };
    if retained.schema != RETAINED_SCHEMA {
        return omit("retained under another schema");
    }
    match retained_parameters(&layer.source) {
        Ok(parameters) if parameters == retained.parameters => {}
        _ => return omit("retained with other parameters"),
    }
    if !retained.coverage.iter().all(|v| v.is_finite()) || !covers(retained.coverage, wanted) {
        return omit("retained for a smaller extent");
    }
    let Some(entries) = retained.layers.as_object() else {
        return omit("retained layers are not a map");
    };
    let own = printed_layer_ids(layer);
    let mut layers = BTreeMap::new();
    for (id, collection) in entries {
        if !own.contains(id) {
            return omit("retained layers are not this source's");
        }
        if collection.get("type").and_then(Value::as_str) != Some("FeatureCollection")
            || !collection.get("features").is_some_and(Value::is_array)
        {
            return omit("retained layer is not a feature collection");
        }
        layers.insert(id.clone(), collection.clone());
    }
    RetainedVerdict::Use {
        layers,
        warnings: retained.warnings.clone(),
    }
}

/// One refusal a host may show for an omitted derived layer, keyed like the
/// other printing decisions: the reason travels as the `reason` parameter.
pub fn omission_refusal(layer: &PrintContextLayer, reason: &str) -> Refusal {
    super::preview::refusal(
        "print_context_omitted",
        "printing_context_omitted",
        &[("label", &layer.label), ("reason", reason)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printing::{default_layout, PrintStyle};

    fn resource(
        layer: &str,
        ready: bool,
        downloadable: bool,
        unavailable: bool,
    ) -> CatalogResource {
        CatalogResource {
            layer: layer.into(),
            label: layer.replace('_', " "),
            country: "Rwanda".into(),
            ready,
            downloadable,
            unavailable,
        }
    }

    #[test]
    fn the_derived_sources_carry_the_product_values_and_validate() {
        let layers = derived_layers();
        assert_eq!(layers.len(), 3);
        let mut layout = default_layout();
        layout.context_layers = layers.clone();
        super::super::validate(&layout).unwrap();
        match &layers[2].source {
            PrintContextSource::ElevationContours {
                minor_interval_m,
                index_interval_m,
                ..
            } => assert_eq!((*minor_interval_m, *index_interval_m), (10, 50)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn every_expanded_layer_of_a_derived_source_has_an_exact_key_without_pens() {
        let keys = derived_style_keys();
        for layer in derived_layers() {
            for id in expanded_layer_ids(&layer) {
                assert!(keys.contains_key(&id), "{id}");
                keys[&id].validate().unwrap();
                assert_eq!(keys[&id].target, crate::style_resolution::Target::Print);
            }
        }
        assert_eq!(keys.len(), 8);
        assert_eq!(keys["dsgrid_mv_lines"].source_kind, "project_model");
        assert_eq!(keys["contours_index"].source_kind, "derived_geojson");
        let value = serde_json::to_value(default_corridor()).unwrap();
        assert_eq!(value, serde_json::json!({"distance_m":6.}));
    }

    #[test]
    fn a_catalogue_row_is_selectable_when_held_or_fetchable_and_parcels_are_never_default() {
        let rows = catalog(&[
            resource("roads", true, false, false),
            resource("rivers", false, true, false),
            resource("wetlands", false, false, false),
            resource("districts", true, true, true),
            resource("rwanda_upi_parcels", true, true, false),
        ])
        .unwrap();
        let by = |id: &str| rows.iter().find(|r| r.layer.id == id).unwrap();
        assert!(by("roads").selectable && by("roads").default_on);
        assert!(by("rivers").selectable && by("rivers").default_on);
        assert!(!by("wetlands").selectable && !by("wetlands").default_on);
        assert_eq!(
            by("wetlands").refusal.as_deref(),
            Some("printing_context_no_dataset")
        );
        assert!(!by("districts").selectable);
        assert!(by("rwanda_upi_parcels").selectable && !by("rwanda_upi_parcels").default_on);
    }

    #[test]
    fn detailed_rwanda_alternatives_prevent_uninvited_broad_defaults_without_removing_fallbacks() {
        let ids = [
            "powerlines",
            "hv_line",
            "mv_line",
            "lv_line",
            "elementary_school",
            "primary_schools",
            "secondary_schools",
            "universities",
        ];
        let resources = ids.map(|id| resource(id, true, false, false));
        let rows = catalog(&resources).unwrap();
        for id in ["powerlines", "elementary_school"] {
            let row = rows.iter().find(|row| row.layer.id == id).unwrap();
            assert!(row.selectable && !row.default_on);
            assert!(row.default_omission.is_some());
        }
        assert!(rows
            .iter()
            .filter(|row| !["powerlines", "elementary_school"].contains(&row.layer.id.as_str()))
            .all(|row| row.default_on));
        let incomplete = resources
            .iter()
            .filter(|r| !["lv_line", "secondary_schools"].contains(&r.layer.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        assert!(catalog(&incomplete)
            .unwrap()
            .iter()
            .all(|row| row.default_on));
        let other_country = resources.map(|mut row| {
            row.country = "Other".into();
            row
        });
        assert!(catalog(&other_country)
            .unwrap()
            .iter()
            .all(|row| row.default_on));
    }

    #[test]
    fn the_default_selection_is_catalogue_defaults_then_derived_with_styles_seeded() {
        let mut layout = default_layout();
        layout.styles.insert(
            "contours_index".into(),
            PrintStyle {
                stroke: "#123456".into(),
                ..PrintStyle::default()
            },
        );
        let out = default_selection(
            layout,
            &[
                resource("roads", true, false, false),
                resource("rwanda_upi_parcels", true, true, false),
                resource("wetlands", false, false, false),
            ],
        )
        .unwrap();
        let ids: Vec<&str> = out.context_layers.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "roads",
                "project_dsgrid_mv",
                "buildings_context",
                "elevation_contours"
            ]
        );
        assert_eq!(
            out.styles["contours_index"].stroke, "#123456",
            "authored styles are kept"
        );
        assert_eq!(
            out.style_keys["dsgrid_mv_lines"].source_kind, "project_model",
            "missing choices are governed keys"
        );
    }

    #[test]
    fn toggling_refuses_a_dataset_that_cannot_be_fetched_unless_already_selected() {
        let layout = default_layout();
        let wetlands = resource("wetlands", false, false, false);
        let layer = PrintContextLayer {
            id: "wetlands".into(),
            label: "Wetlands".into(),
            source: PrintContextSource::Catalog {
                country: "Rwanda".into(),
                layer: "wetlands".into(),
            },
        };
        assert_eq!(
            toggle(layout.clone(), layer.clone(), true, Some(&wetlands)).unwrap_err(),
            "printing_context_no_dataset"
        );
        let mut selected = layout.clone();
        selected.context_layers.push(layer.clone());
        let off = toggle(selected.clone(), layer.clone(), false, Some(&wetlands)).unwrap();
        assert!(off.context_layers.is_empty());
        let on_again = toggle(selected, layer, true, Some(&wetlands)).unwrap();
        assert_eq!(
            on_again.context_layers.len(),
            1,
            "already selected stays togglable"
        );
    }

    /// Switching contours on is one intent: the source is selected AND its
    /// layers are shown. A layout that hid them (the Gisagara CJIC samples)
    /// no longer keeps them acquired-but-invisible after the operator asks
    /// for them.
    #[test]
    fn enabling_a_source_shows_the_layers_the_layout_had_hidden() {
        let contours = derived_layers().remove(2);
        let mut layout = toggle(default_layout(), contours.clone(), true, None).unwrap();
        for id in ["contours_index", "contours_intermediate"] {
            layout
                .style_refs
                .insert(id.into(), format!("print_context/{id}_print"));
            layout.style_overrides.insert(
                id.into(),
                super::super::PrintStyleOverride {
                    visible: Some(false),
                    color: Some("#123456".into()),
                    ..Default::default()
                },
            );
        }
        assert!(!prints_in(&layout, &contours), "selected in name only");
        let on = toggle(layout, contours.clone(), true, None).unwrap();
        assert!(prints_in(&on, &contours));
        assert_eq!(on.style_overrides["contours_index"].visible, None);
        assert_eq!(
            on.style_overrides["contours_index"].color.as_deref(),
            Some("#123456"),
            "only the hide is cleared; the rest of the override stays"
        );
        let off = toggle(on, contours.clone(), false, None).unwrap();
        assert!(!off.context_layers.iter().any(|row| row.id == contours.id));
    }

    #[test]
    fn enabling_a_derived_source_binds_its_expanded_style_keys_once() {
        let contours = derived_layers().remove(2);
        let out = toggle(default_layout(), contours.clone(), true, None).unwrap();
        assert!(out.styles.is_empty());
        for id in ["contours_intermediate", "contours_index"] {
            assert_eq!(
                out.style_keys[id],
                style_key(id, "derived_geojson", "context")
            );
        }
        let again = toggle(out.clone(), contours, true, None).unwrap();
        assert_eq!(again.context_layers.len(), 1);
        assert_eq!(again.style_keys, out.style_keys);
    }

    fn collection() -> Value {
        serde_json::json!({"type": "FeatureCollection", "features": []})
    }

    fn retained(layer: &PrintContextLayer, ids: &[&str]) -> RetainedContext {
        RetainedContext {
            schema: RETAINED_SCHEMA.into(),
            parameters: retained_parameters(&layer.source).unwrap(),
            coverage: [30.0, -2.0, 30.2, -1.8],
            layers: Value::Object(ids.iter().map(|id| ((*id).into(), collection())).collect()),
            source_version: "v".into(),
            warnings: vec!["contours clipped".into()],
        }
    }

    #[test]
    fn retained_parameters_spell_the_browsers_replacer_form() {
        let contours = derived_layers().remove(2);
        assert_eq!(
            retained_parameters(&contours.source).unwrap(),
            r#"{"buffer_m":200,"index_interval_m":50,"kind":"elevation_contours","minor_interval_m":10,"sample_spacing_m":10}"#
        );
        let mv = PrintContextLayer {
            id: "project_dsgrid_mv".into(),
            label: "MV".into(),
            source: PrintContextSource::ProjectDsgridMv {
                buffer_m: 150.5,
                corridor: Some(super::super::PrintCorridor {
                    distance_m: 20.,
                    color: Some("#ff0000".into()),
                    width_mm: Some(0.3),
                    opacity: Some(0.5),
                    dash_mm: Some(vec![]),
                }),
            },
        };
        assert_eq!(
            retained_parameters(&mv.source).unwrap(),
            r#"{"buffer_m":150.5,"corridor":{},"kind":"project_dsgrid_mv"}"#
        );
    }

    #[test]
    fn a_retained_context_is_reused_only_with_the_same_parameters_covering_bounds_and_its_own_layer_ids(
    ) {
        let contours = derived_layers().remove(2);
        let wanted = [30.05, -1.95, 30.15, -1.85];
        let held = retained(&contours, &["contours_intermediate", "contours_index"]);
        match retained_admission(&contours, Some(&held), wanted) {
            RetainedVerdict::Use { layers, warnings } => {
                assert_eq!(layers.len(), 2);
                assert_eq!(warnings, ["contours clipped"]);
            }
            other => panic!("{other:?}"),
        }
        let reason = |retained: &RetainedContext, wanted: [f64; 4]| match retained_admission(
            &contours,
            Some(retained),
            wanted,
        ) {
            RetainedVerdict::Omit { reason } => reason,
            other => panic!("{other:?}"),
        };
        // The same entry does not cover a wider extent.
        assert_eq!(
            reason(&held, [29.9, -1.95, 30.15, -1.85]),
            "retained for a smaller extent"
        );
        let mut other_intervals = held.clone();
        other_intervals.parameters = other_intervals
            .parameters
            .replace("\"index_interval_m\":50", "\"index_interval_m\":100");
        assert_eq!(
            reason(&other_intervals, wanted),
            "retained with other parameters"
        );
        let mut v0 = held.clone();
        v0.schema = "ds.transformer-print-context/v0".into();
        assert_eq!(reason(&v0, wanted), "retained under another schema");
        let foreign = retained(&contours, &["contours_index", "dsgrid_mv_lines"]);
        assert_eq!(
            reason(&foreign, wanted),
            "retained layers are not this source's"
        );
        let mut broken = held.clone();
        broken.layers["contours_index"] = serde_json::json!({"type": "Feature"});
        assert_eq!(
            reason(&broken, wanted),
            "retained layer is not a feature collection"
        );
        let mut nan = held.clone();
        nan.coverage[0] = f64::NAN;
        assert_eq!(reason(&nan, wanted), "retained for a smaller extent");
        // A source no computer retains is omitted even with an entry offered.
        let buildings = derived_layers().remove(1);
        assert_eq!(
            retained_admission(&buildings, Some(&held), wanted),
            RetainedVerdict::Omit {
                reason: "not a retained context kind".into()
            }
        );
    }

    #[test]
    fn a_retained_mv_projection_carries_every_layer_the_kernel_emits() {
        // Reuse must keep all four emitted model layers, including an empty
        // prohibited-zones collection when this model has none.
        let mv = derived_layers().remove(0);
        let mut held = retained(&mv, &super::super::map::MV_PRINT_LAYER_IDS);
        held.layers["dsgrid_mv_angle_points"]["features"] = serde_json::json!([{
            "type": "Feature", "id": "model-a:\"node-a\"",
            "geometry": {"type": "Point", "coordinates": [30.1, -1.9]},
            "properties": {"point_role": "angle", "number": "PI-17"}
        }]);
        match retained_admission(&mv, Some(&held), [30.05, -1.95, 30.15, -1.85]) {
            RetainedVerdict::Use { layers, .. } => {
                assert_eq!(
                    layers.keys().map(String::as_str).collect::<Vec<_>>(),
                    [
                        "dsgrid_mv_angle_points",
                        "dsgrid_mv_lines",
                        "dsgrid_mv_prohibited_zones",
                        "dsgrid_mv_structures"
                    ]
                );
                assert_eq!(
                    layers["dsgrid_mv_angle_points"]["features"][0]["properties"]["number"],
                    "PI-17"
                );
            }
            other => panic!("{other:?}"),
        }
        // Contours still expand as the template does: an MV id is foreign.
        let contours = derived_layers().remove(2);
        assert_eq!(
            retained_admission(
                &contours,
                Some(&retained(
                    &contours,
                    &["contours_index", "dsgrid_mv_angle_points"]
                )),
                [30.05, -1.95, 30.15, -1.85]
            ),
            RetainedVerdict::Omit {
                reason: "retained layers are not this source's".into()
            }
        );
    }

    #[test]
    fn nothing_retained_is_a_named_omission_not_an_error() {
        let mv = derived_layers().remove(0);
        let verdict = retained_admission(&mv, None, [30., -2., 30.1, -1.9]);
        assert_eq!(
            verdict,
            RetainedVerdict::Omit {
                reason: "not retained on this computer".into()
            }
        );
        let refusal = omission_refusal(&mv, "not retained on this computer");
        assert_eq!(refusal.code, "print_context_omitted");
        assert_eq!(refusal.message_key, "printing_context_omitted");
        assert_eq!(refusal.params["label"], mv.label);
        assert_eq!(refusal.params["reason"], "not retained on this computer");
        // The retained entry also reads from the browser's own spelling.
        let entry: RetainedContext = serde_json::from_value(serde_json::json!({
            "schema": RETAINED_SCHEMA, "parameters": "", "coverage": [0., 0., 1., 1.],
            "layers": {}, "sourceVersion": "[]"
        }))
        .unwrap();
        assert_eq!(entry.source_version, "[]");
    }
}
