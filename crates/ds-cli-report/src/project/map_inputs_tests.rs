use super::{
    ACQUISITION_STALLED, CONTEXTS_SCHEMA, REFUSALS, REUSED_CAPTURE_INVALID, REUSED_CAPTURE_STALE,
    bounded, parse_bounds, principal_sha256, read_context_batch, read_layout, reused_contexts,
};
use ds_cli_contract::outcome::Failure;

#[test]
fn missing_layout_refusal_names_the_exact_authored_file() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("maps/layout-rulindo.json");
    let error = read_layout(missing.to_str().unwrap()).unwrap_err();
    assert_eq!(error.code(), "report_inputs_invalid");
    assert!(error.message().contains("open --layout"), "{error:?}");
    assert!(
        error
            .message()
            .contains(&missing.to_string_lossy().to_string()),
        "{error:?}"
    );
}

#[test]
fn transient_context_batch_is_retried_then_read() {
    let mut calls = 0;
    let mut sleeps = Vec::new();
    let result = read_context_batch(
        || {
            calls += 1;
            match calls {
                1 => Err(Failure::unavailable("auth_transient", "temporary")),
                2 => Err(Failure::unavailable("device_auth_transient", "temporary")),
                _ => Ok(42),
            }
        },
        |delay| sleeps.push(delay.as_secs()),
    );
    assert_eq!(result.unwrap(), 42);
    assert_eq!(calls, 3);
    assert_eq!(sleeps, [2, 5]);
}

#[test]
fn permanent_context_refusal_does_not_retry() {
    let mut calls = 0;
    let mut sleeps = Vec::new();
    let result = read_context_batch::<()>(
        || {
            calls += 1;
            Err(Failure::unauthorized("auth_rejected", "denied"))
        },
        |delay| sleeps.push(delay),
    );
    assert_eq!(result.unwrap_err().code(), "auth_rejected");
    assert_eq!(calls, 1);
    assert!(sleeps.is_empty());
}

#[test]
fn transient_context_batch_exhausts_after_three_reads() {
    let mut calls = 0;
    let result = read_context_batch::<()>(
        || {
            calls += 1;
            Err(Failure::unavailable("auth_transient", "temporary"))
        },
        |_| {},
    );
    assert_eq!(result.unwrap_err().code(), "auth_transient");
    assert_eq!(calls, 3);
}

#[test]
fn a_stalled_acquisition_step_is_refused_by_name_and_documented() {
    // f48bd93c: a read that never answers used to hang the capture with
    // no JSON. It is refused by name within its bound instead.
    let error = bounded(
        "the inventory read",
        std::time::Duration::from_millis(50),
        || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), ACQUISITION_STALLED.code);
    assert!(
        error
            .message()
            .contains("the inventory read answered nothing"),
        "{error:?}"
    );
    assert!(error.remedy_text().is_some());
    let answered = bounded("a read", std::time::Duration::from_secs(5), || Ok(7)).unwrap();
    assert_eq!(answered, 7);
    let refused = bounded::<()>("a read", std::time::Duration::from_secs(5), || {
        Err(Failure::unauthorized("auth_rejected", "denied"))
    })
    .unwrap_err();
    assert_eq!(refused.code(), "auth_rejected");
    for code in [
        ACQUISITION_STALLED.code,
        REUSED_CAPTURE_INVALID.code,
        REUSED_CAPTURE_STALE.code,
        "report_inputs_invalid",
    ] {
        assert!(
            REFUSALS.iter().any(|refusal| refusal.code == code),
            "{code}"
        );
    }
}

#[test]
fn a_reused_capture_is_digest_pinned_to_its_project_principal_and_active_rooms() {
    let root = tempfile::tempdir().unwrap();
    let capture = root.path().join("sheet-001");
    std::fs::create_dir(&capture).unwrap();
    let write = |document: &serde_json::Value| {
        let bytes = serde_json::to_vec(document).unwrap();
        let path = capture.join("transformer-contexts.json");
        std::fs::write(&path, &bytes).unwrap();
        let sha256 = ds_command_kernel::report_export::sha256_hex(&bytes);
        std::fs::write(
            capture.join("sources.json"),
            serde_json::to_vec(&serde_json::json!({"project":"p1","transformer_contexts":{"path":path,"sha256":sha256,"reused":false}})).unwrap(),
        )
        .unwrap();
        path
    };
    let document = serde_json::json!({"schema":CONTEXTS_SCHEMA,"project":"p1","principal_sha256":principal_sha256("uid-a"),"transformers":[
        {"transformer":"tr-b","version":3,"content_digest":"d-b","layers":{}},
        {"transformer":"tr-a","version":1,"content_digest":"d-a","layers":{}}]});
    let path = write(&document);
    let active = ["tr-a".to_owned(), "tr-b".to_owned()];
    let (reused, pinned, _) = reused_contexts(&capture, "p1", "uid-a", &active).unwrap();
    assert_eq!(pinned, path);
    assert_eq!(reused["transformers"][0]["content_digest"], "d-b");
    // Another project, another principal or a changed room set is refused.
    assert_eq!(
        reused_contexts(&capture, "p2", "uid-a", &active)
            .unwrap_err()
            .code(),
        REUSED_CAPTURE_INVALID.code
    );
    assert_eq!(
        reused_contexts(&capture, "p1", "uid-b", &active)
            .unwrap_err()
            .code(),
        REUSED_CAPTURE_INVALID.code
    );
    let grown = ["tr-a".to_owned(), "tr-b".to_owned(), "tr-c".to_owned()];
    let stale = reused_contexts(&capture, "p1", "uid-a", &grown).unwrap_err();
    assert_eq!(stale.code(), REUSED_CAPTURE_STALE.code);
    assert!(
        stale.message().starts_with("1 active transformers"),
        "{stale:?}"
    );
    // Bytes that no longer match the recorded digest are not reused.
    std::fs::write(&path, b"{}").unwrap();
    let tampered = reused_contexts(&capture, "p1", "uid-a", &active).unwrap_err();
    assert_eq!(tampered.code(), REUSED_CAPTURE_INVALID.code);
    assert!(tampered.message().contains("SHA-256"), "{tampered:?}");
    // A directory that never pinned a capture names the repair.
    let empty = root.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    assert_eq!(
        reused_contexts(&empty, "p1", "uid-a", &active)
            .unwrap_err()
            .code(),
        REUSED_CAPTURE_INVALID.code
    );
}

#[test]
fn district_area_accepts_authority_extent_while_sheet_focus_stays_bounded() {
    let nyamagabe = "29.2646534136055,-2.60032940893701,29.6660934836079,-2.19979084366594";
    assert!(parse_bounds(nyamagabe, "area-bounds", 0.5).is_ok());
    assert!(parse_bounds(nyamagabe, "focus-bounds", 0.05).is_err());
    assert!(parse_bounds("29,-3,28,-2", "area-bounds", 0.5).is_err());
    assert!(parse_bounds("29,-3,30,-2", "area-bounds", 0.5).is_err());
}
