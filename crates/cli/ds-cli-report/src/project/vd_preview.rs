//! Headless fixed A4 preview: admit retained native source, join the common
//! file room, or render that exact saved result. No transport exists here.
use super::{export, hold};
use ds_cli_contract::spec::Refusal;
use ds_cli_contract::{Failure, Inputs};
use ds_command_kernel::report_export::{self, InputReceipt, held::SavedAnalysis};
use ds_command_kernel::report_formats::DesignOutputSelection;
use ds_project_data::preview_room::Room;
use ds_report_host::{HostFailure, ReportEngine, preview_artifact, vd_preview};
use serde_json::{Value, json};
use std::{io::Read, path::Path};

pub(super) const MISSING: Refusal = Refusal {
    code: "lv_analysis_missing",
    when: "the local hold has no exact saved voltage-drop analysis for the transformer; project-run saves it on the server, not in the hold",
    remedy: "hold it first: run report project export --project <id> --transformer <name> --selection <voltage_drop_pdf selection> --dry-run --out-dir <dir> (publishes nothing), then repeat the preview; or pass a captured --held-preview",
};
pub(super) const STALE: Refusal = Refusal {
    code: "lv_analysis_stale",
    when: "held saved analysis, room revision/content or native source scope disagree",
    remedy: "explicitly refresh the saved room and analysis together; preserve retained files",
};
fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(export::INPUTS_INVALID.code, message).remedy(export::INPUTS_INVALID.remedy)
}
fn host(failure: HostFailure) -> Failure {
    export::host_failure(failure)
}

pub(super) fn preflight(inputs: &Inputs) -> Result<preview_artifact::Retention, Failure> {
    if inputs.repeated("transformer").len() != 1
        || inputs.switch("force")
        || inputs.switch("seed")
        || inputs.value("preview-layout").is_some()
        || !inputs.repeated("print-layout").is_empty()
        || inputs.value("context-vectors").is_some()
        || !inputs.repeated("pin-room").is_empty()
        || inputs.value("admin-bounds").is_some()
    {
        return Err(invalid(
            "--preview-voltage-drop requires one exact held transformer; delivery, acquisition and other preview inputs are refused",
        ));
    }
    report_export::reportable_transformer(&inputs.repeated("transformer")[0]).map_err(invalid)?;
    inputs
        .value("preview-retention")
        .map(|value| serde_json::from_value(json!(value)).map_err(|e| invalid(e.to_string())))
        .transpose()
        .map(|v| v.unwrap_or_default())
}
pub(super) fn run(inputs: &Inputs) -> Result<Value, Failure> {
    let retention = preflight(inputs)?;
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let transformer = &inputs.repeated("transformer")[0];
    // This existing local identity observes the packaged profile and native
    // account. It cannot borrow Desktop state or bypass profile refusals.
    let identity = ds_cli_auth::headless_identity_for_named_project(lane)?;
    let hold = hold::Hold::open(&identity, project)?;
    let captured = hold.inputs("intentional retained preview; no service request was made")?;
    let saved_room = hold.room(transformer).map_err(invalid)?.ok_or_else(|| {
        Failure::unavailable(
            hold::ROOM_NOT_HELD.code,
            "The exact saved transformer room is not held.",
        )
        .remedy(hold::ROOM_NOT_HELD.remedy)
    })?;
    let room = Room {
        root: ds_layer_store::default_root().map_err(invalid)?,
        scope: ds_command_kernel::project_dataset_cache::Scope {
            principal: identity.uid().into(),
            project: project.into(),
        },
        lane: identity.lane().into(),
        credential_audience_sha256: identity.credential_audience_sha256().into(),
    };
    let selection = inputs
        .value("selection")
        .map(|path| {
            let file = ds_layer_store::private::open_options()
                .read(true)
                .open(path)
                .map_err(|e| invalid(e.to_string()))?;
            let meta = file.metadata().map_err(|e| invalid(e.to_string()))?;
            if !meta.is_file() || meta.len() > 1024 * 1024 {
                return Err(invalid("A4 selection requires a bounded regular file"));
            }
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| invalid(e.to_string()))?;
            if bytes.len() > 1024 * 1024 {
                return Err(invalid("A4 selection exceeds its byte bound"));
            }
            serde_json::from_slice::<DesignOutputSelection>(&bytes)
                .map_err(|e| invalid(e.to_string()))
        })
        .transpose()?;
    let guard = || {
        let current = ds_cli_auth::headless_identity_for_named_project(lane)?;
        if current != identity {
            return Err(Failure::conflict(
                super::AUTH_CONTEXT_MISMATCH.code,
                "Native preview account changed.",
            ));
        }
        Ok(())
    };
    execute(
        &export::CliEngine::capture(identity.lane()).map_err(export::host_failure)?,
        &room,
        &captured,
        &saved_room,
        selection,
        retention,
        Path::new(inputs.require("out-dir")?),
        &guard,
    )
}

