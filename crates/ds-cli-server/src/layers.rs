//! The Server's layer routes: the drawer's catalogue, visibility and order,
//! driven by a client with no Tauri process, browser, paired map or renderer.
//!
//! Every decision is `ds_layer_ops`' — the same owner `ds map layer …` calls
//! natively. This module only binds it to the owner-only socket:
//! the request must come from the owner's own account and pass the
//! native authorizer (`access` in `host.rs`), the lane is the connection's,
//! and the principal comes from the native identity the Server is bound to,
//! observed at request time. A client never names a lane, account or
//! preference scope; preferences are keyed by the DS account the document was
//! read under, so another account reaching this host after a restart sees its
//! own defaults and never the previous account's toggles.
//!
//! **The project is named by the caller** (`?project=<exact-id>`), recorded
//! by the kernel as the operation's context, and then READ: the document
//! source is opened for that exact project, so the owner's second project is
//! served on its own terms and this machine's saved selection is never
//! consulted. The named project is then held against the document that
//! actually comes back — a document under another project is
//! `project_context_changed`, never applied. That is the whole of the project
//! fence here: name it, read it, and refuse anything that answers about
//! something else.
//!
//! Answers are the owner's projections, verbatim. There is no renderer here:
//! `writes` name the layout word a renderer would apply, and nothing reports a
//! mount that did not happen.

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use ds_cli_contract::outcome::{ExitClass, Failure};
use ds_layer_ops::{
    DefaultVisibilityReceipt, DefaultVisibilityRequest, DocumentRead, LayerDocuments, ListRequest,
    Order, OrderReceipt, OrderRequest, Preferences, Scope, VisibilityDefault, VisibilityRequest,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::host::App;

/// The operation ids a layer request is admitted under, from the kernel's
/// closed vocabulary.
const LAYER_READ: &str = ds_command_kernel::execution_context::LAYER_READ;
const LAYER_WRITE: &str = ds_command_kernel::execution_context::LAYER_WRITE;

/// One layer document source, held to the project the caller named.
///
/// The source is already opened for that project, so this is the check that
/// the answer AGREES: a document, a scope recheck or an order receipt that
/// comes back under another project stops the request instead of being applied
/// to it. It costs nothing and it is the only thing standing between a source
/// bug and a preference written into the wrong project.
struct Fenced {
    inner: Box<dyn LayerDocuments + Send>,
    project: String,
}

impl Fenced {
    fn refuse(&self, actual: &str) -> Failure {
        Failure::conflict(
            "project_context_changed",
            format!(
                "this server's layer document is for another project than the one this request named ({})",
                self.project
            ),
        )
        .remedy(format!(
            "repeat the request with --project {actual}, or select {} on the server's account",
            self.project
        ))
    }
}

impl LayerDocuments for Fenced {
    fn read(&mut self, refresh: bool) -> Result<DocumentRead, Failure> {
        let read = self.inner.read(refresh)?;
        if read.scope.project != self.project {
            return Err(self.refuse(&read.scope.project));
        }
        Ok(read)
    }
    fn check_scope(&mut self, expected: &Scope) -> Result<(), Failure> {
        if expected.project != self.project {
            return Err(self.refuse(&expected.project));
        }
        self.inner.check_scope(expected)
    }
    fn reorder(&mut self, orders: &[Order]) -> Result<OrderReceipt, Failure> {
        let receipt = self.inner.reorder(orders)?;
        if receipt.project != self.project {
            return Err(self.refuse(&receipt.project));
        }
        Ok(receipt)
    }
    fn set_default_visibility(
        &mut self,
        defaults: &[VisibilityDefault],
    ) -> Result<DefaultVisibilityReceipt, Failure> {
        let receipt = self.inner.set_default_visibility(defaults)?;
        if receipt.project != self.project {
            return Err(self.refuse(&receipt.project));
        }
        Ok(receipt)
    }
}

/// What the Server binds the owner to: a document source under the bound
/// identity, for the project the CALLER named, and this host's preference
/// root. Production is native; tests are fixtures.
///
/// The project is a parameter and not a property of the host, which is the
/// whole of the difference between a Server that serves its owner's projects
/// and one that serves whichever project happened to be selected.
pub trait LayerHost: Send + Sync + 'static {
    fn documents(&self, project: &str) -> Result<Box<dyn LayerDocuments + Send>, Failure>;
    fn preferences(&self) -> Result<Preferences, Failure>;
}

