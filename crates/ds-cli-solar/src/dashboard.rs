//! CLI/MCP host over verified Solar source IO and shared Rust composition.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use std::io::Write;

pub static COMMAND: Command = Command {
    id: "solar.dashboard.compose",
    path: &["solar", "dashboard", "compose"],
    contract: 2,
    summary: "Compose a Solar dashboard as JSON and standalone HTML headlessly.",
    purpose: "Verify a sealed city or membership-pinned portfolio and compose Site, Plant, Finance or BOQ cards, tables and script-free charts through shared Rust. Portfolio sections bind the exact portfolio id and membership revision. External plot files and online publication are not included.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "source",
            "<dir>",
            "Closed Solar city or portfolio batch directory.",
        )
        .required(),
        Arg::value(
            "project",
            "<id>",
            "Exact project attribution in the batch; grants no cloud authority.",
        )
        .required(),
        Arg::value("run-id", "<id>", "Exact sealed source run.").required(),
        Arg::value("city", "<id>", "Exact city; required for city sections."),
        Arg::value(
            "portfolio",
            "<id>",
            "Exact portfolio; required for portfolio sections.",
        ),
        Arg::value(
            "membership-revision",
            "<sha256:digest>",
            "Exact sealed membership; required for portfolio sections.",
        ),
        Arg::value("section", "<name>", "Dashboard section.")
            .required()
            .choices(&[
                "site",
                "plant",
                "finance",
                "boq",
                "portfolio_finance",
                "portfolio_site",
                "portfolio_plant",
                "portfolio_boq",
            ]),
        Arg::value(
            "system",
            "<name>",
            "Exact Plant/Finance/BOQ scenario; defaults to hybrid.",
        )
        .choices(&["hybrid", "solar_battery", "thermal_only"]),
        Arg::value("out", "<dir>", "New private directory; never replaced.").required(),
    ],
    output: "Verified source identity, dashboard.json and index.html; publication not_requested and plots not_included.",
    examples: &[],
    refusals: &[
        Refusal {
            code: "missing_input",
            when: "the selected section lacks its city or portfolio membership selectors",
            remedy: "supply --city for city sections, or --portfolio and --membership-revision for portfolio sections",
        },
        Refusal {
            code: "solar_dashboard_context_invalid",
            when: "city and portfolio selectors, or a city scenario and portfolio section, are mixed",
            remedy: "use city and optional system for city sections; use portfolio and membership-revision for portfolio sections",
        },
        Refusal {
            code: "solar_engine_missing",
            when: "the matching Solar owner is absent",
            remedy: "install matching ds and ds-solar releases",
        },
        Refusal {
            code: "engine_refused",
            when: "the sealed source or exact project/run/city identity cannot be verified",
            remedy: "use an intact closed batch and its exact identity",
        },
        Refusal {
            code: "solar_dashboard_output_exists",
            when: "the destination already exists",
            remedy: "choose a new dashboard directory",
        },
        Refusal {
            code: "solar_dashboard_io",
            when: "composition is unavailable or private output cannot be written",
            remedy: "verify source section availability, compatible currencies and writable private directories",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    requires: Requires::Server,
    availability: crate::project::availability,
};
fn io(error: impl std::fmt::Display) -> Failure {
    Failure::unavailable("solar_dashboard_io", error.to_string())
}
fn selector<'a>(i: &'a Inputs, name: &str) -> Result<&'a str, Failure> {
    i.value(name).ok_or_else(|| {
        Failure::invalid(
            "missing_input",
            format!("this dashboard section requires --{name}"),
        )
    })
}
pub(crate) fn source(i: &Inputs) -> Result<Value, Failure> {
    let section: ds_command_kernel::solar_dashboard::Section =
        serde_json::from_value(json!(i.require("section")?)).map_err(io)?;
    if section.is_portfolio() {
        if i.value("city").is_some() || i.value("system").is_some() {
            return Err(Failure::invalid(
                "solar_dashboard_context_invalid",
                "portfolio sections cannot take city or system selectors",
            ));
        }
        return crate::project::invoke(json!({"operation":"portfolio_dashboard_source",
            "source":i.require("source")?,"project":i.require("project")?,"run":i.require("run-id")?,
            "portfolio":selector(i, "portfolio")?,"membership_revision":selector(i, "membership-revision")?}));
    }
    if i.value("portfolio").is_some() || i.value("membership-revision").is_some() {
        return Err(Failure::invalid(
            "solar_dashboard_context_invalid",
            "city sections cannot take portfolio selectors",
        ));
    }
    crate::project::invoke(
        json!({"operation":"dashboard_source","source":i.require("source")?,"project":i.require("project")?,"run":i.require("run-id")?,"city":selector(i, "city")?}),
    )
}
pub fn execute(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let out = std::path::PathBuf::from(i.require("out")?);
    if std::fs::symlink_metadata(&out).is_ok() {
        return Err(Failure::invalid(
            "solar_dashboard_output_exists",
            "choose a new dashboard directory",
        ));
    }
    let source = source(i)?;
    let request: ds_command_kernel::solar_dashboard::Request =
        serde_json::from_value(json!({"section":i.require("section")?,"system":i.value("system"),"report":source["report"]}))
            .map_err(io)?;
    let dashboard = ds_command_kernel::solar_dashboard::compose(request).map_err(io)?;
    let mut source = source;
    source
        .as_object_mut()
        .ok_or_else(|| io("invalid source receipt"))?
        .remove("report");
    let document = json!({"source":source,"dashboard":dashboard});
    let html = ds_command_kernel::solar_dashboard::html(
        &dashboard,
        &format!(
            "{} · {} · {}",
            i.require("project")?,
            i.value("city")
                .or_else(|| i.value("portfolio"))
                .ok_or_else(|| io("dashboard identity is missing"))?,
            i.require("section")?
        ),
    );
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&out).map_err(io)?;
    let result = (|| -> Result<(), Failure> {
        for (name, bytes) in [
            (
                "dashboard.json",
                serde_json::to_vec_pretty(&document).map_err(io)?,
            ),
            ("index.html", html.into_bytes()),
        ] {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(out.join(name)).map_err(io)?;
            file.write_all(&bytes).map_err(io)?;
            file.sync_all().map_err(io)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&out);
        return Err(error);
    }
    Ok(
        json!({"source":source,"directory":out,"artifacts":["dashboard.json","index.html"],"publication":"not_requested","plots":"not_included"}),
    )
}
