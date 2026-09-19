//! `ds design migrate` — move designs from one project into the selected one.
//!
//! ```text
//!   plan → apply
//! ```
//!
//! ONE verb, one `--kind`. A transformer and an MV DS Grid model are the same
//! domain — a design authored in a project — so migrating either is the same
//! command with a different kind, not a second family. This is deliberately
//! NOT a general migration verb: survey data has its own endpoint
//! (`ds map survey migrate`) and Solar has its own.
//!
//! Four properties a caller must not work around:
//!
//! * **Plan first.** `plan` reads exactly what `apply` reads, is gated exactly
//!   as `apply` is gated, and writes nothing. An approved plan is never a
//!   promise the apply then refuses.
//! * **Inputs and parameters travel; computed results do not.** A migrated
//!   design arrives stale and regenerates: reports, exports, cached GPKG paths
//!   and BigQuery sync stamps belong to the project that computed them.
//! * **A collision is stated, never silent.** A target that already holds the
//!   object is skipped, replaced under `--overwrite`, or — where the object
//!   carries revisions, which a DS Grid model does — given a migrated revision
//!   under its expected-head fence. The receipt names which.
//! * **Nothing moved always says why.** A run that migrates nothing carries a
//!   reason. `ds` refuses a receipt that omits one rather than printing a
//!   confident zero.
//!
//! The TARGET is the selected project. The source is the only project operand,
//! so one confirmation covers one decision.

use ds_cli_auth::DesignMigrationCommand;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::design_migration::{Kind, Mode, Outcome, collision_policy, totals};
use serde_json::{Value, json};

use super::transformer::LANE_ARG;

const SOURCE_ARG: Arg = Arg {
    name: "source-project",
    kind: ArgKind::Value,
    value: "<project-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "Migrate designs FROM this project INTO the selected project.",
};

const KIND_ARG: Arg = Arg {
    name: "kind",
    kind: ArgKind::Value,
    value: "<kind>",
    required: false,
    default: Some("transformer"),
    choices: &["transformer", "dsgrid"],
    summary: "Which design object: an LV transformer, or an MV DS Grid model.",
};

const ITEMS_ARG: Arg = Arg {
    name: "items",
    kind: ArgKind::Value,
    value: "<names>",
    required: true,
    default: None,
    choices: &[],
    summary: "Comma-separated transformer names or DS Grid model ids (1-200).",
};

const OVERWRITE_ARG: Arg = Arg::switch(
    "overwrite",
    "Allow a target that already holds the object to change: a transformer head is replaced, a DS Grid model gains a migrated revision.",
);

const INVALID_PROJECT: Refusal = Refusal {
    code: "invalid_project",
    when: "the source project id is empty, padded, too long, or not canonical",
    remedy: "pass the exact Data Solutions project id shown by project discovery",
};

const SAME_PROJECT: Refusal = Refusal {
    code: "same_project",
    when: "the source project is also the selected target project",
    remedy: "select the intended target project, then run the plan again",
};

const UNKNOWN_KIND: Refusal = Refusal {
    code: "unknown_kind",
    when: "--kind is neither transformer nor dsgrid",
    remedy: "this is the DESIGN migration; survey data uses `ds map survey migrate` and Solar has its own",
};

const INVALID_SELECTION: Refusal = Refusal {
    code: "invalid_selection",
    when: "--items is empty, over 200 names, or carries a name over 200 characters",
    remedy: "name 1 to 200 design objects; one request is one transaction's batch",
};

const MIGRATION_REFUSED: Refusal = Refusal {
    code: "migration_refused",
    when: "the service refuses it: no access on one of the projects, or an archived target",
    remedy: "read detail.detail for the service's exact refusal",
};

const UNVERIFIED_RECEIPT: Refusal = Refusal {
    code: "unverified_receipt",
    when: "the receipt is not for the migration asked for, or moved nothing without saying why",
    remedy: "re-run the plan; an unexplained zero is never reported as a success",
};

const REFUSALS: &[Refusal] = &[
    INVALID_PROJECT,
    SAME_PROJECT,
    UNKNOWN_KIND,
    INVALID_SELECTION,
    MIGRATION_REFUSED,
    UNVERIFIED_RECEIPT,
];

