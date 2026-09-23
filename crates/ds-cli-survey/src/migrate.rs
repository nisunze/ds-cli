//! `ds survey migrate` — copy a project's survey data into another project.
//!
//! ```text
//!   plan → apply
//! ```
//!
//! Migration is stateless: `--source-project` INTO `--project`, both named on
//! every call and neither the saved selection, so one call means the same
//! thing on every machine and in every session (owner ruling 2026-09-23). It
//! needs no window: the native client asks ds-brain's pipeline route, which
//! checks access and `pipeline.migrate` on both projects.
//!
//! The copy policy is fixed and narrower than the service: every entry, the
//! source preserved, an entry id the destination already holds skipped and
//! never overwritten. Filtering, deleting or overwriting would each need a
//! reviewed contract rather than a flag. It speaks as `ds design migrate` and
//! `ds solar migrate` speak (`docs/reference/migration.md`); survey data has
//! no kinds and no items, because it copies everything.

use ds_cli_auth::SurveyMigrationCommand;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::LANE;

const SOURCE_ARG: Arg = Arg::value(
    "source-project",
    "<project-id>",
    "Authorized SOURCE project to migrate from. You must be a member of it too.",
)
.required();

/// The destination, named like every other migration's: explicit and
/// required, never the saved selection.
const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<id>",
    "Explicit authorized DESTINATION project — the project being migrated into.",
)
.required();

const INVALID_PROJECT: Refusal = Refusal {
    code: "invalid_project",
    when: "the source or destination project id is empty, padded, too long, or not canonical",
    remedy: "pass the exact Data Solutions project id shown by project discovery",
};

const SAME_PROJECT: Refusal = Refusal {
    code: "same_project",
    when: "--source-project names the destination --project",
    remedy: "pass a source project that is not the destination",
};

const REFUSALS: &[Refusal] = &[
    INVALID_PROJECT,
    SAME_PROJECT,
    ds_cli_auth::SURVEY_MIGRATION_REFUSED_REFUSAL,
    ds_cli_auth::SURVEY_MIGRATION_UNVERIFIED_REFUSAL,
    ds_cli_auth::SIGNED_OUT_REFUSAL,
];

const ARGS: &[Arg] = &[SOURCE_ARG, PROJECT_ARG, LANE];

fn canonical_project(inputs: &Inputs, flag: &str) -> Result<String, Failure> {
    let project = inputs.require(flag)?;
    let canonical = !project.is_empty()
        && project.len() <= 160
        && project.trim() == project
        && project
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
    if !canonical {
        return Err(Failure::invalid(
            INVALID_PROJECT.code,
            format!("--{flag} is not a canonical project id"),
        )
        .remedy(INVALID_PROJECT.remedy));
    }
    Ok(project.to_owned())
}

/// Source and destination, both explicit, refused locally when they are the
/// same project so no request is sent for a migration that cannot happen.
fn projects(inputs: &Inputs) -> Result<(String, String), Failure> {
    let source = canonical_project(inputs, "source-project")?;
    let destination = canonical_project(inputs, "project")?;
    if source == destination {
        return Err(Failure::invalid(
            SAME_PROJECT.code,
            "--source-project names the destination --project",
        )
        .remedy(SAME_PROJECT.remedy));
    }
    Ok((source, destination))
}

fn run_mode(inputs: &Inputs, dry_run: bool) -> Result<Value, Failure> {
    let (source, destination) = projects(inputs)?;
    let lane = inputs.require("lane")?;
    let command = SurveyMigrationCommand {
        source_project: source.clone(),
        dry_run,
    };
    let data = ds_cli_auth::survey_migration_for_project(lane, &destination, &command)?;
    Ok(receipt(&data, lane, &source, &destination, dry_run))
}

/// The kernel shapes the receipt; the lane is the one fact only `ds` holds.
fn receipt(data: &Value, lane: &str, source: &str, destination: &str, dry_run: bool) -> Value {
    let mut receipt =
        ds_command_kernel::survey::migration_receipt(data, source, destination, dry_run);
    receipt["lane"] = json!(lane);
    receipt
}

fn render_receipt(data: &Value) -> String {
    let planned = data["mode"] == "plan";
    let (verb, count) = if planned {
        ("would copy", data["total_migrated"].as_u64().unwrap_or(0))
    } else {
        ("copied", data["total_target_written"].as_u64().unwrap_or(0))
    };
    let mut output = format!(
        "{verb} {count} survey entr{}  {} -> {}\n  matched {}  ·  skipped {}\n",
        if count == 1 { "y" } else { "ies" },
        data["source_project"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["total_matched"].as_u64().unwrap_or(0),
        data["total_skipped"].as_u64().unwrap_or(0),
    );
    for (reason, skipped) in data["skip_reasons"].as_object().into_iter().flatten() {
        output.push_str(&format!("  skipped {reason}: {skipped}\n"));
    }
    if data["source_preserved"] == true {
        output.push_str("  source preserved  ·  existing target ids not overwritten\n");
    } else {
        output.push_str(&format!(
            "  source NOT preserved: {} deleted\n",
            data["total_source_deleted"].as_u64().unwrap_or(0)
        ));
    }
    if !data["more"].is_null() {
        output.push_str("  more rows than shown; see `more` in --output json\n");
    }
    if planned {
        output.push_str(
            "  preview only; re-run as `ds survey migrate apply … --yes` once the totals are intended\n",
        );
    }
    output
}

pub mod plan {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "survey.migrate.plan",
        path: &["survey", "migrate", "plan"],
        contract: 1,
        summary: "Preview copying all survey data into a destination project.",
        purpose: "\
Asks the migration service for a dry run of copying every survey entry from \
--source-project into --project, and writes nothing. It states what would be \
copied and what would be skipped, per form and per reason. The policy is \
fixed: the source is preserved, and an entry id the destination already holds \
is skipped, never overwritten; nothing is filtered or deleted.",
        chapter: Chapter::Survey,
        effect: Effect::ReadOnly,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: ARGS,
        output: "lane, project.ds_project, source_project and mode; matched, copyable and skipped totals; bounded per_form and skip_reasons counts (overflow in more); source_preserved and overwrite_existing.",
        examples: &[Example {
            command: "ds survey migrate plan --source-project <source-id> --project <destination-id> --output json",
            note: "The service's real dry run; writes nothing.",
            runnable: false,
        }],
        refusals: REFUSALS,
        reference: Some("docs/reference/migration.md"),
        search: &[],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        super::run_mode(inputs, true)
    }

    pub fn render(data: &Value) -> String {
        super::render_receipt(data)
    }
}

