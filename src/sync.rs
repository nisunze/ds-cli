//! The sync gate's one decision.
//!
//! Given what this machine holds for a project, what the shared record holds,
//! the credential in hand and the clock, this module answers the only question
//! the Sync Center exists to answer — upload, download, conflict, nothing — and
//! says when to look again. It is pure: hosts perform IO (list local artifacts,
//! read remote heads, call `work/open`, move bytes, call `work/publish`);
//! `compute_artifact_engine` decides how one publication proceeds and
//! `transfer` decides how bytes move. Nothing here is personal: the plan is
//! keyed by project and artifact identity, and the credential is a work grant
//! bound to an install and a project, never to an account.
//!
//! Owner rulings this module implements (2026-09-11): one function; project
//! data, not personal; no fallback path; the desktop uses the credential
//! ds-brain issued to upload resumably regardless of size; remote conflict and
//! freshness are checked here; WHEN to look again is decided here — and it is
//! never a timer: "we cannot afford passive aggressive polling; this should be
//! triggered by dependencies and navigations, not background hammering of
//! Firestore". A plan is asked for on a trigger (start, navigation, a local
//! change, a completed publish or download, reconnect, a push from the
//! messaging stream, an explicit sync) and says whether the remote heads must
//! be re-read for THIS trigger; the only timed wakes are renewing a grant
//! before an upload and the transport's own retry.
//! Dangling rows of the retired per-user outbox are not an input.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "ds.sync-plan/v1";
const INPUT_BOUND: usize = 8 * 1024 * 1024;
const MAX_ARTIFACTS: usize = 4096;
const MAX_ID_LEN: usize = 128;

/// Renew a grant this long before it expires so publish never races expiry.
/// This is the ONE timed wake the plan may ask for besides the transport's
/// own retry; there is no poll.
pub const GRANT_RENEW_LEAD_MS: u64 = 10 * 60 * 1000;

/// Why the host is asking now. Every plan is asked for a reason; none is
/// asked because time passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// The app started or the project was opened; also the Sync Center opening.
    Startup,
    Navigation,
    /// A local artifact finished or changed on this machine.
    LocalChange,
    /// A `work/publish` completed; the receipt carries the new head.
    PublishCompleted,
    DownloadCompleted,
    Reconnect,
    /// The messaging stream said the project's record changed.
    RemotePush,
    /// The operator pressed "Sync now".
    Manual,
    /// A grant was just opened; the plan is re-asked to proceed with uploads.
    GrantOpened,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub engine: String,
    pub operation: String,
    #[serde(default = "default_variant")]
    pub variant: String,
}