/// The native binding: `ds-cli-auth` under the connection's lane, and the
/// shared native layer store (`DS_LAYER_HOME` or the local data directory).
pub struct NativeLayerHost {
    lane: String,
    binding: Option<(String, Arc<dyn ds_compute_runtime::Authorizer>)>,
}
impl NativeLayerHost {
    #[cfg(test)]
    pub fn fixture_native(lane: &str) -> Arc<dyn LayerHost> {
        Arc::new(Self {
            lane: lane.to_owned(),
            binding: None,
        })
    }
    pub fn bound(
        lane: &str,
        owner: String,
        auth: Arc<dyn ds_compute_runtime::Authorizer>,
    ) -> Arc<dyn LayerHost> {
        Arc::new(Self {
            lane: lane.to_owned(),
            binding: Some((owner, auth)),
        })
    }
}
impl LayerHost for NativeLayerHost {
    fn documents(&self, project: &str) -> Result<Box<dyn LayerDocuments + Send>, Failure> {
        if let Some((owner, auth)) = &self.binding {
            let owner = owner.clone();
            let auth = auth.clone();
            let lane = self.lane.clone();
            return Ok(Box::new(ds_layer_ops::Native::guarded_for_project(
                &self.lane,
                project,
                Box::new(move |fence| {
                    let actual = ds_compute_runtime::digest(
                        &serde_json::to_vec(&(fence.uid(), &lane, fence.audience()))
                            .expect("identity tuple"),
                    );
                    authorize_captured_owner(auth.as_ref(), &owner, &actual)
                }),
            )));
        }
        Ok(Box::new(ds_layer_ops::Native::for_project(
            &self.lane, project,
        )))
    }
    fn preferences(&self) -> Result<Preferences, Failure> {
        Preferences::native()
    }
}

fn authorize_captured_owner(
    auth: &dyn ds_compute_runtime::Authorizer,
    owner: &str,
    captured: &str,
) -> Result<(), Failure> {
    if captured != owner {
        return Err(Failure::unauthorized(
            "server_owner_changed",
            "the captured layer account differs from the Server owner",
        )
        .remedy("restart the Server under the intended account"));
    }
    auth.authorize(owner).map_err(|message| {
        Failure::unauthorized("server_owner_changed", message)
            .remedy("restart the Server under the intended account")
    })
}

/// A typed refusal on the wire: the CLI's class and code, the sentence and
/// the remedy, under the HTTP status the class maps to.
pub fn refusal(failure: &Failure) -> Response {
    let status = match failure.class() {
        ExitClass::Success => StatusCode::OK,
        ExitClass::InvalidInput => StatusCode::BAD_REQUEST,
        ExitClass::Unauthorized => StatusCode::UNAUTHORIZED,
        ExitClass::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        ExitClass::Conflict => StatusCode::CONFLICT,
        ExitClass::Failed => StatusCode::BAD_GATEWAY,
        ExitClass::Internal => StatusCode::INTERNAL_SERVER_ERROR,
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
    (status, Json(body)).into_response()
}

pub(crate) fn invalid(message: impl Into<String>) -> Response {
    refusal(&Failure::invalid("invalid_input", message).remedy("send the documented request body"))
}

pub(crate) async fn run<T: Send + 'static>(
    app: App,
    operation_id: &'static str,
    project: Option<String>,
    about: Vec<u8>,
    operation: impl FnOnce(&mut dyn LayerDocuments, &Preferences) -> Result<T, Failure> + Send + 'static,
) -> Result<T, Response> {
    tokio::task::spawn_blocking(move || {
        // Middleware admission can be separated from this queued native task.
        // Recheck the original Server owner at the effect boundary.
        app.auth
            .authorize(&app.connection.owner)
            .map_err(|message| {
                Failure::unauthorized("server_owner_changed", message)
                    .remedy("restart the Server under the current native account")
            })?;
        // The project is the caller's and is recorded here, before anything
        // is read: an unnamed project is `project_required`, an unbounded
        // name is `context_corrupt`, and nothing is fetched to decide either.
        let context = app.sessions.admit_read(
            operation_id,
            project.as_deref(),
            &about,
            ds_compute_runtime::now_ms(),
        )?;
        // The recorded project is the one the document source is opened for:
        // the Server reads what the caller named, not what this machine last
        // selected. `Fenced` then holds that name against what comes back.
        let mut documents = Fenced {
            inner: app.layers.documents(&context.project)?,
            project: context.project,
        };
        let preferences = app.layers.preferences()?;
        operation(&mut documents, &preferences)
    })
    .await
    .map_err(|_| refusal(&Failure::internal("server_refused", "native task failed")))?
    .map_err(|failure| refusal(&failure))
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ListQuery {
    project: Option<String>,
    refresh: Option<bool>,
    limit: Option<i64>,
    zoom: Option<i64>,
}

/// `?project=<exact-id>` on the two write routes, whose bodies stay exactly
/// the `ds_layer_ops` request types `ds map layer …` sends.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ScopeQuery {
    pub(crate) project: Option<String>,
}

