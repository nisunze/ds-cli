//! Immutable project-revision intentions shared by CLI, Server and presentation hosts.
//! The gateway authorizes and commits; this owner pins the captured source and
//! expected head before any upload. No host may replace a stale expected head.
//!
//! A project model has VERSIONS (v1, v2, …) and every save is a REVISION of the
//! current version. Saving appends a revision to the head's version; only an
//! explicit [`Intent::bump_version`] starts the next one. The governance fields
//! below (milestone, stage, detail, approval, operation summary, composition and
//! migration lineage) are the ones the project catalog stores on the revision;
//! they are validated here with the Server's own bounds so a refusal names the
//! field before any byte is uploaded.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const MAX_MODEL_BYTES: usize = 256 * 1024 * 1024;
/// The Server's operation-summary bound: entries and characters per entry.
pub const MAX_OPERATION_SUMMARY: usize = 100;
pub const MAX_OPERATION_SUMMARY_ENTRY: usize = 300;
/// The Server's composition bound: 2..=100 ordered inputs.
pub const MAX_COMPOSITION_SOURCES: usize = 100;
/// The Server's milestone bound, in bytes.
pub const MAX_MILESTONE: usize = 300;
/// The approval states the catalog stores on a revision.
pub const APPROVAL_STATES: [&str; 4] = ["draft", "submitted", "approved", "rejected"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub project: String,
    pub model_id: String,
    pub model_kind: String,
    pub expected_head_revision_id: String,
    pub source_revision_id: String,
    pub manifest_model_id: String,
    pub manifest_model_revision: u64,
    pub model_schema_version: String,
    pub engine_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Start the next version with this revision. Without it the revision
    /// joins the head's current version. Never sent when false, so a Server
    /// that predates explicit bumps sees the exact request it always saw.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bump_version: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_stage_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_level_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<Approval>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operation_summary: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composition_sources: Vec<CompositionSource>,
    /// Typed provenance of converted source content. The catalog admits it on
    /// a model's first revision, except the closed `current_package_format`
    /// operation bound to the exact existing head and current native identity.
    /// Other later conversions use `operation_summary` instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_source: Option<MigrationSource>,
}

/// The review state recorded on one revision. `approved` and `rejected` are
/// decisions: they need a level and a reason, and the Server additionally
/// requires the approving capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
}

/// One ordered input of a composed revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionSource {
    pub sequence: u64,
    pub source_kind: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<String>,
    pub model_digest: String,
}

/// Where converted content came from: kind, human reference, exact digest of
/// the source bytes and the manifest identity the conversion produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationSource {
    pub kind: String,
    pub reference: String,
    pub source_digest: String,
    pub manifest_id: String,
}

/// The Server's catalog identifier grammar: `^[a-z0-9][a-z0-9_-]{1,127}$`.
/// Stage, detail, approval level, composition and migration identifiers must
/// satisfy it exactly; a looser local check would only move the refusal later.
pub fn catalog_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    (2..=128).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

/// Lowercase 64-hex SHA-256, the only digest spelling the catalog stores.
pub fn sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The one-line lineage statement a later revision carries when typed
/// `migration_source` is not admissible: first revisions and the closed
/// `current_package_format` successor use typed provenance. Other later
/// conversions use this bounded summary, including cross-project migration.
pub fn migration_summary(source: &MigrationSource) -> String {
    let mut line = format!(
        "migration_source kind={} source_digest={} manifest_id={} reference=",
        source.kind, source.source_digest, source.manifest_id
    );
    for c in source.reference.chars() {
        if line.len() + c.len_utf8() > MAX_OPERATION_SUMMARY_ENTRY {
            break;
        }
        line.push(if c.is_control() { ' ' } else { c });
    }
    line.trim_end().to_owned()
}

