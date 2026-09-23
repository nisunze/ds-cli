//! Cross-project design migration — ONE endpoint, `kind` transformer|dsgrid.
//!
//! Migration is stateless: the source project, the explicit destination the
//! caller names (`project`), and the objects to move are the operands. The
//! destination is never the saved selection, so one call means the same thing
//! on every machine and in every session (owner ruling 2026-09-23).
//!
//! Plan and apply share one wire shape and one receipt shape, so a plan can be
//! compared with the apply that followed it. The response is verified here
//! rather than trusted: the receipt must be for the kind, mode and projects
//! that were asked about, and a run that moved nothing must say why.
use crate::{
    authorized::{classify_tile_error, Authority},
    AuthorizedTransport, ClientError, ClientProfile, DesignMigrationCall, ErrorKind,
};
use ds_command_kernel::design_migration::{Kind, Mode};
use serde_json::{json, Value};

pub struct Command {
    pub source_project: String,
    pub kind: Kind,
    pub mode: Mode,
    pub items: Vec<String>,
    /// For a transformer this permits replacing the target's head; for a DS
    /// Grid model it permits adding a migrated revision. It never means
    /// "discard the target's lineage".
    pub overwrite_existing: bool,
}

fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && s.trim() == s
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}

fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "invalid design migration source project or object selection",
    )
}

fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the design migration receipt does not describe the migration that was asked for",
    )
}

impl Command {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if !id(project) || !id(&self.source_project) {
            return Err(invalid());
        }
        if self.source_project == project {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "design migration source and target are the same project",
            ));
        }
        ds_command_kernel::design_migration::normalize_items(&self.items)
            .map(|_| ())
            .map_err(|_| invalid())
    }

    fn payload(&self, project: &str) -> Value {
        json!({
            "action": "migrate_design",
            "eds_project_id": self.source_project,
            "target_project_id": project,
            "kind": self.kind.wire(),
            "mode": self.mode.wire(),
            "items": self.items,
            "overwrite_existing": self.overwrite_existing,
        })
    }
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate(project)?;
    let response = transport.design_migration(DesignMigrationCall {
        profile,
        authority: authority.call(),
        payload: command.payload(project),
    })?;
    if response.status != 200 {
        let body = if response.body.len() <= 64 * 1024 {
            serde_json::from_slice::<Value>(&response.body).ok()
        } else {
            None
        };
        let error = body.as_ref().and_then(|v| v.get("error"));
        let message = error
            .and_then(|v| v.get("message"))
            .and_then(Value::as_str)
            .or_else(|| {
                body.as_ref()
                    .and_then(|v| v.get("message"))
                    .and_then(Value::as_str)
            });
        let code = error.and_then(|v| v.get("code")).and_then(Value::as_str);
        return Err(ClientError::with_service_refusal(
            classify_tile_error(response.status).kind(),
            "the design migration service refused the request",
            crate::ServiceRefusal::new(response.status, code, message),
        ));
    }
    if response.body.len() > 8 * 1024 * 1024 {
        return Err(bad());
    }
    let parsed: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if parsed["success"] != true {
        return Err(bad());
    }
    let data = &parsed["data"];
    // The receipt must be about the migration that was asked for. A receipt
    // for another kind, mode or project pair is not evidence, and accepting
    // one is how a confident wrong answer gets rendered.
    if data["kind"] != command.kind.wire()
        || data["mode"] != command.mode.wire()
        || data["source_project_id"] != command.source_project
        || data["target_project_id"] != project
    {
        return Err(bad());
    }
    // A run that moved nothing MUST say why. The server states it; this
    // refuses a receipt that forgot to, rather than rendering a confident
    // "0 migrated".
    let moving = match command.mode {
        Mode::Plan => data["movable"].as_u64().unwrap_or(0),
        Mode::Apply => data["moved"].as_u64().unwrap_or(0),
    };
    if moving == 0 && data["reason"].as_str().unwrap_or("").trim().is_empty() {
        return Err(bad());
    }
    Ok(data.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> Command {
        Command {
            source_project: "source_one".into(),
            kind: Kind::Transformer,
            mode: Mode::Plan,
            items: vec!["TX-1".into()],
            overwrite_existing: false,
        }
    }

    #[test]
    fn a_migration_into_the_project_it_came_from_is_refused() {
        assert!(command().validate("source_one").is_err());
    }

    #[test]
    fn an_empty_selection_is_refused_before_a_round_trip() {
        let mut cmd = command();
        cmd.items = vec!["  ".into()];
        assert!(cmd.validate("target_one").is_err());
    }

    #[test]
    fn the_payload_names_the_kind_the_mode_and_both_projects() {
        let payload = command().payload("target_one");
        assert_eq!(payload["action"], "migrate_design");
        assert_eq!(payload["kind"], "transformer");
        assert_eq!(payload["mode"], "plan");
        assert_eq!(payload["eds_project_id"], "source_one");
        assert_eq!(payload["target_project_id"], "target_one");
    }

    #[test]
    fn the_dsgrid_kind_travels_on_the_same_endpoint() {
        let mut cmd = command();
        cmd.kind = Kind::Dsgrid;
        cmd.mode = Mode::Apply;
        let payload = cmd.payload("target_one");
        assert_eq!(payload["action"], "migrate_design");
        assert_eq!(payload["kind"], "dsgrid");
        assert_eq!(payload["mode"], "apply");
    }
}
