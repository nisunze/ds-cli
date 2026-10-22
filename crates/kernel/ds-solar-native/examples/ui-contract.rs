//! Test transport for the real, headless native application boundary.
//! No credentials, network, or project data: only a disposable fixture workspace.
use ds_solar_native::{NativeHost, application};
use serde_json::{Value, json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args_os().nth(1).ok_or("fixture state required")?;
    if directory == "--schemas" {
        use ds_solar_native::native::*;
        println!(
            "{}",
            json!({
                "NativePrepareCommand": schemars::schema_for!(PrepareCapturedCommand),
                "NativeCalculateCommand": schemars::schema_for!(CalculateCommand),
                "NativePortfolioCalculateCommand": schemars::schema_for!(PortfolioCalculateCommand),
                "NativeDocumentSelector": schemars::schema_for!(DocumentCommand),
                "NativeLoadProfileSelector": schemars::schema_for!(LoadProfileCommand),
                "NativeBatchArtifactSelector": schemars::schema_for!(BatchArtifactCommand),
                "NativePortfolioBatchOutputSelector": schemars::schema_for!(PortfolioBatchOutputCommand),
            })
        );
        return Ok(());
    }
    let host = NativeHost::headless(directory.into())?;
    let workspace =
        ds_solar_native::workspace::solar_native_prepare_default_workspace(host.clone())?;
    let file = std::env::args_os().nth(2).ok_or("request file required")?;
    let bytes = std::fs::read_to_string(file)?;
    let mut request: Value = serde_json::from_str(&bytes)?;
    if request["command"].get("workspace_handle").is_some() {
        request["command"]["workspace_handle"] = json!(workspace.workspace_handle);
    }
    let result = application::execute(host, "project-test", &serde_json::to_vec(&request)?);
    println!(
        "{}",
        match result {
            Ok(value) => json!({"value": value}),
            Err(error) => json!({"error": error}),
        }
    );
    Ok(())
}
