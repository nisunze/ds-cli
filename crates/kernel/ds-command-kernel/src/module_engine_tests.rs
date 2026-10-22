use super::*;
use serde_json::json;

type Edit = Box<dyn Fn(&mut Value)>;

const SOURCE: &str = "8c4e3024eea453c0aac90d52db4e27e22804757c";
const CLOSURE: &str = "9ad94616c613c54a9f3d93bf1a008f106ee28a30324a24c812c1a6c6ac368bb4";

fn network() -> Value {
    json!({"schema":"ds.engine-build/v1","module":"network",
        "engineIdentity":{"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0",
            "sourceSha":SOURCE,"dependencyClosureSha256":CLOSURE,"environment":"native",
            "runtime":{"environment":"native","logicalCpus":16,"internalWorkerThreads":1},"jsonObjectOrder":"insertion",
            "capabilities":["lv_processing_v1","lv_differential_plan_v1","lv_publication_v1","lv_analysis_validation_v1",
                "grid_query_v1","grid_inspect_v1","grid_apply_v1","grid_publication_v1","profile_session_v1",
                "report_engineering_v1","report_sizing_v1","report_feeder_selection_v1","future_operation_v9"]},
        "build":{"target":"x86_64-unknown-linux-gnu","profile":"release","rustc":"rustc 1.97.0 (2d8144b78 2026-07-07)","cargoLockSha256":"f".repeat(64)}})
}

fn solar() -> Value {
    json!({"schema":"ds.engine-build/v1","name":"ds-solar-engine","version":"0.1.0","source_sha":"a".repeat(40),
        "target":"x86_64-pc-windows-msvc","profile":"release","rustc":"rustc 1.97.0","cargo_lock_sha256":"b".repeat(64),
        "build_manifest_sha256":"c".repeat(64),"schemas":SOLAR_SCHEMAS})
}

fn reporter() -> Value {
    let source = "d".repeat(40);
    json!({"schema":"ds.engine-build/v1","service":"ds-network-reporter","binary":"ds-report","package_version":"0.1.0",
        "source_sha":source,"engine_version":format!("ds-network-reporter@0.1.0+{source}"),"features":[],
        "target":"x86_64-unknown-linux-gnu","profile":"release","rustc":"rustc 1.97.0","cargo_lock_sha256":"e".repeat(64),
        "build_manifest_sha256":"1".repeat(64),"schemas":REPORTER_SCHEMAS})
}

#[test]
fn network_build_admits_its_own_facts_and_derives_the_release() {
    let admitted = admit_engine_build(ModuleComponent::Network, "linux", &network(), None).unwrap();
    assert_eq!(
        admitted,
        AdmittedEngineBuild {
            version: "0.1.0".into(),
            source_sha: SOURCE.into(),
            manifest_sha256: CLOSURE.into(),
            release: format!("ds-network@0.1.0+{SOURCE}"),
        }
    );
    let named = format!("ds-network@0.1.0+{SOURCE}");
    assert_eq!(
        admit_engine_build(ModuleComponent::Network, "linux", &network(), Some(&named)).unwrap(),
        admitted
    );
}

#[test]
fn solar_and_reporter_builds_admit_their_owner_facts() {
    let admitted = admit_engine_build(ModuleComponent::Solar, "windows", &solar(), None).unwrap();
    assert_eq!(
        admitted.release,
        format!("ds-solar@0.1.0+{}", "a".repeat(40))
    );
    assert_eq!(admitted.manifest_sha256, "c".repeat(64));
    let reporter = reporter();
    let admitted = admit_engine_build(ModuleComponent::Reporter, "linux", &reporter, None).unwrap();
    assert_eq!(admitted.release, reporter["engine_version"]);
    let named = reporter["engine_version"].as_str().unwrap().to_owned();
    assert!(
        admit_engine_build(ModuleComponent::Reporter, "linux", &reporter, Some(&named)).is_ok()
    );
}

#[test]
fn reporter_engine_release_must_match_the_named_or_derived_release() {
    let mut crossed = reporter();
    crossed["engine_version"] = json!(format!("ds-network-reporter@0.1.0+{}", "9".repeat(40)));
    let error = admit_engine_build(ModuleComponent::Reporter, "linux", &crossed, None).unwrap_err();
    assert!(error.contains("different Reporter owner"), "{error}");
    let other = format!("ds-network-reporter@0.1.0+{}", "8".repeat(40));
    assert!(admit_engine_build(
        ModuleComponent::Reporter,
        "linux",
        &reporter(),
        Some(&other)
    )
    .is_err());
}

