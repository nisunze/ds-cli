use super::*;
use serde_json::json;

const NOW: &str = "2026-10-20T12:00:00Z";
const BUILD: &str = "6ed7832b370dc3ffb47f593e67063954de9315dae6d4b456dca49b44797c9cfb";
const BUILT_AT: &str = "2026-10-20T08:06:40.396Z";
const OLD_BUILD: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn deb(folder: &str, source: &str, built: &str, released: &str) -> Value {
    json!({"platform":"linux","arch":"x86_64","kind":"deb",
        "url":format!("https://storage.googleapis.com/releases/desktop/canary/{folder}/DS%20GridDesign%20Canary_0.2.0_amd64.deb"),
        "size_bytes":49254970,"sha256":"c".repeat(64),"source_sha":source,"built_at":built,
        "released_at":released,"source_revisions":{"ds-web":"4".repeat(40)}})
}

/// The canary shell member as Brain publishes it.
fn shell_member(version: &str, released: &str, linux: Value) -> Value {
    json!({"channel":"canary","product_id":"rw.datasolutions.desktop.canary","version":version,
        "released_at":released,"source_sha":"","built_at":"","status":"active","assets":[linux,
        {"platform":"windows","arch":"x86_64","kind":"nsis",
         "url":"https://storage.googleapis.com/releases/desktop/canary/0.2.0-r106/DS%20GridDesign%20Canary.exe",
         "size_bytes":10,"sha256":"d".repeat(64),"source_sha":"7".repeat(64),
         "built_at":"2026-10-15T13:27:50.839Z","released_at":"2026-10-15T14:47:21.077427Z"}]})
}

fn r118() -> Value {
    shell_member(
        "0.2.0-r118",
        "2026-10-20T08:20:57.054388Z",
        deb("0.2.0-r118", BUILD, BUILT_AT, "2026-10-20T08:20:57.054388Z"),
    )
}

fn r119() -> Value {
    shell_member(
        "0.2.0-r119",
        "2026-10-21T09:00:00Z",
        deb(
            "0.2.0-r119",
            &"9".repeat(64),
            "2026-10-21T08:40:00.000Z",
            "2026-10-21T09:00:00Z",
        ),
    )
}

fn desktop() -> Value {
    json!({"version":"0.2.0","sourceSha":BUILD,"builtAt":BUILT_AT})
}

fn shell(catalog: CatalogRead, installed: Value, record: Option<Value>) -> ReleaseIdentityAnswer {
    project(
        Application::Shell,
        "canary",
        ReleaseIdentityInput {
            catalog,
            installed,
            platform: "linux".into(),
            record,
            now: NOW.into(),
        },
    )
    .unwrap()
}

fn identity(version: &str, number: u64, date: &str) -> ReleaseIdentity {
    ReleaseIdentity {
        version: Some(version.into()),
        build_number: Some(number),
        release_date: Some(date.into()),
    }
}

#[test]
fn the_member_that_publishes_the_installed_shell_build_is_recorded_as_its_installation() {
    let answer = shell(CatalogRead::Read(r118()), desktop(), None);
    let expected = identity("0.2.0", 118, "2026-10-20T08:20:57.054388Z");
    assert_eq!(answer.installed, expected);
    assert_eq!(answer.published, Some(expected));
    assert!(answer.update.is_none());
    assert_eq!(answer.installation.source, "catalog");
    assert_eq!(answer.installation.recorded_at.as_deref(), Some(NOW));
    assert_eq!(answer.catalog.state, "read");
    let record = answer.record.expect("a first proof is retained");
    assert_eq!(record.schema, INSTALLATION_SCHEMA);
    assert_eq!(
        (record.channel.as_str(), record.platform.as_str()),
        ("canary", "linux")
    );
    assert_eq!(record.release_version, "0.2.0-r118");
    assert_eq!(record.released_at, "2026-10-20T08:20:57.054388Z");
    assert_eq!(
        record.build,
        BTreeMap::from([
            ("built_at".into(), BUILT_AT.into()),
            ("source_sha".into(), BUILD.into())
        ])
    );
    // The same proof again keeps its first record and time.
    let mut raw = serde_json::to_value(&record).unwrap();
    raw["recorded_at"] = json!("2026-10-20T08:30:00Z");
    let again = shell(CatalogRead::Read(r118()), desktop(), Some(raw));
    assert!(again.record.is_none());
    assert_eq!(
        again.installation.recorded_at.as_deref(),
        Some("2026-10-20T08:30:00Z")
    );
}

