//! `ds design group preview` — decide the batch without writing anything.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::group::{GROUP_ARG, TRANSFORMERS_ARG};

pub const VALUE_ARG: Arg = Arg {
    name: "value",
    kind: ArgKind::Value,
    value: "<value>",
    required: false,
    default: None,
    choices: &[],
    summary: "A value from the group's vocabulary. Omit to preview the unassign.",
};

pub static COMMAND: Command = Command {
    id: "design.group.preview",
    path: &["design", "group", "preview"],
    contract: 1,
    summary: "Plan a tag-definition batch and return its fencing digest.",
    purpose: "\
Returns one explicit outcome per named transformer — assign, reassign, \
unchanged, unassign, already_absent, duplicate, or refused with a reason — \
plus the `digest` that `apply` and `unassign` must echo back. Nothing is \
written, so this stays available on a project that accepts no changes: seeing \
what WOULD happen is exactly what an operator needs before asking for one to \
be unarchived. Omitting --value previews the unassign rather than assigning an \
empty value. The value is checked against the project's vocabulary by exact \
bytes; `value_case_mismatch` names what you sent and the stored spelling.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[GROUP_ARG, TRANSFORMERS_ARG, VALUE_ARG, DESCRIPTOR_ARG],
    output: "\
The project, `group`, `operation`, the fencing `digest`, the `state`, counts of \
`changed`/`unchanged`/`refused`, any `outstanding` transformers, and one \
`outcomes` row per entry.",
    examples: &[Example {
        command: "ds design group preview --group city --transformers kigali_a,kigali_b --value kigali --output json",
        note: "Carry .data.digest into `ds design group apply`; it is not reusable after the project moves.",
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
        crate::UNKNOWN_TAG_GROUP,
        crate::INVALID_VALUE_LIST,
        crate::TOO_MANY,
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
    // The value is sent EXACTLY as given. Trimming or folding it here would
    // make `ds` accept a spelling the server refuses, and assign a value the
    // operator never typed.
    if let Some(value) = inputs.value("value") {
        arguments.insert("value".into(), json!(value));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::GROUP_PREVIEW,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    crate::group::render_plan(data)
}
