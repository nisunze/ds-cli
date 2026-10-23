use super::*;
use serde_json::{json, Value};

/// The build-time compiler itself, so the gates tested here are the ones that
/// fail the build.
#[path = "../build_capabilities.rs"]
#[allow(dead_code)]
mod compiler;

const MANIFEST: &str = include_str!("../routing/capabilities.json");
const CARGO_TOML: &str = include_str!("../Cargo.toml");

fn exchange(name: &str) -> bool {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    src.join("gateway_exchange").join(format!("{name}.rs")).is_file()
        || src.join(format!("{name}.rs")).is_file()
}

fn declared(feature: &str) -> bool {
    CARGO_TOML.contains(&format!("\n{feature} = "))
}

/// The crate's wasm-bindgen exports, scanned the way build.rs scans them.
fn crate_exports() -> &'static std::collections::BTreeMap<String, Vec<String>> {
    static EXPORTS: std::sync::OnceLock<std::collections::BTreeMap<String, Vec<String>>> =
        std::sync::OnceLock::new();
    EXPORTS.get_or_init(|| {
        compiler::crate_exports(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
    })
}

fn export(name: &str) -> Vec<String> {
    crate_exports().get(name).cloned().unwrap_or_default()
}

/// Compile `text` as a build enabling `features`.
fn compile_with(text: &str, features: &[&str]) -> String {
    let enabled = |feature: &str| features.contains(&feature);
    compiler::compile(
        text,
        &compiler::Environment {
            exchange: &exchange,
            export: &export,
            feature_declared: &declared,
            feature_enabled: &enabled,
        },
    )
}

fn committed() -> Value {
    serde_json::from_str(MANIFEST).unwrap()
}

/// The canonical text of an edited manifest, so a test meets the gate it aims at.
fn canonical(manifest: &Value) -> String {
    compiler::render(&compiler::parse(&manifest.to_string()))
}

/// The gate a manifest breaks, as the build reports it.
fn refusal(build: impl FnOnce() -> String + std::panic::UnwindSafe) -> String {
    let panic = std::panic::catch_unwind(build).expect_err("the build must refuse this manifest");
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
        .unwrap_or_default()
}

fn refused_text(text: String, features: &[&str]) -> String {
    refusal(|| compile_with(&text, features))
}

/// An edited manifest, canonical so each case meets the gate it aims at.
fn refused(manifest: Value) -> String {
    refusal(|| compile_with(&canonical(&manifest), &[]))
}

fn row<'a>(manifest: &'a mut Value, id: &str) -> &'a mut Value {
    &mut manifest["capabilities"][id]
}

#[test]
fn the_committed_manifest_is_canonical_and_compiles_for_every_build() {
    assert_eq!(canonical(&committed()), MANIFEST);
    for features in [
        &[][..],
        &["platform-core"][..],
        &["platform-desktop"][..],
        &["platform-web"][..],
    ] {
        assert!(compile_with(MANIFEST, features).contains("pub static CAPABILITIES"));
    }
    assert!(
        compile_with(MANIFEST, &["platform-web"]).contains("pub const PLATFORM: &str = \"web\";")
    );
}

#[test]
fn every_row_answers_every_platform_and_is_found_by_its_id() {
    assert_eq!(
        PLATFORMS.len(),
        committed()["platforms"].as_array().unwrap().len()
    );
    assert!(CAPABILITIES.windows(2).all(|pair| pair[0].id < pair[1].id));
    for row in CAPABILITIES {
        assert_eq!(capability(row.id).unwrap().id, row.id);
        for platform in PLATFORMS {
            let route = row.route_on(platform).unwrap();
            assert_eq!(
                route.refusal().is_some(),
                route.status() != Status::Supported
            );
        }
    }
    assert!(CAPABILITIES[0].route_on("plan9").is_none());
}

#[test]
fn this_build_answers_its_own_platform_without_being_told() {
    let answer = route("design.lv.process").unwrap();
    assert_eq!(answer.platform, PLATFORM);
    assert_eq!(answer.schema, SCHEMA);
    assert!(route("nope.never").unwrap_err().starts_with(CODE_UNKNOWN));
    // The default build is the headless Core: every Core command runs there.
    if PLATFORM == "cli" {
        assert_eq!(answer.status, Status::Supported);
        assert_eq!(answer.executor, Some(Executor::Core));
        assert_eq!(answer.page_lanes, Some(1));
        assert!(on_core("pm.task.create").is_ok());
        let photo = on_core("survey.photo.capture").unwrap_err();
        assert!(
            photo.starts_with("capability_discarded: Survey capture lives on Web and Android."),
            "{photo}"
        );
    }
    let wire = serde_json::to_value(route("design.lv.process").unwrap()).unwrap();
    assert_eq!(wire["capability"], "design.lv.process");
    assert!(wire.get("refusal").is_none() || wire["status"] != "supported");
}