fn text(value: &Option<String>, bound: usize) -> bool {
    value.as_ref().is_none_or(|s| {
        !s.is_empty() && s.len() <= bound && s.trim() == s && !s.chars().any(char::is_control)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub digest: String,
    pub object: String,
    pub byte_length: u64,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub intent: Intent,
    pub revision_id: String,
    pub artifact: Artifact,
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        && value != "."
        && value != ".."
}

impl Intent {
    pub fn plan(&self, project: &str, bytes: &[u8]) -> Result<Plan, &'static str> {
        if self.project != project || !identifier(project) || !identifier(&self.model_id) {
            return Err("grid_publication_project_or_model_invalid");
        }
        if !matches!(
            self.model_kind.as_str(),
            "general" | "lv_network" | "mv_line"
        ) || (!self.expected_head_revision_id.is_empty()
            && !identifier(&self.expected_head_revision_id))
            || self.source_revision_id.is_empty()
            || self.source_revision_id.len() > 160
            || self.source_revision_id.chars().any(char::is_control)
            || self.manifest_model_id.is_empty()
            || self.manifest_model_id.len() > 256
            || self.manifest_model_id.trim() != self.manifest_model_id
            || self.manifest_model_id.chars().any(char::is_control)
            || self.manifest_model_revision > i64::MAX as u64
            || bytes.is_empty()
            || bytes.len() > MAX_MODEL_BYTES
        {
            return Err("grid_publication_source_invalid");
        }
        for (value, bound) in [
            (&self.model_schema_version, 160),
            (&self.engine_version, 160),
        ] {
            if value.is_empty()
                || value.len() > bound
                || value.trim() != value
                || value.chars().any(char::is_control)
            {
                return Err("grid_publication_metadata_invalid");
            }
        }
        for (value, bound) in [(&self.display_name, 240), (&self.reason, 2000)] {
            if !text(value, bound) {
                return Err("grid_publication_metadata_invalid");
            }
        }
        self.governance()?;
        let revision_id: String = self
            .source_revision_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        if !identifier(&revision_id) {
            return Err("grid_publication_revision_invalid");
        }
        let digest = format!("{:x}", Sha256::digest(bytes));
        Ok(Plan {
            intent: self.clone(),
            revision_id,
            artifact: Artifact {
                object: format!("ds-grid/objects/sha256/{}/{}", &digest[..2], digest),
                digest,
                byte_length: bytes.len() as u64,
            },
        })
    }

    /// The governance fields, checked against the catalog's own bounds.
    fn governance(&self) -> Result<(), &'static str> {
        if self.bump_version && self.expected_head_revision_id.is_empty() {
            return Err("grid_publication_bump_without_head");
        }
        if !text(&self.milestone, MAX_MILESTONE) || !text(&self.description, 2000) {
            return Err("grid_publication_metadata_invalid");
        }
        if [&self.design_stage_id, &self.detail_level_id]
            .iter()
            .any(|value| value.as_deref().is_some_and(|v| !catalog_identifier(v)))
        {
            return Err("grid_publication_governance_invalid");
        }
        if let Some(approval) = &self.approval {
            let decision = matches!(approval.status.as_str(), "approved" | "rejected");
            if !APPROVAL_STATES.contains(&approval.status.as_str())
                || approval
                    .level_id
                    .as_deref()
                    .is_some_and(|v| !catalog_identifier(v))
                || !text(&approval.decision_reason, 2000)
                || (decision && (approval.level_id.is_none() || approval.decision_reason.is_none()))
            {
                return Err("grid_publication_approval_invalid");
            }
        }
        if self.operation_summary.len() > MAX_OPERATION_SUMMARY
            || self.operation_summary.iter().any(|entry| {
                entry.is_empty()
                    || entry.len() > MAX_OPERATION_SUMMARY_ENTRY
                    || entry.trim() != entry
                    || entry.chars().any(char::is_control)
            })
        {
            return Err("grid_publication_operation_summary_invalid");
        }
        if !self.composition_sources.is_empty() {
            let mut seen = std::collections::BTreeSet::new();
            if !(2..=MAX_COMPOSITION_SOURCES).contains(&self.composition_sources.len())
                || self
                    .composition_sources
                    .iter()
                    .enumerate()
                    .any(|(index, source)| {
                        source.sequence != index as u64
                            || !matches!(
                                source.source_kind.as_str(),
                                "temporary_model" | "project_model"
                            )
                            || !catalog_identifier(&source.model_id)
                            || (source.source_kind == "project_model"
                                && source.revision_id.is_none())
                            || source
                                .revision_id
                                .as_deref()
                                .is_some_and(|v| !catalog_identifier(v))
                            || !sha256_hex(&source.model_digest)
                            || !seen.insert((
                                &source.source_kind,
                                &source.model_id,
                                &source.revision_id,
                                &source.model_digest,
                            ))
                    })
            {
                return Err("grid_publication_composition_invalid");
            }
        }
        if let Some(source) = &self.migration_source {
            let format_successor = source.kind == "current_package_format";
            let source_identity_valid = if format_successor {
                !self.expected_head_revision_id.is_empty()
                    && source.reference == self.expected_head_revision_id
                    && source.manifest_id == self.manifest_model_id
                    && self.manifest_model_revision > 0
                    && self.model_schema_version == "4"
                    && !self.bump_version
            } else {
                self.expected_head_revision_id.is_empty() && catalog_identifier(&source.manifest_id)
            };
            if !source_identity_valid
                || !catalog_identifier(&source.kind)
                || source.reference.trim().is_empty()
                || source.reference.len() > 2000
                || source.reference.chars().any(char::is_control)
                || !sha256_hex(&source.source_digest)
            {
                return Err("grid_publication_migration_invalid");
            }
        }
        Ok(())
    }
}

