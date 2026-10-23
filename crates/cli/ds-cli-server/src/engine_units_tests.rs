use super::*;
use ds_cli_contract::spec::Command;

fn inputs(command: &Command, channel: &str, runtime: Option<&str>, extra: &[&str]) -> Inputs {
    let mut args = vec!["--component", "network", "--channel", channel];
    if let Some(runtime) = runtime {
        args.extend(["--runtime", runtime]);
    }
    args.extend(extra);
    ds_cli_contract::parse(
        command,
        &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
    )
    .unwrap()
}
fn context(confirmed: bool) -> Context {
    Context {
        confirmed,
        output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
    }
}

#[test]
fn explicit_wasm_selects_the_network_page_unit_and_native_keeps_the_addon() {
    let selection = &crate::core_components::SELECTION;
    assert!(requested(&inputs(selection, "canary", Some("wasm"), &[])).unwrap());
    assert!(!requested(&inputs(selection, "canary", None, &[])).unwrap());
    assert!(!requested(&inputs(selection, "canary", Some("native"), &[])).unwrap());
    // The two UI units keep their own path; Solar and Reporter WASM stay refused.
    for component in ["interactions", "solar", "reporter"] {
        let args = ["--component", component, "--channel", "canary", "--runtime", "wasm"];
        let input = ds_cli_contract::parse(
            selection,
            &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(!requested(&input).unwrap(), "{component}");
    }
    // stage-local defaults to runtime wasm.
    let local = inputs(
        &crate::ui_components::STAGE_LOCAL,
        "canary",
        None,
        &[
            "--directory",
            "/abs/producer",
            "--identity-file",
            "/abs/engine-identity.json",
            "--source-sha",
            &"a".repeat(40),
            "--wasm-sha256",
            &"b".repeat(64),
        ],
    );
    assert!(requested(&local).unwrap());
    assert_eq!(
        crate::ui_components::stage_local(&local, &context(false))
            .unwrap_err()
            .code(),
        "confirmation_required"
    );
}

#[test]
fn local_scope_never_enters_stable_and_member_reads_stay_bounded_before_any_store_read() {
    let stable_local = inputs(
        &crate::core_components::SELECTION,
        "stable",
        Some("wasm"),
        &["--scope", "local-dev"],
    );
    assert_eq!(selected(&stable_local).unwrap_err().code(), "module_scope_invalid");
    let published_into_local = inputs(
        &crate::module_updates::STAGE,
        "canary",
        Some("wasm"),
        &["--sha256", &"c".repeat(64)],
    );
    assert!(requested(&published_into_local).unwrap());
    for (name, limit, digest) in [
        ("../selection.json", "10", "a".repeat(64)),
        ("ds_interactions_wasm.js", "10", "a".repeat(64)),
        ("ds_network_wasm.js", "1048577", "a".repeat(64)),
        ("ds_network_wasm.js", "0", "a".repeat(64)),
        ("ds_network_wasm.js", "10", "A".repeat(64)),
    ] {
        let input = inputs(
            &crate::ui_components::MEMBER,
            "canary",
            Some("wasm"),
            &["--unit-sha256", &digest, "--filename", name, "--limit", limit],
        );
        assert_eq!(
            crate::ui_components::member(&input, &context(false))
                .unwrap_err()
                .code(),
            "module_artifact_invalid",
            "{name} {limit}"
        );
    }
}
