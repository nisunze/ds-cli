//! The isolation proof's harness: a REAL Server, on a REAL loopback listener,
//! over a fixture identity, a fixture project directory and a fixture layer
//! document source — and no gateway at all.
//!
//! Nothing here decides anything. The kernel admits, the durable store on disk
//! answers, and the Server's own routes are the boundary every assertion is
//! made at. What the fixtures replace is exactly the three things a machine
//! with no Canary login cannot have: the authenticated native identity
//! (`HostIdentity`), the project directory a membership snapshot is fetched
//! from (`ProjectDirectory`), and the Sync Center gateway session
//! (`SessionOpener`, which here refuses, so a route that must not need one is
//! proven by never reaching it).
//!
//! `tests/fixtures/legacy-queue.sqlite` is a durable queue exactly as a Server
//! released BEFORE this slice wrote it: two `compute_jobs` rows whose stored
//! JSON blob has no `context` member at all. It was generated once against the
//! shared store's own schema, with a real sealed Solar envelope for
//! `project-a` (`aderm_bere`, from `ds-solar/fixtures`) as the Solar row's
//! input, and a real `ds.fast-lv.request/v1` batch as the transformer row's.
//! Reading the Solar bytes back out of it is also how this harness gets a
//! genuine sealed envelope to submit live, so the one fixture serves both.

#![allow(dead_code)]

