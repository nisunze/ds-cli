//! Independent reviewed city report closures, with the APD placement preserved.
use super::*;
use ds_solar_contracts::{ArtifactRole, BatchArtifact, SolarArtifactBatch};

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportCommand {
    pub batch: PortfolioBatchCommand,
    pub city_id: String,
    pub source_name: String,
    pub source_path: PathBuf,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub batch: PortfolioBatchCommand,
    pub city_id: String,
    pub name: String,
}
#[derive(Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Lineage {
    schema_version: String,
    source_batch_id: String,
    source_batch_digest: String,
    source_draft: BatchArtifact,
    source_report_input: BatchArtifact,
}

fn final_name(source: &str) -> Result<&'static str, String> {
    match source {
        PARITY_DRAFT_NAME => Ok(FINAL_APD_NAME),
        APD_DRAFT_EN_NAME => Ok(FINAL_APD_EN_NAME),
        NETWORK_DRAFT_NAME => Ok("network-final-fr.md"),
        NETWORK_DRAFT_EN_NAME => Ok("network-final-en.md"),
        PLANT_DRAFT_NAME => Ok("plant-final-fr.md"),
        PLANT_DRAFT_EN_NAME => Ok("plant-final-en.md"),
        FINANCIAL_DRAFT_NAME => Ok("financial-final-fr.md"),
        FINANCIAL_DRAFT_EN_NAME => Ok("financial-final-en.md"),
        _ => Err("Select a declared city authoring draft".into()),
    }
}

pub(super) fn source_name(name: &str) -> Option<&'static str> {
    [
        PARITY_DRAFT_NAME,
        APD_DRAFT_EN_NAME,
        NETWORK_DRAFT_NAME,
        NETWORK_DRAFT_EN_NAME,
        PLANT_DRAFT_NAME,
        PLANT_DRAFT_EN_NAME,
        FINANCIAL_DRAFT_NAME,
        FINANCIAL_DRAFT_EN_NAME,
    ]
    .into_iter()
    .find(|source| final_name(source).is_ok_and(|final_name| final_name == name))
}

fn source(
    app: &AppHandle,
    batch: &PortfolioBatchCommand,
) -> Result<(PathBuf, SolarArtifactBatch), String> {
    let directory = run_directory(app, &batch.workspace_handle, &batch.root, &batch.run_id)?;
    let closed = closed_batch_at(&directory)?;
    require_batch_selector(&closed, &batch.root, &batch.run_id)?;
    require_expected_portfolio_batch(
        &closed,
        &batch.expected_batch_id,
        &batch.expected_batch_digest,
    )?;
    Ok((directory, closed))
}

fn lineage(
    directory: &Path,
    batch: &SolarArtifactBatch,
    city: &str,
    name: &str,
) -> Result<Lineage, String> {
    final_name(name)?;
    let draft = verified_batch_artifact_at(
        directory,
        batch,
        &format!("cities/{city}/{name}"),
        MAX_DOCUMENT_BYTES,
    )?;
    let input = verified_batch_artifact_at(
        directory,
        batch,
        &format!("cities/{city}/{REPORT_INPUT_NAME}"),
        MAX_BATCH_ARTIFACT_BYTES,
    )?;
    for (file, role, media) in [
        (
            &draft,
            ArtifactRole::Report,
            ds_solar_contracts::artifact_names::MARKDOWN_MEDIA_TYPE,
        ),
        (&input, ArtifactRole::ReportInput, "application/json"),
    ] {
        if file.descriptor.role != role
            || file.descriptor.media_type != media
            || file.descriptor.city_id.as_deref() != Some(city)
        {
            return Err("The city final has an invalid source declaration".into());
        }
    }
    Ok(Lineage {
        schema_version: "ds-solar.city-report-final-lineage/v1".into(),
        source_batch_id: batch.batch_id.clone(),
        source_batch_digest: batch.batch_digest.clone(),
        source_draft: draft.descriptor,
        source_report_input: input.descriptor,
    })
}

