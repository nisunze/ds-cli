//! The loop: observe, ask the kernel, perform the actions in order, re-plan.
//!
//! This is the IO half of `ds_command_kernel::sync`, extracted once from the
//! desktop shell so that desktop, the `ds` CLI and the Linux server run the
//! same code against the same seam. It holds no notion of freshness,
//! ownership, retries or ordering:
//!
//! * what a publication does is the shared record's own door —
//!   `compute_artifacts` `open`, the bytes it asks for, `finalize` — and what
//!   it may lose on is the record's verdict, never a local guess;
//! * how bytes move — every resume point, retry, backoff and terminal verdict —
//!   is `ds_command_kernel::transfer`, driven through each host's one native
//!   transport host behind [`SyncHost::transfer`];
//! * whether a row is stale, conflicted or already in sync is `sync::plan`;
//! * **when** to look again, and whether the shared record must be re-read at
//!   all, is `sync::plan` too.
//!
//! If an `if` about any of those appears here, it is in the wrong file.
//!
//! **Nothing here polls.** The loop runs on a trigger and the plan answers
//! whether that trigger needs the remote heads re-read. The only timed wake a
//! host may honour is the one the plan names in `wake`.

use std::collections::BTreeSet;

use ds_command_kernel::{sync, sync_store::QueueBlock};
use serde_json::Value;
use sync::{publication_verdict, PublicationStage as Stage, PublicationVerdict};

use crate::gateway::GatewayError;
use crate::rows::{
    finalize_declaration, kernel_local, kernel_remote, open_declaration, LocalRow, Receipt,
    Reclaimed, RecordAnswer, RemoteRow, SyncRun, TransferReceipt, MAX_ROWS, SCHEMA,
};

/// The trigger a caller that names none is taken to mean.
///
/// "Sync now" is the only honest default: a call that arrived without a reason
/// is an explicit ask, not a navigation and not a background tick — there is no
/// background tick.
pub const DEFAULT_TRIGGER: sync::Trigger = sync::Trigger::Manual;

// ---------------------------------------------------------------------------
// The host seam
// ---------------------------------------------------------------------------

