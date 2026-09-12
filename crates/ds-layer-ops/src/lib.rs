//! The layer drawer's application operations for native hosts — list, show,
//! hide and reorder — as ONE owner that `ds map layer …` (natively or against a Server)
//! and the Server's HTTP host all call. None of them embeds a rule of its own.
//!
//! The decisions are the kernel's (`ds_command_kernel::layer_state`); this
//! crate scopes them to a principal, lane and project, reads and persists the
//! preferences the kernel returns through `ds_layer_store::visibility`, and
//! shapes the one answer every host renders. Hosts supply two things:
//!
//! * a [`LayerDocuments`] port — the project's assembled layer document under
//!   the identity it was read with, and the governed order write. The native
//!   adapter [`Native`] reads through `ds-cli-auth` (the same client `ds` uses
//!   headlessly); tests supply fixtures. A client path, browser cache or paired
//!   map is never a document source here.
//! * a [`Preferences`] root — where this host remembers toggles. Keyed by
//!   lane, DS account uid and project, so preferences never become another
//!   account's visibility merely because both connect to one host.
//!
//! Renderer state is not modelled: the answers carry `writes` (the layout
//! word per runtime layer) for a host that has a renderer, and nothing
//! pretends a layer was mounted.

use ds_cli_contract::outcome::Failure;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub use ds_command_kernel::layer_document::Order;

pub const LOCAL_STORE_REMEDY: &str =
    "check the local data directory; DS_LAYER_HOME may name an absolute shared directory";
pub const ID_REMEDY: &str = "copy ids from `ds map layer list --output json`";
pub const MAX_LIST_LIMIT: i64 = 500;
pub const MAX_ZOOM: i64 = 24;

// ── scope and ports ─────────────────────────────────────────────────────

/// The principal, lane and project one layer question is about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub lane: String,
    /// The DS account uid, never the OS user.
    pub uid: String,
    pub project: String,
}

/// The assembled layer document and the scope it was read under.
pub struct DocumentRead {
    pub scope: Scope,
    pub document: Value,
}

/// The governed order write's receipt.
pub struct OrderReceipt {
    pub project: String,
    pub reordered: usize,
}

/// What a host supplies: the document and the order write, both fenced by
/// the identity and selected project the host is bound to.
pub trait LayerDocuments {
    fn read(&mut self, refresh: bool) -> Result<DocumentRead, Failure>;
    /// Recheck the exact principal/project captured by `read` before a local
    /// preference or governed order effect. Implementations must refuse before
    /// the effect if their native scope changed.
    fn check_scope(&mut self, expected: &Scope) -> Result<(), Failure>;
    fn reorder(&mut self, orders: &[Order]) -> Result<OrderReceipt, Failure>;
}

/// Where this host remembers toggles.
#[derive(Clone, Debug)]
pub struct Preferences {
    root: PathBuf,
}
impl Preferences {
    /// The shared native root (`DS_LAYER_HOME` or the local data directory).
    pub fn native() -> Result<Self, Failure> {
        ds_layer_store::default_root()
            .map(|root| Self { root })
            .map_err(|message| {
                Failure::invalid("local_layer_refused", message).remedy(LOCAL_STORE_REMEDY)
            })
    }
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }
    pub fn read(&self, scope: &Scope) -> Result<BTreeMap<String, bool>, Failure> {
        ds_layer_store::visibility::read_at(&self.root, &scope.lane, &scope.uid, &scope.project)
            .map_err(|message| {
                Failure::invalid("local_layer_refused", message).remedy(LOCAL_STORE_REMEDY)
            })
    }
    pub fn replace(&self, scope: &Scope, next: &BTreeMap<String, bool>) -> Result<Value, Failure> {
        ds_layer_store::visibility::replace_at(
            &self.root,
            &scope.lane,
            &scope.uid,
            &scope.project,
            next,
        )
        .map_err(|message| {
            Failure::invalid("local_layer_refused", message).remedy(LOCAL_STORE_REMEDY)
        })
    }

    pub fn update<T>(
        &self,
        scope: &Scope,
        transition: impl FnOnce(&BTreeMap<String, bool>) -> Result<(BTreeMap<String, bool>, T), Failure>,
    ) -> Result<(Value, T), Failure> {
        ds_layer_store::visibility::update_at(
            &self.root,
            &scope.lane,
            &scope.uid,
            &scope.project,
            transition,
        )
        .map_err(|error| match error {
            ds_layer_store::visibility::UpdateError::Store(message) => {
                Failure::invalid("local_layer_refused", message).remedy(LOCAL_STORE_REMEDY)
            }
            ds_layer_store::visibility::UpdateError::Transition(failure) => failure,
        })
    }
}

