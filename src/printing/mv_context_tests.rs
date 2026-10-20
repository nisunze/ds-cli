use super::*;
use crate::printing::mv_booklet;

fn layer(id: &str, features: usize) -> VectorLayer {
    let feature = json!({"type":"Feature","properties":{"name":"T1"},
        "geometry":{"type":"Point","coordinates":[29.7,-2.6]}});
    VectorLayer {
        id: id.into(),
        label: id.into(),
        geojson: json!({"type":"FeatureCollection","features":vec![feature; features]}),
    }
}

fn styles(ids: &[&str]) -> Snapshot {
    let bindings = ids
        .iter()
        .map(|id| {
            let document = json!({"type":"line","paint":{"line-color":"#3a6ea5"},"layout":{}});
            let digest = crate::style_plan::style_digest(&document);
            Binding {
                key: style_key(id).unwrap(),
                style_ref: format!("print_context/{id}_print"),
                revision_id: digest.clone(),
                content_sha256: digest,
                scope: "catalogue".into(),
                document,
                presentation_tier: None,
            }
        })
        .collect();
    Snapshot {
        schema: crate::style_resolution::SCHEMA.into(),
        project_id: "project".into(),
        revision_id: "a".repeat(64),
        seed_version: None,
        style_presets: None,
        missing_style_refs: vec![],
        bindings,
    }
}

#[test]
fn context_is_pinned_to_its_sources_and_alignment_not_model_revision_or_sheet_position() {
    let render =
        serde_json::json!({"layers":[{"id":"lv_lines","geojson":{"features":[]}}],"layout":{}});
    let mut capture = Capture {
        schema: SCHEMA.into(),
        project_id: "p".into(),
        alignment_id: "al-22".into(),
        context_sha256: mv_booklet::sha256(&serde_json::to_vec(&render).unwrap()),
        render,
    };
    capture.validate("p").unwrap();
    assert!(capture.validate("other").is_err());
    capture.render["layers"][0]["geojson"]["features"] = serde_json::json!([{"id":"changed"}]);
    assert!(capture.validate("p").is_err());
}

#[test]
fn every_carried_family_prints_in_the_mv_context_role_by_its_source_kind() {
    for (id, kind) in [
        ("roads", "cold_existing"),
        ("rivers", "cold_existing"),
        ("rwanda_settlement_sites", "cold_existing"),
        ("lv_line", "cold_existing"),
        ("hv_line", "cold_existing"),
        ("mv_line", "cold_existing"),
        ("places", "cold_existing"),
        ("buildings_context", "dataset_geojson"),
        ("contours_intermediate", "dataset_geojson"),
        ("contours_index", "dataset_geojson"),
        ("lv_lines", "saved_design"),
        ("tr", "saved_design"),
    ] {
        let key = style_key(id).unwrap();
        assert_eq!(
            (key.entity_class.as_str(), key.source_kind.as_str()),
            (id, kind)
        );
        assert_eq!(
            (key.target, key.role.as_str()),
            (Target::Print, "mv_context")
        );
    }
    // A family the booklet does not declare is never styled by a guess.
    for id in [
        "customers",
        "service_cables",
        "dsgrid_mv_lines",
        "village_boundaries",
    ] {
        assert!(style_key(id)
            .unwrap_err()
            .starts_with("mv_context_family_unknown"));
    }
}

#[test]
fn sheets_read_held_families_and_terrain_while_the_key_plan_reads_its_overview() {
    let resource = |layer: &str, ready: bool| CatalogResource {
        layer: layer.into(),
        label: layer.into(),
        country: "Rwanda".into(),
        ready,
        ..Default::default()
    };
    let held = [
        resource("roads", true),
        resource("rivers", true),
        resource("rwanda_settlement_sites", true),
        resource("lv_line", false),
        resource("hv_line", true),
        resource("places", true),
        resource("village_boundaries", true),
    ];
    let sheet: Vec<String> = sheet_sources(&held).into_iter().map(|l| l.id).collect();
    assert_eq!(
        sheet,
        [
            "roads",
            "rivers",
            "rwanda_settlement_sites",
            "buildings_context",
            "elevation_contours"
        ]
    );
    let key_plan: Vec<String> = key_plan_sources(&held).into_iter().map(|l| l.id).collect();
    assert_eq!(key_plan, ["roads", "hv_line", "places"]);
}

