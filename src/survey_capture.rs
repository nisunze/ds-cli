//! Web survey capture: the form a person fills and the queued entry it
//! becomes. The session holds the form, the values, the location and the
//! network connectivity the person gave, answers what the window shows
//! (visible fields, errors, the accuracy and precedent rules) and plans the
//! submission: validation against the pinned form revision, the media paths
//! and uploads, the create intent and its admission to the survey queue.
//! Hosts perform IO: file bytes, photo processing, storage and transport.
//! Every survey rule here is [`crate::survey`]'s; this module composes them.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::survey::survey_evaluate;

/// A picked file in a media field's value: its token names the bytes the
/// host keeps until the submission plans the upload.
pub const FILE: &str = "__ds_file";
/// The most files one session holds.
pub const MAX_FILES: usize = 64;

/// What opening a form names.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub project: String,
    /// The resolved form: `slug`, `fields`, `version` and its
    /// `entry_document_schema`.
    pub form: Value,
    #[serde(default)]
    pub geometry: Value,
    #[serde(default)]
    pub location: Value,
    /// `gps` or `picked`.
    #[serde(default)]
    pub location_source: Option<String>,
    #[serde(default)]
    pub connectivity: Value,
}

/// One form being filled.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Session {
    pub open: bool,
    /// The account that opened it; another account's session never submits.
    pub owner: String,
    pub project: String,
    pub form: Value,
    pub slug: String,
    pub revision: String,
    pub values: Map<String, Value>,
    pub errors: Map<String, Value>,
    pub geometry: Value,
    pub location: Value,
    pub location_source: Option<String>,
    pub connectivity: Value,
    /// The node picked as this entry's precedent: `{docId, formSlug, coordinates}`.
    pub precedent: Value,
    pub submitting: bool,
    pub submitted: bool,
    pub error: Option<String>,
    pub restored_draft: bool,
    pub incompatible_draft: bool,
}

fn evaluate(request: Value) -> Result<Value, String> {
    survey_evaluate(&serde_json::to_vec(&request).map_err(|e| e.to_string())?)
}

fn text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// A media value's file markers: `(token, name)`.
fn files(value: &Value) -> Vec<(String, String)> {
    let items = match value {
        Value::Array(items) => items.iter().collect(),
        item => vec![item],
    };
    items
        .into_iter()
        .filter_map(|item| {
            let token = item.get(FILE)?.as_str()?;
            Some((token.to_owned(), text(&item["name"])))
        })
        .collect()
}

