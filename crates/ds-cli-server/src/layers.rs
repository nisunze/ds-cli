//! The Server's layer routes: the drawer's catalogue, visibility and order,
//! driven by a client with no Tauri process, browser, paired map or renderer.
//!
//! Every decision is `ds_layer_ops`' — the same owner `ds map layer …` calls
//! natively. This module only binds it to the protected loopback transport:
//! the request must carry the owner-only connection bearer and pass the
//! native authorizer (`access` in `host.rs`), the lane is the connection's,
//! and the principal and project come from the native identity the Server is
//! bound to, observed at request time. A client never names a lane, account
//! or preference scope; preferences are keyed by the DS account the document
//! was read under, so another account reaching this host after a restart sees
//! its own defaults and never the previous account's toggles.
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
use ds_layer_ops::{LayerDocuments, ListRequest, OrderRequest, Preferences, VisibilityRequest};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::host::App;

/// What the Server binds the owner to: a document source under the bound
/// identity and this host's preference root. Production is native; tests are
/// fixtures.
pub trait LayerHost: Send + Sync + 'static {
    fn documents(&self) -> Result<Box<dyn LayerDocuments + Send>, Failure>;
    fn preferences(&self) -> Result<Preferences, Failure>;
}

/// The native binding: `ds-cli-auth` under the connection's lane, and the
/// shared native layer store (`DS_LAYER_HOME` or the local data directory).
pub struct NativeLayerHost {
    lane: String,
}
impl NativeLayerHost {
    pub fn new(lane: &str) -> Arc<dyn LayerHost> {
        Arc::new(Self {
            lane: lane.to_owned(),
        })
    }
}
impl LayerHost for NativeLayerHost {
    fn documents(&self) -> Result<Box<dyn LayerDocuments + Send>, Failure> {
        Ok(Box::new(ds_layer_ops::Native::new(&self.lane)))
    }
    fn preferences(&self) -> Result<Preferences, Failure> {
        Preferences::native()
    }
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

fn invalid(message: impl Into<String>) -> Response {
    refusal(&Failure::invalid("invalid_input", message).remedy("send the documented request body"))
}

async fn run<T: Send + 'static>(
    app: App,
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
        let mut documents = app.layers.documents()?;
        let preferences = app.layers.preferences()?;
        operation(documents.as_mut(), &preferences)
    })
    .await
    .map_err(|_| refusal(&Failure::internal("server_refused", "native task failed")))?
    .map_err(|failure| refusal(&failure))
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ListQuery {
    refresh: Option<bool>,
    limit: Option<i64>,
    zoom: Option<i64>,
}

/// `GET /v1/layers?refresh=&limit=&zoom=` — the canonical catalogue.
pub async fn list(State(app): State<App>, query: Option<Query<ListQuery>>) -> Response {
    let Some(Query(query)) = query else {
        return invalid("layers query accepts refresh, limit and zoom only");
    };
    let request = ListRequest {
        refresh: query.refresh.unwrap_or(false),
        limit: query.limit,
        zoom: query.zoom,
    };
    match run(app, move |documents, preferences| {
        ds_layer_ops::list(documents, preferences, &request)
    })
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(response) => response,
    }
}

/// `POST /v1/layers/visibility {"layers": [canonical ids], "visible": bool}`.
pub async fn visibility(State(app): State<App>, body: axum::body::Bytes) -> Response {
    let request: VisibilityRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => return invalid(format!("invalid visibility request: {error}")),
    };
    match run(app, move |documents, preferences| {
        ds_layer_ops::set_visibility(documents, preferences, &request)
    })
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(response) => response,
    }
}

