//! The report publication producer, shared by native hosts: it seals bytes
//! and records the row in the same step, moves bytes the store's rows name,
//! and frees the bytes of rows the store has settled as lost.
//!
//! The sync store is the queue (owner, 2026-09-20: "what has to be in SQLite
//! has to go there"). Nothing here enumerates the artifact directory to
//! decide what is queued; the directory is bytes, and every decision reads
//! the row.
use crate::{
    kernel_sync as sync, store, LocalOutput, LocalRow, Producer, SharedStore, TransferReceipt,
    MAX_ROWS,
};
use ds_report_artifacts::publication::PublicationBatchReceipt;
use ds_report_artifacts::{
    LocalPublicationBatch, PublishedArtifact, SealedArtifactHandle, VerifiedSidecarArtifact,
};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

pub type ArtifactTransfer<'a> =
    dyn Fn(SealedArtifactHandle, &str, &str) -> Result<TransferReceipt, String> + 'a;

/// The engine name the compute-artifact record keys report heads by.
pub const ENGINE: &str = "network_reporter";
/// The locator a report row's bytes live at: `network_reporter:batch:<id>`.
/// The id is the committed batch's own (64 hex) for rows sealed through
/// [`seal`]; rows recorded before 2026-09-20 carry the publish id instead
/// and are resolved by it.
const LOCATOR_PREFIX: &str = "network_reporter:batch:";

pub struct ReportProducer<'a> {
    pub root: &'a Path,
    pub project: &'a str,
    pub upload: &'a ArtifactTransfer<'a>,
}

/// Projects with committed report batches on disk, optionally fenced by
/// owner. The store's `projects_of_fence` is the queue's own answer; this
/// reads the bytes directory so a project whose batches were sealed before
/// the store knew them is still visited and adopted.
pub fn projects(root: &Path, owner_uid: Option<&str>) -> Result<BTreeSet<String>, String> {
    if owner_uid.is_some_and(|owner| owner.is_empty() || owner.len() > 128) {
        return Err("report publication owner is invalid".to_string());
    }
    let Some(held) = ds_report_artifacts::confined_fs::HeldDirectory::open_absolute(root)? else {
        return Ok(BTreeSet::new());
    };
    Ok(ds_report_artifacts::publication::list_committed(&held)?
        .into_iter()
        .filter(|receipt| owner_uid.is_none_or(|owner| receipt.owner_uid == owner))
        .map(|receipt| receipt.project_id)
        .collect())
}

/// The locator a committed batch's bytes live at.
pub fn locator_of(batch_id: &str) -> String {
    format!("{LOCATOR_PREFIX}{batch_id}")
}

/// The row a committed batch stands for. `readable` is what the seal
/// established: the batch was verified as it was written, and the transfer
/// verifies again as it reads.
fn row_of(batch: &LocalPublicationBatch) -> LocalRow {
    let outputs: Vec<LocalOutput> = batch
        .outputs
        .iter()
        .map(|output| LocalOutput {
            filename: Some(output.filename.clone()),
            paper_size: output.paper_size.clone(),
            presentation: output.presentation.as_ref().map(|p| serde_json::json!(p)),
            output_id: output.output_id.clone(),
            format: output.format.clone(),
            content_type: output.content_type.clone(),
            sha256: output.sha256.clone(),
            size_bytes: output.size_bytes,
        })
        .collect();
    let mut pairs: Vec<(String, String)> = outputs
        .iter()
        .map(|output| (output.output_id.clone(), output.sha256.clone()))
        .collect();
    LocalRow {
        identity: sync::Identity {
            engine: batch.engine.clone(),
            operation: batch.operation.clone(),
            variant: batch.variant.clone(),
        },
        sha256: crate::inventory_digest(&mut pairs),
        size_bytes: outputs.iter().map(|output| output.size_bytes).sum(),
        produced_at_ms: batch.produced_at_ms,
        // Deliberately none, and it must stay none. A report is not computed
        // against an output head, so there is no OCC revision to carry; the
        // batch's `input_base_fingerprint` is a SOURCE digest and
        // `sync::RemoteHead` says in as many words that it is never an OCC
        // revision. Ordering for these rows is `sync::derived_winner` — last
        // writer by production time — not a base-revision comparison.
        base_revision: None,
        input_base_fingerprint: Some(batch.input_base_fingerprint.clone())
            .filter(|value| !value.is_empty()),
        readable: true,
        engine_release: batch.engine_release.clone(),
        engine_build_manifest_sha256: batch.engine_build_manifest_sha256.clone(),
        resource: batch.transformer.clone(),
        client_publish_id: batch.client_publish_id.clone(),
        outputs,
        bytes_locator: locator_of(&batch.batch_id),
    }
}

