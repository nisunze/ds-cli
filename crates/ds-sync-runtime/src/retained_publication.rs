//! Closed native publications retained atomically with the common queue row.
//! No admission, engine workflow, inventory adoption or network at completion.
use crate::{rows, seal, store, LocalRow, Producer, SharedStore, TransferReceipt};
use ds_sync_store::{PublicationBytes, PublicationPayload};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, io::Cursor};

const PREFIX: &str = "retained-publication:";
const SCHEMA: &str = "ds.retained-publication/v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Closure {
    schema: String,
    project: String,
    row: LocalRow,
    declaration: Value,
}

pub fn locator(key: &str) -> String {
    format!("{PREFIX}{key}")
}
pub fn owns(row: &LocalRow) -> bool {
    row.bytes_locator == locator(&row.client_publish_id)
}

/// Seal completed immutable bytes without contacting Brain. The caller owns
/// reclamation of the previous row, which may use another producer's storage.
pub fn seal(
    store: &SharedStore,
    fence: &store::Fence,
    project: &str,
    row: &LocalRow,
    declaration: Value,
    outputs: Vec<PublicationBytes>,
    free: &dyn Fn(&store::ArtifactRow) -> Result<u64, String>,
) -> Result<seal::Recorded, String> {
    validate_closure(project, row, &declaration)?;
    let mut ids = BTreeSet::new();
    let mut total = 0u64;
    for output in &outputs {
        if !ids.insert(output.output_id.as_str()) {
            return Err("duplicate retained output".into());
        }
        let declared = row
            .outputs
            .iter()
            .find(|item| item.output_id == output.output_id)
            .ok_or("retained output is not declared")?;
        if declared.size_bytes != output.bytes.len() as u64
            || declared.sha256 != ds_compute_runtime::digest(&output.bytes)
        {
            return Err("retained bytes differ from their closed declaration".into());
        }
        total = total
            .checked_add(declared.size_bytes)
            .ok_or("publication size overflow")?;
    }
    let mut pairs: Vec<(String, String)> = row
        .outputs
        .iter()
        .map(|output| (output.output_id.clone(), output.sha256.clone()))
        .collect();
    if ids.len() != row.outputs.len()
        || total != row.size_bytes
        || rows::inventory_digest(&mut pairs) != row.sha256
    {
        return Err("retained inventory differs from its sealed row".into());
    }
    // Ask the kernel before reclaiming the current producer's bytes. Equal
    // publications and replay-key clashes must leave the standing bytes intact.
    let preview = {
        let held = store.lock().map_err(|_| "The sync gate is unavailable")?;
        let snapshot = held
            .snapshot(fence, &rows::store_scope(project))
            .map_err(|e| e.to_string())?;
        ds_command_kernel::sync_store::apply(&store::Request {
            schema: store::SCHEMA.into(),
            fence: fence.clone(),
            now_ms: rows::now_ms(),
            snapshot,
            event: store::Event::LocalProduced {
                row: rows::store_row(project, row, rows::now_ms()),
            },
        })
        .map_err(|e| e.to_string())?
    };
    let previous = seal::standing(store, fence, project, &row.identity)?;
    if let Some(previous) = previous.as_ref() {
        if previous.replay_key == row.client_publish_id {
            let retained = RetainedProducer {
                store,
                fence,
                project,
            };
            let closure = retained.closure(&row.client_publish_id)?;
            // A retry's observation time and refreshed OCC head are transport
            // facts. Keep the original completion time and immutable closure.
            let mut replay = row.clone();
            replay.produced_at_ms = closure.row.produced_at_ms;
            replay.base_revision = closure.row.base_revision.clone();
            if closure.row != replay || closure.declaration != declaration {
                return Err("immutable publication replay changed".into());
            }
            return Ok(seal::Recorded::AlreadyRecorded {
                replay_key: previous.replay_key.clone(),
                state: previous.state,
            });
        }
        if previous.produced_at_ms > row.produced_at_ms {
            return Err("a later publication already stands for this identity".into());
        }
    }
    if let Some(refusal) = preview.refusals.into_iter().next() {
        if refusal.code == "already_recorded" {
            let held = previous
                .as_ref()
                .ok_or("duplicate publication has no standing row")?;
            return Ok(seal::Recorded::AlreadyRecorded {
                replay_key: held.replay_key.clone(),
                state: held.state,
            });
        }
        return Ok(seal::Recorded::Refused(refusal));
    }
    let receipt = serde_json::to_string(&Closure {
        schema: SCHEMA.into(),
        project: project.into(),
        row: row.clone(),
        declaration,
    })
    .map_err(|error| error.to_string())?;
    seal::record_with_payload(
        store,
        fence,
        project,
        row,
        previous.as_ref(),
        free,
        Some(&PublicationPayload { receipt, outputs }),
    )
}

