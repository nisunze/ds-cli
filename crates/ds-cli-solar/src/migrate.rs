//! `ds solar migrate` — project-to-project Solar migration, planned then
//! confirmed.
//!
//! ONE verb in the Solar domain with a `--kind` INSIDE it: `city` or
//! `portfolio`. It moves authored INPUTS and PARAMETERS and never a computation
//! result, so a migrated city arrives able to be computed and holding nothing
//! that was computed. The destination recomputes.
//!
//! This is not a general migration command. `ds map survey migrate` moves
//! survey data and the design domain owns its own; an abstraction over all
//! three would describe none of them.
//!
//! It SPEAKS as `ds design migrate` speaks without being merged with it:
//! `--source-project`, a required `--kind`, repeated `--item`, and one receipt
//! vocabulary and refusal shape (`docs/reference/migration.md`). The kinds stay
//! Solar's; only the words a receipt uses for an outcome are shared, and they
//! are the kernel's `design_migration::Outcome` words, projected here from the
//! Solar service's own row actions. The service's plan is kept verbatim under
//! `plan`, because that is the document `migrate_digest` binds.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::design_migration::{Mode, Outcome, empty_state, read_items, totals};
use serde_json::{Value, json};

pub use ds_command_kernel::solar_migration::{
    KIND_CITY, KIND_PORTFOLIO, KINDS, MAX_ID_CHARS, MAX_PROJECT_CHARS, MAX_SELECTION,
};

const PLAN_OPERATION: &str = "solar.migrate.plan";
const APPLY_OPERATION: &str = "solar.migrate.apply";

/// The two actions on ds-brain's unified Solar door, in the order this domain
/// exposes them. `ds` composes no third action.
pub const SERVER_ACTIONS: &[&str] = &["migrate_plan", "migrate_apply"];

/// Every wire key a migration request can carry. The destination `root` is
/// composed by the native client from the explicit authorized project.
pub const SERVER_REQUEST_KEYS: &[&str] = &[
    "root",
    "migrate_kind",
    "migrate_source_project",
    "cities",
    "portfolios",
    "migrate_overwrite",
    "migrate_digest",
];

/// The refusal-detail key carrying ds-brain's own spelling of a refusal code.
const SERVER_CODE_DETAIL: &str = "server_code";

/// ds-brain's own refusal codes, verbatim. Each maps to the CLI refusal of the
/// same name in snake_case, which is the casing `contract.rs` requires.
pub const SERVER_CODES: &[(&str, &str)] = &[
    (
        "SOLAR_MIGRATE_PROJECT_ROOT_REQUIRED",
        "solar_migrate_project_root_required",
    ),
    (
        "SOLAR_MIGRATE_SOURCE_INVALID",
        "solar_migrate_source_invalid",
    ),
    ("SOLAR_MIGRATE_KIND_INVALID", "solar_migrate_kind_invalid"),
    (
        "SOLAR_MIGRATE_COMPONENT_DISABLED",
        "solar_migrate_component_disabled",
    ),
    (
        "SOLAR_MIGRATE_SELECTION_INVALID",
        "solar_migrate_selection_invalid",
    ),
    (
        "SOLAR_MIGRATE_DIGEST_REQUIRED",
        "solar_migrate_digest_required",
    ),
    (
        "SOLAR_MIGRATE_DIGEST_MISMATCH",
        "solar_migrate_digest_mismatch",
    ),
    ("SOLAR_MIGRATE_BOUNDED", "solar_migrate_bounded"),
];

fn native_available() -> Availability {
    Availability::Available
}

const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<id>",
    "Explicit authorized DESTINATION project — the project being migrated into.",
)
.required();

/// `--source-project`, as `ds design migrate` and `ds map survey migrate` name
/// it, and as both services' receipts spell it (`source_project`).
const SOURCE_ARG: Arg = Arg::value(
    "source-project",
    "<project-id>",
    "Authorized SOURCE project to migrate from. You must be a member of it too.",
)
.required();

const KIND_ARG: Arg = Arg::value(
    "kind",
    "<city|portfolio>",
    "Which Solar object migrates. There is no default: cities and portfolios are different objects.",
)
.required();

/// ONE selection flag whose meaning the kind decides, as design's `--item`
/// is. A flag per kind could only ever be refused as "the other kind's
/// selection"; one flag cannot be sent to the wrong kind at all.
const ITEM_ARG: Arg = Arg::repeated(
    "item",
    "<name>",
    "One source city id (kind city) or portfolio NAME (kind portfolio). Repeat, up to 64. Omit for every source object of the kind.",
);

const OVERWRITE_ARG: Arg = Arg::switch(
    "overwrite",
    "Replace an object that already exists in the destination with a different definition; bind this choice in both plan and apply.",
);

const LANE_ARG: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Native authentication lane; defaults to stable.",
);

