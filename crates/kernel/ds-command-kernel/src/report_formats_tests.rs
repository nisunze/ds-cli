use super::*;

#[test]
fn download_names_share_the_cloud_projection_corpus() {
    let cases: Vec<[String; 2]> =
        serde_json::from_str(include_str!("../tests/fixtures/report-artifact-names.json")).unwrap();
    for [input, expected] in cases {
        assert_eq!(report_download_filename(&input), expected, "{input}");
    }
}

#[test]
fn download_names_keep_report_meaning_and_remove_only_generated_run_identity() {
    let run = "021d4e7aaba7bec355f7ceec5aececbc";
    for (suffix, expected_suffix) in [
        (".zip", ".zip"),
        (".shp.zip", ".shp.zip"),
        ("_geojsonl.zip", "_geojsonl.zip"),
        (
            "-overall-combined-report.xlsx",
            "-overall-combined-report.xlsx",
        ),
        (
            "-huye-combined-report.a3.pdf",
            "-huye-combined-report.a3.pdf",
        ),
    ] {
        let old = format!("280926-0916-report-gisagara-cjic-{run}{suffix}");
        let clean = format!("280926-0916-report-gisagara-cjic{expected_suffix}");
        assert_eq!(report_download_filename(&old), clean);
        assert_eq!(report_download_filename(&clean), clean);
        assert_eq!(
            artifact_filename(&format!("gs://bucket/reports/{old}")),
            clean
        );
    }
    assert_eq!(
        report_download_filename(&format!("280926_0916_report_gisagara_cjic_{run}.shp.zip")),
        "280926_0916_report_gisagara_cjic.shp.zip"
    );
    // An authored project/file identifier is not a generated report run.
    for name in [
        format!("survey-{run}.zip"),
        format!("project-{run}-report.xlsx"),
        "280926-0916-report-gisagara-cjic-revision-12.zip".into(),
        "combined_transformer.xlsx".into(),
    ] {
        assert_eq!(report_download_filename(&name), name);
    }
    let stem = format!("280926-0916-report-gisagara-cjic-{run}");
    let (label, folder, archive) = combined_entry_naming("", "", &stem);
    assert_eq!(label, "280926-0916-report-gisagara-cjic");
    assert_eq!(folder, label);
    assert_eq!(archive, "280926-0916-report-gisagara-cjic.zip");
}

#[test]
fn download_names_cannot_escape_a_folder_or_a_content_disposition() {
    assert_eq!(
        report_download_filename("../nested/report.xlsx"),
        "report.xlsx"
    );
    assert_eq!(
        report_download_filename(r"C:\reports\report.xlsx"),
        "report.xlsx"
    );
    assert_eq!(
        report_download_filename("report\r\n\"<>|:*?.xlsx"),
        "report_________.xlsx"
    );
    assert_eq!(report_download_filename("  report.xlsx.  "), "report.xlsx");
    assert_eq!(report_download_filename(".."), "report");
    assert_eq!(report_download_filename(""), "report");
}

/// The name is the artifact's own, whatever evidence it came from: a signed
/// storage locator, a plain path, or the output id of a local outbox row.
#[test]
fn one_filename_rule_covers_every_output_and_pdf_case() {
    assert_eq!(
        report_filename("run_ruhuha", "pdf_a0").unwrap(),
        "Run_Ruhuha.a0.pdf"
    );
    assert_eq!(
        report_filename("RUN_RUHUHA", "pdf_a0").unwrap(),
        "Run_Ruhuha.a0.pdf"
    );
    assert_eq!(
        report_filename("run_ruhuha", "pdf__layout").unwrap(),
        "Run_Ruhuha.layout.pdf"
    );
    for token in INDIVIDUAL.iter().chain(COMBINED) {
        let definition = output(token).unwrap();
        assert!(report_filename("run_ruhuha", token)
            .unwrap()
            .ends_with(&definition.suffix));
    }
    assert!(report_filename("run_ruhuha", "unknown").is_err());
}

#[test]
fn one_artifact_has_one_name_from_either_kind_of_evidence() {
    // A storage locator carries the object path percent-encoded after /o/,
    // and the signature after ? is never part of the name.
    assert_eq!(
        artifact_filename(
            "https://host/v0/b/bucket/o/eds%2Fp1%2Fcombined%20report.xlsx?alt=media&token=t"
        ),
        "combined report.xlsx"
    );
    // A plain URL or path is the same question with less wrapping.
    assert_eq!(
        artifact_filename("https://host/reports/combined_transformer.shp.zip?X-Goog-Signature=a"),
        "combined_transformer.shp.zip"
    );
    assert_eq!(artifact_filename("/outbox/run/report.pdf"), "report.pdf");
    // Malformed encoding keeps the raw text rather than losing the name.
    assert_eq!(
        artifact_filename("https://host/o/a%2Fb%ZZ.pdf"),
        "a%2Fb%ZZ.pdf"
    );
    // A locator that ends in a separator has no basename at all.
    assert_eq!(artifact_filename("https://host/reports/"), "file");
    assert_eq!(artifact_filename(""), "file");
    // The local half answers the same question from the output it is.
    assert_eq!(
        combined_output_filename("compounded", "shp", "zip"),
        "compounded_reports.zip"
    );
    assert_eq!(
        combined_output_filename("combined", "shp", "zip"),
        "combined_transformer.shp.zip"
    );
    assert_eq!(
        combined_output_filename("combined", "xlsx", "xlsx"),
        "combined_transformer.xlsx"
    );
    assert_eq!(
        combined_output_filename("combined", "geojsonseq", "zip"),
        "combined_transformer_geojsonl.zip"
    );
    assert_eq!(
        combined_output_filename("combined", "unheard_of", "bin"),
        "combined_transformer.bin"
    );
}
use serde_json::json;
/// The hazard the campaign opened with. `DesignOutputMatrix` has written an
/// `execution` key into the stored selection since 2026-09-07 — including
/// the empty map of its legacy upgrade — and this type refused it, so every
/// reader of a project edited since then (settings inspection, the native
/// reporter's export read, the `output_selection` op) failed.
#[test]
fn a_stored_selection_carrying_execution_placement_is_readable() {
    let stored = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":[],"tabular":[],"execution":{}});
    serde_json::from_value::<DesignOutputSelection>(stored.clone())
        .expect("the empty execution map the legacy upgrade writes must deserialize");
    let sheets = json!({"project_settings":[{"parameter":"design_export_format","value":stored}]});
    assert!(
        plan(&sheets, false).is_ok(),
        "plan must read the stored selection"
    );
}

/// The cloud plans data only: a named print whose setup is not in the
/// sealed inputs refuses `plan`, but never a cloud data export.
#[test]
fn the_cloud_plan_keeps_data_outputs_and_leaves_every_printout_out() {
    let sheets = json!({"project_settings":[{"parameter":"design_export_format",
            "value":"excel;A3_PDF;pdf__a0-landscape-missing;voltage_drop_pdf"}]});
    assert!(
        plan(&sheets, false).is_err(),
        "the missing setup still refuses a printing plan"
    );
    let planned: Vec<_> = plan_data_outputs(&sheets)
        .expect("a data plan never consults printing inputs")
        .into_iter()
        .map(|output| output.output_id)
        .collect();
    assert_eq!(planned, ["xlsx", "voltage_drop"]);
    assert!(is_printout("pdf_a3") && is_printout("pdf__a0-landscape-missing"));
    assert!(is_printout("voltage_drop_pdf"));
    assert!(!is_printout("xlsx"));
}

