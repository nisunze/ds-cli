//! Deterministic fakes for driving the shared runtime without a network.
//!
//! One [`FakeWorld`] stands for the record, the grant issuer, the storage
//! sessions and the bytes on disk; the fake gateway, producer and reads all
//! observe and mutate it, so a test asserts exactly-once effects (commits,
//! head reads, transfers) from one place. Nothing here decides anything the
//! real seams would not: the fake answers with the same envelopes ds-brain
//! does, and the runtime parses them through the same code.
//!
//! Feature-gated (`fakes`): reached only through dev-dependencies.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use ds_command_kernel::sync;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::gateway::{Gateway, GatewayError};
use crate::host::{Producer, Reads};
use crate::reads::place_verified_cause;
use crate::routes::SyncRoute;
use crate::rows::{inventory_digest, LocalOutput, LocalRow, TransferReceipt};

/// A fixed engine release and build so scenarios share one vocabulary.
pub const RELEASE: &str = "ds-network-reporter@0.1.0+0123456789abcdef0123456789abcdef01234567";
pub const BUILD_MANIFEST: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
pub const FINGERPRINT: &str = "fp-7";

/// How the fake transport answers a transfer.
#[derive(Clone, Debug, PartialEq)]
pub enum TransferMode {
    /// Every byte commits; the session is marked complete.
    Complete,
    /// The transport host reports a typed terminal failure (`Ok`, outcome
    /// `failed`, nothing committed) — a weak link, not a host error.
    Failed,
    /// The transport host itself errors (the producer could not open the
    /// bytes, the session URI was refused): `Err`.
    Refused(String),
    /// A cancel flag is raised mid-transfer: the transport stops with a
    /// resumable state and reports `failed` with the bytes it committed.
    CancelledAfter(u64),
}

#[derive(Debug)]
pub struct FakeWorld {
    /// The record's current input base per operation: what ds-brain would
    /// derive from the project's documents right now. An `open` whose
    /// declared fingerprint differs is answered 409.
    pub current_base: BTreeMap<String, String>,
    /// Refuse every `open` with 403 and this message — an account without
    /// the project capability, a closed project, a missing membership.
    pub refuse_open: Option<String>,
    /// Answer `open` for this one transformer with a 503. This exercises
    /// independent verdicts inside one concurrent project batch.
    pub refuse_open_resource: Option<String>,
    /// Refuse every `open` with a 400 and this message — a declaration the
    /// project's inputs do not produce.
    pub invalid_declaration: Option<String>,
    /// Refuse every `finalize` with this message, no verdict.
    pub refuse_publish: Option<String>,
    /// Refuse the n-th record call (0-based, opens and finalizes counted
    /// together) and every later one — a gateway that goes away after the
    /// uploads and before the publication is acknowledged, which is what a
    /// crash between transfer and commit looks like from the edge.
    pub refuse_publish_from: Option<usize>,
    /// Never hold the bytes: every `open` asks for every output and every
    /// `finalize` answers that they have not finished uploading. Transport
    /// succeeds, the record never advances.
    pub never_commit: bool,
    pub refuse_list: Option<String>,
    /// Answer every `finalize` with this state instead of publishing.
    pub publish_state: Option<String>,
    /// Heads the record holds when it is read, as `compute_artifacts` list rows.
    pub record: Vec<Value>,
    /// Object bytes by download url, for the read side.
    pub objects: BTreeMap<String, Vec<u8>>,
    pub transfer_mode: TransferMode,
    // -- evidence -----------------------------------------------------------
    pub committed_sessions: BTreeSet<String>,
    /// Every `open` and `finalize` body, in order.
    pub declarations: Vec<Value>,
    pub commits: u32,
    pub head_reads: u32,
    pub opens: u32,
    pub transfers: Vec<(String, String)>,
    pub downloads: Vec<String>,
    pub next_head_revision: i64,
    /// Refuse every reclaim with this message (a producer store that cannot
    /// take its lock, a verification that failed).
    pub refuse_reclaim: Option<String>,
    /// `(client_publish_id, bytes freed)` for every reclaim asked of the
    /// producer, freed or not.
    pub reclaims: Vec<(String, u64)>,
}

