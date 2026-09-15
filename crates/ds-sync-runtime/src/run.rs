//! The loop: observe, ask the kernel, perform the actions in order, re-plan.
//!
//! This is the IO half of `ds_command_kernel::sync`, extracted once from the
//! desktop shell so that desktop, the `ds` CLI and the Linux server run the
//! same code against the same seam. It holds no notion of freshness,
//! ownership, retries or ordering:
//!
//! * what a publication does once it is admitted is `work/publish`'s two-phase
//!   declaration, restated verbatim from the authority contract;
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

use std::collections::BTreeMap;

use ds_command_kernel::sync;
use serde_json::{json, Value};

use crate::rows::{
    kernel_grant, kernel_local, kernel_remote, HeldGrant, LocalRow, PublishOutcome, Receipt,
    RemoteRow, SyncRun, TransferReceipt, WorkOpen, MAX_ROWS, SCHEMA,
};

/// How many times one publication may restate the same declaration.
///
/// Not a retry budget: §1.3.1 prescribes exactly two rounds — declare, upload
/// what the server says is missing, restate the *same* declaration. The third
/// is a bound against a server that keeps answering `upload_required`, in the
/// spirit of the transport host's step ceiling. Beyond it the run records what
/// it saw and stops; it never invents a different door.
const MAX_PUBLISH_ROUNDS: u32 = 3;

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
    /// Read the record. Called **only** when the plan's `refresh_remote` says
    /// this trigger needs it.
    fn heads(&self, project: &str) -> Result<Vec<RemoteRow>, String>;
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
    /// The grants this install already holds for the project, from memory and
    /// from the host's durable store.
    fn grants(&self, project: &str) -> Result<Vec<HeldGrant>, String>;
    /// §1.2. The host persists what it obtained before returning it.
    fn open_work(&self, request: &WorkOpen) -> Result<HeldGrant, String>;
    /// §1.3. The same declaration is restated verbatim after the uploads.
    fn publish_work(&self, declaration: &Value) -> Result<PublishOutcome, String>;
    /// Resumable bytes for one declared object, any size.
    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String>;
    /// The gateway read of one head's outputs into the local store.
    fn fetch(&self, project: &str, head: &RemoteRow) -> Result<u64, String>;
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// Observe what the host holds and ask the kernel.
///
/// Everything the plan needs is an observation: the two inventories, the
/// credential, the clock, why the host is asking, and when the held heads were
/// last read. Nothing is derived here.
pub fn plan_once(
    host: &dyn SyncHost,
    project: &str,
    trigger: sync::Trigger,
    remote_read_at_ms: Option<u64>,
    local: &[LocalRow],
    remote: &[RemoteRow],
    grants: &[HeldGrant],
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
        // The kernel's grant check is coarse — project, install, expiry, engine
        // release. The per-resource claim check is the server's at publish, so
        // the freshest held grant is the honest thing to show it.
        grant: grants.last().map(kernel_grant),
        source_grants: grants.iter().map(kernel_grant).collect(),
        local: local.iter().map(kernel_local).collect(),
        remote: remote.iter().map(kernel_remote).collect(),
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
    let grants = host.grants(project)?;
    let mut plan = plan_once(host, project, trigger, read_at, &local, &remote, &grants)?;
    if plan.refresh_remote.needed {
        remote = host.heads(project)?;
        let at = host.server_now_ms();
        host.remember_heads(project, &remote, at)?;
        // Cached replicas are observations of this exact head generation.
        // Re-observe after refresh so an older cache cannot become an upload.
        local = host.local(project)?;
        read_at = Some(at);
        plan = plan_once(host, project, trigger, read_at, &local, &remote, &grants)?;
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
/// and then the actions run. Nothing here waits, sleeps or schedules.
pub fn run(host: &dyn SyncHost, project: &str, trigger: sync::Trigger) -> Result<SyncRun, String> {
    run_scoped(host, project, trigger, None)
}

/// Drive the report work-grant protocol without submitting Solar rows through
/// it. Both protocols retain the same project store and lease. Remote head
/// observations remain complete; only the input to this pass is scoped.
pub fn run_reports(
    host: &dyn SyncHost,
    project: &str,
    trigger: sync::Trigger,
) -> Result<SyncRun, String> {
    run_scoped(host, project, trigger, Some("network_reporter"))
}

#[allow(clippy::too_many_arguments)]
fn scoped_plan(
    host: &dyn SyncHost,
    project: &str,
    trigger: sync::Trigger,
    read_at: Option<u64>,
    local: &[LocalRow],
    remote: &[RemoteRow],
    grants: &[HeldGrant],
    engine: Option<&str>,
) -> Result<sync::Plan, String> {
    let local: Vec<_> = local
        .iter()
        .filter(|row| engine.is_none_or(|engine| row.identity.engine == engine))
        .cloned()
        .collect();
    let remote: Vec<_> = remote
        .iter()
        .filter(|row| engine.is_none_or(|engine| row.identity.engine == engine))
        .cloned()
        .collect();
    plan_once(host, project, trigger, read_at, &local, &remote, grants)
}

fn run_scoped(
    host: &dyn SyncHost,
    project: &str,
    trigger: sync::Trigger,
    engine: Option<&str>,
) -> Result<SyncRun, String> {
    if let Some(holder) = host.take_lease(project)? {
        // Another worker drains this project. The plan is still the honest
        // answer to "what would happen"; nothing is executed twice.
        let local = host.local(project)?;
        let (remote, read_at) = host.held_heads(project)?;
        let grants = host.grants(project)?;
        let plan = scoped_plan(
            host, project, trigger, read_at, &local, &remote, &grants, engine,
        )?;
        let replanned = scoped_plan(
            host, project, trigger, read_at, &local, &remote, &grants, engine,
        )?;
        return Ok(SyncRun {
            schema: SCHEMA,
            project: project.to_string(),
            trigger,
            plan,
            receipts: vec![Receipt::new("lease", "held").detail(holder)],
            replanned,
        });
    }
    struct Release<'a>(&'a dyn SyncHost, &'a str);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            self.0.release_lease(self.1);
        }
    }
    let _lease = Release(host, project);
    run_leased(host, project, trigger, engine)
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
    engine: Option<&str>,
) -> Result<SyncRun, String> {
    let mut local = host.local(project)?;
    let (mut remote, mut read_at) = host.held_heads(project)?;
    let mut grants = host.grants(project)?;
    let mut plan = scoped_plan(
        host, project, trigger, read_at, &local, &remote, &grants, engine,
    )?;

    // The plan decides whether this trigger needs the record; it is honoured
    // exactly once per run, because a second read in the same pass could only
    // repeat the first.
    if plan.refresh_remote.needed {
        remote = host.heads(project)?;
        let at = host.server_now_ms();
        host.remember_heads(project, &remote, at)?;
        // Cached replicas are observations of this exact head generation.
        // Re-observe after refresh so an older cache cannot become an upload.
        local = host.local(project)?;
        read_at = Some(at);
        plan = scoped_plan(
            host, project, trigger, read_at, &local, &remote, &grants, engine,
        )?;
    }

    let mut receipts: Vec<Receipt> = Vec::new();
    if plan
        .actions
        .iter()
        .any(|action| matches!(action, sync::Action::OpenGrant { .. }))
    {
        for receipt in open_grants(host, project, &plan, &local, &mut grants) {
            keep(host, project, &mut receipts, receipt);
        }
        // A newly issued grant is new authority evidence. Ask the kernel again
        // before moving bytes; it may now identify stale computed inputs.
        plan = scoped_plan(
            host,
            project,
            sync::Trigger::GrantOpened,
            read_at,
            &local,
            &remote,
            &grants,
            engine,
        )?;
    }
    for action in &plan.actions {
        match action {
            sync::Action::Offline => {
                receipts.push(Receipt::new("offline", "noted"));
            }
            sync::Action::OpenGrant { .. } => {}
            sync::Action::Upload { identity, .. } => {
                let receipt = if plan.grant_valid {
                    upload(host, &local, &grants, identity)
                } else {
                    Receipt::new("upload", "failed")
                        .about(identity)
                        .detail("no work grant covers this publication")
                };
                keep(host, project, &mut receipts, receipt);
            }
            sync::Action::Download { identity, .. } => {
                let receipt = download(host, project, &remote, identity);
                keep(host, project, &mut receipts, receipt);
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
                keep(host, project, &mut receipts, receipt);
            }
            sync::Action::Nothing { identity, reason } => {
                receipts.push(Receipt::new("nothing", *reason).about(identity));
            }
            sync::Action::Refused { identity, reason } => {
                let mut receipt = Receipt::new("refused", *reason);
                receipt.identity = identity.clone();
                keep(host, project, &mut receipts, receipt);
            }
        }
    }

    // Re-plan from what the run left behind. The held heads already carry
    // every head a publish advanced, so this costs no gateway call.
    local = host.local(project)?;
    let (remote, read_at) = host.held_heads(project)?;
    let grants = host.grants(project)?;
    let replanned = scoped_plan(
        host, project, trigger, read_at, &local, &remote, &grants, engine,
    )?;
    Ok(SyncRun {
        schema: SCHEMA,
        project: project.to_string(),
        trigger,
        plan,
        receipts,
        replanned,
    })
}

