//! Selected-project Settings. The kernel owns model, coercion and mutation;
//! client-core owns authenticated fetch/write/readback. This is host IO only.
use ds_cli_contract::{
    Context, Inputs,
    outcome::Failure,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal},
};
use ds_client_core::ProjectConfigurationChange as Change;
use ds_command_kernel::design_config::{self, Request};
use serde_json::{Value, json};
use std::io::{Read, Write};
const LANE: Arg = Arg::value("lane", "<stable|canary>", "Deployment lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
const SHEET: Arg = Arg::value("sheet", "<key>", "Exact sheet key from config sheets.").required();
const LIMIT: Arg = Arg::value("limit", "<n>", "Maximum returned items, 1–100.").default("50");
const OFFSET: Arg = Arg::value("offset", "<n>", "Skip this many items.").default("0");
const RULE_SET: Arg = Arg::value(
    "rule-set",
    "<name>",
    "Rule set to read; defaults to the first modeled set.",
);
const OUT: Arg = Arg::value(
    "out",
    "<path>",
    "Retain the complete sheet at a new local JSON path.",
);
const FILE: Arg = Arg::value("file", "<path>", "JSON sheet value, at most 16 MiB.").required();
const PARAMETER: Arg = Arg::value(
    "parameter",
    "<name>",
    "Existing parameter; kernel canonical name matching.",
)
.required();
const VALUE: Arg = Arg::value(
    "value",
    "<text>",
    "Value using the same control coercion as Settings.",
)
.required();
const SOURCE: Arg = Arg::value("source", "<name>", "Existing rule set to copy.").required();
const TARGET: Arg = Arg::value(
    "target",
    "<name>",
    "New rule set name; normalized collisions refuse.",
)
.required();
const REFUSALS: &[Refusal] = &[
    ds_cli_report::project::NATIVE_PROFILE,
    ds_cli_report::project::NATIVE_PROFILE_DIGEST,
    ds_cli_report::project::NATIVE_PROFILE_UNSAFE,
    ds_cli_report::project::HEADLESS_SIGNED_OUT,
    ds_cli_report::project::HEADLESS_NO_PROJECT,
    ds_cli_report::project::PROJECT_CONTEXT_STALE,
    ds_cli_report::project::NATIVE_STATE_UNSAFE,
    ds_cli_report::project::NATIVE_STATE_UNAVAILABLE,
    ds_cli_report::project::NATIVE_STATE_PROTECTION,
    ds_cli_report::project::NATIVE_STATE_ROOT,
    ds_cli_report::project::NATIVE_STATE_CONFLICT,
    ds_cli_report::project::NATIVE_CLEANUP,
    ds_cli_report::project::AUTH_CONTEXT_MISMATCH,
    ds_cli_report::project::AUTH_REVOKED,
    ds_cli_report::project::AUTH_IDENTITY_MISMATCH,
    Refusal {
        code: "config_input_invalid",
        when: "file, paging or kernel request is invalid",
        remedy: "inspect config sheets/read and pass a bounded JSON sheet or valid page",
    },
    Refusal {
        code: "auth_input_invalid",
        when: "the kernel refuses a sheet or parameter mutation",
        remedy: "read the current sheet; correct the named parameter, shape or rule-set collision",
    },
    Refusal {
        code: "auth_rejected",
        when: "the gateway refuses configuration access",
        remedy: "verify the selected account has project configuration permission",
    },
    Refusal {
        code: "auth_transient",
        when: "configuration IO is unavailable",
        remedy: "read current state before retrying an uncertain write",
    },
    Refusal {
        code: "auth_response_unreadable",
        when: "configuration or fresh saved readback is invalid",
        remedy: "read current state and report the mismatched receipt",
    },
    Refusal {
        code: "configuration_output_exists",
        when: "the output path already exists",
        remedy: "choose a new path",
    },
    Refusal {
        code: "configuration_output_write_failed",
        when: "the sheet cannot be retained locally",
        remedy: "choose a writable new path",
    },
    Refusal {
        code: "confirmation_required",
        when: "a Settings write lacks --yes",
        remedy: "review the requested change and pass --yes",
    },
];
const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    effect: Effect,
    args: &'static [Arg],
) -> Command {
    Command {
        id,
        path,
        summary,
        contract: 1,
        chapter: Chapter::Design,
        authority: Authority::HeadlessProject,
        effect,
        execution: Execution::Sync,
        purpose: "Use the selected project's fresh Settings through the native user client. The shared kernel decides sheet shape, parameter coercion and rule-set edits. Every write verifies fresh readback. No Desktop or project override.",
        args,
        output: "Project and bounded kernel projection, or saved state verified by fresh readback. more reports omitted items; read --out retains a complete sheet.",
        examples: &[],
        refusals: REFUSALS,
        reference: Some("docs/reference/design.md"),
        availability: ds_cli_auth::native_availability,
    }
}
pub static SHEETS: Command = command(
    "design.config.sheets",
    &["design", "config", "sheets"],
    "List the project's Settings sheets and controls.",
    Effect::LocalAuthState,
    &[LANE, LIMIT, OFFSET],
);
pub static READ: Command = command(
    "design.config.read",
    &["design", "config", "read"],
    "Read one Settings sheet, with optional complete JSON output.",
    Effect::LocalFileWrite,
    &[LANE, SHEET, RULE_SET, LIMIT, OFFSET, OUT],
);
pub static DIFF: Command = command(
    "design.config.diff",
    &["design", "config", "diff"],
    "Compare a local sheet with the project's current Settings.",
    Effect::LocalAuthState,
    &[LANE, SHEET, FILE, LIMIT, OFFSET],
);
pub static SET: Command = command(
    "design.config.set",
    &["design", "config", "set"],
    "Set one existing Settings parameter and verify readback.",
    Effect::GlobalWrite,
    &[LANE, SHEET, PARAMETER, VALUE],
);
pub static SAVE: Command = command(
    "design.config.save",
    &["design", "config", "save"],
    "Save one validated Settings sheet and verify readback.",
    Effect::GlobalWrite,
    &[LANE, SHEET, FILE],
);
pub static DUPLICATE: Command = command(
    "design.config.rule-set.duplicate",
    &["design", "config", "rule-set", "duplicate"],
    "Duplicate a rule set, preserving its rows and metadata.",
    Effect::GlobalWrite,
    &[LANE, SHEET, SOURCE, TARGET],
);
fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("config_input_invalid", e.to_string())
}
fn file(path: &str) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(invalid)?
        .take(design_config::MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > design_config::MAX_INPUT_BYTES {
        return Err(invalid("sheet exceeds 16 MiB"));
    }
    serde_json::from_slice(&bytes).map_err(invalid)
}
fn read_current(i: &Inputs) -> Result<ds_client_core::FeederConfiguration, Failure> {
    ds_cli_auth::settings_configuration(i.require("lane")?, Change::ReadSettings)
}
fn page(i: &Inputs) -> Result<(usize, usize), Failure> {
    let limit = i.require("limit")?.parse::<usize>().map_err(invalid)?;
    let offset = i.require("offset")?.parse::<usize>().map_err(invalid)?;
    if !(1..=100).contains(&limit) {
        return Err(invalid("limit must be 1–100"));
    }
    Ok((offset, limit))
}

