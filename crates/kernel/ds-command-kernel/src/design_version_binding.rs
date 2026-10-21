//! Closed native request and acknowledgement for immutable issued revision bindings.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetPin {
    pub asset_id: String,
    pub asset_version: u64,
    pub asset_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_generation: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub version_id: String,
    pub revision_id: String,
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_record_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone_id: Option<String>,
    #[serde(default)]
    pub assets: Vec<AssetPin>,
    pub expected_catalog_revision: String,
    pub expected_source_revision: String,
    pub reason: String,
    pub idempotency_key: String,
}
fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.trim() == s
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
fn sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn instant(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}
impl Binding {
    pub fn validate(&self) -> Result<(), String> {
        if !super::design_version_catalog::version_id(&self.version_id)
            || !super::design_version_catalog::revision_id(&self.revision_id)
            || !sha(&self.expected_catalog_revision)
            || instant(&self.expected_source_revision).is_none()
            || self.reason.trim().is_empty()
            || self.reason.chars().count() > 280
            || self.reason.chars().any(char::is_control)
            || self.idempotency_key.is_empty()
            || self.idempotency_key.chars().count() > 128
            || self.idempotency_key.chars().any(char::is_control)
        {
            return Err("exact immutable references, catalog/source fences and bounded mutation reason/key required".into());
        }
        if self.record_id.as_deref().is_some_and(|s| !id(s))
            || self.milestone_id.as_deref().is_some_and(|s| !id(s))
            || self.record_id.is_some() != self.expected_record_revision.is_some()
            || self
                .expected_record_revision
                .as_deref()
                .is_some_and(|s| instant(s).is_none())
        {
            return Err("each exact PM record requires its observed immutable source fence".into());
        }
        match self.purpose.as_str() {
            "submission" if self.record_id.is_some() && !self.assets.is_empty() => {},
            "milestone" if self.milestone_id.is_some() => {},
            _ => return Err("submission needs an issued record and delivered asset pins; milestone needs a real milestone identity".into()),
        }
        if !self.assets.is_empty() && self.record_id.is_none() {
            return Err("delivered asset pins require their associated exact PM record".into());
        }
        let mut seen = BTreeSet::new();
        if self.assets.len() > 32
            || self.assets.iter().any(|a| {
                !id(&a.asset_id)
                    || !seen.insert(&a.asset_id)
                    || a.asset_version == 0
                    || a.asset_version > i64::MAX as u64
                    || !sha(&a.asset_sha256)
                    || a.asset_generation
                        .is_some_and(|n| n == 0 || n > i64::MAX as u64)
            })
        {
            return Err("delivered assets require unique exact version/digest pins and positive optional generations".into());
        }
        Ok(())
    }
    pub fn verify_response(
        &self,
        project: &str,
        transformer: &str,
        response: &Value,
    ) -> Result<(), String> {
        let b = &response["binding"];
        if b["project_id"] != project
            || b["transformer_id"] != transformer
            || b["version_id"] != self.version_id
            || b["revision_id"] != self.revision_id
            || b["purpose"] != self.purpose
            || !b["binding_id"].as_str().is_some_and(id)
            || !b["snapshot_digest"].as_str().is_some_and(sha)
            || !b["created_by"].as_str().is_some_and(id)
            || !b["created_at"]
                .as_str()
                .is_some_and(|s| instant(s).is_some())
            || b["display_ordinal_at_issue"].as_u64().is_none()
        {
            return Err(
                "immutable binding acknowledgement identity or capture evidence differs".into(),
            );
        }
        for (key, want) in [
            ("record_id", &self.record_id),
            ("milestone_id", &self.milestone_id),
        ] {
            if let Some(want) = want {
                if b[key] != *want {
                    return Err("immutable binding changed a typed PM target".into());
                }
            } else if b.get(key).is_some_and(|v| !v.is_null() && v != "") {
                return Err("immutable binding invented a PM target".into());
            }
        }
        if let Some(expected) = self.expected_record_revision.as_deref() {
            if b["record_revision"].as_str().and_then(instant) != instant(expected)
                || !b["record_sha256"].as_str().is_some_and(sha)
            {
                return Err("immutable binding record fence or digest differs".into());
            }
        }
        let assets = b["assets"].as_array().ok_or("binding asset pins absent")?;
        if assets.len() != self.assets.len() {
            return Err("binding asset set differs".into());
        }
        let mut seen = BTreeSet::new();
        for a in assets {
            let aid = a["asset_id"]
                .as_str()
                .ok_or("binding asset identity absent")?;
            if !seen.insert(aid) {
                return Err("binding duplicates asset evidence".into());
            }
            let expected = self
                .assets
                .iter()
                .find(|pin| pin.asset_id == aid)
                .ok_or("binding added an unrequested asset")?;
            let generation = a["asset_generation"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= i64::MAX as u64)
                .ok_or("observed delivered generation absent")?;
            if a["asset_version"] != expected.asset_version
                || a["asset_sha256"] != expected.asset_sha256
                || expected.asset_generation.is_some_and(|n| n != generation)
            {
                return Err("binding changed delivered asset pins".into());
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn input() -> Binding {
        Binding {
            version_id: "v0".into(),
            revision_id: "v0".into(),
            purpose: "submission".into(),
            record_id: Some("R-1".into()),
            expected_record_revision: Some("2026-10-13T00:00:00Z".into()),
            milestone_id: None,
            assets: vec![AssetPin {
                asset_id: "A-1".into(),
                asset_version: 3,
                asset_sha256: "a".repeat(64),
                asset_generation: None,
            }],
            expected_catalog_revision: "b".repeat(64),
            expected_source_revision: "2026-10-13T00:00:00Z".into(),
            reason: "Issue reviewed deliverable".into(),
            idempotency_key: "issue-1".into(),
        }
    }
    #[test]
    fn issued_binding_requires_record_fence_and_assets_without_approval_inference() {
        let mut b = input();
        b.validate().unwrap();
        b.assets.clear();
        assert!(b.validate().is_err());
        b = input();
        b.expected_record_revision = None;
        assert!(b.validate().is_err());
        b = input();
        b.purpose = "approved".into();
        assert!(b.validate().is_err());
        b = input();
        b.purpose = "milestone".into();
        b.milestone_id = Some("M-1".into());
        b.assets.clear();
        b.validate().unwrap();
        let mut unknown = json!(b);
        unknown["approved"] = json!(true);
        assert!(serde_json::from_value::<Binding>(unknown).is_err());
    }
    #[test]
    fn immutable_receipt_refuses_changed_record_revision_and_delivered_asset_generation() {
        let b = input();
        let mut ack = json!({"binding":{"binding_id":"binding-1","project_id":"p","transformer_id":"t","version_id":"v0","revision_id":"v0","purpose":"submission","record_id":"R-1","record_revision":"2026-10-13T00:00:00Z","record_sha256":"c".repeat(64),"snapshot_digest":"d".repeat(64),"created_by":"owner","created_at":"2026-10-13T00:01:00Z","display_ordinal_at_issue":0,"assets":[{"asset_id":"A-1","asset_version":3,"asset_sha256":"a".repeat(64),"asset_generation":123}]}});
        b.verify_response("p", "t", &ack).unwrap();
        ack["binding"]["record_revision"] = json!("2026-10-13T00:00:01Z");
        assert!(b.verify_response("p", "t", &ack).is_err());
        ack["binding"]["record_revision"] = json!("2026-10-13T00:00:00Z");
        ack["binding"]["assets"][0]["asset_generation"] = json!(0);
        assert!(b.verify_response("p", "t", &ack).is_err());
    }
}
