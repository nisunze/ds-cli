//! Compatibility file adapter for the shared Solar-owned local controls.
use anyhow::{Result, ensure};
use ds_solar_project::{control, files};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug, clap::Args)]
pub struct Args {
    #[arg(long)]
    pub request: PathBuf,
    #[arg(long)]
    pub result: PathBuf,
}

pub fn execute(args: Args) -> Result<()> {
    ensure!(
        std::fs::symlink_metadata(&args.result).is_err(),
        "project result file already exists"
    );
    let request: Value = serde_json::from_slice(&files::read(&args.request, 32 * 1024 * 1024)?)?;
    let result = control::execute(request)?;
    files::json(&args.result, &result)?;
    println!(
        "{}",
        json!({"schema_version":"ds-solar.project-command/v1","result":args.result})
    );
    Ok(())
}
