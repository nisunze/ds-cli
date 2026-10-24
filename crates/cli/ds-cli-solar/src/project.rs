//! File-only adapters to the Solar-owned offline project lifecycle.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

const WORKSPACE: Arg = Arg::value(
    "workspace",
    "<dir>",
    "Private local Solar project workspace.",
)
.required();

/// The local owner's own bounds, held here so a selection outside them is
/// refused before a request file and an engine process exist, with the code
/// and remedy `--help` documents — not returned as `engine_refused` carrying
/// the owner's prose after the round trip. `ds-solar-project` seeds 1..64
/// cities atomically, computes 1..64 explicitly selected cities per run and
/// accepts concurrency 1..32; this is the same reasoning as
/// `ds_command_kernel::solar_seed::MAX_CITIES` on the governed lane.
const MAX_PROJECT_SEED_INPUTS: usize = 64;
const MAX_PROJECT_RUN_CITIES: usize = MAX_PROJECT_SEED_INPUTS;
const MAX_PROJECT_CONCURRENCY: usize = 32;

const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    args: &'static [Arg],
    effect: Effect,
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose: "The Rust Solar owner stores project inputs, immutable run inputs, local drafts and authored-input publication intents. No map, Desktop, sign-in or network is required. A verified reference cache must already be available for calculation. Local project attribution grants no cloud authority.",
        chapter: Chapter::Solar,
        effect,
        authority: Authority::None,
        execution: Execution::Sync,
        args,
        output: "Bounded Solar project receipt. Local results and pending publication remain separate.",
        examples: &[],
        refusals: &[
            Refusal {
                code: "solar_engine_update_required",
                when: "a reference preparation operation needs a task the selected Solar addon does not declare",
                remedy: "install a Solar addon for the selected lane that declares raw-reference preparation",
            },
            Refusal {
                code: "solar_project_inputs",
                when: "a seed carries no --input, or more than 64",
                remedy: "pass one --input per city, 1 through 64",
            },
            Refusal {
                code: "solar_project_cities",
                when: "a run selects no city, or more than 64",
                remedy: "select 1 through 64 cities, one per --city",
            },
            Refusal {
                code: "solar_project_concurrency",
                when: "the concurrency argument is not an integer from 1 through 32",
                remedy: "use a concurrency from 1 through 32",
            },
            Refusal {
                code: "solar_project_revision",
                when: "an expected revision is malformed or duplicated",
                remedy: "pass each expected city once as city=digest",
            },
            Refusal {
                code: "solar_project_sequence",
                when: "the upload sequence is not an integer",
                remedy: "use the sequence returned by project outbox",
            },
            Refusal {
                code: "engine_refused",
                when: "the local owner rejects an input, revision, path or cache",
                remedy: "inspect the owner error and retry with the exact project inputs and verified reference cache",
            },
        ],
        reference: Some("docs/reference/solar.md"),
        search: &[],
        requires: Requires::Server,
        availability,
    }
}
pub(crate) fn availability() -> Availability {
    Availability::Available
}
pub static INIT: Command = command(
    "solar.project.init",
    &["solar", "project", "init"],
    "Create an offline Solar project workspace.",
    &[
        WORKSPACE,
        Arg::value(
            "project",
            "<id>",
            "Project attribution; not cloud authorization.",
        )
        .required(),
    ],
    Effect::LocalFileWrite,
);
pub static SEED: Command = command(
    "solar.project.seed",
    &["solar", "project", "seed"],
    "Atomically import Solar cities and queue publication.",
    &[
        WORKSPACE,
        Arg::repeated(
            "input",
            "<file>",
            "Complete city input or governed intake; repeat for up to 64 cities.",
        ),
        Arg::repeated(
            "expected",
            "<city=digest>",
            "Required previous local digest when replacing a city.",
        ),
    ],
    Effect::LocalFileWrite,
);
pub static RUN: Command = Command {
    authority: Authority::HeadlessProject,
    purpose: "Run captured Solar-owned input snapshots and manual edits without refreshing network assets. Seeding or refreshing is a separate explicit operation. Copied maps and sizing data belong to Solar; missing geographic data never blocks manual input. A verified reference cache must already exist. The selected lane's Solar addon calculates by typed request, offline; the workspace verifies and commits its closed batch.",
    refusals: &[
        Refusal {
            code: "solar_engine_update_required",
            when: "the captured Solar addon does not declare raw-reference preparation",
            remedy: "install a Solar addon for the selected lane that declares the required preparation task",
        },
        Refusal {
            code: "solar_engine_missing",
            when: "the selected lane has no managed Solar addon and no explicit development override",
            remedy: "install the Solar addon for the selected lane, or explicitly set DS_SOLAR_ENGINE_BIN for development",
        },
        Refusal {
            code: "module_receipt_invalid",
            when: "the selected managed receipt or retained executable fails verification",
            remedy: "repair the selected installation; inspect the run before explicitly retrying",
        },
        Refusal {
            code: "solar_project_cities",
            when: "a run selects no city, or more than 64",
            remedy: "select 1 through 64 cities, one per --city",
        },
        Refusal {
            code: "solar_project_concurrency",
            when: "the concurrency argument is not an integer from 1 through 32",
            remedy: "use a concurrency from 1 through 32",
        },
        Refusal {
            code: "engine_refused",
            when: "the local owner rejects an input, revision, path or cache, or the addon refuses the captured run",
            remedy: "inspect the owner error and retry with the exact project inputs and verified reference cache",
        },
    ],
    ..command(
        "solar.project.run",
        &["solar", "project", "run"],
        "Produce Solar drafts from independent seeded or manual inputs.",
        &[
            WORKSPACE,
            Arg::value("cache", "<dir>", "Existing verified reference cache.").required(),
            Arg::value(
                "run-id",
                "<id>",
                "Stable identity for restart-safe execution.",
            )
            .required(),
            Arg::repeated("city", "<id>", "Required explicit city selection, 1..64.").required(),
            Arg::value(
                "concurrency",
                "<count>",
                "Parallel cities, 1..32; default 2.",
            ),
            Arg::repeated(
                "draft",
                "<kind>",
                "apd, network, plant or financial; default apd.",
            ),
            Arg::switch("charts", "Produce chart images."),
            ds_cli_contract::spec::Arg {
                summary: "Native Solar addon lane; stable is the default. No account or project is read.",
                default: None,
                choices: &[],
                ..ds_cli_contract::spec::LANE
            },
        ],
        Effect::LocalFileWrite,
    )
};

