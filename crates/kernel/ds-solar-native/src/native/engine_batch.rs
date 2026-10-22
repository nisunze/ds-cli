//! Commit verified worker bytes through the existing native artifact lifecycle.
use super::*;
use ds_solar_contracts::ArtifactEntry;
use ds_solar_process::VerifiedBatch;

fn read(
    output: &VerifiedBatch,
    entry: &ArtifactEntry,
    cancel: &CancelToken,
) -> Result<Vec<u8>, String> {
    let file = output
        .file(&entry.name)
        .filter(|file| file.entry == *entry)
        .ok_or_else(|| format!("the Solar worker did not verify {}", entry.name))?;
    file.read(cancel).map_err(|error| error.to_string())
}

fn local_name<'a>(city: &str, entry: &'a ArtifactEntry) -> Result<&'a str, String> {
    let prefix = format!("cities/{city}/");
    entry
        .name
        .strip_prefix(&prefix)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            format!(
                "the Solar worker file {} belongs to another city",
                entry.name
            )
        })
}

fn descriptor(city: &str, entry: &ArtifactEntry) -> Result<DocumentFile, String> {
    Ok(DocumentFile {
        name: local_name(city, entry)?.to_owned(),
        media_type: entry.media_type.clone(),
        byte_count: entry.byte_count,
        content_digest: entry.content_digest.clone(),
    })
}

fn copy(
    directory: &Path,
    city: &str,
    output: &VerifiedBatch,
    entry: &ArtifactEntry,
    cancel: &CancelToken,
) -> Result<Vec<u8>, String> {
    let bytes = read(output, entry, cancel)?;
    let path = directory.join(local_name(city, entry)?);
    if let Some(parent) = path.parent() {
        secure_private_directory(parent)?;
    }
    if cancel.is_cancelled() {
        return Err("Solar calculation was cancelled before artifact commit".into());
    }
    write_atomically(&path, &bytes)?;
    Ok(bytes)
}

pub(super) fn commit(
    output_dir: &Path,
    output: &VerifiedBatch,
    cancel: &CancelToken,
    on_phase: impl Fn(&str, CityPhase),
) -> Result<Vec<CommittedCity>, String> {
    if output.output.portfolio.is_some() || output.output.portfolio_document.is_some() {
        return Err("a Solar city batch cannot implicitly create a portfolio".into());
    }
    let mut committed = Vec::new();
    for city in &output.output.cities {
        let Ok(outcome) = &city.outcome else { continue };
        let started = std::time::Instant::now();
        checked_segment(&city.city_id, "output city id")?;
        let projection = city
            .portfolio_projection
            .as_ref()
            .ok_or_else(|| format!("{} has no portfolio projection declaration", city.city_id))?
            .as_ref()
            .map_err(|error| {
                format!(
                    "{} is not eligible for a closed portfolio: {} ({})",
                    city.city_id,
                    error.message,
                    error.code.as_str()
                )
            })?;
        on_phase(&city.city_id, CityPhase::Committing);
        let directory = secure_private_directory(&output_dir.join("cities").join(&city.city_id))?;
        copy(
            &directory,
            &city.city_id,
            output,
            &outcome.report_input,
            cancel,
        )?;
        let report_input = descriptor(&city.city_id, &outcome.report_input)?;
        let result_name = format!("cities/{}/result.json", city.city_id);
        let result = output
            .file(&result_name)
            .ok_or("Solar result file is missing")?;
        let result_bytes = copy(&directory, &city.city_id, output, &result.entry, cancel)?;
        for entry in [
            &projection.contribution,
            &projection.calculation,
            &outcome.daily_load_profile_frame,
        ] {
            copy(&directory, &city.city_id, output, entry, cancel)?;
        }
        if let Some(model) = &outcome.report_model {
            copy(&directory, &city.city_id, output, model, cancel)?;
        }
        let mut chart_names = Vec::new();
        if let Some(artifacts) = &outcome.artifacts {
            for chart in &artifacts.charts {
                copy(&directory, &city.city_id, output, chart, cancel)?;
                chart_names.push(local_name(&city.city_id, chart)?.to_owned());
            }
        }
        for map in &outcome.input_maps {
            copy(&directory, &city.city_id, output, map, cancel)?;
        }
        chart_names.sort();
        on_phase(&city.city_id, CityPhase::ComposingReport);
        let mut documents = CityDocuments {
            report_input,
            apd: None,
            draft: None,
            drafts: Vec::new(),
            docx: None,
            docx_error: None,
            chart_count: chart_names.len(),
            section_count: 0,
            figure_count: 0,
            result_file_digest: ds_solar_contracts::digest::sha256_hex(&result_bytes)
                .strip_prefix("sha256:")
                .unwrap_or_default()
                .to_owned(),
            chart_names,
        };
        for document in &outcome.documents {
            let name = local_name(&city.city_id, document)?;
            if !valid_solar_segment(name) {
                return Err(format!("{name} is not a usable document name"));
            }
            let bytes = copy(&directory, &city.city_id, output, document, cancel)?;
            let file = descriptor(&city.city_id, document)?;
            if name == PRODUCT_APD_NAME || [PARITY_DRAFT_NAME, APD_DRAFT_EN_NAME].contains(&name) {
                let markdown = String::from_utf8_lossy(&bytes);
                documents.section_count = markdown
                    .lines()
                    .filter(|line| line.starts_with("## "))
                    .count();
                documents.figure_count = markdown.matches("![").count();
                if name == PRODUCT_APD_NAME {
                    documents.apd = Some(file);
                } else {
                    documents.draft = Some(file.clone());
                    documents.drafts.push(file);
                }
            } else if [
                NETWORK_DRAFT_NAME,
                NETWORK_DRAFT_EN_NAME,
                PLANT_DRAFT_NAME,
                PLANT_DRAFT_EN_NAME,
                FINANCIAL_DRAFT_NAME,
                FINANCIAL_DRAFT_EN_NAME,
            ]
            .contains(&name)
            {
                documents.drafts.push(file);
            }
        }
        on_phase(&city.city_id, CityPhase::LocalReady);
        committed.push(CommittedCity {
            city_id: city.city_id.clone(),
            documents,
            commit_ms: started.elapsed().as_millis() as u64,
        });
    }
    Ok(committed)
}