/// Refusals raised at the native migration boundary.
static MIGRATE_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "solar_migrate_kind_invalid",
        when: "--kind is not exactly `city` or `portfolio`",
        remedy: "pass --kind city or --kind portfolio; design objects migrate through the design domain",
    },
    Refusal {
        code: "solar_migrate_selection_invalid",
        when: "an --item value is blank, padded, over 128 characters, or named twice",
        remedy: "name each object once, unpadded, with one --item per object",
    },
    Refusal {
        code: "solar_migrate_bounded",
        when: "more than 64 --item values were named in one migration",
        remedy: "migrate at most 64 objects per request: 64 is the Solar seed writer's per-request bound, which a migration apply writes through",
    },
    Refusal {
        code: "solar_migrate_source_invalid",
        when: "--source-project is blank, names the destination project, or names the governed catalog",
        remedy: "pass one exact source project id that is not the destination",
    },
    Refusal {
        code: "solar_migrate_project_root_required",
        when: "the explicit destination project does not resolve to a project Solar root",
        remedy: "pass the exact authorized Solar project, then retry",
    },
    Refusal {
        code: "solar_migrate_component_disabled",
        when: "the destination project does not declare the `solar` component",
        remedy: "enable the Solar component on the destination project, then retry",
    },
    Refusal {
        code: "solar_migrate_contract_mismatch",
        when: "the reply is not a Solar migration plan, describes another call, or a plan reports that it mutated",
        remedy: "update ds and its native client core to matching releases",
    },
];

/// `apply` adds the two refusals that only exist because it is a confirmation.
static APPLY_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "solar_migrate_digest_required",
        when: "--migrate-digest is not the exact 64-character lowercase digest a plan returned",
        remedy: "run `ds solar migrate plan --project <exact-id> --source-project <exact-id> --kind <kind>` and pass that plan's exact migrate_digest",
    },
    Refusal {
        code: "solar_migrate_digest_mismatch",
        when: "the source or destination moved since the migration was planned",
        remedy: "plan again, review the new plan, and confirm that digest",
    },
    Refusal {
        code: "solar_migrate_kind_invalid",
        when: "--kind is not exactly `city` or `portfolio`",
        remedy: "pass --kind city or --kind portfolio; design objects migrate through the design domain",
    },
    Refusal {
        code: "solar_migrate_selection_invalid",
        when: "an --item value is blank, padded, over 128 characters, or named twice",
        remedy: "name each object once, unpadded, with one --item per object",
    },
    Refusal {
        code: "solar_migrate_bounded",
        when: "more than 64 --item values were named in one migration",
        remedy: "migrate at most 64 objects per request: 64 is the Solar seed writer's per-request bound, which a migration apply writes through",
    },
    Refusal {
        code: "solar_migrate_source_invalid",
        when: "--source-project is blank, names the destination project, or names the governed catalog",
        remedy: "pass one exact source project id that is not the destination",
    },
    Refusal {
        code: "solar_migrate_project_root_required",
        when: "the explicit destination project does not resolve to a project Solar root",
        remedy: "pass the exact authorized Solar project, then retry",
    },
    Refusal {
        code: "solar_migrate_component_disabled",
        when: "the destination project does not declare the `solar` component",
        remedy: "enable the Solar component on the destination project, then retry",
    },
    Refusal {
        code: "solar_migrate_contract_mismatch",
        when: "the reply is not a migration receipt, does not echo the confirmed digest, or claims computation results migrated",
        remedy: "update ds and its native client core to matching releases",
    },
];

const NATIVE_REFUSALS: &[Refusal] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals;
const fn with_native<const N: usize>(local: &[Refusal]) -> [Refusal; N] {
    let mut result = [local[0]; N];
    let mut i = 0;
    while i < local.len() {
        result[i] = local[i];
        i += 1;
    }
    let mut j = 0;
    while j < NATIVE_REFUSALS.len() {
        result[i + j] = NATIVE_REFUSALS[j];
        j += 1;
    }
    result
}
const PLAN_ALL_REFUSALS: [Refusal; MIGRATE_REFUSALS.len() + NATIVE_REFUSALS.len()] =
    with_native(MIGRATE_REFUSALS);
const APPLY_ALL_REFUSALS: [Refusal; APPLY_REFUSALS.len() + NATIVE_REFUSALS.len()] =
    with_native(APPLY_REFUSALS);

