//! Closed wire validation for the LV version catalog; allocation stays server-owned.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mutation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_ordinal: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_before_version_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_record_id: Option<String>,
    pub expected_catalog_revision: String,
    pub expected_source_revision: String,
    pub reason: String,
    pub idempotency_key: String,
}
#[derive(Clone, Debug)]
pub enum Command {
    Compare {
        object_id: String,
        request: Box<super::design_revision_comparison::Request>,
    },
    Promote {
        object_id: String,
        promotion: super::design_version_promotion::Promotion,
    },
    Bind {
        object_id: String,
        binding: super::design_version_binding::Binding,
    },
    Catalog {
        object_id: String,
    },
    Revision {
        object_id: String,
        version_id: String,
        revision_id: String,
    },
    Snapshot {
        object_id: String,
        version_id: String,
        revision_id: String,
    },
    Publish {
        object_id: String,
        mutation: Mutation,
    },
    Renumber {
        object_id: String,
        mutation: Mutation,
    },
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.trim() == s
        && !s
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '\\'))
}
fn sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn legacy(s: &str) -> bool {
    s.strip_prefix('v').is_some_and(|n| {
        !n.is_empty()
            && (n == "0" || !n.starts_with('0'))
            && n.bytes().all(|c| c.is_ascii_digit())
            && n.parse::<u64>().is_ok_and(|n| n <= i64::MAX as u64)
    })
}
fn version(s: &str) -> bool {
    legacy(s) || s.strip_prefix("vc_").is_some_and(sha)
}
fn revision(s: &str) -> bool {
    legacy(s) || s.strip_prefix("vr_").is_some_and(sha)
}
/// Immutable catalog reference grammar, shared by native and WASM admission.
pub fn version_id(id: &str) -> bool {
    version(id)
}
/// Exact immutable revision grammar; a display ordinal is never a reference.
pub fn revision_id(id: &str) -> bool {
    revision(id)
}

/// Admit a legacy v0 only from the authority's immutable baseline evidence.
/// This validates provenance, not engineering approval or current demand.
pub fn baseline_evidence(row: &Value) -> bool {
    if row["object"]["kind"] != "lv_transformer" || row["version_id"] != "v0" || row["version"] != 0
    {
        return false;
    }
    let mode = row["source_mode"].as_str().unwrap_or_default();
    if mode == "source_pinned_import" {
        let source = &row["source"];
        return row["snapshot_scope"] == "exported_design"
            && source["asset_version"]
                .as_u64()
                .is_some_and(|n| n > 0 && n <= i64::MAX as u64)
            && ["record_id", "asset_id"]
                .iter()
                .all(|key| source[key].as_str().is_some_and(identifier))
            && source["archive_member"].as_str().is_some_and(|s| {
                !s.is_empty()
                    && s.len() <= 512
                    && !s.contains('\\')
                    && !s.chars().any(char::is_control)
                    && s.split('/')
                        .all(|part| !part.is_empty() && part != "." && part != "..")
            })
            && source["historical_period"].as_str().is_some_and(|s| {
                !s.trim().is_empty() && s.len() <= 80 && !s.chars().any(char::is_control)
            })
            && ["asset_sha256", "member_sha256", "imported_layers_sha256"]
                .iter()
                .all(|key| source[key].as_str().is_some_and(sha));
    }
    if !matches!(mode, "initial_creation" | "baseline_today")
        || row["snapshot_scope"] != "runtime_design"
        || row["baseline_source"]["kind"] != mode
        || !row["baseline_source"]["snapshot_digest"]
            .as_str()
            .is_some_and(sha)
        || !row["created_by"].as_str().is_some_and(identifier)
    {
        return false;
    }
    let instant = |v: &Value| {
        v.as_str().and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        })
    };
    let Some(created) = instant(&row["created_at"]) else {
        return false;
    };
    if created.unix_timestamp() <= 0
        || instant(&row["baseline_source"]["captured_at"]) != Some(created)
    {
        return false;
    }
    if mode == "initial_creation" {
        return row["source_revision"]
            == format!(
                "sha256:{}",
                row["baseline_source"]["snapshot_digest"]
                    .as_str()
                    .unwrap_or_default()
            )
            && (row["source_update_time"].is_null()
                || row["source_update_time"] == "0001-01-01T00:00:00Z");
    }
    instant(&row["source_update_time"]).is_some_and(|observed| {
        observed.unix_timestamp() > 0
            && row["source_revision"]
                .as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .is_some_and(|revision| {
                    i128::from(revision) == observed.unix_timestamp_nanos() / 1000
                })
    })
}

