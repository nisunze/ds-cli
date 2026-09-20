//! `ds dsgrid report structures` — the structure list / staking table
//! (program contract 04 §5): one row per placed structure with its
//! description, station, line angle, pole family, drawing number,
//! foundation and the REG structure-rule findings, to CSV or XLSX.
//!
//! Every value comes from the engine's `report_structures` operation; this
//! module writes the file and bounds what it prints.

use std::io::Write;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{StructureReport, StructureReportRow, report_structures};
use serde_json::{Map, Value, json};

use crate::mutation;

const OUT_ARG: Arg = Arg::value(
    "out",
    "<file.csv|file.xlsx>",
    "Write the whole list here (never overwrites). Omitted, rows are printed, bounded by --limit.",
);
const LIMIT_ARG: Arg = Arg::value("limit", "<n>", "Rows printed in the receipt.")
    .default(crate::package::DEFAULT_LIMIT);
const ONLY_FINDINGS_ARG: Arg = Arg::switch(
    "only-findings",
    "Keep only structures that carry at least one finding.",
);

/// The CSV columns, in the order a staking-table reader expects them.
pub const COLUMNS: &[&str] = &[
    "structure_id",
    "number",
    "alignment_id",
    "station_m",
    "line_angle_deg",
    "line_angle_source_deg",
    "structure_type",
    "description",
    "type_description",
    "family",
    "pole_count",
    "material",
    "pole_height_m",
    "pole_class",
    "stays",
    "angle_class_deg",
    "assembly_drawing",
    "foundation_depth_m",
    "foundation_width_m",
    "findings",
    "finding_rules",
    "allowed_families",
    "reasons",
    "x_m",
    "y_m",
];

const OWN: &[Refusal] = &[
    Refusal {
        code: "output_format_unknown",
        when: "--out is neither .csv nor .xlsx",
        remedy: "name a .csv or .xlsx file",
    },
    Refusal {
        code: "rule_source_missing",
        when: "the vendored REG structure-rules standard is not the owner-issued bytes",
        remedy: "rebuild ds from a clean checkout; report the digests in detail",
    },
    Refusal {
        code: "operation_failed",
        when: "the engine could not read the model's alignment geometry",
        remedy: "run `ds dsgrid validate` on the package and read detail.engine",
    },
    Refusal {
        code: "invalid_limit",
        when: "--limit is not a whole number in 1..5000",
        remedy: "pass a limit inside the range, or omit it for the default of 50",
    },
];

/// Shared read-target and file refusals, minus the writing-only ones.
const SHARED: &[Refusal] = &[
    mutation::REFUSALS[0],  // target_required
    mutation::REFUSALS[2],  // output_exists
    mutation::REFUSALS[3],  // output_parent_missing
    mutation::REFUSALS[6],  // local_model_not_found
    mutation::REFUSALS[7],  // local_model_ambiguous
    mutation::REFUSALS[8],  // local_model_store_unavailable
    mutation::REFUSALS[9],  // model_not_found
    mutation::REFUSALS[10], // not_a_dsgrid_package
    mutation::REFUSALS[11], // package_decode_failed
    mutation::REFUSALS[17], // output_unwritable
];