impl Default for FakeWorld {
    fn default() -> Self {
        FakeWorld {
            current_base: BTreeMap::from([("export-tx_1".to_string(), FINGERPRINT.to_string())]),
            refuse_open: None,
            refuse_open_resource: None,
            invalid_declaration: None,
            refuse_publish: None,
            refuse_publish_from: None,
            never_commit: false,
            refuse_list: None,
            publish_state: None,
            record: Vec::new(),
            objects: BTreeMap::new(),
            transfer_mode: TransferMode::Complete,
            committed_sessions: BTreeSet::new(),
            declarations: Vec::new(),
            commits: 0,
            head_reads: 0,
            opens: 0,
            transfers: Vec::new(),
            downloads: Vec::new(),
            next_head_revision: 8,
            refuse_reclaim: None,
            reclaims: Vec::new(),
        }
    }
}

pub type SharedWorld = Arc<Mutex<FakeWorld>>;

pub fn world() -> SharedWorld {
    Arc::new(Mutex::new(FakeWorld::default()))
}

pub fn lock(world: &SharedWorld) -> MutexGuard<'_, FakeWorld> {
    world.lock().expect("fake world")
}

/// The identity every scenario publishes.
pub fn identity() -> sync::Identity {
    sync::Identity {
        engine: "network_reporter".into(),
        operation: "export-tx_1".into(),
        variant: "default".into(),
    }
}

/// One local publication of `bytes`, produced by [`BUILD_MANIFEST`].
pub fn local_row(bytes: &[u8]) -> LocalRow {
    let outputs = vec![LocalOutput {
        filename: Some("tx_1.xlsx".into()),
        paper_size: None,
        presentation: None,
        output_id: "xlsx".into(),
        format: "xlsx".into(),
        content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        size_bytes: bytes.len() as u64,
    }];
    let mut pairs: Vec<(String, String)> = outputs
        .iter()
        .map(|output| (output.output_id.clone(), output.sha256.clone()))
        .collect();
    LocalRow {
        identity: identity(),
        sha256: inventory_digest(&mut pairs),
        size_bytes: bytes.len() as u64,
        produced_at_ms: 1_000,
        base_revision: None,
        input_base_fingerprint: Some(FINGERPRINT.into()),
        readable: true,
        engine_release: RELEASE.into(),
        engine_build_manifest_sha256: BUILD_MANIFEST.into(),
        resource: "tx_1".into(),
        client_publish_id: "publish-11111111-1111-4111-8111-111111111111".into(),
        outputs,
        bytes_locator: "fake:batch:publish-11111111-1111-4111-8111-111111111111".into(),
    }
}

/// A `compute_artifacts` list row for one head holding `bytes` under `xlsx`.
pub fn record_head(bytes: &[u8], head_revision: i64, fingerprint: &str) -> Value {
    json!({
        "engine": "network_reporter",
        "operation": "export-tx_1",
        "variant": "default",
        "head_revision": head_revision,
        "input_base_fingerprint": fingerprint,
        "updated_at": "2026-09-11T10:00:00Z",
        "outputs": [{
            "output_id": "xlsx",
            "sha256": format!("{:x}", Sha256::digest(bytes)),
            "size_bytes": bytes.len(),
        }],
    })
}

pub fn session_uri(publish_id: &str, output_id: &str) -> String {
    format!("https://storage.googleapis.com/upload/storage/v1/b/fake/o?uploadType=resumable&upload_id={publish_id}-{output_id}")
}

pub fn download_url(operation: &str, output_id: &str) -> String {
    format!("https://storage.googleapis.com/fake/{operation}/{output_id}?X-Goog-Signature=fake")
}

/// The gateway as ds-brain would answer it.
pub struct FakeGateway {
    pub world: SharedWorld,
}