/// One stored token, one answer. `normalize` (and therefore `plan`, the
/// native reporter and ds-brain) has always aliased `A3_PDF` to `pdf_a3`;
/// the selection reader skipped the table and answered `a3_pdf`, which no
/// catalogue offers and which `output_class` calls geospatial while the
/// grant calls it `print` — so a project restricting prints to the desktop
/// had this output shown as running anywhere.
#[test]
fn a_legacy_alias_reads_the_same_in_the_selection_as_in_the_plan() {
    let selection = stored_output_selection(&json!("A3_PDF;excel")).expect("legacy row reads");
    assert_eq!(selection.tokens().unwrap(), ["pdf_a3", "xlsx"]);
    assert_eq!(output_class("pdf_a3"), "print");
    assert_eq!(output_class("xlsx"), "tabular");
    let sheets =
        json!({"project_settings":[{"parameter":"design_export_format","value":"A3_PDF;excel"}]});
    let planned: Vec<_> = plan(&sheets, false)
        .expect("plan reads the same row")
        .into_iter()
        .map(|output| output.output_id)
        .collect();
    assert_eq!(planned, ["xlsx", "pdf_a3"]);
    // The class the grant filters on is the class this selection answers
    // for, so a project that keeps prints on the desktop keeps this one
    // there too.
    let restricted = normalize_output_selection(
        None,
        Some(vec!["A3_PDF".into(), "excel".into()]),
        Some(BTreeMap::from([(
            "print".to_string(),
            vec![Placement::Desktop],
        )])),
    )
    .expect("a legacy list upgrades with the placement its host holds");
    assert_eq!(restricted.placements("pdf_a3"), [Placement::Desktop]);
    assert_eq!(restricted.placements("xlsx"), BOTH_PLACEMENTS);
    // The same token twice is one selection, as it is for the grant.
    assert_eq!(
        stored_output_selection(&json!("excel,xlsx,SHP,shp"))
            .unwrap()
            .tokens()
            .unwrap(),
        ["shp", "xlsx"]
    );
}

/// ds-brain trims and case-folds a stored placement before matching it. A
/// selection it admits must read here, and must be written back canonical.
#[test]
fn a_stored_placement_is_read_the_way_the_grant_reads_it() {
    let stored = json!({"schema":OUTPUT_SELECTION_SCHEMA,"geospatial":["gpkg"],
            "execution":{"gpkg":[" Desktop ","WEB"]}});
    let selection = stored_output_selection(&stored).expect("the grant admits this project");
    assert_eq!(selection.placements("gpkg"), BOTH_PLACEMENTS);
    assert_eq!(
        serde_json::to_value(&selection).unwrap()["execution"]["gpkg"],
        json!(["desktop", "web"])
    );
    assert!(stored_output_selection(
            &json!({"schema":OUTPUT_SELECTION_SCHEMA,"geospatial":["gpkg"],"execution":{"gpkg":["cloud"]}})
        )
        .is_err());
    // Case-folding must not smuggle a repeat past the duplicate rule.
    assert!(stored_output_selection(
            &json!({"schema":OUTPUT_SELECTION_SCHEMA,"geospatial":["gpkg"],"execution":{"gpkg":["web","WEB"]}})
        )
        .is_err());
}

/// The Settings page saves the whole sheet through `design_config::save`.
/// That writer must not be a second author of the export row: the bytes the
/// output matrix chose are the bytes that are stored.
#[test]
fn the_settings_sheet_save_leaves_the_export_row_exactly_as_chosen() {
    let value = json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "prints":[{"layout_id":"detail","enabled":true,"formats":["pdf"]}],
            "geospatial":["gpkg"],"tabular":["xlsx"],"execution":{"print":["desktop"]}});
    let rows = json!([
        {"parameter":"Design-Export-Format","value":value},
        {"parameter":"max_cable_size","value":"3 x 70"},
    ]);
    let document = json!({"sheets":{"project_settings":rows}});
    let saved = crate::design_config::save_sheet(&document, "project_settings", &rows)
        .expect("the settings sheet saves");
    let saved = saved.as_array().expect("rows");
    // The parameter name is reduced the way every reader of this row
    // reduces it, and the selection itself is untouched.
    assert_eq!(saved[0]["parameter"], "design_export_format");
    assert_eq!(saved[0]["value"], value);
    assert_eq!(
        setting_parameter_key(&saved[0]).as_deref(),
        Some("design_export_format")
    );
    let sheets = json!({"project_settings":saved});
    assert_eq!(
        project_output_setting(&sheets).unwrap().unwrap()["value"],
        value
    );
}

#[test]
fn local_matrix_does_not_change_defaults_and_requires_held_layouts() {
    let mut layout = crate::printing::default_layout();
    layout.id = "detail".into();
    let sheets = json!({"project_settings":[{"parameter":"report_formats","value":["xlsx","pdf__detail"]}],
            "printing_setups":[{"id":"detail","revision":"a".repeat(64),"layout":layout}]});
    let selection: DesignOutputSelection =
        serde_json::from_value(json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "prints":[{"layout_id":"detail","formats":["pdf"]}]}))
        .unwrap();
    let original = sheets.clone();
    assert_eq!(
        selected_plan(&sheets, false, Some(&selection))
            .unwrap()
            .iter()
            .map(|o| o.output_id.as_str())
            .collect::<Vec<_>>(),
        vec!["pdf__detail"]
    );
    assert_eq!(sheets, original);
    assert_eq!(selected_plan(&sheets, false, None).unwrap().len(), 2);
    assert!(selected_plan(&json!({}), false, Some(&selection)).is_err());
    assert!(selected_plan(&sheets, true, Some(&selection)).is_err());
    let empty = DesignOutputSelection {
        schema: OUTPUT_SELECTION_SCHEMA.into(),
        prints: vec![],
        geospatial: vec![],
        tabular: vec![],
        execution: BTreeMap::new(),
        voltage_drop_report: VoltageDropReportFormat::default(),
    };
    assert!(selected_plan(&sheets, false, Some(&empty)).is_err());
}

#[test]
fn settings_inspection_uses_saved_selection_and_actual_paper_not_names() {
    use crate::printing::{named_layout, Orientation, Paper};
    let layout = named_layout(
        Some("a0-looking-name".into()),
        None,
        Paper::A3,
        Orientation::Landscape,
    )
    .unwrap();
    let selection = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[
            {"layout_id":"a0-looking-name","enabled":true,"formats":["pdf"]},
            {"layout_id":"disabled","enabled":false,"formats":["pdf"]}],"geospatial":["gpkg"],"tabular":["xlsx"]});
    let sheets = json!({"project_settings":[{"parameter":"REPORT-FORMATS","value":selection}],
            "printing_setups":[{"id":layout.id,"revision":"a".repeat(64),"layout":layout}]});
    let result = inspect_project_settings(&sheets, None).unwrap();
    assert_eq!(result["setting"]["value"], selection);
    assert_eq!(
        result["outputs"],
        serde_json::to_value(plan(&sheets, false).unwrap()).unwrap()
    );
    assert_eq!(result["papers"].as_array().unwrap().len(), 1);
    assert_eq!(result["papers"][0]["paper"]["size"], "A3");
    assert_eq!(result["source"], "project_settings");
    let defaults = inspect_project_settings(&json!({}), None).unwrap();
    assert_eq!(defaults["source"], "report_defaults");
    assert!(defaults["setting"].is_null());
    assert_eq!(defaults["papers"], json!([]));
    let mut broken = sheets;
    broken["printing_setups"] = json!([]);
    let unreadied = inspect_project_settings(&broken, None).unwrap();
    assert_eq!(unreadied["ready"], false);
    assert_eq!(unreadied["setting"], broken["project_settings"][0]);
}

