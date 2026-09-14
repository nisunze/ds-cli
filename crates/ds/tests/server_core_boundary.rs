//! Desktop consumes server core; server compilation never consumes ds-web.
use std::{path::Path, process::Command};
#[test]
fn server_dependency_graph_contains_no_web_or_tauri_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .current_dir(&root)
        .output()
        .expect("Cargo dependency graph");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    for package in graph["packages"].as_array().unwrap() {
        let path = package["manifest_path"]
            .as_str()
            .unwrap()
            .replace('\\', "/");
        let name = package["name"].as_str().unwrap();
        assert!(
            !path.contains("/ds-web/"),
            "server compiles web-owned source: {path}"
        );
        assert!(
            !name.starts_with("tauri"),
            "server depends on desktop shell: {name}"
        );
    }
    let build = std::fs::read_to_string(root.join("crates/ds-cli-auth/build.rs")).unwrap();
    assert!(
        !build.contains("ds-web"),
        "server build script reads desktop-owned inputs"
    );
}
