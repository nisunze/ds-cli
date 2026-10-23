use super::*;
use serde_json::json;
#[test]
fn delivery_rejects_foreign_project_duplicate_and_tampered_parts() {
    let rows = [
        "weather.json",
        "reference-unit.json",
        "reference-unit.parquet",
        "manifest.json",
    ]
    .map(
        |name| json!({"name":name,"bytes":1,"sha256":digest::hex_sha256(b"x"),"base64":"eA=="}),
    );
    let value = json!({"project":"p","artifacts":rows});
    assert!(bundle_from_delivery("p", &value).is_ok());
    assert!(bundle_from_delivery("other", &value).is_err());
    let mut changed = value.clone();
    changed["artifacts"][0]["sha256"] = json!("0".repeat(64));
    assert!(bundle_from_delivery("p", &changed).is_err());
    let mut changed = value;
    changed["artifacts"][1] = changed["artifacts"][0].clone();
    assert!(bundle_from_delivery("p", &changed).is_err());
}
