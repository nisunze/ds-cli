//! One Server, one authenticated principal, many authorized projects.
//!
//! Before this slice the Server captured `ds auth project use`'s saved
//! selection once, at `serve`, and every operation inherited it: one project
//! per process, and a second project meant a restart. That is the thing this
//! module removes. What the Server holds now is a connection — an account, a
//! lane, a deployment and an install — and per-operation it asks the kernel
//! which project the operation is about, from a freshly fetched membership
//! snapshot. A Sync Center session is opened lazily for each admitted project
//! and cached under (principal, project), so Solar for one project, a report
//! for a second and a layer read for a third are three contexts on one host.
//!
//! Nothing here decides. `execution_context::admit` decides; this module
//! fetches the membership it decides from, caches it for a bounded time,
//! refetches it once when a decision made from a cached snapshot was refused,
//! and relays the refusal by its own name.

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
    ExecutionContext, Limits, Membership, NOT_FOUND, NOT_VISIBLE, PAYLOAD_CHANGED_FOR_KEY,
    PRINCIPAL_MISMATCH, PROJECT_NOT_VISIBLE, PROJECT_REQUIRED, Refusal, SCOPE_MISMATCH,
    SCOPE_MISMATCH_FOR_KEY,
};
use ds_compute_runtime::{HostIdentity, MembershipSource, SubmitError, digest};

use crate::{host::Connection, server_sync::ServerSyncSession};

/// How long a membership snapshot may be reused before it is fetched again.
/// The kernel enforces it too — the snapshot carries the same `ttl_ms` — so a
/// host that stopped refreshing cannot admit from a stale one either.
pub const MEMBERSHIP_TTL_MS: u64 = 10 * 60 * 1_000;

/// One Server serves one authenticated principal. A request that names
/// another one is told so, explicitly, instead of being quietly served under
/// the Server's account.
pub const MULTI_PRINCIPAL_UNSUPPORTED: &str = "multi_principal_unsupported";

/// The projects this native account may act in, freshly fetched — the same
/// directory `ds auth project use` verifies a selection against.
pub trait ProjectDirectory: Send + Sync + 'static {
    fn projects(&self) -> Result<Vec<String>, Failure>;
}

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

struct NativeDirectory {
    lane: String,
}
impl ProjectDirectory for NativeDirectory {
    fn projects(&self) -> Result<Vec<String>, Failure> {
        ds_cli_auth::headless_projects(&self.lane)
    }
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
    directory: Arc<dyn ProjectDirectory>,
    opener: Arc<dyn SessionOpener>,
    membership: Mutex<Option<Membership>>,
    sessions: Mutex<BTreeMap<(String, String), Arc<ServerSyncSession>>>,
    reads: AtomicU64,
}

