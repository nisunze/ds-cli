//! Thin command/HTTP host for the shared native compute runtime.
mod auth;
mod host;
pub mod server_sync;
mod solar_sync;
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{
        Arg, Authority, Availability, Chapter, Command, Domain, Effect, Example, Execution, Refusal,
    },
};
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf, sync::Arc};

const STATE: Arg = Arg::value(
    "state-dir",
    "<absolute-path>",
    "Protected server state directory; defaults to the lane's user state directory.",
);
const LANE: Arg = Arg::value("lane", "<stable|canary>", "Native authentication lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
const JOB: Arg = Arg::value("job", "<id>", "Exact job id returned by submit.").required();
const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "server_platform_unsupported",
        when: "the native server is requested outside Linux",
        remedy: "run these commands on the Linux server, locally or over SSH",
    },
    Refusal {
        code: "server_refused",
        when: "native authentication, protected state, job identity, worker capacity or the server request is invalid or unavailable",
        remedy: "read the stated reason; verify ds auth status, the protected state directory and that ds server serve is running",
    },
    Refusal {
        code: "server_output_exists",
        when: "the result output path already exists",
        remedy: "choose an absent output file; existing files are never overwritten",
    },
];
const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    effect: Effect,
    execution: Execution,
    args: &'static [Arg],
    examples: &'static [Example],
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose: "Drive persistent native transformer and prepared Solar computation through the shared Rust runtime. Jobs survive UI closure and server restart; complete request and result bytes are retained under the initiating native identity. A Solar request carries the sealed prepared input itself, never a client path or browser cache reference. Server control uses an owner-only local connection credential on loopback. Remote administration uses SSH. Publication is represented through Sync Center's shared durable activity owner; these controls never create a browser queue.",
        chapter: Chapter::Design,
        effect,
        authority: Authority::HeadlessUser,
        execution,
        args,
        output: "A bounded job receipt or job list; complete result bytes are saved only by server result. No credential is printed.",
        examples,
        refusals: REFUSALS,
        reference: Some("docs/reference/server.md"),
        availability: || {
            if cfg!(target_os = "linux") {
                Availability::Available
            } else {
                Availability::unavailable(
                    "server_platform_unsupported",
                    "the native server currently requires Linux",
                    "run these commands on the Linux server, locally or over SSH",
                )
            }
        },
    }
}
pub static SERVE: Command = command(
    "server.serve",
    &["server", "serve"],
    "Host durable parallel compute under the native signed-in account.",
    Effect::LocalFileWrite,
    Execution::Sync,
    &[
        STATE,
        LANE,
        Arg::value("listen", "<loopback:port>", "Fixed loopback bind address.")
            .default("127.0.0.1:19766"),
        Arg::value(
            "workers",
            "<count>",
            "Parallel jobs, bounded by measured CPU and memory; defaults to capacity.",
        ),
    ],
    &[Example {
        command: "ds server serve --lane stable",
        note: "Run after native login under the same Linux user.",
        runnable: false,
    }],
);
pub static SUBMIT: Command = command(
    "server.submit",
    &["server", "submit"],
    "Queue a complete native transformer batch and return immediately.",
    Effect::LocalFileWrite,
    Execution::Job,
    &[
        STATE,
        LANE,
        Arg::value(
            "input",
            "<path>",
            "Explicit ds.fast-lv.request/v1 file; at most 64 MiB.",
        )
        .required(),
        Arg::value(
            "key",
            "<idempotency-key>",
            "Stable caller key: resubmission preserves the existing job, changed bytes refuse.",
        )
        .required(),
    ],
    &[Example {
        command: "ds server submit --input transformer-batch.json --key processing-001",
        note: "Queue an explicit native transformer request on the running server.",
        runnable: false,
    }],
);
pub static SOLAR_SUBMIT: Command = command(
    "server.solar.submit",
    &["server", "solar", "submit"],
    "Queue one sealed prepared Solar calculation and return immediately.",
    Effect::LocalFileWrite,
    Execution::Job,
    &[
        STATE,
        LANE,
        Arg::value(
            "input",
            "<path>",
            "Complete ds-solar prepared calculate request; at most 64 MiB.",
        )
        .required(),
        Arg::value(
            "key",
            "<idempotency-key>",
            "Stable caller key: resubmission preserves the existing job, changed bytes refuse.",
        )
        .required(),
    ],
    &[Example {
        command: "ds server solar submit --input solar-prepared.json --key solar-001",
        note: "The input embeds prepared weather and reference bytes; no browser cache or client path is read.",
        runnable: false,
    }],
);
pub static STATUS: Command = command(
    "server.status",
    &["server", "status"],
    "Inspect durable jobs without a browser or desktop.",
    Effect::ReadOnly,
    Execution::Sync,
    &[
        STATE,
        LANE,
        Arg::value(
            "job",
            "<id>",
            "One exact job; otherwise latest 100 jobs and more flag.",
        ),
    ],
    &[Example {
        command: "ds server status --output json",
        note: "Read bounded job receipts from the running server.",
        runnable: false,
    }],
);
pub static ACTIVITY: Command = command(
    "server.activity",
    &["server", "activity"],
    "Read the server's shared Sync Center activity and publication state.",
    Effect::ReadOnly,
    Execution::Sync,
    &[STATE, LANE],
    &[Example {
        command: "ds server activity --lane canary --output json",
        note: "Read running jobs and held publication state from the authenticated server.",
        runnable: false,
    }],
);
pub static CANCEL: Command = command(
    "server.cancel",
    &["server", "cancel"],
    "Cancel a queued or running job while retaining its inputs.",
    Effect::LocalFileWrite,
    Execution::Sync,
    &[STATE, LANE, JOB],
    &[Example {
        command: "ds server cancel --job <job-id>",
        note: "Use the exact id from the submitted job receipt.",
        runnable: false,
    }],
);
pub static RESULT: Command = command(
    "server.result",
    &["server", "result"],
    "Save a completed job's full result to a new local file.",
    Effect::LocalFileWrite,
    Execution::Sync,
    &[
        STATE,
        LANE,
        JOB,
        Arg::value("out", "<path>", "Absent destination file.").required(),
    ],
    &[Example {
        command: "ds server result --job <job-id> --out result.json",
        note: "Export to an absent path after completion.",
        runnable: false,
    }],
);
pub static DOMAIN: Domain = Domain {
    id: "server",
    summary: "Native server.",
    commands: &[
        &SERVE,
        &SUBMIT,
        &SOLAR_SUBMIT,
        &STATUS,
        &ACTIVITY,
        &CANCEL,
        &RESULT,
    ],
};
fn failure(e: impl ToString) -> Failure {
    Failure::failed("server_refused", e.to_string())
        .remedy("verify native authentication, protected server state and ds server serve")
}
fn state(inputs: &Inputs) -> Result<PathBuf, Failure> {
    if let Some(path) = inputs.value("state-dir") {
        return Ok(PathBuf::from(path));
    }
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")))
        .ok_or_else(|| failure("provide --state-dir"))?;
    Ok(root.join("ds/server").join(inputs.require("lane")?))
}
pub fn serve(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?.to_owned();
    let owner = auth::identity(&lane).map_err(failure)?;
    let directory = state(inputs)?;
    let address = inputs.require("listen")?.parse().map_err(failure)?;
    let capacity = ds_compute_runtime::capacity();
    let workers = inputs
        .value("workers")
        .map(str::parse::<usize>)
        .transpose()
        .map_err(failure)?
        .unwrap_or(capacity);
    if workers == 0 || workers > capacity {
        return Err(failure(format!(
            "workers must be 1..{capacity} for this host's CPU and memory"
        )));
    }
    let connection = host::connection(&directory, address, owner, lane.clone()).map_err(failure)?;
    let app = host::App {
        database: directory.join("store.sqlite"),
        connection,
        auth: Arc::new(auth::NativeAuthorizer::new(lane).map_err(failure)?),
        requests: Arc::new(tokio::sync::Semaphore::new(workers.min(8))),
        activity: None,
    };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(failure)?
        .block_on(host::serve(app, workers))
        .map_err(failure)?;
    Ok(json!({"stopped":true}))
}
fn request(
    inputs: &Inputs,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    limit: u64,
) -> Result<Vec<u8>, Failure> {
    let connection = host::load_connection(&state(inputs)?).map_err(failure)?;
    if connection.lane != inputs.require("lane")? {
        return Err(failure("server lane differs from the selected lane"));
    }
    let url = format!("http://{}{path}", connection.address);
    let authorization = format!("Bearer {}", connection.token);
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = if method == "GET" {
        agent
            .get(&url)
            .header("authorization", &authorization)
            .call()
    } else {
        agent
            .post(&url)
            .header("authorization", &authorization)
            .header("content-type", "application/json")
            .send(body.unwrap_or_default())
    }
    .map_err(|_| failure("could not reach the protected server; start ds server serve"))?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(failure)?;
    if bytes.len() as u64 > limit {
        return Err(failure("server response exceeds the command's bound"));
    }
    if status >= 400 {
        return Err(failure(String::from_utf8_lossy(&bytes)));
    }
    Ok(bytes)
}
fn json_request(
    inputs: &Inputs,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<Value, Failure> {
    serde_json::from_slice(&request(inputs, method, path, body, 1024 * 1024)?).map_err(failure)
}
fn id(inputs: &Inputs) -> Result<&str, Failure> {
    let id = inputs.require("job")?;
    if !ds_command_kernel::compute_jobs::digest(id) {
        return Err(failure("job id must be its exact 64-character digest"));
    }
    Ok(id)
}
pub fn submit(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    submit_at(inputs, "/v1/transformer-processing")
}
pub fn solar_submit(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    submit_at(inputs, "/v1/solar-processing")
}
fn submit_at(inputs: &Inputs, endpoint: &str) -> Result<Value, Failure> {
    let key = inputs.require("key")?;
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(failure("key must be 1..128 letters, digits, _ or -"));
    }
    let file = std::fs::File::open(inputs.require("input")?).map_err(failure)?;
    if !file.metadata().map_err(failure)?.is_file() {
        return Err(failure("input must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(failure)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(failure("input exceeds 64 MiB"));
    }
    json_request(inputs, "POST", &format!("{endpoint}/{key}"), Some(&bytes))
}
pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = if inputs.value("job").is_some() {
        format!("/v1/jobs/{}", id(inputs)?)
    } else {
        "/v1/jobs".into()
    };
    json_request(inputs, "GET", &path, None)
}
pub fn activity(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    serde_json::from_slice(&request(
        inputs,
        "GET",
        "/v1/activity",
        None,
        16 * 1024 * 1024,
    )?)
    .map_err(failure)
}
pub fn cancel(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    json_request(
        inputs,
        "POST",
        &format!("/v1/jobs/{}/cancel", id(inputs)?),
        None,
    )
}
pub fn result(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let output = PathBuf::from(inputs.require("out")?);
    if output.exists() {
        return Err(Failure::conflict(
            "server_output_exists",
            "output already exists",
        ));
    }
    let bytes = request(
        inputs,
        "GET",
        &format!("/v1/jobs/{}/result", id(inputs)?),
        None,
        256 * 1024 * 1024,
    )?;
    ds_design_workspace::write_new(&output, &bytes).map_err(failure)?;
    Ok(json!({"out":output,"byte_count":bytes.len(),"sha256":ds_compute_runtime::digest(&bytes)}))
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}
