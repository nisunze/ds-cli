//! Exact artifact binding and legacy display projection shared by native/WASM.
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    artifact: Value,
    binding: Value,
    receipt: Value,
}

pub fn evaluate(result: &[u8], context: &[u8]) -> Result<String, String> {
    if result.is_empty()
        || result.len() > crate::MAX_SOLAR_PORTFOLIO_RESULT_BYTES
        || context.len() > 256 * 1024
    {
        return Err("Solar portfolio read exceeds the shared bound".into());
    }
    let Context {
        artifact: a,
        binding: b,
        receipt: r,
    } = serde_json::from_slice(context).map_err(|_| "Invalid portfolio read context")?;
    let mut d: Value = serde_json::from_slice(result)
        .map_err(|_| "The native Solar portfolio result is not valid JSON.")?;
    let legacy = d["schema_version"] == "ds-solar.portfolio-result/v1";
    if !legacy {
        let read: Value = serde_json::from_str(
            &crate::validate_solar_portfolio_result(result, a["runId"].as_str().unwrap_or(""))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|_| "Invalid shared portfolio projection")?;
        d = read["document"].clone();
    } else {
        legacy_validate(&d)?;
    }
    let cities = d["cities"].as_array().ok_or("Missing portfolio cities")?;
    let ids: Vec<Value> = cities.iter().map(|c| c["city_id"].clone()).collect();
    if d["project_id"] != a["projectId"]
        || d["root"] != a["root"]
        || d["run_id"] != a["runId"]
        || Value::Array(ids.clone()) != b["cityIds"]
        || Value::Array(ids) != a["cityIds"]
        || cities.iter().any(|c| {
            a["cityResultDigests"][c["city_id"].as_str().unwrap_or("")] != c["result_digest"]
        })
    {
        return Err("The validated Solar portfolio does not match the selected portfolio identity, artifact, or committed city batch.".into());
    }
    if !legacy {
        if d["portfolio_id"] != b["portfolioId"]
            || d["portfolio_name"] != b["portfolioName"]
            || (!b["membershipRevision"].is_null()
                && d["membership_revision"] != b["membershipRevision"])
            || d["input_digest"] != r["inputDigest"]
            || d["portfolio_digest"] != r["resultDigest"]
        {
            return Err("The native Solar portfolio result does not match its sealed run receipt or selected portfolio identity.".into());
        }
        let graphs = &d["sections"]["representative_graphs"];
        let strategy = r["representativeStrategy"]
            .as_str()
            .ok_or("Missing representative strategy")?;
        let matches = graphs["_strategy"] == strategy
            && match strategy {
                "first" => {
                    d["representative_city_id"] == b["cityIds"][0]
                        && graphs["_start_index"].is_null()
                }
                "explicit" => {
                    r["representativeCityId"]
                        .as_str()
                        .is_some_and(|id| !id.is_empty())
                        && d["representative_city_id"] == r["representativeCityId"]
                        && graphs["_start_index"].is_null()
                }
                "round_robin" => {
                    graphs["_start_index"] == r["roundRobinStartIndex"]
                        && (d["schema_version"] == "ds-solar.portfolio-result/v2"
                            || d["representative_city_id"].is_null())
                }
                "digest_seeded_random" => graphs["_start_index"].is_null(),
                _ => false,
            };
        if !matches {
            return Err("The native Solar portfolio result does not match its sealed run receipt or representative-graph strategy.".into());
        }
    } else {
        d["legacy_projection"] = legacy_projection(&d);
    }
    serde_json::to_string(&d).map_err(|_| "Cannot encode portfolio read".into())
}

fn number(v: &Value) -> bool {
    v.as_f64().is_some_and(|n| n.is_finite() && n >= 0.)
}
fn integer(v: &Value) -> bool {
    v.as_u64().is_some()
}
fn text(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && s.trim() == s)
}
fn digest(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.strip_prefix("sha256:").is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    })
}
fn legacy_validate(d: &Value) -> Result<(), String> {
    let cities = d["cities"]
        .as_array()
        .ok_or("Invalid legacy portfolio cities")?;
    if !text(&d["engine"]["name"])
        || !text(&d["engine"]["version"])
        || d["city_count"].as_u64() != Some(cities.len() as u64)
        || !integer(&d["total_customers"])
        || !number(&d["annual_energy_kwh"])
        || !digest(&d["portfolio_digest"])
    {
        return Err("Invalid legacy portfolio summary".into());
    }
    let mut seen = BTreeSet::new();
    for c in cities {
        if !text(&c["city_id"])
            || !text(&c["display_name"])
            || !digest(&c["result_digest"])
            || !integer(&c["total_customers"])
            || !number(&c["annual_energy_kwh"])
            || !seen.insert(c["city_id"].as_str().unwrap())
        {
            return Err(
                "The legacy Solar portfolio contains an invalid or unsealed city summary.".into(),
            );
        }
    }
    for (name, s) in d["systems"]
        .as_object()
        .ok_or("Invalid legacy portfolio systems")?
    {
        if name.is_empty()
            || name.trim() != name
            || !integer(&s["cities"])
            || !integer(&s["inverter_count"])
            || [
                "pv_capacity_mw",
                "battery_capacity_mwh",
                "generator_count",
                "diesel_capacity_mw",
            ]
            .iter()
            .any(|f| !number(&s[*f]))
        {
            return Err("The legacy Solar portfolio contains invalid system totals.".into());
        }
    }
    Ok(())
}

