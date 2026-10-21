//! Governance of one project model that needs no upload: starting the next
//! version from the head's exact bytes, renaming or re-describing the head,
//! recording a review decision on an existing revision, and reading the
//! retirement backup. Each write is fenced on the head the caller reviewed;
//! each answer is checked to be about exactly the model and revision asked.
use super::*;

fn text(value: &str, bound: usize) -> bool {
    !value.is_empty()
        && value.len() <= bound
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn optional(value: &Option<String>, bound: usize) -> bool {
    value.as_deref().is_none_or(|v| text(v, bound))
}

/// Local validation with the Server's own bounds.
pub(super) fn validate(command: &Command) -> Result<(), ClientError> {
    let valid = match command {
        Command::BumpVersion {
            model,
            expected_head,
            reason,
            milestone,
        } => {
            id(model)
                && catalog_identifier(expected_head)
                && text(reason, 2000)
                && optional(milestone, MAX_MILESTONE)
        }
        Command::UpdateModel {
            model,
            expected_head,
            display_name,
            description,
            reason,
        } => {
            id(model)
                && catalog_identifier(expected_head)
                && (display_name.is_some() || description.is_some())
                && optional(display_name, 200)
                // An explicit empty description clears it.
                && description
                    .as_deref()
                    .is_none_or(|d| d.is_empty() || text(d, 2000))
                && optional(reason, 2000)
        }
        Command::SetApproval {
            model,
            revision,
            expected_head,
            approval,
        } => {
            id(model)
                && catalog_identifier(revision)
                && catalog_identifier(expected_head)
                && APPROVAL_STATES.contains(&approval.status.as_str())
                && approval
                    .decision_reason
                    .as_deref()
                    .is_some_and(|r| text(r, 2000))
                && approval.level_id.as_deref().is_none_or(catalog_identifier)
                && (!matches!(approval.status.as_str(), "approved" | "rejected")
                    || approval.level_id.is_some())
        }
        Command::DownloadBackup { model } => id(model),
        _ => unreachable!("governance commands only"),
    };
    if valid {
        Ok(())
    } else {
        Err(ClientError::new(
            ErrorKind::InvalidInput,
            "invalid model governance request: ids match ^[a-z0-9][a-z0-9_-]{1,127}$, a reason is 1..2000 trimmed characters, a name 1..200, a milestone at most 300, and a decision names its level",
        ))
    }
}

pub(super) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Receipt, ClientError> {
    let data = match command {
        Command::BumpVersion {
            model,
            expected_head,
            reason,
            milestone,
        } => {
            let mut payload = json!({"action":"bump_version","project_id":project,"model_id":model,
                "expected_head_revision_id":expected_head,"reason":reason});
            if let Some(milestone) = milestone {
                payload["milestone"] = json!(milestone);
            }
            let data = request(profile, transport, authority, payload)?;
            let version = data.get("version").ok_or_else(bad)?;
            // The new version's first revision re-pins the head it was cut
            // from: same bytes, parent = that head, ordinal 1.
            if version["project_id"] != project
                || version["model_id"] != *model
                || version["parent_revision_id"] != *expected_head
                || !version["revision_id"]
                    .as_str()
                    .is_some_and(catalog_identifier)
                || !version["version"].as_u64().is_some_and(|v| v > 1)
                || version["revision_ordinal_within_version"]
                    .as_u64()
                    .is_some_and(|n| n != 1)
                || milestone
                    .as_deref()
                    .is_some_and(|m| version["milestone"] != m)
            {
                return Err(bad());
            }
            json!({"status":"version_started","project":project,"model_id":model,
                "version":version["version"],"revision_id":version["revision_id"],
                "revision_ordinal_within_version":1,"parent_revision_id":expected_head,
                "model_digest":version["model"]["digest"],"milestone":version["milestone"],
                "reason":version["reason"],"approval":version["approval"],"content_changed":false})
        }
        Command::UpdateModel {
            model,
            expected_head,
            display_name,
            description,
            reason,
        } => {
            let mut payload = json!({"action":"update_model","project_id":project,"model_id":model,
                "expected_head_revision_id":expected_head});
            for (key, value) in [
                ("display_name", display_name),
                ("description", description),
                ("reason", reason),
            ] {
                if let Some(value) = value {
                    payload[key] = json!(value);
                }
            }
            let data = request(profile, transport, authority, payload)?;
            let head = data.get("model").ok_or_else(bad)?;
            if head["project_id"] != project
                || head["model_id"] != *model
                || head["head_revision_id"] != *expected_head
                || !data["changed"].is_boolean()
                || display_name
                    .as_deref()
                    .is_some_and(|name| head["display_name"] != name)
            {
                return Err(bad());
            }
            json!({"project":project,"model":head_projection(head),"changed":data["changed"],
                "changes":data["changes"],"revisions_changed":false})
        }
        Command::SetApproval {
            model,
            revision,
            expected_head,
            approval,
        } => {
            let data = request(
                profile,
                transport,
                authority,
                json!({"action":"set_approval","project_id":project,"model_id":model,
                    "revision_id":revision,"expected_head_revision_id":expected_head,
                    "approval":approval}),
            )?;
            if data["project_id"] != project
                || data["model_id"] != *model
                || data["revision_id"] != *revision
                || data["effective_approval"]["status"] != approval.status
                || !data["changed"].is_boolean()
            {
                return Err(bad());
            }
            let mut answer = data.clone();
            answer["project"] = json!(project);
            answer
        }
        Command::DownloadBackup { model } => {
            return backup(profile, transport, authority, project, model);
        }
        _ => unreachable!("governance commands only"),
    };
    Ok(Receipt { data, bytes: None })
}

/// The retirement backup: a generation-pinned object in the backup bucket
/// whose digest must be the head's. The URL may name only that bucket,
/// object and generation; the bytes must be exactly that digest.
fn backup<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    model: &str,
) -> Result<Receipt, ClientError> {
    let data = request(
        profile,
        transport,
        authority,
        json!({"action":"get_backup_artifact","project_id":project,"model_id":model}),
    )?;
    let backup = &data["backup"];
    let digest = data["digest"]
        .as_str()
        .filter(|d| sha256_hex(d))
        .ok_or_else(bad)?;
    let size = data["byte_length"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= MAX_BYTES as u64)
        .ok_or_else(bad)? as usize;
    let bucket = backup["bucket"]
        .as_str()
        .filter(|b| id(b))
        .ok_or_else(bad)?;
    let object = backup["object"].as_str().ok_or_else(bad)?;
    let generation = backup["generation"]
        .as_i64()
        .filter(|g| *g > 0)
        .ok_or_else(bad)?;
    if data["project_id"] != project
        || data["model_id"] != model
        || backup["digest"] != digest
        || data["head_model_digest"] != digest
        || backup["byte_length"].as_u64() != Some(size as u64)
        || object
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(bad());
    }
    let url_text = data["download_url"].as_str().ok_or_else(bad)?;
    let url = url::Url::parse(url_text).map_err(|_| bad())?;
    if url_text.len() > 8192
        || url.scheme() != "https"
        || url.host_str() != Some("storage.googleapis.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path() != format!("/{bucket}/{object}")
        || !url
            .query_pairs()
            .any(|(k, v)| k == "generation" && v == generation.to_string())
    {
        return Err(bad());
    }
    let mut response = transport.grid_model_bytes(DownloadCall {
        url: url_text,
        size,
    })?;
    if response.status != 200
        || response.body.len() != size
        || format!("{:x}", Sha256::digest(&response.body)) != digest
    {
        return Err(bad());
    }
    Ok(Receipt {
        data: json!({"project":project,"model_id":model,"state":data["state"],
            "head_revision_id":data["head_revision_id"],"sha256":digest,"bytes":size,
            "generation":generation,"verified":true}),
        bytes: Some(std::mem::take(&mut response.body)),
    })
}
