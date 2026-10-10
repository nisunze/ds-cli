use ds_command_kernel::{
    design::{self, Snapshot},
    design_repair::{self, ApplySelection, Proposal},
};
use ds_design_workspace::{Error, Workspace};
use serde_json::{Value, json};

fn setup() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace");
    Workspace::init(&path, "test-project").unwrap();
    let mut w = Workspace::open(&path).unwrap();
    let document = json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326",
      "layers":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"tr_size":100,"drafting_status":"approved"}}]},
        "lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","id":"pole","geometry":{"type":"Point","coordinates":[30.001,-2.0]},"properties":{"struct_type":"old","drafting_status":"approved","stay":1,"assembly_type":"terminal"}}]},
        "lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.001,-2.0]]},"properties":{}}]}},
      "settings":{},"network_config":{"sheets":{}},"sources":[],"include_design_customers":true});
    w.write(&serde_json::to_vec(&document).unwrap(), None, "create")
        .unwrap();
    (temp, w)
}
fn request(target: &Snapshot, operation: &str) -> Value {
    let scope = json!({"project":"test-project","principal":"test@example.com","lane":"test","audience":"test"});
    let proposal:Proposal=serde_json::from_value(design_repair::evaluate("propose",json!({"target":target,"request":{
       "scope":scope,"target":{"snapshot_revision":design::digest(target).unwrap()},
       "policy":{"scope":scope,"fields":{"lv_poles":{"struct_type":{"field_type":"string","allowed_values":["old","new"]}}}},
       "allowlist":{"lv_poles":["struct_type"]},"operations":[{"kind":"assign","target_layer":"lv_poles","target_ids":["pole"],"values":{"struct_type":{"kind":"value","value":"new"}}}]
    }})).unwrap()).unwrap();
    let selection:ApplySelection=serde_json::from_value(json!({"scope":scope,"proposal_digest":proposal.digest,"change_ids":proposal.changes.iter().map(|c|c.id.clone()).collect::<Vec<_>>(),"confirmed":true})).unwrap();
    json!({"schema":"ds.design.repair/v1","transformer":"T1","operation_id":operation,"proposal":proposal,"selection":selection})
}
#[test]
fn repair_is_atomic_fenced_replayable_and_preserves_approval_and_history() {
    let (_temp, mut w) = setup();
    let (before_revision, before) = w.read("T1", None).unwrap();
    let input = request(&before, "repair1");
    let bytes = serde_json::to_vec(&input).unwrap();
    let receipt = w.repair(&bytes).unwrap();
    assert_eq!(receipt["publication"], "pending_transport");
    assert_eq!(w.repair(&bytes).unwrap(), receipt);
    let (_, after) = w.read("T1", None).unwrap();
    let mut expected = serde_json::to_value(&before).unwrap();
    expected["layers"]["lv_poles"]["features"][0]["properties"]["struct_type"] = json!("new");
    assert_eq!(serde_json::to_value(&after).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(w.read("T1", Some(&before_revision)).unwrap().1).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    let mut stale = input.clone();
    stale["operation_id"] = json!("stale");
    assert!(matches!(
        w.repair(&serde_json::to_vec(&stale).unwrap()),
        Err(Error::Conflict(_))
    ));
    let mut altered = input;
    altered["selection"]["change_ids"] = json!([]);
    assert!(matches!(
        w.repair(&serde_json::to_vec(&altered).unwrap()),
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        w.outbox(0, 100).unwrap()["rows"].as_array().unwrap().len(),
        2
    );
    assert_eq!(w.status().unwrap()["pending_jobs"], 0);
}
#[test]
fn invalid_selection_rolls_back_without_an_outbox_entry() {
    let (_temp, mut w) = setup();
    let (revision, target) = w.read("T1", None).unwrap();
    let mut input = request(&target, "invalid");
    input["selection"]["change_ids"] = json!(["absent"]);
    assert!(w.repair(&serde_json::to_vec(&input).unwrap()).is_err());
    assert_eq!(w.read("T1", None).unwrap().0, revision);
    assert_eq!(
        w.outbox(0, 100).unwrap()["rows"].as_array().unwrap().len(),
        1
    );
}
