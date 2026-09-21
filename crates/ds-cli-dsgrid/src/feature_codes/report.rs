//! `ds dsgrid feature-codes report` — the model's feature-code table: every
//! code with its number, required vertical/horizontal clearance, TIN flag,
//! usage; the unresolved survey tokens by name and count; the UNKNOWN rows a
//! delivery must not carry.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::feature_code_report;
use serde_json::{Value, json};

use crate::mutation;
use crate::package;

pub static COMMAND: Command = Command {
    id: "dsgrid.feature-codes.report",
    path: &["dsgrid", "feature-codes", "report"],
    contract: 1,
    summary: "List a model's feature codes, clearances, usage and unresolved tokens.",
    purpose: "\
Reads one DS Grid model (a working copy by id, or a .dsgrid file) and reports \
its feature-code table: each code's PLS-CADD number, required vertical and \
horizontal clearance, ground/obstacle (TIN) flag, voltage class, assumed \
values and how many surveyed points carry it; the survey tokens that resolve \
to no code, by name and count; and how many rows sit on UNKNOWN (999). This \
is the before/after evidence of a feature-code migration and the check that a \
delivery carries no unknown code.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
        Arg::value("limit", "<n>", "Cap the codes and unresolved tokens listed.")
            .default(package::DEFAULT_LIMIT),
    ],
    output: "\
The target, the table (codes with number, RV/RH, TIN, class, assumed, usage), \
the unresolved tokens with counts, resolved/unresolved/UNKNOWN totals and the \
voltage classes and standard namespaces the table carries. `more.truncated` \
names any list shortened by --limit.",
    examples: &[
        Example {
            command: "ds dsgrid feature-codes report --model local-e9b0ccbf92d7447b --output json",
            note: "The baseline of a PLS-imported working copy: 0 resolved, the survey tokens unresolved by name and count.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid feature-codes report --package ./nyamagabe.dsgrid --limit 100",
            note: "The same over an immutable package.",
            runnable: false,
        },
    ],
    refusals: super::READ_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["feature code", "feature codes", "clearance", "survey token", "fea"],
    requires: Requires::Server,
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = package::parse_limit(inputs.value("limit"))?;
    let (target, opened) = super::open_read_only(inputs)?;
    let report = feature_code_report(opened.session.snapshot());
    let (codes, codes_withheld) = package::take(report.codes.clone(), limit);
    let (unresolved, unresolved_withheld) = package::take(report.unresolved_tokens.clone(), limit);
    let mut answer = json!({
        "target": target,
        "engine": ds_grid_engine::ENGINE_VERSION,
        "code_count": report.code_count,
        "with_clearance_count": report.with_clearance_count,
        "assumed_count": report.assumed_count,
        "voltage_classes": report.voltage_classes,
        "namespaces": report.namespaces,
        "resolved_token_count": report.resolved_token_count,
        "unresolved_token_count": report.unresolved_token_count,
        "unresolved_row_count": report.unresolved_row_count,
        "unknown_code_rows": report.unknown_code_rows,
        "terrain_point_count": report.terrain_point_count,
        "breakline_count": report.breakline_count,
        "crossing_count": report.crossing_count,
        "codes": codes,
        "unresolved_tokens": unresolved,
        "staged": false,
        "persisted": false,
    });
    let mut truncated = Vec::new();
    if codes_withheld > 0 {
        truncated.push(json!({ "field": "codes", "total": report.code_count, "shown": limit, "withheld": codes_withheld, "limit": limit }));
    }
    if unresolved_withheld > 0 {
        truncated.push(json!({ "field": "unresolved_tokens", "total": report.unresolved_token_count, "shown": limit, "withheld": unresolved_withheld, "limit": limit }));
    }
    if !truncated.is_empty() {
        answer["more"] = json!({ "truncated": truncated });
    }
    Ok(answer)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "feature codes  {} ({} with clearances, {} assumed) classes {}\nresolved {} token(s); unresolved {} token(s) on {} row(s); UNKNOWN rows {}\n",
        data["code_count"],
        data["with_clearance_count"],
        data["assumed_count"],
        data["voltage_classes"]
            .as_array()
            .map(|classes| classes.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(","))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "—".to_string()),
        data["resolved_token_count"],
        data["unresolved_token_count"],
        data["unresolved_row_count"],
        data["unknown_code_rows"],
    );
    for code in data["codes"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:>4} {:<24} RV {:<5} RH {:<5} {:<8} used {}{}\n",
            code["code_number"].as_u64().map(|n| n.to_string()).unwrap_or_else(|| "—".to_string()),
            code["name"].as_str().unwrap_or("?"),
            code["required_vertical_m"].as_f64().map(|v| format!("{v}")).unwrap_or_else(|| "—".to_string()),
            code["required_horizontal_m"].as_f64().map(|v| format!("{v}")).unwrap_or_else(|| "—".to_string()),
            match code["tin_member"].as_bool() {
                Some(true) => "ground",
                Some(false) => "obstacle",
                None => "—",
            },
            code["usage"],
            if code["retired"].as_bool().unwrap_or(false) { " (retired)" } else { "" },
        ));
    }
    for token in data["unresolved_tokens"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  unresolved {:?} x{}\n",
            token["token"].as_str().unwrap_or("?"),
            token["count"]
        ));
    }
    if let Some(truncated) = data["more"]["truncated"].as_array() {
        for entry in truncated {
            out.push_str(&format!(
                "  … {} more {} withheld by --limit\n",
                entry["withheld"],
                entry["field"].as_str().unwrap_or("rows")
            ));
        }
    }
    out
}
