//! `ds map design upload stage` — clean native sources into transformer rooms.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const SOURCE_ARG: Arg = Arg {
    name: "source",
    kind: ArgKind::Repeated,
    value: "<transformer=native-path>",
    required: true,
    default: None,
    choices: &[],
    summary: "Transformer and native source to clean and stage. Repeat for a batch.",
};

const PARALLEL_ARG: Arg = Arg {
    name: "parallel",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: None,
    choices: &[],
    summary: "Concurrent desktop cleaning jobs (1-32); defaults to the app batch setting.",
};

const REPLACE_LOCAL_ARG: Arg = Arg {
    name: "replace-local",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Replace an existing unsaved local room instead of refusing it.",
};

pub static COMMAND: Command = Command {
    id: "map.design.upload.stage",
    path: &["map", "design", "upload", "stage"],
    contract: 1,
    summary: "Clean and stage an explicit native-source batch locally.",
    purpose: "\
Runs the desktop upload parser, LV target inference, header normalization, and \
Rust cleaning pipeline for each transformer/source pair. Successful rooms are \
staged locally and remain dirty; nothing is uploaded until a separate save is confirmed.",
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[SOURCE_ARG, PARALLEL_ARG, REPLACE_LOCAL_ARG, DESCRIPTOR_ARG],
    output: "\
Per-source success or failure with cleaned layer and feature counts, plus \
total/succeeded/failed counts. Successful rows report staged=true and persisted=false.",
    examples: &[Example {
        command: "ds map design upload stage --source TX-1=./tx1.zip --source TX-2=./tx2.xlsx --parallel 4 --output json",
        note: "Use --replace-local only after deliberately discarding any unsaved room for the named transformer.",
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
            code: "invalid_source",
            when: "a --source is not transformer=native-path",
            remedy: "pass e.g. --source TX-1042=./survey.zip",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut items = Vec::new();
    for raw in inputs.repeated("source") {
        let Some((transformer, path)) = raw.split_once('=') else {
            return Err(invalid_source(raw));
        };
        let transformer = transformer.trim();
        let path = path.trim();
        if transformer.is_empty() || path.is_empty() {
            return Err(invalid_source(raw));
        }
        items.push(json!({ "transformer": transformer, "path": path }));
    }
    let mut arguments = Map::new();
    arguments.insert("items".into(), Value::Array(items));
    if let Some(parallel) = inputs.value("parallel") {
        arguments.insert(
            "parallel".into(),
            json!(crate::integer(parallel, "parallel", 1, 32)?),
        );
    }
    if inputs.switch("replace-local") {
        arguments.insert("replaceLocal".into(), Value::Bool(true));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::DESIGN_UPLOAD_STAGE_BATCH,
        Value::Object(arguments),
        crate::DESIGN_PROCESS_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

fn invalid_source(raw: &str) -> Failure {
    Failure::invalid("invalid_source", format!("invalid upload source `{raw}`"))
        .remedy("pass e.g. --source TX-1042=./survey.zip")
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} source(s): {} staged, {} failed · {} parallel\n",
        data["total"].as_u64().unwrap_or(0),
        data["succeeded"].as_u64().unwrap_or(0),
        data["failed"].as_u64().unwrap_or(0),
        data["parallel"].as_u64().unwrap_or(1),
    );
    if let Some(rows) = data["rows"].as_array() {
        for row in rows {
            out.push_str(&format!(
                "  {} {} · {}{}\n",
                if row["ok"].as_bool().unwrap_or(false) {
                    "✓"
                } else {
                    "✗"
                },
                row["transformer"].as_str().unwrap_or("?"),
                row["path"].as_str().unwrap_or("source"),
                row["error"]
                    .as_str()
                    .map(|message| format!("  {message}"))
                    .unwrap_or_default(),
            ));
        }
    }
    out
}