/// What the window shows of a form being filled.
pub fn view(session: &Session) -> Value {
    if !session.open {
        return json!({"open": false});
    }
    let fields = session.form["fields"].clone();
    let visible = evaluate(json!({"operation": "fields", "fields": fields, "values": session.values, "validate": false}))
        .ok()
        .and_then(|result| result["visible_keys"].as_array().cloned())
        .unwrap_or_default();
    let visible_fields: Vec<Value> = fields
        .as_array()
        .map(|all| {
            all.iter()
                .filter(|field| visible.contains(&field["key"]))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let schema = &session.form["entry_document_schema"];
    let rules = &schema["connectivity"];
    let chained = session.connectivity["precedentDocId"].as_str().is_some_and(|id| !id.is_empty());
    let precedent = evaluate(json!({"operation": "policy", "mode": "precedent", "connectivity": rules, "isChainedEdge": chained}))
        .unwrap_or(json!({"showPicker": false, "required": false}));
    let enforce = schema["enforce_accuracy"] == true;
    let required_accuracy = schema["required_accuracy_m"].as_f64().unwrap_or(0.0);
    let picked = session.location_source.as_deref() == Some("picked");
    let accuracy_met = evaluate(json!({"operation": "policy", "mode": "accuracy", "input": {
        "enforceAccuracy": enforce, "requiredAccuracyM": required_accuracy, "locationPicked": picked,
        "hasGeometryWithoutLocation": !session.geometry.is_null() && session.location.is_null(),
        "accuracyM": session.location["accuracy"],
    }}))
    .ok()
    .and_then(|met| met.as_bool())
    .unwrap_or(false);
    let here = evaluate(json!({"operation": "policy", "mode": "coordinates", "geometry": session.geometry, "location": session.location}))
        .ok()
        .and_then(|point| Some((point[0].as_f64()?, point[1].as_f64()?)));
    let there = session.precedent["coordinates"]
        .as_array()
        .and_then(|point| Some((point.first()?.as_f64()?, point.get(1)?.as_f64()?)));
    let distance = here
        .zip(there)
        .map(|((lng, lat), (p_lng, p_lat))| ds_geo_lite::haversine_m(p_lng, p_lat, lng, lat));
    let max_distance = rules["max_span_distance_m"].as_f64().unwrap_or(0.0);
    let distance_ok = max_distance <= 0.0 || distance.is_none_or(|d| d <= max_distance);
    let precedent_met = precedent["required"] != true || !session.precedent.is_null();
    let edge_forms = rules["qualifying_edge_forms"].clone();
    json!({
        "open": true,
        "project": session.project,
        "slug": session.slug,
        "display_name": session.form["display_name"],
        "fields": visible_fields,
        "values": session.values,
        "errors": session.errors,
        "location": session.location,
        "geometry": session.geometry,
        "location_source": session.location_source,
        "connectivity": session.connectivity,
        "precedent": session.precedent,
        "submitting": session.submitting,
        "submitted": session.submitted,
        "error": session.error,
        "restored_draft": session.restored_draft,
        "incompatible_draft": session.incompatible_draft,
        "rules": {
            "enforce_accuracy": enforce,
            "required_accuracy_m": required_accuracy,
            "accuracy_met": accuracy_met,
            "show_precedent_picker": precedent["showPicker"],
            "precedent_required": precedent["required"],
            "precedent_met": precedent_met,
            "precedent_distance_m": distance,
            "max_span_distance_m": max_distance,
            "distance_ok": distance_ok,
            "qualifying_edge_forms": if edge_forms.is_array() { edge_forms } else { json!([]) },
            "qualifying_precedent_forms": if rules["qualifying_precedent_forms"].is_array() { rules["qualifying_precedent_forms"].clone() } else { json!([]) },
        },
        "can_submit": !session.submitting && accuracy_met && precedent_met && distance_ok,
    })
}

impl Session {
    /// Open a form for `owner`: its defaults, its pinned revision and the
    /// location and connectivity the opening named.
    pub fn open(owner: &str, context: Context) -> Result<Self, String> {
        let slug = context.form["slug"]
            .as_str()
            .filter(|slug| !slug.is_empty())
            .ok_or("The form names no slug")?
            .to_owned();
        if owner.is_empty() || context.project.trim().is_empty() {
            return Err("Open a project before filling a form".into());
        }
        let schema = &context.form["entry_document_schema"];
        let revision = evaluate(json!({"operation": "fill", "mode": "revision",
            "version": context.form["version"], "entry_schema_version": schema["schema_version"]}))?;
        let values = evaluate(json!({"operation": "defaults", "fields": context.form["fields"]}))?
            .as_object()
            .cloned()
            .unwrap_or_default();
        Ok(Self {
            open: true,
            owner: owner.into(),
            project: context.project.trim().into(),
            slug,
            revision: text(&revision),
            values,
            geometry: context.geometry,
            location: context.location,
            location_source: context.location_source,
            connectivity: context.connectivity,
            form: context.form,
            ..Self::default()
        })
    }

    /// Set one field's value; its error clears.
    pub fn set_value(&mut self, key: &str, value: Value) {
        self.values.insert(key.into(), value);
        self.errors.remove(key);
    }

    /// The device's location, or a point picked on the map.
    pub fn locate(&mut self, location: Value, source: &str) {
        if let (Some(lng), Some(lat)) = (location["longitude"].as_f64(), location["latitude"].as_f64()) {
            self.geometry = json!({"type": "Point", "coordinates": [lng, lat]});
        }
        self.location = location;
        self.location_source = Some(source.into());
    }

    /// The draft that keeps this form across reloads: values without picked
    /// files, which a reload cannot keep.
    pub fn draft(&self, now_ms: u64) -> Result<Value, String> {
        let values = evaluate(json!({"operation": "fill", "mode": "draft_values",
            "fields": self.form["fields"], "values": self.marked_values()}))?;
        Ok(json!({"owner_uid": self.owner, "ds_project": self.project, "form_slug": self.slug,
            "form_revision": self.revision, "values": values, "geometry": self.geometry,
            "location": self.location, "location_picked": self.location_source.as_deref() == Some("picked"),
            "connectivity": self.connectivity, "saved_at": now_ms}))
    }

    /// Restore a held draft over the defaults, only when it was authored
    /// against this exact form revision. Context the opening named wins.
    pub fn restore(&mut self, draft: &Value) {
        if draft["owner_uid"].as_str() != Some(self.owner.as_str()) {
            return;
        }
        let admission = evaluate(json!({"operation": "fill", "mode": "draft_admission",
            "form_revision": self.revision, "draft_revision": draft["form_revision"]}))
        .unwrap_or_default();
        if admission["incompatible"] == true {
            self.incompatible_draft = true;
            return;
        }
        let Some(values) = draft["values"].as_object().filter(|values| !values.is_empty()) else {
            return;
        };
        if admission["restore"] != true {
            return;
        }
        for (key, value) in values {
            self.values.insert(key.clone(), value.clone());
        }
        if self.geometry.is_null() && !draft["geometry"].is_null() {
            self.geometry = draft["geometry"].clone();
        }
        if self.location.is_null() && !draft["location"].is_null() {
            self.location = draft["location"].clone();
            if draft["location_picked"] == true {
                self.location_source = Some("picked".into());
            }
        }
        if self.connectivity.is_null() && draft["connectivity"].is_object() {
            self.connectivity = draft["connectivity"].clone();
        }
        self.restored_draft = true;
    }

    /// Values with every picked file as the host-binary marker the survey
    /// rules already know.
    fn marked_values(&self) -> Value {
        let mut out = Map::new();
        for (key, value) in &self.values {
            let marked = match value {
                Value::Array(items) => Value::Array(items.iter().map(marker).collect()),
                item => marker(item),
            };
            out.insert(key.clone(), marked);
        }
        Value::Object(out)
    }

    /// Every file token the values still name.
    pub fn file_tokens(&self) -> Vec<String> {
        self.values.values().flat_map(files).map(|(token, _)| token).collect()
    }
}

fn marker(item: &Value) -> Value {
    if item.get(FILE).is_some() {
        json!({"__ds_survey_browser_binary__": true})
    } else {
        item.clone()
    }
}

/// What one submission needs from its host.
#[derive(Clone, Debug, PartialEq)]
pub struct Submit {
    pub now_ms: u64,
    /// The new entry's id, `e_` and 32 hex digits.
    pub doc_id: String,
    /// The intent's idempotency key, `survey_` and a UUID: the queue row's id.
    pub mutation_id: String,
    /// The signed-in account submitting; a create is written as it.
    pub uid: String,
    pub email: String,
    /// The account's entry grants (`entries.create`, …).
    pub grants: Value,
}

/// One upload the submission plans: the picked file's token, its storage
/// path and whether it is a photo (processed and thumbnailed).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Upload {
    pub token: String,
    pub path: String,
    pub photo: bool,
}

/// A planned submission: the uploads to queue, the survey queue row that
/// carries the create, and the feature the window shows until it is
/// delivered.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Planned {
    pub uploads: Vec<Upload>,
    pub row: Value,
    pub feature: Value,
}

