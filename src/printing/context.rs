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

use super::{Layout, PrintContextLayer, PrintContextSource, PrintStyle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Catalogue layers a default selection never includes: cadastral parcels
/// clutter an engineering sheet at every scale a network is drawn at.
pub const DEFAULT_CATALOG_EXCLUSIONS: [&str; 1] = ["rwanda_upi_parcels"];

/// Retained coverage around every printed feature, in metres, for the
/// derived (per-transformer prepared) context sources.
pub const DERIVED_BUFFER_M: f64 = 200.;

/// The context sources a Desktop prepares per transformer, as a new
/// template starts with them.
pub fn derived_layers() -> Vec<PrintContextLayer> {
    vec![
        PrintContextLayer {
            id: "project_dsgrid_mv".into(),
            label: "Proposed MV · project DS Grid models".into(),
            source: PrintContextSource::ProjectDsgridMv {
                buffer_m: DERIVED_BUFFER_M,
                corridor: None,
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
        PrintContextSource::ProjectDsgridMv { .. } => {
            vec!["dsgrid_mv_lines".into(), "dsgrid_mv_structures".into()]
        }
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
            Ok(CatalogRow {
                layer: catalog_layer(resource),
                selectable,
                default_on: selectable
                    && !DEFAULT_CATALOG_EXCLUSIONS.contains(&resource.layer.as_str()),
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
        assert_eq!(fallbacks.len(), 5);
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
}