// ── the native adapter ──────────────────────────────────────────────────

/// The document as `ds` reads it headlessly, under one lane and one account.
/// The identity is observed before and after the read so an account switch
/// during a request is a typed refusal, never a document applied under the
/// wrong scope.
///
/// The PROJECT comes from one of two places, decided once when the source is
/// built and never re-decided:
///
/// * [`Native::new`] / [`Native::guarded`] — this machine's saved selection.
///   That is the desktop-paired CLI's own subject: `ds map layer list` with no
///   `--project` asks about the project the operator selected.
/// * [`Native::for_project`] / [`Native::guarded_for_project`] — the project
///   the CALLER named. That is every host serving more than one project at a
///   time: the Server's `/v1/layers*`, and `ds map layer … --project <id>`.
///   Nothing here reads the selection, so a second authorized project is
///   served on its own terms and no call rewrites what the operator selected.
type NativeGuard = Box<dyn Fn(&ds_cli_auth::LayerScopeFence) -> Result<(), Failure> + Send>;

pub struct Native {
    lane: String,
    /// `None` = the saved selection is the subject; `Some(id)` = the caller's.
    project: Option<String>,
    fence: Option<ds_cli_auth::LayerScopeFence>,
    guard: Option<NativeGuard>,
}
impl Native {
    pub fn new(lane: &str) -> Self {
        Self {
            lane: lane.to_owned(),
            project: None,
            fence: None,
            guard: None,
        }
    }
    pub fn guarded(lane: &str, guard: NativeGuard) -> Self {
        Self {
            lane: lane.to_owned(),
            project: None,
            fence: None,
            guard: Some(guard),
        }
    }
    /// The named project's document source. The saved selection is not read.
    pub fn for_project(lane: &str, project: &str) -> Self {
        Self {
            lane: lane.to_owned(),
            project: Some(project.to_owned()),
            fence: None,
            guard: None,
        }
    }
    /// The named project's document source under a host's own owner check.
    pub fn guarded_for_project(lane: &str, project: &str, guard: NativeGuard) -> Self {
        Self {
            lane: lane.to_owned(),
            project: Some(project.to_owned()),
            fence: None,
            guard: Some(guard),
        }
    }
    fn authorize(&self, fence: &ds_cli_auth::LayerScopeFence) -> Result<(), Failure> {
        if let Some(guard) = &self.guard {
            guard(fence)?;
        }
        Ok(())
    }
    fn capture(&self) -> Result<ds_cli_auth::LayerScopeFence, Failure> {
        match &self.project {
            Some(project) => {
                ds_cli_auth::capture_layer_scope_fence_for_project(&self.lane, project)
            }
            None => ds_cli_auth::capture_layer_scope_fence(&self.lane),
        }
    }
    fn read_fence(&self) -> Result<&ds_cli_auth::LayerScopeFence, Failure> {
        self.fence.as_ref().ok_or_else(|| {
            Failure::conflict(
                "layer_state_refused",
                "read the layer document before applying a scoped operation",
            )
            .remedy("repeat the layer request")
        })
    }
}
impl LayerDocuments for Native {
    fn read(&mut self, refresh: bool) -> Result<DocumentRead, Failure> {
        let fence = self.capture()?;
        self.authorize(&fence)?;
        let headless = match &self.project {
            Some(project) => {
                ds_cli_auth::layer_config_for_project(&self.lane, project, refresh, &fence)?
            }
            None => ds_cli_auth::layer_config_fenced(&self.lane, refresh, &fence)?,
        };
        self.authorize(&fence)?;
        self.fence = Some(fence);
        Ok(DocumentRead {
            scope: Scope {
                lane: headless.lane().to_owned(),
                uid: self.fence.as_ref().expect("fence stored").uid().to_owned(),
                project: headless.project_id().to_owned(),
            },
            document: headless.result().document().clone(),
        })
    }
    fn check_scope(&mut self, expected: &Scope) -> Result<(), Failure> {
        let fence = self.read_fence()?;
        self.authorize(fence)?;
        match &self.project {
            Some(_) => ds_cli_auth::verify_layer_scope_fence_for_project(
                &self.lane,
                fence,
                &expected.uid,
                &expected.project,
            ),
            None => ds_cli_auth::verify_layer_scope_fence(
                &self.lane,
                fence,
                &expected.uid,
                &expected.project,
            ),
        }
    }
    fn reorder(&mut self, orders: &[Order]) -> Result<OrderReceipt, Failure> {
        let fence = self.read_fence()?;
        self.authorize(fence)?;
        let receipt = match &self.project {
            Some(project) => {
                ds_cli_auth::layer_reorder_for_project(&self.lane, project, orders, fence)?
            }
            None => ds_cli_auth::layer_reorder_fenced(&self.lane, orders, fence)?,
        };
        Ok(OrderReceipt {
            project: receipt.project_id().to_owned(),
            reordered: orders.len(),
        })
    }
}