#[test]
fn named_setups_keep_identity_across_renames_and_require_sealed_layouts() {
    use crate::printing::{named_layout, Orientation, Paper};
    let layout = named_layout(
        Some("detail".into()),
        Some("Detail portrait".into()),
        Paper::A4,
        Orientation::Portrait,
    )
    .unwrap();
    let mut sheets = json!({"project_settings":[{"parameter":"design_export_format","value":["xlsx","pdf__detail"]}],"printing_setups":[{"id":"detail","revision":"a".repeat(64),"layout":layout}]});
    let outputs = plan(&sheets, false).unwrap();
    assert_eq!(
        outputs
            .iter()
            .map(|o| o.output_id.as_str())
            .collect::<Vec<_>>(),
        ["xlsx", "pdf__detail"]
    );
    assert_eq!(outputs[1].suffix, ".detail.pdf");
    sheets["printing_setups"][0]["layout"]["name"] = json!("Renamed");
    assert_eq!(plan(&sheets, false).unwrap(), outputs);
    assert_eq!(
        named_layouts(&sheets).unwrap()["detail"].page_mm,
        [210., 297.]
    );
    sheets["printing_setups"] = json!([]);
    assert!(plan(&sheets, false)
        .unwrap_err()
        .contains("not in the sealed project printing inputs"));
    assert!(named_setup_id("pdf__../outside").is_none());
    let options = options(&[json!({"id":"detail","name":"Renamed"})]).unwrap();
    assert!(options
        .iter()
        .any(|o| o["value"] == "pdf__detail" && o["label"] == "PDF · Renamed"));
    assert!(!options.iter().any(|o| o["value"] == "pdf_a0"));
}

#[test]
fn pdf_selection_applies_only_to_individual_reports() {
    let sheets = json!({"project_settings":[
        {"parameter":"design_export_format", "value":{"pdf_a3":true,"pdf_a0":false,"xlsx":true}},
        {"parameter":"print_individual_page_sizes","value":["a0"]},
        {"parameter":"print_combined_page_sizes","value":["a0"]}
    ]});
    assert_eq!(
        plan(&sheets, false)
            .unwrap()
            .iter()
            .map(|o| o.output_id.as_str())
            .collect::<Vec<_>>(),
        ["xlsx", "pdf_a3"]
    );
    assert_eq!(
        plan(&sheets, true)
            .unwrap()
            .iter()
            .map(|o| o.output_id.as_str())
            .collect::<Vec<_>>(),
        ["shp", "xlsx", "geojsonseq"]
    );
    assert_eq!(output("pdf_a3").unwrap().content_type, "application/pdf");
}
#[test]
fn combined_data_does_not_require_a_selected_print_layout() {
    let sheets = json!({"project_settings":[{"parameter":"design_export_format","value":["xlsx","pdf__missing"]}]});
    assert!(plan(&sheets, false).is_err());
    assert_eq!(
        plan(&sheets, true)
            .unwrap()
            .iter()
            .map(|o| o.output_id.as_str())
            .collect::<Vec<_>>(),
        ["shp", "xlsx", "geojsonseq"]
    );
}
#[test]
fn settings_shapes_and_output_order_are_stable() {
    for value in [
        json!("A3_PDF;excel"),
        json!(["pdf_a3", "xlsx"]),
        json!({"pdf_a3":"yes","xlsx":true}),
    ] {
        let sheets = json!({"parameters":{"project_settings":[{"parameter":"design-export-formats","value":value}]}});
        assert_eq!(
            plan(&sheets, false)
                .unwrap()
                .iter()
                .map(|o| o.output_id.as_str())
                .collect::<Vec<_>>(),
            ["xlsx", "pdf_a3"]
        );
    }
    assert!(plan(&json!({"project_settings":{}}), false).is_err());
}

#[test]
fn versioned_output_matrix_selects_rasters_from_one_named_layout() {
    use crate::printing::{named_layout, Orientation, Paper};
    let layout = named_layout(
        Some("aderm-a3".into()),
        Some("ADERM A3".into()),
        Paper::A3,
        Orientation::Landscape,
    )
    .unwrap();
    let sheets = json!({
        "project_settings":[{"parameter":"design_export_format","value":{
            "schema":"ds.design-output-selection/v1",
            "prints":[{"layout_id":"aderm-a3","enabled":true,"formats":["pdf","jpeg"]}],
            "geospatial":["gpkg"],
            "tabular":["xlsx"]
        }}],
        "printing_setups":[{"id":"aderm-a3","revision":"a".repeat(64),"layout":layout}]
    });
    let outputs = plan(&sheets, false).unwrap();
    assert_eq!(
        outputs
            .iter()
            .map(|item| item.output_id.as_str())
            .collect::<Vec<_>>(),
        ["xlsx", "gpkg", "jpeg__aderm-a3", "pdf__aderm-a3"]
    );
    assert_eq!(outputs[2].content_type, "image/jpeg");
    assert_eq!(outputs[2].suffix, ".aderm-a3.jpg");
}

/// ds-brain's precedence for a data output, reproduced: exact output id,
/// then the output class, then `*`, then both lanes. The class token is
/// `print`, singular, as `network_reporter_individual_policy.go:206,:225`
/// matches it — and a print never consults the matrix (next test).
#[test]
fn execution_placement_follows_the_brains_precedence_exactly() {
    let selection = |execution: Value| -> DesignOutputSelection {
        serde_json::from_value(json!({"schema":OUTPUT_SELECTION_SCHEMA,
                "prints":[{"layout_id":"detail","formats":["pdf"]}],
                "geospatial":["gpkg"],"tabular":["xlsx"],"execution":execution}))
        .expect("selection deserializes")
    };
    let absent = selection(json!({}));
    for output in ["gpkg", "shp", "xlsx", "network_information"] {
        assert_eq!(absent.placements(output), BOTH_PLACEMENTS, "{output}");
    }
    let exact = selection(json!({"gpkg":["desktop"],"geospatial":["web"],"*":[]}));
    assert_eq!(exact.placements("gpkg"), [Placement::Desktop]);
    assert_eq!(exact.placements("shp"), [Placement::Web]);
    assert_eq!(exact.placements("xlsx"), [] as [Placement; 0]);
    let classes = selection(json!({"geospatial":["desktop"],"tabular":["web"],"*":["desktop"]}));
    assert_eq!(classes.placements("gpkg"), [Placement::Desktop]);
    assert_eq!(classes.placements("xlsx"), [Placement::Web]);
    assert_eq!(classes.placements("pdf__detail"), [Placement::Desktop]);
    assert_eq!(output_class("xlsx"), "tabular");
    assert_eq!(output_class("jpeg__detail"), "print");
    assert_eq!(output_class("pdf_a0"), "print");
    assert_eq!(output_class("shp"), "geospatial");
    assert_eq!(
        selection(json!({"gpkg":["desktop","web"]}))
            .placement_map()
            .unwrap()["gpkg"],
        vec![Placement::Desktop, Placement::Web]
    );
    // Bounded, not enumerated: a key naming nothing is inert, as it is in
    // Go, and a lane repeated or a third lane is refused outright.
    assert!(selection(json!({"not_an_output":["web"]})).tokens().is_ok());
    assert!(selection(json!({"gpkg":["web","web"]}))
        .tokens()
        .is_err_and(|error| error.contains("repeats a lane")));
    assert!(serde_json::from_value::<DesignOutputSelection>(
        json!({"schema":OUTPUT_SELECTION_SCHEMA,"execution":{"gpkg":["cloud"]}})
    )
    .is_err());
}

