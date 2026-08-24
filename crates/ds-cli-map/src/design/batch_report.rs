//! `ds map design batch report` — export one declarative report deliverable.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{BOOL_CHOICES, DESCRIPTOR_ARG};

const TRANSFORMER_ARG: Arg = Arg {
    name: "transformer",
    kind: ArgKind::Repeated,
    value: "<name>",
    required: true,
    default: None,
    choices: &[],
    summary: "Transformer in the explicit report scope. Repeat 2..=200 times.",
};

const FILE_LEVEL_CHOICES: &[&str] = &["transformer", "sector", "district", "root"];

const FILE_LEVEL_ARG: Arg = Arg {
    name: "file-level",
    kind: ArgKind::Value,
    value: "<level>",
    required: false,
    default: Some("transformer"),
    choices: FILE_LEVEL_CHOICES,
    summary: "Folder level for individual artifacts inside the archive.",
};

const COMBINE_PER_DISTRICT_ARG: Arg = Arg {
    name: "combine-per-district",
    kind: ArgKind::Value,
    value: "<true|false>",
    required: false,
    default: Some("false"),
    choices: BOOL_CHOICES,
    summary: "Also include one district-scoped combined set in each district folder.",
};

pub static COMMAND: Command = Command {
    id: "map.design.batch.report",
    path: &["map", "design", "batch", "report"],
    contract: 1,
    summary: "Export one report archive for an explicit transformer batch.",
    purpose: "\
Declares an exact transformer scope and archive layout to the project report \
service. The service owns freshness and composition: fresh individual report \
artifacts are reused, missing or stale ones are regenerated, and the one \
scope-correct combined set is packaged with them. The CLI does not expose or \
replay those internal API phases.",
    effect: Effect::ArtifactWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        FILE_LEVEL_ARG,
        COMBINE_PER_DISTRICT_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
Project, resolved transformer count, archive layout, archive URL, individual \
artifact coverage, missing artifacts, report errors, and registry status.",
    examples: &[Example {
        command: "ds map design batch report --transformer TX-1 --transformer TX-2 --file-level sector --combine-per-district false --yes --output json",
        note: "One declaration produces the composed cloud deliverable; it does not loop report commands.",
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
        Refusal {
            code: "invalid_transformer_scope",
            when: "the explicit report scope contains fewer than 2 or more than 200 transformers",
            remedy: "repeat --transformer for every transformer in a 2..=200 item deliverable",
        },
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a command that writes report artifacts",
            remedy: "re-run with --yes once you intend the export",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformers = inputs.repeated("transformer");
    if transformers.len() < 2 || transformers.len() > 200 {
        return Err(Failure::invalid(
            "invalid_transformer_scope",
            "a report batch requires 2..=200 explicit transformers",
        )
        .remedy("repeat --transformer for every transformer in the intended deliverable")
        .detail(json!({ "given": transformers.len(), "min": 2, "max": 200 })));
    }

    let mut arguments = Map::new();
    arguments.insert("transformers".into(), json!(transformers));
    arguments.insert(
        "fileLevel".into(),
        json!(inputs.value("file-level").unwrap_or("transformer")),
    );
    arguments.insert(
        "combinePerDistrict".into(),
        json!(crate::boolean(inputs.value("combine-per-district"), false)),
    );

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::DESIGN_REPORT_BATCH,
        Value::Object(arguments),
        crate::DESIGN_PROCESS_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} · {} transformer(s) · {} archive(s)\n",
        data["status"].as_str().unwrap_or("unknown"),
        data["transformerCount"].as_u64().unwrap_or(0),
        data["archiveCount"].as_u64().unwrap_or(0),
    );
    out.push_str(&format!(
        "  individual coverage: {} · missing: {}\n",
        data["individualArtifactTransformerCount"]
            .as_u64()
            .unwrap_or(0),
        data["missingIndividualArtifactCount"].as_u64().unwrap_or(0),
    ));
    if let Some(urls) = data["urls"].as_array() {
        for url in urls {
            out.push_str(&format!("  {}\n", url.as_str().unwrap_or("?")));
        }
    }
    if let Some(errors) = data["errors"].as_array() {
        for error in errors {
            out.push_str(&format!("  error: {}\n", error.as_str().unwrap_or("?")));
        }
    }
    out
}
