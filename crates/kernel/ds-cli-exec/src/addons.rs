//! Fixed native engine addons; inspection acquires no project data.
use crate::External;
use serde_json::{Value, json};
use std::time::Duration;

fn capabilities(component: &str, identity: &Value) -> Option<Value> {
    if !serde_json::to_vec(identity).is_ok_and(|bytes| bytes.len() <= 16 * 1024) {
        return None;
    }
    if component == "network" {
        use ds_network_contracts::engine::{ExecutionEnvironment, NetworkBuildInfo, valid_hex};
        let info: NetworkBuildInfo = serde_json::from_value(identity.clone()).ok()?;
        let engine = info.engine_identity;
        let pins = match (&engine.source_sha, &engine.dependency_closure_sha256) {
            (None, None) => true, // Explicit development identity, not installed trust.
            (Some(source), Some(closure)) => valid_hex(source, 40) && valid_hex(closure, 64),
            _ => false,
        };
        if info.schema != "ds.engine-build/v1"
            || info.module != "network"
            || engine.schema != "ds.network-engine/v1"
            || engine.protocol_major != 1
            || engine.environment != ExecutionEnvironment::Native
            || engine.runtime.environment != ExecutionEnvironment::Native
            || engine.core_version.is_empty()
            || engine.core_version.len() > 128
            || engine.runtime.logical_cpus == 0
            || engine.runtime.logical_cpus > 65536
            || engine.runtime.internal_worker_threads == 0
            || engine.runtime.internal_worker_threads > engine.runtime.logical_cpus
            || engine.capabilities.len() > 64
            || !pins
        {
            return None;
        }
        let capabilities: Vec<String> = engine.capabilities.into_iter().map(String::from).collect();
        if capabilities
            .iter()
            .any(|cap| cap.is_empty() || cap.len() > 256)
        {
            return None;
        }
        return Some(json!(capabilities));
    }
    let valid = if component == "solar" {
        identity["name"] == "ds-solar-engine"
    } else {
        identity["schema"] == "ds.engine-build/v1"
            && identity["service"] == "ds-network-reporter"
            && identity["binary"] == "ds-report"
    };
    let schemas = identity["schemas"].as_array().filter(|schemas| {
        schemas.len() <= 128
            && schemas
                .iter()
                .all(|schema| schema.as_str().is_some_and(|value| value.len() <= 256))
    });
    (valid && schemas.is_some()).then(|| identity["schemas"].clone())
}

pub static SOLAR: External = External {
    name: "solar",
    env_override: "DS_SOLAR_ENGINE_BIN",
    owner: "ds-solar",
    remedy: "install the Solar addon, or set DS_SOLAR_ENGINE_BIN to a compatible solar executable",
    missing_code: "solar_engine_missing",
};

pub static REPORTER: External = External {
    name: "reporter",
    env_override: "DS_REPORT_BIN",
    owner: "ds-network-reporter",
    remedy: "install the Reporter addon, or set DS_REPORT_BIN to a compatible reporter executable",
    missing_code: "reporter_engine_missing",
};

/// Run only build-info, once per available addon, on intentional inspection.
/// Missing and incompatible addons remain separate rows, never fallbacks.
pub fn inventory() -> Value {
    inventory_for(None)
}

/// Intentional consumers inspect a declared owner without starting other addons.
pub fn inventory_for(
    component: Option<ds_command_kernel::module_release::ModuleComponent>,
) -> Value {
    inventory_in_context(component, None)
}

/// The installed native host supplies its captured lane. This never chooses
/// a lane from an account/project default or acquires an unused catalog.
pub fn inventory_for_lane(
    component: Option<ds_command_kernel::module_release::ModuleComponent>,
    lane: &str,
) -> Result<Value, ds_core_contracts::Failure> {
    if !matches!(lane, "stable" | "canary") {
        return Err(ds_core_contracts::Failure::invalid(
            "module_lane_invalid",
            "inspect exactly stable or canary",
        ));
    }
    Ok(inventory_in_context(component, Some(lane)))
}
fn inventory_in_context(
    component: Option<ds_command_kernel::module_release::ModuleComponent>,
    lane: Option<&str>,
) -> Value {
    let modules: Vec<Value> = [
        ("solar", &SOLAR),
        ("reporter", &REPORTER),
        ("network", &NETWORK),
    ]
    .into_iter()
    .filter(|(name, _)| component.is_none_or(|selected| selected.as_str() == *name))
    .map(|(component, owner)| {
        let mut row = json!({"component":component,"executable":owner.name});
        let captured = match lane {
            Some(lane) => owner.capture_for_lane(lane),
            None => owner.capture(),
        };
        let result = captured.and_then(|engine| {
            engine
                .call_json("build-info", &[], Duration::from_secs(20))
                .map(|identity| (engine.executable.clone(), identity))
        });
        match result {
            Ok((path, identity)) => {
                if let Some(capabilities) = capabilities(component, &identity) {
                    row["status"] = json!("available");
                    row["path"] = json!(path);
                    row["capabilities"] = capabilities;
                    row["identity"] = identity;
                } else {
                    row["status"] = json!("refused");
                    row["error"] = json!({"code":"callee_contract_mismatch",
                            "message":"the addon returned an invalid or excessive engine identity",
                            "remedy":"install the matching engine addon"});
                }
            }
            Err(error) => {
                row["status"] = json!(if error.code() == owner.missing_code {
                    "missing"
                } else {
                    "refused"
                });
                row["error"] = json!({"code":error.code(),
                        "message":error.to_string(),"remedy":owner.remedy});
            }
        }
        row
    })
    .collect();
    json!({"schema":"ds.module-inventory/v1","modules":modules})
}

