//! The server-status snapshot's wire contract: what a client asks ds-brain for,
//! and what it accepts back.
//!
//! One document serves the whole infrastructure monitor. ds-server publishes a
//! scrubbed status snapshot to GCS; ds-brain reads it back with its own
//! credential and hands it over unchanged, so the monitor renders identically
//! in local dev and in production. The request body, the response bound and
//! every rule the document must satisfy before a client may render it are
//! decided here, once, for `ds`, the desktop shell and the browser.
//!
//! Hosts perform the transfer through their declared operation
//! (`domains.sre.server_status` in the gateway registry) and hand the parsed
//! response back. They never name the route, build the body or read the wire
//! themselves. A document that fails admission is a typed refusal carrying the
//! sentence that names what was wrong, so a publisher slip is reported rather
//! than rendered as a half-valid dashboard.
//!
//! The decode rules are transcribed from the TypeScript boundary this replaces,
//! sentence for sentence, so a snapshot that rendered before renders now: a
//! missing or null member decodes to absent (an older publisher simply shows
//! less), while a member of the WRONG TYPE is a refusal — silently dropping it
//! would render a number that is not the box's.
//!
//! The browser reaches these through the `serverStatus` WASM export.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::layer_document::text;

pub const SCHEMA: &str = "ds.server-status/v1";
/// The gateway registry id every host resolves the route from.
pub const OPERATION: &str = "domains.sre.server_status";
pub const METHOD: &str = ds_gateway_client::Operation::SreServerStatus.method().as_str();
pub const PATH: &str = ds_gateway_client::Operation::SreServerStatus.path_template();
/// The published document is a few hundred KiB; this is the cap the descriptor
/// declares, and the bound a host reads under.
pub const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
pub const TIMEOUT_SECONDS: u64 = 30;
pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;

/// Bounds the TypeScript boundary never had. The publisher emits a task
/// manager's worth of rows, not a database; an unbounded array is a memory
/// shape decided by whoever wrote the document.
pub const MAX_PROCESSES: usize = 20_000;
pub const MAX_AGENTS: usize = 2_000;
pub const MAX_TEMPERATURES: usize = 512;
pub const MAX_INTERFACES: usize = 512;
pub const MAX_ADDRESSES: usize = 128;
pub const MAX_SAMPLES: usize = 8_192;
/// One reason per health check; the monitor has three.
pub const MAX_REASONS: usize = 16;
/// The monitor names at most three tools per agent.
pub const MAX_ACTIVITY_TOOLS: usize = 8;
/// Labels the monitor caps at ~100 characters (task, branch); a member longer
/// than this is a broken producer, not a long task.
pub const MAX_LABEL_CHARS: usize = 256;

/// Closed vocabularies the monitor publishes. A value outside one decodes to
/// absent — a newer producer renders less until this list learns it, rather
/// than costing the whole snapshot — while a non-string is still refused.
pub const AGENT_ACTIVITIES: &[&str] = &["tool", "model", "idle", "stopped"];
pub const AGENT_CPU_SOURCES: &[&str] = &["cgroup", "process_tree"];
pub const AGENT_MEMORY_SOURCES: &[&str] = &["cgroup", "pss", "rss"];
pub const AGENT_TASK_SOURCES: &[&str] = &[
    "ds_agent_task",
    "agent_name",
    "tmux_session",
    "worktree",
    "output_file",
];

/// What a backend that refused without words is reported as.
const SNAPSHOT_FALLBACK: &str = "Failed to read the server status snapshot";

/// Why an answer was refused.
///
/// `not_published` and `unauthorized` are separated from the general
/// `backend_refused` here rather than in a host: deciding what a 404 on this
/// document MEANS ("the box has never published") is a contract judgement, and
/// a host that re-derived it from the status code would be reading the wire.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i64>,
}

fn refuse(code: &'static str, message: impl Into<String>, status: Option<i64>) -> Refusal {
    Refusal {
        code,
        message: message.into(),
        status,
    }
}

// ── The decode boundary ────────────────────────────────────────────────────
// Every helper answers the same two questions the TypeScript ones did: is this
// member absent (null counts), and if it is present, is it the declared type.

fn object<'a>(value: &'a Value, field: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{field} must be an object"))
}

fn member<'a>(record: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    record.get(key).filter(|value| !value.is_null())
}

fn required_string(record: &Map<String, Value>, key: &str, field: &str) -> Result<String, String> {
    match member(record, key).and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => Ok(value.to_owned()),
        _ => Err(format!("{field} must be a non-empty string")),
    }
}

fn optional_string(
    record: &Map<String, Value>,
    key: &str,
    field: &str,
) -> Result<Option<String>, String> {
    match member(record, key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("{field} must be a string")),
    }
}

fn optional_number(
    record: &Map<String, Value>,
    key: &str,
    field: &str,
) -> Result<Option<f64>, String> {
    match member(record, key) {
        None => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|number| number.is_finite())
            .map(Some)
            .ok_or_else(|| format!("{field} must be a finite number")),
    }
}

fn required_number(record: &Map<String, Value>, key: &str, field: &str) -> Result<f64, String> {
    optional_number(record, key, field)?.ok_or_else(|| format!("{field} must be a finite number"))
}

fn optional_bool(
    record: &Map<String, Value>,
    key: &str,
    field: &str,
) -> Result<Option<bool>, String> {
    match member(record, key) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("{field} must be a boolean")),
    }
}

fn optional_array<'a>(
    record: &'a Map<String, Value>,
    key: &str,
    field: &str,
    bound: usize,
) -> Result<Option<&'a Vec<Value>>, String> {
    match member(record, key) {
        None => Ok(None),
        Some(Value::Array(rows)) if rows.len() <= bound => Ok(Some(rows)),
        Some(Value::Array(_)) => Err(format!("{field} exceeds its bound of {bound} entries")),
        Some(_) => Err(format!("{field} must be an array")),
    }
}

