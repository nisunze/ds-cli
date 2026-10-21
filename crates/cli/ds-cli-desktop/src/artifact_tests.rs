use super::*;

#[test]
fn bridge_contract_has_only_exact_selector_and_destination() {
    assert_eq!(
        READ_OP.arguments,
        ["project", "transformer", "outputId", "intent"]
    );
    assert_eq!(
        COPY_OP.arguments,
        [
            "project",
            "transformer",
            "outputId",
            "destination",
            "intent"
        ]
    );
}

#[test]
fn copy_refuses_an_existing_destination_before_pairing() {
    let temporary = tempfile::NamedTempFile::new().unwrap();
    let error = absolute_new_destination(temporary.path().to_str().unwrap()).unwrap_err();
    assert_eq!(error.code(), "printing_artifact_path_exists");
}

fn inputs(command: &Command, intent: Option<&str>) -> Result<Inputs, Failure> {
    let mut tokens = vec![
        "--project",
        "survey_test",
        "--transformer",
        "agasharu",
        "--output-id",
        "pdf__a3",
    ];
    if command.id == COPY_COMMAND.id {
        tokens.extend(["--out", "./agasharu-preview.pdf"]);
    }
    if let Some(intent) = intent {
        tokens.extend(["--intent", intent]);
    }
    ds_cli_contract::args::parse(
        command,
        &tokens.into_iter().map(str::to_owned).collect::<Vec<_>>(),
    )
}

#[test]
fn read_and_copy_preserve_main_selectors_and_forward_explicit_preview() {
    let main = json!({"project":"survey_test","transformer":"agasharu","outputId":"pdf__a3"});
    for command in [&READ_COMMAND, &COPY_COMMAND] {
        assert_eq!(selector(&inputs(command, None).unwrap()).unwrap(), main);
        let mut preview = main.clone();
        preview["intent"] = json!("preview");
        assert_eq!(
            selector(&inputs(command, Some("preview")).unwrap()).unwrap(),
            preview
        );
    }
}

#[test]
fn read_and_copy_refuse_unknown_intent_before_pairing() {
    for command in [&READ_COMMAND, &COPY_COMMAND] {
        assert_eq!(
            inputs(command, Some("main")).unwrap_err().code(),
            "invalid_choice"
        );
    }
}
