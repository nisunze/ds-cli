//! Calculated project runs: the application orchestrates and verifies, this
//! tooling's in-process engine calculates. Moved from ds-solar-project, which
//! no longer links an engine.
use super::*;
use crate::fixtures::FixtureSet;
use ds_solar_contracts::{
    ArtifactRole, PreparedSolarCityInput, ReferencePin, SolarArtifactBatch, SolarCityInput,
    SolarError,
};
use ds_solar_project::{ProjectIdentity, Workspace, run::RunRequest};
use ds_solar_reference_host::{FixtureReferenceProvider, ReferencePreparer};
use ds_solar_host::cache::DirectoryReferenceCache;
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};

fn fixture() -> FixtureSet {
    FixtureSet::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures"))
}
fn input() -> SolarCityInput {
    fixture()
        .city_input("aderm_bere", "project-1", "eds_project/project-1/eds_solar")
        .unwrap()
}
fn workspace(path: &Path) -> Workspace {
    Workspace::initialize(
        path,
        ProjectIdentity {
            project_id: "project-1".into(),
            root: "eds_project/project-1/eds_solar".into(),
        },
    )
    .unwrap()
}
/// The application's restart-safe project run with this tooling as its engine.
fn run_project(
    w: &mut Workspace,
    request: &RunRequest,
    reference_cache: &Path,
) -> Result<serde_json::Value> {
    w.execute_with_network(request, reference_cache, &BTreeMap::new(), calculate)
}

