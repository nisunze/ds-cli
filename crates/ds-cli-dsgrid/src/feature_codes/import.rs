//! `ds dsgrid feature-codes import` — load the standard's codes with one
//! voltage class's required clearances into a model, as one revision.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{StandardImportOptions, plan_standard_import};
use serde_json::{Value, json};

use crate::mutation::{self, Planned};

const OWN: &[Refusal] = &[
    super::STANDARD_REFUSALS[0],
    super::STANDARD_REFUSALS[1],
    super::STANDARD_REFUSALS[2],
    super::STANDARD_REFUSALS[3],
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
    id: "dsgrid.feature-codes.import",
    path: &["dsgrid", "feature-codes", "import"],
    contract: 1,
    summary: "Load the standard's 60 feature codes with a class's REG clearances.",
    purpose: "\
Authors (or updates, by name) every code of the DS PLS-CADD feature-code \
standard in the model, each carrying the required vertical and horizontal \
clearance of the chosen voltage class — REG Reticulation Standard v7 Table \
15 for MV, Table 16 for LV, the Transmission standard's Tables 7–8 for HV — \
its ground/obstacle kind, default obstacle height and the standard's assumed \
flag. Without --merge, definitions the standard does not name are retired \
(superseded by the code the standard's legacy map points to, when it has \
one); with --merge they stay. The standard's digest is verified; its values \
are never edited. One revision of a working copy, or one new package.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::OUT_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
        super::STANDARD_ARG,
        super::VOLTAGE_CLASS_ARG.required(),
        Arg::switch(
            "merge",
            "Keep the model's other definitions instead of retiring them.",
        ),
        mutation::REVISION_ARG,
        mutation::DRY_RUN_ARG,
        mutation::YES_ARG,
    ],
    output: "\
The family receipt (target, revisions, operations, touched counts) plus the \
standard's identity and digest, the class and its clearance voltage, the \
codes added, updated, retired, and the codes whose clearance for the class is \
an assumption to confirm.",
    examples: &[
        Example {
            command: "ds dsgrid feature-codes import --model local-e9b0ccbf92d7447b --standard pls-feature-codes.v1.json --voltage-class MV --dry-run --output json",
            note: "See what the MV table would add to the working copy.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid feature-codes import --model local-e9b0ccbf92d7447b --voltage-class MV --yes",
            note: "Load the 60 MV codes as the working copy's next revision.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "feature code",
        "feature codes",
        "reg clearances",
        "table 15",
        "voltage class",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let writing = mutation::write_mode(inputs, context)?;
    let standard = super::load_standard(inputs)?;
    let voltage_class = super::voltage_class(inputs, &standard)?;
    let target = mutation::Target::resolve(inputs, writing)?;
    let opened = mutation::open(target, inputs)?;
    let plan = plan_standard_import(
        opened.session.snapshot(),
        &standard,
        &StandardImportOptions {
            voltage_class: voltage_class.clone(),
            merge: inputs.switch("merge"),
        },
    )
    .map_err(super::map_standard_error)?;
    let head = opened.head.clone();
    let planned: Vec<Planned> = plan
        .commands
        .iter()
        .enumerate()
        .map(|(index, command)| Planned {
            command_id: mutation::command_id(
                "feature-codes-import",
                &head,
                &format!("{voltage_class}:{}:{index}", standard.version),
            ),
            command: command.clone(),
        })
        .collect();
    let mut warnings = Vec::new();
    if !standard.pinned {
        warnings.push(json!({
            "code": "standard_unpinned",
            "message": format!("standard version {} is not one this build pins; its digest was not verified against an issued file", standard.version),
        }));
    }
    if !plan.assumed.is_empty() {
        warnings.push(json!({
            "code": "assumed_clearances",
            "message": format!("{} code(s) carry an assumed {} clearance to confirm with the client: {}", plan.assumed.len(), voltage_class, plan.assumed.join(", ")),
        }));
    }
    let extra = json!({
        "standard": super::standard_receipt(&standard),
        "voltage_class": voltage_class,
        "clearance_voltage_kv": plan.clearance_voltage_kv,
        "merge": inputs.switch("merge"),
        "added": plan.added,
        "updated": plan.updated,
        "retired": plan.retired,
        "assumed": plan.assumed,
    });
    if planned.is_empty() {
        // Nothing to change: the table already carries this class. The
        // receipt says so instead of journaling an empty revision.
        let mut receipt = json!({
            "target": match &opened.target {
                mutation::Target::WorkingCopy { row, .. } => json!({ "kind": "working_copy", "model": row.id }),
                mutation::Target::Package { path, .. } => json!({ "kind": "package", "path": path }),
            },
            "dry_run": !writing,
            "persisted": false,
            "commands": 0,
            "source_revision": head.as_str(),
            "resulting_revision": head.as_str(),
            "changed": false,
            "warnings": warnings,
        });
        if let Value::Object(map) = extra {
            for (key, value) in map {
                receipt[key] = value;
            }
        }
        return Ok(receipt);
    }
    mutation::run(opened, planned, writing, extra, warnings, &["FEA"])
}

pub fn render(data: &Value) -> String {
    let mut out = mutation::render_receipt(data);
    out.push_str(&format!(
        "standard {} {} ({}pinned)  class {} ({} kV)\nadded {}  updated {}  retired {}  assumed {}\n",
        data["standard"]["name"].as_str().unwrap_or("?"),
        data["standard"]["version"].as_str().unwrap_or("?"),
        if data["standard"]["pinned"].as_bool().unwrap_or(false) {
            ""
        } else {
            "un"
        },
        data["voltage_class"].as_str().unwrap_or("?"),
        data["clearance_voltage_kv"],
        data["added"].as_array().map_or(0, Vec::len),
        data["updated"].as_array().map_or(0, Vec::len),
        data["retired"].as_array().map_or(0, Vec::len),
        data["assumed"].as_array().map_or(0, Vec::len),
    ));
    out
}
