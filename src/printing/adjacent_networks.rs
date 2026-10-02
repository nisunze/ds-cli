//! Print-only neighbouring circuits. The focused room remains the sole
//! engineering/report source; the renderer clips context to its actual camera.
use super::{Layout, PrintStyleOverride};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const LAYERS: [(&str, &str); 5] = [
    ("tr", "neighbor_transformers"),
    ("lv_lines", "neighbor_lv_lines"),
    ("service_cables", "neighbor_service_cables"),
    ("customers", "neighbor_customers"),
    ("lv_poles", "neighbor_lv_poles"),
];

/// Explicit project-local replacement for governed context colours. Defaults
/// live in the backend's named Style Center seed documents, not in this kernel.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: String,
    pub proposed: String,
    pub existing: String,
    pub unclassified: String,
    pub opacity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub transformer: String,
    pub version: i64,
    pub layers: BTreeMap<String, Value>,
}

pub fn selected(layout: &Layout) -> bool {
    LAYERS[1..]
        .iter()
        .any(|(_, id)| layout.styles.contains_key(*id) || layout.style_refs.contains_key(*id))
}

/// All roles need governed symbols and explicit lifecycle distinctions.
pub fn validate_selection(layout: &Layout) -> Result<(), String> {
    if !selected(layout) {
        return Ok(());
    }
    for (_, id) in LAYERS {
        if !layout.style_refs.contains_key(id) || layout.styles.contains_key(id) {
            return Err("adjacent networks require the complete resolved context style set; reapply set_adjacent_networks with API-resolved references".into());
        }
    }
    Ok(())
}

/// Bind each role's governed print symbol independently, preserving its icon,
/// casing, dimensions and line pattern. Foreground and camera stay exact.
pub fn configure(
    mut layout: Layout,
    enabled: bool,
    style_refs: BTreeMap<String, String>,
    policy: Option<Policy>,
) -> Result<Layout, String> {
    if enabled {
        if let Some(policy) = &policy {
            if policy.schema != "ds.adjacent-print-hierarchy/v1"
                || [&policy.proposed, &policy.existing, &policy.unclassified]
                    .iter()
                    .any(|c| !super::color(c))
                || policy.proposed == policy.existing
                || policy.proposed == policy.unclassified
                || policy.existing == policy.unclassified
                || !policy.opacity.is_finite()
                || policy.opacity <= 0.
                || policy.opacity >= 1.
            {
                return Err("adjacent hierarchy requires its exact schema, three distinct colours and opacity between zero and one".into());
            }
        }
        if style_refs.is_empty() {
            return Err("style_resolution_required: adjacent context styles must be resolved through the governed style API".into());
        }
        if style_refs.len() != LAYERS.len()
            || LAYERS
                .iter()
                .any(|(source, _)| !style_refs.contains_key(*source))
        {
            return Err("adjacent style_refs require exactly tr, lv_lines, service_cables, customers and lv_poles".into());
        }
        for (source, id) in LAYERS {
            let reference = style_refs[source].clone();
            layout.styles.remove(id);
            layout.style_refs.insert(id.into(), reference);
            layout.style_overrides.remove(id);
            if policy.is_some() {
                layout.style_overrides.insert(
                    id.into(),
                    PrintStyleOverride {
                        palette_fallback: policy.as_ref().map(|p| p.unclassified.clone()),
                        color_scope: super::PrintColorScope::BodyAndBoundary,
                        field: policy.as_ref().map(|_| "existing_status".into()),
                        palette: policy.as_ref().map_or_else(BTreeMap::new, |p| {
                            BTreeMap::from([
                                ("new".into(), p.proposed.clone()),
                                ("existing".into(), p.existing.clone()),
                            ])
                        }),
                        opacity: policy.as_ref().map(|p| p.opacity),
                        ..Default::default()
                    },
                );
            }
        }
        layout
            .layer_order
            .retain(|id| !LAYERS.iter().any(|(_, target)| id == target));
        let front = layout
            .layer_order
            .iter()
            .position(|id| LAYERS.iter().any(|(source, _)| id == source))
            .unwrap_or(layout.layer_order.len());
        layout
            .layer_order
            .splice(front..front, LAYERS.into_iter().map(|(_, id)| id.into()));
    } else {
        // Existing point-only context remains available independently.
        for (_, id) in &LAYERS[1..] {
            layout.styles.remove(*id);
            layout.style_refs.remove(*id);
            layout.style_overrides.remove(*id);
        }
        layout
            .layer_order
            .retain(|id| !LAYERS[1..].iter().any(|(_, target)| id == target));
    }
    super::validate(&layout)?;
    Ok(layout)
}

