//! The Server's HTTP surface: the protected loopback door onto one
//! authenticated account's durable compute, publication and layer state.
//!
//! Every route that acts asks `ServerSessions` to admit it, and the kernel
//! decides which project the operation is about. Every route that reads
//! answers through the job's own execution context, so a job in a project the
//! caller did not name is absent in exactly the way an invented id is absent.
//! The loopback boundary is unchanged: one owner-only bearer, one
//! re-authorized native account, one fixed loopback port.

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

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub address: SocketAddr,
    pub owner: String,
    pub lane: String,
    pub token: String,
}
#[derive(Clone)]
pub struct App {
    pub database: PathBuf,
    pub connection: Connection,
    pub auth: Arc<dyn Authorizer>,
    pub requests: Arc<tokio::sync::Semaphore>,
    pub activity: Option<Arc<crate::solar_sync::SolarActivity>>,
    /// The layer drawer's document source and preference root for this host.
    pub layers: Arc<dyn crate::layers::LayerHost>,
    /// One authenticated owner, many of its projects: the admission door and
    /// the per-project sessions. No directory lives here.
    pub sessions: Arc<ServerSessions>,
}

type ApiError = (StatusCode, Json<Value>);

fn error(status: StatusCode, message: impl ToString) -> ApiError {
    (status, Json(json!({"error":message.to_string()})))
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

fn authorize(app: &App, headers: &HeaderMap) -> Result<(), ApiError> {
    let supplied = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let wanted = app.connection.token.as_bytes();
    let supplied = supplied.as_bytes();
    let mismatch = wanted
        .iter()
        .enumerate()
        .fold(wanted.len() ^ supplied.len(), |acc, (i, b)| {
            acc | usize::from(*b ^ supplied.get(i).copied().unwrap_or(0))
        });
    // One Server, one owner: this bearer is that owner's, and there is no
    // second identity for a request to name. Anything else is denied here,
    // and that IS the rule -- many users are many machines.
    if mismatch != 0 {
        return Err(error(StatusCode::UNAUTHORIZED, "server access denied"));
    }
    app.auth
        .authorize(&app.connection.owner)
        .map_err(|e| error(StatusCode::UNAUTHORIZED, e))
}

pub fn router(app: App) -> Router {
    Router::new()
        .route("/v1/jobs", get(list))
        .route("/v1/jobs/:id", get(status))
        .route("/v1/jobs/:id/cancel", post(cancel))
        .route("/v1/jobs/:id/result", get(result))
        .route("/v1/activity", get(activity))
        .route("/v1/layers", get(crate::layers::list))
        .route("/v1/layers/visibility", post(crate::layers::visibility))
        .route("/v1/layers/order", post(crate::layers::order))
        .route("/v1/transformer-processing/:key", post(submit))
        .route("/v1/solar-processing/:key", post(submit_solar))
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
    let headers = request.headers().clone();
    let permit = app.requests.clone().try_acquire_owned().map_err(|_| {
        error(
            StatusCode::TOO_MANY_REQUESTS,
            "server request capacity reached; retry later",
        )
    })?;
    tokio::task::spawn_blocking(move || authorize(&app, &headers))
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "authorization worker unavailable",
            )
        })??;
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

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "native task failed"))?
        .map_err(|e| error(StatusCode::CONFLICT, e))
}
/// The same queue, for work that answers with the kernel's own refusals.
async fn admitting<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "native task failed"))?
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

