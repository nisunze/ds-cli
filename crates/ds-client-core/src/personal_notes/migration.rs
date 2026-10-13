//! Explicit representation migration pins; Brain alone commits stored data.
use super::{Action, command_id_valid, invalid, note_id_valid, sha_valid, unreadable};
use crate::ClientError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPlan {
    pub schema: String,
    pub id: String,
    pub expected_version: i64,
    pub source_sha256: String,
    pub source_update_time: String,
    pub target_sha256: String,
    pub baseline_version: i64,
    pub earlier_history: String,
    pub changes: Vec<String>,
}
impl MigrationPlan {
    pub fn validate(&self) -> Result<(), ClientError> {
        use time::{OffsetDateTime, format_description::well_known::Rfc3339};
        let at =
            OffsetDateTime::parse(&self.source_update_time, &Rfc3339).map_err(|_| invalid())?;
        if self.schema != "ds.personal-note-canonicalize/v1"
            || !note_id_valid(&self.id)
            || self.id.trim() != self.id
            || !(1..i64::MAX).contains(&self.expected_version)
            || self.baseline_version < 1
            || self.baseline_version > self.expected_version
            || !sha_valid(&self.source_sha256)
            || !sha_valid(&self.target_sha256)
            || self.source_update_time.len() > 40
            || !self.source_update_time.ends_with('Z')
            || at.unix_timestamp() <= 0
            || !matches!(self.earlier_history.as_str(), "none" | "not_assessed")
            || (self.earlier_history == "none" && self.baseline_version != 1)
            || self.changes.len() > 5
        {
            return Err(invalid());
        }
        let mut seen = std::collections::BTreeSet::new();
        for c in &self.changes {
            if !matches!(
                c.as_str(),
                "owner_uid_binding"
                    | "stored_id"
                    | "geometry_scalar"
                    | "history_baseline"
                    | "retain_observed_current"
            ) || !seen.insert(c)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Applied {
    plan: MigrationPlan,
    command_id: String,
    applied: bool,
    #[serde(rename = "replayed")]
    _replayed: bool,
}
pub(super) fn verify(action: &Action, data: &Value) -> Result<(), ClientError> {
    match action {
        Action::PrepareMigration { id } => {
            let plan: MigrationPlan =
                serde_json::from_value(data.clone()).map_err(|_| unreadable())?;
            plan.validate().map_err(|_| unreadable())?;
            if plan.id != *id {
                return Err(unreadable());
            }
        }
        Action::ApplyMigration { plan, command_id } => {
            let result: Applied = serde_json::from_value(data.clone()).map_err(|_| unreadable())?;
            result.plan.validate().map_err(|_| unreadable())?;
            if result.plan != *plan
                || result.command_id != *command_id
                || !result.applied
                || !command_id_valid(&result.command_id)
            {
                return Err(unreadable());
            }
        }
        _ => return Err(unreadable()),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn plan() -> MigrationPlan {
        serde_json::from_value(json!({"schema":"ds.personal-note-canonicalize/v1","id":"selected","expected_version":7,"source_sha256":"a".repeat(64),"source_update_time":"2026-10-13T10:15:00.123456Z","target_sha256":"b".repeat(64),"baseline_version":7,"earlier_history":"not_assessed","changes":["stored_id","geometry_scalar","history_baseline","retain_observed_current"]})).unwrap()
    }
    #[test]
    fn exact_migration_plan_is_the_only_admitted_payload() {
        let p = plan();
        p.validate().unwrap();
        let action = Action::ApplyMigration {
            plan: p.clone(),
            command_id: "reviewed-migration".into(),
        };
        action.validate().unwrap();
        assert_eq!(
            action.payload(),
            json!({"action":"canonicalize_apply","migration_plan":p,"command_id":"reviewed-migration"})
        );
        let mut wrong = p.clone();
        wrong.changes.push("geometry_scalar".into());
        assert!(wrong.validate().is_err());
        let mut unknown = serde_json::to_value(&p).unwrap();
        unknown["owner_uid"] = json!("forged");
        assert!(serde_json::from_value::<MigrationPlan>(unknown).is_err());
        let mut wrong = p;
        wrong.source_update_time = "2026-10-13T12:15:00+02:00".into();
        assert!(wrong.validate().is_err());
    }
    #[test]
    fn migration_receipt_proves_every_reviewed_source_pin() {
        let p = plan();
        let action = Action::ApplyMigration {
            plan: p.clone(),
            command_id: "reviewed-migration".into(),
        };
        let receipt =
            json!({"plan":p,"command_id":"reviewed-migration","applied":true,"replayed":false});
        verify(&action, &receipt).unwrap();
        for key in [
            "source_sha256",
            "target_sha256",
            "source_update_time",
            "baseline_version",
            "expected_version",
        ] {
            let mut wrong = receipt.clone();
            wrong["plan"][key] = json!("different");
            assert!(verify(&action, &wrong).is_err(), "{key}");
        }
        let mut wrong = receipt.clone();
        wrong["applied"] = json!(false);
        assert!(verify(&action, &wrong).is_err());
        let mut wrong = receipt;
        wrong["owner_uid"] = json!("hidden");
        assert!(verify(&action, &wrong).is_err());
    }
}
