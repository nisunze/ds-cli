//! Submit one agent-authored product observation.

use std::collections::BTreeMap;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops;
use serde_json::{Map, Value, json};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_TITLE_CHARS: usize = 200;
const MAX_DETAIL_BYTES: usize = 16 * 1024;
const MAX_COMPONENT_CHARS: usize = 200;
const MAX_AGENT_PART_CHARS: usize = 200;
const MAX_EVIDENCE: usize = 20;
const MAX_EVIDENCE_CHARS: usize = 500;
const MAX_CONTEXT: usize = 24;
const MAX_CONTEXT_KEY_CHARS: usize = 500;
const MAX_CONTEXT_VALUE_CHARS: usize = 1_000;

const INVALID_TEXT: Refusal = Refusal {
    code: "invalid_text",
    when: "a required report field is empty, untrimmed, or exceeds its bound",
    remedy: "send a concise title, detail, component and agent name without secrets or customer data",
};
const INVALID_EVIDENCE: Refusal = Refusal {
    code: "invalid_evidence",
    when: "evidence has too many entries or an entry exceeds its bound",
    remedy: "send at most 20 bounded observations; repeat --evidence for distinct facts",
};
const INVALID_CONTEXT: Refusal = Refusal {
    code: "invalid_context",
    when: "context is not a unique bounded key=value pair",
    remedy: "repeat --context with at most 24 unique, non-secret key=value pairs",
};
const NOT_SIGNED_IN: Refusal = Refusal {
    code: "desktop_signed_out",
    when: "DS GridDesign is running but has no signed-in user",
    remedy: "sign in to DS GridDesign, then submit the report again",
};

pub static COMMAND: Command = Command {
    id: "feedback.submit",
    path: &["feedback", "submit"],
    contract: 1,
    summary: "Report an observed product gap to the shared feedback backlog.",
    purpose: "\
Submits one agent-authored sighting through the paired application's existing \
feedback client. It reaches the same deduplicated backlog as the `fb` shortcut; \
it does not create a local gap file or a second issue channel. Use only after \
live capability discovery confirms the task is unsupported or materially broken.",
    chapter: Chapter::Operations,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "title",
            "<text>",
            "One-line observed gap; at most 200 characters.",
        )
        .required(),
        Arg::value(
            "detail",
            "<text>",
            "Expected behavior, observation and acceptance condition; at most 16 KiB.",
        )
        .required(),
        Arg::value(
            "component",
            "<repository[/area]>",
            "Owning repository or area; at most 200 characters.",
        )
        .required(),
        Arg::value("kind", "<kind>", "Classification for the shared backlog.")
            .choices(&["obstacle", "bug", "friction", "idea", "question"])
            .default("obstacle"),
        Arg::value("severity", "<severity>", "Observed impact.")
            .choices(&["blocker", "major", "minor", "trivial"])
            .default("minor"),
        Arg::value(
            "agent",
            "<name>",
            "Name of the reporting chatbot or coding agent; at most 200 characters.",
        )
        .required(),
        Arg::value(
            "model",
            "<name>",
            "Optional model name; at most 200 characters.",
        ),
        Arg::value(
            "client",
            "<name>",
            "Optional client name; at most 200 characters.",
        ),
        Arg::repeated(
            "evidence",
            "<observation>",
            "Bounded non-secret evidence; repeat up to 20 times.",
        ),
        Arg::repeated(
            "context",
            "<key=value>",
            "Bounded triage context; repeat for up to 24 unique keys.",
        ),
        ops::DESCRIPTOR_ARG,
    ],
    output: "\
The shared report, whether this sighting was merged into an existing open \
report, and the report's current occurrence count.",
    examples: &[Example {
        command: "ds feedback submit --title 'CLI cannot discover feeder export' --detail 'Expected a supported export command; live capability search and domain inspection found none. Acceptance: ds exposes and documents the export.' --component ds-cli --severity major --agent codex --evidence 'ds capabilities --search feeder-export matched 0' --yes --output json",
        note: "The report is written only after --yes and is deduplicated by the feedback service.",
        runnable: false,
    }],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        NOT_SIGNED_IN,
        INVALID_TEXT,
        INVALID_EVIDENCE,
        INVALID_CONTEXT,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a report written to the shared backlog",
            remedy: "re-run with --yes once the observed evidence is ready to submit",
        },
    ],
    reference: Some("docs/reference/feedback.md"),
    availability: ops::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let title = bounded_text(inputs.require("title")?, "title", MAX_TITLE_CHARS)?;
    let detail = bounded_detail(inputs.require("detail")?)?;
    let component = bounded_text(
        inputs.require("component")?,
        "component",
        MAX_COMPONENT_CHARS,
    )?;
    let agent = bounded_text(inputs.require("agent")?, "agent", MAX_AGENT_PART_CHARS)?;
    let model = optional_text(inputs.value("model"), "model", MAX_AGENT_PART_CHARS)?;
    let client = optional_text(inputs.value("client"), "client", MAX_AGENT_PART_CHARS)?;
    let evidence = parse_evidence(inputs.repeated("evidence"))?;
    let context = parse_context(inputs.repeated("context"))?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;

    let mut arguments = Map::from_iter([
        ("title".to_string(), Value::String(title.to_string())),
        ("detail".to_string(), Value::String(detail.to_string())),
        (
            "component".to_string(),
            Value::String(component.to_string()),
        ),
        (
            "kind".to_string(),
            Value::String(inputs.require("kind")?.to_string()),
        ),
        (
            "severity".to_string(),
            Value::String(inputs.require("severity")?.to_string()),
        ),
        ("agent".to_string(), Value::String(agent.to_string())),
        ("evidence".to_string(), json!(evidence)),
        ("context".to_string(), json!(context)),
    ]);
    if let Some(model) = model {
        arguments.insert("model".to_string(), Value::String(model.to_string()));
    }
    if let Some(client) = client {
        arguments.insert("client".to_string(), Value::String(client.to_string()));
    }

    ops::invoke(
        &descriptor,
        &crate::SUBMIT,
        Value::Object(arguments),
        TIMEOUT,
    )
    .map_err(ops::classify_signed_out)
}

