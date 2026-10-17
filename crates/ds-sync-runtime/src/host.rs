//! The durable half of every native host: the sync store, the lease, the
//! grants, the heads and the receipts — kept in `ds_sync_store` under one
//! fence, so closing a window, a crash or a restart loses nothing.
//!
//! [`StoreHost`] implements [`SyncHost`] once for desktop, the `ds` CLI and
//! the server. What differs between hosts is supplied through three narrow
//! seams: where the bytes come from ([`Producer`]), how the record is reached
//! ([`crate::gateway::Gateway`]) and where downloaded heads land ([`Reads`]).
//! No host re-implements a store transition.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ds_command_kernel::sync;
use ds_command_kernel::sync_store as store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::gateway::{Gateway, GatewayError};
use crate::lease::LeaseKeeper;
use crate::routes::SyncRoute;
use crate::rows::{
    head_from_remote, local_row_from_store, now_ms, parse_record_answer, receipt_row, remote_row,
    remote_row_from_head, store_row, store_scope, text, LocalRow, Receipt, RecordAnswer, RemoteRow,
    TransferReceipt, MAX_ROWS,
};
use crate::run::SyncHost;

/// The shared public projection, usable over an already opened read-only store.
/// The caller supplies retained native activities; no producer observation runs.
pub fn store_read_projection(
    store: Option<&ds_sync_store::Store>,
    fence: &store::Fence,
    project: &str,
    activities: Vec<ActivityRow>,
    at_ms: u64,
) -> Result<Value, String> {
    let scope = store_scope(project);
    let (snapshot, receipts, read_at, status) = match store {
        Some(store) => (
            store
                .snapshot(fence, &scope)
                .map_err(|error| error.to_string())?,
            store
                .receipts(fence, &scope, 200)
                .map_err(|error| error.to_string())?,
            store
                .heads_read_at_ms(fence, &scope)
                .map_err(|error| error.to_string())?,
            store
                .queue(fence, Some(project), at_ms)
                .map_err(|error| error.to_string())?,
        ),
        None => (
            store::Snapshot {
                artifacts: vec![],
                heads: vec![],
                grants: vec![],
                lease: None,
            },
            vec![],
            None,
            store::queue_status(&[], &[], &[], Some(project), at_ms),
        ),
    };
    let artifacts: Vec<Value> = snapshot
        .artifacts
        .iter()
        .map(|row| {
            let mut value = serde_json::to_value(row).unwrap_or(Value::Null);
            if let Value::Object(map) = &mut value {
                map.remove("transfer_state");
            }
            value
        })
        .collect();
    Ok(json!({
        "schema": "ds.sync-store-read/v1",
        "project": project,
        "artifacts": artifacts,
        "activities": activities,
        "heads": snapshot.heads,
        "readAtMs": read_at,
        "grants": snapshot.grants.iter().map(|grant| json!({
            "grantId": grant.grant_id,
            "expiresAtMs": grant.expires_at_ms,
            "engines": grant.engines,
        })).collect::<Vec<_>>(),
        "lease": snapshot.lease,
        "receipts": receipts,
        "status": status,
    }))
}

/// How long one worker may hold a project's lease without renewing it: a
/// dead worker's lease expires and another recovers the work.
pub const LEASE_TTL_MS: u64 = 5 * 60 * 1000;

/// The store as every host shares it: one open connection, serialised.
pub type SharedStore = Arc<Mutex<ds_sync_store::Store>>;

/// Bounded lifecycle evidence from a native compute owner.  It is rendered by
/// the existing Sync Center store read and never schedules work itself.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRow {
    pub id: String,
    pub engine: String,
    pub state: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Where this host's bytes come from, and how they leave it.
