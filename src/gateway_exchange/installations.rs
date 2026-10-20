//! Installation governance: pages of the installations Brain has seen, one
//! installation's history, and the two statements an operator makes about
//! one — its device and licence policy, and whether it is retired. Brain
//! decides who may read or change them and fences each change by revision;
//! `installation_inventory` projects what a page means.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/api/v1/governance";
/// Brain's own page bound.
pub const PAGE_LIMIT: u32 = 50;
const ID_MAX_CHARS: usize = 128;
const CURSOR_MAX_CHARS: usize = 1_024;
const REASON_MAX_CHARS: usize = 1_000;
const PAGE_CAP: u64 = 8 * 1024 * 1024;
pub const INVALID: &str = "installations_answer_invalid";
pub const INPUT_INVALID: &str = "installations_input_invalid";

/// One installation-governance command, exactly as Brain's schema reads it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum InstallCommand {
    /// One page of installations after `cursor`.
    ListInstalls {
        #[serde(default)]
        cursor: String,
        limit: u32,
    },
    /// One installation with a page of its users and its policy history.
    GetInstall {
        install_id: String,
        #[serde(default)]
        user_cursor: String,
        #[serde(default)]
        event_cursor: String,
        limit: u32,
    },
    /// Block or allow one installation's device and licence.
    SetInstallPolicy {
        install_id: String,
        expected_revision: u64,
        device_status: String,
        license_status: String,
        reason: String,
    },
    /// Record that one installation is gone, or back.
    SetInstallRetirement {
        install_id: String,
        expected_revision: u64,
        retired: bool,
        reason: String,
    },
}

fn text(value: &str, max: usize, required: bool) -> bool {
    (!required || !value.trim().is_empty())
        && value.chars().count() <= max
        && !value.chars().any(char::is_control)
}

fn status(value: &str) -> bool {
    matches!(value, "active" | "blocked")
}

impl InstallCommand {
    fn valid(&self) -> bool {
        match self {
            Self::ListInstalls { cursor, limit } => {
                text(cursor, CURSOR_MAX_CHARS, false) && (1..=PAGE_LIMIT).contains(limit)
            }
            Self::GetInstall {
                install_id,
                user_cursor,
                event_cursor,
                limit,
            } => {
                text(install_id, ID_MAX_CHARS, true)
                    && text(user_cursor, CURSOR_MAX_CHARS, false)
                    && text(event_cursor, CURSOR_MAX_CHARS, false)
                    && (1..=PAGE_LIMIT).contains(limit)
            }
            Self::SetInstallPolicy {
                install_id,
                device_status,
                license_status,
                reason,
                ..
            } => {
                text(install_id, ID_MAX_CHARS, true)
                    && status(device_status)
                    && status(license_status)
                    && text(reason, REASON_MAX_CHARS, true)
            }
            Self::SetInstallRetirement {
                install_id, reason, ..
            } => text(install_id, ID_MAX_CHARS, true) && text(reason, REASON_MAX_CHARS, true),
        }
    }
}

/// One governance command and Brain's answer to it, as served.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installations {
    pub command: InstallCommand,
}

impl Exchange for Installations {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        if !self.command.valid() {
            return Err(Refusal::new(
                INPUT_INVALID,
                "The installation command is outside its bounds.",
            ));
        }
        let Value::Object(mut fields) = serde_json::to_value(&self.command)
            .map_err(|_| Refusal::new(INPUT_INVALID, "unreadable command"))?
        else {
            return Err(Refusal::new(INPUT_INVALID, "unreadable command"));
        };
        let action = fields.remove("action").unwrap_or(Value::Null);
        let body = json!({"action": action, "data": Value::Object(fields)});
        Ok(Request::post(PATH, body).response_cap(PAGE_CAP).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer: &Map<String, Value> = data(&body).as_object().ok_or_else(|| {
            Refusal::new(
                INVALID,
                "The installation service answered an invalid result.",
            )
        })?;
        let expected = match self.command {
            InstallCommand::ListInstalls { .. } => "installs",
            InstallCommand::GetInstall { .. } => "install",
            _ => "install_id",
        };
        if !answer.contains_key(expected) {
            return Err(Refusal::new(
                INVALID,
                "The installation service answered an invalid result.",
            ));
        }
        Ok(Admitted::Done(Value::Object(answer.clone())))
    }
}

/// The typed browser export Svelte calls; it runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{InstallCommand, Installations};
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// One installation-governance command; answers Brain's result as served.
    #[wasm_bindgen(js_name = installationsCommand, unchecked_return_type = "Record<string, unknown>")]
    pub async fn installations_command(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "InstallCommand")] command: JsValue,
    ) -> Result<JsValue, JsValue> {
        let command: InstallCommand = tsify::serde_wasm_bindgen::from_value(command)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        typed(&run(&host, Installations { command }).await?)
    }
}

#[cfg(test)]
#[path = "installations_tests.rs"]
mod tests;