#[test]
fn the_process_path_calls_core_the_way_the_page_did() {
    let process = capability("design.lv.process").unwrap().run.unwrap();
    assert_eq!(
        (process.input, process.output, process.page_lanes),
        (Some("input"), Some("out"), Some(1))
    );
    let plan = capability("design.autoprocess.plan").unwrap().run.unwrap();
    assert_eq!(
        (plan.input, plan.wrap, plan.answer),
        (
            Some("changes"),
            Some("differential_scope"),
            Some("differential_scope")
        )
    );
    let settings = capability("design.process.settings").unwrap().run.unwrap();
    assert_eq!(
        (settings.switches, settings.answer),
        (&["catalog"][..], Some("catalog"))
    );
    for id in [
        "design.lv.process",
        "design.autoprocess.plan",
        "design.process.settings",
    ] {
        let capability = capability(id).unwrap();
        assert_eq!(
            capability.route_on("desktop"),
            Some(Route::Supported {
                executor: Executor::Core,
                exchange: None
            })
        );
        assert_eq!(
            capability.route_on("web").unwrap().status(),
            Status::Pending
        );
    }
}

#[test]
fn owner_rulings_hold_in_the_table() {
    let on = |id: &str, platform: &str| capability(id).unwrap().route_on(platform).unwrap();
    // Survey entries: a create is the same direct Firestore write as its author
    // on every platform that offers it (the page's Kernel survey queue on the
    // Web and the Desktop, Core's own writer on cli); only changes go through
    // Brain's mutate exchange. No mobile app is offered or planned (owner,
    // 2026-10-22): android stays declared, and refuses.
    assert_eq!(
        on("survey.entries.create", "web"),
        Route::Supported {
            executor: Executor::Kernel,
            exchange: Some("survey_create")
        }
    );
    assert_eq!(
        on("survey.entries.create", "desktop"),
        on("survey.entries.create", "web")
    );
    assert_eq!(
        on("survey.entries.create", "cli"),
        Route::Supported {
            executor: Executor::Core,
            exchange: None
        }
    );
    assert_eq!(
        on("survey.entries.create", "android").status(),
        Status::Discarded
    );
    for id in [
        "survey.entries.update",
        "survey.entries.delete",
        "survey.entries.restore",
    ] {
        for platform in ["web", "desktop"] {
            assert_eq!(
                on(id, platform),
                Route::Supported {
                    executor: Executor::Kernel,
                    exchange: Some("survey")
                },
                "{id} on {platform}"
            );
        }
        assert_eq!(on(id, "android").status(), Status::Discarded, "{id}");
    }
    // Photo capture lives on Web and Android.
    let photo = "survey.photo.capture";
    assert_eq!(on(photo, "web").status(), Status::Supported);
    assert_eq!(on(photo, "android").status(), Status::Discarded);
    for platform in ["cli", "desktop", "ios", "macos"] {
        assert_eq!(
            on(photo, platform).refusal().unwrap().reason,
            "survey_capture_web_android"
        );
    }
    // A capability Core does not carry is never supported on cli.
    assert_eq!(
        on("survey.photo.capture", "cli").status(),
        Status::Discarded
    );
    assert_eq!(
        on("survey.entries.restore", "cli").status(),
        Status::Pending
    );
    // Printing is Desktop and Core only.
    assert_eq!(
        on("report.layout.render", "web").refusal().unwrap().reason,
        "printing_on_core"
    );
    assert_eq!(
        on("report.layout.render", "desktop").status(),
        Status::Supported
    );
    // Solar waits for Web parity.
    assert_eq!(
        on("solar.run", "web").refusal().unwrap().reason,
        "solar_web_parity"
    );
    // Platforms without a Kernel build only plan or refuse.
    for capability in CAPABILITIES {
        for platform in ["android", "ios", "macos"] {
            assert_ne!(
                capability.route_on(platform).unwrap().status(),
                Status::Supported
            );
        }
    }
}

