use ds_cli_contract::outcome::Failure;
use std::{
    path::Path,
    process::{Command, Stdio},
};

/// Start only the fixed, typed Solar publication worker in this exact CLI.
/// The caller has already passed the governed-write confirmation gate.
pub fn start_solar_project_sync(workspace: &Path, lane: &str) -> Result<u32, Failure> {
    if !matches!(lane, "stable" | "canary") || !workspace.is_absolute() {
        return Err(Failure::invalid(
            "solar_project_worker_input",
            "invalid Solar worker identity",
        ));
    }
    let executable = std::env::current_exe().map_err(|_| {
        Failure::failed(
            "solar_project_worker_start",
            "cannot resolve the running ds executable",
        )
    })?;
    let mut command = Command::new(executable);
    command
        .args(["solar", "project", "sync", "--workspace"])
        .arg(workspace)
        .args(["--lane", lane, "--watch", "--yes", "--output", "json"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000200);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn().map_err(|_| {
        Failure::failed(
            "solar_project_worker_start",
            "cannot start the Solar publication worker",
        )
    })?;
    Ok(child.id())
}
