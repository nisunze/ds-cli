//! `ds map design attach-print` — attach one completed QGIS page.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const SCOPES: &[&str] = &["transformer", "combined"];
const MAP_FAMILIES: &[&str] = &["lv-atlas", "mv-map", "custom-map"];
const ORIENTATIONS: &[&str] = &["portrait", "landscape"];
const PAGE_ROLES: &[&str] = &["sheet", "atlas", "joined"];

pub static COMMAND: Command = Command {
    id: "map.design.attach-print",
    path: &["map", "design", "attach-print"],
    contract: 1,
    summary: "Attach one completed QGIS PDF or image to report delivery.",
    purpose: "\
Uploads one operator-reviewed QGIS/PyQGIS output and attaches its immutable \
digest, LV-atlas/MV-map/custom-map family, layout, paper size, orientation and page role to an individual \
transformer or the combined report. Repeat the command for multiple paper \
sizes or image variants. It never renders a page and never enters report \
compute. A later compounded report includes individual pages beside each \
transformer's files and combined atlas/joined pages at archive root.",
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
            "Attach to one transformer or to combined delivery.",
        )
        .default("transformer")
        .choices(SCOPES),
        Arg::value(
            "transformer",
            "<name>",
            "Individual transformer; omit when --scope combined.",
        ),
        Arg::value(
            "map-family",
            "<family>",
            "Cartographic family: LV atlas, MV map set, or custom map.",
        )
        .required()
        .choices(MAP_FAMILIES),
        Arg::value("layout", "<name>", "Exact approved QGIS layout name.").required(),
        Arg::value(
            "paper-size",
            "<size>",
            "QGIS paper size, e.g. A0, A1, A3, A4 or 841x1189mm.",
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
            "Optional digest of the exact DS export receipt rendered by QGIS.",
        ),
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
    refusals: &[
        crate::NOT_PAIRED,
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
            remedy: "review the QGIS output and re-run with --yes to attach it",
        },
        Refusal {
            code: "transformer_required",
            when: "--scope transformer was used without --transformer",
            remedy: "pass --transformer <name>, or use --scope combined",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let scope = inputs.value("scope").unwrap_or("transformer");
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
        data["file_name"].as_str().unwrap_or("QGIS artifact"),
        data["transformer"].as_str().unwrap_or("report"),
        data["map_family"].as_str().unwrap_or("map"),
        data["layout"].as_str().unwrap_or("layout"),
        data["paper_size"].as_str().unwrap_or("paper"),
        data["orientation"].as_str().unwrap_or(""),
        data["page_role"].as_str().unwrap_or("sheet"),
        data["sha256"].as_str().unwrap_or("?"),
    )
}