/// The batch a row's bytes live in, by its locator — one directory open for
/// a row sealed through [`seal`]; a scan by publish id for a row recorded
/// before locators named batch ids. `None` when the batch is not committed
/// on this machine any more.
fn batch_for(
    root: &Path,
    project: &str,
    locator: &str,
    client_publish_id: &str,
) -> Result<Option<LocalPublicationBatch>, String> {
    if let Some(batch_id) = locator.strip_prefix(LOCATOR_PREFIX) {
        if ds_report_artifacts::exact_lower_sha256(batch_id) {
            return ds_report_artifacts::committed_batch(root, batch_id);
        }
    }
    if client_publish_id.is_empty() {
        return Ok(None);
    }
    Ok(
        ds_report_artifacts::list_project_publication_batches_from_root(root, project)?
            .into_iter()
            .find(|batch| batch.client_publish_id == client_publish_id),
    )
}

impl Producer for ReportProducer<'_> {
    fn inventory(&self, project: &str) -> Result<Vec<LocalRow>, String> {
        if project != self.project {
            return Err("report producer project changed".into());
        }
        inventory(self.root, project)
    }
    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String> {
        let batch = batch_for(
            self.root,
            self.project,
            &row.bytes_locator,
            &row.client_publish_id,
        )?
        .ok_or("that publication is no longer committed locally")?;
        let handle = ds_report_artifacts::open_batch_output(self.root, &batch, output_id)?;
        (self.upload)(handle, output_id, session_uri)
    }

    fn reclaim(&self, project: &str, row: &LocalRow) -> Result<u64, String> {
        ReportProducer::reclaim(self, project, row)
    }
}

impl ReportProducer<'_> {
    /// Free the sealed batch of one row that lost, through the artifact
    /// store's own discard: each output goes through
    /// [`ds_report_artifacts::discard_artifact_from_root_with_gate`], which
    /// verifies the bytes against the receipt before unlinking them and
    /// retires the batch directory itself once its last output is gone.
    /// Nothing outside the batch the row's locator names is touched.
    ///
    /// Idempotent: a batch that is not on disk any more answers `Ok(0)`.
    pub fn reclaim(&self, project: &str, row: &LocalRow) -> Result<u64, String> {
        if project != self.project {
            return Err("report producer project changed".into());
        }
        if row.client_publish_id.is_empty() {
            // A replica of the head, not a publication of this producer.
            return Ok(0);
        }
        reclaim_batch(
            self.root,
            project,
            &row.bytes_locator,
            &row.client_publish_id,
        )
    }
}

fn reclaim_batch(
    root: &Path,
    project: &str,
    locator: &str,
    client_publish_id: &str,
) -> Result<u64, String> {
    let Some(batch) = batch_for(root, project, locator, client_publish_id)? else {
        return Ok(0);
    };
    let mut freed = 0_u64;
    for output in &batch.outputs {
        let id = ds_report_artifacts::locator_id(&output.locator)?;
        // Count only what is actually there: the discard is idempotent over
        // an absent output and would otherwise report bytes freed twice for
        // a batch retried after a partial failure.
        let present = std::fs::symlink_metadata(root.join(id)).is_ok();
        ds_report_artifacts::discard_artifact_from_root_with_gate(
            root,
            &output.locator,
            &output.sha256,
            |_| Ok(std::time::Instant::now()),
        )?;
        if present {
            freed = freed.saturating_add(output.size_bytes);
        }
    }
    Ok(freed)
}

