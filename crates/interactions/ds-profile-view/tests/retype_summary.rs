use ds_profile_view::payload::evaluate;
use serde_json::{Value, json};
#[test]
fn native_repair_receipt_controls_summary_and_tone_in_original_order() {
    let repair = |section: &str, sequence: u32| json!({"section_id":section,"sequence":sequence,"structure_id":"pole-a","previous_attachment_point_id":"ap-old","set_label":"GW","slot":0});
    let result: Value = serde_json::from_str(&evaluate(&json!({"kind":"retype_summary","outcome":{"outcomes":[{"delta":{"unresolved_retype_attachments":[repair("phase",2),repair("gw",3),repair("gw",4)]}}]}}).to_string()).unwrap()).unwrap();
    assert_eq!(result["tone"], "warning");
    assert_eq!(
        result["message"],
        "Draft type applied: 3 attachments need repair in 2 sections."
    );
    assert_eq!(
        result["lines"][0],
        "phase · support 2 · structure pole-a · GW, slot 0 · previous attachment ap-old"
    );
    assert_eq!(
        result["lines"][2],
        "gw · support 4 · structure pole-a · GW, slot 0 · previous attachment ap-old"
    );
}
#[test]
fn accepted_receipt_without_repairs_uses_supplied_translation() {
    let result: Value = serde_json::from_str(&evaluate(&json!({"kind":"retype_summary","outcome":{"outcomes":[{"delta":{}}]},"translations":{"gprof_retype_applied":"Types appliqués."}}).to_string()).unwrap()).unwrap();
    assert_eq!(result["tone"], "success");
    assert_eq!(result["message"], "Types appliqués.");
    assert_eq!(result["lines"], json!([]));
}
#[test]
fn malformed_native_receipt_is_refused_instead_of_claiming_success() {
    assert!(evaluate(r#"{"kind":"retype_summary","outcome":{}}"#).is_err());
}
