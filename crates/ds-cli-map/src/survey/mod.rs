//! Exact, governed survey-data copy from one project into the active project.
//!
//! This is deliberately narrower than the application's migration API. The
//! CLI copies every survey entry, preserves the source, never overwrites an
//! existing target id, and can only target the signed-in desktop's active
//! project. Expanding any of those consequences requires a new reviewed
//! contract rather than another loosely interpreted flag.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

const SOURCE_PROJECT_ARG: Arg = Arg::value(
    "source-project",
    "<project-id>",
    "Copy survey entries from this project into the active project.",
)
.required();

const INVALID_PROJECT: Refusal = Refusal {
    code: "invalid_project",
    when: "the source project id is empty, padded, too long, or not canonical",
    remedy: "pass the exact Data Solutions project id shown by project discovery",
};

const SAME_PROJECT: Refusal = Refusal {
    code: "same_project",
    when: "the source project is also the active target project",
    remedy: "open the intended target project in DS GridDesign, then run the plan again",
};

const DESKTOP_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "the project is unavailable or the governed survey migration API refuses the request",
    remedy: "read detail.detail for the application's exact API refusal",
};

const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for the survey-data write",
    remedy: "run the plan first, then re-run apply with --yes once its totals are intended",
};

fn source_project(inputs: &Inputs) -> Result<&str, Failure> {
    let source = inputs.require("source-project")?;
    let canonical = source.len() <= 160
        && source.trim() == source
        && source
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
    if !canonical {
        return Err(Failure::invalid(
            "invalid_project",
            "--source-project is not a canonical project id",
        )
        .remedy(INVALID_PROJECT.remedy));
    }
    Ok(source)
}

