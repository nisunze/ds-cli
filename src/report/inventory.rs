//! One report inventory from scoped native facts. Hosts read and retain facts;
//! this owner chooses output identities and separates generation/publication.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::artifacts::Artifact;
use crate::compute_artifact_inventory::{self, Head, OutputOrigin};
use crate::sync_store::{ArtifactState, HeadOutput};

pub const SCHEMA: &str = "ds.report.inventory/v1";
const MAX_SETS: usize = 4096;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadState {
    Ready,
    Held,
    Unavailable,
    NotObserved,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadFact {
    pub state: ReadState,
    /// A held projection remains usable, but its refresh failure stays visible.
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommittedSet {
    pub owner: String,
    pub project: String,
    pub transformer: String,
    pub operation: String,
    pub variant: String,
    pub batch_id: String,
    pub client_publish_id: String,
    pub client_run_id: String,
    pub engine_release: String,
    pub engine_build_manifest_sha256: Option<String>,
    pub input_base_fingerprint: String,
    /// From the native generation/commit fact, never a cloud status timestamp.
    pub generated_at_ms: Option<u64>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncArtifact {
    pub owner: String,
    pub project: String,
    pub operation: String,
    pub variant: String,
    pub batch_id: String,
    pub client_publish_id: String,
    pub input_base_fingerprint: String,
    pub state: ArtifactState,
    /// Bound by the native upload receipt to this publication; an identity-only
    /// historical receipt cannot fill it. Missing evidence remains unknown.
    pub acknowledged_work_id: Option<String>,
    pub updated_at_ms: u64,
    pub reason: Option<String>,
    pub outputs: Vec<HeadOutput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudHead {
    pub owner: String,
    pub project: String,
    pub head: Head,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCloud {
    pub state: String,
    pub reported_at_ms: Option<u64>,
    /// Legacy URL presence is not digest-pinned cloud publication evidence.
    pub files: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub owner: String,
    pub project: String,
    pub transformer: String,
    pub variant: String,
    pub expected_input_base_fingerprint: Option<String>,
    pub local_read: ReadFact,
    pub sync_read: ReadFact,
    pub cloud_read: ReadFact,
    /// Already ordered by the native committed catalog, current first. This
    /// projection never invents order by sorting timestamps. Surviving outputs
    /// retain their producing set, as current_output_indices requires.
    pub local_sets: Vec<CommittedSet>,
    pub sync_artifacts: Vec<SyncArtifact>,
    pub cloud_head: Option<CloudHead>,
    pub legacy_cloud: Option<LegacyCloud>,
    pub required_outputs: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CloudBinding<'a> {
    pub head_revision: u64,
    pub work_id: &'a str,
    pub published_at: &'a str,
    pub origin: Option<&'a OutputOrigin>,
}

#[derive(Debug, Serialize)]
pub struct Output<'a> {
    pub output_id: &'a str,
    pub batch_id: Option<&'a str>,
    pub client_publish_id: Option<&'a str>,
    pub client_run_id: Option<&'a str>,
    pub generated_at_ms: Option<u64>,
    pub local: Option<&'a Artifact>,
    pub cloud: Option<&'a compute_artifact_inventory::Declaration>,
    pub cloud_binding: Option<CloudBinding<'a>>,
    pub publication_phase: &'static str,
    pub source_state: &'static str,
    pub issue: Option<&'a str>,
}

#[derive(Debug, Serialize)]
pub struct View<'a> {
    pub schema: &'static str,
    pub owner: &'a str,
    pub project: &'a str,
    pub transformer: &'a str,
    pub variant: &'a str,
    pub active_client_publish_id: Option<&'a str>,
    pub active_generation_ambiguous: bool,
    pub generated_at_ms: Option<u64>,
    pub status: &'static str,
    pub local_read: &'a ReadFact,
    pub sync_read: &'a ReadFact,
    pub cloud_read: &'a ReadFact,
    pub outputs: Vec<Output<'a>>,
    pub local_output_count: usize,
    pub published_output_count: usize,
    /// Preserved separately: never folded into a native producing generation.
    pub legacy_cloud: Option<&'a LegacyCloud>,
    pub missing_outputs: Vec<&'a str>,
    pub complete_inventory_observed: bool,
}

fn text(value: &str, bound: usize) -> bool {
    !value.is_empty()
        && value.len() <= bound
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_read(fact: &ReadFact) -> bool {
    fact.error.as_deref().is_none_or(|error| text(error, 2000))
        && match fact.state {
            ReadState::Ready | ReadState::NotObserved => fact.error.is_none(),
            ReadState::Held | ReadState::Unavailable => fact.error.is_some(),
        }
}

fn same_pins(set: &CommittedSet, sync: &SyncArtifact) -> bool {
    set.batch_id == sync.batch_id
        && set.client_publish_id == sync.client_publish_id
        && set.input_base_fingerprint == sync.input_base_fingerprint
        && set.artifacts.len() == sync.outputs.len()
        && set.artifacts.iter().all(|artifact| {
            sync.outputs.iter().any(|output| {
                output.output_id == artifact.output_id
                    && output.sha256 == artifact.sha256
                    && output.size_bytes == artifact.size_bytes
            })
        })
}

/// Pure facts projection, not an authority grant or filesystem read. Native
/// hosts fence every underlying read before supplying this bounded snapshot.
pub fn project(snapshot: &Snapshot) -> Result<View<'_>, String> {
    let invalid = || "Report inventory requires bounded exact scoped facts".to_string();
    let operation = format!("export-{}", snapshot.transformer);
    if !text(&snapshot.owner, 128)
        || !text(&snapshot.project, 128)
        || !text(&snapshot.variant, 128)
        || !text(&snapshot.transformer, 128)
        || !snapshot
            .transformer
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || !crate::design::row_allows(&snapshot.transformer).reportable
        || snapshot
            .expected_input_base_fingerprint
            .as_deref()
            .is_some_and(|value| !hex(value))
        || !valid_read(&snapshot.local_read)
        || !valid_read(&snapshot.sync_read)
        || !valid_read(&snapshot.cloud_read)
        || snapshot.local_sets.len() > MAX_SETS
        || snapshot.sync_artifacts.len() > MAX_SETS
        || snapshot.required_outputs.len() > 32
    {
        return Err(invalid());
    }
    if !matches!(
        snapshot.local_read.state,
        ReadState::Ready | ReadState::Held
    ) && !snapshot.local_sets.is_empty()
        || !matches!(snapshot.sync_read.state, ReadState::Ready | ReadState::Held)
            && !snapshot.sync_artifacts.is_empty()
        || !matches!(
            snapshot.cloud_read.state,
            ReadState::Ready | ReadState::Held
        ) && snapshot.cloud_head.is_some()
    {
        return Err(invalid());
    }
    let scoped = |owner: &str, project: &str, op: &str, variant: &str| {
        owner == snapshot.owner
            && project == snapshot.project
            && op == operation
            && variant == snapshot.variant
    };
    let mut batches = BTreeSet::new();
    let mut publications = BTreeSet::new();
    for set in &snapshot.local_sets {
        if !scoped(&set.owner, &set.project, &set.operation, &set.variant)
            || set.transformer != snapshot.transformer
            || !hex(&set.batch_id)
            || !hex(&set.input_base_fingerprint)
            || set
                .engine_build_manifest_sha256
                .as_deref()
                .is_some_and(|value| !hex(value))
            || !text(&set.client_publish_id, 128)
            || !text(&set.client_run_id, 128)
            || !(super::valid_network_reporter_engine_identity(&set.engine_release)
                || set.engine_release == super::DEVELOPMENT_ENGINE)
            || !batches.insert(&set.batch_id)
            || !publications.insert(&set.client_publish_id)
        {
            return Err(invalid());
        }
        super::artifacts::validate(set.artifacts.clone(), false, true, false)?;
    }
    let mut sync_ids = BTreeSet::new();
    for row in &snapshot.sync_artifacts {
        let mut output_ids = BTreeSet::new();
        if !scoped(&row.owner, &row.project, &row.operation, &row.variant)
            || !hex(&row.batch_id)
            || !hex(&row.input_base_fingerprint)
            || !text(&row.client_publish_id, 128)
            || !sync_ids.insert(&row.client_publish_id)
            || row.outputs.is_empty()
            || row.outputs.len() > 32
            || row
                .acknowledged_work_id
                .as_deref()
                .is_some_and(|value| !text(value, 128))
            || row
                .reason
                .as_deref()
                .is_some_and(|value| !text(value, 2000))
            || row.outputs.iter().any(|output| {
                super::artifacts::output(&output.output_id).is_none()
                    || !hex(&output.sha256)
                    || output.size_bytes == 0
                    || output.size_bytes > 9_007_199_254_740_991
                    || !output_ids.insert(&output.output_id)
            })
        {
            return Err(invalid());
        }
    }
    let head = if let Some(cloud) = &snapshot.cloud_head {
        if !scoped(
            &cloud.owner,
            &cloud.project,
            &cloud.head.operation,
            &cloud.head.variant,
        ) || cloud.head.engine != "network_reporter"
        {
            return Err(invalid());
        }
        compute_artifact_inventory::decode_head(
            &serde_json::to_vec(&cloud.head).map_err(|_| invalid())?,
        )?;
        Some(&cloud.head)
    } else {
        None
    };
    if let Some(legacy) = &snapshot.legacy_cloud {
        if !text(&legacy.state, 128)
            || legacy.files.len() > 32
            || legacy.files.iter().any(|file| !text(file, 4096))
        {
            return Err(invalid());
        }
    }
    let mut required = BTreeSet::new();
    for id in &snapshot.required_outputs {
        if super::artifacts::output(id).is_none() || !required.insert(id) {
            return Err(invalid());
        }
    }
    let kept = super::current_output_indices(
        &snapshot
            .local_sets
            .iter()
            .map(|set| {
                set.artifacts
                    .iter()
                    .map(|artifact| artifact.output_id.clone())
                    .collect()
            })
            .collect::<Vec<_>>(),
    );
    let mut outputs = BTreeMap::new();
    for (set, indices) in snapshot.local_sets.iter().zip(kept) {
        let sync = snapshot
            .sync_artifacts
            .iter()
            .find(|row| row.client_publish_id == set.client_publish_id);
        let pins_match = sync.is_some_and(|row| same_pins(set, row));
        for index in indices {
            let artifact = &set.artifacts[index];
            let cloud_output = head.and_then(|head| {
                head.outputs.iter().find(|output| {
                    output.output_id == artifact.output_id
                        && output.sha256 == artifact.sha256
                        && output.size_bytes == artifact.size_bytes
                        && output.format == artifact.format
                        && output.content_type == artifact.content_type
                        && output.paper_size == artifact.paper_size
                        && output.presentation == artifact.presentation
                })
            });
            // A matching filename or timestamp cannot establish a producing
            // source. Older heads lacking per-output origin stay unverified.
            let origin = head
                .and_then(|head| head.output_origin(&artifact.output_id))
                .filter(|origin| {
                    origin.input_base_fingerprint == set.input_base_fingerprint
                        && origin.engine_version.as_deref() == Some(set.engine_release.as_str())
                        && origin.engine_build_manifest_sha256.as_deref()
                            == set.engine_build_manifest_sha256.as_deref()
                });
            let cloud_verified = pins_match
                && sync.is_some_and(|row| row.state == ArtifactState::Published)
                && sync.is_some_and(|row| {
                    row.acknowledged_work_id.is_some()
                        && row.acknowledged_work_id.as_deref()
                            == origin.map(|origin| origin.work_id.as_str())
                })
                && cloud_output.is_some()
                && origin.is_some()
                && snapshot.cloud_read.state == ReadState::Ready;
            let source_state = match snapshot.expected_input_base_fingerprint.as_deref() {
                Some(expected) if expected == set.input_base_fingerprint => "matched",
                Some(_) => "changed",
                None => "not_observed",
            };
            let publication_phase = if set.engine_release == super::DEVELOPMENT_ENGINE {
                "local_only"
            } else if sync.is_some() && !pins_match {
                "evidence_mismatch"
            } else if let Some(row) = sync {
                match row.state {
                    ArtifactState::Held => "queued",
                    ArtifactState::Uploading => "uploading",
                    ArtifactState::Published if cloud_verified => "published",
                    ArtifactState::Published => "acknowledgement_unverified",
                    ArtifactState::Conflict => "conflicted",
                    ArtifactState::Refused => "refused",
                }
            } else if snapshot.sync_read.state == ReadState::Ready {
                "sealed"
            } else {
                "not_observed"
            };
            outputs.insert(
                artifact.output_id.as_str(),
                Output {
                    output_id: &artifact.output_id,
                    batch_id: Some(&set.batch_id),
                    client_publish_id: Some(&set.client_publish_id),
                    client_run_id: Some(&set.client_run_id),
                    generated_at_ms: set.generated_at_ms,
                    local: Some(artifact),
                    cloud: cloud_verified.then_some(cloud_output).flatten(),
                    cloud_binding: cloud_verified.then(|| {
                        let head = head.expect("verified cloud has a head");
                        CloudBinding {
                            head_revision: head.head_revision,
                            work_id: &origin.expect("verified cloud has origin").work_id,
                            published_at: &head.updated_at,
                            origin,
                        }
                    }),
                    publication_phase,
                    source_state,
                    issue: if sync.is_some() && !pins_match {
                        Some("native publication pins disagree with committed generation")
                    } else {
                        sync.and_then(|row| row.reason.as_deref())
                    },
                },
            );
        }
    }
    // Validated current cloud-only outputs keep their own head/origin. They
    // never acquire a local generation id or generation timestamp.
    if let Some(head) = head {
        for output in &head.outputs {
            outputs.entry(&output.output_id).or_insert_with(|| Output {
                output_id: &output.output_id,
                batch_id: None,
                client_publish_id: None,
                client_run_id: None,
                generated_at_ms: None,
                local: None,
                cloud: Some(output),
                cloud_binding: Some(CloudBinding {
                    head_revision: head.head_revision,
                    work_id: head
                        .output_origin(&output.output_id)
                        .map_or(head.work_id.as_str(), |origin| origin.work_id.as_str()),
                    published_at: &head.updated_at,
                    origin: head.output_origin(&output.output_id),
                }),
                publication_phase: if snapshot.cloud_read.state == ReadState::Ready {
                    "published"
                } else {
                    "held_cloud_observation"
                },
                source_state: match (
                    snapshot.expected_input_base_fingerprint.as_deref(),
                    head.output_origin(&output.output_id),
                ) {
                    (Some(expected), Some(origin)) if origin.input_base_fingerprint == expected => {
                        "matched"
                    }
                    (Some(_), Some(_)) => "changed",
                    _ => "not_observed",
                },
                issue: None,
            });
        }
    }
    // The current native queue row selects the active producing run. Dates
    // never select it. A publication refusal/conflict does not erase the
    // generation fact while this exact committed run remains available.
    let active = snapshot
        .sync_artifacts
        .iter()
        .filter_map(|row| {
            snapshot.local_sets.iter().find(|set| {
                same_pins(set, row)
                    && outputs.values().any(|output| {
                        output.client_publish_id == Some(set.client_publish_id.as_str())
                    })
            })
        })
        .collect::<Vec<_>>();
    let active_generation_ambiguous = active.len() > 1;
    let active = (active.len() == 1).then(|| active[0]);
    let missing_outputs: Vec<_> = snapshot
        .required_outputs
        .iter()
        .filter(|id| !outputs.contains_key(id.as_str()))
        .map(String::as_str)
        .collect();
    let complete_inventory_observed = snapshot.local_read.state == ReadState::Ready
        && snapshot.sync_read.state == ReadState::Ready
        && snapshot.cloud_read.state == ReadState::Ready;
    let status = if outputs
        .values()
        .any(|output| output.publication_phase == "evidence_mismatch")
    {
        "evidence_error"
    } else if active_generation_ambiguous {
        "ambiguous_generation"
    } else if outputs
        .values()
        .any(|output| output.source_state == "changed")
    {
        "stale"
    } else if !missing_outputs.is_empty() {
        "missing_outputs"
    } else if !complete_inventory_observed {
        "partial_observation"
    } else if outputs.is_empty() {
        "empty"
    } else {
        "available"
    };
    Ok(View {
        schema: SCHEMA,
        owner: &snapshot.owner,
        project: &snapshot.project,
        transformer: &snapshot.transformer,
        variant: &snapshot.variant,
        active_client_publish_id: active.map(|set| set.client_publish_id.as_str()),
        active_generation_ambiguous,
        generated_at_ms: active.and_then(|set| set.generated_at_ms),
        status,
        local_read: &snapshot.local_read,
        sync_read: &snapshot.sync_read,
        cloud_read: &snapshot.cloud_read,
        local_output_count: outputs
            .values()
            .filter(|output| output.local.is_some())
            .count(),
        published_output_count: outputs
            .values()
            .filter(|output| output.publication_phase == "published")
            .count(),
        outputs: outputs.into_values().collect(),
        legacy_cloud: snapshot.legacy_cloud.as_ref(),
        missing_outputs,
        complete_inventory_observed,
    })
}
