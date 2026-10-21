//! One explicit model read, one captured principal, exactly two owner requests.
use super::{send, verify_graph};
use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};
use ds_command_kernel::project_management::{model_links, overview};
use serde_json::{json, Value};

pub(super) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    model: &str,
    revision: Option<&str>,
    cursor: Option<&str>,
) -> Result<Value, ClientError> {
    let unreadable = |_: String| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "model references do not prove their exact bounded source pair",
        )
    };
    let response = send(
        profile,
        transport,
        authority,
        json!({"action":"get_graph","ds_project":project}),
    )?;
    let graph = response.get("graph").cloned().unwrap_or(response);
    verify_graph(project, &graph)?;
    let options = overview::Options {
        limit: overview::MAX_ROWS,
        records_cursor: cursor.map(str::to_owned),
    };
    let response = send(
        profile,
        transport,
        authority,
        options.context_payload(project),
    )?;
    let context = response.get("context").cloned().unwrap_or(response);
    let mut result = model_links::references(project, model, revision, cursor, &graph, &context)
        .map_err(unreadable)?;
    result["source_authority"] = json!({"uid":authority.uid,"email":authority.email,"lane":profile.lane().token(),"project_id":project});
    Ok(result)
}