/// Project only the fields the effective governed pens read. Hidden context
/// labels do not pull customer identities into the staged print payload.
pub fn required_fields(
    layouts: &[Layout],
    documents: &Value,
) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let mut result: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for layout in layouts.iter().filter(|l| selected(l)) {
        validate_selection(layout)?;
        for (source, target) in LAYERS {
            let reference = &layout.style_refs[target];
            let document = documents
                .get(reference)
                .filter(|d| d.is_object())
                .ok_or_else(|| format!("adjacent print style {reference} is not held"))?;
            let effective = super::style_overrides::apply(
                document,
                &layout
                    .style_overrides
                    .get(target)
                    .cloned()
                    .unwrap_or_default(),
                layout.paper.expression_value(),
            )?;
            validate_lifecycle_pen(&effective)?;
            result
                .entry(source.into())
                .or_default()
                .extend(crate::pinned_context::referenced_fields(&effective));
        }
    }
    Ok(result)
}

fn validate_lifecycle_pen(document: &Value) -> Result<(), String> {
    let (color, opacity) = match document["type"].as_str() {
        Some("line") => ("line-color", "line-opacity"),
        Some("circle") => ("circle-color", "circle-opacity"),
        Some("symbol") => ("icon-color", "icon-opacity"),
        _ => return Err("adjacent role needs a governed line or point symbol".into()),
    };
    let expression = &document["paint"][color];
    let categories: BTreeMap<&str, &str> = expression
        .as_array()
        .filter(|items| items.len() == 7)
        .map(|items| {
            items[2..items.len().saturating_sub(1)]
                .chunks_exact(2)
                .filter_map(|pair| Some((pair[0].as_str()?, pair[1].as_str()?)))
                .collect()
        })
        .unwrap_or_default();
    let fallback = expression
        .as_array()
        .and_then(|items| items.last())
        .and_then(Value::as_str);
    if expression[0] != "match"
        || expression[1] != json!(["get", "existing_status"])
        || categories.len() != 2
        || !categories.contains_key("new")
        || !categories.contains_key("existing")
        || fallback.is_none_or(|c| !super::color(c) || categories.values().any(|v| *v == c))
        || categories.values().any(|v| !super::color(v))
        || categories.get("new") == categories.get("existing")
        || document["paint"][opacity]
            .as_f64()
            .is_none_or(|v| !v.is_finite() || v <= 0. || v >= 1.)
    {
        return Err("adjacent print refs must distinguish proposed, existing and unknown status with subdued opacity; resolve the declared context style or supply a governed project policy".into());
    }
    Ok(())
}

