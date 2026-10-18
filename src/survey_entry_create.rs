//! Closed, transport-free authority for one Survey entry `create` mutation.
//!
//! Hosts provide selected-project identity and perform IO.  This module owns
//! the create grammar, canonical body and the successful service receipt.

pub mod direct;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub const SURVEY_ENTRY_CREATE_SCHEMA: &str = "ds.survey-entry-create/v1";
pub const MAX_DOCUMENT_ID_BYTES: usize = 1500;
pub const MAX_FORM_SLUG_BYTES: usize = 256;
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 512;
pub const MAX_PAYLOAD_BYTES: usize = 900 * 1024;
const MAX_PROJECT_CHARS: usize = 200;
const MAX_CONTEXT_SEGMENTS: usize = 32;
const MAX_INPUT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Mobile,
    Web,
    Unknown,
}
impl Origin {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Mobile => "mobile",
            Self::Web => "web",
            Self::Unknown => "unknown",
        }
    }
}

/// A stable, bounded refusal category. Hosts map it to their own error model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    InvalidInput,
    UnreadableResponse,
}
impl Refusal {
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidInput => {
                "the Survey entry create request is outside its closed grammar or bounds"
            }
            Self::UnreadableResponse => {
                "the Survey entry create response is outside the closed native contract"
            }
        }
    }
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
impl std::error::Error for Refusal {}