/// Everything the loop cannot do itself.
///
/// One trait, deliberately: a second seam would be a second place a host could
/// disagree with the plan. [`crate::host::StoreHost`] implements it over the
/// durable store for every native host; the fakes implement it in memory.
pub trait SyncHost {
    fn online(&self) -> bool;
    /// The host's monotonic high-water mark of server-observed time.
    fn server_now_ms(&self) -> u64;
    fn install_id(&self) -> String;
    fn local(&self, project: &str) -> Result<Vec<LocalRow>, String>;
    /// The heads this host already holds for the project, and when they were
    /// read from the record. Free: no call leaves the machine.
    fn held_heads(&self, project: &str) -> Result<(Vec<RemoteRow>, Option<u64>), String>;
    /// The store's output-only credential blocker for this exact project and
    /// fence. Native hosts override this with the kernel's queue verdict;
    /// a host without a durable queue has no held blocker to observe.
    fn credential_block(&self, project: &str) -> Result<Option<QueueBlock>, String> {
        let _ = project;
        Ok(None)
    }
    /// Read the record. Called **only** when the plan's `refresh_remote` says
    /// this trigger needs it.
    fn heads(&self, project: &str) -> Result<Vec<RemoteRow>, GatewayError>;
    /// Remember what was just read, with the server time it was read at.
    fn remember_heads(
        &self,
        project: &str,
        heads: &[RemoteRow],
        read_at_ms: u64,
    ) -> Result<(), String>;
    /// Every receipt of a run, as the run produces it. The store keeps it as
    /// evidence; a committed upload advances the held head from it, so the
    /// next trigger never re-reads what this run did.
    fn record_receipt(&self, project: &str, receipt: &Receipt) -> Result<(), String>;
    /// One worker drains a project at a time. `Ok(None)`: this worker now
    /// holds the lease. `Ok(Some(holder))`: another worker does, and nothing
    /// is executed. A host with no second worker answers `Ok(None)`.
    fn take_lease(&self, project: &str) -> Result<Option<String>, String> {
        let _ = project;
        Ok(None)
    }
    fn release_lease(&self, project: &str) {
        let _ = project;
    }
    /// Declare one publication on the shared record's door
    /// (`compute_artifacts` `open`). The answer is the record's own.
    fn open_publication(&self, declaration: &Value) -> Result<RecordAnswer, GatewayError>;
    /// Ask the record to publish the run `open` answered with (`finalize`).
    fn finalize_publication(&self, declaration: &Value) -> Result<RecordAnswer, GatewayError>;
    /// Resumable bytes for one declared object, any size.
    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String>;
    /// The gateway read of one head's outputs into the local store.
    fn fetch(&self, project: &str, head: &RemoteRow) -> Result<u64, GatewayError>;
    /// Free the bytes of a publication that lost. Owner, 2026-09-20: "when
    /// there's a conflict, online overwrites local artefacts"; an artefact
    /// never accumulates on the edge. Answers the bytes freed; a batch that
    /// is no longer on disk is `Ok(0)`, never an error. The store row and
    /// its receipts stay — the verdict is kept, the bytes are not.
    fn reclaim(&self, project: &str, row: &LocalRow) -> Result<u64, String>;
    /// Receipts the host produced on its own while observing its inventory —
    /// bytes it reclaimed for rows the store had already settled as lost —
    /// handed to the run once, then forgotten by the host. A host that does
    /// no housekeeping of its own answers none.
    fn take_receipts(&self, project: &str) -> Vec<Receipt> {
        let _ = project;
        Vec::new()
    }
    /// Identities whose newest local publication the store holds as refused
    /// or conflicted. The plan parks a result that depends on one of them
    /// (`sync::DEPENDENCY_REFUSED`) instead of letting it wait forever. A host
    /// without a durable queue holds no settled verdicts.
    fn settled(&self, project: &str) -> Result<Vec<sync::Identity>, String> {
        let _ = project;
        Ok(Vec::new())
    }
    /// The plan found this row `in_sync`: the held head carries its exact
    /// bytes. A host whose store still queues the row settles it as delivered
    /// and answers the retained receipt; `Ok(None)` when nothing was queued.
    fn settle_in_sync(&self, project: &str, row: &LocalRow) -> Result<Option<Receipt>, String> {
        let _ = (project, row);
        Ok(None)
    }
    /// This machine's own publications for the project as the store holds
    /// them now — what [`SyncHost::local`] answers without the head replicas
    /// and without reconciling the producer's disk. A pass reads it between
    /// two download steps, so it must stay cheap: no byte is re-verified and
    /// nothing leaves the machine.
    fn sealed(&self, project: &str) -> Result<Vec<LocalRow>, String> {
        Ok(self
            .local(project)?
            .into_iter()
            .filter(|row| !row.client_publish_id.is_empty())
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// Observe what the host holds and ask the kernel.
///
/// Everything the plan needs is an observation: the two inventories, the
/// clock, why the host is asking, and when the held heads were last read.
/// Nothing is derived here. A publication needs no credential beyond the
/// session the host's door already carries, so no grant is shown to the plan
/// and any `open_grant` it answers is not performed.
pub fn plan_once(
    host: &dyn SyncHost,
    project: &str,
    trigger: sync::Trigger,
    remote_read_at_ms: Option<u64>,
    local: &[LocalRow],
    remote: &[RemoteRow],
) -> Result<sync::Plan, String> {
    plan_once_with_interest(
        host,
        project,
        trigger,
        remote_read_at_ms,
        local,
        remote,
        None,
    )
}

fn plan_once_with_interest(
    host: &dyn SyncHost,
    project: &str,
    trigger: sync::Trigger,
    remote_read_at_ms: Option<u64>,
    local: &[LocalRow],
    remote: &[RemoteRow],
    download_interest: Option<&[sync::Identity]>,
) -> Result<sync::Plan, String> {
    if local.len() > MAX_ROWS || remote.len() > MAX_ROWS {
        return Err("this project's sync inventories exceed their bound".to_string());
    }
    let input = sync::Input {
        schema: sync::SCHEMA.to_string(),
        project: project.to_string(),
        // Project plans are never personal; the personal scope is its own loop.
        personal: false,
        install_id: host.install_id(),
        now_ms: host.server_now_ms(),
        online: host.online(),
        trigger,
        remote_read_at_ms,
        grant: None,
        grants: Vec::new(),
        source_grants: Vec::new(),
        local: local.iter().map(kernel_local).collect(),
        remote: remote.iter().map(kernel_remote).collect(),
        download_interest: download_interest.map(<[sync::Identity]>::to_vec),
        settled: host.settled(project)?,
        published_outputs: remote
            .iter()
            .flat_map(|head| {
                head.outputs.iter().map(|output| sync::PublishedOutput {
                    identity: head.identity.clone(),
                    output_id: output.output_id.clone(),
                    sha256: output.sha256.clone(),
                })
            })
            .collect(),
    };
    sync::plan(&input)
}

/// What one project's sync gate decides for this trigger. Reads only.
///
/// It reads the record only when the plan it would otherwise return says this
/// trigger needs it, so a `local_change` costs nothing remote.
pub fn plan(
    host: &dyn SyncHost,
    project: &str,
    trigger: sync::Trigger,
) -> Result<sync::Plan, String> {
    let mut local = host.local(project)?;
    let (mut remote, mut read_at) = host.held_heads(project)?;
    let mut plan = plan_once(host, project, trigger, read_at, &local, &remote)?;
    if plan.refresh_remote.needed {
        remote = host.heads(project)?;
        let at = host.server_now_ms();
        host.remember_heads(project, &remote, at)?;
        // Cached replicas are observations of this exact head generation.
        // Re-observe after refresh so an older cache cannot become an upload.
        local = host.local(project)?;
        read_at = Some(at);
        plan = plan_once(host, project, trigger, read_at, &local, &remote)?;
    }
    Ok(plan)
}

// ---------------------------------------------------------------------------
// Performing the plan
// ---------------------------------------------------------------------------

/// Plan, perform the actions in order, re-plan — once, for one trigger.
///
/// The shape is the ruling's: a trigger arrives, the kernel says whether the
/// record must be re-read for that trigger, the record is read at most once,
/// and then the actions run. Nothing here waits, sleeps or schedules. Before
/// each download step (one head) the pass publishes whatever was sealed since
/// it planned, so a long mirror never holds a local result behind it.
pub fn run(host: &dyn SyncHost, project: &str, trigger: sync::Trigger) -> Result<SyncRun, String> {
    run_with_download_interest(host, project, trigger, None)
}

/// The existing project pass with exact download demand. Heads and uploads stay
/// complete; only the portable planner decides which downloads are requested.
pub fn run_with_download_interest(
    host: &dyn SyncHost,
    project: &str,
    trigger: sync::Trigger,
    download_interest: Option<&[sync::Identity]>,
) -> Result<SyncRun, String> {
    run_scoped(host, project, trigger, download_interest)
}

fn run_scoped(
    host: &dyn SyncHost,
    project: &str,
    trigger: sync::Trigger,
    download_interest: Option<&[sync::Identity]>,
) -> Result<SyncRun, String> {
    if trigger == sync::Trigger::LocalChange {
        if let Some(block) = host.credential_block(project)? {
            // This trigger may be the Desktop's recovery wake. The same
            // refused credential cannot publish any row, so keep the queue
            // intact without taking a lease or creating another durable
            // refusal receipt. An explicit trigger still tries the door.
            let local = host.local(project)?;
            let mut receipts = host.take_receipts(project);
            receipts.push(Receipt::new("session", "blocked").detail(format!(
                "{}; this local-change pass made no gateway call",
                block.detail
            )));
            let (remote, read_at) = host.held_heads(project)?;
            let plan = plan_once_with_interest(
                host,
                project,
                trigger,
                read_at,
                &local,
                &remote,
                download_interest,
            )?;
            let replanned = plan_once_with_interest(
                host,
                project,
                trigger,
                read_at,
                &local,
                &remote,
                download_interest,
            )?;
            let reclaimed = Reclaimed::from_receipts(&receipts);
            return Ok(SyncRun {
                schema: SCHEMA,
                project: project.to_string(),
                trigger,
                plan,
                receipts,
                replanned,
                reclaimed,
            });
        }
    }
    if let Some(holder) = host.take_lease(project)? {
        // Another worker drains this project. The plan is still the honest
        // answer to "what would happen"; nothing is executed twice.
        let local = host.local(project)?;
        let mut receipts = host.take_receipts(project);
        receipts.push(Receipt::new("lease", "held").detail(holder));
        let (remote, read_at) = host.held_heads(project)?;
        let plan = plan_once_with_interest(
            host,
            project,
            trigger,
            read_at,
            &local,
            &remote,
            download_interest,
        )?;
        let replanned = plan_once_with_interest(
            host,
            project,
            trigger,
            read_at,
            &local,
            &remote,
            download_interest,
        )?;
        let reclaimed = Reclaimed::from_receipts(&receipts);
        return Ok(SyncRun {
            schema: SCHEMA,
            project: project.to_string(),
            trigger,
            plan,
            receipts,
            replanned,
            reclaimed,
        });
    }
    struct Release<'a>(&'a dyn SyncHost, &'a str);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            self.0.release_lease(self.1);
        }
    }
    let _lease = Release(host, project);
    run_leased(host, project, trigger, download_interest)
}

/// Keep one receipt: in the store first, then in the run's answer. A store
/// that cannot keep it says so in the same answer — the run never reports
/// evidence it did not retain.
fn keep(host: &dyn SyncHost, project: &str, receipts: &mut Vec<Receipt>, receipt: Receipt) {
    if let Err(error) = host.record_receipt(project, &receipt) {
        receipts.push(receipt);
        receipts.push(Receipt::new("store", "failed").detail(error));
        return;
    }
    receipts.push(receipt);
}

fn run_leased(
    host: &dyn SyncHost,
    project: &str,
    trigger: sync::Trigger,
    download_interest: Option<&[sync::Identity]>,
) -> Result<SyncRun, String> {
    let mut local = host.local(project)?;
    // What the host freed while observing — rows the store had already
    // settled as lost — is this pass's evidence too, never a silent side
    // effect of reading the inventory.
    let mut receipts: Vec<Receipt> = host.take_receipts(project);
    let (mut remote, mut read_at) = host.held_heads(project)?;
    let mut plan = plan_once_with_interest(
        host,
        project,
        trigger,
        read_at,
        &local,
        &remote,
        download_interest,
    )?;

    // The plan decides whether this trigger needs the record; it is honoured
    // exactly once per run, because a second read in the same pass could only
    // repeat the first.
    if plan.refresh_remote.needed {
        let had_credential_block = host.credential_block(project)?.is_some();
        remote = match host.heads(project) {
            Ok(heads) => heads,
            Err(error) => {
                if error.credential_refused() {
                    // A failed heads read must remain visible even though this
                    // pass cannot return a SyncRun. No row was acknowledged.
                    host.record_receipt(
                        project,
                        &Receipt::new("session", "credential_refused")
                            .detail(credential_refusal_detail())
                            .failure_code(error.code.clone()),
                    )?;
                }
                return Err(error.into());
            }
        };
        let at = host.server_now_ms();
        host.remember_heads(project, &remote, at)?;
        if had_credential_block {
            keep(
                host,
                project,
                &mut receipts,
                Receipt::new("session", "connected").detail(
                    "the gateway accepted this machine's credential while reading project heads; queued work may resume",
                ),
            );
        }
        // Cached replicas are observations of this exact head generation.
        // Re-observe after refresh so an older cache cannot become an upload.
        local = host.local(project)?;
        receipts.extend(host.take_receipts(project));
        read_at = Some(at);
        plan = plan_once_with_interest(
            host,
            project,
            trigger,
            read_at,
            &local,
            &remote,
            download_interest,
        )?;
    }

    // Every publication this pass has asked the record about, and every
    // identity it published: a row is attempted at most once per pass, and a
    // head this pass superseded with its own bytes is not fetched.
    let mut delivery = Delivery::default();
    for action in &plan.actions {
        match action {
            sync::Action::Offline => {
                receipts.push(Receipt::new("offline", "noted"));
            }
            // The record's door asks the session's own authority; there is
            // no second credential to obtain first.
            sync::Action::OpenGrant { .. } => {}
            sync::Action::Upload { identity, .. } => {
                // A publication verdict affects only its row. A refused
                // session affects every remaining gateway call in this pass.
                if delivery.deliver(host, project, &local, identity, &mut receipts) {
                    break;
                }
            }
            sync::Action::Download { identity, .. } => {
                // One download step is one head. Whatever was sealed while
                // the steps before it ran publishes first.
                if delivery.sealed_since_plan(
                    host,
                    project,
                    trigger,
                    download_interest,
                    &mut receipts,
                )? {
                    break;
                }
                if delivery.published.contains(identity) {
                    receipts.push(Receipt::new("nothing", "in_sync").about(identity).detail(
                        "this pass published this identity after it planned; the head it replaced is not fetched",
                    ));
                    continue;
                }
                let receipt = download(host, project, &remote, identity);
                let credential_refused = receipt.outcome == "credential_refused";
                keep(host, project, &mut receipts, receipt);
                if credential_refused {
                    break;
                }
            }
            sync::Action::Conflict {
                identity,
                base_revision,
                current_revision,
                reason,
            } => {
                let receipt = Receipt::new("conflict", *reason)
                    .about(identity)
                    .detail(format!(
                        "computed on {}, the record is at {current_revision}",
                        base_revision.as_deref().unwrap_or("no base"),
                    ));
                keep(
                    host,
                    project,
                    &mut receipts,
                    bind_publication(receipt, &local),
                );
                // The verdict is kept; the bytes are not. Online overwrites
                // local, and a lost batch that stayed on disk was counted as
                // queued for seven days on a machine where nothing was.
                let receipt = reclaim(host, project, &local, identity, reason);
                keep(host, project, &mut receipts, receipt);
            }
            sync::Action::Nothing { identity, reason } => {
                if *reason == sync::SUPERSEDED_BY_HEAD {
                    // Not a no-op: this machine's verified result lost to a
                    // later computation, and the row must leave the pending
                    // count or the outbox would show work that will never
                    // move. The receipt is persisted so the operator can see
                    // WHICH batch was superseded and why, instead of watching
                    // a number quietly go down.
                    let receipt = Receipt::new("conflict", *reason).about(identity).detail(
                        "the shared record already carries a later computation of this room"
                            .to_string(),
                    );
                    keep(
                        host,
                        project,
                        &mut receipts,
                        bind_publication(receipt, &local),
                    );
                    let receipt = reclaim(host, project, &local, identity, reason);
                    keep(host, project, &mut receipts, receipt);
                } else if *reason == "in_sync" {
                    // Already delivered: a row still queued for these exact
                    // bytes leaves the queue as published, with a receipt.
                    let settled = match local
                        .iter()
                        .find(|row| &row.identity == identity && !row.client_publish_id.is_empty())
                    {
                        Some(row) => host.settle_in_sync(project, row)?,
                        None => None,
                    };
                    receipts.push(settled.unwrap_or_else(|| {
                        Receipt::new("nothing", *reason).about(identity)
                    }));
                } else {
                    receipts.push(Receipt::new("nothing", *reason).about(identity));
                }
            }
            sync::Action::Refused { identity, reason } => {
                let mut receipt = Receipt::new("refused", *reason);
                receipt.identity = identity.clone();
                if *reason == sync::DEPENDENCY_REFUSED {
                    // The typed reason is the row's state reason: it parks,
                    // and the Sync Center says why.
                    receipt = receipt.detail(format!(
                        "{}: the result this publication depends on was refused or conflicted; recompute it to publish this one",
                        sync::DEPENDENCY_REFUSED
                    ));
                }
                keep(
                    host,
                    project,
                    &mut receipts,
                    bind_publication(receipt, &local),
                );
            }
        }
    }

    // Re-plan from what the run left behind. The held heads already carry
    // every head a publish advanced, so this costs no gateway call.
    local = host.local(project)?;
    receipts.extend(host.take_receipts(project));
    let (remote, read_at) = host.held_heads(project)?;
    let replanned = plan_once_with_interest(
        host,
        project,
        trigger,
        read_at,
        &local,
        &remote,
        download_interest,
    )?;
    let reclaimed = Reclaimed::from_receipts(&receipts);
    Ok(SyncRun {
        schema: SCHEMA,
        project: project.to_string(),
        trigger,
        plan,
        receipts,
        replanned,
        reclaimed,
    })
}

/// What one pass has delivered so far.
#[derive(Default)]
struct Delivery {
    /// `(identity, client_publish_id)` of every row this pass asked the
    /// record to publish, whatever it answered. Never asked twice in a pass:
    /// a held row waits for the next trigger, as it always has.
    attempted: BTreeSet<(sync::Identity, String)>,
    /// Identities whose head this pass replaced with its own bytes.
    published: BTreeSet<sync::Identity>,
}

impl Delivery {
    /// Perform one planned upload over `rows` and keep its receipts.
    /// Answers `true` when the session was refused, which ends the pass.
    fn deliver(
        &mut self,
        host: &dyn SyncHost,
        project: &str,
        rows: &[LocalRow],
        identity: &sync::Identity,
        receipts: &mut Vec<Receipt>,
    ) -> bool {
        let row = rows.iter().find(|row| &row.identity == identity);
        self.attempted.insert((
            identity.clone(),
            row.map(|row| row.client_publish_id.clone())
                .unwrap_or_default(),
        ));
        let receipt = bind_publication(upload(host, project, rows, identity), rows);
        if receipt.outcome == "credential_refused" {
            keep(host, project, receipts, receipt);
            return true;
        }
        if receipt.outcome == "published" {
            self.published.insert(identity.clone());
        }
        let settled = settled_verdict(&receipt);
        let retain_for_recovery = row.is_some_and(|row| {
            crate::store::recovery::retain_failure_bytes(
                &crate::rows::store_row(project, row, host.server_now_ms()),
                &crate::rows::receipt_row(project, &receipt, host.server_now_ms()),
            )
        });
        keep(host, project, receipts, receipt);
        // Terminal verdicts leave the queue immediately. Only exact
        // recoverable authority evidence retains bytes for explicit recovery.
        if let Some(verdict) = settled.filter(|_| !retain_for_recovery) {
            let receipt = reclaim(host, project, rows, identity, verdict);
            keep(host, project, receipts, receipt);
        }
        false
    }

