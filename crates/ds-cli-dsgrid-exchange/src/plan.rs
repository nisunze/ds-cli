//! `ds dsgrid-exchange plan` — exactly what a conversion would do, before it
//! does any of it.
//!
//! A plan is not a preview in the loose sense. It pins the digest of every
//! source it read, and `convert` re-digests those bytes and refuses to run if
//! any of them changed. So a plan is a commitment: the thing you read here is
//! the thing that runs, or nothing runs.
//!
//! That is why `plan` and `convert` are separate commands rather than
//! `convert --dry-run`. A flag would suggest the two differ by a switch; they
//! differ by effect class — this one is `Discovery` and can be called freely,
//! the other writes files into a directory. The blockers, warnings and losses
//! reported here are the engine's own, and a plan carrying a blocker has no
//! executable stages at all.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_exchange::conversion::{ConversionPlan, plan_conversion};
use serde_json::{Value, json};

use crate::{refusals, render, request, sources, token};

/// How many per-source rows and expected artifacts to project by default. A
/// conversion of a large workspace can plan thousands of artifacts; printing
/// all of them by default would bury the blockers, which are the reason to
/// read a plan at all.
const PROJECTION_LIMIT: usize = 25;

pub static COMMAND: Command = Command {
    id: "dsgrid-exchange.plan",
    path: &["dsgrid-exchange", "plan"],
    contract: 1,
    summary: "Plan a conversion: pinned digests, stages, blockers and losses.",
    purpose: "\
Produces the immutable, digest-pinned plan for a conversion without running \
it. The plan names every stage, every artifact the conversion would write, \
what it would lose, and anything blocking it. `convert` re-checks these pins \
and refuses if the source bytes have changed since — so a plan that reads \
correctly is the plan that runs.",
    chapter: Chapter::GridModel,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: request::SHARED_ARGS,
    output: "\
The plan id, the chosen capabilities, per-source planning status with pinned \
digests, and the expected artifacts — plus blockers, warnings and losses in \
full. Long lists are truncated and the withheld count reported.",
    examples: &[
        Example {
            command: "ds dsgrid-exchange plan --source ./workspace --target dsgrid --output json",
            note: "What importing a PLS-CADD workspace would produce.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid-exchange plan --source ./model.dsgrid --target kmz --output json",
            note: "A GIS export, with any geometry losses named up front.",
            runnable: false,
        },
    ],
    refusals: &REFUSALS,
    reference: Some("docs/reference/dsgrid-exchange.md"),
    availability: available,
};

/// The source refusals, then the request refusals. Spliced at compile time so
/// the list cannot fall out of step with what the two modules actually emit.
static REFUSALS: [Refusal; 6] =
    refusals::splice(&[sources::SHARED_REFUSALS, request::REQUEST_REFUSALS]);

fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let loaded = sources::load(inputs.repeated("source"))?;
    let request = request::build(inputs, loaded.sources)?;
    let plan = plan_conversion(&request);
    Ok(project(&plan))
}

/// Shape the engine's plan into the bounded projection `ds` returns.
///
/// Blockers, warnings and losses are never truncated. Everything else is:
/// those three are the answer to "should this run", and a caller who reads a
/// truncated blocker list has been told a conversion is safe when it is not.
pub fn project(plan: &ConversionPlan) -> Value {
    let (artifacts, artifacts_withheld) = take(
        plan.expected_artifacts
            .iter()
            .map(|artifact| {
                json!({
                    "path": artifact.relative_path,
                    "artifact_type": artifact.artifact_type,
                    "source_index": artifact.source_index,
                })
            })
            .collect(),
        PROJECTION_LIMIT,
    );

    let (per_source, sources_withheld) = take(
        plan.per_source
            .iter()
            .map(|source| {
                json!({
                    "source_index": source.source_index,
                    "name": source.display_name,
                    "kind": token(&source.kind),
                    "digest": source.digest,
                    "planned": source.planned,
                    "blockers": source.blockers,
                    "warnings": source.warnings,
                })
            })
            .collect(),
        PROJECTION_LIMIT,
    );

    let mut answer = json!({
        "plan_id": plan.plan_id,
        "target": token(&plan.target),
        "batch_mode": token(&plan.batch_mode),
        "executable": plan.is_executable(),
        "chosen_capabilities": plan.chosen_capability_ids,
        "stages": plan.stages.len(),
        "expected_artifacts": artifacts,
        "expected_artifact_count": plan.expected_artifacts.len(),
        "per_source": per_source,
        "pinned_digests": plan.pinned_digests,
        "declared_crs": plan.declared_crs,
        "swap_xy": plan.swap_xy,
        "blockers": plan.blockers,
        "warnings": plan.warnings,
        "losses": plan.losses,
    });

    if artifacts_withheld > 0 || sources_withheld > 0 {
        answer["more"] = json!({
            "expected_artifacts_withheld": artifacts_withheld,
            "per_source_withheld": sources_withheld,
            "next": "read plan.expected_artifact_count for the full total",
        });
    }

    answer
}

fn take(mut items: Vec<Value>, limit: usize) -> (Vec<Value>, usize) {
    if items.len() <= limit {
        return (items, 0);
    }
    let withheld = items.len() - limit;
    items.truncate(limit);
    (items, withheld)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "plan {}\n  {} → {}   {} stage(s), {} artifact(s)\n",
        data["plan_id"].as_str().unwrap_or(""),
        data["batch_mode"].as_str().unwrap_or(""),
        data["target"].as_str().unwrap_or(""),
        data["stages"],
        data["expected_artifact_count"],
    );

    out.push_str(if data["executable"].as_bool() == Some(true) {
        "  executable\n"
    } else {
        "  NOT executable\n"
    });

    render::list(&mut out, "BLOCKERS", &data["blockers"]);
    render::list(&mut out, "LOSSES", &data["losses"]);
    render::list(&mut out, "WARNINGS", &data["warnings"]);

    if let Some(rows) = data["per_source"]
        .as_array()
        .filter(|rows| !rows.is_empty())
    {
        out.push_str("\nSOURCES\n");
        for row in rows {
            out.push_str(&format!(
                "  [{}] {:<30} {:<18} {}\n",
                row["source_index"],
                row["name"].as_str().unwrap_or(""),
                row["kind"].as_str().unwrap_or(""),
                if row["planned"].as_bool() == Some(true) {
                    "planned"
                } else {
                    "not planned"
                },
            ));
        }
    }

    if let Some(withheld) = data["more"]["per_source_withheld"]
        .as_u64()
        .filter(|n| *n > 0)
    {
        out.push_str(&format!("  … {withheld} more source(s)\n"));
    }
    if let Some(withheld) = data["more"]["expected_artifacts_withheld"]
        .as_u64()
        .filter(|n| *n > 0)
    {
        out.push_str(&format!("  … {withheld} more expected artifact(s)\n"));
    }

    out
}