///
/// The sync store is the queue (owner, 2026-09-20). A producer holds BYTES
/// and names them by the row's `bytes_locator`; it never decides what is
/// queued. Its inventory is read only to adopt what was sealed before the
/// seal wrote its row — report batches only: a Solar completion with no
/// row is not a publication and is never adopted — and to free what the
/// store has settled as lost.
pub trait Producer {
    /// Add only provenance retained by this producer's committed receipt.
    /// The declaration is already closed; a missing or crossed receipt refuses.
    fn amend_open_declaration(
        &self,
        _project: &str,
        _declaration: &mut Value,
    ) -> Result<(), String> {
        Ok(())
    }
    /// Every publication this producer still holds bytes for in one project,
    /// each carrying the locator its bytes live at. Cheap: it reads the
    /// producer's own receipts, never the bytes. Project-keyed, never
    /// account-keyed. A row the store already knows is ignored by the host;
    /// one it does not is adopted once.
    fn inventory(&self, project: &str) -> Result<Vec<LocalRow>, String>;
    /// Stream one declared output's bytes into a DS-minted resumable session
    /// through this host's one native transport host. Every resume point and
    /// retry inside is `ds_command_kernel::transfer`'s.
    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String>;

    /// Evidence from the producer's existing durable lifecycle owner. Legacy
    /// producers have no compute lifecycle to project.
    fn activity(&self, _project: &str) -> Result<Vec<ActivityRow>, String> {
        Ok(Vec::new())
    }
    /// Free the bytes of one of this producer's rows that lost, through the
    /// producer store's own retirement path. Answers the bytes freed; a row
    /// whose bytes are already gone is `Ok(0)`. Every producer answers: a
    /// producer that held no bytes of its own says so with 0, and one that
    /// could not free them says why, so no reclaim is ever silent.
    fn reclaim(&self, project: &str, row: &LocalRow) -> Result<u64, String>;
}

/// Where downloaded heads land on this host.
pub trait Reads {
    /// A host-owned read cache directory for one head, keyed by project and
    /// artifact identity and by nothing else.
    fn head_root(&self, project: &str, identity: &sync::Identity) -> Result<PathBuf, String>;
    /// Download `url` into `destination`, proving the digest and size before
    /// the file is placed. Copied metadata is never that proof.
    fn download_verified(
        &self,
        url: &str,
        destination: &Path,
        sha256: &str,
        size_bytes: u64,
    ) -> Result<(), String>;
}

/// The one seam, implemented over the durable store.
pub struct StoreHost<'a> {
    store: SharedStore,
    /// The fence every row this host reads or writes lives under.
    fence: store::Fence,
    install_id: String,
    /// Who holds the project's lease while this process drains it.
    worker_id: String,
    /// The project this pass is about.
    project: String,
    gateway: &'a dyn Gateway,
    producer: &'a dyn Producer,
    reads: &'a dyn Reads,
    online: &'a dyn Fn() -> bool,
    lease_keeper: Mutex<Option<LeaseKeeper>>,
    /// Receipts `local` produced on its own — bytes freed for rows the store
    /// already held as lost — waiting for the run to take them.
    housekeeping: Mutex<Vec<Receipt>>,
}

