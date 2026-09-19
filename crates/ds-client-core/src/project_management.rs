//! Project management, headlessly.
//!
//! The plan, its tasks, milestones, notes and records were reachable from
//! exactly one place: a browser window. All nine `ds pm` commands declared
//! `Requires::Window` and relayed through the desktop bridge to the page's own
//! adapters — so an operator on a server, or an agent with no display, could
//! not read a project's plan at all.
//!
//! Nothing about these reads needs a window. `POST /api/v1/pm` is already
//! published on both gateway lanes with its full action vocabulary, and
//! ds-brain's PM handler authenticates from the bearer with no pairing and no
//! device. The window was habit, not necessity.
//!
//! **This module is the DOOR, not the decisions.** It sends a closed set of
//! actions and returns what the server said. What a graph MEANS — the
//! dashboard, the phases, the attention list, the vocabulary — belongs to
//! `ds_command_kernel::project_management`, so the CLI, the Server and the
//! browser fold one answer rather than three.
//!
//! The path is written here rather than declared in the client profile, like
//! `/api/v1/assets` and `/api/v1/governance` beside it. The profile supplies
//! the gateway origin and the credentials; declaring a route there would bump
//! `CLIENT_PROFILE_SCHEMA` and strand every installed `ds` until a package is
//! cut, for nothing this needs.
//!
//! Closed by construction, like every other governed door in this crate: the
//! [`Command`] enum is the complete set of actions this owner can send, and
//! ds-brain decodes with `DisallowUnknownFields`, so a key this file does not
//! write is a key the request cannot carry.

use serde_json::{json, Value};

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ProjectManagementCall,
};

/// The largest plan answer this owner will read. A project's whole graph —
/// tasks, milestones, records, residuals — is bigger than an inventory page
/// and smaller than a design room.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// One governed project-management action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// The project's whole canonical graph, at its current revision.
    ///
    /// There is no narrower read: ds-brain answers the graph whole, and every
    /// projection — the plan, a task list, one task — is a fold over it. A
    /// per-task endpoint would be a second answer that could disagree with
    /// this one.
    Graph,
    /// The context rows the graph does not carry: records, work episodes and
    /// attachments.
    ///
    /// These are a SEPARATE read on purpose, and the browser never made it:
    /// `decodeProjectGraph` hard-codes `records: []`, so `ds pm record list`
    /// answers nothing however many records a project holds. A window does not
    /// fix that; asking for them does.
    Context,
}

impl Command {
    fn action(&self) -> &'static str {
        match self {
            Self::Graph => "get_graph",
            Self::Context => "get_context",
        }
    }
}

fn refused() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "project management did not answer in a shape this build understands",
    )
}

/// The project a read is for, or the refusal an operator can act on.
///
/// Separate from [`run`] so it is testable without a profile and a transport:
/// a request that cannot succeed should not cost a round trip, and the remedy
/// is local.
fn readable_project(project: &str) -> Result<&str, ClientError> {
    let trimmed = project.trim();
    if trimmed.is_empty() {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "project management needs the project it is reading",
        ));
    }
    Ok(trimmed)
}

/// Send one governed action for one project and return its `data`.
pub fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    let project = readable_project(project)?;
    let response = transport.project_management(ProjectManagementCall {
        profile,
        authority: authority.call(),
        payload: json!({ "action": command.action(), "ds_project": project }),
    })?;
    if response.status != 200 {
        return Err(crate::authorized::classify_tile_error(response.status));
    }
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(refused());
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|_| refused())?;
    if body["success"] != true {
        return Err(refused());
    }
    let data = body.get("data").cloned().unwrap_or(Value::Null);
    if data.is_null() {
        return Err(refused());
    }
    // ds-brain answers `{success, data}`, and the graph sits under `data.graph`
    // when it is wrapped and IS the data when it is not. The browser reads it
    // the same way (`response.graph ?? response`, `api/pm.ts:119-121`).
    //
    // Reading only the envelope is not a loud failure — it is an EMPTY PLAN
    // that looks valid: revision 0, no tasks, every permission false. That is
    // indistinguishable from a project with no plan, which is exactly the
    // class of ambiguity this campaign exists to remove, so the unwrap is
    // explicit rather than incidental.
    let graph = match command {
        Command::Graph => data
            .get("graph")
            .filter(|graph| !graph.is_null())
            .cloned()
            .unwrap_or(data),
        Command::Context => data,
    };
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_action_vocabulary_is_closed_and_named() {
        assert_eq!(Command::Graph.action(), "get_graph");
        assert_eq!(Command::Context.action(), "get_context");
    }

    /// A projectless call is refused before anything is sent, and the project
    /// is trimmed rather than sent with the whitespace an operator pasted.
    #[test]
    fn a_blank_project_is_refused_and_a_padded_one_is_trimmed() {
        for blank in ["", "   ", "\t\n"] {
            let error = readable_project(blank).expect_err("a blank project is refused");
            assert_eq!(error.kind(), ErrorKind::InvalidInput);
        }
        assert_eq!(
            readable_project("  arjgpydw_nyam  ").expect("a padded project still reads"),
            "arjgpydw_nyam",
        );
    }
}
