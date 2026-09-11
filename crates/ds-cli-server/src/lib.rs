//! Thin command/HTTP host for the shared native compute runtime.
mod auth;
mod host;
mod layers;
pub mod server_sync;
mod solar_sync;
use ds_cli_contract::{
    Context, Failure, Inputs,
    outcome::ExitClass,
    spec::{
        Arg, Authority, Availability, Chapter, Command, Domain, Effect, Example, Execution, Refusal,
    },
};
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

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
pub static ENGINE: Command = Command {
    id: "server.engine",
    path: &["server", "engine"],
    contract: 1,
    summary: "Inspect the Solar engine embedded in this Server executable.",
    purpose: "Identify the exact linked Solar build for release admission and diagnostics. A bundled standalone engine can have a different dependency closure. This local inspection needs no login, running server, state directory or network connection; development builds intentionally omit release provenance.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "The owning engine's identity and stamped schema inventory. Release builds carry ds.engine-build/v1 provenance, including the linked dependency-lock and build-manifest digests.",
    examples: &[Example {
        command: "ds server engine --output json",
        note: "Inspect this binary's embedded Solar engine without starting or authenticating a server.",
        runnable: true,
    }],
    refusals: &[Refusal {
        code: "server_engine_identity_invalid",
        when: "the owning engine identity cannot be encoded",
        remedy: "rebuild the executable from the pinned engine sources and retry",
    }],
    reference: Some("docs/reference/server.md"),
    availability: || Availability::Available,
};
pub fn engine(_: &Inputs, _: &Context) -> Result<Value, Failure> {
    serde_json::to_value(ds_compute_runtime::solar_engine_identity()).map_err(|error| {
        Failure::failed("server_engine_identity_invalid", error.to_string())
            .remedy("rebuild the executable from the pinned engine sources and retry")
    })
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
            "Private ds.solar.server-submission/v1 envelope: prepared request plus matching governed publication claim; at most 64 MiB.",
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
        command: "ds server solar submit --input pala.server-submission.json --key solar-001",
        note: "The envelope binds the prepared input digest to its governed snapshot claim; no browser cache or client path is read.",
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
        command: "ds server activity --lane stable --output json",
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
        note: "Cancels queued/running compute, or the pending Sync Center publication of a completed Solar job.",
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
// ── the layer drawer, driven on the running Server ─────────────────────────

const LAYER_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "server_owner_changed",
        when: "the captured account differs from the Server owner or its original credential was revoked or replaced",
        remedy: "sign in under the intended Server account and explicitly restart ds server serve",
    },
    Refusal {
        code: "server_refused",
        when: "the protected server is not running, refuses the connection lane, or answers outside its contract",
        remedy: "verify ds auth status, the protected state directory and that ds server serve is running",
    },
    Refusal {
        code: "headless_signed_out",
        when: "the Server's native account signed out or was revoked",
        remedy: "sign in again under the Server's Linux user and restart ds server serve",
    },
    Refusal {
        code: "headless_project_not_selected",
        when: "the Server's account has no selected project",
        remedy: "run ds auth project use --project <exact-id> under the Server's user",
    },
    Refusal {
        code: "unknown_layer",
        when: "an id is not a canonical layer of the Server's selected project (runtime ids are never accepted)",
        remedy: "copy ids from ds server layers list --output json",
    },
    Refusal {
        code: "duplicate_layer",
        when: "a canonical layer is listed more than once",
        remedy: "pass each canonical id once",
    },
    Refusal {
        code: "invalid_order",
        when: "an order is not a bounded integer or the request lists no layers",
        remedy: "use config-id=integer within -1000000..1000000",
    },
    Refusal {
        code: "invalid_number",
        when: "--limit or --zoom is outside its bound",
        remedy: "pass limit 1..500 and zoom 0..24",
    },
    Refusal {
        code: "invalid_input",
        when: "the request body or query the Server received is not the documented shape",
        remedy: "send the documented request body; update ds if the CLI produced it",
    },
    Refusal {
        code: "local_layer_refused",
        when: "the Server's native layer store cannot be read or persisted",
        remedy: "check the Server user's local data directory; DS_LAYER_HOME may name an absolute shared directory",
    },
    Refusal {
        code: "layer_state_refused",
        when: "the shared layer kernel refused the question this build asked",
        remedy: "update ds and report the layer-state contract failure",
    },
    Refusal {
        code: "project_context_changed",
        when: "the selected project or account changed while the Server read the layer document",
        remedy: "repeat the command against the Server's current selected project",
    },
    Refusal {
        code: "auth_identity_mismatch",
        when: "the Server's native account changed during the request",
        remedy: "sign in again and repeat the command",
    },
    Refusal {
        code: "confirmation_required",
        when: "--yes was not supplied to a governed write",
        remedy: "review ds server layers list, then repeat with --yes",
    },
];
const LAYER_ARG: Arg = Arg {
    name: "layer",
    kind: ds_cli_contract::spec::ArgKind::Repeated,
    value: "<config-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "Canonical layer id from `ds server layers list`. Repeat for several.",
};
const ORDER_ARG: Arg = Arg {
    name: "order",
    kind: ds_cli_contract::spec::ArgKind::Repeated,
    value: "<config-id=integer>",
    required: true,
    default: None,
    choices: &[],
    summary: "Canonical id and desired order. Repeat for each override.",
};
const fn layer_command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    effect: Effect,
    args: &'static [Arg],
    output: &'static str,
    examples: &'static [Example],
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose: "Drive the layer drawer's catalogue, visibility and order on the RUNNING Server, with no Tauri process, browser, paired map or renderer. The Server answers from the same shared Rust owner as `ds map layer …` (ds-layer-ops over ds-command-kernel::layer_state): the assembled document is read under the Server's native identity and selected project, remembered visibility is fenced by lane, account and project in the Server's native layer store, order overrides are admitted by the kernel before the governed write. Requests travel over the protected owner-only loopback connection; remote operators use SSH. Nothing pretends a renderer mounted anything: `writes` name the layout word a renderer would apply.",
        chapter: Chapter::Design,
        effect,
        authority: Authority::HeadlessUser,
        execution: Execution::Sync,
        args,
        output,
        examples,
        refusals: LAYER_REFUSALS,
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
pub static LAYERS_LIST: Command = layer_command(
    "server.layers.list",
    &["server", "layers", "list"],
    "List the Server's canonical project layers with remembered visibility.",
    Effect::ReadOnly,
    &[
        STATE,
        LANE,
        Arg::switch(
            "refresh",
            "Rebuild canonical metadata and styles at the API boundary.",
        ),
        Arg::value(
            "limit",
            "<n>",
            "Report at most this many canonical layers; 1..500.",
        )
        .default("100"),
        Arg::value(
            "zoom",
            "<level>",
            "Also report whether each family renders at this zoom; 0..24.",
        ),
    ],
    "The same rows as `ds map layer list`: id, label, class, geometry, order, runtime_ids, style_ref, roles, visibility (count, any_visible, all_visible, next), source_state, in_zoom_range; lane, project, refreshed, visibility_source.",
    &[Example {
        command: "ds server layers list --lane canary --zoom 12 --output json",
        note: "Read the running Server's catalogue and this account's remembered toggles.",
        runnable: false,
    }],
);
pub static LAYERS_SHOW: Command = layer_command(
    "server.layers.show",
    &["server", "layers", "show"],
    "Remember canonical layers visible on the Server for its account.",
    Effect::LocalFileWrite,
    &[STATE, LANE, LAYER_ARG],
    "Lane, project, the remembered rows with folded visibility, which runtime layers changed, the writes a renderer would apply, `persisted: native_local` and the store revision.",
    &[Example {
        command: "ds server layers show --layer survey/poles --lane canary --output json",
        note: "Idempotent; the label companion follows its family.",
        runnable: false,
    }],
);
pub static LAYERS_HIDE: Command = layer_command(
    "server.layers.hide",
    &["server", "layers", "hide"],
    "Remember canonical layers hidden on the Server for its account.",
    Effect::LocalFileWrite,
    &[STATE, LANE, LAYER_ARG],
    "Lane, project, the remembered rows with folded visibility, which runtime layers changed, the writes a renderer would apply, `persisted: native_local` and the store revision.",
    &[Example {
        command: "ds server layers hide --layer survey/poles --lane canary --output json",
        note: "Survives a Server restart; another account on the same host never sees it.",
        runnable: false,
    }],
);
pub static LAYERS_REORDER: Command = layer_command(
    "server.layers.reorder",
    &["server", "layers", "reorder"],
    "Save canonical order overrides through the Server (needs --yes).",
    Effect::GlobalWrite,
    &[STATE, LANE, ORDER_ARG],
    "Lane, project, the admitted id/order pairs, applied/persisted flags, canonical_count, whether the order is complete and the canonical ids it leaves unlisted.",
    &[Example {
        command: "ds server layers reorder --order survey/poles=100 --yes --lane canary --output json",
        note: "Unknown, repeated and out-of-bound ids are refused before anything is sent.",
        runnable: false,
    }],
);