/// Fixed independent Network owner; discovery never starts the executable.
pub static NETWORK: External = External {
    name: "ds-network",
    env_override: "DS_NETWORK_ENGINE_BIN",
    owner: "ds-network",
    remedy: "install the Network addon, or set DS_NETWORK_ENGINE_BIN to a compatible ds-network executable",
    missing_code: "network_engine_missing",
};

/// Shared native installer/resolver base; lane selection stays explicit.
pub fn managed_module_base() -> Result<std::path::PathBuf, ds_core_contracts::Failure> {
    crate::managed_modules::module_base()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network_identity(count: usize) -> Value {
        let mut declared = vec!["lv_processing_v1".to_owned()];
        declared.extend((1..count).map(|index| format!("future_capability_{index}")));
        json!({
            "schema": "ds.engine-build/v1",
            "module": "network",
            "engineIdentity": {
                "schema": "ds.network-engine/v1",
                "protocolMajor": 1,
                "protocolMinor": 0,
                "coreVersion": "0.1.0",
                "sourceSha": "a".repeat(40),
                "dependencyClosureSha256": "b".repeat(64),
                "environment": "native",
                "runtime": {
                    "environment": "native",
                    "logicalCpus": 8,
                    "internalWorkerThreads": 2
                },
                "jsonObjectOrder": "sorted",
                "capabilities": declared
            }
        })
    }

    #[test]
    fn network_accepts_33_and_64_capabilities_preserving_additive_tokens() {
        for count in [33, 64] {
            let identity = network_identity(count);
            assert_eq!(
                capabilities("network", &identity),
                Some(identity["engineIdentity"]["capabilities"].clone()),
                "{count} capabilities must remain available"
            );
        }
    }

    #[test]
    fn network_refuses_65_capabilities() {
        assert!(capabilities("network", &network_identity(65)).is_none());
    }

    #[test]
    fn network_refuses_invalid_native_protocol_and_identity_fields() {
        for (pointer, invalid) in [
            ("/schema", json!("ds.engine-build/v2")),
            ("/module", json!("reporter")),
            ("/engineIdentity/schema", json!("ds.network-engine/v2")),
            ("/engineIdentity/protocolMajor", json!(2)),
            ("/engineIdentity/protocolMajor", json!("1")),
            ("/engineIdentity/environment", json!("wasm")),
            ("/engineIdentity/runtime/environment", json!("wasm")),
            ("/engineIdentity/runtime/logicalCpus", json!(0)),
            ("/engineIdentity/runtime/logicalCpus", json!(65537)),
            ("/engineIdentity/runtime/internalWorkerThreads", json!(0)),
            ("/engineIdentity/runtime/internalWorkerThreads", json!(9)),
            ("/engineIdentity/coreVersion", json!("")),
            ("/engineIdentity/coreVersion", json!("v".repeat(129))),
            ("/engineIdentity/sourceSha", Value::Null),
            ("/engineIdentity/sourceSha", json!("g".repeat(40))),
            (
                "/engineIdentity/dependencyClosureSha256",
                json!("g".repeat(64)),
            ),
            ("/engineIdentity/capabilities/0", json!("")),
            ("/engineIdentity/capabilities/0", json!("x".repeat(257))),
        ] {
            let mut identity = network_identity(33);
            *identity.pointer_mut(pointer).unwrap() = invalid.clone();
            assert!(
                capabilities("network", &identity).is_none(),
                "invalid {pointer}: {invalid} must be refused"
            );
        }
    }

    #[test]
    fn network_identity_remains_bounded_to_16_kib() {
        let mut identity = network_identity(64);
        for capability in identity["engineIdentity"]["capabilities"]
            .as_array_mut()
            .unwrap()
        {
            *capability = json!("x".repeat(240));
        }
        let mut remaining = 16 * 1024 - serde_json::to_vec(&identity).unwrap().len();
        for capability in identity["engineIdentity"]["capabilities"]
            .as_array_mut()
            .unwrap()
        {
            let added = remaining.min(16);
            *capability = json!("x".repeat(240 + added));
            remaining -= added;
        }
        assert_eq!(remaining, 0);
        assert_eq!(serde_json::to_vec(&identity).unwrap().len(), 16 * 1024);
        assert!(capabilities("network", &identity).is_some());

        identity["engineIdentity"]["coreVersion"] = json!("0.1.0x");
        assert_eq!(serde_json::to_vec(&identity).unwrap().len(), 16 * 1024 + 1);
        assert!(capabilities("network", &identity).is_none());
    }
}