pub static PLAN_COMMAND: Command = Command {
    id: "solar.migrate.plan",
    path: &["solar", "migrate", "plan"],
    contract: 1,
    summary: "Plan which Solar objects would migrate in from another project.",
    purpose: "\
Asks ds-brain headlessly what WOULD move from one project's Solar root into \
another's: with --kind city each city's authored input documents, with --kind \
portfolio each portfolio definition and its ordered member cities. It writes \
nothing — the plan carries the server's own `mutated: false`. COMPUTATION \
RESULTS NEVER MIGRATE: the plan names the result collections it leaves behind \
and the destination recomputes. A portfolio whose member cities are absent \
there is reported `refused`, naming them, never written as a definition that \
would read back empty. Confirm the returned `migrate_digest` with `ds solar \
migrate apply` to write it.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT_ARG,
        SOURCE_ARG,
        KIND_ARG,
        ITEM_ARG,
        OVERWRITE_ARG,
        LANE_ARG,
    ],
    output: "\
The shared migration receipt (see reference): per-object outcomes, totals, \
empty_state and warnings; plus the `migrate_digest` binding this plan to its \
apply and ds-brain's plan verbatim under `plan` (travel policy, excluded paths).",
    examples: &[
        Example {
            command: "ds solar migrate plan --project chad-test --source-project aderm --kind city --output json",
            note: "Plans every live source city into the destination. Writes nothing; no computed value moves.",
            runnable: false,
        },
        Example {
            command: "ds solar migrate plan --project chad-test --source-project aderm --kind portfolio --item North --output json",
            note: "Read the `refused` items first: a portfolio whose member cities are not in the destination is refused, not written empty.",
            runnable: false,
        },
    ],
    refusals: &PLAN_ALL_REFUSALS,
    reference: Some("docs/reference/migration.md"),
    search: &[],
    requires: Requires::Server,
    availability: native_available,
};

pub static APPLY_COMMAND: Command = Command {
    id: "solar.migrate.apply",
    path: &["solar", "migrate", "apply"],
    contract: 1,
    summary: "Migrate exactly the planned objects, bound to the plan's digest.",
    purpose: "\
Confirms one plan `ds solar migrate plan` returned. --migrate-digest is echoed \
from that plan and is never derived here: it is what proves the set being \
written is the set someone read. ds-brain re-plans server-side and refuses with \
`solar_migrate_digest_mismatch` if either end moved. A migrated city arrives \
marked `computation_state: never_computed_in_this_project` — so \"not computed \
here yet\" can never be mistaken for \"computed to nothing\" — and one city \
commits atomically. A migrated portfolio writes its definition only; its result \
shell is created by the first calculation in the destination.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "migrate-digest",
            "<sha256>",
            "The exact 64-character migrate_digest of the plan being confirmed.",
        )
        .required(),
        PROJECT_ARG,
        SOURCE_ARG,
        KIND_ARG,
        ITEM_ARG,
        OVERWRITE_ARG,
        LANE_ARG,
    ],
    output: "\
The same receipt the plan returns, with committed outcomes, the confirmed \
`migrate_digest`, `documents_written`, `idempotent`, and \
`computation_results_migrated` — always false, on the wire so a receipt states \
the contract — plus the re-planned ds-brain plan verbatim under `plan`.",
    examples: &[Example {
        command: "ds solar migrate apply --project chad-test --source-project aderm --kind city --migrate-digest <64-hex from plan> --yes --output json",
        note: "Confirms exactly the planned migration; a moved source or destination is refused, not re-planned.",
        runnable: false,
    }],
    refusals: &APPLY_ALL_REFUSALS,
    reference: Some("docs/reference/migration.md"),
    search: &[],
    requires: Requires::Server,
    availability: native_available,
};

/// The kind and selection a migration call sends, validated locally by the
/// SAME kernel rule ds-brain applies.
///
/// Pure, and separate from the handlers, so the properties that matter are
/// testable without authentication or transport: that `--kind` is one of two
/// exact values and that a duplicate is refused rather than deduplicated. The
/// kind decides what an `--item` names; the native client sends the selection
/// under that kind's own wire key, so no selection can reach the other kind.
fn envelope(inputs: &Inputs) -> Result<(String, Vec<String>, String), Failure> {
    let kind = inputs.require("kind")?.to_string();
    if !KINDS.contains(&kind.as_str()) {
        return Err(Failure::invalid(
            "solar_migrate_kind_invalid",
            format!("--kind must be one of {}", KINDS.join(" or ")),
        )
        .remedy("pass --kind city or --kind portfolio; design objects migrate through the design domain"));
    }

    let selection = inputs.repeated("item");
    let from = inputs.require("source-project")?.to_string();
    let context = ds_command_kernel::solar_migration::Context {
        kind: kind.clone(),
        root: String::new(),
        ds_project: String::new(),
        source_project: from.clone(),
        selection: selection.to_vec(),
    };
    ds_command_kernel::solar_migration::validate_context(&context, false).map_err(|refusal| {
        match refusal.code {
            "SOLAR_MIGRATE_BOUNDED" => Failure::invalid(
                "solar_migrate_bounded",
                format!(
                    "{} objects were named; one migration carries at most {MAX_SELECTION}",
                    selection.len()
                ),
            )
            .remedy(format!(
                "migrate at most {MAX_SELECTION} objects per request: the Solar seed writer's per-request bound, which a migration apply writes through"
            )),
            "SOLAR_MIGRATE_SOURCE_INVALID" => Failure::invalid(
                "solar_migrate_source_invalid",
                "--source-project must be one exact unpadded source project id",
            )
            .remedy("pass one exact source project id that is not the destination"),
            _ => Failure::invalid(
                "solar_migrate_selection_invalid",
                "each named object must appear once, unpadded",
            )
            .remedy("name each object once, unpadded"),
        }
    })?;

    if from == inputs.require("project")? {
        return Err(Failure::invalid(
            "solar_migrate_source_invalid",
            "--source-project names the destination project",
        )
        .remedy("pass a source project that is not the destination"));
    }

    Ok((kind, selection.to_vec(), from))
}