fn kind(inputs: &Inputs) -> Result<Kind, Failure> {
    let raw = inputs.value("kind").unwrap_or("transformer");
    Kind::parse(raw).map_err(|_| {
        Failure::invalid("unknown_kind", "--kind is neither transformer nor dsgrid")
            .remedy(UNKNOWN_KIND.remedy)
    })
}

fn source_project(inputs: &Inputs) -> Result<String, Failure> {
    let source = inputs.require("source-project")?;
    let canonical = !source.is_empty()
        && source.len() <= 160
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
    Ok(source.to_owned())
}

fn items(inputs: &Inputs) -> Result<Vec<String>, Failure> {
    let raw: Vec<String> = inputs
        .require("items")?
        .split(',')
        .map(|value| value.trim().to_owned())
        .collect();
    ds_command_kernel::design_migration::normalize_items(&raw).map_err(|refusal| {
        Failure::invalid(
            "invalid_selection",
            format!("--items is not a usable selection ({})", refusal.code()),
        )
        .remedy(INVALID_SELECTION.remedy)
    })
}

fn run_mode(inputs: &Inputs, mode: Mode) -> Result<Value, Failure> {
    let kind = kind(inputs)?;
    let source = source_project(inputs)?;
    let items = items(inputs)?;
    let overwrite_existing = inputs.switch("overwrite");
    let command = DesignMigrationCommand {
        source_project: source.clone(),
        kind,
        mode,
        items,
        overwrite_existing,
    };
    let headless = ds_cli_auth::design_migration(inputs.require("lane")?, &command)
        .map_err(classify_same_project)?;
    let project_id = headless.project_id().to_owned();
    let project_name = headless.project_name().to_owned();
    let lane = headless.lane();
    let data = headless.into_result();
    Ok(receipt(
        &data,
        lane,
        &project_id,
        &project_name,
        &source,
        kind,
        mode,
        overwrite_existing,
    ))
}

