//! Bounded Survey receipt projections. Hosts perform IO only.
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
fn count(v: &Value) -> f64 {
    v.as_f64()
        .filter(|v| v.is_finite())
        .unwrap_or(0.)
        .trunc()
        .max(0.)
}
/// A whole, non-negative count; anything else the service sent counts zero.
fn whole(v: &Value) -> u64 {
    count(v) as u64
}
/// Rows one migration receipt keeps per count map. The rest are counted in
/// `more`, never dropped silently.
pub const MIGRATION_RECEIPT_ROWS: usize = 100;
fn counts(v: &Value) -> (Value, usize) {
    let rows: Vec<_> = v
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.as_f64().is_some_and(f64::is_finite))
        .collect();
    let output: Map<_, _> = rows
        .iter()
        .take(MIGRATION_RECEIPT_ROWS)
        .map(|(key, v)| (key.chars().take(160).collect(), json!(whole(v))))
        .collect();
    (
        json!(output),
        rows.len().saturating_sub(MIGRATION_RECEIPT_ROWS),
    )
}

/// The one receipt for a project-to-project survey-data migration, shaped
/// from the migration service's own answer (`result`: ds-brain's
/// `MigrateResponse`).
///
/// Migration is stateless: `source` INTO `destination`, both named by the
/// caller, and `dry_run` is the plan. The request this receipt answers copies
/// every entry and never asks for source deletion or target overwrite, so
/// `overwrite_existing` is always `false`; `source_preserved` is read from the
/// service's own deletion count rather than asserted.
pub fn migration_receipt(result: &Value, source: &str, destination: &str, dry_run: bool) -> Value {
    let (forms, forms_omitted) = counts(&result["per_form"]);
    let (skips, skips_omitted) = counts(&result["skip_reasons"]);
    let deleted = whole(&result["total_source_deleted"]);
    let mut receipt = json!({
        "project": {"ds_project": destination},
        "source_project": source,
        "mode": if dry_run { "plan" } else { "apply" },
        "total_matched": whole(&result["total_matched"]),
        "total_migrated": whole(&result["total_migrated"]),
        "total_skipped": whole(&result["total_skipped"]),
        "total_target_written": whole(&result["total_target_written"]),
        "total_source_deleted": deleted,
        "per_form": forms,
        "skip_reasons": skips,
        "source_preserved": deleted == 0,
        "overwrite_existing": false,
    });
    if forms_omitted + skips_omitted > 0 {
        receipt["more"] = json!({
            "per_form_omitted": forms_omitted,
            "skip_reasons_omitted": skips_omitted,
        });
    }
    receipt
}
fn args(r: &Value, allowed: &str) -> Result<(), String> {
    let object = r["args"]
        .as_object()
        .ok_or("Survey arguments must be an object")?;
    for key in object.keys() {
        if key != allowed {
            return Err(format!(
                "{} does not accept {key}.",
                r["command"].as_str().unwrap_or("survey")
            ));
        }
    }
    Ok(())
}
pub(super) fn evaluate(r: &Value) -> Result<Value, String> {
    match r["mode"].as_str() {
        Some("download_admit") => {
            args(r, "entireProject")?;
            if r["args"]["entireProject"] != true {
                return Err("entireProject must be true; unscoped survey downloads require explicit intent.".into());
            }
            if r["uid"].as_str().unwrap_or("").is_empty() {
                return Err("Sign in to DS GridDesign before downloading survey data.".into());
            }
            if r["project"].as_str().unwrap_or("").is_empty() {
                return Err("No active project is available for the Working Area download.".into());
            }
            Ok(Value::Null)
        }
        Some("download_plan") => {
            let slugs: BTreeSet<_> = r["layers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l["key"].as_str())
                .filter(|s| !s.is_empty())
                .collect();
            Ok(
                json!({"slugs":slugs,"working_area":{"mode":"full","admin":null,"drawBoundary":null,"fullProjectLoad":true,
                "dateFrom":"","dateTo":"","surveyors":[],"applied":true,"appliedAt":r["now"],"appliedCenter":r["center"],"appliedZoom":r["zoom"],
                "appliedBbox":null}}),
            )
        }
        Some("download_receipt") => {
            let before = &r["before"];
            let after = &r["after"];
            let sum = |v: &Value| {
                v.as_object()
                    .into_iter()
                    .flatten()
                    .map(|(_, v)| count(v))
                    .sum::<f64>()
            };
            let materialized: Map<String, Value> = r["slugs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|s| {
                    (
                        s.into(),
                        json!((count(&after[s]) - count(&before[s])).max(0.)),
                    )
                })
                .collect();
            Ok(
                json!({"project":r["project"],"workingArea":{"mode":"full","fullProjectLoad":true,"applied":true},
                "forms":materialized.len(),"before":before,"after":after,"cachedTotal":sum(after),"materialized":materialized,
                "materializedTotal":(sum(after)-sum(before)).max(0.)}),
            )
        }
        _ => Err("Unknown Survey bridge operation".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_are_bounded_and_truncation_is_visible() {
        let values: Map<String, Value> = (0..105).map(|i| (format!("f{i:03}"), json!(i))).collect();
        let r = migration_receipt(&json!({ "per_form": values }), "src", "dst", true);
        assert_eq!(r["per_form"].as_object().unwrap().len(), 100);
        assert_eq!(r["more"]["per_form_omitted"], 5);
        assert_eq!(r["more"]["skip_reasons_omitted"], 0);
    }
    #[test]
    fn the_migration_receipt_names_both_projects_and_reads_what_the_service_did() {
        let service = json!({"dry_run":false,"total_matched":14,"total_migrated":12,"total_skipped":2,
            "total_target_written":12,"per_form":{"customers":12},"skip_reasons":{"existing":2}});
        let r = migration_receipt(&service, "src", "dst", false);
        assert_eq!(r["project"]["ds_project"], "dst");
        assert_eq!(r["source_project"], "src");
        assert_eq!(r["mode"], "apply");
        // Whole counts, never floats: a native host reads them as integers.
        assert_eq!(r["total_target_written"].as_u64(), Some(12));
        assert_eq!(r["skip_reasons"], json!({"existing":2}));
        assert_eq!(r["source_preserved"], true);
        assert_eq!(r["overwrite_existing"], false);
        assert!(r.get("more").is_none());
        // Preservation is read from the service, never asserted over it.
        let deleted = migration_receipt(&json!({"total_source_deleted":3}), "src", "dst", false);
        assert_eq!(deleted["source_preserved"], false);
        assert_eq!(
            migration_receipt(&json!({}), "src", "dst", true)["mode"],
            "plan"
        );
    }
    #[test]
    fn the_download_plan_names_every_survey_layer_and_carries_no_window_cursor() {
        let plan = evaluate(
            &json!({"mode":"download_plan","now":1,"center":null,"zoom":null,
            "layers":[{"key":"poles"},{"key":"customers"},{"key":""},{"label":"no key"}]}),
        )
        .unwrap();
        assert_eq!(plan["slugs"], json!(["customers", "poles"]));
        assert_eq!(plan["working_area"]["fullProjectLoad"], true);
        // Cursors are the core's (`survey::hold`); the window's working area
        // carries none.
        for key in ["cutoffAt", "cutoffByForm", "lastUpdatedAt"] {
            assert!(plan["working_area"].get(key).is_none(), "{key}");
        }
    }
    #[test]
    fn explicit_download_intent_is_required_and_no_migration_mode_remains() {
        assert!(
            evaluate(&json!({"mode":"download_admit","args":{},"uid":"u","project":"p"})).is_err()
        );
        // Migration is a native call with both projects explicit; the bridge
        // modes that took the target from the window's active project are gone.
        for mode in ["migration_plan", "migration_receipt"] {
            assert!(evaluate(&json!({"mode":mode,"args":{"sourceProject":"p"}})).is_err());
        }
    }
}