// ── the kernel's answers ────────────────────────────────────────────────

/// One shared layer-state question. The kernel's own request errors are ours
/// (a malformed question or a stale project), never the backend's.
pub fn ask_layer_state(request: &Value) -> Result<Value, Failure> {
    let bytes = serde_json::to_vec(request).expect("layer state request encodes");
    let answer = ds_command_kernel::layer_state::evaluate(&bytes).map_err(|message| {
        if message.starts_with("project_context_changed") {
            Failure::conflict("project_context_changed", message)
                .remedy("run the command again against the current selected project")
        } else {
            Failure::internal("layer_state_refused", message)
                .remedy("update ds and report the layer-state contract failure")
        }
    })?;
    Ok(serde_json::from_str(&answer).expect("kernel answers JSON"))
}

fn question(read: &DocumentRead, preferences: &BTreeMap<String, bool>, op: Value) -> Value {
    json!({
        "schema": ds_command_kernel::layer_state::SCHEMA,
        "project": read.scope.project,
        "document": read.document,
        "preferences": preferences,
        "op": op,
    })
}

// ── the operations ──────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ListRequest {
    pub refresh: bool,
    pub limit: Option<i64>,
    pub zoom: Option<i64>,
}

/// The canonical catalogue as the drawer sees it: families, roles, this
/// host's remembered visibility folded over the family, source state and,
/// with a zoom, whether each family renders there.
pub fn list(
    documents: &mut dyn LayerDocuments,
    preferences: &Preferences,
    request: &ListRequest,
) -> Result<Value, Failure> {
    let limit = bounded(request.limit.unwrap_or(100), "limit", 1, MAX_LIST_LIMIT)?;
    let zoom = request
        .zoom
        .map(|zoom| bounded(zoom, "zoom", 0, MAX_ZOOM))
        .transpose()?;
    let read = documents.read(request.refresh)?;
    let remembered = preferences.read(&read.scope)?;
    let mut ask = question(
        &read,
        &remembered,
        json!({"kind": "catalog", "limit": limit}),
    );
    if let Some(zoom) = zoom {
        ask["zoom"] = json!(zoom);
    }
    let mut result = ask_layer_state(&ask)?;
    result["project"] = json!(read.scope.project);
    result["lane"] = json!(read.scope.lane);
    result["refreshed"] = json!(request.refresh);
    result["visibility_source"] = json!("native_local");
    Ok(result)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisibilityRequest {
    /// Canonical layer ids from the catalogue; runtime ids are refused.
    pub layers: Vec<String>,
    pub visible: bool,
}

/// Remember canonical layers visible or hidden for this host's principal:
/// the kernel names the runtime layers each family holds and the preferences
/// after the change; the store persists them; the answer is the rows as the
/// drawer now reads them.
pub fn set_visibility(
    documents: &mut dyn LayerDocuments,
    preferences: &Preferences,
    request: &VisibilityRequest,
) -> Result<Value, Failure> {
    let wanted: Vec<String> = request
        .layers
        .iter()
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
        .collect();
    if wanted.is_empty() || wanted.len() > MAX_LIST_LIMIT as usize {
        return Err(Failure::invalid(
            "unknown_layer",
            format!("name 1..={MAX_LIST_LIMIT} canonical layer ids"),
        )
        .remedy(ID_REMEDY));
    }
    let read = documents.read(false)?;
    let remembered = preferences.read(&read.scope)?;
    let identity = ask_layer_state(&question(&read, &remembered, json!({"kind": "classify"})))?;
    let mut runtime_ids = Vec::new();
    let mut unknown = Vec::new();
    for id in &wanted {
        let members: Vec<&str> = identity["layers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| row["canonical_id"].as_str() == Some(id))
            .filter_map(|row| row["id"].as_str())
            .collect();
        if members.is_empty() {
            unknown.push(id.as_str());
        }
        runtime_ids.extend(members.into_iter().map(str::to_owned));
    }
    if !unknown.is_empty() {
        return Err(Failure::invalid(
            "unknown_layer",
            format!(
                "not canonical layers of the selected project: {}",
                unknown.join(", ")
            ),
        )
        .remedy(ID_REMEDY));
    }
    let (receipt, (transition, next)) = preferences.update(&read.scope, |current| {
        documents.check_scope(&read.scope)?;
        let transition = ask_layer_state(&question(
            &read,
            current,
            json!({"kind": "set", "ids": runtime_ids, "visible": request.visible, "expand": true}),
        ))?;
        let next: BTreeMap<String, bool> =
            serde_json::from_value(transition["preferences"].clone())
                .expect("kernel preferences are a boolean map");
        Ok((next.clone(), (transition, next)))
    })?;
    let catalog = ask_layer_state(&question(
        &read,
        &next,
        json!({"kind": "catalog", "limit": MAX_LIST_LIMIT}),
    ))?;
    let rows: Vec<Value> = catalog["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| {
            row["id"]
                .as_str()
                .is_some_and(|id| wanted.iter().any(|w| w == id))
        })
        .cloned()
        .collect();
    Ok(json!({
        "lane": read.scope.lane,
        "project": read.scope.project,
        "visible": request.visible,
        "layers": rows,
        "changed": transition["changed"],
        "writes": transition["writes"],
        "persisted": receipt["persisted"],
        "revision": receipt["revision"],
    }))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderRequest {
    pub orders: Vec<Order>,
}

/// Save canonical order overrides: admitted by the kernel against the current
/// document before anything is sent — unknown ids (runtime ids included),
/// repeated ids and out-of-bound orders are typed refusals; a partial order is
/// accepted and names what it leaves unlisted — then written through the
/// governed route under the same identity.
pub fn reorder(
    documents: &mut dyn LayerDocuments,
    request: &OrderRequest,
) -> Result<Value, Failure> {
    let read = documents.read(false)?;
    // Revalidate native identity/project before admitting any governed write;
    // the receipt is evidence, never the precondition for the effect.
    documents.check_scope(&read.scope)?;
    let admission = ask_layer_state(&json!({
        "schema": ds_command_kernel::layer_state::SCHEMA,
        "project": read.scope.project,
        "document": read.document,
        "op": {"kind": "reorder", "orders": request.orders},
    }))?;
    if admission["outcome"] == "refused" {
        let ids = admission["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let message = admission["message"].as_str().unwrap_or("order refused");
        let message = if ids.is_empty() {
            message.to_owned()
        } else {
            format!("{message}: {ids}")
        };
        let remedy = "copy ids from `ds map layer list --output json`; pass each canonical id once";
        return Err(match admission["code"].as_str() {
            Some("unknown_layer") => Failure::invalid("unknown_layer", message).remedy(remedy),
            Some("duplicate_layer") => Failure::invalid("duplicate_layer", message).remedy(remedy),
            _ => Failure::invalid("invalid_order", message).remedy(remedy),
        });
    }
    let receipt = documents.reorder(&request.orders)?;
    if receipt.project != read.scope.project {
        return Err(Failure::conflict(
            "project_context_changed",
            "the order receipt names another project than the document it was admitted against",
        )
        .remedy("run the command again against the current selected project"));
    }
    Ok(json!({
        "lane": read.scope.lane,
        "project": read.scope.project,
        "orders": request.orders,
        "applied": true,
        "persisted": true,
        "reordered": receipt.reordered,
        "canonical_count": admission["canonical_count"],
        "complete": admission["complete"],
        "unlisted": admission["unlisted"],
    }))
}

fn bounded(value: i64, name: &str, min: i64, max: i64) -> Result<i64, Failure> {
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(Failure::invalid(
            "invalid_number",
            format!("`--{name}` must be a whole number"),
        )
        .remedy(format!("pass {min}..{max}")))
    }
}

// ── rendering, shared by every host that prints ─────────────────────────

/// One word for a folded visibility: `visible`, `partial` or `hidden`.
pub fn visibility_word(visibility: &Value) -> &'static str {
    match (
        visibility["all_visible"].as_bool().unwrap_or(false),
        visibility["any_visible"].as_bool().unwrap_or(false),
    ) {
        (true, _) => "visible",
        (false, true) => "partial",
        (false, false) => "hidden",
    }
}

pub fn render_list(data: &Value) -> String {
    let mut out = format!(
        "project {} · {} canonical layers\n",
        data["project"].as_str().unwrap_or("?"),
        data["layer_count"]
    );
    for layer in data["layers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{:<38} {:>6}  {:<12} {:<8} {}\n",
            layer["id"].as_str().unwrap_or("?"),
            layer["order"],
            layer["geometry"].as_str().unwrap_or("?"),
            visibility_word(&layer["visibility"]),
            layer["label"].as_str().unwrap_or("?")
        ));
    }
    out
}

