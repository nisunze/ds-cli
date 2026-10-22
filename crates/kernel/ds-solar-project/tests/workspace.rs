use ds_solar_reference_host::{FixtureReferenceProvider, ReferencePreparer};
#[path = "../../ds-solar-cli/src/fixtures.rs"]
#[allow(dead_code)]
mod fixtures;

use ds_solar_contracts::{ReferencePin, SolarCityInput};
use ds_solar_project::{ProjectIdentity, Workspace, run::RunRequest};
use ds_solar_runtime::DirectoryReferenceCache;
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};

fn fixture() -> fixtures::FixtureSet {
    fixtures::FixtureSet::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures"),
    )
}
fn input() -> SolarCityInput {
    fixture()
        .city_input("aderm_bere", "project-1", "eds_project/project-1/eds_solar")
        .unwrap()
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
fn headless_portfolio_verifies_local_sources_without_publication() {
    use ds_solar_contracts::{PortfolioIdentity, PortfolioRepresentativeStrategy};
    use ds_solar_project::portfolio::{self, Request};
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace");
    let mut w = workspace(&path);
    let inputs: Vec<_> = ["aderm_beinamar", "aderm_bere", "aderm_gagal"]
        .into_iter()
        .map(|city| {
            fixture()
                .city_input(city, "project-1", "eds_project/project-1/eds_solar")
                .unwrap()
        })
        .collect();
    let cache = DirectoryReferenceCache::new(tmp.path().join("references"));
    let provider = FixtureReferenceProvider::new(
        inputs
            .iter()
            .map(|i| fixture().reference_bundle(&i.identity.city_id).unwrap())
            .collect(),
        ReferencePin::current(),
    )
    .unwrap();
    for i in &inputs {
        ReferencePreparer::new()
            .with_cache(&cache)
            .with_provider(&provider)
            .prepare(i.clone(), "2026-09-16T00:00:00Z")
            .unwrap();
    }
    w.seed(&inputs, &BTreeMap::new()).unwrap();
    let cities: Vec<_> = inputs.iter().map(|i| i.identity.city_id.clone()).collect();
    let run = w
        .execute(
            &RunRequest {
                run_id: "source".into(),
                cities: cities.clone(),
                concurrency: 2,
                charts: false,
                drafts: vec!["apd".into()],
            },
            &tmp.path().join("references"),
        )
        .unwrap();
    assert_eq!(run["status"], "succeeded", "{run}");
    let source = ds_solar_project::dashboard::source(
        &path.join("runs/source"),
        "project-1",
        "source",
        "aderm_bere",
    )
    .unwrap();
    assert_eq!(source["report"]["_meta"]["city_id"], "aderm_bere");
    assert!(
        ds_solar_project::dashboard::source(
            &path.join("runs/source"),
            "project-2",
            "source",
            "aderm_bere"
        )
        .is_err()
    );
    assert!(
        ds_solar_project::dashboard::source(
            &path.join("runs/source"),
            "project-1",
            "other",
            "aderm_bere"
        )
        .is_err()
    );
    assert!(
        ds_solar_project::dashboard::source(
            &path.join("runs/source"),
            "project-1",
            "source",
            "other_city"
        )
        .is_err()
    );
    let request = |out, cities| Request {
        source: path.join("runs/source"),
        source_run_id: "source".into(),
        out,
        cities,
        run_id: "aggregate".into(),
        identity: PortfolioIdentity {
            project_id: "project-1".into(),
            root: "eds_project/project-1/eds_solar".into(),
            portfolio_id: "pf_1".into(),
            portfolio_name: "Two cities".into(),
            membership_revision: format!("sha256:{}", "a".repeat(64)),
        },
        strategy: PortfolioRepresentativeStrategy::First,
    };
    let source_closure = std::fs::read(path.join("runs/source/batch.json")).unwrap();
    let cities = cities[..2].to_vec();
    let out = tmp.path().join("portfolio");
    let receipt = portfolio::calculate(request(out.clone(), cities.clone())).unwrap();
    assert_eq!(receipt["city_count"], 2);
    assert_eq!(receipt["publication"], "not_requested");
    let sealed = std::fs::read(out.join(ds_solar_runtime::PORTFOLIO_RESULT_NAME)).unwrap();
    let batch: ds_solar_contracts::SolarArtifactBatch =
        serde_json::from_slice(&std::fs::read(out.join("batch.json")).unwrap()).unwrap();
    let chart = batch
        .artifacts
        .iter()
        .find(|entry| entry.media_type == "image/png")
        .unwrap();
    let chart_path = out.join(&chart.name);
    let original_chart = std::fs::read(&chart_path).unwrap();
    std::fs::write(&chart_path, b"corrupt closed chart").unwrap();
    let revision = format!("sha256:{}", "a".repeat(64));
    assert!(
        ds_solar_project::dashboard::portfolio_source(
            &out,
            "project-1",
            "aggregate",
            "pf_1",
            &revision,
        )
        .is_err()
    );
    std::fs::write(chart_path, original_chart).unwrap();
    let revision = format!("sha256:{}", "a".repeat(64));
    let dashboard = ds_solar_project::dashboard::portfolio_source(
        &out,
        "project-1",
        "aggregate",
        "pf_1",
        &revision,
    )
    .unwrap();
    assert_eq!(dashboard["city_ids"], json!(cities));
    assert_eq!(
        dashboard["report_digest"],
        ds_solar_contracts::digest::sha256_hex(&sealed)
    );
    for (project, run, portfolio, pinned) in [
        ("project-2", "aggregate", "pf_1", revision.as_str()),
        ("project-1", "other", "pf_1", revision.as_str()),
        ("project-1", "aggregate", "other", revision.as_str()),
        ("project-1", "aggregate", "pf_1", "other-revision"),
    ] {
        assert!(
            ds_solar_project::dashboard::portfolio_source(&out, project, run, portfolio, pinned)
                .is_err()
        );
    }
    assert!(portfolio::calculate(request(out.clone(), cities.clone())).is_err());
    let mut reversed = cities.clone();
    reversed.reverse();
    let reordered = tmp.path().join("reordered");
    portfolio::calculate(request(reordered.clone(), reversed.clone())).unwrap();
    let reordered_result: serde_json::Value = serde_json::from_slice(
        &std::fs::read(reordered.join(ds_solar_runtime::PORTFOLIO_RESULT_NAME)).unwrap(),
    )
    .unwrap();
    assert_eq!(reordered_result["cities"][0]["city_id"], reversed[0]);
    for invalid in [
        vec![cities[0].clone()],
        vec![cities[0].clone(), cities[0].clone()],
        vec![cities[0].clone(), "foreign_city".into()],
    ] {
        assert!(portfolio::calculate(request(tmp.path().join("invalid"), invalid)).is_err());
        assert!(!tmp.path().join("invalid").exists());
    }
    assert_eq!(
        std::fs::read(path.join("runs/source/batch.json")).unwrap(),
        source_closure
    );
    std::fs::write(out.join(ds_solar_runtime::PORTFOLIO_RESULT_NAME), b"{}").unwrap();
    assert!(
        ds_solar_project::dashboard::portfolio_source(
            &out,
            "project-1",
            "aggregate",
            "pf_1",
            &revision,
        )
        .is_err()
    );
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
fn copied_classification_runs_offline_with_owned_map_and_portfolio_summary() {
    use base64::Engine;
    use ds_solar_contracts::network_form::{self, AssetReference, ResolvedInformation};
    let tmp = tempfile::tempdir().unwrap();
    let mut a = input();
    let mut manual = a.seeded.city_inputs["network_data"].clone();
    let transformers = manual["proposed"]
        .as_object_mut()
        .unwrap()
        .remove("transformers")
        .unwrap();
    manual
        .as_object_mut()
        .unwrap()
        .remove("material_quantities");
    manual.as_object_mut().unwrap().remove("maps");
    let mut rows = Vec::new();
    let mut metadata = Vec::new();
    let mut phase_overrides = serde_json::Map::new();
    for transformer in transformers.as_array().unwrap() {
        let name = transformer["name"].as_str().unwrap();
        metadata.push(json!({"name":name,"capacity_kva":transformer["kva"]}));
        phase_overrides.insert(name.into(), json!({"phase":transformer["phase"]}));
        rows.push(json!({"section":"lv_lines_info_df","description":"classified LV","unit":"m","quantity":transformer["bt_km"].as_f64().unwrap()*1000.,"transformer":name}));
        for (category, quantity) in transformer.as_object().unwrap() {
            if matches!(
                category.as_str(),
                "name" | "phase" | "kva" | "bt_km" | "clients"
            ) {
                continue;
            }
            rows.push(json!({"section":"customers_categories_info_df","description":category,"unit":"pce","quantity":quantity,"transformer":name}));
        }
    }
    let bytes = serde_json::to_vec(
        &json!({"schema":"ds.network-information/v1","rows":rows,"transformers":metadata}),
    )
    .unwrap();
    let reference = AssetReference {
        project_id: a.identity.project_id.clone(),
        asset_id: "a_000000000001".into(),
        digest: format!("sha256:{}", ds_solar_contracts::digest::hex_sha256(&bytes)),
    };
    let information = ResolvedInformation {
        reference: reference.clone(),
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    };
    let map_reference = AssetReference {
        project_id: a.identity.project_id.clone(),
        asset_id: "a_000000000002".into(),
        digest: format!("sha256:{}", "b".repeat(64)),
    };
    let transformer_name = transformers[0]["name"].as_str().unwrap();
    let document = json!({"seeded":{"maps":{"reseau_propose":{"bucket":"solar","object":"eds_project/project-1/eds_solar/aderm_bere/network_plots/_immutable/map","generation":1}}},"schema":network_form::SCHEMA,"city_tag":{"definition_id":"city","value":"Bere"},"sources":{"information":reference,"city_map":map_reference,"transformer_maps":{transformer_name:map_reference}},"overrides":manual,"transformer_overrides":phase_overrides});
    a.seeded.city_inputs["network_data"] = document.clone();
    a = a.sealed();
    let resolved = network_form::prepare_input(a.clone(), Some(&information)).unwrap();
    assert_eq!(
        resolved.seeded.city_inputs["network_data"]["proposed"]["transformers"]
            .as_array()
            .unwrap()
            .len(),
        transformers.as_array().unwrap().len()
    );
    let serialized = serde_json::to_value(&resolved).unwrap();
    assert_eq!(
        serialized["seeded"]["city_inputs"]["network_data"],
        document
    );
    assert!(
        serialized["seeded"]["city_inputs"]["network_data"]["overrides"]["proposed"]
            .get("transformers")
            .is_none()
    );
    let reopened: SolarCityInput = serde_json::from_value(serialized).unwrap();
    reopened.validate().unwrap();
    assert_eq!(reopened.content_digest, resolved.content_digest);
    let fallback = network_form::prepare_input(a.clone(), None).unwrap();
    assert_ne!(fallback.content_digest, resolved.content_digest);
    let cache = DirectoryReferenceCache::new(tmp.path().join("references"));
    let provider = FixtureReferenceProvider::new(
        vec![fixture().reference_bundle("aderm_bere").unwrap()],
        ReferencePin::current(),
    )
    .unwrap();
    ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(resolved, "2026-09-05T00:00:00Z")
        .unwrap();
    let path = tmp.path().join("workspace");
    let mut w = workspace(&path);
    w.seed(std::slice::from_ref(&a), &BTreeMap::new()).unwrap();
    let request = RunRequest {
        run_id: "shared-run".into(),
        cities: vec![a.identity.city_id.clone()],
        concurrency: 1,
        charts: false,
        drafts: vec!["apd".into()],
    };
    let saved = w
        .save_network_form(
            &a.identity.city_id,
            &a.content_digest,
            document.clone(),
            Some(&information),
        )
        .unwrap();
    assert!(saved["document"]["seeded"]["proposed"]["transformers"].is_array());
    let map_bytes = b"\x89PNG\r\n\x1a\nfixture-independent-map";
    let hash = ds_solar_contracts::digest::hex_sha256(map_bytes);
    let media = json!({"schema":1,"kind":"solar_network_media","bucket":"solar","object":"eds_project/project-1/eds_solar/aderm_bere/network_plots/_immutable/map","generation":1,"sha256":hash,"size_bytes":map_bytes.len(),"media_type":"image/png","logical_role":"network.map:reseau_propose"});
    assert!(w.cache_network_media(&media, b"corrupt").is_err());
    let map_path = tmp.path().join("map.png");
    std::fs::write(&map_path, map_bytes).unwrap();
    let saved = w
        .save_network_map(
            &a.identity.city_id,
            saved["expected"].as_str().unwrap(),
            media,
            Some(&map_path),
        )
        .unwrap();
    std::fs::remove_file(&map_path).unwrap();
    drop(w);
    let mut w = Workspace::open(&path).unwrap();
    // No resolver or network client is available to this run.
    let receipt = w.execute(&request, &tmp.path().join("references")).unwrap();
    assert_eq!(receipt["status"], "succeeded", "{receipt}");
    let draft =
        std::fs::read_to_string(path.join("runs/shared-run/cities/aderm_bere/apd-draft-fr.md"))
            .unwrap();
    assert!(draft.contains(&format!("maps/{hash}.png")));
    assert!(!draft.contains("gs://solar/"));
    let exported = path.join(format!("runs/shared-run/cities/aderm_bere/maps/{hash}.png"));
    assert_eq!(std::fs::read(exported).unwrap(), map_bytes);
    assert!(receipt["artifacts"].as_array().unwrap().iter().any(|v| {
        v["role"] == "export"
            && v["name"]
                .as_str()
                .unwrap()
                .ends_with(&format!("{hash}.png"))
    }));
    while w.sync_next_scope(true).unwrap()["pending"] == true {
        let next = w.sync_next_scope(true).unwrap();
        let ack = json!({"root":w.identity.root,"city_id":a.identity.city_id,"client_snapshot_sha256":next["jobs"][0]["fingerprint"],"input_base_fingerprint":"a".repeat(64)});
        w.sync_ack_scope(
            next["sequence"].as_i64().unwrap(),
            next["digest"].as_str().unwrap(),
            &ack,
            true,
        )
        .unwrap();
    }
    assert_eq!(w.sync_next().unwrap()["pending"], false);
    assert_eq!(receipt["publication"], "local_only");
    assert!(!draft.contains("ds-asset://"));
    assert!(!path.join("runs/shared-run/network_plots").exists());
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(path.join("runs/shared-run/cities/aderm_bere/report_input.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        report["01_city_inputs"]["network_data"]["proposed"]["transformers"],
        saved["values"]["proposed"]["transformers"]
    );
    let summary = &report["02_site_prep"]["network_summary"]["active_summary"];
    assert!(summary["n_total_transformers"].as_u64().unwrap() > 0);
    assert!(summary.get("active_transformers").is_none());
    let portfolio = ds_solar_portfolio::compute_network(
        &[ds_solar_portfolio::CityProjection {
            city_id: a.identity.city_id.clone(),
            data: report.clone(),
        }],
        "XAF",
    )
    .unwrap();
    assert!(portfolio.is_object());
    assert_eq!(
        w.cities(&[]).unwrap()[0].seeded.city_inputs["network_data"]["_solar_seed"],
        saved["document"]
    );
}

#[test]
fn failed_manual_city_keeps_its_diagnostic_and_never_queues_an_empty_report() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace");
    let mut w = workspace(&path);
    let a = input();
    let cache_path = tmp.path().join("references");
    let cache = DirectoryReferenceCache::new(&cache_path);
    let provider = FixtureReferenceProvider::new(
        vec![fixture().reference_bundle("aderm_bere").unwrap()],
        ReferencePin::current(),
    )
    .unwrap();
    ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(a.clone(), "2026-09-05T00:00:00Z")
        .unwrap();
    let mut a = a;
    a.seeded.city_inputs["network_data"] = json!({"schema":ds_solar_contracts::network_form::SCHEMA,"sources":{},"overrides":{"minimum_load_profile":0.35,"consider_inst":false,"proposed":{"transformers":[{"name":"T1","phase":"I","kva":100,"bt_km":2,"qty":1}]}}});
    a = a.sealed();
    w.seed(std::slice::from_ref(&a), &BTreeMap::new()).unwrap();
    let request = RunRequest {
        run_id: "manual-invalid".into(),
        cities: vec![a.identity.city_id],
        concurrency: 1,
        charts: false,
        drafts: vec!["network".into()],
    };
    let receipt = w.execute(&request, &cache_path).unwrap();
    assert_eq!(receipt["status"], "failed");
    assert_eq!(receipt["publication"], "not_applicable");
    assert_eq!(receipt["failures"][0]["error"]["code"], "invalid_input");
    assert!(
        receipt["failures"][0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("customer-category columns")
    );
    assert_eq!(w.pending(20).unwrap().len(), 1);
    drop(w);
    assert_eq!(
        Workspace::open(&path)
            .unwrap()
            .run_result("manual-invalid")
            .unwrap(),
        receipt
    );
}

#[test]
fn offline_seed_to_draft_survives_restart_and_reuses_captured_run() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace");
    let mut w = workspace(&path);
    let mut a = input();
    // No tag, geometry, sizing asset or maps: this is a manually designed city.
    let manual = a.seeded.city_inputs["network_data"].clone();
    a.seeded.city_inputs["network_data"] = json!({
        "schema":ds_solar_contracts::network_form::SCHEMA,
        "sources":{}, "overrides":manual,
    });
    a = a.sealed();
    let cache = DirectoryReferenceCache::new(tmp.path().join("references"));
    let provider = FixtureReferenceProvider::new(
        vec![fixture().reference_bundle("aderm_bere").unwrap()],
        ReferencePin::current(),
    )
    .unwrap();
    ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(a.clone(), "2026-09-05T00:00:00Z")
        .unwrap();
    w.seed(std::slice::from_ref(&a), &BTreeMap::new()).unwrap();
    let request = RunRequest {
        run_id: "offline-run".into(),
        cities: vec![a.identity.city_id.clone()],
        concurrency: 1,
        charts: false,
        drafts: vec!["apd".into()],
    };
    let receipt = w.execute(&request, &tmp.path().join("references")).unwrap();
    assert_eq!(receipt["status"], "succeeded", "{receipt}");
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(path.join("runs/offline-run/cities/aderm_bere/report_input.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        report["01_city_inputs"]["network_data"],
        a.seeded.city_inputs["network_data"]
    );
    assert_eq!(
        report["01_city_inputs"]["network_data"]["sources"],
        json!({})
    );
    assert!(
        path.join("runs/offline-run/cities/aderm_bere/apd-draft-fr.md")
            .is_file()
    );
    assert_eq!(
        w.pending(20)
            .unwrap()
            .iter()
            .map(|r| r.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["city"]
    );
    let next = w.sync_next().unwrap();
    let row = w.pending(1).unwrap().remove(0);
    let ack = json!({"root":w.identity.root,"city_id":a.identity.city_id,"client_snapshot_sha256":next["jobs"][0]["fingerprint"],"input_base_fingerprint":"a".repeat(64)});
    assert!(w.sync_ack(row.sequence, &"b".repeat(64), &ack).is_err());
    w.sync_ack(row.sequence, &row.digest, &ack).unwrap();
    assert_eq!(w.sync_next().unwrap()["pending"], false);
    assert_eq!(receipt["publication"], "local_only");
    let mut b = a.clone();
    b.identity.display_name = "Later edit".into();
    b = b.sealed();
    w.seed(
        &[b],
        &BTreeMap::from([(a.identity.city_id.clone(), a.content_digest)]),
    )
    .unwrap();
    drop(w);
    let mut w = Workspace::open(&path).unwrap();
    assert_eq!(
        w.execute(&request, &tmp.path().join("missing-cache"))
            .unwrap(),
        receipt
    );
    assert_eq!(w.pending(20).unwrap().len(), 1);
    let next_input = w.sync_next().unwrap();
    assert_eq!(next_input["kind"], "city");
    w.rebase_pending_city(next_input["sequence"].as_i64().unwrap(), &"c".repeat(64))
        .unwrap();
    let ack = json!({"root":w.identity.root,"city_id":a.identity.city_id,
        "client_snapshot_sha256":next_input["jobs"][0]["fingerprint"],"input_base_fingerprint":"b".repeat(64)});
    w.sync_ack(
        next_input["sequence"].as_i64().unwrap(),
        next_input["digest"].as_str().unwrap(),
        &ack,
    )
    .unwrap();
    assert_eq!(w.sync_next().unwrap()["pending"], false);
    assert_eq!(w.run_result("offline-run").unwrap(), receipt);
    std::fs::write(
        path.join("runs/offline-run/cities/aderm_bere/result.json"),
        b"corrupted",
    )
    .unwrap();
    assert!(w.run_result("offline-run").is_err());
    ds_solar_project::clean::execute(ds_solar_project::clean::Control {
        workspace: path.clone(),
        project_id: "project-1".into(),
    })
    .unwrap();
    assert!(w.run_result("offline-run").is_err());
    let fresh = w.execute(&request, &tmp.path().join("references")).unwrap();
    assert_eq!(fresh["status"], "succeeded");
    assert_ne!(
        fresh["cities"][0]["input_digest"],
        receipt["cities"][0]["input_digest"]
    );
    assert_ne!(fresh["batch_digest"], receipt["batch_digest"]);
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