impl<'a> StoreHost<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: SharedStore,
        fence: store::Fence,
        worker_id: String,
        project: &str,
        gateway: &'a dyn Gateway,
        producer: &'a dyn Producer,
        reads: &'a dyn Reads,
        online: &'a dyn Fn() -> bool,
    ) -> Result<Self, String> {
        if fence.account.trim().is_empty() {
            return Err("The sync gate has no account for this session".to_string());
        }
        if fence.deployment.trim().is_empty() || fence.install_id.trim().is_empty() {
            return Err("The sync gate has no deployment or install for this session".to_string());
        }
        if worker_id.trim().is_empty() {
            return Err("The sync gate has no worker identity".to_string());
        }
        Ok(StoreHost {
            store,
            install_id: fence.install_id.clone(),
            fence,
            worker_id: format!("{worker_id}#{}", uuid::Uuid::new_v4()),
            lease_keeper: Mutex::new(None),
            housekeeping: Mutex::new(Vec::new()),
            project: project.to_string(),
            gateway,
            producer,
            reads,
            online,
        })
    }

    fn ensure_active(&self) -> Result<(), String> {
        if !(self.online)() {
            return Err("sync authority or connectivity is unavailable".into());
        }
        let keeper = self
            .lease_keeper
            .lock()
            .map_err(|_| "sync lease unavailable")?;
        if let Some(keeper) = keeper.as_ref() {
            if !keeper.valid()
                || !self.snapshot()?.lease.is_some_and(|lease| {
                    lease.worker_id == self.worker_id && lease.expires_at_ms > now_ms()
                })
            {
                return Err("sync lease was lost; this pass is fenced".into());
            }
        }
        Ok(())
    }

    pub fn fence(&self) -> &store::Fence {
        &self.fence
    }

    fn retained_for_recovery(&self, row: &store::ArtifactRow) -> Result<bool, String> {
        self.with_store(|store| {
            let attempt = store
                .last_publication_attempt(&self.fence, row)
                .map_err(|error| error.to_string())?;
            Ok(store::recovery::retry_eligible(row, attempt.as_ref()))
        })
    }

    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    fn with_store<T>(
        &self,
        f: impl FnOnce(&mut ds_sync_store::Store) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut store = self
            .store
            .lock()
            .map_err(|_| "The sync gate is unavailable".to_string())?;
        f(&mut store)
    }

    /// One store transition under this fence, at the host's clock.
    pub fn apply(&self, event: store::Event) -> Result<store::Transition, String> {
        let now = now_ms();
        self.with_store(|store| {
            store
                .apply(&self.fence, now, event)
                .map_err(|error| error.to_string())
        })
    }

    pub fn snapshot(&self) -> Result<store::Snapshot, String> {
        self.with_store(|store| {
            store
                .snapshot(&self.fence, &store_scope(&self.project))
                .map_err(|error| error.to_string())
        })
    }

    pub fn read_at(&self) -> Result<Option<u64>, String> {
        self.with_store(|store| {
            store
                .heads_read_at_ms(&self.fence, &store_scope(&self.project))
                .map_err(|error| error.to_string())
        })
    }

    pub fn receipts(&self, limit: usize) -> Result<Vec<store::ReceiptRow>, String> {
        self.with_store(|store| {
            store
                .receipts(&self.fence, &store_scope(&self.project), limit)
                .map_err(|error| error.to_string())
        })
    }

    pub fn known_artifacts(&self) -> Result<Vec<store::ArtifactRow>, String> {
        self.with_store(|store| {
            store
                .artifacts_of_fence(&self.fence)
                .map_err(|error| error.to_string())
        })
    }

    /// Cancel one held or uploading publication. Committed work is never
    /// undone; the store answers with its own refusal when it is not.
    pub fn cancel(&self, identity: &sync::Identity) -> Result<store::Transition, String> {
        self.apply(store::Event::Cancel {
            scope: store_scope(&self.project),
            identity: identity.clone(),
            expected_replay_key: None,
        })
    }

    /// Cancel the exact publication observed by a durable compute job. The
    /// kernel compares this key in the same store transition, so a newer row
    /// that replaced the identity cannot be cancelled by an older job.
    pub fn cancel_publication(
        &self,
        identity: &sync::Identity,
        expected_replay_key: &str,
    ) -> Result<store::Transition, String> {
        if expected_replay_key.is_empty() {
            return Err("publication cancellation has no replay key".into());
        }
        self.apply(store::Event::Cancel {
            scope: store_scope(&self.project),
            identity: identity.clone(),
            expected_replay_key: Some(expected_replay_key.into()),
        })
    }

    /// What the store holds for one project, for a Sync Center to render: the
    /// artifact rows (without the transport's in-flight state, which can carry
    /// a scoped upload credential), the heads, the grants' public facts, the
    /// lease and the latest receipts. Nothing here is decided; it is read.
    pub fn store_read(&self) -> Result<Value, String> {
        let activities = self.producer.activity(&self.project)?;
        self.with_store(|store| {
            store_read_projection(
                Some(store),
                &self.fence,
                &self.project,
                activities,
                now_ms(),
            )
        })
    }

    /// Free one row the store already holds as lost, and keep the evidence.
    /// Never silent: a batch already gone is a `reclaimed` receipt of 0
    /// bytes, which is what tells the store the row names nothing here any
    /// more — so a second observation does not ask again.
    fn reclaim_settled(&self, project: &str, row: &LocalRow, verdict: &str) -> Result<(), String> {
        let name = if row.client_publish_id.is_empty() {
            "a replica of the head".to_string()
        } else {
            format!("batch {}", row.client_publish_id)
        };
        let receipt = match self.producer.reclaim(project, row) {
            Ok(0) => {
                let mut receipt = Receipt::new("reclaimed", verdict.to_string())
                    .about(&row.identity)
                    .detail(format!(
                        "{name} held no bytes on this machine any more; the store holds this publication as {verdict}"
                    ));
                receipt.bytes = Some(0);
                receipt
            }
            Ok(bytes) => {
                let mut receipt = Receipt::new("reclaimed", verdict.to_string())
                    .about(&row.identity)
                    .detail(format!(
                        "{name} freed: the store holds this publication as {verdict}, and online overwrites local"
                    ));
                receipt.bytes = Some(bytes);
                receipt
            }
            Err(error) => Receipt::new("reclaim_failed", verdict.to_string())
                .about(&row.identity)
                .detail(format!("{name} was not freed: {error}")),
        };
        let receipt = receipt.for_publication(row);
        self.record_receipt(project, &receipt)?;
        self.housekeeping
            .lock()
            .map_err(|_| "sync housekeeping receipts unavailable".to_string())?
            .push(receipt);
        Ok(())
    }
}