pub fn render_visibility(data: &Value) -> String {
    let mut out = format!(
        "{} {} canonical layers for {} · saved locally (revision {})\n",
        if data["visible"].as_bool().unwrap_or(false) {
            "showed"
        } else {
            "hid"
        },
        data["layers"].as_array().map_or(0, Vec::len),
        data["project"].as_str().unwrap_or("?"),
        data["revision"],
    );
    for row in data["layers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{:<38} {}\n",
            row["id"].as_str().unwrap_or("?"),
            visibility_word(&row["visibility"]),
        ));
    }
    out
}

pub fn render_reorder(data: &Value) -> String {
    format!(
        "saved {} layer-order overrides for {}{}\n",
        data["orders"].as_array().map_or(0, Vec::len),
        data["project"].as_str().unwrap_or("?"),
        if data["complete"] == false {
            format!(
                " · {} canonical layers left unlisted",
                data["unlisted"].as_array().map_or(0, Vec::len)
            )
        } else {
            String::new()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An in-memory document source: what a fixture upstream would answer.
    pub struct Fixture {
        pub scope: Scope,
        pub document: Value,
        pub reorders: Vec<Vec<Order>>,
        /// The project the order receipt names; defaults to the scope's.
        pub receipt_project: Option<String>,
        /// Test-only context movement after a read, before an effect fence.
        pub scope_after_read: Option<Scope>,
    }
    impl Fixture {
        pub fn new(uid: &str, project: &str) -> Self {
            let mut document = fixture_document();
            document["project_id"] = json!(project);
            Self {
                scope: Scope {
                    lane: "canary".into(),
                    uid: uid.into(),
                    project: project.into(),
                },
                document,
                reorders: vec![],
                receipt_project: None,
                scope_after_read: None,
            }
        }
    }
    impl LayerDocuments for Fixture {
        fn read(&mut self, _refresh: bool) -> Result<DocumentRead, Failure> {
            let read = DocumentRead {
                scope: self.scope.clone(),
                document: self.document.clone(),
            };
            if let Some(next) = self.scope_after_read.take() {
                self.scope = next;
            }
            Ok(read)
        }
        fn check_scope(&mut self, expected: &Scope) -> Result<(), Failure> {
            let actual = self.scope.clone();
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
            self.reorders.push(orders.to_vec());
            Ok(OrderReceipt {
                project: self
                    .receipt_project
                    .clone()
                    .unwrap_or_else(|| self.scope.project.clone()),
                reordered: orders.len(),
            })
        }
    }

    pub fn fixture_document() -> Value {
        let layer = |id: &str, kind: &str, source: &str, metadata: Value| json!({"id": id, "type": kind, "source": source, "style_ref": id, "metadata": metadata});
        json!({
            "project_id": "p1",
            "sources": {"survey_geo": {"type": "geojson"}, "design_vt": {"type": "vector"}},
            "styles": {}, "style_editors": [],
            "layers": [
                layer("ds-poles", "circle", "survey_geo", json!({"config_layer_id": "survey/poles", "label": "Poles", "layer_class": "survey", "geometry_type": "Point", "order": 10})),
                {"id": "ds-poles__label", "type": "symbol", "source": "survey_geo", "style_ref": "ds-poles__label", "layout": {"visibility": "none"}, "metadata": {"config_layer_id": "survey/poles", "parent_layer": "ds-poles"}},
                layer("ds-lines", "line", "design_vt", json!({"config_layer_id": "design/lines", "label": "LV Lines", "layer_class": "design_tile", "geometry_type": "LineString", "order": 20})).tap(|v| { v["minzoom"] = json!(12); }),
            ]
        })
    }
    trait Tap {
        fn tap(self, f: impl FnOnce(&mut Value)) -> Value;
    }
    impl Tap for Value {
        fn tap(mut self, f: impl FnOnce(&mut Value)) -> Value {
            f(&mut self);
            self
        }
    }
    fn orders(pairs: &[(&str, i64)]) -> Vec<Order> {
        pairs
            .iter()
            .map(|(id, order)| Order {
                layer_id: (*id).to_owned(),
                order: *order,
            })
            .collect()
    }

    #[test]
    fn concurrent_layer_toggles_preserve_both_families() {
        struct Concurrent(Fixture, std::sync::Arc<std::sync::Barrier>);
        impl LayerDocuments for Concurrent {
            fn read(&mut self, refresh: bool) -> Result<DocumentRead, Failure> {
                let read = self.0.read(refresh)?;
                self.1.wait();
                Ok(read)
            }
            fn check_scope(&mut self, expected: &Scope) -> Result<(), Failure> {
                self.0.check_scope(expected)
            }
            fn reorder(&mut self, orders: &[Order]) -> Result<OrderReceipt, Failure> {
                self.0.reorder(orders)
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let preferences = Preferences::at(tmp.path().to_owned());
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|threads| {
            for layer in ["survey/poles", "design/lines"] {
                let barrier = barrier.clone();
                let preferences = &preferences;
                threads.spawn(move || {
                    let mut docs = Concurrent(Fixture::new("u1", "p1"), barrier);
                    set_visibility(
                        &mut docs,
                        preferences,
                        &VisibilityRequest {
                            layers: vec![layer.into()],
                            visible: false,
                        },
                    )
                    .unwrap();
                });
            }
        });
        let stored = preferences
            .read(&Scope {
                lane: "canary".into(),
                uid: "u1".into(),
                project: "p1".into(),
            })
            .unwrap();
        assert_eq!(stored.get("ds-poles"), Some(&false));
        assert_eq!(stored.get("ds-poles__label"), Some(&false));
        assert_eq!(stored.get("ds-lines"), Some(&false));
    }

    #[test]
    fn list_hide_show_read_the_same_scoped_preferences() {
        let tmp = tempfile::tempdir().unwrap();
        let preferences = Preferences::at(tmp.path().to_owned());
        let mut docs = Fixture::new("u1", "p1");
        let listed = list(
            &mut docs,
            &preferences,
            &ListRequest {
                zoom: Some(8),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(listed["layer_count"], 2);
        assert_eq!(listed["visibility_source"], "native_local");
        assert_eq!(listed["layers"][0]["visibility"]["any_visible"], true);
        assert_eq!(
            listed["layers"][0]["visibility"]["all_visible"], false,
            "the authored-hidden label keeps the family partial"
        );
        assert_eq!(
            listed["layers"][1]["in_zoom_range"], false,
            "the lines start at zoom 12"
        );
        let hidden = set_visibility(
            &mut docs,
            &preferences,
            &VisibilityRequest {
                layers: vec!["survey/poles".into()],
                visible: false,
            },
        )
        .unwrap();
        assert_eq!(hidden["layers"][0]["visibility"]["any_visible"], false);
        assert_eq!(hidden["changed"], json!(["ds-poles"]));
        assert_eq!(hidden["persisted"], "native_local");
        let listed = list(&mut docs, &preferences, &ListRequest::default()).unwrap();
        assert_eq!(
            listed["layers"][0]["visibility"]["any_visible"], false,
            "the store remembered it"
        );
        assert_eq!(listed["layers"][1]["visibility"]["all_visible"], true);
        // another account on the same host sees its own defaults
        let mut other = Fixture::new("u2", "p1");
        let listed = list(&mut other, &preferences, &ListRequest::default()).unwrap();
        assert_eq!(
            listed["layers"][0]["visibility"]["any_visible"], true,
            "account u1's hide never reached account u2"
        );
        // and showing is idempotent: nothing changes the second time
        set_visibility(
            &mut docs,
            &preferences,
            &VisibilityRequest {
                layers: vec!["survey/poles".into()],
                visible: true,
            },
        )
        .unwrap();
        let again = set_visibility(
            &mut docs,
            &preferences,
            &VisibilityRequest {
                layers: vec!["survey/poles".into()],
                visible: true,
            },
        )
        .unwrap();
        assert_eq!(again["changed"], json!([]));
        assert!(
            ds_layer_store::visibility::read_at(tmp.path(), "canary", "u1", "p1").unwrap()["ds-poles"]
        );
    }

    #[test]
    fn unknown_runtime_and_empty_ids_are_typed_refusals_and_write_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let preferences = Preferences::at(tmp.path().to_owned());
        let mut docs = Fixture::new("u1", "p1");
        let refused = set_visibility(
            &mut docs,
            &preferences,
            &VisibilityRequest {
                layers: vec!["ds-poles".into()],
                visible: false,
            },
        )
        .unwrap_err();
        assert_eq!(refused.code(), "unknown_layer");
        assert!(refused.message().contains("ds-poles"));
        assert_eq!(
            set_visibility(
                &mut docs,
                &preferences,
                &VisibilityRequest {
                    layers: vec!["  ".into()],
                    visible: false
                }
            )
            .unwrap_err()
            .code(),
            "unknown_layer"
        );
        assert!(!tmp.path().join("layer-visibility.json").exists());
        assert_eq!(
            list(
                &mut docs,
                &preferences,
                &ListRequest {
                    limit: Some(0),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code(),
            "invalid_number"
        );
        assert_eq!(
            list(
                &mut docs,
                &preferences,
                &ListRequest {
                    zoom: Some(25),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code(),
            "invalid_number"
        );
    }

    #[test]
    fn reorder_is_admitted_before_the_write_and_reports_what_it_leaves_unlisted() {
        let mut docs = Fixture::new("u1", "p1");
        let saved = reorder(
            &mut docs,
            &OrderRequest {
                orders: orders(&[("survey/poles", 100)]),
            },
        )
        .unwrap();
        assert_eq!(saved["applied"], true);
        assert_eq!(saved["complete"], false);
        assert_eq!(saved["unlisted"], json!(["design/lines"]));
        assert_eq!(saved["reordered"], 1);
        assert_eq!(docs.reorders.len(), 1);
        docs.scope_after_read = Some(Scope {
            lane: "canary".into(),
            uid: "u1".into(),
            project: "p2".into(),
        });
        let refused = reorder(
            &mut docs,
            &OrderRequest {
                orders: orders(&[("survey/poles", 101)]),
            },
        )
        .unwrap_err();
        assert_eq!(refused.code(), "project_context_changed");
        assert_eq!(docs.reorders.len(), 1, "scope change must send no reorder");
        docs.scope.project = "p1".into();
        let refused = reorder(
            &mut docs,
            &OrderRequest {
                orders: orders(&[("ds-poles", 1)]),
            },
        )
        .unwrap_err();
        assert_eq!(refused.code(), "unknown_layer");
        let refused = reorder(
            &mut docs,
            &OrderRequest {
                orders: orders(&[("survey/poles", 1), ("survey/poles", 2)]),
            },
        )
        .unwrap_err();
        assert_eq!(refused.code(), "duplicate_layer");
        let refused = reorder(
            &mut docs,
            &OrderRequest {
                orders: orders(&[("survey/poles", 1_000_001)]),
            },
        )
        .unwrap_err();
        assert_eq!(refused.code(), "invalid_order");
        assert_eq!(docs.reorders.len(), 1, "a refused order is never sent");
        docs.receipt_project = Some("p2".into());
        let refused = reorder(
            &mut docs,
            &OrderRequest {
                orders: orders(&[("survey/poles", 5)]),
            },
        )
        .unwrap_err();
        assert_eq!(refused.code(), "project_context_changed");
    }

    #[test]
    fn a_document_for_another_project_than_the_scope_is_fenced() {
        let tmp = tempfile::tempdir().unwrap();
        let preferences = Preferences::at(tmp.path().to_owned());
        let mut docs = Fixture::new("u1", "p1");
        docs.document["project_id"] = json!("p2");
        let refused = list(&mut docs, &preferences, &ListRequest::default()).unwrap_err();
        assert_eq!(refused.code(), "project_context_changed");
    }

    #[test]
    fn renderers_read_the_shared_answers() {
        let text = render_list(
            &json!({"project": "p1", "layer_count": 1, "layers": [{"id": "survey/poles", "order": 10, "geometry": "Point", "label": "Poles", "visibility": {"count": 2, "any_visible": true, "all_visible": false}}]}),
        );
        assert!(text.contains("partial") && text.contains("1 canonical layers"));
        let text = render_visibility(
            &json!({"visible": false, "project": "p1", "revision": 3, "layers": [{"id": "survey/poles", "visibility": {"any_visible": false, "all_visible": false}}]}),
        );
        assert!(text.starts_with("hid 1 canonical layers for p1") && text.contains("hidden"));
        assert!(
            render_reorder(
                &json!({"orders": [{}], "project": "p1", "complete": false, "unlisted": ["a", "b"]})
            )
            .contains("2 canonical layers left unlisted")
        );
    }
}
