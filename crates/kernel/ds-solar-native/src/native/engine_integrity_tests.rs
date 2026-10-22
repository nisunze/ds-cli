//! Private synthetic selections delegate all calculation bytes to the qualified addon.
use super::tests::project_fixture_city;
use super::*;
use crate::NativeHost;
use ds_command_kernel::{module_install::*, module_release::*};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt};

fn select(
    base: &Path,
    component: ModuleComponent,
    lane: &str,
    binary: &Path,
    info: &Value,
    generation: u64,
    trailer: &[u8],
) -> PathBuf {
    let mut bytes = fs::read(binary).unwrap();
    bytes.extend_from_slice(trailer);
    let sha256 = ds_compute_runtime::digest(&bytes);
    let root = base.join(component.as_str()).join(lane);
    let directory = root.join("versions").join(&sha256);
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(component.executable());
    fs::write(&path, &bytes).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let network = component == ModuleComponent::Network;
    let source = if network {
        info["engineIdentity"]["sourceSha"].as_str().unwrap()
    } else {
        info["source_sha"].as_str().unwrap()
    };
    let version = if network {
        info["engineIdentity"]["coreVersion"].as_str().unwrap()
    } else {
        info["version"].as_str().unwrap()
    };
    let manifest = if network {
        &info["engineIdentity"]["dependencyClosureSha256"]
    } else {
        &info["build_manifest_sha256"]
    };
    let release = validate_module_release(component.as_str(), lane, &json!({
        "schema":"ds.module-release/v1", "component":component, "channel":lane,
        "product_id":format!("rw.datasolutions.module.{}.{lane}",component.as_str()),
        "version":format!("{version}-r{generation}"), "protocol":component.protocol(),
        "released_at":"2026-10-14T10:00:00Z", "status":"active", "assets":[{
            "platform":"linux", "arch":"x86_64", "kind":"module-executable",
            "protocol":component.protocol(),
            "url":format!("https://storage.googleapis.com/synthetic-module-fixtures/modules/{}/{lane}/{version}-r{generation}/{}",component.as_str(), component.executable()),
            "size_bytes":bytes.len(), "sha256":sha256, "source_sha":source,
            "built_at":"2026-10-14T09:00:00Z", "source_revisions":{component.engine_owner():source},
            "engines":[{"engine":component.engine_owner(),
                "release":format!("{}@{version}+{source}",component.engine_owner()),
                "manifest_sha256":manifest}]
        }]
    })).unwrap();
    let installed = InstalledModule {
        schema: INSTALLED_SCHEMA.into(),
        authority: ReleaseAuthorityReceipt {
            gateway_origin: "https://synthetic-module-authority.ue.gateway.dev".into(),
            native_catalog_sha256: "a".repeat(64),
            release_sha256: ds_compute_runtime::digest(&serde_json::to_vec(&release).unwrap()),
        },
        release,
        platform: "linux".into(),
        engine_identity: info.clone(),
    };
    let selected = ModuleSelection {
        schema: SELECTION_SCHEMA.into(),
        component,
        channel: lane.into(),
        generation,
        active: installed,
        previous: None,
    };
    validate_selection(&selected, component, lane, "linux").unwrap();
    let staged = root.join("selection.next");
    fs::write(&staged, serde_json::to_vec(&selected).unwrap()).unwrap();
    fs::rename(staged, root.join("selection.json")).unwrap();
    path
}

fn wrapper(root: &Path, binary: &Path, marker: &Path, change: bool) -> PathBuf {
    let path = root.join("solar-wrapper");
    fs::write(&path, format!(
        "#!/usr/bin/python3\nimport os, subprocess, sys\nresult = subprocess.run([{binary:?}] + sys.argv[1:])\nif sys.argv[1] != 'build-info':\n open({marker:?}, 'w').write(str(result.returncode))\n if {change}:\n  s = os.stat(__file__)\n  data = open(__file__, 'rb').read()\n  open(__file__, 'wb').write(data[:-2] + b'1\\n')\n  os.utime(__file__, ns=(s.st_atime_ns, s.st_mtime_ns))\nsys.exit(result.returncode)\n# stamp:0\n",
        binary=binary.to_str().unwrap(), marker=marker.to_str().unwrap(),
        change=if change { "True" } else { "False" }
    )).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn change_same_metadata(path: &Path) {
    let before = fs::metadata(path).unwrap();
    let mut bytes = fs::read(path).unwrap();
    let last = bytes.len() - 2;
    bytes[last] = b'1';
    fs::write(path, bytes).unwrap();
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap()))
        .unwrap();
}

