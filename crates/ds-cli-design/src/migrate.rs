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
//! It SPEAKS as `ds solar migrate` speaks without being merged with it:
//! `--source-project`, a required `--kind`, repeated `--item`, and one receipt
//! vocabulary and refusal shape (`docs/reference/migration.md`). An operator
//! or agent who learned one drives the other.
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
//! * **Nothing moved always says why.** A run that migrates nothing carries an
//!   `empty_state` key and the service's own statement in `warnings`. `ds`
//!   refuses a receipt that omits one rather than printing a confident zero.
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

/// Required, as Solar's is: a transformer and a DS Grid model are different
/// objects, so a caller who did not say which is refused, never guessed at.
/// (The SERVICE still reads an absent wire `kind` as transformer, for clients
/// that predate `kind`; that is wire compatibility, not a CLI default.)
const KIND_ARG: Arg = Arg {
    name: "kind",
    kind: ArgKind::Value,
    value: "<transformer|dsgrid>",
    required: true,
    default: None,
    choices: &["transformer", "dsgrid"],
    summary: "An LV transformer or an MV DS Grid model. No default.",
};

/// One object per `--item`, as `ds solar migrate` takes it: a name is never
/// split, so no name can be unaddressable because it holds a comma.
const ITEM_ARG: Arg = Arg::repeated(
    "item",
    "<name>",
    "A transformer name or DS Grid model id, per --kind. Repeat, 1-200.",
)
.required();

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

