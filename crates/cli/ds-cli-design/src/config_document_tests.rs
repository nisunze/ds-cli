use super::*;
use ds_command_kernel::gateway_exchange::design::{
    ProjectConfigurationDocument, ProjectConfigurationObservation,
};
use std::cell::Cell;

fn private_root() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir().expect("private configuration fixture")
}

fn inputs(args: &[&str]) -> Inputs {
    ds_cli_contract::parse(
        &READ,
        &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
    )
    .unwrap()
}
fn reply(project: &str, source: ProjectConfigurationRead) -> ProjectConfigurationReply {
    ProjectConfigurationReply { project:project.into(),
        document:serde_json::from_value(json!({"sheets":{"project_settings":[]},"parameters":{},"future_metadata":{"producer":"other-build"}})).unwrap(),
        observation:ProjectConfigurationObservation {source,saved_at_ms:123} }
}

#[test]
fn the_document_handler_retains_one_complete_typed_reply_without_sheet_iteration() {
    let root = private_root();
    let path = root.path().join("configuration.json");
    let i = inputs(&[
        "--project",
        "project-p",
        "--lane",
        "canary",
        "--document",
        "--read",
        "refresh",
        "--out",
        path.to_str().unwrap(),
    ]);
    let calls = Cell::new(0);
    let result = read_with(&i, |lane, project, mode| {
        calls.set(calls.get() + 1);
        assert_eq!(
            (lane, project, mode),
            ("canary", "project-p", ProjectConfigurationRead::Refresh)
        );
        Ok(reply(project, mode))
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(result["project"], "project-p");
    assert_eq!(
        result["observation"],
        json!({"source":"refresh","saved_at_ms":123})
    );
    assert!(
        result.get("document").is_none(),
        "full bytes travel through the output file"
    );
    let file: ProjectConfigurationDocument =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        file,
        reply("project-p", ProjectConfigurationRead::Refresh).document
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn held_mode_reaches_the_same_handler_and_typed_reply() {
    let i = inputs(&[
        "--project",
        "project-p",
        "--lane",
        "canary",
        "--document",
        "--read",
        "held",
    ]);
    let result = read_with(&i, |_, project, mode| {
        assert_eq!(mode, ProjectConfigurationRead::Held);
        Ok(reply(project, mode))
    })
    .unwrap();
    let decoded: ProjectConfigurationReply = serde_json::from_value(result).unwrap();
    assert_eq!(decoded, reply("project-p", ProjectConfigurationRead::Held));
}

#[test]
fn invalid_document_sheet_or_page_inputs_refuse_before_any_owner_call() {
    for args in [
        vec!["--document", "--sheet", "project_settings"],
        vec!["--document", "--rule-set", "one"],
        vec!["--document", "--limit", "1"],
        vec!["--document", "--offset", "1"],
        vec!["--read", "held", "--sheet", "project_settings"],
        vec![],
    ] {
        let mut tokens = vec!["--project", "project-p", "--lane", "canary"];
        tokens.extend(args);
        let i = inputs(&tokens);
        assert_eq!(
            read_with(&i, |_, _, _| panic!("invalid input cannot acquire data"))
                .unwrap_err()
                .code(),
            "config_input_invalid"
        );
    }
    let tokens = ["--project", "project-p", "--document", "--read", "implicit"].map(str::to_owned);
    assert!(ds_cli_contract::parse(&READ, &tokens).is_err());
}

#[test]
fn a_crossed_project_or_mode_receipt_cannot_be_written() {
    for crossed in [
        reply("project-q", ProjectConfigurationRead::Refresh),
        reply("project-p", ProjectConfigurationRead::Held),
    ] {
        let root = private_root();
        let path = root.path().join("config.json");
        let i = inputs(&[
            "--project",
            "project-p",
            "--document",
            "--read",
            "refresh",
            "--out",
            path.to_str().unwrap(),
        ]);
        assert_eq!(
            read_with(&i, |_, _, _| Ok(crossed)).unwrap_err().code(),
            "config_input_invalid"
        );
        assert!(!path.exists());
    }
}

#[test]
fn an_existing_output_refuses_without_acquisition_and_preserves_bytes() {
    let root = private_root();
    let path = root.path().join("config.json");
    std::fs::write(&path, b"operator-owned").unwrap();
    let i = inputs(&[
        "--project",
        "project-p",
        "--document",
        "--out",
        path.to_str().unwrap(),
    ]);
    assert_eq!(
        read_with(&i, |_, _, _| panic!("existing output cannot acquire data"))
            .unwrap_err()
            .code(),
        "configuration_output_exists"
    );
    assert_eq!(std::fs::read(path).unwrap(), b"operator-owned");
}
