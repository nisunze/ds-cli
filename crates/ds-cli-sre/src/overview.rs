//! `ds sre overview` — the bounded platform reliability top line.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

pub static COMMAND: Command = Command {
    id: "sre.overview",
    path: &["sre", "overview"],
    contract: 1,
    summary: "Read fleet health, service SLOs, stale work and incidents.",
    purpose: "\
Start here for recent platform health. Returns the same bounded, read-only \
fleet and service projection as DS GridDesign's Reliability page. The read is \
performed under this machine's restored native user; no Desktop and no active \
project are required, and none is sent. `incidents` is the owner's currently \
unpopulated feed; an empty list is not proof that external incident systems \
have no incidents.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[crate::LANE_ARG],
    output: "\
`generated_at`, `fleet`, `combined_reports`, bounded `services`, `service_ops`, \
`stale`, `incidents`, and `error_catalog`; `totals` carries exact owner counts \
and `more` identifies each truncated collection. `ds_client` is the `ds` CLI \
and MCP row over the last seven days: occurrence-weighted `invocations`, \
`failures`, `crashes`, `refusals`, `fault_ratio_pct`, `top_failing` and \
`most_refused`, with `complete`; `available: false` carries the `reason` it \
could not be read.",
    examples: &[Example {
        command: "ds sre overview --output json",
        note: "Read totals and more before treating a bounded list as complete.",
        runnable: false,
    }],
    refusals: &crate::native_refusals::<
        1,
        { 1 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() },
    >([crate::NOT_PERMITTED]),
    reference: Some("docs/reference/sre.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    crate::invoke_native(inputs, &ds_client_core::sre::Command::Overview)
}

pub fn render(data: &Value) -> String {
    let totals = &data["totals"];
    let fleet = &data["fleet"];
    let mut out = format!(
        "reliability at {} · {} services · {} incidents · {} stale\n",
        data["generated_at"].as_str().unwrap_or("unknown time"),
        totals["services"].as_u64().unwrap_or(0),
        totals["incidents"].as_u64().unwrap_or(0),
        totals["stale"].as_u64().unwrap_or(0),
    );
    if !fleet.is_null() {
        let request_rate = fleet["request_rate"]
            .as_f64()
            .map(|value| format!("{value:.2}"))
            .unwrap_or_else(|| "—".to_string());
        let error_ratio = fleet["error_ratio_pct"]
            .as_f64()
            .map(|value| format!("{value:.2}%"))
            .unwrap_or_else(|| "—".to_string());
        let oom_kills = fleet["oom_kills_1h"]
            .as_u64()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "—".to_string());
        let window = fleet["window_minutes"]
            .as_u64()
            .map(|value| format!("{value}m"))
            .unwrap_or_else(|| "—".to_string());
        out.push_str(&format!(
            "  fleet {request_rate} req/s · {error_ratio} 5xx · {oom_kills} OOM kills ({window})\n",
        ));
    }
    out.push_str(&render_ds_client(&data["ds_client"]));
    out
}

/// One line for the `ds` client row. An unreadable row says so; it is never
/// rendered as a quiet, healthy client.
fn render_ds_client(row: &Value) -> String {
    if row.is_null() {
        return String::new();
    }
    if row["available"].as_bool() != Some(true) {
        return format!(
            "  ds client unavailable: {}\n",
            row["reason"].as_str().unwrap_or("no reason given")
        );
    }
    let fault_ratio = row["fault_ratio_pct"]
        .as_f64()
        .map(|value| format!("{value:.2}%"))
        .unwrap_or_else(|| "—".to_string());
    let commands = |list: &Value, count_field: &str| -> String {
        let named: Vec<String> = list
            .as_array()
            .into_iter()
            .flatten()
            .map(|entry| {
                format!(
                    "{} ({})",
                    entry["action"].as_str().unwrap_or("?"),
                    entry[count_field].as_u64().unwrap_or(0)
                )
            })
            .collect();
        if named.is_empty() {
            "none".to_string()
        } else {
            named.join(", ")
        }
    };
    let mut line = format!(
        "  ds client {fault_ratio} faults · {} of {} invocations failed · {} crashed · {} refused ({}d)\n",
        row["failures"].as_u64().unwrap_or(0),
        row["invocations"].as_u64().unwrap_or(0),
        row["crashes"].as_u64().unwrap_or(0),
        row["refusals"].as_u64().unwrap_or(0),
        row["window_days"].as_u64().unwrap_or(0),
    );
    line.push_str(&format!(
        "    top failing: {}\n    most refused: {}\n",
        commands(&row["top_failing"], "failures"),
        commands(&row["most_refused"], "refusals"),
    ));
    if row["complete"].as_bool() == Some(false) {
        line.push_str("    window saturated; the counts cover the newest events only\n");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_fleet_telemetry_is_not_rendered_as_a_false_zero() {
        let rendered = render(&json!({
            "generated_at": "2026-08-26T00:00:00Z",
            "totals": { "services": 3, "incidents": 0, "stale": 0 },
            "fleet": {
                "request_rate": null,
                "error_ratio_pct": null,
                "oom_kills_1h": null,
                "window_minutes": null
            }
        }));
        assert!(rendered.contains("fleet — req/s · — 5xx · — OOM kills (—)"));
        assert!(!rendered.contains("0.00"));
    }

    #[test]
    fn the_ds_client_row_names_its_faults_and_commands() {
        let rendered = render(&json!({
            "generated_at": "2026-10-09T06:00:00Z",
            "totals": { "services": 3, "incidents": 0, "stale": 0 },
            "fleet": null,
            "ds_client": {
                "available": true, "window_days": 7, "complete": false,
                "invocations": 100, "failures": 4, "crashes": 1, "refusals": 5,
                "fault_ratio_pct": 4.0,
                "top_failing": [{"action": "data.vector.buffer", "failures": 3}],
                "most_refused": [{"action": "sre.events", "refusals": 5}]
            }
        }));
        assert!(rendered.contains(
            "ds client 4.00% faults · 4 of 100 invocations failed · 1 crashed · 5 refused (7d)"
        ));
        assert!(rendered.contains("top failing: data.vector.buffer (3)"));
        assert!(rendered.contains("most refused: sre.events (5)"));
        assert!(rendered.contains("window saturated"));
    }

    #[test]
    fn an_unreadable_ds_client_row_is_never_shown_as_healthy() {
        let rendered = render(&json!({
            "generated_at": "2026-10-09T06:00:00Z",
            "totals": {},
            "fleet": null,
            "ds_client": { "available": false, "reason": "the reliability authority could not answer this read" }
        }));
        assert!(rendered.contains(
            "ds client unavailable: the reliability authority could not answer this read"
        ));
        assert!(!rendered.contains("faults"));
        // A quiet week has no ratio rather than a false zero.
        let quiet = render(&json!({
            "totals": {},
            "fleet": null,
            "ds_client": {
                "available": true, "window_days": 7, "complete": true,
                "invocations": 0, "failures": 0, "crashes": 0, "refusals": 0,
                "fault_ratio_pct": null, "top_failing": [], "most_refused": []
            }
        }));
        assert!(quiet.contains("ds client — faults"));
        assert!(quiet.contains("top failing: none"));
    }
}
