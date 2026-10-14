//! `ds map design batch save` — explicitly persist many staged rooms.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, TARGET_ARG};

const TRANSFORMER_ARG: Arg = Arg {
    name: "transformer",
    kind: ArgKind::Repeated,
    value: "<name>",
    required: true,
    default: None,
    choices: &[],
    summary: "Staged transformer to save. Repeat for the explicit batch.",
};

const PARALLEL_ARG: Arg = Arg {
    name: "parallel",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: None,
    choices: &[],
    summary: "Requested project saves (1-32); this bounded transport admits 1. Omit to use the app setting, which may refuse.",
};

pub static COMMAND: Command = Command {
    id: "map.design.batch.save",
    path: &["map", "design", "batch", "save"],
    contract: 1,
    summary: "Save an explicit batch of staged transformer rooms.",
    purpose: "\
Persists each named dirty desktop room with optimistic concurrency checks and \
v_first/v_last stamping from the deliberate Design Status version. Items are isolated so one conflict does not \
cancel unrelated saves. This durable project write always requires --yes.",
    chapter: Chapter::Design,
    effect: Effect::ArtifactWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, PARALLEL_ARG, TARGET_ARG, DESCRIPTOR_ARG],
    output: "\
Per-transformer saved/persisted state, design version, concurrency generation, or error, plus total, succeeded, \
failed, requested/effective parallelism and the enforced transport reason.",
    examples: &[Example {
        command: "ds map design batch save --transformer TX-1 --transformer TX-2 --parallel 1 --yes --output json",
        note: "Review staged rooms first; without --yes dispatch refuses before opening the bridge.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::PROJECT_NOT_OPEN,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::OFFLINE,
        crate::INVALID_NUMBER,
        Refusal {
            code: "parallelism_unavailable",
            when: "the requested or app-default concurrency exceeds the serial acknowledged save transport",
            remedy: "retry with --parallel 1; no project rooms were read or saved",
        },
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a command that writes to the project",
            remedy: "re-run with --yes once you intend the batch write",
        },
    ],
    reference: Some("docs/reference/map.md"),
    search: &[],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("transformers".into(), json!(inputs.repeated("transformer")));
    if let Some(parallel) = inputs.value("parallel") {
        let requested = crate::integer(parallel, "parallel", 1, 32)?;
        if requested != 1 {
            return Err(Failure::unavailable(
                "parallelism_unavailable",
                "Batch saves admit one acknowledged request at a time under the existing count/byte transport bounds.",
            ).remedy("retry with --parallel 1; no Desktop was discovered and no project rooms were read or saved")
             .detail(json!({ "requested_parallelism": requested, "maximum_parallelism": 1, "reason": "bounded_acknowledged_save_transport" })));
        }
        arguments.insert("parallel".into(), json!(requested));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::DESIGN_SAVE_BATCH,
        Value::Object(arguments),
        crate::DESIGN_PROCESS_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} transformer(s): {} completed, {} failed · {} requested / {} effective parallel\n",
        data["total"].as_u64().unwrap_or(0),
        data["succeeded"].as_u64().unwrap_or(0),
        data["failed"].as_u64().unwrap_or(0),
        data["requestedParallel"]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "not reported".to_string()),
        data["effectiveParallel"]
            .as_u64()
            .or_else(|| data["parallel"].as_u64())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "not reported".to_string()),
    );
    if let Some(rows) = data["rows"].as_array() {
        for row in rows {
            out.push_str(&format!(
                "  {} {}{}\n",
                if row["ok"].as_bool().unwrap_or(false) {
                    "✓"
                } else {
                    "✗"
                },
                row["transformer"].as_str().unwrap_or("?"),
                row["error"]
                    .as_str()
                    .or_else(|| row["reason"].as_str())
                    .map(|message| format!("  {message}"))
                    .unwrap_or_default(),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::{Format, Output, parse};

    fn inputs(parallel: &str) -> Inputs {
        parse(
            &COMMAND,
            &[
                "--transformer".to_string(),
                "TX-1".to_string(),
                "--parallel".to_string(),
                parallel.to_string(),
                // The baseline must never discover or send to a real Desktop.
                "--desktop-descriptor".to_string(),
                "/nonexistent/ds-test-no-desktop.json".to_string(),
            ],
        )
        .unwrap()
    }
    fn context() -> Context {
        Context {
            confirmed: true,
            output: Output::resolve(Format::Json, false, true),
        }
    }

    #[test]
    fn requested_parallel_save_is_refused_before_desktop_discovery() {
        let error = run(&inputs("4"), &context()).unwrap_err();
        assert_eq!(error.code(), "parallelism_unavailable");
        assert_eq!(error.detail_value().unwrap()["requested_parallelism"], 4);
        assert_eq!(error.detail_value().unwrap()["maximum_parallelism"], 1);
    }

    #[test]
    fn one_admitted_request_reaches_desktop_discovery() {
        assert_eq!(
            run(&inputs("1"), &context()).unwrap_err().code(),
            "desktop_unreachable"
        );
    }

    #[test]
    fn renderer_never_invents_unreported_concurrency() {
        assert!(
            render(&json!({ "total": 1, "succeeded": 1, "failed": 0 }))
                .contains("not reported requested / not reported effective parallel")
        );
        assert!(
            render(&json!({
                "total": 1, "succeeded": 1, "failed": 0,
                "requestedParallel": 1, "effectiveParallel": 1
            }))
            .contains("1 requested / 1 effective parallel")
        );
    }

    #[test]
    fn save_refusal_is_discoverable_and_keeps_numeric_bounds() {
        assert!(
            COMMAND
                .refusals
                .iter()
                .any(|refusal| refusal.code == "parallelism_unavailable")
        );
        assert_eq!(
            run(&inputs("33"), &context()).unwrap_err().code(),
            "invalid_number"
        );
        assert_eq!(
            run(&inputs("0"), &context()).unwrap_err().code(),
            "invalid_number"
        );
    }
}
