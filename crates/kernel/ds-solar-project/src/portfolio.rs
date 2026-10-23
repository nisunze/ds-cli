//! Portfolio request vocabulary and verification of sealed portfolio results.
//! Aggregation is engine computation: DS Core runs it through the Solar addon
//! (`solar.application`), so this application crate never calculates one.
use crate::files;
use anyhow::{Result, ensure};
use ds_solar_contracts::{
    ArtifactRole, PortfolioIdentity, PortfolioRepresentativeStrategy, SolarArtifactBatch,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The closed `portfolio_calculate` request. It is decoded so a malformed
/// request keeps its exact refusal; this owner then refuses to aggregate.
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