/// Ask §1.2 for a grant covering every upload this plan holds.
///
/// The kernel emits one `open_grant`; the edge lane scopes a grant to one
/// engine release and one resource, so this opens one per distinct scope the
/// pending uploads need. Which uploads are pending is the plan's answer, not
/// this function's.
fn open_grants(
    host: &dyn SyncHost,
    project: &str,
    plan: &sync::Plan,
    local: &[LocalRow],
    grants: &mut Vec<HeldGrant>,
) -> Vec<Receipt> {
    let mut receipts: Vec<Receipt> = Vec::new();
    let mut wanted: BTreeMap<String, &LocalRow> = BTreeMap::new();
    for action in &plan.actions {
        if let sync::Action::Upload { identity, .. } = action {
            if let Some(row) = local.iter().find(|row| &row.identity == identity) {
                wanted.insert(row.scope(), row);
            }
        }
    }
    for (_, row) in wanted {
        let request = WorkOpen {
            client_request_id: format!("open-{}", uuid::Uuid::new_v4()),
            install_id: host.install_id(),
            project: project.to_string(),
            engine: row.grant_engine.clone(),
            engine_release: row.engine_release.clone(),
            resource: row.resource.clone(),
        };
        match host.open_work(&request) {
            Ok(grant) => {
                let mut receipt = Receipt::new("open_grant", "granted").about(&row.identity);
                receipt.grant_id = Some(grant.grant_id.clone());
                grants.retain(|held| held.scope() != grant.scope());
                grants.push(grant);
                receipts.push(receipt);
            }
            Err(error) => receipts.push(
                Receipt::new("open_grant", "failed")
                    .about(&row.identity)
                    .detail(error),
            ),
        }
    }
    receipts
}

