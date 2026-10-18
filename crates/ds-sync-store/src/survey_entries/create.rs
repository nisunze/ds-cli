//! Read-only create preparation and receipt validation over one retained row.
//! No payload supplied by the host replaces the durable mutation bytes.
use super::*;
use ds_command_kernel::survey_entry_create::{evaluate, SURVEY_ENTRY_CREATE_SCHEMA};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub fence: Fence,
    pub row_sha256: String,
}

fn retained(
    store: &Store,
    scope: &SurveyEntryScope,
    project: &str,
    fence: &Fence,
) -> Result<(Value, Header)> {
    scope.validate()?;
    if !text(project, 1500)
        || !text(&fence.id, 4096)
        || fence.owner_uid.as_deref() != Some(scope.account.as_str())
    {
        return Err(refused(
            "create preparation requires the exact account, project and row fence",
        ));
    }
    let (raw, header) = read_row(&store.connection, scope, &fence.id)?
        .ok_or_else(|| refused("create preparation row is not retained"))?;
    if header.project_id != project
        || !matches(&header, fence)
        || header.operation != kernel::Operation::Create
    {
        return Err(refused(
            "create preparation row changed its project, operation or lifecycle fence",
        ));
    }
    Ok((serde_json::from_str(&raw)?, header))
}

fn normalize(row: &Value, header: &Header) -> Result<Value> {
    let mut request = row["payload"].clone();
    request["schema"] = json!(SURVEY_ENTRY_CREATE_SCHEMA);
    request["operation"] = json!("normalize");
    request["row"] = json!({"project_id":header.project_id,"form_id":header.form_id,
        "doc_id":header.doc_id,"idempotency_key":header.idempotency_key,"created_at":header.created_at});
    let raw = evaluate(&serde_json::to_vec(&request)?).map_err(Error::Kernel)?;
    Ok(serde_json::from_str(&raw)?)
}

/// The full-row digest and lifecycle fence bind later receipt validation.
/// A failed row can be inspected without retry, queue writes, or transport.
pub fn prepare(
    store: Option<&Store>,
    scope: &SurveyEntryScope,
    project: &str,
    fence: &Fence,
) -> Result<Value> {
    let store = store.ok_or_else(|| refused("create preparation row is not retained"))?;
    store.with_read_snapshot(|store| {
        let (row, header) = retained(store, scope, project, fence)?;
        let expected = Expected {
            fence: Fence::from(&header),
            row_sha256: sha256_hex(&serde_json::to_vec(&row)?),
        };
        Ok(json!({"expected":expected,"plan":normalize(&row, &header)?}))
    })?
}

/// Verification is also read-only. Only the existing result transition can
/// settle a queue row, after the exact service receipt has been admitted.
pub fn verify_receipt(
    store: Option<&Store>,
    scope: &SurveyEntryScope,
    project: &str,
    expected: &Expected,
    status: u16,
    response: &Value,
) -> Result<Value> {
    let store = store.ok_or_else(|| refused("create receipt row is not retained"))?;
    store.with_read_snapshot(|store| {
        let (row, header) = retained(store, scope, project, &expected.fence)?;
        if expected.row_sha256 != sha256_hex(&serde_json::to_vec(&row)?) {
            return Err(refused(
                "create receipt row differs from its prepared digest",
            ));
        }
        let mut request = normalize(&row, &header)?["payload"].clone();
        request["schema"] = json!(SURVEY_ENTRY_CREATE_SCHEMA);
        request["operation"] = json!("receipt");
        request["status"] = json!(status);
        request["response"] = response.clone();
        let raw = evaluate(&serde_json::to_vec(&request)?).map_err(Error::Kernel)?;
        let result: Value = serde_json::from_str(&raw)?;
        Ok(json!({"expected":expected,"receipt":result["receipt"]}))
    })?
}