impl ServerSessions {
    /// The production binding: the connection's own account, lane, deployment
    /// and registered install, with no project anywhere in it. This performs
    /// no network call and requires no saved selection, so `ds server serve`
    /// starts for an account that has never run `ds auth project use`.
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
        let directory = Arc::new(NativeDirectory {
            lane: connection.lane.clone(),
        });
        Ok(Self::with(
            connection,
            database,
            limits,
            identity,
            directory,
            Arc::new(NativeOpener),
        ))
    }

    pub fn with(
        connection: Connection,
        database: PathBuf,
        limits: Limits,
        identity: HostIdentity,
        directory: Arc<dyn ProjectDirectory>,
        opener: Arc<dyn SessionOpener>,
    ) -> Arc<Self> {
        Arc::new(Self {
            connection,
            database,
            limits,
            identity,
            directory,
            opener,
            membership: Mutex::new(None),
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

    /// A membership snapshot, fetched at most every [`MEMBERSHIP_TTL_MS`].
    pub fn membership(&self, now_ms: u64) -> Result<Membership, Failure> {
        let cached = {
            let held = self.membership.lock().map_err(|_| unavailable())?;
            held.clone()
        };
        if let Some(snapshot) = cached
            && now_ms >= snapshot.fetched_at_ms
            && now_ms - snapshot.fetched_at_ms < MEMBERSHIP_TTL_MS
        {
            return Ok(snapshot);
        }
        self.fetch_membership(now_ms)
    }

    fn fetch_membership(&self, now_ms: u64) -> Result<Membership, Failure> {
        let snapshot = Membership {
            projects: self.directory.projects()?,
            fetched_at_ms: now_ms,
            ttl_ms: MEMBERSHIP_TTL_MS,
        };
        *self.membership.lock().map_err(|_| unavailable())? = Some(snapshot.clone());
        Ok(snapshot)
    }

    /// A snapshot fetched now, whatever the cache said. Used on the second
    /// try after a refusal, so a project granted a moment ago is admitted on
    /// the call that names it rather than on the one after.
    pub fn membership_now(&self, now_ms: u64) -> Result<Membership, Failure> {
        self.fetch_membership(now_ms)
    }

    /// Forget the cached snapshot; the next question fetches a fresh one.
    pub fn forget_membership(&self) {
        if let Ok(mut held) = self.membership.lock() {
            *held = None;
        }
    }

    /// Admit one operation under one verified project, or refuse it by name.
    ///
    /// A refusal decided from a *cached* snapshot is retried once against a
    /// freshly fetched one — a project added a minute ago must not have to
    /// wait out the cache — and a refusal that survives that is the answer.
    pub fn admit(&self, request: &Request<'_>) -> Result<ExecutionContext, Failure> {
        let cached = self
            .membership
            .lock()
            .ok()
            .and_then(|held| held.clone())
            .is_some();
        let membership = self.membership(request.now_ms)?;
        match self.decide(request, &membership) {
            Ok(context) => Ok(context),
            Err(failure) if cached && failure.code() == PROJECT_NOT_VISIBLE => {
                let fresh = self.fetch_membership(request.now_ms)?;
                self.decide(request, &fresh)
            }
            Err(failure) => Err(failure),
        }
    }

    fn decide(
        &self,
        request: &Request<'_>,
        membership: &Membership,
    ) -> Result<ExecutionContext, Failure> {
        let job_id =
            ds_compute_runtime::job_id(&self.identity.owner, self.identity.lane(), request.key);
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
            membership: membership.clone(),
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
}

impl MembershipSource for ServerSessions {
    fn snapshot(&self, now_ms: u64) -> Result<Membership, String> {
        self.membership(now_ms)
            .map_err(|failure| failure.message().to_owned())
    }
    fn invalidate(&self) {
        self.forget_membership();
    }
}

fn unavailable() -> Failure {
    Failure::internal(
        "server_refused",
        "the server's membership state is unavailable",
    )
    .remedy("restart ds server serve")
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

    struct Fixture(Mutex<Vec<String>>);
    impl ProjectDirectory for Fixture {
        fn projects(&self) -> Result<Vec<String>, Failure> {
            Ok(self.0.lock().unwrap().clone())
        }
    }
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

    fn sessions(projects: &[&str]) -> (Arc<ServerSessions>, Arc<Fixture>) {
        let directory = Arc::new(Fixture(Mutex::new(
            projects.iter().map(|p| (*p).to_owned()).collect(),
        )));
        let identity = HostIdentity {
            owner: "owner-digest".into(),
            principal: execution_context::Principal {
                uid: "uid-a".into(),
                lane: "canary".into(),
                deployment: "https://gateway.example".into(),
                install_id: "install-1".into(),
            },
        };
        (
            ServerSessions::with(
                Connection {
                    address: "127.0.0.1:19766".parse().unwrap(),
                    owner: "owner-digest".into(),
                    lane: "canary".into(),
                    token: "a".repeat(64),
                },
                PathBuf::from("/tmp/does-not-exist/store.sqlite"),
                Limits {
                    global_running: 2,
                    per_project_running: 1,
                    per_project_queued: 4,
                    global_queued: 8,
                },
                identity,
                directory.clone(),
                Arc::new(NoSessions),
            ),
            directory,
        )
    }

    #[test]
    fn a_project_added_after_the_snapshot_is_admitted_without_waiting_out_the_cache() {
        let (sessions, directory) = sessions(&["project-a"]);
        let now = 1_700_000_000_000;
        sessions
            .admit_read("layer_read", Some("project-a"), b"list", now)
            .expect("a member project");
        let refused = sessions
            .admit_read("layer_read", Some("project-b"), b"list", now)
            .expect_err("not a member yet");
        assert_eq!(refused.code(), "project_not_visible");
        directory.0.lock().unwrap().push("project-b".into());
        // Same millisecond, cached snapshot: the refusal refetches once.
        sessions
            .admit_read("layer_read", Some("project-b"), b"list", now)
            .expect("membership was refetched on the refusal");
    }

    #[test]
    fn a_read_carries_a_context_of_its_own_and_never_collides_with_a_caller_key() {
        let (sessions, _) = sessions(&["project-a"]);
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
    fn an_unnamed_project_is_required_and_a_stale_snapshot_says_what_a_stranger_hears() {
        let (sessions, _) = sessions(&["project-a"]);
        let now = 1_700_000_000_000;
        assert_eq!(
            sessions
                .admit_read("layer_read", None, b"list", now)
                .expect_err("no project")
                .code(),
            "project_required"
        );
        let stranger = sessions
            .admit_read("layer_read", Some("project-z"), b"list", now)
            .expect_err("not a member");
        // Push the cached snapshot beyond its life and ask about a project
        // that IS a member: the sentence is the stranger's, exactly.
        let expired = sessions
            .admit_read(
                "layer_read",
                Some("project-a"),
                b"list",
                now + MEMBERSHIP_TTL_MS * 3,
            )
            .map(|context| context.project);
        assert!(expired.is_ok(), "a refetch makes it visible again");
        let mut held = sessions.membership.lock().unwrap();
        *held = Some(Membership {
            projects: vec!["project-a".into()],
            fetched_at_ms: 1,
            ttl_ms: 1,
        });
        drop(held);
        let stale = sessions
            .decide(
                &Request {
                    operation: "layer_read",
                    key: "read:stale",
                    input_sha256: digest(b"list"),
                    requested_project: Some("project-a"),
                    sealed_project: None,
                    client: "test",
                    now_ms: now,
                },
                &Membership {
                    projects: vec!["project-a".into()],
                    fetched_at_ms: 1,
                    ttl_ms: 1,
                },
            )
            .expect_err("the snapshot expired");
        assert_eq!(stale.code(), stranger.code());
        assert_eq!(stale.message(), stranger.message());
    }
}