/// Every committed batch of one project as a row, one per batch, each naming
/// the batch its bytes live in. Cheap: the receipts are read, the bytes are
/// not. The host adopts the ones the store does not know and frees the ones
/// the store has settled as lost; it decides nothing else from this.
pub fn inventory(root: &Path, project: &str) -> Result<Vec<LocalRow>, String> {
    let batches = ds_report_artifacts::list_project_publication_batches_from_root(root, project)?;
    if batches.len() > MAX_ROWS {
        return Err("this project holds more publications than the sync gate admits".to_string());
    }
    // Oldest first, so two batches of one room sealed before the store knew
    // them are adopted in production order and the newer supersedes.
    let mut rows: Vec<LocalRow> = batches.iter().map(row_of).collect();
    rows.sort_by(|left, right| {
        left.produced_at_ms
            .cmp(&right.produced_at_ms)
            .then_with(|| left.client_publish_id.cmp(&right.client_publish_id))
    });
    Ok(rows)
}

// ---------------------------------------------------------------------------
// The seal
// ---------------------------------------------------------------------------

/// Everything a seal needs, as the producing host established it: the
/// verified engine outputs and the facts the batch receipt carries.
pub struct SealRequest<'a> {
    pub owner_uid: &'a str,
    pub project_id: &'a str,
    pub engine_version: &'a str,
    pub engine_build_manifest_sha256: &'a str,
    pub transformer: &'a str,
    pub transformer_revision: i64,
    pub input_base_fingerprint: &'a str,
    pub room_content_sha256: &'a str,
    pub client_run_id: &'a str,
    pub artifacts: &'a [VerifiedSidecarArtifact],
    pub expected_formats: &'a [&'a str],
    pub deadline: Instant,
    /// Re-checked at the one durable visibility boundary, after every copy,
    /// rename and fsync and before the batch becomes visible: the producing
    /// host's own scope fence (account, project, credential generation).
    pub guard: &'a dyn Fn() -> Result<(), String>,
}

/// What one seal established.
#[derive(Debug)]
pub struct Sealed {
    pub receipt: PublicationBatchReceipt,
    pub artifacts: Vec<PublishedArtifact>,
    /// The row as the store now holds it: `held`, queued.
    pub row: store::ArtifactRow,
    /// The batch this seal superseded on this machine and freed, if the
    /// room already held one with other bytes: `(publish id, bytes freed)`.
    pub superseded: Option<(String, u64)>,
}

#[derive(Debug)]
pub enum SealOutcome {
    /// Bytes on disk, integrity receipt written, row recorded — one
    /// acknowledged step.
    Sealed(Sealed),
    /// The store already holds this exact publication for the room (same
    /// inventory digest). Nothing was written, or what was written was
    /// removed again; the store's row is the one that stands.
    AlreadyRecorded {
        replay_key: String,
        state: store::ArtifactState,
    },
}

