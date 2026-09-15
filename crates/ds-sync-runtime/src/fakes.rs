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

use crate::gateway::Gateway;
use crate::host::{Producer, Reads};
use crate::reads::place_verified;
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
    /// `scope.engines[].manifest_sha256` the minted grant carries.
    pub admitted_builds: Vec<String>,
    pub snapshot_base: BTreeMap<String, String>,
    /// RFC 3339, as ds-brain spells `expires_at`.
    pub grant_expires_at: String,
    /// Refuse `work/open` with this message (an unauthorized identity, a
    /// blocked install, a missing membership).
    pub refuse_open: Option<String>,
    /// Refuse only the grant for this resource. This exercises independent
    /// resource authority inside one concurrent project batch.
    pub refuse_open_resource: Option<String>,
    pub refuse_publish: Option<String>,
    pub refuse_list: Option<String>,
    /// Answer every publish with this state instead of the two-phase flow.
    pub publish_state: Option<String>,
    /// Heads the record holds when it is read, as `compute_artifacts` list rows.
    pub record: Vec<Value>,
    /// Object bytes by download url, for the read side.
    pub objects: BTreeMap<String, Vec<u8>>,
    pub transfer_mode: TransferMode,
    // -- evidence -----------------------------------------------------------
    pub committed_sessions: BTreeSet<String>,
    pub declarations: Vec<Value>,
    pub commits: u32,
    pub head_reads: u32,
    pub grants_minted: u32,
    pub transfers: Vec<(String, String)>,
    pub downloads: Vec<String>,
    pub next_head_revision: i64,
}

