//! Process-level coverage for the sealed city-input adapter.
//!
//! This is deliberately not a unit test of `prepare`: it proves the shipped
//! binary can receive an application-exported input, find its verified bundle
//! in a local cache without a URL, and produce the calculation and selected
//! deterministic drafts without reviving the removed local final-report step.

use ds_solar_reference_host::ReferencePreparer;
use std::path::PathBuf;
use std::process::Command;

use ds_solar_contracts::{
    BundleBytes, CityIdentity, RunOptions, SnapshotRevision, SolarArtifactBatch,
    SolarCalculationResult, SolarCityInput, SolarPortfolioResult, digest,
};
use ds_solar_host::cache::{DirectoryReferenceCache, ReferenceCache};

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures")
}

fn fixture_input(city: &str) -> SolarCityInput {
    let root = fixtures_root();
    let snapshot: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("city").join(format!("{city}.snapshot.json")))
            .expect("read fixture snapshot"),
    )
    .expect("parse fixture snapshot");
    SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: "explicit-city-input-test".to_string(),
            root: "solar".to_string(),
            city_id: city.to_string(),
            display_name: snapshot["_root"]["display_name"]
                .as_str()
                .unwrap_or(city)
                .to_string(),
        },
        SnapshotRevision {
            source: "test-export".to_string(),
            captured_at: "2026-08-22T00:00:00Z".to_string(),
            record_updated_at_ms: None,
            revision: None,
        },
        RunOptions::default(),
        &snapshot,
    )
    .expect("fixture snapshot normalises")
}

fn fixture_bundle(city: &str) -> BundleBytes {
    let directory = fixtures_root().join("reference").join(city);
    let read = |name: &str| std::fs::read(directory.join(name)).expect("read bundle artifact");
    BundleBytes {
        weather_json: read("weather.json"),
        reference_unit_json: read("reference-unit.json"),
        reference_unit_parquet: read("reference-unit.parquet"),
        manifest_json: read("manifest.json"),
    }
}

