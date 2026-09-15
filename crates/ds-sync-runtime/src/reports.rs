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
}

pub fn inventory(root: &Path, project: &str) -> Result<Vec<LocalRow>, String> {
    let batches = ds_report_artifacts::list_project_publication_batches_from_root(root, project)?;
    let mut rows: std::collections::BTreeMap<sync::Identity, LocalRow> =
        std::collections::BTreeMap::new();
    for batch in batches {
        let identity = sync::Identity {
            engine: batch.engine.clone(),
            operation: batch.operation.clone(),
            variant: batch.variant.clone(),
        };
        if rows.contains_key(&identity) {
            continue;
        }
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
                // The durable row retains the computed source fingerprint.
                // kernel_local projects it as input_base_fingerprint so the
                // kernel compares it with admission, not an older output head.
                base_revision: Some(batch.input_base_fingerprint.clone())
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
