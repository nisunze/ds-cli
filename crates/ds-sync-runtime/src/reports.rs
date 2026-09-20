//! One report publication producer, shared by native hosts.
use crate::{kernel_sync as sync, LocalOutput, LocalRow, Producer, TransferReceipt, MAX_ROWS};
use ds_report_artifacts::SealedArtifactHandle;
use std::collections::BTreeSet;
use std::path::Path;

pub type ArtifactTransfer<'a> =
    dyn Fn(SealedArtifactHandle, &str, &str) -> Result<TransferReceipt, String> + 'a;

pub struct ReportProducer<'a> {
    pub root: &'a Path,
    pub project: &'a str,
    pub upload: &'a ArtifactTransfer<'a>,
}

/// Projects with committed report publications on this machine.
///
/// Desktop and Server call this same inventory function so neither host
/// depends on a selected project or maintains a second queue index. The
/// optional owner fence is used by a multi-account Desktop installation;
/// Server already runs inside one native account and therefore passes none.
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
impl Producer for ReportProducer<'_> {
    fn inventory(&self, project: &str) -> Result<Vec<LocalRow>, String> {
        if project != self.project {
            return Err("report producer project changed".into());
        }
        inventory(self.root, project)
    }
    fn locator(&self, row: &LocalRow) -> String {
        format!("network_reporter:batch:{}", row.client_publish_id)
    }
    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String> {
        let batches = ds_report_artifacts::list_project_publication_batches_from_root(
            self.root,
            self.project,
        )?;
        let batch = batches
            .into_iter()
            .find(|batch| batch.client_publish_id == row.client_publish_id)
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
    /// Nothing outside the batch the row's publish id names is touched.
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
        let batches =
            ds_report_artifacts::list_project_publication_batches_from_root(self.root, project)?;
        let Some(batch) = batches
            .into_iter()
            .find(|batch| batch.client_publish_id == row.client_publish_id)
        else {
            return Ok(0);
        };
        let mut freed = 0_u64;
        for output in &batch.outputs {
            let id = ds_report_artifacts::locator_id(&output.locator)?;
            // Count only what is actually there: the discard is idempotent
            // over an absent output and would otherwise report bytes freed
            // twice for a batch retried after a partial failure.
            let present = std::fs::symlink_metadata(self.root.join(id)).is_ok();
            ds_report_artifacts::discard_artifact_from_root_with_gate(
                self.root,
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
}

/// One committed batch this machine holds that is NOT the queue row for its
/// room, with the batch that displaced it.
///
/// It exists because "not the current row" used to be spelled `continue`: the
/// batch vanished from every surface and could be reached by no pump and no
/// command. A superseded batch is still bytes on a disk and still the
/// operator's, so it is named, counted and reclaimable.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SupersededBatch {
    pub batch_id: String,
    pub client_publish_id: String,
    pub transformer: String,
    pub produced_at_ms: u64,
    pub size_bytes: u64,
    /// The `client_publish_id` of the batch that is the queue row instead.
    pub superseded_by: String,
}

/// Everything this machine holds for one project's reports, counted.
///
/// `ds_command_kernel::report::PublicationStage::Sealed` is exactly
/// `batches - queued`: committed, digest-verified bytes with no queue row. The
/// blocker was that this number had no reader at all, so 593 of them looked
/// like nothing had ever run.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct SealedCensus {
    /// Committed publication batches, superseded ones included.
    pub batches: usize,
    /// Rooms with a queue row — one per `(engine, operation, variant)`.
    pub queued: usize,
    pub bytes: u64,
    pub oldest_produced_at_ms: Option<u64>,
    pub superseded: Vec<SupersededBatch>,
}

