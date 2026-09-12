//! Thin command/HTTP host for the shared native compute runtime.
mod auth;
// The HTTP host and the layer binding are reachable from an integration test
// so the isolation proof can stand up a real loopback listener over a fixture
// identity and a fixture document source. Nothing here is a supported API:
// `ds` itself reaches this crate only through its command handlers.
#[doc(hidden)]
pub mod host;
#[doc(hidden)]
pub mod layers;
mod server_reports;
pub mod server_sync;
#[doc(hidden)]
pub mod solar_sync;
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
/// The project a call is about. Optional here and never optional on the wire:
/// absent, the saved selection is read locally and sent anyway, so the Server
/// verifies exactly one named project on every request.
const PROJECT: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Exact ds_project id this call is about; defaults to the saved selection and is always sent.",
);

/// The bound the kernel's execution context puts on a project id
/// (`ds_command_kernel::execution_context::MAX_PROJECT_CHARS`). Checked here so
/// an unusable value is refused before it becomes a query string.
const MAX_PROJECT_CHARS: usize = 500;

// -- The refusal vocabulary a Server call can raise ----------------------
//
// Split by what a command can actually do: reading a job cannot refuse a
// changed payload under an idempotency key, and a submission cannot refuse an
// output path that already exists. Every code below is re-raised literally
// from the Server's own answer (`typed_refusal`), so a caller plans for the
// same names whichever host executed the operation.

const PLATFORM: Refusal = Refusal {
    code: "server_platform_unsupported",
    when: "the native server is requested outside Linux",
    remedy: "run these commands on a Linux machine",
};
const REFUSED: Refusal = Refusal {
    code: "server_refused",
    when: "native authentication, protected state, job identity, capacity or the request is invalid or unavailable",
    remedy: "read the stated reason; verify ds auth status, the state directory and that ds server serve is running",
};
const OWNER_CHANGED: Refusal = Refusal {
    code: "server_owner_changed",
    when: "the Server's account differs from the caller's, or its credential was revoked or replaced",
    remedy: "sign in under the intended Server account and explicitly restart ds server serve",
};
const PROJECT_REQUIRED: Refusal = Refusal {
    code: "project_required",
    when: "no --project was passed and this account has no saved selection",
    remedy: "pass --project <exact-id> or run ds auth project use --project <exact-id>",
};
const CONTEXT_CORRUPT: Refusal = Refusal {
    code: "context_corrupt",
    when: "a project id is empty, padded, over 500 characters or holds a control character",
    remedy: "copy one exact ds_project value from ds auth project list",
};
const NOT_VISIBLE: Refusal = Refusal {
    code: "not_visible",
    when: "the job id is unknown, or belongs to another principal, lane, deployment or project",
    remedy: "read ds server status --project <exact-id> for the jobs you can see",
};
const PRINCIPAL_MISMATCH: Refusal = Refusal {
    code: "principal_mismatch",
    when: "the stored job belongs to another account, lane or deployment",
    remedy: "run it under the account, lane and deployment that submitted the job",
};
const SCOPE_MISMATCH: Refusal = Refusal {
    code: "scope_mismatch",
    when: "the sealed input names one project and --project names another",
    remedy: "submit the sealed input under the project it names, or prepare it again elsewhere",
};
const SCOPE_MISMATCH_FOR_KEY: Refusal = Refusal {
    code: "scope_mismatch_for_key",
    when: "that idempotency key already admitted a job in a different project",
    remedy: "use a key that is unique per project; a key never moves a job",
};
const PAYLOAD_CHANGED_FOR_KEY: Refusal = Refusal {
    code: "payload_changed_for_key",
    when: "that idempotency key already admitted a job whose input bytes differ from these",
    remedy: "resubmit the identical bytes, or choose a new key for the changed input",
};
const CAPACITY_EXHAUSTED: Refusal = Refusal {
    code: "capacity_exhausted",
    when: "the global or per-project queue is full; the answer names the scope and retry_after_ms",
    remedy: "retry after retry_after_ms, cancel work you no longer need, or raise --workers/--per-project",
};
const CONTEXT_UNRECOVERABLE: Refusal = Refusal {
    code: "context_unrecoverable",
    when: "a job stored by an older Server names no project and none can be recovered",
    remedy: "read that job's result and resubmit under an explicit --project",
};
const OUTPUT_EXISTS: Refusal = Refusal {
    code: "server_output_exists",
    when: "the result output path already exists",
    remedy: "choose an absent output file; existing files are never overwritten",
};

const INSTALL_UNAVAILABLE: Refusal = Refusal {
    code: "headless_install_unavailable",
    when: "the registered install this host runs under cannot be read or created in the protected DS state root",
    remedy: "make the Server user's protected DS state root present, owner-only and writable, then start the host again",
};