/// `migrate_digest` is only ever echoed. This checks the caller echoed
/// something that could have been a digest at all — a truncated copy/paste is
/// refused here rather than becoming a round trip that reports drift.
fn validate_digest(raw: &str) -> Result<&str, Failure> {
    if ds_command_kernel::solar_migration::confirm_digest(raw).is_err() {
        return Err(Failure::invalid(
            "solar_migrate_digest_required",
            "--migrate-digest must be the exact 64-character lowercase migrate_digest of a plan",
        )
        .remedy("run `ds solar migrate plan` and pass that plan's exact migrate_digest")
        .next("ds solar migrate plan --project <exact-id> --source-project <exact-id> --kind <kind> --output json"));
    }
    Ok(raw)
}

pub fn plan(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let (kind, selection, from) = envelope(inputs)?;
    let result = invoke(inputs, kind, selection, from, None)?;
    let plan = migration_plan(&result, PLAN_OPERATION)?;
    // A plan that reports it mutated is a contract break, not a state this
    // command can render. `mutated` is on the wire precisely so no client
    // infers "this was safe" from which action it called.
    if plan["mutated"] != Value::Bool(false) {
        return Err(mismatch(
            PLAN_OPERATION,
            "a migration plan reported that it mutated",
        ));
    }
    Ok(receipt(&result, Mode::Plan, lane(inputs)))
}

pub fn apply(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let confirmed = validate_digest(inputs.require("migrate-digest")?)?.to_string();
    let (kind, selection, from) = envelope(inputs)?;
    let result = invoke(inputs, kind, selection, from, Some(confirmed.clone()))?;
    migration_plan(&result, APPLY_OPERATION)?;
    if result["migrate_digest"].as_str() != Some(confirmed.as_str()) {
        return Err(mismatch(
            APPLY_OPERATION,
            "the migration receipt does not echo the confirmed migrate_digest",
        ));
    }
    // The endpoint's defining constraint, checked at the last boundary that can
    // still refuse to render it.
    if !ds_command_kernel::solar_migration::results_are_honest(
        result["computation_results_migrated"]
            .as_bool()
            .unwrap_or(true),
    ) {
        return Err(mismatch(
            APPLY_OPERATION,
            "the receipt claims computation results migrated",
        ));
    }
    Ok(receipt(&result, Mode::Apply, lane(inputs)))
}

fn lane(inputs: &Inputs) -> &str {
    inputs.value("lane").unwrap_or("stable")
}

/// What one planned row amounts to, in the ONE receipt vocabulary.
///
/// The Solar service plans with the seeding words (create / replace / skip /
/// changed / missing, plus blocked for a portfolio); the receipt speaks the
/// words `ds design migrate` speaks, so a reader learns one set. A word this
/// table does not know is an error row, never a dropped one.
fn planned(action: &str) -> Outcome {
    match action {
        "create" => Outcome::WouldCopy,
        "replace" => Outcome::WouldReplace,
        "skip" => Outcome::Identical,
        "changed" => Outcome::TargetExists,
        "missing" => Outcome::MissingSource,
        "blocked" => Outcome::Refused,
        _ => Outcome::Error,
    }
}

/// What one row became once the apply ran, read from the service's own
/// applied / skipped lists — never inferred from the plan alone.
fn committed(action: &str, name: &str, result: &Value) -> Outcome {
    let listed = |field: &str| {
        result[field]
            .as_array()
            .is_some_and(|names| names.iter().any(|held| held.as_str() == Some(name)))
    };
    if listed("applied") {
        return if action == "replace" {
            Outcome::Replaced
        } else {
            Outcome::Copied
        };
    }
    match planned(action) {
        // Planned to move and skipped: the destination gained it between the
        // plan and the write.
        Outcome::WouldCopy | Outcome::WouldReplace if listed("skipped") => Outcome::TargetExists,
        // Planned to move, neither written nor skipped: never reported moved.
        Outcome::WouldCopy | Outcome::WouldReplace => Outcome::Error,
        settled => settled,
    }
}

