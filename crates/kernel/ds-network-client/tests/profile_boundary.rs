use ds_network_client::{CapturedNetworkClient, ProfileSession};
use ds_network_contracts::profile::*;
use serde_json::{json, Value};
use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
fn required(key: &str) -> PathBuf {
    PathBuf::from(env::var_os(key).unwrap_or_else(|| panic!("{key} must name an acceptance input")))
}
fn state(session: &mut ProfileSession) -> Value {
    serde_json::from_slice(
        &session
            .request(NativeProfileOperationV1::State, &|| false)
            .unwrap()
            .bytes,
    )
    .unwrap()
}
fn isolated(engine: &std::path::Path, case: &str) {
    let output = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "isolated_profile_case",
            "--ignored",
            "--nocapture",
        ])
        .env("DS_NETWORK_ENGINE_BIN", engine)
        .env("DS_PROFILE_TEST_CASE", case)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{case}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[ignore = "requires explicit built native Profile owner and emitted owning fixture"]
fn acceptance_profile_real_owner_retains_history_exact_artifacts_and_explicit_restart() {
    isolated(&required("DS_PROFILE_REAL_ENGINE"), "real");
}
#[test]
#[ignore = "requires explicit built Profile authored-facts owner and exact owning replay fixture"]
fn acceptance_profile_real_owner_reconciles_staged_journal_and_reads_exact_authored_facts() {
    isolated(&required("DS_PROFILE_REAL_ENGINE"), "authored");
}
#[test]
#[ignore = "requires explicit previous ProfileSession engine without authored-facts capability"]
fn acceptance_profile_old_owner_refuses_authored_facts_before_staging() {
    isolated(&required("DS_PROFILE_OLD_ENGINE"), "old-authored");
}
#[test]
#[ignore = "requires explicit built authored-recovery owner and emitted owning open fixture"]
fn acceptance_profile_real_owner_retains_incremental_authored_rows_and_restores_exactly() {
    isolated(&required("DS_PROFILE_REAL_ENGINE"), "authored-recovery");
}
#[test]
#[ignore = "requires explicit built native Profile owner and emitted owning fixture"]
fn acceptance_profile_real_owner_consumed_artifacts_never_end_a_long_session() {
    isolated(&required("DS_PROFILE_REAL_ENGINE"), "long-session");
}
#[test]
#[ignore = "requires explicit previous engine without profile_authored_checkpoint_v1"]
fn acceptance_profile_old_owner_refuses_authored_recovery_before_staging() {
    isolated(
        &required("DS_PROFILE_PRE_AUTHORED_RECOVERY_ENGINE"),
        "old-authored-recovery",
    );
}
#[test]
#[ignore = "requires explicit adversarial retained native owner fixture"]
fn acceptance_profile_adversarial_receipts_supervision_and_captured_selection() {
    let fixture = required("DS_PROFILE_TEST_FIXTURE");
    let temp = tempfile::tempdir().unwrap();
    for mode in [
        "profile-capability",
        "profile-sequence",
        "profile-model",
        "profile-history",
        "profile-input",
        "profile-identity",
        "profile-name",
        "profile-digest",
        "profile-extra",
        "profile-symlink",
        "profile-open-hung",
        "profile-open-flood",
        "profile-read-hung",
        "profile-task-hung",
        "profile-task-cancel",
        "profile-stderr-flood",
        "profile-early-exit",
        "profile-good",
    ] {
        let exe = temp
            .path()
            .join(format!("{mode}{}", if cfg!(windows) { ".exe" } else { "" }));
        fs::copy(&fixture, &exe).unwrap();
        isolated(&exe, mode);
    }
}
#[test]
#[ignore = "subprocess isolates engine environment and actual child lifetime"]
fn isolated_profile_case() {
    #[cfg(target_os = "linux")]
    assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) }, 0);
    let case = env::var("DS_PROFILE_TEST_CASE").unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let input = if matches!(
        case.as_str(),
        "real"
            | "authored"
            | "old-authored"
            | "authored-recovery"
            | "old-authored-recovery"
            | "long-session"
    ) {
        fs::read(required("DS_PROFILE_OPEN_FIXTURE")).unwrap()
    } else {
        serde_json::to_vec(&NativeProfileOpenV1{schema:PROFILE_OPEN_SCHEMA.into(),package_lineage_json:None,seed_json:json!({"model_id":"fixture","history":{"initial_revision":format!("rev:{}", "1".repeat(64)),"model_revision":format!("rev:{}", "1".repeat(64)),"undo_depth":0,"redo_depth":0,"history_pin":"2".repeat(64)}}).to_string(),binding_history:None,binding_runs:None,calculated:None,analysis:None}).unwrap()
    };
    let start = Instant::now();
    let result = captured.open_profile_session_with_timeout(
        &input,
        scratch.path(),
        Duration::from_millis(
            if matches!(
                case.as_str(),
                "real"
                    | "authored"
                    | "old-authored"
                    | "authored-recovery"
                    | "old-authored-recovery"
                    | "long-session"
            ) {
                30000
            } else {
                200
            },
        ),
        &|| false,
    );
    if case == "profile-capability" {
        let e = match result {
            Err(e) => e,
            Ok(_) => panic!("missing capability accepted"),
        };
        assert_eq!(e.code(), "network_engine_incompatible");
        assert!(!captured.executable().with_extension("pid").exists());
        assert_eq!(
            fs::read_to_string(captured.executable().with_extension("calls"))
                .unwrap()
                .trim(),
            "build-info"
        );
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
        return;
    }
    if case == "profile-open-hung" || case == "profile-open-flood" {
        let e = match result {
            Err(e) => e,
            Ok(_) => panic!("open accepted {case}"),
        };
        assert_eq!(
            e.code(),
            if case == "profile-open-hung" {
                "callee_timed_out"
            } else {
                "network_receipt_bounded"
            }
        );
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_gone(&case);
        assert_eq!(
            fs::read_dir(scratch.path()).unwrap().count(),
            0,
            "startup staging leaked"
        );
        return;
    }
    let mut session = result.unwrap();
    let pid = session.process_id();
    if case == "long-session" {
        // Every artifact is consumed, so a session outlives the former
        // cumulative budget of MAX_PROFILE_ARTIFACTS files.
        for _ in 0..MAX_PROFILE_ARTIFACTS + 100 {
            state(&mut session);
        }
        assert!(!session.is_closed());
        assert_eq!(session.process_id(), pid);
        session.close(&|| false).unwrap();
        assert_dead(pid);
        drop(session);
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
        return;
    }
    if case == "old-authored-recovery" {
        let initial = session.history().clone();
        let refused = session.authored_checkpoint(None, &|| false).unwrap_err();
        assert_eq!(refused.code(), "network_engine_incompatible");
        assert_eq!(
            refused.owner_detail.as_deref().unwrap()["required_capability"],
            "profile_authored_checkpoint_v1"
        );
        let refused = session
            .restore_authored(b"{}", Duration::from_secs(5), &|| false)
            .unwrap_err();
        assert_eq!(refused.code(), "network_engine_incompatible");
        let run = fs::read_dir(scratch.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            fs::read_dir(&run).unwrap().count(),
            2,
            "no frame or staged recovery input reaches an older engine"
        );
        assert_eq!(session.history(), &initial);
        session.close(&|| false).unwrap();
        assert_dead(pid);
        return;
    }
    if case == "authored-recovery" {
        let initial = session.history().clone();
        let (_, empty) = session.authored_checkpoint(None, &|| false).unwrap();
        assert_eq!((empty.entry_count, empty.base_entries), (0, 0));
        session
            .request(
                NativeProfileOperationV1::CalculateScene { options: json!({}) },
                &|| false,
            )
            .unwrap();
        for id in ["recover-one", "recover-two"] {
            session.request(NativeProfileOperationV1::Control { operation: json!({"action":"apply", "envelope":{
                "command_id": id, "command_schema_version":11,
                "expected_revision": session.history()["model_revision"],
                "command":{"command_kind":"describe_structure", "id":"str-2", "description":id}
            }}) }, &|| false).unwrap();
        }
        let base = |document: &NativeProfileAuthoredCheckpointV1| NativeProfileAuthoredBaseV1 {
            entry_count: document.entry_count,
            chain_sha256: document.chain_sha256.clone(),
        };
        let (_, delta) = session
            .authored_checkpoint(Some(base(&empty)), &|| false)
            .unwrap();
        assert_eq!(
            (delta.entry_count, delta.base_entries, delta.entries.len()),
            (2, 0, 2)
        );
        let (_, unchanged) = session
            .authored_checkpoint(Some(base(&delta)), &|| false)
            .unwrap();
        assert_eq!((unchanged.base_entries, unchanged.entries.len()), (2, 0));
        let (_, complete) = session.authored_checkpoint(None, &|| false).unwrap();
        assert_eq!(complete.entries, delta.entries);
        let head = session.history().clone();
        let facts = session
            .request(NativeProfileOperationV1::Snapshot, &|| false)
            .unwrap()
            .bytes;
        session.close(&|| false).unwrap();
        assert_dead(pid);
        let mut restored = captured
            .open_profile_session(&input, scratch.path(), &|| false)
            .unwrap();
        let mut forged = complete.clone();
        forged.model_id = "another-model".into();
        let refused = restored
            .restore_authored(
                &serde_json::to_vec(&forged).unwrap(),
                Duration::from_secs(30),
                &|| false,
            )
            .unwrap_err();
        assert_eq!(refused.code(), "profile_recovery_input_invalid");
        assert_eq!(restored.history(), &initial);
        let (_, receipt) = restored
            .restore_authored(
                &serde_json::to_vec(&complete).unwrap(),
                Duration::from_secs(30),
                &|| false,
            )
            .unwrap();
        assert_eq!(receipt.entry_count, 2);
        assert_eq!(restored.history(), &head);
        assert_eq!(
            restored
                .request(NativeProfileOperationV1::Snapshot, &|| false)
                .unwrap()
                .bytes,
            facts,
            "replay onto the original reproduces the exact authored facts"
        );
        assert_eq!(state(&mut restored)["calculation_requests"], 0);
        restored.close(&|| false).unwrap();
        drop(restored);
        drop(session);
        assert_eq!(
            fs::read_dir(scratch.path()).unwrap().count(),
            0,
            "private input and result staging is cleaned"
        );
        return;
    }
    if case == "old-authored" {
        let initial = session.history().clone();
        let snapshot = session
            .request(NativeProfileOperationV1::Snapshot, &|| false)
            .unwrap_err();
        assert_eq!(snapshot.code(), "network_engine_incompatible");
        let journal = fs::read(required("DS_PROFILE_OWNER_JOURNAL")).unwrap();
        let target: Value =
            serde_json::from_slice(&fs::read(required("DS_PROFILE_OWNER_HISTORY")).unwrap())
                .unwrap();
        assert_eq!(
            session
                .reconcile_journal(&journal, target, &|| false)
                .unwrap_err()
                .code(),
            "network_engine_incompatible"
        );
        let run = fs::read_dir(scratch.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            fs::read_dir(&run).unwrap().count(),
            2,
            "only original open and output directory exist"
        );
        assert_eq!(session.history(), &initial);
        assert_eq!(state(&mut session)["calculation_requests"], 0);
        session.close(&|| false).unwrap();
        assert_dead(pid);
        return;
    }
    if case == "authored" {
        let journal = fs::read(required("DS_PROFILE_OWNER_JOURNAL")).unwrap();
        assert!(journal.len() > MAX_PROFILE_CONTROL_BYTES);
        let target: Value =
            serde_json::from_slice(&fs::read(required("DS_PROFILE_OWNER_HISTORY")).unwrap())
                .unwrap();
        let initial = session.history().clone();
        let snapshot = session
            .request(NativeProfileOperationV1::Snapshot, &|| false)
            .unwrap()
            .bytes;
        let facts: NativeProfileSnapshotV1 = serde_json::from_slice(&snapshot).unwrap();
        assert!(facts.snapshot_json.contains("-0.0"));
        assert_eq!(facts.history, initial);
        assert_eq!(state(&mut session)["calculation_requests"], 0);
        for file in [
            "../journal.json",
            "/journal.json",
            "https://example.invalid/journal",
        ] {
            let refusal = session
                .request(
                    NativeProfileOperationV1::ReconcileJournal {
                        journal: ds_network_contracts::engine::ArtifactReceipt {
                            file: file.into(),
                            byte_count: journal.len(),
                            sha256: "0".repeat(64),
                            schema: PROFILE_JOURNAL_SCHEMA.into(),
                        },
                        target_history: target.clone(),
                    },
                    &|| false,
                )
                .unwrap_err();
            assert_eq!(refusal.code(), "profile_replay_input_invalid");
            assert_eq!(session.history(), &initial);
        }
        let outcome = session
            .reconcile_journal(&journal, target.clone(), &|| false)
            .unwrap();
        let result: NativeProfileReconcileV1 = serde_json::from_slice(&outcome.bytes).unwrap();
        assert_eq!(
            result.snapshot.snapshot_json.as_bytes(),
            fs::read(required("DS_PROFILE_OWNER_SNAPSHOT")).unwrap()
        );
        assert_eq!(result.snapshot.journal_json.as_bytes(), journal);
        assert_eq!(session.history(), &target);
        assert_eq!(
            session.process_id(),
            pid,
            "one held model retains one child"
        );
        let status = state(&mut session);
        assert_eq!(status["calculation_requests"], 0);
        assert_eq!(status["analysis_requests"], 0);
        assert_eq!(status["session_initializations"], 1);
        let refused = session
            .reconcile_journal(b"{}", target.clone(), &|| false)
            .unwrap_err();
        assert_eq!(refused.owner_code(), Some("profile_replay_invalid"));
        assert_eq!(session.history(), &target);
        assert!(!session.is_closed());
        let facts_bytes = session
            .request(NativeProfileOperationV1::Snapshot, &|| false)
            .unwrap()
            .bytes;
        let checkpoint = session.checkpoint(&|| false).unwrap().bytes;
        let pin = session.pin().clone();
        session.close(&|| false).unwrap();
        assert_dead(pid);
        let captured = CapturedNetworkClient::from_pin(&pin, &|| false).unwrap();
        let mut resumed = captured
            .open_profile_session(&checkpoint, scratch.path(), &|| false)
            .unwrap();
        assert_eq!(
            resumed
                .request(NativeProfileOperationV1::Snapshot, &|| false)
                .unwrap()
                .bytes,
            facts_bytes
        );
        assert_eq!(state(&mut resumed)["calculation_requests"], 0);
        resumed.close(&|| false).unwrap();
        drop(resumed);
        drop(session);
        assert_eq!(
            fs::read_dir(scratch.path()).unwrap().count(),
            0,
            "private input and result staging is cleaned"
        );
        return;
    }
    if case == "real" {
        let initial_state = state(&mut session);
        assert_eq!(initial_state["calculation_requests"], 0);
        let analysis = session.request(NativeProfileOperationV1::Analyze { request: json!({
            "expected_revision": initial_state["revision"]["revision_id"],
            "expected_engineering_input_root": initial_state["revision"]["roots"]["engineering_input_root"],
            "max_rows":2000
        }) }, &|| false).unwrap().bytes;
        assert_eq!(state(&mut session)["calculation_requests"], 0);
        assert_eq!(
            session
                .request(NativeProfileOperationV1::CachedAnalysis, &|| false)
                .unwrap()
                .bytes,
            analysis
        );
        let scene = session
            .request(
                NativeProfileOperationV1::CalculateScene { options: json!({}) },
                &|| false,
            )
            .unwrap()
            .bytes;
        assert_eq!(
            scene,
            fs::read(required("DS_PROFILE_OWNER_SCENE")).unwrap(),
            "exact independently captured owner scene bytes"
        );
        for _ in 0..3 {
            assert_eq!(
                session
                    .request(NativeProfileOperationV1::CachedScene, &|| false)
                    .unwrap()
                    .bytes,
                scene
            );
        }
        let scene_value: Value = serde_json::from_slice(&scene).unwrap();
        let display_case = scene_value["display_case"]["requested"].clone();
        let case_bytes = session
            .request(
                NativeProfileOperationV1::CachedCase {
                    display_case: display_case.clone(),
                },
                &|| false,
            )
            .unwrap()
            .bytes;
        for _ in 0..3 {
            assert_eq!(
                session
                    .request(
                        NativeProfileOperationV1::CachedCase {
                            display_case: display_case.clone()
                        },
                        &|| false
                    )
                    .unwrap()
                    .bytes,
                case_bytes
            );
        }
        let status = state(&mut session);
        assert_eq!(status["calculation_requests"], 1);
        assert_eq!(status["analysis_requests"], 1);
        assert_eq!(status["session_initializations"], 1);
        let mut stale = session.history().clone();
        stale["redo_depth"] = json!(999);
        let e = session
            .request_at(
                NativeProfileOperationV1::CachedScene,
                Some(stale),
                Duration::from_secs(5),
                &|| false,
            )
            .unwrap_err();
        assert_eq!(e.owner_code(), Some("profile_replay_stale"));
        assert!(!session.is_closed());
        assert_eq!(
            session
                .request(NativeProfileOperationV1::CachedScene, &|| false)
                .unwrap()
                .bytes,
            scene
        );
        let checkpoint = session.checkpoint(&|| false).unwrap().bytes;
        assert_eq!(session.last_checkpoint().unwrap().bytes, checkpoint);
        // Results are disposable RAM state; the authored document omits them.
        let open: NativeProfileOpenV1 = serde_json::from_slice(&checkpoint).unwrap();
        assert!(open.calculated.is_none() && open.analysis.is_none());
        let pin = session.pin().clone();
        session.close(&|| false).unwrap();
        assert_dead(pid);
        let resumed = CapturedNetworkClient::from_pin(&pin, &|| false).unwrap();
        let mut session = resumed
            .open_profile_session(&checkpoint, scratch.path(), &|| false)
            .unwrap();
        for (operation, code) in [
            (
                NativeProfileOperationV1::CachedScene,
                "profile_case_base_unavailable",
            ),
            (
                NativeProfileOperationV1::CachedAnalysis,
                "profile_analysis_unavailable",
            ),
        ] {
            let refused = session.request(operation, &|| false).unwrap_err();
            assert_eq!(refused.owner_code(), Some(code));
            assert!(!session.is_closed());
        }
        let resumed_state = state(&mut session);
        assert_eq!(resumed_state["calculation_requests"], 0);
        assert_eq!(resumed_state["analysis_requests"], 0);
        assert_eq!(
            session
                .request(
                    NativeProfileOperationV1::CalculateScene { options: json!({}) },
                    &|| false
                )
                .unwrap()
                .bytes,
            scene,
            "explicit calculation reproduces the discarded scene"
        );
        assert_eq!(
            session
                .request(
                    NativeProfileOperationV1::CachedCase { display_case },
                    &|| false
                )
                .unwrap()
                .bytes,
            case_bytes
        );
        assert_eq!(
            session
                .request(
                    NativeProfileOperationV1::Analyze {
                        request: json!({
                            "expected_revision": initial_state["revision"]["revision_id"],
                            "expected_engineering_input_root": initial_state["revision"]["roots"]["engineering_input_root"],
                            "max_rows":2000
                        })
                    },
                    &|| false
                )
                .unwrap()
                .bytes,
            analysis
        );
        session.close(&|| false).unwrap();
        // Complete retained history is transported, including redo and assets.
        let mut session = captured
            .open_profile_session(&input, scratch.path(), &|| false)
            .unwrap();
        for id in ["client-one", "client-two"] {
            session.request(NativeProfileOperationV1::Control { operation: json!({"action":"apply", "envelope":{
                "command_id": id, "command_schema_version":11,
                "expected_revision": session.history()["model_revision"],
                "command":{"command_kind":"describe_structure", "id":"str-2", "description":id}
            }}) }, &|| false).unwrap();
        }
        session
            .request(
                NativeProfileOperationV1::Control {
                    operation: json!({"action":"undo"}),
                },
                &|| false,
            )
            .unwrap();
        assert_eq!(session.history()["redo_depth"], 1);
        let checkpoint = session.close(&|| false).unwrap().bytes;
        let document: NativeProfileOpenV1 = serde_json::from_slice(&checkpoint).unwrap();
        // Unchanged binding states travel once, as runs over every authored state.
        assert!(document.binding_history.is_none());
        assert_eq!(
            document
                .binding_runs
                .as_ref()
                .unwrap()
                .iter()
                .map(|run| run.states)
                .sum::<u64>(),
            3
        );
        let original: NativeProfileOpenV1 = serde_json::from_slice(&input).unwrap();
        let old_seed: Value = serde_json::from_str(&original.seed_json).unwrap();
        let seed: Value = serde_json::from_str(&document.seed_json).unwrap();
        assert_eq!(seed["assets"], old_seed["assets"]);
        let mut resumed = captured
            .open_profile_session(&checkpoint, scratch.path(), &|| false)
            .unwrap();
        assert_eq!(resumed.history()["redo_depth"], 1);
        resumed
            .request(
                NativeProfileOperationV1::Control {
                    operation: json!({"action":"redo"}),
                },
                &|| false,
            )
            .unwrap();
        assert_eq!(resumed.history()["undo_depth"], 2);
        resumed.close(&|| false).unwrap();
    } else if case == "profile-good" {
        for _ in 0..3 {
            assert_eq!(state(&mut session)["calculation_requests"], 0);
        }
        let pin = session.pin().clone();
        let checkpoint = session.checkpoint(&|| false).unwrap().bytes;
        session.close(&|| false).unwrap();
        assert_dead(pid);
        let log = fs::read_to_string(pin.executable.with_extension("calls")).unwrap();
        assert_eq!(log.lines().filter(|s| *s == "build-info").count(), 1);
        assert_eq!(log.lines().filter(|s| *s == "profile-session").count(), 1);
        // Retained path is selected independently of the current active pointer.
        env::set_var(
            "DS_NETWORK_ENGINE_BIN",
            pin.executable.with_file_name("profile-incompatible"),
        );
        let resumed = CapturedNetworkClient::from_pin(&pin, &|| false).unwrap();
        let mut held = resumed
            .open_profile_session(&checkpoint, scratch.path(), &|| false)
            .unwrap();
        state(&mut held);
        let heldpid = held.process_id();
        let before = fs::read_to_string(pin.executable.with_extension("calls"))
            .unwrap()
            .lines()
            .filter(|s| s.starts_with("frame:"))
            .count();
        let failure = held
            .request(NativeProfileOperationV1::State, &|| true)
            .unwrap_err();
        assert_eq!(failure.code(), "callee_cancelled");
        assert_dead(heldpid);
        let after = fs::read_to_string(pin.executable.with_extension("calls"))
            .unwrap()
            .lines()
            .filter(|s| s.starts_with("frame:"))
            .count();
        assert_eq!(before, after, "cancelled request was sent to compute");
        drop(held);
        let log = fs::read_to_string(pin.executable.with_extension("calls")).unwrap();
        assert_eq!(log.lines().filter(|s| *s == "build-info").count(), 2);
        assert_eq!(log.lines().filter(|s| *s == "profile-session").count(), 2);
        let held = resumed
            .open_profile_session(&checkpoint, scratch.path(), &|| false)
            .unwrap();
        let pid = held.process_id();
        drop(held);
        assert_dead(pid);
    } else {
        let started = Instant::now();
        let operation = if case == "profile-read-hung" {
            NativeProfileOperationV1::Control {
                operation: json!({"padding":"x".repeat(60000)}),
            }
        } else {
            NativeProfileOperationV1::State
        };
        let result = session.request_at(
            operation,
            Some(session.history().clone()),
            Duration::from_millis(200),
            &|| case == "profile-task-cancel" && started.elapsed() > Duration::from_millis(50),
        );
        let e = result.unwrap_err();
        let expected = match case.as_str() {
            "profile-digest" => "network_result_digest_mismatch",
            "profile-symlink" => "network_result_invalid",
            "profile-read-hung" | "profile-task-hung" => "callee_timed_out",
            "profile-stderr-flood" => "network_receipt_bounded",
            "profile-task-cancel" => "callee_cancelled",
            "profile-early-exit" => "profile_session_closed",
            _ => "network_profile_reply_invalid",
        };
        if case == "profile-early-exit" {
            assert!(matches!(
                e.code(),
                "profile_session_closed" | "callee_stdin_failed"
            ));
        } else {
            assert_eq!(e.code(), expected);
        }
        assert!(session.is_closed());
        assert_dead(pid);
        assert_gone(&case);
    }
    drop(session);
    assert_eq!(
        fs::read_dir(scratch.path()).unwrap().count(),
        0,
        "private staging leaked"
    );
}
fn assert_gone(case: &str) {
    let exe = PathBuf::from(env::var_os("DS_NETWORK_ENGINE_BIN").unwrap());
    if let Ok(pid) = fs::read_to_string(exe.with_extension("pid")) {
        assert_dead(pid.parse().unwrap());
    }
    if let Ok(pid) = fs::read_to_string(exe.with_extension("descendant")) {
        assert_dead(pid.parse().unwrap());
    }
    let _ = case;
}
fn assert_dead(pid: u32) {
    #[cfg(target_os = "linux")]
    {
        let path = format!("/proc/{pid}/stat");
        if let Ok(stat) = fs::read_to_string(path) {
            let state = stat.rsplit_once(") ").unwrap().1.chars().next().unwrap();
            assert!(
                state == 'Z' || state == 'X',
                "process {pid} remains {state}"
            );
            let mut status = 0;
            assert_eq!(
                unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
                pid as i32,
                "owned descendant was not reaped"
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
    }
}