const REFUSALS: &[Refusal; OWN.len() + SHARED.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + SHARED.len()] {
    let mut all = [OWN[0]; OWN.len() + SHARED.len()];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < SHARED.len() {
        all[OWN.len() + shared] = SHARED[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.report.structures",
    path: &["dsgrid", "report", "structures"],
    contract: 1,
    summary: "Structure list: description, angle, pole, foundation, rule findings.",
    purpose: "\
Lists every placed structure of a working copy or package — id and number, \
alignment and station, the line angle from the model's own alignment \
geometry (and the native source's recorded angle beside it), structure \
type, the structure's own description, pole family / material / height / \
class / stays read from the type name, the assembly drawing number when a \
description carries one, REG v7 Table 14 foundation depth and width by \
pole height, and the rule findings the engine evaluates today \
(`structure_type_not_allowed`: a single pole carrying 10° ≤ |angle| < 60°) \
with their rule id, source clause and an empty reason slot. Written whole \
to CSV or XLSX with --out; the receipt carries the counts, the standard's \
digest, the declared assumptions and a bounded page of rows. A proposal: \
PLS-CADD confirms.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        OUT_ARG,
        ONLY_FINDINGS_ARG,
        LIMIT_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "\
`target`, `revision`, `standard` {schema, version, digest}, `assumptions[]` \
(each `assumed: true`), `verification_level: proposal`, `line_angle_definition`, \
`counts` {structures, with_description, on_alignment, findings by id}, \
`rows[]` bounded by --limit with `more.withheld`, and `artifact` {path, \
format, rows, byte_len, sha256} when --out was given.",
    examples: &[
        Example {
            command: "ds dsgrid report structures --model local-… --out structures.csv",
            note: "The whole list to CSV; the receipt says how many carry each finding.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid report structures --model local-… --only-findings --output json",
            note: "Only the structures with a finding, printed.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "staking table",
        "structure list",
        "pole schedule",
        "angle",
        "foundation",
        "csv",
        "xlsx",
        "excel",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::package::parse_limit(inputs.value("limit"))?;
    let out = inputs.value("out").map(str::trim).filter(|v| !v.is_empty());
    let format = match out {
        None => None,
        Some(path) => Some(output_format(path)?),
    };
    if let Some(path) = out {
        crate::apply::validate_output_path(path)?;
    }

    let (target, path) = mutation::read_target_path(inputs)?;
    let bytes = crate::package::read_bytes(&path)?;
    let package = crate::package::decode(&path, &bytes)?;
    let evidence = package
        .assets
        .iter()
        .find(|asset| {
            asset.invariant_leaf
                == ds_grid_exchange::engineering_table_evidence::ENGINEERING_ATTRIBUTE_EVIDENCE_LEAF
        })
        .and_then(|asset| {
            ds_grid_exchange::engineering_table_evidence::decode_engineering_attribute_evidence(
                &asset.bytes,
            )
            .ok()
        });
    let session = ds_grid_engine::GridSession::open(package.snapshot.clone());
    let revision = session.current_revision().revision_id.as_str().to_string();
    let mut report =
        report_structures(session.snapshot(), evidence.as_ref()).map_err(|error| match error {
            ds_grid_engine::StructureRulesError::RuleSourceMissing { expected, found } => {
                Failure::failed(
                    "rule_source_missing",
                    "the REG structure-rules standard is not the owner-issued bytes",
                )
                .remedy("rebuild ds from a clean checkout")
                .detail(json!({ "expected_digest": expected, "found_digest": found }))
            }
            other => Failure::failed("operation_failed", "the structure list could not be built")
                .remedy("run `ds dsgrid validate` on the package")
                .detail(json!({ "engine": other.to_string() })),
        })?;
    if inputs.switch("only-findings") {
        report.rows.retain(|row| !row.findings.is_empty());
    }

    let mut artifact = Value::Null;
    if let (Some(path), Some(format)) = (out, format) {
        let bytes = match format {
            "csv" => csv_bytes(&report),
            _ => xlsx_bytes(&report)?,
        };
        crate::apply::write_new(path, &bytes)?;
        artifact = json!({
            "path": path,
            "format": format,
            "rows": report.rows.len(),
            "columns": COLUMNS,
            "byte_len": bytes.len(),
            "sha256": format!("sha256:{:x}", <sha2::Sha256 as sha2::Digest>::digest(&bytes)),
        });
    }

    let total_rows = report.rows.len();
    let rows: Vec<Value> = report.rows.iter().take(limit).map(row_json).collect();
    let withheld = total_rows.saturating_sub(rows.len());
    Ok(json!({
        "target": target,
        "revision": revision,
        "operation": report.operation_id,
        "engine": ds_grid_engine::ENGINE_VERSION,
        "verification_level": report.verification_level,
        "standard": report.standard,
        "assumptions": report.assumptions,
        "line_angle_definition": report.line_angle_definition,
        "counts": report.counts,
        "only_findings": inputs.switch("only-findings"),
        "rows": rows,
        "more": { "withheld": withheld, "total": total_rows },
        "artifact": artifact,
        "pls_source": target["pls_source"].clone(),
    }))
}

fn output_format(path: &str) -> Result<&'static str, Failure> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".csv") {
        Ok("csv")
    } else if lower.ends_with(".xlsx") {
        Ok("xlsx")
    } else {
        Err(Failure::invalid(
            "output_format_unknown",
            format!("`{path}` is neither .csv nor .xlsx"),
        )
        .remedy("name a .csv or .xlsx file"))
    }
}

fn material(row: &StructureReportRow) -> Option<&'static str> {
    row.family.material.map(|m| match m {
        ds_grid_engine::structure_rules::PoleMaterial::Wooden => "wooden",
        ds_grid_engine::structure_rules::PoleMaterial::Steel => "steel",
        ds_grid_engine::structure_rules::PoleMaterial::Concrete => "concrete",
    })
}

fn family(row: &StructureReportRow) -> &'static str {
    match row.family.kind {
        ds_grid_engine::FamilyKind::SinglePole => "single_pole",
        ds_grid_engine::FamilyKind::HPole => "h_pole",
        ds_grid_engine::FamilyKind::Transformer => "transformer",
        ds_grid_engine::FamilyKind::Tapping => "tapping",
        ds_grid_engine::FamilyKind::Unknown => "unknown",
    }
}

