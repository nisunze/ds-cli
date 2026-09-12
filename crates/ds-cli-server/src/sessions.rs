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
//! One user per machine; many users are many machines. The Server is signed in
//! as exactly one owner and its bearer is that owner's; there is no second
//! identity to distinguish, so there is no per-principal anything here.
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
    ExecutionContext, Limits, NOT_FOUND, NOT_VISIBLE, PAYLOAD_CHANGED_FOR_KEY, PRINCIPAL_MISMATCH,
    PROJECT_REQUIRED, Refusal, SCOPE_MISMATCH, SCOPE_MISMATCH_FOR_KEY,
};
use ds_compute_runtime::{self as runtime, HostIdentity, SubmitError, digest};

use crate::{host::Connection, server_sync::ServerSyncSession};

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

    /// Admit one operation under the project it names, or refuse it by name.
    ///
    /// There is nothing to consult and nothing to fetch: the owner named the
    /// project (or its sealed bytes did), the kernel records it, and whether
    /// that project's effects may leave the machine is the gateway's answer at
    /// publication and sync.
    pub fn admit(&self, request: &Request<'_>) -> Result<ExecutionContext, Failure> {
        let fault = |fault| match fault {
            execution_context::Fault::Refused(refusal) => refusal_failure(&refusal),
            execution_context::Fault::Hard(message) => Failure::internal("server_refused", message)
                .remedy("report this: the Server built a malformed admission"),
        };
        // The project decides the durable id, so the kernel resolves it first
        // and answers the same way when it reads the three names again.
        let project =
            execution_context::project_for(request.requested_project, None, request.sealed_project)
                .map_err(fault)?;
        let job_id = runtime::job_id(
            &self.identity.owner,
            self.identity.lane(),
            &project,
            request.key,
        );
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
            existing: None,
        })
        .map(|admitted| admitted.context().clone())
        .map_err(fault)
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
    /// this host has: what its owner already asked it to do. It answers
    /// questions about this host's own work; it admits nothing and gates
    /// nothing.
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

/// One kernel refusal as the CLI's own typed failure, so `ds` re-raises the
/// code and the sentence it was given rather than a translation of them.
pub fn refusal_failure(refusal: &Refusal) -> Failure {
    let (class, remedy) = match refusal.code {
        PROJECT_REQUIRED => (
            ExitClass::InvalidInput,
            "pass --project <exact-id>, or select one with ds auth project use",
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
        // One sentence, everywhere this code is raised, and it is one the
        // owner can carry out: the row is readable and will never run, it has
        // no result to read, and `ds server input` is how its own request
        // bytes come back so they can be resubmitted as new work.
        CONTEXT_UNRECOVERABLE => (
            ExitClass::Conflict,
            "read the job's stored input with ds server input, then resubmit it under an explicit --project",
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
    fn the_projects_this_host_knows_are_the_ones_its_owner_handed_it_work_for() {
        // A queue with nothing in it names no project; once the owner has
        // handed the Server work for two projects those two are the answer,
        // read from the rows on disk with no upstream anywhere. It is an
        // answer about this host's own work, not a gate on anything.
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("store.sqlite");
        let sessions = sessions(database.clone());
        assert_eq!(sessions.durable_projects().unwrap(), Vec::<String>::new());
        let batch = |name: &str| -> Vec<u8> {
            serde_json::to_vec(&serde_json::json!({"schema":"ds.fast-lv.request/v1","jobs":[{"transformer_name":name,"gdfs":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr-1","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":name,"names":name}}]},"lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line-1","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]},"customers":{"type":"FeatureCollection","features":[]}},"settings":{}}]})).unwrap()
        };
        let now = 1_700_000_000_000;
        for (key, project) in [
            ("k-a", "project-a"),
            ("k-b", "project-b"),
            ("k-a2", "project-a"),
        ] {
            let client = sessions.client_label();
            let job = runtime::submit(
                &database,
                &runtime::Admission {
                    identity: sessions.identity(),
                    client: &client,
                    key,
                    requested_project: Some(project),
                    saved_project: None,
                    limits: sessions.limits(),
                    now_ms: now,
                },
                &batch(key),
            )
            .expect("admitted on the owner's word");
            assert_eq!(job.context.unwrap().project, project);
        }
        let mut projects = sessions.durable_projects().unwrap();
        projects.sort();
        assert_eq!(
            projects,
            vec!["project-a".to_owned(), "project-b".to_owned()]
        );
    }

    #[test]
    fn one_key_in_two_projects_is_two_ids_and_the_same_key_twice_is_one() {
        // The durable id digests the project, so the owner's daily key in two
        // projects is two pieces of work rather than a refusal or a collision.
        let sessions = sessions(PathBuf::from("/tmp/does-not-exist/store.sqlite"));
        let now = 1_700_000_000_000;
        let admit = |project: &str| {
            sessions
                .admit(&Request {
                    operation: "transformer_processing",
                    key: "daily",
                    input_sha256: digest(b"same bytes"),
                    requested_project: Some(project),
                    sealed_project: None,
                    client: "cli:1",
                    now_ms: now,
                })
                .expect("admitted")
        };
        let a = admit("project-a");
        let b = admit("project-b");
        assert_ne!(a.job_id, b.job_id);
        assert_eq!(a.idempotency_key, b.idempotency_key);
        assert_eq!(admit("project-a").job_id, a.job_id);
    }
}
