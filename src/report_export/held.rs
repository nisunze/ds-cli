//! Admitted project inputs retained for local rendering. Retention never grants
//! publication authority or substitutes a current service answer.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub project: String,
    pub lane: String,
    pub principal: String,
    pub credential_audience_sha256: String,
    pub sheets_sha256: String,
}
impl Scope {
    pub fn validate(&self, expected: &Self) -> Result<(), String> {
        if self != expected
            || self.project.trim().is_empty()
            || self.principal.trim().is_empty()
            || self.project.len() > 500
            || self.principal.len() > 500
            || !matches!(self.lane.as_str(), "stable" | "canary")
            || !super::exact_lower_sha256(&self.credential_audience_sha256)
            || !super::exact_lower_sha256(&self.sheets_sha256)
        {
            return Err("held report inputs belong to another authenticated source".into());
        }
        Ok(())
    }
}

/// Exact projection, or a service-confirmed missing definition. An unheld
/// projection is represented by the host's absence of this entire capture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformerNatures {
    pub scope: Scope,
    pub transformers: Vec<String>,
    pub document: Option<String>,
    pub document_sha256: Option<String>,
}
impl TransformerNatures {
    pub fn new(
        scope: Scope,
        transformers: &[String],
        document: Option<&str>,
    ) -> Result<Self, String> {
        let capture = Self {
            scope,
            transformers: transformers.to_vec(),
            document: document.map(str::to_owned),
            document_sha256: document.map(|s| super::sha256_hex(s.as_bytes())),
        };
        capture.values(&capture.scope, transformers)?;
        Ok(capture)
    }
    pub fn values(
        &self,
        expected: &Scope,
        requested: &[String],
    ) -> Result<BTreeMap<String, String>, String> {
        self.scope.validate(expected)?;
        let held: BTreeSet<_> = self.transformers.iter().collect();
        if held.len() != self.transformers.len()
            || held.is_empty()
            || held.len() > 100_000
            || self
                .transformers
                .iter()
                .any(|s| super::reportable_transformer(s).is_err())
            || requested.iter().any(|s| !held.contains(s))
        {
            return Err("transformer nature capture does not cover this scope".into());
        }
        let values = match (&self.document, &self.document_sha256) {
            (Some(document), Some(sha))
                if document.len() <= 32 * 1024 * 1024
                    && super::sha256_hex(document.as_bytes()) == *sha =>
            {
                transformer_natures_from_projection(
                    document,
                    &self.scope.project,
                    &self.transformers,
                )?
            }
            (None, None) => BTreeMap::new(),
            _ => return Err("held transformer nature projection differs from its pin".into()),
        };
        Ok(values
            .into_iter()
            .filter(|(name, _)| requested.contains(name))
            .collect())
    }
}

pub fn transformer_natures_from_projection(
    document: &str,
    project: &str,
    transformers: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let document: Value = serde_json::from_str(document).map_err(|e| e.to_string())?;
    if document["schema_version"] != "ds-report.design-tags/v3"
        || document["project_id"] != project
        || document["selected_definition_ids"] != serde_json::json!(["transformer_nature"])
    {
        return Err("the projection is for another schema, project, or definition".into());
    }
    let group = document["groups"]
        .as_array()
        .and_then(|groups| {
            groups
                .iter()
                .find(|g| g["definition_id"] == "transformer_nature")
        })
        .ok_or("the projection does not describe transformer_nature")?;
    if group["state"] != "active" || group["cardinality"] != "single" {
        return Err("transformer_nature is not an active single-value definition".into());
    }
    let allowed: BTreeSet<&str> = group["values"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["value"].as_str())
        .collect();
    if allowed.is_empty() {
        return Err("transformer_nature has no allowed values".into());
    }
    let requested: BTreeSet<&str> = transformers.iter().map(String::as_str).collect();
    let mut natures = BTreeMap::new();
    for assignment in document["assignments"].as_array().into_iter().flatten() {
        if assignment["definition_id"] != "transformer_nature" {
            continue;
        }
        let name = assignment["object"]["id"]
            .as_str()
            .ok_or("an assignment has no transformer id")?;
        if assignment["object"]["kind"] != "lv_transformer" || !requested.contains(name) {
            return Err("an assignment is outside this transformer scope".into());
        }
        let values = assignment["values"]
            .as_array()
            .ok_or("an assignment has no value list")?;
        if values.len() != 1 {
            return Err("transformer_nature must have exactly one value".into());
        }
        let nature = values[0]
            .as_str()
            .ok_or("an assignment value is not text")?;
        if !allowed.contains(nature) || natures.insert(name.to_owned(), nature.to_owned()).is_some()
        {
            return Err("an assignment has an unknown value or is duplicated".into());
        }
    }
    Ok(natures)
}