/// A label the publisher caps: present, a string, and inside the bound.
fn optional_label(
    record: &Map<String, Value>,
    key: &str,
    field: &str,
) -> Result<Option<String>, String> {
    let value = optional_string(record, key, field)?;
    match value {
        Some(text) if text.chars().count() > MAX_LABEL_CHARS => {
            Err(format!("{field} exceeds {MAX_LABEL_CHARS} characters"))
        }
        other => Ok(other),
    }
}

/// A member of a closed vocabulary: a non-string is refused, an unknown value
/// decodes to absent (see `AGENT_ACTIVITIES`).
fn optional_enum(
    record: &Map<String, Value>,
    key: &str,
    field: &str,
    allowed: &[&str],
) -> Result<Option<String>, String> {
    Ok(optional_string(record, key, field)?.filter(|value| allowed.contains(&value.as_str())))
}

/// Non-strings are dropped rather than refused, exactly as the boundary this
/// replaces did: one odd address must not cost the whole snapshot.
fn optional_string_array(
    record: &Map<String, Value>,
    key: &str,
    field: &str,
    bound: usize,
) -> Result<Option<Vec<String>>, String> {
    Ok(optional_array(record, key, field, bound)?.map(|rows| {
        rows.iter()
            .filter_map(|row| row.as_str().map(str::to_owned))
            .collect()
    }))
}

/// Drops non-finite entries rather than failing the whole snapshot for one bad
/// sample — a gap in a series is a gap, not a corrupt document.
fn optional_number_array(
    record: &Map<String, Value>,
    key: &str,
    field: &str,
    bound: usize,
) -> Result<Option<Vec<f64>>, String> {
    Ok(optional_array(record, key, field, bound)?.map(|rows| {
        rows.iter()
            .filter_map(Value::as_f64)
            .filter(|number| number.is_finite())
            .collect()
    }))
}

/// Insert a member only when the publisher emitted one, so an absent field
/// stays absent instead of becoming a null the reader has to test for.
struct Doc(Map<String, Value>);

impl Doc {
    fn new() -> Self {
        Self(Map::new())
    }
    fn put(&mut self, key: &str, value: Option<impl Into<Value>>) {
        if let Some(value) = value {
            self.0.insert(key.to_owned(), value.into());
        }
    }
    fn set(&mut self, key: &str, value: impl Into<Value>) {
        self.0.insert(key.to_owned(), value.into());
    }
    fn into_value(self) -> Value {
        Value::Object(self.0)
    }
}

fn decode_system(value: &Value) -> Result<Value, String> {
    let r = object(value, "health.system")?;
    let mut out = Doc::new();
    for key in [
        "os_name",
        "os_version",
        "kernel",
        "machine_vendor",
        "machine_model",
        "bios_version",
        "boot_mode",
        "environment",
    ] {
        out.put(
            key,
            optional_string(r, key, &format!("health.system.{key}"))?,
        );
    }
    out.put(
        "uptime_seconds",
        optional_number(r, "uptime_seconds", "health.system.uptime_seconds")?,
    );
    Ok(out.into_value())
}

fn decode_cpu(value: &Value) -> Result<Value, String> {
    let r = object(value, "health.cpu")?;
    let mut out = Doc::new();
    for key in ["model", "vendor", "architecture"] {
        out.put(key, optional_string(r, key, &format!("health.cpu.{key}"))?);
    }
    for key in [
        "usage_percent",
        "logical_cores",
        "physical_cores",
        "current_mhz",
        "max_mhz",
    ] {
        out.put(key, optional_number(r, key, &format!("health.cpu.{key}"))?);
    }
    Ok(out.into_value())
}

fn decode_load(value: &Value) -> Result<Value, String> {
    let r = object(value, "health.load")?;
    let mut out = Doc::new();
    out.put(
        "available",
        optional_bool(r, "available", "health.load.available")?,
    );
    for key in [
        "one_minute",
        "five_minutes",
        "fifteen_minutes",
        "one_minute_per_cpu",
    ] {
        out.put(key, optional_number(r, key, &format!("health.load.{key}"))?);
    }
    Ok(out.into_value())
}

fn decode_memory(value: &Value) -> Result<Value, String> {
    let r = object(value, "health.memory")?;
    let mut out = Doc::new();
    for key in [
        "used_bytes",
        "total_bytes",
        "available_bytes",
        "cached_bytes",
        "used_percent",
        "swap_used_bytes",
        "swap_total_bytes",
    ] {
        out.put(
            key,
            optional_number(r, key, &format!("health.memory.{key}"))?,
        );
    }
    Ok(out.into_value())
}

fn decode_disk(value: &Value) -> Result<Value, String> {
    let r = object(value, "health.disk")?;
    let mut out = Doc::new();
    for key in ["path", "device", "filesystem"] {
        out.put(key, optional_string(r, key, &format!("health.disk.{key}"))?);
    }
    for key in ["used_bytes", "total_bytes", "free_bytes", "used_percent"] {
        out.put(key, optional_number(r, key, &format!("health.disk.{key}"))?);
    }
    Ok(out.into_value())
}

fn decode_temperature(value: &Value, index: usize) -> Result<Value, String> {
    let field = format!("health.temperatures[{index}]");
    let r = object(value, &field)?;
    let mut out = Doc::new();
    out.set(
        "label",
        required_string(r, "label", &format!("{field}.label"))?,
    );
    out.set(
        "celsius",
        required_number(r, "celsius", &format!("{field}.celsius"))?,
    );
    Ok(out.into_value())
}

