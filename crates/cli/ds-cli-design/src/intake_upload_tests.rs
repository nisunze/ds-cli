use super::*;

#[test]
fn command_is_headless_project_and_global_write() {
    assert_eq!(COMMAND.authority, Authority::HeadlessProject);
    assert_eq!(COMMAND.effect, Effect::GlobalWrite);
    assert!(!COMMAND.authority.requires_desktop());
}

#[test]
fn settings_reject_non_object() {
    let path = std::env::temp_dir().join(format!("ds-upload-settings-{}.json", std::process::id()));
    std::fs::write(&path, b"[]").unwrap();
    let result = read_settings(path.to_str());
    let _ = std::fs::remove_file(path);
    assert!(result.is_err());
}

/// Every code the edge processing step can construct is declared, each once,
/// alongside the Network engine's and the governed save's own refusals.
#[test]
fn the_edge_processing_refusals_are_declared_once() {
    let codes: Vec<&str> = COMMAND
        .refusals
        .iter()
        .map(|refusal| refusal.code)
        .collect();
    for code in [
        "status_upload_invalid",
        "status_upload_layers_unreadable",
        "status_upload_layers_unrecognized",
        "status_upload_transformer_unregistered",
        "network_engine_missing",
        "fast_lv_save_input_invalid",
        "transformer_context_unfenced",
    ] {
        assert!(codes.contains(&code), "{code} undeclared");
    }
    let mut unique = codes.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), codes.len());
    assert!(COMMAND.purpose.contains("processed on this machine"));
    assert!(COMMAND.purpose.contains("ds-brain only stores them"));
}