/// The bound's basis is the owner's write path, stated once for both domains
/// in docs/reference/migration.md: 200 is ds-brain's transformer-copy
/// transaction batch (`transformerCopyReadBatchSize`), which the DS Grid kind
/// shares so one verb has one bound.
const INVALID_SELECTION: Refusal = Refusal {
    code: "invalid_selection",
    when: "no --item names an object, over 200 are named, or one is over 200 characters",
    remedy: "name 1 to 200 objects with repeated --item; 200 is one transformer-copy transaction's batch",
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
    let raw = inputs.require("kind")?;
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
    let raw = inputs.repeated("item");
    ds_command_kernel::design_migration::normalize_items(raw).map_err(|refusal| {
        Failure::invalid(
            "invalid_selection",
            format!("--item is not a usable selection ({})", refusal.code()),
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
///
/// Two refusals arrive here, and both must land on the declared code. The
/// service's travels in `detail.detail`; the client's own pre-check never
/// reaches the service at all and arrives as the message of a generic
/// `auth_input_invalid`, which carries no remedy. Reading only the detail let
/// the local path — the one a caller hits first — lose both.
fn classify_same_project(failure: Failure) -> Failure {
    let detail = failure
        .detail_value()
        .and_then(|value| value["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let message = failure.message().to_ascii_lowercase();
    let says_same_project =
        |text: &str| text.contains("same project") || text.contains("must be different");
    if !says_same_project(&detail) && !says_same_project(&message) {
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
    // The service's own run-level statement travels in `warnings`, the one
    // shape both migration domains use for it (Solar's plan warnings land in
    // the same field). There is no second, single-string `reason` shape.
    let warnings: Vec<&str> = data["reason"]
        .as_str()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .into_iter()
        .collect();
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
        "empty_state": empty.as_ref().map(|state| state.key),
        "empty_breakdown": empty.as_ref().map(|state| {
            state
                .breakdown
                .iter()
                .map(|row| json!({"outcome": row.outcome.wire(), "count": row.count}))
                .collect::<Vec<_>>()
        }),
        "warnings": warnings,
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
    // Nothing moved is never printed on its own: the kernel's key, then the
    // service's own statement from `warnings`.
    if let Some(state) = data["empty_state"].as_str() {
        output.push_str(&format!("  nothing moved: {state}\n"));
    }
    for warning in data["warnings"].as_array().into_iter().flatten() {
        output.push_str(&format!("  warning: {}\n", warning.as_str().unwrap_or("?")));
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
        args: &[SOURCE_ARG, KIND_ARG, ITEM_ARG, OVERWRITE_ARG, LANE_ARG],
        output: "The shared migration receipt (see reference): per-object outcomes, totals, empty_state and warnings; plus collision policy and bytes.",
        examples: &[
            Example {
                command: "ds design migrate plan --source-project arjgpydw_aderm --kind transformer --item TX-1 --item TX-2 --output json",
                note: "Writes nothing.",
                runnable: false,
            },
            Example {
                command: "ds design migrate plan --source-project arjgpydw_aderm --kind dsgrid --item huye_mv",
                note: "An MV model is the same domain under another kind.",
                runnable: false,
            },
        ],
        refusals: REFUSALS,
        reference: Some("docs/reference/migration.md"),
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
        args: &[SOURCE_ARG, KIND_ARG, ITEM_ARG, OVERWRITE_ARG, LANE_ARG],
        output: "The same receipt the plan returns, with the committed per-object outcomes.",
        examples: &[Example {
            command: "ds design migrate apply --source-project arjgpydw_aderm --kind transformer --item TX-1 --item TX-2 --yes --output json",
            note: "Migrate only after reviewing the plan receipt.",
            runnable: false,
        }],
        refusals: REFUSALS,
        reference: Some("docs/reference/migration.md"),
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

    /// The local pre-check refuses before a request is ever sent, and it must
    /// reach the caller under the code this command declares — not as the
    /// generic `auth_input_invalid`, which carries no remedy at all.
    #[test]
    fn the_local_same_project_check_surfaces_under_the_declared_code() {
        let local = Failure::invalid(
            "auth_input_invalid",
            "design migration source and target are the same project",
        );
        let named = classify_same_project(local);
        assert_eq!(named.code(), SAME_PROJECT.code);
        assert_eq!(named.remedy_text(), Some(SAME_PROJECT.remedy));
    }

    /// The service's own refusal still names the same code, from its detail.
    #[test]
    fn the_service_same_project_refusal_keeps_the_same_code() {
        let remote = Failure::invalid("migration_refused", "the service refused the migration")
            .detail(json!({ "detail": "source and target must be different" }));
        assert_eq!(classify_same_project(remote).code(), SAME_PROJECT.code);
    }

    /// Nothing else is rewritten: an unrelated refusal passes through whole.
    #[test]
    fn an_unrelated_refusal_is_left_exactly_as_it_arrived() {
        let other = Failure::invalid("auth_input_invalid", "--item is empty");
        assert_eq!(classify_same_project(other).code(), "auth_input_invalid");
    }

    #[test]
    fn one_verb_carries_both_design_kinds() {
        assert_eq!(KIND_ARG.choices, &["transformer", "dsgrid"]);
        // Required with no default, exactly as Solar's kind is.
        const { assert!(KIND_ARG.required) };
        assert_eq!(KIND_ARG.default, None);
        let plan_args: Vec<&str> = plan::COMMAND.args.iter().map(|arg| arg.name).collect();
        let apply_args: Vec<&str> = apply::COMMAND.args.iter().map(|arg| arg.name).collect();
        assert_eq!(plan_args, apply_args);
    }

    /// One object per `--item`: a name holding a comma is one name, never two.
    #[test]
    fn an_item_is_never_split_and_repeats_collapse() {
        let tokens: Vec<String> = [
            "--source-project",
            "source_one",
            "--kind",
            "transformer",
            "--item",
            "TX-1,A",
            "--item",
            "TX-2",
            "--item",
            "TX-2",
        ]
        .iter()
        .map(|token| (*token).to_owned())
        .collect();
        let parsed =
            ds_cli_contract::parse(&plan::COMMAND, &tokens).expect("declared tokens parse");
        assert_eq!(items(&parsed).unwrap(), vec!["TX-1,A", "TX-2"]);
        assert_eq!(kind(&parsed).unwrap(), Kind::Transformer);
    }

    /// A kind that was not stated is refused at the door, never defaulted.
    #[test]
    fn an_unstated_kind_is_refused_before_anything_runs() {
        let tokens: Vec<String> = ["--source-project", "source_one", "--item", "TX-1"]
            .iter()
            .map(|token| (*token).to_owned())
            .collect();
        assert!(ds_cli_contract::parse(&plan::COMMAND, &tokens).is_err());
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
    fn a_receipt_that_moved_nothing_renders_why() {
        let data = json!({
            "mode": "plan",
            "kind": "transformer",
            "source_project": "source_one",
            "project": {"ds_project": "target_one", "project_name": "Target"},
            "collision_policy": "skip_existing",
            "requested": 2, "moving": 0, "identical": 2, "blocked": 0, "failed": 0, "bytes": 0,
            "empty_state": "mig_empty_all_identical",
            "warnings": ["nothing moved: 2 requested, 2 identical"],
            "items": [
                {"name": "TX-1", "status": "identical", "reason": "target already holds this exact content"},
                {"name": "TX-2", "status": "identical", "reason": "target already holds this exact content"}
            ]
        });
        let rendered = render_receipt(&data);
        assert!(
            rendered.contains("nothing moved: mig_empty_all_identical"),
            "{rendered}"
        );
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
            "empty_state": Value::Null,
            "warnings": [],
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
        // The service's statement is a warning; there is no single `reason`.
        assert_eq!(folded["warnings"], json!(["x"]));
        assert!(folded.get("reason").is_none());
    }

    /// The fields both migration domains' receipts carry, under one name each
    /// (docs/reference/migration.md). `ds-cli-solar` pins the same list.
    #[test]
    fn the_receipt_carries_the_shared_migration_fields() {
        let folded = receipt(
            &json!({"items": []}),
            "stable",
            "target_one",
            "Target",
            "source_one",
            Kind::Dsgrid,
            Mode::Apply,
            true,
        );
        for field in [
            "lane",
            "project",
            "source_project",
            "kind",
            "mode",
            "requested",
            "moving",
            "identical",
            "blocked",
            "failed",
            "empty_state",
            "empty_breakdown",
            "warnings",
            "items",
        ] {
            assert!(
                folded.get(field).is_some(),
                "missing shared field `{field}`"
            );
        }
        assert_eq!(folded["project"]["ds_project"], "target_one");
        assert!(folded["warnings"].is_array());
    }
}
