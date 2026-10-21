use super::*;
use serde_json::json;
fn input() -> InspectionInput {
    serde_json::from_value(json!({"platform":"linux","installed":{"version":"1.0.0","sourceSha":"old","builtAt":"2026-10-01T00:00:00Z"},"release":{"channel":"canary","productId":"rw.datasolutions.desktop.canary","version":"1.0.0-r4","releasedAt":"2026-10-03T00:00:00Z","assets":[{"platform":"linux","arch":"x86_64","kind":"deb","url":"https://example.test/app.deb","sizeBytes":10,"sha256":"a".repeat(64),"sourceSha":"new","builtAt":"2026-10-02T00:00:00Z"}]}})).unwrap()
}
#[test]
fn release_number_is_real_publication_identity_not_build_time() {
    assert_eq!(
        release_identity(Some("1.0.0-r4"), Some("2026-10-03T00:00:00Z")),
        ReleaseIdentity {
            version: Some("1.0.0".into()),
            build_number: Some(4),
            release_date: Some("2026-10-03T00:00:00Z".into())
        }
    );
    assert_eq!(release_identity(Some("1.0.0"), None).build_number, None);
    assert_eq!(
        release_identity(Some("1.0.0-rx"), Some("invalid")).release_date,
        None
    );
}
#[test]
fn inspection_admits_main_update_and_truthful_missing_installed_fields() {
    let result = inspect("shell", "canary", input()).unwrap();
    assert!(result.update.is_some());
    assert_eq!(result.installed.build_number, None);
    assert_eq!(result.installed.release_date, None);
    assert_eq!(result.published.unwrap().build_number, Some(4));
}

#[test]
fn network_inspection_projects_engine_version_without_inventing_publication() {
    let held = InspectionInput {
        release: Value::Null,
        installed: json!({"schema":"ds.engine-build/v1","module":"network", "engineIdentity":{"coreVersion":"1.4.0","builtAt":"2026-10-14T00:00:00Z"}}),
        platform: "linux".into(),
    };
    let result = inspect("network", "local", held).unwrap();
    assert_eq!(result.installed.version.as_deref(), Some("1.4.0"));
    assert_eq!(result.installed.build_number, None);
    assert_eq!(result.installed.release_date, None);
    assert!(result.update.is_none());
    assert!(result.published.is_none());
    let missing = InspectionInput {
        release: Value::Null,
        installed: Value::Null,
        platform: "linux".into(),
    };
    let result = inspect("network", "canary", missing).unwrap();
    assert_eq!(result.installed.version, None);
    assert!(result.update.is_none());
}
#[test]
fn carried_asset_ahead_identity_withdrawal_and_crossed_lane_never_update() {
    for (field, value) in [("status", json!("blocked")), ("version", json!("0.9.0-r8"))] {
        let mut held = input();
        held.release[field] = value;
        assert!(inspect("shell", "canary", held).unwrap().update.is_none());
    }
    let mut held = input();
    held.release["assets"][0]["builtAt"] = json!("2026-09-01T00:00:00Z");
    assert!(inspect("shell", "canary", held).unwrap().update.is_none());
    let mut held = input();
    held.release["productId"] = json!("rw.datasolutions.desktop");
    assert!(inspect("shell", "canary", held).is_err());
    let mut held = input();
    held.release["assets"][0]["sha256"] = json!("wrong");
    assert!(inspect("shell", "canary", held).is_err());
    let mut held = input();
    held.release["assets"][0]["sizeBytes"] = json!(0);
    assert!(inspect("shell", "canary", held).is_err());
    let mut held = input();
    held.release["assets"][0]["arch"] = json!("arm64");
    assert!(inspect("shell", "canary", held).unwrap().update.is_none());
}
#[test]
fn development_acquisition_and_unbounded_metadata_refuse() {
    let request = DownloadRequest {
        version: "1.0.0-r2".into(),
        channel: "canary".into(),
        product_id: "rw.datasolutions.desktop.canary".into(),
        platform: "linux".into(),
        kind: "deb".into(),
        url: "https://example.test/app".into(),
        size_bytes: 1,
        sha256: "a".repeat(64),
    };
    assert!(validate_request(&request, "local", "linux").is_err());
    assert!(validate_request(&request, "stable", "linux").is_err());
    let mut held = input();
    held.installed = json!({"version":"x".repeat(33*1024)});
    assert!(inspect("shell", "canary", held).is_err());
}

