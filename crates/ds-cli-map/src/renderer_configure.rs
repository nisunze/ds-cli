//! `ds map renderer configure` — explicitly select 2D or one 3D provider.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::map_active_view::{self, REQUEST_SCHEMA};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.renderer.configure",
    path: &["map", "renderer", "configure"],
    contract: 1,
    summary: "Select the paired map's active 2D or 3D renderer.",
    purpose: "Selects 2D, AWS terrain 3D, or Google Photorealistic 3D through the application-owned renderer lifecycle. Three-dimensional mode always requires an explicit provider; provider auto-selection is refused.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("mode", "<2d|3d>", "Active renderer dimension.")
            .choices(&["2d", "3d"])
            .required(),
        Arg::value(
            "provider",
            "<aws-terrain|google>",
            "Required only for 3d mode.",
        ),
        DESCRIPTOR_ARG,
    ],
    output: "The active renderer mode and, for 3D, the exact provider selected.",
    examples: &[
        Example {
            command: "ds map renderer configure --mode 3d --provider aws-terrain",
            note: "Use the public terrain globe.",
            runnable: false,
        },
        Example {
            command: "ds map renderer configure --mode 3d --provider google",
            note: "Use Google Photorealistic 3D when this project is configured for it.",
            runnable: false,
        },
        Example {
            command: "ds map renderer configure --mode 2d",
            note: "Return to the 2D renderer.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "renderer",
            when: "the mode/provider combination is ambiguous or unsupported",
            remedy: "use --mode 2d without --provider, or --mode 3d with --provider aws-terrain|google",
        },
        Refusal {
            code: "desktop_refused",
            when: "the map is closed or the requested provider is unavailable",
            remedy: "open a project map; for Google, load a project with Google 3D configured",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut request = json!({
        "schema": REQUEST_SCHEMA,
        "operation": "renderer.configure",
        "mode": inputs.require("mode")?,
    });
    if let Some(provider) = inputs.value("provider") {
        request["provider"] = json!(provider);
    }
    let plan = map_active_view::evaluate(&serde_json::to_vec(&request).unwrap())
        .map_err(|error| Failure::invalid("renderer", error))?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::RENDERER_CONFIGURE,
        plan["renderer"].clone(),
        crate::UI_TIMEOUT,
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "renderer set to {}{}\n",
        data["mode"].as_str().unwrap_or("unknown"),
        data["provider"]
            .as_str()
            .map(|provider| format!("/{provider}"))
            .unwrap_or_default(),
    )
}