/// One typed refusal back from the Server, re-raised under the CLI's own
/// class and code so `ds server layers …` fails exactly like `ds map layer …`.
fn typed_refusal(status: u16, body: &[u8]) -> Failure {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return failure(String::from_utf8_lossy(body));
    };
    let message = value["error"]
        .as_str()
        .unwrap_or("the server refused the request")
        .to_owned();
    let class = match value["class"].as_str() {
        Some("invalid_input") => ExitClass::InvalidInput,
        Some("unauthorized") => ExitClass::Unauthorized,
        Some("unavailable") => ExitClass::Unavailable,
        Some("conflict") => ExitClass::Conflict,
        Some("internal") => ExitClass::Internal,
        _ if status == 401 => ExitClass::Unauthorized,
        _ => ExitClass::Failed,
    };
    let remedy = value["remedy"].as_str().map(str::to_owned);
    let refusal = match value["code"].as_str() {
        Some("server_owner_changed") => Failure::new(class, "server_owner_changed", message),
        Some("unknown_layer") => Failure::new(class, "unknown_layer", message),
        Some("duplicate_layer") => Failure::new(class, "duplicate_layer", message),
        Some("invalid_order") => Failure::new(class, "invalid_order", message),
        Some("invalid_number") => Failure::new(class, "invalid_number", message),
        Some("local_layer_refused") => Failure::new(class, "local_layer_refused", message),
        Some("layer_state_refused") => Failure::new(class, "layer_state_refused", message),
        Some("project_context_changed") => Failure::new(class, "project_context_changed", message),
        Some("auth_identity_mismatch") => Failure::new(class, "auth_identity_mismatch", message),
        Some("headless_signed_out") => Failure::new(class, "headless_signed_out", message),
        Some("headless_project_not_selected") => {
            Failure::new(class, "headless_project_not_selected", message)
        }
        _ => Failure::new(class, "server_refused", message),
    };
    match remedy {
        Some(remedy) => refusal.remedy(remedy),
        None => refusal,
    }
}

