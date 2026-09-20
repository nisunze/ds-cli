//! `ds design group apply` — commit the plan that was previewed.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use ds_cli_contract::spec::{Arg, ArgKind};

use crate::LANE_ARG;
use crate::group::{DIGEST_ARG, GROUP_ARG, TRANSFORMERS_ARG};

/// Assigning REQUIRES a value. It is declared required here rather than shared
/// with `preview`, where omitting it is the meaningful way to plan an unassign:
/// a caller who forgets the flag on an apply is told what is missing by the
/// parser, and can never have the omission read as "clear the group".
const VALUE_ARG: Arg = Arg {
    name: "value",
    kind: ArgKind::Value,
    value: "<value>",
    required: true,
    default: None,
    choices: &[],
    summary: "A value from the group's vocabulary. Clearing is `unassign`.",
};

pub static COMMAND: Command = Command {
    id: "design.group.apply",
    path: &["design", "group", "apply"],
    contract: 1,
    summary: "Assign a tag definition's value across a set of transformers.",
    purpose: "\
Commits the plan `ds design group preview` returned, fenced by its digest: if \
the project moved since, the batch is refused rather than landing against a \
state nobody approved. The accepted entries commit together in one \
transaction, and one transformer at fault refuses only its own entry. \
Each landed outcome echoes the exact stored value in `to`; a case-only value \
mismatch is refused rather than rewritten. \
\
If the returned plan carries explicit model evidence, report its state and \
outstanding worklist exactly. Never infer model behavior from the definition id.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[GROUP_ARG, TRANSFORMERS_ARG, VALUE_ARG, DIGEST_ARG, LANE_ARG],
    output: "The same plan shape `preview` returns, with the committed `state` and per-entry outcomes.",
    examples: &[Example {
        command: "ds design group apply --group city --transformers kigali_a,kigali_b --value kigali --digest <plan-digest> --yes",
        note: "The digest comes from `ds design group preview`; without --yes dispatch refuses first.",
        runnable: false,
    }],
    refusals: &crate::headless_refusals!(
        crate::NOT_PERMITTED,
        crate::READ_ONLY,
        crate::CONFLICT,
        crate::group::PLAN_STALE,
        crate::UNKNOWN_TAG_GROUP,
        crate::INVALID_VALUE_LIST,
        crate::TOO_MANY,
        crate::CONFIRMATION_REQUIRED,
    ),
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("group".into(), json!(crate::group::group(inputs)?));
    arguments.insert(
        "transformers".into(),
        json!(crate::group::transformers(inputs)?),
    );
    // Assigning requires a value. Clearing is `unassign`, a different command,
    // so no omission can quietly turn an assign into a clear.
    arguments.insert("value".into(), json!(inputs.require("value")?));
    arguments.insert("digest".into(), json!(inputs.require("digest")?));
    crate::headless::perform(
        "design.group.apply",
        Value::Object(arguments),
        inputs.value("lane").unwrap_or("stable"),
    )
}

pub fn render(data: &Value) -> String {
    crate::group::render_plan(data)
}
