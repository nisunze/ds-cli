//! One report publication producer, shared by native hosts.
use crate::{kernel_sync as sync, LocalOutput, LocalRow, Producer, TransferReceipt, MAX_ROWS};
use ds_report_artifacts::SealedArtifactHandle;
use std::path::Path;

pub type ArtifactTransfer<'a> =
    dyn Fn(SealedArtifactHandle, &str, &str) -> Result<TransferReceipt, String> + 'a;

pub struct ReportProducer<'a> {
    pub root: &'a Path,
    pub project: &'a str,
    pub upload: &'a ArtifactTransfer<'a>,
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
