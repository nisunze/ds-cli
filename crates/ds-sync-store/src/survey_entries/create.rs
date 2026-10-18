//! Read-only create preparation and receipt validation over one retained row.
//! No payload supplied by the host replaces the durable mutation bytes.
use super::*;
use ds_command_kernel::survey_entry_create::{evaluate, SURVEY_ENTRY_CREATE_SCHEMA};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub fence: Fence,
    pub row_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_sha256: Option<String>,
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
fn prepare(
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
            direct_sha256: None,
        };
        Ok(json!({"expected":expected,"plan":normalize(&row, &header)?}))
    })?
}

/// Same retained-row fence, with a fixed direct plan bound to the native Firebase principal.
pub fn prepare_direct(store: Option<&Store>, scope: &SurveyEntryScope, project: &str, fence: &Fence,
    authority: &ds_command_kernel::survey_entry_create::direct::Authority) -> Result<Value> {
    if authority.uid != scope.account { return Err(refused("direct create account differs from retained owner")); }
    let mut prepared = prepare(store, scope, project, fence)?;
    let mut request = prepared["plan"]["payload"].clone();
    request["schema"] = json!(SURVEY_ENTRY_CREATE_SCHEMA);
    request["operation"] = json!("direct_plan");
    request["authority"] = serde_json::to_value(authority)?;
    let direct:Value = serde_json::from_str(&evaluate(&serde_json::to_vec(&request)?).map_err(Error::Kernel)?)?;
    prepared["expected"]["direct_sha256"] = json!(sha256_hex(&serde_json::to_vec(&direct["direct"])?));
    prepared["plan"] = direct;
    Ok(prepared)
}
pub fn verify_direct(store: Option<&Store>, scope: &SurveyEntryScope, project: &str, expected: &Expected,
    authority: &ds_command_kernel::survey_entry_create::direct::Authority, status:u16, response:&Value) -> Result<Value> {
    let prepared = prepare_direct(store, scope, project, &expected.fence, authority)?;
    if prepared["expected"] != serde_json::to_value(expected)? { return Err(refused("direct create retained row or authority changed")); }
    let mut request=prepared["plan"]["payload"].clone();
    request["schema"]=json!(SURVEY_ENTRY_CREATE_SCHEMA);request["operation"]=json!("direct_receipt");
    request["authority"]=serde_json::to_value(authority)?;request["status"]=json!(status);request["response"]=response.clone();
    let result:Value=serde_json::from_str(&evaluate(&serde_json::to_vec(&request)?).map_err(Error::Kernel)?)?;
    Ok(json!({"expected":expected,"receipt":result["receipt"]}))
}