impl Plan {
    pub fn upload_request(&self) -> Value {
        json!({"action":"start_upload", "project_id":self.intent.project,
            "upload":{"model_id":self.intent.model_id,"digest":self.artifact.digest,"byte_length":self.artifact.byte_length}})
    }
    pub fn version_request(&self) -> Value {
        let mut version = json!({
            "model_id":self.intent.model_id,"model_kind":self.intent.model_kind,
            "revision_id":self.revision_id,"expected_head_revision_id":self.intent.expected_head_revision_id,
            "manifest_model_id":self.intent.manifest_model_id,"manifest_model_revision":self.intent.manifest_model_revision,
            "model":self.artifact,"model_schema_version":self.intent.model_schema_version,
            "engine_version":self.intent.engine_version,"validation":{"state":"valid","issue_count":0}
        });
        for (key, value) in [
            ("display_name", &self.intent.display_name),
            ("reason", &self.intent.reason),
            ("milestone", &self.intent.milestone),
            ("description", &self.intent.description),
            ("design_stage_id", &self.intent.design_stage_id),
            ("detail_level_id", &self.intent.detail_level_id),
        ] {
            if let Some(value) = value {
                version[key] = json!(value);
            }
        }
        // Every governance field is omitted when unset, so the request a
        // caller who sets none of them sends is byte-identical to before.
        if self.intent.bump_version {
            version["bump_version"] = json!(true);
        }
        if let Some(approval) = &self.intent.approval {
            version["approval"] = json!(approval);
        }
        if !self.intent.operation_summary.is_empty() {
            version["operation_summary"] = json!(self.intent.operation_summary);
        }
        if !self.intent.composition_sources.is_empty() {
            version["composition_sources"] = json!(self.intent.composition_sources);
        }
        if let Some(source) = &self.intent.migration_source {
            version["migration_source"] = json!(source);
        }
        json!({"action":"create_version","project_id":self.intent.project,"version":version})
    }
    pub fn matches_version(&self, value: &Value) -> bool {
        value["project_id"] == self.intent.project
            && value["model_id"] == self.intent.model_id
            && value["revision_id"] == self.revision_id
            && value["manifest_model_id"] == self.intent.manifest_model_id
            && value["manifest_model_revision"] == self.intent.manifest_model_revision
            && value["model_kind"] == self.intent.model_kind
            && value["model_schema_version"] == self.intent.model_schema_version
            && value["engine_version"] == self.intent.engine_version
            && value["parent_revision_id"].as_str().unwrap_or("")
                == self.intent.expected_head_revision_id
            && serde_json::from_value::<Artifact>(value["model"].clone())
                .ok()
                .as_ref()
                == Some(&self.artifact)
            && value["version"].as_u64().is_some_and(|v| v > 0)
            && self.governance_stored(value)
    }
    /// The governance the catalog echoes must be what was sent. The Server
    /// trims text and defaults an omitted approval to draft; this owner already
    /// refuses untrimmed text, so equality is exact.
    fn governance_stored(&self, value: &Value) -> bool {
        let echoed = |key: &str, sent: &Option<String>| {
            sent.as_ref()
                .is_none_or(|sent| value[key].as_str() == Some(sent.as_str()))
        };
        echoed("milestone", &self.intent.milestone)
            && echoed("design_stage_id", &self.intent.design_stage_id)
            && echoed("detail_level_id", &self.intent.detail_level_id)
            && self
                .intent
                .approval
                .as_ref()
                .is_none_or(|approval| value["approval"]["status"] == approval.status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn intent(project: &str) -> Intent {
        Intent {
            project: project.into(),
            model_id: "model_one".into(),
            model_kind: "mv_line".into(),
            expected_head_revision_id: "rev-old".into(),
            source_revision_id: "rev:new".into(),
            manifest_model_id: "native-model".into(),
            manifest_model_revision: 1,
            model_schema_version: "19".into(),
            engine_version: "ds-grid@1".into(),
            display_name: None,
            reason: Some("Reviewed route".into()),
            ..Default::default()
        }
    }
    #[test]
    fn current_format_successor_uses_exact_head_and_native_identity() {
        let mut current = intent("project_a");
        current.manifest_model_id = "pls-import-fnv1a64:81e10012".into();
        current.model_schema_version = "4".into();
        current.migration_source = Some(MigrationSource {
            kind: "current_package_format".into(),
            reference: current.expected_head_revision_id.clone(),
            source_digest: "b".repeat(64),
            manifest_id: current.manifest_model_id.clone(),
        });
        let plan = current.clone().plan("project_a", b"native model").unwrap();
        assert_eq!(
            plan.version_request()["version"]["migration_source"]["manifest_id"],
            "pls-import-fnv1a64:81e10012"
        );
        let mut controls = Vec::new();
        let mut bad = current.clone();
        bad.expected_head_revision_id.clear();
        controls.push(bad);
        let mut bad = current.clone();
        bad.migration_source.as_mut().unwrap().reference = "other-head".into();
        controls.push(bad);
        let mut bad = current.clone();
        bad.migration_source.as_mut().unwrap().manifest_id = "other-native".into();
        controls.push(bad);
        let mut bad = current.clone();
        bad.manifest_model_revision = 0;
        controls.push(bad);
        let mut bad = current.clone();
        bad.model_schema_version = "3".into();
        controls.push(bad);
        let mut bad = current.clone();
        bad.bump_version = true;
        controls.push(bad);
        let mut bad = current.clone();
        bad.migration_source.as_mut().unwrap().source_digest = "invalid".into();
        controls.push(bad);
        let mut bad = current;
        bad.migration_source.as_mut().unwrap().kind = "pls_cadd_bak".into();
        controls.push(bad);
        for refused in controls {
            assert_eq!(
                refused.plan("project_a", b"native model").unwrap_err(),
                "grid_publication_migration_invalid"
            );
        }
    }
    #[test]
    fn interleaved_projects_keep_their_bytes_and_expected_heads() {
        let a = intent("project_a").plan("project_a", b"first").unwrap();
        let mut second = intent("project_b");
        second.expected_head_revision_id = "rev-other".into();
        let b = second.plan("project_b", b"second").unwrap();
        assert_ne!(a.artifact.digest, b.artifact.digest);
        assert_eq!(
            a.version_request()["version"]["expected_head_revision_id"],
            "rev-old"
        );
        assert_eq!(b.version_request()["project_id"], "project_b");
        assert!(intent("project_a").plan("project_b", b"first").is_err());
        assert_eq!(a.revision_id, "rev-new");
    }
    #[test]
    fn readback_must_match_project_revision_kind_and_exact_bytes() {
        let plan = intent("p").plan("p", b"model").unwrap();
        let mut v = plan.version_request()["version"].clone();
        v["project_id"] = json!("p");
        v["version"] = json!(1);
        v["parent_revision_id"] = json!("rev-old");
        assert!(plan.matches_version(&v));
        for (key, wrong) in [
            ("project_id", json!("q")),
            ("revision_id", json!("rev-other")),
            ("model_kind", json!("general")),
            ("version", json!(0)),
            ("model", json!({})),
            ("parent_revision_id", json!("unreviewed")),
            ("engine_version", json!("unknown")),
        ] {
            let mut changed = v.clone();
            changed[key] = wrong;
            assert!(!plan.matches_version(&changed));
        }
    }

    #[test]
    fn a_save_joins_the_current_version_unless_a_bump_is_explicit() {
        let plan = intent("p").plan("p", b"model").unwrap();
        let request = plan.version_request();
        assert!(request["version"].get("bump_version").is_none());
        for absent in [
            "milestone",
            "approval",
            "operation_summary",
            "composition_sources",
            "migration_source",
            "design_stage_id",
        ] {
            assert!(request["version"].get(absent).is_none(), "{absent}");
        }
        let mut bumped = intent("p");
        bumped.bump_version = true;
        let request = bumped.plan("p", b"model").unwrap().version_request();
        assert_eq!(request["version"]["bump_version"], true);
        // The first revision of a new model is v1 by definition.
        let mut first = intent("p");
        first.expected_head_revision_id = String::new();
        first.bump_version = true;
        assert_eq!(
            first.plan("p", b"model").unwrap_err(),
            "grid_publication_bump_without_head"
        );
    }
    #[test]
    fn governance_fields_are_sent_and_must_be_echoed_exactly() {
        let mut submitted = intent("p");
        submitted.milestone = Some("Submission 2 to REG".into());
        submitted.design_stage_id = Some("detailed_design".into());
        submitted.approval = Some(Approval {
            status: "submitted".into(),
            level_id: None,
            decision_reason: None,
        });
        submitted.operation_summary = vec!["respotted 14 structures".into()];
        let plan = submitted.plan("p", b"model").unwrap();
        let request = plan.version_request();
        assert_eq!(request["version"]["milestone"], "Submission 2 to REG");
        assert_eq!(
            request["version"]["approval"],
            json!({"status":"submitted"})
        );
        assert_eq!(request["version"]["design_stage_id"], "detailed_design");
        let mut stored = request["version"].clone();
        stored["project_id"] = json!("p");
        stored["version"] = json!(3);
        stored["parent_revision_id"] = json!("rev-old");
        assert!(plan.matches_version(&stored));
        let mut dropped = stored.clone();
        dropped["milestone"] = json!("");
        assert!(!plan.matches_version(&dropped));
        let mut draft = stored;
        draft["approval"] = json!({"status":"draft"});
        assert!(!plan.matches_version(&draft));
    }
    #[test]
    fn governance_bounds_are_the_catalogs_own() {
        let refused = |edit: &dyn Fn(&mut Intent)| {
            let mut candidate = intent("p");
            edit(&mut candidate);
            candidate.plan("p", b"model").unwrap_err()
        };
        assert_eq!(
            refused(&|i| i.design_stage_id = Some("Detailed Design".into())),
            "grid_publication_governance_invalid"
        );
        assert_eq!(
            refused(&|i| i.milestone = Some("m".repeat(MAX_MILESTONE + 1))),
            "grid_publication_metadata_invalid"
        );
        // A decision needs a level and a reason; a status is mandatory.
        assert_eq!(
            refused(&|i| {
                i.approval = Some(Approval {
                    status: "approved".into(),
                    level_id: None,
                    decision_reason: Some("ok".into()),
                })
            }),
            "grid_publication_approval_invalid"
        );
        assert_eq!(
            refused(&|i| {
                i.approval = Some(Approval {
                    status: "locked".into(),
                    level_id: None,
                    decision_reason: None,
                })
            }),
            "grid_publication_approval_invalid"
        );
        assert_eq!(
            refused(&|i| i.operation_summary = vec![" padded".into()]),
            "grid_publication_operation_summary_invalid"
        );
        let source = |sequence: u64, model: &str| CompositionSource {
            sequence,
            source_kind: "project_model".into(),
            model_id: model.into(),
            revision_id: Some("rev-a".into()),
            model_digest: "a".repeat(64),
        };
        assert_eq!(
            refused(&|i| i.composition_sources = vec![source(0, "model-a")]),
            "grid_publication_composition_invalid"
        );
        assert_eq!(
            refused(&|i| i.composition_sources = vec![source(0, "model-a"), source(2, "model-b")]),
            "grid_publication_composition_invalid"
        );
        assert_eq!(
            refused(&|i| i.composition_sources = vec![source(0, "model-a"), source(1, "model-a")]),
            "grid_publication_composition_invalid"
        );
        let mut composed = intent("p");
        composed.composition_sources = vec![source(0, "model-a"), source(1, "model-b")];
        assert!(composed.plan("p", b"model").is_ok());
        // Typed migration lineage is admissible on a first revision only.
        let migration = MigrationSource {
            kind: "pls_cadd_bak".into(),
            reference: "Kamonyi.bak".into(),
            source_digest: "b".repeat(64),
            manifest_id: "native-model".into(),
        };
        let with_head = migration.clone();
        assert_eq!(
            refused(&move |i| i.migration_source = Some(with_head.clone())),
            "grid_publication_migration_invalid"
        );
        let mut first = intent("p");
        first.expected_head_revision_id = String::new();
        first.migration_source = Some(migration.clone());
        let request = first.plan("p", b"model").unwrap().version_request();
        assert_eq!(
            request["version"]["migration_source"]["kind"],
            "pls_cadd_bak"
        );
        let line = migration_summary(&migration);
        assert!(line.starts_with("migration_source kind=pls_cadd_bak source_digest=bbbb"));
        assert!(line.ends_with("reference=Kamonyi.bak"));
        let long = MigrationSource {
            reference: "x".repeat(1000),
            ..migration
        };
        assert_eq!(migration_summary(&long).len(), MAX_OPERATION_SUMMARY_ENTRY);
    }
    #[test]
    fn catalog_identifiers_match_the_server_grammar() {
        for good in ["ab", "rev-1", "detailed_design", &"a".repeat(128)] {
            assert!(catalog_identifier(good), "{good}");
        }
        for bad in ["a", "-ab", "Ab", "a.b", "a b", &"a".repeat(129), ""] {
            assert!(!catalog_identifier(bad), "{bad}");
        }
    }
}