use std::{
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use ds_cli_contract::{
    Context, Failure, Inputs, output::Format, output::Output, parse, spec::Command,
};
use ds_cli_server::host::{App, Connection};
use ds_cli_server::layers::LayerHost;
use ds_cli_server::server_sync::{
    ServerSyncSession,
    sessions::{ProjectDirectory, ServerSessions, SessionOpener},
};
use ds_command_kernel::execution_context::{Limits, Membership, Principal};
use ds_compute_runtime::{self as runtime, Authorizer, HostIdentity, MembershipSource};
use ds_layer_ops::{DocumentRead, LayerDocuments, Order, OrderReceipt, Preferences, Scope};
use serde_json::{Value, json};

pub const A: &str = "project-a";
pub const B: &str = "project-b";
pub const C: &str = "project-c";
/// A project no directory in this harness ever returns.
pub const OUTSIDE: &str = "project-z";
pub const UID: &str = "uid-a";
pub const OWNER: &str = "test-owner";
pub const DEPLOYMENT: &str = "https://gateway.example";
pub const LANE: &str = "stable";
/// The city the checked-in sealed Solar envelope was prepared for.
pub const SOLAR_CITY: &str = "aderm_bere";

// ── the three fixture boundaries ────────────────────────────────────────

/// The projects this account may act in, changeable while the Server runs.
/// It is both what `ServerSessions` fetches a membership snapshot from and
/// what a worker re-checks a running job's project against.
pub struct Directory(pub Mutex<Vec<String>>);
impl Directory {
    pub fn new(projects: &[&str]) -> Arc<Self> {
        Arc::new(Self(Mutex::new(
            projects.iter().map(|p| (*p).to_owned()).collect(),
        )))
    }
    pub fn revoke(&self, project: &str) {
        self.0.lock().unwrap().retain(|held| held != project);
    }
    pub fn grant(&self, project: &str) {
        self.0.lock().unwrap().push(project.to_owned());
    }
}
impl ProjectDirectory for Directory {
    fn projects(&self) -> Result<Vec<String>, Failure> {
        Ok(self.0.lock().unwrap().clone())
    }
}
impl MembershipSource for Directory {
    fn snapshot(&self, now_ms: u64) -> Result<Membership, String> {
        Ok(Membership {
            projects: self.0.lock().unwrap().clone(),
            fetched_at_ms: now_ms,
            ttl_ms: 600_000,
        })
    }
}

/// No gateway anywhere in this proof. A route that needs one says so; every
/// route that must not need one is proven by never reaching this.
pub struct NoGateway;
impl SessionOpener for NoGateway {
    fn open(&self, _: &Path, _: &Connection, _: &str) -> Result<Arc<ServerSyncSession>, String> {
        Err("no gateway session in this test".into())
    }
}

/// The native device authorizer. `Paused` is how a proof starts a worker pool
/// for its recovery pass without letting it claim and execute anything.
pub struct Allow;
impl Authorizer for Allow {
    fn authorize(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}
pub struct Paused;
impl Authorizer for Paused {
    fn authorize(&self, _: &str) -> Result<(), String> {
        Err("device paused for this proof".into())
    }
}

/// One in-memory layer document, under one project, plus this host's
/// preference root. The Server's own `Fenced` wrapper is what holds a named
/// project against it, which is the thing being proven.
pub struct LayerFixture {
    pub project: String,
    pub root: PathBuf,
}
struct FixtureDocuments {
    scope: Scope,
    document: Value,
}
impl LayerDocuments for FixtureDocuments {
    fn read(&mut self, _refresh: bool) -> Result<DocumentRead, Failure> {
        Ok(DocumentRead {
            scope: self.scope.clone(),
            document: self.document.clone(),
        })
    }
    fn check_scope(&mut self, expected: &Scope) -> Result<(), Failure> {
        if expected == &self.scope {
            Ok(())
        } else {
            Err(
                Failure::conflict("project_context_changed", "fixture scope changed")
                    .remedy("repeat the layer request"),
            )
        }
    }
    fn reorder(&mut self, orders: &[Order]) -> Result<OrderReceipt, Failure> {
        Ok(OrderReceipt {
            project: self.scope.project.clone(),
            reordered: orders.len(),
        })
    }
}
impl LayerHost for LayerFixture {
    fn documents(&self) -> Result<Box<dyn LayerDocuments + Send>, Failure> {
        let mut document = layer_document();
        document["project_id"] = json!(self.project);
        Ok(Box::new(FixtureDocuments {
            scope: Scope {
                lane: LANE.into(),
                uid: UID.into(),
                project: self.project.clone(),
            },
            document,
        }))
    }
    fn preferences(&self) -> Result<Preferences, Failure> {
        Ok(Preferences::at(self.root.clone()))
    }
}

fn layer_document() -> Value {
    json!({
        "project_id": A,
        "sources": {"survey_geo": {"type": "geojson"}, "design_vt": {"type": "vector"}},
        "styles": {}, "style_editors": [],
        "layers": [
            {"id": "ds-poles", "type": "circle", "source": "survey_geo", "style_ref": "ds-poles",
             "metadata": {"config_layer_id": "survey/poles", "label": "Poles",
                          "layer_class": "survey", "geometry_type": "Point", "order": 10}},
            {"id": "ds-lines", "type": "line", "source": "design_vt", "style_ref": "ds-lines",
             "metadata": {"config_layer_id": "design/lines", "label": "LV Lines",
                          "layer_class": "design_tile", "geometry_type": "LineString", "order": 20}}
        ]
    })
}

// ── the running host ────────────────────────────────────────────────────

/// One raw answer off the wire, before anything parses it. Non-disclosure is
/// a property of these bytes, not of a projection of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub body: Vec<u8>,
}
impl Answer {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    pub fn code(&self) -> String {
        self.json()["code"].as_str().unwrap_or_default().to_owned()
    }
}

struct Running {
    runtime: tokio::runtime::Runtime,
    shutdown: tokio::sync::oneshot::Sender<()>,
}

/// A Server listening on loopback, plus everything a proof needs to talk to
/// it as `ds` does, to restart it, and to read its store on disk.
pub struct Host {
    pub state: tempfile::TempDir,
    pub prefs: tempfile::TempDir,
    pub address: SocketAddr,
    pub token: String,
    pub directory: Arc<Directory>,
    pub identity: HostIdentity,
    pub limits: Limits,
    pub app: App,
    running: Option<Running>,
    shadows: Vec<tempfile::TempDir>,
}

pub fn identity(uid: &str, lane: &str) -> HostIdentity {
    identity_of(OWNER, uid, lane)
}
pub fn identity_of(owner: &str, uid: &str, lane: &str) -> HostIdentity {
    HostIdentity {
        owner: owner.to_owned(),
        principal: Principal {
            uid: uid.to_owned(),
            lane: lane.to_owned(),
            deployment: DEPLOYMENT.to_owned(),
            install_id: "install-1".to_owned(),
        },
    }
}

/// The durable owner fence exactly as `ds-cli-server::auth::identity` derives
/// it: the digest of (uid, lane, credential audience). Two principals
/// therefore never share one in production, which is the thing a proof that
/// deliberately shares it is testing the absence of.
pub fn owner_digest(uid: &str, lane: &str) -> String {
    runtime::digest(&serde_json::to_vec(&(uid, lane, DEPLOYMENT)).expect("identity tuple"))
}

pub const fn limits() -> Limits {
    Limits {
        global_running: 4,
        per_project_running: 2,
        per_project_queued: 8,
        global_queued: 16,
    }
}

impl Host {
    /// A Server for the given member projects, with a layer document under
    /// `layer_project`, listening on a free loopback port.
    pub fn start(projects: &[&str], limits: Limits, layer_project: &str) -> Self {
        Self::start_with(projects, limits, layer_project, None)
    }

    /// The same, over a store.sqlite that is already on disk (the legacy
    /// queue fixture).
    pub fn start_over(
        projects: &[&str],
        limits: Limits,
        layer_project: &str,
        queue: &Path,
    ) -> Self {
        Self::start_with(projects, limits, layer_project, Some(queue))
    }

    fn start_with(
        projects: &[&str],
        limits: Limits,
        layer_project: &str,
        queue: Option<&Path>,
    ) -> Self {
        let state = tempfile::tempdir().expect("state directory");
        let prefs = tempfile::tempdir().expect("preference root");
        if let Some(queue) = queue {
            std::fs::copy(queue, state.path().join("store.sqlite")).expect("stage legacy queue");
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback port");
        let address = listener.local_addr().expect("bound address");
        let directory = Directory::new(projects);
        let identity = identity(UID, LANE);
        let app = build_app(
            state.path(),
            state.path().join("store.sqlite"),
            address,
            LANE,
            identity.clone(),
            limits,
            directory.clone(),
            Arc::new(LayerFixture {
                project: layer_project.to_owned(),
                root: prefs.path().to_owned(),
            }),
        );
        let token = app.connection.token.clone();
        let running = serve(app.clone(), listener);
        Self {
            state,
            prefs,
            address,
            token,
            directory,
            identity,
            limits,
            app,
            running: Some(running),
            shadows: Vec::new(),
        }
    }

    pub fn database(&self) -> PathBuf {
        self.state.path().join("store.sqlite")
    }
    pub fn store(&self) -> ds_sync_store::Store {
        runtime::open(&self.database()).expect("durable store")
    }
    /// The durable rows this connection can see for one project, read from
    /// disk rather than from an answer.
    pub fn stored(&self, project: Option<&str>) -> Vec<ds_command_kernel::compute_jobs::Job> {
        self.store()
            .jobs(&self.identity.caller(project), 100)
            .expect("read jobs")
    }

    /// Stop the listener and free the port, keeping every byte on disk.
    pub fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            let _ = running.shutdown.send(());
            running.runtime.shutdown_timeout(Duration::from_secs(5));
        }
    }

    /// Restart on the same protected state and the same fixed loopback port,
    /// exactly as `ds server serve` would after a machine reboot.
    pub fn restart(&mut self, layer_project: &str) {
        self.stop();
        let listener = std::net::TcpListener::bind(self.address).expect("rebind the same port");
        self.app = build_app(
            self.state.path(),
            self.database(),
            self.address,
            LANE,
            self.identity.clone(),
            self.limits,
            self.directory.clone(),
            Arc::new(LayerFixture {
                project: layer_project.to_owned(),
                root: self.prefs.path().to_owned(),
            }),
        );
        self.running = Some(serve(self.app.clone(), listener));
    }

    /// A second Server, on its own protected state and port, over THIS
    /// Server's durable queue — the only honest way to ask the same store as
    /// another authenticated identity or another lane.
    ///
    /// `owner` is the durable fence it presents. Pass `owner_digest(uid, lane)`
    /// for the production-true configuration; pass this Server's own [`OWNER`]
    /// to take the durable fence away and leave only the execution context.
    pub fn shadow(&mut self, uid: &str, lane: &str, owner: &str) -> Loopback {
        let state = tempfile::tempdir().expect("shadow state");
        let prefs = tempfile::tempdir().expect("shadow preferences");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback port");
        let address = listener.local_addr().expect("bound address");
        let app = build_app_as(
            state.path(),
            self.database(),
            address,
            owner,
            lane,
            identity_of(owner, uid, lane),
            self.limits,
            self.directory.clone(),
            Arc::new(LayerFixture {
                project: A.to_owned(),
                root: prefs.path().to_owned(),
            }),
        );
        let token = app.connection.token.clone();
        let state_path = state.path().to_owned();
        let running = serve(app, listener);
        self.shadows.push(state);
        self.shadows.push(prefs);
        Loopback {
            address,
            token,
            state: state_path,
            lane: lane.to_owned(),
            running: Some(running),
        }
    }

    /// A worker pool over this Server's queue, exactly as `serve` starts one:
    /// it recovers every row's context before any worker can claim.
    pub fn workers(
        &self,
        auth: Arc<dyn Authorizer>,
        count: usize,
        saved_project: Option<&str>,
    ) -> runtime::Workers {
        runtime::Workers::start(
            Arc::new(runtime::WorkerContext {
                path: self.database(),
                identity: self.identity.clone(),
                limits: self.limits,
                auth,
                membership: self.directory.clone(),
                observer: None,
                saved_project: saved_project.map(str::to_owned),
            }),
            count,
        )
        .expect("worker pool")
    }

    // -- talking to it exactly as `ds` does -----------------------------

    /// The arguments a `ds server …` invocation carries, with this Server's
    /// protected state directory and lane already on them.
    pub fn args(&self, command: &Command, tokens: &[&str]) -> Inputs {
        let mut all = vec![
            "--state-dir".to_owned(),
            self.state.path().display().to_string(),
            "--lane".to_owned(),
            LANE.to_owned(),
        ];
        all.extend(tokens.iter().map(|token| (*token).to_owned()));
        parse(command, &all).expect("declared tokens parse")
    }

    /// One request straight at the listener, with the owner bearer, returning
    /// the exact bytes.
    pub fn raw(&self, method: &str, path: &str, body: Option<&[u8]>) -> Answer {
        raw_at(self.address, &self.token, method, path, body)
    }

    /// Write one input file and return its absolute path, as a caller would
    /// hand `ds server submit --input`.
    pub fn input(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.state.path().join(name);
        std::fs::write(&path, bytes).expect("write input");
        path.display().to_string()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A second listener over the same durable queue.
pub struct Loopback {
    pub address: SocketAddr,
    pub token: String,
    pub state: PathBuf,
    pub lane: String,
    running: Option<Running>,
}
impl Loopback {
    pub fn raw(&self, method: &str, path: &str, body: Option<&[u8]>) -> Answer {
        raw_at(self.address, &self.token, method, path, body)
    }
}
impl Drop for Loopback {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            let _ = running.shutdown.send(());
            running.runtime.shutdown_timeout(Duration::from_secs(5));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_app(
    directory_path: &Path,
    database: PathBuf,
    address: SocketAddr,
    lane: &str,
    identity: HostIdentity,
    limits: Limits,
    projects: Arc<Directory>,
    layers: Arc<LayerFixture>,
) -> App {
    build_app_as(
        directory_path,
        database,
        address,
        OWNER,
        lane,
        identity,
        limits,
        projects,
        layers,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_app_as(
    directory_path: &Path,
    database: PathBuf,
    address: SocketAddr,
    owner: &str,
    lane: &str,
    identity: HostIdentity,
    limits: Limits,
    projects: Arc<Directory>,
    layers: Arc<LayerFixture>,
) -> App {
    owner_only(directory_path);
    let connection =
        ds_cli_server::host::connection(directory_path, address, owner.to_owned(), lane.to_owned())
            .expect("protected connection");
    let sessions = ServerSessions::with(
        connection.clone(),
        database.clone(),
        limits,
        identity,
        projects,
        Arc::new(NoGateway),
    );
    App {
        database,
        connection,
        auth: Arc::new(Allow),
        requests: Arc::new(tokio::sync::Semaphore::new(8)),
        activity: None,
        layers,
        sessions,
    }
}

/// The protected state directory the Server insists on: 0700, owned here.
/// A temp directory inherits the process umask, so it is set explicitly.
fn owner_only(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only state directory");
    }
}

fn serve(app: App, listener: std::net::TcpListener) -> Running {
    listener.set_nonblocking(true).expect("non-blocking");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("server runtime");
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let router = ds_cli_server::host::router(app);
    runtime.spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
    });
    Running { runtime, shutdown }
}

fn raw_at(
    address: SocketAddr,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Answer {
    let url = format!("http://{address}{path}");
    let authorization = format!("Bearer {token}");
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = if method == "GET" {
        agent
            .get(&url)
            .header("authorization", &authorization)
            .call()
    } else {
        agent
            .post(&url)
            .header("authorization", &authorization)
            .header("content-type", "application/json")
            .send(body.unwrap_or_default())
    }
    .expect("the loopback listener answered");
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .expect("read the answer");
    Answer {
        status,
        body: bytes,
    }
}

// ── inputs ──────────────────────────────────────────────────────────────

pub fn context() -> Context {
    Context {
        confirmed: true,
        output: Output {
            format: Format::Json,
            pretty: false,
            color: false,
        },
    }
}

/// A real `ds.fast-lv.request/v1` batch: one transformer, one span.
pub fn transformer(name: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema":"ds.fast-lv.request/v1","jobs":[{"transformer_name":name,"gdfs":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr-1","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":name,"names":name}}]},"lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line-1","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]},"customers":{"type":"FeatureCollection","features":[]}},"settings":{}}]})).unwrap()
}