/// The shared receipt, from a reply `migration_plan` already accepted.
///
/// The fields both migration domains carry come first, under one name each
/// (docs/reference/migration.md); the Solar-only evidence follows, and the
/// service's plan travels verbatim under `plan` because `migrate_digest`
/// binds that document, not this projection.
fn receipt(result: &Value, mode: Mode, lane: &str) -> Value {
    let plan = if result["plan"].is_object() {
        &result["plan"]
    } else {
        result
    };
    let rows = plan["cities"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| (row, "city_id", KIND_CITY))
        .chain(
            plan["portfolios"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|row| (row, "name", KIND_PORTFOLIO)),
        );
    let items: Vec<Value> = rows
        .map(|(row, id_field, kind)| {
            let name = row[id_field].as_str().unwrap_or_default();
            let action = row["action"].as_str().unwrap_or_default();
            let outcome = match mode {
                Mode::Plan => planned(action),
                Mode::Apply => committed(action, name, result),
            };
            json!({"name": name, "kind": kind, "status": outcome.wire(), "reason": row["reason"]})
        })
        .collect();
    let items = Value::Array(items);
    let outcomes = read_items(&items);
    let folded = totals(&outcomes);
    let empty = empty_state(&folded, &outcomes);
    // Plan warnings first, then each row's, de-duplicated — the kernel's fold.
    let parsed: ds_command_kernel::solar_migration::Plan =
        serde_json::from_value(plan.clone()).unwrap_or_default();
    let warnings = ds_command_kernel::solar_migration::summarize(Some(&parsed)).warnings;
    let mut receipt = json!({
        "lane": lane,
        "project": {"ds_project": plan["ds_project"]},
        "source_project": plan["source_project"],
        "kind": plan["kind"],
        "mode": mode.wire(),
        "requested": folded.requested,
        "moving": folded.moving,
        "identical": folded.identical,
        "blocked": folded.blocked,
        "failed": folded.failed,
        "empty_state": empty.as_ref().map(|state| state.key),
        "empty_breakdown": empty.as_ref().map(|state| {
            state
                .breakdown
                .iter()
                .map(|row| json!({"outcome": row.outcome.wire(), "count": row.count}))
                .collect::<Vec<_>>()
        }),
        "warnings": warnings,
        "items": items,
        "migrate_digest": plan["migrate_digest"],
    });
    if mode == Mode::Apply {
        for field in [
            "documents_written",
            "idempotent",
            "computation_results_migrated",
        ] {
            receipt[field] = result[field].clone();
        }
    }
    receipt["plan"] = plan.clone();
    receipt
}

fn invoke(
    inputs: &Inputs,
    kind: String,
    selection: Vec<String>,
    source_project: String,
    digest: Option<String>,
) -> Result<Value, Failure> {
    let mut session =
        ds_cli_auth::solar_project_session_for_project(lane(inputs), inputs.require("project")?)?;
    session
        .execute(&ds_cli_auth::SolarProjectCommand::Migrate {
            kind,
            source_project,
            selection,
            overwrite: inputs.switch("overwrite"),
            digest,
        })
        .map_err(classify_migration_failure)
}

/// Name the conditions ds-brain gives a stable code, so a caller branching on
/// `error.code` sees what the server saw. The match is on the CODE, not prose.
pub fn classify_migration_failure(failure: Failure) -> Failure {
    // No paired-window classification here. This command is `Requires::Server`
    // and answers from the native session, so a desktop bridge refusal is not
    // one of its outcomes; reaching for the lens to say so would make a core
    // command need a window (ds-lens-core-boundary.md §4 L0).
    let Some(detail) = failure.detail_value() else {
        return failure;
    };
    let native_code = detail["service_code"].as_str();
    let reported = if failure.code() == "desktop_refused" {
        format!(
            "{} {}",
            detail["code"].as_str().unwrap_or_default(),
            detail["detail"].as_str().unwrap_or_default(),
        )
    } else {
        String::new()
    };
    let Some((server_code, code)) = SERVER_CODES.iter().find(|(server_code, code)| {
        native_code == Some(*code) || (!reported.is_empty() && reported.contains(server_code))
    }) else {
        return failure;
    };
    let refusal = APPLY_REFUSALS
        .iter()
        .find(|refusal| refusal.code == *code)
        .expect("every mapped server code has a declared refusal");
    let detail = serde_json::json!({ SERVER_CODE_DETAIL: server_code });
    let named = match *code {
        "solar_migrate_digest_mismatch" => Failure::conflict(*code, refusal.when),
        "solar_migrate_component_disabled" => Failure::unauthorized(*code, refusal.when),
        _ => Failure::invalid(*code, refusal.when),
    };
    named.remedy(refusal.remedy).detail(detail)
}