/// `POST /v1/layers/order {"orders": [{"layer_id", "order"}]}`.
pub async fn order(State(app): State<App>, body: axum::body::Bytes) -> Response {
    let request: OrderRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => return invalid(format!("invalid order request: {error}")),
    };
    match run(app, move |documents, _| {
        ds_layer_ops::reorder(documents, &request)
    })
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    //! The realistic workflow through a REAL loopback listener with a fixture
    //! identity and a fixture upstream: list, hide, restart, list the retained
    //! state, show, reorder, invalid id, unauthorized, revoked, project change
    //! during a request, account change across restart. Fixtures are not a
    //! Canary proof; they prove the host boundary and the shared owner.
    use super::*;
    use crate::host::{Connection, router};
    use ds_compute_runtime::Authorizer;
    use ds_layer_ops::{DocumentRead, Order, OrderReceipt, Scope};
    use serde_json::Value;
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

    /// The fixture upstream: one project's document under one account, with
    /// the reorders it received and a switch that flips the project mid-flight.
    struct Upstream {
        uid: String,
        project: String,
        document: Value,
        reorders: Mutex<Vec<Vec<Order>>>,
        switch_project_on_read: AtomicBool,
    }
    struct FixtureDocuments(Arc<Upstream>);
    impl LayerDocuments for FixtureDocuments {
        fn read(&mut self, _refresh: bool) -> Result<DocumentRead, Failure> {
            let mut document = self.0.document.clone();
            if self.0.switch_project_on_read.load(Ordering::SeqCst) {
                document["project_id"] = json!("someone-elses-project");
            }
            Ok(DocumentRead {
                scope: Scope {
                    lane: "canary".into(),
                    uid: self.0.uid.clone(),
                    project: self.0.project.clone(),
                },
                document,
            })
        }
        fn check_scope(&mut self, expected: &Scope) -> Result<(), Failure> {
            let actual = Scope {
                lane: "canary".into(),
                uid: self.0.uid.clone(),
                project: self.0.project.clone(),
            };
            if &actual == expected {
                Ok(())
            } else {
                Err(
                    Failure::conflict("project_context_changed", "fixture scope changed")
                        .remedy("repeat the layer request"),
                )
            }
        }
        fn reorder(&mut self, orders: &[Order]) -> Result<OrderReceipt, Failure> {
            self.0.reorders.lock().unwrap().push(orders.to_vec());
            Ok(OrderReceipt {
                project: self.0.project.clone(),
                reordered: orders.len(),
            })
        }
    }
    struct FixtureHost {
        upstream: Arc<Upstream>,
        root: std::path::PathBuf,
    }
    impl LayerHost for FixtureHost {
        fn documents(&self) -> Result<Box<dyn LayerDocuments + Send>, Failure> {
            Ok(Box::new(FixtureDocuments(self.upstream.clone())))
        }
        fn preferences(&self) -> Result<Preferences, Failure> {
            Ok(Preferences::at(self.root.clone()))
        }
    }

    fn document() -> Value {
        json!({
            "project_id": "proj-kigali",
            "sources": {"survey_geo": {"type": "geojson"}, "design_vt": {"type": "vector"}},
            "styles": {}, "style_editors": [],
            "layers": [
                {"id": "ds-poles", "type": "circle", "source": "survey_geo", "style_ref": "poles", "metadata": {"config_layer_id": "survey/poles", "label": "Poles", "layer_class": "survey", "geometry_type": "Point", "order": 10}},
                {"id": "ds-poles__label", "type": "symbol", "source": "survey_geo", "style_ref": "poles__label", "metadata": {"config_layer_id": "survey/poles", "parent_layer": "ds-poles"}},
                {"id": "ds-lines", "type": "line", "source": "design_vt", "source-layer": "lv", "style_ref": "lines", "minzoom": 12, "metadata": {"config_layer_id": "design/lines", "label": "LV Lines", "layer_class": "design_tile", "geometry_type": "LineString", "order": 20}}
            ]
        })
    }
    fn fixture_upstream(uid: &str) -> Arc<Upstream> {
        Arc::new(Upstream {
            uid: uid.into(),
            project: "proj-kigali".into(),
            document: document(),
            reorders: Mutex::new(vec![]),
            switch_project_on_read: AtomicBool::new(false),
        })
    }
    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// One running Server: a real TCP listener on a free loopback port.
    struct Running {
        address: std::net::SocketAddr,
        handle: tokio::task::JoinHandle<()>,
    }
    async fn start(
        dir: &std::path::Path,
        upstream: Arc<Upstream>,
        allowed: Arc<AtomicBool>,
    ) -> Running {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = App {
            database: dir.join("store.sqlite"),
            connection: Connection {
                address,
                owner: "owner-digest".into(),
                lane: "canary".into(),
                token: TOKEN.into(),
            },
            auth: Arc::new(Auth(allowed)),
            requests: Arc::new(tokio::sync::Semaphore::new(4)),
            activity: None,
            layers: Arc::new(FixtureHost {
                upstream,
                root: dir.join("layers"),
            }),
        };
        let handle = tokio::spawn(async move {
            axum::serve(listener, router(app)).await.unwrap();
        });
        Running { address, handle }
    }
    /// The client half, over the wire like `ds server layers …` does.
    fn call(
        address: std::net::SocketAddr,
        method: &str,
        path: &str,
        body: Option<Value>,
        token: &str,
    ) -> (u16, Value) {
        let url = format!("http://{address}{path}");
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut response = match method {
            "GET" => agent
                .get(&url)
                .header("authorization", &format!("Bearer {token}"))
                .call()
                .unwrap(),
            _ => agent
                .post(&url)
                .header("authorization", &format!("Bearer {token}"))
                .header("content-type", "application/json")
                .send(serde_json::to_vec(&body.unwrap()).unwrap())
                .unwrap(),
        };
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }
    async fn wire(
        address: std::net::SocketAddr,
        method: &'static str,
        path: &'static str,
        body: Option<Value>,
        token: &'static str,
    ) -> (u16, Value) {
        tokio::task::spawn_blocking(move || call(address, method, path, body, token))
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

    #[tokio::test(flavor = "multi_thread")]
    async fn the_workflow_runs_through_a_real_listener_and_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let upstream = fixture_upstream("uid-a");
        let allowed = Arc::new(AtomicBool::new(true));
        let server = start(dir.path(), upstream.clone(), allowed.clone()).await;

        // list: every family reads as the document authored it, zoom reported
        let (status, listing) = wire(server.address, "GET", "/v1/layers?zoom=8", None, TOKEN).await;
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
            server.address,
            "POST",
            "/v1/layers/visibility",
            Some(json!({"layers": ["survey/poles"], "visible": false})),
            TOKEN,
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
        server.handle.abort();
        let server = start(dir.path(), upstream.clone(), allowed.clone()).await;
        let (status, listing) = wire(server.address, "GET", "/v1/layers", None, TOKEN).await;
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
            server.address,
            "POST",
            "/v1/layers/visibility",
            Some(json!({"layers": ["survey/poles"], "visible": true})),
            TOKEN,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(shown["changed"], json!(["ds-poles", "ds-poles__label"]));
        let (_, again) = wire(
            server.address,
            "POST",
            "/v1/layers/visibility",
            Some(json!({"layers": ["survey/poles"], "visible": true})),
            TOKEN,
        )
        .await;
        assert_eq!(again["changed"], json!([]));

        // reorder: admitted, then written upstream; partial says what it leaves unlisted
        let (status, ordered) = wire(
            server.address,
            "POST",
            "/v1/layers/order",
            Some(json!({"orders": [{"layer_id": "survey/poles", "order": 100}]})),
            TOKEN,
        )
        .await;
        assert_eq!(status, 200, "{ordered}");
        assert_eq!(ordered["applied"], true);
        assert_eq!(ordered["complete"], false);
        assert_eq!(ordered["unlisted"], json!(["design/lines"]));
        assert_eq!(upstream.reorders.lock().unwrap().len(), 1);

        // invalid ids: typed refusals, nothing written upstream or locally
        let (status, refused) = wire(
            server.address,
            "POST",
            "/v1/layers/order",
            Some(json!({"orders": [{"layer_id": "ds-poles", "order": 1}]})),
            TOKEN,
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(refused["code"], "unknown_layer");
        assert_eq!(refused["class"], "invalid_input");
        assert_eq!(upstream.reorders.lock().unwrap().len(), 1);
        let (status, refused) = wire(
            server.address,
            "POST",
            "/v1/layers/visibility",
            Some(json!({"layers": ["ds-poles"], "visible": false})),
            TOKEN,
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(refused["code"], "unknown_layer");
        let (status, refused) = wire(
            server.address,
            "POST",
            "/v1/layers/visibility",
            Some(json!({"layer": "x"})),
            TOKEN,
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(refused["code"], "invalid_input");
        let (status, refused) =
            wire(server.address, "GET", "/v1/layers?limit=0", None, TOKEN).await;
        assert_eq!(status, 400);
        assert_eq!(refused["code"], "invalid_number");
        let (status, _) = wire(server.address, "GET", "/v1/layers?foo=1", None, TOKEN).await;
        assert_eq!(status, 400);
        let before = ds_layer_store::visibility::read_at(
            &dir.path().join("layers"),
            "canary",
            "uid-a",
            "proj-kigali",
        )
        .unwrap();

        // unauthorized: a wrong bearer and a revoked device both stop at the door
        let (status, _) = wire(
            server.address,
            "POST",
            "/v1/layers/visibility",
            Some(json!({"layers": ["survey/poles"], "visible": false})),
            "not-the-token",
        )
        .await;
        assert_eq!(status, 401);
        allowed.store(false, Ordering::SeqCst);
        let (status, _) = wire(server.address, "GET", "/v1/layers", None, TOKEN).await;
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
        let (status, refused) = wire(server.address, "GET", "/v1/layers", None, TOKEN).await;
        assert_eq!(status, 409, "{refused}");
        assert_eq!(refused["code"], "project_context_changed");
        upstream
            .switch_project_on_read
            .store(false, Ordering::SeqCst);

        // account change across restart: the new account sees its own defaults,
        // and the previous account's toggles are still on disk under its scope
        server.handle.abort();
        let server = start(dir.path(), fixture_upstream("uid-b"), allowed.clone()).await;
        let (status, hidden_b) = wire(
            server.address,
            "POST",
            "/v1/layers/visibility",
            Some(json!({"layers": ["design/lines"], "visible": false})),
            TOKEN,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(hidden_b["changed"], json!(["ds-lines"]));
        let (_, listing_b) = wire(server.address, "GET", "/v1/layers", None, TOKEN).await;
        assert_eq!(
            row(&listing_b, "survey/poles")["visibility"]["any_visible"],
            true,
            "account b never inherited account a's toggles"
        );
        assert_eq!(
            ds_layer_store::visibility::read_at(
                &dir.path().join("layers"),
                "canary",
                "uid-a",
                "proj-kigali"
            )
            .unwrap()["ds-poles"],
            true
        );
        assert_eq!(
            ds_layer_store::visibility::read_at(
                &dir.path().join("layers"),
                "canary",
                "uid-b",
                "proj-kigali"
            )
            .unwrap()["ds-lines"],
            false
        );
        server.handle.abort();
    }
}