fn run(command: &mut Command) {
    let output = command.output().expect("start ds-solar binary");
    assert!(
        output.status.success(),
        "ds-solar failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn explicit_city_input_prepares_and_runs_entirely_from_verified_local_cache() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let city = "aderm_bere";
    let input = fixture_input(city);
    let input_path = temporary.path().join("city-input.json");
    std::fs::write(
        &input_path,
        serde_json::to_vec_pretty(&input).expect("encode sealed input"),
    )
    .expect("write sealed input");

    // A normal cache hit must be enough: the command has no reference URL or
    // token, so a hidden network fallback would make this invocation fail.
    let cache_root = temporary.path().join("reference-cache");
    let cache = DirectoryReferenceCache::new(&cache_root);
    let key = ReferencePreparer::new()
        .reference_key(&input)
        .expect("derive reference key");
    cache
        .put(&key, &fixture_bundle(city))
        .expect("seed verified local reference bundle");

    let prepared = temporary.path().join("prepared");
    let binary = env!("CARGO_BIN_EXE_ds-solar");
    run(Command::new(binary)
        .arg("--fixtures")
        .arg(fixtures_root())
        .arg("prepare")
        .arg("--city-input")
        .arg(&input_path)
        .arg("--cache")
        .arg(&cache_root)
        .arg("--out")
        .arg(&prepared));

    let prepared_input = prepared.join(format!("{city}.prepared.json"));
    assert!(prepared_input.is_file(), "prepare did not write its input");

    let result = temporary.path().join("result");
    run(Command::new(binary)
        .arg("run")
        .arg("--prepared")
        .arg(&prepared)
        .arg("--out")
        .arg(&result)
        .arg("--concurrency")
        .arg("1"));

    let city_result = result.join("cities").join(city);
    for artifact in [
        city_result.join("report_input.json"),
        city_result.join("result.json"),
        city_result.join("apd-draft-fr.md"),
        result.join("batch.json"),
        result.join("sync-intent.json"),
    ] {
        assert!(
            artifact.is_file(),
            "missing local artifact {}",
            artifact.display()
        );
    }
    assert!(
        !result.join("portfolio-result.json").exists()
            && !result.join("portfolio-draft-fr.md").exists(),
        "one city run must not be promoted into a portfolio"
    );

    let batch: SolarArtifactBatch = serde_json::from_slice(
        &std::fs::read(result.join("batch.json")).expect("read committed batch"),
    )
    .expect("parse committed batch");
    batch.validate().expect("committed batch validates");
    for artifact in &batch.artifacts {
        let path = result.join(&artifact.name);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{} is missing: {error}", path.display()));
        assert_eq!(
            artifact.byte_count,
            bytes.len() as u64,
            "{}",
            path.display()
        );
        assert_eq!(
            artifact.content_digest,
            digest::sha256_hex(&bytes),
            "{}",
            path.display()
        );
    }

    let result_path = city_result.join("result.json");
    let compare = Command::new(binary)
        .arg("compare")
        .arg("--left")
        .arg(&result_path)
        .arg("--right")
        .arg(&result_path)
        .arg("--json")
        .output()
        .expect("compare sealed result");
    assert!(
        compare.status.success(),
        "comparison should be data, not a refusal"
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&compare.stdout).expect("parse comparison receipt");
    assert_eq!(receipt["schema"], "ds-solar.result-comparison/v1");
    assert_eq!(receipt["equal"], true);
    assert_eq!(
        receipt["left"]["identity"]["project_id"],
        "explicit-city-input-test"
    );
    assert_eq!(
        receipt["left"]["result_digest"],
        receipt["right"]["result_digest"]
    );
    assert!(
        !String::from_utf8_lossy(&compare.stdout).contains(&result_path.display().to_string()),
        "machine receipt must not reflect local paths"
    );

    let mut different: SolarCalculationResult =
        serde_json::from_slice(&std::fs::read(&result_path).expect("read city result"))
            .expect("parse city result");
    different.identity.display_name.push_str(" revised");
    different = different.sealed();
    let different_path = temporary.path().join("different-result.json");
    std::fs::write(
        &different_path,
        serde_json::to_vec_pretty(&different).expect("encode different sealed result"),
    )
    .expect("write different sealed result");
    let compare = Command::new(binary)
        .arg("compare")
        .arg("--left")
        .arg(&result_path)
        .arg("--right")
        .arg(&different_path)
        .arg("--json")
        .output()
        .expect("compare different sealed results");
    assert!(
        compare.status.success(),
        "a valid difference is not an engine refusal"
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&compare.stdout).expect("parse different receipt");
    assert_eq!(receipt["equal"], false);
    assert_ne!(
        receipt["left"]["result_digest"],
        receipt["right"]["result_digest"]
    );

    different.identity.display_name = " unsafe ".to_string();
    different = different.sealed();
    let unsafe_path = temporary.path().join("unsafe-result.json");
    std::fs::write(
        &unsafe_path,
        serde_json::to_vec_pretty(&different).expect("encode unsafe sealed result"),
    )
    .expect("write unsafe sealed result");
    let compare = Command::new(binary)
        .arg("compare")
        .arg("--left")
        .arg(&result_path)
        .arg("--right")
        .arg(&unsafe_path)
        .arg("--json")
        .output()
        .expect("compare unsafe result");
    assert!(
        !compare.status.success(),
        "unsafe provenance must refuse instead of entering the machine receipt"
    );
}

#[test]
fn exact_city_batch_can_be_closed_into_an_explicit_portfolio_run() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let binary = env!("CARGO_BIN_EXE_ds-solar");
    let prepared = temporary.path().join("prepared");
    run(Command::new(binary)
        .arg("--fixtures")
        .arg(fixtures_root())
        .arg("prepare")
        .arg("--city")
        .arg("aderm_beinamar")
        .arg("--city")
        .arg("aderm_bere")
        .arg("--out")
        .arg(&prepared));

    let source = temporary.path().join("source-run");
    run(Command::new(binary)
        .arg("run")
        .arg("--prepared")
        .arg(&prepared)
        .arg("--run-id")
        .arg("portfolio-source")
        .arg("--out")
        .arg(&source)
        .arg("--concurrency")
        .arg("2"));

    let portfolio = temporary.path().join("portfolio-run");
    run(Command::new(binary)
        .arg("portfolio")
        .arg("--source")
        .arg(&source)
        .arg("--source-run-id")
        .arg("portfolio-source")
        .arg("--city")
        .arg("aderm_beinamar")
        .arg("--city")
        .arg("aderm_bere")
        .arg("--portfolio-id")
        .arg("cli-portfolio")
        .arg("--portfolio-name")
        .arg("CLI portfolio")
        .arg("--membership-revision")
        .arg("test:1")
        .arg("--run-id")
        .arg("cli-portfolio-run")
        .arg("--currency")
        .arg("XAF")
        .arg("--project-years")
        .arg("25")
        .arg("--discount-rate")
        .arg("0.1038")
        .arg("--representative-city")
        .arg("aderm_beinamar")
        .arg("--language")
        .arg("fr")
        .arg("--report")
        .arg("apd")
        .arg("--out")
        .arg(&portfolio));

    for artifact in [
        "portfolio-run-input.json",
        "portfolio-result.json",
        "portfolio-draft-fr.md",
        "batch.json",
        "sync-intent.json",
    ] {
        assert!(portfolio.join(artifact).is_file(), "missing {artifact}");
    }
    let result: SolarPortfolioResult = serde_json::from_slice(
        &std::fs::read(portfolio.join("portfolio-result.json")).expect("portfolio result"),
    )
    .expect("parse portfolio result");
    result.validate().expect("portfolio result validates");
    assert_eq!(
        result
            .cities
            .iter()
            .map(|city| city.city_id.as_str())
            .collect::<Vec<_>>(),
        ["aderm_beinamar", "aderm_bere"]
    );
    let batch: SolarArtifactBatch = serde_json::from_slice(
        &std::fs::read(portfolio.join("batch.json")).expect("portfolio batch"),
    )
    .expect("parse portfolio batch");
    batch.validate().expect("portfolio batch validates");
    for artifact in &batch.artifacts {
        let bytes = std::fs::read(portfolio.join(&artifact.name)).expect("declared artifact");
        assert_eq!(artifact.byte_count, bytes.len() as u64);
        assert_eq!(artifact.content_digest, digest::sha256_hex(&bytes));
    }
}
