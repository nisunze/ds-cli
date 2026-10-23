use super::*;
use serde_json::json;

fn row(by: Option<&str>, at: Value, admin: Option<[&str; 4]>, deleted: bool) -> Value {
    let mut properties =
        json!({"doc_id": "d", "metadata": {"is_deleted": deleted, "created_at": at}});
    if let Some(by) = by {
        properties["metadata"]["created_by"] = json!(by);
    }
    if let Some([district, sector, cell, village]) = admin {
        properties["admin_district"] = json!(district);
        properties["admin_sector"] = json!(sector);
        properties["admin_cell"] = json!(cell);
        properties["admin_village"] = json!(village);
    }
    json!({"type": "Feature", "geometry": null, "properties": properties, "future": true})
}

fn request(forms: Value, rwanda: bool) -> Request {
    serde_json::from_value(json!({
        "forms": forms, "rwanda": rwanda,
        "generated_at": "2026-10-22T08:00:00Z", "generated_by": "agent@example.rw"
    }))
    .unwrap()
}

fn fixture(rwanda: bool) -> Value {
    let huye = Some(["Huye", "Tumba", "Cyarwa", "Karambi"]);
    let report = aggregate(&request(
        json!([
            {"form": "lv_poles_as_built", "rows": [
                row(Some("a@x.rw"), json!("2026-10-01T09:00:00Z"), huye, false),
                row(Some("a@x.rw"), json!("2026-10-01T23:30:00+00:00"), huye, false),
                row(Some("b@x.rw"), json!("2026-10-03T10:00:00Z"), Some(["Huye", "", "", ""]), false),
                row(Some("c@x.rw"), json!("2026-10-03T10:00:00Z"), None, true),
            ]},
            {"form": "customers", "rows": [
                row(Some("b@x.rw"), json!(1_790_845_200_000_i64), None, false),
                row(None, json!(null), None, false),
            ]},
            {"form": "empty_form", "rows": [row(Some("a@x.rw"), json!("2026-10-02T00:00:00Z"), None, true)]}
        ]),
        rwanda,
    ))
    .unwrap();
    report
}

/// The report Brain's BigQuery views produced, from the rows the edge holds:
/// deleted rows excluded, zero-entry forms pruned, surveyor spread with the
/// even-count integer median, a dense distinct-surveyor series with gaps, and
/// the Rwanda breakdown folding blanks into `Unknown`.
#[test]
fn the_held_rows_give_the_dashboard_report() {
    let report = fixture(true);
    assert_eq!(report["schema"], SCHEMA);
    assert_eq!(report["source"], "edge");
    assert_eq!(report["generatedBy"], "agent@example.rw");
    assert_eq!(
        report["totals"]["lv_poles_as_built"],
        json!({"active": 3, "deleted": 0, "total": 3})
    );
    assert_eq!(
        report["totals"]["customers"],
        json!({"active": 2, "deleted": 0, "total": 2})
    );
    assert!(report["totals"].get("empty_form").is_none());
    assert_eq!(report["activeGrandTotal"], 5);
    assert_eq!(
        report["byDay"]["2026-10-01"],
        json!({"lv_poles_as_built": 2, "customers": 1, "total": 3})
    );
    assert_eq!(
        report["byDay"]["2026-10-03"],
        json!({"lv_poles_as_built": 1, "total": 1})
    );
    assert!(report["byDay"].get("2026-10-02").is_none());
    assert_eq!(
        report["bySurveyor"]["a@x.rw"],
        json!({"lv_poles_as_built": 2, "total": 2})
    );
    assert_eq!(
        report["bySurveyor"]["b@x.rw"],
        json!({"lv_poles_as_built": 1, "customers": 1, "total": 2})
    );
    assert!(report["bySurveyor"].get("c@x.rw").is_none());
    assert_eq!(
        report["surveyorStats"],
        json!({"median": 2, "min": 2, "max": 2})
    );
    assert_eq!(
        report["activeSurveyorsByDay"],
        json!({"startDate": "2026-10-01", "counts": {"all": [2, 0, 1]}})
    );
    let huye = &report["byAdmin"]["Huye"];
    assert_eq!(huye["total"], 3);
    assert_eq!(
        huye["sectors"]["Tumba"]["cells"]["Cyarwa"]["villages"]["Karambi"],
        2
    );
    assert_eq!(
        huye["sectors"]["Unknown"]["cells"]["Unknown"]["villages"]["Unknown"],
        1
    );
}

#[test]
fn the_administrative_breakdown_is_rwanda_only_and_the_median_is_integer() {
    assert_eq!(fixture(false)["byAdmin"], json!({}));
    assert_eq!(spread(vec![1, 4]), json!({"median": 2, "min": 1, "max": 4}));
    assert_eq!(
        spread(vec![5, 1, 3]),
        json!({"median": 3, "min": 1, "max": 5})
    );
    assert_eq!(spread(vec![]), json!({"median": 0, "min": 0, "max": 0}));
}

#[test]
fn nothing_held_is_an_empty_report_not_an_error() {
    let report = aggregate(&request(json!([]), true)).unwrap();
    assert_eq!(report["activeGrandTotal"], 0);
    assert_eq!(report["activeSurveyorsByDay"], json!({}));
    assert_eq!(report["totals"], json!({}));
}

/// Malformed requests refuse by name; bounds hold; additive row members are
/// accepted (the rows are the cloud's, which grow).
#[test]
fn malformed_or_unbounded_requests_refuse() {
    for bad in [
        json!({"forms": [], "generated_at": "yesterday"}),
        json!({"forms": [{"form": "Bad Slug", "rows": []}], "generated_at": "2026-10-22T08:00:00Z"}),
        json!({"forms": [{"form": "a", "rows": []}, {"form": "a", "rows": []}], "generated_at": "2026-10-22T08:00:00Z"}),
        json!({"forms": [{"form": "a", "rows": [1]}], "generated_at": "2026-10-22T08:00:00Z"}),
        json!({"forms": [], "generated_at": "2026-10-22T08:00:00Z", "generated_by": ""}),
    ] {
        let refusal = evaluate(bad.to_string().as_bytes()).unwrap_err();
        assert_eq!(refusal.code, REQUEST_INVALID, "{bad}");
    }
    assert_eq!(
        evaluate(br#"{"forms":[],"generated_at":"2026-10-22T08:00:00Z","extra":1}"#)
            .unwrap_err()
            .code,
        REQUEST_INVALID
    );
    let many: Vec<Value> = (0..=MAX_FORMS)
        .map(|i| json!({"form": format!("f{i}"), "rows": []}))
        .collect();
    assert!(aggregate(&request(json!(many), false)).is_err());
    let ok = evaluate(
        json!({"forms": [{"form": "a", "rows": [row(Some("x@y.rw"), json!("2026-10-01T00:00:00Z"), None, false)]}],
               "generated_at": "2026-10-22T08:00:00Z"})
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    assert!(ok.contains("\"activeGrandTotal\":1"));
}
