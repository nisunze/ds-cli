use ds_design_workspace::Workspace;
use serde_json::{json, Value};

fn initial() -> Value {
    json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326",
        "layers":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr","geometry":{"type":"Point","coordinates":[30,-2]},"properties":{}}]},
        "tapping_poles":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tap","geometry":{"type":"Point","coordinates":[30.001,-2]},"properties":{"drafting_status":"approved","assembly_type":"ABC Terminal Assembly"}}]},
        "lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line","geometry":{"type":"LineString","coordinates":[[30,-2],[30.001,-2]]},"properties":{}}]}},
        "settings":{},"network_config":{"sheets":{}},"sources":[],"include_design_customers":true})
}

fn setup() -> (tempfile::TempDir, Workspace, String, Value) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("design");
    Workspace::init(&path, "project").unwrap();
    let mut workspace = Workspace::open(&path).unwrap();
    workspace
        .write(&serde_json::to_vec(&initial()).unwrap(), None, "create")
        .unwrap();
    let (revision, snapshot) = workspace.read("T1", None).unwrap();
    (
        directory,
        workspace,
        revision,
        serde_json::to_value(snapshot).unwrap(),
    )
}

#[test]
fn replacement_refusals_leave_revision_history_outbox_and_operation_unused() {
    let (_directory, mut workspace, revision, before) = setup();
    for kind in [
        "delete_layer",
        "delete_row",
        "geometry",
        "property",
        "owner",
        "demote",
        "duplicate",
        "malformed",
        "wrong_geometry",
    ] {
        let mut incoming = before.clone();
        let row = &mut incoming["layers"]["tapping_poles"]["features"][0];
        match kind {
            "delete_layer" => {
                incoming["layers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("tapping_poles");
            }
            "delete_row" => incoming["layers"]["tapping_poles"]["features"] = json!([]),
            "geometry" => row["geometry"]["coordinates"][0] = json!(31),
            "property" => row["properties"]["assembly_type"] = json!("other"),
            "owner" => row["properties"]["transfo"] = json!("different-transformer"),
            "demote" => row["properties"]["drafting_status"] = json!("draft"),
            "duplicate" => {
                let mut duplicate = row.clone();
                duplicate["id"] = json!("new");
                duplicate["properties"]["doc_id"] = json!("tap");
                incoming["layers"]["tapping_poles"]["features"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            "malformed" => row["geometry"]["coordinates"] = json!([181, -2]),
            _ => {
                incoming["layers"]["lv_lines"]["features"][0]["geometry"] =
                    json!({"type":"Point","coordinates":[30,-2]})
            }
        }
        assert!(
            workspace
                .write(
                    &serde_json::to_vec(&incoming).unwrap(),
                    Some(&revision),
                    "retryable"
                )
                .is_err(),
            "{kind}"
        );
        let (after_revision, after) = workspace.read("T1", None).unwrap();
        assert_eq!(after_revision, revision, "{kind}");
        assert_eq!(serde_json::to_value(after).unwrap(), before, "{kind}");
        assert_eq!(
            workspace.outbox(0, 100).unwrap()["rows"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            workspace.revisions("T1", "", 100).unwrap()["revisions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    // Failed requests never reserve the operation ID; an admitted retry works.
    let mut incoming = before.clone();
    let mut added = incoming["layers"]["tapping_poles"]["features"][0].clone();
    added["id"] = json!("new");
    added["properties"]["drafting_status"] = json!("draft");
    incoming["layers"]["tapping_poles"]["features"]
        .as_array_mut()
        .unwrap()
        .push(added);
    let receipt = workspace
        .write(
            &serde_json::to_vec(&incoming).unwrap(),
            Some(&revision),
            "retryable",
        )
        .unwrap();
    assert_ne!(receipt["revision"], revision);
    let (_, after) = workspace.read("T1", None).unwrap();
    assert_eq!(
        after.layers["tapping_poles"]["features"][0],
        before["layers"]["tapping_poles"]["features"][0]
    );
    assert_eq!(
        after.layers["tapping_poles"]["features"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn deliberate_typed_edit_and_restore_remain_available_for_approved_rows() {
    let (_directory, mut workspace, revision, before) = setup();
    let edit = json!({"schema":"ds.design.edit/v1","transformer":"T1","expected_revision":revision,"operation_id":"explicit-edit",
        "mutations":[{"kind":"set_properties","layer":"tapping_poles","ids":["tap"],"values":{"assembly_type":"reviewed change"}}]});
    let receipt = workspace.edit(&serde_json::to_vec(&edit).unwrap()).unwrap();
    assert_eq!(
        workspace.read("T1", None).unwrap().1.layers["tapping_poles"]["features"][0]["properties"]
            ["assembly_type"],
        "reviewed change"
    );
    workspace
        .restore(
            "T1",
            &revision,
            receipt["revision"].as_str().unwrap(),
            "explicit-restore",
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(workspace.read("T1", None).unwrap().1).unwrap(),
        before
    );
}

#[test]
fn malformed_initial_import_never_creates_a_head_or_outbox() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("design");
    Workspace::init(&path, "project").unwrap();
    let mut workspace = Workspace::open(&path).unwrap();
    let mut bad = initial();
    bad["layers"]["lv_lines"]["features"][0]["geometry"] =
        json!({"type":"Point","coordinates":[30,-2]});
    assert!(workspace
        .write(&serde_json::to_vec(&bad).unwrap(), None, "bad")
        .is_err());
    assert!(workspace.read("T1", None).is_err());
    assert!(workspace.outbox(0, 100).unwrap()["rows"]
        .as_array()
        .unwrap()
        .is_empty());
}
