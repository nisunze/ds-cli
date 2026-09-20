//! What a print context layer *is*, which ones a new template starts with,
//! and whether a catalogue layer may be selected — decided once.
//!
//! The Printing setup page authored these as literals: the three derived
//! sources with their buffers, thresholds and intervals, five complete
//! physical print styles for the layers those sources expand into, the rule
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
use super::{Layout, PrintContextLayer, PrintContextSource, PrintCorridor, PrintStyle};
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
        color: "#957197".into(),
        width_mm: 0.15,
        opacity: 0.3,
        dash_mm: vec![2., 1.],
    }
}

/// Context sources prepared per transformer by a new template.
pub fn derived_layers() -> Vec<PrintContextLayer> {
    vec![
        PrintContextLayer {
            id: "project_dsgrid_mv".into(),
            label: "Proposed MV · project DS Grid models".into(),
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

#[allow(clippy::too_many_arguments)] // one literal per print style, read like the table it was
fn style(
    stroke: &str,
    width_mm: f64,
    stroke_opacity: f64,
    fill: &str,
    fill_opacity: f64,
    casing: &str,
    casing_mm: f64,
    dash_mm: &[f64],
    radius_mm: f64,
    label_field: &str,
    label_pt: f64,
) -> PrintStyle {
    PrintStyle {
        hatch: None,
        stroke: stroke.into(),
        width_mm,
        stroke_opacity,
        fill: fill.into(),
        fill_opacity,
        casing: casing.into(),
        casing_mm,
        dash_mm: dash_mm.to_vec(),
        radius_mm,
        label_field: label_field.into(),
        label_pt,
        color_field: String::new(),
        colors: BTreeMap::new(),
        width_field: String::new(),
        widths: BTreeMap::new(),
    }
}

/// The physical styles a derived source's layers print with when the
/// template has authored none of its own. Keyed by the expanded layer id.
pub fn fallback_styles() -> BTreeMap<String, PrintStyle> {
    BTreeMap::from([
        (
            "dsgrid_mv_lines".into(),
            style(
                "#b0006d",
                0.42,
                0.94,
                "#ffffff",
                0.,
                "#ffffff",
                0.32,
                &[6., 1.4, 1.2, 1.4],
                0.7,
                "model_label",
                6.,
            ),
        ),
        (
            "dsgrid_mv_structures".into(),
            style(
                "#7a004c",
                0.2,
                1.,
                "#b0006d",
                0.78,
                "#ffffff",
                0.2,
                &[],
                0.9,
                "label",
                5.5,
            ),
        ),
        (
            "dsgrid_mv_angle_points".into(),
            style(
                "#7a004c",
                0.18,
                1.,
                "#ffffff",
                1.,
                "#ffffff",
                0.15,
                &[],
                0.65,
                "number",
                5.5,
            ),
        ),
        (
            "buildings_context".into(),
            style(
                "#4b5563",
                0.16,
                0.72,
                "#d6d3d1",
                0.34,
                "",
                0.,
                &[],
                0.8,
                "",
                6.,
            ),
        ),
        (
            "contours_intermediate".into(),
            style(
                "#8b7355",
                0.13,
                0.48,
                "#ffffff",
                0.,
                "",
                0.,
                &[],
                0.5,
                "",
                6.,
            ),
        ),
        (
            "contours_index".into(),
            style(
                "#6f5539",
                0.28,
                0.68,
                "#ffffff",
                0.,
                "#ffffff",
                0.25,
                &[],
                0.5,
                "contour_label",
                6.,
            ),
        ),
    ])
}

/// The print layer ids one context source expands into. A source that
/// prints as one layer expands to its own id.
pub fn expanded_layer_ids(layer: &PrintContextLayer) -> Vec<String> {
    match layer.source {
        PrintContextSource::ElevationContours { .. } => {
            vec!["contours_intermediate".into(), "contours_index".into()]
        }
        PrintContextSource::ProjectDsgridMv { .. } => super::map::MV_PRINT_LAYER_IDS
            .iter()
            .map(|id| (*id).to_owned())
            .collect(),
        _ => vec![layer.id.clone()],
    }
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

fn seed_styles(layout: &mut Layout, layer: &PrintContextLayer) {
    let fallbacks = fallback_styles();
    for id in expanded_layer_ids(layer) {
        if let Some(fallback) = fallbacks.get(&id) {
            layout.styles.entry(id).or_insert_with(|| fallback.clone());
        }
    }
}

/// The selection a new project template starts with: every default-on
/// catalogue dataset, then the derived sources, with fallback styles seeded
/// wherever the template has authored none. Existing styles are kept.
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
        seed_styles(&mut layout, layer);
    }
    layout.context_layers = context_layers;
    super::validate(&layout)?;
    Ok(layout)
}

/// Switch one context layer on or off. Enabling a catalogue layer needs its
/// dataset held or fetchable — unless it is already selected, in which case
/// the operator may always switch it off and on again without losing it.
/// Enabling a derived source seeds the fallback styles of the layers it
/// prints as.
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
        seed_styles(&mut layout, &layer);
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
    use crate::printing::default_layout;

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
    fn every_expanded_layer_of_a_derived_source_has_a_fallback_style() {
        let fallbacks = fallback_styles();
        for layer in derived_layers() {
            for id in expanded_layer_ids(&layer) {
                assert!(fallbacks.contains_key(&id), "{id}");
            }
        }
        assert_eq!(fallbacks.len(), 6);
        assert_eq!(
            fallbacks["dsgrid_mv_lines"].dash_mm,
            vec![6., 1.4, 1.2, 1.4]
        );
        assert_eq!(fallbacks["contours_index"].label_field, "contour_label");
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
            out.styles["dsgrid_mv_lines"].stroke, "#b0006d",
            "missing styles are seeded"
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

    #[test]
    fn enabling_a_derived_source_seeds_its_expanded_styles_once() {
        let contours = derived_layers().remove(2);
        let out = toggle(default_layout(), contours.clone(), true, None).unwrap();
        assert!(out.styles.contains_key("contours_intermediate"));
        assert!(out.styles.contains_key("contours_index"));
        let again = toggle(out.clone(), contours, true, None).unwrap();
        assert_eq!(again.context_layers.len(), 1);
        assert_eq!(again.styles.len(), out.styles.len());
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
                    color: "#ff0000".into(),
                    width_mm: 0.3,
                    opacity: 0.5,
                    dash_mm: vec![],
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
        // The browser retains exactly what `map_mv_layers` answered: lines,
        // structures and angle points. Reuse must take all three, and an
        // empty structures collection with them.
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