/// `GET /v1/layers?project=&refresh=&limit=&zoom=` — the canonical catalogue.
pub async fn list(State(app): State<App>, query: Option<Query<ListQuery>>) -> Response {
    let Some(Query(query)) = query else {
        return invalid("layers query accepts project, refresh, limit and zoom only");
    };
    let request = ListRequest {
        refresh: query.refresh.unwrap_or(false),
        limit: query.limit,
        zoom: query.zoom,
    };
    let about = format!(
        "list:{}:{}:{}",
        request.refresh,
        request.limit.unwrap_or_default(),
        request.zoom.unwrap_or_default()
    );
    match run(
        app,
        LAYER_READ,
        query.project,
        about.into_bytes(),
        move |documents, preferences| ds_layer_ops::list(documents, preferences, &request),
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(response) => response,
    }
}

/// `POST /v1/layers/visibility?project= {"layers": [canonical ids], "visible": bool}`.
pub async fn visibility(
    State(app): State<App>,
    query: Option<Query<ScopeQuery>>,
    body: axum::body::Bytes,
) -> Response {
    let Some(Query(query)) = query else {
        return invalid("this route accepts a project query parameter only");
    };
    let request: VisibilityRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => return invalid(format!("invalid visibility request: {error}")),
    };
    match run(
        app,
        LAYER_WRITE,
        query.project,
        body.to_vec(),
        move |documents, preferences| {
            ds_layer_ops::set_visibility(documents, preferences, &request)
        },
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(response) => response,
    }
}

/// `POST /v1/layers/order?project= {"orders": [{"layer_id", "order"}]}`.
pub async fn order(
    State(app): State<App>,
    query: Option<Query<ScopeQuery>>,
    body: axum::body::Bytes,
) -> Response {
    let Some(Query(query)) = query else {
        return invalid("this route accepts a project query parameter only");
    };
    let request: OrderRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => return invalid(format!("invalid order request: {error}")),
    };
    match run(
        app,
        LAYER_WRITE,
        query.project,
        body.to_vec(),
        move |documents, _| ds_layer_ops::reorder(documents, &request),
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(response) => response,
    }
}

