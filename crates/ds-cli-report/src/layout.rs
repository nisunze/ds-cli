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
const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "printing_invalid",
        when: "The bounded print document or intent is invalid",
        remedy: "Use report.layout.schema and correct the reported layout constraint",
    },
    // The deployed layout validator's own refusal, re-raised under its name.
    // It is a client-input failure even when the deployed validator is the
    // half that is out of date, which is why its remedy names both causes.
    Refusal {
        code: "print_layout_invalid",
        when: "The deployed layout validator refused the submitted document, naming the field",
        remedy: "Correct the named field against report.layout.schema; if it is valid, the deployed print validator is older than this client and must be redeployed",
    },
    Refusal {
        code: "print_setup_not_found",
        when: "The named printing setup does not exist in the addressed scope",
        remedy: "List the scope with report.layout.list, or copy a setup into it with report.layout.copy",
    },
    Refusal {
        code: "print_validator_unavailable",
        when: "The layout validator could not be reached, or answered a server fault",
        remedy: "Retry without changing the layout; a repeated refusal is a deployment failure, not an authoring one",
    },
];
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
const SESSION_STATE: Arg = Arg::value(
    "state",
    "<json-file>",
    "The editor state the host holds: library, has_layout, editing, dirty, loaded_global, has_baseline, saved_id, revision, undo_depth, redo_depth. Omit with --event omitted to read the zero state.",
);
const SESSION_EVENT: Arg = Arg::value(
    "event",
    "<json-file>",
    "The event: {kind: draft|load|begin_edit|open_context|edit|undo|redo|discard|saved|deleted, ...}.",
);
pub static SESSION: Command = Command {
    id: "report.layout.session",
    path: &["report", "layout", "session"],
    contract: 1,
    summary: "Decide one print-editor event: next state and the host's operations.",
    purpose: "May a layout be changed right now, is it dirty, what does Cancel restore, what do Undo and Redo mean and how deep they go are decided once in ds-command-kernel (printing::session) for the Printing setup page and any headless editor alike. The host reports the state it holds and the event; the kernel returns the next state, the operations to perform on the documents the host keeps (install, push/pop undo, snapshot/restore baseline, clear), or a refusal. Documents never cross this boundary. With no arguments it returns the zero state and the history cap.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[SESSION_STATE, SESSION_EVENT],
    output: "{state, ops, applied, refusal?} for an event; {state, history_cap} with no arguments.",
    examples: &[
        Example {
            command: "ds report layout session --output json",
            note: "The editor's zero state and how deep its undo stack goes.",
            runnable: true,
        },
        Example {
            command: "ds report layout session --state state.json --event event.json --output json",
            note: "One event; `ops` says what to do to the held documents, in order.",
            runnable: false,
        },
    ],
    refusals: SESSION_REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: local,
};
const SESSION_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "printing_invalid",
        when: "The state or event file is malformed, or a field is unknown or unbounded",
        remedy: "Use report.layout.schema and correct the reported layout constraint",
    },
    Refusal {
        code: "printing_discard_unconfirmed",
        when: "A draft, load or cancel would drop unsaved edits and the event does not say confirmed",
        remedy: "Ask the operator, then send the same event with confirmed: true",
    },
    Refusal {
        code: "printing_not_editing",
        when: "An edit, undo or redo arrives outside edit mode",
        remedy: "Send begin_edit first (or draft a new document)",
    },
    Refusal {
        code: "printing_global_read_only",
        when: "A global template is opened for editing from a project page",
        remedy: "Copy it into the project (report.layout.copy) and edit the copy",
    },
    Refusal {
        code: "printing_no_document",
        when: "The event needs an open document and none is held",
        remedy: "Draft or load a document first",
    },
    Refusal {
        code: "printing_nothing_to_undo",
        when: "Undo with an empty undo stack",
        remedy: "Nothing to do; the document is at its oldest held state",
    },
    Refusal {
        code: "printing_nothing_to_redo",
        when: "Redo with an empty redo stack",
        remedy: "Nothing to do; the document is at its newest held state",
    },
];
const ADD_LAYOUT: Arg =
    Arg::value("layout", "<json-file>", "Held print layout to add to.").required();
const ADD_KIND: Arg = Arg::value("kind", "<kind>", "The element kind.")
    .choices(&[
        "map",
        "text",
        "legend",
        "scale_bar",
        "north_arrow",
        "logo",
        "rectangle",
        "table",
    ])
    .required();
