//! Durable publication state machine. Native and browser hosts execute only
//! the returned IO effect, after atomically persisting the returned row.
//! Re-entering `start` after a crash reuses the same run, bytes and sessions.
#[cfg(test)]
mod tests;
mod transition;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Start,
    Bytes {
        output_id: String,
        sha256: String,
        size_bytes: u64,
        #[serde(default)]
        offset: Option<u64>,
    },
    Opened {
        response: Value,
    },
    Probed {
        output_id: String,
        status: String,
        #[serde(default)]
        offset: u64,
    },
    Uploaded {
        output_id: String,
    },
    Finalized {
        response: Value,
    },
}
#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Prepare { output_id: String },
    Open { request: Value },
    Probe { output_id: String },
    VerifyUpload { output_id: String, offset: u64 },
    Upload { output_id: String, offset: u64 },
    Finalize { request: Value },
    Done { state: &'static str },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    owner: String,
    now: u64,
    row: Value,
    event: Event,
    #[serde(default)]
    reopens: u8,
}
#[derive(Debug, Serialize)]
pub struct Reply {
    pub row: Value,
    pub effect: Effect,
    pub reopens: u8,
}
#[derive(Debug, Serialize)]
pub struct Failure {
    pub status: &'static str,
    pub message: String,
}
type Result<T> = std::result::Result<T, Failure>;
fn failed(message: impl Into<String>) -> Failure {
    Failure {
        status: "failed",
        message: message.into(),
    }
}
fn conflict(message: impl Into<String>) -> Failure {
    Failure {
        status: "conflict",
        message: message.into(),
    }
}
fn retry(message: impl Into<String>) -> Failure {
    Failure {
        status: "pending",
        message: message.into(),
    }
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn outputs(row: &Value) -> Result<&Vec<Value>> {
    row["outputs"]
        .as_array()
        .ok_or_else(|| failed("Publication outputs are absent."))
}
fn output_mut<'a>(row: &'a mut Value, id: &str) -> Result<&'a mut Value> {
    row["outputs"]
        .as_array_mut()
        .and_then(|rows| rows.iter_mut().find(|v| text(v, "output_id") == id))
        .ok_or_else(|| failed(format!("Server returned undeclared output {id}.")))
}
fn remove(value: &mut Value, key: &str) {
    if let Some(object) = value.as_object_mut() {
        object.remove(key);
    }
}
fn complete(value: &Value) -> bool {
    matches!(text(value, "state"), "uploaded" | "already_exists")
}
fn declaration(value: &Value) -> Value {
    let mut result = json!({});
    for key in [
        "output_id",
        "format",
        "content_type",
        "sha256",
        "size_bytes",
        "paper_size",
        "presentation",
    ] {
        if let Some(v) = value.get(key) {
            result[key] = v.clone();
        }
    }
    result
}
fn validate_row(row: &Value, owner: &str) -> Result<()> {
    if owner.is_empty()
        || text(row, "owner_uid") != owner
        || text(row, "id").is_empty()
        || text(row, "project_id").is_empty()
        || text(row, "client_run_id").is_empty()
        || !matches!(text(row, "engine"), "network_reporter" | "solar")
    {
        return Err(failed(
            "Publication has no matching account, project or engine-owned reader.",
        ));
    }
    let values = outputs(row)?;
    let mut ids = std::collections::BTreeSet::new();
    if values.is_empty()
        || values.len() > 100
        || values
            .iter()
            .any(|v| text(v, "output_id").is_empty() || !ids.insert(text(v, "output_id")))
    {
        return Err(failed("Publication output inventory is invalid."));
    }
    if row["sync_archived_at"].as_u64().unwrap_or(0) != 0 {
        return Err(failed("Publication is retained in history."));
    }
    Ok(())
}
fn start(row: &mut Value) -> Result<()> {
    if !matches!(text(row, "status"), "pending" | "syncing") {
        return Err(failed(
            "Publication requires explicit recovery before retry.",
        ));
    }
    if text(row, "engine") == "network_reporter" {
        let publish = text(row, "client_publish_id");
        let uuid = regex::Regex::new(
            r"^publish-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$",
        )
        .expect("constant regex");
        if !hex(text(row, "batch_id"))
            || !uuid.is_match(publish)
            || !crate::report::valid_network_reporter_engine_identity(text(row, "engine_version"))
        {
            return Err(failed("Network Reporter publication has no exact native batch receipt. Run the export again; its local bytes were preserved."));
        }
    }
    if !hex(text(row, "engine_build_manifest_sha256")) {
        return Err(failed("This publication predates exact engine-build provenance. Recompute it with the current release."));
    }
    row["status"] = json!("syncing");
    row["attempts"] = json!(row["attempts"].as_u64().unwrap_or(0).saturating_add(1));
    row["error"] = Value::Null;
    remove(row, "failure_code");
    Ok(())
}
fn opening(row: &mut Value) -> Result<Effect> {
    row["phase"] = json!("opening");
    let mut request = json!({});
    for key in [
        "project_id",
        "client_run_id",
        "engine",
        "operation",
        "variant",
        "engine_version",
        "engine_build_manifest_sha256",
        "input_base_fingerprint",
        "source_snapshot_sha256",
        "snapshot_receipt_id",
    ] {
        if let Some(value) = row.get(key) {
            request[key] = value.clone();
        }
    }
    request["outputs"] = json!(outputs(row)?.iter().map(declaration).collect::<Vec<_>>());
    Ok(Effect::Open { request })
}
fn next(row: &mut Value) -> Result<Effect> {
    if let Some(id) = outputs(row)?
        .iter()
        .find(|v| text(v, "state") == "preparing")
        .map(|v| text(v, "output_id").to_string())
    {
        row["phase"] = json!("preparing");
        return Ok(Effect::Prepare { output_id: id });
    }
    if text(row, "work_id").is_empty()
        || outputs(row)?
            .iter()
            .any(|v| !complete(v) && text(v, "session_uri").is_empty())
    {
        return opening(row);
    }
    if let Some(id) = outputs(row)?
        .iter()
        .find(|v| !complete(v))
        .map(|v| text(v, "output_id").to_string())
    {
        row["phase"] = json!("uploading");
        return Ok(Effect::Probe { output_id: id });
    }
    row["phase"] = json!("finalizing");
    Ok(Effect::Finalize {
        request: json!({"project_id":row["project_id"],"work_id":row["work_id"],"client_run_id":row["client_run_id"]}),
    })
}
pub fn evaluate(input: &[u8]) -> std::result::Result<String, String> {
    if input.is_empty() || input.len() > 2 * 1024 * 1024 {
        return Err("Sync transition exceeds its byte bound.".into());
    }
    let request: Request =
        serde_json::from_slice(input).map_err(|_| "Invalid sync transition request.")?;
    let result = apply(request);
    serde_json::to_string(&match result {
        Ok(reply) => json!({"ok":true,"data":reply}),
        Err(error) => json!({"ok":false,"error":error}),
    })
    .map_err(|e| e.to_string())
}
fn apply(mut request: Request) -> Result<Reply> {
    validate_row(&request.row, &request.owner)?;
    if request.now > 9_007_199_254_740_991 {
        return Err(failed("Invalid sync clock."));
    }
    let effect = match request.event {
        Event::Start => {
            start(&mut request.row)?;
            request.reopens = 0;
            next(&mut request.row)?
        }
        event => {
            if text(&request.row, "status") != "syncing" {
                return Err(failed("Sync event requires an active publication."));
            }
            transition::apply(&mut request.row, event, &mut request.reopens, request.now)?
        }
    };
    request.row["updated_at"] = json!(request.now);
    Ok(Reply {
        row: request.row,
        effect,
        reopens: request.reopens,
    })
}