fn decode_nic(value: &Value, index: usize, parent: &str) -> Result<Value, String> {
    let field = format!("{parent}[{index}]");
    let r = object(value, &field)?;
    let mut out = Doc::new();
    out.set(
        "name",
        required_string(r, "name", &format!("{field}.name"))?,
    );
    for key in ["state", "mac"] {
        out.put(key, optional_string(r, key, &format!("{field}.{key}"))?);
    }
    for key in [
        "speed_mbps",
        "received_bytes",
        "transmitted_bytes",
        "received_bytes_per_second",
        "transmitted_bytes_per_second",
    ] {
        out.put(key, optional_number(r, key, &format!("{field}.{key}"))?);
    }
    out.put(
        "addresses",
        optional_string_array(r, "addresses", &format!("{field}.addresses"), MAX_ADDRESSES)?,
    );
    Ok(out.into_value())
}

fn decode_nics(record: &Map<String, Value>, field: &str) -> Result<Option<Value>, String> {
    let Some(rows) = optional_array(record, "network", field, MAX_INTERFACES)? else {
        return Ok(None);
    };
    let decoded = rows
        .iter()
        .enumerate()
        .map(|(index, row)| decode_nic(row, index, field))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(Value::Array(decoded)))
}

fn decode_history(value: &Value) -> Result<Value, String> {
    let r = object(value, "health.history")?;
    let mut out = Doc::new();
    for key in ["interval_seconds", "capacity", "samples"] {
        out.put(
            key,
            optional_number(r, key, &format!("health.history.{key}"))?,
        );
    }
    out.put(
        "interface",
        optional_string(r, "interface", "health.history.interface")?,
    );
    for key in [
        "sample_unix",
        "cpu_percent",
        "memory_percent",
        "received_bytes_per_second",
        "transmitted_bytes_per_second",
    ] {
        out.put(
            key,
            optional_number_array(r, key, &format!("health.history.{key}"), MAX_SAMPLES)?,
        );
    }
    Ok(out.into_value())
}

fn decode_proc_info(value: &Value) -> Result<Value, String> {
    let r = object(value, "health.processes")?;
    let mut out = Doc::new();
    for key in ["count", "tcp_connections"] {
        out.put(
            key,
            optional_number(r, key, &format!("health.processes.{key}"))?,
        );
    }
    Ok(out.into_value())
}

/// Why the box is degraded: one entry per failed check. `check` is an
/// identifier rather than a closed vocabulary, so a check a newer monitor adds
/// is still shown (by its name) instead of being hidden from the reader.
fn decode_reason(value: &Value, index: usize) -> Result<Value, String> {
    let field = format!("health.reasons[{index}]");
    let r = object(value, &field)?;
    let check = required_string(r, "check", &format!("{field}.check"))?;
    let identifier = check.len() <= 32
        && check.starts_with(|first: char| first.is_ascii_lowercase())
        && check.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        });
    if !identifier {
        return Err(format!("{field}.check must be a lowercase identifier"));
    }
    let mut out = Doc::new();
    out.set("check", check);
    for key in ["observed", "threshold"] {
        out.set(key, required_number(r, key, &format!("{field}.{key}"))?);
    }
    Ok(out.into_value())
}

fn decode_optional<'a>(
    record: &'a Map<String, Value>,
    key: &str,
    decode: impl Fn(&'a Value) -> Result<Value, String>,
) -> Result<Option<Value>, String> {
    member(record, key).map(decode).transpose()
}

