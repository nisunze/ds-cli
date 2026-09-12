//! One Server, one authenticated owner, many of that owner's projects.
//!
//! Before this slice the Server captured `ds auth project use`'s saved
//! selection once, at `serve`, and every operation inherited it: one project
//! per process, and a second project meant a restart. That is the thing this
//! module removes. What the Server holds now is a connection — an account, a
//! lane, a deployment and an install — and per operation it asks the kernel
//! to record which project the operation is about. A Sync Center session is
//! opened lazily for each admitted project and cached under (principal,
//! project), so Solar for one project, a report for a second and a layer read
//! for a third are three contexts on one host.
//!
//! The Server is the desktop's core and stands on the desktop's side of the
//! one boundary with ds-brain: it holds **no project directory**, fetches
//! none, caches none and refreshes none to admit work. The owner hands it work
//! that names a project (a sealed Solar input's project outranks the name),
//! the kernel records the immutable context, and entitlement is enforced by
//! the gateway where an effect crosses to the cloud — publication and sync —
//! exactly as the desktop behaves. Admission, queueing, execution and recovery
//! therefore need no upstream at all.
//!
//! Nothing here decides. `execution_context::admit` decides; this module
//! hands it the connection identity and the named project, and relays the
//! refusal by its own name.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use ds_cli_contract::{Failure, outcome::ExitClass};
use ds_command_kernel::execution_context::{
    self, AdmitRequest, CAPACITY_EXHAUSTED, CONTEXT_CORRUPT, CONTEXT_UNRECOVERABLE,
    ExecutionContext, Limits, MAX_MEMBERSHIP_TTL_MS, Membership, NOT_FOUND, NOT_VISIBLE,
    PAYLOAD_CHANGED_FOR_KEY, PRINCIPAL_MISMATCH, PROJECT_NOT_VISIBLE, PROJECT_REQUIRED, Refusal,
    SCOPE_MISMATCH, SCOPE_MISMATCH_FOR_KEY,
};
use ds_compute_runtime::{self as runtime, HostIdentity, MembershipSource, SubmitError, digest};

use crate::{host::Connection, server_sync::ServerSyncSession};

/// One Server serves one authenticated owner. A request that names another
/// one is told so, explicitly, instead of being quietly served under the
/// Server's account. Many users are many machines, never one process.
pub const MULTI_PRINCIPAL_UNSUPPORTED: &str = "multi_principal_unsupported";

/// How a per-project Sync Center session is opened. Production opens the
/// native gateway session; a test opens nothing and says so.
pub trait SessionOpener: Send + Sync + 'static {
    fn open(
        &self,
        database: &Path,
        connection: &Connection,
        project: &str,
    ) -> Result<Arc<ServerSyncSession>, String>;
}

struct NativeOpener;
impl SessionOpener for NativeOpener {
    fn open(
        &self,
        database: &Path,
        connection: &Connection,
        project: &str,
    ) -> Result<Arc<ServerSyncSession>, String> {
        ServerSyncSession::open(database, connection, project).map(Arc::new)
    }
}

/// What one operation is asking to be admitted for.
pub struct Request<'a> {
    pub operation: &'a str,
    /// The caller's own idempotency key for a submission, or a read's request
    /// id. A read's key carries a `:` so it can never collide with a caller
    /// key, which is letters, digits, `_` and `-` only.
    pub key: &'a str,
    /// The digest of the exact bytes this operation is about.
    pub input_sha256: String,
    pub requested_project: Option<&'a str>,
    /// The project the sealed input itself names, which outranks the request.
    pub sealed_project: Option<&'a str>,
    pub client: &'a str,
    pub now_ms: u64,
}

pub struct ServerSessions {
    connection: Connection,
    database: PathBuf,
    limits: Limits,
    identity: HostIdentity,
    opener: Arc<dyn SessionOpener>,
    sessions: Mutex<BTreeMap<(String, String), Arc<ServerSyncSession>>>,
    reads: AtomicU64,
}

