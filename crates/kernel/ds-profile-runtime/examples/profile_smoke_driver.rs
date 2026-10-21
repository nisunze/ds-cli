//! Test fixture driver for the renderer smoke; never packaged as a DS command.
use ds_profile_runtime::{InitializeRequest, RefreshRequest, Request, Runtime};
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
    let session = ds_grid_exchange::linked_models::open_session(&package);
    let seed = ds_grid_exchange::profile_session::ProfileSessionSeed::capture(
        &session,
        package.manifest.model.model_id.clone(),
        package.manifest.model.presentation.clone(),
        &package.assets,
    );
    let seed_json = serde_json::to_string(&seed)?;
    let journal_json = serde_json::to_string(&session.retained_journal())?;
    let mut initialized = false;
    let mut runtime = Runtime::default();
    for line in io::stdin().lock().lines() {
        let value: Value = serde_json::from_str(&line?)?;
        let patch = serde_json::from_value(value["patch"].clone())?;
        let analyze = value["analyze"]
            .as_bool()
            .ok_or("require analyze boolean")?;
        let request = if initialized {
            Request::from(RefreshRequest {
                scope: "profile-display-smoke".into(),
                publication_id: None,
                model_id: seed.model_id.to_string(),
                base_revision: session.current_revision().revision_id.clone(),
                base_history: session.history_state(),
                expected_revision: session.current_revision().revision_id.clone(),
                target_history: session.history_state(),
                journal_json: journal_json.clone(),
                options: Default::default(),
                style_document: styles.clone(),
                patch,
                analyze,
                calculated_base_signature: None,
            })
        } else {
            Request::from(InitializeRequest {
                scope: "profile-display-smoke".into(),
                publication_id: None,
                seed_json: seed_json.clone(),
                restore_state: None,
                options: Default::default(),
                style_document: styles.clone(),
                patch,
                analyze,
            })
        };
        let result = runtime.execute(request);
        initialized |= result.is_ok();
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
