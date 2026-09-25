//! The Server's HTTP surface: the owner-only door onto one authenticated
//! account's durable compute, publication and layer state.
//!
//! Every route that acts asks `ServerSessions` to admit it, and the kernel
//! decides which project the operation is about. Every route that reads
//! answers through the job's own execution context, so a job in a project the
//! caller did not name is absent in exactly the way an invented id is absent.
//! The door is `crate::transport`: one owner-only Unix socket in the protected
//! state directory, a peer the kernel names as this Server's own account, and
//! one re-authorized native account behind it. No bearer and no port.

use axum::extract::{Query, Request};
use axum::middleware::{self, Next};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path as Param, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use ds_cli_contract::{Failure, outcome::ExitClass};
use ds_command_kernel::compute_jobs::Event;
use ds_command_kernel::execution_context::CAPACITY_EXHAUSTED;
use ds_compute_runtime::{self as runtime, Admission, Authorizer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::server_sync::sessions::{self, ServerSessions};

/// The activity envelope every `/v1/activity` answer carries, one entry per
/// project. One shape whether the caller narrowed or not.
pub const ACTIVITY_SCHEMA: &str = "ds.server-activity/v1";
/// The typed refusal for an operation that genuinely needs a rendered map.
/// The operation keeps its id and its shape; only this host cannot run it.
pub const NEEDS_PAIRED_MAP: &str = "needs_paired_map";

/// The one Server a protected state directory holds: whose it is, on which
/// lane, and the socket it answers on.
///
/// `connection.json` records the owner and the lane and nothing that opens
/// the door — there is no bearer to keep, because the door asks the kernel
/// who is knocking. The socket is not recorded either: it is always
/// `<state>/server.sock`.
#[derive(Clone, Debug)]
pub struct Connection {
    pub owner: String,
    pub lane: String,
    /// `<state>/server.sock`, the Server's only door.
    pub socket: PathBuf,
    /// Set only when `connection.json` was written by a `ds` from before the
    /// socket: that Server listened on this TCP loopback address and admitted
    /// a bearer. Such a record identifies its owner and lane and is never a
    /// transport — a client refuses it by name and sends nothing, and the
    /// next `ds server serve` rewrites it without the bearer.
    pub legacy_address: Option<SocketAddr>,
}

/// `connection.json` as this build writes it.
pub const CONNECTION_SCHEMA: &str = "ds.server-connection/v2";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    owner: String,
    lane: String,
}

/// `connection.json` as a `ds` from before the socket wrote it. Read only to
/// learn its owner, its lane and where it listened; the bearer is skipped
/// unread and never leaves this parse.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyRecord {
    address: SocketAddr,
    owner: String,
    lane: String,
    #[allow(dead_code)]
    token: serde::de::IgnoredAny,
}
#[derive(Clone)]
pub struct App {
    pub database: PathBuf,
    pub connection: Connection,
    /// The operating-system account this host runs as: the only peer its
    /// socket admits (`transport::own_uid()`), or `None` where there is no
    /// socket and so no peer to admit.
    pub os_uid: Option<u32>,
    pub auth: Arc<dyn Authorizer>,
    /// How many requests this host answers at once, and what it says when it
    /// is full.
    pub requests: Arc<Door>,
    pub activity: Option<Arc<crate::solar_sync::SolarActivity>>,
    /// The layer drawer's document source and preference root for this host.
    pub layers: Arc<dyn crate::layers::LayerHost>,
    /// One authenticated owner, many of its projects: the admission door and
    /// the per-project sessions. No directory lives here.
    pub sessions: Arc<ServerSessions>,
    pub solar: Arc<crate::solar_application::Applications>,
}

type ApiError = (StatusCode, Json<Value>);

/// A quarter second per request already inside the door.
///
/// The same shape as the kernel's own capacity guidance — a base per unit of
/// work that must go first — with the base a door request deserves: what a
/// request holds here is a store read, a digest or one project's document
/// fetch, not a worker running an engine.
const DOOR_RETRY_BASE_MS: u64 = 250;
/// The longest a caller is ever told to wait for the door, matching the
/// kernel's own ceiling on retry guidance.
const DOOR_RETRY_MAX_MS: u64 = 60_000;

/// The request door: how many requests this host answers at once.
///
/// It is bounded separately from the workers (see `request_permits`) because
/// it bounds different work. Being full is a typed refusal in the kernel's
/// own vocabulary — `capacity_exhausted`, scope `door`, retryable, with the
/// wait and the remedy — and not a bare 429 a caller has to guess about.
pub struct Door {
    permits: Arc<tokio::sync::Semaphore>,
    width: usize,
}

impl Door {
    pub fn new(width: usize) -> Self {
        Self {
            permits: Arc::new(tokio::sync::Semaphore::new(width)),
            width,
        }
    }

    /// How wide this door is, which is also how many requests a caller that
    /// finds it full is waiting behind.
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Deterministic guidance: no clock, no randomness, no project named.
    pub const fn retry_after_ms(waiting: usize) -> u64 {
        let wait = DOOR_RETRY_BASE_MS.saturating_mul(waiting as u64);
        if wait < DOOR_RETRY_BASE_MS {
            DOOR_RETRY_BASE_MS
        } else if wait > DOOR_RETRY_MAX_MS {
            DOOR_RETRY_MAX_MS
        } else {
            wait
        }
    }

    /// Take one place in the door, or say why there is none. The permit is
    /// owned, so it lives exactly as long as the request it belongs to.
    fn enter(&self) -> Result<tokio::sync::OwnedSemaphorePermit, Failure> {
        self.permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| self.saturated())
    }

    /// The answer a full door gives, in the shape every other refusal on this
    /// host has.
    fn saturated(&self) -> Failure {
        let waiting = self.width.saturating_sub(self.permits.available_permits());
        let retry_after_ms = Self::retry_after_ms(waiting);
        Failure::unavailable(
            // Written out, not named through the kernel's constant: the
            // refusal-coverage scan reads a literal, and a code it cannot
            // read is a code nothing checks is documented.
            "capacity_exhausted",
            format!(
                "this host answers {} requests at once and all of them are in flight",
                self.width
            ),
        )
        .remedy(format!(
            "retry after {retry_after_ms} ms; the door clears as requests finish, and it is the larger of --workers and {}, so only a host restarted with --workers above {} opens a wider one",
            crate::MIN_REQUEST_PERMITS,
            crate::MIN_REQUEST_PERMITS
        ))
        .detail(json!({"retry_after_ms": retry_after_ms, "scope": "door"}))
    }
}

/// One typed refusal on the wire, in the shape `ds` already re-raises: the
/// class, the code, the sentence, the remedy, and — for capacity — the retry
/// guidance the kernel computed.
pub fn typed(failure: &Failure) -> ApiError {
    let status = if failure.code() == CAPACITY_EXHAUSTED {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        match failure.class() {
            ExitClass::Success => StatusCode::OK,
            ExitClass::InvalidInput => StatusCode::BAD_REQUEST,
            ExitClass::Unauthorized => StatusCode::UNAUTHORIZED,
            ExitClass::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            ExitClass::Conflict => StatusCode::CONFLICT,
            ExitClass::Failed => StatusCode::BAD_GATEWAY,
            ExitClass::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    };
    let mut body = json!({
        "error": failure.message(),
        "class": failure.class().token(),
        "code": failure.code(),
        "retryable": failure.class().retryable(),
    });
    if let Some(remedy) = failure.remedy_text() {
        body["remedy"] = json!(remedy);
    }
    if let Some(Value::Object(detail)) = failure.detail_value() {
        for (key, value) in detail {
            body[key.as_str()] = value.clone();
        }
    }
    (status, Json(body))
}

fn authorize(app: &App) -> Result<(), ApiError> {
    // The authorizer answers from the credential this machine holds, and it
    // refuses for exactly one reason: a local answer that the owner changed.
    // "The gateway is unreachable" is not one of its answers, so there is no
    // reason left here to translate into a generic 401 — the refusal is the
    // named one a caller can plan for.
    app.auth.authorize(&app.connection.owner).map_err(|reason| {
        typed(
            &Failure::unauthorized(
                // A literal, like `needs_paired_map` below: the
                // refusal-coverage scan reads literals, and a code it cannot
                // read is a code nothing checks is documented.
                "server_owner_changed",
                reason,
            )
            .remedy(crate::OWNER_CHANGED.remedy),
        )
    })
}

pub fn router(app: App) -> Router {
    Router::new()
        .route("/v1/jobs", get(list))
        .route("/v1/jobs/:id", get(status))
        .route("/v1/jobs/:id/cancel", post(cancel))
        .route("/v1/jobs/:id/result", get(result))
        .route("/v1/jobs/:id/input", get(input))
        .route("/v1/activity", get(activity))
        .route("/v1/layers", get(crate::layers::list))
        .route("/v1/layers/visibility", post(crate::layers::visibility))
        .route("/v1/layers/order", post(crate::layers::order))
        .route(
            "/v1/layers/default-visibility",
            post(crate::layers::default_visibility),
        )
        .route(
            "/v1/survey/working-area-forms",
            get(crate::working_area_forms::read),
        )
        .route(
            "/v1/survey/working-area-forms/select",
            post(crate::working_area_forms::select),
        )
        .route(
            "/v1/survey/working-area-forms/clear",
            post(crate::working_area_forms::clear),
        )
        .route("/v1/transformer-processing/:key", post(submit))
        .route("/v1/solar-processing/:key", post(submit_solar))
        .route(
            "/v1/solar-application",
            post(crate::solar_application::invoke),
        )
        .route("/v1/tile-processing/:key", post(submit_tiles))
        .fallback(unserved)
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(app.clone(), access))
        .with_state(app)
}
async fn access(
    State(app): State<App>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // One Server, one owner: the process at the other end of the socket runs
    // as the account this Server runs as, by the kernel's word, and there is
    // no second identity for a request to name. Anything else is denied here,
    // before it takes a place in the door -- and that IS the rule: many users
    // are many machines. A request that did not come through the socket
    // carries no peer and is a stranger too.
    let peer = request
        .extensions()
        .get::<crate::transport::Peer>()
        .copied();
    crate::transport::admit(peer, app.os_uid).map_err(|refused| typed(&refused))?;
    let permit = app.requests.enter().map_err(|full| typed(&full))?;
    tokio::task::spawn_blocking(move || authorize(&app))
        .await
        .map_err(|_| typed(&worker_lost("authorization")))??;
    let response = next.run(request).await;
    drop(permit);
    Ok(response)
}
/// Anything this host does not serve, said in words rather than as an empty
/// 404. A Server is a real host of the shared operations — it answers the
/// same ids and shapes the paired desktop answers — but it has no rendered
/// map, so an operation that needs one is a named refusal with the remedy,
/// never a different command and never silence.
async fn unserved(request: Request) -> ApiError {
    let path = request.uri().path().to_owned();
    if path.starts_with("/v1/map/") || path.starts_with("/v1/invoke") {
        return typed(
            &Failure::unavailable(
                // Written out, not named through the constant above: the
                // refusal-coverage scan reads a literal, and a code it cannot
                // read is a code nothing checks is documented.
                "needs_paired_map",
                "this host runs the operation but has no rendered map to run it against",
            )
            .remedy(
                "run the same command with --target desktop, against a window open on this project",
            ),
        );
    }
    typed(
        &Failure::invalid(
            "unsupported_operation",
            format!("this server does not serve {path}"),
        )
        .remedy("update ds, or read ds server --help for what this host serves"),
    )
}

/// A blocking worker that never came back. The host's fault, not the
/// caller's — and still typed, because an answer with no code, no class and no
/// remedy leaves whoever receives it nothing to plan for and nothing to do.
fn worker_lost(what: &str) -> Failure {
    Failure::internal(
        "server_refused",
        format!("this host's {what} worker did not return"),
    )
    .remedy("repeat the request; if it repeats, restart ds server serve and report it")
}

