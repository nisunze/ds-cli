//! CLI/MCP host over verified Solar source IO and shared Rust composition.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use std::io::Write;

pub static COMMAND: Command = Command {
    id: "solar.dashboard.compose",
    path: &["solar", "dashboard", "compose"],
    contract: 1,
    summary: "Compose a Solar dashboard as JSON and standalone HTML headlessly.",
    purpose: "Verify an exact sealed city report input, compose cards, derived metrics and declarative Plant chart options in shared Rust, and write a new private dashboard directory. No TypeScript, browser, paired desktop, network or publication is required. Plot files are not included.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("source", "<dir>", "Closed Solar city batch directory.").required(),
        Arg::value(
            "project",
            "<id>",
            "Exact project attribution in the batch; grants no cloud authority.",
        )
        .required(),
        Arg::value("run-id", "<id>", "Exact sealed source run.").required(),
        Arg::value("city", "<id>", "Exact city in that batch.").required(),
        Arg::value("section", "<name>", "Dashboard section.")
            .required()
            .choices(&["site", "plant"]),
        Arg::value(
            "system",
            "<name>",
            "Exact Plant scenario; defaults to hybrid.",
        )
        .choices(&["hybrid", "solar_battery", "thermal_only"]),
        Arg::value("out", "<dir>", "New private directory; never replaced.").required(),
    ],
    output: "Verified source identity, dashboard.json and index.html; publication not_requested and plots not_included.",
    examples: &[],
    refusals: crate::project::RUN.refusals,
    reference: Some("docs/reference/solar.md"),
    availability: crate::project::availability,
};
fn io(error: impl std::fmt::Display) -> Failure {
    Failure::unavailable("solar_dashboard_io", error.to_string())
}
pub(crate) fn source(i: &Inputs) -> Result<Value, Failure> {
    crate::project::invoke(
        json!({"operation":"dashboard_source","source":i.require("source")?,"project":i.require("project")?,"run":i.require("run-id")?,"city":i.require("city")?}),
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
            i.require("city")?,
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