    /// Publish what was sealed since this pass planned, before its next
    /// download step.
    ///
    /// A fresh host mirroring a whole project spends its pass fetching heads
    /// (ADERM, 10-23: 106 heads, ~950 MB), while its Solar runs keep sealing
    /// results that only this machine holds; those waited for the last head.
    /// Between two steps the pass re-plans from the store's own rows and the
    /// held heads — nothing remote is read, no replica is re-verified — and
    /// performs only the uploads it has not attempted yet. The plan still
    /// decides each one; this pass's lease is what keeps a second worker from
    /// publishing the same row. Answers `true` when the session was refused.
    fn sealed_since_plan(
        &mut self,
        host: &dyn SyncHost,
        project: &str,
        trigger: sync::Trigger,
        download_interest: Option<&[sync::Identity]>,
        receipts: &mut Vec<Receipt>,
    ) -> Result<bool, String> {
        let rows = host.sealed(project)?;
        let (remote, read_at) = host.held_heads(project)?;
        let plan = plan_once_with_interest(
            host,
            project,
            trigger,
            read_at,
            &rows,
            &remote,
            download_interest,
        )?;
        for action in &plan.actions {
            let sync::Action::Upload { identity, .. } = action else {
                continue;
            };
            let Some(row) = rows.iter().find(|row| &row.identity == identity) else {
                continue;
            };
            if self
                .attempted
                .contains(&(identity.clone(), row.client_publish_id.clone()))
            {
                continue;
            }
            if self.deliver(host, project, &rows, identity, receipts) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Bind only the publication selected in this pass, never a fresh lookup
/// of the operation slot after the gateway answered.
fn bind_publication(receipt: Receipt, local: &[LocalRow]) -> Receipt {
    match receipt
        .identity
        .as_ref()
        .and_then(|identity| local.iter().find(|row| &row.identity == identity))
    {
        Some(row) => receipt.for_publication(row),
        None => receipt,
    }
}

/// Free the bytes of the local row a verdict was passed on, and say so.
///
/// Never silent: a reclaim that freed nothing because the batch was already
/// gone still answers `reclaimed` with 0 bytes, and one the producer could
/// not perform answers `reclaim_failed` with its reason, so the next pass
/// retries it in the open. The outcome names the verdict the row lost to.
fn reclaim(
    host: &dyn SyncHost,
    project: &str,
    local: &[LocalRow],
    identity: &sync::Identity,
    verdict: &'static str,
) -> Receipt {
    let Some(row) = local.iter().find(|row| &row.identity == identity) else {
        return Receipt::new("reclaim_failed", verdict)
            .about(identity)
            .detail("the row this verdict was passed on is no longer in the local inventory");
    };
    let receipt = match host.reclaim(project, row) {
        Ok(bytes) => {
            let mut receipt =
                Receipt::new("reclaimed", verdict)
                    .about(identity)
                    .detail(if bytes == 0 {
                        format!("{} held no bytes on this machine any more", batch_name(row))
                    } else {
                        format!("{} freed: online overwrites local", batch_name(row))
                    });
            receipt.bytes = Some(bytes);
            receipt
        }
        Err(error) => Receipt::new("reclaim_failed", verdict)
            .about(identity)
            .detail(format!("{} was not freed: {error}", batch_name(row))),
    };
    receipt.for_publication(row)
}

/// How a receipt names the batch it is about: by its publish id, which is
/// what the batch directory's receipt carries and the store's replay key is.
fn batch_name(row: &LocalRow) -> String {
    if row.client_publish_id.is_empty() {
        "a replica of the head".to_string()
    } else {
        format!("batch {}", row.client_publish_id)
    }
}

/// Publish one local report on the shared record's door: declare it, move
/// exactly the bytes the record says it lacks, then ask it to publish.
///
/// The record decides everything a publication can lose on. `open` refuses a
/// declaration whose source inputs moved (409), that the project's current
/// inputs do not produce (400), or that the account may not publish (403);
/// a 401 is a refused session and holds this and every remaining row;
/// `finalize` re-checks the source inside the transaction that advances the
/// head and answers `stored_stale` when it lost. A verdict settles the row —
/// `conflict` or `refused`, and the bytes leave this machine on the next
/// observation — while anything that never reached a verdict (offline, a 5xx,
/// a transfer that did not complete) keeps it held for the next trigger, with
/// the cause on the row. `finalize` is asked only once every byte it needs
/// has committed.
fn upload(
    host: &dyn SyncHost,
    project: &str,
    local: &[LocalRow],
    identity: &sync::Identity,
) -> Receipt {
    let Some(row) = local.iter().find(|row| &row.identity == identity) else {
        return Receipt::new("upload", "failed")
            .about(identity)
            .detail("the planned row is no longer in the local inventory");
    };
    if let Err(block) =
        sync::publication_preflight(&row.identity, row.input_base_fingerprint.as_deref())
    {
        return Receipt::new("upload", block.verdict().outcome())
            .about(identity)
            .detail(block.detail());
    }
    let mut answer = match host.open_publication(&open_declaration(project, row)) {
        Ok(answer) => answer,
        Err(error) => return record_refusal(identity, Stage::Open, error, None, Vec::new()),
    };
    let mut transfers: Vec<TransferReceipt> = Vec::new();
    if answer.state == "pending" {
        for (output_id, session_uri) in &answer.uploads {
            let transferred = match host.transfer(row, output_id, session_uri) {
                Ok(receipt) => receipt,
                Err(error) => {
                    let mut receipt = Receipt::new("upload", "failed")
                        .about(identity)
                        .detail(error);
                    receipt.work_id = Some(answer.work_id.clone());
                    receipt.transfers = transfers;
                    return receipt;
                }
            };
            if !transferred.committed() {
                let mut receipt = Receipt::new("upload", "failed")
                    .about(identity)
                    .detail(format!(
                        "the bytes of {output_id} did not finish moving ({}, {} of {} bytes); the record was not asked to publish",
                        transferred.outcome, transferred.committed_bytes, transferred.total_bytes
                    ));
                receipt.work_id = Some(answer.work_id.clone());
                transfers.push(transferred);
                receipt.transfers = transfers;
                return receipt;
            }
            transfers.push(transferred);
        }
        let work_id = answer.work_id.clone();
        answer = match host.finalize_publication(&finalize_declaration(project, row, &work_id)) {
            Ok(answer) => answer,
            Err(error) => {
                return record_refusal(identity, Stage::Finalize, error, Some(work_id), transfers);
            }
        };
    }
    let mut receipt = match publication_verdict(Stage::Finalize, None, Some(&answer.state)) {
        PublicationVerdict::Published => {
            // A completed publish advances the held head from its own
            // receipt, so the next trigger never re-reads what this run did.
            let mut receipt = Receipt::new("upload", "published").about(identity);
            receipt.revision = Some(published_revision(answer.head_revision));
            receipt.head_revision = answer.head_revision;
            receipt
        }
        PublicationVerdict::StoredStale => Receipt::new("upload", "stored_stale")
            .about(identity)
            .detail(answer.conflict.clone().unwrap_or_else(|| {
                "the record moved past the inputs this report was computed from".to_string()
            })),
        verdict => Receipt::new("upload", verdict.outcome())
            .about(identity)
            .detail(format!("the record left this publication {}", answer.state)),
    };
    receipt.work_id = Some(answer.work_id);
    receipt.transfers = transfers;
    receipt
}

/// The verdict an upload receipt settled its row with, when it settled it.
fn settled_verdict(receipt: &Receipt) -> Option<&'static str> {
    publication_verdict(Stage::Finalize, None, Some(&receipt.outcome)).settled()
}

/// The receipt for a call the record answered with no `data`.
///
/// A verdict about THIS publication settles it: 409 (its source moved, or it
/// conflicts with what the record holds) is a conflict; 403 (the account may
/// not publish reports here, or the project is closed) and a 400 at `open`
/// (the project's inputs do not produce this declaration) are refusals. A 401
/// refuses the session, holds the row and ends this pass. Every
/// other answer is about the moment, not the publication — a 5xx, no network, or a 400
/// at `finalize` for bytes the record has not seen yet — and holds the row.
fn record_refusal(
    identity: &sync::Identity,
    stage: Stage,
    error: GatewayError,
    work_id: Option<String>,
    transfers: Vec<TransferReceipt>,
) -> Receipt {
    let outcome = publication_verdict(stage, error.status, None).outcome();
    let detail = if error.credential_refused() {
        credential_refusal_detail().to_string()
    } else {
        error.detail
    };
    let mut receipt = Receipt::new("upload", outcome)
        .about(identity)
        .detail(detail)
        .failure_code(error.code);
    receipt.work_id = work_id;
    receipt.transfers = transfers;
    receipt
}

fn credential_refusal_detail() -> &'static str {
    "the gateway refused this machine's credential (401): sign in again; nothing was lost"
}

/// The output head identity that stands after a publish. The source-input
/// fingerprint remains a separate field on the artifact and head rows.
fn published_revision(head_revision: Option<i64>) -> String {
    format!("head-{}", head_revision.unwrap_or_default())
}

/// Read one head's outputs into the local store. Downloads need no grant.
fn download(
    host: &dyn SyncHost,
    project: &str,
    remote: &[RemoteRow],
    identity: &sync::Identity,
) -> Receipt {
    let Some(head) = remote.iter().find(|row| &row.identity == identity) else {
        return Receipt::new("download", "failed")
            .about(identity)
            .detail("the planned head is no longer in the shared record");
    };
    match host.fetch(project, head) {
        Ok(bytes) => {
            let mut receipt = Receipt::new("download", "downloaded").about(identity);
            receipt.head_revision = Some(head.head_revision);
            receipt.bytes = Some(bytes);
            receipt
        }
        Err(error) if error.credential_refused() => Receipt::new("download", "credential_refused")
            .about(identity)
            .detail(credential_refusal_detail())
            .failure_code(error.code),
        Err(error) => Receipt::new("download", "failed")
            .about(identity)
            .detail(error.detail)
            .failure_code(error.code),
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    use crate::{rows, store};

    #[test]
    fn authority_failure_codes_survive_native_restart_without_changing_the_verdict() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("sync.sqlite3");
        let fence = store::Fence {
            account: "account".into(),
            deployment: "gateway".into(),
            install_id: "install".into(),
        };
        let mut native = ds_sync_store::Store::open(&database).unwrap();
        let scope = rows::store_scope("project");
        let mut expected = Vec::new();
        for engine in sync::engines::ENGINES {
            for (index, (stage, status, code)) in [
                (Stage::Open, 403, Some("ENGINE_BUILD_NOT_ADMITTED")),
                (Stage::Finalize, 409, Some("CONFLICT")),
                (Stage::Open, 400, Some("INVALID_INPUT")),
                (Stage::Open, 401, Some("UNAUTHENTICATED")),
                (Stage::Open, 503, Some("SERVICE_UNAVAILABLE")),
                (Stage::Open, 403, None),
            ]
            .into_iter()
            .enumerate()
            {
                let mut row = crate::fakes::local_row(b"immutable derived bytes");
                row.identity.engine = engine.name.into();
                row.identity.operation = format!("operation-{index}");
                row.client_publish_id = format!("{}-{index}", engine.name);
                native
                    .apply(
                        &fence,
                        1,
                        store::Event::LocalProduced {
                            row: rows::store_row("project", &row, 1),
                        },
                    )
                    .unwrap();
                // The same display text cannot supply or override a typed code.
                let receipt = record_refusal(
                    &row.identity,
                    stage,
                    GatewayError::refused(status, code.map(str::to_owned), "display text"),
                    None,
                    Vec::new(),
                )
                .for_publication(&row);
                let serialized = serde_json::to_value(&receipt).unwrap();
                assert_eq!(
                    serialized.get("failureCode"),
                    code.map(|value| serde_json::json!(value)).as_ref()
                );
                let durable = rows::receipt_row("project", &receipt, 2);
                assert_eq!(durable.failure_code.as_deref(), code);
                expected.push((
                    durable.receipt_id.clone(),
                    durable.failure_code.clone(),
                    durable.outcome.clone(),
                ));
                native
                    .apply(&fence, 2, store::Event::ActionReceipt { receipt: durable })
                    .unwrap();
            }
        }
        drop(native);
        let reopened = ds_sync_store::Store::open(&database).unwrap();
        let recorded = reopened.receipts(&fence, &scope, 100).unwrap();
        assert_eq!(recorded.len(), expected.len());
        for (id, code, outcome) in expected {
            let actual = recorded.iter().find(|row| row.receipt_id == id).unwrap();
            assert_eq!(actual.failure_code, code);
            assert_eq!(actual.outcome, outcome);
            let mut old = serde_json::to_value(actual).unwrap();
            old.as_object_mut().unwrap().remove("failure_code");
            let legacy: store::ReceiptRow = serde_json::from_value(old).unwrap();
            assert_eq!(legacy.failure_code, None);
        }
    }

    #[test]
    fn a_row_without_a_fingerprint_never_reaches_the_record_for_any_fenced_engine() {
        for engine in sync::engines::ENGINES {
            let mut row = crate::fakes::local_row(b"derived bytes");
            row.identity.engine = engine.name.into();
            row.input_base_fingerprint = None;
            assert_eq!(
                sync::publication_preflight(&row.identity, None),
                Err(sync::PublicationBlock::MissingSource)
            );
        }
        let mut unknown = crate::fakes::local_row(b"owned by another producer");
        unknown.identity.engine = "unregistered".into();
        let block = sync::publication_preflight(&unknown.identity, None).unwrap_err();
        assert_eq!(block.verdict(), PublicationVerdict::Held);
    }

    #[test]
    fn engine_names_live_only_in_the_engine_table() {
        for (path, source) in [
            ("run.rs", include_str!("run.rs")),
            ("host.rs", include_str!("host.rs")),
            ("sync.rs", include_str!("../../ds-command-kernel/src/sync.rs")),
        ] {
            let production = source.split("#[cfg(test)]").next().unwrap();
            for engine in sync::engines::ENGINES {
                assert!(
                    !production.contains(&format!("\"{}\"", engine.name)),
                    "{path}"
                );
            }
            assert!(!production.contains("REPORT_ENGINE"), "{path}");
        }
    }

    #[test]
    fn same_record_answer_same_verdict_for_every_engine() {
        // Run the native receipt mapping and the real store transition, not
        // just the enum: a credential refusal must leave both rows held.
        for (stage, status, expected, state) in [
            (
                Stage::Open,
                401,
                "credential_refused",
                store::ArtifactState::Held,
            ),
            (
                Stage::Finalize,
                401,
                "credential_refused",
                store::ArtifactState::Held,
            ),
            (Stage::Open, 403, "refused", store::ArtifactState::Refused),
            (Stage::Open, 409, "conflict", store::ArtifactState::Conflict),
            (
                Stage::Finalize,
                409,
                "stored_stale",
                store::ArtifactState::Conflict,
            ),
            (Stage::Open, 400, "refused", store::ArtifactState::Refused),
            (Stage::Finalize, 400, "failed", store::ArtifactState::Held),
            (Stage::Open, 503, "failed", store::ArtifactState::Held),
        ] {
            for engine in ["network_reporter", "solar"] {
                let mut row = crate::fakes::local_row(b"derived bytes");
                row.identity.engine = engine.into();
                let receipt = record_refusal(
                    &row.identity,
                    stage,
                    GatewayError::refused(status, None, "record answer"),
                    None,
                    Vec::new(),
                )
                .for_publication(&row);
                assert_eq!(receipt.outcome, expected);
                assert_eq!(
                    settled_verdict(&receipt),
                    publication_verdict(stage, Some(status), None).settled()
                );
                let fence = store::Fence {
                    account: "account".into(),
                    deployment: "gateway".into(),
                    install_id: "install".into(),
                };
                let transition = store::apply(&store::Request {
                    schema: store::SCHEMA.into(),
                    fence,
                    now_ms: 2,
                    snapshot: store::Snapshot {
                        artifacts: vec![rows::store_row("project", &row, 1)],
                        ..Default::default()
                    },
                    event: store::Event::ActionReceipt {
                        receipt: rows::receipt_row("project", &receipt, 2),
                    },
                })
                .unwrap();
                let settled = transition
                    .changes
                    .iter()
                    .find_map(|change| match change {
                        store::RowChange::UpsertArtifact(row) => Some(row),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(settled.state, state, "{engine} {stage:?} {status}");
            }
        }
    }
}
