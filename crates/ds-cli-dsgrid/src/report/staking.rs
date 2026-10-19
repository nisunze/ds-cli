//! Canonical client MV staking table from one DS Grid model. Quantities are
//! computed in ds-grid-exchange; this command only selects, writes and reports.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_contracts::grid_operation::{GridOperationV1, ReportStakingParamsV1};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{mutation, native};

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
    mutation::refusal("target_required"),
    mutation::refusal("output_exists"),
    mutation::refusal("output_parent_missing"),
    mutation::refusal("local_model_not_found"),
    mutation::refusal("local_model_ambiguous"),
    mutation::refusal("local_model_store_unavailable"),
    mutation::refusal("model_not_found"),
    mutation::refusal("not_a_dsgrid_package"),
    mutation::refusal("package_decode_failed"),
    mutation::refusal("output_unwritable"),
];

const REFUSALS: &[Refusal; OWN.len() + SHARED.len()] = &splice();
const ALL_REFUSALS: &[Refusal] =
    &native::extend_refusals::<{ REFUSALS.len() + native::REFUSALS.len() }>(REFUSALS);
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
    refusals: ALL_REFUSALS,
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
    // The options file is read here; its StakingTableOptions shape is the
    // owner's and is admitted by the addon.
    let options = match inputs.value("options") {
        Some(path) => Some((
            path,
            std::fs::read(path).map_err(|error| {
                Failure::invalid("staking_options_invalid", format!("cannot read {path}"))
                    .detail(json!({"error": error.to_string()}))
            })?,
        )),
        None => None,
    };
    let (target, path) = mutation::read_target_path(inputs)?;
    let source = crate::package::read_bytes(&path)?;
    let (params, staged) = match options {
        Some((label, bytes)) => (
            ReportStakingParamsV1 {
                options_input: Some("options.json".into()),
                options_label: Some(label.to_owned()),
            },
            vec![("options.json".to_owned(), bytes)],
        ),
        None => (
            ReportStakingParamsV1 {
                options_input: None,
                options_label: None,
            },
            Vec::new(),
        ),
    };
    let mut outcome = native::operation(
        &GridOperationV1::ReportStaking(params),
        Some(&source),
        &staged,
    )
    .map_err(|failure| native::at_path(failure, &path))?;
    let table = std::mem::take(&mut outcome.result.result);
    let bytes = outcome.take_artifact("staking.xlsx").ok_or_else(|| {
        Failure::failed(
            "network_result_invalid",
            "the staking table returned no workbook",
        )
    })?;
    crate::apply::write_new(out, &bytes)?;
    let warnings = table["warnings"].as_array().cloned().unwrap_or_default();
    Ok(json!({
        "target": target,
        "revision": table["revision"],
        "rows": table["rows"],
        "columns": table["columns"],
        "warnings": warnings.iter().take(limit).collect::<Vec<_>>(),
        "more": {"withheld": warnings.len().saturating_sub(limit), "total": warnings.len()},
        "options": table["options"],
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
