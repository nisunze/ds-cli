//! Headless version adapters: one explicit project and server-assigned history.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_client_core::design_versions::Command as Request;
use ds_command_kernel::design_versions_headless::{BatchItem, Create, ObjectRef, Shared};
use serde_json::{Value, json};
pub const PROJECT: Arg = Arg::value(
    "project",
    "<project-id>",
    "Explicit project authorized for this request; saved selection is unused.",
)
.required();
const TRANSFORMER: Arg = Arg::value(
    "transformer",
    "<name>",
    "Compatibility spelling for one LV object; use either this or --object.",
);
const KIND: Arg = Arg::value(
    "kind",
    "<kind>",
    "Governed object kind; MV versions pin immutable content revisions.",
)
.choices(&["lv_transformer", "mv_model"])
.default("lv_transformer");
const OBJECT: Arg = Arg::value(
    "object",
    "<id>",
    "Exact LV transformer or MV project model identity.",
);
const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "design_version_refused",
        when: "The authenticated history call or shared Rust validation refuses the request",
        remedy: "Read the nested cause; use one explicit project/kind/object and assigned vN identities. MV restore is unsupported; download its pinned content revision.",
    },
    Refusal {
        code: "invalid_input",
        when: "Object spelling is ambiguous or a local request is invalid",
        remedy: "Pass --project and either --transformer or --kind with --object; never combine the two object spellings.",
    },
];
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
        contract: 2,
        summary,
        purpose: "Use one explicit project and captured identity without Desktop or active-project state. ds-brain alone assigns vN ordinals. An MV vN marks a milestone such as a submission; show names the content revision its attachments bind to. LV comparison uses exact snapshots; MV comparison reports pinned content-revision metadata without claiming geometry comparison. Local browser rooms are not published history. Restore is LV-only.",
        chapter: Chapter::Design,
        effect,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args,
        output: "Explicit project/object and validated bounded history, saved head or assigned vN receipt. MV comparison identifies changed lineage metadata; LV comparison returns bounded exact change counts.",
        examples: &[],
        refusals: REFUSALS,
        reference: Some("docs/reference/design.md"),
        search: &[],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    }
}
pub static LIST: Command = command(
    "design.version.list",
    &["design", "version", "list"],
    "List governed LV or MV versions and their pinned source.",
    &[
        PROJECT,
        KIND,
        OBJECT,
        TRANSFORMER,
        crate::transformer::LANE_ARG,
    ],
    Effect::ReadOnly,
);
pub static STATUS: Command = command(
    "design.version.status",
    &["design", "version", "status"],
    "Read the exact saved LV or MV version head.",
    &[
        PROJECT,
        KIND,
        OBJECT,
        TRANSFORMER,
        crate::transformer::LANE_ARG,
    ],
    Effect::ReadOnly,
);
pub static COMPARE: Command = command(
    "design.version.compare",
    &["design", "version", "compare"],
    "Compare governed LV snapshots or MV revision metadata.",
    &[
        PROJECT,
        KIND,
        OBJECT,
        TRANSFORMER,
        Arg::value("from", "<vN>", "Exact assigned version on the left.").required(),
        Arg::value(
            "to",
            "<vN|head>",
            "Exact assigned version or saved head pinned once.",
        )
        .required(),
        crate::transformer::LANE_ARG,
    ],
    Effect::ReadOnly,
);
pub static SHOW: Command = command(
    "design.version.show",
    &["design", "version", "show"],
    "Show one governed LV or MV version and, for MV, its content revision.",
    &[
        PROJECT,
        KIND,
        OBJECT,
        TRANSFORMER,
        Arg::value("version", "<vN>", "Exact assigned version.").required(),
        crate::transformer::LANE_ARG,
    ],
    Effect::ReadOnly,
);
pub static BEGIN: Command = command(
    "design.version.begin",
    &["design", "version", "begin"],
    "Create a governed LV or MV version (needs --yes).",
    &[
        PROJECT,
        KIND,
        OBJECT,
        TRANSFORMER,
        Arg::value(
            "reason",
            "<text>",
            "Why this deliberate version is being created.",
        )
        .required(),
        Arg::value(
            "idempotency-key",
            "<key>",
            "Stable key for retries of this exact object/reason.",
        )
        .required(),
        Arg::value(
            "milestone",
            "<text>",
            "Milestone label, e.g. the submission this marks (at most 120 bytes).",
        ),
        Arg::value(
            "expected-source",
            "<revision|->",
            "MV: the head revision you reviewed, or - for no content yet. LV: its RFC3339 update time.",
        ),
        crate::transformer::LANE_ARG,
    ],
    Effect::GlobalWrite,
);
pub static BEGIN_BATCH: Command = command(
    "design.version.begin-batch",
    &["design", "version", "begin-batch"],
    "Create governed versions for many LV/MV objects at once (needs --yes).",
    &[
        PROJECT,
        Arg::value(
            "file",
            "<batch.json>",
            "JSON {reason?, milestone?, items[]} of 1..200 objects; shape in design.md.",
        )
        .required(),
        crate::transformer::LANE_ARG,
    ],
    Effect::GlobalWrite,
);
pub static SUMMARIES: Command = command(
    "design.version.summaries",
    &["design", "version", "summaries"],
    "Latest governed version and count for many LV/MV objects.",
    &[
        PROJECT,
        KIND,
        Arg::repeated("object", "<id>", "Object of --kind; repeat for up to 200."),
        crate::transformer::LANE_ARG,
    ],
    Effect::ReadOnly,
);
pub static RESTORE: Command = command(
    "design.version.restore",
    &["design", "version", "restore"],
    "Restore an exact governed LV version (needs --yes).",
    &[
        PROJECT,
        KIND,
        OBJECT,
        TRANSFORMER,
        Arg::value("version", "<vN>", "Exact assigned LV version to restore.").required(),
        Arg::value("reason", "<text>", "Why this version is being restored.").required(),
        Arg::value(
            "idempotency-key",
            "<key>",
            "Stable key for retries of this exact restore.",
        )
        .required(),
        crate::transformer::LANE_ARG,
    ],
    Effect::GlobalWrite,
);
fn object(i: &Inputs) -> Result<String, Failure> {
    match (i.value("object"), i.value("transformer")) {
        (Some(object), None) => Ok(object.into()),
        (None, Some(transformer)) if i.require("kind")? == "lv_transformer" => {
            Ok(transformer.into())
        }
        _ => Err(Failure::invalid(
            "invalid_input",
            "Choose either --kind/--object or the LV --transformer compatibility spelling",
        )
        .remedy("Pass exactly one object spelling and an explicit --project")),
    }
}
fn ask(i: &Inputs, request: Request) -> Result<Value, Failure> {
    let request = Request::Object {
        kind: i.require("kind")?.into(),
        command: Box::new(request),
    };
    ds_cli_auth::design_versions_for_project(i.require("lane")?,i.require("project")?,&request).map_err(|error|Failure::failed("design_version_refused",error.to_string()).detail(json!({"cause":error.code(),"detail":error.detail_value()})).remedy(error.remedy_text().unwrap_or("Choose exact assigned versions; MV restore is unavailable. Review a conflict without changing the accepted request.")))
}
pub fn list(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::List {
            transformer: object(i)?,
        },
    )
}
pub fn status(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Status {
            transformer: object(i)?,
        },
    )
}
pub fn compare(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Compare {
            transformer: object(i)?,
            from: i.require("from")?.into(),
            to: i.require("to")?.into(),
        },
    )
}
pub fn show(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Show {
            transformer: object(i)?,
            version: i.require("version")?.into(),
        },
    )
}
pub fn begin(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Begin {
            transformer: object(i)?,
            create: Create {
                reason: i.require("reason")?.into(),
                milestone: i.value("milestone").map(str::to_owned),
                expected_source: i.value("expected-source").map(str::to_owned),
                idempotency_key: i.require("idempotency-key")?.into(),
            },
        },
    )
}
/// The batch file: `{reason?, milestone?, items:[BatchItem]}`, no other keys.
/// Items are the kernel's own closed type, so the file and the request
/// cannot describe an object two different ways.
fn batch(value: Value) -> Result<(Vec<BatchItem>, Shared), String> {
    let mut object = match value {
        Value::Object(object) => object,
        _ => return Err("the batch file must be one JSON object".into()),
    };
    let text = |object: &mut serde_json::Map<String, Value>, key: &str| match object.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(format!("`{key}` must be text")),
    };
    let shared = Shared {
        reason: text(&mut object, "reason")?,
        milestone: text(&mut object, "milestone")?,
    };
    let items = object
        .remove("items")
        .ok_or("the batch file needs `items`")?;
    if let Some(unknown) = object.keys().next() {
        return Err(format!("unknown key `{unknown}`"));
    }
    let items: Vec<BatchItem> = serde_json::from_value(items).map_err(|e| e.to_string())?;
    Ok((items, shared))
}
/// A selection spans objects, so it is not wrapped in one `--kind`.
fn ask_selection(i: &Inputs, request: Request) -> Result<Value, Failure> {
    ds_cli_auth::design_versions_for_project(i.require("lane")?, i.require("project")?, &request)
        .map_err(|error| {
            Failure::failed("design_version_refused", error.to_string())
                .detail(json!({"cause":error.code(),"detail":error.detail_value()}))
                .remedy(error.remedy_text().unwrap_or(
                    "Read each item's own outcome; fix the named object and retry only it.",
                ))
        })
}
pub fn begin_batch(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = i.require("file")?;
    let text = std::fs::metadata(path)
        .ok()
        .filter(|meta| meta.len() <= 1024 * 1024)
        .and_then(|_| std::fs::read(path).ok())
        .ok_or_else(|| {
            Failure::invalid(
                "invalid_input",
                format!("{path}: unreadable or above 1 MiB"),
            )
            .remedy("Name a readable JSON batch file of at most 1 MiB")
        })?;
    let (items, shared) = serde_json::from_slice(&text)
        .map_err(|e| e.to_string())
        .and_then(batch)
        .map_err(|e| {
            Failure::invalid("invalid_input", format!("{path}: {e}"))
                .remedy("Write {reason?, milestone?, items:[{object:{kind,id}, idempotency_key, …}]} with no other keys")
        })?;
    ask_selection(i, Request::BeginBatch { items, shared })
}
pub fn summaries(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let kind = i.require("kind")?;
    ask_selection(
        i,
        Request::Summaries {
            objects: i
                .repeated("object")
                .iter()
                .map(|id| ObjectRef {
                    kind: kind.into(),
                    id: id.clone(),
                })
                .collect(),
        },
    )
}
pub fn restore(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Restore {
            transformer: object(i)?,
            version: i.require("version")?.into(),
            reason: i.require("reason")?.into(),
            idempotency_key: i.require("idempotency-key")?.into(),
        },
    )
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