impl SyncHost for StoreHost<'_> {
    fn online(&self) -> bool {
        (self.online)()
    }

    fn server_now_ms(&self) -> u64 {
        now_ms()
    }

    fn install_id(&self) -> String {
        self.install_id.clone()
    }

    fn credential_block(&self, project: &str) -> Result<Option<store::QueueBlock>, String> {
        self.with_store(|store| {
            store
                .queue(&self.fence, Some(project), now_ms())
                .map(|status| status.blocked)
                .map_err(|error| error.to_string())
        })
    }

    /// The store's rows ARE the inventory: every row that is not settled as
    /// lost, as the store holds it, plus the replicas of heads this machine
    /// has downloaded. The producer is consulted for two things only, and
    /// both are reconciliations, not decisions:
    ///
    /// * a batch it holds bytes for that the store does not know — sealed
    ///   before the seal wrote its row (2026-09-20) — is adopted once, with
    ///   an `adopted` receipt; a producer whose every batch is known adds
    ///   nothing here;
    /// * a row the store holds as lost (`conflict` / `refused`) whose bytes
    ///   are still here is freed through the producer, once, with a
    ///   `reclaimed` receipt the kernel turns into "bytes gone" on the row.
    ///   Online overwrites local; a lost batch never stays.
    fn local(&self, project: &str) -> Result<Vec<LocalRow>, String> {
        let mut snapshot = self.snapshot()?;
        let now = now_ms();
        let held = self.producer.inventory(project)?;
        let mut adopted = false;
        // A batch the producer holds that the store has settled as lost, by
        // publish id: its bytes are freed below whether or not the row still
        // says they are here — a store restored beside old artifacts, or an
        // earlier failed reclaim, must not leave them orphaned.
        let mut to_free: Vec<(LocalRow, String)> = Vec::new();
        for row in &held {
            let standing = snapshot.artifacts.iter().find(|stored| {
                stored.replay_key == row.client_publish_id
                    || (stored.identity == row.identity && stored.sha256 == row.sha256)
            });
            let historical = if standing.is_none() {
                self.with_store(|store| {
                    store
                        .artifact(&self.fence, &store_scope(project), &row.client_publish_id)
                        .map_err(|error| error.to_string())
                })?
            } else {
                None
            };
            let known = standing.or(historical.as_ref());
            match known {
                None => {
                    let recorded = self.apply(store::Event::LocalProduced {
                        row: store_row(project, row, now),
                    })?;
                    // A refusal is the store's answer (a replay key already
                    // naming another identity), kept as evidence and never
                    // a failure of this observation.
                    let receipt = match recorded.refusals.first() {
                        None => Receipt::new("adopted", "from_producer")
                            .about(&row.identity)
                            .detail(format!(
                                "batch {} was held by the producer with no row in the store (sealed before the seal wrote its row); recorded once",
                                row.client_publish_id
                            )),
                        Some(refusal) => Receipt::new("adopted", refusal.code.clone())
                            .about(&row.identity)
                            .detail(format!(
                                "batch {} was not adopted: {}",
                                row.client_publish_id, refusal.detail
                            )),
                    };
                    self.record_receipt(project, &receipt)?;
                    self.housekeeping
                        .lock()
                        .map_err(|_| "sync housekeeping receipts unavailable".to_string())?
                        .push(receipt);
                    adopted = true;
                }
                Some(stored)
                    if !stored.archived
                        && matches!(stored.state.as_str(), "conflict" | "refused")
                        && !self.retained_for_recovery(stored)? =>
                {
                    to_free.push((row.clone(), stored.state.as_str().to_string()));
                }
                Some(_) => {}
            }
        }
        if adopted {
            snapshot = self.snapshot()?;
        }
        // Rows the store holds as lost whose bytes it still names: freed
        // once. After the receipt the row says its bytes are gone and is not
        // asked about again; a failed reclaim leaves it, and the next pass
        // tries again in the open.
        for stored in &snapshot.artifacts {
            if stored.archived
                || !matches!(stored.state.as_str(), "conflict" | "refused")
                || self.retained_for_recovery(stored)?
            {
                continue;
            }
            if !(stored.readable && !stored.bytes_locator.is_empty()) {
                continue;
            }
            if to_free
                .iter()
                .any(|(row, _)| row.client_publish_id == stored.replay_key)
            {
                continue;
            }
            to_free.push((
                local_row_from_store(stored),
                stored.state.as_str().to_string(),
            ));
        }
        for (row, verdict) in to_free {
            self.reclaim_settled(project, &row, &verdict)?;
        }
        let mut rows: Vec<LocalRow> = snapshot
            .artifacts
            .iter()
            .filter(|stored| {
                !stored.archived && !matches!(stored.state.as_str(), "conflict" | "refused")
            })
            .map(local_row_from_store)
            .collect();
        for stored in &snapshot.heads {
            let head = remote_row_from_head(stored);
            // A producer's readable local result remains authoritative for its
            // own edit; a replica must never replace it or enter the outbox.
            if rows
                .iter()
                .any(|row| row.identity == head.identity && row.readable)
            {
                continue;
            }
            let root = self.reads.head_root(project, &head.identity)?;
            if !crate::reads::held_head(&root, &head)? {
                continue;
            }
            let size_bytes = head
                .outputs
                .iter()
                .try_fold(0_u64, |total, output| total.checked_add(output.size_bytes))
                .ok_or("cached head size overflow")?;
            rows.retain(|row| row.identity != head.identity);
            rows.push(LocalRow {
                identity: head.identity,
                sha256: head.sha256,
                size_bytes,
                produced_at_ms: head.published_at_ms,
                base_revision: Some(head.revision),
                input_base_fingerprint: head.input_base_fingerprint,
                readable: true,
                engine_release: String::new(),
                engine_build_manifest_sha256: String::new(),
                resource: String::new(),
                client_publish_id: String::new(),
                outputs: Vec::new(),
                bytes_locator: String::new(),
            });
        }
        Ok(rows)
    }

    fn held_heads(&self, _project: &str) -> Result<(Vec<RemoteRow>, Option<u64>), String> {
        let heads = self
            .snapshot()?
            .heads
            .iter()
            .map(remote_row_from_head)
            .collect();
        Ok((heads, self.read_at()?))
    }

    fn remember_heads(
        &self,
        project: &str,
        heads: &[RemoteRow],
        _read_at_ms: u64,
    ) -> Result<(), String> {
        let transition = self.apply(store::Event::HeadsRead {
            scope: store_scope(project),
            heads: heads
                .iter()
                .map(|head| head_from_remote(project, head))
                .collect(),
        })?;
        if let Some(refusal) = transition.refusals.first() {
            return Err(refusal.detail.clone());
        }
        Ok(())
    }

    fn record_receipt(&self, project: &str, receipt: &Receipt) -> Result<(), String> {
        if !matches!(
            receipt.action,
            "open_grant"
                | "upload"
                | "download"
                | "session"
                | "conflict"
                | "refused"
                | "cancel"
                | "reclaimed"
                | "reclaim_failed"
                | "adopted"
        ) {
            return Ok(());
        }
        if receipt.outcome == "published" || receipt.outcome == "committed" {
            self.ensure_active()?;
        }
        let recorded = self.apply(store::Event::ActionReceipt {
            receipt: receipt_row(project, receipt, now_ms()),
        })?;
        if let Some(refusal) = recorded.refusals.first() {
            return Err(refusal.detail.clone());
        }
        Ok(())
    }

    fn take_lease(&self, project: &str) -> Result<Option<String>, String> {
        let transition = self.apply(store::Event::LeaseTake {
            scope: store_scope(project),
            worker_id: self.worker_id.clone(),
            ttl_ms: LEASE_TTL_MS,
        })?;
        if let Some(refusal) = transition.refusals.first() {
            return Ok(Some(format!("{}: {}", refusal.code, refusal.detail)));
        }
        match LeaseKeeper::start(
            self.store.clone(),
            self.fence.clone(),
            project.to_string(),
            self.worker_id.clone(),
            LEASE_TTL_MS,
        ) {
            Ok(keeper) => {
                *self
                    .lease_keeper
                    .lock()
                    .map_err(|_| "sync lease unavailable")? = Some(keeper);
                Ok(None)
            }
            Err(error) => {
                self.release_lease(project);
                Err(error)
            }
        }
    }

    fn release_lease(&self, project: &str) {
        if let Ok(mut keeper) = self.lease_keeper.lock() {
            drop(keeper.take());
        }
        let _ = self.apply(store::Event::LeaseRelease {
            scope: store_scope(project),
            worker_id: self.worker_id.clone(),
        });
    }

    fn heads(&self, project: &str) -> Result<Vec<RemoteRow>, GatewayError> {
        let mut rows: Vec<RemoteRow> = Vec::new();
        let mut cursor = String::new();
        let mut seen = std::collections::BTreeSet::new();
        loop {
            self.ensure_active()?;
            // Match the service's ordinary page size. A whole project's
            // heads need not fit one control-plane request on a slow link;
            // the cursor walk still proves the complete bounded inventory.
            let mut body = json!({"action": "list", "project_id": project, "limit": 25});
            if !cursor.is_empty() {
                body["cursor"] = json!(cursor);
            }
            let page = self.gateway.post(SyncRoute::ComputeArtifacts, &body)?;
            let heads = page
                .get("heads")
                .and_then(Value::as_array)
                .ok_or("the record did not return a head inventory")?;
            for head in heads {
                rows.push(remote_row(head));
            }
            cursor = text(&page, "next_cursor");
            if rows.len() > MAX_ROWS || (!cursor.is_empty() && rows.len() == MAX_ROWS) {
                return Err("the complete head inventory exceeds its bound".into());
            }
            if cursor.is_empty() {
                break;
            }
            if heads.is_empty() || !seen.insert(cursor.clone()) {
                return Err("head pagination did not advance".into());
            }
        }
        Ok(rows)
    }

    fn open_publication(&self, declaration: &Value) -> Result<RecordAnswer, GatewayError> {
        self.ensure_active()?;
        let mut declaration = declaration.clone();
        self.producer
            .amend_open_declaration(&self.project, &mut declaration)?;
        let answer = self
            .gateway
            .post(SyncRoute::ComputeArtifacts, &declaration)?;
        self.ensure_active()?;
        Ok(parse_record_answer(&answer)?)
    }

    fn finalize_publication(&self, declaration: &Value) -> Result<RecordAnswer, GatewayError> {
        self.ensure_active()?;
        let answer = self
            .gateway
            .post(SyncRoute::ComputeArtifacts, declaration)?;
        Ok(parse_record_answer(&answer)?)
    }

    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String> {
        self.ensure_active()?;
        let receipt = self.producer.transfer(row, output_id, session_uri)?;
        self.ensure_active()?;
        Ok(receipt)
    }

    fn fetch(&self, project: &str, head: &RemoteRow) -> Result<u64, GatewayError> {
        self.ensure_active()?;
        crate::reads::validate_output_names(head)?;
        let root = self.reads.head_root(project, &head.identity)?;
        let mut bytes = 0_u64;
        for output in &head.outputs {
            self.ensure_active()?;
            let ticket = self.gateway.post(
                SyncRoute::ComputeArtifacts,
                &json!({
                    "action": "download",
                    "project_id": project,
                    "engine": head.identity.engine,
                    "operation": head.identity.operation,
                    "variant": head.identity.variant,
                    "output_id": output.output_id,
                }),
            )?;
            let url = text(&ticket, "download_url");
            if url.is_empty() {
                return Err("that head output has no readable location".into());
            }
            self.reads.download_verified(
                &url,
                &root.join(&output.output_id),
                &output.sha256,
                output.size_bytes,
            )?;
            self.ensure_active()?;
            bytes = bytes
                .checked_add(output.size_bytes)
                .ok_or("download byte count overflow")?;
        }
        let receipt = serde_json::to_vec(&json!({
            "revision": head.revision,
            "head_revision": head.head_revision,
            "input_base_fingerprint": head.input_base_fingerprint,
            "sha256": head.sha256,
            "published_at_ms": head.published_at_ms,
        }))
        .map_err(|error| error.to_string())?;
        use sha2::Digest;
        crate::reads::place_verified(
            &receipt[..],
            &root.join("head.json"),
            &format!("{:x}", sha2::Sha256::digest(&receipt)),
            receipt.len() as u64,
        )?;
        Ok(bytes)
    }

    fn reclaim(&self, project: &str, row: &LocalRow) -> Result<u64, String> {
        self.producer.reclaim(project, row)
    }

    fn take_receipts(&self, _project: &str) -> Vec<Receipt> {
        self.housekeeping
            .lock()
            .map(|mut receipts| std::mem::take(&mut *receipts))
            .unwrap_or_default()
    }
}

