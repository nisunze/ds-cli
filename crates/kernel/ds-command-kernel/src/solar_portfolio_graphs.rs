//! Bound representative graph read plans; browser hosts own only I/O and object URLs.
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const SECTIONS: &[&str] = &[
    "site",
    "demand",
    "network",
    "performance",
    "investment",
    "financial_kpis",
    "lcoe",
];
const MAX_BYTES: u64 = 64 * 1024 * 1024;
pub fn address(input: &[u8]) -> Result<String, String> {
    if input.len() > 16 * 1024 {
        return Err("Portfolio graph address exceeds its bound".into());
    }
    let value: Value = serde_json::from_slice(input).map_err(|_| "Invalid graph address")?;
    Ok(crate::solar_dashboard::portfolio_views::graph_url(&value, 0).unwrap_or_default())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    graphs: Value,
    receipt: Value,
}
fn text(v: &Value) -> Option<&str> {
    v.as_str()
        .filter(|s| !s.is_empty() && s.trim() == *s && !s.chars().any(char::is_control))
}
fn digest(v: &Value) -> bool {
    text(v)
        .and_then(|s| s.strip_prefix("sha256:"))
        .is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}
fn expected_name(section: &str, key: &str, city: &str) -> Option<String> {
    if key.is_empty()
        || key.len() > 160
        || !key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return None;
    }
    let family = match section {
        "site" | "demand" => "site",
        "performance" => "plant",
        "investment" | "financial_kpis" | "lcoe" => "finance",
        _ => return None,
    };
    Some(format!(
        "cities/{city}/plots/{family}/{}.png",
        key.strip_suffix("_image").unwrap_or(key)
    ))
}
fn unavailable_kind(key: &str) -> &str {
    let key = key
        .strip_prefix("hybrid_")
        .or_else(|| key.strip_prefix("solar_battery_"))
        .unwrap_or(key);
    match key.strip_suffix("_image").unwrap_or(key) {
        "monthly_avg_daily_energy" => "monthly_average_daily_energy",
        "financial_25yr_overview" => "financial_overview",
        value => value,
    }
}
fn graph_receipt(value: &Value, source: &Value, city: &str, name: &str) -> bool {
    const FIELDS: &[&str] = &[
        "schema",
        "source_batch_id",
        "source_batch_digest",
        "name",
        "role",
        "city_id",
        "media_type",
        "byte_count",
        "content_digest",
    ];
    value.as_object().is_some_and(|v| {
        v.len() == FIELDS.len() && FIELDS.iter().all(|field| v.contains_key(*field))
    }) && value["schema"] == "ds-solar.graph-artifact-receipt/v1"
        && value["source_batch_id"] == source["sourceBatchId"]
        && value["source_batch_digest"] == source["sourceBatchDigest"]
        && value["name"] == name
        && value["role"] == "chart"
        && value["city_id"] == city
        && value["media_type"] == "image/png"
        && value["byte_count"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= MAX_BYTES)
        && digest(&value["content_digest"])
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.is_empty() || input.len() > 32 * 1024 * 1024 {
        return Err("Portfolio graph plan exceeds its bound".into());
    }
    let Input { graphs, receipt } =
        serde_json::from_slice(input).map_err(|_| "Invalid closed portfolio graph request")?;
    let ids = receipt["cityIds"]
        .as_array()
        .ok_or("Portfolio graph source lacks city membership")?;
    let cities = ids.iter().filter_map(text).collect::<BTreeSet<_>>();
    if cities.is_empty()
        || cities.len() != ids.len()
        || cities
            .iter()
            .any(|city| ds_solar_contracts::batch::validate_segment(city, "graph.city_id").is_err())
        || text(&receipt["sourceBatchId"]).is_none()
        || !digest(&receipt["sourceBatchDigest"])
    {
        return Err("Invalid portfolio graph source membership or batch identity".into());
    }
    let positions: BTreeMap<_, _> = graphs["graphs"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, row)| {
            Some((
                (
                    row["section"].as_str()?.to_owned(),
                    row["graph_key"].as_str()?.to_owned(),
                ),
                index,
            ))
        })
        .collect();
    let graphs = crate::solar_portfolio_read::graph_groups(graphs)?;
    let mut targets = Vec::new();
    for section in SECTIONS {
        let Some(group) = graphs[*section].as_object() else {
            continue;
        };
        if group.len() > 128 {
            return Err("Excessive portfolio graph section".into());
        }
        for (key, candidate) in group {
            if !candidate.is_object() {
                continue;
            }
            let row_city = text(&candidate["city_id"]).filter(|city| cities.contains(city));
            let receipt_city =
                text(&candidate["url"]["city_id"]).filter(|city| cities.contains(city));
            let city = if row_city.is_some() && receipt_city.is_some() && row_city != receipt_city {
                None
            } else {
                receipt_city.or(row_city)
            };
            let name = city.and_then(|city| expected_name(section, key, city));
            let admitted = city
                .zip(name.as_deref())
                .is_some_and(|(city, name)| graph_receipt(&candidate["url"], &receipt, city, name));
            let unavailable = json!({"kind":unavailable_kind(key),"city_id":city.or_else(||text(&candidate["city_id"])).unwrap_or("unknown"),
                "missing_field":name.clone().unwrap_or_else(||format!("representative_graphs.{section}.{key}"))});
            targets.push(json!({"section":section,"key":key,"ordered_index":positions.get(&(section.to_string(),key.clone())),"receipt":admitted.then(||candidate["url"].clone()),"unavailable":unavailable,
                "identity":admitted.then(||format!("{}\0{}\0{}",candidate["url"]["name"].as_str().unwrap(),candidate["url"]["content_digest"].as_str().unwrap(),candidate["url"]["byte_count"].as_u64().unwrap()))}));
        }
    }
    serde_json::to_string(&json!({"targets":targets,"unavailable":graphs["_unavailable"].as_array().cloned().unwrap_or_default()})).map_err(|_| "Cannot encode portfolio graph plan".into())
}
pub fn verify_bytes(bytes: &[u8], context: &[u8]) -> Result<(), String> {
    if context.len() > 4096 || bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
        return Err("Portfolio graph verification exceeds its bound".into());
    }
    let receipt: Value =
        serde_json::from_slice(context).map_err(|_| "Invalid graph verification receipt")?;
    if receipt["schema"] != "ds-solar.graph-artifact-receipt/v1"
        || receipt["media_type"] != "image/png"
        || receipt["byte_count"].as_u64() != Some(bytes.len() as u64)
        || !digest(&receipt["content_digest"])
        || receipt["content_digest"].as_str()
            != Some(ds_solar_contracts::digest::sha256_hex(bytes).as_str())
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return Err("Portfolio graph bytes do not match the sealed image receipt".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_membership_batch_and_exact_path_fence_graph_reads() {
        let source = json!({"cityIds":["city_a"],"sourceBatchId":"batch","sourceBatchDigest":format!("sha256:{}","a".repeat(64))});
        let mut receipt = json!({"schema":"ds-solar.graph-artifact-receipt/v1","source_batch_id":"batch","source_batch_digest":source["sourceBatchDigest"],"name":"cities/city_a/plots/site/monthly_avg_daily_energy.png","role":"chart","city_id":"city_a","media_type":"image/png","byte_count":100,"content_digest":format!("sha256:{}","b".repeat(64))});
        assert!(graph_receipt(
            &receipt,
            &source,
            "city_a",
            "cities/city_a/plots/site/monthly_avg_daily_energy.png"
        ));
        receipt["source_batch_id"] = json!("another-batch");
        let input = json!({"receipt":source,"graphs":{"graphs":[{"section":"site","graph_key":"monthly_avg_daily_energy_image","city_id":"city_a","url":receipt}]}});
        let plan: Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap()).unwrap();
        assert!(plan["targets"][0]["receipt"].is_null());
        assert_eq!(
            plan["targets"][0]["unavailable"]["kind"],
            "monthly_average_daily_energy"
        );
    }
    #[test]
    fn corrupt_png_bytes_fail_even_with_correct_size() {
        let bytes = b"\x89PNG\r\n\x1a\nfixture";
        let receipt = json!({"schema":"ds-solar.graph-artifact-receipt/v1","media_type":"image/png","byte_count":bytes.len(),"content_digest":ds_solar_contracts::digest::sha256_hex(bytes)});
        let context = serde_json::to_vec(&receipt).unwrap();
        verify_bytes(bytes, &context).unwrap();
        let mut changed = bytes.to_vec();
        changed[8] ^= 1;
        assert!(verify_bytes(&changed, &context).is_err());
    }
}
