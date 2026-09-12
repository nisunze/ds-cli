//! Two machines that are not this one.
//!
//! Every claim in `instances.rs` is about which *live runtime* an operation
//! reached, so nothing here is a stub of the decision: the descriptors are real
//! files in a real registry directory, the instances are real loopback HTTP
//! servers on real ephemeral ports, and the only thing this module fakes is the
//! application behind the socket. `ds` reads the files, sends the token, reads
//! the handshake and posts the operation exactly as it does against DS
//! GridDesign — and each fixture records what it actually received, so a test
//! can assert *zero misrouted effects* rather than a friendly error message.
//!
//! Three fixtures matter, and the tests build them from these parts:
//!
//! * two instances on different projects, whose projects carry the **same
//!   display name** — the contract's "names alone do not distinguish projects";
//! * a third holding the **same project** as the first — the contract's
//!   "the same project may also be open in several instances";
//! * a fourth that answers 401 to the pairing token — a stale descriptor, and
//!   the port a restarted instance reused.
//!
//! Nothing here contacts anything but its own listeners: no installed state, no
//! credential, no operator desktop.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use ds_cli_contract::spec::{Authority, Command};
use ds_cli_contract::{Context, Failure, Format, Inputs, Output, parse};
use ds_cli_desktop::discover::{self, Requirement, Target};
use ds_cli_desktop::ops::{self, BridgeOp, HeadlessIdentity};
use serde_json::{Value, json};

/// The install profile every fixture is published under, and the Tauri bundle
/// identifier whose app-data directory `ds` reads it from. Canary, because
/// `Requirement.lane` is `stable | canary` and a development lane is a
/// different rule with its own tests.
pub const PROFILE: &str = "canary";
pub const IDENTIFIER: &str = "rw.datasolutions.desktop.canary";

/// One signed-in account, in the shape `/v1/session` publishes it.
pub const UID: &str = "uid-operator";
pub const EMAIL: &str = "operator@example.test";
pub const AUDIENCE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
pub const LANE: &str = "canary";
pub const BUILD: &str = "2026.9.12+1";

/// A real project-authority operation, declared exactly as `ds-cli-assets`
/// declares it. The routing decision does not depend on which operation it is;
/// what matters is that it is one a Project-authority command really sends, so
/// the requirement this test scopes is the requirement dispatch would scope.
pub const PROJECT_OP: BridgeOp = BridgeOp {
    operation: "assets.list",
    arguments: &["folder", "limit"],
};

/// Serialise every test in the binary.
///
/// The descriptor directory is `XDG_DATA_HOME`, which `ds` reads from the
/// process environment at the moment it enumerates — so a test that points it
/// at its own temporary registry owns the process while it does. One lock, held
/// for the whole test body, is what makes that sound: no other test thread is
/// running, and the fixture threads never read the environment.
pub fn machine() -> MutexGuard<'static, ()> {
    static MACHINE: OnceLock<Mutex<()>> = OnceLock::new();
    MACHINE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

static NEXT: AtomicU64 = AtomicU64::new(1);