#[test]
fn network_refusals_keep_their_typed_reasons() {
    let cases: Vec<(&str, Edit, &str)> = vec![
        (
            "schema",
            Box::new(|v| v["schema"] = json!("ds.engine-build/v2")),
            "protocol or target",
        ),
        (
            "module",
            Box::new(|v| v["module"] = json!("solar")),
            "protocol or target",
        ),
        (
            "protocol",
            Box::new(|v| v["engineIdentity"]["protocolMajor"] = json!(2)),
            "protocol or target",
        ),
        (
            "wasm",
            Box::new(|v| v["engineIdentity"]["environment"] = json!("wasm")),
            "protocol or target",
        ),
        (
            "runtime",
            Box::new(|v| v["engineIdentity"]["runtime"]["environment"] = json!("wasm")),
            "protocol or target",
        ),
        (
            "debug",
            Box::new(|v| v["build"]["profile"] = json!("debug")),
            "protocol or target",
        ),
        (
            "target",
            Box::new(|v| v["build"]["target"] = json!("x86_64-pc-windows-msvc")),
            "protocol or target",
        ),
        (
            "lockfile",
            Box::new(|v| v["build"]["cargoLockSha256"] = json!("F".repeat(64))),
            "compiler provenance",
        ),
        (
            "compiler",
            Box::new(|v| v["build"]["rustc"] = json!("")),
            "compiler provenance",
        ),
        (
            "long compiler",
            Box::new(|v| v["build"]["rustc"] = json!("r".repeat(4097))),
            "compiler provenance",
        ),
        (
            "no cpus",
            Box::new(|v| v["engineIdentity"]["runtime"]["logicalCpus"] = json!(0)),
            "capacity",
        ),
        (
            "oversubscribed",
            Box::new(|v| v["engineIdentity"]["runtime"]["internalWorkerThreads"] = json!(17)),
            "capacity",
        ),
        (
            "ordering",
            Box::new(|v| v["engineIdentity"]["jsonObjectOrder"] = json!("random")),
            "capacity",
        ),
        (
            "capability",
            Box::new(|v| v["engineIdentity"]["capabilities"] = json!(["lv_processing_v1"])),
            "required capability",
        ),
        (
            "repeat",
            Box::new(|v| {
                v["engineIdentity"]["capabilities"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("grid_query_v1"))
            }),
            "repeated",
        ),
        (
            "unbounded",
            Box::new(|v| {
                v["engineIdentity"]["capabilities"] =
                    json!((0..129).map(|i| format!("c{i}")).collect::<Vec<_>>())
            }),
            "bounded",
        ),
        (
            "token",
            Box::new(|v| {
                v["engineIdentity"]["capabilities"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(7))
            }),
            "capability token",
        ),
        (
            "source",
            Box::new(|v| v["engineIdentity"]["sourceSha"] = json!("main")),
            "exact producer",
        ),
        (
            "closure",
            Box::new(|v| v["engineIdentity"]["dependencyClosureSha256"] = json!("a".repeat(63))),
            "exact producer",
        ),
        (
            "version",
            Box::new(|v| v["engineIdentity"]["coreVersion"] = json!("0.1")),
            "exact producer",
        ),
        (
            "missing version",
            Box::new(|v| {
                v["engineIdentity"]
                    .as_object_mut()
                    .unwrap()
                    .remove("coreVersion");
            }),
            "missing coreVersion",
        ),
        (
            "oversize",
            Box::new(|v| v["build"]["padding"] = json!("x".repeat(MAX_BUILD_INFO_BYTES))),
            "excessive build-info",
        ),
    ];
    for (name, edit, reason) in cases {
        let mut value = network();
        edit(&mut value);
        let error =
            admit_engine_build(ModuleComponent::Network, "linux", &value, None).unwrap_err();
        assert!(
            error.starts_with("module_engine_incompatible:"),
            "{name}: {error}"
        );
        assert!(error.contains(reason), "{name}: {error}");
    }
    for required in NETWORK_CAPABILITIES {
        let mut value = network();
        value["engineIdentity"]["capabilities"]
            .as_array_mut()
            .unwrap()
            .retain(|cap| cap != required);
        assert!(
            admit_engine_build(ModuleComponent::Network, "linux", &value, None).is_err(),
            "{required}"
        );
    }
}