/// Work whose failure is a plain sentence rather than a typed refusal.
///
/// The sentence is the host's own — an unreadable durable store, a Sync Center
/// projection that could not be built — and it used to leave here as a bare
/// `409 {"error": …}`: no code, no class, no remedy. A client cannot re-raise
/// what it was not told, so `ds` reported those as class `failed` with the
/// generic `server_refused` fallback while the host had said `conflict`, and
/// the caller got no remedy at all. It is the same failure `admitting` states
/// through [`host_failure`], so it is stated that way here too.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| typed(&worker_lost("native")))?
        .map_err(|reason| typed(&host_failure(reason)))
}
/// The same queue, for work that answers with the kernel's own refusals.
pub(crate) async fn admitting<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| typed(&worker_lost("native")))?
        .map_err(|failure| typed(&failure))
}

/// `?project=<id>` — the caller naming which project this call is about.
/// Optional on the job routes, where it narrows; required where an operation
/// must be about exactly one project.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectQuery {
    pub project: Option<String>,
}

/// The project a caller named, or nothing. A name that is not one path
/// segment is refused HERE, before the handler opens anything — by the same
/// kernel rule and in the same typed shape the write doors answer with
/// ([`sessions::narrowing_project`]) — so a read route never turns `..` into
/// an empty list or `A/../B` into `job not found`.
pub(crate) fn project_query(
    query: Option<Query<ProjectQuery>>,
) -> Result<Option<String>, ApiError> {
    let Some(Query(query)) = query else {
        return Err(typed(
            &Failure::invalid(
                "invalid_input",
                "this route accepts an optional project query parameter only",
            )
            .remedy("send ?project=<exact-id>"),
        ));
    };
    sessions::narrowing_project(query.project.as_deref()).map_err(|failure| typed(&failure))
}

async fn list(
    State(app): State<App>,
    _headers: HeaderMap,
    query: Option<Query<ProjectQuery>>,
) -> Result<Json<Value>, ApiError> {
    let project = project_query(query)?;
    blocking(move || {
        let identity = app.sessions.identity();
        let rows = runtime::open(&app.database)?
            .jobs(&identity.caller(project.as_deref()), 101)
            .map_err(|e| e.to_string())?;
        let more = rows.len() > 100;
        Ok(Json(
            json!({"jobs":rows.into_iter().take(100).collect::<Vec<_>>(),"more":more}),
        ))
    })
    .await
}
async fn status(
    State(app): State<App>,
    _headers: HeaderMap,
    Param(id): Param<String>,
    query: Option<Query<ProjectQuery>>,
) -> Result<Json<Value>, ApiError> {
    let project = project_query(query)?;
    admitting(move || {
        let identity = app.sessions.identity();
        let job = runtime::open(&app.database)
            .map_err(host_failure)?
            .job(&identity.caller(project.as_deref()), &id)
            .map_err(|error| host_failure(error.to_string()))?
            .ok_or_else(sessions::not_found)?;
        Ok(Json(json!({"job":job})))
    })
    .await
}
async fn submit(
    State(app): State<App>,
    _headers: HeaderMap,
    Param(key): Param<String>,
    query: Option<Query<ProjectQuery>>,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let project = project_query(query)?;
    admitting(move || {
        let job = admitted(&app.sessions, &key, project.as_deref(), |admission| {
            runtime::submit(&app.database, admission, &body)
        })?;
        Ok((StatusCode::ACCEPTED, Json(json!({"job":job}))))
    })
    .await
}
/// Where a submission's input is a file on this machine rather than bytes in
/// the request. The Server accesses the filesystem exactly as the desktop
/// does: a prepared Solar envelope IS a workspace file, so the route takes its
/// path, reads it under the owner's identity and digests the bytes it read.
/// `ds` passes `--input` straight through.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedInputPath {
    pub input_path: String,
}

/// One local file, read as the owner. Absolute because the client and the
/// host are one machine's user and a relative path would name whatever
/// directory the host happens to be running in.
fn read_owner_file(path: &str) -> Result<Vec<u8>, Failure> {
    let refused = |message: &str| {
        Failure::invalid("server_refused", message.to_owned())
            .remedy("send input_path as an absolute path to a readable file on this machine")
    };
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(refused(
            "input_path must be an absolute path on this machine",
        ));
    }
    let file = fs::File::open(path).map_err(|error| refused(&error.to_string()))?;
    if !file
        .metadata()
        .map_err(|error| refused(&error.to_string()))?
        .is_file()
    {
        return Err(refused("input_path must name a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| refused(&error.to_string()))?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(refused("input exceeds 64 MiB"));
    }
    Ok(bytes)
}

/// The only native Server entry point for a sealed prepared Solar request.
/// Keeping this separate from legacy transformer processing prevents a failed
/// LV request decode from becoming an alternate engine-dispatch authority.
async fn submit_tiles(
    State(app): State<App>,
    Param(key): Param<String>,
    query: Option<Query<ProjectQuery>>,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let project = project_query(query)?;
    admitting(move || {
        let job = admitted(&app.sessions, &key, project.as_deref(), |admission| {
            runtime::tiles::submit(&app.database, admission, &body)
        })?;
        Ok((StatusCode::ACCEPTED, Json(json!({"job":job}))))
    })
    .await
}

async fn submit_solar(
    State(app): State<App>,
    _headers: HeaderMap,
    Param(key): Param<String>,
    query: Option<Query<ProjectQuery>>,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let project = project_query(query)?;
    admitting(move || {
        let sessions = app.sessions.clone();
        let named: SealedInputPath = serde_json::from_slice(&body).map_err(|error| {
            Failure::invalid("server_refused", error.to_string())
                .remedy("send {\"input_path\": \"<absolute path to the sealed envelope>\"}")
        })?;
        let envelope = read_owner_file(&named.input_path)?;
        let (sealed, provenance) =
            runtime::decode_solar_server_submission(&envelope).map_err(|error| {
                Failure::invalid("server_refused", error)
                    .remedy("send the documented ds.solar.server-submission/v1 envelope")
            })?;
        // The claim's project is the sealed input's own — the decoder refused
        // anything else — so it is what the kernel's sealed-outranks-named
        // rule is given, and what the request is admitted about. The project
        // the caller named still reaches the kernel through the submission
        // itself, which is how a contradicting name is refused.
        let job = admitted(&sessions, &key, project.as_deref(), |admission| {
            runtime::submit_solar_with_provenance(
                &app.database,
                admission,
                &sealed,
                provenance.clone(),
            )
        })?;
        Ok((StatusCode::ACCEPTED, Json(json!({"job":job}))))
    })
    .await
}
async fn cancel(
    State(app): State<App>,
    _headers: HeaderMap,
    Param(id): Param<String>,
    query: Option<Query<ProjectQuery>>,
) -> Result<Json<Value>, ApiError> {
    let project = project_query(query)?;
    admitting(move || {
        let identity = app.sessions.identity();
        let caller = identity.caller(project.as_deref());
        let completed_solar = |job: &ds_command_kernel::compute_jobs::Job| {
            job.engine == ds_command_kernel::compute_jobs::EngineKind::SolarPrepared
                && job.phase == ds_command_kernel::compute_jobs::Phase::Completed
        };
        let publication_cancel = |job: ds_command_kernel::compute_jobs::Job| {
            let activity = app.activity.as_ref().ok_or_else(|| {
                host_failure("Solar Sync Center activity is unavailable before server startup")
            })?;
            let publication = activity.cancel_publication(&job).map_err(host_failure)?;
            Ok(Json(json!({"job":job,"publication":publication})))
        };
        let current = runtime::open(&app.database)
            .map_err(host_failure)?
            .job(&caller, &id)
            .map_err(|error| host_failure(error.to_string()))?
            // An id belonging to another project is not disclosed by being
            // refused differently from one that never existed.
            .ok_or_else(sessions::not_found)?;
        if completed_solar(&current) {
            return publication_cancel(current);
        }
        match runtime::open(&app.database)
            .map_err(host_failure)?
            .update_job(&caller, &id, Event::Cancel, runtime::now_ms(), None)
        {
            Ok(job) => Ok(Json(json!({"job":job}))),
            Err(error) if error.to_string().contains("job_already_terminal") => {
                let completed = runtime::open(&app.database)
                    .map_err(host_failure)?
                    .job(&caller, &id)
                    .map_err(|error| host_failure(error.to_string()))?
                    .ok_or_else(sessions::not_found)?;
                if completed_solar(&completed) {
                    publication_cancel(completed)
                } else {
                    Err(host_failure(error.to_string()))
                }
            }
            Err(error) => Err(host_failure(error.to_string())),
        }
    })
    .await
}
async fn result(
    State(app): State<App>,
    _headers: HeaderMap,
    Param(id): Param<String>,
    query: Option<Query<ProjectQuery>>,
) -> Result<Response, ApiError> {
    let project = project_query(query)?;
    admitting(move || {
        let identity = app.sessions.identity();
        let caller = identity.caller(project.as_deref());
        let store = runtime::open(&app.database).map_err(host_failure)?;
        // Visibility first, and separately: a job in another project is not
        // found, exactly as an invented id is, while a job this caller CAN
        // see and that simply has not finished is told so.
        store
            .job(&caller, &id)
            .map_err(|error| host_failure(error.to_string()))?
            .ok_or_else(sessions::not_found)?;
        let bytes = store
            .job_result(&caller, &id)
            .map_err(|error| host_failure(error.to_string()))?
            .ok_or_else(|| {
                Failure::conflict("server_refused", "job has no completed result")
                    .remedy("wait for the job to complete, then repeat")
            })?;
        Ok((
            [
                ("content-type", "application/json"),
                ("cache-control", "no-store"),
            ],
            bytes,
        )
            .into_response())
    })
    .await
}
/// `GET /v1/jobs/:id/input[?project=<id>]` — the exact request bytes this job
/// was admitted with, back to the owner who handed them over.
///
/// Retained input is not new: the store has always kept it so a publication
/// can be reconstructed after a restart. What was missing was a way for the
/// OWNER to read it, which made one remedy unactionable — a row stored by a
/// released Server names no project, will never run, and has no result to
/// read, so "read the result and resubmit" asked for something that does not
/// exist. Its input does exist, and this is how it comes back.
///
/// Visible-fenced exactly like `result`: a job in another project is `job not
/// found`, identical to an id that never existed. The one difference is that
/// no phase is required — a queued job's input is as readable as a completed
/// one's, which is the whole point.
///
/// `project` is optional narrowing here and the client sends only what the
/// caller named, because a row with no context of its own is visible only to
/// an unnarrowed read. Narrowing a request for exactly that row would hide
/// the thing the remedy sends the owner to fetch.
async fn input(
    State(app): State<App>,
    _headers: HeaderMap,
    Param(id): Param<String>,
    query: Option<Query<ProjectQuery>>,
) -> Result<Response, ApiError> {
    let project = project_query(query)?;
    admitting(move || {
        let identity = app.sessions.identity();
        let caller = identity.caller(project.as_deref());
        let bytes = runtime::open(&app.database)
            .map_err(host_failure)?
            .job_input(&caller, &id)
            .map_err(|error| host_failure(error.to_string()))?
            .ok_or_else(sessions::not_found)?;
        Ok((
            [
                ("content-type", "application/json"),
                ("cache-control", "no-store"),
            ],
            bytes,
        )
            .into_response())
    })
    .await
}
/// `GET /v1/activity[?project=<id>]` — one envelope, one entry per project
/// this connection has durable work in. Without a project it reports every
/// one of them; with a project it reports that one, or nothing at all when
/// the project holds nothing this caller may see.
///
/// One project's projection failing is THAT project's entry, never the
/// envelope's: a Sync Center session one project cannot open — a gateway that
/// refused it, a project whose entitlement is gone — must not hide what every
/// other project is doing. Such an entry carries `unavailable` with the reason
/// and no activity, which is why it cannot be misread as "no work".
async fn activity(
    State(app): State<App>,
    _headers: HeaderMap,
    query: Option<Query<ProjectQuery>>,
) -> Result<Json<Value>, ApiError> {
    let project = project_query(query)?;
    blocking(move || {
        let activity = app
            .activity
            .clone()
            .ok_or("Solar Sync Center activity is unavailable before server startup")?;
        let mut projects = Vec::new();
        for scope in project_scopes(&app, project.as_deref())? {
            // Whether this project holds more durable Solar work than one
            // projection covers is a LOCAL fact about the rows on disk, so it
            // is reported whether or not the projection itself could be read.
            let more = activity.projection_truncated(&scope)?;
            let mut entry = match activity.store_read(&scope) {
                Ok(read) => json!({"project": scope, "activity": read}),
                Err(reason) => json!({"project": scope, "unavailable": reason}),
            };
            entry["more"] = json!(more);
            projects.push(entry);
        }
        Ok(Json(
            json!({"schema": ACTIVITY_SCHEMA, "projects": projects}),
        ))
    })
    .await
}

/// Which projects an answer about "this Server's work" covers: the distinct
/// projects of the durable jobs this caller can see, plus the ones holding
/// report publications, narrowed to one when the caller named one.
///
/// Read by PAGING the queue, never by taking its newest page: a project whose
/// work is older than the last thousand rows of a long-running host is still a
/// project this Server has work in, and an activity answer that quietly left
/// it out would report "nothing" for a project that has something.
pub fn project_scopes(app: &App, project: Option<&str>) -> Result<Vec<String>, String> {
    let mut scopes: BTreeSet<String> = app
        .sessions
        .durable_projects()?
        .into_iter()
        .filter(|held| project.is_none_or(|named| named == held))
        .collect();
    scopes.extend(
        crate::server_reports::projects_with_publications(
            &app.database,
            &crate::server_sync::fence_of(app.sessions.identity()),
        )?
        .into_iter()
        .filter(|held| project.is_none_or(|named| named == held)),
    );
    Ok(scopes.into_iter().collect())
}

/// Run one submission under this connection's identity and the project it
/// names. There is nothing to fetch and nothing to retry: the owner named the
/// project (or the sealed input did), the kernel records it, and everything
/// it decides is relayed exactly as decided.
fn admitted<T>(
    sessions: &ServerSessions,
    key: &str,
    project: Option<&str>,
    submit: impl FnOnce(&Admission<'_>) -> Result<T, runtime::SubmitError>,
) -> Result<T, Failure> {
    let now_ms = runtime::now_ms();
    let client = sessions.client_label();
    submit(&Admission {
        identity: sessions.identity(),
        client: &client,
        key,
        requested_project: project,
        limits: sessions.limits(),
        now_ms,
    })
    .map_err(|error| sessions::submit_failure(&error))
}

fn host_failure(message: impl ToString) -> Failure {
    Failure::conflict("server_refused", message.to_string())
        .remedy("read the stated reason; verify ds auth status and that ds server serve is running")
}

pub fn prepare_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("server state directory must be absolute".into());
    }
    if !path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)
                .map_err(|e| e.to_string())?;
        }
        #[cfg(not(unix))]
        {
            return Err(
                "server hosting currently requires Linux protected filesystem state".into(),
            );
        }
    }
    protected(path, true)?;
    // Everything the Server keeps below its protected directory is private
    // too (owner rule, ds_layer_store::private): an install from before the
    // rule — store.sqlite, the tile cache, held media — is tightened once.
    ds_layer_store::private::tighten_root_once(path);
    Ok(())
}
fn protected(path: &Path, directory: bool) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err("server state must be an owned regular path".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid reads the calling process identity and has no pointers.
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(
                "server state must be owner-only (directory 0700, connection file 0600)".into(),
            );
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        Err("server connection requires a protected native state adapter on this platform".into())
    }
}
/// Read the protected `connection.json` of the Server over `directory`.
///
/// Both shapes are read — this build's, and the one a `ds` from before the
/// socket wrote — because both name whose state this is. The older one comes
/// back with `legacy_address` set, and is never used as a transport: its
/// bearer is not even read.
pub fn load_connection(directory: &Path) -> Result<Connection, String> {
    protected(directory, true)?;
    let path = directory.join("connection.json");
    protected(&path, false)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4096 {
        return Err("connection file too large".into());
    }
    let socket = crate::transport::socket_path(directory);
    if let Ok(record) = serde_json::from_slice::<Record>(&bytes) {
        if record.schema != CONNECTION_SCHEMA {
            return Err(format!(
                "this server connection is {}, and this ds reads {CONNECTION_SCHEMA}; use the ds that started the Server",
                record.schema
            ));
        }
        return Ok(Connection {
            owner: record.owner,
            lane: record.lane,
            socket,
            legacy_address: None,
        });
    }
    if let Ok(legacy) = serde_json::from_slice::<LegacyRecord>(&bytes) {
        if !legacy.address.ip().is_loopback() {
            return Err("invalid server address".into());
        }
        return Ok(Connection {
            owner: legacy.owner,
            lane: legacy.lane,
            socket,
            legacy_address: Some(legacy.address),
        });
    }
    Err("invalid protected server connection".into())
}
/// One Server serves one owner, and this is the sentence a second account
/// meets when it asks one Server to serve it too: the protected state it
/// pointed at is already another account's. Many users are many machines, so
/// the remedy is a host of one's own, never a second identity in this process.
pub const MULTI_PRINCIPAL_UNSUPPORTED: &str = "multi_principal_unsupported";

