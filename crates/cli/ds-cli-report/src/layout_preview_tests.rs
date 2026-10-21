use super::*;
fn parse(tokens: &[&str]) -> Inputs {
    ds_cli_contract::args::parse(
        &crate::layout::RENDER,
        &tokens.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .unwrap()
}
#[test]
fn preview_admission_is_declared_and_refuses_before_identity_or_input_file_reads() {
    let inputs = parse(&["--request", "unread.json", "--preview"]);
    assert_eq!(preflight(&inputs).unwrap_err().code(), "project_required");
    let inputs = parse(&["--request", "unread.json", "--preview-retention", "7d"]);
    assert_eq!(preflight(&inputs).unwrap_err().code(), "printing_invalid");
    let inputs = parse(&["--request", "unread.json", "--preview-format", "pdf"]);
    assert_eq!(preflight(&inputs).unwrap_err().code(), "printing_invalid");
    assert!(
        ds_cli_contract::args::parse(
            &crate::layout::RENDER,
            &[
                "--request".into(),
                "unread.json".into(),
                "--preview-format".into(),
                "png".into()
            ]
        )
        .is_err()
    );
    let inputs = parse(&[
        "--request",
        "unread.json",
        "--preview",
        "--project",
        "p",
        "--lane",
        "canary",
        "--preview-retention",
        "7d",
    ]);
    assert!(preflight(&inputs).is_ok());
    assert_eq!(inputs.require("lane").unwrap(), "canary");
    // Existing delivery remains local and needs no native account.
    assert!(preflight(&parse(&["--request", "unread.json"])).is_ok());
}
