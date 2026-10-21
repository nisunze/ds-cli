use super::*;
use serde_json::json;

fn page(index: usize, asset_id: &str) -> PreservedPage {
    PreservedPage {
        page_index: index,
        asset_id: asset_id.into(),
        sha256: format!("sha256:{}", "0".repeat(64)),
    }
}

#[test]
fn a_missing_or_surplus_asset_refuses_with_the_reporters_own_code() {
    let pages = [page(0, "cover"), page(3, "key-map")];
    for (supplied, missing, unexpected) in [
        (json!({}), json!(["cover", "key-map"]), json!([])),
        (json!({ "cover": "/h/cover.pdf" }), json!(["key-map"]), json!([])),
        (
            json!({ "cover": "/h/c.pdf", "key-map": "/h/k.pdf", "extra": "/h/e.pdf" }),
            json!([]),
            json!(["extra"]),
        ),
    ] {
        let failure = check_publication_assets(&pages, &supplied).expect_err("must refuse");
        assert_eq!(failure.code(), "mv_print_asset_missing");
        assert_eq!(failure.class().token(), "invalid_input");
        let detail = failure.detail_value().expect("detail");
        assert_eq!(detail["missing"], missing);
        assert_eq!(detail["unexpected"], unexpected);
    }
}

#[test]
fn exactly_the_required_assets_pass_and_a_setup_without_pages_needs_none() {
    let pages = [page(0, "cover")];
    check_publication_assets(&pages, &json!({ "cover": "/h/cover.pdf" })).expect("exact");
    check_publication_assets(&[], &json!({})).expect("no preserved pages");
    check_publication_assets(&[], &Value::Null).expect("absent map");
    assert!(check_publication_assets(&[], &json!({ "x": "/h/x.pdf" })).is_err());
}
