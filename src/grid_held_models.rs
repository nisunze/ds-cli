//! Admission of an intentional Desktop inventory of existing worker sessions.
//! The host captures identities; each worker supplies its atomic Rust facts.
//! This owner performs no acquisition, focus change, calculation or checkpoint.
use crate::grid_model_management::saved::{History, Workspace};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const MAX_HELD: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub account: Option<String>,
    pub project: Option<String>,
    pub generation: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Native {
    pub manifest_model_id: String,
    pub revision: String,
    pub model_revision: u64,
    pub coordinate_system: String,
    pub history: History,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub model_id: String,
    pub name: String,
    pub selected: bool,
    pub read_only: bool,
    pub persisted: bool,
    pub host_revision: Option<String>,
    pub workspace_owner: Workspace,
    /// Exact held provenance is forwarded; inventory grants it no authority.
    pub project_binding: Option<Value>,
    pub saving: bool,
    pub pending_operations: u64,
    pub physical_worker_active: bool,
    pub native: Native,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub captured_context: Context,
    pub context: Context,
    pub total: usize,
    pub rows: Vec<Row>,
    pub mutated: bool,
}

fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn history(value: &History) -> bool {
    id(&value.model_revision)
        && id(&value.initial_revision)
        && id(&value.history_pin)
        && value
            .undo_depth
            .checked_add(value.redo_depth)
            .is_some_and(|n| n <= 16_384)
}

pub fn admit(value: Value) -> Result<Inventory, &'static str> {
    let mut inventory: Inventory = serde_json::from_value(value)
        .map_err(|_| "held_model_inventory_invalid: incomplete or undeclared inventory fields")?;
    if inventory.context != inventory.captured_context {
        return Err("held_model_context_stale: the Desktop context changed during inventory");
    }
    if inventory.mutated
        || inventory.total != inventory.rows.len()
        || inventory.total > MAX_HELD
        || inventory.context.account.as_deref().is_some_and(|v| !id(v))
        || inventory.context.project.as_deref().is_some_and(|v| !id(v))
    {
        return Err("held_model_inventory_invalid: invalid context, count or mutation fact");
    }
    let mut seen = BTreeSet::new();
    let mut selected = 0;
    for row in &mut inventory.rows {
        if !id(&row.model_id)
            || !id(&row.name)
            || !seen.insert(row.model_id.clone())
            || !id(&row.native.manifest_model_id)
            || !id(&row.native.coordinate_system)
            || !history(&row.native.history)
            || row.native.revision != row.native.history.model_revision
            || row.host_revision.as_deref().is_some_and(|v| !id(v))
            || row
                .project_binding
                .as_ref()
                .is_some_and(|v| !v.is_object() || v.to_string().len() > 4096)
        {
            return Err(
                "held_model_inventory_invalid: duplicate identity or invalid native worker facts",
            );
        }
        selected += usize::from(row.selected);
        // A lagging host projection must never label a newer native head saved.
        row.persisted &= row.host_revision.as_deref() == Some(row.native.revision.as_str());
    }
    if selected > 1 {
        return Err("held_model_inventory_invalid: more than one Profile occupant");
    }
    Ok(inventory)
}

