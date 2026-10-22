//! Verify the exact city artifact before any dashboard host can compose it.
use crate::files;
use anyhow::{Result, ensure};
use ds_solar_contracts::{ArtifactRole, SolarArtifactBatch};
use serde_json::{Value, json};
use std::path::Path;

/// Bind one immutable portfolio closure before handing presentation to Rust.
pub fn portfolio_source(
    source: &Path,
    project: &str,
    run: &str,
    portfolio: &str,
    revision: &str,
) -> Result<Value> {
    let (batch, result, bytes) = crate::portfolio::sealed_result(source)?;
    ensure!(
        result.project_id == project
            && result.root == format!("eds_project/{project}/eds_solar")
            && result.run_id == run
            && result.portfolio_id == portfolio
            && result.membership_revision == revision,
        "portfolio dashboard project, run, portfolio or membership mismatch"
    );
    let report: Value = serde_json::from_slice(&bytes)?;
    Ok(
        json!({"schema":"ds-solar.portfolio-dashboard-source/v1","project_id":project,"root":result.root,
        "run_id":run,"portfolio_id":portfolio,"membership_revision":revision,
        "city_ids":result.cities.iter().map(|c| c.city_id.as_str()).collect::<Vec<_>>(),
        "batch_id":batch.batch_id,"batch_digest":batch.batch_digest,
        "report_digest":ds_solar_contracts::digest::sha256_hex(&bytes),"report":report}),
    )
}

pub fn source(source: &Path, project: &str, run: &str, city: &str) -> Result<Value> {
    let batch: SolarArtifactBatch =
        serde_json::from_slice(&files::read(&source.join("batch.json"), 64 * 1024 * 1024)?)?;
    files::verify(source, &batch)?;
    ensure!(
        batch.run.project_id == project
            && batch.run.root == format!("eds_project/{project}/eds_solar")
            && batch.run.run_id == run,
        "dashboard source project or run mismatch"
    );
    let member = batch
        .run
        .cities
        .iter()
        .find(|c| c.city_id == city)
        .ok_or_else(|| anyhow::anyhow!("dashboard city is not in the closed batch"))?;
    let artifacts: Vec<_> = batch
        .artifacts
        .iter()
        .filter(|a| a.role == ArtifactRole::ReportInput && a.city_id.as_deref() == Some(city))
        .collect();
    ensure!(
        artifacts.len() == 1,
        "dashboard needs exactly one sealed city report input"
    );
    let artifact = artifacts[0];
    ensure!(
        artifact.media_type == "application/json",
        "dashboard report input must be JSON"
    );
    let bytes = files::read(&source.join(&artifact.name), 32 * 1024 * 1024)?;
    ensure!(
        ds_solar_contracts::digest::sha256_hex(&bytes) == artifact.content_digest,
        "dashboard report changed during verification"
    );
    let report: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        report["_meta"]["project_id"] == project
            && report["_meta"]["root"] == batch.run.root
            && report["_meta"]["city_id"] == city
            && report["_meta"]["input_digest"] == member.input_digest
            && report["_meta"]["result_digest"] == member.result_digest,
        "dashboard report metadata is outside the requested project, run or city"
    );
    let results: Vec<_> = batch
        .artifacts
        .iter()
        .filter(|a| a.role == ArtifactRole::CityResult && a.city_id.as_deref() == Some(city))
        .collect();
    ensure!(
        results.len() == 1,
        "dashboard needs exactly one sealed city result"
    );
    let result_bytes = files::read(&source.join(&results[0].name), 32 * 1024 * 1024)?;
    ensure!(
        ds_solar_contracts::digest::sha256_hex(&result_bytes) == results[0].content_digest,
        "dashboard city result changed during verification"
    );
    let result: ds_solar_contracts::SolarCalculationResult = serde_json::from_slice(&result_bytes)?;
    result.validate()?;
    ensure!(
        result.identity.project_id == project
            && result.identity.root == batch.run.root
            && result.identity.city_id == city
            && result.input_digest == member.input_digest
            && result.result_digest == member.result_digest
            && report["_meta"]["pipeline_run_id"].as_str() == result.run_id.as_deref()
            && result.run_id.is_some(),
        "dashboard city result identity mismatch"
    );
    Ok(
        json!({"schema":"ds-solar.dashboard-source/v1","project_id":project,"root":batch.run.root,"run_id":run,"city_id":city,"batch_id":batch.batch_id,"batch_digest":batch.batch_digest,"report_digest":artifact.content_digest,"report":report}),
    )
}