/// Refuse a reply that is not the contract's own shape.
///
/// Mirrors a boundary parser rather than a cast: a plan with no digest cannot
/// be confirmed and one with no kind cannot be told from the other kind's plan,
/// so neither is safe to hand back as a plan.
fn migration_plan<'a>(result: &'a Value, operation: &'static str) -> Result<&'a Value, Failure> {
    let plan = if result["plan"].is_object() {
        &result["plan"]
    } else {
        result
    };
    for field in ["kind", "root", "source_project", "migrate_digest"] {
        if plan[field].as_str().is_none_or(str::is_empty) {
            return Err(mismatch(
                operation,
                &format!("the migration plan carries no `{field}`"),
            ));
        }
    }
    if !KINDS.contains(&plan["kind"].as_str().unwrap_or_default()) {
        return Err(mismatch(
            operation,
            "the migration plan names a kind this contract does not define",
        ));
    }
    // Both arrays are always present, of both kinds — the empty one included —
    // so a renderer reads one shape and an absent array is a malformed reply
    // rather than a kind that happens to have no rows.
    for field in ["cities", "portfolios"] {
        if !plan[field].is_array() {
            return Err(mismatch(
                operation,
                &format!("the migration plan carries no `{field}` rows"),
            ));
        }
    }
    if !plan["travels"].is_object() {
        return Err(mismatch(
            operation,
            "the migration plan does not declare what travels",
        ));
    }
    Ok(plan)
}

fn mismatch(operation: &'static str, detail: &str) -> Failure {
    Failure::unavailable(
        "solar_migrate_contract_mismatch",
        format!("the Solar service returned an invalid reply for `{operation}`: {detail}"),
    )
    .remedy("update ds and its native client core to matching releases")
}

