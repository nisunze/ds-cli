//! Verify the closed native reference handoff before it reaches preparation.
use base64::Engine;
use ds_solar_contracts::{BundleBytes, SolarError, SolarErrorCode, SolarResult, digest};
use serde_json::Value;

pub fn bundle_from_delivery(project: &str, payload: &Value) -> SolarResult<BundleBytes> {
    let invalid = |message| SolarError::new(SolarErrorCode::ReferenceUnitInvalid, message);
    if payload["project"] != project {
        return Err(invalid("reference project mismatch"));
    }
    let rows = payload["artifacts"]
        .as_array()
        .filter(|r| r.len() == 4)
        .ok_or_else(|| invalid("reference bundle requires four artifacts"))?;
    let mut bundle = BundleBytes::default();
    let mut seen = std::collections::BTreeSet::new();
    for row in rows {
        let name = row["name"].as_str().unwrap_or("");
        if !seen.insert(name) {
            return Err(invalid("duplicate reference artifact"));
        }
        let encoded = row["base64"]
            .as_str()
            .filter(|s| s.len() <= 24 * 1024 * 1024)
            .ok_or_else(|| invalid("reference bytes missing or excessive"))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid("reference base64 invalid"))?;
        if bytes.is_empty()
            || bytes.len() > 16 * 1024 * 1024
            || row["bytes"].as_u64() != Some(bytes.len() as u64)
            || row["sha256"] != digest::hex_sha256(&bytes)
        {
            return Err(invalid("reference artifact digest/size mismatch"));
        }
        match name {
            "weather.json" => bundle.weather_json = bytes,
            "reference-unit.json" => bundle.reference_unit_json = bytes,
            "reference-unit.parquet" => bundle.reference_unit_parquet = bytes,
            "manifest.json" => bundle.manifest_json = bytes,
            _ => return Err(invalid("unknown reference artifact")),
        }
    }
    Ok(bundle)
}

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;