#[test]
fn a_new_platform_is_filled_pending_by_the_generator() {
    let mut manifest = committed();
    manifest["platforms"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "harmony", "doc": "A test platform.", "executors": []}));
    let message = refused(manifest.clone());
    assert!(
        message.contains("rows on `harmony`") && message.contains("DS_CAPABILITIES_REWRITE=1"),
        "{message}"
    );
    let mut parsed = compiler::parse(&manifest.to_string());
    compiler::fill(&mut parsed);
    let filled = compiler::render(&parsed);
    let table = compile_with(&filled, &[]);
    assert!(table.contains("\"harmony\""));
    let filled: Value = serde_json::from_str(&filled).unwrap();
    assert_eq!(
        filled["capabilities"]["pm.task.create"]["harmony"],
        "pending:harmony_not_yet_supported"
    );
    assert_eq!(
        filled["reasons"]["harmony_not_yet_supported"],
        "Not yet supported on harmony."
    );
}

#[test]
fn each_gate_refuses_a_manifest_that_breaks_it() {
    let mut text = MANIFEST.replacen("\"cli\": \"core\"", "\"cli\":  \"core\"", 1);
    assert!(refused_text(text.clone(), &[]).contains("canonical form"));
    text = MANIFEST.to_owned();
    assert!(
        refused_text(text, &["platform-desktop", "platform-web"]).contains("serves one platform")
    );

    let mut stray = committed();
    row(&mut stray, "pm.task.create")["vision"] = json!("core");
    assert!(refused(stray).contains("undeclared platform `vision`"));

    let mut absent_here = committed();
    row(&mut absent_here, "pm.task.create")["web"] = json!("core");
    assert!(refused(absent_here).contains("does not exist on web"));

    let mut nowhere = committed();
    row(&mut nowhere, "pm.task.create")["android"] = json!("core");
    assert!(refused(nowhere).contains("does not exist on android"));

    let mut unknown = committed();
    row(&mut unknown, "pm.task.create")["web"] = json!("cloud");
    assert!(refused(unknown).contains("unknown value `cloud`"));

    let mut no_exchange = committed();
    row(&mut no_exchange, "pm.task.create")["web"] = json!("kernel:nowhere");
    assert!(refused(no_exchange).contains("src/gateway_exchange does not have"));

    let mut bare_kernel = committed();
    row(&mut bare_kernel, "pm.task.create")["web"] = json!("kernel");
    assert!(refused(bare_kernel).contains("names no Kernel exchange"));

    let mut silent = committed();
    row(&mut silent, "pm.task.create")["web"] = json!("pending");
    assert!(refused(silent).contains("without a reason"));

    let mut undeclared = committed();
    row(&mut undeclared, "pm.task.create")["web"] = json!("pending:because");
    assert!(refused(undeclared).contains("undeclared reason `because`"));

    let mut unused = committed();
    unused["reasons"]["spare"] = json!("Nobody says this.");
    assert!(refused(unused).contains("`spare` is declared but no row names it"));

    let mut no_default = committed();
    no_default["platforms"][0]
        .as_object_mut()
        .unwrap()
        .remove("default");
    assert!(refused(no_default).contains("exactly one platform is the default"));

    let mut ghost_feature = committed();
    ghost_feature["platforms"][3]["feature"] = json!("platform-android");
    assert!(refused(ghost_feature).contains("has a Kernel build but no executor"));

    let mut undeclared_feature = committed();
    undeclared_feature["platforms"][1]["feature"] = json!("platform-tv");
    assert!(refused(undeclared_feature).contains("Cargo.toml does not declare"));

    let mut stray_executor = committed();
    stray_executor["platforms"][2]["executors"] = json!(["kernel", "cloud"]);
    assert!(refused(stray_executor).contains("unknown executor `cloud`"));

    let mut run_off_core = committed();
    row(&mut run_off_core, "survey.photo.capture")["run"] = json!({"answer": "x"});
    assert!(refused(run_off_core).contains("no platform runs it on core"));

    let mut file_and_member = committed();
    row(&mut file_and_member, "design.lv.process")["run"]["answer"] = json!("jobs");
    assert!(refused(file_and_member).contains("answers the file Core writes or a data member"));

    let mut loose_wrap = committed();
    row(&mut loose_wrap, "design.process.settings")["run"]["wrap"] = json!("scope");
    assert!(refused(loose_wrap).contains("wraps an input it does not name"));

    let mut lanes = committed();
    row(&mut lanes, "design.lv.process")["run"]["page_lanes"] = json!(0);
    assert!(refused(lanes).contains("page lanes"));

    let mut member = committed();
    row(&mut member, "design.lv.process")["run"]["retries"] = json!(3);
    assert!(refused(member).contains("unknown member `retries`"));

    let mut shouting = committed();
    shouting["capabilities"]["PM.Task"] = row(&mut committed(), "pm.task.create").clone();
    assert!(refused(shouting).contains("not a lowercase identifier"));
}
