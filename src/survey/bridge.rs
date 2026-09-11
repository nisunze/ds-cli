//! Bounded Survey bridge intent and receipt projections. Hosts perform IO only.
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
fn count(v: &Value) -> f64 {
    v.as_f64()
        .filter(|v| v.is_finite())
        .unwrap_or(0.)
        .trunc()
        .max(0.)
}
fn counts(v: &Value) -> (Value, usize) {
    let rows: Vec<_> = v
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.as_f64().is_some_and(f64::is_finite))
        .collect();
    let output: Map<_, _> = rows
        .iter()
        .take(100)
        .map(|(key, v)| (key.chars().take(160).collect(), json!(count(v))))
        .collect();
    (json!(output), rows.len().saturating_sub(100))
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
        Some("migration_plan") => {
            args(r, "sourceProject")?;
            let source = r["args"]["sourceProject"].as_str().unwrap_or("");
            if source.is_empty()
                || source.len() > 160
                || !source.as_bytes()[0].is_ascii_alphanumeric()
                || !source
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err("sourceProject must be a canonical Data Solutions project id.".into());
            }
            if r["uid"].as_str().unwrap_or("").is_empty() {
                return Err("Sign in to DS GridDesign before migrating survey data.".into());
            }
            let target = r["project"].as_str().unwrap_or("").trim();
            if target.is_empty() {
                return Err(
                    "No active project is available as the survey migration target.".into(),
                );
            }
            if source == target {
                return Err("The survey migration source and target are the same project.".into());
            }
            Ok(json!({"sourceProjectId":source,"targetProjectId":target,"dryRun":r["dry_run"]}))
        }
        Some("migration_receipt") => {
            let result = &r["result"];
            let (forms, f_omitted) = counts(&result["per_form"]);
            let (skips, s_omitted) = counts(&result["skip_reasons"]);
            let mut output = json!({"status":result["status"],"sourceProject":r["source"],"targetProject":r["target"],"dryRun":r["dry_run"],
                "totalMatched":count(&result["total_matched"]),"totalMigrated":count(&result["total_migrated"]),"totalSkipped":count(&result["total_skipped"]),
                "totalTargetWritten":count(&result["total_target_written"]),"totalSourceDeleted":count(&result["total_source_deleted"]),
                "perForm":forms,"skipReasons":skips,"sourcePreserved":true,"overwriteExisting":false});
            if f_omitted + s_omitted > 0 {
                output["more"] = json!({"perFormOmitted":f_omitted,"skipReasonOmitted":s_omitted});
            }
            Ok(output)
        }
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
                "appliedBbox":null,"lastUpdatedAt":null,"cutoffAt":null,"cutoffByForm":{}}}),
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
        let r =
            evaluate(&json!({"mode":"migration_receipt","result":{"per_form":values}})).unwrap();
        assert_eq!(r["perForm"].as_object().unwrap().len(), 100);
        assert_eq!(r["more"]["perFormOmitted"], 5);
    }
    #[test]
    fn source_and_explicit_download_intent_are_required() {
        assert!(
            evaluate(&json!({"mode":"migration_plan","args":{"sourceProject":"../other"}}))
                .is_err()
        );
        assert!(
            evaluate(&json!({"mode":"download_admit","args":{},"uid":"u","project":"p"})).is_err()
        );
    }
}
