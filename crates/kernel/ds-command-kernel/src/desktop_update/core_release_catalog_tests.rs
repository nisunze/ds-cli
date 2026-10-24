use super::*;
use serde_json::json;

fn asset(platform: &str, kind: &str, channel: &str, version: &str) -> Value {
    json!({"platform":platform,"arch":"x86_64","kind":kind,
        "url":format!("https://storage.googleapis.com/release-bucket/core/{channel}/{version}/ds-core-{platform}.pkg"),
        "size_bytes":1234,"sha256":"a".repeat(64),"source_sha":"b".repeat(40),
        "built_at":"2026-10-23T01:00:00Z","released_at":"",
        "source_revisions":{"ds-cli":"b".repeat(40)},"protocol":"ds.core-release/v1"})
}

fn release(channel: &str, version: &str) -> Value {
    json!({"schema":"ds.module-release/v1","component":"core","channel":channel,
        "product_id":format!("rw.datasolutions.core{}",if channel=="canary" {".canary"} else {""}),
        "version":version,"protocol":"ds.core-release/v1","released_at":"2026-10-24T01:00:00Z",
        "status":"active","assets":[asset("linux","core-deb",channel,version),
            asset("windows","core-zip",channel,version),
            asset("windows","core-artifact-receipt",channel,version)]})
}

fn present(channel: &str, platform: &str, read: Value) -> Value {
    let request = json!({"channel":channel,"platform":platform,"read":read});
    serde_json::from_str(
        &core_release_presentation(&serde_json::to_vec(&request).unwrap()).unwrap(),
    )
    .unwrap()
}

#[test]
fn each_platform_uses_only_its_independent_core_package_and_provenance() {
    for channel in ["stable", "canary"] {
        let raw = release(channel, "0.1.0-r8");
        for (platform, kind) in [("linux", "core-deb"), ("windows", "core-zip")] {
            let shown = present(channel, platform, json!({"release":raw}));
            assert_eq!(shown["state"], "available");
            assert_eq!(shown["version"], "0.1.0-r8");
            assert_eq!(shown["releasedAt"], raw["released_at"]);
            assert_eq!(shown["assets"].as_array().unwrap().len(), 1);
            assert_eq!(shown["assets"][0]["kind"], kind);
            assert_eq!(shown["assets"][0]["platform"], platform);
            assert_eq!(shown["assets"][0]["sourceSha"], "b".repeat(40));
            assert_eq!(shown["assets"][0]["sha256"], "a".repeat(64));
            assert_eq!(shown["assets"][0]["sizeBytes"], 1234);
        }
    }
}

#[test]
fn cross_base_carried_core_keeps_its_own_version_and_unknown_date() {
    let mut raw = release("canary", "2.0.0-r42");
    raw["assets"][0] = asset("linux", "core-deb", "canary", "0.1.0-r8");
    let shown = present("canary", "linux", json!({"release":raw}));
    assert_eq!(shown["version"], "0.1.0-r8");
    assert_eq!(shown["releasedAt"], "");
    assert_eq!(shown["assets"][0]["releasedAt"], "");
    raw["assets"][0]["source_revisions"] = json!({});
    assert_eq!(
        present("canary", "linux", json!({"release":raw}))["assets"][0]["sourceRevisions"],
        json!({})
    );
    let (native, _) = inspect(
        "canary",
        InspectionInput {
            release: raw.clone(),
            installed: Value::Null,
            platform: "linux".into(),
        },
    )
    .unwrap();
    let published = native.published.unwrap();
    assert_eq!(published.version.as_deref(), Some("0.1.0"));
    assert_eq!(published.build_number, Some(8));
    assert!(published.release_date.is_none());
    raw["assets"][0]["released_at"] = json!("2026-10-20T02:00:00Z");
    assert_eq!(
        present("canary", "linux", json!({"release":raw}))["releasedAt"],
        "2026-10-20T02:00:00Z"
    );
}

