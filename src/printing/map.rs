//! Local map composition plans. Hosts acquire data; this kernel owns scope,
//! paper furniture, canonical names and feature membership for every consumer.
use super::{custom::Boundary, *};
use ds_geo::spatial_predicate::{area_bounds, intersects_area, validate_area};
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub project: String,
    pub id: String,
    pub family: String,
    pub codes: Vec<String>,
    pub paper: Paper,
    #[serde(default = "default_dpi")]
    pub dpi: u16,
    #[serde(default)]
    pub layout: Option<Layout>,
}
pub fn default_dpi() -> u16 {
    300
}

pub fn validate_request(r: &Request) -> Result<(), String> {
    if !identifier(&r.project) || !identifier(&r.id) || r.project.len() > 128 || r.id.len() > 80 {
        return Err("Map printing requires exact bounded project and map identifiers".into());
    }
    let digits = match r.family.as_str() {
        "custom-map" => 4,
        "mv-map" => 2,
        _ => return Err("Map family must be custom-map or mv-map".into()),
    };
    if r.codes.is_empty()
        || r.codes.len() > 8
        || r.codes.iter().collect::<BTreeSet<_>>().len() != r.codes.len()
        || r.codes
            .iter()
            .any(|c| c.len() != digits || !c.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(
            "Select 1..8 unique Rwanda sector codes for custom maps or district codes for MV maps"
                .into(),
        );
    }
    if !matches!(r.paper, Paper::A0 | Paper::A3) {
        return Err("Local map printing supports A0 or A3".into());
    }
    if !(72..=1200).contains(&r.dpi) {
        return Err("Print DPI must be between 72 and 1200".into());
    }
    if let Some(layout) = &r.layout {
        validate(layout)?;
        if layout.paper.size != r.paper || layout.paper.orientation != Orientation::Landscape {
            return Err("Authored layout paper differs from requested paper".into());
        }
    }
    Ok(())
}

