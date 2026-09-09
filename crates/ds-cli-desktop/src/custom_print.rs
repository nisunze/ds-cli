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

pub const EXPORT_OP: BridgeOp = BridgeOp {
    operation: "printing.map.export",
    arguments: &["request"],
};
pub const LIST_OP: BridgeOp = BridgeOp {
    operation: "printing.map.list",
    arguments: &["project", "limit"],
};
pub static EXPORT_COMMAND: Command = Command {
    id: "desktop.printing.map.export", path: &["desktop","printing","map","export"], contract:1,
    summary:"Render and retain a custom-sector or district MV PDF locally.",
    purpose:"The matching signed-in desktop captures every project MV source and selected geographic context. The command kernel owns exact scope, paper and canonical output identity. Native Reporter renders one custom-area page or ordered district pages in one PDF. Project Control retains the local artifact and source warnings; only the selected paper is replaced. Completed printouts attach automatically through the existing report-artifact channel; disconnected or failed publication remains pending locally. Cloud rendering never occurs.",
    chapter:Chapter::Reports,effect:Effect::ArtifactWrite,authority:Authority::DesktopUser,execution:Execution::Sync,
    args:&[Arg::value("request","<json>","JSON containing project, id, family (custom-map or mv-map), codes (sector or district codes), paper (A3 or A0), and optional authored layout.").required(),DESCRIPTOR_ARG],
    output:"Canonical filename, verified path/SHA-256/bytes/page count, source inventory, local preview reference and source warnings. Incomplete inputs remain explicitly warned.",
    examples:&[],refusals:&[ops::NOT_PAIRED,ops::AMBIGUOUS,ops::UNREACHABLE,ops::PAIRING_REJECTED,ops::REFUSED,ops::UNSUPPORTED,ops::UNREADABLE,ops::SIGNED_OUT,INVALID],
    reference:Some("docs/reference/desktop.printing.md"),availability:ops::paired_availability,
};
pub static LIST_COMMAND: Command = Command {
    id: "desktop.printing.map.list",
    path: &["desktop", "printing", "map", "list"],
    contract: 1,
    summary: "List the active project's retained local map PDFs.",
    purpose: "Reads the owner/project-fenced local map catalog used by Project Control, preserving canonical filenames and separate paper outputs. Does not render, refresh shared data or upload.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("project", "<id>", "Exact active project.").required(),
        Arg::value("limit", "<count>", "Maximum returned printouts, 1..200.").default("50"),
        DESCRIPTOR_ARG,
    ],
    output: "Project and retained PDF receipts with source warnings and local preview references.",
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
pub fn export(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = inputs.require("request")?;
    let metadata = std::fs::metadata(path).map_err(|e| {
        Failure::invalid(INVALID.code, e.to_string())
            .remedy("Provide one readable map request JSON file")
    })?;
    if !metadata.is_file() || metadata.len() > 800_000 {
        return Err(Failure::invalid(
            INVALID.code,
            "Map request must be a file under 800,000 bytes",
        )
        .remedy("Reduce the authored layout"));
    }
    let bytes = std::fs::read(path).map_err(|e| {
        Failure::invalid(INVALID.code, e.to_string())
            .remedy("Provide one readable map request JSON file")
    })?;
    if bytes.len() > 800_000 {
        return Err(
            Failure::invalid(INVALID.code, "Map request exceeds 800,000 bytes")
                .remedy("Reduce the authored layout"),
        );
    }
    let request: ds_command_kernel::printing::map::Request = serde_json::from_slice(&bytes)
        .map_err(|e| Failure::invalid(INVALID.code, e.to_string()).remedy(INVALID.remedy))?;
    ds_command_kernel::printing::map::validate_request(&request)
        .map_err(|e| Failure::invalid(INVALID.code, e).remedy(INVALID.remedy))?;
    ops::invoke(
        &ops::paired(inputs.value("desktop-descriptor"))?,
        &EXPORT_OP,
        json!({"request":request}),
        Duration::from_secs(2400),
    )
    .map_err(ops::classify_signed_out)
}
pub fn list(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = inputs
        .require("limit")?
        .parse::<usize>()
        .ok()
        .filter(|limit| (1..=200).contains(limit))
        .ok_or_else(|| {
            Failure::invalid(INVALID.code, "limit must be 1..200")
                .remedy("Choose a bounded integer")
        })?;
    ops::invoke(
        &ops::paired(inputs.value("desktop-descriptor"))?,
        &LIST_OP,
        json!({"project":inputs.require("project")?,"limit":limit}),
        Duration::from_secs(30),
    )
    .map_err(ops::classify_signed_out)
}