/// One row as the receipt prints it: the columns of the CSV, typed.
fn row_json(row: &StructureReportRow) -> Value {
    let mut map = Map::new();
    for (column, value) in cells(row) {
        map.insert(column.to_string(), value);
    }
    map.insert("findings_detail".into(), json!(row.findings));
    Value::Object(map)
}

fn cells(row: &StructureReportRow) -> Vec<(&'static str, Value)> {
    let joined = |values: Vec<String>| -> Value {
        if values.is_empty() {
            Value::Null
        } else {
            json!(values.join("; "))
        }
    };
    vec![
        ("structure_id", json!(row.structure_id.as_str())),
        ("number", json!(row.engineering_number)),
        (
            "alignment_id",
            json!(row.alignment_id.as_ref().map(|id| id.as_str())),
        ),
        ("station_m", json!(row.station_m)),
        ("line_angle_deg", json!(row.line_angle_deg)),
        ("line_angle_source_deg", json!(row.line_angle_source_deg)),
        ("structure_type", json!(row.structure_type)),
        ("description", json!(row.description)),
        ("type_description", json!(row.type_description)),
        ("family", json!(family(row))),
        ("pole_count", json!(row.family.pole_count)),
        ("material", json!(material(row))),
        ("pole_height_m", json!(row.family.pole_height_m)),
        ("pole_class", json!(row.family.pole_class)),
        ("stays", json!(row.family.stays)),
        ("angle_class_deg", json!(row.family.angle_class_deg)),
        ("assembly_drawing", json!(row.assembly_drawing)),
        ("foundation_depth_m", json!(row.foundation_depth_m)),
        ("foundation_width_m", json!(row.foundation_width_m)),
        (
            "findings",
            joined(row.findings.iter().map(|f| f.finding.clone()).collect()),
        ),
        (
            "finding_rules",
            joined(row.findings.iter().map(|f| f.rule_id.clone()).collect()),
        ),
        (
            "allowed_families",
            joined(
                row.findings
                    .iter()
                    .flat_map(|f| f.allowed_families.iter().cloned())
                    .collect(),
            ),
        ),
        (
            "reasons",
            joined(
                row.findings
                    .iter()
                    .filter_map(|f| f.reason.clone())
                    .collect(),
            ),
        ),
        ("x_m", json!(row.x_m)),
        ("y_m", json!(row.y_m)),
    ]
}

