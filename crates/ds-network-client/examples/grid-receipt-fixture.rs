//! Adversarial artifact transport only: this fixture performs no Grid computation.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::Path};
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn main() {
    let executable = env::current_exe().unwrap();
    let mode = executable.file_stem().unwrap().to_str().unwrap();
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    let command = arguments[0].to_str().unwrap();
    let value = |flag: &str| arguments.windows(2).find(|pair| pair[0] == flag).unwrap()[1].clone();
    let workers = value("--workers")
        .to_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert!(workers > 0);
    let mut capabilities = vec!["lv_processing_v1"];
    if mode != "missing-capability" {
        capabilities.extend(["grid_query_v1", "grid_manifest_v1", "grid_publication_v1"]);
        if mode != "apply-missing-capability" {
            capabilities.push("grid_apply_v1");
        }
        if mode != "differential-missing-capability" {
            capabilities.push("lv_differential_plan_v1");
        }
        if mode != "catalog-missing-capability" {
            capabilities.push("grid_catalog_v1");
        }
        if mode != "inspection-missing-capability" {
            capabilities.push("grid_inspect_v1");
        }
        if mode != "read-missing-capability" {
            capabilities.push("grid_read_v1");
        }
    }
    let identity = json!({"schema":"ds.network-engine/v1","jsonObjectOrder":"sorted","protocolMajor":1,"protocolMinor":0,"coreVersion":"fixture-core","sourceSha":null,"dependencyClosureSha256":null,"environment":"native","runtime":{"environment":"native","logicalCpus":workers,"internalWorkerThreads":workers},"capabilities":capabilities});
    if command == "build-info" {
        println!(
            "{}",
            json!({"schema":"ds.engine-build/v1","module":"network","engineIdentity":identity})
        );
        return;
    }
    if mode == "missing-capability"
        || mode == "inspection-missing-capability"
        || mode == "differential-missing-capability"
        || mode == "catalog-missing-capability"
        || mode == "apply-missing-capability"
        || mode == "read-missing-capability"
    {
        panic!("missing capability must refuse without starting an operation");
    }
    if mode == "structured-refusal" || mode == "malformed-refusal" {
        println!(
            "{}",
            if mode == "malformed-refusal" {
                json!({"schema":"invalid","code":"bad"})
            } else {
                json!({"schema":"ds.network.task-refusal/v1","code":"precise_grid_refusal","error":"owner declined","detail":{"constraint":"retained"},"remedy":"use the exact owner parameters","invalidInput":false,"engineIdentity":identity})
            }
        );
        std::process::exit(1);
    }
    let model_path = if command == "grid-catalog" || command == "lv-differential-plan" {
        None
    } else {
        Some(value("--model"))
    };
    let model = model_path
        .as_ref()
        .map(|path| fs::read(path).unwrap())
        .unwrap_or_default();
    let request_bytes = if command == "grid-manifest" || command == "grid-inspect" {
        None
    } else {
        Some(fs::read(value("--request")).unwrap())
    };
    let request: Value = request_bytes
        .as_ref()
        .map(|bytes| serde_json::from_slice(bytes).unwrap())
        .unwrap_or(Value::Null);
    if mode == "live-progress" || mode == "wrong-progress-input" {
        eprintln!(
            "{}",
            json!({"schema":"ds.network.grid-progress/v1",
            "input_sha256":if mode == "wrong-progress-input" {"0".repeat(64)} else {sha(request_bytes.as_ref().unwrap())},
            "progress":{"fixture":"before-terminal"},"elapsed_ms":0,"eta":null})
        );
        std::io::stderr().flush().unwrap();
        if mode == "live-progress" {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    let source = json!({"package_sha256":if mode=="wrong-package" {"0".repeat(64)} else {sha(&model)}, "model_id":"fixture-model", "expected_revision":format!("rev:{}", if mode=="wrong-revision" {"0"} else {"a"}.repeat(64)), "engineering_input_root":format!("fnv1a64:{}", "b".repeat(16))});
    let (name, schema, result) = match command {
        "grid-apply" => {
            let dry_run = request["dry_run"].as_bool().unwrap();
            let emit = !dry_run || mode == "apply-preview-model";
            let payload = b"complete opaque model";
            let mut projection = json!({
                "source":{"model_id":if mode=="apply-wrong-source" {"other"} else {"fixture-model"},"package_revision":0,"authored_revision":format!("rev:{}","a".repeat(64))},
                "command_id":request["envelope"]["command_id"],"command_kind":request["envelope"]["command"]["command_kind"],
                "dry_run":dry_run,"persisted":false,"resulting_revision":format!("rev:{}",if mode=="apply-wrong-projection" {"d"} else {"c"}.repeat(64)),
            });
            let mut artifacts = Vec::new();
            if emit {
                let out = value("--out");
                let target = Path::new(&out).join("model.dsgrid");
                if mode != "apply-missing-model" {
                    #[cfg(unix)]
                    if mode == "apply-symlink-model" {
                        let untrusted = Path::new(&out).parent().unwrap().join("untrusted.dsgrid");
                        fs::write(&untrusted, payload).unwrap();
                        std::os::unix::fs::symlink(untrusted, &target).unwrap();
                    } else {
                        fs::write(&target, payload).unwrap();
                    }
                    #[cfg(not(unix))]
                    fs::write(&target, payload).unwrap();
                }
                if mode == "apply-extra-file" {
                    fs::write(Path::new(&out).join("unexpected.bin"), b"extra").unwrap();
                }
                artifacts.push(json!({"file":"model.dsgrid","schema":"ds.grid/v5","byteCount":payload.len(),"sha256":if mode=="apply-model-digest" {"0".repeat(64)} else {sha(payload)}}));
                projection["artifact"] = json!({"package_revision":1,"byte_len":payload.len(),"sha256":format!("sha256:{}",sha(payload))});
            }
            let mut source = source;
            if mode == "apply-wrong-source" {
                source["model_id"] = json!("other");
            }
            (
                "result.json",
                "ds.grid.native-apply-result/v1",
                json!({
                    "schema":"ds.grid.native-apply-result/v1","source":source,"engine_version":"fixture-grid/1","result_revision":format!("rev:{}","c".repeat(64)),
                    "result_engineering_input_root":format!("fnv1a64:{}","b".repeat(16)),"model_schema":5,"projection":projection,"journal":{"history":{},"entries":[]},"artifacts":artifacts,
                }),
            )
        }
        "lv-differential-plan" => (
            "plan.json",
            "ds.lv.native-differential-plan-result/v1",
            json!({"schema":if mode=="differential-wrong-schema" {"other/v1"} else {"ds.lv.native-differential-plan-result/v1"},
                "plan":{"scope":if mode=="differential-wrong-scope" {"host_override"} else {"full"},
                    "feeders":if mode=="differential-wrong-full" {json!(["f1"])} else {json!([])},
                    "reason_key":if mode=="differential-wrong-reason" {"future_reason"} else {"differential_full_no_feeders"},
                    "selected_count":0,"frozen_count":0,"blocking_diagnostics":false,"manual_available":false}}),
        ),
        "grid-catalog" => {
            let kind = if mode == "catalog-wrong-kind" {
                json!("types")
            } else {
                request["kind"].clone()
            };
            let version = "fixture-grid/1";
            let data = if let Some(id) = request["id"].as_str() {
                json!({"engine":version,"kind":kind,"descriptor":{"operation_id":if mode=="catalog-wrong-descriptor" {"other"} else {id}}})
            } else {
                json!({"engine":if mode=="catalog-wrong-version" {"other"} else {version},"kind":kind,"entries":[]})
            };
            (
                "catalog.json",
                "ds.grid.native-catalog-result/v1",
                json!({"schema":"ds.grid.native-catalog-result/v1","kind":kind,"engine_version":version,"data":data}),
            )
        }
        "grid-read" => {
            let issue_limit = request["issue_limit"].as_u64().unwrap() as usize;
            let advisory_limit = request["advisory_limit"].as_u64().unwrap() as usize;
            let damaged = mode == "read-damaged-shape";
            (
                "read.json",
                "ds.grid.native-read-result/v1",
                json!({
                    "schema":if mode=="read-wrong-schema" {"other/v1"} else {"ds.grid.native-read-result/v1"},
                    "package_sha256":if mode=="read-wrong-package" {"0".repeat(64)} else {sha(&model)},
                    "issue_limit":if mode=="read-wrong-limits" {issue_limit+1} else {issue_limit},
                    "advisory_limit":advisory_limit,
                    "container":{"verified":!damaged,"members":if damaged {Value::Null} else {json!(65)},
                        "detail":if damaged {json!({"detail":"damaged"})} else {Value::Null}},
                    "model":if damaged {
                        json!({"id":"impossible"})
                    } else {
                        json!({"id":"fixture-model","package_revision":7,
                            "authored_revision":format!("rev:{}","a".repeat(64)),
                            "fingerprint":"fnv1a64:0123456789abcdef","valid":mode!="read-invalid-counts",
                            "issue_count":if mode=="read-invalid-counts" {1} else {0},
                            "issues":[],"issues_withheld":0,"advisory_count":0,
                            "advisories":[],"advisories_withheld":0,
                            "counts":{"alignments":1,"route_nodes":2,"structures":3,
                                "structure_types":4,"tension_sections":5,"described_structures":2}})
                    }
                }),
            )
        }
        "grid-inspect" => (
            "inspection.json",
            "ds.grid.native-inspection/v1",
            json!({"schema":"ds.grid.native-inspection/v1","source":source,"model_schema":5,"model_revision":0,
                "coordinate_system":"EPSG:32736","source_snapshot_fingerprint":"retained","opened_content_root":"retained",
                "table_counts":{},"content_summary":{"structure_type_names":[],"cable_names":[],"resource_leaves":[],
                "route_node_extent":null,"structure_extent":null,"terrain_extent":null}}),
        ),
        "grid-query" => (
            "query.json",
            "ds.grid.native-query-result/v1",
            json!({"schema":"ds.grid.native-query-result/v1","source":source,"model_revision":0,"engine_version":"fixture-grid/1","operation":{"operation_id":if mode=="wrong-operation" {json!("other")} else {request["operation_id"].clone()},"journaled":false},"result":{"complete":true}}),
        ),
        "grid-manifest" => (
            "manifest.json",
            "ds.grid.native-manifest/v1",
            json!({"schema":"ds.grid.native-manifest/v1","package_sha256":if mode=="manifest-wrong-source" {"0".repeat(64)} else {sha(&model)},"decoded":mode=="manifest-decoded","manifest":{"model":{"schema_version":5}},"members":["manifest.json"]}),
        ),
        "grid-publication" => (
            "publication.json",
            "ds.grid.native-publication-result/v1",
            json!({"schema":"ds.grid.native-publication-result/v1","source":source,"source_revision_id":format!("rev:{}",sha(&model)),"manifest_model_revision":0,"model_schema":5,"engine_version":"fixture-grid/1"}),
        ),
        _ => panic!("unknown fixed Grid fixture task"),
    };
    let bytes = serde_json::to_vec(&result).unwrap();
    let out = value("--out");
    let artifact = Path::new(&out).join(name);
    #[cfg(unix)]
    if mode == "symlink-result" {
        let target = Path::new(&out).join("untrusted.json");
        fs::write(&target, &bytes).unwrap();
        std::os::unix::fs::symlink(target, &artifact).unwrap();
    } else {
        fs::write(&artifact, &bytes).unwrap();
    }
    #[cfg(not(unix))]
    fs::write(&artifact, &bytes).unwrap();
    if mode == "changed-input" || mode == "read-changed-input" {
        if let Some(model_path) = &model_path {
            fs::write(model_path, b"changed package").unwrap();
        } else {
            fs::write(value("--request"), b"changed request").unwrap();
        }
    }
    let receipt_identity = if mode == "differential-wrong-identity" {
        let mut changed = identity;
        changed["coreVersion"] = json!("changed-core");
        changed
    } else {
        identity
    };
    println!(
        "{}",
        json!({
            "schema":"ds.network.task-result/v1", "inputSha256":if mode=="wrong-input" {"0".repeat(64)} else {sha(request_bytes.as_deref().unwrap_or(&model))},
            "engineIdentity":receipt_identity, "outcome":if mode=="cancelled" {"cancelled"} else {"completed"},
            "artifact":{"file":if mode=="wrong-file" {"other.json"} else {name},"schema":if mode=="wrong-receipt-schema" {"other/v1"} else {schema},"byteCount":bytes.len(),"sha256":if mode=="wrong-digest" {"0".repeat(64)} else {sha(&bytes)}}
        })
    );
}
