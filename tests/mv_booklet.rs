use ds_command_kernel::printing::{self, mv, mv_booklet};
use serde_json::json;
use std::collections::BTreeMap;

fn approved_input() -> serde_json::Value {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/mv-printing-setup.json")).unwrap();
    let mut naming = value["printing_setups"][0]["layout"]["mv"]["pages"][0].clone();
    naming["layout"]["id"] = json!("test_naming");
    naming["layout"]["name"] = json!("TEST ONLY approved naming");
    value["printing_setups"][0]["layout"]["mv"]["pages"]
        .as_array_mut()
        .unwrap()
        .insert(1, naming);
    for key in [
        "contract_title",
        "formal_client",
        "formal_employer",
        "formal_consultant",
        "formal_contractor",
    ] {
        value["printing_setups"][0]["layout"]["title_block"][key] =
            json!(format!("TEST ONLY {key}"));
    }
    value
}

#[test]
fn standard_preserves_approved_pages_and_project_facts_with_no_print_options() {
    let mut input = approved_input();
    let original: printing::Layout =
        serde_json::from_value(input["printing_setups"][0]["layout"].clone()).unwrap();
    let standardized = mv::standardize(original.clone()).unwrap();
    let before = original.mv.unwrap();
    let after = standardized.mv.as_ref().unwrap();
    assert_eq!(&after.pages[..2], &before.pages[..2]);
    assert_eq!(
        after.settings.title_employer,
        before.settings.title_employer
    );
    assert_eq!(
        standardized.assets,
        serde_json::from_value::<BTreeMap<String, printing::Asset>>(
            input["printing_setups"][0]["layout"]["assets"].clone()
        )
        .unwrap()
    );
    assert_eq!(after.pages.len(), 6);
    assert_eq!(after.settings.horizontal_scale, Some(1500));
    assert_eq!(after.settings.vertical_scale, Some(500));
    input["printing_setups"][0]["layout"] = serde_json::to_value(&standardized).unwrap();
    let setup = mv::resolve(&input, "project_a", BTreeMap::new()).unwrap();
    let pages = vec![mv_booklet::Page {
        id: "cover".into(),
        role: mv_booklet::PageRole::Cover,
        content_sha256: mv_booklet::sha256(b"approved page"),
    }];
    let first = mv_booklet::plan(&setup, "rev_fixture", pages.clone()).unwrap();
    assert_eq!(
        first,
        mv_booklet::plan(&setup, "rev_fixture", pages.clone()).unwrap()
    );
    assert_ne!(
        first.sha256,
        mv_booklet::plan(&setup, "rev_next", pages).unwrap().sha256
    );
    assert_eq!(first.external_version, "v3");
    let command = json!({"op":"mv_booklet_plan","setup":setup,"model_revision":"rev_fixture","pages":first.pages});
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &printing::evaluate(&serde_json::to_vec(&command).unwrap()).unwrap()
        )
        .unwrap(),
        json!(first)
    );
}

#[test]
fn portable_preview_checks_cover_bytes_dimensions_order_and_plan_identity() {
    use base64::Engine as _;
    let input = approved_input();
    let setup = mv::resolve(&input, "project_a", BTreeMap::new()).unwrap();
    let plan = mv_booklet::plan(
        &setup,
        "rev_fixture",
        vec![mv_booklet::Page {
            id: "cover".into(),
            role: mv_booklet::PageRole::Cover,
            content_sha256: mv_booklet::sha256(b"approved"),
        }],
    )
    .unwrap();
    let image = image::RgbaImage::from_pixel(1587, 1123, image::Rgba([250, 250, 250, 255]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();
    let preview = mv_booklet::Preview {
        page_id: "cover".into(),
        png_sha256: mv_booklet::sha256(&bytes),
        raster_sha256: mv_booklet::sha256(image.as_raw()),
        png_data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ),
    };
    let command = json!({"op":"mv_booklet_preview","plan":plan,"previews":[preview.clone()]});
    let native = printing::evaluate(&serde_json::to_vec(&command).unwrap()).unwrap();
    #[cfg(feature = "wasm")]
    {
        let portable =
            ds_command_kernel::printing_command(&serde_json::to_vec(&command).unwrap()).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&native).unwrap(),
            serde_json::from_str::<serde_json::Value>(&portable).unwrap()
        );
    }
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&native).unwrap()["plan"],
        json!(plan)
    );
    let mut changed = preview;
    changed.page_id = "other".into();
    assert!(mv_booklet::preview_document(&plan, &[changed]).is_err());
    let mut changed = plan.clone();
    changed.model_revision = "other".into();
    assert!(mv_booklet::preview_document(&changed, &[]).is_err());
}

