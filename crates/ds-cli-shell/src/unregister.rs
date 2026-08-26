//! `ds shell unregister` — the exact inverse of `register`.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::platform;
use crate::reach;
use crate::status;

pub static COMMAND: Command = Command {
    id: "shell.unregister",
    path: &["shell", "unregister"],
    contract: 1,
    summary: "Remove this executable's directory from the registration.",
    purpose: "\
Removes the directory holding this executable from the user's durable PATH \
registration, and nothing else: every other entry keeps its place and its \
spelling, and a `~/.local/bin/ds` link is removed only when it points here. \
Idempotent. The desktop uninstaller runs this before removing files.",
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "Whether anything changed, and the registration as it now stands.",
    examples: &[Example {
        command: "ds shell unregister",
        note: "",
        runnable: false,
    }],
    refusals: &[
        crate::EXECUTABLE_UNRESOLVED,
        crate::REGISTRATION_UNREADABLE,
        crate::REGISTRATION_UNWRITABLE,
    ],
    reference: Some("docs/reference/shell.md"),
    availability: crate::always,
};

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let reach = reach::probe()?;
    let changed = platform::unregister(&reach)?;
    let registration = platform::inspect(&reach)?;
    let snapshot = status::Snapshot {
        reach,
        registration,
    };
    Ok(json!({
        "changed": changed,
        "status": status::data(&snapshot),
    }))
}

pub fn render(data: &Value) -> String {
    let entry = data["status"]["registration"]["entry"]
        .as_str()
        .unwrap_or("");
    if data["changed"].as_bool().unwrap_or(false) {
        format!("unregistered  {entry}\n")
    } else {
        format!("was not registered  {entry}\n")
    }
}
