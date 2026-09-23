//! The Solar publication seal, shared by native hosts that run the Solar
//! engine: the row a completed prepared calculation stands for, recorded in
//! the sync store in the same acknowledged step as the completion.
//!
//! Solar's bytes are the completed job's `result` column in the host's own
//! SQLite — the sealed response with its `report_input.json` body — so a
//! Solar seal has no bytes to write: the job's completion wrote them. What
//! it has is the row (`ds-sync-store.md` §6: the store is the queue, and a
//! row is what makes a publication queued), which until 2026-09-20 was
//! derived by the pump from the compute table on every pass and adopted
//! with a `from_producer` receipt — the legacy signal — and was not in the
//! queue at all until a pump with a gateway session had run. Now the host
//! seals it at completion, under its own local fence, with no session and
//! no gateway — and that seal is the ONLY way a Solar row is born (owner,
//! 2026-09-20: "the legacy jobs in solar are not good — we should remove
//! them"). A completed job the store holds no row for — completed before
//! the seal existed, or one whose seal the store could not record — is not
//! a publication and is never adopted: the producer retires its result on
//! its next observation, and the job can be run again. An artefact never
//! stacks on the edge.
//!
//! The choreography is [`crate::seal`]'s, the same one the report seal
//! runs: the city's previous result on this machine is freed through the
//! producer (`reclaimed:superseded_locally`) before the new row takes the
//! identity, because an artefact never accumulates on the edge.
use crate::{store, LocalOutput, LocalRow, SharedStore};
use ds_command_kernel::sync;

/// The engine name the compute-artifact record keys Solar heads by, and
/// the grant engine the Solar publication protocol registers.
pub const ENGINE: &str = "solar";
/// The one output ds-brain admits for a Solar calculation publication.
pub const OUTPUT_ID: &str = "report-input";
/// The locator a Solar row's bytes live at: `solar:prepared:job:<job id>`.
const LOCATOR_PREFIX: &str = "solar:prepared:job:";

/// The locator a completed prepared job's bytes live at.
pub fn locator_of(job_id: &str) -> String {
    format!("{LOCATOR_PREFIX}{job_id}")
}

/// The identity one city's calculation publishes under in a project.
pub fn identity_of(city_id: &str) -> sync::Identity {
    sync::Identity {
        engine: ENGINE.to_string(),
        operation: format!("calculate-{city_id}"),
        variant: "default".to_string(),
    }
}

/// Everything a seal needs, as the producing host established it from the
/// completed job, its sealed input and its retained result.
pub struct SealRequest<'a> {
    pub project_id: &'a str,
    pub city_id: &'a str,
    /// The completed job's id: the row's publish id, replay key and the id
    /// its bytes locator names.
    pub job_id: &'a str,
    pub completed_at_ms: u64,
    pub engine_release: &'a str,
    pub engine_build_manifest_sha256: &'a str,
    /// The sealed snapshot provenance's `input_base_fingerprint`, which the
    /// shared host writes the published head's revision from. A completion
    /// without provenance seals a row the publisher will block, and says
    /// so then, with the row in the queue rather than invisible.
    pub input_base_fingerprint: Option<&'a str>,
    /// The `report_input.json` output as the result declares it.
    pub report_input_sha256: &'a str,
    pub report_input_size_bytes: u64,
}

/// The row a completed prepared job stands for — the one shape the seal
/// records and the producer's adoption inventory reconstructs, so a row
/// sealed at completion and a row adopted later are the same row.
pub fn row_of(request: &SealRequest<'_>) -> Result<LocalRow, String> {
    let outputs = vec![LocalOutput {
        output_id: OUTPUT_ID.into(),
        format: "json".into(),
        content_type: "application/json".into(),
        sha256: request.report_input_sha256.to_string(),
        size_bytes: request.report_input_size_bytes,
    }];
    let mut pairs: Vec<(String, String)> = outputs
        .iter()
        .map(|output| (output.output_id.clone(), output.sha256.clone()))
        .collect();
    let size_bytes = outputs.iter().try_fold(0_u64, |total, output| {
        total
            .checked_add(output.size_bytes)
            .ok_or("Solar outputs overflow their size bound")
    })?;
    Ok(LocalRow {
        identity: identity_of(request.city_id),
        sha256: crate::inventory_digest(&mut pairs),
        size_bytes,
        produced_at_ms: request.completed_at_ms,
        base_revision: request.input_base_fingerprint.map(str::to_string),
        input_base_fingerprint: None,
        readable: true,
        engine_release: request.engine_release.to_string(),
        engine_build_manifest_sha256: request.engine_build_manifest_sha256.to_string(),
        resource: request.city_id.to_string(),
        client_publish_id: request.job_id.to_string(),
        outputs,
        bytes_locator: locator_of(request.job_id),
    })
}

