use ds_cli_contract::Failure;
use std::{
    path::Path,
    process::{Command, Stdio},
};

/// Only this executable's fixed offline processing command is spawnable.
pub fn start_design_project_process(
    workspace: &Path,
    run_id: &str,
    workers: usize,
) -> Result<u32, Failure> {
    let fail = || {
        Failure::failed(
            "design_workspace_worker",
            "cannot launch the fixed Design worker",
        )
    };
    if !workspace.is_absolute()
        || run_id.is_empty()
        || run_id.len() > 120
        || run_id.chars().any(char::is_control)
        || workers == 0
    {
        return Err(fail());
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(workspace.join("worker.log"))
        .map_err(|_| fail())?;
    let mut command = Command::new(std::env::current_exe().map_err(|_| fail())?);
    command
        .args(["design", "project", "process", "--workspace"])
        .arg(workspace)
        .args([
            "--run-id",
            run_id,
            "--workers",
            &workers.to_string(),
            "--output",
            "json",
        ])
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|_| fail())?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000200);
    }
    Ok(command.spawn().map_err(|_| fail())?.id())
}
