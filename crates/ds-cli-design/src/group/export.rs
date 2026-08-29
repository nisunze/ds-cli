//! `ds design group export` — the digest-pinned tag document a report groups by.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{json, Map, Value};

use crate::group::{PROJECTION_DEFINITION_IDS_ARG, PROJECTION_TRANSFORMERS_ARG};
use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "design.group.export",
    path: &["design", "group", "export"],
    contract: 1,
    summary: "Export the digest-pinned tag document a per-city report groups by.",
    purpose: "\
Returns the read-only tag projection a report holds and pins: the project's \
active group vocabularies and the values the named transformers carry, as the \
exact bytes plus the sha256 over them. A per-city report is grouped by the \
group named exactly `city`, so the export refuses when the project has none, \
or has two — neither is repairable, and guessing would decide a published \
total. `phasing` and any other active group are carried past untouched. \
\
Write `.data.document` VERBATIM. Do not parse and re-serialize it: the digest \
is over those exact bytes, and a re-encoding that reorders a key or escapes a \
character differently no longer matches the pin.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[PROJECTION_TRANSFORMERS_ARG, PROJECTION_DEFINITION_IDS_ARG, DESCRIPTOR_ARG],
    output: "\
The project, the `schema`, the `cityGroup` that will do the grouping, the \
`sha256` and `bytes` of the document, counts of `groups`/`assignments`/\
`transformers`, any values `excluded` from the document with why, and the \
`document` text itself.",
    examples: &[Example {
        command: "ds design group export --transformers kigali_a,kigali_b --output json",
        note: "Save it with: ds design group export --transformers ... --output json | jq -r .data.document > tags.json",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::NOT_PERMITTED,
        crate::INVALID_VALUE_LIST,
        crate::TOO_MANY,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
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
    arguments.insert("definition_ids".into(), json!(definition_ids));
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::GROUP_EXPORT,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
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
        "  {} groups, {} assignments over {} transformers · grouped by {}\n",
        data["groups"].as_u64().unwrap_or(0),
        data["assignments"].as_u64().unwrap_or(0),
        data["transformers"].as_u64().unwrap_or(0),
        data["cityGroup"].as_str().unwrap_or("?"),
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