pub static STATUS: Command = command(
    "solar.project.status",
    &["solar", "project", "status"],
    "Inspect local Solar cities, runs and pending uploads.",
    &[WORKSPACE],
    Effect::ReadOnly,
);
pub static RESULT: Command = command(
    "solar.project.result",
    &["solar", "project", "result"],
    "Verify and locate a committed local Solar result.",
    &[
        WORKSPACE,
        Arg::value("run-id", "<id>", "Committed run identity.").required(),
    ],
    Effect::ReadOnly,
);
pub static OUTBOX: Command = command(
    "solar.project.outbox",
    &["solar", "project", "outbox"],
    "Inspect pending Solar publication without connecting.",
    &[WORKSPACE],
    Effect::ReadOnly,
);
pub static CLEAN: Command = Command {
    purpose: "Delete local computed runs and their publication intents. Preserve authored inputs, parameters, copied maps and reference cache. Refuse while computation is active. Run the engine again from those inputs; fresh native runs support offline publication.",
    ..command(
        "solar.project.clean",
        &["solar", "project", "clean"],
        "Clear local computed Solar runs so they can be recomputed.",
        &[WORKSPACE, crate::portfolio_headless::PROJECT],
        Effect::GlobalWrite,
    )
};

pub fn init(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = i.require("project")?;
    invoke(
        json!({"operation":"initialize","workspace":i.require("workspace")?,"identity":{"project_id":project,"root":format!("eds_project/{project}/eds_solar")}}),
    )
}
pub fn seed(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let files = i.repeated("input");
    if files.is_empty() {
        return Err(
            Failure::invalid("solar_project_inputs", "seed requires at least one --input").remedy(
                "pass one --input per complete city input or governed intake, 1 through 64",
            ),
        );
    }
    if files.len() > MAX_PROJECT_SEED_INPUTS {
        return Err(Failure::invalid(
            "solar_project_inputs",
            format!(
                "{} inputs were given; one seed carries at most {MAX_PROJECT_SEED_INPUTS} cities",
                files.len()
            ),
        )
        .remedy("seed in sets of at most 64 cities")
        .detail(json!({ "given": files.len(), "max": MAX_PROJECT_SEED_INPUTS })));
    }
    let mut expected = serde_json::Map::new();
    for entry in i.repeated("expected") {
        let (city, digest) = entry.split_once('=').ok_or_else(|| {
            Failure::invalid(
                "solar_project_revision",
                "expected revision must be city=digest",
            )
        })?;
        if expected.insert(city.to_owned(), json!(digest)).is_some() {
            return Err(Failure::invalid(
                "solar_project_revision",
                "duplicate expected city revision",
            ));
        }
    }
    invoke(
        json!({"operation":"seed","workspace":i.require("workspace")?,"inputs":files,"expected":expected}),
    )
}
pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    run_with(i, || crate::capture_engine(i))
}
/// [`run`] over an explicit addon capture, so a test can select a qualified engine.
pub(crate) fn run_with(
    i: &Inputs,
    capture: impl FnOnce() -> Result<crate::Solar, Failure>,
) -> Result<Value, Failure> {
    let concurrency = i
        .value("concurrency")
        .unwrap_or("2")
        .parse::<usize>()
        .map_err(|_| {
            Failure::invalid("solar_project_concurrency", "concurrency must be 1..32")
                .remedy("use a concurrency from 1 through 32")
        })?;
    if !(1..=MAX_PROJECT_CONCURRENCY).contains(&concurrency) {
        return Err(Failure::invalid(
            "solar_project_concurrency",
            format!("concurrency {concurrency} is outside 1..{MAX_PROJECT_CONCURRENCY}"),
        )
        .remedy("use a concurrency from 1 through 32")
        .detail(json!({ "given": concurrency, "max": MAX_PROJECT_CONCURRENCY })));
    }
    let cities = i.repeated("city");
    if cities.is_empty() {
        return Err(
            Failure::invalid("solar_project_cities", "run requires at least one --city")
                .remedy("pass one --city per seeded city this run computes, 1 through 64"),
        );
    }
    if cities.len() > MAX_PROJECT_RUN_CITIES {
        return Err(Failure::invalid(
            "solar_project_cities",
            format!(
                "{} cities were selected; one run selects at most {MAX_PROJECT_RUN_CITIES}",
                cities.len()
            ),
        )
        .remedy("select at most 64 cities in one run")
        .detail(json!({ "given": cities.len(), "max": MAX_PROJECT_RUN_CITIES })));
    }
    let drafts = if i.repeated("draft").is_empty() {
        vec!["apd".to_owned()]
    } else {
        i.repeated("draft").to_vec()
    };
    let request = json!({"operation":"run","workspace":i.require("workspace")?,"cache":i.require("cache")?,"request":{"run_id":i.require("run-id")?,"cities":cities,"concurrency":concurrency,"charts":i.switch("charts"),"drafts":drafts}});
    // The addon is captured before any workspace effect: a missing or invalid
    // engine refuses with its own typed code and leaves no run behind.
    let engine = std::sync::Arc::new(capture()?);
    let preparation =
        std::sync::Arc::new(ds_solar_host::preparation::AddonPreparation::from_client(
            std::env::temp_dir(),
            engine.clone(),
        ));
    let lane = i.value("lane").unwrap_or("stable");
    let engine_refusal = std::cell::RefCell::new(None);
    let calculated = ds_solar_project::control::execute_with_tasks(
        request,
        Some(&|calculation| {
            crate::run::calculate_captured(&engine, lane, calculation).map_err(|failure| {
                let message = format!("{}: {}", failure.code(), failure.message());
                *engine_refusal.borrow_mut() = Some(failure);
                std::io::Error::other(message).into()
            })
        }),
        preparation,
    );
    let mut result = calculated.map_err(|error| {
        engine_refusal.take().unwrap_or_else(|| {
            if let Some(solar) = error.downcast_ref::<ds_solar_contracts::SolarError>()
                && solar.code == ds_solar_contracts::SolarErrorCode::SolarEngineUpdateRequired
            {
                return Failure::unavailable("solar_engine_update_required", &solar.message)
                    .remedy("install a Solar addon for the selected lane that declares raw-reference preparation");
            }
            Failure::failed("engine_refused", error.to_string())
                .remedy("inspect the owner refusal; use exact local project inputs and a verified reference cache")
        })
    })?;
    result["source_notices"] = json!([]);
    Ok(result)
}
pub fn status(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(json!({"operation":"status","workspace":i.require("workspace")?}))
}
pub fn result(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(
        json!({"operation":"result","workspace":i.require("workspace")?,"run_id":i.require("run-id")?}),
    )
}
pub fn outbox(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(json!({"operation":"outbox","workspace":i.require("workspace")?}))
}
pub fn clean(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(
        json!({"operation":"clean","command":{"workspace":i.require("workspace")?,"project_id":i.require("project")?}}),
    )
}

