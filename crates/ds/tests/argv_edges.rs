//! Words on the command line that no command can accept are refused by name,
//! never with a panic.

use serde_json::Value;
use std::process::Command;

fn run(args: &[std::ffi::OsString]) -> (i32, Value, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .args(["--output", "json"])
        .env("DS_SRE_REPORTING", "off")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let envelope = serde_json::from_str(&stdout).unwrap_or(Value::Null);
    (
        output.status.code().unwrap_or(-1),
        envelope,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[cfg(unix)]
#[test]
fn a_non_utf8_argument_is_a_clean_invalid_input_error_not_a_panic() {
    use std::os::unix::ffi::OsStringExt;
    let bad = std::ffi::OsString::from_vec(vec![0xff, 0xfe]);
    let (code, envelope, stderr) = run(&[
        "pm".into(),
        "task".into(),
        "update".into(),
        "--description".into(),
        bad,
    ]);
    assert_eq!(code, 2, "invalid_input exits 2, not the panic code 101");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert_eq!(envelope["status"], "error");
    assert_eq!(envelope["error"]["class"], "invalid_input");
    assert_eq!(envelope["error"]["code"], "argument_not_utf8");
    assert_eq!(envelope["error"]["detail"]["argument_index"], 5);
    assert!(!envelope.to_string().contains('\u{fffd}'));
}

#[test]
fn lane_before_the_domain_says_where_it_goes_instead_of_unknown_domain() {
    for lane in [vec!["--lane", "canary"], vec!["--lane=canary"]] {
        let mut args: Vec<std::ffi::OsString> = lane.into_iter().map(Into::into).collect();
        args.extend(["auth".into(), "project".into(), "list".into()]);
        let (code, envelope, _) = run(&args);
        assert_eq!(code, 2);
        assert_eq!(envelope["error"]["code"], "lane_after_command");
        assert!(
            envelope["error"]["remedy"]
                .as_str()
                .unwrap()
                .contains("ds <domain> <command> --lane canary")
        );
    }
}

#[test]
fn a_genuinely_unknown_domain_is_still_unknown_domain() {
    let (_, envelope, _) = run(&["nonesuch".into()]);
    assert_eq!(envelope["error"]["code"], "unknown_domain");
}
