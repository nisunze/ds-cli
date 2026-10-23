//! One availability table. `routing/operations.json` is transport: which
//! gateway path, method and native command an operation uses. Where it also
//! says an operation is `unavailable` or `native` on a placement, that is an
//! availability claim, so it must agree with the capability rows it serves.
//! Every operation that is not plain gateway on both placements is linked here
//! to its rows, and the claim is checked against them:
//!   unavailable on P => no linked row runs in the page on P;
//!   native on P      => every linked row runs on core on P;
//!   gateway on P     => every linked row runs in the page on P.
//! (operations.json is digest-pinned into the generated gateway registry, so
//! the links live beside the check, not in that file.)
use super::*;
use serde_json::Value;

const OPERATIONS: &str = include_str!("../routing/operations.json");

/// Each operation with an availability claim, and the rows it serves.
const LINKS: &[(&str, &[&str])] = &[
    ("auth.device.begin", &["account.connect", "auth.link.begin"]),
    ("auth.device.status", &["auth.link.status"]),
    ("auth.device.complete", &["auth.link.complete"]),
    ("auth.device.refresh", &["account.connect"]),
    ("auth.device.list", &["auth.device.list"]),
    ("auth.device.read", &["auth.device.read"]),
    ("auth.device.revoke", &["auth.device.revoke"]),
    ("solar.compute", &["solar.application", "solar.run"]),
    ("solar.compute_batch", &["solar.application", "solar.portfolio.calculate"]),
    ("solar.report.generate", &["solar.application"]),
    ("solar.reference_unit", &["solar.reference.import", "solar.reference.plan"]),
    ("solar.desktop_snapshot", &["solar.project.sync"]),
];

/// Every operation with its placement resolutions.
fn operations(descriptor: &Value) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    for (domain, spec) in descriptor["domains"].as_object().expect("domains") {
        for (name, operation) in spec["operations"].as_object().expect("operations") {
            found.push((format!("{domain}.{name}"), operation["resolution"].clone()));
        }
    }
    found
}

/// Whether the page itself runs the row on the platform.
fn in_page(capability: &Capability, platform: &str) -> bool {
    match capability.route_on(platform) {
        Some(Route::Supported { executor: Executor::Kernel | Executor::NetworkUnit, .. }) => true,
        Some(Route::Supported { executor: Executor::Core, .. }) => {
            beside_core(platform) && capability.export.is_some()
        }
        _ => false,
    }
}

/// Every disagreement between operations.json and the rows, by name.
fn drift(descriptor: &Value, links: &[(&str, &[&str])]) -> Vec<String> {
    let mut found = Vec::new();
    let operations = operations(descriptor);
    for (id, resolution) in &operations {
        let plain = ["web", "desktop"].iter().all(|p| resolution[*p] == "gateway");
        let linked = links.iter().find(|(op, _)| op == id);
        let Some((_, rows)) = linked else {
            if !plain {
                found.push(format!("{id}: claims availability but names no rows"));
            }
            continue;
        };
        for platform in ["web", "desktop"] {
            let claim = resolution[platform].as_str().unwrap_or_default();
            for row in *rows {
                let Ok(capability) = capability(row) else {
                    found.push(format!("{id}: no row `{row}`"));
                    continue;
                };
                let core = capability.route_on(platform)
                    == Some(Route::Supported { executor: Executor::Core, exchange: None });
                let agrees = match claim {
                    "unavailable" => !in_page(capability, platform),
                    "native" => core,
                    "gateway" => in_page(capability, platform),
                    _ => false,
                };
                if !agrees {
                    found.push(format!("{id} is {claim} on {platform}, but `{row}` disagrees"));
                }
            }
        }
    }
    for (op, _) in links {
        if !operations.iter().any(|(id, _)| id == op) {
            found.push(format!("{op}: no such operation"));
        }
    }
    found
}

#[test]
fn operations_claim_no_availability_their_rows_do_not_declare() {
    let descriptor: Value = serde_json::from_str(OPERATIONS).unwrap();
    assert_eq!(drift(&descriptor, LINKS), Vec::<String>::new());
    assert!(operations(&descriptor).len() > LINKS.len());
}

#[test]
fn a_drifting_operation_or_an_unlinked_claim_fails() {
    let mut descriptor: Value = serde_json::from_str(OPERATIONS).unwrap();
    // The Web cannot reach Solar compute through the gateway: its rows are pending there.
    descriptor["domains"]["solar"]["operations"]["compute"]["resolution"]["web"] = "gateway".into();
    assert_eq!(
        drift(&descriptor, LINKS),
        [
            "solar.compute is gateway on web, but `solar.application` disagrees",
            "solar.compute is gateway on web, but `solar.run` disagrees"
        ]
    );
    let descriptor: Value = serde_json::from_str(OPERATIONS).unwrap();
    let unlinked: Vec<_> = LINKS.iter().copied().filter(|(op, _)| *op != "auth.device.list").collect();
    assert_eq!(drift(&descriptor, &unlinked), ["auth.device.list: claims availability but names no rows"]);
}