/// Seal one report run: write the bytes and their integrity receipts,
/// commit the batch, and record its `held` row in the sync store — in that
/// order, acknowledged together or not at all. If the store refuses or
/// cannot be written, the bytes are removed and the seal is not
/// acknowledged. A room that already held another batch on this machine
/// has it freed: online overwrites local, and a superseded local result is
/// not the project's artefact.
pub fn seal(
    store: &SharedStore,
    fence: &store::Fence,
    root: &Path,
    request: &SealRequest<'_>,
) -> Result<SealOutcome, String> {
    let identity = sync::Identity {
        engine: ENGINE.to_string(),
        operation: format!("export-{}", request.transformer),
        variant: "default".to_string(),
    };
    let mut pairs: Vec<(String, String)> = request
        .artifacts
        .iter()
        .map(|artifact| (artifact.format.clone(), artifact.sha256.clone()))
        .collect();
    let inventory_sha256 = crate::inventory_digest(&mut pairs);
    let previous = crate::seal::standing(store, fence, request.project_id, &identity)?;
    if let Some(previous) = previous.as_ref() {
        if previous.sha256 == inventory_sha256 {
            return Ok(SealOutcome::AlreadyRecorded {
                replay_key: previous.replay_key.clone(),
                state: previous.state,
            });
        }
    }

    // The bytes: verified as they are copied, receipted per output, and
    // committed by one rename. Everything below this line either ends in a
    // recorded row or removes what it wrote.
    let pending = ds_report_artifacts::promote_local_artifacts(
        root,
        request.artifacts,
        request.owner_uid,
        request.project_id,
        request.engine_version,
        request.engine_build_manifest_sha256,
        request.transformer,
        request.transformer_revision,
        request.input_base_fingerprint,
        request.room_content_sha256,
        request.client_run_id,
        request.expected_formats,
        request.deadline,
    )?;
    (request.guard)()?;
    let committed = pending.commit(request.deadline)?;
    let batch = ds_report_artifacts::committed_batch(root, &committed.receipt.batch_id)?
        .ok_or("the sealed batch is not committed where it was written")?;
    let row = row_of(&batch);

    // The row: the room's previous batch on this machine lost to this one
    // and is freed first; then the new row is recorded. Not acknowledged
    // means the new bytes leave with the failure.
    let free = |previous: &store::ArtifactRow| {
        reclaim_batch(
            root,
            request.project_id,
            &previous.bytes_locator,
            &previous.client_publish_id,
        )
    };
    let recorded = crate::seal::record(
        store,
        fence,
        request.project_id,
        &row,
        previous.as_ref(),
        &free,
    );
    match recorded {
        Ok(crate::seal::Recorded::Recorded {
            row: stored,
            superseded,
        }) => Ok(SealOutcome::Sealed(Sealed {
            receipt: committed.receipt,
            artifacts: committed.artifacts,
            row: *stored,
            superseded,
        })),
        Ok(crate::seal::Recorded::AlreadyRecorded { replay_key, state }) => {
            // A race with another seal of the same bytes: theirs stands.
            let _ = reclaim_batch(
                root,
                request.project_id,
                &row.bytes_locator,
                &row.client_publish_id,
            );
            Ok(SealOutcome::AlreadyRecorded { replay_key, state })
        }
        Ok(crate::seal::Recorded::Refused(refusal)) => {
            let _ = reclaim_batch(
                root,
                request.project_id,
                &row.bytes_locator,
                &row.client_publish_id,
            );
            Err(format!(
                "the sync store refused the seal ({}): {}",
                refusal.code, refusal.detail
            ))
        }
        Err(error) => {
            let _ = reclaim_batch(
                root,
                request.project_id,
                &row.bytes_locator,
                &row.client_publish_id,
            );
            Err(format!(
                "the seal was not recorded in the sync store: {error}"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows::store_scope;
    use ds_report_artifacts::publication::{
        PendingPublicationCommit, PublicationBatchReceipt, PublicationOutput, BATCH_SCHEMA,
    };
    use std::collections::BTreeMap;

    fn receipt(batch: char, owner: &str, project: &str, publish: &str) -> PublicationBatchReceipt {
        PublicationBatchReceipt {
            schema: BATCH_SCHEMA,
            batch_id: batch.to_string().repeat(64),
            owner_uid: owner.to_string(),
            organization_id: None,
            project_id: project.to_string(),
            root: format!("eds_project/{project}/eds_design"),
            client_run_id: format!("report-{}", batch.to_string().repeat(32)),
            engine: "network_reporter".to_string(),
            operation: "export-tx_1".to_string(),
            variant: "default".to_string(),
            engine_version: format!("ds-network-reporter@0.1.0+{}", "b".repeat(40)),
            engine_build_manifest_sha256: "c".repeat(64),
            transformer: "tx_1".to_string(),
            transformer_revision: 1,
            input_base_fingerprint: "d".repeat(64),
            room_content_sha256: "e".repeat(64),
            grant_id: None,
            snapshot_base: BTreeMap::new(),
            client_publish_id: publish.to_string(),
            outputs: vec![PublicationOutput {
                output_id: "xlsx".to_string(),
                locator: format!("ds-reporter:v1:{}", batch.to_string().repeat(64)),
                filename: "tx_1.xlsx".to_string(),
                format: "xlsx".to_string(),
                content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                    .to_string(),
                sha256: "f".repeat(64),
                size_bytes: 42,
                paper_size: None,
                presentation: None,
            }],
        }
    }

    /// Every committed batch is a row naming its own batch, oldest first,
    /// and the same three batches list the same on every machine and run.
    #[test]
    fn every_committed_batch_is_one_row_naming_its_batch_in_production_order() {
        let directory = tempfile::tempdir().expect("temp root");
        let held =
            ds_report_artifacts::confined_fs::HeldDirectory::ensure_absolute(directory.path())
                .expect("hold root");
        for batch in ['1', '2', '3'] {
            let mut row = receipt(
                batch,
                "owner-1",
                "project-a",
                &format!("publish-{batch}23e4567-e89b-42d3-a456-426614174000"),
            );
            row.outputs[0].sha256 = batch.to_string().repeat(64);
            PendingPublicationCommit::stage(&held, &row)
                .expect("stage")
                .commit()
                .expect("commit");
        }
        let rows = inventory(directory.path(), "project-a").expect("inventory");
        assert_eq!(
            rows.len(),
            3,
            "the queue is the store's; every batch is offered for adoption"
        );
        for row in &rows {
            assert!(row.bytes_locator.starts_with(LOCATOR_PREFIX));
            let batch_id = row.bytes_locator.strip_prefix(LOCATOR_PREFIX).unwrap();
            assert!(ds_report_artifacts::exact_lower_sha256(batch_id));
            assert_eq!(row.resource, "tx_1");
            assert!(row.readable);
        }
        for _ in 0..4 {
            assert_eq!(
                inventory(directory.path(), "project-a").expect("inventory"),
                rows
            );
        }
    }

    /// The producer for a root with no upload path: reclaim needs none.
    fn producer<'a>(
        root: &'a Path,
        project: &'a str,
        upload: &'a ArtifactTransfer<'a>,
    ) -> ReportProducer<'a> {
        ReportProducer {
            root,
            project,
            upload,
        }
    }

    fn write_bytes(root: &Path, row: &mut PublicationBatchReceipt, bytes: &[u8]) {
        row.outputs[0].sha256 = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(bytes));
        row.outputs[0].size_bytes = bytes.len() as u64;
        let entry = root.join(ds_report_artifacts::locator_id(&row.outputs[0].locator).unwrap());
        std::fs::create_dir_all(&entry).expect("artifact entry");
        std::fs::write(entry.join(ds_report_artifacts::ARTIFACT_FILE), bytes).expect("bytes");
        let receipt_json = serde_json::json!({
            "schema": ds_report_artifacts::RECEIPT_SCHEMA,
            "batch_id": row.batch_id,
            "output_id": "xlsx",
            "locator": row.outputs[0].locator,
            "artifact_file": ds_report_artifacts::ARTIFACT_FILE,
            "size_bytes": bytes.len(),
            "sha256": row.outputs[0].sha256,
        });
        std::fs::write(
            entry.join(ds_report_artifacts::RECEIPT_FILE),
            serde_json::to_vec_pretty(&receipt_json).expect("encode"),
        )
        .expect("receipt");
    }

    /// The 86-batch defect, in miniature: a row the store settled as lost
    /// is freed through the store's own discard, the batch directory goes
    /// with its last output, and asking again costs nothing and frees 0 —
    /// by the batch id the row's locator names, and by publish id for a row
    /// recorded before locators named batch ids.
    #[test]
    fn a_lost_row_is_reclaimed_once_by_its_locator_or_its_publish_id() {
        let directory = tempfile::tempdir().expect("temp root");
        let held =
            ds_report_artifacts::confined_fs::HeldDirectory::ensure_absolute(directory.path())
                .expect("hold root");
        let mut row = receipt(
            '1',
            "owner-1",
            "project-a",
            "publish-123e4567-e89b-42d3-a456-426614174000",
        );
        let bytes = b"lost report";
        write_bytes(directory.path(), &mut row, bytes);
        PendingPublicationCommit::stage(&held, &row)
            .expect("stage")
            .commit()
            .expect("commit");
        let upload =
            |_: SealedArtifactHandle, _: &str, _: &str| -> Result<TransferReceipt, String> {
                Err("no upload in this test".into())
            };
        let producer = producer(directory.path(), "project-a", &upload);
        let rows = inventory(directory.path(), "project-a").expect("inventory");
        assert_eq!(rows.len(), 1);
        // A row from before this rule: its locator is the publish id.
        let mut legacy = rows[0].clone();
        legacy.bytes_locator = format!("{LOCATOR_PREFIX}{}", legacy.client_publish_id);
        assert!(
            batch_for(
                directory.path(),
                "project-a",
                &legacy.bytes_locator,
                &legacy.client_publish_id
            )
            .expect("resolve")
            .is_some(),
            "a publish-id locator still finds its batch"
        );

        let freed = producer.reclaim("project-a", &rows[0]).expect("reclaim");
        assert_eq!(freed, bytes.len() as u64);
        let entry = directory.path().join('1'.to_string().repeat(64));
        assert!(!entry.exists(), "the output's bytes are gone");
        assert!(
            !directory
                .path()
                .join(ds_report_artifacts::publication::BATCH_DIRECTORY)
                .join(&row.batch_id)
                .exists(),
            "the batch directory retired with its last output"
        );
        assert!(inventory(directory.path(), "project-a")
            .expect("inventory")
            .is_empty());
        assert_eq!(
            producer
                .reclaim("project-a", &rows[0])
                .expect("reclaim again"),
            0,
            "a batch that is not on disk any more is not an error"
        );
        assert_eq!(
            producer
                .reclaim("project-a", &legacy)
                .expect("legacy again"),
            0
        );
        assert_eq!(
            producer.reclaim("project-b", &rows[0]),
            Err("report producer project changed".to_string()),
            "a row is never reclaimed under another project"
        );
    }

    #[test]
    fn project_inventory_is_shared_and_owner_fenced() {
        let directory = tempfile::tempdir().expect("temp root");
        let held =
            ds_report_artifacts::confined_fs::HeldDirectory::ensure_absolute(directory.path())
                .expect("hold root");
        for row in [
            receipt(
                '1',
                "owner-1",
                "project-a",
                "publish-123e4567-e89b-42d3-a456-426614174000",
            ),
            receipt(
                '2',
                "owner-1",
                "project-b",
                "publish-223e4567-e89b-42d3-a456-426614174000",
            ),
            receipt(
                '3',
                "owner-2",
                "project-c",
                "publish-323e4567-e89b-42d3-a456-426614174000",
            ),
        ] {
            PendingPublicationCommit::stage(&held, &row)
                .expect("stage")
                .commit()
                .expect("commit");
        }

        assert_eq!(
            projects(directory.path(), Some("owner-1")).expect("owner inventory"),
            BTreeSet::from(["project-a".to_string(), "project-b".to_string()])
        );
        assert_eq!(
            projects(directory.path(), None).expect("host inventory"),
            BTreeSet::from([
                "project-a".to_string(),
                "project-b".to_string(),
                "project-c".to_string(),
            ])
        );
    }

    // -- the seal -----------------------------------------------------------

    fn seal_fence() -> store::Fence {
        store::Fence {
            account: "owner-1".into(),
            deployment: "https://gateway.example".into(),
            install_id: "install-1".into(),
        }
    }

    /// One verified engine output on disk, as the report host hands it over.
    fn staged(dir: &Path, name: &str, bytes: &[u8]) -> VerifiedSidecarArtifact {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("stage");
        VerifiedSidecarArtifact {
            format: "xlsx".into(),
            filename: name.into(),
            size_bytes: bytes.len() as u64,
            sha256: format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(bytes)),
            paper_size: None,
            presentation: None,
            held_file: std::fs::File::open(&path).expect("open staged"),
        }
    }

    fn seal_request<'a>(
        artifacts: &'a [VerifiedSidecarArtifact],
        formats: &'a [&'a str],
        deadline: Instant,
        guard: &'a dyn Fn() -> Result<(), String>,
    ) -> SealRequest<'a> {
        SealRequest {
            owner_uid: "owner-1",
            project_id: "project-a",
            engine_version: "ds-network-reporter@0.1.0+bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            engine_build_manifest_sha256:
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            transformer: "tx_1",
            transformer_revision: 3,
            input_base_fingerprint:
                "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            room_content_sha256: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            client_run_id: "report-0123456789abcdef0123456789abcdef",
            artifacts,
            expected_formats: formats,
            deadline,
            guard,
        }
    }

    fn rows_in(store: &SharedStore, project: &str) -> Vec<store::ArtifactRow> {
        store
            .lock()
            .unwrap()
            .snapshot(&seal_fence(), &store_scope(project))
            .unwrap()
            .artifacts
    }

    fn receipts_in(store: &SharedStore, project: &str) -> Vec<store::ReceiptRow> {
        store
            .lock()
            .unwrap()
            .receipts(&seal_fence(), &store_scope(project), 50)
            .unwrap()
    }

    /// The seal is one acknowledged step: bytes committed, integrity receipt
    /// written, row `held` — and the queue reads the row, not the disk.
    #[test]
    fn a_seal_writes_bytes_receipt_and_row_in_one_step_and_the_queue_reads_the_row() {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("report-artifacts");
        let store = crate::open_store(&dir.path().join("store.sqlite")).expect("store");
        let staged = [staged(dir.path(), "tx_1.xlsx", b"first bytes")];
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        let guard = || Ok(());
        let outcome = seal(
            &store,
            &seal_fence(),
            &root,
            &seal_request(&staged, &["xlsx"], deadline, &guard),
        )
        .expect("seal");
        let SealOutcome::Sealed(sealed) = outcome else {
            panic!("a first seal is sealed: {outcome:?}");
        };
        assert_eq!(sealed.row.state, store::ArtifactState::Held);
        assert_eq!(
            sealed.row.bytes_locator,
            locator_of(&sealed.receipt.batch_id)
        );
        assert!(sealed.superseded.is_none());
        let committed = ds_report_artifacts::committed_batch(&root, &sealed.receipt.batch_id)
            .expect("read")
            .expect("the batch is committed");
        assert_eq!(committed.client_publish_id, sealed.row.client_publish_id);
        let rows = rows_in(&store, "project-a");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].replay_key, sealed.receipt.client_publish_id);
        assert_eq!(rows[0].resource.as_deref(), Some("tx_1"));
        let status = store
            .lock()
            .unwrap()
            .queue(&seal_fence(), None, crate::now_ms())
            .unwrap();
        assert_eq!(status.queued_batches, 1);
        assert_eq!(status.queued_bytes, b"first bytes".len() as u64);
        assert_eq!(status.projects[0].transformers, vec!["tx_1".to_string()]);
        // Nothing on disk is adopted: the row the seal wrote is the one the
        // producer's inventory names.
        assert_eq!(
            inventory(&root, "project-a").unwrap()[0].bytes_locator,
            rows[0].bytes_locator
        );
    }

    /// Invariant (2), at the seal: the same bytes sealed twice are one row,
    /// and the second seal leaves no second batch on disk.
    #[test]
    fn invariant_2_the_same_bytes_sealed_twice_are_one_row_and_one_batch() {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("report-artifacts");
        let store = crate::open_store(&dir.path().join("store.sqlite")).expect("store");
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        let guard = || Ok(());
        let first = [staged(dir.path(), "tx_1.xlsx", b"same bytes")];
        let SealOutcome::Sealed(sealed) = seal(
            &store,
            &seal_fence(),
            &root,
            &seal_request(&first, &["xlsx"], deadline, &guard),
        )
        .expect("seal") else {
            panic!("first seal")
        };
        let again = [staged(dir.path(), "tx_1-again.xlsx", b"same bytes")];
        let outcome = seal(
            &store,
            &seal_fence(),
            &root,
            &seal_request(&again, &["xlsx"], deadline, &guard),
        )
        .expect("seal again");
        let SealOutcome::AlreadyRecorded { replay_key, state } = outcome else {
            panic!("the same bytes are already recorded: {outcome:?}");
        };
        assert_eq!(replay_key, sealed.receipt.client_publish_id);
        assert_eq!(state, store::ArtifactState::Held);
        assert_eq!(rows_in(&store, "project-a").len(), 1);
        assert_eq!(
            inventory(&root, "project-a").unwrap().len(),
            1,
            "no second batch was written for the same bytes"
        );
    }

    /// If the store cannot record the row, the seal is not acknowledged and
    /// the bytes it wrote are removed: a batch on disk with no row is exactly
    /// the state this rule exists to end. The seam is the store's own mutex,
    /// poisoned by a panicking holder — no fake store, no flag.
    #[test]
    fn a_seal_the_store_cannot_record_leaves_no_bytes_behind() {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("report-artifacts");
        let store = crate::open_store(&dir.path().join("store.sqlite")).expect("store");
        let poisoned = store.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.lock().unwrap();
            panic!("the store's holder died mid-transition");
        })
        .join();
        assert!(store.lock().is_err(), "the mutex is poisoned");
        let staged = [staged(dir.path(), "tx_1.xlsx", b"unrecorded bytes")];
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        let guard = || Ok(());
        let error = seal(
            &store,
            &seal_fence(),
            &root,
            &seal_request(&staged, &["xlsx"], deadline, &guard),
        )
        .expect_err("not acknowledged");
        assert!(error.contains("sync gate is unavailable"), "{error}");
        assert!(
            inventory(&root, "project-a").unwrap().is_empty(),
            "no batch is committed for a seal that was not recorded"
        );
    }

    /// A room sealed again with other bytes: the earlier batch lost locally
    /// and leaves the disk with a `reclaimed` receipt; the row is the new
    /// one. An artefact never accumulates on the edge.
    #[test]
    fn a_new_seal_of_a_room_frees_the_batch_it_supersedes_and_says_so() {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("report-artifacts");
        let store = crate::open_store(&dir.path().join("store.sqlite")).expect("store");
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        let guard = || Ok(());
        let first = [staged(dir.path(), "tx_1.xlsx", b"first computation")];
        let SealOutcome::Sealed(older) = seal(
            &store,
            &seal_fence(),
            &root,
            &seal_request(&first, &["xlsx"], deadline, &guard),
        )
        .expect("seal") else {
            panic!("first seal")
        };
        let second = [staged(
            dir.path(),
            "tx_1-v2.xlsx",
            b"second computation, longer",
        )];
        let SealOutcome::Sealed(newer) = seal(
            &store,
            &seal_fence(),
            &root,
            &seal_request(&second, &["xlsx"], deadline, &guard),
        )
        .expect("seal") else {
            panic!("second seal")
        };
        assert_eq!(
            newer.superseded,
            Some((
                older.receipt.client_publish_id.clone(),
                b"first computation".len() as u64
            ))
        );
        assert!(
            ds_report_artifacts::committed_batch(&root, &older.receipt.batch_id)
                .unwrap()
                .is_none(),
            "the superseded batch left the disk"
        );
        let rows = rows_in(&store, "project-a");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].replay_key, newer.receipt.client_publish_id);
        assert_eq!(rows[0].state, store::ArtifactState::Held);
        let receipts = receipts_in(&store, "project-a");
        let reclaimed = receipts
            .iter()
            .find(|receipt| receipt.action == "reclaimed")
            .expect("the supersession is evidence");
        assert_eq!(reclaimed.outcome, "superseded_locally");
        assert_eq!(
            reclaimed.committed_bytes,
            Some(b"first computation".len() as u64)
        );
        assert!(reclaimed
            .detail
            .as_deref()
            .unwrap()
            .contains(&older.receipt.client_publish_id));
        assert_eq!(inventory(&root, "project-a").unwrap().len(), 1);
    }

    /// The guard is re-checked at the durable boundary: a scope that moved
    /// between the copy and the commit seals nothing and records nothing.
    #[test]
    fn a_guard_that_refuses_at_the_boundary_seals_nothing() {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("report-artifacts");
        let store = crate::open_store(&dir.path().join("store.sqlite")).expect("store");
        let staged = [staged(dir.path(), "tx_1.xlsx", b"scoped bytes")];
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        let guard = || Err("the native publication scope changed".to_string());
        let error = seal(
            &store,
            &seal_fence(),
            &root,
            &seal_request(&staged, &["xlsx"], deadline, &guard),
        )
        .expect_err("refused at the boundary");
        assert!(error.contains("scope changed"));
        assert!(rows_in(&store, "project-a").is_empty());
        assert!(inventory(&root, "project-a").unwrap().is_empty());
    }
}
