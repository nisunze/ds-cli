//! A closed native replay of one fully validated held-row cascade plan.
//! Backend deletes are individual operations, never a remote transaction.
use crate::transport::{AuthorizedTransport, CallAuthority, SecretRequestBody};
use crate::{ClientError, ClientProfile, ErrorKind, TransportResponse};
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub struct SurveyDeleteRequest {
    project: String,
    intents: Vec<Value>,
    cascaded_edges: usize,
}
impl SurveyDeleteRequest {
    /// All caller facts are validated together, before profile or auth access.
    /// Synthetic preview grants permit planning only; the server authorizes
    /// every actual delete against the captured native principal.
    pub fn new(
        project: &str,
        form: &str,
        doc: &str,
        key: &str,
        now: &str,
        document: Value,
    ) -> Result<Self, ClientError> {
        if !document
            .as_object()
            .is_some_and(|o| o.keys().all(|k| matches!(k.as_str(), "prior" | "rows")))
        {
            return Err(invalid());
        }
        let input = json!({"operation":"mutation","mode":"delete_cascade_preview","project_id":project,
            "form_id":form,"doc_id":doc,"mutation_id":key,"now":now,"prior":document["prior"],"rows":document["rows"]});
        let plan = ds_command_kernel::survey::survey_evaluate(
            &serde_json::to_vec(&input).map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?;
        if plan["project_id"] != project {
            return Err(invalid());
        }
        Ok(Self {
            project: project.into(),
            intents: plan["intents"].as_array().ok_or_else(invalid)?.clone(),
            cascaded_edges: plan["cascaded_edges"].as_u64().ok_or_else(invalid)? as usize,
        })
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn targets(&self) -> usize {
        self.intents.len()
    }
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "invalid bounded Survey delete plan",
    )
}

/// Privileged transport input. No caller can select another operation/body.
pub struct SurveyDeleteCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: &'a Value,
    pub(crate) authority: CallAuthority<'a>,
}
impl SurveyDeleteCall<'_> {
    fn route(&self) -> &'static crate::GatewayOperation {
        crate::gateway_operation("domains.survey_entries.delete")
            .expect("generated Survey delete route")
    }
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> &'static str {
        self.route().path
    }
    pub fn method(&self) -> &'static str {
        self.route().method
    }
    pub fn response_limit(&self) -> usize {
        self.route().response_cap_bytes as usize
    }
    pub fn timeout_seconds(&self) -> u64 {
        u64::from(self.route().timeout_s)
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
    }
    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody::new(serde_json::to_vec(self.payload).expect("validated delete payload"))
    }
}

