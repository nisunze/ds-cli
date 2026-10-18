//! Portable SQLite schema and admission for full authenticated publication heads.
//! Native and browser storage adapters execute these statements and retain the
//! same complete declarations. This module performs no transport or filesystem IO.
use super::Fence;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const MAX_HEADS: usize = 4096;
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS report_head_observations(
 account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
 project TEXT NOT NULL, observed_at_ms INTEGER NOT NULL, document TEXT NOT NULL,
 PRIMARY KEY(account,deployment,install_id,project));";
pub const EXISTS: &str = "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='report_head_observations')";
pub const READ: &str = "SELECT observed_at_ms,document FROM report_head_observations WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4";
pub const REPLACE: &str = "INSERT INTO report_head_observations VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(account,deployment,install_id,project) DO UPDATE SET observed_at_ms=excluded.observed_at_ms,document=excluded.document";
pub const UPDATE: &str = "UPDATE report_head_observations SET document=?5,observed_at_ms=?6 WHERE account=?1 AND deployment=?2 AND install_id=?3 AND project=?4";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportHeadObservation {
    pub observed_at_ms: u64,
    pub heads: Vec<Value>,
}

pub fn validate_scope(fence: &Fence, project: &str, at_ms: u64) -> Result<(), String> {
    if [&fence.account, &fence.deployment, &fence.install_id]
        .iter()
        .any(|v| v.trim().is_empty())
    {
        return Err("incomplete report observation fence".into());
    }
    if !crate::execution_context::valid_project(project) || at_ms > i64::MAX as u64 {
        return Err("report head observation scope is invalid".into());
    }
    Ok(())
}

pub fn decode(bytes: &[u8]) -> Result<Vec<Value>, String> {
    if bytes.len() > MAX_BYTES {
        return Err("report head observation exceeds its byte bound".into());
    }
    let values: Vec<Value> = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if values.len() > MAX_HEADS {
        return Err("report head observation exceeds its row bound".into());
    }
    let mut identities = BTreeSet::new();
    for value in &values {
        let identity =
            ["engine", "operation", "variant"].map(|key| value[key].as_str().unwrap_or_default());
        if identity
            .iter()
            .any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
            || !identities.insert(identity.map(str::to_owned))
        {
            return Err(
                "publication head observation contains an invalid or duplicate identity".into(),
            );
        }
    }
    Ok(values)
}

pub fn encode(heads: &[Value]) -> Result<String, String> {
    let document = serde_json::to_string(heads).map_err(|e| e.to_string())?;
    decode(document.as_bytes())?;
    Ok(document)
}

/// A stale selected read does not overwrite a newer complete observation.
/// The same decoded revision cannot acquire different output declarations.
pub fn merge(document: &str, head: &Value) -> Result<Option<String>, String> {
    encode(&[head.clone()])?;
    let mut heads = decode(document.as_bytes())?;
    if let Some(index) = heads.iter().position(|old| {
        ["engine", "operation", "variant"]
            .iter()
            .all(|k| old[k] == head[k])
    }) {
        let old = &heads[index];
        if old["head_revision"].as_u64() > head["head_revision"].as_u64() {
            return Ok(None);
        }
        if old["head_revision"] == head["head_revision"] {
            let decode = |v: &Value| {
                crate::compute_artifact_inventory::decode_head(
                    &serde_json::to_vec(v).unwrap_or_default(),
                )
            };
            if decode(old).is_ok() && decode(old).ok() != decode(head).ok() {
                return Err("the same publication revision has different declarations".into());
            }
        }
        heads[index] = head.clone();
    } else {
        heads.push(head.clone());
    }
    encode(&heads).map(Some)
}
