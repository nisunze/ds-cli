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
            || engine.capabilities.len() > 32
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
    let modules: Vec<Value> = [
        ("solar", &SOLAR),
        ("reporter", &REPORTER),
        ("network", &NETWORK),
    ]
    .into_iter()
    .filter(|(name, _)| component.is_none_or(|selected| selected.as_str() == *name))
    .map(|(component, owner)| {
        let mut row = json!({"component":component,"executable":owner.name});
        let result = owner.capture().and_then(|engine| {
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