fn execute(
    engine: &dyn ReportEngine,
    room: &Room,
    captured: &hold::Inputs,
    saved_room: &ds_project_data::room_hold::Room,
    selection: Option<DesignOutputSelection>,
    retention: preview_artifact::Retention,
    scratch_parent: &Path,
    guard: &dyn Fn() -> Result<(), Failure>,
) -> Result<Value, Failure> {
    guard()?;
    let transformer = &saved_room.transformer;
    if !captured
        .rows
        .iter()
        .any(|row| row.name == *transformer && row.is_transformer() && row.is_active())
    {
        return Err(invalid(
            "The retained lifecycle does not admit this active saved transformer.",
        ));
    }
    let receipt = InputReceipt::from_config(&captured.configuration).map_err(invalid)?;
    let version = saved_room
        .version
        .filter(|v| *v > 0)
        .ok_or_else(|| invalid("The retained room has no saved revision."))?;
    let content = report_export::jcs::layers_content_digest(&saved_room.layers).map_err(invalid)?;
    if saved_room.content_digest.as_deref() != Some(content.as_str()) {
        return Err(Failure::conflict(
            STALE.code,
            "Held layers differ from their saved content pin.",
        )
        .remedy(STALE.remedy));
    }
    let expected = report_export::held::Scope {
        project: room.scope.project.clone(),
        principal: room.scope.principal.clone(),
        lane: room.lane.clone(),
        credential_audience_sha256: room.credential_audience_sha256.clone(),
        sheets_sha256: receipt.sheets_sha256.clone(),
    };
    let saved: &SavedAnalysis = captured.saved_analyses.get(transformer).ok_or_else(|| {
        Failure::unavailable(MISSING.code, "The exact saved analysis is not held.")
            .remedy(MISSING.remedy)
    })?;
    saved
        .validate(&expected, transformer, version, &content)
        .map_err(|e| Failure::conflict(STALE.code, e).remedy(STALE.remedy))?;
    let selection = match selection {
        Some(selection) => selection,
        None => {
            let mut selected = ds_command_kernel::report_formats::normalize_output_selection(
                None,
                Some(vec!["voltage_drop_pdf".into()]),
                None,
            )
            .map_err(invalid)?;
            selected.voltage_drop_report =
                ds_command_kernel::report_formats::project_voltage_drop_report_format(
                    &receipt.sheets().map_err(invalid)?,
                )
                .map_err(invalid)?;
            selected
        }
    };
    if selection.tokens().map_err(invalid)? != ["voltage_drop_pdf"] {
        return Err(invalid(
            "A4 preview selection must name only voltage_drop_pdf.",
        ));
    }
    let engine_before =
        report_export::engine_identity(&engine.build_info().map_err(host)?).map_err(invalid)?;
    let pins = vd_preview::Inputs {
        transformer: transformer.clone(),
        transformer_revision: version,
        analysis_sha256: saved.sha256.clone(),
        content_digest: content,
        settings_sha256: report_export::sha256_hex(
            &serde_json::to_vec(&json!({"receipt":receipt,"selection":selection}))
                .map_err(|e| invalid(e.to_string()))?,
        ),
        engine_version: engine_before.engine_version.clone(),
        engine_build_manifest_sha256: engine_before.build_manifest_sha256.clone(),
    };
    let mut lease = vd_preview::begin(room, pins.clone(), retention).map_err(host)?;
    if let Some(metadata) = lease.retained() {
        let artifact: preview_artifact::Artifact =
            serde_json::from_value(metadata["previewArtifact"].clone())
                .map_err(|e| invalid(e.to_string()))?;
        vd_preview::verify(room, &pins, &artifact).map_err(host)?;
        guard()?;
        return answer(room, &pins, &artifact, &lease, true);
    }
    // All source admission precedes either scratch creation or a renderer.
    // out-dir is this mode's disk scratch parent, never a submission output.
    std::fs::create_dir_all(scratch_parent).map_err(|e| invalid(e.to_string()))?;
    let stage = tempfile::Builder::new()
        .prefix(".a4-preview-")
        .tempdir_in(scratch_parent)
        .map_err(|e| invalid(e.to_string()))?;
    let reference = if receipt.requires_admin_bounds() {
        Some(
            ds_report_host::verify_admin_bounds_asset(
                &ds_report_host::installed_admin_bounds_path(
                    &ds_report_host::shared_root().map_err(invalid)?,
                    &receipt.reference_semantic_sha256,
                ),
                &receipt.reference_semantic_sha256,
            )
            .map_err(host)?,
        )
    } else {
        None
    };
    let settings = ds_report_host::RunSettings {
        project_id: &room.scope.project,
        receipt: &receipt,
        admin_bounds: reference.as_ref(),
        staging_root: &stage.path().join("staging"),
        out_root: &stage.path().join("out"),
    };
    let input = ds_report_host::TransformerReportInputs {
        transformer: transformer.clone(),
        server_version: version,
        layers: saved_room.layers.clone(),
        content_digest: saved_room.content_digest.clone(),
        selection: Some(selection.clone()),
        transformer_natures: None,
        print_context: None,
        sheet: None,
        survey: None,
    };
    let rendered = export::render_saved_a4_preview(
        engine,
        &settings,
        &input,
        saved,
        selection.voltage_drop_report_format().token(),
    )
    .map_err(host)?;
    let pdf = rendered
        .artifacts
        .iter()
        .find(|a| a.format == "voltage_drop_pdf")
        .ok_or_else(|| {
            Failure::failed(
                export::RESULT_INVALID.code,
                "Saved result did not produce an A4 PDF.",
            )
            .detail(json!({"failed_formats":rendered.failed}))
        })?;
    if rendered.engine != engine_before {
        return Err(Failure::failed(
            "report_engine_identity_changed",
            "Reporter changed during retained A4 preview.",
        ));
    }
    guard()?;
    let mut file = std::fs::File::open(rendered.artifact_dir.join(&pdf.filename))
        .map_err(|e| invalid(e.to_string()))?;
    let artifact = vd_preview::materialize(
        room,
        &pins,
        &mut file,
        pdf.size_bytes,
        &pdf.sha256,
        retention,
    )
    .map_err(host)?;
    lease
        .commit(json!({"previewArtifact":artifact}), &artifact)
        .map_err(host)?;
    stage.close().map_err(|e| invalid(e.to_string()))?;
    guard()?;
    answer(room, &pins, &artifact, &lease, false)
}
fn answer(
    room: &Room,
    pins: &vd_preview::Inputs,
    artifact: &preview_artifact::Artifact,
    lease: &vd_preview::Lease,
    reused: bool,
) -> Result<Value, Failure> {
    Ok(
        json!({"project":room.scope.project,"lane":room.lane,"preview":{"format":"pdf","paper":"A4","saved_analysis_sha256":pins.analysis_sha256},
        "results":[{"transformer":pins.transformer,"server_version":pins.transformer_revision,"page":artifact}],
        "preview_cache":lease.receipt(reused).map_err(host)?,"publication":{"published_nothing":true},"source":"held","remote_reads":0}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    struct Engine(AtomicUsize);
    impl ReportEngine for Engine {
        fn build_info(&self) -> Result<Value, HostFailure> {
            Ok(
                json!({"schema":"ds.engine-build/v1","service":"ds-network-reporter","binary":"ds-report",
            "package_version":"0.1.0","source_sha":"a".repeat(40),"engine_version":format!("ds-network-reporter@0.1.0+{}","a".repeat(40)),
            "cargo_lock_sha256":"d".repeat(64),"features":[],"rustc":"rustc 1.97.0","target":"x86_64-unknown-linux-gnu",
            "profile":"release","schemas":["ds-network-reporter.task-result/v1"],"build_manifest_sha256":"f".repeat(64)}),
            )
        }
        fn export_transformer_report(
            &self,
            path: &Path,
            result: &Path,
        ) -> Result<ds_report_host::EngineExit, HostFailure> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let request: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(
                request["formats"],
                json!(["voltage_drop", "voltage_drop_pdf"])
            );
            assert!(request.get("transformer_natures").is_none());
            let source =
                std::fs::read(request["voltage_drop_result"]["path"].as_str().unwrap()).unwrap();
            assert_eq!(
                report_export::sha256_hex(&source),
                request["voltage_drop_result"]["sha256"]
            );
            let name = request["transformer"].as_str().unwrap();
            let out = Path::new(request["out_dir"].as_str().unwrap());
            let mut artifacts = vec![];
            for (format, body) in [
                ("voltage_drop", source.as_slice()),
                (
                    "voltage_drop_pdf",
                    b"%PDF-1.7 pinned saved result".as_slice(),
                ),
            ] {
                let filename =
                    ds_command_kernel::report_formats::report_filename(name, format).unwrap();
                let path = out.join(&filename);
                std::fs::write(&path, body).unwrap();
                let mut artifact = json!({"format":format,"filename":filename,"path":path,"bytes":body.len(),"sha256":report_export::sha256_hex(body)});
                if format == "voltage_drop_pdf" {
                    artifact["paper_size"] = json!("A4");
                }
                artifacts.push(artifact);
            }
            std::fs::write(result,serde_json::to_vec(&json!({"task":"export_transformer_report","status":"completed","transformer":name,
                "country":"Benin","layers_decoded":1,"features_decoded":1,"admin_bounds":{"configured":false,"villages_loaded":0,"features_matched":0,"features_requested":0},
                "geometry_changed":false,"formats_requested":request["formats"],"artifacts":artifacts,"blockers":[],"elapsed_ms":1})).unwrap()).unwrap();
            Ok(ds_report_host::EngineExit {
                succeeded: true,
                summary: String::new(),
            })
        }
    }
    fn fixture(root: &Path) -> (Room, hold::Inputs, ds_project_data::room_hold::Room) {
        let room = Room {
            root: root.into(),
            scope: ds_command_kernel::project_dataset_cache::Scope {
                principal: "owner".into(),
                project: "project".into(),
            },
            lane: "canary".into(),
            credential_audience_sha256: "b".repeat(64),
        };
        let layers = BTreeMap::from([(
            "lv_poles".into(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[2.0,6.0]}}]}),
        )]);
        let content = report_export::jcs::layers_content_digest(&layers).unwrap();
        let held = ds_project_data::room_hold::Room {
            transformer: "tx_a".into(),
            version: Some(7),
            content_digest: Some(content.clone()),
            layers,
        };
        let sheets =
            json!({"project_settings":[{"parameter":"design_export_format","value":["xlsx"]}]})
                .to_string();
        let receipt = InputReceipt {
            schema: 1,
            country: "Benin".into(),
            sheets_sha256: report_export::sha256_hex(sheets.as_bytes()),
            sheets_json: sheets,
            reference_semantic_sha256: "c".repeat(64),
            local_print_recipe: None,
        };
        let doc = json!({"summary":{"verdict":"complies"},"saved_result":true}).to_string();
        let saved = SavedAnalysis {
            scope: report_export::held::Scope {
                project: "project".into(),
                principal: "owner".into(),
                lane: "canary".into(),
                credential_audience_sha256: "b".repeat(64),
                sheets_sha256: receipt.sheets_sha256.clone(),
            },
            transformer: "tx_a".into(),
            version: 7,
            content_digest: content,
            sha256: report_export::sha256_hex(doc.as_bytes()),
            document: doc,
        };
        let captured = hold::Inputs {
            project_crs: None,
            printing_style_capture: None,
            transformer_natures: None,
            saved_analyses: BTreeMap::from([("tx_a".into(), saved)]),
            rows: vec![hold::Row {
                name: "tx_a".into(),
                kind: "transformer".into(),
                state: "active".into(),
                retired: false,
                reason: None,
            }],
            configuration: json!({"network_reporter_input_receipt":receipt}),
            setups: vec![],
            read_at: "captured".into(),
        };
        (room, captured, held)
    }
    #[test]
    fn headless_cold_concurrent_warm_and_offline_consumers_share_one_saved_renderer_and_common_file()
     {
        let dir = tempfile::tempdir().unwrap();
        let (room, captured, held) = fixture(dir.path());
        let engine = Arc::new(Engine(AtomicUsize::new(0)));
        let barrier = Arc::new(Barrier::new(8));
        std::thread::scope(|threads| {
            for _ in 0..8 {
                let (room, captured, held, engine, barrier) =
                    (&room, &captured, &held, engine.clone(), barrier.clone());
                threads.spawn(move || {
                    barrier.wait();
                    execute(
                        engine.as_ref(),
                        room,
                        captured,
                        held,
                        None,
                        preview_artifact::Retention::Hours24,
                        &room.root.join("scratch"),
                        &|| Ok(()),
                    )
                    .unwrap();
                });
            }
        });
        assert_eq!(engine.0.load(Ordering::SeqCst), 1);
        // A later consumer uses only admitted local source and common files;
        // this owner has no network/provider port at all.
        let warm = execute(
            engine.as_ref(),
            &room,
            &captured,
            &held,
            None,
            preview_artifact::Retention::Hours24,
            &dir.path().join("unused-scratch"),
            &|| Ok(()),
        )
        .unwrap();
        assert_eq!(engine.0.load(Ordering::SeqCst), 1);
        assert_eq!(warm["remote_reads"], 0);
        assert_eq!(warm["preview_cache"]["reused"], true);
        assert_eq!(
            warm["results"][0]["page"]["schema"],
            preview_artifact::SCHEMA
        );
        assert!(!dir.path().join("unused-scratch").exists());
        let artifact: preview_artifact::Artifact =
            serde_json::from_value(warm["results"][0]["page"].clone()).unwrap();
        assert_eq!(
            preview_artifact::read(&room, &artifact).unwrap(),
            b"%PDF-1.7 pinned saved result"
        );
        assert_eq!(
            std::fs::read_dir(dir.path().join("scratch"))
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn missing_changed_or_foreign_saved_inputs_refuse_before_renderer_cache_or_scratch() {
        let dir = tempfile::tempdir().unwrap();
        let (room, captured, held) = fixture(dir.path());
        let engine = Engine(AtomicUsize::new(0));
        let mut missing = captured.clone();
        missing.saved_analyses.clear();
        assert_eq!(
            execute(
                &engine,
                &room,
                &missing,
                &held,
                None,
                preview_artifact::Retention::Hours24,
                &dir.path().join("scratch"),
                &|| Ok(())
            )
            .unwrap_err()
            .code(),
            MISSING.code
        );
        let mut changed = held.clone();
        changed.version = Some(8);
        assert_eq!(
            execute(
                &engine,
                &room,
                &captured,
                &changed,
                None,
                preview_artifact::Retention::Hours24,
                &dir.path().join("scratch"),
                &|| Ok(())
            )
            .unwrap_err()
            .code(),
            STALE.code
        );
        for field in 0..4 {
            let mut wrong = room.clone();
            match field {
                0 => wrong.scope.principal = "other".into(),
                1 => wrong.scope.project = "other".into(),
                2 => wrong.lane = "stable".into(),
                _ => wrong.credential_audience_sha256 = "d".repeat(64),
            };
            assert_eq!(
                execute(
                    &engine,
                    &wrong,
                    &captured,
                    &held,
                    None,
                    preview_artifact::Retention::Hours24,
                    &dir.path().join("scratch"),
                    &|| Ok(())
                )
                .unwrap_err()
                .code(),
                STALE.code
            );
        }
        assert_eq!(engine.0.load(Ordering::SeqCst), 0);
        assert!(!dir.path().join("scratch").exists());
    }
    #[test]
    fn mode_refuses_delivery_or_acquisition_before_native_identity_or_file_reads() {
        let parse = |tokens: &[&str]| {
            ds_cli_contract::args::parse(
                &export::COMMAND,
                &tokens.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            )
            .unwrap()
        };
        let base = [
            "--project",
            "project",
            "--transformer",
            "tx_a",
            "--out-dir",
            "unread-scratch",
            "--preview-voltage-drop",
        ];
        assert!(preflight(&parse(&base)).is_ok());
        for flag in ["--force", "--seed"] {
            let mut tokens = base.to_vec();
            tokens.push(flag);
            assert!(preflight(&parse(&tokens)).is_err());
        }
        let mut tokens = base.to_vec();
        tokens.extend(["--preview-layout", "unread-file"]);
        assert!(preflight(&parse(&tokens)).is_err());
    }
}
