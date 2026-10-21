use super::*;
use serde_json::json;

#[test]
fn a_release_reads_from_the_flat_wire_or_the_envelope() {
    assert_eq!(release_member(&json!({"release": {"version": "1.2.3"}})), Some(json!({"version": "1.2.3"})));
    assert_eq!(release_member(&json!({"success": true, "data": {"release": null}})), Some(json!(null)));
    assert_eq!(release_member(&json!({"success": false, "release": {"version": "1"}})), None);
    assert_eq!(release_member(&json!({"data": {"other": 1}})), None);
    assert_eq!(release_member(&json!([1, 2])), None);
}

fn manager_unit_fixture(platform: &str) -> Value {
    let names: &[&str] = if platform == "windows" {
        &["package.receipt.json", "ds-install-manager.ps1", "install-ds-core-windows.ps1"]
    } else {
        &["package.receipt.json", "ds-install-manager.sh", "install-core.sh", "install-desktop.sh"]
    };
    let receipt = "a".repeat(64);
    let extension = if platform == "windows" { "ps1" } else { "sh" };
    json!({
        "arch": "x86_64",
        "package_receipt_sha256": receipt,
        "source_revisions": {"ds-cli": "b".repeat(40), "ds-web": "c".repeat(40)},
        "assets": names.iter().map(|name| json!({
            "filename": name, "size_bytes": 100, "sha256": receipt,
            "path": format!("/installation-manager/{receipt}/{name}"),
        })).collect::<Vec<_>>(),
        "bootstrap": {
            "filename": format!("install-manager.{extension}"), "sha256": "d".repeat(64), "size_bytes": 100,
            "path": format!("/installation-manager/{}/install-manager.{extension}", "d".repeat(64)),
        },
    })
}

fn manager_fixture() -> Value {
    json!({
        "schema": "ds.installation-manager-publication/v1",
        "channel": "canary",
        "product_id": "rw.datasolutions.installation-manager.canary",
        "published_at": "2026-10-15T11:00:00Z",
        "platforms": {"windows": manager_unit_fixture("windows"), "linux": manager_unit_fixture("linux")},
    })
}

#[test]
fn the_manager_publication_is_admitted_only_complete_and_for_its_own_lane() {
    let publication = manager_fixture();
    assert_eq!(installation_manager(&publication, "canary"), Some(publication.clone()));
    assert_eq!(installation_manager(&publication, "stable"), None);
    assert_eq!(installation_manager(&publication, "local"), None);
    let mutations: [fn(&mut Value); 9] = [
        |m| { m["platforms"]["windows"]["assets"].as_array_mut().unwrap().pop(); },
        |m| { let first = m["platforms"]["linux"]["assets"][0].clone(); m["platforms"]["linux"]["assets"].as_array_mut().unwrap().push(first); },
        |m| m["platforms"]["windows"]["assets"][0]["path"] = json!("https://other.invalid/pkg"),
        |m| m["platforms"]["windows"]["bootstrap"]["path"] = json!("/installation-manager/latest/install-manager.ps1"),
        |m| m["platforms"]["linux"]["source_revisions"]["ds-cli"] = json!("HEAD"),
        |m| { let linux = m["platforms"]["linux"].clone(); m["platforms"]["macos"] = linux; },
        |m| m["platforms"]["linux"]["assets"][0]["size_bytes"] = json!(65537),
        |m| m["published_at"] = json!("yesterday"),
        |m| m["platforms"]["linux"]["arch"] = json!("aarch64"),
    ];
    for mutate in mutations {
        let mut publication = manager_fixture();
        mutate(&mut publication);
        assert_eq!(installation_manager(&publication, "canary"), None, "{publication}");
    }
}
