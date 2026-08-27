//! `ds map design batch save` — explicitly persist many staged rooms.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const TRANSFORMER_ARG: Arg = Arg {
    name: "transformer",
    kind: ArgKind::Repeated,
    value: "<name>",
    required: true,
    default: None,
    choices: &[],
    summary: "Staged transformer to save. Repeat for the explicit batch.",
};

const PARALLEL_ARG: Arg = Arg {
    name: "parallel",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: None,
    choices: &[],
    summary: "Concurrent project saves (1-32); defaults to the app batch setting.",
};

pub static COMMAND: Command = Command {
    id: "map.design.batch.save",
    path: &["map", "design", "batch", "save"],
    contract: 1,
    summary: "Save an explicit batch of staged transformer rooms.",
    purpose: "\
Persists each named dirty desktop room with optimistic concurrency checks and \
v_first/v_last stamping from the deliberate Design Status version. Items are isolated so one conflict does not \
cancel unrelated saves. This durable project write always requires --yes.",
    chapter: Chapter::Design,
    effect: Effect::ArtifactWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, PARALLEL_ARG, DESCRIPTOR_ARG],
    output: "\
Per-transformer saved/persisted state, design version, concurrency generation, or error, plus total, succeeded, \
failed, and requested parallelism.",
    examples: &[Example {
        command: "ds map design batch save --transformer TX-1 --transformer TX-2 --parallel 4 --yes --output json",
        note: "Review staged rooms first; without --yes dispatch refuses before opening the bridge.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_NUMBER,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a command that writes to the project",
            remedy: "re-run with --yes once you intend the batch write",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("transformers".into(), json!(inputs.repeated("transformer")));
    if let Some(parallel) = inputs.value("parallel") {
        arguments.insert(
            "parallel".into(),
            json!(crate::integer(parallel, "parallel", 1, 32)?),
        );
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::DESIGN_SAVE_BATCH,
        Value::Object(arguments),
        crate::DESIGN_PROCESS_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} transformer(s): {} completed, {} failed · {} parallel\n",
        data["total"].as_u64().unwrap_or(0),
        data["succeeded"].as_u64().unwrap_or(0),
        data["failed"].as_u64().unwrap_or(0),
        data["parallel"].as_u64().unwrap_or(1),
    );
    if let Some(rows) = data["rows"].as_array() {
        for row in rows {
            out.push_str(&format!(
                "  {} {}{}\n",
                if row["ok"].as_bool().unwrap_or(false) {
                    "✓"
                } else {
                    "✗"
                },
                row["transformer"].as_str().unwrap_or("?"),
                row["error"]
                    .as_str()
                    .or_else(|| row["reason"].as_str())
                    .map(|message| format!("  {message}"))
                    .unwrap_or_default(),
            ));
        }
    }
    out
}
