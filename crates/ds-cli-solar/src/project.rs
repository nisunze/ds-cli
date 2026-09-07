//! File-only adapters to the Solar-owned offline project lifecycle.
use crate::{DISCOVERY_TIMEOUT, DS_SOLAR, RUN_TIMEOUT};
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    io::{Read, Write},
};

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
/// `crate::seed::MAX_CITIES` on the governed lane.
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
        purpose: "The Rust Solar owner stores project inputs, immutable run inputs, drafts and publication intents locally. No map, Desktop, sign-in or network is required. A verified reference cache must already be available for calculation. Local project attribution grants no cloud authority.",
        chapter: Chapter::Solar,
        effect,
        authority: Authority::None,
        execution: Execution::Sync,
        args,
        output: "Bounded Solar project receipt. Local results and pending publication remain separate.",
        examples: &[],
        refusals: &[
            Refusal {
                code: "solar_project_schema_unavailable",
                when: "the packaged Solar owner lacks the workspace schema",
                remedy: "install matching ds and ds-solar releases",
            },
            Refusal {
                code: "solar_project_io",
                when: "a bounded owner request or receipt file cannot be handled",
                remedy: "verify private writable directories and matching Solar releases",
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
        availability,
    }
}
fn availability() -> Availability {
    DS_SOLAR.availability()
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
pub static RUN: Command = command(
    "solar.project.run",
    &["solar", "project", "run"],
    "Prepare, compute and produce drafts entirely offline.",
    &[
        WORKSPACE,
        Arg::value("cache", "<dir>", "Existing verified reference cache.").required(),
        Arg::value(
            "run-id",
            "<id>",
            "Stable identity for restart-safe execution.",
        )
        .required(),
        Arg::repeated("city", "<id>", "Explicit city selection, 1..64."),
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
    ],
    Effect::LocalFileWrite,
);
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
    invoke(
        json!({"operation":"run","workspace":i.require("workspace")?,"cache":i.require("cache")?,"request":{"run_id":i.require("run-id")?,"cities":cities,"concurrency":concurrency,"charts":i.switch("charts"),"drafts":drafts}}),
    )
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

pub(crate) fn invoke(request: Value) -> Result<Value, Failure> {
    let identity = DS_SOLAR.call_json("build-info", &[], DISCOVERY_TIMEOUT)?;
    if !identity["schemas"]
        .as_array()
        .is_some_and(|a| a.iter().any(|s| s == "ds-solar.project-workspace/v1"))
    {
        return Err(Failure::unavailable(
            "solar_project_schema_unavailable",
            "install matching ds and ds-solar with the offline project contract",
        ));
    }
    let temp = tempfile::Builder::new()
        .prefix("ds-solar-project-")
        .tempdir()
        .map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700))
            .map_err(io_error)?;
    }
    let input = temp.path().join("request.json");
    let output = temp.path().join("result.json");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&input)
        .map_err(io_error)?
        .write_all(&serde_json::to_vec(&request).map_err(io_error)?)
        .map_err(io_error)?;
    let result = DS_SOLAR.call(
        "project",
        &[
            OsString::from("--request"),
            input.into_os_string(),
            OsString::from("--result"),
            output.clone().into_os_string(),
        ],
        RUN_TIMEOUT,
    )?;
    if !result.succeeded() {
        return Err(DS_SOLAR.failure_from(&result, "project"));
    }
    let meta = std::fs::symlink_metadata(&output).map_err(io_error)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 32 * 1024 * 1024 {
        return Err(io_error("invalid owner result file"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(output)
        .map_err(io_error)?
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(io_error("owner result exceeds bound"));
    }
    serde_json::from_slice(&bytes).map_err(io_error)
}
fn io_error(error: impl std::fmt::Display) -> Failure {
    Failure::failed(
        "solar_project_io",
        format!("Solar local IO failed: {error}"),
    )
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
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
mod tests {
    use super::*;
    use ds_cli_contract::spec::Command;
    use ds_cli_contract::{ExitClass, Format, Output, parse};

    fn inputs(command: &'static Command, tokens: &[String]) -> Inputs {
        parse(command, tokens).expect("declared tokens parse")
    }

    fn context() -> Context {
        Context {
            confirmed: true,
            output: Output::resolve(Format::Json, false, true),
        }
    }

    fn workspace() -> Vec<String> {
        vec![
            "--workspace".to_owned(),
            "/nonexistent/ds-solar-project".to_owned(),
        ]
    }

    fn repeat(tokens: &mut Vec<String>, flag: &str, count: usize) {
        for index in 0..count {
            tokens.push(flag.to_owned());
            tokens.push(format!("city-{index:03}"));
        }
    }

    // These call the handlers directly on purpose. Dispatch applies the
    // availability gate first, so an end-to-end `ds solar project seed` on a
    // box without ds-solar refuses as unavailable and never reaches the
    // bound. Reaching it here also proves the refusal arrives before
    // `invoke`, which is the whole point: no request file, no engine process,
    // no `engine_refused` carrying the owner's prose.
    #[test]
    fn seed_refuses_an_empty_input_set_before_reaching_the_owner() {
        let error = seed(&inputs(&SEED, &workspace()), &context())
            .expect_err("a seed with no --input is refused");
        assert_eq!(error.class(), ExitClass::InvalidInput);
        assert_eq!(error.code(), "solar_project_inputs");
        assert!(
            error.remedy_text().is_some_and(|remedy| remedy.len() > 10),
            "the refusal must say how to get out of it"
        );
    }

    #[test]
    fn seed_refuses_more_inputs_than_one_atomic_import_carries() {
        let mut tokens = workspace();
        repeat(&mut tokens, "--input", MAX_PROJECT_SEED_INPUTS + 1);
        let error = seed(&inputs(&SEED, &tokens), &context())
            .expect_err("65 inputs exceed the owner's atomic seed");
        assert_eq!(error.class(), ExitClass::InvalidInput);
        assert_eq!(error.code(), "solar_project_inputs");
        assert_eq!(
            error.detail_value(),
            Some(&json!({ "given": MAX_PROJECT_SEED_INPUTS + 1, "max": MAX_PROJECT_SEED_INPUTS }))
        );
    }

    #[test]
    fn run_refuses_a_concurrency_outside_the_range_its_help_states() {
        let mut tokens = workspace();
        tokens.extend([
            "--cache".to_owned(),
            "/nonexistent/cache".to_owned(),
            "--run-id".to_owned(),
            "r1".to_owned(),
            "--concurrency".to_owned(),
            "0".to_owned(),
        ]);
        let error =
            run(&inputs(&RUN, &tokens), &context()).expect_err("0 is not a concurrency of 1..32");
        assert_eq!(error.class(), ExitClass::InvalidInput);
        assert_eq!(error.code(), "solar_project_concurrency");
    }

    // Both branches of `solar_project_concurrency` are the same code to a
    // caller, so both have to hand back the same way out. The non-integer
    // branch predates the range check and carried none.
    #[test]
    fn every_concurrency_refusal_carries_the_remedy_its_help_declares() {
        for given in ["0", "33", "abc"] {
            let mut tokens = workspace();
            tokens.extend([
                "--cache".to_owned(),
                "/nonexistent/cache".to_owned(),
                "--run-id".to_owned(),
                "r1".to_owned(),
                "--city".to_owned(),
                "city-000".to_owned(),
                "--concurrency".to_owned(),
                given.to_owned(),
            ]);
            let error = run(&inputs(&RUN, &tokens), &context())
                .err()
                .unwrap_or_else(|| panic!("`--concurrency {given}` is not a concurrency"));
            assert_eq!(
                error.class(),
                ExitClass::InvalidInput,
                "--concurrency {given}"
            );
            assert_eq!(
                error.code(),
                "solar_project_concurrency",
                "--concurrency {given}"
            );
            assert!(
                error.remedy_text().is_some_and(|remedy| remedy.len() > 10),
                "`--concurrency {given}` refuses with no way out"
            );
        }
    }

    #[test]
    fn run_refuses_a_run_that_selects_no_city_at_all() {
        let mut tokens = workspace();
        tokens.extend([
            "--cache".to_owned(),
            "/nonexistent/cache".to_owned(),
            "--run-id".to_owned(),
            "r1".to_owned(),
        ]);
        let error = run(&inputs(&RUN, &tokens), &context())
            .expect_err("a run without an explicit city has nothing to compute");
        assert_eq!(error.class(), ExitClass::InvalidInput);
        assert_eq!(error.code(), "solar_project_cities");
    }

    #[test]
    fn run_refuses_more_selected_cities_than_the_owner_accepts() {
        let mut tokens = workspace();
        tokens.extend([
            "--cache".to_owned(),
            "/nonexistent/cache".to_owned(),
            "--run-id".to_owned(),
            "r1".to_owned(),
        ]);
        repeat(&mut tokens, "--city", MAX_PROJECT_RUN_CITIES + 1);
        let error = run(&inputs(&RUN, &tokens), &context())
            .expect_err("65 cities exceed the owner's selection");
        assert_eq!(error.class(), ExitClass::InvalidInput);
        assert_eq!(error.code(), "solar_project_cities");
    }
}
