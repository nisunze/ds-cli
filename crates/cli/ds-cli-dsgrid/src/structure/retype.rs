//! `ds dsgrid structure retype` — change the structure type of one or many
//! placed structures as one revision (consultant point 2).
//!
//! The engine's `retype_structure` command per structure, applied as one
//! transaction, with the REG structure rule evaluated before anything is
//! written: a retype that would leave or CREATE a single pole in the
//! not-approved angle band is named on the dry-run receipt and refused on
//! the write. `--from-finding structure_type_not_allowed` selects every
//! structure carrying that finding in the target revision, so the whole line
//! is corrected in one revision after one dry run.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_contracts::grid_operation::TypedMutationVerbV1;
use serde_json::Value;

use crate::mutation::{self, Target};

/// The one REG finding a selection may be drawn from; a hand copy held to
/// the owner by `finding_choice_matches_the_owner`.
const FINDING_STRUCTURE_TYPE_NOT_ALLOWED: &str = "structure_type_not_allowed";

const STRUCTURE_ARG: Arg = Arg::repeated(
    "structure",
    "<id|number>",
    "A placed structure by id (str-…) or unique engineering number; repeat for several.",
);
const SKIP_ARG: Arg = Arg::repeated(
    "skip",
    "<id|number>",
    "Leave this structure out of the selection; repeat for several.",
);
const FROM_FINDING_ARG: Arg = Arg::value(
    "from-finding",
    "<finding>",
    "Also retype every structure carrying this finding in the target revision.",
)
.choices(&[FINDING_STRUCTURE_TYPE_NOT_ALLOWED]);
const TYPE_ARG: Arg = Arg::value(
    "type",
    "<type-id|name>",
    "The structure type to give them: a type id (st-…) or an exact library name in this model, e.g. j-w-60d-S325.014.",
)
.required();

