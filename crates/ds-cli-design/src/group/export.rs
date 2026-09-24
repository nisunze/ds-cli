//! `ds design group export` — an explicit digest-pinned tag projection.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::group::{PROJECTION_DEFINITION_IDS_ARG, PROJECTION_TRANSFORMERS_ARG};
use crate::{LANE_ARG, PROJECT_ARG};

pub static COMMAND: Command = Command {
    id: "design.group.export",
    path: &["design", "group", "export"],
    contract: 1,
    summary: "Export a digest-pinned projection for ordered tag definition IDs.",
    purpose: "\
Returns the read-only tag projection a report holds and pins for an explicit \
ordered definition-id selection and named transformer set, as exact bytes plus \
the sha256 over them. Missing, archived, or inapplicable selected definitions \
are refused by id; display names never select an authority. An empty selection \
deliberately means one untagged group. \
\
Write `.data.document` VERBATIM. Do not parse and re-serialize it: the digest \
is over those exact bytes, and a re-encoding that reorders a key or escapes a \
character differently no longer matches the pin.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECTION_TRANSFORMERS_ARG,
        PROJECTION_DEFINITION_IDS_ARG,
        PROJECT_ARG,
        LANE_ARG,
    ],
    output: "\
The project, the `schema`, ordered `definitionIds`, the \
`sha256` and `bytes` of the document, counts of `groups`/`assignments`/\
`transformers`, any values `excluded` from the document with why, and the \
`document` text itself.",
    examples: &[Example {
        command: "ds design group export --project <id> --transformers site_a,site_b --definition-ids service_region --output json",
        note: "Save `.data.document` verbatim and keep `.data.sha256` with it.",
        runnable: false,
    }],
    refusals: &crate::headless_refusals!(
        crate::NOT_PERMITTED,
        crate::INVALID_VALUE_LIST,
        crate::TOO_MANY,
    ),
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    // Bounded by the EXPORT, not by the write batch: one document has to be
    // able to cover a whole project, or the report pins two digests it will
    // not join.
    arguments.insert(
        "transformers".into(),
        json!(crate::group::projection_transformers(inputs)?),
    );
    let definition_ids = crate::group::projection_definition_ids(inputs)?;
    // The route publishes the hyphenated key; the kernel door validates it.
    arguments.insert("definition-ids".into(), json!(definition_ids));
    crate::headless::perform(
        "design.group.export",
        Value::Object(arguments),
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
    )
}

pub fn render(data: &Value) -> String {
    // The document itself is deliberately NOT printed here: it is bytes for a
    // file, and a text renderer that reflowed or truncated it would produce
    // something whose digest no longer matches. `--output json` carries it.
    let mut out = format!(
        "{} · {} bytes · sha256 {}\n",
        data["schema"].as_str().unwrap_or("?"),
        data["bytes"].as_u64().unwrap_or(0),
        data["sha256"].as_str().unwrap_or("?"),
    );
    out.push_str(&format!(
        "  {} groups, {} assignments over {} transformers · definitions {}\n",
        data["groups"].as_u64().unwrap_or(0),
        data["assignments"].as_u64().unwrap_or(0),
        data["transformers"].as_u64().unwrap_or(0),
        data["definitionIds"]
            .as_array()
            .map(|ids| ids
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(","))
            .unwrap_or_default(),
    ));
    for row in data["excluded"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  not in the document: {} / {} ({})\n",
            row["transformer"].as_str().unwrap_or("?"),
            row["definition"].as_str().unwrap_or("?"),
            row["reason"].as_str().unwrap_or("?"),
        ));
    }
    out
}