pub mod apply {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "survey.migrate.apply",
        path: &["survey", "migrate", "apply"],
        contract: 1,
        summary: "Copy all survey data from a source into a destination project.",
        purpose: "\
Copies every survey entry from --source-project into --project, as the plan \
previewed. The source is preserved and an entry id the destination already \
holds is skipped, never overwritten, so running it again writes only what is \
missing. It cannot filter, delete or overwrite.",
        chapter: Chapter::Survey,
        effect: Effect::GlobalWrite,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: ARGS,
        output: "The plan's receipt with the committed totals, including total_target_written and total_source_deleted.",
        examples: &[Example {
            command: "ds survey migrate apply --source-project <source-id> --project <destination-id> --yes --output json",
            note: "Copy only after reviewing the plan receipt.",
            runnable: false,
        }],
        refusals: REFUSALS,
        reference: Some("docs/reference/migration.md"),
        search: &[],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        super::run_mode(inputs, false)
    }

    pub fn render(data: &Value) -> String {
        super::render_receipt(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(tokens: &[&str]) -> Result<Inputs, Failure> {
        let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_owned()).collect();
        ds_cli_contract::parse(&plan::COMMAND, &tokens)
    }

    /// Migration is stateless: both projects are operands, and a destination
    /// that is the source is refused under the declared code before any
    /// request is sent.
    #[test]
    fn both_projects_are_explicit_and_the_same_one_is_refused_locally() {
        const { assert!(SOURCE_ARG.required && PROJECT_ARG.required) };
        assert!(
            parse(&["--source-project", "a"]).is_err(),
            "a destination is never implied"
        );
        let same = parse(&["--source-project", "a", "--project", "a"]).expect("parses");
        let refused = projects(&same).unwrap_err();
        assert_eq!(refused.code(), SAME_PROJECT.code);
        assert_eq!(refused.remedy_text(), Some(SAME_PROJECT.remedy));
        let padded = parse(&["--source-project", "a ", "--project", "b"]).expect("parses");
        assert_eq!(projects(&padded).unwrap_err().code(), INVALID_PROJECT.code);
        let distinct = parse(&["--source-project", "a", "--project", "b"]).expect("parses");
        assert_eq!(projects(&distinct).unwrap(), ("a".into(), "b".into()));
    }

    #[test]
    fn the_plan_writes_nothing_and_neither_needs_a_window() {
        assert_eq!(plan::COMMAND.effect, Effect::ReadOnly);
        assert_eq!(apply::COMMAND.effect, Effect::GlobalWrite);
        assert!(apply::COMMAND.effect.needs_confirmation());
        assert!(!plan::COMMAND.effect.needs_confirmation());
        for command in [&plan::COMMAND, &apply::COMMAND] {
            assert_eq!(command.requires, Requires::Server);
            assert_eq!(command.authority, Authority::HeadlessProject);
        }
    }

    /// The receipt names both projects and the lane, and folds the service's
    /// answer with the kernel rather than a second count here.
    #[test]
    fn the_receipt_carries_the_shared_migration_fields() {
        let service = json!({"dry_run": true, "total_matched": 14, "total_migrated": 12,
            "total_skipped": 2, "per_form": {"customers": 12}, "skip_reasons": {"existing": 2}});
        let folded = receipt(&service, "canary", "source_one", "target_one", true);
        for field in ["lane", "project", "source_project", "mode"] {
            assert!(
                folded.get(field).is_some(),
                "missing shared field `{field}`"
            );
        }
        assert_eq!(folded["project"]["ds_project"], "target_one");
        assert_eq!(folded["mode"], "plan");
        let rendered = render_receipt(&folded);
        assert!(
            rendered.contains("would copy 12 survey entries  source_one -> target_one"),
            "{rendered}"
        );
        assert!(rendered.contains("skipped existing: 2"), "{rendered}");
        assert!(rendered.contains("preview only"), "{rendered}");
        let applied = receipt(
            &json!({"dry_run": false, "total_target_written": 1}),
            "stable",
            "source_one",
            "target_one",
            false,
        );
        let rendered = render_receipt(&applied);
        assert!(rendered.starts_with("copied 1 survey entry "), "{rendered}");
        assert!(!rendered.contains("preview only"), "{rendered}");
    }
}
