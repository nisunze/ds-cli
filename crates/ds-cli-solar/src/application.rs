//! CLI/MCP transport for the closed Server-owned Solar application surface.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution},
};
use serde_json::Value;
pub static COMMAND: Command = Command {
    id: "solar.application",
    path: &["solar", "application"],
    contract: 1,
    summary: "Execute one closed native Solar application request on Server.",
    purpose: "The native Server owns workspace approval, preparation, readiness, annual generation profiles, city and portfolio runs, progress, cancellation, inventories, bounded artifact reads and final import. Requests are the closed ds-solar-native application enum, never a generic operation or API escape. The Server captures and authorizes the explicit project; browser, WASM and paired Desktop are unnecessary. Use the local project lifecycle for city editing and its publication outbox; use portfolio governance for catalog editing.",
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
        Arg::value("lane", "<stable|canary>", "Native Server authority lane.")
            .default("stable")
            .choices(&["stable", "canary"]),
        Arg::value(
            "state-dir",
            "<absolute-path>",
            "Protected native Server state directory.",
        ),
        Arg::value(
            "request",
            "<json-file>",
            "Closed native Solar application request, maximum 32 MiB.",
        )
        .required(),
    ],
    output: "Project-pinned ds-solar.application-receipt/v1. Launch receipts describe continuing native work; read its progress until done.",
    examples: &[],
    refusals: ds_cli_server::STATUS.refusals,
    reference: Some("docs/reference/solar.md"),
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
    availability: || ds_cli_contract::spec::Availability::Available,
};
pub fn schema(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_cli_server::solar_application_schema(inputs.value("operation"))
        .map_err(|e| Failure::invalid("solar_application_schema", e))
}