/// Hosting the process itself. No project is resolved, so none can refuse —
/// but the host does need the registered install its sessions are fenced by.
// -- what the HOST answers, for a request its own commands do not make -----
//
// Both of these are `host.rs`'s answers to a path rather than to an argument:
// one for an operation that genuinely needs a rendered map, one for a route
// this build does not serve at all — which is what a caller of a different
// version reaches. They are declared here, on the command that RUNS the host,
// because that is the only `ds server` command they belong to; the map-bound
// half belongs in `ds map …`'s own descriptor once `--target server` is wired
// to this transport.
const NEEDS_MAP: Refusal = Refusal {
    code: "needs_paired_map",
    when: "an operation that needs a rendered map reaches this host, which has none",
    remedy: "run the same command with --target desktop, against a window open on this project",
};
const UNSUPPORTED: Refusal = Refusal {
    code: "unsupported_operation",
    when: "a request names a route this host does not serve, usually a version skew",
    remedy: "update ds, or read ds server --help for what this host serves",
};
const SERVE_REFUSALS: &[Refusal] = &[
    PLATFORM,
    REFUSED,
    OWNER_CHANGED,
    NEEDS_MAP,
    UNSUPPORTED,
    INSTALL_UNAVAILABLE,
];
/// Queueing work under an idempotency key.
const SUBMIT_REFUSALS: &[Refusal] = &[
    PLATFORM,
    REFUSED,
    OWNER_CHANGED,
    PROJECT_REQUIRED,
    CONTEXT_CORRUPT,
    SCOPE_MISMATCH,
    SCOPE_MISMATCH_FOR_KEY,
    PAYLOAD_CHANGED_FOR_KEY,
    PRINCIPAL_MISMATCH,
    CAPACITY_EXHAUSTED,
];
/// Reading or cancelling a job that already exists.
const JOB_REFUSALS: &[Refusal] = &[
    PLATFORM,
    REFUSED,
    OWNER_CHANGED,
    PROJECT_REQUIRED,
    CONTEXT_CORRUPT,
    NOT_VISIBLE,
    PRINCIPAL_MISMATCH,
    CONTEXT_UNRECOVERABLE,
];
/// Naming a job and a destination file.
const RESULT_REFUSALS: &[Refusal] = &[
    PLATFORM,
    REFUSED,
    OWNER_CHANGED,
    PROJECT_REQUIRED,
    CONTEXT_CORRUPT,
    NOT_VISIBLE,
    PRINCIPAL_MISMATCH,
    CONTEXT_UNRECOVERABLE,
    OUTPUT_EXISTS,
];

