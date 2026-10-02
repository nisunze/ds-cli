//! Receipt-bound, read-only native report inventory. No recovery/adoption,
//! cleanup, publication pump, remote read or credential belongs to this read.
use std::{ffi::OsStr, path::Path};

use ds_command_kernel::{
    report::{artifacts::Artifact, inventory::*},
    sync_store::{ArtifactRow, HeadOutput},
};

use crate::{confined_fs::HeldDirectory, publication, *};

/// Read one authenticated host's exact project/transformer. The caller obtains
/// `rows` under the same owner/deployment/install fence from its shared store.
/// The current catalogue order is inherited from the existing native reader;
/// filesystem times never become generation timestamps in this projection.
pub fn read_snapshot(
    root: &Path,
    owner: &str,
    project: &str,
    transformer: &str,
    rows: &[ArtifactRow],
) -> Result<Snapshot, String> {
    if !valid_semantic_text(owner, 128)
        || !valid_semantic_text(project, 128)
        || !valid_semantic_text(transformer, 128)
    {
        return Err("Report inventory requires an exact native owner/project/transformer".into());
    }
    let operation = format!("export-{transformer}");
    let mut snapshot = Snapshot {
        owner: owner.into(),
        project: project.into(),
        transformer: transformer.into(),
        variant: "default".into(),
        expected_input_base_fingerprint: None,
        local_read: ReadFact {
            state: ReadState::Ready,
            error: None,
        },
        sync_read: ReadFact {
            state: ReadState::Ready,
            error: None,
        },
        cloud_read: ReadFact {
            state: ReadState::NotObserved,
            error: None,
        },
        local_sets: vec![],
        sync_artifacts: vec![],
        cloud_head: None,
        legacy_cloud: None,
        required_outputs: vec![],
    };
    ds_command_kernel::report::inventory::project(&snapshot)?;
    // A terminal stale row may have already lost its bytes_locator. Keep its
    // explicit outcome only when an exact still-held receipt binds its batch.
    let Some(pending_root) = HeldDirectory::open_absolute(root)? else {
        return Ok(snapshot);
    };
    let catalogue =
        list_project_local_artifacts_from_root(root, owner, project, Some(transformer))?;
    if catalogue.len() > 4096
        || catalogue
            .iter()
            .map(|set| set.artifacts.len())
            .sum::<usize>()
            > 32
    {
        return Err("Report inventory exceeds its bounded native catalogue".into());
    }
    for selected in catalogue {
        let first = selected
            .artifacts
            .first()
            .ok_or("Report inventory has no selected output")?;
        let entry = pending_root
            .open_child(OsStr::new(locator_id(&first.locator)?))?
            .ok_or("Report inventory committed bytes are missing")?;
        let artifact_receipt = read_receipt(&entry, &first.locator)?;
        let receipt = publication::committed_receipt(&pending_root, &artifact_receipt.batch_id)?
            .ok_or("Report inventory commit disappeared during its read")?;
        if receipt.owner_uid != owner
            || receipt.project_id != project
            || receipt.transformer != transformer
            || receipt.operation != operation
            || receipt.variant != "default"
            || receipt.client_run_id != selected.client_run_id
        {
            return Err("Report inventory receipt changed its native scope".into());
        }
        let mut artifacts = Vec::with_capacity(selected.artifacts.len());
        for output in selected.artifacts {
            let entry = pending_root
                .open_child(OsStr::new(locator_id(&output.locator)?))?
                .ok_or("Report inventory committed bytes are missing")?;
            let artifact_receipt = read_receipt(&entry, &output.locator)?;
            if artifact_receipt.batch_id != receipt.batch_id
                || artifact_receipt.output_id != output.output_id
                || artifact_receipt.sha256 != output.sha256
                || artifact_receipt.size_bytes != output.size_bytes
                || publication::require_committed_output(
                    &pending_root,
                    &receipt.batch_id,
                    &output.locator,
                    &output.output_id,
                    &output.sha256,
                    output.size_bytes,
                )? != receipt
            {
                return Err("Report inventory artifact changed its committed pins".into());
            }
            let mut file =
                entry.open_regular_file(OsStr::new(ARTIFACT_FILE), "Report inventory artifact")?;
            verify_open_artifact(&mut file, output.size_bytes, &output.sha256, false)?;
            artifacts.push(Artifact {
                output_id: output.output_id,
                locator: output.locator,
                filename: output.filename,
                format: output.format,
                content_type: output.content_type,
                sha256: output.sha256,
                size_bytes: output.size_bytes,
                paper_size: output.paper_size,
                presentation: output.presentation,
            });
        }
        let row = rows.iter().find(|row| {
            row.scope.project() == project
                && row.identity.engine == "network_reporter"
                && row.identity.operation == operation
                && row.identity.variant == "default"
                && row.client_publish_id == receipt.client_publish_id
                && row.input_base_fingerprint.is_some()
                && (row.bytes_locator == format!("network_reporter:batch:{}", receipt.batch_id)
                    || (row.bytes_locator.is_empty()
                        && !row.readable
                        && row.outputs.len() == receipt.outputs.len()
                        && receipt.outputs.iter().all(|output| {
                            row.outputs.iter().any(|stored| {
                                stored.output_id == output.output_id
                                    && stored.sha256 == output.sha256
                                    && stored.size_bytes == output.size_bytes
                            })
                        })))
        });
        if let Some(row) = row {
            let complete_row_pins_match = committed_row_pins_match(&receipt, row);
            snapshot.sync_artifacts.push(SyncArtifact {
                owner: owner.into(),
                project: project.into(),
                operation: operation.clone(),
                variant: "default".into(),
                batch_id: receipt.batch_id.clone(),
                client_publish_id: row.client_publish_id.clone(),
                input_base_fingerprint: row.input_base_fingerprint.clone().unwrap_or_default(),
                state: row.state,
                acknowledged_work_id: complete_row_pins_match
                    .then(|| row.acknowledged_work_id.clone())
                    .flatten(),
                updated_at_ms: row.updated_at_ms,
                reason: row.state_reason.clone(),
                outputs: row
                    .outputs
                    .iter()
                    // The catalogue trims an older commit to surviving output
                    // indices. Preserve its state for that exact subset only
                    // after checking the FULL stored declaration against the
                    // committed receipt; extra or duplicate pins cannot hide.
                    .filter(|output| {
                        !complete_row_pins_match
                            || artifacts
                                .iter()
                                .any(|artifact| artifact.output_id == output.output_id)
                    })
                    .map(|output| HeadOutput {
                        output_id: output.output_id.clone(),
                        sha256: output.sha256.clone(),
                        size_bytes: output.size_bytes,
                    })
                    .collect(),
            });
        }
        snapshot.local_sets.push(CommittedSet {
            owner: owner.into(),
            project: project.into(),
            transformer: transformer.into(),
            operation: operation.clone(),
            variant: "default".into(),
            batch_id: receipt.batch_id,
            client_publish_id: receipt.client_publish_id,
            client_run_id: receipt.client_run_id,
            engine_release: receipt.engine_version,
            engine_build_manifest_sha256: (!receipt.engine_build_manifest_sha256.is_empty())
                .then_some(receipt.engine_build_manifest_sha256),
            input_base_fingerprint: receipt.input_base_fingerprint,
            generated_at_ms: None,
            artifacts,
        });
    }
    // Reuse the portable owner to validate every assembled shape before IPC.
    ds_command_kernel::report::inventory::project(&snapshot)?;
    Ok(snapshot)
}