/// Why a submission did not plan.
#[derive(Clone, Debug, PartialEq)]
pub enum Refused {
    /// Field errors, shown beside their fields.
    Invalid(Map<String, Value>),
    /// The session cannot submit, with the reason a person is shown.
    Unavailable(String),
}

/// The survey queue record of one entry: its steps (the create, then every
/// correction) leave in the order they were queued. A digest, so any form
/// slug and document id fit the queue's token bound.
pub fn record(form: &str, doc: &str) -> String {
    crate::durable_store::sha256(format!("{form}\n{doc}").as_bytes())[..40].to_owned()
}

/// One survey queue row: the step's wire payload and the feature it shows
/// until it is delivered.
#[allow(clippy::too_many_arguments)]
fn queue_row(project: &str, form: &str, doc: &str, operation: &str, id: &str, wire: &Value, overlay: &Value, submit: &Submit) -> Value {
    json!({
        "project": project, "id": id, "record": record(form, doc),
        "meta": {"operation": operation, "form": form, "doc": doc},
        "payload": {"operation": operation, "form": form, "doc": doc, "idempotency_key": id,
            "created_at": submit.now_ms, "uid": submit.uid, "wire": wire, "overlay": overlay},
    })
}

fn iso(now_ms: u64) -> Result<String, String> {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(now_ms) * 1_000_000)
        .ok()
        .and_then(|at| at.format(&time::format_description::well_known::Rfc3339).ok())
        .ok_or_else(|| "The device clock is unreadable".into())
}