pub fn plan(r: &Request, boundaries: Vec<Boundary>, sheets: &Value) -> Result<Value, String> {
    validate_request(r)?;
    let mut codes = r.codes.clone();
    codes.sort();
    let mut captured = BTreeMap::new();
    for b in boundaries {
        if b.level
            != if r.family == "custom-map" {
                "sector"
            } else {
                "district"
            }
            || !codes.contains(&b.code)
            || b.name.trim().is_empty()
            || b.name.len() > 200
            || captured.contains_key(&b.code)
        {
            return Err("Map boundary identity does not match the requested scope".into());
        }
        validate_area(&b.geometry)?;
        captured.insert(b.code.clone(), b);
    }
    if captured.len() != codes.len() {
        return Err("Map boundary capture is incomplete".into());
    }
    let areas = if r.family == "custom-map" {
        let full = super::custom::prepare_area(
            &r.project,
            &r.id,
            &codes,
            captured.into_values().collect(),
        )?;
        vec![(
            codes.join("-"),
            full["sectors"]["features"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["properties"]["name"].as_str().unwrap())
                .collect::<Vec<_>>()
                .join(" · "),
            full["area"].clone(),
            full["sectors"].clone(),
        )]
    } else {
        captured.into_values().map(|b| (b.code.clone(),b.name.clone(),b.geometry.clone(),json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":b.name,"code":b.code},"geometry":b.geometry}]}))).collect()
    };
    let templates = crate::report_formats::named_layouts(sheets)?;
    let base = templates
        .values()
        .find(|l| l.paper.size == r.paper)
        .or_else(|| templates.values().next());
    let mut layout = if let Some(layout) = &r.layout {
        layout.clone()
    } else {
        let mut l = named_layout(
            Some(r.id.clone()),
            Some(r.id.replace('_', " ")),
            r.paper,
            Orientation::Landscape,
        )?;
        let [w, h] = l.page_mm;
        let sidebar = if r.paper == Paper::A0 { 100. } else { 80. };
        for e in &mut l.elements {
            e.flow = None;
            match e.id.as_str() {
                "map" => e.rect = [12., 32., w - sidebar - 34., h - 56.],
                "title" => {
                    e.rect = [12., 9., w - 24., 18.];
                    e.font_pt = if r.paper == Paper::A0 { 28. } else { 18. };
                }
                "legend" => {
                    e.rect = [w - sidebar - 12., 43., sidebar, h - 170.];
                    e.font_pt = 9.;
                }
                "north" => e.rect = [w - 42., h - 114., 20., 27.],
                "scale" => {
                    e.rect = [20., h - 44., 78., 12.];
                    e.font_pt = 8.;
                }
                "project" => {
                    e.rect = [w - sidebar - 12., h - 75., sidebar, 48.];
                    e.font_pt = 8.;
                }
                "footer" => {
                    e.rect = [12., h - 16., w - 24., 10.];
                    e.font_pt = 8.;
                }
                _ => {}
            }
        }
        if let Some(base) = base {
            l.styles = base.styles.clone();
            l.style_refs = base.style_refs.clone();
            l.context_layers = base.context_layers.clone();
            l.layer_order = base.layer_order.clone();
            l.style_overrides = base.style_overrides.clone();
        }
        l.styles.insert(
            "print_area".into(),
            PrintStyle {
                stroke: "#536B50".into(),
                width_mm: 0.45,
                fill_opacity: 0.,
                label_field: "name".into(),
                label_pt: 12.,
                ..Default::default()
            },
        );
        l.layer_order.push("print_area".into());
        l.style_refs.remove("print_area");
        l
    };
    layout.id = r.id.clone();
    // Default district overviews omit fine context. An authored recipe is
    // authoritative at every paper size, including custom A3 compositions.
    layout.context_layers.retain(|c| {
        !matches!(c.source, PrintContextSource::ProjectDsgridMv { .. })
            && (!(r.family == "mv-map" && r.layout.is_none())
                || !matches!(
                    c.source,
                    PrintContextSource::GoogleOpenBuildings { .. }
                        | PrintContextSource::ElevationContours { .. }
                ))
    });
    // Village names may remain useful; boundary strokes do not.
    for (id, style) in &mut layout.styles {
        if id.contains("village") {
            style.width_mm = 0.;
            style.stroke_opacity = 0.;
        }
    }

    validate(&layout)?;
    let frame = layout
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Map)
        .ok_or("Map layout has no viewport")?
        .rect;
    let pages = areas
        .into_iter()
        .map(|(code, name, area, boundaries)| {
            let bounds = area_bounds(&area)?;
            // Expand the acquisition rectangle to the entire Mercator viewport,
            // including the aspect-ratio margins which used to print blank.
            let project_y =
                |lat: f64| ((std::f64::consts::PI / 4. + lat.to_radians() / 2.).tan()).ln();
            let inverse_y = |y: f64| (2. * y.exp().atan() - std::f64::consts::PI / 2.).to_degrees();
            let cx = (bounds[0] + bounds[2]) / 2.;
            let cy = (project_y(bounds[1]) + project_y(bounds[3])) / 2.;
            let mut dx = (bounds[2] - bounds[0]).to_radians() * 1.06;
            let mut dy = (project_y(bounds[3]) - project_y(bounds[1])) * 1.06;
            let ratio = frame[2] / frame[3];
            dx = dx.max(dy * ratio);
            dy = dy.max(dx / ratio);
            let extent = [
                cx - (dx / 2.).to_degrees(),
                inverse_y(cy - dy / 2.),
                cx + (dx / 2.).to_degrees(),
                inverse_y(cy + dy / 2.),
            ];
            let [w, s, e, n] = extent;
            Ok(
                json!({"code":code,"name":name,"area":area,"boundaries":boundaries,"extent":extent,
            "capture_geometry":{"type":"Polygon","coordinates":[[[w,s],[e,s],[e,n],[w,n],[w,s]]]}}),
            )
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(
        json!({"request":r,"layout":layout,"pages":pages,"filename":format!("{}.{}-landscape.pdf",r.id,if r.paper==Paper::A0 {"a0"} else {"a3"}),"family":r.family}),
    )
}

/// Merge all captured sources in canonical order before selecting the area.
/// Qualified feature IDs retain source identity; no collision drops a model.
pub fn select_layers(area: &Value, sources: &BTreeMap<String, Value>) -> Result<Value, String> {
    validate_area(area)?;
    let mut merged: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut count = 0;
    for (source, layers) in sources {
        for (name, collection) in layers
            .as_object()
            .ok_or("Source layers must be an object")?
        {
            for f in collection["features"]
                .as_array()
                .ok_or("Source layer must be a FeatureCollection")?
            {
                count += 1;
                if count > 200_000 {
                    return Err("Map source inventory exceeds 200,000 features".into());
                }
                if f["geometry"].is_null() || !intersects_area(&f["geometry"], area)? {
                    continue;
                }
                let mut f = f.clone();
                if !f["properties"].is_object() {
                    f["properties"] = json!({});
                }
                f["properties"]["print_source"] = json!(source);
                if !f["id"].is_null() {
                    f["id"] = json!(format!("{source}:{}", f["id"]));
                }
                merged.entry(name.clone()).or_default().push(f);
            }
        }
    }
    Ok(merged
        .into_iter()
        .map(|(id, features)| (id, json!({"type":"FeatureCollection","features":features})))
        .collect::<serde_json::Map<_, _>>()
        .into())
}

pub fn current(rows: &[Value], item: &Value) -> bool {
    rows.iter().any(|row| {
        row["project"] == item["project"]
            && row["filename"] == item["filename"]
            && row["sha256"] == item["sha256"]
            && row["createdAt"] == item["createdAt"]
    })
}

/// Replace only one canonical filename. Never remove unselected paper outputs.
pub fn catalog(rows: Vec<Value>, item: Value) -> Result<Value, String> {
    if rows.len() > 500
        || item["filename"].as_str().is_none()
        || item["project"].as_str().is_none()
        || item["sha256"]
            .as_str()
            .is_none_or(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("Invalid local map artifact inventory".into());
    }
    if rows.iter().any(|row| {
        row["project"] == item["project"]
            && row["filename"] == item["filename"]
            && row["createdAt"].as_u64().unwrap_or(0) > item["createdAt"].as_u64().unwrap_or(0)
    }) {
        return Ok(json!(rows));
    }
    let mut next = Vec::new();
    for row in rows {
        if row["project"] != item["project"] {
            return Err("Map catalog project mismatch".into());
        }
        if row["filename"] != item["filename"] {
            next.push(row);
        }
    }
    next.push(item);
    next.sort_by(|a, b| a["filename"].as_str().cmp(&b["filename"].as_str()));
    Ok(json!(next))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub label: String,
    pub revision_id: String,
    pub digest: String,
}
pub fn mv_layers(projection: Value, model: Model, bounds: [f64; 4]) -> Result<Value, String> {
    let [w, s, e, n] = bounds;
    let area = json!({"type":"Polygon","coordinates":[[[w,s],[e,s],[e,n],[w,n],[w,s]]]});
    validate_area(&area)?;
    let features = projection["features"]
        .as_array()
        .ok_or("MV projection must contain features")?;
    if features.len() > 200_000 {
        return Err("MV projection exceeds 200,000 features".into());
    }
    let mut lines = Vec::new();
    let mut poles = Vec::new();
    let mut angle_points = Vec::new();
    for feature in features {
        let layer = feature["properties"]["_layer"].as_str().unwrap_or("");
        if !matches!(layer, "alignments" | "structures" | "angle_points")
            || feature["geometry"].is_null()
            || !intersects_area(&feature["geometry"], &area)?
        {
            continue;
        }
        let mut f = feature.clone();
        let p = f["properties"]
            .as_object_mut()
            .ok_or("MV properties missing")?;
        for (key, value) in [
            ("print_context_source", "project_dsgrid_mv"),
            ("model_id", &model.id),
            ("model_label", &model.label),
            ("model_revision_id", &model.revision_id),
            ("model_digest", &model.digest),
        ] {
            p.insert(key.into(), json!(value));
        }
        if !f["id"].is_null() {
            f["id"] = json!(format!("{}:{}", model.id, f["id"]));
        }
        if layer == "alignments" {
            lines.push(f);
        } else if layer == "structures" {
            poles.push(f);
        } else {
            angle_points.push(f);
        }
    }
    Ok(
        json!({"dsgrid_mv_lines":{"type":"FeatureCollection","features":lines},"dsgrid_mv_structures":{"type":"FeatureCollection","features":poles},"dsgrid_mv_angle_points":{"type":"FeatureCollection","features":angle_points}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn boundary(code: &str, level: &str, x: f64) -> Boundary {
        Boundary {
            code: code.into(),
            level: level.into(),
            name: format!("Area {code}"),
            geometry: json!({"type":"Polygon","coordinates":[[[x,-2.],[x+0.1,-2.],[x+0.1,-1.9],[x,-1.9],[x,-2.]]]}),
        }
    }
    fn request(family: &str, codes: &[&str]) -> Request {
        Request {
            project: "project".into(),
            id: "map".into(),
            family: family.into(),
            codes: codes.iter().map(|s| s.to_string()).collect(),
            paper: Paper::A3,
            dpi: default_dpi(),
            layout: None,
        }
    }
    #[test]
    fn mv_print_preserves_canonical_angle_point_roles_and_numbers() {
        let features = json!({"features":[{"type":"Feature","id":"node-a","geometry":{"type":"Point","coordinates":[29.5,-2.]},"properties":{"_layer":"angle_points","point_role":"angle","number":"PI-17","tap_type":"transformer","tr_name":"Agatare","struct_no":"TR-4"}}]});
        let model = Model {
            id: "model-a".into(),
            label: "A".into(),
            revision_id: "rev".into(),
            digest: "a".repeat(64),
        };
        let layers = mv_layers(features.clone(), model, [29., -3., 30., -1.]).unwrap();
        let p = &layers["dsgrid_mv_angle_points"]["features"][0]["properties"];
        for field in ["point_role", "number", "tap_type", "tr_name", "struct_no"] {
            assert_eq!(p[field], features["features"][0]["properties"][field]);
        }
        assert_eq!(
            layers["dsgrid_mv_angle_points"]["features"][0]["id"],
            "model-a:\"node-a\""
        );
        assert_eq!(
            layers["dsgrid_mv_structures"]["features"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
    #[test]
    fn authored_district_context_survives_paper_planning() {
        let mut r = request("mv-map", &["24"]);
        let mut layout =
            named_layout(Some("map".into()), None, Paper::A3, Orientation::Landscape).unwrap();
        layout.context_layers = vec![
            PrintContextLayer {
                id: "buildings".into(),
                label: "Buildings".into(),
                source: PrintContextSource::GoogleOpenBuildings {
                    buffer_m: 0.,
                    min_area_m2: 10.,
                    min_confidence: 0.5,
                },
            },
            PrintContextLayer {
                id: "contours".into(),
                label: "Contours".into(),
                source: PrintContextSource::ElevationContours {
                    buffer_m: 0.,
                    minor_interval_m: 10,
                    index_interval_m: 50,
                    sample_spacing_m: 10.,
                },
            },
        ];
        r.layout = Some(layout.clone());
        let p = plan(&r, vec![boundary("24", "district", 29.)], &json!({})).unwrap();
        assert_eq!(
            p["layout"]["context_layers"],
            serde_json::to_value(&layout.context_layers).unwrap()
        );
    }
    #[test]
    fn dpi_defaults_and_roundtrips_without_changing_output_identity() {
        let mut raw =
            json!({"project":"project","id":"map","family":"mv-map","codes":["24"],"paper":"A3"});
        let r: Request = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(r.dpi, 300);
        for dpi in [72, 150, 300, 600, 1200] {
            raw["dpi"] = json!(dpi);
            let r: Request = serde_json::from_value(raw.clone()).unwrap();
            let p = plan(&r, vec![boundary("24", "district", 29.)], &json!({})).unwrap();
            assert_eq!(p["request"]["dpi"], dpi);
            assert_eq!(p["filename"], "map.a3-landscape.pdf");
        }
        for dpi in [0, 71, 1201] {
            raw["dpi"] = json!(dpi);
            assert!(validate_request(&serde_json::from_value(raw.clone()).unwrap()).is_err());
        }
    }
    #[test]
    fn mv_pages_are_district_ordered_and_capture_full_page_margins() {
        let p = plan(
            &request("mv-map", &["25", "24"]),
            vec![
                boundary("25", "district", 30.),
                boundary("24", "district", 29.),
            ],
            &json!({}),
        )
        .unwrap();
        assert_eq!(p["pages"][0]["code"], "24");
        assert_eq!(p["pages"][1]["code"], "25");
        assert_eq!(p["filename"], "map.a3-landscape.pdf");
        let extent = p["pages"][0]["extent"].as_array().unwrap();
        assert!(extent[0].as_f64().unwrap() < 29.);
        assert!(extent[2].as_f64().unwrap() > 29.1);
        assert!(plan(
            &request("mv-map", &["24", "25"]),
            vec![boundary("24", "district", 29.)],
            &json!({})
        )
        .is_err());
    }
    #[test]
    fn custom_scope_preserves_connected_sector_membership() {
        let p = plan(
            &request("custom-map", &["2401", "2402"]),
            vec![
                boundary("2401", "sector", 29.),
                boundary("2402", "sector", 29.1),
            ],
            &json!({}),
        )
        .unwrap();
        assert_eq!(p["pages"].as_array().unwrap().len(), 1);
        assert_eq!(
            p["pages"][0]["boundaries"]["features"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(plan(
            &request("custom-map", &["2401", "2402"]),
            vec![
                boundary("2401", "sector", 29.),
                boundary("2402", "sector", 30.)
            ],
            &json!({})
        )
        .is_err());
    }
    #[test]
    fn merging_same_ids_from_two_models_preserves_both_and_filters_outside() {
        let f = json!({"type":"Feature","id":"pole-1","geometry":{"type":"Point","coordinates":[29.05,-1.95]},"properties":{}});
        let layer = json!({"poles":{"type":"FeatureCollection","features":[f]}});
        let p = select_layers(
            &boundary("24", "district", 29.).geometry,
            &BTreeMap::from([("model-a".into(), layer.clone()), ("model-b".into(), layer)]),
        )
        .unwrap();
        assert_eq!(p["poles"]["features"].as_array().unwrap().len(), 2);
        assert_ne!(
            p["poles"]["features"][0]["id"],
            p["poles"]["features"][1]["id"]
        );
    }
    #[test]
    fn selected_output_overwrite_keeps_other_paper_and_rejects_other_project() {
        let item = |name: &str| json!({"project":"p","filename":name,"sha256":"a".repeat(64)});
        let p = catalog(vec![item("a0.pdf"), item("a3.pdf")], item("a3.pdf")).unwrap();
        assert_eq!(p.as_array().unwrap().len(), 2);
        let mut wrong = item("a3.pdf");
        wrong["project"] = json!("other");
        assert!(catalog(vec![wrong], item("a3.pdf")).is_err());
    }
}