/// Settle the protected record of the Server about to start over `directory`
/// for `owner` on `lane`, writing it when there is none.
///
/// A record left by a `ds` from before the socket is adopted — same owner,
/// same lane — and rewritten without its bearer, unless the TCP address it
/// names still accepts connections: then an older Server may still be
/// running on this state, and two hosts on one queue is refused. Nothing is
/// sent to that address; the probe is a connect and nothing more.
pub fn connection(directory: &Path, owner: String, lane: String) -> Result<Connection, Failure> {
    prepare_directory(directory).map_err(host_failure)?;
    let path = directory.join("connection.json");
    if !path.exists() {
        write_record(directory, &owner, &lane)?;
        return Ok(Connection {
            owner,
            lane,
            socket: crate::transport::socket_path(directory),
            legacy_address: None,
        });
    }
    let existing = load_connection(directory).map_err(host_failure)?;
    // A second ACCOUNT is the one thing this Server can never become, so it
    // is answered by its own name rather than as a generic refusal — and
    // separately from a lane that simply does not match, which is one owner's
    // own misconfiguration.
    if existing.owner != owner {
        return Err(Failure::conflict(
            // Written out, like `needs_paired_map` above: the
            // refusal-coverage scan reads a literal, and a code it cannot
            // read is a code nothing checks is documented.
            "multi_principal_unsupported",
            "this protected server state belongs to another account; one Server serves exactly one owner",
        )
        .remedy("run that account its own ds server serve, with its own --state-dir"));
    }
    if existing.lane != lane {
        return Err(host_failure(
            "the existing server connection is on another lane; use a separate state directory",
        ));
    }
    let Some(address) = existing.legacy_address else {
        return Ok(existing);
    };
    if std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(1)).is_ok() {
        return Err(Failure::conflict(
            "server_refused",
            format!(
                "{address}, where an older ds server on this state directory listened, still accepts connections; nothing was sent to it"
            ),
        )
        .remedy(
            "stop that older ds server serve, then start this one again; if that port belongs to something else, remove connection.json from the state directory (it holds only the retired bearer) and start again",
        ));
    }
    write_record(directory, &existing.owner, &existing.lane)?;
    Ok(Connection {
        legacy_address: None,
        ..existing
    })
}

/// Write this build's record, owner-only, beside the old one and then over
/// it, so a reader meets the old record or the new one and never half of
/// either. Replacing a record from before the socket is what takes its
/// bearer off the disk.
fn write_record(directory: &Path, owner: &str, lane: &str) -> Result<(), Failure> {
    let bytes = serde_json::to_vec(&Record {
        schema: CONNECTION_SCHEMA.into(),
        owner: owner.into(),
        lane: lane.into(),
    })
    .map_err(host_failure)?;
    let staging = directory.join("connection.json.new");
    match fs::remove_file(&staging) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(host_failure(error));
        }
        _ => {}
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&staging).map_err(host_failure)?;
    file.write_all(&bytes).map_err(host_failure)?;
    file.sync_all().map_err(host_failure)?;
    fs::rename(&staging, directory.join("connection.json")).map_err(host_failure)?;
    #[cfg(unix)]
    fs::File::open(directory)
        .and_then(|d| d.sync_all())
        .map_err(host_failure)?;
    Ok(())
}

/// Run this host on the socket `listening` holds until it is told to stop.
pub async fn serve(
    mut app: App,
    workers: usize,
    listening: crate::transport::Listening,
) -> Result<(), String> {
    // The durable store, created ONCE and by one thread, before anything that
    // will open it concurrently.
    //
    // A cold start is the only moment this matters, and every Server has that
    // moment exactly once: a brand-new `store.sqlite` must be converted to
    // WAL, and SQLite refuses that conversion outright — no busy handler, no
    // retry inside `busy_timeout` — while another connection holds the file.
    // The Solar pump, the worker pool's recovery pass and the first request
    // all open it within milliseconds of each other, so on a fresh state
    // directory two of them raced and the loser killed the host with
    // `the sync store could not be read or written: database is locked`
    // before it ever answered. Opening it here, on this thread, leaves every
    // later open finding a database that is already WAL, where the pragma is
    // a no-op. An unusable store still stops the host, which is the honest
    // answer for a process whose whole job is a durable queue.
    runtime::open(&app.database)?;
    let activity =
        crate::solar_sync::SolarActivity::open(app.database.clone(), app.sessions.clone())?;
    app.activity = Some(activity.clone());
    // The pump merely drains durable StoreHost rows after a completion wake;
    // it does not run in a compute worker or block recovery/server readiness.
    let solar_pump = activity.start_pump();
    let workers = runtime::Workers::start(
        Arc::new(runtime::WorkerContext {
            path: app.database.clone(),
            identity: app.sessions.identity().clone(),
            limits: app.sessions.limits(),
            auth: app.auth.clone(),
            observer: Some(activity),
        }),
        workers,
    )?;
    eprintln!(
        "DS server ready at {} (owner-only socket)",
        listening.socket().display()
    );
    let result = crate::transport::serve(listening, router(app), stop_requested()).await;
    workers.stop();
    solar_pump.stop();
    drop(workers);
    drop(solar_pump);
    result
}

