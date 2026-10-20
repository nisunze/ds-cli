use super::*;
use serde_json::{json, Value};

/// The build-time compiler itself, so the gates tested here are the ones that
/// fail the build.
#[path = "../build_network_routes.rs"]
#[allow(dead_code)]
mod compiler;

const MANIFEST: &str = include_str!("../routing/network-routes.json");

fn committed() -> Value {
    serde_json::from_str(MANIFEST).unwrap()
}

/// The gate a manifest breaks, as the build reports it.
fn refused(manifest: Value) -> String {
    let text = manifest.to_string();
    let panic = std::panic::catch_unwind(|| compiler::compile(&text))
        .expect_err("the build must refuse this manifest");
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
        .unwrap_or_default()
}

#[test]
fn the_committed_manifest_compiles_and_routes_every_platform() {
    let table = compiler::compile(MANIFEST);
    for capability in CAPABILITIES {
        assert!(table.contains(&format!("{:?}", capability.name)));
        for platform in [Platform::Desktop, Platform::Web, Platform::Core] {
            let route = capability.route(platform);
            assert_eq!(route.executor == Executor::Refuse, route.refusal.is_some());
        }
    }
}

#[test]
fn engineering_jobs_never_reach_a_browser_engine_and_the_web_refuses_processing() {
    for capability in CAPABILITIES {
        if capability.kind == CapabilityKind::EngineeringJob {
            assert_eq!(capability.route(Platform::Desktop).executor, Executor::Core);
            assert_eq!(capability.route(Platform::Core).executor, Executor::Core);
        }
    }
    for name in ["lv.process", "lv.plan", "process.catalog"] {
        let web = capability(name).unwrap().route(Platform::Web);
        assert_eq!(web.executor, Executor::Refuse);
        assert_eq!(web.refusal.unwrap().code, "core_required");
    }
}

#[test]
fn core_routes_name_the_command_or_door_that_serves_them() {
    let plan = capability("lv.plan").unwrap().core.unwrap();
    assert_eq!(plan.command, Some("design.autoprocess.plan"));
    assert_eq!((plan.input, plan.wrap, plan.answer), (Some("changes"), Some("differential_scope"), Some("differential_scope")));
    let catalog = capability("process.catalog").unwrap().core.unwrap();
    assert_eq!(catalog.switches, &["catalog"]);
    assert_eq!(catalog.answer, Some("catalog"));
    let process = capability("lv.process").unwrap().core.unwrap();
    assert_eq!(process.door, Some("network_lv_process"));
    assert_eq!(capability("lv.process").unwrap().route(Platform::Desktop).page_lanes, Some(1));
}

#[test]
fn this_build_answers_its_own_platform_without_being_told() {
    let answer = route("lv.process").unwrap();
    assert_eq!(answer.platform, PLATFORM);
    assert_eq!(answer.route, capability("lv.process").unwrap().route(PLATFORM));
    let wire: Value = serde_json::from_str(
        &evaluate(json!({"schema": SCHEMA, "capability": "lv.plan"}).to_string().as_bytes()).unwrap(),
    )
    .unwrap();
    assert_eq!(wire["capability"], "lv.plan");
    assert!(evaluate(json!({"schema": SCHEMA, "capability": "lv.plan", "platform": "web"}).to_string().as_bytes()).is_err());
    assert!(route("nope").unwrap_err().starts_with("network_route_unknown"));
}

#[test]
fn each_gate_refuses_a_manifest_that_breaks_it() {
    let mut missing = committed();
    missing["capabilities"]["lv.plan"]["routes"].as_object_mut().unwrap().remove("core");
    assert!(refused(missing).contains("must route every declared platform"));

    let mut extra = committed();
    extra["capabilities"]["lv.plan"]["routes"]["android"] = json!({"executor": "refuse"});
    assert!(refused(extra).contains("must route every declared platform"));

    let mut absent_here = committed();
    absent_here["capabilities"]["lv.plan"]["routes"]["desktop"] = json!({"executor": "network_unit"});
    assert!(refused(absent_here).contains("does not exist there"));

    let mut unknown = committed();
    unknown["capabilities"]["lv.plan"]["routes"]["web"] = json!({"executor": "cloud"});
    assert!(refused(unknown).contains("unknown executor `cloud`"));

    let mut undeclared = committed();
    undeclared["executors"].as_object_mut().unwrap().remove("refuse");
    assert!(refused(undeclared).contains("executor refuse is not declared"));

    let mut stray_platform = committed();
    stray_platform["executors"]["core"]["platforms"] = json!(["desktop", "core", "android"]);
    assert!(refused(stray_platform).contains("undeclared platform `android`"));

    let mut job_in_page = committed();
    job_in_page["executors"]["network_unit"]["platforms"] = json!(["web", "desktop"]);
    job_in_page["capabilities"]["lv.process"]["routes"]["desktop"] = json!({"executor": "network_unit", "page_lanes": 1});
    assert!(refused(job_in_page).contains("where Core is installed"));

    let mut silent = committed();
    silent["capabilities"]["lv.plan"]["routes"]["web"] = json!({"executor": "refuse"});
    assert!(refused(silent).contains("without a typed code and message"));

    let mut stray_refusal = committed();
    stray_refusal["capabilities"]["lv.plan"]["routes"]["desktop"] = json!({
        "executor": "core", "refusal": {"code": "x", "message": "x"}});
    assert!(refused(stray_refusal).contains("does not refuse"));

    let mut lanes = committed();
    lanes["capabilities"]["lv.process"]["routes"]["desktop"]["page_lanes"] = json!(0);
    assert!(refused(lanes).contains("page lanes"));

    let mut field = committed();
    field["capabilities"]["lv.process"]["routes"]["core"]["door"] = json!("x");
    assert!(refused(field).contains("unknown member `door`"));

    let mut doorless = committed();
    doorless["capabilities"]["lv.plan"].as_object_mut().unwrap().remove("core");
    assert!(refused(doorless).contains("names no Core command or door"));

    let mut both = committed();
    both["capabilities"]["lv.plan"]["core"]["door"] = json!("network_lv_plan");
    assert!(refused(both).contains("exactly one Core command or one door"));
}
