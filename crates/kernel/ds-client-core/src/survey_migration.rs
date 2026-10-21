//! Project-to-project survey-data migration, headlessly.
//!
//! Migration is stateless: survey entries move FROM `source_project` INTO the
//! destination the caller names, both on every call and neither the saved
//! selection (owner ruling 2026-09-23). The paired-Desktop path that took the
//! destination from the window's active project is gone.
//!
//! The contract is deliberately narrower than the service: copy EVERY entry,
//! preserve the source, never overwrite an existing target id. The body is
//! exactly `action`, `eds_project_id`, `target_project_id` and `dry_run` —
//! what the web's migration dialog sends for that policy. The service's
//! deletion, overwrite and filter keys are never written here, and ds-brain
//! reads each absent one as "no".
//!
//! The path is written here rather than declared in the client profile, like
//! `/api/v1/pm` beside it: declaring a route there would bump
//! `CLIENT_PROFILE_SCHEMA` and strand every installed `ds`, for a route the
//! gateway already publishes on both lanes.
//!
//! **This module is the DOOR, not the decisions.** The receipt a host renders
//! is `ds_command_kernel::survey::migration_receipt`.

use serde_json::{json, Value};

use crate::{
    authorized::{classify_tile_error, Authority},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, ServiceRefusal,
    SurveyMigrationCall,
};

/// The largest answer this door reads: the gateway registry's cap for
/// `POST /api/v1/pipeline`. A receipt is totals and per-form counts.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// The gateway route's own deadline. A migration syncs the whole source
/// before it copies, so it is the long call on this route.
pub const TIMEOUT_SECONDS: u64 = 600;

/// One survey-data migration of every entry in `source_project` into the
/// explicit destination project passed beside it. `dry_run` is the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub source_project: String,
    pub dry_run: bool,
}

const INVALID: &str = "invalid survey migration source or destination project";
const SAME: &str = "survey migration source and destination are the same project";
/// The door's message for a refusal the route authored; the route's status
/// and sentence travel beside it as a [`ServiceRefusal`].
pub const REFUSED: &str = "the survey migration service refused the request";
/// The door's message for an answer that is not a receipt for the mode that
/// was asked for, so a host can name it without guessing at a kind.
pub const UNVERIFIED: &str =
    "the survey migration receipt does not describe the migration that was asked for";

fn unverified() -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, UNVERIFIED)
}

impl Command {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        let valid = ds_command_kernel::execution_context::valid_project;
        if !valid(project) || !valid(&self.source_project) {
            return Err(ClientError::new(ErrorKind::InvalidInput, INVALID));
        }
        if self.source_project == project {
            return Err(ClientError::new(ErrorKind::InvalidInput, SAME));
        }
        Ok(())
    }

    fn payload(&self, project: &str) -> Value {
        json!({
            "action": "migrate",
            "eds_project_id": self.source_project,
            "target_project_id": project,
            "dry_run": self.dry_run,
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
    let response = transport.survey_migration(SurveyMigrationCall {
        profile,
        authority: authority.call(),
        payload: command.payload(project),
    })?;
    if response.status != 200 {
        let body = (response.body.len() <= 64 * 1024)
            .then(|| serde_json::from_slice::<Value>(&response.body).ok())
            .flatten();
        let error = body.as_ref().and_then(|value| value.get("error"));
        // ds-brain's pipeline handler writes `error` as a sentence; its shared
        // middleware writes `{code, message}`. Both reach the caller.
        let message = error
            .and_then(|value| value.get("message").or(Some(value)))
            .and_then(Value::as_str)
            .or_else(|| {
                body.as_ref()
                    .and_then(|value| value.get("message"))
                    .and_then(Value::as_str)
            });
        let code = error
            .and_then(|value| value.get("code"))
            .and_then(Value::as_str);
        return Err(ClientError::with_service_refusal(
            classify_tile_error(response.status).kind(),
            REFUSED,
            ServiceRefusal::new(response.status, code, message),
        ));
    }
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(unverified());
    }
    let parsed: Value = serde_json::from_slice(&response.body).map_err(|_| unverified())?;
    let data = &parsed["data"];
    // The receipt must answer the question asked: a real run reported as a
    // dry run (or the reverse) is not evidence of either. The service does
    // not echo the projects; the request named them and nothing else could.
    if parsed["success"] != true || !data.is_object() || data["dry_run"] != command.dry_run {
        return Err(unverified());
    }
    Ok(data.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(dry_run: bool) -> Command {
        Command {
            source_project: "source_one".into(),
            dry_run,
        }
    }

    #[test]
    fn the_body_names_both_projects_and_nothing_that_deletes_filters_or_overwrites() {
        assert_eq!(
            command(true).payload("target_one"),
            json!({"action":"migrate","eds_project_id":"source_one",
                "target_project_id":"target_one","dry_run":true})
        );
        assert_eq!(command(false).payload("target_one")["dry_run"], false);
    }

    #[test]
    fn a_migration_into_its_own_source_or_a_path_is_refused_before_a_round_trip() {
        assert!(command(true).validate("source_one").is_err());
        assert!(command(true).validate("../target").is_err());
        assert!(command(true).validate("").is_err());
        assert!(command(true).validate("target_one").is_ok());
    }
}
