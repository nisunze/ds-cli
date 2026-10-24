use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn an_old_addon_preserves_its_typed_refusal_through_final_import_without_mutation() {
    let root = crate::test_directory::tempdir().unwrap();
    let path = root.path().join("solar");
    fs::write(&path, "#!/bin/sh\ncase \"$1\" in\nbuild-info) printf '%s' '{\"name\":\"ds-solar-engine\",\"version\":\"old\",\"schemas\":[]}' ;;\n*) touch \"$(dirname \"$0\")/unexpected-task\"; exit 90;;\nesac\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    // The override lives only in this child; concurrent tests retain their addon.
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "native::final_lint_application_tests::old_addon_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("DS_SOLAR_ENGINE_BIN", &path)
        .env("DS_FINAL_LINT_REFUSAL_FIXTURE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.path().join("unexpected-task").exists());
}

#[test]
#[ignore = "private subprocess control for the final import refusal"]
fn old_addon_fixture() {
    assert_eq!(std::env::var("DS_FINAL_LINT_REFUSAL_FIXTURE").unwrap(), "1");
    super::tests::final_import_scenario(true);
}
