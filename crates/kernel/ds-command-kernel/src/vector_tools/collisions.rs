//! Read the reporter's collision metadata. Detection and overlap geometry remain
//! with the reporter; this is the former UI summary fold, shared by all hosts.
use ds_geo::layer::Feature;
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn numeric(p: &Value, key: &str) -> Option<f64> {
    p[key].as_f64().filter(|n| n.is_finite())
}
pub fn computed_at(feature: &Feature) -> Option<i64> {
    let text = feature.properties.get("computed_at")?.as_str()?;
    time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
}
pub fn transformers(feature: &Feature) -> Vec<String> {
    let p = Value::Object(feature.properties.clone());
    if let Some(text) = p["transformers"].as_str().filter(|s| !s.trim().is_empty()) {
        return text
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
    }
    let a = p["transfo_a"].as_str().unwrap_or("");
    let b = p["transfo_b"].as_str().unwrap_or("");
    if a.is_empty() && b.is_empty() {
        vec![]
    } else {
        vec![a.into(), b.into()]
    }
}
fn region(feature: &Feature, index: usize) -> Value {
    let p = Value::Object(feature.properties.clone());
    let names = transformers(feature);
    let id = p["collision_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            feature
                .id
                .as_ref()
                .map(|v| {
                    v.as_str()
                        .map(String::from)
                        .unwrap_or_else(|| v.to_string())
                })
                .unwrap_or_else(|| index.to_string())
        });
    json!({"id":id,"transformers":names,"transformerCount":numeric(&p,"tr_count").unwrap_or(names.len() as f64),"pairCount":numeric(&p,"collision_count").unwrap_or(1.),"coveragePct":numeric(&p,"coverage_pct"),"overlapClass":p["overlap_class"].as_str().filter(|s|matches!(*s,"duplicated"|"overlapping")),"canConsolidate":p["can_consolidate"]==true,"lvCrossings":numeric(&p,"lv_line_crossings").unwrap_or(0.),"serviceCrossings":numeric(&p,"service_cable_crossings").unwrap_or(0.),"sharedCustomers":numeric(&p,"shared_customers").unwrap_or(0.),"feature_index":index})
}
pub fn summarize(
    features: &[Feature],
    loaded: bool,
    computed: bool,
    last_read: Option<bool>,
) -> Value {
    let mut regions: Vec<_> = features
        .iter()
        .enumerate()
        .map(|(i, f)| region(f, i))
        .collect();
    let ranked = regions.iter().any(|r| r["coveragePct"].is_number());
    regions.sort_by(|a, b| {
        let score = |r: &Value, key: &str, default: f64| r[key].as_f64().unwrap_or(default);
        let evidence = |r: &Value| score(r, "lvCrossings", 0.) + score(r, "serviceCrossings", 0.);
        let id_order = || a["id"].as_str().unwrap().cmp(b["id"].as_str().unwrap());
        if ranked {
            score(b, "coveragePct", -1.)
                .total_cmp(&score(a, "coveragePct", -1.))
                .then_with(|| evidence(b).total_cmp(&evidence(a)))
                .then_with(|| {
                    score(b, "sharedCustomers", 0.).total_cmp(&score(a, "sharedCustomers", 0.))
                })
                .then_with(|| {
                    score(b, "transformerCount", 0.).total_cmp(&score(a, "transformerCount", 0.))
                })
                .then_with(id_order)
        } else {
            id_order()
        }
    });
    let involved = regions
        .iter()
        .flat_map(|r| {
            r["transformers"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
        })
        .collect::<BTreeSet<_>>();
    let phase = if !loaded {
        "idle"
    } else if last_read == Some(false) && !regions.is_empty() {
        "held"
    } else if !computed {
        "never"
    } else if regions.is_empty() {
        "clear"
    } else {
        "found"
    };
    json!({"phase":phase,"regions":regions,"total":regions.len(),"ranked":ranked,"transformerCount":involved.len(),"consolidatableCount":regions.iter().filter(|r|r["canConsolidate"]==true).count(),"computedAtMs":features.iter().filter_map(computed_at).max(),"phaseCopy":phase_copy(phase)})
}
pub fn phase_copy(phase: &str) -> Value {
    let tone = match phase {
        "clear" => "good",
        "found" => "alert",
        "held" => "warn",
        _ => "neutral",
    };
    json!({"tone":tone,"badgeKey":format!("dsn_collisions_badge_{phase}"),"headlineKey":format!("dsn_collisions_headline_{phase}"),"hintKey":format!("dsn_collisions_hint_{phase}")})
}