/// Printing is Desktop and the Linux Server only, both `Desktop`, and the
/// cloud never renders a printout. Every print spelling the kernel's
/// grammar reads resolves there under any stored matrix — absent, both
/// lanes, `web` alone or refused on every lane, by exact id, class or `*` —
/// and reading such a project never refuses it. Data placement is not
/// touched by the rule.
#[test]
fn every_print_output_resolves_to_the_desktop_whatever_the_matrix_says() {
    let prints = [
        "pdf_a0",
        "pdf_a3",
        "voltage_drop_pdf",
        "pdf__detail",
        "png__detail",
        "jpeg__detail",
        "jpg__detail",
        "svg__detail",
    ];
    for output in prints {
        assert!(is_printout(output), "{output} is a print");
        assert_eq!(output_class(output), "print", "{output}");
    }
    for execution in [
        json!({}),
        json!({"print":["desktop","web"]}),
        json!({"print":["web"]}),
        json!({"print":[]}),
        json!({"*":["web"]}),
        json!({"*":[]}),
        json!({"pdf_a0":["web"],"pdf_a3":[],"pdf__detail":["web"],
                "png__detail":["web"],"jpeg__detail":["web"],"svg__detail":[]}),
    ] {
        // The fixed PDFs sit in `geospatial`, where the legacy upgrade
        // puts them; `output_class` still reads them as prints.
        let stored = stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
                "prints":[{"layout_id":"detail","formats":["pdf","png","jpeg"]}],
                "geospatial":["pdf_a0","pdf_a3","voltage_drop_pdf","gpkg"],"tabular":["xlsx"],
                "execution":execution}))
        .expect("reading a stored selection never refuses its placement");
        for output in prints {
            assert_eq!(
                stored.placements(output),
                PRINT_PLACEMENTS,
                "{output} under {execution}"
            );
        }
        let map = stored.placement_map().unwrap();
        for token in [
            "pdf_a0",
            "pdf_a3",
            "voltage_drop_pdf",
            "pdf__detail",
            "png__detail",
            "jpeg__detail",
        ] {
            assert_eq!(map[token], PRINT_PLACEMENTS, "{token} under {execution}");
        }
    }
    let legacy = stored_output_selection(&json!("A3_PDF;pdf;jpg__detail;xlsx")).unwrap();
    for (token, placements) in legacy.placement_map().unwrap() {
        let expected = if token == "xlsx" {
            BOTH_PLACEMENTS
        } else {
            PRINT_PLACEMENTS
        };
        assert_eq!(placements, expected, "{token}");
    }
    let data = stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "geospatial":["gpkg"],"tabular":["xlsx"],"execution":{"print":["web"],"*":["web"]}}))
    .unwrap();
    assert_eq!(data.placements("gpkg"), [Placement::Web]);
    assert_eq!(data.placements("xlsx"), [Placement::Web]);
}

/// The door the silent loss came through, closed where a matrix is
/// authored and saved. ds-brain's desktop grant reads the stored matrix
/// for a print, so an entry that reaches one without `desktop` is refused
/// by name — never rewritten — and a refused save leaves the row as it was.
/// Keeping data on `web` alone stays open, and so does naming both lanes.
#[test]
fn a_saved_matrix_may_not_place_a_print_off_the_desktop() {
    let versioned = |execution: &Value| -> DesignOutputSelection {
        serde_json::from_value(json!({"schema":OUTPUT_SELECTION_SCHEMA,
                "prints":[{"layout_id":"detail","formats":["pdf"]}],
                "geospatial":["gpkg"],"tabular":["xlsx"],"execution":execution}))
        .expect("selection deserializes")
    };
    for (execution, key) in [
        (json!({"print":["web"]}), "print"),
        (json!({"print":[]}), "print"),
        (json!({"pdf__detail":["desktop"],"print":["web"]}), "print"),
        (
            json!({"pdf__detail":["web"],"print":["desktop"]}),
            "pdf__detail",
        ),
        (json!({"pdf_a3":["web"]}), "pdf_a3"),
        (json!({"jpeg__detail":[]}), "jpeg__detail"),
        (json!({"*":["web"]}), "*"),
        (json!({"*":[]}), "*"),
    ] {
        let selection = versioned(&execution);
        assert!(
            selection.tokens().is_ok(),
            "reading {execution} still works"
        );
        let refused = normalize_output_selection(Some(selection.clone()), None, None)
            .expect_err("authoring refuses it");
        assert!(
            refused.contains(&format!("placement for {key} keeps prints off the desktop")),
            "{execution}: {refused}"
        );
        let upgraded = normalize_output_selection(
            None,
            Some(vec!["pdf__detail".into(), "xlsx".into()]),
            Some(serde_json::from_value(execution.clone()).unwrap()),
        );
        assert_eq!(upgraded, Err(refused.clone()), "the legacy upgrade too");
        let mut rows = vec![json!({"parameter":"design_export_format","value":"xlsx"})];
        let held = rows.clone();
        assert_eq!(apply_output_selection(&mut rows, &selection), Err(refused));
        assert_eq!(rows, held, "a refused save leaves the row as it was");
    }
    let data_only: DesignOutputSelection = serde_json::from_value(json!({
        "schema": OUTPUT_SELECTION_SCHEMA,
        "geospatial": ["gpkg"], "tabular": ["xlsx"],
        "execution": {"*": ["web"]}
    }))
    .unwrap();
    assert!(normalize_output_selection(Some(data_only.clone()), None, None).is_ok());
    assert_eq!(data_only.placements("gpkg"), [Placement::Web]);
    assert_eq!(data_only.placements("xlsx"), [Placement::Web]);
    for execution in [
        json!({}),
        json!({"print":["desktop"]}),
        json!({"print":["desktop","web"]}),
        json!({"*":["web"],"print":["desktop"]}),
        json!({"*":["desktop","web"]}),
        json!({"geospatial":["web"],"tabular":["web"],"gpkg":[]}),
        json!({"not_an_output":["web"]}),
    ] {
        let selection = versioned(&execution);
        assert_eq!(
            normalize_output_selection(Some(selection.clone()), None, None).as_ref(),
            Ok(&selection),
            "{execution}"
        );
        let mut rows = Vec::new();
        apply_output_selection(&mut rows, &selection).unwrap();
        assert_eq!(
            rows[0]["value"],
            serde_json::to_value(&selection).unwrap(),
            "{execution} is stored as chosen"
        );
    }
}

/// The empty `execution` map the Settings matrix writes on the first edit
/// of a legacy setting is not a restriction; it means both lanes, and it
/// survives a save/read round trip without being serialized back.
#[test]
fn an_empty_execution_map_means_both_lanes_and_is_not_written_back() {
    let selection: DesignOutputSelection = serde_json::from_value(
            json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":["gpkg"],"tabular":[],"execution":{}}),
        )
        .unwrap();
    assert_eq!(selection.placements("gpkg"), BOTH_PLACEMENTS);
    assert_eq!(
        serde_json::to_value(&selection).unwrap(),
        json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":["gpkg"],"tabular":[]})
    );
}

/// One reader for every shape a project has ever stored, including the two
/// raster tokens the deleted TypeScript mislaid into `geospatial`.
#[test]
fn stored_settings_of_every_shape_read_through_one_reader() {
    let read = |value: Value| stored_output_selection(&value).unwrap();
    let png = read(json!("png__detail,jpg__detail,xlsx"));
    assert_eq!(png.prints.len(), 1);
    assert_eq!(png.prints[0].layout_id, "detail");
    assert_eq!(
        png.prints[0].formats,
        vec![PrintArtifactFormat::Png, PrintArtifactFormat::Jpeg]
    );
    assert_eq!(png.geospatial, [] as [String; 0]);
    assert_eq!(png.tabular, ["xlsx"]);
    assert_eq!(read(json!("shp;kmz")).geospatial, ["shp", "kmz"]);
    assert_eq!(read(json!(["gpkg", "xlsx"])).tabular, ["xlsx"]);
    assert_eq!(read(json!({"shp":true,"kmz":false})).geospatial, ["shp"]);
    // Reading is not authoring: an absent or empty setting is an empty
    // selection, where the authoring normalizer demands 1..32 tokens.
    for empty in [json!(null), json!(""), json!([]), json!({})] {
        assert_eq!(read(empty).tokens().unwrap(), [] as [String; 0]);
    }
    let versioned = read(
        json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":[],"tabular":[],"execution":{"gpkg":["web"]}}),
    );
    assert_eq!(versioned.placements("gpkg"), [Placement::Web]);
    assert!(
        stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":"no"})).is_err()
    );
}

