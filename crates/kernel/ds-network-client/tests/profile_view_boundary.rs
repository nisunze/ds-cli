//! Actual captured-process acceptance. Inputs are emitted by the owning fixture.
use ds_network_client::{CapturedNetworkClient, ProfileSession};
use ds_network_contracts::profile::*;
use serde_json::Value;
use std::{env, fs, path::PathBuf, process::Command, time::Duration};
fn path(key: &str) -> PathBuf {
    PathBuf::from(env::var_os(key).expect(key))
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
#[test]
#[ignore = "requires exact qualified new/previous engines and emitted owning input fixtures"]
fn acceptance_profile_complete_view_capability_staging_and_restart() {
    for (case, engine) in [
        ("current", "DS_PROFILE_REAL_ENGINE"),
        ("old", "DS_PROFILE_OLD_ENGINE"),
        ("collision", "DS_PROFILE_REAL_ENGINE"),
        ("partial-write", "DS_PROFILE_REAL_ENGINE"),
    ] {
        let result = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_complete_view",
                "--ignored",
                "--nocapture",
            ])
            .env("DS_NETWORK_ENGINE_BIN", path(engine))
            .env("DS_PROFILE_VIEW_CASE", case)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{case}: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
#[test]
#[ignore = "isolates captured engine selection in an acceptance subprocess"]
fn isolated_complete_view() {
    let scratch = tempfile::tempdir().unwrap();
    let client = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let open = fs::read(path("DS_PROFILE_OPEN_FIXTURE")).unwrap();
    let input: NativeProfileViewInputV1 =
        serde_json::from_slice(&fs::read(path("DS_PROFILE_VIEW_INPUT")).unwrap()).unwrap();
    let mut session = client
        .open_profile_session(&open, scratch.path(), &|| false)
        .unwrap();
    let history = session.history().clone();
    let run = fs::read_dir(scratch.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let no_staged_input = || {
        assert_eq!(
            fs::read_dir(&run).unwrap().count(),
            2,
            "only original open and output remain"
        );
    };
    no_staged_input();
    let case = env::var("DS_PROFILE_VIEW_CASE").unwrap();
    if case == "collision" {
        fs::write(run.join("profile-input-0000000001.json"), b"occupied").unwrap();
        assert!(
            session
                .view_at(
                    &input,
                    false,
                    history.clone(),
                    Duration::from_secs(30),
                    &|| false
                )
                .is_err()
        );
        assert!(session.is_closed());
        drop(session);
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
        return;
    }
    if case == "partial-write" {
        #[cfg(target_os = "linux")]
        {
            // This isolated test process limits only its own file writes after
            // opening the worker. EFBIG leaves an actual partial staged file.
            let mut previous = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            assert_eq!(
                unsafe { libc::getrlimit(libc::RLIMIT_FSIZE, &mut previous) },
                0
            );
            let handler = unsafe { libc::signal(libc::SIGXFSZ, libc::SIG_IGN) };
            let limit = libc::rlimit {
                rlim_cur: 2048,
                rlim_max: previous.rlim_max,
            };
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &limit) }, 0);
            let result = session.view_at(
                &input,
                false,
                history.clone(),
                Duration::from_secs(30),
                &|| false,
            );
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &previous) }, 0);
            unsafe {
                libc::signal(libc::SIGXFSZ, handler);
            }
            assert!(result.is_err());
            assert!(session.is_closed());
            assert_eq!(
                fs::metadata(run.join("profile-input-0000000001.json"))
                    .unwrap()
                    .len(),
                2048
            );
            drop(session);
            assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
        }
        return;
    }
    if case == "old" {
        let error = session
            .view_at(
                &input,
                false,
                history.clone(),
                Duration::from_secs(30),
                &|| false,
            )
            .unwrap_err();
        assert_eq!(error.code(), "network_engine_incompatible");
        no_staged_input();
        assert_eq!(state(&mut session)["calculation_requests"], 0);
        assert_eq!(session.history(), &history);
        session.close(&|| false).unwrap();
        return;
    }
    assert!(serde_json::to_vec(&input).unwrap().len() > MAX_PROFILE_CONTROL_BYTES);
    let first = session
        .view_at(
            &input,
            false,
            history.clone(),
            Duration::from_secs(30),
            &|| false,
        )
        .unwrap();
    no_staged_input();
    let cached = session
        .view_at(
            &input,
            true,
            history.clone(),
            Duration::from_secs(30),
            &|| false,
        )
        .unwrap();
    assert_eq!(cached.bytes, first.bytes);
    assert_eq!(state(&mut session)["calculation_requests"], 1);
    let mut malformed = input.clone();
    malformed.style_document = serde_json::json!({});
    assert!(
        session
            .view_at(
                &malformed,
                true,
                history.clone(),
                Duration::from_secs(30),
                &|| false
            )
            .is_err()
    );
    no_staged_input();
    assert!(!session.is_closed());
    let mut oversized = input.clone();
    oversized.style_document = Value::String("x".repeat(MAX_PROFILE_VIEW_INPUT_BYTES));
    assert_eq!(
        session
            .view_at(
                &oversized,
                true,
                history.clone(),
                Duration::from_secs(30),
                &|| false
            )
            .unwrap_err()
            .code(),
        "profile_view_input_invalid"
    );
    no_staged_input();
    drop(oversized);
    let closed = session.close(&|| false).unwrap();
    let resumed_scratch = tempfile::tempdir().unwrap();
    let mut resumed = client
        .open_profile_session(&closed.bytes, resumed_scratch.path(), &|| false)
        .unwrap();
    let result = resumed
        .view_at(
            &input,
            true,
            history.clone(),
            Duration::from_secs(30),
            &|| false,
        )
        .unwrap();
    assert_eq!(result.bytes, first.bytes);
    assert_eq!(state(&mut resumed)["calculation_requests"], 0);
    assert_eq!(resumed.history(), &history);
    resumed.close(&|| false).unwrap();
}
