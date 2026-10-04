//! Test fixture driver for the renderer smoke; never packaged as a DS command.
use ds_profile_runtime::{ProjectRequest as Request, Runtime};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().collect();
    if arguments.len() != 3 {
        return Err("require the test .dsgrid and governed style document paths".into());
    }
    let bytes = std::fs::read(&arguments[1])?;
    let styles: Value = serde_json::from_slice(&std::fs::read(&arguments[2])?)?;
    let (package, _) = ds_grid_exchange::dsgrid::ingest(&bytes)?;
    let session = ds_grid_engine::GridSession::open(package.snapshot);
    let revision = session.current_revision().revision_id.to_string();
    let mut runtime = Runtime::default();
    for line in io::stdin().lock().lines() {
        let value: Value = serde_json::from_str(&line?)?;
        let result = runtime.execute(Request {
            scope: "profile-display-smoke".into(),
            expected_revision: revision.clone(),
            package_bytes: bytes.clone(),
            style_document: styles.clone(),
            options: Default::default(),
            patch: serde_json::from_value(value["patch"].clone())?,
            analyze: value["analyze"]
                .as_bool()
                .ok_or("require analyze boolean")?,
        });
        println!(
            "{}",
            match result {
                Ok(value) => json!({"ok": value}),
                Err(error) => json!({"error": error}),
            }
        );
        io::stdout().flush()?;
    }
    Ok(())
}
