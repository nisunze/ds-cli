//! The style document's wire contract: what a client asks ds-brain for in the
//! style family, and what it accepts back.
//!
//! Four backend actions serve styling — `update_style` (publish one authored
//! document), `seed_style` (a deterministic starter model that is never
//! persisted), `get_user_data_style` (one shared-class document) and
//! `assemble_work_map_runtime_style` (one work-map runtime family). The
//! payload a publish carries, the request body for each action, the response
//! bound, and every rule an answer must satisfy before a client may treat it
//! as the published style are decided here, once, for `ds`, the desktop shell
//! and the browser.
//!
//! Hosts perform the transfer through the declared operation
//! (`domains.styles.update` for a publish, `domains.styles.action` for the
//! read family) and hand the parsed response back. They never build the body,
//! choose the route or read the wire themselves. An answer that fails
//! admission is a typed refusal carrying the backend's own words and the
//! issues it named, bounded — so a contract slip is reported, never rendered
//! as a half-published style.
//!
//! `ds-client-core::styles` delegates to these functions for the native
//! clients; the browser reaches them through the `styleDocument` WASM export.
//! The sibling `layer_document` owns the drawer's document; this module owns
//! the style family's wire and reuses its envelope reading.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::layer_document::text;
pub use crate::layer_document::Refusal;
use crate::style_authoring::MAX_STYLE_REQUEST_BYTES;

pub const SCHEMA: &str = "ds.style-document/v1";
/// The gateway registry ids every host resolves the routes from.
pub const UPDATE_OPERATION: &str = "domains.styles.update";
pub const ACTION_OPERATION: &str = "domains.styles.action";
pub const METHOD: &str = "POST";
pub const PATH: &str = "/api/v1/styles";
pub const UPDATE_RESPONSE_LIMIT: usize = 1024 * 1024;
pub const ACTION_RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
pub const TIMEOUT_SECONDS: u64 = 30;
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_STYLE_REF_CHARS: usize = 500;
pub const MAX_LOGICAL_ROOT_CHARS: usize = 200;
pub const MAX_GEOMETRY_CHARS: usize = 40;
pub const MAX_SEED_CHARS: usize = 200;
pub const MAX_WARNINGS: usize = 1_000;
pub const MAX_WARNING_CHARS: usize = 4_096;
/// Issues carried out of one backend refusal.
pub const MAX_ISSUES: usize = 50;
pub const MAX_RUNTIME_LAYERS: usize = 100;
pub const MAX_REQUIRED_IMAGES: usize = 500;
/// The assembler that owns every runtime identity in a work-map family.
pub const RUNTIME_FAMILY_AUTHORITY: &str = "ds-brain/work-map-runtime-family/v1";
/// What a style backend that refused without words is reported as.
const FALLBACK: &str = "The style service refused the request";

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Publish one authored document against its declared write target.
    Publish,
    /// A deterministic starter model for a ref with no document yet. Nothing
    /// is persisted: the first publish is the first persistence event.
    Seed,
    /// One shared-class (`ud/`, `pd/`, `gt/`) document, or its absence.
    ReadShared,
    /// One work-map runtime family assembled from an authored document.
    AssembleRuntime,
}

impl Action {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Publish => "update_style",
            Self::Seed => "seed_style",
            Self::ReadShared => "get_user_data_style",
            Self::AssembleRuntime => "assemble_work_map_runtime_style",
        }
    }
    pub const fn operation(self) -> &'static str {
        match self {
            Self::Publish => UPDATE_OPERATION,
            _ => ACTION_OPERATION,
        }
    }
    pub const fn response_limit(self) -> usize {
        match self {
            Self::Publish => UPDATE_RESPONSE_LIMIT,
            _ => ACTION_RESPONSE_LIMIT,
        }
    }
}

fn unreadable(message: impl Into<String>) -> Refusal {
    Refusal {
        code: "unreadable_response",
        message: message.into(),
        issues: vec![],
    }
}

/// A style reference is bounded, trimmed and free of control characters — the
/// same bound the Style Center's catalogue is admitted under.
fn checked_style_ref(reference: &str) -> Result<&str, String> {
    if reference.is_empty()
        || reference.trim() != reference
        || reference.chars().count() > MAX_STYLE_REF_CHARS
        || reference.chars().any(char::is_control)
    {
        return Err("a style request needs a bounded style_ref".into());
    }
    Ok(reference)
}

/// A style document is an object naming its MapLibre `type`.
fn is_document(value: Option<&Value>) -> bool {
    value.is_some_and(|value| value.get("type").is_some_and(Value::is_string))
}