/// The service refuses a same-project migration too; this names it with the
/// CLI's own code so a caller reads one refusal rather than two spellings.
fn classify_same_project(failure: Failure) -> Failure {
    let detail = failure
        .detail_value()
        .and_then(|value| value["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !detail.contains("same project") && !detail.contains("must be different") {
        return failure;
    }
    Failure::invalid(
        "same_project",
        "the design migration source and the selected target are the same project",
    )
    .remedy(SAME_PROJECT.remedy)
}

#[allow(clippy::too_many_arguments)]
fn receipt(
    data: &Value,
    lane: &'static str,
    project_id: &str,
    project_name: &str,
    source: &str,
    kind: Kind,
    mode: Mode,
    overwrite_existing: bool,
) -> Value {
    let outcomes = ds_command_kernel::design_migration::read_items(&data["items"]);
    let folded = totals(&outcomes);
    let empty = ds_command_kernel::design_migration::empty_state(&folded, &outcomes);
    json!({
        "lane": lane,
        "project": {"ds_project": project_id, "project_name": project_name},
        "source_project": source,
        "kind": kind.wire(),
        "mode": mode.wire(),
        // The policy is the kernel's fold, so the plan and the apply describe
        // a collision the same way whichever surface asks.
        "collision_policy": collision_policy(kind, overwrite_existing).wire(),
        "requested": folded.requested,
        "moving": folded.moving,
        "identical": folded.identical,
        "blocked": folded.blocked,
        "failed": folded.failed,
        "bytes": folded.bytes,
        // The service's own sentence, kept verbatim: it names the projects and
        // objects this `ds` never read.
        "reason": data["reason"],
        "empty_state": empty.as_ref().map(|state| state.key),
        "empty_breakdown": empty.as_ref().map(|state| {
            state
                .breakdown
                .iter()
                .map(|row| json!({"outcome": row.outcome.wire(), "count": row.count}))
                .collect::<Vec<_>>()
        }),
        "items": data["items"],
    })
}

fn render_receipt(data: &Value) -> String {
    let mode = data["mode"].as_str().unwrap_or("?");
    let verb = if mode == "plan" {
        "would migrate"
    } else {
        "migrated"
    };
    let mut output = format!(
        "{verb} {} {} design object(s)  {} -> {} ({})\n  requested {}  ·  identical {}  ·  blocked {}  ·  failed {}  ·  {} bytes\n  collision policy: {}\n",
        data["moving"].as_u64().unwrap_or(0),
        data["kind"].as_str().unwrap_or("?"),
        data["source_project"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["requested"].as_u64().unwrap_or(0),
        data["identical"].as_u64().unwrap_or(0),
        data["blocked"].as_u64().unwrap_or(0),
        data["failed"].as_u64().unwrap_or(0),
        data["bytes"].as_u64().unwrap_or(0),
        data["collision_policy"].as_str().unwrap_or("?"),
    );
    // Nothing moved is never printed on its own.
    if let Some(reason) = data["reason"].as_str().filter(|value| !value.is_empty()) {
        output.push_str("  nothing moved: ");
        output.push_str(reason);
        output.push('\n');
    }
    for item in data["items"].as_array().into_iter().flatten().take(20) {
        let outcome = item["status"].as_str().unwrap_or("?");
        output.push_str(&format!(
            "  {}  {}{}\n",
            item["name"].as_str().unwrap_or("?"),
            Outcome::parse(outcome)
                .map(Outcome::state_key)
                .unwrap_or("mig_outcome_error"),
            item["reason"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(|value| format!("  ({value})"))
                .unwrap_or_default(),
        ));
    }
    if mode == "plan" {
        output.push_str(
            "  preview only; re-run as `ds design migrate apply … --yes` once the totals are intended\n",
        );
    }
    output
}

pub mod plan {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "design.migrate.plan",
        path: &["design", "migrate", "plan"],
        contract: 1,
        summary: "Preview migrating designs from another project into this one.",
        purpose: "\
States what a migration would move, by name and byte size, and what would be \
SKIPPED and why. It writes nothing and is gated exactly as the apply is, so \
an approved plan is never a promise the apply refuses. Inputs and parameters \
travel; computed results never do, so a migrated design arrives stale and \
regenerates. When nothing would move the receipt says why; a zero without a \
reason is not reported as a success. The reference explains each kind.",
        chapter: Chapter::Design,
        effect: Effect::ReadOnly,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[SOURCE_ARG, KIND_ARG, ITEMS_ARG, OVERWRITE_ARG, LANE_ARG],
        output: "Both projects, the kind, the collision policy, per-object outcomes with byte sizes, and the reason when nothing would move.",
        examples: &[
            Example {
                command: "ds design migrate plan --source-project arjgpydw_aderm --items TX-1,TX-2 --output json",
                note: "Transformers are the default kind; nothing is written.",
                runnable: false,
            },
            Example {
                command: "ds design migrate plan --source-project arjgpydw_aderm --kind dsgrid --items huye_mv",
                note: "An MV model is the same domain under another kind.",
                runnable: false,
            },
        ],
        refusals: REFUSALS,
        reference: Some("docs/reference/design.md"),
        search: &[],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        super::run_mode(inputs, Mode::Plan)
    }

    pub fn render(data: &Value) -> String {
        super::render_receipt(data)
    }
}

pub mod apply {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "design.migrate.apply",
        path: &["design", "migrate", "apply"],
        contract: 1,
        summary: "Migrate designs from another project into this one.",
        purpose: "\
Executes the migration `ds design migrate plan` previewed. It is idempotent: \
an object whose content the target already holds is reported as identical and \
is not written again, so running twice never duplicates and never advances a \
version for no change. A target that already holds the object is skipped \
unless --overwrite is given; with it, a transformer head is replaced while a \
DS Grid model instead gains a migrated revision under its expected-head fence \
— an object that carries revisions is never overwritten. Source project ids, \
storage prefixes and derived fields are rewritten for the destination, and \
what was rewritten or dropped is stated per object.",
        chapter: Chapter::Design,
        effect: Effect::GlobalWrite,
        // The TARGET is the session's own selected project and no window is
        // involved, so the authority is the headless one the rest of the
        // native design spine uses.
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[SOURCE_ARG, KIND_ARG, ITEMS_ARG, OVERWRITE_ARG, LANE_ARG],
        output: "The same receipt shape the plan returns, with the committed per-object outcomes.",
        examples: &[Example {
            command: "ds design migrate apply --source-project arjgpydw_aderm --items TX-1,TX-2 --yes --output json",
            note: "Migrate only after reviewing the plan receipt.",
            runnable: false,
        }],
        refusals: REFUSALS,
        reference: Some("docs/reference/design.md"),
        search: &[],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        super::run_mode(inputs, Mode::Apply)
    }

    pub fn render(data: &Value) -> String {
        super::render_receipt(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_verb_carries_both_design_kinds() {
        assert_eq!(KIND_ARG.choices, &["transformer", "dsgrid"]);
        assert_eq!(KIND_ARG.default, Some("transformer"));
        let plan_args: Vec<&str> = plan::COMMAND.args.iter().map(|arg| arg.name).collect();
        let apply_args: Vec<&str> = apply::COMMAND.args.iter().map(|arg| arg.name).collect();
        assert_eq!(plan_args, apply_args);
    }

    #[test]
    fn the_plan_writes_nothing_and_the_apply_needs_a_confirmation() {
        assert_eq!(plan::COMMAND.effect, Effect::ReadOnly);
        assert_eq!(apply::COMMAND.effect, Effect::GlobalWrite);
        assert!(apply::COMMAND.effect.needs_confirmation());
        assert!(!plan::COMMAND.effect.needs_confirmation());
    }

    #[test]
    fn migrating_between_projects_needs_no_window() {
        assert_eq!(plan::COMMAND.requires, Requires::Server);
        assert_eq!(apply::COMMAND.requires, Requires::Server);
    }

    #[test]
    fn a_receipt_that_moved_nothing_renders_the_reason() {
        let data = json!({
            "mode": "plan",
            "kind": "transformer",
            "source_project": "source_one",
            "project": {"ds_project": "target_one", "project_name": "Target"},
            "collision_policy": "skip_existing",
            "requested": 2, "moving": 0, "identical": 2, "blocked": 0, "failed": 0, "bytes": 0,
            "reason": "nothing moved: 2 requested, 2 identical",
            "items": [
                {"name": "TX-1", "status": "identical", "reason": "target already holds this exact content"},
                {"name": "TX-2", "status": "identical", "reason": "target already holds this exact content"}
            ]
        });
        let rendered = render_receipt(&data);
        assert!(rendered.contains("nothing moved"), "{rendered}");
        assert!(rendered.contains("2 identical"), "{rendered}");
        assert!(rendered.contains("mig_outcome_identical"), "{rendered}");
    }

    #[test]
    fn a_receipt_that_moved_something_states_no_empty_reason() {
        let data = json!({
            "mode": "apply",
            "kind": "dsgrid",
            "source_project": "source_one",
            "project": {"ds_project": "target_one", "project_name": "Target"},
            "collision_policy": "revise_existing",
            "requested": 1, "moving": 1, "identical": 0, "blocked": 0, "failed": 0, "bytes": 4096,
            "reason": Value::Null,
            "items": [{"name": "huye_mv", "status": "revised"}]
        });
        let rendered = render_receipt(&data);
        assert!(!rendered.contains("nothing moved"), "{rendered}");
        assert!(rendered.contains("migrated 1 dsgrid"), "{rendered}");
        assert!(rendered.contains("revise_existing"), "{rendered}");
    }

    #[test]
    fn the_receipt_folds_item_outcomes_with_the_kernel_not_a_second_count() {
        let data = json!({"items": [
            {"status": "would_copy", "bytes": 10},
            {"status": "target_exists"},
            {"status": "identical"},
            {"status": "error"}
        ], "reason": "x"});
        let folded = receipt(
            &data,
            "canary",
            "target_one",
            "Target",
            "source_one",
            Kind::Transformer,
            Mode::Plan,
            false,
        );
        assert_eq!(folded["requested"], 4);
        assert_eq!(folded["moving"], 1);
        assert_eq!(folded["identical"], 1);
        assert_eq!(folded["blocked"], 1);
        assert_eq!(folded["failed"], 1);
        assert_eq!(folded["bytes"], 10);
        assert_eq!(folded["collision_policy"], "skip_existing");
        assert!(folded["empty_state"].is_null());
    }
}