pub fn legacy_queue_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy-queue.sqlite")
}
pub fn legacy_solar_id() -> String {
    runtime::digest(b"legacy-solar")
}
pub fn legacy_transformer_id() -> String {
    runtime::digest(b"legacy-transformer")
}

/// The sealed prepared Solar request the fixture queue holds, read back out of
/// it through the store's own API — a genuine `PreparedSolarCityInput` for
/// `project-a`, weather and reference unit and all.
fn sealed_prepared() -> &'static [u8] {
    static SEALED: OnceLock<Vec<u8>> = OnceLock::new();
    SEALED.get_or_init(|| {
        let staging = tempfile::tempdir().expect("staging");
        let database = staging.path().join("store.sqlite");
        std::fs::copy(legacy_queue_fixture(), &database).expect("stage the fixture queue");
        let identity = identity(UID, LANE);
        runtime::open(&database)
            .expect("open the fixture queue")
            // A row a released Server wrote has no context, so it is visible
            // only to its own unnarrowed connection. That is the store's rule,
            // and reading through it is what makes this a real sealed input.
            .job_input(&identity.caller(None), &legacy_solar_id())
            .expect("read the sealed input")
            .expect("the fixture queue holds it")
    })
}

/// One `ds.solar.server-submission/v1` envelope: the sealed prepared request
/// above plus the governed publication claim the kernel binds to it. The
/// project it names is [`A`] and no query parameter can change that — which is
/// the whole point of the sealed-input precedence proofs.
pub fn solar_submission() -> Vec<u8> {
    let mut envelope: Value = serde_json::from_slice(sealed_prepared()).expect("sealed request");
    let prepared = envelope["prepared"].clone();
    envelope["schema_version"] = json!("ds.solar.server-submission/v1");
    envelope["publication_claim"] = json!({
        "schema_version": "ds-solar.prepared-publication-claim/v1",
        "claim_kind": "server-revalidated-publication-handoff",
        "project_id": A,
        "city_id": SOLAR_CITY,
        "city_content_digest": prepared["city"]["content_digest"],
        "prepared_input_digest": prepared["input_digest"],
        "source_snapshot_sha256": "b".repeat(64),
        "input_base_fingerprint": "a".repeat(64),
        "snapshot_receipt_id": "550e8400-e29b-51d4-a716-446655440000",
        "snapshot_receipt_expires_at": "2030-01-01T00:00:00Z",
    });
    serde_json::to_vec(&envelope).expect("envelope encodes")
}

// ── waiting ─────────────────────────────────────────────────────────────

/// Poll a durable condition for at most `seconds`. Used only where real
/// workers are running; every other assertion is immediate.
pub fn until(seconds: u64, mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    ready()
}
