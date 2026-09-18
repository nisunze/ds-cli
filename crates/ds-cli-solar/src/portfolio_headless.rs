//! Native portfolio governance and the fixed Solar owner file handoff.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires};
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
pub static PUBLISHED_READ: Command = Command {
    id: "solar.portfolio.published.read",
    path: &["solar", "portfolio", "published", "read"],
    contract: 1,
    summary: "Read an exact published portfolio result headlessly.",
    purpose: "Download the generation-pinned aggregate-v3 result, verify its exact size and digest, recheck the artifact head and governed membership, and require the selected run identity before returning a bounded semantic projection. No Desktop, browser or local result directory is required. A different published run is refused; no newer run is silently selected.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        PORTFOLIO,
        Arg::value("run-id", "<id>", "Exact published portfolio source run.").required(),
        Arg::repeated(
            "path",
            "<key>",
            "Semantic object key; repeat to descend, at most eight keys.",
        ),
    ],
    output: "Portfolio trace, exact publication work/head/output digest, selected path/value and explicit completeness; private download capabilities never escape.",
    examples: &[],
    refusals: PUBLISHED_READ_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
static PUBLISHED_READ_REFUSALS: &[ds_cli_contract::spec::Refusal] = &[
    ds_cli_contract::spec::Refusal {
        code: "auth_input_invalid",
        when: "the selected run, project, portfolio or current membership does not match the published result",
        remedy: "read current membership and saved analysis, then select the exact published source run",
    },
    ds_cli_contract::spec::Refusal {
        code: "invalid_portfolio_path",
        when: "the semantic path is empty, too deep or has an overlong key",
        remedy: "pass at most eight non-empty semantic keys of at most 120 characters",
    },
    ds_cli_contract::spec::Refusal {
        code: "portfolio_path_not_found",
        when: "the verified portfolio has no value at that semantic path",
        remedy: "omit path to inspect the bounded root outline, then select declared keys",
    },
];
pub fn published_read(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = crate::workflow::portfolio_path(i)?;
    let receipt = execute(
        i,
        ds_cli_auth::SolarPortfolioCommand::PublishedRead {
            portfolio: i.require("portfolio")?.into(),
            run_id: i.require("run-id")?.into(),
        },
    )?;
    let selected = crate::workflow::select_portfolio_path(&receipt["document"], &path)?;
    let (value, complete) = crate::workflow::bounded_portfolio_projection(selected);
    let mut result = json!({"project_id":receipt["project_id"],"trace":receipt["trace"],
        "publication":receipt["publication"],"path":path,"value":value,"complete":complete});
    if !complete {
        result["more"] =
            json!({"reason":"projection_elided","next":"repeat with a narrower --path"});
    }
    Ok(result)
}
pub static CALCULATE: Command = Command {
    id: "solar.portfolio.calculate",
    path: &["solar", "portfolio", "calculate"],
    contract: 1,
    summary: "Aggregate a verified city batch for pinned membership headlessly.",
    purpose: "Fetch exact portfolio membership, verify its revision and each selected city in a sealed source batch, derive assumptions from sealed city results, and produce a closed portfolio result, French APD draft and chart files. The source may contain additional cities; its closure stays unchanged and the governed member order is retained. Does not publish or rerun cities.",
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
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PUBLISH: Command = Command {
    id: "solar.portfolio.publish",
    path: &["solar", "portfolio", "publish"],
    contract: 2,
    summary: "Publish an exact closed portfolio result without a paired desktop.",
    purpose: "Verify a portfolio batch and every declared result, draft and chart, recheck current governed membership, and publish all selected outputs through the native compute-artifact protocol. Repeating the same run resumes or returns its existing publication receipt.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("source", "<dir>", "Closed portfolio output directory.").required(),
    ],
    output: "Verified online publication receipt, source run, immutable publication identity and published output count.",
    examples: &[],
    refusals: ds_cli_auth::PROJECT_STATUS_COMMAND.refusals,
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
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
    use base64::Engine;
    let value = crate::project::invoke(
        json!({"operation":"portfolio_publication","source":i.require("source")?}),
    )?;
    let result = value["result_json"].as_str().ok_or_else(|| {
        Failure::unavailable(
            "solar_project_io",
            "Solar owner did not return the sealed result bytes",
        )
    })?;
    let entries = value["outputs"]
        .as_array()
        .filter(|v| v.len() <= 99)
        .ok_or_else(|| {
            Failure::unavailable(
                "solar_project_io",
                "Solar owner did not return a bounded publication plan",
            )
        })?;
    let mut outputs = Vec::with_capacity(entries.len());
    let mut total = result.len();
    for entry in entries {
        let declaration = &entry["declaration"];
        let encoded = entry["base64"]
            .as_str()
            .filter(|v| v.len() <= 24 * 1024 * 1024)
            .ok_or_else(publication_handoff_error)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| publication_handoff_error())?;
        total = total
            .checked_add(bytes.len())
            .ok_or_else(publication_handoff_error)?;
        if total > 128 * 1024 * 1024
            || declaration["size_bytes"].as_u64() != Some(bytes.len() as u64)
            || declaration["sha256"].as_str()
                != Some(format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(&bytes)).as_str())
        {
            return Err(publication_handoff_error());
        }
        outputs.push(ds_cli_auth::SolarProjectOutput {
            id: declaration["output_id"]
                .as_str()
                .ok_or_else(publication_handoff_error)?
                .into(),
            format: declaration["format"]
                .as_str()
                .ok_or_else(publication_handoff_error)?
                .into(),
            content_type: declaration["content_type"]
                .as_str()
                .ok_or_else(publication_handoff_error)?
                .into(),
            bytes,
        });
    }
    execute(
        i,
        ds_cli_auth::SolarPortfolioCommand::Publish {
            result: result.as_bytes().to_vec(),
            outputs,
        },
    )
}

fn publication_handoff_error() -> Failure {
    Failure::unavailable(
        "solar_project_io",
        "Solar owner publication handoff is incomplete or changed",
    )
}
pub fn render_publication(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}