fn bounded(payload: &Value) -> Result<(), String> {
    if serde_json::to_vec(payload)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_STYLE_REQUEST_BYTES
    {
        return Err("style payload exceeds the 2 MiB bound".into());
    }
    Ok(())
}

/// The seed geometry a styleable shape maps to. One owner for what the
/// browser's `mapGeometryToSeed` and Project Work's `seedGeometry` each
/// decided on their own.
pub fn seed_geometry(geometry: &str) -> Result<&'static str, String> {
    match geometry.trim().to_ascii_lowercase().as_str() {
        "point" | "multipoint" => Ok("point"),
        "line" | "linestring" | "multilinestring" => Ok("line"),
        "polygon" | "multipolygon" | "fill" => Ok("polygon"),
        _ => Err(format!("no styleable seed geometry for {geometry}")),
    }
}

/// The payload one publish carries: the reference, the authored document and
/// the declared write target, and nothing the backend did not declare.
pub fn validate_publish_payload(payload: &Value) -> Result<(), String> {
    let fields = payload
        .as_object()
        .ok_or("a style publish payload must be an object")?;
    checked_style_ref(
        fields
            .get("style_ref")
            .and_then(Value::as_str)
            .ok_or("a style request needs a bounded style_ref")?,
    )?;
    if !fields
        .get("style_target")
        .and_then(Value::as_str)
        .is_some_and(|target| !target.is_empty())
    {
        return Err("a style publish payload needs its style_target".into());
    }
    if !is_document(fields.get("style_data")) {
        return Err("a style publish payload needs a style_data object with a string type".into());
    }
    crate::style_validation::validate(&fields["style_data"])?;
    for (key, value) in fields {
        let acceptable = match key.as_str() {
            "style_ref" | "style_target" | "style_data" => true,
            "slug" | "template_name" | "styling_template_id" | "layer_name" | "project_id" => {
                value.is_string()
            }
            "available_fields" => value
                .as_array()
                .is_some_and(|fields| fields.iter().all(Value::is_string)),
            "field_values" => value.is_object(),
            "create_only" => value.is_boolean(),
            "expected_style_sha256" => value.as_str().is_some_and(|digest| {
                digest.len() == 64 && digest.bytes().all(|c| c.is_ascii_hexdigit())
            }),
            other => return Err(format!("unknown style payload field {other}")),
        };
        if !acceptable {
            return Err(format!("style payload field {key} has the wrong type"));
        }
    }
    bounded(payload)
}

/// The body a publish sends. The host performs `domains.styles.update`.
pub fn publish_body(payload: &Value) -> Result<Value, String> {
    validate_publish_payload(payload)?;
    Ok(json!({"action": Action::Publish.wire(), "data": payload}))
}

/// The body a seed sends: the ref, the canonical geometry, and the optional
/// tile kind and seed word. `kind` is the request's `seed_kind` — the op's own
/// `kind` names the operation, so the wire word is set here.
pub fn seed_body(
    reference: &str,
    geometry: &str,
    kind: Option<&str>,
    seed: Option<&str>,
) -> Result<Value, String> {
    checked_style_ref(reference)?;
    let mut data = json!({"style_ref": reference, "geometry": seed_geometry(geometry)?});
    if let Some(kind) = kind {
        if kind != "geojson" && kind != "vt" {
            return Err(format!("no style seed kind for {kind}"));
        }
        data["kind"] = json!(kind);
    }
    if let Some(seed) = seed {
        if seed.is_empty() || seed.chars().count() > MAX_SEED_CHARS {
            return Err("a style seed word must be 1 through 200 characters".into());
        }
        data["seed"] = json!(seed);
    }
    Ok(json!({"action": Action::Seed.wire(), "data": data}))
}

/// The body a shared-class read sends.
pub fn read_shared_body(reference: &str) -> Result<Value, String> {
    checked_style_ref(reference)?;
    Ok(json!({"action": Action::ReadShared.wire(), "data": {"style_ref": reference}}))
}

/// The body a work-map runtime assembly sends. Every runtime identity is the
/// backend's; the client names only the logical root, the geometry and the
/// authored document.
pub fn assemble_runtime_body(
    logical_root: &str,
    geometry: &str,
    style_data: &Value,
) -> Result<Value, String> {
    if logical_root.is_empty() || logical_root.chars().count() > MAX_LOGICAL_ROOT_CHARS {
        return Err("a runtime style request needs a bounded logical_root".into());
    }
    if geometry.is_empty() || geometry.chars().count() > MAX_GEOMETRY_CHARS {
        return Err("a runtime style request needs a bounded geometry".into());
    }
    if !is_document(Some(style_data)) {
        return Err("a runtime style request needs a style_data object with a string type".into());
    }
    let body = json!({
        "action": Action::AssembleRuntime.wire(),
        "data": {"logical_root": logical_root, "geometry": geometry, "style_data": style_data},
    });
    bounded(&body)?;
    Ok(body)
}