/// Count what is committed on this machine without opening a single artifact.
///
/// Deliberately cheap: `inventory` rehashes every output to decide
/// `readable`, which is right before an upload and wrong for a status line an
/// operator asks for repeatedly.
pub fn census(root: &Path, project: &str) -> Result<SealedCensus, String> {
    let batches = ds_report_artifacts::list_project_publication_batches_from_root(root, project)?;
    let mut census = SealedCensus {
        batches: batches.len(),
        ..SealedCensus::default()
    };
    let mut current: std::collections::BTreeMap<sync::Identity, &_> =
        std::collections::BTreeMap::new();
    for batch in &batches {
        let identity = sync::Identity {
            engine: batch.engine.clone(),
            operation: batch.operation.clone(),
            variant: batch.variant.clone(),
        };
        let size: u64 = batch.outputs.iter().map(|output| output.size_bytes).sum();
        census.bytes = census.bytes.saturating_add(size);
        if batch.produced_at_ms > 0 {
            census.oldest_produced_at_ms = Some(
                census
                    .oldest_produced_at_ms
                    .map_or(batch.produced_at_ms, |old| old.min(batch.produced_at_ms)),
            );
        }
        match current.get(&identity) {
            Some(held) if !newer(batch, held) => {
                census
                    .superseded
                    .push(superseded(batch, size, &held.client_publish_id))
            }
            Some(held) => {
                let displaced: u64 = held.outputs.iter().map(|output| output.size_bytes).sum();
                census
                    .superseded
                    .push(superseded(held, displaced, &batch.client_publish_id));
                current.insert(identity, batch);
            }
            None => {
                current.insert(identity, batch);
            }
        }
    }
    census.queued = current.len();
    census
        .superseded
        .sort_by(|left, right| left.batch_id.cmp(&right.batch_id));
    Ok(census)
}

fn superseded(
    batch: &ds_report_artifacts::LocalPublicationBatch,
    size_bytes: u64,
    superseded_by: &str,
) -> SupersededBatch {
    SupersededBatch {
        batch_id: batch.batch_id.clone(),
        client_publish_id: batch.client_publish_id.clone(),
        transformer: batch.transformer.clone(),
        produced_at_ms: batch.produced_at_ms,
        size_bytes,
        superseded_by: superseded_by.to_string(),
    }
}

/// Which of two committed batches for the same room is the current one.
///
/// Ordered here rather than trusted from the listing's order: the listing
/// sorts, but a caller that ever stops sorting must not silently change which
/// of 593 batches is the one that publishes. Production time decides, and the
/// batch id breaks a tie so the answer is the same on every machine.
fn newer(
    candidate: &ds_report_artifacts::LocalPublicationBatch,
    held: &ds_report_artifacts::LocalPublicationBatch,
) -> bool {
    (candidate.produced_at_ms, &candidate.batch_id) > (held.produced_at_ms, &held.batch_id)
}