#[test]
fn solar_and_reporter_refuse_other_owners_and_unqualified_builds() {
    for (component, base, path, value) in [
        (ModuleComponent::Solar, solar(), "/name", json!("reporter")),
        (ModuleComponent::Solar, solar(), "/profile", json!("debug")),
        (
            ModuleComponent::Solar,
            solar(),
            "/target",
            json!("x86_64-unknown-linux-gnu"),
        ),
        (
            ModuleComponent::Solar,
            solar(),
            "/schemas",
            json!(["ds-solar.result/v1"]),
        ),
        (ModuleComponent::Solar, solar(), "/version", json!("1")),
        (
            ModuleComponent::Solar,
            solar(),
            "/build_manifest_sha256",
            json!(""),
        ),
        (
            ModuleComponent::Reporter,
            reporter(),
            "/features",
            json!(["default"]),
        ),
        (
            ModuleComponent::Reporter,
            reporter(),
            "/binary",
            json!("reporter"),
        ),
        (
            ModuleComponent::Reporter,
            reporter(),
            "/service",
            json!("ds-solar"),
        ),
        (
            ModuleComponent::Reporter,
            reporter(),
            "/cargo_lock_sha256",
            json!(null),
        ),
    ] {
        let mut changed = base.clone();
        *changed.pointer_mut(path).unwrap() = value;
        let platform = if component == ModuleComponent::Solar {
            "windows"
        } else {
            "linux"
        };
        let error = admit_engine_build(component, platform, &changed, None).unwrap_err();
        assert!(
            error.starts_with("module_engine_incompatible:"),
            "{path}: {error}"
        );
    }
    assert!(admit_engine_build(ModuleComponent::Solar, "linux", &solar(), None).is_err());
    assert!(admit_engine_build(ModuleComponent::Network, "linux", &solar(), None).is_err());
    assert!(admit_engine_build(ModuleComponent::Solar, "windows", &network(), None).is_err());
}

#[test]
fn platforms_and_component_names_are_closed() {
    let error =
        admit_engine_build(ModuleComponent::Network, "macos", &network(), None).unwrap_err();
    assert!(error.starts_with("module_platform_unsupported:"), "{error}");
    for component in [
        ModuleComponent::Solar,
        ModuleComponent::Reporter,
        ModuleComponent::Network,
    ] {
        assert_eq!(
            ModuleComponent::from_name(component.as_str()),
            Some(component)
        );
    }
    assert_eq!(ModuleComponent::from_name("core"), None);
    assert_eq!(ModuleComponent::Network.executable(), "ds-network");
    assert_eq!(ModuleComponent::Reporter.protocol(), MODULE_PROTOCOL);
    assert_eq!(ModuleComponent::Solar.engine_owner(), "ds-solar");
}

fn browser_network() -> Value {
    json!({"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0",
        "sourceSha":SOURCE,"dependencyClosureSha256":CLOSURE,"environment":"wasm",
        "runtime":{"environment":"wasm","logicalCpus":1,"internalWorkerThreads":1},"jsonObjectOrder":"sorted",
        "capabilities":["lv_processing_v1","grid_query_v1"]})
}

fn browser_reporter() -> Value {
    json!({"schema":"ds.reporter-engine/v1","protocolMajor":1,"coreVersion":"0.1.0","sourceSha":SOURCE,
        "dependencyClosureSha256":CLOSURE,"environment":"wasm","capabilities":["render_held_svg_pdf_v1"]})
}

#[test]
fn browser_engine_units_admit_their_identity_facts() {
    let admitted =
        admit_browser_engine_build(ModuleComponent::Network, &browser_network(), None).unwrap();
    assert_eq!(admitted.release, format!("ds-network@0.1.0+{SOURCE}"));
    assert_eq!(admitted.manifest_sha256, CLOSURE);
    let producer = BrowserEngineProducer {
        version: "0.1.0",
        source_sha: SOURCE,
        manifest_sha256: CLOSURE,
    };
    assert!(admit_browser_engine_build(
        ModuleComponent::Reporter,
        &browser_reporter(),
        Some(producer)
    )
    .is_ok());
    assert_eq!(
        browser_engine_profile(ModuleComponent::Reporter)
            .unwrap()
            .stem,
        "ds_reporter_wasm"
    );
    assert!(browser_engine_profile(ModuleComponent::Solar).is_none());
    let solar = admit_browser_engine_build(ModuleComponent::Solar, &browser_network(), None);
    assert!(solar
        .unwrap_err()
        .starts_with("module_release_target_unsupported:"));
}

