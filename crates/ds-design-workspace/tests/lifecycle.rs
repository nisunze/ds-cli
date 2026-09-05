use ds_command_kernel::design::{self, Snapshot};
use ds_design_workspace::{Error, Workspace};
use serde_json::{json, Value};

fn document(name: &str) -> Value {
    json!({"schema":"ds.design.snapshot/v1","transformer":name,"crs":"EPSG:4326",
        "layers":{
            "tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"transformer","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":name,"names":name}}]},
            "lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]},
            "customers":{"type":"FeatureCollection","features":[]}},
        "settings":{},"network_config":{"sheets":{}},"sources":[],"include_design_customers":true})
}
fn write(w: &mut Workspace, name: &str, op: &str) -> Value {
    w.write(&serde_json::to_vec(&document(name)).unwrap(), None, op)
        .unwrap()
}
fn edit(revision: &str, op: &str) -> Value {
    json!({"schema":"ds.design.edit/v1","transformer":"T1","expected_revision":revision,"operation_id":op,
    "mutations":[{"kind":"set_properties","layer":"lv_lines","ids":["line"],"values":{"note":op}}]})
}

#[test]
fn atomic_retry_conflict_and_restore_preserve_history_and_outbox() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    Workspace::init(&root, "local-project").unwrap();
    let mut w = Workspace::open(&root).unwrap();
    let original = write(&mut w, "T1", "create");
    let revision = original["revision"].as_str().unwrap();
    assert_eq!(write(&mut w, "T1", "create"), original);
    let request = edit(revision, "edit1");
    let bytes = serde_json::to_vec(&request).unwrap();
    let changed = w.edit(&bytes).unwrap();
    assert_ne!(changed["revision"], original["revision"]);
    assert_eq!(w.edit(&bytes).unwrap(), changed);
    assert!(matches!(
        w.edit(&serde_json::to_vec(&edit(revision, "edit2")).unwrap()),
        Err(Error::Conflict(_))
    ));
    let mut collision = request.clone();
    collision["mutations"][0]["values"]["note"] = json!("different");
    assert!(matches!(
        w.edit(&serde_json::to_vec(&collision).unwrap()),
        Err(Error::Conflict(_))
    ));
    let mut partial = edit(changed["revision"].as_str().unwrap(), "bad");
    partial["mutations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"delete","layer":"tr","ids":["missing"]}));
    assert!(w.edit(&serde_json::to_vec(&partial).unwrap()).is_err());
    assert_eq!(w.read("T1", None).unwrap().0, changed["revision"]);
    let restored = w
        .restore(
            "T1",
            revision,
            changed["revision"].as_str().unwrap(),
            "undo",
        )
        .unwrap();
    assert_eq!(restored["revision"], original["revision"]);
    assert_eq!(
        w.outbox(0, 100).unwrap()["rows"].as_array().unwrap().len(),
        3
    );
    assert_eq!(
        w.read("T1", Some(changed["revision"].as_str().unwrap()))
            .unwrap()
            .1
            .layers["lv_lines"]["features"][0]["properties"]["note"],
        "edit1"
    );
}

#[test]
fn captured_parallel_run_survives_reopen_and_does_not_consume_later_edits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    Workspace::init(&root, "p").unwrap();
    let mut w = Workspace::open(&root).unwrap();
    let original = write(&mut w, "T1", "create1");
    write(&mut w, "T2", "create2");
    w.prepare_run("r1", &["T2".into(), "T1".into()]).unwrap();
    let changed = w
        .edit(&serde_json::to_vec(&edit(original["revision"].as_str().unwrap(), "edit")).unwrap())
        .unwrap();
    drop(w);
    // Simulate a worker killed after admission but before its atomic result.
    let interrupted = rusqlite::Connection::open(root.join("design.sqlite")).unwrap();
    interrupted
        .execute(
            "UPDATE jobs SET state='running' WHERE run='r1' AND transformer='T1'",
            [],
        )
        .unwrap();
    drop(interrupted);
    let mut w = Workspace::open(&root).unwrap();
    let workers = std::thread::available_parallelism().unwrap().get().min(2);
    let receipt = w.process("r1", workers).unwrap();
    assert_eq!(receipt.completed, 2);
    assert_eq!(receipt.failed, 0);
    let status = w.run_status("r1").unwrap();
    assert_eq!(status["jobs"][0]["transformer"], "T2");
    assert_eq!(status["jobs"][1]["input_revision"], original["revision"]);
    assert_eq!(w.read("T1", None).unwrap().0, changed["revision"]);
    let pending = w.status().unwrap()["pending_publications"].clone();
    w.process("r1", workers).unwrap();
    assert_eq!(w.status().unwrap()["pending_publications"], pending);
    let out = temp.path().join("report-inputs");
    let manifest = w.report_inputs("r1", "T1", &out).unwrap();
    assert_eq!(manifest["input_revision"], original["revision"]);
    let doc: Value =
        serde_json::from_slice(&std::fs::read(out.join("transformer.json")).unwrap()).unwrap();
    assert!(doc["layers"]["lv_poles"]["features"]
        .as_array()
        .is_some_and(|a| !a.is_empty()));
    assert!(w.report_inputs("r1", "T1", &out).is_err());
    assert!(w.prepare_run("r1", &["T1".into(), "T2".into()]).is_err());
}