/// Plan the submission of the session's entry.
pub fn plan(session: &Session, submit: &Submit) -> Result<Planned, Refused> {
    let unavailable = |message: String| Refused::Unavailable(message);
    if !session.open || session.owner.is_empty() {
        return Err(unavailable("Open a project before creating an entry".into()));
    }
    if session.owner != submit.uid {
        return Err(unavailable("This form was opened by another account; open it again".into()));
    }
    let schema = &session.form["entry_document_schema"];
    if !schema.is_object() {
        return Err(unavailable("Entry schema not available; open the project again".into()));
    }
    let checked = evaluate(json!({"operation": "fill", "mode": "submission", "form_revision": session.revision,
        "fields": session.form["fields"], "values": session.marked_values(), "entry_document_schema": schema,
        "location": session.location, "connectivity": connectivity(session)}))
    .map_err(unavailable)?;
    if let Some(errors) = checked["errors"].as_object().filter(|errors| !errors.is_empty()) {
        return Err(Refused::Invalid(errors.clone()));
    }
    let visible: Vec<&str> = checked["visible_keys"]
        .as_array()
        .map(|keys| keys.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let fields: Vec<Value> = session.form["fields"]
        .as_array()
        .map(|all| all.iter().filter(|field| field["key"].as_str().is_some_and(|key| visible.contains(&key))).cloned().collect())
        .unwrap_or_default();
    let mut data = Map::new();
    let mut media = Map::new();
    for (key, value) in &session.values {
        if !visible.contains(&key.as_str()) {
            continue;
        }
        data.insert(key.clone(), value.clone());
        let items = match value {
            Value::Array(items) => items.clone(),
            item => vec![item.clone()],
        };
        let existing: Vec<&Value> = items.iter().filter(|item| item.as_str().is_some_and(|path| !path.is_empty())).collect();
        let picked: Vec<Value> = files(value)
            .into_iter()
            .map(|(token, name)| json!({"token": token, "identity": token, "name": name}))
            .collect();
        media.insert(key.clone(), json!({"existing_paths": existing, "files": picked}));
    }
    let media_plan = evaluate(json!({"operation": "fill", "mode": "media_plan", "fields": fields, "media": media,
        "project_id": session.project, "form_slug": session.slug, "document_id": submit.doc_id, "now_ms": submit.now_ms}))
    .map_err(unavailable)?;
    for field in media_plan["fields"].as_array().into_iter().flatten() {
        let Some(key) = field["key"].as_str() else { continue };
        data.remove(key);
        if field["present"] == true {
            data.insert(key.into(), field["value"].clone());
        }
    }
    let uploads: Vec<Upload> = media_plan["uploads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|upload| {
            Some(Upload {
                token: upload["token"].as_str()?.into(),
                path: upload["path"].as_str()?.into(),
                photo: upload["photo"] == true,
            })
        })
        .collect();
    let identity = json!({"project_id": session.project, "form_id": session.slug, "doc_id": submit.doc_id});
    let captured = evaluate(json!({"operation": "mutation", "mode": "capture", "now": iso(submit.now_ms).map_err(unavailable)?,
        "mutation_id": submit.mutation_id, "user_email": submit.email, "grants": submit.grants, "identity": identity,
        "params": {"dsProject": session.project, "formSlug": session.slug, "data": data,
            "entryDocumentSchema": schema, "geometry": session.geometry, "location": session.location,
            "connectivity": checked["connectivity"], "detailedLocation": checked["detailed_location"],
            "docId": submit.doc_id}}))
    .map_err(unavailable)?;
    let feature = captured["feature"].clone();
    Ok(Planned {
        uploads,
        row: queue_row(&session.project, &session.slug, &submit.doc_id, "create", &submit.mutation_id,
            &captured["payload"], &feature, submit),
        feature,
    })
}