fn sheet<'a>(doc: &'a Value, key: &str) -> Result<&'a Value, Failure> {
    doc["sheets"]
        .get(key)
        .ok_or_else(|| invalid("config_sheet_unknown"))
}
pub fn sheets(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (offset, limit) = page(i)?;
    let current = read_current(i)?;
    let mut result = design_config::apply(Request::List {
        document: current.document,
        offset,
        limit,
    })
    .map_err(invalid)?;
    result["project"] = current.summary["project"].clone();
    Ok(result)
}
pub fn read(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (offset, limit) = page(i)?;
    let key = i.require("sheet")?;
    if i.value("out")
        .is_some_and(|p| std::path::Path::new(p).exists())
    {
        return Err(Failure::invalid(
            "configuration_output_exists",
            "output path already exists",
        ));
    }
    let current = read_current(i)?;
    let value = sheet(&current.document, key)?;
    let mut result = design_config::apply(Request::Read {
        document: current.document.clone(),
        sheet_key: key.into(),
        rule_set: i.value("rule-set").map(str::to_owned),
        offset,
        limit,
    })
    .map_err(invalid)?;
    result["project"] = current.summary["project"].clone();
    result["sheet"] = json!(key);
    if let Some(path) = i.value("out") {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| {
                Failure::invalid("configuration_output_write_failed", "cannot create output")
            })?;
        out.write_all(&serde_json::to_vec_pretty(value).map_err(invalid)?)
            .and_then(|_| out.sync_all())
            .map_err(|_| {
                Failure::invalid("configuration_output_write_failed", "cannot retain sheet")
            })?;
        result["path"] = json!(path)
    }
    Ok(result)
}
pub fn diff(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (offset, limit) = page(i)?;
    let baseline = file(i.require("file")?)?;
    let current = read_current(i)?;
    let mut result = design_config::apply(Request::DiffPage {
        baseline,
        current: sheet(&current.document, i.require("sheet")?)?.clone(),
        offset,
        limit,
    })
    .map_err(invalid)?;
    result["project"] = current.summary["project"].clone();
    Ok(result)
}
pub fn set(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let change = Change::SetParameter {
        sheet: i.require("sheet")?.into(),
        parameter: i.require("parameter")?.into(),
        raw: json!(i.require("value")?),
    };
    Ok(ds_cli_auth::settings_configuration(i.require("lane")?, change)?.summary)
}
pub fn save(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let change = Change::SaveSheet {
        sheet: i.require("sheet")?.into(),
        value: file(i.require("file")?)?,
    };
    Ok(ds_cli_auth::settings_configuration(i.require("lane")?, change)?.summary)
}
pub fn duplicate(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let change = Change::DuplicateRuleSet {
        sheet: i.require("sheet")?.into(),
        source: i.require("source")?.into(),
        target: i.require("target")?.into(),
    };
    Ok(ds_cli_auth::settings_configuration(i.require("lane")?, change)?.summary)
}
pub fn render(data: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(data).expect("JSON settings")
    )
}
