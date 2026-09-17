//! Native portfolio governance and the fixed Solar owner file handoff.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

pub const PROJECT: Arg = Arg::value(
    "project",
    "<id>",
    "Explicit authorized project; leaves selection unchanged.",
)
.required();
pub const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Native authority lane; defaults to stable.",
);
const PORTFOLIO: Arg =
    Arg::value("portfolio", "<id>", "Governed portfolio id from list.").required();
pub fn execute(i: &Inputs, command: ds_cli_auth::SolarPortfolioCommand) -> Result<Value, Failure> {
    ds_cli_auth::solar_for_project(
        i.value("lane").unwrap_or("stable"),
        i.require("project")?,
        &ds_cli_auth::SolarProjectCommand::Portfolio(command),
    )
}
pub static CALCULATE: Command = Command {
    id: "solar.portfolio.calculate",
    path: &["solar", "portfolio", "calculate"],
    contract: 1,
    summary: "Aggregate a verified city batch for pinned membership headlessly.",
    purpose: "Fetch exact portfolio membership, verify its revision and ordered source batch, derive assumptions from sealed city results, and produce a closed portfolio result, French APD draft and chart files. Does not publish or rerun cities.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        PORTFOLIO,
        Arg::value(
            "membership-revision",
            "<sha256:digest>",
            "Exact revision from list.",
        )
        .required(),
        Arg::value("source", "<dir>", "Verified city batch directory.").required(),
        Arg::value("source-run-id", "<id>", "Exact source city run id.").required(),
        Arg::value("run-id", "<id>", "New portfolio run identity.").required(),
        Arg::value("out", "<dir>", "New output directory; never replaced.").required(),
    ],
    output: "Closed batch identity, exact membership, city count and output directory; publication not_requested.",
    examples: &[],
    refusals: crate::project::RUN.refusals,
    reference: Some("docs/reference/solar.md"),
    availability: ds_cli_auth::native_availability,
};
pub static PUBLISH: Command = Command {
    id: "solar.portfolio.publish",
    path: &["solar", "portfolio", "publish"],
    contract: 1,
    summary: "Publish an exact closed portfolio result without a paired desktop.",
    purpose: "Verify a portfolio batch and result, recheck current governed membership, and publish the sealed result through the native compute-artifact protocol. Repeating the same run resumes or returns its existing publication receipt. Draft documents and charts remain in the local bundle.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("source", "<dir>", "Closed portfolio output directory.").required(),
    ],
    output: "Verified online publication receipt for the portfolio result.",
    examples: &[],
    refusals: ds_cli_auth::PROJECT_STATUS_COMMAND.refusals,
    reference: Some("docs/reference/solar.md"),
    availability: ds_cli_auth::native_availability,
};
pub fn calculate(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let row = execute(
        i,
        ds_cli_auth::SolarPortfolioCommand::Get {
            portfolio: i.require("portfolio")?.into(),
        },
    )?;
    if row["membership_revision"] != i.require("membership-revision")? {
        return Err(Failure::invalid(
            "invalid_portfolio_mutation",
            "portfolio membership changed; list and pin its current revision",
        ));
    }
    crate::project::invoke(json!({"operation":"portfolio_calculate","request":{
        "source":i.require("source")?,"source_run_id":i.require("source-run-id")?,"out":i.require("out")?,
        "identity":{"project_id":i.require("project")?,"root":format!("eds_project/{}/eds_solar",i.require("project")?),"portfolio_id":row["id"],"portfolio_name":row["display_name"],"membership_revision":row["membership_revision"]},
        "cities":row["cities"],"run_id":i.require("run-id")?,"strategy":{"strategy":"first"}
    }}))
}
pub fn publish(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let value = crate::project::invoke(
        json!({"operation":"portfolio_publication","source":i.require("source")?}),
    )?;
    let result = value["result_json"].as_str().ok_or_else(|| {
        Failure::unavailable(
            "solar_project_io",
            "Solar owner did not return the sealed result bytes",
        )
    })?;
    execute(
        i,
        ds_cli_auth::SolarPortfolioCommand::Publish {
            result: result.as_bytes().to_vec(),
        },
    )
}