/// The five aliases, the hyphen/case normalisation, and the row a sheet
/// without any of them gets — the job `prepare.ts` did with two names and
/// a refusal.
#[test]
fn the_export_row_is_patched_through_every_alias_and_created_when_absent() {
    let selection: DesignOutputSelection = serde_json::from_value(
        json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":["gpkg"],"tabular":[]}),
    )
    .unwrap();
    for alias in [
        "design_export_format",
        "design_export_formats",
        "transformer_export_formats",
        "tr_export_formats",
        "report_formats",
        "REPORT-FORMATS",
        " design-export-format ",
    ] {
        let mut rows = vec![
            json!({"parameter":"unrelated","value":1}),
            json!({"parameter":alias,"value":"shp"}),
        ];
        assert_eq!(
            apply_output_selection(&mut rows, &selection).unwrap(),
            setting_parameter_key(&json!({"parameter":alias})).unwrap(),
            "{alias} keeps the row it already has"
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["value"]["geospatial"], json!(["gpkg"]));
        assert_eq!(rows[0], json!({"parameter":"unrelated","value":1}));
    }
    let mut absent = vec![json!({"parameter":"unrelated","value":1})];
    assert_eq!(
        apply_output_selection(&mut absent, &selection).unwrap(),
        SETTING_ALIASES[0]
    );
    assert_eq!(absent.len(), 2);
    assert_eq!(absent[1]["parameter"], "design_export_format");
    assert_eq!(absent[1]["value"]["schema"], OUTPUT_SELECTION_SCHEMA);
    // A patched sheet reads back as the selection it was given.
    let sheets = json!({"project_settings":absent});
    assert_eq!(
        plan(&sheets, false)
            .unwrap()
            .iter()
            .map(|output| output.output_id.as_str())
            .collect::<Vec<_>>(),
        ["gpkg"]
    );
}

/// A local override is one run's inventory. Placement is the project's
/// policy and ds-brain admits against it, so an override carrying one is
/// refused rather than quietly deciding it per invocation.
#[test]
fn a_local_override_may_not_carry_execution_placement() {
    let mut layout = crate::printing::default_layout();
    layout.id = "detail".into();
    let sheets = json!({"project_settings":[{"parameter":"report_formats","value":["xlsx","pdf__detail"]}],
            "printing_setups":[{"id":"detail","revision":"a".repeat(64),"layout":layout}]});
    let selection: DesignOutputSelection = serde_json::from_value(
        json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[{"layout_id":"detail","formats":["pdf"]}],
                "execution":{"pdf__detail":["desktop"]}}),
    )
    .unwrap();
    assert!(selected_plan(&sheets, false, Some(&selection))
        .is_err_and(|error| error.contains("never carry an execution placement")));
}

/// The refusal the GUI and `ds` both render, as keys rather than English.
#[test]
fn an_unready_settings_read_names_its_refusal_by_key_and_mode() {
    let sheets = json!({"project_settings":[{"parameter":"design_export_format",
            "value":{"schema":OUTPUT_SELECTION_SCHEMA,"prints":[{"layout_id":"missing","formats":["pdf"]}]}}]});
    for (mode, key) in [
        (None, "printing_inputs_incomplete_refreshed"),
        (
            Some(ReadinessMode::Refreshed),
            "printing_inputs_incomplete_refreshed",
        ),
        (
            Some(ReadinessMode::Offline),
            "printing_inputs_incomplete_offline",
        ),
    ] {
        let result = inspect_project_settings(&sheets, mode).unwrap();
        assert_eq!(result["ready"], false);
        assert_eq!(result["refusal"]["code"], "printing_inputs_incomplete");
        assert_eq!(result["refusal"]["message_key"], key);
        assert_eq!(result["refusal"]["mode"], mode.unwrap_or_default().token());
        assert!(!result["issues"].as_array().unwrap().is_empty());
    }
    assert!(
        inspect_project_settings(&json!({}), Some(ReadinessMode::Offline)).unwrap()["refusal"]
            .is_null()
    );
}

/// The option rows the matrix renders. A named print row carries the
/// layout id and the formats it may be produced in, so no host recovers
/// them by slicing `pdf__` off a value; a data row carries neither,
/// because it has no layout.
#[test]
fn every_option_row_declares_what_the_matrix_needs_without_string_surgery() {
    let rows = options(&[json!({"id":"detail","name":"Detail"})]).unwrap();
    assert!(!rows.is_empty());
    for row in &rows {
        let value = row["value"].as_str().expect("option value");
        if let Some((_, id)) = named_print_output(value) {
            assert_eq!(row["layout_id"], id);
            assert_eq!(row["formats"], json!(["pdf", "png", "jpeg"]));
        } else {
            assert!(row["layout_id"].is_null(), "{value} declares a layout");
            assert!(row["formats"].is_null(), "{value} declares formats");
            assert!(output(value).is_ok(), "{value} is not a report output");
        }
    }
}

#[test]
fn human_output_metadata_distinguishes_reports_from_saved_json_without_changing_policy() {
    let rows = options(&[json!({"id":"detail","name":"Detail"})]).unwrap();
    for row in &rows {
        let value = row["value"].as_str().unwrap();
        let definition = output(value).unwrap();
        assert_eq!(
            row["file_format"],
            definition.suffix.rsplit('.').next().unwrap()
        );
        assert_eq!(row["class"], output_class(value));
        assert!(row["description"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty()));
        assert!(matches!(
            row["display_group"].as_str(),
            Some("reports" | "data_exports")
        ));
    }
    let row = |value: &str| rows.iter().find(|row| row["value"] == value).unwrap();
    assert_eq!(row("voltage_drop")["label"], "Voltage-drop results (JSON)");
    assert_eq!(
        row("network_information")["label"],
        "Network information (JSON)"
    );
    for value in ["voltage_drop", "network_information"] {
        assert_eq!(row(value)["file_format"], "json");
        assert_eq!(row(value)["display_group"], "data_exports");
        assert!(row(value)["report_format"].is_null());
        assert_eq!(output(value).unwrap().content_type, "application/json");
    }
    assert_eq!(
        row("voltage_drop_pdf")["label"],
        "Voltage-drop report (A4 PDF)"
    );
    assert_eq!(row("voltage_drop_pdf")["file_format"], "pdf");
    assert_eq!(row("voltage_drop_pdf")["display_group"], "reports");
    assert_eq!(row("xlsx")["display_group"], "reports");
    assert_eq!(row("pdf__detail")["display_group"], "reports");

    // Presentation cannot add a PDF, change a stored choice or its placement.
    let stored = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],
            "geospatial":["gpkg"],"tabular":["voltage_drop","network_information"],
            "voltage_drop_report":"brief",
            "execution":{"voltage_drop":["web"],"network_information":["web"]}});
    let selection = stored_output_selection(&stored).unwrap();
    assert_eq!(serde_json::to_value(&selection).unwrap(), stored);
    assert!(!selection
        .tokens()
        .unwrap()
        .contains(&"voltage_drop_pdf".into()));
    // The stored entry stays as authored and places the saved analysis
    // like any other data output (owner correction 2026-10-19).
    assert_eq!(selection.placements("voltage_drop"), vec![Placement::Web]);
    assert_eq!(
        selection.placements("network_information"),
        vec![Placement::Web]
    );
}

#[test]
fn output_selection_normalization_has_one_alias_and_bound_policy() {
    let selection = normalize_output_selection(
        None,
        Some(vec![
            "PDF__ADERM-A3".into(),
            "jpg__aderm-a3".into(),
            "gpkg".into(),
            "xlsx".into(),
        ]),
        None,
    )
    .unwrap();
    assert_eq!(selection.schema, OUTPUT_SELECTION_SCHEMA);
    assert_eq!(selection.prints[0].layout_id, "aderm-a3");
    assert_eq!(
        selection.prints[0].formats,
        vec![PrintArtifactFormat::Pdf, PrintArtifactFormat::Jpeg]
    );
    assert_eq!(selection.geospatial, ["gpkg"]);
    assert_eq!(selection.tabular, ["xlsx"]);
    assert!(normalize_output_selection(None, Some(vec![]), None).is_err());
    assert!(normalize_output_selection(None, Some(vec!["gif__a3".into()]), None).is_err());
}

