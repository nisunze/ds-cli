//! Selected-project Settings. The kernel owns model, coercion and mutation;
//! client-core owns authenticated fetch/write/readback. This is host IO only.
use ds_cli_contract::{
    outcome::Failure,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
    Context, Inputs,
};
use ds_client_core::ProjectConfigurationChange as Change;
use ds_command_kernel::design_config::{self, Request};
use ds_command_kernel::gateway_exchange::design::{
    ProjectConfigurationRead, ProjectConfigurationReply,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
const LANE: ds_cli_contract::spec::Arg = ds_cli_contract::spec::LANE.summary("Deployment lane.");
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
    "Retain the complete sheet or --document configuration at a new local JSON path.",
);
const DOCUMENT: Arg = Arg::switch(
    "document",
    "Read the complete configuration instead of one paged sheet.",
);
const READ_MODE: Arg = Arg::value("read", "<held|refresh>", "held reads SQLite only; refresh performs one explicit project configuration read and retains it.")
    .choices(ProjectConfigurationRead::NAMES).default("refresh");
const FILE: Arg = Arg::value("file", "<path>", "JSON sheet value, at most 16 MiB.").required();
const PARAMETER: Arg = Arg::value(
    "parameter",
    "<name>",
    "Existing parameter or governed map printing default.",
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
    ds_cli_auth::PROJECT_CONFIGURATION_MISSING_REFUSAL,
    ds_cli_auth::config_document::NOT_HELD,
    ds_cli_auth::config_document::CONTEXT_CHANGED,
    ds_cli_auth::config_document::STORE_REFUSED,
    ds_cli_auth::config_document::STALE,
    ds_cli_auth::config_document::ANSWER_INVALID,
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
        purpose: "Inspect and correct project configuration seeds, including pole material tables, through the signed-in native client without a paired Desktop. Discover exact sheet keys with design config sheets; read --out retains complete JSON, diff reviews edits, and save validates the edited sheet and verifies fresh readback. Seed changes and transformer reprocessing are separate operations. The shared kernel owns sheet shape and parameter coercion.",
        args,
        output: "Project and bounded kernel projection, or saved state verified by fresh readback. more reports omitted items; read --out retains a complete sheet.",
        examples: &[],
        refusals: REFUSALS,
        reference: Some("docs/reference/design.md"),
        search: &[],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    }
}
pub static SHEETS: Command = command(
    "design.config.sheets",
    &["design", "config", "sheets"],
    "Find project configuration and pole material seed sheets.",
    Effect::LocalAuthState,
    &[crate::PROJECT_ARG, LANE, LIMIT, OFFSET],
);
pub static READ: Command = Command {
    purpose: "Read one project configuration sheet, or select --document for its complete shared Rust document. --document --read held uses only Core SQLite under the retained account; --read refresh performs one explicit native configuration GET and retains it for later offline opens. --out writes complete JSON to a new private file. Sheet mode preserves bounded Settings projection and rule-set selection.",
    output: "Sheet mode: bounded items, total, more, truncated and project. Document mode: project, document and observation {source, saved_at_ms}; with --out the complete document is the output file and data contains project, observation and path.",
    ..command(
    "design.config.read",
    &["design", "config", "read"],
    "Read project configuration or material seeds as complete JSON.",
    Effect::LocalFileWrite,
    &[
        crate::PROJECT_ARG,
        LANE,
        Arg::value(
            "sheet",
            "<key>",
            "Exact sheet key; required unless --document is selected.",
        ),
        DOCUMENT,
        READ_MODE,
        RULE_SET,
        LIMIT,
        OFFSET,
        OUT,
    ],
)
};
pub static DIFF: Command = command(
    "design.config.diff",
    &["design", "config", "diff"],
    "Compare a local sheet with the project's current Settings.",
    Effect::LocalAuthState,
    &[crate::PROJECT_ARG, LANE, SHEET, FILE, LIMIT, OFFSET],
);
pub static SET: Command = Command {
    purpose: "Set one project Settings parameter and verify readback through the signed-in native client. Governed map printing defaults can be authored before a row exists: keep_all_labels_at_high_density is false; print_metric_grid_enabled is true; print_metric_grid_interval_m is automatic (use automatic or positive metres). Every map print reads these settings. Automatic spacing uses its actual frame/scale and round 1/2/2.5/5 steps per decade for about 4–8 lines. Other parameters must already exist. Read/diff first; --yes saves only the named project.",
    search: &["printing", "labels", "metric grid", "density"],
    ..command(
        "design.config.set",
        &["design", "config", "set"],
        "Set a project Settings parameter and verify readback.",
        Effect::GlobalWrite,
        &[crate::PROJECT_ARG, LANE, SHEET, PARAMETER, VALUE],
    )
};
pub static SAVE: Command = command(
    "design.config.save",
    &["design", "config", "save"],
    "Save corrected material seeds or configuration; verify readback.",
    Effect::GlobalWrite,
    &[crate::PROJECT_ARG, LANE, SHEET, FILE],
);
pub static DUPLICATE: Command = command(
    "design.config.rule-set.duplicate",
    &["design", "config", "rule-set", "duplicate"],
    "Duplicate a rule set, preserving its rows and metadata.",
    Effect::GlobalWrite,
    &[crate::PROJECT_ARG, LANE, SHEET, SOURCE, TARGET],
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
    ds_cli_auth::settings_configuration(
        i.require("lane")?,
        i.require("project")?,
        Change::ReadSettings,
    )
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
    read_with(i, ds_cli_auth::config_document::read)
}
fn read_with(
    i: &Inputs,
    document: impl FnOnce(
        &str,
        &str,
        ProjectConfigurationRead,
    ) -> Result<ProjectConfigurationReply, Failure>,
) -> Result<Value, Failure> {
    let (offset, limit) = page(i)?;
    if i.switch("document")
        && (i.value("sheet").is_some()
            || i.value("rule-set").is_some()
            || offset != 0
            || limit != 50)
    {
        return Err(invalid(
            "--document cannot select a sheet, rule set or page",
        ));
    }
    let key = if i.switch("document") {
        None
    } else {
        Some(
            i.value("sheet")
                .ok_or_else(|| invalid("--sheet is required unless --document is selected"))?,
        )
    };
    if !i.switch("document") && i.value("read") == Some("held") {
        return Err(invalid("--read held requires --document"));
    }
    if i.value("out")
        .is_some_and(|p| std::path::Path::new(p).exists())
    {
        return Err(Failure::invalid(
            "configuration_output_exists",
            "output path already exists",
        ));
    }
    if i.switch("document") {
        let mode = serde_json::from_value::<ProjectConfigurationRead>(json!(i.require("read")?))
            .map_err(invalid)?;
        let answer = document(i.require("lane")?, i.require("project")?, mode)?;
        if answer.project != i.require("project")? || answer.observation.source != mode {
            return Err(invalid(
                "configuration receipt does not answer this project and read mode",
            ));
        }
        if let Some(path) = i.value("out") {
            write_output(
                path,
                &serde_json::to_vec_pretty(&answer.document).map_err(invalid)?,
            )?;
            return Ok(
                json!({"project":answer.project,"observation":answer.observation,"path":path}),
            );
        }
        return serde_json::to_value(answer).map_err(invalid);
    }
    let key = key.expect("sheet was admitted before acquisition");
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
        write_output(path, &serde_json::to_vec_pretty(value).map_err(invalid)?)?;
        result["path"] = json!(path)
    }
    Ok(result)
}
fn write_output(path: &str, bytes: &[u8]) -> Result<(), Failure> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(path).map_err(|_| {
        Failure::invalid("configuration_output_write_failed", "cannot create output")
    })?;
    out.write_all(bytes)
        .and_then(|_| out.sync_all())
        .map_err(|_| {
            Failure::invalid(
                "configuration_output_write_failed",
                "cannot retain configuration",
            )
        })
}
pub fn diff(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (offset, limit) = page(i)?;
    let edited = file(i.require("file")?)?;
    let stored = read_current(i)?;
    let stored_sheet = sheet(&stored.document, i.require("sheet")?)?.clone();
    let mut result = diff_page(stored_sheet, edited, offset, limit)?;
    result["project"] = stored.summary["project"].clone();
    Ok(result)
}
/// The stored sheet is "before" and the edited file "after", so a change
/// reads the way `save` would apply it.
fn diff_page(stored: Value, edited: Value, offset: usize, limit: usize) -> Result<Value, Failure> {
    design_config::apply(Request::DiffPage {
        baseline: stored,
        current: edited,
        offset,
        limit,
    })
    .map_err(invalid)
}
pub fn set(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let change = Change::SetParameter {
        sheet: i.require("sheet")?.into(),
        parameter: i.require("parameter")?.into(),
        raw: json!(i.require("value")?),
    };
    Ok(
        ds_cli_auth::settings_configuration(i.require("lane")?, i.require("project")?, change)?
            .summary,
    )
}
pub fn save(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let change = Change::SaveSheet {
        sheet: i.require("sheet")?.into(),
        value: file(i.require("file")?)?,
    };
    Ok(
        ds_cli_auth::settings_configuration(i.require("lane")?, i.require("project")?, change)?
            .summary,
    )
}
pub fn duplicate(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let change = Change::DuplicateRuleSet {
        sheet: i.require("sheet")?.into(),
        source: i.require("source")?.into(),
        target: i.require("target")?.into(),
    };
    Ok(
        ds_cli_auth::settings_configuration(i.require("lane")?, i.require("project")?, change)?
            .summary,
    )
}
pub fn render(data: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(data).expect("JSON settings")
    )
}

#[cfg(test)]
mod tests {
    use super::diff_page;
    use serde_json::json;

    #[test]
    fn a_diff_reads_the_stored_value_as_before_and_the_edit_as_after() {
        let stored = json!([{"transfo_sizes": 315, "mv_fuze_15_kv_amp": 16}]);
        let edited = json!([{"transfo_sizes": 315, "mv_fuze_15_kv_amp": 99}]);
        let page = diff_page(stored, edited, 0, 50).unwrap();
        let items = page["items"].as_array().expect("diff items");
        assert_eq!(items.len(), 1, "{page}");
        assert_eq!(items[0]["path"], "/0/mv_fuze_15_kv_amp");
        assert_eq!(items[0]["before"], 16);
        assert_eq!(items[0]["after"], 99);
    }
}

#[cfg(test)]
#[path = "config_document_tests.rs"]
mod document_tests;
