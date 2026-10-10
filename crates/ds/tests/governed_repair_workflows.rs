//! Exercise the shipping executable against retained synthetic facts. No DS
//! project, session, cloud acquisition or processing is involved.
use ds_command_kernel::{design, design_repair};
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn invoke(args: &[&str]) -> Value {
    let result = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .args(["--output", "json"])
        .output()
        .unwrap();
    let envelope: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(result.status.success(), "{envelope}");
    envelope["data"].clone()
}
fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn guarded_repair_and_reusable_workflow_share_exact_native_results_offline() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let scope = json!({"project":"P1","principal":"fixture-user","lane":"canary","audience":"fixture-authority"});
    let target: design::Snapshot = serde_json::from_value(json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326","layers":{"lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","id":u64::MAX,"geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"struct_type":"old","drafting_status":"approved","stay":0}}]}},"settings":{},"network_config":{"sheets":{}},"include_design_customers":true,"sources":[]})).unwrap();
    let mut layers = target.layers.clone();
    layers.get_mut("lv_poles").unwrap()["features"][0]["properties"]["struct_type"] = json!("new");
    let source = json!({"schema":"ds.repair.evidence/v1","id":"reviewed-external","scope":scope,"content_digest":design::digest(&layers).unwrap(),"crs":"EPSG:4326","units":{},"layers":layers});
    let request = json!({"scope":scope,"target":{"snapshot_revision":design::digest(&target).unwrap()},"policy":{"scope":scope,"fields":{"lv_poles":{"struct_type":{"field_type":"string"}}}},"allowlist":{"lv_poles":["struct_type"]},"operations":[{"kind":"restore","source_layer":"lv_poles","target_layer":"lv_poles","matching":{"kind":"exact_position","crs":"EPSG:4326","dimensions":"xy"},"fields":[{"source":"struct_type","target":"struct_type"}]}]});
    let input = json!({"source":source,"target":target,"request":request});
    let expected = design_repair::evaluate("design.repair.propose", input.clone()).unwrap();
    let input_path = root.join("proposal-input.json");
    let output_path = root.join("proposal.json");
    write(&input_path, &input);
    invoke(&[
        "design",
        "repair",
        "propose",
        "--input",
        input_path.to_str().unwrap(),
        "--out",
        output_path.to_str().unwrap(),
    ]);
    assert_eq!(read(&output_path), expected);
    let selection = json!({"scope":scope,"proposal_digest":expected["digest"],"change_ids":[expected["changes"][0]["id"]],"confirmed":true});
    let apply = json!({"source":source,"target":target,"proposal":expected,"selection":selection});
    let apply_path = root.join("apply-input.json");
    let applied_path = root.join("applied.json");
    write(&apply_path, &apply);
    invoke(&[
        "design",
        "repair",
        "apply-selected",
        "--input",
        apply_path.to_str().unwrap(),
        "--out",
        applied_path.to_str().unwrap(),
    ]);
    let projected = read(&applied_path);
    assert_eq!(
        projected,
        design_repair::evaluate("design.repair.apply-selected", apply).unwrap()
    );
    assert_eq!(projected["persisted"], false);
    assert_eq!(
        projected["snapshot"]["layers"]["lv_poles"]["features"][0]["id"],
        json!(u64::MAX)
    );
    assert_eq!(
        projected["snapshot"]["layers"]["lv_poles"]["features"][0]["properties"]["drafting_status"],
        "approved"
    );
    let graph = json!({"schema":"ds.vector-workflow/v1","name":"Exact approved facts","inputs":{"request":{"type":"document"},"unused_cloud":{"type":"document"}},"steps":[{"id":"proposal","tool":"design.repair.propose","request":{"$ref":"inputs.request"}}],"outputs":{"proposal":{"$ref":"steps.proposal.outputs.report"}}});
    let graph_path = root.join("temporary.json");
    write(&graph_path, &graph);
    let bindings = serde_json::to_string(&json!({"request":input,"unused_cloud":{"source":{"kind":"transformer","lane":"canary","project":"NEVER","transformer":"NEVER","version":1,"content_digest":"moved"},"projection":"snapshot"}})).unwrap();
    let run = invoke(&[
        "data",
        "vector",
        "workflow",
        "run",
        "--file",
        graph_path.to_str().unwrap(),
        "--inputs",
        &bindings,
    ]);
    assert_eq!(run["outputs"]["proposal"], expected);
    assert_eq!(run["source_acquisition"]["reads"], 0);
    let library = root.join("library");
    let saved = invoke(&[
        "data",
        "vector",
        "workflow",
        "save",
        "--file",
        graph_path.to_str().unwrap(),
        "--library",
        library.to_str().unwrap(),
        "--name",
        "reviewed-repair",
    ]);
    let shown = invoke(&[
        "data",
        "vector",
        "workflow",
        "show",
        "--library",
        library.to_str().unwrap(),
        "--name",
        "reviewed-repair",
        "--digest",
        saved["definition"]["digest"].as_str().unwrap(),
    ]);
    assert_eq!(shown, saved["definition"]);
    let listing = invoke(&[
        "data",
        "vector",
        "workflow",
        "list",
        "--library",
        library.to_str().unwrap(),
    ]);
    assert_eq!(listing["revisions"].as_array().unwrap().len(), 1);
    let reused = invoke(&[
        "data",
        "vector",
        "workflow",
        "run",
        "--file",
        saved["path"].as_str().unwrap(),
        "--inputs",
        &bindings,
    ]);
    assert_eq!(reused["outputs"]["proposal"], expected);
    assert_eq!(reused["source_acquisition"]["reads"], 0);
    // Existing vector recipes retain their native string contract version too.
    let vector = root.join("vector.json");
    write(&vector, &ds_network::vector::workflow::examples()[0]);
    // Saved reusable definitions bind layer holdings per run rather than retain defaults.
    let mut vector_recipe = read(&vector);
    for input in vector_recipe["inputs"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        if input["type"] == "layer" {
            input.as_object_mut().unwrap().remove("default");
        }
    }
    write(&vector, &vector_recipe);
    let saved_vector = invoke(&[
        "data",
        "vector",
        "workflow",
        "save",
        "--file",
        vector.to_str().unwrap(),
        "--library",
        library.to_str().unwrap(),
        "--name",
        "existing-vector",
    ]);
    assert_eq!(
        saved_vector["definition"]["document"]["steps"][0]["tool_version"],
        "ds.vector-tool/v1"
    );
    assert_eq!(read(&input_path), input);
}

#[test]
fn invalid_graph_refuses_before_selected_cloud_source_acquisition() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("invalid.json");
    write(
        &file,
        &json!({"schema":"ds.vector-workflow/v1","name":"Invalid explicit cloud consumer","inputs":{"selected":{"type":"document"}},"steps":[{"id":"bad","tool":"unknown-operation","request":{"source":{"$ref":"inputs.selected"}}}],"outputs":{"result":{"$ref":"steps.bad.outputs.report"}}}),
    );
    let bindings = json!({"selected":{"source":{"kind":"transformer","lane":"canary","project":"P1","transformer":"T1","version":1,"content_digest":"not-acquired"},"projection":"snapshot"}}).to_string();
    for action in ["validate", "run"] {
        let result = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args([
                "data",
                "vector",
                "workflow",
                action,
                "--file",
                file.to_str().unwrap(),
                "--inputs",
                &bindings,
                "--output",
                "json",
            ])
            .output()
            .unwrap();
        let envelope: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert!(!result.status.success());
        // A source acquisition would instead hit missing authority/native state.
        assert_eq!(
            envelope["error"]["code"], "vector_tool_unknown",
            "{envelope}"
        );
    }
}
