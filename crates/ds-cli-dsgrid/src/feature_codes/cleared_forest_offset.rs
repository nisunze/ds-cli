//! Model-owned additional clearance at cleared forest survey points.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::feature_code_ops::{ClearedForestOffsetError, plan_cleared_forest_offset};
use serde_json::{Value, json};

use crate::mutation::{self, Planned};

const OWN: &[Refusal] = &[
    Refusal {
        code: "invalid_additional_clearance",
        when: "--additional-clearance-m is not a finite nonnegative number",
        remedy: "pass 0 for the ground-only default, or a nonnegative number of metres",
    },
    Refusal {
        code: "feature_code_unknown",
        when: "--code does not name an active feature code in this model",
        remedy: "read ds dsgrid feature-codes report at this revision",
    },
    Refusal {
        code: "feature_code_clearance_missing",
        when: "the code has no clearance facts",
        remedy: "import the issued feature-code standard for this voltage class",
    },
    Refusal {
        code: "not_cleared_forest",
        when: "the code is not FOREST_EDGE (51) or FOREST_INSIDE (52)",
        remedy: "select one of those two cleared forest codes",
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
    id: "dsgrid.feature-codes.cleared-forest-offset.set",
    path: &["dsgrid", "feature-codes", "cleared-forest-offset", "set"],
    contract: 1,
    summary: "Set added clearance over cleared forest ground (default 0 m).",
    purpose: "Authors one DS-only additional vertical clearance for FOREST_EDGE or FOREST_INSIDE above the model's GROUND rule at the surveyed point elevation. Survey h= and the issued standing-tree default do not become obstacles. The issued standard's FEA columns remain unchanged. One revision-gated model edit feeds spotting, clearance analysis, and Profile/print scenes.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::OUT_ARG,
        Arg::value("code", "<FOREST_EDGE|FOREST_INSIDE>", "The cleared forest feature code in this model.")
            .required()
            .choices(&["FOREST_EDGE", "FOREST_INSIDE"]),
        Arg::value("additional-clearance-m", "<metres>", "Nonnegative clearance added above GROUND at the surveyed forest Z; 0 removes the override.")
            .required(),
        mutation::REVISION_ARG,
        mutation::DRY_RUN_ARG,
        mutation::YES_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "The revision-pinned receipt, code, prior and proposed added clearance in metres, and whether the model changed.",
    examples: &[
        Example {
            command: "ds dsgrid feature-codes cleared-forest-offset set --model local-… --code FOREST_EDGE --additional-clearance-m 2.5 --dry-run --output json",
            note: "Preview one code's added clearance without changing surveyed elevations.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid feature-codes cleared-forest-offset set --model local-… --code FOREST_EDGE --additional-clearance-m 0 --yes --output json",
            note: "Return that code to the ground-only default.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["standing tree", "survey height", "fea columns"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let writing = mutation::write_mode(inputs, context)?;
    let code = inputs.require("code")?;
    let raw = inputs.require("additional-clearance-m")?;
    let metres = raw.parse::<f64>().map_err(|_| invalid_offset(raw))?;
    if !metres.is_finite() || metres < 0.0 {
        return Err(invalid_offset(raw));
    }
    let target = mutation::Target::resolve(inputs, writing)?;
    let opened = mutation::open(target, inputs)?;
    let plan = plan_cleared_forest_offset(opened.session.snapshot(), code, metres)
        .map_err(map_plan_error)?;
    let head = opened.head.clone();
    let extra = json!({
        "forest_code": plan.code_token,
        "feature_code": plan.code_number,
        "before_m": plan.before_m,
        "additional_clearance_m": plan.after_m,
        "surveyed_elevation": "unchanged",
        "clearance_basis": "GROUND.required_vertical_m + additional_clearance_m",
        "changed": !plan.commands.is_empty(),
    });
    if plan.commands.is_empty() {
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
            "warnings": [],
        });
        if let Value::Object(map) = extra {
            for (key, value) in map {
                receipt[key] = value;
            }
        }
        return Ok(receipt);
    }
    let planned = plan
        .commands
        .into_iter()
        .map(|command| Planned {
            command_id: mutation::command_id(
                "cleared-forest-offset",
                &head,
                &format!("{code}:{metres}"),
            ),
            command,
        })
        .collect();
    // This DS-only value has no FEA 15 column.
    mutation::run(opened, planned, writing, extra, Vec::new(), &[])
}

fn invalid_offset(raw: &str) -> Failure {
    Failure::invalid(
        "invalid_additional_clearance",
        format!("{raw:?} is not a finite nonnegative clearance in metres"),
    )
    .remedy("pass 0 for the ground-only default, or a nonnegative number of metres")
}

fn map_plan_error(error: ClearedForestOffsetError) -> Failure {
    match error {
        ClearedForestOffsetError::InvalidOffset => invalid_offset("offset"),
        ClearedForestOffsetError::CodeUnknown(code) => Failure::invalid(
            "feature_code_unknown",
            format!("{code:?} is not an active feature code in this model"),
        )
        .remedy("read ds dsgrid feature-codes report at this revision"),
        ClearedForestOffsetError::ClearanceMissing(code) => Failure::invalid(
            "feature_code_clearance_missing",
            format!("{code:?} has no clearance facts"),
        )
        .remedy("import the issued feature-code standard for this voltage class"),
        ClearedForestOffsetError::NotClearedForest(code) => Failure::invalid(
            "not_cleared_forest",
            format!("{code:?} is not a cleared forest code"),
        )
        .remedy("select FOREST_EDGE or FOREST_INSIDE"),
    }
}

pub fn render(data: &Value) -> String {
    let mut out = mutation::render_receipt(data);
    out.push_str(&format!(
        "{} (code {}) additional ground clearance: {} m → {} m; surveyed Z unchanged\n",
        data["forest_code"].as_str().unwrap_or("?"),
        data["feature_code"],
        data["before_m"],
        data["additional_clearance_m"],
    ));
    out
}