pub(super) fn context(
    directory: &Path,
    source: &SolarArtifactBatch,
    city: &str,
    name: &str,
) -> Result<(PathBuf, SolarArtifactBatch), String> {
    checked_segment(city, "city id")?;
    let draft = source_name(name).ok_or("That is not a reviewed city report")?;
    if matches!(name, FINAL_APD_NAME | FINAL_APD_EN_NAME) {
        return final_context_at(directory, source, &format!("cities/{city}/{name}"));
    }
    let folder = draft
        .strip_suffix(".md")
        .ok_or("Invalid city source name")?;
    let final_dir = secure_existing_private_directory(
        &directory.join("finals-reports").join(city).join(folder),
    )?;
    let closed = closed_batch_at(&final_dir)?;
    let member = source
        .run
        .cities
        .iter()
        .find(|member| member.city_id == city)
        .ok_or("City is not declared by the source run")?;
    let mut expected = source.run.clone();
    expected.cities = vec![member.clone()];
    if closed.run != expected || closed.artifacts.len() != 2 {
        return Err("The city final closure does not match its source run and city".into());
    }
    let provenance = verified_batch_artifact_at(
        &final_dir,
        &closed,
        &format!("cities/{city}/source.json"),
        MAX_DOCUMENT_BYTES,
    )?;
    if provenance.descriptor.role != ArtifactRole::CityReportModel
        || provenance.descriptor.media_type != "application/json"
        || provenance.descriptor.city_id.as_deref() != Some(city)
    {
        return Err("The city final provenance declaration is invalid".into());
    }
    let saved: Lineage =
        serde_json::from_slice(&provenance.bytes).map_err(|_| "Invalid city final provenance")?;
    if saved != lineage(directory, source, city, draft)? {
        return Err(
            "The city final no longer matches its pinned source draft and report input".into(),
        );
    }
    let file = declared_batch_artifact(&closed, &format!("cities/{city}/{name}"))?;
    if file.role != ArtifactRole::Report
        || file.media_type != ds_solar_contracts::artifact_names::MARKDOWN_MEDIA_TYPE
        || file.city_id.as_deref() != Some(city)
    {
        return Err("The reviewed city report declaration is invalid".into());
    }
    Ok((final_dir, closed))
}

pub fn inspect(app: AppHandle, command: Selector) -> Result<ImportedFinal, String> {
    let (directory, source) = source(&app, &command.batch)?;
    let city = checked_segment(&command.city_id, "city id")?;
    let (_, closed) = context(&directory, &source, &city, &command.name)?;
    let artifact = verified_batch_artifact_at(
        &directory,
        &source,
        &format!("cities/{city}/{}", command.name),
        MAX_DOCUMENT_BYTES,
    )?;
    Ok(ImportedFinal {
        markdown: file_descriptor(
            &command.name,
            ds_solar_contracts::artifact_names::MARKDOWN_MEDIA_TYPE,
            &artifact.bytes,
        ),
        final_batch_id: closed.batch_id,
        final_batch_digest: closed.batch_digest,
        source_batch_id: source.batch_id,
        source_batch_digest: source.batch_digest,
        publication: None,
        publication_error: None,
        docx: None,
        docx_error: None,
    })
}

