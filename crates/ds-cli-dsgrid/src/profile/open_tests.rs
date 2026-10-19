use super::*;

#[test]
fn live_capture_validates_native_history_without_catalogue_or_pairing() {
    let history = r#"{"model_revision":"rev:head","initial_revision":"rev:head","undo_depth":0,"redo_depth":1,"history_pin":"pin"}"#;
    let parse = |extra: &[&str]| {
        let mut args = vec![
            "--model",
            "browser-only",
            "--live",
            "--checkpoint-out",
            "/unused/new.dsgrid",
        ];
        args.extend_from_slice(extra);
        ds_cli_contract::args::parse(
            &COMMAND,
            &args.iter().map(|v| (*v).into()).collect::<Vec<String>>(),
        )
        .unwrap()
    };
    let inputs = parse(&["--expect-history", history]);
    let arguments = live_arguments(&inputs).unwrap();
    assert_eq!(arguments["model"], "browser-only");
    assert_eq!(arguments["expect_history"]["redo_depth"], 1);
    assert!(arguments.get("path").is_none());
    assert!(arguments.get("held").is_none());
    let held = live_arguments(&parse(&["--expect-history", history, "--held"])).unwrap();
    assert_eq!(held["held"], true);
    assert!(held.get("path").is_none());
    assert!(live_arguments(&parse(&[])).is_err());
    assert!(
        live_arguments(&parse(&[
            "--expect-history",
            history,
            "--alignment",
            "al-1"
        ]))
        .is_err()
    );
    assert!(live_arguments(&parse(&["--expect-history", r#"{"project":"untrusted"}"#])).is_err());
    let data = receipt("browser-only", &json!({"model":"browser-only","profile_open":true,"live_checkpoint":true,"checkpoint":{"persisted":true}})).unwrap();
    assert!(data["alignment"].is_null());
}

#[test]
fn profile_open_receipt_keeps_temporary_removal_counts() {
    let reply = json!({
        "model": "local-1",
        "profile_open": true,
        "alignment": "al-1",
        "temporary_models_removed": {"groups": 1, "models": 2}
    });
    let data = receipt("local-1", &reply).expect("valid profile open");
    assert_eq!(
        data["temporary_models_removed"],
        json!({"groups": 1, "models": 2})
    );
    let mut no_replacement = reply;
    no_replacement
        .as_object_mut()
        .unwrap()
        .remove("temporary_models_removed");
    let data = receipt("local-1", &no_replacement).expect("valid profile open");
    assert!(data.get("temporary_models_removed").is_none());
}