fn unique(what: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ds-instances-{what}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// This machine's app-data root for the run of one test: a fresh temporary
/// directory that `ds` will read as `XDG_DATA_HOME`.
pub struct Machine {
    root: PathBuf,
    _guard: MutexGuard<'static, ()>,
}

impl Machine {
    /// Take the process and point its descriptor discovery at a temporary
    /// registry. Every `Machine` is a different directory, so nothing a test
    /// wrote survives into the next one.
    pub fn new() -> Self {
        let guard = machine();
        let root = unique("machine");
        std::fs::create_dir_all(root.join(IDENTIFIER).join(discover::DESCRIPTOR_DIR))
            .expect("a registry directory");
        // SAFETY: `machine()` is held for the lifetime of this value, so this
        // is the only test thread running; the fixture listener threads read
        // no environment. Restored on drop.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &root);
            std::env::remove_var(discover::DESCRIPTOR_ENV);
            std::env::remove_var(ops::TARGET_ENV);
        }
        Self {
            root,
            _guard: guard,
        }
    }

    /// The registry root, as `XDG_DATA_HOME`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn app_data(&self) -> PathBuf {
        self.root.join(IDENTIFIER)
    }

    pub fn registry(&self) -> PathBuf {
        self.app_data().join(discover::DESCRIPTOR_DIR)
    }

    /// Where this instance's own descriptor lives, exactly as the shell names
    /// it: one file per live instance, named by its id.
    pub fn descriptor_path(&self, instance_id: &str) -> PathBuf {
        self.registry().join(format!("{instance_id}.json"))
    }

    /// The legacy per-profile file the first live instance keeps refreshing so
    /// an older `ds` still pairs with something real.
    pub fn legacy_path(&self) -> PathBuf {
        self.app_data().join(discover::DESCRIPTOR_FILE)
    }

    /// Publish one instance's descriptor, in the shell's own bytes.
    pub fn publish(&self, bridge: &Bridge) -> PathBuf {
        let path = self.descriptor_path(&bridge.instance_id);
        write_descriptor(&path, bridge.descriptor());
        path
    }

    /// Publish the legacy copy naming `bridge`, as the first live instance does.
    pub fn publish_legacy(&self, bridge: &Bridge) -> PathBuf {
        let path = self.legacy_path();
        write_descriptor(&path, bridge.descriptor());
        path
    }

    /// A descriptor for an instance that is not running: written, never served.
    pub fn publish_unanswered(&self, instance_id: &str, port: u16) -> PathBuf {
        let path = self.descriptor_path(instance_id);
        write_descriptor(
            &path,
            json!({
                "version": 1,
                "url": format!("http://127.0.0.1:{port}"),
                "token": "0123456789abcdef0123456789abcdef",
                "pid": 424_242 + NEXT.fetch_add(1, Ordering::Relaxed) as u32,
                "instance_id": instance_id,
                "profile": PROFILE,
            }),
        );
        path
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        // SAFETY: as in `new` — the machine lock is still held here.
        unsafe {
            std::env::remove_var("XDG_DATA_HOME");
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn write_descriptor(path: &Path, body: Value) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
    std::fs::write(path, serde_json::to_vec(&body).expect("encodes")).expect("a descriptor");
}

/// One request a fixture actually received.
#[derive(Clone, Debug)]
pub struct Received {
    pub method: String,
    pub path: String,
    /// Did it carry this instance's own pairing token?
    pub authorized: bool,
    /// The posted body, for `/v1/invoke`.
    pub body: Value,
}

impl Received {
    pub fn operation(&self) -> Option<&str> {
        self.body.get("operation").and_then(Value::as_str)
    }
}

/// A live DS GridDesign instance, as far as `ds` can tell: a loopback endpoint
/// that answers `/v1/session` with a bounded session and `/v1/invoke` with a
/// result, and refuses anything that is not its own pairing token.
pub struct Bridge {
    pub instance_id: String,
    pub token: String,
    pub port: u16,
    /// This process's id. Distinct per fixture, as two processes on one machine
    /// always are — a restarted instance never inherits its predecessor's.
    pub pid: u32,
    pub display_name: String,
    session: Arc<Mutex<Value>>,
    log: Arc<Mutex<Vec<Received>>>,
    /// A refusal this instance owes the next operation it is asked to perform.
    /// The application raises these itself — a context fence, an offline
    /// switch — and what a caller receives is the whole claim.
    owed: Arc<Mutex<Vec<(u16, Value)>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Bridge {
    /// Start an instance on an ephemeral port, holding `project` open in its
    /// owner window. `display_name` is what the operator sees for that project
    /// — two instances may show the same one.
    pub fn start(instance_id: &str, project: Option<&str>, display_name: &str) -> Self {
        Self::start_on(
            instance_id,
            project,
            display_name,
            &token_for(instance_id),
            None,
        )
    }

    /// The same, on a port a previous instance used — the restart case.
    pub fn restart_on(
        port: u16,
        instance_id: &str,
        project: Option<&str>,
        display_name: &str,
    ) -> Self {
        Self::start_on(
            instance_id,
            project,
            display_name,
            &token_for(instance_id),
            Some(port),
        )
    }

    fn start_on(
        instance_id: &str,
        project: Option<&str>,
        display_name: &str,
        token: &str,
        port: Option<u16>,
    ) -> Self {
        let listener = bind(port);
        let port = listener.local_addr().expect("an address").port();
        let session = Arc::new(Mutex::new(session_of(instance_id, project)));
        let log: Arc<Mutex<Vec<Received>>> = Arc::new(Mutex::new(Vec::new()));
        let owed: Arc<Mutex<Vec<(u16, Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let session = Arc::clone(&session);
            let log = Arc::clone(&log);
            let owed = Arc::clone(&owed);
            let stop = Arc::clone(&stop);
            let token = token.to_owned();
            let display_name = display_name.to_owned();
            std::thread::spawn(move || {
                serve(listener, session, log, owed, stop, token, display_name)
            })
        };
        Self {
            instance_id: instance_id.to_owned(),
            token: token.to_owned(),
            port,
            pid: 4_700 + NEXT.fetch_add(1, Ordering::Relaxed) as u32,
            display_name: display_name.to_owned(),
            session,
            log,
            owed,
            stop,
            worker: Some(worker),
        }
    }

    /// A loopback endpoint that answers 401 to every token: a descriptor whose
    /// instance is gone, and the port a new process took over.
    pub fn stale(port: Option<u16>) -> Self {
        Self::start_on(
            "99999999999999999999999999999999",
            None,
            "nobody",
            "a-token-no-descriptor-ever-carried",
            port,
        )
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The bytes this instance publishes, version 1 with the additive fields.
    pub fn descriptor(&self) -> Value {
        json!({
            "version": 1,
            "url": self.url(),
            "token": self.token,
            "pid": self.pid,
            "instance_id": self.instance_id,
            "lane": LANE,
            "build": BUILD,
            "started_at_ms": 1_757_000_000_000u64,
            "profile": PROFILE,
        })
    }

    pub fn project(&self) -> Option<String> {
        self.session
            .lock()
            .expect("session")
            .get("project")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    /// Everything this instance received, in order.
    pub fn received(&self) -> Vec<Received> {
        self.log.lock().expect("log").clone()
    }

    /// The operations this instance was asked to perform. Empty is the whole
    /// point of most of these tests: an effect it never had.
    pub fn invoked(&self) -> Vec<String> {
        self.received()
            .iter()
            .filter(|request| request.path == "/v1/invoke")
            .filter_map(|request| request.operation().map(str::to_owned))
            .collect()
    }

    /// The identity fence one invoke carried.
    pub fn fence(&self, index: usize) -> Value {
        self.received()
            .iter()
            .filter(|request| request.path == "/v1/invoke")
            .nth(index)
            .and_then(|request| request.body.get("identity_fence").cloned())
            .unwrap_or(Value::Null)
    }

    /// Was this instance asked who it is, with its own token?
    pub fn probed(&self) -> bool {
        self.received()
            .iter()
            .any(|request| request.path == "/v1/session")
    }

    /// The application will refuse the next operation with this exact typed
    /// body — the shape its own structured refusals have.
    pub fn refuse_next_invoke(&self, status: u16, body: Value) {
        self.owed.lock().expect("owed").push((status, body));
    }

    pub fn forget(&self) {
        self.log.lock().expect("log").clear();
    }

    /// Stop answering and release the port, so a restart can take it.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Unblock a listener parked in `accept`.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A per-instance pairing token: 32 ASCII characters, as the shell's is.
fn token_for(instance_id: &str) -> String {
    format!("tok-{}", &instance_id[..28])
}

fn bind(port: Option<u16>) -> TcpListener {
    match port {
        None => TcpListener::bind("127.0.0.1:0").expect("an ephemeral port"),
        // The restart case. `std` sets SO_REUSEADDR, so the port its previous
        // owner released is available again; a few retries cover the moment
        // the kernel needs to release it.
        Some(port) => {
            for _ in 0..50 {
                if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
                    return listener;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("port {port} could not be reused by the restarted instance");
        }
    }
}

/// The session an instance publishes. Field names are the shell's, pinned
/// against its source by `ds/tests/bridge_parity.rs`.
fn session_of(instance_id: &str, project: Option<&str>) -> Value {
    json!({
        "session_revision": 3,
        "instance_id": instance_id,
        "build": BUILD,
        "started_at_ms": 1_757_000_000_000u64,
        "signed_in": true,
        "uid": UID,
        "email": EMAIL,
        "lane": LANE,
        "credential_audience_sha256": AUDIENCE,
        "project": project,
        "design_context": Value::Null,
        "windows": [
            { "label": "main", "project": project, "generation": 2 },
        ],
    })
}

#[allow(clippy::too_many_arguments)]
fn serve(
    listener: TcpListener,
    session: Arc<Mutex<Value>>,
    log: Arc<Mutex<Vec<Received>>>,
    owed: Arc<Mutex<Vec<(u16, Value)>>>,
    stop: Arc<AtomicBool>,
    token: String,
    display_name: String,
) {
    listener.set_nonblocking(true).expect("a pollable listener");
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                stream.set_nonblocking(false).expect("a blocking stream");
                answer(stream, &session, &log, &owed, &token, &display_name);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(_) => break,
        }
    }
}

fn answer(
    mut stream: TcpStream,
    session: &Arc<Mutex<Value>>,
    log: &Arc<Mutex<Vec<Received>>>,
    owed: &Arc<Mutex<Vec<(u16, Value)>>>,
    token: &str,
    display_name: &str,
) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a bounded read");
    let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut headers: BTreeMap<String, String> = BTreeMap::new();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line == "\r\n" || line == "\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
                }
            }
            Err(_) => return,
        }
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let authorized =
        headers.get("authorization").map(String::as_str) == Some(&format!("Bearer {token}"));
    log.lock().expect("log").push(Received {
        method: method.clone(),
        path: path.clone(),
        authorized,
        body: body.clone(),
    });

    let (status, payload) = if !authorized {
        (401, json!({ "error": "pairing_required" }))
    } else {
        match (method.as_str(), path.as_str()) {
            ("GET", "/v1/session") => (200, session.lock().expect("session").clone()),
            ("POST", "/v1/invoke") => match owed.lock().expect("owed").pop() {
                Some(refusal) => refusal,
                None => perform(&body, session, display_name),
            },
            _ => (404, json!({ "error": "unknown_operation" })),
        }
    };
    let encoded = serde_json::to_vec(&payload).expect("encodes");
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        if status == 200 { "OK" } else { "Refused" },
        encoded.len()
    );
    let _ = stream.write_all(&encoded);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Write);
}

