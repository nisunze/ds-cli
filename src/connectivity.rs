//! Local test isolation is not a license or an authorization bypass.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetOfflineMode {
    pub schema: String,
    pub enabled: bool,
    pub principal: String,
    pub capabilities: BTreeMap<String, bool>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct OfflineMode {
    pub schema: &'static str,
    pub enabled: bool,
}

pub fn set_offline_mode(request: SetOfflineMode) -> Result<OfflineMode, String> {
    if request.schema != "ds.connectivity/v1" {
        return Err("Unsupported connectivity command schema.".into());
    }
    if request.enabled
        && (request.principal.is_empty()
            || request.principal.trim() != request.principal
            || !["platform.admin", "app.admin"]
                .iter()
                .any(|key| request.capabilities.get(*key) == Some(&true)))
    {
        return Err("App-level all governance is required to enable offline mode.".into());
    }
    // Reconnection must remain possible after sign-out or cached grants expire.
    // It grants no remote authority: normal server authentication still applies.
    Ok(OfflineMode {
        schema: "ds.connectivity/v1",
        enabled: request.enabled,
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 64 * 1024 {
        return Err("Connectivity command exceeds its bound.".into());
    }
    let request =
        serde_json::from_slice(input).map_err(|e| format!("Invalid connectivity command: {e}"))?;
    serde_json::to_string(&set_offline_mode(request)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_global_governance_can_isolate_and_anyone_can_reconnect() {
        for capability in [
            "project.admin",
            "ui.governance",
            "all",
            "platform.admin",
            "app.admin",
        ] {
            let result = evaluate(
                &serde_json::to_vec(&json!({"schema":"ds.connectivity/v1", "enabled":true,
                "principal":"u", "capabilities":{capability:true}}))
                .unwrap(),
            );
            assert_eq!(
                result.is_ok(),
                matches!(capability, "platform.admin" | "app.admin")
            );
        }
        assert!(evaluate(br#"{"schema":"ds.connectivity/v1","enabled":true,"principal":"","capabilities":{"platform.admin":true}}"#).is_err());
        assert!(evaluate(
            br#"{"schema":"ds.connectivity/v1","enabled":false,"principal":"","capabilities":{}}"#
        )
        .is_ok());
        assert!(evaluate(
            br#"{"schema":"other","enabled":false,"principal":"","capabilities":{}}"#
        )
        .is_err());
        assert!(evaluate(br#"{"schema":"ds.connectivity/v1","enabled":true,"principal":"u","capabilities":{},"force":true}"#).is_err());
    }
}