fn recorded() -> Value {
    serde_json::to_value(
        shell(CatalogRead::Read(r118()), desktop(), None)
            .record
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn a_recorded_installation_keeps_its_release_after_the_catalog_moves_on_or_fails() {
    let moved = shell(CatalogRead::Read(r119()), desktop(), Some(recorded()));
    assert_eq!(
        moved.installed,
        identity("0.2.0", 118, "2026-10-20T08:20:57.054388Z")
    );
    assert_eq!(
        moved.published,
        Some(identity("0.2.0", 119, "2026-10-21T09:00:00Z"))
    );
    assert_eq!(moved.installation.source, "recorded");
    assert!(moved.record.is_none());
    assert_eq!(
        moved.update.as_ref().map(|u| u.release.version.as_str()),
        Some("0.2.0-r119")
    );
    let offline = shell(
        CatalogRead::Unavailable("module_release_unavailable: timed out".into()),
        desktop(),
        Some(recorded()),
    );
    assert_eq!(offline.installed.build_number, Some(118));
    assert!(offline.published.is_none() && offline.update.is_none());
    assert_eq!(offline.catalog.state, "unavailable");
    assert_eq!(
        offline.catalog.message.as_deref(),
        Some("module_release_unavailable: timed out")
    );
    let retained = shell(CatalogRead::Offline, desktop(), Some(recorded()));
    assert_eq!(
        (retained.installed.build_number, retained.catalog.state),
        (Some(118), "offline")
    );
    // Without a proof only the build's own version is known.
    let unknown = shell(CatalogRead::Unavailable("offline".into()), desktop(), None);
    assert_eq!(
        unknown.installed,
        ReleaseIdentity {
            version: Some("0.2.0".into()),
            build_number: None,
            release_date: None
        }
    );
    assert_eq!(unknown.installation.source, "unrecorded");
}

#[test]
fn a_record_of_another_build_lane_platform_or_shape_is_never_trusted() {
    let newer =
        json!({"version":"0.2.0","sourceSha":"9".repeat(64),"builtAt":"2026-10-21T08:40:00.000Z"});
    let other_build = shell(CatalogRead::Offline, newer, Some(recorded()));
    assert_eq!(other_build.installed.build_number, None);
    assert_eq!(other_build.installation.source, "unrecorded");
    let older =
        json!({"version":"0.2.0","sourceSha":OLD_BUILD,"builtAt":"2026-10-19T20:56:00.000Z"});
    assert!(shell(CatalogRead::Read(r118()), older, Some(recorded()))
        .record
        .is_none());
    for (field, value) in [
        ("channel", json!("stable")),
        ("platform", json!("windows")),
        ("component", json!("core")),
        ("schema", json!("ds.release-installation/v0")),
        ("release_version", json!("0.2.0")),
        ("released_at", json!("yesterday")),
        ("build", json!({"source_sha":BUILD})),
        (
            "build",
            json!({"source_sha":BUILD,"built_at":BUILT_AT,"signer":"x"}),
        ),
        ("note", json!("x")),
    ] {
        let mut record = recorded();
        record[field] = value;
        let answer = shell(CatalogRead::Offline, desktop(), Some(record));
        assert_eq!(answer.installed.build_number, None, "{field}");
    }
    let mut excessive = recorded();
    excessive["recorded_at"] = json!("x".repeat(MAX_RECORD_BYTES));
    assert_eq!(
        shell(CatalogRead::Offline, desktop(), Some(excessive))
            .installed
            .build_number,
        None
    );
    // A record whose release names another version is not this build's.
    let mut crossed = recorded();
    crossed["release_version"] = json!("0.3.0-r1");
    assert_eq!(
        shell(CatalogRead::Offline, desktop(), Some(crossed))
            .installed
            .build_number,
        None
    );
}

#[test]
fn a_carried_platform_asset_keeps_the_release_that_first_published_its_bytes() {
    // Windows published r119 and carried the r118 Linux bytes forward.
    let carried = shell_member(
        "0.2.0-r119",
        "2026-10-21T09:00:00Z",
        deb("0.2.0-r118", BUILD, BUILT_AT, "2026-10-20T08:20:57.054388Z"),
    );
    let answer = shell(CatalogRead::Read(carried.clone()), desktop(), None);
    let r118 = identity("0.2.0", 118, "2026-10-20T08:20:57.054388Z");
    assert_eq!(answer.published, Some(r118.clone()));
    assert_eq!(answer.installed, r118);
    assert_eq!(answer.record.unwrap().release_version, "0.2.0-r118");
    // Windows reads its own carried r106 bytes, not the member's number.
    let windows = project(
        Application::Shell,
        "canary",
        ReleaseIdentityInput {
            catalog: CatalogRead::Read(carried),
            installed: Value::Null,
            platform: "windows".into(),
            record: None,
            now: NOW.into(),
        },
    )
    .unwrap();
    assert_eq!(
        windows.published,
        Some(identity("0.2.0", 106, "2026-10-15T14:47:21.077427Z"))
    );
    // A carried asset without its own date has no known date, so the proof
    // shows its build number and retains nothing incomplete.
    let mut undated = shell_member(
        "0.2.0-r119",
        "2026-10-21T09:00:00Z",
        deb("0.2.0-r118", BUILD, BUILT_AT, ""),
    );
    undated["assets"][0]
        .as_object_mut()
        .unwrap()
        .remove("released_at");
    let answer = shell(CatalogRead::Read(undated), desktop(), None);
    assert_eq!(
        answer.installed,
        ReleaseIdentity {
            version: Some("0.2.0".into()),
            build_number: Some(118),
            release_date: None
        }
    );
    assert_eq!(answer.installation.source, "catalog");
    assert!(answer.record.is_none() && answer.installation.recorded_at.is_none());
    // Carrying a package across a semantic-version change still keeps the
    // package's own publication, with no invented date.
    let mut foreign = r119();
    foreign["assets"][0]["url"] =
        json!("https://storage.googleapis.com/releases/desktop/canary/0.1.9-r7/app.deb");
    foreign["assets"][0]
        .as_object_mut()
        .unwrap()
        .remove("released_at");
    assert_eq!(
        shell(CatalogRead::Read(foreign), desktop(), None).published,
        Some(ReleaseIdentity {
            version: Some("0.1.9".into()),
            build_number: Some(7),
            release_date: None
        })
    );
}

#[test]
fn a_main_application_the_host_cannot_see_is_its_last_proven_build() {
    let answer = shell(CatalogRead::Read(r119()), Value::Null, Some(recorded()));
    assert_eq!(
        answer.installed,
        identity("0.2.0", 118, "2026-10-20T08:20:57.054388Z")
    );
    assert_eq!(
        answer.installation,
        Installation {
            source: "recorded",
            recorded_at: Some(NOW.into())
        }
    );
    assert_eq!(answer.published.unwrap().build_number, Some(119));
    assert!(answer.update.is_some() && answer.record.is_none());
    let current = shell(CatalogRead::Read(r118()), Value::Null, Some(recorded()));
    assert_eq!(
        (current.installation.source, current.record.is_none()),
        ("recorded", true)
    );
    let unseen = shell(CatalogRead::Read(r118()), Value::Null, None);
    assert_eq!(
        unseen.installed,
        ReleaseIdentity {
            version: None,
            build_number: None,
            release_date: None
        }
    );
    assert_eq!(unseen.installation.source, "unrecorded");
    assert!(unseen.update.is_none() && unseen.record.is_none());
}

const CORE_SOURCE: &str = "35a573140141de44a5159ff156ee5810e79cb787";
const KERNEL: &str = "a63e8df37a8acaf66b629d6cbc155fa84b62c3c1";
const NETWORK: &str = "8c4e3024eea453c0aac90d52db4e27e22804757c";

fn core_asset(
    folder: &str,
    platform: &str,
    kind: &str,
    source: &str,
    released: Option<&str>,
) -> Value {
    let mut asset = json!({"platform":platform,"arch":"x86_64","kind":kind,
        "url":format!("https://storage.googleapis.com/releases/core/canary/{folder}/ds-core-canary_0.1.0_amd64.deb"),
        "size_bytes":18938358,"sha256":"3".repeat(64),"source_sha":source,"built_at":"2026-10-20T09:24:04.059Z",
        "source_revisions":{"ds-cli":source,"ds-command-kernel":KERNEL,"ds-network":NETWORK},
        "protocol":"ds.core-release/v1"});
    if let Some(released) = released {
        asset["released_at"] = json!(released);
    }
    asset
}

/// The live independent Core member: the Linux executable carries no own date.
fn core_member(version: &str, source: &str) -> Value {
    let folder = version;
    json!({"schema":"ds.module-release/v1","component":"core","channel":"canary",
        "product_id":"rw.datasolutions.core.canary","version":version,"protocol":"ds.core-release/v1",
        "released_at":"2026-10-20T09:25:21.664916Z","status":"active","assets":[
        core_asset(folder, "linux", "core-deb", source, None),
        core_asset(folder, "linux", "core-artifact-receipt", source, None),
        core_asset("0.1.0-r2", "windows", "core-zip", &"3".repeat(40), Some("2026-10-15T10:30:00Z"))]})
}

fn runtime() -> Value {
    json!({"product":"ds","version":"0.1.0","source_sha":CORE_SOURCE,
        "native_client_core_source_sha":KERNEL,"command_kernel_source_sha":KERNEL,
        "native_client_profile_catalog_sha256":"4".repeat(64),"ds_network_source_sha":NETWORK,
        "ds_network_source_state":"release_pin","dirty":false,"target":"x86_64-unknown-linux-gnu",
        "profile":"release","envelope":1,"executable_sha256":"5".repeat(64)})
}

fn core(
    catalog: CatalogRead,
    installed: Value,
    record: Option<Value>,
    platform: &str,
) -> ReleaseIdentityAnswer {
    project(
        Application::Core,
        "canary",
        ReleaseIdentityInput {
            catalog,
            installed,
            platform: platform.into(),
            record,
            now: NOW.into(),
        },
    )
    .unwrap()
}

#[test]
fn core_records_the_release_of_its_exact_executable() {
    let answer = core(
        CatalogRead::Read(core_member("0.1.0-r26", CORE_SOURCE)),
        runtime(),
        None,
        "linux",
    );
    let r26 = identity("0.1.0", 26, "2026-10-20T09:25:21.664916Z");
    assert_eq!(
        (answer.installed.clone(), answer.published.clone()),
        (r26.clone(), Some(r26.clone()))
    );
    assert_eq!(answer.installation.source, "catalog");
    let record = answer.record.unwrap();
    assert_eq!(record.release_version, "0.1.0-r26");
    assert_eq!(
        record.build.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "command_kernel_source_sha",
            "ds_network_source_sha",
            "executable_sha256",
            "native_client_profile_catalog_sha256",
            "source_sha"
        ]
    );
    // r27 is published: the installed executable is still r26.
    let raw = serde_json::to_value(&record).unwrap();
    let r27 = core_member("0.1.0-r27", &"6".repeat(40));
    let moved = core(
        CatalogRead::Read(r27.clone()),
        runtime(),
        Some(raw.clone()),
        "linux",
    );
    assert_eq!(moved.installed, r26);
    assert_eq!(moved.published.unwrap().build_number, Some(27));
    assert_eq!(moved.installation.source, "recorded");
    assert!(moved.update.is_none());
    // Other bytes from the same sources are not the recorded executable.
    let mut rebuilt = runtime();
    rebuilt["executable_sha256"] = json!("6".repeat(64));
    assert_eq!(
        core(CatalogRead::Read(r27), rebuilt, Some(raw), "linux")
            .installed
            .build_number,
        None
    );
    // Windows reads its own carried executable's publication.
    let windows = core(
        CatalogRead::Read(core_member("0.1.0-r26", CORE_SOURCE)),
        runtime(),
        None,
        "windows",
    );
    assert_eq!(
        windows.published,
        Some(identity("0.1.0", 2, "2026-10-15T10:30:00Z"))
    );
    assert_eq!(
        (windows.installed.build_number, windows.record.is_none()),
        (None, true)
    );
}

#[test]
fn a_debug_dirty_or_crossed_core_build_is_not_the_published_release() {
    let member = || CatalogRead::Read(core_member("0.1.0-r26", CORE_SOURCE));
    for (field, value) in [
        ("profile", json!("debug")),
        ("dirty", json!(true)),
        ("command_kernel_source_sha", json!("7".repeat(40))),
        ("ds_network_source_sha", json!("7".repeat(40))),
    ] {
        let mut installed = runtime();
        installed[field] = value;
        let answer = core(member(), installed, None, "linux");
        assert_eq!(answer.installed.build_number, None, "{field}");
        assert!(answer.record.is_none(), "{field}");
    }
    // An unknown executable digest shows the proof but retains nothing.
    let mut unbound = runtime();
    unbound.as_object_mut().unwrap().remove("executable_sha256");
    let answer = core(member(), unbound, None, "linux");
    assert_eq!(
        (answer.installed.build_number, answer.installation.source),
        (Some(26), "catalog")
    );
    assert!(answer.record.is_none() && answer.installation.recorded_at.is_none());
}

#[test]
fn lane_clock_and_local_development_are_explicit() {
    let input = |catalog, now: &str| ReleaseIdentityInput {
        catalog,
        installed: desktop(),
        platform: "linux".into(),
        record: Some(recorded()),
        now: now.into(),
    };
    assert!(project(Application::Shell, "canary", input(CatalogRead::Local, NOW)).is_err());
    assert!(project(
        Application::Shell,
        "local",
        input(CatalogRead::Offline, NOW)
    )
    .is_err());
    assert!(project(
        Application::Shell,
        "canary",
        input(CatalogRead::Offline, "now")
    )
    .is_err());
    let local = project(Application::Shell, "local", input(CatalogRead::Local, NOW)).unwrap();
    assert_eq!(
        (local.catalog.state, local.installed.build_number),
        ("local", None)
    );
    assert_eq!(Application::parse("core"), Ok(Application::Core));
    assert!(Application::parse("solar").is_err());
    let unpublished = shell(CatalogRead::Read(Value::Null), desktop(), None);
    assert_eq!(
        (unpublished.catalog.state, unpublished.published),
        ("unpublished", None)
    );
}
