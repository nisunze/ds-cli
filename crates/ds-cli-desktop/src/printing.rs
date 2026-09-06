//! Prepare the paired application's project printing workflow; Brain owns saves.
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal},
};
use serde_json::{Value, json};
use std::{io::Read, time::Duration};

pub const PREPARE_OP: BridgeOp = BridgeOp {
    operation: "printing.prepare",
    arguments: &["request"],
};
pub const LIST_OP: BridgeOp = BridgeOp {
    operation: "printing.list",
    arguments: &["scope"],
};
pub const GET_OP: BridgeOp = BridgeOp {
    operation: "printing.get",
    arguments: &["scope", "id"],
};
pub const SAVE_OP: BridgeOp = BridgeOp {
    operation: "printing.save",
    arguments: &["request"],
};

const SCOPE_ARG: Arg = Arg::value(
    "scope",
    "<project|global>",
    "Read the active project's catalog or the shared global samples.",
)
.choices(&["project", "global"])
.default("project");

pub static LIST_COMMAND: Command = Command {
    id: "desktop.printing.list",
    path: &["desktop", "printing", "list"],
    contract: 1,
    summary: "List named printing setups from Brain through the paired desktop.",
    purpose: "Returns the dynamic named printing catalog for the active project or the shared global samples. Project scope uses the paired application's exact active project; global scope is shared across projects.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[SCOPE_ARG, DESCRIPTOR_ARG],
    output: "Scope, active project when applicable, cache status and bounded setup summaries including their revision tokens.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    availability: ops::paired_availability,
};

pub static GET_COMMAND: Command = Command {
    id: "desktop.printing.get",
    path: &["desktop", "printing", "get"],
    contract: 1,
    summary: "Read one named printing setup and its authored layout.",
    purpose: "Reads one exact setup from the active project or global sample catalog through Brain. Its revision is the required optimistic token for a later prepare update.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        SCOPE_ARG,
        Arg::value(
            "id",
            "<setup-id>",
            "Exact id returned by desktop printing list.",
        )
        .required(),
        DESCRIPTOR_ARG,
    ],
    output: "Scope, project when applicable, cache status and the exact setup with layout and revision.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    availability: ops::paired_availability,
};

pub static SAVE_COMMAND: Command = Command {
    id: "desktop.printing.save",
    path: &["desktop", "printing", "save"],
    contract: 1,
    summary: "Save one project or global named printing setup through Brain.",
    purpose: "Publishes one authored layout into the active project's catalog or the shared global sample catalog. The request carries scope, layout and the exact expectedRevision returned by desktop printing get; an empty revision creates a new setup.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<json-file>",
            "Scope, authored layout and expectedRevision; at most 800 KB.",
        )
        .required(),
        DESCRIPTOR_ARG,
    ],
    output: "Scope, project when applicable, and the saved setup id, name and new revision.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
        Refusal {
            code: "printing_request_invalid",
            when: "the request file cannot be read or is not a bounded JSON object",
            remedy: "provide scope, layout and expectedRevision, at most 800 KB",
        },
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    availability: ops::paired_availability,
};

pub static PREPARE_COMMAND: Command = Command {
    id: "desktop.printing.prepare",
    path: &["desktop", "printing", "prepare"],
    contract: 1,
    summary: "Save a project print layout, select exports and prepare inputs.",
    purpose: "Runs the paired application's printing preparation under its signed-in active project. The request names layout, expectedRevision (empty for create) and formats including pdf__<layout-id>. Brain validates and saves the layout and project settings; the app then refreshes the sealed receipt and installs required reference data. These are sequential durable actions: a later preparation failure does not roll back a saved layout. Use the returned revision for further edits. This does not export a report; follow with map design report.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<json-file>",
            "Authored layout, expectedRevision and formats; at most 800 KB.",
        )
        .required(),
        DESCRIPTOR_ARG,
    ],
    output: "Active project, saved setup id/name/revision, selected formats and ready=true; no raw design features or credentials.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
        Refusal {
            code: "printing_request_invalid",
            when: "the request file cannot be read or is not a bounded JSON object",
            remedy: "provide a JSON object containing layout, expectedRevision and formats, at most 800 KB",
        },
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    availability: ops::paired_availability,
};

fn read_request(path: &str, remedy: &'static str) -> Result<Value, Failure> {
    let invalid =
        |message: String| Failure::invalid("printing_request_invalid", message).remedy(remedy);
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(800_001).read_to_end(&mut bytes))
        .map_err(|e| invalid(e.to_string()))?;
    if bytes.len() > 800_000 {
        return Err(invalid("printing request exceeds 800 KB".into()));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    if !value.is_object() {
        return Err(invalid("printing request must be an object".into()));
    }
    Ok(value)
}
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = read_request(
        inputs.require("request")?,
        "provide a JSON object containing layout, expectedRevision and formats, at most 800 KB",
    )?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &PREPARE_OP,
        json!({"request":request}),
        Duration::from_secs(600),
    )
}

pub fn save(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = read_request(
        inputs.require("request")?,
        "provide scope, layout and expectedRevision, at most 800 KB",
    )?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &SAVE_OP,
        json!({"request":request}),
        Duration::from_secs(180),
    )
}

pub fn list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &LIST_OP,
        json!({"scope": inputs.require("scope")?}),
        Duration::from_secs(120),
    )
    .map_err(ops::classify_signed_out)
}

pub fn get(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let id = inputs.require("id")?;
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(Failure::invalid(
            "printing_request_invalid",
            "invalid printing setup id",
        ));
    }
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &GET_OP,
        json!({"scope": inputs.require("scope")?, "id": id}),
        Duration::from_secs(120),
    )
    .map_err(ops::classify_signed_out)
}
pub fn render(data: &Value) -> String {
    format!("{data}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_input_is_a_named_refusal_before_pairing() {
        assert_eq!(
            read_request("/nonexistent/printing-request.json", "remedy")
                .unwrap_err()
                .code(),
            "printing_request_invalid"
        );
    }
}