/// The checkpoint is a new artifact, not Save or durable Undo/Redo recovery.
pub fn admit_checkpoint(
    model: &str,
    expected: &History,
    path: &str,
    value: &Value,
) -> Result<(), &'static str> {
    let checkpoint = &value["checkpoint"];
    let actual: History = serde_json::from_value(checkpoint["history"].clone())
        .map_err(|_| "held_model_checkpoint_invalid: missing native history witness")?;
    let digest = checkpoint["artifact"]["sha256"]
        .as_str()
        .unwrap_or_default();
    if !history(expected)
        || &actual != expected
        || value["model"] != model
        || value["revision"] != expected.model_revision
        || checkpoint["model"] != model
        || checkpoint["revision"] != expected.model_revision
        || checkpoint["persisted"] != true
        || value["held_checkpoint"] != true
        || value["live_checkpoint"] != true
        || value["already_open"] != true
        || value["focus_changed"] != false
        || value["source_saved"] != false
        || value["undo_redo_preserved"] != false
        || checkpoint["artifact"]["path"] != path
        || !checkpoint["artifact"]["byte_len"]
            .as_u64()
            .is_some_and(|n| n > 0)
        || !digest
            .strip_prefix("sha256:")
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("held_model_checkpoint_invalid: identity, history, artifact or nonmutation facts differ");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> Value {
        let context = json!({"account":"owner","project":"project","generation":7});
        json!({"captured_context":context,"context":context,"total":2,"mutated":false,"rows":[
            {"model_id":"local-a","name":"Original","selected":false,"read_only":false,"persisted":false,"host_revision":"rev:dirty","workspace_owner":"temporary","project_binding":null,"saving":false,"pending_operations":0,"physical_worker_active":false,"native":{"manifest_model_id":"native-a","revision":"rev:dirty","model_revision":3,"coordinate_system":"EPSG:32736","history":{"model_revision":"rev:dirty","initial_revision":"rev:base","undo_depth":4,"redo_depth":2,"history_pin":"native-pin"}}},
            {"model_id":"local-b","name":"Current","selected":true,"read_only":true,"persisted":true,"host_revision":"rev:saved","workspace_owner":"mv-design","project_binding":{"dsProject":"project","modelId":"head"},"saving":false,"pending_operations":0,"physical_worker_active":true,"native":{"manifest_model_id":"native-b","revision":"rev:saved","model_revision":1,"coordinate_system":"EPSG:32736","history":{"model_revision":"rev:saved","initial_revision":"rev:saved","undo_depth":0,"redo_depth":0,"history_pin":"saved-pin"}}}
        ]})
    }
    #[test]
    fn inventory_retains_all_native_histories_and_dirty_work_without_effects() {
        let got = admit(fixture()).unwrap();
        assert_eq!(got.total, 2);
        assert_eq!(got.rows[0].native.history.redo_depth, 2);
        assert!(!got.rows[0].persisted);
        assert!(got.rows[1].physical_worker_active);
        assert!(!got.mutated);
        let mut lag = fixture();
        lag["rows"][1]["host_revision"] = json!("rev:older");
        assert!(!admit(lag).unwrap().rows[1].persisted);
    }
    #[test]
    fn refuses_partial_duplicate_crossed_or_mutated_inventory() {
        for n in 0..7 {
            let mut f = fixture();
            match n {
                0 => f["context"]["generation"] = json!(8),
                1 => f["total"] = json!(3),
                2 => f["rows"][1]["model_id"] = json!("local-a"),
                3 => f["mutated"] = json!(true),
                4 => f["rows"][0]["native"]["history"]["model_revision"] = json!("rev:wrong"),
                5 => f["rows"][0]["selected"] = json!(true),
                _ => f["rows"][0]["native"]["extra"] = json!(true),
            }
            assert!(admit(f).is_err(), "boundary {n}");
        }
        let context = json!({"account":null,"project":null,"generation":0});
        assert!(admit(json!({"captured_context":context,"context":context,"total":0,"rows":[],"mutated":false})).is_ok());
    }
    #[test]
    fn checkpoint_admits_exact_new_artifact_and_truthful_history_witness_only() {
        let h = admit(fixture()).unwrap().rows.remove(0).native.history;
        let f = json!({"model":"local-a","revision":h.model_revision,"live_checkpoint":true,"held_checkpoint":true,"already_open":true,"focus_changed":false,"source_saved":false,"undo_redo_preserved":false,"checkpoint":{"model":"local-a","revision":h.model_revision,"persisted":true,"history":h,"artifact":{"path":"/new.dsgrid","byte_len":123,"sha256":format!("sha256:{}","a".repeat(64))}}});
        assert!(admit_checkpoint("local-a", &h, "/new.dsgrid", &f).is_ok());
        for field in ["focus_changed", "source_saved", "undo_redo_preserved"] {
            let mut wrong = f.clone();
            wrong[field] = json!(true);
            assert!(admit_checkpoint("local-a", &h, "/new.dsgrid", &wrong).is_err());
        }
        let mut aba = f.clone();
        aba["checkpoint"]["history"]["redo_depth"] = json!(0);
        assert!(admit_checkpoint("local-a", &h, "/new.dsgrid", &aba).is_err());
        assert!(admit_checkpoint("local-b", &h, "/new.dsgrid", &f).is_err());
        assert!(admit_checkpoint("local-a", &h, "/other.dsgrid", &f).is_err());
    }
}
