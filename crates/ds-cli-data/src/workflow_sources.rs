//! IO adapter for native-pinned document sources. No active-project state,
//! implicit current revision, processing call or reusable credential payload.
use ds_cli_contract::Failure;
use ds_command_kernel::{
    design::{self, Snapshot},
    workflow_documents::{self, Binding, Source},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
fn failure(message: String) -> Failure {
    let code = message
        .split(':')
        .next()
        .filter(|c| c.starts_with("workflow_"))
        .unwrap_or("workflow_document_invalid")
        .to_owned();
    match code.as_str() {
        "workflow_source_changed"
        | "workflow_authority_missing"
        | "workflow_budget_exceeded"
        | "workflow_op_version_unsupported"
        | "workflow_document_invalid" => Failure::invalid(code, message),
        _ => Failure::invalid("workflow_document_invalid", message),
    }
    .remedy("Bind the exact authorized source and its digest; never substitute a newer head.")
}
pub fn bind(document: &Value, bindings: &mut Value, base: &Path) -> Result<Value, Failure> {
    let required = ds_network::vector::workflow::referenced_inputs(document);
    let mut receipts = BTreeMap::new();
    let Some(inputs) = document["inputs"].as_object() else {
        return Ok(json!({"reads":0,"local_file_bytes":0,"sources":{}}));
    };
    let mut reads = 0;
    let mut bytes = 0;
    for (name, input) in inputs {
        if !required.contains(name) {
            continue;
        }
        if input["type"] != "document" {
            continue;
        }
        let value = bindings
            .get(name)
            .cloned()
            .or_else(|| input.get("default").cloned());
        let Some(value) = value else {
            continue;
        };
        if value.get("projection").is_none() {
            continue;
        }
        let binding: Binding = serde_json::from_value(value).map_err(|e| failure(e.to_string()))?;
        let started = std::time::Instant::now();
        let mut raw_sha = None;
        let raw = match &binding.source {
            Source::File { path, .. } => {
                let content = read(&base.join(path))?;
                bytes += content.len();
                reads += 1;
                raw_sha = Some(format!("{:x}", Sha256::digest(&content)));
                serde_json::from_slice(&content).map_err(|e| failure(e.to_string()))?
            }
            Source::Workspace {
                path,
                project,
                transformer,
                revision,
            } => {
                let workspace = ds_design_workspace::Workspace::open(&base.join(path))
                    .map_err(|e| failure(e.to_string()))?;
                if workspace.status().map_err(|e| failure(e.to_string()))?["project"] != *project {
                    return Err(failure(
                        "workflow_source_changed: workspace project differs".into(),
                    ));
                }
                let (actual, snapshot) = workspace
                    .read(transformer, Some(revision))
                    .map_err(|e| failure(e.to_string()))?;
                if actual != *revision {
                    return Err(failure(
                        "workflow_source_changed: retained revision differs".into(),
                    ));
                }
                reads += 1;
                serde_json::to_value(snapshot).unwrap()
            }
            Source::Transformer {
                lane,
                project,
                transformer,
                version,
                content_digest,
            } => {
                let scope = binding.scope.as_ref().ok_or_else(|| {
                    failure(
                        "workflow_authority_missing: transformer input requires explicit scope"
                            .into(),
                    )
                })?;
                scope
                    .validate()
                    .map_err(|e| failure(format!("workflow_authority_missing: {}", e.message)))?;
                if scope.project != *project || scope.lane != *lane {
                    return Err(failure(
                        "workflow_authority_missing: source and declared scope differ".into(),
                    ));
                }
                let held =
                    ds_cli_auth::saved_transformer_context_for_project(lane, project, transformer)?;
                let identity = held.identity();
                if scope.project != *project
                    || scope.lane != identity.lane()
                    || scope.principal != identity.uid()
                    || scope.audience != identity.credential_audience_sha256()
                {
                    return Err(failure(
                        "workflow_authority_missing: source scope differs from restored identity"
                            .into(),
                    ));
                }
                let context = held.snapshot();
                if context.metadata().version() != Some(*version)
                    || context.metadata().content_digest() != Some(content_digest.as_str())
                {
                    return Err(failure(
                        "workflow_source_changed: saved transformer head moved".into(),
                    ));
                }
                reads += 1;
                let snapshot = Snapshot {
                    schema: design::SCHEMA.into(),
                    transformer: transformer.clone(),
                    crs: "EPSG:4326".into(),
                    layers: context.layers().clone(),
                    settings: Default::default(),
                    network_config: serde_json::from_value(json!({"sheets":{}})).unwrap(),
                    include_design_customers: true,
                    sources: vec![],
                };
                serde_json::to_value(snapshot).unwrap()
            }
        };
        let projected =
            workflow_documents::project(&binding, raw, raw_sha.as_deref()).map_err(failure)?;
        receipts.insert(name.clone(),json!({"source":binding.source,"digest":design::digest(&projected).map_err(failure)?,"elapsed_ms":started.elapsed().as_millis(),"cache":"explicit_immutable_source"}));
        bindings[name] = projected;
    }
    Ok(json!({"reads":reads,"local_file_bytes":bytes,"sources":receipts}))
}
fn read(path: &Path) -> Result<Vec<u8>, Failure> {
    ds_design_workspace::read_file(path, workflow_documents::MAX_BYTES)
        .map_err(|e| failure(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static ID: AtomicU64 = AtomicU64::new(0);
    #[test]
    fn historical_bindings_read_only_exact_retained_versions_and_file_pins() {
        let root = std::env::temp_dir().join(format!(
            "ds-workflow-sources-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let workspace = root.join("workspace");
        ds_design_workspace::Workspace::init(&workspace, "P1").unwrap();
        let mut w = ds_design_workspace::Workspace::open(&workspace).unwrap();
        let snapshot = json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326","layers":{"lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","id":"p1","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"struct_type":"old","drafting_status":"drafted"}}]}},"settings":{},"network_config":{"sheets":{}},"include_design_customers":true,"sources":[]});
        w.write(&serde_json::to_vec(&snapshot).unwrap(), None, "create")
            .unwrap();
        let (first, retained_first) = w.read("T1", None).unwrap();
        let mut changed = snapshot.clone();
        changed["layers"]["lv_poles"]["features"][0]["properties"]["struct_type"] = json!("new");
        w.write(
            &serde_json::to_vec(&changed).unwrap(),
            Some(&first),
            "change",
        )
        .unwrap();
        let (second, retained_second) = w.read("T1", None).unwrap();
        let source = |revision: &str| json!({"source":{"kind":"workspace","path":"workspace","project":"P1","transformer":"T1","revision":revision},"projection":"snapshot"});
        let document = json!({"inputs":{"before":{"type":"document"},"after":{"type":"document"}},"outputs":{"before":{"$ref":"inputs.before"},"after":{"$ref":"inputs.after"}}});
        let mut inputs = json!({"before":source(&first),"after":source(&second)});
        let receipt = bind(&document, &mut inputs, &root).unwrap();
        assert_eq!(receipt["reads"], 2);
        assert_eq!(
            inputs["before"],
            serde_json::to_value(&retained_first).unwrap()
        );
        assert_eq!(
            inputs["after"],
            serde_json::to_value(&retained_second).unwrap()
        );
        assert_eq!(w.read("T1", None).unwrap().0, second);
        let mut wrong = source(&first);
        wrong["source"]["project"] = json!("P2");
        assert!(
            bind(
                &json!({"inputs":{"before":{"type":"document"}},"outputs":{"before":{"$ref":"inputs.before"}}}),
                &mut json!({"before":wrong}),
                &root
            )
            .is_err()
        );
        let file = root.join("source.json");
        let content = serde_json::to_vec(&snapshot).unwrap();
        std::fs::write(&file, &content).unwrap();
        let pinned = json!({"source":{"kind":"file","path":"source.json","sha256":format!("{:x}",Sha256::digest(&content))},"projection":"snapshot"});
        let mut inputs = json!({"before":pinned});
        assert!(bind(&document, &mut inputs, &root).is_ok());
        let changed_bytes = serde_json::to_vec(&changed).unwrap();
        std::fs::write(root.join("target.json"), &changed_bytes).unwrap();
        let target_file = json!({"source":{"kind":"file","path":"target.json","sha256":format!("{:x}",Sha256::digest(&changed_bytes))},"projection":"snapshot"});
        let scope =
            json!({"project":"P1","principal":"user","lane":"canary","audience":"authority"});
        let graph = json!({"schema":"ds.vector-workflow/v1","name":"Pinned source comparison","inputs":{"before":{"type":"document"},"after":{"type":"document"}},"steps":[{"id":"compare","tool":"design.repair.compare","request":{"source":{"$ref":"inputs.before"},"target":{"$ref":"inputs.after"},"request":{"scope":scope,"source_layer":"lv_poles","target_layer":"lv_poles","matching":{"kind":"exact_position","crs":"EPSG:4326","dimensions":"xy"},"fields":[{"source":"struct_type","target":"struct_type"}]}}}],"outputs":{"comparison":{"$ref":"steps.compare.outputs.report"}}});
        for (before, after) in [
            (pinned.clone(), target_file),
            (pinned.clone(), source(&second)),
            (source(&first), source(&second)),
        ] {
            let mut before = before;
            before["projection"] = json!("evidence");
            before["scope"] = scope.clone();
            before["id"] = json!("reviewed-history");
            let mut inputs = json!({"before":before,"after":after});
            let receipt = bind(&graph, &mut inputs, &root).unwrap();
            assert_eq!(receipt["reads"], 2);
            let result = ds_network::vector::workflow::run_with_operations(
                graph.clone(),
                inputs,
                Default::default(),
                ds_network::vector::RunOptions::default(),
                &super::super::workflow_operations::NativeOperations,
            )
            .unwrap();
            assert_eq!(
                result.metadata["status"], "completed",
                "{}",
                result.metadata
            );
            assert_eq!(
                result.metadata["outputs"]["comparison"]["different_cells"],
                1
            );
            assert_eq!(w.read("T1", None).unwrap().0, second);
        }
        std::fs::write(&file, b"{}").unwrap();
        assert!(bind(&document, &mut json!({"before":pinned}), &root).is_err());
        // Unused cloud-looking bindings are never resolved, not even freshness-read.
        let unused = json!({"inputs":{"used":{"type":"document"},"unused":{"type":"document"}},"outputs":{"used":{"$ref":"inputs.used"}}});
        let mut inputs = json!({"used":{"already":"held"},"unused":{"source":{"kind":"transformer","lane":"canary","project":"P1","transformer":"T2","version":1,"content_digest":"moved"},"projection":"snapshot"}});
        let receipt = bind(&unused, &mut inputs, &root).unwrap();
        assert_eq!(receipt["reads"], 0);
        // Reject a missing or contradictory explicit cloud scope before restoring
        // credentials or acquiring even the selected transformer.
        let cloud = json!({"source":{"kind":"transformer","lane":"canary","project":"P1","transformer":"T2","version":1,"content_digest":"moved"},"projection":"snapshot"});
        let failure = bind(&document, &mut json!({"before":cloud}), &root).unwrap_err();
        assert_eq!(failure.code(), "workflow_authority_missing");
        let mut cloud = cloud;
        cloud["scope"] =
            json!({"project":"P2","principal":"user","lane":"canary","audience":"authority"});
        let failure = bind(&document, &mut json!({"before":cloud}), &root).unwrap_err();
        assert_eq!(failure.code(), "workflow_authority_missing");
        std::fs::remove_dir_all(root).unwrap();
    }
}
