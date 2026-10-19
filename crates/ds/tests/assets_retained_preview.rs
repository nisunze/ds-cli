//! Exact native CLI/MCP boundary for retained PDFs; no operator state or source reads.
use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
fn command(config: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ds"));
    cmd.env("DS_CONFIG_HOME", config)
        .env("DS_DESKTOP_DESCRIPTOR", config.join("no-desktop.json"))
        .env(
            "DS_NATIVE_CLIENT_PROFILE_BUNDLE",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../ds-cli-auth/tests/fixtures/development-catalog.json"),
        );
    cmd
}
fn cli(config: &Path, args: &[&str]) -> Value {
    let out = command(config).args(args).output().unwrap();
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|_| panic!("non-JSON CLI: {}", String::from_utf8_lossy(&out.stdout)))
}
#[test]
fn retained_preview_declares_native_pins_and_refuses_unpinned_reads_before_source() {
    // The maintained development catalog is deliberately held v30; compiled
    // v31 must refuse it, not fabricate an admitted release or source read.
    // Argument/source semantics are tested directly in the owning modules.
    let config = tempfile::tempdir().unwrap();
    let described = cli(
        config.path(),
        &["capabilities", "assets.preview", "--output", "json"],
    );
    let descriptor = &described["data"]["command"];
    assert_eq!(descriptor["contract"], 2);
    assert_eq!(descriptor["effect"], "read_only");
    for name in [
        "retain",
        "offline",
        "expected-revision",
        "expected-generation",
        "expected-sha256",
        "preview-retention",
    ] {
        assert!(
            descriptor["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["name"] == name)
        );
    }
    for extra in [
        vec!["--retain", "--offline"],
        vec!["--offline"],
        vec!["--retain", "--pages", "5"],
        vec!["--retain", "--rows", "200"],
    ] {
        let mut args = vec![
            "assets",
            "preview",
            "--project",
            "test-project",
            "--asset",
            "sys:pm_attachment:pma_one",
        ];
        args.extend(extra);
        args.extend(["--output", "json"]);
        let envelope = cli(config.path(), &args);
        // The development catalogue admits the compiled profile, so the
        // missing exact revision/generation/SHA pins are refused locally,
        // before any credential or source read.
        assert_eq!(
            envelope["error"]["code"], "asset_preview_pins_invalid",
            "{envelope}"
        );
        assert!(envelope["data"].is_null());
    }
    for asset in [
        "a_wrong",
        "sys:pm_attachment:..",
        "sys:pm_attachment:parent/child",
    ] {
        let rejected = cli(
            config.path(),
            &[
                "assets",
                "preview",
                "--project",
                "test-project",
                "--asset",
                asset,
                "--retain",
                "--output",
                "json",
            ],
        );
        assert_eq!(
            rejected["error"]["code"], "asset_preview_pins_invalid",
            "{rejected}"
        );
    }
    let online = cli(
        config.path(),
        &[
            "assets",
            "preview",
            "--project",
            "test-project",
            "--asset",
            "sys:pm_attachment:pma_one",
            "--retain",
            "--output",
            "json",
        ],
    );
    // A well-formed online retained read stops at the absent native account.
    assert_eq!(online["error"]["code"], "headless_signed_out", "{online}");
}
#[test]
fn retained_preview_mcp_uses_the_same_native_descriptor_and_refusal() {
    let config = tempfile::tempdir().unwrap();
    let expected = cli(
        config.path(),
        &[
            "assets",
            "preview",
            "--project",
            "test-project",
            "--asset",
            "sys:pm_attachment:pma_one",
            "--retain",
            "--offline",
            "--output",
            "json",
        ],
    );
    let mut child = command(config.path())
        .args(["mcp", "serve", "--exposure", "chapters"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        for request in [
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ds_assets","arguments":{"operation":"describe","command":"assets.preview"}}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ds_assets","arguments":{"operation":"invoke","command":"assets.preview","arguments":{"project":"test-project","asset":"sys:pm_attachment:pma_one","retain":true,"offline":true}}}}),
            json!({"jsonrpc":"2.0","id":999,"method":"shutdown"}),
        ] {
            serde_json::to_writer(&mut *stdin, &request).unwrap();
            stdin.write_all(b"\n").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let describe = replies.iter().find(|r| r["id"] == 1).unwrap();
    assert_eq!(
        describe["result"]["structuredContent"]["data"]["command"]["contract"],
        2
    );
    let invoke = replies.iter().find(|r| r["id"] == 2).unwrap();
    assert_eq!(invoke["result"]["structuredContent"], expected);
    assert_eq!(invoke["result"]["isError"], true);
}
