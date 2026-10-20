//! Source-pinned context independent of MV revision and pagination.
//!
//! The booklet's geographic context is a Kernel decision: which held families
//! a plan/profile sheet and the key plan carry, how each is styled (the API's
//! `mv_context` paper role) and how the capture the Reporter consumes is
//! assembled. Hosts acquire the held features and hand them here; the
//! Reporter renders the result. No host or UI chooses these families.
use super::context::CatalogResource;
use super::mv_booklet::sha256;
use super::resolved_styles::VectorLayer;
use super::{PrintContextLayer, PrintContextSource};
use crate::style_resolution::{Binding, Key, Snapshot, Target};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "ds.mv-alignment-context/v1";
pub const KEY_PLAN_SCHEMA: &str = "ds.mv-key-plan-context/v1";

/// Held catalogue families every plan/profile sheet carries (owner rulings
/// 2026-10-17 and 2026-10-20): roads, rivers, settlement sites and existing LV.
/// Terrain contours and building footprints follow as derived context.
pub const SHEET_CATALOG: [&str; 4] = ["roads", "rivers", "rwanda_settlement_sites", "lv_line"];
/// New LV from the project's saved design rooms, in the same subordinate
/// family under the red MV route. Transformers also name profile supports.
pub const SHEET_DESIGN: [&str; 2] = ["lv_lines", "tr"];
/// The key plan's overview: roads, power lines and a small set of places.
pub const KEY_PLAN_CATALOG: [&str; 4] = ["roads", "hv_line", "mv_line", "places"];
/// Transformers labelled by their actual project names.
pub const KEY_PLAN_DESIGN: [&str; 1] = ["tr"];
/// Sheet context reaches this far beyond an alignment's route (about 220 m),
/// enough to fill a plan panel at the standard 1:1500.
pub const SHEET_MARGIN_DEG: f64 = 0.002;
/// The key plan frames its routes with this share of their extent around them.
pub const KEY_PLAN_MARGIN: f64 = 0.12;
/// A key plan shows at least this much ground (about 2.2 km), so even a short
/// substation link sits among its roads and places.
pub const KEY_PLAN_MIN_SPAN_DEG: f64 = 0.02;

/// The WGS84 rectangle a key plan frames around the model's route bounds:
/// the same area the host reads and the Reporter draws.
pub fn key_plan_extent(routes: [f64; 4]) -> [f64; 4] {
    let center = [(routes[0] + routes[2]) / 2., (routes[1] + routes[3]) / 2.];
    let half = |span: f64| span.max(KEY_PLAN_MIN_SPAN_DEG) * (1. + 2. * KEY_PLAN_MARGIN) / 2.;
    let (dx, dy) = (half(routes[2] - routes[0]), half(routes[3] - routes[1]));
    [
        center[0] - dx,
        center[1] - dy,
        center[0] + dx,
        center[1] + dy,
    ]
}

/// The key plan names transformers and places by these properties, at a size
/// legible on A3; the map renderer places them clear of each other.
const KEY_PLAN_LABELS: [(&str, &str); 2] = [("tr", "name"), ("places", "name")];
const KEY_PLAN_LABEL_PT: f64 = 7.0;

/// Back-to-front drawing order: terrain first, the new LV network on top.
const ORDER: [&str; 12] = [
    "contours_intermediate",
    "contours_index",
    "rwanda_settlement_sites",
    "buildings_context",
    "rivers",
    "roads",
    "places",
    "hv_line",
    "mv_line",
    "lv_line",
    "lv_lines",
    "tr",
];

/// Legend wording the sheets' plan key reads for its network rows.
const TITLES: [(&str, &str); 3] = [
    ("lv_lines", "New LV"),
    ("lv_line", "Existing LV"),
    ("mv_line", "Existing MV"),
];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub schema: String,
    pub project_id: String,
    /// Stable native alignment identity, never sheet number or array position.
    pub alignment_id: String,
    /// Exact held render input bytes, including source layers and style recipe.
    pub context_sha256: String,
    pub render: Value,
}

impl Capture {
    pub fn validate(&self, project: &str) -> Result<(), String> {
        if self.schema != SCHEMA
            || self.project_id != project
            || self.alignment_id.is_empty()
            || self.alignment_id.len() > 200
            || self.context_sha256 != digest(&self.render)?
        {
            return Err("mv_context_invalid: project, alignment or source digest differs".into());
        }
        refuse_mv_geometry(&self.render)
    }
}

/// The key plan's overview context for one model: one capture, no alignment.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyPlan {
    pub schema: String,
    pub project_id: String,
    pub context_sha256: String,
    pub render: Value,
}

impl KeyPlan {
    pub fn validate(&self, project: &str) -> Result<(), String> {
        if self.schema != KEY_PLAN_SCHEMA
            || self.project_id != project
            || self.context_sha256 != digest(&self.render)?
        {
            return Err("mv_key_plan_context_invalid: project or source digest differs".into());
        }
        refuse_mv_geometry(&self.render)
    }
}

fn digest(render: &Value) -> Result<String, String> {
    Ok(sha256(
        &serde_json::to_vec(render).map_err(|e| e.to_string())?,
    ))
}

// Current model linework belongs to the engine plan, never an old captured
// map. The independent capture contains geographic/LV context.
fn refuse_mv_geometry(render: &Value) -> Result<(), String> {
    if render["layers"].as_array().is_some_and(|layers| {
        layers.iter().any(|layer| {
            layer["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("dsgrid_mv_"))
        })
    }) {
        return Err(
            "mv_context_invalid: captured MV geometry must be removed; the exact native plan owns MV"
                .into(),
        );
    }
    Ok(())
}

pub fn schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Capture)).expect("context schema")
}

