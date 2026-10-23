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
//!
//! The plan is the exchange owner's, computed in the Network addon from the
//! staged sources and the declared choices (DECOUPLING-003).

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_dsgrid::native;
use ds_network_contracts::grid_exchange::ExchangeRequestParamsV1;
use ds_network_contracts::grid_operation::GridOperationV1;
use serde_json::Value;

use crate::{refusals, render, request, sources};
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
            command: "ds dsgrid-exchange plan --source ./rev3.dsgrid --source ./school.dsgrid --target kmz --mode merge-gis --output json",
            note: "Combine GIS features without reconciling or modifying the source engineering libraries.",
            runnable: false,
        },
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
        Example {
            command: "ds dsgrid-exchange plan --source ./network.zip --target dsgrid --alignment-layer mv_lines --alignment-label-property node_id --network-source-layer site_solaires --network-source-role plant --crs EPSG:32633 --output json",
            note: "Plan a source-rooted MV skeleton from explicit route and source layers while retaining all GIS evidence.",
            runnable: false,
        },
    ],
    refusals: &REFUSALS,
    reference: Some("docs/reference/dsgrid-exchange.md"),
    search: &["combine gis", "merged kmz", "merged shapefile"],
    requires: Requires::Server,
    availability: available,
};

/// The source refusals, the request refusals the owner raises, then the
/// addon's own. Spliced at compile time so the list cannot fall out of step.
static REFUSALS: [Refusal; 12 + native::REFUSALS.len()] = refusals::splice(&[
    sources::SHARED_REFUSALS,
    request::REQUEST_REFUSALS,
    native::REFUSALS,
]);

fn available() -> Availability {
    Availability::Available
}

/// The owner's bounded plan projection. Blockers, warnings and losses are
/// never truncated: they are the answer to "should this run", and a caller
/// who reads a truncated blocker list has been told a conversion is safe
/// when it is not. Long artifact and source lists name the withheld count.
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let loaded = sources::load(inputs.repeated("source"))?;
    let outcome = crate::exchange(
        &GridOperationV1::ExchangePlan(ExchangeRequestParamsV1 {
            sources: loaded.sources,
            choices: request::choices(inputs),
            warnings: loaded.warnings,
        }),
        &loaded.inputs,
    )?;
    crate::result(outcome)
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
