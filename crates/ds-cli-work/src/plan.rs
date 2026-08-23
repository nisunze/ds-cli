//! `ds work plan` — the project's plan in one screen.
//!
//! The cheapest useful question a caller can ask about Project Work, and the
//! one that answers "what should I look at": the Dashboard's own rollups, the
//! progress of each discipline, and the items whose state has earned
//! attention — blocked first, then late, then paused, then in review, then
//! carrying an open residual.
//!
//! It also publishes the project's field-model vocabulary, which is what makes
//! the write commands usable without reading anything else: `--delivery`,
//! `--review` and `--closeout` take values from here, and the engine — not
//! this CLI — decides what they are.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("10"),
    choices: &[],
    summary: "Attention and recent rows to return (1-100). Totals are always reported.",
};

pub static COMMAND: Command = Command {
    id: "work.plan",
    path: &["work", "plan"],
    contract: 1,
    summary: "The plan's rollups, phases, attention list and vocabulary.",
    purpose: "\
Start here. Returns the same rollups the Dashboard renders — plan nodes, \
overall progress, what is in progress, blocked, late, under review, and how \
many open residuals block acceptance or closeout — plus progress by \
discipline, the items that have earned attention, and the field-model \
vocabulary the write commands take their state values from.",
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[LIMIT_ARG, DESCRIPTOR_ARG],
    output: "\
`project`, `revision`, `today`, `dashboard` with the rollups, `phases` by \
discipline, `attention` rows with their magnitude, `recent` task changes, \
full `phaseTotal`/`attentionTotal`/`recentTotal` counts for those bounded lists, \
`permissions` for the signed-in user, and `vocabulary` — the delivery, review \
and closeout states this project's engine accepts.",
    examples: &[Example {
        command: "ds work plan --output json",
        note: "Read .data.vocabulary before calling `ds work task update --delivery`.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::WORK_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/work.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    if let Some(limit) = inputs.value("limit") {
        arguments.insert(
            "limit".into(),
            json!(crate::integer(limit, "limit", 1, 100)?),
        );
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PLAN_READ,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_work_failure)
}

pub fn render(data: &Value) -> String {
    let dashboard = &data["dashboard"];
    let percent = |key: &str| (dashboard[key].as_f64().unwrap_or(0.0) * 100.0).round() as u64;
    let count = |key: &str| dashboard[key].as_u64().unwrap_or(0);

    let mut out = format!(
        "{} · revision {} · {}% complete over {}\n",
        data["project"].as_str().unwrap_or("?"),
        data["revision"].as_u64().unwrap_or(0),
        percent("overallProgress"),
        crate::plural(count("planNodes"), "plan node"),
    );
    out.push_str(&format!(
        "  in progress {} · blocked {} · paused {} · late {} · in review {}\n",
        count("inProgress"),
        count("blocked"),
        count("paused"),
        count("pastPlannedFinish"),
        count("underReview"),
    ));
    out.push_str(&format!(
        "  {} open across {} · {} block acceptance · {} block closeout\n",
        crate::plural(count("openResiduals"), "residual"),
        crate::plural(count("residualTaskCount"), "task"),
        count("acceptanceBlocked"),
        count("closeoutBlocked"),
    ));

    if let Some(phases) = data["phases"].as_array().filter(|rows| !rows.is_empty()) {
        out.push_str("\nBy discipline\n");
        for phase in phases {
            out.push_str(&format!(
                "  {:<24} {:>4}%  {} of {}\n",
                crate::truncate(
                    phase["discipline"]
                        .as_str()
                        .filter(|name| !name.is_empty())
                        .unwrap_or("(unassigned)"),
                    24,
                ),
                (phase["progress"].as_f64().unwrap_or(0.0) * 100.0).round() as u64,
                phase["complete"].as_u64().unwrap_or(0),
                phase["total"].as_u64().unwrap_or(0),
            ));
        }
    }

    if let Some(attention) = data["attention"].as_array().filter(|rows| !rows.is_empty()) {
        out.push_str(&format!(
            "\nNeeds attention ({} of {})\n",
            attention.len(),
            data["attentionTotal"]
                .as_u64()
                .unwrap_or(attention.len() as u64),
        ));
        for row in attention {
            out.push_str(&format!(
                "  {:<9} {:<10} {}\n",
                row["kind"].as_str().unwrap_or("?"),
                row["wbs"].as_str().unwrap_or("—"),
                crate::truncate(row["title"].as_str().unwrap_or("?"), 52),
            ));
        }
    }
    out
}
