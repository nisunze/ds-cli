use super::*;
use ds_cli_contract::{Format, Output};

fn context() -> Context {
    Context {
        confirmed: false,
        output: Output::resolve(Format::Json, false, true),
    }
}

fn run(args: &[&str]) -> Result<Value, Failure> {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let inputs = ds_cli_contract::parse(&INSPECT, &args)?;
    inspect(
        &inputs,
        &context(),
        json!({"product":"ds","version":"0.1.0","source_sha":"3".repeat(40),"profile":"debug","envelope":1}),
    )
}

#[test]
fn the_descriptor_is_a_host_read_with_closed_inputs() {
    assert_eq!(INSPECT.effect, Effect::ReadOnly);
    assert_eq!(INSPECT.authority, Authority::None);
    assert!(!INSPECT.requires_confirmation());
    assert!(INSPECT.summary.len() <= 70);
    assert_eq!(
        INSPECT.args.iter().map(|arg| arg.name).collect::<Vec<_>>(),
        [
            "component",
            "lane",
            "installed-version",
            "installed-build",
            "installed-built-at",
            "offline"
        ]
    );
    for forbidden in ["project", "file", "url", "path", "release", "record"] {
        assert!(INSPECT.arg(forbidden).is_none(), "{forbidden}");
    }
}

#[test]
fn core_names_itself_and_the_main_application_names_its_whole_build() {
    let refused = run(&["--component", "core", "--installed-version", "0.1.0"]).unwrap_err();
    assert_eq!(refused.code(), "release_identity_invalid");
    let partial = run(&["--component", "shell", "--installed-build", &"6".repeat(64)]).unwrap_err();
    assert_eq!(partial.code(), "release_identity_invalid");
    // A development Core reads only its own lane.
    let crossed = run(&["--component", "core", "--lane", "canary"]).unwrap_err();
    assert_eq!(crossed.code(), "release_identity_invalid");
}

#[test]
fn local_development_projects_the_build_without_a_catalog_or_record() {
    let shell = run(&[
        "--component",
        "shell",
        "--installed-version",
        "0.2.0",
        "--installed-build",
        &"6".repeat(64),
        "--installed-built-at",
        "2026-10-20T08:06:40.396Z",
    ])
    .unwrap();
    assert_eq!(shell["component"], "shell");
    assert_eq!(shell["channel"], "local");
    assert_eq!(shell["catalog"]["state"], "local");
    assert_eq!(
        shell["installed"],
        json!({"version":"0.2.0","buildNumber":null,"releaseDate":null})
    );
    assert_eq!(shell["installation"]["source"], "unrecorded");
    assert!(shell["published"].is_null() && shell["update"].is_null());
    let core = run(&["--component", "core"]).unwrap();
    assert_eq!(core["installed"]["version"], "0.1.0");
    assert_eq!(core["catalog"]["state"], "local");
}