#[test]
fn unavailable_states_are_distinct_and_offer_no_assets() {
    assert_eq!(present("stable", "linux", Value::Null)["state"], "failed");
    assert_eq!(
        present("stable", "linux", json!({"release":null}))["state"],
        "unpublished"
    );
    assert_eq!(
        present("stable", "macos", Value::Null)["state"],
        "unsupported"
    );
    let mut raw = release("stable", "0.1.0-r8");
    raw["assets"].as_array_mut().unwrap().remove(0);
    assert_eq!(
        present("stable", "linux", json!({"release":raw}))["state"],
        "unpublished"
    );
    for state in ["blocked", "stopped", "rolled_back"] {
        let mut raw = release("stable", "0.1.0-r8");
        raw["status"] = json!(state);
        let shown = present("stable", "linux", json!({"release":raw}));
        assert_eq!(shown["state"], "withdrawn");
        assert_eq!(shown["assets"], json!([]));
    }
}

#[test]
fn desktop_metadata_and_unsafe_or_ambiguous_core_assets_are_refused() {
    let mut variants = vec![];
    let mut desktop = release("stable", "0.1.0-r8");
    desktop["component"] = json!("shell");
    desktop["product_id"] = json!("rw.datasolutions.desktop");
    variants.push(desktop);
    variants.push(release("canary", "0.1.0-r8"));
    for url in [
        "http://storage.googleapis.com/release-bucket/core/stable/0.1.0-r8/core.deb",
        "https://user:secret@storage.googleapis.com/release-bucket/core/stable/0.1.0-r8/core.deb",
        "https://storage.googleapis.com/release-bucket/desktop/stable/0.2.0-r91/core.deb",
        "https://storage.googleapis.com/release-bucket/core/canary/0.1.0-r8/core.deb",
        "https://storage.googleapis.com/release-bucket/core/stable/0.1.0-r8/nested/core.deb",
        "https://example.org/core/stable/0.1.0-r8/core.deb",
        "https://storage.googleapis.com:8443/release-bucket/core/stable/0.1.0-r8/core.deb",
        "https://storage.googleapis.com/release-bucket/core/stable/0.1.0-r8/core.deb?download=1",
        "https://storage.googleapis.com/release-bucket/core/stable/0.1.0-r8/core.deb#fragment",
    ] {
        let mut raw = release("stable", "0.1.0-r8");
        raw["assets"][0]["url"] = json!(url);
        variants.push(raw);
    }
    let mut duplicate = release("stable", "0.1.0-r8");
    duplicate["assets"]
        .as_array_mut()
        .unwrap()
        .push(asset("linux", "core-deb", "stable", "0.1.0-r7"));
    variants.push(duplicate);
    for raw in variants {
        let shown = present("stable", "linux", json!({"release":raw}));
        assert_eq!(shown["state"], "malformed");
        assert_eq!(shown["assets"], json!([]));
    }
}

#[test]
fn additive_catalog_members_and_protocol_families_remain_compatible() {
    let mut raw = release("stable", "0.1.0-r8");
    raw["schema"] = json!("ds.module-release/v2");
    raw["protocol"] = json!("ds.core-release/v2");
    raw["future_member"] = json!({"recorded":true});
    for asset in raw["assets"].as_array_mut().unwrap() {
        asset["protocol"] = json!("ds.core-release/v2");
        asset["future_member"] = json!(true);
    }
    assert_eq!(
        present(
            "stable",
            "linux",
            json!({"release":raw,"extra_reply_member":true})
        )["state"],
        "available"
    );
}

#[test]
fn the_presentation_window_is_bounded_and_its_request_stays_closed() {
    assert!(core_release_presentation(&vec![b' '; 128 * 1024 + 1]).is_err());
    for input in [
        json!({"channel":"stable","platform":"linux","read":null,"argv":["anything"]}),
        json!({"channel":"other","platform":"linux","read":null}),
        json!({"channel":"stable","platform":"other","read":null}),
    ] {
        assert!(core_release_presentation(&serde_json::to_vec(&input).unwrap()).is_err());
    }
}