fn layers_answer(bytes: Vec<u8>) -> Result<Value, Failure> {
    serde_json::from_slice(&bytes).map_err(|_| failure("the server answered outside its contract"))
}
pub fn layers_list(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut path = format!(
        "/v1/layers?refresh={}&limit={}",
        inputs.switch("refresh"),
        inputs.require("limit")?
    );
    if let Some(zoom) = inputs.value("zoom") {
        path.push_str(&format!("&zoom={zoom}"));
    }
    layers_answer(request(inputs, "GET", &path, None, 32 * 1024 * 1024)?)
}
fn layers_visibility(inputs: &Inputs, visible: bool) -> Result<Value, Failure> {
    let body = serde_json::to_vec(&json!({"layers": inputs.repeated("layer"), "visible": visible}))
        .expect("closed request");
    layers_answer(request(
        inputs,
        "POST",
        "/v1/layers/visibility",
        Some(&body),
        32 * 1024 * 1024,
    )?)
}
pub fn layers_show(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    layers_visibility(inputs, true)
}
pub fn layers_hide(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    layers_visibility(inputs, false)
}
pub fn layers_reorder(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut orders = Vec::new();
    for value in inputs.repeated("order") {
        let Some((id, order)) = value.rsplit_once('=') else {
            return Err(
                Failure::invalid("invalid_order", "--order must be config-id=integer")
                    .remedy("copy the id from `ds server layers list --output json`"),
            );
        };
        let order: i64 = order.trim().parse().map_err(|_| {
            Failure::invalid(
                "invalid_order",
                format!("{value} does not end in an integer"),
            )
            .remedy("use config-id=integer")
        })?;
        orders.push(json!({"layer_id": id.trim(), "order": order}));
    }
    let body = serde_json::to_vec(&json!({"orders": orders})).expect("closed request");
    layers_answer(request(
        inputs,
        "POST",
        "/v1/layers/order",
        Some(&body),
        1024 * 1024,
    )?)
}
pub fn render_layers_list(value: &Value) -> String {
    ds_layer_ops::render_list(value)
}
pub fn render_layers_visibility(value: &Value) -> String {
    ds_layer_ops::render_visibility(value)
}
pub fn render_layers_reorder(value: &Value) -> String {
    ds_layer_ops::render_reorder(value)
}

pub static DOMAIN: Domain = Domain {
    id: "server",
    summary: "Native server.",
    commands: &[
        &ENGINE,
        &SERVE,
        &SUBMIT,
        &SOLAR_SUBMIT,
        &STATUS,
        &ACTIVITY,
        &CANCEL,
        &RESULT,
        &LAYERS_LIST,
        &LAYERS_SHOW,
        &LAYERS_HIDE,
        &LAYERS_REORDER,
    ],
};
fn failure(e: impl ToString) -> Failure {
    Failure::failed("server_refused", e.to_string())
        .remedy("verify native authentication, protected server state and ds server serve")
}
fn state(inputs: &Inputs) -> Result<PathBuf, Failure> {
    ds_compute_runtime::server_state_directory(
        inputs.require("lane")?,
        inputs.value("state-dir").map(Path::new),
    )
    .map_err(failure)
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
    let layer_auth: Arc<dyn ds_compute_runtime::Authorizer> =
        Arc::new(auth::NativeAuthorizer::new(lane.clone()).map_err(failure)?);
    let layer_host =
        layers::NativeLayerHost::bound(&lane, connection.owner.clone(), layer_auth.clone());
    let app = host::App {
        database: directory.join("store.sqlite"),
        connection,
        layers: layer_host,
        auth: layer_auth,
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
        return Err(typed_refusal(status, &bytes));
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