#[derive(Clone, PartialEq)]
pub struct Request {
    form: String,
    doc_id: String,
    idempotency_key: String,
    data: Map<String, Value>,
    created_at: String,
    origin: Origin,
    context_key: Option<String>,
    geometry: Option<Value>,
    connectivity: Option<Map<String, Value>>,
    detailed_location: Option<Map<String, Value>>,
}
impl Request {
    pub fn new(
        form: impl Into<String>,
        doc_id: impl Into<String>,
        idempotency_key: impl Into<String>,
        data: Map<String, Value>,
        created_at: impl Into<String>,
        origin: Origin,
    ) -> Result<Self, Refusal> {
        let created_at = canonicalize_rfc3339(&created_at.into()).ok_or(Refusal::InvalidInput)?;
        let result = Self {
            form: form.into(),
            doc_id: doc_id.into(),
            idempotency_key: idempotency_key.into(),
            data,
            created_at,
            origin,
            context_key: None,
            geometry: None,
            connectivity: None,
            detailed_location: None,
        };
        result.validate()?;
        Ok(result)
    }
    pub fn with_context_key(mut self, value: impl Into<String>) -> Result<Self, Refusal> {
        self.context_key = Some(value.into());
        self.validate()?;
        Ok(self)
    }
    pub fn with_geometry(mut self, value: Value) -> Result<Self, Refusal> {
        self.geometry = Some(value);
        self.validate()?;
        Ok(self)
    }
    pub fn with_connectivity(mut self, value: Map<String, Value>) -> Result<Self, Refusal> {
        self.connectivity = Some(value);
        self.validate()?;
        Ok(self)
    }
    pub fn with_detailed_location(mut self, value: Map<String, Value>) -> Result<Self, Refusal> {
        self.detailed_location = Some(value);
        self.validate()?;
        Ok(self)
    }
    pub fn form(&self) -> &str {
        &self.form
    }
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
    pub fn data(&self) -> &Map<String, Value> {
        &self.data
    }
    pub fn created_at(&self) -> &str {
        &self.created_at
    }
    pub const fn origin(&self) -> Origin {
        self.origin
    }
    pub fn context_key(&self) -> Option<&str> {
        self.context_key.as_deref()
    }
    pub fn geometry(&self) -> Option<&Value> {
        self.geometry.as_ref()
    }
    pub fn connectivity(&self) -> Option<&Map<String, Value>> {
        self.connectivity.as_ref()
    }
    pub fn detailed_location(&self) -> Option<&Map<String, Value>> {
        self.detailed_location.as_ref()
    }
    fn validate(&self) -> Result<(), Refusal> {
        if !valid_form_slug(&self.form)
            || !valid_document_id(&self.doc_id)
            || !bounded_idempotency_key(&self.idempotency_key)
            || self
                .context_key
                .as_deref()
                .is_some_and(|v| !valid_context_key(v))
            || self.geometry.as_ref().is_some_and(|v| !valid_geometry(v))
            || self.geometry.as_ref().is_some_and(|v| admitted(v).is_err())
            || !finite_json(&Value::Object(self.data.clone()))
            || self.geometry.as_ref().is_some_and(|v| !finite_json(v))
            || self
                .connectivity
                .as_ref()
                .is_some_and(|v| !finite_json(&Value::Object(v.clone())))
            || self
                .detailed_location
                .as_ref()
                .is_some_and(|v| !finite_json(&Value::Object(v.clone())))
            || serde_json::to_vec(&(
                &self.data,
                &self.geometry,
                &self.connectivity,
                &self.detailed_location,
            ))
            .expect("JSON serializes")
            .len()
                > MAX_PAYLOAD_BYTES
        {
            return Err(Refusal::InvalidInput);
        }
        Ok(())
    }
}
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("form", &self.form)
            .field("doc_id", &self.doc_id)
            .field("data_field_count", &self.data.len())
            .field("origin", &self.origin)
            .field("has_context", &self.context_key.is_some())
            .field("has_geometry", &self.geometry.is_some())
            .field(
                "connectivity_field_count",
                &self.connectivity.as_ref().map_or(0, Map::len),
            )
            .field(
                "detailed_location_field_count",
                &self.detailed_location.as_ref().map_or(0, Map::len),
            )
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Identity {
    pub project_id: String,
    pub form_id: String,
    pub doc_id: String,
}
#[derive(Clone, PartialEq)]
pub struct Plan {
    identity: Identity,
    body: Value,
}
impl Plan {
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn body(&self) -> &Value {
        &self.body
    }
    pub fn body_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.body).expect("closed create body serializes")
    }
}
impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan")
            .field("identity", &self.identity)
            .finish()
    }
}
pub fn validate_project(project_id: &str) -> Result<(), Refusal> {
    if project_id.is_empty()
        || project_id.trim() != project_id
        || project_id.chars().count() > MAX_PROJECT_CHARS
        || project_id.chars().any(char::is_control)
        || !valid_document_id(project_id)
    {
        Err(Refusal::InvalidInput)
    } else {
        Ok(())
    }
}
pub fn plan(request: &Request, project_id: &str) -> Result<Plan, Refusal> {
    validate_project(project_id)?;
    request.validate()?;
    let identity = Identity {
        project_id: project_id.into(),
        form_id: request.form.clone(),
        doc_id: request.doc_id.clone(),
    };
    let mut body = json!({"project_id":project_id,"form_id":request.form,"doc_id":request.doc_id,"operation":"create","idempotency_key":request.idempotency_key,"data":request.data,"metadata":{"created_at":request.created_at,"origin":request.origin.token()}});
    let object = body.as_object_mut().expect("body is object");
    if let Some(v) = &request.context_key {
        object.insert("context_key".into(), json!(v));
    }
    if let Some(v) = &request.geometry {
        object.insert("geometry".into(), v.clone());
    }
    if let Some(v) = &request.connectivity {
        object.insert("connectivity".into(), json!(v));
    }
    if let Some(v) = &request.detailed_location {
        object.insert("detailed_location".into(), json!(v));
    }
    // Preserve the historical field order: optional buckets precede metadata.
    let mut ordered = Map::new();
    for key in [
        "project_id",
        "form_id",
        "doc_id",
        "operation",
        "idempotency_key",
        "data",
        "context_key",
        "geometry",
        "connectivity",
        "detailed_location",
        "metadata",
    ] {
        if let Some(v) = object.remove(key) {
            ordered.insert(key.into(), v);
        }
    }
    Ok(Plan {
        identity,
        body: Value::Object(ordered),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Receipt {
    project_id: String,
    form_id: String,
    doc_id: String,
    client_version: i64,
    replication_clock: String,
    replication_authority: String,
}
impl Receipt {
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn form_id(&self) -> &str {
        &self.form_id
    }
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }
    pub const fn client_version(&self) -> i64 {
        self.client_version
    }
    pub fn replication_clock(&self) -> &str {
        &self.replication_clock
    }
    pub fn replication_authority(&self) -> &str {
        &self.replication_authority
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evaluate {
    schema: String,
    operation: String,
    #[serde(flatten)]
    request: WireRequest,
    #[serde(default)]
    row: Option<WireRow>,
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    response: Option<Value>,
    #[serde(default)]
    authority: Option<direct::Authority>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum WireTimestamp {
    Text(String),
    Millis(i64),
}
impl WireTimestamp {
    fn canonical(self) -> Option<String> {
        match self {
            Self::Text(v) => canonicalize_rfc3339(&v),
            Self::Millis(ms) => OffsetDateTime::from_unix_timestamp_nanos((ms as i128) * 1_000_000)
                .ok()?
                .format(&Rfc3339)
                .ok(),
        }
    }
}
#[derive(Deserialize)]
struct WireRequest {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    eds_project_id: Option<String>,
    #[serde(default)]
    form: Option<String>,
    #[serde(default)]
    form_id: Option<String>,
    #[serde(default)]
    form_slug: Option<String>,
    #[serde(default)]
    doc_id: Option<String>,
    #[serde(default)]
    entry_id: Option<String>,
    #[serde(default)]
    idempotency_key: Option<String>,
    data: Map<String, Value>,
    #[serde(default)]
    created_at: Option<WireTimestamp>,
    #[serde(default)]
    metadata: Option<WireMetadata>,
    #[serde(default)]
    origin: Option<Origin>,
    #[serde(default)]
    context_key: Option<String>,
    #[serde(default)]
    geometry: Option<Value>,
    #[serde(default)]
    connectivity: Option<Map<String, Value>>,
    #[serde(default)]
    detailed_location: Option<Map<String, Value>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMetadata {
    created_at: WireTimestamp,
    #[serde(default)]
    origin: Option<Origin>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRow {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    form_id: Option<String>,
    #[serde(default)]
    doc_id: Option<String>,
    #[serde(default)]
    idempotency_key: Option<String>,
    #[serde(default)]
    created_at: Option<WireTimestamp>,
}
fn canonical_wire_request(
    w: WireRequest,
    row: Option<WireRow>,
) -> Result<(Request, String), Refusal> {
    let project = choose_many([
        w.project_id,
        w.eds_project_id,
        row.as_ref().and_then(|r| r.project_id.clone()),
    ])?;
    let form = choose_many([
        w.form,
        w.form_id,
        w.form_slug,
        row.as_ref().and_then(|r| r.form_id.clone()),
    ])?;
    let doc = choose_many([
        w.doc_id,
        w.entry_id,
        row.as_ref().and_then(|r| r.doc_id.clone()),
    ])?;
    let key = choose_many([
        w.idempotency_key,
        row.as_ref().and_then(|r| r.idempotency_key.clone()),
    ])?;
    let metadata = w.metadata;
    let created_at = w
        .created_at
        .or_else(|| {
            metadata.as_ref().map(|m| match &m.created_at {
                WireTimestamp::Text(v) => WireTimestamp::Text(v.clone()),
                WireTimestamp::Millis(v) => WireTimestamp::Millis(*v),
            })
        })
        .or_else(|| row.and_then(|r| r.created_at))
        .and_then(WireTimestamp::canonical)
        .ok_or(Refusal::InvalidInput)?;
    let origin = w
        .origin
        .or_else(|| metadata.and_then(|m| m.origin))
        .unwrap_or(Origin::Unknown);
    let mut request = Request::new(form, doc, key, w.data, created_at, origin)?;
    if let Some(v) = w.context_key.filter(|v| !v.is_empty()) {
        request = request.with_context_key(v)?;
    }
    if let Some(v) = w.geometry {
        request = request.with_geometry(v)?;
    }
    if let Some(v) = w.connectivity {
        request = request.with_connectivity(v)?;
    }
    if let Some(v) = w.detailed_location {
        request = request.with_detailed_location(v)?;
    }
    Ok((request, project))
}
fn choose_many<const N: usize>(items: [Option<String>; N]) -> Result<String, Refusal> {
    let values: Vec<String> = items.into_iter().flatten().collect();
    let value = values.first().cloned().ok_or(Refusal::InvalidInput)?;
    if values.iter().all(|candidate| candidate == &value) {
        Ok(value)
    } else {
        Err(Refusal::InvalidInput)
    }
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(Refusal::InvalidInput.message().into());
    }
    let wire: Evaluate =
        serde_json::from_slice(input).map_err(|_| Refusal::InvalidInput.message().to_owned())?;
    if wire.schema != SURVEY_ENTRY_CREATE_SCHEMA {
        return Err(Refusal::InvalidInput.message().into());
    }
    if let Some(geometry) = &wire.request.geometry {
        admitted(geometry)?;
    }
    let (request, project) =
        canonical_wire_request(wire.request, wire.row).map_err(|e| e.message().to_owned())?;
    let plan = plan(&request, &project).map_err(|e| e.message().to_owned())?;
    let output = match wire.operation.as_str() {
        "direct_plan" | "direct_receipt" => {
            let authority = wire.authority.ok_or_else(|| Refusal::InvalidInput.message().to_owned())?;
            let direct = direct::plan(&request, &project, &authority).map_err(|e| e.message().to_owned())?;
            if wire.operation == "direct_plan" {
                json!({"schema":SURVEY_ENTRY_CREATE_SCHEMA,"operation":"plan","identity":plan.identity,"payload":plan.body,"direct":direct})
            } else {
                let receipt = direct::receipt(&direct, wire.status.ok_or_else(|| Refusal::UnreadableResponse.message().to_owned())?,
                    &wire.response.ok_or_else(|| Refusal::UnreadableResponse.message().to_owned())?).map_err(|e| e.message().to_owned())?;
                json!({"schema":SURVEY_ENTRY_CREATE_SCHEMA,"operation":"receipt","receipt":receipt})
            }
        }
        "plan" | "normalize" => {
            json!({"schema":SURVEY_ENTRY_CREATE_SCHEMA,"operation":"plan","identity":plan.identity,"payload":plan.body})
        }
        _ => return Err(Refusal::InvalidInput.message().into()),
    };
    serde_json::to_string(&output).map_err(|_| "Survey entry create result encoding failed".into())
}

/// JSON boundary used by the browser's update adapter too.  It deliberately
/// returns the original JSON geometry only after the same strict create
/// grammar accepts it; the host owns persistence representation.
pub fn evaluate_geometry(input: &[u8]) -> Result<String, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GeometryRequest {
        schema: String,
        geometry: Value,
    }
    if input.len() > MAX_INPUT_BYTES {
        return Err(Refusal::InvalidInput.message().into());
    }
    let request: GeometryRequest =
        serde_json::from_slice(input).map_err(|_| Refusal::InvalidInput.message().to_owned())?;
    if request.schema == "ds.survey-entry-geometry/v1" {
        admitted(&request.geometry)?;
    }
    if request.schema != "ds.survey-entry-geometry/v1"
        || !valid_geometry(&request.geometry)
        || !finite_json(&request.geometry)
    {
        return Err(Refusal::InvalidInput.message().into());
    }
    serde_json::to_string(&request.geometry)
        .map_err(|_| "Survey geometry result encoding failed".into())
}

/// A Survey entry geometry DS admits: what BigQuery would refuse, and a polygon
/// covering a hemisphere, never reach the outbox. The sentence names the rule
/// and the part; `Request::validate` folds it into `Refusal::InvalidInput`.
fn admitted(geometry: &Value) -> Result<(), String> {
    crate::geometry_admission::admit_geometry(geometry)
        .map_err(|refusal| format!("the Survey entry geometry is not admitted: {refusal}"))
}
fn valid_form_slug(v: &str) -> bool {
    let b = v.as_bytes();
    (2..=MAX_FORM_SLUG_BYTES).contains(&b.len())
        && b.iter()
            .all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || matches!(x, b'_' | b'-'))
        && !matches!(b.first(), Some(b'_' | b'-'))
        && !matches!(b.last(), Some(b'_' | b'-'))
}
fn valid_document_id(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= MAX_DOCUMENT_ID_BYTES
        && v.trim() == v
        && v != "."
        && v != ".."
        && !v.contains('/')
        && !(v.starts_with("__") && v.ends_with("__"))
        && !v.chars().any(char::is_control)
}
fn bounded_idempotency_key(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= MAX_IDEMPOTENCY_KEY_BYTES
        && v.trim() == v
        && !v.chars().any(char::is_control)
}
fn valid_context_key(v: &str) -> bool {
    if v.is_empty() || v.eq_ignore_ascii_case("root") {
        return false;
    }
    let parts: Vec<_> = v.split(';').collect();
    !parts.is_empty()
        && parts.len() <= MAX_CONTEXT_SEGMENTS
        && parts.iter().all(|part| {
            let Some((slug, id)) = part.split_once(':') else {
                return false;
            };
            valid_form_slug(&unescape(slug)) && valid_document_id(&unescape(id))
        })
}
fn unescape(v: &str) -> String {
    v.replace("%2F", "/")
        .replace("%3A", ":")
        .replace("%3B", ";")
        .replace("%25", "%")
}
fn finite_json(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(_) | Value::String(_) => true,
        Value::Number(n) => n.as_f64().is_some_and(f64::is_finite),
        Value::Array(a) => a.iter().all(finite_json),
        Value::Object(o) => o.values().all(finite_json),
    }
}
fn exact_keys(o: &Map<String, Value>, e: &[&str]) -> bool {
    o.len() == e.len() && e.iter().all(|k| o.contains_key(*k))
}
fn valid_position(v: &Value) -> bool {
    v.as_array()
        .is_some_and(|a| a.len() >= 2 && a.iter().all(|n| n.as_f64().is_some_and(f64::is_finite)))
}
fn valid_positions(v: &Value, min: usize) -> bool {
    v.as_array()
        .is_some_and(|a| a.len() >= min && a.iter().all(valid_position))
}
fn valid_polygon(v: &Value) -> bool {
    v.as_array()
        .is_some_and(|a| !a.is_empty() && a.iter().all(|r| valid_positions(r, 4)))
}
fn valid_geometry(v: &Value) -> bool {
    let Some(o) = v.as_object() else { return false };
    match o.get("type").and_then(Value::as_str) {
        Some("Point") => {
            exact_keys(o, &["coordinates", "type"])
                && o.get("coordinates").is_some_and(valid_position)
        }
        Some("MultiPoint") => {
            exact_keys(o, &["coordinates", "type"])
                && o.get("coordinates").is_some_and(|x| valid_positions(x, 1))
        }
        Some("LineString") => {
            exact_keys(o, &["coordinates", "type"])
                && o.get("coordinates").is_some_and(|x| valid_positions(x, 2))
        }
        Some("MultiLineString") => {
            exact_keys(o, &["coordinates", "type"])
                && o.get("coordinates")
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty() && a.iter().all(|x| valid_positions(x, 2)))
        }
        Some("Polygon") => {
            exact_keys(o, &["coordinates", "type"])
                && o.get("coordinates").is_some_and(valid_polygon)
        }
        Some("MultiPolygon") => {
            exact_keys(o, &["coordinates", "type"])
                && o.get("coordinates")
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty() && a.iter().all(valid_polygon))
        }
        Some("GeometryCollection") => {
            exact_keys(o, &["geometries", "type"])
                && o.get("geometries")
                    .and_then(Value::as_array)
                    .is_some_and(|a| a.iter().all(valid_geometry))
        }
        _ => false,
    }
}
fn canonicalize_rfc3339(v: &str) -> Option<String> {
    if v.is_empty() || v.trim() != v {
        return None;
    }
    OffsetDateTime::parse(v, &Rfc3339)
        .ok()?
        .to_offset(time::UtcOffset::UTC)
        .format(&Rfc3339)
        .ok()
}
fn canonical_utc_rfc3339(v: &str) -> bool {
    canonicalize_rfc3339(v).as_deref() == Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        Request::new(
            "survey_points",
            "entry_1",
            "outbox-1",
            serde_json::from_value(json!({"name":"Pole 1"})).unwrap(),
            "2026-08-30T02:00:00+02:00",
            Origin::Unknown,
        )
        .unwrap()
        .with_context_key("parents:parent%3A1")
        .unwrap()
        .with_geometry(json!({"type":"Point","coordinates":[30.0,-1.0]}))
        .unwrap()
    }
    #[test]
    fn preserves_native_request_grammar_and_canonical_body() {
        let r = request();
        let p = plan(&r, "project_a").unwrap();
        assert_eq!(r.created_at(), "2026-08-30T00:00:00Z");
        assert_eq!(p.body_bytes(), br#"{"project_id":"project_a","form_id":"survey_points","doc_id":"entry_1","operation":"create","idempotency_key":"outbox-1","data":{"name":"Pole 1"},"context_key":"parents:parent%3A1","geometry":{"type":"Point","coordinates":[30.0,-1.0]},"metadata":{"created_at":"2026-08-30T00:00:00Z","origin":"unknown"}}"#);
        for (form, doc, key) in [
            ("Bad Form", "entry_1", "outbox-1"),
            ("survey_points", "../entry", "outbox-1"),
            ("survey_points", "entry_1", " key "),
        ] {
            assert_eq!(
                Request::new(
                    form,
                    doc,
                    key,
                    Map::new(),
                    "2026-08-30T00:00:00Z",
                    Origin::Unknown
                )
                .unwrap_err(),
                Refusal::InvalidInput
            );
        }
    }
    #[test]
    fn retired_gateway_receipt_operation_refuses() {
        let mut input=plan(&request(),"project_a").unwrap().body().clone();
        input["schema"]=json!(SURVEY_ENTRY_CREATE_SCHEMA);input["operation"]=json!("receipt");
        input["status"]=json!(201);input["response"]=json!({"success":true});
        assert!(evaluate(&serde_json::to_vec(&input).unwrap()).is_err());
    }
    #[test]
    fn wasm_surface_normalizes_durable_aliases_without_changing_identity() {
        let output:Value=serde_json::from_str(&evaluate(br#"{"schema":"ds.survey-entry-create/v1","operation":"normalize","eds_project_id":"project_a","form_slug":"survey_points","entry_id":"entry_1","idempotency_key":"outbox-1","data":{},"metadata":{"created_at":"2026-08-30T00:00:00Z","origin":"web"},"context_key":""}"#).unwrap()).unwrap();
        assert_eq!(
            output["identity"],
            json!({"project_id":"project_a","form_id":"survey_points","doc_id":"entry_1"})
        );
        assert!(output["payload"].get("context_key").is_none());
    }
    #[test]
    fn durable_row_is_a_fenced_identity_and_timestamp_fallback() {
        let input = br#"{"schema":"ds.survey-entry-create/v1","operation":"normalize","eds_project_id":"project_a","form_slug":"survey_points","entry_id":"entry_1","data":{},"row":{"project_id":"project_a","form_id":"survey_points","doc_id":"entry_1","idempotency_key":"outbox-1","created_at":1788048000000}}"#;
        assert!(evaluate(input).is_ok());
        let conflict = br#"{"schema":"ds.survey-entry-create/v1","operation":"normalize","project_id":"project_a","form_id":"survey_points","doc_id":"entry_1","idempotency_key":"key-a","data":{},"created_at":"2026-08-30T00:00:00Z","row":{"doc_id":"entry_2","idempotency_key":"key-b"}}"#;
        assert!(evaluate(conflict).is_err());
    }
    #[test]
    fn geometry_surface_is_exact_create_geojson_grammar() {
        assert_eq!(evaluate_geometry(br#"{"schema":"ds.survey-entry-geometry/v1","geometry":{"type":"Point","coordinates":[30.0,-1.0]}}"#).unwrap(), r#"{"type":"Point","coordinates":[30.0,-1.0]}"#);
        assert!(evaluate_geometry(br#"{"schema":"ds.survey-entry-geometry/v1","geometry":{"type":"Feature","geometry":null}}"#).is_err());
    }
    #[test]
    fn a_geometry_ds_does_not_admit_never_reaches_the_outbox() {
        let bowtie = json!({"type":"Polygon","coordinates":[[[30.0,-2.0],[30.2,-1.8],[30.2,-2.0],[30.0,-1.8],[30.0,-2.0]]]});
        let with = |geometry: &Value| {
            Request::new(
                "survey_points",
                "entry_1",
                "outbox-1",
                Map::new(),
                "2026-08-30T00:00:00Z",
                Origin::Web,
            )
            .unwrap()
            .with_geometry(geometry.clone())
        };
        assert_eq!(with(&bowtie).unwrap_err(), Refusal::InvalidInput);
        let altitude = json!({"type":"Point","coordinates":[30.0,-1.0,1500.0]});
        assert_eq!(with(&altitude).unwrap_err(), Refusal::InvalidInput);
        let plan = |geometry: &Value| {
            evaluate(
                &serde_json::to_vec(
                    &json!({"schema":"ds.survey-entry-create/v1","operation":"plan",
                    "project_id":"project_a","form_id":"survey_points","doc_id":"entry_1",
                    "idempotency_key":"outbox-1","data":{},"created_at":"2026-08-30T00:00:00Z",
                    "geometry":geometry}),
                )
                .unwrap(),
            )
        };
        assert_eq!(
            plan(&bowtie).unwrap_err(),
            "the Survey entry geometry is not admitted: geometry_invalid: Polygon ring 0 crosses \
             itself: edges 0 and 2 cross at [30.1, -1.9]"
        );
        let inverted = json!({"type":"Polygon","coordinates":[
            [[180.0,90.0],[-180.0,90.0],[-180.0,-90.0],[180.0,-90.0],[180.0,90.0]],
            [[30.0,-2.0],[30.1,-2.0],[30.1,-2.1],[30.0,-2.1],[30.0,-2.0]]]});
        assert!(plan(&inverted)
            .unwrap_err()
            .starts_with("the Survey entry geometry is not admitted: polygon_covers_hemisphere: "));
        assert!(plan(&json!({"type":"Point","coordinates":[30.0,-1.0]})).is_ok());
        assert!(evaluate_geometry(
            &serde_json::to_vec(
                &json!({"schema":"ds.survey-entry-geometry/v1","geometry":altitude})
            )
            .unwrap()
        )
        .unwrap_err()
        .contains("geometry_invalid: Point position has 3 values"));
    }
}
