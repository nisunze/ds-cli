//! The isolation proof's harness: a REAL Server, on a REAL loopback listener,
//! over a fixture identity and a fixture layer document source — and no
//! gateway, no directory and no upstream at all.
//!
//! Nothing here decides anything. The kernel admits, the durable store on disk
//! answers, and the Server's own routes are the boundary every assertion is
//! made at. What the fixtures replace is exactly the two things a machine
//! with no Canary login cannot have: the authenticated native identity
//! (`HostIdentity`) and the Sync Center gateway session (`SessionOpener`).
//! Both count what they are asked for, so a proof can assert that a whole
//! path was walked with **nothing upstream constructed at all** rather than
//! merely assert that it worked. There is deliberately no project directory
//! of any kind: the Server holds none, so the proof builds none.
//!
//! The layer source is the owner's account, not one project: it holds the
//! several projects that account can read, each with its own catalogue, and
//! it is opened per request for the project the caller named. A project the
//! account cannot read is the source's own `auth_rejected` — the answer that
//! comes back from where the account is established — and never something the
//! Server decided from a list it kept. `answer_about_another_project` makes
//! the source misbehave on purpose, which is the only way to reach
//! `project_context_changed`.
//!
//! `tests/fixtures/legacy-queue.sqlite` is a durable queue exactly as a Server
//! released BEFORE this slice wrote it: two `compute_jobs` rows whose stored
//! JSON blob has no `context` member at all. It was generated once against the
//! shared store's own schema, with a real sealed Solar envelope for
//! `project-a` (`aderm_bere`, from `ds-solar/fixtures`) as the Solar row's
//! input, and a real `ds.fast-lv.request/v1` batch as the transformer row's.
//! Reading the Solar bytes back out of it is also how this harness gets a
//! genuine sealed envelope to submit live, so the one fixture serves both.
//!
//! Three things here are NOT fixtures, and exist because the second
//! adversarial pass showed that a stub in their place proved nothing:
//!
//!   * [`device_home`] writes the protected native state a real `ds auth link`
//!     leaves, so the production `NativeAuthorizer` can be exercised over a
//!     real machine instead of a stub `Authorizer`.
//!   * [`LiveServer`] starts the REAL `ds server serve` as its own process, on
//!     a real port, over that machine — the shipped wiring, end to end,
//!     including the startup path the pass refuted.
//!   * [`cut_network`] compiles an `LD_PRELOAD` shim that fails every
//!     non-loopback connect and every non-loopback name lookup, so "there is
//!     no gateway" stops being an arrangement of this test and becomes a
//!     property of the machine it runs on.

#![allow(dead_code)]

pub mod device_home;

use std::{
    collections::BTreeMap,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use ds_cli_contract::{
    Context, Failure, Inputs, output::Format, output::Output, parse, spec::Command,
};
use ds_cli_server::host::{App, Connection};
use ds_cli_server::layers::LayerHost;
use ds_cli_server::server_sync::{
    ServerSyncSession,
    sessions::{ServerSessions, SessionOpener},
};
use ds_command_kernel::execution_context::{Limits, Principal};
use ds_compute_runtime::{self as runtime, Authorizer, HostIdentity};
use ds_layer_ops::{DocumentRead, LayerDocuments, Order, OrderReceipt, Preferences, Scope};
use serde_json::{Value, json};

pub const A: &str = "project-a";
pub const B: &str = "project-b";
pub const C: &str = "project-c";
/// A project this owner's account cannot read a layer document for. It is
/// still a project the owner may hand this Server compute work in — admission
/// is the owner's word and there is no directory to contradict it — which is
/// exactly the difference between admitting work and crossing to the gateway.
pub const OUTSIDE: &str = "project-z";
pub const UID: &str = "uid-a";
pub const OWNER: &str = "test-owner";
pub const DEPLOYMENT: &str = "https://gateway.example";
pub const LANE: &str = "stable";
/// The city the checked-in sealed Solar envelope was prepared for.
pub const SOLAR_CITY: &str = "aderm_bere";
/// The projects the fixture account can read a layer document for.
pub const READABLE: &[&str] = &[A, B, C];

// ── the two fixture boundaries ──────────────────────────────────────────

/// No gateway anywhere in this proof — and a count of how many times one was
/// asked for, so "this path needs no upstream" is an assertion and not a
/// hope. A route that genuinely needs a session says so; every route that
/// must not need one is proven by this counter staying at zero.
#[derive(Default)]
pub struct NoGateway {
    asked: AtomicUsize,
}
impl NoGateway {
    pub fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}
impl SessionOpener for NoGateway {
    fn open(&self, _: &Path, _: &Connection, _: &str) -> Result<Arc<ServerSyncSession>, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Err("no gateway session in this test".into())
    }
}

