//! The store half of every producer's seal — one choreography for the
//! report and the Solar producers, so the two sides of every engine record
//! a publication the same way (owner, 2026-09-20: "ds-vector-tiler,
//! ds-solar and ds-network-reporter are located on two sides — they have to
//! use the same patterns").
//!
//! A producer's seal has two halves: the bytes, which are the producer's
//! (a committed batch directory, a completed job's result column), and the
//! row, which is the store's. This module is the row half: the identity's
//! standing row is read, a publication the store already holds is answered
//! as such before anything is written, the previous row's bytes are freed
//! through the producer with a `reclaimed:superseded_locally` receipt —
//! an artefact never accumulates on the edge, and a superseded local result
//! is not the project's artefact — and the new row is recorded `held`.
//! What to do with the new bytes when the store refuses is the producer's
//! decision, taken by its own seal.
use crate::rows::{store_row, store_scope};
use crate::{store, LocalRow, SharedStore};
use ds_command_kernel::sync;

/// What recording one produced row established.
#[derive(Debug)]
pub enum Recorded {
    /// The row is in the store, `held`; the previous row of the identity
    /// on this machine, if it named bytes, was freed: `(publish id, bytes)`.
    Recorded {
        row: Box<store::ArtifactRow>,
        superseded: Option<(String, u64)>,
    },
    /// The store already holds this exact publication for the identity —
    /// or a race recorded it first. Nothing was written; the store's row is
    /// the one that stands.
    AlreadyRecorded {
        replay_key: String,
        state: store::ArtifactState,
    },
    /// The store refused the row for another reason (a replay key naming
    /// another identity). Nothing was written.
    Refused(store::Refusal),
}

/// The row the store holds for `identity` in `project`, if any — read
/// before a producer writes bytes, so a duplicate seal writes none.
pub fn standing(
    store: &SharedStore,
    fence: &store::Fence,
    project: &str,
    identity: &sync::Identity,
) -> Result<Option<store::ArtifactRow>, String> {
    let store = store
        .lock()
        .map_err(|_| "The sync gate is unavailable".to_string())?;
    let snapshot = store
        .snapshot(fence, &store_scope(project))
        .map_err(|error| error.to_string())?;
    Ok(snapshot
        .artifacts
        .into_iter()
        .find(|row| &row.identity == identity))
}

/// Free one publication's bytes through the producer and tell the store
/// so: a `reclaimed:superseded_locally` receipt naming the publication
/// that lost (`loser`) and the one it lost to (`winner`), with the bytes
/// freed; a `free` that fails is a `reclaim_failed` receipt and the bytes
/// are freed in the open on the next observation. What it answers is the
/// bytes freed, when they were.
///
/// The kernel keys the `reclaimed` transition — bytes gone, locator
/// cleared — by the receipt's identity, so the receipt carries `identity`
/// only when the loser IS the identity's standing row (`loser_is_the_row`:
/// [`record`] freeing the previous row before the new one takes the
/// identity). A loser that never held the row — a completion a producer
/// observes AFTER a later one of the same identity, replayed after a
/// restart — is freed with a receipt that names no identity, so the
/// standing row keeps its bytes; the detail still names the operation.
#[allow(clippy::too_many_arguments)]
pub fn free_superseded(
    store: &SharedStore,
    fence: &store::Fence,
    project: &str,
    identity: &sync::Identity,
    loser_is_the_row: bool,
    loser: &str,
    winner: &str,
    free: &dyn Fn() -> Result<u64, String>,
) -> Result<Option<u64>, String> {
    let now = crate::now_ms();
    let (action, outcome, bytes) = match free() {
        Ok(bytes) => ("reclaimed", "superseded_locally".to_string(), Some(bytes)),
        Err(error) => (
            "reclaim_failed",
            format!("superseded_locally: {error}"),
            None,
        ),
    };
    let receipt = store::ReceiptRow {
        receipt_id: format!("{action}:{now}:{}", uuid::Uuid::new_v4().simple()),
        scope: store_scope(project),
        action: action.to_string(),
        identity: loser_is_the_row.then(|| identity.clone()),
        outcome,
        detail: Some(format!(
            "{loser} lost to a later computation of {} sealed on this machine ({winner})",
            identity.operation
        )),
        grant_id: None,
        work_id: None,
        head_revision: None,
        head_sha256: None,
        head_counter: None,
        committed_bytes: bytes,
        at_ms: now,
    };
    let mut store = store
        .lock()
        .map_err(|_| "The sync gate is unavailable".to_string())?;
    store
        .apply(fence, now, store::Event::ActionReceipt { receipt })
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

/// Record `row` as `held` for `project`. `previous` is the identity's
/// standing row as [`standing`] read it before the producer wrote its
/// bytes; when it still names bytes on this machine they are freed through
/// `free` FIRST and told so with a receipt, because the kernel keys the
/// `reclaimed` transition by identity and afterwards the identity is the
/// new row's. A `free` that fails is a `reclaim_failed` receipt and the new
/// row is still recorded: the next observation frees the bytes in the open.
pub fn record(
    store: &SharedStore,
    fence: &store::Fence,
    project: &str,
    row: &LocalRow,
    previous: Option<&store::ArtifactRow>,
    free: &dyn Fn(&store::ArtifactRow) -> Result<u64, String>,
) -> Result<Recorded, String> {
    let now = crate::now_ms();
    let with_store = |event: store::Event| -> Result<store::Transition, String> {
        let mut store = store
            .lock()
            .map_err(|_| "The sync gate is unavailable".to_string())?;
        store
            .apply(fence, now, event)
            .map_err(|error| error.to_string())
    };
    let mut superseded = None;
    if let Some(previous) = previous {
        if previous.readable && !previous.bytes_locator.is_empty() {
            let bytes = free_superseded(
                store,
                fence,
                project,
                &row.identity,
                true,
                &previous.client_publish_id,
                &row.client_publish_id,
                &|| free(previous),
            )?;
            if let Some(bytes) = bytes {
                superseded = Some((previous.client_publish_id.clone(), bytes));
            }
        }
    }
    let stored = store_row(project, row, now);
    let transition = with_store(store::Event::LocalProduced {
        row: stored.clone(),
    })?;
    if let Some(refusal) = transition.refusals.into_iter().next() {
        if refusal.code == "already_recorded" {
            // A race with another seal of the same publication: theirs
            // stands, and it is the row this identity now holds.
            let held = standing(store, fence, project, &row.identity)?
                .ok_or_else(|| format!("the sync store refused the seal: {}", refusal.detail))?;
            return Ok(Recorded::AlreadyRecorded {
                replay_key: held.replay_key,
                state: held.state,
            });
        }
        return Ok(Recorded::Refused(refusal));
    }
    Ok(Recorded::Recorded {
        row: Box::new(stored),
        superseded,
    })
}