/// Exact saved analysis already verified by the native client, fenced to the
/// saved room. Its bytes remain separate from the room's content digest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedAnalysis {
    pub scope: Scope,
    pub transformer: String,
    pub version: i64,
    pub content_digest: String,
    pub document: String,
    pub sha256: String,
}
impl SavedAnalysis {
    pub fn validate(
        &self,
        expected: &Scope,
        transformer: &str,
        version: i64,
        content_digest: &str,
    ) -> Result<(), String> {
        self.scope.validate(expected)?;
        if self.transformer != transformer
            || self.version != version
            || self.content_digest != content_digest
            || self.version <= 0
            || super::reportable_transformer(transformer).is_err()
            || !super::exact_lower_sha256(content_digest)
            || self.document.is_empty()
            || self.document.len() > 32 * 1024 * 1024
            || super::sha256_hex(self.document.as_bytes()) != self.sha256
            || serde_json::from_str::<Value>(&self.document).is_err()
        {
            return Err("held analysis differs from its exact saved room or byte pin".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn scope() -> Scope {
        Scope {
            project: "project_a".into(),
            lane: "stable".into(),
            principal: "owner".into(),
            credential_audience_sha256: "a".repeat(64),
            sheets_sha256: "b".repeat(64),
        }
    }
    fn projection() -> String {
        json!({"schema_version":"ds-report.design-tags/v3","project_id":"project_a","selected_definition_ids":["transformer_nature"],"groups":[{"definition_id":"transformer_nature","state":"active","cardinality":"single","values":[{"value":"upgrade"}]}],"assignments":[{"definition_id":"transformer_nature","object":{"kind":"lv_transformer","id":"tx_a"},"values":["upgrade"]}]}).to_string()
    }
    #[test]
    fn nature_capture_retains_exact_projection_and_admits_only_covered_subset() {
        let held = TransformerNatures::new(
            scope(),
            &["tx_a".into(), "tx_b".into()],
            Some(&projection()),
        )
        .unwrap();
        assert_eq!(
            held.values(&scope(), &["tx_a".into()]).unwrap(),
            BTreeMap::from([("tx_a".into(), "upgrade".into())])
        );
        assert!(held.values(&scope(), &["tx_b".into()]).unwrap().is_empty());
        assert!(held.values(&scope(), &["tx_c".into()]).is_err());
        let absent = TransformerNatures::new(scope(), &["tx_a".into()], None).unwrap();
        assert!(absent
            .values(&scope(), &["tx_a".into()])
            .unwrap()
            .is_empty());
    }
    #[test]
    fn nature_capture_refuses_crossed_identity_source_and_altered_bytes() {
        let held = TransformerNatures::new(scope(), &["tx_a".into()], Some(&projection())).unwrap();
        for field in ["project", "lane", "principal", "audience", "sheets"] {
            let mut other = scope();
            match field {
                "project" => other.project = "other".into(),
                "lane" => other.lane = "canary".into(),
                "principal" => other.principal = "other".into(),
                "audience" => other.credential_audience_sha256 = "c".repeat(64),
                _ => other.sheets_sha256 = "c".repeat(64),
            };
            assert!(held.values(&other, &["tx_a".into()]).is_err());
        }
        let mut altered = held;
        altered.document.as_mut().unwrap().push(' ');
        assert!(altered.values(&scope(), &["tx_a".into()]).is_err());
    }
    #[test]
    fn saved_analysis_refuses_moved_room_and_altered_bytes() {
        let mut held = SavedAnalysis {
            scope: scope(),
            transformer: "tx_a".into(),
            version: 2,
            content_digest: "c".repeat(64),
            document: "{\"actual\":1}".into(),
            sha256: super::super::sha256_hex(b"{\"actual\":1}"),
        };
        held.validate(&scope(), "tx_a", 2, &"c".repeat(64)).unwrap();
        assert!(held.validate(&scope(), "tx_a", 3, &"c".repeat(64)).is_err());
        assert!(held.validate(&scope(), "tx_a", 2, &"d".repeat(64)).is_err());
        assert!(held.validate(&scope(), "tx_b", 2, &"c".repeat(64)).is_err());
        held.document.push(' ');
        assert!(held.validate(&scope(), "tx_a", 2, &"c".repeat(64)).is_err());
    }
}