fn decode_health(value: &Value) -> Result<Value, String> {
    let r = object(value, "health")?;
    let mut out = Doc::new();
    out.set("status", required_string(r, "status", "health.status")?);
    out.set(
        "hostname",
        required_string(r, "hostname", "health.hostname")?,
    );
    out.put(
        "uptime_seconds",
        optional_number(r, "uptime_seconds", "health.uptime_seconds")?,
    );
    out.put("system", decode_optional(r, "system", decode_system)?);
    out.put("cpu", decode_optional(r, "cpu", decode_cpu)?);
    out.put("load", decode_optional(r, "load", decode_load)?);
    out.put("memory", decode_optional(r, "memory", decode_memory)?);
    out.put("disk", decode_optional(r, "disk", decode_disk)?);
    if let Some(rows) = optional_array(r, "temperatures", "health.temperatures", MAX_TEMPERATURES)?
    {
        out.set(
            "temperatures",
            rows.iter()
                .enumerate()
                .map(|(index, row)| decode_temperature(row, index))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    out.put("network", decode_nics(r, "health.network")?);
    out.put(
        "processes",
        decode_optional(r, "processes", decode_proc_info)?,
    );
    out.put("history", decode_optional(r, "history", decode_history)?);
    if let Some(rows) = optional_array(r, "reasons", "health.reasons", MAX_REASONS)? {
        out.set(
            "reasons",
            rows.iter()
                .enumerate()
                .map(|(index, row)| decode_reason(row, index))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    Ok(out.into_value())
}

fn decode_process(value: &Value, index: usize) -> Result<Value, String> {
    let field = format!("task_manager.process_report.processes[{index}]");
    let r = object(value, &field)?;
    let mut out = Doc::new();
    out.set("pid", required_number(r, "pid", &format!("{field}.pid"))?);
    out.put(
        "parent_pid",
        optional_number(r, "parent_pid", &format!("{field}.parent_pid"))?,
    );
    for key in ["name", "user", "state"] {
        out.put(key, optional_string(r, key, &format!("{field}.{key}"))?);
    }
    for key in ["threads", "cpu_percent", "memory_bytes"] {
        out.put(key, optional_number(r, key, &format!("{field}.{key}"))?);
    }
    Ok(out.into_value())
}

fn decode_process_report(value: &Value) -> Result<Value, String> {
    let r = object(value, "task_manager.process_report")?;
    let rows = match member(r, "processes") {
        None => Vec::new(),
        Some(Value::Array(rows)) if rows.len() <= MAX_PROCESSES => rows
            .iter()
            .enumerate()
            .map(|(index, row)| decode_process(row, index))
            .collect::<Result<Vec<_>, _>>()?,
        Some(Value::Array(_)) => {
            return Err(format!(
                "task_manager.process_report.processes exceeds its bound of {MAX_PROCESSES} entries"
            ))
        }
        Some(_) => return Err("task_manager.process_report.processes must be an array".into()),
    };
    let mut out = Doc::new();
    for key in ["total", "shown", "cpu_window_seconds"] {
        out.put(
            key,
            optional_number(r, key, &format!("task_manager.process_report.{key}"))?,
        );
    }
    out.set("processes", rows);
    Ok(out.into_value())
}

fn decode_agent(value: &Value, index: usize) -> Result<Value, String> {
    let field = format!("task_manager.agents.agents[{index}]");
    let r = object(value, &field)?;
    let mut out = Doc::new();
    for key in [
        "pid",
        "parent_pid",
        "uptime_seconds",
        "cpu_percent",
        "memory_bytes",
        "threads",
        "process_count",
        "inprocess_subagents",
        "inprocess_workflows",
        "page_cache_bytes",
        "evidence_age_seconds",
    ] {
        out.put(key, optional_number(r, key, &format!("{field}.{key}"))?);
    }
    for key in ["task", "slice", "branch"] {
        out.put(key, optional_label(r, key, &format!("{field}.{key}"))?);
    }
    for (key, allowed) in [
        ("task_source", AGENT_TASK_SOURCES),
        ("activity", AGENT_ACTIVITIES),
        ("cpu_source", AGENT_CPU_SOURCES),
        ("memory_source", AGENT_MEMORY_SOURCES),
    ] {
        out.put(
            key,
            optional_enum(r, key, &format!("{field}.{key}"), allowed)?,
        );
    }
    if let Some(tools) = optional_string_array(
        r,
        "activity_tools",
        &format!("{field}.activity_tools"),
        MAX_ACTIVITY_TOOLS,
    )? {
        if let Some(long) = tools.iter().position(|tool| tool.chars().count() > 64) {
            return Err(format!(
                "{field}.activity_tools[{long}] exceeds 64 characters"
            ));
        }
        out.set("activity_tools", tools);
    }
    for key in [
        "role",
        "product",
        "kind",
        "model",
        "effort",
        "permission_mode",
        "entrypoint",
        "command",
        "cwd",
        "user",
        "state",
    ] {
        out.put(key, optional_string(r, key, &format!("{field}.{key}"))?);
    }
    for key in ["cwd_deleted", "orphaned"] {
        out.put(key, optional_bool(r, key, &format!("{field}.{key}"))?);
    }
    Ok(out.into_value())
}

fn decode_agents(value: &Value) -> Result<Value, String> {
    let r = object(value, "task_manager.agents")?;
    let rows = match member(r, "agents") {
        None => Vec::new(),
        Some(Value::Array(rows)) if rows.len() <= MAX_AGENTS => rows
            .iter()
            .enumerate()
            .map(|(index, row)| decode_agent(row, index))
            .collect::<Result<Vec<_>, _>>()?,
        Some(Value::Array(_)) => {
            return Err(format!(
                "task_manager.agents.agents exceeds its bound of {MAX_AGENTS} entries"
            ))
        }
        Some(_) => return Err("task_manager.agents.agents must be an array".into()),
    };
    let mut out = Doc::new();
    for key in [
        "count",
        "main_count",
        "subagent_count",
        "supervisor_count",
        "orphan_count",
        "inprocess_subagent_count",
        "discarded_noise_count",
        "cpu_window_seconds",
        "sampled_unix",
        "machine_cpu_percent",
        "agents_cpu_percent",
    ] {
        out.put(
            key,
            optional_number(r, key, &format!("task_manager.agents.{key}"))?,
        );
    }
    out.set("agents", rows);
    Ok(out.into_value())
}

fn decode_internet(value: &Value) -> Result<Value, String> {
    let r = object(value, "task_manager.internet")?;
    let mut out = Doc::new();
    out.put(
        "reachable",
        optional_bool(r, "reachable", "task_manager.internet.reachable")?,
    );
    out.put(
        "latency_ms",
        optional_number(r, "latency_ms", "task_manager.internet.latency_ms")?,
    );
    for key in ["target", "route_interface", "gateway"] {
        out.put(
            key,
            optional_string(r, key, &format!("task_manager.internet.{key}"))?,
        );
    }
    Ok(out.into_value())
}

fn decode_wifi(value: &Value) -> Result<Value, String> {
    let r = object(value, "task_manager.wifi")?;
    let mut out = Doc::new();
    out.put(
        "available",
        optional_bool(r, "available", "task_manager.wifi.available")?,
    );
    for key in ["interface", "ssid"] {
        out.put(
            key,
            optional_string(r, key, &format!("task_manager.wifi.{key}"))?,
        );
    }
    for key in [
        "signal_dbm",
        "quality_percent",
        "frequency_mhz",
        "receive_mbps",
        "transmit_mbps",
    ] {
        out.put(
            key,
            optional_number(r, key, &format!("task_manager.wifi.{key}"))?,
        );
    }
    Ok(out.into_value())
}

fn decode_tailscale(value: &Value) -> Result<Value, String> {
    let r = object(value, "task_manager.tailscale")?;
    let mut out = Doc::new();
    for key in ["installed", "online"] {
        out.put(
            key,
            optional_bool(r, key, &format!("task_manager.tailscale.{key}"))?,
        );
    }
    for key in ["backend_state", "dns_name", "relay", "version"] {
        out.put(
            key,
            optional_string(r, key, &format!("task_manager.tailscale.{key}"))?,
        );
    }
    out.put(
        "ips",
        optional_string_array(r, "ips", "task_manager.tailscale.ips", MAX_ADDRESSES)?,
    );
    for key in ["peers_total", "peers_online"] {
        out.put(
            key,
            optional_number(r, key, &format!("task_manager.tailscale.{key}"))?,
        );
    }
    Ok(out.into_value())
}

fn decode_task_manager(value: &Value) -> Result<Value, String> {
    let r = object(value, "task_manager")?;
    let mut out = Doc::new();
    out.put("agents", decode_optional(r, "agents", decode_agents)?);
    out.put(
        "process_report",
        decode_optional(r, "process_report", decode_process_report)?,
    );
    out.put("internet", decode_optional(r, "internet", decode_internet)?);
    out.put("wifi", decode_optional(r, "wifi", decode_wifi)?);
    out.put(
        "tailscale",
        decode_optional(r, "tailscale", decode_tailscale)?,
    );
    out.put("network", decode_nics(r, "task_manager.network")?);
    Ok(out.into_value())
}

/// Validate an already-parsed snapshot document; callers must not reinterpret
/// what comes back.
pub fn decode_snapshot(value: &Value) -> Result<Value, String> {
    let root = object(value, "snapshot")?;
    if root.get("schema") != Some(&json!(1)) {
        return Err("snapshot schema is not supported".into());
    }
    let published = match root.get("published_unix").and_then(Value::as_i64) {
        Some(seconds) if seconds > 0 => seconds,
        _ => return Err("published_unix must be a positive integer".into()),
    };
    let mut out = Doc::new();
    out.set("schema", 1);
    out.set("published_unix", published);
    out.set(
        "health",
        decode_health(root.get("health").unwrap_or(&Value::Null))?,
    );
    out.set(
        "task_manager",
        decode_task_manager(root.get("task_manager").unwrap_or(&Value::Null))?,
    );
    Ok(out.into_value())
}

/// The failure envelope ds-brain answers a refused read with, reported in its
/// own words with its status and code — the same reading `layer_document`
/// performs, over a body that is otherwise the document itself rather than a
/// `{success, data}` wrapper.
fn refusal_of(response: &Value) -> Option<Refusal> {
    let record = response.as_object()?;
    let message = text(record.get("error").unwrap_or(&Value::Null))
        .or_else(|| text(record.get("message").unwrap_or(&Value::Null)))
        .or(if record.get("success") == Some(&json!(false)) {
            Some(SNAPSHOT_FALLBACK)
        } else {
            None
        })?;
    let status = record.get("status").and_then(Value::as_i64);
    let code = text(record.get("error_code").unwrap_or(&Value::Null)).unwrap_or("");
    let worded = match (status, code.is_empty()) {
        (Some(status), false) => format!("{message} ({status} {code})"),
        (Some(status), true) => format!("{message} ({status})"),
        (None, false) => format!("{message} ({code})"),
        (None, true) => message.to_owned(),
    };
    Some(refuse(
        match status {
            Some(404) => "not_published",
            Some(401 | 403) => "unauthorized",
            _ => "backend_refused",
        },
        worded,
        status,
    ))
}

/// Whether a parsed answer is the box's status snapshot, or why it is not.
pub fn admit(response: &Value) -> Result<Value, Refusal> {
    if let Some(refusal) = refusal_of(response) {
        return Err(refusal);
    }
    decode_snapshot(response).map_err(|message| refuse("contract_violation", message, None))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    op: Op,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Op {
    /// The declared operation, the body to send and the bounds to read under.
    Request,
    /// Whether a parsed answer is the box's status snapshot, or why it is not.
    Admit { response: Value },
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("server status request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid server status request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!(
            "unsupported server status schema; expected {SCHEMA}"
        ));
    }
    let value = match request.op {
        Op::Request => json!({
            "operation": OPERATION,
            "method": METHOD,
            "path": PATH,
            "body": {},
            "response_limit": RESPONSE_LIMIT,
            "timeout_seconds": TIMEOUT_SECONDS,
        }),
        Op::Admit { response } => match admit(&response) {
            Ok(data) => json!({"outcome": "admitted", "data": data}),
            Err(refusal) => json!({"outcome": "refused", "refusal": refusal}),
        },
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Value {
        json!({
            "schema": 1,
            "published_unix": 1_700_000_000,
            "health": {
                "status": "degraded",
                "hostname": "ds-server",
                "uptime_seconds": 259_428.0,
                "system": {"os_name": "Ubuntu 26.04", "kernel": "7.0.0-31-generic", "machine_model": "MS-7E26"},
                "cpu": {"model": "AMD Ryzen 9 7950X", "usage_percent": 39.9, "logical_cores": 16.0},
                "load": {"available": true, "one_minute": 2.72, "one_minute_per_cpu": 0.17},
                "memory": {"used_percent": 31.3, "total_bytes": 68_719_476_736.0},
                "disk": {"used_percent": 89.6, "path": "/", "filesystem": "ext4"},
                "temperatures": [{"label": "k10temp · Tctl", "celsius": 77.8}],
                "network": [{"name": "wlp5s0", "state": "up", "addresses": ["192.168.1.20"]}],
                "processes": {"count": 512.0, "tcp_connections": 44.0},
                "history": {"interval_seconds": 5.0, "interface": "wlp5s0", "sample_unix": [1.0, 2.0], "cpu_percent": [30.0, 39.9]},
                "reasons": [{"check": "load", "observed": 2.36, "threshold": 2.0}]
            },
            "task_manager": {
                "agents": {
                    "count": 2.0, "orphan_count": 1.0, "cpu_window_seconds": 10.04, "sampled_unix": 1_699_999_998.25,
                    "machine_cpu_percent": 1484.8, "agents_cpu_percent": 1279.1,
                    "agents": [{
                        "pid": 4242.0, "product": "claude", "orphaned": false,
                        "task": "Server monitor: accurate agent CPU", "task_source": "ds_agent_task",
                        "slice": "server-monitor", "branch": "slice/server-monitor",
                        "activity": "tool", "activity_tools": ["rustc", "cargo"],
                        "evidence_age_seconds": 12.0,
                        "cpu_percent": 371.5, "cpu_source": "cgroup",
                        "memory_bytes": 2_861_563_904.0, "memory_source": "cgroup",
                        "page_cache_bytes": 2_014_314_496.0
                    }]
                },
                "process_report": {"total": 512.0, "shown": 1.0, "cpu_window_seconds": 10.04, "processes": [{"pid": 1.0, "name": "systemd"}]},
                "internet": {"reachable": true, "latency_ms": 72.0, "target": "1.1.1.1"},
                "wifi": {"available": true, "ssid": "DS-FIELD", "signal_dbm": -77.0},
                "tailscale": {"installed": true, "backend_state": "Running", "peers_total": 5.0, "peers_online": 3.0},
                "network": [{"name": "tailscale0"}]
            }
        })
    }

    fn run(op: Value) -> Value {
        let input = serde_json::to_vec(&json!({"schema": SCHEMA, "op": op})).unwrap();
        serde_json::from_str(&evaluate(&input).unwrap()).unwrap()
    }

    fn refusal(response: Value) -> Refusal {
        admit(&response).expect_err("this response must not be admitted")
    }

    /// A snapshot with one member replaced, to pin one rule at a time.
    fn with(pointer: &[&str], value: Value) -> Value {
        let mut document = snapshot();
        let mut cursor = &mut document;
        for key in &pointer[..pointer.len() - 1] {
            cursor = cursor.get_mut(*key).expect("fixture path exists");
        }
        cursor
            .as_object_mut()
            .unwrap()
            .insert((*pointer.last().unwrap()).to_owned(), value);
        document
    }

    #[test]
    fn the_request_carries_the_declared_operation_and_no_body() {
        let out = run(json!({"kind": "request"}));
        assert_eq!(out["operation"], OPERATION);
        assert_eq!(out["method"], METHOD);
        assert_eq!(out["path"], PATH);
        assert_eq!(out["body"], json!({}));
        assert_eq!(out["response_limit"], RESPONSE_LIMIT);
        assert_eq!(out["timeout_seconds"], TIMEOUT_SECONDS);
    }

    #[test]
    fn the_requested_operation_is_declared_with_matching_route_and_bounds() {
        let request = run(json!({"kind": "request"}));
        let descriptor: Value =
            serde_json::from_str(include_str!("../routing/operations.json")).unwrap();
        let name = request["operation"]
            .as_str()
            .unwrap()
            .strip_prefix("domains.sre.")
            .expect("server status must request an SRE operation");
        let operation = descriptor["domains"]["sre"]["operations"]
            .get(name)
            .expect("the requested server status operation must exist in the routing descriptor");
        assert_eq!(request["method"], operation["gateway"]["method"]);
        assert_eq!(request["path"], operation["gateway"]["path"]);
        let policy = |field: &str| {
            operation
                .get(field)
                .unwrap_or(&descriptor["operation_defaults"][field])
        };
        assert_eq!(&request["response_limit"], policy("response_cap_bytes"));
        assert_eq!(&request["timeout_seconds"], policy("timeout_s"));
        assert_eq!(policy("authority"), "gateway");
        assert_eq!(policy("principal"), "user");
        assert_eq!(policy("transport"), "json");
        assert_eq!(operation["resolution"]["web"], "gateway");
        assert_eq!(operation["resolution"]["desktop"], "gateway");
    }

    #[test]
    fn a_published_snapshot_is_admitted_unchanged() {
        let out = run(json!({"kind": "admit", "response": snapshot()}));
        assert_eq!(out["outcome"], "admitted");
        assert_eq!(out["data"], snapshot());
    }

    #[test]
    fn an_absent_member_stays_absent_rather_than_becoming_null() {
        // An older publisher renders less, never wrongly: null and missing are
        // the same thing, and neither is a zero.
        let document = with(&["health", "cpu"], json!({"usage_percent": null}));
        let decoded = decode_snapshot(&document).unwrap();
        assert_eq!(decoded["health"]["cpu"], json!({}));
        assert!(decoded["health"]["cpu"].get("usage_percent").is_none());
    }

    #[test]
    fn the_root_must_be_a_schema_1_object_stamped_with_a_real_time() {
        assert_eq!(refusal(json!([])).message, "snapshot must be an object");
        assert_eq!(
            refusal(with(&["schema"], json!(2))).message,
            "snapshot schema is not supported"
        );
        assert_eq!(
            refusal(with(&["schema"], json!("1"))).message,
            "snapshot schema is not supported"
        );
        for stamp in [
            json!(0),
            json!(-5),
            json!(1.5),
            json!("1700000000"),
            Value::Null,
        ] {
            assert_eq!(
                refusal(with(&["published_unix"], stamp)).message,
                "published_unix must be a positive integer"
            );
        }
    }

    #[test]
    fn the_identity_members_are_required_and_non_empty() {
        assert_eq!(
            refusal(with(&["health", "status"], json!(" "))).message,
            "health.status must be a non-empty string"
        );
        assert_eq!(
            refusal(with(&["health", "hostname"], Value::Null)).message,
            "health.hostname must be a non-empty string"
        );
        assert_eq!(
            refusal(with(&["health"], json!("healthy"))).message,
            "health must be an object"
        );
    }

    #[test]
    fn a_member_of_the_wrong_type_is_refused_rather_than_dropped() {
        // Dropping it would render a figure that is not the box's.
        assert_eq!(
            refusal(with(&["health", "cpu"], json!({"usage_percent": "39.9"}))).message,
            "health.cpu.usage_percent must be a finite number"
        );
        assert_eq!(
            refusal(with(&["health", "cpu"], json!({"model": 7}))).message,
            "health.cpu.model must be a string"
        );
        assert_eq!(
            refusal(with(&["health", "load"], json!({"available": 1}))).message,
            "health.load.available must be a boolean"
        );
        assert_eq!(
            refusal(with(&["health", "system"], json!([]))).message,
            "health.system must be an object"
        );
        assert_eq!(
            refusal(with(&["health", "temperatures"], json!({}))).message,
            "health.temperatures must be an array"
        );
        assert_eq!(
            refusal(with(
                &["health", "temperatures"],
                json!([{"celsius": 50.0}])
            ))
            .message,
            "health.temperatures[0].label must be a non-empty string"
        );
        assert_eq!(
            refusal(with(&["health", "temperatures"], json!([{"label": "cpu"}]))).message,
            "health.temperatures[0].celsius must be a finite number"
        );
        assert_eq!(
            refusal(with(&["health", "network"], json!([{"state": "up"}]))).message,
            "health.network[0].name must be a non-empty string"
        );
    }

    #[test]
    fn the_two_row_collections_must_be_arrays() {
        let agents = with(&["task_manager"], json!({"agents": {"agents": {}}}));
        assert_eq!(
            refusal(agents).message,
            "task_manager.agents.agents must be an array"
        );
        let processes = with(
            &["task_manager"],
            json!({"process_report": {"processes": 7}}),
        );
        assert_eq!(
            refusal(processes).message,
            "task_manager.process_report.processes must be an array"
        );
        // Absent rows are an empty report, not a refusal.
        let empty = with(&["task_manager"], json!({"agents": {"count": 0.0}}));
        let decoded = decode_snapshot(&empty).unwrap();
        assert_eq!(decoded["task_manager"]["agents"]["agents"], json!([]));
    }

    #[test]
    fn series_and_address_arrays_drop_what_they_cannot_read() {
        // A gap in a series is a gap; it must not cost the whole snapshot.
        let document = with(
            &["health", "history"],
            json!({"cpu_percent": [30.0, "x", 39.9], "sample_unix": [1.0, null, 2.0]}),
        );
        let decoded = decode_snapshot(&document).unwrap();
        assert_eq!(
            decoded["health"]["history"]["cpu_percent"],
            json!([30.0, 39.9])
        );
        assert_eq!(
            decoded["health"]["history"]["sample_unix"],
            json!([1.0, 2.0])
        );
        let nics = with(
            &["health", "network"],
            json!([{"name": "eth0", "addresses": ["10.0.0.1", 7]}]),
        );
        let decoded = decode_snapshot(&nics).unwrap();
        assert_eq!(
            decoded["health"]["network"][0]["addresses"],
            json!(["10.0.0.1"])
        );
    }

    #[test]
    fn every_collection_is_bounded() {
        let rows: Vec<Value> = (0..=MAX_AGENTS)
            .map(|pid| json!({"pid": pid as f64}))
            .collect();
        let document = with(&["task_manager"], json!({"agents": {"agents": rows}}));
        assert_eq!(
            refusal(document).message,
            format!("task_manager.agents.agents exceeds its bound of {MAX_AGENTS} entries")
        );
        let rows: Vec<Value> = (0..=MAX_TEMPERATURES)
            .map(|_| json!({"label": "s", "celsius": 1.0}))
            .collect();
        assert_eq!(
            refusal(with(&["health", "temperatures"], json!(rows))).message,
            format!("health.temperatures exceeds its bound of {MAX_TEMPERATURES} entries")
        );
    }

    #[test]
    fn members_this_reader_does_not_know_are_ignored_so_the_producer_ships_first() {
        // The decoder copies an allow-list: a field the monitor adds before
        // this kernel learns it is dropped, never a refusal.
        let mut document = snapshot();
        document["health"]["future_gauge"] = json!({"x": 1});
        document["task_manager"]["agents"]["agents"][0]["future_field"] = json!("y");
        document["task_manager"]["future_panel"] = json!([1, 2]);
        let decoded = decode_snapshot(&document).unwrap();
        assert_eq!(decoded, snapshot());
    }

    #[test]
    fn degraded_reasons_name_the_check_its_reading_and_threshold() {
        let decoded = decode_snapshot(&snapshot()).unwrap();
        assert_eq!(
            decoded["health"]["reasons"],
            json!([{"check": "load", "observed": 2.36, "threshold": 2.0}])
        );
        // A check a newer monitor adds is shown by name, not hidden.
        let newer = with(
            &["health", "reasons"],
            json!([{"check": "temperature_c", "observed": 91.0, "threshold": 90.0}]),
        );
        assert_eq!(
            decode_snapshot(&newer).unwrap()["health"]["reasons"][0]["check"],
            "temperature_c"
        );
        for (reasons, message) in [
            (json!({}), "health.reasons must be an array"),
            (
                json!([{"observed": 1.0, "threshold": 1.0}]),
                "health.reasons[0].check must be a non-empty string",
            ),
            (
                json!([{"check": "Load <b>", "observed": 1.0, "threshold": 1.0}]),
                "health.reasons[0].check must be a lowercase identifier",
            ),
            (
                json!([{"check": "load", "observed": "2.36", "threshold": 2.0}]),
                "health.reasons[0].observed must be a finite number",
            ),
            (
                json!([{"check": "load", "observed": 2.36}]),
                "health.reasons[0].threshold must be a finite number",
            ),
        ] {
            assert_eq!(
                refusal(with(&["health", "reasons"], reasons)).message,
                message
            );
        }
        let rows: Vec<Value> = (0..=MAX_REASONS)
            .map(|_| json!({"check": "load", "observed": 1.0, "threshold": 1.0}))
            .collect();
        assert_eq!(
            refusal(with(&["health", "reasons"], json!(rows))).message,
            format!("health.reasons exceeds its bound of {MAX_REASONS} entries")
        );
    }

    fn with_agent(key: &str, value: Value) -> Value {
        let mut document = snapshot();
        document["task_manager"]["agents"]["agents"][0][key] = value;
        document
    }

    fn decoded_agent(document: &Value) -> Value {
        decode_snapshot(document).unwrap()["task_manager"]["agents"]["agents"][0].clone()
    }

    #[test]
    fn agent_vocabularies_drop_unknown_values_and_refuse_wrong_types() {
        for (key, allowed) in [
            ("activity", AGENT_ACTIVITIES),
            ("cpu_source", AGENT_CPU_SOURCES),
            ("memory_source", AGENT_MEMORY_SOURCES),
            ("task_source", AGENT_TASK_SOURCES),
        ] {
            for value in allowed {
                assert_eq!(
                    decoded_agent(&with_agent(key, json!(value)))[key],
                    json!(value)
                );
            }
            // A newer producer's value renders less, never a refusal.
            assert!(decoded_agent(&with_agent(key, json!("compacting")))
                .get(key)
                .is_none());
            assert_eq!(
                refusal(with_agent(key, json!(7))).message,
                format!("task_manager.agents.agents[0].{key} must be a string")
            );
        }
    }

    #[test]
    fn agent_labels_and_tools_are_bounded() {
        let long = "x".repeat(MAX_LABEL_CHARS + 1);
        for key in ["task", "slice", "branch"] {
            assert_eq!(
                refusal(with_agent(key, json!(long))).message,
                format!("task_manager.agents.agents[0].{key} exceeds {MAX_LABEL_CHARS} characters")
            );
        }
        // At the bound is fine, multi-byte characters count once.
        let edge = "é".repeat(MAX_LABEL_CHARS);
        assert_eq!(
            decoded_agent(&with_agent("task", json!(edge)))["task"],
            json!(edge)
        );
        // One odd tool entry is dropped, like an odd address.
        assert_eq!(
            decoded_agent(&with_agent("activity_tools", json!(["cargo", 7, "rustc"])))
                ["activity_tools"],
            json!(["cargo", "rustc"])
        );
        let tools: Vec<Value> = (0..=MAX_ACTIVITY_TOOLS).map(|_| json!("cargo")).collect();
        assert_eq!(
            refusal(with_agent("activity_tools", json!(tools))).message,
            format!(
                "task_manager.agents.agents[0].activity_tools exceeds its bound of {MAX_ACTIVITY_TOOLS} entries"
            )
        );
        assert_eq!(
            refusal(with_agent("activity_tools", json!(["y".repeat(65)]))).message,
            "task_manager.agents.agents[0].activity_tools[0] exceeds 64 characters"
        );
        assert_eq!(
            refusal(with_agent("evidence_age_seconds", json!("12"))).message,
            "task_manager.agents.agents[0].evidence_age_seconds must be a finite number"
        );
    }

    #[test]
    fn the_measurement_window_and_machine_reconciliation_are_admitted() {
        let decoded = decode_snapshot(&snapshot()).unwrap();
        let agents = &decoded["task_manager"]["agents"];
        assert_eq!(agents["cpu_window_seconds"], json!(10.04));
        assert_eq!(agents["sampled_unix"], json!(1_699_999_998.25));
        assert_eq!(agents["machine_cpu_percent"], json!(1484.8));
        assert_eq!(agents["agents_cpu_percent"], json!(1279.1));
        // Workers a finished session left running are counted apart.
        assert_eq!(agents["orphan_count"], json!(1.0));
        assert_eq!(
            decoded["task_manager"]["process_report"]["cpu_window_seconds"],
            json!(10.04)
        );
        let mut document = snapshot();
        document["task_manager"]["agents"]["machine_cpu_percent"] = json!("1484.8");
        assert_eq!(
            refusal(document).message,
            "task_manager.agents.machine_cpu_percent must be a finite number"
        );
    }

    #[test]
    fn a_backend_refusal_is_reported_in_its_own_words() {
        let refused = refusal(
            json!({"error": "Failed to read status object", "status": 500, "error_code": "INTERNAL_ERROR"}),
        );
        assert_eq!(refused.code, "backend_refused");
        assert_eq!(
            refused.message,
            "Failed to read status object (500 INTERNAL_ERROR)"
        );
        assert_eq!(refused.status, Some(500));
        // The two answers the monitor renders differently: never published, and
        // not this account's to read. Deciding that is the contract's, not a
        // host's re-reading of the status code.
        assert_eq!(
            refusal(json!({"error": "not found", "status": 404})).code,
            "not_published"
        );
        for status in [401, 403] {
            assert_eq!(
                refusal(json!({"error": "forbidden", "status": status})).code,
                "unauthorized"
            );
        }
        // A backend that refused without words still refuses.
        let silent = refusal(json!({"success": false, "status": 502}));
        assert_eq!(silent.code, "backend_refused");
        assert_eq!(silent.message, format!("{SNAPSHOT_FALLBACK} (502)"));
        assert_eq!(
            refusal(json!({"message": "upstream is down"})).message,
            "upstream is down"
        );
    }

    #[test]
    fn a_contract_violation_is_a_refusal_with_the_sentence_that_names_it() {
        let out = run(json!({"kind": "admit", "response": with(&["schema"], json!(3))}));
        assert_eq!(out["outcome"], "refused");
        assert_eq!(out["refusal"]["code"], "contract_violation");
        assert_eq!(
            out["refusal"]["message"],
            "snapshot schema is not supported"
        );
        assert!(out["refusal"].get("status").is_none());
    }

    #[test]
    fn the_request_envelope_is_closed() {
        let unknown = serde_json::to_vec(
            &json!({"schema": SCHEMA, "op": {"kind": "request"}, "project": "p1"}),
        )
        .unwrap();
        assert!(evaluate(&unknown)
            .unwrap_err()
            .starts_with("invalid server status request"));
        let wrong_schema = serde_json::to_vec(
            &json!({"schema": "ds.server-status/v2", "op": {"kind": "request"}}),
        )
        .unwrap();
        assert_eq!(
            evaluate(&wrong_schema).unwrap_err(),
            format!("unsupported server status schema; expected {SCHEMA}")
        );
        assert_eq!(
            evaluate(&vec![b'x'; MAX_INPUT_BYTES + 1]).unwrap_err(),
            "server status request exceeds its bound"
        );
    }
}