fn csv_field(value: &Value) -> String {
    let text = match value {
        Value::Null => return String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

fn csv_bytes(report: &StructureReport) -> Vec<u8> {
    let mut out = Vec::new();
    // A UTF-8 BOM so Excel reads the degree signs and dashes in descriptions.
    out.extend_from_slice(b"\xEF\xBB\xBF");
    let _ = writeln!(out, "{}", COLUMNS.join(","));
    for row in &report.rows {
        let line: Vec<String> = cells(row).iter().map(|(_, v)| csv_field(v)).collect();
        let _ = writeln!(out, "{}", line.join(","));
    }
    out
}

/// The shared workbook writer: one sheet, the CSV columns as properties, the
/// plan coordinate as the geometry column it always adds.
fn xlsx_bytes(report: &StructureReport) -> Result<Vec<u8>, Failure> {
    let features: Vec<Value> = report
        .rows
        .iter()
        .map(|row| {
            let mut properties = Map::new();
            for (column, value) in cells(row) {
                properties.insert(column.to_string(), value);
            }
            json!({
                "type": "Feature",
                "geometry": { "type": "Point", "coordinates": [row.x_m, row.y_m] },
                "properties": Value::Object(properties),
            })
        })
        .collect();
    let input: ds_io::GeoLayerExportInput = serde_json::from_value(json!({
        "layers": [{
            "name": "structures",
            "geojson": { "type": "FeatureCollection", "features": features },
        }]
    }))
    .map_err(|error| Failure::internal("output_unwritable", error.to_string()))?;
    ds_io::layers_to_xlsx(input).map_err(|error| {
        Failure::failed("output_unwritable", "the workbook could not be built")
            .remedy("write the list as .csv instead")
            .detail(json!({ "detail": error }))
    })
}

pub fn render(data: &Value) -> String {
    let counts = &data["counts"];
    let mut out = format!(
        "structures {} · described {} · on alignment {} · revision {}\n",
        counts["structures"],
        counts["with_description"],
        counts["on_alignment"],
        mutation::short(data["revision"].as_str().unwrap_or("?")),
    );
    if let Some(findings) = counts["findings"].as_object() {
        for (finding, count) in findings {
            out.push_str(&format!("finding    {finding}: {count}\n"));
        }
    }
    for assumption in data["assumptions"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "assumed    {} — {}\n",
            assumption["rule_id"].as_str().unwrap_or("?"),
            assumption["value"].as_str().unwrap_or("?"),
        ));
    }
    for row in data["rows"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  #{:<5} {:<24} {:>9} {:<24} {}  {}\n",
            row["number"].as_str().unwrap_or("—"),
            crate::model::truncate(row["structure_type"].as_str().unwrap_or("?"), 24),
            row["line_angle_deg"]
                .as_f64()
                .map(|a| format!("{a:.4}°"))
                .unwrap_or_else(|| "—".into()),
            crate::model::truncate(row["description"].as_str().unwrap_or("—"), 24),
            row["findings"].as_str().unwrap_or(""),
            row["station_m"]
                .as_f64()
                .map(|s| format!("@ {s:.2} m"))
                .unwrap_or_default(),
        ));
    }
    let withheld = data["more"]["withheld"].as_u64().unwrap_or(0);
    if withheld > 0 {
        out.push_str(&format!(
            "  … {withheld} more rows; raise --limit or read the file\n"
        ));
    }
    if let Some(path) = data["artifact"]["path"].as_str() {
        out.push_str(&format!(
            "written    {} ({} rows, {})\n",
            path,
            data["artifact"]["rows"],
            data["artifact"]["sha256"].as_str().unwrap_or("?")
        ));
    }
    out
}
