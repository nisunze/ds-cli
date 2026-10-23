//! Synthetic local receipts grant no installation or publication authority.
#![cfg(target_os = "linux")]
use super::*;
use ds_cli_contract::{Context, Format, Inputs, Output};
use ds_command_kernel::{module_install::*, module_release::*};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}
fn select(base: &Path, lane: &str, label: &str, log: &Path, switch: Option<&Path>) -> PathBuf {
    let root = base.join("reporter").join(lane);
    let source = "a".repeat(40);
    let info = json!({
        "schema":"ds.engine-build/v1","service":"ds-network-reporter","binary":"ds-report",
        "profile":"release","target":"x86_64-unknown-linux-gnu","cargo_lock_sha256":"c".repeat(64),"rustc":"rustc synthetic-fixture",
        "package_version":"0.1.0","source_sha":source,"build_manifest_sha256":"f".repeat(64),"features":[],
        "engine_version":format!("ds-network-reporter@0.1.0+{source}"),
        "schemas":["ds.engine-build/v1","ds-network-reporter.task-result/v1"],"fixture":label
    });
    let schemas = json!({"service":"ds-network-reporter","tasks":[{
        "name":"export_lv_standard","subcommand":"export-lv-standard",
        "request_schema":{"required":["fixture"]},"effect":"local_file_write","network":"none"
    }]});
    let switch = switch
        .map(|next| {
            format!(
                "if test -f {}; then /bin/mv {} {}; fi",
                quote(next),
                quote(next),
                quote(&root.join("selection.json"))
            )
        })
        .unwrap_or_default();
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' '{label}:'\"$1\" >> {}\ncase \"$1\" in\nbuild-info) printf '%s\\n' '{}' ;;\ntask-schemas) {switch}\nprintf '%s\\n' '{}' ;;\nexport-lv-standard) shift; while test \"$#\" -gt 0; do case \"$1\" in --result) result=\"$2\";; esac; shift 2; done; printf '%s\\n' '{{\"status\":\"completed\",\"fixture\":\"{label}\"}}' > \"$result\" ;;\n*) exit 99;;\nesac\n",
        quote(log),
        info,
        schemas
    );
    let bytes = script.as_bytes();
    let sha = digest(bytes);
    let directory = root.join("versions").join(&sha);
    fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("reporter");
    fs::write(&executable, bytes).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let release=validate_module_release("reporter",lane,&json!({
        "schema":"ds.module-release/v1","component":"reporter","channel":lane,
        "product_id":format!("rw.datasolutions.module.reporter{}",if lane=="canary" {".canary"} else {""}),
        "version":"0.1.0-r1","protocol":ModuleComponent::Reporter.protocol(),
        "released_at":"2026-10-17T10:00:00Z","status":"active","assets":[{
            "platform":"linux","arch":"x86_64","kind":"module-executable","protocol":ModuleComponent::Reporter.protocol(),
            "url":format!("https://storage.googleapis.com/synthetic-module-fixtures/modules/reporter/{lane}/0.1.0-r1/reporter"),
            "size_bytes":bytes.len(),"sha256":sha,"source_sha":source,
            "built_at":"2026-10-17T09:00:00Z","source_revisions":{"ds-network-reporter":source},
            "engines":[{"engine":"ds-network-reporter","release":info["engine_version"],"manifest_sha256":"f".repeat(64)}]
        }]
    })).unwrap();
    let active = InstalledModule {
        schema: INSTALLED_SCHEMA.into(),
        authority: ReleaseAuthorityReceipt {
            gateway_origin: "https://synthetic-module-authority.ue.gateway.dev".into(),
            native_catalog_sha256: "b".repeat(64),
            release_sha256: digest(&serde_json::to_vec(&release).unwrap()),
        },
        release,
        platform: "linux".into(),
        engine_identity: info,
    };
    let selection = ModuleSelection {
        schema: SELECTION_SCHEMA.into(),
        component: ModuleComponent::Reporter,
        channel: lane.into(),
        generation: 1,
        active,
        previous: None,
    };
    validate_selection(&selection, ModuleComponent::Reporter, lane, "linux").unwrap();
    fs::write(
        root.join("selection.json"),
        serde_json::to_vec(&selection).unwrap(),
    )
    .unwrap();
    executable
}
fn parse(command: &ds_cli_contract::spec::Command, args: &[&str]) -> Inputs {
    ds_cli_contract::args::parse(
        command,
        &args.iter().map(|a| (*a).into()).collect::<Vec<_>>(),
    )
    .unwrap()
}
#[test]
fn discovery_keeps_local_lane_selection_available_without_engine_probes() {
    for command in [
        &engine::COMMAND,
        &tasks::COMMAND,
        &export::COMMAND,
        &bundle::COMMAND,
        &voltage_drop_combined::COMMAND,
        &mv_frontmatter::COMMAND,
        &spatial_workbook::COMMAND,
    ] {
        assert!(matches!(
            (command.availability)(),
            ds_cli_contract::spec::Availability::Available
        ));
        assert!(matches!(
            command.authority,
            ds_cli_contract::spec::Authority::None
        ));
    }
    assert_eq!(parse(&engine::COMMAND, &[]).value("lane"), Some("stable"));
    assert_eq!(
        parse(&engine::COMMAND, &["--lane", "canary"]).value("lane"),
        Some("canary")
    );
}
#[test]
fn managed_reporter_consumers_are_isolated_from_accounts_and_ambient_engines() {
    let scratch = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "engine_selection_tests::isolated_managed_reporter_consumers",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("DS_REPORT_SELECTION_TEST_ROOT", scratch.path())
        .env("XDG_STATE_HOME", scratch.path().join("no-account"))
        .env("PATH", scratch.path())
        .env_remove("DS_REPORT_BIN")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[ignore = "isolated subprocess with private managed store and PATH"]
fn isolated_managed_reporter_consumers() {
    let root =
        PathBuf::from(std::env::var_os("DS_REPORT_SELECTION_TEST_ROOT").expect("isolated fixture"));
    let base = root.join("addons");
    let log = root.join("spawn.log");
    let fallback = root.join("reporter");
    fs::write(
        &fallback,
        format!(
            "#!/bin/sh\nprintf 'fallback\\n' >> {}\nexit 99\n",
            quote(&log)
        ),
    )
    .unwrap();
    fs::set_permissions(&fallback, fs::Permissions::from_mode(0o700)).unwrap();
    let stable_inputs = parse(&engine::COMMAND, &[]);
    let canary_inputs = parse(&engine::COMMAND, &["--lane", "canary"]);
    let stable = select(&base, "stable", "stable", &log, None);
    assert_eq!(
        capture_engine_at(&canary_inputs, &base)
            .err()
            .unwrap()
            .code(),
        "reporter_engine_missing"
    );
    assert!(
        !log.exists(),
        "missing selection must not spawn or try PATH"
    );
    let lane = base.join("reporter/canary");
    fs::create_dir_all(&lane).unwrap();
    let selection = lane.join("selection.json");
    fs::write(&selection, b"{torn").unwrap();
    assert_eq!(
        capture_engine_at(&canary_inputs, &base)
            .err()
            .unwrap()
            .code(),
        "module_receipt_invalid"
    );
    assert!(
        !log.exists(),
        "corrupt selection must not spawn or try PATH"
    );
    let inspected =
        engine::run_captured(capture_engine_at(&stable_inputs, &base).unwrap()).unwrap();
    assert_eq!(inspected["path"], stable.to_str().unwrap());
    assert_eq!(inspected["identity"]["fixture"], "stable");
    let switch = lane.join("selection.next");
    let first = select(&base, "canary", "first", &log, Some(&switch));
    let first_selection = fs::read(&selection).unwrap();
    let second = select(&base, "canary", "second", &log, None);
    fs::write(&switch, fs::read(&selection).unwrap()).unwrap();
    fs::write(&selection, first_selection).unwrap();
    assert_ne!(first, second);
    let request = root.join("request.json");
    fs::write(&request, b"{}").unwrap();
    let result = root.join("result.json");
    let inputs = parse(
        &export::COMMAND,
        &[
            "--task",
            "lv-standard",
            "--request",
            request.to_str().unwrap(),
            "--result",
            result.to_str().unwrap(),
            "--lane",
            "canary",
        ],
    );
    let captures = std::cell::Cell::new(0);
    let completed = export::run_with_capture(&inputs, || {
        captures.set(captures.get() + 1);
        capture_engine_at(&inputs, &base)
    })
    .unwrap();
    assert_eq!(captures.get(), 1);
    assert_eq!(completed["fixture"], "first");
    assert_eq!(
        capture_engine_at(&canary_inputs, &base)
            .unwrap()
            .executable(),
        second
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "stable:build-info\nfirst:task-schemas\nfirst:export-lv-standard\n"
    );
    assert_eq!(fs::read(&request).unwrap(), b"{}");
    assert!(result.is_file(), "caller-owned result is retained");
    let held = capture_engine_at(&canary_inputs, &base).unwrap();
    fs::write(&selection, b"{}").unwrap();
    assert_eq!(
        capture_engine_at(&canary_inputs, &base)
            .err()
            .unwrap()
            .code(),
        "module_receipt_invalid"
    );
    let held_result = root.join("held-result.json");
    let held_inputs = parse(
        &export::COMMAND,
        &[
            "--task",
            "lv-standard",
            "--request",
            request.to_str().unwrap(),
            "--result",
            held_result.to_str().unwrap(),
            "--lane",
            "canary",
        ],
    );
    let held_completion = export::run_captured(&held_inputs, &held).unwrap();
    assert_eq!(held_completion["fixture"], "second");
    assert!(held_result.is_file());
    let inspected = engine::run_captured(held).unwrap();
    assert_eq!(inspected["path"], second.to_str().unwrap());
    assert_eq!(inspected["identity"]["fixture"], "second");
    let before = fs::read(&log).unwrap();
    assert_eq!(
        capture_engine_at(&canary_inputs, &base)
            .err()
            .unwrap()
            .code(),
        "module_receipt_invalid"
    );
    assert_eq!(fs::read(&log).unwrap(), before);
    assert!(
        !root.join("no-account").exists(),
        "local consumers must not create account state"
    );
    let context = Context {
        confirmed: false,
        output: Output::resolve(Format::Json, false, true),
    };
    for (command, run) in [
        (
            &bundle::COMMAND,
            bundle::run as fn(&Inputs, &Context) -> Result<Value, ds_cli_contract::Failure>,
        ),
        (&voltage_drop_combined::COMMAND, voltage_drop_combined::run),
        (&mv_frontmatter::COMMAND, mv_frontmatter::run),
    ] {
        let inputs = parse(
            command,
            &[
                "--request",
                root.join("missing.json").to_str().unwrap(),
                "--lane",
                "canary",
            ],
        );
        assert_eq!(
            run(&inputs, &context).err().unwrap().code(),
            "request_not_found"
        );
    }
    let invalid = parse(
        &export::COMMAND,
        &[
            "--task",
            "lv-standard",
            "--request",
            root.join("missing.json").to_str().unwrap(),
        ],
    );
    assert_eq!(
        export::run_with_capture(
            &invalid,
            || -> Result<crate::Reporter, ds_cli_contract::Failure> {
                panic!("invalid input must precede capture")
            },
        )
        .err()
        .unwrap()
        .code(),
        "request_not_found"
    );
}
