//! Aggregate exact verified city artifacts; never refresh or recalculate a city.
use crate::files;
use anyhow::{Result, ensure};
use ds_solar_contracts::{
    ArtifactRole, PortfolioIdentity, PortfolioRepresentativeStrategy, SolarArtifactBatch,
};
#[cfg(feature = "compute")]
use ds_solar_contracts::{
    PortfolioCityContribution, PortfolioDocumentFormat, PortfolioDocumentOptions,
    PortfolioLanguage, PortfolioMember, PortfolioReportIntent, PortfolioRunInput,
};
#[cfg(feature = "compute")]
use ds_solar_runtime::{
    PortfolioCityCalculation, derive_portfolio_calculation_assumptions, run_portfolio,
};
use serde::{Deserialize, Serialize};
#[cfg(feature = "compute")]
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub source: PathBuf,
    pub source_run_id: String,
    pub out: PathBuf,
    pub identity: PortfolioIdentity,
    pub cities: Vec<String>,
    pub run_id: String,
    pub strategy: PortfolioRepresentativeStrategy,
}
#[cfg(feature = "compute")]
pub fn calculate(request: Request) -> Result<Value> {
    let batch: SolarArtifactBatch = serde_json::from_slice(&files::read(
        &request.source.join("batch.json"),
        4 * 1024 * 1024,
    )?)?;
    files::verify(&request.source, &batch)?;
    ensure!(
        batch.run.run_id == request.source_run_id,
        "source run id mismatch"
    );
    ensure!(
        batch.run.project_id == request.identity.project_id
            && batch.run.root == request.identity.root,
        "source project mismatch"
    );
    ds_solar_runtime::verify_portfolio_source_membership(&batch, &request.cities)?;
    let mut contributions = Vec::new();
    let mut calculations = Vec::new();
    let mut members = Vec::new();
    for city in &request.cities {
        let load = |name: &str, role: ArtifactRole| -> Result<(Vec<u8>, String)> {
            let path = format!("cities/{city}/{name}");
            let entry = batch
                .artifacts
                .iter()
                .find(|a| a.name == path)
                .ok_or_else(|| anyhow::anyhow!("missing portfolio input {path}"))?;
            ensure!(
                entry.role == role
                    && entry.city_id.as_deref() == Some(city)
                    && entry.media_type == "application/json",
                "invalid portfolio source role"
            );
            let bytes = files::read(&request.source.join(&path), 16 * 1024 * 1024)?;
            ensure!(
                ds_solar_contracts::digest::sha256_hex(&bytes) == entry.content_digest,
                "portfolio source changed during verification"
            );
            Ok((bytes, entry.content_digest.clone()))
        };
        let contribution: PortfolioCityContribution = serde_json::from_slice(
            &load(
                ds_solar_contracts::artifact_names::PORTFOLIO_CONTRIBUTION_NAME,
                ArtifactRole::PortfolioContribution,
            )?
            .0,
        )?;
        let calculation: PortfolioCityCalculation = serde_json::from_slice(
            &load(
                ds_solar_contracts::artifact_names::PORTFOLIO_CALCULATION_NAME,
                ArtifactRole::PortfolioCalculation,
            )?
            .0,
        )?;
        let (bytes, digest) = load(
            ds_solar_contracts::artifact_names::REPORT_INPUT_NAME,
            ArtifactRole::ReportInput,
        )?;
        let report = serde_json::from_slice(&bytes)?;
        ensure!(
            PortfolioCityCalculation::close(&contribution, &report, digest)? == calculation,
            "portfolio calculation does not match the sealed report input"
        );
        members.push(PortfolioMember::bind(
            &contribution,
            calculation.projection_digest.clone(),
            batch.batch_id.clone(),
            batch.batch_digest.clone(),
        )?);
        contributions.push(contribution);
        calculations.push(calculation);
    }
    let input = PortfolioRunInput::close(
        request.identity,
        derive_portfolio_calculation_assumptions(&calculations)?,
        request.run_id,
        members,
        request.strategy,
        PortfolioLanguage::Fr,
        PortfolioDocumentOptions {
            report_intents: vec![PortfolioReportIntent::Apd],
            formats: vec![PortfolioDocumentFormat::Markdown],
        },
    )?;
    let output = run_portfolio(&input, &contributions, &calculations, &[batch])?;
    ensure!(
        std::fs::symlink_metadata(&request.out).is_err(),
        "portfolio output already exists"
    );
    let parent = request
        .out
        .parent()
        .ok_or_else(|| anyhow::anyhow!("output needs a parent"))?;
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".solar-portfolio-")
        .tempdir_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(staging.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    for artifact in std::iter::once(&output.input_artifact)
        .chain(std::iter::once(&output.result_artifact))
        .chain(output.documents.iter())
        .chain(output.charts.iter())
    {
        files::write(&staging.path().join(artifact.name()), &artifact.bytes)?;
    }
    files::verify(staging.path(), &output.batch)?;
    files::json(&staging.path().join("batch.json"), &output.batch)?;
    files::json(
        &staging.path().join("sync-intent.json"),
        &output.batch.sync_intent(crate::timestamp()),
    )?;
    files::sync_dir(staging.path())?;
    std::fs::rename(staging.path(), &request.out)?;
    files::sync_dir(parent)?;
    Ok(
        json!({"project_id":output.result.project_id,"portfolio_id":output.result.portfolio_id,"membership_revision":output.result.membership_revision,"city_count":output.result.city_count,"run_id":output.result.run_id,"batch_id":output.batch.batch_id,"batch_digest":output.batch.batch_digest,"directory":request.out,"publication":"not_requested"}),
    )
}

pub(crate) fn sealed_result(
    source: &Path,
) -> Result<(
    SolarArtifactBatch,
    ds_solar_contracts::SolarPortfolioResult,
    Vec<u8>,
)> {
    let batch: SolarArtifactBatch =
        serde_json::from_slice(&files::read(&source.join("batch.json"), 4 * 1024 * 1024)?)?;
    files::verify(source, &batch)?;
    let bytes = files::read(
        &source.join(ds_solar_contracts::artifact_names::PORTFOLIO_RESULT_NAME),
        16 * 1024 * 1024,
    )?;
    let result: ds_solar_contracts::SolarPortfolioResult = serde_json::from_slice(&bytes)?;
    result.validate()?;
    ensure!(
        batch.run.run_id == result.run_id
            && batch.run.project_id == result.project_id
            && batch.run.root == result.root,
        "portfolio batch identity mismatch"
    );
    let declarations: Vec<_> = batch
        .artifacts
        .iter()
        .filter(|a| a.name == ds_solar_contracts::artifact_names::PORTFOLIO_RESULT_NAME)
        .collect();
    ensure!(
        declarations.len() == 1,
        "portfolio needs exactly one result declaration"
    );
    let declared = declarations[0];
    ensure!(
        declared.role == ArtifactRole::PortfolioResult
            && declared.city_id.is_none()
            && declared.media_type == "application/json"
            && declared.byte_count == bytes.len() as u64
            && declared.content_digest == ds_solar_contracts::digest::sha256_hex(&bytes),
        "portfolio result is not sealed by its batch"
    );
    ensure!(
        batch.run.cities.len() == result.cities.len()
            && batch
                .run
                .cities
                .iter()
                .zip(&result.cities)
                .all(|(a, b)| a.city_id == b.city_id
                    && a.input_digest == b.input_digest
                    && a.result_digest == b.result_digest),
        "portfolio batch members differ from its result"
    );
    Ok((batch, result, bytes))
}
