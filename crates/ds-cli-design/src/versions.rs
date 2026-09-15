//! Thin native history adapters: server snapshots, shared kernel comparison.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal},
};
use ds_client_core::design_versions::Command as Request;
use serde_json::{Value, json};
const TRANSFORMER: Arg = Arg::value(
    "transformer",
    "<name>",
    "Exact transformer in the selected project.",
)
.required();
const REFUSALS: &[Refusal] = &[Refusal {
    code: "design_version_refused",
    when: "The authenticated version read or kernel snapshot validation refuses the request",
    remedy: "Read the nested cause and remedy; list published versions and choose playback_available=true. Sign in to the named lane if required.",
}];
const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    args: &'static [Arg],
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose: "Read published transformer history without a desktop. Compare exact vN snapshots or a saved server head pinned once, under one captured project/owner/lane. Rust supplies exact change counts and consistency findings; no mutation or geometry payload. Unpublished browser versions are not server history.",
        chapter: Chapter::Design,
        effect: Effect::ReadOnly,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args,
        output: "Project, transformer, exact version descriptors and snapshot digests; bounded per-layer change counts and changed property names, consistency findings, and explicit truncation. Listing includes playback availability; at 200 rows truncation is reported.",
        examples: &[],
        refusals: REFUSALS,
        reference: Some("docs/reference/design.md"),
        availability: ds_cli_auth::native_availability,
    }
}
pub static LIST: Command = command(
    "design.version.list",
    &["design", "version", "list"],
    "List published transformer versions and playback availability.",
    &[TRANSFORMER, crate::transformer::LANE_ARG],
);
pub static COMPARE: Command = command(
    "design.version.compare",
    &["design", "version", "compare"],
    "Compare published transformer versions headlessly.",
    &[
        TRANSFORMER,
        Arg::value("from", "<vN>", "Exact published version on the left.").required(),
        Arg::value(
            "to",
            "<vN|head>",
            "Exact published version or saved server head pinned once.",
        )
        .required(),
        crate::transformer::LANE_ARG,
    ],
);
pub static BEGIN: Command = Command {
    id: "design.version.begin",
    path: &["design", "version", "begin"],
    contract: 1,
    summary: "Create one deliberate published transformer version (needs --yes).",
    purpose: "Create an immutable version of the selected project's current saved transformer without a Desktop or open map. ds-brain assigns the next vN ordinal and snapshots the current governed state; Rust validates the exact returned project, transformer and version identity.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER,
        Arg::value(
            "reason",
            "<text>",
            "Why this deliberate version is being created.",
        )
        .required(),
        Arg::value(
            "idempotency-key",
            "<opaque-key>",
            "Stable caller key; reuse it when retrying this exact version request.",
        )
        .required(),
        crate::transformer::LANE_ARG,
    ],
    output: "Lane, selected project, transformer, the server-assigned vN descriptor, and mutated=true.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};
fn ask(i: &Inputs, request: Request) -> Result<Value, Failure> {
    ds_cli_auth::design_versions(i.require("lane")?, &request).map(|r|r.into_result()).map_err(|e|
        Failure::failed("design_version_refused",e.to_string())
            .detail(json!({"cause":e.code(),"detail":e.detail_value()}))
            .remedy(e.remedy_text().unwrap_or("List published versions; use exact vN identifiers with playback_available=true. Unpublished browser versions must be published first.")))
}
pub fn list(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::List {
            transformer: i.require("transformer")?.into(),
        },
    )
}
pub fn compare(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Compare {
            transformer: i.require("transformer")?.into(),
            from: i.require("from")?.into(),
            to: i.require("to")?.into(),
        },
    )
}
pub fn begin(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let transformer = i.require("transformer")?;
    let reason = i.require("reason")?;
    ask(
        i,
        Request::Begin {
            transformer: transformer.into(),
            reason: reason.into(),
            idempotency_key: i.require("idempotency-key")?.into(),
        },
    )
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
