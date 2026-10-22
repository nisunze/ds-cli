//! Reviewed portfolio reports have independent immutable closures.
use super::*;
use ds_solar_contracts::{ArtifactRole, BatchArtifact, SolarArtifactBatch};

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortfolioFinalImportCommand {
    pub batch: PortfolioBatchCommand,
    pub source_name: String,
    pub source_path: PathBuf,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortfolioFinalSelector {
    pub batch: PortfolioBatchCommand,
    pub name: String,
}
#[derive(Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Lineage {
    schema_version: String,
    source_batch_id: String,
    source_batch_digest: String,
    source_draft: BatchArtifact,
    source_input: BatchArtifact,
    source_result: BatchArtifact,
}
pub(super) fn final_name(source: &str) -> Result<&'static str, String> {
    match source {
        "portfolio-draft-fr.md" => Ok("portfolio-final-fr.md"),
        "portfolio-draft-en.md" => Ok("portfolio-final-en.md"),
        "portfolio-reseau-fr.md" => Ok("portfolio-reseau-final-fr.md"),
        "portfolio-network-en.md" => Ok("portfolio-network-final-en.md"),
        "portfolio-centrale-fr.md" => Ok("portfolio-centrale-final-fr.md"),
        "portfolio-plant-en.md" => Ok("portfolio-plant-final-en.md"),
        "portfolio-financier-fr.md" => Ok("portfolio-financier-final-fr.md"),
        "portfolio-financial-en.md" => Ok("portfolio-financial-final-en.md"),
        _ => Err("Select a declared portfolio authoring draft".into()),
    }
}
pub(super) fn source_name(name: &str) -> Option<&'static str> {
    [
        "portfolio-draft-fr.md",
        "portfolio-draft-en.md",
        "portfolio-reseau-fr.md",
        "portfolio-network-en.md",
        "portfolio-centrale-fr.md",
        "portfolio-plant-en.md",
        "portfolio-financier-fr.md",
        "portfolio-financial-en.md",
    ]
    .into_iter()
    .find(|source| final_name(source).is_ok_and(|final_name| final_name == name))
}
fn lineage(
    directory: &Path,
    batch: &SolarArtifactBatch,
    source_name: &str,
) -> Result<Lineage, String> {
    final_name(source_name)?;
    let draft = verified_batch_artifact_at(directory, batch, source_name, MAX_DOCUMENT_BYTES)?;
    let input = verified_batch_artifact_at(
        directory,
        batch,
        "portfolio-run-input.json",
        MAX_DOCUMENT_BYTES,
    )?;
    let result =
        verified_batch_artifact_at(directory, batch, PORTFOLIO_RESULT_NAME, MAX_DOCUMENT_BYTES)?;
    for (artifact, role, media) in [
        (
            &draft,
            ArtifactRole::Report,
            ds_solar_contracts::artifact_names::MARKDOWN_MEDIA_TYPE,
        ),
        (&input, ArtifactRole::PortfolioRunInput, "application/json"),
        (&result, ArtifactRole::PortfolioResult, "application/json"),
    ] {
        if artifact.descriptor.role != role
            || artifact.descriptor.media_type != media
            || artifact.descriptor.city_id.is_some()
        {
            return Err("The portfolio final has an invalid source declaration".into());
        }
    }
    Ok(Lineage {
        schema_version: "ds-solar.portfolio-final-lineage/v1".into(),
        source_batch_id: batch.batch_id.clone(),
        source_batch_digest: batch.batch_digest.clone(),
        source_draft: draft.descriptor,
        source_input: input.descriptor,
        source_result: result.descriptor,
    })
}
fn source(
    app: &AppHandle,
    selector: &PortfolioBatchCommand,
) -> Result<(PathBuf, SolarArtifactBatch), String> {
    let directory = portfolio_run_directory(
        app,
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
    Ok((directory, batch))
}
pub(super) fn context(
    directory: &Path,
    source: &SolarArtifactBatch,
    name: &str,
) -> Result<(PathBuf, SolarArtifactBatch), String> {
    let source_name = source_name(name).ok_or("That is not a reviewed portfolio report")?;
    let folder = source_name
        .strip_suffix(".md")
        .ok_or("Invalid portfolio source name")?;
    let directory_final =
        secure_existing_private_directory(&directory.join("finals").join(folder))?;
    let batch = closed_batch_at(&directory_final)?;
    if batch.run != source.run || batch.artifacts.len() != 2 {
        return Err("The final closure does not match its source portfolio run".into());
    }
    let provenance = verified_batch_artifact_at(
        &directory_final,
        &batch,
        "portfolio-final-source.json",
        MAX_DOCUMENT_BYTES,
    )?;
    if provenance.descriptor.role != ArtifactRole::Export
        || provenance.descriptor.media_type != "application/json"
        || provenance.descriptor.city_id.is_some()
    {
        return Err("The portfolio final provenance declaration is invalid".into());
    }
    let stored: Lineage = serde_json::from_slice(&provenance.bytes)
        .map_err(|_| "Invalid portfolio final provenance")?;
    if stored != lineage(directory, source, source_name)? {
        return Err(
            "The final no longer matches its source portfolio input, result and draft".into(),
        );
    }
    let report = verified_batch_artifact_at(&directory_final, &batch, name, MAX_DOCUMENT_BYTES)?;
    if report.descriptor.role != ArtifactRole::Report
        || report.descriptor.media_type != ds_solar_contracts::artifact_names::MARKDOWN_MEDIA_TYPE
        || report.descriptor.city_id.is_some()
    {
        return Err("The portfolio final declaration is invalid".into());
    }
    Ok((directory_final, batch))
}
fn receipt(
    source: &SolarArtifactBatch,
    final_batch: SolarArtifactBatch,
    name: &str,
    bytes: &[u8],
) -> ImportedFinal {
    ImportedFinal {
        markdown: file_descriptor(
            name,
            ds_solar_contracts::artifact_names::MARKDOWN_MEDIA_TYPE,
            bytes,
        ),
        final_batch_id: final_batch.batch_id,
        final_batch_digest: final_batch.batch_digest,
        source_batch_id: source.batch_id.clone(),
        source_batch_digest: source.batch_digest.clone(),
        publication: None,
        publication_error: None,
        docx: None,
        docx_error: None,
    }
}
pub(super) fn inspect(
    app: AppHandle,
    command: PortfolioFinalSelector,
) -> Result<ImportedFinal, String> {
    let (directory, source) = source(&app, &command.batch)?;
    let (final_directory, batch) = context(&directory, &source, &command.name)?;
    let report =
        verified_batch_artifact_at(&final_directory, &batch, &command.name, MAX_DOCUMENT_BYTES)?;
    Ok(receipt(&source, batch, &command.name, &report.bytes))
}
pub(super) fn import(
    app: AppHandle,
    command: PortfolioFinalImportCommand,
) -> Result<ImportedFinal, String> {
    let (directory, source) = source(&app, &command.batch)?;
    let name = final_name(&command.source_name)?;
    let expected = lineage(&directory, &source, &command.source_name)?;
    if command.source_path.extension().and_then(|s| s.to_str()) != Some("md") {
        return Err("The reviewed portfolio report must be Markdown".into());
    }
    let bytes = read_bounded_regular_file(
        &command.source_path,
        "reviewed portfolio report",
        2 * 1024 * 1024,
    )?;
    if bytes.is_empty() {
        return Err("The reviewed portfolio report cannot be empty".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "Reviewed Markdown must be UTF-8")?;
    let draft = verified_batch_artifact_at(
        &directory,
        &source,
        &command.source_name,
        MAX_DOCUMENT_BYTES,
    )?;
    let lint = ds_solar_contracts::lint_final(
        std::str::from_utf8(&draft.bytes).map_err(|_| "Portfolio draft must be UTF-8")?,
        text,
    );
    if !lint.ok() {
        return Err(format!(
            "The reviewed portfolio report violates its authoring contract:\n{}",
            lint.render()
        ));
    }
    let parent = secure_private_directory(&directory.join("finals"))?;
    let folder = command
        .source_name
        .strip_suffix(".md")
        .ok_or("Invalid portfolio source name")?;
    let committed = parent.join(folder);
    if std::fs::symlink_metadata(&committed).is_ok() {
        return Err("This source report already has an immutable final; calculate a new portfolio run to replace it".into());
    }
    let staging = parent.join(format!(".portfolio-final-{}", uuid::Uuid::new_v4()));
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
        .map_err(|e| format!("Could not stage portfolio final: {e}"))?;
    let mut pending = PendingPortfolioDirectory::new(staging.clone());
    let provenance = json_bytes(&expected)?;
    write_new_final_file(&staging.join(name), &bytes)?;
    write_new_final_file(&staging.join("portfolio-final-source.json"), &provenance)?;
    let artifacts = [
        (
            name,
            ds_solar_contracts::artifact_names::MARKDOWN_MEDIA_TYPE,
            &bytes,
            ArtifactRole::Report,
        ),
        (
            "portfolio-final-source.json",
            "application/json",
            &provenance,
            ArtifactRole::Export,
        ),
    ]
    .into_iter()
    .map(|(name, media, bytes, role)| {
        BatchArtifact::from_entry(
            &ds_solar_contracts::ArtifactEntry::from_bytes(name, media, bytes),
            role,
            None,
        )
    })
    .collect();
    let batch = SolarArtifactBatch::close(source.run.clone(), artifacts).map_err(|e| e.message)?;
    write_new_final_file(&staging.join("batch.json"), &json_bytes(&batch)?)?;
    write_new_final_file(
        &staging.join("sync-intent.json"),
        &json_bytes(&batch.sync_intent(rfc3339_now()))?,
    )?;
    for artifact in &batch.artifacts {
        verified_batch_artifact_at(&staging, &batch, &artifact.name, MAX_DOCUMENT_BYTES)?;
    }
    if closed_batch_at(&directory)? != source
        || lineage(&directory, &source, &command.source_name)? != expected
    {
        return Err("The portfolio source changed during final import".into());
    }
    #[cfg(unix)]
    std::fs::File::open(&staging)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("Could not persist final directory: {e}"))?;
    pending.commit(&committed)?;
    #[cfg(unix)]
    std::fs::File::open(&parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("Final committed but directory persistence failed: {e}"))?;
    Ok(receipt(&source, batch, name, &bytes))
}