impl ServerSessions {
    /// The production binding: the connection's own account, lane, deployment
    /// and registered install, with no project anywhere in it. It reads the
    /// protected native state and nothing else — no network, no directory,
    /// no saved selection — so `ds server serve` starts for an account that
    /// has never run `ds auth project use`, and starts with no upstream.
    pub fn native(
        connection: Connection,
        database: PathBuf,
        limits: Limits,
    ) -> Result<Arc<Self>, String> {
        let principal = ds_cli_auth::headless_principal(&connection.lane)
            .map_err(|error| error.message().to_owned())?;
        let identity = HostIdentity {
            owner: connection.owner.clone(),
            principal: execution_context::Principal {
                uid: principal.account_uid().to_owned(),
                lane: connection.lane.clone(),
                deployment: principal.deployment().to_owned(),
                install_id: principal.install_id().to_owned(),
            },
        };
        Ok(Self::with(
            connection,
            database,
            limits,
            identity,
            Arc::new(NativeOpener),
        ))
    }

    pub fn with(
        connection: Connection,
        database: PathBuf,
        limits: Limits,
        identity: HostIdentity,
        opener: Arc<dyn SessionOpener>,
    ) -> Arc<Self> {
        Arc::new(Self {
            connection,
            database,
            limits,
            identity,
            opener,
            sessions: Mutex::new(BTreeMap::new()),
            reads: AtomicU64::new(0),
        })
    }

    pub const fn identity(&self) -> &HostIdentity {
        &self.identity
    }
    pub const fn limits(&self) -> Limits {
        self.limits
    }
    pub fn principal_uid(&self) -> &str {
        &self.identity.principal.uid
    }

