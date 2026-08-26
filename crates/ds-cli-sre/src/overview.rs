//! `ds sre overview` — the bounded platform reliability top line.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "sre.overview",
    path: &["sre", "overview"],
    contract: 1,
    summary: "Read fleet health, service SLOs, stale work and incidents.",
    purpose: "\
Start here for recent platform health. Returns the same bounded, read-only \
fleet and service projection as DS GridDesign's Reliability page. The paired \
application performs the governed read under its signed-in user; an active \
project is not required. `incidents` is the owner's currently unpopulated feed; \
an empty list is not proof that external incident systems have no incidents.",
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[crate::DESCRIPTOR_ARG],
    output: "\
`generated_at`, `fleet`, `combined_reports`, bounded `services`, `service_ops`, \
`stale`, `incidents`, and `error_catalog`; `totals` carries exact owner counts \
and `more` identifies each truncated collection.",
    examples: &[Example {
        command: "ds sre overview --output json",
        note: "Read totals and more before treating a bounded list as complete.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::SRE_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::NOT_PERMITTED,
    ],
    reference: Some("docs/reference/sre.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::OVERVIEW,
        json!({}),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_sre_failure)
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
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