fn bounded_text<'a>(value: &'a str, flag: &str, max: usize) -> Result<&'a str, Failure> {
    if value.is_empty() || value.trim() != value || value.chars().count() > max {
        return Err(Failure::invalid(
            "invalid_text",
            format!("`--{flag}` must be non-empty, trimmed, and at most {max} characters"),
        )
        .remedy(INVALID_TEXT.remedy));
    }
    Ok(value)
}

fn optional_text<'a>(
    value: Option<&'a str>,
    flag: &str,
    max: usize,
) -> Result<Option<&'a str>, Failure> {
    value
        .map(|value| bounded_text(value, flag, max))
        .transpose()
}

fn bounded_detail(value: &str) -> Result<&str, Failure> {
    if value.is_empty() || value.trim() != value || value.len() > MAX_DETAIL_BYTES {
        return Err(Failure::invalid(
            "invalid_text",
            format!("`--detail` must be non-empty, trimmed, and at most {MAX_DETAIL_BYTES} bytes"),
        )
        .remedy(INVALID_TEXT.remedy));
    }
    Ok(value)
}

fn parse_evidence(values: &[String]) -> Result<Vec<&str>, Failure> {
    if values.len() > MAX_EVIDENCE {
        return Err(Failure::invalid(
            "invalid_evidence",
            format!("`--evidence` may be repeated at most {MAX_EVIDENCE} times"),
        )
        .remedy(INVALID_EVIDENCE.remedy));
    }
    values
        .iter()
        .map(|value| {
            bounded_text(value, "evidence", MAX_EVIDENCE_CHARS).map_err(|_| {
                Failure::invalid(
                    "invalid_evidence",
                    format!(
                        "each `--evidence` must be trimmed and at most {MAX_EVIDENCE_CHARS} characters"
                    ),
                )
                .remedy(INVALID_EVIDENCE.remedy)
            })
        })
        .collect()
}

fn parse_context(values: &[String]) -> Result<BTreeMap<&str, &str>, Failure> {
    if values.len() > MAX_CONTEXT {
        return Err(Failure::invalid(
            "invalid_context",
            format!("`--context` may be repeated at most {MAX_CONTEXT} times"),
        )
        .remedy(INVALID_CONTEXT.remedy));
    }
    let mut parsed = BTreeMap::new();
    for value in values {
        let Some((key, value)) = value.split_once('=') else {
            return Err(Failure::invalid(
                "invalid_context",
                "each `--context` must be one key=value pair",
            )
            .remedy(INVALID_CONTEXT.remedy));
        };
        let key = bounded_text(key, "context", MAX_CONTEXT_KEY_CHARS).map_err(|_| {
            Failure::invalid("invalid_context", "context has an invalid key")
                .remedy(INVALID_CONTEXT.remedy)
        })?;
        let value = bounded_text(value, "context", MAX_CONTEXT_VALUE_CHARS).map_err(|_| {
            Failure::invalid("invalid_context", "context has an invalid value")
                .remedy(INVALID_CONTEXT.remedy)
        })?;
        if parsed.insert(key, value).is_some() {
            return Err(Failure::invalid(
                "invalid_context",
                format!("`--context` repeats the key `{key}`"),
            )
            .remedy(INVALID_CONTEXT.remedy));
        }
    }
    Ok(parsed)
}

pub fn render(data: &Value) -> String {
    let report = &data["report"];
    let id = report["id"].as_str().unwrap_or("recorded");
    let occurrences = report["occurrences"].as_u64().unwrap_or(1);
    if data["deduplicated"].as_bool().unwrap_or(false) {
        format!("feedback merged  {id} ({occurrences} sightings)\n")
    } else {
        format!("feedback recorded  {id}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_and_context_are_bounded() {
        let evidence = vec!["capabilities matched 0".to_string()];
        assert_eq!(
            parse_evidence(&evidence).unwrap(),
            ["capabilities matched 0"]
        );
        let context = vec!["operation=feeder.export".to_string()];
        assert_eq!(
            parse_context(&context).unwrap().get("operation"),
            Some(&"feeder.export")
        );
        assert!(parse_context(&["operation=a".into(), "operation=b".into()]).is_err());
    }

    #[test]
    fn bridge_contract_contains_only_report_fields() {
        assert_eq!(
            crate::SUBMIT.arguments,
            [
                "title",
                "detail",
                "component",
                "kind",
                "severity",
                "agent",
                "model",
                "client",
                "evidence",
                "context",
            ]
        );
    }
}
