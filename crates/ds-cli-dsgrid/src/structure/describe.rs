//! `ds dsgrid structure describe` — the description a structure list prints
//! for one placed structure (consultant point 1).

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{GridCommand, MAX_STRUCTURE_DESCRIPTION_CHARS};
use serde_json::{Value, json};

use crate::mutation::{self, Planned, Target};
use crate::structure::{id_of, resolve_structure, structure_json, type_name};

const STRUCTURE_ARG: Arg = Arg::value(
    "structure",
    "<id|number>",
    "The placed structure: its id (str-…) or its unique engineering number.",
)
.required();
const TEXT_ARG: Arg = Arg::value(
    "text",
    "<line>",
    "One line, at most 500 characters: assembly and drawing number, pole height and material, angle band, stays.",
);
const CLEAR_ARG: Arg = Arg::switch("clear", "Remove the description instead of setting one.");

const OWN: &[Refusal] = &[
    Refusal {
        code: "structure_unknown",
        when: "--structure is neither a structure id nor a unique engineering number in this revision",
        remedy: "use an id or number from `ds dsgrid report structures`",
    },
    Refusal {
        code: "text_required",
        when: "neither --text nor --clear was given, or both were",
        remedy: "pass --text \"…\" to describe, or --clear to remove",
    },
    Refusal {
        code: "target_not_found",
        when: "the engine no longer finds the structure at the pinned revision",
        remedy: "re-read the revision and name the structure again",
    },
];

const REFUSALS: &[Refusal; OWN.len() + mutation::REFUSALS.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + mutation::REFUSALS.len()] {
    let mut all = [OWN[0]; OWN.len() + mutation::REFUSALS.len()];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < mutation::REFUSALS.len() {
        all[OWN.len() + shared] = mutation::REFUSALS[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.structure.describe",
    path: &["dsgrid", "structure", "describe"],
    contract: 1,
    summary: "Set the description the structure list prints for one structure.",
    purpose: "\
Writes the free-text description of ONE placed structure — the line a \
consultant reads in the structure list and the staking table (assembly name \
and drawing number, pole height and material, angle band, stays) — as the \
next revision of a working copy, or as a new package from an immutable one. \
The text is the structure's own, distinct from its library type's \
description, and is exported into the native structure record on sync. \
Nothing is parsed out of it: family, height and class come from the \
structure type. One engine command, `describe_structure`, revision-gated; \
--dry-run first, --yes to write.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::OUT_ARG,
        STRUCTURE_ARG,
        TEXT_ARG,
        CLEAR_ARG,
        mutation::REVISION_ARG,
        mutation::DRY_RUN_ARG,
        mutation::YES_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "\
The family receipt: target, engine operation with its descriptor digest, \
source → resulting revision, counts touched, warnings, `pls_source` (empty \
until the copy is linked), plus `structure` before and after. A write adds \
`artifact` with the working copy's new package revision and digest.",
    examples: &[
        Example {
            command: "ds dsgrid structure describe --model local-… --structure 230 --text \"W045S-A0101-11 MV H-Poles Assembly, 12 m wooden, 10–60°, 2 stays\" --dry-run",
            note: "Evaluate against the working copy's head; nothing is written.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid structure describe --model local-… --structure str-…-230 --text \"…\" --yes --output json",
            note: "Write the next revision in place; the receipt names it.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "structure list",
        "staking table",
        "comment",
        "label",
        "pole information",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let writing = mutation::write_mode(inputs, context)?;
    let text = inputs
        .value("text")
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let clear = inputs.switch("clear");
    let description = match (text, clear) {
        (Some(text), false) => Some(text.to_owned()),
        (None, true) => None,
        _ => {
            return Err(
                Failure::invalid("text_required", "pass exactly one of --text or --clear")
                    .remedy("--text \"…\" sets the description; --clear removes it"),
            );
        }
    };
    if let Some(text) = &description
        && text.chars().count() > MAX_STRUCTURE_DESCRIPTION_CHARS
    {
        return Err(Failure::invalid(
            "command_invalid",
            format!(
                "--text is {} characters; the bound is {MAX_STRUCTURE_DESCRIPTION_CHARS}",
                text.chars().count()
            ),
        )
        .remedy("shorten the description to one line"));
    }

    let target = Target::resolve(inputs, writing)?;
    let opened = mutation::open(target, inputs)?;
    let snapshot = opened.session.snapshot();
    let structure = resolve_structure(snapshot, inputs.require("structure")?)?;
    let before = structure_json(
        structure,
        &type_name(snapshot, &structure.structure_type_id),
    );
    let id = id_of(structure);
    let mut warnings = Vec::new();
    if structure.description == description {
        warnings.push(json!({
            "code": "description_unchanged",
            "message": "the structure already carries exactly this description",
        }));
    }
    let planned = vec![Planned {
        command_id: mutation::command_id("describe", &opened.head, id.as_str()),
        command: GridCommand::DescribeStructure {
            id,
            description: description.clone(),
        },
    }];
    let mut after = before.clone();
    after["description"] = json!(description);
    // The description lives in the native structure record of the line file.
    mutation::run(
        opened,
        planned,
        writing,
        json!({ "structure": { "before": before, "after": after } }),
        warnings,
        &["DON"],
    )
}

pub fn render(data: &Value) -> String {
    let mut out = mutation::render_receipt(data);
    out.push_str(&format!(
        "structure {} ({})\n  was  {}\n  now  {}\n",
        data["structure"]["after"]["structure"]
            .as_str()
            .unwrap_or("?"),
        data["structure"]["after"]["structure_type"]
            .as_str()
            .unwrap_or("?"),
        data["structure"]["before"]["description"]
            .as_str()
            .unwrap_or("—"),
        data["structure"]["after"]["description"]
            .as_str()
            .unwrap_or("—"),
    ));
    out
}
