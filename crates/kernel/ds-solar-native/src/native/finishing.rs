//! Final document finishing keeps the immutable authoring source separate.
use super::*;
use crate::ReportFormat;

#[derive(Clone, Deserialize, schemars::JsonSchema)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportFinishSource {
    City {
        batch: PortfolioBatchCommand,
        city_id: String,
        name: String,
    },
    Portfolio {
        batch: PortfolioBatchCommand,
        source_run_id: String,
        expected_source_batch_id: String,
        expected_source_batch_digest: String,
        name: String,
    },
}
impl ReportFinishSource {
    pub fn root(&self) -> &str {
        match self {
            Self::City { batch, .. } | Self::Portfolio { batch, .. } => &batch.root,
        }
    }
}
#[derive(Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportFinishCommand {
    pub source: ReportFinishSource,
    pub format: ReportFormat,
    pub destination: PathBuf,
}

pub fn finish(app: AppHandle, command: ReportFinishCommand) -> Result<serde_json::Value, String> {
    let selector = command.source.clone();
    let renderer = app
        .renderer
        .as_ref()
        .ok_or("This host has no headless document renderer; install Pandoc and LibreOffice")?;
    let extension = match command.format {
        ReportFormat::Docx => "docx",
        ReportFormat::Pdf => "pdf",
    };
    if command
        .destination
        .extension()
        .and_then(|value| value.to_str())
        != Some(extension)
    {
        return Err("The destination must match the requested document format".into());
    }
    let parent = secure_private_directory(&app.data_dir()?.join("report-finishing"))?;
    let staging = parent.join(uuid::Uuid::new_v4().to_string());
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&staging)
        .map_err(|_| "Cannot create private report staging")?;
    struct Staging(PathBuf);
    impl Drop for Staging {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _guard = Staging(staging.clone());
    let zip = staging.join("source.zip");
    let (name, package) = match command.source {
        ReportFinishSource::City {
            batch,
            city_id,
            name,
        } => {
            if city_finals::source_name(&name).is_none() {
                return Err("Select an imported reviewed city final".into());
            }
            let package = solar_native_report_bundle(
                app.clone(),
                ReportBundleCommand {
                    workspace_handle: batch.workspace_handle,
                    root: batch.root,
                    run_id: batch.run_id,
                    city_id,
                    expected_batch_id: batch.expected_batch_id,
                    expected_batch_digest: batch.expected_batch_digest,
                    name: name.clone(),
                    destination: zip.clone(),
                },
            )?;
            (name, package)
        }
        ReportFinishSource::Portfolio {
            batch,
            source_run_id,
            expected_source_batch_id,
            expected_source_batch_digest,
            name,
        } => {
            if portfolio_finals::source_name(&name).is_none() {
                return Err("Select an imported reviewed portfolio final".into());
            }
            let package = bundles::portfolio(
                app.clone(),
                PortfolioReportBundleCommand {
                    batch,
                    source_run_id,
                    expected_source_batch_id,
                    expected_source_batch_digest,
                    name: name.clone(),
                    destination: zip.clone(),
                },
            )?;
            (name, package)
        }
    };
    let path = committed_file(zip, "verified report package")?;
    let limit = (128 + 40) * 1024 * 1024;
    let file =
        std::fs::File::open(path).map_err(|_| "Cannot reopen the verified report package")?;
    if file
        .metadata()
        .map_err(|_| "Cannot inspect the verified report package")?
        .len()
        > limit
    {
        return Err("The verified report package exceeds its archive bound".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read the verified report package")?;
    if bytes.len() as u64 > limit {
        return Err("The verified report package exceeds its archive bound".into());
    }
    if package["content_digest"] != ds_solar_contracts::digest::sha256_hex(&bytes) {
        return Err("The report package changed before finishing".into());
    }
    let source_digest = package["source_content_digest"]
        .as_str()
        .ok_or("The report package has no source digest")?;
    let copy = ds_command_kernel::solar_report_bundle::render_copy(&bytes, &name, source_digest)?;
    let bytes = renderer.render(copy, command.format)?;
    revalidate_source(&app, &selector, source_digest)?;
    if bytes.is_empty()
        || bytes.len() > 64 * 1024 * 1024
        || match command.format {
            ReportFormat::Docx => !bytes.starts_with(b"PK"),
            ReportFormat::Pdf => !bytes.starts_with(b"%PDF-"),
        }
    {
        return Err("The document renderer returned an invalid or oversized document".into());
    }
    let mut receipt = crate::application::export_bytes(
        &command.destination,
        &ds_solar_contracts::digest::sha256_hex(&bytes),
        &bytes,
    )?;
    for key in [
        "portfolio_batch_id",
        "portfolio_batch_digest",
        "source_batch_id",
        "source_batch_digest",
        "source_content_digest",
        "image_count",
        "media_byte_count",
    ] {
        if let Some(value) = package.get(key) {
            receipt[key] = value.clone();
        }
    }
    receipt["format"] = extension.into();
    receipt["source_name"] = name.into();
    receipt["presentation_copy"] = true.into();
    Ok(receipt)
}

fn revalidate_source(
    app: &AppHandle,
    selector: &ReportFinishSource,
    digest: &str,
) -> Result<(), String> {
    let (batch_selector, directory, name) = match selector {
        ReportFinishSource::City {
            batch,
            city_id,
            name,
        } => {
            let city = checked_segment(city_id, "city id")?;
            (
                batch,
                run_directory(app, &batch.workspace_handle, &batch.root, &batch.run_id)?,
                format!("cities/{city}/{name}"),
            )
        }
        ReportFinishSource::Portfolio {
            batch,
            source_run_id,
            expected_source_batch_id,
            expected_source_batch_digest,
            name,
        } => {
            let source = run_directory(app, &batch.workspace_handle, &batch.root, source_run_id)?;
            let source_batch = closed_batch_at(&source)?;
            require_batch_selector(&source_batch, &batch.root, source_run_id)?;
            require_expected_portfolio_batch(
                &source_batch,
                expected_source_batch_id,
                expected_source_batch_digest,
            )?;
            verify_outputs(&source, &source_batch)?;
            (
                batch,
                portfolio_run_directory(app, &batch.workspace_handle, &batch.root, &batch.run_id)?,
                name.clone(),
            )
        }
    };
    let batch = closed_batch_at(&directory)?;
    require_batch_selector(&batch, &batch_selector.root, &batch_selector.run_id)?;
    require_expected_portfolio_batch(
        &batch,
        &batch_selector.expected_batch_id,
        &batch_selector.expected_batch_digest,
    )?;
    verify_outputs(&directory, &batch)?;
    let final_source = verified_batch_artifact_at(&directory, &batch, &name, MAX_DOCUMENT_BYTES)?;
    if final_source.descriptor.content_digest != digest {
        return Err("The report source changed during document rendering".into());
    }
    Ok(())
}

fn verify_outputs(
    directory: &Path,
    batch: &ds_solar_contracts::SolarArtifactBatch,
) -> Result<(), String> {
    for artifact in &batch.artifacts {
        verified_batch_artifact_at(directory, batch, &artifact.name, MAX_BATCH_ARTIFACT_BYTES)?;
    }
    Ok(())
}