fn request_answer(action: Action, body: Value) -> Value {
    json!({
        "operation": action.operation(),
        "method": METHOD,
        "path": PATH,
        "action": action.wire(),
        "body": body,
        "response_limit": action.response_limit(),
        "timeout_seconds": TIMEOUT_SECONDS,
    })
}

/// What the backend named as wrong with the request, in its own words:
/// `error_details.issues[]` as `"<path>: <message>"`, or the message alone.
/// Non-object entries and entries without a message are skipped.
fn issue_messages(response: &Value) -> Vec<String> {
    response
        .get("error_details")
        .map(backend_issues)
        .unwrap_or_default()
}

/// The issues a backend refusal's details name, as `"<path>: <message>"`
/// text, at most [`MAX_ISSUES`].
pub fn backend_issues(details: &Value) -> Vec<String> {
    details
        .get("issues")
        .and_then(Value::as_array)
        .map(|issues| {
            issues
                .iter()
                .filter_map(|issue| {
                    let issue = issue.as_object()?;
                    let message = issue
                        .get("message")
                        .and_then(Value::as_str)
                        .filter(|message| !message.is_empty())?;
                    Some(match issue.get("path").and_then(text) {
                        Some(path) => format!("{path}: {message}"),
                        None => message.to_owned(),
                    })
                })
                .take(MAX_ISSUES)
                .collect()
        })
        .unwrap_or_default()
}

/// The `{success, data}` envelope the style family answers with, plus the
/// issues a refusal named. The envelope itself is `layer_document`'s.
fn envelope(response: &Value) -> Result<&Map<String, Value>, Refusal> {
    crate::layer_document::envelope(response, FALLBACK).map_err(|mut refusal| {
        refusal.issues = issue_messages(response);
        refusal
    })
}

/// One answered warning, as text. ds-brain's expression warnings are
/// `{path, message}` issue objects; older shapes carry `property` and
/// `issue.message`; the Style Center has always shown plain strings too.
fn warning_messages(warnings: Option<&Value>) -> Result<Vec<String>, ()> {
    let Some(warnings) = warnings else {
        return Ok(vec![]);
    };
    let warnings = warnings.as_array().ok_or(())?;
    if warnings.len() > MAX_WARNINGS {
        return Err(());
    }
    let mut messages = Vec::with_capacity(warnings.len());
    for warning in warnings {
        if let Some(warning) = warning.as_str() {
            if warning.len() > MAX_WARNING_CHARS {
                return Err(());
            }
            messages.push(warning.to_owned());
            continue;
        }
        let warning = warning.as_object().ok_or(())?;
        let message = warning
            .get("issue")
            .and_then(|issue| issue.get("message"))
            .and_then(Value::as_str)
            .or_else(|| warning.get("message").and_then(Value::as_str))
            .unwrap_or_default();
        if message.is_empty() {
            continue;
        }
        if message.len() > MAX_WARNING_CHARS {
            return Err(());
        }
        let property = warning
            .get("property")
            .and_then(text)
            .or_else(|| warning.get("path").and_then(text));
        messages.push(match property {
            Some(property) => format!("{property}: {message}"),
            None => message.to_owned(),
        });
    }
    Ok(messages)
}

fn same(expected: &Value, data: &Map<String, Value>, key: &str) -> bool {
    data.get(key).unwrap_or(&Value::Null) == expected.get(key).unwrap_or(&Value::Null)
}

/// Admit one publish receipt: the backend wrote what was authored, against the
/// target that was authored, and every warning it answered is readable text.
pub fn admit_publish(expected: &Value, response: &Value) -> Result<Value, Refusal> {
    let data = envelope(response)?;
    let mismatch = || unreadable("style publish receipt does not match the authored target");
    // A receipt is only ever admitted against the payload that was sent, and
    // that payload named its target: an expectation without one is a host
    // slip, never a match.
    if !expected
        .get("style_target")
        .and_then(Value::as_str)
        .is_some_and(|target| !target.is_empty())
        || data.get("updated") != Some(&Value::Bool(true))
        || !data.get("style_data").is_some_and(Value::is_object)
        || !same(expected, data, "style_target")
        || ["slug", "template_name", "styling_template_id", "layer_name"]
            .iter()
            .any(|key| expected.get(*key).is_some() && !same(expected, data, key))
    {
        return Err(mismatch());
    }
    let warnings = warning_messages(data.get("warnings"))
        .map_err(|()| unreadable("style publish warnings are malformed"))?;
    let mut admitted = Map::new();
    for key in ["style_ref", "style_target"] {
        admitted.insert(
            key.to_owned(),
            expected.get(key).cloned().unwrap_or(Value::Null),
        );
    }
    for key in ["slug", "template_name", "styling_template_id", "layer_name"] {
        if let Some(value) = expected.get(key) {
            admitted.insert(key.to_owned(), value.clone());
        }
    }
    admitted.insert(
        "document".to_owned(),
        data.get("style_data").cloned().unwrap_or(Value::Null),
    );
    admitted.insert("warnings".to_owned(), json!(warnings));
    Ok(Value::Object(admitted))
}