/// The authorizer a proof uses when authorization is not what it is about —
/// the store, the document source, the door. It is NOT where the offline
/// claim rests: that is made against the production `NativeAuthorizer`,
/// reading a real protected credential, in the real `ds server serve`
/// process ([`LiveServer`]). `Paused` is how a proof starts a worker pool for
/// its recovery pass without letting it claim and execute anything.
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

/// The owner's layer document source: the projects THIS ACCOUNT can read,
/// each with its own catalogue, plus this host's preference root.
///
/// The project is a parameter of `documents`, never a property of the host,
/// so two of the owner's projects are served side by side through one running
/// Server. Every open and every read is counted, and every project a source
/// was opened for is recorded in order, so a proof can state which project
/// was actually read rather than infer it from the answer.
struct Shared {
    uid: String,
    lane: String,
    root: PathBuf,
    documents: BTreeMap<String, Value>,
    opened: Mutex<Vec<String>>,
    reads: AtomicUsize,
    reorders: Mutex<Vec<(String, Vec<Order>)>>,
    switch_project_on_read: AtomicBool,
    /// A door held open: while this is set, every read parks inside the
    /// request that made it, so a proof can fill the host's request door with
    /// real requests rather than reason about a semaphore.
    held: Mutex<bool>,
    resumed: std::sync::Condvar,
    /// How many reads are parked in there right now.
    inside: AtomicUsize,
}

pub struct LayerFixture {
    shared: Arc<Shared>,
}

impl LayerFixture {
    /// One account, the projects it can read, and where this host remembers
    /// its toggles.
    pub fn new(uid: &str, root: PathBuf, readable: &[&str]) -> Self {
        Self {
            shared: Arc::new(Shared {
                uid: uid.to_owned(),
                lane: LANE.to_owned(),
                root,
                documents: readable
                    .iter()
                    .map(|project| ((*project).to_owned(), layer_document(project)))
                    .collect(),
                opened: Mutex::new(Vec::new()),
                reads: AtomicUsize::new(0),
                reorders: Mutex::new(Vec::new()),
                switch_project_on_read: AtomicBool::new(false),
                held: Mutex::new(false),
                resumed: std::sync::Condvar::new(),
                inside: AtomicUsize::new(0),
            }),
        }
    }
    /// Every project a document source was opened for, in order.
    pub fn opened(&self) -> Vec<String> {
        self.shared.opened.lock().expect("opened").clone()
    }
    /// How many times a document was actually read out of this source.
    pub fn reads(&self) -> usize {
        self.shared.reads.load(Ordering::SeqCst)
    }
    pub fn reorders(&self) -> Vec<(String, Vec<Order>)> {
        self.shared.reorders.lock().expect("reorders").clone()
    }
    /// Park every subsequent read inside the request that made it, until
    /// [`release`](LayerFixture::release). This is how the proof saturates the
    /// host's request door with genuine in-flight requests.
    pub fn hold(&self) {
        *self.shared.held.lock().expect("hold") = true;
    }
    /// Let every parked read finish, and take no new ones.
    pub fn release(&self) {
        *self.shared.held.lock().expect("hold") = false;
        self.shared.resumed.notify_all();
    }
    /// How many reads are parked right now — which is how many requests are
    /// holding a place in the door.
    pub fn inside(&self) -> usize {
        self.shared.inside.load(Ordering::SeqCst)
    }
    /// Make the source answer about a project it was not opened for — the one
    /// way a well-behaved caller can reach `project_context_changed`.
    pub fn answer_about_another_project(&self, misbehave: bool) {
        self.shared
            .switch_project_on_read
            .store(misbehave, Ordering::SeqCst);
    }
    /// The desktop's own half of a layer operation: the same source, opened
    /// for the same project, with no Server between it and `ds_layer_ops`.
    /// This is what `ds map layer … --target desktop --project <id>` reaches
    /// through `Native::for_project`, and it is deliberately the CONCRETE
    /// type, so a proof hands it to the shared owner exactly as either host
    /// hands it its own.
    pub fn desktop_documents(&self, project: &str) -> FixtureDocuments {
        self.shared
            .opened
            .lock()
            .expect("opened")
            .push(project.to_owned());
        FixtureDocuments {
            shared: self.shared.clone(),
            project: project.to_owned(),
        }
    }
    pub fn preference_root(&self) -> PathBuf {
        self.shared.root.clone()
    }
}