#[test]
fn cancelled_run_keeps_pending_inputs_and_editing_still_works() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    Workspace::init(&root, "p").unwrap();
    let mut w = Workspace::open(&root).unwrap();
    let original = write(&mut w, "T1", "create");
    w.prepare_run("r", &["T1".into()]).unwrap();
    w.cancel("r").unwrap();
    assert_eq!(w.process("r", 1).unwrap().pending, 1);
    w.edit(&serde_json::to_vec(&edit(original["revision"].as_str().unwrap(), "edit")).unwrap())
        .unwrap();
    assert!(w
        .report_inputs("r", "T1", &temp.path().join("report"))
        .is_err());
}

#[test]
fn source_digest_and_native_settings_are_validated_before_persisting() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    Workspace::init(&root, "p").unwrap();
    let mut w = Workspace::open(&root).unwrap();
    let mut doc = document("T1");
    doc["settings"]["misspelled_setting"] = json!(true);
    assert!(w
        .write(&serde_json::to_vec(&doc).unwrap(), None, "bad")
        .is_err());
    doc = document("T1");
    let collection = json!({"type":"FeatureCollection","features":[]});
    doc["sources"] =
        json!([{"id":"survey","role":"customers","sha256":"bad","collection":collection}]);
    assert!(w
        .write(&serde_json::to_vec(&doc).unwrap(), None, "bad")
        .is_err());
    doc["sources"][0]["sha256"] = json!(design::digest(&collection).unwrap());
    doc["include_design_customers"] = json!(false);
    w.write(&serde_json::to_vec(&doc).unwrap(), None, "valid")
        .unwrap();
    assert_eq!(w.status().unwrap()["transformers"], 1);
    w.prepare_run("source", &["T1".into()]).unwrap();
    assert_eq!(w.process("source", 1).unwrap().completed, 1);
}

#[test]
fn batch_edits_use_stable_identity_through_deletions_and_additions() {
    let snapshot: Snapshot = design::decode(&serde_json::to_vec(&document("T1")).unwrap()).unwrap();
    let request = json!({"schema":"ds.design.edit/v1","transformer":"T1","expected_revision":design::digest(&snapshot).unwrap(),"operation_id":"batch",
        "mutations":[{"kind":"delete","layer":"lv_lines","ids":["line"]},
            {"kind":"add","layer":"lv_lines","features":[{"type":"Feature","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.001,-2.0]]},"properties":{"drafting_status":"approved"}}]}]});
    let result = design::apply_edit(&snapshot, &serde_json::from_value(request).unwrap()).unwrap();
    let rows = result.layers["lv_lines"]["features"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_ne!(rows[0]["id"], "line");
    assert_eq!(rows[0]["properties"]["drafting_status"], "draft");
    assert_eq!(snapshot.layers["lv_lines"]["features"][0]["id"], "line");
}

#[test]
fn independent_connections_cannot_commit_the_same_base_twice() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    Workspace::init(&root, "p").unwrap();
    let mut w = Workspace::open(&root).unwrap();
    let original = write(&mut w, "T1", "create");
    let revision = original["revision"].as_str().unwrap().to_owned();
    drop(w);
    let outcomes = std::thread::scope(|scope| {
        let handles = (0..2)
            .map(|n| {
                let root = &root;
                let revision = &revision;
                scope.spawn(move || {
                    let mut w = Workspace::open(root).unwrap();
                    w.edit(&serde_json::to_vec(&edit(revision, &format!("writer{n}"))).unwrap())
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, Err(Error::Conflict(_))))
            .count(),
        1
    );
}