fn legacy_projection(d: &Value) -> Value {
    let energy = |v: &Value| v["annual_energy_kwh"].as_f64().unwrap();
    let demand: Vec<Value> = d["cities"].as_array().unwrap().iter().map(|c| json!({"city":c["display_name"],"customers":c["total_customers"],"daily_kwh":energy(c)/365.,"annual_mwh":energy(c)/1000.})).collect();
    let systems = d["systems"].as_object().unwrap();
    let label = |s: &str| {
        match s {
            "hybrid" => "Hybrid PV + BESS + diesel",
            "solar_battery" => "PV + storage",
            "thermal_only" => "Diesel only",
            other => other,
        }
        .to_string()
    };
    let equipment: Vec<Value> = systems.iter().map(|(name,s)| json!({"city":format!("Portfolio ({})",s["cities"]),"system_type":label(name),"pv_mw":s["pv_capacity_mw"],"storage_mwh":s["battery_capacity_mwh"],"inverters":s["inverter_count"],"generator_config":if s["generator_count"].as_f64().unwrap() > 0. { format!("{} / {:.3} MW",s["generator_count"],s["diesel_capacity_mw"].as_f64().unwrap()) } else { "—".into() }})).collect();
    let sum = |field: &str| {
        systems
            .values()
            .map(|s| s[field].as_f64().unwrap())
            .sum::<f64>()
    };
    json!({
        "demand":{"table":demand,"aggregations":{"total_customers":d["total_customers"],"total_annual_mwh":energy(d)/1000.,"total_daily_kwh":energy(d)/365.}},
        "scenarios":{"table":systems.iter().map(|(name,s)| json!({"city":"Portfolio","selected_system":label(name),"cities":s["cities"]})).collect::<Vec<_>>()},
        "performance":{"equipment_table":equipment,"energy_table":demand.iter().map(|c|json!({"city":c["city"],"daily_load_mwh":c["daily_kwh"].as_f64().unwrap()/1000.,"energy_year1_mwh":c["annual_mwh"]})).collect::<Vec<_>>(),"aggregations":{"total_pv_mw":sum("pv_capacity_mw"),"total_storage_mwh":sum("battery_capacity_mwh"),"total_inverters":sum("inverter_count"),"total_energy_year1_mwh":energy(d)/1000.}}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value, Value) {
        let digest = format!("sha256:{}", "d".repeat(64));
        let result = json!({"schema_version":"ds-solar.portfolio-result/v1","engine":{"name":"solar","version":"1"},"project_id":"p","root":"root","run_id":"r","cities":[{"city_id":"a","display_name":"A","result_digest":digest,"total_customers":5,"annual_energy_kwh":365000}],"city_count":1,"total_customers":5,"annual_energy_kwh":365000,"portfolio_digest":digest,"systems":{"hybrid":{"cities":1,"pv_capacity_mw":2.5,"battery_capacity_mwh":4,"inverter_count":2,"generator_count":0,"diesel_capacity_mw":0}}});
        let context = json!({"artifact":{"projectId":"p","root":"root","runId":"r","cityIds":["a"],"cityResultDigests":{"a":digest}},"binding":{"cityIds":["a"]},"receipt":{}});
        (result, context)
    }
    #[test]
    fn legacy_projection_is_owner_computed_and_rejects_changed_city_seals() {
        let (result, mut context) = fixture();
        let read: Value = serde_json::from_str(
            &evaluate(
                &serde_json::to_vec(&result).unwrap(),
                &serde_json::to_vec(&context).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            read["legacy_projection"]["performance"]["aggregations"]["total_pv_mw"],
            2.5
        );
        assert_eq!(
            read["legacy_projection"]["demand"]["aggregations"]["total_daily_kwh"],
            1000.
        );
        context["artifact"]["cityResultDigests"]["a"] = json!(format!("sha256:{}", "e".repeat(64)));
        assert!(evaluate(
            &serde_json::to_vec(&result).unwrap(),
            &serde_json::to_vec(&context).unwrap()
        )
        .is_err());
    }
    #[test]
    fn legacy_summary_refuses_invalid_counts_and_foreign_project() {
        let (mut result, mut context) = fixture();
        result["systems"]["hybrid"]["inverter_count"] = json!(-1);
        assert!(evaluate(
            &serde_json::to_vec(&result).unwrap(),
            &serde_json::to_vec(&context).unwrap()
        )
        .is_err());
        let (result, _) = fixture();
        context["artifact"]["projectId"] = json!("other");
        assert!(evaluate(
            &serde_json::to_vec(&result).unwrap(),
            &serde_json::to_vec(&context).unwrap()
        )
        .is_err());
    }
}