pub fn inventory(root: &Path, project: &str) -> Result<Vec<LocalRow>, String> {
    let batches = ds_report_artifacts::list_project_publication_batches_from_root(root, project)?;
    let mut current: std::collections::BTreeMap<
        sync::Identity,
        ds_report_artifacts::LocalPublicationBatch,
    > = std::collections::BTreeMap::new();
    for batch in batches {
        let identity = sync::Identity {
            engine: batch.engine.clone(),
            operation: batch.operation.clone(),
            variant: batch.variant.clone(),
        };
        // One room, one head: the queue row is the LATEST computation of it.
        // This used to keep whichever batch the listing happened to yield
        // first and drop the rest without a word.
        match current.get(&identity) {
            Some(held) if !newer(&batch, held) => continue,
            _ => {
                current.insert(identity, batch);
            }
        }
    }
    let mut rows: std::collections::BTreeMap<sync::Identity, LocalRow> =
        std::collections::BTreeMap::new();
    for (identity, batch) in current {
        let outputs: Vec<LocalOutput> = batch
            .outputs
            .iter()
            .map(|output| LocalOutput {
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
        let readable = outputs.iter().all(|output| {
            ds_report_artifacts::open_batch_output(root, &batch, &output.output_id).is_ok_and(
                |handle| handle.size_bytes == output.size_bytes && handle.sha256 == output.sha256,
            )
        });
        rows.insert(
            identity.clone(),
            LocalRow {
                identity,
                sha256: crate::inventory_digest(&mut pairs),
                size_bytes: outputs.iter().map(|output| output.size_bytes).sum(),
                produced_at_ms: batch.produced_at_ms,
                // Deliberately none, and it must stay none. A report is not
                // computed against an output head, so there is no OCC
                // revision to carry; the batch's `input_base_fingerprint` is
                // a SOURCE digest and `sync::RemoteHead` says in as many
                // words that it is never an OCC revision. Ordering for these
                // rows is `sync::derived_winner` — last writer by production
                // time — not a base-revision comparison.
                base_revision: None,
                input_base_fingerprint: Some(batch.input_base_fingerprint.clone())
                    .filter(|value| !value.is_empty()),
                readable,
                engine_release: batch.engine_release.clone(),
                engine_build_manifest_sha256: batch.engine_build_manifest_sha256.clone(),
                grant_engine: "ds-network-reporter".to_string(),
                resource: batch.transformer.clone(),
                client_publish_id: batch.client_publish_id.clone(),
                outputs,
            },
        );
    }
    if rows.len() > MAX_ROWS {
        return Err("this project holds more publications than the sync gate admits".to_string());
    }
    Ok(rows.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// The 593-artifact defect, in miniature. Three committed batches for one
    /// room used to collapse to a single queue row and the other two vanished
    /// from every surface — no pump could reach them and no command could
    /// name them. The queue still carries one row per room, which is what a
    /// head means, but the batches it displaced are now counted and named.
    #[test]
    fn displaced_batches_are_counted_and_named_instead_of_vanishing() {
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

        let census = census(directory.path(), "project-a").expect("census");
        assert_eq!(census.batches, 3, "every committed batch is still counted");
        assert_eq!(census.queued, 1, "one room, one head");
        assert_eq!(census.superseded.len(), 2);
        assert_eq!(census.bytes, 126);
        for displaced in &census.superseded {
            assert_eq!(displaced.transformer, "tx_1");
            assert!(
                !displaced.superseded_by.is_empty(),
                "a displaced batch names the batch that replaced it"
            );
        }
        // The queue row is one of the three, not an invention.
        let rows = inventory(directory.path(), "project-a").expect("inventory");
        assert_eq!(rows.len(), 1);
        assert!(census
            .superseded
            .iter()
            .all(|displaced| displaced.client_publish_id != rows[0].client_publish_id));
    }

    /// Which batch publishes cannot depend on the order a directory listing
    /// happened to produce: the same three batches must choose the same
    /// winner on every machine and every run.
    #[test]
    fn the_queue_row_for_a_room_is_chosen_deterministically() {
        let directory = tempfile::tempdir().expect("temp root");
        let held =
            ds_report_artifacts::confined_fs::HeldDirectory::ensure_absolute(directory.path())
                .expect("hold root");
        for batch in ['a', 'b', 'c'] {
            let mut row = receipt(
                batch,
                "owner-1",
                "project-a",
                &format!(
                    "publish-a23e4567-e89b-42d3-a456-4266141740{}0",
                    batch as u8 % 10
                ),
            );
            row.outputs[0].sha256 = batch.to_string().repeat(64);
            PendingPublicationCommit::stage(&held, &row)
                .expect("stage")
                .commit()
                .expect("commit");
        }
        let first = inventory(directory.path(), "project-a").expect("inventory");
        for _ in 0..4 {
            let again = inventory(directory.path(), "project-a").expect("inventory");
            assert_eq!(first, again);
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

    /// The 86-batch defect, in miniature: a row the store settled as lost
    /// is freed through the store's own discard, the batch directory goes
    /// with its last output, and asking again costs nothing and frees 0.
    #[test]
    fn a_lost_row_is_reclaimed_once_and_an_absent_batch_answers_zero() {
        let directory = tempfile::tempdir().expect("temp root");
        let held =
            ds_report_artifacts::confined_fs::HeldDirectory::ensure_absolute(directory.path())
                .expect("hold root");
        let row = receipt(
            '1',
            "owner-1",
            "project-a",
            "publish-123e4567-e89b-42d3-a456-426614174000",
        );
        // Put real bytes under the output's locator so the discard has a
        // receipt-bound artifact to verify and unlink, as a sealed batch does.
        let bytes = b"lost report";
        let mut row = row;
        row.outputs[0].sha256 = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(bytes));
        row.outputs[0].size_bytes = bytes.len() as u64;
        let entry = directory.path().join('1'.to_string().repeat(64));
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
        assert!(rows[0].readable, "the fixture is a readable sealed batch");

        let freed = producer.reclaim("project-a", &rows[0]).expect("reclaim");
        assert_eq!(freed, bytes.len() as u64);
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
}