/// Publish one local publication, uploading exactly the objects §1.3 declares
/// missing, then restating the same declaration.
fn upload(
    host: &dyn SyncHost,
    local: &[LocalRow],
    grants: &[HeldGrant],
    identity: &sync::Identity,
) -> Receipt {
    let Some(row) = local.iter().find(|row| &row.identity == identity) else {
        return Receipt::new("upload", "failed")
            .about(identity)
            .detail("the planned row is no longer in the local inventory");
    };
    let scope = row.scope();
    let Some(grant) = grants.iter().rev().find(|grant| grant.scope() == scope) else {
        return Receipt::new("upload", "failed")
            .about(identity)
            .detail("no work grant covers this publication");
    };

    // The grant names the exact builds ds-brain admitted for this release; the
    // row names the build that actually produced its bytes. Checking that here
    // costs nothing and spends nothing: the server refuses the same mismatch,
    // but only after a multi-megabyte upload has crossed the wire.
    if !grant
        .admitted_builds
        .iter()
        .any(|admitted| admitted == &row.engine_build_manifest_sha256)
    {
        let mut receipt = Receipt::new("upload", "engine_build_not_admitted")
            .about(identity)
            .detail(format!(
                "this publication was produced by {} build {}, which this release does not register (admitted: {})",
                row.engine_release,
                if row.engine_build_manifest_sha256.is_empty() {
                    "«unrecorded»"
                } else {
                    &row.engine_build_manifest_sha256
                },
                if grant.admitted_builds.is_empty() {
                    "none".to_string()
                } else {
                    grant.admitted_builds.join(", ")
                },
            ));
        receipt.grant_id = Some(grant.grant_id.clone());
        return receipt;
    }

    let declaration = declaration(row, grant);
    let mut transfers: Vec<TransferReceipt> = Vec::new();
    let mut rounds = 0;
    loop {
        rounds += 1;
        let outcome = match host.publish_work(&declaration) {
            Ok(outcome) => outcome,
            Err(error) => {
                let mut receipt = Receipt::new("upload", "failed")
                    .about(identity)
                    .detail(error);
                receipt.grant_id = Some(grant.grant_id.clone());
                receipt.transfers = transfers;
                return receipt;
            }
        };
        if outcome.state == "upload_required" && rounds < MAX_PUBLISH_ROUNDS {
            for (output_id, session_uri) in &outcome.uploads {
                match host.transfer(row, output_id, session_uri) {
                    Ok(receipt) => transfers.push(receipt),
                    Err(error) => {
                        let mut receipt = Receipt::new("upload", "failed")
                            .about(identity)
                            .detail(error);
                        receipt.grant_id = Some(grant.grant_id.clone());
                        receipt.work_id = outcome.work_id.clone();
                        receipt.transfers = transfers;
                        return receipt;
                    }
                }
            }
            continue;
        }
        // A completed publish advances the held head from its own receipt, so
        // the next trigger never pays for a re-read to learn what this run did.
        let mut receipt = Receipt::new("upload", outcome.state.clone()).about(identity);
        if outcome.state == "published" {
            receipt.revision = Some(published_revision(outcome.head_revision));
        }
        receipt.grant_id = Some(grant.grant_id.clone());
        receipt.work_id = outcome.work_id;
        receipt.head_revision = outcome.head_revision;
        receipt.transfers = transfers;
        return receipt;
    }
}