#[test]
#[ignore = "requires DS_INTEGRITY_SOLAR_BIN qualified exact producer executable"]
fn retained_native_receipt_guards_capture_and_real_batch_portfolio_adoption() {
    assert!(std::env::var_os("DS_SOLAR_ENGINE_BIN").is_none());
    let binary = PathBuf::from(std::env::var_os("DS_INTEGRITY_SOLAR_BIN").unwrap());
    let info: Value = ds_cli_exec::addons::SOLAR
        .capture_pinned(&binary)
        .unwrap()
        .call_json("build-info", &[], std::time::Duration::from_secs(20))
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let host = NativeHost::headless(tmp.path().join("host"))
        .unwrap()
        .with_engine_lane("canary")
        .unwrap();
    let cancel = CancelToken::new();

    // Receipt survives selection -> process capture, including an Err closure.
    for during_capture in [false, true] {
        let root = secure_private_directory(&tmp.path().join(format!("capture-{during_capture}")))
            .unwrap();
        let wrapped = wrapper(&root, &binary, &root.join("task"), false);
        let base = root.join("addons");
        let path = select(
            &base,
            ModuleComponent::Solar,
            "canary",
            &wrapped,
            &info,
            1,
            b"",
        );
        let selected = host.capture_engine_from(Some(&base)).unwrap();
        if !during_capture {
            change_same_metadata(&path);
        }
        let mut called = false;
        let result = CapturedEngine::<()>::capture(selected, |_| {
            called = true;
            change_same_metadata(&path);
            Err(ds_solar_process::ProcessFailure::from(
                "original fixture refusal".to_string(),
            ))
        });
        let error = result.err().unwrap();
        assert!(error.contains("module_"), "{error}");
        assert_eq!(called, during_capture);
    }

    let workspace = crate::workspace::solar_native_prepare_default_workspace(host.clone()).unwrap();
    let workspace_path = resolve_workspace_handle(&host, &workspace.workspace_handle).unwrap();
    let source = secure_private_directory(
        &native_root(&workspace_path).join("runs/eds_project/ds-solar-fixtures/eds_solar/run-test"),
    )
    .unwrap();
    let prepared = vec![
        project_fixture_city("ds-solar-fixtures", "aderm_bere"),
        project_fixture_city("ds-solar-fixtures", "aderm_beinamar"),
    ];
    // Compare exact bytes against the same qualified process owner, whose
    // stamped identity is intentionally different from a linked dev test build.
    let request = ds_solar_contracts::prepared_batch::PreparedBatchRequest {
        schema_version: ds_solar_contracts::prepared_batch::PREPARED_BATCH_REQUEST_SCHEMA.into(),
        project_id: "ds-solar-fixtures".into(),
        root: "eds_project/ds-solar-fixtures/eds_solar".into(),
        prepared: prepared.clone(),
        options: ds_solar_contracts::prepared_batch::PreparedBatchOptions {
            run_id: Some("run-test".into()),
            render_charts: true,
            generate_documents: true,
            ..Default::default()
        },
        concurrency: Some(1),
    };
    let expected = ds_solar_process::BatchEngine::capture(&binary, tmp.path(), 1, &cancel)
        .unwrap()
        .calculate(&request, &cancel, None)
        .unwrap();
    engine_batch::commit(&source, &expected, &cancel, |_, _| {}).unwrap();
    write_json(&source.join("batch.json"), &expected.output.batch).unwrap();
    let source_manifest = fs::read(source.join("batch.json")).unwrap();

    for portfolio in [false, true] {
        for change in [false, true] {
            let root =
                secure_private_directory(&tmp.path().join(format!("task-{portfolio}-{change}")))
                    .unwrap();
            let marker = root.join("completed");
            let wrapped = wrapper(&root, &binary, &marker, change);
            let base = root.join("addons");
            let path = select(
                &base,
                ModuleComponent::Solar,
                "canary",
                &wrapped,
                &info,
                1,
                b"",
            );
            let before = fs::metadata(&path).unwrap();
            let selected = host.capture_engine_from(Some(&base)).unwrap();
            let output = if portfolio {
                native_root(&workspace_path)
                    .join("portfolio-runs/eds_project/ds-solar-fixtures/eds_solar")
                    .join(if change { "changed" } else { "retained" })
            } else {
                root.join("output")
            };
            let error = if portfolio {
                let engine = CapturedEngine::capture(selected, |path| {
                    ds_solar_process::PortfolioEngine::capture(path, &root, 1, &cancel)
                })
                .unwrap();
                // An invalid current pointer cannot invalidate/reselect accepted work.
                fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
                calculate_portfolio_with_cancel(
                    host.clone(),
                    PortfolioCalculateCommand {
                        workspace_handle: workspace.workspace_handle.clone(),
                        project_id: "ds-solar-fixtures".into(),
                        root: "eds_project/ds-solar-fixtures/eds_solar".into(),
                        source_run_id: "run-test".into(),
                        run_id: if change { "changed" } else { "retained" }.into(),
                        portfolio_id: "integrity".into(),
                        portfolio_name: "Integrity fixture".into(),
                        membership_revision: format!("sha256:{}", "a".repeat(64)),
                        city_ids: vec!["aderm_bere".into(), "aderm_beinamar".into()],
                        currency: None,
                        project_years: None,
                        discount_rate: None,
                        representative_strategy: "first".into(),
                        representative_city_id: None,
                        round_robin_start_index: None,
                        language: "fr".into(),
                        report_intents: vec!["apd".into()],
                    },
                    cancel.clone(),
                    engine,
                )
                .err()
            } else {
                let engine = CapturedEngine::capture(selected, |path| {
                    ds_solar_process::BatchEngine::capture(path, &root, 1, &cancel)
                })
                .unwrap();
                fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
                execute_run(
                    &host,
                    "run-test",
                    "ds-solar-fixtures",
                    "eds_project/ds-solar-fixtures/eds_solar",
                    &output,
                    prepared.clone(),
                    &BTreeMap::new(),
                    true,
                    true,
                    vec![],
                    Concurrency::serial(),
                    engine,
                    &cancel,
                )
                .err()
            };
            assert_eq!(
                fs::read_to_string(&marker).unwrap_or_else(|failure| panic!(
                    "missing task completion: portfolio={portfolio}, change={change}, host={error:?}, file={failure}"
                )),
                "0",
                "real owner task must succeed"
            );
            let after = fs::metadata(&path).unwrap();
            assert_eq!(before.len(), after.len());
            assert_eq!(before.modified().unwrap(), after.modified().unwrap());
            if change {
                assert!(
                    error.as_ref().is_some_and(|e| e.contains("module_")),
                    "{error:?}"
                );
                assert!(
                    !output.exists(),
                    "no output may be adopted after failed postcheck"
                );
                assert!(!output.join("sync-intent.json").exists());
            } else {
                assert!(error.is_none(), "{error:?}");
                assert!(output.join("batch.json").is_file());
                if !portfolio {
                    let actual: ds_solar_contracts::SolarArtifactBatch =
                        serde_json::from_slice(&fs::read(output.join("batch.json")).unwrap())
                            .unwrap();
                    assert_eq!(actual, expected.output.batch);
                    for artifact in &actual.artifacts {
                        if artifact.name != "batch.json" {
                            assert_eq!(
                                fs::read(output.join(&artifact.name)).unwrap(),
                                fs::read(source.join(&artifact.name)).unwrap(),
                                "{}",
                                artifact.name
                            );
                        }
                    }
                }
            }
            assert_eq!(
                fs::read(source.join("batch.json")).unwrap(),
                source_manifest
            );
        }
    }
}