/// The saved JSON analysis is an ordinary tabular output: delivered by the
/// defaults and whenever a selection names it, never forced into a
/// project's selection of other outputs (owner correction 2026-10-19).
#[test]
fn voltage_drop_is_an_ordinary_selectable_tabular_json_output() {
    assert_eq!(normalize(&[]), ["shp", "kmz", "xlsx", "voltage_drop"]);
    assert_eq!(normalize(&["shp".into(), "xlsx".into()]), ["shp", "xlsx"]);
    assert_eq!(
        normalize(&["shp".into(), "voltage_drop".into(), "xlsx".into()]),
        ["shp", "xlsx", "voltage_drop"]
    );
    let definition = output("voltage_drop").unwrap();
    assert_eq!(definition.output_id, "voltage_drop");
    assert_eq!(definition.format, "voltage_drop");
    assert_eq!(definition.content_type, "application/json");
    assert_eq!(definition.suffix, ".voltage-drop.json");
    assert_eq!(output_class("voltage_drop"), "tabular");
    let selection = stored_output_selection(&json!("shp,voltage_drop,xlsx")).unwrap();
    assert_eq!(selection.geospatial, ["shp"]);
    assert_eq!(selection.tabular, ["voltage_drop", "xlsx"]);
    assert!(options(&[])
        .unwrap()
        .iter()
        .any(|option| option["value"] == "voltage_drop"));
    let planned = plan(&json!({}), false).unwrap();
    assert_eq!(
        planned.last().map(|o| o.output_id.as_str()),
        Some("voltage_drop")
    );
    assert!(!combined(&["voltage_drop".into()]).contains(&"voltage_drop"));
}

#[test]
fn selected_analysis_preserves_defaults_aliases_optional_outputs_and_order() {
    for (raw, expected) in [
        (vec![], vec!["shp", "kmz", "xlsx", "voltage_drop"]),
        (vec!["unknown"], vec!["shp", "kmz", "xlsx", "voltage_drop"]),
        (vec![" SHP ", "excel"], vec!["shp", "xlsx"]),
        (
            vec!["VOLtage_drop", "voltage_drop", "xlsx"],
            vec!["xlsx", "voltage_drop"],
        ),
        (vec!["pdf__detail"], vec!["pdf__detail"]),
        // The A4 PDF keeps its exact saved-analysis companion.
        (
            vec!["voltage_drop_pdf"],
            vec!["voltage_drop", "voltage_drop_pdf"],
        ),
    ] {
        let raw = raw.into_iter().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(normalize(&raw), expected, "{raw:?}");
    }
}

#[test]
fn unselected_analysis_is_not_exported_and_the_saved_selection_stays_as_authored() {
    let raw = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],
            "geospatial":["gpkg"],"tabular":[],"execution":{"voltage_drop":[],"*":[]},
            "voltage_drop_report":"brief"});
    let sheets = json!({"project_settings":[{"parameter":"report_formats","value":raw}]});
    let original = sheets.clone();
    let selection = stored_output_selection(&raw).unwrap();
    assert_eq!(selection.tokens().unwrap(), ["gpkg"]);
    assert_eq!(serde_json::to_value(&selection).unwrap(), raw);
    assert_eq!(selection.placements("gpkg"), [] as [Placement; 0]);
    assert_eq!(selection.placements("voltage_drop"), [] as [Placement; 0]);
    assert_eq!(
        project_voltage_drop_report_format(&sheets).unwrap(),
        VoltageDropReportFormat::Brief
    );
    assert!(!project_exports_voltage_drop(&sheets).unwrap());
    for outputs in [
        plan(&sheets, false).unwrap(),
        plan_data_outputs(&sheets).unwrap(),
    ] {
        assert_eq!(
            outputs
                .iter()
                .map(|output| output.output_id.as_str())
                .collect::<Vec<_>>(),
            ["gpkg"]
        );
    }
    assert_eq!(
        inspect_project_settings(&sheets, None).unwrap()["setting"]["value"],
        raw
    );
    assert_eq!(sheets, original);

    for raw in [
        json!(null),
        json!(""),
        json!([]),
        json!({}),
        json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],"geospatial":[],"tabular":[]}),
    ] {
        let sheets = json!({"project_settings":[{"parameter":"report_formats","value":raw}]});
        assert_eq!(
            plan(&sheets, false)
                .unwrap()
                .iter()
                .map(|output| output.output_id.as_str())
                .collect::<Vec<_>>(),
            DEFAULT_INDIVIDUAL
        );
    }
}

#[test]
fn analysis_placement_follows_the_brains_precedence_like_every_data_output() {
    for (execution, analysis) in [
        (json!({"voltage_drop":[]}), vec![]),
        (
            json!({"voltage_drop":["desktop"],"tabular":[]}),
            vec![Placement::Desktop],
        ),
        (
            json!({"voltage_drop":["web"],"tabular":[]}),
            vec![Placement::Web],
        ),
        (json!({"tabular":[]}), vec![]),
        (json!({"*":["web"]}), vec![Placement::Web]),
        (json!({"*":[]}), vec![]),
    ] {
        let raw = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],
                "geospatial":[],"tabular":["xlsx","voltage_drop"],"execution":execution});
        let selection = stored_output_selection(&raw).unwrap();
        assert_eq!(
            selection.placements("voltage_drop"),
            analysis,
            "{execution}"
        );
        let ordinary = execution.get("tabular").or_else(|| execution.get("*"));
        let expected: Vec<Placement> = ordinary.map_or_else(
            || BOTH_PLACEMENTS.to_vec(),
            |value| serde_json::from_value(value.clone()).unwrap(),
        );
        assert_eq!(selection.placements("xlsx"), expected, "{execution}");
        assert_eq!(serde_json::to_value(&selection).unwrap(), raw);
    }
    assert!(
        stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "execution":{"voltage_drop":["web","WEB"]}}))
        .is_err()
    );
    assert!(
        stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "execution":{"voltage_drop":["cloud"]}}))
        .is_err()
    );
}

#[test]
fn only_the_analysis_outputs_require_the_saved_voltage_drop_analysis() {
    for outputs in [
        vec![],
        vec!["shp", "kmz", "xlsx"],
        vec![
            "gpkg",
            "geojson",
            "network_information",
            "pdf_a0",
            "pdf_a3",
            "pdf__detail",
            "png__detail",
        ],
    ] {
        assert!(!requires_voltage_drop_analysis(&outputs), "{outputs:?}");
    }
    assert!(requires_voltage_drop_analysis(&["shp", "voltage_drop"]));
    assert!(requires_voltage_drop_analysis(&["voltage_drop_pdf"]));
    assert!(!requires_voltage_drop_analysis(&["xlsx".to_string()]));
}

#[test]
fn an_invocation_without_a_selection_exports_analysis_only_when_the_project_does() {
    let tokens = |sheets: &Value| {
        invocation_selection(sheets, None)
            .unwrap()
            .tokens()
            .unwrap()
    };
    // No stored selection: the defaults, which include the analysis.
    assert_eq!(tokens(&json!({})), ["shp", "kmz", "xlsx", "voltage_drop"]);
    // A legacy row naming only GIS outputs exports no analysis.
    let legacy = json!({"project_settings":[{"parameter":"design_export_format",
            "value":{"geojson":false,"gpkg":false,"kmz":true,"shp":true,"xlsx":true}}]});
    assert!(!project_exports_voltage_drop(&legacy).unwrap());
    assert_eq!(tokens(&legacy), ["shp", "kmz", "xlsx"]);
    // A versioned selection naming the analysis exports it.
    let selected = json!({"project_settings":[{"parameter":"design_export_format",
            "value":{"schema":OUTPUT_SELECTION_SCHEMA,"prints":[{"layout_id":"a0","formats":["pdf"]}],
            "geospatial":["kmz","shp"],"tabular":["xlsx","voltage_drop"]}}]});
    assert_eq!(tokens(&selected), ["shp", "kmz", "xlsx", "voltage_drop"]);
    // The A4 PDF brings its companion; the invocation prints nothing implicitly.
    let a4 = json!({"project_settings":[{"parameter":"design_export_format",
            "value":["voltage_drop_pdf","xlsx"]}]});
    let selection = invocation_selection(&a4, None).unwrap();
    assert!(selection.prints.is_empty());
    assert_eq!(
        selection.tokens().unwrap(),
        ["shp", "kmz", "xlsx", "voltage_drop"]
    );
    // An explicit selection is the run's exact inventory, whatever the
    // project stores, and it requires no analysis.
    let explicit: DesignOutputSelection = serde_json::from_value(json!({
            "schema":OUTPUT_SELECTION_SCHEMA,"geospatial":["shp","kmz"],"tabular":["xlsx"]}))
    .unwrap();
    let resolved = invocation_selection(&selected, Some(explicit.clone())).unwrap();
    assert_eq!(resolved, explicit);
    let planned = selected_plan(&json!({}), false, Some(&resolved)).unwrap();
    let ids: Vec<_> = planned.iter().map(|o| o.output_id.as_str()).collect();
    assert_eq!(ids, ["shp", "kmz", "xlsx"]);
    assert!(!requires_voltage_drop_analysis(&ids));
    // An unreadable stored selection is named, never guessed around.
    assert!(invocation_selection(
        &json!({"project_settings":[{"parameter":"design_export_format",
                "value":{"schema":OUTPUT_SELECTION_SCHEMA,"tabular":"xlsx"}}]}),
        None
    )
    .is_err());
}