#[test]
fn same_version_identity_comparison_preserves_rebuilds_offsets_and_missing_facts() {
    let mut same_source = input();
    same_source.release["assets"][0]["sourceSha"] = json!("old");
    assert!(inspect("shell", "canary", same_source)
        .unwrap()
        .update
        .is_some());
    let mut offset = input();
    offset.installed["builtAt"] = json!("2026-10-02T01:00:00+02:00");
    assert!(inspect("shell", "canary", offset).unwrap().update.is_some());
    for field in ["sourceSha", "builtAt"] {
        let mut missing = input();
        missing.installed[field] = Value::Null;
        assert!(inspect("shell", "canary", missing)
            .unwrap()
            .update
            .is_none());
        let mut missing = input();
        missing.release["assets"][0][field] = json!("");
        assert!(inspect("shell", "canary", missing)
            .unwrap()
            .update
            .is_none());
    }
    let mut malformed = input();
    malformed.release["assets"][0]["builtAt"] = json!("later-ish");
    assert!(inspect("shell", "canary", malformed)
        .unwrap()
        .update
        .is_none());
    let mut newer = input();
    newer.release["version"] = json!("2.0.0-r1");
    newer.installed["sourceSha"] = Value::Null;
    newer.installed["builtAt"] = Value::Null;
    assert!(inspect("shell", "canary", newer).unwrap().update.is_some());
    let mut identical = input();
    identical.release["assets"][0]["sourceSha"] = json!("old");
    identical.release["assets"][0]["builtAt"] = json!("2026-10-01T00:00:00Z");
    assert!(inspect("shell", "canary", identical)
        .unwrap()
        .update
        .is_none());
}

#[test]
fn carry_is_platform_specific_and_legacy_source_fallback_remains_readable() {
    let mut carry = input();
    let mut windows = carry.release["assets"][0].clone();
    windows["platform"] = json!("windows");
    windows["kind"] = json!("nsis");
    windows["sourceSha"] = json!("old");
    windows["builtAt"] = json!("2026-10-01T00:00:00Z");
    carry.release["assets"]
        .as_array_mut()
        .unwrap()
        .push(windows);
    let mut win = carry.clone();
    win.platform = "windows".into();
    assert!(inspect("shell", "canary", win).unwrap().update.is_none());
    let mut newer_pointer = carry.clone();
    newer_pointer.release["version"] = json!("2.0.0-r5");
    newer_pointer.platform = "windows".into();
    assert!(inspect("shell", "canary", newer_pointer)
        .unwrap()
        .update
        .is_none());
    assert!(inspect("shell", "canary", carry).unwrap().update.is_some());
    let mut legacy = input();
    legacy.release["sourceSha"] = json!("new");
    legacy.release["builtAt"] = json!("2026-10-02T00:00:00Z");
    legacy.release["assets"][0]["sourceSha"] = json!("");
    legacy.release["assets"][0]["builtAt"] = json!("");
    assert!(inspect("shell", "canary", legacy).unwrap().update.is_some());
    let mut unsupported = input();
    unsupported.platform = "windows".into();
    assert!(inspect("shell", "canary", unsupported)
        .unwrap()
        .update
        .is_none());
}
#[test]
fn absent_optional_receipt_fields_do_not_hide_known_versions() {
    for missing in [Value::Null, json!("")] {
        let input = InspectionInput {
            release: Value::Null,
            installed: json!({"release_version":missing,"package_version":"1.2.3"}),
            platform: "linux".into(),
        };
        assert_eq!(
            inspect("solar", "local", input)
                .unwrap()
                .installed
                .version
                .as_deref(),
            Some("1.2.3")
        );
    }
    let mut held = input();
    held.installed["releaseVersion"] = json!("");
    assert_eq!(
        inspect("shell", "canary", held)
            .unwrap()
            .installed
            .version
            .as_deref(),
        Some("1.0.0")
    );
}