/// Admit one seed: the ref that was asked for, the canonical geometry that was
/// asked for, a known tile kind, a real document — and nothing persisted.
pub fn admit_seed(expected: &Value, response: &Value) -> Result<Value, Refusal> {
    let reference = expected
        .get("style_ref")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let geometry = expected
        .get("geometry")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let invalid = || unreadable(format!("invalid style seed for {reference}"));
    let data = envelope(response)?;
    if reference.is_empty()
        || geometry.is_empty()
        || data.get("style_ref").and_then(Value::as_str) != Some(reference)
        || data.get("persisted") != Some(&Value::Bool(false))
        || data.get("geometry").and_then(Value::as_str) != Some(geometry)
        || !data
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "geojson" || kind == "vt")
        || !is_document(data.get("style_data"))
    {
        return Err(invalid());
    }
    Ok(json!({
        "style_ref": reference,
        "geometry": geometry,
        "kind": data.get("kind").cloned().unwrap_or(Value::Null),
        "style_data": data.get("style_data").cloned().unwrap_or(Value::Null),
    }))
}

/// Admit one shared-class read: the ref that was asked for, and either a found
/// document or an honest absence. A found document that is not there is a
/// contract slip, not an empty style.
pub fn admit_read_shared(expected: &Value, response: &Value) -> Result<Value, Refusal> {
    let reference = expected
        .get("style_ref")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let invalid = || unreadable(format!("invalid shared style for {reference}"));
    let data = envelope(response)?;
    if reference.is_empty() || data.get("style_ref").and_then(Value::as_str) != Some(reference) {
        return Err(invalid());
    }
    let found = data
        .get("found")
        .and_then(Value::as_bool)
        .ok_or_else(invalid)?;
    if !found {
        return Ok(json!({"style_ref": reference, "found": false}));
    }
    if !is_document(data.get("style_data")) {
        return Err(invalid());
    }
    Ok(json!({
        "style_ref": reference,
        "found": true,
        "style_data": data.get("style_data").cloned().unwrap_or(Value::Null),
    }))
}

/// Admit one work-map runtime family: the backend's authority, the root and
/// geometry that were asked for, and a bounded family of layers that all
/// belong to the one source the backend named.
pub fn admit_runtime(expected: &Value, response: &Value) -> Result<Value, Refusal> {
    let data = envelope(response)?;
    let invalid = || unreadable("invalid work-map runtime family");
    let logical_root = expected
        .get("logical_root")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let geometry = expected
        .get("geometry")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if logical_root.is_empty()
        || geometry.is_empty()
        || data.get("authority").and_then(Value::as_str) != Some(RUNTIME_FAMILY_AUTHORITY)
        || data.get("logical_root").and_then(Value::as_str) != Some(logical_root)
        || data.get("geometry").and_then(Value::as_str) != Some(geometry)
        || !data.get("style_data").is_some_and(Value::is_object)
    {
        return Err(invalid());
    }
    let source = data.get("source_id").and_then(text).ok_or_else(invalid)?;
    if data.get("layer_id").and_then(text).is_none()
        || data.get("style_ref").and_then(text).is_none()
    {
        return Err(invalid());
    }
    let layers = data
        .get("layers")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if layers.is_empty()
        || layers.len() > MAX_RUNTIME_LAYERS
        || layers.iter().any(|layer| {
            !layer.is_object()
                || layer.get("id").and_then(text).is_none()
                || !layer.get("type").is_some_and(Value::is_string)
                || layer.get("source").and_then(Value::as_str) != Some(source)
        })
    {
        return Err(invalid());
    }
    if let Some(images) = data.get("required_images") {
        if !images.as_array().is_some_and(|images| {
            images.len() <= MAX_REQUIRED_IMAGES && images.iter().all(Value::is_object)
        }) {
            return Err(invalid());
        }
    }
    Ok(Value::Object(data.clone()))
}