/// Verify exact immutable UTF-8 bytes and decode their native snapshot once.
/// The digest of the compressed stored frame remains separate server evidence.
pub fn decode_pinned_snapshot(value: &Value) -> Result<Value, String> {
    if value["metadata"]["playback_available"] != true {
        return Err("immutable revision authority does not permit playback".into());
    }
    use sha2::{Digest, Sha256};
    let raw = value["snapshot_json"]
        .as_str()
        .ok_or("immutable snapshot text absent")?;
    if raw.len() > 64 * 1024 * 1024
        || value["snapshot_sha256"] != format!("{:x}", Sha256::digest(raw.as_bytes()))
    {
        return Err("immutable snapshot exact UTF-8 digest differs or exceeds 64MiB".into());
    }
    let snapshot: Value =
        serde_json::from_str(raw).map_err(|_| "immutable snapshot JSON is invalid")?;
    if snapshot["schema"] != "ds.transformer-version-snapshot/v1"
        || !snapshot["layers"].as_object().is_some_and(|layers| {
            !layers.is_empty()
                && layers.values().all(|layer| {
                    layer["type"] == "FeatureCollection" && layer["features"].is_array()
                })
        })
    {
        return Err("immutable snapshot schema/layers are invalid".into());
    }
    Ok(snapshot)
}