#[test]
fn browser_engine_units_refuse_native_crossed_or_unversioned_identities() {
    for (component, base, path, value) in [
        (
            ModuleComponent::Network,
            browser_network(),
            "/environment",
            json!("native"),
        ),
        (
            ModuleComponent::Network,
            browser_network(),
            "/runtime/environment",
            json!("native"),
        ),
        (
            ModuleComponent::Network,
            browser_network(),
            "/schema",
            json!("ds.reporter-engine/v1"),
        ),
        (
            ModuleComponent::Network,
            browser_network(),
            "/protocolMajor",
            json!(2),
        ),
        (
            ModuleComponent::Network,
            browser_network(),
            "/jsonObjectOrder",
            json!("random"),
        ),
        (
            ModuleComponent::Network,
            browser_network(),
            "/capabilities",
            json!(["grid_query_v1"]),
        ),
        (
            ModuleComponent::Network,
            browser_network(),
            "/coreVersion",
            json!("0.1"),
        ),
        (
            ModuleComponent::Network,
            browser_network(),
            "/sourceSha",
            json!("main"),
        ),
        (
            ModuleComponent::Reporter,
            browser_reporter(),
            "/capabilities",
            json!(["render_held_svg_pdf_v1", "extra_v1"]),
        ),
        (
            ModuleComponent::Reporter,
            browser_reporter(),
            "/coreVersion",
            json!("0.2.0"),
        ),
    ] {
        let mut changed = base;
        *changed.pointer_mut(path).unwrap() = value;
        let error = admit_browser_engine_build(component, &changed, None).unwrap_err();
        assert!(
            error.starts_with("module_engine_incompatible:"),
            "{path}: {error}"
        );
    }
    let crossed = BrowserEngineProducer {
        version: "0.1.0",
        source_sha: SOURCE,
        manifest_sha256: &"0".repeat(64),
    };
    assert!(admit_browser_engine_build(
        ModuleComponent::Network,
        &browser_network(),
        Some(crossed)
    )
    .is_err());
    let mut oversize = browser_network();
    oversize["padding"] = json!("x".repeat(MAX_BUILD_INFO_BYTES));
    let error = admit_browser_engine_build(ModuleComponent::Network, &oversize, None).unwrap_err();
    assert!(error.contains("exceeds 16 KiB"), "{error}");
}

fn ui_identity() -> Value {
    serde_json::from_str(include_str!(
        "module_release/fixtures/interactions-ui-engine-build.json"
    ))
    .unwrap()
}

#[test]
fn ui_units_admit_their_closed_identity() {
    let identity = parse_ui_identity(&ui_identity()).unwrap();
    admit_ui_identity(UiComponent::Interactions, &identity, None).unwrap();
    admit_ui_identity(
        UiComponent::Interactions,
        &identity,
        Some(&identity.source_sha),
    )
    .unwrap();
    assert_eq!(identity.manifest_sha256().len(), 64);
    assert!(admit_ui_identity(UiComponent::CommandKernel, &identity, None).is_err());
    assert!(admit_ui_identity(UiComponent::Interactions, &identity, Some(SOURCE)).is_err());
    for component in [UiComponent::Interactions, UiComponent::CommandKernel] {
        assert_eq!(UiComponent::from_name(component.as_str()), Some(component));
    }
    assert_eq!(UiComponent::CommandKernel.stem(), "ds_command_kernel");
    assert_eq!(UiComponent::from_name("network"), None);
}

#[test]
fn ui_units_refuse_unknown_fields_compilers_and_targets() {
    let mut unknown = ui_identity();
    unknown["extra"] = json!(true);
    assert!(parse_ui_identity(&unknown)
        .unwrap_err()
        .starts_with("module_engine_incompatible:"));
    for (key, value) in [
        ("rustc", json!("rustc different")),
        ("target", json!("x86_64-unknown-linux-gnu")),
        ("profile", json!("debug")),
        ("protocol", json!("ds.ui-wasm/v2")),
        ("config_sha256", json!("d")),
    ] {
        let mut changed = ui_identity();
        changed[key] = value;
        let identity = parse_ui_identity(&changed).unwrap();
        let error = admit_ui_identity(UiComponent::Interactions, &identity, None).unwrap_err();
        assert!(
            error.starts_with("module_engine_incompatible:"),
            "{key}: {error}"
        );
    }
}

#[test]
fn different_contract_metadata_admits_the_same_working_typed_exchange() {
    use crate::table_edit::{decode_request, decode_response, execute, TableEditRequest};
    // Owner ruling and module-releases.md: metadata equality is not the
    // compatibility criterion. Preserve all producer-integrity checks above.
    for version in [1, 2] {
        let mut value = ui_identity();
        value["component"] = serde_json::to_value(UiComponent::CommandKernel).unwrap();
        value["ui_contract"] = json!(version);
        let identity = parse_ui_identity(&value).unwrap();
        admit_ui_identity(UiComponent::CommandKernel, &identity, None).unwrap();
        let request: TableEditRequest = serde_json::from_value(json!({
            "operation":"add_column",
            "input":{"rows":[{}],"column":"n","data_type":"int","default_value":"3.5"}
        }))
        .unwrap();
        let request = decode_request(&serde_json::to_vec(&request).unwrap()).unwrap();
        let reply = execute(&request).unwrap();
        let decoded = decode_response(&request, &serde_json::to_vec(&reply).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap()["result"]["default_value"],
            4
        );
    }
}
