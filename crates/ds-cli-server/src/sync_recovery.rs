//! Owner-bound transport for the kernel's common publication recovery controls.
use axum::{
    Json,
    body::Bytes,
    extract::State,
    response::{IntoResponse, Response},
};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Command, Effect, Execution, Refusal},
};
use ds_command_kernel::sync_store::{
    Fence, Scope,
    recovery::{self, Control},
};
use serde_json::Value;

const PROJECT: Arg = super::PROJECT.required();
const LIMIT: Arg = Arg::value(
    "limit",
    "<count>",
    "Bounded display page (1..200); the digest always covers the complete project snapshot.",
)
.default("50");
const ROW: Arg = Arg::value(
    "row",
    "<exact-id>",
    "Exact retained publication id returned by status.",
)
.required();
const DIGEST: Arg = Arg::value(
    "digest",
    "<sha256>",
    "Exact complete snapshot digest returned by sanitation preview.",
)
.required();
const REFUSALS: &[Refusal] = &[
    super::PLATFORM,
    super::REFUSED,
    super::OWNER_CHANGED,
    super::PROJECT_REQUIRED,
    super::CONTEXT_CORRUPT,
    Refusal {
        code: "sync_invalid_input",
        when: "the recovery JSON, display limit, row or digest is invalid",
        remedy: "use the declared controls and exact values returned by status or preview",
    },
    Refusal {
        code: "sync_worker_active",
        when: "a live publication worker holds the project's lease",
        remedy: "wait for that worker to release its lease, then inspect and apply again",
    },
    Refusal {
        code: "sync_row_not_found",
        when: "the exact standing row is absent from this owner fence and project",
        remedy: "read server sync status under the intended account and project",
    },
    Refusal {
        code: "sync_row_not_retryable",
        when: "the row has no recoverable typed authority verdict or readable immutable bytes",
        remedy: "follow the retained verdict; stale, integrity and permission failures cannot be retried by this control",
    },
    Refusal {
        code: "sync_sanitation_refused",
        when: "the complete retained snapshot changed after preview",
        remedy: "preview sanitation again and apply its exact new digest",
    },
];

pub static STATUS: Command = super::command(
    "server.sync.status",
    &["server", "sync", "status"],
    "Inspect native retained publications under one explicit project.",
    Effect::ReadOnly,
    Execution::Sync,
    &[super::STATE, super::LANE, PROJECT, LIMIT],
    REFUSALS,
    &[],
);
pub static RETRY: Command = super::command(
    "server.sync.retry",
    &["server", "sync", "retry"],
    "Explicitly retry one recoverable retained publication (needs --yes).",
    Effect::GlobalWrite,
    Execution::Sync,
    &[super::STATE, super::LANE, PROJECT, ROW],
    REFUSALS,
    &[],
);
pub static PREVIEW: Command = super::command(
    "server.sync.sanitize.preview",
    &["server", "sync", "sanitize", "preview"],
    "Preview digest-bound native queue recovery and history archival.",
    Effect::ReadOnly,
    Execution::Sync,
    &[super::STATE, super::LANE, PROJECT, LIMIT],
    REFUSALS,
    &[],
);
pub static APPLY: Command = super::command(
    "server.sync.sanitize.apply",
    &["server", "sync", "sanitize", "apply"],
    "Apply inspected native sanitation while retaining files (needs --yes).",
    Effect::GlobalWrite,
    Execution::Sync,
    &[super::STATE, super::LANE, PROJECT, DIGEST],
    REFUSALS,
    &[],
);

fn invalid(reason: impl ToString) -> Failure {
    Failure::invalid("sync_invalid_input", reason.to_string()).remedy(REFUSALS[5].remedy)
}