/// The public catalog's shell member, as Brain publishes it (snake_case).
fn catalog_release() -> Value {
    json!({"channel":"canary","product_id":"rw.datasolutions.desktop.canary","version":"0.2.0-r117",
        "released_at":"2026-10-19T20:58:24Z","source_sha":"","built_at":"","status":"active","assets":[
        {"platform":"linux","arch":"x86_64","kind":"deb","url":"https://example.test/app.deb","size_bytes":10,
         "sha256":"a".repeat(64),"source_sha":"new","built_at":"2026-10-19T20:56:00Z",
         "released_at":"2026-10-19T20:58:24Z","source_revisions":{"ds-web":"b".repeat(40)}},
        {"platform":"windows","arch":"x86_64","kind":"nsis","url":"https://example.test/app.exe","size_bytes":10,
         "sha256":"c".repeat(64),"source_sha":"win","built_at":"2026-10-15T14:40:00Z",
         "released_at":"2026-10-15T14:47:21Z","source_revisions":{"ds-web":"d".repeat(40)}}]})
}
fn shell(installed: Value, release: Value, platform: &str) -> Inspection {
    inspect(
        "shell",
        "canary",
        InspectionInput {
            release,
            installed,
            platform: platform.into(),
        },
    )
    .unwrap()
}
#[test]
fn per_asset_publication_dates_from_the_catalog_and_the_page_shape_are_admitted() {
    let old = json!({"version":"0.2.0","sourceSha":"old","builtAt":"2026-10-18T00:00:00Z"});
    let raw = shell(old.clone(), catalog_release(), "linux");
    assert!(raw.update.is_some());
    // The page's camelCase projection carries the same per-asset date.
    let mut page = input();
    page.release["assets"][0]["releasedAt"] = json!("2026-10-03T06:00:00Z");
    let page = inspect("shell", "canary", page).unwrap();
    assert_eq!(
        page.published.unwrap().release_date.as_deref(),
        Some("2026-10-03T06:00:00Z")
    );
    // A carried Windows asset keeps the date its own bytes were published.
    let windows = shell(old, catalog_release(), "windows").published.unwrap();
    assert_eq!(windows.build_number, Some(117));
    assert_eq!(
        windows.release_date.as_deref(),
        Some("2026-10-15T14:47:21Z")
    );
    let mut unknown = catalog_release();
    unknown["assets"][0]["signature"] = json!("x");
    assert!(inspect(
        "shell",
        "canary",
        InspectionInput {
            release: unknown,
            installed: Value::Null,
            platform: "linux".into(),
        },
    )
    .is_err());
}
#[test]
fn installed_shell_bytes_that_are_the_published_asset_carry_its_publication_identity() {
    let same = json!({"version":"0.2.0","sourceSha":"new","builtAt":"2026-10-19T20:56:00Z"});
    let result = shell(same, catalog_release(), "linux");
    assert!(result.update.is_none());
    assert_eq!(result.installed.build_number, Some(117));
    assert_eq!(
        result.installed.release_date.as_deref(),
        Some("2026-10-19T20:58:24Z")
    );
    let older = json!({"version":"0.2.0","sourceSha":"old","builtAt":"2026-10-18T00:00:00Z"});
    let result = shell(older, catalog_release(), "linux");
    assert_eq!(result.installed.build_number, None);
    assert_eq!(result.installed.release_date, None);
    let unknown_time = json!({"version":"0.2.0","sourceSha":"new","builtAt":null});
    assert_eq!(
        shell(unknown_time, catalog_release(), "linux")
            .installed
            .build_number,
        None
    );
}

