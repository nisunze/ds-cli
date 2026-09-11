//! The layer document's wire contract: what a client asks ds-brain for, and
//! what it accepts back.
//!
//! One backend action family serves the layer drawer — `get_config` and
//! `refresh` (the assembled layer document), `get_style_catalog` (the Style
//! Center's metadata-only snapshot) and `reorder` (canonical order overrides).
//! The request body for each, the response bound, and every rule an answer
//! must satisfy before a client may treat it as the project's layer document
//! are decided here, once, for `ds`, the desktop shell and the browser.
//!
//! Hosts perform the transfer through their declared operation
//! (`domains.layers.action` in the gateway registry) and hand the parsed
//! response back. They never build the body, choose the route or read the
//! wire themselves. An answer that fails admission is a typed refusal with
//! the issues named, bounded, so a backend contract slip is reported and never
//! rendered as a half-valid map.
//!
//! `ds-client-core::layers` delegates to these functions for the native
//! clients; the browser reaches them through the `layerDocument` WASM export.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub use crate::layer_state::Order;

pub const SCHEMA: &str = "ds.layer-document/v1";
/// The gateway registry id every host resolves the route from.
pub const OPERATION: &str = "domains.layers.action";
pub const METHOD: &str = "POST";
pub const PATH: &str = "/api/v1/layers";
pub const ACTIONS: [&str; 4] = ["get_config", "get_style_catalog", "refresh", "reorder"];
pub const DOCUMENT_RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
pub const STYLE_CATALOG_RESPONSE_LIMIT: usize = 16 * 1024 * 1024;
pub const TIMEOUT_SECONDS: u64 = 120;
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_LAYERS: usize = 50_000;
pub const MAX_LAYER_ID_CHARS: usize = 2_000;
pub const MAX_ORDERS: usize = 500;
pub const MAX_ORDER: i64 = 1_000_000;
pub const MAX_STYLES: usize = 1_000;
pub const MAX_STYLE_REF_CHARS: usize = 500;
pub const MAX_WARNINGS: usize = 1_000;
pub const MAX_WARNING_CHARS: usize = 4_096;
/// Issues named in a contract refusal before the count of the rest.
pub const ISSUE_SAMPLE: usize = 6;

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    GetConfig,
    Refresh,
    GetStyleCatalog,
    Reorder,
}

impl Action {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::GetConfig => "get_config",
            Self::Refresh => "refresh",
            Self::GetStyleCatalog => "get_style_catalog",
            Self::Reorder => "reorder",
        }
    }
    pub const fn response_limit(self) -> usize {
        match self {
            Self::GetStyleCatalog => STYLE_CATALOG_RESPONSE_LIMIT,
            _ => DOCUMENT_RESPONSE_LIMIT,
        }
    }
}

/// The order request's own bounds: 1..=500 rows, trimmed bounded ids without
/// control characters, orders within ±1 000 000, each layer once.
pub fn validate_orders(orders: &[Order]) -> Result<(), String> {
    let mut seen = HashSet::new();
    if orders.is_empty()
        || orders.len() > MAX_ORDERS
        || orders.iter().any(|row| {
            row.layer_id.is_empty()
                || row.layer_id.trim() != row.layer_id
                || row.layer_id.chars().count() > MAX_STYLE_REF_CHARS
                || row.layer_id.chars().any(char::is_control)
                || !(-MAX_ORDER..=MAX_ORDER).contains(&row.order)
                || !seen.insert(row.layer_id.as_str())
        })
    {
        return Err("invalid canonical layer order".into());
    }
    Ok(())
}

/// The request body a host sends for `action`. `platform` names the client
/// kind the assembler tailors for; every current client says `web`.
pub fn request_body(
    action: Action,
    project: &str,
    orders: &[Order],
    platform: &str,
) -> Result<Value, String> {
    if project.is_empty() || project.trim() != project || project.chars().count() > 500 {
        return Err("a layer request needs a bounded project id".into());
    }
    Ok(match action {
        Action::GetConfig | Action::Refresh => {
            json!({"action": action.wire(), "project_id": project, "platform": platform})
        }
        Action::GetStyleCatalog => json!({"action": action.wire(), "project_id": project}),
        Action::Reorder => {
            validate_orders(orders)?;
            json!({"action": action.wire(), "project_id": project, "layer_order": orders})
        }
    })
}

/// Why an answer was refused. `backend_refused` carries the backend's own
/// words; `contract_violation` names what the document got wrong.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<String>,
}

