//! `ds desktop sync plan` — the sync gate's one decision, answered with no
//! browser, no pairing and no credential: the caller hands the kernel what an
//! install holds and what the project's shared record holds, and reads back
//! upload / download / conflict / nothing and when to look again.
//!
//! Contract: `ds-command-kernel/docs/contracts/ds-sync-engine.md`. This command
//! decides nothing; it reads bounded files, asks the kernel, and renders.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use std::io::Read;

const FILE_BOUND: usize = 8 * 1024 * 1024;

const PROJECT: Arg = Arg::value(
    "project",
    "<project-id>",
    "The project whose artifacts are planned.",
)
.required();
const INSTALL: Arg = Arg {
    name: "install-id",
    kind: ds_cli_contract::spec::ArgKind::Value,
    value: "<id>",
    required: false,
    default: Some("headless"),
    choices: &[],
    summary: "The install the grant is bound to; a label for a headless plan.",
};
const LOCAL: Arg = Arg::value(
    "local",
    "<file>",
    "JSON array of what this install holds: {engine, operation, variant?, sha256, size_bytes, produced_at_ms, base_revision?, readable?, engine_release}.",
);
const REMOTE: Arg = Arg::value(
    "remote",
    "<file>",
    "JSON array of the project's shared heads: {engine, operation, variant?, revision, sha256, published_at_ms}.",
);
const GRANT: Arg = Arg::value(
    "grant",
    "<file>",
    "The work grant in hand, as work/open returned it (optional).",
);
const NOW: Arg = Arg::value(
    "now-ms",
    "<ms>",
    "Server-observed time in ms; defaults to this machine's clock.",
);
const OFFLINE: Arg = Arg::switch("offline", "Plan as if the network were unreachable.");
const TRIGGER: Arg = Arg {
    name: "trigger",
    kind: ds_cli_contract::spec::ArgKind::Value,
    value: "<trigger>",
    required: false,
    default: Some("manual"),
    choices: &[
        "startup",
        "navigation",
        "local_change",
        "publish_completed",
        "download_completed",
        "reconnect",
        "remote_push",
        "manual",
        "grant_opened",
    ],
    summary: "Why the plan is asked for now; a plan is never asked for because time passed.",
};
const REMOTE_READ_AT: Arg = Arg::value(
    "remote-read-at-ms",
    "<ms>",
    "When the held remote heads were last read; omit when never.",
);

const REFUSALS: &[Refusal] = &[Refusal {
    code: "sync_plan_invalid",
    when: "an inventory file is missing, malformed, over 8 MiB, or the kernel refuses the request",
    remedy: "pass the inventories in the documented shapes; the kernel names the offending field",
}];

pub static PLAN_COMMAND: Command = Command {
    id: "desktop.sync.plan",
    path: &["desktop", "sync", "plan"],
    contract: 1,
    summary: "Decide upload, download, conflict or nothing for project artifacts.",
    purpose: "The Sync Center's one decision, headless and credential-free: given what this install holds and what the project's shared record holds, the kernel returns the ordered actions a host performs (open a grant, upload, download, surface a conflict, nothing, refuse a malformed row) and when to look again. Nothing is keyed by an account; the same answer holds on every machine and in the desktop shell.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        INSTALL,
        LOCAL,
        REMOTE,
        GRANT,
        NOW,
        OFFLINE,
        TRIGGER,
        REMOTE_READ_AT,
    ],
    output: "{schema, project, actions[{kind, identity?, reason, …}], refresh_remote{needed, reason}, wake{kind, at_ms?, reason?}, grant_valid, summary{uploads, downloads, conflicts, refused, in_sync}}.",
    examples: &[Example {
        command: "ds desktop sync plan --project my-project --local local.json --remote remote.json --output json",
        note: "`.data.actions` is what a host performs, in order; `open_grant` always comes first when an upload needs one.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.status.md"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("sync_plan_invalid", e.to_string()).remedy(
        "pass the inventories in the documented shapes; the kernel names the offending field",
    )
}

fn json_file(path: &str) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| invalid(format!("{path}: {e}")))?
        .take(FILE_BOUND as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > FILE_BOUND {
        return Err(invalid(format!("{path} exceeds 8 MiB")));
    }
    serde_json::from_slice(&bytes).map_err(|e| invalid(format!("{path}: {e}")))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn run(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let now = match i.value("now-ms") {
        Some(raw) => raw
            .trim()
            .parse::<u64>()
            .map_err(|_| invalid("--now-ms must be a whole number of milliseconds"))?,
        None => now_ms(),
    };
    let personal = i.switch("personal");
    let project = i.value("project").unwrap_or("");
    if personal == !project.is_empty() {
        return Err(invalid(if personal {
            "--personal takes no --project"
        } else {
            "pass --project <id> or --personal"
        }));
    }
    let mut request = json!({
        "schema": "ds.sync-plan/v1",
        "project": project,
        "personal": personal,
        "install_id": i.value("install-id").unwrap_or("headless"),
        "now_ms": now,
        "online": !i.switch("offline"),
        "trigger": i.value("trigger").unwrap_or("manual"),
        "local": match i.value("local") { Some(path) => json_file(path)?, None => json!([]) },
        "remote": match i.value("remote") { Some(path) => json_file(path)?, None => json!([]) },
    });
    if let Some(path) = i.value("grant") {
        request["grant"] = json_file(path)?;
    }
    if let Some(raw) = i.value("remote-read-at-ms") {
        request["remote_read_at_ms"] =
            json!(raw.trim().parse::<u64>().map_err(|_| invalid(
                "--remote-read-at-ms must be a whole number of milliseconds"
            ))?);
    }
    let input = serde_json::to_vec(&request).map_err(invalid)?;
    let reply = ds_command_kernel::sync::evaluate(&input).map_err(invalid)?;
    serde_json::from_str(&reply).map_err(invalid)
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    let summary = &data["summary"];
    out.push_str(&format!(
        "{}: {} to upload, {} to download, {} conflicts, {} refused, {} in sync; grant {}\n",
        if data["personal"].as_bool().unwrap_or(false) {
            "personal"
        } else {
            data["project"].as_str().unwrap_or("-")
        },
        summary["uploads"],
        summary["downloads"],
        summary["conflicts"],
        summary["refused"],
        summary["in_sync"],
        if data["grant_valid"].as_bool().unwrap_or(false) {
            "valid"
        } else {
            "needed"
        }
    ));
    for action in data["actions"].as_array().into_iter().flatten() {
        let identity = &action["identity"];
        let name = if identity.is_object() {
            format!(
                "{}/{}/{}",
                identity["engine"].as_str().unwrap_or("?"),
                identity["operation"].as_str().unwrap_or("?"),
                identity["variant"].as_str().unwrap_or("default")
            )
        } else {
            String::from("-")
        };
        out.push_str(&format!(
            "  {:<11} {:<48} {}\n",
            action["kind"].as_str().unwrap_or("?"),
            name,
            action["reason"].as_str().unwrap_or("")
        ));
    }
    out.push_str(&format!(
        "remote heads: {} ({}); wake: {}\n",
        if data["refresh_remote"]["needed"].as_bool().unwrap_or(false) {
            "re-read"
        } else {
            "held"
        },
        data["refresh_remote"]["reason"].as_str().unwrap_or(""),
        match data["wake"]["kind"].as_str() {
            Some("at") => format!(
                "at {} ms ({})",
                data["wake"]["at_ms"],
                data["wake"]["reason"].as_str().unwrap_or("")
            ),
            _ => String::from("next trigger"),
        }
    ));
    out
}