impl Command {
    pub fn object_id(&self) -> &str {
        match self {
            Self::Catalog { object_id }
            | Self::Revision { object_id, .. }
            | Self::Snapshot { object_id, .. }
            | Self::Publish { object_id, .. }
            | Self::Renumber { object_id, .. }
            | Self::Bind { object_id, .. }
            | Self::Promote { object_id, .. }
            | Self::Compare { object_id, .. } => object_id,
        }
    }
    pub fn validate(&self, project: &str) -> Result<(), String> {
        if !identifier(project) || !identifier(self.object_id()) {
            return Err("catalog request needs exact project and transformer identities".into());
        }
        match self {
            Self::Catalog { .. } => Ok(()),
            Self::Bind { binding, .. } => binding.validate(),
            Self::Promote { promotion, .. } => promotion.validate(),
            Self::Compare { request, .. } => request.validate(),
            Self::Revision {
                version_id,
                revision_id,
                ..
            }
            | Self::Snapshot {
                version_id,
                revision_id,
                ..
            } => {
                if version(version_id) && revision(revision_id) {
                    Ok(())
                } else {
                    Err("use immutable catalog version and revision identities, not display ordinals".into())
                }
            }
            Self::Publish { mutation: m, .. } | Self::Renumber { mutation: m, .. } => {
                if m.task_id.as_deref().is_some_and(|s| !identifier(s))
                    || m.source_record_id
                        .as_deref()
                        .is_some_and(|s| !identifier(s))
                    || !sha(&m.expected_catalog_revision)
                    || time::OffsetDateTime::parse(
                        &m.expected_source_revision,
                        &time::format_description::well_known::Rfc3339,
                    )
                    .is_err()
                    || m.reason.trim().is_empty()
                    || m.reason.trim() != m.reason
                    || m.reason.chars().count() > 280
                    || m.reason.chars().any(char::is_control)
                    || m.idempotency_key.trim().is_empty()
                    || m.idempotency_key.trim() != m.idempotency_key
                    || m.idempotency_key.chars().count() > 128
                    || m.idempotency_key.chars().any(char::is_control)
                    || m.version_id.as_deref().is_some_and(|s| !version(s))
                    || m.display_ordinal.is_some_and(|n| n > 1_000_000)
                    || m.insert_before_version_id
                        .as_deref()
                        .is_some_and(|s| !version(s))
                {
                    return Err("catalog mutation needs exact catalog/source fences, bounded reason/key and immutable identities".into());
                }
                let valid = match self {
                    Self::Publish { .. } => {
                        (m.version_id.is_some() ^ m.display_ordinal.is_some())
                            && (m.insert_before_version_id.is_none() || m.display_ordinal.is_some())
                    }
                    Self::Renumber { .. } => {
                        m.version_id.is_some()
                            && m.display_ordinal.is_some()
                            && m.insert_before_version_id.is_none()
                    }
                    _ => unreachable!(),
                };
                if valid {
                    Ok(())
                } else {
                    Err("publish names an existing version OR a new display ordinal; insertion is new-only; renumber names version and ordinal without insertion".into())
                }
            }
        }
    }
    pub fn payload(&self, project: &str) -> Result<Value, String> {
        self.validate(project)?;
        if matches!(self, Self::Compare { .. }) {
            return Err(
                "comparison consumes exact reads through its owning workflow, not one raw action"
                    .into(),
            );
        }
        let action = match self {
            Self::Compare { .. } => unreachable!("comparison has a separate consuming workflow"),
            Self::Catalog { .. } => "get_version_catalog",
            Self::Bind { .. } => "bind_version_revision",
            Self::Promote { .. } => "promote_transformer_version_revision",
            Self::Revision { .. } => "get_version_revision",
            Self::Snapshot { .. } => "get_transformer_version_revision",
            Self::Publish { .. } => "publish_transformer_version_revision",
            Self::Renumber { .. } => "renumber_version",
        };
        let mut value = json!({"action":action,"project_id":project,"object":{"kind":"lv_transformer","id":self.object_id()}});
        match self {
            Self::Revision {
                version_id,
                revision_id,
                ..
            } => {
                value["version_id"] = json!(version_id);
                value["revision_id"] = json!(revision_id);
            }
            Self::Snapshot {
                version_id,
                revision_id,
                ..
            } => {
                value = json!({"action":action,"eds_project_id":project,
                    "catalog_revision_read":{"transformer_id":self.object_id(),"version_id":version_id,"revision_id":revision_id}});
            }
            Self::Promote { promotion, .. } => {
                let mut promotion = json!(promotion);
                promotion["transformer_id"] = json!(self.object_id());
                value =
                    json!({"action":action,"eds_project_id":project,"catalog_promotion":promotion});
            }
            Self::Publish { mutation, .. } => {
                let mut publication = json!(mutation);
                publication["transformer_id"] = json!(self.object_id());
                value = json!({"action":action,"eds_project_id":project,"catalog_publication":publication});
            }
            Self::Renumber { mutation, .. } => value["catalog_mutation"] = json!(mutation),
            Self::Bind { binding, .. } => value["catalog_binding"] = json!(binding),
            _ => {}
        }
        Ok(value)
    }
    pub fn verify_response(&self, project: &str, value: &Value) -> Result<(), String> {
        if matches!(self, Self::Compare { .. }) {
            return Err("comparison receipts are verified by the owning workflow".into());
        }
        if let Self::Promote { promotion, .. } = self {
            return promotion.verify_response(value);
        }
        if !matches!(
            self,
            Self::Revision { .. } | Self::Snapshot { .. } | Self::Promote { .. }
        ) {
            let c = &value["catalog"];
            if c["schema"] != "ds.lv-version-catalog/v1"
                || c["project_id"] != project
                || c["object"] != json!({"kind":"lv_transformer","id":self.object_id()})
                || !c["catalog_revision"].as_str().is_some_and(sha)
            {
                return Err("catalog response scope or revision is invalid".into());
            }
            let rows = c["versions"].as_array().ok_or("catalog versions absent")?;
            if rows.len() > 1000 {
                return Err("catalog exceeds 1000 versions".into());
            }
            let mut ids = std::collections::BTreeSet::new();
            let mut ordinals = std::collections::BTreeSet::new();
            for row in rows {
                let id = row["version_id"]
                    .as_str()
                    .filter(|s| version(s))
                    .ok_or("invalid catalog version identity")?;
                let ordinal = row["display_ordinal"]
                    .as_u64()
                    .filter(|n| *n <= 1_000_000)
                    .ok_or("invalid catalog display ordinal")?;
                if !ids.insert(id)
                    || !ordinals.insert(ordinal)
                    || !row["head_revision_id"].as_str().is_some_and(revision)
                    || !row["revision_count"].as_u64().is_some_and(|n| n > 0)
                {
                    return Err(
                        "catalog contains duplicate identity/ordinal or invalid revision".into(),
                    );
                }
            }
        }
        if matches!(
            self,
            Self::Revision { .. } | Self::Snapshot { .. } | Self::Publish { .. }
        ) {
            let m = &value["metadata"];
            if m["project_id"] != project
                || m["transformer_id"] != self.object_id()
                || !m["revision_number"].as_u64().is_some_and(|n| n > 0)
            {
                return Err(
                    "immutable revision metadata differs from requested project or transformer"
                        .into(),
                );
            }
        }
        if let Self::Revision {
            version_id,
            revision_id,
            ..
        }
        | Self::Snapshot {
            version_id,
            revision_id,
            ..
        } = self
        {
            let m = &value["metadata"];
            if m["version_id"] != *version_id
                || m["revision_id"] != *revision_id
                || !m["content_digest"].as_str().is_some_and(sha)
            {
                return Err("revision response differs from exact requested identity or content digest is absent".into());
            }
        }
        if matches!(self, Self::Snapshot { .. }) {
            decode_pinned_snapshot(value)?;
        }
        if matches!(self, Self::Publish { .. } | Self::Renumber { .. })
            && !value["changed"].is_boolean()
        {
            return Err("mutation change receipt absent".into());
        }
        if matches!(self, Self::Publish { .. }) {
            let m = &value["metadata"];
            if !m["version_id"].as_str().is_some_and(version)
                || !m["revision_id"].as_str().is_some_and(revision)
                || !m["content_digest"].as_str().is_some_and(sha)
            {
                return Err("published immutable revision metadata absent".into());
            }
        }
        if let Self::Bind { binding, .. } = self {
            binding.verify_response(project, self.object_id(), value)?;
            let entry = value["catalog"]["versions"]
                .as_array()
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row["version_id"] == binding.version_id)
                })
                .ok_or("bound catalog identity absent")?;
            if !entry["binding_count"].as_u64().is_some_and(|n| n > 0)
                || (binding.purpose == "submission"
                    && entry["issued_revision_id"] != binding.revision_id)
            {
                return Err("catalog did not retain immutable issuance evidence".into());
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn mutation() -> Mutation {
        Mutation {
            version_id: None,
            task_id: None,
            source_record_id: None,
            display_ordinal: Some(0),
            insert_before_version_id: Some("v1".into()),
            expected_catalog_revision: "a".repeat(64),
            expected_source_revision: "2026-10-12T12:00:00Z".into(),
            reason: "April baseline".into(),
            idempotency_key: "freeze-april".into(),
        }
    }
    #[test]
    fn placement_and_stable_reference_are_distinct_without_rewriting_legacy_refs() {
        let c = Command::Publish {
            object_id: "TR-1".into(),
            mutation: mutation(),
        };
        let p = c.payload("p").unwrap();
        assert_eq!(p["catalog_publication"]["display_ordinal"], 0);
        assert_eq!(p["catalog_publication"]["insert_before_version_id"], "v1");
        assert!(p["catalog_publication"].get("version_id").is_none());
        let c = Command::Revision {
            object_id: "TR-1".into(),
            version_id: "v1".into(),
            revision_id: "v1".into(),
        };
        assert_eq!(c.payload("p").unwrap()["version_id"], "v1");
    }
    #[test]
    fn malformed_mutations_refuse_before_transport() {
        let mut m = mutation();
        m.version_id = Some("v1".into());
        assert!(Command::Publish {
            object_id: "TR-1".into(),
            mutation: m.clone()
        }
        .payload("p")
        .is_err());
        m.insert_before_version_id = None;
        assert!(Command::Renumber {
            object_id: "TR-1".into(),
            mutation: m.clone()
        }
        .payload("p")
        .is_ok());
        m.expected_source_revision = "yesterday".into();
        assert!(Command::Renumber {
            object_id: "TR-1".into(),
            mutation: m
        }
        .payload("p")
        .is_err());
    }
    #[test]
    fn pinned_snapshot_verifies_exact_utf8_without_number_reserialization() {
        use sha2::{Digest, Sha256};
        let c = Command::Snapshot {
            object_id: "TR-1".into(),
            version_id: "v0".into(),
            revision_id: "v0".into(),
        };
        let raw = r#"{"schema":"ds.transformer-version-snapshot/v1","layers":{"tr":{"type":"FeatureCollection","features":[]}},"metadata":{"sentinel":1.0000,"name":"??"}}"#;
        let mut reply = json!({"metadata":{"project_id":"p","transformer_id":"TR-1","version_id":"v0","revision_id":"v0","revision_number":1,"playback_available":true,"content_digest":"a".repeat(64)},"snapshot_json":raw,"snapshot_sha256":format!("{:x}",Sha256::digest(raw.as_bytes()))});
        c.verify_response("p", &reply).unwrap();
        reply["metadata"]["playback_available"] = json!(false);
        assert!(c.verify_response("p", &reply).is_err());
        reply["metadata"]["playback_available"] = json!(true);
        let payload = c.payload("p").unwrap();
        assert_eq!(payload["action"], "get_transformer_version_revision");
        assert_eq!(payload["catalog_revision_read"]["version_id"], "v0");
        assert!(payload.get("object").is_none());
        reply["snapshot_json"] = json!(raw.replace("1.0000", "1"));
        assert!(c.verify_response("p", &reply).is_err());
        reply["snapshot_json"] = json!(raw);
        reply["metadata"]["revision_id"] = json!("v1");
        assert!(c.verify_response("p", &reply).is_err());
    }

    #[test]
    fn catalog_scope_and_duplicate_labels_are_checked() {
        let c = Command::Catalog {
            object_id: "TR-1".into(),
        };
        let row = json!({"version_id":"v1","display_ordinal":4,"head_revision_id":"v1","revision_count":1});
        let mut v = json!({"catalog":{"schema":"ds.lv-version-catalog/v1","project_id":"p","object":{"kind":"lv_transformer","id":"TR-1"},"catalog_revision":"a".repeat(64),"versions":[row.clone()]}});
        c.verify_response("p", &v).unwrap();
        v["catalog"]["versions"].as_array_mut().unwrap().push(row);
        assert!(c.verify_response("p", &v).is_err());
        assert!(c.verify_response("other", &v).is_err());
    }
    #[test]
    fn baseline_requires_immutable_evidence_for_each_actual_origin() {
        let mut row =
            json!({"object":{"kind":"lv_transformer","id":"TR-1"},"version_id":"v0","version":0});
        assert!(!baseline_evidence(&row));
        row["source_mode"] = json!("initial_creation");
        row["snapshot_scope"] = json!("runtime_design");
        row["created_by"] = json!("owner@example.com");
        row["created_at"] = json!("2026-10-12T12:00:00Z");
        row["baseline_source"] = json!({"kind":"initial_creation","snapshot_digest":"a".repeat(64),"captured_at":"2026-10-12T12:00:00Z"});
        row["source_revision"] = json!(format!("sha256:{}", "a".repeat(64)));
        assert!(baseline_evidence(&row));
        row["baseline_source"]["snapshot_digest"] = json!("bad");
        assert!(!baseline_evidence(&row));
        row["baseline_source"]["snapshot_digest"] = json!("a".repeat(64));
        row["source_mode"] = json!("baseline_today");
        row["baseline_source"]["kind"] = json!("baseline_today");
        row["source_update_time"] = json!("2026-10-12T11:00:00.123456Z");
        let observed = time::OffsetDateTime::parse(
            "2026-10-12T11:00:00.123456Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        row["source_revision"] = json!((observed.unix_timestamp_nanos() / 1000).to_string());
        assert!(baseline_evidence(&row));
        row["source_revision"] = json!(1);
        assert!(!baseline_evidence(&row));
        row["source_mode"] = json!("source_pinned_import");
        row["snapshot_scope"] = json!("exported_design");
        row["source"] = json!({"record_id":"R-1","asset_id":"A-1","asset_version":3,"asset_sha256":"a".repeat(64),"archive_member":"GEOPACKAGE/TR-1.gpkg","member_sha256":"b".repeat(64),"imported_layers_sha256":"c".repeat(64),"historical_period":"April 2026"});
        assert!(baseline_evidence(&row));
        row["source"]["member_sha256"] = json!("bad");
        assert!(!baseline_evidence(&row));
        assert!(!version_id("v00"));
        assert!(!revision_id("v01"));
    }
}
