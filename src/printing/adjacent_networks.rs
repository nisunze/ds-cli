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

/// An effective page may omit absent roles or partition them by source kind.
/// The configuration boundary below still requires all five API-resolved roles.
/// Every active context role must use a governed reference, never an inline pen.
pub fn validate_selection(layout: &Layout) -> Result<(), String> {
    for id in layout.styles.keys() {
        let base = id.split("-source-").next().unwrap_or(id);
        if LAYERS.iter().any(|(_, role)| *role == base) {
            return Err("adjacent networks require API-resolved context references; inline context pens are forbidden".into());
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
            return Err("style_resolution_required: pass API-resolved context refs for tr, lv_lines, service_cables, customers and lv_poles".into());
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
        for (source, target) in LAYERS {
            let Some(reference) = layout.style_refs.get(target) else {
                continue;
            };
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
            // Existing neighbours use this exact foreground document and all
            // its overrides. Retain its categorical/size/label fields before
            // native projection, including fields hidden by the context pen.
            if source != "tr" {
                if let Some(reference) = layout.style_refs.get(source) {
                    let document = documents
                        .get(reference)
                        .filter(|doc| doc.is_object())
                        .ok_or_else(|| {
                            format!("invariant foreground print style {reference} is not held")
                        })?;
                    let effective = super::style_overrides::apply(
                        document,
                        &layout
                            .style_overrides
                            .get(source)
                            .cloned()
                            .unwrap_or_default(),
                        layout.paper.expression_value(),
                    )?;
                    result
                        .entry(source.into())
                        .or_default()
                        .extend(crate::pinned_context::referenced_fields(&effective));
                }
            }
            if source == "lv_poles" {
                result
                    .entry(source.into())
                    .or_default()
                    .extend(["struct_type".into(), "material".into()]);
            }
        }
    }
    Ok(result)
}

/// Split existing neighbouring LV from design context using the native
/// producer's exact normalized lifecycle vocabulary. The effective page binds
/// existing features to the foreground role's already-held governed reference
/// and complete override; it never authors a pen or changes source features.
/// This projection also admits old/raw renderer captures at the final boundary.
pub fn preserve_existing_lv(
    layout: &Layout,
    layers: &[super::resolved_styles::VectorLayer],
) -> Result<Option<(Layout, Vec<super::resolved_styles::VectorLayer>)>, String> {
    const SUFFIX: &str = "-source-invariant-existing";
    let ids = layers
        .iter()
        .map(|layer| layer.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut output = None;
    for layer in layers {
        let base = layer.id.split("-source-").next().unwrap_or(&layer.id);
        let Some((source, _)) = LAYERS[1..].iter().find(|(_, role)| *role == base) else {
            continue;
        };
        let features = layer.geojson["features"].as_array().ok_or_else(|| {
            format!(
                "print_neighbor_lifecycle_invalid: {} has no feature collection",
                layer.id
            )
        })?;
        let mut existing = Vec::new();
        let mut proposed = Vec::new();
        for feature in features {
            match feature["properties"]["existing_status"].as_str() {
                Some("existing") => existing.push(feature.clone()),
                Some("new") => proposed.push(feature.clone()),
                _ => return Err(format!("print_neighbor_lifecycle_invalid: {} feature {} lacks normalized new/existing evidence; refresh its native room without guessing status", layer.id, feature.get("id").unwrap_or(&Value::Null))),
            }
        }
        if existing.is_empty() {
            continue;
        }
        let reference = layout.style_refs.get(*source)
            .ok_or_else(|| format!("print_invariant_style_missing: {} needs the exact governed foreground {source} reference", layer.id))?;
        let foreground_override = layout.style_overrides.get(*source);
        if layer.id.ends_with(SUFFIX) {
            if !proposed.is_empty()
                || layout.style_refs.get(&layer.id) != Some(reference)
                || layout.style_overrides.get(&layer.id) != foreground_override
                || layout.style_keys.get(&layer.id) != layout.style_keys.get(*source)
                || layout.styles.contains_key(&layer.id)
            {
                return Err(format!(
                    "print_invariant_style_mismatch: {} differs from its exact foreground binding",
                    layer.id
                ));
            }
            continue;
        }
        let destination = format!("{}{SUFFIX}", layer.id);
        if ids.contains(destination.as_str()) {
            return Err(format!(
                "print_invariant_style_mismatch: {destination} already exists in the held capture"
            ));
        }
        let (effective, projected) =
            output.get_or_insert_with(|| (layout.clone(), layers.to_vec()));
        let old = projected
            .iter_mut()
            .find(|candidate| candidate.id == layer.id)
            .ok_or("print_neighbor_lifecycle_invalid: effective layer disappeared")?;
        old.geojson["features"] = json!(proposed);
        let mut separate = layer.clone();
        separate.id = destination.clone();
        separate.geojson["features"] = json!(existing);
        projected.push(separate);
        effective.styles.remove(&destination);
        effective
            .style_refs
            .insert(destination.clone(), reference.clone());
        effective.style_overrides.remove(&destination);
        if let Some(change) = foreground_override {
            effective
                .style_overrides
                .insert(destination.clone(), change.clone());
        }
        effective.style_keys.remove(&destination);
        if let Some(key) = layout.style_keys.get(*source) {
            effective
                .style_keys
                .insert(destination.clone(), key.clone());
        }
        let index = effective
            .layer_order
            .iter()
            .position(|id| id == &layer.id)
            .map_or(effective.layer_order.len(), |index| index + 1);
        effective.layer_order.insert(index, destination.clone());
        for element in &mut effective.elements {
            if let Some(legend) = &mut element.legend {
                if legend.layers.contains(&layer.id) {
                    let index = legend.layers.iter().position(|id| id == &layer.id).unwrap() + 1;
                    legend.layers.insert(index, destination.clone());
                    if let Some(title) = legend.titles.get(*source).cloned() {
                        legend.titles.insert(destination.clone(), title);
                    }
                }
                if legend.layers.is_empty() || legend.layers.contains(&destination) {
                    if let Some(fields) = legend.category_fields.get(*source).cloned() {
                        legend.category_fields.insert(destination.clone(), fields);
                    }
                }
            }
        }
    }
    if let Some((effective, _)) = &output {
        super::validate(effective)?;
    }
    Ok(output)
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
    fn existing_neighbour_lv_copies_every_foreground_binding_without_recolour_or_source_edit() {
        let mut layout = super::super::default_layout();
        let mut layers = Vec::new();
        for (source, target) in &LAYERS[1..] {
            layout
                .style_refs
                .insert((*source).into(), format!("master/{source}_print"));
            layout
                .style_refs
                .insert((*target).into(), format!("context/{source}_print"));
            let change =
                serde_json::from_value(json!({"field":"category", "palette":{"wood":"#112233"},
                "size":2.3, "dash_mm":[0.6,0.3], "opacity":0.9,
                "label":{"visible":true,"size_pt":8,"color":"#445566","halo_mm":0.25}}))
                .unwrap();
            layout.style_overrides.insert((*source).into(), change);
            layout.layer_order.insert(0, (*target).into());
            layers.push(super::super::resolved_styles::VectorLayer {
                id: (*target).into(), label: (*target).into(), geojson: json!({"type":"FeatureCollection","features":[
                    {"id":"existing", "geometry":{"type":"Point","coordinates":[30,-2]},"properties":{"existing_status":"existing","category":"wood","struct_type":"S255","transfo":"other"}},
                    {"id":"new", "geometry":{"type":"Point","coordinates":[30.001,-2]},"properties":{"existing_status":"new","category":"steel","struct_type":"S140","transfo":"other"}}
                ]})
            });
        }
        let held = json!({"layout":layout,"layers":layers});
        let (effective, projected) = preserve_existing_lv(&layout, &layers).unwrap().unwrap();
        for (source, target) in &LAYERS[1..] {
            let alias = format!("{target}-source-invariant-existing");
            assert_eq!(effective.style_refs[&alias], layout.style_refs[*source]);
            assert_eq!(
                effective.style_overrides[&alias],
                layout.style_overrides[*source]
            );
            let context = projected.iter().find(|layer| layer.id == *target).unwrap();
            let invariant = projected.iter().find(|layer| layer.id == alias).unwrap();
            assert_eq!(
                context.geojson["features"],
                json!([layers
                    .iter()
                    .find(|layer| layer.id == *target)
                    .unwrap()
                    .geojson["features"][1]])
            );
            assert_eq!(
                invariant.geojson["features"],
                json!([layers
                    .iter()
                    .find(|layer| layer.id == *target)
                    .unwrap()
                    .geojson["features"][0]])
            );
            assert_eq!(
                effective.layer_order.iter().position(|id| id == &alias),
                effective
                    .layer_order
                    .iter()
                    .position(|id| id == *target)
                    .map(|index| index + 1)
            );
        }
        assert_eq!(json!({"layout":layout,"layers":layers}), held);
        assert!(
            preserve_existing_lv(&effective, &projected)
                .unwrap()
                .is_none(),
            "already-admitted projection is idempotent"
        );
        let mut tampered = effective.clone();
        tampered
            .style_overrides
            .remove("neighbor_lv_lines-source-invariant-existing");
        assert!(preserve_existing_lv(&tampered, &projected)
            .unwrap_err()
            .contains("print_invariant_style_mismatch"));
        let mut unknown = layers.clone();
        unknown[0].geojson["features"][0]["properties"]["existing_status"] = json!("Existing pole");
        assert!(
            preserve_existing_lv(&layout, &unknown)
                .unwrap_err()
                .contains("normalized new/existing"),
            "printing does not repeat producer status normalization"
        );
        let mut missing = layout.clone();
        missing.style_refs.remove("lv_lines");
        assert!(preserve_existing_lv(&missing, &layers)
            .unwrap_err()
            .contains("print_invariant_style_missing"));
        let mut collision = layers.clone();
        let mut member = layers[0].clone();
        member.id.push_str("-source-invariant-existing");
        collision.push(member);
        assert!(preserve_existing_lv(&layout, &collision)
            .unwrap_err()
            .contains("already exists"));
    }

    #[test]
    fn mixed_source_partitions_keep_each_feature_once_and_status_in_every_native_role() {
        let mut layout = super::super::default_layout();
        layout
            .style_refs
            .insert("lv_lines".into(), "master/lv_lines_print".into());
        let mut layers = Vec::new();
        for suffix in ["0", "1"] {
            let id = format!("neighbor_lv_lines-source-{suffix}");
            layout
                .style_refs
                .insert(id.clone(), "context/lv_lines_print".into());
            layout.layer_order.push(id.clone());
            layers.push(super::super::resolved_styles::VectorLayer { id: id.clone(),label:id,
                geojson:json!({"type":"FeatureCollection","features":[
                    {"id":format!("existing-{suffix}"),"geometry":{"type":"Point","coordinates":[30,-2]},"properties":{"existing_status":"existing"}},
                    {"id":format!("new-{suffix}"),"geometry":{"type":"Point","coordinates":[30.001,-2]},"properties":{"existing_status":"new"}}
                ]}) });
        }
        let held = json!({"layout":layout,"layers":layers});
        let (effective, projected) = preserve_existing_lv(&layout, &layers).unwrap().unwrap();
        let ids = projected
            .iter()
            .flat_map(|layer| layer.geojson["features"].as_array().unwrap())
            .map(|feature| feature["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            ids,
            BTreeSet::from(["existing-0", "new-0", "existing-1", "new-1"])
        );
        assert_eq!(
            projected
                .iter()
                .map(|layer| layer.geojson["features"].as_array().unwrap().len())
                .sum::<usize>(),
            4
        );
        for suffix in ["0", "1"] {
            assert_eq!(
                effective.style_refs
                    [&format!("neighbor_lv_lines-source-{suffix}-source-invariant-existing")],
                layout.style_refs["lv_lines"]
            );
        }
        assert_eq!(json!({"layout":layout,"layers":layers}), held);
        let mut other = room("other", -2.);
        for (source, _) in LAYERS {
            other.layers.entry(source.into()).or_insert_with(||json!({"type":"FeatureCollection","features":[{"geometry":{"type":"Point","coordinates":[30,-2]},"properties":{}}]}))["features"][0]["properties"]["existing_status"] = json!("existing");
        }
        let context = attach(
            "focus",
            &[room("focus", -2.), other],
            None,
            &BTreeMap::new(),
        )
        .unwrap()
        .unwrap();
        for (_, role) in LAYERS {
            assert_eq!(
                context["layers"][role]["features"][0]["properties"]["existing_status"], "existing",
                "{role}"
            );
        }
    }

    #[test]
    fn existing_neighbor_legend_keeps_foreground_category_axes() {
        let mut layout = super::super::default_layout();
        layout
            .style_refs
            .insert("lv_poles".into(), "master/lv_poles_print".into());
        layout
            .style_refs
            .insert("neighbor_lv_poles".into(), "context/lv_poles_print".into());
        let legend = layout
            .elements
            .iter_mut()
            .find_map(|element| element.legend.as_mut())
            .unwrap();
        legend.layers = vec!["lv_poles".into(), "neighbor_lv_poles".into()];
        let fields = vec![
            "struct_type".into(),
            "material".into(),
            "existing_status".into(),
        ];
        legend
            .category_fields
            .insert("lv_poles".into(), fields.clone());
        let layers = vec![super::super::resolved_styles::VectorLayer {
            id: "neighbor_lv_poles".into(),
            label: "Neighbour poles".into(),
            geojson: json!({"type":"FeatureCollection","features":[{"properties":{"existing_status":"existing","struct_type":"S140","material":"wooden"}}]}),
        }];
        let held = json!({"layout":layout,"layers":layers});
        let (effective, projected) = preserve_existing_lv(&layout, &layers).unwrap().unwrap();
        let alias = "neighbor_lv_poles-source-invariant-existing";
        let legend = effective
            .elements
            .iter()
            .find_map(|element| element.legend.as_ref())
            .unwrap();
        assert!(legend.layers.iter().any(|id| id == alias));
        assert_eq!(legend.category_fields[alias], fields);
        assert_eq!(projected.last().unwrap().geojson, layers[0].geojson);
        assert_eq!(json!({"layout":layout,"layers":layers}), held);
        assert!(preserve_existing_lv(&effective, &projected)
            .unwrap()
            .is_none());
    }

    #[test]
    fn new_only_neighbours_and_mv_are_unchanged_by_existing_lv_projection() {
        let layout = super::super::default_layout();
        let layers = vec![
            super::super::resolved_styles::VectorLayer {
                id: "neighbor_lv_lines".into(),
                label: "new design".into(),
                geojson: json!({"features":[{"properties":{"existing_status":"new"}}]}),
            },
            super::super::resolved_styles::VectorLayer {
                id: "dsgrid_mv_lines".into(),
                label: "MV lifecycle owned separately".into(),
                geojson: json!({"features":[{"properties":{}}]}),
            },
        ];
        assert!(preserve_existing_lv(&layout, &layers).unwrap().is_none());
    }
    #[test]
    fn effective_context_can_omit_absent_roles_but_configuration_is_complete() {
        let mut page = super::super::default_layout();
        page.style_refs.insert(
            "neighbor_lv_lines-source-live".into(),
            "master/lv_lines_print".into(),
        );
        validate_selection(&page).unwrap();
        assert!(configure(
            page.clone(),
            true,
            BTreeMap::from([("lv_lines".into(), "master/lv_lines_print".into())]),
            None
        )
        .unwrap_err()
        .contains("tr, lv_lines"));
        page.styles.insert(
            "neighbor_lv_lines-source-live".into(),
            serde_json::from_value(json!({"stroke":"#112233","width_mm":0.2})).unwrap(),
        );
        assert!(validate_selection(&page)
            .unwrap_err()
            .contains("inline context pens"));
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
        for document in documents.as_object_mut().unwrap().values_mut() {
            document.as_object_mut().unwrap().remove("label");
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
        let baseline: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/governed-physical-print-baselines.json"
        ))
        .unwrap();
        let foreground =
            super::super::style_overrides::physical_style(&baseline["documents"][0]["document"])
                .unwrap()
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
        super::super::validate(&partial).unwrap();
        assert!(!selected(
            &configure(configured, false, BTreeMap::new(), None).unwrap()
        ));
    }
}