/// The independent Core catalog member, as Brain publishes it.
fn core_release() -> Value {
    let asset = |platform: &str, kind: &str, sha: &str, released: &str| {
        json!({"platform":platform,"arch":"x86_64","kind":kind,"url":"https://example.test/core",
            "size_bytes":10,"sha256":"e".repeat(64),"source_sha":sha,"built_at":"2026-10-20T06:02:54Z",
            "released_at":released,"source_revisions":{"ds-cli":sha},"protocol":"ds.core-release/v1"})
    };
    json!({"schema":"ds.module-release/v1","component":"core","channel":"canary",
        "product_id":"rw.datasolutions.core.canary","version":"0.1.0-r23","protocol":"ds.core-release/v1",
        "released_at":"2026-10-20T06:03:58Z","status":"active","assets":[
        asset("linux", "core-deb", &"5".repeat(40), "2026-10-20T06:03:58Z"),
        asset("linux", "core-artifact-receipt", &"5".repeat(40), "2026-10-20T06:03:58Z"),
        asset("windows", "core-zip", &"3".repeat(40), "2026-10-15T10:30:00Z")]})
}
fn core(installed: Value, release: Value, platform: &str) -> Result<Inspection, String> {
    inspect(
        "core",
        "canary",
        InspectionInput {
            release,
            installed,
            platform: platform.into(),
        },
    )
}
#[test]
fn core_projects_its_published_release_and_the_installed_source_proves_its_own() {
    let runtime =
        json!({"product":"ds","version":"0.1.0","envelope":1,"source_sha":"5".repeat(40)});
    let result = core(runtime.clone(), core_release(), "linux").unwrap();
    let published = result.published.clone().unwrap();
    assert_eq!(published.version.as_deref(), Some("0.1.0"));
    assert_eq!(published.build_number, Some(23));
    assert_eq!(
        published.release_date.as_deref(),
        Some("2026-10-20T06:03:58Z")
    );
    assert_eq!(result.installed, published);
    assert!(result.update.is_none());
    assert_eq!(result.release["version"], "0.1.0-r23");
    // Windows reads its own executable's publication, not the Linux receipt.
    let windows = core(runtime.clone(), core_release(), "windows").unwrap();
    assert_eq!(
        windows.published.unwrap().release_date.as_deref(),
        Some("2026-10-15T10:30:00Z")
    );
    assert_eq!(windows.installed.build_number, None);
    let older = json!({"product":"ds","version":"0.1.0","source_sha":"4".repeat(40)});
    let result = core(older, core_release(), "linux").unwrap();
    assert_eq!(result.installed.version.as_deref(), Some("0.1.0"));
    assert_eq!(
        (result.installed.build_number, result.installed.release_date),
        (None, None)
    );
    let offline = core(runtime, Value::Null, "linux").unwrap();
    assert_eq!(offline.installed.version.as_deref(), Some("0.1.0"));
    assert!(offline.published.is_none());
}
#[test]
fn a_crossed_or_malformed_core_release_refuses() {
    let runtime = json!({"version":"0.1.0","source_sha":"5".repeat(40)});
    for (field, value) in [
        ("product_id", json!("rw.datasolutions.core")),
        ("channel", json!("stable")),
        ("component", json!("network")),
        ("protocol", json!("ds.module/v1")),
        ("status", json!("unknown")),
        ("assets", json!([])),
        ("signature", json!("x")),
    ] {
        let mut release = core_release();
        release[field] = value;
        assert!(core(runtime.clone(), release, "linux").is_err(), "{field}");
    }
    let mut release = core_release();
    release["assets"][0]["sha256"] = json!("short");
    assert!(core(runtime.clone(), release, "linux").is_err());
    assert!(inspect(
        "core",
        "local",
        InspectionInput {
            release: core_release(),
            installed: runtime,
            platform: "linux".into(),
        },
    )
    .is_err());
}