/// Ctrl-C, or the SIGTERM a service manager sends.
async fn stop_requested() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {_ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {}}
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use ds_command_kernel::execution_context::{Limits, Principal};
    use ds_compute_runtime::HostIdentity;
    use sessions::{ServerSessions, SessionOpener};
    use tower::ServiceExt;

    const A: &str = "project-a";
    const B: &str = "project-b";
    const UID: &str = "uid-a";
    const DEPLOYMENT: &str = "https://gateway.example";

    struct Auth(bool);
    impl Authorizer for Auth {
        fn authorize(&self, _: &str) -> Result<(), String> {
            if self.0 {
                Ok(())
            } else {
                Err("device revoked".into())
            }
        }
    }
    /// No gateway in a test: a route that needs one says so, and every route
    /// that must not need one is proven by never reaching this.
    pub(crate) struct NoGateway;
    impl SessionOpener for NoGateway {
        fn open(
            &self,
            _: &Path,
            _: &Connection,
            _: &str,
        ) -> Result<Arc<crate::server_sync::ServerSyncSession>, String> {
            Err("no gateway session in this test".into())
        }
    }

    /// The operating-system account every host in these tests runs as, and
    /// so the peer the kernel would name for its owner's own `ds`.
    pub(crate) const OS_UID: u32 = 4242;
    /// The peer the socket hands the router for the owner's own process.
    pub(crate) const OWNER_PEER: crate::transport::Peer = crate::transport::Peer { uid: OS_UID };

    pub(crate) fn test_connection(state: &Path) -> Connection {
        Connection {
            owner: "test-owner".into(),
            lane: "stable".into(),
            socket: crate::transport::socket_path(state),
            legacy_address: None,
        }
    }
    pub(crate) fn identity(connection: &Connection) -> HostIdentity {
        HostIdentity {
            owner: connection.owner.clone(),
            principal: Principal {
                uid: UID.into(),
                lane: connection.lane.clone(),
                deployment: DEPLOYMENT.into(),
                install_id: "install-1".into(),
            },
        }
    }
    pub(crate) fn limits() -> Limits {
        Limits {
            global_running: 4,
            per_project_running: 2,
            per_project_queued: 8,
            global_queued: 16,
        }
    }
    fn app(path: &Path, authorized: bool) -> App {
        app_with(path, authorized, limits())
    }
    /// The same host, authorizing through the REAL `NativeAuthorizer` over a
    /// machine that holds a credential and an upstream that never answers.
    /// This is the production authorization path with no gateway behind it.
    fn offline_app(path: &Path, source: Arc<crate::auth::tests::FixtureCredential>) -> App {
        let mut app = app_with(path, true, limits());
        app.auth = Arc::new(
            crate::auth::NativeAuthorizer::from_source(source, std::time::Duration::ZERO)
                .expect("a machine that holds a credential"),
        );
        app
    }
    /// A Server over `path` with no directory, no snapshot and no upstream:
    /// exactly what production has.
    fn app_with(path: &Path, authorized: bool, limits: Limits) -> App {
        let connection = test_connection(path);
        let database = path.join("store.sqlite");
        let sessions = ServerSessions::with(
            connection.clone(),
            database.clone(),
            limits,
            identity(&connection),
            Arc::new(NoGateway),
        );
        App {
            database,
            connection,
            os_uid: Some(OS_UID),
            auth: Arc::new(Auth(authorized)),
            requests: Arc::new(Door::new(4)),
            activity: None,
            solar: Arc::new(crate::solar_application::Applications::default()),
            layers: crate::layers::NativeLayerHost::fixture_native("stable"),
            sessions,
        }
    }
    fn transformer(name: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"schema":"ds.fast-lv.request/v1","jobs":[{"transformer_name":name,"gdfs":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr-1","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":name,"names":name}}]},"lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line-1","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]},"customers":{"type":"FeatureCollection","features":[]}},"settings":{}}]})).unwrap()
    }
    #[tokio::test]
    async fn native_solar_application_is_headless_and_project_workspaces_cannot_cross() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path(), true);
        let body = serde_json::to_vec(&json!({"operation":"workspace_default"})).unwrap();
        let (status, a) = call(
            app.clone(),
            "POST",
            "/v1/solar-application?project=project-a",
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{a}");
        assert_eq!(a["project_id"], "project-a");
        let (status, b) = call(
            app.clone(),
            "POST",
            "/v1/solar-application?project=project-b",
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{b}");
        assert_ne!(
            a["value"]["workspace_handle"],
            b["value"]["workspace_handle"]
        );
        assert_ne!(a["value"]["dir"], b["value"]["dir"]);
        let foreign = serde_json::to_vec(&json!({"operation":"readiness","command":{"workspace_handle":a["value"]["workspace_handle"],"root":"eds_project/project-b/eds_solar","city_ids":["city"]}})).unwrap();
        let (status, denied) = call(
            app,
            "POST",
            "/v1/solar-application?project=project-b",
            Some(foreign),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{denied}");
        let no_auth = self::app(dir.path(), false);
        let (status, _) = call(
            no_auth,
            "POST",
            "/v1/solar-application?project=project-a",
            Some(br#"{"operation":"engine"}"#.to_vec()),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    /// One request straight at the router, as the owner's own process.
    async fn call(app: App, method: &str, uri: &str, body: Option<Vec<u8>>) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .extension(OWNER_PEER)
            .header("content-type", "application/json");
        let response = router(app)
            .oneshot(
                request
                    .body(body.map_or_else(Body::empty, Body::from))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// The same call, keeping the answer's exact bytes: a route that returns
    /// stored bytes is judged on the bytes, not on a re-encoding of them.
    async fn raw(app: App, method: &str, uri: &str) -> (StatusCode, Vec<u8>) {
        let response = router(app)
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .extension(OWNER_PEER)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
            .await
            .unwrap();
        (status, bytes.to_vec())
    }

    /// A request as a given peer — or, with `None`, one that did not come
    /// through the socket at all.
    async fn as_peer(app: App, peer: Option<crate::transport::Peer>) -> Response {
        let mut request = Request::builder().uri("/v1/jobs");
        if let Some(peer) = peer {
            request = request.extension(peer);
        }
        router(app)
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn unauthenticated_or_revoked_calls_never_read_or_create_jobs() {
        let dir = tempfile::tempdir().unwrap();
        for (allowed, peer) in [
            (true, Some(crate::transport::Peer { uid: OS_UID + 1 })),
            (true, None),
            (false, Some(OWNER_PEER)),
        ] {
            let response = as_peer(app(dir.path(), allowed), peer).await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert!(!dir.path().join("store.sqlite").exists());
        }
    }
    #[tokio::test]
    async fn authenticated_status_is_served_without_a_ui() {
        let dir = tempfile::tempdir().unwrap();
        let (status, value) = call(app(dir.path(), true), "GET", "/v1/jobs", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value, json!({"jobs":[],"more":false}));
    }

    /// One owner per Server, enforced by the kernel's word on who is at the
    /// other end of the socket and by nothing else. There is no header that
    /// names an account and no second identity to distinguish: a process of
    /// any other account — root included — is denied at the door, and that is
    /// the whole of the rule. Many users are many machines.
    #[tokio::test]
    async fn one_owner_per_server_is_the_socket_peer_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        for stranger in [OS_UID + 1, 0] {
            let response = as_peer(
                app(dir.path(), true),
                Some(crate::transport::Peer { uid: stranger }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["code"], "server_peer_refused");
            assert_eq!(value["class"], "unauthorized");
            // The refusal names nobody: not the peer, not the owner.
            let said = String::from_utf8_lossy(&bytes);
            for secret in [OS_UID.to_string(), stranger.to_string(), UID.to_owned()] {
                assert!(!said.contains(&secret), "{said} names {secret}");
            }
        }
        // A header naming an account, or the bearer an older client sends, is
        // not a thing this host reads: the owner's own process is served
        // whatever it says, and a stranger is refused whatever it says.
        for (peer, expected) in [
            (OWNER_PEER, StatusCode::OK),
            (
                crate::transport::Peer { uid: OS_UID + 1 },
                StatusCode::UNAUTHORIZED,
            ),
        ] {
            let response = router(app(dir.path(), true))
                .oneshot(
                    Request::builder()
                        .uri("/v1/jobs")
                        .extension(peer)
                        .header("x-ds-principal", "uid-somebody-else")
                        .header("authorization", format!("Bearer {}", "a".repeat(64)))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                expected,
                "the socket's peer is the only question asked"
            );
        }
    }

    /// One operation id, one shape, whichever host runs it — and where this
    /// host genuinely cannot, it says which one can.
    #[tokio::test]
    async fn an_operation_that_needs_a_rendered_map_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let (status, refused) = call(
            app(dir.path(), true),
            "POST",
            "/v1/map/screenshot",
            Some(b"{}".to_vec()),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refused}");
        assert_eq!(refused["code"], NEEDS_PAIRED_MAP);
        assert!(
            refused["remedy"]
                .as_str()
                .unwrap()
                .contains("--target desktop")
        );
        // Anything else this host does not serve is still a sentence, not an
        // empty 404 a client has to guess about.
        let (status, unknown) = call(app(dir.path(), true), "GET", "/v1/nothing", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(unknown["code"], "unsupported_operation");
        assert!(unknown["error"].as_str().unwrap().contains("/v1/nothing"));
    }

    #[tokio::test]
    async fn tile_jobs_retain_bytes_and_isolate_sealed_projects_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let host = app(dir.path(), true);
        let prepared = |project: &str| {
            serde_json::to_vec(&json!({
            "schema":"ds.tiles.prepared/v1", "project":project,
            "layers":{"poles":"{\"type\":\"Feature\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[29.8,-2.4]},\"properties\":{}}\n"},
            "options":{"min_zoom":4,"max_zoom":8,"base_zoom":8,"full_detail":16,"drop_densest_as_needed":true,"no_feature_limit":true,"no_tile_size_limit":true}
        })).unwrap()
        };
        let input_a = prepared(A);
        let (status, refused) = call(
            host.clone(),
            "POST",
            &format!("/v1/tile-processing/shared?project={B}"),
            Some(input_a.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{refused}");
        let (status, first) = call(
            host.clone(),
            "POST",
            "/v1/tile-processing/shared",
            Some(input_a.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{first}");
        assert_eq!(first["job"]["engine"], "tiles_prepared");
        assert_eq!(first["job"]["context"]["operation"], "tile_processing");
        let (_, second) = call(
            host.clone(),
            "POST",
            "/v1/tile-processing/shared",
            Some(prepared(B)),
        )
        .await;
        assert_ne!(first["job"]["id"], second["job"]["id"]);
        let (_, replay) = call(
            host.clone(),
            "POST",
            "/v1/tile-processing/shared",
            Some(input_a.clone()),
        )
        .await;
        assert_eq!(first["job"]["id"], replay["job"]["id"]);
        let id = first["job"]["id"].as_str().unwrap();
        let restarted = app(dir.path(), true);
        let (status, bytes) = raw(
            restarted.clone(),
            "GET",
            &format!("/v1/jobs/{id}/input?project={A}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, input_a);
        let (status, _) = call(
            restarted.clone(),
            "POST",
            &format!("/v1/jobs/{id}/cancel?project={B}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = call(
            restarted,
            "POST",
            &format!("/v1/jobs/{id}/cancel?project={A}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn two_projects_run_on_one_server_and_neither_is_disclosed_to_the_other() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path(), true);
        let (status, first) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/for-a?project={A}"),
            Some(transformer("T1")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{first}");
        assert_eq!(first["job"]["context"]["project"], A);
        assert_eq!(
            first["job"]["context"]["operation"],
            "transformer_processing"
        );
        let (status, second) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/for-b?project={B}"),
            Some(transformer("T2")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{second}");
        assert_eq!(second["job"]["context"]["project"], B);
        let (a_id, b_id) = (
            first["job"]["id"].as_str().unwrap().to_owned(),
            second["job"]["id"].as_str().unwrap().to_owned(),
        );

        // Unnarrowed: both. Narrowed: one each.
        let (_, all) = call(app.clone(), "GET", "/v1/jobs", None).await;
        assert_eq!(all["jobs"].as_array().unwrap().len(), 2);
        let (_, only_a) = call(app.clone(), "GET", &format!("/v1/jobs?project={A}"), None).await;
        assert_eq!(only_a["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(only_a["jobs"][0]["id"], a_id);
        // A project no work was ever handed in for simply holds nothing.
        let (status, stranger) = call(app.clone(), "GET", "/v1/jobs?project=project-z", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(stranger["jobs"], json!([]));

        // Status, cancel and result of A's job asked for as B, and the same
        // three for an id that never existed: one sentence, one code, always.
        let guessed = "0".repeat(64);
        let mut answers = Vec::new();
        for (method, uri) in [
            ("GET", format!("/v1/jobs/{a_id}?project={B}")),
            ("GET", format!("/v1/jobs/{guessed}?project={B}")),
            ("GET", format!("/v1/jobs/{guessed}")),
            ("POST", format!("/v1/jobs/{a_id}/cancel?project={B}")),
            ("POST", format!("/v1/jobs/{guessed}/cancel?project={B}")),
            ("GET", format!("/v1/jobs/{a_id}/result?project={B}")),
        ] {
            let (status, body) = call(app.clone(), method, &uri, None).await;
            answers.push((status, body["code"].clone(), body["error"].clone()));
        }
        let status_answers = &answers[..5];
        for answer in status_answers {
            assert_eq!(answer.1, "not_visible", "{answer:?}");
            assert_eq!(answer.2, "job not found", "{answer:?}");
            assert_eq!(answer.0, StatusCode::CONFLICT);
        }
        // A completed result of another project's job is the same absence.
        assert_eq!(answers[5].1, "not_visible");

        // Nothing was cancelled by any of that.
        let (_, a_status) = call(
            app.clone(),
            "GET",
            &format!("/v1/jobs/{a_id}?project={A}"),
            None,
        )
        .await;
        assert_eq!(a_status["job"]["phase"], "queued");
        let (_, b_status) = call(
            app.clone(),
            "GET",
            &format!("/v1/jobs/{b_id}?project={B}"),
            None,
        )
        .await;
        assert_eq!(b_status["job"]["phase"], "queued");

        // Restart: a new App over the same protected state keeps every job in
        // the project it was admitted into.
        let restarted = self::app(dir.path(), true);
        let (_, after) = call(
            restarted.clone(),
            "GET",
            &format!("/v1/jobs?project={B}"),
            None,
        )
        .await;
        assert_eq!(after["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(after["jobs"][0]["id"], b_id);
        assert_eq!(after["jobs"][0]["context"]["project"], B);
        // And each project's own cancel still works, in its own project.
        let (status, cancelled) = call(
            restarted,
            "POST",
            &format!("/v1/jobs/{a_id}/cancel?project={A}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{cancelled}");
        assert_eq!(cancelled["job"]["phase"], "cancelled");
    }

    #[tokio::test]
    async fn a_key_is_one_handle_inside_its_project_and_other_work_outside_it() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path(), true);
        let (status, first) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/shared?project={A}"),
            Some(transformer("T1")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        // Same key, same bytes, same project: the same job.
        let (status, again) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/shared?project={A}"),
            Some(transformer("T1")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(again["job"]["id"], first["job"]["id"]);
        // The same key in another project is another piece of work: the id
        // digests the project, so the owner's daily key is theirs in each of
        // their projects and neither row can reach the other.
        let (status, elsewhere) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/shared?project={B}"),
            Some(transformer("T1")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{elsewhere}");
        assert_ne!(elsewhere["job"]["id"], first["job"]["id"]);
        assert_eq!(elsewhere["job"]["context"]["project"], B);
        // Same key, other bytes, in the project that holds it.
        let (status, changed) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/shared?project={A}"),
            Some(transformer("T2")),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(changed["code"], "payload_changed_for_key");
        let (_, all) = call(app, "GET", "/v1/jobs", None).await;
        assert_eq!(all["jobs"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_submit_names_a_project_or_is_refused_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path(), true);
        let (status, unnamed) = call(
            app.clone(),
            "POST",
            "/v1/transformer-processing/unnamed",
            Some(transformer("T1")),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(unnamed["code"], "project_required");
        // A name outside the kernel's bound is refused under the kernel's own
        // word for it; the Server substitutes nothing and guesses nothing.
        let (status, padded) = call(
            app.clone(),
            "POST",
            "/v1/transformer-processing/padded?project=%20padded",
            Some(transformer("T1")),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{padded}");
        assert_eq!(padded["code"], "context_corrupt");
        let (_, all) = call(app.clone(), "GET", "/v1/jobs", None).await;
        assert_eq!(all["jobs"], json!([]), "neither refusal queued anything");
        // And a project the Server has never heard of is admitted on the
        // owner's word alone: there is no directory to consult, and whether
        // its effects may leave the machine is the gateway's answer later.
        let (status, first_time) = call(
            app,
            "POST",
            "/v1/transformer-processing/first?project=project-never-seen",
            Some(transformer("T1")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{first_time}");
        assert_eq!(
            first_time["job"]["context"]["project"],
            "project-never-seen"
        );
    }

    #[tokio::test]
    async fn capacity_is_a_typed_answer_and_a_cancellation_gives_the_room_back() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_with(
            dir.path(),
            true,
            Limits {
                global_running: 2,
                per_project_running: 1,
                per_project_queued: 2,
                global_queued: 3,
            },
        );
        let mut ids = Vec::new();
        for (key, project, name) in [("a1", A, "T1"), ("a2", A, "T2")] {
            let (status, body) = call(
                app.clone(),
                "POST",
                &format!("/v1/transformer-processing/{key}?project={project}"),
                Some(transformer(name)),
            )
            .await;
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
            ids.push(body["job"]["id"].as_str().unwrap().to_owned());
        }
        // A is at its share; the answer names the scope and how long to wait.
        let (status, refused) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/a3?project={A}"),
            Some(transformer("T3")),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(refused["code"], "capacity_exhausted");
        assert_eq!(refused["scope"], "project");
        assert!(refused["retry_after_ms"].as_u64().is_some_and(|ms| ms > 0));
        assert_eq!(refused["retryable"], true);
        assert!(
            !refused["error"].as_str().unwrap().contains(A),
            "a capacity refusal names counts and limits, never a project"
        );
        // B is unaffected by A's saturation.
        let (status, _) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/b1?project={B}"),
            Some(transformer("T4")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        // Now the whole host's queue is full, for everyone.
        let (status, global) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/b2?project={B}"),
            Some(transformer("T5")),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(global["scope"], "global");
        // Cancelling one of A's returns the room it held.
        let (status, _) = call(
            app.clone(),
            "POST",
            &format!("/v1/jobs/{}/cancel?project={A}", ids[0]),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(
            app,
            "POST",
            &format!("/v1/transformer-processing/b2?project={B}"),
            Some(transformer("T5")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }

    /// A project named for the first time mid-flight is admitted on the spot
    /// and moves nothing already admitted: a job keeps the project it was
    /// admitted into, and no restart, refresh or directory is involved.
    #[tokio::test]
    async fn a_project_named_mid_flight_is_admitted_and_re_scopes_nothing_queued() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_with(dir.path(), true, limits());
        let mut ids = Vec::new();
        for (key, project, name) in [("a1", A, "T1"), ("b1", B, "T2")] {
            let (status, body) = call(
                app.clone(),
                "POST",
                &format!("/v1/transformer-processing/{key}?project={project}"),
                Some(transformer(name)),
            )
            .await;
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
            ids.push(body["job"]["id"].as_str().unwrap().to_owned());
        }
        // Reading is by the job's own context: the operator sees the work
        // exactly as it was admitted.
        let (status, still) = call(
            app.clone(),
            "GET",
            &format!("/v1/jobs/{}?project={A}", ids[0]),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{still}");
        assert_eq!(still["job"]["context"]["project"], A);
        // A third project, never named before, is admitted without a restart
        // and without anything being fetched.
        let (status, granted) = call(
            app.clone(),
            "POST",
            "/v1/transformer-processing/c1?project=project-c",
            Some(transformer("T4")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{granted}");
        assert_eq!(granted["job"]["context"]["project"], "project-c");
        // B never noticed any of it.
        let (_, b) = call(app, "GET", &format!("/v1/jobs?project={B}"), None).await;
        assert_eq!(b["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(b["jobs"][0]["id"], ids[1]);
    }

    /// The same bytes submitted for two projects are two pieces of work with
    /// two ids and two contexts. Only the caller's key is shared ground, and
    /// sharing it across projects is the refusal proven above.
    #[tokio::test]
    async fn identical_work_in_two_projects_never_collides() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path(), true);
        let mut ids = Vec::new();
        for (key, project) in [("daily-a", A), ("daily-b", B)] {
            let (status, body) = call(
                app.clone(),
                "POST",
                &format!("/v1/transformer-processing/{key}?project={project}"),
                Some(transformer("T1")),
            )
            .await;
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
            assert_eq!(body["job"]["context"]["project"], project);
            ids.push(body["job"]["id"].as_str().unwrap().to_owned());
        }
        assert_ne!(ids[0], ids[1], "one job id per project, not one shared row");
        // Identical input bytes, and still two rows with the same digest.
        let (_, all) = call(app.clone(), "GET", "/v1/jobs", None).await;
        let rows = all["jobs"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["input_sha256"], rows[1]["input_sha256"]);
        // Each is reachable only under its own project.
        for (id, own, other) in [(&ids[0], A, B), (&ids[1], B, A)] {
            let (status, _) = call(
                app.clone(),
                "GET",
                &format!("/v1/jobs/{id}?project={own}"),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let (status, hidden) = call(
                app.clone(),
                "GET",
                &format!("/v1/jobs/{id}?project={other}"),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(hidden["error"], "job not found");
        }
    }

    #[tokio::test]
    async fn the_activity_scope_is_one_entry_per_project_that_holds_work() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path(), true);
        for (key, project, name) in [("a1", A, "T1"), ("b1", B, "T2")] {
            let (status, body) = call(
                app.clone(),
                "POST",
                &format!("/v1/transformer-processing/{key}?project={project}"),
                Some(transformer(name)),
            )
            .await;
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        }
        assert_eq!(
            project_scopes(&app, None).unwrap(),
            vec![A.to_owned(), B.to_owned()]
        );
        assert_eq!(project_scopes(&app, Some(B)).unwrap(), vec![B.to_owned()]);
        assert!(project_scopes(&app, Some("project-z")).unwrap().is_empty());
    }

    /// The Solar route takes the workspace file's PATH and reads it here, as
    /// the desktop's own commands read local files: same machine, same user,
    /// same filesystem. The envelope is still decoded before anything is
    /// admitted, so a missing claim is the caller's malformed input (400).
    #[tokio::test]
    async fn solar_submit_reads_the_named_workspace_file_and_still_requires_the_claim() {
        let dir = tempfile::tempdir().unwrap();
        let envelope = dir.path().join("solar.server-submission.json");
        fs::write(
            &envelope,
            br#"{"schema_version":"ds.solar.server-submission/v1"}"#,
        )
        .unwrap();
        let body = |path: &str| Some(serde_json::to_vec(&json!({ "input_path": path })).unwrap());
        let (status, refused) = call(
            app(dir.path(), true),
            "POST",
            &format!("/v1/solar-processing/job-1?project={A}"),
            body(&envelope.display().to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert!(
            !dir.path().join("store.sqlite").exists(),
            "a missing claim cannot create a compute-only Solar job"
        );
        // A path that is not absolute, and one that is not there at all, are
        // the caller's own answer — never a silent fallback to bytes.
        for path in ["solar.server-submission.json", "/nonexistent/solar.json"] {
            let (status, refused) = call(
                app(dir.path(), true),
                "POST",
                &format!("/v1/solar-processing/job-1?project={A}"),
                body(path),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
            assert!(
                refused["remedy"]
                    .as_str()
                    .unwrap()
                    .contains("absolute path"),
                "{refused}"
            );
        }
        // And the bytes themselves are no longer a body this route accepts.
        let (status, refused) = call(
            app(dir.path(), true),
            "POST",
            &format!("/v1/solar-processing/job-1?project={A}"),
            Some(br#"{"schema_version":"ds.solar.server-submission/v1"}"#.to_vec()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    }

    #[tokio::test]
    async fn activity_route_is_authenticated_and_refuses_prestartup_projection() {
        let dir = tempfile::tempdir().unwrap();
        let unauthorized = router(app(dir.path(), true))
            .oneshot(
                Request::builder()
                    .uri("/v1/activity")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let (status, _) = call(app(dir.path(), true), "GET", "/v1/activity", None).await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    /// One project's Sync Center being unreachable is that project's entry,
    /// not the envelope's. There is no gateway in this test, so EVERY session
    /// refuses to open -- and the answer still names both projects and says of
    /// each why it has no activity, rather than hiding one project's work
    /// behind another project's failure.
    #[tokio::test]
    async fn a_project_whose_projection_fails_does_not_hide_another_projects_work() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(dir.path(), true);
        for (key, project) in [("a1", A), ("b1", B)] {
            runtime::submit(
                &app.database,
                &Admission {
                    identity: app.sessions.identity(),
                    client: "cli:1",
                    key,
                    requested_project: Some(project),
                    limits: app.sessions.limits(),
                    now_ms: runtime::now_ms(),
                },
                &transformer(key),
            )
            .expect("admitted");
        }
        app.activity = Some(
            crate::solar_sync::SolarActivity::open(app.database.clone(), app.sessions.clone())
                .expect("the activity host needs no gateway to exist"),
        );

        let (status, body) = call(app.clone(), "GET", "/v1/activity", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["schema"], ACTIVITY_SCHEMA);
        let entries = body["projects"].as_array().expect("one entry per project");
        assert_eq!(entries.len(), 2, "{body}");
        for (entry, project) in entries.iter().zip([A, B]) {
            assert_eq!(entry["project"], project);
            assert!(
                entry["activity"].is_null(),
                "no gateway, so no projection: {entry}"
            );
            assert!(
                entry["unavailable"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty()),
                "a project with no projection says why, so it cannot be read as `no work`: {entry}"
            );
        }
        // Narrowed, the same answer is about exactly the project named.
        let (status, narrowed) = call(app, "GET", &format!("/v1/activity?project={B}"), None).await;
        assert_eq!(status, StatusCode::OK, "{narrowed}");
        assert_eq!(narrowed["projects"].as_array().expect("entries").len(), 1);
        assert_eq!(narrowed["projects"][0]["project"], B);
    }

    #[tokio::test]
    async fn completed_solar_cancel_keeps_compute_result_and_requires_publication_owner() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("store.sqlite");
        let input = b"sealed Solar input";
        let app = app(dir.path(), true);
        let identity = app.sessions.identity().clone();
        let id = ds_compute_runtime::digest(b"completed-solar-cancel");
        let queued = ds_command_kernel::compute_jobs::Job {
            id: id.clone(),
            owner: identity.owner.clone(),
            lane: "stable".into(),
            input_sha256: ds_compute_runtime::digest(input),
            engine: ds_command_kernel::compute_jobs::EngineKind::SolarPrepared,
            input_tag: "ds.solar.calculate.prepared/v1".into(),
            phase: ds_command_kernel::compute_jobs::Phase::Queued,
            attempts: 0,
            created_at_ms: 1,
            updated_at_ms: 1,
            worker: None,
            lease_until_ms: 0,
            result_sha256: None,
            error: None,
            context: Some(ds_command_kernel::execution_context::ExecutionContext {
                principal_uid: UID.into(),
                lane: "stable".into(),
                deployment: DEPLOYMENT.into(),
                install_id: "install-1".into(),
                project: A.into(),
                client: "test:1".into(),
                operation: "solar_processing".into(),
                job_id: id.clone(),
                idempotency_key: "completed-solar-cancel".into(),
                input_sha256: ds_compute_runtime::digest(input),
                admitted_at_ms: 1,
            }),
        };
        let caller = identity.caller(None);
        let mut store = runtime::open(&database).unwrap();
        store.submit_job(&queued, input, limits()).unwrap();
        let (running, _) = store
            .claim_job(&caller, "worker", 2, 1_000, limits())
            .unwrap()
            .unwrap();
        let result = b"completed solar result";
        let completed = store
            .update_job(
                &caller,
                &running.id,
                Event::Complete {
                    worker: "worker",
                    result_sha256: &runtime::digest(result),
                },
                3,
                Some(result),
            )
            .unwrap();
        drop(store);

        let (status, body) = call(
            app,
            "POST",
            &format!("/v1/jobs/{}/cancel?project={A}", completed.id),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("Solar Sync Center activity")
        );
        let store = runtime::open(&database).unwrap();
        assert_eq!(
            store.job(&caller, &completed.id).unwrap().unwrap().phase,
            ds_command_kernel::compute_jobs::Phase::Completed
        );
        assert_eq!(
            store.job_result(&caller, &completed.id).unwrap().unwrap(),
            result
        );
    }
    /// EVERY route answers with the real authorizer and no gateway anywhere.
    ///
    /// This is the claim the second pass refuted: authorization used to
    /// refresh a credential through the gateway, so a host that lost its
    /// upstream answered 401 to everything within fifteen seconds. The
    /// authorizer here is the production `NativeAuthorizer`; what stands in
    /// for the machine is a credential source that holds a credential and
    /// whose every refresh fails. Nothing may reach for that refresh.
    #[tokio::test]
    async fn every_route_answers_from_the_held_credential_with_no_gateway() {
        let dir = tempfile::tempdir().unwrap();
        let source = crate::auth::tests::FixtureCredential::held("test-owner", "device:first");
        let mut app = offline_app(dir.path(), source.clone());
        app.activity = Some(
            crate::solar_sync::SolarActivity::open(app.database.clone(), app.sessions.clone())
                .expect("the activity host needs no gateway to exist"),
        );

        let (status, submitted) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/offline?project={A}"),
            Some(transformer("T1")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{submitted}");
        let id = submitted["job"]["id"].as_str().unwrap().to_owned();

        // Every job route, the activity envelope and the layer drawer: each
        // one answers, and not one of them answers the authorizer's refusal.
        for (method, uri, expected) in [
            ("GET", "/v1/jobs".to_owned(), StatusCode::OK),
            ("GET", format!("/v1/jobs/{id}?project={A}"), StatusCode::OK),
            (
                "GET",
                format!("/v1/jobs/{id}/input?project={A}"),
                StatusCode::OK,
            ),
            (
                "GET",
                format!("/v1/jobs/{id}/result?project={A}"),
                // Queued, so no result yet — an answer about the job, which
                // is exactly what "the host is working" looks like.
                StatusCode::CONFLICT,
            ),
            ("GET", "/v1/activity".to_owned(), StatusCode::OK),
            (
                "POST",
                format!("/v1/jobs/{id}/cancel?project={A}"),
                StatusCode::OK,
            ),
        ] {
            let (status, body) = call(app.clone(), method, &uri, None).await;
            assert_eq!(status, expected, "{method} {uri}: {body}");
            assert_ne!(
                body["code"], "server_owner_changed",
                "{method} {uri} was stopped by authorization: {body}"
            );
        }
        // The layer drawer reaches its own document source, whose answer on a
        // machine with no login is its own business (`layers::tests` proves
        // that half against a fixture source). What matters here is that
        // authorization let it through.
        let (_, layers) = call(app.clone(), "GET", &format!("/v1/layers?project={A}"), None).await;
        assert_ne!(layers["code"], "server_owner_changed", "{layers}");

        assert_eq!(
            source.refreshes(),
            0,
            "not one request may reach for the gateway"
        );
    }

    /// Losing the gateway mid-run changes no answer, because no answer ever
    /// depended on it: the same requests, before and after, byte for byte.
    #[tokio::test]
    async fn losing_the_gateway_mid_run_changes_no_answer() {
        let dir = tempfile::tempdir().unwrap();
        let source = crate::auth::tests::FixtureCredential::held("test-owner", "device:first");
        let app = offline_app(dir.path(), source.clone());
        let (status, submitted) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/steady?project={A}"),
            Some(transformer("T1")),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{submitted}");
        let before = call(app.clone(), "GET", &format!("/v1/jobs?project={A}"), None).await;
        // The upstream goes away. Nothing local changes: the credential is
        // still the one on disk, and that is the only thing asked about.
        for _ in 0..3 {
            assert!(
                crate::auth::OwnerCredential::refresh(source.as_ref()).is_err(),
                "the gateway is gone"
            );
        }
        let after = call(app.clone(), "GET", &format!("/v1/jobs?project={A}"), None).await;
        assert_eq!(before.0, after.0);
        assert_eq!(before.1, after.1, "the same answer, with no upstream");
    }

    /// The one thing that stops a host, over the wire and by its own name.
    #[tokio::test]
    async fn a_changed_credential_on_disk_stops_the_host_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let source = crate::auth::tests::FixtureCredential::held("test-owner", "device:first");
        let app = offline_app(dir.path(), source.clone());
        let (status, _) = call(app.clone(), "GET", "/v1/jobs", None).await;
        assert_eq!(status, StatusCode::OK);

        source.set(Ok(crate::auth::OwnerAnswer::Held {
            owner: "test-owner".into(),
            credential: "device:second".into(),
        }));
        let (status, stopped) = call(app.clone(), "GET", "/v1/jobs", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{stopped}");
        assert_eq!(stopped["code"], "server_owner_changed");
        assert_eq!(stopped["class"], "unauthorized");
        assert!(
            stopped["error"]
                .as_str()
                .unwrap_or_default()
                .contains("credential changed"),
            "{stopped}"
        );
        assert!(
            stopped["remedy"]
                .as_str()
                .unwrap_or_default()
                .contains("restart"),
            "{stopped}"
        );

        // A machine that cannot answer is NOT that: the host carries on.
        source.set(Err("protected state is locked".into()));
        let (status, _) = call(app, "GET", "/v1/jobs", None).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "could not tell must never become could not serve"
        );
    }

    /// A full door is a typed refusal with a scope of its own, not a bare 429
    /// a caller has to guess about.
    #[tokio::test]
    async fn a_full_door_answers_a_typed_refusal_with_its_own_scope() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(dir.path(), true);
        app.requests = Arc::new(Door::new(1));
        let held = app.requests.enter().expect("the only place in the door");
        let (status, refused) = call(app.clone(), "GET", "/v1/jobs", None).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refused}");
        assert_eq!(refused["code"], "capacity_exhausted");
        assert_eq!(refused["class"], "unavailable");
        assert_eq!(refused["retryable"], true);
        // The door is its own scope: nothing a caller cancels empties it, and
        // it is not the queue the kernel bounds.
        assert_eq!(refused["scope"], "door");
        assert_eq!(refused["retry_after_ms"], 250);
        let remedy = refused["remedy"].as_str().unwrap_or_default();
        assert!(remedy.contains("250 ms"), "{remedy}");
        // The knob it names has to be the knob that works: the door is the
        // larger of `--workers` and the floor, so on any host at or under the
        // floor "use more workers" is advice a caller cannot act on.
        assert!(
            remedy.contains(&format!("above {}", crate::MIN_REQUEST_PERMITS)),
            "{remedy}"
        );
        assert!(
            !refused["error"].as_str().unwrap_or_default().contains(A),
            "a capacity refusal names counts, never a project"
        );
        // And the door clears as requests finish.
        drop(held);
        let (status, _) = call(app, "GET", "/v1/jobs", None).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[test]
    fn the_doors_retry_guidance_is_deterministic_and_bounded() {
        // A quarter second per request already inside it, never zero and
        // never a wait nobody would sit through.
        assert_eq!(Door::retry_after_ms(0), 250);
        assert_eq!(Door::retry_after_ms(1), 250);
        assert_eq!(Door::retry_after_ms(8), 2_000);
        assert_eq!(Door::retry_after_ms(usize::MAX), 60_000);
        assert_eq!(Door::new(8).width(), 8);
    }

    /// A job's own stored input comes back to its owner — including the row
    /// that has no result to read — and to nobody else.
    #[tokio::test]
    async fn a_jobs_stored_input_is_readable_by_its_owner_and_by_nobody_else() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path(), true);
        let bytes = transformer("T1");
        let (status, submitted) = call(
            app.clone(),
            "POST",
            &format!("/v1/transformer-processing/readable?project={A}"),
            Some(bytes.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{submitted}");
        let id = submitted["job"]["id"].as_str().unwrap().to_owned();

        // Exactly the bytes that were admitted, while the job is still queued.
        let (status, returned) = raw(
            app.clone(),
            "GET",
            &format!("/v1/jobs/{id}/input?project={A}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(returned, bytes, "the owner's own request bytes, unchanged");

        // Non-disclosing exactly like the result route: another project's job
        // and an id that never existed are one answer.
        let guessed = "0".repeat(64);
        for uri in [
            format!("/v1/jobs/{id}/input?project={B}"),
            format!("/v1/jobs/{guessed}/input"),
            format!("/v1/jobs/{guessed}/input?project={A}"),
        ] {
            let (status, hidden) = call(app.clone(), "GET", &uri, None).await;
            assert_eq!(status, StatusCode::CONFLICT, "{uri}: {hidden}");
            assert_eq!(hidden["code"], "not_visible");
            assert_eq!(hidden["error"], "job not found");
        }
    }

    /// A project with more durable Solar work than one projection covers is
    /// told so — and every other project's answer is unaffected. The bound
    /// that used to be a hard failure at 4096 rows is now a page and a flag.
    #[tokio::test]
    async fn a_project_with_more_solar_rows_than_a_projection_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(dir.path(), true);
        seed_solar_rows(&app, A, 5_000);
        seed_solar_rows(&app, B, 3);
        app.activity = Some(
            crate::solar_sync::SolarActivity::open(app.database.clone(), app.sessions.clone())
                .expect("the activity host needs no gateway to exist"),
        );
        let activity = app.activity.clone().expect("activity");
        assert!(
            activity.projection_truncated(A).expect("a bounded read"),
            "5000 rows is more than one projection covers"
        );
        assert!(!activity.projection_truncated(B).expect("a bounded read"));

        // And the envelope says it, per project, whether or not that
        // project's Sync Center could be read (there is no gateway here).
        let (status, body) = call(app.clone(), "GET", "/v1/activity", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let entries = body["projects"].as_array().expect("one entry per project");
        assert_eq!(entries.len(), 2, "{body}");
        for entry in entries {
            let more = entry["project"] == A;
            assert_eq!(entry["more"], json!(more), "{entry}");
            assert!(entry["unavailable"].is_string(), "{entry}");
        }

        // And `more` is a bound on the PROJECTION, not on the record: the
        // oldest row on the host — four and a half thousand rows past where
        // the projection stops — is still its owner's to read by id, status
        // and stored input alike. That is the sentence the reference makes,
        // so it is the sentence this asserts.
        let oldest = runtime::digest(format!("{A}-0").as_bytes());
        let (status, job) = call(
            app.clone(),
            "GET",
            &format!("/v1/jobs/{oldest}?project={A}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{job}");
        assert_eq!(job["job"]["context"]["project"], A);
        let (status, input) = raw(
            app.clone(),
            "GET",
            &format!("/v1/jobs/{oldest}/input?project={A}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            input,
            format!("{{\"city\":\"{A}-0\"}}").into_bytes(),
            "a row past the projection is read by id, byte for byte"
        );
        // …and it is still nobody else's.
        let (status, hidden) = call(
            app,
            "GET",
            &format!("/v1/jobs/{oldest}/input?project={B}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{hidden}");
        assert_eq!(hidden["error"], "job not found");
    }

    /// Write `count` durable Solar rows for one project the way a Server that
    /// has been running for months would have accumulated them.
    ///
    /// Directly, in one transaction, because the point is the SIZE: submitting
    /// five thousand rows through the queue would spend the test's whole life
    /// on capacity censuses and fsyncs, and prove nothing this does not.
    fn seed_solar_rows(app: &App, project: &str, count: usize) {
        use ds_command_kernel::compute_jobs::{EngineKind, Job, Phase};
        use ds_command_kernel::execution_context::ExecutionContext;
        // Create the schema through the store itself, so the rows land in the
        // table the store reads and not in one this test invented.
        runtime::open(&app.database).expect("durable store");
        let mut connection =
            rusqlite::Connection::open(&app.database).expect("the same protected database");
        let transaction = connection.transaction().expect("one write");
        for index in 0..count {
            let id = runtime::digest(format!("{project}-{index}").as_bytes());
            let input = format!("{{\"city\":\"{project}-{index}\"}}");
            let digest = runtime::digest(input.as_bytes());
            let created = 1_000 + index as u64;
            let job = Job {
                id: id.clone(),
                owner: app.connection.owner.clone(),
                lane: app.connection.lane.clone(),
                input_sha256: digest.clone(),
                engine: EngineKind::SolarPrepared,
                input_tag: "ds.solar.calculate.prepared/v1".into(),
                phase: Phase::Completed,
                attempts: 1,
                created_at_ms: created,
                updated_at_ms: created,
                worker: None,
                lease_until_ms: 0,
                result_sha256: Some(digest.clone()),
                error: None,
                context: Some(ExecutionContext {
                    principal_uid: UID.into(),
                    lane: app.connection.lane.clone(),
                    deployment: DEPLOYMENT.into(),
                    install_id: "install-1".into(),
                    project: project.to_owned(),
                    client: "test:1".into(),
                    operation: "solar_processing".into(),
                    job_id: id.clone(),
                    idempotency_key: format!("seed-{index}"),
                    input_sha256: digest,
                    admitted_at_ms: created,
                }),
            };
            transaction
                .execute(
                    "INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES (?1,?2,?3,'completed',0,?4,?5,?6)",
                    rusqlite::params![
                        job.id,
                        job.owner,
                        job.lane,
                        job.created_at_ms,
                        serde_json::to_string(&job).expect("row"),
                        input.as_bytes(),
                    ],
                )
                .expect("seeded row");
        }
        transaction.commit().expect("seeded queue");
    }

    /// The prepared input `ds solar prepare` seals for the fixture city,
    /// with the snapshot provenance a publication needs: what a Server is
    /// handed, built the same way the compute runtime's own lifecycle proof
    /// builds it.
    fn prepared_solar_request(project: &str, run_id: &str) -> Vec<u8> {
        use ds_solar_contracts::{
            BundleBytes, CityIdentity, PreparedSolarCityInput, RunOptions, SnapshotRevision,
            SolarCityInput, WeatherDataset, WeatherKey, WeatherPin,
        };
        let root =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-solar/fixtures");
        let city = "aderm_bere";
        let snapshot: Value = serde_json::from_slice(
            &fs::read(root.join(format!("city/{city}.snapshot.json"))).unwrap(),
        )
        .unwrap();
        let input = SolarCityInput::from_city_snapshot(
            CityIdentity {
                project_id: project.to_string(),
                root: "solar".to_string(),
                city_id: city.to_string(),
                display_name: snapshot["_root"]["display_name"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            },
            SnapshotRevision {
                source: "ds-brain".to_string(),
                captured_at: "2026-08-08T00:00:00Z".to_string(),
                record_updated_at_ms: None,
                revision: Some("a".repeat(64)),
            },
            RunOptions::default(),
            &snapshot,
        )
        .unwrap();
        let weather: WeatherDataset = serde_json::from_slice(
            &fs::read(root.join(format!("weather/{city}.weather.json"))).unwrap(),
        )
        .unwrap();
        let weather_key = WeatherKey::new(
            input.site.latitude,
            input.site.longitude,
            weather.provenance.timezone.clone(),
            WeatherPin::pvgis_tmy(),
        )
        .unwrap();
        let reference_root = root.join("reference").join(city);
        let reference = ds_solar_contracts::reference_unit::open(&BundleBytes {
            weather_json: fs::read(reference_root.join("weather.json")).unwrap(),
            reference_unit_json: fs::read(reference_root.join("reference-unit.json")).unwrap(),
            reference_unit_parquet: fs::read(reference_root.join("reference-unit.parquet"))
                .unwrap(),
            manifest_json: fs::read(reference_root.join("manifest.json")).unwrap(),
        })
        .unwrap()
        .reference_unit;
        let prepared = PreparedSolarCityInput::commit(
            input,
            weather_key,
            weather,
            reference,
            "2026-08-20T00:00:00Z",
        )
        .unwrap();
        serde_json::to_vec(&json!({
            "prepared": prepared,
            "render_charts": false,
            "run_id": run_id,
            "provenance": {
                "project_id": project, "root": "solar", "template_id": city,
                "input_base_fingerprint": "a".repeat(64), "source_snapshot_sha256": "b".repeat(64),
                "snapshot_receipt_id": "550e8400-e29b-51d4-a716-446655440000"
            }
        }))
        .unwrap()
    }

    /// A completed prepared Solar job seals its row into the sync store in
    /// the completion's own step — under this host's local fence, with no
    /// session and no gateway — so the queue holds it before any pump runs,
    /// and the producer's inventory offers nothing to adopt for it. The
    /// engine is the real one over the fixture city.
    #[test]
    fn a_completed_solar_job_seals_its_row_with_no_session_and_the_pump_adopts_nothing() {
        use ds_command_kernel::sync_store::ArtifactState;
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path(), true);
        let identity = app.sessions.identity().clone();
        let activity =
            crate::solar_sync::SolarActivity::open(app.database.clone(), app.sessions.clone())
                .expect("the activity host needs no gateway to exist");
        let bytes = prepared_solar_request(A, "sealed-at-completion");
        let job = runtime::submit_solar(
            &app.database,
            &Admission {
                identity: &identity,
                client: "test:1",
                key: "solar-seal",
                requested_project: None,
                limits: limits(),
                now_ms: runtime::now_ms(),
            },
            &bytes,
        )
        .expect("admitted");
        let fence = crate::server_sync::fence_of(&identity);
        let scope = ds_sync_runtime::rows::store_scope(A);
        let rows = |database: &Path| {
            ds_sync_store::Store::open(database)
                .unwrap()
                .snapshot(&fence, &scope)
                .unwrap()
                .artifacts
        };
        assert!(
            rows(&app.database).is_empty(),
            "queued work is not a publication"
        );

        let context = runtime::WorkerContext {
            path: app.database.clone(),
            identity: identity.clone(),
            limits: limits(),
            auth: app.auth.clone(),
            observer: Some(activity.clone()),
        };
        assert!(runtime::run_one(&context, "worker").expect("the real engine runs"));

        // The row: held, keyed by the job, naming its bytes in this file.
        let held = rows(&app.database);
        assert_eq!(held.len(), 1, "{held:?}");
        assert_eq!(held[0].replay_key, job.id);
        assert_eq!(held[0].client_publish_id, job.id);
        assert_eq!(held[0].state, ArtifactState::Held);
        assert_eq!(
            held[0].identity,
            ds_sync_runtime::solar::identity_of("aderm_bere")
        );
        assert_eq!(
            held[0].bytes_locator,
            format!("solar:prepared:job:{}", job.id)
        );
        assert_eq!(
            held[0].base_revision.as_deref(),
            Some("a".repeat(64).as_str())
        );
        assert_eq!(held[0].outputs.len(), 1);
        assert_eq!(held[0].outputs[0].output_id, "report-input");
        let result = runtime::open(&app.database)
            .unwrap()
            .job_result(&identity.caller(Some(A)), &job.id)
            .unwrap()
            .expect("the result is the bytes");
        let publication = runtime::solar_publication(
            &runtime::open(&app.database)
                .unwrap()
                .job(&identity.caller(None), &job.id)
                .unwrap()
                .unwrap(),
            &bytes,
            &result,
        )
        .unwrap();
        assert_eq!(held[0].outputs[0].sha256, publication.outputs[0].sha256);
        // The queue holds it, read with no session and no identity.
        let queue = ds_sync_store::Store::open_read_only(&app.database)
            .unwrap()
            .expect("the store exists")
            .queue_all(Some(A), runtime::now_ms())
            .unwrap();
        assert_eq!(queue.queued_batches, 1);
        assert_eq!(queue.queued_bytes, held[0].size_bytes);
        // The seal is evidence-free: no adoption, no receipt, nothing but
        // the row (the first computation of a city supersedes nothing).
        let receipts = ds_sync_store::Store::open(&app.database)
            .unwrap()
            .receipts(&fence, &scope, 10)
            .unwrap();
        assert!(receipts.is_empty(), "{receipts:?}");
        // The producer's observation offers nothing (the seal is the only
        // way a row is born) and retires nothing: the sealed job's result
        // stays, because its row is in the store.
        let (retired, offered) = activity.observe_for_test(A).unwrap();
        assert!(retired.is_empty(), "{retired:?}");
        assert!(offered.is_empty());
        assert!(
            runtime::open(&app.database)
                .unwrap()
                .job_result(&identity.caller(Some(A)), &job.id)
                .unwrap()
                .is_some(),
            "a sealed job keeps its result"
        );
        // The completion projected again (the same worker step, replayed)
        // changes nothing: already recorded, one row.
        let completed = runtime::open(&app.database)
            .unwrap()
            .job(&identity.caller(None), &job.id)
            .unwrap()
            .unwrap();
        ds_compute_runtime::CompletionObserver::completed(&*activity, &completed)
            .expect("already recorded is not an error");
        assert_eq!(rows(&app.database).len(), 1);
    }

    /// A completed job the store holds no row for — completed before the
    /// seal existed, or one whose seal the store could not record — is NOT
    /// a publication (owner, 2026-09-20: pre-seal Solar jobs are not
    /// migrated). The producer offers nothing for it; its next observation
    /// retires the job's result (the job row stays as evidence), the store
    /// holds no row and the queue counts nothing, and a second observation
    /// has nothing left to retire. The job can be run again.
    #[test]
    fn a_solar_job_completed_with_no_row_is_not_adopted_and_its_result_is_retired_on_observation() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path(), true);
        let identity = app.sessions.identity().clone();
        let bytes = prepared_solar_request(A, "before-the-seal");
        let job = runtime::submit_solar(
            &app.database,
            &Admission {
                identity: &identity,
                client: "test:1",
                key: "solar-unsealed",
                requested_project: None,
                limits: limits(),
                now_ms: runtime::now_ms(),
            },
            &bytes,
        )
        .expect("admitted");
        // Completed with no observer: the way every release before the seal
        // completed a Solar job — the result is durable, the row is not.
        let context = runtime::WorkerContext {
            path: app.database.clone(),
            identity: identity.clone(),
            limits: limits(),
            auth: app.auth.clone(),
            observer: None,
        };
        assert!(runtime::run_one(&context, "worker").expect("the real engine runs"));
        let result = |id: &str| {
            runtime::open(&app.database)
                .unwrap()
                .job_result(&identity.caller(Some(A)), id)
                .unwrap()
        };
        let result_bytes = result(&job.id).expect("the result is durable").len() as u64;

        let activity =
            crate::solar_sync::SolarActivity::open(app.database.clone(), app.sessions.clone())
                .expect("the activity host needs no gateway to exist");
        let (retired, offered) = activity.observe_for_test(A).unwrap();
        assert!(offered.is_empty(), "nothing is adopted: {offered:?}");
        assert_eq!(retired, vec![(job.id.clone(), result_bytes)]);
        assert!(result(&job.id).is_none(), "the unsealed result left");
        let completed = runtime::open(&app.database)
            .unwrap()
            .job(&identity.caller(None), &job.id)
            .unwrap()
            .expect("the job row stays as evidence");
        assert_eq!(
            completed.phase,
            ds_command_kernel::compute_jobs::Phase::Completed
        );
        assert!(completed.result_sha256.is_some());

        let fence = crate::server_sync::fence_of(&identity);
        let scope = ds_sync_runtime::rows::store_scope(A);
        let store = ds_sync_store::Store::open(&app.database).unwrap();
        assert!(
            store.snapshot(&fence, &scope).unwrap().artifacts.is_empty(),
            "no row was born for it"
        );
        assert!(store.receipts(&fence, &scope, 10).unwrap().is_empty());
        let queue = ds_sync_store::Store::open_read_only(&app.database)
            .unwrap()
            .expect("the store exists")
            .queue_all(Some(A), runtime::now_ms())
            .unwrap();
        assert_eq!(queue.queued_batches, 0);

        // Observed again: nothing left to retire, nothing offered.
        let (retired, offered) = activity.observe_for_test(A).unwrap();
        assert!(retired.is_empty() && offered.is_empty());

        // The completion observed now — the seal on a job whose result is
        // gone — seals nothing and errs nothing: there is nothing to seal.
        ds_compute_runtime::CompletionObserver::completed(&*activity, &completed)
            .expect("a result already gone has nothing to seal");
        assert!(
            ds_sync_store::Store::open(&app.database)
                .unwrap()
                .snapshot(&fence, &scope)
                .unwrap()
                .artifacts
                .is_empty()
        );
    }

    /// A restart replays NO completion: two jobs of one city completed with
    /// no row (before the seal existed) are two results the next
    /// observation retires, and neither becomes the city's row. The seal's
    /// own order-independence for two live completions of one city is the
    /// kernel harness's proof
    /// (`a_solar_completion_sealed_behind_a_newer_one_frees_its_own_result_and_the_newer_row_stands`).
    #[test]
    fn a_restart_replays_no_completion_and_unsealed_results_of_one_city_are_all_retired() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(dir.path(), true);
        let identity = app.sessions.identity().clone();
        let submitted_at = runtime::now_ms();
        let mut unsealed = Vec::new();
        for (index, run_id) in ["unsealed-older", "unsealed-newer"].into_iter().enumerate() {
            let bytes = prepared_solar_request(A, run_id);
            let job = runtime::submit_solar(
                &app.database,
                &Admission {
                    identity: &identity,
                    client: "test:1",
                    key: run_id,
                    requested_project: None,
                    limits: limits(),
                    now_ms: submitted_at + index as u64 * 10,
                },
                &bytes,
            )
            .expect("admitted");
            let context = runtime::WorkerContext {
                path: app.database.clone(),
                identity: identity.clone(),
                limits: limits(),
                auth: app.auth.clone(),
                observer: None,
            };
            assert!(runtime::run_one(&context, "worker").expect("the real engine runs"));
            unsealed.push(job.id);
        }
        let activity =
            crate::solar_sync::SolarActivity::open(app.database.clone(), app.sessions.clone())
                .expect("the activity host needs no gateway to exist");
        // The restart: `recover` wakes the pump and replays nothing.
        ds_compute_runtime::CompletionObserver::recover(&*activity).unwrap();
        let fence = crate::server_sync::fence_of(&identity);
        let scope = ds_sync_runtime::rows::store_scope(A);
        assert!(
            ds_sync_store::Store::open(&app.database)
                .unwrap()
                .snapshot(&fence, &scope)
                .unwrap()
                .artifacts
                .is_empty(),
            "a restart seals nothing"
        );
        // The pump's observation: both results retired, newest first, no row.
        let (retired, offered) = activity.observe_for_test(A).unwrap();
        assert!(offered.is_empty());
        assert_eq!(
            retired
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            vec![unsealed[1].as_str(), unsealed[0].as_str()]
        );
        assert!(retired.iter().all(|(_, bytes)| *bytes > 0));
        for id in &unsealed {
            assert!(
                runtime::open(&app.database)
                    .unwrap()
                    .job_result(&identity.caller(Some(A)), id)
                    .unwrap()
                    .is_none(),
                "{id}: retired"
            );
        }
        let store = ds_sync_store::Store::open(&app.database).unwrap();
        assert!(store.snapshot(&fence, &scope).unwrap().artifacts.is_empty());
        assert!(store.receipts(&fence, &scope, 10).unwrap().is_empty());
        assert_eq!(
            ds_sync_store::Store::open_read_only(&app.database)
                .unwrap()
                .unwrap()
                .queue_all(Some(A), runtime::now_ms())
                .unwrap()
                .queued_batches,
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn owner_only_connection_survives_restart_and_refuses_identity_switch() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let first = super::connection(dir.path(), "owner".into(), "stable".into()).unwrap();
        assert_eq!(first.socket, dir.path().join("server.sock"));
        assert_eq!(first.legacy_address, None);
        // What is on disk names the owner and the lane, and nothing that
        // opens the door: there is no bearer left to keep.
        let recorded: Value =
            serde_json::from_slice(&fs::read(dir.path().join("connection.json")).unwrap()).unwrap();
        assert_eq!(
            recorded,
            json!({"schema": CONNECTION_SCHEMA, "owner": "owner", "lane": "stable"})
        );
        // The same owner, restarted: the same protected connection.
        let again = super::connection(dir.path(), "owner".into(), "stable".into()).unwrap();
        assert_eq!(again.owner, first.owner);
        assert_eq!(load_connection(dir.path()).unwrap().socket, first.socket);
        // A SECOND ACCOUNT pointed at one Server's protected state is the one
        // place many users meet one host, and it is answered by its own name
        // with the remedy that is the whole model: a host of one's own.
        let second_account = super::connection(dir.path(), "other".into(), "stable".into())
            .expect_err("one Server serves one owner");
        assert_eq!(second_account.code(), MULTI_PRINCIPAL_UNSUPPORTED);
        assert_eq!(second_account.class(), ExitClass::Conflict);
        assert!(
            second_account
                .remedy_text()
                .is_some_and(|remedy| remedy.contains("--state-dir")),
            "the remedy is a host of its own: {second_account:?}"
        );
        // The same owner on another lane is that owner's own
        // misconfiguration, and says so instead of accusing them of being
        // somebody else.
        let other_lane = super::connection(dir.path(), "owner".into(), "canary".into())
            .expect_err("one state directory, one lane");
        assert_eq!(other_lane.code(), "server_refused");
        fs::set_permissions(
            dir.path().join("connection.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(load_connection(dir.path()).is_err());
    }

    /// The record a `ds` from before the socket left: an address and a bearer
    /// that never rotated. It still says whose state this is; it is never a
    /// door. The next host adopts it — unless the old one may still be
    /// running — and takes the bearer off the disk.
    #[cfg(unix)]
    #[test]
    fn a_record_from_before_the_socket_is_adopted_and_its_bearer_retired() {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        // An older Server still listening where the record says it does.
        let older = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = older.local_addr().unwrap();
        let bearer = "d".repeat(64);
        let legacy =
            json!({"address": address, "owner": "owner", "lane": "stable", "token": bearer});
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dir.path().join("connection.json"))
            .unwrap();
        file.write_all(&serde_json::to_vec(&legacy).unwrap())
            .unwrap();
        drop(file);

        // A client reads whose it is and where it was, and nothing more.
        let read = load_connection(dir.path()).unwrap();
        assert_eq!(read.owner, "owner");
        assert_eq!(read.legacy_address, Some(address));
        assert!(
            !format!("{read:?}").contains(&bearer),
            "the bearer is never read"
        );

        // Another account is still another account, whatever wrote the record.
        let stranger = super::connection(dir.path(), "other".into(), "stable".into())
            .expect_err("one Server serves one owner");
        assert_eq!(stranger.code(), MULTI_PRINCIPAL_UNSUPPORTED);

        // The older Server may still be running: two hosts on one queue is
        // refused, and the record stays as it was.
        let running = super::connection(dir.path(), "owner".into(), "stable".into())
            .expect_err("an older host may still be serving this state");
        assert_eq!(running.code(), "server_refused");
        assert!(
            running.message().contains(&address.to_string()),
            "{running:?}"
        );
        assert!(!running.message().contains(&bearer));
        assert!(
            fs::read_to_string(dir.path().join("connection.json"))
                .unwrap()
                .contains(&bearer)
        );

        // It stopped: the record is adopted and rewritten without the bearer.
        drop(older);
        let adopted = super::connection(dir.path(), "owner".into(), "stable".into()).unwrap();
        assert_eq!(adopted.legacy_address, None);
        let on_disk = fs::read_to_string(dir.path().join("connection.json")).unwrap();
        assert!(
            !on_disk.contains(&bearer),
            "the retired bearer is off the disk"
        );
        assert!(!on_disk.contains("token"), "{on_disk}");
        let meta = fs::metadata(dir.path().join("connection.json")).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        assert!(!dir.path().join("connection.json.new").exists());
        assert_eq!(load_connection(dir.path()).unwrap().legacy_address, None);
    }

    /// The Server's protected directory is private all the way down: state
    /// written before the owner rule (a 0644 store, a 0755 cache) is
    /// tightened when the Server prepares the directory.
    #[cfg(unix)]
    #[test]
    fn preparing_the_state_directory_tightens_what_is_already_there() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let cache = dir.path().join("tile-cache");
        fs::create_dir(&cache).unwrap();
        fs::set_permissions(&cache, fs::Permissions::from_mode(0o755)).unwrap();
        let store = dir.path().join("store.sqlite");
        fs::write(&store, b"").unwrap();
        fs::set_permissions(&store, fs::Permissions::from_mode(0o644)).unwrap();
        prepare_directory(dir.path()).unwrap();
        assert_eq!(mode(&cache), 0o700);
        assert_eq!(mode(&store), 0o600);
    }
}
