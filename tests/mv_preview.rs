use ds_command_kernel::printing::{self, mv, mv_preview, Layout};
use ds_command_kernel::{
    style_plan,
    style_resolution::{self, Binding, Key, Snapshot},
};
use serde_json::{json, Value};
use std::path::Path;

fn canonical() -> Layout {
    serde_json::from_str(include_str!("fixtures/mv-global-canonical.json")).unwrap()
}

fn sheets(selection: Value) -> Value {
    json!({
        "project_settings": [],
        "printing_defaults": {"schema": "ds.printing-defaults/v1", "mv_selection": selection},
        "report_context": {"project_title": "Template MV Design"},
    })
}

fn default_selection() -> Value {
    json!({"schema": mv::SELECTION_SCHEMA, "layout_id": "mv-a3-plan-profile-canonical-20261002",
        "revision": "a".repeat(64), "external_version": "1", "issue_date": "2026-10-02"})
}

fn table(project: &str, layout: &Layout) -> Snapshot {
    let settings = &layout.mv.as_ref().unwrap().settings;
    let binding = |key: Key, name: &str, document: Value| Binding {
        key,
        style_ref: name.into(),
        revision_id: "b".repeat(64),
        content_sha256: style_plan::style_digest(&document),
        scope: "global".into(),
        presentation_tier: None,
        document,
    };
    let paper = binding(
        mv::paper_style_key(settings),
        "fixture/mv_print",
        json!({"type":"line","paint":{"line-color":"#123456","line-width":1}}),
    );
    let renderer = binding(
        printing::renderer_defaults::key(),
        "fixture/renderer_print",
        json!({"type":"line","paint":{},"metadata":{"print_renderer_defaults":
            serde_json::from_str::<Value>(include_str!("fixtures/governed-renderer-defaults.json")).unwrap()}}),
    );
    serde_json::from_value(
        json!({"schema": style_resolution::SCHEMA, "project_id": project,
        "revision_id": "c".repeat(64), "bindings": [paper, renderer]}),
    )
    .unwrap()
}

fn paths() -> mv_preview::Paths<'static> {
    mv_preview::Paths {
        scene: Path::new("/held/scene.json"),
        plan: Path::new("/held/plan.json"),
        out_dir: Path::new("/held/out"),
    }
}

#[test]
fn draft_template_previews_one_sheet_under_the_project_issue_identity() {
    let layout = canonical();
    let input = sheets(default_selection());
    let preview = mv_preview::request(
        "template_project",
        &input,
        &layout,
        &table("template_project", &layout),
        None,
        Some("EPSG:32735"),
        paths(),
    )
    .unwrap();
    let request = &preview.request;
    assert_eq!(request["preview_only"], true);
    assert_eq!(request["sample_pages"], mv_preview::SAMPLE_PAGES);
    assert_eq!(request["publication_assets"], json!({}));
    assert_eq!(request["context_page_files"], json!([]));
    assert_eq!(request["scene_path"], "/held/scene.json");
    assert_eq!(request["model_crs"], "EPSG:32735");
    assert_eq!(request["style_resolution"]["style_ref"], "fixture/mv_print");
    assert_eq!(
        request["renderer_defaults"]["style_ref"],
        "fixture/renderer_print"
    );
    let selection = &preview.setup.selection;
    assert_eq!(selection.layout_id, layout.id);
    assert_eq!(selection.revision, mv_preview::revision(&layout).unwrap());
    assert_eq!(selection.external_version, "1");
    assert_eq!(selection.issue_date, "2026-10-02");
    assert!(preview.setup.preserved_pages.is_empty());
    // A previewed project template prints its own authored text.
    assert_eq!(
        preview.setup.settings.project_title,
        layout.mv.as_ref().unwrap().settings.project_title
    );
    assert_eq!(
        request["settings"],
        serde_json::to_value(&preview.setup.settings).unwrap()
    );
    assert_eq!(
        request["report_config"]["printing_setups"][0]["id"],
        json!(layout.id)
    );
    // The held configuration is read, never rewritten.
    assert_eq!(input, sheets(default_selection()));
}

#[test]
fn explicit_project_selection_supplies_identity_but_not_the_previewed_layout() {
    let layout = canonical();
    let mut input = sheets(default_selection());
    input["project_settings"] = json!([{"parameter": mv::SETTING, "value": {
        "schema": mv::SELECTION_SCHEMA, "layout_id": "another-mv-setup", "revision": "d".repeat(64),
        "external_version": "Version 3", "issue_date": "2026-09-01"}}]);
    let preview = mv_preview::request(
        "template_project",
        &input,
        &layout,
        &table("template_project", &layout),
        None,
        None,
        paths(),
    )
    .unwrap();
    assert_eq!(preview.setup.selection.layout_id, layout.id);
    assert_eq!(preview.setup.selection.external_version, "Version 3");
    assert_eq!(preview.setup.selection.issue_date, "2026-09-01");
    let rows = preview.request["report_config"]["project_settings"]
        .as_array()
        .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "the previewed selection replaces the stored one in the request only"
    );
}

#[test]
fn previews_refuse_with_keyed_reasons() {
    let layout = canonical();
    let good = table("template_project", &layout);
    let refuse = |sheets: &Value, layout: &Layout, table: &Snapshot| {
        mv_preview::request(
            "template_project",
            sheets,
            layout,
            table,
            None,
            None,
            paths(),
        )
        .unwrap_err()
        .code
    };
    let mut plain = layout.clone();
    plain.mv = None;
    assert_eq!(
        refuse(&sheets(default_selection()), &plain, &good),
        "mv_print_template_invalid"
    );
    assert_eq!(
        refuse(&json!({"project_settings": []}), &layout, &good),
        "mv_print_setup_missing"
    );
    assert_eq!(
        refuse(&Value::Null, &layout, &good),
        "mv_print_setup_missing"
    );
    let foreign = table("another_project", &layout);
    assert_eq!(
        refuse(&sheets(default_selection()), &layout, &foreign),
        mv_preview::STYLE_UNRESOLVED
    );
    let mut unbound = good.clone();
    unbound.bindings.truncate(1);
    assert_eq!(
        refuse(&sheets(default_selection()), &layout, &unbound),
        mv_preview::STYLE_UNRESOLVED
    );
}

#[test]
fn preview_revision_pins_the_rendered_layout_bytes() {
    let layout = canonical();
    let mut edited = layout.clone();
    edited.name = "Edited draft".into();
    assert_ne!(
        mv_preview::revision(&layout).unwrap(),
        mv_preview::revision(&edited).unwrap()
    );
    assert!(mv::valid_revision(&mv_preview::revision(&layout).unwrap()));
}
