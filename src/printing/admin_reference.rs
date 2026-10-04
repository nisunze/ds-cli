//! Held administrative reference and the exact source transformer point for
//! the reserved print subtitle. Hosts verify the asset; the renderer rechecks it.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub country: String,
    pub path: PathBuf,
    pub sha256: String,
}
impl Asset {
    pub fn validate(&self) -> Result<(), String> {
        if !self.country.trim().eq_ignore_ascii_case("Rwanda")
            || !self.path.is_absolute()
            || self.path.as_os_str().len() > 4096
            || !crate::report_export::exact_lower_sha256(&self.sha256)
        {
            return Err("print_admin_subtitle_unavailable: exact Rwanda reference path and byte receipt are required".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub schema: String,
    pub asset: Asset,
    pub source_sha256: String,
}
impl Capture {
    pub fn new(asset: Asset, point: &Value) -> Result<Self, String> {
        asset.validate()?;
        Ok(Self {
            schema: "ds.print-admin-reference/v1".into(),
            asset,
            source_sha256: point_digest(point)?,
        })
    }
    pub fn validate(&self, point: &Value) -> Result<(), String> {
        self.asset.validate()?;
        if self.schema != "ds.print-admin-reference/v1"
            || self.source_sha256 != point_digest(point)?
        {
            return Err("print_admin_subtitle_unavailable: reference capture belongs to another source transformer point".into());
        }
        Ok(())
    }
}
pub fn coordinates(point: &Value) -> Result<[f64; 2], String> {
    if point["type"] != "Feature" || point["geometry"]["type"] != "Point" {
        return Err(
            "print_admin_subtitle_unavailable: one exact source transformer Point is required"
                .into(),
        );
    }
    let values=point["geometry"]["coordinates"].as_array().filter(|v|v.len()==2)
        .ok_or("print_admin_subtitle_unavailable: source Point requires exact WGS84 longitude/latitude")?;
    let lon = values[0]
        .as_f64()
        .filter(|v| v.is_finite() && (-180. ..=180.).contains(v))
        .ok_or("print_admin_subtitle_unavailable: invalid longitude")?;
    let lat = values[1]
        .as_f64()
        .filter(|v| v.is_finite() && (-90. ..=90.).contains(v))
        .ok_or("print_admin_subtitle_unavailable: invalid latitude")?;
    Ok([lon, lat])
}
fn point_digest(point: &Value) -> Result<String, String> {
    coordinates(point)?;
    // Derived administrative/cost fields may be stamped later. Geometry and
    // the source feature identity are unchanged and alone bind this lookup.
    let identity = json!({"id":point.get("id"),"geometry":point["geometry"]});
    Ok(crate::report_export::sha256_hex(
        &serde_json::to_vec(&identity).map_err(|e| e.to_string())?,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_binds_exact_point_and_never_stored_administrative_words() {
        let point = json!({"type":"Feature","id":"tr_1","geometry":{"type":"Point","coordinates":[29.8,-2.5]},"properties":{"district":"Forged"}});
        let capture = Capture::new(
            Asset {
                country: "Rwanda".into(),
                path: std::env::current_dir().unwrap().join("held.dsab"),
                sha256: "a".repeat(64),
            },
            &point,
        )
        .unwrap();
        capture.validate(&point).unwrap();
        let mut changed = point.clone();
        changed["properties"]["district"] = json!("Derived");
        capture.validate(&changed).unwrap();
        for field in ["id", "geometry"] {
            let mut changed = point.clone();
            changed[field] = Value::Null;
            assert!(capture.validate(&changed).is_err());
        }
        let mut extra = point.clone();
        extra["geometry"]["coordinates"] = json!([29.8, -2.5, 1700]);
        assert!(capture.validate(&extra).is_err());
        let mut wrong = capture.clone();
        wrong.schema = "unknown".into();
        assert!(wrong.validate(&point).is_err());
        wrong = capture.clone();
        wrong.asset.country = "Chad".into();
        assert!(wrong.validate(&point).is_err());
        wrong = capture.clone();
        wrong.asset.path = "unbound.dsab".into();
        assert!(wrong.validate(&point).is_err());
        wrong = capture;
        wrong.asset.sha256 = "A".repeat(64);
        assert!(wrong.validate(&point).is_err());
    }
}
