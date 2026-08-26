//! `ds shell status` — this shell, and the next one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::platform::{self, Registration};
use crate::reach::{self, Reach};

pub static COMMAND: Command = Command {
    id: "shell.status",
    path: &["shell", "status"],
    contract: 1,
    summary: "Where `ds` resolves from: this shell, and new ones.",
    purpose: "\
Reports which executable `ds` resolves to on this process's PATH, whether that \
is this executable, and whether this executable's directory is registered \
where a freshly opened PowerShell, cmd, Bash or Git Bash window will find it. \
The two answers differ more often than one would think: a terminal opened \
before an install keeps its old PATH. Reads nothing but the environment and \
the user's own PATH registration.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "\
This executable, what `ds` resolves to on the current PATH, other `ds` \
executables on it, the durable registration, and a remedy when a new shell \
would not find this one.",
    examples: &[
        Example {
            command: "ds shell status",
            note: "",
            runnable: true,
        },
        Example {
            command: "ds shell status --output json",
            note: "Branch on .data.reachable and .data.registration.new_shells_see.",
            runnable: true,
        },
    ],
    refusals: &[crate::EXECUTABLE_UNRESOLVED, crate::REGISTRATION_UNREADABLE],
    reference: Some("docs/reference/shell.md"),
    availability: crate::always,
};

/// Both answers, taken together.
pub struct Snapshot {
    pub reach: Reach,
    pub registration: Registration,
}

pub fn snapshot() -> Result<Snapshot, Failure> {
    let reach = reach::probe()?;
    let registration = platform::inspect(&reach)?;
    Ok(Snapshot {
        reach,
        registration,
    })
}

/// One word for doctor: `reachable` (this shell finds it), `registered`
/// (only new shells will), or `unreachable`.
pub fn word(snapshot: &Snapshot) -> &'static str {
    if snapshot.reach.reachable {
        "reachable"
    } else if snapshot.registration.new_shells_see {
        "registered"
    } else {
        "unreachable"
    }
}

/// The one thing to do next, when there is one.
pub fn remedy(snapshot: &Snapshot) -> Option<String> {
    if snapshot.reach.reachable {
        return None;
    }
    if snapshot.registration.new_shells_see {
        return Some(
            "open a new terminal window; this one started before `ds` was registered".to_string(),
        );
    }
    Some("run `ds shell register`, then open a new terminal window".to_string())
}

pub fn data(snapshot: &Snapshot) -> Value {
    json!({
        "executable": reach::display(&snapshot.reach.executable),
        "directory": reach::display(&snapshot.reach.directory),
        "reachable": snapshot.reach.reachable,
        "resolves_to": snapshot.reach.resolves_to.as_deref().map(reach::display),
        "others": snapshot.reach.others.iter().map(|path| reach::display(path)).collect::<Vec<_>>(),
        "registration": snapshot.registration.json(),
        "remedy": remedy(snapshot),
    })
}

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    Ok(data(&snapshot()?))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "ds          {}\n",
        data["executable"].as_str().unwrap_or("")
    );
    let this_shell = if data["reachable"].as_bool().unwrap_or(false) {
        "reachable".to_string()
    } else {
        match data["resolves_to"].as_str() {
            Some(other) => format!("not reachable — `ds` resolves to {other}"),
            None => "not reachable — no `ds` on this PATH".to_string(),
        }
    };
    out.push_str(&format!("this shell  {this_shell}\n"));
    let registration = &data["registration"];
    let new_shells = if registration["new_shells_see"].as_bool().unwrap_or(false) {
        "will find it"
    } else if registration["present"].as_bool().unwrap_or(false) {
        "registered, but not yet honoured"
    } else {
        "will not find it — not registered"
    };
    out.push_str(&format!(
        "new shells  {new_shells}  ({})\n",
        registration["entry"].as_str().unwrap_or("")
    ));
    if let Some(note) = registration["note"].as_str() {
        out.push_str(&format!("            {note}\n"));
    }
    let others = data["others"].as_array().map_or(0, Vec::len);
    if others > 0 {
        out.push_str(&format!(
            "also on PATH  {} other `ds` executable(s):\n",
            others
        ));
        for other in data["others"].as_array().into_iter().flatten() {
            out.push_str(&format!("            {}\n", other.as_str().unwrap_or("")));
        }
    }
    if let Some(remedy) = data["remedy"].as_str() {
        out.push_str(&format!("→ {remedy}\n"));
    }
    out
}