fn project_query(query: Option<Query<ProjectQuery>>) -> Result<Option<String>, ApiError> {
    let Some(Query(query)) = query else {
        return Err(typed(
            &Failure::invalid(
                "invalid_input",
                "this route accepts an optional project query parameter only",
            )
            .remedy("send ?project=<exact-id>"),
        ));
    };
    Ok(query.project)
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
/// Fast LV decode from becoming an alternate engine-dispatch authority.
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
/// `GET /v1/activity[?project=<id>]` — one envelope, one entry per project
/// this connection has durable work in. Without a project it reports every
/// one of them; with a project it reports that one, or nothing at all when
/// the project holds nothing this caller may see.
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
            projects.push(json!({"project": scope, "activity": activity.store_read(&scope)?}));
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
pub fn project_scopes(app: &App, project: Option<&str>) -> Result<Vec<String>, String> {
    let identity = app.sessions.identity();
    let mut scopes: BTreeSet<String> = runtime::open(&app.database)?
        .jobs(&identity.caller(project), 1000)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter_map(|job| job.context.map(|context| context.project))
        .collect();
    scopes.extend(
        crate::server_reports::projects_with_publications(&app.database)?
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
        saved_project: None,
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
    protected(path, true)
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
    let connection: Connection = serde_json::from_slice(&bytes)
        .map_err(|_| "invalid protected server connection".to_string())?;
    if !connection.address.ip().is_loopback()
        || !ds_command_kernel::compute_jobs::digest(&connection.token)
    {
        return Err("invalid server address or token".into());
    }
    Ok(connection)
}
/// One Server serves one owner, and this is the sentence a second account
/// meets when it asks one Server to serve it too: the protected state it
/// pointed at is already another account's. Many users are many machines, so
/// the remedy is a host of one's own, never a second identity in this process.
pub const MULTI_PRINCIPAL_UNSUPPORTED: &str = "multi_principal_unsupported";

pub fn connection(
    directory: &Path,
    address: SocketAddr,
    owner: String,
    lane: String,
) -> Result<Connection, Failure> {
    prepare_directory(directory).map_err(host_failure)?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(Failure::invalid(
            "server_refused",
            "server must listen on a fixed loopback port",
        )
        .remedy("pass --listen 127.0.0.1:<port>"));
    }
    let path = directory.join("connection.json");
    if path.exists() {
        let existing = load_connection(directory).map_err(host_failure)?;
        // A second ACCOUNT is the one thing this Server can never become, so
        // it is answered by its own name rather than as a generic refusal —
        // and separately from a lane or address that simply does not match,
        // which is one owner's own misconfiguration.
        if existing.owner != owner {
            return Err(Failure::conflict(
                // Written out, like `needs_paired_map` above: the
                // refusal-coverage scan reads a literal, and a code it cannot
                // read is a code nothing checks is documented.
                "multi_principal_unsupported",
                "this protected server state belongs to another account; one Server serves exactly one owner",
            )
            .remedy(
                "run that account its own ds server serve, with its own --state-dir and --listen",
            ));
        }
        if existing.lane != lane || existing.address != address {
            return Err(host_failure(
                "the existing server connection is on another lane or address; use a separate state directory",
            ));
        }
        return Ok(existing);
    }
    let mut secret = [0u8; 32];
    getrandom::getrandom(&mut secret).map_err(host_failure)?;
    let connection = Connection {
        address,
        owner,
        lane,
        token: runtime::digest(&secret),
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(host_failure)?;
    file.write_all(&serde_json::to_vec(&connection).map_err(host_failure)?)
        .map_err(host_failure)?;
    file.sync_all().map_err(host_failure)?;
    #[cfg(unix)]
    fs::File::open(directory)
        .and_then(|d| d.sync_all())
        .map_err(host_failure)?;
    Ok(connection)
}
pub async fn serve(mut app: App, workers: usize) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(app.connection.address)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                format!("{} already has a listener; use the running server, stop that instance, or select a different --listen and --state-dir", app.connection.address)
            } else {
                format!("cannot bind {}: {e}", app.connection.address)
            }
        })?;
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
        "DS server ready at {} (protected owner access)",
        app.connection.address
    );
    let result = axum::serve(listener, router(app))
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                if let Ok(mut terminate) =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                {
                    tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}}
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
        })
        .await
        .map_err(|e| e.to_string());
    workers.stop();
    solar_pump.stop();
    drop(workers);
    drop(solar_pump);
    result
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

    pub(crate) fn test_connection(address: SocketAddr) -> Connection {
        Connection {
            address,
            owner: "test-owner".into(),
            lane: "stable".into(),
            token: "a".repeat(64),
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
    /// A Server over `path` with no directory, no snapshot and no upstream:
    /// exactly what production has.
    fn app_with(path: &Path, authorized: bool, limits: Limits) -> App {
        let connection = test_connection("127.0.0.1:19766".parse().unwrap());
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
            auth: Arc::new(Auth(authorized)),
            requests: Arc::new(tokio::sync::Semaphore::new(4)),
            activity: None,
            layers: crate::layers::NativeLayerHost::fixture_native("stable"),
            sessions,
        }
    }
    fn transformer(name: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"schema":"ds.fast-lv.request/v1","jobs":[{"transformer_name":name,"gdfs":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr-1","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":name,"names":name}}]},"lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line-1","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]},"customers":{"type":"FeatureCollection","features":[]}},"settings":{}}]})).unwrap()
    }
    /// One request straight at the router, with the owner bearer.
    async fn call(app: App, method: &str, uri: &str, body: Option<Vec<u8>>) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {}", "a".repeat(64)))
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

    #[tokio::test]
    async fn unauthenticated_or_revoked_calls_never_read_or_create_jobs() {
        let dir = tempfile::tempdir().unwrap();
        for (allowed, token) in [(true, "wrong".to_owned()), (false, "a".repeat(64))] {
            let response = router(app(dir.path(), allowed))
                .oneshot(
                    Request::builder()
                        .uri("/v1/jobs")
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
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

    /// One owner per Server, enforced by the bearer and by nothing else.
    /// There is no header that names an account and no second identity to
    /// distinguish: anything but this owner's bearer is denied at the door,
    /// and that is the whole of the rule. Many users are many machines.
    #[tokio::test]
    async fn one_owner_per_server_is_the_bearer_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let denied = |token: String| {
            let app = app(dir.path(), true);
            async move {
                router(app)
                    .oneshot(
                        Request::builder()
                            .uri("/v1/jobs")
                            .header("authorization", format!("Bearer {token}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap()
            }
        };
        let response = denied("b".repeat(64)).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"], "server access denied");
        // A header naming an account is not a thing this host reads.
        let response = router(app(dir.path(), true))
            .oneshot(
                Request::builder()
                    .uri("/v1/jobs")
                    .header("authorization", format!("Bearer {}", "a".repeat(64)))
                    .header("x-ds-principal", "uid-somebody-else")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "the owner's bearer is the only question asked"
        );
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
    #[cfg(unix)]
    #[test]
    fn owner_only_connection_survives_restart_and_refuses_identity_switch() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let first = super::connection(
            dir.path(),
            "127.0.0.1:19766".parse().unwrap(),
            "owner".into(),
            "stable".into(),
        )
        .unwrap();
        assert_eq!(load_connection(dir.path()).unwrap().token, first.token);
        // The same owner, restarted: the same protected connection, so a
        // running host's bearer survives a restart rather than rotating.
        assert_eq!(
            super::connection(dir.path(), first.address, "owner".into(), "stable".into())
                .unwrap()
                .token,
            first.token
        );
        // A SECOND ACCOUNT pointed at one Server's protected state is the one
        // place many users meet one host, and it is answered by its own name
        // with the remedy that is the whole model: a host of one's own.
        // `.err()` rather than `expect_err`: `Connection` has no `Debug` on
        // purpose, because a panic message must never carry the bearer.
        let second_account =
            super::connection(dir.path(), first.address, "other".into(), "stable".into())
                .err()
                .expect("one Server serves one owner");
        assert_eq!(second_account.code(), MULTI_PRINCIPAL_UNSUPPORTED);
        assert_eq!(second_account.class(), ExitClass::Conflict);
        assert!(
            second_account
                .remedy_text()
                .is_some_and(|remedy| remedy.contains("--state-dir")),
            "the remedy is a host of its own: {second_account:?}"
        );
        // The same owner on another lane or address is that owner's own
        // misconfiguration, and says so instead of accusing them of being
        // somebody else.
        let other_lane =
            super::connection(dir.path(), first.address, "owner".into(), "canary".into())
                .err()
                .expect("one state directory, one lane");
        assert_eq!(other_lane.code(), "server_refused");
        fs::set_permissions(
            dir.path().join("connection.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(load_connection(dir.path()).is_err());
    }
}
