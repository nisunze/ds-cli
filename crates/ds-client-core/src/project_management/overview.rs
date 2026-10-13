//! One deliberate overview acquisition under one captured native authority.
//! No personal notes, directory fallback, graph retry, polling or hidden page walk.
use super::send;
use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};
use ds_command_kernel::project_management::overview::{self, Input, Options};
use serde_json::{json, Value};

pub(super) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    options: &Options,
) -> Result<Value, ClientError> {
    let invalid = |_: String| {
        ClientError::new(
            ErrorKind::InvalidInput,
            "project overview options are invalid",
        )
    };
    let unreadable = |_: String| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "project overview source is not an exact bounded graph/context pair",
        )
    };
    options.validate().map_err(invalid)?;
    let graph_response = send(
        profile,
        transport,
        authority,
        json!({"action":"get_graph","ds_project":project}),
    )?;
    let graph = graph_response
        .get("graph")
        .cloned()
        .unwrap_or(graph_response);
    overview::validate_graph(&graph, project, "wire").map_err(unreadable)?;
    let context_response = send(
        profile,
        transport,
        authority,
        options.context_payload(project),
    )?;
    let context = context_response
        .get("context")
        .cloned()
        .unwrap_or(context_response);
    let overview = overview::fold(Input {
        schema: ds_command_kernel::project_management::SCHEMA.into(),
        action: "overview".into(),
        ds_project: project.into(),
        graph,
        context,
        graph_format: "wire".into(),
        options: options.clone(),
    })
    .map_err(unreadable)?;
    let mut result = serde_json::to_value(overview).map_err(|e| unreadable(e.to_string()))?;
    result["source_authority"] = json!({"uid":authority.uid,"email":authority.email,"lane":profile.lane().token(),"project_id":project});
    Ok(result)
}
