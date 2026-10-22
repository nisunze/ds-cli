use super::*;
use std::io::Write;

fn inspect_file(
    path: &std::path::Path,
    component: &str,
    runtime: Option<&str>,
) -> Result<Value, Failure> {
    let mut tokens = vec![
        "--component".into(),
        component.into(),
        "--channel".into(),
        "canary".into(),
        "--file".into(),
        path.to_string_lossy().into_owned(),
    ];
    if let Some(runtime) = runtime {
        tokens.extend(["--runtime".into(), runtime.into()]);
    }
    let inputs = ds_cli_contract::parse(&RELEASE, &tokens)?;
    release(
        &inputs,
        &Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(
                ds_cli_contract::Format::Json,
                false,
                true,
            ),
        },
    )
}

#[test]
fn browser_release_inspection_is_explicit_and_native_default_is_preserved() {
    let mut browser = tempfile::NamedTempFile::new().unwrap();
    browser
        .write_all(include_bytes!(
            "../../../kernel/ds-command-kernel/src/module_release/fixtures/network-wasm-canary.json"
        ))
        .unwrap();
    assert!(inspect_file(browser.path(), "network", None).is_err());
    let admitted = inspect_file(browser.path(), "network", Some("wasm")).unwrap();
    assert_eq!(admitted["runtime"], "wasm");
    assert_eq!(admitted["protocol"], "ds.network-wasm/v1");
    assert_eq!(admitted["assets"].as_array().unwrap().len(), 4);
    let mut native = tempfile::NamedTempFile::new().unwrap();
    native.write_all(include_bytes!("../../../kernel/ds-command-kernel/src/module_release/fixtures/network-native-canary.json")).unwrap();
    let legacy = inspect_file(native.path(), "network", None).unwrap();
    assert_eq!(
        legacy,
        inspect_file(native.path(), "network", Some("native")).unwrap()
    );
    assert_eq!(legacy["protocol"], "ds.network-engine/v1");
    assert!(legacy.get("runtime").is_none());
    assert!(inspect_file(native.path(), "network", Some("wasm")).is_err());
}

#[test]
fn unsupported_browser_target_refuses_before_file_access_and_unknown_runtime_is_not_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("must-not-read.json");
    let error = inspect_file(&absent, "solar", Some("wasm")).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("module_release_target_unsupported"),
        "{error}"
    );
    // Reporter has an admitted browser unit (Kernel 6a8ad64), so its target
    // passes and only then is the absent file refused.
    let error = inspect_file(&absent, "reporter", Some("wasm")).unwrap_err();
    assert!(
        !error
            .to_string()
            .contains("module_release_target_unsupported"),
        "{error}"
    );
    assert_eq!(
        inspect_file(&absent, "network", Some("other"))
            .unwrap_err()
            .code(),
        "invalid_choice"
    );
}
#[test]
fn inventory_explicit_lane_is_optional_and_closed() {
    for lane in ["stable", "canary"] {
        let inputs = ds_cli_contract::parse(
            &INVENTORY,
            &[
                "--lane".into(),
                lane.into(),
                "--component".into(),
                "solar".into(),
            ],
        )
        .unwrap();
        assert_eq!(inputs.value("lane"), Some(lane));
        assert_eq!(inputs.value("component"), Some("solar"));
    }
    assert!(
        ds_cli_contract::parse(&INVENTORY, &[])
            .unwrap()
            .value("lane")
            .is_none()
    );
    for lane in ["local", "server", "other"] {
        assert_eq!(
            ds_cli_contract::parse(&INVENTORY, &["--lane".into(), lane.into()])
                .unwrap_err()
                .code(),
            "invalid_choice"
        );
    }
}

#[test]
fn unsupported_release_lane_refuses_before_any_addon_probe() {
    let error = inventory_for_release_lane(
        Some(ds_command_kernel::module_release::ModuleComponent::Network),
        "server",
    )
    .unwrap_err();
    assert_eq!(error.code(), "module_lane_invalid");
}

#[test]
fn ui_inspection_returns_complete_digest_and_requires_explicit_wasm_before_file_access() {
    for (component, bytes) in [
        ("interactions", include_bytes!("../../../kernel/ds-command-kernel/src/module_release/fixtures/interactions-ui-wasm-canary.json").as_slice()),
        ("command-kernel", include_bytes!("../../../kernel/ds-command-kernel/src/module_release/fixtures/command-kernel-ui-wasm-canary.json").as_slice()),
    ] {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        let inspected = inspect_file(file.path(), component, Some("wasm")).unwrap();
        assert_eq!(inspected["release"]["protocol"], "ds.ui-wasm/v1");
        assert_eq!(inspected["release"]["assets"].as_array().unwrap().len(), 4);
        assert_eq!(inspected["unit_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(inspected["displayIdentity"]["version"].as_str(), inspected["release"]["version"].as_str().map(|v| v.split("-r").next().unwrap()));
        let absent = file.path().with_extension("absent");
        assert!(inspect_file(&absent, component, None).unwrap_err().to_string().contains("module_release_component_invalid"));
        assert!(inspect_file(&absent, component, Some("native")).unwrap_err().to_string().contains("module_release_component_invalid"));
    }
}
