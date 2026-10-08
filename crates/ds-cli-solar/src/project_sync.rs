//! Authenticated IO adapter for the Solar owner's authored-input queue.
use crate::project::{invoke, render};
use ds_cli_auth::SolarProjectCommand;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use fs2::FileExt;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

pub static COMMAND: Command = Command {
    id: "solar.project.sync",
    path: &["solar", "project", "sync"],
    contract: 3,
    summary: "Publish authored Solar city inputs without Desktop.",
    purpose: "Publish queued authored city inputs and copied maps under the restored native principal and explicit project. Use project clean to clear local computed runs, then compute again from the engine and current inputs. --background starts this same fixed input worker; --watch retries transient connectivity failures. Local drafts remain available.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::portfolio_headless::PROJECT,
        Arg::value("workspace", "<dir>", "Private local Solar workspace.").required(),
        ds_cli_contract::spec::Arg {
            summary: "stable or canary; default stable.",
            value: "<lane>",
            default: None,
            choices: &[],
            ..ds_cli_contract::spec::LANE
        },
        Arg::switch(
            "background",
            "Start a detached worker and return its process id.",
        ),
        Arg::switch(
            "watch",
            "Watch for new work; retry connectivity failures for up to 12 hours.",
        ),
    ],
    output: "Published row count, remaining work, or detached worker process id. No credentials or upload sessions.",
    examples: &[],
    refusals: &[
        Refusal {
            code: "solar_project_lane",
            when: "the lane is neither stable nor canary",
            remedy: "choose the native account deployment lane",
        },
        Refusal {
            code: "solar_project_worker_input",
            when: "the worker workspace or lane is invalid",
            remedy: "use an absolute existing workspace and stable or canary lane",
        },
        Refusal {
            code: "solar_project_worker_start",
            when: "the fixed worker cannot be launched",
            remedy: "retry in the foreground or repair the installed ds executable",
        },
        Refusal {
            code: "solar_project_sync_busy",
            when: "another worker holds this workspace",
            remedy: "use the existing worker or stop it before starting another",
        },
        Refusal {
            code: "solar_project_sync_io",
            when: "the authored city handoff is invalid or cannot be read safely",
            remedy: "inspect the authored city inputs; never acknowledge a failed input commit",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability,
};
fn availability() -> Availability {
    crate::DS_SOLAR.availability()
}
pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = i.require("project")?;
    if !ds_command_kernel::execution_context::valid_project(project) {
        return Err(Failure::invalid(
            "solar_project_worker_input",
            "Invalid explicit Solar project identity",
        ));
    }
    let workspace = fs::canonicalize(i.require("workspace")?).map_err(io_error)?;
    let lane = i.value("lane").unwrap_or("stable");
    if !matches!(lane, "stable" | "canary") {
        return Err(Failure::invalid(
            "solar_project_lane",
            "lane must be stable or canary",
        ));
    }
    let status = invoke(json!({"operation":"status","workspace":workspace}))?;
    validate_workspace_project(&status, project)?;
    if i.switch("background") {
        return Ok(
            json!({"worker_pid":ds_cli_exec::start_solar_project_sync(&workspace,lane,project)?,"project_id":project,"workspace":workspace,"publication":"background"}),
        );
    }
    let path = workspace.join("sync.lock");
    if let Ok(meta) = fs::symlink_metadata(&path)
        && (!meta.is_file() || meta.file_type().is_symlink())
    {
        return Err(io_error("unsafe sync lock"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(path).map_err(io_error)?;
    lock.try_lock_exclusive().map_err(|_| {
        Failure::conflict(
            "solar_project_sync_busy",
            "a Solar upload worker already owns this workspace",
        )
    })?;
    let started = std::time::Instant::now();
    let mut published = 0;
    let mut delay = 2;
    loop {
        let outcome = sync_one(&workspace, lane, project);
        match outcome {
            Ok(true) => {
                published += 1;
                delay = 2;
            }
            Ok(false) if !i.switch("watch") => {
                return Ok(json!({"published_rows":published,"pending":false,"scope":"inputs"}));
            }
            Ok(false) => {
                delay = 5;
                std::thread::sleep(Duration::from_secs(delay));
            }
            Err(error) => {
                if !i.switch("watch")
                    || !matches!(
                        error.code(),
                        "auth_transient"
                            | "headless_service_unavailable"
                            | "headless_transport_failed"
                            | "headless_signed_out"
                    )
                {
                    return Err(error);
                }
                std::thread::sleep(Duration::from_secs(delay));
                delay = (delay * 2).min(300);
            }
        }
        if started.elapsed() > Duration::from_secs(12 * 60 * 60) {
            return Ok(json!({"published_rows":published,"worker":"time_limit"}));
        }
    }
}
fn sync_one(workspace: &Path, lane: &str, project: &str) -> Result<bool, Failure> {
    let next = invoke(json!({"operation":"sync_next","workspace":workspace,"inputs_only":true}))?;
    if next["pending"] == false {
        return Ok(false);
    }
    let outcome = (|| {
        let mut session = ds_cli_auth::solar_project_session_for_project(lane, project)?;
        let b = session.binding();
        invoke(
            json!({"operation":"sync_bind","workspace":workspace,"project":b["project"],"lane":b["lane"],"principal":b["principal"],"audience":b["audience"]}),
        )?;
        let jobs = next["jobs"]
            .as_array()
            .ok_or_else(|| io_error("invalid owner handoff"))?;
        if jobs.len() != 1 || next["kind"] != "city" {
            return Err(io_error("expected one authored city input handoff"));
        }
        let mut receipts = Vec::new();
        for job in jobs {
            let command: Job = serde_json::from_value(job.clone()).map_err(io_error)?;
            let command = match command {
                Job::CommitCity {
                    city,
                    expected_base,
                    snapshot_json,
                    fingerprint,
                } => SolarProjectCommand::CommitCity {
                    city,
                    expected_base,
                    snapshot_json,
                    fingerprint,
                },
            };
            receipts.push(session.execute(&command)?);
        }
        let receipt = receipts.remove(0);
        invoke(
            json!({"operation":"sync_ack","workspace":workspace,"inputs_only":true,"sequence":next["sequence"],"digest":next["digest"],"receipt":receipt}),
        )?;
        Ok(true)
    })();
    if let Err(error) = &outcome {
        invoke(
            json!({"operation":"sync_failed","workspace":workspace,"inputs_only":true,"sequence":next["sequence"],"digest":next["digest"],"code":error.code()}),
        )?;
    }
    outcome
}

fn validate_workspace_project(status: &Value, project: &str) -> Result<(), Failure> {
    if status["project"]["project_id"] != project
        || status["project"]["root"] != format!("eds_project/{project}/eds_solar")
    {
        return Err(Failure::invalid(
            "solar_project_worker_input",
            "The explicit project differs from the Solar workspace identity",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod project_context_tests {
    use super::*;
    #[test]
    fn publication_context_cannot_borrow_another_workspace_project() {
        let status =
            json!({"project":{"project_id":"project_a","root":"eds_project/project_a/eds_solar"}});
        assert!(validate_workspace_project(&status, "project_a").is_ok());
        assert!(validate_workspace_project(&status, "project_b").is_err());
        let mixed =
            json!({"project":{"project_id":"project_a","root":"eds_project/project_b/eds_solar"}});
        assert!(validate_workspace_project(&mixed, "project_a").is_err());
    }
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Job {
    CommitCity {
        city: String,
        expected_base: String,
        snapshot_json: String,
        fingerprint: String,
    },
}

fn io_error(e: impl std::fmt::Display) -> Failure {
    Failure::failed(
        "solar_project_sync_io",
        format!("Solar upload handoff failed: {e}"),
    )
}
pub fn render_sync(value: &Value) -> String {
    render(value)
}