pub fn import(app: AppHandle, command: ImportCommand) -> Result<ImportedFinal, String> {
    let (directory, source) = source(&app, &command.batch)?;
    let city = checked_segment(&command.city_id, "city id")?;
    let name = final_name(&command.source_name)?;
    if matches!(name, FINAL_APD_NAME | FINAL_APD_EN_NAME) {
        let legacy = final_lineage_at(&directory, &source, &city)?;
        if legacy.source_draft.name != format!("cities/{city}/{}", command.source_name) {
            return Err("Select the exact APD language declared by the source batch".into());
        }
        return solar_native_import_final(
            app,
            ImportFinalCommand {
                workspace_handle: command.batch.workspace_handle,
                root: command.batch.root,
                run_id: command.batch.run_id,
                city_id: city,
                source_path: Some(
                    command
                        .source_path
                        .to_str()
                        .ok_or("Final source path must be UTF-8")?
                        .into(),
                ),
            },
        )?
        .ok_or_else(|| "An explicit reviewed city source is required".into());
    }
    if command.source_path.extension().and_then(|s| s.to_str()) != Some("md") {
        return Err("The reviewed city source must be a Markdown file".into());
    }
    let provenance = lineage(&directory, &source, &city, &command.source_name)?;
    let draft = verified_batch_artifact_at(
        &directory,
        &source,
        &provenance.source_draft.name,
        MAX_DOCUMENT_BYTES,
    )?;
    let bytes = read_bounded_regular_file(
        &command.source_path,
        "reviewed city report",
        MAX_DOCUMENT_BYTES,
    )?;
    let reviewed =
        std::str::from_utf8(&bytes).map_err(|_| "Reviewed city Markdown must be UTF-8")?;
    let lint = ds_solar_contracts::lint_final(
        std::str::from_utf8(&draft.bytes).map_err(|_| "City authoring draft must be UTF-8")?,
        reviewed,
    );
    if !lint.ok() {
        return Err(format!(
            "The reviewed city report violates its authoring contract:\n{}",
            lint.render()
        ));
    }
    let parent = secure_private_directory(&directory.join("finals-reports").join(&city))?;
    let folder = command
        .source_name
        .strip_suffix(".md")
        .ok_or("Invalid city source name")?;
    let committed = parent.join(folder);
    if std::fs::symlink_metadata(&committed).is_ok() {
        return Err(
            "This source report already has a reviewed final; use a new source run to replace it"
                .into(),
        );
    }
    let staging = parent.join(format!("{folder}-{}.partial", uuid::Uuid::new_v4()));
    secure_private_directory(&staging)?;
    let mut guard = PendingPortfolioDirectory::new(staging.clone());
    let member = source
        .run
        .cities
        .iter()
        .find(|member| member.city_id == city)
        .ok_or("City is not declared by the source run")?;
    let logical = format!("cities/{city}/{name}");
    let provenance_name = format!("cities/{city}/source.json");
    let lineage_bytes = json_bytes(&provenance)?;
    for (logical, body) in [
        (&logical, bytes.as_slice()),
        (&provenance_name, lineage_bytes.as_slice()),
    ] {
        let path = staging.join(logical);
        secure_private_directory(path.parent().ok_or("Invalid city final path")?)?;
        write_new_final_file(&path, body)?;
    }
    let mut run = source.run.clone();
    run.cities = vec![member.clone()];
    let closed = SolarArtifactBatch::close(
        run,
        vec![
            BatchArtifact {
                name: logical,
                role: ArtifactRole::Report,
                city_id: Some(city.clone()),
                media_type: ds_solar_contracts::artifact_names::MARKDOWN_MEDIA_TYPE.into(),
                byte_count: bytes.len() as u64,
                content_digest: ds_solar_contracts::digest::sha256_hex(&bytes),
            },
            BatchArtifact {
                name: provenance_name,
                role: ArtifactRole::CityReportModel,
                city_id: Some(city.clone()),
                media_type: "application/json".into(),
                byte_count: lineage_bytes.len() as u64,
                content_digest: ds_solar_contracts::digest::sha256_hex(&lineage_bytes),
            },
        ],
    )
    .map_err(|error| error.message)?;
    write_new_final_file(&staging.join("batch.json"), &json_bytes(&closed)?)?;
    write_new_final_file(
        &staging.join("sync-intent.json"),
        &json_bytes(&closed.sync_intent(rfc3339_now()))?,
    )?;
    if closed_batch_at(&directory)? != source
        || lineage(&directory, &source, &city, &command.source_name)? != provenance
    {
        return Err("The city source changed during final import".into());
    }
    for artifact in &closed.artifacts {
        verified_batch_artifact_at(&staging, &closed, &artifact.name, MAX_DOCUMENT_BYTES)?;
    }
    #[cfg(unix)]
    std::fs::File::open(&staging)
        .and_then(|file| file.sync_all())
        .map_err(|_| "Cannot persist reviewed city staging directory")?;
    guard.commit(&committed)?;
    #[cfg(unix)]
    std::fs::File::open(&parent)
        .and_then(|file| file.sync_all())
        .map_err(|_| "Reviewed city report committed but directory persistence failed")?;
    let mut imported = inspect(
        app.clone(),
        Selector {
            batch: command.batch,
            city_id: city.clone(),
            name: name.into(),
        },
    )?;
    (imported.publication, imported.publication_error) =
        publication::city_final_completed(&app, &directory, &source, &closed, &city, name);
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_solar_contracts::{BatchCity, BatchRunIdentity};

    #[test]
    fn six_reviewed_city_variants_keep_independent_closures_and_finish_after_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("authority");
        let queue = std::sync::Arc::new(super::super::tests::LocalPublicationStore(
            ds_sync_runtime::open_store(&tmp.path().join("sync.sqlite")).unwrap(),
        ));
        let host = AppHandle::headless(data.clone())
            .unwrap()
            .with_publication_provider(queue.clone());
        let workspace =
            crate::workspace::solar_native_prepare_default_workspace(host.clone()).unwrap();
        let path = resolve_workspace_handle(&host, &workspace.workspace_handle).unwrap();
        let root = "eds_project/project_test/eds_solar";
        let directory = secure_private_directory(
            &native_root(&path)
                .join("runs")
                .join(root)
                .join("city-reports"),
        )
        .unwrap();
        let city_dir = secure_private_directory(&directory.join("cities/town")).unwrap();
        let draft = "# City report fixture\n\n## Review\n\n<!-- LLM_OUTPUT_START -->\n[LLM interpretation will be inserted here]\n<!-- LLM_OUTPUT_END -->\n\n## A.6 Finance\n\nSealed fixture data.\n";
        let reviewed = draft.replace(
            "[LLM interpretation will be inserted here]",
            "Reviewed fixture narrative.",
        );
        let input = b"{\"fixture\":true}";
        let names = [
            NETWORK_DRAFT_NAME,
            NETWORK_DRAFT_EN_NAME,
            PLANT_DRAFT_NAME,
            PLANT_DRAFT_EN_NAME,
            FINANCIAL_DRAFT_NAME,
            FINANCIAL_DRAFT_EN_NAME,
        ];
        let mut artifacts = Vec::new();
        for (name, bytes, role, media) in names
            .into_iter()
            .map(|name| {
                (
                    name,
                    draft.as_bytes(),
                    ArtifactRole::Report,
                    ds_solar_contracts::artifact_names::MARKDOWN_MEDIA_TYPE,
                )
            })
            .chain(std::iter::once((
                REPORT_INPUT_NAME,
                input.as_slice(),
                ArtifactRole::ReportInput,
                "application/json",
            )))
        {
            write_new_final_file(&city_dir.join(name), bytes).unwrap();
            artifacts.push(BatchArtifact::from_entry(
                &ds_solar_contracts::ArtifactEntry::from_bytes(
                    format!("cities/town/{name}"),
                    media,
                    bytes,
                ),
                role,
                Some("town"),
            ));
        }
        let batch = SolarArtifactBatch::close(
            BatchRunIdentity {
                project_id: "project_test".into(),
                root: root.into(),
                run_id: "city-reports".into(),
                cities: vec![BatchCity {
                    city_id: "town".into(),
                    input_digest: ds_solar_contracts::digest::sha256_hex(b"input"),
                    result_digest: ds_solar_contracts::digest::sha256_hex(b"result"),
                }],
            },
            artifacts,
        )
        .unwrap();
        write_json(&directory.join("batch.json"), &batch).unwrap();
        let before = std::fs::read(directory.join("batch.json")).unwrap();
        let selector = || PortfolioBatchCommand {
            workspace_handle: workspace.workspace_handle.clone(),
            root: root.into(),
            run_id: "city-reports".into(),
            expected_batch_id: batch.batch_id.clone(),
            expected_batch_digest: batch.batch_digest.clone(),
        };
        let reviewed_path = tmp.path().join("reviewed.md");
        std::fs::write(&reviewed_path, &reviewed).unwrap();
        struct Renderer;
        impl crate::ReportRenderer for Renderer {
            fn render(
                &self,
                copy: ds_command_kernel::solar_report_bundle::RenderCopy,
                _: crate::ReportFormat,
            ) -> Result<Vec<u8>, String> {
                assert!(copy.markdown.contains("Reviewed fixture narrative."));
                Ok(b"PK\x03\x04fixture".to_vec())
            }
        }
        let restart = AppHandle::headless(data)
            .unwrap()
            .with_report_renderer(std::sync::Arc::new(Renderer));
        let mut closures = BTreeSet::new();
        for (index, name) in names.into_iter().enumerate() {
            let imported = import(
                host.clone(),
                ImportCommand {
                    batch: selector(),
                    city_id: "town".into(),
                    source_name: name.into(),
                    source_path: reviewed_path.clone(),
                },
            )
            .unwrap();
            assert!(
                imported.publication_error.is_none(),
                "{:?}",
                imported.publication_error
            );
            assert_eq!(imported.publication.as_ref().unwrap()["state"], "held");
            assert!(closures.insert(imported.final_batch_digest.clone()));
            assert_eq!(imported.source_batch_digest, batch.batch_digest);
            assert!(
                import(
                    host.clone(),
                    ImportCommand {
                        batch: selector(),
                        city_id: "town".into(),
                        source_name: name.into(),
                        source_path: reviewed_path.clone()
                    }
                )
                .is_err()
            );
            let final_name = final_name(name).unwrap();
            let fence = super::super::tests::LocalPublicationStore::fence();
            let rows = queue
                .0
                .lock()
                .unwrap()
                .snapshot(&fence, &ds_sync_runtime::rows::store_scope("project_test"))
                .unwrap()
                .artifacts;
            assert_eq!(rows.len(), index + 1);
            let row = rows
                .iter()
                .find(|r| r.identity.variant == final_name.trim_end_matches(".md"))
                .unwrap();
            assert_eq!(
                row.input_base_fingerprint.as_deref(),
                Some(ds_compute_runtime::digest(input).as_str())
            );
            let producer = ds_sync_runtime::retained_publication::RetainedProducer {
                store: &queue.0,
                fence: &fence,
                project: "project_test",
            };
            let local = ds_sync_runtime::rows::local_row_from_store(row);
            assert_eq!(
                producer
                    .output(&local, final_name.trim_end_matches(".md"))
                    .unwrap(),
                reviewed.as_bytes()
            );
            let inspected = inspect(
                restart.clone(),
                Selector {
                    batch: selector(),
                    city_id: "town".into(),
                    name: final_name.into(),
                },
            )
            .unwrap();
            assert_eq!(inspected.final_batch_digest, imported.final_batch_digest);
            assert!(inspected.publication.is_none());
            let finished = finishing::finish(
                restart.clone(),
                ReportFinishCommand {
                    source: ReportFinishSource::City {
                        batch: selector(),
                        city_id: "town".into(),
                        name: final_name.into(),
                    },
                    format: crate::ReportFormat::Docx,
                    destination: tmp.path().join(format!("report-{index}.docx")),
                },
            )
            .unwrap();
            assert_eq!(finished["publication"], "not_requested");
            let logical = format!("cities/town/{final_name}");
            let (final_dir, _) = context(&directory, &batch, "town", final_name).unwrap();
            std::fs::write(final_dir.join(&logical), "tampered final").unwrap();
            assert!(
                inspect(
                    restart.clone(),
                    Selector {
                        batch: selector(),
                        city_id: "town".into(),
                        name: final_name.into()
                    }
                )
                .is_err()
            );
            std::fs::write(final_dir.join(logical), &reviewed).unwrap();
        }
        assert_eq!(closures.len(), 6);
        assert_eq!(std::fs::read(directory.join("batch.json")).unwrap(), before);
        let mut foreign = selector();
        foreign.root = "eds_project/project_other/eds_solar".into();
        assert!(
            inspect(
                restart.clone(),
                Selector {
                    batch: foreign,
                    city_id: "town".into(),
                    name: "network-final-fr.md".into()
                }
            )
            .is_err()
        );
        let mut stale = selector();
        stale.expected_batch_digest = ds_solar_contracts::digest::sha256_hex(b"stale");
        assert!(
            inspect(
                restart,
                Selector {
                    batch: stale,
                    city_id: "town".into(),
                    name: "network-final-fr.md".into()
                }
            )
            .is_err()
        );
    }
}
