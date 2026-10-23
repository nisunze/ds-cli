//! Gisagara's MV booklet refused on its first context page: "print symbol
//! `ds-transformer` is missing from held symbol_assets". The icons live in
//! the configuration's sealed reporter receipt, not its plain sheets.
use serde_json::json;

use super::sheets_with_sealed_symbols;

#[test]
fn mv_print_sheets_carry_the_receipt_sealed_icons() {
    let sealed = json!({"printing_symbol_assets":{"ds-transformer":{"label":"t"}}});
    let document = json!({
        "sheets": {"project_settings": []},
        "network_reporter_input_receipt": {"sheets_json": sealed.to_string()},
    });
    let sheets = sheets_with_sealed_symbols(&document);
    assert_eq!(sheets["printing_symbol_assets"]["ds-transformer"]["label"], "t");
    assert_eq!(sheets["project_settings"], json!([]));
}

#[test]
fn plain_icons_win_and_a_missing_receipt_changes_nothing() {
    let own = json!({"sheets": {"printing_symbol_assets": {"own": {}}},
        "network_reporter_input_receipt": {"sheets_json":
            json!({"printing_symbol_assets":{"sealed":{}}}).to_string()}});
    assert_eq!(sheets_with_sealed_symbols(&own)["printing_symbol_assets"], json!({"own": {}}));
    let bare = json!({"sheets": {"project_settings": []}});
    assert_eq!(sheets_with_sealed_symbols(&bare), json!({"project_settings": []}));
}
