//! Source-pinned exported-design history input. Runtime identity restoration is excluded.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
pub const LAYERS: [&str; 6] = [
    "customers",
    "lv_lines",
    "lv_poles",
    "service_cables",
    "spans",
    "tr",
];
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub record_id: String,
    pub asset_id: String,
    pub asset_version: u64,
    pub asset_sha256: String,
    pub archive_member: String,
    pub member_sha256: String,
    pub historical_period: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Import {
    pub transformer_id: String,
    pub version_number: u64,
    pub expected_source_revision: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    pub idempotency_key: String,
    pub source: Source,
    pub layers: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    pub schema: String,
    pub project_id: String,
    pub history_import: Import,
}
fn sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.trim() == s
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
impl Import {
    pub fn validate(&self, project: &str) -> Result<(), String> {
        if !id(project) || !id(&self.transformer_id) || self.version_number != 0 {
            return Err(
                "historical import names one exact project/transformer and explicit v0".into(),
            );
        }
        if time::OffsetDateTime::parse(
            &self.expected_source_revision,
            &time::format_description::well_known::Rfc3339,
        )
        .is_err()
            || self.reason.trim().is_empty()
            || self.reason.chars().count() > 280
            || self.reason.chars().any(char::is_control)
            || self.idempotency_key.is_empty()
            || self.idempotency_key.chars().count() > 128
            || self.idempotency_key.chars().any(char::is_control)
            || self
                .milestone
                .as_deref()
                .is_some_and(|m| m.is_empty() || m.len() > 120 || m.chars().any(char::is_control))
        {
            return Err(
                "exact current source fence, bounded reason/milestone/replay key required".into(),
            );
        }
        let s = &self.source;
        let parts = s.archive_member.split('/').collect::<Vec<_>>();
        if !id(&s.record_id)
            || !id(&s.asset_id)
            || s.asset_version == 0
            || s.asset_version > i64::MAX as u64
            || !sha(&s.asset_sha256)
            || !sha(&s.member_sha256)
            || s.archive_member.is_empty()
            || s.archive_member.len() > 512
            || s.archive_member.contains('\\')
            || parts
                .iter()
                .any(|p| p.is_empty() || *p == "." || *p == "..")
            || s.archive_member.chars().any(char::is_control)
            || s.historical_period.is_empty()
            || s.historical_period.len() > 80
            || s.historical_period.chars().any(char::is_control)
        {
            return Err(
                "exact record/asset/member pins and bounded relative archival source required"
                    .into(),
            );
        }
        if self.layers.len() != LAYERS.len() || LAYERS.iter().any(|k| !self.layers.contains_key(*k))
        {
            return Err("exactly six exported engineering layers required".into());
        }
        let bytes = serde_json::to_vec(&self.layers).map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("exported layers exceed16MiB".into());
        }
        for (name, fc) in &self.layers {
            let features = fc["features"]
                .as_array()
                .ok_or("exported features absent")?;
            if fc["type"] != "FeatureCollection"
                || features.len() > 20000
                || (name == "tr" && features.len() != 1)
            {
                return Err("invalid exported FeatureCollection count".into());
            }
            let mut ids = BTreeSet::new();
            for f in features {
                let fid = f["properties"]["fid"]
                    .as_i64()
                    .filter(|n| *n >= 0)
                    .ok_or("original integer fid absent")?;
                if f["type"] != "Feature"
                    || !ids.insert(fid)
                    || !f["geometry"]["type"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                    || !f["geometry"]["coordinates"].is_array()
                {
                    return Err("duplicate original fid or invalid original geometry".into());
                }
            }
        }
        Ok(())
    }
    pub fn payload(&self, project: &str) -> Result<Value, String> {
        self.validate(project)?;
        Ok(
            json!({"action":"import_transformer_version","eds_project_id":project,"history_import":self}),
        )
    }
    pub fn verify_response(&self, project: &str, response: &Value) -> Result<(), String> {
        let v = &response["version"];
        let source = &v["source"];
        if v["project_id"] != project
            || v["object"] != json!({"kind":"lv_transformer","id":self.transformer_id})
            || v["version_id"] != "v0"
            || v["version"] != 0
            || v["source_mode"] != "source_pinned_import"
            || v["snapshot_scope"] != "exported_design"
        {
            return Err(
                "import acknowledgement differs from exact immutable exported baseline".into(),
            );
        }
        for (key, want) in [
            ("record_id", &self.source.record_id),
            ("asset_id", &self.source.asset_id),
            ("asset_sha256", &self.source.asset_sha256),
            ("archive_member", &self.source.archive_member),
            ("member_sha256", &self.source.member_sha256),
            ("historical_period", &self.source.historical_period),
        ] {
            if source[key] != *want {
                return Err("imported archival source pin differs".into());
            }
        }
        if source["asset_version"] != self.source.asset_version
            || !source["imported_layers_sha256"].as_str().is_some_and(sha)
        {
            return Err("imported source version or layer digest absent".into());
        }
        Ok(())
    }
}
impl Prepared {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "ds.lv-history-import/v1" {
            return Err("unknown history import schema".into());
        }
        self.history_import.validate(&self.project_id)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Import {
        Import{transformer_id:"TR-1".into(),version_number:0,expected_source_revision:"2026-10-12T12:00:00Z".into(),reason:"Approved April source".into(),milestone:None,idempotency_key:"april".into(),source:Source{record_id:"r1".into(),asset_id:"a1".into(),asset_version:3,asset_sha256:"a".repeat(64),archive_member:"GEOPACKAGE/TR-1.gpkg".into(),member_sha256:"b".repeat(64),historical_period:"April2026".into()},layers:LAYERS.into_iter().map(|name|{let features=if name=="tr"{vec![json!({"type":"Feature","properties":{"fid":7,"name":"original","survey_entry_id":"retain"},"geometry":{"type":"Point","coordinates":[30.0,-2.0,1234.5]}})]}else{vec![]};(name.into(),json!({"type":"FeatureCollection","features":features}))}).collect()}
    }
    #[test]
    fn exported_ids_and_dimensions_are_preserved_without_runtime_claims() {
        let i = input();
        let p = i.payload("p").unwrap();
        assert_eq!(p["history_import"]["layers"]["tr"], i.layers["tr"]);
        assert!(p["history_import"]["source"]
            .get("source_update_time")
            .is_none());
        assert!(p.get("gdfs").is_none());
    }
    #[test]
    fn duplicate_fid_bad_source_and_nonzero_version_refuse() {
        let mut i = input();
        let duplicate = i.layers["tr"]["features"][0].clone();
        i.layers.get_mut("tr").unwrap()["features"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(i.validate("p").is_err());
        let mut i = input();
        i.version_number = 1;
        assert!(i.validate("p").is_err());
        let mut i = input();
        i.source.archive_member = "../source.gpkg".into();
        assert!(i.validate("p").is_err());
    }
    #[test]
    fn caller_cannot_assert_server_observed_source_fields() {
        let mut v = json!(input());
        v["source"]["source_update_time"] = json!("invented");
        assert!(serde_json::from_value::<Import>(v).is_err());
    }
}