    /// The kernel's membership input for one admission, satisfied by exactly
    /// the projects the request names. The Server holds no directory: the
    /// owner of this connection is entitled to hand it work for any project,
    /// and the gateway — not this host — decides at publication and sync
    /// whether that project's effects may leave the machine. Stamped now and
    /// given the kernel's own maximum life, so no freshness rule can refuse
    /// a name the owner just typed.
    pub fn named<'a>(
        &self,
        now_ms: u64,
        projects: impl IntoIterator<Item = Option<&'a str>>,
    ) -> Membership {
        let mut named: Vec<String> = Vec::new();
        for project in projects.into_iter().flatten() {
            if !named.iter().any(|known| known == project) {
                named.push(project.to_owned());
            }
        }
        Membership {
            projects: named,
            fetched_at_ms: now_ms,
            ttl_ms: MAX_MEMBERSHIP_TTL_MS,
        }
    }

    /// Admit one operation under the project it names, or refuse it by name.
    pub fn admit(&self, request: &Request<'_>) -> Result<ExecutionContext, Failure> {
        let job_id = runtime::job_id(&self.identity.owner, self.identity.lane(), request.key);
        execution_context::admit(&AdmitRequest {
            now_ms: request.now_ms,
            principal: self.identity.principal.clone(),
            client: request.client.to_owned(),
            operation: request.operation.to_owned(),
            job_id,
            idempotency_key: request.key.to_owned(),
            input_sha256: request.input_sha256.clone(),
            requested_project: request.requested_project.map(str::to_owned),
            saved_project: None,
            sealed_project: request.sealed_project.map(str::to_owned),
            membership: self.named(
                request.now_ms,
                [request.requested_project, request.sealed_project],
            ),
            existing: None,
        })
        .map(|admitted| admitted.context().clone())
        .map_err(|fault| match fault {
            execution_context::Fault::Refused(refusal) => refusal_failure(&refusal),
            execution_context::Fault::Hard(message) => Failure::internal("server_refused", message)
                .remedy("report this: the Server built a malformed admission"),
        })
    }

    /// A request id for an operation that has no durable job: a layer read or
    /// write. The `:` keeps it out of the caller-key space for good.
    pub fn read_key(&self, operation: &str) -> String {
        format!(
            "read:{operation}:{}:{}",
            std::process::id(),
            self.reads.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// Admit one read or write that carries no durable job.
    pub fn admit_read(
        &self,
        operation: &str,
        requested_project: Option<&str>,
        about: &[u8],
        now_ms: u64,
    ) -> Result<ExecutionContext, Failure> {
        self.admit(&Request {
            operation,
            key: &self.read_key(operation),
            input_sha256: digest(about),
            requested_project,
            sealed_project: None,
            client: &self.client_label(),
            now_ms,
        })
    }

    /// How this connection names itself in a context. The connection token is
    /// never any part of it; the address it listens on is enough to tell two
    /// Servers on one machine apart.
    pub fn client_label(&self) -> String {
        format!("server-connection:{}", self.connection.address)
    }

    /// The Sync Center session for one admitted project, opened on first use
    /// and kept for the life of the Server. A project's session is its own:
    /// its rows, lease and grants are fenced by (account, deployment,
    /// install, project), and one project's failure never touches another's.
    pub fn session(&self, project: &str) -> Result<Arc<ServerSyncSession>, String> {
        let key = (self.principal_uid().to_owned(), project.to_owned());
        if let Some(session) = self
            .sessions
            .lock()
            .map_err(|_| "server sessions are unavailable".to_string())?
            .get(&key)
        {
            return Ok(session.clone());
        }
        let session = self
            .opener
            .open(&self.database, &self.connection, project)?;
        let mut held = self
            .sessions
            .lock()
            .map_err(|_| "server sessions are unavailable".to_string())?;
        Ok(held.entry(key).or_insert(session).clone())
    }

    /// The projects this Server already holds a session for, in id order.
    pub fn open_projects(&self) -> Vec<String> {
        self.sessions
            .lock()
            .map(|held| held.keys().map(|(_, project)| project.clone()).collect())
            .unwrap_or_default()
    }

    /// Every project this owner has handed this Server durable work for, read
    /// from the rows themselves. Local, unnarrowed, and the only "directory"
    /// this host has: what its owner already asked it to do.
    pub fn durable_projects(&self) -> Result<Vec<String>, String> {
        let store = runtime::open(&self.database)?;
        let caller = self.identity.caller(None);
        let mut cursor: Option<(u64, String)> = None;
        let mut projects: Vec<String> = Vec::new();
        loop {
            let page = store
                .jobs_page(
                    &caller,
                    cursor.as_ref().map(|(created, id)| (*created, id.as_str())),
                    1000,
                )
                .map_err(|error| error.to_string())?;
            let Some(last) = page.last() else { break };
            cursor = Some((last.created_at_ms, last.id.clone()));
            for project in page
                .into_iter()
                .filter_map(|job| job.context.map(|c| c.project))
            {
                if !projects.contains(&project) {
                    projects.push(project);
                }
            }
        }
        Ok(projects)
    }
}

/// The runtime asks, before it executes a claimed job and before it projects
/// a completion, whether the job's project is still one this host may act
/// in. On the Server the answer is the owner's own durable work: a project
/// the owner handed work for is one the owner may run — a revocation is a
/// gateway answer on a publication, never a decision made here. No network
/// is touched, so an outage cannot fail a job and a job cannot be failed by
/// anything but its own execution.
impl MembershipSource for ServerSessions {
    fn snapshot(&self, now_ms: u64) -> Result<Membership, String> {
        Ok(Membership {
            projects: self.durable_projects()?,
            fetched_at_ms: now_ms,
            ttl_ms: MAX_MEMBERSHIP_TTL_MS,
        })
    }
}

/// One kernel refusal as the CLI's own typed failure, so `ds` re-raises the
/// code and the sentence it was given rather than a translation of them.
pub fn refusal_failure(refusal: &Refusal) -> Failure {
    let (class, remedy) = match refusal.code {
        PROJECT_REQUIRED => (
            ExitClass::InvalidInput,
            "pass --project <exact-id>, or select one with ds auth project use",
        ),
        PROJECT_NOT_VISIBLE => (
            ExitClass::InvalidInput,
            "run ds auth project list and pass one exact ds_project value",
        ),
        SCOPE_MISMATCH => (
            ExitClass::Conflict,
            "submit the input under the project it was prepared for, or prepare it again",
        ),
        SCOPE_MISMATCH_FOR_KEY => (
            ExitClass::Conflict,
            "use a new --key for work in another project",
        ),
        PAYLOAD_CHANGED_FOR_KEY => (
            ExitClass::Conflict,
            "use a new --key for changed input bytes",
        ),
        PRINCIPAL_MISMATCH => (
            ExitClass::Conflict,
            "sign in under the account that submitted this work",
        ),
        NOT_VISIBLE => (
            ExitClass::Conflict,
            "check the job id and the project it belongs to",
        ),
        CAPACITY_EXHAUSTED => (
            ExitClass::Unavailable,
            "retry after the stated delay, or start the server with more workers",
        ),
        CONTEXT_UNRECOVERABLE => (
            ExitClass::Conflict,
            "select a project with ds auth project use and restart the server so the retained work can be recovered",
        ),
        CONTEXT_CORRUPT => (
            ExitClass::InvalidInput,
            "shorten the value the server refused and repeat the request",
        ),
        _ => (ExitClass::Failed, "read the stated reason and retry"),
    };
    let failure = Failure::new(class, refusal.code, refusal.message.clone()).remedy(remedy);
    match (refusal.retry_after_ms, refusal.scope) {
        (Some(retry_after_ms), scope) => failure.detail(serde_json::json!({
            "retry_after_ms": retry_after_ms,
            "scope": scope,
        })),
        (None, _) => failure,
    }
}

/// A submission's outcome as a typed failure: the kernel's refusals keep
/// their names, the host's own faults do not pretend to be the caller's.
pub fn submit_failure(error: &SubmitError) -> Failure {
    match error {
        SubmitError::Refused(refusal) => refusal_failure(refusal),
        SubmitError::Invalid(message) => Failure::invalid("server_refused", message.clone())
            .remedy("send the documented request bytes"),
        SubmitError::Host(message) => Failure::failed("server_refused", message.clone())
            .remedy("verify native authentication, protected server state and ds server serve"),
    }
}

/// The one sentence every invisible job gets, whatever the reason.
pub fn not_found() -> Failure {
    refusal_failure(&Refusal {
        code: NOT_VISIBLE,
        message: NOT_FOUND.to_owned(),
        retry_after_ms: None,
        scope: None,
    })
}

/// A request that names a principal other than the one this Server is signed
/// in as. One Server serves one account and many of its projects; a second
/// account needs a second `ds server serve` with its own state directory.
pub fn multi_principal() -> Failure {
    Failure::unauthorized(
        MULTI_PRINCIPAL_UNSUPPORTED,
        "this server serves one authenticated account, and it is not the one this request names",
    )
    .remedy("run a second ds server serve under that account with its own --listen and --state-dir")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoSessions;
    impl SessionOpener for NoSessions {
        fn open(
            &self,
            _: &Path,
            _: &Connection,
            _: &str,
        ) -> Result<Arc<ServerSyncSession>, String> {
            Err("no gateway session in this test".into())
        }
    }

    fn sessions(database: PathBuf) -> Arc<ServerSessions> {
        let identity = HostIdentity {
            owner: "owner-digest".into(),
            principal: execution_context::Principal {
                uid: "uid-a".into(),
                lane: "canary".into(),
                deployment: "https://gateway.example".into(),
                install_id: "install-1".into(),
            },
        };
        ServerSessions::with(
            Connection {
                address: "127.0.0.1:19766".parse().unwrap(),
                owner: "owner-digest".into(),
                lane: "canary".into(),
                token: "a".repeat(64),
            },
            database,
            Limits {
                global_running: 2,
                per_project_running: 1,
                per_project_queued: 4,
                global_queued: 8,
            },
            identity,
            Arc::new(NoSessions),
        )
    }

    #[test]
    fn a_project_named_for_the_first_time_is_admitted_with_no_directory_anywhere() {
        // Nothing lists project-a or project-b beforehand: there is no
        // directory, no snapshot and no upstream. The owner names them, and
        // that is the whole of what the Server needs.
        let sessions = sessions(PathBuf::from("/tmp/does-not-exist/store.sqlite"));
        let now = 1_700_000_000_000;
        for project in ["project-a", "project-b", "a-project-never-seen"] {
            let context = sessions
                .admit_read("layer_read", Some(project), b"list", now)
                .expect("the owner's word is enough");
            assert_eq!(context.project, project);
            assert_eq!(context.principal_uid, "uid-a");
        }
        assert!(
            sessions.open_projects().is_empty(),
            "admission opens no session"
        );
    }

    #[test]
    fn the_kernels_membership_input_is_exactly_what_the_request_names() {
        let sessions = sessions(PathBuf::from("/tmp/does-not-exist/store.sqlite"));
        let named = sessions.named(5, [Some("project-a"), None, Some("project-a"), Some("b")]);
        assert_eq!(named.projects, vec!["project-a".to_owned(), "b".to_owned()]);
        assert_eq!(named.fetched_at_ms, 5);
        assert_eq!(named.ttl_ms, MAX_MEMBERSHIP_TTL_MS);
        assert!(sessions.named(5, [None, None]).projects.is_empty());
    }

    #[test]
    fn a_read_carries_a_context_of_its_own_and_never_collides_with_a_caller_key() {
        let sessions = sessions(PathBuf::from("/tmp/does-not-exist/store.sqlite"));
        let context = sessions
            .admit_read(
                "layer_write",
                Some("project-a"),
                b"hide poles",
                1_700_000_000_000,
            )
            .expect("admitted");
        assert_eq!(context.operation, "layer_write");
        assert_eq!(context.project, "project-a");
        assert!(context.idempotency_key.starts_with("read:layer_write:"));
        assert!(
            context.idempotency_key.contains(':'),
            "a caller key is letters, digits, _ and - only, so this can never be one"
        );
        assert_eq!(context.client, sessions.client_label());
        let second = sessions
            .admit_read(
                "layer_write",
                Some("project-a"),
                b"hide poles",
                1_700_000_000_000,
            )
            .expect("admitted");
        assert_ne!(
            context.idempotency_key, second.idempotency_key,
            "two reads are two operations, never an idempotent pair"
        );
    }

    #[test]
    fn an_unnamed_project_is_required_and_an_unbounded_one_is_named_not_guessed() {
        let sessions = sessions(PathBuf::from("/tmp/does-not-exist/store.sqlite"));
        let now = 1_700_000_000_000;
        assert_eq!(
            sessions
                .admit_read("layer_read", None, b"list", now)
                .expect_err("no project")
                .code(),
            "project_required"
        );
        let padded = sessions
            .admit_read("layer_read", Some(" padded"), b"list", now)
            .expect_err("not a bounded project id");
        assert_eq!(padded.code(), "context_corrupt");
        assert_eq!(padded.class(), ExitClass::InvalidInput);
    }

    #[test]
    fn the_workers_source_is_the_owners_own_durable_work_and_touches_no_network() {
        // A queue with nothing in it names no project; once the owner has
        // handed the Server work for two projects — through the runtime's own
        // admission, with the Server's own membership input — those two are
        // the answer, read from the rows on disk with no upstream anywhere.
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("store.sqlite");
        let sessions = sessions(database.clone());
        assert_eq!(sessions.snapshot(1).unwrap().projects, Vec::<String>::new());
        let batch = |name: &str| -> Vec<u8> {
            serde_json::to_vec(&serde_json::json!({"schema":"ds.fast-lv.request/v1","jobs":[{"transformer_name":name,"gdfs":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr-1","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":name,"names":name}}]},"lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line-1","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]},"customers":{"type":"FeatureCollection","features":[]}},"settings":{}}]})).unwrap()
        };
        let now = 1_700_000_000_000;
        for (key, project) in [
            ("k-a", "project-a"),
            ("k-b", "project-b"),
            ("k-a2", "project-a"),
        ] {
            let membership = sessions.named(now, [Some(project)]);
            let client = sessions.client_label();
            let job = runtime::submit(
                &database,
                &runtime::Admission {
                    identity: sessions.identity(),
                    client: &client,
                    key,
                    requested_project: Some(project),
                    saved_project: None,
                    membership: &membership,
                    limits: sessions.limits(),
                    now_ms: now,
                },
                &batch(key),
            )
            .expect("admitted on the owner's word");
            assert_eq!(job.context.unwrap().project, project);
        }
        let snapshot = sessions.snapshot(now + 1).unwrap();
        let mut projects = snapshot.projects.clone();
        projects.sort();
        assert_eq!(
            projects,
            vec!["project-a".to_owned(), "project-b".to_owned()]
        );
        // And every job the runtime would re-check still holds, because the
        // job's own row is what the answer is made of.
        for project in ["project-a", "project-b"] {
            let context = sessions
                .admit_read("layer_read", Some(project), b"x", now + 1)
                .unwrap();
            assert!(runtime::still_admitted(&context, &snapshot, now + 2).is_ok());
        }
    }
}
