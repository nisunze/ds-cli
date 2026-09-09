//! Printing authority discovery; schemas come directly from their Rust owner.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "map.print.schema",
    path: &["map", "print", "schema"],
    contract: 1,
    summary: "Discover every supported map-print request, layout and style field.",
    purpose: "Returns a compact section index, then the selected kernel-owned schema. Print styles are independent of live-map styles. Request controls include area grouping, paper and DPI; layout covers furniture, layer styles, label hierarchy, tables and context acquisition. Author through report.layout.edit and render through desktop.printing.map.export; the same commands are exposed through MCP.",
    chapter: Chapter::MapPresentation,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "section",
        "<section>",
        "Return one complete authoritative schema.",
    )
    .choices(&["request", "layout", "edit", "outputs"])],
    output: "Section index or one kernel-generated JSON Schema, with no network or project reads.",
    examples: &[Example {
        command: "ds map print schema --output json",
        note: "Start with the compact index, then select a section.",
        runnable: true,
    }],
    refusals: &[],
    reference: Some("docs/reference/map.md"),
    availability: crate::layer::local_availability,
};
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    Ok(match inputs.value("section") {
        Some("request") => ds_command_kernel::printing::map::request_schema(),
        Some("layout") => ds_command_kernel::printing::layout_schema(),
        Some("edit") => ds_command_kernel::printing::command_schema(),
        Some("outputs") => ds_command_kernel::report_formats::output_selection_schema(),
        _ => {
            json!({"sections":["request","layout","edit","outputs"],"next":"ds map print schema --section <section> --output json","author":"report.layout.edit","export":"desktop.printing.map.export","preview":"map.ui.open","assets":"assets.tree"})
        }
    })
}
pub fn render(value: &Value) -> String {
    format!("{value}\n")
}
