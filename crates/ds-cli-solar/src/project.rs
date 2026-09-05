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
                code: "solar_project_concurrency",
                when: "the concurrency argument is not an integer",
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
        Arg::repeated("city", "<id>", "Explicit city selection."),
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
        json!({"operation":"seed","workspace":i.require("workspace")?,"inputs":i.repeated("input"),"expected":expected}),
    )
}
pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let concurrency = i
        .value("concurrency")
        .unwrap_or("2")
        .parse::<usize>()
        .map_err(|_| Failure::invalid("solar_project_concurrency", "concurrency must be 1..32"))?;
    let drafts = if i.repeated("draft").is_empty() {
        vec!["apd".to_owned()]
    } else {
        i.repeated("draft").to_vec()
    };
    invoke(
        json!({"operation":"run","workspace":i.require("workspace")?,"cache":i.require("cache")?,"request":{"run_id":i.require("run-id")?,"cities":i.repeated("city"),"concurrency":concurrency,"charts":i.switch("charts"),"drafts":drafts}}),
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