/// Keep every circuit, even when its transformer lies outside the sheet.
/// Spatial selection is the final map clip, including explicit view exceptions.
/// No nearest-transformer heuristic or geometry repair belongs here.
pub fn attach(
    current: &str,
    rooms: &[Room],
    context: Option<&Value>,
    fields: &BTreeMap<String, BTreeSet<String>>,
) -> Result<Option<Value>, String> {
    if !rooms.iter().any(|room| room.transformer == current) {
        return Err("adjacent project roster does not contain the focused transformer".into());
    }
    if rooms.len() > 500 {
        return Err("adjacent context exceeds 500 project rooms".into());
    }
    let mut doc = context
        .cloned()
        .unwrap_or_else(|| json!({"coverage":null,"sources":[],"layers":{}}));
    let mut layers: BTreeMap<String, Value> = serde_json::from_value(doc["layers"].clone())
        .map_err(|e| format!("invalid adjacent print context: {e}"))?;
    let mut projected: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut sources = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    let mut count = 0usize;
    let mut bounds: Option<[f64; 4]> = None;
    let world = json!({"type":"Polygon","coordinates":[[[-180.,-90.],[180.,-90.],[180.,90.],[-180.,90.],[-180.,-90.]]]});
    for room in rooms {
        if room.transformer.trim().is_empty()
            || room.transformer.len() > 200
            || room.version <= 0
            || !names.insert(&room.transformer)
        {
            return Err(
                "adjacent rooms require distinct identities and positive saved versions".into(),
            );
        }
        if room.transformer == current {
            continue;
        }
        let mut owner_count = 0usize;
        for (source, target) in LAYERS {
            let Some(layer) = room.layers.get(source) else {
                continue;
            };
            let features = layer["features"].as_array().ok_or_else(|| {
                format!("{} {source} has no feature collection", room.transformer)
            })?;
            for (index, feature) in features.iter().enumerate() {
                let geometry = &feature["geometry"];
                if geometry.is_null() {
                    continue;
                }
                if let Some(owner) = feature["properties"]["transfo"].as_str() {
                    if owner != room.transformer {
                        return Err(format!(
                            "{} {source} carries another transformer's ownership",
                            room.transformer
                        ));
                    }
                }
                // Reuse the vector owner's exact WGS84 admission.
                ds_geo_lite::spatial_predicate::intersects_area(geometry, &world)?;
                let b = ds_geo_lite::viewport::geojson_bounds(geometry)
                    .ok_or("adjacent geometry has no geographic bounds")?;
                bounds = Some(bounds.map_or(b, |a| {
                    [
                        a[0].min(b[0]),
                        a[1].min(b[1]),
                        a[2].max(b[2]),
                        a[3].max(b[3]),
                    ]
                }));
                count += 1;
                owner_count += 1;
                if count > 200_000 {
                    return Err(
                        "adjacent context exceeds 200,000 features; no circuits were truncated"
                            .into(),
                    );
                }
                let mut properties = json!({"transfo":room.transformer,"print_context_source":"project_transformer_networks",
                    "room_version":room.version,"source_layer":source,"source_feature_id":feature.get("id")});
                for key in fields
                    .get(source)
                    .into_iter()
                    .flatten()
                    .map(String::as_str)
                    .chain(["existing_status", "existing_status_detail"])
                {
                    if let Some(value) = feature["properties"].get(key) {
                        if key != "transfo"
                            && !key.starts_with("print_context_")
                            && key != "room_version"
                            && key != "source_layer"
                            && key != "source_feature_id"
                        {
                            properties[key] = value.clone();
                        }
                    }
                }
                projected.entry(target.into()).or_default().push(json!({
                    "type":"Feature", "id":format!("{}:{source}:{index}", room.transformer),
                    "geometry":geometry,
                    "properties":properties
                }));
            }
        }
        sources.push(json!({"kind":"project_transformer_networks","transformer":room.transformer,
            "room_version":room.version,"feature_count":owner_count,
            "source_sha256":crate::report_export::sha256_hex(&serde_json::to_vec(room).map_err(|e|e.to_string())?)}));
    }
    if count == 0 {
        return Ok(context.cloned());
    }
    for (id, features) in projected {
        if layers.contains_key(&id) {
            return Err(format!("print context already contains {id}"));
        }
        layers.insert(id, json!({"type":"FeatureCollection","features":features}));
    }
    if doc["coverage"].is_null() {
        let [mut w, mut s, mut e, mut n] = bounds.expect("nonempty context has bounds");
        if w == e {
            w = (w - 0.00001).max(-180.);
            e = (e + 0.00001).min(180.);
        }
        if s == n {
            s = (s - 0.00001).max(-90.);
            n = (n + 0.00001).min(90.);
        }
        doc["coverage"] = json!({"type":"Polygon","coordinates":[[[w,s],[e,s],[e,n],[w,n],[w,s]]]});
    }
    doc["sources"]
        .as_array_mut()
        .ok_or("print context has no sources")?
        .extend(sources);
    doc["layers"] = serde_json::to_value(layers).map_err(|e| e.to_string())?;
    Ok(Some(doc))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn room(name: &str, y: f64) -> Room {
        Room {
            transformer: name.into(),
            version: 3,
            layers: BTreeMap::from([
                (
                    "tr".into(),
                    json!({"features":[{"geometry":{"type":"Point","coordinates":[30.1,y]},"properties":{"transfo":name}}]}),
                ),
                (
                    "lv_lines".into(),
                    json!({"features":[{"id":"line","geometry":{"type":"LineString","coordinates":[[29.9,y],[30.1,y]]},"properties":{"transfo":name,"voltage_drop":99,"private_note":"secret"}}]}),
                ),
                (
                    "service_cables".into(),
                    json!({"features":[{"geometry":{"type":"LineString","coordinates":[[30.,y],[30.,y+0.01]]}}]}),
                ),
                (
                    "customers".into(),
                    json!({"features":[{"geometry":{"type":"Point","coordinates":[30.,y+0.01]},"properties":{"customer_name":"private"}}]}),
                ),
            ]),
        }
    }
    #[test]
    fn all_owned_circuits_survive_with_crossings_and_edge_transformers_and_no_engineering_attributes(
    ) {
        let rooms = vec![
            room("focus", -2.),
            room("edge-a", -2.01),
            room("edge-b", -1.99),
        ];
        let before = serde_json::to_vec(&rooms).unwrap();
        let doc = attach("focus", &rooms, None, &BTreeMap::new())
            .unwrap()
            .unwrap();
        for (_, id) in &LAYERS[..4] {
            assert_eq!(doc["layers"][*id]["features"].as_array().unwrap().len(), 2);
        }
        let lines = &doc["layers"]["neighbor_lv_lines"]["features"];
        assert_eq!(
            lines[0]["geometry"],
            rooms[1].layers["lv_lines"]["features"][0]["geometry"]
        );
        assert_eq!(lines[1]["properties"]["transfo"], "edge-b");
        let text = doc.to_string();
        for absent in ["private_note", "customer_name", "voltage_drop"] {
            assert!(!text.contains(absent));
        }
        assert_eq!(serde_json::to_vec(&rooms).unwrap(), before);
        let mut bad = rooms.clone();
        bad[1].layers.get_mut("lv_lines").unwrap()["features"][0]["properties"]["transfo"] =
            json!("focus");
        assert!(attach("focus", &bad, None, &BTreeMap::new())
            .unwrap_err()
            .contains("ownership"));
    }
    #[test]
    fn proposed_transformer_on_existing_lv_keeps_roles_status_geometry_and_unknowns() {
        let mut layout = super::super::default_layout();
        let mut documents = json!({});
        for (source, _) in LAYERS {
            let reference = format!("master/{source}_print");
            layout.style_refs.insert(source.into(), reference.clone());
            documents[reference] = match source {
                "lv_lines" | "service_cables" => {
                    json!({"type":"line","paint":{"line-color":"#ffcc00","line-width":2,"line-dasharray":[2,1]},"boundary":{"type":"line","paint":{"line-color":"#ffffff","line-width":4}}})
                }
                _ => {
                    json!({"type":"symbol","layout":{"icon-image":["match",["get","asset_kind"],"school","school","pole"],"icon-size":1.2},"paint":{"icon-color":"#ff0000"},"boundary":{"type":"symbol","layout":{"icon-image":"outline","icon-size":1.4},"paint":{"icon-color":"#00ff00"}}})
                }
            };
        }
        let refs = LAYERS
            .iter()
            .map(|(source, _)| (source.to_string(), format!("master/{source}_print")))
            .collect();
        let configured = configure(
            layout,
            true,
            refs,
            Some(Policy {
                schema: "ds.adjacent-print-hierarchy/v1".into(),
                proposed: "#356d96".into(),
                existing: "#768e9f".into(),
                unclassified: "#94a3b8".into(),
                opacity: 0.65,
            }),
        )
        .unwrap();
        let fields = required_fields(std::slice::from_ref(&configured), &documents).unwrap();
        assert!(fields["customers"].contains("asset_kind"));
        assert!(!fields["customers"].contains("customer_name"));
        assert!(!fields["lv_lines"].contains("private_note"));
        let mut adjacent = room("other", -2.);
        adjacent.layers.get_mut("tr").unwrap()["features"][0]["properties"]["existing_status"] =
            json!("new");
        adjacent.layers.get_mut("lv_lines").unwrap()["features"][0]["properties"]
            ["existing_status"] = json!("existing");
        adjacent.layers.get_mut("customers").unwrap()["features"][0]["properties"]["asset_kind"] =
            json!("school");
        let original = serde_json::to_vec(&adjacent).unwrap();
        let context = attach(
            "focus",
            &[room("focus", -2.), adjacent.clone()],
            None,
            &fields,
        )
        .unwrap()
        .unwrap();
        let layers = &context["layers"];
        assert_eq!(
            layers["neighbor_transformers"]["features"][0]["properties"]["existing_status"],
            "new"
        );
        assert_eq!(
            layers["neighbor_lv_lines"]["features"][0]["properties"]["existing_status"],
            "existing"
        );
        assert!(layers["neighbor_customers"]["features"][0]["properties"]
            .get("existing_status")
            .is_none());
        assert_eq!(
            layers["neighbor_customers"]["features"][0]["properties"]["asset_kind"],
            "school"
        );
        assert_eq!(
            layers["neighbor_lv_lines"]["features"][0]["geometry"],
            adjacent.layers["lv_lines"]["features"][0]["geometry"]
        );
        assert_eq!(serde_json::to_vec(&adjacent).unwrap(), original);
        let pen = super::super::style_overrides::apply(
            &documents["master/lv_lines_print"],
            &configured.style_overrides["neighbor_lv_lines"],
            "print_a0",
        )
        .unwrap();
        assert_eq!(pen["paint"]["line-width"], 2);
        assert_eq!(pen["paint"]["line-dasharray"], json!([2, 1]));
        assert_eq!(pen["boundary"]["paint"]["line-width"], 4);
        assert_eq!(
            pen["boundary"]["paint"]["line-color"],
            pen["paint"]["line-color"]
        );
        assert_eq!(pen["boundary"]["paint"]["line-opacity"], 0.65);
        assert_eq!(
            pen["paint"]["line-color"]
                .as_array()
                .unwrap()
                .last()
                .unwrap(),
            "#94a3b8"
        );
        let customer_pen = super::super::style_overrides::apply(
            &documents["master/customers_print"],
            &configured.style_overrides["neighbor_customers"],
            "print_a0",
        )
        .unwrap();
        assert_eq!(
            customer_pen["layout"],
            documents["master/customers_print"]["layout"]
        );
        assert_eq!(
            customer_pen["boundary"]["layout"],
            documents["master/customers_print"]["boundary"]["layout"]
        );
    }
    #[test]
    fn configured_context_preserves_primary_pens_and_focus_camera() {
        let mut layout = super::super::default_layout();
        let foreground = super::super::context::fallback_styles()
            .remove("dsgrid_mv_lines")
            .unwrap();
        layout.styles.insert("lv_lines".into(), foreground.clone());
        for (source, _) in LAYERS {
            layout
                .style_refs
                .insert(source.into(), format!("master/{source}_print"));
        }
        layout.layer_order = vec!["buildings".into(), "lv_lines".into(), "customers".into()];
        let before = layout.clone();
        assert!(configure(layout.clone(), true, BTreeMap::new(), None)
            .unwrap_err()
            .contains("style_resolution_required"));
        let references = LAYERS
            .iter()
            .map(|(source, _)| {
                (
                    source.to_string(),
                    format!("fixture/context-{source}_print"),
                )
            })
            .collect();
        let configured = configure(layout, true, references, None).unwrap();
        assert!(selected(&configured));
        assert_eq!(configured.styles["lv_lines"], before.styles["lv_lines"]);
        assert_eq!(configured.focus, before.focus);
        assert_eq!(configured.elements, before.elements);
        assert_eq!(
            configured.style_refs["neighbor_customers"],
            "fixture/context-customers_print"
        );
        assert!(!configured
            .style_overrides
            .contains_key("neighbor_customers"));
        assert_eq!(configured.layer_order.first().unwrap(), "buildings");
        assert!(
            configured
                .layer_order
                .iter()
                .position(|id| id == "neighbor_customers")
                .unwrap()
                < configured
                    .layer_order
                    .iter()
                    .position(|id| id == "lv_lines")
                    .unwrap()
        );
        let mut partial = configured.clone();
        partial.style_refs.remove("neighbor_customers");
        assert!(super::super::validate(&partial)
            .unwrap_err()
            .contains("complete"));
        assert!(!selected(
            &configure(configured, false, BTreeMap::new(), None).unwrap()
        ));
    }
}