/// One producer set over every engine a host runs, keyed by the engine a
/// row's identity names — the host's whole answer to "where do this
/// machine's bytes live", so a row of ANY engine is adopted, moved and freed
/// through the producer that owns its bytes.
///
/// WHY (2026-09-20): [`StoreHost::local`] frees every row the store holds
/// as lost through the ONE producer the host was built with. The Server ran
/// its report pass over the report producer and its Solar pass over the
/// Solar producer, so a lost Solar row observed by the report pass was
/// "reclaimed" with 0 bytes — the report producer holds no batch by that
/// id — and the kernel cleared the row's locator while the result stayed in
/// the compute store; the desktop, built with the report producer only, did
/// the same to every Solar row. That is the gap the two-sided-engines rule
/// names: a producer that answers `Ok(0)` because "its bytes live
/// elsewhere". One set, keyed by engine, closes it on both hosts: the
/// producer asked is always the one whose engine produced the row, and a
/// row of an engine this host has no producer for is a `reclaim_failed`
/// that says so — never a silent 0 that tells the store the bytes are gone.
pub struct Producers<'a> {
    engines: Vec<(&'a str, &'a dyn Producer)>,
}

impl<'a> Producers<'a> {
    /// The set, one producer per engine. Two producers for one engine would
    /// make "the producer that owns the bytes" ambiguous, so the set refuses
    /// them; an empty set is [`StoreOnlyProducer`]'s job, not this one's.
    pub fn new(engines: Vec<(&'a str, &'a dyn Producer)>) -> Result<Self, String> {
        if engines.is_empty() {
            return Err("a producer set names at least one engine".into());
        }
        for (index, (engine, _)) in engines.iter().enumerate() {
            if engine.trim().is_empty() {
                return Err("a producer set names its engines".into());
            }
            if engines[..index].iter().any(|(held, _)| held == engine) {
                return Err(format!("the producer set names {engine} twice"));
            }
        }
        Ok(Producers { engines })
    }

