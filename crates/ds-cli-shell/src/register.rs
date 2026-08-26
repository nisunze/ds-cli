//! `ds shell register` — make new shells find this executable.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::platform;
use crate::reach;
use crate::status;

pub static COMMAND: Command = Command {
    id: "shell.register",
    path: &["shell", "register"],
    contract: 1,
    summary: "Register this executable's directory for new shells.",
    purpose: "\
Adds the directory holding this executable to the user's durable PATH \
registration — `HKCU\\Environment\\Path` on Windows, a `~/.local/bin/ds` link \
elsewhere — so every new PowerShell, cmd, Bash or Git Bash window resolves \
`ds` here. Idempotent: an entry already present is left exactly as it is, \
and a package install into a system directory needs nothing and changes \
nothing. Windows already open keep the PATH they started with. The desktop \
installer runs this after copying files; run it by hand after a source build \
or a copied binary.",
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "\
Whether anything changed, the registration as it now stands, and the same \
report `ds shell status` gives.",
    examples: &[Example {
        command: "ds shell register",
        note: "Then open a new terminal window.",
        runnable: false,
    }],
    refusals: &[
        crate::EXECUTABLE_UNRESOLVED,
        crate::REGISTRATION_UNREADABLE,
        crate::REGISTRATION_UNWRITABLE,
        crate::LINK_FOREIGN,
    ],
    reference: Some("docs/reference/shell.md"),
    availability: crate::always,
};

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let reach = reach::probe()?;
    let changed = platform::register(&reach)?;
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
    let status = &data["status"];
    let entry = status["registration"]["entry"].as_str().unwrap_or("");
    let mut out = if data["changed"].as_bool().unwrap_or(false) {
        format!("registered  {entry}\n")
    } else if status["registration"]["new_shells_see"]
        .as_bool()
        .unwrap_or(false)
    {
        format!("already registered  {entry}\n")
    } else {
        format!("nothing to change  {entry}\n")
    };
    if let Some(note) = status["registration"]["note"].as_str() {
        out.push_str(&format!("  {note}\n"));
    }
    if let Some(remedy) = status["remedy"].as_str() {
        out.push_str(&format!("→ {remedy}\n"));
    }
    out
}
