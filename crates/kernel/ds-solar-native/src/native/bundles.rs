//! Portfolio report packages resolve only the sealed aggregate and its pinned city source.
use super::*;
use ds_command_kernel::solar_report_bundle as bundle;
use ds_solar_contracts::{ArtifactRole, portfolio::PortfolioRunInput};

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortfolioReportBundleCommand {
    pub batch: PortfolioBatchCommand,
    pub source_run_id: String,
    pub expected_source_batch_id: String,
    pub expected_source_batch_digest: String,
    pub name: String,
    pub destination: PathBuf,
}

pub(super) fn portfolio(
    app: AppHandle,
    command: PortfolioReportBundleCommand,
) -> Result<serde_json::Value, String> {
    let selector = &command.batch;
    let directory = portfolio_run_directory(
        &app,
        &selector.workspace_handle,
        &selector.root,
        &selector.run_id,
    )?;
    let batch = closed_batch_at(&directory)?;
    require_batch_selector(&batch, &selector.root, &selector.run_id)?;
    require_expected_portfolio_batch(
        &batch,
        &selector.expected_batch_id,
        &selector.expected_batch_digest,
    )?;
    let source_directory = run_directory(
        &app,
        &selector.workspace_handle,
        &selector.root,
        &command.source_run_id,
    )?;
    let source_batch = closed_batch_at(&source_directory)?;
    require_batch_selector(&source_batch, &selector.root, &command.source_run_id)?;
    require_expected_portfolio_batch(
        &source_batch,
        &command.expected_source_batch_id,
        &command.expected_source_batch_digest,
    )?;
    let input_file = verified_batch_artifact_at(
        &directory,
        &batch,
        "portfolio-run-input.json",
        MAX_DOCUMENT_BYTES,
    )?;
    if input_file.descriptor.role != ArtifactRole::PortfolioRunInput {
        return Err("The aggregate has no declared portfolio input".into());
    }
    let input: PortfolioRunInput =
        serde_json::from_slice(&input_file.bytes).map_err(|_| "Invalid sealed portfolio input")?;
    input.validate().map_err(|e| e.message)?;
    if input.identity.root != selector.root
        || input.run_id != selector.run_id
        || input.members.iter().any(|m| {
            m.source_batch_id != source_batch.batch_id
                || m.source_batch_digest != source_batch.batch_digest
        })
    {
        return Err(
            "The report package does not name the city source sealed by its portfolio".into(),
        );
    }
    let report = verified_batch_artifact_at(&directory, &batch, &command.name, MAX_DOCUMENT_BYTES)?;
    if report.descriptor.city_id.is_some() || report.descriptor.role != ArtifactRole::Report {
        return Err("Select a declared portfolio Markdown report".into());
    }
    if report.descriptor.media_type != ds_solar_contracts::artifact_names::MARKDOWN_MEDIA_TYPE {
        return Err("Select a declared portfolio Markdown report".into());
    }
    let markdown =
        std::str::from_utf8(&report.bytes).map_err(|_| "Portfolio report must be UTF-8")?;
    let plan = bundle::plan(&command.name, markdown)?;
    let member = |city: &str| -> Result<(), String> {
        checked_segment(city, "report city")?;
        if !input.members.iter().any(|m| m.city_id == city) {
            return Err("A report image belongs to a city outside the sealed portfolio".into());
        }
        Ok(())
    };
    let mut media = BTreeMap::new();
    let mut total = 0usize;
    for image in &plan.images {
        let bytes = if let Some(object) = image
            .reference
            .strip_prefix("gs://")
            .and_then(|s| s.split_once('/').map(|(_, object)| object))
        {
            let path = object
                .strip_prefix(&format!("{}/", selector.root))
                .ok_or("A governed map belongs to another project")?;
            let (city, _) = path.split_once('/').ok_or("A governed map has no city")?;
            member(city)?;
            app.media
                .as_ref()
                .ok_or("This host has no governed report-media producer")?
                .fetch(city, &image.reference)?
        } else if let Some(path) = image
            .reference
            .strip_prefix("ds-solar://city/")
            .or_else(|| image.reference.strip_prefix("cities/"))
        {
            let (city, relative) = path.split_once('/').ok_or("A source chart has no city")?;
            member(city)?;
            let image = verified_batch_artifact_at(
                &source_directory,
                &source_batch,
                &format!("cities/{city}/{relative}"),
                bundle::MAX_IMAGE_BYTES as u64,
            )?;
            require_chart(&image.descriptor, Some(city))?;
            image.bytes
        } else {
            let image = verified_batch_artifact_at(
                &directory,
                &batch,
                &image.reference,
                bundle::MAX_IMAGE_BYTES as u64,
            )?;
            require_chart(&image.descriptor, None)?;
            image.bytes
        };
        if bytes.is_empty()
            || bytes.len() > bundle::MAX_IMAGE_BYTES
            || !(bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                || bytes.starts_with(b"\xff\xd8\xff")
                || bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
        {
            return Err("A portfolio report image is not a bounded PNG, JPEG or WebP".into());
        }
        total = total
            .checked_add(bytes.len())
            .ok_or("Portfolio report media overflow")?;
        if total > bundle::MAX_MEDIA_BYTES {
            return Err("Portfolio report media exceeds 128 MiB".into());
        }
        media.insert(image.reference.clone(), bytes);
    }
    let bytes = bundle::build(&command.name, markdown, &media)?;
    let current = closed_batch_at(&directory)?;
    require_expected_portfolio_batch(
        &current,
        &selector.expected_batch_id,
        &selector.expected_batch_digest,
    )?;
    let current_source = closed_batch_at(&source_directory)?;
    require_expected_portfolio_batch(
        &current_source,
        &command.expected_source_batch_id,
        &command.expected_source_batch_digest,
    )?;
    if verified_batch_artifact_at(&directory, &current, &command.name, MAX_DOCUMENT_BYTES)?
        .descriptor
        != report.descriptor
    {
        return Err("The portfolio report changed during packaging".into());
    }
    let mut receipt = crate::application::export_bytes(
        &command.destination,
        &ds_solar_contracts::digest::sha256_hex(&bytes),
        &bytes,
    )?;
    receipt["portfolio_batch_id"] = batch.batch_id.into();
    receipt["portfolio_batch_digest"] = batch.batch_digest.into();
    receipt["source_batch_id"] = source_batch.batch_id.into();
    receipt["source_batch_digest"] = source_batch.batch_digest.into();
    receipt["source_content_digest"] = report.descriptor.content_digest.into();
    receipt["image_count"] = plan.images.len().into();
    receipt["media_byte_count"] = total.into();
    receipt["publication"] = "not_requested".into();
    Ok(receipt)
}

fn require_chart(
    artifact: &ds_solar_contracts::BatchArtifact,
    city: Option<&str>,
) -> Result<(), String> {
    if artifact.city_id.as_deref() != city
        || artifact.role
            != if city.is_some() {
                ArtifactRole::Chart
            } else {
                ArtifactRole::PortfolioChart
            }
        || !matches!(
            artifact.media_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp"
        )
    {
        return Err("The report image is outside its declared chart inventory".into());
    }
    Ok(())
}