fn default_variant() -> String {
    "default".into()
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EngineRelease {
    pub name: String,
    pub release: String,
}

/// The signed work grant as the host holds it. Parsed, never verified here:
/// the signature is the server's to check, and the server re-authorizes at
/// publish anyway. This module only reads the scope it claims.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub grant_id: String,
    pub install_id: String,
    pub project: String,
    #[serde(default)]
    pub engines: Vec<EngineRelease>,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalArtifact {
    #[serde(flatten)]
    pub identity: Identity,
    pub sha256: String,
    pub size_bytes: u64,
    pub produced_at_ms: u64,
    /// The remote head revision this result was computed against, when one
    /// existed at compute time. `None` means the result was computed with no
    /// remote head in view.
    #[serde(default)]
    pub base_revision: Option<String>,
    /// The host could open the bytes and they match `sha256`/`size_bytes`.
    #[serde(default = "default_true")]
    pub readable: bool,
    pub engine_release: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteHead {
    #[serde(flatten)]
    pub identity: Identity,
    pub revision: String,
    pub sha256: String,
    pub published_at_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema: String,
    pub project: String,
    pub install_id: String,
    /// The host's monotonic high-water mark of server-observed time.
    pub now_ms: u64,
    pub online: bool,
    pub trigger: Trigger,
    /// When the held `remote` heads were last read from the record; absent
    /// when they never were. Never a reason to read again by itself.
    #[serde(default)]
    pub remote_read_at_ms: Option<u64>,
    #[serde(default)]
    pub grant: Option<Grant>,
    #[serde(default)]
    pub local: Vec<LocalArtifact>,
    #[serde(default)]
    pub remote: Vec<RemoteHead>,
}

/// Whether the remote heads must be re-read for this trigger.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RefreshRemote {
    pub needed: bool,
    pub reason: &'static str,
}

/// When the host must plan again if no trigger arrives first. `event` means
/// never on a timer.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Wake {
    Event,
    At { at_ms: u64, reason: &'static str },
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Ask ds-brain for a work grant before any upload; the host re-plans after.
    OpenGrant {
        reason: &'static str,
    },
    Upload {
        identity: Identity,
        sha256: String,
        size_bytes: u64,
        reason: &'static str,
    },
    Download {
        identity: Identity,
        revision: String,
        sha256: String,
        reason: &'static str,
    },
    /// The shared record moved past the base this result was computed on. It
    /// is surfaced, never overwritten: the operator recomputes or downloads.
    Conflict {
        identity: Identity,
        base_revision: Option<String>,
        current_revision: String,
        reason: &'static str,
    },
    Nothing {
        identity: Identity,
        reason: &'static str,
    },
    Refused {
        #[serde(skip_serializing_if = "Option::is_none")]
        identity: Option<Identity>,
        reason: &'static str,
    },
    Offline,
}

#[derive(Debug, Serialize, Default, PartialEq, Eq)]
pub struct Summary {
    pub uploads: usize,
    pub downloads: usize,
    pub conflicts: usize,
    pub refused: usize,
    pub in_sync: usize,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub schema: &'static str,
    pub project: String,
    /// Ordered: the host performs these top to bottom and re-plans after.
    pub actions: Vec<Action>,
    /// Whether to re-read the remote heads for this trigger before acting.
    pub refresh_remote: RefreshRemote,
    /// The only timed wake, if any; otherwise wait for a trigger.
    pub wake: Wake,
    pub grant_valid: bool,
    pub summary: Summary,
}

fn refresh_remote(input: &Input) -> RefreshRemote {
    if input.remote_read_at_ms.is_none() {
        return RefreshRemote {
            needed: true,
            reason: "never_read",
        };
    }
    match input.trigger {
        Trigger::Startup | Trigger::Navigation | Trigger::Reconnect | Trigger::Manual => {
            RefreshRemote {
                needed: true,
                reason: "trigger_reads_record",
            }
        }
        Trigger::RemotePush => RefreshRemote {
            needed: true,
            reason: "record_changed",
        },
        Trigger::LocalChange => RefreshRemote {
            needed: false,
            reason: "held_heads_suffice",
        },
        Trigger::PublishCompleted => RefreshRemote {
            needed: false,
            reason: "receipt_carries_the_head",
        },
        Trigger::DownloadCompleted | Trigger::GrantOpened => RefreshRemote {
            needed: false,
            reason: "nothing_remote_changed",
        },
    }
}

fn valid_id(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    value.len() <= MAX_ID_LEN && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn valid_identity(identity: &Identity) -> bool {
    valid_id(&identity.engine) && valid_id(&identity.operation) && valid_id(&identity.variant)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Why the grant in hand cannot cover an upload, if it cannot.
fn grant_problem(input: &Input, needed: &BTreeSet<&str>) -> Option<&'static str> {
    let Some(grant) = &input.grant else {
        return Some("no_grant");
    };
    if grant.project != input.project {
        return Some("grant_project_mismatch");
    }
    if grant.install_id != input.install_id {
        return Some("grant_install_mismatch");
    }
    if grant.expires_at_ms <= input.now_ms {
        return Some("grant_expired");
    }
    if needed.iter().any(|release| {
        !grant
            .engines
            .iter()
            .any(|engine| engine.release == *release)
    }) {
        return Some("grant_engine_missing");
    }
    None
}

pub fn plan(input: &Input) -> Result<Plan, String> {
    if input.schema != SCHEMA {
        return Err(format!("unsupported sync plan schema `{}`", input.schema));
    }
    if input.project.is_empty() || input.install_id.is_empty() {
        return Err("a sync plan needs a project and an install".into());
    }
    if input.local.len() > MAX_ARTIFACTS || input.remote.len() > MAX_ARTIFACTS {
        return Err("sync inventories exceed their bound".into());
    }
    let mut plan = Plan {
        schema: SCHEMA,
        project: input.project.clone(),
        actions: Vec::new(),
        refresh_remote: RefreshRemote {
            needed: false,
            reason: "offline",
        },
        wake: Wake::Event,
        grant_valid: false,
        summary: Summary::default(),
    };
    if !input.online {
        // Reconnect is the wake; nothing is read and nothing is timed.
        plan.actions.push(Action::Offline);
        return Ok(plan);
    }
    plan.refresh_remote = refresh_remote(input);

    // Index both inventories; a malformed or duplicated identity is refused
    // and takes no further part, so one bad row never poisons the queue.
    let mut local: BTreeMap<Identity, &LocalArtifact> = BTreeMap::new();
    let mut remote: BTreeMap<Identity, &RemoteHead> = BTreeMap::new();
    let mut refused: Vec<Action> = Vec::new();
    for artifact in &input.local {
        if !valid_identity(&artifact.identity) || !valid_sha256(&artifact.sha256) {
            refused.push(Action::Refused {
                identity: Some(artifact.identity.clone()),
                reason: "identity_malformed",
            });
            continue;
        }
        if local.insert(artifact.identity.clone(), artifact).is_some() {
            local.remove(&artifact.identity);
            refused.push(Action::Refused {
                identity: Some(artifact.identity.clone()),
                reason: "duplicate_identity",
            });
        }
    }
    for head in &input.remote {
        if !valid_identity(&head.identity)
            || !valid_sha256(&head.sha256)
            || head.revision.is_empty()
        {
            refused.push(Action::Refused {
                identity: Some(head.identity.clone()),
                reason: "identity_malformed",
            });
            continue;
        }
        if remote.insert(head.identity.clone(), head).is_some() {
            remote.remove(&head.identity);
            refused.push(Action::Refused {
                identity: Some(head.identity.clone()),
                reason: "duplicate_identity",
            });
        }
    }
    let refused_identities: BTreeSet<Identity> = refused
        .iter()
        .filter_map(|action| match action {
            Action::Refused { identity, .. } => identity.clone(),
            _ => None,
        })
        .collect();
    local.retain(|identity, _| !refused_identities.contains(identity));
    remote.retain(|identity, _| !refused_identities.contains(identity));

    let identities: BTreeSet<Identity> = local.keys().chain(remote.keys()).cloned().collect();
    let mut actions: Vec<Action> = Vec::new();
    let mut needed_releases: BTreeSet<&str> = BTreeSet::new();
    for identity in identities {
        match (local.get(&identity), remote.get(&identity)) {
            (Some(artifact), None) => {
                if !artifact.readable {
                    actions.push(Action::Refused {
                        identity: Some(identity.clone()),
                        reason: "local_unreadable",
                    });
                    continue;
                }
                needed_releases.insert(artifact.engine_release.as_str());
                actions.push(Action::Upload {
                    identity: identity.clone(),
                    sha256: artifact.sha256.clone(),
                    size_bytes: artifact.size_bytes,
                    reason: "absent_remotely",
                });
            }
            (None, Some(head)) => actions.push(Action::Download {
                identity: identity.clone(),
                revision: head.revision.clone(),
                sha256: head.sha256.clone(),
                reason: "absent_locally",
            }),
            (Some(artifact), Some(head)) => {
                if !artifact.readable {
                    // The bytes here cannot be trusted; the shared record is the truth.
                    actions.push(Action::Download {
                        identity: identity.clone(),
                        revision: head.revision.clone(),
                        sha256: head.sha256.clone(),
                        reason: "local_unreadable",
                    });
                } else if artifact.sha256 == head.sha256 {
                    actions.push(Action::Nothing {
                        identity: identity.clone(),
                        reason: "in_sync",
                    });
                } else if artifact.base_revision.as_deref() == Some(head.revision.as_str()) {
                    // Computed on the current head: a newer result, not a conflict.
                    needed_releases.insert(artifact.engine_release.as_str());
                    actions.push(Action::Upload {
                        identity: identity.clone(),
                        sha256: artifact.sha256.clone(),
                        size_bytes: artifact.size_bytes,
                        reason: "local_newer",
                    });
                } else {
                    // The record moved past what this result was computed on —
                    // or the result never saw a head at all. Never overwrite silently.
                    actions.push(Action::Conflict {
                        identity: identity.clone(),
                        base_revision: artifact.base_revision.clone(),
                        current_revision: head.revision.clone(),
                        reason: "base_moved",
                    });
                }
            }
            (None, None) => unreachable!("identity came from one of the inventories"),
        }
    }

    let uploads_pending = actions
        .iter()
        .any(|action| matches!(action, Action::Upload { .. }));
    let problem = grant_problem(input, &needed_releases);
    plan.grant_valid = problem.is_none();
    if uploads_pending {
        if let Some(reason) = problem {
            plan.actions.push(Action::OpenGrant { reason });
        }
    }
    plan.actions.extend(actions);
    plan.actions.extend(refused);

    for action in &plan.actions {
        match action {
            Action::Upload { .. } => plan.summary.uploads += 1,
            Action::Download { .. } => plan.summary.downloads += 1,
            Action::Conflict { .. } => plan.summary.conflicts += 1,
            Action::Refused { .. } => plan.summary.refused += 1,
            Action::Nothing { .. } => plan.summary.in_sync += 1,
            Action::OpenGrant { .. } | Action::Offline => {}
        }
    }
    // The only timed wake: renew a valid grant before an upload can race its
    // expiry. Everything else waits for a trigger.
    if let Some(grant) = &input.grant {
        if plan.grant_valid && uploads_pending {
            let renew_at = grant.expires_at_ms.saturating_sub(GRANT_RENEW_LEAD_MS);
            if renew_at > input.now_ms {
                plan.wake = Wake::At {
                    at_ms: renew_at,
                    reason: "grant_renewal",
                };
            }
        }
    }
    Ok(plan)
}

/// The closed entry point every host calls: bytes in, the plan out.
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > INPUT_BOUND {
        return Err("sync plan input exceeds its byte bound".into());
    }
    let input: Input = serde_json::from_slice(input)
        .map_err(|error| format!("invalid sync plan input: {error}"))?;
    let plan = plan(&input)?;
    serde_json::to_string(&json!(plan)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const REL: &str = "ds-network-reporter@0.1.0+0123456789abcdef0123456789abcdef01234567";

    fn ident(op: &str) -> Identity {
        Identity {
            engine: "network_reporter".into(),
            operation: op.into(),
            variant: "default".into(),
        }
    }
    fn local(op: &str, sha: &str, base: Option<&str>) -> LocalArtifact {
        LocalArtifact {
            identity: ident(op),
            sha256: sha.into(),
            size_bytes: 10,
            produced_at_ms: 1_000,
            base_revision: base.map(Into::into),
            readable: true,
            engine_release: REL.into(),
        }
    }
    fn remote(op: &str, sha: &str, revision: &str) -> RemoteHead {
        RemoteHead {
            identity: ident(op),
            revision: revision.into(),
            sha256: sha.into(),
            published_at_ms: 900,
        }
    }
    fn grant(expires_at_ms: u64) -> Grant {
        Grant {
            grant_id: "g1".into(),
            install_id: "install-1".into(),
            project: "p".into(),
            engines: vec![EngineRelease {
                name: "network_reporter".into(),
                release: REL.into(),
            }],
            expires_at_ms,
        }
    }
    fn input(local: Vec<LocalArtifact>, remote: Vec<RemoteHead>, grant: Option<Grant>) -> Input {
        Input {
            schema: SCHEMA.into(),
            project: "p".into(),
            install_id: "install-1".into(),
            now_ms: 10_000,
            online: true,
            trigger: Trigger::Navigation,
            remote_read_at_ms: Some(9_000),
            grant,
            local,
            remote,
        }
    }
    fn kinds(plan: &Plan) -> Vec<String> {
        plan.actions
            .iter()
            .map(|a| match a {
                Action::OpenGrant { reason } => format!("open_grant:{reason}"),
                Action::Upload {
                    identity, reason, ..
                } => format!("upload:{}:{reason}", identity.operation),
                Action::Download {
                    identity, reason, ..
                } => format!("download:{}:{reason}", identity.operation),
                Action::Conflict {
                    identity, reason, ..
                } => format!("conflict:{}:{reason}", identity.operation),
                Action::Nothing { identity, reason } => {
                    format!("nothing:{}:{reason}", identity.operation)
                }
                Action::Refused { identity, reason } => format!(
                    "refused:{}:{reason}",
                    identity
                        .as_ref()
                        .map(|i| i.operation.as_str())
                        .unwrap_or("-")
                ),
                Action::Offline => "offline".into(),
            })
            .collect()
    }

    #[test]
    fn offline_plans_nothing_and_waits_for_reconnect() {
        let p = plan(&Input {
            online: false,
            ..input(vec![local("a", SHA_A, None)], vec![], None)
        })
        .unwrap();
        assert_eq!(kinds(&p), vec!["offline"]);
        assert_eq!(p.wake, Wake::Event);
        assert!(!p.refresh_remote.needed);
    }

    #[test]
    fn local_only_uploads_under_a_valid_grant() {
        let p = plan(&input(
            vec![local("a", SHA_A, None)],
            vec![],
            Some(grant(10_000_000)),
        ))
        .unwrap();
        assert_eq!(kinds(&p), vec!["upload:a:absent_remotely"]);
        assert!(p.grant_valid);
        assert_eq!(p.summary.uploads, 1);
        assert_eq!(
            p.wake,
            Wake::At {
                at_ms: 10_000_000 - GRANT_RENEW_LEAD_MS,
                reason: "grant_renewal"
            }
        );
    }

    #[test]
    fn local_only_without_a_grant_opens_one_first() {
        let p = plan(&input(vec![local("a", SHA_A, None)], vec![], None)).unwrap();
        assert_eq!(
            kinds(&p),
            vec!["open_grant:no_grant", "upload:a:absent_remotely"]
        );
        assert!(!p.grant_valid);
    }

    #[test]
    fn an_expired_or_foreign_grant_is_renewed_before_uploading() {
        let p = plan(&input(
            vec![local("a", SHA_A, None)],
            vec![],
            Some(grant(10_000)),
        ))
        .unwrap();
        assert_eq!(kinds(&p)[0], "open_grant:grant_expired");
        let mut g = grant(10_000_000);
        g.install_id = "other".into();
        let p = plan(&input(vec![local("a", SHA_A, None)], vec![], Some(g))).unwrap();
        assert_eq!(kinds(&p)[0], "open_grant:grant_install_mismatch");
        let mut g = grant(10_000_000);
        g.engines.clear();
        let p = plan(&input(vec![local("a", SHA_A, None)], vec![], Some(g))).unwrap();
        assert_eq!(kinds(&p)[0], "open_grant:grant_engine_missing");
    }

    #[test]
    fn downloads_need_no_grant_and_read_only_survives_expiry() {
        let p = plan(&input(
            vec![],
            vec![remote("a", SHA_A, "r1")],
            Some(grant(10_000)),
        ))
        .unwrap();
        assert_eq!(kinds(&p), vec!["download:a:absent_locally"]);
        assert!(!p.grant_valid);
    }

    #[test]
    fn equal_digests_are_in_sync_and_idle() {
        let p = plan(&input(
            vec![local("a", SHA_A, Some("r1"))],
            vec![remote("a", SHA_A, "r1")],
            Some(grant(10_000_000)),
        ))
        .unwrap();
        assert_eq!(kinds(&p), vec!["nothing:a:in_sync"]);
        assert_eq!(p.wake, Wake::Event);
    }

    #[test]
    fn a_result_computed_on_the_current_head_uploads_as_newer() {
        let p = plan(&input(
            vec![local("a", SHA_B, Some("r1"))],
            vec![remote("a", SHA_A, "r1")],
            Some(grant(10_000_000)),
        ))
        .unwrap();
        assert_eq!(kinds(&p), vec!["upload:a:local_newer"]);
    }

    #[test]
    fn a_moved_or_unknown_base_is_a_conflict_never_an_overwrite() {
        let p = plan(&input(
            vec![local("a", SHA_B, Some("r0"))],
            vec![remote("a", SHA_A, "r1")],
            Some(grant(10_000_000)),
        ))
        .unwrap();
        assert_eq!(kinds(&p), vec!["conflict:a:base_moved"]);
        let p = plan(&input(
            vec![local("a", SHA_B, None)],
            vec![remote("a", SHA_A, "r1")],
            Some(grant(10_000_000)),
        ))
        .unwrap();
        assert_eq!(kinds(&p), vec!["conflict:a:base_moved"]);
        assert_eq!(p.summary.conflicts, 1);
        assert_eq!(p.summary.uploads, 0);
    }

    #[test]
    fn unreadable_local_bytes_never_upload() {
        let mut a = local("a", SHA_A, None);
        a.readable = false;
        let p = plan(&input(vec![a], vec![], Some(grant(10_000_000)))).unwrap();
        assert_eq!(kinds(&p), vec!["refused:a:local_unreadable"]);
        let mut a = local("a", SHA_A, Some("r1"));
        a.readable = false;
        let p = plan(&input(
            vec![a],
            vec![remote("a", SHA_B, "r1")],
            Some(grant(10_000_000)),
        ))
        .unwrap();
        assert_eq!(kinds(&p), vec!["download:a:local_unreadable"]);
    }

    #[test]
    fn one_bad_row_never_poisons_the_queue() {
        let mut bad = local("bad", SHA_A, None);
        bad.identity.operation = "-not-allowed".into();
        let dup1 = local("dup", SHA_A, None);
        let dup2 = local("dup", SHA_B, None);
        let p = plan(&input(
            vec![bad, dup1, dup2, local("ok", SHA_A, None)],
            vec![],
            Some(grant(10_000_000)),
        ))
        .unwrap();
        assert_eq!(
            kinds(&p),
            vec![
                "upload:ok:absent_remotely",
                "refused:-not-allowed:identity_malformed",
                "refused:dup:duplicate_identity"
            ]
        );
        assert_eq!(p.summary.refused, 2);
    }

    #[test]
    fn the_plan_is_ordered_by_identity_and_serialises_closed() {
        let p = plan(&input(
            vec![local("b", SHA_A, None), local("a", SHA_A, None)],
            vec![remote("c", SHA_A, "r1")],
            Some(grant(10_000_000)),
        ))
        .unwrap();
        assert_eq!(
            kinds(&p),
            vec![
                "upload:a:absent_remotely",
                "upload:b:absent_remotely",
                "download:c:absent_locally"
            ]
        );
        let text = evaluate(serde_json::to_string(&json!({
            "schema": SCHEMA, "project": "p", "install_id": "install-1", "now_ms": 10_000, "online": true, "trigger": "local_change",
            "local": [{"engine":"solar","operation":"calculate-x","sha256":SHA_A,"size_bytes":1,"produced_at_ms":1,"engine_release":"ds-solar-engine@0.1.0"}],
            "remote": []
        })).unwrap().as_bytes()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["schema"], SCHEMA);
        assert_eq!(value["actions"][0]["kind"], "open_grant");
        assert_eq!(value["actions"][1]["kind"], "upload");
        assert!(evaluate(br#"{"schema":"ds.sync-plan/v1","project":"p","install_id":"i","now_ms":1,"online":true,"extra":1}"#).is_err());
        assert!(evaluate(
            br#"{"schema":"other","project":"p","install_id":"i","now_ms":1,"online":true}"#
        )
        .is_err());
    }
}
