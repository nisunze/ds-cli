//! The tooling's in-process Solar engine for the application's local project run.
//! `ds-solar-project` captures and prepares inputs, holds the run locks and
//! verifies each closed batch, but links no engine. Only this local tooling
//! calculates in process; DS Core reaches the separately released Solar addon
//! by request/response.
use anyhow::{Result, ensure};
use ds_solar_project::{
    files::{self, RunFailure, commit_artifact_run},
    run::LocalCalculation,
};
use ds_solar_runtime::{CalculationOptions, Concurrency, RunOutput, SolarRuntime, run_cities};
use std::path::Path;

/// Calculate one captured project run and commit its closed batch at the
/// destination the workspace selected.
pub fn calculate(calculation: LocalCalculation<'_>) -> Result<()> {
    let mut options = CalculationOptions::default()
        .with_run_id(calculation.run_id)
        .with_charts(calculation.charts)
        .with_documents(true)
        .with_report_drafts(calculation.drafts.to_vec());
    // The workspace holds Core's host documents; this tooling's in-process
    // runtime takes its own document type with the same entry and bytes.
    options.input_maps = calculation
        .input_maps
        .into_iter()
        .map(|(city, maps)| {
            let maps = maps
                .into_iter()
                .map(|(uri, map)| {
                    let document = ds_solar_runtime::GeneratedDocument {
                        entry: map.entry,
                        bytes: map.bytes,
                    };
                    (uri, document)
                })
                .collect();
            (city, maps)
        })
        .collect();
    let output = run_cities(
        &SolarRuntime::new(),
        calculation.prepared,
        &options,
        Concurrency::new(calculation.concurrency),
        calculation.project_id,
        calculation.root,
    )?;
    commit_run(calculation.destination, &output, None)
}

/// Rendering and calculation are complete before this IO operation starts.
/// A visible destination always contains the complete verified artifact batch.
pub fn commit_run(
    out: &Path,
    output: &RunOutput,
    progress: Option<&serde_json::Value>,
) -> Result<()> {
    let failures: Vec<_> = output
        .cities
        .iter()
        .filter_map(|city| {
            city.outcome.as_ref().err().map(|error| RunFailure {
                city_id: &city.city_id,
                error,
            })
        })
        .collect();
    commit_artifact_run(out, &output.batch, &failures, progress, |root| {
        for city in &output.cities {
            let Ok(outcome) = &city.outcome else { continue };
            ensure!(
                ds_solar_project::identifier(&city.city_id),
                "invalid output city id"
            );
            let dir = root.join("cities").join(&city.city_id);
            files::json(&dir.join("result.json"), &outcome.result)?;
            if let Some(model) = &outcome.report_model {
                files::json(&dir.join("report-model.json"), model)?;
            }
            files::write(
                &dir.join(outcome.report_input.name()),
                &outcome.report_input.bytes,
            )?;
            if let Some(artifacts) = &outcome.artifacts {
                for chart in &artifacts.charts {
                    files::write(&dir.join(&chart.name), &chart.bytes)?;
                }
            }
            for document in outcome.documents.iter().chain(&outcome.input_maps) {
                files::write(&dir.join(document.name()), &document.bytes)?;
            }
            if let Some(Ok(projection)) = &city.portfolio_projection {
                files::json(
                    &dir.join(ds_solar_contracts::artifact_names::PORTFOLIO_CONTRIBUTION_NAME),
                    &projection.contribution,
                )?;
                files::json(
                    &dir.join(ds_solar_contracts::artifact_names::PORTFOLIO_CALCULATION_NAME),
                    &projection.calculation,
                )?;
            }
        }
        if let Some(portfolio) = &output.portfolio {
            files::json(&root.join("portfolio-result.json"), portfolio)?;
        }
        if let Some(document) = &output.portfolio_document {
            files::write(&root.join(document.name()), &document.bytes)?;
        }
        Ok(())
    })
}

#[cfg(test)]
#[path = "local_engine_tests.rs"]
mod tests;
