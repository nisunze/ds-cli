//! Typed status metadata and the private-file projection receipt.
//!
//! Hosts retain private-file creation/reading and current-identity checks. This
//! owner admits the shared response fields, source state and exact byte fence.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::Observation;

pub const STATUS_FILE_SCHEMA: &str = "ds.design-status-file/v1";
pub const MAX_STATUS_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum StatusObservationSource {
    Held,
    Refresh,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum StatusObservationState {
    Ready,
    Held,
}

/// Metadata about these retained bytes, never an assertion of remote freshness.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(missing_as_null))]
pub struct StatusObservation {
    pub source: StatusObservationSource,
    pub state: StatusObservationState,
    pub observed_at_ms: u64,
    pub sha256: String,
    pub last_error: Option<String>,
}
impl StatusObservation {
    pub fn validate(&self) -> Result<(), String> {
        if self.observed_at_ms > MAX_SAFE_INTEGER
            || !crate::report_export::exact_lower_sha256(&self.sha256)
            || self
                .last_error
                .as_ref()
                .is_some_and(|error| error.len() > 256)
            || (self.state == StatusObservationState::Ready && self.last_error.is_some())
            || (self.state == StatusObservationState::Held
                && self.source == StatusObservationSource::Refresh)
        {
            return Err("malformed observation state".into());
        }
        Ok(())
    }
}

/// Native and browser reads construct exactly the same metadata vocabulary.
pub fn status_observation(
    held: &Observation,
    refreshed: bool,
    acquisition_error: Option<&str>,
) -> Result<StatusObservation, String> {
    let last_error = acquisition_error
        .map(str::to_owned)
        .or_else(|| held.last_error.clone());
    let observation = StatusObservation {
        source: if refreshed {
            StatusObservationSource::Refresh
        } else {
            StatusObservationSource::Held
        },
        state: if held.needs_refresh || last_error.is_some() {
            StatusObservationState::Held
        } else {
            StatusObservationState::Ready
        },
        observed_at_ms: held.observed_at_ms,
        sha256: held.sha256.clone(),
        last_error,
    };
    observation.validate()?;
    Ok(observation)
}

/// Domain fields are required and typed; additive metadata fields are compatible.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(missing_as_null))]
pub struct StatusFileReceipt {
    pub schema: String,
    pub path: PathBuf,
    pub sha256: String,
    pub size_bytes: u64,
    pub lane: String,
    pub project: String,
    pub principal: String,
    pub count: u64,
    /// Omitted only by the existing CLI read without an explicit --read mode.
    pub observation: Option<StatusObservation>,
}
impl StatusFileReceipt {
    /// The CLI constructor reads the actual bytes it will write. No duplicate
    /// response dictionary can drift from the reader's admitted fields.
    pub fn from_projection_bytes(
        path: &Path,
        lane: &str,
        project: &str,
        principal: &str,
        bytes: &[u8],
    ) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > MAX_STATUS_FILE_BYTES {
            return Err("Status projection exceeds 64 MiB".into());
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|_| "malformed status JSON")?;
        let count = value
            .get("count")
            .and_then(Value::as_u64)
            .ok_or("crossed or malformed status projection")?;
        let observation = projection_observation(&value)?;
        let receipt = Self {
            schema: STATUS_FILE_SCHEMA.into(),
            path: path.into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            size_bytes: bytes.len() as u64,
            lane: lane.into(),
            project: project.into(),
            principal: principal.into(),
            count,
            observation,
        };
        receipt.validate_projection(&value, path, lane, project, principal, false)?;
        Ok(receipt)
    }
    pub fn decode(value: Value) -> Result<Self, String> {
        serde_json::from_value(value).map_err(|_| "malformed file receipt".into())
    }
    /// Validate before a host opens the fixed private output path.
    pub fn validate_target(
        &self,
        path: &Path,
        lane: &str,
        project: &str,
        principal: &str,
    ) -> Result<(), String> {
        if self.schema != STATUS_FILE_SCHEMA
            || self.path != path
            || !path.is_absolute()
            || path.to_str().is_none()
            || self.lane != lane
            || self.project != project
            || self.principal != principal
            || !crate::report_export::exact_lower_sha256(&self.sha256)
            || self.size_bytes == 0
            || self.size_bytes > MAX_STATUS_FILE_BYTES as u64
        {
            return Err("crossed or malformed status projection".into());
        }
        Ok(())
    }
    pub fn validate_projection(
        &self,
        value: &Value,
        path: &Path,
        lane: &str,
        project: &str,
        principal: &str,
        require_observation: bool,
    ) -> Result<(), String> {
        self.validate_target(path, lane, project, principal)?;
        let observation = projection_observation(value)?;
        let rows = value
            .get("transformers")
            .and_then(Value::as_array)
            .ok_or("crossed or malformed status projection")?;
        if value["lane"] != lane
            || value["project"]["ds_project"] != project
            || value["count"].as_u64() != Some(self.count)
            || rows.len() as u64 != self.count
            || observation != self.observation
            || (require_observation && observation.is_none())
        {
            return Err("crossed or malformed status projection".into());
        }
        // Reuse the status owner's row/name bounds without narrowing additive row metadata.
        super::admit_rows(Value::Array(rows.clone()))?;
        if let Some(observation) = &self.observation {
            observation.validate()?;
        }
        Ok(())
    }
    /// Private-file and principal liveness mechanics remain with the host.
    pub fn decode_projection_bytes(
        &self,
        bytes: &[u8],
        path: &Path,
        lane: &str,
        project: &str,
        principal: &str,
        require_observation: bool,
    ) -> Result<Value, String> {
        self.validate_target(path, lane, project, principal)?;
        if bytes.len() > MAX_STATUS_FILE_BYTES
            || bytes.len() as u64 != self.size_bytes
            || format!("{:x}", Sha256::digest(bytes)) != self.sha256
        {
            return Err("status output digest mismatch".into());
        }
        let value = serde_json::from_slice(bytes).map_err(|_| "malformed status JSON")?;
        self.validate_projection(&value, path, lane, project, principal, require_observation)?;
        Ok(value)
    }
}
fn projection_observation(value: &Value) -> Result<Option<StatusObservation>, String> {
    serde_json::from_value(value.get("observation").cloned().unwrap_or(Value::Null))
        .map_err(|_| "malformed observation state".into())
}

#[cfg(test)]
#[path = "receipts_tests.rs"]
mod tests;