#[test]
fn headless_portfolio_verifies_local_sources_without_publication() {
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
    let run = run_project(
        &mut w,
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
    // The local aggregation is the tooling's explicit portfolio command; the
    // application's dashboard verifies its sealed closure.
    let source_closure = std::fs::read(path.join("runs/source/batch.json")).unwrap();
    let revision = format!("sha256:{}", "a".repeat(64));
    let out = tmp.path().join("portfolio");
    crate::portfolio(crate::PortfolioArgs {
        source: path.join("runs/source"),
        source_run_id: "source".into(),
        city: cities.clone(),
        portfolio_id: "pf_1".into(),
        portfolio_name: "Three cities".into(),
        membership_revision: revision.clone(),
        run_id: "aggregate".into(),
        currency: "XAF".into(),
        project_years: 25,
        discount_rate: 0.1038,
        representative_city: cities[0].clone(),
        language: crate::PortfolioLanguageArg::Fr,
        report: vec![crate::PortfolioReportArg::Apd],
        out: out.clone(),
    })
    .unwrap();
    // Dashboards admit only private Solar directories, as native hosts create them.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let sealed = std::fs::read(out.join(ds_solar_runtime::PORTFOLIO_RESULT_NAME)).unwrap();
    let batch: SolarArtifactBatch =
        serde_json::from_slice(&std::fs::read(out.join("batch.json")).unwrap()).unwrap();
    let chart = batch
        .artifacts
        .iter()
        .find(|entry| entry.media_type == "image/png")
        .unwrap();
    let chart_path = out.join(&chart.name);
    let original_chart = std::fs::read(&chart_path).unwrap();
    std::fs::write(&chart_path, b"corrupt closed chart").unwrap();
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
    let receipt = run_project(&mut w, &request, &tmp.path().join("references")).unwrap();
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
    let receipt = run_project(&mut w, &request, &cache_path).unwrap();
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
    let receipt = run_project(&mut w, &request, &tmp.path().join("references")).unwrap();
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
        run_project(&mut w, &request, &tmp.path().join("missing-cache")).unwrap(),
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
    let fresh = run_project(&mut w, &request, &tmp.path().join("references")).unwrap();
    assert_eq!(fresh["status"], "succeeded");
    assert_ne!(
        fresh["cities"][0]["input_digest"],
        receipt["cities"][0]["input_digest"]
    );
    assert_ne!(fresh["batch_digest"], receipt["batch_digest"]);
}

fn prepared(city: &str) -> PreparedSolarCityInput {
    let temp = tempfile::tempdir().unwrap();
    let cache = DirectoryReferenceCache::new(temp.path());
    let provider = FixtureReferenceProvider::new(
        vec![fixture().reference_bundle(city).unwrap()],
        ReferencePin::current(),
    )
    .unwrap();
    ReferencePreparer::new()
        .with_cache(&cache)
        .with_provider(&provider)
        .prepare(
            fixture()
                .city_input(city, "ds-solar-fixtures", "solar")
                .unwrap(),
            "2026-08-20T00:00:00Z",
        )
        .unwrap()
        .input
}

fn computed_output() -> RunOutput {
    run_cities(
        &SolarRuntime::new(),
        &[prepared("aderm_bere")],
        &CalculationOptions::default()
            .with_run_id("local-byte-parity")
            .with_charts(true)
            .with_documents(true),
        Concurrency::new(1),
        "ds-solar-fixtures",
        "solar",
    )
    .unwrap()
}

fn tree(path: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(path, path, &mut out);
    out
}

fn assert_same_output(left: &Path, right: &Path) {
    let mut left = tree(left);
    let mut right = tree(right);
    // The enqueue clock is intentionally sampled separately at each local commit.
    let mut l: serde_json::Value =
        serde_json::from_slice(&left.remove("sync-intent.json").unwrap()).unwrap();
    let mut r: serde_json::Value =
        serde_json::from_slice(&right.remove("sync-intent.json").unwrap()).unwrap();
    l["enqueued_at"] = json!("clock");
    r["enqueued_at"] = json!("clock");
    assert_eq!(l, r);
    assert_eq!(left, right);
}

#[test]
fn legacy_runtime_wrapper_and_engine_free_commit_have_identical_artifact_bytes() {
    let output = computed_output();
    assert!(output.failures().is_empty());
    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join("legacy");
    let copied = temp.path().join("copied");
    let progress = json!([{ "stage":"fixture", "message":"done" }]);
    commit_run(&legacy, &output, Some(&progress)).unwrap();
    commit_artifact_run(&copied, &output.batch, &[], Some(&progress), |root| {
        for artifact in &output.batch.artifacts {
            files::write(
                &root.join(&artifact.name),
                &files::read(&legacy.join(&artifact.name), 128 * 1024 * 1024)?,
            )?;
        }
        Ok(())
    })
    .unwrap();
    assert_same_output(&legacy, &copied);
    assert_eq!(tree(&legacy).len(), output.batch.artifacts.len() + 4);
}

#[test]
fn legacy_wrapper_preserves_city_success_when_portfolio_projection_and_another_city_fail() {
    let mut output = computed_output();
    output.cities[0].portfolio_projection = Some(Err(SolarError::invalid_input(
        "portfolio-ineligible fixture",
    )));
    output.batch = SolarArtifactBatch::close(
        output.batch.run.clone(),
        output
            .batch
            .artifacts
            .into_iter()
            .filter(|artifact| {
                !matches!(
                    artifact.role,
                    ArtifactRole::PortfolioContribution | ArtifactRole::PortfolioCalculation
                )
            })
            .collect(),
    )
    .unwrap();
    output.cities.push(ds_solar_runtime::batch::CityRun {
        city_id: "failed".into(),
        outcome: Err(SolarError::invalid_input("failed city fixture")),
        portfolio_projection: None,
    });
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("partial");
    commit_run(&out, &output, Some(&json!([]))).unwrap();
    files::verify(&out, &output.batch).unwrap();
    let failures: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("failures.json")).unwrap()).unwrap();
    assert_eq!(failures.as_array().unwrap().len(), 1);
    assert_eq!(failures[0]["city_id"], "failed");
    assert!(out.join("cities/aderm_bere/result.json").is_file());
    assert!(
        !out.join("cities/aderm_bere/portfolio-contribution.json")
            .exists()
    );
    assert!(
        !out.join("cities/aderm_bere/portfolio-calculation.json")
            .exists()
    );
    assert!(!out.join("cities/failed").exists());
}