/// The §1.3 declaration. Every attributed field is the grant's own: ds-brain
/// compares them field for field against what it signed.
///
/// `engine_build_manifest_sha256` is the one exception, and deliberately so:
/// it names the build that PRODUCED these bytes, which only the local receipt
/// knows. The grant is what makes it publishable, not what supplies it — so it
/// is checked against the grant's admitted set before this is ever assembled.
pub fn declaration(row: &LocalRow, grant: &HeldGrant) -> Value {
    let attribution = json!({
        "engine": grant.engine,
        "engine_release": grant.engine_release,
        "engine_build_manifest_sha256": row.engine_build_manifest_sha256,
        "grant_id": grant.grant_id,
        "snapshot_base": grant.snapshot_base,
    });
    json!({
        "client_publish_id": row.client_publish_id,
        "grant_id": grant.grant_id,
        "artifacts": row
            .outputs
            .iter()
            .map(|output| json!({
                "kind": output.output_id,
                "format": output.format,
                "content_type": output.content_type,
                "sha256": output.sha256,
                "size_bytes": output.size_bytes,
                "attribution": attribution,
            }))
            .collect::<Vec<_>>(),
        "base_revisions": grant.snapshot_base,
    })
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
        Err(error) => Receipt::new("download", "failed")
            .about(identity)
            .detail(error),
    }
}
