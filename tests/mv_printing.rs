//! Synthetic policy evidence; no fixture here is approved project content.
use ds_command_kernel::printing::{
    self,
    mv::{self, ModelField, Selection},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn sheets() -> Value {
    serde_json::from_str(include_str!("fixtures/mv-printing-setup.json")).unwrap()
}
#[test]
fn missing_project_selection_uses_only_the_pinned_global_default() {
    let mut value = sheets();
    let adopted = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap();
    value["printing_defaults"] =
        json!({"schema":"ds.printing-defaults/v1","mv_selection":adopted.selection});
    value["global_printing_setups"] = value["printing_setups"].clone();
    value["project_settings"] = json!([]);
    value["printing_setups"] = json!([]);
    let global = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap();
    assert_eq!(global.settings, adopted.settings);
    assert_eq!(global.layout, adopted.layout);
    assert_eq!(global.inherited_from.id, global.selection.layout_id);
    value["global_printing_setups"][0]["revision"] = json!("c".repeat(64));
    assert_eq!(
        mv::resolve(&value, "project_a", BTreeMap::new())
            .unwrap_err()
            .code,
        "mv_print_setup_revision_changed"
    );
    value["project_settings"] = json!([{"parameter":mv::SETTING,"value":null}]);
    assert!(!mv::uses_global_default(&value));
    assert!(mv::effective_selection(&value).is_err());
}
#[test]
fn selected_adopted_revision_is_the_only_authority_and_never_tracks_global_head() {
    let mut value = sheets();
    let resolved = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap();
    assert_eq!(resolved.settings.drawing_revision, "v3");
    assert_eq!(resolved.settings.drawing_date, "2026-10-01");
    assert_eq!(resolved.inherited_from.revision, "b".repeat(64));
    // A global library row cannot redirect a project's adopted copy.
    value["global_printing_setups"] = json!([{"id":"test_global_mv","revision":"c".repeat(64)}]);
    assert_eq!(
        mv::resolve(&value, "project_a", BTreeMap::new()).unwrap(),
        resolved
    );
    value["printing_setups"][0]["revision"] = json!("c".repeat(64));
    assert_eq!(
        mv::resolve(&value, "project_a", BTreeMap::new())
            .unwrap_err()
            .code,
        "mv_print_setup_revision_changed"
    );
}
#[test]
fn generated_front_matter_child_layouts_require_the_adopted_a3_landscape_paper() {
    let mut value = sheets();
    // The fixture's front-matter child is valid at the adopted native size.
    mv::resolve(&value, "project_a", BTreeMap::new()).unwrap();
    for page_mm in [json!([297, 210]), json!([297, 420])] {
        value["printing_setups"][0]["layout"]["mv"]["pages"][0]["layout"]["page_mm"] = page_mm;
        let refusal = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap_err();
        assert_eq!(refusal.code, "mv_print_template_invalid");
        assert_eq!(refusal.field, "layout");
        assert!(refusal.remedy.contains("A3"));
    }
}

#[test]
fn model_fields_inherit_every_project_choice_and_project_context_is_fenced() {
    let value = sheets();
    let base = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap();
    let fields = BTreeMap::from([
        (ModelField::Title, "TEST ONLY model drawing".into()),
        (ModelField::Identity, "TEST ONLY model identity".into()),
    ]);
    let resolved = mv::resolve(&value, "project_a", fields).unwrap();
    let mut expected = base.settings.clone();
    expected.sheet_title = "TEST ONLY model drawing".into();
    assert_eq!(resolved.settings, expected);
    assert_eq!(resolved.layout, base.layout);
    assert_eq!(resolved.selection, base.selection);
    assert_eq!(resolved.text["sheet_title"], "TEST ONLY model drawing");
    resolved.validate("project_a").unwrap();
    assert_eq!(
        resolved.validate("project_b").unwrap_err().code,
        "mv_print_project_mismatch"
    );
    let mut modified = resolved.clone();
    modified.settings.horizontal_scale = Some(2000);
    assert_eq!(
        modified.validate("project_a").unwrap_err().code,
        "mv_print_receipt_changed"
    );
    let mut forbidden = value.clone();
    forbidden["printing_setups"][0]["layout"]["mv"]["allowed_model_fields"] = json!([]);
    assert_eq!(
        mv::resolve(
            &forbidden,
            "project_a",
            BTreeMap::from([(ModelField::Title, "Different".into())])
        )
        .unwrap_err()
        .code,
        "mv_print_model_override_forbidden"
    );
}
#[test]
fn missing_fields_and_duplicate_selection_are_keyed_refusals_before_output() {
    let mut value = sheets();
    value["project_settings"] = json!([]);
    let refused = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap_err();
    assert_eq!(refused.code, "mv_print_setup_missing");
    assert_eq!(refused.message_key, refused.code);
    assert!(!refused.remedy.is_empty());
    let mut value = sheets();
    let duplicate = value["project_settings"][0].clone();
    value["project_settings"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert_eq!(
        mv::selection(&value).unwrap_err().code,
        "mv_print_setup_missing"
    );
    let mut value = sheets();
    value["printing_setups"][0]["layout"]["mv"]["required_text"] = json!(["contract_number"]);
    let refused = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap_err();
    assert_eq!(refused.code, "mv_print_text_missing");
    assert_eq!(refused.field, "contract_number");
    let mut value = sheets();
    value["printing_setups"][0]["inherited_from"] = json!({});
    assert_eq!(
        mv::resolve(&value, "project_a", BTreeMap::new())
            .unwrap_err()
            .code,
        "mv_print_setup_not_adopted"
    );
}
#[test]
fn copied_project_setup_preserves_its_exact_source_and_selected_revision() {
    let mut value = sheets();
    let source = json!({"scope":"project","project_id":"rwanda_template","id":"source_mv","revision":"b".repeat(64)});
    value["printing_setups"][0]["inherited_from"] = source.clone();
    let resolved = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap();
    assert_eq!(
        serde_json::to_value(&resolved.inherited_from).unwrap(),
        source
    );
    resolved.validate("project_a").unwrap();
    // An unrelated global setup cannot advance the project's independently
    // selected copied revision.
    value["global_printing_setups"] = json!([{"id":"source_mv","revision":"c".repeat(64)}]);
    assert_eq!(
        mv::resolve(&value, "project_a", BTreeMap::new()).unwrap(),
        resolved
    );
    value["printing_setups"][0]["revision"] = json!("c".repeat(64));
    assert_eq!(
        mv::resolve(&value, "project_a", BTreeMap::new())
            .unwrap_err()
            .code,
        "mv_print_setup_revision_changed"
    );
    let mut modified = resolved;
    modified.inherited_from.revision = "d".repeat(64);
    assert_eq!(
        modified.validate("project_a").unwrap_err().code,
        "mv_print_receipt_changed"
    );
}

#[test]
fn authored_project_setup_captures_itself_without_global_lineage() {
    let base = mv::resolve(&sheets(), "project_a", BTreeMap::new()).unwrap();
    for absent in [false, true] {
        let mut value = sheets();
        if absent {
            value["printing_setups"][0]
                .as_object_mut()
                .unwrap()
                .remove("inherited_from");
        } else {
            value["printing_setups"][0]["inherited_from"] = Value::Null;
        }
        let resolved = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap();
        assert_eq!(resolved.inherited_from.scope, "project");
        assert_eq!(
            resolved.inherited_from.project_id.as_deref(),
            Some("project_a")
        );
        assert_eq!(resolved.inherited_from.id, resolved.selection.layout_id);
        assert_eq!(
            resolved.inherited_from.revision,
            resolved.selection.revision
        );
        assert_eq!(resolved.layout, base.layout);
        assert_eq!(resolved.settings, base.settings);
        assert_eq!(resolved.selection, base.selection);
        resolved.validate("project_a").unwrap();
        assert_eq!(
            resolved.validate("project_b").unwrap_err().code,
            "mv_print_project_mismatch"
        );
    }
}

#[test]
fn malformed_copy_lineage_is_never_treated_as_an_authored_project_setup() {
    let valid = json!({"scope":"project","project_id":"source_project","id":"source_mv","revision":"b".repeat(64)});
    let mut invalid = vec![json!({}), json!("source_project")];
    for (field, replacement) in [
        ("scope", json!("personal")),
        ("scope", json!("global")),
        ("project_id", Value::Null),
        ("project_id", json!("")),
        ("project_id", json!("source/project")),
        ("project_id", json!(" source_project")),
        ("project_id", json!("x".repeat(129))),
        ("id", json!("invalid/id")),
        ("revision", json!("b".repeat(63))),
        ("revision", json!("B".repeat(64))),
    ] {
        let mut origin = valid.clone();
        origin[field] = replacement;
        invalid.push(origin);
    }
    let mut missing_project = valid.clone();
    missing_project
        .as_object_mut()
        .unwrap()
        .remove("project_id");
    invalid.push(missing_project);
    for origin in invalid {
        let mut value = sheets();
        value["printing_setups"][0]["inherited_from"] = origin.clone();
        let refusal = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap_err();
        assert_eq!(refusal.code, "mv_print_setup_not_adopted", "{origin}");
        assert_eq!(refusal.field, "inherited_from", "{origin}");
    }
}
#[test]
fn issue_policy_and_library_documents_never_guess_a_date_logo_font_or_presentation() {
    let mut selected: Selection =
        serde_json::from_value(sheets()["project_settings"][0]["value"].clone()).unwrap();
    for date in ["", "2026-02-29", "2026-13-01", "today"] {
        selected.issue_date = date.into();
        assert_eq!(selected.validate().unwrap_err().field, "issue_date");
    }
    selected.issue_date = "2024-02-29".into();
    selected.validate().unwrap();
    let mut layout = sheets()["printing_setups"][0]["layout"].clone();
    layout["mv"]["settings"]
        .as_object_mut()
        .unwrap()
        .remove("title_country");
    assert!(serde_json::from_value::<printing::Layout>(layout)
        .unwrap_err()
        .to_string()
        .contains("settings.title_country"));
    let mut layout: printing::Layout =
        serde_json::from_value(sheets()["printing_setups"][0]["layout"].clone()).unwrap();
    layout.mv.as_mut().unwrap().font_family = "Unknown".into();
    assert!(printing::validate(&layout)
        .unwrap_err()
        .contains("mv_print_template_invalid"));
}
#[test]
fn selection_patch_preserves_lv_settings_and_mv_cannot_masquerade_as_lv_geometry() {
    let mut value = sheets();
    let selected = mv::selection(&value).unwrap();
    let original = json!({"parameter":"design_export_format","value":["xlsx"]});
    let rows = mv::patch_selection(std::slice::from_ref(&original), &selected).unwrap();
    assert_eq!(rows[0], original);
    assert_eq!(rows.len(), 2);
    let inspected =
        ds_command_kernel::report_formats::inspect_project_settings(&value, None).unwrap();
    assert_eq!(mv::inspect(&value, Some("project_a"))["configured"], true);
    assert_eq!(inspected["mv"]["ready"], false);
    value["project_settings"]
        .as_array_mut()
        .unwrap()
        .push(json!({"parameter":"design_export_format","value":["pdf__test_project_mv"]}));
    assert!(ds_command_kernel::report_formats::plan(&value, false)
        .unwrap_err()
        .contains("mv_print_requires_model_geometry"));
}

#[test]
fn two_models_share_one_setup_and_policy_with_exact_distinct_approved_cover_pins() {
    let mut value = sheets();
    let cover = |identity: &str, asset: &str, digest: char| json!({"model_identity":identity,"asset_id":asset,"sha256":format!("sha256:{}",digest.to_string().repeat(64))});
    value["printing_setups"][0]["layout"]["mv"]["pages"] = json!([
        {"kind":"model_preserved_pdf","bindings":[cover("Model1","cover_m1",'1'),cover("Model2","cover_m2",'2')]},
        {"kind":"preserved_pdf","asset_id":"naming_convention","sha256":format!("sha256:{}","3".repeat(64))},
        {"kind":"plan_profile"}
    ]);
    let resolve = |identity: &str| {
        mv::resolve(
            &value,
            "project_a",
            BTreeMap::from([(ModelField::Identity, identity.into())]),
        )
    };
    let m1 = resolve("Model1").unwrap();
    let m2 = resolve("Model2").unwrap();
    assert_eq!(m1.selection, m2.selection);
    assert_eq!(m1.layout, m2.layout);
    assert_eq!(m1.settings, m2.settings);
    assert_eq!(m1.inherited_from, m2.inherited_from);
    assert_eq!(m1.preserved_pages[0].asset_id, "cover_m1");
    assert_eq!(m2.preserved_pages[0].asset_id, "cover_m2");
    assert_ne!(m1.preserved_pages[0].sha256, m2.preserved_pages[0].sha256);
    assert_eq!(m1.preserved_pages[1], m2.preserved_pages[1]);
    assert_eq!(
        resolve("Model3").unwrap_err().code,
        "mv_print_model_binding_missing"
    );
    assert_eq!(
        mv::resolve(&value, "project_a", BTreeMap::new())
            .unwrap_err()
            .code,
        "mv_print_model_binding_missing"
    );
    let mut tampered = m1.clone();
    tampered.preserved_pages[0] = m2.preserved_pages[0].clone();
    assert_eq!(
        tampered.validate("project_a").unwrap_err().code,
        "mv_print_receipt_changed"
    );
    value["printing_setups"][0]["layout"]["mv"]["pages"][0]["bindings"]
        .as_array_mut()
        .unwrap()
        .push(cover("Model1", "another", '4'));
    assert!(mv::resolve(&value, "project_a", BTreeMap::new())
        .unwrap_err()
        .remedy
        .contains("ambiguous"));
}

#[test]
fn canonical_global_body_admits_generic_and_exact_preserved_model_pages() {
    let layout: Value =
        serde_json::from_str(include_str!("fixtures/mv-global-canonical.json")).unwrap();
    let selected = json!({"schema":mv::SELECTION_SCHEMA,"layout_id":layout["id"],"revision":"a".repeat(64),"external_version":"1","issue_date":"2026-10-02"});
    let sheets = json!({"project_settings":[],"printing_defaults":{"schema":"ds.printing-defaults/v1","mv_selection":selected},"report_context":{"project_title":"Gisagara MV Design"},"global_printing_setups":[{"id":layout["id"],"revision":"a".repeat(64),"layout":layout}]});
    let generic = mv::resolve(&sheets, "gisagara", BTreeMap::new()).unwrap();
    assert!(generic.preserved_pages.is_empty());
    assert_eq!(generic.settings.vertical_scale, Some(800));
    assert_eq!(generic.settings.project_title, "Gisagara MV Design");
    generic.validate("gisagara").unwrap();
    let model = mv::resolve(
        &sheets,
        "nyamagabe",
        BTreeMap::from([
            (ModelField::Identity, "Nyamagabe-MV-Model1".into()),
            (
                ModelField::Title,
                "MV Plan and Profile - Model 1 - Display 5 C initial, no wind".into(),
            ),
        ]),
    )
    .unwrap();
    assert_eq!(model.preserved_pages.len(), 3);
    assert_eq!(
        model.settings.sheet_title,
        "MV Plan and Profile - Model 1 - Display 5 C initial, no wind"
    );
    model.validate("nyamagabe").unwrap();
}
#[test]
fn the_global_default_takes_the_printing_projects_title_and_country_and_names_no_employer() {
    // Owner, 2026-10-05: the global MV default printed one project's parties
    // for every project. It now names none; the printing project supplies
    // its title and country, and only its own setup may name an employer.
    let mut value = sheets();
    for setting in ["project_title", "title_country", "title_employer"] {
        value["printing_setups"][0]["layout"]["mv"]["settings"][setting] = json!("");
    }
    let mut own = value.clone();
    own["report_context"] =
        json!({"schema":"ds.report-context/v1","country":"Chad","project_title":"Projet test"});
    assert_eq!(
        mv::resolve(&own, "project_a", BTreeMap::new())
            .unwrap_err()
            .code,
        "mv_print_text_missing",
        "a project's own setup still approves every title field"
    );
    let selection = mv::resolve(&sheets(), "project_a", BTreeMap::new())
        .unwrap()
        .selection;
    value["printing_defaults"] =
        json!({"schema":"ds.printing-defaults/v1","mv_selection":selection});
    value["global_printing_setups"] = value["printing_setups"].clone();
    value["project_settings"] = json!([]);
    value["printing_setups"] = json!([]);
    value["report_context"] =
        json!({"schema":"ds.report-context/v1","country":"Chad","project_title":"Projet test"});
    let global = mv::resolve(&value, "project_a", BTreeMap::new()).unwrap();
    assert_eq!(global.settings.project_title, "Projet test");
    assert_eq!(global.settings.title_country, "Chad");
    assert_eq!(global.settings.title_employer, "");
    // Every consumer re-resolves the receipt, through the same global default.
    global.validate("project_a").unwrap();
    value["report_context"] =
        json!({"schema":"ds.report-context/v1","project_title":"Projet test"});
    assert_eq!(
        mv::resolve(&value, "project_a", BTreeMap::new())
            .unwrap_err()
            .code,
        "mv_print_text_missing",
        "a project without a country is refused by name, never given another's"
    );
}