fn receipt(response: &TransportResponse, intent: &Value) -> Option<Value> {
    if response.status != 200 || response.body.len() > 65536 {
        return None;
    }
    let value: Value = serde_json::from_slice(&response.body).ok()?;
    if value["success"] != true
        || !value["error"].is_null()
        || value["service"] != "data-solutions-backend"
        || value["version"] != "1.0.0"
        || OffsetDateTime::parse(value["timestamp"].as_str()?, &Rfc3339).is_err()
        || value["data"]["operation"] != "delete"
        || value["data"]["doc_id"] != intent["identity"]["doc_id"]
    {
        return None;
    }
    let version = value["data"]["client_version"]
        .as_u64()
        .filter(|v| *v > 0 && *v <= 9_007_199_254_740_991);
    if !value["data"]["client_version"].is_null() && version.is_none() {
        return None;
    }
    let state = if let Some(v) = version {
        if intent["payload"]["expected_version"]
            .as_u64()
            .is_some_and(|prior| prior.checked_add(1) != Some(v))
        {
            return None;
        }
        OffsetDateTime::parse(value["data"]["deleted_at"].as_str()?, &Rfc3339).ok()?;
        "deleted"
    } else if value["message"] == "Entry already absent" && value["data"]["idempotent"] == true {
        "already_absent"
    } else if value["message"] == "Entry already deleted" {
        "already_deleted"
    } else {
        return None;
    };
    Some(
        json!({"form_id":intent["identity"]["form_id"],"doc_id":intent["identity"]["doc_id"],
        "state":state,"client_version":version,"bigquery":"unconfirmed"}),
    )
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: crate::authorized::Authority<'_>,
    project: &str,
    request: &SurveyDeleteRequest,
) -> Result<Value, ClientError> {
    if request.project != project {
        return Err(invalid());
    }
    Ok(replay(request, |payload| {
        transport.survey_delete(SurveyDeleteCall {
            profile,
            payload,
            authority: authority.call(),
        })
    }))
}
fn replay(
    request: &SurveyDeleteRequest,
    mut dispatch: impl FnMut(&Value) -> Result<TransportResponse, crate::TransportError>,
) -> Value {
    let project = &request.project;
    let mut receipts = Vec::new();
    let mut failure = Value::Null;
    for (index, intent) in request.intents.iter().enumerate() {
        let response = dispatch(&intent["payload"]);
        let accepted = response.as_ref().ok().and_then(|r| receipt(r, intent));
        if let Some(accepted) = accepted {
            receipts.push(accepted);
        } else {
            failure = json!({"index":index,"form_id":intent["identity"]["form_id"],"doc_id":intent["identity"]["doc_id"],
                "status":response.as_ref().ok().map(|r| r.status),
                "code":if response.is_err() {"transport_ambiguous"} else if response.as_ref().unwrap().status == 409 {"conflict"}
                    else if matches!(response.as_ref().unwrap().status,401|403) {"authority_refused"}
                    else if response.as_ref().unwrap().status == 200 {"receipt_ambiguous"} else {"service_refused"}});
            break;
        }
    }
    json!({"schema":"ds.survey-delete-replay/v1","project_id":project,"scope":"held_rows","remote_atomic":false,
        "targets":request.targets(),"cascaded_edges":request.cascaded_edges,"acknowledged":receipts.len(),
        "complete":failure.is_null(),"receipts":receipts,"failure":failure})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_stops_at_first_conflict_and_preserves_confirmed_prefix() {
        let doc = json!({"rows":[{"form_slug":"lines","feature":{"id":"e","geometry":{"type":"LineString"},"properties":{"connectivity":{"precedent":"n"}}}}]});
        let request =
            SurveyDeleteRequest::new("p", "poles", "n", "k", "2026-10-02T00:00:00Z", doc).unwrap();
        let mut calls = Vec::new();
        let result = replay(&request, |payload| {
            calls.push(payload.clone());
            if calls.len() == 1 {
                Ok(TransportResponse::new(200, serde_json::to_vec(&json!({"success":true,
                "service":"data-solutions-backend","version":"1.0.0","timestamp":"2026-10-02T00:00:00Z",
                "message":"Entry already absent","data":{"operation":"delete","doc_id":"n","idempotent":true}})).unwrap()))
            } else {
                Ok(TransportResponse::new(409, b"{}".to_vec()))
            }
        });
        assert_eq!(result["acknowledged"], 1);
        assert_eq!(result["complete"], false);
        assert_eq!(result["failure"]["code"], "conflict");
        assert_eq!(result["receipts"][0]["doc_id"], "n");
        assert_eq!(calls[0]["project_id"], "p");
        assert_eq!(calls[1]["idempotency_key"], "k:1");
        let mut count = 0;
        let result = replay(&request, |_| {
            count += 1;
            Err(crate::TransportError::Unreachable)
        });
        assert_eq!(count, 1);
        assert_eq!(result["acknowledged"], 0);
        assert_eq!(result["failure"]["code"], "transport_ambiguous");
    }
    #[test]
    fn receipt_requires_the_exact_target_and_acknowledges_noop_without_inventing_a_version() {
        let intent =
            json!({"identity":{"form_id":"poles","doc_id":"n"},"payload":{"expected_version":2}});
        let mut v = json!({"success":true,"service":"data-solutions-backend","version":"1.0.0","timestamp":"2026-10-02T00:00:00Z",
            "data":{"doc_id":"n","operation":"delete","client_version":3,"deleted_at":"2026-10-02T00:00:00Z"}});
        let decode = |v: &Value| {
            receipt(
                &TransportResponse::new(200, serde_json::to_vec(v).unwrap()),
                &intent,
            )
        };
        assert_eq!(decode(&v).unwrap()["state"], "deleted");
        v["data"]["doc_id"] = json!("other");
        assert!(decode(&v).is_none());
        v["data"] = json!({"doc_id":"n","operation":"delete","idempotent":true});
        v["message"] = json!("Entry already absent");
        assert!(decode(&v).unwrap()["client_version"].is_null());
        v["data"]["client_version"] = json!("invalid");
        assert!(decode(&v).is_none());
        v["data"]["client_version"] = Value::Null;
        v["message"] = json!("unknown");
        assert!(decode(&v).is_none());
    }
    #[test]
    fn native_request_uses_the_full_kernel_plan_and_rejects_invalid_scope_before_auth() {
        let doc = json!({"rows":[{"form_slug":"lines","feature":{"id":"e","geometry":{"type":"LineString"},"properties":{"connectivity":{"precedent":"n"}}}}]});
        assert_eq!(
            SurveyDeleteRequest::new("p", "poles", "n", "k", "2026-10-02T00:00:00Z", doc.clone())
                .unwrap()
                .targets(),
            2
        );
        let mut doc = doc;
        doc["rows"][0]["project_id"] = json!("other");
        assert!(
            SurveyDeleteRequest::new("p", "poles", "n", "k", "2026-10-02T00:00:00Z", doc).is_err()
        );
    }
}
