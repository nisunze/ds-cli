//! What AutoProcess would do with a set of committed edits — the kernel's
//! three admission answers, headless.
//!
//! The host half of AutoProcess (the accumulator, the GDF walk that maps
//! changed features to feeders, the timer and the engine latch) is a browser
//! edit session and stays there. The RULES are not: whether an edit warrants
//! re-running the LV network, how wide the next run must be, and when queued
//! work dispatches are `ds_command_kernel::autoprocess`, so an agent can ask
//! them without a Desktop and get the answer the page acts on.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use std::io::Read;

const MAX_BYTES: usize = 1024 * 1024;

fn local() -> Availability {
    Availability::Available
}

const REFUSALS: &[Refusal] = &[Refusal {
    code: "autoprocess_request_invalid",
    when: "The changes document is malformed, too large, or a section is not a kernel request",
    remedy: "Pass {trigger?, differential_scope?, cadence?} with the fields `ds capabilities design.autoprocess.plan` names",
}];

const CHANGES: Arg = Arg::value(
    "changes",
    "<json-file>",
    "{trigger?, differential_scope?, cadence?}, at most 1 MiB.",
)
.required();
const NOW_MS: Arg = Arg::value(
    "now-ms",
    "<ms>",
    "Epoch milliseconds for the cadence answer; defaults to this clock.",
);

pub static COMMAND: Command = Command {
    id: "design.autoprocess.plan",
    path: &["design", "autoprocess", "plan"],
    contract: 1,
    summary: "Plan what AutoProcess would do with committed edits.",
    purpose: "Answers the three AutoProcess admission questions from one document: does a committed edit warrant re-running the LV network, must the next run be differential or full, and does queued work dispatch now or wait. The clock is an input, so the same document always plans the same way. AutoProcess is a Fast-lane activity; this plans it without running it.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[CHANGES, NOW_MS],
    output: "The sections present in the request: trigger {schedule, reason_key}, differential_scope {scope, reason_key, pending?} and cadence {decision, wait_ms?, reason_key, waiting_not_executing}.",
    examples: &[Example {
        command: "ds design autoprocess plan --changes edits.json --output json",
        note: "`.data.cadence.wait_ms` is when the host should evaluate again.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: local,
};

fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("autoprocess_request_invalid", e.to_string()).remedy(
        "Pass {trigger?, differential_scope?, cadence?} as `ds capabilities design.autoprocess.plan` names them",
    )
}

fn document(path: &str) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(invalid)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("changes document exceeds 1 MiB"));
    }
    serde_json::from_slice(&bytes).map_err(invalid)
}

/// One section through the kernel. `op` is added here so the document stays
/// the three plain request bodies rather than a tagged union the caller has
/// to spell.
fn section(op: &str, mut body: Value) -> Result<Value, Failure> {
    let Some(map) = body.as_object_mut() else {
        return Err(invalid(format!("`{op}` must be an object")));
    };
    map.insert("op".into(), json!(op));
    let reply =
        ds_command_kernel::autoprocess::evaluate(&serde_json::to_vec(&body).map_err(invalid)?)
            .map_err(invalid)?;
    let parsed: Value = serde_json::from_str(&reply).map_err(invalid)?;
    Ok(parsed["result"].clone())
}

pub fn run(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let mut request = document(i.require("changes")?)?;
    let Some(sections) = request.as_object_mut() else {
        return Err(invalid("the changes document must be an object"));
    };
    if let Some(raw) = i.value("now-ms") {
        let now: u64 = raw.trim().parse().map_err(invalid)?;
        if let Some(cadence) = sections.get_mut("cadence").and_then(Value::as_object_mut) {
            cadence.insert("now_ms".into(), json!(now));
        }
    }
    let known = ["trigger", "differential_scope", "cadence"];
    if let Some(unknown) = sections.keys().find(|key| !known.contains(&key.as_str())) {
        return Err(invalid(format!("unknown section `{unknown}`")));
    }
    let mut out = json!({});
    for op in known {
        if let Some(body) = sections.get(op) {
            out[op] = section(op, body.clone())?;
        }
    }
    if out.as_object().is_some_and(serde_json::Map::is_empty) {
        return Err(invalid("no section to plan"));
    }
    Ok(out)
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    for (key, label) in [
        ("trigger", "trigger"),
        ("differential_scope", "scope"),
        ("cadence", "cadence"),
    ] {
        let section = &data[key];
        if section.is_null() {
            continue;
        }
        let verdict = section["decision"]
            .as_str()
            .or_else(|| section["scope"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| section["schedule"].to_string());
        out.push_str(&format!(
            "  {label:<18} {verdict:<12} {}\n",
            section["reason_key"].as_str().unwrap_or("")
        ));
        if let Some(wait) = section["wait_ms"].as_u64() {
            out.push_str(&format!("  {:<18} {wait} ms\n", "wait"));
        }
    }
    out
}
