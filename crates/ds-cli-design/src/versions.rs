//! Headless version adapters: one explicit project and server-assigned history and explicit MV freezing.
use ds_cli_contract::{
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
    Context, Failure, Inputs,
};
use ds_client_core::design_versions::Command as Request;
use serde_json::{json, Value};
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
        purpose: "Use one explicit project and exact object ID without Desktop or active-project state. ds-brain alone assigns vN ordinals. New MV markers remain open for fenced design iterations until explicitly frozen; existing historical markers are frozen. LV comparison uses exact snapshots; MV comparison reads content-revision metadata. Restore is LV-only.",
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
pub static READ: Command = command(
    "design.version.read",
    &["design", "version", "read"],
    "Read one exact governed LV or MV vN marker.",
    &[
        PROJECT,
        KIND,
        OBJECT,
        TRANSFORMER,
        Arg::value("version", "<vN>", "Exact assigned governance marker.").required(),
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
        crate::transformer::LANE_ARG,
    ],
    Effect::GlobalWrite,
);
pub static REVISE: Command = command(
    "design.version.revise",
    &["design", "version", "revise"],
    "Revise one open MV governance vN against exact marker and source fences.",
    &[
        PROJECT,
        KIND,
        OBJECT,
        Arg::value("version", "<vN>", "Exact open governance marker.").required(),
        Arg::value("reason", "<text>", "Why this design iteration changed.").required(),
        Arg::value(
            "milestone",
            "<text>",
            "Optional updated milestone; omission keeps the existing value.",
        ),
        Arg::value(
            "expected-revision",
            "<number>",
            "Marker revision read before editing.",
        )
        .required(),
        Arg::value(
            "expected-source",
            "<content-revision|->",
            "Observed model head revision, or - when no content exists.",
        )
        .required(),
        crate::transformer::LANE_ARG,
    ],
    Effect::GlobalWrite,
);
pub static FREEZE: Command = command(
    "design.version.freeze",
    &["design", "version", "freeze"],
    "Freeze one exact MV governance vN before submission or a new version.",
    &[
        PROJECT,
        KIND,
        OBJECT,
        Arg::value("version", "<vN>", "Exact open governance marker.").required(),
        Arg::value("reason", "<text>", "Why this version is ready to freeze.").required(),
        Arg::value(
            "expected-revision",
            "<number>",
            "Marker revision read before freezing.",
        )
        .required(),
        crate::transformer::LANE_ARG,
    ],
    Effect::GlobalWrite,
);
pub static EVENTS: Command = command(
    "design.version.events",
    &["design", "version", "events"],
    "Read the audit trail of one exact MV governance vN.",
    &[
        PROJECT,
        KIND,
        OBJECT,
        Arg::value("version", "<vN>", "Exact governance marker.").required(),
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
pub fn read(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Read {
            transformer: object(i)?,
            version: i.require("version")?.into(),
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
pub fn begin(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Begin {
            transformer: object(i)?,
            reason: i.require("reason")?.into(),
            idempotency_key: i.require("idempotency-key")?.into(),
        },
    )
}
fn mv_object(i: &Inputs) -> Result<String, Failure> {
    if i.require("kind")? != "mv_model" {
        return Err(Failure::invalid(
            "invalid_input",
            "this lifecycle action requires --kind mv_model",
        )
        .remedy("use the exact MV model ID and --kind mv_model"));
    }
    object(i)
}
fn expected_revision(i: &Inputs) -> Result<i64, Failure> {
    i.require("expected-revision")?
        .parse::<i64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            Failure::invalid(
                "invalid_input",
                "expected-revision must be a positive number",
            )
            .remedy("read the exact vN marker and pass its revision")
        })
}
pub fn revise(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::ReviseModel {
            transformer: mv_object(i)?,
            version: i.require("version")?.into(),
            reason: i.require("reason")?.into(),
            expected_revision: expected_revision(i)?,
            expected_source_revision: i.require("expected-source")?.into(),
            milestone: i.value("milestone").map(str::to_owned),
        },
    )
}
pub fn freeze(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::FreezeModel {
            transformer: mv_object(i)?,
            version: i.require("version")?.into(),
            reason: i.require("reason")?.into(),
            expected_revision: expected_revision(i)?,
        },
    )
}
pub fn events(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::ModelEvents {
            transformer: mv_object(i)?,
            version: i.require("version")?.into(),
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