fn violation(issues: Vec<String>) -> Refusal {
    let sample = issues
        .iter()
        .take(ISSUE_SAMPLE)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    let suffix = if issues.len() > ISSUE_SAMPLE {
        format!("; +{} more", issues.len() - ISSUE_SAMPLE)
    } else {
        String::new()
    };
    Refusal {
        code: "contract_violation",
        message: format!("Layer config contract violation: {sample}{suffix}"),
        issues,
    }
}

fn text(value: &Value) -> Option<&str> {
    value.as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// The `{success, data}` envelope every layer action answers with. A failure
/// envelope is the backend's refusal, reported in its words with its status.
fn envelope(response: &Value) -> Result<&Map<String, Value>, Refusal> {
    if response["success"] == true {
        if let Some(data) = response["data"].as_object() {
            return Ok(data);
        }
    }
    let message = text(&response["error"])
        .or_else(|| text(&response["message"]))
        .unwrap_or("Failed to load layers");
    let status = response["status"].as_i64();
    let code = text(&response["error_code"]).unwrap_or("");
    let message = match (status, code.is_empty()) {
        (Some(status), false) => format!("{message} ({status} {code})"),
        (Some(status), true) => format!("{message} ({status})"),
        (None, false) => format!("{message} ({code})"),
        (None, true) => message.to_owned(),
    };
    Err(Refusal {
        code: if response["success"] == true {
            "unreadable_response"
        } else {
            "backend_refused"
        },
        message,
        issues: vec![],
    })
}

fn duplicate_issues(issues: &mut Vec<String>, label: &str, values: &[&str]) {
    let mut seen = HashSet::new();
    let mut duplicates = Vec::new();
    for value in values {
        if value.is_empty() {
            continue;
        }
        if !seen.insert(*value) && !duplicates.contains(value) {
            duplicates.push(*value);
        }
    }
    if !duplicates.is_empty() {
        issues.push(format!(
            "{label} contains duplicate ids: {}",
            duplicates
                .iter()
                .take(5)
                .copied()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

fn unknown_references(issues: &mut Vec<String>, label: &str, ids: &Value, known: &HashSet<&str>) {
    for id in ids.as_array().into_iter().flatten() {
        if let Some(id) = text(id) {
            if !known.contains(id) {
                issues.push(format!("{label} references missing layer {id}"));
            }
        }
    }
}

/// Admit an assembled layer document for `project`: the envelope, the
/// document's shape and bounds, and every cross-reference the drawer relies on
/// (sources, parents, matrix rows, sidebar groups, dedicated sections).
pub fn admit_document(project: &str, response: &Value) -> Result<Value, Refusal> {
    let data = envelope(response)?;
    let unreadable = || Refusal {
        code: "unreadable_response",
        message: "invalid governed layer response".into(),
        issues: vec![],
    };
    if data.get("project_id").and_then(Value::as_str) != Some(project)
        || !data.get("sources").is_some_and(Value::is_object)
        || !data.get("styles").is_some_and(Value::is_object)
        || !data.get("style_editors").is_some_and(Value::is_array)
    {
        return Err(unreadable());
    }
    let layers = data
        .get("layers")
        .and_then(Value::as_array)
        .ok_or_else(unreadable)?;
    if layers.len() > MAX_LAYERS {
        return Err(unreadable());
    }
    let sources: HashSet<&str> = data["sources"]
        .as_object()
        .expect("checked")
        .keys()
        .map(String::as_str)
        .collect();
    let mut issues = Vec::new();
    let mut ids = Vec::with_capacity(layers.len());
    for layer in layers {
        let id = layer["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.chars().count() <= MAX_LAYER_ID_CHARS)
            .ok_or_else(unreadable)?;
        if !layer["type"].is_string() {
            return Err(unreadable());
        }
        ids.push(id.trim());
    }
    let known: HashSet<&str> = ids.iter().copied().collect();
    duplicate_issues(&mut issues, "layers", &ids);
    for layer in layers {
        let id = layer["id"].as_str().unwrap_or_default().trim();
        if let Some(source) = text(&layer["source"]) {
            if !sources.contains(source) {
                issues.push(format!("layer {id} references missing source {source}"));
            }
        }
        if let Some(parent) = text(&layer["metadata"]["parent_layer"]) {
            if !known.contains(parent) {
                issues.push(format!(
                    "layer {id} references missing parent_layer {parent}"
                ));
            }
        }
    }
    for (domain, matrix) in data
        .get("matrix")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        for row in matrix["rows"].as_array().into_iter().flatten() {
            let key = row["key"].as_str().unwrap_or_default();
            unknown_references(
                &mut issues,
                &format!("matrix.{domain}.{key}"),
                &row["ids"],
                &known,
            );
        }
    }
    for group in data["sidebar"]["groups"].as_array().into_iter().flatten() {
        let key = group["key"].as_str().unwrap_or_default();
        unknown_references(
            &mut issues,
            &format!("sidebar group {key}"),
            &group["layers"],
            &known,
        );
    }
    for (section, member) in [
        ("survey_layers", "layer_ids"),
        ("design_layers", "layer_ids"),
    ] {
        for layer in data[section].as_array().into_iter().flatten() {
            let key = layer["key"].as_str().unwrap_or_default();
            unknown_references(
                &mut issues,
                &format!("{section}.{key}"),
                &layer[member],
                &known,
            );
        }
    }
    for grouping in ["by_layer_type", "by_source"] {
        for group in data["sidebar_groupings"][grouping]
            .as_array()
            .into_iter()
            .flatten()
        {
            let key = group["key"].as_str().unwrap_or_default();
            unknown_references(
                &mut issues,
                &format!("sidebar_groupings.{grouping}.{key}"),
                &group["layers"],
                &known,
            );
        }
    }
    if !issues.is_empty() {
        return Err(violation(issues));
    }
    Ok(Value::Object(data.clone()))
}

/// Admit the Style Center's metadata-only snapshot: no renderer members, the
/// authoring catalogues present, styles and editors bounded, warnings text.
pub fn admit_style_catalog(project: &str, response: &Value) -> Result<Value, Refusal> {
    let data = envelope(response)?;
    let unreadable = || Refusal {
        code: "unreadable_response",
        message:
            "The Style Center returned an invalid metadata catalogue for the requested project."
                .into(),
        issues: vec![],
    };
    if data.get("project_id").and_then(Value::as_str) != Some(project)
        || data.get("resolved_at").and_then(Value::as_i64).is_none()
        || ["sources", "layers", "sidebar", "viewport", "basemaps"]
            .iter()
            .any(|key| data.contains_key(*key))
        || [
            "style_properties",
            "style_presets",
            "label_editor",
            "fill_pattern",
            "platform_config",
            "symbol_catalog",
            "design_layer_catalog",
        ]
        .iter()
        .any(|key| !data.get(*key).is_some_and(Value::is_object))
        || !data.get("required_images").is_some_and(Value::is_array)
    {
        return Err(unreadable());
    }
    let styles = data
        .get("styles")
        .and_then(Value::as_object)
        .ok_or_else(unreadable)?;
    if styles.len() > MAX_STYLES
        || styles.iter().any(|(reference, style)| {
            reference.is_empty()
                || reference.len() > MAX_STYLE_REF_CHARS
                || reference.trim() != reference
                || !style.is_object()
        })
    {
        return Err(unreadable());
    }
    let editors = data
        .get("style_editors")
        .and_then(Value::as_array)
        .ok_or_else(unreadable)?;
    if editors.len() > MAX_STYLES || editors.iter().any(|editor| !editor.is_object()) {
        return Err(unreadable());
    }
    let warnings = data
        .get("warnings")
        .and_then(Value::as_array)
        .ok_or_else(unreadable)?;
    if warnings.len() > MAX_WARNINGS
        || warnings.iter().any(|warning| {
            !warning
                .as_str()
                .is_some_and(|w| w.len() <= MAX_WARNING_CHARS)
        })
    {
        return Err(unreadable());
    }
    Ok(Value::Object(data.clone()))
}

/// Admit a reorder receipt: the same project, and exactly the rows sent.
pub fn admit_order_receipt(
    project: &str,
    count: usize,
    response: &Value,
) -> Result<Value, Refusal> {
    let data = envelope(response)?;
    if data.get("project_id").and_then(Value::as_str) != Some(project)
        || data.get("reordered").and_then(Value::as_u64) != Some(count as u64)
    {
        return Err(Refusal {
            code: "unreadable_response",
            message: "invalid governed layer response".into(),
            issues: vec![],
        });
    }
    Ok(json!({"project_id": project, "reordered": count}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    op: Op,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Op {
    /// The declared operation, the body to send and the bounds to read under.
    Request {
        action: Action,
        project: String,
        #[serde(default)]
        orders: Vec<Order>,
        #[serde(default = "web")]
        platform: String,
    },
    /// Whether a parsed answer is the project's layer document, style
    /// catalogue or order receipt, or why it is not.
    Admit {
        action: Action,
        project: String,
        response: Value,
        #[serde(default)]
        count: usize,
    },
}

fn web() -> String {
    "web".into()
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("layer document request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid layer document request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!(
            "unsupported layer document schema; expected {SCHEMA}"
        ));
    }
    let value = match request.op {
        Op::Request {
            action,
            project,
            orders,
            platform,
        } => json!({
            "operation": OPERATION,
            "method": METHOD,
            "path": PATH,
            "action": action.wire(),
            "body": request_body(action, &project, &orders, &platform)?,
            "response_limit": action.response_limit(),
            "timeout_seconds": TIMEOUT_SECONDS,
            "count": orders.len(),
        }),
        Op::Admit {
            action,
            project,
            response,
            count,
        } => {
            let admitted = match action {
                Action::GetConfig | Action::Refresh => admit_document(&project, &response),
                Action::GetStyleCatalog => admit_style_catalog(&project, &response),
                Action::Reorder => admit_order_receipt(&project, count, &response),
            };
            match admitted {
                Ok(data) => json!({"outcome": "admitted", "data": data}),
                Err(refusal) => json!({"outcome": "refused", "refusal": refusal}),
            }
        }
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> Value {
        json!({
            "project_id": "p1",
            "resolved_at": 1,
            "sources": {"s": {"type": "geojson"}},
            "styles": {"a": {}},
            "style_editors": [],
            "layers": [
                {"id": "a", "type": "circle", "source": "s"},
                {"id": "a__label", "type": "symbol", "source": "s", "metadata": {"parent_layer": "a"}}
            ],
            "matrix": {"survey": {"rows": [{"key": "r", "ids": ["a", null]}]}},
            "sidebar": {"groups": [{"key": "g", "layers": ["a"]}]},
            "survey_layers": [{"key": "k", "layer_ids": ["a", "a__label"]}],
            "design_layers": [],
            "sidebar_groupings": {"by_layer_type": [{"key": "t", "layers": ["a"]}], "by_source": []}
        })
    }
    fn wrap(data: Value) -> Value {
        json!({"success": true, "data": data})
    }

    #[test]
    fn requests_carry_the_declared_operation_and_the_closed_bodies() {
        let out: Value = serde_json::from_str(
            &evaluate(&serde_json::to_vec(&json!({"schema": SCHEMA, "op": {"kind": "request", "action": "refresh", "project": "p1"}})).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(out["operation"], OPERATION);
        assert_eq!(out["path"], PATH);
        assert_eq!(
            out["body"],
            json!({"action": "refresh", "project_id": "p1", "platform": "web"})
        );
        assert_eq!(out["response_limit"], DOCUMENT_RESPONSE_LIMIT);
        let out: Value = serde_json::from_str(
            &evaluate(&serde_json::to_vec(&json!({"schema": SCHEMA, "op": {"kind": "request", "action": "get_style_catalog", "project": "p1"}})).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            out["body"],
            json!({"action": "get_style_catalog", "project_id": "p1"})
        );
        assert_eq!(out["response_limit"], STYLE_CATALOG_RESPONSE_LIMIT);
        let out: Value = serde_json::from_str(
            &evaluate(&serde_json::to_vec(&json!({"schema": SCHEMA, "op": {"kind": "request", "action": "reorder", "project": "p1", "orders": [{"layer_id": "survey/a", "order": 3}]}})).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            out["body"]["layer_order"],
            json!([{"layer_id": "survey/a", "order": 3}])
        );
        assert_eq!(out["count"], 1);
        assert!(request_body(Action::Reorder, "p1", &[], "web").is_err());
        assert!(request_body(Action::GetConfig, " p1", &[], "web").is_err());
        let duplicate = [
            Order {
                layer_id: "x".into(),
                order: 1,
            },
            Order {
                layer_id: "x".into(),
                order: 2,
            },
        ];
        assert!(validate_orders(&duplicate).is_err());
        assert!(validate_orders(&[Order {
            layer_id: "x".into(),
            order: MAX_ORDER + 1
        }])
        .is_err());
    }

    #[test]
    fn a_coherent_document_is_admitted_and_returned_unreshaped() {
        let admitted = admit_document("p1", &wrap(document())).unwrap();
        assert_eq!(admitted, document());
    }

    #[test]
    fn contract_slips_are_named_and_bounded() {
        let mut broken = document();
        broken["layers"].as_array_mut().unwrap().push(json!({"id": "b", "type": "line", "source": "gone", "metadata": {"parent_layer": "nope"}}));
        broken["matrix"]["survey"]["rows"][0]["ids"] = json!(["a", "ghost"]);
        broken["sidebar"]["groups"][0]["layers"] = json!(["missing"]);
        broken["survey_layers"][0]["layer_ids"] = json!(["a", "stale-runtime-id"]);
        broken["sidebar_groupings"]["by_layer_type"][0]["layers"] = json!(["gone-too"]);
        broken["sidebar_groupings"]["by_source"] = json!([{"key": "src", "layers": ["also-gone"]}]);
        let refusal = admit_document("p1", &wrap(broken)).unwrap_err();
        assert_eq!(refusal.code, "contract_violation");
        assert_eq!(refusal.issues.len(), 7);
        assert!(refusal.message.starts_with("Layer config contract violation: layer b references missing source gone; layer b references missing parent_layer nope; matrix.survey.r references missing layer ghost"));
        assert!(
            refusal.message.ends_with("; +1 more"),
            "{}",
            refusal.message
        );
        let mut duplicated = document();
        duplicated["layers"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "a", "type": "circle", "source": "s"}));
        assert_eq!(
            admit_document("p1", &wrap(duplicated)).unwrap_err().issues,
            vec!["layers contains duplicate ids: a"]
        );
    }

    #[test]
    fn unreadable_and_refused_answers_are_typed() {
        assert_eq!(
            admit_document("p2", &wrap(document())).unwrap_err().code,
            "unreadable_response"
        );
        let mut no_type = document();
        no_type["layers"][0]["type"] = json!(7);
        assert_eq!(
            admit_document("p1", &wrap(no_type)).unwrap_err().code,
            "unreadable_response"
        );
        let refused = admit_document(
            "p1",
            &json!({"success": false, "error": "denied", "status": 403, "error_code": "forbidden"}),
        )
        .unwrap_err();
        assert_eq!(refused.code, "backend_refused");
        assert_eq!(refused.message, "denied (403 forbidden)");
        let refused = admit_document("p1", &json!({"message": "slow"})).unwrap_err();
        assert_eq!(refused.message, "slow");
        assert_eq!(
            admit_document("p1", &json!({"success": true, "data": []}))
                .unwrap_err()
                .code,
            "unreadable_response"
        );
        let out: Value = serde_json::from_str(
            &evaluate(&serde_json::to_vec(&json!({"schema": SCHEMA, "op": {"kind": "admit", "action": "get_config", "project": "p1", "response": {"success": false, "error": "denied"}}})).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(out["outcome"], "refused");
        assert_eq!(out["refusal"]["code"], "backend_refused");
    }

    #[test]
    fn style_catalogue_and_order_receipts_are_admitted_on_their_own_terms() {
        let catalog = json!({
            "project_id": "p1", "resolved_at": 5, "styles": {"a": {}}, "style_editors": [{}], "warnings": ["w"],
            "style_properties": {}, "style_presets": {}, "label_editor": {}, "fill_pattern": {}, "platform_config": {},
            "symbol_catalog": {}, "design_layer_catalog": {}, "required_images": []
        });
        assert_eq!(
            admit_style_catalog("p1", &wrap(catalog.clone())).unwrap(),
            catalog
        );
        let mut leaking = catalog.clone();
        leaking["layers"] = json!([]);
        assert_eq!(
            admit_style_catalog("p1", &wrap(leaking)).unwrap_err().code,
            "unreadable_response"
        );
        let mut untyped = catalog.clone();
        untyped["warnings"] = json!([1]);
        assert!(admit_style_catalog("p1", &wrap(untyped)).is_err());
        assert_eq!(
            admit_order_receipt("p1", 2, &wrap(json!({"project_id": "p1", "reordered": 2})))
                .unwrap(),
            json!({"project_id": "p1", "reordered": 2})
        );
        assert!(
            admit_order_receipt("p1", 2, &wrap(json!({"project_id": "p1", "reordered": 1})))
                .is_err()
        );
        assert!(
            admit_order_receipt("p1", 2, &wrap(json!({"project_id": "px", "reordered": 2})))
                .is_err()
        );
    }

    #[test]
    fn malformed_requests_are_refused() {
        assert!(evaluate(b"{").is_err());
        assert!(evaluate(&serde_json::to_vec(&json!({"schema": "x", "op": {"kind": "request", "action": "refresh", "project": "p1"}})).unwrap()).is_err());
        assert!(evaluate(&serde_json::to_vec(&json!({"schema": SCHEMA, "op": {"kind": "request", "action": "sideways", "project": "p1"}})).unwrap()).is_err());
        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
    }
}
