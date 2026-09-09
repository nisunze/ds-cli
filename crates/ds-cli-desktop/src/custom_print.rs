//! Exact selected-sector area preparation. All cartographic decisions are kernel-owned.
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal},
};
use serde_json::{Value, json};
use std::time::Duration;

pub const AREA_OP: BridgeOp = BridgeOp {
    operation: "printing.custom.area",
    arguments: &["project", "id", "codes", "include_geometry"],
};
const INVALID: Refusal = Refusal {
    code: "custom_print_area_invalid",
    when: "The exact project, map identity or selected sector codes fail kernel validation",
    remedy: "Use a bounded project and map id and one to eight distinct four-digit Rwanda sector codes",
};
pub static AREA_COMMAND: Command = Command {
    id: "desktop.printing.custom.area",
    path: &["desktop", "printing", "custom", "area"],
    contract: 1,
    summary: "Prepare an exact connected custom print area from Rwanda sectors.",
    purpose: "Reads the selected sectors through the matching active signed-in desktop's existing national boundary authority. The shared Rust kernel checks returned identities, polygon validity and connectivity, preserving polygon holes. This prepares an area only: no PDF is rendered, project document saved or artifact published. Complete coordinates are an explicit projection for later local map composition.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<exact-id>",
            "Must match the signed-in desktop's active project.",
        )
        .required(),
        Arg::value(
            "id",
            "<map-id>",
            "Canonical local map identifier: 1..128 ASCII letters, digits, underscore or hyphen.",
        )
        .required(),
        Arg::repeated(
            "sector",
            "<code>",
            "One exact four-digit Rwanda sector code; repeat for up to eight connected sectors.",
        )
        .required(),
        Arg::switch(
            "geometry",
            "Include full area and sector GeoJSON, bounded to 50,000 coordinates; otherwise return identity and bounds only.",
        ),
        DESCRIPTOR_ARG,
    ],
    output: "Project/map identity, sorted sector codes and names, exact area SHA-256, bounds, connected=true, publication=local_only and geometry_included. --geometry includes full MultiPolygon and sector FeatureCollection, without dissolving their shared edges.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
        INVALID,
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    availability: ops::paired_availability,
};
pub fn area(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let id = inputs.require("id")?;
    let codes = inputs.repeated("sector").to_vec();
    let mut request = ds_command_kernel::printing::custom::plan_area(project, id, &codes)
        .map_err(|message| Failure::invalid(INVALID.code, message).remedy(INVALID.remedy))?;
    request["include_geometry"] = json!(inputs.switch("geometry"));
    ops::invoke(
        &ops::paired(inputs.value("desktop-descriptor"))?,
        &AREA_OP,
        request,
        Duration::from_secs(240),
    )
    .map_err(ops::classify_signed_out)
}
