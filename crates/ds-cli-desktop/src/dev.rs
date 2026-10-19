//! Bounded semantic controls for the active local development Transformer page.
//! Requests use the ordinary paired bridge; the native owner admits debug/local-dev.

use std::collections::BTreeSet;
use std::time::Duration;

use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires},
};
use serde::Serialize;
use serde_json::Value;

use crate::ops::{self, DESCRIPTOR_ARG, TARGET_ARG};

pub const MAX_ROWS: usize = 200;
pub const MAX_TRANSFORMER_CHARS: usize = 256;
const TIMEOUT: Duration = Duration::from_secs(120);
const INVALID_INPUT: Refusal = Refusal {
    code: "desktop_dev_invalid_input",
    when: "the project, exact transformer text, selection count or mode is invalid",
    remedy: "use an exact project and transformer from the active development Transformer page; select at most 200 names and use clear without names",
};
const PROJECT: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Exact project already open on the active Transformer page.",
)
.required();
const TRANSFORMER: Arg = Arg::value(
    "transformer",
    "<exact-name>",
    "One exact transformer name; 1..256 characters, without surrounding whitespace or controls.",
);
const REFUSALS: &[Refusal] = &[
    ops::NOT_PAIRED,
    ops::AMBIGUOUS,
    ops::TARGET_NOT_LIVE,
    ops::TARGET_MISMATCH,
    ops::UNKNOWN_TARGET,
    ops::HOST_UNSUPPORTED,
    ops::UNREACHABLE,
    ops::PAIRING_REJECTED,
    ops::REFUSED,
    ops::UNSUPPORTED,
    ops::UNREADABLE,
    ops::SIGNED_OUT,
    ops::PROJECT_NOT_OPEN,
    ops::CONTEXT_GENERATION_STALE,
    ds_cli_contract::args::INVALID_NUMBER,
    INVALID_INPUT,
];

#[allow(clippy::too_many_arguments)]
const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    purpose: &'static str,
    effect: Effect,
    args: &'static [Arg],
    output: &'static str,
    examples: &'static [Example],
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        chapter: Chapter::Operations,
        summary,
        purpose,
        effect,
        authority: Authority::DesktopPairing,
        execution: Execution::Sync,
        args,
        output,
        examples,
        refusals: REFUSALS,
        reference: None,
        search: &["developer", "transformer page", "ui diagnostics"],
        requires: Requires::Window,
        availability: ops::paired_availability,
    }
}

pub static SNAPSHOT: Command = command(
    "desktop.dev.snapshot",
    &["desktop", "dev", "snapshot"],
    "Read held Transformer page state in a development desktop.",
    "Read the active local development Transformer page state through the paired owner. The native host admits only a debug local-dev instance, and the active page and project must match. Reports bounded row metadata, selection and safe current errors. This read does not navigate, refresh, acquire data, or export files. These development controls are CLI-only and excluded from MCP.",
    Effect::ReadOnly,
    &[
        PROJECT,
        TRANSFORMER,
        Arg::value(
            "limit",
            "<count>",
            "Maximum held status rows to return; 1..200.",
        )
        .default("25"),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    "Active page/project, selection, bounded status rows and safe current errors; truncation is explicit.",
    &[Example {
        command: "ds desktop dev snapshot --project czgmdwth_gisagara --transformer Agahehe --output json",
        note: "Inspect the page's held report state without refreshing it.",
        runnable: false,
    }],
);
pub static SELECT: Command = command(
    "desktop.dev.select",
    &["desktop", "dev", "select"],
    "Select exact Transformer page rows in a development desktop.",
    "Change only the active local development Transformer page's real selection through its existing owner. The native host admits only a debug local-dev instance, and the active page and project must match. Replace or add at most 200 exact transformer names; clear accepts no names. Unknown names refuse without changing the selection. No project data is saved. These development controls are CLI-only and excluded from MCP.",
    Effect::LocalUi,
    &[
        PROJECT,
        Arg::repeated(
            "transformer",
            "<exact-name>",
            "Exact row name, repeated at most 200 times; each is 1..256 characters.",
        ),
        Arg::value(
            "mode",
            "<replace|add|clear>",
            "Apply one closed selection action.",
        )
        .choices(&["replace", "add", "clear"])
        .default("replace"),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    "The active page/project and actual resulting selection; project data is unchanged.",
    &[Example {
        command: "ds desktop dev select --project czgmdwth_gisagara --transformer Agahehe --output json",
        note: "Select the same exact row the page's controls select.",
        runnable: false,
    }],
);
pub static REFRESH: Command = command(
    "desktop.dev.refresh",
    &["desktop", "dev", "refresh"],
    "Refresh held Transformer page state in a development desktop.",
    "Run the active local development Transformer page's existing explicit Refresh action. The native host admits only a debug local-dev instance, and the active page and project must match. With an exact transformer, refresh that row through the existing owner; omit it for the page refresh. The ordinary read authority and coalescing apply, and a failed read retains prior held state and exposes its error. This does not export or publish a report. These development controls are CLI-only and excluded from MCP.",
    Effect::LocalUi,
    &[PROJECT, TRANSFORMER, TARGET_ARG, DESCRIPTOR_ARG],
    "The active page/project and the page's refresh outcome and safe current error.",
    &[Example {
        command: "ds desktop dev refresh --project czgmdwth_gisagara --transformer Agahehe --output json",
        note: "Refresh one exact row through the page's ordinary read action.",
        runnable: false,
    }],
);

#[derive(Serialize)]
pub struct SnapshotRequest<'a> {
    pub project: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transformer: Option<&'a str>,
    pub limit: usize,
}

#[derive(Serialize)]
pub struct SelectRequest<'a> {
    pub project: &'a str,
    pub transformers: Vec<&'a str>,
    pub mode: &'a str,
}