/// What one seal established.
#[derive(Debug)]
pub struct Sealed {
    /// The row as the store now holds it: `held`, queued.
    pub row: store::ArtifactRow,
    /// The job this seal superseded on this machine and freed, if the city
    /// already held another result: `(job id, bytes freed)`.
    pub superseded: Option<(String, u64)>,
}

#[derive(Debug)]
pub enum SealOutcome {
    /// The row is recorded — one acknowledged step after the completion.
    Sealed(Box<Sealed>),
    /// The store already holds this exact publication for the city (the
    /// same job, or the same bytes). Nothing was written; the store's row
    /// is the one that stands.
    AlreadyRecorded {
        replay_key: String,
        state: store::ArtifactState,
    },
    /// The city's standing row is a LATER completion than this one: this
    /// job lost before it was observed — two workers completed two jobs of
    /// one city and the later completion reached the seal first — so it
    /// does not take the identity. Its result is freed through the
    /// producer and told so (`reclaimed:superseded_locally`), exactly as
    /// the row it would have superseded is freed by a seal in order: the
    /// city's row is its newest completion whatever order completions are
    /// observed in, and an artefact never accumulates on the edge.
    Superseded {
        /// The job whose row stands.
        by: String,
        /// This job's bytes freed, when the producer could free them.
        freed: Option<u64>,
    },
}

/// Seal one completed prepared calculation: record its `held` row in the
/// sync store under `fence`. A city that already held another result on
/// this machine has it freed through `free` — the producer's reclaim of
/// that job's result bytes — with a receipt, before the new row takes the
/// identity; a city whose standing row completed LATER than this job keeps
/// it, and this job's result is freed instead ([`SealOutcome::Superseded`]:
/// two workers can complete two jobs of one city and reach the seal out of
/// completion order). If the store refuses or cannot be written, nothing is
/// undone here and nothing adopts the job later: it is not a publication,
/// the producer retires its result on its next observation, and the job
/// can be run again.
pub fn seal(
    store: &SharedStore,
    fence: &store::Fence,
    request: &SealRequest<'_>,
    free: &dyn Fn(&str) -> Result<u64, String>,
) -> Result<SealOutcome, String> {
    if request.job_id.is_empty() {
        return Err("a Solar seal names its completed job".into());
    }
    let row = row_of(request)?;
    let previous = crate::seal::standing(store, fence, request.project_id, &row.identity)?;
    if let Some(previous) = previous.as_ref() {
        if previous.sha256 == row.sha256 || previous.replay_key == row.client_publish_id {
            return Ok(SealOutcome::AlreadyRecorded {
                replay_key: previous.replay_key.clone(),
                state: previous.state,
            });
        }
        if previous.produced_at_ms > row.produced_at_ms {
            let freed = crate::seal::free_superseded(
                store,
                fence,
                request.project_id,
                &row.identity,
                false,
                &row.client_publish_id,
                &previous.client_publish_id,
                &|| free(request.job_id),
            )?;
            return Ok(SealOutcome::Superseded {
                by: previous.client_publish_id.clone(),
                freed,
            });
        }
    }
    let free_previous = |previous: &store::ArtifactRow| free(&previous.client_publish_id);
    match crate::seal::record(
        store,
        fence,
        request.project_id,
        &row,
        previous.as_ref(),
        &free_previous,
    )? {
        crate::seal::Recorded::Recorded { row, superseded } => {
            Ok(SealOutcome::Sealed(Box::new(Sealed {
                row: *row,
                superseded,
            })))
        }
        crate::seal::Recorded::AlreadyRecorded { replay_key, state } => {
            Ok(SealOutcome::AlreadyRecorded { replay_key, state })
        }
        crate::seal::Recorded::Refused(refusal) => Err(format!(
            "the sync store refused the seal ({}): {}",
            refusal.code, refusal.detail
        )),
    }
}
