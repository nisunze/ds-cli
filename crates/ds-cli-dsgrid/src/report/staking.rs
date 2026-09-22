//! Canonical client MV staking table from one DS Grid model. Quantities are
//! computed in ds-grid-exchange; this command only selects, writes and reports.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_exchange::staking_table::{StakingTableOptions, build_staking_table};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::mutation;

const OUT_ARG: Arg = Arg::value(
    "out",
    "<staking.xlsx>",
    "Write a new two-header client staking workbook; never overwrites.",
)
.required();
const OPTIONS_ARG: Arg = Arg::value(
    "options",
    "<options.json>",
    "Optional project overrides: conductors, actual spans, location source, and allowances.",
);
const LIMIT_ARG: Arg = Arg::value("limit", "<n>", "Maximum warnings in the receipt.")
    .default(crate::package::DEFAULT_LIMIT);

const OWN: &[Refusal] = &[
    Refusal {
        code: "output_required",
        when: "--out is omitted",
        remedy: "name a new .xlsx file",
    },
    Refusal {
        code: "invalid_limit",
        when: "--limit is not a whole number in 1..5000",
        remedy: "pass a limit inside the range, or omit it",
    },
    Refusal {
        code: "output_format_unknown",
        when: "--out is not .xlsx",
        remedy: "name a new .xlsx file",
    },
    Refusal {
        code: "staking_options_invalid",
        when: "--options cannot be read or parsed",
        remedy: "provide a JSON file matching StakingTableOptions",
    },
    Refusal {
        code: "staking_projection_failed",
        when: "the model or project options cannot produce a canonical table",
        remedy: "read the reported validation issue and correct the model or options",
    },
    Refusal {
        code: "staking_write_failed",
        when: "the workbook cannot be encoded",
        remedy: "read the reported writer issue",
    },
];

const SHARED: &[Refusal] = &[
    mutation::REFUSALS[0],  // target_required
    mutation::REFUSALS[2],  // output_exists
    mutation::REFUSALS[3],  // output_parent_missing
    mutation::REFUSALS[6],  // local_model_not_found
    mutation::REFUSALS[7],  // local_model_ambiguous
    mutation::REFUSALS[8],  // local_model_store_unavailable
    mutation::REFUSALS[9],  // model_not_found
    mutation::REFUSALS[10], // not_a_dsgrid_package
    mutation::REFUSALS[11], // package_decode_failed
    mutation::REFUSALS[17], // output_unwritable
];

const REFUSALS: &[Refusal; OWN.len() + SHARED.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + SHARED.len()] {
    let mut all = [OWN[0]; OWN.len() + SHARED.len()];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < SHARED.len() {
        all[OWN.len() + shared] = SHARED[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.report.staking",
    path: &["dsgrid", "report", "staking"],
    contract: 1,
    summary: "Build the canonical MV staking table and quantity matrix.",
    purpose: "Build the client staking table from canonical structure names, comments 1–9, ahead spans, conductor set and transformer kVA. The fixed matrix has one row per structure and a numeric sum row for later BOQ references. Project choices are supplied as JSON options; unknown facts remain visible as warnings rather than being invented.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        OUT_ARG,
        OPTIONS_ARG,
        LIMIT_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "target, revision, rows, columns, warnings, options, artifact {path, bytes, sha256}",
    examples: &[Example {
        command: "ds dsgrid report staking --package design.dsgrid --out staking.xlsx",
        note: "Write a new client table with the shipped defaults.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "staking table",
        "mv matrix",
        "earthing",
        "transformer protection",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let out = inputs
        .value("out")
        .ok_or_else(|| Failure::invalid("output_required", "name a new .xlsx output file"))?;
    if !out.to_ascii_lowercase().ends_with(".xlsx") {
        return Err(
            Failure::invalid("output_format_unknown", "staking output must be .xlsx")
                .remedy("name a new .xlsx file"),
        );
    }
    crate::apply::validate_output_path(out)?;
    let limit = crate::package::parse_limit(inputs.value("limit"))?;
    let options: StakingTableOptions = match inputs.value("options") {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|error| {
                Failure::invalid("staking_options_invalid", format!("cannot read {path}"))
                    .detail(json!({"error": error.to_string()}))
            })?;
            serde_json::from_slice(&bytes).map_err(|error| {
                Failure::invalid("staking_options_invalid", format!("invalid {path}"))
                    .detail(json!({"error": error.to_string()}))
            })?
        }
        None => StakingTableOptions::default(),
    };
    let (target, path) = mutation::read_target_path(inputs)?;
    let source = crate::package::read_bytes(&path)?;
    let package = crate::package::decode(&path, &source)?;
    let revision = ds_grid_engine::GridSession::open(package.snapshot.clone())
        .current_revision()
        .revision_id
        .as_str()
        .to_string();
    let options_bytes = serde_json::to_vec(&options)
        .map_err(|error| Failure::internal("staking_options_invalid", error.to_string()))?;
    let table = build_staking_table(&package.snapshot, &options).map_err(|detail| {
        Failure::failed(
            "staking_projection_failed",
            "the staking table could not be built",
        )
        .detail(json!({"detail": detail}))
    })?;
    let bytes = table.to_xlsx().map_err(|detail| {
        Failure::failed(
            "staking_write_failed",
            "the staking workbook could not be encoded",
        )
        .detail(json!({"detail": detail}))
    })?;
    crate::apply::write_new(out, &bytes)?;
    Ok(json!({
        "target": target,
        "revision": revision,
        "rows": table.rows.len(),
        "columns": table.columns.len(),
        "warnings": table.warnings.iter().take(limit).collect::<Vec<_>>(),
        "more": {"withheld": table.warnings.len().saturating_sub(limit), "total": table.warnings.len()},
        "options": {
            "sha256": format!("sha256:{:x}", Sha256::digest(&options_bytes)),
            "conductor_by_span": options.conductor_by_span.len(),
            "conductor_by_cable": options.conductor_by_cable.len(),
            "actual_span_by_structure": options.actual_span_by_structure.len(),
            "admin_by_structure": options.admin_by_structure.len(),
            "location_from_comments": options.location_from_comments,
            "conductor_length_factor": options.conductor_length_factor,
            "strut_allowance_fraction": options.strut_allowance_fraction,
            "flying_allowance_fraction": options.flying_allowance_fraction,
        },
        "artifact": {
            "path": out,
            "byte_len": bytes.len(),
            "sha256": format!("sha256:{:x}", Sha256::digest(&bytes))
        },
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "staking table {} structures · {} columns · {} warnings\n{}\n",
        data["rows"],
        data["columns"],
        data["more"]["total"],
        data["artifact"]["path"].as_str().unwrap_or("?")
    )
}
