//! Two fixed native engine addons; inspection acquires no project data.
use crate::External;
use serde_json::{Value, json};
use std::time::Duration;

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
    let modules: Vec<Value> = [("solar", &SOLAR), ("reporter", &REPORTER)]
        .into_iter()
        .map(|(component, owner)| {
            let mut row = json!({"component":component,"executable":owner.name});
            let result = owner.capture().and_then(|engine| {
                engine
                    .call_json("build-info", &[], Duration::from_secs(20))
                    .map(|identity| (engine.executable.clone(), identity))
            });
            match result {
                Ok((path, identity)) => {
                    let valid = if component == "solar" {
                        identity["name"] == "ds-solar-engine"
                    } else {
                        identity["schema"] == "ds.engine-build/v1"
                            && identity["service"] == "ds-network-reporter"
                            && identity["binary"] == "ds-report"
                    };
                    let capabilities = identity["schemas"].as_array().filter(|schemas| {
                        schemas.len() <= 128
                            && schemas.iter().all(|schema| {
                                schema.as_str().is_some_and(|value| value.len() <= 256)
                            })
                    });
                    let bounded =
                        serde_json::to_vec(&identity).is_ok_and(|bytes| bytes.len() <= 16 * 1024);
                    if valid && capabilities.is_some() && bounded {
                        row["status"] = json!("available");
                        row["path"] = json!(path);
                        row["capabilities"] = identity["schemas"].clone();
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
