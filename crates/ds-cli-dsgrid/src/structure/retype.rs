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
use ds_grid_engine::{
    FINDING_STRUCTURE_TYPE_NOT_ALLOWED, GridCommand, StructureFinding, evaluate_structure_type,
    report_structures,
};
use ds_grid_model::StructureId;
use serde_json::{Value, json};

use crate::mutation::{self, Planned, Target};
use crate::structure::{resolve_structure, resolve_structure_type, structure_json, type_name};

const STRUCTURE_ARG: Arg = Arg::repeated(
    "structure",
    "<id|number>",
    "A placed structure by id (str-…) or unique engineering number; repeat for several.",
);
const SKIP_ARG: Arg = Arg::repeated(
    "skip",
    "<id|number>",
    "Leave this structure out of the selection (e.g. a T-off whose sets the new type lacks); repeat for several.",
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
        when: "a write would leave or create a single pole carrying 10° ≤ |line angle| < 60° (REG v7 angle-pole rule, EDCL drawing -04 not recommended)",
        remedy: "choose an H-pole type (family j, drawing W045S-A0101-11); detail lists each structure, its angle and the allowed families",
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
Retypes the selected placed structures to one structure type of the model \
(a single wooden pole on a big angle to the H-pole assembly, for instance) \
through the engine's `retype_structure` command, all of them as ONE revision \
of a working copy or one new package. Before anything is written the REG v7 \
angle-pole rule is evaluated for every selected structure at its line angle \
from the model's alignment geometry: the dry-run receipt lists the findings \
the retype clears and the ones it would leave or create, and a write that \
would leave or create `structure_type_not_allowed` is refused. \
`--from-finding structure_type_not_allowed` selects every structure carrying \
that finding, so a whole line is corrected in one dry run and one write.",
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
The family receipt plus `selection` (how each structure was selected), \
`structures[]` (id, number, station, line angle, type before → after, \
findings before → after), \
`findings` {cleared, remaining, created} under the REG rule with the standard's \
digest and the declared assumptions, and `refusal_on_write` when a dry run \
shows the write would be refused.",
    examples: &[
        Example {
            command: "ds dsgrid structure retype --model local-… --structure 230 --type j-w-60d-S325.014 --dry-run",
            note: "One structure; the receipt shows the rule findings before and after.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid structure retype --model local-… --from-finding structure_type_not_allowed --type j-w-60d-S190.012 --dry-run --output json",
            note: "Every single pole in the 10°–60° band, listed; nothing written.",
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
    let opened = mutation::open(target, inputs)?;
    let snapshot = opened.session.snapshot();

    let new_type = resolve_structure_type(snapshot, inputs.require("type")?)?;
    let new_type_name = type_name(snapshot, &new_type);

    // Selection: explicit structures, then the finding's carriers.
    let mut selected: Vec<(StructureId, &'static str)> = Vec::new();
    for raw in inputs.repeated("structure") {
        let row = resolve_structure(snapshot, raw)?;
        if !selected.iter().any(|(id, _)| *id == row.id) {
            selected.push((row.id.clone(), "explicit"));
        }
    }
    let mut report_identity = Value::Null;
    if let Some(finding) = inputs.value("from-finding") {
        let report = report_structures(snapshot, None).map_err(rules_error)?;
        report_identity = json!({
            "standard": report.standard,
            "assumptions": report.assumptions,
            "verification_level": report.verification_level,
        });
        for row in &report.rows {
            if row.findings.iter().any(|f| f.finding == finding)
                && !selected.iter().any(|(id, _)| *id == row.structure_id)
            {
                selected.push((row.structure_id.clone(), "from_finding"));
            }
        }
    }
    let mut skipped = Vec::new();
    for raw in inputs.repeated("skip") {
        let row = resolve_structure(snapshot, raw)?;
        if let Some(index) = selected.iter().position(|(id, _)| *id == row.id) {
            selected.remove(index);
            skipped.push(row.id.as_str().to_string());
        }
    }
    if selected.is_empty() {
        return Err(Failure::invalid(
            "structure_required",
            "no structure was selected",
        )
        .remedy("name at least one --structure, or a --from-finding that some structure carries")
        .next("ds dsgrid report structures"));
    }
    if report_identity.is_null() {
        let standard = ds_grid_engine::load_structure_rules_standard().map_err(rules_error)?;
        report_identity = json!({
            "standard": {
                "schema": standard.schema,
                "version": standard.version,
                "issued": standard.issued,
                "digest": standard.digest,
            },
            "assumptions": [{
                "rule_id": ds_grid_engine::SINGLE_POLE_ANGLE_BAND.rule_id,
                "value": format!("{}° <= |line angle| < {}°", ds_grid_engine::SINGLE_POLE_ANGLE_BAND.min_deg, ds_grid_engine::SINGLE_POLE_ANGLE_BAND.max_deg),
                "source_clause": ds_grid_engine::SINGLE_POLE_ANGLE_BAND.source_clause,
                "assumed": ds_grid_engine::SINGLE_POLE_ANGLE_BAND.assumed,
            }],
            "verification_level": ds_grid_engine::structure_rules::VERIFICATION_LEVEL,
        });
    }

    // Evaluate the rule before and after for every selected structure.
    let angles = ds_grid_engine::line_angles_by_structure(snapshot).map_err(rules_error)?;
    let mut structures = Vec::with_capacity(selected.len());
    let mut cleared: Vec<StructureFinding> = Vec::new();
    let mut remaining: Vec<StructureFinding> = Vec::new();
    let mut created: Vec<StructureFinding> = Vec::new();
    let mut warnings = Vec::new();
    let mut planned = Vec::with_capacity(selected.len());
    for (id, how) in &selected {
        let row = snapshot
            .structures
            .iter()
            .find(|row| row.id == *id)
            .expect("selected from this snapshot");
        let before = evaluate_structure_type(snapshot, id, None).map_err(rules_error)?;
        let after = evaluate_structure_type(snapshot, id, Some(&new_type)).map_err(rules_error)?;
        for finding in &before {
            if after.iter().any(|f| f.finding == finding.finding) {
                remaining.push(finding.clone());
            } else {
                cleared.push(finding.clone());
            }
        }
        for finding in &after {
            if !before.iter().any(|f| f.finding == finding.finding) {
                created.push(finding.clone());
            }
        }
        let current_type = type_name(snapshot, &row.structure_type_id);
        if row.structure_type_id == new_type {
            warnings.push(json!({
                "code": "type_unchanged",
                "structure": id.as_str(),
                "message": format!("{} is already {new_type_name}", id.as_str()),
            }));
        }
        let mut entry = structure_json(row, &current_type);
        entry["selected_by"] = json!(how);
        entry["line_angle_deg"] = json!(angles.get(id));
        entry["type_before"] = json!(current_type);
        entry["type_after"] = json!(new_type_name);
        entry["findings_before"] =
            json!(before.iter().map(|f| f.finding.clone()).collect::<Vec<_>>());
        entry["findings_after"] =
            json!(after.iter().map(|f| f.finding.clone()).collect::<Vec<_>>());
        structures.push(entry);
        planned.push(Planned {
            command_id: mutation::command_id(
                "retype",
                &opened.head,
                &format!("{}\0{}", id.as_str(), new_type.as_str()),
            ),
            command: GridCommand::RetypeStructure {
                id: id.clone(),
                structure_type_id: new_type.clone(),
            },
        });
    }

    let blocking: Vec<&StructureFinding> = remaining
        .iter()
        .chain(created.iter())
        .filter(|f| f.finding == FINDING_STRUCTURE_TYPE_NOT_ALLOWED)
        .collect();
    if writing && !blocking.is_empty() {
        return Err(Failure::invalid(
            "structure_type_not_allowed",
            format!(
                "retyping to {new_type_name} would leave or create {} single pole(s) in the 10°–60° band",
                blocking.len()
            ),
        )
        .remedy("choose an H-pole type (family j, drawing W045S-A0101-11) for these structures")
        .detail(json!({
            "rule": report_identity,
            "created": created,
            "remaining": remaining,
        })));
    }
    let refusal_on_write = (!blocking.is_empty()).then_some(FINDING_STRUCTURE_TYPE_NOT_ALLOWED);
    if let Some(code) = refusal_on_write {
        warnings.push(json!({
            "code": code,
            "message": format!(
                "a write would be refused: {} structure(s) would remain or become a single pole in the 10°–60° band",
                blocking.len()
            ),
        }));
    }

    let extra = json!({
        "type": { "id": new_type.as_str(), "name": new_type_name },
        "selection": {
            "explicit": selected.iter().filter(|(_, how)| *how == "explicit").count(),
            "from_finding": selected.iter().filter(|(_, how)| *how == "from_finding").count(),
            "finding": inputs.value("from-finding"),
            "skipped": skipped,
        },
        "structures": structures,
        "findings": {
            "rule": report_identity,
            "cleared": cleared,
            "remaining": remaining,
            "created": created,
        },
        "refusal_on_write": refusal_on_write,
    });
    // A retype changes the structure-file reference in the line file's
    // structure record; the structure files themselves are untouched.
    mutation::run(opened, planned, writing, extra, warnings, &["DON"])
}

fn rules_error(error: ds_grid_engine::StructureRulesError) -> Failure {
    match error {
        ds_grid_engine::StructureRulesError::RuleSourceMissing { expected, found } => {
            Failure::failed(
                "rule_source_missing",
                "the REG structure-rules standard is not the owner-issued bytes",
            )
            .remedy("rebuild ds from a clean checkout")
            .detail(json!({ "expected_digest": expected, "found_digest": found }))
        }
        other => Failure::failed(
            "operation_failed",
            "the structure rules could not be evaluated",
        )
        .remedy("read detail.engine")
        .detail(json!({ "engine": other.to_string() })),
    }
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
    out
}