// Eight, because a command descriptor has eight parts and naming them at each
// call site is what makes the list below readable. The same allow the two
// test helpers in this repo already carry.
#[allow(clippy::too_many_arguments)]
const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    effect: Effect,
    execution: Execution,
    args: &'static [Arg],
    refusals: &'static [Refusal],
    examples: &'static [Example],
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose: "Drive native transformer and prepared Solar computation on the shared Rust runtime. Jobs survive UI closure and server restart; request and result bytes are retained under the initiating identity. Every call is about one project: --project names it, else the saved selection (ds auth project use) is sent as this client's DEFAULT, recorded by the Server and never held as its state -- one Server serves several of its owner's projects at once. The Server runs what its owner hands it for the project named; the gateway enforces entitlement at publication and sync. A Solar request carries its sealed input, whose project is authoritative.",
        chapter: Chapter::Design,
        effect,
        authority: Authority::HeadlessUser,
        execution,
        args,
        output: "A bounded job receipt or list, each naming its project; full bytes only via server result. No credential is printed.",
        examples,
        refusals,
        reference: Some("docs/reference/server.md"),
        availability: || {
            if cfg!(target_os = "linux") {
                Availability::Available
            } else {
                Availability::unavailable(
                    "server_platform_unsupported",
                    "the native server currently requires Linux",
                    "run these commands on a Linux machine",
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

/// Hosting has its own purpose because it has its own subject. The other
/// commands are about a project; this one is about the host, and the whole
/// point of the slice is that starting a host settles no project at all.
pub static SERVE: Command = Command {
    id: "server.serve",
    path: &["server", "serve"],
    contract: 1,
    summary: "Host durable parallel compute for every project this account reaches.",
    purpose: "Host the shared Rust compute runtime under this Linux user's native account: the desktop's own core, without the desktop. No project is selected or captured here and no directory is fetched: callers name the project per request, the Server records it and runs what its owner hands it, and the gateway enforces entitlement at publication and sync -- so one host serves several projects at once, needs no ds auth project use to start, and runs with no upstream. One owner per Server; many users are many machines. --workers bounds the whole host; --per-project bounds what one project may hold while another has work queued. Control is an owner-only loopback credential. This process runs in the foreground until it stops.",
    chapter: Chapter::Design,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        STATE,
        LANE,
        Arg::value("listen", "<loopback:port>", "Fixed loopback bind address.")
            .default("127.0.0.1:19766"),
        Arg::value(
            "workers",
            "<count>",
            "Parallel jobs, bounded by measured CPU and memory; defaults to capacity.",
        ),
        Arg::value(
            "per-project",
            "<count>",
            "Jobs one project may run at once while another queues; 1..workers, default half.",
        ),
    ],
    output: "The worker and per-project limits the host ran under, once it stops. No credential is printed.",
    examples: &[Example {
        command: "ds server serve --lane stable",
        note: "Run after native login under the same Linux user. No project need be selected: callers name theirs.",
        runnable: false,
    }],
    refusals: SERVE_REFUSALS,
    reference: Some("docs/reference/server.md"),
    availability: || {
        if cfg!(target_os = "linux") {
            Availability::Available
        } else {
            Availability::unavailable(
                "server_platform_unsupported",
                "the native server currently requires Linux",
                "run these commands on a Linux machine",
            )
        }
    },
};
pub static SUBMIT: Command = command(
    "server.submit",
    &["server", "submit"],
    "Queue a complete native transformer batch and return immediately.",
    Effect::LocalFileWrite,
    Execution::Job,
    &[
        STATE,
        LANE,
        PROJECT,
        Arg::value(
            "input",
            "<path>",
            "Explicit ds.fast-lv.request/v1 file; at most 64 MiB.",
        )
        .required(),
        Arg::value(
            "key",
            "<idempotency-key>",
            "Stable caller key: the same bytes reuse the job; changed bytes or another project refuse.",
        )
        .required(),
    ],
    SUBMIT_REFUSALS,
    &[Example {
        command: "ds server submit --input transformer-batch.json --key processing-001 --project <exact-id>",
        note: "Queue an explicit native transformer request in one named project on the running server.",
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
        PROJECT,
        Arg::value(
            "input",
            "<path>",
            "Private ds.solar.server-submission/v1 envelope on this machine; the Server reads this path itself. At most 64 MiB.",
        )
        .required(),
        Arg::value(
            "key",
            "<idempotency-key>",
            "Stable caller key: the same bytes reuse the job; changed bytes or another project refuse.",
        )
        .required(),
    ],
    SUBMIT_REFUSALS,
    &[Example {
        command: "ds server solar submit --input pala.server-submission.json --key solar-001",
        note: "The sealed project is authoritative; a different --project refuses scope_mismatch.",
        runnable: false,
    }],
);
pub static STATUS: Command = command(
    "server.status",
    &["server", "status"],
    "Inspect one project's durable jobs without a browser or desktop.",
    Effect::ReadOnly,
    Execution::Sync,
    &[
        STATE,
        LANE,
        PROJECT,
        Arg::value(
            "job",
            "<id>",
            "One exact job; otherwise this project's latest 100 jobs and more flag.",
        ),
    ],
    JOB_REFUSALS,
    &[Example {
        command: "ds server status --project <exact-id> --output json",
        note: "Read bounded job receipts for one project from the running server.",
        runnable: false,
    }],
);
pub static ACTIVITY: Command = command(
    "server.activity",
    &["server", "activity"],
    "Read one project's shared Sync Center activity and publication state.",
    Effect::ReadOnly,
    Execution::Sync,
    &[STATE, LANE, PROJECT],
    JOB_REFUSALS,
    &[Example {
        command: "ds server activity --project <exact-id> --output json",
        note: "Read running jobs and held publication state for one project.",
        runnable: false,
    }],
);
pub static CANCEL: Command = command(
    "server.cancel",
    &["server", "cancel"],
    "Cancel a queued or running job while retaining its inputs.",
    Effect::LocalFileWrite,
    Execution::Sync,
    &[STATE, LANE, PROJECT, JOB],
    JOB_REFUSALS,
    &[Example {
        command: "ds server cancel --job <job-id> --project <exact-id>",
        note: "Cancels queued/running compute, or a completed Solar job's pending publication, and releases its capacity.",
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
        PROJECT,
        JOB,
        Arg::value("out", "<path>", "Absent destination file.").required(),
    ],
    RESULT_REFUSALS,
    &[Example {
        command: "ds server result --job <job-id> --out result.json",
        note: "Export to an absent path after completion.",
        runnable: false,
    }],
);

// -- the layer drawer on a Server: a transport, no longer a command set --
//
// `ds server layers list|show|hide|reorder` are RETIRED. The standing ruling
// is one command id per operation whichever host executes it, so the drawer's
// catalogue, visibility and order are `ds map layer list|show|hide|reorder`
// with an explicit `--target server|desktop[:instance]`. A second set of ids
// that differed only by which host answered is exactly what that ruling ends.
//
// What stays here is the part that genuinely is the Server's: the protected
// loopback transport. This crate owns `connection.json`, its bearer and the
// lane fence, so the `--target server` half of those four commands calls the
// functions below. They send an explicit project on every request -- the
// Server reads no selection of this machine's -- and re-raise the Server's
// typed refusals unchanged, which is why one `ds map layer …` invocation
// fails with the same code and remedy against either host.

/// The arguments a command must declare for this transport to reach a Server:
/// which protected state directory, which lane, and which project the request
/// is about.
pub const STATE_DIR_ARG: Arg = STATE;
pub const LANE_ARG: Arg = LANE;
pub const PROJECT_ARG: Arg = PROJECT;
pub static SERVER_TARGET_ARGS: &[Arg] = &[STATE, LANE, PROJECT];

/// One typed refusal back from the Server, re-raised under the CLI's own class
/// and code so an operation executed on the Server fails exactly like the same
/// operation executed on the desktop.
fn typed_refusal(status: u16, body: &[u8]) -> Failure {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return failure(String::from_utf8_lossy(body));
    };
    let message = value["error"]
        .as_str()
        .unwrap_or("the server refused the request")
        .to_owned();
    let code = value["code"].as_str();
    let class = match value["class"].as_str() {
        Some("invalid_input") => ExitClass::InvalidInput,
        Some("unauthorized") => ExitClass::Unauthorized,
        Some("unavailable") => ExitClass::Unavailable,
        Some("conflict") => ExitClass::Conflict,
        Some("internal") => ExitClass::Internal,
        _ if status == 401 => ExitClass::Unauthorized,
        _ => default_class(code, status),
    };
    let refusal = match code {
        // The execution context's own vocabulary, kept literal: a caller that
        // learned these names on the desktop plans for them here unchanged.
        Some("project_required") => Failure::new(class, "project_required", message),
        Some("context_corrupt") => Failure::new(class, "context_corrupt", message),
        Some("not_visible") => Failure::new(class, "not_visible", message),
        Some("principal_mismatch") => Failure::new(class, "principal_mismatch", message),
        Some("scope_mismatch") => Failure::new(class, "scope_mismatch", message),
        Some("scope_mismatch_for_key") => Failure::new(class, "scope_mismatch_for_key", message),
        Some("payload_changed_for_key") => Failure::new(class, "payload_changed_for_key", message),
        Some("capacity_exhausted") => Failure::new(class, "capacity_exhausted", message),
        Some("context_unrecoverable") => Failure::new(class, "context_unrecoverable", message),
        // The Server connection's own fence.
        Some("server_owner_changed") => Failure::new(class, "server_owner_changed", message),
        // The shared layer owner's, re-raised for `ds map layer … --target server`.
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
    let refusal = match value["remedy"].as_str() {
        Some(remedy) => refusal.remedy(remedy),
        None => match default_remedy(code, &value) {
            Some(remedy) => refusal.remedy(remedy),
            None => refusal,
        },
    };
    // Capacity is the one refusal a caller can act on programmatically, so its
    // numbers travel as structured detail and not only inside a sentence.
    match (code, value.get("retry_after_ms")) {
        (Some("capacity_exhausted"), Some(retry)) => refusal.detail(json!({
            "retry_after_ms": retry.clone(),
            "scope": value.get("scope").cloned().unwrap_or(Value::Null),
        })),
        _ => refusal,
    }
}

/// The class a code carries when the Server named the code but not the class.
/// The mapping is the route contract's own (`docs-routes.md` §1). `not_visible`
/// deliberately shares its class with a wholly unknown id: the class is part of
/// the answer, and a differing class would disclose that the job exists.
fn default_class(code: Option<&str>, status: u16) -> ExitClass {
    match code {
        Some("project_required" | "context_corrupt") => ExitClass::InvalidInput,
        Some("server_owner_changed") => ExitClass::Unauthorized,
        Some(
            "scope_mismatch"
            | "scope_mismatch_for_key"
            | "payload_changed_for_key"
            | "principal_mismatch"
            | "not_visible"
            | "context_unrecoverable",
        ) => ExitClass::Conflict,
        Some("capacity_exhausted") => ExitClass::Unavailable,
        _ if status == 429 => ExitClass::Unavailable,
        _ => ExitClass::Failed,
    }
}

/// The remedy a caller gets when the Server named a code but no remedy: the
/// same sentence the command's own REFUSALS section declares, so help and
/// runtime cannot disagree.
fn default_remedy(code: Option<&str>, body: &Value) -> Option<String> {
    let code = code?;
    if code == CAPACITY_EXHAUSTED.code {
        let scope = body["scope"].as_str().unwrap_or("global");
        return Some(match body["retry_after_ms"].as_u64() {
            Some(retry) => format!(
                "the {scope} queue is full; retry after {retry} ms, cancel work you no longer need, or restart the host with a larger --workers/--per-project"
            ),
            None => CAPACITY_EXHAUSTED.remedy.to_owned(),
        });
    }
    [
        SUBMIT_REFUSALS,
        RESULT_REFUSALS,
        JOB_REFUSALS,
        SERVE_REFUSALS,
    ]
    .iter()
    .flat_map(|list| list.iter())
    .find(|refusal| refusal.code == code)
    .map(|refusal| refusal.remedy.to_owned())
}

fn layers_answer(bytes: Vec<u8>) -> Result<Value, Failure> {
    serde_json::from_slice(&bytes).map_err(|_| failure("the server answered outside its contract"))
}
/// `map.layer.list` executed on a Server. The project is explicit: the host
/// resolves no selection of its own.
pub fn layers_list(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut path = format!(
        "/v1/layers?refresh={}&limit={}",
        inputs.switch("refresh"),
        inputs.value("limit").unwrap_or("100")
    );
    if let Some(zoom) = inputs.value("zoom") {
        path.push_str(&format!("&zoom={zoom}"));
    }
    let path = with_project(&path, &project(inputs)?);
    layers_answer(request(inputs, "GET", &path, None, 32 * 1024 * 1024)?)
}
fn layers_visibility(inputs: &Inputs, visible: bool) -> Result<Value, Failure> {
    // The body stays exactly the `ds_layer_ops` request type, so nothing about
    // `ds map layer …`'s shapes changes with the host; the project rides in the
    // query, where every Server route reads it.
    let path = with_project("/v1/layers/visibility", &project(inputs)?);
    let body = serde_json::to_vec(&json!({"layers": inputs.repeated("layer"), "visible": visible}))
        .expect("closed request");
    layers_answer(request(
        inputs,
        "POST",
        &path,
        Some(&body),
        32 * 1024 * 1024,
    )?)
}
/// `map.layer.show` executed on a Server.
pub fn layers_show(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    layers_visibility(inputs, true)
}
/// `map.layer.hide` executed on a Server.
pub fn layers_hide(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    layers_visibility(inputs, false)
}
/// `map.layer.reorder` executed on a Server.
pub fn layers_reorder(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut orders = Vec::new();
    for value in inputs.repeated("order") {
        let Some((id, order)) = value.rsplit_once('=') else {
            return Err(
                Failure::invalid("invalid_order", "--order must be config-id=integer")
                    .remedy("copy the id from `ds map layer list --output json`"),
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
    let path = with_project("/v1/layers/order", &project(inputs)?);
    let body = serde_json::to_vec(&json!({"orders": orders})).expect("closed request");
    layers_answer(request(inputs, "POST", &path, Some(&body), 1024 * 1024)?)
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

/// The project this call is about, resolved to an explicit name before
/// anything is sent.
///
/// `--project` wins. Without it the saved selection is read from this
/// machine's own protected context -- the same probe `ds auth project use`
/// writes and `ds auth project status` reads -- and sent as if it had been
/// typed. That is the whole role of a saved selection here: a client-side
/// default. The Server records whichever name arrives as the job's project
/// and executes what its owner handed it; whether that project's effects may
/// leave the machine is the gateway's answer at publication and sync, never
/// a directory the Server fetched. With neither there is nothing to record
/// and nothing to guess, so the call refuses here rather than admitting an
/// unscoped job.
fn project(inputs: &Inputs) -> Result<String, Failure> {
    known_project(inputs)?.ok_or_else(|| {
        Failure::invalid(
            "project_required",
            "no project was named and this account has no saved selection to default to",
        )
        .remedy(PROJECT_REQUIRED.remedy)
        .next("ds auth project list")
    })
}

/// The project if one can be named at all, without deciding whether the
/// operation needs one. A sealed Solar envelope names its own project and that
/// name is authoritative, so that one submission can proceed with nothing to
/// send while every other call refuses through [`project`].
fn known_project(inputs: &Inputs) -> Result<Option<String>, Failure> {
    if let Some(named) = inputs.value("project") {
        return bounded_project(named).map(Some);
    }
    match ds_cli_auth::probe_headless_identity(inputs.require("lane")?)?
        .and_then(|(_, selected)| selected)
    {
        Some(selected) => bounded_project(&selected).map(Some),
        None => Ok(None),
    }
}

/// The kernel's bound on a project id, checked before it becomes a query
/// value. The same rule, stated here so an unusable name never reaches a wire.
fn bounded_project(value: &str) -> Result<String, Failure> {
    let usable = !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= MAX_PROJECT_CHARS
        && !value.chars().any(char::is_control);
    if !usable {
        return Err(Failure::invalid(
            "context_corrupt",
            format!(
                "a project id is 1..{MAX_PROJECT_CHARS} characters, unpadded and free of control characters"
            ),
        )
        .remedy(CONTEXT_CORRUPT.remedy)
        .next("ds auth project list"));
    }
    Ok(value.to_owned())
}

/// Percent-encode one query value. Written out rather than pulled in: the only
/// values this crate puts in a query are a project id and bounded numbers, and
/// a URL dependency for that would be a larger surface than the rule.
fn query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}
fn with_project(path: &str, project: &str) -> String {
    let separator = if path.contains('?') { '&' } else { '?' };
    format!("{path}{separator}project={}", query_value(project))
}
fn with_known_project(path: &str, project: Option<&str>) -> String {
    match project {
        Some(project) => with_project(path, project),
        None => path.to_owned(),
    }
}

/// The default share of a host's workers one project may hold: half, never
/// fewer than one, so a single-worker host still admits work and a busy
/// project cannot starve a second one on a host with room for two.
///
/// A share equal to the worker count is not a share at all -- one project
/// could hold every worker while another waits -- so [`per_project`] refuses
/// it on any host with more than one worker.
pub const fn default_per_project(workers: usize) -> usize {
    let half = workers / 2;
    if half == 0 { 1 } else { half }
}

/// How deep a queue one project, and the whole host, may hold. Waiting is not
/// free -- a queue nobody bounds is a refusal deferred until memory runs out --
/// so both are finite and the global bound is the kernel's own maximum.
const PER_PROJECT_QUEUED: usize = 512;
const GLOBAL_QUEUED: usize = 4_096;

fn per_project(inputs: &Inputs, workers: usize) -> Result<usize, Failure> {
    let Some(requested) = inputs.value("per-project") else {
        return Ok(default_per_project(workers));
    };
    let requested: usize = requested.parse().map_err(|_| {
        failure("--per-project must be a whole number of concurrently running jobs")
    })?;
    if requested == 0 || requested >= workers.max(2) {
        return Err(failure(format!(
            "--per-project must leave a worker for a second project: 1..{} on a host with {workers}",
            workers.saturating_sub(1).max(1)
        )));
    }
    Ok(requested)
}

pub fn serve(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?.to_owned();
    // The host's own numbers first. They are measured and parsed locally, so a
    // typo'd bound is answered without refreshing a credential to find out --
    // and the answer is the same on a machine that has never signed in.
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
    let per_project = per_project(inputs, workers)?;
    let limits = ds_command_kernel::execution_context::Limits {
        global_running: workers,
        per_project_running: per_project,
        per_project_queued: PER_PROJECT_QUEUED,
        global_queued: GLOBAL_QUEUED,
    };
    // Hosting needs an authenticated account and nothing else. No project is
    // selected, resolved or captured here: a caller names the project on the
    // request and the Server verifies it per call. `ServerSessions::native`
    // makes no network call and reads no saved selection, which is what lets
    // an account that never ran `ds auth project use` start a host at all.
    let owner = auth::identity(&lane).map_err(failure)?;
    let directory = state(inputs)?;
    let connection = host::connection(&directory, address, owner, lane.clone()).map_err(failure)?;
    let database = directory.join("store.sqlite");
    let sessions =
        server_sync::sessions::ServerSessions::native(connection.clone(), database.clone(), limits)
            .map_err(failure)?;
    let layer_auth: Arc<dyn ds_compute_runtime::Authorizer> =
        Arc::new(auth::NativeAuthorizer::new(lane.clone()).map_err(failure)?);
    let layer_host =
        layers::NativeLayerHost::bound(&lane, connection.owner.clone(), layer_auth.clone());
    let app = host::App {
        database,
        connection,
        layers: layer_host,
        auth: layer_auth,
        requests: Arc::new(tokio::sync::Semaphore::new(workers.min(8))),
        activity: None,
        sessions,
    };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(failure)?
        .block_on(host::serve(app, workers))
        .map_err(failure)?;
    Ok(json!({"stopped":true,"workers":workers,"per_project":per_project}))
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
fn submit_key(inputs: &Inputs) -> Result<&str, Failure> {
    let key = inputs.require("key")?;
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(failure("key must be 1..128 letters, digits, _ or -"));
    }
    Ok(key)
}

/// A transformer request carries no project of its own by design, so one must
/// be named or defaulted here or there is nothing for the Server to record.
///
/// Its bytes travel in the request, because the desktop's own transformer
/// processing takes bytes: the request is assembled from what the caller
/// holds, not read back out of a workspace file.
pub fn submit(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let key = submit_key(inputs)?.to_owned();
    // The project is resolved before the input is opened: an unscoped call
    // refuses without reading 64 MiB it would only have discarded.
    let project = project(inputs)?;
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
    json_request(
        inputs,
        "POST",
        &with_project(&format!("/v1/transformer-processing/{key}"), &project),
        Some(&bytes),
    )
}

/// A sealed Solar envelope names its own project, and that name wins. The
/// client still sends what it knows — it is how a caller learns it prepared the
/// wrong city (`scope_mismatch`) — but an account with no selection can submit
/// a sealed envelope without naming anything.
///
/// The envelope is a workspace file, and the Server reads the filesystem
/// exactly as the desktop does — same machine, same user — so what travels is
/// the PATH. The Server reads those bytes itself and digests what it read; the
/// client neither copies 64 MiB through a socket nor decides what the bytes
/// are.
pub fn solar_submit(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let key = submit_key(inputs)?.to_owned();
    let project = known_project(inputs)?;
    let path = std::fs::canonicalize(inputs.require("input")?).map_err(failure)?;
    if !path.is_file() {
        return Err(failure("input must be a regular file"));
    }
    let body = serde_json::to_vec(&json!({ "input_path": path })).expect("closed request");
    json_request(
        inputs,
        "POST",
        &with_known_project(&format!("/v1/solar-processing/{key}"), project.as_deref()),
        Some(&body),
    )
}
pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = if inputs.value("job").is_some() {
        format!("/v1/jobs/{}", id(inputs)?)
    } else {
        "/v1/jobs".into()
    };
    json_request(inputs, "GET", &with_project(&path, &project(inputs)?), None)
}
pub fn activity(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    serde_json::from_slice(&request(
        inputs,
        "GET",
        &with_project("/v1/activity", &project(inputs)?),
        None,
        16 * 1024 * 1024,
    )?)
    .map_err(failure)
}
pub fn cancel(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    json_request(
        inputs,
        "POST",
        &with_project(
            &format!("/v1/jobs/{}/cancel", id(inputs)?),
            &project(inputs)?,
        ),
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
        &with_project(
            &format!("/v1/jobs/{}/result", id(inputs)?),
            &project(inputs)?,
        ),
        None,
        256 * 1024 * 1024,
    )?;
    ds_design_workspace::write_new(&output, &bytes).map_err(failure)?;
    Ok(json!({"out":output,"byte_count":bytes.len(),"sha256":ds_compute_runtime::digest(&bytes)}))
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::parse;

    fn inputs(command: &Command, tokens: &[&str]) -> Inputs {
        parse(
            command,
            &tokens.iter().map(|t| (*t).to_owned()).collect::<Vec<_>>(),
        )
        .expect("declared tokens parse")
    }

    fn refused(body: Value) -> Failure {
        typed_refusal(409, serde_json::to_string(&body).unwrap().as_bytes())
    }

    #[test]
    fn a_named_project_wins_and_never_consults_local_state() {
        // A caller that names a project is telling the Server what to verify.
        // A machine with no login at all can still make that call, so this
        // path must not touch the saved selection.
        let named = inputs(&STATUS, &["--project", "project-b"]);
        assert_eq!(project(&named).unwrap(), "project-b");
    }

    #[test]
    fn an_unusable_project_never_reaches_the_wire() {
        // The same bound the kernel enforces, under the kernel's own code:
        // where a situation has a name, both sides of the wire use it.
        for value in ["", " padded", "padded ", "with\u{1}control"] {
            let error = bounded_project(value).expect_err(value);
            assert_eq!(error.code(), "context_corrupt");
            assert_eq!(error.class(), ExitClass::InvalidInput);
            assert!(error.remedy_text().is_some(), "{value} needs a remedy");
        }
        assert!(bounded_project(&"p".repeat(MAX_PROJECT_CHARS)).is_ok());
        assert!(bounded_project(&"p".repeat(MAX_PROJECT_CHARS + 1)).is_err());
    }

    #[test]
    fn the_project_is_always_a_query_value_the_server_can_read_back() {
        assert_eq!(
            with_project("/v1/jobs", "a b/c?d&e"),
            "/v1/jobs?project=a%20b%2Fc%3Fd%26e"
        );
        assert_eq!(
            with_project("/v1/layers?limit=100", "p-1"),
            "/v1/layers?limit=100&project=p-1"
        );
    }

    #[test]
    fn a_typed_refusal_keeps_the_servers_code_and_gains_its_declared_remedy() {
        let error = refused(json!({
            "class": "conflict",
            "code": "scope_mismatch_for_key",
            "error": "that key already admitted a job in another project",
        }));
        assert_eq!(error.code(), "scope_mismatch_for_key");
        assert_eq!(error.class(), ExitClass::Conflict);
        assert_eq!(error.remedy_text(), Some(SCOPE_MISMATCH_FOR_KEY.remedy));
    }

    #[test]
    fn an_invisible_job_and_an_unknown_id_are_one_answer() {
        // Non-disclosure is a property of the whole answer, not only its
        // sentence: code, class and message must be identical.
        let unknown = refused(json!({"code": "not_visible", "error": "job not found"}));
        let foreign = refused(json!({"code": "not_visible", "error": "job not found"}));
        assert_eq!(unknown.code(), foreign.code());
        assert_eq!(unknown.class(), foreign.class());
        assert_eq!(unknown.message(), foreign.message());
        assert_eq!(unknown.class(), ExitClass::Conflict);
        assert_eq!(unknown.message(), "job not found");
    }

    #[test]
    fn capacity_carries_its_retry_guidance_as_words_and_as_numbers() {
        let error = refused(json!({
            "code": "capacity_exhausted",
            "error": "the project queue is full",
            "retry_after_ms": 4_000,
            "scope": "project",
        }));
        assert_eq!(error.code(), "capacity_exhausted");
        assert_eq!(error.class(), ExitClass::Unavailable);
        let remedy = error.remedy_text().expect("retry guidance").to_owned();
        assert!(remedy.contains("4000 ms"), "{remedy}");
        assert!(remedy.contains("project queue"), "{remedy}");
        let detail = error.detail_value().expect("machine-readable retry");
        assert_eq!(detail["retry_after_ms"], 4_000);
        assert_eq!(detail["scope"], "project");
    }

    #[test]
    fn every_execution_context_code_is_declared_and_carries_a_remedy() {
        // The runtime mapping and the help text are one list, or they are two
        // contracts. The vocabulary is read from the kernel rather than copied
        // here, so a code the execution context gains cannot ship undeclared:
        // the Server relays that closed set verbatim, and `refusal_coverage`'s
        // source scan cannot see through the relay, so this is what makes its
        // exemption for `sessions.rs` a true statement rather than a hole.
        let declared: Vec<&str> = [
            SERVE_REFUSALS,
            SUBMIT_REFUSALS,
            JOB_REFUSALS,
            RESULT_REFUSALS,
        ]
        .iter()
        .flat_map(|list| list.iter())
        .map(|refusal| refusal.code)
        .collect();
        let kernel = ds_command_kernel::execution_context::REFUSALS;
        assert!(kernel.contains(&"project_required"), "vocabulary moved");
        for code in kernel
            .iter()
            .copied()
            // The host's own one, which the kernel does not decide: the
            // Server connection's fence on its owner.
            .chain(["server_owner_changed"])
        {
            assert!(declared.contains(&code), "`{code}` is declared nowhere");
            assert!(
                default_remedy(Some(code), &Value::Null).is_some(),
                "`{code}` has no remedy when the Server sends none"
            );
        }
    }

    #[test]
    fn every_command_that_names_a_job_or_a_project_declares_project() {
        for command in DOMAIN.commands {
            if command.id == "server.engine" || command.id == "server.serve" {
                continue;
            }
            assert!(
                command.arg("project").is_some(),
                "`{}` reads a job or a project and must declare --project",
                command.id
            );
        }
        assert!(
            SERVE.arg("project").is_none(),
            "hosting resolves no project: callers name theirs per request"
        );
        assert!(SERVE.arg("per-project").is_some());
    }

    #[test]
    fn a_sealed_solar_envelope_may_name_its_own_project_and_nothing_else_may() {
        // The one documented exception: the envelope carries the project, so a
        // machine with no saved selection can still submit one. Every other
        // call refuses rather than letting the Server pick.
        let none = inputs(&SOLAR_SUBMIT, &["--input", "/dev/null", "--key", "k"]);
        assert_eq!(
            with_known_project("/v1/solar-processing/k", None),
            "/v1/solar-processing/k",
            "an unnamed sealed submission sends no project at all"
        );
        assert_eq!(none.value("project"), None);
        let named = inputs(
            &SOLAR_SUBMIT,
            &["--input", "/dev/null", "--key", "k", "--project", "p-1"],
        );
        assert_eq!(known_project(&named).unwrap().as_deref(), Some("p-1"));
    }

    #[test]
    fn the_retired_layer_commands_are_gone_from_the_domain() {
        for command in DOMAIN.commands {
            assert!(
                !command.id.starts_with("server.layers"),
                "`{}` is retired: the operation is `ds map layer …` with --target",
                command.id
            );
        }
    }

    #[test]
    fn a_project_may_hold_half_the_workers_and_never_all_of_them() {
        assert_eq!(default_per_project(1), 1);
        assert_eq!(default_per_project(2), 1);
        assert_eq!(default_per_project(9), 4);
        assert_eq!(per_project(&inputs(&SERVE, &[]), 8).unwrap(), 4);
        assert_eq!(
            per_project(&inputs(&SERVE, &["--per-project", "7"]), 8).unwrap(),
            7
        );
        // A share equal to the worker count is no share: one project could
        // hold every worker while another waits, which is the thing the
        // fairness rule exists to prevent.
        for refused in ["8", "9", "0", "half"] {
            assert!(
                per_project(&inputs(&SERVE, &["--per-project", refused]), 8).is_err(),
                "--per-project {refused} must refuse"
            );
        }
        // A one-worker host has nothing to share, so its only share is one.
        assert_eq!(
            per_project(&inputs(&SERVE, &["--per-project", "1"]), 1).unwrap(),
            1
        );
    }
}