    /// The producer whose engine produced `row`.
    fn of(&self, row: &LocalRow) -> Result<&'a dyn Producer, String> {
        let engine = row.identity.engine.as_str();
        self.engines
            .iter()
            .find(|(held, _)| *held == engine)
            .map(|(_, producer)| *producer)
            .ok_or_else(|| {
                format!(
                    "no {engine} producer on this host holds the bytes of {}",
                    if row.client_publish_id.is_empty() {
                        "a replica of the head".to_string()
                    } else {
                        format!("batch {}", row.client_publish_id)
                    }
                )
            })
    }
}

impl Producer for Producers<'_> {
    fn amend_open_declaration(&self, project: &str, declaration: &mut Value) -> Result<(), String> {
        let engine = declaration["engine"]
            .as_str()
            .ok_or("publication engine missing")?;
        self.engines
            .iter()
            .find(|(name, _)| *name == engine)
            .ok_or("publication producer missing")?
            .1
            .amend_open_declaration(project, declaration)
    }
    /// Every producer's inventory, in the order the set names them. A
    /// producer that offers a row of another engine is a host bug — its rows
    /// would be moved and freed by a producer that never held them — and is
    /// refused rather than adopted.
    fn inventory(&self, project: &str) -> Result<Vec<LocalRow>, String> {
        let mut rows = Vec::new();
        for (engine, producer) in &self.engines {
            for row in producer.inventory(project)? {
                if row.identity.engine != *engine {
                    return Err(format!(
                        "the {engine} producer offered batch {} of engine {}",
                        row.client_publish_id, row.identity.engine
                    ));
                }
                rows.push(row);
            }
        }
        Ok(rows)
    }

    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String> {
        self.of(row)?.transfer(row, output_id, session_uri)
    }

    /// Freed through the producer whose engine produced the row. A replica
    /// of the head is nobody's publication and frees 0; a row of an engine
    /// with no producer here is an error, so the receipt is `reclaim_failed`
    /// and the row keeps naming its bytes until a producer can free them.
    fn reclaim(&self, project: &str, row: &LocalRow) -> Result<u64, String> {
        if row.client_publish_id.is_empty() {
            return Ok(0);
        }
        self.of(row)?.reclaim(project, row)
    }

    fn activity(&self, project: &str) -> Result<Vec<ActivityRow>, String> {
        let mut rows = Vec::new();
        for (_, producer) in &self.engines {
            rows.extend(producer.activity(project)?);
        }
        Ok(rows)
    }
}