const OWN: &[Refusal] = &[
    Refusal {
        code: "structure_unknown",
        when: "a --structure is neither a structure id nor a unique engineering number in this revision",
        remedy: "use ids or numbers from `ds dsgrid report structures`",
    },
    Refusal {
        code: "structure_required",
        when: "no structure was selected: no --structure, and --from-finding matched none",
        remedy: "name at least one structure, or check `ds dsgrid report structures` for the finding",
    },
    Refusal {
        code: "structure_type_unknown",
        when: "--type is neither a type id nor a library name in this model",
        remedy: "use a name from the model's structure-type list (detail lists them)",
    },
    Refusal {
        code: "structure_type_ambiguous",
        when: "--type names a library name held by two types of this model",
        remedy: "name the type by its id (st-…)",
    },
    Refusal {
        code: "structure_type_not_allowed",
        when: "a write would leave or create an unsupported single pole carrying 10° ≤ |line angle| < 60° (REG v7 angle-pole rule, EDCL drawing -04 not recommended)",
        remedy: "choose an H-pole or a stayed steel strain assembly; detail lists each structure, its angle and the allowed families",
    },
    Refusal {
        code: "rule_source_missing",
        when: "the vendored REG structure-rules standard is not the owner-issued bytes",
        remedy: "rebuild ds from a clean checkout; report the digests in detail",
    },
    Refusal {
        code: "target_not_found",
        when: "the engine no longer finds a structure at the pinned revision",
        remedy: "re-read the revision and select again",
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
    id: "dsgrid.structure.retype",
    path: &["dsgrid", "structure", "retype"],
    contract: 1,
    summary: "Retype placed structures (single pole → H-pole) as one revision.",
    purpose: "\
Retype selected structures through native `retype_structure` as one working-copy \
revision or new package. REG v7 uses \
each structure's alignment angle: dry-run lists cleared, remaining and created \
findings; writes that leave or create `structure_type_not_allowed` are refused. \
`--from-finding structure_type_not_allowed` selects all carriers. Exact (set, \
slot) matches rebound; a duty change splits or rejoins sets through the new \
type's characterized sets; otherwise unmatched attachments clear as a native \
draft. Repair those before full validation, export or \
publication. Package persistence requires a valid snapshot; a draft write may \
fail `package_emit_failed` with `detail.receipt`.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::OUT_ARG,
        STRUCTURE_ARG,
        FROM_FINDING_ARG,
        SKIP_ARG,
        TYPE_ARG,
        mutation::REVISION_ARG,
        mutation::DRY_RUN_ARG,
        mutation::YES_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "\
Family receipt with `selection`, `structures[]` (id, number, station, angle, \
before/after type and findings), `findings` {rule, cleared, remaining, created} \
with standard digest/assumptions, and `refusal_on_write` for REG duty. When \
present, `deltas[].unresolved_retype_attachments` carries native section_id, \
sequence, structure_id, previous_attachment_point_id, set_label and slot with \
repair guidance in `warnings[]`. Draft package emission failure retains the receipt \
in `detail.receipt` with `persisted: false`.",
    examples: &[
        Example {
            command: "ds dsgrid structure retype --model local-… --structure 230 --type j-w-60d-S325.014 --dry-run",
            note: "One structure; the receipt shows the rule findings before and after.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid structure retype --model local-… --from-finding structure_type_not_allowed --type j-w-60d-S190.012 --dry-run --output json",
            note: "Every unsupported single pole in the 10°–60° band, listed; nothing written.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid structure retype --model local-… --from-finding structure_type_not_allowed --type j-w-60d-S190.012 --yes",
            note: "The same selection written as one revision.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "h-pole",
        "single pole",
        "angle",
        "structure type",
        "change type",
        "replace structure",
        "deviation",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let writing = mutation::write_mode(inputs, context)?;
    let target = Target::resolve(inputs, writing)?;
    // A retype changes the structure-file reference in the line file's
    // structure record; the structure files themselves are untouched.
    mutation::run(
        target,
        TypedMutationVerbV1::StructureRetype {
            structures: inputs.repeated("structure").to_vec(),
            skip: inputs.repeated("skip").to_vec(),
            from_finding: inputs.value("from-finding").map(str::to_owned),
            structure_type: inputs.require("type")?.to_owned(),
        },
        inputs,
        writing,
        &[],
    )
}

pub fn render(data: &Value) -> String {
    let mut out = mutation::render_receipt(data);
    out.push_str(&format!(
        "type     -> {}\n",
        data["type"]["name"].as_str().unwrap_or("?")
    ));
    for row in data["structures"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<28} #{:<5} angle {:>9}  {} -> {}  {}\n",
            row["structure"].as_str().unwrap_or("?"),
            row["number"].as_str().unwrap_or("—"),
            row["line_angle_deg"]
                .as_f64()
                .map(|a| format!("{a:.4}°"))
                .unwrap_or_else(|| "—".into()),
            row["type_before"].as_str().unwrap_or("?"),
            row["type_after"].as_str().unwrap_or("?"),
            row["findings_after"]
                .as_array()
                .filter(|f| !f.is_empty())
                .map(|f| format!(
                    "[{}]",
                    f.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(",")
                ))
                .unwrap_or_default(),
        ));
    }
    out.push_str(&format!(
        "findings cleared {} · remaining {} · created {}\n",
        data["findings"]["cleared"].as_array().map_or(0, Vec::len),
        data["findings"]["remaining"].as_array().map_or(0, Vec::len),
        data["findings"]["created"].as_array().map_or(0, Vec::len),
    ));
    for delta in data["deltas"].as_array().into_iter().flatten() {
        for finding in delta["unresolved_retype_attachments"]
            .as_array()
            .into_iter()
            .flatten()
        {
            out.push_str(&format!(
                "unresolved  section {} sequence {} structure {} previous attachment {} set {} slot {}\n",
                finding["section_id"].as_str().unwrap_or("?"),
                finding["sequence"],
                finding["structure_id"].as_str().unwrap_or("?"),
                finding["previous_attachment_point_id"].as_str().unwrap_or("?"),
                finding["set_label"].as_str().unwrap_or("?"),
                finding["slot"],
            ));
        }
    }
    out
}

#[cfg(test)]
#[path = "retype_tests.rs"]
mod tests;
