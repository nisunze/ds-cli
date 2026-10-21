//! Offline backup-ledger planning; archive access stays with the authenticated gateway.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::Value;
pub static COMMAND: Command = Command {
    id: "assets.backup.plan",
    path: &["assets", "backup", "plan"],
    contract: 1,
    summary: "Inspect deletion metadata and plan exact backup downloads.",
    purpose: "Reads a bounded local ledger request. Rust returns stable deletion event IDs or a validated download selection. One event downloads as its original file; multiple events form a ZIP. No active project, identity, archive fetch, or restore is performed.",
    chapter: Chapter::Assets,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "request",
        "<json>",
        "JSON with action=project and rows, or action=download, rows and selected_ids; at most 100 metadata rows.",
    )
    .required()],
    output: "Projected event metadata, or targets, delivery=file|zip and max_parallel. Archive bytes are absent.",
    examples: &[],
    refusals: &[Refusal {
        code: "backup_request_invalid",
        when: "the request is unreadable, oversized, malformed or selection is unavailable",
        remedy: "use project/download with at most 100 rows and known event IDs",
    }],
    reference: None,
    search: &[], requires: Requires::Server,
    availability: || Availability::Available,
};
pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let invalid = |e: String| {
        Failure::invalid("backup_request_invalid", e)
            .remedy("use at most 100 rows and known downloadable IDs")
    };
    let path = inputs.require("request")?;
    let meta = std::fs::metadata(path).map_err(|e| invalid(e.to_string()))?;
    if meta.len() > 16 * 1024 * 1024 {
        return Err(invalid("Request exceeds 16 MiB".into()));
    }
    let bytes = std::fs::read(path).map_err(|e| invalid(e.to_string()))?;
    let request: Value = serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    if request["rows"]
        .as_array()
        .is_none_or(|rows| rows.len() > 100)
    {
        return Err(invalid("Request requires at most 100 rows".into()));
    }
    let answer = ds_command_kernel::backup_ledger::evaluate(&bytes).map_err(invalid)?;
    serde_json::from_str(&answer)
        .map_err(|e| Failure::invalid("backup_request_invalid", e.to_string()))
}
pub fn render(value: &Value) -> String {
    format!("{}\n", value)
}