/// A producer for a host that holds no local byte source of its own yet.
///
/// Its inventory is empty, so [`StoreHost::local`] returns exactly the rows the
/// durable store already holds, including another engine's rows, and no more. A transfer is a typed refusal, never a different door:
/// a host with no producer cannot move bytes, and says so rather than pretend.
/// This is the honest producer for the Linux server until a native publication
/// source is wired to it.
pub struct StoreOnlyProducer;

impl Producer for StoreOnlyProducer {
    fn inventory(&self, _project: &str) -> Result<Vec<LocalRow>, String> {
        Ok(Vec::new())
    }

    fn transfer(
        &self,
        _row: &LocalRow,
        _output_id: &str,
        _session_uri: &str,
    ) -> Result<TransferReceipt, String> {
        Err("this host has no local artifact producer wired; it cannot upload bytes".to_string())
    }

    /// No bytes of its own on this host: nothing to free, and honestly 0.
    fn reclaim(&self, _project: &str, _row: &LocalRow) -> Result<u64, String> {
        Ok(0)
    }
}

/// Open (or create) the shared store at `path`. An incompatible store on disk
/// is refused by the store itself, never rewritten.
pub fn open_store(path: &Path) -> Result<SharedStore, String> {
    ds_sync_store::Store::open(path)
        .map(|store| Arc::new(Mutex::new(store)))
        .map_err(|error| error.to_string())
}

/// The projects this fence holds rows for: what a host restarts with.
pub fn projects_of_fence(store: &SharedStore, fence: &store::Fence) -> Result<Vec<String>, String> {
    let store = store
        .lock()
        .map_err(|_| "The sync gate is unavailable".to_string())?;
    let rows = store
        .artifacts_of_fence(fence)
        .map_err(|error| error.to_string())?;
    let mut projects: Vec<String> = rows
        .iter()
        .filter_map(|row| match &row.scope {
            store::Scope::Project { project } => Some(project.clone()),
            store::Scope::Personal => None,
        })
        .collect();
    projects.sort();
    projects.dedup();
    Ok(projects)
}
