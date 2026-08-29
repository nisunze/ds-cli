//! `ds design group unassign` — clear a governed group's value.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{json, Map, Value};

use crate::group::{DIGEST_ARG, GROUP_ARG, TRANSFORMERS_ARG};
use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "design.group.unassign",
    path: &["design", "group", "unassign"],
    contract: 1,
    summary: "Clear a governed group's value on a set of transformers.",
    purpose: "\
Removes the group's value from every named transformer, fenced by the digest \
`ds design group preview` returned for the same set with no value. A \
transformer that carries nothing already is reported `already_absent` rather \
than refused — the batch is idempotent, so a retry after a partial run costs \
nothing. This is a separate command from `apply` for one reason: an unassign \
must never be reachable by forgetting a flag.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[GROUP_ARG, TRANSFORMERS_ARG, DIGEST_ARG, DESCRIPTOR_ARG],
    output: "The same plan shape `preview` returns, with the committed `state` and per-entry outcomes.",
    examples: &[Example {
        command: "ds design group unassign --group city --transformers kigali_a --digest <plan-digest> --yes",
        note: "Preview the same set with no --value to obtain the digest this expects.",
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
        crate::READ_ONLY,
        crate::CONFLICT,
        crate::group::PLAN_STALE,
        crate::UNKNOWN_TAG_GROUP,
        crate::INVALID_VALUE_LIST,
        crate::TOO_MANY,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("group".into(), json!(crate::group::group(inputs)?));
    arguments.insert(
        "transformers".into(),
        json!(crate::group::transformers(inputs)?),
    );
    arguments.insert("digest".into(), json!(inputs.require("digest")?));
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::GROUP_UNASSIGN,
        Value::Object(arguments),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    crate::group::render_plan(data)
}
