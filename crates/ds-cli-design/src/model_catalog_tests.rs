use super::*;

fn parsed(tokens: &[&str]) -> Result<Inputs, Failure> {
    ds_cli_contract::parse(
        &COMMAND,
        &tokens.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
}
#[test]
fn grammar_defaults_to_the_offline_held_read_and_declares_every_owned_code() {
    let inputs = parsed(&["--project", "template"]).unwrap();
    assert_eq!(inputs.value("read"), Some("held"));
    assert!(parsed(&["--project", "template", "--read", "eager"]).is_err());
    assert!(parsed(&["--read", "held"]).is_err());
    for code in OWN.iter().map(|refusal| refusal.code) {
        assert!(
            REFUSALS.iter().any(|refusal| refusal.code == code),
            "{code}"
        );
    }
    assert!(COMMAND.summary.len() <= 70);
}
#[test]
fn retain_refuses_a_refresh_or_a_version_without_identity_before_any_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("version.json");
    std::fs::write(&path, br#"{"revision_id":"r1"}"#).unwrap();
    assert_eq!(
        retained_version(path.to_str().unwrap()).unwrap_err().code(),
        INPUT.code
    );
    let inputs = parsed(&[
        "--project",
        "template",
        "--read",
        "refresh",
        "--retain",
        path.to_str().unwrap(),
    ])
    .unwrap();
    let context = Context {
        confirmed: false,
        output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
    };
    assert_eq!(run(&inputs, &context).unwrap_err().code(), INPUT.code);
}
