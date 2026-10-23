//! Typed, data-only correction of one retained Survey entry.
//! The caller supplies a held mirror row and form revision; no data is fetched
//! or authority invented by planning. The backend owns the version fence.
use crate::transport::{AuthorizedTransport, CallAuthority, SecretRequestBody};
use crate::{ClientError, ClientProfile, ErrorKind, TransportResponse};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: String,
    prior: Value,
    changes: Map<String, Value>,
    fields: Vec<Value>,
    form_revision: String,
    expected_version: u64,
    #[serde(default)]
    context_key: String,
}

pub struct SurveyUpdateRequest {
    project: String,
    payload: Value,
    preview: Value,
}
impl SurveyUpdateRequest {
    /// Validate the closed retained document before profile or auth access.
    /// Only declared data fields can change; geometry and capture metadata are
    /// never part of the wire payload. Local form validity is not a cloud fence.
    pub fn new(
        project: &str,
        form: &str,
        doc: &str,
        key: &str,
        now: &str,
        document: Value,
    ) -> Result<Self, ClientError> {
        if serde_json::to_vec(&document).map_err(|_| invalid())?.len() > 1024 * 1024 {
            return Err(invalid());
        }
        let held: Document = serde_json::from_value(document).map_err(|_| invalid())?;
        let clock = OffsetDateTime::parse(now, &Rfc3339).map_err(|_| invalid())?;
        if held.schema != "ds.survey-entry-update/v1"
            || held.changes.is_empty()
            || held.prior["type"] != "Feature"
            || !held.prior["properties"]["data"].is_object()
            || held.expected_version >= 9_007_199_254_740_991
            || key.trim() != key
            || key.is_empty()
            || key.len() > 512
            || key.chars().any(char::is_control)
            || held.context_key.len() > 2048
            || held.context_key.chars().any(char::is_control)
            || clock.unix_timestamp() < 0
        {
            return Err(invalid());
        }
        let props = &held.prior["properties"];
        let identities = [held.prior.get("id"), props.get("id"), props.get("doc_id")];
        if !identities.iter().any(Option::is_some)
            || identities.into_iter().flatten().any(|value| value != doc)
        {
            return Err(invalid());
        }

        for (name, expected) in [
            ("project_id", project),
            ("form_slug", form),
            ("doc_id", doc),
        ] {
            if props.get(name).is_some_and(|value| value != expected) {
                return Err(invalid());
            }
        }
        if props["metadata"]["is_deleted"] == true || props["is_deleted"] == true {
            return Err(invalid());
        }
        let prior_version = match props["metadata"].get("client_version") {
            None | Some(Value::Null) => 0,
            Some(value) => value.as_u64().ok_or_else(invalid)?,
        };
        if held.expected_version != prior_version
            || held.changes.iter().any(|(name, value)| {
                name.is_empty()
                    || name.contains('.')
                    || name.contains('/')
                    || name.chars().any(char::is_control)
                    || !held.fields.iter().any(|field| field["key"] == *name)
                    || props["data"].get(name) == Some(value)
            })
        {
            return Err(invalid());
        }
        let feature = evaluate(
            json!({"operation":"mutation","mode":"patch","prior":held.prior,
            "doc_id":doc,"patch":{"updatedData":held.changes},"deleted":false}),
        )?;
        let validation = ds_command_kernel::survey::preview::validate(
            &serde_json::to_vec(&json!({
                "schema":"ds.survey-form-validation/v1","form_revision":held.form_revision,
                "fields":held.fields,"values":feature["properties"]["data"]
            }))
            .map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?;
        if validation["valid"] != true {
            return Err(invalid());
        }
        let payload = evaluate(json!({"operation":"mutation","mode":"normalize","row":{
            "project_id":project,"form_id":form,"doc_id":doc,"operation":"update","idempotency_key":key,
            "payload":{"updated_data":held.changes,"context_key":held.context_key,
                "expected_version":held.expected_version,"queued_at":clock.unix_timestamp(),
                "metadata":{"edit_at":now,"origin":"cli"}}
        }}))?;
        let preview = json!({"schema":"ds.survey-entry-update-plan/v1","project_id":project,"form_id":form,"doc_id":doc,
            "expected_version":held.expected_version,"changed_fields":held.changes.keys().collect::<Vec<_>>(),
            "form_revision":held.form_revision,"form_validation":validation,
            "scope":"retained_data_fields","effects":"none","authorized":false,
            "geometry_preserved":true,"capture_metadata_preserved":true,"cloud_freshness":"unconfirmed"});
        Ok(Self {
            project: project.into(),
            payload,
            preview,
        })
    }
    /// A photo rotation: the one system-owned `ds_media_rotation` data key
    /// (`survey_media_rotation`), under the entry version the person saw. No
    /// form field changes; geometry and capture metadata are never sent.
    pub fn media_rotation(
        project: &str,
        form: &str,
        doc: &str,
        key: &str,
        now: &str,
        expected_version: u64,
        updated_data: &Value,
    ) -> Result<Self, ClientError> {
        let clock = OffsetDateTime::parse(now, &Rfc3339).map_err(|_| invalid())?;
        let only_rotation = updated_data.as_object().is_some_and(|changes| {
            changes.len() == 1
                && changes
                    .get(ds_command_kernel::survey_media_rotation::KEY)
                    .is_some_and(Value::is_object)
        });
        if !only_rotation
            || expected_version >= 9_007_199_254_740_991
            || key.trim() != key
            || key.is_empty()
            || key.len() > 512
            || key.chars().any(char::is_control)
            || clock.unix_timestamp() < 0
        {
            return Err(invalid());
        }
        let payload = evaluate(json!({"operation":"mutation","mode":"normalize","row":{
            "project_id":project,"form_id":form,"doc_id":doc,"operation":"update","idempotency_key":key,
            "payload":{"updated_data":updated_data,"expected_version":expected_version,
                "queued_at":clock.unix_timestamp(),"metadata":{"edit_at":now,"origin":"cli"}}
        }}))?;
        let preview = json!({"schema":"ds.survey-entry-update-plan/v1","project_id":project,"form_id":form,"doc_id":doc,
            "expected_version":expected_version,"changed_fields":[ds_command_kernel::survey_media_rotation::KEY],
            "scope":"media_rotation","effects":"none","authorized":false,
            "geometry_preserved":true,"capture_metadata_preserved":true,"cloud_freshness":"unconfirmed"});
        Ok(Self {
            project: project.into(),
            payload,
            preview,
        })
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn preview(&self) -> Value {
        self.preview.clone()
    }
}
fn evaluate(value: Value) -> Result<Value, ClientError> {
    ds_command_kernel::survey::survey_evaluate(&serde_json::to_vec(&value).map_err(|_| invalid())?)
        .map_err(|_| invalid())
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "invalid bounded Survey data correction",
    )
}

/// A fixed governed update call, not an arbitrary route or body adapter.
pub struct SurveyUpdateCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: &'a Value,
    pub(crate) authority: CallAuthority<'a>,
}
impl SurveyUpdateCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::MutateEntryPut;

    fn route(&self) -> &'static crate::GatewayOperation {
        crate::gateway_operation("domains.survey_entries.update")
            .expect("generated Survey update route")
    }
    crate::gateway_compat::gateway_common_accessors! {
        gateway_origin: normal,
        gateway_api_key: normal,
        client_id: normal,
        bearer_token: normal,
        canonical_email: normal,
    }
    pub fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }
    pub fn response_limit(&self) -> usize {
        self.route().response_cap_bytes as usize
    }
    pub fn timeout_seconds(&self) -> u64 {
        u64::from(self.route().timeout_s)
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody::new(
            serde_json::to_vec(self.payload).expect("validated Survey update payload"),
        )
    }
}
fn receipt(response: &TransportResponse, request: &SurveyUpdateRequest) -> Option<Value> {
    if response.status != 200 || response.body.len() > 65536 {
        return None;
    }
    let value: Value = serde_json::from_slice(&response.body).ok()?;
    let version = value["data"]["client_version"].as_u64()?;
    if value["success"] != true
        || !value["error"].is_null()
        || !crate::survey_delete::brain_acknowledged(&value)
        || value["data"]["operation"] != "update"
        || value["data"]["doc_id"] != request.payload["doc_id"]
        || request.payload["expected_version"].as_u64()?.checked_add(1) != Some(version)
        || OffsetDateTime::parse(value["timestamp"].as_str()?, &Rfc3339).is_err()
        || OffsetDateTime::parse(value["data"]["updated_at"].as_str()?, &Rfc3339).is_err()
    {
        return None;
    }
    Some(
        json!({"form_id":request.payload["form_id"],"doc_id":request.payload["doc_id"],"state":"committed",
        "client_version":version,"updated_at":value["data"]["updated_at"],"bigquery":"unconfirmed"}),
    )
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: crate::authorized::Authority<'_>,
    project: &str,
    request: &SurveyUpdateRequest,
) -> Result<Value, ClientError> {
    if request.project != project {
        return Err(invalid());
    }
    Ok(dispatch(request, || {
        transport.survey_update(SurveyUpdateCall {
            profile,
            payload: &request.payload,
            authority: authority.call(),
        })
    }))
}
fn dispatch(
    request: &SurveyUpdateRequest,
    send: impl FnOnce() -> Result<TransportResponse, crate::TransportError>,
) -> Value {
    let response = send();
    let accepted = response.as_ref().ok().and_then(|r| receipt(r, request));
    let failure = if accepted.is_some() {
        Value::Null
    } else {
        let code = match &response {
            Err(_) => "transport_ambiguous",
            Ok(r) => match r.status {
                409 => "conflict",
                401 | 403 => "authority_refused",
                404 => "not_found",
                200 => "receipt_ambiguous",
                _ => "service_refused",
            },
        };
        json!({"code":code,"status":response.as_ref().ok().map(|r|r.status)})
    };
    json!({"schema":"ds.survey-entry-update-receipt/v1","project_id":request.project,
        "form_id":request.payload["form_id"],"doc_id":request.payload["doc_id"],
        "complete":accepted.is_some(),"receipt":accepted,"failure":failure,"bigquery":"unconfirmed"})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document() -> Value {
        json!({"schema":"ds.survey-entry-update/v1","form_revision":"17","expected_version":2,
        "fields":[{"key":"category","type":"text","required":true},{"key":"type","type":"text","required":true}],
        "changes":{"type":"Church"},"prior":{"type":"Feature","id":"n","geometry":{"type":"Point","coordinates":[29.8,-2.5]},
        "properties":{"data":{"name":"original","category":"Productive","type":"Other","photos":["retained-photo"]},
            "metadata":{"captured_at":"2026-10-10T09:00:00Z","client_version":2},"form_slug":"customers","doc_id":"n"}}})
    }
    fn request() -> SurveyUpdateRequest {
        SurveyUpdateRequest::new(
            "p",
            "customers",
            "n",
            "stable-key",
            "2026-10-12T00:00:00Z",
            document(),
        )
        .unwrap()
    }
    #[test]
    fn data_only_request_preserves_capture_and_rejects_stale_or_retargeted_document() {
        let r = request();
        let mut mirrored = document();
        mirrored["prior"].as_object_mut().unwrap().remove("id");
        mirrored["prior"]["properties"]["id"] = json!("n");
        assert!(SurveyUpdateRequest::new(
            "p",
            "customers",
            "n",
            "stable-key",
            "2026-10-12T00:00:00Z",
            mirrored
        )
        .is_ok());
        assert_eq!(r.payload["updated_data"], json!({"type":"Church"}));
        for key in [
            "updated_geometry",
            "updated_connectivity",
            "updated_detailed_location",
            "data",
        ] {
            assert!(r.payload.get(key).is_none());
        }
        assert_eq!(
            r.payload["metadata"],
            json!({"edit_at":"2026-10-12T00:00:00Z","origin":"cli"})
        );
        let text = r.preview().to_string();
        assert!(
            !text.contains("retained-photo")
                && !text.contains("original")
                && !text.contains("stable-key")
        );
        for kind in 0..5 {
            let mut d = document();
            match kind {
                0 => d["expected_version"] = json!(1),
                1 => d["prior"]["id"] = json!("other"),
                2 => d["geometry"] = json!(null),
                3 => d["changes"] = json!({"undeclared":1}),
                _ => d["prior"]["properties"]["metadata"]["is_deleted"] = json!(true),
            }
            assert!(SurveyUpdateRequest::new(
                "p",
                "customers",
                "n",
                "key",
                "2026-10-12T00:00:00Z",
                d
            )
            .is_err());
        }
    }
    #[test]
    fn dispatch_is_single_attempt_never_creates_and_requires_exact_versioned_receipt() {
        let r = request();
        let mut calls = 0;
        let v = dispatch(&r, || {
            calls += 1;
            Ok(TransportResponse::new(409, b"{}".to_vec()))
        });
        assert_eq!(calls, 1);
        assert_eq!(v["failure"]["code"], "conflict");
        assert_eq!(v["complete"], false);
        let ack = json!({"success":true,"service":"data-solutions-backend","version":"1.0.0","timestamp":"2026-10-12T00:00:01Z", "data":{"operation":"update","doc_id":"n","client_version":3,"updated_at":"2026-10-12T00:00:01Z"}});
        let decode = |v: &Value| {
            receipt(
                &TransportResponse::new(200, serde_json::to_vec(v).unwrap()),
                &r,
            )
        };
        assert_eq!(decode(&ack).unwrap()["state"], "committed");
        for key in ["doc_id", "operation", "client_version", "updated_at"] {
            let mut bad = ack.clone();
            bad["data"][key] = json!("incorrect");
            assert!(decode(&bad).is_none());
        }
        let v = dispatch(&r, || Err(crate::TransportError::Unreachable));
        assert_eq!(v["failure"]["code"], "transport_ambiguous");
    }
}
