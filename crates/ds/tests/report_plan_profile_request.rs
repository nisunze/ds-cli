//! `report plan-profile --request` hands a held engine request, byte for byte,
//! to the engine call a project render makes, and reads no project.
#![cfg(unix)]
use serde_json::{Value, json};
use std::path::Path;
use std::{os::unix::fs::PermissionsExt, process::Command};

fn plan_profile(root: &Path, args: &[&str]) -> (bool, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["report", "plan-profile"])
        .args(args)
        .args(["--output", "json"])
        .env("DS_REPORT_BIN", root.join("ds-report"))
        .env("TEST_REQUEST", root.join("request.json"))
        .env("TEST_CALLS", root.join("calls"))
        .env("TMPDIR", root)
        .env("DS_CONFIG_HOME", root)
        .env("DS_DESKTOP_DESCRIPTOR", root.join("absent"))
        .output()
        .unwrap();
    let answer = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    (output.status.success(), answer)
}

#[test]
fn request_mode_renders_the_exact_request_without_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let engine = root.join("ds-report");
    std::fs::write(
        &engine,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$1" >> "$TEST_CALLS"
test "$1" = render-grid-plan-profile
test "$#" = 5
test "$2" = --request
test "$4" = --result
cmp "$3" "$TEST_REQUEST"
test ! -e "$5"
printf '%s' '{"project_id":"global_fixture","model_revision":"r1","alignments":1,"booklet_previews":[{"raster_sha256":"sha256:00"}]}' > "$5"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
    let request = root.join("request.json");
    let raw =
        json!({"project_id":"global_fixture","preview_only":true,"out_dir":root.join("booklet")})
            .to_string();
    std::fs::write(&request, &raw).unwrap();
    let path = request.to_str().unwrap();
    let receipt = root.join("receipt.json");

    let (ok, answer) = plan_profile(
        root,
        &["--request", path, "--result", receipt.to_str().unwrap()],
    );
    assert!(ok, "{answer}");
    assert_eq!(
        answer["data"]["booklet_previews"][0]["raster_sha256"],
        "sha256:00"
    );
    assert_eq!(answer["data"]["result_path"], receipt.to_str().unwrap());
    assert!(receipt.is_file());
    assert_eq!(std::fs::read_to_string(&request).unwrap(), raw);
    let staged = std::fs::read_dir(root).unwrap().flatten().any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with("ds-grid-print-")
    });
    assert!(!staged, "the private request copy outlived the engine call");

    // Mixed modes and unusable requests refuse before the engine is called.
    for (args, code) in [
        (
            &["--request", path, "--project", "fixture"][..],
            "request_mode_invalid",
        ),
        (
            &["--scene", path, "--plan", path, "--out-dir", "/fresh"],
            "request_mode_invalid",
        ),
        (
            &["--request", path, "--result", receipt.to_str().unwrap()],
            "output_exists",
        ),
    ] {
        let (ok, answer) = plan_profile(root, args);
        assert!(!ok);
        assert_eq!(answer["error"]["code"], code, "{args:?}");
    }
    std::fs::write(&request, b"{not json").unwrap();
    let (_, answer) = plan_profile(root, &["--request", path]);
    assert_eq!(answer["error"]["code"], "print_request_invalid");
    let calls = std::fs::read_to_string(root.join("calls")).unwrap();
    assert_eq!(calls.lines().count(), 1, "{calls}");
}

#[test]
fn preview_layout_mode_refuses_mixed_or_unusable_inputs_before_any_read() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let engine = root.join("ds-report");
    std::fs::write(
        &engine,
        "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$TEST_CALLS\"\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
    let projection = root.join("projection.json");
    std::fs::write(&projection, b"{}").unwrap();
    let layout = root.join("layout.json");
    std::fs::write(&layout, b"{\"not\":\"a layout\"}").unwrap();
    let p = projection.to_str().unwrap();
    let l = layout.to_str().unwrap();
    let missing = root.join("absent.json");
    let m = missing.to_str().unwrap();
    for (args, code) in [
        (
            &[
                "--preview-layout",
                l,
                "--project",
                "p",
                "--scene",
                p,
                "--plan",
                p,
                "--out-dir",
                "/fresh",
            ][..],
            "request_mode_invalid",
        ),
        (
            &[
                "--preview-layout",
                l,
                "--project",
                "p",
                "--scene",
                p,
                "--plan",
                p,
                "--sample-pages",
                "2",
            ][..],
            "request_mode_invalid",
        ),
        (
            &[
                "--preview-layout",
                l,
                "--request",
                p,
                "--project",
                "p",
                "--scene",
                p,
                "--plan",
                p,
            ][..],
            "request_mode_invalid",
        ),
        (
            &["--preview-layout", l, "--scene", p, "--plan", p][..],
            "request_mode_invalid",
        ),
        (
            &[
                "--preview-layout",
                l,
                "--project",
                "p",
                "--scene",
                m,
                "--plan",
                p,
            ][..],
            "projection_missing",
        ),
        (
            &[
                "--preview-layout",
                l,
                "--project",
                "p",
                "--scene",
                p,
                "--plan",
                p,
            ][..],
            "preview_layout_invalid",
        ),
    ] {
        let (ok, answer) = plan_profile(root, args);
        assert!(!ok);
        assert_eq!(answer["error"]["code"], code, "{args:?}");
    }
    assert!(!root.join("calls").exists(), "no engine call");
}
