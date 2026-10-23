use super::*;
use ds_client_core::TransformerDigestReseal;
use ds_command_kernel::report_export::jcs::{layers_content_digest, sha256};
use serde_json::Value;
use std::collections::BTreeMap;

fn reseal_layers() -> BTreeMap<String, Value> {
    serde_json::from_value(json!({"lv_poles":{"type":"FeatureCollection","features":[
        {"type":"Feature","id":"14","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"assembly_type":""}}]}}))
    .unwrap()
}
fn status_body(version: u64, digest: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"transformers":[{"name":"t1","metadata":{"version":version,"content_digest":digest}}]})).unwrap()
}
fn context_body(layers: &BTreeMap<String, Value>, version: u64) -> Vec<u8> {
    serde_json::to_vec(&json!({"success":true,"data":{"total":1,"found_count":1,"failed_count":0,"results":[{"transformer_name":"t1","ok":true,"data":{"layers":layers,"metadata":{"version":version,"content_digest":layers_content_digest(layers).unwrap()}}}]}})).unwrap()
}
fn request(confirmed: bool, version: Option<u64>, stored: Option<&str>) -> TransformerDigestReseal {
    TransformerDigestReseal {
        operation_id: "reseal-one".into(),
        transformer: "t1".into(),
        confirmed,
        expect_version: version,
        expect_stored_digest: stored.map(str::to_owned),
    }
}

#[test]
fn reseal_plans_from_two_reads_and_writes_only_the_fenced_stored_digest() {
    let layers = reseal_layers();
    let fresh = layers_content_digest(&layers).unwrap();
    let stale = "2".repeat(64);
    let receipt = json!({"success":true,"data":{"operation_id":"reseal-one","transformer_name":"t1","version":5,
        "previous_content_digest":stale,"content_digest":fresh,"layers_changed":false,"resealed_at":1}});
    let run = |input: &TransformerDigestReseal, after: Option<&str>, save: Option<(u16, Value)>| {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        transport.push_project_report(200, &status_body(5, &stale));
        transport.push_transformer_context(200, &context_body(&layers, 5));
        if let Some((status, body)) = save {
            transport
                .0
                .lock()
                .unwrap()
                .transformer_saves
                .push_back(TransportResponse::new(
                    status,
                    serde_json::to_vec(&body).unwrap(),
                ));
        }
        if let Some(after) = after {
            transport.push_project_report(200, &status_body(5, after));
        }
        let result = client.reseal_transformer_digest("project_a", input, NOW + 1);
        let bodies = transport.0.lock().unwrap().transformer_save_bodies.clone();
        (result, bodies, transport)
    };

    // Without confirmation it reads the stored row and the saved room, then plans.
    let (plan, bodies, _) = run(&request(false, None, None), None, None);
    let plan = plan.unwrap();
    assert_eq!(plan["written"], false);
    assert_eq!(plan["plan"]["stored_content_digest"], stale);
    assert_eq!(plan["plan"]["layers_content_digest"], fresh);
    assert_eq!(plan["plan"]["version"], 5);
    assert!(bodies.is_empty());

    // Confirmation binds to the reviewed version and stale digest.
    let (moved, bodies, _) = run(&request(true, Some(5), Some(&"3".repeat(64))), None, None);
    assert_eq!(
        moved.unwrap_err().to_string(),
        "repair_reseal_expectation_changed"
    );
    assert!(bodies.is_empty());
    let (moved, bodies, _) = run(&request(true, Some(4), Some(&stale)), None, None);
    assert_eq!(
        moved.unwrap_err().to_string(),
        "repair_reseal_expectation_changed"
    );
    assert!(bodies.is_empty());

    // The write carries no layer, only the four fences, and is verified by a re-read.
    let (sealed, bodies, _) = run(
        &request(true, Some(5), Some(&stale)),
        Some(&fresh),
        Some((200, receipt.clone())),
    );
    let sealed = sealed.unwrap();
    assert_eq!(sealed["verified"], true);
    assert_eq!(sealed["receipt"]["previous_content_digest"], stale);
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["action"], "reseal_transformer_content_digest");
    assert_eq!(
        bodies[0]["reseal"],
        json!({"operation_id":"reseal-one","source_version":5,"stored_content_digest":stale,"layers_content_digest":fresh})
    );
    for forbidden in [
        "gdfs",
        "repair",
        "content_digest",
        "base_version",
        "process_metadata",
    ] {
        assert!(bodies[0].get(forbidden).is_none(), "{forbidden}");
    }

    // A stored digest that did not change on readback is not a verified reseal.
    let (unverified, _, _) = run(
        &request(true, Some(5), Some(&stale)),
        Some(&stale),
        Some((200, receipt.clone())),
    );
    assert_eq!(
        unverified.unwrap_err().to_string(),
        "repair_readback_unverified"
    );

    // The server's own fenced refusal crosses with its code and no re-read.
    let refusal = json!({"success":false,"error":{"code":"TRANSFORMER_RESEAL_SOURCE_MOVED","message":"moved"}});
    let (refused, _, transport) = run(
        &request(true, Some(5), Some(&stale)),
        None,
        Some((409, refusal)),
    );
    let error = refused.unwrap_err();
    assert_eq!(
        error.service_refusal().unwrap().code(),
        Some("transformer_reseal_source_moved")
    );
    assert_eq!(transport.count_calls("project_report "), 1);

    // An agreeing head refuses before any write, with or without confirmation.
    for confirmed in [false, true] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        transport.push_project_report(200, &status_body(5, &fresh));
        transport.push_transformer_context(200, &context_body(&layers, 5));
        let input = request(confirmed, Some(5), Some(&fresh));
        let error = client
            .reseal_transformer_digest("project_a", &input, NOW + 1)
            .unwrap_err();
        assert_eq!(error.to_string(), "repair_reseal_not_needed");
        assert!(transport
            .0
            .lock()
            .unwrap()
            .transformer_save_bodies
            .is_empty());
    }

    // A confirmed reseal without the reviewed fences is refused before any read.
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let error = client
        .reseal_transformer_digest("project_a", &request(true, None, None), NOW + 1)
        .unwrap_err();
    assert_eq!(error.to_string(), "repair_reseal_expectation_required");
    assert_eq!(transport.count_calls("project_report "), 0);
}

#[test]
fn renumber_publication_pins_the_reviewed_repeat_by_its_canonical_feature_digest() {
    use ds_command_kernel::{
        design::{self, Snapshot},
        design_repair::{self, ApplySelection, Proposal},
    };
    let target:Snapshot=serde_json::from_value(json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326",
      "layers":{"lv_poles":{"type":"FeatureCollection","features":[
        {"type":"Feature","id":"14","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"struct_type":"S140"}},
        {"type":"Feature","id":"14","geometry":{"type":"Point","coordinates":[30.1,-2.0]},"properties":{"struct_type":"S190"}}]}},
      "settings":{},"network_config":{"sheets":{}},"sources":[],"include_design_customers":true})).unwrap();
    let second = &target.layers["lv_poles"]["features"][1];
    let scope = json!({"project":"explicit-project","principal":"uid-1","lane":"stable","audience":profile(DeploymentLane::Stable).credential_audience_sha256()});
    let source_digest = layers_content_digest(&target.layers).unwrap();
    let proposal:Proposal=serde_json::from_value(design_repair::evaluate("propose",json!({"target":target,"request":{
       "scope":scope,"target":{"snapshot_revision":design::digest(&target).unwrap(),"server_version":3,"layers_content_digest":source_digest},
       "policy":{"scope":scope,"fields":{"lv_poles":{"id":{"field_type":"string"}}}},"allowlist":{"lv_poles":["id"]},
       "operations":[{"kind":"renumber","target_layer":"lv_poles","target_id":"14","before_feature_digest":design::digest(second).unwrap(),"new_id":"99"}]
    }})).unwrap()).unwrap();
    let selection:ApplySelection=serde_json::from_value(json!({"scope":scope,"proposal_digest":proposal.digest,"change_ids":proposal.changes.iter().map(|c|c.id.clone()).collect::<Vec<_>>(),"confirmed":true})).unwrap();
    let projected = design_repair::apply_selected(None, &target, &proposal, &selection)
        .unwrap()
        .snapshot;
    let expected = layers_content_digest(&projected.layers).unwrap();
    let publication = ds_client_core::TransformerRepairPublication {
        operation_id: "renumber-one".into(),
        source: None,
        target: target.clone(),
        proposal,
        selection,
    };
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    transport.push_transformer_context(200, &serde_json::to_vec(&json!({"success":true,"data":{"total":1,"found_count":1,"failed_count":0,"results":[{"transformer_name":"T1","ok":true,"data":{"layers":projected.layers,"metadata":{"version":4,"content_digest":expected}}}]}})).unwrap());
    transport.0.lock().unwrap().transformer_saves.push_back(TransportResponse::new(200, serde_json::to_vec(&json!({"success":true,"data":{"operation_id":"renumber-one","transformer_name":"T1","source_version":3,"source_content_digest":source_digest,"version":4,"content_digest":expected,"changed_cells":1}})).unwrap()));
    let result = client
        .repair_transformer("explicit-project", &publication, NOW + 1)
        .unwrap();
    assert_eq!(result["verified"], true);
    let state = transport.0.lock().unwrap();
    let change = &state.transformer_save_bodies[0]["repair"]["changes"][0];
    assert_eq!(change["identity"], json!({"field":"id","value":"14"}));
    assert_eq!(change["property"], "id");
    assert_eq!(change["after"], json!({"kind":"value","value":"99"}));
    assert_eq!(change["feature_digest"], sha256(second).unwrap());
    assert_eq!(projected.layers["lv_poles"]["features"][0]["id"], "14");
    assert_eq!(projected.layers["lv_poles"]["features"][1]["id"], "99");
}

/// The same vector ds-brain asserts for its ContentDigest of a feature: the
/// renumber pin is computed identically on both sides of the wire.
#[test]
fn renumber_feature_pin_matches_the_server_canonical_digest() {
    let feature: Value = serde_json::from_str(
        r#"{"type":"Feature","id":"14","geometry":{"type":"Point","coordinates":[30.1,-2.0,1e-7]},"properties":{"struct_type":"S190","n":100,"x":0.000001,"t":"é "}}"#,
    )
    .unwrap();
    assert_eq!(
        sha256(&feature).unwrap(),
        "5ace0daf2cfd5eb6510687ae2704ef76712f0eba2660602cecad0d562dae6728"
    );
}