#[test]
fn standard_refuses_missing_parties_and_retains_optional_context_on_reapplication() {
    let mut input = approved_input();
    let mut context = input["printing_setups"][0]["layout"]["mv"]["pages"][0].clone();
    context["layout"]["id"] = json!("test_context");
    input["printing_setups"][0]["layout"]["mv"]["pages"]
        .as_array_mut()
        .unwrap()
        .push(context.clone());
    let layout: printing::Layout =
        serde_json::from_value(input["printing_setups"][0]["layout"].clone()).unwrap();
    let standard = mv::standardize(layout).unwrap();
    let expected: mv::Page = serde_json::from_value(context).unwrap();
    assert_eq!(
        standard.mv.as_ref().unwrap().pages.last().unwrap(),
        &expected
    );
    assert_eq!(mv::standardize(standard.clone()).unwrap(), standard);
    input["printing_setups"][0]["layout"]["title_block"]
        .as_object_mut()
        .unwrap()
        .remove("formal_client");
    let layout = serde_json::from_value(input["printing_setups"][0]["layout"].clone()).unwrap();
    input["printing_setups"][0]["layout"] =
        serde_json::to_value(mv::standardize(layout).unwrap()).unwrap();
    assert!(mv::resolve(&input, "project_a", BTreeMap::new()).is_err());
}

#[test]
fn mv_acquisition_resolves_paper_and_policy_from_one_project_table() {
    use ds_command_kernel::{
        style_plan,
        style_resolution::{self, Binding, Key, Snapshot},
    };
    let mut setup = mv::resolve(&approved_input(), "project_a", BTreeMap::new()).unwrap();
    setup.settings.ink_mode = printing::mv_settings::InkMode::ReferenceAccents;
    let paper_key = mv::paper_style_key(&setup.settings);
    let binding = |key: Key, name: &str, document: serde_json::Value| Binding {
        key,
        style_ref: name.into(),
        revision_id: "b".repeat(64),
        content_sha256: style_plan::style_digest(&document),
        scope: "global".into(),
        presentation_tier: None,
        document,
    };
    let paper = binding(
        paper_key,
        "fixture/mv_print",
        json!({"type":"line","paint":{"line-color":"#123456","line-width":1}}),
    );
    let renderer = binding(
        printing::renderer_defaults::key(),
        "fixture/renderer_print",
        json!({
            "type":"line","paint":{},"metadata":{"print_renderer_defaults":serde_json::from_str::<serde_json::Value>(include_str!("fixtures/governed-renderer-defaults.json")).unwrap()}
        }),
    );
    let mut table: Snapshot = serde_json::from_value(json!({
        "schema":style_resolution::SCHEMA,"project_id":"project_a","revision_id":"a".repeat(64),
        "bindings":[paper,renderer]
    }))
    .unwrap();
    let selected = mv::resolve_print_bindings(&setup, &table).unwrap();
    assert_eq!(selected.0.style_ref, "fixture/mv_print");
    assert_eq!(selected.1.style_ref, "fixture/renderer_print");
    let mut mono_setup = setup.clone();
    mono_setup.settings.ink_mode = printing::mv_settings::InkMode::Monochrome;
    assert!(mv::resolve_print_bindings(&mono_setup, &table)
        .unwrap_err()
        .contains("style_resolution_unknown"));
    let mut monochrome = selected.0.clone();
    monochrome.key = mv::paper_style_key(&mono_setup.settings);
    monochrome.style_ref = "fixture/mv_monochrome".into();
    table.bindings.push(monochrome);
    assert_eq!(
        mv::resolve_print_bindings(&mono_setup, &table)
            .unwrap()
            .0
            .style_ref,
        "fixture/mv_monochrome"
    );
    table.bindings.pop();
    table.project_id = "other_project".into();
    assert!(mv::resolve_print_bindings(&setup, &table)
        .unwrap_err()
        .starts_with("project_context_changed:"));
    table.project_id = "project_a".into();
    table.bindings.pop();
    assert!(mv::resolve_print_bindings(&setup, &table)
        .unwrap_err()
        .contains("style_resolution_unknown"));
}
