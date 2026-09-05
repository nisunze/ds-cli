//! Exact UI/CLI portfolio batch request. Scheduling lives in ds-command-kernel.
use crate::paired;
use ds_cli_contract::{
    Context, Inputs,
    outcome::Failure,
    spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal},
};
use ds_command_kernel::solar_batch::{MAX_BYTES, Request};
use serde_json::{Value, json};
use std::{fs::File, io::Read, time::Duration};

const DESCRIPTOR: Arg = Arg::value(
    "desktop-descriptor",
    "<path>",
    "Select one paired DS session.",
);
static REFUSALS: &[Refusal] = &[
    Refusal {
        code: "invalid_portfolio_batch",
        when: "the request file is unreadable or violates the shared Rust batch schema",
        remedy: "use the complete v1 request documented in docs/reference/solar.md",
    },
    Refusal {
        code: "desktop_not_paired",
        when: "no local DS session is running",
        remedy: "start DS GridDesign and sign in",
    },
    Refusal {
        code: "desktop_refused",
        when: "project, placement, membership or batch identity was refused",
        remedy: "read the refusal and use the active project's exact catalog; never replay a batch id",
    },
    Refusal {
        code: "desktop_contract_mismatch",
        when: "the session returned a different batch id",
        remedy: "update ds and DS GridDesign to matching versions",
    },
];
pub static START_COMMAND: Command = Command {
    id: "solar.portfolio.batch.start",
    path: &["solar", "portfolio", "batch", "start"],
    contract: 1,
    summary: "Submit the shared Rust portfolio batch command.",
    purpose: "Validates the exact UI request with ds-command-kernel, then submits it to the paired effect host. Rust owns bounded scheduling, source reuse, lifecycle and cancellation. Work continues after this receipt; no cloud is needed for native placement with prepared inputs.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Job,
    args: &[
        Arg::value(
            "request",
            "<json-file>",
            "Complete ds-solar.portfolio-batch/v1 request, at most 1 MiB.",
        )
        .required(),
        DESCRIPTOR,
    ],
    output: "Durable run_id, exact request, per-portfolio run ids and current kernel state.",
    examples: &[Example {
        command: "ds solar portfolio batch start --request batch.json --output json",
        note: "The same request shape used by the UI, with no hidden CLI defaults.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};
pub static STATUS_COMMAND: Command = Command {
    id: "solar.portfolio.batch.status",
    path: &["solar", "portfolio", "batch", "status"],
    contract: 1,
    summary: "Read one durable portfolio batch state.",
    purpose: "Reads the exact owner/project batch. Interrupted work is recorded explicitly after restart; reading never restarts compute.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Exact batch_id from the submission.").required(),
        DESCRIPTOR,
    ],
    output: "Kernel batch state with complete/cancelled flags and each row's terminal outcome.",
    examples: &[Example {
        command: "ds solar portfolio batch status --run-id batch-example --output json",
        note: "Complete can contain failures; inspect every row.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};
pub static CANCEL_COMMAND: Command = Command {
    id: "solar.portfolio.batch.cancel",
    path: &["solar", "portfolio", "batch", "cancel"],
    contract: 1,
    summary: "Cancel queued and active native portfolio batch work.",
    purpose: "Asks the same kernel command used by the UI to stop queued work and coordinate native cancellation. Already committed results win a cancellation race. Active hosted calls settle normally; the cloud endpoint has no cancellation acknowledgement.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Exact batch_id to cancel.").required(),
        DESCRIPTOR,
    ],
    output: "Current kernel state; poll until complete to observe settled outcomes.",
    examples: &[Example {
        command: "ds solar portfolio batch cancel --run-id batch-example --output json",
        note: "Does not delete completed artifacts.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};
fn invalid(error: impl std::fmt::Display) -> Failure {
    Failure::invalid("invalid_portfolio_batch", error.to_string())
        .remedy("correct the v1 request using docs/reference/solar.md")
}
pub fn parse_request(bytes: &[u8]) -> Result<Request, Failure> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(invalid("request exceeds 1 MiB"));
    }
    let request: Request = serde_json::from_slice(bytes).map_err(invalid)?;
    request.validate().map_err(invalid)?;
    Ok(request)
}
pub fn start(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    File::open(inputs.require("request")?)
        .map_err(invalid)?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    let request = parse_request(&bytes)?;
    let result = paired::invoke(
        inputs,
        START_COMMAND.id,
        json!({"request": request}),
        Duration::from_secs(30),
    )?;
    paired::require_exact_identity(result, START_COMMAND.id, &request.batch_id, None)
}
fn addressed(inputs: &Inputs, operation: &'static str) -> Result<Value, Failure> {
    let id = inputs.require("run-id")?;
    let result = paired::invoke(
        inputs,
        operation,
        json!({"run_id": id}),
        Duration::from_secs(30),
    )?;
    paired::require_exact_identity(result, operation, id, None)
}
pub fn status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    addressed(inputs, STATUS_COMMAND.id)
}
pub fn cancel(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    addressed(inputs, CANCEL_COMMAND.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_unknown_contract_fields_never_get_cli_defaults() {
        assert!(parse_request(b"{}").is_err());
        assert!(parse_request(&vec![b' '; MAX_BYTES + 1]).is_err());
    }
}
