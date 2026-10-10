//! Exact selected-cell persistence. The pure repair owner re-derives the
//! proposal; the existing SQLite transaction owns CAS, replay and the outbox.
use super::*;
use ds_command_kernel::design_repair::{self, ApplySelection, EvidenceSource, Proposal};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairRequest {
    pub schema: String,
    pub transformer: String,
    pub operation_id: String,
    pub source: Option<EvidenceSource>,
    pub proposal: Proposal,
    pub selection: ApplySelection,
}
impl Workspace {
    pub fn repair(&mut self, bytes: &[u8]) -> Result<Value> {
        if bytes.len() > design::MAX_BYTES {
            return Err(invalid(
                "repair_bound_exceeded: repair request exceeds 64 MiB",
            ));
        }
        let input: RepairRequest = serde_json::from_slice(bytes)?;
        if input.schema != "ds.design.repair/v1" {
            return Err(invalid(
                "repair_document_invalid: expected ds.design.repair/v1",
            ));
        }
        design::identifier(&input.operation_id).map_err(invalid)?;
        let raw: Value = serde_json::from_slice(bytes)?;
        let request_digest = design::digest(&raw).map_err(invalid)?;
        if let Some(receipt) = self.replay(&input.operation_id, &request_digest)? {
            return Ok(receipt);
        }
        let project: String = self
            .db
            .query_row("SELECT project FROM meta", [], |r| r.get(0))?;
        if input.proposal.request.scope.project != project {
            return Err(invalid(
                "repair_scope_mismatch: proposal belongs to another workspace project",
            ));
        }
        let expected = &input.proposal.request.target.snapshot_revision;
        let (_, snapshot) = self.read(&input.transformer, Some(expected))?;
        let applied = design_repair::apply_selected(
            input.source.as_ref(),
            &snapshot,
            &input.proposal,
            &input.selection,
        )
        .map_err(invalid)?;
        let after = applied.after_revision.clone();
        let projected = serde_json::to_value(&applied.snapshot)?;
        let receipt = self.commit(
            applied.snapshot,
            Some(expected),
            &input.operation_id,
            &request_digest,
            "repair",
        )?;
        // Verify the exact retained revision, even if another writer advances
        // the current head after our transaction. Publication remains pending.
        let (revision, retained) = self.read(&input.transformer, Some(&after))?;
        if revision != after || serde_json::to_value(retained)? != projected {
            return Err(invalid(
                "repair_readback_mismatch: retained result differs from the selected projection",
            ));
        }
        Ok(receipt)
    }
}
