//! Register the portable repair owner into the existing DAG, without a second
//! interpreter or an effect-capable step. Persistence stays explicit.
use ds_command_kernel::design_repair;
use ds_network::vector::workflow::{OperationError, WorkflowOperations};
use serde_json::Value;
pub struct NativeOperations;
impl WorkflowOperations for NativeOperations {
    fn descriptor(&self, id: &str) -> Option<Value> {
        let mut op = design_repair::describe()["operations"]
            .as_array()?
            .iter()
            .find(|op| op["id"] == id)?
            .clone();
        op["effect"] = Value::String("pure".into());
        Some(op)
    }
    fn evaluate(&self, id: &str, request: Value) -> Result<Value, OperationError> {
        design_repair::evaluate(id, request).map_err(|e| OperationError {
            code: e.code,
            message: e.message,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_command_kernel::{
        design::{self, Snapshot},
        design_repair::{ApplySelection, Proposal},
    };
    use ds_network::vector::{RunOptions, workflow};
    use serde_json::json;
    use std::collections::BTreeMap;
    fn target() -> Snapshot {
        serde_json::from_value(json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326","layers":{"lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","id":"pole-1","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"struct_type":"old","drafting_status":"approved","reference":"a"}}]}},"settings":{},"network_config":{"sheets":{}},"include_design_customers":true,"sources":[]})).unwrap()
    }
    fn scope() -> Value {
        json!({"project":"P1","principal":"user","lane":"canary","audience":"authority"})
    }
    fn evidence() -> Value {
        let mut layers = target().layers;
        layers.get_mut("lv_poles").unwrap()["features"][0]["properties"]["struct_type"] =
            json!("new");
        json!({"schema":"ds.repair.evidence/v1","id":"file","scope":scope(),"content_digest":design::digest(&layers).unwrap(),"crs":"EPSG:4326","units":{},"layers":layers})
    }
    #[test]
    fn file_and_retained_snapshot_compare_and_propose_use_one_native_graph() {
        let target = target();
        let source = evidence();
        let scope = scope();
        let request = json!({"scope":scope,"source_layer":"lv_poles","target_layer":"lv_poles","matching":{"kind":"exact_position","crs":"EPSG:4326","dimensions":"xy"},"fields":[{"source":"struct_type","target":"struct_type"}],"max_details":100});
        let proposal_request = json!({"scope":scope,"target":{"snapshot_revision":design::digest(&target).unwrap()},"policy":{"scope":scope,"fields":{"lv_poles":{"struct_type":{"field_type":"string"}}}},"allowlist":{"lv_poles":["struct_type"]},"operations":[{"kind":"restore","source_layer":"lv_poles","target_layer":"lv_poles","matching":{"kind":"exact_position","crs":"EPSG:4326","dimensions":"xy"},"fields":[{"source":"struct_type","target":"struct_type"}]}]});
        let document = json!({"schema":"ds.vector-workflow/v1","name":"Restore exact facts","inputs":{"source":{"type":"document"},"target":{"type":"document"},"compare_options":{"type":"document"},"proposal_options":{"type":"document"}},"steps":[{"id":"compare","tool":"design.repair.compare","request":{"source":{"$ref":"inputs.source"},"target":{"$ref":"inputs.target"},"request":{"$ref":"inputs.compare_options"}}},{"id":"propose","tool":"design.repair.propose","request":{"source":{"$ref":"inputs.source"},"target":{"$ref":"inputs.target"},"request":{"$ref":"inputs.proposal_options"}}}],"outputs":{"findings":{"$ref":"steps.compare.outputs.report"},"proposal":{"$ref":"steps.propose.outputs.report"}}});
        let bindings = json!({"source":source,"target":target,"compare_options":request,"proposal_options":proposal_request});
        if let Ok(path) = std::env::var("DS_REPAIR_FIXTURE_DIR") {
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                std::path::Path::new(&path).join("exact-position-workflow.json"),
                serde_json::to_vec(&document).unwrap(),
            )
            .unwrap();
            std::fs::write(
                std::path::Path::new(&path).join("bindings.json"),
                serde_json::to_vec(&bindings).unwrap(),
            )
            .unwrap();
        }
        let result = workflow::run_with_operations(
            document,
            bindings,
            BTreeMap::new(),
            RunOptions::default(),
            &NativeOperations,
        )
        .unwrap();
        assert_eq!(
            result.metadata["status"], "completed",
            "{}",
            result.metadata
        );
        let proposal: Proposal =
            serde_json::from_value(result.metadata["outputs"]["proposal"].clone()).unwrap();
        assert_eq!(proposal.changes.len(), 1);
        let selection:ApplySelection=serde_json::from_value(json!({"scope":scope,"proposal_digest":proposal.digest,"change_ids":[proposal.changes[0].id],"confirmed":true})).unwrap();
        let projected = design_repair::apply_selected(
            Some(&serde_json::from_value(source).unwrap()),
            &target,
            &proposal,
            &selection,
        )
        .unwrap();
        assert_eq!(
            projected.snapshot.layers["lv_poles"]["features"][0]["properties"]["struct_type"],
            "new"
        );
        assert_eq!(
            projected.snapshot.layers["lv_poles"]["features"][0]["properties"]["drafting_status"],
            "approved"
        );
        assert!(!projected.persisted);
    }
    #[test]
    fn independent_key_join_and_owner_refusals_survive_workflow_execution() {
        let source = evidence();
        let target = target();
        let request = json!({"source":source,"target":target,"request":{"scope":scope(),"source_layer":"lv_poles","target_layer":"lv_poles","matching":{"kind":"keys","pairs":[{"source":"reference","target":"reference"}]},"fields":[{"source":"struct_type","target":"struct_type"}],"max_details":100}});
        let mut document = json!({"schema":"ds.vector-workflow/v1","name":"Key compare","steps":[{"id":"compare","tool":"design.repair.compare","tool_version":1,"request":request}],"outputs":{"findings":{"$ref":"steps.compare.outputs.report"}}});
        let result = workflow::run_with_operations(
            document.clone(),
            json!({}),
            BTreeMap::new(),
            RunOptions::default(),
            &NativeOperations,
        )
        .unwrap();
        assert_eq!(
            result.metadata["status"], "completed",
            "{}",
            result.metadata
        );
        document["steps"][0]["request"]["source"]["content_digest"] = json!("moved");
        let result = workflow::run_with_operations(
            document.clone(),
            json!({}),
            BTreeMap::new(),
            RunOptions::default(),
            &NativeOperations,
        )
        .unwrap();
        assert_eq!(result.metadata["error"]["code"], "repair_source_changed");
        document["steps"][0]["tool_version"] = json!(99);
        assert!(
            workflow::validate_with_operations(
                document,
                json!({}),
                BTreeMap::new(),
                &NativeOperations
            )
            .is_err()
        );
    }
}