#[derive(Serialize)]
pub struct RefreshRequest<'a> {
    pub project: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transformer: Option<&'a str>,
}

fn transformer(raw: &str) -> Result<&str, Failure> {
    if raw.is_empty()
        || raw.trim() != raw
        || raw.chars().count() > MAX_TRANSFORMER_CHARS
        || raw.chars().any(char::is_control)
    {
        return Err(Failure::invalid(
            INVALID_INPUT.code,
            "transformer must be exact text of 1..256 characters without surrounding whitespace or controls",
        )
        .remedy(INVALID_INPUT.remedy));
    }
    Ok(raw)
}

fn snapshot_request(inputs: &Inputs) -> Result<SnapshotRequest<'_>, Failure> {
    Ok(SnapshotRequest {
        project: crate::project_id(inputs.require("project")?, INVALID_INPUT.code)?,
        transformer: inputs.value("transformer").map(transformer).transpose()?,
        limit: ds_cli_contract::args::integer(
            inputs.require("limit")?,
            "limit",
            1,
            MAX_ROWS as i64,
        )? as usize,
    })
}

fn select_request(inputs: &Inputs) -> Result<SelectRequest<'_>, Failure> {
    let project = crate::project_id(inputs.require("project")?, INVALID_INPUT.code)?;
    let mode = inputs.require("mode")?;
    let names = inputs.repeated("transformer");
    if !matches!(mode, "replace" | "add" | "clear")
        || names.len() > MAX_ROWS
        || (mode == "clear" && !names.is_empty())
        || (mode != "clear" && names.is_empty())
    {
        return Err(Failure::invalid(
            INVALID_INPUT.code,
            "replace/add require 1..200 transformer values; clear accepts none",
        )
        .remedy(INVALID_INPUT.remedy));
    }
    let mut seen = BTreeSet::new();
    let mut transformers = Vec::new();
    for name in names {
        let name = transformer(name)?;
        if seen.insert(name) {
            transformers.push(name);
        }
    }
    Ok(SelectRequest {
        project,
        transformers,
        mode,
    })
}

fn refresh_request(inputs: &Inputs) -> Result<RefreshRequest<'_>, Failure> {
    Ok(RefreshRequest {
        project: crate::project_id(inputs.require("project")?, INVALID_INPUT.code)?,
        transformer: inputs.value("transformer").map(transformer).transpose()?,
    })
}

fn invoke(inputs: &Inputs, op: &ops::BridgeOp, request: impl Serialize) -> Result<Value, Failure> {
    let arguments = serde_json::to_value(request)
        .map_err(|error| Failure::internal("desktop_dev_request_encoding", error.to_string()))?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(&descriptor, op, arguments, TIMEOUT).map_err(ops::classify_signed_out)
}

pub fn snapshot(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(inputs, &ops::DEV_SNAPSHOT, snapshot_request(inputs)?)
}

pub fn select(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(inputs, &ops::DEV_SELECT, select_request(inputs)?)
}