/// `POST /v1/layers/default-visibility?project=...`.
pub async fn default_visibility(
    State(app): State<App>,
    query: Option<Query<ScopeQuery>>,
    body: axum::body::Bytes,
) -> Response {
    let Some(Query(query)) = query else {
        return invalid("this route accepts a project query parameter only");
    };
    let request: DefaultVisibilityRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => return invalid(format!("invalid visibility-default request: {error}")),
    };
    match run(
        app,
        LAYER_WRITE,
        query.project,
        body.to_vec(),
        move |documents, _| ds_layer_ops::set_default_visibility(documents, &request),
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(response) => response,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    // Everything below the first test drives the real owner-only socket, which
    // only Unix has; elsewhere its fixtures stand unused rather than absent.
    #![cfg_attr(not(unix), allow(dead_code, unused_imports))]
    #[test]
    fn captured_account_cannot_pass_after_current_account_switches_back() {
        struct Accept;
        impl ds_compute_runtime::Authorizer for Accept {
            fn authorize(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
        }
        assert!(super::authorize_captured_owner(&Accept, "owner-a", "owner-a").is_ok());
        let error = super::authorize_captured_owner(&Accept, "owner-a", "owner-b").unwrap_err();
        assert_eq!(error.code(), "server_owner_changed");
        struct Revoked;
        impl ds_compute_runtime::Authorizer for Revoked {
            fn authorize(&self, _: &str) -> Result<(), String> {
                Err("credential revoked".into())
            }
        }
        assert!(super::authorize_captured_owner(&Revoked, "owner-a", "owner-a").is_err());
    }
    // The realistic workflow through the REAL owner-only socket with a fixture
    // identity and a fixture upstream: list, hide, restart, list the retained
    // state, show, reorder, invalid id, unauthorized, revoked, project change
    // during a request, account change across restart. Fixtures are not a
    // Canary proof; they prove the host boundary and the shared owner.
    use super::*;
    use crate::host::{Connection, router};
    use ds_compute_runtime::Authorizer;
    use ds_layer_ops::{
        DefaultVisibilityReceipt, DocumentRead, Order, OrderReceipt, Scope, VisibilityDefault,
    };
    use serde_json::Value;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Auth(Arc<AtomicBool>);
    impl Authorizer for Auth {
        fn authorize(&self, _: &str) -> Result<(), String> {
            if self.0.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err("device revoked".into())
            }
        }
    }

    /// The fixture upstream: the account's projects, each with the document
    /// that account reads for it, the reorders it received, and a switch that
    /// makes it answer about somebody else's project mid-flight.
    ///
    /// It has SEVERAL projects because the account has several: the Server
    /// opens its source for the project a request named, exactly as
    /// `ds_layer_ops::Native::for_project` does against the gateway. A project
    /// this account cannot read is the source's own refusal, which is what
    /// comes back from the gateway — not something the Server decides from a
    /// directory it does not hold.
    pub(crate) struct Upstream {
        uid: String,
        documents: BTreeMap<String, Value>,
        reorders: Mutex<Vec<(String, Vec<Order>)>>,
        pub(crate) switch_project_on_read: AtomicBool,
    }
    struct FixtureDocuments {
        upstream: Arc<Upstream>,
        project: String,
    }
    impl FixtureDocuments {
        fn scope(&self) -> Scope {
            Scope {
                lane: "canary".into(),
                uid: self.upstream.uid.clone(),
                project: self.project.clone(),
            }
        }
    }
    impl LayerDocuments for FixtureDocuments {
        fn read(&mut self, _refresh: bool) -> Result<DocumentRead, Failure> {
            let Some(document) = self.upstream.documents.get(&self.project) else {
                return Err(Failure::unauthorized(
                    "auth_rejected",
                    "this account reads no layer configuration for that project",
                )
                .remedy("run ds auth project list and name a project this account can read"));
            };
            let mut document = document.clone();
            let mut scope = self.scope();
            if self.upstream.switch_project_on_read.load(Ordering::SeqCst) {
                document["project_id"] = json!("someone-elses-project");
                scope.project = "someone-elses-project".into();
            }
            Ok(DocumentRead { scope, document })
        }
        fn check_scope(&mut self, expected: &Scope) -> Result<(), Failure> {
            if &self.scope() == expected {
                Ok(())
            } else {
                Err(
                    Failure::conflict("project_context_changed", "fixture scope changed")
                        .remedy("repeat the layer request"),
                )
            }
        }
        fn reorder(&mut self, orders: &[Order]) -> Result<OrderReceipt, Failure> {
            self.upstream
                .reorders
                .lock()
                .unwrap()
                .push((self.project.clone(), orders.to_vec()));
            Ok(OrderReceipt {
                project: self.project.clone(),
                reordered: orders.len(),
            })
        }
        fn set_default_visibility(
            &mut self,
            defaults: &[VisibilityDefault],
        ) -> Result<DefaultVisibilityReceipt, Failure> {
            Ok(DefaultVisibilityReceipt {
                project: self.project.clone(),
                updated: defaults.len(),
            })
        }
    }
    struct FixtureHost {
        upstream: Arc<Upstream>,
        root: std::path::PathBuf,
    }
    impl LayerHost for FixtureHost {
        fn documents(&self, project: &str) -> Result<Box<dyn LayerDocuments + Send>, Failure> {
            Ok(Box::new(FixtureDocuments {
                upstream: self.upstream.clone(),
                project: project.to_owned(),
            }))
        }
        fn preferences(&self) -> Result<Preferences, Failure> {
            Ok(Preferences::at(self.root.clone()))
        }
    }

    fn document(project: &str, lines: &str) -> Value {
        json!({
            "project_id": project,
            "sources": {"survey_geo": {"type": "geojson"}, "design_vt": {"type": "vector"}},
            "styles": {}, "style_editors": [],
            "layers": [
                {"id": "ds-poles", "type": "circle", "source": "survey_geo", "style_ref": "poles", "metadata": {"config_layer_id": "survey/poles", "label": "Poles", "layer_class": "survey", "geometry_type": "Point", "order": 10}},
                {"id": "ds-poles__label", "type": "symbol", "source": "survey_geo", "style_ref": "poles__label", "metadata": {"config_layer_id": "survey/poles", "parent_layer": "ds-poles"}},
                {"id": "ds-lines", "type": "line", "source": "design_vt", "source-layer": "lv", "style_ref": "lines", "minzoom": 12, "metadata": {"config_layer_id": lines, "label": "LV Lines", "layer_class": "design_tile", "geometry_type": "LineString", "order": 20}}
            ],
            // The survey form catalogue the working-area routes read; the
            // second project deliberately has a different one.
            "survey_layers": if lines == "design/lines" {
                json!([
                    {"key": "poles", "label": "Poles", "geometry_type": "Point", "layer_ids": ["ds-poles", "ds-poles__label"], "style_ref": "poles"},
                    {"key": "customers", "label": "Customers", "geometry_type": "Point", "layer_ids": ["ds-customers"], "style_ref": "customers"}
                ])
            } else {
                json!([
                    {"key": "mv_poles", "label": "MV poles", "geometry_type": "Point", "layer_ids": ["ds-mv-poles"], "style_ref": "mv_poles"}
                ])
            }
        })
    }
    /// The two projects this account can read. `proj-lome`'s catalogue is
    /// deliberately not `proj-kigali`'s, so a test cannot pass by serving the
    /// wrong one.
    pub(crate) fn fixture_upstream(uid: &str) -> Arc<Upstream> {
        Arc::new(Upstream {
            uid: uid.into(),
            documents: [
                (
                    "proj-kigali".to_owned(),
                    document("proj-kigali", "design/lines"),
                ),
                (
                    "proj-lome".to_owned(),
                    document("proj-lome", "design/mv_lines"),
                ),
            ]
            .into_iter()
            .collect(),
            reorders: Mutex::new(vec![]),
            switch_project_on_read: AtomicBool::new(false),
        })
    }
    /// One running Server: the real owner-only socket in `dir`, answered by
    /// the production accept loop.
    #[cfg(unix)]
    pub(crate) struct Running {
        pub(crate) socket: std::path::PathBuf,
        handle: tokio::task::JoinHandle<()>,
    }
    #[cfg(unix)]
    impl Running {
        /// Stop it and wait until its socket and lock are released, as a
        /// host that exits does.
        pub(crate) async fn stop(self) {
            self.handle.abort();
            let _ = self.handle.await;
        }
    }
    #[cfg(unix)]
    pub(crate) async fn start(
        dir: &std::path::Path,
        upstream: Arc<Upstream>,
        allowed: Arc<AtomicBool>,
    ) -> Running {
        let listening = crate::transport::listen(dir).unwrap();
        let socket = listening.socket().to_owned();
        let connection = Connection {
            owner: "owner-digest".into(),
            lane: "canary".into(),
            socket: socket.clone(),
            legacy_address: None,
        };
        let app = App {
            database: dir.join("store.sqlite"),
            sessions: crate::server_sync::sessions::ServerSessions::with(
                connection.clone(),
                dir.join("store.sqlite"),
                ds_command_kernel::execution_context::Limits {
                    global_running: 2,
                    per_project_running: 1,
                    per_project_queued: 4,
                    global_queued: 8,
                },
                ds_compute_runtime::HostIdentity {
                    owner: connection.owner.clone(),
                    principal: ds_command_kernel::execution_context::Principal {
                        uid: upstream.uid.clone(),
                        lane: "canary".into(),
                        deployment: "https://gateway.example".into(),
                        install_id: "install-1".into(),
                    },
                },
                Arc::new(crate::host::tests::NoGateway),
            ),
            connection,
            os_uid: crate::transport::own_uid(),
            auth: Arc::new(Auth(allowed)),
            requests: Arc::new(crate::host::Door::new(4)),
            activity: None,
            solar: Arc::new(crate::solar_application::Applications::default()),
            layers: Arc::new(FixtureHost {
                upstream,
                root: dir.join("layers"),
            }),
        };
        let handle = tokio::spawn(async move {
            crate::transport::serve(listening, router(app), std::future::pending())
                .await
                .unwrap();
        });
        Running { socket, handle }
    }
    /// The client half, over the socket exactly as `ds map layer …
    /// --target server` reaches it.
    #[cfg(unix)]
    fn call(
        socket: &std::path::Path,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let body = body.map(|body| serde_json::to_vec(&body).unwrap());
        let reply = crate::transport::call(
            socket,
            method,
            path,
            &[("content-type", "application/json")],
            body.as_deref(),
            64 * 1024 * 1024,
            std::time::Duration::from_secs(60),
        )
        .unwrap();
        (
            reply.status,
            serde_json::from_slice(&reply.body).unwrap_or(Value::Null),
        )
    }
    #[cfg(unix)]
    pub(crate) async fn wire(
        socket: &std::path::Path,
        method: &'static str,
        path: &'static str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let socket = socket.to_owned();
        tokio::task::spawn_blocking(move || call(&socket, method, path, body))
            .await
            .unwrap()
    }
    fn row<'a>(listing: &'a Value, id: &str) -> &'a Value {
        listing["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap()
    }

    /// The project is the caller's word, and the Server checks it two ways:
    /// it must be there, and it must be the one the document that comes back
    /// is actually for. Nothing is fetched to decide either.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn the_project_is_named_by_the_caller_and_never_assumed() {
        let dir = tempfile::tempdir().unwrap();
        let upstream = fixture_upstream("uid-a");
        let allowed = Arc::new(AtomicBool::new(true));
        let server = start(dir.path(), upstream.clone(), allowed).await;

        // No project at all.
        let (status, refused) = wire(&server.socket, "GET", "/v1/layers", None).await;
        assert_eq!(status, 400, "{refused}");
        assert_eq!(refused["code"], "project_required");
        assert_eq!(refused["class"], "invalid_input");

        // A name outside the kernel's bound: refused under the kernel's own
        // word for it, before anything is read.
        let (status, padded) =
            wire(&server.socket, "GET", "/v1/layers?project=%20padded", None).await;
        assert_eq!(status, 400, "{padded}");
        assert_eq!(padded["code"], "context_corrupt");

        // A project this account cannot read is the SOURCE's refusal, raised
        // where the account is actually established — never an answer the
        // Server invents from a directory it does not hold, and never another
        // project's catalogue served under the requested name.
        let (status, elsewhere) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-nowhere",
            None,
        )
        .await;
        assert_eq!(status, 401, "{elsewhere}");
        assert_eq!(elsewhere["code"], "auth_rejected");

        // A write is refused the same way, and writes nothing.
        let before = ds_layer_store::visibility::read_at(
            &dir.path().join("layers"),
            "canary",
            "uid-a",
            "proj-kigali",
        )
        .unwrap();
        for (path, body) in [
            (
                "/v1/layers/visibility?project=proj-nowhere",
                json!({"layers": ["survey/poles"], "visible": false}),
            ),
            (
                "/v1/layers/order?project=proj-nowhere",
                json!({"orders": [{"layer_id": "survey/poles", "order": 100}]}),
            ),
        ] {
            let (status, refused) = call(&server.socket, "POST", path, Some(body));
            assert_eq!(status, 401, "{refused}");
            assert_eq!(refused["code"], "auth_rejected");
        }
        assert_eq!(
            ds_layer_store::visibility::read_at(
                &dir.path().join("layers"),
                "canary",
                "uid-a",
                "proj-kigali"
            )
            .unwrap(),
            before,
            "a refused project wrote nothing under any project"
        );
        assert!(upstream.reorders.lock().unwrap().is_empty());

        // Named correctly, the same request is served.
        let (status, listing) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali",
            None,
        )
        .await;
        assert_eq!(status, 200, "{listing}");
        assert_eq!(listing["project"], "proj-kigali");

        // And the fence still bites where it is the only thing that can: a
        // source that answers about ANOTHER project than the one it was
        // opened for. Nothing is applied to the named project's preferences.
        upstream
            .switch_project_on_read
            .store(true, Ordering::SeqCst);
        let (status, switched) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali",
            None,
        )
        .await;
        assert_eq!(status, 409, "{switched}");
        assert_eq!(switched["code"], "project_context_changed");
        assert!(
            switched["remedy"]
                .as_str()
                .unwrap()
                .contains("someone-elses-project"),
            "the remedy names what came back: {switched}"
        );
        upstream
            .switch_project_on_read
            .store(false, Ordering::SeqCst);
        server.stop().await;
    }

    /// F3: the Server serves ANY project its owner names, not only whichever
    /// one this machine has selected. Two of the account's projects are read
    /// through one running host, each answers its own catalogue, and each
    /// remembers its own toggles — a hide in one is invisible in the other.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_second_project_the_owner_names_is_served_on_its_own_terms() {
        let dir = tempfile::tempdir().unwrap();
        let upstream = fixture_upstream("uid-a");
        let allowed = Arc::new(AtomicBool::new(true));
        let server = start(dir.path(), upstream.clone(), allowed).await;

        // Two catalogues, from one host, without anything being selected.
        let (status, kigali) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali",
            None,
        )
        .await;
        assert_eq!(status, 200, "{kigali}");
        assert_eq!(kigali["project"], "proj-kigali");
        let (status, lome) =
            wire(&server.socket, "GET", "/v1/layers?project=proj-lome", None).await;
        assert_eq!(status, 200, "{lome}");
        assert_eq!(lome["project"], "proj-lome");
        assert_eq!(
            row(&lome, "design/mv_lines")["label"],
            "LV Lines",
            "the second project answered its OWN document, not the first's: {lome}"
        );
        assert!(
            lome["layers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|layer| layer["id"] != "design/lines"),
            "the first project's catalogue leaked into the second: {lome}"
        );

        // A write lands in the project it named, and only there.
        let (status, hidden) = wire(
            &server.socket,
            "POST",
            "/v1/layers/visibility?project=proj-lome",
            Some(json!({"layers": ["survey/poles"], "visible": false})),
        )
        .await;
        assert_eq!(status, 200, "{hidden}");
        assert_eq!(hidden["project"], "proj-lome");
        let (_, lome) = wire(&server.socket, "GET", "/v1/layers?project=proj-lome", None).await;
        assert_eq!(
            row(&lome, "survey/poles")["visibility"]["any_visible"],
            false
        );
        let (_, kigali) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali",
            None,
        )
        .await;
        assert_eq!(
            row(&kigali, "survey/poles")["visibility"]["any_visible"],
            true,
            "hiding a family in one project hid it in the other"
        );

        // The governed order write reaches the source under the same name.
        let (status, ordered) = wire(
            &server.socket,
            "POST",
            "/v1/layers/order?project=proj-lome",
            Some(json!({"orders": [{"layer_id": "survey/poles", "order": 100}]})),
        )
        .await;
        assert_eq!(status, 200, "{ordered}");
        assert_eq!(ordered["project"], "proj-lome");
        assert_eq!(
            upstream.reorders.lock().unwrap().first().unwrap().0,
            "proj-lome"
        );
        server.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn the_workflow_runs_through_a_real_listener_and_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let upstream = fixture_upstream("uid-a");
        let allowed = Arc::new(AtomicBool::new(true));
        let server = start(dir.path(), upstream.clone(), allowed.clone()).await;

        // list: every family reads as the document authored it, zoom reported
        let (status, listing) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali&zoom=8",
            None,
        )
        .await;
        assert_eq!(status, 200, "{listing}");
        assert_eq!(listing["layer_count"], 2);
        assert_eq!(listing["lane"], "canary");
        assert_eq!(listing["project"], "proj-kigali");
        assert_eq!(
            row(&listing, "survey/poles")["visibility"]["any_visible"],
            true
        );
        assert_eq!(row(&listing, "design/lines")["in_zoom_range"], false);
        assert_eq!(listing["visibility_source"], "native_local");

        // hide: the family is written, the answer reads hidden
        let (status, hidden) = wire(
            &server.socket,
            "POST",
            "/v1/layers/visibility?project=proj-kigali",
            Some(json!({"layers": ["survey/poles"], "visible": false})),
        )
        .await;
        assert_eq!(status, 200, "{hidden}");
        assert_eq!(hidden["layers"][0]["visibility"]["any_visible"], false);
        assert_eq!(
            hidden["changed"],
            json!(["ds-poles", "ds-poles__label"]),
            "the label companion follows its family"
        );
        assert!(
            hidden["writes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|w| w["runtime"] == "none")
        );

        // restart: a new process over the same state; the toggle is retained
        server.stop().await;
        let server = start(dir.path(), upstream.clone(), allowed.clone()).await;
        let (status, listing) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali",
            None,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(
            row(&listing, "survey/poles")["visibility"]["any_visible"],
            false,
            "retained across restart"
        );
        assert_eq!(
            row(&listing, "design/lines")["visibility"]["all_visible"],
            true
        );

        // show: back to visible, idempotent on repeat
        let (status, shown) = wire(
            &server.socket,
            "POST",
            "/v1/layers/visibility?project=proj-kigali",
            Some(json!({"layers": ["survey/poles"], "visible": true})),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(shown["changed"], json!(["ds-poles", "ds-poles__label"]));
        let (_, again) = wire(
            &server.socket,
            "POST",
            "/v1/layers/visibility?project=proj-kigali",
            Some(json!({"layers": ["survey/poles"], "visible": true})),
        )
        .await;
        assert_eq!(again["changed"], json!([]));

        // reorder: admitted, then written upstream; partial says what it leaves unlisted
        let (status, ordered) = wire(
            &server.socket,
            "POST",
            "/v1/layers/order?project=proj-kigali",
            Some(json!({"orders": [{"layer_id": "survey/poles", "order": 100}]})),
        )
        .await;
        assert_eq!(status, 200, "{ordered}");
        assert_eq!(ordered["applied"], true);
        assert_eq!(ordered["complete"], false);
        assert_eq!(ordered["unlisted"], json!(["design/lines"]));
        assert_eq!(upstream.reorders.lock().unwrap().len(), 1);

        // project default: the Server transports the same shared request and
        // the owner admits a canonical id before the governed write.
        let (status, defaulted) = wire(
            &server.socket,
            "POST",
            "/v1/layers/default-visibility?project=proj-kigali",
            Some(json!({"defaults": [{"layer_id": "survey/poles", "visible": false}]})),
        )
        .await;
        assert_eq!(status, 200, "{defaulted}");
        assert_eq!(defaulted["project"], "proj-kigali");
        assert_eq!(defaulted["updated"], 1);

        // invalid ids: typed refusals, nothing written upstream or locally
        let (status, refused) = wire(
            &server.socket,
            "POST",
            "/v1/layers/order?project=proj-kigali",
            Some(json!({"orders": [{"layer_id": "ds-poles", "order": 1}]})),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(refused["code"], "unknown_layer");
        assert_eq!(refused["class"], "invalid_input");
        assert_eq!(upstream.reorders.lock().unwrap().len(), 1);
        let (status, refused) = wire(
            &server.socket,
            "POST",
            "/v1/layers/visibility?project=proj-kigali",
            Some(json!({"layers": ["ds-poles"], "visible": false})),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(refused["code"], "unknown_layer");
        let (status, refused) = wire(
            &server.socket,
            "POST",
            "/v1/layers/visibility?project=proj-kigali",
            Some(json!({"layer": "x"})),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(refused["code"], "invalid_input");
        let (status, refused) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali&limit=0",
            None,
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(refused["code"], "invalid_number");
        let (status, _) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali&foo=1",
            None,
        )
        .await;
        assert_eq!(status, 400);
        let before = ds_layer_store::visibility::read_at(
            &dir.path().join("layers"),
            "canary",
            "uid-a",
            "proj-kigali",
        )
        .unwrap();

        // unauthorized: a revoked device stops at the door. (A process of
        // another account stops there too, before this; that is the socket's
        // own proof, `host::tests::one_owner_per_server_is_the_socket_peer…`.)
        allowed.store(false, Ordering::SeqCst);
        let (status, _) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali",
            None,
        )
        .await;
        assert_eq!(status, 401);
        allowed.store(true, Ordering::SeqCst);
        assert_eq!(
            ds_layer_store::visibility::read_at(
                &dir.path().join("layers"),
                "canary",
                "uid-a",
                "proj-kigali"
            )
            .unwrap(),
            before,
            "refused requests wrote nothing"
        );

        // project change during a request: the document no longer matches the scope
        upstream
            .switch_project_on_read
            .store(true, Ordering::SeqCst);
        let (status, refused) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali",
            None,
        )
        .await;
        assert_eq!(status, 409, "{refused}");
        assert_eq!(refused["code"], "project_context_changed");
        upstream
            .switch_project_on_read
            .store(false, Ordering::SeqCst);

        // account change across restart: the new account sees its own defaults,
        // and the previous account's toggles are still on disk under its scope
        server.stop().await;
        let server = start(dir.path(), fixture_upstream("uid-b"), allowed.clone()).await;
        let (status, hidden_b) = wire(
            &server.socket,
            "POST",
            "/v1/layers/visibility?project=proj-kigali",
            Some(json!({"layers": ["design/lines"], "visible": false})),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(hidden_b["changed"], json!(["ds-lines"]));
        let (_, listing_b) = wire(
            &server.socket,
            "GET",
            "/v1/layers?project=proj-kigali",
            None,
        )
        .await;
        assert_eq!(
            row(&listing_b, "survey/poles")["visibility"]["any_visible"],
            true,
            "account b never inherited account a's toggles"
        );
        assert!(
            ds_layer_store::visibility::read_at(
                &dir.path().join("layers"),
                "canary",
                "uid-a",
                "proj-kigali"
            )
            .unwrap()["ds-poles"]
        );
        assert!(
            !ds_layer_store::visibility::read_at(
                &dir.path().join("layers"),
                "canary",
                "uid-b",
                "proj-kigali"
            )
            .unwrap()["ds-lines"]
        );
        server.stop().await;
    }
}
