use super::*;

#[test]
fn remote_context_refuses_before_auth_or_manifest_io() {
    for extra in [
        &[][..],
        &["--local-source", "/nonexistent/never-read.json"][..],
        &["--publisher-input", "/nonexistent/never-read.json"][..],
    ] {
        let mut tokens = vec!["--project", "fixture-project", "--remote-context"];
        tokens.extend_from_slice(extra);
        let tokens: Vec<String> = tokens.into_iter().map(str::to_owned).collect();
        let inputs = ds_cli_contract::args::parse(&SEED_COMMAND, &tokens).unwrap();
        let error = run_seed(
            &inputs,
            &Context {
                confirmed: true,
                output: ds_cli_contract::Output::resolve(
                    ds_cli_contract::Format::Json,
                    false,
                    true,
                ),
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "native_compute_required");
        assert!(
            SEED_COMMAND
                .refusals
                .iter()
                .any(|refusal| refusal.code == error.code()),
            "the refusal is declared"
        );
    }
    // Without the switch the seed proceeds past source-mode validation.
    let inputs = ds_cli_contract::args::parse(
        &SEED_COMMAND,
        &["--project".to_owned(), "fixture-project".to_owned()],
    )
    .unwrap();
    assert!(validate_source_mode(&inputs).is_ok());
    assert!(
        !STATUS_COMMAND
            .args
            .iter()
            .any(|arg| arg.name == "remote-context")
    );
    assert!(
        !QUERY_COMMAND
            .args
            .iter()
            .any(|arg| arg.name == "remote-context")
    );
}