/// The human tier. Every item that does not move is named, because the whole
/// value of a plan is the rows nobody planned for: a `refused` portfolio whose
/// members are absent, a `target_exists` destination that will not be
/// overwritten, a `missing_source` object, and the documents a migrated city
/// will not have. The totals line is the one `ds design migrate` prints.
pub fn render(data: &Value) -> String {
    let plan = &data["plan"];
    let count = |field: &str| data[field].as_u64().unwrap_or(0);
    let mut out = format!(
        "kind      {}\nsource    {}\ninto      {}\ndigest    {}\n",
        data["kind"].as_str().unwrap_or("?"),
        data["source_project"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["migrate_digest"].as_str().unwrap_or("?"),
    );
    out.push_str(&format!(
        "{:<9} requested {}  ·  moving {}  ·  identical {}  ·  blocked {}  ·  failed {}  ({} documents, {} excluded)\n",
        data["mode"].as_str().unwrap_or("?"),
        count("requested"),
        count("moving"),
        count("identical"),
        count("blocked"),
        count("failed"),
        plan["document_count"].as_u64().unwrap_or(0),
        plan["excluded_document_count"].as_u64().unwrap_or(0),
    ));
    if let Some(state) = data["empty_state"].as_str() {
        out.push_str(&format!("nothing moved: {state}\n"));
    }

    for item in data["items"].as_array().into_iter().flatten() {
        let status = item["status"].as_str().unwrap_or("?");
        if Outcome::parse(status).is_some_and(Outcome::moves) {
            continue;
        }
        let name = item["name"].as_str().unwrap_or("?");
        // A refused portfolio names the member cities the destination lacks.
        let missing = plan["portfolios"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| item["kind"] == KIND_PORTFOLIO && row["name"].as_str() == Some(name))
            .and_then(|row| row["missing_cities"].as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|joined| !joined.is_empty())
            .map(|joined| format!(" [{joined}]"))
            .unwrap_or_default();
        out.push_str(&format!(
            "{status:<14} {name}  {}{missing}\n",
            item["reason"].as_str().unwrap_or(""),
        ));
    }

    // The declared result set, always shown: an operator must not have to read
    // documentation to learn that the numbers stayed behind.
    for exclusion in plan["travels"]["excluded"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "excluded  {}  {}\n",
            exclusion["path"].as_str().unwrap_or("?"),
            exclusion["reason"].as_str().unwrap_or(""),
        ));
    }

    if data["mode"] == Mode::Apply.wire() {
        out.push_str(&format!(
            "applied   {} objects, {} documents{}\n",
            count("moving"),
            count("documents_written"),
            if data["idempotent"] == Value::Bool(true) {
                " (idempotent; nothing was written)"
            } else {
                ""
            },
        ));
        out.push_str(&format!(
            "results   {}\n",
            if data["computation_results_migrated"] == Value::Bool(false) {
                "not migrated — the destination recomputes"
            } else {
                "UNEXPECTED: the receipt claims results migrated"
            },
        ));
    }
    for warning in data["warnings"].as_array().into_iter().flatten() {
        out.push_str(&format!("warning   {}\n", warning.as_str().unwrap_or("?")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inputs(command: &'static Command, tokens: &[&str]) -> Inputs {
        let mut tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        tokens.extend([
            "--project".to_owned(),
            "chad-test".to_owned(),
            "--source-project".to_owned(),
            "aderm".to_owned(),
        ]);
        ds_cli_contract::parse(command, &tokens).expect("declared tokens parse")
    }

    #[test]
    fn a_kind_outside_the_solar_domain_is_refused_locally() {
        let parsed = inputs(&PLAN_COMMAND, &["--kind", "transformer"]);
        let failure = envelope(&parsed).expect_err("a design kind must not reach the server");
        assert_eq!(failure.code(), "solar_migrate_kind_invalid");
    }

    /// One `--item` flag for both kinds: the kind decides what it names, so a
    /// portfolio name on a portfolio migration and a city id on a city
    /// migration travel through the same flag.
    #[test]
    fn one_item_flag_carries_either_kinds_selection() {
        for (kind, item) in [("city", "fianga"), ("portfolio", "North, phase 1")] {
            let parsed = inputs(&PLAN_COMMAND, &["--kind", kind, "--item", item]);
            let (resolved, selection, _) = envelope(&parsed).expect("envelope resolves");
            assert_eq!(resolved, kind);
            // A name holding a comma stays one name.
            assert_eq!(selection, vec![item.to_string()]);
        }
    }

    #[test]
    fn a_repeated_object_is_refused_rather_than_deduplicated() {
        let parsed = inputs(
            &PLAN_COMMAND,
            &["--kind", "city", "--item", "fianga", "--item", "fianga"],
        );
        let failure = envelope(&parsed).expect_err("a repeated object must be refused");
        assert_eq!(failure.code(), "solar_migrate_selection_invalid");
    }

    #[test]
    fn a_selection_beyond_the_shared_bound_is_refused_locally() {
        let mut tokens = vec!["--kind".to_owned(), "city".to_owned()];
        for index in 0..=MAX_SELECTION {
            tokens.push("--item".to_owned());
            tokens.push(format!("city-{index}"));
        }
        let borrowed: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let parsed = inputs(&PLAN_COMMAND, &borrowed);
        let failure = envelope(&parsed).expect_err("an over-large selection must be refused");
        assert_eq!(failure.code(), "solar_migrate_bounded");
        // The bound is named with its basis, as design's is.
        let remedy = failure.remedy_text().unwrap_or_default();
        assert!(
            remedy.contains("64") && remedy.contains("seed writer"),
            "{remedy}"
        );
    }

    #[test]
    fn a_source_equal_to_the_destination_is_refused_locally() {
        let tokens = [
            "--kind",
            "city",
            "--project",
            "chad-test",
            "--source-project",
            "chad-test",
        ];
        let tokens: Vec<String> = tokens.iter().map(|t| (*t).to_string()).collect();
        let parsed = ds_cli_contract::parse(&PLAN_COMMAND, &tokens).expect("tokens parse");
        let failure = envelope(&parsed).expect_err("a project must not migrate into itself");
        assert_eq!(failure.code(), "solar_migrate_source_invalid");
    }

    #[test]
    fn a_well_formed_city_migration_resolves_its_envelope() {
        let parsed = inputs(&PLAN_COMMAND, &["--kind", "city", "--item", "fianga"]);
        let (kind, selection, from) = envelope(&parsed).expect("a well-formed envelope resolves");
        assert_eq!(kind, KIND_CITY);
        assert_eq!(selection, vec!["fianga".to_string()]);
        assert_eq!(from, "aderm");
    }

    #[test]
    fn a_truncated_digest_is_refused_before_any_round_trip() {
        assert!(validate_digest("abc").is_err());
        assert!(validate_digest(&"a".repeat(64)).is_ok());
    }

    fn valid_plan() -> Value {
        json!({
            "kind": "city",
            "root": "eds_project/chad-test/eds_solar",
            "ds_project": "chad-test",
            "source_project": "aderm",
            "migrate_digest": "a".repeat(64),
            "mutated": false,
            "cities": [],
            "portfolios": [],
            "travels": {"excluded": []},
        })
    }

    #[test]
    fn a_plan_without_a_kind_is_not_a_plan() {
        let mut plan = valid_plan();
        plan["kind"] = json!("");
        assert!(migration_plan(&plan, PLAN_OPERATION).is_err());
    }

    #[test]
    fn a_plan_that_does_not_declare_what_travels_is_refused() {
        let mut plan = valid_plan();
        plan["travels"] = json!(null);
        assert!(migration_plan(&plan, PLAN_OPERATION).is_err());
    }

    #[test]
    fn a_plan_missing_the_other_kinds_empty_array_is_refused() {
        let mut plan = valid_plan();
        plan["portfolios"] = json!(null);
        assert!(migration_plan(&plan, PLAN_OPERATION).is_err());
    }

    /// Every Solar row action lands on the ONE receipt vocabulary design
    /// speaks, in both tenses, and nothing is dropped.
    #[test]
    fn the_plan_speaks_the_shared_outcome_vocabulary() {
        let mut plan = valid_plan();
        plan["cities"] = json!([
            {"city_id": "a", "action": "create"},
            {"city_id": "b", "action": "replace"},
            {"city_id": "c", "action": "skip", "reason": "already_migrated"},
            {"city_id": "d", "action": "changed", "reason": "destination_differs"},
            {"city_id": "e", "action": "missing"},
        ]);
        plan["warnings"] = json!(["computation_results_are_not_migrated"]);
        let shaped = receipt(&json!({"plan": plan}), Mode::Plan, "stable");
        let statuses: Vec<&str> = shaped["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["status"].as_str().unwrap())
            .collect();
        assert_eq!(
            statuses,
            [
                "would_copy",
                "would_replace",
                "identical",
                "target_exists",
                "missing_source"
            ]
        );
        assert_eq!(shaped["requested"], 5);
        assert_eq!(shaped["moving"], 2);
        assert_eq!(shaped["identical"], 1);
        assert_eq!(shaped["blocked"], 2);
        assert_eq!(shaped["failed"], 0);
        assert!(shaped["empty_state"].is_null());
        assert_eq!(
            shaped["warnings"],
            json!(["computation_results_are_not_migrated"])
        );
        assert_eq!(shaped["items"][2]["reason"], "already_migrated");
        // The digest-bound document is kept verbatim beside the projection.
        assert_eq!(shaped["plan"]["cities"][0]["action"], "create");
        assert_eq!(shaped["migrate_digest"], "a".repeat(64));
    }

    /// The apply reads the service's own lists: a written row is `copied`, a
    /// row the destination gained since the plan is `target_exists`, a
    /// refused portfolio stays `refused`.
    #[test]
    fn the_apply_reports_committed_outcomes_from_the_service_lists() {
        let mut plan = valid_plan();
        plan["kind"] = json!("portfolio");
        plan["portfolios"] = json!([
            {"name": "North", "action": "create"},
            {"name": "South", "action": "create"},
            {"name": "East", "action": "blocked", "reason": "member_cities_missing_in_destination",
             "missing_cities": ["x"]},
        ]);
        let result = json!({
            "plan": plan,
            "migrate_digest": "a".repeat(64),
            "applied": ["North"],
            "skipped": ["South"],
            "blocked": ["East"],
            "documents_written": 1,
            "idempotent": false,
            "computation_results_migrated": false,
        });
        let shaped = receipt(&result, Mode::Apply, "canary");
        let statuses: Vec<&str> = shaped["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["status"].as_str().unwrap())
            .collect();
        assert_eq!(statuses, ["copied", "target_exists", "refused"]);
        assert_eq!(shaped["mode"], "apply");
        assert_eq!(shaped["moving"], 1);
        // `blocked` is the shared COUNT, never the service's name list.
        assert_eq!(shaped["blocked"], 2);
        assert_eq!(shaped["documents_written"], 1);
        assert_eq!(shaped["computation_results_migrated"], false);
        let rendered = render(&shaped);
        assert!(rendered.contains("refused"), "{rendered}");
        assert!(rendered.contains("[x]"), "{rendered}");
    }

    /// A run that moves nothing says why in the shape design uses.
    #[test]
    fn an_empty_plan_states_why_nothing_moved() {
        let mut plan = valid_plan();
        plan["warnings"] = json!(["nothing_was_selected_to_migrate"]);
        let shaped = receipt(&json!({"plan": plan}), Mode::Plan, "stable");
        assert_eq!(shaped["empty_state"], "mig_empty_nothing_requested");
        let rendered = render(&shaped);
        assert!(
            rendered.contains("nothing moved: mig_empty_nothing_requested"),
            "{rendered}"
        );
        assert!(
            rendered.contains("nothing_was_selected_to_migrate"),
            "{rendered}"
        );
    }

    /// The fields both migration domains' receipts carry, under one name each
    /// (docs/reference/migration.md). `ds-cli-design` pins the same list.
    #[test]
    fn the_receipt_carries_the_shared_migration_fields() {
        let shaped = receipt(&json!({"plan": valid_plan()}), Mode::Plan, "stable");
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
                shaped.get(field).is_some(),
                "missing shared field `{field}`"
            );
        }
        assert_eq!(shaped["project"]["ds_project"], "chad-test");
        assert!(shaped["warnings"].is_array());
    }

    #[test]
    fn a_receipt_claiming_results_migrated_never_renders_as_success() {
        let receipt = json!({
            "mode": "apply",
            "plan": valid_plan(),
            "migrate_digest": "a".repeat(64),
            "computation_results_migrated": true,
        });
        let rendered = render(&receipt);
        assert!(rendered.contains("UNEXPECTED"), "{rendered}");
    }

    #[test]
    fn a_rendered_plan_always_states_the_result_set_it_left_behind() {
        let mut plan = valid_plan();
        plan["travels"]["excluded"] = json!([
            {"path": "02_site_prep", "reason": "computation_result"},
        ]);
        let rendered = render(&receipt(&json!({"plan": plan}), Mode::Plan, "stable"));
        assert!(rendered.contains("excluded  02_site_prep"), "{rendered}");
    }
}