fn committed_row_pins_match(receipt: &PublicationBatchReceipt, row: &ArtifactRow) -> bool {
    let mut ids = std::collections::BTreeSet::new();
    row.input_base_fingerprint.as_deref() == Some(receipt.input_base_fingerprint.as_str())
        && row.engine_release == receipt.engine_version
        && row.engine_build_manifest_sha256.as_deref()
            == Some(receipt.engine_build_manifest_sha256.as_str())
                .filter(|digest| !digest.is_empty())
        && row.sha256
            == ds_command_kernel::sync_store::inventory_digest(
                &receipt
                    .outputs
                    .iter()
                    .map(|output| (output.output_id.clone(), output.sha256.clone()))
                    .collect::<Vec<_>>(),
            )
        && row.size_bytes
            == receipt
                .outputs
                .iter()
                .map(|output| output.size_bytes)
                .sum::<u64>()
        && row.outputs.len() == receipt.outputs.len()
        && row.outputs.iter().all(|stored| {
            ids.insert(&stored.output_id)
                && receipt.outputs.iter().any(|output| {
                    stored.output_id == output.output_id
                        && stored.sha256 == output.sha256
                        && stored.size_bytes == output.size_bytes
                        && stored.format == output.format
                        && stored.content_type == output.content_type
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_command_kernel::{report, sync, sync_store::*};
    use serde_json::json;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!(
                    "ds-report-inventory-{}-{}",
                    std::process::id(),
                    rand::random::<u64>()
                ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn commit(root: &Path, batch: char, ids: &[&str]) -> PublicationBatchReceipt {
        // The existing catalogue's millisecond order is distinct from a
        // generation timestamp. Do not rely on tied filesystem clock ticks.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let batch_id = batch.to_string().repeat(64);
        let pending = HeldDirectory::ensure_absolute(root).unwrap();
        let mut outputs = vec![];
        for (index, id) in ids.iter().enumerate() {
            let bytes = format!("{batch}-{id}-native-report").into_bytes();
            let locator_id =
                format!("{batch}{}", format!("{index:02x}").repeat(32))[..64].to_string();
            let locator = format!("ds-reporter:v1:{locator_id}");
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            let dir = pending.create_child(OsStr::new(&locator_id)).unwrap();
            std::fs::write(root.join(&locator_id).join(ARTIFACT_FILE), &bytes).unwrap();
            std::fs::write(
                root.join(&locator_id).join(RECEIPT_FILE),
                serde_json::to_vec(&NetworkReporterArtifactReceipt {
                    schema: RECEIPT_SCHEMA,
                    batch_id: batch_id.clone(),
                    output_id: (*id).into(),
                    locator: locator.clone(),
                    artifact_file: ARTIFACT_FILE.into(),
                    size_bytes: bytes.len() as u64,
                    sha256: sha256.clone(),
                })
                .unwrap(),
            )
            .unwrap();
            drop(dir);
            let metadata = report::artifacts::output(id).unwrap();
            outputs.push(PublicationOutput {
                output_id: (*id).into(),
                locator,
                filename: format!("tx_1{}", metadata.suffix),
                format: metadata.format,
                content_type: metadata.content_type.into(),
                sha256,
                size_bytes: bytes.len() as u64,
                paper_size: id
                    .starts_with("pdf__")
                    .then(|| if id.ends_with("a0l") { "a0" } else { "a3" }.into()),
                presentation: None,
            });
        }
        let receipt = PublicationBatchReceipt {
            schema: publication::BATCH_SCHEMA,
            batch_id,
            owner_uid: "owner-1".into(),
            organization_id: None,
            project_id: "project-1".into(),
            root: "eds_project/project-1/eds_design".into(),
            client_run_id: format!("report-{}", batch.to_string().repeat(32)),
            engine: "network_reporter".into(),
            operation: "export-tx_1".into(),
            variant: "default".into(),
            engine_version: format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
            engine_build_manifest_sha256: "d".repeat(64),
            transformer: "tx_1".into(),
            transformer_revision: 7,
            input_base_fingerprint: "b".repeat(64),
            room_content_sha256: "c".repeat(64),
            grant_id: None,
            snapshot_base: BTreeMap::new(),
            client_publish_id: format!("publish-123e4567-e89b-42d3-a456-42661417400{batch}"),
            outputs,
        };
        PendingPublicationCommit::stage(&pending, &receipt)
            .unwrap()
            .commit()
            .unwrap();
        receipt
    }

    fn row(receipt: &PublicationBatchReceipt) -> ArtifactRow {
        ArtifactRow {
            scope: Scope::Project {
                project: "project-1".into(),
            },
            identity: sync::Identity {
                engine: "network_reporter".into(),
                operation: "export-tx_1".into(),
                variant: "default".into(),
            },
            sha256: ds_command_kernel::sync_store::inventory_digest(
                &receipt
                    .outputs
                    .iter()
                    .map(|output| (output.output_id.clone(), output.sha256.clone()))
                    .collect::<Vec<_>>(),
            ),
            size_bytes: receipt.outputs.iter().map(|r| r.size_bytes).sum(),
            produced_at_ms: 111,
            base_revision: None,
            input_base_fingerprint: Some(receipt.input_base_fingerprint.clone()),
            engine_release: receipt.engine_version.clone(),
            engine_build_manifest_sha256: Some(receipt.engine_build_manifest_sha256.clone()),
            grant_engine: None,
            resource: Some("tx_1".into()),
            client_publish_id: receipt.client_publish_id.clone(),
            acknowledged_work_id: None,
            outputs: receipt
                .outputs
                .iter()
                .map(|output| ArtifactOutput {
                    output_id: output.output_id.clone(),
                    format: output.format.clone(),
                    content_type: output.content_type.clone(),
                    sha256: output.sha256.clone(),
                    size_bytes: output.size_bytes,
                })
                .collect(),
            bytes_locator: format!("network_reporter:batch:{}", receipt.batch_id),
            readable: true,
            state: ArtifactState::Held,
            state_reason: None,
            replay_key: receipt.client_publish_id.clone(),
            updated_at_ms: 999,
            transfer_state: None,
        }
    }

    #[test]
    fn native_inventory_reads_six_real_held_outputs_across_commits_without_date_or_ack_invention() {
        let fixture = Fixture::new();
        commit(&fixture.0, 'a', &["shp", "kmz", "xlsx", "geojson"]);
        let recent = commit(&fixture.0, 'b', &["pdf__a3l", "pdf__a0l"]);
        let mut stored = row(&recent);
        stored.state = ArtifactState::Published;
        let before = std::fs::read(
            fixture
                .0
                .join(publication::BATCH_DIRECTORY)
                .join(&recent.batch_id)
                .join(publication::BATCH_RECEIPT_FILE),
        )
        .unwrap();
        let snapshot =
            read_snapshot(&fixture.0, "owner-1", "project-1", "tx_1", &[stored]).unwrap();
        let view = serde_json::to_value(report::inventory::project(&snapshot).unwrap()).unwrap();
        assert_eq!(view["local_output_count"], 6);
        assert_eq!(view["published_output_count"], 0);
        assert_eq!(view["active_client_publish_id"], recent.client_publish_id);
        assert!(view["generated_at_ms"].is_null());
        assert!(view["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["generated_at_ms"].is_null()));
        assert!(snapshot.sync_artifacts[0].acknowledged_work_id.is_none());
        assert_eq!(
            std::fs::read(
                fixture
                    .0
                    .join(publication::BATCH_DIRECTORY)
                    .join(&recent.batch_id)
                    .join(publication::BATCH_RECEIPT_FILE)
            )
            .unwrap(),
            before
        );
        assert!(
            read_snapshot(&fixture.0, "other-owner", "project-1", "tx_1", &[])
                .unwrap()
                .local_sets
                .is_empty()
        );
        assert!(
            read_snapshot(&fixture.0, "owner-1", "other-project", "tx_1", &[])
                .unwrap()
                .local_sets
                .is_empty()
        );
    }

    #[test]
    fn bound_upload_ack_reaches_physical_inventory_and_still_requires_exact_cloud_origins() {
        use ds_command_kernel::compute_artifact_inventory::{Declaration, Head, OutputOrigin};
        use report::inventory::{CloudHead, ReadFact, ReadState};
        let fixture = Fixture::new();
        let artifact_root = fixture.0.join("report-artifacts");
        let receipt = commit(
            &artifact_root,
            'a',
            &["shp", "kmz", "xlsx", "geojson", "pdf__a3l"],
        );
        let fence = Fence {
            account: "owner-1".into(),
            deployment: "fixture-deployment".into(),
            install_id: "fixture-install".into(),
        };
        let scope = Scope::Project {
            project: "project-1".into(),
        };
        let database = fixture.0.join("store.sqlite");
        let mut store = ds_sync_store::Store::open(&database).unwrap();
        store
            .apply(&fence, 1, Event::LocalProduced { row: row(&receipt) })
            .unwrap();
        let acknowledgement = ReceiptRow {
            receipt_id: "bound-ACK".into(),
            scope: scope.clone(),
            action: "upload".into(),
            identity: Some(row(&receipt).identity),
            outcome: "committed".into(),
            detail: None,
            grant_id: None,
            work_id: Some("exact-work".into()),
            client_publish_id: Some(receipt.client_publish_id.clone()),
            head_revision: Some("exact-head".into()),
            head_sha256: Some(row(&receipt).sha256),
            head_counter: Some(1),
            committed_bytes: None,
            at_ms: 2,
        };
        let mut historical_receipt = acknowledgement.clone();
        historical_receipt.receipt_id = "historical-identity-only".into();
        historical_receipt.client_publish_id = None;
        let refused = store
            .apply(
                &fence,
                2,
                Event::ActionReceipt {
                    receipt: historical_receipt,
                },
            )
            .unwrap();
        assert_eq!(refused.refusals[0].code, "publication_replaced");
        let before_ack = store.snapshot(&fence, &scope).unwrap();
        assert!(before_ack.heads.is_empty());
        let historical = read_snapshot(
            &artifact_root,
            "owner-1",
            "project-1",
            "tx_1",
            &before_ack.artifacts,
        )
        .unwrap();
        assert!(historical.sync_artifacts[0].acknowledged_work_id.is_none());
        assert!(store
            .apply(
                &fence,
                3,
                Event::ActionReceipt {
                    receipt: acknowledgement
                }
            )
            .unwrap()
            .refusals
            .is_empty());
        drop(store);
        let stored = ds_sync_store::Store::open_read_only(&database)
            .unwrap()
            .unwrap()
            .snapshot(&fence, &scope)
            .unwrap()
            .artifacts;
        let mut snapshot =
            read_snapshot(&artifact_root, "owner-1", "project-1", "tx_1", &stored).unwrap();
        assert_eq!(
            snapshot.sync_artifacts[0].acknowledged_work_id.as_deref(),
            Some("exact-work")
        );
        let view = serde_json::to_value(report::inventory::project(&snapshot).unwrap()).unwrap();
        assert_eq!(view["local_output_count"], 5);
        assert_eq!(
            view["published_output_count"], 0,
            "a native ACK alone is not cloud readback"
        );
        snapshot.cloud_read = ReadFact {
            state: ReadState::Ready,
            error: None,
        };
        snapshot.cloud_head = Some(CloudHead {
            owner: "owner-1".into(),
            project: "project-1".into(),
            head: Head {
                work_id: "exact-work".into(),
                engine: "network_reporter".into(),
                engine_version: Some(receipt.engine_version.clone()),
                engine_build_manifest_sha256: Some(receipt.engine_build_manifest_sha256.clone()),
                operation: receipt.operation.clone(),
                variant: "default".into(),
                input_base_fingerprint: receipt.input_base_fingerprint.clone(),
                head_revision: 1,
                updated_at: "2026-10-02T03:00:00Z".into(),
                outputs: receipt
                    .outputs
                    .iter()
                    .map(|output| Declaration {
                        output_id: output.output_id.clone(),
                        format: output.format.clone(),
                        content_type: output.content_type.clone(),
                        sha256: output.sha256.clone(),
                        size_bytes: output.size_bytes,
                        paper_size: output.paper_size.clone(),
                        presentation: None,
                    })
                    .collect(),
                output_origins: Some(
                    receipt
                        .outputs
                        .iter()
                        .map(|output| {
                            (
                                output.output_id.clone(),
                                OutputOrigin {
                                    work_id: "exact-work".into(),
                                    input_base_fingerprint: receipt.input_base_fingerprint.clone(),
                                    engine_version: Some(receipt.engine_version.clone()),
                                    engine_build_manifest_sha256: Some(
                                        receipt.engine_build_manifest_sha256.clone(),
                                    ),
                                },
                            )
                        })
                        .collect(),
                ),
            },
        });
        let view = serde_json::to_value(report::inventory::project(&snapshot).unwrap()).unwrap();
        assert_eq!(view["published_output_count"], 5);
        // A different work with identical bytes is not this publication's ACK.
        let head = &mut snapshot.cloud_head.as_mut().unwrap().head;
        head.work_id = "other-work".into();
        for origin in head.output_origins.as_mut().unwrap().values_mut() {
            origin.work_id = "other-work".into();
        }
        assert_eq!(
            serde_json::to_value(report::inventory::project(&snapshot).unwrap()).unwrap()
                ["published_output_count"],
            0
        );
        // Full native declaration validation also fences the ACK itself.
        let mut unpinned = stored.clone();
        unpinned[0].outputs[0].sha256 = "f".repeat(64);
        let snapshot =
            read_snapshot(&artifact_root, "owner-1", "project-1", "tx_1", &unpinned).unwrap();
        assert!(snapshot.sync_artifacts[0].acknowledged_work_id.is_none());
        assert_eq!(
            serde_json::to_value(report::inventory::project(&snapshot).unwrap()).unwrap()
                ["published_output_count"],
            0
        );
        let mut wrong_engine = stored.clone();
        wrong_engine[0].engine_release = format!("ds-network-reporter@0.1.0+{}", "f".repeat(40));
        let snapshot = read_snapshot(
            &artifact_root,
            "owner-1",
            "project-1",
            "tx_1",
            &wrong_engine,
        )
        .unwrap();
        assert!(snapshot.sync_artifacts[0].acknowledged_work_id.is_none());
    }

    #[test]
    fn native_inventory_refuses_corrupt_or_missing_current_bytes_without_old_output_fallback() {
        let fixture = Fixture::new();
        let old = commit(&fixture.0, 'a', &["xlsx"]);
        let current = commit(&fixture.0, 'b', &["xlsx"]);
        // An old shadowed output can be gone; it is never re-read or resurrected.
        std::fs::remove_dir_all(fixture.0.join(locator_id(&old.outputs[0].locator).unwrap()))
            .unwrap();
        assert_eq!(
            read_snapshot(&fixture.0, "owner-1", "project-1", "tx_1", &[])
                .unwrap()
                .local_sets
                .len(),
            1
        );
        std::fs::write(
            fixture
                .0
                .join(locator_id(&current.outputs[0].locator).unwrap())
                .join(ARTIFACT_FILE),
            b"corruption",
        )
        .unwrap();
        assert!(read_snapshot(&fixture.0, "owner-1", "project-1", "tx_1", &[]).is_err());
        std::fs::remove_dir_all(
            fixture
                .0
                .join(locator_id(&current.outputs[0].locator).unwrap()),
        )
        .unwrap();
        assert!(read_snapshot(&fixture.0, "owner-1", "project-1", "tx_1", &[]).is_err());
    }

    #[test]
    fn native_inventory_never_binds_foreign_or_unpinned_store_rows_to_a_commit() {
        let fixture = Fixture::new();
        let receipt = commit(&fixture.0, 'c', &["xlsx"]);
        let mut stored = row(&receipt);
        stored.scope = Scope::Project {
            project: "other-project".into(),
        };
        assert!(
            read_snapshot(&fixture.0, "owner-1", "project-1", "tx_1", &[stored])
                .unwrap()
                .sync_artifacts
                .is_empty()
        );
        let mut stored = row(&receipt);
        stored.client_publish_id = "another-publication".into();
        assert!(
            read_snapshot(&fixture.0, "owner-1", "project-1", "tx_1", &[stored])
                .unwrap()
                .sync_artifacts
                .is_empty()
        );
        let mut stored = row(&receipt);
        stored.bytes_locator = "network_reporter:batch:wrong".into();
        assert!(
            read_snapshot(&fixture.0, "owner-1", "project-1", "tx_1", &[stored])
                .unwrap()
                .sync_artifacts
                .is_empty()
        );
        let mut stored = row(&receipt);
        stored.input_base_fingerprint = None;
        assert!(
            read_snapshot(&fixture.0, "owner-1", "project-1", "tx_1", &[stored])
                .unwrap()
                .sync_artifacts
                .is_empty()
        );
        let mut stored = row(&receipt);
        stored.readable = false;
        stored.bytes_locator.clear();
        stored.state = ArtifactState::Conflict;
        let snapshot =
            read_snapshot(&fixture.0, "owner-1", "project-1", "tx_1", &[stored]).unwrap();
        assert_eq!(
            serde_json::to_value(report::inventory::project(&snapshot).unwrap()).unwrap()
                ["outputs"][0]["publication_phase"],
            json!("conflicted")
        );
    }

    #[test]
    fn retained_output_subset_keeps_exact_prior_pins_and_current_store_has_one_row() {
        let fixture = Fixture::new();
        let artifact_root = fixture.0.join("report-artifacts");
        let old = commit(&artifact_root, 'a', &["shp", "kmz", "xlsx", "geojson"]);
        let fence = Fence {
            account: "owner-1".into(),
            deployment: "fixture-deployment".into(),
            install_id: "fixture-install".into(),
        };
        let scope = Scope::Project {
            project: "project-1".into(),
        };
        let database = fixture.0.join("store.sqlite");
        let mut store = ds_sync_store::Store::open(&database).unwrap();
        assert!(store
            .apply(&fence, 1, Event::LocalProduced { row: row(&old) })
            .unwrap()
            .refusals
            .is_empty());
        let first = store.snapshot(&fence, &scope).unwrap().artifacts;
        let recent = commit(
            &artifact_root,
            'b',
            &["xlsx", "geojson", "pdf__a3l", "pdf__a0l"],
        );
        assert!(store
            .apply(&fence, 2, Event::LocalProduced { row: row(&recent) })
            .unwrap()
            .refusals
            .is_empty());
        drop(store);
        let current = ds_sync_store::Store::open_read_only(&database)
            .unwrap()
            .unwrap()
            .snapshot(&fence, &scope)
            .unwrap()
            .artifacts;
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].client_publish_id, recent.client_publish_id);
        // Previously observed durable facts may be retained by a host. They
        // are not invented history in the current canonical Store.
        let mut retained = first.clone();
        retained.extend(current.clone());
        let snapshot =
            read_snapshot(&artifact_root, "owner-1", "project-1", "tx_1", &retained).unwrap();
        let view = serde_json::to_value(report::inventory::project(&snapshot).unwrap()).unwrap();
        let outputs = view["outputs"].as_array().unwrap();
        assert_eq!(outputs.len(), 6);
        assert!(outputs
            .iter()
            .all(|output| output["publication_phase"] == "queued"));
        for output in outputs {
            let prior = matches!(output["output_id"].as_str().unwrap(), "shp" | "kmz");
            assert_eq!(
                output["client_publish_id"].as_str(),
                if prior {
                    Some(old.client_publish_id.as_str())
                } else {
                    Some(recent.client_publish_id.as_str())
                }
            );
        }
        let snapshot =
            read_snapshot(&artifact_root, "owner-1", "project-1", "tx_1", &current).unwrap();
        let view = serde_json::to_value(report::inventory::project(&snapshot).unwrap()).unwrap();
        for output in view["outputs"].as_array().unwrap() {
            assert_eq!(
                output["publication_phase"],
                if matches!(output["output_id"].as_str().unwrap(), "shp" | "kmz") {
                    "sealed"
                } else {
                    "queued"
                }
            );
        }
        for corrupt in ["duplicate", "unmatched", "foreign"] {
            let mut rows = retained.clone();
            match corrupt {
                "duplicate" => rows[0].outputs[1] = rows[0].outputs[0].clone(),
                "unmatched" => rows[0].outputs[2].sha256 = "f".repeat(64),
                _ => rows[0].client_publish_id = "foreign-publication".into(),
            }
            match read_snapshot(&artifact_root, "owner-1", "project-1", "tx_1", &rows) {
                Err(_) => assert_eq!(corrupt, "duplicate"),
                Ok(snapshot) => {
                    let view = serde_json::to_value(report::inventory::project(&snapshot).unwrap())
                        .unwrap();
                    let old_output = view["outputs"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|output| output["output_id"] == "shp")
                        .unwrap();
                    assert_eq!(
                        old_output["publication_phase"],
                        if corrupt == "foreign" {
                            "sealed"
                        } else {
                            "evidence_mismatch"
                        }
                    );
                }
            }
        }
    }
}