fn validate_closure(project: &str, row: &LocalRow, declaration: &Value) -> Result<(), String> {
    if !owns(row) || !row.readable {
        return Err("publication has no retained bytes locator".into());
    }
    if ds_command_kernel::sync::declaration::validate(project, declaration)?
        != ds_command_kernel::sync::declaration::Action::Open
    {
        return Err("a retained publication requires an open declaration".into());
    }
    let mut expected = rows::open_declaration(project, row);
    // Only extra closed provenance fields from the native producer's receipt.
    for field in [
        "source_snapshot_sha256",
        "snapshot_receipt_id",
        "printing_preview",
    ] {
        if let Some(value) = declaration.get(field) {
            expected[field] = value.clone();
        }
    }
    if expected != *declaration {
        return Err("publication declaration differs from its sealed row".into());
    }
    Ok(())
}

pub struct RetainedProducer<'a> {
    pub store: &'a SharedStore,
    pub fence: &'a store::Fence,
    pub project: &'a str,
}
impl RetainedProducer<'_> {
    fn closure(&self, key: &str) -> Result<Closure, String> {
        let receipt = self
            .store
            .lock()
            .map_err(|_| "The sync gate is unavailable")?
            .publication_payload(self.fence, self.project, key)
            .map_err(|e| e.to_string())?
            .ok_or("publication bytes are no longer retained")?;
        let closure: Closure =
            serde_json::from_str(&receipt).map_err(|_| "invalid retained publication receipt")?;
        if closure.schema != SCHEMA
            || closure.project != self.project
            || closure.row.client_publish_id != key
        {
            return Err("retained publication receipt crosses its fence".into());
        }
        validate_closure(self.project, &closure.row, &closure.declaration)?;
        Ok(closure)
    }
    /// Verified local read: previews and downloads use the same bytes as upload.
    pub fn output(&self, row: &LocalRow, output_id: &str) -> Result<Vec<u8>, String> {
        let closure = self.closure(&row.client_publish_id)?;
        // OCC base_revision may advance after a head read; producer provenance
        // and the immutable inventory must still equal their committed receipt.
        let mut expected = closure.row.clone();
        expected.base_revision = row.base_revision.clone();
        if expected != *row {
            return Err("publication row differs from its retained receipt".into());
        }
        let declared = row
            .outputs
            .iter()
            .find(|output| output.output_id == output_id)
            .ok_or("requested output is not declared")?;
        let bytes = self
            .store
            .lock()
            .map_err(|_| "The sync gate is unavailable")?
            .publication_output(self.fence, self.project, &row.client_publish_id, output_id)
            .map_err(|e| e.to_string())?
            .ok_or("publication output was retired")?;
        if bytes.len() as u64 != declared.size_bytes
            || ds_compute_runtime::digest(&bytes) != declared.sha256
        {
            return Err("retained output differs from its sealed digest".into());
        }
        Ok(bytes)
    }
}
impl Producer for RetainedProducer<'_> {
    fn inventory(&self, project: &str) -> Result<Vec<LocalRow>, String> {
        if project != self.project {
            return Err("publication inventory crosses its project".into());
        }
        Ok(Vec::new())
    }
    fn amend_open_declaration(&self, project: &str, declaration: &mut Value) -> Result<(), String> {
        if project != self.project {
            return Err("publication declaration crosses its project".into());
        }
        let key = declaration["client_run_id"]
            .as_str()
            .ok_or("publication has no replay key")?;
        let closure = self.closure(key)?;
        let mut expected = closure.declaration.clone();
        for field in [
            "source_snapshot_sha256",
            "snapshot_receipt_id",
            "printing_preview",
        ] {
            expected.as_object_mut().unwrap().remove(field);
        }
        if expected != *declaration {
            return Err("publication declaration differs from its retained receipt".into());
        }
        *declaration = closure.declaration;
        Ok(())
    }
    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String> {
        let bytes = self.output(row, output_id)?;
        let declared = row
            .outputs
            .iter()
            .find(|output| output.output_id == output_id)
            .ok_or("output not declared")?;
        crate::transfer_verified_output(
            output_id,
            session_uri,
            declared.size_bytes,
            &declared.sha256,
            &mut Cursor::new(bytes),
        )
    }
    fn reclaim(&self, project: &str, row: &LocalRow) -> Result<u64, String> {
        if project != self.project || !owns(row) {
            return Err("reclamation crosses its retained publication".into());
        }
        self.store
            .lock()
            .map_err(|_| "The sync gate is unavailable")?
            .retire_publication_payload(self.fence, project, &row.client_publish_id)
            .map_err(|e| e.to_string())
    }
}
