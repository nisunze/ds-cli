use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path as Param, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use ds_command_kernel::compute_jobs::Event;
use ds_compute_runtime::{self as runtime, Authorizer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

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
}

type ApiError = (StatusCode, Json<Value>);

fn error(status: StatusCode, message: impl ToString) -> ApiError {
    (status, Json(json!({"error":message.to_string()})))
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
        .route("/v1/transformer-processing/:key", post(submit))
        .route("/v1/solar-processing/:key", post(submit_solar))
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
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "native task failed"))?
        .map_err(|e| error(StatusCode::CONFLICT, e))
}
async fn list(State(app): State<App>, _headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    blocking(move || {
        let rows = runtime::open(&app.database)?
            .jobs(&app.connection.owner, &app.connection.lane, 101)
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
) -> Result<Json<Value>, ApiError> {
    blocking(move || {
        let job = runtime::open(&app.database)?
            .job(&app.connection.owner, &app.connection.lane, &id)
            .map_err(|e| e.to_string())?
            .ok_or("job not found")?;
        Ok(Json(json!({"job":job})))
    })
    .await
}
async fn submit(
    State(app): State<App>,
    _headers: HeaderMap,
    Param(key): Param<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    blocking(move || {
        let job = runtime::submit(
            &app.database,
            &app.connection.owner,
            &app.connection.lane,
            &key,
            &body,
        )?;
        Ok((StatusCode::ACCEPTED, Json(json!({"job":job}))))
    })
    .await
}
/// The only native Server entry point for a sealed prepared Solar request.
/// Keeping this separate from legacy transformer processing prevents a failed
/// Fast LV decode from becoming an alternate engine-dispatch authority.
async fn submit_solar(
    State(app): State<App>,
    _headers: HeaderMap,
    Param(key): Param<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    blocking(move || {
        let (sealed, provenance) = runtime::decode_solar_server_submission(&body)?;
        let job = runtime::submit_solar_with_provenance(
            &app.database,
            &app.connection.owner,
            &app.connection.lane,
            &key,
            &sealed,
            provenance,
        )?;
        Ok((StatusCode::ACCEPTED, Json(json!({"job":job}))))
    })
    .await
}
async fn cancel(
    State(app): State<App>,
    _headers: HeaderMap,
    Param(id): Param<String>,
) -> Result<Json<Value>, ApiError> {
    blocking(move || {
        let job = runtime::open(&app.database)?
            .update_job(
                &app.connection.owner,
                &app.connection.lane,
                &id,
                Event::Cancel,
                runtime::now_ms(),
                None,
            )
            .map_err(|e| e.to_string())?;
        Ok(Json(json!({"job":job})))
    })
    .await
}
async fn result(
    State(app): State<App>,
    _headers: HeaderMap,
    Param(id): Param<String>,
) -> Result<Response, ApiError> {
    blocking(move || {
        let bytes = runtime::open(&app.database)?
            .job_result(&app.connection.owner, &app.connection.lane, &id)
            .map_err(|e| e.to_string())?
            .ok_or("job has no completed result")?;
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
async fn activity(State(app): State<App>, _headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    blocking(move || {
        let activity = app
            .activity
            .ok_or("Solar Sync Center activity is unavailable before server startup")?;
        activity.store_read().map(Json)
    })
    .await
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
pub fn connection(
    directory: &Path,
    address: SocketAddr,
    owner: String,
    lane: String,
) -> Result<Connection, String> {
    prepare_directory(directory)?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(
            "server must listen on a fixed loopback port; use SSH forwarding for remote access"
                .into(),
        );
    }
    let path = directory.join("connection.json");
    if path.exists() {
        let existing = load_connection(directory)?;
        if existing.owner != owner || existing.lane != lane || existing.address != address {
            return Err("existing server connection belongs to another identity, lane or address; use a separate state directory".into());
        }
        return Ok(existing);
    }
    let mut secret = [0u8; 32];
    getrandom::getrandom(&mut secret).map_err(|e| e.to_string())?;
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
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(&connection).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    #[cfg(unix)]
    fs::File::open(directory)
        .and_then(|d| d.sync_all())
        .map_err(|e| e.to_string())?;
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
        crate::solar_sync::SolarActivity::open(app.database.clone(), app.connection.clone())?;
    app.activity = Some(activity.clone());
    // The pump merely drains durable StoreHost rows after a completion wake;
    // it does not run in a compute worker or block recovery/server readiness.
    let solar_pump = activity.start_pump();
    let workers = runtime::Workers::start(
        app.database.clone(),
        app.connection.owner.clone(),
        app.connection.lane.clone(),
        workers,
        app.auth.clone(),
        Some(activity),
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
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
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
    fn app(path: &Path, authorized: bool) -> App {
        App {
            database: path.join("store.sqlite"),
            connection: Connection {
                address: "127.0.0.1:19766".parse().unwrap(),
                owner: "test-owner".into(),
                lane: "stable".into(),
                token: "a".repeat(64),
            },
            auth: Arc::new(Auth(authorized)),
            requests: Arc::new(tokio::sync::Semaphore::new(2)),
            activity: None,
        }
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
        let response = router(app(dir.path(), true))
            .oneshot(
                Request::builder()
                    .uri("/v1/jobs")
                    .header("authorization", format!("Bearer {}", "a".repeat(64)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value, json!({"jobs":[],"more":false}));
    }

    #[tokio::test]
    async fn solar_submit_requires_the_kernel_claim_envelope_after_local_auth() {
        let dir = tempfile::tempdir().unwrap();
        let response = router(app(dir.path(), true))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/solar-processing/job-1")
                    .header("authorization", format!("Bearer {}", "a".repeat(64)))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"schema_version":"ds.solar.server-submission/v1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(
            !dir.path().join("store.sqlite").exists(),
            "a missing claim cannot create a compute-only Solar job"
        );
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

        let response = router(app(dir.path(), true))
            .oneshot(
                Request::builder()
                    .uri("/v1/activity")
                    .header("authorization", format!("Bearer {}", "a".repeat(64)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
    #[cfg(unix)]
    #[test]
    fn owner_only_connection_survives_restart_and_refuses_identity_switch() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let first = connection(
            dir.path(),
            "127.0.0.1:19766".parse().unwrap(),
            "owner".into(),
            "stable".into(),
        )
        .unwrap();
        assert_eq!(load_connection(dir.path()).unwrap().token, first.token);
        assert!(connection(dir.path(), first.address, "other".into(), "stable".into()).is_err());
        fs::set_permissions(
            dir.path().join("connection.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(load_connection(dir.path()).is_err());
    }
}