/// The payload one guided or direct save publishes: `style_plan`'s answer,
/// bounded, over a document that names its type.
pub fn save_payload(snapshot: &Value, reference: &str, document: &Value) -> Result<Value, String> {
    checked_style_ref(reference)?;
    if !is_document(Some(document)) {
        return Err("a style save needs a style document with a string type".into());
    }
    crate::style_validation::validate(document)?;
    let payload = crate::style_plan::style_save_payload(snapshot, reference, document)?;
    bounded(&payload)?;
    Ok(payload)
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
    /// The payload a publish carries for one authored document, routed by the
    /// backend's own editor rows.
    SavePayload {
        snapshot: Value,
        reference: String,
        document: Value,
    },
    /// The declared operation, the body to send and the bound to read under.
    /// `seed_kind` is the seed's tile kind: `kind` names this op.
    Request {
        action: Action,
        #[serde(default)]
        payload: Value,
        #[serde(default)]
        style_ref: String,
        #[serde(default)]
        geometry: String,
        #[serde(default)]
        seed_kind: Option<String>,
        #[serde(default)]
        seed: Option<String>,
        #[serde(default)]
        logical_root: String,
        #[serde(default)]
        style_data: Value,
    },
    /// Whether a parsed answer is the style that was authored, the seed, the
    /// shared document or the runtime family that was asked for — or why not.
    Admit {
        action: Action,
        #[serde(default)]
        expected: Value,
        response: Value,
    },
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("style document request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid style document request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!(
            "unsupported style document schema; expected {SCHEMA}"
        ));
    }
    let value = match request.op {
        Op::SavePayload {
            snapshot,
            reference,
            document,
        } => json!({"payload": save_payload(&snapshot, &reference, &document)?}),
        Op::Request {
            action,
            payload,
            style_ref,
            geometry,
            seed_kind,
            seed,
            logical_root,
            style_data,
        } => request_answer(
            action,
            match action {
                Action::Publish => publish_body(&payload)?,
                Action::Seed => {
                    seed_body(&style_ref, &geometry, seed_kind.as_deref(), seed.as_deref())?
                }
                Action::ReadShared => read_shared_body(&style_ref)?,
                Action::AssembleRuntime => {
                    assemble_runtime_body(&logical_root, &geometry, &style_data)?
                }
            },
        ),
        Op::Admit {
            action,
            expected,
            response,
        } => {
            let admitted = match action {
                Action::Publish => admit_publish(&expected, &response),
                Action::Seed => admit_seed(&expected, &response),
                Action::ReadShared => admit_read_shared(&expected, &response),
                Action::AssembleRuntime => admit_runtime(&expected, &response),
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

    fn wrap(data: Value) -> Value {
        json!({"success": true, "data": data})
    }
    fn answer(op: Value) -> Value {
        serde_json::from_str(
            &evaluate(&serde_json::to_vec(&json!({"schema": SCHEMA, "op": op})).unwrap()).unwrap(),
        )
        .unwrap()
    }
    fn payload() -> Value {
        json!({
            "style_ref": "master/roads", "style_target": "design", "layer_name": "roads",
            "style_data": {"type": "line", "paint": {"line-width": 3}}
        })
    }
    fn receipt() -> Value {
        json!({
            "updated": true, "style_target": "design", "layer_name": "roads",
            "style_data": {"type": "line", "paint": {"line-width": 3}}
        })
    }

    #[test]
    fn requests_carry_the_declared_operation_and_the_closed_bodies() {
        let out = answer(json!({"kind": "request", "action": "publish", "payload": payload()}));
        assert_eq!(out["operation"], UPDATE_OPERATION);
        assert_eq!(out["method"], METHOD);
        assert_eq!(out["path"], PATH);
        assert_eq!(out["action"], "update_style");
        assert_eq!(
            out["body"],
            json!({"action": "update_style", "data": payload()})
        );
        assert_eq!(out["response_limit"], UPDATE_RESPONSE_LIMIT);
        assert_eq!(out["timeout_seconds"], TIMEOUT_SECONDS);

        let out = answer(json!({
            "kind": "request", "action": "seed", "style_ref": "ud/x",
            "geometry": "MultiLineString", "seed_kind": "vt", "seed": "x"
        }));
        assert_eq!(out["operation"], ACTION_OPERATION);
        assert_eq!(out["response_limit"], ACTION_RESPONSE_LIMIT);
        assert_eq!(
            out["body"],
            json!({"action": "seed_style", "data": {"style_ref": "ud/x", "geometry": "line", "kind": "vt", "seed": "x"}})
        );
        let out = answer(json!({"kind": "request", "action": "read_shared", "style_ref": "ud/x"}));
        assert_eq!(
            out["body"],
            json!({"action": "get_user_data_style", "data": {"style_ref": "ud/x"}})
        );
        let out = answer(json!({
            "kind": "request", "action": "assemble_runtime", "logical_root": "personal_notes",
            "geometry": "Point", "style_data": {"type": "circle"}
        }));
        assert_eq!(
            out["body"],
            json!({"action": "assemble_work_map_runtime_style", "data": {"logical_root": "personal_notes", "geometry": "Point", "style_data": {"type": "circle"}}})
        );
    }

    #[test]
    fn every_styleable_shape_maps_to_one_seed_geometry() {
        for (given, canonical) in [
            ("Point", "point"),
            ("MULTIPOINT", "point"),
            ("LineString", "line"),
            ("multilinestring", "line"),
            ("line", "line"),
            ("Polygon", "polygon"),
            ("MultiPolygon", "polygon"),
            ("fill", "polygon"),
        ] {
            assert_eq!(seed_geometry(given).unwrap(), canonical, "{given}");
        }
        assert_eq!(
            seed_geometry("GeometryCollection").unwrap_err(),
            "no styleable seed geometry for GeometryCollection"
        );
        assert!(seed_body("ud/x", "Point", Some("png"), None).is_err());
        assert!(seed_body("ud/x", "Point", None, Some("")).is_err());
        assert!(seed_body(" ud/x", "Point", None, None).is_err());
    }

    #[test]
    fn a_publish_payload_declares_only_what_the_backend_declared() {
        assert!(validate_publish_payload(&payload()).is_ok());
        let mut unknown = payload();
        unknown["colour"] = json!("red");
        assert_eq!(
            validate_publish_payload(&unknown).unwrap_err(),
            "unknown style payload field colour"
        );
        for (key, value) in [
            ("slug", json!(7)),
            ("available_fields", json!([1])),
            ("field_values", json!([])),
            ("create_only", json!("yes")),
        ] {
            let mut wrong = payload();
            wrong[key] = value;
            assert!(validate_publish_payload(&wrong).is_err(), "{key}");
        }
        let mut untyped = payload();
        untyped["style_data"] = json!({"paint": {}});
        assert!(validate_publish_payload(&untyped).is_err());
        let mut targetless = payload();
        targetless["style_target"] = json!("");
        assert!(validate_publish_payload(&targetless).is_err());
        let mut unbounded = payload();
        unbounded["style_ref"] = json!("x".repeat(MAX_STYLE_REF_CHARS + 1));
        assert!(validate_publish_payload(&unbounded).is_err());
        let mut oversize = payload();
        oversize["style_data"]["paint"]["line-pattern"] =
            json!("x".repeat(MAX_STYLE_REQUEST_BYTES));
        assert_eq!(
            validate_publish_payload(&oversize).unwrap_err(),
            "style payload exceeds the 2 MiB bound"
        );
        assert!(validate_publish_payload(&json!([])).is_err());
    }

    #[test]
    fn a_publish_receipt_is_admitted_only_against_the_authored_target() {
        let out = answer(
            json!({"kind": "admit", "action": "publish", "expected": payload(), "response": wrap(receipt())}),
        );
        assert_eq!(out["outcome"], "admitted");
        assert_eq!(
            out["data"],
            json!({
                "style_ref": "master/roads", "style_target": "design", "layer_name": "roads",
                "document": {"type": "line", "paint": {"line-width": 3}}, "warnings": []
            })
        );
        for broken in [
            json!({"updated": false, "style_target": "design", "layer_name": "roads", "style_data": {}}),
            json!({"updated": true, "style_target": "user_data", "layer_name": "roads", "style_data": {}}),
            json!({"updated": true, "style_target": "design", "layer_name": "other", "style_data": {}}),
            json!({"updated": true, "style_target": "design", "layer_name": "roads", "style_data": []}),
            json!({"updated": true, "style_target": "design", "layer_name": "roads"}),
        ] {
            let refusal = admit_publish(&payload(), &wrap(broken.clone())).unwrap_err();
            assert_eq!(refusal.code, "unreadable_response", "{broken}");
            assert_eq!(
                refusal.message,
                "style publish receipt does not match the authored target"
            );
        }
        // An optional identity the payload never declared is not compared.
        let mut extra = receipt();
        extra["slug"] = json!("anything");
        assert!(admit_publish(&payload(), &wrap(extra)).is_ok());
        // An expectation that names no target is a host slip: a receipt that
        // also names none must not slip through as a match of nothing.
        for expected in [
            json!({}),
            json!({"style_ref": "master/roads", "style_target": ""}),
        ] {
            let mut targetless = receipt();
            targetless.as_object_mut().unwrap().remove("style_target");
            let refusal = admit_publish(&expected, &wrap(targetless)).unwrap_err();
            assert_eq!(
                refusal.message,
                "style publish receipt does not match the authored target"
            );
        }
    }

    #[test]
    fn publish_warnings_are_normalised_text_or_the_answer_is_unreadable() {
        for (given, expected) in [
            (json!(["plain"]), json!(["plain"])),
            (
                json!([{"path": "paint.line-width", "message": "clamped"}]),
                json!(["paint.line-width: clamped"]),
            ),
            (
                json!([{"property": "label", "issue": {"message": "ignored"}}]),
                json!(["label: ignored"]),
            ),
            (json!([{"message": "no property"}]), json!(["no property"])),
            (
                json!([{"path": "", "message": "empty path"}]),
                json!(["empty path"]),
            ),
            (json!([{"path": "x"}]), json!([])),
            (json!([]), json!([])),
        ] {
            let mut answered = receipt();
            answered["warnings"] = given.clone();
            assert_eq!(
                admit_publish(&payload(), &wrap(answered)).unwrap()["warnings"],
                expected,
                "{given}"
            );
        }
        for malformed in [json!([7]), json!("one"), json!([null]), json!([["x"]])] {
            let mut answered = receipt();
            answered["warnings"] = malformed.clone();
            let refusal = admit_publish(&payload(), &wrap(answered)).unwrap_err();
            assert_eq!(
                refusal.message, "style publish warnings are malformed",
                "{malformed}"
            );
        }
        let mut excessive = receipt();
        excessive["warnings"] = json!(vec!["w"; MAX_WARNINGS + 1]);
        assert!(admit_publish(&payload(), &wrap(excessive)).is_err());
        let mut long = receipt();
        long["warnings"] = json!(["w".repeat(MAX_WARNING_CHARS + 1)]);
        assert!(admit_publish(&payload(), &wrap(long)).is_err());
    }

    #[test]
    fn a_backend_refusal_is_reported_in_its_own_words_with_its_issues() {
        let refusal = admit_publish(
            &payload(),
            &json!({
                "success": false, "error": "Invalid style document", "status": 400,
                "error_code": "validation_failed",
                "error_details": {"issues": [
                    {"path": "paint.line-color", "message": "unknown expression"},
                    {"message": "no path"},
                    "not an object",
                    {"path": "x"}
                ]}
            }),
        )
        .unwrap_err();
        assert_eq!(refusal.code, "backend_refused");
        assert_eq!(
            refusal.message,
            "Invalid style document (400 validation_failed)"
        );
        assert_eq!(
            refusal.issues,
            ["paint.line-color: unknown expression", "no path"]
        );
        let silent = admit_publish(&payload(), &json!({"success": false})).unwrap_err();
        assert_eq!(silent.message, FALLBACK);
        let out = answer(
            json!({"kind": "admit", "action": "publish", "expected": payload(), "response": {"success": false, "error": "denied"}}),
        );
        assert_eq!(out["outcome"], "refused");
        assert_eq!(out["refusal"]["code"], "backend_refused");
        assert_eq!(out["refusal"]["message"], "denied");
        assert_eq!(
            admit_publish(&payload(), &json!({"success": true, "data": []}))
                .unwrap_err()
                .code,
            "unreadable_response"
        );
    }

    #[test]
    fn seeds_shared_reads_and_runtime_families_are_admitted_on_their_own_terms() {
        let expected = json!({"style_ref": "ud/x", "geometry": "polygon"});
        let seed = json!({"style_ref": "ud/x", "geometry": "polygon", "kind": "geojson", "persisted": false, "seed": "x", "style_data": {"type": "fill"}});
        assert_eq!(
            admit_seed(&expected, &wrap(seed.clone())).unwrap(),
            json!({"style_ref": "ud/x", "geometry": "polygon", "kind": "geojson", "style_data": {"type": "fill"}})
        );
        for broken in [
            json!({"style_ref": "ud/other", "geometry": "polygon", "kind": "geojson", "persisted": false, "style_data": {"type": "fill"}}),
            json!({"style_ref": "ud/x", "geometry": "polygon", "kind": "geojson", "persisted": true, "style_data": {"type": "fill"}}),
            json!({"style_ref": "ud/x", "geometry": "point", "kind": "geojson", "persisted": false, "style_data": {"type": "fill"}}),
            json!({"style_ref": "ud/x", "geometry": "polygon", "kind": "mvt", "persisted": false, "style_data": {"type": "fill"}}),
            json!({"style_ref": "ud/x", "geometry": "polygon", "kind": "geojson", "persisted": false, "style_data": {}}),
            json!({"style_ref": "ud/x", "geometry": "polygon", "kind": "geojson", "style_data": {"type": "fill"}}),
        ] {
            assert_eq!(
                admit_seed(&expected, &wrap(broken.clone()))
                    .unwrap_err()
                    .message,
                "invalid style seed for ud/x",
                "{broken}"
            );
        }
        assert!(admit_seed(&json!({}), &wrap(seed)).is_err());

        let expected = json!({"style_ref": "ud/x"});
        assert_eq!(
            admit_read_shared(
                &expected,
                &wrap(json!({"style_ref": "ud/x", "layer_name": "x", "found": true, "style_data": {"type": "circle"}}))
            )
            .unwrap(),
            json!({"style_ref": "ud/x", "found": true, "style_data": {"type": "circle"}})
        );
        assert_eq!(
            admit_read_shared(
                &expected,
                &wrap(json!({"style_ref": "ud/x", "found": false}))
            )
            .unwrap(),
            json!({"style_ref": "ud/x", "found": false})
        );
        for broken in [
            json!({"style_ref": "ud/x", "found": true}),
            json!({"style_ref": "ud/x", "found": "yes"}),
            json!({"style_ref": "ud/other", "found": false}),
        ] {
            assert_eq!(
                admit_read_shared(&expected, &wrap(broken.clone()))
                    .unwrap_err()
                    .message,
                "invalid shared style for ud/x",
                "{broken}"
            );
        }

        let expected = json!({"logical_root": "personal_notes", "geometry": "Point"});
        let family = json!({
            "authority": RUNTIME_FAMILY_AUTHORITY, "logical_root": "personal_notes",
            "geometry": "Point", "source_id": "ds-work-notes-source",
            "layer_id": "ds-work-notes-point", "style_ref": "ud/personal_notes_point",
            "style_data": {"type": "circle"},
            "layers": [{"id": "ds-work-notes-point", "type": "circle", "source": "ds-work-notes-source"}],
            "required_images": []
        });
        assert_eq!(
            admit_runtime(&expected, &wrap(family.clone())).unwrap(),
            family
        );
        for broken in [
            json!({"authority": "someone-else/v1"}),
            json!({"logical_root": "project_work"}),
            json!({"geometry": "LineString"}),
            json!({"source_id": ""}),
            json!({"layers": []}),
            json!({"layers": [{"id": "x", "type": "circle", "source": "another-source"}]}),
            json!({"layers": [{"type": "circle", "source": "ds-work-notes-source"}]}),
            json!({"required_images": [1]}),
        ] {
            let mut invalid = family.clone();
            for (key, value) in broken.as_object().unwrap() {
                invalid[key] = value.clone();
            }
            assert_eq!(
                admit_runtime(&expected, &wrap(invalid))
                    .unwrap_err()
                    .message,
                "invalid work-map runtime family",
                "{broken}"
            );
        }
    }

    #[test]
    fn a_save_payload_is_style_plans_answer_bounded_and_typed() {
        let snapshot = json!({
            "project_id": "p1", "styles": {},
            "style_editors": [{"style_ref": "master/roads", "style_target": "design", "layer_name": "roads"}]
        });
        let out = answer(json!({
            "kind": "save_payload", "snapshot": snapshot, "reference": "master/roads",
            "document": {"type": "line", "paint": {"line-width": 3}}
        }));
        assert_eq!(out["payload"], payload());
        assert!(save_payload(&snapshot, "master/roads", &json!({"paint": {}})).is_err());
        assert!(save_payload(&snapshot, "", &json!({"type": "line"})).is_err());
        assert!(save_payload(&snapshot, "unknown/ref", &json!({"type": "line"})).is_err());
    }

    #[test]
    fn malformed_requests_are_refused() {
        assert!(evaluate(b"{").is_err());
        assert!(evaluate(&serde_json::to_vec(&json!({"schema": "x", "op": {"kind": "request", "action": "publish", "payload": payload()}})).unwrap()).is_err());
        assert!(evaluate(
            &serde_json::to_vec(&json!({"schema": SCHEMA, "op": {"kind": "sideways"}})).unwrap()
        )
        .is_err());
        assert!(evaluate(&serde_json::to_vec(&json!({"schema": SCHEMA, "op": {"kind": "request", "action": "publish", "payload": payload(), "extra": 1}})).unwrap()).is_err());
        assert!(evaluate(&serde_json::to_vec(&json!({"schema": SCHEMA, "op": {"kind": "request", "action": "erase", "payload": payload()}})).unwrap()).is_err());
        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
    }
}
