//! Headless camera commands use the same owner as Profile's WASM adapter.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;
use std::io::Read;

pub static COMMAND: Command = Command {
    id: "map.canvas.camera",
    path: &["map", "canvas", "camera"],
    contract: 1,
    summary: "Compute a Canvas2D camera using Profile's shared Rust primitives.",
    purpose: "Evaluates one bounded camera intent without a desktop, project, or network. Fit, pan, centered zoom, cursor zoom, projection and centering use the same ds-network spatial core used by Profile and printing WASM hosts. This computes a camera; it does not move an open application's camera.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "request",
        "<json-file>",
        "Camera intent document, at most 1 MiB; schema and examples in docs/reference/map.md.",
    )
    .required()],
    output: "Camera zoom, pan_x, pan_y; project returns scale, offset_x, offset_y. No model or scene payload.",
    examples: &[Example {
        command: "ds map canvas camera --request camera.json --output json",
        note: "Use {\"operation\":\"fit\"} for the initial camera; no app or authentication required.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "canvas_request_unreadable",
            when: "the local request file cannot be read",
            remedy: "pass a readable JSON file with --request",
        },
        Refusal {
            code: "canvas_request_invalid",
            when: "the shared kernel rejects the intent, dimensions or 1 MiB bound",
            remedy: "correct the camera request using docs/reference/map.md; dimensions must be finite and positive",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::layer::local_availability,
};
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let path = inputs.require("request")?;
    let io_error = |e: std::io::Error| {
        Failure::invalid("canvas_request_unreadable", e.to_string())
            .remedy("pass a readable JSON file with --request")
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(io_error)?
        .take(ds_command_kernel::canvas::MAX_COMMAND_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    ds_command_kernel::canvas::evaluate(&bytes).map_err(|e| Failure::invalid("canvas_request_invalid",e).remedy("correct the camera request using docs/reference/map.md; dimensions must be finite and positive"))
}
pub fn render(data: &Value) -> String {
    format!("{data}\n")
}
