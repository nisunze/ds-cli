//! The engine-free project lifecycle: creation, seeding, edits, sender binding
//! and exact publication acknowledgement. Calculated runs are exercised where an
//! engine is supplied, by the local tooling (ds-solar-cli `local_engine` tests).
#[path = "../../ds-solar-reference-host/tests/common/mod.rs"]
mod common;

use ds_solar_contracts::SolarCityInput;
use ds_solar_project::{ProjectIdentity, Workspace};
use serde_json::json;
use std::collections::BTreeMap;

fn input() -> SolarCityInput {
    let mut city = common::city_input("aderm_bere");
    city.identity.project_id = "project-1".into();
    city.identity.root = "eds_project/project-1/eds_solar".into();
    city.sealed()
}
fn workspace(path: &std::path::Path) -> Workspace {
    Workspace::initialize(
        path,
        ProjectIdentity {
            project_id: "project-1".into(),
            root: "eds_project/project-1/eds_solar".into(),
        },
    )
    .unwrap()
}

#[test]
fn incomplete_creation_survives_restart_edits_and_exact_sync_acknowledgement() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace");
    let mut w = workspace(&path);
    let created = w.create_city("town", "Town", None).unwrap();
    assert_eq!(created["readiness"]["input_complete"], false);
    assert_eq!(
        w.create_city("town", "Other name", None).unwrap()["created"],
        false
    );
    assert_eq!(w.pending(20).unwrap().len(), 1);
    let mut form = w.network_form("town").unwrap();
    form["document"]["transformer_overrides"] = json!({"t1":{"name":"t1","kva":100,"Pauvre":80}});
    w.save_network_form(
        "town",
        form["expected"].as_str().unwrap(),
        form["document"].clone(),
        None,
    )
    .unwrap();
    let first = w.sync_next().unwrap();
    assert_eq!(first["jobs"][0]["expected_base"], "");
    let snapshot: serde_json::Value =
        serde_json::from_str(first["jobs"][0]["snapshot_json"].as_str().unwrap()).unwrap();
    assert!(snapshot["01_city_inputs"]["city"].get("latitude").is_none());
    let base = "b".repeat(64);
    w.sync_ack(first["sequence"].as_i64().unwrap(),first["digest"].as_str().unwrap(),&json!({"root":w.identity.root,"city_id":"town","client_snapshot_sha256":first["jobs"][0]["fingerprint"],"input_base_fingerprint":base})).unwrap();
    drop(w);
    let reopened = Workspace::open(&path).unwrap();
    let second = reopened.sync_next().unwrap();
    assert_eq!(second["jobs"][0]["expected_base"], base);
    assert_eq!(
        reopened.network_form("town").unwrap()["document"]["transformer_overrides"]["t1"]["Pauvre"],
        80
    );
    assert_eq!(reopened.pending(20).unwrap().len(), 1);
}

#[test]
fn seed_conflict_rolls_back_every_city_and_upload_intent() {
    let tmp = tempfile::tempdir().unwrap();
    let mut w = workspace(&tmp.path().join("workspace"));
    let a = input();
    w.seed(std::slice::from_ref(&a), &BTreeMap::new()).unwrap();
    let mut changed = a.clone();
    changed.identity.display_name = "Edited Bere".into();
    changed = changed.sealed();
    let mut other = a.clone();
    other.identity.city_id = "another_city".into();
    other.run_options.demand_noise_seed = None;
    other = other.sealed();
    assert!(w.seed(&[other, changed.clone()], &BTreeMap::new()).is_err());
    assert_eq!(w.cities(&[]).unwrap(), vec![a.clone()]);
    assert_eq!(w.pending(20).unwrap().len(), 1);
    w.seed(
        &[changed],
        &BTreeMap::from([(a.identity.city_id.clone(), a.content_digest.clone())]),
    )
    .unwrap();
    assert_eq!(w.pending(20).unwrap().len(), 2);
}

#[test]
fn acknowledgement_preserves_new_edits_and_reverting_is_a_new_upload() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace");
    let mut w = workspace(&path);
    let a = input();
    w.seed(std::slice::from_ref(&a), &BTreeMap::new()).unwrap();
    let original = w.pending(20).unwrap().remove(0);
    let mut b = a.clone();
    b.identity.display_name = "Edited".into();
    b = b.sealed();
    w.seed(
        std::slice::from_ref(&b),
        &BTreeMap::from([(a.identity.city_id.clone(), a.content_digest.clone())]),
    )
    .unwrap();
    w.acknowledge(&original, &json!({"published":true}))
        .unwrap();
    assert_eq!(w.pending(20).unwrap()[0].digest, b.content_digest);
    w.seed(
        std::slice::from_ref(&a),
        &BTreeMap::from([(b.identity.city_id.clone(), b.content_digest.clone())]),
    )
    .unwrap();
    drop(w);
    let reopened = Workspace::open(&path).unwrap();
    let pending = reopened.pending(20).unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[1].digest, a.content_digest);
    assert_eq!(reopened.cities(&[]).unwrap(), vec![a]);
}

#[test]
fn sender_cannot_switch_principal_or_project() {
    let tmp = tempfile::tempdir().unwrap();
    let mut w = workspace(&tmp.path().join("workspace"));
    w.bind_sender("project-1", "stable", &"a".repeat(64), &"b".repeat(64))
        .unwrap();
    assert!(
        w.bind_sender("project-1", "stable", &"c".repeat(64), &"b".repeat(64))
            .is_err()
    );
    assert!(
        w.bind_sender("project-2", "stable", &"a".repeat(64), &"b".repeat(64))
            .is_err()
    );
}

#[test]
fn independent_network_survives_restart_missing_source_and_manual_refresh() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace");
    let mut w = workspace(&path);
    w.create_city("town", "Town", None).unwrap();
    let held = w.network_form("town").unwrap();
    let mut form = held["document"].clone();
    form["seeded"] =
        json!({"proposed":{"transformers":[{"name":"t1","phase":"I","kva":100,"Clinic":2}]}});
    let saved = w
        .save_network_form("town", held["expected"].as_str().unwrap(), form, None)
        .unwrap();
    assert_eq!(saved["values"]["proposed"]["transformers"][0]["kva"], 100);
    drop(w);
    let mut w = Workspace::open(&path).unwrap();
    let held = w.network_form("town").unwrap();
    let mut form = held["document"].clone();
    form["transformer_overrides"] = json!({"t1":{"kva":160,"Clinic":0}});
    // Explicit refresh changes source fields but must preserve manual zeroes.
    form["seeded"]["proposed"]["transformers"][0]["kva"] = json!(200);
    form["seeded"]["proposed"]["transformers"][0]["Clinic"] = json!(9);
    let saved = w
        .save_network_form("town", held["expected"].as_str().unwrap(), form, None)
        .unwrap();
    assert_eq!(saved["values"]["proposed"]["transformers"][0]["kva"], 160);
    assert_eq!(saved["values"]["proposed"]["transformers"][0]["Clinic"], 0);
    assert!(
        w.network_sources(&["town".into()]).unwrap()["sources"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    // Generic UI/CLI edits to inline fields are recognized as manual changes.
    let held = w.network_form("town").unwrap();
    assert_eq!(
        held["document"]["seeded"]["proposed"]["transformers"][0]["kva"],
        200
    );
}
