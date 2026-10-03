//! Count placed native DON structure assignments, by definition leaf.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_tasks::{backup_stringing_inventory, placed_structure_inventory};
use serde_json::Value;
use std::path::PathBuf;

pub static COMMAND: Command = Command {
    id: "pls.structure-inventory",
    path: &["pls", "structure-inventory"],
    contract: 1,
    summary: "Count placed structures in a PLS-CADD backup, DON or workspace.",
    purpose: "Reads DON design blocks through the native parser and counts actual placed rows by structure definition leaf. Keeps projects and active or historical blocks separate. With --stringing, reads one .bak without normalization and reports bounded section evidence: STRUCT set addresses, phase counts, dead-end flags and display labels, plus every PLS-CADD load-rule blocker count. No CRS or model import is required. A clean headless check does not grant native Restore/reopen acceptance.",
    chapter: Chapter::PlsCadd,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "source",
            "<bak|don|dir>",
            "Native backup, direct DON, or workspace folder.",
        )
        .required(),
        Arg::switch(
            "stringing",
            "Check native section endpoints and phase counts; requires a .bak.",
        ),
        Arg::value(
            "limit",
            "<1..5000>",
            "Cap stringing findings and sections; withheld counts stay explicit.",
        )
        .default("50"),
    ],
    output: "Placed counts by DON block, or --stringing: source and DON/STR/PPS digests, all blocker counts, bounded findings and section/set evidence, and explicit withheld counts.",
    examples: &[Example {
        command: "ds pls structure-inventory --source ./model.bak --output json",
        note: "Read placed rows without importing or changing the model.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "source_not_found",
            when: "--source does not exist",
            remedy: "pass an existing backup, DON, or workspace directory",
        },
        Refusal {
            code: "task_refused",
            when: "the native source or a DON design block cannot be read",
            remedy: "inspect the source and report the parser's exact detail",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    search: &[],
    requires: Requires::Server,
    availability: available,
};
fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = PathBuf::from(inputs.require("source")?);
    if !source.exists() {
        return Err(
            Failure::invalid("source_not_found", "source does not exist")
                .remedy("pass an existing backup, DON, or workspace directory"),
        );
    }
    let source = source
        .canonicalize()
        .map_err(|e| Failure::failed("task_refused", e.to_string()))?;
    let result = if inputs.switch("stringing") {
        let limit = inputs
            .value("limit")
            .unwrap_or("50")
            .parse::<usize>()
            .map_err(|_| Failure::invalid("task_refused", "limit must be 1..5000"))?;
        backup_stringing_inventory(&source, limit)
    } else {
        placed_structure_inventory(&source)
    };
    result.map_err(|e| {
        Failure::failed("task_refused", e)
            .remedy("inspect the source and report the parser's exact detail")
    })
}
pub fn render(data: &Value) -> String {
    if data.get("stringing").is_some() {
        return format!(
            "{} structures, {} sections, {} stringing blockers; native Restore/reopen unverified\n",
            data["stringing"]["structure_count"],
            data["stringing"]["section_count"],
            data["stringing"]["blocker_count"]
        );
    }
    let mut out = String::new();
    if let Some(projects) = data["projects"].as_array() {
        for project in projects {
            out.push_str(&format!(
                "{}\n",
                project["don_path"].as_str().unwrap_or("DON")
            ));
            if let Some(blocks) = project["blocks"].as_array() {
                for block in blocks {
                    out.push_str(&format!(
                        "  block {} active={} A-={} / {}\n",
                        block["index"],
                        block["active"],
                        block["a_family_placed"],
                        block["placed_total"]
                    ));
                }
            }
        }
    }
    out
}