#[test]
fn local_override_consumes_only_selected_analysis_and_refuses_invalid_inventory() {
    let selection = |tokens: Value| -> DesignOutputSelection {
        serde_json::from_value(json!({"schema":OUTPUT_SELECTION_SCHEMA,"tabular":tokens})).unwrap()
    };
    let sheets = json!({"project_settings":[{"parameter":"report_formats","value":"gpkg"}]});
    for (tokens, expected) in [
        (json!(["xlsx"]), vec!["xlsx"]),
        (
            json!(["xlsx", "voltage_drop"]),
            vec!["xlsx", "voltage_drop"],
        ),
    ] {
        let selection = selection(tokens);
        let original = serde_json::to_value(&selection).unwrap();
        assert_eq!(
            selected_plan(&sheets, false, Some(&selection))
                .unwrap()
                .iter()
                .map(|output| output.output_id.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(serde_json::to_value(&selection).unwrap(), original);
    }
    for tokens in [json!([]), json!(["unknown"]), json!(["xlsx", "xlsx"])] {
        assert!(selected_plan(&sheets, false, Some(&selection(tokens))).is_err());
    }
}

#[test]
fn explicit_inventory_and_selected_analysis_companion_stay_inside_the_artifact_bound() {
    let mut selection: DesignOutputSelection = serde_json::from_value(json!({
        "schema":OUTPUT_SELECTION_SCHEMA,
        "geospatial":["shp","kmz","gpkg","pdf_a0","pdf_a3"],"tabular":["xlsx","network_information"]
    }))
    .unwrap();
    let mut layouts = Vec::new();
    for index in 0..25 {
        let id = format!("detail-{index}");
        let mut layout = crate::printing::default_layout();
        layout.id = id.clone();
        layouts.push(json!({"id":id,"revision":"a".repeat(64),"layout":layout}));
        selection.prints.push(NamedPrintSelection {
            layout_id: id,
            enabled: true,
            formats: vec![PrintArtifactFormat::Pdf],
        });
    }
    let sheets = json!({"printing_setups":layouts});
    let outputs = selected_plan(&sheets, false, Some(&selection)).unwrap();
    assert_eq!(outputs.len(), 32);
    assert!(!outputs
        .iter()
        .any(|output| output.output_id == "voltage_drop"));
    selection.geospatial.push("voltage_drop_pdf".into());
    assert!(selected_plan(&sheets, false, Some(&selection)).is_err());
    selection.prints.pop();
    assert!(selected_plan(&sheets, false, Some(&selection))
        .is_err_and(|error| error.contains("including the saved-analysis companion")));
    selection.prints.pop();
    let outputs = selected_plan(&sheets, false, Some(&selection)).unwrap();
    assert_eq!(outputs.len(), 32);
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.output_id == "voltage_drop")
            .count(),
        1
    );
}

#[test]
fn voltage_drop_a4_pdf_is_an_explicit_local_print_output() {
    assert!(!DEFAULT_INDIVIDUAL.contains(&"voltage_drop_pdf"));
    let pdf = output("voltage_drop_pdf").unwrap();
    assert_eq!(pdf.output_id, "voltage_drop_pdf");
    assert_eq!(pdf.format, "voltage_drop_pdf");
    assert_eq!(pdf.content_type, "application/pdf");
    assert_eq!(pdf.suffix, " voltage drop.pdf");
    assert_eq!(output_class("voltage_drop_pdf"), "print");
    assert!(is_printout("voltage_drop_pdf"));
    assert_eq!(
        voltage_drop_pdf_filename("fill_in_bukorota"),
        "Fill In Bukorota voltage drop.pdf"
    );
    assert_eq!(
        voltage_drop_pdf_filename("Agahehe"),
        "Agahehe voltage drop.pdf"
    );
    assert_eq!(
        voltage_drop_pdf_filename("_001_main"),
        "001 Main voltage drop.pdf"
    );
    assert_eq!(
        voltage_drop_pdf_filename("  fill__in_bukorota  "),
        "Fill In Bukorota voltage drop.pdf"
    );

    let selected = json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "geospatial":["voltage_drop_pdf"],
            "execution":{"voltage_drop_pdf":["web"]}});
    let selection = stored_output_selection(&selected).unwrap();
    assert_eq!(
        selection.placements("voltage_drop_pdf"),
        [Placement::Desktop]
    );
    assert!(normalize_output_selection(
        Some(serde_json::from_value(selected.clone()).unwrap()),
        None,
        None,
    )
    .is_err_and(|error| error.contains("keeps prints off the desktop")));
    let sheets = json!({"project_settings":[{"parameter":"design_export_format",
            "value":selected}]});
    let planned = plan(&sheets, false).unwrap();
    assert_eq!(planned.len(), 2);
    assert_eq!(planned[0].output_id, "voltage_drop");
    assert_eq!(planned[1].output_id, pdf.output_id);
    assert_eq!(planned[1].content_type, pdf.content_type);
    assert_eq!(
        plan_data_outputs(&sheets).unwrap()[0].output_id,
        "voltage_drop"
    );
    let local = serde_json::from_value(json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "geospatial":["voltage_drop_pdf"]}))
    .unwrap();
    assert_eq!(
        selected_plan(&json!({}), false, Some(&local))
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn historical_fixed_output_buckets_are_repaired_without_losing_selection() {
    let raw = json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "geospatial":["shp","voltage_drop","xlsx"],
            "tabular":["xlsx","voltage_drop_pdf","network_information"],
            "execution":{"voltage_drop_pdf":["desktop"]}});
    let expected_geo = ["shp", "voltage_drop_pdf"];
    let expected_tabular = ["voltage_drop", "xlsx", "network_information"];
    let read = stored_output_selection(&raw).unwrap();
    assert_eq!(read.geospatial, expected_geo);
    assert_eq!(read.tabular, expected_tabular);
    assert_eq!(read.placements("voltage_drop_pdf"), [Placement::Desktop]);

    let authored: DesignOutputSelection = serde_json::from_value(raw).unwrap();
    let normalized = normalize_output_selection(Some(authored.clone()), None, None).unwrap();
    assert_eq!(normalized.geospatial, expected_geo);
    assert_eq!(normalized.tabular, expected_tabular);
    let mut rows = Vec::new();
    apply_output_selection(&mut rows, &authored).unwrap();
    assert_eq!(rows[0]["value"]["geospatial"], json!(expected_geo));
    assert_eq!(rows[0]["value"]["tabular"], json!(expected_tabular));
    assert_eq!(
        rows[0]["value"]["execution"],
        json!({"voltage_drop_pdf":["desktop"]})
    );
}

