//! CLI/MCP transport for the closed Core-owned Solar application surface.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires},
};
use serde_json::Value;
pub static COMMAND: Command = Command {
    id: "solar.application",
    path: &["solar", "application"],
    contract: 1,
    summary: "Execute one closed native Solar application request through DS Core.",
    purpose: "Execute one schema-discovered native Solar request for an explicit project. DS Core owns Solar workspaces and selection, input capture, city and portfolio runs, progress, artifacts, final import, report bundles and publication; automated delivery ends at prompting drafts. The host captures account, lane and project; no browser, WASM or Desktop. City editing and portfolio membership use their own commands.",
    chapter: Chapter::Solar,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<id>",
            "Explicit authorized project; selection remains unchanged.",
        )
        .required(),
        ds_cli_contract::spec::Arg {
            summary: "Native Core authority lane.",
            ..ds_cli_contract::spec::LANE
        },
        Arg::value(
            "state-dir",
            "<absolute-path>",
            "Protected native Core state directory.",
        ),
        Arg::value(
            "request",
            "<json-file>",
            "Closed native Solar application request, maximum 32 MiB.",
        )
        .required(),
        Arg::value(
            "out",
            "<absolute-new-json-file>",
            "Optional new receipt file in an existing private directory, at most 257 MiB; returns path, size, SHA, project, root and lane, and never replaces a file.",
        ),
    ],
    output: "ds-solar.application-receipt/v1 pinned to the project. With --out, the full JSON is written privately and stdout returns ds-solar.application-artifact/v1 (out, byte_count, sha256, project_id, root, lane). A failure after the effect keeps server_refused with the completed operation; inspect it before retrying. Launches continue natively; read their progress.",
    examples: &[],
    refusals: ds_cli_server::STATUS.refusals,
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub fn execute(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    ds_cli_server::solar_application(inputs, context)
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|_| "Invalid Solar application receipt".into())
}
pub static SCHEMA: Command = Command {
    id: "solar.application.schema",
    path: &["solar", "application", "schema"],
    contract: 1,
    summary: "Discover one native Solar application request contract.",
    purpose: "Lists the closed native operations, or derives the exact selected request JSON schema directly from the Rust owner DTO. Reads no workspace, account, project, network or Desktop state. Repeat with one operation before executing its request through solar application.",
    chapter: Chapter::Solar,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "operation",
        "<exact-name>",
        "Native operation from the owner catalog; omitted lists names.",
    )],
    output: "Bounded ds-solar.application-schema/v1 catalog or selected closed request schema.",
    examples: &[],
    refusals: &[ds_cli_contract::spec::Refusal {
        code: "solar_application_schema",
        when: "the operation is unknown or its owner schema is unavailable",
        remedy: "list operation names without --operation and select one exact name",
    }],
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};
pub fn schema(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_cli_server::solar_application_schema(inputs.value("operation"))
        .map_err(|e| Failure::invalid("solar_application_schema", e))
}