fn refused(status: u16, code: &str, message: impl Into<String>) -> GatewayError {
    GatewayError::refused(status, Some(code.to_string()), message.into())
}

impl FakeWorld {
    fn head_of(&self, engine: &Value, operation: &str, variant: &Value) -> Option<&Value> {
        self.record.iter().find(|head| {
            head["engine"] == *engine
                && head["operation"] == operation
                && head["variant"] == *variant
        })
    }

    /// `open`: the declaration is judged on arrival, then the record names
    /// the bytes it still lacks.
    fn open(&mut self, body: &Value) -> Result<Value, GatewayError> {
        self.opens += 1;
        if let Some(reason) = &self.refuse_open {
            return Err(refused(403, "INSUFFICIENT_PERMISSIONS", reason.clone()));
        }
        let operation = body["operation"].as_str().unwrap_or_default().to_string();
        if self
            .refuse_open_resource
            .as_deref()
            .is_some_and(|resource| operation.strip_prefix("export-") == Some(resource))
        {
            return Err(refused(
                503,
                "SERVICE_UNAVAILABLE",
                "the authentication or project service is temporarily unreachable",
            ));
        }
        if let Some(reason) = &self.invalid_declaration {
            return Err(refused(400, "VALIDATION_FAILED", reason.clone()));
        }
        let run = body["client_run_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        // A replay of a run the record already published is that result.
        if let Some(head) = self
            .record
            .iter()
            .find(|head| head["client_publish_id"] == run.as_str())
        {
            return Ok(json!({
                "work_id": format!("work-{run}"),
                "state": "published",
                "head_revision": head["head_revision"],
                "outputs": [],
            }));
        }
        let declared = body["input_base_fingerprint"].as_str().unwrap_or_default();
        if self.current_base.get(&operation).map(String::as_str) != Some(declared) {
            return Err(refused(
                409,
                "CONFLICT",
                "compute artifact conflict: network reporter input receipt is stale",
            ));
        }
        let head_revision = self
            .head_of(&body["engine"], &operation, &body["variant"])
            .and_then(|head| head["head_revision"].as_i64())
            .unwrap_or_default();
        let never_commit = self.never_commit;
        let outputs: Vec<Value> = body["outputs"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|output| {
                let output_id = output["output_id"].as_str().unwrap_or_default();
                let uri = session_uri(&run, output_id);
                if never_commit || !self.committed_sessions.contains(&uri) {
                    json!({"output_id": output_id, "status": "upload", "session_uri": uri})
                } else {
                    json!({"output_id": output_id, "status": "already_exists", "object_generation": 1})
                }
            })
            .collect();
        Ok(json!({
            "work_id": format!("work-{run}"),
            "state": "pending",
            "head_revision": head_revision,
            "outputs": outputs,
        }))
    }

    /// `finalize`: every declared object must be stored, then the head
    /// advances once per run.
    fn finalize(&mut self, body: &Value) -> Result<Value, GatewayError> {
        if let Some(reason) = &self.refuse_publish {
            return Err(reason.clone().into());
        }
        let work_id = body["work_id"].as_str().unwrap_or_default().to_string();
        let run = body["client_run_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if let Some(state) = &self.publish_state {
            return Ok(
                json!({"work_id": work_id, "state": state, "head_revision": 0,
                "conflict": "the Network Reporter input base changed before finalization"}),
            );
        }
        let Some(opened) = self
            .declarations
            .iter()
            .rev()
            .find(|declaration| {
                declaration["action"] == "open" && declaration["client_run_id"] == run.as_str()
            })
            .cloned()
        else {
            return Err(refused(404, "NOT_FOUND", "Compute artifact work not found"));
        };
        let outputs = opened["outputs"].as_array().cloned().unwrap_or_default();
        for output in &outputs {
            let output_id = output["output_id"].as_str().unwrap_or_default();
            if self.never_commit
                || !self
                    .committed_sessions
                    .contains(&session_uri(&run, output_id))
            {
                return Err(refused(
                    400,
                    "VALIDATION_FAILED",
                    format!("compute artifact validation failed: output {output_id} has not finished uploading"),
                ));
            }
        }
        let operation = opened["operation"].as_str().unwrap_or_default().to_string();
        if let Some(head) = self
            .record
            .iter()
            .find(|head| head["client_publish_id"] == run.as_str())
        {
            return Ok(
                json!({"work_id": work_id, "state": "published", "head_revision": head["head_revision"]}),
            );
        }
        // The fence again, inside the transaction that would advance the head.
        let declared = opened["input_base_fingerprint"]
            .as_str()
            .unwrap_or_default();
        if self.current_base.get(&operation).map(String::as_str) != Some(declared) {
            return Ok(
                json!({"work_id": work_id, "state": "stored_stale", "head_revision": 0,
                "conflict": "the Network Reporter input base changed before finalization"}),
            );
        }
        self.commits += 1;
        let revision = self.next_head_revision;
        self.next_head_revision += 1;
        let head_outputs: Vec<Value> = outputs
            .iter()
            .map(|output| {
                json!({
                    "output_id": output["output_id"],
                    "sha256": output["sha256"],
                    "size_bytes": output["size_bytes"],
                })
            })
            .collect();
        self.record.retain(|head| {
            !(head["engine"] == opened["engine"]
                && head["operation"] == operation.as_str()
                && head["variant"] == opened["variant"])
        });
        self.record.push(json!({
            "engine": opened["engine"],
            "operation": operation,
            "variant": opened["variant"],
            "head_revision": revision,
            "input_base_fingerprint": declared,
            "updated_at": "2026-09-11T10:00:00Z",
            "outputs": head_outputs,
            "client_publish_id": run,
        }));
        Ok(json!({"work_id": work_id, "state": "published", "head_revision": revision}))
    }
}

impl Gateway for FakeGateway {
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, GatewayError> {
        let mut world = lock(&self.world);
        let SyncRoute::ComputeArtifacts = route;
        match body["action"].as_str().unwrap_or_default() {
            "list" => {
                if let Some(reason) = &world.refuse_list {
                    return Err(reason.clone().into());
                }
                let limit = body["limit"]
                    .as_u64()
                    .filter(|n| (1..=100).contains(n))
                    .ok_or("compute artifact list limit must be between 1 and 100")?
                    as usize;
                let offset = body["cursor"]
                    .as_str()
                    .unwrap_or("0")
                    .parse::<usize>()
                    .map_err(|_| "invalid fake cursor")?;
                if offset > world.record.len() {
                    return Err("fake cursor is outside inventory".into());
                }
                world.head_reads += 1;
                let end = (offset + limit).min(world.record.len());
                let next = if end < world.record.len() {
                    end.to_string()
                } else {
                    String::new()
                };
                Ok(json!({"heads": world.record[offset..end], "next_cursor": next}))
            }
            "download" => {
                let operation = body["operation"].as_str().unwrap_or_default().to_string();
                let output_id = body["output_id"].as_str().unwrap_or_default().to_string();
                world.downloads.push(operation.clone());
                Ok(json!({"download_url": download_url(&operation, &output_id)}))
            }
            action @ ("open" | "finalize") => {
                world.declarations.push(body.clone());
                if world
                    .refuse_publish_from
                    .is_some_and(|from| world.declarations.len() > from)
                {
                    return Err(
                        "the gateway went away before the publication was acknowledged".into(),
                    );
                }
                if action == "open" {
                    world.open(body)
                } else {
                    world.finalize(body)
                }
            }
            other => Err(format!("the fake record does not answer `{other}`").into()),
        }
    }
}

/// The producer: rows this machine holds, and a scripted transport.
pub struct FakeProducer {
    pub world: SharedWorld,
    pub rows: Mutex<Vec<LocalRow>>,
    /// Raised by a caller to cancel an in-flight transfer.
    pub cancel: Arc<AtomicBool>,
}

impl FakeProducer {
    pub fn new(world: SharedWorld, rows: Vec<LocalRow>) -> Self {
        FakeProducer {
            world,
            rows: Mutex::new(rows),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Producer for FakeProducer {
    fn inventory(&self, _project: &str) -> Result<Vec<LocalRow>, String> {
        Ok(self.rows.lock().expect("rows").clone())
    }

    fn transfer(
        &self,
        row: &LocalRow,
        output_id: &str,
        session_uri: &str,
    ) -> Result<TransferReceipt, String> {
        let mut world = lock(&self.world);
        world
            .transfers
            .push((output_id.to_string(), session_uri.to_string()));
        let total = row
            .outputs
            .iter()
            .find(|output| output.output_id == output_id)
            .map(|output| output.size_bytes)
            .unwrap_or_default();
        let mode = world.transfer_mode.clone();
        match mode {
            TransferMode::Complete => {
                world.committed_sessions.insert(session_uri.to_string());
                Ok(TransferReceipt {
                    output_id: output_id.to_string(),
                    outcome: TransferReceipt::COMPLETED.into(),
                    committed_bytes: total,
                    total_bytes: total,
                })
            }
            TransferMode::Failed => Ok(TransferReceipt {
                output_id: output_id.to_string(),
                outcome: "failed".into(),
                committed_bytes: 0,
                total_bytes: total,
            }),
            TransferMode::Refused(reason) => Err(reason),
            TransferMode::CancelledAfter(committed) => {
                self.cancel.store(true, Ordering::SeqCst);
                Ok(TransferReceipt {
                    outcome: "failed".into(),
                    output_id: output_id.to_string(),
                    committed_bytes: committed.min(total),
                    total_bytes: total,
                })
            }
        }
    }

    /// The bytes leave this machine: the row is dropped from the inventory
    /// and its size is what was freed. A row already gone frees 0.
    fn reclaim(&self, _project: &str, row: &LocalRow) -> Result<u64, String> {
        if let Some(refusal) = &lock(&self.world).refuse_reclaim {
            return Err(refusal.clone());
        }
        let mut rows = self.rows.lock().expect("rows");
        let before = rows.len();
        rows.retain(|held| held.client_publish_id != row.client_publish_id);
        let freed = if rows.len() < before {
            row.size_bytes
        } else {
            0
        };
        lock(&self.world)
            .reclaims
            .push((row.client_publish_id.clone(), freed));
        Ok(freed)
    }
}

/// A read cache under a temporary root, verifying the fake objects' bytes
/// through the real placement path.
pub struct FakeReads {
    pub world: SharedWorld,
    pub root: PathBuf,
}

impl Reads for FakeReads {
    fn head_root(&self, project: &str, identity: &sync::Identity) -> Result<PathBuf, String> {
        crate::reads::owned_directory(
            &self.root,
            &Path::new(project)
                .join(&identity.engine)
                .join(&identity.operation)
                .join(&identity.variant),
        )
    }

    fn download_verified(
        &self,
        url: &str,
        destination: &Path,
        sha256: &str,
        size_bytes: u64,
    ) -> Result<(), String> {
        static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        self.download_verified_cause_until(url, destination, sha256, size_bytes, &NEVER)
            .map_err(String::from)
    }

    fn download_verified_cause_until(
        &self,
        url: &str,
        destination: &Path,
        sha256: &str,
        size_bytes: u64,
        _stop: &std::sync::atomic::AtomicBool,
    ) -> Result<(), GatewayError> {
        let bytes = lock(&self.world)
            .objects
            .get(url)
            .cloned()
            .ok_or_else(|| GatewayError::from("the storage host answered 404"))?;
        place_verified_cause(&bytes[..], destination, sha256, size_bytes)
    }

    fn sweep_temporaries(
        &self,
        project: &str,
        before: std::time::SystemTime,
    ) -> Result<(usize, u64), String> {
        crate::reads::sweep_temporaries(&self.root, project, before)
    }
}
