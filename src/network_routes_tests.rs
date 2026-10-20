use super::*;
use serde_json::{json, Value};

const PLATFORMS: [&str; 3] = ["desktop", "web", "core"];

fn committed() -> Value {
    serde_json::from_str(MANIFEST).unwrap()
}

fn refused(manifest: Value) -> String {
    parse(&manifest.to_string()).expect_err("the gate must refuse this manifest")
}

#[test]
fn every_capability_routes_desktop_web_and_core() {
    let manifest = parse(MANIFEST).unwrap();
    let declared: BTreeSet<&str> = manifest.platforms.iter().map(String::as_str).collect();
    assert_eq!(declared, BTreeSet::from(PLATFORMS));
    assert!(!manifest.capabilities.is_empty());
    for (name, capability) in &manifest.capabilities {
        let routed: BTreeSet<&str> = capability.routes.keys().map(String::as_str).collect();
        assert_eq!(routed, declared, "`{name}` must route every platform");
    }
}

#[test]
fn every_route_names_an_executor_that_exists_on_its_platform() {
    let manifest = parse(MANIFEST).unwrap();
    for executor in EXECUTORS {
        assert!(manifest.executors.contains_key(&executor), "{executor:?}");
    }
    for (name, capability) in &manifest.capabilities {
        for (platform, route) in &capability.routes {
            assert!(
                manifest.executors[&route.executor]
                    .platforms
                    .contains(platform),
                "`{name}` on {platform} names {:?}",
                route.executor
            );
        }
    }
}

#[test]
fn no_desktop_route_sends_an_engineering_job_to_the_browser_engine() {
    let manifest = parse(MANIFEST).unwrap();
    let jobs: Vec<_> = manifest
        .capabilities
        .iter()
        .filter(|(_, capability)| capability.kind == CapabilityKind::EngineeringJob)
        .collect();
    assert!(!jobs.is_empty());
    for (name, capability) in jobs {
        assert_eq!(
            capability.routes["desktop"].executor,
            Executor::Core,
            "`{name}` is an engineering job"
        );
    }
}

#[test]
fn each_gate_refuses_a_manifest_that_breaks_it() {
    let mut missing = committed();
    missing["capabilities"]["lv.plan"]["routes"]
        .as_object_mut()
        .unwrap()
        .remove("core");
    assert!(refused(missing).contains("must route every declared platform"));

    let mut extra = committed();
    extra["capabilities"]["lv.plan"]["routes"]["android"] = json!({"executor": "refuse"});
    assert!(refused(extra).contains("must route every declared platform"));

    let mut absent_here = committed();
    absent_here["capabilities"]["lv.plan"]["routes"]["desktop"] =
        json!({"executor": "network_unit"});
    assert!(refused(absent_here).contains("does not exist there"));

    let mut unknown = committed();
    unknown["capabilities"]["lv.plan"]["routes"]["web"] = json!({"executor": "cloud"});
    assert!(refused(unknown).contains("unknown variant `cloud`"));

    let mut undeclared = committed();
    undeclared["executors"]
        .as_object_mut()
        .unwrap()
        .remove("refuse");
    assert!(refused(undeclared).contains("executor \"refuse\" is not declared"));

    let mut stray_platform = committed();
    stray_platform["executors"]["core"]["platforms"] = json!(["desktop", "core", "android"]);
    assert!(refused(stray_platform).contains("undeclared platform `android`"));

    // Even where the unit exists, an engineering job stays on Core.
    let mut job_in_page = committed();
    job_in_page["executors"]["network_unit"]["platforms"] = json!(["web", "desktop"]);
    job_in_page["capabilities"]["lv.process"]["routes"]["desktop"] =
        json!({"executor": "network_unit", "page_lanes": 1});
    assert!(refused(job_in_page).contains("where Core is installed"));

    let mut silent = committed();
    silent["capabilities"]["lv.plan"]["routes"]["web"] = json!({"executor": "refuse"});
    assert!(refused(silent).contains("without a typed code and message"));

    let mut stray_refusal = committed();
    stray_refusal["capabilities"]["lv.plan"]["routes"]["web"] = json!({
        "executor": "network_unit",
        "refusal": {"code": "network_unit_required", "message": "x"},
    });
    assert!(refused(stray_refusal).contains("does not refuse"));

    let mut lanes = committed();
    lanes["capabilities"]["lv.process"]["routes"]["desktop"]["page_lanes"] = json!(0);
    assert!(refused(lanes).contains("page lanes"));

    let mut field = committed();
    field["capabilities"]["lv.process"]["routes"]["core"]["door"] = json!("x");
    assert!(refused(field).contains("unknown field `door`"));
}

#[test]
fn a_typed_refusal_route_is_answered_as_data() {
    let mut manifest = committed();
    manifest["capabilities"]["lv.plan"]["routes"]["web"] = json!({
        "executor": "refuse",
        "refusal": {"code": "network_unit_required", "message": "Install the Network unit."},
    });
    let parsed = parse(&manifest.to_string()).unwrap();
    let route = &parsed.capabilities["lv.plan"].routes["web"];
    assert_eq!(route.executor, Executor::Refuse);
    assert_eq!(
        route.refusal.as_ref().unwrap().code,
        "network_unit_required"
    );
}

#[test]
fn route_answers_each_platform_and_refuses_unknown_names() {
    let desktop = route("lv.process", "desktop").unwrap();
    assert_eq!(desktop.kind, CapabilityKind::EngineeringJob);
    assert_eq!(desktop.route.executor, Executor::Core);
    assert_eq!(desktop.route.page_lanes, Some(1));
    let web = route("lv.process", "web").unwrap();
    assert_eq!(web.route.executor, Executor::NetworkUnit);
    assert_eq!(web.route.page_lanes, None);
    assert_eq!(
        route("lv.plan", "core").unwrap().route.executor,
        Executor::Core
    );
    assert!(route("lv.absent", "desktop")
        .unwrap_err()
        .starts_with("network_route_unknown:"));
    assert!(route("lv.process", "android")
        .unwrap_err()
        .starts_with("network_route_platform_unknown:"));
}

#[test]
fn the_wasm_door_answers_the_native_route() {
    let input = json!({"schema": SCHEMA, "capability": "lv.process", "platform": "desktop"});
    let answer: Value =
        serde_json::from_str(&evaluate(input.to_string().as_bytes()).unwrap()).unwrap();
    assert_eq!(
        answer,
        json!({
            "schema": SCHEMA,
            "capability": "lv.process",
            "platform": "desktop",
            "kind": "engineering_job",
            "executor": "core",
            "page_lanes": 1,
        })
    );
    for bad in [
        json!({"schema": "other", "capability": "lv.process", "platform": "web"}),
        json!({"schema": SCHEMA, "capability": "lv.process", "platform": "web", "host": "x"}),
        json!({"schema": SCHEMA, "capability": "lv.process"}),
    ] {
        assert!(evaluate(bad.to_string().as_bytes())
            .unwrap_err()
            .starts_with("network_route_request_invalid:"));
    }
    assert!(evaluate(&[b' '; MAX_INPUT_BYTES + 1])
        .unwrap_err()
        .starts_with("network_route_request_invalid:"));
}