/// The connectivity a submission records: the opening's, with a picked
/// precedent and its form and coordinates.
fn connectivity(session: &Session) -> Value {
    let mut connectivity = session.connectivity.as_object().cloned().unwrap_or_default();
    if let Some(doc) = session.precedent["docId"].as_str() {
        connectivity.insert("precedentDocId".into(), doc.into());
        connectivity.insert("precedentFormId".into(), session.precedent["formSlug"].clone());
        connectivity.insert("precedentCoordinates".into(), session.precedent["coordinates"].clone());
    }
    if connectivity.is_empty() {
        Value::Null
    } else {
        Value::Object(connectivity)
    }
}

/// One correction, deletion or restore of an existing entry, planned against
/// the row this device holds of it (`prior`): the queue row and the feature
/// the window shows until it is delivered.
pub fn plan_mutation(payload: &Value, prior: &Value, submit: &Submit) -> Result<(Value, Value), String> {
    let identity = evaluate(json!({"operation": "mutation", "mode": "identity", "payload": payload}))?;
    let planned = evaluate(json!({"operation": "mutation", "mode": "mutate", "payload": payload, "prior": prior,
        "mutation_id": submit.mutation_id, "now": iso(submit.now_ms)?, "grants": submit.grants, "user_email": submit.email}))?;
    let (project, form, doc, operation) = (
        text(&identity["project_id"]),
        text(&identity["form_id"]),
        text(&identity["doc_id"]),
        text(&identity["operation"]),
    );
    let wire = &planned["payload"];
    let id = wire["idempotency_key"].as_str().unwrap_or(&submit.mutation_id).to_owned();
    let row = queue_row(&project, &form, &doc, &operation, &id, wire, &planned["feature"], submit);
    Ok((row, planned["feature"].clone()))
}

/// A node deletion and the edges it cascades to, planned against the held
/// rows that may reference it: one queued deletion per entry, each showing
/// its tombstone until delivered.
pub fn plan_cascade(
    project: &str,
    form: &str,
    doc: &str,
    prior: &Value,
    candidates: &Value,
    options: &Value,
    priors: impl Fn(&str, &str) -> Value,
    submit: &Submit,
) -> Result<Value, String> {
    let plan = evaluate(json!({"operation": "mutation", "mode": "delete_cascade", "project_id": project, "form_id": form,
        "doc_id": doc, "prior": prior, "rows": candidates, "mutation_id": submit.mutation_id, "now": iso(submit.now_ms)?,
        "grants": submit.grants, "edges_only": options["edges_only"] == true, "match": options["match"]}))?;
    let mut rows = Vec::new();
    let mut edges = Vec::new();
    for (index, intent) in plan["intents"].as_array().into_iter().flatten().enumerate() {
        let identity = &intent["identity"];
        let (form_id, doc_id) = (text(&identity["form_id"]), text(&identity["doc_id"]));
        let overlay = evaluate(json!({"operation": "mutation", "mode": "patch", "prior": priors(&form_id, &doc_id),
            "doc_id": doc_id, "deleted": true}))?;
        let key = text(&intent["payload"]["idempotency_key"]);
        rows.push(queue_row(&text(&identity["project_id"]), &form_id, &doc_id, "delete", &key, &intent["payload"], &overlay, submit));
        if index > 0 || options["edges_only"] == true {
            edges.push(json!({"formSlug": form_id, "docId": doc_id}));
        }
    }
    Ok(json!({"rows": rows, "deleted": plan["deleted"], "cascaded_edges": plan["cascaded_edges"], "edges": edges}))
}

#[cfg(test)]
#[path = "survey_capture_tests.rs"]
mod tests;