pub fn refresh(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(inputs, &ops::DEV_REFRESH, refresh_request(inputs)?)
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inputs(command: &Command, tokens: &[&str]) -> Inputs {
        ds_cli_contract::args::parse(
            command,
            &tokens
                .iter()
                .map(|token| (*token).to_owned())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn dev_snapshot_and_refresh_encode_only_the_closed_owner_payload() {
        let snapshot = inputs(&SNAPSHOT, &["--project", "project-a"]);
        assert_eq!(
            serde_json::to_value(snapshot_request(&snapshot).unwrap()).unwrap(),
            json!({"project":"project-a", "limit":25})
        );
        let snapshot = inputs(
            &SNAPSHOT,
            &[
                "--project",
                "project-a",
                "--transformer",
                "Agahehe",
                "--limit",
                "200",
            ],
        );
        assert_eq!(
            serde_json::to_value(snapshot_request(&snapshot).unwrap()).unwrap(),
            json!({"project":"project-a", "transformer":"Agahehe", "limit":200})
        );
        let refresh = inputs(
            &REFRESH,
            &["--project", "project-a", "--transformer", "Agahehe"],
        );
        assert_eq!(
            serde_json::to_value(refresh_request(&refresh).unwrap()).unwrap(),
            json!({"project":"project-a", "transformer":"Agahehe"})
        );
        let refresh = inputs(&REFRESH, &["--project", "project-a"]);
        assert_eq!(
            serde_json::to_value(refresh_request(&refresh).unwrap()).unwrap(),
            json!({"project":"project-a"})
        );
    }

    #[test]
    fn dev_select_preserves_exact_order_and_clear_is_explicit() {
        let select = inputs(
            &SELECT,
            &[
                "--project",
                "project-a",
                "--transformer",
                "Agahehe",
                "--transformer",
                "Bere",
                "--transformer",
                "Agahehe",
                "--mode",
                "add",
            ],
        );
        assert_eq!(
            serde_json::to_value(select_request(&select).unwrap()).unwrap(),
            json!({"project":"project-a", "transformers":["Agahehe","Bere"], "mode":"add"})
        );
        let clear = inputs(&SELECT, &["--project", "project-a", "--mode", "clear"]);
        assert_eq!(
            serde_json::to_value(select_request(&clear).unwrap()).unwrap(),
            json!({"project":"project-a", "transformers":[], "mode":"clear"})
        );
        for args in [
            vec!["--project", "project-a"],
            vec![
                "--project",
                "project-a",
                "--mode",
                "clear",
                "--transformer",
                "Agahehe",
            ],
        ] {
            assert_eq!(
                select_request(&inputs(&SELECT, &args))
                    .err()
                    .unwrap()
                    .code(),
                INVALID_INPUT.code
            );
        }
        let mut tokens = vec!["--project", "project-a"];
        for _ in 0..=MAX_ROWS {
            tokens.extend(["--transformer", "Agahehe"]);
        }
        assert_eq!(
            select_request(&inputs(&SELECT, &tokens))
                .err()
                .unwrap()
                .code(),
            INVALID_INPUT.code,
            "duplicate names cannot evade admission's input count bound"
        );
    }

    #[test]
    fn dev_requests_reject_bad_scope_names_and_limits_before_pairing() {
        for project in ["", " project-a", "project-a ", "project\n-a"] {
            assert_eq!(
                snapshot_request(&inputs(&SNAPSHOT, &["--project", project]))
                    .err()
                    .unwrap()
                    .code(),
                INVALID_INPUT.code
            );
            assert_eq!(
                refresh_request(&inputs(&REFRESH, &["--project", project]))
                    .err()
                    .unwrap()
                    .code(),
                INVALID_INPUT.code
            );
        }
        for name in ["", " Agahehe", "Agahehe ", "Agahehe\n"] {
            assert_eq!(transformer(name).err().unwrap().code(), INVALID_INPUT.code);
        }
        assert!(transformer(&"x".repeat(MAX_TRANSFORMER_CHARS)).is_ok());
        assert!(transformer(&"x".repeat(MAX_TRANSFORMER_CHARS + 1)).is_err());
        for limit in ["0", "201", "1.5", "anything"] {
            assert_eq!(
                snapshot_request(&inputs(
                    &SNAPSHOT,
                    &["--project", "project-a", "--limit", limit]
                ))
                .err()
                .unwrap()
                .code(),
                "invalid_number"
            );
        }
    }

    #[test]
    fn dev_command_schema_has_no_navigation_script_or_effect_fallback() {
        for command in [&SNAPSHOT, &SELECT, &REFRESH] {
            assert!(command.arg("project").unwrap().required);
            assert_eq!(command.authority, Authority::DesktopPairing);
            assert_eq!(command.requires, Requires::Window);
            for key in ["script", "selector", "url", "path", "page", "token"] {
                assert!(command.arg(key).is_none());
            }
            let tokens = vec![
                "--project".to_owned(),
                "project-a".to_owned(),
                "--script".to_owned(),
                "ignored".to_owned(),
            ];
            assert!(ds_cli_contract::args::parse(command, &tokens).is_err());
        }
        assert_eq!(SNAPSHOT.effect, Effect::ReadOnly);
        assert_eq!(SELECT.effect, Effect::LocalUi);
        assert_eq!(REFRESH.effect, Effect::LocalUi);
    }
}
