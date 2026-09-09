//! One drain pass: ordering, stream fences and Solar dependencies live here.
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Deserialize)]
struct Row {
    id: String,
    owner_uid: String,
    project_id: String,
    engine: String,
    operation: String,
    variant: String,
    client_run_id: String,
    status: String,
    #[serde(default)]
    sync_archived_at: Option<u64>,
}
type Stream = (String, String, String, String);
fn stream(row: &Row) -> Stream {
    (
        row.project_id.clone(),
        row.engine.clone(),
        row.operation.clone(),
        row.variant.clone(),
    )
}
#[derive(Deserialize)]
struct Input {
    owner: String,
    rows: Vec<Row>,
}
#[cfg_attr(feature = "wasm", wasm_bindgen::prelude::wasm_bindgen)]
pub struct ComputeArtifactDrainPlan {
    rows: Vec<Row>,
    dependencies: BTreeMap<(String, String, String), usize>,
    blocked: BTreeSet<Stream>,
    cursor: usize,
    current: Option<usize>,
    deferred: bool,
    progressed: bool,
}
#[cfg_attr(feature = "wasm", wasm_bindgen::prelude::wasm_bindgen)]
impl ComputeArtifactDrainPlan {
    #[cfg_attr(feature = "wasm", wasm_bindgen::prelude::wasm_bindgen(constructor))]
    pub fn new(input: &[u8]) -> Result<Self, String> {
        if input.len() > 8 * 1024 * 1024 {
            return Err("Sync queue exceeds its byte bound.".into());
        }
        let input: Input = serde_json::from_slice(input).map_err(|_| "Invalid sync queue.")?;
        if input.owner.is_empty()
            || input.rows.len() > 4096
            || input.rows.iter().any(|v| v.owner_uid != input.owner)
        {
            return Err("Invalid account-owned sync queue.".into());
        }
        let mut identities = BTreeSet::new();
        let mut dependencies = BTreeMap::new();
        for (index, row) in input.rows.iter().enumerate() {
            if !identities.insert(&row.id) {
                return Err("Duplicate sync row identity.".into());
            }
            if row.engine == "solar" && row.operation.starts_with("calculate-") {
                let key = (
                    row.project_id.clone(),
                    row.client_run_id.clone(),
                    row.operation.clone(),
                );
                if dependencies.insert(key, index).is_some() {
                    return Err("Ambiguous calculation dependency.".into());
                }
            }
        }
        Ok(Self {
            rows: input.rows,
            dependencies,
            blocked: BTreeSet::new(),
            cursor: 0,
            current: None,
            deferred: false,
            progressed: false,
        })
    }
    pub fn next(&mut self) -> Result<String, String> {
        if self.current.is_some() {
            return Err("Settle the current sync effect first.".into());
        }
        while self.cursor < self.rows.len() {
            let index = self.cursor;
            self.cursor += 1;
            let row = &self.rows[index];
            let stream = stream(row);
            if row.status == "synced" || row.sync_archived_at.is_some_and(|v| v > 0) {
                continue;
            }
            if matches!(row.status.as_str(), "failed" | "conflict") {
                if row.engine != "network_reporter" {
                    self.blocked.insert(stream);
                }
                continue;
            }
            if self.blocked.contains(&stream) {
                continue;
            }
            if row.engine == "solar" {
                if let Some(city) = row.operation.strip_prefix("report-") {
                    let key = (
                        row.project_id.clone(),
                        row.client_run_id.clone(),
                        format!("calculate-{city}"),
                    );
                    if self
                        .dependencies
                        .get(&key)
                        .is_some_and(|index| self.rows[*index].status != "synced")
                    {
                        self.deferred = true;
                        continue;
                    }
                }
            }
            self.current = Some(index);
            return Ok(json!({"id":row.id}).to_string());
        }
        Ok(json!({"done":true,"deferred":self.deferred,"progressed":self.progressed}).to_string())
    }
    pub fn settle(&mut self, status: &str) -> Result<(), String> {
        if !matches!(
            status,
            "synced" | "conflict" | "failed" | "pending" | "cancelled" | "changed" | "leased"
        ) {
            return Err("Invalid sync outcome.".into());
        }
        let index = self.current.take().ok_or("No active sync effect.")?;
        let row = &mut self.rows[index];
        if status == "synced" {
            self.progressed = true;
            row.status = status.into();
        } else if matches!(
            status,
            "conflict" | "failed" | "pending" | "leased" | "changed"
        ) {
            if row.engine != "network_reporter"
                || matches!(status, "pending" | "leased" | "changed")
            {
                self.blocked.insert(stream(row));
            }
            row.status = status.into();
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn row(id: &str, engine: &str, operation: &str, status: &str) -> serde_json::Value {
        json!({"id":id,"owner_uid":"u","project_id":"p","engine":engine,"operation":operation,"variant":"default","client_run_id":"run","status":status})
    }
    fn plan(rows: Vec<serde_json::Value>) -> ComputeArtifactDrainPlan {
        ComputeArtifactDrainPlan::new(
            &serde_json::to_vec(&json!({"owner":"u","rows":rows})).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn old_reporter_conflict_does_not_strand_new_inventory_and_transient_does() {
        let mut p = plan(vec![
            row("old", "network_reporter", "export-t", "conflict"),
            row("new", "network_reporter", "export-t", "pending"),
            row("later", "network_reporter", "export-t", "pending"),
            row("other", "network_reporter", "export-u", "pending"),
        ]);
        assert!(p.next().unwrap().contains("new"));
        p.settle("pending").unwrap();
        assert!(p.next().unwrap().contains("other"));
        p.settle("synced").unwrap();
        assert!(p.next().unwrap().contains("\"done\":true"));
    }
    #[test]
    fn calculation_dependency_is_project_fenced_and_failed_pass_does_not_spin() {
        let mut p = plan(vec![
            row("report", "solar", "report-city", "pending"),
            row("calc", "solar", "calculate-city", "pending"),
        ]);
        assert!(p.next().unwrap().contains("calc"));
        p.settle("failed").unwrap();
        let outcome = p.next().unwrap();
        assert!(outcome.contains("\"deferred\":true"));
        assert!(outcome.contains("\"progressed\":false"));
        let mut unrelated = row("other", "solar", "calculate-city", "pending");
        unrelated["project_id"] = json!("other");
        let mut p = plan(vec![
            row("report", "solar", "report-city", "pending"),
            unrelated,
        ]);
        assert!(p.next().unwrap().contains("report"));
    }
}