/// What the application does with one named operation.
///
/// Only two behaviours matter to routing: an answer that names the instance
/// that produced it, and — for the one operation allowed to move a window's
/// project — actually moving it, so `ds`'s own re-observation is answered by a
/// session that really changed.
fn perform(request: &Value, session: &Arc<Mutex<Value>>, display_name: &str) -> (u16, Value) {
    let operation = request
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut session = session.lock().expect("session");
    let instance = session["instance_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let project = session["project"].as_str().map(str::to_owned);
    match operation {
        "project.list" => (
            200,
            json!({
                "activeProject": project,
                "instance": instance,
                "window": "main",
                "status": "all",
                "query": Value::Null,
                "matched": 1,
                "projects": [{
                    "project": project,
                    "name": display_name,
                    "location": Value::Null,
                    "role": "owner",
                    "status": "active",
                }],
                "more": { "omitted": 0 },
                "servedBy": instance,
            }),
        ),
        "project.switch" => {
            let requested = request["arguments"]["project"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let previous = project.clone();
            session["project"] = json!(requested);
            session["windows"][0]["project"] = json!(requested);
            // A project switch is a context change: the window's generation
            // moves, and the session revision with it.
            let generation = session["windows"][0]["generation"].as_u64().unwrap_or(1) + 1;
            session["windows"][0]["generation"] = json!(generation);
            let revision = session["session_revision"].as_u64().unwrap_or(1) + 1;
            session["session_revision"] = json!(revision);
            (
                200,
                json!({
                    "changed": previous.as_deref() != Some(requested.as_str()),
                    "previousProject": previous,
                    "activeProject": requested,
                    "instance": instance,
                    "window": "main",
                    "servedBy": instance,
                }),
            )
        }
        _ => (
            200,
            json!({ "ok": true, "operation": operation, "servedBy": instance }),
        ),
    }
}

// ---------------------------------------------------------------------------
// Driving `ds`
// ---------------------------------------------------------------------------

/// One invocation, resolved the way `ds` resolves it.
///
/// The three steps are the binary's own: parse the declared arguments with the
/// contract's parser, scope the non-secret headless observation dispatch scopes
/// (including the host `--target` named, read from the command's own declared
/// flag exactly as `registry::host_target` reads it), then call the handler.
/// Everything the claim is about — enumeration, admission, the authenticated
/// probe, selection, the fence, the send — happens inside that handler, over
/// the real sockets.
pub struct Invocation {
    identity: Option<HeadlessIdentity>,
}

impl Invocation {
    /// A caller whose machine has a native profile: `ds` knows who is running
    /// it, and the operation is about no particular project.
    pub fn signed_in() -> Self {
        Self {
            identity: Some(HeadlessIdentity {
                uid: UID.to_owned(),
                lane: LANE.to_owned(),
                credential_audience_sha256: AUDIENCE.to_owned(),
                project: None,
                command_authority: Authority::DesktopUser,
                target: None,
            }),
        }
    }

    /// The same caller with a saved project selection, running a command whose
    /// declared authority is `Project` — the one authority that narrows which
    /// live instance may serve the work.
    pub fn on_project(project: &str) -> Self {
        let mut invocation = Self::signed_in();
        if let Some(identity) = invocation.identity.as_mut() {
            identity.project = Some(project.to_owned());
            identity.command_authority = Authority::Project;
        }
        invocation
    }

    /// A machine with no native profile at all: nothing has told `ds` who is
    /// running it, and the identity is adopted from the live instances.
    pub fn unprovisioned() -> Self {
        Self { identity: None }
    }

    /// Run one command's real handler over its real arguments.
    pub fn run(
        &self,
        command: &Command,
        handler: fn(&Inputs, &Context) -> Result<Value, Failure>,
        tokens: &[&str],
    ) -> Result<Value, Failure> {
        let owned: Vec<String> = tokens.iter().map(|token| (*token).to_owned()).collect();
        let inputs = parse(command, &owned)?;
        let _scope = ops::scope_headless_identity(self.scoped(command, &inputs));
        handler(&inputs, &Self::context())
    }

    /// Resolve the instance a paired invocation is for, and send one declared
    /// operation to it — the two halves of every paired domain command.
    pub fn invoke(
        &self,
        target: Option<&str>,
        op: &BridgeOp,
        arguments: Value,
    ) -> Result<Value, Failure> {
        let mut identity = self.identity.clone();
        if let Some(identity) = identity.as_mut() {
            identity.target = target.map(str::to_owned);
        }
        let _scope = ops::scope_headless_identity(identity);
        let found = ds_cli_desktop::bridge::paired(None)?;
        ops::invoke(&found.descriptor, op, arguments, Duration::from_secs(10))
    }

    /// The same, with a descriptor FILE pinned as well — the legacy explicit
    /// path a desktop's own `cl` terminal sets.
    pub fn invoke_pinned(
        &self,
        descriptor: &Path,
        target: Option<&str>,
        op: &BridgeOp,
        arguments: Value,
    ) -> Result<Value, Failure> {
        let mut identity = self.identity.clone();
        if let Some(identity) = identity.as_mut() {
            identity.target = target.map(str::to_owned);
        }
        let _scope = ops::scope_headless_identity(identity);
        let found = ds_cli_desktop::bridge::paired(descriptor.to_str())?;
        ops::invoke(&found.descriptor, op, arguments, Duration::from_secs(10))
    }

    /// Resolution alone, for the claims that are about where an operation would
    /// have gone — and that must therefore reach no instance at all.
    pub fn resolve(&self, target: Option<&str>) -> Result<String, Failure> {
        let mut identity = self.identity.clone();
        if let Some(identity) = identity.as_mut() {
            identity.target = target.map(str::to_owned);
        }
        let _scope = ops::scope_headless_identity(identity);
        ds_cli_desktop::bridge::paired(None).map(|found| found.descriptor.instance_id.clone())
    }

    /// The requirement dispatch would scope for this invocation.
    pub fn requirement(&self) -> Option<Requirement> {
        let _scope = ops::scope_headless_identity(self.identity.clone());
        ops::scoped_requirement()
    }

    fn scoped(&self, command: &Command, inputs: &Inputs) -> Option<HeadlessIdentity> {
        let mut identity = self.identity.clone()?;
        identity.command_authority = command.authority;
        // `registry::host_target`, verbatim: only a `--target` declared with the
        // host's own placeholder names a runtime, and `DS_TARGET` is that flag's
        // session default.
        identity.target = command
            .arg(ops::TARGET_ARG.name)
            .filter(|arg| arg.value == ops::TARGET_ARG.value)
            .and_then(|arg| inputs.value(arg.name))
            .map(str::to_owned)
            .or_else(ops::env_target);
        Some(identity)
    }

    fn context() -> Context {
        Context {
            confirmed: true,
            output: Output {
                format: Format::Json,
                pretty: false,
                color: false,
            },
        }
    }
}

/// An explicit target naming one live instance.
pub fn target(instance_id: &str) -> String {
    format!("desktop:{instance_id}")
}

pub fn instance_target(instance_id: &str) -> Target {
    Target {
        instance_id: instance_id.to_owned(),
        window: None,
    }
}

/// Take the refusal out of a result whose success side holds a pairing token
/// and therefore has no `Debug`.
pub fn refused<T>(result: Result<T, Failure>, what: &str) -> Failure {
    match result {
        Ok(_) => panic!("expected a refusal: {what}"),
        Err(failure) => failure,
    }
}

/// Every instance in this test received nothing at all.
pub fn untouched(bridges: &[&Bridge]) {
    for bridge in bridges {
        assert!(
            bridge.invoked().is_empty(),
            "instance {} performed {:?}; it should have performed nothing",
            bridge.instance_id,
            bridge.invoked(),
        );
    }
}

// ---------------------------------------------------------------------------
// The executable itself, for the one test that drives argv
// ---------------------------------------------------------------------------

/// Where the built `ds` is, if it has been built.
///
/// `DS_BIN` names it outright; otherwise it is the sibling of this test binary
/// in the same profile directory, which is where `cargo build -p ds --bin ds`
/// leaves it for the target directory this test is running out of.
pub fn ds_executable() -> PathBuf {
    if let Some(named) = std::env::var_os("DS_BIN") {
        let named = PathBuf::from(named);
        assert!(named.is_file(), "DS_BIN does not name a file: {named:?}");
        return named;
    }
    let current = std::env::current_exe().expect("this test binary");
    // …/<profile>/deps/instances-<hash> → …/<profile>/ds
    let profile = current
        .parent()
        .and_then(Path::parent)
        .expect("a profile directory");
    let executable = profile.join(if cfg!(windows) { "ds.exe" } else { "ds" });
    assert!(
        executable.is_file(),
        "the ds executable is not built at {executable:?}; run `cargo build -p ds --bin ds` first",
    );
    executable
}

/// Run `ds` as the operator runs it, against this machine's registry, and read
/// its JSON envelope.
pub fn run_ds(executable: &Path, machine: &Machine, args: &[&str]) -> Value {
    let output = std::process::Command::new(executable)
        .args(args)
        .env("XDG_DATA_HOME", machine.root())
        .env_remove(discover::DESCRIPTOR_ENV)
        .env_remove(ops::TARGET_ENV)
        .output()
        .expect("the ds executable runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    serde_json::from_str(stdout.trim())
        .or_else(|_| serde_json::from_str(stderr.trim()))
        .unwrap_or_else(|_| {
            panic!("ds {args:?} answered no JSON envelope\nstdout: {stdout}\nstderr: {stderr}")
        })
}
