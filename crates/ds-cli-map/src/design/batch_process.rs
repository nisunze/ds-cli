//! `ds map design batch process` — Fast-process many transformers locally.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Command, Effect, Example, Execution, Refusal,
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
    summary: "Transformer to Fast-process. Repeat for the explicit batch.",
};

const SETTING_ARG: Arg = Arg {
    name: "setting",
    kind: ArgKind::Repeated,
    value: "<key=bool|number>",
    required: false,
    default: None,
    choices: &[],
    summary: "Override a processor setting. Repeat; omit to use saved Fast Mode settings.",
};

const PARALLEL_ARG: Arg = Arg {
    name: "parallel",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: None,
    choices: &[],
    summary: "Concurrent local process jobs (1-32); defaults to the app batch setting.",
};

pub static COMMAND: Command = Command {
    id: "map.design.batch.process",
    path: &["map", "design", "batch", "process"],
    contract: 1,
    summary: "Fast-process an explicit transformer batch locally.",
    purpose: "\
Runs the same local-WASM replay scheduler as the Design Status bulk Process \
action. Each transformer is isolated, the bounded worker pool continues after \
an item failure, and successful outputs are staged in the desktop room. Nothing \
is uploaded until a separate save command is confirmed.",
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, SETTING_ARG, PARALLEL_ARG, DESCRIPTOR_ARG],
    output: "\
Requested parallelism and per-transformer success, warning, or error rows, plus \
total/succeeded/failed counts. Successful rows report staged=true and \
persisted=false.",
    examples: &[Example {
        command: "ds map design batch process --transformer TX-1 --transformer TX-2 --parallel 4 --output json",
        note: "Uses saved Fast Mode settings and the desktop-owned WASM worker scheduler.",
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
            code: "invalid_setting",
            when: "a --setting is not key=true, key=false, or key=<finite number>",
            remedy: "pass e.g. --setting keep_lv_poles=true or --setting max_span_m=45",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformers = inputs.repeated("transformer");
    let mut settings = Map::new();
    for raw in inputs.repeated("setting") {
        let Some((key, raw_value)) = raw.split_once('=') else {
            return Err(invalid_setting(raw));
        };
        let key = key.trim();
        let raw_value = raw_value.trim();
        if key.is_empty() || raw_value.is_empty() {
            return Err(invalid_setting(raw));
        }
        let value = match raw_value {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            other => other
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map_or_else(|| Err(invalid_setting(raw)), |value| Ok(json!(value)))?,
        };
        settings.insert(key.to_string(), value);
    }
    let mut arguments = Map::new();
    arguments.insert("transformers".into(), json!(transformers));
    if !settings.is_empty() {
        arguments.insert("settings".into(), Value::Object(settings));
    }
    if let Some(parallel) = inputs.value("parallel") {
        arguments.insert(
            "parallel".into(),
            json!(crate::integer(parallel, "parallel", 1, 32)?),
        );
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::DESIGN_PROCESS_BATCH,
        Value::Object(arguments),
        crate::DESIGN_PROCESS_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

fn invalid_setting(raw: &str) -> Failure {
    Failure::invalid(
        "invalid_setting",
        format!("invalid processor setting `{raw}`"),
    )
    .remedy("pass e.g. --setting keep_lv_poles=true or --setting max_span_m=45")
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} transformer(s): {} staged, {} failed · {} parallel\n",
        data["total"].as_u64().unwrap_or(0),
        data["succeeded"].as_u64().unwrap_or(0),
        data["failed"].as_u64().unwrap_or(0),
        data["parallel"].as_u64().unwrap_or(1),
    );
    if let Some(rows) = data["rows"].as_array() {
        for row in rows {
            out.push_str(&format!(
                "  {} {}{}\n",
                if row["error"].is_string() {
                    "✗"
                } else {
                    "✓"
                },
                row["name"].as_str().unwrap_or("?"),
                row["error"]
                    .as_str()
                    .or_else(|| row["warning"].as_str())
                    .map(|message| format!("  {message}"))
                    .unwrap_or_default(),
            ));
        }
    }
    out
}