/// Owner ruling 2026-09-29: the extended voltage-drop report is the
/// default. A project that never chose — no export row, a legacy token
/// list, a versioned selection without the field — reads as extended,
/// and its stored bytes stay free of the field every older reader
/// refuses.
#[test]
fn a_selection_that_never_chose_a_voltage_drop_layout_is_extended() {
    let versioned = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],
            "geospatial":["shp"],"tabular":["voltage_drop"]});
    let selection = stored_output_selection(&versioned).unwrap();
    assert_eq!(
        selection.voltage_drop_report_format(),
        VoltageDropReportFormat::Extended
    );
    assert_eq!(serde_json::to_value(&selection).unwrap(), versioned);
    for stored in [json!(null), json!("shp,voltage_drop"), versioned.clone()] {
        assert_eq!(
            stored_output_selection(&stored)
                .unwrap()
                .voltage_drop_report_format(),
            VoltageDropReportFormat::Extended,
            "{stored}"
        );
    }
    for sheets in [
        json!({}),
        json!({"project_settings":[{"parameter":"report_formats","value":"shp,voltage_drop"}]}),
        json!({"project_settings":[{"parameter":"design_export_format","value":versioned}]}),
    ] {
        assert_eq!(
            project_voltage_drop_report_format(&sheets).unwrap(),
            VoltageDropReportFormat::Extended,
            "{sheets}"
        );
    }
    // An explicit `extended` is honoured and written back as the absence
    // it means, as an empty `execution` map is.
    let explicit = stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "tabular":["voltage_drop"],"voltage_drop_report":"extended"}))
    .unwrap();
    assert_eq!(
        explicit.voltage_drop_report,
        VoltageDropReportFormat::Extended
    );
    assert!(serde_json::to_value(&explicit).unwrap()[VOLTAGE_DROP_REPORT_FIELD].is_null());
}

/// `brief` survives every path a selection travels: reading, authoring
/// normalisation, the settings-row patch and the planner reading the row
/// back — and the reporter's reader answers it.
#[test]
fn a_brief_voltage_drop_layout_round_trips_through_every_path() {
    let stored = json!({"schema":OUTPUT_SELECTION_SCHEMA,"prints":[],
            "geospatial":["shp"],"tabular":["xlsx","voltage_drop"],"voltage_drop_report":"brief"});
    let selection = stored_output_selection(&stored).unwrap();
    assert_eq!(
        selection.voltage_drop_report_format(),
        VoltageDropReportFormat::Brief
    );
    assert_eq!(serde_json::to_value(&selection).unwrap(), stored);
    // Read as ds-brain reads it: trimmed and case-folded, written canonical.
    let spelled = stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "tabular":["voltage_drop"],"voltage_drop_report":" Brief "}))
    .unwrap();
    assert_eq!(
        serde_json::to_value(&spelled).unwrap()[VOLTAGE_DROP_REPORT_FIELD],
        "brief"
    );
    let normalized = normalize_output_selection(Some(selection.clone()), None, None).unwrap();
    assert_eq!(normalized, selection);
    let mut rows = vec![json!({"parameter":"design_export_format","value":"shp"})];
    apply_output_selection(&mut rows, &normalized).unwrap();
    assert_eq!(rows[0]["value"], stored);
    let sheets = json!({"project_settings":rows});
    assert_eq!(
        project_voltage_drop_report_format(&sheets).unwrap(),
        VoltageDropReportFormat::Brief
    );
    assert_eq!(
        plan(&sheets, false)
            .unwrap()
            .iter()
            .map(|output| output.output_id.as_str())
            .collect::<Vec<_>>(),
        ["shp", "xlsx", "voltage_drop"]
    );
    let inspected = inspect_project_settings(&sheets, None).unwrap();
    assert_eq!(inspected["voltage_drop_report"], "brief");
    assert_eq!(
        inspect_project_settings(&json!({}), None).unwrap()["voltage_drop_report"],
        "extended"
    );
}

/// A stored value no report has is refused by name, never read as the
/// default: a typo must not silently become a different report.
#[test]
fn an_unknown_voltage_drop_layout_is_refused_by_name() {
    for value in [json!("summary"), json!(""), json!(2), json!(null)] {
        let stored = json!({"schema":OUTPUT_SELECTION_SCHEMA,
                "tabular":["voltage_drop"],"voltage_drop_report":value});
        let error = stored_output_selection(&stored).unwrap_err();
        assert!(
            error.starts_with("Invalid design output selection:"),
            "{value}: {error}"
        );
        let sheets =
            json!({"project_settings":[{"parameter":"design_export_format","value":stored}]});
        assert!(project_voltage_drop_report_format(&sheets).is_err());
        assert!(inspect_project_settings(&sheets, None).unwrap()["voltage_drop_report"].is_null());
    }
    let error = stored_output_selection(&json!({"schema":OUTPUT_SELECTION_SCHEMA,
            "voltage_drop_report":"summary"}))
    .unwrap_err();
    assert!(
        error.contains(
            "unknown voltage-drop report format `summary`, expected `extended` or `brief`"
        ),
        "{error}"
    );
}

/// Hosts learn the choice, its field and its default from the kernel: the
/// selection schema and the A4 `voltage_drop_pdf` option row both carry it;
/// the raw JSON option has no layout choice.
#[test]
fn the_voltage_drop_layout_is_declared_with_its_default() {
    let schema = output_selection_schema();
    let property = &schema["properties"][VOLTAGE_DROP_REPORT_FIELD];
    assert_eq!(property["default"], "extended", "{schema}");
    assert!(!schema["required"]
        .as_array()
        .is_some_and(|required| required.iter().any(|r| r == VOLTAGE_DROP_REPORT_FIELD)));
    let text = schema.to_string();
    assert!(
        text.contains("\"extended\"") && text.contains("\"brief\""),
        "{text}"
    );
    let expected =
        json!({"field":"voltage_drop_report","values":["extended","brief"],"default":"extended"});
    assert_eq!(voltage_drop_report_option(), expected);
    let rows = options(&[]).unwrap();
    for row in &rows {
        if row["value"] == "voltage_drop_pdf" {
            assert_eq!(row["report_format"], expected);
        } else {
            assert!(row["report_format"].is_null(), "{row}");
        }
        if row["value"] == "voltage_drop_pdf" {
            assert_eq!(row["selection_field"], "geospatial");
            assert_eq!(row["class"], "print");
            assert_eq!(row["paper"], "A4");
        }
        if row["value"] == "voltage_drop" {
            assert_eq!(row["selection_field"], "tabular");
        }
    }
}

/// The layout is project policy the reporter reads from the sealed
/// settings; a one-run override naming one would be silently ignored.
#[test]
fn a_local_override_may_not_carry_a_voltage_drop_layout() {
    let sheets =
        json!({"project_settings":[{"parameter":"report_formats","value":"voltage_drop"}]});
    let brief: DesignOutputSelection = serde_json::from_value(json!({
            "schema":OUTPUT_SELECTION_SCHEMA,"tabular":["voltage_drop"],"voltage_drop_report":"brief"}))
        .unwrap();
    assert!(selected_plan(&sheets, false, Some(&brief))
        .is_err_and(|error| error.contains("never carry a voltage-drop report format")));
    let extended = DesignOutputSelection {
        voltage_drop_report: VoltageDropReportFormat::Extended,
        ..brief
    };
    assert_eq!(
        selected_plan(&sheets, false, Some(&extended))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn individual_geojson_selection_uses_the_reporter_wire_token() {
    let sheets = json!({"project_settings":[{"parameter":"report_formats","value":"shp"}]});
    let selection: DesignOutputSelection = serde_json::from_value(json!({
        "schema":OUTPUT_SELECTION_SCHEMA, "geospatial":["geojson", "shp"]
    }))
    .unwrap();
    let planned = selected_plan(&sheets, false, Some(&selection)).unwrap();
    assert_eq!(
        planned
            .iter()
            .map(|output| output.output_id.as_str())
            .collect::<Vec<_>>(),
        ["shp", "geojson"]
    );
    let geojson = &planned[1];
    assert_eq!(geojson.format, "geojson");
    assert_eq!(geojson.content_type, "application/zip");
    assert_eq!(geojson.suffix, "_geojson.zip");
    assert!(options(&[])
        .unwrap()
        .iter()
        .any(|row| row["value"] == "geojson"));
}