fn invoke_migration(inputs: &Inputs, operation: &crate::BridgeOp) -> Result<Value, Failure> {
    let source = source_project(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        operation,
        json!({ "sourceProject": source }),
        crate::SURVEY_MIGRATION_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
    .map_err(classify_same_project)
}

fn classify_same_project(failure: Failure) -> Failure {
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|value| value["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !detail.contains("same project") {
        return failure;
    }
    Failure::invalid(
        "same_project",
        "the survey migration source and active target are the same project",
    )
    .remedy(SAME_PROJECT.remedy)
}

fn receipt(result: Value) -> Value {
    json!({
        "source_project": result["sourceProject"],
        "target_project": result["targetProject"],
        "dry_run": result["dryRun"],
        "status": result["status"],
        "total_matched": result["totalMatched"],
        "total_migrated": result["totalMigrated"],
        "total_skipped": result["totalSkipped"],
        "total_target_written": result["totalTargetWritten"],
        "total_source_deleted": result["totalSourceDeleted"],
        "per_form": result["perForm"],
        "skip_reasons": result["skipReasons"],
        "more": result["more"],
        "source_preserved": true,
        "overwrite_existing": false,
    })
}

fn render_receipt(data: &Value, planned: bool) -> String {
    let verb = if planned { "would copy" } else { "copied" };
    let count = if planned {
        data["total_migrated"].as_u64().unwrap_or(0)
    } else {
        data["total_target_written"]
            .as_u64()
            .or_else(|| data["total_migrated"].as_u64())
            .unwrap_or(0)
    };
    let mut output = format!(
        "{verb} {count} survey entr{}  {} -> {}\n  matched {}  ·  skipped {}\n  source preserved  ·  existing target ids not overwritten\n",
        if count == 1 { "y" } else { "ies" },
        data["source_project"].as_str().unwrap_or("?"),
        data["target_project"].as_str().unwrap_or("?"),
        data["total_matched"].as_u64().unwrap_or(0),
        data["total_skipped"].as_u64().unwrap_or(0),
    );
    if planned {
        output.push_str("  preview only; run `ds map survey migrate apply --source-project <project> --yes` to write\n");
    }
    output
}

pub mod download {
    use super::*;

    const ENTIRE_PROJECT_ARG: Arg = Arg::switch(
        "entire-project",
        "Explicitly materialize every survey form in the active project Working Area.",
    )
    .required();

    pub static COMMAND: Command = Command {
        id: "map.survey.download",
        path: &["map", "survey", "download"],
        contract: 1,
        summary: "Materialize survey data through the active Working Area.",
        purpose: "Asks the paired desktop to apply its existing full-project Working Area and sequentially materialize every survey form into the desktop-owned IndexedDB cache. Rust sends only explicit intent and receives bounded counts; it never queries or interprets survey rows.",
        chapter: Chapter::Survey,
        effect: Effect::LocalUi,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[ENTIRE_PROJECT_ARG, DESCRIPTOR_ARG],
        output: "The active project, applied full-project Working Area, form count, and bounded before/after/materialized cache counts. No survey row is returned.",
        examples: &[Example {
            command: "ds map survey download --entire-project --output json",
            note: "Uses the same loader as checking Load entire project survey data and applying Working Area.",
            runnable: false,
        }],
        refusals: &[
            crate::NOT_PAIRED,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            DESKTOP_REFUSED,
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            crate::SIGNED_OUT,
        ],
        reference: Some("docs/reference/map.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::SURVEY_WORKING_AREA_DOWNLOAD,
            json!({ "entireProject": inputs.switch("entire-project") }),
            crate::SURVEY_DOWNLOAD_TIMEOUT,
        )
        .map(receipt)
        .map_err(crate::classify_design_failure)
    }

    fn receipt(result: Value) -> Value {
        json!({
            "project": result["project"],
            "working_area": result["workingArea"],
            "forms": result["forms"],
            "before": result["before"],
            "after": result["after"],
            "cached_total": result["cachedTotal"],
            "materialized": result["materialized"],
            "materialized_total": result["materializedTotal"],
            "rows_returned": 0,
        })
    }

    pub fn render(data: &Value) -> String {
        format!(
            "survey cache materialized  {}\n  forms {}  ·  cached {}  ·  newly materialized {}\n  Working Area: full project  ·  raw rows returned: 0\n",
            data["project"].as_str().unwrap_or("?"),
            data["forms"].as_u64().unwrap_or(0),
            data["cached_total"].as_u64().unwrap_or(0),
            data["materialized_total"].as_u64().unwrap_or(0),
        )
    }
}

pub mod plan {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "map.survey.migrate.plan",
        path: &["map", "survey", "migrate", "plan"],
        contract: 1,
        summary: "Preview copying all survey data into the active project.",
        purpose: "Calls the governed survey migration API in dry-run mode. It previews copying every survey entry from --source-project into the signed-in desktop's active project. The source is preserved and existing target ids are skipped; it cannot overwrite, delete, filter, or choose another target.",
        chapter: Chapter::Survey,
        effect: Effect::ReadOnly,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[SOURCE_PROJECT_ARG, DESCRIPTOR_ARG],
        output: "The source and active target projects; matched, copyable and skipped totals; bounded per-form and skip-reason counts; and the fixed source-preserved/no-overwrite policy.",
        examples: &[Example {
            command: "ds map survey migrate plan --source-project arjgpydw_huye2 --output json",
            note: "Uses the API's real dry run and writes nothing.",
            runnable: false,
        }],
        refusals: &[
            crate::NOT_PAIRED,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            DESKTOP_REFUSED,
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            crate::SIGNED_OUT,
            INVALID_PROJECT,
            SAME_PROJECT,
        ],
        reference: Some("docs/reference/map.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        super::invoke_migration(inputs, &crate::SURVEY_MIGRATE_PLAN).map(super::receipt)
    }

    pub fn render(data: &Value) -> String {
        super::render_receipt(data, true)
    }
}

pub mod apply {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "map.survey.migrate.apply",
        path: &["map", "survey", "migrate", "apply"],
        contract: 1,
        summary: "Copy all survey data into the active project.",
        purpose: "Calls the governed survey migration API to copy every survey entry from --source-project into the signed-in desktop's active project. The source is preserved and existing target ids are skipped. Dispatch requires --yes, and this command cannot overwrite, delete, filter, or choose another target.",
        chapter: Chapter::Survey,
        effect: Effect::GlobalWrite,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[SOURCE_PROJECT_ARG, DESCRIPTOR_ARG],
        output: "The source and active target projects; matched, written and skipped totals; bounded per-form and skip-reason counts; and confirmation that the source was preserved.",
        examples: &[Example {
            command: "ds map survey migrate apply --source-project arjgpydw_huye2 --yes --output json",
            note: "Copy only after reviewing the plan receipt.",
            runnable: false,
        }],
        refusals: &[
            crate::NOT_PAIRED,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            DESKTOP_REFUSED,
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            crate::SIGNED_OUT,
            INVALID_PROJECT,
            SAME_PROJECT,
            CONFIRMATION_REQUIRED,
        ],
        reference: Some("docs/reference/map.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        super::invoke_migration(inputs, &crate::SURVEY_MIGRATE_APPLY).map(super::receipt)
    }

    pub fn render(data: &Value) -> String {
        super::render_receipt(data, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_project_is_the_only_migration_operand() {
        assert_eq!(plan::COMMAND.args[0].name, "source-project");
        assert_eq!(apply::COMMAND.effect, Effect::GlobalWrite);
        assert!(apply::COMMAND.effect.needs_confirmation());
        assert_eq!(crate::SURVEY_MIGRATE_APPLY.arguments, &["sourceProject"]);
    }

    #[test]
    fn working_area_download_declares_only_explicit_intent() {
        assert_eq!(download::COMMAND.effect, Effect::LocalUi);
        assert!(download::COMMAND.args[0].required);
        assert_eq!(
            crate::SURVEY_WORKING_AREA_DOWNLOAD.arguments,
            &["entireProject"]
        );
    }
}
