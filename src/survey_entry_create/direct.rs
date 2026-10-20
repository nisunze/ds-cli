//! Fixed Firestore create-only protocol. Hosts supply verified Firebase identity and IO.
use super::*;

pub const ORIGIN: &str = "https://firestore.googleapis.com";
pub const SCHEMA: &str = "ds.survey-direct-create/v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub firebase_project: String,
    pub uid: String,
    pub email: String,
    pub lane: String,
}
impl Authority {
    pub fn validate(&self) -> Result<(), Refusal> {
        if !(6..=63).contains(&self.firebase_project.len())
            || !self
                .firebase_project
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || !self.firebase_project.as_bytes()[0].is_ascii_lowercase()
            || self.firebase_project.ends_with('-')
            || !valid_document_id(&self.uid)
            || self.uid.len() > 128
            || self.email.is_empty()
            || self.email.len() > 320
            || self.email.trim() != self.email
            || self.email.chars().any(char::is_control)
            || !matches!(self.lane.as_str(), "canary" | "stable" | "production")
        {
            return Err(Refusal::InvalidInput);
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
pub struct DirectPlan {
    pub schema: &'static str,
    pub identity: Identity,
    pub authority: Authority,
    pub method: &'static str,
    pub origin: &'static str,
    pub path: String,
    pub document_name: String,
    pub body: Value,
}
fn value(v: &Value) -> Result<Value, Refusal> {
    Ok(match v {
        Value::Null => json!({"nullValue":null}),
        Value::Bool(v) => json!({"booleanValue":v}),
        Value::String(v) => json!({"stringValue":v}),
        Value::Number(v) => {
            if let Some(n) = v.as_i64() {
                json!({"integerValue":n.to_string()})
            } else if v.as_u64().is_some() {
                return Err(Refusal::InvalidInput);
            } else {
                json!({"doubleValue":v.as_f64().filter(|v| v.is_finite()).ok_or(Refusal::InvalidInput)?})
            }
        }
        Value::Array(v) => {
            // Firestore cannot encode an array immediately inside another array.
            if v.iter().any(Value::is_array) {
                return Err(Refusal::InvalidInput);
            }
            json!({"arrayValue":{"values":v.iter().map(value).collect::<Result<Vec<_>,_>>()?}})
        }
        Value::Object(v) => json!({"mapValue":{"fields":fields(v)?}}),
    })
}
fn fields(v: &Map<String, Value>) -> Result<Map<String, Value>, Refusal> {
    v.iter().map(|(k, v)| Ok((k.clone(), value(v)?))).collect()
}
pub fn plan(
    request: &Request,
    project: &str,
    authority: &Authority,
) -> Result<DirectPlan, Refusal> {
    authority.validate()?;
    let admitted = super::plan(request, project)?;
    let database = format!(
        "projects/{}/databases/(default)",
        authority.firebase_project
    );
    let mut document = format!("{database}/documents/eds_project/{project}");
    if let Some(context) = &request.context_key {
        for part in context.split(';') {
            let (collection, id) = part.split_once(':').ok_or(Refusal::InvalidInput)?;
            document.push_str(&format!("/{}/{}", unescape(collection), unescape(id)));
        }
    }
    document.push_str(&format!(
        "/forms/{}/entries/{}",
        request.form, request.doc_id
    ));
    let mut document_fields = Map::new();
    document_fields.insert("data".into(), value(&json!(request.data))?);
    let mut metadata = fields(
        &json!({
            "created_by":authority.email,"created_by_uid":authority.uid,
            "is_deleted":false,"client_version":0,"origin":request.origin.token(),
            "created_via":"direct","idempotency_key":request.idempotency_key
        })
        .as_object()
        .unwrap(),
    )?;
    metadata.insert(
        "created_at".into(),
        json!({"timestampValue":request.created_at}),
    );
    document_fields.insert("metadata".into(), json!({"mapValue":{"fields":metadata}}));
    if let Some(geometry) = &request.geometry {
        document_fields.insert("geometry".into(),json!({"stringValue":serde_json::to_string(geometry).map_err(|_| Refusal::InvalidInput)?}));
    }
    for (key, bucket) in [
        ("connectivity", &request.connectivity),
        ("detailed_location", &request.detailed_location),
    ] {
        if let Some(bucket) = bucket {
            document_fields.insert(key.into(), value(&json!(bucket))?);
        }
    }
    let body = json!({"writes":[{"update":{"name":document,"fields":document_fields},
        "currentDocument":{"exists":false},
        "updateTransforms":[{"fieldPath":"metadata.firestore_updated_at","setToServerValue":"REQUEST_TIME"}]}]});
    if serde_json::to_vec(&body)
        .map_err(|_| Refusal::InvalidInput)?
        .len()
        > MAX_PAYLOAD_BYTES
    {
        return Err(Refusal::InvalidInput);
    }
    Ok(DirectPlan {
        schema: SCHEMA,
        identity: admitted.identity,
        authority: authority.clone(),
        method: "POST",
        origin: ORIGIN,
        path: format!("/v1/{database}/documents:commit"),
        document_name: document,
        body,
    })
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Commit {
    commit_time: String,
    write_results: Vec<WriteResult>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WriteResult {
    update_time: String,
    transform_results: Vec<Timestamp>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Timestamp {
    timestamp_value: String,
}
pub fn receipt(plan: &DirectPlan, status: u16, response: &Value) -> Result<Receipt, Refusal> {
    if status != 200 {
        return Err(Refusal::UnreadableResponse);
    }
    let commit: Commit =
        serde_json::from_value(response.clone()).map_err(|_| Refusal::UnreadableResponse)?;
    if !canonical_utc_rfc3339(&commit.commit_time) || commit.write_results.len() != 1 {
        return Err(Refusal::UnreadableResponse);
    }
    let write = &commit.write_results[0];
    if !canonical_utc_rfc3339(&write.update_time)
        || write.transform_results.len() != 1
        || !canonical_utc_rfc3339(&write.transform_results[0].timestamp_value)
    {
        return Err(Refusal::UnreadableResponse);
    }
    let parse =
        |s: &str| OffsetDateTime::parse(s, &Rfc3339).map_err(|_| Refusal::UnreadableResponse);
    let commit_time = parse(&commit.commit_time)?;
    if parse(&write.update_time)? > commit_time
        || parse(&write.transform_results[0].timestamp_value)? > commit_time
    {
        return Err(Refusal::UnreadableResponse);
    }
    Ok(Receipt {
        project_id: plan.identity.project_id.clone(),
        form_id: plan.identity.form_id.clone(),
        doc_id: plan.identity.doc_id.clone(),
        client_version: 0,
        replication_clock: "metadata.firestore_updated_at".into(),
        replication_authority: "firestore_server_timestamp".into(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn authority() -> Authority {
        Authority {
            firebase_project: "test-project".into(),
            uid: "uid1".into(),
            email: "test@example.invalid".into(),
            lane: "canary".into(),
        }
    }
    fn request() -> Request {
        Request::new(
            "notes",
            "doc1",
            "key1",
            json!({"text":"held","count":2})
                .as_object()
                .unwrap()
                .clone(),
            "2026-10-18T12:00:00Z",
            Origin::Web,
        )
        .unwrap()
    }
    fn response() -> Value {
        json!({"commitTime":"2026-10-18T12:00:01.123456Z","writeResults":[{"updateTime":"2026-10-18T12:00:01.123456Z","transformResults":[{"timestampValue":"2026-10-18T12:00:01.123Z"}]}]})
    }
    #[test]
    fn exact_create_only_document_and_server_clock() {
        let p = plan(&request(), "project1", &authority()).unwrap();
        assert_eq!(p.origin, ORIGIN);
        assert_eq!(
            p.path,
            "/v1/projects/test-project/databases/(default)/documents:commit"
        );
        assert_eq!(
            p.body["writes"][0]["currentDocument"],
            json!({"exists":false})
        );
        let fields = &p.body["writes"][0]["update"]["fields"];
        assert_eq!(
            fields["data"]["mapValue"]["fields"]["count"],
            json!({"integerValue":"2"})
        );
        assert!(fields["metadata"]["mapValue"]["fields"]
            .get("updated_at")
            .is_none());
        assert!(fields["metadata"]["mapValue"]["fields"]
            .get("firestore_updated_at")
            .is_none());
        assert_eq!(
            p.body["writes"][0]["updateTransforms"],
            json!([{"fieldPath":"metadata.firestore_updated_at","setToServerValue":"REQUEST_TIME"}])
        );
        assert!(receipt(&p, 200, &response()).is_ok());
    }
    #[test]
    fn conflict_ambiguous_or_missing_transform_never_settles() {
        let p = plan(&request(), "project1", &authority()).unwrap();
        for status in [0, 201, 409, 500] {
            assert!(receipt(&p, status, &response()).is_err());
        }
        for bad in [
            json!({"error":{"status":"ALREADY_EXISTS"}}),
            json!({"commitTime":"2026-10-18T12:00:01Z","writeResults":[]}),
        ] {
            assert!(receipt(&p, 200, &bad).is_err());
        }
        let mut bad = response();
        bad["writeResults"][0]["transformResults"] = json!([]);
        assert!(receipt(&p, 200, &bad).is_err());
        let mut bad = authority();
        bad.firebase_project = "https://attacker.invalid".into();
        assert!(plan(&request(), "project1", &bad).is_err());
    }
}
