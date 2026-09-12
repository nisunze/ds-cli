//! `ds dsgrid model prepare-project` — verify and fill governed MV heads.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Authority, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::model::{
    AMBIGUOUS, AUTH_CONTEXT_MISMATCH, DESCRIPTOR_ARG, LOCAL_TIMEOUT, NOT_PAIRED, PAIRING_REJECTED,
    PROJECT_NOT_OPEN, REFUSED, SIGNED_OUT, UNREACHABLE, UNREADABLE, UNSUPPORTED,
};

const DOWNLOAD_MISSING_ARG: Arg = Arg {
    name: "download-missing",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Download and verify missing exact MV heads sequentially in the shared Desktop cache.",
};

pub static COMMAND: Command = Command {
    id: "dsgrid.model.prepare-project",
    path: &["dsgrid", "model", "prepare-project"],
    contract: 1,
    summary: "Show or prepare the active project's exact DS Grid MV heads.",
    purpose: "Reads every governed MV model in the paired application's active project and reports whether its exact immutable head is cached. With --download-missing, fills missing heads one at a time. Design and physical printing consume this same verified cache; model bytes never cross the CLI bridge.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[DOWNLOAD_MISSING_ARG, DESCRIPTOR_ARG],
    output: "Project, total and ready counts, completeness, and one bounded row per model with id, name, revision, digest, byte length and cached status.",
    examples: &[],
    refusals: &[
        NOT_PAIRED,
        PROJECT_NOT_OPEN,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        UNSUPPORTED,
        UNREADABLE,
        SIGNED_OUT,
        AUTH_CONTEXT_MISMATCH,
    ],
    reference: Some("docs/reference/dsgrid.md"),
    availability: crate::model::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert(
        "downloadMissing".into(),
        json!(inputs.switch("download-missing")),
    );
    let descriptor = crate::model::paired(inputs.value("desktop-descriptor"))?;
    crate::model::invoke(
        &descriptor,
        &crate::model::MODEL_PREPARE_PROJECT,
        Value::Object(arguments),
        LOCAL_TIMEOUT,
    )
    .map_err(crate::model::classify)
}

pub fn render(data: &Value) -> String {
    let ready = data["ready"].as_u64().unwrap_or(0);
    let total = data["total"].as_u64().unwrap_or(0);
    let mut out = format!("{ready}/{total} project MV model heads ready offline\n");
    for row in data["models"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        out.push_str(&format!(
            "  {} · {} · {} bytes · {}\n",
            row["name"].as_str().unwrap_or("unnamed"),
            row["revision"].as_str().unwrap_or("unknown revision"),
            row["byte_length"].as_u64().unwrap_or(0),
            if row["cached"].as_bool().unwrap_or(false) {
                "ready offline"
            } else {
                "missing"
            },
        ));
    }
    out
}