/// The context sources a sheet reads from the project's held rooms: the
/// selected catalogue families this machine holds or can read, then terrain
/// contours and building footprints.
pub fn sheet_sources(resources: &[CatalogResource]) -> Vec<PrintContextLayer> {
    let mut layers = catalog(resources, &SHEET_CATALOG);
    layers.extend(
        super::context::derived_layers()
            .into_iter()
            .filter(|layer| {
                matches!(
                    layer.source,
                    PrintContextSource::GoogleOpenBuildings { .. }
                        | PrintContextSource::ElevationContours { .. }
                )
            }),
    );
    layers
}

/// The key plan's catalogue sources.
pub fn key_plan_sources(resources: &[CatalogResource]) -> Vec<PrintContextLayer> {
    catalog(resources, &KEY_PLAN_CATALOG)
}

fn catalog(resources: &[CatalogResource], families: &[&str]) -> Vec<PrintContextLayer> {
    families
        .iter()
        .filter_map(|family| {
            resources.iter().find(|resource| {
                resource.layer == *family
                    && !resource.unavailable
                    && (resource.ready || resource.downloadable)
            })
        })
        .map(|resource| PrintContextLayer {
            id: resource.layer.clone(),
            label: resource.label.clone(),
            source: PrintContextSource::Catalog {
                country: resource.country.clone(),
                layer: resource.layer.clone(),
            },
        })
        .collect()
}

/// The governed paper key of one carried layer: every family prints in the
/// API's `mv_context` role, by the kind of source it was read from.
pub fn style_key(layer: &str) -> Result<Key, String> {
    let source_kind = match layer {
        "lv_lines" | "tr" => "saved_design",
        "buildings_context" | "contours_intermediate" | "contours_index" => "dataset_geojson",
        _ if SHEET_CATALOG.contains(&layer) || KEY_PLAN_CATALOG.contains(&layer) => "cold_existing",
        _ => return Err(format!("mv_context_family_unknown: {layer}")),
    };
    Ok(Key {
        entity_class: layer.into(),
        source_kind: source_kind.into(),
        target: Target::Print,
        role: "mv_context".into(),
        ink: Default::default(),
    })
}

/// Assemble one alignment's sheet context from the held layers a host read.
pub fn assemble(
    project: &str,
    alignment_id: &str,
    layers: Vec<VectorLayer>,
    styles: &Snapshot,
) -> Result<Capture, String> {
    let render = render(layers, styles, &[])?;
    let capture = Capture {
        schema: SCHEMA.into(),
        project_id: project.into(),
        alignment_id: alignment_id.into(),
        context_sha256: digest(&render)?,
        render,
    };
    capture.validate(project)?;
    Ok(capture)
}

/// Assemble the key plan's overview context.
pub fn assemble_key_plan(
    project: &str,
    layers: Vec<VectorLayer>,
    styles: &Snapshot,
) -> Result<KeyPlan, String> {
    let render = render(layers, styles, &KEY_PLAN_LABELS)?;
    let capture = KeyPlan {
        schema: KEY_PLAN_SCHEMA.into(),
        project_id: project.into(),
        context_sha256: digest(&render)?,
        render,
    };
    capture.validate(project)?;
    Ok(capture)
}

/// One A3 map, its layers back to front, and for every layer the exact key
/// and API reply the Reporter admits. Empty layers are not carried.
fn render(
    mut layers: Vec<VectorLayer>,
    styles: &Snapshot,
    labels: &[(&str, &str)],
) -> Result<Value, String> {
    layers.retain(|layer| {
        layer.geojson["features"]
            .as_array()
            .is_some_and(|features| !features.is_empty())
    });
    layers.sort_by_key(|layer| ORDER.iter().position(|id| *id == layer.id));
    let mut layout = super::default_layout();
    layout.id = "mv-plan-profile-context".into();
    layout.name = "MV plan/profile context".into();
    layout
        .elements
        .retain(|element| matches!(element.id.as_str(), "map" | "legend"));
    for element in &mut layout.elements {
        if element.id == "legend" {
            element.legend = Some(
                serde_json::from_value(json!({"titles": TITLES
                    .iter()
                    .map(|(id, title)| (id.to_string(), title.to_string()))
                    .collect::<BTreeMap<_, _>>()}))
                .map_err(|e| e.to_string())?,
            );
        }
    }
    let mut sources = BTreeMap::new();
    let mut resolutions: BTreeMap<String, Binding> = BTreeMap::new();
    for layer in &layers {
        let key = style_key(&layer.id)?;
        let binding = crate::style_resolution::resolve(styles, &key).map_err(|e| e.to_string())?;
        layout.style_keys.insert(layer.id.clone(), key.clone());
        layout.layer_order.push(layer.id.clone());
        if let Some((_, field)) = labels.iter().find(|(id, _)| *id == layer.id) {
            layout.style_overrides.insert(
                layer.id.clone(),
                super::PrintStyleOverride {
                    label: Some(super::PrintLabelOverride {
                        field: Some((*field).into()),
                        visible: Some(true),
                        size_pt: Some(KEY_PLAN_LABEL_PT),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            );
        }
        sources.insert(layer.id.clone(), key);
        resolutions.insert(layer.id.clone(), binding);
    }
    super::validate(&layout)?;
    Ok(json!({
        "layout": layout,
        "layers": layers,
        "style_sources": sources,
        "style_resolutions": resolutions,
    }))
}

#[cfg(test)]
#[path = "mv_context_tests.rs"]
mod tests;