fn send(inputs: &Inputs, command: recovery::Command) -> Result<Value, Failure> {
    let control = Control {
        schema: recovery::SCHEMA.into(),
        project: super::bounded_project(inputs.require("project")?)?,
        command,
    };
    control.validate().map_err(invalid)?;
    let body = serde_json::to_vec(&control).map_err(invalid)?;
    let result = super::json_request(inputs, "POST", "/v1/sync-recovery", Some(&body))?;
    if let Some(refusal) = result
        .get("refusals")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
    {
        let code = refusal["code"].as_str().unwrap_or("server_refused");
        let detail = refusal["detail"]
            .as_str()
            .unwrap_or("Native recovery refused");
        let remedy = REFUSALS
            .iter()
            .find(|known| known.code == code)
            .map(|known| known.remedy)
            .unwrap_or(super::REFUSED.remedy);
        let failure = match code {
            "sync_worker_active"
            | "sync_row_not_found"
            | "sync_row_not_retryable"
            | "sync_sanitation_refused" => Failure::conflict(code.to_owned(), detail.to_owned()),
            _ => Failure::conflict(super::REFUSED.code, detail.to_owned()),
        };
        return Err(failure.remedy(remedy));
    }
    Ok(result)
}
fn limit(inputs: &Inputs) -> Result<usize, Failure> {
    inputs
        .require("limit")?
        .parse()
        .map_err(|_| invalid("--limit must be 1..200"))
}
pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    send(
        inputs,
        recovery::Command::Status {
            limit: limit(inputs)?,
        },
    )
}
pub fn retry(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    send(
        inputs,
        recovery::Command::Retry {
            row: inputs.require("row")?.into(),
        },
    )
}
pub fn preview(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    send(
        inputs,
        recovery::Command::SanitizePreview {
            limit: limit(inputs)?,
        },
    )
}
pub fn apply(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    send(
        inputs,
        recovery::Command::SanitizeApply {
            digest: inputs.require("digest")?.into(),
        },
    )
}

pub async fn invoke(State(app): State<crate::host::App>, bytes: Bytes) -> Response {
    if bytes.len() > 16 * 1024 {
        return crate::host::typed(&invalid("Recovery control exceeds its byte bound"))
            .into_response();
    }
    let control: Control = match serde_json::from_slice(&bytes) {
        Ok(control) => control,
        Err(error) => return crate::host::typed(&invalid(error)).into_response(),
    };
    if let Err(error) = control.validate() {
        return crate::host::typed(&invalid(error)).into_response();
    }
    match crate::host::admitting(move || {
        let now = ds_sync_runtime::now_ms();
        app.sessions.admit_read(
            ds_command_kernel::execution_context::SYNC_RECOVERY,
            Some(&control.project),
            &bytes,
            now,
        )?;
        let fence: Fence = crate::server_sync::fence_of(app.sessions.identity());
        let scope = Scope::Project {
            project: control.project,
        };
        let write = matches!(
            &control.command,
            recovery::Command::Retry { .. } | recovery::Command::SanitizeApply { .. }
        );
        let mut store = if write {
            Some(ds_sync_store::Store::open(&app.database).map_err(super::failure)?)
        } else {
            ds_sync_store::Store::open_read_only(&app.database).map_err(super::failure)?
        };
        let result = match store.as_mut() {
            Some(store) => store
                .recovery(&fence, &scope, now, control.command)
                .map_err(super::failure)?,
            None => recovery::resolve(recovery::Request {
                schema: recovery::SCHEMA.into(),
                fence,
                scope,
                now_ms: now,
                entries: Vec::new(),
                lease: None,
                command: control.command,
            })
            .map_err(super::failure)?,
        };
        if result.applied
            && !result.changes.is_empty()
            && let Some(activity) = &app.activity
        {
            activity.local_publication_completed();
        }
        app.auth
            .authorize(&app.connection.owner)
            .map_err(|reason| {
                Failure::unauthorized("server_owner_changed", reason)
                    .remedy(super::OWNER_CHANGED.remedy)
            })?;
        Ok(Json(result))
    })
    .await
    {
        Ok(result) => result.into_response(),
        Err(error) => error.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn public_controls_refuse_authority_injection_and_require_the_complete_preview_digest() {
        let control = json!({"schema":recovery::SCHEMA,"project":"project","command":{"action":"status","limit":50}});
        let parsed: Control = serde_json::from_value(control.clone()).unwrap();
        assert!(parsed.validate().is_ok());
        let mut crossed = control;
        crossed["fence"] = json!({"account":"other"});
        assert!(serde_json::from_value::<Control>(crossed).is_err());
        let malformed = Control {
            schema: recovery::SCHEMA.into(),
            project: "project".into(),
            command: recovery::Command::SanitizeApply {
                digest: "wrong".into(),
            },
        };
        assert!(malformed.validate().is_err());
    }
}