pub(crate) fn invoke(request: Value) -> Result<Value, Failure> {
    ds_solar_project::control::execute(request)
        .map_err(|error| Failure::failed("engine_refused", error.to_string())
            .remedy("inspect the owner refusal; use exact local project inputs and a verified reference cache; run calculations through solar.application"))
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

pub static CITY_CREATE: Command = Command {
    purpose: "Create an editable Solar city from a partial input snapshot or empty manual form. No geographic data, map, classified table or sign-in is required. Solar owns copied inputs and maps; manual values remain editable. Repeating creation preserves the existing city and its edits. Use city read/write to revise it, network resolve/save to copy network or compose manual inputs, and project run after completing calculation inputs.",
    output: "City identity, content_digest, created, editable, publication and calculation readiness. Missing inputs are an editable draft, not a refusal.",
    ..command(
        "solar.project.city.create",
        &["solar", "project", "city", "create"],
        "Create an editable Solar city with optional seed inputs.",
        &[
            WORKSPACE,
            Arg::value("city", "<id>", "Stable city identity.").required(),
            Arg::value("name", "<text>", "Display name for the new city.").required(),
            Arg::value(
                "input",
                "<file>",
                "Optional partial canonical snapshot JSON: _root, _config, 00_templates and 01_city_inputs. Network inputs are independent Solar snapshots with optional source provenance.",
            ),
        ],
        Effect::LocalFileWrite,
    )
};
pub fn city_create(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(
        json!({"operation":"city_create","workspace":i.require("workspace")?,"city":i.require("city")?,"display_name":i.require("name")?,"input":i.value("input")}),
    )
}

pub static CITY_READ: Command = command(
    "solar.project.city.read",
    &["solar", "project", "city", "read"],
    "Export a local Solar city snapshot for editing.",
    &[
        WORKSPACE,
        Arg::value("city", "<id>", "Local city.").required(),
        Arg::value("out", "<file>", "Absent private snapshot output.").required(),
    ],
    Effect::LocalFileWrite,
);
pub static CITY_WRITE: Command = command(
    "solar.project.city.write",
    &["solar", "project", "city", "write"],
    "Create or replace a Solar city from a complete local snapshot.",
    &[
        WORKSPACE,
        Arg::value("city", "<id>", "City identity.").required(),
        Arg::value("snapshot", "<file>", "Complete local city snapshot.").required(),
        Arg::value(
            "expected",
            "<digest>",
            "Previous local city digest, required for replacement.",
        ),
    ],
    Effect::LocalFileWrite,
);
pub fn city_read(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(
        json!({"operation":"city_read","workspace":i.require("workspace")?,"city":i.require("city")?,"out":i.require("out")?}),
    )
}
pub fn city_write(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(
        json!({"operation":"city_write","workspace":i.require("workspace")?,"city":i.require("city")?,"snapshot":i.require("snapshot")?,"expected":i.value("expected")}),
    )
}

pub static REBASE: Command = command(
    "solar.project.sync.rebase",
    &["solar", "project", "sync", "rebase"],
    "Rebase a queued city onto a reviewed cloud revision.",
    &[
        WORKSPACE,
        Arg::value("sequence", "<id>", "Oldest pending city upload sequence.").required(),
        Arg::value(
            "expected-cloud",
            "<fingerprint>",
            "Exact cloud fingerprint obtained by capturing and reviewing the current city.",
        )
        .required(),
    ],
    Effect::LocalFileWrite,
);
pub fn rebase(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let sequence = i
        .require("sequence")?
        .parse::<i64>()
        .map_err(|_| Failure::invalid("solar_project_sequence", "sequence must be an integer"))?;
    invoke(
        json!({"operation":"sync_rebase","workspace":i.require("workspace")?,"sequence":sequence,"expected_cloud":i.require("expected-cloud")?}),
    )
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
