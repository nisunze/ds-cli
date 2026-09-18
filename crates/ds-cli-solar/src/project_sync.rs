//! Authenticated IO adapter for the Solar owner's durable publication queue.
use crate::project::{invoke, render};
use ds_cli_auth::{SolarProjectCommand, SolarProjectOutput};
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use fs2::FileExt;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub static COMMAND: Command = Command {
    id: "solar.project.sync",
    path: &["solar", "project", "sync"],
    contract: 2,
    summary: "Publish queued Solar cities and drafts without Desktop.",
    purpose: "Restore the native user in the selected lane and bind the workspace to that principal, audience and explicit project, leaving saved selection unchanged. Publish the oldest city revision with a cloud revision fence, then verified run artifacts through the existing compute artifact service. Failures preserve local drafts and pending work. --background starts the same fixed worker with its captured project; --watch keeps retrying transient failures. Use --inputs-only to publish editable inputs and copied maps while leaving draft publication intents untouched (for example a development run awaiting a release build). Only the Solar owner reads the workspace database.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::portfolio_headless::PROJECT,
        Arg::value("workspace", "<dir>", "Private local Solar workspace.").required(),
        Arg::value("lane", "<lane>", "stable or canary; default stable."),
        Arg::value(
            "run-id",
            "<id>",
            "Publish one closed run and pending inputs, retaining other runs. Foreground only.",
        ),
        Arg::switch(
            "inputs-only",
            "Publish city inputs and maps only; retain queued report runs. Foreground only.",
        ),
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
            when: "an owner artifact is unsafe or no longer matches its digest",
            remedy: "restore the exact closed local artifacts; never acknowledge a failed upload",
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
    if (i.switch("background") && (i.switch("inputs-only") || i.value("run-id").is_some()))
        || (i.switch("inputs-only") && i.value("run-id").is_some())
    {
        return Err(Failure::invalid(
            "solar_project_worker_input",
            "Use inputs-only or run-id in the foreground, separately.",
        ));
    }
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
        let outcome = sync_one(
            &workspace,
            lane,
            project,
            i.switch("inputs-only"),
            i.value("run-id"),
        );
        match outcome {
            Ok(true) => {
                published += 1;
                delay = 2;
            }
            Ok(false) if !i.switch("watch") => {
                return Ok(
                    json!({"published_rows":published,"pending":false,"scope":if i.switch("inputs-only") {"inputs"} else if i.value("run-id").is_some() {"run"} else {"all"},"run_id":i.value("run-id")}),
                );
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
fn sync_one(
    workspace: &Path,
    lane: &str,
    project: &str,
    inputs_only: bool,
    run_id: Option<&str>,
) -> Result<bool, Failure> {
    let next = invoke(
        json!({"operation":"sync_next","workspace":workspace,"inputs_only":inputs_only,"run_id":run_id}),
    )?;
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
        if jobs.is_empty() {
            return Err(io_error("empty publication handoff"));
        }
        let mut receipts = Vec::new();
        for job in jobs {
            let command: Job = serde_json::from_value(job.clone()).map_err(io_error)?;
            if let Job::Publish {
                calculation,
                reports,
                ..
            } = &command
                && std::iter::once(calculation)
                    .chain(reports)
                    .map(|o| o.size_bytes)
                    .try_fold(0_u64, |sum, n| sum.checked_add(n))
                    .is_none_or(|n| n > 128 * 1024 * 1024)
            {
                return Err(io_error("city upload exceeds 128 MiB"));
            }
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
                Job::Publish {
                    city,
                    client_run_id,
                    engine_version,
                    build_manifest,
                    fingerprint,
                    calculation,
                    reports,
                } => SolarProjectCommand::Publish {
                    city,
                    client_run_id,
                    engine_version,
                    build_manifest,
                    fingerprint,
                    calculation: read_output(workspace, &calculation)?,
                    reports: reports
                        .iter()
                        .map(|o| read_output(workspace, o))
                        .collect::<Result<_, _>>()?,
                },
            };
            receipts.push(session.execute(&command)?);
        }
        let receipt = if next["kind"] == "city" {
            receipts.remove(0)
        } else {
            json!({"published":true,"batch_digest":next["digest"],"cities":receipts})
        };
        invoke(
            json!({"operation":"sync_ack","workspace":workspace,"inputs_only":inputs_only,"run_id":run_id,"sequence":next["sequence"],"digest":next["digest"],"receipt":receipt}),
        )?;
        Ok(true)
    })();
    if let Err(error) = &outcome {
        invoke(
            json!({"operation":"sync_failed","workspace":workspace,"inputs_only":inputs_only,"run_id":run_id,"sequence":next["sequence"],"digest":next["digest"],"code":error.code()}),
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
    Publish {
        city: String,
        client_run_id: String,
        engine_version: String,
        build_manifest: String,
        fingerprint: String,
        calculation: Artifact,
        reports: Vec<Artifact>,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    id: String,
    format: String,
    content_type: String,
    path: PathBuf,
    sha256: String,
    size_bytes: u64,
}
fn read_output(workspace: &Path, artifact: &Artifact) -> Result<SolarProjectOutput, Failure> {
    if artifact.size_bytes == 0 || artifact.size_bytes > 64 * 1024 * 1024 {
        return Err(io_error("artifact exceeds upload limit"));
    }
    let root = workspace.join("runs");
    if fs::symlink_metadata(&root)
        .map_err(io_error)?
        .file_type()
        .is_symlink()
    {
        return Err(io_error("runs directory is a link"));
    }
    let relative = artifact.path.strip_prefix(&root).map_err(io_error)?;
    let mut path = root;
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(io_error("unsafe artifact path"));
        }
        path.push(part);
        if fs::symlink_metadata(&path)
            .map_err(io_error)?
            .file_type()
            .is_symlink()
        {
            return Err(io_error("artifact path contains a link"));
        }
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(io_error("artifact is not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(artifact.size_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 != artifact.size_bytes
        || format!("{:x}", Sha256::digest(&bytes)) != artifact.sha256
    {
        return Err(io_error("artifact bytes changed"));
    }
    Ok(SolarProjectOutput {
        id: artifact.id.clone(),
        format: artifact.format.clone(),
        content_type: artifact.content_type.clone(),
        bytes,
    })
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
