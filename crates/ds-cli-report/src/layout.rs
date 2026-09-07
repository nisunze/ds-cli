//! Print authoring is pure kernel intent; persistence uses the closed native
//! client and delivery uses one fixed reporter process task.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use std::io::Read;
fn local() -> Availability {
    Availability::Available
}
const REFUSALS: &[Refusal] = &[Refusal {
    code: "printing_invalid",
    when: "The bounded print document or intent is invalid",
    remedy: "Use report.layout.schema and correct the reported layout constraint",
}];
const REQUEST: Arg = Arg::value("request", "<json-file>", "Bounded typed request file.").required();
const SCOPE: Arg = Arg::value(
    "scope",
    "<scope>",
    "Global published samples or selected project customizations.",
)
.choices(&["global", "project"])
.required();
const LANE: Arg = Arg::value("lane", "<lane>", "Native credential lane.")
    .choices(&["canary", "stable"])
    .default("canary");
fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("printing_invalid", e.to_string())
        .remedy("Use report.layout.schema and correct the reported layout constraint")
}
fn bytes(path: &str, max: usize) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(invalid)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > max {
        return Err(invalid("Print request exceeds size limit"));
    }
    Ok(bytes)
}
pub fn render_text(value: &Value) -> String {
    format!("{value}\n")
}
pub static NEW: Command = Command {
    id: "report.layout.new",
    path: &["report", "layout", "new"],
    contract: 1,
    summary: "Create an A3 vector map layout.",
    purpose: "Printing commands delegate to their owning Rust and native client contracts. Shared templates live in ds-brain; project scope requires a held selected-project context. Geometry stays in ds-network and document validation in ds-command-kernel.",
    chapter: Chapter::Reports,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "The authoritative layout, schema, shared setup receipt or artifact manifest.",
    examples: &[Example {
        command: "ds report layout new --output json",
        note: "See command arguments and report.layout.schema before invocation.",
        runnable: true,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: local,
};
pub static EDIT: Command = Command {
    id: "report.layout.edit",
    path: &["report", "layout", "edit"],
    contract: 1,
    summary: "Validate and apply one print document intent.",
    purpose: "Printing commands delegate to their owning Rust and native client contracts. Shared templates live in ds-brain; project scope requires a held selected-project context. Geometry stays in ds-network and document validation in ds-command-kernel.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[REQUEST],
    output: "The authoritative layout, schema, shared setup receipt or artifact manifest.",
    examples: &[Example {
        command: "ds report layout edit --output json",
        note: "See command arguments and report.layout.schema before invocation.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: local,
};
pub static SCHEMA: Command = Command {
    id: "report.layout.schema",
    path: &["report", "layout", "schema"],
    contract: 1,
    summary: "Describe the exact print document and editing grammar.",
    purpose: "Printing commands delegate to their owning Rust and native client contracts. Shared templates live in ds-brain; project scope requires a held selected-project context. Geometry stays in ds-network and document validation in ds-command-kernel.",
    chapter: Chapter::Reports,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "The authoritative layout, schema, shared setup receipt or artifact manifest.",
    examples: &[Example {
        command: "ds report layout schema --output json",
        note: "See command arguments and report.layout.schema before invocation.",
        runnable: true,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: local,
};
pub static RENDER: Command = Command {
    id: "report.layout.render",
    path: &["report", "layout", "render"],
    contract: 1,
    summary: "Produce vector PDF and SVG from a layout and held GeoJSON.",
    purpose: "Printing commands delegate to their owning Rust and native client contracts. Shared templates live in ds-brain; project scope requires a held selected-project context. Geometry stays in ds-network and document validation in ds-command-kernel.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[REQUEST],
    output: "The authoritative layout, schema, shared setup receipt or artifact manifest.",
    examples: &[Example {
        command: "ds report layout render --output json",
        note: "See command arguments and report.layout.schema before invocation.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: reporter,
};
pub static LIST: Command = Command {
    id: "report.layout.list",
    path: &["report", "layout", "list"],
    contract: 1,
    summary: "List published global samples or project printing setups.",
    purpose: "Printing commands delegate to their owning Rust and native client contracts. Shared templates live in ds-brain; project scope requires a held selected-project context. Geometry stays in ds-network and document validation in ds-command-kernel.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[SCOPE, LANE],
    output: "The authoritative layout, schema, shared setup receipt or artifact manifest.",
    examples: &[Example {
        command: "ds report layout list --output json",
        note: "See command arguments and report.layout.schema before invocation.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};
pub static GET: Command = Command {
    id: "report.layout.get",
    path: &["report", "layout", "get"],
    contract: 1,
    summary: "Read one shared printing setup and its revision.",
    purpose: "Printing commands delegate to their owning Rust and native client contracts. Shared templates live in ds-brain; project scope requires a held selected-project context. Geometry stays in ds-network and document validation in ds-command-kernel.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        SCOPE,
        LANE,
        Arg::value("id", "<id>", "Setup id.").required(),
    ],
    output: "The authoritative layout, schema, shared setup receipt or artifact manifest.",
    examples: &[Example {
        command: "ds report layout get --output json",
        note: "See command arguments and report.layout.schema before invocation.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};
pub static SAVE: Command = Command {
    id: "report.layout.save",
    path: &["report", "layout", "save"],
    contract: 1,
    summary: "Publish a printing setup using its expected revision.",
    purpose: "Printing commands delegate to their owning Rust and native client contracts. Shared templates live in ds-brain; project scope requires a held selected-project context. Geometry stays in ds-network and document validation in ds-command-kernel.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[SCOPE, LANE, REQUEST],
    output: "The authoritative layout, schema, shared setup receipt or artifact manifest.",
    examples: &[Example {
        command: "ds report layout save --output json",
        note: "See command arguments and report.layout.schema before invocation.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};
pub static CREATE: Command = Command {
    id: "report.layout.create",
    path: &["report", "layout", "create"],
    contract: 1,
    summary: "Create one global or project printing setup.",
    purpose: "Publishes a validated layout as a new stable setup ID. Project scope is the held selected project; global scope requires global printing authority.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[SCOPE, LANE, REQUEST],
    output: "The created setup, including its stable ID and first revision.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};
pub static UPDATE: Command = Command {
    id: "report.layout.update",
    path: &["report", "layout", "update"],
    contract: 1,
    summary: "Update one exact global or project printing revision.",
    purpose: "Publishes a validated layout only when expected_revision still names the current setup. It never retries a conflict as an overwrite.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[SCOPE, LANE, REQUEST],
    output: "The updated setup and its new revision.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};
pub static DELETE: Command = Command {
    id: "report.layout.delete",
    path: &["report", "layout", "delete"],
    contract: 1,
    summary: "Delete one exact global or project printing revision.",
    purpose: "Deletes the named setup only when expected_revision is current. Brain retains the audited tombstone; copied templates remain independent.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        SCOPE,
        LANE,
        Arg::value("id", "<id>", "Stable setup ID returned by layout list.").required(),
        Arg::value(
            "expected-revision",
            "<sha256>",
            "Exact current revision returned by layout get.",
        )
        .required(),
    ],
    output: "The deleted setup ID and exact removed revision.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};
pub static COPY: Command = Command {
    id: "report.layout.copy",
    path: &["report", "layout", "copy"],
    contract: 1,
    summary: "Copy an exact printing revision between libraries.",
    purpose: "Performs global adoption, global promotion or same-library duplication as one Brain transaction. Project locations always mean the held selected project. The source remains unchanged.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[LANE, REQUEST],
    output: "The independent destination setup, its revision and pinned source lineage.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};

fn reporter() -> Availability {
    crate::DS_REPORT.availability()
}
pub fn new(_i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    Ok(json!(ds_command_kernel::printing::default_layout()))
}
pub fn schema(_i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    Ok(
        json!({"layout":ds_command_kernel::printing::layout_schema(),"edit":ds_command_kernel::printing::command_schema(),"transactions":{"create":{"action":"create","layout":"<layout document>"},"update":{"action":"update","layout":"<layout document>","expected_revision":"<exact revision>"},"delete":"use --id and --expected-revision","copy":{"action":"copy","source":{"scope":"global|project","id":"<id>","revision":"<exact revision>"},"destination":{"scope":"global|project","id":"<new id>","name":"<optional name>","expected_revision":"<empty for create or exact revision>"}}},"render":"ds report tasks --task render_print_layout --output json"}),
    )
}
pub fn edit(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let input = bytes(
        i.require("request")?,
        ds_command_kernel::printing::MAX_INPUT_BYTES,
    )?;
    let result = ds_command_kernel::printing::evaluate(&input).map_err(invalid)?;
    serde_json::from_str(&result).map_err(invalid)
}
pub fn list(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    ds_cli_auth::printing(
        i.require("lane")?,
        i.require("scope")? == "global",
        &ds_cli_auth::PrintingRequest::List {},
    )
}
pub fn get(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    ds_cli_auth::printing(
        i.require("lane")?,
        i.require("scope")? == "global",
        &ds_cli_auth::PrintingRequest::Get {
            id: i.require("id")?.into(),
        },
    )
}
pub fn save(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let request =
        serde_json::from_slice(&bytes(i.require("request")?, 800_000)?).map_err(invalid)?;
    if !matches!(request, ds_cli_auth::PrintingRequest::Save { .. }) {
        return Err(invalid("save requires a save request"));
    }
    ds_cli_auth::printing(
        i.require("lane")?,
        i.require("scope")? == "global",
        &request,
    )
}
fn typed_request(i: &Inputs) -> Result<ds_cli_auth::PrintingRequest, Failure> {
    serde_json::from_slice(&bytes(i.require("request")?, 800_000)?).map_err(invalid)
}
fn scoped(i: &Inputs, request: &ds_cli_auth::PrintingRequest) -> Result<Value, Failure> {
    ds_cli_auth::printing(i.require("lane")?, i.require("scope")? == "global", request)
}
pub fn create(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let request = typed_request(i)?;
    if !matches!(request, ds_cli_auth::PrintingRequest::Create { .. }) {
        return Err(invalid("create requires a create request"));
    }
    scoped(i, &request)
}
pub fn update(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let request = typed_request(i)?;
    if !matches!(request, ds_cli_auth::PrintingRequest::Update { .. }) {
        return Err(invalid("update requires an update request"));
    }
    scoped(i, &request)
}
pub fn delete(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let request = ds_cli_auth::PrintingRequest::Delete {
        id: i.require("id")?.into(),
        expected_revision: i.require("expected-revision")?.into(),
    };
    scoped(i, &request)
}
pub fn copy(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let request = typed_request(i)?;
    if !matches!(request, ds_cli_auth::PrintingRequest::Copy { .. }) {
        return Err(invalid("copy requires a copy request"));
    }
    ds_cli_auth::printing(i.require("lane")?, true, &request)
}
pub fn render(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    // Copy input to a private scratch file so the bytes cannot change between validation and dispatch.
    let raw = bytes(i.require("request")?, 32 * 1024 * 1024)?;
    let scratch = tempfile::tempdir().map_err(invalid)?;
    let request = scratch.path().join("request.json");
    let result = scratch.path().join("result.json");
    std::fs::write(&request, raw).map_err(invalid)?;
    let completed = crate::DS_REPORT.call(
        "render-print-layout",
        &[
            "--request".into(),
            request.into_os_string(),
            "--result".into(),
            result.clone().into_os_string(),
        ],
        crate::EXPORT_TIMEOUT,
    )?;
    if !completed.succeeded() {
        return Err(invalid(completed.stderr));
    }
    serde_json::from_slice(&bytes(
        result
            .to_str()
            .ok_or_else(|| invalid("Non-UTF8 result path"))?,
        1024 * 1024,
    )?)
    .map_err(invalid)
}
