//! `ds map design attach-print` — attach one completed cartographic page.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};
use std::{io::Read, path::Path};

use crate::DESCRIPTOR_ARG;

const SCOPES: &[&str] = &["transformer", "combined", "mv"];
const MAP_FAMILIES: &[&str] = &["lv-atlas", "mv-map", "custom-map"];
const ORIENTATIONS: &[&str] = &["portrait", "landscape"];
const PAGE_ROLES: &[&str] = &["sheet", "atlas", "joined"];

pub static COMMAND: Command = Command {
    id: "map.design.attach-print",
    path: &["map", "design", "attach-print"],
    contract: 1,
    summary: "Attach one completed cartographic PDF or image to report delivery.",
    purpose: "--scope mv uses the native selected project to publish a reviewed MV PDF or PNG under mv_data; --lane chooses its account. Other scopes use the paired Desktop. \
Uploads one operator-reviewed cartographic output and attaches its immutable \
digest, LV-atlas/MV-map/custom-map family, layout, paper size, orientation and page role to an individual \
transformer or the combined report. Repeat the command for multiple paper \
sizes or image variants. It never renders a page and never enters report \
compute. A later compounded report includes individual pages beside each \
transformer's files and combined atlas/joined pages at archive root.",
    chapter: Chapter::Design,
    effect: Effect::ArtifactWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "path",
            "<file>",
            "Completed local .pdf, .png, .jpg, or .jpeg file.",
        )
        .required(),
        Arg::value(
            "scope",
            "<scope>",
            "transformer/combined use the paired Desktop; mv publishes natively to mv_data.",
        )
        .default("transformer")
        .choices(SCOPES),
        Arg::value(
            "transformer",
            "<name>",
            "Individual transformer; omit when --scope combined or mv.",
        ),
        Arg::value(
            "map-family",
            "<family>",
            "Cartographic family: LV atlas, MV map set, or custom map.",
        )
        .required()
        .choices(MAP_FAMILIES),
        Arg::value("layout", "<name>", "Exact governed DS layout name.").required(),
        Arg::value(
            "paper-size",
            "<size>",
            "Paper size, e.g. A0, A1, A3, A4 or 841x1189mm.",
        )
        .required(),
        Arg::value("orientation", "<orientation>", "Rendered page orientation.")
            .required()
            .choices(ORIENTATIONS),
        Arg::value(
            "page-role",
            "<role>",
            "Single sheet, atlas output, or joined top-level document.",
        )
        .default("sheet")
        .choices(PAGE_ROLES),
        Arg::value(
            "source-receipt-sha256",
            "<sha256>",
            "Optional digest of the exact DS export receipt used for rendering.",
        ),
        crate::layer::native::LANE_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "Project, target, filename, SHA-256, durable artifact reference, map family, layout, paper size, orientation, and page role.",
    examples: &[
        Example {
            command: "ds map design attach-print --path '/deliverables/TX-1-A3.pdf' --transformer TX-1 --map-family lv-atlas --layout 'LV A3' --paper-size A3 --orientation landscape --page-role atlas --yes --output json",
            note: "Attach an LV atlas PDF; repeat for A0 or PNG variants.",
            runnable: false,
        },
        Example {
            command: "ds map design attach-print --path '/deliverables/project-mv-A1.pdf' --scope combined --map-family mv-map --layout 'Project MV A1' --paper-size A1 --orientation landscape --page-role joined --yes --output json",
            note: "Attach a multipage MV PDF that compounded archives place at top level.",
            runnable: false,
        },
    ],
    refusals: ALL_REFUSALS,
    reference: Some("docs/reference/map.md"),
    search: &[],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let scope = inputs.value("scope").unwrap_or("transformer");
    if scope == "mv" {
        return attach_native_mv(inputs);
    }
    let transformer = inputs.value("transformer");
    if scope == "transformer" && transformer.is_none() {
        return Err(Failure::invalid(
            "transformer_required",
            "--transformer is required when --scope transformer",
        )
        .remedy("pass --transformer <name>, or use --scope combined"));
    }
    let mut arguments = Map::new();
    arguments.insert("path".into(), json!(inputs.require("path")?));
    arguments.insert("scope".into(), json!(scope));
    if let Some(value) = transformer {
        arguments.insert("transformer".into(), json!(value));
    }
    arguments.insert("mapFamily".into(), json!(inputs.require("map-family")?));
    arguments.insert("layoutName".into(), json!(inputs.require("layout")?));
    arguments.insert("paperSize".into(), json!(inputs.require("paper-size")?));
    arguments.insert("orientation".into(), json!(inputs.require("orientation")?));
    arguments.insert(
        "pageRole".into(),
        json!(inputs.value("page-role").unwrap_or("sheet")),
    );
    if let Some(value) = inputs.value("source-receipt-sha256") {
        arguments.insert("sourceReceiptSha256".into(), json!(value));
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_ATTACH_PRINT,
        Value::Object(arguments),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;
    Ok(json!({
        "project": result["project"],
        "scope": result["scope"],
        "transformer": result["transformer"],
        "file_name": result["fileName"],
        "sha256": result["sha256"],
        "artifact": result["gcsPath"],
        "map_family": result["mapFamily"],
        "layout": result["layoutName"],
        "paper_size": result["paperSize"],
        "orientation": result["orientation"],
        "page_role": result["pageRole"],
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "{} attached to {}\n  {} · {} · {} {} · {}\n  sha256 {}\n",
        data["file_name"]
            .as_str()
            .unwrap_or("cartographic artifact"),
        data["transformer"].as_str().unwrap_or("report"),
        data["map_family"].as_str().unwrap_or("map"),
        data["layout"].as_str().unwrap_or("layout"),
        data["paper_size"].as_str().unwrap_or("paper"),
        data["orientation"].as_str().unwrap_or(""),
        data["page_role"].as_str().unwrap_or("sheet"),
        data["sha256"].as_str().unwrap_or("?"),
    )
}

const BASE_REFUSALS: &[Refusal] = &[
    crate::NOT_PAIRED,
    crate::PROJECT_NOT_OPEN,
    crate::AMBIGUOUS,
    crate::UNREACHABLE,
    crate::PAIRING_REJECTED,
    super::DESIGN_REFUSED,
    crate::UNSUPPORTED,
    crate::UNREADABLE,
    crate::SIGNED_OUT,
    Refusal {
        code: "confirmation_required",
        when: "--yes was not given for a project artifact upload",
        remedy: "review the rendered output and re-run with --yes to attach it",
    },
    Refusal {
        code: "transformer_required",
        when: "--scope transformer was used without --transformer",
        remedy: "pass --transformer <name>, or use --scope combined",
    },
];
const ALL_REFUSALS: &[Refusal] = &{
    let native = crate::layer::native::NATIVE_REFUSALS;
    let mut out =
        [BASE_REFUSALS[0]; BASE_REFUSALS.len() + crate::layer::native::NATIVE_REFUSALS.len() + 1];
    let mut n = 0;
    while n < BASE_REFUSALS.len() {
        out[n] = BASE_REFUSALS[n];
        n += 1;
    }
    let mut i = 0;
    while i < native.len() {
        out[n + i] = native[i];
        i += 1;
    }
    out[n + i] = Refusal {
        code: "report_inputs_invalid",
        when: "MV print bytes or metadata are invalid",
        remedy: "Select the reviewed PDF/PNG and its exact paper metadata",
    };
    out
};

fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("report_inputs_invalid", e.to_string())
        .remedy("Select a reviewed PDF or PNG and its exact paper metadata")
}
fn attach_native_mv(i: &Inputs) -> Result<Value, Failure> {
    if i.require("map-family")? != "mv-map" || i.value("page-role").unwrap_or("sheet") != "sheet" {
        return Err(invalid(
            "Native MV attachment requires mv-map and a single sheet",
        ));
    }
    let path = Path::new(i.require("path")?);
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(invalid)?
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    let command = ds_cli_auth::ReportArtifactCommand {
        file_name: path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| invalid("file name is not UTF-8"))?
            .into(),
        bytes,
        layout: i.require("layout")?.into(),
        paper: i.require("paper-size")?.into(),
        orientation: i.require("orientation")?.into(),
        source_receipt: i.value("source-receipt-sha256").unwrap_or("").into(),
    };
    let result = ds_cli_auth::report_artifact(i.require("lane")?, &command)?;
    let result = result.into_result();
    Ok(
        json!({"project":result["project_id"],"scope":"mv","transformer":"mv_data","file_name":result["file_name"],"sha256":result["sha256"],"artifact":result["gcs_path"],"map_family":result["map_family"],"layout":i.require("layout")?,"paper_size":i.require("paper-size")?,"orientation":i.require("orientation")?,"page_role":"sheet"}),
    )
}
