use super::*;

fn parse(tokens: &[&str]) -> Result<Inputs, Failure> {
    let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
    ds_cli_contract::parse(&COMMAND, &tokens)
}

#[test]
fn the_record_and_one_file_are_required_and_a_bad_file_is_refused_before_any_network() {
    assert!(parse(&["--project", "p", "--file", "f"]).is_err());
    assert!(parse(&["--project", "p", "--record", "r"]).is_err());
    let root = std::env::temp_dir().join(format!("pm-record-attach-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let empty = root.join("empty.rar");
    std::fs::write(&empty, b"").unwrap();
    for file in [root.join("absent.rar"), empty, root.clone()] {
        let file = file.to_str().unwrap().to_owned();
        let inputs = parse(&["--project", "p", "--record", "r", "--file", file.as_str()]).unwrap();
        let error = run(&inputs, &context()).unwrap_err();
        assert_eq!(error.code(), FILE_INVALID.code, "{file}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_descriptor_declares_a_confirmed_write_with_its_typed_refusals() {
    assert_eq!(COMMAND.effect, Effect::GlobalWrite);
    let codes: Vec<&str> = COMMAND
        .refusals
        .iter()
        .map(|refusal| refusal.code)
        .collect();
    for code in [
        FILE_INVALID.code,
        UNVERIFIED.code,
        crate::RECORD_NOT_FOUND.code,
    ] {
        assert!(codes.contains(&code), "{code}");
    }
}

fn context() -> Context {
    Context {
        confirmed: true,
        output: ds_cli_contract::output::Output {
            format: ds_cli_contract::output::Format::Json,
            pretty: false,
            color: false,
        },
    }
}
