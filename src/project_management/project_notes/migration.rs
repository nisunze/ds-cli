//! Explicit representation adoption: no automatic semantic interpretation.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub project_id: String,
    pub id: String,
    pub expected_version: u64,
    pub source_sha256: String,
    pub source_update_time: String,
    pub target_sha256: String,
    pub memory: Memory,
    pub retain_observed_current: bool,
}
impl Plan {
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.schema == "ds.project-note-canonicalize/v1"
                && id(&self.project_id)
                && id(&self.id)
                && self.expected_version > 0
                && self.expected_version < i64::MAX as u64
                && sha(&self.source_sha256)
                && sha(&self.target_sha256)
                && crate::survey_entries_changes::canonical_utc_rfc3339_nano(
                    &self.source_update_time,
                ),
            "Exact reviewed migration source pins required",
        )?;
        self.memory.provenance.validate()?;
        self.memory.scope.validate()?;
        require(
            self.memory.schema == MEMORY_SCHEMA
                && self.memory.provenance.agent_label.is_none()
                && self.memory.provenance.session_ref.is_none()
                && self.memory.provenance.source_author.is_none()
                && self.memory.note_type == NoteType::Observation
                && self.memory.provenance.kind == Basis::Observation
                && self.memory.claims.is_empty()
                && self.memory.evidence.is_empty()
                && self.memory.scope.kind == ScopeKind::Conditions
                && self.memory.scope.conditions == ["unclassified historical observation"]
                && self.memory.scope.asset_ids.is_empty()
                && self.memory.scope.object_refs.is_empty()
                && self.memory.scope.structure_ids.is_empty()
                && self.memory.scope.workstream.is_none()
                && self.memory.scope.user_ref.is_none()
                && [Lifecycle::Active, Lifecycle::Archived].contains(&self.memory.lifecycle)
                && self.memory.parent.is_none()
                && self.memory.supersedes.is_empty()
                && self.memory.unresolved.is_empty(),
            "Migration preserves literal source, never inferred project rulings",
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepare {
    pub id: String,
}
impl Prepare {
    pub fn validate(&self) -> Result<(), String> {
        require(id(&self.id), "Exact note identity required")
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apply {
    pub operation_id: String,
    pub plan: Plan,
}
impl Apply {
    pub fn validate(&self) -> Result<(), String> {
        require(
            id(&self.operation_id),
            "Stable migration operation required",
        )?;
        self.plan.validate()
    }
}
pub(super) fn verify_plan(project: &str, v: &serde_json::Value) -> Result<Plan, String> {
    let p: Plan =
        serde_json::from_value(v.clone()).map_err(|_| "Closed migration receipt required")?;
    p.validate()?;
    require(p.project_id == project, "Migration project mismatch")?;
    Ok(p)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn plan() -> Plan {
        serde_json::from_value(json!({"schema":"ds.project-note-canonicalize/v1","project_id":"p","id":"n","expected_version":7,"source_sha256":"a".repeat(64),"source_update_time":"2026-10-13T10:15:00Z","target_sha256":"b".repeat(64),"retain_observed_current":true,"memory":{"schema":MEMORY_SCHEMA,"note_type":"observation","lifecycle":"active","provenance":{"kind":"observation"},"scope":{"kind":"conditions","conditions":["unclassified historical observation"]},"claims":[],"evidence":[]}})).unwrap()
    }
    #[test]
    fn reviewed_migration_cannot_infer_decision_or_global_scope() {
        let mut p = plan();
        p.validate().unwrap();
        p.memory.note_type = NoteType::Decision;
        assert!(p.validate().is_err());
        p = plan();
        p.memory.scope.kind = ScopeKind::Project;
        assert!(p.validate().is_err());
        for stamp in [
            "not-a-time",
            "2026-10-13T10:15:00+00:00",
            "2026-02-31T10:15:00Z",
            "2026-10-13T10:15:00.1200Z",
        ] {
            let mut invalid = plan();
            invalid.source_update_time = stamp.into();
            assert!(invalid.validate().is_err(), "accepted {stamp}");
        }
        let mut raw = serde_json::to_value(plan()).unwrap();
        raw["actor_uid"] = json!("spoof");
        assert!(serde_json::from_value::<Plan>(raw).is_err());
    }
    #[test]
    fn migration_receipt_keeps_exact_project_source_and_authored_version() {
        let p = plan();
        let c = Command::ApplyMigration(Apply {
            operation_id: "adopt".into(),
            plan: p.clone(),
        });
        assert_eq!(
            c.payload("p").unwrap()["action"],
            "project_note_canonicalize_apply"
        );
        assert!(c.payload("other").is_err());
        let mut receipt = json!({"project_id":"p","operation_id":"adopt","plan":p,"applied":true,"replayed":false,"authored_version_changed":false});
        c.verify_response("p", &receipt).unwrap();
        receipt["plan"]["source_sha256"] = json!("c".repeat(64));
        assert!(c.verify_response("p", &receipt).is_err());
    }
    #[test]
    fn prepared_source_cannot_belong_to_another_note_or_version() {
        let c = Command::PrepareMigration(Prepare { id: "n".into() });
        let mut response = json!({"project_id":"p","plan":plan(),"authored_version_changed":false,"document":{"id":"n","version":7,"data":{"body":"literal"}}});
        c.verify_response("p", &response).unwrap();
        response["document"]["id"] = json!("other");
        assert!(c.verify_response("p", &response).is_err());
    }
    #[test]
    fn plain_history_is_literal_only_for_exact_version_without_claim() {
        let literal = json!({"project_id":"p","context_revision":"a".repeat(64),"note":{"project_id":"p","id":"n","version":7,"document":{"body":"unaltered"},"memory":null,"semantic_status":"unrecorded"}});
        let c = Command::Read(Read {
            id: "n".into(),
            version: Some(7),
            claim_id: None,
        });
        c.verify_response("p", &literal).unwrap();
        assert!(
            Command::Read(Read {
                id: "n".into(),
                version: None,
                claim_id: None
            })
            .verify_response("p", &literal)
            .is_err()
        );
        assert!(
            Command::Read(Read {
                id: "n".into(),
                version: Some(7),
                claim_id: Some("claim".into())
            })
            .verify_response("p", &literal)
            .is_err()
        );
    }
}
