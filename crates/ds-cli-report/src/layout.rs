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
const ACTION: Arg = Arg::value(
    "action",
    "<action>",
    "defaults: derived sources, fallback styles, exclusions; catalog: which datasets may be selected; select: apply the default selection; toggle: switch one layer.",
)
.choices(&["defaults", "catalog", "select", "toggle"])
.default("defaults");
const LAYOUT_FILE: Arg = Arg::value(
    "layout",
    "<json-file>",
    "Held print layout (select, toggle).",
);
const RESOURCES: Arg = Arg::value(
    "resources",
    "<json-file>",
    "Catalogue facts: [{layer,label,country,ready,downloadable,unavailable}] (catalog, select; toggle of a catalogue layer).",
);
const LAYER: Arg = Arg::value(
    "layer",
    "<layer-id>",
    "Context layer id to toggle: a derived source or a catalogue layer.",
);
const ENABLED: Arg = Arg::value("enabled", "<bool>", "Switch the layer on or off.")
    .choices(&["true", "false"])
    .default("true");
pub static CONTEXT: Command = Command {
    id: "report.layout.context",
    path: &["report", "layout", "context"],
    contract: 1,
    summary: "Decide a layout's context layers: defaults, catalogue, selection.",
    purpose: "Which context layers exist, which a template starts with, and whether a catalogue dataset may be switched on are decided once in ds-command-kernel (printing::context) for the desktop page and this command alike. The host reports the catalogue's facts; the kernel returns rows, a selection or a refusal.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[ACTION, LAYOUT_FILE, RESOURCES, LAYER, ENABLED],
    output: "defaults: {derived,fallback_styles,excluded_catalog_layers}; catalog: {rows}; select/toggle: {layout}.",
    examples: &[
        Example {
            command: "ds report layout context --output json",
            note: "The derived sources every template may carry and their print styles.",
            runnable: true,
        },
        Example {
            command: "ds report layout context --action toggle --layout layout.json --layer elevation_contours --enabled true --output json",
            note: "Switch a derived source on; its print styles are seeded.",
            runnable: false,
        },
    ],
    refusals: CONTEXT_REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: local,
};
const CONTEXT_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "printing_invalid",
        when: "The bounded print document, catalogue facts or layer id is invalid",
        remedy: "Use report.layout.schema and correct the reported layout constraint",
    },
    Refusal {
        code: "printing_context_no_dataset",
        when: "The catalogue layer is unavailable, or neither held nor downloadable",
        remedy: "Download the dataset first, or choose one the catalogue reports as ready",
    },
];
pub static RENDER: Command = Command {
    id: "report.layout.render",
    path: &["report", "layout", "render"],
    contract: 1,
    summary: "Produce PDF, SVG, PNG or JPEG from a layout and held GeoJSON.",
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
    summary: "Publish a printing setup; with a revision it updates, else creates.",
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
        json!({"map_request":ds_command_kernel::printing::map::request_schema(),"layout":ds_command_kernel::printing::layout_schema(),"edit":ds_command_kernel::printing::command_schema(),"output_selection":ds_command_kernel::report_formats::output_selection_schema(),"transactions":{"create":{"action":"create","layout":"<layout document>"},"update":{"action":"update","layout":"<layout document>","expected_revision":"<exact revision>"},"delete":"use --id and --expected-revision","copy":{"action":"copy","source":{"scope":"global|project","id":"<id>","revision":"<exact revision>"},"destination":{"scope":"global|project","id":"<new id>","name":"<optional name>","expected_revision":"<empty for create or exact revision>"}}},"render":"ds report tasks --task render_print_layout --output json"}),
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
fn context_eval(request: Value) -> Result<Value, Failure> {
    let input = serde_json::to_vec(&request).map_err(invalid)?;
    let result = ds_command_kernel::printing::evaluate(&input).map_err(|e| {
        if e.contains("printing_context_no_dataset") {
            Failure::invalid("printing_context_no_dataset", e)
                .remedy("Download the dataset first, or choose one the catalogue reports as ready")
        } else {
            invalid(e)
        }
    })?;
    serde_json::from_str(&result).map_err(invalid)
}
fn json_file(i: &Inputs, name: &str) -> Result<Value, Failure> {
    serde_json::from_slice(&bytes(i.require(name)?, 800_000)?).map_err(invalid)
}
fn optional_json_file(i: &Inputs, name: &str) -> Result<Value, Failure> {
    match i.value(name) {
        Some(path) => serde_json::from_slice(&bytes(path, 800_000)?).map_err(invalid),
        None => Ok(json!([])),
    }
}
pub fn context(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    match i.require("action")? {
        "defaults" => context_eval(json!({"op": "context_defaults"})),
        "catalog" => context_eval(json!({
            "op": "context_catalog",
            "resources": json_file(i, "resources")?,
        })),
        "select" => context_eval(json!({
            "op": "context_default_selection",
            "layout": json_file(i, "layout")?,
            "resources": optional_json_file(i, "resources")?,
        })),
        "toggle" => {
            let id = i.require("layer")?;
            let enabled = i.require("enabled")? == "true";
            let resources = optional_json_file(i, "resources")?;
            let derived = context_eval(json!({"op": "context_defaults"}))?;
            let mut layer = derived["derived"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|row| row["id"] == id)
                .cloned();
            let mut resource = Value::Null;
            if layer.is_none() {
                let rows = context_eval(json!({"op": "context_catalog", "resources": resources}))?;
                if let Some(row) = rows["rows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|row| row["layer"]["id"] == id)
                {
                    layer = Some(row["layer"].clone());
                    resource = resources
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|fact| fact["layer"] == id)
                        .cloned()
                        .unwrap_or(Value::Null);
                }
            }
            let Some(layer) = layer else {
                return Err(invalid(format!(
                    "unknown context layer `{id}`: not a derived source and not in --resources"
                )));
            };
            let mut request = json!({
                "op": "context_toggle",
                "layout": json_file(i, "layout")?,
                "layer": layer,
                "enabled": enabled,
            });
            if !resource.is_null() {
                request["resource"] = resource;
            }
            context_eval(request)
        }
        other => Err(invalid(format!("unknown action `{other}`"))),
    }
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
    let request: ds_cli_auth::PrintingRequest =
        serde_json::from_slice(&bytes(i.require("request")?, 800_000)?).map_err(invalid)?;
    let ds_cli_auth::PrintingRequest::Save {
        layout,
        expected_revision,
    } = request
    else {
        return Err(invalid("save requires a save request"));
    };
    let global = i.require("scope")? == "global";
    // Whether this publish is a create or an update is the kernel's
    // decision — the same one the Printing setup page takes from the
    // revision it holds — so `ds` never sends the compatibility `save`
    // action of its own accord.
    let facts = ds_command_kernel::printing::lifecycle::Facts {
        intent: ds_command_kernel::printing::lifecycle::Intent::Save,
        library: if global {
            ds_command_kernel::printing::lifecycle::Library::Global
        } else {
            ds_command_kernel::printing::lifecycle::Library::Project
        },
        online: true,
        editing: true,
        loaded_matches: !expected_revision.is_empty(),
        held_revision: expected_revision.clone(),
        ..Default::default()
    };
    let plan = ds_command_kernel::printing::lifecycle::plan(&facts).map_err(invalid)?;
    let request = match plan.decision {
        ds_command_kernel::printing::lifecycle::Decision::Update => {
            ds_cli_auth::PrintingRequest::Update {
                layout,
                expected_revision: plan.expected_revision.unwrap_or(expected_revision),
            }
        }
        ds_command_kernel::printing::lifecycle::Decision::Create => {
            ds_cli_auth::PrintingRequest::Create { layout }
        }
        _ => {
            return Err(invalid(
                plan.refusal
                    .map(|r| r.message_key)
                    .unwrap_or_else(|| "save is not admissible".into()),
            ));
        }
    };
    ds_cli_auth::printing(i.require("lane")?, global, &request)
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
