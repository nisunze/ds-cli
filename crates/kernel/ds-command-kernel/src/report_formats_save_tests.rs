//! The Kernel's settings-sheet writer over the shared output matrix.
use super::*;
use serde_json::json;

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