impl Default for FakeWorld {
    fn default() -> Self {
        FakeWorld {
            admitted_builds: vec![BUILD_MANIFEST.to_string()],
            snapshot_base: BTreeMap::from([("export-tx_1".to_string(), FINGERPRINT.to_string())]),
            grant_expires_at: "2100-01-01T00:00:00Z".to_string(),
            refuse_open: None,
            refuse_open_resource: None,
            refuse_publish: None,
            refuse_list: None,
            publish_state: None,
            record: Vec::new(),
            objects: BTreeMap::new(),
            transfer_mode: TransferMode::Complete,
            committed_sessions: BTreeSet::new(),
            declarations: Vec::new(),
            commits: 0,
            head_reads: 0,
            grants_minted: 0,
            transfers: Vec::new(),
            downloads: Vec::new(),
            next_head_revision: 8,
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
        grant_engine: "ds-network-reporter".into(),
        resource: "tx_1".into(),
        client_publish_id: "publish-11111111-1111-4111-8111-111111111111".into(),
        outputs,
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

fn session_uri(publish_id: &str, kind: &str) -> String {
    format!("https://storage.googleapis.com/upload/storage/v1/b/fake/o?uploadType=resumable&upload_id={publish_id}-{kind}")
}

pub fn download_url(operation: &str, output_id: &str) -> String {
    format!("https://storage.googleapis.com/fake/{operation}/{output_id}?X-Goog-Signature=fake")
}

/// The gateway as ds-brain would answer it.
pub struct FakeGateway {
    pub world: SharedWorld,
}

impl Gateway for FakeGateway {
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, String> {
        let mut world = lock(&self.world);
        match route {
            SyncRoute::ComputeArtifacts => match body["action"].as_str().unwrap_or_default() {
                "list" => {
                    if let Some(reason) = &world.refuse_list {
                        return Err(reason.clone());
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
                other => Err(format!("the fake record does not answer `{other}`")),
            },
            SyncRoute::WorkOpen => {
                if let Some(reason) = &world.refuse_open {
                    return Err(reason.clone());
                }
                let resource = body["resources"]["transformer_names"]
                    .as_array()
                    .and_then(|resources| resources.first())
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if world.refuse_open_resource.as_deref() == Some(resource) {
                    return Err(
                        "the authentication or project service is temporarily unreachable"
                            .to_string(),
                    );
                }
                world.grants_minted += 1;
                let engine = body["engine"].as_str().unwrap_or_default();
                let release = body["engine_release"].as_str().unwrap_or_default();
                let engines: Vec<Value> = world
                    .admitted_builds
                    .iter()
                    .map(|digest| json!({"name": engine, "release": release, "manifest_sha256": digest}))
                    .collect();
                Ok(json!({
                    "grant": {
                        "grant_id": format!("grant-{}", world.grants_minted),
                        "install_id": body["install_id"],
                        "scope": {"ds_project": body["ds_project"], "engines": engines},
                        "expires_at": world.grant_expires_at,
                        "snapshot_base": world.snapshot_base,
                    }
                }))
            }
            SyncRoute::WorkPublish => {
                world.declarations.push(body.clone());
                if let Some(reason) = &world.refuse_publish {
                    return Err(reason.clone());
                }
                if let Some(state) = &world.publish_state {
                    return Ok(json!({"state": state, "work_id": "work-1"}));
                }
                let publish_id = body["client_publish_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let artifacts = body["artifacts"].as_array().cloned().unwrap_or_default();
                let missing: Vec<Value> = artifacts
                    .iter()
                    .filter_map(|artifact| {
                        let kind = artifact["kind"].as_str().unwrap_or_default().to_string();
                        let uri = session_uri(&publish_id, &kind);
                        (!world.committed_sessions.contains(&uri))
                            .then(|| json!({"kind": kind, "session_uri": uri}))
                    })
                    .collect();
                if !missing.is_empty() {
                    return Ok(json!({
                        "state": "upload_required",
                        "work_id": "work-1",
                        "uploads": missing,
                    }));
                }
                // The record advances once per publication. A repeat of the
                // same declaration replays the durable result.
                let already = world
                    .record
                    .iter()
                    .any(|head| head["client_publish_id"] == publish_id);
                let head_revision = if already {
                    world
                        .record
                        .iter()
                        .find(|head| head["client_publish_id"] == publish_id)
                        .and_then(|head| head["head_revision"].as_i64())
                        .unwrap_or_default()
                } else {
                    world.commits += 1;
                    let revision = world.next_head_revision;
                    world.next_head_revision += 1;
                    let outputs: Vec<Value> = artifacts
                        .iter()
                        .map(|artifact| {
                            json!({
                                "output_id": artifact["kind"],
                                "sha256": artifact["sha256"],
                                "size_bytes": artifact["size_bytes"],
                            })
                        })
                        .collect();
                    let base = body["base_revisions"]["export-tx_1"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    world.record.retain(|head| {
                        !(head["engine"] == "network_reporter"
                            && head["operation"] == "export-tx_1"
                            && head["variant"] == "default")
                    });
                    world.record.push(json!({
                        "engine": "network_reporter",
                        "operation": "export-tx_1",
                        "variant": "default",
                        "head_revision": revision,
                        "input_base_fingerprint": base,
                        "updated_at": "2026-09-11T10:00:00Z",
                        "outputs": outputs,
                        "client_publish_id": publish_id,
                    }));
                    revision
                };
                Ok(json!({
                    "state": "published",
                    "work_id": "work-1",
                    "head_revision": head_revision,
                    "uploads": [],
                }))
            }
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

    fn locator(&self, row: &LocalRow) -> String {
        format!("fake:batch:{}", row.client_publish_id)
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
                    outcome: "done".into(),
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
                    output_id: output_id.to_string(),
                    outcome: "failed".into(),
                    committed_bytes: committed.min(total),
                    total_bytes: total,
                })
            }
        }
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
        let bytes = lock(&self.world)
            .objects
            .get(url)
            .cloned()
            .ok_or_else(|| "the storage host answered 404".to_string())?;
        place_verified(&bytes[..], destination, sha256, size_bytes)
    }
}