pub struct FixtureDocuments {
    shared: Arc<Shared>,
    project: String,
}
impl FixtureDocuments {
    fn scope(&self) -> Scope {
        Scope {
            lane: self.shared.lane.clone(),
            uid: self.shared.uid.clone(),
            project: self.project.clone(),
        }
    }
}
impl LayerDocuments for FixtureDocuments {
    fn read(&mut self, _refresh: bool) -> Result<DocumentRead, Failure> {
        self.shared.reads.fetch_add(1, Ordering::SeqCst);
        {
            let mut held = self.shared.held.lock().expect("hold");
            if *held {
                self.shared.inside.fetch_add(1, Ordering::SeqCst);
                while *held {
                    held = self.shared.resumed.wait(held).expect("resume");
                }
                self.shared.inside.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let Some(document) = self.shared.documents.get(&self.project) else {
            // Where the account is established, not here: this is the answer
            // the gateway gives for a project this account cannot read, and
            // the Server relays it rather than deciding it.
            return Err(Failure::unauthorized(
                "auth_rejected",
                "this account reads no layer configuration for that project",
            )
            .remedy("run ds auth project list and name a project this account can read"));
        };
        let mut document = document.clone();
        let mut scope = self.scope();
        if self.shared.switch_project_on_read.load(Ordering::SeqCst) {
            document["project_id"] = json!("someone-elses-project");
            scope.project = "someone-elses-project".into();
        }
        Ok(DocumentRead { scope, document })
    }
    fn check_scope(&mut self, expected: &Scope) -> Result<(), Failure> {
        if expected == &self.scope() {
            Ok(())
        } else {
            Err(
                Failure::conflict("project_context_changed", "fixture scope changed")
                    .remedy("repeat the layer request"),
            )
        }
    }
    fn reorder(&mut self, orders: &[Order]) -> Result<OrderReceipt, Failure> {
        self.shared
            .reorders
            .lock()
            .expect("reorders")
            .push((self.project.clone(), orders.to_vec()));
        Ok(OrderReceipt {
            project: self.project.clone(),
            reordered: orders.len(),
        })
    }
}

impl LayerHost for LayerFixture {
    fn documents(&self, project: &str) -> Result<Box<dyn LayerDocuments + Send>, Failure> {
        self.shared
            .opened
            .lock()
            .expect("opened")
            .push(project.to_owned());
        Ok(Box::new(FixtureDocuments {
            shared: self.shared.clone(),
            project: project.to_owned(),
        }))
    }
    fn preferences(&self) -> Result<Preferences, Failure> {
        Ok(Preferences::at(self.shared.root.clone()))
    }
}

/// One project's catalogue. Each project's differs — `A` designs LV lines,
/// `B` MV lines, `C` HV lines — so a test cannot pass by being served the
/// wrong project's document.
fn layer_document(project: &str) -> Value {
    let lines = match project {
        B => "design/mv_lines",
        C => "design/hv_lines",
        _ => "design/lines",
    };
    json!({
        "project_id": project,
        "sources": {"survey_geo": {"type": "geojson"}, "design_vt": {"type": "vector"}},
        "styles": {}, "style_editors": [],
        "layers": [
            {"id": "ds-poles", "type": "circle", "source": "survey_geo", "style_ref": "ds-poles",
             "metadata": {"config_layer_id": "survey/poles", "label": "Poles",
                          "layer_class": "survey", "geometry_type": "Point", "order": 10}},
            {"id": "ds-lines", "type": "line", "source": "design_vt", "style_ref": "ds-lines",
             "metadata": {"config_layer_id": lines, "label": "Lines",
                          "layer_class": "design_tile", "geometry_type": "LineString", "order": 20}}
        ]
    })
}

/// The canonical layer id only this project's catalogue carries.
pub fn only_in(project: &str) -> &'static str {
    match project {
        B => "design/mv_lines",
        C => "design/hv_lines",
        _ => "design/lines",
    }
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
    /// The exact bytes as text, for an assertion about what an answer must
    /// NOT contain.
    pub fn stringify(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
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
    pub identity: HostIdentity,
    pub limits: Limits,
    pub app: App,
    /// The owner's layer source: which projects it can read, what it was
    /// opened for, and the switch that makes it misbehave.
    pub layers: Arc<LayerFixture>,
    /// The gateway that is not here, and its count of how often it was asked.
    pub gateway: Arc<NoGateway>,
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

/// The durable owner fence exactly as `ds-cli-server::auth::owner_fence`
/// derives it from the credential this machine holds: the digest of
/// (uid, lane, credential audience). Two principals
/// therefore never share one in production, which is the thing a proof that
/// deliberately shares it is testing the absence of.
pub fn owner_digest(uid: &str, lane: &str) -> String {
    runtime::digest(&serde_json::to_vec(&(uid, lane, DEPLOYMENT)).expect("identity tuple"))
}

/// The request door every proof but the door's own runs behind: wide enough
/// that nothing else in this file ever meets it.
pub const DOOR: usize = 8;

pub const fn limits() -> Limits {
    Limits {
        global_running: 4,
        per_project_running: 2,
        per_project_queued: 8,
        global_queued: 16,
    }
}

impl Host {
    /// A Server whose owner's account can read [`READABLE`], listening on a
    /// free loopback port. No project is declared to it beforehand: there is
    /// no directory, so callers name theirs and that is the whole of it.
    pub fn start(limits: Limits) -> Self {
        Self::start_with(limits, None, DOOR)
    }

    /// The same, over a store.sqlite that is already on disk (the legacy
    /// queue fixture).
    pub fn start_over(limits: Limits, queue: &Path) -> Self {
        Self::start_with(limits, Some(queue), DOOR)
    }

    /// The same, with a request door of a stated width. A door only answers
    /// when it is full, and filling the default one would mean holding eight
    /// requests open to prove one sentence.
    pub fn start_with_door(limits: Limits, door: usize) -> Self {
        Self::start_with(limits, None, door)
    }

    fn start_with(limits: Limits, queue: Option<&Path>, door: usize) -> Self {
        let state = tempfile::tempdir().expect("state directory");
        let prefs = tempfile::tempdir().expect("preference root");
        if let Some(queue) = queue {
            std::fs::copy(queue, state.path().join("store.sqlite")).expect("stage legacy queue");
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback port");
        let address = listener.local_addr().expect("bound address");
        let identity = identity(UID, LANE);
        let layers = Arc::new(LayerFixture::new(UID, prefs.path().to_owned(), READABLE));
        let gateway = Arc::new(NoGateway::default());
        let mut app = build_app(
            state.path(),
            state.path().join("store.sqlite"),
            address,
            LANE,
            identity.clone(),
            limits,
            layers.clone(),
            gateway.clone(),
        );
        app.requests = Arc::new(ds_cli_server::host::Door::new(door));
        let token = app.connection.token.clone();
        let running = serve(app.clone(), listener);
        Self {
            state,
            prefs,
            address,
            token,
            identity,
            limits,
            app,
            layers,
            gateway,
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

    /// Restart on the same protected state, the same layer source and the
    /// same fixed loopback port, exactly as `ds server serve` would after a
    /// machine reboot.
    pub fn restart(&mut self) {
        self.stop();
        let listener = std::net::TcpListener::bind(self.address).expect("rebind the same port");
        self.app = build_app(
            self.state.path(),
            self.database(),
            self.address,
            LANE,
            self.identity.clone(),
            self.limits,
            self.layers.clone(),
            self.gateway.clone(),
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
            Arc::new(LayerFixture::new(uid, prefs.path().to_owned(), READABLE)),
            Arc::new(NoGateway::default()),
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
    /// it recovers every row's context before any worker can claim, and what
    /// a worker may run is what was admitted — there is no directory, no
    /// membership and no saved selection anywhere in a `WorkerContext`.
    pub fn workers(&self, auth: Arc<dyn Authorizer>, count: usize) -> runtime::Workers {
        runtime::Workers::start(
            Arc::new(runtime::WorkerContext {
                path: self.database(),
                identity: self.identity.clone(),
                limits: self.limits,
                auth,
                observer: None,
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
        raw_at(self.address, &self.token, method, path, body, &[])
    }

    /// One request with a bearer that is not this Server's owner's.
    pub fn as_bearer(&self, token: &str, method: &str, path: &str, body: Option<&[u8]>) -> Answer {
        raw_at(self.address, token, method, path, body, &[])
    }

    /// The owner's own request, carrying extra headers — for proving that a
    /// header this host does not read changes nothing at all.
    pub fn raw_with(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        headers: &[(&str, &str)],
    ) -> Answer {
        raw_at(self.address, &self.token, method, path, body, headers)
    }

    /// Write one input file and return its absolute path, as a caller would
    /// hand `ds server submit --input`.
    pub fn input(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.state.path().join(name);
        std::fs::write(&path, bytes).expect("write input");
        path.display().to_string()
    }

    /// The body `POST /v1/solar-processing/:key` takes: the PATH of a sealed
    /// envelope on this machine, which the Server opens and digests itself.
    pub fn sealed_at(&self, name: &str, bytes: &[u8]) -> Vec<u8> {
        let path = self.input(name, bytes);
        serde_json::to_vec(&json!({ "input_path": path })).expect("closed request")
    }

    /// `ds …`, as an operator types it, against THIS Server. The command
    /// path comes first, exactly as it is typed; this Server's protected
    /// state directory and lane are appended as the transport arguments the
    /// declaration carries.
    pub fn ds(&self, tokens: &[&str]) -> DsRun {
        let state = self.state.path().display().to_string();
        let mut args = tokens.to_vec();
        args.extend_from_slice(&["--state-dir", state.as_str(), "--lane", LANE]);
        run_ds(&args)
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
        raw_at(self.address, &self.token, method, path, body, &[])
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
    state_path: &Path,
    database: PathBuf,
    address: SocketAddr,
    lane: &str,
    identity: HostIdentity,
    limits: Limits,
    layers: Arc<LayerFixture>,
    gateway: Arc<NoGateway>,
) -> App {
    build_app_as(
        state_path, database, address, OWNER, lane, identity, limits, layers, gateway,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_app_as(
    state_path: &Path,
    database: PathBuf,
    address: SocketAddr,
    owner: &str,
    lane: &str,
    identity: HostIdentity,
    limits: Limits,
    layers: Arc<LayerFixture>,
    gateway: Arc<NoGateway>,
) -> App {
    owner_only(state_path);
    let connection =
        ds_cli_server::host::connection(state_path, address, owner.to_owned(), lane.to_owned())
            .expect("protected connection");
    let sessions = ServerSessions::with(
        connection.clone(),
        database.clone(),
        limits,
        identity,
        gateway,
    );
    App {
        database,
        connection,
        auth: Arc::new(Allow),
        requests: Arc::new(ds_cli_server::host::Door::new(8)),
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

/// One request at any listener, with any bearer, returning the exact bytes.
/// The same call [`Host::raw`] makes, for a Server this harness did not build.
pub fn wire(
    address: SocketAddr,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Answer {
    raw_at(address, token, method, path, body, &[])
}

fn raw_at(
    address: SocketAddr,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    headers: &[(&str, &str)],
) -> Answer {
    let url = format!("http://{address}{path}");
    let authorization = format!("Bearer {token}");
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = if method == "GET" {
        let mut request = agent.get(&url).header("authorization", &authorization);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request.call()
    } else {
        let mut request = agent
            .post(&url)
            .header("authorization", &authorization)
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request.send(body.unwrap_or_default())
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

// ── the `ds` executable ─────────────────────────────────────────────────

/// One run of the real `ds` binary: what an operator sees.
pub struct DsRun {
    pub envelope: Value,
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

/// The `ds` executable this workspace builds, beside the test binary. It
/// belongs to another package, so `cargo test -p ds-cli-server` does not
/// build it; it is built here, once per test process, into the same target
/// directory.
///
/// It is built EVERY time, not only when the file is absent. A proof whose
/// whole claim is about the SHIPPED Server — the real process, the real
/// authorization path — proves nothing if it runs whatever `ds` happens to be
/// lying in the target directory: a binary from before the change under test
/// passes or fails on behalf of code nobody edited. `cargo build` is the
/// cheapest way to be sure, because when nothing changed it is a freshness
/// check and returns in under a second; when something did, that is exactly
/// the run that must not be skipped.
pub fn ds_binary() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let exe = std::env::current_exe().expect("this test binary's own path");
            let profile = exe
                .parent()
                .and_then(|deps| deps.parent())
                .expect("<target>/<profile>/deps/<test>")
                .to_path_buf();
            let binary = profile.join(if cfg!(windows) { "ds.exe" } else { "ds" });
            let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .expect("the ds-cli workspace root");
            let target = profile.parent().expect("<target>");
            let built = std::process::Command::new(
                std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()),
            )
            .args(["build", "-p", "ds", "--bin", "ds"])
            .current_dir(&workspace)
            .env("CARGO_TARGET_DIR", target)
            .status();
            assert!(
                matches!(built, Ok(status) if status.success()) && binary.exists(),
                "the proof drives the real `ds`; build it with \
                 `cargo build -p ds --bin ds` into {}",
                target.display()
            );
            binary
        })
        .clone()
}

/// `ds …` with a valid native client catalogue and a private config home, so
/// the command is available to parse and dispatch, and with no desktop
/// descriptor, so nothing here can reach the operator's running app. The
/// `--target server` path reads neither: it reads `connection.json`.
pub fn run_ds(args: &[&str]) -> DsRun {
    let config = tempfile::tempdir().expect("private config home");
    run_ds_in(config.path(), args)
}

/// The same, against a NAMED config home — a machine that already holds a
/// device credential, for instance.
pub fn run_ds_in(config: &Path, args: &[&str]) -> DsRun {
    let output = ds_command(config, args)
        .output()
        .expect("the ds binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    DsRun {
        envelope: serde_json::from_str(&stdout).unwrap_or(Value::Null),
        stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// One `ds` invocation, configured but not yet run: a valid development client
/// catalogue, one private config home, no desktop descriptor, and — whenever
/// this machine can build it — the network cut out from under it.
fn ds_command(config: &Path, args: &[&str]) -> std::process::Command {
    let mut command = std::process::Command::new(ds_binary());
    command
        .args(args)
        .env("NO_COLOR", "1")
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", device_home::catalogue())
        .env("DS_CONFIG_HOME", config)
        .env("DS_DESKTOP_DESCRIPTOR", config.join("no-desktop.json"));
    if let Some(shim) = cut_network() {
        command.env("LD_PRELOAD", shim);
    }
    command
}

// ── the network, cut ────────────────────────────────────────────────────

/// Set on a run that is already under the shim, naming it. Its presence is
/// also how the cut-network re-run recognises itself and does not recurse.
pub const CUT_NETWORK_SHIM: &str = "DS_ISOLATION_NO_NETWORK";

/// The compiled `LD_PRELOAD` shim, or `None` when this machine has no C
/// compiler. Built once per test process, into a directory that outlives it.
///
/// See `tests/fixtures/no_network.c`: every non-loopback `connect` answers
/// `ENETUNREACH` and every non-loopback name lookup answers `EAI_FAIL`, for
/// whatever process loads it and everything that process spawns.
pub fn cut_network() -> Option<PathBuf> {
    static SHIM: OnceLock<Option<PathBuf>> = OnceLock::new();
    SHIM.get_or_init(|| {
        if !cfg!(target_os = "linux") {
            return None;
        }
        // A run that is ALREADY under the cut network inherits the shim
        // rather than rebuilding it: rewriting a shared object while other
        // processes have it mapped is a way to break a machine, not a way to
        // prove something about one.
        if let Some(inherited) = std::env::var_os(CUT_NETWORK_SHIM) {
            return Some(PathBuf::from(inherited));
        }
        let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/no_network.c");
        // Beside the test binary, not in a temp directory: the shim has to
        // outlive every child process that loads it, including one this
        // process is still waiting on when it panics.
        let target = std::env::current_exe()
            .ok()?
            .parent()?
            .join("ds-isolation-no-network.so");
        let built = std::process::Command::new(&compiler)
            .args(["-shared", "-fPIC", "-O1", "-o"])
            .arg(&target)
            .arg(&source)
            .arg("-ldl")
            .output()
            .ok()?;
        if !built.status.success() {
            eprintln!(
                "the network-cut shim did not compile with {compiler}: {}",
                String::from_utf8_lossy(&built.stderr)
            );
            return None;
        }
        Some(target)
    })
    .clone()
}

// ── the real `ds server serve`, as its own process ──────────────────────

/// The SHIPPED Server: `ds server serve`, started as an operator starts it,
/// over a machine that holds a device credential, with no gateway anywhere.
///
/// Everything the in-process [`Host`] replaces is real here — the startup
/// path, the production `NativeAuthorizer` reading the protected state on
/// disk, the credential refresher, the worker pool, the port. Nothing is
/// injected: the only things this harness supplies are the two environment
/// values any install has (a client catalogue and a config home) and the
/// state directory and port an operator passes on the command line.
pub struct LiveServer {
    child: std::process::Child,
    pub address: SocketAddr,
    pub token: String,
    pub state: tempfile::TempDir,
    pub config: PathBuf,
    said: Arc<Mutex<String>>,
    /// One machine, one owner, one Server — the standing ruling, and here
    /// also a practical fence: two real hosts starting at once on one box
    /// contend for the CPU hard enough to lose the race their own startup
    /// runs between the Solar pump and the worker pool's recovery pass.
    _machine: std::sync::MutexGuard<'static, ()>,
}

/// The machine a live Server runs on. There is one.
static MACHINE: Mutex<()> = Mutex::new(());

impl LiveServer {
    /// Start it and wait until it says it is ready, or panic with what it
    /// said instead.
    pub fn start(home: &device_home::DeviceHome, workers: usize) -> Self {
        // Taken before anything is spawned and held for this Server's whole
        // life. A poisoned lock is a panicking test, not a broken machine.
        let machine = MACHINE.lock().unwrap_or_else(|held| held.into_inner());
        let state = tempfile::tempdir().expect("protected server state");
        owner_only(state.path());
        // The state directory is EMPTY, deliberately: every start here is a
        // cold start, store and all. It used to be warmed by this harness,
        // because the host opened a brand-new store.sqlite from three places
        // at once and lost the WAL conversion race often enough to exit
        // `database is locked` before answering. `host::serve` now opens the
        // store once itself before anything else touches it, so warming it
        // here would only hide whether that holds.
        assert!(
            !state.path().join("store.sqlite").exists(),
            "every live Server here starts cold, durable store included"
        );
        let address = free_loopback_port();
        let mut child = ds_command(
            home.config_home(),
            &[
                "server",
                "serve",
                "--lane",
                LANE,
                "--state-dir",
                &state.path().display().to_string(),
                "--listen",
                &address.to_string(),
                "--workers",
                &workers.to_string(),
            ],
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the ds binary starts");
        let said = Arc::new(Mutex::new(String::new()));
        for stream in [
            child.stderr.take().map(Reading::Err),
            child.stdout.take().map(Reading::Out),
        ]
        .into_iter()
        .flatten()
        {
            let said = said.clone();
            std::thread::spawn(move || stream.drain(&said));
        }
        let mut server = Self {
            child,
            address,
            token: String::new(),
            state,
            config: home.config_home().to_owned(),
            said,
            _machine: machine,
        };
        assert!(
            until(30, || server.said().contains("DS server ready at")),
            "the Server never became ready with no gateway present. It said: {}",
            server.said()
        );
        let connection: Value = serde_json::from_slice(
            &std::fs::read(server.state.path().join("connection.json"))
                .expect("the Server wrote its protected connection"),
        )
        .expect("connection.json parses");
        server.token = connection["token"]
            .as_str()
            .expect("an owner bearer")
            .to_owned();
        server
    }

    /// Everything the Server has said on either stream so far.
    pub fn said(&self) -> String {
        self.said.lock().expect("output").clone()
    }

    /// One request at the running Server, with the owner bearer.
    pub fn raw(&self, method: &str, path: &str, body: Option<&[u8]>) -> Answer {
        wire(self.address, &self.token, method, path, body)
    }

    /// `ds …` against this running Server, from the same machine.
    pub fn ds(&self, tokens: &[&str]) -> DsRun {
        let state = self.state.path().display().to_string();
        let mut args = tokens.to_vec();
        args.extend_from_slice(&["--state-dir", state.as_str(), "--lane", LANE]);
        run_ds_in(&self.config, &args)
    }

    /// Ask it to stop the way a service manager does, and wait for it.
    pub fn stop(&mut self) -> Option<i32> {
        #[cfg(unix)]
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        for _ in 0..100 {
            match self.child.try_wait() {
                Ok(Some(status)) => return status.code(),
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        self.child.wait().ok().and_then(|status| status.code())
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One of this machine's own streams, drained into the shared transcript.
enum Reading {
    Out(std::process::ChildStdout),
    Err(std::process::ChildStderr),
}
impl Reading {
    fn drain(self, said: &Mutex<String>) {
        use std::io::BufRead;
        let reader: Box<dyn BufRead> = match self {
            Self::Out(stream) => Box::new(std::io::BufReader::new(stream)),
            Self::Err(stream) => Box::new(std::io::BufReader::new(stream)),
        };
        for line in reader.lines().map_while(Result::ok) {
            if let Ok(mut said) = said.lock() {
                said.push_str(&line);
                said.push('\n');
            }
        }
    }
}

/// A loopback port nothing is listening on, released again immediately. The
/// Server takes a FIXED port by design, so the proof picks one the way an
/// operator would and hands it over.
fn free_loopback_port() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback port");
    let address = listener.local_addr().expect("bound address");
    drop(listener);
    address
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