const ADD_ID: Arg = Arg::value(
    "id",
    "<element-id>",
    "Element id; minted as <kind>-<n> when omitted.",
);
const ADD_ASSET: Arg = Arg::value(
    "asset",
    "<asset-id>",
    "For a logo: the layout asset it shows.",
);
pub static ADD: Command = Command {
    id: "report.layout.add",
    path: &["report", "layout", "add"],
    contract: 1,
    summary: "Add the canonical new element of a kind to a layout.",
    purpose: "What a newly added text, frame, table, legend or logo element is — its frame, ink, type size and bindings — is decided once in ds-command-kernel (printing::elements) for the Printing setup page and this command alike, so an author never retypes the defaults. A legend carries its binding from the start; a logo names an asset the layout holds.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[ADD_LAYOUT, ADD_KIND, ADD_ID, ADD_ASSET],
    output: "{layout, index}: the layout with the new element and where it sits.",
    examples: &[Example {
        command: "ds report layout add --layout layout.json --kind table --output json",
        note: "The new table binds lv_print_info with Description/Unit/Quantity.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: local,
};
const STYLE_ACTION: Arg = Arg::value(
    "action",
    "<action>",
    "choices: which governed print styles a layer may bind; set: bind (or, with an empty --style-ref, unbind) one layer.",
)
.choices(&["choices", "set"])
.default("choices");
const STYLE_CATALOGUE: Arg = Arg::value(
    "catalogue",
    "<json-file>",
    "Style catalogue facts: [{style_ref, style_target}] as the layers snapshot reports them.",
)
.required();
const STYLE_LAYOUT: Arg = Arg::value("layout", "<json-file>", "Held print layout (set).");
const STYLE_LAYER: Arg = Arg::value("layer", "<layer-id>", "Logical print layer id (set).");
const STYLE_REF: Arg = Arg::value(
    "style-ref",
    "<style-ref>",
    "Governed print style to bind; empty unbinds (set).",
);
pub static STYLE_REF_COMMAND: Command = Command {
    id: "report.layout.style-ref",
    path: &["report", "layout", "style-ref"],
    contract: 1,
    summary: "Which governed print styles a print layer may bind; bind one.",
    purpose: "Which `_print` style documents may be bound to a print layer, and what binding one means, are decided once in ds-command-kernel (printing::elements) from the style catalogue's facts — the same decision the Printing setup page's picker takes. The governed clones themselves are created with style.print.create.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        STYLE_ACTION,
        STYLE_CATALOGUE,
        STYLE_LAYOUT,
        STYLE_LAYER,
        STYLE_REF,
    ],
    output: "choices: {refs}; set: {layout}.",
    examples: &[Example {
        command: "ds report layout style-ref --catalogue styles.json --output json",
        note: "The print styles a layer may bind, in stable order.",
        runnable: false,
    }],
    refusals: STYLE_REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: local,
};
const STYLE_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "printing_invalid",
        when: "The layout, catalogue or layer id is malformed",
        remedy: "Use report.layout.schema and correct the reported layout constraint",
    },
    Refusal {
        code: "printing_style_ref_ineligible",
        when: "The style reference is not a governed print style the catalogue offers",
        remedy: "Create the governed clone with style.print.create, or pick one from --action choices",
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
        json!({"map_request":ds_command_kernel::printing::map::request_schema(),"layout":ds_command_kernel::printing::layout_schema(),"edit":ds_command_kernel::printing::command_schema(),"output_selection":ds_command_kernel::report_formats::output_selection_schema(),"transactions":{"create":{"action":"create","layout":"<layout document>"},"update":{"action":"update","layout":"<layout document>","expected_revision":"<exact revision>"},"save":{"action":"save","layout":"<layout document>","expected_revision":"<empty to create, or the exact revision to update>"},"delete":"use --id and --expected-revision","copy":{"action":"copy","source":{"scope":"global|project","id":"<id>","revision":"<exact revision>"},"destination":{"scope":"global|project","id":"<new id>","name":"<optional name>","expected_revision":"<empty for create or exact revision>"}}},"render":"ds report tasks --task render_print_layout --output json"}),
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
pub fn add(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let mut request = json!({
        "op": "add_element",
        "layout": json_file(i, "layout")?,
        "kind": i.require("kind")?,
        "asset": i.value("asset").unwrap_or(""),
    });
    if let Some(id) = i.value("id") {
        request["id"] = Value::String(id.into());
    }
    let input = serde_json::to_vec(&request).map_err(invalid)?;
    let result = ds_command_kernel::printing::evaluate(&input).map_err(invalid)?;
    serde_json::from_str(&result).map_err(invalid)
}
pub fn style_ref(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let catalogue = json_file(i, "catalogue")?;
    let request = match i.require("action")? {
        "choices" => json!({"op": "style_ref_choices", "catalogue": catalogue}),
        _ => json!({
            "op": "set_layer_style_ref",
            "layout": json_file(i, "layout")?,
            "layer_id": i.require("layer")?,
            "style_ref": i.value("style-ref").unwrap_or(""),
            "catalogue": catalogue,
        }),
    };
    let input = serde_json::to_vec(&request).map_err(invalid)?;
    let result = ds_command_kernel::printing::evaluate(&input).map_err(|e| {
        if e.starts_with("printing_style_ref_ineligible") {
            Failure::invalid("printing_style_ref_ineligible", e)
                .remedy("Create the governed clone with style.print.create, or pick one from --action choices")
        } else {
            invalid(e)
        }
    })?;
    serde_json::from_str(&result).map_err(invalid)
}
pub fn session(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let (state, event) = (i.value("state"), i.value("event"));
    let request = match (state, event) {
        (None, None) => json!({"op": "session_defaults"}),
        (Some(_), Some(_)) => json!({
            "op": "session_step",
            "state": json_file(i, "state")?,
            "event": json_file(i, "event")?,
        }),
        _ => return Err(invalid("--state and --event go together")),
    };
    let input = serde_json::to_vec(&request).map_err(invalid)?;
    let result = ds_command_kernel::printing::evaluate(&input).map_err(invalid)?;
    let value: Value = serde_json::from_str(&result).map_err(invalid)?;
    if let Some(code) = value["refusal"]["code"].as_str() {
        return Err(session_refusal(code));
    }
    Ok(value)
}
/// The kernel's refusal under its own code, with the remedy this command
/// declares for it. One arm per declared code, so the declaration and the
/// emission cannot drift apart unnoticed.
fn session_refusal(code: &str) -> Failure {
    let remedy = |code: &str| {
        SESSION_REFUSALS
            .iter()
            .find(|r| r.code == code)
            .map(|r| r.remedy)
            .unwrap_or("Correct the reported state or event")
    };
    match code {
        "printing_discard_unconfirmed" => {
            Failure::invalid("printing_discard_unconfirmed", code).remedy(remedy(code))
        }
        "printing_not_editing" => {
            Failure::invalid("printing_not_editing", code).remedy(remedy(code))
        }
        "printing_global_read_only" => {
            Failure::invalid("printing_global_read_only", code).remedy(remedy(code))
        }
        "printing_no_document" => {
            Failure::invalid("printing_no_document", code).remedy(remedy(code))
        }
        "printing_nothing_to_undo" => {
            Failure::invalid("printing_nothing_to_undo", code).remedy(remedy(code))
        }
        "printing_nothing_to_redo" => {
            Failure::invalid("printing_nothing_to_redo", code).remedy(remedy(code))
        }
        other => invalid(format!("unknown editor refusal `{other}`")),
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
/// What `layout save` publishes, from whichever documented transaction the
/// caller derived from `report.layout.schema`.
///
/// `schema` documented `create`, `update` and `copy` and never `save`, while
/// `save` accepted nothing but a save request — so the only shapes discovery
/// offered were the ones this command refused, and an agent that followed the
/// schema got `save requires a save request`. Both halves are fixed: the
/// schema documents `save`, and a create or an update reaches the same
/// publish, because create-versus-update is decided from facts below and not
/// from the word the caller used. `copy` and `delete` are different
/// transactions with their own commands and stay refused here, by name.
fn publication(
    request: ds_cli_auth::PrintingRequest,
) -> Result<(ds_command_kernel::printing::Layout, String), Failure> {
    match request {
        ds_cli_auth::PrintingRequest::Save {
            layout,
            expected_revision,
        }
        | ds_cli_auth::PrintingRequest::Update {
            layout,
            expected_revision,
        } => Ok((layout, expected_revision)),
        // A create names no revision, which is exactly the fact that makes
        // the lifecycle plan below choose Create.
        ds_cli_auth::PrintingRequest::Create { layout } => Ok((layout, String::new())),
        _ => Err(invalid(
            "layout save accepts action save, create or update; use report.layout.copy for a copy \
             and report.layout.delete for a delete",
        )),
    }
}

pub fn save(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let request: ds_cli_auth::PrintingRequest =
        serde_json::from_slice(&bytes(i.require("request")?, 800_000)?).map_err(invalid)?;
    let (layout, expected_revision) = publication(request)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Fill one documented transaction's placeholders with real values.
    ///
    /// A placeholder is any string in angle brackets; what it stands for is
    /// decided by the key it sits under, so the test never re-states the
    /// shape the schema is publishing.
    fn filled(key: &str, template: &Value) -> Value {
        match template {
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(name, value)| (name.clone(), filled(name, value)))
                    .collect(),
            ),
            // A documented choice list is filled with its first choice.
            Value::String(text) if text.contains('|') => json!(text.split('|').next()),
            Value::String(text) if text.starts_with('<') => match key {
                "layout" => {
                    json!(ds_command_kernel::printing::default_layout())
                }
                "revision" => json!("a".repeat(64)),
                // An update pins an exact revision; a save may name none,
                // which is the fact that makes it a create.
                "expected_revision" if text.contains("empty") => json!(""),
                "expected_revision" => json!("a".repeat(64)),
                "id" => json!("sample_layout"),
                "name" => json!("Sample"),
                "scope" => json!("global"),
                _ => template.clone(),
            },
            other => other.clone(),
        }
    }

    fn documented_schema() -> Value {
        let context = Context {
            confirmed: false,
            output: ds_cli_contract::Output {
                format: ds_cli_contract::Format::Json,
                pretty: false,
                color: false,
            },
        };
        schema(&Inputs::default(), &context).expect("the schema is local and cannot fail")
    }

    fn transaction(name: &str) -> ds_cli_auth::PrintingRequest {
        let schema = documented_schema();
        let documented = filled(name, &schema["transactions"][name]);
        serde_json::from_value(documented)
            .unwrap_or_else(|error| panic!("documented `{name}` does not parse: {error}"))
    }

    /// Discovery and acceptance were two different contracts: `schema`
    /// documented `create`, `update` and `copy`, `save` accepted only `save`,
    /// and an agent that read the schema was refused by every command it
    /// could reach. Every documented transaction must now parse into a
    /// request the command named for it actually accepts.
    #[test]
    fn every_documented_transaction_parses_into_the_request_its_command_accepts() {
        assert!(matches!(
            transaction("create"),
            ds_cli_auth::PrintingRequest::Create { .. }
        ));
        assert!(matches!(
            transaction("update"),
            ds_cli_auth::PrintingRequest::Update { .. }
        ));
        assert!(matches!(
            transaction("save"),
            ds_cli_auth::PrintingRequest::Save { .. }
        ));
        assert!(matches!(
            transaction("copy"),
            ds_cli_auth::PrintingRequest::Copy { .. }
        ));
        // Delete is documented as the two flags it takes, not as a body.
        let schema = documented_schema();
        let delete = schema["transactions"]["delete"]
            .as_str()
            .expect("delete is documented as its flags");
        assert!(delete.contains("--id") && delete.contains("--expected-revision"));
    }

    /// `layout save` publishes whichever of the three publishing shapes the
    /// caller derived from the schema. Create-versus-update stays a decision
    /// taken from the revision, never from the word.
    #[test]
    fn save_publishes_every_schema_derived_publishing_shape_and_names_what_it_refuses() {
        for name in ["save", "create", "update"] {
            let (layout, revision) = publication(transaction(name))
                .unwrap_or_else(|error| panic!("save refused documented `{name}`: {error:?}"));
            assert_eq!(layout, ds_command_kernel::printing::default_layout());
            // A create carries no revision; the other two carry the one the
            // schema documents for them.
            let expected = if name == "update" { 64 } else { 0 };
            assert_eq!(revision.len(), expected, "{name}");
        }
        for other in [
            transaction("copy"),
            ds_cli_auth::PrintingRequest::List {},
            ds_cli_auth::PrintingRequest::Delete {
                id: "sample_layout".into(),
                expected_revision: "a".repeat(64),
            },
        ] {
            let refusal = publication(other).expect_err("a different transaction is refused");
            let message = format!("{refusal:?}");
            for action in ["save", "create", "update"] {
                assert!(message.contains(action), "{message}");
            }
        }
    }
}