#[test]
fn assembled_sheet_context_is_admitted_with_exact_keys_and_api_replies_back_to_front() {
    let ids = ["tr", "roads", "lv_lines", "contours_index", "rivers"];
    let snapshot = styles(&ids);
    let layers = vec![
        layer("tr", 1),
        layer("roads", 2),
        layer("lv_lines", 1),
        layer("contours_index", 1),
        layer("rivers", 0),
    ];
    let capture = assemble("project", "aln-1", layers, &snapshot).unwrap();
    capture.validate("project").unwrap();
    let render = &capture.render;
    let carried: Vec<&str> = render["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect();
    // Empty layers are not carried; terrain is drawn first, the network last.
    assert_eq!(carried, ["contours_index", "roads", "lv_lines", "tr"]);
    assert_eq!(
        render["layout"]["layer_order"],
        json!(["contours_index", "roads", "lv_lines", "tr"])
    );
    for id in carried {
        let key: Key = serde_json::from_value(render["style_sources"][id].clone()).unwrap();
        let reply: Binding =
            serde_json::from_value(render["style_resolutions"][id].clone()).unwrap();
        assert_eq!(key, style_key(id).unwrap());
        crate::style_resolution::admit_binding(&reply, &key).unwrap();
        assert_eq!(render["layout"]["style_keys"][id], json!(key));
    }
    assert_eq!(render["layout"]["page_mm"], json!([420.0, 297.0]));
    let elements = render["layout"]["elements"].as_array().unwrap();
    assert!(elements.iter().any(|e| e["kind"] == "map"));
    assert_eq!(
        elements.iter().find(|e| e["id"] == "legend").unwrap()["legend"]["titles"]["lv_lines"],
        "New LV"
    );
    // Same held bytes, same capture: the digest pins sources and recipe.
    let again = assemble(
        "project",
        "aln-1",
        vec![
            layer("tr", 1),
            layer("roads", 2),
            layer("lv_lines", 1),
            layer("contours_index", 1),
        ],
        &snapshot,
    )
    .unwrap();
    assert_eq!(again.context_sha256, capture.context_sha256);
}

#[test]
fn an_unresolved_family_or_captured_mv_geometry_refuses_instead_of_printing_unstyled() {
    let snapshot = styles(&["roads"]);
    let refused = assemble("project", "aln-1", vec![layer("rivers", 1)], &snapshot).unwrap_err();
    assert!(refused.contains("style_resolution_unknown"), "{refused}");
    let refused = assemble(
        "project",
        "aln-1",
        vec![layer("dsgrid_mv_lines", 1)],
        &snapshot,
    )
    .unwrap_err();
    assert!(
        refused.starts_with("mv_context_family_unknown"),
        "{refused}"
    );
}

#[test]
fn the_key_plan_capture_is_one_model_overview_pinned_to_its_sources() {
    let snapshot = styles(&["roads", "tr"]);
    let capture = assemble_key_plan(
        "project",
        vec![layer("roads", 3), layer("tr", 2)],
        &snapshot,
    )
    .unwrap();
    assert_eq!(capture.schema, KEY_PLAN_SCHEMA);
    capture.validate("project").unwrap();
    assert!(capture.validate("other").is_err());
    let mut changed = capture.clone();
    changed.render["layers"][0]["geojson"]["features"] = json!([]);
    assert!(changed.validate("project").is_err());
}

#[test]
fn the_key_plan_names_transformers_and_places_while_sheets_name_nothing_extra() {
    let snapshot = styles(&["places", "tr", "roads"]);
    let key_plan = assemble_key_plan(
        "project",
        vec![layer("tr", 2), layer("places", 1), layer("roads", 1)],
        &snapshot,
    )
    .unwrap();
    let overrides = &key_plan.render["layout"]["style_overrides"];
    for id in ["tr", "places"] {
        assert_eq!(overrides[id]["label"]["field"], "name");
        assert_eq!(overrides[id]["label"]["visible"], true);
        assert_eq!(overrides[id]["label"]["size_pt"], 7.0);
    }
    assert!(overrides.get("roads").is_none());
    let sheet = assemble("project", "aln-1", vec![layer("tr", 1)], &snapshot).unwrap();
    assert_eq!(sheet.render["layout"]["style_overrides"], json!({}));
}

#[test]
fn a_key_plan_frames_its_routes_with_a_margin_and_never_less_than_two_kilometres() {
    let framed = key_plan_extent([29.80, -2.64, 29.90, -2.60]);
    let width = framed[2] - framed[0];
    assert!((width - 0.1 * (1. + 2. * KEY_PLAN_MARGIN)).abs() < 1e-9);
    assert!(((framed[0] + framed[2]) / 2. - 29.85).abs() < 1e-9);
    // A six-metre substation link still shows its surroundings.
    let short = key_plan_extent([29.80676, -2.62650, 29.80681, -2.62648]);
    assert!(short[2] - short[0] >= KEY_PLAN_MIN_SPAN_DEG);
    assert!(short[3] - short[1] >= KEY_PLAN_MIN_SPAN_DEG);
}
